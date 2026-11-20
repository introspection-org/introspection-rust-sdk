//! Integration tests for the issues DP surface (`client.issues()`), backed by
//! `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    IssueAttachment, IssueAttachmentCreate, IssueCreateParams, IssueEventReference, IssueFile,
    IssueInclude, IssueLink, IssueListParams, IssueMemberRelation, IssueOwner, IssuePriority,
    IssueSpanReference, IssueStatus, IssueUpdateParams, PaginationParams, TaskStatus,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ISSUE_ID: &str = "11111111-1111-7111-8111-111111111111";
const TASK_ID: &str = "22222222-2222-7222-8222-222222222222";
const MEMBER_ID: &str = "44444444-4444-7444-8444-444444444444";
const REVIEWER_ID: &str = "55555555-5555-7555-8555-555555555555";
const FILE_ID: &str = "66666666-6666-7666-8666-666666666666";
const EVENT_ID: &str = "77777777-7777-7777-8777-777777777777";
const LINK_ATTACHMENT_ID: &str = "99999999-9999-7999-8999-999999999991";
const FILE_ATTACHMENT_ID: &str = "99999999-9999-7999-8999-999999999992";
const EVENT_ATTACHMENT_ID: &str = "99999999-9999-7999-8999-999999999993";
const SPAN_ATTACHMENT_ID: &str = "99999999-9999-7999-8999-999999999994";

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
        "display_key": "I-7",
        "warnings": ["comments beyond the first 50 left out"],
        "title": "Clarify affiliation rules",
        "description": "Affiliate classifications are inconsistent.",
        "status": "waiting",
        "priority": "high",
        "revision": 3,
        "task_id": TASK_ID,
        "task_status": "idle",
        "member_id": null,
        "created_by_member_id": MEMBER_ID,
        "assignee_member_id": MEMBER_ID,
        "reviewer_member_id": null,
        "closed_at": null,
        "tags": ["customer:acme"],
        "metadata": {"source": "check-in", "attempts": 2},
        "members": null,
        "attachments": null,
        "comments": null,
    })
}

/// The issue with every list an `include` adds.
fn issue_with_lists() -> serde_json::Value {
    let mut issue = issue_json();
    issue["members"] = json!([
        {"member_id": MEMBER_ID, "relation": "creator"},
        {"member_id": MEMBER_ID, "relation": "assignee"},
        {"member_id": REVIEWER_ID, "relation": "follower"},
    ]);
    issue["attachments"] = json!([
        {"id": LINK_ATTACHMENT_ID, "kind": "link", "url": "https://github.com/acme/app/pull/12", "title": "Proposed fix"},
        {"id": FILE_ATTACHMENT_ID, "kind": "file", "file_id": FILE_ID, "name": "Examples", "checksum": null, "source_event_ids": []},
        {"id": EVENT_ATTACHMENT_ID, "kind": "event", "event_id": EVENT_ID},
        {"id": SPAN_ATTACHMENT_ID, "kind": "span", "trace_id": "0123456789abcdef0123456789abcdef", "span_id": "0123456789abcdef"},
    ]);
    issue["comments"] = json!([comment_json()]);
    issue
}

fn comment_json() -> serde_json::Value {
    json!({
        "id": "88888888-8888-7888-8888-888888888888",
        "issue_id": ISSUE_ID,
        "comment": "Is a missing salary allowed?",
        "member_id": MEMBER_ID,
        "actor_member_id": REVIEWER_ID,
        "actor_member_type": "agent",
        "created_at": "2026-11-04T00:30:00Z",
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
            "records": [issue_with_lists()],
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
        relation: Some(vec![
            IssueMemberRelation::Assignee,
            IssueMemberRelation::Reviewer,
        ]),
        member_id: Some(id(MEMBER_ID)),
        include: Some(vec![IssueInclude::Members, IssueInclude::Attachments]),
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
    assert_eq!(first.display_key.as_deref(), Some("I-7"));
    assert_eq!(
        first.warnings.as_deref(),
        Some(&["comments beyond the first 50 left out".to_string()][..])
    );
    assert_eq!(first.task_status, Some(TaskStatus::Idle));
    assert_eq!(first.metadata["attempts"], json!(2));
    assert_eq!(first.created_by_member_id, Some(id(MEMBER_ID)));
    assert_eq!(first.assignee_member_id, Some(id(MEMBER_ID)));
    assert_eq!(first.reviewer_member_id, None);
    let attachments = first
        .attachments
        .as_deref()
        .expect("the included attachments");
    assert_eq!(
        attachments[0],
        IssueAttachment::Link {
            id: id(LINK_ATTACHMENT_ID),
            url: "https://github.com/acme/app/pull/12".into(),
            title: Some("Proposed fix".into()),
        }
    );
    assert_eq!(
        attachments[3],
        IssueAttachment::Span {
            id: id(SPAN_ATTACHMENT_ID),
            trace_id: "0123456789abcdef0123456789abcdef".into(),
            span_id: "0123456789abcdef".into(),
        }
    );
    let members = first.members.as_deref().expect("the included members");
    assert_eq!(members[2].member_id, id(REVIEWER_ID));
    assert_eq!(members[2].relation, IssueMemberRelation::Follower);

    let received = server.received_requests().await.unwrap();
    let mut pairs = query_pairs(&received[0]);
    pairs.sort();
    let mut expected: Vec<(String, String)> = [
        ("exclude_task_status", "completed"),
        ("exclude_task_status", "failed"),
        ("include", "attachments"),
        ("include", "members"),
        ("include_total", "true"),
        ("member_id", MEMBER_ID),
        ("limit", "20"),
        ("metadata", "customer:acme"),
        ("metadata", "source:check-in"),
        ("owner", "project"),
        ("relation", "assignee"),
        ("relation", "reviewer"),
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
async fn get_without_an_include_reads_the_brief_alone() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .mount(&server)
        .await;

    let issue = build_http(&server)
        .issues()
        .get(id(ISSUE_ID), &[])
        .await
        .unwrap();
    assert_eq!(issue.title, "Clarify affiliation rules");
    assert_eq!(issue.task_id, Some(id(TASK_ID)));
    assert!(issue.members.is_none() && issue.attachments.is_none() && issue.comments.is_none());
    let received = server.received_requests().await.unwrap();
    assert!(query_pairs(&received[0]).is_empty());
}

#[tokio::test]
async fn get_asks_for_each_include_and_reads_its_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_with_lists()))
        .mount(&server)
        .await;

    let issue = build_http(&server)
        .issues()
        .get(
            id(ISSUE_ID),
            &[
                IssueInclude::Members,
                IssueInclude::Attachments,
                IssueInclude::Comments,
            ],
        )
        .await
        .unwrap();

    let received = server.received_requests().await.unwrap();
    let sent: Vec<_> = query_pairs(&received[0])
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    assert_eq!(
        sent,
        ["include=members", "include=attachments", "include=comments"]
    );
    assert_eq!(issue.members.as_deref().unwrap().len(), 3);
    let attachments = issue.attachments.as_deref().unwrap();
    assert_eq!(
        attachments[1],
        IssueAttachment::File {
            id: id(FILE_ATTACHMENT_ID),
            file_id: id(FILE_ID),
            name: Some("Examples".into()),
            checksum: None,
            source_event_ids: Vec::new(),
        }
    );
    assert_eq!(
        attachments[2],
        IssueAttachment::Event {
            id: id(EVENT_ATTACHMENT_ID),
            event_id: id(EVENT_ID),
        }
    );
    let comments = issue.comments.as_deref().expect("the included comments");
    assert_eq!(comments[0].comment, "Is a missing salary allowed?");
    assert_eq!(comments[0].member_id, Some(id(MEMBER_ID)));
    assert_eq!(comments[0].actor_member_type.as_deref(), Some("agent"));
}

#[tokio::test]
async fn a_relation_this_build_does_not_know_still_decodes() {
    let mut issue = issue_json();
    issue["members"] = json!([{"member_id": MEMBER_ID, "relation": "approver"}]);
    let issue: introspection_sdk::Issue = serde_json::from_value(issue).unwrap();
    assert_eq!(
        issue.members.unwrap()[0].relation,
        IssueMemberRelation::Unknown(json!("approver"))
    );
}

#[tokio::test]
async fn comments_create_posts_the_comment_and_reads_it_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/issues/{ISSUE_ID}/comments")))
        .and(body_json(
            json!({"comment": "Is a missing salary allowed?"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(comment_json()))
        .expect(1)
        .mount(&server)
        .await;

    let comment = build_http(&server)
        .issues()
        .comments
        .create(id(ISSUE_ID), "Is a missing salary allowed?")
        .await
        .unwrap();
    assert_eq!(comment.issue_id, id(ISSUE_ID));
    assert_eq!(comment.actor_member_id, Some(id(REVIEWER_ID)));
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
            "assignee_member_id": MEMBER_ID,
            "follower_member_ids": [REVIEWER_ID],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = IssueCreateParams {
        priority: Some(IssuePriority::Urgent),
        tags: Some(vec!["customer:acme".into()]),
        assignee_member_id: Some(id(MEMBER_ID)),
        follower_member_ids: Some(vec![id(REVIEWER_ID)]),
        ..IssueCreateParams::new("Clarify affiliation rules", "Agree the affiliate standard.")
            .with_task_id(id(TASK_ID))
    };
    let issue = build_http(&server)
        .issues()
        .create(&params, &[IssueInclude::Members])
        .await
        .unwrap();
    assert_eq!(issue.id, id(ISSUE_ID));
    let received = server.received_requests().await.unwrap();
    assert_eq!(
        query_pairs(&received[0]),
        [("include".to_string(), "members".to_string())]
    );
}

#[tokio::test]
async fn create_without_a_task_omits_task_id_and_decodes_a_taskless_issue() {
    let server = MockServer::start().await;
    let mut taskless = issue_json();
    taskless["task_id"] = serde_json::Value::Null;
    taskless["task_status"] = serde_json::Value::Null;
    Mock::given(method("POST"))
        .and(path("/v1/issues"))
        .and(body_json(json!({
            "title": "Clarify affiliation rules",
            "description": "Agree the affiliate standard.",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(taskless))
        .expect(1)
        .mount(&server)
        .await;

    let params =
        IssueCreateParams::new("Clarify affiliation rules", "Agree the affiliate standard.");
    assert_eq!(params.task_id, None);
    let issue = build_http(&server)
        .issues()
        .create(&params, &[])
        .await
        .unwrap();
    assert_eq!(issue.id, id(ISSUE_ID));
    assert_eq!(issue.task_id, None);
    assert_eq!(issue.task_status, None);
    let received = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    assert!(body.get("task_id").is_none());
}

#[tokio::test]
async fn update_sends_null_to_clear_a_person_and_omits_one_left_alone() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(body_json(json!({
            "status": "waiting",
            "assignee_member_id": MEMBER_ID,
            "reviewer_member_id": null,
            "follower_member_ids": [],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_with_lists()))
        .expect(1)
        .mount(&server)
        .await;

    let update = IssueUpdateParams {
        status: Some(IssueStatus::Waiting),
        assignee_member_id: Some(Some(id(MEMBER_ID))),
        reviewer_member_id: Some(None),
        follower_member_ids: Some(Vec::new()),
        ..IssueUpdateParams::default()
    };
    let issue = build_http(&server)
        .issues()
        .update(
            id(ISSUE_ID),
            &update,
            &[IssueInclude::Members, IssueInclude::Attachments],
            None,
        )
        .await
        .unwrap();
    assert!(issue.members.is_some());

    let received = server.received_requests().await.unwrap();
    assert_eq!(query_pairs(&received[0]).len(), 2);
    // Neither person named: neither key is sent.
    let untouched = serde_json::to_value(IssueUpdateParams {
        title: Some("Renamed".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(untouched, json!({"title": "Renamed"}));
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
            &IssueUpdateParams {
                title: Some("Renamed".into()),
                ..Default::default()
            },
            &[],
            Some(tag),
        )
        .await
        .unwrap_err();
    assert_eq!(stale.status(), Some(412));
    issues.delete(id(ISSUE_ID), Some(tag)).await.unwrap();
}

#[test]
fn an_issue_etag_is_its_updated_at_as_rendered() {
    let mut issue: introspection_sdk::Issue = serde_json::from_value(issue_json()).unwrap();
    issue.updated_at = "2026-11-04T01:00:00.123456Z".into();
    assert_eq!(issue.etag(), "\"2026-11-04T01:00:00.123456Z\"");
}

#[tokio::test]
async fn list_filters_by_display_key() {
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

    let params = IssueListParams {
        display_key: Some("I-7".into()),
        ..Default::default()
    };
    let issue = build_http(&server)
        .issues()
        .list(&params)
        .next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(issue.display_key.as_deref(), Some("I-7"));

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        query_pairs(&received[0]),
        vec![("display_key".to_string(), "I-7".to_string())]
    );
}

#[tokio::test]
async fn an_issue_without_a_display_index_or_warnings_decodes() {
    let mut body = issue_json();
    body["display_index"] = serde_json::Value::Null;
    body.as_object_mut().unwrap().remove("display_key");
    body.as_object_mut().unwrap().remove("warnings");
    let issue: introspection_sdk::Issue = serde_json::from_value(body).unwrap();
    assert!(issue.display_key.is_none());
    assert!(issue.warnings.is_none());
}

#[test]
fn an_attachment_kind_this_build_does_not_know_still_decodes() {
    let unknown = json!({
        "id": LINK_ATTACHMENT_ID,
        "kind": "commit",
        "sha": "0123456789abcdef0123456789abcdef01234567",
    });
    let mut body = issue_json();
    body["attachments"] = json!([unknown.clone()]);
    let issue: introspection_sdk::Issue = serde_json::from_value(body).unwrap();
    let attachment = &issue.attachments.as_deref().unwrap()[0];
    assert_eq!(attachment, &IssueAttachment::Unknown(unknown.clone()));
    assert_eq!(attachment.id(), Some(id(LINK_ATTACHMENT_ID)));
    assert_eq!(serde_json::to_value(attachment).unwrap(), unknown);
}

#[tokio::test]
async fn create_sends_attachments_tagged_on_kind() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/issues"))
        .and(body_json(json!({
            "title": "Clarify affiliation rules",
            "description": "Affiliate classifications are inconsistent.",
            "task_id": TASK_ID,
            "attachments": [
                {"kind": "link", "url": "https://github.com/acme/app/pull/12", "title": "Fix"},
                {"kind": "file", "file_id": FILE_ID},
                {"kind": "event", "event_id": EVENT_ID},
                {
                    "kind": "span",
                    "trace_id": "0af7651916cd43dd8448eb211c80319c",
                    "span_id": "b7ad6b7169203331",
                },
            ],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = IssueCreateParams {
        attachments: Some(vec![
            IssueLink {
                url: "https://github.com/acme/app/pull/12".into(),
                title: Some("Fix".into()),
            }
            .into(),
            IssueFile::new(id(FILE_ID)).into(),
            IssueAttachmentCreate::Event(IssueEventReference {
                event_id: id(EVENT_ID),
            }),
            IssueSpanReference {
                trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
                span_id: "b7ad6b7169203331".into(),
            }
            .into(),
        ]),
        ..IssueCreateParams::new(
            "Clarify affiliation rules",
            "Affiliate classifications are inconsistent.",
        )
        .with_task_id(id(TASK_ID))
    };
    build_http(&server)
        .issues()
        .create(&params, &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn update_adds_and_removes_attachments_without_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/issues/{ISSUE_ID}")))
        .and(body_json(json!({
            "add_attachments": [
                {"kind": "link", "url": "https://github.com/acme/app/pull/12"},
            ],
            "remove_attachments": [FILE_ATTACHMENT_ID],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json()))
        .expect(1)
        .mount(&server)
        .await;

    let update = IssueUpdateParams {
        add_attachments: Some(vec![IssueLink {
            url: "https://github.com/acme/app/pull/12".into(),
            title: None,
        }
        .into()]),
        remove_attachments: Some(vec![id(FILE_ATTACHMENT_ID)]),
        ..IssueUpdateParams::default()
    };
    build_http(&server)
        .issues()
        .update(id(ISSUE_ID), &update, &[], None)
        .await
        .unwrap();

    let received = server.received_requests().await.unwrap();
    assert_eq!(if_match(&received[0]), None);
}

#[test]
fn unset_attachment_fields_are_omitted() {
    assert_eq!(
        serde_json::to_value(IssueUpdateParams::default()).unwrap(),
        json!({})
    );
    let create =
        serde_json::to_value(IssueCreateParams::new("t", "d").with_task_id(id(TASK_ID))).unwrap();
    assert!(create.get("attachments").is_none());
}
