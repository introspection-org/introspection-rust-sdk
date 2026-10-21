//! Reading a resource's R3 proposal ([draft-hardt-aauth-r3]): the document a
//! resource token's `r3_uri` names, whose bytes hash to its `r3_s256`. A
//! Person Server shows its `display.summary` to the approver, and an Access
//! Server hands it to its policy.
//!
//! [draft-hardt-aauth-r3]: https://github.com/dickhardt/AAuth

use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value};

use crate::discovery::Fetch;
use crate::httpsig::{self, Outgoing, SignatureKey};
use crate::identifier::same_origin;
use crate::keys::SigningKey;

/// Who reads a proposal: a server signing its `GET` under `sig=jwks_uri`
/// with its own key and metadata document.
#[derive(Clone, Copy)]
pub struct ProposalReader<'a> {
    /// The reader's server identifier, the `Signature-Key` `id`.
    pub issuer: &'a str,
    /// Its metadata document: `aauth-person.json` or `aauth-access.json`.
    pub dwk: &'a str,
    /// Its signing key, published under `dwk`.
    pub key: &'a SigningKey,
    /// How the proposal is fetched.
    pub fetch: &'a dyn Fetch,
    /// Seconds since the epoch.
    pub now: i64,
}

/// The `display.summary` of the proposal a verified resource token's claims
/// name; `None` when they name none, the proposal is not on the resource's
/// own origin, cannot be read, or does not hash to `r3_s256`.
pub async fn read_summary(
    resource_token: &Map<String, Value>,
    reader: &ProposalReader<'_>,
) -> Option<String> {
    let text = |name: &str| resource_token.get(name).and_then(Value::as_str);
    let (uri, expected, resource) = (text("r3_uri")?, text("r3_s256")?, text("iss")?);
    // Fetched signed as the reader, so only from the resource itself.
    if !uri.starts_with("https://") || !same_origin(resource, uri) {
        return None;
    }
    let signed = httpsig::sign(
        reader.key,
        &SignatureKey::JwksUri {
            id: reader.issuer.to_string(),
            dwk: reader.dwk.to_string(),
            kid: reader.key.thumbprint().to_string(),
        },
        &Outgoing {
            method: "GET",
            url: uri,
            content_type: None,
            body: None,
            created: reader.now,
        },
    )
    .ok()?;
    let mut headers = HeaderMap::new();
    for (name, value) in signed {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).ok()?,
            HeaderValue::from_str(&value).ok()?,
        );
    }
    let fetched = reader.fetch.get(uri, headers).await.ok()?;
    if !fetched.status.is_success() || crate::s256(&fetched.body) != expected {
        return None;
    }
    let document: Value = serde_json::from_slice(&fetched.body).ok()?;
    document
        .get("display")?
        .get("summary")?
        .as_str()
        .map(str::to_string)
}
