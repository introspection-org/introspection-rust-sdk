//! `client.runtimes` (CP) — read, resolve, and run runtimes.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{RunRequest, RunnerSpec, Runtime, RuntimeListParams, StringOrUuid};
use crate::runner::{Runner, RunnerSource};

/// `client.runtimes` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Runtimes {
    http: Arc<HttpClient>,
}

impl Runtimes {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/runtimes` — paginated.
    pub fn list(&self, params: &RuntimeListParams) -> Paginator<Runtime> {
        Paginator::new(self.http.clone(), "/v1/runtimes", params)
            .expect("RuntimeListParams must serialize to a JSON object")
    }

    /// `GET /v1/runtimes/{id}?project=...`.
    pub async fn get(
        &self,
        runtime_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<Runtime> {
        #[derive(Serialize)]
        struct Q {
            project: StringOrUuid,
        }
        let path = format!("/v1/runtimes/{}", runtime_id);
        self.http
            .get_json(
                &path,
                &Q {
                    project: project.into(),
                },
            )
            .await
    }

    /// Look up a runtime by runtime group slug or ID and return a [`RuntimeHandle`].
    ///
    /// Queries `GET /v1/runtimes?runtime=…` and returns a handle to the
    /// first match. The route answers newest-first, so a slug published
    /// more than once resolves to the version the group currently
    /// serves. The server infers the project from the API token.
    /// Returns `IntrospectionAPIError::Http` with status 404 if no
    /// runtime with that runtime group slug or ID exists.
    pub async fn resolve(&self, runtime: &str) -> ApiResult<RuntimeHandle> {
        let mut paginator = self.list(&RuntimeListParams {
            runtime: Some(runtime.into()),
            limit: Some(1),
            ..Default::default()
        });
        let runtime = paginator
            .next_page()
            .await?
            .and_then(|p| p.records.into_iter().next())
            .ok_or_else(|| IntrospectionAPIError::Http {
                message: format!("no runtime '{runtime}'"),
                status: 404,
                code: None,
                request_id: None,
                body: None,
                retry_after: None,
            })?;
        Ok(self.handle(runtime.id))
    }

    /// Build a [`RuntimeHandle`] for `runtime_id`. The handle is the
    /// surface used to call `.run(...)`.
    pub fn handle(&self, runtime_id: Uuid) -> RuntimeHandle {
        RuntimeHandle::new(self.http.clone(), runtime_id)
    }

    /// Build a [`RuntimeHandle`] for a Runtime slug, without a request.
    ///
    /// `.run(...)` posts the slug to `POST /v1/runtimes/{slug}/run`, which
    /// resolves it in the caller's project, so a credential refused
    /// `GET /v1/runtimes` (a `customer` signed in by email code) can still
    /// open a runner. [`Runner::refresh`] posts the same path.
    pub fn by_slug(&self, slug: &str) -> RuntimeHandle {
        RuntimeHandle {
            http: self.http.clone(),
            runtime: StringOrUuid::from(slug),
        }
    }
}

/// Handle returned by `client.runtimes().handle(id)`,
/// `client.runtimes().by_slug(slug)` and `client.runtime(..)`. Opens a
/// [`Runner`] via [`Self::run`]. Runtime lifecycle and version selection are
/// managed by the CLI and platform.
#[derive(Clone)]
pub struct RuntimeHandle {
    http: Arc<HttpClient>,
    runtime: StringOrUuid,
}

impl RuntimeHandle {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>, runtime_id: Uuid) -> Self {
        Self {
            http,
            runtime: StringOrUuid::from(runtime_id),
        }
    }

    /// The concrete Runtime id, or `None` for a handle built from a slug,
    /// which the Control Plane resolves on each `run`.
    pub fn id(&self) -> Option<Uuid> {
        match &self.runtime {
            StringOrUuid::Uuid(id) => Some(*id),
            StringOrUuid::String(_) => None,
        }
    }

    /// `POST /v1/runtimes/{id or slug}/run` — open a [`Runner`] for this
    /// runtime.
    pub async fn run(&self, ctx: RunRequest) -> ApiResult<Runner> {
        let path = run_path(&self.runtime);
        let spec: RunnerSpec = self.http.post_json(&path, &ctx).await?;
        let source = RunnerSource::Runtime {
            cp_http: self.http.clone(),
            runtime: self.runtime.clone(),
            ctx,
        };
        Runner::from_spec(spec, source)
    }
}

pub(crate) fn run_path(runtime: &StringOrUuid) -> String {
    format!(
        "/v1/runtimes/{}/run",
        crate::api::encoding::encode(&runtime.to_string())
    )
}
