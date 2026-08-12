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
use crate::resources::{
    Connectors, ExperimentHandle, Experiments, Recipes, RuntimeHandle, Runtimes,
};
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

    /// The REST client could not be built (a bad header value, a token that
    /// is not valid ASCII). Distinct from [`Self::OpenTelemetry`], which is
    /// where this used to be reported -- confusingly, since a REST-only
    /// build has no OpenTelemetry in it at all.
    #[error("Invalid client configuration: {0}")]
    InvalidConfig(String),
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
    runtimes: Runtimes,
    experiments: Experiments,
    recipes: Recipes,
    connectors: Connectors,
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
            .map_err(|e| IntrospectionError::InvalidConfig(e.to_string()))?;
        let cp_http = Arc::new(http);

        Ok(Self {
            runtimes: Runtimes::new(cp_http.clone()),
            experiments: Experiments::new(cp_http.clone()),
            recipes: Recipes::new(cp_http.clone()),
            connectors: Connectors::new(cp_http),
        })
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

    /// `/v1/connectors` CRUD, its nested `connections`, and `authorize()` —
    /// the consent URL a Business hands its customer.
    pub fn connectors(&self) -> &Connectors {
        &self.connectors
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

    /// Authenticate as a confidential service account and return a ready
    /// client.
    ///
    /// Mints a short-lived, project-scoped CP access token via the
    /// `client_credentials` grant (see
    /// [`crate::auth::service_account_token`]) and wires it in as the bearer
    /// token, so the runtime flow works exactly as it does with an API key.
    ///
    /// The token is not auto-refreshed: it lives for `expires_in` seconds, so
    /// re-mint (call this again) for long-lived processes once it lapses.
    /// Call [`crate::auth::service_account_token`] directly if you also need
    /// the resolved `dp_url` (e.g. to hand a browser the Data Plane
    /// endpoint).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use introspection_sdk::{auth::ServiceAccountTokenParams, IntrospectionClient};
    ///
    /// # async fn main_() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = IntrospectionClient::from_service_account(
    ///     ServiceAccountTokenParams::builder()
    ///         .client_id(std::env::var("INTRO_SA_CLIENT_ID")?)
    ///         .client_secret(std::env::var("INTRO_SA_CLIENT_SECRET")?)
    ///         .project(std::env::var("INTRO_PROJECT")?)
    ///         .build()?,
    ///     None,
    /// )
    /// .await?;
    /// let runtime = client.runtime("customer-agent").await?;
    /// # let _ = runtime;
    /// # Ok(()) }
    /// ```
    pub async fn from_service_account(
        params: crate::auth::ServiceAccountTokenParams,
        advanced: Option<types::AdvancedOptions>,
    ) -> crate::api::error::ApiResult<Self> {
        // Resolved before the params move into the mint call, so the client
        // talks to the same CP host the token was minted against. Leaving it
        // to `new()` would send a token minted against a staging CP to
        // whatever `INTROSPECTION_BASE_API_URL` happened to say.
        let base_api_url = crate::auth::resolve_base_api_url(params.base_api_url.as_deref());
        let token = crate::auth::service_account_token(params).await?;
        let advanced = types::AdvancedOptions {
            base_api_url: advanced
                .as_ref()
                .and_then(|a| a.base_api_url.clone())
                .or(Some(base_api_url)),
            additional_headers: advanced.and_then(|a| a.additional_headers),
        };
        Self::new(ClientConfig::with_token(token.access_token).advanced(advanced))
            .map_err(|e| crate::api::error::IntrospectionAPIError::InvalidConfig(e.to_string()))
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
