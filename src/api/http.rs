//! Thin async HTTP wrapper used by [`crate::api::tasks`] and
//! [`crate::api::files`].
//!
//! Centralises base-URL joining, `Authorization` header injection, query
//! string encoding, multipart uploads, and DP error translation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::{Response, StatusCode};
use serde::Serialize;

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

/// Cap on the `429` retry backoff.
const RETRY_MAX_BACKOFF: Duration = Duration::from_secs(10);

impl HttpConfig {
    fn build_default_headers(&self) -> ApiResult<HeaderMap> {
        let mut h = HeaderMap::new();
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
        for (k, v) in &self.additional_headers {
            let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| {
                IntrospectionAPIError::InvalidConfig(format!("invalid header name `{k}`"))
            })?;
            let value = HeaderValue::from_str(v).map_err(|_| {
                IntrospectionAPIError::InvalidConfig(format!("invalid header value for `{k}`"))
            })?;
            h.insert(name, value);
        }
        h.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_str(&format!(
                "introspection-sdk-rust/{}",
                env!("CARGO_PKG_VERSION")
            ))
            .expect("static user agent is valid"),
        );
        Ok(h)
    }
}

/// Async HTTP client shared by every REST API namespace.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    cfg: Arc<HttpConfig>,
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
        let inner = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(cfg.timeout)
            .build()?;
        Ok(Self {
            inner,
            cfg: Arc::new(cfg),
        })
    }

    /// Test seam: build an [`HttpClient`] around an already-configured
    /// [`reqwest::Client`]. Skips the token-empty / header-validation
    /// checks so wiremock fixtures don't need a real token.
    #[doc(hidden)]
    pub fn from_parts(inner: reqwest::Client, cfg: HttpConfig) -> Self {
        Self {
            inner,
            cfg: Arc::new(cfg),
        }
    }

    fn url(&self, path: &str) -> String {
        let base = self.cfg.api_url.trim_end_matches('/');
        if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
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
        let mut attempt: u32 = 0;
        loop {
            let res = build().send().await?;
            let retryable = is_retryable_status(res.status(), idempotent);
            if retryable && attempt < self.cfg.max_retries {
                let delay = retry_delay(
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
    pub async fn get_json<Q: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &Q,
    ) -> ApiResult<R> {
        let res = self
            .send_retrying(true, || self.inner.get(self.url(path)).query(query))
            .await?;
        decode_json(res).await
    }

    /// POST a JSON body, decode JSON response.
    pub async fn post_json<B: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<R> {
        let res = self
            .send_retrying(false, || self.inner.post(self.url(path)).json(body))
            .await?;
        decode_json(res).await
    }

    /// PATCH a JSON body, decode JSON response.
    pub async fn patch_json<B: Serialize, R: serde::de::DeserializeOwned>(
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
    pub async fn post_multipart<R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> ApiResult<R> {
        let res = self
            .inner
            .post(self.url(path))
            .multipart(form)
            .send()
            .await?;
        decode_json(expect_ok(res).await?).await
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

/// Parse a `Retry-After` header as a delay. Only the delta-seconds form is
/// honoured (what the DP emits); an HTTP-date value is ignored.
fn retry_after_from(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|secs| secs.is_finite() && *secs >= 0.0)
        .map(Duration::from_secs_f64)
}

/// `Retry-After` as the floor of a capped-exponential step (`base * 2^attempt`).
fn retry_delay(attempt: u32, base: Duration, retry_after: Option<Duration>) -> Duration {
    let factor = 1u64.checked_shl(attempt.min(20)).unwrap_or(u64::MAX);
    let exp = Duration::from_millis((base.as_millis() as u64).saturating_mul(factor))
        .min(RETRY_MAX_BACKOFF);
    retry_after.map(|ra| ra.max(exp)).unwrap_or(exp)
}

async fn decode_json<R: serde::de::DeserializeOwned>(res: Response) -> ApiResult<R> {
    let bytes = res.bytes().await?;
    serde_json::from_slice(&bytes)
        .map_err(|e| IntrospectionAPIError::Decode(format!("failed to parse JSON response: {e}")))
}

async fn expect_ok(res: Response) -> ApiResult<Response> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    Err(to_api_error(res, status).await)
}

async fn to_api_error(res: Response, status: StatusCode) -> IntrospectionAPIError {
    let request_id = res
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
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
            return IntrospectionAPIError::http(status.as_u16(), message, request_id, Some(value));
        }
    }
    let text = String::from_utf8_lossy(&body_bytes).into_owned();
    let (body, message) = if text.is_empty() {
        (None, format!("HTTP {}", status.as_u16()))
    } else {
        (Some(serde_json::Value::String(text.clone())), text)
    };
    IntrospectionAPIError::http(status.as_u16(), message, request_id, body)
}

fn extract_message(value: &serde_json::Value) -> Option<String> {
    let obj = value.as_object()?;
    let detail = obj.get("detail")?;
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
