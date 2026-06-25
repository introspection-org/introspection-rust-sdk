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
use uuid::Uuid;

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
    pub project_id: Uuid,
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
    pub project_id: Uuid,
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
    pub project_id: Uuid,
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
    pub runtime: Option<String>,
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
    pub project_id: Uuid,
    pub name: String,
    pub runtime: String,
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
    pub project_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

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
            runtime: Some("customer-agent".to_string()),
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
}
