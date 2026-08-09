//! Integration tests for the connectors CP surface (`client.connectors` and
//! its nested `connections`) backed by `wiremock`.
//!
//! Like the other REST tests these construct the namespace directly via
//! [`HttpClient::from_parts`], swapping the real CP for a mock server.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, IntrospectionAPIError};
use introspection_sdk::{
    ConnectionCreateParams, ConnectionSubjectType, Connections, ConnectorAuthMode,
    ConnectorAuthorizeParams, ConnectorCreateParams, ConnectorListParams, ConnectorUpdateParams,
    Connectors, PaginationParams,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

const CONNECTOR_ID: &str = "11111111-1111-1111-1111-111111111111";
const CONNECTION_ID: &str = "22222222-2222-2222-2222-222222222222";

fn connector_id() -> Uuid {
    Uuid::parse_str(CONNECTOR_ID).unwrap()
}

fn connection_id() -> Uuid {
    Uuid::parse_str(CONNECTION_ID).unwrap()
}

fn connector_json() -> serde_json::Value {
    json!({
        "id": CONNECTOR_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": "00000000-0000-0000-0000-0000000000bb",
        "created_at": "2026-08-08T00:00:00Z",
        "updated_at": "2026-08-08T00:00:00Z",
        "slug": "slack-support",
        "name": "Slack (support)",
        "provider": "slack",
        "auth_mode": "oauth_stored",
        "environment": "production",
        "scopes": ["chat:write"],
        "api_hosts": ["slack.com"],
        "approval_policy": "human",
        "status": "active",
        "requires_runtime": true,
    })
}

fn connection_json() -> serde_json::Value {
    json!({
        "id": CONNECTION_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "created_at": "2026-08-08T00:00:00Z",
        "updated_at": "2026-08-08T00:00:00Z",
        "connector_id": CONNECTOR_ID,
        "member_id": "00000000-0000-0000-0000-0000000000cc",
        "runtime_group_id": "33333333-3333-3333-3333-333333333333",
        "subject_type": "workspace",
        "scopes_granted": ["chat:write"],
        "status": "active",
        "token_expires_at": null,
    })
}

fn page(records: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "records": records, "count": records.len(), "total_count": records.len(), "next": null })
}

#[tokio::test]
async fn list_sends_project_and_limit_and_parses_records() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/connectors"))
        .and(query_param("project", "proj-1"))
        .and(query_param("limit", "50"))
        .and(query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![connector_json()])))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let params = ConnectorListParams {
        project: Some("proj-1".into()),
        limit: Some(50),
        ..Default::default()
    };
    let found: Vec<_> = connectors.list(&params).collect().await;

    assert_eq!(found.len(), 1);
    let connector = found[0].as_ref().unwrap();
    assert_eq!(connector.slug, "slack-support");
    // Server-derived: a chat provider must name the agent that replies.
    assert!(connector.requires_runtime);
}

#[tokio::test]
async fn create_posts_the_body_with_project_on_the_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/connectors"))
        .and(query_param("project", "proj-1"))
        .and(body_json(json!({
            "name": "Slack (support)",
            "provider": "slack",
            "auth_mode": "oauth_stored",
            "scopes": ["chat:write"],
            "client_id": "client-abc",
            "client_secret": "secret-xyz",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(connector_json()))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let params = ConnectorCreateParams {
        scopes: Some(vec!["chat:write".to_string()]),
        client_id: Some("client-abc".to_string()),
        // Write-only: sent here, never present on the response.
        client_secret: Some("secret-xyz".to_string()),
        ..ConnectorCreateParams::new("Slack (support)", "slack", ConnectorAuthMode::OauthStored)
    };

    let created = connectors.create(&params, "proj-1").await.unwrap();
    assert_eq!(created.slug, "slack-support");
}

#[tokio::test]
async fn get_update_and_delete_address_one_connector() {
    let server = MockServer::start().await;
    let connector_path = format!("/v1/connectors/{CONNECTOR_ID}");
    Mock::given(method("GET"))
        .and(path(connector_path.clone()))
        .and(query_param("project", "proj-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector_json()))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(connector_path.clone()))
        .and(query_param("project", "proj-1"))
        // An unset secret must not ride along as null: omitted is "unchanged".
        .and(body_json(json!({ "name": "Slack (renamed)" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector_json()))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(connector_path))
        .and(query_param("project", "proj-1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));

    let fetched = connectors.get(connector_id(), "proj-1").await.unwrap();
    assert_eq!(fetched.id, connector_id());

    let update = ConnectorUpdateParams {
        name: Some("Slack (renamed)".to_string()),
        ..Default::default()
    };
    connectors
        .update(connector_id(), &update, "proj-1")
        .await
        .unwrap();

    connectors.delete(connector_id(), "proj-1").await.unwrap();
}

#[tokio::test]
async fn authorize_merges_the_connector_id_and_returns_both_expiry_forms() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/authorize"))
        .and(body_json(json!({
            "connector_id": CONNECTOR_ID,
            "runtime": "support-agent",
            "expires_in": 3600,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorize_url": "https://slack.com/oauth/v2/authorize?client_id=abc&state=single-use",
            "expires_in": 3600,
            "expires_at": "2026-08-08T21:00:00Z",
        })))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let params = ConnectorAuthorizeParams {
        runtime: Some("support-agent".into()),
        expires_in: Some(3600),
        ..Default::default()
    };

    let minted = connectors.authorize(connector_id(), &params).await.unwrap();

    assert!(minted.authorize_url.starts_with("https://slack.com/oauth"));
    assert_eq!(minted.expires_in, 3600);
    assert_eq!(minted.expires_at, "2026-08-08T21:00:00Z");
}

#[tokio::test]
async fn authorize_sends_only_the_connector_when_given_nothing_else() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/authorize"))
        .and(body_json(json!({ "connector_id": CONNECTOR_ID })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorize_url": "https://accounts.google.com/o/oauth2/v2/auth?state=x",
            "expires_in": 600,
            "expires_at": "2026-08-08T20:10:00Z",
        })))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let minted = connectors
        .authorize(connector_id(), &ConnectorAuthorizeParams::default())
        .await
        .unwrap();

    assert_eq!(minted.expires_in, 600);
}

#[tokio::test]
async fn authorize_surfaces_the_missing_runtime_detail() {
    let server = MockServer::start().await;
    let detail = "`runtime` is required for a slack connector — it names the agent that replies";
    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/authorize"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({ "detail": detail })))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let err = connectors
        .authorize(connector_id(), &ConnectorAuthorizeParams::default())
        .await
        .unwrap_err();

    match err {
        IntrospectionAPIError::Http {
            status, message, ..
        } => {
            assert_eq!(status, 422);
            assert_eq!(message, detail);
        }
        other => panic!("expected an HTTP error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_disabled_deployment_keeps_the_servers_wording() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/connectors/{CONNECTOR_ID}")))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({ "detail": "Connectors are not enabled" })),
        )
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let err = connectors.get(connector_id(), "proj-1").await.unwrap_err();

    match err {
        IntrospectionAPIError::Http {
            status, message, ..
        } => {
            assert_eq!(status, 404);
            // "not enabled here", not "no such connector".
            assert_eq!(message, "Connectors are not enabled");
        }
        other => panic!("expected an HTTP error, got {other:?}"),
    }
}

#[tokio::test]
async fn connections_list_and_create_use_the_nested_path() {
    let server = MockServer::start().await;
    let connections_path = format!("/v1/connectors/{CONNECTOR_ID}/connections");
    Mock::given(method("GET"))
        .and(path(connections_path.clone()))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![connection_json()])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(connections_path))
        .and(body_json(json!({
            "access_token": "xoxb-token",
            "subject_type": "app",
            "scopes_granted": ["chat:write"],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(connection_json()))
        .mount(&server)
        .await;

    let connections = Connections::new(build_http(&server));

    let params = PaginationParams {
        limit: Some(25),
        ..Default::default()
    };
    let listed: Vec<_> = connections.list(connector_id(), &params).collect().await;
    assert_eq!(listed.len(), 1);
    let connection = listed[0].as_ref().unwrap();
    assert_eq!(connection.subject_type, ConnectionSubjectType::Workspace);

    let create = ConnectionCreateParams {
        subject_type: Some(ConnectionSubjectType::App),
        scopes_granted: Some(vec!["chat:write".to_string()]),
        ..ConnectionCreateParams::new("xoxb-token")
    };
    let created = connections.create(connector_id(), &create).await.unwrap();
    assert_eq!(created.connector_id, connector_id());
}

#[tokio::test]
async fn connections_get_and_revoke_address_one_connection() {
    let server = MockServer::start().await;
    let connection_path = format!("/v1/connectors/{CONNECTOR_ID}/connections/{CONNECTION_ID}");
    Mock::given(method("GET"))
        .and(path(connection_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(connection_json()))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(connection_path))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let connections = Connections::new(build_http(&server));

    let fetched = connections
        .get(connector_id(), connection_id())
        .await
        .unwrap();
    assert_eq!(fetched.id, connection_id());

    connections
        .revoke(connector_id(), connection_id())
        .await
        .unwrap();
}

#[tokio::test]
async fn the_connectors_namespace_carries_connections() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/connectors/{CONNECTOR_ID}/connections/{CONNECTION_ID}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(connection_json()))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let fetched = connectors
        .connections
        .get(connector_id(), connection_id())
        .await
        .unwrap();

    assert_eq!(fetched.connector_id, connector_id());
}
