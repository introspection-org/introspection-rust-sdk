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
    ConnectionBrokerSubjectType, ConnectionCreateParams, ConnectionCreateSubjectType,
    ConnectionMissionConstraints, ConnectionSubjectType, ConnectionTokenParams,
    ConnectionTokenResult, Connections, ConnectorAuthMode, ConnectorAuthorizeParams,
    ConnectorCreateParams, ConnectorListParams, ConnectorUpdateParams, Connectors,
    PaginationParams, RunnerIdentity,
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
        "created_by_member_id": "00000000-0000-0000-0000-0000000000dd",
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
async fn list_uses_authenticated_project_and_parses_records() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/connectors"))
        .and(query_param("limit", "50"))
        .and(query_param_is_missing("next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![connector_json()])))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let params = ConnectorListParams {
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
async fn create_posts_the_body_using_authenticated_project_scope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/connectors"))
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

    let created = connectors.create(&params).await.unwrap();
    assert_eq!(created.slug, "slack-support");
}

#[tokio::test]
async fn get_update_and_delete_address_one_connector() {
    let server = MockServer::start().await;
    let connector_path = format!("/v1/connectors/{CONNECTOR_ID}");
    Mock::given(method("GET"))
        .and(path(connector_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector_json()))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(connector_path.clone()))
        // An unset secret must not ride along as null: omitted is "unchanged".
        .and(body_json(json!({ "name": "Slack (renamed)" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector_json()))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(connector_path))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));

    let fetched = connectors.get(connector_id()).await.unwrap();
    assert_eq!(fetched.id, connector_id());

    let update = ConnectorUpdateParams {
        name: Some("Slack (renamed)".to_string()),
        ..Default::default()
    };
    connectors.update(connector_id(), &update).await.unwrap();

    connectors.delete(connector_id()).await.unwrap();
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
async fn authorize_carries_an_asserted_end_customer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/authorize"))
        .and(body_json(json!({
            "connector_id": CONNECTOR_ID,
            "runtime": "support-agent",
            "identity": { "user_id": "u_demo" },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorize_url": "https://slack.com/oauth/v2/authorize?state=single-use",
            "expires_in": 600,
            "expires_at": "2026-08-08T20:10:00Z",
        })))
        .mount(&server)
        .await;

    let connectors = Connectors::new(build_http(&server));
    let params = ConnectorAuthorizeParams {
        runtime: Some("support-agent".into()),
        // Only the asserted field rides along; the rest stay absent.
        identity: Some(RunnerIdentity {
            user_id: Some("u_demo".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    connectors.authorize(connector_id(), &params).await.unwrap();
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
async fn pipedream_apps_and_progressive_scope_authorization() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/connectors/{CONNECTOR_ID}/apps")))
        .and(query_param("q", "sheets"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "slug": "google_sheets",
                "name": "Google Sheets",
                "icon_url": "https://assets.example/sheets.png",
                "auth_type": "oauth"
            }]
        })))
        .mount(&server)
        .await;
    let connectors = Connectors::new(build_http(&server));
    let apps = connectors
        .list_apps(connector_id(), Some("sheets"), Some(5))
        .await
        .unwrap();
    assert_eq!(apps[0].slug, "google_sheets");

    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/authorize"))
        .and(body_json(json!({
            "connector_id": CONNECTOR_ID,
            "runtime": "coding-agent",
            "app": "google_sheets",
            "allow_progressive_scopes": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorize_url": "https://pipedream.com/_static/connect.html?app=google_sheets",
            "expires_in": 600,
            "expires_at": "2026-08-08T20:10:00Z"
        })))
        .mount(&server)
        .await;
    connectors
        .authorize(
            connector_id(),
            &ConnectorAuthorizeParams {
                runtime: Some("coding-agent".into()),
                app: Some("google_sheets".to_string()),
                allow_progressive_scopes: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
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
    let err = connectors.get(connector_id()).await.unwrap_err();

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
    // The installer is recorded apart from the subject: for a workspace
    // install they are never the same principal.
    assert!(connection.created_by_member_id.is_some());
    assert_ne!(connection.created_by_member_id, connection.member_id);

    let create = ConnectionCreateParams {
        subject_type: Some(ConnectionCreateSubjectType::App),
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
async fn connections_get_token_returns_a_provider_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/token"))
        .and(body_json(json!({
            "connector_id": CONNECTOR_ID,
            "subject": "user",
            "action": "calendar.list",
            "requested_permissions": { "host": "calendar.example.com" },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": "provider-token",
            "token_type": "bearer",
            "expires_at": null,
            "scopes": ["calendar.read"],
        })))
        .mount(&server)
        .await;

    let result = Connections::new(build_http(&server))
        .get_token(
            connector_id(),
            &ConnectionTokenParams {
                subject: Some(ConnectionBrokerSubjectType::User),
                action: Some("calendar.list".to_string()),
                requested_permissions: Some(ConnectionMissionConstraints {
                    host: Some("calendar.example.com".to_string()),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();

    match result {
        ConnectionTokenResult::Token(token) => assert_eq!(token.token, "provider-token"),
        other => panic!("expected token, got {other:?}"),
    }
}

#[tokio::test]
async fn connections_get_token_preserves_pending_authorization() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/connections/token"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "authorization_pending",
            "mission_id": "33333333-3333-3333-3333-333333333333",
            "approval_url": "https://consent.example/m/333?cap=secret",
        })))
        .mount(&server)
        .await;

    let result = Connections::new(build_http(&server))
        .get_token(connector_id(), &ConnectionTokenParams::default())
        .await
        .unwrap();

    match result {
        ConnectionTokenResult::AuthorizationPending(pending) => {
            assert_eq!(pending.status, "authorization_pending");
        }
        other => panic!("expected pending authorization, got {other:?}"),
    }
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
