//! Compares the reference's shape of one part of an operation with the SDK's.
//!
//! Deliberate rules:
//!
//! - **Nullability is not compared.** The reference writes every optional
//!   field as `anyOf: [T, null]`, so comparing it would flag nearly every
//!   field.
//! - **Required mismatch has one direction: required in the reference,
//!   optional in the SDK.** A client that always sends a field the reference
//!   makes optional is fine, and in a response the reference marks any field
//!   with a default as optional although the API always serializes it.
//! - **A scalar query value matches an array query parameter.** A query string
//!   repeats a key for a list, so one value is a list of one.
//! - **Where the SDK has no concrete type**, the difference is a warning, not a
//!   failure: the check cannot tell what is sent or read there.

use std::collections::BTreeSet;

use super::shape::{Object, Shape, Union};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Missing,
    Extra,
    Type,
    Required,
    Media,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Missing => "field missing in SDK",
            Self::Extra => "field extra in SDK",
            Self::Type => "type mismatch",
            Self::Required => "required mismatch",
            Self::Media => "media type",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Diff {
    pub kind: Kind,
    /// `body.name`, `response.records[].metadata`.
    pub path: String,
    /// Said after the path, in parentheses.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Warning {
    pub path: String,
    pub why: String,
}

#[derive(Default)]
pub struct Comparison {
    pub diffs: Vec<Diff>,
    pub warnings: Vec<Warning>,
    /// Top-level names the SDK need not have (query parameters flagged
    /// `clients: []`).
    pub not_expected: BTreeSet<String>,
    pub query: bool,
}

impl Comparison {
    pub fn diff(&mut self, kind: Kind, path: &str, detail: Option<String>) {
        self.diffs.push(Diff {
            kind,
            path: path.to_owned(),
            detail,
        });
    }

    pub fn warn(&mut self, path: &str, why: &str) {
        self.warnings.push(Warning {
            path: path.to_owned(),
            why: why.to_owned(),
        });
    }

    pub fn compare(&mut self, reference: &Shape, sdk: &Shape, path: &str) {
        match (reference, sdk) {
            (Shape::Any, _) | (_, Shape::Any) => {}
            (_, Shape::Untyped(why)) => self.warn(path, why),
            (Shape::Union(r), Shape::Union(s)) if r.tag.is_some() && r.tag == s.tag => {
                self.tagged(r, s, path);
            }
            (Shape::Union(r), sdk) => match best_variant(r, sdk) {
                Some(variant) => self.compare(variant, sdk, path),
                None => self.mismatch(reference, sdk, path),
            },
            (reference, Shape::Union(s)) => match best_variant(s, reference) {
                Some(variant) => self.compare(reference, variant, path),
                None => self.mismatch(reference, sdk, path),
            },
            (Shape::Array(r), Shape::Array(s)) => self.compare(r, s, &format!("{path}[]")),
            (Shape::Array(r), scalar) if self.query && is_scalar(scalar) => {
                self.compare(r, scalar, path);
            }
            (Shape::Object(r), Shape::Object(s)) => self.objects(r, s, path),
            (r, s) if r.kind() != s.kind() => self.mismatch(r, s, path),
            _ => {}
        }
    }

    fn mismatch(&mut self, reference: &Shape, sdk: &Shape, path: &str) {
        let detail = format!(
            "{} in the reference, {} in the SDK",
            reference.kind(),
            sdk.kind()
        );
        self.diff(Kind::Type, path, Some(detail));
    }

    fn objects(&mut self, reference: &Object, sdk: &Object, path: &str) {
        // A map with no named fields takes any key. A closed object with none
        // (an operation without query parameters) still makes every SDK
        // field an extra one.
        if reference.open && reference.fields.is_empty() {
            return;
        }
        if sdk.fields.is_empty() && sdk.open {
            self.warn(path, "no named fields (a map)");
            return;
        }
        let top = !path.contains(['.', '[', '<']);
        for (name, field) in &reference.fields {
            let at = format!("{path}.{name}");
            match sdk.fields.get(name) {
                None if top && self.not_expected.contains(name) => {}
                None if !sdk.open => self.diff(Kind::Missing, &at, None),
                None => {}
                Some(theirs) => {
                    if field.required && !theirs.required {
                        let detail = "required in the reference, optional in the SDK".to_owned();
                        self.diff(Kind::Required, &at, Some(detail));
                    }
                    self.compare(&field.shape, &theirs.shape, &at);
                }
            }
        }
        if reference.open {
            return;
        }
        for name in sdk
            .fields
            .keys()
            .filter(|n| !reference.fields.contains_key(*n))
        {
            self.diff(Kind::Extra, &format!("{path}.{name}"), None);
        }
    }

    /// Discriminated unions, paired by the tag value.
    fn tagged(&mut self, reference: &Union, sdk: &Union, path: &str) {
        let tag = reference.tag.as_deref().unwrap_or_default();
        for (value, shape) in &reference.variants {
            let at = format!("{path}<{tag}={value}>");
            match sdk.variants.get(value) {
                Some(theirs) => self.compare(shape, theirs, &at),
                None => self.diff(Kind::Missing, &at, None),
            }
        }
        for value in sdk
            .variants
            .keys()
            .filter(|v| !reference.variants.contains_key(*v))
        {
            self.diff(Kind::Extra, &format!("{path}<{tag}={value}>"), None);
        }
    }
}

fn is_scalar(shape: &Shape) -> bool {
    matches!(shape, Shape::String | Shape::Number | Shape::Boolean)
}

/// The variant of `union` that `other` is meant to be: one of the same basic
/// type, and among objects the one sharing the most field names.
fn best_variant<'u>(union: &'u Union, other: &Shape) -> Option<&'u Shape> {
    let overlap = |variant: &Shape| match (variant, other) {
        (Shape::Object(a), Shape::Object(b)) => a
            .fields
            .keys()
            .filter(|k| b.fields.contains_key(*k))
            .count(),
        _ => 0,
    };
    union
        .variants
        .values()
        .filter(|v| v.kind() == other.kind() || matches!(v, Shape::Any))
        .max_by_key(|v| overlap(v))
}
