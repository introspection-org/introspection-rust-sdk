use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EXPERIMENT_ID: &str = "22222222-2222-2222-2222-222222222222";
const PROJECT_ID: &str = "00000000-0000-0000-0000-0000000000bb";

fn experiment(name: &str) -> serde_json::Value {
    json!({
        "id": EXPERIMENT_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": PROJECT_ID,
        "name": name,
        "status": "running",
        "created_at": "2026-08-25T12:00:00Z",
        "updated_at": "2026-08-25T12:00:00Z"
    })
}

#[tokio::test]
async fn create_update_delete_preserve_control_plane_documents() {
    let server = MockServer::start().await;
    let id = Uuid::parse_str(EXPERIMENT_ID).unwrap();
    Mock::given(method("POST"))
        .and(path("/v1/experiments"))
        .and(body_json(
            json!({"project_id": PROJECT_ID, "name": "created", "custom": {"kept": true}}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(experiment("created")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/experiments/{EXPERIMENT_ID}")))
        .and(query_param("project", PROJECT_ID))
        .and(body_json(json!({"name": "renamed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(experiment("renamed")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/experiments/{EXPERIMENT_ID}")))
        .and(query_param("project", PROJECT_ID))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = IntrospectionClient::new(ClientConfig::with_token("intro_test").advanced(
        AdvancedOptions {
            base_api_url: Some(server.uri()),
            ..Default::default()
        },
    ))
    .unwrap();
    let experiments = client.experiments();
    assert_eq!(
        experiments
            .create(&json!({"project_id": PROJECT_ID, "name": "created", "custom": {"kept": true}}))
            .await
            .unwrap()
            .name,
        "created"
    );
    assert_eq!(
        experiments
            .update(id, PROJECT_ID, &json!({"name": "renamed"}))
            .await
            .unwrap()
            .name,
        "renamed"
    );
    experiments.delete(id, PROJECT_ID).await.unwrap();
}
