//! `client.projects` (CP) — the org's projects, addressed by slug or id.
//!
//! Gated on `projects:read` / `projects:write` / `projects:delete`; a write
//! needs an org credential. A delete also revokes the project's API keys and
//! sessions.

use std::sync::Arc;

use serde::Serialize;

use crate::api::encoding::encode;
use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Project, ProjectCreateParams, ProjectListParams, ProjectUpdateParams, StringOrUuid,
};

/// `client.projects` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Projects {
    http: Arc<HttpClient>,
}

impl Projects {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/projects` — paginated, the credential's org only.
    pub fn list(&self, params: &ProjectListParams) -> Paginator<Project> {
        Paginator::new(self.http.clone(), "/v1/projects", params)
            .expect("ProjectListParams must serialize to a JSON object")
    }

    /// `GET /v1/projects/{project}`, by slug or id. A project outside the
    /// credential's org answers 404.
    pub async fn get(&self, project: impl Into<StringOrUuid>) -> ApiResult<Project> {
        #[derive(Serialize)]
        struct Q {}
        self.http
            .get_json(&project_path(project.into()), &Q {})
            .await
    }

    /// `POST /v1/projects` — on a deployment that can take new projects, and
    /// within the plan's project limit (403 past it).
    pub async fn create(&self, params: &ProjectCreateParams) -> ApiResult<Project> {
        self.http.post_json("/v1/projects", params).await
    }

    /// `PATCH /v1/projects/{project}`.
    pub async fn update(
        &self,
        project: impl Into<StringOrUuid>,
        params: &ProjectUpdateParams,
    ) -> ApiResult<Project> {
        self.http
            .patch_json(&project_path(project.into()), params)
            .await
    }

    /// `DELETE /v1/projects/{project}`.
    pub async fn delete(&self, project: impl Into<StringOrUuid>) -> ApiResult<()> {
        self.http.delete_empty(&project_path(project.into())).await
    }
}

fn project_path(project: StringOrUuid) -> String {
    format!("/v1/projects/{}", encode(&project.to_string()))
}
