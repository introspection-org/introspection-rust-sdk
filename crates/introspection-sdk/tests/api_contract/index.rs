//! Everything the SDK source declares that the call-site reader needs to look
//! up by name: types, functions, string constants and hand-written serde
//! impls. Built once from `crates/introspection-sdk/src`, skipping
//! `#[cfg(test)]` code.

use std::collections::HashMap;
use std::path::Path;

use quote::ToTokens;
use syn::visit::Visit;
use syn::{Attribute, Expr, ImplItem, Item, Lit, Signature, Type};

pub struct SourceFile {
    /// Relative to the crate root, as findings print it: `src/api/tasks.rs`.
    pub path: String,
    /// `api::tasks`, matched against `use` paths to pick between two types of
    /// the same name.
    pub module: String,
    uses: Vec<String>,
}

/// A struct, enum or type alias.
#[derive(Clone)]
pub enum TypeItem {
    Struct(syn::ItemStruct),
    Enum(syn::ItemEnum),
    Alias(syn::ItemType),
}

#[derive(Clone)]
pub struct TypeDef {
    pub file: usize,
    /// Declared inside this function's body (`struct Q {}`), so only visible
    /// from there.
    pub local_to: Option<usize>,
    pub item: TypeItem,
}

pub struct FnDef {
    pub file: usize,
    /// The `impl` block's self type, for methods.
    pub impl_type: Option<String>,
    pub sig: Signature,
    pub block: syn::Block,
}

/// Where a type name is looked up from: a file, and the function whose local
/// items are visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scope {
    pub file: usize,
    pub func: Option<usize>,
}

#[derive(Default)]
pub struct Index {
    pub files: Vec<SourceFile>,
    pub fns: Vec<FnDef>,
    types: HashMap<String, Vec<TypeDef>>,
    fns_by_name: HashMap<String, Vec<usize>>,
    consts: HashMap<String, String>,
    /// Hand-written `Serialize` / `Deserialize` impls, by type name: whether
    /// each writes or reads a plain string, the only shape the check infers
    /// from a hand-written impl.
    serialize_str: HashMap<String, bool>,
    deserialize_str: HashMap<String, bool>,
    /// `macro_rules!` bodies by name, and the types macro invocations
    /// declare, as (type, macro).
    macros: HashMap<String, String>,
    macro_types: Vec<(String, String)>,
}

impl Index {
    pub fn read(src: &Path) -> Self {
        let mut index = Self::default();
        let mut paths = Vec::new();
        collect_rs(src, &mut paths);
        paths.sort();
        let crate_root = src.parent().unwrap_or(src);
        for path in paths {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
            let ast =
                syn::parse_file(&text).unwrap_or_else(|e| panic!("{} parses: {e}", path.display()));
            let rel = path.strip_prefix(crate_root).unwrap_or(&path);
            index.add_file(&rel.to_string_lossy(), &ast);
        }
        index.resolve_macro_types();
        index
    }

    /// Indexes one file's text, as if it were `src/<name>`; tests use it to
    /// check the reader on a small source.
    pub fn from_source(name: &str, text: &str) -> Self {
        let mut index = Self::default();
        let ast = syn::parse_file(text).expect("source parses");
        index.add_file(&format!("src/{name}"), &ast);
        index.resolve_macro_types();
        index
    }

    fn add_file(&mut self, path: &str, ast: &syn::File) {
        let module = path
            .trim_start_matches("src/")
            .trim_end_matches(".rs")
            .trim_end_matches("/mod")
            .replace('/', "::");
        let file = self.files.len();
        self.files.push(SourceFile {
            path: path.to_owned(),
            module,
            uses: Vec::new(),
        });
        self.add_items(file, &ast.items);
    }

    fn add_items(&mut self, file: usize, items: &[Item]) {
        for item in items {
            if is_test_only(item_attrs(item)) {
                continue;
            }
            self.add_item(file, item);
        }
    }

    fn add_item(&mut self, file: usize, item: &Item) {
        match item {
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    self.add_items(file, items);
                }
            }
            Item::Use(u) => self.files[file]
                .uses
                .push(u.tree.to_token_stream().to_string().replace(' ', "")),
            Item::Impl(imp) => self.add_impl(file, imp),
            Item::Fn(f) => self.add_fn(file, None, &f.sig, &f.block),
            Item::Const(c) => self.add_const(&c.ident, &c.expr),
            Item::Static(s) => self.add_const(&s.ident, &s.expr),
            Item::Struct(s) => self.add_type(&s.ident, file, None, TypeItem::Struct(s.clone())),
            Item::Enum(e) => self.add_type(&e.ident, file, None, TypeItem::Enum(e.clone())),
            Item::Type(t) => self.add_type(&t.ident, file, None, TypeItem::Alias(t.clone())),
            Item::Macro(m) => self.add_macro(m),
            _ => {}
        }
    }

    /// A `macro_rules!` definition, or an invocation that declares types
    /// (`open_wire_enum! { pub enum AutomationKind { ... } }`).
    fn add_macro(&mut self, item: &syn::ItemMacro) {
        let body = item.mac.tokens.to_string();
        if let Some(name) = &item.ident {
            self.macros.insert(name.to_string(), body);
            return;
        }
        let Some(name) = item.mac.path.segments.last().map(|s| s.ident.to_string()) else {
            return;
        };
        let tokens: Vec<String> = body.split_whitespace().map(str::to_owned).collect();
        let declared = tokens
            .windows(2)
            .filter(|w| w[0] == "enum" || w[0] == "struct")
            .map(|w| w[1].clone());
        self.macro_types
            .extend(declared.map(|ty| (ty, name.clone())));
    }

    /// A type a macro declares has the serde impls the macro writes for it.
    fn resolve_macro_types(&mut self) {
        for (ty, name) in std::mem::take(&mut self.macro_types) {
            let Some(body) = self.macros.get(&name) else {
                continue;
            };
            if body.contains("fn serialize") {
                self.serialize_str
                    .insert(ty.clone(), body.contains("serialize_str"));
            }
            if body.contains("Deserialize") && body.contains("fn deserialize") {
                let reads_string = body.contains("String :: deserialize");
                self.deserialize_str.insert(ty, reads_string);
            }
        }
    }

    fn add_impl(&mut self, file: usize, imp: &syn::ItemImpl) {
        let self_type = type_name(&imp.self_ty);
        if let (Some(name), Some((_, trait_path, _))) = (&self_type, &imp.trait_) {
            let body = imp.to_token_stream().to_string();
            match trait_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .as_deref()
            {
                Some("Serialize") => {
                    self.serialize_str
                        .insert(name.clone(), body.contains("serialize_str"));
                }
                Some("Deserialize") => {
                    let reads_string = body.contains("String :: deserialize")
                        || body.contains("< String > :: deserialize");
                    self.deserialize_str.insert(name.clone(), reads_string);
                }
                _ => {}
            }
        }
        for item in &imp.items {
            match item {
                ImplItem::Fn(f) if !is_test_only(&f.attrs) => {
                    self.add_fn(file, self_type.clone(), &f.sig, &f.block);
                }
                ImplItem::Const(c) => self.add_const(&c.ident, &c.expr),
                _ => {}
            }
        }
    }

    fn add_fn(
        &mut self,
        file: usize,
        impl_type: Option<String>,
        sig: &Signature,
        block: &syn::Block,
    ) {
        let id = self.fns.len();
        let mut locals = LocalItems::default();
        locals.visit_block(block);
        for item in locals.items {
            let local = match item {
                Item::Struct(s) => Some((s.ident.clone(), TypeItem::Struct(s))),
                Item::Enum(e) => Some((e.ident.clone(), TypeItem::Enum(e))),
                _ => None,
            };
            if let Some((ident, item)) = local {
                self.add_type(&ident, file, Some(id), item);
            }
        }
        self.fns_by_name
            .entry(sig.ident.to_string())
            .or_default()
            .push(id);
        self.fns.push(FnDef {
            file,
            impl_type,
            sig: sig.clone(),
            block: block.clone(),
        });
    }

    fn add_const(&mut self, ident: &syn::Ident, expr: &Expr) {
        if let Expr::Lit(lit) = expr {
            if let Lit::Str(s) = &lit.lit {
                self.consts.insert(ident.to_string(), s.value());
            }
        }
    }

    fn add_type(
        &mut self,
        ident: &syn::Ident,
        file: usize,
        local_to: Option<usize>,
        item: TypeItem,
    ) {
        self.types
            .entry(ident.to_string())
            .or_default()
            .push(TypeDef {
                file,
                local_to,
                item,
            });
    }

    pub fn constant(&self, name: &str) -> Option<&str> {
        self.consts.get(name).map(String::as_str)
    }

    /// The function `name`, preferring a method of `impl_type`, then a free
    /// function in `file`, then the only one of that name.
    pub fn function(&self, name: &str, impl_type: Option<&str>, file: usize) -> Option<usize> {
        let ids = self.fns_by_name.get(name)?;
        let method =
            |id: &&usize| impl_type.is_some() && self.fns[**id].impl_type.as_deref() == impl_type;
        let in_file =
            |id: &&usize| self.fns[**id].file == file && self.fns[**id].impl_type.is_none();
        ids.iter()
            .find(method)
            .or_else(|| ids.iter().find(in_file))
            .or_else(|| (ids.len() == 1).then(|| &ids[0]))
            .copied()
    }

    /// The type `name` as seen from `scope`: a local of its function, else one
    /// in its file, else the one its file imports, else the only one.
    pub fn find_type(&self, name: &str, scope: Scope) -> Option<&TypeDef> {
        let defs = self.types.get(name)?;
        let visible: Vec<&TypeDef> = defs
            .iter()
            .filter(|d| d.local_to.is_none() || d.local_to == scope.func)
            .collect();
        if let Some(local) = visible.iter().find(|d| d.local_to.is_some()) {
            return Some(local);
        }
        if let Some(same_file) = visible.iter().find(|d| d.file == scope.file) {
            return Some(same_file);
        }
        let uses = &self.files.get(scope.file)?.uses;
        let imported = visible.iter().find(|d| {
            let module = &self.files[d.file].module;
            uses.iter()
                .any(|u| u.contains(module.as_str()) && u.contains(name))
        });
        imported.or_else(|| visible.first()).copied()
    }

    pub fn writes_string(&self, name: &str) -> Option<bool> {
        self.serialize_str.get(name).copied()
    }

    pub fn reads_string(&self, name: &str) -> Option<bool> {
        self.deserialize_str.get(name).copied()
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{} is readable: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Mod(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::Const(i) => &i.attrs,
        _ => &[],
    }
}

fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.meta.to_token_stream().to_string().contains("cfg (test)")
            || a.path().is_ident("test")
    })
}

/// The last path segment's name: `crate::api::Task` → `Task`.
pub fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Type::Reference(r) => type_name(&r.elem),
        Type::Paren(p) => type_name(&p.elem),
        Type::Group(g) => type_name(&g.elem),
        _ => None,
    }
}

/// Structs and enums declared anywhere inside a function body.
#[derive(Default)]
struct LocalItems {
    items: Vec<Item>,
}

impl<'ast> Visit<'ast> for LocalItems {
    fn visit_item(&mut self, item: &'ast Item) {
        if matches!(item, Item::Struct(_) | Item::Enum(_)) {
            self.items.push(item.clone());
        }
    }
}
