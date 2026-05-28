//! Shared error type for the REST API namespaces.
//!
//! Raised by every method on [`crate::Runner::tasks`] and
//! [`crate::Runner::files`] (via the runner-bound `tasks` / `files`
//! namespaces), as well as the CP-side [`crate::Runtimes`] /
//! [`crate::Experiments`] / [`crate::Recipes`] / [`crate::Projects`]
//! resources. The OTLP paths (`track` / `feedback` / `identify` on
//! `crate::otel::IntrospectionLogs`) keep returning
//! [`crate::IntrospectionError`].

use thiserror::Error;

/// HTTP error from the Introspection DP REST API.
///
/// Mirrors the shape of the JS `IntrospectionAPIError` and Python
/// `IntrospectionAPIError`: a status code, optional machine-readable code,
/// optional request ID (from `X-Request-Id`), and the raw response body
/// (parsed JSON when the response was JSON, else the text).
#[derive(Error, Debug)]
pub enum IntrospectionAPIError {
    /// Non-2xx HTTP response from the DP.
    #[error("{message} (status={status})")]
    Http {
        message: String,
        status: u16,
        code: Option<String>,
        request_id: Option<String>,
        body: Option<serde_json::Value>,
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
}

impl IntrospectionAPIError {
    /// Build an `Http` variant.
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
