//! Interop with the JavaScript implementations: `@hellocoop/httpsig`, by the
//! spec's author, and `@introspection-sdk/person-server`. Runs where
//! `AAUTH_INTEROP` is set, after `npm install` in `crates/aauth-protocol/interop/`.

#![cfg(feature = "client")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use aauth_protocol::client::{Agent, Answer, ConnectTo, Person, PersonServerClient, Reach};
use aauth_protocol::discovery::{JwksResolver, NoKeys, StaticFetch};
use aauth_protocol::httpsig::{sign_jwt, verify, Incoming, Outgoing, VerifyOptions};
use aauth_protocol::mission::ApprovedMission;
use aauth_protocol::token::{mint, verify_token, TokenKind, VerifyTokenOptions};
use aauth_protocol::SigningKey;
use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

const AP: &str = "https://agents.example";
const BOOKING: &str = "https://api.booking.example";

fn interop_dir() -> Option<PathBuf> {
    if std::env::var_os("AAUTH_INTEROP").is_none() {
        eprintln!(
            "skipped: set AAUTH_INTEROP=1 after npm install in crates/aauth-protocol/interop/"
        );
        return None;
    }
    Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("interop"))
}

fn agent_token(ap: &SigningKey, session: &SigningKey) -> String {
    let now = aauth_protocol::now_unix();
    mint(
        ap,
        TokenKind::Agent,
        &json!({
            "iss": AP, "dwk": "aauth-agent.json", "sub": "aauth:travel@agents.example",
            "iat": now, "exp": now + 3600, "cnf": { "jwk": session.public_jwk() },
        }),
    )
}

#[test]
fn hellocoop_verifies_requests_signed_here() {
    let Some(dir) = interop_dir() else { return };
    let key = SigningKey::generate();
    let token = agent_token(&SigningKey::generate(), &key);
    let cases: Vec<Value> = [
        ("POST", "/v1/reserve", Some(r#"{"offer_id":"fl-qf1"}"#)),
        ("GET", "/v1/search", None),
    ]
    .into_iter()
    .map(|(method, path, body)| {
        let url = format!("https://api.booking.example{path}");
        let set = sign_jwt(
            &key,
            &token,
            &Outgoing {
                method,
                url: &url,
                content_type: None,
                body: body.map(str::as_bytes),
                created: aauth_protocol::now_unix(),
            },
        )
        .unwrap();
        let headers: serde_json::Map<String, Value> = set.into_iter().map(|(k, v)| (k, json!(v))).collect();
        json!({ "method": method, "authority": "api.booking.example", "path": path, "headers": headers, "body": body })
    })
    .collect();
    let path = std::env::temp_dir().join(format!("aauth-rs-signed-{}.json", std::process::id()));
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "cases": cases })).unwrap(),
    )
    .unwrap();
    let output = Command::new("node")
        .args(["verify.mjs", path.to_str().unwrap()])
        .current_dir(dir)
        .output()
        .expect("node runs");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn requests_hellocoop_signs_verify_here() {
    let Some(dir) = interop_dir() else { return };
    let output = Command::new("node")
        .arg("sign.mjs")
        .current_dir(dir)
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let signed: Value = serde_json::from_slice(&output.stdout).unwrap();
    for case in signed["cases"].as_array().unwrap() {
        let mut headers = HeaderMap::new();
        for (name, value) in case["headers"].as_object().unwrap() {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value.as_str().unwrap()).unwrap(),
            );
        }
        let body = case["body"].as_str().map(str::as_bytes);
        let verified = verify(
            &Incoming {
                method: case["method"].as_str().unwrap(),
                authority: "ps.example",
                path: case["path"].as_str().unwrap(),
                query: None,
                headers: &headers,
                body,
            },
            &VerifyOptions {
                require_content_digest: true,
                ..Default::default()
            },
            &NoKeys,
        )
        .await
        .unwrap_or_else(|e| panic!("{} {}: {e}", case["method"], case["path"]));
        let jwk: aauth_protocol::Jwk = serde_json::from_value(signed["jwk"].clone()).unwrap();
        assert_eq!(verified.thumbprint, jwk.thumbprint().unwrap());
    }
}

/// The Rust agent, through `PersonServerClient`, against the JavaScript
/// Person Server: a mission proposed, approved, and its person token
/// verified here.
#[tokio::test]
async fn the_javascript_person_server_answers_the_rust_agent() {
    let Some(dir) = interop_dir() else { return };
    let ap = SigningKey::generate();
    let session = SigningKey::generate();
    let jwks_path = std::env::temp_dir().join(format!("aauth-rs-ap-{}.json", std::process::id()));
    std::fs::write(&jwks_path, serde_json::to_vec(&ap.jwks()).unwrap()).unwrap();
    let mut child = tokio::process::Command::new("node")
        .args(["person-server.mjs", jwks_path.to_str().unwrap()])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("node runs");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let port = lines
        .next_line()
        .await
        .unwrap()
        .expect("the server prints its port");
    let local = format!("http://127.0.0.1:{port}");

    let client = PersonServerClient::new(
        reqwest::Client::new(),
        Reach {
            allow_http: true,
            allow_private: true,
            connect_to: ConnectTo::parse(&format!("ps.example={local}")).unwrap(),
        },
    );
    let token = agent_token(&ap, &session);
    let agent = Agent {
        agent_token: &token,
        key: &session,
    };
    let metadata = client.metadata("https://ps.example").await.unwrap();
    let sam = Person {
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

    let admin = reqwest::Client::new();
    let pending: Value = admin
        .get(format!("{local}/consent?status=pending"))
        .bearer_auth("admin")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = pending[0]["id"].as_str().unwrap();
    let decided = admin
        .post(format!("{local}/consent/{id}/decision"))
        .bearer_auth("admin")
        .json(&json!({ "decision": "approve" }))
        .send()
        .await
        .unwrap();
    assert!(decided.status().is_success());

    let Answer::Granted(granted) = client.poll(&agent, &location).await.unwrap() else {
        panic!("expected the mission")
    };
    let mission = ApprovedMission::from_approval(&granted).expect("the mission matches its hash");
    assert_eq!(mission.approved_resources(), vec![BOOKING.to_string()]);

    let ps_jwks: Value = admin
        .get(format!("{local}/.well-known/jwks.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let fetch = StaticFetch::default();
    fetch.insert(
        "https://ps.example/.well-known/aauth-person.json",
        json!({ "issuer": "https://ps.example", "jwks": ps_jwks }),
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
        &JwksResolver::new(Arc::new(fetch)),
    )
    .await
    .unwrap();
    assert_eq!(person.mission_s256(), Some(mission.s256.as_str()));
    let _ = std::fs::remove_file(&jwks_path);
}
