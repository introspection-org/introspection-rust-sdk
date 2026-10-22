//! The one model both sides are lowered into before they are compared: what a
//! JSON value looks like on the wire, at the level of detail the check compares
//! (field names, required vs optional, basic types).

use std::collections::BTreeMap;

/// A JSON value's shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    String,
    /// Integers and floats alike: the reference's `integer` and the SDK's
    /// `u32` are one basic type here.
    Number,
    Boolean,
    Array(Box<Shape>),
    Object(Object),
    Union(Union),
    /// Anything goes: the reference declares no type, or the comparison
    /// stopped at a recursive reference it is already inside.
    Any,
    /// The SDK side has no concrete type here. The string says why, for the
    /// warning that names it.
    Untyped(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    pub fields: BTreeMap<String, Field>,
    /// A map (`additionalProperties`, `HashMap`) rather than named fields.
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub shape: Shape,
    /// On the wire every time: required in the reference; for the SDK, a
    /// request field it always sends or a response field it cannot do without.
    pub required: bool,
}

/// A value that is one of several shapes. When `tag` is set, the variants are
/// keyed by the value of that property (a discriminated union); otherwise by
/// their position.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Union {
    pub tag: Option<String>,
    pub variants: BTreeMap<String, Shape>,
}

impl Shape {
    /// The basic type's name, as findings print it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
            Self::Union(_) => "union",
            Self::Any => "any",
            Self::Untyped(_) => "untyped",
        }
    }

    pub fn object(fields: impl IntoIterator<Item = (String, Field)>) -> Self {
        Self::Object(Object {
            fields: fields.into_iter().collect(),
            open: false,
        })
    }
}
