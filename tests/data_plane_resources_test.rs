//! [`DataPlaneResources`] is one surface on two holders: the same generic
//! function drives every Data Plane namespace through an
//! [`IntrospectionClient`] and through a [`Runner`], and each reaches the Data
//! Plane with its own credential.

use futures::StreamExt;
use introspection_sdk::api::RunRequest;
use introspection_sdk::{
    AdvancedOptions, AutomationListParams, ClientConfig, ConversationListParams,
    DataPlaneResources, EventListParams, FileListParams, IntrospectionClient,
    IntrospectionEventName, IssueListParams, MemberConnectionListParams, MetricSpec, MetricsQuery,
    Runner, ShareListParams, TaskListParams,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RUNTIME_ID: &str = "11111111-1111-4111-8111-111111111111";
const RUNTIME_GROUP_ID: &str = "33333333-3333-4333-8333-333333333333";

fn empty_page() -> serde_json::Value {
    json!({"records": [], "count": 0, "next": null})
}

/// Every namespace the trait names answers on `dp`, but only to `bearer`.
async fn mount_data_plane(dp: &MockServer, bearer: &str) {
    let auth = format!("Bearer {bearer}");
    for list in [
        "/v1/tasks",
        "/v1/files",
        "/v1/shares",
        "/v1/conversations",
        "/v1/events",
        "/v1/automations",
        "/v1/issues",
        "/v1/connections",
    ] {
        Mock::given(method("GET"))
            .and(path(list))
            .and(header("authorization", auth.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(empty_page()))
            .expect(1)
            .mount(dp)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/v1/metrics"))
        .and(header("authorization", auth.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [], "meta": {}})))
        .expect(1)
        .mount(dp)
        .await;
}

/// One call on each namespace, written once against the trait.
async fn drive_every_namespace(dp: &impl DataPlaneResources) {
    let tasks = dp.tasks();
    let _runs = &tasks.runs;
    assert!(tasks
        .list(&TaskListParams::default())
        .next_page()
        .await
        .unwrap()
        .is_some());
    assert!(dp
        .files()
        .list(&FileListParams::default())
        .next()
        .await
        .is_none());
    assert!(dp
        .shares()
        .list(&ShareListParams::default())
        .next()
        .await
        .is_none());
    assert!(dp
        .conversations()
        .list(&ConversationListParams::default())
        .unwrap()
        .next()
        .await
        .is_none());
    assert!(dp
        .events()
        .list(&EventListParams::new(IntrospectionEventName::Feedback))
        .unwrap()
        .next()
        .await
        .is_none());
    dp.metrics()
        .query(&MetricsQuery {
            view: "spans".into(),
            metrics: vec![MetricSpec {
                measure: "duration_ns".into(),
                aggregation: "p95".into(),
            }],
            lookback: Some("24h".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(dp
        .automations()
        .list(&AutomationListParams::default())
        .next()
        .await
        .is_none());
    assert!(dp
        .issues()
        .list(&IssueListParams::default())
        .next()
        .await
        .is_none());
    assert!(dp
        .connections()
        .list(&MemberConnectionListParams::default())
        .next()
        .await
        .is_none());
}

fn client(cp: &MockServer, dp: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(cp.uri()),
                dp_url: Some(dp.uri()),
                ..Default::default()
            })
            .build()
            .unwrap(),
    )
    .unwrap()
}

async fn runner(cp: &MockServer, dp: &MockServer) -> Runner {
    Mock::given(method("POST"))
        .and(path(format!("/v1/runtimes/{RUNTIME_ID}/run")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "session_id": "sess_1",
            "deployment": {"endpoint": dp.uri(), "slug": "gcp01", "region": "us-east-1"},
            "session_token": "runner-jwt",
            "expires_at": "2026-01-01T00:00:00Z",
            "runtime_context": {
                "runtime_id": RUNTIME_ID,
                "runtime_group_id": RUNTIME_GROUP_ID,
                "recipe_id": "22222222-2222-4222-8222-222222222222",
                "identity": {},
            },
        })))
        .mount(cp)
        .await;
    client(cp, dp)
        .runtimes()
        .handle(Uuid::parse_str(RUNTIME_ID).unwrap())
        .run(RunRequest::default())
        .await
        .unwrap()
}

#[tokio::test]
async fn the_client_serves_every_data_plane_namespace_on_its_own_token() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    mount_data_plane(&dp, "intro_test").await;

    drive_every_namespace(&client(&cp, &dp)).await;
}

#[tokio::test]
async fn a_runner_serves_every_data_plane_namespace_on_its_session_token() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    mount_data_plane(&dp, "runner-jwt").await;

    drive_every_namespace(&runner(&cp, &dp).await).await;
}
