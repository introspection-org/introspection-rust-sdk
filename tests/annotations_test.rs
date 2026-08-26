use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    AdvancedOptions, AnnotationEventOptions, AnnotationListParams, AnnotationMutation,
    AnnotationTarget, Annotations, ClientConfig, Event, EventListParams, IntrospectionClient,
    IntrospectionEventName, ProjectLabelCreate, ProjectLabelUpdate, ProjectLabels,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TRACE_ID: &str = "0af7651916cd43dd8448eb211c80319c";
const SPAN_ID: &str = "b7ad6b7169203331";
const MEMBER_ID: &str = "00000000-0000-0000-0000-0000000000cc";
const EVENT_ID: &str = "019fc000-0000-7000-8000-000000000001";

fn http(server: &MockServer) -> Arc<HttpClient> {
    Arc::new(HttpClient::from_parts(
        reqwest::Client::new(),
        HttpConfig {
            api_url: server.uri(),
            token: "intro_test".into(),
            additional_headers: HashMap::new(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            retry_base: Duration::from_millis(1),
        },
    ))
}

fn annotation() -> serde_json::Value {
    json!({
        "trace_id": TRACE_ID,
        "span_id": SPAN_ID,
        "conversation_id": "conversation-1",
        "labels": ["needs-review"],
        "assignee_member_ids": [MEMBER_ID],
        "annotator_member_ids": [],
        "has_comment": false,
        "comment_count": 0,
        "latest_comment": null,
        "latest_comment_member_id": null,
        "updated_at": "2026-08-25T12:00:00Z",
        "updated_by_member_id": MEMBER_ID,
        "assignment_event_id": EVENT_ID
    })
}

fn label(description: serde_json::Value) -> serde_json::Value {
    json!({
        "slug": "needs-review",
        "color": "#f97316",
        "description": description,
        "created_at": "2026-08-25T12:00:00Z",
        "updated_at": "2026-08-25T12:00:00Z"
    })
}

#[tokio::test]
async fn lists_folded_annotation_state_with_filters() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations"))
        .and(query_param("label", "needs-review"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [annotation()], "count": 1, "total_count": 1, "next": null
        })))
        .mount(&dp)
        .await;
    let annotations = Annotations::new(http(&dp), http(&dp));
    let mut stream = annotations.list(&AnnotationListParams {
        label: Some("needs-review".into()),
        ..Default::default()
    });
    assert_eq!(
        stream.next().await.unwrap().unwrap().labels,
        vec!["needs-review"]
    );
}

#[tokio::test]
async fn resolves_email_list_filter_and_requests_total() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{"id": MEMBER_ID, "email": "expert@example.com", "is_deactivated": false}],
            "count": 1, "next": null
        })))
        .expect(1)
        .mount(&cp)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations"))
        .and(query_param("assignee_member_id", MEMBER_ID))
        .and(query_param("include_total", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [annotation()], "count": 1, "total_count": 1, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let annotations = Annotations::new(http(&cp), http(&dp));
    let mut page = annotations
        .list_by_email(
            AnnotationListParams {
                include_total: Some(true),
                ..Default::default()
            },
            None,
            Some("expert@example.com".into()),
        )
        .await
        .unwrap();
    assert_eq!(
        page.next_page().await.unwrap().unwrap().total_count,
        Some(1)
    );
}

#[tokio::test]
async fn top_level_events_decode_annotation_payload_on_the_data_plane() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param("event_name", "introspection.annotation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{
                "id": EVENT_ID,
                "timestamp": "2026-08-25T12:00:00Z",
                "event_name": "introspection.annotation",
                "trace_id": TRACE_ID,
                "span_id": SPAN_ID,
                "payload": {"member_id": MEMBER_ID, "comment": "Strong evidence"}
            }],
            "count": 1, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let client = IntrospectionClient::new(ClientConfig::with_token("intro_test").advanced(
        AdvancedOptions {
            base_api_url: Some(cp.uri()),
            dp_url: Some(dp.uri()),
            ..Default::default()
        },
    ))
    .unwrap();
    let mut events = client
        .events()
        .list(&EventListParams::new(IntrospectionEventName::Annotation))
        .unwrap();
    let event = events.next().await.unwrap().unwrap();
    let Event::Annotation(annotation) = event else {
        panic!("expected annotation event")
    };
    assert_eq!(
        annotation.payload.comment.as_deref(),
        Some("Strong evidence")
    );
}

#[tokio::test]
async fn appends_one_label_snapshot_with_stable_event_id() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .and(body_json(json!({
            "trace_id": TRACE_ID, "span_id": SPAN_ID, "event_id": EVENT_ID, "labels": []
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&dp)
        .await;
    Annotations::new(http(&dp), http(&dp))
        .create(
            AnnotationTarget {
                trace_id: TRACE_ID.into(),
                span_id: SPAN_ID.into(),
            },
            AnnotationMutation::Labels(vec![]),
            AnnotationEventOptions {
                event_id: Some(Uuid::parse_str(EVENT_ID).unwrap()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn resolves_reviewer_email_before_single_dp_write() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/members"))
        .and(query_param("member_type", "business"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{"id": MEMBER_ID, "email": "Expert@Example.com", "is_deactivated": false}],
            "count": 1, "next": null
        })))
        .mount(&cp)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .and(body_json(json!({
            "trace_id": TRACE_ID, "span_id": SPAN_ID, "event_id": EVENT_ID,
            "assignee_member_ids": [MEMBER_ID]
        })))
        .respond_with(ResponseTemplate::new(204))
        .mount(&dp)
        .await;
    Annotations::new(http(&cp), http(&dp))
        .create(
            AnnotationTarget {
                trace_id: TRACE_ID.into(),
                span_id: SPAN_ID.into(),
            },
            AnnotationMutation::ReviewerEmails(vec![" expert@example.com ".into()]),
            AnnotationEventOptions {
                event_id: Some(Uuid::parse_str(EVENT_ID).unwrap()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn validates_labels_and_only_updates_description() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/project-labels"))
        .and(body_json(
            json!({"slug": "needs-review", "color": "#f97316", "description": "Queue"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(label(json!("Queue"))))
        .mount(&dp)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/v1/project-labels/needs-review"))
        .and(body_json(json!({"description": null})))
        .respond_with(ResponseTemplate::new(200).set_body_json(label(serde_json::Value::Null)))
        .mount(&dp)
        .await;
    let labels = ProjectLabels::new(http(&dp));
    let created = labels
        .create(ProjectLabelCreate {
            slug: " needs-review ".into(),
            color: "#F97316".into(),
            description: Some("Queue".into()),
        })
        .await
        .unwrap();
    assert_eq!(created.slug, "needs-review");
    assert!(labels
        .create(ProjectLabelCreate {
            slug: "bad".into(),
            color: "orange".into(),
            description: None
        })
        .await
        .is_err());
    assert!(labels
        .update("needs-review", ProjectLabelUpdate { description: None })
        .await
        .unwrap()
        .description
        .is_none());
}

#[tokio::test]
async fn client_routes_annotations_to_the_configured_data_plane() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [], "count": 0, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let client = IntrospectionClient::new(ClientConfig::with_token("intro_test").advanced(
        AdvancedOptions {
            base_api_url: Some(cp.uri()),
            dp_url: Some(dp.uri()),
            ..Default::default()
        },
    ))
    .unwrap();
    let mut page = client.annotations().list(&AnnotationListParams::default());
    assert!(page.next_page().await.unwrap().unwrap().records.is_empty());
}

#[tokio::test]
async fn member_session_is_used_only_for_reviewer_resolution() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/members"))
        .and(header("cookie", "intro_cp_session=encoded-session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{
                "id": MEMBER_ID,
                "email": "expert@example.com",
                "is_deactivated": false
            }],
            "count": 1,
            "next": null
        })))
        .expect(1)
        .mount(&cp)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .and(header("authorization", "Bearer member-token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&dp)
        .await;
    let client = IntrospectionClient::new(ClientConfig::with_token("member-token").advanced(
        AdvancedOptions {
            base_api_url: Some(cp.uri()),
            dp_url: Some(dp.uri()),
            cp_session: Some("encoded-session".into()),
            ..Default::default()
        },
    ))
    .unwrap();

    client
        .annotations()
        .create(
            AnnotationTarget {
                trace_id: TRACE_ID.into(),
                span_id: SPAN_ID.into(),
            },
            AnnotationMutation::ReviewerEmails(vec!["expert@example.com".into()]),
            AnnotationEventOptions::default(),
        )
        .await
        .unwrap();

    let cp_requests = cp.received_requests().await.unwrap();
    assert!(!cp_requests[0].headers.contains_key("authorization"));
    let dp_requests = dp.received_requests().await.unwrap();
    assert!(!dp_requests[0].headers.contains_key("cookie"));
}
