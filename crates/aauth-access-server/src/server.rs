//! The Access Server: verifies each Person Server's token request, asks the
//! resource's policy, and issues an auth token or makes the Person Server
//! wait while someone decides.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aauth_protocol::discovery::{Fetch, JwksResolver};
use aauth_protocol::federation::{AuthTokenRequest, AuthTokenResponse};
use aauth_protocol::httpsig::{self, Incoming, Signer, VerifyOptions};
use aauth_protocol::identifier::{is_server_identifier, trim};
use aauth_protocol::jwk::Jwks;
use aauth_protocol::metadata::{dwk, AccessServerMetadata};
use aauth_protocol::proposal::{read_summary, ProposalReader};
use aauth_protocol::token::{mint, TokenKind};
use aauth_protocol::{Budget, Jwk, Requirement, SigningKey};
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::error::{problem, AAuthError};
use crate::types::{
    AccessDecision, AccessRequest, DecisionStatus, Grant, IssuedAccess, PendingAccess,
    PendingStatus, RequestingAgent, Verdict, VerifiedClaims,
};
use crate::verify::{Keys, Verifier};

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The most an auth token may live (`#auth-token-structure`).
pub const MAX_TOKEN_TTL: Duration = Duration::from_secs(60 * 60);

/// Pending requests are kept this long past their expiry, so a late poll
/// still reads `expired` rather than `not_found`.
const PENDING_RETENTION_SECS: i64 = 24 * 60 * 60;

/// The resource's policy: what it answers each verified request.
#[async_trait]
pub trait Policy: Send + Sync {
    /// Decide `request`.
    async fn decide(&self, request: &AccessRequest) -> Verdict;
}

#[async_trait]
impl<F> Policy for F
where
    F: Fn(&AccessRequest) -> Verdict + Send + Sync,
{
    async fn decide(&self, request: &AccessRequest) -> Verdict {
        self(request)
    }
}

/// What an Access Server tells its operator.
#[async_trait]
pub trait Hooks: Send + Sync {
    /// The policy is waiting on someone: reach whoever decides.
    async fn on_pending(&self, _pending: &PendingAccess) {}
    /// An auth token was issued.
    async fn on_issued(&self, _issued: &IssuedAccess) {}
}

/// No hooks.
pub struct NoHooks;

impl Hooks for NoHooks {}

/// How an Access Server is set up. The issuer, the policy and the parties it
/// trusts are required; everything else has a default.
pub struct Config {
    issuer: String,
    policy: Arc<dyn Policy>,
    resources: Vec<String>,
    person_servers: Vec<String>,
    agent_providers: Vec<String>,
    signing_key: Option<SigningKey>,
    hooks: Arc<dyn Hooks>,
    fetch: Option<Arc<dyn Fetch>>,
    base_path: String,
    name: Option<String>,
    description: Option<String>,
    token_ttl: Duration,
    pending_ttl: Duration,
    retry_after: Duration,
    now: Clock,
}

fn trimmed<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    values
        .into_iter()
        .map(|v| trim(v.as_ref()).to_string())
        .collect()
}

impl Config {
    /// `issuer` is `https://host`, with no port or path; `policy` decides.
    /// Name the resources it decides for, and the Person Servers and agent
    /// providers it takes requests from: by default none.
    pub fn new(issuer: &str, policy: impl Policy + 'static) -> Self {
        Self {
            issuer: trim(issuer).to_string(),
            policy: Arc::new(policy),
            resources: Vec::new(),
            person_servers: Vec::new(),
            agent_providers: Vec::new(),
            signing_key: None,
            hooks: Arc::new(NoHooks),
            fetch: None,
            base_path: String::new(),
            name: None,
            description: None,
            token_ttl: MAX_TOKEN_TTL,
            pending_ttl: Duration::from_secs(24 * 60 * 60),
            retry_after: Duration::from_secs(5),
            now: Arc::new(aauth_protocol::now_unix),
        }
    }

    /// The resources it decides for, by their `iss`. A resource token from
    /// any other is refused before its keys are fetched.
    pub fn resources<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, resources: I) -> Self {
        self.resources = trimmed(resources);
        self
    }

    /// The Person Servers it takes token requests from.
    pub fn person_servers<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, servers: I) -> Self {
        self.person_servers = trimmed(servers);
        self
    }

    /// The agent providers whose agents it serves.
    pub fn agent_providers<I: IntoIterator<Item = S>, S: AsRef<str>>(
        mut self,
        providers: I,
    ) -> Self {
        self.agent_providers = trimmed(providers);
        self
    }

    /// The token-signing key. Unset, a fresh key per process.
    pub fn signing_key(mut self, key: SigningKey) -> Self {
        self.signing_key = Some(key);
        self
    }

    /// Tell `hooks` about pending requests and issued tokens.
    pub fn hooks(mut self, hooks: impl Hooks + 'static) -> Self {
        self.hooks = Arc::new(hooks);
        self
    }

    /// How resource, agent provider, Person Server and R3 documents are
    /// fetched. Unset, a guarded reqwest client (with the `reqwest` feature).
    pub fn fetch(mut self, fetch: Arc<dyn Fetch>) -> Self {
        self.fetch = Some(fetch);
        self
    }

    /// Where the routes live under the issuer, e.g. `/as`. Default: the root.
    pub fn base_path(mut self, base: &str) -> Self {
        self.base_path = trim(base).to_string();
        self
    }

    /// A display name, in the metadata document.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// A display description, in the metadata document.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// How long an auth token lasts, at most; never more than an hour.
    pub fn token_ttl(mut self, ttl: Duration) -> Self {
        self.token_ttl = ttl.min(MAX_TOKEN_TTL);
        self
    }

    /// How long a request may wait on a decision. Default: 24 hours.
    pub fn pending_ttl(mut self, ttl: Duration) -> Self {
        self.pending_ttl = ttl;
        self
    }

    /// The `Retry-After` a pending answer carries. Default: 5 seconds.
    pub fn retry_after(mut self, after: Duration) -> Self {
        self.retry_after = after;
        self
    }

    /// Seconds since the epoch; for tests.
    pub fn clock(mut self, now: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.now = Arc::new(now);
        self
    }
}

/// The URLs an Access Server answers at.
#[derive(Clone, Debug, Serialize)]
pub struct Endpoints {
    /// `POST`: a Person Server's token request.
    pub token: String,
    pending_prefix: String,
}

impl Endpoints {
    /// Where a Person Server polls pending request `id`.
    pub fn pending(&self, id: &str) -> String {
        format!("{}{id}", self.pending_prefix)
    }
}

/// An Access Server. Build one with [`AccessServer::new`] from a [`Config`],
/// then hand it requests with [`AccessServer::handle`].
pub struct AccessServer {
    config: Config,
    key: SigningKey,
    fetch: Arc<dyn Fetch>,
    authority: String,
    keys: Keys,
    endpoints: Endpoints,
    pending: Mutex<HashMap<String, PendingAccess>>,
}

fn secs(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}

fn json_response(status: StatusCode, body: &impl Serialize) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from(serde_json::to_vec(body).unwrap_or_default()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    response
}

fn words(value: Option<&str>) -> Vec<&str> {
    value
        .map(|s| s.split(' ').filter(|w| !w.is_empty()).collect())
        .unwrap_or_default()
}

/// `scope`, unless it is broader than `asked`; `asked` when no scope is given.
fn narrowed(asked: Option<&str>, scope: Option<&str>) -> Result<Option<String>, AAuthError> {
    let Some(scope) = scope else {
        return Ok(asked.map(str::to_string));
    };
    let allowed = words(asked);
    if words(Some(scope)).iter().any(|s| !allowed.contains(s)) {
        return Err(AAuthError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "the policy granted a scope that was not asked for",
        ));
    }
    Ok(Some(scope.to_string()))
}

fn text(claims: &Map<String, Value>, name: &str) -> Option<String> {
    claims
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// What an auth token for a resource token carries besides its own claims.
fn grant_for(rt: &Map<String, Value>) -> Grant {
    Grant {
        aud: text(rt, "iss").unwrap_or_default(),
        ps: text(rt, "ps").unwrap_or_default(),
        sub: text(rt, "sub").unwrap_or_default(),
        account: text(rt, "account"),
        mission_s256: text(rt, "mission_s256"),
        tenant: text(rt, "tenant"),
        r3_uri: text(rt, "r3_uri"),
        r3_s256: text(rt, "r3_s256"),
    }
}

/// A verified request, with what its auth token would be issued with.
struct Asked {
    request: AccessRequest,
    grant: Grant,
    jwk: Jwk,
    not_after: i64,
}

impl AccessServer {
    /// An Access Server as `config` describes it. Fails when the issuer is
    /// not a server identifier, it names no resource, Person Server or agent
    /// provider, or no [`Fetch`] is given without the `reqwest` feature.
    pub fn new(mut config: Config) -> aauth_protocol::Result<Self> {
        if !is_server_identifier(&config.issuer) {
            return Err(aauth_protocol::Error::Invalid(format!(
                "issuer {} must be https://host with no port or path",
                config.issuer
            )));
        }
        if config.resources.is_empty()
            || config.person_servers.is_empty()
            || config.agent_providers.is_empty()
        {
            return Err(aauth_protocol::Error::Invalid(
                "name the resources, Person Servers and agent providers it serves".into(),
            ));
        }
        let fetch = match config.fetch.take() {
            Some(fetch) => fetch,
            None => default_fetch()?,
        };
        let key = config
            .signing_key
            .take()
            .unwrap_or_else(SigningKey::generate);
        let authority = url::Url::parse(&config.issuer)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let url = |path: &str| format!("{}{}{path}", config.issuer, config.base_path);
        let endpoints = Endpoints {
            token: url("/token"),
            pending_prefix: url("/pending/"),
        };
        let keys = Keys {
            issuer: config.issuer.clone(),
            own: key.jwks(),
            resources: config.resources.clone(),
            person_servers: config.person_servers.clone(),
            agent_providers: config.agent_providers.clone(),
            inner: JwksResolver::new(fetch.clone()),
        };
        Ok(Self {
            config,
            key,
            fetch,
            authority,
            keys,
            endpoints,
            pending: Mutex::new(HashMap::new()),
        })
    }

    /// Its server identifier.
    pub fn issuer(&self) -> &str {
        &self.config.issuer
    }

    /// The URLs it answers at.
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// Its `aauth-access.json` (`#access-server-metadata`).
    pub fn metadata(&self) -> AccessServerMetadata {
        AccessServerMetadata {
            issuer: self.config.issuer.clone(),
            jwks_uri: format!("{}/.well-known/jwks.json", self.config.issuer),
            auth_token_endpoint: self.endpoints.token.clone(),
            revocation_endpoint: None,
            name: self.config.name.clone(),
            description: self.config.description.clone(),
        }
    }

    /// The keys its auth tokens are signed with.
    pub fn jwks(&self) -> Jwks {
        self.key.jwks()
    }

    /// A pending request, by its id.
    pub fn pending(&self, id: &str) -> Option<PendingAccess> {
        self.lock().get(id).cloned()
    }

    /// Every pending request it keeps, decided or not.
    pub fn pendings(&self) -> Vec<PendingAccess> {
        self.lock().values().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingAccess>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn now(&self) -> i64 {
        (self.config.now)()
    }

    /// Answer any request; AAuth errors become problem responses and a
    /// request this server does not serve a `404`.
    pub async fn handle(&self, request: Request<Bytes>) -> Response<Bytes> {
        let path = request.uri().path().to_string();
        self.handle_at(request, &path).await
    }

    /// [`Self::handle`], for the `pathname` the Person Server addressed when
    /// a framework rewrote the request's URI.
    pub async fn handle_at(&self, request: Request<Bytes>, pathname: &str) -> Response<Bytes> {
        match self.route(&request, pathname).await {
            Ok(Some(response)) => response,
            Ok(None) => problem(StatusCode::NOT_FOUND, "not_found", None),
            Err(error) => error.into_response(),
        }
    }

    /// The response to a request this server serves; `None` for any other.
    pub async fn route(
        &self,
        request: &Request<Bytes>,
        pathname: &str,
    ) -> Result<Option<Response<Bytes>>, AAuthError> {
        let method = request.method();
        if method == Method::GET && pathname == format!("/.well-known/{}", dwk::ACCESS) {
            return Ok(Some(json_response(StatusCode::OK, &self.metadata())));
        }
        if method == Method::GET && pathname == "/.well-known/jwks.json" {
            return Ok(Some(json_response(StatusCode::OK, &self.jwks())));
        }
        let Some(path) = pathname.strip_prefix(self.config.base_path.as_str()) else {
            return Ok(None);
        };
        if !self.config.base_path.is_empty() && !path.starts_with('/') {
            return Ok(None);
        }
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        Ok(Some(match (method, segments.as_slice()) {
            (&Method::POST, ["token"]) => self.request_token(request, path).await?,
            (&Method::GET, ["pending", id]) if !id.is_empty() => {
                self.poll(request, path, id).await?
            }
            _ => return Ok(None),
        }))
    }

    /// The Person Server that signed a request: `sig=jwks_uri` under its
    /// `aauth-person.json` (`#keying-material`), one this server takes
    /// requests from. Only those servers' keys are ever fetched.
    async fn person_server(
        &self,
        request: &Request<Bytes>,
        path: &str,
    ) -> Result<String, AAuthError> {
        let full_path = format!("{}{path}", self.config.base_path);
        let body = request.body();
        let verified = httpsig::verify(
            &Incoming {
                method: request.method().as_str(),
                authority: &self.authority,
                path: &full_path,
                query: request.uri().query(),
                headers: request.headers(),
                body: (!body.is_empty()).then_some(body.as_ref()),
            },
            &VerifyOptions {
                now: self.now(),
                require_content_digest: true,
                ..VerifyOptions::default()
            },
            &self.keys,
        )
        .await
        .map_err(|e| AAuthError::signature(e.code, e.detail))?;
        match verified.signer {
            Signer::JwksUri {
                id, dwk: document, ..
            } if document == dwk::PERSON => Ok(id),
            _ => Err(AAuthError::signature(
                "unsupported_scheme",
                "a Person Server signs with sig=jwks_uri and its aauth-person.json",
            )),
        }
    }

    /// The request, verified, as the policy sees it.
    async fn read_request(&self, ps: &str, body: &[u8]) -> Result<Asked, AAuthError> {
        let tokens: AuthTokenRequest = serde_json::from_slice(body).map_err(|_| {
            AAuthError::bad(
                "invalid_request",
                "resource_token, agent_token and presented_token are required",
            )
        })?;
        if tokens.subagent_token.is_some() || tokens.upstream_token.is_some() {
            return Err(AAuthError::bad(
                "invalid_request",
                "subagent_token and upstream_token are not served yet",
            ));
        }
        let now = self.now();
        let verifier = Verifier {
            keys: &self.keys,
            now,
        };
        let rt = verifier.resource_token(&tokens.resource_token, ps).await?;
        let agent = verifier.agent_token(&tokens.agent_token, &rt, ps).await?;
        let presented = verifier
            .presented_token(&tokens.presented_token, &rt, ps)
            .await?;
        let summary = read_summary(
            &rt,
            &ProposalReader {
                issuer: &self.config.issuer,
                dwk: dwk::ACCESS,
                key: &self.key,
                fetch: self.fetch.as_ref(),
                now,
            },
        )
        .await;
        let grant = grant_for(&rt);
        let request = AccessRequest {
            resource: grant.aud.clone(),
            ps: ps.to_string(),
            sub: grant.sub.clone(),
            tenant: grant.tenant.clone(),
            scope: text(&rt, "scope"),
            account: grant.account.clone(),
            mission_s256: grant.mission_s256.clone(),
            budget: Budget::from_value(rt.get("budget")).ok().flatten(),
            summary,
            agent: RequestingAgent {
                id: agent.sub.clone(),
                provider: agent.iss.clone(),
                jkt: text(&rt, "agent_jkt").unwrap_or_default(),
            },
            claims: VerifiedClaims {
                resource_token: rt.clone(),
                agent_token: agent.claims.clone(),
                presented_token: presented.claims.clone(),
            },
        };
        Ok(Asked {
            request,
            grant,
            jwk: agent.cnf_jwk.clone(),
            not_after: agent.exp.min(presented.exp),
        })
    }

    /// An auth token (`#auth-token-structure`): `iss` this server, for no
    /// longer than an hour, the agent token, or the presented token.
    async fn issue(
        &self,
        request: &AccessRequest,
        grant: &Grant,
        jwk: &Jwk,
        not_after: i64,
        scope: Option<String>,
    ) -> Response<Bytes> {
        let iat = self.now();
        let exp = (iat + secs(self.config.token_ttl)).min(not_after);
        let jti = uuid::Uuid::now_v7().to_string();
        let mut claims = match serde_json::to_value(grant) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };
        claims.insert("iss".into(), json!(self.config.issuer));
        claims.insert("dwk".into(), json!(dwk::ACCESS));
        claims.insert("jti".into(), json!(jti));
        claims.insert("iat".into(), json!(iat));
        claims.insert("exp".into(), json!(exp));
        if let Some(scope) = &scope {
            claims.insert("scope".into(), json!(scope));
        }
        claims.insert("cnf".into(), json!({ "jwk": jwk }));
        let auth_token = mint(&self.key, TokenKind::Auth, &Value::Object(claims));
        self.config
            .hooks
            .on_issued(&IssuedAccess {
                request: request.clone(),
                jti,
                scope,
                exp,
            })
            .await;
        json_response(
            StatusCode::OK,
            &AuthTokenResponse {
                auth_token,
                expires_in: exp - iat,
            },
        )
    }

    fn waiting(&self, record: &PendingAccess) -> Response<Bytes> {
        let mut response = json_response(StatusCode::ACCEPTED, &json!({ "status": "pending" }));
        let headers = response.headers_mut();
        for (name, value) in [
            ("location", self.endpoints.pending(&record.id)),
            ("retry-after", self.config.retry_after.as_secs().to_string()),
            ("cache-control", "no-store".to_string()),
            (
                aauth_protocol::requirement::HEADER,
                record.requirement.to_header(),
            ),
        ] {
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.insert(name, value);
            }
        }
        response
    }

    /// `#as-token-endpoint`: a Person Server's signed request for an auth token.
    async fn request_token(
        &self,
        request: &Request<Bytes>,
        path: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let ps = self.person_server(request, path).await?;
        let asked = self.read_request(&ps, request.body()).await?;
        let requirement = match self.config.policy.decide(&asked.request).await {
            Verdict::Allow { scope } => {
                let scope = narrowed(asked.request.scope.as_deref(), scope.as_deref())?;
                return Ok(self
                    .issue(
                        &asked.request,
                        &asked.grant,
                        &asked.jwk,
                        asked.not_after,
                        scope,
                    )
                    .await);
            }
            Verdict::Deny { detail } => {
                return Ok(problem(
                    StatusCode::FORBIDDEN,
                    "denied",
                    Some(
                        detail
                            .as_deref()
                            .unwrap_or("this resource's policy refuses it"),
                    ),
                ))
            }
            Verdict::Approval => Requirement::Approval,
            Verdict::Interaction { url, code } => Requirement::Interaction {
                url: Some(url),
                code,
            },
        };
        let created = self.now();
        let record = PendingAccess {
            id: uuid::Uuid::now_v7().to_string(),
            status: PendingStatus::Pending,
            requirement,
            request: asked.request,
            grant: asked.grant,
            jwk: asked.jwk,
            not_after: asked.not_after,
            scope: None,
            decided_by: None,
            answered: false,
            created_at: created,
            expires_at: (created + secs(self.config.pending_ttl)).min(asked.not_after),
        };
        {
            let mut pending = self.lock();
            pending.retain(|_, p| p.expires_at + PENDING_RETENTION_SECS > created);
            pending.insert(record.id.clone(), record.clone());
        }
        self.config.hooks.on_pending(&record).await;
        Ok(self.waiting(&record))
    }

    /// The Person Server polls with a signed `GET`; only the one that asked
    /// may. A terminal answer is given once: later polls are `410`.
    async fn poll(
        &self,
        request: &Request<Bytes>,
        path: &str,
        id: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let ps = self.person_server(request, path).await?;
        let now = self.now();
        let collecting = {
            let mut pending = self.lock();
            let Some(record) = pending.get_mut(id).filter(|r| r.request.ps == ps) else {
                return Ok(problem(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    Some("no such pending request"),
                ));
            };
            if record.answered {
                return Ok(problem(
                    StatusCode::GONE,
                    "invalid_request",
                    Some("this request was answered already"),
                ));
            }
            match record.status {
                PendingStatus::Denied => {
                    record.answered = true;
                    return Ok(problem(
                        StatusCode::FORBIDDEN,
                        "denied",
                        Some("this resource's policy refuses it"),
                    ));
                }
                _ if now >= record.expires_at => {
                    record.answered = true;
                    return Ok(problem(
                        StatusCode::REQUEST_TIMEOUT,
                        "expired",
                        Some("nobody decided in time"),
                    ));
                }
                PendingStatus::Pending => return Ok(self.waiting(record)),
                PendingStatus::Approved => {
                    record.status = PendingStatus::Collected;
                    record.answered = true;
                    record.clone()
                }
                _ => {
                    return Ok(problem(
                        StatusCode::GONE,
                        "invalid_request",
                        Some("its auth token was issued already"),
                    ))
                }
            }
        };
        let scope = match &collecting.scope {
            Some(scope) => Some(scope.clone()),
            None => collecting.request.scope.clone(),
        };
        Ok(self
            .issue(
                &collecting.request,
                &collecting.grant,
                &collecting.jwk,
                collecting.not_after,
                scope,
            )
            .await)
    }

    /// Settle a request the policy waited on. A decision is made once: a
    /// second is a `409`, and one past its expiry a `410`.
    pub fn decide(&self, id: &str, decision: AccessDecision) -> Result<PendingAccess, AAuthError> {
        let now = self.now();
        let mut pending = self.lock();
        let Some(record) = pending.get_mut(id) else {
            return Err(AAuthError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "no such request",
            ));
        };
        if record.status != PendingStatus::Pending {
            return Err(AAuthError::new(
                StatusCode::CONFLICT,
                "invalid_request",
                "already decided",
            ));
        }
        if now >= record.expires_at {
            return Err(AAuthError::new(
                StatusCode::GONE,
                "expired",
                "this request has expired",
            ));
        }
        if decision.scope.is_some() {
            record.scope = narrowed(record.request.scope.as_deref(), decision.scope.as_deref())?;
        }
        record.status = match decision.status {
            DecisionStatus::Approved => PendingStatus::Approved,
            DecisionStatus::Denied => PendingStatus::Denied,
        };
        record.decided_by = decision.decided_by;
        Ok(record.clone())
    }
}

#[cfg(feature = "reqwest")]
fn default_fetch() -> aauth_protocol::Result<Arc<dyn Fetch>> {
    use aauth_protocol::client::DeniedNetworks;
    Ok(Arc::new(aauth_protocol::discovery::ReqwestFetch::guarded(
        DeniedNetworks::default(),
    )?))
}

#[cfg(not(feature = "reqwest"))]
fn default_fetch() -> aauth_protocol::Result<Arc<dyn Fetch>> {
    Err(aauth_protocol::Error::Invalid(
        "without the reqwest feature, give the Access Server a Fetch".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_may_narrow_the_scope_but_never_widen_it() {
        assert_eq!(
            narrowed(Some("read write"), Some("read")).unwrap(),
            Some("read".into())
        );
        assert_eq!(narrowed(Some("read"), None).unwrap(), Some("read".into()));
        let widened = narrowed(Some("read"), Some("read admin")).unwrap_err();
        assert_eq!(widened.error, "server_error");
    }
}
