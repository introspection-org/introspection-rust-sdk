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

/// Where a Person Server keeps its state.
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
    async fn binding(&self, jkt: &str) -> Option<Person>;
    /// Bind a key to a person unless it is bound already; returns whoever it is bound to.
    async fn bind(&self, jkt: &str, person: &Person) -> Person;
    /// The mission `s256` names.
    async fn mission(&self, s256: &str) -> Option<Mission>;
    /// Save a newly approved mission, unless one with its hash exists.
    /// Returns whether it did: an existing mission is never replaced.
    async fn insert_mission(&self, mission: &Mission) -> bool;
    /// Append `entry` to a live mission's log. Returns whether it was live.
    async fn log_mission(&self, s256: &str, entry: LogEntry, now: i64) -> bool;
    /// Mark a mission completed at `at`, unless it already was. Returns the
    /// mission, or `None` when there is no such mission.
    async fn end_mission(&self, s256: &str, at: i64) -> Option<Mission>;
    /// Add `amount` to a live mission's spend, under `rule`. Returns the
    /// mission as spent, or `None` when it was not live or `amount` did not fit.
    async fn spend(&self, s256: &str, amount: u64, rule: Spend, now: i64) -> Option<Mission>;
    /// Every mission, for the management routes.
    async fn missions(&self) -> Vec<Mission>;
    /// The approval `id` names.
    async fn approval(&self, id: &str) -> Option<Approval>;
    /// Save a new approval.
    async fn insert_approval(&self, approval: &Approval);
    /// Replace approval `next.id` with `next`, only if its status is still
    /// `expected`. Returns whether it did.
    async fn transition_approval(&self, expected: ApprovalStatus, next: &Approval) -> bool;
    /// Every approval, for the management routes.
    async fn approvals(&self) -> Vec<Approval>;
}

#[derive(Default)]
struct State {
    bindings: HashMap<String, Person>,
    missions: HashMap<String, Mission>,
    approvals: HashMap<String, Approval>,
}

/// Approvals are kept this long past their expiry, so a late poll still
/// reads `expired` rather than `not_found`.
const APPROVAL_RETENTION_SECS: i64 = 24 * 60 * 60;

/// Everything in memory: for development, tests and single-process demos.
/// Expired approvals are dropped a day after they expire; nothing else is
/// ever dropped.
#[derive(Default)]
pub struct MemoryStore(Mutex<State>);

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut state)
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn binding(&self, jkt: &str) -> Option<Person> {
        self.with(|s| s.bindings.get(jkt).cloned())
    }

    async fn bind(&self, jkt: &str, person: &Person) -> Person {
        self.with(|s| {
            s.bindings
                .entry(jkt.to_string())
                .or_insert_with(|| person.clone())
                .clone()
        })
    }

    async fn mission(&self, s256: &str) -> Option<Mission> {
        self.with(|s| s.missions.get(s256).cloned())
    }

    async fn insert_mission(&self, mission: &Mission) -> bool {
        self.with(|s| match s.missions.entry(mission.s256.clone()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(mission.clone());
                true
            }
        })
    }

    async fn log_mission(&self, s256: &str, entry: LogEntry, now: i64) -> bool {
        self.with(|s| match s.missions.get_mut(s256) {
            Some(mission) if mission.is_live(now) => {
                mission.log.push(entry);
                true
            }
            _ => false,
        })
    }

    async fn end_mission(&self, s256: &str, at: i64) -> Option<Mission> {
        self.with(|s| {
            let mission = s.missions.get_mut(s256)?;
            mission.completed_at.get_or_insert(at);
            Some(mission.clone())
        })
    }

    async fn spend(&self, s256: &str, amount: u64, rule: Spend, now: i64) -> Option<Mission> {
        self.with(|s| {
            let mission = s.missions.get_mut(s256)?;
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

    async fn missions(&self) -> Vec<Mission> {
        self.with(|s| s.missions.values().cloned().collect())
    }

    async fn approval(&self, id: &str) -> Option<Approval> {
        self.with(|s| s.approvals.get(id).cloned())
    }

    async fn insert_approval(&self, approval: &Approval) {
        self.with(|s| {
            let now = approval.created_at;
            s.approvals
                .retain(|_, a| a.expires_at + APPROVAL_RETENTION_SECS > now);
            s.approvals.insert(approval.id.clone(), approval.clone());
        });
    }

    async fn transition_approval(&self, expected: ApprovalStatus, next: &Approval) -> bool {
        self.with(|s| match s.approvals.get_mut(&next.id) {
            Some(current) if current.status == expected => {
                *current = next.clone();
                true
            }
            _ => false,
        })
    }

    async fn approvals(&self) -> Vec<Approval> {
        self.with(|s| s.approvals.values().cloned().collect())
    }
}
