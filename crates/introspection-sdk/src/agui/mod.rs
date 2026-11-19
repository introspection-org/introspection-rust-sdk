//! AG-UI protocol types ([`Event`]) and Introspection's extensions to them.
//!
//! The task-run stream ([`crate::api::TaskRuns::stream`]) speaks the
//! [AG-UI protocol] natively. This module owns an exact Rust mirror of the
//! `@ag-ui/core` event taxonomy — see [`event`] for why the SDK owns these
//! types rather than depending on a third-party crate — and layers
//! Introspection's own additions on the protocol's sanctioned `CUSTOM`
//! channel (see [`introspection`]).
//!
//! The types are deliberately self-contained (no dependency on the SDK's HTTP
//! layer) so they can be promoted to a standalone crate without changing the
//! public type names.
//!
//! [AG-UI protocol]: https://github.com/ag-ui-protocol/ag-ui

mod de;
pub mod event;
pub mod input;
pub mod message;
pub mod patch;

pub use event::{
    ActivityDeltaEvent, ActivitySnapshotEvent, BaseEvent, CustomEvent, Event, EventType, Interrupt,
    MessagesSnapshotEvent, RawEvent, ReasoningEncryptedValueEvent, ReasoningEncryptedValueSubtype,
    ReasoningEndEvent, ReasoningMessageChunkEvent, ReasoningMessageContentEvent,
    ReasoningMessageEndEvent, ReasoningMessageStartEvent, ReasoningRole, ReasoningStartEvent,
    RunErrorEvent, RunFinishedEvent, RunFinishedOutcome, RunStartedEvent, StateDeltaEvent,
    StateSnapshotEvent, StepFinishedEvent, StepStartedEvent, SubagentErrorEvent,
    SubagentFinishedEvent, SubagentFinishedOutcome, SubagentStartedEvent, TextMessageChunkEvent,
    TextMessageContentEvent, TextMessageEndEvent, TextMessageRole, TextMessageStartEvent,
    TokenUsage, ToolCallArgsEvent, ToolCallChunkEvent, ToolCallEndEvent, ToolCallResultEvent,
    ToolCallStartEvent, ToolRole,
};
pub use input::{Context, ResumeEntry, ResumeStatus, RunAgentInput, Tool};
pub use message::{
    ActivityMessage, AssistantMessage, AudioPart, DataSource, DeveloperMessage, DocumentPart,
    FileSource, FunctionCall, ImagePart, InputContent, InstructionMessage, MediaPart, Message,
    MessageContent, PartSource, ReasoningMessage, SystemMessage, TextPart, ToolCall, ToolCallType,
    ToolMessage, UrlSource, UserMessage, VideoPart,
};
pub use patch::{InvalidJsonPointer, JsonPatch, JsonPatchOperation, JsonPointer};

/// Introspection's extensions to the AG-UI protocol, carried on the `CUSTOM`
/// event channel so they interoperate with any AG-UI consumer and are
/// expressible identically in every language Introspection supports.
pub mod introspection {
    use super::event::{BaseEvent, CustomEvent, Event};
    use serde_json::Value;

    /// `CUSTOM` event name for the opt-in reconnect marker the resumable
    /// task-run stream surfaces on each reconnect / readiness wait.
    pub const RECONNECT_EVENT_NAME: &str = "introspection.reconnect";

    /// Build the `introspection.reconnect` [`Event::Custom`] marker with the
    /// given metadata `value`. Used by the resilient run stream when reconnect
    /// events are opted in; consumers branch on
    /// `Event::Custom(e) if e.name == RECONNECT_EVENT_NAME`.
    pub fn reconnect_event(value: Value) -> Event {
        Event::Custom(CustomEvent {
            base: BaseEvent::default(),
            subagent_run_id: None,
            name: RECONNECT_EVENT_NAME.to_string(),
            value,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reconnect_event_is_a_named_custom_event() {
            let ev = reconnect_event(serde_json::json!({"reason": "severed"}));
            match ev {
                Event::Custom(e) => {
                    assert_eq!(e.name, RECONNECT_EVENT_NAME);
                    assert_eq!(e.value["reason"], "severed");
                }
                other => panic!("expected Custom, got {other:?}"),
            }
        }
    }
}
