//! Compares this SDK's request/response surface against the published API
//! reference.
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
//! Every request body, read model, and list-filter set this SDK declares is
//! checked, not just the task ones. `mode` went stale on three task surfaces at
//! once, so a create-body-only check would have caught one of three; restricting
//! the check to tasks has the same shape of blind spot one resource wider. Files,
//! shares, metrics and the cancel body carry the same undeclared-field hazard on
//! their bodies, and the event envelope is shared by all six families, so one
//! drift there moves all of them at once.
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
//! network. Run on a schedule, not on pull requests:
//! it goes red when the *API* changes, which is a fact about the world and not
//! about the commit under review, and a gate people learn to ignore is how the
//! last one survived.
//!
//! Run: cargo test --test api_contract_test -- --ignored --nocapture
//!
//! Set `INTROSPECTION_OPENAPI_SPEC=<path>` to check against a local spec file
//! instead of the published one, for the window where an API change has landed
//! but the reference it generates has not been published yet.

use std::collections::{BTreeSet, HashMap};

use introspection_sdk::api::schemas::{
    AgentInfo, ConnectionCreateParams, ConnectionSubjectType, ConnectorAuthMode,
    ConnectorAuthorizeParams, ConnectorCreateParams, ConnectorListParams, ConnectorStatus,
    ConnectorUpdateParams, ConversationExportParams, ConversationItemInclude,
    ConversationItemListParams, ConversationListParams, ConversationResolution,
    ConversationSentiment, ConversationStatus, Dimension, Event, EventListParams,
    ExperimentListParams, ExperimentStatus, FeedbackEvent, FeedbackPayload, File, FileListParams,
    FileType, FileUpdate, HavingTerm, IntrospectionEventName, MetricFilter, MetricSpec,
    MetricsConfig, MetricsQuery, OrderTerm, PaginationParams, RecipeListParams, ResourceShare,
    RunnerIdentity, RuntimeListParams, ShareCreate, ShareListParams, ShareResourceType,
    SortDirection, StringOrUuid, Task, TaskCancelOptions, TaskCreate, TaskFileRef, TaskKind,
    TaskListParams, TaskPrompt, TaskRepoRequest, TaskRunCreate, TaskRunKind, TaskStatus,
    TimeDimension,
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

const SPEC_URL: &str = "https://docs.introspection.dev/openapi/dataplane.json";
const CP_SPEC_URL: &str = "https://docs.introspection.dev/openapi/controlplane.json";

/// Env var pointing the check at a local spec file instead of the published
/// one. The reason it exists: an API change and the SDK change that follows it
/// land in different repositories, so there is a window where the only way to
/// tell whether this SDK matches the *next* reference is to check it against a
/// candidate spec that is not published yet.
const SPEC_PATH_ENV: &str = "INTROSPECTION_OPENAPI_SPEC";

/// Same, for the Control-Plane reference. The two planes are separate services
/// with separate specs, and the CP half went unchecked entirely until an
/// experiments filter the API does not accept shipped in two SDKs at once.
const CP_SPEC_PATH_ENV: &str = "INTROSPECTION_CP_OPENAPI_SPEC";

/// The reference to check against: a local file when [`SPEC_PATH_ENV`] is set,
/// otherwise the published one. Returns the source alongside the spec so a
/// failure report names which reference it disagreed with.
fn load_cp_reference() -> (Value, String) {
    load_one(CP_SPEC_PATH_ENV, CP_SPEC_URL)
}

fn load_reference() -> (Value, String) {
    load_one(SPEC_PATH_ENV, SPEC_URL)
}

fn load_one(env_var: &str, url: &str) -> (Value, String) {
    if let Ok(path) = std::env::var(env_var) {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{env_var}={path} is readable: {e}"));
        let spec =
            serde_json::from_str(&body).unwrap_or_else(|e| panic!("{env_var}={path} is JSON: {e}"));
        return (spec, path);
    }

    let spec = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client builds")
        .get(url)
        .send()
        .expect("the published reference is reachable")
        .json()
        .expect("the published reference is JSON");
    (spec, url.to_string())
}

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

/// Query parameters only. A templated route also declares its path segments as
/// parameters, and counting those would report a path argument the caller
/// passes positionally as a query parameter the SDK forgot.
fn query_parameters(spec: &Value, path: &str, method: &str) -> BTreeSet<String> {
    spec["paths"][path][method]["parameters"]
        .as_array()
        .unwrap_or_else(|| panic!("the reference has no parameters for {method} {path}"))
        .iter()
        .filter(|p| p["in"].as_str() == Some("query"))
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
fn sdk_surface_matches_the_published_reference() {
    let (spec, reference) = load_reference();
    let (cp_spec, cp_reference) = load_cp_reference();

    // Every field populated, so `skip_serializing_if` cannot hide one. No
    // `..Default::default()` anywhere: a new field must fail to compile here.
    let create = TaskCreate {
        title: Some("t".into()),
        prompt: Some("p".into()),
        agent_name: Some("agent".into()),
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
        tags: Some(vec!["customer:acme".into()]),
    };

    let run_create = TaskRunCreate {
        prompt: Some(TaskPrompt {
            text: "p".into(),
            images: Some(vec![]),
        }),
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
        tag: Some("customer:acme".into()),
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
        tags: vec!["customer:acme".into()],
    };

    let cancel = TaskCancelOptions::Drain {
        drain_within_seconds: Some(1),
    };

    let file = File {
        id: Uuid::nil(),
        org_id: Uuid::nil(),
        project_id: Uuid::nil(),
        created_at: "now".into(),
        updated_at: "now".into(),
        name: "n".into(),
        file_type: FileType::Upload,
        storage_path: "p".into(),
        mime_type: "text/plain".into(),
        metadata: Some(HashMap::new()),
        member_id: Some(Uuid::nil()),
        identity_key: Some("k".into()),
        task_id: Some(Uuid::nil()),
        size_bytes: 1,
        version: 1,
        parent_id: Some(Uuid::nil()),
        storage_version_id: Some("v".into()),
    };

    let file_update = FileUpdate {
        name: Some("n".into()),
        metadata: Some(HashMap::new()),
    };

    let file_list = FileListParams {
        limit: Some(1),
        next: Some("cursor".into()),
        include_total: Some(true),
        name: Some("n".into()),
        file_type: Some(FileType::Upload),
        storage_path: Some("p".into()),
    };

    let share_create = ShareCreate {
        resource_type: ShareResourceType::File,
        resource_id: "file".into(),
        granted_member_id: Some(Uuid::nil()),
        granted_identity_key: Some("k".into()),
    };

    let share = ResourceShare {
        id: Uuid::nil(),
        org_id: Uuid::nil(),
        project_id: Uuid::nil(),
        created_at: "now".into(),
        updated_at: "now".into(),
        resource_type: ShareResourceType::File,
        resource_id: "file".into(),
        granted_member_id: Some(Uuid::nil()),
        granted_identity_key: Some("k".into()),
        created_by_member_id: Uuid::nil(),
        created_by_identity_key: Some("k".into()),
        url: Some("https://example.invalid/s".into()),
    };

    let share_list = ShareListParams {
        limit: Some(1),
        next: Some("cursor".into()),
        resource_type: Some(ShareResourceType::File),
        resource_id: Some("file".into()),
        created_by_me: Some(true),
        granted_to_me: Some(true),
    };

    // Serialized through the `Event` enum rather than the bare `TypedEvent`:
    // `event_name` is the enum's tag, so it only reaches the wire that way, and
    // the reference declares it on the family schema.
    let feedback = Event::Feedback(FeedbackEvent {
        id: "e".into(),
        timestamp: "now".into(),
        trace_id: Some("t".into()),
        span_id: Some("s".into()),
        conversation_id: Some("c".into()),
        service_name: Some("svc".into()),
        environment: Some("prod".into()),
        runtime_group_id: Some(Uuid::nil()),
        runtime_id: Some(Uuid::nil()),
        experiment_id: Some(Uuid::nil()),
        recipe_git_commit_sha: Some("sha".into()),
        payload: FeedbackPayload {
            name: "thumbs_up".into(),
            comments: Some("c".into()),
            value: Some(1.0),
            user_id: Some("u".into()),
            anonymous_id: Some("a".into()),
            sentiment: Some("positive".into()),
            previous_response_id: Some("r".into()),
            agent_name: Some("agent".into()),
            agent_id: Some("id".into()),
            properties: Some(HashMap::new()),
        },
    });

    // Every field populated, so a new one must be considered here rather than
    // silently skipped. This surface exists because widening the check still
    // left the routes this SDK had just *gained* unchecked, and the export
    // shipped without `start_date`/`end_date` as a result.
    let experiment_list = ExperimentListParams {
        project: Some(StringOrUuid::from("proj")),
        runtime: Some("support-agent".into()),
        environment: Some("production".into()),
        status: Some(ExperimentStatus::Running),
        limit: Some(1),
        next: Some("cursor".into()),
    };

    // The last list surface without a declaration, and it had drifted:
    // `only_active` was a filter the route never accepted, and it was
    // load-bearing — `resolve()` passed it and trusted the narrowing.
    let runtime_list = RuntimeListParams {
        project: Some("proj".into()),
        runtime: Some(StringOrUuid::from("support-agent")),
        recipe_id: Some(Uuid::nil()),
        environment: Some("production".into()),
        limit: Some(1),
        next: Some("cursor".into()),
    };

    let recipe_list = RecipeListParams {
        project: Some(StringOrUuid::from("proj")),
        repository_id: Some(Uuid::nil()),
        name: Some("support".into()),
        pagination: PaginationParams {
            limit: Some(1),
            next: Some("cursor".into()),
        },
    };

    let connector_list = ConnectorListParams {
        project: Some(StringOrUuid::from("proj")),
        limit: Some(1),
        next: Some("cursor".into()),
    };

    let connector_create = ConnectorCreateParams {
        slug: Some("slack-support".into()),
        environment: Some("production".into()),
        agent_member_id: Some(Uuid::nil()),
        authorization_endpoint: Some("https://slack.com/oauth/v2/authorize".into()),
        token_endpoint: Some("https://slack.com/api/oauth.v2.access".into()),
        scopes: Some(vec!["chat:write".into()]),
        api_hosts: Some(vec!["slack.com".into()]),
        client_id: Some("client".into()),
        client_secret: Some("secret".into()),
        signing_secret: Some("signing".into()),
        metadata: Some(Default::default()),
        issuer: Some("https://slack.com".into()),
        person_server_mode: Some("managed".into()),
        person_server_url: Some("https://ps.example".into()),
        approval_policy: Some("human".into()),
        application_id: Some(Uuid::nil()),
        assertion_audience: Some("https://slack.com".into()),
        webhook_url: Some("https://api.example/webhooks".into()),
        ..ConnectorCreateParams::new("Slack", "slack", ConnectorAuthMode::OauthStored)
    };

    let connector_update = ConnectorUpdateParams {
        name: Some("Slack".into()),
        agent_member_id: Some(Uuid::nil()),
        scopes: Some(vec!["chat:write".into()]),
        api_hosts: Some(vec!["slack.com".into()]),
        status: Some(ConnectorStatus::Active),
        metadata: Some(Default::default()),
        webhook_url: Some("https://api.example/webhooks".into()),
        client_secret: Some("secret".into()),
        signing_secret: Some("signing".into()),
    };

    let connection_create = ConnectionCreateParams {
        subject_type: Some(ConnectionSubjectType::App),
        scopes_granted: Some(vec!["chat:write".into()]),
        refresh_token: Some("refresh".into()),
        token_expires_at: Some("2026-08-08T21:00:00Z".into()),
        ..ConnectionCreateParams::new("xoxb-token")
    };

    let connector_authorize = ConnectorAuthorizeParams {
        runtime: Some(StringOrUuid::from("support-agent")),
        subject: Some(ConnectionSubjectType::App),
        return_url: Some("https://app.example/done".into()),
        expires_in: Some(3600),
        identity: Some(RunnerIdentity {
            user_id: Some("u_demo".into()),
            anonymous_id: None,
            conversation_id: None,
        }),
    };

    let conversation_export = ConversationExportParams {
        agent: Some("root".into()),
        service_name: Some("svc".into()),
        operation_name: Some("op".into()),
        lookback_days: Some(7),
        share_id: Some(Uuid::nil()),
        start_date: Some("2026-01-01T00:00:00Z".into()),
        end_date: Some("2026-01-02T00:00:00Z".into()),
    };

    let conversation_item_list = ConversationItemListParams {
        limit: Some(1),
        next: Some("cursor".into()),
        include: vec![ConversationItemInclude::Events],
        agent: Some("root".into()),
        service_name: Some("svc".into()),
        operation_name: Some("op".into()),
        lookback_days: Some(7),
        share_id: Some(Uuid::nil()),
        start_date: Some("2026-01-01T00:00:00Z".into()),
        end_date: Some("2026-01-02T00:00:00Z".into()),
    };

    // Every typed field is populated, so the assertion below is that all 22
    // declared query params of `GET /v1/conversations` are reachable from the
    // typed surface -- not just the handful a spot-check would set.
    let conversation_list = ConversationListParams {
        limit: Some(1),
        next: Some("cursor".into()),
        sort: Some("created".into()),
        order: Some(SortDirection::Asc),
        start: Some("2026-01-01T00:00:00Z".into()),
        end: Some("2026-01-02T00:00:00Z".into()),
        // Mutually exclusive with `start`/`end`, as on the event params.
        lookback: None,
        conversation_id: Some("conv".into()),
        conversation_ids: Some(vec!["conv".into()]),
        share_id: Some(vec![Uuid::nil().to_string()]),
        model: Some("claude-opus-5".into()),
        agent_name: Some("agent".into()),
        status: Some(ConversationStatus::Ok),
        service_name: Some("svc".into()),
        service_names: Some(vec!["svc".into()]),
        environment: Some("prod".into()),
        runtime_id: Some(Uuid::nil()),
        runtime_group_id: Some(Uuid::nil()),
        experiment_id: Some(Uuid::nil()),
        recipe_git_commit_sha: Some("deadbeef".into()),
        resolution: Some(ConversationResolution::Resolved),
        sentiment: Some(ConversationSentiment::Positive),
        owner_key: Some("owner".into()),
        // A declared param through the escape hatch, so the verbatim merge is
        // exercised rather than assumed.
        filters: Some(HashMap::from([(
            "environment".to_string(),
            Value::from("prod"),
        )])),
    };

    let event_list = EventListParams {
        limit: Some(1),
        next: Some("cursor".into()),
        sort: Some("timestamp".into()),
        order: Some(SortDirection::Asc),
        start: Some("2026-01-01T00:00:00Z".into()),
        end: Some("2026-01-02T00:00:00Z".into()),
        // Mutually exclusive with `start`/`end` — populating all three is
        // rejected client-side, and the window keys are already covered.
        lookback: None,
        conversation_id: Some("conv".into()),
        conversation_ids: Some(vec!["conv".into()]),
        event_id: Some(vec!["evt".into()]),
        service_name: Some("svc".into()),
        environment: Some("prod".into()),
        runtime_group_id: Some(Uuid::nil()),
        runtime_group_unattributed: Some(true),
        lens: Some("lens".into()),
        pattern_id: Some(Uuid::nil()),
        status: Some("open".into()),
        include_superseded: Some(true),
        severities: Some(vec!["high".into()]),
        trace_id: Some("trace".into()),
        span_id: Some("span".into()),
        owner_key: Some("owner".into()),
        // A declared param through the escape hatch, so the verbatim merge is
        // exercised rather than assumed.
        filters: Some(HashMap::from([(
            "environment".to_string(),
            Value::from("prod"),
        )])),
        ..EventListParams::new(IntrospectionEventName::Feedback)
    };

    let metrics = MetricsQuery {
        view: "v".into(),
        metrics: vec![MetricSpec {
            measure: "m".into(),
            aggregation: "sum".into(),
        }],
        dimensions: Some(vec![Dimension { field: "f".into() }]),
        filters: Some(vec![MetricFilter {
            field: "f".into(),
            operator: "eq".into(),
            value: Value::from("x"),
        }]),
        time_dimension: Some(TimeDimension {
            bins: Some(1),
            granularity: Some("hour".into()),
        }),
        order_by: Some(vec![OrderTerm {
            term_type: "metric".into(),
            metric_index: Some(0),
            field: Some("f".into()),
            direction: SortDirection::Desc,
        }]),
        having: Some(vec![HavingTerm {
            metric_index: 0,
            operator: "gt".into(),
            value: Value::from(1),
        }]),
        config: Some(MetricsConfig {
            row_limit: Some(1),
            series_limit: Some(1),
        }),
        start: Some("2026-01-01T00:00:00Z".into()),
        end: Some("2026-01-02T00:00:00Z".into()),
        // As above: `lookback` conflicts with `start`/`end`, and all three
        // resolve into the same two wire keys.
        lookback: None,
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
        compare(
            "TaskCancelOptions — POST /v1/tasks/{id}/runs/{run_id}/cancel body",
            // The `Drain` variant, because `Abort` carries only the `mode` tag
            // while `Drain` carries both wire keys.
            wire_fields(&cancel),
            schema_properties(&spec, "TaskCancelRequest"),
            &[],
            "sent here but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "File — the file read model",
            wire_fields(&file),
            schema_properties(&spec, "File"),
            &[],
            "declared here but not returned by the API",
            "returned by the API but not surfaced by this SDK",
            true,
        ),
        compare(
            "FileUpdate — PATCH /v1/files/{id} body",
            wire_fields(&file_update),
            schema_properties(&spec, "FileUpdate"),
            &[],
            "sent here but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "file list filters — GET /v1/files query parameters",
            wire_fields(&file_list),
            query_parameters(&spec, "/v1/files", "get"),
            // `identity_key` is privileged-only and 403s for the credentials
            // this client carries; `task_id`/`share_id` are scoping params the
            // runner already carries. Which filters to expose is a product
            // decision, so absence is reported and does not fail.
            &["identity_key", "task_id", "share_id"],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
        compare(
            "ShareCreate — POST /v1/shares body",
            wire_fields(&share_create),
            schema_properties(&spec, "ShareCreate"),
            &[],
            "sent here but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "ResourceShare — the share read model",
            wire_fields(&share),
            schema_properties(&spec, "ResourceShare"),
            &[],
            "declared here but not returned by the API",
            "returned by the API but not surfaced by this SDK",
            true,
        ),
        compare(
            "share list filters — GET /v1/shares query parameters",
            wire_fields(&share_list),
            query_parameters(&spec, "/v1/shares", "get"),
            &[],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
        compare(
            "event envelope — the common envelope on every event family",
            // One family stands in for all six: the envelope is shared, so a
            // field added or dropped there moves every family at once. The
            // payloads are a different check and belong to whichever family
            // owns them.
            wire_fields(&feedback),
            schema_properties(&spec, "FeedbackEvent"),
            &[],
            "declared here but not returned by the API",
            "returned by the API but not surfaced by this SDK",
            true,
        ),
        // There is deliberately no `Conversation` read-model surface: the
        // published reference declares no properties for that schema, so the
        // comparison would pass by doing nothing. The list filters are
        // declared, so they are checked.
        compare(
            "conversation list filters — GET /v1/conversations query parameters",
            // Lowered, as with the event params below.
            wire_fields(
                &conversation_list
                    .to_wire()
                    .expect("conversation params lower"),
            ),
            query_parameters(&spec, "/v1/conversations", "get"),
            // As with events: the remaining declared parameters are reachable
            // verbatim through `filters`, so absence is a note, not a failure.
            &[],
            "sent as a query parameter the API does not accept",
            "accepted by the API and reachable only through the verbatim `filters` map",
            false,
        ),
        compare(
            "experiment list filters — GET /v1/experiments query parameters",
            wire_fields(&experiment_list),
            query_parameters(&cp_spec, "/v1/experiments", "get"),
            // The deprecated spelling of `project`; this SDK sends the current one.
            &["project_id"],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
        compare(
            "runtime list filters — GET /v1/runtimes query parameters",
            wire_fields(&runtime_list),
            query_parameters(&cp_spec, "/v1/runtimes", "get"),
            &["project_id"],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            true,
        ),
        compare(
            "recipe list filters — GET /v1/recipes query parameters",
            wire_fields(&recipe_list),
            query_parameters(&cp_spec, "/v1/recipes", "get"),
            &["project_id"],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
        compare(
            "connector list filters — GET /v1/connectors query parameters",
            wire_fields(&connector_list),
            query_parameters(&cp_spec, "/v1/connectors", "get"),
            &["project_id"],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
        compare(
            "connection list filters — GET /v1/connectors/{id}/connections query parameters",
            wire_fields(&PaginationParams {
                limit: Some(1),
                next: Some("cursor".into()),
            }),
            query_parameters(
                &cp_spec,
                "/v1/connectors/{connector_id}/connections",
                "get",
            ),
            &[],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            false,
        ),
        compare(
            "ConnectorCreate — POST /v1/connectors body",
            wire_fields(&connector_create),
            schema_properties(&cp_spec, "ConnectorCreate"),
            &[],
            "sent but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "ConnectorUpdate — PATCH /v1/connectors/{id} body",
            wire_fields(&connector_update),
            schema_properties(&cp_spec, "ConnectorUpdate"),
            &[],
            "sent but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            "ConnectionCreate — POST /v1/connectors/{id}/connections body",
            wire_fields(&connection_create),
            schema_properties(&cp_spec, "ConnectionCreate"),
            &[],
            "sent but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
        compare(
            // `connector_id` is the method's first argument, merged into the
            // body by `authorize`, so it is absent from the params struct.
            "ConnectAuthorizeRequest — POST /v1/oauth/connections/authorize body",
            wire_fields(&connector_authorize),
            schema_properties(&cp_spec, "ConnectAuthorizeRequest"),
            &["connector_id"],
            "sent but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            false,
        ),
        compare(
            "conversation export filters — GET /v1/conversations/{id}/export query parameters",
            wire_fields(&conversation_export),
            query_parameters(&spec, "/v1/conversations/{conversation_id}/export", "get"),
            &[],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            true,
        ),
        // The items route is where the ordering bug hid:
        // they declared an `order` the route never accepted and omitted the
        // window/share params it did. A sub-resource is still a route.
        compare(
            "conversation item list filters — GET /v1/conversations/{id}/items query parameters",
            conversation_item_list
                .to_wire()
                .into_iter()
                .map(|(key, _)| key)
                .collect::<BTreeSet<String>>(),
            query_parameters(&spec, "/v1/conversations/{conversation_id}/items", "get"),
            &[],
            "sent as a query parameter the API does not accept",
            "accepted by the API but not exposed here",
            true,
        ),
        compare(
            "event list filters — GET /v1/events query parameters",
            // The lowered query object, not the struct: `order`/`start`/`end`
            // are ergonomic aliases resolved into `direction`/`start_date`/
            // `end_date` before anything is sent, so comparing the struct
            // would report three parameters the API never sees.
            wire_fields(&event_list.to_wire().expect("event params lower")),
            query_parameters(&spec, "/v1/events", "get"),
            // No exemptions: every remaining parameter is reachable verbatim
            // through `EventListParams::filters`, so the small typed set is a
            // convenience layer rather than the limit of what can be sent, and
            // a parameter absent from it is a note rather than a failure.
            &[],
            "sent as a query parameter the API does not accept",
            "accepted by the API and reachable only through the verbatim `filters` map",
            false,
        ),
        compare(
            "MetricsQuery — POST /v1/metrics body",
            // Lowered, as above: `start`/`end`/`lookback` are the ergonomic
            // window and become `from_timestamp`/`to_timestamp`.
            wire_fields(&metrics.to_wire().expect("metrics query lowers")),
            schema_properties(&spec, "MetricQueryRequest"),
            &[],
            "sent here but not declared by the API",
            "accepted by the API but unavailable to callers of this SDK",
            true,
        ),
    ];

    let report: String = comparisons
        .iter()
        .filter(|c| !c.problems.is_empty())
        .map(|c| format!("{}\n{}\n", c.surface, c.problems.join("\n")))
        .collect();

    assert!(
        report.is_empty(),
        "the SDK surface has drifted from the published reference:\n\n{report}\nreference: {reference} + {cp_reference}"
    );

    println!(
        "✓ SDK surface matches the published reference ({} surfaces, {reference} + {cp_reference})",
        comparisons.len()
    );
}
