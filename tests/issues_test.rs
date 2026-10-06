//! Integration tests for the issues DP surface (`issues()` on the client and
//! the runner), backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, IntrospectionAPIError};
use introspection_sdk::{
    IssueCreate, IssueLink, IssueListParams, IssueOwner, IssuePriority, IssueRequestStatus,
    IssueRequestUpdate, IssueSpanReference, IssueStatus, IssueUpdate, Issues, PaginationParams,
    TaskStatus,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ISSUE_ID: &str = "11111111-1111-7111-8111-111111111111";
const TASK_ID: &str = "22222222-2222-7222-8222-222222222222";
const MEMBER_ID: &str = "44444444-4444-7444-8444-444444444444";
const REQUEST_ID: &str = "55555555-5555-7555-8555-555555555555";

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

fn issue_json() -> serde_json::Value {
    json!({
        "id": ISSUE_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": "00000000-0000-0000-0000-0000000000bb",
        "created_at": "2026-10-04T00:00:00Z",
        "updated_at": "2026-10-05T00:00:00Z",
        "display_index": 42,
        "title": "Checkout retries double-charge",
        "description": "Customers on retry see two charges.",
        "priority": "high",
        "status": "waiting",
        "revision": 3,
        "task_id": TASK_ID,
        "task_status": "running",
        "member_id": null,
        "closed_at": null,
        "tags": ["customer:acme"],
        "metadata": {"severity": 2, "flow": "checkout", "paged": true},
        "files": [{"file_id": "66666666-6666-7666-8666-666666666666", "name": "trace.json"}],
        "links": [{"url": "https://status.example.com/1"}],
        "events": [{"event_id": "77777777-7777-7777-8777-777777777777"}],
        "spans": [{"trace_id": "0123456789abcdef0123456789abcdef", "span_id": "0123456789abcdef"}],
        "open_requests": [{
            "id": REQUEST_ID,
            "question": "Refund the second charge?",
            "assignee_id": MEMBER_ID,
            "created_at": "2026-10-05T00:00:00Z",
        }],
        "added_later": "ignored",
    })
}

fn query_pairs(request: &Request) -> Vec<(String, String)> {
    request
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[tokio::test]
async fn list_parses_the_issue_and_sends_list_filters_as_repeated_keys() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [issue_json()],
            "count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;

    let found: Vec<_> = Issues::new(build_http(&server))
        .list(&IssueListParams {
            pagination: PaginationParams {
                limit: Some(20),
                ..Default::default()
            },
            status: vec![IssueStatus::Open, IssueStatus::Waiting],
            owner: vec![IssueOwner::Me],
            assigned_to_me: Some(true),
            exclude_task_status: vec![TaskStatus::Completed],
            tag: Some("customer:acme".into()),
            metadata: Some(HashMap::from([("flow".into(), "checkout".into())])),
            search: Some("double".into()),
            ..Default::default()
        })
        .collect()
        .await;

    let issue = found[0].as_ref().unwrap();
    assert_eq!(issue.id, id(ISSUE_ID));
    assert_eq!(issue.display_index, Some(42));
    assert_eq!(issue.priority, IssuePriority::High);
    assert_eq!(issue.status, IssueStatus::Waiting);
    assert_eq!(issue.revision, 3);
    assert_eq!(issue.task_id, Some(id(TASK_ID)));
    assert_eq!(issue.task_status, Some(TaskStatus::Running));
    assert_eq!(issue.member_id, None);
    assert_eq!(issue.metadata["severity"], json!(2));
    assert_eq!(issue.files[0].name.as_deref(), Some("trace.json"));
    assert_eq!(issue.spans[0].span_id, "0123456789abcdef");
    assert_eq!(issue.open_requests[0].assignee_id, id(MEMBER_ID));

    let requests = server.received_requests().await.unwrap();
    let mut pairs = query_pairs(&requests[0]);
    pairs.sort();
    let mut expected: Vec<(String, String)> = [
        ("assigned_to_me", "true"),
        ("exclude_task_status", "completed"),
        ("limit", "20"),
        ("metadata", "flow:checkout"),
        ("owner", "me"),
        ("search", "double"),
        ("status", "open"),
        ("status", "waiting"),
        ("tag", "customer:acme"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    expected.sort();
    assert_eq!(pairs, expected);
}

#[tokio::test]
async fn an_unset_list_sends_no_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [],
            "count": 0,
            "next": null,
        })))
        .mount(&server)
        .await;

    let _: Vec<_> = Issues::new(build_http(&server))
        .list(&IssueListParams::default())
        .collect()
        .await;

    let requests = server.received_requests().await.unwrap();
    assert!(query_pairs(&requests[0]).is_empty());
}

#[tokio::test]
async fn create_sends_the_brief_and_omits_empty_evidence() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/issues"))
        .and(body_json(json!({
            "title": "Checkout retries double-charge",
            "description": "Customers on retry see two charges.",
            "task_id": TASK_ID,
            "priority": "urgent",
            "tags": ["customer:acme"],
            "links": [{"url": "https://status.example.com/1", "title": "Status"}],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let issue = Issues::new(build_http(&server))
        .create(&IssueCreate {
            priority: Some(IssuePriority::Urgent),
            tags: vec!["customer:acme".into()],
            links: vec![IssueLink {
                url: "https://status.example.com/1".into(),
                title: Some("Status".into()),
            }],
            ..IssueCreate::new(
                "Checkout retries double-charge",
                "Customers on retry see two charges.",
                id(TASK_ID),
            )
        })
        .await
        .unwrap();
    assert_eq!(issue.id, id(ISSUE_ID));
}

#[tokio::test]
async fn get_reads_one_issue() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .mount(&server)
        .await;

    let issue = Issues::new(build_http(&server))
        .get(id(ISSUE_ID))
        .await
        .unwrap();
    assert_eq!(issue.title, "Checkout retries double-charge");
}

#[tokio::test]
async fn update_sends_only_the_fields_set_with_the_expected_revision() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(body_json(json!({
            "expected_revision": 3,
            "status": "closed",
            "metadata": {},
            "spans": [{"trace_id": "0123456789abcdef0123456789abcdef", "span_id": "0123456789abcdef"}],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    Issues::new(build_http(&server))
        .update(
            id(ISSUE_ID),
            &IssueUpdate {
                status: Some(IssueStatus::Closed),
                metadata: Some(HashMap::new()),
                spans: Some(vec![IssueSpanReference {
                    trace_id: "0123456789abcdef0123456789abcdef".into(),
                    span_id: "0123456789abcdef".into(),
                }]),
                ..IssueUpdate::new(3)
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn update_request_wraps_the_request_change() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(body_json(json!({
            "request": {
                "id": REQUEST_ID,
                "expected_revision": 1,
                "status": "resolved",
                "resolution": "Refunded; the retry path is fixed.",
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    Issues::new(build_http(&server))
        .update_request(
            id(ISSUE_ID),
            &IssueRequestUpdate {
                id: id(REQUEST_ID),
                expected_revision: 1,
                status: Some(IssueRequestStatus::Resolved),
                resolution: Some("Refunded; the retry path is fixed.".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn delete_is_a_bodyless_204_and_a_403_surfaces_as_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(204))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"detail": "Missing scope"})))
        .mount(&server)
        .await;

    let issues = Issues::new(build_http(&server));
    issues.delete(id(ISSUE_ID)).await.unwrap();
    match issues.delete(id(ISSUE_ID)).await {
        Err(IntrospectionAPIError::Http { status: 403, .. }) => {}
        other => panic!("expected a 403, got {other:?}"),
    }
}

#[test]
fn a_value_added_later_decodes_as_other() {
    let status: IssueStatus = serde_json::from_value(json!("triaged")).unwrap();
    assert_eq!(status, IssueStatus::Other("triaged".into()));
}
