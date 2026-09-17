//! `client.repositories` (CP) — repository lookup.
//!
//! Read-only: a runner resolves the repository behind the recipe it runs, it
//! does not register one. Linking a repository to a project is a
//! project-authoring act and lives in the CLI.
//!
//! Unlike the other CP lists, `GET /v1/repositories` answers a bare JSON
//! array rather than the cursor envelope, so [`Repositories::list`] returns a
//! `Vec` and there is no paginator.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::schemas::{Repository, RepositoryListParams, StringOrUuid};

#[derive(Serialize)]
struct ProjectQuery {
    project: StringOrUuid,
}

/// `client.repositories` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Repositories {
    http: Arc<HttpClient>,
}

impl Repositories {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/repositories?project=…[&slug=…]` — the repositories linked to
    /// the project, as one array.
    pub async fn list(&self, params: &RepositoryListParams) -> ApiResult<Vec<Repository>> {
        self.http.get_json("/v1/repositories", params).await
    }

    /// `GET /v1/repositories/{id}?project=…`.
    pub async fn get(
        &self,
        repository_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Repository> {
        let path = format!("/v1/repositories/{repository_id}");
        self.http
            .get_json(
                &path,
                &ProjectQuery {
                    project: project.into(),
                },
            )
            .await
    }
}
