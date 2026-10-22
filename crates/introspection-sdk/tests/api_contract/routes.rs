//! What `/v1/...` route an expression in the SDK source evaluates to.
//!
//! Routes are built in a handful of ways: a literal, a `format!` with path
//! arguments, a constant spliced into a `format!`, a `let path = ...` bound
//! earlier, a helper function that returns one (`run_path(...)`), or a struct
//! field a constructor stored one in (`self.path`). Each is followed here.
//! Every path argument becomes `{}`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, FnArg, Lit, Pat, Token};

use super::index::{FnDef, Index};

const MAX_DEPTH: u8 = 16;

/// A function's parameters and `let` bindings, by name.
pub struct FnScope<'a> {
    pub id: usize,
    pub def: &'a FnDef,
    pub params: HashSet<String>,
    /// Each `let`'s initializer, in order.
    pub lets: HashMap<String, Vec<Expr>>,
}

impl<'a> FnScope<'a> {
    pub fn new(index: &'a Index, id: usize) -> Self {
        let def = &index.fns[id];
        let params = def
            .sig
            .inputs
            .iter()
            .filter_map(|input| match input {
                FnArg::Typed(t) => pat_name(&t.pat),
                FnArg::Receiver(_) => None,
            })
            .collect();
        let mut lets = LetCollector::default();
        lets.visit_block(&def.block);
        Self {
            id,
            def,
            params,
            lets: lets.lets,
        }
    }

    pub fn impl_type(&self) -> Option<&str> {
        self.def.impl_type.as_deref()
    }
}

#[derive(Default)]
struct LetCollector {
    lets: HashMap<String, Vec<Expr>>,
}

impl<'ast> Visit<'ast> for LetCollector {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (Some(name), Some(init)) = (pat_name(&local.pat), &local.init) {
            self.lets
                .entry(name)
                .or_default()
                .push((*init.expr).clone());
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(stmts) = macro_stmts(mac) {
            for stmt in &stmts {
                self.visit_stmt(stmt);
            }
        }
    }
}

/// Whether `mac` is `name!`, however it is qualified.
fn is_macro(mac: &syn::Macro, name: &str) -> bool {
    mac.path.segments.last().is_some_and(|s| s.ident == name)
}

/// The statements of a macro whose body is code (`stream! { ... }`), so the
/// requests inside it are read too. `format!` and friends are not code.
pub fn macro_stmts(mac: &syn::Macro) -> Option<Vec<syn::Stmt>> {
    const NOT_CODE: [&str; 9] = [
        "format", "json", "vec", "write", "writeln", "println", "eprintln", "panic", "assert",
    ];
    if NOT_CODE.iter().any(|name| is_macro(mac, name)) {
        return None;
    }
    mac.parse_body_with(syn::Block::parse_within).ok()
}

fn pat_name(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(i) => Some(i.ident.to_string()),
        Pat::Type(t) => pat_name(&t.pat),
        _ => None,
    }
}

/// Evaluates expressions to routes.
pub struct Routes<'a> {
    pub index: &'a Index,
    /// Routes stored in struct fields, by (type, field).
    fields: HashMap<(String, String), Vec<String>>,
    /// Routes each helper function can return, computed on first use.
    produced: RefCell<HashMap<usize, Vec<String>>>,
    busy: RefCell<HashSet<usize>>,
}

impl<'a> Routes<'a> {
    pub fn new(index: &'a Index) -> Self {
        let mut routes = Self {
            index,
            fields: HashMap::new(),
            produced: RefCell::default(),
            busy: RefCell::default(),
        };
        routes.fields = FieldRoutes::collect(&routes);
        routes
    }

    /// Every route `expr` can evaluate to, unnormalized (`?query` kept).
    pub fn of(&self, scope: &FnScope, expr: &Expr) -> Vec<String> {
        self.eval(scope, expr, 0)
    }

    fn eval(&self, scope: &FnScope, expr: &Expr, depth: u8) -> Vec<String> {
        if depth > MAX_DEPTH {
            return Vec::new();
        }
        let depth = depth + 1;
        match expr {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(s) => route_in(&s.value()).into_iter().collect(),
                _ => Vec::new(),
            },
            Expr::Macro(m) if is_macro(&m.mac, "format") => self.format(scope, &m.mac, depth),
            Expr::Path(p) => self.path(scope, &p.path, depth),
            Expr::Reference(r) => self.eval(scope, &r.expr, depth),
            Expr::Paren(p) => self.eval(scope, &p.expr, depth),
            Expr::Group(g) => self.eval(scope, &g.expr, depth),
            Expr::Try(t) => self.eval(scope, &t.expr, depth),
            Expr::Field(f) => self.field(scope, &f.member),
            Expr::MethodCall(m) => self.method_call(scope, m, depth),
            Expr::Call(c) => self.call(scope, c, depth),
            Expr::If(_) | Expr::Match(_) | Expr::Block(_) => {
                let mut tails = Vec::new();
                collect_tails(expr, &mut tails);
                tails
                    .into_iter()
                    .flat_map(|t| self.eval(scope, t, depth))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn path(&self, scope: &FnScope, path: &syn::Path, depth: u8) -> Vec<String> {
        let Some(last) = path.segments.last() else {
            return Vec::new();
        };
        let name = last.ident.to_string();
        if path.segments.len() == 1 {
            if let Some(bindings) = scope.lets.get(&name) {
                return bindings
                    .iter()
                    .flat_map(|init| self.eval(scope, init, depth))
                    .collect();
            }
            if scope.params.contains(&name) {
                return Vec::new();
            }
        }
        self.index
            .constant(&name)
            .and_then(route_in)
            .into_iter()
            .collect()
    }

    fn field(&self, scope: &FnScope, member: &syn::Member) -> Vec<String> {
        let syn::Member::Named(name) = member else {
            return Vec::new();
        };
        let Some(owner) = scope.impl_type() else {
            return Vec::new();
        };
        self.fields
            .get(&(owner.to_owned(), name.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn method_call(&self, scope: &FnScope, call: &syn::ExprMethodCall, depth: u8) -> Vec<String> {
        const PASS_THROUGH: [&str; 9] = [
            "as_str",
            "to_string",
            "clone",
            "to_owned",
            "into",
            "as_ref",
            "as_deref",
            "unwrap",
            "expect",
        ];
        let name = call.method.to_string();
        if PASS_THROUGH.contains(&name.as_str()) {
            return self.eval(scope, &call.receiver, depth);
        }
        let owner = match &*call.receiver {
            Expr::Path(p) if p.path.is_ident("self") => scope.impl_type(),
            _ => None,
        };
        self.helper(&name, owner, scope.def.file)
    }

    fn call(&self, scope: &FnScope, call: &syn::ExprCall, depth: u8) -> Vec<String> {
        let Expr::Path(func) = &*call.func else {
            return Vec::new();
        };
        let segments: Vec<String> = func
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let name = segments.last().map(String::as_str).unwrap_or_default();
        if matches!(name, "Ok" | "Some" | "from") {
            return call
                .args
                .iter()
                .flat_map(|a| self.eval(scope, a, depth))
                .collect();
        }
        let owner = match segments.as_slice() {
            [owner, _] if owner == "Self" => scope.impl_type().map(str::to_owned),
            [.., owner, _] => Some(owner.clone()),
            _ => None,
        };
        self.helper(name, owner.as_deref(), scope.def.file)
    }

    /// The routes a helper such as `run_path(...)` returns: every route its
    /// body builds, when it returns a `String`.
    fn helper(&self, name: &str, owner: Option<&str>, file: usize) -> Vec<String> {
        let Some(id) = self.index.function(name, owner, file) else {
            return Vec::new();
        };
        if let Some(done) = self.produced.borrow().get(&id) {
            return done.clone();
        }
        let def = &self.index.fns[id];
        let returns_string = def
            .sig
            .output
            .to_token_stream()
            .to_string()
            .contains("String");
        if !returns_string || !self.busy.borrow_mut().insert(id) {
            return Vec::new();
        }
        let scope = FnScope::new(self.index, id);
        let mut built = RouteExprs::default();
        built.visit_block(&def.block);
        let mut routes: Vec<String> = built
            .exprs
            .iter()
            .flat_map(|e| self.of(&scope, e))
            .collect();
        for (var, segment) in built.pushed {
            let Ok(var) = syn::parse_str::<Expr>(&var) else {
                continue;
            };
            for route in self.of(&scope, &var) {
                routes.push(format!("{route}{segment}"));
            }
        }
        routes.sort();
        routes.dedup();
        self.busy.borrow_mut().remove(&id);
        self.produced.borrow_mut().insert(id, routes.clone());
        routes
    }

    /// Expands a `format!`: each `{...}` becomes the route its argument
    /// evaluates to, or `{}`.
    fn format(&self, scope: &FnScope, mac: &syn::Macro, depth: u8) -> Vec<String> {
        let Ok(args) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) else {
            return Vec::new();
        };
        let mut args = args.into_iter();
        let Some(Expr::Lit(syn::ExprLit {
            lit: Lit::Str(template),
            ..
        })) = args.next()
        else {
            return Vec::new();
        };
        let args: Vec<Expr> = args.collect();
        let mut out = vec![String::new()];
        let mut positional = 0;
        for piece in pieces(&template.value()) {
            let options = match piece {
                Piece::Text(text) => vec![text],
                Piece::Arg(spec) => {
                    let expr = arg_for(&spec, &args, &mut positional);
                    let routes = expr
                        .map(|e| self.eval(scope, &e, depth))
                        .unwrap_or_default();
                    if routes.is_empty() {
                        vec!["{}".to_owned()]
                    } else {
                        routes
                    }
                }
            };
            out = out
                .iter()
                .flat_map(|prefix| options.iter().map(move |o| format!("{prefix}{o}")))
                .collect();
        }
        out.iter().filter_map(|s| route_in(s)).collect()
    }
}

/// The `/v1/...` part of a string, if it has one.
fn route_in(text: &str) -> Option<String> {
    let at = text.find("/v1/")?;
    Some(text[at..].to_owned())
}

enum Piece {
    Text(String),
    /// What is between the braces, before any `:` format spec.
    Arg(String),
}

fn pieces(template: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('{', Some('{')) | ('}', Some('}')) => {
                text.push(c);
                chars.next();
            }
            ('{', _) => {
                out.push(Piece::Text(std::mem::take(&mut text)));
                let inner: String = chars.by_ref().take_while(|c| *c != '}').collect();
                let name = inner
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                out.push(Piece::Arg(name));
            }
            _ => text.push(c),
        }
    }
    out.push(Piece::Text(text));
    out
}

/// The expression a `{...}` refers to: the next positional argument, an
/// indexed one, a `name = value` one, or the variable it names inline.
fn arg_for(spec: &str, args: &[Expr], positional: &mut usize) -> Option<Expr> {
    if spec.is_empty() {
        let arg = args.get(*positional).cloned();
        *positional += 1;
        return arg;
    }
    if let Ok(at) = spec.parse::<usize>() {
        return args.get(at).cloned();
    }
    let named = args.iter().find_map(|a| match a {
        Expr::Assign(assign) if assign.left.to_token_stream().to_string() == spec => {
            Some((*assign.right).clone())
        }
        _ => None,
    });
    named.or_else(|| syn::parse_str::<Expr>(spec).ok())
}

/// The expressions a block, `if` or `match` can end with.
fn collect_tails<'e>(expr: &'e Expr, out: &mut Vec<&'e Expr>) {
    match expr {
        Expr::If(i) => {
            block_tail(&i.then_branch, out);
            if let Some((_, other)) = &i.else_branch {
                collect_tails(other, out);
            }
        }
        Expr::Match(m) => m.arms.iter().for_each(|arm| collect_tails(&arm.body, out)),
        Expr::Block(b) => block_tail(&b.block, out),
        other => out.push(other),
    }
}

fn block_tail<'e>(block: &'e syn::Block, out: &mut Vec<&'e Expr>) {
    if let Some(syn::Stmt::Expr(tail, None)) = block.stmts.last() {
        collect_tails(tail, out);
    }
}

/// The literals and `format!`s in a body, the expressions a route is built
/// from.
#[derive(Default)]
struct RouteExprs {
    exprs: Vec<Expr>,
    /// Path segments pushed onto a route variable (`out.push('/');
    /// out.push_str(&encode(segment))`), as (variable, `/{}`).
    pushed: Vec<(String, String)>,
}

impl<'ast> Visit<'ast> for RouteExprs {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let slash = match call.args.first() {
            Some(Expr::Lit(lit)) => match &lit.lit {
                Lit::Char(c) => c.value() == '/',
                Lit::Str(s) => s.value() == "/",
                _ => false,
            },
            _ => false,
        };
        if slash && (call.method == "push" || call.method == "push_str") {
            let var = call.receiver.to_token_stream().to_string();
            self.pushed.push((var, "/{}".to_owned()));
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Lit(_) => self.exprs.push(expr.clone()),
            Expr::Macro(m) if is_macro(&m.mac, "format") => self.exprs.push(expr.clone()),
            _ => syn::visit::visit_expr(self, expr),
        }
    }
}

/// Routes stored in struct fields: a struct literal that sets a field to a
/// route (`Self { path: format!(...) }`), or to a constructor parameter that a
/// caller passes a route to (`Paginator::new(http, path, ...)`).
struct FieldRoutes<'r, 'a> {
    routes: &'r Routes<'a>,
    scope: Option<FnScope<'a>>,
    found: HashMap<(String, String), Vec<String>>,
    /// (constructor, parameter) → the field it is stored in.
    stored: HashMap<(usize, String), (String, String)>,
    pass: u8,
}

impl<'r, 'a> FieldRoutes<'r, 'a> {
    fn collect(routes: &'r Routes<'a>) -> HashMap<(String, String), Vec<String>> {
        let mut this = Self {
            routes,
            scope: None,
            found: HashMap::new(),
            stored: HashMap::new(),
            pass: 0,
        };
        for pass in 0..2 {
            this.pass = pass;
            for id in 0..routes.index.fns.len() {
                this.scope = Some(FnScope::new(routes.index, id));
                this.visit_block(&routes.index.fns[id].block);
            }
        }
        this.found
    }

    fn struct_literal(&mut self, lit: &syn::ExprStruct) {
        let Some(scope) = &self.scope else { return };
        let Some(ty) = lit.path.segments.last().map(|s| s.ident.to_string()) else {
            return;
        };
        let ty = match ty.as_str() {
            "Self" => scope.impl_type().unwrap_or_default().to_owned(),
            _ => ty,
        };
        for field in &lit.fields {
            let syn::Member::Named(name) = &field.member else {
                continue;
            };
            let key = (ty.clone(), name.to_string());
            let routes = self.routes.of(scope, &field.expr);
            if !routes.is_empty() {
                self.found.entry(key).or_default().extend(routes);
            } else if let Expr::Path(p) = &field.expr {
                let param = p.path.get_ident().map(ToString::to_string);
                if let Some(param) = param.filter(|p| scope.params.contains(p)) {
                    self.stored.insert((scope.id, param), key);
                }
            }
        }
    }

    fn constructor_call(&mut self, call: &syn::ExprCall) {
        let Some(scope) = &self.scope else { return };
        let Expr::Path(func) = &*call.func else {
            return;
        };
        let segments: Vec<String> = func
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let [.., owner, name] = segments.as_slice() else {
            return;
        };
        let owner = if owner == "Self" {
            scope.impl_type().unwrap_or_default()
        } else {
            owner.as_str()
        };
        let index = self.routes.index;
        let Some(id) = index.function(name, Some(owner), scope.def.file) else {
            return;
        };
        let params = index.fns[id].sig.inputs.iter().filter_map(|i| match i {
            FnArg::Typed(t) => pat_name(&t.pat),
            FnArg::Receiver(_) => None,
        });
        for (param, arg) in params.zip(call.args.iter()) {
            if let Some(key) = self.stored.get(&(id, param)).cloned() {
                let routes = self.routes.of(scope, arg);
                self.found.entry(key).or_default().extend(routes);
            }
        }
    }
}

impl<'ast> Visit<'ast> for FieldRoutes<'_, '_> {
    fn visit_expr_struct(&mut self, lit: &'ast syn::ExprStruct) {
        if self.pass == 0 {
            self.struct_literal(lit);
        }
        syn::visit::visit_expr_struct(self, lit);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.pass == 1 {
            self.constructor_call(call);
        }
        syn::visit::visit_expr_call(self, call);
    }
}
