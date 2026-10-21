use bytes::Bytes;
use http::{HeaderValue, Response, StatusCode};
use serde_json::json;

/// An AAuth error, answered as `application/problem+json`
/// (`#error-response-format`).
#[derive(Clone, Debug, thiserror::Error)]
#[error("{error}: {detail}")]
pub struct AAuthError {
    /// The HTTP status.
    pub status: StatusCode,
    /// The error code, such as `invalid_resource_token`.
    pub error: String,
    /// A human-readable explanation.
    pub detail: String,
    /// Headers the response carries, such as `Signature-Error`.
    pub headers: Vec<(&'static str, String)>,
}

impl AAuthError {
    /// An error answered with `status`, `error` and `detail`.
    pub fn new(status: StatusCode, error: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            error: error.into(),
            detail: detail.into(),
            headers: Vec::new(),
        }
    }

    /// A `400` with `error`: a token request this server cannot act on.
    pub(crate) fn bad(error: &str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error, detail)
    }

    /// A `401` naming the signature problem in `Signature-Error`.
    pub(crate) fn signature(code: &str, detail: impl Into<String>) -> Self {
        let mut error = Self::new(StatusCode::UNAUTHORIZED, code, detail);
        error
            .headers
            .push(("signature-error", format!("error={code}")));
        error
    }

    /// The problem response this error is answered with.
    pub fn into_response(self) -> Response<Bytes> {
        let mut response = problem(self.status, &self.error, Some(&self.detail));
        for (name, value) in self.headers {
            if let Ok(value) = HeaderValue::from_str(&value) {
                response.headers_mut().insert(name, value);
            }
        }
        response
    }
}

/// A problem response: `{error, detail?}`.
pub fn problem(status: StatusCode, error: &str, detail: Option<&str>) -> Response<Bytes> {
    let mut body = json!({ "error": error });
    if let Some(detail) = detail.filter(|d| !d.is_empty()) {
        body["detail"] = json!(detail);
    }
    let mut response = Response::new(Bytes::from(body.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/problem+json"),
    );
    response
}
