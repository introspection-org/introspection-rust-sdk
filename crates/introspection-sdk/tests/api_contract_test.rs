//! Checks this SDK against the Control Plane and Data Plane OpenAPI documents.
//!
//! Each plane serves its own document at `/openapi.json`. An operation is for
//! every client unless it carries `x-introspection: {clients: [...]}`, which
//! names the clients expected to call it; `[]` names none. Those flags are the
//! only classification; this check keeps no list of its own.
//!
//! The check reads `crates/introspection-sdk/src` (see `api_contract/calls.rs`
//! for how) and fails when:
//!
//! 1. an operation for the SDK (no `clients`, or `clients` include `sdk`) is
//!    not called anywhere in it;
//! 2. it calls a `/v1/...` route that is in neither document.
//!
//! Both compare method and path only.
//!
//! `#[ignore]`d, following this repository's convention for tests that reach
//! the network. The `API Contract` workflow runs it against staging: on pull
//! requests with `INTROSPECTION_API_CONTRACT_UNCALLED=report`, so only check 2
//! fails and a new API operation doesn't block unrelated work; after each
//! staging deploy of the API with both checks failing. A document that cannot
//! be read (any HTTP error, 404 included, a network error or a body that is
//! not JSON) fails the test before it compares anything.
//!
//! No host is fixed here: `INTROSPECTION_CP_URL` and `INTROSPECTION_DP_URL`
//! name the planes' base URLs, and their `/openapi.json` is read. Run against
//! staging:
//!
//!     INTROSPECTION_CP_URL=https://api.staging.introspection.dev \
//!     INTROSPECTION_DP_URL=https://api.gcp01.us-central1.gcp.introspection.dev \
//!     cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture
//!
//! Run against local copies (a path or a URL each, in place of the base URLs):
//!
//!     INTROSPECTION_CONTROLPLANE_SPEC=path/to/controlplane.json \
//!     INTROSPECTION_DATAPLANE_SPEC=path/to/dataplane.json \
//!     cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture
//!
//! `INTROSPECTION_API_CONTRACT_REPORT=<file>` also writes the report there,
//! which the workflow reads for its job summary and alert.

mod api_contract;

use std::path::Path;

use api_contract::index::Index;
use api_contract::{calls, check, Document};

/// Each plane's base URL; its document is `/openapi.json` under it.
const CP_URL_ENV: &str = "INTROSPECTION_CP_URL";
const DP_URL_ENV: &str = "INTROSPECTION_DP_URL";

/// A local file or another URL to read instead, for the window where an API
/// change has landed in a branch but not on staging yet.
const CONTROLPLANE_ENV: &str = "INTROSPECTION_CONTROLPLANE_SPEC";
const DATAPLANE_ENV: &str = "INTROSPECTION_DATAPLANE_SPEC";

const REPORT_ENV: &str = "INTROSPECTION_API_CONTRACT_REPORT";
/// `report` lists operations not called without failing on them.
const UNCALLED_ENV: &str = "INTROSPECTION_API_CONTRACT_UNCALLED";

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The document, from `spec_env` when set, else `/openapi.json` under the
/// base URL in `url_env`. Panics when it cannot be read.
fn load(plane: &'static str, spec_env: &str, url_env: &str) -> Document {
    let source = env(spec_env)
        .or_else(|| env(url_env).map(|base| format!("{}/openapi.json", base.trim_end_matches('/'))))
        .unwrap_or_else(|| {
            panic!("could not read the API reference: {url_env} is unset or empty; set it to the {plane}'s base URL, or {spec_env} to its document")
        });
    let fail =
        |e: &dyn std::fmt::Display| format!("could not read the API reference {source}: {e}");
    let spec = if source.starts_with("https://") || source.starts_with("http://") {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("client builds")
            .get(&source)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::json)
            .unwrap_or_else(|e| panic!("{}", fail(&e)))
    } else {
        // Cargo runs tests in the crate directory; read a relative path from
        // the repository root, where the documented commands run.
        let text = std::fs::read_to_string(repo_root().join(&source))
            .unwrap_or_else(|e| panic!("{}", fail(&e)));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}", fail(&e)))
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
        load("controlplane", CONTROLPLANE_ENV, CP_URL_ENV),
        load("dataplane", DATAPLANE_ENV, DP_URL_ENV),
    ];
    let mut report = check(&documents, &sdk_source());
    report.uncalled_fail = std::env::var(UNCALLED_ENV).as_deref() != Ok("report");
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
const WIDGETS: &str = "/v1/widgets";

impl Widgets {
    pub async fn create(&self, params: &WidgetCreate) -> ApiResult<Widget> {
        self.http.post_json(WIDGETS, params).await
    }

    pub fn list(&self, params: &WidgetListParams) -> Paginator<Widget> {
        Paginator::new(self.http.clone(), "/v1/widgets", params).expect("serializes")
    }

    pub async fn find(&self, id: &str, project: Option<&str>) -> ApiResult<Widget> {
        let path = match project {
            Some(project) => format!("/v1/widgets/{id}?project={project}"),
            None => format!("/v1/widgets/{id}"),
        };
        self.http.get_json(&path, &()).await
    }

    pub async fn watch(&self, id: &str) -> ApiResult<Response> {
        self.http.get_stream_raw(&events_path(id), Some("text/event-stream"), None).await
    }

    pub async fn replace(&self, id: &str, body: &Widget) -> ApiResult<()> {
        self.client.put(format!("{}/v1/widgets/{}", self.base, id)).json(body).send().await?;
        Ok(())
    }

    pub async fn legacy(&self) -> ApiResult<()> {
        self.http.delete_empty("/v1/gadgets").await
    }
}

fn events_path(id: &str) -> String {
    format!("/v1/widgets/{id}/events")
}
"#;

fn reference() -> Document {
    let sdk = serde_json::json!({"clients": ["sdk"]});
    let both = serde_json::json!({"clients": ["cli", "sdk"]});
    let cli = serde_json::json!({"clients": ["cli"]});
    let none = serde_json::json!({"clients": []});
    let spec = serde_json::json!({
        "paths": {
            "/v1/widgets": {"post": {}, "get": {"x-introspection": sdk}},
            "/v1/widgets/{widget_id}": {"get": {"x-introspection": {}}, "put": {"x-introspection": both}},
            "/v1/widgets/{widget_id}/events": {"get": {"x-introspection": sdk}},
            "/v1/widgets/{widget_id}/archive": {"post": {}},
            "/v1/widgets/{widget_id}/restore": {"post": {"x-introspection": {}}},
            "/v1/widgets/{widget_id}/share": {"post": {"x-introspection": both}},
            "/v1/widgets/stats": {"get": {"x-introspection": cli}},
            "/health": {"get": {"x-introspection": none}}
        }
    });
    Document {
        plane: "dataplane",
        source: "inline".to_owned(),
        spec,
    }
}

#[test]
fn reports_uncalled_operations_and_unknown_routes() {
    let index = Index::from_source("widgets.rs", SOURCE);
    let report = check(&[reference()], &index);
    println!("{}", report.render());

    // An operation without `clients` is for every client, one with `clients`
    // only for those it names: the cli-only `stats` and the no-client
    // `/health` are not reported.
    assert_eq!(
        report.not_called,
        [
            "dataplane POST /v1/widgets/{widget_id}/archive",
            "dataplane POST /v1/widgets/{widget_id}/restore",
            "dataplane POST /v1/widgets/{widget_id}/share",
        ]
    );
    assert_eq!(
        report.unknown_routes,
        ["DELETE /v1/gadgets at src/widgets.rs:31"]
    );
    assert!(!report.passed());

    // On a pull request only the unknown route fails.
    let mut listed = report;
    listed.uncalled_fail = false;
    assert!(!listed.passed());
    listed.unknown_routes.clear();
    assert!(listed.passed());
}
