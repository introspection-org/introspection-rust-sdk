//! Four-party AAuth (draft `-11` §Access Server Federation,
//! `#access-server-federation`), in process: a resource that names its own
//! Access Server, the person's Person Server federating with it, and an agent,
//! every party behind one in-memory network. The cases mirror
//! `tests/access-server/federation.test.ts` in introspection-js-sdk.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use aauth_access_server::{
    AccessDecision, AccessRequest, AccessServer, Config as AsConfig, PendingStatus, Verdict,
};
use aauth_person_server::{
    Approval, ApprovalKind, Config as PsConfig, Decision, Directory, Hooks, Person, PersonServer,
};
use aauth_protocol::discovery::{Fetch, Fetched, JwksResolver};
use aauth_protocol::federation::AuthTokenRequest;
use aauth_protocol::httpsig::{self, header_map, sign_jwt, Incoming, Outgoing, SignatureKey};
use aauth_protocol::jwt;
use aauth_protocol::resource::{
    authenticate, resource_token, AuthTokenIssuers, ResourceTokenParams,
};
use aauth_protocol::token::{mint, TokenKind};
use aauth_protocol::{Budget, Requirement, SigningKey};
use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use http::{HeaderMap, Method, Request, Response, StatusCode};
use serde_json::{json, Map, Value};

const PS: &str = "https://ps.acme.example";
const OTHER_PS: &str = "https://ps.other.example";
const AS: &str = "https://as.flightsector.example";
const RESOURCE: &str = "https://booking.flightsector.example";
const AP: &str = "https://agents.example";
const T0: i64 = 1_791_000_000;

fn usd(amount: u64) -> Budget {
    Budget::new(amount, "USD", 2)
}

/// Every party, by origin: documents, and the Access Server's routes.
#[derive(Default)]
struct Net {
    documents: Mutex<HashMap<String, Bytes>>,
    access: OnceLock<Arc<AccessServer>>,
    calls: Mutex<Vec<String>>,
}

impl Net {
    fn serve(&self, url: impl Into<String>, body: impl Into<Bytes>) {
        self.documents
            .lock()
            .unwrap()
            .insert(url.into(), body.into());
    }

    fn json(&self, url: impl Into<String>, document: Value) {
        self.serve(url, document.to_string());
    }

    fn calls_to(&self, origin: &str) -> Vec<String> {
        let calls = self.calls.lock().unwrap();
        calls
            .iter()
            .filter(|c| c.contains(origin))
            .cloned()
            .collect()
    }

    async fn route(&self, request: Request<Bytes>) -> Fetched {
        let url = request.uri().to_string();
        self.calls
            .lock()
            .unwrap()
            .push(format!("{} {url}", request.method()));
        let response = match url.strip_prefix(AS) {
            Some(path) => {
                let (mut parts, body) = request.into_parts();
                parts.uri = path.parse().unwrap();
                let access = self.access.get().expect("the Access Server is up");
                Some(access.handle(Request::from_parts(parts, body)).await)
            }
            None => None,
        };
        match response {
            Some(response) => Fetched {
                status: response.status(),
                headers: response.headers().clone(),
                body: response.into_body(),
            },
            None => {
                let body = self.documents.lock().unwrap().get(&url).cloned();
                Fetched {
                    status: if body.is_some() {
                        StatusCode::OK
                    } else {
                        StatusCode::NOT_FOUND
                    },
                    headers: HeaderMap::new(),
                    body: body.unwrap_or_default(),
                }
            }
        }
    }
}

#[async_trait]
impl Fetch for Net {
    async fn get(&self, url: &str, headers: HeaderMap) -> aauth_protocol::Result<Fetched> {
        let mut request = Request::get(url).body(Bytes::new()).unwrap();
        *request.headers_mut() = headers;
        Ok(self.route(request).await)
    }

    async fn send(&self, request: Request<Bytes>) -> aauth_protocol::Result<Fetched> {
        Ok(self.route(request).await)
    }
}

/// A shared handle, so the network can be every party's `Fetch`.
struct Shared(Arc<Net>);

#[async_trait]
impl Fetch for Shared {
    async fn get(&self, url: &str, headers: HeaderMap) -> aauth_protocol::Result<Fetched> {
        self.0.get(url, headers).await
    }

    async fn send(&self, request: Request<Bytes>) -> aauth_protocol::Result<Fetched> {
        self.0.send(request).await
    }
}

struct People;

#[async_trait]
impl Directory for People {
    async fn person(&self, hint: &str, _tenant: &str) -> Option<Person> {
        (hint == "user:sam@acme.example").then(|| Person {
            name: Some("Sam".into()),
            tenant: Some("acme".into()),
            ..Person::new("sam")
        })
    }

    async fn approver(&self, _kind: ApprovalKind, _person: &Person) -> Option<Person> {
        Some(Person {
            name: Some("Dana".into()),
            ..Person::new("dana")
        })
    }
}

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<Approval>>>);

#[async_trait]
impl Hooks for Seen {
    async fn on_approval(&self, approval: &Approval) {
        self.0.lock().unwrap().push(approval.clone());
    }
}

type Rule = Arc<Mutex<Box<dyn Fn(&AccessRequest) -> Verdict + Send>>>;

struct World {
    net: Arc<Net>,
    ps: PersonServer,
    access: Arc<AccessServer>,
    clock: Arc<AtomicI64>,
    seen: Seen,
    policy: Rule,
    decided: Arc<Mutex<Vec<AccessRequest>>>,
    ap: SigningKey,
    ps_seed: [u8; 32],
    as_seed: [u8; 32],
    resource_key: SigningKey,
    other_ps: SigningKey,
}

struct Agent {
    key: SigningKey,
    token: String,
}

impl World {
    fn new() -> Self {
        let net = Arc::new(Net::default());
        let (ap, resource_key, other_ps) = (
            SigningKey::generate(),
            SigningKey::generate(),
            SigningKey::generate(),
        );
        // Seeds this test keeps, so it can sign as the Person Server or Access Server too.
        let (ps_seed, as_seed) = ([7u8; 32], [9u8; 32]);
        let ps_key = SigningKey::from_bytes(&ps_seed);
        net.json(
            format!("{AP}/.well-known/aauth-agent.json"),
            json!({ "issuer": AP, "jwks": ap.jwks() }),
        );
        net.json(
            format!("{RESOURCE}/.well-known/aauth-resource.json"),
            json!({ "issuer": RESOURCE, "jwks": resource_key.jwks() }),
        );
        net.json(
            format!("{PS}/.well-known/aauth-person.json"),
            json!({ "issuer": PS, "jwks_uri": format!("{PS}/.well-known/jwks.json") }),
        );
        net.json(format!("{PS}/.well-known/jwks.json"), json!(ps_key.jwks()));
        net.json(
            format!("{OTHER_PS}/.well-known/aauth-person.json"),
            json!({ "issuer": OTHER_PS, "jwks": other_ps.jwks() }),
        );

        let clock = Arc::new(AtomicI64::new(T0));
        let seen = Seen::default();
        let now = clock.clone();
        let ps = PersonServer::new(
            PsConfig::new(PS, People, [AP])
                .default_tenant("acme")
                .signing_key(ps_key)
                .subject_secret("test-secret")
                .hooks(seen.clone())
                .fetch(Arc::new(Shared(net.clone())))
                .clock(move || now.load(Ordering::SeqCst)),
        )
        .unwrap();

        let policy: Rule = Arc::new(Mutex::new(Box::new(
            |request: &AccessRequest| match request.tenant.as_deref() {
                Some("acme") => Verdict::allow(),
                _ => Verdict::deny("no Flight Sector account"),
            },
        )));
        let decided = Arc::new(Mutex::new(Vec::new()));
        let (rule, log) = (policy.clone(), decided.clone());
        let now = clock.clone();
        let access = Arc::new(
            AccessServer::new(
                AsConfig::new(AS, move |request: &AccessRequest| {
                    log.lock().unwrap().push(request.clone());
                    (rule.lock().unwrap())(request)
                })
                .resources([RESOURCE])
                .person_servers([PS, OTHER_PS])
                .agent_providers([AP])
                .signing_key(SigningKey::from_bytes(&as_seed))
                .fetch(Arc::new(Shared(net.clone())))
                .clock(move || now.load(Ordering::SeqCst)),
            )
            .unwrap(),
        );
        assert!(net.access.set(access.clone()).is_ok());
        Self {
            net,
            ps,
            access,
            clock,
            seen,
            policy,
            decided,
            ap,
            ps_seed,
            as_seed,
            resource_key,
            other_ps,
        }
    }

    fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst)
    }

    fn set_policy(&self, rule: impl Fn(&AccessRequest) -> Verdict + Send + 'static) {
        *self.policy.lock().unwrap() = Box::new(rule);
    }

    fn agent(&self) -> Agent {
        let key = SigningKey::generate();
        let token = mint(
            &self.ap,
            TokenKind::Agent,
            &json!({
                "iss": AP, "dwk": "aauth-agent.json", "sub": "aauth:travel@agents.example",
                "ps": PS, "iat": T0, "exp": T0 + 7 * 86_400, "cnf": { "jwk": key.public_jwk() },
            }),
        );
        Agent { key, token }
    }

    /// A request signed by `agent`, presenting `token`.
    fn signed(
        &self,
        agent: &Agent,
        token: &str,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Request<Bytes> {
        let bytes = body.map(|b| serde_json::to_vec(b).unwrap());
        let headers = sign_jwt(
            &agent.key,
            token,
            &Outgoing {
                method: method.as_str(),
                url,
                content_type: None,
                body: bytes.as_deref(),
                created: self.now(),
            },
        )
        .unwrap();
        let path = &url[url.find(".example").unwrap() + ".example".len()..];
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        request
            .body(Bytes::from(bytes.unwrap_or_default()))
            .unwrap()
    }

    /// An agent's call to its Person Server.
    async fn to_ps(
        &self,
        agent: &Agent,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value, Response<Bytes>) {
        let response = self
            .ps
            .handle(self.signed(agent, &agent.token, method, url, body.as_ref()))
            .await;
        let json = serde_json::from_slice(response.body()).unwrap_or(Value::Null);
        (response.status(), json, response)
    }

    /// A request to the Access Server's token endpoint signed as Person Server
    /// `id` with `key`, as `#ps-to-as-token-request` has it.
    async fn as_person_server(
        &self,
        id: &str,
        key: &SigningKey,
        body: &Value,
    ) -> (StatusCode, Value) {
        let bytes = serde_json::to_vec(body).unwrap();
        let headers = httpsig::sign(
            key,
            &SignatureKey::JwksUri {
                id: id.into(),
                dwk: "aauth-person.json".into(),
                kid: key.thumbprint().into(),
            },
            &Outgoing {
                method: "POST",
                url: &format!("{AS}/token"),
                content_type: None,
                body: Some(&bytes),
                created: self.now(),
            },
        )
        .unwrap();
        let mut request = Request::post("/token");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = self
            .access
            .handle(request.body(Bytes::from(bytes)).unwrap())
            .await;
        let json = serde_json::from_slice(response.body()).unwrap_or(Value::Null);
        (response.status(), json)
    }

    /// The booking resource's one paid call: it books with an auth token its
    /// Access Server issued, and challenges a person token with a resource
    /// token addressed to that Access Server.
    async fn reserve(&self, agent: &Agent, token: &str, amount: u64) -> Response<Bytes> {
        let url = format!("{RESOURCE}/reserve");
        let body = json!({ "amount": amount });
        let request = self.signed(agent, token, Method::POST, &url, Some(&body));
        let resolver = JwksResolver::new(Arc::new(Shared(self.net.clone())));
        let authenticated = authenticate(
            &Incoming {
                method: "POST",
                authority: "booking.flightsector.example",
                path: "/reserve",
                query: None,
                headers: request.headers(),
                body: Some(request.body()),
            },
            RESOURCE,
            &[TokenKind::Person, TokenKind::Auth],
            &httpsig::VerifyOptions {
                now: self.now(),
                ..Default::default()
            },
            &resolver,
        )
        .await;
        let person_servers = [PS.to_string()];
        let issuers = AuthTokenIssuers::AccessServer {
            access_server: AS,
            person_servers: &person_servers,
        };
        let refused = |code: &str| {
            let mut response = Response::new(Bytes::new());
            *response.status_mut() = StatusCode::UNAUTHORIZED;
            response
                .headers_mut()
                .insert("signature-error", format!("error={code}").parse().unwrap());
            response
        };
        let authenticated = match authenticated {
            Ok(a) => a,
            Err(aauth_protocol::Error::Signature(e)) => return refused(e.code),
            Err(aauth_protocol::Error::Token(e)) => return refused(e.code),
            Err(e) => panic!("{e}"),
        };
        if authenticated.token.kind == TokenKind::Auth {
            if let Err(e) = issuers.check(&authenticated.token) {
                return refused(e.code);
            }
            let mut response = Response::new(Bytes::from(
                json!({ "booked": true, "ps": authenticated.token.ps() }).to_string(),
            ));
            *response.status_mut() = StatusCode::OK;
            return response;
        }
        let proposal =
            json!({ "display": { "summary": format!("QT Sydney for ${}", amount / 100) } })
                .to_string();
        let r3_uri = format!("{RESOURCE}/r3/{amount}");
        let r3_s256 = aauth_protocol::s256(&proposal);
        self.net.serve(r3_uri.clone(), proposal);
        let budget = usd(amount);
        let rt = resource_token(
            &self.resource_key,
            &ResourceTokenParams {
                now: self.now(),
                ..ResourceTokenParams::new(
                    RESOURCE,
                    &authenticated.token,
                    &authenticated.signature.thumbprint,
                    "booking.reserve",
                )
                .budget(&budget)
                .r3(&r3_uri, &r3_s256)
                .access_server(AS)
            },
        )
        .unwrap();
        let mut response = Response::new(Bytes::new());
        *response.status_mut() = StatusCode::UNAUTHORIZED;
        response.headers_mut().insert(
            "aauth-requirement",
            Requirement::AuthToken { resource_token: rt }
                .to_header()
                .parse()
                .unwrap(),
        );
        response
    }

    /// Call the resource with `token`; the resource token it challenges with.
    async fn challenge(&self, agent: &Agent, token: &str, amount: u64) -> String {
        let response = self.reserve(agent, token, amount).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let header = response.headers()["aauth-requirement"].to_str().unwrap();
        match Requirement::parse(header) {
            Some(Requirement::AuthToken { resource_token }) => resource_token,
            other => panic!("{other:?}"),
        }
    }

    /// A mission over `resources` with a budget of `amount`, approved; its
    /// person token for the booking resource.
    async fn approved_trip(
        &self,
        agent: &Agent,
        amount: u64,
        resources: &[&str],
    ) -> (String, String) {
        let (status, _, response) = self
            .to_ps(
                agent,
                Method::POST,
                &format!("{PS}/mission"),
                Some(json!({ "description": "Sydney", "resources": resources, "login_hint": "user:sam@acme.example" })),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let id = self.seen.0.lock().unwrap().last().unwrap().id.clone();
        self.ps
            .decide("acme", &id, Decision::approve().with_budget(usd(amount)))
            .await
            .unwrap();
        let (status, trip, _) = self
            .to_ps(agent, Method::GET, &location(&response), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{trip}");
        let s256 = trip["s256"].as_str().unwrap().to_string();
        let person_token = match trip["person_tokens"][RESOURCE].as_str() {
            Some(token) => token.to_string(),
            None => {
                let (status, body, _) = self
                    .to_ps(
                        agent,
                        Method::POST,
                        &format!("{PS}/person-token"),
                        Some(json!({ "resource": RESOURCE, "mission_s256": s256 })),
                    )
                    .await;
                assert_eq!(status, StatusCode::OK, "{body}");
                body["person_token"].as_str().unwrap().to_string()
            }
        };
        (s256, person_token)
    }

    async fn spent(&self, s256: &str) -> u64 {
        self.ps
            .store()
            .mission("acme", s256)
            .await
            .unwrap()
            .unwrap()
            .spent
    }

    async fn redeem(
        &self,
        agent: &Agent,
        resource_token: &str,
        person_token: &str,
    ) -> (StatusCode, Value, Response<Bytes>) {
        self.to_ps(
            agent,
            Method::POST,
            &format!("{PS}/token"),
            Some(json!({ "resource_token": resource_token, "presented_token": person_token })),
        )
        .await
    }
}

fn location(response: &Response<Bytes>) -> String {
    response.headers()["location"].to_str().unwrap().to_string()
}

fn claims(token: &str) -> Map<String, Value> {
    jwt::decode(token).unwrap().claims
}

#[tokio::test]
async fn it_publishes_its_metadata_and_keys() {
    let world = World::new();
    let response = world
        .access
        .handle(
            Request::get("/.well-known/aauth-access.json")
                .body(Bytes::new())
                .unwrap(),
        )
        .await;
    let metadata: Value = serde_json::from_slice(response.body()).unwrap();
    // #access-server-metadata: issuer, jwks_uri and auth_token_endpoint are REQUIRED.
    assert_eq!(
        metadata,
        json!({
            "issuer": AS,
            "jwks_uri": format!("{AS}/.well-known/jwks.json"),
            "auth_token_endpoint": format!("{AS}/token"),
        })
    );
    assert_eq!(world.access.jwks().keys.len(), 1);
}

#[tokio::test]
async fn the_person_server_redeems_it_and_its_auth_token_admits_the_call() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    let rt = world.challenge(&agent, &person_token, 165_000).await;
    // #resource-token: aud is the AS in four-party, ps the person's Person Server.
    assert_eq!(claims(&rt)["aud"], AS);
    assert_eq!(claims(&rt)["ps"], PS);

    let (status, body, _) = world.redeem(&agent, &rt, &person_token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let auth_token = body["auth_token"].as_str().unwrap();
    let issued = claims(auth_token);
    // #auth-token-structure, as issued by an AS.
    assert_eq!(jwt::decode(auth_token).unwrap().typ(), Some("aa-auth+jwt"));
    assert_eq!(issued["iss"], AS);
    assert_eq!(issued["dwk"], "aauth-access.json");
    assert_eq!(issued["aud"], RESOURCE);
    assert_eq!(issued["ps"], PS);
    assert_eq!(issued["sub"], claims(&rt)["sub"]);
    assert_eq!(issued["mission_s256"], s256.as_str());
    assert_eq!(issued["scope"], "booking.reserve");
    assert_eq!(issued["cnf"]["jwk"]["x"], json!(agent.key.public_jwk().x));
    let lifetime = issued["exp"].as_i64().unwrap() - issued["iat"].as_i64().unwrap();
    assert!(lifetime <= 3600);
    assert!(issued["exp"].as_i64().unwrap() <= claims(&person_token)["exp"].as_i64().unwrap());

    let decided = world.decided.lock().unwrap()[0].clone();
    assert_eq!(decided.resource, RESOURCE);
    assert_eq!(decided.ps, PS);
    assert_eq!(decided.tenant.as_deref(), Some("acme"));
    assert_eq!(decided.mission_s256.as_deref(), Some(s256.as_str()));
    assert_eq!(decided.budget, Some(usd(165_000)));
    assert_eq!(decided.summary.as_deref(), Some("QT Sydney for $1650"));
    assert_eq!(decided.agent.provider, AP);
    assert_eq!(decided.agent.jkt, agent.key.thumbprint());
    assert_eq!(world.spent(&s256).await, 165_000);

    let booked = world.reserve(&agent, auth_token, 165_000).await;
    assert_eq!(booked.status(), StatusCode::OK);
    let booked: Value = serde_json::from_slice(booked.body()).unwrap();
    assert_eq!(booked, json!({ "booked": true, "ps": PS }));
}

#[tokio::test]
async fn a_refusal_is_relayed_and_nothing_is_spent() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    world.set_policy(|_| Verdict::deny("no Flight Sector account"));
    let rt = world.challenge(&agent, &person_token, 100_000).await;
    let (status, body, _) = world.redeem(&agent, &rt, &person_token).await;
    // #auth-token-delivery: the AS's error and status, in the PS's problem+json.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "denied");
    assert!(body["detail"]
        .as_str()
        .unwrap()
        .contains("no Flight Sector account"));
    assert_eq!(world.spent(&s256).await, 0);
}

#[tokio::test]
async fn it_refuses_a_request_not_signed_by_the_person_server_the_resource_token_names() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    let tokens = json!(AuthTokenRequest::new(
        world.challenge(&agent, &person_token, 100_000).await,
        agent.token.clone(),
        person_token,
    ));
    // #resource-token-verification: an AS MUST verify that ps names the PS that sent it.
    let (status, body) = world
        .as_person_server(OTHER_PS, &world.other_ps, &tokens)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_resource_token");
    let (status, _) = world
        .as_person_server("https://ps.stranger.example", &world.other_ps, &tokens)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(world.net.calls_to("ps.stranger.example").is_empty());
    assert!(world.decided.lock().unwrap().is_empty());
}

#[tokio::test]
async fn it_refuses_a_resource_token_for_another_audience() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    let mut rt = claims(&world.challenge(&agent, &person_token, 100_000).await);
    rt.insert("aud".into(), json!("https://as.elsewhere.example"));
    let elsewhere = mint(&world.resource_key, TokenKind::Resource, &Value::Object(rt));
    let ps_key = SigningKey::from_bytes(&world.ps_seed);
    let (status, body) = world
        .as_person_server(
            PS,
            &ps_key,
            &json!(AuthTokenRequest::new(
                elsewhere,
                agent.token.clone(),
                person_token
            )),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_resource_token");
    assert!(body["detail"].as_str().unwrap().contains("aud"));
}

#[tokio::test]
async fn the_person_server_calls_only_the_access_server_a_verified_token_names() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    let rt = world.challenge(&agent, &person_token, 100_000).await;
    // Point the resource token at another server without the resource's signature.
    let mut parts: Vec<String> = rt.split('.').map(str::to_string).collect();
    let mut tampered = claims(&rt);
    tampered.insert("aud".into(), json!("https://evil.example"));
    parts[1] = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Value::Object(tampered).to_string());
    let (status, body, _) = world.redeem(&agent, &parts.join("."), &person_token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_resource_token");
    assert!(world.net.calls_to("evil.example").is_empty());
    assert!(world.net.calls_to(AS).is_empty());
}

#[tokio::test]
async fn it_does_not_federate_for_a_resource_the_mission_does_not_cover() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world
        .approved_trip(&agent, 300_000, &["https://hotels.example"])
        .await;
    let rt = world.challenge(&agent, &person_token, 100_000).await;
    world.net.calls.lock().unwrap().clear();
    let (status, _, _) = world.redeem(&agent, &rt, &person_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(world.net.calls_to(AS).is_empty());
}

#[tokio::test]
async fn it_waits_on_its_own_approval_then_hands_the_auth_token_out_once() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    world.set_policy(|_| Verdict::Approval);
    let rt = world.challenge(&agent, &person_token, 120_000).await;
    let (status, _, waiting) = world.redeem(&agent, &rt, &person_token).await;
    // #approval-pending, relayed: the agent polls its own Person Server.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        waiting.headers()["aauth-requirement"],
        "requirement=approval"
    );
    let pending = location(&waiting);
    assert!(pending.starts_with(&format!("{PS}/pending/")));
    let (status, _, _) = world.to_ps(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(world.spent(&s256).await, 0);

    let at_as = world.access.pendings().pop().unwrap();
    assert_eq!(at_as.status, PendingStatus::Pending);
    assert_eq!(at_as.requirement, Requirement::Approval);
    world
        .access
        .decide(&at_as.id, AccessDecision::approve().by("ops"))
        .unwrap();
    let (status, body, _) = world.to_ps(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let issued = claims(body["auth_token"].as_str().unwrap());
    assert_eq!(issued["iss"], AS);
    assert_eq!(issued["aud"], RESOURCE);
    assert_eq!(world.spent(&s256).await, 120_000);
    let (status, _, _) = world.to_ps(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::GONE);
}

#[tokio::test]
async fn its_interaction_is_relayed_to_the_agent_and_so_is_its_denial() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    world.set_policy(|_| Verdict::Interaction {
        url: format!("{AS}/bind"),
        code: "ABCD-1234".into(),
    });
    let rt = world.challenge(&agent, &person_token, 120_000).await;
    let (status, _, waiting) = world.redeem(&agent, &rt, &person_token).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let header = waiting.headers()["aauth-requirement"].to_str().unwrap();
    assert_eq!(
        Requirement::parse(header),
        Some(Requirement::Interaction {
            url: Some(format!("{AS}/bind")),
            code: "ABCD-1234".into(),
        })
    );
    let id = world.access.pendings().pop().unwrap().id;
    world.access.decide(&id, AccessDecision::deny()).unwrap();
    let pending = location(&waiting);
    let (status, body, _) = world.to_ps(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body, _) = world.to_ps(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["detail"].as_str().unwrap().contains("Access Server"));
    assert_eq!(world.spent(&s256).await, 0);
}

#[tokio::test]
async fn a_call_over_what_is_left_goes_to_the_approver_before_the_access_server() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = world.approved_trip(&agent, 100_000, &[RESOURCE]).await;
    let rt = world.challenge(&agent, &person_token, 150_000).await;
    world.net.calls.lock().unwrap().clear();
    let (status, _, waiting) = world.redeem(&agent, &rt, &person_token).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(world.net.calls_to(AS).is_empty());
    let asked = world.seen.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked.kind, ApprovalKind::AuthToken);
    assert_eq!(asked.summary.as_deref(), Some("QT Sydney for $1500"));
    let federation = asked.federation.clone().unwrap();
    assert_eq!(federation.access_server, AS);
    assert!(!federation.within_budget);
    world
        .ps
        .decide("acme", &asked.id, Decision::approve())
        .await
        .unwrap();
    let (status, body, _) = world
        .to_ps(&agent, Method::GET, &location(&waiting), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(world.spent(&s256).await, 150_000);
}

#[tokio::test]
async fn a_resource_with_an_access_server_refuses_an_auth_token_its_person_server_minted() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    let rt = claims(&world.challenge(&agent, &person_token, 100_000).await);
    let minted = mint(
        &SigningKey::from_bytes(&world.ps_seed),
        TokenKind::Auth,
        &json!({
            "iss": PS, "dwk": "aauth-person.json", "jti": "minted-by-ps", "aud": RESOURCE,
            "ps": PS, "sub": rt["sub"], "mission_s256": rt["mission_s256"],
            "scope": "booking.reserve", "iat": world.now(), "exp": world.now() + 600,
            "cnf": { "jwk": agent.key.public_jwk() },
        }),
    );
    let response = world.reserve(&agent, &minted, 100_000).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["signature-error"], "error=invalid_jwt");
}

#[tokio::test]
async fn a_resource_refuses_an_auth_token_for_another_person_servers_people() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    let forged = mint(
        &SigningKey::from_bytes(&world.as_seed),
        TokenKind::Auth,
        &json!({
            "iss": AS, "dwk": "aauth-access.json", "jti": "forged", "aud": RESOURCE,
            "ps": OTHER_PS, "sub": claims(&person_token)["sub"], "scope": "booking.reserve",
            "iat": world.now(), "exp": world.now() + 600,
            "cnf": { "jwk": agent.key.public_jwk() },
        }),
    );
    let response = world.reserve(&agent, &forged, 1).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_pending_request_answers_408_once_expired_and_410_once_answered() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = world.approved_trip(&agent, 300_000, &[RESOURCE]).await;
    world.set_policy(|_| Verdict::Approval);
    let rt = world.challenge(&agent, &person_token, 120_000).await;
    let ps_key = SigningKey::from_bytes(&world.ps_seed);
    let tokens = json!(AuthTokenRequest::new(rt, agent.token.clone(), person_token));
    let (status, _) = world.as_person_server(PS, &ps_key, &tokens).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let pending = world.access.pendings().pop().unwrap();
    // The person token bounds how long the request may wait.
    world.clock.store(pending.expires_at, Ordering::SeqCst);
    let poll = |key: &SigningKey| {
        let headers = httpsig::sign(
            key,
            &SignatureKey::JwksUri {
                id: PS.into(),
                dwk: "aauth-person.json".into(),
                kid: key.thumbprint().into(),
            },
            &Outgoing {
                method: "GET",
                url: &world.access.endpoints().pending(&pending.id),
                content_type: None,
                body: None,
                created: world.now(),
            },
        )
        .unwrap();
        let mut request = Request::get(format!("/pending/{}", pending.id))
            .body(Bytes::new())
            .unwrap();
        *request.headers_mut() = header_map(&headers).unwrap();
        request
    };
    // #polling-error-codes: expired is 408; #deferred-responses: 410 is terminal.
    assert_eq!(
        world.access.handle(poll(&ps_key)).await.status(),
        StatusCode::REQUEST_TIMEOUT
    );
    assert_eq!(
        world.access.handle(poll(&ps_key)).await.status(),
        StatusCode::GONE
    );
}
