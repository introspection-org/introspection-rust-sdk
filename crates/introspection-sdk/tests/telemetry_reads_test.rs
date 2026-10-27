//! The telemetry reads that pass a body through unmodelled: the raw metrics
//! query, an export in a representation the SDK does not type, and an events
//! read over several families.

use introspection_sdk::api::telemetry::{ATIF_ACCEPT, REPLAY_V1_ACCEPT};
use introspection_sdk::{
    AdvancedOptions, ClientConfig, ConversationExportFormat, ConversationExportParams,
    EventListParams, IntrospectionAPIError, IntrospectionClient, IntrospectionEventName,
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
