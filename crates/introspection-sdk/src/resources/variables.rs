//! `client.variables` (CP) — environment variables a project's runtimes
//! read, scoped like [`crate::Credentials`].
//!
//! The API has no read-one route. Gated on `variables:read` /
//! `variables:write` / `variables:delete`, which only an org credential
//! carries.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    RuntimeSettingListParams, StringOrUuid, Variable, VariableCreateParams, VariableUpdateParams,
};
use crate::resources::with_project;

/// `client.variables` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Variables {
    http: Arc<HttpClient>,
}

impl Variables {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/variables` — paginated.
    pub fn list(&self, params: &RuntimeSettingListParams) -> Paginator<Variable> {
        Paginator::new(self.http.clone(), "/v1/variables", params)
            .expect("RuntimeSettingListParams must serialize to a JSON object")
    }

    /// `POST /v1/variables`.
    pub async fn create(
        &self,
        params: &VariableCreateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Variable> {
        self.http
            .post_json(&with_project("/v1/variables", project.as_ref()), params)
            .await
    }

    /// `PATCH /v1/variables/{id}`.
    pub async fn update(
        &self,
        variable_id: Uuid,
        params: &VariableUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Variable> {
        let path = with_project(&format!("/v1/variables/{variable_id}"), project.as_ref());
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/variables/{id}`.
    pub async fn delete(&self, variable_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<()> {
        let path = with_project(&format!("/v1/variables/{variable_id}"), project.as_ref());
        self.http.delete_empty(&path).await
    }
}
