//! Integration tests for the projects CP surface (`client.projects`), backed
//! by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{PaginationParams, ProjectListParams, Projects};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "11111111-1111-1111-1111-111111111111";

fn build_http(server: &MockServer) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: server.uri(),
        token: "intro_test".to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        max_retries: 2,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

fn project_json() -> serde_json::Value {
    json!({
        "id": PROJECT_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "deployment_id": "00000000-0000-0000-0000-0000000000bb",
        "name": "Support desk",
        "slug": "support-desk",
        "description": null,
        "settings": {"theme": "dark"},
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
    })
}

#[tokio::test]
async fn get_reads_a_project_by_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PROJECT_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_json()))
        .expect(1)
        .mount(&server)
        .await;

    let project = Projects::new(build_http(&server))
        .get(Uuid::parse_str(PROJECT_ID).unwrap())
        .await
        .unwrap();

    assert_eq!(project.id, Uuid::parse_str(PROJECT_ID).unwrap());
    assert_eq!(project.slug.as_deref(), Some("support-desk"));
    assert_eq!(project.name, "Support desk");
    assert!(project.description.is_none());
    assert_eq!(project.settings.unwrap()["theme"], "dark");
}

#[tokio::test]
async fn get_reads_a_project_by_slug_and_tolerates_a_missing_slug() {
    let server = MockServer::start().await;
    let mut body = project_json();
    body.as_object_mut().unwrap().remove("slug");
    body.as_object_mut().unwrap().remove("created_by_member_id");
    Mock::given(method("GET"))
        .and(path("/v1/projects/support%20desk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let project = Projects::new(build_http(&server))
        .get("support desk")
        .await
        .unwrap();

    assert!(project.slug.is_none());
    assert!(project.created_by_member_id.is_none());
}

#[tokio::test]
async fn get_surfaces_a_missing_project() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/nope"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"detail": "Project not found"})),
        )
        .mount(&server)
        .await;

    let err = Projects::new(build_http(&server))
        .get("nope")
        .await
        .unwrap_err();

    assert_eq!(err.status(), Some(404));
}

#[tokio::test]
async fn list_pages_through_the_org_projects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .and(query_param("project", "support-desk"))
        .and(query_param("limit", "10"))
        .and(query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [project_json()],
            "count": 1,
            "next": null,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let page = Projects::new(build_http(&server))
        .list(&ProjectListParams {
            project: Some("support-desk".into()),
            pagination: PaginationParams {
                limit: Some(10),
                next: None,
            },
        })
        .next_page()
        .await
        .unwrap()
        .unwrap();

    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].slug.as_deref(), Some("support-desk"));
}
