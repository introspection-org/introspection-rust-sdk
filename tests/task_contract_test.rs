//! Compares this SDK's task surface against the published API reference.
//!
//! `start_prompt` hardcoded `mode: Some(TaskMode::Agent)` for the entire life of
//! that field's retirement, so the one call this SDK documents in its own
//! rustdoc example 422'd against a current Data Plane. `Task.mode` and the
//! `modes` list filter went stale at the same time. Nothing in CI could catch
//! any of it, because nothing in CI knew what the API accepted.
//!
//! The reference at docs.introspection.dev is generated from the Data Plane API
//! itself, so comparing against it compares against the API's own declaration
//! rather than a second hand-maintained copy.
//!
//! # Why serialization rather than a field list
//!
//! Each surface is compared by building a fully populated value and serializing
//! it, so what gets checked is the **wire** name, not the Rust field name. That
//! matters here: `TaskRepoRequest::git_ref` is `ref` on the wire (a keyword in
//! Rust), and a dropped or mistyped serde rename would clone the wrong branch
//! rather than fail. A field-name comparison would sail straight past it.
//!
//! None of the literals below use `..Default::default()`. That is deliberate:
//! adding a field to one of these structs then fails to compile here, so the
//! new field has to be considered rather than silently skipped by the very
//! check meant to catch it.
//!
//! `#[ignore]`d, following this repository's convention for tests that reach the
//! network (see `responses_api_test`). Run on a schedule, not on pull requests:
//! it goes red when the *API* changes, which is a fact about the world and not
//! about the commit under review, and a gate people learn to ignore is how the
//! last one survived.
//!
//! Run: cargo test --test task_contract_test -- --ignored --nocapture

use std::collections::{BTreeSet, HashMap};

use introspection_sdk::api::schemas::{
    AgentInfo, Task, TaskCreate, TaskFileRef, TaskKind, TaskListParams, TaskPrompt,
    TaskRepoRequest, TaskRunCreate, TaskRunKind, TaskStatus,
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

const SPEC_URL: &str = "https://docs.introspection.dev/openapi/dataplane.json";

/// Wire field names produced by serializing a fully populated value.
fn wire_fields<T: Serialize>(value: &T) -> BTreeSet<String> {
    match serde_json::to_value(value).expect("value serializes") {
        Value::Object(map) => map.keys().cloned().collect(),
        other => panic!("expected a JSON object, got {other}"),
    }
}

fn schema_properties(spec: &Value, name: &str) -> BTreeSet<String> {
    spec["components"]["schemas"][name]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("the reference has no components.schemas.{name}.properties"))
        .keys()
        .cloned()
        .collect()
}

fn query_parameters(spec: &Value, path: &str, method: &str) -> BTreeSet<String> {
    spec["paths"][path][method]["parameters"]
        .as_array()
        .unwrap_or_else(|| panic!("the reference has no parameters for {method} {path}"))
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect()
}

fn names(fields: &BTreeSet<String>) -> String {
    fields
        .iter()
        .map(|f| format!("\n      {f}"))
        .collect::<String>()
}

/// One surface's verdict, accumulated so a run reports every difference rather
/// than only the first.
struct Comparison {
    surface: &'static str,
    problems: Vec<String>,
}

fn compare(
    surface: &'static str,
    sdk: BTreeSet<String>,
    server: BTreeSet<String>,
    // Fields the API has that this SDK omits on purpose.
    exempt: &[&str],
    extra_means: &str,
    missing_means: &str,
    missing_is_fatal: bool,
) -> Comparison {
    let exempt: BTreeSet<String> = exempt.iter().map(|s| (*s).to_owned()).collect();

    let extra: BTreeSet<String> = sdk.difference(&server).cloned().collect();
    let missing: BTreeSet<String> = server
        .difference(&sdk)
        .filter(|f| !exempt.contains(*f))
        .cloned()
        .collect();
    // An exemption naming a field the API no longer has is itself drift: it
    // would otherwise stay quietly true forever, hiding nothing.
    let stale: BTreeSet<String> = exempt.difference(&server).cloned().collect();

    let mut problems = Vec::new();
    if !extra.is_empty() {
        problems.push(format!("  {extra_means}:{}", names(&extra)));
    }
    if !missing.is_empty() && missing_is_fatal {
        problems.push(format!("  {missing_means}:{}", names(&missing)));
    } else if !missing.is_empty() {
        println!("note: {surface}: {missing_means}:{}", names(&missing));
    }
    if !stale.is_empty() {
        problems.push(format!(
            "  exempted here but no longer in the API (drop the exemption):{}",
            names(&stale)
        ));
    }

    Comparison { surface, problems }
}

#[test]
#[ignore = "reaches the network: fetches the published API reference"]
fn task_surface_matches_the_published_reference() {
    let spec: Value = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client builds")
        .get(SPEC_URL)
        .send()
        .expect("the published reference is reachable")
        .json()
        .expect("the published reference is JSON");

    // Every field populated, so `skip_serializing_if` cannot hide one. No
    // `..Default::default()` anywhere: a new field must fail to compile here.
    let create = TaskCreate {
        title: Some("t".into()),
        prompt: Some("p".into()),
        agent_name: Some("agent".into()),
        repository_id: Some("repo".into()),
        repositories: Some(vec![TaskRepoRequest {
            repo: "owner/name".into(),
            git_ref: Some("main".into()),
            depth: Some(1),
        }]),
        files: Some(vec![TaskFileRef {
            id: "file".into(),
            name: Some("n".into()),
            size_bytes: Some(1),
        }]),
        idle_timeout_seconds: Some(1),
        fork_share_id: Some("share".into()),
        metadata: Some(HashMap::new()),
    };

    let run_create = TaskRunCreate {
        prompt: Some(TaskPrompt {
            text: "p".into(),
            images: Some(vec![]),
        }),
        message: Some("m".into()),
        kind: Some(TaskRunKind::Prompt),
        files: Some(vec![TaskFileRef {
            id: "file".into(),
            name: None,
            size_bytes: None,
        }]),
        metadata: Some(HashMap::new()),
    };

    let list = TaskListParams {
        limit: Some(1),
        next: Some("cursor".into()),
        include_total: Some(true),
        statuses: Some(vec![TaskStatus::Running]),
        require_automation_id: Some(true),
    };

    let task = Task {
        id: Uuid::nil(),
        org_id: Uuid::nil(),
        project_id: Uuid::nil(),
        created_at: "now".into(),
        updated_at: "now".into(),
        title: Some("t".into()),
        display_index: Some(1),
        kind: TaskKind::Agent,
        status: TaskStatus::Running,
        member_id: Some(Uuid::nil()),
        automation_id: Some(Uuid::nil()),
        runtime_id: Some(Uuid::nil()),
        is_archived: false,
        started_at: Some("now".into()),
        completed_at: Some("now".into()),
        last_user_message_at: Some("now".into()),
        metadata: Some(HashMap::new()),
        agent: Some(AgentInfo {
            sandbox_status: Some("s".into()),
            session_id: Some("s".into()),
        }),
        identity_key: Some("k".into()),
    };

    let comparisons = [
        compare(
            "TaskCreate — POST /v1/tasks body",
            wire_fields(&create),
            schema_properties(&spec, "TaskCreate"),
            // Runner-bound client: the credential's claim is authoritative for
            // runtime selection and the API ignores a body `runtime_id` from
            // such a caller, so exposing it would do nothing.
            &["runtime_id"],
            "sent here but not accepted by the API (rejected with a 422 — the create body forbids undeclared fields)",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "Task — the task read model",
            wire_fields(&task),
            schema_properties(&spec, "Task"),
            &[],
            "declared here but not returned by the API (the SDK describes a response that no longer exists)",
            "returned by the API but not surfaced by this SDK",
            true,
        ),
        compare(
            "TaskRunCreate — POST /v1/tasks/{id}/runs body",
            wire_fields(&run_create),
            schema_properties(&spec, "TaskRunCreate"),
            // `resume` is a separate typed call on this client, not a field on
            // the create body.
            &["resume"],
            "sent here but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "list filters — GET /v1/tasks query parameters",
            wire_fields(&list),
            query_parameters(&spec, "/v1/tasks", "get"),
            // runtime_id/runtime_ids/updated_after/conversation_id are
            // product-UI shaped; identity_key is privileged-only and 403s for
            // the credentials this client carries. Which filters to expose is a
            // product decision, so absence is reported and does not fail.
            &[
                "runtime_id",
                "runtime_ids",
                "updated_after",
                "conversation_id",
                "identity_key",
            ],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
    ];

    let report: String = comparisons
        .iter()
        .filter(|c| !c.problems.is_empty())
        .map(|c| format!("{}\n{}\n", c.surface, c.problems.join("\n")))
        .collect();

    assert!(
        report.is_empty(),
        "the task surface has drifted from the published reference:\n\n{report}\nreference: {SPEC_URL}"
    );

    println!("✓ task surface matches the published reference ({SPEC_URL})");
}
