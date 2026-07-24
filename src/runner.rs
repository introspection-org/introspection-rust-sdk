//! `Runner` — "one user session" wrapping a [`RunnerSpec`] from CP.
//!
//! A Runner is an agent-session with a runtime context attached. The CP
//! `/run` route mints a single RS256 `session_token` (a self-contained
//! Runner capability — the customer's only credential). The SDK sends it as
//! `Authorization: Bearer …` on every DP call, and the target DP validates it
//! directly.
//!
//! A [`Runner`] always represents one session; call the Runtime or Experiment
//! `run` operation again to create another. [`Runner::close`] flips a local
//! closed flag; it does not revoke the capability server-side.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::files::Files;
use crate::api::http::{HttpClient, HttpConfig};
use crate::api::schemas::{RunnerContext, RunnerDeployment, RunnerSpec};
use crate::api::shares::Shares;
use crate::api::tasks::Tasks;
use crate::api::telemetry::{Conversations, Events, Metrics};
use crate::types::defaults;

/// One user session against a single DP, scoped to a (runtime,
/// identity) or (experiment-arm, identity) pair.
pub struct Runner {
    dp_http: Arc<HttpClient>,
    deployment: RunnerDeployment,
    context: RunnerContext,
    session_id: String,
    expires_at: String,
    closed: AtomicBool,
}

impl Runner {
    pub(crate) fn from_spec(spec: RunnerSpec) -> ApiResult<Self> {
        let dp_http = Arc::new(build_dp_http(
            &spec.deployment.endpoint,
            &spec.session_token,
        )?);
        Ok(Self {
            dp_http,
            deployment: spec.deployment,
            context: spec.runtime_context,
            session_id: spec.session_id,
            expires_at: spec.expires_at,
            closed: AtomicBool::new(false),
        })
    }

    fn dp_http(&self) -> ApiResult<Arc<HttpClient>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(IntrospectionAPIError::InvalidConfig(
                "runner has been closed".to_string(),
            ));
        }
        Ok(self.dp_http.clone())
    }

    /// `runner.tasks.*` — runner-bound task operations. Cheap clone.
    pub fn tasks(&self) -> Tasks {
        let http = self.dp_http().unwrap_or_else(|e| panic!("{e}"));
        Tasks::new(http)
    }

    /// `runner.files.*` — runner-bound file operations. Cheap clone.
    pub fn files(&self) -> Files {
        let http = self.dp_http().unwrap_or_else(|e| panic!("{e}"));
        Files::new(http)
    }

    /// Runner-bound read-sharing grants for files and conversations.
    pub fn shares(&self) -> Shares {
        let http = self.dp_http().unwrap_or_else(|e| panic!("{e}"));
        Shares::new(http)
    }

    /// `runner.conversations.*` — Data-Plane telemetry reads over
    /// `GET /v1/conversations` (append-only `otel_traces`). Runner-scoped (DP
    /// bearer + `events:read`). Cheap clone.
    pub fn conversations(&self) -> Conversations {
        let http = self.dp_http().unwrap_or_else(|e| panic!("{e}"));
        Conversations::new(http)
    }

    /// `runner.events.*` — Data-Plane telemetry reads over `GET /v1/events`
    /// (append-only `otel_logs`; typed six-family read, `event_name`
    /// required). Runner-scoped (DP bearer + `events:read`). Cheap clone.
    pub fn events(&self) -> Events {
        let http = self.dp_http().unwrap_or_else(|e| panic!("{e}"));
        Events::new(http)
    }

    /// `runner.metrics.*` — the bounded `POST /v1/metrics` analytics surface.
    /// Runner-scoped (DP bearer + `events:read`). Cheap clone.
    pub fn metrics(&self) -> Metrics {
        let http = self.dp_http().unwrap_or_else(|e| panic!("{e}"));
        Metrics::new(http)
    }

    /// Resolved runtime context (runtime / arm / recipe pin / identity
    /// / caller).
    pub fn context(&self) -> RunnerContext {
        self.context.clone()
    }

    /// DP base URL the runner is currently talking to.
    ///
    /// Convenience accessor — equivalent to
    /// `runner.deployment().endpoint`. Use [`Self::deployment`] when
    /// you also need the slug / region.
    pub fn dp_endpoint(&self) -> String {
        self.deployment.endpoint.clone()
    }

    /// Resolved DP deployment (endpoint / slug / region).
    pub fn deployment(&self) -> RunnerDeployment {
        self.deployment.clone()
    }

    /// Session ID assigned by CP.
    pub fn session_id(&self) -> String {
        self.session_id.clone()
    }

    /// Session expiry (ISO-8601 string).
    pub fn expires_at(&self) -> String {
        self.expires_at.clone()
    }

    /// Mark the runner closed locally. Future `tasks()` / `files()`
    /// accessors panic; advanced callers can check [`Self::is_closed`]
    /// first.
    ///
    /// v1: no server-side revoke (RS256 isn't natively revocable; the
    /// locator-based session-row revoke path is a follow-up).
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

fn build_dp_http(dp_endpoint: &str, bearer: &str) -> ApiResult<HttpClient> {
    let cfg = HttpConfig {
        api_url: dp_endpoint.to_string(),
        token: bearer.to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(defaults::API_TIMEOUT_SECS),
        max_retries: defaults::API_MAX_RETRIES,
        retry_base: Duration::from_millis(defaults::API_RETRY_BASE_MS),
    };
    HttpClient::new(cfg)
}
