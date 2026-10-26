//! `client.runtimes` (CP) — read, resolve, update, and run runtimes, and
//! with the `authoring` feature create and delete them.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::encoding::encode;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
#[cfg(feature = "authoring")]
use crate::api::schemas::RuntimeCreateParams;
use crate::api::schemas::{
    RunRequest, RunnerSpec, Runtime, RuntimeListParams, RuntimeUpdateParams, StringOrUuid,
};
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

    /// `GET /v1/runtimes/{id}[?project=...]`. `project` is needed only when
    /// the credential does not already name one; see [`Self::update`].
    pub async fn get(&self, runtime_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<Runtime> {
        #[derive(Serialize)]
        struct Q {
            #[serde(skip_serializing_if = "Option::is_none")]
            project: Option<StringOrUuid>,
        }
        let path = format!("/v1/runtimes/{}", runtime_id);
        self.http.get_json(&path, &Q { project }).await
    }

    /// `PATCH /v1/runtimes/{id}` — steer the group's lanes, yank or restore
    /// this version, or edit its settings. Needs `runtimes:write`.
    ///
    /// `project` is needed only when the credential does not already name
    /// one: an API key or a project-scoped access token carries its project
    /// as a claim, while a session cookie or an org-wide token does not, and
    /// without one the route answers 422.
    ///
    /// Only the fields set on `params` are sent; the rest stay unchanged.
    /// Lanes move through [`RuntimeUpdateParams::environment_ref`]; the
    /// constructors [`RuntimeUpdateParams::pin_ref`],
    /// [`RuntimeUpdateParams::unpin`],
    /// [`RuntimeUpdateParams::pin_staging_to_this_version`],
    /// [`RuntimeUpdateParams::yank`] and [`RuntimeUpdateParams::unyank`]
    /// build the common requests. A malformed ref or an `environment` other
    /// than `staging` answers 400, an unknown id 404, and a concurrent lane
    /// change 409, which is safe to retry.
    pub async fn update(
        &self,
        runtime_id: Uuid,
        project: Option<StringOrUuid>,
        params: &RuntimeUpdateParams,
    ) -> ApiResult<Runtime> {
        let mut path = format!("/v1/runtimes/{}", runtime_id);
        if let Some(project) = project {
            path.push_str("?project=");
            path.push_str(&encode(&project.to_string()));
        }
        self.http.patch_json(&path, params).await
    }

    /// `POST /v1/runtimes` — create a Git runtime from a Recipe. Needs
    /// `runtimes:write` and a member identity (a member login or the
    /// project's service account), so a project API key gets 403.
    ///
    /// The Recipe's repository must be one the project may read. A slug the
    /// project already has answers 409; so does `bootstrap` when the group
    /// already has a live version.
    #[cfg(feature = "authoring")]
    pub async fn create(&self, params: &RuntimeCreateParams) -> ApiResult<Runtime> {
        self.http.post_json("/v1/runtimes", params).await
    }

    /// `DELETE /v1/runtimes/{id}[?project=...]` — soft-delete the runtime
    /// group and every version of it; `runtime_id` may name any of them.
    /// Needs `runtimes:delete`. `project` is needed only when the credential
    /// does not already name one; see [`Self::update`].
    #[cfg(feature = "authoring")]
    pub async fn delete(&self, runtime_id: Uuid, project: Option<StringOrUuid>) -> ApiResult<()> {
        let path =
            crate::resources::with_project(&format!("/v1/runtimes/{runtime_id}"), project.as_ref());
        self.http.delete_empty(&path).await
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
