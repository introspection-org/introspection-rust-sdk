//! Shared error type for the REST API namespaces.
//!
//! Raised by every method on [`crate::Runner::tasks`] and
//! [`crate::Runner::files`] (via the runner-bound `tasks` / `files`
//! namespaces), as well as the CP-side [`crate::Runtimes`] /
//! [`crate::Experiments`] / [`crate::Recipes`] resources. The OTLP paths (`track` / `feedback` / `identify` on
//! `crate::otel::IntrospectionLogs`) keep returning
//! [`crate::IntrospectionError`].

use thiserror::Error;

/// HTTP error from the Introspection DP REST API.
///
/// Carries a status code, the optional machine-readable code from the error
/// envelope, the optional request ID (from `X-Request-Id`), and the raw
/// response body (parsed JSON when the response was JSON, else the text).
#[derive(Error, Debug)]
pub enum IntrospectionAPIError {
    /// Non-2xx HTTP response from the DP.
    #[error("{message} (status={status})")]
    Http {
        message: String,
        status: u16,
        /// Machine-readable error code from the response body, when the DP
        /// sent one. This is what distinguishes an expired runner JWT from
        /// any other `401`, so a caller can branch on the cause rather than
        /// on the status alone.
        code: Option<String>,
        request_id: Option<String>,
        body: Option<serde_json::Value>,
        /// `Retry-After`, when the response carried one. A floor on when to
        /// come back, not advice — the retry path already honours it, and a
        /// caller handling the error after the budget is spent needs the
        /// same number.
        retry_after: Option<std::time::Duration>,
    },

    /// Network / transport layer failure (DNS, TLS, connection reset, …).
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Failure decoding the response body (JSON / UTF-8 / etc).
    #[error("decode error: {0}")]
    Decode(String),

    /// Invalid SDK configuration (missing token, malformed base URL, …).
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// I/O error reading a local file for upload.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// An operation with a deadline ran out of time. Distinct from `Decode`
    /// so a caller can retry a timeout without also retrying a malformed
    /// payload.
    #[error("timeout: {0}")]
    Timeout(String),
}

impl IntrospectionAPIError {
    /// Build an `Http` variant for a response that carried no error
    /// envelope to read a `code` or a `Retry-After` out of.
    pub(crate) fn http(
        status: u16,
        message: impl Into<String>,
        request_id: Option<String>,
        body: Option<serde_json::Value>,
    ) -> Self {
        Self::Http {
            status,
            message: message.into(),
            code: None,
            request_id,
            body,
            retry_after: None,
        }
    }

    /// HTTP status code, if this is an `Http` variant.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// `X-Request-Id` header value, if this is an `Http` variant.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Http { request_id, .. } => request_id.as_deref(),
            _ => None,
        }
    }

    /// Machine-readable error code from the response body, if the DP sent
    /// one. `"runner_expired"` on a `401` means the Runner's session token
    /// has aged out and the runner needs refreshing, which is otherwise
    /// indistinguishable from a bad API key.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Http { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    /// How long the server asked the caller to wait, from `Retry-After`.
    ///
    /// Present on a `429` (and any other status that carried the header)
    /// once the transparent retry budget is spent, so a caller scheduling
    /// its own retry uses the server's number rather than guessing.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Parsed response body, if this is an `Http` variant.
    pub fn body(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Http { body, .. } => body.as_ref(),
            _ => None,
        }
    }
}

/// Result alias used throughout the REST API surface.
pub type ApiResult<T> = std::result::Result<T, IntrospectionAPIError>;
