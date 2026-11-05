//! Integration tests for the issues DP surface (`client.issues()`), backed by
//! `wiremock`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    IssueCreateParams, IssueLink, IssueListParams, IssueOwner, IssuePriority, IssueRequestMutation,
    IssueRequestStatus, IssueSpanReference, IssueStatus, IssueUpdateParams, PaginationParams,
    TaskStatus,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path};
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
        max_retries: 2,
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
        "created_at": "2026-11-04T00:00:00Z",
        "updated_at": "2026-11-04T01:00:00Z",
        "display_index": 7,
        "title": "Clarify affiliation rules",
        "description": "Affiliate classifications are inconsistent.",
        "status": "waiting",
        "priority": "high",
        "revision": 3,
        "task_id": TASK_ID,
        "task_status": "idle",
        "member_id": null,
        "closed_at": null,
        "tags": ["customer:acme"],
        "metadata": {"source": "check-in", "attempts": 2},
        "files": [{"file_id": "66666666-6666-7666-8666-666666666666", "name": "Examples", "checksum": null, "source_event_ids": []}],
        "links": [{"url": "https://github.com/acme/app/pull/12", "title": "Proposed fix"}],
        "events": [{"event_id": "77777777-7777-7777-8777-777777777777"}],
        "spans": [{"trace_id": "0123456789abcdef0123456789abcdef", "span_id": "0123456789abcdef"}],
        "open_requests": [{
            "id": REQUEST_ID,
            "question": "Is a missing salary allowed?",
            "assignee_id": MEMBER_ID,
            "created_at": "2026-11-04T00:30:00Z",
        }],
    })
}

fn query_pairs(request: &Request) -> Vec<(String, String)> {
    request
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn if_match(request: &Request) -> Option<String> {
    request
        .headers
        .get("If-Match")
        .map(|value| value.to_str().unwrap().to_string())
}

#[tokio::test]
async fn list_sends_repeated_filters_and_decodes_every_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [issue_json()],
            "count": 1,
            "total_count": 1,
            "next": null,
        })))
        .mount(&server)
        .await;

    let issues = build_http(&server).issues();
    let params = IssueListParams {
        pagination: PaginationParams {
            limit: Some(20),
            next: None,
        },
        status: Some(vec![IssueStatus::Open, IssueStatus::Waiting]),
        owner: Some(vec![IssueOwner::Project]),
        has_open_requests: Some(true),
        exclude_task_status: Some(vec![TaskStatus::Completed, TaskStatus::Failed]),
        metadata: Some(vec!["source:check-in".into(), "customer:acme".into()]),
        include_total: Some(true),
        ..Default::default()
    };
    let mut page = issues.list(&params);
    let first = page.next().await.unwrap().unwrap();
    assert!(page.next().await.is_none());

    assert_eq!(first.id, id(ISSUE_ID));
    assert_eq!(first.status, IssueStatus::Waiting);
    assert_eq!(first.priority, IssuePriority::High);
    assert_eq!(first.revision, 3);
    assert_eq!(first.display_index, Some(7));
    assert_eq!(first.task_status, Some(TaskStatus::Idle));
    assert_eq!(first.metadata["attempts"], json!(2));
    assert_eq!(first.links[0].title.as_deref(), Some("Proposed fix"));
    assert_eq!(first.spans[0].span_id, "0123456789abcdef");
    assert_eq!(first.open_requests[0].assignee_id, id(MEMBER_ID));

    let received = server.received_requests().await.unwrap();
    let mut pairs = query_pairs(&received[0]);
    pairs.sort();
    let mut expected: Vec<(String, String)> = [
        ("exclude_task_status", "completed"),
        ("exclude_task_status", "failed"),
        ("has_open_requests", "true"),
        ("include_total", "true"),
        ("limit", "20"),
        ("metadata", "customer:acme"),
        ("metadata", "source:check-in"),
        ("owner", "project"),
        ("status", "open"),
        ("status", "waiting"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    expected.sort();
    assert_eq!(pairs, expected);
}

#[tokio::test]
async fn get_reads_one_issue() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .mount(&server)
        .await;

    let issue = build_http(&server)
        .issues()
        .get(id(ISSUE_ID))
        .await
        .unwrap();
    assert_eq!(issue.title, "Clarify affiliation rules");
    assert_eq!(issue.task_id, Some(id(TASK_ID)));
}

#[tokio::test]
async fn create_sends_only_the_fields_set() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/issues"))
        .and(body_json(json!({
            "title": "Clarify affiliation rules",
            "description": "Agree the affiliate standard.",
            "task_id": TASK_ID,
            "priority": "urgent",
            "tags": ["customer:acme"],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = IssueCreateParams {
        priority: Some(IssuePriority::Urgent),
        tags: Some(vec!["customer:acme".into()]),
        ..IssueCreateParams::new(
            "Clarify affiliation rules",
            "Agree the affiliate standard.",
            id(TASK_ID),
        )
    };
    let issue = build_http(&server).issues().create(&params).await.unwrap();
    assert_eq!(issue.id, id(ISSUE_ID));
}

#[tokio::test]
async fn update_omits_unset_fields_and_sends_an_empty_list_to_clear_one() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(body_json(json!({
            "status": "closed",
            "links": [{"url": "https://github.com/acme/app/pull/12"}],
            "spans": [],
            "metadata": {},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let update = IssueUpdateParams {
        status: Some(IssueStatus::Closed),
        links: Some(vec![IssueLink {
            url: "https://github.com/acme/app/pull/12".into(),
            title: None,
        }]),
        spans: Some(Vec::<IssueSpanReference>::new()),
        metadata: Some(BTreeMap::new()),
        ..IssueUpdateParams::default()
    };
    build_http(&server)
        .issues()
        .update(id(ISSUE_ID), update, None)
        .await
        .unwrap();

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        if_match(&received[0]),
        None,
        "no If-Match unless one is given"
    );
}

#[tokio::test]
async fn update_with_a_request_mutation_sends_the_request_body() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(body_json(json!({
            "request": {
                "id": REQUEST_ID,
                "status": "resolved",
                "resolution": "Missing salary is allowed.",
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let change = IssueRequestMutation {
        status: Some(IssueRequestStatus::Resolved),
        resolution: Some("Missing salary is allowed.".into()),
        ..IssueRequestMutation::change(id(REQUEST_ID))
    };
    build_http(&server)
        .issues()
        .update(id(ISSUE_ID), change, None)
        .await
        .unwrap();
}

#[test]
fn a_new_request_has_a_fresh_uuid7() {
    let request = IssueRequestMutation::create("Is a missing salary allowed?", id(MEMBER_ID));
    assert_eq!(request.id.get_version_num(), 7);
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(
        wire,
        json!({
            "id": request.id.to_string(),
            "question": "Is a missing salary allowed?",
            "assignee_id": MEMBER_ID,
        })
    );
}

#[tokio::test]
async fn a_conditional_edit_and_delete_send_if_match_and_surface_412() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(header("If-Match", "\"2026-11-04T01:00:00.123456Z\""))
        .respond_with(
            ResponseTemplate::new(412)
                .set_body_json(json!({"detail": "Issue changed; read it again"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(header("If-Match", "\"2026-11-04T01:00:00.123456Z\""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let issues = build_http(&server).issues();
    let tag = "\"2026-11-04T01:00:00.123456Z\"";

    let stale = issues
        .update(
            id(ISSUE_ID),
            IssueUpdateParams {
                title: Some("Renamed".into()),
                ..Default::default()
            },
            Some(tag),
        )
        .await
        .unwrap_err();
    assert_eq!(stale.status(), Some(412));
    issues.delete(id(ISSUE_ID), Some(tag)).await.unwrap();

    let refused = issues
        .update(
            id(ISSUE_ID),
            IssueRequestMutation::change(id(REQUEST_ID)),
            Some(tag),
        )
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("If-Match"), "{refused}");
}

#[test]
fn an_issue_etag_is_its_updated_at_as_rendered() {
    let mut issue: introspection_sdk::Issue = serde_json::from_value(issue_json()).unwrap();
    issue.updated_at = "2026-11-04T01:00:00.123456Z".into();
    assert_eq!(issue.etag(), "\"2026-11-04T01:00:00.123456Z\"");
}
