//! What survives the trip from an HTTP error response into the typed error.
//!
//! The DP's error envelope carries a machine-readable `code` alongside the
//! human `detail`, and a rate-limited response carries `Retry-After`. Both
//! were read and discarded: `IntrospectionAPIError::Http` had a public `code`
//! field hardcoded to `None` with no accessor, and the retry floor lived only
//! inside the retry loop, so a caller handling the error after the budget was
//! spent had nothing to schedule against. The JS and Python SDKs surface both.
//!
//! This drives real requests through the client rather than calling the
//! private translator, so the header read (which has to happen before the
//! body is consumed) is exercised the way it runs in production.

use std::time::Duration;

use introspection_sdk::api::IntrospectionAPIError;
use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(server.uri()),
                ..Default::default()
            })
            .build()
            .unwrap(),
    )
    .unwrap()
}

/// A runtime UUID the mock will be asked about. Any single CP call would
/// do; `get` is the simplest one that is not a paginator.
const RUNTIME_ID: uuid::Uuid = uuid::uuid!("11111111-1111-4111-8111-111111111111");
const ROUTE: &str = "/v1/runtimes/11111111-1111-4111-8111-111111111111";

async fn get_error(server: &MockServer) -> IntrospectionAPIError {
    client(server)
        .runtimes()
        .get(RUNTIME_ID, "proj")
        .await
        .expect_err("the mock only answers with errors")
}

#[tokio::test]
async fn the_envelope_code_reaches_the_caller() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ROUTE))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "detail": "runner session has expired",
            "code": "runner_expired",
        })))
        .mount(&server)
        .await;

    let err = get_error(&server).await;
    assert_eq!(err.status(), Some(401));
    // Without this, an expired runner JWT is indistinguishable from a bad
    // API key -- both are a bare 401.
    assert_eq!(err.code(), Some("runner_expired"));
    assert_eq!(err.to_string(), "runner session has expired (status=401)");
}

#[tokio::test]
async fn an_envelope_without_a_code_reports_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ROUTE))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "detail": "not found" })))
        .mount(&server)
        .await;

    let err = get_error(&server).await;
    assert_eq!(err.code(), None);
    assert_eq!(err.retry_after(), None);
}

#[tokio::test]
async fn retry_after_survives_onto_the_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ROUTE))
        .respond_with(
            // A small floor deliberately: `Retry-After` is honoured as the
            // floor on the transparent retry too, so a large value here
            // would make this test sleep through the retry budget.
            ResponseTemplate::new(429)
                .insert_header("retry-after", "1")
                .set_body_json(json!({ "detail": "slow down", "code": "rate_limited" })),
        )
        .mount(&server)
        .await;

    let err = get_error(&server).await;
    assert_eq!(err.status(), Some(429));
    assert_eq!(err.code(), Some("rate_limited"));
    // The caller schedules its own retry with the server's number instead
    // of guessing one.
    assert_eq!(err.retry_after(), Some(Duration::from_secs(1)));
}

#[tokio::test]
async fn a_non_json_error_body_still_carries_the_retry_floor() {
    // The header read has to happen before the body is consumed, so the
    // text-body branch is a separate path from the JSON one. A 403 rather
    // than a 5xx: the header is read the same way at any status, and a
    // retryable one would spend the retry budget sleeping on this floor.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ROUTE))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("retry-after", "5")
                .set_body_string("upstream unavailable"),
        )
        .mount(&server)
        .await;

    let err = get_error(&server).await;
    assert_eq!(err.retry_after(), Some(Duration::from_secs(5)));
    assert_eq!(err.code(), None);
}

#[tokio::test]
async fn every_request_names_the_sdk_and_its_release() {
    // The same `introspection-sdk/<version>` string the OTLP exporters in
    // this crate send, and the same one the other SDKs send. It used to be
    // language-tagged here and nowhere else.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ROUTE))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let _ = get_error(&server).await;

    let request = &server.received_requests().await.unwrap()[0];
    let user_agent = request
        .headers
        .get("user-agent")
        .expect("a User-Agent is sent")
        .to_str()
        .unwrap();
    assert_eq!(
        user_agent,
        format!("introspection-sdk/{}", env!("CARGO_PKG_VERSION"))
    );
}

#[tokio::test]
async fn a_caller_supplied_user_agent_is_not_overwritten() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ROUTE))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(server.uri()),
                additional_headers: Some(
                    [("User-Agent".to_string(), "my-app/1.0".to_string())]
                        .into_iter()
                        .collect(),
                ),
            })
            .build()
            .unwrap(),
    )
    .unwrap();
    let _ = client.runtimes().get(RUNTIME_ID, "proj").await;

    let request = &server.received_requests().await.unwrap()[0];
    assert_eq!(
        request.headers.get("user-agent").unwrap().to_str().unwrap(),
        "my-app/1.0"
    );
}
