//! `client.oidc` (CP) — who the client's credential signs in as.

use std::sync::Arc;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::schemas::Me;

/// `client.oidc` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Oidc {
    http: Arc<HttpClient>,
}

impl Oidc {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/oidc/me` — the member and org a member session or access
    /// token belongs to, with the org's resolved feature flags.
    pub async fn me(&self) -> ApiResult<Me> {
        self.http.get_json("/v1/oidc/me", &()).await
    }
}
