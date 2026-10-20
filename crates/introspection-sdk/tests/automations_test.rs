//! Integration tests for the automations DP surface (`client.automations()`)
//! and the automation event families, backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    AdvancedOptions, AutomationCondition, AutomationConditionType, AutomationCreateParams,
    AutomationExecutionStatus, AutomationKind, AutomationListParams, AutomationMetadata,
    AutomationSkipReason, AutomationTriggerType, AutomationUpdateParams, Automations, ClientConfig,
    Event, EventListParams, Events, IntrospectionClient, IntrospectionEventName, PaginationParams,
    TaskRepoRequest,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const AUTOMATION_ID: &str = "11111111-1111-7111-8111-111111111111";
const TASK_ID: &str = "22222222-2222-7222-8222-222222222222";
const GROUP_ID: &str = "33333333-3333-7333-8333-333333333333";
const MEMBER_ID: &str = "44444444-4444-7444-8444-444444444444";

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

fn automation_json() -> serde_json::Value {
    json!({
        "id": AUTOMATION_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": "00000000-0000-0000-0000-0000000000bb",
        "created_at": "2026-10-04T00:00:00Z",
        "updated_at": "2026-10-04T00:00:00Z",
        "name": "Daily follow-up",
        "description": null,
        "enabled": true,
        "runtime_group_id": GROUP_ID,
        "task_id": TASK_ID,
        "created_by_member_id": MEMBER_ID,
        "execution_blocked_reason": null,
        "can_manage": true,
        "tags": [],
        "trigger_type": "cron",
        "cron_schedule": "0 9 * * *",
        "kind": null,
        "prompt": "Check in on the open items.",
        "metadata": {
            "cron_schedules": ["0 9 * * 1-5"],
            "timezone": "Australia/Sydney",
            "repositories": [{"repo": "acme/app", "ref": "main"}],
            "conditions": [{"type": "has_new_tasks_since_last_run"}],
        },
        "last_triggered_at": null,
        "next_trigger_at": "2026-10-05T22:00:00Z",
        "owner_role": "operator",
    })
}

fn page(records: Vec<serde_json::Value>, next: Option<&str>) -> serde_json::Value {
    json!({ "records": records, "count": records.len(), "next": next })
}

fn query_pairs(request: &Request) -> Vec<(String, String)> {
    request
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[tokio::test]
async fn list_parses_the_read_model_and_its_typed_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/automations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![automation_json()], None)))
        .mount(&server)
        .await;

    let found: Vec<_> = Automations::new(build_http(&server))
        .list(&AutomationListParams::default())
        .collect()
        .await;

    let automation = found[0].as_ref().unwrap();
    assert_eq!(automation.id, id(AUTOMATION_ID));
    assert_eq!(automation.kind, None);
    assert_eq!(automation.trigger_type, AutomationTriggerType::Cron);
    assert_eq!(automation.task_id, Some(id(TASK_ID)));
    assert_eq!(automation.runtime_group_id, Some(id(GROUP_ID)));
    assert_eq!(automation.created_by_member_id, Some(id(MEMBER_ID)));
    assert!(automation.can_manage);
    assert_eq!(automation.owner_role.as_deref(), Some("operator"));
    assert_eq!(
        automation.next_trigger_at.as_deref(),
        Some("2026-10-05T22:00:00Z")
    );

    let metadata = automation.typed_metadata().unwrap();
    assert_eq!(metadata.cron_schedules, vec!["0 9 * * 1-5".to_string()]);
    assert_eq!(metadata.timezone.as_deref(), Some("Australia/Sydney"));
    assert_eq!(metadata.repositories[0].repo, "acme/app");
    assert_eq!(metadata.repositories[0].git_ref.as_deref(), Some("main"));
    assert_eq!(
        metadata.conditions[0].condition_type,
        AutomationConditionType::HasNewTasksSinceLastRun
    );
}

#[tokio::test]
async fn kinds_decode_including_project_check_in_and_an_unknown_value() {
    let server = MockServer::start().await;
    let mut records = Vec::new();
    for kind in [
        "project_check_in",
        "observation_synthesis",
        "observation_clustering",
        "a_kind_from_the_future",
    ] {
        let mut record = automation_json();
        record["kind"] = json!(kind);
        record["task_id"] = json!(null);
        record["created_by_member_id"] = json!(null);
        records.push(record);
    }
    Mock::given(method("GET"))
        .and(path("/v1/automations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(records, None)))
        .mount(&server)
        .await;

    let kinds: Vec<_> = Automations::new(build_http(&server))
        .list(&AutomationListParams::default())
        .map(|automation| automation.unwrap().kind)
        .collect()
        .await;

    assert_eq!(
        kinds,
        vec![
            Some(AutomationKind::ProjectCheckIn),
            Some(AutomationKind::ObservationSynthesis),
            Some(AutomationKind::ObservationClustering),
            Some(AutomationKind::Other("a_kind_from_the_future".into())),
        ]
    );
    // An unknown kind round-trips to the value it came from.
    assert_eq!(
        serde_json::to_value(AutomationKind::from("a_kind_from_the_future")).unwrap(),
        json!("a_kind_from_the_future")
    );
}

#[tokio::test]
async fn list_sends_every_filter_and_keeps_task_id_across_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/automations"))
        .and(query_param("next", "cursor-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![automation_json()], None)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/automations"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page(vec![automation_json()], Some("cursor-2"))),
        )
        .mount(&server)
        .await;

    let params = AutomationListParams {
        pagination: PaginationParams {
            limit: Some(1),
            next: None,
        },
        kind: Some(AutomationKind::ProjectCheckIn),
        enabled: Some(true),
        scheduled: Some(false),
        task_id: Some(id(TASK_ID)),
        filters: None,
    };
    let found: Vec<_> = Automations::new(build_http(&server))
        .list(&params)
        .collect()
        .await;
    assert_eq!(found.len(), 2);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let pairs = query_pairs(request);
        for expected in [
            ("limit", "1"),
            ("kind", "project_check_in"),
            ("enabled", "true"),
            ("scheduled", "false"),
            ("task_id", TASK_ID),
        ] {
            assert!(
                pairs.contains(&(expected.0.into(), expected.1.into())),
                "{expected:?} missing from {pairs:?}"
            );
        }
    }
    assert!(!query_pairs(&requests[0]).iter().any(|(k, _)| k == "next"));
    assert!(query_pairs(&requests[1]).contains(&("next".into(), "cursor-2".into())));
}

#[tokio::test]
async fn an_empty_list_params_sends_no_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/automations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![], None)))
        .mount(&server)
        .await;

    let _: Vec<_> = Automations::new(build_http(&server))
        .list(&AutomationListParams::default())
        .collect()
        .await;

    let requests = server.received_requests().await.unwrap();
    assert!(query_pairs(&requests[0]).is_empty());
}

#[tokio::test]
async fn get_reads_one_automation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(automation_json()))
        .mount(&server)
        .await;

    let automation = Automations::new(build_http(&server))
        .get(id(AUTOMATION_ID))
        .await
        .unwrap();

    assert_eq!(automation.id, id(AUTOMATION_ID));
    assert_eq!(
        automation.prompt.as_deref(),
        Some("Check in on the open items.")
    );
}

#[tokio::test]
async fn create_sends_only_the_set_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/automations"))
        .and(body_json(json!({
            "name": "Remind me",
            "trigger_type": "manual",
            "prompt": "Follow up on the invoice.",
            "runtime_group_id": GROUP_ID,
            "task_id": TASK_ID,
            "next_trigger_at": "2026-10-06T09:00:00+10:00",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(automation_json()))
        .mount(&server)
        .await;

    let automation = Automations::new(build_http(&server))
        .create(&AutomationCreateParams {
            prompt: Some("Follow up on the invoice.".into()),
            runtime_group_id: Some(id(GROUP_ID)),
            task_id: Some(id(TASK_ID)),
            next_trigger_at: Some("2026-10-06T09:00:00+10:00".into()),
            ..AutomationCreateParams::new("Remind me", AutomationTriggerType::Manual)
        })
        .await
        .unwrap();

    assert_eq!(automation.id, id(AUTOMATION_ID));
}

#[tokio::test]
async fn create_serializes_a_platform_kind_and_typed_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/automations"))
        .and(body_json(json!({
            "name": "Check-in",
            "trigger_type": "cron",
            "cron_schedule": "0 9 * * *",
            "kind": "project_check_in",
            "prompt": "How is the project going?",
            "runtime_group_id": GROUP_ID,
            "metadata": {
                "repositories": [{"repo": "acme/app"}],
                "cron_schedules": ["0 9 * * 1-5"],
                "timezone": "UTC",
                "conditions": [
                    {"type": "no_live_task_for_automation"},
                    {"type": "last_run_issues_resolved", "runtime_group_id": GROUP_ID},
                ],
            },
            "enabled": false,
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(automation_json()))
        .mount(&server)
        .await;

    Automations::new(build_http(&server))
        .create(&AutomationCreateParams {
            cron_schedule: Some("0 9 * * *".into()),
            kind: Some(AutomationKind::ProjectCheckIn),
            prompt: Some("How is the project going?".into()),
            runtime_group_id: Some(id(GROUP_ID)),
            metadata: Some(AutomationMetadata {
                repositories: vec![TaskRepoRequest {
                    repo: "acme/app".into(),
                    ..Default::default()
                }],
                cron_schedules: vec!["0 9 * * 1-5".into()],
                timezone: Some("UTC".into()),
                conditions: vec![
                    AutomationCondition::new(AutomationConditionType::NoLiveTaskForAutomation),
                    AutomationCondition {
                        runtime_group_id: Some(id(GROUP_ID)),
                        ..AutomationCondition::new(AutomationConditionType::LastRunIssuesResolved)
                    },
                ],
            }),
            enabled: Some(false),
            ..AutomationCreateParams::new("Check-in", AutomationTriggerType::Cron)
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn update_sends_only_the_set_fields() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}")))
        .and(body_json(json!({ "enabled": false })))
        .respond_with(ResponseTemplate::new(200).set_body_json(automation_json()))
        .mount(&server)
        .await;

    Automations::new(build_http(&server))
        .update(
            id(AUTOMATION_ID),
            &AutomationUpdateParams {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn update_metadata_sends_only_the_set_metadata_fields() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}")))
        .and(body_json(json!({
            "cron_schedule": "30 8 * * *",
            "task_id": TASK_ID,
            "metadata": {"timezone": "Europe/London"},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(automation_json()))
        .mount(&server)
        .await;

    Automations::new(build_http(&server))
        .update(
            id(AUTOMATION_ID),
            &AutomationUpdateParams {
                cron_schedule: Some("30 8 * * *".into()),
                task_id: Some(id(TASK_ID)),
                metadata: Some(AutomationMetadata {
                    timezone: Some("Europe/London".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[test]
fn an_empty_update_serializes_to_an_empty_object() {
    assert_eq!(
        serde_json::to_value(AutomationUpdateParams::default()).unwrap(),
        json!({})
    );
}

#[tokio::test]
async fn delete_soft_deletes() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    Automations::new(build_http(&server))
        .delete(id(AUTOMATION_ID))
        .await
        .unwrap();
}

#[tokio::test]
async fn trigger_returns_the_task_it_ran() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}/trigger")))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "triggered",
            "automation_id": AUTOMATION_ID,
            "task_id": TASK_ID,
            "reason": null,
        })))
        .mount(&server)
        .await;

    let response = Automations::new(build_http(&server))
        .trigger(id(AUTOMATION_ID))
        .await
        .unwrap();

    assert_eq!(response.status, AutomationExecutionStatus::Triggered);
    assert_eq!(response.automation_id, id(AUTOMATION_ID));
    assert_eq!(response.task_id, Some(id(TASK_ID)));
}

#[tokio::test]
async fn a_skipped_trigger_carries_its_reason() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}/trigger")))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "skipped",
            "automation_id": AUTOMATION_ID,
            "task_id": null,
            "reason": "The target task is archived",
        })))
        .mount(&server)
        .await;

    let response = Automations::new(build_http(&server))
        .trigger(id(AUTOMATION_ID))
        .await
        .unwrap();

    assert_eq!(response.status, AutomationExecutionStatus::Skipped);
    assert_eq!(response.task_id, None);
    assert_eq!(
        response.reason.as_deref(),
        Some("The target task is archived")
    );
}

#[tokio::test]
async fn a_403_surfaces_as_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "detail": "Only administrators can access automations",
        })))
        .mount(&server)
        .await;

    let err = Automations::new(build_http(&server))
        .get(id(AUTOMATION_ID))
        .await
        .unwrap_err();

    assert_eq!(err.status(), Some(403));
}

#[tokio::test]
async fn the_client_routes_automations_to_the_data_plane() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/automations/{AUTOMATION_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(automation_json()))
        .expect(1)
        .mount(&server)
        .await;

    let client = IntrospectionClient::new(ClientConfig::with_token("intro_test").advanced(
        AdvancedOptions {
            base_api_url: Some("http://127.0.0.1:9".into()),
            dp_url: Some(server.uri()),
            ..Default::default()
        },
    ))
    .unwrap();

    let automation = client.automations().get(id(AUTOMATION_ID)).await.unwrap();
    assert_eq!(automation.name, "Daily follow-up");
}

#[tokio::test]
async fn automation_events_decode_and_filter_by_automation_and_task() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param(
            "event_name",
            "introspection.automation.triggered",
        ))
        .and(query_param("automation_id", AUTOMATION_ID))
        .and(query_param("task_id", TASK_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(
            vec![json!({
                "id": "evt_1",
                "timestamp": "2026-10-05T22:00:01Z",
                "event_name": "introspection.automation.triggered",
                "runtime_group_id": GROUP_ID,
                "payload": {
                    "automation_id": AUTOMATION_ID,
                    "automation_name": "Daily follow-up",
                    "prompt": "Check in on the open items.",
                    "trigger_type": "cron",
                    "slot": "2026-10-05T22:00:00Z",
                    "task_id": TASK_ID,
                    "posted": true,
                    "member_id": MEMBER_ID,
                    "runtime_group_id": GROUP_ID,
                    "triggered_by_member_id": null,
                },
            })],
            None,
        )))
        .mount(&server)
        .await;

    let events = Events::new(build_http(&server));
    let params = EventListParams {
        automation_id: Some(id(AUTOMATION_ID)),
        task_id: Some(id(TASK_ID)),
        ..EventListParams::new(IntrospectionEventName::AutomationTriggered)
    };
    let found: Vec<_> = events.list(&params).unwrap().collect().await;

    let event = found[0].as_ref().unwrap();
    assert_eq!(
        event.event_name(),
        Some(IntrospectionEventName::AutomationTriggered)
    );
    let Event::AutomationTriggered(triggered) = event else {
        panic!("expected AutomationTriggered, got {event:?}");
    };
    assert_eq!(triggered.payload.automation_id, id(AUTOMATION_ID));
    assert_eq!(triggered.payload.trigger_type, AutomationTriggerType::Cron);
    assert_eq!(
        triggered.payload.slot.as_deref(),
        Some("2026-10-05T22:00:00Z")
    );
    assert_eq!(triggered.payload.task_id, id(TASK_ID));
    assert!(triggered.payload.posted);
    assert_eq!(triggered.payload.member_id, id(MEMBER_ID));
    assert_eq!(triggered.payload.triggered_by_member_id, None);
}

#[tokio::test]
async fn skipped_events_decode_their_reason_including_an_unknown_one() {
    let server = MockServer::start().await;
    let skipped = |reason: &str| {
        json!({
            "id": format!("evt_{reason}"),
            "timestamp": "2026-10-05T22:00:01Z",
            "event_name": "introspection.automation.skipped",
            "payload": {
                "automation_id": AUTOMATION_ID,
                "trigger_type": "manual",
                "slot": "2026-10-05T22:00:00Z",
                "task_id": TASK_ID,
                "reason": reason,
            },
        })
    };
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param(
            "event_name",
            "introspection.automation.skipped",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(
            vec![skipped("target_busy"), skipped("a_reason_from_the_future")],
            None,
        )))
        .mount(&server)
        .await;

    let events = Events::new(build_http(&server));
    let found: Vec<_> = events
        .list(&EventListParams::new(
            IntrospectionEventName::AutomationSkipped,
        ))
        .unwrap()
        .collect()
        .await;

    let reasons: Vec<_> = found
        .iter()
        .map(|event| match event.as_ref().unwrap() {
            Event::AutomationSkipped(skipped) => {
                assert_eq!(skipped.payload.trigger_type, AutomationTriggerType::Manual);
                assert_eq!(skipped.payload.task_id, Some(id(TASK_ID)));
                skipped.payload.reason.clone()
            }
            other => panic!("expected AutomationSkipped, got {other:?}"),
        })
        .collect();
    assert_eq!(
        reasons,
        vec![
            AutomationSkipReason::TargetBusy,
            AutomationSkipReason::Other("a_reason_from_the_future".into()),
        ]
    );
}

#[test]
fn automation_event_names_round_trip() {
    for (name, wire) in [
        (
            IntrospectionEventName::AutomationTriggered,
            "introspection.automation.triggered",
        ),
        (
            IntrospectionEventName::AutomationSkipped,
            "introspection.automation.skipped",
        ),
    ] {
        assert_eq!(name.as_str(), wire);
        assert_eq!(
            serde_json::from_value::<IntrospectionEventName>(json!(wire)).unwrap(),
            name
        );
    }
}
