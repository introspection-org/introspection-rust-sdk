//! End-to-end lifecycle of a [`Runner`]: open it against a mock CP `/run`
//! route, prove the accessors report what the spec said, drive a real DP call
//! through it, re-mint with `refresh()`, and close it.
//!
//! `tests/runner_resources_test.rs` covers the resource namespaces by
//! constructing them directly. This file covers the thing that hands those
//! namespaces out — including the paths that used to `panic!` or silently
//! keep serving a closed session.

use introspection_sdk::api::{IntrospectionAPIError, RunRequest};
use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient, RunnerIdentity};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RUNTIME_ID: &str = "11111111-1111-4111-8111-111111111111";
const RECIPE_ID: &str = "22222222-2222-4222-8222-222222222222";

/// A real [`IntrospectionClient`] pointed at the mock CP -- the same
/// construction path a caller uses, not a hand-assembled HTTP client.
fn client(server: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::builder()
            .token("intro_test")
            .advanced(AdvancedOptions {
                base_api_url: Some(server.uri()),
                ..Default::default()
            })
            .build()
            .unwrap(),
    )
    .unwrap()
}

/// One task as the DP returns it.
fn task_response() -> serde_json::Value {
    json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "org_id": "00000000-0000-0000-0000-00000000aaaa",
        "project_id": "00000000-0000-0000-0000-00000000bbbb",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "kind": "agent",
        "status": "completed",
        "is_archived": false,
    })
}

/// A CP `/run` response pointing the runner at `dp_endpoint`.
fn runner_spec(session_id: &str, dp_endpoint: &str, expires_at: &str) -> serde_json::Value {
    json!({
        "session_id": session_id,
        "deployment": {
            "endpoint": dp_endpoint,
            "slug": "gcp01",
            "region": "us-east-1",
        },
        "session_token": format!("jwt-for-{session_id}"),
        "expires_at": expires_at,
        "runtime_context": {
            "runtime_id": RUNTIME_ID,
            "recipe_id": RECIPE_ID,
            "arm_label": "control",
            "agent_name": "support",
            "identity": {"user_id": "user_1"},
        },
    })
}

fn run_request() -> RunRequest {
    RunRequest {
        identity: Some(RunnerIdentity {
            user_id: Some("user_1".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Mount `POST /v1/runtimes/{id}/run` returning `spec`, capped at `calls`
/// responses so a second call has to be mounted explicitly.
async fn mount_run(server: &MockServer, spec: serde_json::Value, calls: u64) {
    Mock::given(method("POST"))
        .and(path(format!("/v1/runtimes/{RUNTIME_ID}/run")))
        .respond_with(ResponseTemplate::new(200).set_body_json(spec))
        .up_to_n_times(calls)
        .expect(calls)
        .mount(server)
        .await;
}

#[tokio::test]
async fn run_opens_a_runner_whose_accessors_report_the_spec() {
    let cp = MockServer::start().await;
    mount_run(
        &cp,
        runner_spec("sess_1", "https://dp.example.com", "2026-01-01T00:00:00Z"),
        1,
    )
    .await;

    let runtime_id = Uuid::parse_str(RUNTIME_ID).unwrap();
    let runner = client(&cp)
        .runtimes()
        .handle(runtime_id)
        .run(run_request())
        .await
        .unwrap();

    assert_eq!(runner.session_id(), "sess_1");
    assert_eq!(runner.expires_at(), "2026-01-01T00:00:00Z");
    assert_eq!(runner.dp_endpoint(), "https://dp.example.com");
    let deployment = runner.deployment();
    assert_eq!(deployment.slug, "gcp01");
    assert_eq!(deployment.region, "us-east-1");
    let ctx = runner.context();
    assert_eq!(ctx.runtime_id, runtime_id);
    assert_eq!(ctx.arm_label.as_deref(), Some("control"));
    assert_eq!(ctx.identity.user_id.as_deref(), Some("user_1"));
    assert!(!runner.is_closed());
}

#[tokio::test]
async fn the_namespaces_a_runner_hands_out_talk_to_the_deployment_from_the_spec() {
    // The whole point of the runner: `runner.tasks()` must reach the DP the
    // CP named, with the session token as the bearer -- not the CP host and
    // not the CP token.
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks/task_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_response()))
        .expect(1)
        .mount(&dp)
        .await;

    let cp = MockServer::start().await;
    mount_run(
        &cp,
        runner_spec("sess_1", &dp.uri(), "2026-01-01T00:00:00Z"),
        1,
    )
    .await;

    let runner = client(&cp)
        .runtimes()
        .handle(Uuid::parse_str(RUNTIME_ID).unwrap())
        .run(run_request())
        .await
        .unwrap();

    let task = runner.tasks().get("task_1").await.unwrap();
    assert_eq!(task.id.to_string(), "00000000-0000-0000-0000-000000000001");

    let received = &dp.received_requests().await.unwrap()[0];
    let auth = received.headers.get("authorization").unwrap();
    assert_eq!(auth.to_str().unwrap(), "Bearer jwt-for-sess_1");
}

#[tokio::test]
async fn refresh_re_mints_the_session_and_repoints_the_namespaces() {
    // Two DPs: refresh() must swap which one subsequent calls reach, so a
    // stale endpoint cached anywhere would show up as a request to `first`.
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for dp in [&first, &second] {
        Mock::given(method("GET"))
            .and(path("/v1/tasks/task_1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(task_response()))
            .mount(dp)
            .await;
    }

    let cp = MockServer::start().await;
    // `up_to_n_times(1)` on the first mount means the second /run call falls
    // through to this one -- wiremock matches the most recently mounted first.
    mount_run(
        &cp,
        runner_spec("sess_1", &first.uri(), "2026-01-01T00:00:00Z"),
        1,
    )
    .await;

    let runner = client(&cp)
        .runtimes()
        .handle(Uuid::parse_str(RUNTIME_ID).unwrap())
        .run(run_request())
        .await
        .unwrap();
    runner.tasks().get("task_1").await.unwrap();
    assert_eq!(first.received_requests().await.unwrap().len(), 1);

    mount_run(
        &cp,
        runner_spec("sess_2", &second.uri(), "2026-06-01T00:00:00Z"),
        1,
    )
    .await;
    runner.refresh().await.unwrap();

    assert_eq!(runner.session_id(), "sess_2");
    assert_eq!(runner.expires_at(), "2026-06-01T00:00:00Z");
    assert_eq!(runner.dp_endpoint(), second.uri());

    runner.tasks().get("task_1").await.unwrap();
    assert_eq!(
        first.received_requests().await.unwrap().len(),
        1,
        "the pre-refresh DP must see no further traffic"
    );
    assert_eq!(second.received_requests().await.unwrap().len(), 1);
    let received = &second.received_requests().await.unwrap()[0];
    assert_eq!(
        received
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        "Bearer jwt-for-sess_2",
        "the refreshed session token must be the one on the wire"
    );
}

#[tokio::test]
async fn a_namespace_taken_before_close_stops_working_after_it() {
    // The handle is a cheap clone captured *before* close(). It shares the
    // client's closed flag, so it must refuse too -- and refuse with a typed
    // error rather than the panic the accessors used to raise.
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks/task_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_response()))
        .expect(1)
        .mount(&dp)
        .await;

    let cp = MockServer::start().await;
    mount_run(
        &cp,
        runner_spec("sess_1", &dp.uri(), "2026-01-01T00:00:00Z"),
        1,
    )
    .await;

    let runner = client(&cp)
        .runtimes()
        .handle(Uuid::parse_str(RUNTIME_ID).unwrap())
        .run(run_request())
        .await
        .unwrap();
    let tasks = runner.tasks();
    tasks.get("task_1").await.unwrap();

    runner.close();
    assert!(runner.is_closed());

    // Accessors still answer -- they are pure reads of local state.
    assert_eq!(runner.session_id(), "sess_1");
    assert_eq!(runner.dp_endpoint(), dp.uri());

    for label in ["handle taken before close", "handle taken after close"] {
        let handle = if label.contains("before") {
            tasks.clone()
        } else {
            runner.tasks()
        };
        match handle.get("task_1").await {
            Ok(_) => panic!("{label}: expected the closed runner to refuse"),
            Err(IntrospectionAPIError::InvalidConfig(message)) => {
                assert!(message.contains("closed"), "{label}: got {message}");
            }
            Err(other) => panic!("{label}: expected InvalidConfig, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn refresh_after_close_is_refused() {
    let cp = MockServer::start().await;
    mount_run(
        &cp,
        runner_spec("sess_1", "https://dp.example.com", "2026-01-01T00:00:00Z"),
        1,
    )
    .await;

    let runner = client(&cp)
        .runtimes()
        .handle(Uuid::parse_str(RUNTIME_ID).unwrap())
        .run(run_request())
        .await
        .unwrap();
    runner.close();

    mount_run(
        &cp,
        runner_spec("sess_2", "https://dp2.example.com", "2026-06-01T00:00:00Z"),
        1,
    )
    .await;
    let err = runner.refresh().await.unwrap_err();
    let IntrospectionAPIError::InvalidConfig(message) = err else {
        panic!("expected InvalidConfig");
    };
    assert!(message.contains("closed"), "got {message}");
    // ...and the close sticks: the refused refresh must not have swapped in a
    // live session behind the closed flag.
    assert!(runner.is_closed());
    assert_eq!(runner.session_id(), "sess_1");
}
