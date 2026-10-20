use aauth_protocol::{Budget, Jwk};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Someone this Person Server speaks for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Person {
    /// Stable within this Person Server; never shown to a resource, which
    /// gets a directed `sub` instead.
    pub id: String,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// An email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The person's organisation, per OpenID Connect Enterprise; carried as `tenant`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// Anything else the directory knows, kept with the person.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Person {
    /// A person known only by `id`.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Default::default()
        }
    }
}

/// The agent a request came from, once its signature and agent token verify.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    /// `aauth:local@domain`.
    pub id: String,
    /// Its agent provider.
    pub provider: String,
    /// JWK thumbprint of its signing key.
    pub jkt: String,
    /// Its signing key.
    pub jwk: Jwk,
    /// When its agent token expires, in seconds since the epoch.
    pub exp: i64,
}

/// What an approval decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalKind {
    /// A mission the agent proposed.
    Mission,
    /// An auth token the mission's budget does not cover.
    AuthToken,
    /// The agent says the mission is done; the person accepts.
    Completion,
}

/// Where an approval stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalStatus {
    /// Nobody has decided.
    Pending,
    /// Approved. A mission's tokens are minted on each poll while it is
    /// live; an auth token is minted on the first, which collects it.
    Approved,
    /// Denied, by the approver or withdrawn by the agent.
    Denied,
    /// Approved, and its auth token handed to the agent: an auth token is
    /// collected once, so one approval spends once.
    Collected,
}

/// A decision this Person Server waits on. Whoever operates it makes the decision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    /// Its identifier, in the pending URL.
    pub id: String,
    /// What it decides.
    pub kind: ApprovalKind,
    /// Where it stands.
    pub status: ApprovalStatus,
    /// The agent that asked.
    pub agent: Agent,
    /// Who the agent acts for.
    pub person: Person,
    /// Who should decide: the directory's answer, else the person.
    pub approver: Person,
    /// The mission's description, the request's scope, or the completion summary.
    pub description: String,
    /// The resource's own one-line description of an auth token request (its R3 proposal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The mission it is under, or the one approving it created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_s256: Option<String>,
    /// The resource an auth token request is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// What an auth token request costs, from the resource token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// For a mission: the resources it would cover. For an auth token: the claims it is minted with.
    #[serde(default)]
    pub grant: Map<String, Value>,
    /// Approving a mission sets its budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_budget: Option<Budget>,
    /// Who decided, in the operator's terms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    /// When it was decided, in seconds since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<i64>,
    /// When it was asked, in seconds since the epoch.
    pub created_at: i64,
    /// When it expires undecided.
    pub expires_at: i64,
    /// The tenant it was asked under.
    pub tenant: String,
}

/// An update the agent logged against a mission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// When, in seconds since the epoch.
    pub at: i64,
    /// What the agent said.
    pub entry: String,
}

/// An approved mission.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mission {
    /// Its hash: the `mission_s256` its tokens carry.
    pub s256: String,
    /// The approved mission's bytes, as hashed and returned to the agent.
    pub mission: String,
    /// The agent it was approved for.
    pub agent_id: String,
    /// The person it acts for.
    pub person: Person,
    /// The resources it covers.
    pub resources: Vec<String>,
    /// What it may spend, when the approver set a budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// What it has spent, in the budget's unit and scale.
    pub spent: u64,
    /// The agent's updates.
    #[serde(default)]
    pub log: Vec<LogEntry>,
    /// When it ends, in seconds since the epoch.
    pub expires_at: i64,
    /// When it was completed or terminated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    /// The tenant it was approved under.
    pub tenant: String,
}

impl Mission {
    /// Not completed, and not past its expiry at `now`.
    pub fn is_live(&self, now: i64) -> bool {
        self.completed_at.is_none() && now < self.expires_at
    }
}

/// An approver's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Do what was asked.
    Approved,
    /// Refuse it.
    Denied,
}

/// What an operator answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// Approve or deny.
    pub status: Verdict,
    /// A mission's budget, when approving one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// Who decided, in the operator's own terms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
}

impl Decision {
    /// Approve.
    pub fn approve() -> Self {
        Self {
            status: Verdict::Approved,
            budget: None,
            decided_by: None,
        }
    }

    /// Deny.
    pub fn deny() -> Self {
        Self {
            status: Verdict::Denied,
            budget: None,
            decided_by: None,
        }
    }

    /// Set the budget an approved mission may spend.
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Record who decided.
    pub fn by(mut self, decided_by: impl Into<String>) -> Self {
        self.decided_by = Some(decided_by.into());
        self
    }
}

/// Which token was issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum IssuedKind {
    /// A person token.
    PersonToken,
    /// An auth token.
    AuthToken,
}

/// A token this Person Server issued, as [`crate::Hooks::on_issued`] reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issued {
    /// Which token.
    pub kind: IssuedKind,
    /// The agent it was issued to.
    pub agent: Agent,
    /// Who it speaks for.
    pub person: Person,
    /// The resource it is for.
    pub resource: String,
    /// The mission it was issued under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission: Option<Mission>,
    /// What it spent from the mission's budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
}
