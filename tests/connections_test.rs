//! Integration tests for the member connections DP surface (`connections()`
//! on the client and the runner), backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, IntrospectionAPIError, RunRequest};
use introspection_sdk::{
    AdvancedOptions, ClientConfig, IntrospectionClient, MemberConnectionCreate,
    MemberConnectionListParams, MemberConnections, PaginationParams, Runner,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const CONNECTION_ID: &str = "11111111-1111-7111-8111-111111111111";
const MEMBER_ID: &str = "44444444-4444-7444-8444-444444444444";
const RUNTIME_ID: &str = "55555555-5555-4555-8555-555555555555";
const RUNTIME_GROUP_ID: &str = "33333333-3333-4333-8333-333333333333";

fn build_http(server: &MockServer) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: server.uri(),
        token: "intro_test".to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        max_retries: 0,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

fn id(value: &str) -> Uuid {
    Uuid::parse_str(value).unwrap()
}

fn connection_json() -> serde_json::Value {
    json!({
        "id": CONNECTION_ID,
        "member_id": MEMBER_ID,
        "app": "gmail",
        "account_name": "ada@example.com",
        "healthy": true,
        "created_at": "2026-10-04T00:00:00Z",
        "added_later": "ignored",
    })
}

fn connect_page_json() -> serde_json::Value {
    json!({
        "authorize_url": "https://connect.example.com/c/abc",
        "expires_in": 600,
        "expires_at": "2026-10-06T00:10:00Z",
    })
}

fn query_pairs(request: &Request) -> Vec<(String, String)> {
    let mut pairs: Vec<_> = request
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    pairs.sort();
    pairs
}

/// A runner opened against `cp` whose Data Plane is `dp`, with
/// `runtime_group_id` in its runtime context.
async fn runner(cp: &MockServer, dp: &MockServer, runtime_group_id: Option<&str>) -> Runner {
    Mock::given(method("POST"))
        .and(path(format!("/v1/runtimes/{RUNTIME_ID}/run")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "session_id": "sess_1",
            "deployment": {"endpoint": dp.uri(), "slug": "gcp01", "region": "us-east-1"},
            "session_token": "runner-jwt",
            "expires_at": "2026-01-01T00:00:00Z",
            "runtime_context": {
                "runtime_id": RUNTIME_ID,
                "runtime_group_id": runtime_group_id,
                "recipe_id": "22222222-2222-4222-8222-222222222222",
                "identity": {},
            },
        })))
        .mount(cp)
        .await;
    IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(cp.uri()),
                ..Default::default()
            })
            .build()
            .unwrap(),
    )
    .unwrap()
    .runtimes()
    .handle(id(RUNTIME_ID))
    .run(RunRequest::default())
    .await
    .unwrap()
}

#[tokio::test]
async fn list_parses_connections_and_sends_its_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/connections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [connection_json()],
            "count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;

    let found: Vec<_> = MemberConnections::new(build_http(&server), None)
        .list(&MemberConnectionListParams {
            pagination: PaginationParams {
                limit: Some(10),
                ..Default::default()
            },
            member_id: Some(id(MEMBER_ID)),
            app: Some("gmail".into()),
            ..Default::default()
        })
        .collect()
        .await;

    let connection = found[0].as_ref().unwrap();
    assert_eq!(connection.id, id(CONNECTION_ID));
    assert_eq!(connection.member_id, id(MEMBER_ID));
    assert_eq!(connection.app, "gmail");
    assert_eq!(connection.account_name.as_deref(), Some("ada@example.com"));
    assert!(connection.healthy);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        query_pairs(&requests[0]),
        vec![
            ("app".to_string(), "gmail".to_string()),
            ("limit".to_string(), "10".to_string()),
            ("member_id".to_string(), MEMBER_ID.to_string()),
        ]
    );
}

#[tokio::test]
async fn list_follows_the_next_cursor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/connections"))
        .and(wiremock::matchers::query_param("next", "cursor_2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [connection_json()],
            "count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/connections"))
        .and(wiremock::matchers::query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [connection_json()],
            "count": 1,
            "next": "cursor_2",
        })))
        .mount(&server)
        .await;

    let found: Vec<_> = MemberConnections::new(build_http(&server), None)
        .list(&MemberConnectionListParams::default())
        .collect()
        .await;
    assert_eq!(found.len(), 2);
}

#[tokio::test]
async fn create_on_the_client_sends_the_runtime_it_was_given() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/connections"))
        .and(body_json(
            json!({"app": "gmail", "runtime": "customer-agent"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(connect_page_json()))
        .expect(1)
        .mount(&server)
        .await;

    let page = MemberConnections::new(build_http(&server), None)
        .create(&MemberConnectionCreate {
            runtime: Some("customer-agent".into()),
            ..MemberConnectionCreate::new("gmail")
        })
        .await
        .unwrap();
    assert_eq!(page.authorize_url, "https://connect.example.com/c/abc");
    assert_eq!(page.expires_in, 600);
    assert_eq!(page.expires_at.as_deref(), Some("2026-10-06T00:10:00Z"));
}

#[tokio::test]
async fn create_on_the_client_without_a_runtime_fails_before_any_request() {
    let server = MockServer::start().await;

    let err = MemberConnections::new(build_http(&server), None)
        .create(&MemberConnectionCreate::new("gmail"))
        .await
        .unwrap_err();
    assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn create_on_a_runner_sends_the_runners_runtime_group_on_its_token() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/connections"))
        .and(header("authorization", "Bearer runner-jwt"))
        .and(body_json(
            json!({"app": "gmail", "runtime": RUNTIME_GROUP_ID}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(connect_page_json()))
        .expect(1)
        .mount(&dp)
        .await;

    let runner = runner(&cp, &dp, Some(RUNTIME_GROUP_ID)).await;
    runner
        .connections()
        .create(&MemberConnectionCreate::new("gmail"))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_explicit_runtime_overrides_the_runners_own() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/connections"))
        .and(body_json(json!({"app": "slack", "runtime": "other-agent"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(connect_page_json()))
        .expect(1)
        .mount(&dp)
        .await;

    let runner = runner(&cp, &dp, Some(RUNTIME_GROUP_ID)).await;
    runner
        .connections()
        .create(&MemberConnectionCreate {
            runtime: Some("other-agent".into()),
            ..MemberConnectionCreate::new("slack")
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn create_on_a_runner_without_a_runtime_group_fails_before_any_request() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;

    let runner = runner(&cp, &dp, None).await;
    let err = runner
        .connections()
        .create(&MemberConnectionCreate::new("gmail"))
        .await
        .unwrap_err();
    assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
    assert!(dp.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn get_reads_one_connection() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/connections/{CONNECTION_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(connection_json()))
        .mount(&server)
        .await;

    let connection = MemberConnections::new(build_http(&server), None)
        .get(id(CONNECTION_ID))
        .await
        .unwrap();
    assert_eq!(connection.app, "gmail");
}

#[tokio::test]
async fn delete_is_a_bodyless_204_and_a_403_surfaces_as_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/connections/{CONNECTION_ID}")))
        .respond_with(ResponseTemplate::new(204))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/connections/{CONNECTION_ID}")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"detail": "Missing scope"})))
        .mount(&server)
        .await;

    let connections = MemberConnections::new(build_http(&server), None);
    connections.delete(id(CONNECTION_ID)).await.unwrap();
    match connections.delete(id(CONNECTION_ID)).await {
        Err(IntrospectionAPIError::Http { status: 403, .. }) => {}
        other => panic!("expected a 403, got {other:?}"),
    }
}
