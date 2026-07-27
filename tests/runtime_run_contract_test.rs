use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient, RunRequest};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn runner_spec(server: &MockServer, runtime_id: Uuid) -> Value {
    json!({
        "session_id": "session-1",
        "deployment": {
            "endpoint": server.uri(),
            "slug": "local",
            "region": "local"
        },
        "session_token": "runner-token",
        "expires_at": "2026-01-01T01:00:00Z",
        "runtime_context": {
            "runtime_id": runtime_id,
            "runtime_group_id": "00000000-0000-0000-0000-000000000010",
            "experiment_id": null,
            "recipe_id": "00000000-0000-0000-0000-000000000020",
            "identity": {}
        }
    })
}

fn client(server: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::with_token("intro-test").advanced(AdvancedOptions {
            base_api_url: Some(server.uri()),
            ..Default::default()
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn stable_runtime_run_and_refresh_replay_the_stable_selector() {
    let server = MockServer::start().await;
    let runtime_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/runtimes/run"))
        .and(body_json(json!({
            "runtime": "customer-agent",
            "ttl_seconds": 600
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(runner_spec(&server, runtime_id)))
        .expect(2)
        .mount(&server)
        .await;

    let client = client(&server);
    let runner = client
        .runtimes()
        .run(
            "customer-agent",
            RunRequest {
                ttl_seconds: Some(600),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(runner.context().runtime_id, runtime_id);
    runner.refresh().await.unwrap();
}

#[tokio::test]
async fn exact_runtime_handle_and_refresh_replay_the_exact_id() {
    let server = MockServer::start().await;
    let runtime_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/runtimes/run"))
        .and(body_json(json!({
            "runtime_id": runtime_id,
            "scope": "customer:acme"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(runner_spec(&server, runtime_id)))
        .expect(2)
        .mount(&server)
        .await;

    let client = client(&server);
    let handle = client.runtimes().handle(runtime_id);
    let runner = handle
        .run(RunRequest {
            scope: Some("customer:acme".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(handle.id(), runtime_id);
    assert_eq!(runner.context().runtime_id, runtime_id);
    runner.refresh().await.unwrap();
}
