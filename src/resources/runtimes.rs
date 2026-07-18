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
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
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
    /// Queries `GET /v1/runtimes?runtime=…&only_active=true` and returns a
    /// handle to the first match. The server infers the project from the
    /// API token. Returns `IntrospectionAPIError::Http` with status 404
    /// if no active runtime with that runtime group slug or ID exists.
    pub async fn resolve(&self, runtime: &str) -> ApiResult<RuntimeHandle> {
        let mut paginator = self.list(&RuntimeListParams {
            runtime: Some(runtime.into()),
            only_active: Some(true),
            limit: Some(1),
            ..Default::default()
        });
        let runtime = paginator
            .next_page()
            .await?
            .and_then(|p| p.records.into_iter().next())
            .ok_or_else(|| IntrospectionAPIError::Http {
                message: format!("no active runtime '{runtime}'"),
                status: 404,
                code: None,
                request_id: None,
                body: None,
            })?;
        Ok(self.handle(runtime.id))
    }

    /// Build a [`RuntimeHandle`] for `runtime_id`. The handle is the
    /// surface used to call `.run(...)`.
    pub fn handle(&self, runtime_id: Uuid) -> RuntimeHandle {
        RuntimeHandle::new(self.http.clone(), runtime_id)
    }
}

/// Handle returned by `client.runtimes().handle(id)`. Opens a [`Runner`] via
/// [`Self::run`]. Runtime lifecycle and version selection are managed by the
/// CLI and platform.
#[derive(Clone)]
pub struct RuntimeHandle {
    http: Arc<HttpClient>,
    runtime_id: Uuid,
}

impl RuntimeHandle {
    pub(crate) fn new(http: Arc<HttpClient>, runtime_id: Uuid) -> Self {
        Self { http, runtime_id }
    }

    pub fn id(&self) -> Uuid {
        self.runtime_id
    }

    /// `POST /v1/runtimes/{id}/run` — open a [`Runner`] for this runtime.
    pub async fn run(&self, ctx: RunRequest) -> ApiResult<Runner> {
        let path = format!("/v1/runtimes/{}/run", self.runtime_id);
        let spec: RunnerSpec = self.http.post_json(&path, &ctx).await?;
        let source = RunnerSource::Runtime {
            cp_http: self.http.clone(),
            runtime_id: self.runtime_id,
            ctx,
        };
        Runner::from_spec(spec, source)
    }
}
