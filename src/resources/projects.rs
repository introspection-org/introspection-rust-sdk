//! `client.projects` (CP) — read the org's projects.
//!
//! Read-only. Creating, renaming or deleting a project is project
//! administration and lives in the CLI; reading one is what a tool needs to
//! turn a project id into the slug its dashboard URLs use.
//!
//! Both calls need `projects:read`.

use std::sync::Arc;

use serde::Serialize;

use crate::api::encoding::encode;
use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Project, ProjectListParams, StringOrUuid};

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
        let path = format!("/v1/projects/{}", encode(&project.into().to_string()));
        self.http.get_json(&path, &Q {}).await
    }
}
