//! Key discovery: `{iss}/.well-known/{dwk}` names a `jwks_uri` (or carries
//! `jwks`), and the key a token or signature names is found there.
//!
//! Discovery runs over a [`Fetch`], so the caller decides which HTTP client
//! (and which SSRF guard) reaches the network, and tests need none.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::error::{Error, Result};
use crate::jwk::Jwks;
use crate::metadata::{metadata_url, KeyedMetadata};

/// The most a fetched document may weigh. Metadata and key sets are small;
/// anything larger is a peer misbehaving.
pub const MAX_DOCUMENT_BYTES: usize = 1 << 20;

/// Finds an issuer's signing keys.
#[async_trait]
pub trait KeyResolver: Send + Sync {
    /// The keys `issuer` publishes through its `dwk` metadata document.
    async fn jwks(&self, issuer: &str, dwk: &str) -> Result<Jwks>;
}

/// Resolves nothing: for verifying signatures whose scheme carries its key.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoKeys;

#[async_trait]
impl KeyResolver for NoKeys {
    async fn jwks(&self, _issuer: &str, _dwk: &str) -> Result<Jwks> {
        Err(Error::Discovery("this verifier looks up no keys".into()))
    }
}

/// A response to a [`Fetch`].
#[derive(Clone, Debug)]
pub struct Fetched {
    /// The response status.
    pub status: StatusCode,
    /// The response headers.
    pub headers: HeaderMap,
    /// The response body, at most [`MAX_DOCUMENT_BYTES`].
    pub body: Bytes,
}

/// An HTTP GET.
#[async_trait]
pub trait Fetch: Send + Sync {
    /// GET `url` with `headers`. An error is a transport failure; any HTTP
    /// status is a [`Fetched`].
    async fn get(&self, url: &str, headers: HeaderMap) -> Result<Fetched>;
}

const MAX_CACHED: usize = 256;

/// Discovery over a [`Fetch`], caching each issuer's keys for ten minutes
/// (see [`JwksResolver::with_ttl`]).
///
/// Only a server identifier is looked up. Its document must name it as
/// `issuer`, exactly (RFC 8414 §3.3): otherwise a document served at
/// someone's well-known path by misconfigured hosting could vouch for keys
/// that are not theirs. A `jwks_uri` must be on the issuer's own origin.
pub struct JwksResolver {
    fetch: Arc<dyn Fetch>,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, Jwks)>>,
}

impl std::fmt::Debug for JwksResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksResolver")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl JwksResolver {
    /// Discovery over `fetch`.
    pub fn new(fetch: Arc<dyn Fetch>) -> Self {
        Self {
            fetch,
            ttl: Duration::from_secs(600),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Keep each issuer's keys for `ttl` instead.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Forget every cached key set, as after an out-of-band rotation.
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let fetched = self.fetch.get(url, HeaderMap::new()).await?;
        if !fetched.status.is_success() {
            return Err(Error::Discovery(format!(
                "{url} answered {}",
                fetched.status
            )));
        }
        serde_json::from_slice(&fetched.body)
            .map_err(|e| Error::Discovery(format!("{url} is unreadable: {e}")))
    }
}

#[async_trait]
impl KeyResolver for JwksResolver {
    async fn jwks(&self, issuer: &str, dwk: &str) -> Result<Jwks> {
        if !crate::identifier::is_server_identifier(issuer) {
            return Err(Error::Discovery(format!(
                "{issuer} is not a server identifier"
            )));
        }
        if !crate::metadata::dwk::is_known(dwk) {
            return Err(Error::Discovery(format!(
                "{dwk} is not a metadata document"
            )));
        }
        let url = metadata_url(issuer, dwk);
        if let Some((at, jwks)) = self.cache.lock().ok().and_then(|c| c.get(&url).cloned()) {
            if at.elapsed() < self.ttl {
                return Ok(jwks);
            }
        }
        let metadata: KeyedMetadata = self.json(&url).await?;
        match &metadata.issuer {
            Some(named) if named == issuer => {}
            Some(named) => {
                return Err(Error::Discovery(format!(
                    "{url} names another issuer, {named}"
                )))
            }
            None => return Err(Error::Discovery(format!("{url} names no issuer"))),
        }
        let jwks = match (metadata.jwks, metadata.jwks_uri) {
            (Some(jwks), _) => jwks,
            (None, Some(uri)) if crate::identifier::same_origin(issuer, &uri) => {
                self.json(&uri).await?
            }
            (None, Some(uri)) => {
                return Err(Error::Discovery(format!(
                    "{url} puts its keys on another origin: {uri}"
                )))
            }
            (None, None) => {
                return Err(Error::Discovery(format!(
                    "{url} has neither jwks nor jwks_uri"
                )))
            }
        };
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= MAX_CACHED {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(k, _)| k.clone());
                if let Some(oldest) = oldest {
                    cache.remove(&oldest);
                }
            }
            cache.insert(url, (Instant::now(), jwks.clone()));
        }
        Ok(jwks)
    }
}

/// Read a response body, refusing one larger than `limit`.
#[cfg(feature = "client")]
pub(crate) async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Bytes> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err(Error::Http(format!(
            "response is larger than {limit} bytes"
        )));
    }
    let mut body = bytes::BytesMut::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Error::Http(e.to_string()))?
    {
        if body.len() + chunk.len() > limit {
            return Err(Error::Http(format!(
                "response is larger than {limit} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

/// [`Fetch`] over a reqwest client.
#[cfg(feature = "client")]
#[derive(Clone, Debug)]
pub struct ReqwestFetch {
    client: reqwest::Client,
    guarded: bool,
}

#[cfg(feature = "client")]
impl ReqwestFetch {
    /// Fetch URLs someone else chose: `https` only, never an IP literal or
    /// `localhost`, no redirects or proxies, and a resolver that refuses any
    /// name answering with a non-public address or one in `denied`.
    pub fn guarded(denied: crate::client::DeniedNetworks) -> Result<Self> {
        Ok(Self {
            client: crate::client::guarded_client(denied)?,
            guarded: true,
        })
    }

    /// Fetch through `client`, unchecked: for URLs you chose yourself, or local development.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            guarded: false,
        }
    }
}

#[cfg(feature = "client")]
#[async_trait]
impl Fetch for ReqwestFetch {
    async fn get(&self, url: &str, headers: HeaderMap) -> Result<Fetched> {
        if self.guarded {
            if !url.starts_with("https://") {
                return Err(Error::Url(format!("refusing to fetch {url}")));
            }
            crate::client::guard::admit_host(url)?;
        }
        let response = self
            .client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| Error::Http(format!("{url}: {e}")))?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = read_limited(response, MAX_DOCUMENT_BYTES).await?;
        Ok(Fetched {
            status,
            headers,
            body,
        })
    }
}

/// Serves fixed JSON documents by URL, and `404` for anything else: for
/// tests, and for parties that live in the same process.
#[derive(Clone, Debug, Default)]
pub struct StaticFetch {
    documents: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl StaticFetch {
    /// No documents yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Serve `document` at `url`.
    pub fn insert(&self, url: impl Into<String>, document: serde_json::Value) {
        if let Ok(mut documents) = self.documents.lock() {
            documents.insert(url.into(), document);
        }
    }
}

#[async_trait]
impl Fetch for StaticFetch {
    async fn get(&self, url: &str, _headers: HeaderMap) -> Result<Fetched> {
        let document = self.documents.lock().ok().and_then(|d| d.get(url).cloned());
        Ok(match document {
            Some(document) => Fetched {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::from(document.to_string()),
            },
            None => Fetched {
                status: StatusCode::NOT_FOUND,
                headers: HeaderMap::new(),
                body: Bytes::new(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn keys_are_found_through_the_metadata_document() {
        let fetch = StaticFetch::new();
        fetch.insert(
            "https://ap.example/.well-known/aauth-agent.json",
            json!({ "issuer": "https://ap.example", "jwks_uri": "https://ap.example/jwks.json" }),
        );
        fetch.insert(
            "https://ap.example/jwks.json",
            json!({ "keys": [{ "kty": "OKP", "kid": "k" }] }),
        );
        let resolver = JwksResolver::new(Arc::new(fetch));
        let jwks = resolver
            .jwks("https://ap.example", "aauth-agent.json")
            .await
            .unwrap();
        assert_eq!(jwks.keys[0].kid.as_deref(), Some("k"));
    }

    #[tokio::test]
    async fn a_document_naming_another_issuer_is_refused() {
        let fetch = StaticFetch::new();
        fetch.insert(
            "https://ap.example/.well-known/aauth-agent.json",
            json!({ "issuer": "https://evil.example", "jwks": { "keys": [] } }),
        );
        let resolver = JwksResolver::new(Arc::new(fetch));
        let refused = resolver
            .jwks("https://ap.example", "aauth-agent.json")
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("another issuer"), "{refused}");
    }

    #[tokio::test]
    async fn keys_are_cached_until_cleared() {
        let fetch = StaticFetch::new();
        let url = "https://ap.example/.well-known/aauth-agent.json";
        fetch.insert(
            url,
            json!({ "issuer": "https://ap.example", "jwks": { "keys": [{ "kty": "OKP", "kid": "a" }] } }),
        );
        let resolver = JwksResolver::new(Arc::new(fetch.clone()));
        resolver
            .jwks("https://ap.example", "aauth-agent.json")
            .await
            .unwrap();
        fetch.insert(
            url,
            json!({ "issuer": "https://ap.example", "jwks": { "keys": [{ "kty": "OKP", "kid": "b" }] } }),
        );
        let cached = resolver
            .jwks("https://ap.example", "aauth-agent.json")
            .await
            .unwrap();
        assert_eq!(cached.keys[0].kid.as_deref(), Some("a"));
        resolver.clear();
        let fresh = resolver
            .jwks("https://ap.example", "aauth-agent.json")
            .await
            .unwrap();
        assert_eq!(fresh.keys[0].kid.as_deref(), Some("b"));
    }
}
