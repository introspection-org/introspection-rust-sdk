//! `client.runtimes` (CP) — runtime CRUD + `runtime(id).run()` to open
//! a [`crate::Runner`].

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    Recipe, RunRequest, RunnerSpec, Runtime, RuntimeCreate, RuntimeListParams, RuntimeUpdate,
    StringOrUuid,
};
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

    /// `POST /v1/runtimes`.
    pub async fn create(&self, body: &RuntimeCreate) -> ApiResult<Runtime> {
        self.http.post_json("/v1/runtimes", body).await
    }

    /// `PATCH /v1/runtimes/{id}?project=...`.
    pub async fn update(
        &self,
        runtime_id: Uuid,
        project: impl Into<StringOrUuid>,
        body: &RuntimeUpdate,
    ) -> ApiResult<Runtime> {
        let path = format!("/v1/runtimes/{}?project={}", runtime_id, project.into());
        self.http.patch_json(&path, body).await
    }

    /// `DELETE /v1/runtimes/{id}?project=...`.
    pub async fn delete(
        &self,
        runtime_id: Uuid,
        project: impl Into<StringOrUuid>,
    ) -> ApiResult<()> {
        let path = format!("/v1/runtimes/{}?project={}", runtime_id, project.into());
        self.http.delete_empty(&path).await
    }

    /// Look up a runtime by slug or id and return a [`RuntimeHandle`].
    ///
    /// Queries `GET /v1/runtimes?runtime=…&only_active=true` and returns a
    /// handle to the first match. The server infers the project from the
    /// API token. Returns `IntrospectionAPIError::Http` with status 404
    /// if no active runtime with that slug or id exists.
    pub async fn resolve(&self, runtime: &str) -> ApiResult<RuntimeHandle> {
        let mut paginator = self.list(&RuntimeListParams {
            runtime: Some(runtime.to_string()),
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
    /// surface used to call `.run(...)` / `.activate(...)` / `.pin(...)`.
    pub fn handle(&self, runtime_id: Uuid) -> RuntimeHandle {
        RuntimeHandle::new(self.http.clone(), runtime_id)
    }
}

/// Handle returned by `client.runtime(id)`. Opens a [`Runner`] via
/// [`Self::run`] or activates the row via [`Self::activate`]. Use
/// [`Self::pin`] to derive a child handle that opens runners against a
/// specific historical recipe (the "canary a previous version" flow).
#[derive(Clone)]
pub struct RuntimeHandle {
    http: Arc<HttpClient>,
    runtime_id: Uuid,
    recipe_id: Option<Uuid>,
}

impl RuntimeHandle {
    pub(crate) fn new(http: Arc<HttpClient>, runtime_id: Uuid) -> Self {
        Self {
            http,
            runtime_id,
            recipe_id: None,
        }
    }

    pub fn id(&self) -> Uuid {
        self.runtime_id
    }

    /// The recipe this handle is pinned to, if any. Set by
    /// [`Self::pin`]; unset on handles minted directly via
    /// `client.runtime(id)`.
    pub fn pinned_recipe_id(&self) -> Option<Uuid> {
        self.recipe_id
    }

    /// `POST /v1/runtimes/{id}/activate`.
    pub async fn activate(&self, project: Option<impl Into<StringOrUuid>>) -> ApiResult<Runtime> {
        let path = match project {
            Some(project) => format!(
                "/v1/runtimes/{}/activate?project={}",
                self.runtime_id,
                project.into()
            ),
            None => format!("/v1/runtimes/{}/activate", self.runtime_id),
        };
        self.http.post_json(&path, &serde_json::json!({})).await
    }

    /// Pin this handle to a specific recipe. Returns a child handle
    /// (the original is left unchanged). Subsequent [`Self::run`] calls
    /// open a runner against the runtime row in this name whose
    /// `recipe_id == recipe.id` — CP resolves the matching row
    /// server-side via the `recipe_id` field on [`RunRequest`].
    pub fn pin(&self, recipe: impl Into<RecipePin>) -> RuntimeHandle {
        let pin = recipe.into();
        RuntimeHandle {
            http: self.http.clone(),
            runtime_id: self.runtime_id,
            recipe_id: Some(pin.recipe_id()),
        }
    }

    /// `POST /v1/runtimes/{id}/run` — open a [`Runner`] pinned to this
    /// runtime row. If this handle was returned by [`Self::pin`], the
    /// pinned `recipe_id` is set on the [`RunRequest`] body and CP
    /// resolves the matching runtime row server-side.
    pub async fn run(&self, mut ctx: RunRequest) -> ApiResult<Runner> {
        if let Some(rid) = self.recipe_id {
            ctx.recipe_id = Some(rid);
        }
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

/// Accepted by [`RuntimeHandle::pin`]. Carries the recipe id only — the
/// SDK never round-trips the full [`Recipe`] to CP, just its id. The
/// `Recipe` variant is boxed to keep the enum compact.
#[derive(Debug, Clone)]
pub enum RecipePin {
    Recipe(Box<Recipe>),
    Id(Uuid),
}

impl RecipePin {
    fn recipe_id(&self) -> Uuid {
        match self {
            Self::Recipe(r) => r.id,
            Self::Id(id) => *id,
        }
    }
}

impl From<Recipe> for RecipePin {
    fn from(r: Recipe) -> Self {
        Self::Recipe(Box::new(r))
    }
}

impl From<&Recipe> for RecipePin {
    fn from(r: &Recipe) -> Self {
        Self::Id(r.id)
    }
}

impl From<Uuid> for RecipePin {
    fn from(id: Uuid) -> Self {
        Self::Id(id)
    }
}

impl From<&str> for RecipePin {
    /// Parses the string as a UUID. Panics on malformed input — use
    /// `RecipePin::Id(Uuid::parse_str(...)?)` for fallible parsing.
    fn from(s: &str) -> Self {
        Self::Id(Uuid::parse_str(s).expect("RecipePin::from(&str) requires a valid UUID string"))
    }
}
