//! The agent's `PersonServerClient`, from `aauth-protocol`, against this
//! Person Server over a real socket: a mission proposed, approved, and its
//! person token verified.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use aauth_person_server::{
    Approval, ApprovalKind, Config, Decision, Directory, Hooks, Person, PersonServer,
};
use aauth_protocol::client::{Agent, Answer, ConnectTo, Person as Hint, PersonServerClient, Reach};
use aauth_protocol::discovery::{JwksResolver, StaticFetch};
use aauth_protocol::mission::ApprovedMission;
use aauth_protocol::token::{mint, verify_token, TokenKind, VerifyTokenOptions};
use aauth_protocol::SigningKey;
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use serde_json::json;

const PS: &str = "https://ps.example";
const AP: &str = "https://agents.example";
const BOOKING: &str = "https://api.booking.example";

struct Sam;

#[async_trait]
impl Directory for Sam {
    async fn person(&self, hint: &str, _tenant: &str) -> Option<Person> {
        (hint == "user:sam@acme.example").then(|| Person::new("sam"))
    }

    async fn approver(&self, _kind: ApprovalKind, _person: &Person) -> Option<Person> {
        Some(Person::new("dana"))
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

/// Serves `ps` on 127.0.0.1 and returns the address.
async fn serve(ps: Arc<PersonServer>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let ps = ps.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: http::Request<Incoming>| {
                    let ps = ps.clone();
                    async move {
                        let (parts, body) = request.into_parts();
                        let body = body.collect().await.unwrap().to_bytes();
                        let response = ps.handle(http::Request::from_parts(parts, body)).await;
                        Ok::<_, Infallible>(response.map(Full::<Bytes>::new))
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn the_agent_client_runs_a_mission_with_this_server() {
    let ap = SigningKey::generate();
    let session = SigningKey::generate();
    let ps_key = SigningKey::generate();
    let fetch = StaticFetch::default();
    fetch.insert(
        format!("{AP}/.well-known/aauth-agent.json"),
        json!({ "issuer": AP, "jwks": ap.jwks() }),
    );
    let ps_jwks = ps_key.jwks();
    let seen = Seen::default();
    let ps = Arc::new(
        PersonServer::new(
            Config::new(PS, Sam, [AP])
                .signing_key(ps_key)
                .hooks(seen.clone())
                .fetch(Arc::new(fetch)),
        )
        .unwrap(),
    );
    let local = serve(ps.clone()).await;

    let client = PersonServerClient::new(
        reqwest::Client::new(),
        Reach {
            allow_http: true,
            allow_private: true,
            connect_to: ConnectTo::parse(&format!("ps.example={local}")).unwrap(),
        },
    );
    let now = aauth_protocol::now_unix();
    let token = mint(
        &ap,
        TokenKind::Agent,
        &json!({
            "iss": AP, "dwk": "aauth-agent.json", "sub": "aauth:travel@agents.example",
            "iat": now, "exp": now + 3600, "cnf": { "jwk": session.public_jwk() },
        }),
    );
    let agent = Agent {
        agent_token: &token,
        key: &session,
    };
    let metadata = client.metadata(PS).await.unwrap();
    let sam = Hint {
        login_hint: Some("user:sam@acme.example"),
        tenant: None,
    };
    let answer = client
        .propose_mission(
            &agent,
            &metadata,
            &json!({ "description": "Sydney", "resources": [BOOKING] }),
            &sam,
        )
        .await
        .unwrap();
    let Answer::Pending { location, .. } = answer else {
        panic!("expected a pending approval, got {answer:?}")
    };
    assert!(matches!(
        client.poll(&agent, &location).await.unwrap(),
        Answer::Pending { .. }
    ));

    let id = seen.0.lock().unwrap()[0].id.clone();
    ps.decide("default", &id, Decision::approve())
        .await
        .unwrap();
    let Answer::Granted(granted) = client.poll(&agent, &location).await.unwrap() else {
        panic!("expected the mission")
    };
    let mission = ApprovedMission::from_approval(&granted).expect("the mission matches its hash");
    assert_eq!(mission.approved_resources(), vec![BOOKING.to_string()]);

    let keys = StaticFetch::default();
    keys.insert(
        format!("{PS}/.well-known/aauth-person.json"),
        json!({ "issuer": PS, "jwks": ps_jwks }),
    );
    let person = verify_token(
        &VerifyTokenOptions {
            jwt: granted["person_tokens"][BOOKING].as_str().unwrap(),
            http_signature_thumbprint: session.thumbprint(),
            resource: BOOKING,
            accept: &[TokenKind::Person],
            now: aauth_protocol::now_unix(),
            clock_tolerance: 60,
        },
        &JwksResolver::new(Arc::new(keys)),
    )
    .await
    .unwrap();
    assert_eq!(person.mission_s256(), Some(mission.s256.as_str()));
}
