use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient, RunRequest};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::with_token("intro_test").advanced(AdvancedOptions {
            base_api_url: Some(server.uri()),
            ..Default::default()
        }),
    )
    .unwrap()
}

fn runner_spec(
    server: &MockServer,
    runtime_id: Uuid,
    experiment_id: Option<Uuid>,
) -> serde_json::Value {
    json!({
        "session_id": "session-1",
        "deployment": {
            "endpoint": server.uri(),
            "slug": "local",
            "region": "local"
        },
        "session_token": "session-token",
        "expires_at": "2026-01-01T01:00:00Z",
        "runtime_context": {
            "runtime_id": runtime_id,
            "runtime_group_id": "00000000-0000-0000-0000-000000000099",
            "experiment_id": experiment_id,
            "recipe_id": "00000000-0000-0000-0000-000000000077",
            "agent_name": "support-agent",
            "identity": {
                "user_id": "user-1",
                "anonymous_id": null,
                "conversation_id": null
            }
        }
    })
}

#[tokio::test]
async fn runtime_ref_resolves_then_runs_with_current_request() {
    let server = MockServer::start().await;
    let runtime_id = Uuid::parse_str("00000000-0000-0000-0000-000000000042").unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/runtimes"))
        .and(query_param("runtime", "support"))
        .and(query_param("only_active", "true"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{"id": runtime_id}],
            "count": 1,
            "next": null
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/runtimes/{runtime_id}/run")))
        .and(body_json(json!({
            "agent_name": "support-agent",
            "scope": "support"
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(runner_spec(&server, runtime_id, None)),
        )
        .mount(&server)
        .await;

    let runner = client(&server)
        .runtime("support")
        .run(RunRequest {
            agent_name: Some("support-agent".into()),
            scope: Some("support".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(runner.context().runtime_id, Some(runtime_id));
    assert_eq!(
        runner.context().agent_name.as_deref(),
        Some("support-agent")
    );
}

#[tokio::test]
async fn experiment_handle_runs_without_project_management_selector() {
    let server = MockServer::start().await;
    let runtime_id = Uuid::parse_str("00000000-0000-0000-0000-000000000042").unwrap();
    let experiment_id = Uuid::parse_str("00000000-0000-0000-0000-000000000088").unwrap();

    Mock::given(method("POST"))
        .and(path(format!("/v1/experiments/{experiment_id}/run")))
        .and(query_param_is_missing("project"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(runner_spec(
            &server,
            runtime_id,
            Some(experiment_id),
        )))
        .mount(&server)
        .await;

    let runner = client(&server)
        .experiment(experiment_id)
        .run(RunRequest::default())
        .await
        .unwrap();

    assert_eq!(runner.context().experiment_id, Some(experiment_id));
}
