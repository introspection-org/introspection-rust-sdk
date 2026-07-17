//! Wire types for the DP `/v1/tasks` and `/v1/files` surface.
//!
//! Mirrored from `apps/dataplane-api/introspection_dataplane/models/{task,file}.py`
//! and the Pydantic/TS implementations in
//! `introspection-python-sdk` / `introspection-js-sdk`.
//!
//! Field names are kept on-the-wire (`snake_case`) so the JSON round-trips
//! verbatim — no camelCase translation layer.
//!
//! Unknown fields on responses are silently ignored. Enum values added by
//! the DP after this SDK is compiled deserialize into the `Other`/`Unknown`
//! fallback variant so callers can still read the rest of the record.

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringOrUuid {
    String(String),
    Uuid(Uuid),
}

impl Default for StringOrUuid {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl From<String> for StringOrUuid {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for StringOrUuid {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<Uuid> for StringOrUuid {
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl fmt::Display for StringOrUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => f.write_str(value),
            Self::Uuid(value) => write!(f, "{value}"),
        }
    }
}

impl Serialize for StringOrUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::String(value) => serializer.serialize_str(value),
            Self::Uuid(value) => serializer.serialize_str(&value.to_string()),
        }
    }
}

// ----- enums -----------------------------------------------------------------

/// Mode of a task — mirrors the DP `TaskMode` enum.
///
/// The `Other` variant captures any new mode added by the DP that the SDK
/// has not been recompiled against. The string is the raw on-the-wire value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TaskMode {
    #[default]
    Agent,
    Introspect,
    SystemReview,
    SystemInstrumentation,
    ObservationReview,
    SecurityReview,
    RepoIndex,
    SystemDiscovery,
    Onboarding,
    Heartbeat,
    /// Forward-compatible escape hatch for modes the DP adds later.
    Other(String),
}

impl TaskMode {
    /// On-the-wire string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Agent => "agent",
            Self::Introspect => "introspect",
            Self::SystemReview => "system_review",
            Self::SystemInstrumentation => "system_instrumentation",
            Self::ObservationReview => "observation_review",
            Self::SecurityReview => "security_review",
            Self::RepoIndex => "repo_index",
            Self::SystemDiscovery => "system_discovery",
            Self::Onboarding => "onboarding",
            Self::Heartbeat => "heartbeat",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for TaskMode {
    fn from(s: &str) -> Self {
        match s {
            "agent" => Self::Agent,
            "introspect" => Self::Introspect,
            "system_review" => Self::SystemReview,
            "system_instrumentation" => Self::SystemInstrumentation,
            "observation_review" => Self::ObservationReview,
            "security_review" => Self::SecurityReview,
            "repo_index" => Self::RepoIndex,
            "system_discovery" => Self::SystemDiscovery,
            "onboarding" => Self::Onboarding,
            "heartbeat" => Self::Heartbeat,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for TaskMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Status of a task or run — mirrors the DP `TaskStatus` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Queued,
    Scheduled,
    Running,
    Idle,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for TaskStatus {
    fn from(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "queued" => Self::Queued,
            "scheduled" => Self::Scheduled,
            "running" => Self::Running,
            "idle" => Self::Idle,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for TaskStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Mirrors the DP `FileType` enum.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FileType {
    Upload,
    Filesystem,
    #[default]
    Other,
    /// Forward-compatible escape hatch.
    Unknown(String),
}

impl FileType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Upload => "upload",
            Self::Filesystem => "filesystem",
            Self::Other => "other",
            Self::Unknown(s) => s,
        }
    }
}

impl From<&str> for FileType {
    fn from(s: &str) -> Self {
        match s {
            "upload" => Self::Upload,
            "filesystem" => Self::Filesystem,
            "other" => Self::Other,
            other => Self::Unknown(other.to_string()),
        }
    }
}

impl Serialize for FileType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FileType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

// ----- pagination ------------------------------------------------------------

/// Cursor pagination envelope shared by every DP list endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Paginated<T> {
    pub records: Vec<T>,
    pub count: u64,
    #[serde(default)]
    pub total_count: Option<u64>,
    #[serde(default)]
    pub next: Option<String>,
}

/// Shared cursor-pagination query params for every paginated list
/// endpoint (`?limit`, `?next`, `?include_total`). Embedded by the
/// per-endpoint `*ListParams` structs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PaginationParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
}

// ----- tasks -----------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_index: Option<i64>,
    #[serde(default)]
    pub mode: TaskMode,
    #[serde(default = "default_task_status")]
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_user_message_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
}

fn default_task_status() -> TaskStatus {
    TaskStatus::Pending
}

/// POST /v1/tasks body. All fields optional — the DP fills in defaults.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskCreate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<TaskMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Filters supported by `GET /v1/tasks`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<TaskStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<Vec<TaskMode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_automation_id: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPrompt {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskRunCreate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<TaskPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRun {
    pub id: String,
    pub task_id: Uuid,
    pub status: TaskStatus,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskCreateResponse {
    pub task: Task,
    pub run: TaskRun,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRunResponse {
    pub run: TaskRun,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskCancelResponse {
    pub id: String,
}

// ----- files -----------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct File {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    #[serde(default)]
    pub file_type: FileType,
    pub storage_path: String,
    #[serde(default = "default_mime")]
    pub mime_type: String,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub member_id: Option<Uuid>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub storage_version_id: Option<String>,
}

fn default_mime() -> String {
    "application/octet-stream".to_string()
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileCreateText {
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<FileType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
}

// ----- SSE -------------------------------------------------------------------

/// A single Server-Sent Event frame.
///
/// The DP does not define the event taxonomy — frames are proxied verbatim
/// from the agents-worker, so callers branch on `event` and parse `data`
/// themselves (typically `serde_json::from_str(&ev.data)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

impl SseEvent {
    pub(crate) fn empty() -> Self {
        Self {
            event: "message".to_string(),
            data: String::new(),
            id: None,
            retry: None,
        }
    }
}

// ----- projects (CP) ---------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Project {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ----- repositories (CP) ----------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Repository {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RepositoryListParams {
    pub project_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ----- recipes (CP) ----------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recipe {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub repository_id: Uuid,
    pub name: String,
    pub slug: String,
    pub git_ref: String,
    pub git_commit_sha: String,
    #[serde(default)]
    pub sub_path: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecipeCreate {
    pub project: StringOrUuid,
    pub repository_id: Uuid,
    pub name: String,
    pub git_ref: String,
    pub git_commit_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecipeUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecipeListParams {
    pub project: StringOrUuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit_sha: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ----- runtimes (CP) ---------------------------------------------------------

/// How a Runtime acquires LLM provider credentials at session create —
/// mirrors the CP `RuntimeLlmMode` enum.
///
/// `Managed` (the default) uses Introspection-managed keys; `Byok` uses
/// the project's Endpoint pool. The `Other` variant captures any future
/// mode the CP adds, so callers can still read the rest of the record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RuntimeLlmMode {
    #[default]
    Managed,
    Byok,
    /// Forward-compatible escape hatch.
    Other(String),
}

impl RuntimeLlmMode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Managed => "managed",
            Self::Byok => "byok",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for RuntimeLlmMode {
    fn from(s: &str) -> Self {
        match s {
            "managed" => Self::Managed,
            "byok" => Self::Byok,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for RuntimeLlmMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RuntimeLlmMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Runtime {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub recipe_id: Uuid,
    pub created_by_member_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub allow_hot_swap: bool,
    /// LLM credential source. Defaults to `Managed` when the CP omits
    /// the field (older servers) or sends `"managed"` explicitly.
    #[serde(default)]
    pub llm_mode: RuntimeLlmMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_json: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeCreate {
    pub project: StringOrUuid,
    pub recipe_id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_hot_swap: Option<bool>,
    /// LLM credential source. Defaults to `Managed`. The wire value is
    /// always sent; the server's default (`"managed"`) matches so no
    /// behaviour change for unset callers.
    pub llm_mode: RuntimeLlmMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_json: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_hot_swap: Option<bool>,
    /// PATCH semantics: `None` means "don't change". Set to switch the
    /// runtime between managed credentials and the BYOK pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_mode: Option<RuntimeLlmMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_json: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

// ----- experiments (CP) ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExperimentStatus {
    Draft,
    Running,
    Concluded,
    Cancelled,
    Other(String),
}

impl ExperimentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Concluded => "concluded",
            Self::Cancelled => "cancelled",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for ExperimentStatus {
    fn from(s: &str) -> Self {
        match s {
            "draft" => Self::Draft,
            "running" => Self::Running,
            "concluded" => Self::Concluded,
            "cancelled" => Self::Cancelled,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for ExperimentStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ExperimentStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Arm {
    pub runtime_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arm_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Experiment {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub runtime: String,
    pub status: ExperimentStatus,
    pub arms: Vec<Arm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_json: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_strategy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash_key_fields: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoring_interval_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentCreate {
    pub project: StringOrUuid,
    pub name: String,
    pub runtime: StringOrUuid,
    pub arms: Vec<Arm>,
    pub goal_json: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_key_fields: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoring_interval_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExperimentUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_json: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_key_fields: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoring_interval_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExperimentListParams {
    pub project: StringOrUuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StringOrUuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ExperimentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

// ----- runner ----------------------------------------------------------------

/// Identity captured at session creation. Drives experiment routing
/// (HRW / beta-sample) and rides on the access-token claims so DP can
/// stamp it onto `task.metadata.identity` + forward as
/// `TASK_USER_ID` / `TASK_ANONYMOUS_ID` / `TASK_CONVERSATION_ID`
/// sandbox env.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunnerIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

/// Optional segment.io-style observability payload attached to a
/// [`RunRequest`]. Used by CP for telemetry / experiment-report
/// slicing only — **routing never reads `caller`** (it walks
/// `identity.*` via `hash_key_fields` only). Mixing the two would be
/// a privacy + stability footgun.
///
/// Unknown fields ride along verbatim via [`Self::extra`].
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCaller {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<RunCallerLibrary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<RunCallerPage>,
    /// Any additional fields the caller supplied (app / device / os /
    /// campaign / network / screen / timezone / traits / custom keys)
    /// pass through verbatim.
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCallerLibrary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RunCallerPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// `POST /v1/runtimes/{id}/run` and `/v1/experiments/{id}/run` body.
///
/// User-facing request type. CP infers everything else (runtime_id /
/// experiment_id from the URL; member_id / org_id / project_id from
/// the bearer key).
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<RunnerIdentity>,
    /// Optional segment.io-style observability payload — see
    /// [`RunCaller`]. Echoed on the response's `runtime_context.caller`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller: Option<RunCaller>,
    /// Session lifetime override, max 24h. Default 1h on CP side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u32>,
    /// Recipe pin set by [`crate::resources::RuntimeHandle::pin`]. When
    /// present, CP resolves the runtime row in this runtime's slug whose
    /// `recipe_id == recipe_id` and opens the runner against that row —
    /// the "canary a previous version" flow from the SDK design doc.
    /// Defaults to `None`; the regular `runtime(id).run()` path leaves
    /// it unset and CP uses the row's current `recipe_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_id: Option<Uuid>,
}

/// Resolved context attached to a [`RunnerSpec`] — the runtime / arm /
/// identity CP picked. Surfaced on `runner.context()` for telemetry.
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerContext {
    pub runtime_id: Uuid,
    #[serde(default)]
    pub experiment_id: Option<Uuid>,
    pub recipe_id: Uuid,
    #[serde(default)]
    pub recipe_repository_id: Option<Uuid>,
    #[serde(default)]
    pub recipe_git_ref: Option<String>,
    #[serde(default)]
    pub recipe_git_commit_sha: Option<String>,
    #[serde(default)]
    pub arm_label: Option<String>,
    #[serde(default)]
    pub identity: RunnerIdentity,
    /// Echoed from the request body when supplied — see [`RunCaller`].
    #[serde(default)]
    pub caller: Option<RunCaller>,
}

/// DP deployment the runner should talk to. CP picks per project /
/// deployment and surfaces the resolved endpoint plus its slug + region
/// for telemetry / UX.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunnerDeployment {
    /// DP base URL (e.g. `https://api.gcp01.introspection.dev`).
    pub endpoint: String,
    /// Short deployment slug (e.g. `gcp01`).
    pub slug: String,
    /// Region the deployment lives in (e.g. `us-east-1`).
    pub region: String,
}

/// CP `/run` response — the customer-facing shape.
///
/// Sandbox-internal fields (`credentials` for ext_proc egress, the
/// `bootstrap` repo manifest, DP `limits`, and the any-llm `llm_proxy`
/// descriptor) live on `InternalRunnerSpec` on the CP→DP internal
/// route. They are never returned to customer callers — see the
/// design doc at `introspection-cloud/docs/design/sdk-api.md`.
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerSpec {
    pub session_id: String,
    /// DP deployment the runner should talk to.
    pub deployment: RunnerDeployment,
    /// RS256 session-locator JWT — the customer's only credential.
    /// SDK sends it as `Authorization: Bearer …`; the DP server looks
    /// up the session by JWT claims and reads the materialized access
    /// token from its Redis cache.
    pub session_token: String,
    /// Session lifetime (ISO-8601 string).
    pub expires_at: String,
    /// Resolved runtime context — runtime / arm / recipe / identity /
    /// caller that CP picked. Surfaced on `runner.context()` for
    /// telemetry + UX.
    pub runtime_context: RunnerContext,
}

// ----- telemetry: conversations / events / metrics (DP, runner-scoped) -------
//
// These are Data-Plane telemetry reads — they hang off the [`crate::Runner`]
// (DP bearer + `events:read`), never the CP-scoped top-level client. The
// stores are append-only (`otel_traces` → `/v1/conversations`, `otel_logs` →
// `/v1/events`); all aggregation goes through the bounded `POST /v1/metrics`
// contract. Records carry open telemetry attributes, so the typed structs keep
// a `#[serde(flatten)] extra` bag — unknown fields ride along verbatim rather
// than being dropped.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::api::error::{ApiResult, IntrospectionAPIError};

/// Sort direction for the telemetry list reads. Maps to the wire `direction`
/// query param; defaults to descending (newest-first) like the DP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    /// The DP default — newest-first.
    #[default]
    Desc,
}

impl SortDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

impl From<&str> for SortDirection {
    fn from(s: &str) -> Self {
        match s {
            "asc" => Self::Asc,
            _ => Self::Desc,
        }
    }
}

impl Serialize for SortDirection {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SortDirection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s.as_str()))
    }
}

/// Shared window / ordering / pagination inputs for the telemetry list reads.
///
/// Borrowed view over the ergonomic client params — applied onto the wire
/// query object by [`Window::apply`], which performs the client-side
/// validation (limit range, `lookback` vs `start`/`end` mutual exclusion) and
/// the ergonomic → wire mapping (`order`→`direction`, `start`→`start_date`,
/// `end`→`end_date`, `lookback`→computed `start_date`).
struct Window<'a> {
    limit: Option<u32>,
    next: Option<&'a str>,
    sort: Option<&'a str>,
    order: Option<SortDirection>,
    start: Option<&'a str>,
    end: Option<&'a str>,
    lookback: Option<&'a str>,
    include_total: Option<bool>,
}

impl Window<'_> {
    fn apply(&self, obj: &mut serde_json::Map<String, serde_json::Value>) -> ApiResult<()> {
        if let Some(limit) = self.limit {
            if !(1..=1000).contains(&limit) {
                return Err(IntrospectionAPIError::InvalidConfig(format!(
                    "limit must be between 1 and 1000 (got {limit})"
                )));
            }
            obj.insert("limit".to_string(), limit.into());
        }
        if let Some(next) = self.next {
            obj.insert("next".to_string(), next.into());
        }
        if let Some(sort) = self.sort {
            obj.insert("sort".to_string(), sort.into());
        }
        if let Some(order) = self.order {
            obj.insert("direction".to_string(), order.as_str().into());
        }
        apply_time_window(
            obj,
            self.start,
            self.end,
            self.lookback,
            "start_date",
            "end_date",
        )?;
        if let Some(include_total) = self.include_total {
            obj.insert("include_total".to_string(), include_total.into());
        }
        Ok(())
    }
}

/// Resolve the ergonomic `start` / `end` / `lookback` triple into the wire
/// window keys. `lookback` (relative, e.g. `"24h"`) is **mutually exclusive**
/// with `start`/`end` — the mismatch is rejected client-side *before* any
/// request is sent. When `lookback` is set the start key is computed as
/// `now - lookback`.
fn apply_time_window(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    start: Option<&str>,
    end: Option<&str>,
    lookback: Option<&str>,
    start_key: &str,
    end_key: &str,
) -> ApiResult<()> {
    if lookback.is_some() && (start.is_some() || end.is_some()) {
        return Err(IntrospectionAPIError::InvalidConfig(
            "`lookback` is mutually exclusive with `start`/`end`".to_string(),
        ));
    }
    if let Some(lookback) = lookback {
        let dur = parse_lookback(lookback)?;
        let start_at = SystemTime::now().checked_sub(dur).unwrap_or(UNIX_EPOCH);
        obj.insert(start_key.to_string(), rfc3339_utc(start_at).into());
    } else {
        if let Some(start) = start {
            obj.insert(start_key.to_string(), start.into());
        }
        if let Some(end) = end {
            obj.insert(end_key.to_string(), end.into());
        }
    }
    Ok(())
}

/// Parse a relative lookback like `"24h"`, `"30m"`, `"7d"`, or a compound
/// `"1h30m"` into a [`Duration`]. Units: `s`, `m`, `h`, `d`, `w`.
fn parse_lookback(s: &str) -> ApiResult<Duration> {
    let trimmed = s.trim();
    let invalid = || {
        IntrospectionAPIError::InvalidConfig(format!(
            "invalid lookback `{s}` (expected e.g. `24h`, `30m`, `7d`, `1h30m`)"
        ))
    };
    if trimmed.is_empty() {
        return Err(invalid());
    }
    let mut total: u64 = 0;
    let mut digits = String::new();
    let mut saw_unit = false;
    for c in trimmed.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if digits.is_empty() {
            return Err(invalid());
        }
        let value: u64 = digits.parse().map_err(|_| invalid())?;
        let unit_secs = match c.to_ascii_lowercase() {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            'w' => 604_800,
            _ => return Err(invalid()),
        };
        total = total
            .checked_add(value.checked_mul(unit_secs).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        digits.clear();
        saw_unit = true;
    }
    // A trailing number with no unit (`"24"`) or no units at all is invalid.
    if !digits.is_empty() || !saw_unit {
        return Err(invalid());
    }
    Ok(Duration::from_secs(total))
}

/// Format a [`SystemTime`] as an RFC 3339 / ISO-8601 UTC instant
/// (`YYYY-MM-DDThh:mm:ssZ`) with second precision. Dependency-free (the crate
/// does not pull `chrono`) via Howard Hinnant's civil-from-days algorithm.
fn rfc3339_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert a count of days since the Unix epoch to a `(year, month, day)`
/// civil date. Howard Hinnant's public-domain algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// One conversation record from `GET /v1/conversations` (append-only
/// `otel_traces`). Telemetry attributes are open, so only a few stable
/// identifiers are named; everything else rides along in [`Self::extra`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Conversation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Ergonomic params for `GET /v1/conversations`. `order`/`start`/`end`/
/// `lookback` map to the wire `direction`/`start_date`/`end_date` window (see
/// [`Window`]); `filters` is a passthrough for resource filters that avoids
/// baking the open attribute vocabulary into the SDK.
#[derive(Debug, Clone, Default)]
pub struct ConversationListParams {
    pub limit: Option<u32>,
    pub next: Option<String>,
    pub sort: Option<String>,
    pub order: Option<SortDirection>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub lookback: Option<String>,
    pub include_total: Option<bool>,
    /// Arbitrary resource filters merged verbatim onto the query string.
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl ConversationListParams {
    /// Validate and lower to the wire query object. Returns
    /// [`IntrospectionAPIError::InvalidConfig`] for an out-of-range `limit` or
    /// a `lookback`/`start`/`end` conflict — *before* any request is issued.
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        Window {
            limit: self.limit,
            next: self.next.as_deref(),
            sort: self.sort.as_deref(),
            order: self.order,
            start: self.start.as_deref(),
            end: self.end.as_deref(),
            lookback: self.lookback.as_deref(),
            include_total: self.include_total,
        }
        .apply(&mut obj)?;
        merge_filters(&mut obj, self.filters.as_ref());
        Ok(serde_json::Value::Object(obj))
    }
}

// ----- events: typed six-family read (`GET /v1/events`) ----------------------

/// The six canonical platform event families served by `GET /v1/events`.
///
/// The events read is a **closed, typed set**: `event_name` is required on
/// every list read — exactly one family per request — so a response page is
/// always homogeneous and fully typeable (JSON discriminated member; Arrow
/// typed payload struct column). Legacy verb-suffixed names on historical
/// rows are normalized to these canonical names server-side; anything outside
/// the set (`gen_ai.*`, customer / `track()` events) is not returned and
/// remains aggregable via `POST /v1/metrics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntrospectionEventName {
    #[serde(rename = "introspection.feedback")]
    Feedback,
    #[serde(rename = "introspection.observation")]
    Observation,
    #[serde(rename = "introspection.observation_clustering.run")]
    ObservationClusteringRun,
    #[serde(rename = "introspection.judgement")]
    Judgement,
    #[serde(rename = "introspection.pattern")]
    Pattern,
    #[serde(rename = "introspection.pattern.assignment")]
    PatternAssignment,
}

impl IntrospectionEventName {
    /// On-the-wire dotted family name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Feedback => "introspection.feedback",
            Self::Observation => "introspection.observation",
            Self::ObservationClusteringRun => "introspection.observation_clustering.run",
            Self::Judgement => "introspection.judgement",
            Self::Pattern => "introspection.pattern",
            Self::PatternAssignment => "introspection.pattern.assignment",
        }
    }
}

impl fmt::Display for IntrospectionEventName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Common envelope shared by every typed event family — the queryable
/// surface. `org`/`project` never appear on the wire (tenant scope is implied
/// by auth). The `event_name` discriminator lives on the [`Event`] enum tag,
/// not duplicated here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypedEvent<P> {
    pub id: String,
    /// Per-family semantics: observation → `observed_at`, pattern →
    /// `updated_at` (catalog cursor), stream families → emit/observed time.
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_group_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_git_commit_sha: Option<String>,
    /// Family detail — one of the six `*Payload` types, fixed by the
    /// [`Event`] variant.
    pub payload: P,
}

/// `introspection.observation` payload — one **resolved** observation (the
/// server-side fold: supersession applied, current pattern assignment
/// joined). All fields optional except the `observation_id` identity.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservationPayload {
    pub observation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_refs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces_observation_id: Option<Uuid>,
    /// CURRENT pattern assignment (fold), not the assignment history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// `introspection.pattern` payload — one **folded** catalog row (current
/// state: latest lifecycle action, status, fold timestamps).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatternPayload {
    pub pattern_id: String,
    /// Latest lifecycle action (`created` / `updated` / `retired`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    /// `active` | `retired` (fold).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_detected_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from_pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// `introspection.pattern.assignment` payload — one observation→pattern
/// assignment event (stream family). `observation_id` is the sole identity
/// field; `pattern_id: None` means the observation was explicitly unassigned.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatternAssignmentPayload {
    pub observation_id: Uuid,
    /// Target pattern; `None` = explicitly unassigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// `introspection.observation_clustering.run` payload — one clustering run
/// (stream family).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusteringRunPayload {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `introspection.feedback` payload — mirrors what the SDK `feedback()`
/// surfaces actually emit (`properties.*` / `identity.*` attributes).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedbackPayload {
    /// The feedback label (`"thumbs_up"`, …) — `properties.name`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<String>,
    /// Numeric axis, when present — `properties.value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_id: Option<String>,
    /// Optional **emitted** field (positive/negative/neutral) — never derived
    /// server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<String>,
    /// Response the feedback anchors to —
    /// `gen_ai.request.previous_response_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// `gen_ai.agent.name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// `gen_ai.agent.id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Remaining `properties.*` extras.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
}

/// `introspection.judgement` payload — mirrors the runtime-agent judges
/// emitter (`introspection.judgement.*` / `introspection.judge.*` attributes).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgementPayload {
    pub judgement_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_arm_id: Option<Uuid>,
}

/// Whole-event envelope + typed payload per family.
pub type ObservationEvent = TypedEvent<ObservationPayload>;
pub type PatternEvent = TypedEvent<PatternPayload>;
pub type PatternAssignmentEvent = TypedEvent<PatternAssignmentPayload>;
pub type ClusteringRunEvent = TypedEvent<ClusteringRunPayload>;
pub type FeedbackEvent = TypedEvent<FeedbackPayload>;
pub type JudgementEvent = TypedEvent<JudgementPayload>;

/// One event from `GET /v1/events` — a discriminated union of the six
/// canonical platform families, tagged by the top-level `event_name`.
///
/// Because `event_name` is required on the list read, a page is always
/// homogeneous — every record matches the requested family. The hidden
/// [`Event::Unknown`] fallback tolerates a family this SDK build doesn't know
/// yet (a seventh family added server-side must not fail the whole page);
/// match on it to skip or hand-parse such rows.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "event_name")]
pub enum Event {
    #[serde(rename = "introspection.feedback")]
    Feedback(FeedbackEvent),
    #[serde(rename = "introspection.observation")]
    Observation(ObservationEvent),
    #[serde(rename = "introspection.observation_clustering.run")]
    ObservationClusteringRun(ClusteringRunEvent),
    #[serde(rename = "introspection.judgement")]
    Judgement(JudgementEvent),
    #[serde(rename = "introspection.pattern")]
    Pattern(PatternEvent),
    #[serde(rename = "introspection.pattern.assignment")]
    PatternAssignment(PatternAssignmentEvent),
    /// Forward-compatible escape hatch: a row whose `event_name` this SDK
    /// build doesn't recognise. Carries the raw record verbatim.
    #[serde(untagged)]
    Unknown(serde_json::Value),
}

impl Event {
    /// The canonical family, or `None` for [`Event::Unknown`] rows.
    pub fn event_name(&self) -> Option<IntrospectionEventName> {
        match self {
            Self::Feedback(_) => Some(IntrospectionEventName::Feedback),
            Self::Observation(_) => Some(IntrospectionEventName::Observation),
            Self::ObservationClusteringRun(_) => {
                Some(IntrospectionEventName::ObservationClusteringRun)
            }
            Self::Judgement(_) => Some(IntrospectionEventName::Judgement),
            Self::Pattern(_) => Some(IntrospectionEventName::Pattern),
            Self::PatternAssignment(_) => Some(IntrospectionEventName::PatternAssignment),
            Self::Unknown(_) => None,
        }
    }
}

/// Ergonomic params for `GET /v1/events`. [`Self::event_name`] is
/// **required** (compile-enforced) — exactly one family per request, so the
/// response is always homogeneous. Per-family filters (§4.3 of the telemetry
/// read design — e.g. observation `pattern_id` / `lens` /
/// `include_superseded`, pattern `lens` / `status`) pass through
/// [`Self::filters`] verbatim.
#[derive(Debug, Clone)]
pub struct EventListParams {
    /// The one family to list — required; there is no unfiltered read.
    pub event_name: IntrospectionEventName,
    pub limit: Option<u32>,
    pub next: Option<String>,
    pub sort: Option<String>,
    pub order: Option<SortDirection>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub lookback: Option<String>,
    pub include_total: Option<bool>,
    /// Envelope + family-scoped filters merged verbatim onto the query
    /// string. A filter outside the requested family's allow-map is a 422.
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl EventListParams {
    /// Params for one family with every optional field unset. Combine with
    /// struct-update syntax:
    /// `EventListParams { limit: Some(10), ..EventListParams::new(family) }`.
    pub fn new(event_name: IntrospectionEventName) -> Self {
        Self {
            event_name,
            limit: None,
            next: None,
            sort: None,
            order: None,
            start: None,
            end: None,
            lookback: None,
            include_total: None,
            filters: None,
        }
    }

    /// Validate and lower to the wire query object (see
    /// [`ConversationListParams::to_wire`]).
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        obj.insert("event_name".to_string(), self.event_name.as_str().into());
        Window {
            limit: self.limit,
            next: self.next.as_deref(),
            sort: self.sort.as_deref(),
            order: self.order,
            start: self.start.as_deref(),
            end: self.end.as_deref(),
            lookback: self.lookback.as_deref(),
            include_total: self.include_total,
        }
        .apply(&mut obj)?;
        merge_filters(&mut obj, self.filters.as_ref());
        Ok(serde_json::Value::Object(obj))
    }
}

fn merge_filters(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    filters: Option<&HashMap<String, serde_json::Value>>,
) {
    if let Some(filters) = filters {
        for (k, v) in filters {
            obj.insert(k.clone(), v.clone());
        }
    }
}

// ----- metrics (POST /v1/metrics) --------------------------------------------

/// One `{measure, aggregation}` metric term in a [`MetricsQuery`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    pub measure: String,
    pub aggregation: String,
}

/// One grouping dimension `{field}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub field: String,
}

/// One `{field, operator, value}` filter term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricFilter {
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

/// Time-bucketing dimension — `bins` (count) or `granularity`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeDimension {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bins: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<String>,
}

/// One typed ordering term: metric-index, dimension-field, or time bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTerm {
    #[serde(rename = "type")]
    pub term_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub direction: SortDirection,
}

/// One post-grouping `having` term over an aggregated metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HavingTerm {
    pub metric_index: u32,
    pub operator: String,
    pub value: serde_json::Value,
}

/// Bounded execution config — `row_limit` (default 100, max 10 000) and the
/// grouped-time-series `series_limit`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_limit: Option<u32>,
}

/// Request body for the bounded `POST /v1/metrics` analytics endpoint.
///
/// Ergonomic `start` / `end` / `lookback` map to the wire
/// `from_timestamp` / `to_timestamp` window (same mutual-exclusion validation
/// as the list reads). This is not a general query endpoint — the DP enforces
/// the allow-listed views / measures / dimensions and hard limits.
#[derive(Debug, Clone, Default)]
pub struct MetricsQuery {
    pub view: String,
    pub metrics: Vec<MetricSpec>,
    pub dimensions: Option<Vec<Dimension>>,
    pub filters: Option<Vec<MetricFilter>>,
    pub time_dimension: Option<TimeDimension>,
    pub order_by: Option<Vec<OrderTerm>>,
    pub having: Option<Vec<HavingTerm>>,
    pub config: Option<MetricsConfig>,
    /// Window start (→ `from_timestamp`). Mutually exclusive with `lookback`.
    pub start: Option<String>,
    /// Window end (→ `to_timestamp`). Mutually exclusive with `lookback`.
    pub end: Option<String>,
    /// Relative window (e.g. `"24h"`) → computed `from_timestamp`.
    pub lookback: Option<String>,
}

impl MetricsQuery {
    /// Validate and lower to the wire request body. Rejects a
    /// `lookback`/`start`/`end` conflict client-side before sending.
    pub fn to_wire(&self) -> ApiResult<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        obj.insert("view".to_string(), self.view.clone().into());
        obj.insert(
            "metrics".to_string(),
            serde_json::to_value(&self.metrics).map_err(encode_err)?,
        );
        if let Some(dimensions) = &self.dimensions {
            obj.insert(
                "dimensions".to_string(),
                serde_json::to_value(dimensions).map_err(encode_err)?,
            );
        }
        if let Some(filters) = &self.filters {
            obj.insert(
                "filters".to_string(),
                serde_json::to_value(filters).map_err(encode_err)?,
            );
        }
        if let Some(time_dimension) = &self.time_dimension {
            obj.insert(
                "time_dimension".to_string(),
                serde_json::to_value(time_dimension).map_err(encode_err)?,
            );
        }
        if let Some(order_by) = &self.order_by {
            obj.insert(
                "order_by".to_string(),
                serde_json::to_value(order_by).map_err(encode_err)?,
            );
        }
        if let Some(having) = &self.having {
            obj.insert(
                "having".to_string(),
                serde_json::to_value(having).map_err(encode_err)?,
            );
        }
        if let Some(config) = &self.config {
            obj.insert(
                "config".to_string(),
                serde_json::to_value(config).map_err(encode_err)?,
            );
        }
        apply_time_window(
            &mut obj,
            self.start.as_deref(),
            self.end.as_deref(),
            self.lookback.as_deref(),
            "from_timestamp",
            "to_timestamp",
        )?;
        Ok(serde_json::Value::Object(obj))
    }
}

fn encode_err(e: serde_json::Error) -> IntrospectionAPIError {
    IntrospectionAPIError::Decode(format!("failed to encode metrics query: {e}"))
}

/// Response from `POST /v1/metrics`. The row shape depends on the requested
/// view / metrics / dimensions, so rows stay as `serde_json::Value` and any
/// envelope fields other than `data`/`meta` ride along in [`Self::extra`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetricsResponse {
    #[serde(default)]
    pub data: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_mode_round_trips_known_variants() {
        let mode: TaskMode = serde_json::from_str("\"agent\"").unwrap();
        assert_eq!(mode, TaskMode::Agent);
        assert_eq!(serde_json::to_string(&mode).unwrap(), "\"agent\"");
    }

    #[test]
    fn task_mode_tolerates_unknown_values() {
        let mode: TaskMode = serde_json::from_str("\"brand_new_mode\"").unwrap();
        assert_eq!(mode, TaskMode::Other("brand_new_mode".to_string()));
    }

    #[test]
    fn task_status_round_trips() {
        let s: TaskStatus = serde_json::from_str("\"running\"").unwrap();
        assert_eq!(s, TaskStatus::Running);
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"running\"");
    }

    #[test]
    fn file_type_round_trips() {
        let ft: FileType = serde_json::from_str("\"upload\"").unwrap();
        assert_eq!(ft, FileType::Upload);
        assert_eq!(serde_json::to_string(&ft).unwrap(), "\"upload\"");
    }

    #[test]
    fn paginated_envelope_parses() {
        let payload = r#"{"records":[],"count":0,"total_count":null,"next":null}"#;
        let page: Paginated<serde_json::Value> = serde_json::from_str(payload).unwrap();
        assert_eq!(page.count, 0);
        assert!(page.next.is_none());
    }

    #[test]
    fn runtime_llm_mode_round_trips_known_variants() {
        let m: RuntimeLlmMode = serde_json::from_str("\"managed\"").unwrap();
        assert_eq!(m, RuntimeLlmMode::Managed);
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"managed\"");

        let m: RuntimeLlmMode = serde_json::from_str("\"byok\"").unwrap();
        assert_eq!(m, RuntimeLlmMode::Byok);
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"byok\"");
    }

    #[test]
    fn runtime_llm_mode_tolerates_unknown_values() {
        let m: RuntimeLlmMode = serde_json::from_str("\"brand_new_mode\"").unwrap();
        assert_eq!(m, RuntimeLlmMode::Other("brand_new_mode".to_string()));
    }

    #[test]
    fn runtime_llm_mode_default_is_managed() {
        assert_eq!(RuntimeLlmMode::default(), RuntimeLlmMode::Managed);
    }

    #[test]
    fn runtime_list_params_serialize_runtime_not_name_or_slug() {
        let value = serde_json::to_value(RuntimeListParams {
            runtime: Some("customer-agent".into()),
            ..Default::default()
        })
        .expect("runtime list params serialize");

        assert_eq!(value["runtime"], "customer-agent");
        assert!(value.get("name").is_none());
        assert!(value.get("slug").is_none());
    }

    #[test]
    fn project_list_params_serialize_project_not_name_or_slug() {
        let value = serde_json::to_value(ProjectListParams {
            project: Some("main".to_string()),
            ..Default::default()
        })
        .expect("project list params serialize");

        assert_eq!(value["project"], "main");
        assert!(value.get("name").is_none());
        assert!(value.get("slug").is_none());
    }

    #[test]
    fn runtime_create_accepts_project_string_or_uuid() {
        let uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap();
        let by_slug = serde_json::to_value(RuntimeCreate {
            project: "main".into(),
            name: "agent".to_string(),
            ..Default::default()
        })
        .expect("runtime create serializes slug project");
        let by_uuid = serde_json::to_value(RuntimeCreate {
            project: uuid.into(),
            name: "agent".to_string(),
            ..Default::default()
        })
        .expect("runtime create serializes uuid project");

        assert_eq!(by_slug["project"], "main");
        assert_eq!(by_uuid["project"], uuid.to_string());
    }

    #[test]
    fn sort_direction_defaults_desc_and_round_trips() {
        assert_eq!(SortDirection::default(), SortDirection::Desc);
        assert_eq!(SortDirection::Asc.as_str(), "asc");
        let d: SortDirection = serde_json::from_str("\"asc\"").unwrap();
        assert_eq!(d, SortDirection::Asc);
        assert_eq!(
            serde_json::to_string(&SortDirection::Desc).unwrap(),
            "\"desc\""
        );
    }

    #[test]
    fn conversation_params_map_ergonomic_names_to_wire() {
        let wire = ConversationListParams {
            limit: Some(50),
            order: Some(SortDirection::Asc),
            start: Some("2026-01-01T00:00:00Z".into()),
            end: Some("2026-02-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["limit"], 50);
        assert_eq!(wire["direction"], "asc");
        assert_eq!(wire["start_date"], "2026-01-01T00:00:00Z");
        assert_eq!(wire["end_date"], "2026-02-01T00:00:00Z");
        // Ergonomic aliases never leak onto the wire.
        assert!(wire.get("order").is_none());
        assert!(wire.get("start").is_none());
        assert!(wire.get("end").is_none());
    }

    #[test]
    fn lookback_is_mutually_exclusive_with_start_end() {
        let err = ConversationListParams {
            lookback: Some("24h".into()),
            start: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap_err();
        assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn lookback_computes_start_date_and_omits_end() {
        let wire = EventListParams {
            lookback: Some("24h".into()),
            ..EventListParams::new(IntrospectionEventName::Feedback)
        }
        .to_wire()
        .unwrap();
        let start = wire["start_date"].as_str().unwrap();
        // RFC3339 UTC, second precision.
        assert!(start.ends_with('Z'));
        assert_eq!(start.len(), 20);
        assert!(wire.get("end_date").is_none());
    }

    #[test]
    fn parse_lookback_supports_compound_units() {
        assert_eq!(parse_lookback("24h").unwrap(), Duration::from_secs(86_400));
        assert_eq!(parse_lookback("30m").unwrap(), Duration::from_secs(1_800));
        assert_eq!(parse_lookback("7d").unwrap(), Duration::from_secs(604_800));
        assert_eq!(parse_lookback("1h30m").unwrap(), Duration::from_secs(5_400));
        assert!(parse_lookback("24").is_err());
        assert!(parse_lookback("").is_err());
        assert!(parse_lookback("10y").is_err());
    }

    #[test]
    fn rfc3339_utc_formats_known_epoch() {
        // 1_700_000_000 == 2023-11-14T22:13:20Z
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(rfc3339_utc(t), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn limit_out_of_range_is_rejected() {
        assert!(ConversationListParams {
            limit: Some(0),
            ..Default::default()
        }
        .to_wire()
        .is_err());
        assert!(ConversationListParams {
            limit: Some(1001),
            ..Default::default()
        }
        .to_wire()
        .is_err());
    }

    #[test]
    fn event_params_require_event_name_and_pass_family_filters() {
        // `event_name` is a required (compile-enforced) field: there is no
        // `Default` impl and no way to build the params without a family.
        let wire = EventListParams {
            filters: Some(HashMap::from([
                ("pattern_id".to_string(), json!("pat_1")),
                ("include_superseded".to_string(), json!(true)),
            ])),
            ..EventListParams::new(IntrospectionEventName::Observation)
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["event_name"], "introspection.observation");
        // Family-scoped filters pass through verbatim.
        assert_eq!(wire["pattern_id"], "pat_1");
        assert_eq!(wire["include_superseded"], true);
        // The retired grain-era params never reach the wire.
        assert!(wire.get("grain").is_none());
        assert!(wire.get("include").is_none());
        assert!(wire.get("event_name_prefix").is_none());
        assert!(wire.get("q").is_none());
        assert!(wire.get("q_regex").is_none());
    }

    #[test]
    fn introspection_event_name_serde_uses_dotted_names() {
        for (variant, wire) in [
            (IntrospectionEventName::Feedback, "introspection.feedback"),
            (
                IntrospectionEventName::Observation,
                "introspection.observation",
            ),
            (
                IntrospectionEventName::ObservationClusteringRun,
                "introspection.observation_clustering.run",
            ),
            (IntrospectionEventName::Judgement, "introspection.judgement"),
            (IntrospectionEventName::Pattern, "introspection.pattern"),
            (
                IntrospectionEventName::PatternAssignment,
                "introspection.pattern.assignment",
            ),
        ] {
            assert_eq!(variant.as_str(), wire);
            assert_eq!(serde_json::to_value(variant).unwrap(), json!(wire));
            let back: IntrospectionEventName = serde_json::from_value(json!(wire)).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn observation_event_round_trips_typed_payload() {
        let raw = json!({
            "id": "evt_1",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.observation",
            "conversation_id": "conv_1",
            "runtime_group_id": "00000000-0000-0000-0000-00000000cccc",
            "payload": {
                "observation_id": "00000000-0000-0000-0000-000000000042",
                "lens": "user_frustration",
                "summary": "User repeated the question",
                "severity": "high",
                "confidence": 0.92,
                "pattern_id": "pat_7",
                "assignment_score": 0.81,
            },
        });
        let event: Event = serde_json::from_value(raw.clone()).unwrap();
        let Event::Observation(obs) = &event else {
            panic!("expected Observation, got {event:?}");
        };
        assert_eq!(obs.id, "evt_1");
        assert_eq!(obs.conversation_id.as_deref(), Some("conv_1"));
        assert_eq!(
            obs.payload.observation_id.to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert_eq!(obs.payload.lens.as_deref(), Some("user_frustration"));
        assert_eq!(obs.payload.confidence, Some(0.92));
        // The current-assignment fold fields ride on the payload.
        assert_eq!(obs.payload.pattern_id.as_deref(), Some("pat_7"));
        assert_eq!(
            event.event_name(),
            Some(IntrospectionEventName::Observation)
        );
        // Serialize → the top-level discriminator is re-emitted.
        let back = serde_json::to_value(&event).unwrap();
        assert_eq!(back["event_name"], "introspection.observation");
        assert_eq!(back["payload"]["pattern_id"], "pat_7");
    }

    #[test]
    fn pattern_event_round_trips_fold_fields() {
        let raw = json!({
            "id": "evt_2",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.pattern",
            "payload": {
                "pattern_id": "pat_7",
                "action": "created",
                "name": "Repeated question",
                "status": "active",
                "created_at": "2026-06-01T00:00:00Z",
                "last_detected_at": "2026-07-01T00:00:00Z",
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::Pattern(pat) = &event else {
            panic!("expected Pattern, got {event:?}");
        };
        assert_eq!(pat.payload.pattern_id, "pat_7");
        // Legacy `introspection.pattern.created` rows normalize server-side
        // to the canonical family with `payload.action = "created"`.
        assert_eq!(pat.payload.action.as_deref(), Some("created"));
        assert_eq!(pat.payload.status.as_deref(), Some("active"));
        assert_eq!(event.event_name(), Some(IntrospectionEventName::Pattern));
    }

    #[test]
    fn feedback_event_round_trips_typed_payload() {
        let raw = json!({
            "id": "evt_3",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.feedback",
            "payload": {
                "name": "thumbs_up",
                "comments": "great answer",
                "value": 1.0,
                "user_id": "user_9",
                "sentiment": "positive",
                "previous_response_id": "resp_42",
                "agent_name": "support-agent",
                "agent_id": "agent_7",
                "properties": {"surface": "chat"},
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::Feedback(fb) = &event else {
            panic!("expected Feedback, got {event:?}");
        };
        assert_eq!(fb.payload.name, "thumbs_up");
        assert_eq!(fb.payload.value, Some(1.0));
        assert_eq!(fb.payload.sentiment.as_deref(), Some("positive"));
        // gen_ai anchoring fields (cloud phase-1 final models).
        assert_eq!(fb.payload.previous_response_id.as_deref(), Some("resp_42"));
        assert_eq!(fb.payload.agent_name.as_deref(), Some("support-agent"));
        assert_eq!(fb.payload.agent_id.as_deref(), Some("agent_7"));
        assert_eq!(
            fb.payload.properties.as_ref().unwrap()["surface"],
            json!("chat")
        );
        let back = serde_json::to_value(&event).unwrap();
        assert_eq!(back["payload"]["previous_response_id"], "resp_42");
        assert_eq!(back["payload"]["agent_name"], "support-agent");
        assert_eq!(back["payload"]["agent_id"], "agent_7");
    }

    #[test]
    fn pattern_assignment_event_tolerates_explicit_unassignment() {
        // `pattern_id: null` = explicitly unassigned — still the typed
        // variant (observation_id alone is identity), never Unknown.
        let raw = json!({
            "id": "evt_7",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.pattern.assignment",
            "payload": {
                "observation_id": "00000000-0000-0000-0000-000000000042",
                "pattern_id": null,
                "method": "manual",
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::PatternAssignment(pa) = &event else {
            panic!("expected PatternAssignment, got {event:?}");
        };
        assert_eq!(
            pa.payload.observation_id.to_string(),
            "00000000-0000-0000-0000-000000000042"
        );
        assert!(pa.payload.pattern_id.is_none());
        assert_eq!(pa.payload.method.as_deref(), Some("manual"));

        // Assigned rows still carry the pattern.
        let assigned = json!({
            "id": "evt_8",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.pattern.assignment",
            "payload": {
                "observation_id": "00000000-0000-0000-0000-000000000042",
                "pattern_id": "pat_7",
                "score": 0.8,
            },
        });
        let event: Event = serde_json::from_value(assigned).unwrap();
        let Event::PatternAssignment(pa) = &event else {
            panic!("expected PatternAssignment, got {event:?}");
        };
        assert_eq!(pa.payload.pattern_id.as_deref(), Some("pat_7"));
        assert_eq!(pa.payload.score, Some(0.8));
    }

    #[test]
    fn judgement_event_round_trips_typed_payload() {
        let raw = json!({
            "id": "evt_4",
            "timestamp": "2026-07-01T00:00:00Z",
            "event_name": "introspection.judgement",
            "payload": {
                "judgement_id": "jm_1",
                "judge_id": "judge_1",
                "result": "pass",
                "definition_hash": "abc123",
                "contract_version": "1",
                "sequence_hash": "def456",
                "experiment_arm_id": "00000000-0000-0000-0000-00000000eeee",
            },
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        let Event::Judgement(j) = &event else {
            panic!("expected Judgement, got {event:?}");
        };
        assert_eq!(j.payload.judgement_id, "jm_1");
        assert_eq!(j.payload.result.as_deref(), Some("pass"));
        assert_eq!(
            j.payload.experiment_arm_id.unwrap().to_string(),
            "00000000-0000-0000-0000-00000000eeee"
        );
    }

    #[test]
    fn unknown_event_family_does_not_fail_the_page() {
        // A seventh family added server-side after this SDK build must not
        // fail the whole page — it falls into `Event::Unknown` verbatim.
        let payload = json!({
            "records": [
                {
                    "id": "evt_5",
                    "timestamp": "2026-07-01T00:00:00Z",
                    "event_name": "introspection.brand_new.family",
                    "payload": {"anything": true},
                },
                {
                    "id": "evt_6",
                    "timestamp": "2026-07-01T00:00:00Z",
                    "event_name": "introspection.feedback",
                    "payload": {"name": "thumbs_down"},
                },
            ],
            "count": 2,
            "next": null,
        });
        let page: Paginated<Event> = serde_json::from_value(payload).unwrap();
        assert_eq!(page.records.len(), 2);
        let Event::Unknown(raw) = &page.records[0] else {
            panic!("expected Unknown, got {:?}", page.records[0]);
        };
        assert_eq!(raw["event_name"], "introspection.brand_new.family");
        assert!(page.records[0].event_name().is_none());
        assert!(matches!(page.records[1], Event::Feedback(_)));
    }

    #[test]
    fn metrics_query_maps_window_to_from_to_timestamp() {
        let wire = MetricsQuery {
            view: "spans".into(),
            metrics: vec![MetricSpec {
                measure: "duration_ns".into(),
                aggregation: "p95".into(),
            }],
            start: Some("2026-06-01T00:00:00Z".into()),
            end: Some("2026-07-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap();
        assert_eq!(wire["view"], "spans");
        assert_eq!(wire["metrics"][0]["aggregation"], "p95");
        assert_eq!(wire["from_timestamp"], "2026-06-01T00:00:00Z");
        assert_eq!(wire["to_timestamp"], "2026-07-01T00:00:00Z");
    }

    #[test]
    fn metrics_query_rejects_lookback_with_start() {
        let err = MetricsQuery {
            view: "spans".into(),
            metrics: vec![],
            lookback: Some("7d".into()),
            start: Some("2026-06-01T00:00:00Z".into()),
            ..Default::default()
        }
        .to_wire()
        .unwrap_err();
        assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
    }
}
