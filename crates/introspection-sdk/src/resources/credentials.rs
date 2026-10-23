//! `client.credentials` (CP) — secrets a project's runtimes read, scoped to
//! the project, a runtime group, a runtime or one lane.
//!
//! The secret is write-only: no read returns it. Gated on
//! `credentials:read` / `credentials:write` / `credentials:delete`, which only
//! an org credential carries.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Credential, CredentialCreateParams, CredentialUpdateParams, ProjectSelector,
    RuntimeSettingListParams, StringOrUuid,
};
use crate::resources::with_project;

/// `client.credentials` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Credentials {
    http: Arc<HttpClient>,
}

impl Credentials {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/credentials` — paginated.
    pub fn list(&self, params: &RuntimeSettingListParams) -> Paginator<Credential> {
        Paginator::new(self.http.clone(), "/v1/credentials", params)
            .expect("RuntimeSettingListParams must serialize to a JSON object")
    }

    /// `GET /v1/credentials/{id}`.
    pub async fn get(
        &self,
        credential_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Credential> {
        let path = format!("/v1/credentials/{credential_id}");
        self.http
            .get_json(&path, &ProjectSelector::new(project))
            .await
    }

    /// `POST /v1/credentials`.
    pub async fn create(
        &self,
        params: &CredentialCreateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Credential> {
        self.http
            .post_json(&with_project("/v1/credentials", project.as_ref()), params)
            .await
    }

    /// `PATCH /v1/credentials/{id}` — set `secret` to rotate it.
    pub async fn update(
        &self,
        credential_id: Uuid,
        params: &CredentialUpdateParams,
        project: Option<StringOrUuid>,
    ) -> ApiResult<Credential> {
        let path = with_project(
            &format!("/v1/credentials/{credential_id}"),
            project.as_ref(),
        );
        self.http.patch_json(&path, params).await
    }

    /// `DELETE /v1/credentials/{id}`.
    pub async fn delete(
        &self,
        credential_id: Uuid,
        project: Option<StringOrUuid>,
    ) -> ApiResult<()> {
        let path = with_project(
            &format!("/v1/credentials/{credential_id}"),
            project.as_ref(),
        );
        self.http.delete_empty(&path).await
    }
}
