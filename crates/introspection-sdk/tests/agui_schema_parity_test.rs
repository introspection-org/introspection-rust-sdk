//! Holds the hand-written AG-UI types to `@ag-ui/core`'s own schemas.
//!
//! `fixtures/ag-ui-core.schema.json` is a JSON Schema snapshot of the
//! published package's zod schemas (`scripts/export-ag-ui-schema.mjs`). From
//! it this test generates instances and checks each against the Rust type:
//!
//! - every maximal instance (every property, each enum value and union branch
//!   in turn) decodes typed and serializes back to the same JSON, so a dropped
//!   or renamed property fails;
//! - every minimal instance (required properties only) does the same;
//! - removing any required property, or putting a value of the wrong type, out
//!   of an enum, off a pattern or under `minItems` anywhere in an instance,
//!   fails the decode (for an event: decodes to `Event::Unknown`).
//!
//! To move to a new AG-UI release, regenerate the snapshot and rerun this.

use std::collections::{BTreeSet, HashSet};

use introspection_sdk::agui::{
    Context, Event, EventType, FunctionCall, InputContent, Interrupt, JsonPatchOperation,
    JsonPointer, Message, PartSource, ReasoningEncryptedValueSubtype, ResumeEntry, RunAgentInput,
    RunFinishedOutcome, SubagentFinishedOutcome, TextMessageRole, TokenUsage, Tool, ToolCall,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Map, Value};

const SNAPSHOT: &str = include_str!("fixtures/ag-ui-core.schema.json");

/// Decodes a value as a Rust type and serializes it back; `None` when the
/// type rejects it.
type Decoder = fn(&Value) -> Option<Value>;

fn typed<T: DeserializeOwned + Serialize>(v: &Value) -> Option<Value> {
    let t: T = serde_json::from_value(v.clone()).ok()?;
    Some(serde_json::to_value(t).expect("a decoded value serializes"))
}

fn event(v: &Value) -> Option<Value> {
    match serde_json::from_value::<Event>(v.clone()).expect("an event never fails to decode") {
        Event::Unknown(_) => None,
        e => Some(serde_json::to_value(e).expect("an event serializes")),
    }
}

/// The non-event `$defs` and the Rust type each one decodes as. A union's
/// branches are checked through the union.
fn decoders() -> Vec<(&'static str, Decoder)> {
    vec![
        ("Message", typed::<Message>),
        ("InputContent", typed::<InputContent>),
        ("ContentPart", typed::<InputContent>),
        ("PartSource", typed::<PartSource>),
        ("InputContentSource", typed::<PartSource>),
        ("ToolCall", typed::<ToolCall>),
        ("FunctionCall", typed::<FunctionCall>),
        ("JsonPatchOperation", typed::<JsonPatchOperation>),
        ("JsonPointer", typed::<JsonPointer>),
        ("RunAgentInput", typed::<RunAgentInput>),
        ("Tool", typed::<Tool>),
        ("Context", typed::<Context>),
        ("ResumeEntry", typed::<ResumeEntry>),
        ("RunFinishedOutcome", typed::<RunFinishedOutcome>),
        ("Interrupt", typed::<Interrupt>),
        ("SubagentFinishedOutcome", typed::<SubagentFinishedOutcome>),
        ("TokenUsage", typed::<TokenUsage>),
        ("TextMessageRole", typed::<TextMessageRole>),
        (
            "ReasoningEncryptedValueSubtype",
            typed::<ReasoningEncryptedValueSubtype>,
        ),
        ("EventType", typed::<EventType>),
        ("Event", event),
    ]
}

#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    Index(usize),
}

type Path = Vec<Seg>;

/// A generated instance, with the edits that must each make it invalid.
#[derive(Debug, Clone)]
struct Instance {
    value: Value,
    /// Replacing the value at the path with this one breaks the schema.
    invalid: Vec<(Path, Value)>,
    /// Removing the property at the path breaks the schema.
    required: Vec<Path>,
}

impl Instance {
    fn leaf(value: Value, invalid: Vec<Value>) -> Self {
        Instance {
            value,
            invalid: invalid.into_iter().map(|v| (Vec::new(), v)).collect(),
            required: Vec::new(),
        }
    }

    fn nest(&mut self, seg: &Seg, inner: Instance) {
        let prefix = |p: Path| std::iter::once(seg.clone()).chain(p).collect();
        self.invalid
            .extend(inner.invalid.into_iter().map(|(p, v)| (prefix(p), v)));
        self.required.extend(inner.required.into_iter().map(prefix));
    }
}

struct Schema {
    defs: Map<String, Value>,
}

impl Schema {
    fn load() -> Self {
        let doc: Value = serde_json::from_str(SNAPSHOT).expect("the snapshot is JSON");
        let defs = doc["$defs"]
            .as_object()
            .expect("the snapshot has $defs")
            .clone();
        Schema { defs }
    }

    fn def(&self, name: &str) -> &Value {
        self.defs
            .get(name)
            .unwrap_or_else(|| panic!("no $def {name}"))
    }

    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        match ref_name(schema) {
            Some(name) => self.resolve(self.def(name)),
            None => schema,
        }
    }

    fn branches<'a>(&'a self, schema: &'a Value) -> Option<&'a Vec<Value>> {
        let s = self.resolve(schema);
        s.get("oneOf")
            .or_else(|| s.get("anyOf"))
            .and_then(Value::as_array)
    }

    /// Every instance worth checking: `maximal` sets every property,
    /// otherwise only the required ones.
    fn instances(&self, schema: &Value, maximal: bool) -> Vec<Instance> {
        let s = self.resolve(schema);
        if let Some(branches) = self.branches(s) {
            return self.union(branches, maximal);
        }
        if let Some(c) = s.get("const") {
            return vec![Instance::leaf(c.clone(), vec![json!("not-the-const")])];
        }
        if let Some(values) = s.get("enum").and_then(Value::as_array) {
            let invalid = || vec![json!("not-in-the-enum")];
            return values
                .iter()
                .map(|v| Instance::leaf(v.clone(), invalid()))
                .collect();
        }
        match s.get("type").and_then(Value::as_str) {
            Some("object") => self.object(s, maximal),
            Some("array") => vec![self.array(s, maximal)],
            Some(ty) => vec![scalar(s, ty)],
            None => vec![Instance::leaf(json!({"key": "value"}), Vec::new())],
        }
    }

    /// Each branch's instances. A replacement at the union's own root that
    /// breaks one branch may satisfy another, so those give way to one that
    /// breaks every branch.
    fn union(&self, branches: &[Value], maximal: bool) -> Vec<Instance> {
        let open = branches.iter().any(|b| {
            let r = self.resolve(b);
            r.get("type").is_none() && self.branches(r).is_none() && r.get("const").is_none()
        });
        let mut out = Vec::new();
        for branch in branches {
            for mut inst in self.instances(branch, maximal) {
                inst.invalid.retain(|(p, _)| !p.is_empty());
                if !open {
                    inst.invalid.push((Vec::new(), json!(true)));
                }
                out.push(inst);
            }
        }
        out
    }

    fn object(&self, s: &Value, maximal: bool) -> Vec<Instance> {
        let required: HashSet<&str> = s["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let props: Vec<(&String, Vec<Instance>)> = s["properties"]
            .as_object()
            .map(|p| p.iter().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| maximal || required.contains(name.as_str()))
            .map(|(name, prop)| (name, self.instances(prop, maximal)))
            .collect();
        let count = props.iter().map(|(_, i)| i.len()).max().unwrap_or(1);
        (0..count)
            .map(|n| {
                let mut out = Instance::leaf(json!({}), vec![json!("not-an-object")]);
                for (name, insts) in &props {
                    let inst = insts[n % insts.len()].clone();
                    out.value[name.as_str()] = inst.value.clone();
                    let seg = Seg::Key((*name).clone());
                    if required.contains(name.as_str()) {
                        out.required.push(vec![seg.clone()]);
                    }
                    out.nest(&seg, inst);
                }
                out
            })
            .collect()
    }

    fn array(&self, s: &Value, maximal: bool) -> Instance {
        let min_items = s.get("minItems").and_then(Value::as_u64).unwrap_or(0);
        let mut invalid = vec![json!("not-an-array")];
        if min_items > 0 {
            invalid.push(json!([]));
        }
        let mut out = Instance::leaf(json!([]), invalid);
        if !maximal && min_items == 0 {
            return out;
        }
        let items = self.instances(&s["items"], maximal);
        for (i, inst) in items.into_iter().enumerate() {
            out.value
                .as_array_mut()
                .expect("an array")
                .push(inst.value.clone());
            out.nest(&Seg::Index(i), inst);
        }
        out
    }

    /// The `$defs` an `Event` can reach.
    fn reachable(&self, from: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from.to_string()];
        while let Some(name) = stack.pop() {
            if seen.insert(name.clone()) {
                collect_refs(self.def(&name), &mut stack);
            }
        }
        seen
    }
}

fn ref_name(schema: &Value) -> Option<&str> {
    schema.get("$ref")?.as_str()?.strip_prefix("#/$defs/")
}

fn collect_refs(schema: &Value, out: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(name) = ref_name(schema) {
                out.push(name.to_string());
            }
            map.values().for_each(|v| collect_refs(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_refs(v, out)),
        _ => {}
    }
}

fn scalar(s: &Value, ty: &str) -> Instance {
    match ty {
        "string" if s.get("pattern").is_some() => Instance::leaf(
            json!("/a~0b/c~1d"),
            vec![json!(42), json!("off-the-pattern")],
        ),
        "string" => Instance::leaf(json!("s"), vec![json!(42)]),
        "integer" => {
            let mut invalid = vec![json!(1.5), json!("s")];
            if s.get("minimum").and_then(Value::as_i64) == Some(0) {
                invalid.push(json!(-1));
            }
            Instance::leaf(json!(42), invalid)
        }
        "boolean" => Instance::leaf(json!(true), vec![json!("s")]),
        "null" => Instance::leaf(Value::Null, Vec::new()),
        other => panic!("no sample for type {other}"),
    }
}

/// A JSON value cut short enough to read in a failure list.
fn short(v: &Value) -> String {
    let s = v.to_string();
    match s.char_indices().nth(200) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// Fails listing the first failures, if there are any.
fn assert_none(failures: &[String]) {
    let shown: Vec<&str> = failures.iter().take(20).map(String::as_str).collect();
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        shown.join("\n")
    );
}

fn at<'a>(value: &'a mut Value, path: &[Seg]) -> &'a mut Value {
    path.iter().fold(value, |v, seg| match seg {
        Seg::Key(k) => &mut v[k.as_str()],
        Seg::Index(i) => &mut v[*i],
    })
}

/// Every way `decode` disagrees with the schema at `schema`, as messages.
fn check(schema: &Schema, def: &str, decode: Decoder) -> Vec<String> {
    let mut failures = Vec::new();
    let root = json!({ "$ref": format!("#/$defs/{def}") });
    for maximal in [true, false] {
        for inst in schema.instances(&root, maximal) {
            match decode(&inst.value) {
                Some(back) if back == inst.value => {}
                Some(back) => failures.push(format!(
                    "{def}: {} came back as {}",
                    short(&inst.value),
                    short(&back)
                )),
                None => failures.push(format!(
                    "{def}: valid {} did not decode",
                    short(&inst.value)
                )),
            }
            for (path, bad) in &inst.invalid {
                let mut v = inst.value.clone();
                *at(&mut v, path) = bad.clone();
                if decode(&v).is_some() {
                    failures.push(format!("{def}: {bad} at {path:?} decoded: {}", short(&v)));
                }
            }
            for path in &inst.required {
                let mut v = inst.value.clone();
                let (last, parent) = path.split_last().expect("a property path");
                let Seg::Key(key) = last else {
                    panic!("a property path")
                };
                at(&mut v, parent)
                    .as_object_mut()
                    .expect("an object")
                    .remove(key);
                if decode(&v).is_some() {
                    failures.push(format!(
                        "{def}: decoded without required {path:?}: {}",
                        short(&v)
                    ));
                }
            }
        }
    }
    failures
}

fn event_defs(schema: &Schema) -> Vec<(String, String)> {
    schema.def("Event")["oneOf"]
        .as_array()
        .expect("Event is a oneOf")
        .iter()
        .map(|b| {
            let name = ref_name(b).expect("an Event branch is a $ref").to_string();
            let ty = schema.def(&name)["properties"]["type"]["const"]
                .as_str()
                .unwrap_or_else(|| panic!("{name} has no type const"))
                .to_string();
            (name, ty)
        })
        .collect()
}

/// Its index in [`EventType::ALL`]. Exhaustive, so a new variant does not
/// compile until it has one, and the test below then needs it in `ALL`.
fn position(t: EventType) -> usize {
    use EventType::*;
    match t {
        TextMessageStart => 0,
        TextMessageContent => 1,
        TextMessageEnd => 2,
        TextMessageChunk => 3,
        ToolCallStart => 4,
        ToolCallArgs => 5,
        ToolCallEnd => 6,
        ToolCallChunk => 7,
        ToolCallResult => 8,
        StateSnapshot => 9,
        StateDelta => 10,
        MessagesSnapshot => 11,
        ActivitySnapshot => 12,
        ActivityDelta => 13,
        Raw => 14,
        Custom => 15,
        RunStarted => 16,
        RunFinished => 17,
        RunError => 18,
        StepStarted => 19,
        StepFinished => 20,
        ReasoningStart => 21,
        ReasoningMessageStart => 22,
        ReasoningMessageContent => 23,
        ReasoningMessageEnd => 24,
        ReasoningMessageChunk => 25,
        ReasoningEnd => 26,
        ReasoningEncryptedValue => 27,
        SubagentStarted => 28,
        SubagentFinished => 29,
        SubagentError => 30,
    }
}

#[test]
fn snapshot_is_ag_ui_core_1_0_5() {
    let doc: Value = serde_json::from_str(SNAPSHOT).unwrap();
    assert_eq!(doc["title"], "@ag-ui/core@1.0.5");
}

#[test]
fn event_types_are_exactly_the_schemas() {
    let schema = Schema::load();
    for (i, t) in EventType::ALL.iter().enumerate() {
        assert_eq!(position(*t), i, "EventType::ALL is out of order at {t}");
    }
    let rust: BTreeSet<String> = EventType::ALL.iter().map(ToString::to_string).collect();
    let consts: BTreeSet<String> = event_defs(&schema).into_iter().map(|(_, t)| t).collect();
    assert_eq!(rust, consts);
    let enum_values: BTreeSet<String> = schema.def("EventType")["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(rust, enum_values);
}

#[test]
fn every_event_matches_its_schema() {
    let schema = Schema::load();
    let mut failures = Vec::new();
    for (def, ty) in event_defs(&schema) {
        failures.extend(check(&schema, &def, event));
        let root = json!({ "$ref": format!("#/$defs/{def}") });
        for inst in schema.instances(&root, true) {
            let decoded: Event = serde_json::from_value(inst.value).unwrap();
            let got = decoded.event_type().map(|t| t.to_string());
            if got.as_deref() != Some(ty.as_str()) {
                failures.push(format!("{def}: decoded as {got:?}, not {ty}"));
            }
        }
    }
    assert_none(&failures);
}

#[test]
fn every_type_an_event_reaches_matches_its_schema() {
    let schema = Schema::load();
    let mut failures = Vec::new();
    for (def, decode) in decoders() {
        failures.extend(check(&schema, def, decode));
    }
    assert_none(&failures);
}

/// A `$def` an event reaches is checked through its own Rust type, through a
/// union it is a branch of, as an event, or holds nothing to check.
#[test]
fn every_def_an_event_reaches_is_checked() {
    let schema = Schema::load();
    let mut covered: BTreeSet<String> = decoders().iter().map(|(d, _)| d.to_string()).collect();
    covered.extend(event_defs(&schema).into_iter().map(|(d, _)| d));
    for (def, _) in decoders() {
        if let Some(branches) = schema.branches(schema.def(def)) {
            covered.extend(branches.iter().filter_map(ref_name).map(str::to_string));
        }
    }
    let unchecked: Vec<String> = schema
        .reachable("Event")
        .into_iter()
        .filter(|d| !covered.contains(d))
        .filter(|d| {
            let s = schema.def(d);
            let ty = s.get("type").and_then(Value::as_str);
            let plain_string = ty == Some("string") && s.get("pattern").is_none();
            !(plain_string
                || ty == Some("array")
                || (ty.is_none()
                    && ref_name(s).is_none()
                    && s.as_object().is_some_and(Map::is_empty)))
        })
        .collect();
    assert!(
        unchecked.is_empty(),
        "no Rust type checked for {unchecked:?}"
    );
}
