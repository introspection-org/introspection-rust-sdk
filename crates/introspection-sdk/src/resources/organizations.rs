//! `client.organizations` (CP) — the organization the credential belongs to.

use std::sync::Arc;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::schemas::Organization;

/// `client.organizations` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Organizations {
    http: Arc<HttpClient>,
}

impl Organizations {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/organizations/me` — the credential's organization. Needs
    /// `members:read`.
    pub async fn me(&self) -> ApiResult<Organization> {
        self.http.get_json("/v1/organizations/me", &()).await
    }
}
