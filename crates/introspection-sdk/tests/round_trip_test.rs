//! Every response the walk recorded survives the trip through its typed
//! response and back to JSON.
//!
//! Each JSON response body in `tests/recordings/walk/recording.har` is decoded
//! into the type the SDK returns for its route, serialized again, and compared
//! with the original. Key order is ignored, and so are `null` fields: the
//! types skip a `None` when they serialize, so a `null` the server sent comes
//! back as an absent key. A `null` the original did not have fails the test,
//! as does any other difference, and so does a field the type does not
//! model even when the server sent it as `null`.
//!
//! Numbers compare by value, so a `1` the server sent into an `f64` field may
//! come back as `1.0`.
//!
//! A response type keeps no bag of unknown fields, so a field the server sends
//! that the SDK does not model is lost on the way back and fails the test until
//! it is modelled. Only the maps that are open by design keep keys they do not
//! name: the `gen_ai.*` span attributes, the GenAI message and part types, and
//! `EndpointMetadata`.
//!
//! A route the SDK has no response type for is listed in [`UNTYPED`] with the
//! reason, so a new route in the recording fails the test until it is mapped.

use std::path::Path;

use introspection_sdk::*;
use regex::Regex;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

type Outcome = std::result::Result<(), String>;
type Check = fn(&Value) -> Outcome;

/// Decodes `body` into `T`, serializes it again and compares the two.
fn round_trip<T: DeserializeOwned + Serialize>(body: &Value) -> Outcome {
    let mut diffs = Vec::new();
    let typed: T = serde_ignored::deserialize(body.clone(), |path| {
        diffs.push(format!("$.{path}: not modelled"))
    })
    .map_err(|e| format!("decode: {e}"))?;
    let back = serde_json::to_value(&typed).map_err(|e| format!("encode: {e}"))?;
    compare("$", body, &back, &mut diffs);
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs.join("\n    "))
    }
}

/// `GET /v1/api-keys` wraps its keys in an `api_keys` envelope the SDK
/// unwraps privately; check the envelope holds nothing else.
fn api_key_page(body: &Value) -> Outcome {
    let obj = body.as_object().ok_or("not an object")?;
    let extra: Vec<_> = obj.keys().filter(|k| *k != "api_keys").collect();
    if !extra.is_empty() {
        return Err(format!("envelope keys the SDK drops: {extra:?}"));
    }
    round_trip::<Vec<ApiKey>>(&obj["api_keys"])
}

/// `(method, path regex, check)`, first match wins. Paths are matched without
/// host and query.
const TYPED: &[(&str, &str, Check)] = &[
    (
        "GET",
        r"^/v1/aauth/approvals$",
        round_trip::<Paginated<Approval>>,
    ),
    (
        "GET",
        r"^/v1/aauth/missions$",
        round_trip::<Paginated<Mission>>,
    ),
    (
        "GET",
        r"^/v1/annotations$",
        round_trip::<Paginated<AnnotationState>>,
    ),
    ("GET", r"^/v1/api-keys$", api_key_page),
    ("POST", r"^/v1/api-keys$", round_trip::<ApiKeyCreated>),
    ("PATCH", r"^/v1/api-keys/[^/]+$", round_trip::<ApiKey>),
    (
        "POST",
        r"^/v1/api-keys/[^/]+/acknowledge$",
        round_trip::<ApiKeyAcknowledgement>,
    ),
    (
        "GET",
        r"^/v1/applications$",
        round_trip::<Paginated<Application>>,
    ),
    ("POST", r"^/v1/applications$", round_trip::<Application>),
    (
        "GET|PATCH",
        r"^/v1/applications/[^/]+$",
        round_trip::<Application>,
    ),
    (
        "GET",
        r"^/v1/applications/[^/]+/secrets$",
        round_trip::<Paginated<ApplicationSecret>>,
    ),
    (
        "POST",
        r"^/v1/applications/[^/]+/secrets$",
        round_trip::<ApplicationSecret>,
    ),
    (
        "GET",
        r"^/v1/automations$",
        round_trip::<Paginated<Automation>>,
    ),
    ("POST", r"^/v1/automations$", round_trip::<Automation>),
    (
        "GET|PATCH",
        r"^/v1/automations/[^/]+$",
        round_trip::<Automation>,
    ),
    (
        "POST",
        r"^/v1/automations/[^/]+/trigger$",
        round_trip::<AutomationTriggerResponse>,
    ),
    (
        "GET",
        r"^/v1/connections$",
        round_trip::<Paginated<MemberConnection>>,
    ),
    (
        "GET",
        r"^/v1/connectors$",
        round_trip::<Paginated<Connector>>,
    ),
    ("POST", r"^/v1/connectors$", round_trip::<Connector>),
    (
        "POST",
        r"^/v1/connectors/discover-oauth$",
        round_trip::<ConnectorOAuthDiscovery>,
    ),
    (
        "GET|PATCH",
        r"^/v1/connectors/[^/]+$",
        round_trip::<Connector>,
    ),
    (
        "GET",
        r"^/v1/connectors/[^/]+/connections$",
        round_trip::<Paginated<Connection>>,
    ),
    (
        "GET",
        r"^/v1/connectors/[^/]+/connections/[^/]+$",
        round_trip::<Connection>,
    ),
    (
        "GET",
        r"^/v1/conversations$",
        round_trip::<Paginated<Conversation>>,
    ),
    (
        "GET",
        r"^/v1/conversations/[^/]+$",
        round_trip::<Conversation>,
    ),
    (
        "GET",
        r"^/v1/conversations/[^/]+/items$",
        round_trip::<GenAiSpanList>,
    ),
    (
        "GET",
        r"^/v1/conversations/[^/]+/items/[^/]+$",
        round_trip::<GenAiSpan>,
    ),
    (
        "GET",
        r"^/v1/credentials$",
        round_trip::<Paginated<Credential>>,
    ),
    ("POST", r"^/v1/credentials$", round_trip::<Credential>),
    (
        "GET|PATCH",
        r"^/v1/credentials/[^/]+$",
        round_trip::<Credential>,
    ),
    ("GET", r"^/v1/endpoints$", round_trip::<Paginated<Endpoint>>),
    ("POST", r"^/v1/endpoints$", round_trip::<Endpoint>),
    (
        "GET|PATCH",
        r"^/v1/endpoints/[^/]+$",
        round_trip::<Endpoint>,
    ),
    ("GET", r"^/v1/events$", round_trip::<Paginated<Event>>),
    (
        "GET",
        r"^/v1/experiments$",
        round_trip::<Paginated<Experiment>>,
    ),
    ("POST", r"^/v1/experiments$", round_trip::<Experiment>),
    (
        "GET|PATCH",
        r"^/v1/experiments/[^/]+$",
        round_trip::<Experiment>,
    ),
    (
        "POST",
        r"^/v1/experiments/[^/]+/run$",
        round_trip::<RunnerSpec>,
    ),
    (
        "POST",
        r"^/v1/experiments/[^/]+/(start|end|cancel)$",
        round_trip::<Experiment>,
    ),
    ("GET", r"^/v1/files$", round_trip::<Paginated<File>>),
    ("POST", r"^/v1/files$", round_trip::<File>),
    ("GET|PATCH", r"^/v1/files/[^/]+$", round_trip::<File>),
    (
        "GET",
        r"^/v1/files/[^/]+/versions$",
        round_trip::<Paginated<File>>,
    ),
    ("POST", r"^/v1/files/[^/]+/versions$", round_trip::<File>),
    (
        "GET",
        r"^/v1/files/[^/]+/versions/[^/]+$",
        round_trip::<File>,
    ),
    ("GET", r"^/v1/images$", round_trip::<Paginated<Image>>),
    ("GET", r"^/v1/images/[^/]+$", round_trip::<Image>),
    (
        "GET",
        r"^/v1/integrations$",
        round_trip::<Paginated<Integration>>,
    ),
    ("GET", r"^/v1/judges$", round_trip::<Paginated<Judge>>),
    ("GET|PATCH", r"^/v1/judges/[^/]+$", round_trip::<Judge>),
    ("GET", r"^/v1/members$", round_trip::<Paginated<Member>>),
    ("POST", r"^/v1/members$", round_trip::<Member>),
    ("GET|PATCH", r"^/v1/members/[^/]+$", round_trip::<Member>),
    ("POST", r"^/v1/metrics$", round_trip::<MetricsResponse>),
    (
        "POST",
        r"^/v1/oauth/connections/authorize$",
        round_trip::<ConnectorAuthorization>,
    ),
    (
        "POST",
        r"^/v1/oauth/connections/token$",
        round_trip::<ConnectionTokenResult>,
    ),
    (
        "POST",
        r"^/v1/oauth/device/code$",
        round_trip::<DeviceCodeResponse>,
    ),
    ("POST", r"^/v1/oauth/token$", round_trip::<OAuthToken>),
    ("GET", r"^/v1/oidc/me$", round_trip::<Me>),
    ("GET", r"^/v1/projects$", round_trip::<Paginated<Project>>),
    ("POST", r"^/v1/projects$", round_trip::<Project>),
    ("GET|PATCH", r"^/v1/projects/[^/]+$", round_trip::<Project>),
    ("GET", r"^/v1/recipes$", round_trip::<Paginated<Recipe>>),
    ("POST", r"^/v1/recipes$", round_trip::<Recipe>),
    ("GET|PATCH", r"^/v1/recipes/[^/]+$", round_trip::<Recipe>),
    (
        "GET",
        r"^/v1/remotes/connect$",
        round_trip::<RemotesConnectDocs>,
    ),
    ("GET", r"^/v1/repositories$", round_trip::<Vec<Repository>>),
    ("POST", r"^/v1/repositories$", round_trip::<Repository>),
    ("GET", r"^/v1/repositories/[^/]+$", round_trip::<Repository>),
    (
        "GET",
        r"^/v1/repositories/[^/]+/commits$",
        round_trip::<Paginated<RepositoryCommit>>,
    ),
    (
        "GET",
        r"^/v1/repositories/[^/]+/commits/[^/]+$",
        round_trip::<RepositoryCommitDetail>,
    ),
    (
        "GET",
        r"^/v1/repositories/[^/]+/contents(/.*)?$",
        round_trip::<RepositoryContent>,
    ),
    ("GET", r"^/v1/runtimes$", round_trip::<Paginated<Runtime>>),
    ("POST", r"^/v1/runtimes$", round_trip::<Runtime>),
    ("GET|PATCH", r"^/v1/runtimes/[^/]+$", round_trip::<Runtime>),
    (
        "POST",
        r"^/v1/runtimes/[^/]+/run$",
        round_trip::<RunnerSpec>,
    ),
    (
        "GET",
        r"^/v1/shares$",
        round_trip::<Paginated<ResourceShare>>,
    ),
    ("POST", r"^/v1/shares$", round_trip::<ResourceShare>),
    (
        "GET|PATCH",
        r"^/v1/shares/[^/]+$",
        round_trip::<ResourceShare>,
    ),
    ("GET", r"^/v1/tags$", round_trip::<Paginated<Tag>>),
    ("POST", r"^/v1/tags$", round_trip::<Tag>),
    ("GET|PATCH", r"^/v1/tags/[^/]+/[^/]+$", round_trip::<Tag>),
    ("GET", r"^/v1/tasks$", round_trip::<Paginated<Task>>),
    ("POST", r"^/v1/tasks$", round_trip::<TaskCreateResponse>),
    ("GET|PATCH", r"^/v1/tasks/[^/]+$", round_trip::<Task>),
    (
        "GET",
        r"^/v1/tasks/[^/]+/commands$",
        round_trip::<Paginated<TaskCommand>>,
    ),
    (
        "POST",
        r"^/v1/tasks/[^/]+/commands$",
        round_trip::<TaskCommand>,
    ),
    (
        "GET",
        r"^/v1/tasks/[^/]+/commands/[^/]+$",
        round_trip::<TaskCommand>,
    ),
    (
        "POST",
        r"^/v1/tasks/[^/]+/runs$",
        round_trip::<TaskRunResponse>,
    ),
    (
        "GET",
        r"^/v1/tasks/[^/]+/runs/[^/]+$",
        round_trip::<TaskRun>,
    ),
    (
        "POST",
        r"^/v1/tasks/[^/]+/runs/[^/]+/cancel$",
        round_trip::<TaskCancelResponse>,
    ),
    ("GET", r"^/v1/variables$", round_trip::<Paginated<Variable>>),
    ("POST", r"^/v1/variables$", round_trip::<Variable>),
    ("PATCH", r"^/v1/variables/[^/]+$", round_trip::<Variable>),
];

/// JSON responses no SDK type decodes: `(method, path regex, reason)`.
const UNTYPED: &[(&str, &str, &str)] = &[
    (
        "POST",
        r"^/v1/annotations$",
        "Annotations::create returns ()",
    ),
    (
        "GET",
        r"^/v1/annotations/(facets|navigation)$",
        "no SDK method",
    ),
    ("GET", r"^/v1/repositories/[^/]+/branches$", "no SDK method"),
    ("POST", r"^/v1/tokens$", "no SDK method"),
    ("GET", r"^/v1/organizations/me$", "no SDK method"),
];

/// The SDK type for a JSON export, picked by its media type. The replay and
/// ATIF exports have none: `Conversations::export_json_value` returns them
/// as the server's JSON.
fn export_check(mime: &str) -> Option<Check> {
    if mime.starts_with("application/vnd.letta.trajectory+json") {
        Some(round_trip::<Trajectory>)
    } else if mime.starts_with("application/vnd.introspection.conversation-replay+json")
        || mime.starts_with("application/vnd.harbor.atif+json")
    {
        None
    } else {
        Some(round_trip::<GenAiSpanList>)
    }
}

/// Records every difference between `original` (nulls ignored) and `back`.
fn compare(path: &str, original: &Value, back: &Value, diffs: &mut Vec<String>) {
    match (original, back) {
        (Value::Object(a), Value::Object(b)) => compare_objects(path, a, b, diffs),
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                compare(&format!("{path}[{i}]"), x, y, diffs);
            }
        }
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => {}
        (a, b) if a == b => {}
        (a, b) => diffs.push(format!("{path}: sent {a}, got back {b}")),
    }
}

fn compare_objects(
    path: &str,
    a: &Map<String, Value>,
    b: &Map<String, Value>,
    diffs: &mut Vec<String>,
) {
    for (key, value) in a {
        let child = format!("{path}.{key}");
        match (value, b.get(key)) {
            (Value::Null, None | Some(Value::Null)) => {}
            (_, None) => diffs.push(format!("{child}: lost ({value})")),
            (_, Some(other)) => compare(&child, value, other, diffs),
        }
    }
    for (key, value) in b {
        if !a.contains_key(key) {
            diffs.push(format!("{path}.{key}: not sent, got back {value}"));
        }
    }
}

fn route_matches(method: &str, path: &str, (methods, pattern): (&str, &str)) -> bool {
    methods.split('|').any(|m| m == method) && Regex::new(pattern).unwrap().is_match(path)
}

#[test]
fn every_recorded_response_round_trips() {
    let har = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recordings/walk/recording.har");
    let har: Value = serde_json::from_slice(&std::fs::read(har).unwrap()).unwrap();
    let host = Regex::new(r"^https?://[^/]+").unwrap();

    let mut checked = 0;
    let mut failures = Vec::new();
    for entry in har["log"]["entries"].as_array().unwrap() {
        let method = entry["request"]["method"].as_str().unwrap();
        let url = entry["request"]["url"].as_str().unwrap();
        let path = host.replace(url, "");
        let path = path.split('?').next().unwrap();
        let status = entry["response"]["status"].as_u64().unwrap();
        let content = &entry["response"]["content"];
        let mime = content["mimeType"].as_str().unwrap_or_default();
        let text = content["text"].as_str().unwrap_or_default();

        // Error bodies are kept verbatim as a `Value` on the error, and empty
        // bodies (204) have nothing to decode.
        if !(200..300).contains(&status) || text.is_empty() || !mime.contains("json") {
            continue;
        }
        let body: Value = serde_json::from_str(text).unwrap();
        let label = format!("{method} {url}");

        let check = if path.ends_with("/export") && method == "GET" {
            match export_check(mime) {
                Some(check) => Some(check),
                None => continue,
            }
        } else {
            TYPED
                .iter()
                .find(|(m, p, _)| route_matches(method, path, (m, p)))
                .map(|(_, _, check)| *check)
        };
        match check {
            Some(check) => {
                checked += 1;
                if let Err(e) = check(&body) {
                    failures.push(format!("{label}\n    {e}"));
                }
            }
            None => {
                if !UNTYPED
                    .iter()
                    .any(|(m, p, _)| route_matches(method, path, (m, p)))
                {
                    failures.push(format!("{label}\n    no type mapped for this route"));
                }
            }
        }
    }

    assert!(checked > 0, "no response checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} responses do not round-trip:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn an_unmodelled_field_fails_the_round_trip() {
    let body = serde_json::json!({"records": [], "count": 0, "unmodelled": {"a": 1}});

    let err = round_trip::<Paginated<Tag>>(&body).unwrap_err();

    assert!(err.contains("$.unmodelled: lost"), "{err}");
}
