//! Introspection Client — REST-only surface.
//!
//! Always available with no OpenTelemetry dependency. Exposes
//! `client.runtimes()` / `client.experiments()` / `client.runtimes().handle(id)` /
//! `client.experiment(id, project)` accessors over the Introspection
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

use crate::api::http::{HttpClient, HttpConfig};
use crate::dev_target;
use crate::resources::{ExperimentHandle, Experiments, Recipes, RuntimeHandle, Runtimes};
use crate::types::{self, ClientConfig};

/// SDK version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Errors that can occur in the Introspection client. HTTP failures bubble up
/// as [`crate::IntrospectionAPIError`] from the underlying namespaces.
#[derive(Error, Debug)]
pub enum IntrospectionError {
    #[error("OpenTelemetry error: {0}")]
    OpenTelemetry(String),

    #[error("A token is required: set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`")]
    TokenRequired,

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
    runtimes: Runtimes,
    experiments: Experiments,
    recipes: Recipes,
    #[allow(dead_code)]
    cp_http: Arc<HttpClient>,
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

        let project_id = config.project_id;

        let advanced = config.advanced.unwrap_or_default();

        let base_api_url = advanced
            .base_api_url
            .clone()
            .or_else(|| env::var("INTROSPECTION_BASE_API_URL").ok())
            .unwrap_or_else(|| types::defaults::BASE_API_URL.to_string());

        // Fail here rather than storing `None` and panicking on first use:
        // every method on this client is a REST call, so a tokenless client
        // has no usable surface at all.
        if token.is_empty() {
            return Err(IntrospectionError::TokenRequired);
        }

        // INTROSPECTION_DEV_TARGET rides every request as a header so it
        // reaches the paths a runner cannot: a bare `POST /v1/tasks` with a
        // dev API key mints its JWT from the key row and has no per-request
        // claim to carry a target.
        let api_headers =
            dev_target::with_dev_target(advanced.additional_headers.clone().unwrap_or_default());
        let http_cfg = HttpConfig {
            api_url: base_api_url,
            token: token.clone(),
            additional_headers: api_headers,
            timeout: Duration::from_secs(types::defaults::API_TIMEOUT_SECS),
            max_retries: types::defaults::API_MAX_RETRIES,
            retry_base: Duration::from_millis(types::defaults::API_RETRY_BASE_MS),
        };
        let http = HttpClient::new(http_cfg)
            .map_err(|e| IntrospectionError::OpenTelemetry(e.to_string()))?;
        let cp_http = Arc::new(http);

        Ok(Self {
            service_name,
            project_id,
            runtimes: Runtimes::new(cp_http.clone()),
            experiments: Experiments::new(cp_http.clone()),
            recipes: Recipes::new(cp_http.clone()),
            cp_http,
        })
    }

    /// The resolved project ID from [`ClientConfig::project_id`], if supplied.
    pub fn project_id(&self) -> Option<uuid::Uuid> {
        self.project_id
    }

    pub fn runtimes(&self) -> &Runtimes {
        &self.runtimes
    }

    pub fn experiments(&self) -> &Experiments {
        &self.experiments
    }

    pub fn recipes(&self) -> &Recipes {
        &self.recipes
    }

    /// Look up an active runtime by runtime group slug or ID. The server infers the
    /// project from the API token. Equivalent to
    /// `client.runtimes().resolve(runtime)`.
    ///
    /// To build a handle for a concrete runtime UUID without a lookup, use
    /// `client.runtimes().handle(runtime_id)`.
    pub async fn runtime(&self, runtime: &str) -> crate::api::error::ApiResult<RuntimeHandle> {
        self.runtimes().resolve(runtime).await
    }

    pub fn experiment(
        &self,
        experiment_id: uuid::Uuid,
        project: impl Into<crate::api::schemas::StringOrUuid>,
    ) -> ExperimentHandle {
        self.experiments().handle(experiment_id, project)
    }

    /// Graceful shutdown. The REST build has nothing to flush, so this
    /// is a no-op — kept for API parity with the `otel` build.
    pub fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises the tests that touch `INTROSPECTION_TOKEN`.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_new_requires_a_token() {
        let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved = env::var("INTROSPECTION_TOKEN").ok();
        unsafe { env::remove_var("INTROSPECTION_TOKEN") };

        // Every method on this client is a REST call, so a tokenless client
        // has no usable surface. It used to construct successfully and then
        // panic on the first accessor.
        match IntrospectionClient::new(ClientConfig::default()) {
            Err(IntrospectionError::TokenRequired) => {}
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("a tokenless client must not construct"),
        }

        if let Some(token) = saved {
            unsafe { env::set_var("INTROSPECTION_TOKEN", token) };
        }
    }

    #[test]
    fn test_new_accepts_an_explicit_token() {
        let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(client) = IntrospectionClient::new(ClientConfig::with_token("intro_test")) else {
            panic!("an explicit token is enough");
        };
        // The accessors are infallible now that construction enforces the token.
        let _ = client.runtimes();
        let _ = client.experiments();
        let _ = client.recipes();
    }
}
