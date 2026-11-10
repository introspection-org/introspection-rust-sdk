//! `issues()` (DP) — a project's pursuits, each with a living brief, its
//! people, its evidence, and one fixed worker Task, on the client or a
//! [`Runner`](crate::Runner).
//!
//! An issue is created on its worker Task and starts `open`. An edit applies
//! the fields it names and the last write wins; pass the issue's
//! [`Issue::etag`] as `If-Match` to refuse the edit (412) when the issue has
//! changed since it was read. Each evidence list (`files`, `links`, `events`,
//! `spans`), `tags`, `metadata` and the followers replace the stored ones
//! wholesale.
//!
//! An issue has an assignee, a reviewer and followers. To ask a person
//! something, assign the issue to them, set it `waiting` and comment
//! ([`IssueComments::create`]). A read returns the brief; the people, the
//! evidence and the comments come with [`IssueInclude`]. The change history
//! is read through [`crate::Events`] (`introspection.issue` with `issue_id`).
//!
//! These are Data Plane routes, gated on `issues:read` / `issues:write`, and
//! `issues:delete` to delete. `issues()` is an inherent method on each holder,
//! not on [`crate::DataPlaneResources`]: adding a method to that public trait
//! breaks its outside implementors, so it joins the trait at the next major.

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Issue, IssueComment, IssueCreateParams, IssueInclude, IssueListParams, IssueUpdateParams,
};

/// `issues()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Issues {
    http: Arc<HttpClient>,
    /// Comments on an issue (`/v1/issues/{id}/comments`).
    pub comments: IssueComments,
}

/// `issues().comments` — comment on an issue. Comments are read back on the
/// issue with [`IssueInclude::Comments`], or from its events.
#[derive(Clone)]
pub struct IssueComments {
    http: Arc<HttpClient>,
}

impl HttpClient {
    /// `/v1/issues` on this Data Plane client's own credential.
    pub fn issues(self: &Arc<Self>) -> Issues {
        Issues::new(self.clone())
    }
}

/// `path` with one `include` query key per value.
fn with_include(path: &str, include: &[IssueInclude]) -> String {
    let mut path = path.to_string();
    for (index, value) in include.iter().enumerate() {
        path.push(if index == 0 { '?' } else { '&' });
        path.push_str("include=");
        path.push_str(value.as_str());
    }
    path
}

impl Issues {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            comments: IssueComments { http: http.clone() },
            http,
        }
    }

    /// `GET /v1/issues` — paginated, newest activity first.
    pub fn list(&self, params: &IssueListParams) -> Paginator<Issue> {
        Paginator::new(self.http.clone(), "/v1/issues", params)
            .expect("IssueListParams must serialize to a JSON object")
    }

    /// `GET /v1/issues/{id}` — the brief, with the lists `include` names.
    pub async fn get(&self, issue_id: Uuid, include: &[IssueInclude]) -> ApiResult<Issue> {
        let path = with_include(&format!("/v1/issues/{issue_id}"), include);
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/issues` — open an issue on its worker Task. Repeating it
    /// with the same content returns the Task's issue.
    pub async fn create(
        &self,
        params: &IssueCreateParams,
        include: &[IssueInclude],
    ) -> ApiResult<Issue> {
        self.http
            .post_json(&with_include("/v1/issues", include), params)
            .await
    }

    /// `PATCH /v1/issues/{id}` — edit the brief and who is on the issue.
    /// Answers with the issue as it now stands. `if_match` is an
    /// [`Issue::etag`].
    pub async fn update(
        &self,
        issue_id: Uuid,
        params: &IssueUpdateParams,
        include: &[IssueInclude],
        if_match: Option<&str>,
    ) -> ApiResult<Issue> {
        let path = with_include(&format!("/v1/issues/{issue_id}"), include);
        self.http.patch_json_if_match(&path, params, if_match).await
    }

    /// `DELETE /v1/issues/{id}` — soft delete; its activity history is kept.
    /// Needs `issues:delete`.
    pub async fn delete(&self, issue_id: Uuid, if_match: Option<&str>) -> ApiResult<()> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.delete_empty_if_match(&path, if_match).await
    }
}

impl IssueComments {
    /// `POST /v1/issues/{id}/comments` — comment on an issue. It does not
    /// edit the issue: its `updated_at`, `revision` and etag are unchanged. A
    /// read right after may not include the comment yet.
    pub async fn create(&self, issue_id: Uuid, comment: &str) -> ApiResult<IssueComment> {
        let path = format!("/v1/issues/{issue_id}/comments");
        self.http
            .post_json(&path, &serde_json::json!({ "comment": comment }))
            .await
    }
}
