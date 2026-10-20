use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::types::{Approval, ApprovalStatus, LogEntry, Mission, Person};

/// Whether [`Store::spend`] may take a mission past its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spend {
    /// Only if it fits what is left: a request issued without asking anyone.
    WithinBudget,
    /// Whatever is left: a request the approver approved.
    Approved,
}

/// A store that could not answer, such as a database that is down. The
/// Person Server answers the request with a `503`.
#[derive(Debug)]
pub struct StoreError(Box<dyn std::error::Error + Send + Sync>);

impl StoreError {
    /// Wrap the underlying error.
    pub fn new(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self(error.into())
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "store unavailable: {}", self.0)
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// What a [`Store`] answers.
pub type StoreResult<T> = Result<T, StoreError>;

/// Where a Person Server keeps its state.
///
/// Every method takes the request's tenant: the AAuth `tenant` the agent
/// named, or the server's default tenant when it named none (see
/// [`crate::Config::default_tenant`]). A store keeps each tenant's records
/// apart: a record is found only under the tenant it was saved with. A
/// tenant the store cannot scope, such as one in a format it does not
/// serve, finds nothing.
///
/// Three methods must be atomic, because concurrent requests race on them:
/// [`Store::spend`] checks and spends in one step, so two bookings cannot
/// both fit in what is left; [`Store::transition_approval`] changes an
/// approval only from the status it was read in, so a decision, a
/// withdrawal or a collection happens once; and [`Store::insert_mission`]
/// inserts only when no mission has that hash, so it never replaces one.
#[async_trait]
pub trait Store: Send + Sync {
    /// The person an agent key is bound to.
    async fn binding(&self, tenant: &str, jkt: &str) -> StoreResult<Option<Person>>;
    /// Bind a key to a person unless it is bound already; returns whoever it is bound to.
    async fn bind(&self, tenant: &str, jkt: &str, person: &Person) -> StoreResult<Person>;
    /// The mission `s256` names.
    async fn mission(&self, tenant: &str, s256: &str) -> StoreResult<Option<Mission>>;
    /// Save a newly approved mission, unless one with its hash exists.
    /// Returns whether it did: an existing mission is never replaced.
    async fn insert_mission(&self, tenant: &str, mission: &Mission) -> StoreResult<bool>;
    /// Append `entry` to a live mission's log. Returns whether it was live.
    async fn log_mission(
        &self,
        tenant: &str,
        s256: &str,
        entry: LogEntry,
        now: i64,
    ) -> StoreResult<bool>;
    /// Mark a mission completed at `at`, unless it already was. Returns the
    /// mission, or `None` when there is no such mission.
    async fn end_mission(&self, tenant: &str, s256: &str, at: i64) -> StoreResult<Option<Mission>>;
    /// Add `amount` to a live mission's spend, under `rule`. Returns the
    /// mission as spent, or `None` when it was not live or `amount` did not fit.
    async fn spend(
        &self,
        tenant: &str,
        s256: &str,
        amount: u64,
        rule: Spend,
        now: i64,
    ) -> StoreResult<Option<Mission>>;
    /// The tenant's missions, for the management routes.
    async fn missions(&self, tenant: &str) -> StoreResult<Vec<Mission>>;
    /// The approval `id` names.
    async fn approval(&self, tenant: &str, id: &str) -> StoreResult<Option<Approval>>;
    /// Save a new approval.
    async fn insert_approval(&self, tenant: &str, approval: &Approval) -> StoreResult<()>;
    /// Replace approval `next.id` with `next`, only if its status is still
    /// `expected`. Returns whether it did.
    async fn transition_approval(
        &self,
        tenant: &str,
        expected: ApprovalStatus,
        next: &Approval,
    ) -> StoreResult<bool>;
    /// The tenant's approvals, for the management routes.
    async fn approvals(&self, tenant: &str) -> StoreResult<Vec<Approval>>;
}

/// A record's key: its tenant and its own identifier.
type Key = (String, String);

fn key(tenant: &str, id: &str) -> Key {
    (tenant.to_string(), id.to_string())
}

#[derive(Default)]
struct State {
    bindings: HashMap<Key, Person>,
    missions: HashMap<Key, Mission>,
    approvals: HashMap<Key, Approval>,
}

/// Approvals are kept this long past their expiry, so a late poll still
/// reads `expired` rather than `not_found`.
const APPROVAL_RETENTION_SECS: i64 = 24 * 60 * 60;

/// Everything in memory: for development, tests and single-process demos.
/// Expired approvals are dropped a day after they expire; nothing else is
/// ever dropped, and nothing survives a restart.
#[derive(Default)]
pub struct MemoryStore(Mutex<State>);

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> StoreResult<T> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(f(&mut state))
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn binding(&self, tenant: &str, jkt: &str) -> StoreResult<Option<Person>> {
        self.with(|s| s.bindings.get(&key(tenant, jkt)).cloned())
    }

    async fn bind(&self, tenant: &str, jkt: &str, person: &Person) -> StoreResult<Person> {
        self.with(|s| {
            s.bindings
                .entry(key(tenant, jkt))
                .or_insert_with(|| person.clone())
                .clone()
        })
    }

    async fn mission(&self, tenant: &str, s256: &str) -> StoreResult<Option<Mission>> {
        self.with(|s| s.missions.get(&key(tenant, s256)).cloned())
    }

    async fn insert_mission(&self, tenant: &str, mission: &Mission) -> StoreResult<bool> {
        self.with(|s| match s.missions.entry(key(tenant, &mission.s256)) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(mission.clone());
                true
            }
        })
    }

    async fn log_mission(
        &self,
        tenant: &str,
        s256: &str,
        entry: LogEntry,
        now: i64,
    ) -> StoreResult<bool> {
        self.with(|s| match s.missions.get_mut(&key(tenant, s256)) {
            Some(mission) if mission.is_live(now) => {
                mission.log.push(entry);
                true
            }
            _ => false,
        })
    }

    async fn end_mission(&self, tenant: &str, s256: &str, at: i64) -> StoreResult<Option<Mission>> {
        self.with(|s| {
            let mission = s.missions.get_mut(&key(tenant, s256))?;
            mission.completed_at.get_or_insert(at);
            Some(mission.clone())
        })
    }

    async fn spend(
        &self,
        tenant: &str,
        s256: &str,
        amount: u64,
        rule: Spend,
        now: i64,
    ) -> StoreResult<Option<Mission>> {
        self.with(|s| {
            let mission = s.missions.get_mut(&key(tenant, s256))?;
            if !mission.is_live(now) {
                return None;
            }
            if rule == Spend::WithinBudget {
                let left = mission.budget.as_ref()?.amount.checked_sub(mission.spent)?;
                if left < amount {
                    return None;
                }
            }
            mission.spent = mission.spent.saturating_add(amount);
            Some(mission.clone())
        })
    }

    async fn missions(&self, tenant: &str) -> StoreResult<Vec<Mission>> {
        self.with(|s| {
            s.missions
                .iter()
                .filter(|((t, _), _)| t == tenant)
                .map(|(_, m)| m.clone())
                .collect()
        })
    }

    async fn approval(&self, tenant: &str, id: &str) -> StoreResult<Option<Approval>> {
        self.with(|s| s.approvals.get(&key(tenant, id)).cloned())
    }

    async fn insert_approval(&self, tenant: &str, approval: &Approval) -> StoreResult<()> {
        self.with(|s| {
            let now = approval.created_at;
            s.approvals
                .retain(|_, a| a.expires_at + APPROVAL_RETENTION_SECS > now);
            s.approvals
                .insert(key(tenant, &approval.id), approval.clone());
        })
    }

    async fn transition_approval(
        &self,
        tenant: &str,
        expected: ApprovalStatus,
        next: &Approval,
    ) -> StoreResult<bool> {
        self.with(|s| match s.approvals.get_mut(&key(tenant, &next.id)) {
            Some(current) if current.status == expected => {
                *current = next.clone();
                true
            }
            _ => false,
        })
    }

    async fn approvals(&self, tenant: &str) -> StoreResult<Vec<Approval>> {
        self.with(|s| {
            s.approvals
                .iter()
                .filter(|((t, _), _)| t == tenant)
                .map(|(_, a)| a.clone())
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission(person: &str) -> Mission {
        Mission {
            s256: "h".into(),
            mission: "{}".into(),
            agent_id: "aauth:travel@agents.example".into(),
            person: Person::new(person),
            resources: Vec::new(),
            budget: None,
            spent: 0,
            log: Vec::new(),
            expires_at: i64::MAX,
            completed_at: None,
            tenant: "default".into(),
        }
    }

    #[tokio::test]
    async fn a_mission_is_never_replaced() {
        let store = MemoryStore::default();
        assert!(store
            .insert_mission("default", &mission("sam"))
            .await
            .unwrap());
        store.end_mission("default", "h", 10).await.unwrap();
        assert!(!store
            .insert_mission("default", &mission("dana"))
            .await
            .unwrap());
        let kept = store.mission("default", "h").await.unwrap().unwrap();
        assert_eq!(kept.person.id, "sam");
        assert_eq!(kept.completed_at, Some(10));
    }

    #[tokio::test]
    async fn each_tenant_sees_only_its_own() {
        let store = MemoryStore::default();
        assert!(store.insert_mission("acme", &mission("sam")).await.unwrap());
        assert!(store
            .insert_mission("globex", &mission("hank"))
            .await
            .unwrap());
        let at = |t| store.mission(t, "h");
        assert_eq!(at("acme").await.unwrap().unwrap().person.id, "sam");
        assert_eq!(at("globex").await.unwrap().unwrap().person.id, "hank");
        assert!(at("default").await.unwrap().is_none());
        assert_eq!(store.missions("acme").await.unwrap().len(), 1);
        store.bind("acme", "k", &Person::new("sam")).await.unwrap();
        assert!(store.binding("globex", "k").await.unwrap().is_none());
    }
}
