//! `issues()` (DP) — a project's pursuits, each with a living brief, its
//! evidence, and one fixed worker Task, on the client or a
//! [`Runner`](crate::Runner).
//!
//! An issue is created on its worker Task and starts `open`. Edits name the
//! [`Issue::revision`] they were made against
//! ([`IssueUpdateParams::expected_revision`]); a stale one answers 409, so
//! read the issue again rather than resending. Each evidence list (`files`,
//! `links`, `events`, `spans`), `tags` and `metadata` replaces the stored one
//! wholesale. A human request on the issue is the other `PATCH` body,
//! [`IssueMutation`]: create one at revision 0, or change one at its own
//! revision. Request history is read through [`crate::Events`]
//! (`introspection.issue` with `request=true`).
//!
//! Every write carries an `Idempotency-Key`. Pass your own to keep one
//! deliberate write a single attempt across your own retries; without one,
//! each call mints a fresh key, which this client resends unchanged on its
//! transport retries.
//!
//! These are Data Plane routes, gated on `issues:read` / `issues:write`, and
//! `issues:delete` to delete.
//!
//! [`IssueUpdateParams::expected_revision`]: crate::IssueUpdateParams::expected_revision
//! [`IssueMutation`]: crate::IssueMutation

use std::sync::Arc;

use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Issue, IssueCreateParams, IssueListParams, IssuePatch};

/// `issues()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Issues {
    http: Arc<HttpClient>,
}

/// The caller's key, or a fresh UUID7 for one write.
fn key_or_fresh(idempotency_key: Option<&str>) -> String {
    idempotency_key
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::now_v7().to_string())
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

    /// `GET /v1/issues/{id}`, with its evidence, open requests and current
    /// `revision`.
    pub async fn get(&self, issue_id: Uuid) -> ApiResult<Issue> {
        let path = format!("/v1/issues/{issue_id}");
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/issues` — open an issue on its worker Task.
    pub async fn create(
        &self,
        params: &IssueCreateParams,
        idempotency_key: Option<&str>,
    ) -> ApiResult<Issue> {
        let key = key_or_fresh(idempotency_key);
        self.http.post_json_keyed("/v1/issues", params, &key).await
    }

    /// `PATCH /v1/issues/{id}` — edit the brief ([`IssueUpdateParams`]) or
    /// create or change one human request ([`IssueMutation`]). Answers with
    /// the issue as it now stands.
    ///
    /// [`IssueUpdateParams`]: crate::IssueUpdateParams
    /// [`IssueMutation`]: crate::IssueMutation
    pub async fn update(
        &self,
        issue_id: Uuid,
        patch: impl Into<IssuePatch>,
        idempotency_key: Option<&str>,
    ) -> ApiResult<Issue> {
        let path = format!("/v1/issues/{issue_id}");
        let key = key_or_fresh(idempotency_key);
        self.http.patch_json_keyed(&path, &patch.into(), &key).await
    }

    /// `DELETE /v1/issues/{id}` — soft delete; its activity history is kept.
    /// Needs `issues:delete`.
    pub async fn delete(&self, issue_id: Uuid, idempotency_key: Option<&str>) -> ApiResult<()> {
        let path = format!("/v1/issues/{issue_id}");
        let key = key_or_fresh(idempotency_key);
        self.http.delete_empty_keyed(&path, &key).await
    }
}
