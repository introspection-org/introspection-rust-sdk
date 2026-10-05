//! Integration tests for the members CP surface (`client.members`) and the
//! `metadata` an identity assertion carries, backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, RunRequest};
use introspection_sdk::{
    AdvancedOptions, ClientConfig, IntrospectionClient, MemberCreateParams, MemberListParams,
    MemberType, MemberUpdateParams, Members, PaginationParams, RunnerIdentity,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const MEMBER_ID: &str = "11111111-1111-1111-1111-111111111111";
const RUNTIME_ID: &str = "22222222-2222-4222-8222-222222222222";

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

fn member_id() -> Uuid {
    Uuid::parse_str(MEMBER_ID).unwrap()
}

fn member_json() -> serde_json::Value {
    json!({
        "id": MEMBER_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "created_at": "2026-10-04T00:00:00Z",
        "updated_at": "2026-10-04T00:00:00Z",
        "deleted_at": null,
        "email": null,
        "name": null,
        "external_user_id": "user:u_demo",
        "image_url": null,
        "role": "member",
        "member_type": "customer",
        "is_deactivated": false,
        "tags": ["customer:acme"],
        "metadata": {"plan": "enterprise", "region": "eu"},
        "application_idp_id": null,
        "connector_id": null,
        "integration_id": null,
        "is_external_credential_agent": false,
    })
}

fn page(records: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "records": records, "count": records.len(), "total_count": records.len(), "next": null })
}

/// Every `(key, value)` pair on the request's query string, in order.
fn query_pairs(request: &Request) -> Vec<(String, String)> {
    request
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[tokio::test]
async fn list_parses_tags_and_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/members"))
        .and(query_param("member_type", "customer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![member_json()])))
        .mount(&server)
        .await;

    let members = Members::new(build_http(&server));
    let params = MemberListParams {
        member_type: Some(MemberType::Customer),
        ..Default::default()
    };
    let found: Vec<_> = members.list(&params).collect().await;

    let member = found[0].as_ref().unwrap();
    assert_eq!(member.member_type, MemberType::Customer);
    assert_eq!(member.tags, vec!["customer:acme".to_string()]);
    assert_eq!(member.metadata["plan"], "enterprise");
    assert_eq!(member.metadata.len(), 2);
}

#[tokio::test]
async fn a_member_without_metadata_reads_as_an_empty_map() {
    let server = MockServer::start().await;
    let mut body = member_json();
    let object = body.as_object_mut().unwrap();
    object.remove("metadata");
    object.remove("tags");
    Mock::given(method("GET"))
        .and(path(format!("/v1/members/{MEMBER_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let member = Members::new(build_http(&server))
        .get(member_id())
        .await
        .unwrap();

    assert!(member.metadata.is_empty());
    assert!(member.tags.is_empty());
}

#[tokio::test]
async fn list_sends_tag_metadata_and_resolve_filters_as_repeated_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![])))
        .mount(&server)
        .await;

    let members = Members::new(build_http(&server));
    let params = MemberListParams {
        pagination: PaginationParams {
            limit: Some(50),
            next: None,
        },
        tag: Some("customer:acme".into()),
        // A value may itself contain colons; the API splits on the first.
        metadata: Some(HashMap::from([
            ("region".into(), "eu".into()),
            ("crm_url".into(), "https://crm.example/a".into()),
        ])),
        ids: Some(vec![member_id()]),
        external_user_ids: Some(vec!["user:a".into(), "user:b".into()]),
        ..Default::default()
    };
    members.list(&params).next_page().await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let mut pairs = query_pairs(&requests[0]);
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("external_user_id".into(), "user:a".into()),
            ("external_user_id".into(), "user:b".into()),
            ("id".into(), MEMBER_ID.into()),
            ("limit".into(), "50".into()),
            ("metadata".into(), "crm_url:https://crm.example/a".into()),
            ("metadata".into(), "region:eu".into()),
            ("tag".into(), "customer:acme".into()),
        ]
    );
}

#[tokio::test]
async fn an_empty_metadata_filter_is_not_sent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![])))
        .mount(&server)
        .await;

    let params = MemberListParams {
        metadata: Some(HashMap::new()),
        ..Default::default()
    };
    Members::new(build_http(&server))
        .list(&params)
        .next_page()
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(query_pairs(&requests[0]).is_empty());
}

#[tokio::test]
async fn create_sends_tags_and_metadata_only_when_set() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/members"))
        .and(body_json(json!({
            "email": "ada@example.com",
            "name": "Ada Lovelace",
            "tags": ["team:support"],
            "metadata": {"plan": "enterprise"},
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(member_json()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/members"))
        .and(body_json(json!({
            "email": "grace@example.com",
            "name": "Grace Hopper",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(member_json()))
        .mount(&server)
        .await;

    let members = Members::new(build_http(&server));
    members
        .create(&MemberCreateParams {
            tags: Some(vec!["team:support".into()]),
            metadata: Some(HashMap::from([("plan".into(), "enterprise".into())])),
            ..MemberCreateParams::new("ada@example.com", "Ada Lovelace")
        })
        .await
        .unwrap();
    members
        .create(&MemberCreateParams::new(
            "grace@example.com",
            "Grace Hopper",
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn update_replaces_metadata_and_an_empty_map_clears_it() {
    let server = MockServer::start().await;
    let member_path = format!("/v1/members/{MEMBER_ID}");
    Mock::given(method("PATCH"))
        .and(path(member_path.clone()))
        .and(body_json(json!({ "metadata": {"plan": "team"} })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member_json()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(member_path.clone()))
        .and(body_json(json!({ "metadata": {} })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member_json()))
        .expect(1)
        .mount(&server)
        .await;
    // Unset fields stay off the wire: omitted means "unchanged".
    Mock::given(method("PATCH"))
        .and(path(member_path))
        .and(body_json(json!({ "tags": [] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member_json()))
        .expect(1)
        .mount(&server)
        .await;

    let members = Members::new(build_http(&server));
    let replace = MemberUpdateParams {
        metadata: Some(HashMap::from([("plan".into(), "team".into())])),
        ..Default::default()
    };
    members.update(member_id(), &replace).await.unwrap();
    let clear = MemberUpdateParams {
        metadata: Some(HashMap::new()),
        ..Default::default()
    };
    members.update(member_id(), &clear).await.unwrap();
    let clear_tags = MemberUpdateParams {
        tags: Some(vec![]),
        ..Default::default()
    };
    members.update(member_id(), &clear_tags).await.unwrap();
}

#[tokio::test]
async fn an_asserted_identity_carries_member_metadata_on_run() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/runtimes/{RUNTIME_ID}/run")))
        .and(body_partial_json(json!({
            "identity": {
                "user_id": "u_demo",
                "metadata": {"plan": "enterprise"},
            },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "session_id": "sess_1",
            "deployment": {
                "endpoint": "https://dp.example.com",
                "slug": "gcp01",
                "region": "us-east-1",
            },
            "session_token": "jwt",
            "expires_at": "2026-10-04T01:00:00Z",
            "runtime_context": {
                "runtime_id": RUNTIME_ID,
                "recipe_id": "33333333-3333-4333-8333-333333333333",
                // Metadata never rides the session claims, so it is not echoed.
                "identity": {"user_id": "u_demo"},
            },
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(server.uri()),
                ..Default::default()
            })
            .build()
            .unwrap(),
    )
    .unwrap();
    let request = RunRequest {
        identity: Some(RunnerIdentity {
            user_id: Some("u_demo".into()),
            metadata: Some(HashMap::from([("plan".into(), "enterprise".into())])),
            ..Default::default()
        }),
        ..Default::default()
    };
    let runner = client
        .runtimes()
        .handle(Uuid::parse_str(RUNTIME_ID).unwrap())
        .run(request)
        .await
        .unwrap();

    assert!(runner.context().identity.metadata.is_none());
}

#[test]
fn an_identity_without_metadata_omits_the_key() {
    let identity = RunnerIdentity {
        user_id: Some("u_demo".into()),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(identity).unwrap(),
        json!({"user_id": "u_demo"})
    );
}
