//! Native email-code sign-in for a `native` Application; see [`EmailCodeAuth`].

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::{to_api_error, HttpClient, HttpConfig};
use crate::auth::{
    post_token_form_with, resolve_base_api_url, token_http_client, urlencode_form, OAuthToken,
};
use crate::client::IntrospectionClient;
use crate::types::{defaults, AdvancedOptions, ClientConfig};

/// The `grant_type` the token endpoint takes an emailed code under.
pub const EMAIL_CODE_GRANT: &str = "urn:introspection:params:oauth:grant-type:email_code";

const EMAIL_CODE_PATH: &str = "/v1/oauth/email/code";
const REVOKE_PATH: &str = "/v1/oauth/revoke";
const GRANT_REFRESH_TOKEN: &str = "refresh_token";
const DEFAULT_LEEWAY: Duration = Duration::from_secs(60);

/// A signed-in member's session: the access token plus what renews it.
///
/// Serializable so an app can keep it between launches (in the platform
/// keychain, say) and hand it back with [`EmailCodeAuth::set_session`].
/// Persist it again on every [`AuthChangeEvent::TokenRefreshed`]: each
/// refresh rotates the refresh token and the old one stops working.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    /// Project-scoped Data Plane access token (`Authorization: Bearer …`).
    pub access_token: String,
    /// Rotated by every refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When the access token expires, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// The Control Plane session the refresh grant is keyed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The `customer` member the session belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    /// The Data Plane URL the Control Plane resolved for the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dp_url: Option<String>,
    /// The granted scope: the Application's `allowed_scopes` ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthSession")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .field("session_id", &self.session_id)
            .field("org_id", &self.org_id)
            .field("project_id", &self.project_id)
            .field("member_id", &self.member_id)
            .field("dp_url", &self.dp_url)
            .field("scope", &self.scope)
            .finish()
    }
}

impl AuthSession {
    /// Build a session from a token-endpoint response received at
    /// `received_at`. A field the response omits is kept from `previous`: the
    /// refresh grant answers without `dp_url`, for one.
    pub fn from_token(
        token: &OAuthToken,
        received_at: SystemTime,
        previous: Option<&AuthSession>,
    ) -> Self {
        let extra = |key: &str| {
            token
                .extra
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        let received = received_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            access_token: token.access_token.clone(),
            refresh_token: extra("refresh_token")
                .or_else(|| previous.and_then(|p| p.refresh_token.clone())),
            expires_at: u64::try_from(token.expires_in)
                .ok()
                .map(|expires_in| received.saturating_add(expires_in)),
            session_id: extra("session_id").or_else(|| previous.and_then(|p| p.session_id.clone())),
            org_id: extra("org_id").or_else(|| previous.and_then(|p| p.org_id.clone())),
            project_id: extra("project_id").or_else(|| previous.and_then(|p| p.project_id.clone())),
            member_id: extra("member_id").or_else(|| previous.and_then(|p| p.member_id.clone())),
            dp_url: token
                .dp_url
                .clone()
                .or_else(|| previous.and_then(|p| p.dp_url.clone())),
            scope: token
                .scope
                .clone()
                .or_else(|| previous.and_then(|p| p.scope.clone())),
        }
    }

    /// Whether the access token expires within `leeway` of now. A session
    /// with no known expiry never does.
    pub fn expires_within(&self, leeway: Duration) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        now + leeway >= Duration::from_secs(expires_at)
    }
}

/// What changed, mirroring Supabase's `AuthChangeEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthChangeEvent {
    /// The state a new [`EmailCodeAuth`] starts in: no session yet.
    InitialSession,
    SignedIn,
    SignedOut,
    TokenRefreshed,
}

/// The latest session change, as [`EmailCodeAuth::changes`] reports it.
#[derive(Debug, Clone)]
pub struct AuthState {
    pub event: AuthChangeEvent,
    pub session: Option<AuthSession>,
}

/// Configuration for [`EmailCodeAuth`].
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into), default)]
pub struct EmailCodeAuthConfig {
    /// The `native` Application's `client_id` (public: there is no secret).
    pub client_id: String,
    /// The project the session is scoped to (slug or id).
    pub project: String,
    /// CP API base URL. Defaults to `INTROSPECTION_BASE_API_URL` or
    /// `https://api.introspection.dev`.
    #[builder(setter(strip_option))]
    pub base_api_url: Option<String>,
    /// Data Plane URL. Defaults to the `dp_url` the token response carries.
    #[builder(setter(strip_option))]
    pub dp_url: Option<String>,
    /// Refresh this long before the access token expires. Defaults to 60 s.
    #[builder(setter(strip_option))]
    pub leeway: Option<Duration>,
}

impl EmailCodeAuthConfig {
    /// Create a builder.
    pub fn builder() -> EmailCodeAuthConfigBuilder {
        EmailCodeAuthConfigBuilder::default()
    }
}

#[derive(Serialize)]
struct EmailCodeRequest<'a> {
    client_id: &'a str,
    email: &'a str,
    project: &'a str,
}

struct State {
    session: Option<AuthSession>,
    /// Bumped by every sign-in and sign-out. A response that started under
    /// an older generation has been superseded.
    generation: u64,
}

struct Inner {
    client_id: String,
    project: String,
    base_api_url: String,
    dp_url: Option<String>,
    leeway: Duration,
    http: reqwest::Client,
    state: Mutex<State>,
    refresh_gate: tokio::sync::Mutex<()>,
    changes: watch::Sender<AuthState>,
}

/// Email-code sign-in, session refresh and sign-out for a `native`
/// Application. Cheap to clone; clones share one session.
///
/// The platform's own sign-in for a mobile or desktop app that draws its own
/// screens: send a one-time code to an email, then exchange the code at the
/// Control Plane token endpoint for an access token and a rotating refresh
/// token. The token belongs to a `customer` member of the Application's
/// organization, and its scopes are the Application's `allowed_scopes`
/// ceiling.
///
/// [`EmailCodeAuth`] holds the session: it refreshes the access token before
/// it expires and after the Data Plane answers `401`, shares one refresh
/// between concurrent callers, and drops a sign-in or refresh that answers
/// after a sign-out or a newer sign-in instead of letting it overwrite the
/// current session.
///
/// A native token is a **Data Plane** credential. Control Plane routes
/// (runtimes, connectors, members) answer it with `401`, so reach the Data
/// Plane through [`EmailCodeAuth::with_data_plane`] or
/// [`EmailCodeAuth::client`]'s Data Plane namespaces.
///
/// ```rust,no_run
/// use introspection_sdk::auth::{EmailCodeAuth, EmailCodeAuthConfig};
/// use introspection_sdk::{DataPlaneResources, TaskListParams};
///
/// # async fn main_() -> Result<(), Box<dyn std::error::Error>> {
/// let auth = EmailCodeAuth::new(
///     EmailCodeAuthConfig::builder()
///         .client_id("intro_app_…")
///         .project("acme")
///         .build()?,
/// )?;
/// auth.send_code("user@example.com").await?;
/// // A returning user's code is six digits; a new user's first code is six
/// // characters of A-Z and 0-9, so take it as typed.
/// let session = auth.verify_code("user@example.com", "K7Q2ZD").await?;
/// println!("signed in as member {:?}", session.member_id);
///
/// let page = auth
///     .with_data_plane(|dp| async move {
///         dp.tasks().list(&TaskListParams::default()).next_page().await
///     })
///     .await?;
/// # let _ = page; Ok(()) }
/// ```
#[derive(Clone)]
pub struct EmailCodeAuth {
    inner: Arc<Inner>,
}

impl fmt::Debug for EmailCodeAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailCodeAuth")
            .field("client_id", &self.inner.client_id)
            .field("project", &self.inner.project)
            .field("base_api_url", &self.inner.base_api_url)
            .finish_non_exhaustive()
    }
}

impl EmailCodeAuth {
    /// Create a signed-out client. Restore a persisted session with
    /// [`Self::set_session`].
    pub fn new(config: EmailCodeAuthConfig) -> ApiResult<Self> {
        if config.client_id.is_empty() || config.project.is_empty() {
            return Err(IntrospectionAPIError::InvalidConfig(
                "email-code sign-in needs the Application's client_id and a project".to_string(),
            ));
        }
        let (changes, _) = watch::channel(AuthState {
            event: AuthChangeEvent::InitialSession,
            session: None,
        });
        Ok(Self {
            inner: Arc::new(Inner {
                base_api_url: resolve_base_api_url(config.base_api_url.as_deref()),
                client_id: config.client_id,
                project: config.project,
                dp_url: config.dp_url,
                leeway: config.leeway.unwrap_or(DEFAULT_LEEWAY),
                http: token_http_client()?,
                state: Mutex::new(State {
                    session: None,
                    generation: 0,
                }),
                refresh_gate: tokio::sync::Mutex::new(()),
                changes,
            }),
        })
    }

    /// Session changes. The receiver starts at the current state and sees
    /// the latest change; persist `session` on every change to keep the
    /// rotated refresh token.
    pub fn changes(&self) -> watch::Receiver<AuthState> {
        self.inner.changes.subscribe()
    }

    /// `POST /v1/oauth/email/code`: email a sign-in code.
    ///
    /// The answer is the same whether or not the email has an account. A
    /// `429` (`slow_down`) carries the server's wait as
    /// [`IntrospectionAPIError::retry_after`].
    pub async fn send_code(&self, email: &str) -> ApiResult<()> {
        let res = self
            .inner
            .http
            .post(format!("{}{EMAIL_CODE_PATH}", self.inner.base_api_url))
            .json(&EmailCodeRequest {
                client_id: &self.inner.client_id,
                email,
                project: &self.inner.project,
            })
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            return Err(to_api_error(res, status).await);
        }
        Ok(())
    }

    /// Exchange the emailed `code` for a session and sign in.
    ///
    /// The code is opaque: a returning user's is six digits, and a new
    /// user's first code is six characters of `A-Z0-9`. Pass it as typed;
    /// only surrounding whitespace is trimmed. Every rejected code is the
    /// same `invalid_grant`.
    ///
    /// Fails with [`IntrospectionAPIError::Superseded`] when a sign-out or
    /// another sign-in completed while this code was being verified; the
    /// newer state is kept.
    pub async fn verify_code(&self, email: &str, code: &str) -> ApiResult<AuthSession> {
        let generation = self.lock().generation;
        let form = [
            ("grant_type", EMAIL_CODE_GRANT),
            ("client_id", self.inner.client_id.as_str()),
            ("email", email),
            ("code", code.trim()),
            ("project", self.inner.project.as_str()),
        ];
        let token = post_token_form_with(&self.inner.http, &self.inner.base_api_url, &form).await?;
        let session = AuthSession::from_token(&token, SystemTime::now(), None);
        let mut state = self.lock();
        if state.generation != generation {
            return Err(IntrospectionAPIError::Superseded(
                "a sign-out or another sign-in completed while the code was being verified"
                    .to_string(),
            ));
        }
        self.signed_in(&mut state, session.clone());
        Ok(session)
    }

    /// Adopt a session persisted earlier (or minted elsewhere) and sign in
    /// with it. Supersedes any sign-in or refresh in flight.
    pub fn set_session(&self, session: AuthSession) {
        let mut state = self.lock();
        self.signed_in(&mut state, session);
    }

    /// The session as it stands, without refreshing it.
    pub fn current_session(&self) -> Option<AuthSession> {
        self.lock().session.clone()
    }

    /// The session, refreshed first when it is within the leeway of expiry.
    /// `None` when signed out.
    pub async fn session(&self) -> ApiResult<Option<AuthSession>> {
        match self.current_session() {
            Some(session) if session.expires_within(self.inner.leeway) => self
                .refresh_from(Some(session.access_token))
                .await
                .map(Some),
            session => Ok(session),
        }
    }

    /// A current access token, refreshed first when it is about to expire.
    pub async fn access_token(&self) -> ApiResult<String> {
        Ok(self
            .session()
            .await?
            .ok_or_else(not_signed_in)?
            .access_token)
    }

    /// Renew the session now, or join the renewal already in flight.
    ///
    /// A refresh the server rejects (`invalid_grant`, `401`, `403`, `422`)
    /// signs the user out and returns that error; a network failure or a
    /// `5xx` leaves the session in place.
    pub async fn refresh(&self) -> ApiResult<AuthSession> {
        let observed = self.current_session().map(|s| s.access_token);
        self.refresh_from(observed).await
    }

    /// Renew the session after the server answered `401` to
    /// `rejected_access_token`. When another caller already replaced that
    /// token, the current session is returned without a second refresh.
    pub async fn refresh_after_unauthorized(
        &self,
        rejected_access_token: &str,
    ) -> ApiResult<AuthSession> {
        self.refresh_from(Some(rejected_access_token.to_string()))
            .await
    }

    /// Sign out: forget the session locally, then revoke it on the Control
    /// Plane (`POST /v1/oauth/revoke`). The local session is cleared even
    /// when revocation fails, and the revocation error is returned.
    pub async fn sign_out(&self) -> ApiResult<()> {
        let session = {
            let mut state = self.lock();
            state.generation += 1;
            let session = state.session.take();
            if session.is_some() {
                self.inner.changes.send_replace(AuthState {
                    event: AuthChangeEvent::SignedOut,
                    session: None,
                });
            }
            session
        };
        let Some(AuthSession {
            refresh_token: Some(_),
            session_id: Some(session_id),
            org_id: Some(org_id),
            ..
        }) = session
        else {
            return Ok(());
        };
        let form = [
            ("client_id", self.inner.client_id.as_str()),
            ("session_id", session_id.as_str()),
            ("org_id", org_id.as_str()),
        ];
        let res = self
            .inner
            .http
            .post(format!("{}{REVOKE_PATH}", self.inner.base_api_url))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(urlencode_form(&form))
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            return Err(to_api_error(res, status).await);
        }
        Ok(())
    }

    /// A Data Plane client authenticated as the signed-in member, with a
    /// token refreshed first when it is about to expire.
    ///
    /// It implements [`crate::DataPlaneResources`], so `dp.tasks()`,
    /// `dp.issues()`, `dp.connections()` and the rest work on it directly. The
    /// token inside is fixed, so prefer [`Self::with_data_plane`], which also
    /// recovers from a `401`.
    pub async fn data_plane(&self) -> ApiResult<Arc<HttpClient>> {
        Ok(self.data_plane_with_token().await?.0)
    }

    /// Run `op` against the Data Plane as the signed-in member. When it
    /// fails with `401`, refresh the session and run it once more.
    ///
    /// `op` receives a client that implements [`crate::DataPlaneResources`].
    pub async fn with_data_plane<T, F, Fut>(&self, op: F) -> ApiResult<T>
    where
        F: Fn(Arc<HttpClient>) -> Fut,
        Fut: Future<Output = ApiResult<T>>,
    {
        let (http, token) = self.data_plane_with_token().await?;
        match op(http).await {
            Err(err) if err.status() == Some(401) => {
                self.refresh_after_unauthorized(&token).await?;
                op(self.data_plane().await?).await
            }
            result => result,
        }
    }

    /// An [`IntrospectionClient`] holding the current access token, with the
    /// Data Plane URL defaulted to the session's.
    ///
    /// Only its Data Plane namespaces accept a native token: the
    /// [`crate::DataPlaneResources`] set (`tasks`, `files`, `shares`,
    /// `conversations`, `events`, `metrics`, `automations`, `issues`,
    /// `connections`) and `project_labels`. Control Plane calls answer `401`.
    /// The token is not refreshed inside the client: build a new one after
    /// [`Self::refresh`], or use [`Self::with_data_plane`].
    pub async fn client(
        &self,
        advanced: Option<AdvancedOptions>,
    ) -> ApiResult<IntrospectionClient> {
        let session = self.session().await?.ok_or_else(not_signed_in)?;
        let advanced = advanced.unwrap_or_default();
        let options = AdvancedOptions {
            base_api_url: advanced
                .base_api_url
                .or_else(|| Some(self.inner.base_api_url.clone())),
            dp_url: advanced
                .dp_url
                .or_else(|| self.inner.dp_url.clone())
                .or(session.dp_url),
            cp_session: advanced.cp_session,
            additional_headers: advanced.additional_headers,
        };
        IntrospectionClient::new(ClientConfig::with_token(session.access_token).advanced(options))
            .map_err(|e| IntrospectionAPIError::InvalidConfig(e.to_string()))
    }

    async fn data_plane_with_token(&self) -> ApiResult<(Arc<HttpClient>, String)> {
        let session = self.session().await?.ok_or_else(not_signed_in)?;
        let dp_url = self
            .inner
            .dp_url
            .clone()
            .or(session.dp_url)
            .ok_or_else(|| {
                IntrospectionAPIError::InvalidConfig(
                    "no Data Plane URL: the token response carried no dp_url; set EmailCodeAuthConfig::dp_url"
                        .to_string(),
                )
            })?;
        let http = HttpClient::new(HttpConfig {
            api_url: dp_url,
            token: session.access_token.clone(),
            additional_headers: crate::dev_target::with_dev_target(HashMap::new()),
            timeout: Duration::from_secs(defaults::API_TIMEOUT_SECS),
            max_retries: defaults::API_MAX_RETRIES,
            retry_base: Duration::from_millis(defaults::API_RETRY_BASE_MS),
        })?;
        Ok((Arc::new(http), session.access_token))
    }

    /// Refresh unless `observed` (the token the caller saw) has already been
    /// replaced, in which case the replacement is the answer. Holding the
    /// gate across the request is what makes concurrent callers share one.
    async fn refresh_from(&self, observed: Option<String>) -> ApiResult<AuthSession> {
        let _gate = self.inner.refresh_gate.lock().await;
        let (session, generation) = {
            let state = self.lock();
            (state.session.clone(), state.generation)
        };
        let session = session.ok_or_else(not_signed_in)?;
        if observed.is_some_and(|token| token != session.access_token) {
            return Ok(session);
        }
        let (Some(refresh_token), Some(session_id), Some(org_id)) = (
            session.refresh_token.as_deref(),
            session.session_id.as_deref(),
            session.org_id.as_deref(),
        ) else {
            return Err(IntrospectionAPIError::InvalidConfig(
                "the session cannot be refreshed: it has no refresh token, session id or org id"
                    .to_string(),
            ));
        };
        let form = [
            ("grant_type", GRANT_REFRESH_TOKEN),
            ("client_id", self.inner.client_id.as_str()),
            ("refresh_token", refresh_token),
            ("session_id", session_id),
            ("org_id", org_id),
        ];
        let result = post_token_form_with(&self.inner.http, &self.inner.base_api_url, &form).await;
        let mut state = self.lock();
        if state.generation != generation {
            return Err(IntrospectionAPIError::Superseded(
                "a sign-out or another sign-in completed while the session was refreshing"
                    .to_string(),
            ));
        }
        match result {
            Ok(token) => {
                let next = AuthSession::from_token(&token, SystemTime::now(), Some(&session));
                state.session = Some(next.clone());
                self.inner.changes.send_replace(AuthState {
                    event: AuthChangeEvent::TokenRefreshed,
                    session: Some(next.clone()),
                });
                Ok(next)
            }
            Err(err) => {
                if is_rejection(&err) {
                    state.generation += 1;
                    state.session = None;
                    self.inner.changes.send_replace(AuthState {
                        event: AuthChangeEvent::SignedOut,
                        session: None,
                    });
                }
                Err(err)
            }
        }
    }

    fn signed_in(&self, state: &mut State, session: AuthSession) {
        state.generation += 1;
        state.session = Some(session.clone());
        self.inner.changes.send_replace(AuthState {
            event: AuthChangeEvent::SignedIn,
            session: Some(session),
        });
    }

    /// The state lock is never held across an `await`, and the data behind
    /// it has no invariant a panic could break, so a poisoned lock is used
    /// as is.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn not_signed_in() -> IntrospectionAPIError {
    IntrospectionAPIError::InvalidConfig(
        "not signed in: call verify_code or set_session first".to_string(),
    )
}

/// A rejection by the server, as opposed to a network failure or an outage,
/// which must not sign the user out.
fn is_rejection(err: &IntrospectionAPIError) -> bool {
    matches!(err.status(), Some(401 | 403 | 422)) || err.code() == Some("invalid_grant")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(json: serde_json::Value) -> OAuthToken {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_refresh_response_keeps_what_it_omits_from_the_previous_session() {
        let first = AuthSession::from_token(
            &token(serde_json::json!({
                "access_token": "a1",
                "expires_in": 900,
                "refresh_token": "r1",
                "session_id": "s",
                "org_id": "o",
                "project_id": "p",
                "member_id": "m",
                "dp_url": "https://dp.example.com",
            })),
            UNIX_EPOCH + Duration::from_secs(1_000),
            None,
        );
        assert_eq!(first.expires_at, Some(1_900));

        let refreshed = AuthSession::from_token(
            &token(serde_json::json!({
                "access_token": "a2",
                "expires_in": 900,
                "refresh_token": "r2",
                "session_id": "s",
                "org_id": "o",
            })),
            UNIX_EPOCH + Duration::from_secs(2_000),
            Some(&first),
        );
        assert_eq!(refreshed.refresh_token.as_deref(), Some("r2"));
        assert_eq!(refreshed.dp_url.as_deref(), Some("https://dp.example.com"));
        assert_eq!(refreshed.member_id.as_deref(), Some("m"));
        assert_eq!(refreshed.expires_at, Some(2_900));
    }

    #[test]
    fn debug_output_never_carries_a_token() {
        let session = AuthSession::from_token(
            &token(serde_json::json!({
                "access_token": "secret-access",
                "expires_in": 900,
                "refresh_token": "secret-refresh",
            })),
            SystemTime::now(),
            None,
        );
        let printed = format!("{session:?}");
        assert!(!printed.contains("secret-access"));
        assert!(!printed.contains("secret-refresh"));
    }

    #[test]
    fn rejections_are_told_apart_from_outages() {
        let oauth = |status: u16, code: Option<&str>| IntrospectionAPIError::Http {
            message: String::new(),
            status,
            code: code.map(str::to_string),
            request_id: None,
            body: None,
            retry_after: None,
        };
        assert!(is_rejection(&oauth(400, Some("invalid_grant"))));
        assert!(is_rejection(&oauth(401, None)));
        assert!(!is_rejection(&oauth(503, Some("temporarily_unavailable"))));
        assert!(!is_rejection(&oauth(500, None)));
    }
}
