//! RFC 9421 HTTP Message Signatures under the AAuth profile (AAuth §HTTP
//! Message Signatures), with the signer's key named in `Signature-Key`
//! (draft-hardt-httpbis-signature-key).
//!
//! [`sign`] covers `@method`, `@authority`, `@path`, `@query` when there is
//! one, `signature-key`, and for a request with a body `content-digest` and
//! `content-type`. [`verify`] checks a signature and hands back the key that
//! made it; a token in a `jwt` scheme is decoded but NOT verified — that is
//! [`crate::token::verify_token`]'s job, against its issuer's keys.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use http::HeaderMap;
use sha2::{Digest, Sha256};
use url::Url;

use crate::discovery::KeyResolver;
use crate::error::{Error, SignatureError};
use crate::identifier::authority;
use crate::jwk::{Algorithm, Jwk, KeyProblem};
use crate::jwt::{self, Decoded};
use crate::keys::SigningKey;
use crate::sf::{self, Bare, Item, Member};

/// The label this crate signs under.
pub const LABEL: &str = "sig";

/// Request headers a signer sets, which a proxy signing on someone's behalf
/// should strip from what it was handed.
pub const SIGNATURE_HEADERS: &[&str] = &[
    "signature",
    "signature-input",
    "signature-key",
    "content-digest",
];

/// How the verifier finds the signer's key: the `Signature-Key` scheme.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureKey {
    /// A JWT whose `cnf.jwk` is the key: agent, person and auth tokens.
    Jwt(String),
    /// A server's published key: `{id}/.well-known/{dwk}` names a `jwks_uri`.
    JwksUri {
        /// The server's identifier.
        id: String,
        /// Its metadata document's name.
        dwk: String,
        /// The key's `kid` in its JWKS.
        kid: String,
    },
    /// The key itself, inline: a pseudonymous agent.
    Hwk,
}

impl SignatureKey {
    fn header_value(&self, key: &SigningKey) -> Result<String, Error> {
        let quoted = |v: &str| {
            if v.contains(['"', '\\']) || !v.is_ascii() {
                Err(Error::Invalid(
                    "value cannot travel in Signature-Key".into(),
                ))
            } else {
                Ok(sf::quote(v))
            }
        };
        Ok(match self {
            SignatureKey::Jwt(token) => format!("{LABEL}=jwt;jwt={}", quoted(token)?),
            SignatureKey::JwksUri { id, dwk, kid } => format!(
                "{LABEL}=jwks_uri;id={};dwk={};kid={}",
                quoted(id)?,
                quoted(dwk)?,
                quoted(kid)?
            ),
            SignatureKey::Hwk => {
                let jwk = key.public_jwk();
                format!(
                    "{LABEL}=hwk;kty={};crv={};x={};alg={}",
                    quoted(&jwk.kty)?,
                    quoted(jwk.crv.as_deref().unwrap_or_default())?,
                    quoted(jwk.x.as_deref().unwrap_or_default())?,
                    quoted(jwk.alg.as_deref().unwrap_or_default())?
                )
            }
        })
    }
}

/// A request to sign, as its recipient will see it.
#[derive(Clone, Copy, Debug)]
pub struct Outgoing<'a> {
    /// The method, in any case.
    pub method: &'a str,
    /// Absolute: its authority, path and query are covered.
    pub url: &'a str,
    /// Defaults to `application/json` for a request with a body.
    pub content_type: Option<&'a str>,
    /// The body exactly as it will be sent, when there is one.
    pub body: Option<&'a [u8]>,
    /// Seconds since the epoch.
    pub created: i64,
}

/// `sha-256=:…:`, the RFC 9530 `Content-Digest` of `body`.
pub fn content_digest(body: &[u8]) -> String {
    format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(body)))
}

/// The headers that sign `request` with `key`, presenting `signature_key`.
pub fn sign(
    key: &SigningKey,
    signature_key: &SignatureKey,
    request: &Outgoing<'_>,
) -> Result<Vec<(String, String)>, Error> {
    let url = Url::parse(request.url).map_err(|e| Error::Url(format!("{}: {e}", request.url)))?;
    let authority = authority(&url).ok_or_else(|| Error::Url("url has no host".into()))?;
    let signature_key = signature_key.header_value(key)?;

    let mut covered: Vec<(&str, String)> = vec![
        ("@method", request.method.to_ascii_uppercase()),
        ("@authority", authority),
        ("@path", url.path().to_string()),
    ];
    if let Some(query) = url.query() {
        covered.push(("@query", format!("?{query}")));
    }
    covered.push(("signature-key", signature_key.clone()));
    let mut headers = Vec::new();
    if let Some(body) = request.body.filter(|b| !b.is_empty()) {
        let digest = content_digest(body);
        let content_type = request
            .content_type
            .unwrap_or("application/json")
            .to_string();
        covered.push(("content-digest", digest.clone()));
        covered.push(("content-type", content_type.clone()));
        headers.push(("content-digest".to_string(), digest));
        headers.push(("content-type".to_string(), content_type));
    }

    let components: Vec<String> = covered
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect();
    let params = format!("({});created={}", components.join(" "), request.created);
    let mut base: Vec<String> = covered
        .iter()
        .map(|(name, value)| format!("\"{name}\": {value}"))
        .collect();
    base.push(format!("\"@signature-params\": {params}"));
    let signature = key.sign(base.join("\n").as_bytes());

    headers.push(("signature-key".to_string(), signature_key));
    headers.push(("signature-input".to_string(), format!("{LABEL}={params}")));
    headers.push((
        "signature".to_string(),
        format!("{LABEL}=:{}:", STANDARD.encode(signature)),
    ));
    Ok(headers)
}

/// The headers [`sign`] returns, as a [`HeaderMap`] to send.
pub fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, Error> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.insert(
            http::HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| Error::Invalid(e.to_string()))?,
            http::HeaderValue::from_str(value).map_err(|e| Error::Invalid(e.to_string()))?,
        );
    }
    Ok(map)
}

/// `sign` presenting `token` under the `jwt` scheme: how an agent signs.
pub fn sign_jwt(
    key: &SigningKey,
    token: &str,
    request: &Outgoing<'_>,
) -> Result<Vec<(String, String)>, Error> {
    sign(key, &SignatureKey::Jwt(token.to_string()), request)
}

/// A signed request as it arrived.
#[derive(Clone, Copy, Debug)]
pub struct Incoming<'a> {
    /// The method, in any case.
    pub method: &'a str,
    /// The authority the signer addressed: this server's own, whatever proxy
    /// or rewrite delivered the request (AAuth §Canonical Authority).
    pub authority: &'a str,
    /// The path, without its query.
    pub path: &'a str,
    /// Without its leading `?`.
    pub query: Option<&'a str>,
    /// Every request header.
    pub headers: &'a HeaderMap,
    /// The body as received, when there is one.
    pub body: Option<&'a [u8]>,
}

/// How [`verify`] judges a signature. The default is now, a 60-second
/// window either way, and Ed25519 or ES256.
#[derive(Clone, Debug)]
pub struct VerifyOptions {
    /// Seconds since the epoch.
    pub now: i64,
    /// How far `created` may be from `now`, either way.
    pub max_clock_skew: i64,
    /// A request with a body must cover `content-digest`: required of a
    /// Person Server or Access Server by the profile.
    pub require_content_digest: bool,
    /// The algorithms accepted; any other is `unsupported_algorithm`.
    pub algorithms: Vec<Algorithm>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            now: crate::now_unix(),
            max_clock_skew: 60,
            require_content_digest: false,
            algorithms: vec![Algorithm::Ed25519, Algorithm::Es256],
        }
    }
}

/// What signed a request.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Signer {
    /// The token the signer presented, decoded and not yet verified.
    Jwt {
        /// The token as presented.
        raw: String,
        /// The token, split and decoded.
        decoded: Box<Decoded>,
    },
    /// A server's published key.
    JwksUri {
        /// The server's identifier.
        id: String,
        /// Its metadata document's name.
        dwk: String,
        /// The key's `kid` in its JWKS.
        kid: String,
    },
    /// A key carried inline.
    Hwk,
}

/// A signature that verified.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Verified {
    /// The label it was made under, usually `sig`.
    pub label: String,
    /// How the signer's key was named.
    pub signer: Signer,
    /// The key that made it.
    pub public_key: Jwk,
    /// RFC 7638 thumbprint of `public_key`.
    pub thumbprint: String,
    /// When the signer says it signed, in seconds since the epoch.
    pub created: i64,
}

impl Verified {
    /// The presented JWT, for the `jwt` scheme.
    pub fn jwt(&self) -> Option<&str> {
        match &self.signer {
            Signer::Jwt { raw, .. } => Some(raw),
            _ => None,
        }
    }
}

const REQUIRED_COMPONENTS: &[&str] = &["@method", "@authority", "@path", "signature-key"];

fn invalid_input(detail: impl Into<String>) -> SignatureError {
    SignatureError::new("invalid_input", detail)
}

/// A header's values joined as RFC 9421 §2.1 prescribes. A value that is
/// not visible ASCII makes the whole header unreadable, rather than
/// silently dropping that value from what is covered.
fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    let values: Option<Vec<&str>> = headers
        .get_all(name)
        .iter()
        .map(|v| v.to_str().ok())
        .collect();
    values.filter(|v| !v.is_empty()).map(|v| v.join(", "))
}

fn text_params(item: &Item) -> Result<Vec<(String, String)>, SignatureError> {
    item.params
        .iter()
        .map(|(k, v)| {
            v.as_text()
                .map(|t| (k.clone(), t.to_string()))
                .ok_or_else(|| {
                    SignatureError::new(
                        "invalid_key",
                        format!("Signature-Key {k} must be a String or Token"),
                    )
                })
        })
        .collect()
}

/// Verify `request`'s signature. `resolver` is consulted only for the
/// `jwks_uri` scheme.
pub async fn verify(
    request: &Incoming<'_>,
    options: &VerifyOptions,
    resolver: &dyn KeyResolver,
) -> Result<Verified, SignatureError> {
    let invalid = |detail: &str| SignatureError::new("invalid_signature", detail);
    let raw_key =
        header(request.headers, "signature-key").ok_or_else(|| invalid("missing Signature-Key"))?;
    let key_dict = sf::parse_dictionary(&raw_key)
        .map_err(|e| SignatureError::new("invalid_key", format!("Signature-Key: {e}")))?;
    let [(label, Member::Item(key_item))] = key_dict.as_slice() else {
        return Err(SignatureError::new(
            "invalid_key",
            "Signature-Key must have exactly one member, an item",
        ));
    };
    let Bare::Token(scheme) = &key_item.bare else {
        return Err(SignatureError::new(
            "invalid_key",
            "Signature-Key scheme must be a token",
        ));
    };
    let params = text_params(key_item)?;
    let param = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    let raw_input = header(request.headers, "signature-input")
        .ok_or_else(|| invalid("missing Signature-Input"))?;
    let inputs =
        sf::parse_dictionary(&raw_input).map_err(|e| invalid(&format!("Signature-Input: {e}")))?;
    let Some((_, Member::InnerList(component_items, sig_params))) =
        inputs.iter().find(|(l, _)| l == label)
    else {
        return Err(invalid("no Signature-Input for the Signature-Key label"));
    };
    let mut components = Vec::new();
    for item in component_items {
        match (&item.bare, item.params.is_empty()) {
            (Bare::String(name), true) if components.contains(name) => {
                return Err(invalid_input(format!("{name} is covered twice")))
            }
            (Bare::String(name), true) => components.push(name.clone()),
            _ => return Err(invalid_input("only plain covered components are supported")),
        }
    }
    if let Some(missing) = REQUIRED_COMPONENTS
        .iter()
        .find(|c| !components.iter().any(|n| n == *c))
    {
        return Err(invalid_input(format!("{missing} must be covered")));
    }
    let created = sig_params
        .iter()
        .find_map(|(k, v)| match (k.as_str(), v) {
            ("created", Bare::Integer(i)) => Some(*i),
            _ => None,
        })
        .ok_or_else(|| invalid("Signature-Input has no created"))?;
    if created > options.now + options.max_clock_skew {
        return Err(SignatureError::new(
            "clock_skew",
            format!(
                "created is {}s ahead of this verifier's clock",
                created - options.now
            ),
        ));
    }
    if options.now - created > options.max_clock_skew {
        return Err(invalid("signature is stale"));
    }
    if let Some(expires) = sig_params.iter().find_map(|(k, v)| match (k.as_str(), v) {
        ("expires", Bare::Integer(i)) => Some(*i),
        _ => None,
    }) {
        if expires < options.now {
            return Err(invalid("signature has expired"));
        }
    }

    let (signer, public_key) = match scheme.as_str() {
        "jwt" => {
            let raw = param("jwt")
                .ok_or_else(|| SignatureError::new("invalid_key", "jwt scheme missing jwt"))?;
            let decoded =
                jwt::decode(&raw).map_err(|e| SignatureError::new("invalid_jwt", e.to_string()))?;
            let cnf = decoded
                .claims
                .get("cnf")
                .and_then(|c| c.get("jwk"))
                .cloned()
                .ok_or_else(|| SignatureError::new("invalid_jwt", "JWT missing cnf.jwk"))?;
            if !decoded.claims.get("exp").is_some_and(|e| e.is_number()) {
                return Err(SignatureError::new("invalid_jwt", "JWT missing exp"));
            }
            let jwk: Jwk = serde_json::from_value(cnf)
                .map_err(|e| SignatureError::new("invalid_key", format!("cnf.jwk: {e}")))?;
            (
                Signer::Jwt {
                    raw,
                    decoded: Box::new(decoded),
                },
                jwk,
            )
        }
        "jwks_uri" => {
            let (Some(id), Some(dwk), Some(kid)) = (param("id"), param("dwk"), param("kid")) else {
                return Err(SignatureError::new(
                    "invalid_key",
                    "jwks_uri scheme needs id, dwk and kid",
                ));
            };
            // Checked before anything is fetched: both come from whoever sent
            // the request.
            if !crate::identifier::is_server_identifier(&id)
                || !crate::metadata::dwk::is_known(&dwk)
            {
                return Err(SignatureError::new(
                    "invalid_key",
                    "jwks_uri needs a server identifier and a known dwk",
                ));
            }
            let jwks = resolver
                .jwks(&id, &dwk)
                .await
                .map_err(|e| SignatureError::new("unknown_key", e.to_string()))?;
            let jwk = jwks
                .keys
                .into_iter()
                .find(|k| k.kid.as_deref() == Some(kid.as_str()))
                .ok_or_else(|| {
                    SignatureError::new("unknown_key", format!("no key {kid:?} for {id}"))
                })?;
            (Signer::JwksUri { id, dwk, kid }, jwk)
        }
        "hwk" => {
            if param("kid").is_some() {
                return Err(SignatureError::new(
                    "invalid_key",
                    "hwk must not carry a kid",
                ));
            }
            let jwk = Jwk {
                kty: param("kty").unwrap_or_default(),
                crv: param("crv"),
                x: param("x"),
                y: param("y"),
                n: param("n"),
                e: param("e"),
                alg: param("alg"),
                ..Default::default()
            };
            (Signer::Hwk, jwk)
        }
        other => {
            return Err(SignatureError::new(
                "unsupported_scheme",
                format!("unsupported Signature-Key scheme {other}"),
            ))
        }
    };
    let algorithm = public_key.algorithm().map_err(|problem| match problem {
        KeyProblem::Invalid(d) => SignatureError::new("invalid_key", d),
        KeyProblem::Unsupported(d) => SignatureError::new("unsupported_algorithm", d),
    })?;
    if !options.algorithms.contains(&algorithm) {
        return Err(SignatureError::new(
            "unsupported_algorithm",
            format!("{} is not accepted here", algorithm.as_str()),
        ));
    }
    // An `alg` parameter, when the signer sends one, must name the key's
    // algorithm in RFC 9421's registry; the key's own `alg` decides either way.
    if let Some(declared) = sig_params
        .iter()
        .find_map(|(k, v)| (k == "alg").then_some(v))
    {
        let expected = match algorithm {
            Algorithm::Ed25519 => "ed25519",
            Algorithm::Es256 => "ecdsa-p256-sha256",
        };
        if declared.as_text() != Some(expected) {
            return Err(invalid("the alg parameter disagrees with the key"));
        }
    }

    let body = request.body.filter(|b| !b.is_empty());
    let covers_digest = components.iter().any(|c| c == "content-digest");
    if body.is_some() && options.require_content_digest && !covers_digest {
        return Err(invalid_input(
            "content-digest must be covered on a request with a body",
        ));
    }
    if covers_digest {
        let sent = header(request.headers, "content-digest")
            .ok_or_else(|| invalid_input("content-digest is covered but absent"))?;
        if sent != content_digest(body.unwrap_or_default()) {
            return Err(invalid_input("content-digest does not match the body"));
        }
    }

    let mut base = Vec::new();
    for component in &components {
        let value = match component.as_str() {
            "@method" => request.method.to_ascii_uppercase(),
            "@authority" => request.authority.to_ascii_lowercase(),
            "@path" => request.path.to_string(),
            "@query" => format!("?{}", request.query.unwrap_or_default()),
            "@scheme" => "https".to_string(),
            "@target-uri" => match request.query {
                Some(q) => format!("https://{}{}?{q}", request.authority, request.path),
                None => format!("https://{}{}", request.authority, request.path),
            },
            "@request-target" => match request.query {
                Some(q) => format!("{}?{q}", request.path),
                None => request.path.to_string(),
            },
            name if name.starts_with('@') => {
                return Err(invalid_input(format!(
                    "unsupported derived component {name}"
                )))
            }
            name => header(request.headers, name)
                .map(|v| v.trim().to_string())
                .ok_or_else(|| invalid_input(format!("covered header {name} is absent")))?,
        };
        base.push(format!("\"{component}\": {value}"));
    }
    base.push(format!(
        "\"@signature-params\": {}",
        sf::serialize_inner_list(component_items, sig_params)
    ));

    let raw_signature =
        header(request.headers, "signature").ok_or_else(|| invalid("missing Signature"))?;
    let signatures =
        sf::parse_dictionary(&raw_signature).map_err(|e| invalid(&format!("Signature: {e}")))?;
    let Some((
        _,
        Member::Item(Item {
            bare: Bare::Bytes(signature),
            ..
        }),
    )) = signatures.iter().find(|(l, _)| l == label)
    else {
        return Err(invalid("no Signature for the Signature-Key label"));
    };
    if !public_key.verify(algorithm, base.join("\n").as_bytes(), signature) {
        return Err(invalid("signature does not verify"));
    }
    let thumbprint = public_key
        .thumbprint()
        .map_err(|e| SignatureError::new("invalid_key", e.to_string()))?;
    Ok(Verified {
        label: label.clone(),
        signer,
        public_key,
        thumbprint,
        created,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::NoKeys;
    use http::{HeaderName, HeaderValue};
    use serde_json::json;

    fn agent_token(session: &SigningKey) -> String {
        let header = json!({ "typ": "aa-agent+jwt", "kid": "ap-1" });
        jwt::encode(
            &SigningKey::generate(),
            header.as_object().unwrap().clone(),
            &json!({
                "iss": "https://agent.example",
                "dwk": "aauth-agent.json",
                "sub": "aauth:travel@agent.example",
                "iat": 1_791_000_000,
                "exp": 1_791_003_600,
                "cnf": { "jwk": session.public_jwk() },
            }),
        )
    }

    fn header_map(set: &[(String, String)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (k, v) in set {
            headers.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        headers
    }

    fn get<'a>(set: &'a [(String, String)], name: &str) -> &'a str {
        &set.iter().find(|(k, _)| k == name).unwrap().1
    }

    #[test]
    fn a_signed_post_covers_its_body_in_the_signature_base() {
        let key = SigningKey::generate();
        let token = agent_token(&key);
        let body = br#"{"offer_id":"ht-syd-qt"}"#;
        let set = sign_jwt(
            &key,
            &token,
            &Outgoing {
                method: "post",
                url: "https://API.booking.example/v1/reserve?x=1",
                content_type: None,
                body: Some(body),
                created: 1_791_000_100,
            },
        )
        .unwrap();
        let params = r#"("@method" "@authority" "@path" "@query" "signature-key" "content-digest" "content-type");created=1791000100"#;
        assert_eq!(get(&set, "signature-input"), format!("sig={params}"));
        let digest = content_digest(body);
        assert_eq!(get(&set, "content-digest"), digest);
        let base = format!(
            "\"@method\": POST\n\"@authority\": api.booking.example\n\"@path\": /v1/reserve\n\"@query\": ?x=1\n\"signature-key\": sig=jwt;jwt=\"{token}\"\n\"content-digest\": {digest}\n\"content-type\": application/json\n\"@signature-params\": {params}"
        );
        let raw = get(&set, "signature")
            .strip_prefix("sig=:")
            .unwrap()
            .strip_suffix(':')
            .unwrap();
        let signature = STANDARD.decode(raw).unwrap();
        assert!(key
            .public_jwk()
            .verify(Algorithm::Ed25519, base.as_bytes(), &signature));
    }

    async fn round_trip(
        set: &[(String, String)],
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<&[u8]>,
        now: i64,
    ) -> Result<Verified, SignatureError> {
        let headers = header_map(set);
        verify(
            &Incoming {
                method,
                authority: "api.booking.example",
                path,
                query,
                headers: &headers,
                body,
            },
            &VerifyOptions {
                now,
                require_content_digest: true,
                ..Default::default()
            },
            &NoKeys,
        )
        .await
    }

    #[tokio::test]
    async fn what_is_signed_here_verifies_here() {
        let key = SigningKey::generate();
        let token = agent_token(&key);
        let body = br#"{"a":1}"#;
        let set = sign_jwt(
            &key,
            &token,
            &Outgoing {
                method: "POST",
                url: "https://api.booking.example/v1/reserve?x=1",
                content_type: None,
                body: Some(body),
                created: 1_000,
            },
        )
        .unwrap();
        let verified = round_trip(&set, "POST", "/v1/reserve", Some("x=1"), Some(body), 1_010)
            .await
            .unwrap();
        assert_eq!(verified.thumbprint, key.thumbprint());
        assert_eq!(verified.jwt(), Some(token.as_str()));

        let tampered = round_trip(
            &set,
            "POST",
            "/v1/reserve",
            Some("x=1"),
            Some(br#"{"a":2}"#),
            1_010,
        )
        .await;
        assert_eq!(tampered.unwrap_err().code, "invalid_input");
        let moved = round_trip(&set, "POST", "/v1/cancel", Some("x=1"), Some(body), 1_010).await;
        assert_eq!(moved.unwrap_err().code, "invalid_signature");
        let stale = round_trip(&set, "POST", "/v1/reserve", Some("x=1"), Some(body), 2_000).await;
        assert_eq!(stale.unwrap_err().code, "invalid_signature");
        let early = round_trip(&set, "POST", "/v1/reserve", Some("x=1"), Some(body), 0).await;
        assert_eq!(early.unwrap_err().code, "clock_skew");
    }

    #[tokio::test]
    async fn an_unsigned_body_is_refused_where_the_profile_requires_it() {
        let key = SigningKey::generate();
        let set = sign_jwt(
            &key,
            &agent_token(&key),
            &Outgoing {
                method: "POST",
                url: "https://api.booking.example/token",
                content_type: None,
                body: None,
                created: 1_000,
            },
        )
        .unwrap();
        let refused = round_trip(&set, "POST", "/token", None, Some(b"{}"), 1_000).await;
        assert_eq!(refused.unwrap_err().code, "invalid_input");
    }

    #[tokio::test]
    async fn an_hwk_signature_carries_its_own_key() {
        let key = SigningKey::generate();
        let set = sign(
            &key,
            &SignatureKey::Hwk,
            &Outgoing {
                method: "GET",
                url: "https://api.booking.example/v1/search",
                content_type: None,
                body: None,
                created: 1_000,
            },
        )
        .unwrap();
        let verified = round_trip(&set, "GET", "/v1/search", None, None, 1_000)
            .await
            .unwrap();
        assert!(matches!(verified.signer, Signer::Hwk));
        assert_eq!(verified.thumbprint, key.thumbprint());
    }

    #[test]
    fn a_token_that_would_break_the_header_is_refused() {
        let key = SigningKey::generate();
        let out = Outgoing {
            method: "GET",
            url: "https://a.example/",
            content_type: None,
            body: None,
            created: 0,
        };
        assert!(sign_jwt(&key, "a\"b", &out).is_err());
    }

    /// A resolver that records every lookup, so a test can prove none happened.
    #[derive(Default)]
    struct Recording(std::sync::Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl KeyResolver for Recording {
        async fn jwks(&self, issuer: &str, dwk: &str) -> crate::Result<crate::jwk::Jwks> {
            self.0.lock().unwrap().push(format!("{issuer} {dwk}"));
            Err(crate::Error::Discovery("none".into()))
        }
    }

    #[tokio::test]
    async fn a_jwks_uri_naming_an_internal_address_or_path_is_never_fetched() {
        let key = SigningKey::generate();
        for (id, dwk) in [
            ("http://127.0.0.1:8080", "aauth-agent.json"),
            ("https://10.0.0.1", "aauth-agent.json"),
            ("https://ps.example", "../internal/admin?x=1"),
        ] {
            let set = sign(
                &key,
                &SignatureKey::JwksUri {
                    id: id.into(),
                    dwk: dwk.into(),
                    kid: "k".into(),
                },
                &Outgoing {
                    method: "GET",
                    url: "https://api.booking.example/v1/search",
                    content_type: None,
                    body: None,
                    created: 1_000,
                },
            )
            .unwrap();
            let headers = super::header_map(&set).unwrap();
            let resolver = Recording::default();
            let refused = verify(
                &Incoming {
                    method: "GET",
                    authority: "api.booking.example",
                    path: "/v1/search",
                    query: None,
                    headers: &headers,
                    body: None,
                },
                &VerifyOptions {
                    now: 1_000,
                    ..Default::default()
                },
                &resolver,
            )
            .await
            .unwrap_err();
            assert_eq!(refused.code, "invalid_key", "{id} {dwk}");
            assert!(
                resolver.0.lock().unwrap().is_empty(),
                "{id} {dwk} was fetched"
            );
        }
    }

    #[tokio::test]
    async fn a_component_covered_twice_is_refused() {
        let key = SigningKey::generate();
        let mut set = sign_jwt(
            &key,
            &agent_token(&key),
            &Outgoing {
                method: "GET",
                url: "https://api.booking.example/v1/search",
                content_type: None,
                body: None,
                created: 1_000,
            },
        )
        .unwrap();
        for (name, value) in set.iter_mut() {
            if name == "signature-input" {
                *value = value.replace("\"@path\"", "\"@path\" \"@path\"");
            }
        }
        let refused = round_trip(&set, "GET", "/v1/search", None, None, 1_000)
            .await
            .unwrap_err();
        assert_eq!(refused.code, "invalid_input");
    }
}
