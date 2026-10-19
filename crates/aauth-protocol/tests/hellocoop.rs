//! Agreement with `@hellocoop/httpsig`, by the spec's author, on fixed
//! requests (`vectors/hellocoop.json`): what it signed verifies here, and what
//! this crate signs is byte for byte what it verified.

use aauth_protocol::discovery::NoKeys;
use aauth_protocol::httpsig::{sign_jwt, verify, Incoming, Outgoing, VerifyOptions};
use aauth_protocol::SigningKey;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value};

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/hellocoop.json")).unwrap()
}

fn key(vectors: &Value) -> SigningKey {
    let seed = URL_SAFE_NO_PAD
        .decode(vectors["seed"].as_str().unwrap())
        .unwrap();
    SigningKey::from_bytes(&seed.try_into().unwrap())
}

fn headers(case: &Value) -> &Map<String, Value> {
    case["headers"].as_object().unwrap()
}

#[test]
fn requests_signed_here_are_the_ones_hellocoop_verified() {
    let vectors = vectors();
    let key = key(&vectors);
    for case in vectors["signed_here_verified_by_hellocoop"]
        .as_array()
        .unwrap()
    {
        let set = sign_jwt(
            &key,
            vectors["jwt"].as_str().unwrap(),
            &Outgoing {
                method: case["method"].as_str().unwrap(),
                url: case["url"].as_str().unwrap(),
                content_type: case["content_type"].as_str(),
                body: case["body"].as_str().map(str::as_bytes),
                created: vectors["created"].as_i64().unwrap(),
            },
        )
        .unwrap();
        let signed: Map<String, Value> = set
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        assert_eq!(&signed, headers(case), "{} {}", case["method"], case["url"]);
    }
}

#[tokio::test]
async fn requests_hellocoop_signed_verify_here() {
    let vectors = vectors();
    let thumbprint = key(&vectors).thumbprint().to_string();
    for case in vectors["signed_by_hellocoop"].as_array().unwrap() {
        let url = url::Url::parse(case["url"].as_str().unwrap()).unwrap();
        let mut map = HeaderMap::new();
        for (name, value) in headers(case) {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value.as_str().unwrap()).unwrap(),
            );
        }
        let verified = verify(
            &Incoming {
                method: case["method"].as_str().unwrap(),
                authority: url.host_str().unwrap(),
                path: url.path(),
                query: url.query(),
                headers: &map,
                body: case["body"].as_str().map(str::as_bytes),
            },
            &VerifyOptions {
                now: vectors["created"].as_i64().unwrap(),
                require_content_digest: true,
                ..Default::default()
            },
            &NoKeys,
        )
        .await
        .unwrap_or_else(|e| panic!("{} {}: {e}", case["method"], case["url"]));
        assert_eq!(verified.thumbprint, thumbprint);
    }
}
