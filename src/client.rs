//! Introspection Client — REST-only surface.
//!
//! Always available with no OpenTelemetry dependency. Exposes
//! `client.runtime(ref).run()` and `client.experiment(id).run()` accessors over
//! the Introspection execution API.
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
use crate::resources::{ExperimentHandle, RuntimeHandle};
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
/// Use [`Self::runtime`] / [`Self::experiment`] to open runners. For the
/// OpenTelemetry-based `track` / `feedback` / `identify` flow, enable
/// the `otel` Cargo feature.
pub struct IntrospectionClient {
    #[allow(dead_code)]
    service_name: String,
    project_id: Option<uuid::Uuid>,
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

        let project_id = config.project_id;

        let advanced = config.advanced.unwrap_or_default();

        let base_api_url = advanced
            .base_api_url
            .clone()
            .or_else(|| env::var("INTROSPECTION_BASE_API_URL").ok())
            .unwrap_or_else(|| types::defaults::BASE_API_URL.to_string());

        if token.is_empty() {
            warn!("IntrospectionClient: No token provided. REST calls will fail.");
        }

        let cp_http = if token.is_empty() {
            None
        } else {
            let api_headers = advanced.additional_headers.clone().unwrap_or_default();
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
            let http_arc = Arc::new(http);
            Some(http_arc)
        };

        Ok(Self {
            service_name,
            project_id,
            cp_http,
        })
    }

    /// The resolved project ID from [`ClientConfig::project_id`], if supplied.
    pub fn project_id(&self) -> Option<uuid::Uuid> {
        self.project_id
    }

    /// Create a handle that resolves a runtime group slug or ID when `.run()` is called.
    pub fn runtime(&self, runtime: impl Into<String>) -> RuntimeHandle {
        RuntimeHandle::new(self.cp_http(), runtime)
    }

    pub fn experiment(&self, experiment_id: uuid::Uuid) -> ExperimentHandle {
        ExperimentHandle::new(self.cp_http(), experiment_id)
    }

    fn cp_http(&self) -> Arc<HttpClient> {
        self.cp_http.as_ref().cloned().expect(
            "runner creation requires a token; set `INTROSPECTION_TOKEN` or `ClientConfig::with_token`",
        )
    }

    /// Graceful shutdown. The REST build has nothing to flush, so this
    /// is a no-op — kept for API parity with the `otel` build.
    pub fn shutdown(self) -> Result<()> {
        Ok(())
    }
}
