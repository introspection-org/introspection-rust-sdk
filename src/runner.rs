//! `Runner` — "one user session" wrapping a [`RunnerSpec`] from CP.
//!
//! A Runner is an agent-session with a runtime context attached. The CP
//! `/run` route mints a single RS256 `session_token` (a session-locator
//! JWT — the customer's only credential). The SDK sends it as
//! `Authorization: Bearer …` on every DP call; the DP server looks up
//! the session by JWT claims and reads the materialized access token
//! from its Redis cache.
//!
//! v1: no auto-refresh in the SDK. DP's session-materializer handles
//! it transparently. [`Runner::refresh`] is a manual escape hatch.
//! [`Runner::close`] flips a local closed flag; server-side revoke via
//! the locator path is a follow-up.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::files::Files;
use crate::api::http::{HttpClient, HttpConfig};
use crate::api::schemas::{RunRequest, RunnerContext, RunnerDeployment, RunnerSpec, StringOrUuid};
use crate::api::tasks::Tasks;
use crate::types::defaults;

/// How a [`Runner`] was opened. Captured so [`Runner::refresh`] can
/// re-call the CP `/run` route with the same input.
#[derive(Clone)]
pub enum RunnerSource {
    Runtime {
        cp_http: Arc<HttpClient>,
        runtime_id: Uuid,
        ctx: RunRequest,
    },
    Experiment {
        cp_http: Arc<HttpClient>,
        experiment_id: Uuid,
        project: StringOrUuid,
        ctx: RunRequest,
    },
}

impl RunnerSource {
    async fn mint(&self) -> ApiResult<RunnerSpec> {
        match self {
            Self::Runtime {
                cp_http,
                runtime_id,
                ctx,
            } => {
                let path = format!("/v1/runtimes/{}/run", runtime_id);
                cp_http.post_json(&path, ctx).await
            }
            Self::Experiment {
                cp_http,
                experiment_id,
                project,
                ctx,
            } => {
                let path = format!("/v1/experiments/{}/run?project={}", experiment_id, project);
                cp_http.post_json(&path, ctx).await
            }
        }
    }
}

/// Resolved live state derived from a [`RunnerSpec`]. Replaced
/// wholesale when [`Runner::refresh`] is called.
struct RunnerState {
    dp_http: Arc<HttpClient>,
    deployment: RunnerDeployment,
    context: RunnerContext,
    session_id: String,
    expires_at: String,
    closed: bool,
}

impl RunnerState {
    fn from_spec(spec: RunnerSpec) -> ApiResult<Self> {
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
            closed: false,
        })
    }
}

/// One user session against a single DP, scoped to a (runtime,
/// identity) or (experiment-arm, identity) pair.
pub struct Runner {
    state: Arc<RwLock<RunnerState>>,
    source: RunnerSource,
}

impl Runner {
    pub(crate) fn from_spec(spec: RunnerSpec, source: RunnerSource) -> ApiResult<Self> {
        let state = RunnerState::from_spec(spec)?;
        Ok(Self {
            state: Arc::new(RwLock::new(state)),
            source,
        })
    }

    fn dp_http(&self) -> ApiResult<Arc<HttpClient>> {
        let state = self
            .state
            .read()
            .map_err(|_| IntrospectionAPIError::InvalidConfig("runner lock poisoned".into()))?;
        if state.closed {
            return Err(IntrospectionAPIError::InvalidConfig(
                "runner has been closed".to_string(),
            ));
        }
        Ok(state.dp_http.clone())
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

    /// Resolved runtime context (runtime / arm / recipe pin / identity
    /// / caller).
    pub fn context(&self) -> RunnerContext {
        self.state.read().expect("runner lock").context.clone()
    }

    /// DP base URL the runner is currently talking to.
    ///
    /// Convenience accessor — equivalent to
    /// `runner.deployment().endpoint`. Use [`Self::deployment`] when
    /// you also need the slug / region.
    pub fn dp_endpoint(&self) -> String {
        self.state
            .read()
            .expect("runner lock")
            .deployment
            .endpoint
            .clone()
    }

    /// Resolved DP deployment (endpoint / slug / region).
    pub fn deployment(&self) -> RunnerDeployment {
        self.state.read().expect("runner lock").deployment.clone()
    }

    /// Session ID assigned by CP.
    pub fn session_id(&self) -> String {
        self.state.read().expect("runner lock").session_id.clone()
    }

    /// Session expiry (ISO-8601 string).
    pub fn expires_at(&self) -> String {
        self.state.read().expect("runner lock").expires_at.clone()
    }

    /// Manual escape hatch — re-call the CP `/run` route with the
    /// original [`RunRequest`] and swap in the new spec.
    ///
    /// v1: not auto-scheduled. DP's session-materializer keeps the
    /// access token fresh transparently. Call this only if you
    /// explicitly want a brand-new session (e.g. after a hard error).
    pub async fn refresh(&self) -> ApiResult<()> {
        let spec = self.source.mint().await?;
        let new_state = RunnerState::from_spec(spec)?;
        let mut state = self
            .state
            .write()
            .map_err(|_| IntrospectionAPIError::InvalidConfig("runner lock poisoned".into()))?;
        if state.closed {
            return Err(IntrospectionAPIError::InvalidConfig(
                "runner has been closed".to_string(),
            ));
        }
        *state = new_state;
        Ok(())
    }

    /// Mark the runner closed locally. Future `tasks()` / `files()`
    /// accessors panic; advanced callers can check [`Self::is_closed`]
    /// first.
    ///
    /// v1: no server-side revoke (RS256 isn't natively revocable; the
    /// locator-based session-row revoke path is a follow-up).
    pub fn close(&self) {
        if let Ok(mut state) = self.state.write() {
            state.closed = true;
        }
    }

    pub fn is_closed(&self) -> bool {
        self.state.read().map(|s| s.closed).unwrap_or(true)
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
