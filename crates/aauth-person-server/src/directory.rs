use async_trait::async_trait;

use crate::types::{Approval, ApprovalKind, Issued, Person};

/// Who this Person Server speaks for, and who decides for them.
#[async_trait]
pub trait Directory: Send + Sync {
    /// The person a `login_hint` names in `tenant`, or `None` if this Person
    /// Server does not speak for them there. A person returned with a
    /// tenant is in that tenant alone: a request under any other is
    /// refused. One returned without a tenant takes the request's, so a
    /// directory that serves several tenants checks `tenant` itself.
    async fn person(&self, login_hint: &str, tenant: &str) -> Option<Person>;

    /// Who decides a request; the person themself when `None`.
    async fn approver(&self, _kind: ApprovalKind, _person: &Person) -> Option<Person> {
        None
    }
}

/// What a Person Server tells its operator.
#[async_trait]
pub trait Hooks: Send + Sync {
    /// A decision is needed: reach whoever decides, however you reach people.
    async fn on_approval(&self, _approval: &Approval) {}
    /// An approval was decided.
    async fn on_decision(&self, _approval: &Approval) {}
    /// A token was issued.
    async fn on_issued(&self, _issued: &Issued) {}
}

/// No hooks.
pub struct NoHooks;

impl Hooks for NoHooks {}
