//! Thin async HTTP wrapper used by [`crate::api::tasks`] and
//! [`crate::api::files`].
//!
//! Centralises base-URL joining, `Authorization` header injection, query
//! string encoding, multipart uploads, and DP error translation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, COOKIE};
use reqwest::{Response, StatusCode};
use serde::Serialize;
use serde_json::Value;

use crate::api::backoff::{backoff_delay, retry_after_from};
use crate::api::error::{ApiResult, IntrospectionAPIError};

/// Resolved HTTP configuration used by every REST API call.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub api_url: String,
    pub token: String,
    pub additional_headers: HashMap<String, String>,
    pub timeout: std::time::Duration,
    /// Automatic retries on a `429 Too Many Requests` for unary REST calls
    /// (honouring `Retry-After`). `0` disables retrying. Defaults to
    /// [`crate::types::defaults::API_MAX_RETRIES`] when built from
    /// [`crate::ClientConfig`].
    pub max_retries: u32,
    /// Base step of the capped-exponential `429` retry backoff (`Retry-After`
    /// is the floor). Defaults to
    /// [`crate::types::defaults::API_RETRY_BASE_MS`] when built from
    /// [`crate::ClientConfig`].
    pub retry_base: Duration,
}

impl HttpConfig {
    fn build_default_headers(&self) -> ApiResult<HeaderMap> {
        let mut h = HeaderMap::new();
        for (k, v) in &self.additional_headers {
            let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| {
                IntrospectionAPIError::InvalidConfig(format!("invalid header name `{k}`"))
            })?;
            let value = HeaderValue::from_str(v).map_err(|_| {
                IntrospectionAPIError::InvalidConfig(format!("invalid header value for `{k}`"))
            })?;
            h.insert(name, value);
        }
        if !h.contains_key(COOKIE) {
            let auth = format!("Bearer {}", self.token);
            h.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&auth).map_err(|_| {
                    IntrospectionAPIError::InvalidConfig(
                        "INTROSPECTION_TOKEN contains characters that are not valid in an HTTP header"
                            .to_string(),
                    )
                })?,
            );
        }
        // `entry`, not `insert`: additional_headers is applied above, and an
        // unconditional insert silently discarded a caller-supplied
        // User-Agent.
        h.entry(reqwest::header::USER_AGENT)
            .or_insert_with(|| HeaderValue::from_static(crate::USER_AGENT));
        Ok(h)
    }
}

/// Async HTTP client shared by every REST API namespace.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    cfg: Arc<HttpConfig>,
    /// Flipped by [`crate::Runner::close`]. Shared with every clone, so a
    /// namespace handle a caller kept hold of stops working the moment the
    /// runner that produced it is closed -- which is why the runner's
    /// accessors can be infallible instead of panicking.
    closed: Arc<AtomicBool>,
}

impl HttpClient {
    pub fn new(cfg: HttpConfig) -> ApiResult<Self> {
        if cfg.token.is_empty() {
            return Err(IntrospectionAPIError::InvalidConfig(
                "API token is empty; set `INTROSPECTION_TOKEN` or `ClientConfig::token`"
                    .to_string(),
            ));
        }
        let headers = cfg.build_default_headers()?;
        // `read_timeout`, not `timeout`. The latter is a total deadline that
        // runs "until the response body has finished", which for an SSE run
        // or a chunked download means the connection is severed once the
        // configured window elapses no matter how healthy the stream is. A
        // read timeout resets on every successful read, so it still catches a
        // stalled connection. Unary calls re-apply the total deadline
        // per-request in `send_retrying` / `post_multipart`.
        let inner = reqwest::Client::builder()
            .default_headers(headers)
            .read_timeout(cfg.timeout)
            .build()?;
        Ok(Self {
            inner,
            cfg: Arc::new(cfg),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Build an [`HttpClient`] around an already-configured
    /// [`reqwest::Client`], for a caller that owns its own transport
    /// (proxy, trust roots, user agent, connect timeout).
    ///
    /// The caller's client is used as given: `cfg.token` and
    /// `cfg.additional_headers` are **not** applied to it, so authentication
    /// has to be on `inner` as a default header. `cfg.api_url`, `cfg.timeout`
    /// and the retry fields still govern every request. There is no
    /// token-empty check, which is also what lets a test fixture skip one.
    ///
    /// Wrap the result in `Arc` and hand it to a resource namespace's `new`
    /// (e.g. `Repositories::new`, `Recipes::new`) to reach one resource
    /// directly — without going through [`crate::IntrospectionClient`], whose
    /// constructor demands a non-empty bearer token even for a caller (a CP
    /// session-cookie flow, say) that authenticates entirely through
    /// `additional_headers` here. Every resource namespace's `new` is `pub`
    /// for exactly this; it is `#[doc(hidden)]` because it is not the
    /// intended way to reach a resource with a bearer token in hand —
    /// `IntrospectionClient` still is.
    pub fn from_parts(inner: reqwest::Client, cfg: HttpConfig) -> Self {
        Self {
            inner,
            cfg: Arc::new(cfg),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Refuse all further requests through this client and its clones.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    /// The base URL and credential this client was built with.
    pub(crate) fn config(&self) -> &HttpConfig {
        &self.cfg
    }

    /// `Err` once [`close`](Self::close) has been called.
    fn check_open(&self) -> ApiResult<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(IntrospectionAPIError::InvalidConfig(
                "runner has been closed".to_string(),
            ));
        }
        Ok(())
    }

    fn url(&self, path: &str) -> String {
        let base = self.cfg.api_url.trim_end_matches('/');
        if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
        }
    }

    /// Lower a serializable query object into the tuple sequence reqwest uses
    /// for repeated parameters. Passing a JSON array directly to
    /// `RequestBuilder::query` fails with `unsupported value`; flattening it
    /// here produces `key=a&key=b` and keeps every GET namespace consistent.
    fn query_pairs<Q: Serialize>(query: &Q) -> ApiResult<Vec<(String, String)>> {
        let value = serde_json::to_value(query).map_err(|e| {
            IntrospectionAPIError::Decode(format!("failed to encode query params: {e}"))
        })?;
        let mut pairs = Vec::new();
        if let Value::Array(entries) = value {
            for entry in entries {
                let Value::Array(mut pair) = entry else {
                    return Err(IntrospectionAPIError::Decode(
                        "query pair must serialize as [key, value]".to_string(),
                    ));
                };
                if pair.len() != 2 {
                    return Err(IntrospectionAPIError::Decode(
                        "query pair must contain exactly two values".to_string(),
                    ));
                }
                let value = pair.pop().expect("length checked");
                let key = pair.pop().expect("length checked");
                let Value::String(key) = key else {
                    return Err(IntrospectionAPIError::Decode(
                        "query parameter name must be a string".to_string(),
                    ));
                };
                pairs.push((key, Self::query_scalar(value)?));
            }
            return Ok(pairs);
        }
        let Value::Object(fields) = value else {
            if value.is_null() {
                return Ok(pairs);
            }
            return Err(IntrospectionAPIError::Decode(
                "query params must serialize to an object or pair sequence".to_string(),
            ));
        };
        for (key, value) in fields {
            match value {
                Value::Null => {}
                Value::Array(values) => {
                    for value in values {
                        pairs.push((key.clone(), Self::query_scalar(value)?));
                    }
                }
                value => pairs.push((key, Self::query_scalar(value)?)),
            }
        }
        Ok(pairs)
    }

    fn query_scalar(value: Value) -> ApiResult<String> {
        match value {
            Value::String(value) => Ok(value),
            Value::Bool(value) => Ok(value.to_string()),
            Value::Number(value) => Ok(value.to_string()),
            _ => Err(IntrospectionAPIError::Decode(
                "query parameter values must be scalars or scalar arrays".to_string(),
            )),
        }
    }

    /// Send a unary request, transparently retrying on a rejected-but-retryable
    /// status, honouring `Retry-After` as the floor of a capped-exponential
    /// backoff.
    ///
    /// `build` is invoked once per attempt (so the request is rebuilt rather
    /// than cloned). Retry policy:
    /// - **`429 Too Many Requests`** is retried for **any** method — the
    ///   request was rejected and never processed, so re-sending is
    ///   side-effect-safe even for writes.
    /// - **`502` / `503` / `504`** are retried **only when `idempotent`** is
    ///   set (i.e. the caller is a `GET`), since a transient gateway/upstream
    ///   error on a non-idempotent write can't be safely re-sent.
    ///
    /// Once the retry budget is spent the status is mapped to a typed error by
    /// [`expect_ok`] like any other non-2xx. Streaming has its own resume
    /// budget and does not go through here (see [`crate::api::resumable`]).
    async fn send_retrying<F>(&self, idempotent: bool, build: F) -> ApiResult<Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        self.check_open()?;
        let mut attempt: u32 = 0;
        loop {
            let res = build().timeout(self.cfg.timeout).send().await?;
            let retryable = is_retryable_status(res.status(), idempotent);
            if retryable && attempt < self.cfg.max_retries {
                let delay = backoff_delay(
                    attempt,
                    self.cfg.retry_base,
                    retry_after_from(res.headers()),
                );
                attempt += 1;
                tokio::time::sleep(delay).await;
                continue;
            }
            return expect_ok(res).await;
        }
    }

    /// GET that returns JSON. Pass `&()` for no query string.
    pub async fn get_json<Q: Serialize, R: ResponseBody>(
        &self,
        path: &str,
        query: &Q,
    ) -> ApiResult<R> {
        let query = Self::query_pairs(query)?;
        let res = self
            .send_retrying(true, || self.inner.get(self.url(path)).query(&query))
            .await?;
        decode_json(res).await
    }

    /// POST a JSON body, decode JSON response.
    pub async fn post_json<B: Serialize, R: ResponseBody>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<R> {
        let res = self
            .send_retrying(false, || self.inner.post(self.url(path)).json(body))
            .await?;
        decode_json(res).await
    }

    /// POST a JSON body; `None` on `204 No Content`, else the decoded JSON.
    pub async fn post_json_or_no_content<B: Serialize, R: ResponseBody>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<Option<R>> {
        let res = self
            .send_retrying(false, || self.inner.post(self.url(path)).json(body))
            .await?;
        if res.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        decode_json(res).await.map(Some)
    }

    /// POST a JSON body and accept an empty success response.
    pub async fn post_json_empty<B: Serialize>(&self, path: &str, body: &B) -> ApiResult<()> {
        self.send_retrying(false, || self.inner.post(self.url(path)).json(body))
            .await?;
        Ok(())
    }

    /// POST a JSON body the server deduplicates (an event carrying its own id),
    /// accepting an empty success response. Unlike [`Self::post_json_empty`],
    /// a `502` / `503` / `504` is retried too, since re-sending the same body
    /// cannot apply it twice.
    pub async fn post_json_empty_idempotent<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<()> {
        self.send_retrying(true, || self.inner.post(self.url(path)).json(body))
            .await?;
        Ok(())
    }

    /// [`Self::post_json_empty_idempotent`] that hands back the 2xx
    /// response, for a route whose status and headers carry the result.
    pub(crate) async fn post_json_idempotent_response<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<Response> {
        self.send_retrying(true, || self.inner.post(self.url(path)).json(body))
            .await
    }

    /// PATCH a JSON body, decode JSON response.
    pub async fn patch_json<B: Serialize, R: ResponseBody>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<R> {
        let res = self
            .send_retrying(false, || self.inner.patch(self.url(path)).json(body))
            .await?;
        decode_json(res).await
    }

    /// POST with no body, no response body.
    pub async fn post_empty(&self, path: &str) -> ApiResult<()> {
        self.send_retrying(false, || self.inner.post(self.url(path)))
            .await?;
        Ok(())
    }

    /// DELETE, no response body.
    pub async fn delete_empty(&self, path: &str) -> ApiResult<()> {
        self.send_retrying(false, || self.inner.delete(self.url(path)))
            .await?;
        Ok(())
    }

    /// POST multipart, decode JSON response.
    ///
    /// Not auto-retried on `429`: a multipart [`Form`](reqwest::multipart::Form)
    /// is consumed on send and can't be rebuilt per attempt, and uploads are
    /// not the high-frequency path the retry policy targets.
    pub async fn post_multipart<R: ResponseBody>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> ApiResult<R> {
        self.check_open()?;
        let res = self
            .inner
            .post(self.url(path))
            .multipart(form)
            .timeout(self.cfg.timeout)
            .send()
            .await?;
        decode_json(expect_ok(res).await?).await
    }

    /// GET returning the raw (2xx) [`Response`] with a query string and an
    /// optional `Accept` header, applying the same retry policy as
    /// [`get_json`](Self::get_json). The caller reads the body and response
    /// headers itself — used by the telemetry Arrow reads, where pagination
    /// metadata (`X-Next-Cursor`, `X-Result-Count`, …) moves to headers. A
    /// non-2xx (e.g. `406` when Arrow is unsupported) is mapped to a typed
    /// [`IntrospectionAPIError`] like any other call.
    pub async fn get_raw<Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
        accept: Option<&str>,
    ) -> ApiResult<Response> {
        let accept = accept.map(str::to_string);
        let query = Self::query_pairs(query)?;
        self.send_retrying(true, || {
            let mut req = self.inner.get(self.url(path)).query(&query);
            if let Some(a) = &accept {
                req = req.header(reqwest::header::ACCEPT, a.clone());
            }
            req
        })
        .await
    }

    /// GET binary content into memory.
    pub async fn get_bytes(&self, path: &str) -> ApiResult<Bytes> {
        let res = self
            .send_retrying(true, || self.inner.get(self.url(path)))
            .await?;
        Ok(res.bytes().await?)
    }

    /// GET returning the raw streaming [`Response`]. Used by SSE streaming
    /// and chunked binary downloads.
    pub async fn get_stream(&self, path: &str, accept: Option<&str>) -> ApiResult<Response> {
        self.check_open()?;
        let mut req = self.inner.get(self.url(path));
        if let Some(a) = accept {
            req = req.header(reqwest::header::ACCEPT, a);
        }
        let res = req.send().await?;
        expect_ok(res).await
    }

    /// GET returning the raw [`Response`] **without** the non-2xx → error
    /// translation [`get_stream`] applies. The caller inspects `status()` and
    /// headers itself — used by the resumable stream's `429` readiness handling
    /// (see [`crate::api::resumable`]), where a `429` is a retry signal, not a
    /// failure. Only a transport-level failure yields `Err`.
    ///
    /// [`get_stream`]: Self::get_stream
    pub async fn get_stream_raw(
        &self,
        path: &str,
        accept: Option<&str>,
        last_event_id: Option<&str>,
    ) -> ApiResult<Response> {
        self.check_open()?;
        let mut req = self.inner.get(self.url(path));
        if let Some(a) = accept {
            req = req.header(reqwest::header::ACCEPT, a);
        }
        if let Some(id) = last_event_id {
            // SSE-standard resume cursor — the DP replays content frames after
            // this id so a reconnect is gap-free (see `crate::api::resumable`).
            req = req.header("last-event-id", id);
        }
        Ok(req.send().await?)
    }
}

/// Whether a non-2xx status should be retried. `429` is always retryable (the
/// request was rejected, not processed); transient gateway/upstream errors
/// (`502`/`503`/`504`) are retryable only for `idempotent` (GET) calls.
fn is_retryable_status(status: StatusCode, idempotent: bool) -> bool {
    match status {
        StatusCode::TOO_MANY_REQUESTS => true,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            idempotent
        }
        _ => false,
    }
}

/// A type a JSON response decodes into.
///
/// With the `strict-schema` feature it must serialize too: the decode writes
/// the value back to JSON to find the fields it dropped.
/// Every response type the SDK returns does both.
#[cfg(not(feature = "strict-schema"))]
pub trait ResponseBody: serde::de::DeserializeOwned {}
#[cfg(not(feature = "strict-schema"))]
impl<T: serde::de::DeserializeOwned> ResponseBody for T {}

/// A type a JSON response decodes into.
///
/// With the `strict-schema` feature it must serialize too: the decode writes
/// the value back to JSON to find the fields it dropped.
/// Every response type the SDK returns does both.
#[cfg(feature = "strict-schema")]
pub trait ResponseBody: serde::de::DeserializeOwned + Serialize {}
#[cfg(feature = "strict-schema")]
impl<T: serde::de::DeserializeOwned + Serialize> ResponseBody for T {}

pub(crate) async fn decode_json<R: ResponseBody>(res: Response) -> ApiResult<R> {
    let route = res.url().path().to_string();
    let bytes = res.bytes().await?;
    decode_slice(&bytes, &route)
}

/// Decode a JSON response body read from `route`.
///
/// With the `strict-schema` feature, a field in the body that `R` does not
/// model fails the decode too, and the error lists every such field.
#[cfg(not(feature = "strict-schema"))]
pub(crate) fn decode_slice<R: ResponseBody>(bytes: &[u8], _route: &str) -> ApiResult<R> {
    serde_json::from_slice(bytes).map_err(parse_error)
}

/// Decode a JSON response body read from `route`.
///
/// With the `strict-schema` feature, a field in the body that `R` does not
/// model fails the decode too, and the error lists every such field.
#[cfg(feature = "strict-schema")]
pub(crate) fn decode_slice<R: ResponseBody>(bytes: &[u8], route: &str) -> ApiResult<R> {
    decode_value(serde_json::from_slice(bytes).map_err(parse_error)?, route)
}

/// [`decode_slice`] for a body already parsed into a [`Value`].
#[cfg(not(feature = "strict-schema"))]
pub(crate) fn decode_value<R: ResponseBody>(value: Value, _route: &str) -> ApiResult<R> {
    serde_json::from_value(value).map_err(parse_error)
}

/// [`decode_slice`] for a body already parsed into a [`Value`].
///
/// A field is unmodelled when the decode ignores it or drops it.
/// `serde_ignored` sees the fields a struct ignores, but not inside a
/// `#[serde(flatten)]` struct or an untagged or internally tagged enum: serde
/// buffers those and discards what the type did not take without telling the
/// deserializer. So the decoded value is serialized again, and a field of the
/// body missing from that output was dropped. The response types keep what
/// they model (`tests/round_trip_test.rs` holds them to that), so the diff
/// finds exactly the unmodelled fields; see [`dropped_fields`] for the two
/// differences that are not.
#[cfg(feature = "strict-schema")]
pub(crate) fn decode_value<R: ResponseBody>(value: Value, route: &str) -> ApiResult<R> {
    let mut ignored = std::collections::HashSet::new();
    let decoded: R = serde_ignored::deserialize(&value, |path| {
        ignored.insert(json_path(&path));
    })
    .map_err(parse_error)?;
    let back = serde_json::to_value(&decoded).map_err(|e| {
        IntrospectionAPIError::Decode(format!("{route}: failed to re-encode the response: {e}"))
    })?;
    let mut unknown = Vec::new();
    dropped_fields("$".to_string(), &value, Some(&back), &ignored, &mut unknown);
    if unknown.is_empty() {
        Ok(decoded)
    } else {
        Err(IntrospectionAPIError::Decode(format!(
            "{route}: response fields this SDK does not model: {}",
            unknown.join(", ")
        )))
    }
}

fn parse_error(e: serde_json::Error) -> IntrospectionAPIError {
    IntrospectionAPIError::Decode(format!("failed to parse JSON response: {e}"))
}

/// Push to `out`, in body order, the path of every field of `sent` that
/// `serde_ignored` reported or that `back`, its decoded value serialized
/// again, does not hold.
///
/// A field missing from `back` is not counted when:
///
/// - its value is what the type leaves out when it serializes: `null` for a
///   `None`, and `[]`, `{}` or `false` for the fields that skip an empty
///   collection or a `false`. `serde_ignored` still reports such a field when
///   it is unmodelled outside a buffered position.
/// - `back` holds its value under a key the body did not send: the type read
///   it through a `#[serde(alias)]` and writes it under its own name.
///
/// Only objects and arrays on both sides are walked into. A value the type
/// writes back in another shape (a string enum, a timestamp) is a leaf.
#[cfg(feature = "strict-schema")]
fn dropped_fields(
    path: String,
    sent: &Value,
    back: Option<&Value>,
    ignored: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    match (sent, back) {
        (Value::Object(sent), Some(Value::Object(back))) => {
            for (key, value) in sent {
                let child = format!("{path}.{key}");
                if ignored.contains(&child) {
                    out.push(child);
                    continue;
                }
                match back.get(key) {
                    Some(other) => dropped_fields(child, value, Some(other), ignored, out),
                    None if left_out_when_serialized(value) => {}
                    None if back
                        .iter()
                        .any(|(k, v)| v == value && !sent.contains_key(k)) => {}
                    None => out.push(child),
                }
            }
        }
        (Value::Array(sent), Some(Value::Array(back))) => {
            for (index, value) in sent.iter().enumerate() {
                dropped_fields(
                    format!("{path}[{index}]"),
                    value,
                    back.get(index),
                    ignored,
                    out,
                );
            }
        }
        _ => {
            // A leaf, or a value the type writes back in another shape. Only
            // `serde_ignored` can still see inside it.
            let prefix = [format!("{path}."), format!("{path}[")];
            let mut nested: Vec<_> = ignored
                .iter()
                .filter(|p| prefix.iter().any(|pre| p.starts_with(pre.as_str())))
                .cloned()
                .collect();
            nested.sort();
            out.extend(nested);
        }
    }
}

#[cfg(feature = "strict-schema")]
fn left_out_when_serialized(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// `$.data[0].foo` for a field `serde_ignored` reports.
#[cfg(feature = "strict-schema")]
fn json_path(path: &serde_ignored::Path<'_>) -> String {
    use serde_ignored::Path;
    match path {
        Path::Root => "$".to_string(),
        Path::Seq { parent, index } => format!("{}[{index}]", json_path(parent)),
        Path::Map { parent, key } => format!("{}.{key}", json_path(parent)),
        Path::Some { parent }
        | Path::NewtypeStruct { parent }
        | Path::NewtypeVariant { parent } => json_path(parent),
    }
}

async fn expect_ok(res: Response) -> ApiResult<Response> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    Err(to_api_error(res, status).await)
}

pub(crate) async fn to_api_error(res: Response, status: StatusCode) -> IntrospectionAPIError {
    let request_id = res
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    // Read before the body is consumed. The transparent retry path already
    // honours this; carrying it on the error is what lets a caller schedule
    // its own retry with the server's number once the budget is spent.
    let retry_after = retry_after_from(res.headers());
    let ct = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body_bytes = match res.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return IntrospectionAPIError::http(
                status.as_u16(),
                format!("HTTP {} (failed to read body: {e})", status.as_u16()),
                request_id,
                None,
            );
        }
    };

    if ct.contains("json") {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            let message =
                extract_message(&value).unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            return IntrospectionAPIError::Http {
                status: status.as_u16(),
                message,
                // The envelope's `code` used to be dropped on the floor, so
                // `runner_expired` on a 401 was indistinguishable from a bad
                // API key.
                code: extract_code(&value),
                request_id,
                body: Some(value),
                retry_after,
            };
        }
    }
    let text = String::from_utf8_lossy(&body_bytes).into_owned();
    let (body, message) = if text.is_empty() {
        (None, format!("HTTP {}", status.as_u16()))
    } else {
        (Some(serde_json::Value::String(text.clone())), text)
    };
    IntrospectionAPIError::Http {
        status: status.as_u16(),
        message,
        code: None,
        request_id,
        body,
        retry_after,
    }
}

/// The DP's machine-readable error code, when the envelope carries one.
/// The envelope's `code`, or for an RFC 6749 §5.2 token-endpoint error (a
/// top-level `{"error", "error_description"}` body) its `error`, so a caller
/// can branch on `invalid_grant` the way it branches on `runner_expired`.
fn extract_code(value: &serde_json::Value) -> Option<String> {
    let obj = value.as_object()?;
    obj.get("code")
        .or_else(|| obj.get("error"))?
        .as_str()
        .map(str::to_string)
}

fn extract_message(value: &serde_json::Value) -> Option<String> {
    let obj = value.as_object()?;
    let Some(detail) = obj.get("detail") else {
        return obj
            .get("error_description")
            .and_then(|d| d.as_str())
            .map(str::to_string);
    };
    if let Some(s) = detail.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = detail.as_array() {
        let msgs: Vec<String> = arr
            .iter()
            .filter_map(|item| item.get("msg").and_then(|m| m.as_str()).map(String::from))
            .collect();
        if !msgs.is_empty() {
            return Some(msgs.join("; "));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_with_unknown_fields() -> Value {
        serde_json::json!({
            "records": [
                {"id": "t1", "name": "a", "color": "red"},
                {"id": "t2", "name": "b", "shade": {"dark": true}}
            ],
            "count": 2,
            "cursor_v2": "x"
        })
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct Tag {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        name: String,
    }

    #[test]
    #[cfg(not(feature = "strict-schema"))]
    fn unknown_response_fields_are_ignored() {
        let page: crate::api::schemas::Paginated<Tag> =
            decode_value(page_with_unknown_fields(), "/v1/tags").unwrap();
        assert_eq!(page.records.len(), 2);
    }

    #[test]
    #[cfg(feature = "strict-schema")]
    fn strict_schema_reports_every_unknown_field_with_the_route() {
        let bytes = serde_json::to_vec(&page_with_unknown_fields()).unwrap();

        let from_bytes = decode_slice::<crate::api::schemas::Paginated<Tag>>(&bytes, "/v1/tags")
            .unwrap_err()
            .to_string();
        let from_value = decode_value::<crate::api::schemas::Paginated<Tag>>(
            page_with_unknown_fields(),
            "/v1/tags",
        )
        .unwrap_err()
        .to_string();

        // `Value` keeps its keys sorted, so `cursor_v2` comes first.
        let expected = "/v1/tags: response fields this SDK does not model: \
                        $.cursor_v2, $.records[0].color, $.records[1].shade";
        assert!(from_bytes.ends_with(expected), "{from_bytes}");
        assert!(from_value.ends_with(expected), "{from_value}");
    }

    #[test]
    #[cfg(feature = "strict-schema")]
    fn strict_schema_reports_inside_the_span_attributes_but_not_the_open_maps() {
        let body = serde_json::json!({
            "object": "list",
            "data": [{
                "trace_id": "t",
                "start_time": "2026-08-04T22:14:34Z",
                "attributes": {
                    "gen_ai": {"vendor_specific": 1, "request": {"thinking_budget": 2}},
                    "introspection": {"tenant_tier": "max"},
                    "acme": {"a": 1}
                }
            }]
        });

        let error = decode_value::<crate::api::genai_span::GenAiSpanList>(body, "/v1/x")
            .unwrap_err()
            .to_string();

        assert!(
            error.ends_with(
                "/v1/x: response fields this SDK does not model: \
                             $.data[0].attributes.introspection.tenant_tier"
            ),
            "{error}"
        );
    }

    /// A record whose fields serde buffers before it decodes them: a
    /// flattened struct and an untagged enum.
    #[cfg(feature = "strict-schema")]
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct Buffered {
        id: String,
        #[serde(flatten)]
        common: Common,
        value: Either,
    }

    #[cfg(feature = "strict-schema")]
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct Common {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        #[serde(alias = "old_label")]
        label: Option<String>,
    }

    #[cfg(feature = "strict-schema")]
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    #[serde(untagged)]
    enum Either {
        Text(String),
        Range { from: u32, to: u32 },
    }

    #[cfg(feature = "strict-schema")]
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Tagged {
        Point { x: u32 },
    }

    #[test]
    #[cfg(feature = "strict-schema")]
    fn strict_schema_reports_inside_a_flattened_struct_and_an_untagged_variant() {
        let body = serde_json::json!({
            "id": "a",
            "name": "n",
            "shade": "blue",
            "value": {"from": 1, "to": 2, "step": 1}
        });

        let error = decode_value::<Buffered>(body, "/v1/x")
            .unwrap_err()
            .to_string();

        assert!(
            error
                .ends_with("/v1/x: response fields this SDK does not model: $.shade, $.value.step"),
            "{error}"
        );
    }

    #[test]
    #[cfg(feature = "strict-schema")]
    fn strict_schema_reports_inside_an_internally_tagged_variant() {
        let body = serde_json::json!([{"kind": "point", "x": 1, "y": 2}]);

        let error = decode_value::<Vec<Tagged>>(body, "/v1/x")
            .unwrap_err()
            .to_string();

        assert!(
            error.ends_with("/v1/x: response fields this SDK does not model: $[0].y"),
            "{error}"
        );
    }

    #[test]
    #[cfg(feature = "strict-schema")]
    fn strict_schema_accepts_what_a_type_leaves_out_or_renames() {
        // `tags: []` and `label: null` are left out when the record
        // serializes, and `old_label` comes back as `label`.
        let body = serde_json::json!([
            {"id": "a", "name": "n", "tags": [], "label": null, "value": "v"},
            {"id": "b", "name": "n", "old_label": "l", "value": {"from": 1, "to": 2}}
        ]);

        let records = decode_value::<Vec<Buffered>>(body.clone(), "/v1/x").unwrap();
        let from_bytes =
            decode_slice::<Vec<Buffered>>(&serde_json::to_vec(&body).unwrap(), "/v1/x").unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(from_bytes[1].common.label.as_deref(), Some("l"));
    }

    #[test]
    fn retry_policy_429_any_method_5xx_get_only() {
        // 429: retryable regardless of idempotency (rejected, not processed).
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS, false));
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS, true));

        // 502/503/504: retryable only for idempotent (GET) calls.
        for status in [
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert!(is_retryable_status(status, true), "{status} GET");
            assert!(!is_retryable_status(status, false), "{status} write");
        }

        // Everything else is never retried.
        for status in [
            StatusCode::OK,
            StatusCode::BAD_REQUEST,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert!(!is_retryable_status(status, true));
            assert!(!is_retryable_status(status, false));
        }
    }
}
