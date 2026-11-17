//! Integration tests for the routines DP surface (`client.routines()`)
//! and the routine event families, backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use introspection_sdk::api::{HttpClient, HttpConfig, IntrospectionAPIError};
use introspection_sdk::{
    AdvancedOptions, ClientConfig, EnqueuedConversation, Event, EventListParams, Events,
    IntrospectionClient, IntrospectionEventName, ObservationSynthesisStatus, PaginationParams,
    RoutineCondition, RoutineConditionType, RoutineCreateParams, RoutineExecutionStatus,
    RoutineGithubTrigger, RoutineKind, RoutineLinearTrigger, RoutineListParams, RoutineMetadata,
    RoutineParameters, RoutineScheduleTrigger, RoutineSkipReason, RoutineSlackTrigger,
    RoutineTrigger, RoutineTriggerEventRef, RoutineTriggerParams, RoutineTriggerRequest,
    RoutineTriggerType, RoutineUpdateParams, RoutineWebhookTrigger, Routines, TaskFileRef,
    TaskRepoRequest,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ROUTINE_ID: &str = "11111111-1111-7111-8111-111111111111";
const TASK_ID: &str = "22222222-2222-7222-8222-222222222222";
const RUN_ID: &str = "77777777-7777-7777-8777-777777777777";
const GROUP_ID: &str = "33333333-3333-7333-8333-333333333333";
const MEMBER_ID: &str = "44444444-4444-7444-8444-444444444444";
const SCHEDULE_ID: &str = "55555555-5555-7555-8555-555555555555";

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

fn routine_json() -> serde_json::Value {
    json!({
        "id": ROUTINE_ID,
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
        "kind": null,
        "prompt": "Check in on the open items.",
        "routine_metadata": {
            "repositories": [{"repo": "acme/app", "ref": "main"}],
            "conditions": [{"type": "has_new_tasks_since_last_run"}],
            "synthesis_max_conversations": null,
        },
        "triggers": [
            {"id": SCHEDULE_ID, "type": "schedule", "cron": "0 9 * * 1-5", "timezone": "Australia/Sydney"},
        ],
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
async fn list_parses_the_read_model() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/routines"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![routine_json()], None)))
        .mount(&server)
        .await;

    let found: Vec<_> = Routines::new(build_http(&server))
        .list(&RoutineListParams::default())
        .collect()
        .await;

    let routine = found[0].as_ref().unwrap();
    assert_eq!(routine.id, id(ROUTINE_ID));
    assert_eq!(routine.kind, None);
    assert_eq!(routine.task_id, Some(id(TASK_ID)));
    assert_eq!(routine.runtime_group_id, Some(id(GROUP_ID)));
    assert_eq!(routine.created_by_member_id, Some(id(MEMBER_ID)));
    assert!(routine.can_manage);
    assert_eq!(routine.owner_role.as_deref(), Some("operator"));
    assert_eq!(
        routine.next_trigger_at.as_deref(),
        Some("2026-10-05T22:00:00Z")
    );
    let metadata = &routine.routine_metadata;
    assert_eq!(metadata.repositories[0].repo, "acme/app");
    assert_eq!(metadata.repositories[0].git_ref.as_deref(), Some("main"));
    assert_eq!(
        metadata.conditions[0].condition_type,
        RoutineConditionType::HasNewTasksSinceLastRun
    );
    assert_eq!(metadata.synthesis_max_conversations, None);
    assert_eq!(
        routine.triggers,
        vec![RoutineTrigger::Schedule(RoutineScheduleTrigger {
            id: Some(id(SCHEDULE_ID)),
            timezone: Some("Australia/Sydney".into()),
            ..RoutineScheduleTrigger::new("0 9 * * 1-5")
        })]
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
        let mut record = routine_json();
        record["kind"] = json!(kind);
        record["task_id"] = json!(null);
        record["created_by_member_id"] = json!(null);
        records.push(record);
    }
    Mock::given(method("GET"))
        .and(path("/v1/routines"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(records, None)))
        .mount(&server)
        .await;

    let kinds: Vec<_> = Routines::new(build_http(&server))
        .list(&RoutineListParams::default())
        .map(|routine| routine.unwrap().kind)
        .collect()
        .await;

    assert_eq!(
        kinds,
        vec![
            Some(RoutineKind::ProjectCheckIn),
            Some(RoutineKind::ObservationSynthesis),
            Some(RoutineKind::ObservationClustering),
            Some(RoutineKind::Other("a_kind_from_the_future".into())),
        ]
    );
    // An unknown kind round-trips to the value it came from.
    assert_eq!(
        serde_json::to_value(RoutineKind::from("a_kind_from_the_future")).unwrap(),
        json!("a_kind_from_the_future")
    );
}

#[tokio::test]
async fn list_sends_every_filter_and_keeps_task_id_across_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/routines"))
        .and(query_param("next", "cursor-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![routine_json()], None)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/routines"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(page(vec![routine_json()], Some("cursor-2"))),
        )
        .mount(&server)
        .await;

    let params = RoutineListParams {
        pagination: PaginationParams {
            limit: Some(1),
            next: None,
        },
        kind: Some(RoutineKind::ProjectCheckIn),
        enabled: Some(true),
        scheduled: Some(false),
        task_id: Some(id(TASK_ID)),
        filters: None,
    };
    let found: Vec<_> = Routines::new(build_http(&server))
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
        .and(path("/v1/routines"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![], None)))
        .mount(&server)
        .await;

    let _: Vec<_> = Routines::new(build_http(&server))
        .list(&RoutineListParams::default())
        .collect()
        .await;

    let requests = server.received_requests().await.unwrap();
    assert!(query_pairs(&requests[0]).is_empty());
}

#[tokio::test]
async fn get_reads_one_routine() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(routine_json()))
        .mount(&server)
        .await;

    let routine = Routines::new(build_http(&server))
        .get(id(ROUTINE_ID))
        .await
        .unwrap();

    assert_eq!(routine.id, id(ROUTINE_ID));
    assert_eq!(
        routine.prompt.as_deref(),
        Some("Check in on the open items.")
    );
}

#[tokio::test]
async fn create_sends_only_the_set_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/routines"))
        .and(body_json(json!({
            "name": "Remind me",
            "prompt": "Follow up on the invoice.",
            "runtime_group_id": GROUP_ID,
            "task_id": TASK_ID,
            "next_trigger_at": "2026-10-06T09:00:00+10:00",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(routine_json()))
        .mount(&server)
        .await;

    let routine = Routines::new(build_http(&server))
        .create(&RoutineCreateParams {
            prompt: Some("Follow up on the invoice.".into()),
            runtime_group_id: Some(id(GROUP_ID)),
            task_id: Some(id(TASK_ID)),
            next_trigger_at: Some("2026-10-06T09:00:00+10:00".into()),
            ..RoutineCreateParams::new("Remind me")
        })
        .await
        .unwrap();

    assert_eq!(routine.id, id(ROUTINE_ID));
}

#[tokio::test]
async fn create_serializes_a_platform_kind_its_schedules_and_conditions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/routines"))
        .and(body_json(json!({
            "name": "Check-in",
            "kind": "project_check_in",
            "prompt": "How is the project going?",
            "runtime_group_id": GROUP_ID,
            "routine_metadata": {
                "repositories": [{"repo": "acme/app"}],
                "conditions": [
                    {"type": "no_live_task_for_routine"},
                    {"type": "last_run_issues_resolved", "runtime_group_id": GROUP_ID},
                ],
            },
            "enabled": false,
            "triggers": [
                {"type": "schedule", "cron": "0 9 * * *"},
                {"type": "schedule", "cron": "0 9 * * 1-5", "timezone": "UTC"},
            ],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(routine_json()))
        .mount(&server)
        .await;

    Routines::new(build_http(&server))
        .create(&RoutineCreateParams {
            kind: Some(RoutineKind::ProjectCheckIn),
            prompt: Some("How is the project going?".into()),
            runtime_group_id: Some(id(GROUP_ID)),
            routine_metadata: Some(RoutineMetadata {
                repositories: vec![TaskRepoRequest {
                    repo: "acme/app".into(),
                    ..Default::default()
                }],
                conditions: vec![
                    RoutineCondition::new(RoutineConditionType::NoLiveTaskForRoutine),
                    RoutineCondition {
                        runtime_group_id: Some(id(GROUP_ID)),
                        ..RoutineCondition::new(RoutineConditionType::LastRunIssuesResolved)
                    },
                ],
                ..Default::default()
            }),
            enabled: Some(false),
            triggers: Some(vec![
                RoutineScheduleTrigger::new("0 9 * * *").into(),
                RoutineScheduleTrigger {
                    timezone: Some("UTC".into()),
                    ..RoutineScheduleTrigger::new("0 9 * * 1-5")
                }
                .into(),
            ]),
            ..RoutineCreateParams::new("Check-in")
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn update_sends_only_the_set_fields() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .and(body_json(json!({ "enabled": false })))
        .respond_with(ResponseTemplate::new(200).set_body_json(routine_json()))
        .mount(&server)
        .await;

    Routines::new(build_http(&server))
        .update(
            id(ROUTINE_ID),
            &RoutineUpdateParams {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[test]
fn routine_metadata_decodes_and_an_update_sends_the_whole_object() {
    let mut value = routine_json();
    value["kind"] = json!("observation_synthesis");
    value["routine_metadata"] = json!({"synthesis_max_conversations": 50});
    let routine: introspection_sdk::Routine = serde_json::from_value(value).unwrap();
    assert_eq!(
        routine.routine_metadata,
        RoutineMetadata {
            synthesis_max_conversations: Some(50),
            ..Default::default()
        }
    );
    assert_eq!(
        serde_json::to_value(RoutineUpdateParams {
            routine_metadata: Some(RoutineMetadata {
                synthesis_max_conversations: Some(20),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap(),
        json!({ "routine_metadata": {
            "repositories": [],
            "conditions": [],
            "synthesis_max_conversations": 20,
        } })
    );
}

#[test]
fn routine_metadata_keeps_a_key_it_does_not_model() {
    let value = json!({
        "repositories": [],
        "conditions": [],
        "synthesis_max_conversations": 10,
        "review_window_hours": 24,
    });
    let metadata: RoutineMetadata = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(metadata.other["review_window_hours"], json!(24));
    assert_eq!(serde_json::to_value(&metadata).unwrap(), value);
}

#[test]
fn a_routine_without_routine_metadata_reads_the_defaults() {
    let mut value = routine_json();
    value.as_object_mut().unwrap().remove("routine_metadata");
    let routine: introspection_sdk::Routine = serde_json::from_value(value).unwrap();
    assert_eq!(routine.routine_metadata, RoutineMetadata::default());
}

#[test]
fn an_empty_update_serializes_to_an_empty_object() {
    assert_eq!(
        serde_json::to_value(RoutineUpdateParams::default()).unwrap(),
        json!({})
    );
}

const TRIGGER_ID: &str = "66666666-6666-7666-8666-666666666666";

fn webhook_trigger_json() -> serde_json::Value {
    json!({
        "id": TRIGGER_ID,
        "type": "webhook",
        "connector_slug": "ci-alerts",
        "event_types": ["alert.fired"],
        "payload_fields": { "env": ["prod"] },
    })
}

#[tokio::test]
async fn create_sends_a_webhook_trigger() {
    let server = MockServer::start().await;
    let mut created = routine_json();
    created["triggers"] = json!([webhook_trigger_json()]);
    Mock::given(method("POST"))
        .and(path("/v1/routines"))
        .and(body_json(json!({
            "name": "Triage alerts",
            "prompt": "Triage the @parameters.severity alert.",
            "runtime_group_id": GROUP_ID,
            "triggers": [{
                "type": "webhook",
                "connector_slug": "ci-alerts",
                "event_types": ["alert.fired"],
                "payload_fields": { "env": ["prod"] },
            }],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(created))
        .expect(1)
        .mount(&server)
        .await;

    let routine = Routines::new(build_http(&server))
        .create(&RoutineCreateParams {
            prompt: Some("Triage the @parameters.severity alert.".into()),
            runtime_group_id: Some(id(GROUP_ID)),
            triggers: Some(vec![RoutineWebhookTrigger {
                payload_fields: Some([("env".into(), vec!["prod".into()])].into()),
                ..RoutineWebhookTrigger::new("ci-alerts", ["alert.fired"])
            }
            .into()]),
            ..RoutineCreateParams::new("Triage alerts")
        })
        .await
        .unwrap();

    let trigger = &routine.triggers[0];
    assert_eq!(trigger.id(), Some(id(TRIGGER_ID)));
    assert_eq!(trigger.type_name(), Some("webhook"));
    let RoutineTrigger::Webhook(webhook) = trigger else {
        panic!("a webhook trigger: {trigger:?}");
    };
    assert_eq!(webhook.connector_slug, "ci-alerts");
}

#[tokio::test]
async fn update_keeps_a_trigger_by_id_and_adds_another() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .and(body_json(json!({
            "triggers": [
                {
                    "id": TRIGGER_ID,
                    "type": "webhook",
                    "connector_slug": "ci-alerts",
                    "event_types": ["alert.fired"],
                    "payload_fields": { "env": ["prod"] },
                },
                {
                    "type": "github",
                    "event_types": ["github.pull_request.opened"],
                    "branches": ["main"],
                },
            ],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(routine_json()))
        .expect(1)
        .mount(&server)
        .await;

    let stored: RoutineTrigger = serde_json::from_value(webhook_trigger_json()).unwrap();
    Routines::new(build_http(&server))
        .update(
            id(ROUTINE_ID),
            &RoutineUpdateParams {
                triggers: Some(vec![
                    stored,
                    RoutineGithubTrigger {
                        branches: Some(vec!["main".into()]),
                        ..RoutineGithubTrigger::new(["github.pull_request.opened"])
                    }
                    .into(),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[test]
fn an_empty_trigger_list_removes_every_trigger() {
    let params = RoutineUpdateParams {
        triggers: Some(Vec::new()),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        json!({ "triggers": [] })
    );
}

#[test]
fn a_routine_without_triggers_reads_an_empty_list() {
    let mut value = routine_json();
    value.as_object_mut().unwrap().remove("triggers");
    let routine: introspection_sdk::Routine = serde_json::from_value(value).unwrap();
    assert!(routine.triggers.is_empty());
}

#[test]
fn each_trigger_type_decodes_and_serializes_back_unchanged() {
    let cases = [
        (
            json!({"id": TRIGGER_ID, "type": "schedule", "cron": "0 9 * * 1-5", "timezone": "Australia/Sydney"}),
            RoutineTrigger::Schedule(RoutineScheduleTrigger {
                id: Some(id(TRIGGER_ID)),
                timezone: Some("Australia/Sydney".into()),
                ..RoutineScheduleTrigger::new("0 9 * * 1-5")
            }),
        ),
        (
            json!({"id": TRIGGER_ID, "type": "slack", "event_types": ["slack.reaction.added"], "channel_ids": ["C1"], "emoji": ["eyes"]}),
            RoutineTrigger::Slack(RoutineSlackTrigger {
                id: Some(id(TRIGGER_ID)),
                channel_ids: Some(vec!["C1".into()]),
                emoji: Some(vec!["eyes".into()]),
                ..RoutineSlackTrigger::new(["slack.reaction.added"])
            }),
        ),
        (
            json!({"id": TRIGGER_ID, "type": "github", "event_types": ["github.push"], "repository_ids": ["R1"], "branches": ["main"]}),
            RoutineTrigger::Github(RoutineGithubTrigger {
                id: Some(id(TRIGGER_ID)),
                repository_ids: Some(vec!["R1".into()]),
                branches: Some(vec!["main".into()]),
                ..RoutineGithubTrigger::new(["github.push"])
            }),
        ),
        (
            json!({"id": TRIGGER_ID, "type": "linear", "event_types": ["linear.issue.created"], "to_states": ["Todo"]}),
            RoutineTrigger::Linear(RoutineLinearTrigger {
                id: Some(id(TRIGGER_ID)),
                to_states: Some(vec!["Todo".into()]),
                ..RoutineLinearTrigger::new(["linear.issue.created"])
            }),
        ),
        (
            webhook_trigger_json(),
            RoutineTrigger::Webhook(RoutineWebhookTrigger {
                id: Some(id(TRIGGER_ID)),
                payload_fields: Some([("env".into(), vec!["prod".into()])].into()),
                ..RoutineWebhookTrigger::new("ci-alerts", ["alert.fired"])
            }),
        ),
    ];
    for (value, expected) in cases {
        let trigger: RoutineTrigger = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(trigger, expected);
        assert_eq!(trigger.id(), Some(id(TRIGGER_ID)));
        assert_eq!(serde_json::to_value(&trigger).unwrap(), value);
    }
}

#[test]
fn omitted_filter_keys_decode_as_none_and_are_not_sent() {
    let value = json!({"id": TRIGGER_ID, "type": "slack", "event_types": ["slack.reaction.added"]});
    let trigger: RoutineTrigger = serde_json::from_value(value.clone()).unwrap();
    let RoutineTrigger::Slack(slack) = &trigger else {
        panic!("a slack trigger: {trigger:?}");
    };
    assert_eq!(slack.channel_ids, None);
    assert_eq!(slack.emoji, None);
    assert_eq!(serde_json::to_value(&trigger).unwrap(), value);
    assert_eq!(
        serde_json::to_value(RoutineTrigger::from(RoutineScheduleTrigger::new(
            "0 9 * * *"
        )))
        .unwrap(),
        json!({"type": "schedule", "cron": "0 9 * * *"})
    );
}

#[test]
fn an_unknown_trigger_type_is_kept() {
    let value = json!({
        "id": TRIGGER_ID,
        "type": "jira",
        "event_types": ["jira.issue.created"],
        "project_keys": ["OPS"],
    });
    let trigger: RoutineTrigger = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(trigger, RoutineTrigger::Unknown(value.clone()));
    assert_eq!(trigger.type_name(), Some("jira"));
    assert_eq!(trigger.id(), Some(id(TRIGGER_ID)));
    assert_eq!(serde_json::to_value(&trigger).unwrap(), value);
}

#[tokio::test]
async fn a_routine_with_an_unknown_trigger_type_decodes() {
    let server = MockServer::start().await;
    let mut value = routine_json();
    value["triggers"] = json!([
        {"id": TRIGGER_ID, "type": "introspection", "event_types": ["introspection.task.completed"]},
    ]);
    Mock::given(method("GET"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .mount(&server)
        .await;

    let routine = Routines::new(build_http(&server))
        .get(id(ROUTINE_ID))
        .await
        .unwrap();
    assert_eq!(routine.triggers[0].type_name(), Some("introspection"));
    assert_eq!(routine.triggers[0].id(), Some(id(TRIGGER_ID)));
}

#[tokio::test]
async fn delete_soft_deletes() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    Routines::new(build_http(&server))
        .delete(id(ROUTINE_ID))
        .await
        .unwrap();
}

#[tokio::test]
async fn trigger_returns_the_task_it_ran() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}/trigger")))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "triggered",
            "routine_id": ROUTINE_ID,
            "task_id": TASK_ID,
            "reason": null,
        })))
        .mount(&server)
        .await;

    let response = Routines::new(build_http(&server))
        .trigger(id(ROUTINE_ID), &RoutineTriggerParams::default())
        .await
        .unwrap();

    assert_eq!(response.status, RoutineExecutionStatus::Triggered);
    assert_eq!(response.routine_id, id(ROUTINE_ID));
    assert_eq!(response.task_id, Some(id(TASK_ID)));
    assert_eq!(response.run_id, None);
    assert_eq!(response.result, None);
    assert_eq!(response.synthesis_result(), None);
}

#[tokio::test]
async fn trigger_sends_an_empty_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}/trigger")))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "triggered",
            "routine_id": ROUTINE_ID,
        })))
        .expect(2)
        .mount(&server)
        .await;

    let routines = Routines::new(build_http(&server));
    routines
        .trigger(id(ROUTINE_ID), &RoutineTriggerParams::default())
        .await
        .unwrap();
    routines
        .trigger_with(id(ROUTINE_ID), &RoutineTriggerParams::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn trigger_sends_the_conversation_ids_inline_without_an_upload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}/trigger")))
        .and(body_json(
            json!({ "parameters": { "conversation_ids": ["conv-a", "conv-b"] } }),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "triggered",
            "routine_id": ROUTINE_ID,
            "task_id": null,
            "run_id": null,
            "reason": null,
            "result": {
                "enqueued": 2,
                "conversations": [
                    { "conversation_id": "conv-a", "completed_at": "2026-11-12T08:59:00Z" },
                    { "conversation_id": "conv-b", "completed_at": "2026-11-12T08:58:00Z" },
                ],
            },
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = Routines::new(build_http(&server))
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams::conversation_ids(["conv-a", "conv-b"]),
        )
        .await
        .unwrap();

    let result = response.synthesis_result().unwrap();
    assert_eq!(result.enqueued, Some(2));
    assert_eq!(
        result.conversations,
        Some(vec![
            EnqueuedConversation {
                conversation_id: "conv-a".into(),
                completed_at: "2026-11-12T08:59:00Z".into(),
            },
            EnqueuedConversation {
                conversation_id: "conv-b".into(),
                completed_at: "2026-11-12T08:58:00Z".into(),
            },
        ])
    );
}

const FILE_ID: &str = "66666666-6666-7666-8666-666666666666";
const EXTRA_FILE_ID: &str = "77777777-7777-7777-8777-777777777777";

fn uploaded_file_json(name: &str) -> serde_json::Value {
    json!({
        "id": FILE_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": "00000000-0000-0000-0000-0000000000bb",
        "created_at": "2026-11-15T00:00:00Z",
        "updated_at": "2026-11-15T00:00:00Z",
        "name": name,
        "storage_path": format!("/storage/{name}"),
        "mime_type": "application/json",
        "size_bytes": 40,
        "expires_at": "2026-12-15T00:00:00Z",
    })
}

fn triggered_json() -> serde_json::Value {
    json!({
        "status": "triggered",
        "routine_id": ROUTINE_ID,
        "task_id": TASK_ID,
        "run_id": null,
        "reason": null,
        "result": null,
    })
}

fn parameters() -> serde_json::Map<String, serde_json::Value> {
    let serde_json::Value::Object(parameters) = json!({ "severity": "high", "alert": { "id": 7 } })
    else {
        unreachable!()
    };
    parameters
}

async fn mount_upload(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .respond_with(|request: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let name = body["name"].as_str().unwrap().to_string();
            ResponseTemplate::new(201).set_body_json(uploaded_file_json(&name))
        })
        .mount(server)
        .await;
}

async fn mount_trigger(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}/trigger")))
        .respond_with(ResponseTemplate::new(202).set_body_json(triggered_json()))
        .mount(server)
        .await;
}

async fn mount_routine(server: &MockServer, routine: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(routine))
        .mount(server)
        .await;
}

async fn reads(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.method.as_str() == "GET")
        .count()
}

async fn bodies(server: &MockServer, route: &str) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == route)
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

#[tokio::test]
async fn trigger_attaching_explicitly_uploads_without_reading_the_routine() {
    let server = MockServer::start().await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    let routines = Routines::new(build_http(&server));
    let params = RoutineTriggerParams {
        files: vec![TaskFileRef {
            id: id(EXTRA_FILE_ID),
            name: Some("inputs/report.csv".into()),
            size_bytes: None,
        }],
        attach_parameters_file: Some(true),
        ..RoutineTriggerParams::with_parameters(parameters())
    };
    let response = routines.trigger(id(ROUTINE_ID), &params).await.unwrap();
    assert_eq!(response.task_id, Some(id(TASK_ID)));
    routines.trigger(id(ROUTINE_ID), &params).await.unwrap();

    let uploads = bodies(&server, "/v1/files").await;
    assert_eq!(uploads.len(), 2);
    for upload in &uploads {
        let name = upload["name"].as_str().unwrap();
        let stem = name
            .strip_prefix("routine-parameters/")
            .and_then(|rest| rest.strip_suffix(".json"))
            .unwrap_or_else(|| panic!("unexpected name {name}"));
        Uuid::parse_str(stem).unwrap();
        assert_eq!(upload["mime_type"], "application/json");
        assert_eq!(upload["ttl_seconds"], 2_592_000);
        let content: serde_json::Value =
            serde_json::from_str(upload["content"].as_str().unwrap()).unwrap();
        assert_eq!(content, serde_json::Value::Object(parameters()));
    }
    assert_ne!(uploads[0]["name"], uploads[1]["name"]);
    assert_eq!(reads(&server).await, 0);

    let triggers = bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await;
    assert_eq!(triggers.len(), 2);
    assert_eq!(
        triggers[0],
        json!({
            "parameters": { "severity": "high", "alert": { "id": 7 } },
            "files": [
                { "id": FILE_ID, "name": "parameters.json" },
                { "id": EXTRA_FILE_ID, "name": "inputs/report.csv" },
            ],
        })
    );
}

#[tokio::test]
async fn trigger_with_a_parameters_file_sends_only_its_reference() {
    let server = MockServer::start().await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    let uploaded: introspection_sdk::File =
        serde_json::from_value(uploaded_file_json("inputs/params.json")).unwrap();
    let routines = Routines::new(build_http(&server));
    // An id, an uploaded `File`, and an explicit attach choice the file path ignores.
    for params in [
        RoutineTriggerParams::with_parameters(id(EXTRA_FILE_ID)),
        RoutineTriggerParams::with_parameters(&uploaded),
        RoutineTriggerParams {
            parameters: Some(RoutineParameters::file(id(EXTRA_FILE_ID))),
            files: vec![TaskFileRef {
                id: id(FILE_ID),
                name: Some("inputs/report.csv".into()),
                size_bytes: None,
            }],
            attach_parameters_file: Some(true),
        },
    ] {
        routines.trigger(id(ROUTINE_ID), &params).await.unwrap();
    }

    assert!(bodies(&server, "/v1/files").await.is_empty());
    assert_eq!(reads(&server).await, 0);
    assert_eq!(
        bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await,
        vec![
            json!({ "files": [{ "id": EXTRA_FILE_ID, "name": "parameters.json" }] }),
            json!({ "files": [{ "id": FILE_ID, "name": "parameters.json" }] }),
            json!({ "files": [
                { "id": EXTRA_FILE_ID, "name": "parameters.json" },
                { "id": FILE_ID, "name": "inputs/report.csv" },
            ] }),
        ]
    );
}

#[tokio::test]
async fn trigger_without_parameters_uploads_nothing() {
    let server = MockServer::start().await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    let routines = Routines::new(build_http(&server));
    routines
        .trigger(id(ROUTINE_ID), &RoutineTriggerParams::default())
        .await
        .unwrap();
    routines
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams {
                files: vec![TaskFileRef {
                    id: id(EXTRA_FILE_ID),
                    name: None,
                    size_bytes: Some(12),
                }],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert!(bodies(&server, "/v1/files").await.is_empty());
    assert_eq!(reads(&server).await, 0);
    assert_eq!(
        bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await,
        vec![
            json!({}),
            json!({ "files": [{ "id": EXTRA_FILE_ID, "size_bytes": 12 }] }),
        ]
    );
}

#[tokio::test]
async fn trigger_by_default_reads_a_prompt_routine_and_uploads() {
    let server = MockServer::start().await;
    let mut own_task = routine_json();
    own_task["task_id"] = json!(null);
    mount_routine(&server, own_task).await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    Routines::new(build_http(&server))
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams::with_parameters(parameters()),
        )
        .await
        .unwrap();

    assert_eq!(reads(&server).await, 1);
    assert_eq!(bodies(&server, "/v1/files").await.len(), 1);
    assert_eq!(
        bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await,
        vec![json!({
            "parameters": { "severity": "high", "alert": { "id": 7 } },
            "files": [{ "id": FILE_ID, "name": "parameters.json" }],
        })]
    );
}

#[tokio::test]
async fn trigger_by_default_sends_a_platform_or_task_routines_parameters_inline() {
    let inline = json!({ "parameters": { "severity": "high", "alert": { "id": 7 } } });
    let mut platform = routine_json();
    platform["kind"] = json!("observation_synthesis");
    platform["task_id"] = json!(null);
    // `routine_json()` posts into an existing task.
    for routine in [platform, routine_json()] {
        let server = MockServer::start().await;
        mount_routine(&server, routine).await;
        mount_upload(&server).await;
        mount_trigger(&server).await;

        Routines::new(build_http(&server))
            .trigger(
                id(ROUTINE_ID),
                &RoutineTriggerParams::with_parameters(parameters()),
            )
            .await
            .unwrap();

        assert_eq!(reads(&server).await, 1);
        assert!(bodies(&server, "/v1/files").await.is_empty());
        assert_eq!(
            bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await,
            vec![inline.clone()]
        );
    }
}

#[tokio::test]
async fn trigger_not_attaching_explicitly_reads_and_uploads_nothing() {
    let server = MockServer::start().await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    let routine: introspection_sdk::Routine = serde_json::from_value(routine_json()).unwrap();
    assert!(!routine.takes_files());
    let routines = Routines::new(build_http(&server));
    routines
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams::for_routine(&routine, parameters()),
        )
        .await
        .unwrap();
    routines
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams {
                attach_parameters_file: Some(false),
                ..RoutineTriggerParams::with_parameters(parameters())
            },
        )
        .await
        .unwrap();

    assert_eq!(reads(&server).await, 0);
    assert!(bodies(&server, "/v1/files").await.is_empty());
    let inline = json!({ "parameters": { "severity": "high", "alert": { "id": 7 } } });
    assert_eq!(
        bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await,
        vec![inline.clone(), inline]
    );

    let mut own_task = routine_json();
    own_task["task_id"] = json!(null);
    let own_task: introspection_sdk::Routine = serde_json::from_value(own_task).unwrap();
    assert!(own_task.takes_files());
    assert_eq!(
        RoutineTriggerParams::for_routine(&own_task, parameters()).attach_parameters_file,
        Some(true)
    );
}

fn parameters_on_disk(contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("routine-parameters-{}.json", Uuid::now_v7()));
    std::fs::write(&path, contents).unwrap();
    path
}

#[tokio::test]
async fn trigger_with_a_local_file_sends_it_as_inline_parameters() {
    let server = MockServer::start().await;
    let mut own_task = routine_json();
    own_task["task_id"] = json!(null);
    mount_routine(&server, own_task).await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    let path = parameters_on_disk(r#"{"severity": "high", "alert": {"id": 7}}"#);
    let routines = Routines::new(build_http(&server));
    routines
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams::with_parameters(path.clone()),
        )
        .await
        .unwrap();
    routines
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams {
                attach_parameters_file: Some(false),
                ..RoutineTriggerParams::with_parameters(path.clone())
            },
        )
        .await
        .unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(reads(&server).await, 1);
    let uploads = bodies(&server, "/v1/files").await;
    assert_eq!(uploads.len(), 1);
    let content: serde_json::Value =
        serde_json::from_str(uploads[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(content, serde_json::Value::Object(parameters()));
    let inline = json!({ "severity": "high", "alert": { "id": 7 } });
    assert_eq!(
        bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger")).await,
        vec![
            json!({
                "parameters": inline,
                "files": [{ "id": FILE_ID, "name": "parameters.json" }],
            }),
            json!({ "parameters": inline }),
        ]
    );
}

#[tokio::test]
async fn a_local_file_that_is_not_a_json_object_is_refused_before_any_request() {
    let server = MockServer::start().await;
    mount_upload(&server).await;
    mount_trigger(&server).await;

    let routines = Routines::new(build_http(&server));
    for contents in ["[1, 2]", "severity: high"] {
        let path = parameters_on_disk(contents);
        let err = routines
            .trigger(
                id(ROUTINE_ID),
                &RoutineTriggerParams::with_parameters(path.clone()),
            )
            .await
            .unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(
            matches!(err, IntrospectionAPIError::InvalidConfig(_)),
            "{err}"
        );
    }
    let missing = std::env::temp_dir().join(format!("missing-{}.json", Uuid::now_v7()));
    let err = routines
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams::with_parameters(missing),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, IntrospectionAPIError::Io(_)), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_upload_does_not_trigger() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .respond_with(ResponseTemplate::new(413).set_body_json(json!({ "detail": "too large" })))
        .mount(&server)
        .await;
    mount_trigger(&server).await;

    let err = Routines::new(build_http(&server))
        .trigger(
            id(ROUTINE_ID),
            &RoutineTriggerParams {
                attach_parameters_file: Some(true),
                ..RoutineTriggerParams::with_parameters(parameters())
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(413));
    assert!(
        bodies(&server, &format!("/v1/routines/{ROUTINE_ID}/trigger"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_clustering_trigger_returns_its_run() {
    const RUN_ID: &str = "55555555-5555-7555-8555-555555555555";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}/trigger")))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "triggered",
            "routine_id": ROUTINE_ID,
            "task_id": null,
            "reason": null,
            "run_id": RUN_ID,
        })))
        .mount(&server)
        .await;

    let response = Routines::new(build_http(&server))
        .trigger(id(ROUTINE_ID), &RoutineTriggerParams::default())
        .await
        .unwrap();

    assert_eq!(response.run_id, Some(id(RUN_ID)));
    assert_eq!(response.synthesis_result(), None);
}

#[test]
fn the_trigger_body_refuses_an_unknown_field() {
    // The old top-level `conversation_ids` moved under `parameters`.
    let err = serde_json::from_value::<RoutineTriggerRequest>(json!({
        "conversation_ids": ["conv-a"],
    }))
    .unwrap_err();
    assert!(err.to_string().contains("conversation_ids"), "{err}");
}

#[tokio::test]
async fn a_skipped_trigger_carries_its_reason() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}/trigger")))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "status": "skipped",
            "routine_id": ROUTINE_ID,
            "task_id": null,
            "reason": "The target task is archived",
        })))
        .mount(&server)
        .await;

    let response = Routines::new(build_http(&server))
        .trigger(id(ROUTINE_ID), &RoutineTriggerParams::default())
        .await
        .unwrap();

    assert_eq!(response.status, RoutineExecutionStatus::Skipped);
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
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "detail": "Only administrators can access routines",
        })))
        .mount(&server)
        .await;

    let err = Routines::new(build_http(&server))
        .get(id(ROUTINE_ID))
        .await
        .unwrap_err();

    assert_eq!(err.status(), Some(403));
}

#[tokio::test]
async fn the_client_routes_routines_to_the_data_plane() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/routines/{ROUTINE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(routine_json()))
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

    let routine = client.routines().get(id(ROUTINE_ID)).await.unwrap();
    assert_eq!(routine.name, "Daily follow-up");
}

#[tokio::test]
async fn routine_events_decode_and_filter_by_routine_and_task() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param(
            "event_name",
            "introspection.routines.triggered",
        ))
        .and(query_param("routine_id", ROUTINE_ID))
        .and(query_param("task_id", TASK_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(
            vec![json!({
                "id": "evt_1",
                "timestamp": "2026-10-05T22:00:01Z",
                "event_name": "introspection.routines.triggered",
                "runtime_group_id": GROUP_ID,
                "payload": {
                    "routine_id": ROUTINE_ID,
                    "routine_name": "Daily follow-up",
                    "prompt": "Check in on the open items.",
                    "trigger_type": "cron",
                    "slot": "2026-10-05T22:00:00Z",
                    "task_id": TASK_ID,
                    "posted": true,
                    "member_id": MEMBER_ID,
                    "runtime_group_id": GROUP_ID,
                    "triggered_by_member_id": null,
                    "run_id": RUN_ID,
                    "event": {
                        "trigger_id": TRIGGER_ID,
                        "event_type": "alert.fired",
                        "delivery_id": "webhook:msg_1",
                    },
                },
            })],
            None,
        )))
        .mount(&server)
        .await;

    let events = Events::new(build_http(&server));
    let params = EventListParams {
        routine_id: Some(id(ROUTINE_ID)),
        task_id: Some(id(TASK_ID)),
        ..EventListParams::new(IntrospectionEventName::RoutineTriggered)
    };
    let found: Vec<_> = events.list(&params).unwrap().collect().await;

    let event = found[0].as_ref().unwrap();
    assert_eq!(
        event.event_name(),
        Some(IntrospectionEventName::RoutineTriggered)
    );
    let Event::RoutineTriggered(triggered) = event else {
        panic!("expected RoutineTriggered, got {event:?}");
    };
    assert_eq!(triggered.payload.routine_id, id(ROUTINE_ID));
    assert_eq!(triggered.payload.trigger_type, RoutineTriggerType::Cron);
    assert_eq!(
        triggered.payload.slot.as_deref(),
        Some("2026-10-05T22:00:00Z")
    );
    assert_eq!(triggered.payload.task_id, id(TASK_ID));
    assert!(triggered.payload.posted);
    assert_eq!(triggered.payload.member_id, id(MEMBER_ID));
    assert_eq!(triggered.payload.triggered_by_member_id, None);
    assert_eq!(triggered.payload.run_id, Some(id(RUN_ID)));
    assert_eq!(
        triggered.payload.event,
        Some(RoutineTriggerEventRef {
            trigger_id: id(TRIGGER_ID),
            event_type: "alert.fired".into(),
            delivery_id: "webhook:msg_1".into(),
        })
    );
}

#[tokio::test]
async fn skipped_events_decode_their_reason_including_an_unknown_one() {
    let server = MockServer::start().await;
    let skipped = |reason: &str| {
        json!({
            "id": format!("evt_{reason}"),
            "timestamp": "2026-10-05T22:00:01Z",
            "event_name": "introspection.routines.skipped",
            "payload": {
                "routine_id": ROUTINE_ID,
                "trigger_type": "manual",
                "slot": "2026-10-05T22:00:00Z",
                "task_id": TASK_ID,
                "reason": reason,
            },
        })
    };
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param("event_name", "introspection.routines.skipped"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(
            vec![
                skipped("target_busy"),
                skipped("trigger_busy"),
                skipped("a_reason_from_the_future"),
            ],
            None,
        )))
        .mount(&server)
        .await;

    let events = Events::new(build_http(&server));
    let found: Vec<_> = events
        .list(&EventListParams::new(
            IntrospectionEventName::RoutineSkipped,
        ))
        .unwrap()
        .collect()
        .await;

    let reasons: Vec<_> = found
        .iter()
        .map(|event| match event.as_ref().unwrap() {
            Event::RoutineSkipped(skipped) => {
                assert_eq!(skipped.payload.trigger_type, RoutineTriggerType::Manual);
                assert_eq!(skipped.payload.task_id, Some(id(TASK_ID)));
                skipped.payload.reason.clone()
            }
            other => panic!("expected RoutineSkipped, got {other:?}"),
        })
        .collect();
    assert_eq!(
        reasons,
        vec![
            RoutineSkipReason::TargetBusy,
            RoutineSkipReason::TriggerBusy,
            RoutineSkipReason::Other("a_reason_from_the_future".into()),
        ]
    );
}

#[test]
fn routine_event_names_round_trip() {
    for (name, wire) in [
        (
            IntrospectionEventName::RoutineTriggered,
            "introspection.routines.triggered",
        ),
        (
            IntrospectionEventName::RoutineSkipped,
            "introspection.routines.skipped",
        ),
    ] {
        assert_eq!(name.as_str(), wire);
        assert_eq!(
            serde_json::from_value::<IntrospectionEventName>(json!(wire)).unwrap(),
            name
        );
    }
}

#[tokio::test]
async fn synthesis_completed_events_list_by_conversation() {
    let server = MockServer::start().await;
    let completed = json!({
        "id": "evt_completed",
        "timestamp": "2026-11-12T09:00:00Z",
        "event_name": "introspection.observation_synthesis.completed",
        "conversation_id": "conv-a",
        "payload": {
            "status": "completed",
            "observation_count": 0,
            "skipped_reason": "no_observations",
            "conversation_completed_at": "2026-11-12T08:59:00Z",
        },
    });
    let failed = json!({
        "id": "evt_failed",
        "timestamp": "2026-11-12T09:00:01Z",
        "event_name": "introspection.observation_synthesis.completed",
        "conversation_id": "conv-b",
        "payload": {
            "status": "failed",
            "observation_count": 0,
            "error": "model timed out",
        },
    });
    Mock::given(method("GET"))
        .and(path("/v1/events"))
        .and(query_param(
            "event_name",
            "introspection.observation_synthesis.completed",
        ))
        .and(query_param("conversation_ids", "conv-a"))
        .and(query_param("conversation_ids", "conv-b"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page(vec![completed.clone(), failed.clone()], None)),
        )
        .expect(1)
        .mount(&server)
        .await;

    let found: Vec<_> = Events::new(build_http(&server))
        .list(&EventListParams {
            conversation_ids: Some(vec!["conv-a".into(), "conv-b".into()]),
            ..EventListParams::new(IntrospectionEventName::ObservationSynthesisCompleted)
        })
        .unwrap()
        .collect()
        .await;

    let events: Vec<Event> = found.into_iter().map(Result::unwrap).collect();
    let [Event::ObservationSynthesisCompleted(first), Event::ObservationSynthesisCompleted(second)] =
        &events[..]
    else {
        panic!("expected two synthesis events, got {events:?}");
    };
    assert_eq!(first.conversation_id.as_deref(), Some("conv-a"));
    assert_eq!(first.payload.status, ObservationSynthesisStatus::Completed);
    assert_eq!(first.payload.observation_count, 0);
    assert_eq!(
        first.payload.skipped_reason.as_deref(),
        Some("no_observations")
    );
    assert_eq!(second.payload.status, ObservationSynthesisStatus::Failed);
    assert_eq!(
        first.payload.conversation_completed_at.as_deref(),
        Some("2026-11-12T08:59:00Z")
    );
    assert_eq!(second.payload.error.as_deref(), Some("model timed out"));
    // An event from a server that predates the field still decodes.
    assert_eq!(second.payload.conversation_completed_at, None);
    assert_eq!(
        events[0].event_name(),
        Some(IntrospectionEventName::ObservationSynthesisCompleted)
    );
    // Both serialise back to what the server sent.
    assert_eq!(serde_json::to_value(&events[0]).unwrap(), completed);
    assert_eq!(serde_json::to_value(&events[1]).unwrap(), failed);
}

#[test]
fn a_synthesis_status_from_the_future_is_kept() {
    let event: Event = serde_json::from_value(json!({
        "id": "evt_future",
        "timestamp": "2026-11-12T09:00:00Z",
        "event_name": "introspection.observation_synthesis.completed",
        "payload": { "status": "cancelled" },
    }))
    .unwrap();
    let Event::ObservationSynthesisCompleted(event) = event else {
        panic!("expected a synthesis event, got {event:?}");
    };
    assert_eq!(
        event.payload.status,
        ObservationSynthesisStatus::Unknown(json!("cancelled"))
    );
    assert_eq!(event.payload.observation_count, 0);
}

#[test]
fn a_failed_clustering_run_decodes() {
    let wire = json!({
        "id": "evt_run",
        "timestamp": "2026-11-12T09:00:00Z",
        "event_name": "introspection.observation_clustering.run",
        "payload": {
            "run_id": "run_1",
            "lens": "friction",
            "status": "failed",
            "trigger": "manual",
            "error": "persisting the lens failed",
        },
    });
    let event: Event = serde_json::from_value(wire.clone()).unwrap();
    let Event::ObservationClusteringRun(run) = &event else {
        panic!("expected a clustering run, got {event:?}");
    };
    assert_eq!(run.payload.status.as_deref(), Some("failed"));
    assert_eq!(
        run.payload.error.as_deref(),
        Some("persisting the lens failed")
    );
    assert_eq!(serde_json::to_value(&event).unwrap(), wire);
}

#[test]
fn synthesis_completed_event_name_round_trips() {
    let name = IntrospectionEventName::ObservationSynthesisCompleted;
    assert_eq!(
        name.as_str(),
        "introspection.observation_synthesis.completed"
    );
    assert_eq!(
        serde_json::from_value::<IntrospectionEventName>(json!(name.as_str())).unwrap(),
        name
    );
}

#[test]
fn a_schedules_idle_window_is_left_out_cleared_or_set() {
    let unset = serde_json::to_value(RoutineScheduleTrigger::new("0 9 * * *")).unwrap();
    assert!(unset.get("member_idle_seconds").is_none());

    let cleared =
        serde_json::to_value(RoutineScheduleTrigger::new("0 9 * * *").member_idle_seconds(None))
            .unwrap();
    assert_eq!(cleared["member_idle_seconds"], serde_json::Value::Null);

    let set = serde_json::to_value(
        RoutineScheduleTrigger::new("0 */6 * * *").member_idle_seconds(Some(172_800)),
    )
    .unwrap();
    assert_eq!(set["member_idle_seconds"], 172_800);

    let read: RoutineScheduleTrigger =
        serde_json::from_value(json!({ "cron": "0 9 * * *", "member_idle_seconds": 3600 }))
            .unwrap();
    assert_eq!(read.member_idle_seconds, Some(Some(3600)));
    assert_eq!(
        serde_json::from_value::<RoutineSkipReason>(json!("member_idle")).unwrap(),
        RoutineSkipReason::MemberIdle
    );
}
