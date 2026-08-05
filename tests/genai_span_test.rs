//! The GenAI span — the object `/v1/conversations` returns.
//!
//! Pure serialization-contract tests: nothing here crosses a process or
//! network boundary, because the thing under test *is* the shape.
//!
//! Two properties carry most of the weight, because they are the two the flat
//! representation got wrong:
//!
//! - **Nothing serializes as null.** An absent value is an absent key.
//! - **Nothing is dropped.** The attribute tree is open, so an attribute no
//!   model declared still arrives and still round-trips.

use introspection_sdk::api::genai_span::{GenAiSpan, GenAiSpanList};
use serde_json::json;

/// Every path in a serialized payload whose value is `null`.
fn nulls(value: &serde_json::Value, path: &str) -> Vec<String> {
    let mut found = Vec::new();
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                if child.is_null() {
                    found.push(child_path);
                } else {
                    found.extend(nulls(child, &child_path));
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                found.extend(nulls(child, &format!("{path}[{index}]")));
            }
        }
        _ => {}
    }
    found
}

/// The worked example from `conversations-genai-representation.md` §3.
fn full_span() -> serde_json::Value {
    json!({
        "trace_id": "8f0efe5966587e51364046b44b5d0029",
        "span_id": "caa8ff6f77084ded",
        "parent_span_id": "623224d3c1b1a99b",
        "name": "chat claude-sonnet-4-6",
        "kind": "INTERNAL",
        "start_time": "2026-08-04T22:14:34.506000Z",
        "end_time": "2026-08-04T22:14:37.482470Z",
        "duration_ns": 2_976_470_577i64,
        "status": {"code": "Unset"},
        "resource": {"service": {"name": "coding-agent"}},
        "attributes": {
            "gen_ai": {
                "operation": {"name": "chat"},
                "provider": {"name": "anthropic"},
                "conversation": {"id": "019fcee7-4fcc-7793-a1ce-8047b3518303"},
                "agent": {"id": "agent:019fced4", "name": "agent"},
                "request": {"model": "claude-sonnet-4-6"},
                "response": {"model": "claude-sonnet-4-6", "id": "msg_011Cdi"},
                "usage": {
                    "input_tokens": 1527,
                    "output_tokens": 45,
                    "cache_creation": {"input_tokens": 1524}
                },
                "input": {"messages": [
                    {"role": "user", "parts": [{"type": "text", "content": "hey"}]}
                ]},
                "output": {"messages": [
                    {
                        "role": "assistant",
                        "parts": [{"type": "text", "content": "hi"}],
                        "finish_reason": "stop"
                    }
                ]}
            },
            "introspection": {
                "member": {"id": "019fbe0c"},
                "environment": "production",
                "cost_usd": 0.0098,
                "conversation": {"position": 1, "is_new": true}
            }
        }
    })
}

fn bare_span() -> serde_json::Value {
    json!({"trace_id": "t", "start_time": "2026-08-04T22:14:34Z"})
}

fn parse(value: serde_json::Value) -> GenAiSpan {
    serde_json::from_value(value).expect("span parses")
}

// ----- the attributes keep the names the span was written with ---------------

#[test]
fn the_tree_is_addressed_by_convention_name() {
    // The whole point: a reader who knows the semantic conventions can find a
    // value without learning a private dialect for it.
    let span = parse(full_span());
    let gen_ai = span.attributes.gen_ai.as_ref().unwrap();

    assert_eq!(
        gen_ai.operation.as_ref().unwrap().name.as_deref(),
        Some("chat")
    );
    assert_eq!(
        gen_ai.provider.as_ref().unwrap().name.as_deref(),
        Some("anthropic")
    );
    assert_eq!(
        gen_ai.request.as_ref().unwrap().model.as_deref(),
        Some("claude-sonnet-4-6")
    );
    assert_eq!(gen_ai.usage.as_ref().unwrap().input_tokens, Some(1527));
    assert_eq!(
        gen_ai.response.as_ref().unwrap().id.as_deref(),
        Some("msg_011Cdi")
    );
}

#[test]
fn cache_tokens_nest_the_way_the_convention_nests_them() {
    // `gen_ai.usage.cache_creation.input_tokens` is a nested count, not a flat
    // `cache_creation_input_tokens` — our local extension before the
    // conventions adopted it, and the nesting is the adopted spelling.
    let span = parse(full_span());

    assert_eq!(
        span.attributes
            .gen_ai
            .unwrap()
            .usage
            .unwrap()
            .cache_creation
            .unwrap()
            .input_tokens,
        Some(1524)
    );
}

#[test]
fn introspection_attributes_sit_beside_gen_ai_not_inside_it() {
    let span = parse(full_span());
    let introspection = span.attributes.introspection.unwrap();

    assert_eq!(
        introspection.member.unwrap().id.as_deref(),
        Some("019fbe0c")
    );
    assert_eq!(introspection.environment.as_deref(), Some("production"));
    // Cost is not in the GenAI conventions at all, which is why it is here and
    // not under `gen_ai.usage`.
    assert_eq!(introspection.cost_usd, Some(0.0098));
    assert_eq!(introspection.conversation.unwrap().position, Some(1));
}

#[test]
fn cost_is_typed_where_the_server_emits_it() {
    // Placement needs its own test precisely because the tree is open: had
    // this been modelled under `conversation` — where an early draft of the
    // design doc put it — nothing would fail. The value would land in `extra`
    // and typed access would quietly return `None`. That is the failure mode
    // an open tree buys you.
    let span = parse(json!({
        "trace_id": "t",
        "start_time": "2026-08-04T22:14:34Z",
        "attributes": {"introspection": {
            "cost_usd": 0.0098,
            "conversation": {"trace_count": 3}
        }}
    }));

    let introspection = span.attributes.introspection.unwrap();
    assert_eq!(introspection.cost_usd, Some(0.0098));
    // The counts genuinely describe the conversation, so they nest; cost
    // describes whichever object you are holding, so it does not.
    assert_eq!(introspection.conversation.unwrap().trace_count, Some(3));
}

#[test]
fn the_summary_shape_parses_as_the_same_type() {
    // §3a: a conversation summary is the same envelope carrying the latest
    // turn plus rollups. If this needed a second type the "one parser" claim
    // would be false.
    let summary = parse(json!({
        "trace_id": "8f0efe5966587e51364046b44b5d0029",
        "start_time": "2026-08-04T22:14:34.462000Z",
        "end_time": "2026-08-04T22:14:37.488000Z",
        "duration_ns": 3_026_835_672i64,
        "status": {"code": "Ok"},
        "resource": {"service": {"name": "coding-agent"}},
        "attributes": {
            "gen_ai": {
                "conversation": {"id": "019fcee7"},
                "agent": {"name": "agent"},
                "request": {"model": "claude-sonnet-4-6"},
                "usage": {"input_tokens": 1527, "output_tokens": 45},
                "input": {"messages": [
                    {"role": "user", "parts": [{"type": "text", "content": "hey"}]}
                ]},
                "output": {"messages": [
                    {"role": "assistant", "parts": [{"type": "text", "content": "hi"}]}
                ]}
            },
            "introspection": {
                "org": {"id": "019fbe0c"},
                "project": {"id": "019fce34"},
                "environment": "production",
                "runtime": {"id": "019fced4-a", "group_id": "019fced4-b"},
                "recipe": {"git_commit_sha": "df7339af"},
                "cost_usd": 0.0098,
                "conversation": {
                    "trace_count": 3,
                    "span_count": 12,
                    "tool_use_count": 4,
                    "failed_tool_use_count": 0,
                    "has_errors": false
                }
            }
        }
    }));

    let introspection = summary.attributes.introspection.as_ref().unwrap();
    let runtime = introspection.runtime.as_ref().unwrap();
    assert_eq!(runtime.id.as_deref(), Some("019fced4-a"));
    assert_eq!(runtime.group_id.as_deref(), Some("019fced4-b"));
    assert_eq!(
        introspection
            .recipe
            .as_ref()
            .unwrap()
            .git_commit_sha
            .as_deref(),
        Some("df7339af")
    );
    // Rollups with no semantic-convention name live under `introspection`;
    // token totals keep the `gen_ai.usage.*` name they are summable under.
    let conversation = introspection.conversation.as_ref().unwrap();
    assert_eq!(conversation.trace_count, Some(3));
    assert_eq!(conversation.has_errors, Some(false));
    assert_eq!(
        summary
            .attributes
            .gen_ai
            .as_ref()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .output_tokens,
        Some(45)
    );
    // A preview is a one-element message list, not a different schema.
    assert_eq!(summary.input_messages().len(), 1);
}

// ----- null is never serialized ---------------------------------------------

#[test]
fn a_fully_populated_span_serializes_no_nulls() {
    let dumped = serde_json::to_value(parse(full_span())).unwrap();

    assert_eq!(nulls(&dumped, ""), Vec::<String>::new());
}

#[test]
fn a_minimal_span_serializes_no_nulls() {
    // The case that matters more: most spans are mostly empty, and the flat
    // representation rendered that emptiness as ~30 explicit nulls.
    let dumped = serde_json::to_value(parse(bare_span())).unwrap();

    assert_eq!(nulls(&dumped, ""), Vec::<String>::new());
}

#[test]
fn absent_optional_fields_are_absent_keys_not_null_values() {
    let dumped = serde_json::to_value(parse(bare_span())).unwrap();
    let object = dumped.as_object().unwrap();

    assert!(!object.contains_key("parent_span_id"));
    assert!(!object.contains_key("end_time"));
    assert!(!object.contains_key("status"));
    // An empty attribute tree is an absent key too, not `{}`.
    assert!(!object.contains_key("attributes"));
    assert_eq!(object.len(), 2);
}

#[test]
fn message_parts_omit_their_own_nulls_too() {
    // The leak that is easy to miss: the envelope can omit nulls perfectly
    // while the message models nested four levels down still emit theirs.
    let dumped = serde_json::to_value(parse(full_span())).unwrap();
    let message = &dumped["attributes"]["gen_ai"]["input"]["messages"][0];

    assert_eq!(nulls(message, ""), Vec::<String>::new());
    assert!(message.get("finish_reason").is_none());
}

#[test]
fn a_real_zero_is_kept() {
    // Omitting nulls must not become omitting falsey values: a turn that
    // genuinely produced no output tokens is a fact, not an absence.
    let span = parse(json!({
        "trace_id": "t",
        "start_time": "2026-08-04T22:14:34Z",
        "attributes": {"gen_ai": {"usage": {"output_tokens": 0}}}
    }));

    let dumped = serde_json::to_value(span).unwrap();
    assert_eq!(
        dumped["attributes"]["gen_ai"]["usage"],
        json!({"output_tokens": 0})
    );
}

#[test]
fn an_empty_page_serializes_no_nulls() {
    let page: GenAiSpanList =
        serde_json::from_value(json!({"object": "list", "data": []})).unwrap();

    let dumped = serde_json::to_value(&page).unwrap();
    assert!(page.data.is_empty());
    assert_eq!(nulls(&dumped, ""), Vec::<String>::new());
}

// ----- the tree is open ------------------------------------------------------

#[test]
fn an_undeclared_gen_ai_attribute_survives() {
    // The lossiness fix. A customer attribute nobody modelled must arrive, or
    // this representation has the same defect as the one it replaces.
    let span = parse(json!({
        "trace_id": "t",
        "start_time": "2026-08-04T22:14:34Z",
        "attributes": {"gen_ai": {"vendor_specific": {"nested": "kept"}}}
    }));

    assert_eq!(
        span.attributes.gen_ai.unwrap().extra.get("vendor_specific"),
        Some(&json!({"nested": "kept"}))
    );
}

#[test]
fn an_entirely_unknown_attribute_family_survives() {
    let span = parse(json!({
        "trace_id": "t",
        "start_time": "2026-08-04T22:14:34Z",
        "attributes": {"acme": {"tenant": "x"}}
    }));

    assert_eq!(
        span.attributes.extra.get("acme"),
        Some(&json!({"tenant": "x"}))
    );
}

#[test]
fn unknown_attributes_round_trip_through_serialization() {
    // Surviving deserialization is not enough — it has to come back out, at
    // every depth of the tree and at the top level of the span.
    let raw = json!({
        "trace_id": "t",
        "start_time": "2026-08-04T22:14:34Z",
        "future_top_level": "kept",
        "attributes": {
            "gen_ai": {
                "vendor_specific": "kept",
                "usage": {"reasoning_tokens": 12},
                "request": {"model": "m", "thinking_budget": 4096}
            },
            "introspection": {"tenant_tier": "max"},
            "acme": {"a": 1}
        }
    });

    let dumped = serde_json::to_value(parse(raw.clone())).unwrap();

    assert_eq!(dumped, raw);
}

#[test]
fn an_unmodelled_message_part_survives_inside_the_tree() {
    // The open tree is only as open as its deepest typed node. A part type
    // this build has never seen must neither fail the parse nor come back out
    // with its payload missing.
    let raw = json!({
        "trace_id": "t",
        "start_time": "2026-08-04T22:14:34Z",
        "attributes": {"gen_ai": {"input": {"messages": [
            {"role": "user", "parts": [
                {"type": "text", "content": "look at this"},
                {"type": "uri", "modality": "image", "uri": "https://x/y.png"}
            ]}
        ]}}}
    });

    let span = parse(raw.clone());

    assert_eq!(span.input_messages()[0].parts.len(), 2);
    assert_eq!(serde_json::to_value(span).unwrap(), raw);
}

#[test]
fn a_full_span_round_trips_byte_for_byte() {
    // The strongest statement of "nothing is dropped": the whole §3 example
    // out the other side unchanged, keys and values.
    let raw = full_span();

    assert_eq!(serde_json::to_value(parse(raw.clone())).unwrap(), raw);
}

// ----- accessors -------------------------------------------------------------

#[test]
fn messages_are_reachable_without_walking_the_tree() {
    let span = parse(full_span());

    assert_eq!(
        span.conversation_id(),
        Some("019fcee7-4fcc-7793-a1ce-8047b3518303")
    );
    assert_eq!(span.request_model(), Some("claude-sonnet-4-6"));
    assert_eq!(span.input_messages()[0].role, "user");
    assert_eq!(
        span.output_messages()[0].finish_reason.as_deref(),
        Some("stop")
    );
}

#[test]
fn accessors_return_empty_rather_than_panicking_on_a_bare_span() {
    // A tool span carries no messages at all; reaching for them is normal and
    // must not require four levels of Option handling at the call site.
    let span = parse(bare_span());

    assert!(span.input_messages().is_empty());
    assert!(span.output_messages().is_empty());
    assert_eq!(span.conversation_id(), None);
    assert_eq!(span.operation_name(), None);
}

// ----- one shape, several message depths -------------------------------------

#[test]
fn the_same_type_parses_a_preview_and_a_full_history() {
    // The list read sends one message; the item detail read sends the whole
    // conversation so it can be resumed. Same type either way.
    for count in [1usize, 12] {
        let messages: Vec<serde_json::Value> = (0..count)
            .map(|i| json!({"role": "user", "parts": [{"type": "text", "content": format!("m{i}")}]}))
            .collect();
        let span = parse(json!({
            "trace_id": "t",
            "start_time": "2026-08-04T22:14:34Z",
            "attributes": {"gen_ai": {"input": {"messages": messages}}}
        }));

        assert_eq!(span.input_messages().len(), count);
    }
}

#[test]
fn the_list_envelope_keeps_cursor_pagination() {
    let page: GenAiSpanList = serde_json::from_value(json!({
        "object": "list",
        "data": [full_span()],
        "first_id": "caa8ff6f77084ded",
        "has_more": true,
        "next": "cursor-abc"
    }))
    .unwrap();

    assert!(page.has_more);
    assert_eq!(page.next.as_deref(), Some("cursor-abc"));
    assert_eq!(page.data[0].operation_name(), Some("chat"));
}
