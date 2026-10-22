//! Rust types lowered into the [`Shape`] serde gives them on the wire, read
//! from the source: field names after `rename` / `rename_all`, `flatten`,
//! `skip*`, `Option` and `default` for what is required, and the enum tagging
//! modes.

use std::collections::HashMap;

use quote::ToTokens;
use syn::{Attribute, Fields, GenericArgument, PathArguments, Type};

use super::index::{Index, Scope, TypeDef, TypeItem};
use super::shape::{Field, Object, Shape, Union};

/// Which way the value travels: the SDK serializes what it sends and
/// deserializes what it reads, and serde attributes differ between the two.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Send,
    Read,
}

pub struct Lower<'a> {
    index: &'a Index,
    dir: Dir,
    /// Types being lowered, so a recursive one stops at [`Shape::Any`].
    stack: Vec<String>,
}

/// Generic parameters in scope: `T` of `Paginated<T>` → the argument.
type Generics = HashMap<String, (Type, Scope)>;

impl<'a> Lower<'a> {
    pub fn new(index: &'a Index, dir: Dir) -> Self {
        Self {
            index,
            dir,
            stack: Vec::new(),
        }
    }

    pub fn shape(&mut self, ty: &Type, scope: Scope) -> Shape {
        self.lower(ty, scope, &Generics::new())
    }

    fn lower(&mut self, ty: &Type, scope: Scope, generics: &Generics) -> Shape {
        match ty {
            Type::Reference(r) => self.lower(&r.elem, scope, generics),
            Type::Paren(p) => self.lower(&p.elem, scope, generics),
            Type::Group(g) => self.lower(&g.elem, scope, generics),
            Type::Slice(s) => Shape::Array(Box::new(self.lower(&s.elem, scope, generics))),
            Type::Array(a) => Shape::Array(Box::new(self.lower(&a.elem, scope, generics))),
            Type::Tuple(t) if t.elems.is_empty() => Shape::object([]),
            Type::Path(p) => self.path(p, scope, generics),
            other => Shape::Untyped(format!(
                "`{}`",
                super::calls::tidy(&other.to_token_stream().to_string())
            )),
        }
    }

    fn path(&mut self, p: &syn::TypePath, scope: Scope, generics: &Generics) -> Shape {
        let Some(last) = p.path.segments.last() else {
            return Shape::Any;
        };
        let name = last.ident.to_string();
        let args = type_args(&last.arguments);
        if let Some((bound, bound_scope)) = generics.get(&name) {
            return self.lower(&bound.clone(), *bound_scope, &Generics::new());
        }
        if let Some(shape) = self.builtin(&name, &args, scope, generics) {
            return shape;
        }
        self.declared(&name, &args, scope, generics)
    }

    /// Standard library and well-known crate types.
    fn builtin(
        &mut self,
        name: &str,
        args: &[Type],
        scope: Scope,
        generics: &Generics,
    ) -> Option<Shape> {
        let mut inner = |at: usize| {
            args.get(at)
                .map_or(Shape::Any, |t| self.lower(t, scope, generics))
        };
        Some(match name {
            "String" | "str" | "char" | "Uuid" | "DateTime" | "NaiveDate" | "NaiveDateTime"
            | "PathBuf" | "Url" => Shape::String,
            "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
            | "i128" | "isize" | "f32" | "f64" | "NonZeroU32" | "NonZeroU64" => Shape::Number,
            "bool" => Shape::Boolean,
            "Option" | "Box" | "Arc" | "Rc" => inner(0),
            "Cow" => inner(args.len().saturating_sub(1)),
            "Vec" | "VecDeque" | "HashSet" | "BTreeSet" | "IndexSet" => {
                Shape::Array(Box::new(inner(0)))
            }
            "HashMap" | "BTreeMap" | "IndexMap" | "Map" => Shape::Object(Object {
                open: true,
                ..Object::default()
            }),
            "Value" => Shape::Untyped("serde_json::Value".to_owned()),
            _ => return None,
        })
    }

    fn declared(&mut self, name: &str, args: &[Type], scope: Scope, generics: &Generics) -> Shape {
        if let Some(shape) = self.hand_written(name) {
            return shape;
        }
        let Some(def) = self.index.find_type(name, scope).cloned() else {
            return Shape::Untyped(format!("`{name}` is not declared in the SDK source"));
        };
        let key = format!("{name}@{}", def.file);
        if self.stack.contains(&key) {
            return Shape::Any;
        }
        let def_scope = Scope {
            file: def.file,
            func: def.local_to,
        };
        let bound = bind_generics(&def, args, scope, generics);
        self.stack.push(key);
        let shape = match &def.item {
            TypeItem::Alias(alias) => self.lower(&alias.ty, def_scope, &bound),
            TypeItem::Struct(s) => self.structure(s, def_scope, &bound),
            TypeItem::Enum(e) => self.enumeration(e, def_scope, &bound),
        };
        self.stack.pop();
        shape
    }

    /// A type with a hand-written impl for this direction: a plain string
    /// when the impl writes or reads one, untyped otherwise.
    fn hand_written(&self, name: &str) -> Option<Shape> {
        let writes = match self.dir {
            Dir::Send => self.index.writes_string(name)?,
            Dir::Read => self.index.reads_string(name)?,
        };
        Some(if writes {
            Shape::String
        } else {
            Shape::Untyped(format!("`{name}` has a hand-written serde impl"))
        })
    }

    fn structure(&mut self, s: &syn::ItemStruct, scope: Scope, generics: &Generics) -> Shape {
        let serde = SerdeAttrs::of(&s.attrs);
        if let Some(via) = serde.via(self.dir) {
            return self.via(&via, scope, generics);
        }
        match &s.fields {
            Fields::Named(named) if serde.transparent && named.named.len() == 1 => {
                self.lower(&named.named[0].ty, scope, generics)
            }
            Fields::Named(named) => {
                let mut object = Object::default();
                for field in &named.named {
                    self.field(field, &serde, scope, generics, &mut object);
                }
                Shape::Object(object)
            }
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                self.lower(&unnamed.unnamed[0].ty, scope, generics)
            }
            Fields::Unnamed(_) => Shape::Untyped("a tuple struct".to_owned()),
            Fields::Unit => Shape::object([]),
        }
    }

    /// `#[serde(from = "...")]` and friends: the value travels as that type.
    fn via(&mut self, via: &str, scope: Scope, generics: &Generics) -> Shape {
        match syn::parse_str::<Type>(via) {
            Ok(ty) => self.lower(&ty, scope, generics),
            Err(_) => Shape::Untyped(format!("serde via `{via}`")),
        }
    }

    fn field(
        &mut self,
        field: &syn::Field,
        container: &SerdeAttrs,
        scope: Scope,
        generics: &Generics,
        object: &mut Object,
    ) {
        let attrs = SerdeAttrs::of(&field.attrs);
        if attrs.skipped(self.dir) {
            return;
        }
        let is_option = is_option(&field.ty);
        if attrs.flatten {
            let shape = self.lower(&field.ty, scope, generics);
            merge(object, shape, !is_option);
            return;
        }
        let Some(ident) = &field.ident else { return };
        let name = attrs.rename.clone().unwrap_or_else(|| {
            let ident = ident.to_string();
            let ident = ident.trim_start_matches("r#");
            rename_field(ident, container.rename_all.as_deref())
        });
        let required = match self.dir {
            Dir::Send => attrs.skip_serializing_if.is_none(),
            Dir::Read => !is_option && !attrs.default && !container.default,
        };
        let shape = if attrs.custom {
            Shape::Any
        } else {
            self.lower(&field.ty, scope, generics)
        };
        object.fields.insert(name, Field { shape, required });
    }

    fn enumeration(&mut self, e: &syn::ItemEnum, scope: Scope, generics: &Generics) -> Shape {
        let serde = SerdeAttrs::of(&e.attrs);
        if let Some(via) = serde.via(self.dir) {
            return self.via(&via, scope, generics);
        }
        let variants: Vec<&syn::Variant> = e
            .variants
            .iter()
            .filter(|v| {
                let attrs = SerdeAttrs::of(&v.attrs);
                // `#[serde(other)]` and an `#[serde(untagged)]` variant are
                // catch-alls for values this SDK build does not know.
                !attrs.other && !attrs.untagged && !attrs.skipped(self.dir)
            })
            .collect();
        if serde.untagged {
            let shapes = variants
                .iter()
                .map(|v| self.variant_content(v, scope, generics));
            let variants = shapes
                .enumerate()
                .map(|(i, s)| (i.to_string(), s))
                .collect();
            return Shape::Union(Union {
                tag: None,
                variants,
            });
        }
        let all_unit = variants.iter().all(|v| matches!(v.fields, Fields::Unit));
        match (&serde.tag, &serde.content) {
            (None, _) if all_unit => Shape::String,
            (Some(tag), None) => self.internally_tagged(tag, &variants, &serde, scope, generics),
            (Some(_), Some(_)) => Shape::Untyped("an adjacently tagged enum".to_owned()),
            (None, _) => Shape::Untyped("an externally tagged enum".to_owned()),
        }
    }

    fn internally_tagged(
        &mut self,
        tag: &str,
        variants: &[&syn::Variant],
        serde: &SerdeAttrs,
        scope: Scope,
        generics: &Generics,
    ) -> Shape {
        let mut union = Union {
            tag: Some(tag.to_owned()),
            ..Union::default()
        };
        for variant in variants {
            let attrs = SerdeAttrs::of(&variant.attrs);
            let value = attrs.rename.clone().unwrap_or_else(|| {
                rename_variant(&variant.ident.to_string(), serde.rename_all.as_deref())
            });
            let mut object = match self.variant_content(variant, scope, generics) {
                Shape::Object(object) => object,
                _ => Object::default(),
            };
            let tag_field = Field {
                shape: Shape::String,
                required: true,
            };
            object.fields.insert(tag.to_owned(), tag_field);
            union.variants.insert(value, Shape::Object(object));
        }
        Shape::Union(union)
    }

    fn variant_content(
        &mut self,
        variant: &syn::Variant,
        scope: Scope,
        generics: &Generics,
    ) -> Shape {
        let attrs = SerdeAttrs::of(&variant.attrs);
        match &variant.fields {
            Fields::Named(named) => {
                let container = SerdeAttrs {
                    rename_all: attrs.rename_all.clone(),
                    ..SerdeAttrs::default()
                };
                let mut object = Object::default();
                for field in &named.named {
                    self.field(field, &container, scope, generics, &mut object);
                }
                Shape::Object(object)
            }
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                self.lower(&unnamed.unnamed[0].ty, scope, generics)
            }
            Fields::Unnamed(_) => Shape::Untyped("a tuple variant".to_owned()),
            Fields::Unit => Shape::object([]),
        }
    }
}

/// A flattened field's fields join the parent's; a flattened map makes the
/// parent open.
fn merge(object: &mut Object, shape: Shape, required: bool) {
    match shape {
        Shape::Object(inner) => {
            object.open |= inner.open;
            for (name, mut field) in inner.fields {
                field.required &= required;
                object.fields.insert(name, field);
            }
        }
        _ => object.open = true,
    }
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

fn type_args(arguments: &PathArguments) -> Vec<Type> {
    match arguments {
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter_map(|a| match a {
                GenericArgument::Type(t) => Some(t.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn bind_generics(def: &TypeDef, args: &[Type], scope: Scope, outer: &Generics) -> Generics {
    let params = match &def.item {
        TypeItem::Struct(s) => &s.generics,
        TypeItem::Enum(e) => &e.generics,
        TypeItem::Alias(a) => &a.generics,
    };
    params
        .type_params()
        .zip(args)
        .map(|(param, arg)| {
            let bound = type_args_name(arg)
                .and_then(|n| outer.get(&n).cloned())
                .unwrap_or_else(|| (arg.clone(), scope));
            (param.ident.to_string(), bound)
        })
        .collect()
}

fn type_args_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) if p.path.segments.len() == 1 => Some(p.path.segments[0].ident.to_string()),
        _ => None,
    }
}

/// The `#[serde(...)]` attributes the check reads.
#[derive(Default)]
struct SerdeAttrs {
    rename: Option<String>,
    rename_all: Option<String>,
    tag: Option<String>,
    content: Option<String>,
    untagged: bool,
    default: bool,
    flatten: bool,
    skip: bool,
    skip_serializing: bool,
    skip_deserializing: bool,
    skip_serializing_if: Option<String>,
    other: bool,
    /// `with`, `serialize_with` or `deserialize_with`: the wire type is
    /// whatever that function makes of it.
    custom: bool,
    from: Option<String>,
    into: Option<String>,
    transparent: bool,
}

impl SerdeAttrs {
    fn of(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
            let _ = attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let value = match meta.value() {
                    Ok(v) => v.parse::<syn::LitStr>().ok().map(|s| s.value()),
                    Err(_) => None,
                };
                out.set(&key, value);
                Ok(())
            });
        }
        out
    }

    fn set(&mut self, key: &str, value: Option<String>) {
        match key {
            "rename" => self.rename = value.or(self.rename.take()),
            "rename_all" => self.rename_all = value,
            "tag" => self.tag = value,
            "content" => self.content = value,
            "untagged" => self.untagged = true,
            "default" => self.default = true,
            "flatten" => self.flatten = true,
            "skip" => self.skip = true,
            "skip_serializing" => self.skip_serializing = true,
            "skip_deserializing" => self.skip_deserializing = true,
            "skip_serializing_if" => self.skip_serializing_if = value,
            "other" => self.other = true,
            "with" | "serialize_with" | "deserialize_with" => self.custom = true,
            "from" | "try_from" => self.from = value,
            "into" => self.into = value,
            "transparent" => self.transparent = true,
            _ => {}
        }
    }

    fn skipped(&self, dir: Dir) -> bool {
        self.skip
            || match dir {
                Dir::Send => self.skip_serializing,
                Dir::Read => self.skip_deserializing,
            }
    }

    fn via(&self, dir: Dir) -> Option<String> {
        match dir {
            Dir::Send => self.into.clone(),
            Dir::Read => self.from.clone(),
        }
    }
}

/// serde's `rename_all` for a `snake_case` field name.
fn rename_field(ident: &str, rule: Option<&str>) -> String {
    let pascal = || {
        ident
            .split('_')
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<String>()
    };
    match rule {
        Some("UPPERCASE" | "SCREAMING_SNAKE_CASE") => ident.to_uppercase(),
        Some("PascalCase") => pascal(),
        Some("camelCase") => lower_first(&pascal()),
        Some("kebab-case") => ident.replace('_', "-"),
        Some("SCREAMING-KEBAB-CASE") => ident.replace('_', "-").to_uppercase(),
        _ => ident.to_owned(),
    }
}

/// serde's `rename_all` for a `PascalCase` variant name.
fn rename_variant(ident: &str, rule: Option<&str>) -> String {
    let snake = || {
        let mut out = String::new();
        for (i, c) in ident.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        }
        out
    };
    match rule {
        Some("lowercase") => ident.to_lowercase(),
        Some("UPPERCASE") => ident.to_uppercase(),
        Some("camelCase") => lower_first(ident),
        Some("snake_case") => snake(),
        Some("SCREAMING_SNAKE_CASE") => snake().to_uppercase(),
        Some("kebab-case") => snake().replace('_', "-"),
        Some("SCREAMING-KEBAB-CASE") => snake().replace('_', "-").to_uppercase(),
        _ => ident.to_owned(),
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}
