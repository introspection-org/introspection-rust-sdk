use aauth_protocol::{Budget, Jwk, Requirement};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The agent behind a token request, from its verified agent token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestingAgent {
    /// `aauth:local@domain`.
    pub id: String,
    /// Its agent provider.
    pub provider: String,
    /// JWK thumbprint of its signing key: the resource token's `agent_jkt`.
    pub jkt: String,
}

/// The verified claims of each token in a request, for whatever else a
/// policy reads, such as the agent token's posture claims or R3 details.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerifiedClaims {
    /// The resource token's.
    pub resource_token: Map<String, Value>,
    /// The agent token's.
    pub agent_token: Map<String, Value>,
    /// The presented token's.
    pub presented_token: Map<String, Value>,
}

/// A Person Server's token request, once its signature and all three tokens
/// verify (`#ps-to-as-token-request`): who it is for, what is asked, and by
/// which agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccessRequest {
    /// The resource the auth token would be for: the resource token's `iss`.
    pub resource: String,
    /// The Person Server that sent the request, whose namespace `sub` is in.
    pub ps: String,
    /// The person's directed identifier at the resource.
    pub sub: String,
    /// The person's organisation, per OpenID Connect Enterprise; `(ps,
    /// tenant)` names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// The scopes asked for, space-separated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// The account the request is for (`#account-binding`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The person's mission, when the request is under one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_s256: Option<String>,
    /// What the call costs, from the resource token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// The resource's one-line description of the call, from its R3
    /// proposal, when it has one this server may read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The agent that asked.
    pub agent: RequestingAgent,
    /// Every verified claim.
    pub claims: VerifiedClaims,
}

/// The resource's answer to a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict {
    /// Issue the auth token, for `scope` when given (no broader than asked),
    /// else for what was asked.
    Allow {
        /// Space-separated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
    /// Refuse: the Person Server relays `403 denied` with `detail`.
    Deny {
        /// Why, for the agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Wait on someone's approval, without the person; settle it with
    /// [`crate::AccessServer::decide`].
    Approval,
    /// Wait on the person at `url` with `code`; settle it with
    /// [`crate::AccessServer::decide`] once they are done.
    Interaction {
        /// Where the person goes.
        url: String,
        /// The code they carry there.
        code: String,
    },
}

impl Verdict {
    /// Issue what was asked.
    pub fn allow() -> Self {
        Self::Allow { scope: None }
    }

    /// Refuse, saying why.
    pub fn deny(detail: impl Into<String>) -> Self {
        Self::Deny {
            detail: Some(detail.into()),
        }
    }
}

/// Where a pending request stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PendingStatus {
    /// Nobody has decided.
    Pending,
    /// Approved; its auth token is minted on the next poll.
    Approved,
    /// Denied.
    Denied,
    /// Approved, and its auth token handed out: one approval issues once.
    Collected,
}

/// The claims an auth token for a request carries besides its own, copied
/// from the resource token (`#auth-token-structure`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// The resource: the resource token's `iss`.
    pub aud: String,
    /// The person's Person Server.
    pub ps: String,
    /// The person's directed identifier.
    pub sub: String,
    /// The account, when the resource token names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The mission, when the resource token names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_s256: Option<String>,
    /// The person's organisation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// The R3 proposal the request was made under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r3_uri: Option<String>,
    /// Its hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r3_s256: Option<String>,
}

/// A request this Access Server's policy is waiting on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingAccess {
    /// Its identifier, in the pending URL.
    pub id: String,
    /// Where it stands.
    pub status: PendingStatus,
    /// What the Person Server is told it waits on:
    /// [`Requirement::Approval`] or [`Requirement::Interaction`].
    pub requirement: Requirement,
    /// The verified request.
    pub request: AccessRequest,
    /// What its auth token is issued with, once approved.
    pub grant: Grant,
    /// The agent's key, which the auth token binds.
    pub jwk: Jwk,
    /// The latest its auth token may expire: its agent token's and
    /// presented token's `exp`.
    pub not_after: i64,
    /// Narrowed by a decision that approves less than was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Who decided, in the operator's terms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    /// Whether the Person Server has had its terminal answer: later polls
    /// are `410 Gone`.
    #[serde(default)]
    pub answered: bool,
    /// When it was asked, in seconds since the epoch.
    pub created_at: i64,
    /// When it expires undecided.
    pub expires_at: i64,
}

/// Approve or deny.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    /// Issue the auth token on the next poll.
    Approved,
    /// Refuse it.
    Denied,
}

/// What an operator answers on a pending request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessDecision {
    /// Approve or deny.
    pub status: DecisionStatus,
    /// Approve for no more than this, space-separated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Who decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
}

impl AccessDecision {
    /// Approve.
    pub fn approve() -> Self {
        Self {
            status: DecisionStatus::Approved,
            scope: None,
            decided_by: None,
        }
    }

    /// Deny.
    pub fn deny() -> Self {
        Self {
            status: DecisionStatus::Denied,
            scope: None,
            decided_by: None,
        }
    }

    /// Approve for no more than `scope`.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Record who decided.
    pub fn by(mut self, decided_by: impl Into<String>) -> Self {
        self.decided_by = Some(decided_by.into());
        self
    }
}

/// An auth token this Access Server issued, as [`crate::Hooks::on_issued`]
/// reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IssuedAccess {
    /// The request it answered.
    pub request: AccessRequest,
    /// Its `jti`.
    pub jti: String,
    /// The scope it carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// When it expires.
    pub exp: i64,
}
