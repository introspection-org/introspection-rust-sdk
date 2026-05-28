//! Introspection Client — REST-only surface.
//!
//! Always available with no OpenTelemetry dependency. Exposes
//! `client.runtimes()` / `client.experiments()` / `client.runtime(id)` /
//! `client.experiment(id, project_id)` accessors over the Introspection
//! DP REST API.
//!
//! For analytics events (`track` / `feedback` / `identify`), construct
//! an `crate::otel::IntrospectionLogs` separately — see the `otel`
//! Cargo feature. For OpenTelemetry trace export, attach an
//! `crate::otel::IntrospectionSpanProcessor` to your own
//! `SdkTracerProvider`.

use std::env;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tracing::warn;

use crate::api::http::{HttpClient, HttpConfig};
use crate::resources::{
    ExperimentHandle, Experiments, Projects, Recipes, Repositories, RuntimeHandle, Runtimes,
};
use crate::types::{self, ClientConfig};

/// SDK version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Errors that can occur in the Introspection client. The REST build
/// only ever raises `NotInitialized` / `AlreadyShutdown` / `InvalidConfig`
/// directly; HTTP failures bubble up as
/// [`crate::IntrospectionAPIError`] from the underlying namespaces.
#[derive(Error, Debug)]
pub enum IntrospectionError {
    #[error("OpenTelemetry error: {0}")]
    OpenTelemetry(String),

    #[error("Client not initialized")]
    NotInitialized,

    #[error("Client already shut down")]
    AlreadyShutdown,
}

/// Result type for Introspection operations.
pub type Result<T> = std::result::Result<T, IntrospectionError>;

/// REST-only Introspection client.
///
/// Use [`Self::runtimes`] / [`Self::experiments`] / [`Self::runtime`] /
/// [`Self::experiment`] to drive the CP / DP API surface. For the
/// OpenTelemetry-based `track` / `feedback` / `identify` flow, enable
/// the `otel` Cargo feature.
pub struct IntrospectionClient {
    #[allow(dead_code)]
    service_name: String,
    project_id: Option<uuid::Uuid>,
    projects: Option<Projects>,
    repositories: Option<Repositories>,
    runtimes: Option<Runtimes>,
    experiments: Option<Experiments>,
    recipes: Option<Recipes>,
    #[allow(dead_code)]
    cp_http: Option<Arc<HttpClient>>,
}

impl IntrospectionClient {
    /// Create a new Introspection client. Reads `INTROSPECTION_TOKEN`
    /// and `INTROSPECTION_BASE_API_URL` from the environment when the
    /// matching fields on [`ClientConfig`] are not set.
    pub fn new(config: ClientConfig) -> Result<Self> {
        let token = config
            .token
            .clone()
            .or_else(|| env::var("INTROSPECTION_TOKEN").ok())
            .unwrap_or_default();

        let service_name = config
            .service_name
            .clone()
            .or_else(|| env::var("INTROSPECTION_SERVICE_NAME").ok())
            .unwrap_or_else(|| types::defaults::SERVICE_NAME.to_string());

        let project_id = config.project_id.or_else(|| {
            env::var("INTROSPECTION_PROJECT_ID")
                .ok()
                .and_then(|s| s.parse::<uuid::Uuid>().ok())
        });

        let advanced = config.advanced.unwrap_or_default();

        let base_api_url = advanced
            .base_api_url
            .clone()
            .or_else(|| env::var("INTROSPECTION_BASE_API_URL").ok())
            .unwrap_or_else(|| types::defaults::BASE_API_URL.to_string());

        if token.is_empty() {
            warn!("IntrospectionClient: No token provided. REST calls will fail.");
        }

        let (projects, repositories, runtimes, experiments, recipes, cp_http) = if token.is_empty()
        {
            (None, None, None, None, None, None)
        } else {
            let api_headers = advanced.additional_headers.clone().unwrap_or_default();
            let http_cfg = HttpConfig {
                api_url: base_api_url,
                token: token.clone(),
                additional_headers: api_headers,
                timeout: Duration::from_secs(types::defaults::API_TIMEOUT_SECS),
            };
            let http = HttpClient::new(http_cfg)
                .map_err(|e| IntrospectionError::OpenTelemetry(e.to_string()))?;
            let http_arc = Arc::new(http);
            (
                Some(Projects::new(http_arc.clone())),
                Some(Repositories::new(http_arc.clone())),
                Some(Runtimes::new(http_arc.clone())),
                Some(Experiments::new(http_arc.clone())),
                Some(Recipes::new(http_arc.clone())),
                Some(http_arc),
            )
        };

        Ok(Self {
            service_name,
            project_id,
            projects,
            repositories,
            runtimes,
            experiments,
            recipes,
            cp_http,
        })
    }

    /// The default project ID, resolved from [`ClientConfig::project_id`]
    /// or `INTROSPECTION_PROJECT_ID`.
    pub fn project_id(&self) -> Option<uuid::Uuid> {
        self.project_id
    }

    pub fn projects(&self) -> &Projects {
        self.projects.as_ref().expect(
            "client.projects() requires a token; set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`",
        )
    }

    pub fn repositories(&self) -> &Repositories {
        self.repositories.as_ref().expect(
            "client.repositories() requires a token; set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`",
        )
    }

    pub fn runtimes(&self) -> &Runtimes {
        self.runtimes.as_ref().expect(
            "client.runtimes() requires a token; set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`",
        )
    }

    pub fn experiments(&self) -> &Experiments {
        self.experiments.as_ref().expect(
            "client.experiments() requires a token; set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`",
        )
    }

    pub fn recipes(&self) -> &Recipes {
        self.recipes.as_ref().expect(
            "client.recipes() requires a token; set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`",
        )
    }

    pub fn runtime(&self, runtime_id: uuid::Uuid) -> RuntimeHandle {
        self.runtimes().handle(runtime_id)
    }

    /// Look up an active runtime by name. The server infers the project
    /// from the API token. Equivalent to `client.runtimes().by_name(name)`.
    pub async fn runtime_by_name(
        &self,
        name: &str,
    ) -> crate::api::error::ApiResult<RuntimeHandle> {
        self.runtimes().by_name(name).await
    }

    pub fn experiment(
        &self,
        experiment_id: uuid::Uuid,
        project_id: uuid::Uuid,
    ) -> ExperimentHandle {
        self.experiments().handle(experiment_id, project_id)
    }

    pub fn try_projects(&self) -> Option<&Projects> {
        self.projects.as_ref()
    }

    pub fn try_repositories(&self) -> Option<&Repositories> {
        self.repositories.as_ref()
    }

    pub fn try_runtimes(&self) -> Option<&Runtimes> {
        self.runtimes.as_ref()
    }

    pub fn try_experiments(&self) -> Option<&Experiments> {
        self.experiments.as_ref()
    }

    pub fn try_recipes(&self) -> Option<&Recipes> {
        self.recipes.as_ref()
    }

    /// Graceful shutdown. The REST build has nothing to flush, so this
    /// is a no-op — kept for API parity with the `otel` build.
    pub fn shutdown(self) -> Result<()> {
        Ok(())
    }
}
