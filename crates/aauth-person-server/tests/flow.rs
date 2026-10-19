//! The whole conversation, in process: an agent provider, this Person Server,
//! and a resource that verifies what the Person Server issues with the `aauth`
//! crate alone.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use aauth_person_server::{
    Approval, ApprovalKind, ApprovalStatus, Config, Decision, Directory, Hooks, Person,
    PersonServer,
};
use aauth_protocol::discovery::{JwksResolver, StaticFetch};
use aauth_protocol::httpsig::{sign_jwt, Outgoing};
use aauth_protocol::mission::ApprovedMission;
use aauth_protocol::resource::{resource_token, ResourceTokenParams};
use aauth_protocol::token::{mint, verify_token, TokenKind, VerifiedToken, VerifyTokenOptions};
use aauth_protocol::{Budget, SigningKey};
use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use serde_json::{json, Value};

const PS: &str = "https://ps.acme.example";
const AP: &str = "https://agents.example";
const BOOKING: &str = "https://api.booking.example";
const BANK: &str = "https://bank.example";
const T0: i64 = 1_791_000_000;

struct People;

#[async_trait]
impl Directory for People {
    async fn person(&self, hint: &str, _tenant: Option<&str>) -> Option<Person> {
        let (id, name) = match hint {
            "user:sam@acme.example" => ("sam", "Sam"),
            "user:dana@acme.example" => ("dana", "Dana"),
            _ => return None,
        };
        Some(Person {
            name: Some(name.into()),
            ..Person::new(id)
        })
    }

    async fn approver(&self, _kind: ApprovalKind, _person: &Person) -> Option<Person> {
        self.person("user:dana@acme.example", None).await
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

struct World {
    ps: PersonServer,
    clock: Arc<AtomicI64>,
    seen: Seen,
    ap: SigningKey,
    resource_key: SigningKey,
    fetch: StaticFetch,
}

struct TestAgent {
    key: SigningKey,
    token: String,
}

impl World {
    fn new() -> Self {
        let ap = SigningKey::generate();
        let resource_key = SigningKey::generate();
        let ps_key = SigningKey::generate();
        let fetch = StaticFetch::default();
        fetch.insert(
            format!("{AP}/.well-known/aauth-agent.json"),
            json!({ "issuer": AP, "jwks_uri": format!("{AP}/jwks.json") }),
        );
        fetch.insert(
            format!("{AP}/jwks.json"),
            serde_json::to_value(ap.jwks()).unwrap(),
        );
        for resource in [BOOKING, BANK] {
            fetch.insert(
                format!("{resource}/.well-known/aauth-resource.json"),
                json!({ "issuer": resource, "jwks": resource_key.jwks() }),
            );
        }
        // How a resource finds the Person Server's keys.
        fetch.insert(
            format!("{PS}/.well-known/aauth-person.json"),
            json!({ "issuer": PS, "jwks_uri": format!("{PS}/.well-known/jwks.json") }),
        );
        fetch.insert(
            format!("{PS}/.well-known/jwks.json"),
            serde_json::to_value(ps_key.jwks()).unwrap(),
        );

        let clock = Arc::new(AtomicI64::new(T0));
        let seen = Seen::default();
        let now = clock.clone();
        let ps = PersonServer::new(
            Config::new(PS, People, [AP])
                .signing_key(ps_key)
                .subject_secret("test-secret")
                .admin_token("admin")
                .hooks(seen.clone())
                .fetch(Arc::new(fetch.clone()))
                .clock(move || now.load(Ordering::SeqCst)),
        )
        .unwrap();
        Self {
            ps,
            clock,
            seen,
            ap,
            resource_key,
            fetch,
        }
    }

    fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst)
    }

    fn agent(&self) -> TestAgent {
        let key = SigningKey::generate();
        let token = mint(
            &self.ap,
            TokenKind::Agent,
            &json!({
                "iss": AP, "dwk": "aauth-agent.json", "sub": "aauth:travel@agents.example",
                "iat": T0, "exp": T0 + 7 * 86_400, "cnf": { "jwk": key.public_jwk() },
            }),
        );
        TestAgent { key, token }
    }

    fn signed(
        &self,
        agent: &TestAgent,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Request<Bytes> {
        let bytes = body.map(|b| serde_json::to_vec(b).unwrap());
        let headers = sign_jwt(
            &agent.key,
            &agent.token,
            &Outgoing {
                method: method.as_str(),
                url,
                content_type: None,
                body: bytes.as_deref(),
                created: self.now(),
            },
        )
        .unwrap();
        let path = url.strip_prefix(PS).unwrap();
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        request
            .body(Bytes::from(bytes.unwrap_or_default()))
            .unwrap()
    }

    async fn call(
        &self,
        agent: &TestAgent,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value, Response<Bytes>) {
        let response = self
            .ps
            .handle(self.signed(agent, method, url, body.as_ref()))
            .await;
        let json = serde_json::from_slice(response.body()).unwrap_or(Value::Null);
        (response.status(), json, response)
    }

    async fn as_resource(
        &self,
        resource: &str,
        token: &str,
        agent: &TestAgent,
        kind: TokenKind,
    ) -> VerifiedToken {
        verify_token(
            &VerifyTokenOptions {
                jwt: token,
                http_signature_thumbprint: agent.key.thumbprint(),
                resource,
                accept: &[kind],
                now: self.now(),
                clock_tolerance: 60,
            },
            &JwksResolver::new(Arc::new(self.fetch.clone())),
        )
        .await
        .unwrap()
    }

    fn charge(
        &self,
        resource: &str,
        presented: &VerifiedToken,
        agent: &TestAgent,
        amount: u64,
    ) -> String {
        resource_token(
            &self.resource_key,
            &ResourceTokenParams {
                now: self.now(),
                ..ResourceTokenParams::new(
                    resource,
                    presented,
                    agent.key.thumbprint(),
                    "booking.reserve",
                )
                .budget(&usd(amount))
            },
        )
        .unwrap()
    }

    fn last_approval(&self) -> Approval {
        self.seen.0.lock().unwrap().last().cloned().unwrap()
    }
}

fn usd(amount: u64) -> Budget {
    Budget {
        amount,
        unit: "USD".into(),
        decimals: 2,
    }
}

fn location(response: &Response<Bytes>) -> String {
    response.headers()["location"].to_str().unwrap().to_string()
}

/// A mission for Sam at `BOOKING`, approved by Dana with a $3,000 budget.
async fn approved_mission(world: &World, agent: &TestAgent) -> (String, String) {
    let (status, _, response) = world
        .call(
            agent,
            Method::POST,
            &format!("{PS}/mission"),
            Some(json!({ "description": "Sydney, 19-23 October", "resources": [BOOKING], "login_hint": "user:sam@acme.example" })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let pending = location(&response);
    world
        .ps
        .decide(
            &world.last_approval().id,
            Decision::approve().with_budget(usd(300_000)).by("dana"),
        )
        .await
        .unwrap();
    let (status, body, _) = world.call(agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mission = ApprovedMission::from_approval(&body).expect("the mission matches its hash");
    let person_token = body["person_tokens"][BOOKING].as_str().unwrap().to_string();
    (mission.s256, person_token)
}

#[tokio::test]
async fn metadata_names_this_servers_endpoints() {
    let world = World::new();
    let response = world
        .ps
        .handle(
            Request::get("/.well-known/aauth-person.json")
                .body(Bytes::new())
                .unwrap(),
        )
        .await;
    let metadata: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(metadata["issuer"], PS);
    assert_eq!(metadata["auth_token_endpoint"], format!("{PS}/token"));
    assert_eq!(metadata["mission_endpoint"], format!("{PS}/mission"));
}

#[tokio::test]
async fn a_mission_is_put_to_the_approver_and_its_tokens_are_minted_once_approved() {
    let world = World::new();
    let agent = world.agent();
    let (status, _, response) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/mission"),
            Some(json!({ "description": "Sydney", "resources": [BOOKING, "http://insecure.example"], "login_hint": "user:sam@acme.example" })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        response.headers()["aauth-requirement"],
        "requirement=approval"
    );
    let approval = world.last_approval();
    assert_eq!(approval.approver.id, "dana");
    assert_eq!(approval.person.id, "sam");
    assert_eq!(approval.grant["resources"], json!([BOOKING]));

    let pending = location(&response);
    let (status, _, _) = world.call(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let someone_else = world.agent();
    let (status, _, _) = world.call(&someone_else, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    world
        .ps
        .decide(&approval.id, Decision::approve().with_budget(usd(300_000)))
        .await
        .unwrap();
    let (status, body, _) = world.call(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::OK);
    let mission = ApprovedMission::from_approval(&body).unwrap();
    assert_eq!(mission.approved_resources(), vec![BOOKING.to_string()]);
    assert_eq!(mission.mission["budget"]["amount"], 300_000);

    let person = world
        .as_resource(
            BOOKING,
            body["person_tokens"][BOOKING].as_str().unwrap(),
            &agent,
            TokenKind::Person,
        )
        .await;
    assert_eq!(person.mission_s256(), Some(mission.s256.as_str()));
    assert_ne!(person.sub, "sam", "a resource sees a directed identifier");
}

#[tokio::test]
async fn a_charge_within_budget_is_issued_at_once_and_one_over_it_is_put_to_the_approver() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = approved_mission(&world, &agent).await;
    let person = world
        .as_resource(BOOKING, &person_token, &agent, TokenKind::Person)
        .await;

    let (status, body, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/token"),
            Some(json!({ "resource_token": world.charge(BOOKING, &person, &agent, 120_000), "presented_token": person_token })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let auth = world
        .as_resource(
            BOOKING,
            body["auth_token"].as_str().unwrap(),
            &agent,
            TokenKind::Auth,
        )
        .await;
    assert_eq!(auth.ps(), Some(PS));
    assert_eq!(auth.scope(), Some("booking.reserve"));
    assert_eq!(auth.claims["budget"]["amount"], 120_000);
    assert_eq!(
        world.ps.store().mission(&s256).await.unwrap().spent,
        120_000
    );

    let (status, _, response) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/token"),
            Some(json!({ "resource_token": world.charge(BOOKING, &person, &agent, 200_000), "presented_token": person_token })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let approval = world.last_approval();
    assert_eq!(approval.kind, ApprovalKind::AuthToken);
    assert_eq!(approval.budget, Some(usd(200_000)));
    world
        .ps
        .decide(&approval.id, Decision::approve())
        .await
        .unwrap();
    let (status, body, _) = world
        .call(&agent, Method::GET, &location(&response), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["auth_token"].is_string());
    assert_eq!(
        world.ps.store().mission(&s256).await.unwrap().spent,
        320_000
    );
}

#[tokio::test]
async fn a_resource_outside_the_mission_gets_nothing() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = approved_mission(&world, &agent).await;
    let person = world
        .as_resource(BOOKING, &person_token, &agent, TokenKind::Person)
        .await;
    let (status, body, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/token"),
            Some(json!({ "resource_token": world.charge(BANK, &person, &agent, 1), "presented_token": person_token })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "access_denied");
}

#[tokio::test]
async fn a_resource_token_for_another_agent_is_refused() {
    let world = World::new();
    let agent = world.agent();
    let (_, person_token) = approved_mission(&world, &agent).await;
    let person = world
        .as_resource(BOOKING, &person_token, &agent, TokenKind::Person)
        .await;
    let thief = world.agent();
    let token = world.charge(BOOKING, &person, &thief, 1);
    let (status, body, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/token"),
            Some(json!({ "resource_token": token, "presented_token": person_token })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_resource_token");
}

#[tokio::test]
async fn completing_a_mission_is_the_persons_decision_and_ends_it() {
    let world = World::new();
    let agent = world.agent();
    let (s256, _) = approved_mission(&world, &agent).await;
    let (status, _, response) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/mission/{s256}"),
            Some(json!({ "action": "completion", "summary": "Booked" })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let approval = world.last_approval();
    assert_eq!(approval.kind, ApprovalKind::Completion);
    assert_eq!(
        approval.approver.id, "sam",
        "the person, not the organisation, accepts a completion"
    );
    world
        .ps
        .decide(&approval.id, Decision::approve())
        .await
        .unwrap();
    let (status, _, _) = world
        .call(&agent, Method::GET, &location(&response), None)
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/person-token"),
            Some(json!({ "resource": BOOKING, "mission_s256": s256 })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "mission_terminated");
}

#[tokio::test]
async fn an_agent_key_acts_for_the_first_person_it_names() {
    let world = World::new();
    let agent = world.agent();
    let ask = |hint: &str| json!({ "resource": BOOKING, "login_hint": hint });
    let (status, _, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/person-token"),
            Some(ask("user:sam@acme.example")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/person-token"),
            Some(ask("user:dana@acme.example")),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/person-token"),
            Some(json!({ "resource": BOOKING })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "a bound key needs no hint");
    let (status, body, _) = world
        .call(
            &world.agent(),
            Method::POST,
            &format!("{PS}/person-token"),
            Some(ask("user:eve@evil.example")),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_bad_signature_or_an_untrusted_provider_is_a_401() {
    let world = World::new();
    let agent = world.agent();
    let mut request = world.signed(
        &agent,
        Method::POST,
        &format!("{PS}/person-token"),
        Some(&json!({ "resource": BOOKING, "login_hint": "user:sam@acme.example" })),
    );
    *request.body_mut() = Bytes::from_static(
        br#"{"resource":"https://bank.example","login_hint":"user:sam@acme.example"}"#,
    );
    let response = world.ps.handle(request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["signature-error"], "error=invalid_input");

    let rogue_provider = SigningKey::generate();
    let key = SigningKey::generate();
    let rogue = TestAgent {
        token: mint(
            &rogue_provider,
            TokenKind::Agent,
            &json!({ "iss": "https://rogue.example", "dwk": "aauth-agent.json", "sub": "aauth:x@rogue.example",
                     "iat": T0, "exp": T0 + 60, "cnf": { "jwk": key.public_jwk() } }),
        ),
        key,
    };
    let (status, body, response) = world
        .call(
            &rogue,
            Method::POST,
            &format!("{PS}/person-token"),
            Some(json!({ "resource": BOOKING })),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(response.headers()["signature-error"], "error=invalid_jwt");
}

#[tokio::test]
async fn an_unanswered_approval_expires() {
    let world = World::new();
    let agent = world.agent();
    let (_, _, response) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/mission"),
            Some(json!({ "description": "x", "login_hint": "user:sam@acme.example" })),
        )
        .await;
    world.clock.fetch_add(25 * 3600, Ordering::SeqCst);
    let (status, _, _) = world
        .call(&agent, Method::GET, &location(&response), None)
        .await;
    assert_eq!(status, StatusCode::REQUEST_TIMEOUT);
    let late = world
        .ps
        .decide(&world.last_approval().id, Decision::approve())
        .await
        .unwrap_err();
    assert_eq!(late.status, StatusCode::GONE);
}

#[tokio::test]
async fn the_management_routes_need_the_admin_token() {
    let world = World::new();
    let agent = world.agent();
    world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/mission"),
            Some(json!({ "description": "x", "login_hint": "user:sam@acme.example" })),
        )
        .await;
    let list = |auth: Option<&str>| {
        let mut request = Request::get("/consent?status=pending");
        if let Some(auth) = auth {
            request = request.header("authorization", auth);
        }
        request.body(Bytes::new()).unwrap()
    };
    assert_eq!(
        world.ps.handle(list(None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        world.ps.handle(list(Some("Bearer nope"))).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = world.ps.handle(list(Some("Bearer admin"))).await;
    let pending: Vec<Approval> = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(pending.len(), 1);

    let decide = Request::post(format!("/consent/{}/decision", pending[0].id))
        .header("authorization", "Bearer admin")
        .body(Bytes::from(r#"{"decision":"deny","decided_by":"dana"}"#))
        .unwrap();
    let decided: Approval = serde_json::from_slice(world.ps.handle(decide).await.body()).unwrap();
    assert_eq!(decided.status, ApprovalStatus::Denied);
    assert_eq!(decided.decided_by.as_deref(), Some("dana"));
}

#[tokio::test]
async fn an_approved_charge_is_collected_once() {
    let world = World::new();
    let agent = world.agent();
    let (s256, person_token) = approved_mission(&world, &agent).await;
    let person = world
        .as_resource(BOOKING, &person_token, &agent, TokenKind::Person)
        .await;
    let (status, _, response) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/token"),
            Some(json!({ "resource_token": world.charge(BOOKING, &person, &agent, 400_000), "presented_token": person_token })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    world
        .ps
        .decide(&world.last_approval().id, Decision::approve())
        .await
        .unwrap();
    let pending = location(&response);
    let (status, _, _) = world.call(&agent, Method::GET, &pending, None).await;
    assert_eq!(status, StatusCode::OK);
    for _ in 0..3 {
        let (status, body, _) = world.call(&agent, Method::GET, &pending, None).await;
        assert_eq!(status, StatusCode::GONE, "{body}");
    }
    assert_eq!(
        world.ps.store().mission(&s256).await.unwrap().spent,
        400_000
    );
}

#[tokio::test]
async fn a_decision_is_made_once() {
    let world = World::new();
    let agent = world.agent();
    world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/mission"),
            Some(json!({ "description": "x", "login_hint": "user:sam@acme.example" })),
        )
        .await;
    let id = world.last_approval().id;
    let (first, second) = tokio::join!(
        world.ps.decide(&id, Decision::approve()),
        world.ps.decide(&id, Decision::approve()),
    );
    assert!(
        first.is_ok() != second.is_ok(),
        "exactly one decision lands"
    );
    assert_eq!(world.ps.store().missions().await.len(), 1);
}

#[tokio::test]
async fn a_terminated_mission_issues_nothing_more() {
    let world = World::new();
    let agent = world.agent();
    let (_, _, response) = world
        .call(
            &agent,
            Method::POST,
            &format!("{PS}/mission"),
            Some(json!({ "description": "x", "resources": [BOOKING], "login_hint": "user:sam@acme.example" })),
        )
        .await;
    let decided = world
        .ps
        .decide(&world.last_approval().id, Decision::approve())
        .await
        .unwrap();
    world
        .ps
        .terminate(decided.mission_s256.as_deref().unwrap())
        .await
        .unwrap();
    let (status, body, _) = world
        .call(&agent, Method::GET, &location(&response), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "mission_terminated");
}

#[tokio::test]
async fn another_key_under_the_same_agent_cannot_touch_someone_elses_mission() {
    let world = World::new();
    let sams_agent = world.agent();
    let (s256, _) = approved_mission(&world, &sams_agent).await;
    let danas_agent = world.agent();
    let (status, _, _) = world
        .call(
            &danas_agent,
            Method::POST,
            &format!("{PS}/person-token"),
            Some(json!({ "resource": BOOKING, "login_hint": "user:dana@acme.example" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = world
        .call(
            &danas_agent,
            Method::POST,
            &format!("{PS}/mission/{s256}"),
            Some(json!({ "action": "update", "description": "rebook to Dana" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_unsigned_jwks_uri_request_fetches_nothing() {
    let world = World::new();
    let key = SigningKey::generate();
    let headers = aauth_protocol::httpsig::sign(
        &key,
        &aauth_protocol::httpsig::SignatureKey::JwksUri {
            id: "https://internal.example".into(),
            dwk: "aauth-agent.json".into(),
            kid: "k".into(),
        },
        &Outgoing {
            method: "POST",
            url: &format!("{PS}/person-token"),
            content_type: None,
            body: Some(b"{}"),
            created: world.now(),
        },
    )
    .unwrap();
    let mut request = Request::post("/person-token");
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = world
        .ps
        .handle(request.body(Bytes::from_static(b"{}")).unwrap())
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body: Value = serde_json::from_slice(response.body()).unwrap();
    assert!(
        !body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("internal.example"),
        "{body}"
    );
}

#[tokio::test]
async fn identical_missions_approved_together_stay_apart() {
    let world = World::new();
    let propose = |agent: TestAgent, hint: &'static str| {
        let world = &world;
        async move {
            world
                .call(
                    &agent,
                    Method::POST,
                    &format!("{PS}/mission"),
                    Some(json!({ "description": "Sydney", "resources": [BOOKING], "login_hint": hint })),
                )
                .await;
            world
                .ps
                .decide(&world.last_approval().id, Decision::approve())
                .await
                .unwrap()
                .mission_s256
                .unwrap()
        }
    };
    // The same agent, description and resources, approved in the same second.
    let sams = propose(world.agent(), "user:sam@acme.example").await;
    let ended = world.ps.terminate(&sams).await.unwrap();
    let danas = propose(world.agent(), "user:dana@acme.example").await;
    assert_ne!(sams, danas);
    let still = world.ps.terminate(&sams).await.unwrap();
    assert_eq!(still.person.id, "sam");
    assert_eq!(still.completed_at, ended.completed_at);
}
