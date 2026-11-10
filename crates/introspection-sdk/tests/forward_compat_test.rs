//! An SDK built before the API adds a response field or an enum value still
//! reads the response. `strict-schema` reports unknown fields instead, so the
//! tests that go through a client's decode run only without it.

use introspection_sdk::{
    AnnotationState, ExperimentGoal, ExperimentGoalComponent, ExperimentGoalDirection,
    FeatureFlagValue, IntegrationRepositories, Me, Paginated, RepositoryContent, ResourceShare,
    ShareResourceType, Tag, TagKind, Task, TaskKind, TaskStatus,
};
use serde_json::{json, Value};

const TASK_ID: &str = "019fc000-0000-7000-8000-000000000001";
const ORG_ID: &str = "019fc000-0000-7000-8000-000000000002";
const PROJECT_ID: &str = "019fc000-0000-7000-8000-000000000003";
const MEMBER_ID: &str = "019fc000-0000-7000-8000-000000000004";

fn annotation_page() -> Value {
    json!({
        "records": [{
            "trace_id": "0af7651916cd43dd8448eb211c80319c",
            "span_id": "b7ad6b7169203331",
            "labels": ["needs-review"],
            "assignee_member_ids": [],
            "reviewer_member_ids": [],
            "comment_count": 0,
            "updated_at": "2026-08-25T12:00:00Z",
            "updated_by_member_id": MEMBER_ID,
            "priority": "high",
            "review": {"stage": "second-pass", "due_at": "2026-09-01T00:00:00Z"}
        }],
        "count": 1,
        "next": null,
        "facets": {"label": [{"value": "needs-review", "count": 1}]}
    })
}

fn task() -> Value {
    json!({
        "id": TASK_ID,
        "org_id": ORG_ID,
        "project_id": PROJECT_ID,
        "created_at": "2026-08-25T12:00:00Z",
        "updated_at": "2026-08-25T12:00:00Z",
        "kind": "batch",
        "status": "paused",
        "agent": {"sandbox_status": "warm", "session_id": "s-1", "region": "eu-west-1"},
        "budget": {"max_tokens": 1000}
    })
}

#[test]
fn an_annotation_page_ignores_unknown_fields_at_every_level() {
    let page: Paginated<AnnotationState> = serde_json::from_value(annotation_page()).unwrap();
    assert_eq!(page.count, 1);
    assert_eq!(page.records[0].labels, vec!["needs-review"]);
}

#[test]
fn a_task_ignores_unknown_fields_and_keeps_unknown_enum_values() {
    let task: Task = serde_json::from_value(task()).unwrap();
    assert_eq!(task.kind, TaskKind::Other("batch".into()));
    assert_eq!(task.status, TaskStatus::from("paused"));
    assert_eq!(task.status.as_str(), "paused");
    assert_eq!(task.agent.unwrap().sandbox_status.as_deref(), Some("warm"));
}

#[test]
fn a_share_keeps_a_resource_type_this_build_predates() {
    let share: ResourceShare = serde_json::from_value(json!({
        "id": TASK_ID,
        "org_id": ORG_ID,
        "project_id": PROJECT_ID,
        "created_at": "2026-08-25T12:00:00Z",
        "updated_at": "2026-08-25T12:00:00Z",
        "resource_type": "dataset",
        "resource_id": "dataset_1",
        "created_by_member_id": MEMBER_ID
    }))
    .unwrap();
    assert_eq!(
        share.resource_type,
        ShareResourceType::Other("dataset".into())
    );
    assert_eq!(
        serde_json::to_value(&share.resource_type).unwrap(),
        json!("dataset")
    );
}

#[test]
fn a_tag_reads_a_kind_this_build_predates() {
    let tag: Tag = serde_json::from_value(json!({
        "kind": "system",
        "name": "internal",
        "created_at": "2026-08-25T12:00:00Z",
        "updated_at": "2026-08-25T12:00:00Z"
    }))
    .unwrap();
    assert_eq!(tag.kind, TagKind::Unknown);
}

#[test]
fn an_experiment_goal_reads_a_direction_and_a_source_this_build_predates() {
    let component = json!({"source": "rubric", "rubric_id": "r-1", "weight": 2.0});
    let goal: ExperimentGoal = serde_json::from_value(json!({
        "kind": "composite",
        "direction": "target",
        "components": [component.clone()]
    }))
    .unwrap();
    assert_eq!(goal.direction, ExperimentGoalDirection::Unknown);
    let ExperimentGoalComponent::Unknown(raw) = &goal.components[0] else {
        panic!(
            "expected an unknown component, got {:?}",
            goal.components[0]
        );
    };
    assert_eq!(raw, &component);
}

#[test]
fn repository_contents_and_integration_repositories_read_a_type_this_build_predates() {
    let symlink = json!({"type": "symlink", "path": "docs", "target": "../docs"});
    let content: RepositoryContent = serde_json::from_value(symlink.clone()).unwrap();
    assert!(matches!(content, RepositoryContent::Unknown(ref raw) if raw == &symlink));

    let gitlab = json!({"type": "gitlab", "records": []});
    let repos: IntegrationRepositories = serde_json::from_value(gitlab).unwrap();
    assert!(repos.github_records().is_none());
}

#[test]
fn a_feature_flag_of_any_json_shape_reads() {
    let me: Me = serde_json::from_value(json!({
        "feature_flags": {
            "evals": true,
            "layout": {"variant": "b"},
            "unset": null
        }
    }))
    .unwrap();
    assert_eq!(me.feature_flags["evals"], FeatureFlagValue::Bool(true));
    assert_eq!(
        me.feature_flags["layout"],
        FeatureFlagValue::Json(json!({"variant": "b"}))
    );
    assert_eq!(
        me.feature_flags["unset"],
        FeatureFlagValue::Json(Value::Null)
    );
}

#[cfg(not(feature = "strict-schema"))]
mod through_the_client {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use futures::StreamExt;
    use introspection_sdk::api::{HttpClient, HttpConfig, Tasks};
    use introspection_sdk::{AnnotationListParams, Annotations, TaskKind};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{annotation_page, task, TASK_ID};

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

    #[tokio::test]
    async fn an_annotation_list_with_unknown_fields_decodes() {
        let dp = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/annotations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(annotation_page()))
            .mount(&dp)
            .await;
        let mut stream = Annotations::new(http(&dp), http(&dp))
            .list(&AnnotationListParams::default())
            .await
            .unwrap();
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.labels, vec!["needs-review"]);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn a_task_with_unknown_fields_and_enum_values_decodes() {
        let dp = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/v1/tasks/{TASK_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(task()))
            .mount(&dp)
            .await;
        let task = Tasks::new(http(&dp)).get(TASK_ID).await.unwrap();
        assert_eq!(task.kind, TaskKind::Other("batch".into()));
        assert_eq!(task.status.as_str(), "paused");
    }
}
