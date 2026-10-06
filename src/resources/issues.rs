//! `issues()` (DP) — project pursuits, each with a living brief, a fixed
//! worker task, and the human requests raised on it.
//!
//! Data Plane routes gated on `issues:read`, `issues:write` and
//! `issues:delete`. A runner a member opens for themself carries
//! `issues:read` / `issues:write`. Writes are applied by the Data Plane
//! worker, so each returns the issue as it stands once the change is applied.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Issue, IssueCreate, IssueListParams, IssueRequestUpdate, IssueUpdate};

/// `issues()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Issues {
    http: Arc<HttpClient>,
}

impl Issues {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/issues` — paginated, newest activity first.
    pub fn list(&self, params: &IssueListParams) -> Paginator<Issue> {
        Paginator::new(self.http.clone(), "/v1/issues", params)
            .expect("IssueListParams must serialize to a JSON object")
    }

    /// `POST /v1/issues`.
    pub async fn create(&self, params: &IssueCreate) -> ApiResult<Issue> {
        self.http.post_json("/v1/issues", params).await
    }

    /// `GET /v1/issues/{id}`.
    pub async fn get(&self, issue_id: Uuid) -> ApiResult<Issue> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.get_json(&path, &()).await
    }

    /// `PATCH /v1/issues/{id}` — edit the brief at
    /// [`IssueUpdate::expected_revision`]. Only the fields set are sent.
    pub async fn update(&self, issue_id: Uuid, params: &IssueUpdate) -> ApiResult<Issue> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.patch_json(&path, params).await
    }

    /// `PATCH /v1/issues/{id}` with `{"request": ...}` — create or change one
    /// human request on the issue.
    pub async fn update_request(
        &self,
        issue_id: Uuid,
        request: &IssueRequestUpdate,
    ) -> ApiResult<Issue> {
        #[derive(Serialize)]
        struct Body<'a> {
            request: &'a IssueRequestUpdate,
        }
        let path = format!("/v1/issues/{issue_id}");
        self.http.patch_json(&path, &Body { request }).await
    }

    /// `DELETE /v1/issues/{id}` — soft delete; needs `issues:delete`.
    pub async fn delete(&self, issue_id: Uuid) -> ApiResult<()> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.delete_empty(&path).await
    }
}
