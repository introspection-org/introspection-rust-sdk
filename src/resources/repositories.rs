//! `client.repositories` — repository lookup (CP), and contents and commit
//! reads and branch merges (DP).
//!
//! A runner resolves the repository behind the recipe it runs, it does not
//! register one. Linking a repository to a project is a project-authoring act
//! and lives in the CLI.
//!
//! Unlike the other CP lists, `GET /v1/repositories` answers a bare JSON
//! array rather than the cursor envelope, so [`Repositories::list`] returns a
//! `Vec` and there is no paginator.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::encoding::encode;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    CommitsQuery, ContentsQuery, Paginated, Repository, RepositoryCommit, RepositoryCommitDetail,
    RepositoryContent, RepositoryEntry, RepositoryListParams, RepositoryMergeCommit,
    RepositoryMergeCreate, StringOrUuid,
};

#[derive(Serialize)]
struct ProjectQuery {
    project: StringOrUuid,
}

/// `client.repositories` namespace. Repository rows come from the Control
/// Plane; their contents, commits and merges go through the Data Plane.
#[derive(Clone)]
pub struct Repositories {
    cp_http: Arc<HttpClient>,
    dp_http: Arc<HttpClient>,
}

impl Repositories {
    #[doc(hidden)]
    pub fn new(cp_http: Arc<HttpClient>, dp_http: Arc<HttpClient>) -> Self {
        Self { cp_http, dp_http }
    }

    /// `GET /v1/repositories?project=…[&slug=…]` — the repositories linked to
    /// the project, as one array.
    pub async fn list(&self, params: &RepositoryListParams) -> ApiResult<Vec<Repository>> {
        self.cp_http.get_json("/v1/repositories", params).await
    }

    /// `GET /v1/repositories/{id}?project=…`.
    pub async fn get(
        &self,
        repository_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Repository> {
        let path = format!("/v1/repositories/{repository_id}");
        self.cp_http
            .get_json(
                &path,
                &ProjectQuery {
                    project: project.into(),
                },
            )
            .await
    }

    /// `GET /v1/repositories/{id}/commits` on the Data Plane — the history
    /// reachable from `query.sha`, newest first, following the `next` cursor.
    pub fn commits(
        &self,
        repository_id: Uuid,
        query: &CommitsQuery,
    ) -> ApiResult<Paginator<RepositoryCommit>> {
        let path = format!("/v1/repositories/{repository_id}/commits");
        Paginator::with_cursor_param(self.dp_http.clone(), path, query, "cursor")
    }

    /// `GET /v1/repositories/{id}/commits/{sha}` on the Data Plane — one
    /// commit with its changed files and unified diff. `sha` may be a branch
    /// or tag, which reads its head commit.
    pub async fn commit(
        &self,
        repository_id: Uuid,
        sha: &str,
    ) -> ApiResult<RepositoryCommitDetail> {
        // A URL parser resolves `.` / `..` even percent-encoded, so the request
        // would silently read a different path.
        if sha.is_empty() || sha == "." || sha == ".." {
            return Err(IntrospectionAPIError::InvalidConfig(format!(
                "invalid commit sha {sha:?}"
            )));
        }
        let path = format!("/v1/repositories/{repository_id}/commits/{}", encode(sha));
        self.dp_http.get_json(&path, &()).await
    }

    /// `POST /v1/repositories/{id}/merges` on the Data Plane
    /// (`repositories:write`) — merges `head` into `base`, like GitHub's
    /// merges API. `Ok(None)` is `204`: `base` already contains `head`.
    ///
    /// `404` (repository or `head` not found), `409` (conflict; nothing
    /// changed), `502` (the merge failed) and `504` (still running after 60s)
    /// surface as [`IntrospectionAPIError::Http`]. The call is idempotent on
    /// repository + `base` + `head`, so re-sending after a `504` attaches to
    /// the running merge.
    pub async fn merge(
        &self,
        repository_id: Uuid,
        merge: &RepositoryMergeCreate,
    ) -> ApiResult<Option<RepositoryMergeCommit>> {
        let path = format!("/v1/repositories/{repository_id}/merges");
        self.dp_http.post_json_or_no_content(&path, merge).await
    }

    /// The files of one repository, read through the Data Plane.
    pub fn contents(&self, repository_id: Uuid) -> RepositoryContents {
        RepositoryContents {
            http: self.dp_http.clone(),
            repository_id,
        }
    }
}

/// `client.repositories().contents(id)` — `GET
/// /v1/repositories/{id}/contents/{path}` on the Data Plane.
#[derive(Clone)]
pub struct RepositoryContents {
    http: Arc<HttpClient>,
    repository_id: Uuid,
}

impl RepositoryContents {
    /// Every entry of the directory at `query.path`, following the `next`
    /// cursor. Yields an error if the path is a file.
    pub fn list(&self, query: &ContentsQuery) -> ApiResult<Paginator<RepositoryEntry>> {
        let path = self.contents_path(&query.path)?;
        Paginator::with_decoder(self.http.clone(), path, query, "cursor", directory_page)
    }

    /// A directory's first page or a file's content at `path`. `query.path` is
    /// not read.
    pub async fn get(&self, path: &str, query: &ContentsQuery) -> ApiResult<RepositoryContent> {
        let path = self.contents_path(path)?;
        self.http.get_json(&path, query).await
    }

    fn contents_path(&self, path: &str) -> ApiResult<String> {
        let base = format!("/v1/repositories/{}/contents", self.repository_id);
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return Ok(base);
        }
        let mut out = base;
        for segment in trimmed.split('/') {
            // A URL parser resolves `.` / `..` even percent-encoded, so the
            // request would silently read a different path.
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(IntrospectionAPIError::InvalidConfig(format!(
                    "invalid repository path {path:?}"
                )));
            }
            out.push('/');
            out.push_str(&encode(segment));
        }
        Ok(out)
    }
}

fn directory_page(body: serde_json::Value) -> ApiResult<Paginated<RepositoryEntry>> {
    let content: RepositoryContent = serde_json::from_value(body).map_err(|e| {
        IntrospectionAPIError::Decode(format!("failed to decode repository contents: {e}"))
    })?;
    match content {
        RepositoryContent::Dir(dir) => Ok(Paginated {
            records: dir.records,
            count: dir.count,
            total_count: None,
            next: dir.next,
        }),
        RepositoryContent::File(file) => Err(IntrospectionAPIError::Decode(format!(
            "{:?} is a file, not a directory; read it with RepositoryContents::get",
            file.path
        ))),
    }
}
