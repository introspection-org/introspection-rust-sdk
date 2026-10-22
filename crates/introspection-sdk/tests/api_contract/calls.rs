//! Every request the SDK source makes: its method and route.
//!
//! A request is a call of one of the SDK's HTTP client methods
//! (`self.http.get_json(path, query)`, ...), a `Paginator` constructor, or a
//! `reqwest` builder (`client.post(url)`). `routes.rs` follows the route
//! argument to the `/v1/...` routes it can be.

use quote::ToTokens;
use syn::visit::Visit;
use syn::Expr;

use super::index::Index;
use super::normalize_path;
use super::routes::{macro_stmts, FnScope, Routes};

pub struct CallSite {
    pub method: &'static str,
    /// The route as built, without its `?query`.
    pub path: String,
    /// `src/api/tasks.rs:12`.
    pub location: String,
}

impl CallSite {
    /// `GET /v1/tasks/{}`, matched against the documents' operations.
    pub fn key(&self) -> String {
        format!("{} {}", self.method, normalize_path(&self.path))
    }
}

/// The SDK's HTTP client methods, each with the route as its first argument.
const CLIENT_METHODS: [(&str, &str); 12] = [
    ("get_json", "GET"),
    ("post_json", "POST"),
    ("post_json_or_no_content", "POST"),
    ("post_json_empty", "POST"),
    ("patch_json", "PATCH"),
    ("post_empty", "POST"),
    ("delete_empty", "DELETE"),
    ("post_multipart", "POST"),
    ("get_raw", "GET"),
    ("get_bytes", "GET"),
    ("get_stream", "GET"),
    ("get_stream_raw", "GET"),
];

const REQWEST_VERBS: [(&str, &str); 5] = [
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
];

/// Every call site in the indexed source.
pub fn call_sites(index: &Index) -> Vec<CallSite> {
    let routes = Routes::new(index);
    let mut sites = Vec::new();
    for id in 0..index.fns.len() {
        let scope = FnScope::new(index, id);
        let mut walker = Walker {
            routes: &routes,
            scope: &scope,
            sites: Vec::new(),
        };
        walker.visit_block(&index.fns[id].block);
        sites.extend(walker.sites);
    }
    sites.sort_by(|a, b| a.location.cmp(&b.location));
    sites
}

struct Walker<'w, 'a> {
    routes: &'w Routes<'a>,
    scope: &'w FnScope<'a>,
    sites: Vec<CallSite>,
}

impl<'ast> Visit<'ast> for Walker<'_, '_> {
    /// An item nested in a function body is not part of that function.
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    /// A macro whose body is code (`stream! { ... }`) is walked like a block.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(stmts) = macro_stmts(mac) {
            for stmt in &stmts {
                self.visit_stmt(stmt);
            }
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        let client = CLIENT_METHODS.iter().find(|(n, _)| *n == name);
        // A reqwest builder takes the URL as its only argument.
        let verb = REQWEST_VERBS
            .iter()
            .find(|(n, _)| *n == name)
            .filter(|_| call.args.len() == 1);
        if let (Some((_, method)), Some(route)) = (client.or(verb), call.args.first()) {
            self.push(method, route, call.method.span().start().line);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let (true, Some(route)) = (is_paginator(call), call.args.iter().nth(1)) {
            let line = call
                .func
                .to_token_stream()
                .into_iter()
                .next()
                .map_or(0, |t| t.span().start().line);
            self.push("GET", route, line);
        }
        syn::visit::visit_expr_call(self, call);
    }
}

impl Walker<'_, '_> {
    /// One call site per path `route` can be; alternatives that differ only
    /// in their `?query` are one request.
    fn push(&mut self, method: &'static str, route: &Expr, line: usize) {
        let mut paths: Vec<String> = self
            .routes
            .of(self.scope, route)
            .iter()
            .map(|r| r.split('?').next().unwrap_or_default().to_owned())
            .collect();
        paths.sort();
        paths.dedup();
        let file = &self.routes.index.files[self.scope.def.file];
        for path in paths {
            self.sites.push(CallSite {
                method,
                path,
                location: format!("{file}:{line}"),
            });
        }
    }
}

fn is_paginator(call: &syn::ExprCall) -> bool {
    let Expr::Path(p) = &*call.func else {
        return false;
    };
    let segments: Vec<String> = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    matches!(
        segments.as_slice(),
        [.., owner, ctor] if owner == "Paginator"
            && matches!(ctor.as_str(), "new" | "with_cursor_param" | "with_decoder")
    )
}
