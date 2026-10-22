//! The API contract check: the SDK source against the Control Plane and Data
//! Plane OpenAPI documents. See `api_contract_test.rs` for what it fails on
//! and how to run it.

pub mod calls;
pub mod compare;
pub mod index;
pub mod routes;
pub mod shape;
pub mod spec;
pub mod types;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use calls::{CallSite, Part};
use compare::{Comparison, Diff, Kind, Warning};
use index::Index;
use shape::{Field, Shape};
use spec::{media, Document, Operation};
use types::{Dir, Lower};

/// The client this check reads the flags for.
const CLIENT: &str = "sdk";

#[derive(Default)]
pub struct Report {
    pub sources: Vec<String>,
    pub call_sites: usize,
    /// `controlplane GET /v1/organizations/current`.
    pub not_called: Vec<String>,
    /// `GET /v1/members at src/resources/members.rs:38`.
    pub unknown_routes: Vec<String>,
    /// By operation (`POST /v1/tasks (dataplane)`): each difference, with the
    /// call sites it was found at.
    pub diffs: BTreeMap<String, OperationDiffs>,
    pub warnings: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Default)]
pub struct OperationDiffs {
    sites: BTreeSet<String>,
    found: BTreeMap<Diff, BTreeSet<String>>,
}

impl Report {
    pub fn failures(&self) -> usize {
        let diffs: usize = self.diffs.values().map(|d| d.found.len()).sum();
        self.not_called.len() + self.unknown_routes.len() + diffs
    }

    pub fn passed(&self) -> bool {
        self.failures() == 0
    }

    fn warning_count(&self) -> usize {
        self.warnings.values().map(BTreeSet::len).sum()
    }

    /// One line, for the alert's summary.
    pub fn summary(&self) -> String {
        let diffs: usize = self.diffs.values().map(|d| d.found.len()).sum();
        format!(
            "{} operations not called, {} routes in neither reference, {} field differences in {} operations; {} warnings",
            self.not_called.len(),
            self.unknown_routes.len(),
            diffs,
            self.diffs.len(),
            self.warning_count(),
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
        if !self.diffs.is_empty() {
            let _ = writeln!(
                out,
                "\nField differences ({} operations):",
                self.diffs.len()
            );
            for (operation, diffs) in &self.diffs {
                let _ = writeln!(out, "  {operation}");
                for line in diffs.lines() {
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
        if !self.warnings.is_empty() {
            let _ = writeln!(
                out,
                "\nWarning: SDK has no concrete types ({}; printed, do not fail):",
                self.warning_count()
            );
            for (operation, lines) in &self.warnings {
                let _ = writeln!(out, "  {operation}");
                for line in lines {
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
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

impl OperationDiffs {
    /// One line per kind of difference. A difference found at only some of
    /// the operation's call sites names them.
    fn lines(&self) -> Vec<String> {
        let mut by_kind: BTreeMap<Kind, Vec<String>> = BTreeMap::new();
        for (diff, at) in &self.found {
            let mut item = diff.path.clone();
            if let Some(detail) = &diff.detail {
                let _ = write!(item, " ({detail})");
            }
            if at.len() < self.sites.len() {
                let sites: Vec<&str> = at.iter().map(String::as_str).collect();
                let _ = write!(item, " at {}", sites.join(", "));
            }
            by_kind.entry(diff.kind).or_default().push(item);
        }
        by_kind
            .into_iter()
            .map(|(kind, items)| format!("{}: {}", kind.label(), items.join(", ")))
            .collect()
    }
}

/// Runs the check: the SDK source in `index` against `documents`.
pub fn check(documents: &[Document], index: &Index) -> Report {
    let sites = calls::call_sites(index);
    let operations: Vec<Operation> = documents.iter().flat_map(Document::operations).collect();
    let mut report = Report {
        sources: documents.iter().map(|d| d.source.clone()).collect(),
        call_sites: sites.len(),
        ..Report::default()
    };

    let called: HashSet<String> = sites.iter().map(CallSite::key).collect();
    report.not_called = operations
        .iter()
        .filter(|op| op.is_for(CLIENT) && !called.contains(&op.key()))
        .map(|op| format!("{} {}", op.plane, op.label()))
        .collect();
    report.not_called.sort();

    for site in &sites {
        let matching: Vec<&Operation> = operations
            .iter()
            .filter(|op| op.key() == site.key())
            .collect();
        let Some(op) = matching
            .iter()
            .find(|op| op.is_for(CLIENT))
            .or(matching.first())
        else {
            report
                .unknown_routes
                .push(format!("{} at {}", site.label(), site.location));
            continue;
        };
        let name = format!("{} ({})", op.label(), op.plane);
        let comparison = compare_site(op, site, index);
        let diffs = report.diffs.entry(name.clone()).or_default();
        diffs.sites.insert(site.location.clone());
        for diff in comparison.diffs {
            diffs
                .found
                .entry(diff)
                .or_default()
                .insert(site.location.clone());
        }
        for Warning { path, why } in dedup_warnings(comparison.warnings) {
            let line = format!("{path}: {why} at {}", site.location);
            report
                .warnings
                .entry(name.clone())
                .or_default()
                .insert(line);
        }
    }
    report.unknown_routes.sort();
    report.unknown_routes.dedup();
    report.diffs.retain(|_, d| !d.found.is_empty());
    report
}

fn dedup_warnings(mut warnings: Vec<Warning>) -> Vec<Warning> {
    warnings.sort();
    warnings.dedup();
    warnings
}

fn compare_site(op: &Operation, site: &CallSite, index: &Index) -> Comparison {
    let mut comparison = Comparison::default();
    compare_body(op, site, index, &mut comparison);
    compare_query(op, site, index, &mut comparison);
    compare_response(op, site, index, &mut comparison);
    comparison
}

fn compare_body(op: &Operation, site: &CallSite, index: &Index, out: &mut Comparison) {
    let declared = op.request_body();
    let accepts = |content: &serde_json::Map<String, serde_json::Value>| {
        content.keys().cloned().collect::<Vec<_>>().join(", ")
    };
    match (&site.body, declared) {
        (Part::Absent, None) => {}
        (Part::Absent, Some((required, content))) => {
            if required {
                let detail = "required by the reference; the SDK sends none".to_owned();
                out.diff(Kind::Media, "body", Some(detail));
            }
            if let Some(json) = media(content, "application/json") {
                out.compare(&op.lower(&json["schema"]), &Shape::object([]), "body");
            }
        }
        (Part::Media(sent), Some((_, content))) if media(content, sent).is_none() => {
            let detail = format!(
                "the SDK sends {sent}; the reference accepts {}",
                accepts(content)
            );
            out.diff(Kind::Media, "body", Some(detail));
        }
        (Part::Media(_), Some(_)) => {}
        (sent, None) => {
            let what = match sent {
                Part::Media(m) => m.clone(),
                _ => "a JSON body".to_owned(),
            };
            let detail = format!("the SDK sends {what}; the reference declares no body");
            out.diff(Kind::Media, "body", Some(detail));
        }
        (Part::Untyped(why), Some(_)) => out.warn("body", why),
        (Part::Typed(ty, scope), Some((_, content))) => match media(content, "application/json") {
            Some(json) => {
                let sdk = Lower::new(index, Dir::Send).shape(ty, *scope);
                out.compare(&op.lower(&json["schema"]), &sdk, "body");
            }
            None => {
                let detail = format!(
                    "the SDK sends JSON; the reference accepts {}",
                    accepts(content)
                );
                out.diff(Kind::Media, "body", Some(detail));
            }
        },
        (Part::Raw | Part::NotRead, Some(_)) => {}
    }
}

fn compare_query(op: &Operation, site: &CallSite, index: &Index, out: &mut Comparison) {
    let mut sdk = match &site.query {
        Part::Typed(ty, scope) => Lower::new(index, Dir::Send).shape(ty, *scope),
        Part::Untyped(why) => {
            out.warn("query", why);
            return;
        }
        _ => Shape::object([]),
    };
    if !matches!(sdk, Shape::Object(_) | Shape::Untyped(_)) {
        let why = format!(
            "the query is built as {} rather than named fields",
            sdk.kind()
        );
        return out.warn("query", &why);
    }
    if let Shape::Object(object) = &mut sdk {
        for (name, required) in &site.route_query {
            let field = Field {
                shape: Shape::String,
                required: *required,
            };
            object.fields.entry(name.clone()).or_insert(field);
        }
    }
    out.query = true;
    out.not_expected = op.unexpected_parameters();
    out.compare(&op.query(), &sdk, "query");
    out.query = false;
    out.not_expected.clear();
}

fn compare_response(op: &Operation, site: &CallSite, index: &Index, out: &mut Comparison) {
    match &site.response {
        Part::NotRead | Part::Absent | Part::Media(_) => return,
        Part::Untyped(why) => out.warn("response", why),
        Part::Typed(..) | Part::Raw => {}
    }
    // The media type is checked whenever the call names one, even for a
    // response the SDK reads as bytes, a stream or Arrow.
    let Ok(wanted) = &site.accept else { return };
    let Some(content) = op.response() else { return };
    let Some(declared) = media(&content, wanted) else {
        let offers = if content.is_empty() {
            "no body".to_owned()
        } else {
            content.keys().cloned().collect::<Vec<_>>().join(", ")
        };
        let detail = format!("the SDK reads {wanted}; the reference's response has {offers}");
        return out.diff(Kind::Media, "response", Some(detail));
    };
    if let Part::Typed(ty, scope) = &site.response {
        let sdk = Lower::new(index, Dir::Read).shape(ty, *scope);
        out.compare(&op.lower(&declared["schema"]), &sdk, "response");
    }
}
