//! Checks this SDK against the Control Plane and Data Plane OpenAPI documents.
//!
//! The documents classify every route: each public operation carries
//! `x-introspection: {visibility: public, clients: [...]}`, and a query
//! parameter no client is expected to send carries `clients: []`. Those flags
//! are the only classification; this check keeps no list of its own.
//!
//! The check reads `crates/introspection-sdk/src` (see `api_contract/calls.rs`
//! for how) and fails when:
//!
//! 1. an operation whose `clients` include `sdk` is not called anywhere in it;
//! 2. it calls a `/v1/...` route that is in neither document;
//! 3. a field differs for an operation it calls: names, required vs optional
//!    and basic types, for the request body, the query parameters and the
//!    response, following `$ref` into nested schemas
//!    (`api_contract/compare.rs` has the rules).
//!
//! A call site without concrete types (a `serde_json::Value`, a generic, a
//! response read after the call) is a warning naming the operation, the part
//! and the call site. It does not fail the check.
//!
//! `#[ignore]`d, following this repository's convention for tests that reach
//! the network. The nightly `API Contract` workflow runs it against staging;
//! pull requests do not, because it goes red when the *API* changes, which is
//! a fact about the world and not about the commit under review.
//!
//! Run against staging:
//!
//!     cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture
//!
//! Run against local copies (a path or a URL each):
//!
//!     INTROSPECTION_CONTROLPLANE_SPEC=api-reference/controlplane.json \
//!     INTROSPECTION_DATAPLANE_SPEC=api-reference/dataplane.json \
//!     cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture
//!
//! `INTROSPECTION_API_CONTRACT_REPORT=<file>` also writes the report there,
//! which the workflow reads to build its alert.

mod api_contract;

use std::path::Path;

use api_contract::index::Index;
use api_contract::spec::Document;
use api_contract::{calls, check};

const CONTROLPLANE_URL: &str = "https://api.staging.introspection.dev/openapi/controlplane.json";
const DATAPLANE_URL: &str = "https://api.staging.introspection.dev/openapi/dataplane.json";

/// A local file or another URL to read instead of staging, for the window
/// where an API change has landed in a branch but not on staging yet.
const CONTROLPLANE_ENV: &str = "INTROSPECTION_CONTROLPLANE_SPEC";
const DATAPLANE_ENV: &str = "INTROSPECTION_DATAPLANE_SPEC";

const REPORT_ENV: &str = "INTROSPECTION_API_CONTRACT_REPORT";

fn load(plane: &'static str, env_var: &str, url: &str) -> Document {
    let source = std::env::var(env_var).unwrap_or_else(|_| url.to_owned());
    let spec = if source.starts_with("https://") || source.starts_with("http://") {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("client builds")
            .get(&source)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .unwrap_or_else(|e| panic!("could not read the API reference {source}: {e}"))
            .json()
            .unwrap_or_else(|e| panic!("could not read the API reference {source}: {e}"))
    } else {
        // Cargo runs tests in the crate directory; read a relative path from
        // the repository root, where the documented commands run.
        let text = std::fs::read_to_string(repo_root().join(&source))
            .unwrap_or_else(|e| panic!("could not read the API reference {source}: {e}"));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("could not read the API reference {source}: {e}"))
    };
    Document {
        plane,
        source,
        spec,
    }
}

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sdk_source() -> Index {
    Index::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
}

#[test]
#[ignore = "reaches the network: reads the staging API reference"]
fn sdk_matches_the_api_reference() {
    let documents = [
        load("controlplane", CONTROLPLANE_ENV, CONTROLPLANE_URL),
        load("dataplane", DATAPLANE_ENV, DATAPLANE_URL),
    ];
    let report = check(&documents, &sdk_source());
    let text = report.render();
    println!("{text}");
    if let Ok(path) = std::env::var(REPORT_ENV) {
        std::fs::write(repo_root().join(&path), &text)
            .unwrap_or_else(|e| panic!("{REPORT_ENV}={path}: {e}"));
    }
    assert!(
        report.passed(),
        "the SDK disagrees with the API reference: {}",
        report.summary()
    );
}

/// The reader finds the SDK's requests. Runs on every pull request, so a
/// refactor that hides call sites from it fails here rather than as a wave
/// of "not called" findings the next night.
#[test]
fn reads_the_sdk_source() {
    let index = sdk_source();
    let sites = calls::call_sites(&index);
    assert!(sites.len() > 100, "only {} call sites found", sites.len());
    for site in &sites {
        assert!(
            site.key()
                .split_once(' ')
                .is_some_and(|(_, p)| p.starts_with("/v1/")),
            "{} at {} is not a /v1 route",
            site.key(),
            site.location
        );
    }
    let keys: Vec<String> = sites.iter().map(calls::CallSite::key).collect();
    for expected in [
        "GET /v1/tasks/{}/runs/{}/stream",
        "POST /v1/oauth/token",
        "GET /v1/repositories/{}/contents/{}",
        "GET /v1/conversations/{}/items",
        "PATCH /v1/tags/{}/{}",
    ] {
        assert!(keys.iter().any(|k| k == expected), "{expected} not found");
    }
}

const SOURCE: &str = r#"
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct WidgetCreate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    #[serde(rename = "colour")]
    pub color: String,
}

#[derive(Deserialize)]
pub struct Widget {
    pub id: String,
    pub size: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Deserialize)]
pub struct Part {
    pub label: String,
}

#[derive(Deserialize)]
pub struct Paginated<T> {
    pub records: Vec<T>,
    pub count: u64,
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Deserialize)]
pub struct Count {
    pub count: u64,
}

#[derive(Serialize)]
pub struct WidgetListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Widgets {
    pub async fn create(&self, params: &WidgetCreate) -> ApiResult<Widget> {
        self.http.post_json("/v1/widgets", params).await
    }

    pub fn list(&self, params: &WidgetListParams) -> Paginator<Widget> {
        Paginator::new(self.http.clone(), "/v1/widgets", params).expect("serializes")
    }

    pub async fn get(&self, id: &str) -> ApiResult<Widget> {
        let path = format!("/v1/widgets/{}?project={}", id, "p");
        self.http.get_json(&path, &()).await
    }

    pub async fn touch(&self, id: &str) -> ApiResult<()> {
        let body = serde_json::json!({ "at": 1 });
        self.http.post_json_empty(&format!("/v1/widgets/{id}/touch"), &body).await
    }

    pub async fn legacy(&self) -> ApiResult<()> {
        self.http.delete_empty("/v1/gadgets").await
    }

    pub async fn count(&self, params: &WidgetListParams) -> ApiResult<Count> {
        self.http.get_json("/v1/widgets/count", params).await
    }

    pub async fn watch(&self, id: &str) -> ApiResult<Response> {
        let path = format!("/v1/widgets/{id}/events");
        self.http.get_stream_raw(&path, Some("text/event-stream"), None).await
    }
}
"#;

fn reference() -> Document {
    let widget = serde_json::json!({
        "type": "object",
        "required": ["id", "parts"],
        "properties": {
            "id": {"type": "string"},
            "size": {"anyOf": [{"type": "integer"}, {"type": "null"}]},
            "parts": {"type": "array", "items": {"$ref": "#/components/schemas/Part"}},
            "created_at": {"type": "string"}
        }
    });
    let page = serde_json::json!({
        "type": "object",
        "properties": {
            "records": {"type": "array", "items": {"$ref": "#/components/schemas/Widget"}},
            "count": {"type": "integer"},
            "total_count": {"type": "integer"},
            "next": {"type": "string"}
        }
    });
    let json =
        |schema: serde_json::Value| serde_json::json!({"application/json": {"schema": schema}});
    let ok = |schema: serde_json::Value| serde_json::json!({"200": {"content": json(schema)}});
    let sdk = serde_json::json!({"visibility": "public", "clients": ["sdk"]});
    let query = |name: &str, flags: serde_json::Value| serde_json::json!({"name": name, "in": "query", "schema": {"type": "string"}, "x-introspection": flags});
    let spec = serde_json::json!({
        "paths": {
            "/v1/widgets": {
                "post": {
                    "x-introspection": sdk,
                    "requestBody": {"required": true, "content": json(serde_json::json!({
                        "type": "object",
                        "required": ["name", "size"],
                        "properties": {"name": {"type": "string"}, "size": {"type": "integer"}, "colour": {"type": "string"}}
                    }))},
                    "responses": ok(serde_json::json!({"$ref": "#/components/schemas/Widget"}))
                },
                "get": {
                    "x-introspection": sdk,
                    "parameters": [
                        {"name": "limit", "in": "query", "schema": {"type": "integer"}},
                        {"name": "next", "in": "query", "schema": {"type": "string"}},
                        query("project_id", serde_json::json!({"clients": []}))
                    ],
                    "responses": ok(page)
                }
            },
            "/v1/widgets/{widget_id}": {
                "get": {
                    "x-introspection": sdk,
                    "parameters": [{"name": "project", "in": "query", "schema": {"type": "string"}}],
                    "responses": ok(serde_json::json!({"$ref": "#/components/schemas/Widget"}))
                }
            },
            "/v1/widgets/{widget_id}/touch": {
                "post": {
                    "x-introspection": sdk,
                    "requestBody": {"content": json(serde_json::json!({"type": "object", "properties": {"at": {"type": "integer"}}}))},
                    "responses": {"204": {}}
                }
            },
            "/v1/widgets/count": {
                "get": {"responses": ok(serde_json::json!({"type": "object", "properties": {"count": {"type": "integer"}}}))}
            },
            "/v1/widgets/{widget_id}/events": {
                "get": {"responses": ok(serde_json::json!({"type": "object"}))}
            },
            "/v1/widgets/{widget_id}/archive": {
                "post": {"x-introspection": sdk, "responses": {"204": {}}}
            }
        },
        "components": {"schemas": {
            "Widget": widget,
            "Part": {"type": "object", "required": ["label"], "properties": {"label": {"type": "string"}}}
        }}
    });
    Document {
        plane: "dataplane",
        source: "inline".to_owned(),
        spec,
    }
}

#[test]
fn reports_every_kind_of_finding() {
    let index = Index::from_source("widgets.rs", SOURCE);
    let report = check(&[reference()], &index);
    let text = report.render();
    println!("{text}");

    assert_eq!(
        report.not_called,
        ["dataplane POST /v1/widgets/{widget_id}/archive"]
    );
    assert_eq!(
        report.unknown_routes,
        ["DELETE /v1/gadgets at src/widgets.rs:64"]
    );
    for line in [
        "  POST /v1/widgets (dataplane)",
        "    field missing in SDK: response.created_at",
        "    type mismatch: response.size (number in the reference, string in the SDK)",
        "    required mismatch: body.size (required in the reference, optional in the SDK)",
        // A paginator sends the cursor itself; `project_id` is `clients: []`.
        "  GET /v1/widgets (dataplane)",
        "    field missing in SDK: response.records[].created_at, response.total_count",
        "  POST /v1/widgets/{widget_id}/touch (dataplane)",
        "    body: serde_json::json! builds an untyped value at src/widgets.rs:60",
        // An operation without query parameters still has extra ones.
        "  GET /v1/widgets/count (dataplane)",
        "    field extra in SDK: query.limit",
        // A raw response's media type is checked too.
        "  GET /v1/widgets/{widget_id}/events (dataplane)",
        "    media type: response (the SDK reads text/event-stream; the reference's response has application/json)",
    ] {
        assert!(
            text.contains(&format!("{line}\n")),
            "missing line {line:?} in:\n{text}"
        );
    }
    // `?project=` in the route is a query parameter the SDK sends.
    assert!(!text.contains("query.project "), "{text}");
    assert!(!text.contains("query.project_id"), "{text}");
    assert!(!text.contains("query.next"), "{text}");
    assert!(!report.passed());
}
