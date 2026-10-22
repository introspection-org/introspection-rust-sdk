//! The API contract check: the SDK's routes against the Control Plane and
//! Data Plane OpenAPI documents. See `api_contract_test.rs` for what it fails
//! on and how to run it.

pub mod calls;
pub mod index;
pub mod routes;

use std::collections::HashSet;
use std::fmt::Write as _;

use serde_json::Value;

use calls::CallSite;
use index::Index;

/// The client this check reads the flags for.
const CLIENT: &str = "sdk";

const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// One plane's document and where it was read from.
pub struct Document {
    pub plane: &'static str,
    pub source: String,
    pub spec: Value,
}

/// One operation of a document.
struct Operation {
    plane: &'static str,
    method: String,
    path: String,
    for_client: bool,
}

impl Document {
    fn operations(&self) -> Vec<Operation> {
        let Some(paths) = self.spec["paths"].as_object() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (path, item) in paths {
            for method in METHODS {
                let op = &item[method];
                if !op.is_object() {
                    continue;
                }
                let clients = op["x-introspection"]["clients"].as_array();
                out.push(Operation {
                    plane: self.plane,
                    method: method.to_uppercase(),
                    path: path.clone(),
                    for_client: clients.is_some_and(|c| c.iter().any(|c| c == CLIENT)),
                });
            }
        }
        out
    }
}

impl Operation {
    fn key(&self) -> String {
        format!("{} {}", self.method, normalize_path(&self.path))
    }
}

/// A route with every path parameter written `{}`, so `/v1/tasks/{task_id}`
/// and the SDK's `format!("/v1/tasks/{}", id)` meet by position.
pub fn normalize_path(path: &str) -> String {
    path.trim_end_matches('/')
        .split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Default)]
pub struct Report {
    pub sources: Vec<String>,
    pub call_sites: usize,
    /// `controlplane GET /v1/organizations/current`.
    pub not_called: Vec<String>,
    /// `GET /v1/members at src/resources/members.rs:38`.
    pub unknown_routes: Vec<String>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.not_called.is_empty() && self.unknown_routes.is_empty()
    }

    /// One line, for the alert's summary.
    pub fn summary(&self) -> String {
        format!(
            "{} operations not called, {} routes in neither reference",
            self.not_called.len(),
            self.unknown_routes.len(),
        )
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let verdict = if self.passed() { "passed" } else { "FAILED" };
        let _ = writeln!(out, "API contract check {verdict}: {}", self.summary());
        let _ = writeln!(out, "reference: {}", self.sources.join(" + "));
        let _ = writeln!(
            out,
            "call sites read: {} in crates/introspection-sdk/src",
            self.call_sites
        );
        section(
            &mut out,
            "Operation not called (x-introspection clients include sdk; no call site in crates/introspection-sdk/src)",
            &self.not_called,
        );
        section(
            &mut out,
            "Route missing from both references (the SDK calls a /v1 route neither document has)",
            &self.unknown_routes,
        );
        out
    }
}

fn section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n{title} ({}):", items.len());
    for item in items {
        let _ = writeln!(out, "  {item}");
    }
}

/// Runs the check: the SDK source in `index` against `documents`.
pub fn check(documents: &[Document], index: &Index) -> Report {
    let sites = calls::call_sites(index);
    let operations: Vec<Operation> = documents.iter().flat_map(Document::operations).collect();
    let called: HashSet<String> = sites.iter().map(CallSite::key).collect();
    let known: HashSet<String> = operations.iter().map(Operation::key).collect();

    let mut not_called: Vec<String> = operations
        .iter()
        .filter(|op| op.for_client && !called.contains(&op.key()))
        .map(|op| format!("{} {} {}", op.plane, op.method, op.path))
        .collect();
    not_called.sort();
    let mut unknown_routes: Vec<String> = sites
        .iter()
        .filter(|site| !known.contains(&site.key()))
        .map(|site| format!("{} {} at {}", site.method, site.path, site.location))
        .collect();
    unknown_routes.sort();
    unknown_routes.dedup();

    Report {
        sources: documents.iter().map(|d| d.source.clone()).collect(),
        call_sites: sites.len(),
        not_called,
        unknown_routes,
    }
}
