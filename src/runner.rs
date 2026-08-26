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
use crate::api::shares::Shares;
use crate::api::tasks::Tasks;
use crate::api::telemetry::{Conversations, Events, Metrics};
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
                let path = format!(
                    "/v1/experiments/{}/run?project={}",
                    experiment_id,
                    crate::resources::experiments::encode_project(project)
                );
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

    /// The DP client, whatever state the runner is in.
    ///
    /// Infallible on purpose. This used to return a `Result` that six public
    /// accessors then `panic!`d on, so `runner.tasks()` after
    /// `runner.close()` -- or any poisoned lock -- aborted the caller's
    /// process. Closure is enforced on the *request* path instead
    /// (`HttpClient::close`), which also covers a namespace handle taken
    /// before the close.
    fn dp_http(&self) -> Arc<HttpClient> {
        self.read_state().dp_http.clone()
    }

    /// Read the state, recovering from a poisoned lock.
    ///
    /// A panic in another thread must not turn every later accessor on this
    /// runner into a panic of its own; the data behind the lock is plain
    /// cloned values with no invariant to violate.
    fn read_state(&self) -> std::sync::RwLockReadGuard<'_, RunnerState> {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `runner.tasks.*` — runner-bound task operations. Cheap clone.
    pub fn tasks(&self) -> Tasks {
        Tasks::new(self.dp_http())
    }

    /// `runner.files.*` — runner-bound file operations. Cheap clone.
    pub fn files(&self) -> Files {
        Files::new(self.dp_http())
    }

    /// Runner-bound read-sharing grants for files and conversations.
    pub fn shares(&self) -> Shares {
        Shares::new(self.dp_http())
    }

    /// `runner.conversations.*` — Data-Plane telemetry reads over
    /// `GET /v1/conversations` (append-only `otel_traces`). Runner-scoped (DP
    /// bearer + `events:read`). Cheap clone.
    pub fn conversations(&self) -> Conversations {
        Conversations::new(self.dp_http())
    }

    /// `runner.events.*` — Data-Plane telemetry reads over `GET /v1/events`
    /// (append-only `otel_logs`; typed seven-family read, `event_name`
    /// required). Runner-scoped (DP bearer + `events:read`). Cheap clone.
    pub fn events(&self) -> Events {
        Events::new(self.dp_http())
    }

    /// `runner.metrics.*` — the bounded `POST /v1/metrics` analytics surface.
    /// Runner-scoped (DP bearer + `events:read`). Cheap clone.
    pub fn metrics(&self) -> Metrics {
        Metrics::new(self.dp_http())
    }

    /// Resolved runtime context (runtime / arm / recipe pin / identity
    /// / caller).
    pub fn context(&self) -> RunnerContext {
        self.read_state().context.clone()
    }

    /// DP base URL the runner is currently talking to.
    ///
    /// Convenience accessor — equivalent to
    /// `runner.deployment().endpoint`. Use [`Self::deployment`] when
    /// you also need the slug / region.
    pub fn dp_endpoint(&self) -> String {
        self.read_state().deployment.endpoint.clone()
    }

    /// Resolved DP deployment (endpoint / slug / region).
    pub fn deployment(&self) -> RunnerDeployment {
        self.read_state().deployment.clone()
    }

    /// Session ID assigned by CP.
    pub fn session_id(&self) -> String {
        self.read_state().session_id.clone()
    }

    /// Session expiry (ISO-8601 string).
    pub fn expires_at(&self) -> String {
        self.read_state().expires_at.clone()
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

    /// Mark the runner closed locally. The accessors keep working; the
    /// requests they make return
    /// [`IntrospectionAPIError::InvalidConfig`] instead. Callers who want
    /// to branch before that can check [`Self::is_closed`].
    ///
    /// v1: no server-side revoke (RS256 isn't natively revocable; the
    /// locator-based session-row revoke path is a follow-up).
    pub fn close(&self) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closed = true;
        // Closes every namespace handle taken before now, too: they hold a
        // clone of this client, and the flag is shared across clones.
        state.dp_http.close();
    }

    pub fn is_closed(&self) -> bool {
        self.read_state().closed
    }
}

fn build_dp_http(dp_endpoint: &str, bearer: &str) -> ApiResult<HttpClient> {
    let cfg = HttpConfig {
        api_url: dp_endpoint.to_string(),
        token: bearer.to_string(),
        // INTROSPECTION_DEV_TARGET has to ride the DP calls too: `POST
        // /v1/tasks` is the exact path dev_target exists for, and it is
        // served by this client, not the CP one.
        additional_headers: crate::dev_target::with_dev_target(HashMap::new()),
        timeout: Duration::from_secs(defaults::API_TIMEOUT_SECS),
        max_retries: defaults::API_MAX_RETRIES,
        retry_base: Duration::from_millis(defaults::API_RETRY_BASE_MS),
    };
    HttpClient::new(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_client() -> HttpClient {
        let client = HttpClient::new(HttpConfig {
            api_url: "https://dp.example.com".to_string(),
            token: "t".to_string(),
            additional_headers: HashMap::new(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            retry_base: Duration::from_millis(1),
        })
        .unwrap();
        client.close();
        client
    }

    #[tokio::test]
    async fn a_closed_client_refuses_requests_instead_of_panicking() {
        // `runner.tasks()` used to `panic!` on the Result this replaces, so
        // using a runner after `close()` aborted the caller's process.
        let tasks = Tasks::new(Arc::new(closed_client()));
        let err = match tasks.get("task-1").await {
            Ok(_) => panic!("expected the closed client to refuse"),
            Err(e) => e,
        };
        let IntrospectionAPIError::InvalidConfig(message) = err else {
            panic!("expected InvalidConfig");
        };
        assert!(message.contains("closed"), "got {message}");
    }
}
