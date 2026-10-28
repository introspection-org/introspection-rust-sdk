//! `client.tokens` (CP) — mint a Data Plane token for a project.

use std::sync::Arc;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::schemas::{DataPlaneToken, TokenCreateParams};

/// `client.tokens` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Tokens {
    http: Arc<HttpClient>,
}

impl Tokens {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `POST /v1/tokens` — a Data Plane JWT for one project, carrying the
    /// caller's scopes (`tokens:write`). A member's token is the one
    /// credential that carries `git:push` to a hosted repository.
    pub async fn create(&self, params: &TokenCreateParams) -> ApiResult<DataPlaneToken> {
        self.http.post_json("/v1/tokens", params).await
    }
}
