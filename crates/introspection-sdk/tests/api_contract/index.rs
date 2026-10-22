//! The functions and string constants of the SDK source, which the call-site
//! reader looks up by name. Built once from `crates/introspection-sdk/src`,
//! skipping `#[cfg(test)]` code.

use std::collections::HashMap;
use std::path::Path;

use quote::ToTokens;
use syn::{Attribute, Expr, ImplItem, Item, Lit, Signature, Type};

pub struct FnDef {
    pub file: usize,
    /// The `impl` block's self type, for methods.
    pub impl_type: Option<String>,
    pub sig: Signature,
    pub block: syn::Block,
}

#[derive(Default)]
pub struct Index {
    /// Relative to the crate root, as findings print it: `src/api/tasks.rs`.
    pub files: Vec<String>,
    pub fns: Vec<FnDef>,
    fns_by_name: HashMap<String, Vec<usize>>,
    consts: HashMap<String, String>,
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
        index
    }

    /// Indexes one file's text, as if it were `src/<name>`; tests use it to
    /// check the reader on a small source.
    pub fn from_source(name: &str, text: &str) -> Self {
        let mut index = Self::default();
        let ast = syn::parse_file(text).expect("source parses");
        index.add_file(&format!("src/{name}"), &ast);
        index
    }

    fn add_file(&mut self, path: &str, ast: &syn::File) {
        let file = self.files.len();
        self.files.push(path.to_owned());
        self.add_items(file, &ast.items);
    }

    fn add_items(&mut self, file: usize, items: &[Item]) {
        for item in items {
            if !is_test_only(item_attrs(item)) {
                self.add_item(file, item);
            }
        }
    }

    fn add_item(&mut self, file: usize, item: &Item) {
        match item {
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    self.add_items(file, items);
                }
            }
            Item::Impl(imp) => self.add_impl(file, imp),
            Item::Fn(f) => self.add_fn(file, None, &f.sig, &f.block),
            Item::Const(c) => self.add_const(&c.ident, &c.expr),
            Item::Static(s) => self.add_const(&s.ident, &s.expr),
            _ => {}
        }
    }

    fn add_impl(&mut self, file: usize, imp: &syn::ItemImpl) {
        let self_type = type_name(&imp.self_ty);
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
        self.fns_by_name
            .entry(sig.ident.to_string())
            .or_default()
            .push(self.fns.len());
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
fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Type::Reference(r) => type_name(&r.elem),
        Type::Paren(p) => type_name(&p.elem),
        Type::Group(g) => type_name(&g.elem),
        _ => None,
    }
}
