//! `issues()` (DP) — a project's pursuits, each with a living brief, its
//! evidence, and one fixed worker Task, on the client or a
//! [`Runner`](crate::Runner).
//!
//! An issue is created on its worker Task and starts `open`. An edit applies
//! the fields it names and the last write wins; pass the issue's
//! [`Issue::etag`] as `If-Match` to refuse the edit (412) when the issue has
//! changed since it was read. Each evidence list (`files`, `links`, `events`,
//! `spans`), `tags` and `metadata` replaces the stored one wholesale. A human
//! request on the issue is the other `PATCH` body, [`IssueMutation`]: create
//! one at revision 0, or change one at its own revision. Request history is
//! read through [`crate::Events`] (`introspection.issue` with
//! `request=true`).
//!
//! These are Data Plane routes, gated on `issues:read` / `issues:write`, and
//! `issues:delete` to delete.
//!
//! [`IssueMutation`]: crate::IssueMutation

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Issue, IssueCreateParams, IssueListParams, IssuePatch};

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

    /// `GET /v1/issues/{id}`, with its evidence and open requests.
    pub async fn get(&self, issue_id: Uuid) -> ApiResult<Issue> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/issues` — open an issue on its worker Task. Repeating it
    /// with the same content returns the Task's issue.
    pub async fn create(&self, params: &IssueCreateParams) -> ApiResult<Issue> {
        self.http.post_json("/v1/issues", params).await
    }

    /// `PATCH /v1/issues/{id}` — edit the brief ([`IssueUpdateParams`]) or
    /// create or change one human request ([`IssueMutation`]). Answers with
    /// the issue as it now stands. `if_match` (an [`Issue::etag`]) applies to
    /// a brief edit only.
    ///
    /// [`IssueUpdateParams`]: crate::IssueUpdateParams
    /// [`IssueMutation`]: crate::IssueMutation
    pub async fn update(
        &self,
        issue_id: Uuid,
        patch: impl Into<IssuePatch>,
        if_match: Option<&str>,
    ) -> ApiResult<Issue> {
        let patch = patch.into();
        if if_match.is_some() && matches!(patch, IssuePatch::Mutation(_)) {
            return Err(IntrospectionAPIError::InvalidConfig(
                "If-Match applies to a brief edit; a request change names its own revision"
                    .to_string(),
            ));
        }
        let path = format!("/v1/issues/{issue_id}");
        self.http.patch_json_if_match(&path, &patch, if_match).await
    }

    /// `DELETE /v1/issues/{id}` — soft delete; its activity history is kept.
    /// Needs `issues:delete`.
    pub async fn delete(&self, issue_id: Uuid, if_match: Option<&str>) -> ApiResult<()> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.delete_empty_if_match(&path, if_match).await
    }
}
