//! The Person Server: verifies each agent by its HTTP signature and agent
//! token, approves missions, issues person and auth tokens, and weighs each
//! auth token request against its mission's budget. Whatever needs a human
//! becomes an [`Approval`], decided through [`PersonServer::decide`] or the
//! management routes.

use std::sync::Arc;
use std::time::Duration;

use aauth_protocol::discovery::{Fetch, JwksResolver, KeyResolver, NoKeys};
use aauth_protocol::httpsig::{self, Incoming, SignatureKey, VerifyOptions};
use aauth_protocol::identifier::{is_server_identifier, trim};
use aauth_protocol::jwk::{Algorithm, Jwks};
use aauth_protocol::metadata::{dwk, PersonServerMetadata};
use aauth_protocol::token::{mint, verify_token, TokenKind, VerifyTokenOptions};
use aauth_protocol::{jwt, Budget, SigningKey};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use bytes::Bytes;
use hmac::{Hmac, Mac};
use http::{HeaderValue, Method, Request, Response, StatusCode};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::Sha256;

use crate::directory::{Directory, Hooks, NoHooks};
use crate::error::{problem, AAuthError};
use crate::store::{MemoryStore, Spend, Store};
use crate::time::rfc3339;
use crate::types::{
    Agent, Approval, ApprovalKind, ApprovalStatus, Decision, Issued, IssuedKind, LogEntry, Mission,
    Person, Verdict,
};

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
type Trust = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Which tenant a request that names none is under.
#[derive(Clone, Debug)]
enum Tenancy {
    /// This one.
    Default(String),
    /// None: such a request is refused.
    Required,
}

/// How a Person Server is set up. `issuer`, the directory and the trusted
/// agent providers are required; everything else has a default.
pub struct Config {
    // Required.
    issuer: String,
    directory: Arc<dyn Directory>,
    trusted: Trust,
    // Keys: generated per process when unset.
    signing_key: Option<SigningKey>,
    subject_secret: Option<String>,
    // What it runs on.
    store: Arc<dyn Store>,
    hooks: Arc<dyn Hooks>,
    fetch: Option<Arc<dyn Fetch>>,
    // What it serves.
    base_path: String,
    tenancy: Tenancy,
    admin_token: Option<String>,
    // Its metadata.
    name: Option<String>,
    description: Option<String>,
    // Timings.
    token_ttl: Duration,
    mission_ttl: Duration,
    approval_ttl: Duration,
    retry_after: Duration,
    now: Clock,
}

/// The tenant a request that names none is under, unless the server is
/// set up otherwise.
pub const DEFAULT_TENANT: &str = "default";

impl Config {
    /// `issuer` is `https://host`, with no port or path. Agents may ask only
    /// when their agent token's `iss` is one of `trusted_agent_providers`.
    pub fn new<I, S>(
        issuer: &str,
        directory: impl Directory + 'static,
        trusted_agent_providers: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let trusted: Vec<String> = trusted_agent_providers
            .into_iter()
            .map(|p| trim(p.as_ref()).to_string())
            .collect();
        Self {
            issuer: trim(issuer).to_string(),
            directory: Arc::new(directory),
            trusted: Arc::new(move |iss| trusted.iter().any(|t| t == iss)),
            signing_key: None,
            subject_secret: None,
            store: Arc::new(MemoryStore::new()),
            hooks: Arc::new(NoHooks),
            fetch: None,
            base_path: String::new(),
            tenancy: Tenancy::Default(DEFAULT_TENANT.to_string()),
            admin_token: None,
            name: None,
            description: None,
            token_ttl: Duration::from_secs(60 * 60),
            mission_ttl: Duration::from_secs(14 * 24 * 60 * 60),
            approval_ttl: Duration::from_secs(24 * 60 * 60),
            retry_after: Duration::from_secs(5),
            now: Arc::new(aauth_protocol::now_unix),
        }
    }

    /// The tenant a request that names none is under. Default:
    /// [`DEFAULT_TENANT`]. A company's own Person Server names its own.
    pub fn default_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenancy = Tenancy::Default(tenant.into());
        self
    }

    /// Refuse a request that names no tenant: for a Person Server that
    /// serves several.
    pub fn require_tenant(mut self) -> Self {
        self.tenancy = Tenancy::Required;
        self
    }

    /// Decide trust in an agent provider by its `iss` instead of a list.
    pub fn trust(mut self, trusted: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.trusted = Arc::new(trusted);
        self
    }

    /// The token-signing key. Unset, a fresh key per process.
    pub fn signing_key(mut self, key: SigningKey) -> Self {
        self.signing_key = Some(key);
        self
    }

    /// Derives each person's directed `sub` per audience. Unset, a fresh secret per process.
    pub fn subject_secret(mut self, secret: impl Into<String>) -> Self {
        self.subject_secret = Some(secret.into());
        self
    }

    /// Where state is kept; in memory by default.
    pub fn store(mut self, store: Arc<dyn Store>) -> Self {
        self.store = store;
        self
    }

    /// Where the routes live under the issuer, e.g. `/ps`. Default: the root.
    pub fn base_path(mut self, base: &str) -> Self {
        self.base_path = trim(base).to_string();
        self
    }

    /// Tell `hooks` about approvals, decisions and issued tokens.
    pub fn hooks(mut self, hooks: impl Hooks + 'static) -> Self {
        self.hooks = Arc::new(hooks);
        self
    }

    /// Bearer token for the management routes (`/consent`, `/missions`). Unset
    /// or empty, they are off.
    pub fn admin_token(mut self, token: impl Into<String>) -> Self {
        self.admin_token = Some(token.into()).filter(|t: &String| !t.is_empty());
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

    /// How long a person or auth token lasts, at most. Default: an hour.
    pub fn token_ttl(mut self, ttl: Duration) -> Self {
        self.token_ttl = ttl;
        self
    }

    /// How long an approved mission lasts. Default: 14 days.
    pub fn mission_ttl(mut self, ttl: Duration) -> Self {
        self.mission_ttl = ttl;
        self
    }

    /// How long an approval waits for a decision. Default: 24 hours.
    pub fn approval_ttl(mut self, ttl: Duration) -> Self {
        self.approval_ttl = ttl;
        self
    }

    /// The `Retry-After` a pending answer carries, in whole seconds. Default:
    /// 5 seconds.
    pub fn retry_after(mut self, after: Duration) -> Self {
        self.retry_after = after;
        self
    }

    /// How agent provider, resource and R3 documents are fetched. Unset,
    /// a guarded reqwest client (with the `reqwest` feature).
    pub fn fetch(mut self, fetch: Arc<dyn Fetch>) -> Self {
        self.fetch = Some(fetch);
        self
    }

    /// Seconds since the epoch; for tests.
    pub fn clock(mut self, now: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.now = Arc::new(now);
        self
    }
}

/// The URLs a Person Server answers at.
#[derive(Clone, Debug, Serialize)]
pub struct Endpoints {
    /// `POST`: a person token.
    pub person_token: String,
    /// `POST`: an auth token, for a resource token.
    pub token: String,
    /// `POST`: propose a mission; `POST {mission}/{s256}` acts on one.
    pub mission: String,
    pending_prefix: String,
}

impl Endpoints {
    /// Where the agent polls approval `id`, asked under `tenant`.
    pub fn pending(&self, tenant: &str, id: &str) -> String {
        format!(
            "{}{}/{id}",
            self.pending_prefix,
            utf8_percent_encode(tenant, SEGMENT)
        )
    }
}

/// A Person Server. Build one with [`PersonServer::new`] from a [`Config`],
/// then hand it requests with [`PersonServer::handle`].
pub struct PersonServer {
    issuer: String,
    authority: String,
    base: String,
    directory: Arc<dyn Directory>,
    trusted: Trust,
    key: SigningKey,
    secret: String,
    store: Arc<dyn Store>,
    hooks: Arc<dyn Hooks>,
    admin_token: Option<String>,
    name: Option<String>,
    description: Option<String>,
    token_ttl: i64,
    mission_ttl: i64,
    approval_ttl: i64,
    retry_after: u64,
    fetch: Arc<dyn Fetch>,
    resolver: JwksResolver,
    now: Clock,
    endpoints: Endpoints,
    tenancy: Tenancy,
}

/// A duration in whole seconds, as token and mission times are kept.
fn secs(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the operating system provides randomness");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_body(body: &[u8]) -> Result<Map<String, Value>, AAuthError> {
    match serde_json::from_slice(body) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(AAuthError::invalid_request("body must be a JSON object")),
    }
}

fn str_of<'a>(params: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    params.get(name).and_then(Value::as_str)
}

fn json_response(status: StatusCode, body: &impl Serialize) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from(serde_json::to_vec(body).unwrap_or_default()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    response
}

fn ok(body: &impl Serialize) -> Response<Bytes> {
    json_response(StatusCode::OK, body)
}

fn mission_not_found() -> AAuthError {
    AAuthError::new(
        StatusCode::NOT_FOUND,
        "mission_not_found",
        "no such mission",
    )
}

/// What a tenant escapes to in a pending URL's path segment.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.');

/// The tenant a request body names, if any.
fn named(params: &Map<String, Value>) -> Option<&str> {
    str_of(params, "tenant").filter(|t| !t.is_empty())
}

/// A pending approval's grant claims, kept to the scalar claims a resource token carries.
const GRANT_CLAIMS: &[&str] = &["scope", "r3_uri", "r3_s256"];

impl PersonServer {
    /// A Person Server as `config` describes it. Fails when the issuer is
    /// not a server identifier, or no [`Fetch`] is given without the
    /// `reqwest` feature.
    pub fn new(config: Config) -> aauth_protocol::Result<Self> {
        if !is_server_identifier(&config.issuer) {
            return Err(aauth_protocol::Error::Invalid(format!(
                "issuer {} must be https://host with no port or path",
                config.issuer
            )));
        }
        let fetch = match config.fetch {
            Some(fetch) => fetch,
            None => default_fetch()?,
        };
        let authority = url::Url::parse(&config.issuer)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let url = |path: &str| format!("{}{}{path}", config.issuer, config.base_path);
        let endpoints = Endpoints {
            person_token: url("/person-token"),
            token: url("/token"),
            mission: url("/mission"),
            pending_prefix: url("/pending/"),
        };
        Ok(Self {
            authority,
            base: config.base_path,
            directory: config.directory,
            trusted: config.trusted,
            key: config.signing_key.unwrap_or_else(SigningKey::generate),
            secret: config.subject_secret.unwrap_or_else(|| random_hex(32)),
            store: config.store,
            hooks: config.hooks,
            admin_token: config.admin_token,
            name: config.name,
            description: config.description,
            token_ttl: secs(config.token_ttl),
            mission_ttl: secs(config.mission_ttl),
            approval_ttl: secs(config.approval_ttl),
            retry_after: config.retry_after.as_secs(),
            resolver: JwksResolver::new(fetch.clone()),
            fetch,
            now: config.now,
            endpoints,
            tenancy: config.tenancy,
            issuer: config.issuer,
        })
    }

    /// Its server identifier.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The URLs it answers at.
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// Where its state is kept.
    pub fn store(&self) -> &Arc<dyn Store> {
        &self.store
    }

    /// Its `aauth-person.json`.
    pub fn metadata(&self) -> PersonServerMetadata {
        PersonServerMetadata {
            issuer: self.issuer.clone(),
            auth_token_endpoint: self.endpoints.token.clone(),
            person_token_endpoint: Some(self.endpoints.person_token.clone()),
            mission_endpoint: Some(self.endpoints.mission.clone()),
            jwks_uri: Some(format!("{}/.well-known/jwks.json", self.issuer)),
            name: self.name.clone(),
            description: self.description.clone(),
        }
    }

    /// The keys its tokens are signed with, as `/.well-known/jwks.json` serves them.
    pub fn jwks(&self) -> Jwks {
        self.key.jwks()
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    fn default_tenant(&self) -> Option<&str> {
        match &self.tenancy {
            Tenancy::Default(tenant) => Some(tenant),
            Tenancy::Required => None,
        }
    }

    /// The tenant a request is under: the one it names, else the default.
    fn tenant(&self, named: Option<&str>) -> Result<String, AAuthError> {
        named
            .map(str::to_string)
            .or_else(|| self.default_tenant().map(str::to_string))
            .ok_or_else(|| AAuthError::invalid_request("name the tenant"))
    }

    /// Answer any request; AAuth errors become problem responses and a
    /// request this server does not serve a `404`.
    pub async fn handle(&self, request: Request<Bytes>) -> Response<Bytes> {
        let path = request.uri().path().to_string();
        self.handle_at(request, &path).await
    }

    /// [`Self::handle`], for the `pathname` the agent addressed when a
    /// framework rewrote the request's URI.
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
        if method == Method::GET && pathname == format!("/.well-known/{}", dwk::PERSON) {
            return Ok(Some(ok(&self.metadata())));
        }
        if method == Method::GET && pathname == "/.well-known/jwks.json" {
            return Ok(Some(ok(&self.jwks())));
        }
        let Some(path) = pathname.strip_prefix(self.base.as_str()) else {
            return Ok(None);
        };
        if !self.base.is_empty() && !path.starts_with('/') {
            return Ok(None);
        }
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let response = match (method, segments.as_slice()) {
            (&Method::POST, ["mission"]) => self.propose_mission(request, path).await?,
            (&Method::POST, ["mission", s256]) if !s256.is_empty() => {
                self.mission_action(request, path, s256).await?
            }
            (&Method::POST, ["person-token"]) => self.request_person_token(request, path).await?,
            (&Method::POST, ["token"]) => self.request_auth_token(request, path).await?,
            (&Method::GET | &Method::DELETE, ["pending", id]) if !id.is_empty() => {
                let tenant = self.tenant(None)?;
                self.pending_state(request, path, &tenant, id).await?
            }
            (&Method::GET | &Method::DELETE, ["pending", tenant, id])
                if !tenant.is_empty() && !id.is_empty() =>
            {
                let tenant = percent_decode_str(tenant).decode_utf8_lossy();
                self.pending_state(request, path, &tenant, id).await?
            }
            _ => return self.manage(request, &segments).await,
        };
        Ok(Some(response))
    }

    /// The request's signature, and the agent token it presents, from a
    /// trusted agent provider.
    async fn verify_agent(
        &self,
        request: &Request<Bytes>,
        path: &str,
    ) -> Result<Agent, AAuthError> {
        let full_path = format!("{}{path}", self.base);
        let body = request.body();
        let verified = httpsig::verify(
            &Incoming {
                method: request.method().as_str(),
                // As the agent addressed it: this identifier and the route's
                // path, whatever routed the request here.
                authority: &self.authority,
                path: &full_path,
                query: request.uri().query(),
                headers: request.headers(),
                body: (!body.is_empty()).then_some(body.as_ref()),
            },
            &VerifyOptions {
                now: self.now(),
                require_content_digest: true,
                algorithms: vec![Algorithm::Ed25519, Algorithm::Es256],
                ..VerifyOptions::default()
            },
            // The scheme must be `jwt`: nothing is fetched on an
            // unauthenticated caller's say-so.
            &NoKeys,
        )
        .await
        .map_err(|e| AAuthError::signature(e.code, e.detail))?;
        let Some(raw) = verified.jwt() else {
            return Err(AAuthError::signature(
                "unsupported_scheme",
                "sign with sig=jwt",
            ));
        };
        let decoded =
            jwt::decode(raw).map_err(|_| AAuthError::signature("invalid_jwt", "not a JWT"))?;
        let provider = trim(decoded.claim_str("iss").unwrap_or_default()).to_string();
        if !(self.trusted)(&provider) {
            return Err(AAuthError::signature(
                "invalid_jwt",
                format!("{provider} is not trusted here"),
            ));
        }
        let token = verify_token(
            &VerifyTokenOptions {
                jwt: raw,
                http_signature_thumbprint: &verified.thumbprint,
                resource: &self.issuer,
                accept: &[TokenKind::Agent],
                now: self.now(),
                clock_tolerance: 60,
            },
            &self.resolver,
        )
        .await
        .map_err(|e| {
            let code = if e.code == "token_expired" {
                "expired_jwt"
            } else {
                "invalid_jwt"
            };
            AAuthError::signature(code, e.detail)
        })?;
        if token.ps().is_some_and(|ps| trim(ps) != self.issuer) {
            return Err(AAuthError::new(
                StatusCode::FORBIDDEN,
                "invalid_request",
                "this agent's Person Server is another",
            ));
        }
        Ok(Agent {
            id: token.sub,
            provider,
            jkt: verified.thumbprint,
            jwk: token.cnf_jwk,
            exp: token.exp,
        })
    }

    /// The person the agent acts for. Each agent key is bound to the first person it names.
    async fn person_for(
        &self,
        agent: &Agent,
        params: &Map<String, Value>,
    ) -> Result<Person, AAuthError> {
        let tenant = self.tenant(named(params))?;
        let tenant = tenant.as_str();
        if let Some(hint) = str_of(params, "login_hint") {
            let Some(hinted) = self.directory.person(hint, tenant).await else {
                return Err(AAuthError::invalid_request(
                    "not someone this Person Server speaks for",
                ));
            };
            // The directory's tenant is the person's; a request can only be
            // under it, never change it.
            if hinted
                .tenant
                .as_deref()
                .is_some_and(|theirs| theirs != tenant)
            {
                return Err(AAuthError::new(
                    StatusCode::FORBIDDEN,
                    "invalid_request",
                    "this person is not in that tenant",
                ));
            }
            // Tokens name a tenant the directory gave or the agent named.
            let hinted = Person {
                tenant: hinted.tenant.or_else(|| named(params).map(str::to_string)),
                ..hinted
            };
            let bound = self.store.bind(tenant, &agent.jkt, &hinted).await?;
            if bound.id != hinted.id {
                return Err(AAuthError::new(
                    StatusCode::FORBIDDEN,
                    "invalid_request",
                    "this agent acts for another person",
                ));
            }
            return Ok(hinted);
        }
        self.store
            .binding(tenant, &agent.jkt)
            .await?
            .ok_or_else(|| AAuthError::invalid_request("name the person: login_hint"))
    }

    /// The person's identifier at one audience: no two audiences can correlate it.
    fn directed(&self, person: &Person, audience: &str) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(self.secret.as_bytes())
            .expect("HMAC takes any key");
        mac.update(format!("{}\n{audience}", person.id).as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    fn issue(
        &self,
        kind: TokenKind,
        claims: Map<String, Value>,
        not_after: i64,
        person: &Person,
    ) -> (String, i64) {
        let iat = self.now();
        let exp = (iat + self.token_ttl).min(not_after);
        let mut all = Map::new();
        all.insert("iss".into(), json!(self.issuer));
        all.insert("dwk".into(), json!(dwk::PERSON));
        all.insert("jti".into(), json!(uuid::Uuid::new_v4().to_string()));
        all.insert("iat".into(), json!(iat));
        all.insert("exp".into(), json!(exp));
        if let Some(tenant) = &person.tenant {
            all.insert("tenant".into(), json!(tenant));
        }
        all.extend(claims);
        (mint(&self.key, kind, &Value::Object(all)), exp - iat)
    }

    async fn person_token(
        &self,
        agent: &Agent,
        person: &Person,
        resource: &str,
        mission: Option<&Mission>,
    ) -> Value {
        let mut claims = Map::new();
        claims.insert("aud".into(), json!(resource));
        claims.insert("sub".into(), json!(self.directed(person, resource)));
        claims.insert("cnf".into(), json!({ "jwk": agent.jwk }));
        if let Some(mission) = mission {
            claims.insert("mission_s256".into(), json!(mission.s256));
        }
        let not_after = agent.exp.min(mission.map_or(i64::MAX, |m| m.expires_at));
        let (token, expires_in) = self.issue(TokenKind::Person, claims, not_after, person);
        self.hooks
            .on_issued(&Issued {
                kind: IssuedKind::PersonToken,
                agent: agent.clone(),
                person: person.clone(),
                resource: resource.to_string(),
                mission: mission.cloned(),
                budget: None,
            })
            .await;
        json!({ "person_token": token, "expires_in": expires_in })
    }

    async fn auth_token(
        &self,
        agent: &Agent,
        mission: &Mission,
        grant: &Map<String, Value>,
    ) -> Value {
        let mut claims = Map::new();
        claims.insert("ps".into(), json!(self.issuer));
        claims.insert("cnf".into(), json!({ "jwk": agent.jwk }));
        claims.insert("mission_s256".into(), json!(mission.s256));
        claims.extend(grant.clone());
        let (token, expires_in) = self.issue(
            TokenKind::Auth,
            claims,
            agent.exp.min(mission.expires_at),
            &mission.person,
        );
        self.hooks
            .on_issued(&Issued {
                kind: IssuedKind::AuthToken,
                agent: agent.clone(),
                person: mission.person.clone(),
                resource: grant
                    .get("aud")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                mission: Some(mission.clone()),
                budget: grant
                    .get("budget")
                    .and_then(|b| serde_json::from_value(b.clone()).ok()),
            })
            .await;
        json!({ "auth_token": token, "expires_in": expires_in })
    }

    async fn request_person_token(
        &self,
        request: &Request<Bytes>,
        path: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let agent = self.verify_agent(request, path).await?;
        let params = read_body(request.body())?;
        let resource = trim(str_of(&params, "resource").unwrap_or_default()).to_string();
        if !resource.starts_with("https://") {
            return Err(AAuthError::invalid_request("resource is an https origin"));
        }
        let person = self.person_for(&agent, &params).await?;
        let mission = match params.get("mission_s256") {
            None | Some(Value::Null) => None,
            Some(hash) => Some(
                self.live_mission(&self.tenant(named(&params))?, &agent, hash.as_str())
                    .await?,
            ),
        };
        if mission.as_ref().is_some_and(|m| m.person.id != person.id) {
            return Err(mission_not_found());
        }
        Ok(ok(&self
            .person_token(&agent, &person, &resource, mission.as_ref())
            .await))
    }

    /// The live mission `s256` names, if it is this agent's; the same 404 otherwise.
    async fn live_mission(
        &self,
        tenant: &str,
        agent: &Agent,
        s256: Option<&str>,
    ) -> Result<Mission, AAuthError> {
        let mission = match s256 {
            Some(s256) => self.store.mission(tenant, s256).await?,
            None => None,
        };
        let Some(mission) = mission.filter(|m| m.agent_id == agent.id) else {
            return Err(mission_not_found());
        };
        // The agent identifier is the provider's; the person is the key's.
        // Another key under the same identifier, bound to someone else, gets
        // the same 404 as a stranger.
        if self.store.binding(tenant, &agent.jkt).await?.map(|p| p.id)
            != Some(mission.person.id.clone())
        {
            return Err(mission_not_found());
        }
        if mission.completed_at.is_some() {
            return Err(AAuthError::new(
                StatusCode::FORBIDDEN,
                "mission_terminated",
                "mission completed",
            ));
        }
        if self.now() >= mission.expires_at {
            return Err(AAuthError::new(
                StatusCode::FORBIDDEN,
                "mission_terminated",
                "mission expired",
            ));
        }
        Ok(mission)
    }

    async fn propose_mission(
        &self,
        request: &Request<Bytes>,
        path: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let agent = self.verify_agent(request, path).await?;
        let params = read_body(request.body())?;
        let person = self.person_for(&agent, &params).await?;
        let description = str_of(&params, "description").unwrap_or_default();
        if description.is_empty() {
            return Err(AAuthError::invalid_request("describe the mission"));
        }
        let resources: Vec<Value> = params
            .get("resources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|r| trim(r).to_string())
            .filter(|r| r.starts_with("https://"))
            .map(Value::from)
            .collect();
        let mut grant = Map::new();
        grant.insert("resources".into(), Value::Array(resources));
        let approval = self.approval(
            &self.tenant(named(&params))?,
            ApprovalKind::Mission,
            agent,
            person,
            description.to_string(),
        );
        self.ask(Approval { grant, ..approval }).await
    }

    async fn mission_action(
        &self,
        request: &Request<Bytes>,
        path: &str,
        s256: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let agent = self.verify_agent(request, path).await?;
        let params = read_body(request.body())?;
        let tenant = self.tenant(named(&params))?;
        let tenant = tenant.as_str();
        let mission = self.live_mission(tenant, &agent, Some(s256)).await?;
        match str_of(&params, "action") {
            Some("update") => {
                let entry = str_of(&params, "description")
                    .unwrap_or_default()
                    .to_string();
                let hash = aauth_protocol::s256(&entry);
                let now = self.now();
                if !self
                    .store
                    .log_mission(tenant, &mission.s256, LogEntry { at: now, entry }, now)
                    .await?
                {
                    return Err(AAuthError::new(
                        StatusCode::FORBIDDEN,
                        "mission_terminated",
                        "mission ended",
                    ));
                }
                Ok(ok(&json!({ "s256": hash })))
            }
            Some("completion") => {
                let summary =
                    str_of(&params, "summary").unwrap_or("The agent says the mission is done");
                let approval = self.approval(
                    tenant,
                    ApprovalKind::Completion,
                    agent,
                    mission.person.clone(),
                    summary.to_string(),
                );
                self.ask(Approval {
                    mission_s256: Some(mission.s256),
                    ..approval
                })
                .await
            }
            _ => Err(AAuthError::invalid_request(
                "action is update or completion",
            )),
        }
    }

    /// A resource token, verified against its issuer's keys, binding a token
    /// this Person Server issued.
    async fn verify_resource_token(
        &self,
        agent: &Agent,
        params: &Map<String, Value>,
    ) -> Result<(Map<String, Value>, Mission, String), AAuthError> {
        let (Some(resource_token), Some(presented_token)) = (
            str_of(params, "resource_token"),
            str_of(params, "presented_token"),
        ) else {
            return Err(AAuthError::invalid_request(
                "resource_token and presented_token are required",
            ));
        };
        let invalid = |detail: &str| {
            AAuthError::new(StatusCode::BAD_REQUEST, "invalid_resource_token", detail)
        };

        let presented = jwt::decode(presented_token)
            .ok()
            .filter(|p| p.kid() == Some(self.key.thumbprint()) || p.kid().is_none())
            .filter(|p| p.verify_signature(&self.key.public_jwk(), Algorithm::Ed25519))
            .filter(|p| p.claim_str("iss") == Some(self.issuer.as_str()))
            .filter(|p| {
                [TokenKind::Person.typ(), TokenKind::Auth.typ()]
                    .contains(&p.typ().unwrap_or_default())
            })
            // Issued to this agent's key, not another's.
            .filter(|p| {
                p.claims
                    .get("cnf")
                    .and_then(|c| c.get("jwk"))
                    .and_then(|j| serde_json::from_value::<aauth_protocol::Jwk>(j.clone()).ok())
                    .and_then(|j| j.thumbprint().ok())
                    .as_deref()
                    == Some(agent.jkt.as_str())
            })
            .ok_or_else(|| {
                AAuthError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_presented_token",
                    "not a live token this Person Server issued",
                )
            })?;
        if presented
            .claims
            .get("exp")
            .and_then(Value::as_i64)
            .is_none_or(|exp| self.now() >= exp)
        {
            return Err(AAuthError::new(
                StatusCode::BAD_REQUEST,
                "expired_presented_token",
                "not a live token this Person Server issued",
            ));
        }

        let unverified = jwt::decode(resource_token).map_err(|_| invalid("not a JWT"))?;
        if unverified.typ() != Some(TokenKind::Resource.typ()) {
            return Err(invalid("not a resource token"));
        }
        let resource = trim(unverified.claim_str("iss").unwrap_or_default()).to_string();
        // The mission's approved resources are the only issuers whose keys are fetched.
        let tenant = self.tenant(named(params))?;
        let tenant = tenant.as_str();
        // The request is under the tenant the presented token was issued
        // under; a token that names none was issued under the default.
        let issued_under = presented
            .claim_str("tenant")
            .map(str::to_string)
            .or_else(|| self.default_tenant().map(str::to_string));
        if issued_under.as_deref() != Some(tenant) {
            return Err(invalid("tenant does not match the presented token"));
        }
        let mission = match unverified.claim_str("mission_s256") {
            Some(s256) => self.live_mission(tenant, agent, Some(s256)).await?,
            None => {
                return Err(AAuthError::new(
                    StatusCode::FORBIDDEN,
                    "access_denied",
                    "an auth token is issued only within a mission",
                ))
            }
        };
        if !mission.resources.contains(&resource) {
            return Err(AAuthError::new(
                StatusCode::FORBIDDEN,
                "access_denied",
                "this mission does not cover that resource",
            ));
        }
        let jwks = self
            .resolver
            .jwks(&resource, dwk::RESOURCE)
            .await
            .map_err(|e| invalid(&e.to_string()))?;
        let alg = unverified
            .alg()
            .and_then(Algorithm::from_name)
            .ok_or_else(|| invalid("unsupported alg"))?;
        let key = match unverified.kid() {
            Some(kid) => jwks.keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
            None if jwks.keys.len() == 1 => jwks.keys.first(),
            None => None,
        }
        .ok_or_else(|| invalid("no matching key in the resource's JWKS"))?;
        if key.alg.as_deref().is_some_and(|a| a != alg.as_str())
            || !unverified.verify_signature(key, alg)
        {
            return Err(invalid("signature verification failed"));
        }
        let rt = unverified.claims;
        if rt
            .get("exp")
            .and_then(Value::as_i64)
            .is_none_or(|exp| self.now() >= exp)
        {
            return Err(invalid("resource token has expired"));
        }
        let not_before = rt.get("nbf").and_then(Value::as_i64);
        let issued_at = rt.get("iat").and_then(Value::as_i64);
        if not_before.is_some_and(|nbf| self.now() < nbf)
            || issued_at.is_some_and(|iat| iat > self.now() + 60)
        {
            return Err(invalid("resource token is not yet valid"));
        }
        if str_of(&rt, "aud") != Some(self.issuer.as_str())
            || trim(str_of(&rt, "ps").unwrap_or_default()) != self.issuer
        {
            return Err(invalid("not for this Person Server"));
        }
        if str_of(&rt, "agent_jkt") != Some(agent.jkt.as_str()) {
            return Err(invalid("agent_jkt is not the signer"));
        }
        let p = &presented.claims;
        if p.get("jti") != rt.get("presented_jti")
            || p.get("sub") != rt.get("sub")
            || p.get("mission_s256") != rt.get("mission_s256")
            || p.get("tenant") != rt.get("tenant")
            || str_of(p, "aud") != Some(resource.as_str())
        {
            return Err(invalid("does not match the presented token"));
        }
        Ok((rt, mission, resource))
    }

    /// The resource's one-line description of the request, from its R3
    /// proposal; `None` if it has none or it cannot be read.
    async fn proposal_summary(&self, rt: &Map<String, Value>) -> Option<String> {
        let (uri, expected) = (str_of(rt, "r3_uri")?, str_of(rt, "r3_s256")?);
        // Fetched signed as this server, so only from the resource itself.
        let resource = str_of(rt, "iss")?;
        if !uri.starts_with("https://") || !aauth_protocol::identifier::same_origin(resource, uri) {
            return None;
        }
        let signed = httpsig::sign(
            &self.key,
            &SignatureKey::JwksUri {
                id: self.issuer.clone(),
                dwk: dwk::PERSON.into(),
                kid: self.key.thumbprint().into(),
            },
            &httpsig::Outgoing {
                method: "GET",
                url: uri,
                content_type: None,
                body: None,
                created: self.now(),
            },
        )
        .ok()?;
        let mut headers = http::HeaderMap::new();
        for (name, value) in signed {
            headers.insert(
                http::HeaderName::from_bytes(name.as_bytes()).ok()?,
                HeaderValue::from_str(&value).ok()?,
            );
        }
        let fetched = self.fetch.get(uri, headers).await.ok()?;
        if !fetched.status.is_success() || aauth_protocol::s256(&fetched.body) != expected {
            return None;
        }
        let document: Value = serde_json::from_slice(&fetched.body).ok()?;
        document
            .get("display")?
            .get("summary")?
            .as_str()
            .map(str::to_string)
    }

    async fn request_auth_token(
        &self,
        request: &Request<Bytes>,
        path: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let agent = self.verify_agent(request, path).await?;
        let params = read_body(request.body())?;
        let (rt, mission, resource) = self.verify_resource_token(&agent, &params).await?;
        let tenant = mission.tenant.clone();
        let tenant = tenant.as_str();
        let budget = Budget::from_value(rt.get("budget")).map_err(|_| {
            AAuthError::new(
                StatusCode::BAD_REQUEST,
                "invalid_resource_token",
                "budget is not a budget object",
            )
        })?;
        let mut grant = Map::new();
        grant.insert("aud".into(), json!(resource));
        grant.insert("sub".into(), rt.get("sub").cloned().unwrap_or(Value::Null));
        for claim in GRANT_CLAIMS {
            if let Some(value) = rt.get(*claim) {
                grant.insert((*claim).into(), value.clone());
            }
        }
        let Some(budget) = budget else {
            return Ok(ok(&self.auth_token(&agent, &mission, &grant).await));
        };
        grant.insert("budget".into(), json!(budget));
        if mission
            .budget
            .as_ref()
            .is_some_and(|b| b.same_scale(&budget))
        {
            if let Some(spent) = self
                .store
                .spend(
                    tenant,
                    &mission.s256,
                    budget.amount,
                    Spend::WithinBudget,
                    self.now(),
                )
                .await?
            {
                return Ok(ok(&self.auth_token(&agent, &spent, &grant).await));
            }
        }
        let summary = self.proposal_summary(&rt).await;
        let description = str_of(&rt, "scope").unwrap_or("a request").to_string();
        let approval = self.approval(
            tenant,
            ApprovalKind::AuthToken,
            agent,
            mission.person.clone(),
            description,
        );
        self.ask(Approval {
            mission_s256: Some(mission.s256),
            resource: Some(resource),
            budget: Some(budget),
            grant,
            summary,
            ..approval
        })
        .await
    }

    fn approval(
        &self,
        tenant: &str,
        kind: ApprovalKind,
        agent: Agent,
        person: Person,
        description: String,
    ) -> Approval {
        let created = self.now();
        Approval {
            id: uuid::Uuid::now_v7().to_string(),
            kind,
            status: ApprovalStatus::Pending,
            agent,
            approver: person.clone(),
            person,
            description,
            summary: None,
            mission_s256: None,
            resource: None,
            budget: None,
            grant: Map::new(),
            decided_budget: None,
            decided_by: None,
            decided_at: None,
            created_at: created,
            expires_at: created + self.approval_ttl,
            tenant: tenant.to_string(),
        }
    }

    async fn ask(&self, mut approval: Approval) -> Result<Response<Bytes>, AAuthError> {
        if approval.kind != ApprovalKind::Completion {
            if let Some(approver) = self
                .directory
                .approver(approval.kind, &approval.person)
                .await
            {
                approval.approver = approver;
            }
        }
        let tenant = approval.tenant.as_str();
        self.store.insert_approval(tenant, &approval).await?;
        self.hooks.on_approval(&approval).await;
        Ok(self.still_pending(tenant, &approval.id))
    }

    fn still_pending(&self, tenant: &str, id: &str) -> Response<Bytes> {
        let mut response = json_response(StatusCode::ACCEPTED, &json!({ "status": "pending" }));
        let headers = response.headers_mut();
        for (name, value) in [
            ("location", self.endpoints.pending(tenant, id)),
            ("retry-after", self.retry_after.to_string()),
            ("cache-control", "no-store".to_string()),
            (
                aauth_protocol::requirement::HEADER,
                "requirement=approval".to_string(),
            ),
        ] {
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.insert(name, value);
            }
        }
        response
    }

    /// The agent polls with a signed GET; only the key that asked may read it.
    /// Results are minted now.
    async fn pending_state(
        &self,
        request: &Request<Bytes>,
        path: &str,
        tenant: &str,
        id: &str,
    ) -> Result<Response<Bytes>, AAuthError> {
        let agent = self.verify_agent(request, path).await?;
        let Some(approval) = self
            .store
            .approval(tenant, id)
            .await?
            .filter(|a| a.agent.jkt == agent.jkt)
        else {
            return Ok(problem(
                StatusCode::NOT_FOUND,
                "not_found",
                Some("no such pending request"),
            ));
        };
        if request.method() == Method::DELETE {
            if approval.status == ApprovalStatus::Pending {
                let withdrawn = Approval {
                    status: ApprovalStatus::Denied,
                    decided_by: Some("agent".into()),
                    decided_at: Some(self.now()),
                    ..approval
                };
                self.store
                    .transition_approval(tenant, ApprovalStatus::Pending, &withdrawn)
                    .await?;
            }
            let mut response = Response::new(Bytes::new());
            *response.status_mut() = StatusCode::NO_CONTENT;
            return Ok(response);
        }
        match approval.status {
            ApprovalStatus::Denied => {
                let who = approval.approver.name.as_deref().unwrap_or("The approver");
                return Ok(problem(
                    StatusCode::FORBIDDEN,
                    "denied",
                    Some(&format!("{who} declined")),
                ));
            }
            ApprovalStatus::Pending if self.now() >= approval.expires_at => {
                return Ok(problem(
                    StatusCode::REQUEST_TIMEOUT,
                    "expired",
                    Some("nobody answered in time"),
                ));
            }
            ApprovalStatus::Pending => return Ok(self.still_pending(tenant, id)),
            ApprovalStatus::Collected => {
                return Ok(problem(
                    StatusCode::GONE,
                    "collected",
                    Some("this approval's token was already collected"),
                ));
            }
            ApprovalStatus::Approved => {}
        }
        if approval.kind == ApprovalKind::Completion {
            return Ok(ok(&json!({})));
        }
        let mission = self
            .live_mission(tenant, &agent, approval.mission_s256.as_deref())
            .await?;
        if approval.kind == ApprovalKind::Mission {
            let mut tokens = Map::new();
            for resource in &mission.resources {
                let issued = self
                    .person_token(&agent, &mission.person, resource, Some(&mission))
                    .await;
                tokens.insert(resource.clone(), issued["person_token"].clone());
            }
            return Ok(ok(&json!({
                "s256": mission.s256,
                "mission": URL_SAFE_NO_PAD.encode(mission.mission.as_bytes()),
                "person_tokens": tokens,
            })));
        }
        let grant = approval.grant.clone();
        let asked = approval.budget.clone();
        let collected = Approval {
            status: ApprovalStatus::Collected,
            ..approval.clone()
        };
        // Collecting claims the token, so only one poll spends it.
        if !self
            .store
            .transition_approval(tenant, ApprovalStatus::Approved, &collected)
            .await?
        {
            return Ok(problem(
                StatusCode::GONE,
                "collected",
                Some("this approval's token was already collected"),
            ));
        }
        let mut mission = mission;
        // A request in another unit than the mission's budget is approved
        // without spending it.
        if let Some(asked) =
            asked.filter(|a| mission.budget.as_ref().is_some_and(|b| b.same_scale(a)))
        {
            let spent = self
                .store
                .spend(
                    tenant,
                    &mission.s256,
                    asked.amount,
                    Spend::Approved,
                    self.now(),
                )
                .await;
            match spent {
                Ok(Some(spent)) => mission = spent,
                // Unspent, nothing is issued, and the approval is collectible again.
                unspent => {
                    let _ = self
                        .store
                        .transition_approval(tenant, ApprovalStatus::Collected, &approval)
                        .await;
                    unspent?;
                    return Err(AAuthError::new(
                        StatusCode::FORBIDDEN,
                        "mission_terminated",
                        "the mission has ended",
                    ));
                }
            }
        }
        Ok(ok(&self.auth_token(&agent, &mission, &grant).await))
    }

    /// Record a decision on approval `id`, asked under `tenant`. Approving
    /// does what was asked: creates the mission, or completes it; an
    /// approved auth token request is spent when the agent collects its
    /// token. A decision is made once: a second, even a
    /// concurrent one, is a `409`.
    pub async fn decide(
        &self,
        tenant: &str,
        id: &str,
        decision: Decision,
    ) -> Result<Approval, AAuthError> {
        let Some(approval) = self.store.approval(tenant, id).await? else {
            return Err(AAuthError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "no such approval",
            ));
        };
        let already =
            || AAuthError::new(StatusCode::CONFLICT, "invalid_request", "already decided");
        if approval.status != ApprovalStatus::Pending {
            return Err(already());
        }
        let now = self.now();
        if now >= approval.expires_at {
            return Err(AAuthError::new(
                StatusCode::GONE,
                "expired",
                "this approval has expired",
            ));
        }
        if decision.budget.is_some() && approval.kind != ApprovalKind::Mission {
            return Err(AAuthError::invalid_request("only a mission takes a budget"));
        }
        let approved = decision.status == Verdict::Approved;
        let mut decided = Approval {
            status: if approved {
                ApprovalStatus::Approved
            } else {
                ApprovalStatus::Denied
            },
            decided_budget: decision.budget.clone(),
            decided_by: decision.decided_by.clone().or(approval.decided_by.clone()),
            decided_at: Some(now),
            ..approval.clone()
        };
        let mut new_mission = None;
        let mut existing = None;
        if approved && approval.kind == ApprovalKind::Mission {
            let mission = self.mission_from(&approval, decision.budget.clone(), now);
            decided.mission_s256 = Some(mission.s256.clone());
            new_mission = Some(mission);
        } else if approved {
            let mission = match &approval.mission_s256 {
                Some(s256) => self.store.mission(tenant, s256).await?,
                None => None,
            };
            existing = Some(mission.filter(|m| m.is_live(now)).ok_or_else(|| {
                AAuthError::new(
                    StatusCode::CONFLICT,
                    "mission_terminated",
                    "the mission has ended",
                )
            })?);
        }
        // The mission goes in first, so a decision never points at a mission
        // it did not create.
        if let Some(mission) = &new_mission {
            if !self.store.insert_mission(tenant, mission).await? {
                return Err(already());
            }
        }
        // The transition is the decision: whatever it does happens only once.
        if !self
            .store
            .transition_approval(tenant, ApprovalStatus::Pending, &decided)
            .await?
        {
            if let Some(mission) = &new_mission {
                self.store.end_mission(tenant, &mission.s256, now).await?;
            }
            return Err(already());
        }
        // An approved auth token request is spent when its token is
        // collected, so no token is out before its spend.
        if let Some(mission) = existing.filter(|_| approval.kind == ApprovalKind::Completion) {
            self.store.end_mission(tenant, &mission.s256, now).await?;
        }
        self.hooks.on_decision(&decided).await;
        Ok(decided)
    }

    /// The mission an approved proposal creates.
    fn mission_from(&self, approval: &Approval, budget: Option<Budget>, now: i64) -> Mission {
        let resources: Vec<String> = approval
            .grant
            .get("resources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
        let expires_at = now + self.mission_ttl;
        // The approval's id makes each mission's hash its own.
        let mut document = json!({
            "approval": approval.id,
            "agent": approval.agent.id,
            "approved_at": rfc3339(now),
            "expires_at": rfc3339(expires_at),
            "description": approval.description,
            "approved_resources": resources,
        });
        if let Some(budget) = &budget {
            document["budget"] = json!(budget);
        }
        let bytes = document.to_string();
        Mission {
            s256: aauth_protocol::s256(&bytes),
            mission: bytes,
            agent_id: approval.agent.id.clone(),
            person: approval.person.clone(),
            resources,
            budget,
            spent: 0,
            log: Vec::new(),
            expires_at,
            completed_at: None,
            tenant: approval.tenant.clone(),
        }
    }

    /// End mission `s256`, approved under `tenant`, now: nothing is issued
    /// under it afterwards.
    pub async fn terminate(&self, tenant: &str, s256: &str) -> Result<Mission, AAuthError> {
        self.store
            .end_mission(tenant, s256, self.now())
            .await?
            .ok_or_else(mission_not_found)
    }

    fn admitted(&self, request: &Request<Bytes>) -> Result<(), AAuthError> {
        let presented = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match (&self.admin_token, presented) {
            (Some(token), Some(presented)) if constant_time_eq(token, presented) => Ok(()),
            _ => Err(AAuthError::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "the management routes need the admin token",
            )),
        }
    }

    /// `GET /consent[?status=]`, `POST /consent/{id}/decision`,
    /// `GET /missions[/{s256}]`, `PATCH /missions/{s256}` (terminate); each
    /// takes `?tenant=` for a tenant's records.
    async fn manage(
        &self,
        request: &Request<Bytes>,
        segments: &[&str],
    ) -> Result<Option<Response<Bytes>>, AAuthError> {
        let method = request.method();
        let query = |name: &str| {
            request.uri().query().and_then(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.into_owned())
            })
        };
        let tenant = self.tenant(query("tenant").filter(|t| !t.is_empty()).as_deref())?;
        let tenant = tenant.as_str();
        let response = match segments {
            ["consent"] => {
                self.admitted(request)?;
                if method != Method::GET {
                    return Ok(Some(problem(
                        StatusCode::METHOD_NOT_ALLOWED,
                        "method_not_allowed",
                        None,
                    )));
                }
                let status = query("status");
                let approvals: Vec<Approval> = self
                    .store
                    .approvals(tenant)
                    .await?
                    .into_iter()
                    .filter(|a| {
                        status.as_deref().is_none_or(|s| {
                            serde_json::to_value(a.status)
                                .ok()
                                .and_then(|v| v.as_str().map(|v| v == s))
                                == Some(true)
                        })
                    })
                    .collect();
                ok(&approvals)
            }
            ["consent", id, "decision"] => {
                self.admitted(request)?;
                if method != Method::POST {
                    return Ok(Some(problem(
                        StatusCode::METHOD_NOT_ALLOWED,
                        "method_not_allowed",
                        None,
                    )));
                }
                let params = read_body(request.body())?;
                let verdict = str_of(&params, "decision").or_else(|| str_of(&params, "status"));
                let status = match verdict {
                    Some("approve" | "approved") => Verdict::Approved,
                    Some("deny" | "denied") => Verdict::Denied,
                    _ => return Err(AAuthError::invalid_request("decision is approve or deny")),
                };
                let budget = Budget::from_value(params.get("budget"))
                    .map_err(|e| AAuthError::invalid_request(e.to_string()))?;
                let decision = Decision {
                    status,
                    budget,
                    decided_by: str_of(&params, "decided_by").map(str::to_string),
                };
                ok(&self.decide(tenant, id, decision).await?)
            }
            ["missions"] => {
                self.admitted(request)?;
                if method != Method::GET {
                    return Ok(Some(problem(
                        StatusCode::METHOD_NOT_ALLOWED,
                        "method_not_allowed",
                        None,
                    )));
                }
                ok(&self.store.missions(tenant).await?)
            }
            ["missions", s256] => {
                self.admitted(request)?;
                match *method {
                    Method::GET => match self.store.mission(tenant, s256).await? {
                        Some(mission) => ok(&mission),
                        None => json_response(StatusCode::NOT_FOUND, &Value::Null),
                    },
                    Method::PATCH => ok(&self.terminate(tenant, s256).await?),
                    _ => problem(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", None),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(response))
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
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
        "without the reqwest feature, give the Person Server a Fetch".into(),
    ))
}
