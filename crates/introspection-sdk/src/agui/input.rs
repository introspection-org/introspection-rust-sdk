//! `RunAgentInput`, the request that starts a run, as `RUN_STARTED.input`
//! echoes it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::de::keep_null;
use super::message::Message;

/// The input a run started with.
///
/// `tools` and `context` default to empty in `@ag-ui/core`; here an absent
/// list stays `None`, so the input serializes back as it was sent.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    pub thread_id: String,
    pub run_id: String,
    pub protocol_version: Option<String>,
    pub parent_run_id: Option<String>,
    #[serde(default, deserialize_with = "keep_null")]
    pub state: Option<Value>,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<Tool>>,
    pub context: Option<Vec<Context>>,
    #[serde(default, deserialize_with = "keep_null")]
    pub forwarded_props: Option<Value>,
    pub resume: Option<Vec<ResumeEntry>>,
}

/// A tool the caller offers the agent.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    /// The tool's parameters, as a JSON Schema.
    #[serde(default, deserialize_with = "keep_null")]
    pub parameters: Option<Value>,
    pub metadata: Option<Map<String, Value>>,
}

/// A piece of context the caller gives the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub description: String,
    pub value: String,
}

/// The answer to one interrupt, sent to resume a paused run.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    pub interrupt_id: String,
    pub status: ResumeStatus,
    #[serde(default, deserialize_with = "keep_null")]
    pub payload: Option<Value>,
    pub metadata: Option<Map<String, Value>>,
}

/// Whether a [`ResumeEntry`] answers its interrupt or withdraws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResumeStatus {
    Resolved,
    Cancelled,
}
