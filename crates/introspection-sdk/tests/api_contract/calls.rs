//! Every request the SDK source makes: its method and route, and the types of
//! the body and query it sends and the response it reads.
//!
//! A request is a call of one of the SDK's HTTP client methods
//! (`self.http.get_json(path, query)`, ...), a `Paginator` constructor, or a
//! `reqwest` builder (`client.post(url).json(body)`). Types are read from the
//! source alone: a parameter's declared type, a `let` annotation, a struct
//! literal, a function's return type. Where none of those names one (a
//! `serde_json::Value`, a generic, a value read after the call), the part is
//! [`Part::Untyped`] and the check warns instead of comparing it.

use quote::ToTokens;
use syn::visit::Visit;
use syn::{Expr, GenericArgument, PathArguments, Type};

use super::index::{type_name, Index, Scope};
use super::routes::{is_macro, macro_stmts, FnScope, Routes};
use super::spec::normalize_path;

/// What a call site sends or reads as one part of a request.
#[derive(Clone)]
pub enum Part {
    /// Nothing: no body, no query parameters.
    Absent,
    /// JSON of this type.
    Typed(Box<Type>, Scope),
    /// No concrete type at the call site; the string says why.
    Untyped(String),
    /// A non-JSON body of this media type (multipart, form-encoded); only the
    /// media type is compared.
    Media(String),
    /// A response the SDK does not decode as JSON (bytes, a stream, Arrow).
    Raw,
    /// A response the SDK never reads.
    NotRead,
}

pub struct CallSite {
    pub method: &'static str,
    /// The route as built, `?query` and all.
    pub route: String,
    /// `src/api/tasks.rs:12`.
    pub location: String,
    /// Query parameters written into the route itself (`?project={}`), and
    /// whether every call sends them.
    pub route_query: Vec<(String, bool)>,
    pub body: Part,
    pub query: Part,
    pub response: Part,
    /// The media type the response is asked for in.
    pub accept: Result<String, String>,
}

impl CallSite {
    /// `GET /v1/tasks/{}`, matched against [`super::spec::Operation::key`].
    pub fn key(&self) -> String {
        let path = self.route.split('?').next().unwrap_or_default();
        format!("{} {}", self.method, normalize_path(path))
    }

    pub fn label(&self) -> String {
        let path = self.route.split('?').next().unwrap_or_default();
        format!("{} {}", self.method, path)
    }
}

/// How one of the SDK's HTTP client methods sends and reads.
struct ClientMethod {
    name: &'static str,
    method: &'static str,
    body: Option<usize>,
    query: Option<usize>,
    multipart: bool,
    response: Read,
}

#[derive(Clone, Copy, PartialEq)]
enum Read {
    Json,
    /// `Ok(None)` on a 204, so the type read is inside an `Option`.
    JsonOrNothing,
    /// The caller reads the raw response; the `Accept` argument says as what.
    ByAccept(usize),
    Bytes,
    Nothing,
}

const CLIENT_METHODS: [ClientMethod; 12] = [
    client("get_json", "GET", None, Some(1), Read::Json),
    client("post_json", "POST", Some(1), None, Read::Json),
    client(
        "post_json_or_no_content",
        "POST",
        Some(1),
        None,
        Read::JsonOrNothing,
    ),
    client("post_json_empty", "POST", Some(1), None, Read::Nothing),
    client("patch_json", "PATCH", Some(1), None, Read::Json),
    client("post_empty", "POST", None, None, Read::Nothing),
    client("delete_empty", "DELETE", None, None, Read::Nothing),
    ClientMethod {
        multipart: true,
        ..client("post_multipart", "POST", Some(1), None, Read::Json)
    },
    client("get_raw", "GET", None, Some(1), Read::ByAccept(2)),
    client("get_bytes", "GET", None, None, Read::Bytes),
    client("get_stream", "GET", None, None, Read::ByAccept(1)),
    client("get_stream_raw", "GET", None, None, Read::ByAccept(1)),
];

const fn client(
    name: &'static str,
    method: &'static str,
    body: Option<usize>,
    query: Option<usize>,
    response: Read,
) -> ClientMethod {
    ClientMethod {
        name,
        method,
        body,
        query,
        multipart: false,
        response,
    }
}

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
        let def = &index.fns[id];
        let mut walker = Walker {
            routes: &routes,
            scope: &scope,
            expect: return_expect(&def.sig.output),
            sites: Vec::new(),
            resolving: std::cell::RefCell::default(),
        };
        walker.visit_block(&def.block);
        sites.extend(walker.sites);
    }
    sites.sort_by(|a, b| a.location.cmp(&b.location));
    sites
}

/// The type the expression being walked must evaluate to, as far as the code
/// around it says: a `let` annotation, the function's return type, an `Ok(..)`
/// it is wrapped in. `tried` is set once a `?` (or `.expect`) has unwrapped the
/// `Result` between the request and that type.
#[derive(Clone)]
struct Expect {
    ty: Type,
    tried: bool,
}

/// One element of a postfix chain: `.method(args)`, `.await` or `?`.
enum Link<'e> {
    Call(&'e syn::ExprMethodCall),
    Await,
    Try,
}

struct Walker<'w, 'a> {
    routes: &'w Routes<'a>,
    scope: &'w FnScope<'a>,
    expect: Option<Expect>,
    sites: Vec<CallSite>,
    resolving: std::cell::RefCell<Vec<String>>,
}

fn return_expect(output: &syn::ReturnType) -> Option<Expect> {
    match output {
        syn::ReturnType::Type(_, ty) => Some(Expect {
            ty: (**ty).clone(),
            tried: false,
        }),
        syn::ReturnType::Default => None,
    }
}

impl<'ast> Visit<'ast> for Walker<'_, '_> {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let tail = self.expect.take();
        let last = block.stmts.len().saturating_sub(1);
        for (at, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                syn::Stmt::Expr(expr, None) if at == last => {
                    self.expect = tail.clone();
                    self.visit_expr(expr);
                }
                syn::Stmt::Item(_) => {}
                other => self.visit_stmt(other),
            }
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let Some(init) = &local.init else { return };
        self.expect = match &local.pat {
            syn::Pat::Type(t) => Some(Expect {
                ty: (*t.ty).clone(),
                tried: false,
            }),
            _ => None,
        };
        self.visit_expr(&init.expr);
        if let Some((_, diverge)) = &init.diverge {
            self.visit_expr(diverge);
        }
    }

    fn visit_expr_return(&mut self, ret: &'ast syn::ExprReturn) {
        if let Some(expr) = &ret.expr {
            self.expect = return_expect(&self.scope.def.sig.output);
            self.visit_expr(expr);
        }
    }

    /// A macro whose body is code (`stream! { ... }`) is walked like a block.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(stmts) = macro_stmts(mac) {
            for stmt in &stmts {
                self.visit_stmt(stmt);
            }
        }
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.expect = None;
        self.visit_expr(&closure.body);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        let expect = self.expect.take();
        match expr {
            Expr::MethodCall(_) | Expr::Await(_) | Expr::Try(_) => self.chain(expr, expect),
            Expr::Call(call) => self.call(call, expect),
            Expr::Paren(p) => {
                self.expect = expect;
                self.visit_expr(&p.expr);
            }
            Expr::Block(b) => {
                self.expect = expect;
                self.visit_block(&b.block);
            }
            Expr::If(i) => {
                self.visit_expr(&i.cond);
                self.expect = expect.clone();
                self.visit_block(&i.then_branch);
                if let Some((_, other)) = &i.else_branch {
                    self.expect = expect;
                    self.visit_expr(other);
                }
            }
            Expr::Match(m) => {
                self.visit_expr(&m.expr);
                for arm in &m.arms {
                    self.expect = expect.clone();
                    self.visit_expr(&arm.body);
                }
            }
            _ => syn::visit::visit_expr(self, expr),
        }
    }
}

impl Walker<'_, '_> {
    /// `Ok(e)` / `Some(e)` pass the inner type on; a `Paginator`
    /// constructor is a request; anything else is walked for nested calls.
    fn call(&mut self, call: &syn::ExprCall, expect: Option<Expect>) {
        let name = callee_name(call);
        if matches!(name.as_deref(), Some("Ok" | "Some")) && call.args.len() == 1 {
            self.expect = expect.and_then(|e| {
                Some(Expect {
                    ty: first_generic(&e.ty)?,
                    tried: true,
                })
            });
            self.visit_expr(&call.args[0]);
            return;
        }
        if is_paginator(call) {
            self.paginator(call, expect);
        }
        for arg in &call.args {
            self.visit_expr(arg);
        }
    }

    fn chain(&mut self, expr: &Expr, expect: Option<Expect>) {
        let (root, links) = flatten(expr);
        let mut handled = false;
        for (at, link) in links.iter().enumerate() {
            let Link::Call(call) = link else { continue };
            let after = expect_after(&links[at + 1..], expect.clone());
            if self.client_request(call, after) || self.reqwest_request(call, &links[at + 1..]) {
                handled = true;
                break;
            }
        }
        match root {
            Expr::Call(call) if !handled && is_paginator(call) => {
                let after = expect_after(&links, expect);
                self.paginator(call, after);
                for arg in &call.args {
                    self.visit_expr(arg);
                }
            }
            other => self.visit_expr(other),
        }
        for link in &links {
            if let Link::Call(call) = link {
                for arg in &call.args {
                    self.visit_expr(arg);
                }
            }
        }
    }

    fn client_request(&mut self, call: &syn::ExprMethodCall, expect: Option<Expect>) -> bool {
        let name = call.method.to_string();
        let Some(spec) = CLIENT_METHODS.iter().find(|m| m.name == name) else {
            return false;
        };
        let Some(path) = call.args.first() else {
            return false;
        };
        let routes = self.routes.of(self.scope, path);
        if routes.is_empty() {
            return false;
        }
        let arg = |at: Option<usize>| at.and_then(|at| call.args.iter().nth(at));
        let body = match (arg(spec.body), spec.multipart) {
            (Some(_), true) => Part::Media("multipart/form-data".to_owned()),
            (Some(body), false) => self.part(body),
            (None, _) => Part::Absent,
        };
        let query = arg(spec.query).map_or(Part::Absent, |q| self.part(q));
        let (response, accept) = self.client_response(spec.response, call, expect);
        let line = call.method.span().start().line;
        self.push(
            &routes,
            path,
            spec.method,
            line,
            [body, query, response],
            accept,
        );
        true
    }

    fn client_response(
        &self,
        read: Read,
        call: &syn::ExprMethodCall,
        expect: Option<Expect>,
    ) -> (Part, Result<String, String>) {
        let json = Ok("application/json".to_owned());
        match read {
            Read::Json => (self.response(expect, false), json),
            Read::JsonOrNothing => (self.response(expect, true), json),
            Read::Nothing => (Part::NotRead, json),
            Read::Bytes => (Part::Raw, json),
            Read::ByAccept(at) => {
                let accept = call
                    .args
                    .iter()
                    .nth(at)
                    .map_or(Ok(None), |a| self.accept(a));
                match accept {
                    Ok(Some(media)) if media.contains("json") => (self.decoded(), Ok(media)),
                    Ok(Some(media)) => (Part::Raw, Ok(media)),
                    Ok(None) if at == 2 => (self.decoded(), json),
                    Ok(None) => (Part::Raw, json),
                    Err(e) => (
                        Part::Untyped(format!("Accept `{e}` is not a constant")),
                        Err(e),
                    ),
                }
            }
        }
    }

    /// The `Accept` argument: `None`, or `Some(media)` with a literal or a
    /// constant.
    fn accept(&self, arg: &Expr) -> Result<Option<String>, String> {
        let text = || tidy(&arg.to_token_stream().to_string());
        match arg {
            Expr::Path(p) if p.path.is_ident("None") => Ok(None),
            Expr::Call(c) if callee_name(c).as_deref() == Some("Some") => {
                let inner = c.args.first().ok_or_else(text)?;
                let inner_text = || tidy(&inner.to_token_stream().to_string());
                self.constant(inner).map(Some).ok_or_else(inner_text)
            }
            _ => Err(text()),
        }
    }

    fn constant(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            Expr::Path(p) => {
                let name = p.path.segments.last()?.ident.to_string();
                self.routes.index.constant(&name).map(str::to_owned)
            }
            Expr::Reference(r) => self.constant(&r.expr),
            _ => None,
        }
    }

    /// A response read after the call: `decode_json(res)` decodes it into
    /// the function's return type, `res.json::<T>()` into `T`.
    fn decoded(&self) -> Part {
        let body = self.scope.def.block.to_token_stream().to_string();
        if let Some(ty) = turbofish_json(&self.scope.def.block) {
            return self.typed(ty);
        }
        if body.contains("decode_json (") {
            return self.response(return_expect(&self.scope.def.sig.output), false);
        }
        Part::Untyped("the response is read after the call".to_owned())
    }

    fn paginator(&mut self, call: &syn::ExprCall, expect: Option<Expect>) {
        let name = callee_name(call).unwrap_or_default();
        let args: Vec<&Expr> = call.args.iter().collect();
        let Some(path) = args.get(1) else { return };
        let routes = self.routes.of(self.scope, path);
        if routes.is_empty() {
            return;
        }
        let cursor = args
            .get(3)
            .and_then(|c| self.constant(c))
            .unwrap_or_else(|| "next".to_owned());
        let query = args.get(2).map_or(Part::Absent, |q| self.part(q));
        let response = if name == "with_decoder" {
            let decoder = args.get(4).map(|d| d.to_token_stream().to_string());
            let decoder = tidy(&decoder.unwrap_or_default());
            Part::Untyped(format!("pages are decoded by `{decoder}`"))
        } else {
            self.page(expect)
        };
        let line = call
            .func
            .to_token_stream()
            .into_iter()
            .next()
            .map_or(0, |t| t.span().start().line);
        let accept = Ok("application/json".to_owned());
        let before = self.sites.len();
        self.push(
            &routes,
            path,
            "GET",
            line,
            [Part::Absent, query, response],
            accept,
        );
        // The paginator sends the cursor itself.
        for site in &mut self.sites[before..] {
            site.route_query.push((cursor.clone(), false));
        }
    }

    /// A paginator's page: `Paginated<T>` for a `Paginator<T>`.
    fn page(&self, expect: Option<Expect>) -> Part {
        let Some(ty) = expect.and_then(|e| self.ok_type(e)) else {
            return Part::Untyped("the call site does not name the page type".to_owned());
        };
        if type_name(&ty).as_deref() != Some("Paginator") {
            return Part::Untyped(format!(
                "pages of `{}`",
                tidy(&ty.to_token_stream().to_string())
            ));
        }
        match first_generic(&ty) {
            Some(item) => {
                let page: Type = syn::parse_quote!(Paginated<#item>);
                self.typed(page)
            }
            None => Part::Untyped("Paginator without an item type".to_owned()),
        }
    }

    fn reqwest_request(&mut self, call: &syn::ExprMethodCall, rest: &[Link]) -> bool {
        let name = call.method.to_string();
        let Some((_, method)) = REQWEST_VERBS.iter().find(|(verb, _)| *verb == name) else {
            return false;
        };
        let Some(url) = call.args.first().filter(|_| call.args.len() == 1) else {
            return false;
        };
        let routes = self.routes.of(self.scope, url);
        if routes.is_empty() {
            return false;
        }
        let (mut body, mut query, mut content_type) = (Part::Absent, Part::Absent, None);
        for link in rest {
            let Link::Call(next) = link else { continue };
            let arg = next.args.iter().last();
            match (next.method.to_string().as_str(), arg) {
                ("json", Some(b)) => body = self.part(b),
                ("form", Some(_)) => body = Part::Media("application/x-www-form-urlencoded".into()),
                ("multipart", Some(_)) => body = Part::Media("multipart/form-data".into()),
                ("body", Some(_)) => body = Part::Untyped("a raw body".into()),
                ("query", Some(q)) => query = self.part(q),
                ("header", Some(value)) => content_type = content_type.or(self.constant(value)),
                _ => {}
            }
        }
        if let (Part::Untyped(_), Some(media)) = (&body, content_type) {
            body = Part::Media(media);
        }
        let response = match turbofish_json(&self.scope.def.block) {
            Some(ty) => self.typed(ty),
            None => Part::NotRead,
        };
        let line = call.method.span().start().line;
        let accept = Ok("application/json".to_owned());
        self.push(&routes, url, method, line, [body, query, response], accept);
        true
    }

    fn push(
        &mut self,
        routes: &[String],
        path: &Expr,
        method: &'static str,
        line: usize,
        [body, query, response]: [Part; 3],
        accept: Result<String, String>,
    ) {
        let appended = self.appended_query(path);
        let file = &self.routes.index.files[self.scope.def.file].path;
        for route in dedup(routes) {
            let mut route_query: Vec<(String, bool)> =
                query_keys(&route).into_iter().map(|k| (k, true)).collect();
            route_query.extend(appended.iter().map(|k| (k.clone(), false)));
            self.sites.push(CallSite {
                method,
                route,
                location: format!("{file}:{line}"),
                route_query,
                body: body.clone(),
                query: query.clone(),
                response: response.clone(),
                accept: accept.clone(),
            });
        }
    }

    /// Query parameters appended to a route variable after it was built:
    /// `path.push_str("?project=")`.
    fn appended_query(&self, path: &Expr) -> Vec<String> {
        let name = match path {
            Expr::Reference(r) => r.expr.to_token_stream().to_string(),
            other => other.to_token_stream().to_string(),
        };
        let mut pushes = PushStr {
            var: name,
            keys: Vec::new(),
        };
        pushes.visit_block(&self.scope.def.block);
        pushes.keys
    }

    /// The type of a body or query argument.
    fn part(&self, expr: &Expr) -> Part {
        match self.type_of(expr, 0) {
            Typed::Ty(ty, scope) => Part::Typed(Box::new(ty), scope),
            Typed::Unit => Part::Absent,
            Typed::Untyped(why) => Part::Untyped(why),
        }
    }

    fn typed(&self, ty: Type) -> Part {
        match self.generic_param(&ty) {
            Some(name) => Part::Untyped(format!("generic `{name}`")),
            None => Part::Typed(Box::new(ty), self.here()),
        }
    }

    fn here(&self) -> Scope {
        Scope {
            file: self.scope.def.file,
            func: Some(self.scope.id),
        }
    }

    fn response(&self, expect: Option<Expect>, optional: bool) -> Part {
        let Some(ty) = expect.and_then(|e| self.ok_type(e)) else {
            return Part::Untyped("the call site does not name the response type".to_owned());
        };
        let ty = if optional {
            unwrap_named(&ty, &["Option"]).unwrap_or(ty)
        } else {
            ty
        };
        self.typed(ty)
    }

    /// The `Ok` type of a request whose value must end up as `expect`.
    fn ok_type(&self, expect: Expect) -> Option<Type> {
        if expect.tried {
            Some(expect.ty)
        } else {
            unwrap_named(&expect.ty, &["Result", "ApiResult"])
        }
    }

    fn generic_param(&self, ty: &Type) -> Option<String> {
        let name = match ty {
            Type::Path(p) if p.path.segments.len() == 1 => p.path.segments[0].ident.to_string(),
            _ => return None,
        };
        self.scope
            .def
            .sig
            .generics
            .type_params()
            .any(|p| p.ident == name)
            .then_some(name)
    }
}

/// What the type of an expression resolves to. Short-lived, so the size of
/// its `Type` does not matter.
#[allow(clippy::large_enum_variant)]
enum Typed {
    Ty(Type, Scope),
    /// `()`: nothing sent.
    Unit,
    Untyped(String),
}

const MAX_DEPTH: u8 = 12;

impl Walker<'_, '_> {
    fn type_of(&self, expr: &Expr, depth: u8) -> Typed {
        if depth > MAX_DEPTH {
            return Typed::Untyped("expression too deep to follow".to_owned());
        }
        let depth = depth + 1;
        match expr {
            Expr::Reference(r) => self.type_of(&r.expr, depth),
            Expr::Paren(p) => self.type_of(&p.expr, depth),
            Expr::Group(g) => self.type_of(&g.expr, depth),
            Expr::Await(a) => self.type_of(&a.base, depth),
            Expr::Try(t) => match self.type_of(&t.expr, depth) {
                Typed::Ty(ty, scope) => unwrap_named(&ty, &["Result", "ApiResult", "Option"])
                    .map_or(Typed::Ty(ty, scope), |ok| Typed::Ty(ok, scope)),
                other => other,
            },
            Expr::Tuple(t) if t.elems.is_empty() => Typed::Unit,
            Expr::Struct(s) => self.struct_type(&s.path),
            Expr::Path(p) => self.path_type(&p.path, depth),
            Expr::Call(c) => self.call_type(c),
            Expr::MethodCall(m) => self.method_type(m, depth),
            Expr::Macro(m) if is_macro(&m.mac, "json") => {
                Typed::Untyped("serde_json::json! builds an untyped value".to_owned())
            }
            other => Typed::Untyped(format!("`{}` has no declared type", short(other))),
        }
    }

    fn struct_type(&self, path: &syn::Path) -> Typed {
        let ty = if path.is_ident("Self") {
            self.self_type()
        } else {
            Some(Type::Path(syn::TypePath {
                qself: None,
                path: path.clone(),
            }))
        };
        ty.map_or_else(
            || Typed::Untyped("Self outside an impl".to_owned()),
            |ty| Typed::Ty(ty, self.here()),
        )
    }

    fn self_type(&self) -> Option<Type> {
        syn::parse_str(self.scope.impl_type()?).ok()
    }

    fn path_type(&self, path: &syn::Path, depth: u8) -> Typed {
        let Some(name) = path.get_ident().map(ToString::to_string) else {
            let path = tidy(&path.to_token_stream().to_string());
            return Typed::Untyped(format!("`{path}` has no declared type"));
        };
        // `let params = params.clone()` shadows a parameter with itself, so
        // a name met again while resolving its own binding is the parameter.
        let shadowing = self.resolving.borrow().contains(&name);
        let binding = self.scope.lets.get(&name).and_then(|b| b.last());
        if let Some(binding) = binding.filter(|_| !shadowing) {
            return match (&binding.ty, &binding.init) {
                (Some(ty), _) => Typed::Ty(ty.clone(), self.here()),
                (None, Some(init)) => {
                    self.resolving.borrow_mut().push(name);
                    let typed = self.type_of(init, depth);
                    self.resolving.borrow_mut().pop();
                    typed
                }
                (None, None) => Typed::Untyped(format!("`{name}` has no declared type")),
            };
        }
        match self.scope.params.get(&name) {
            Some(ty) if self.generic_param(strip_ref(ty)).is_some() => Typed::Untyped(format!(
                "generic `{}`",
                tidy(&ty.to_token_stream().to_string())
            )),
            Some(ty) => Typed::Ty(strip_ref(ty).clone(), self.here()),
            None => Typed::Untyped(format!("`{name}` has no declared type")),
        }
    }

    fn call_type(&self, call: &syn::ExprCall) -> Typed {
        let Expr::Path(func) = &*call.func else {
            return Typed::Untyped("a computed function".to_owned());
        };
        let segments: Vec<String> = func
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let name = segments.last().cloned().unwrap_or_default();
        if name == "to_value" {
            return Typed::Untyped("serde_json::to_value builds an untyped value".to_owned());
        }
        let owner = match segments.as_slice() {
            [owner, _] if owner == "Self" => self.scope.impl_type().map(str::to_owned),
            [.., owner, _] => Some(owner.clone()),
            _ => None,
        };
        self.returned(&name, owner.as_deref())
    }

    fn method_type(&self, call: &syn::ExprMethodCall, depth: u8) -> Typed {
        let name = call.method.to_string();
        if matches!(
            name.as_str(),
            "clone" | "to_owned" | "as_ref" | "unwrap" | "expect" | "map_err"
        ) {
            return self.type_of(&call.receiver, depth);
        }
        let owner = match &*call.receiver {
            Expr::Path(p) if p.path.is_ident("self") => self.scope.impl_type().map(str::to_owned),
            receiver => match self.type_of(receiver, depth) {
                Typed::Ty(ty, _) => type_name(&ty),
                _ => None,
            },
        };
        self.returned(&name, owner.as_deref())
    }

    /// What the function `name` (a method of `owner` when given) returns.
    fn returned(&self, name: &str, owner: Option<&str>) -> Typed {
        let index = self.routes.index;
        let Some(id) = index.function(name, owner, self.scope.def.file) else {
            return Typed::Untyped(format!("`{name}` is not a function of this crate"));
        };
        let def = &index.fns[id];
        let syn::ReturnType::Type(_, ty) = &def.sig.output else {
            return Typed::Unit;
        };
        let text = ty.to_token_stream().to_string();
        let ty: Type = match (&def.impl_type, text.contains("Self")) {
            (Some(owner), true) => {
                let replaced: Vec<&str> = text
                    .split_whitespace()
                    .map(|t| if t == "Self" { owner.as_str() } else { t })
                    .collect();
                syn::parse_str(&replaced.join(" ")).unwrap_or_else(|_| (**ty).clone())
            }
            _ => (**ty).clone(),
        };
        Typed::Ty(
            ty,
            Scope {
                file: def.file,
                func: Some(id),
            },
        )
    }
}

/// Source text as written, rather than with a space between every token.
pub fn tidy(text: &str) -> String {
    let mut out = text.to_owned();
    for (spaced, tight) in [
        (" . ", "."),
        (" :: ", "::"),
        (" (", "("),
        ("( ", "("),
        (" )", ")"),
        ("& ", "&"),
        (" < ", "<"),
        (" >", ">"),
        (" !", "!"),
        (" ,", ","),
    ] {
        out = out.replace(spaced, tight);
    }
    out
}

fn short(expr: &Expr) -> String {
    let text = tidy(&expr.to_token_stream().to_string());
    match text.char_indices().nth(60) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

fn strip_ref(ty: &Type) -> &Type {
    match ty {
        Type::Reference(r) => strip_ref(&r.elem),
        other => other,
    }
}

fn callee_name(call: &syn::ExprCall) -> Option<String> {
    match &*call.func {
        Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
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

/// `a.b().await?.c()` → (`a`, [`.b()`, await, `?`, `.c()`]).
fn flatten(expr: &Expr) -> (&Expr, Vec<Link<'_>>) {
    let mut links = Vec::new();
    let mut at = expr;
    loop {
        match at {
            Expr::MethodCall(m) => {
                links.push(Link::Call(m));
                at = &m.receiver;
            }
            Expr::Await(a) => {
                links.push(Link::Await);
                at = &a.base;
            }
            Expr::Try(t) => {
                links.push(Link::Try);
                at = &t.expr;
            }
            _ => break,
        }
    }
    links.reverse();
    (at, links)
}

/// The expectation on a request whose value goes through `rest` first: a `?`
/// or `.expect` unwraps its `Result`; any other method transforms the value
/// out of recognition.
fn expect_after(rest: &[Link], expect: Option<Expect>) -> Option<Expect> {
    let mut expect = expect?;
    for link in rest {
        match link {
            Link::Await => {}
            Link::Try => expect.tried = true,
            Link::Call(c) if matches!(c.method.to_string().as_str(), "expect" | "unwrap") => {
                expect.tried = true;
            }
            Link::Call(_) => return None,
        }
    }
    Some(expect)
}

fn first_generic(ty: &Type) -> Option<Type> {
    let Type::Path(p) = ty else { return None };
    let PathArguments::AngleBracketed(args) = &p.path.segments.last()?.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

/// `Result<T, E>` → `T` when the outer type is one of `names`.
fn unwrap_named(ty: &Type, names: &[&str]) -> Option<Type> {
    let name = type_name(ty)?;
    if names.contains(&name.as_str()) {
        first_generic(ty)
    } else {
        None
    }
}

/// `res.json::<T>()` anywhere in the body.
fn turbofish_json(block: &syn::Block) -> Option<Type> {
    struct Find(Option<Type>);
    impl<'ast> Visit<'ast> for Find {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if call.method == "json" {
                if let Some(turbofish) = &call.turbofish {
                    if let Some(GenericArgument::Type(t)) = turbofish.args.first() {
                        self.0 = Some(t.clone());
                    }
                }
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }
    let mut find = Find(None);
    find.visit_block(block);
    find.0
}

struct PushStr {
    var: String,
    keys: Vec<String>,
}

impl<'ast> Visit<'ast> for PushStr {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let receiver = call.receiver.to_token_stream().to_string();
        if call.method == "push_str" && receiver == self.var {
            if let Some(Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            })) = call.args.first()
            {
                self.keys.extend(query_keys(&s.value()));
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

/// The parameter names in a route's `?a={}&b={}`, or in a fragment of one.
fn query_keys(route: &str) -> Vec<String> {
    let Some(at) = route.find('?') else {
        return Vec::new();
    };
    route[at + 1..]
        .split('&')
        .filter_map(|pair| pair.split('=').next())
        .filter(|k| !k.is_empty())
        .map(str::to_owned)
        .collect()
}

fn dedup(routes: &[String]) -> Vec<String> {
    let mut out = routes.to_vec();
    out.sort();
    out.dedup();
    out
}
