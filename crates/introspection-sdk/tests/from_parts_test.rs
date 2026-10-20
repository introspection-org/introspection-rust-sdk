//! `HttpClient::from_parts` — the seam a caller that owns its transport
//! depends on (the `introspection` CLI is one).
//!
//! The contract: the supplied `reqwest::Client` is used as given, so its
//! default headers are what reach the wire and `cfg.token` is never applied;
//! `cfg.api_url` and `cfg.timeout` still govern every unary request; and the
//! stream reads stay unbounded by that timeout.

use std::collections::HashMap;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::IntrospectionAPIError;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde_json::{json, Value};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_with_cookie(server: &MockServer, timeout: Duration) -> HttpClient {
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, HeaderValue::from_static("intro_cp_session=abc"));
    let inner = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();
    HttpClient::from_parts(
        inner,
        HttpConfig {
            api_url: server.uri(),
            token: "never-sent".into(),
            additional_headers: HashMap::new(),
            timeout,
            max_retries: 0,
            retry_base: Duration::ZERO,
        },
    )
}

#[tokio::test]
async fn the_supplied_client_is_used_as_given_and_the_config_token_is_not_applied() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/oidc/me"))
        .and(header("cookie", "intro_cp_session=abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"email": "a@b.c"})))
        .expect(1)
        .mount(&server)
        .await;
    // A bearer reaching the wire would be the config token leaking through
    // a client the caller configured deliberately.
    Mock::given(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let http = client_with_cookie(&server, Duration::from_secs(5));
    let me: Value = http.get_json("/v1/oidc/me", &()).await.unwrap();
    assert_eq!(me["email"], "a@b.c");
}

#[tokio::test]
async fn the_config_timeout_bounds_unary_calls_but_not_stream_reads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"records": []}))
                .set_delay(Duration::from_millis(400)),
        )
        .mount(&server)
        .await;

    let http = client_with_cookie(&server, Duration::from_millis(100));
    let unary = http.get_json::<(), Value>("/v1/tasks", &()).await;
    assert!(
        matches!(unary, Err(IntrospectionAPIError::Transport(ref e)) if e.is_timeout()),
        "{unary:?}"
    );

    let stream = http
        .get_stream_raw("/v1/tasks", Some("text/event-stream"), None)
        .await
        .unwrap();
    assert_eq!(stream.status().as_u16(), 200);
}
