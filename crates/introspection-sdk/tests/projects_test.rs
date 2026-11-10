//! Integration tests for the projects CP surface (`client.projects`), backed
//! by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    PaginationParams, ProjectListParams, Projects, RuntimePeering, RuntimePeeringListParams,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "11111111-1111-1111-1111-111111111111";
const PEERING_ID: &str = "22222222-2222-2222-2222-222222222222";
const RUNTIME_GROUP_ID: &str = "33333333-3333-3333-3333-333333333333";

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

fn peering_json() -> serde_json::Value {
    json!({
        "id": PEERING_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": PROJECT_ID,
        "runtime_group_id": RUNTIME_GROUP_ID,
        "granted_org_id": null,
        "granted_project_id": "00000000-0000-0000-0000-0000000000dd",
        "granted_deployment_id": null,
        "expires_at": "2026-12-01T00:00:00Z",
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
        "created_at": "2026-10-01T12:00:00Z",
        "updated_at": "2026-10-02T12:00:00Z",
        "deleted_at": null,
    })
}

#[tokio::test]
async fn peerings_list_narrows_to_a_runtime_group() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/support-desk/peerings"))
        .and(query_param("runtime", "billing-agent"))
        .and(query_param("limit", "50"))
        .and(query_param_is_missing("granted"))
        .and(query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [peering_json()],
            "count": 1,
            "next": null,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let page = Projects::new(build_http(&server))
        .peerings
        .list(
            "support-desk",
            &RuntimePeeringListParams {
                runtime: Some("billing-agent".into()),
                pagination: PaginationParams {
                    limit: Some(50),
                    next: None,
                },
                ..Default::default()
            },
        )
        .next_page()
        .await
        .unwrap()
        .unwrap();

    assert_eq!(page.records.len(), 1);
    let peering = &page.records[0];
    assert_eq!(peering.id, Uuid::parse_str(PEERING_ID).unwrap());
    assert_eq!(
        peering.runtime_group_id,
        Uuid::parse_str(RUNTIME_GROUP_ID).unwrap()
    );
    assert!(peering.granted_org_id.is_none());
    assert_eq!(
        peering.granted_project_id,
        Some(Uuid::parse_str("00000000-0000-0000-0000-0000000000dd").unwrap())
    );
    assert_eq!(peering.expires_at.as_deref(), Some("2026-12-01T00:00:00Z"));
    assert!(peering.deleted_at.is_none());
}

#[tokio::test]
async fn peerings_list_reads_the_peerings_granted_to_a_project_across_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PROJECT_ID}/peerings")))
        .and(query_param("granted", "true"))
        .and(query_param_is_missing("runtime"))
        .and(query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [peering_json()],
            "count": 2,
            "next": "cursor-2",
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut second = peering_json();
    second["id"] = json!("44444444-4444-4444-4444-444444444444");
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PROJECT_ID}/peerings")))
        .and(query_param("granted", "true"))
        .and(query_param("next", "cursor-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [second],
            "count": 2,
            "next": null,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let peerings: Vec<RuntimePeering> = Projects::new(build_http(&server))
        .peerings
        .list(
            Uuid::parse_str(PROJECT_ID).unwrap(),
            &RuntimePeeringListParams {
                granted: Some(true),
                ..Default::default()
            },
        )
        .collect_all(10)
        .await
        .unwrap();

    assert_eq!(peerings.len(), 2);
    assert_eq!(
        peerings[1].id,
        Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap()
    );
}

#[tokio::test]
async fn peerings_get_reads_one_by_id() {
    let server = MockServer::start().await;
    let mut body = peering_json();
    for key in ["expires_at", "created_by_member_id", "deleted_at"] {
        body.as_object_mut().unwrap().remove(key);
    }
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/projects/support-desk/peerings/{PEERING_ID}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let peering = Projects::new(build_http(&server))
        .peerings
        .get("support-desk", Uuid::parse_str(PEERING_ID).unwrap())
        .await
        .unwrap();

    assert_eq!(peering.project_id, Uuid::parse_str(PROJECT_ID).unwrap());
    assert_eq!(peering.updated_at, "2026-10-02T12:00:00Z");
    assert!(peering.expires_at.is_none());
    assert!(peering.created_by_member_id.is_none());
}

#[tokio::test]
async fn peerings_get_surfaces_a_peering_the_project_cannot_see() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/projects/support-desk/peerings/{PEERING_ID}"
        )))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"detail": "Peering not found"})),
        )
        .mount(&server)
        .await;

    let err = Projects::new(build_http(&server))
        .peerings
        .get("support-desk", Uuid::parse_str(PEERING_ID).unwrap())
        .await
        .unwrap_err();

    assert_eq!(err.status(), Some(404));
}
