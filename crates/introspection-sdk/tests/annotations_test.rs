use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    AdvancedOptions, AnnotationCreated, AnnotationEventOptions, AnnotationListParams,
    AnnotationMutation, AnnotationTagCreate, AnnotationTarget, Annotations, ClientConfig, Event,
    EventListParams, IntrospectionClient, IntrospectionEventName, TagCreate, TagKind, TagUpdate,
    Tags,
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

fn retrying_http(server: &MockServer) -> Arc<HttpClient> {
    Arc::new(HttpClient::from_parts(
        reqwest::Client::new(),
        HttpConfig {
            api_url: server.uri(),
            token: "intro_test".into(),
            additional_headers: HashMap::new(),
            timeout: Duration::from_secs(5),
            max_retries: 2,
            retry_base: Duration::from_millis(1),
        },
    ))
}

fn target() -> AnnotationTarget {
    AnnotationTarget {
        trace_id: TRACE_ID.into(),
        span_id: SPAN_ID.into(),
    }
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

fn tag(kind: &str, color: serde_json::Value, description: serde_json::Value) -> serde_json::Value {
    json!({
        "kind": kind,
        "name": "needs-review",
        "color": color,
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
    let mut stream = annotations
        .list(&AnnotationListParams {
            label: Some("needs-review".into()),
            ..Default::default()
        })
        .await
        .unwrap();
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
        .list(&AnnotationListParams {
            include_total: Some(true),
            assigned_to_email: Some("expert@example.com".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        page.next_page().await.unwrap().unwrap().total_count,
        Some(1)
    );
}

#[tokio::test]
async fn refuses_a_member_id_and_email_filter_naming_the_same_person() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    let annotations = Annotations::new(http(&cp), http(&dp));

    let annotator = match annotations
        .list(&AnnotationListParams {
            annotated_by_member_id: Some(Uuid::nil()),
            annotated_by_email: Some("expert@example.com".into()),
            ..Default::default()
        })
        .await
    {
        Ok(_) => panic!("expected the conflicting annotator filters to be refused"),
        Err(err) => err,
    };
    assert!(annotator.to_string().contains("annotated_by_member_id"));

    let assignee = match annotations
        .list(&AnnotationListParams {
            assignee_member_id: Some(Uuid::nil()),
            assigned_to_email: Some("expert@example.com".into()),
            ..Default::default()
        })
        .await
    {
        Ok(_) => panic!("expected the conflicting assignee filters to be refused"),
        Err(err) => err,
    };
    assert!(assignee.to_string().contains("assignee_member_id"));

    // Refused before either mock server is asked anything.
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
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_comment_rides_along_with_a_label_snapshot_in_one_event() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .and(body_json(json!({
            "trace_id": TRACE_ID, "span_id": SPAN_ID, "event_id": EVENT_ID,
            "labels": ["needs-review"], "comment": "Wrong tool picked"
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&dp)
        .await;
    Annotations::new(http(&dp), http(&dp))
        .create(
            target(),
            AnnotationMutation::Labels(vec!["needs-review".into()]),
            AnnotationEventOptions {
                event_id: Some(Uuid::parse_str(EVENT_ID).unwrap()),
                comment: Some("Wrong tool picked".into()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_comment_rides_along_with_a_reviewer_snapshot_in_one_event() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .and(body_json(json!({
            "trace_id": TRACE_ID, "span_id": SPAN_ID, "event_id": EVENT_ID,
            "assignee_member_ids": [], "comment": "Unassigning"
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&dp)
        .await;
    Annotations::new(http(&dp), http(&dp))
        .create(
            target(),
            AnnotationMutation::ReviewerEmails(vec![]),
            AnnotationEventOptions {
                event_id: Some(Uuid::parse_str(EVENT_ID).unwrap()),
                comment: Some("Unassigning".into()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn two_comments_on_one_event_are_refused_before_any_request() {
    let dp = MockServer::start().await;
    let error = Annotations::new(http(&dp), http(&dp))
        .create(
            target(),
            AnnotationMutation::Comment("one".into()),
            AnnotationEventOptions {
                comment: Some("two".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some("conflicting_annotation_comments"));
    assert!(dp.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_gateway_error_is_retried_with_the_same_event_id() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&dp)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&dp)
        .await;
    Annotations::new(retrying_http(&dp), retrying_http(&dp))
        .create(
            target(),
            AnnotationMutation::Comment("Looks right".into()),
            AnnotationEventOptions::default(),
        )
        .await
        .unwrap();

    let requests = dp.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let event_ids: Vec<serde_json::Value> = requests
        .iter()
        .map(|request| request.body_json::<serde_json::Value>().unwrap()["event_id"].clone())
        .collect();
    assert_eq!(event_ids[0], event_ids[1]);
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
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn reviewer_ids_go_out_as_sent_on_a_data_plane_client() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .and(body_json(json!({
            "trace_id": TRACE_ID, "span_id": SPAN_ID, "event_id": EVENT_ID,
            "assignee_member_ids": [MEMBER_ID], "comment": "please review"
        })))
        .respond_with(
            ResponseTemplate::new(204).insert_header("x-introspection-event-id", EVENT_ID),
        )
        .expect(1)
        .mount(&dp)
        .await;
    let created = Annotations::data_plane(http(&dp))
        .create(
            target(),
            AnnotationMutation::ReviewerIds(vec![Uuid::parse_str(MEMBER_ID).unwrap()]),
            AnnotationEventOptions {
                event_id: Some(Uuid::parse_str(EVENT_ID).unwrap()),
                comment: Some("please review".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        created,
        AnnotationCreated {
            event_id: Uuid::parse_str(EVENT_ID).unwrap(),
            visible: true,
        }
    );
}

#[tokio::test]
async fn an_accepted_but_not_yet_visible_event_reads_back_from_the_202_body() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .respond_with(
            ResponseTemplate::new(202)
                .insert_header("x-introspection-event-id", EVENT_ID)
                .set_body_json(json!({"event_id": EVENT_ID, "visible": false})),
        )
        .mount(&dp)
        .await;
    let created = Annotations::data_plane(http(&dp))
        .create(
            target(),
            AnnotationMutation::Comment("later".into()),
            AnnotationEventOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(created.event_id, Uuid::parse_str(EVENT_ID).unwrap());
    assert!(!created.visible);
}

#[tokio::test]
async fn a_204_without_the_header_answers_with_the_sent_event_id() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&dp)
        .await;
    let created = Annotations::data_plane(http(&dp))
        .create(
            target(),
            AnnotationMutation::Labels(vec!["walk".into()]),
            AnnotationEventOptions {
                event_id: Some(Uuid::parse_str(EVENT_ID).unwrap()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(created.event_id, Uuid::parse_str(EVENT_ID).unwrap());
    assert!(created.visible);
}

#[tokio::test]
async fn a_data_plane_client_refuses_an_email_before_any_request() {
    let dp = MockServer::start().await;
    let annotations = Annotations::data_plane(http(&dp));
    let error = annotations
        .create(
            target(),
            AnnotationMutation::ReviewerEmails(vec!["expert@example.com".into()]),
            AnnotationEventOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            introspection_sdk::IntrospectionAPIError::InvalidConfig(_)
        ),
        "{error:?}"
    );
    // Clearing reviewers and id filters need no lookup.
    Mock::given(method("POST"))
        .and(path("/v1/annotations"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&dp)
        .await;
    annotations
        .create(
            target(),
            AnnotationMutation::ReviewerEmails(vec![]),
            AnnotationEventOptions::default(),
        )
        .await
        .unwrap();
    let filtered = AnnotationListParams {
        assigned_to_email: Some("expert@example.com".into()),
        ..Default::default()
    };
    assert!(annotations.list(&filtered).await.is_err());
    assert_eq!(dp.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_annotation_tag_normalizes_its_name_and_color() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tags"))
        .and(body_json(json!({
            "kind": "annotation", "name": "needs-review", "color": "#f97316", "description": "Queue"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(tag(
            "annotation",
            json!("#f97316"),
            json!("Queue"),
        )))
        .mount(&dp)
        .await;
    let tags = Tags::new(http(&dp));
    let created = tags
        .create(TagCreate::Annotation(AnnotationTagCreate {
            name: " needs-review ".into(),
            color: "#F97316".into(),
            description: Some("Queue".into()),
        }))
        .await
        .unwrap();
    assert_eq!(created.name, "needs-review");
    assert_eq!(created.kind, TagKind::Annotation);
}

/// The kind is the serde tag, and an access tag carries no `color` key at all
/// — the API refuses one, so the variant has no field to send.
#[tokio::test]
async fn an_access_tag_sends_no_color() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tags"))
        .and(body_json(json!({"kind": "access", "name": "acme/eu"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(tag(
            "access",
            serde_json::Value::Null,
            serde_json::Value::Null,
        )))
        .mount(&dp)
        .await;
    let tags = Tags::new(http(&dp));
    let created = tags.create(TagCreate::access("acme/eu")).await.unwrap();
    assert_eq!(created.kind, TagKind::Access);
    assert!(created.color.is_none());
}

#[tokio::test]
async fn a_bad_color_is_refused_before_the_request() {
    let tags = Tags::new(http(&MockServer::start().await));
    assert!(tags
        .create(TagCreate::annotation("bad", "orange"))
        .await
        .is_err());
}

#[tokio::test]
async fn an_access_tag_rejects_a_color_update_and_clears_a_description() {
    let dp = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/tags/access/acme%2Feu"))
        .and(body_json(json!({"description": null})))
        .respond_with(ResponseTemplate::new(200).set_body_json(tag(
            "access",
            serde_json::Value::Null,
            serde_json::Value::Null,
        )))
        .mount(&dp)
        .await;
    let tags = Tags::new(http(&dp));
    assert!(tags
        .update(TagKind::Access, "acme/eu", TagUpdate::color("#f97316"))
        .await
        .is_err());
    assert!(tags
        .update(TagKind::Access, "acme/eu", TagUpdate::clear_description())
        .await
        .unwrap()
        .description
        .is_none());
}

#[tokio::test]
async fn an_update_naming_nothing_is_refused_before_the_request() {
    let tags = Tags::new(http(&MockServer::start().await));
    assert!(tags
        .update(TagKind::Annotation, "needs-review", TagUpdate::default())
        .await
        .is_err());
}

#[tokio::test]
async fn deletes_a_tag_by_kind_and_name() {
    let dp = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/tags/annotation/needs-review"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&dp)
        .await;
    Tags::new(http(&dp))
        .delete(TagKind::Annotation, "needs-review")
        .await
        .unwrap();
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
    let mut page = client
        .annotations()
        .list(&AnnotationListParams::default())
        .await
        .unwrap();
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
