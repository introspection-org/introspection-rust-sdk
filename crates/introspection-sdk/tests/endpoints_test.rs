//! Integration tests for the endpoints CP surface (`client.endpoints`),
//! backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{EndpointKind, EndpointListParams, Endpoints, PaginationParams};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ENDPOINT_ID: &str = "11111111-1111-1111-1111-111111111111";
const CONNECTOR_ID: &str = "22222222-2222-2222-2222-222222222222";

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

fn endpoint_json() -> serde_json::Value {
    json!({
        "id": ENDPOINT_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": "00000000-0000-0000-0000-0000000000bb",
        "created_at": "2026-10-01T00:00:00Z",
        "updated_at": "2026-10-01T00:00:00Z",
        "name": "CRM",
        "host": "mcp.crm.example",
        "base_url": "https://mcp.crm.example/mcp",
        "kind": "mcp",
        "metadata": {
            "mcp_server_id": "crm",
            "connector_id": CONNECTOR_ID,
            "extra_hosts": [],
            "auth_audience": null,
            "region": "eu",
        },
        "assertion_claims": null,
        "runtime_id": null,
        "runtime_group_id": null,
        "environment": null,
        "verify_path": null,
        "headers": {"X-Api-Key": "${CRM_KEY}"},
        "header_names": ["X-Api-Key"],
        "credential_refs": ["CRM_KEY"],
        "last_verified_at": null,
        "authorized_application_id": null,
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
    })
}

#[tokio::test]
async fn list_finds_the_mcp_binding_for_a_server() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/endpoints"))
        .and(query_param("project", "support"))
        .and(query_param("kind", "mcp"))
        .and(query_param("mcp_server_id", "crm"))
        .and(query_param("limit", "100"))
        .and(query_param_is_missing("runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [endpoint_json()],
            "count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;

    let page = Endpoints::new(build_http(&server))
        .list(&EndpointListParams {
            project: Some("support".into()),
            kind: Some(EndpointKind::Mcp),
            mcp_server_id: Some("crm".into()),
            pagination: PaginationParams {
                limit: Some(100),
                next: None,
            },
            ..Default::default()
        })
        .next_page()
        .await
        .unwrap()
        .unwrap();

    let endpoint = &page.records[0];
    assert_eq!(endpoint.kind, EndpointKind::Mcp);
    assert_eq!(
        endpoint.base_url.as_deref(),
        Some("https://mcp.crm.example/mcp")
    );
    assert_eq!(endpoint.metadata.mcp_server_id.as_deref(), Some("crm"));
    assert_eq!(
        endpoint.metadata.connector_id,
        Some(Uuid::parse_str(CONNECTOR_ID).unwrap())
    );
    assert_eq!(endpoint.metadata.other["region"], "eu");
    assert_eq!(endpoint.header_names, vec!["X-Api-Key".to_string()]);
    assert!(endpoint.runtime_id.is_none() && endpoint.runtime_group_id.is_none());
}

#[tokio::test]
async fn get_reads_one_binding_and_keeps_an_unknown_kind() {
    let server = MockServer::start().await;
    let mut body = endpoint_json();
    body["kind"] = json!("websocket");
    Mock::given(method("GET"))
        .and(path(format!("/v1/endpoints/{ENDPOINT_ID}")))
        .and(query_param_is_missing("project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let endpoint = Endpoints::new(build_http(&server))
        .get(Uuid::parse_str(ENDPOINT_ID).unwrap(), None)
        .await
        .unwrap();

    assert_eq!(endpoint.kind, EndpointKind::Other("websocket".into()));
    assert_eq!(endpoint.kind.as_str(), "websocket");
}

#[test]
fn a_null_metadata_reads_as_empty() {
    let mut row = endpoint_json();
    row["metadata"] = serde_json::Value::Null;
    let endpoint: introspection_sdk::Endpoint = serde_json::from_value(row).unwrap();
    assert_eq!(endpoint.metadata, Default::default());
    // Written back, it is an empty object, not a null.
    assert_eq!(
        serde_json::to_value(&endpoint).unwrap()["metadata"],
        json!({})
    );

    let mut row = endpoint_json();
    row.as_object_mut().unwrap().remove("metadata");
    let endpoint: introspection_sdk::Endpoint = serde_json::from_value(row).unwrap();
    assert_eq!(endpoint.metadata, Default::default());
}

#[test]
fn a_null_page_of_records_reads_as_empty() {
    let page: introspection_sdk::Paginated<introspection_sdk::Endpoint> =
        serde_json::from_value(json!({"records": null, "count": 0, "next": null})).unwrap();
    assert!(page.records.is_empty());
    let page: introspection_sdk::Paginated<introspection_sdk::Endpoint> =
        serde_json::from_value(json!({"count": 0})).unwrap();
    assert!(page.records.is_empty());
    let page: introspection_sdk::Paginated<introspection_sdk::Endpoint> =
        serde_json::from_value(json!({"records": [endpoint_json()], "count": 1})).unwrap();
    assert_eq!(page.records.len(), 1);
}
