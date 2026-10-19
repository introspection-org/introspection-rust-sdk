//! `client.integrations` (CP) — the org's own provider installs (a GitHub App
//! installation, a Slack workspace). Org-wide, so no project selector.
//!
//! A create returns a `pending` row (or the live pending one for that type:
//! there is at most one live integration per type), and the provider's
//! activation flow completes it with an update. Gated on
//! `integrations:read` / `integrations:write` / `integrations:delete`, which
//! only an org credential carries.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Integration, IntegrationCreateParams, IntegrationUpdateParams, PaginationParams,
};

/// `client.integrations` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Integrations {
    http: Arc<HttpClient>,
}

impl Integrations {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/integrations` — paginated.
    pub fn list(&self, params: &PaginationParams) -> Paginator<Integration> {
        Paginator::new(self.http.clone(), "/v1/integrations", params)
            .expect("PaginationParams must serialize to a JSON object")
    }

    /// `GET /v1/integrations/{id}`.
    pub async fn get(&self, integration_id: Uuid) -> ApiResult<Integration> {
        let path = format!("/v1/integrations/{integration_id}");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/integrations`. Answers 409 when an active or disabled
    /// integration of the type already exists.
    pub async fn create(&self, params: &IntegrationCreateParams) -> ApiResult<Integration> {
        self.http.post_json("/v1/integrations", params).await
    }

    /// `PATCH /v1/integrations/{id}`.
    pub async fn update(
        &self,
        integration_id: Uuid,
        params: &IntegrationUpdateParams,
    ) -> ApiResult<Integration> {
        let path = format!("/v1/integrations/{integration_id}");
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/integrations/{id}`.
    pub async fn delete(&self, integration_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/integrations/{integration_id}");
        self.http.delete_empty(&path).await
    }
}
