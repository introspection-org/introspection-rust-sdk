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
//! 2. it calls a `/v1/...` route that is in neither document.
//!
//! Both compare method and path only.
//!
//! `#[ignore]`d, following this repository's convention for tests that reach
//! the network. The `API Contract` workflow runs it against staging: on pull
//! requests with `INTROSPECTION_API_CONTRACT_UNCALLED=report`, so only check 2
//! fails and a new API operation doesn't block unrelated work; after each
//! staging deploy of the API with both checks failing. A document staging
//! answers with 404 (not deployed yet) skips the check.
//!
//! Run against staging:
//!
//!     cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture
//!
//! Run against local copies (a path or a URL each):
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

const CONTROLPLANE_URL: &str = "https://api.staging.introspection.dev/openapi/controlplane.json";
const DATAPLANE_URL: &str = "https://api.staging.introspection.dev/openapi/dataplane.json";

/// A local file or another URL to read instead of staging, for the window
/// where an API change has landed in a branch but not on staging yet.
const CONTROLPLANE_ENV: &str = "INTROSPECTION_CONTROLPLANE_SPEC";
const DATAPLANE_ENV: &str = "INTROSPECTION_DATAPLANE_SPEC";

const REPORT_ENV: &str = "INTROSPECTION_API_CONTRACT_REPORT";
/// `report` lists operations not called without failing on them.
const UNCALLED_ENV: &str = "INTROSPECTION_API_CONTRACT_UNCALLED";

/// The document, or `Err` with why the check is skipped: a URL that answers
/// 404.
fn load(plane: &'static str, env_var: &str, url: &str) -> Result<Document, String> {
    let source = std::env::var(env_var).unwrap_or_else(|_| url.to_owned());
    let fail =
        |e: &dyn std::fmt::Display| format!("could not read the API reference {source}: {e}");
    let spec = if source.starts_with("https://") || source.starts_with("http://") {
        let response = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("client builds")
            .get(&source)
            .send()
            .unwrap_or_else(|e| panic!("{}", fail(&e)));
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(format!("{source} answered 404"));
        }
        response
            .error_for_status()
            .and_then(reqwest::blocking::Response::json)
            .unwrap_or_else(|e| panic!("{}", fail(&e)))
    } else {
        // Cargo runs tests in the crate directory; read a relative path from
        // the repository root, where the documented commands run.
        let text = std::fs::read_to_string(repo_root().join(&source))
            .unwrap_or_else(|e| panic!("{}", fail(&e)));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}", fail(&e)))
    };
    Ok(Document {
        plane,
        source,
        spec,
    })
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
    let documents: Result<Vec<Document>, String> = documents.into_iter().collect();
    let (text, passed, summary) = match documents {
        Ok(documents) => {
            let mut report = check(&documents, &sdk_source());
            report.uncalled_fail = std::env::var(UNCALLED_ENV).as_deref() != Ok("report");
            (report.render(), report.passed(), report.summary())
        }
        Err(why) => (format!("API contract check skipped: {why}\n"), true, why),
    };
    println!("{text}");
    if let Ok(path) = std::env::var(REPORT_ENV) {
        std::fs::write(repo_root().join(&path), &text)
            .unwrap_or_else(|e| panic!("{REPORT_ENV}={path}: {e}"));
    }
    assert!(
        passed,
        "the SDK disagrees with the API reference: {summary}"
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
    let sdk = serde_json::json!({"visibility": "public", "clients": ["sdk"]});
    let cli = serde_json::json!({"visibility": "public", "clients": ["cli"]});
    let spec = serde_json::json!({
        "paths": {
            "/v1/widgets": {"post": {"x-introspection": sdk}, "get": {"x-introspection": sdk}},
            "/v1/widgets/{widget_id}": {"get": {"x-introspection": sdk}, "put": {}},
            "/v1/widgets/{widget_id}/events": {"get": {"x-introspection": sdk}},
            "/v1/widgets/{widget_id}/archive": {"post": {"x-introspection": sdk}},
            "/v1/widgets/stats": {"get": {"x-introspection": cli}}
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

    // Only operations flagged for the SDK must be called: the cli-only
    // `stats` is not reported, and neither is the unflagged `put`.
    assert_eq!(
        report.not_called,
        ["dataplane POST /v1/widgets/{widget_id}/archive"]
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
