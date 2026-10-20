//! AAuth on the Data Plane: the hosted Person Server's approvals and the
//! missions they create, under `/v1/aauth`.
//!
//! **Experimental:** the shapes may change between minor releases.
//!
//! The customer's approval app reads what is waiting and records the
//! approver's answer. Agent sessions never hold `approvals:*`, so an agent
//! cannot decide its own approval.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::PaginationParams;

/// A draft-hardt-aauth-budgets budget: `amount / 10^decimals` in `unit`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Budget {
    pub amount: i64,
    pub unit: String,
    pub decimals: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    Mission,
    AuthToken,
    Completion,
    /// A kind this SDK build predates.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
    /// A status this SDK build predates.
    #[serde(other)]
    Unknown,
}

/// A decision the hosted Person Server waits on.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Approval {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub kind: ApprovalKind,
    pub status: ApprovalStatus,
    pub agent_id: String,
    /// The person the agent acts for.
    pub member_id: Uuid,
    #[serde(default)]
    pub mission_id: Option<Uuid>,
    /// The mission's description, the request's scope, or the completion summary.
    pub description: String,
    #[serde(default)]
    pub resource: Option<String>,
    /// What the resource asks for, on an auth-token approval.
    #[serde(default)]
    pub budget: Option<Budget>,
    /// The resource's own description of the request.
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub decided_budget: Option<Budget>,
    #[serde(default)]
    pub decided_by_member_id: Option<Uuid>,
    #[serde(default)]
    pub decided_at: Option<String>,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
}

/// The approver as your app knows them: the identity a runner takes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ApproverIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anonymous_id: Option<String>,
}

/// Who decided: recorded as a member id, never as free text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    Identity(ApproverIdentity),
    MemberId(Uuid),
}

impl DecidedBy {
    /// The approver by your app's user id for them.
    pub fn user(user_id: impl Into<String>) -> Self {
        Self::Identity(ApproverIdentity {
            user_id: Some(user_id.into()),
            anonymous_id: None,
        })
    }

    pub fn anonymous(anonymous_id: impl Into<String>) -> Self {
        Self::Identity(ApproverIdentity {
            user_id: None,
            anonymous_id: Some(anonymous_id.into()),
        })
    }

    pub fn member(member_id: Uuid) -> Self {
        Self::MemberId(member_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approved,
    Denied,
}

/// `PATCH /v1/aauth/approvals/{id}`: the approver's answer.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovalDecision {
    pub status: Decision,
    /// A mission approval's budget; none approves the mission without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    pub decided_by: DecidedBy,
}

impl ApprovalDecision {
    pub fn approve(decided_by: DecidedBy) -> Self {
        Self {
            status: Decision::Approved,
            budget: None,
            decided_by,
        }
    }

    pub fn deny(decided_by: DecidedBy) -> Self {
        Self {
            status: Decision::Denied,
            budget: None,
            decided_by,
        }
    }

    /// Set the budget a mission approval grants.
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = Some(budget);
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ApprovalListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Repeatable; `pending` lists only approvals that have not expired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Vec<ApprovalStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ApprovalKind>,
    /// Only approvals for this person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    /// Only approvals this member decided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<Uuid>,
}

impl ApprovalListParams {
    /// What is waiting for someone.
    pub fn pending() -> Self {
        Self {
            status: Some(vec![ApprovalStatus::Pending]),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MissionLogEntry {
    pub at: String,
    pub entry: String,
}

/// A mission the Person Server approved, and what it has spent.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Mission {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub s256: String,
    /// The mission's bytes, as hashed and returned to the agent.
    pub mission: String,
    pub agent_id: String,
    pub member_id: Uuid,
    pub resources: Vec<String>,
    #[serde(default)]
    pub budget: Option<Budget>,
    #[serde(default)]
    pub spent: i64,
    #[serde(default)]
    pub log: Vec<MissionLogEntry>,
    pub expires_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MissionListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Only missions for this person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
}

#[derive(Serialize)]
struct NoQuery {}

/// `/v1/aauth/approvals`.
#[derive(Clone)]
pub struct Approvals {
    http: Arc<HttpClient>,
}

impl Approvals {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub fn list(&self, params: &ApprovalListParams) -> Paginator<Approval> {
        Paginator::new(self.http.clone(), "/v1/aauth/approvals", params)
            .expect("ApprovalListParams must serialize to an object")
    }

    pub async fn get(&self, approval_id: Uuid) -> ApiResult<Approval> {
        self.http
            .get_json(&format!("/v1/aauth/approvals/{approval_id}"), &NoQuery {})
            .await
    }

    /// Record the approver's answer. Approving does what the agent asked:
    /// creates the mission, spends the booking, or completes the mission.
    pub async fn decide(
        &self,
        approval_id: Uuid,
        decision: &ApprovalDecision,
    ) -> ApiResult<Approval> {
        self.http
            .patch_json(&format!("/v1/aauth/approvals/{approval_id}"), decision)
            .await
    }
}

/// `/v1/aauth/missions`.
#[derive(Clone)]
pub struct Missions {
    http: Arc<HttpClient>,
}

impl Missions {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub fn list(&self, params: &MissionListParams) -> Paginator<Mission> {
        Paginator::new(self.http.clone(), "/v1/aauth/missions", params)
            .expect("MissionListParams must serialize to an object")
    }

    pub async fn get(&self, mission_id: Uuid) -> ApiResult<Mission> {
        self.http
            .get_json(&format!("/v1/aauth/missions/{mission_id}"), &NoQuery {})
            .await
    }
}

/// AAuth on the Data Plane, as the API nests it under `/v1/aauth`.
#[derive(Clone)]
pub struct Aauth {
    approvals: Approvals,
    missions: Missions,
}

impl Aauth {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            approvals: Approvals::new(http.clone()),
            missions: Missions::new(http),
        }
    }

    pub fn approvals(&self) -> &Approvals {
        &self.approvals
    }

    pub fn missions(&self) -> &Missions {
        &self.missions
    }
}
