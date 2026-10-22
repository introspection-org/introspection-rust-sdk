//! The two OpenAPI documents: their operations, the `x-introspection` flags on
//! them, and their schemas lowered into [`Shape`]s.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::shape::{Field, Object, Shape, Union};

const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// One plane's document and where it was read from.
pub struct Document {
    pub plane: &'static str,
    pub source: String,
    pub spec: Value,
}

/// One operation of a document.
pub struct Operation<'a> {
    pub plane: &'static str,
    pub method: String,
    pub path: String,
    pub clients: Vec<String>,
    op: &'a Value,
    spec: &'a Value,
}

impl Document {
    pub fn operations(&self) -> Vec<Operation<'_>> {
        let Some(paths) = self.spec["paths"].as_object() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (path, item) in paths {
            for method in METHODS {
                let op = &item[method];
                if op.is_object() {
                    out.push(Operation {
                        plane: self.plane,
                        method: method.to_uppercase(),
                        path: path.clone(),
                        clients: clients(op),
                        op,
                        spec: &self.spec,
                    });
                }
            }
        }
        out
    }
}

fn clients(value: &Value) -> Vec<String> {
    value["x-introspection"]["clients"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|c| c.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A route with every path parameter written `{}`, so `/v1/tasks/{task_id}`
/// and the SDK's `format!("/v1/tasks/{}", id)` meet by position.
pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed
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

impl Operation<'_> {
    pub fn key(&self) -> String {
        format!("{} {}", self.method, normalize_path(&self.path))
    }

    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }

    pub fn is_for(&self, client: &str) -> bool {
        self.clients.iter().any(|c| c == client)
    }

    fn parameters(&self) -> impl Iterator<Item = &Value> {
        self.op["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| self.resolve(p))
            .filter(|p| p["in"].as_str() == Some("query"))
    }

    /// The query parameters as one object.
    pub fn query(&self) -> Shape {
        let mut lower = Lowerer::new(self.spec);
        Shape::object(self.parameters().filter_map(|p| {
            let name = p["name"].as_str()?;
            let field = Field {
                shape: lower.shape(&p["schema"]),
                required: p["required"].as_bool().unwrap_or(false),
            };
            Some((name.to_owned(), field))
        }))
    }

    /// Query parameters flagged `clients: []`: no client is expected to send
    /// them, so a client that leaves them out is not missing anything.
    pub fn unexpected_parameters(&self) -> BTreeSet<String> {
        self.parameters()
            .filter(|p| p["x-introspection"].is_object() && clients(p).is_empty())
            .filter_map(|p| p["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// The request body's media types, and whether a body is required.
    pub fn request_body(&self) -> Option<(bool, &Map<String, Value>)> {
        let body = self.resolve(&self.op["requestBody"]);
        let content = body["content"].as_object()?;
        Some((body["required"].as_bool().unwrap_or(false), content))
    }

    /// The first 2xx response's media types (empty for a response without a
    /// body, such as a 204).
    pub fn response(&self) -> Option<Map<String, Value>> {
        let responses = self.op["responses"].as_object()?;
        let (_, response) = responses
            .iter()
            .filter(|(status, _)| status.starts_with('2'))
            .min_by_key(|(status, _)| status.as_str())?;
        Some(
            self.resolve(response)["content"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )
    }

    pub fn lower(&self, schema: &Value) -> Shape {
        Lowerer::new(self.spec).shape(schema)
    }

    fn resolve<'v>(&'v self, value: &'v Value) -> &'v Value {
        match value["$ref"].as_str() {
            Some(reference) => pointer(self.spec, reference).unwrap_or(value),
            None => value,
        }
    }
}

fn pointer<'v>(spec: &'v Value, reference: &str) -> Option<&'v Value> {
    spec.pointer(reference.strip_prefix('#')?)
}

/// The media type of `content` the client reads or sends as `media`: an exact
/// match, else one whose type without parameters matches.
pub fn media<'c>(content: &'c Map<String, Value>, media: &str) -> Option<&'c Value> {
    let bare = |m: &str| m.split(';').next().unwrap_or(m).trim().to_owned();
    content.get(media).or_else(|| {
        content
            .iter()
            .find(|(m, _)| bare(m) == bare(media))
            .map(|(_, v)| v)
    })
}

/// Lowers schemas, following `$ref` and stopping at a reference it is already
/// inside, so a recursive schema ends in [`Shape::Any`] instead of looping.
struct Lowerer<'a> {
    spec: &'a Value,
    stack: Vec<String>,
}

impl<'a> Lowerer<'a> {
    fn new(spec: &'a Value) -> Self {
        Self {
            spec,
            stack: Vec::new(),
        }
    }

    fn shape(&mut self, schema: &Value) -> Shape {
        if let Some(reference) = schema["$ref"].as_str() {
            return self.reference(reference);
        }
        if let Some(parts) = schema["allOf"].as_array() {
            return self.all_of(parts);
        }
        let branches = schema["anyOf"].as_array().or(schema["oneOf"].as_array());
        if let Some(branches) = branches {
            return self.one_of(schema, branches);
        }
        self.typed(schema)
    }

    fn reference(&mut self, reference: &str) -> Shape {
        if self.stack.iter().any(|r| r == reference) {
            return Shape::Any;
        }
        let Some(target) = pointer(self.spec, reference) else {
            return Shape::Any;
        };
        self.stack.push(reference.to_owned());
        let shape = self.shape(target);
        self.stack.pop();
        shape
    }

    fn all_of(&mut self, parts: &[Value]) -> Shape {
        let mut shapes: Vec<Shape> = parts.iter().map(|p| self.shape(p)).collect();
        if shapes.len() == 1 {
            return shapes.remove(0);
        }
        let mut merged = Object::default();
        for shape in shapes {
            if let Shape::Object(object) = shape {
                merged.open |= object.open;
                merged.fields.extend(object.fields);
            }
        }
        Shape::Object(merged)
    }

    fn one_of(&mut self, schema: &Value, branches: &[Value]) -> Shape {
        let branches: Vec<&Value> = branches.iter().filter(|b| !is_null(b)).collect();
        if let [only] = branches.as_slice() {
            return self.shape(only);
        }
        let tag = schema["discriminator"]["propertyName"].as_str();
        let mut union = Union {
            tag: tag.map(str::to_owned),
            ..Union::default()
        };
        for (index, branch) in branches.iter().enumerate() {
            let shape = self.shape(branch);
            let key = tag
                .and_then(|tag| self.tag_value(schema, branch, tag))
                .unwrap_or_else(|| index.to_string());
            union.variants.insert(key, shape);
        }
        Shape::Union(union)
    }

    /// The value of a discriminated branch's tag: from the discriminator's
    /// mapping, else from the branch's own `const` or one-value `enum`.
    fn tag_value(&self, schema: &Value, branch: &Value, tag: &str) -> Option<String> {
        if let (Some(mapping), Some(reference)) = (
            schema["discriminator"]["mapping"].as_object(),
            branch["$ref"].as_str(),
        ) {
            let found = mapping.iter().find(|(_, r)| r.as_str() == Some(reference));
            if let Some((value, _)) = found {
                return Some(value.clone());
            }
        }
        let target = match branch["$ref"].as_str() {
            Some(reference) => pointer(self.spec, reference)?,
            None => branch,
        };
        let property = &target["properties"][tag];
        property["const"]
            .as_str()
            .or_else(|| property["enum"].as_array()?.first()?.as_str())
            .map(str::to_owned)
    }

    fn typed(&mut self, schema: &Value) -> Shape {
        match type_name(schema).as_deref() {
            Some("string") => Shape::String,
            Some("integer" | "number") => Shape::Number,
            Some("boolean") => Shape::Boolean,
            Some("array") => Shape::Array(Box::new(self.shape(&schema["items"]))),
            Some("object") => self.object(schema),
            None if schema["properties"].is_object() => self.object(schema),
            _ => Shape::Any,
        }
    }

    fn object(&mut self, schema: &Value) -> Shape {
        let required: BTreeSet<&str> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let Some(properties) = schema["properties"].as_object() else {
            return Shape::Object(Object {
                open: true,
                ..Object::default()
            });
        };
        let fields = properties.iter().map(|(name, property)| {
            let field = Field {
                shape: self.shape(property),
                required: required.contains(name.as_str()),
            };
            (name.clone(), field)
        });
        Shape::object(fields.collect::<Vec<_>>())
    }
}

fn is_null(schema: &Value) -> bool {
    type_name(schema).as_deref() == Some("null")
}

/// The schema's type: `type` (the first non-null one when it is a list), else
/// what its `const` or `enum` values are.
fn type_name(schema: &Value) -> Option<String> {
    match &schema["type"] {
        Value::String(t) => return Some(t.clone()),
        Value::Array(types) => {
            let types: Vec<&str> = types.iter().filter_map(Value::as_str).collect();
            let non_null = types.iter().find(|t| **t != "null");
            return non_null.or(types.first()).map(|t| (*t).to_owned());
        }
        _ => {}
    }
    let value = schema
        .get("const")
        .or_else(|| schema["enum"].as_array()?.first())?;
    Some(
        match value {
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "boolean",
            _ => return None,
        }
        .to_owned(),
    )
}
