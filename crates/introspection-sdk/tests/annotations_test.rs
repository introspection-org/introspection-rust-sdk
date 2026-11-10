use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    AdvancedOptions, AnnotationCreated, AnnotationEventOptions, AnnotationExportParams,
    AnnotationFacet, AnnotationListParams, AnnotationMutation, AnnotationTagCreate,
    AnnotationTarget, Annotations, ClientConfig, Event, EventListParams, IntrospectionClient,
    IntrospectionEventName, TagCreate, TagKind, TagUpdate, Tags,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::Request;
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

fn member() -> Uuid {
    MEMBER_ID.parse().unwrap()
}

fn annotation() -> serde_json::Value {
    json!({
        "trace_id": TRACE_ID,
        "span_id": SPAN_ID,
        "conversation_id": "conversation-1",
        "labels": ["needs-review"],
        "assignee_member_ids": [MEMBER_ID],
        "reviewer_member_ids": [MEMBER_ID],
        "comment_count": 1,
        "updated_at": "2026-08-25T12:00:00Z",
        "updated_by_member_id": MEMBER_ID,
        "assignment_event_id": EVENT_ID,
        "comments": null
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
    let state = stream.next().await.unwrap().unwrap();
    assert_eq!(state.labels, vec!["needs-review"]);
    assert_eq!(state.reviewer_member_ids, vec![member()]);
    assert_eq!(state.comment_count, 1);
    assert_eq!(state.updated_by_member_id, member());
    assert_eq!(state.comments, None);
}

const JSON_LINES: &str = "application/x-ndjson";

fn export_body() -> String {
    let older = json!({
        "trace_id": TRACE_ID,
        "span_id": "00f067aa0ba902b7",
        "human_commenter_member_ids": [MEMBER_ID],
        "updated_at": "2026-08-25T12:00:00Z",
        "updated_by_member_id": MEMBER_ID
    });
    format!("{}\n{older}\n", annotation())
}

#[tokio::test]
async fn export_reads_every_row_with_the_filters_on_the_query() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations/export"))
        .and(header("accept", JSON_LINES))
        .and(query_param("assignee_member_id", MEMBER_ID))
        .and(query_param("conversation_id", "conversation-1"))
        .and(query_param("status", "reviewed"))
        .and(|request: &Request| {
            let labels: Vec<_> = request
                .url
                .query_pairs()
                .filter(|(key, _)| key == "label")
                .map(|(_, value)| value.into_owned())
                .collect();
            let paged = request
                .url
                .query_pairs()
                .any(|(key, _)| key == "limit" || key == "next" || key == "include_total");
            labels == ["needs-review", "wrong-tool"] && !paged
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(export_body(), JSON_LINES))
        .expect(1)
        .mount(&dp)
        .await;

    let rows: Vec<_> = Annotations::data_plane(http(&dp))
        .export(&AnnotationExportParams {
            assignee_member_id: Some(member()),
            conversation_id: Some("conversation-1".into()),
            labels: vec!["needs-review".into(), "wrong-tool".into()],
            status: Some("reviewed".into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    let spans: Vec<_> = rows.iter().map(|row| row.span_id.as_str()).collect();
    assert_eq!(spans, [SPAN_ID, "00f067aa0ba902b7"]);
    assert_eq!(rows[0].labels, vec!["needs-review"]);
    // The second row names its reviewers the way an older Data Plane does.
    assert_eq!(rows[1].reviewer_member_ids, vec![member()]);
}

#[tokio::test]
async fn export_rows_carry_their_comments_oldest_first() {
    let dp = MockServer::start().await;
    let mut row = annotation();
    row["comment_count"] = json!(2);
    row["comments"] = json!([
        {
            "event_id": EVENT_ID,
            "comment": "Wrong tool picked",
            "member_id": MEMBER_ID,
            "actor_member_id": null,
            "actor_member_type": null,
            "created_at": "2026-08-25T11:00:00Z"
        },
        {
            "event_id": "019fc000-0000-7000-8000-000000000002",
            "comment": "Confirmed against the trace",
            "member_id": MEMBER_ID,
            "actor_member_id": "00000000-0000-0000-0000-0000000000dd",
            "actor_member_type": "agent",
            "created_at": "2026-08-25T12:00:00Z"
        }
    ]);
    let mut bare = annotation();
    bare["comment_count"] = json!(0);
    bare["comments"] = json!([]);
    Mock::given(method("GET"))
        .and(path("/v1/annotations/export"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(format!("{row}\n{bare}\n"), JSON_LINES),
        )
        .mount(&dp)
        .await;

    let rows: Vec<_> = Annotations::data_plane(http(&dp))
        .export(&AnnotationExportParams::default())
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    let comments = rows[0].comments.as_deref().expect("the export's comments");
    let texts: Vec<_> = comments.iter().map(|c| c.comment.as_str()).collect();
    assert_eq!(texts, ["Wrong tool picked", "Confirmed against the trace"]);
    assert_eq!(comments[0].member_id, member());
    assert_eq!(comments[0].actor_member_type, None);
    assert_eq!(comments[1].actor_member_type.as_deref(), Some("agent"));
    assert!(comments[1].actor_member_id.is_some());
    assert_eq!(comments[1].created_at, "2026-08-25T12:00:00Z");
    assert_eq!(rows[1].comments.as_deref(), Some(&[][..]));
}

#[tokio::test]
async fn export_stream_passes_the_json_lines_bytes_through() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations/export"))
        .and(header("accept", JSON_LINES))
        .and(query_param("trace_id", TRACE_ID))
        .respond_with(ResponseTemplate::new(200).set_body_raw(export_body(), JSON_LINES))
        .expect(1)
        .mount(&dp)
        .await;

    let chunks: Vec<_> = Annotations::data_plane(http(&dp))
        .export_stream(&AnnotationExportParams {
            trace_id: Some(TRACE_ID.into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(chunks.concat(), export_body().as_bytes());
}

#[tokio::test]
async fn export_of_an_empty_body_yields_nothing() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations/export"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("", JSON_LINES))
        .expect(1)
        .mount(&dp)
        .await;

    let rows: Vec<_> = Annotations::data_plane(http(&dp))
        .export(&AnnotationExportParams::default())
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    assert!(rows.is_empty());
}

#[tokio::test]
async fn export_surfaces_a_406() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations/export"))
        .respond_with(ResponseTemplate::new(406))
        .expect(1)
        .mount(&dp)
        .await;

    let result = Annotations::data_plane(http(&dp))
        .export(&AnnotationExportParams::default())
        .await;

    let err = result.err().expect("the 406 is an error");
    assert_eq!(err.status(), Some(406), "{err}");
}

#[tokio::test]
async fn export_refuses_an_id_and_an_email_for_one_filter_without_a_request() {
    let dp = MockServer::start().await;

    let result = Annotations::data_plane(http(&dp))
        .export(&AnnotationExportParams {
            assignee_member_id: Some(member()),
            assigned_to_email: Some("expert@example.com".into()),
            ..Default::default()
        })
        .await;

    let err = result.err().expect("the conflict is an error");
    assert_eq!(err.status(), Some(422), "{err}");
    assert!(dp.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn facets_read_the_dataset_summary_of_a_review_queue_bucket() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations/facets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"dimension": "status", "value": "backlog", "count": 3},
            {
                "dimension": "review_queue",
                "value": "needs-review",
                "count": 2,
                "conversation_count": 1,
                "comment_count": 4,
                "reviewer_member_ids": [MEMBER_ID],
                "updated_at": "2026-08-25T12:00:00Z"
            }
        ])))
        .mount(&dp)
        .await;

    let facets: Vec<AnnotationFacet> = Annotations::data_plane(http(&dp)).facets().await.unwrap();

    assert_eq!(facets[0].conversation_count, None);
    assert_eq!(facets[1].dimension, "review_queue");
    assert_eq!(facets[1].conversation_count, Some(1));
    assert_eq!(facets[1].comment_count, Some(4));
    assert_eq!(facets[1].reviewer_member_ids, Some(vec![member()]));
}

#[tokio::test]
async fn reads_the_row_an_older_data_plane_sends() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/annotations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{
                "trace_id": TRACE_ID,
                "span_id": SPAN_ID,
                "conversation_id": "conversation-1",
                "labels": ["needs-review"],
                "assignee_member_ids": [MEMBER_ID],
                "annotator_member_ids": [MEMBER_ID],
                "commenter_member_ids": [MEMBER_ID],
                "human_commenter_member_ids": [MEMBER_ID],
                "has_comment": true,
                "has_human_comment": true,
                "comment_count": 2,
                "latest_comment": "looks wrong",
                "latest_comment_member_id": MEMBER_ID,
                "latest_comment_actor_member_id": MEMBER_ID,
                "latest_comment_actor_type": "human",
                "updated_at": "2026-08-25T12:00:00Z",
                "updated_by_member_id": MEMBER_ID,
                "assignment_event_id": EVENT_ID
            }],
            "count": 1, "total_count": null, "next": null
        })))
        .mount(&dp)
        .await;
    let result = Annotations::new(http(&dp), http(&dp))
        .list(&AnnotationListParams::default())
        .await
        .unwrap()
        .next()
        .await
        .unwrap();

    // With `strict-schema` the retired fields are reported, not dropped; the
    // renamed one is read either way.
    if cfg!(feature = "strict-schema") {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("latest_comment"), "{error}");
        assert!(!error.contains("human_commenter_member_ids"), "{error}");
        return;
    }
    let state = result.unwrap();
    assert_eq!(state.reviewer_member_ids, vec![member()]);
    assert_eq!(state.comment_count, 2);
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
