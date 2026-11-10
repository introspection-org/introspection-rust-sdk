//! The telemetry reads that pass a body through unmodelled: the raw metrics
//! query, an export in a representation the SDK does not type, and an events
//! read over several families.

use futures::TryStreamExt;
use introspection_sdk::api::telemetry::{ATIF_ACCEPT, JSON_LINES_ACCEPT, REPLAY_V1_ACCEPT};
use introspection_sdk::{
    AdvancedOptions, ClientConfig, ConversationExportFormat, ConversationExportInclude,
    ConversationExportParams, EventListParams, IntrospectionAPIError, IntrospectionClient,
    IntrospectionEventName,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(dp: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(dp.uri()),
                dp_url: Some(dp.uri()),
                ..Default::default()
            })
            .build()
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn query_raw_sends_the_document_unchanged_and_returns_the_body() {
    let dp = MockServer::start().await;
    // A request field and a response field this SDK build does not model.
    let request = json!({
        "view": "events",
        "metrics": [{"aggregation": "count"}],
        "from_timestamp": "2026-01-01T00:00:00Z",
        "to_timestamp": "2026-01-02T00:00:00Z",
        "future_knob": {"nested": [1, 2]},
    });
    let response = json!({"data": [{"count": 3}], "meta": {"future": true}});
    Mock::given(method("POST"))
        .and(path("/v1/metrics"))
        .and(body_json(&request))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .expect(1)
        .mount(&dp)
        .await;

    let got = client(&dp).metrics().query_raw(&request).await.unwrap();

    assert_eq!(got, response);
}

#[tokio::test]
async fn query_raw_refuses_a_non_object_without_a_request() {
    let dp = MockServer::start().await;
    let err = client(&dp)
        .metrics()
        .query_raw(&json!([1, 2]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IntrospectionAPIError::InvalidConfig(_)),
        "{err}"
    );
    assert!(dp.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn export_json_value_negotiates_each_json_representation() {
    let dp = MockServer::start().await;
    for accept in [REPLAY_V1_ACCEPT, ATIF_ACCEPT, "application/json"] {
        Mock::given(method("GET"))
            .and(path("/v1/conversations/conv%2F1/export"))
            .and(header("accept", accept))
            .and(query_param("agent", "root"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accept": accept})))
            .expect(1)
            .mount(&dp)
            .await;
    }
    let params = ConversationExportParams {
        agent: Some("root".into()),
        ..Default::default()
    };
    let conversations = client(&dp).conversations();
    for format in [
        ConversationExportFormat::Replay,
        ConversationExportFormat::Atif,
        ConversationExportFormat::Json,
    ] {
        let body = conversations
            .export_json_value("conv/1", format, &params)
            .await
            .unwrap();
        assert_eq!(body["accept"], format.accept());
    }
}

#[tokio::test]
async fn export_spans_reads_the_json_lines_export_a_span_at_a_time() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/conversations/conv%2F1/export"))
        .and(header("accept", JSON_LINES_ACCEPT))
        .and(query_param("agent", "root"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                r#"{"trace_id":"t","span_id":"a","start_time":"2026-01-01T00:00:00Z"}"#,
                "\n",
                r#"{"trace_id":"t","span_id":"b","start_time":"2026-01-01T00:00:00Z"}"#,
                "\n",
            ),
            JSON_LINES_ACCEPT,
        ))
        .expect(1)
        .mount(&dp)
        .await;
    let params = ConversationExportParams {
        agent: Some("root".into()),
        ..Default::default()
    };

    let spans: Vec<_> = client(&dp)
        .conversations()
        .export_spans("conv/1", &params)
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = spans.iter().map(|s| s.span_id.as_deref()).collect();
    assert_eq!(ids, [Some("a"), Some("b")]);
    assert_eq!(
        ConversationExportFormat::JsonLines.accept(),
        JSON_LINES_ACCEPT
    );
}

#[tokio::test]
async fn export_spans_asks_for_annotations_and_reads_them_on_the_span() {
    let dp = MockServer::start().await;
    let member = "00000000-0000-0000-0000-0000000000cc";
    let annotated = json!({
        "trace_id": "t", "span_id": "a", "start_time": "2026-01-01T00:00:00Z",
        "annotation": {
            "trace_id": "t", "span_id": "a", "conversation_id": "conv",
            "labels": ["needs-review"], "assignee_member_ids": [member],
            "reviewer_member_ids": [member], "comment_count": 2,
            "updated_at": "2026-08-25T12:00:00Z", "updated_by_member_id": member,
            "comments": [
                {
                    "event_id": "019fc000-0000-7000-8000-000000000001",
                    "comment": "Wrong tool picked", "member_id": member,
                    "created_at": "2026-08-25T11:00:00Z"
                },
                {
                    "event_id": "019fc000-0000-7000-8000-000000000002",
                    "comment": "Confirmed", "member_id": member,
                    "actor_member_id": "00000000-0000-0000-0000-0000000000dd",
                    "actor_member_type": "agent", "created_at": "2026-08-25T12:00:00Z"
                }
            ]
        }
    });
    let plain = json!({"trace_id": "t", "span_id": "b", "start_time": "2026-01-01T00:00:00Z"});
    Mock::given(method("GET"))
        .and(path("/v1/conversations/conv/export"))
        .and(header("accept", JSON_LINES_ACCEPT))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(format!("{annotated}\n{plain}\n"), JSON_LINES_ACCEPT),
        )
        .expect(1)
        .mount(&dp)
        .await;
    let params = ConversationExportParams {
        include: vec![
            ConversationExportInclude::Annotations,
            ConversationExportInclude::Comments,
        ],
        ..Default::default()
    };

    let spans: Vec<_> = client(&dp)
        .conversations()
        .export_spans("conv", &params)
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    let requests = dp.received_requests().await.unwrap();
    let includes: Vec<_> = requests[0]
        .url
        .query_pairs()
        .filter(|(key, _)| key == "include")
        .map(|(_, value)| value.into_owned())
        .collect();
    assert_eq!(includes, ["annotations", "comments"]);
    let annotation = spans[0].annotation.as_ref().expect("the span's annotation");
    assert_eq!(annotation.labels, ["needs-review"]);
    let comments = annotation.comments.as_deref().expect("its comments");
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[1].actor_member_type.as_deref(), Some("agent"));
    assert_eq!(spans[1].annotation, None);
}

#[tokio::test]
async fn export_spans_surfaces_the_406_of_an_older_data_plane() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/conversations/conv/export"))
        .respond_with(ResponseTemplate::new(406))
        .expect(1)
        .mount(&dp)
        .await;

    let result = client(&dp)
        .conversations()
        .export_spans("conv", &ConversationExportParams::default())
        .await;

    let err = result.err().expect("the 406 is an error");
    assert_eq!(err.status(), Some(406), "{err}");
}

#[tokio::test]
async fn export_json_value_refuses_json_lines_without_a_request() {
    let dp = MockServer::start().await;
    let err = client(&dp)
        .conversations()
        .export_json_value(
            "conv",
            ConversationExportFormat::JsonLines,
            &ConversationExportParams::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, IntrospectionAPIError::InvalidConfig(_)),
        "{err}"
    );
    assert!(dp.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn export_json_value_refuses_arrow_without_a_request() {
    let dp = MockServer::start().await;
    let err = client(&dp)
        .conversations()
        .export_json_value(
            "conv",
            ConversationExportFormat::Arrow,
            &ConversationExportParams::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, IntrospectionAPIError::InvalidConfig(_)),
        "{err}"
    );
    assert!(dp.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn an_events_read_over_several_families_repeats_event_name() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"records": [], "count": 0, "next": null})),
        )
        .expect(1)
        .mount(&dp)
        .await;
    let params = EventListParams {
        extra_event_names: Some(vec![IntrospectionEventName::AutomationSkipped]),
        ..EventListParams::new(IntrospectionEventName::AutomationTriggered)
    };
    let page = client(&dp)
        .events()
        .list(&params)
        .unwrap()
        .next_page()
        .await
        .unwrap();
    assert!(page.is_some());

    let requests = dp.received_requests().await.unwrap();
    let names: Vec<String> = requests[0]
        .url
        .query_pairs()
        .filter(|(key, _)| key == "event_name")
        .map(|(_, value)| value.into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "introspection.automation.triggered",
            "introspection.automation.skipped"
        ]
    );
}
