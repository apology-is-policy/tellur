//! Invocation-owned identities for the sequential cold-task executor. Captures
//! remain in values; this ledger tracks obligations, not scheduling or storage.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OwnerId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TaskId(pub(super) u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Cold,
    Running,
    Completed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Task {
    owner: OwnerId,
    operation: String,
    phase: Phase,
}
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) struct Registry {
    next_owner: u64,
    next_task: u64,
    owners: BTreeMap<OwnerId, BTreeSet<TaskId>>,
    tasks: BTreeMap<TaskId, Task>,
}
impl Registry {
    /// Check capacity before diagnostic activation admission. Commit is infallible
    /// while this exclusively borrowed execution has not admitted another owner.
    pub(super) fn prepare_owner(&self) -> Result<OwnerId> {
        self.next_owner
            .checked_add(1)
            .map(OwnerId)
            .ok_or_else(|| error("invocation owner identity space exhausted"))
    }
    pub(super) fn commit_owner(&mut self, owner: OwnerId) {
        assert_eq!(self.next_owner.checked_add(1), Some(owner.0));
        self.next_owner = owner.0;
        assert!(self.owners.insert(owner, BTreeSet::new()).is_none());
    }
    pub(super) fn defer(&mut self, owner: OwnerId, operation: &str) -> Result<TaskId> {
        if !self.owners.contains_key(&owner) {
            return Err(error("task owner is no longer active"));
        }
        let id = TaskId(
            self.next_task
                .checked_add(1)
                .ok_or_else(|| error("task identity space exhausted"))?,
        );
        self.next_task = id.0;
        self.tasks.insert(
            id,
            Task {
                owner,
                operation: operation.into(),
                phase: Phase::Cold,
            },
        );
        self.owners.get_mut(&owner).unwrap().insert(id);
        Ok(id)
    }
    fn require(&self, id: TaskId, owner: OwnerId, phase: Phase) -> Result<&Task> {
        let task = self
            .tasks
            .get(&id)
            .ok_or_else(|| error("task is unknown or already consumed"))?;
        if task.owner != owner {
            return Err(error("task belongs to another invocation"));
        }
        if task.phase != phase {
            return Err(error("invalid task lifecycle transition"));
        }
        Ok(task)
    }
    pub(super) fn begin(&mut self, id: TaskId, owner: OwnerId, operation: &str) -> Result<()> {
        if self.require(id, owner, Phase::Cold)?.operation != operation {
            return Err(error("pending task producer differs"));
        }
        self.tasks.get_mut(&id).unwrap().phase = Phase::Running;
        Ok(())
    }
    pub(super) fn complete(&mut self, id: TaskId, owner: OwnerId) -> Result<()> {
        self.require(id, owner, Phase::Running)?;
        self.tasks.get_mut(&id).unwrap().phase = Phase::Completed;
        Ok(())
    }
    pub(super) fn consume(&mut self, id: TaskId, owner: OwnerId) -> Result<()> {
        self.require(id, owner, Phase::Completed)?;
        self.abandon(id);
        Ok(())
    }
    /// Emergency retirement does not manufacture a completion or run a cold body.
    pub(super) fn abandon(&mut self, id: TaskId) {
        if let Some(task) = self.tasks.remove(&id) {
            self.owners.get_mut(&task.owner).unwrap().remove(&id);
        }
    }
    pub(super) fn retire_owner(&mut self, owner: OwnerId, abrupt: bool) -> Result<()> {
        let pending = self
            .owners
            .get(&owner)
            .ok_or_else(|| error("invocation owner is no longer active"))?;
        if !abrupt && !pending.is_empty() {
            return Err(error("invocation has outstanding task obligations"));
        }
        for task in self.owners.remove(&owner).unwrap() {
            self.tasks.remove(&task);
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn live_counts(&self) -> (usize, usize) {
        (self.owners.len(), self.tasks.len())
    }
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.owners.is_empty() && self.tasks.is_empty()
    }
}

pub(super) fn retire_value(registry: &mut Registry, value: Value, abrupt: bool) -> Result<()> {
    match value {
        Value::Task { id, arguments, .. } => {
            if !abrupt && registry.tasks.contains_key(&TaskId(id)) {
                return Err(error("pending task cannot be discarded normally"));
            }
            registry.abandon(TaskId(id));
            for value in arguments {
                retire_value(registry, value, abrupt)?;
            }
        }
        Value::Construct { fields, .. } => {
            for value in fields {
                retire_value(registry, value, abrupt)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(r: &mut Registry) -> OwnerId {
        let id = r.prepare_owner().unwrap();
        r.commit_owner(id);
        id
    }
    #[test]
    fn completion_is_one_shot_and_remains_an_obligation_until_consumed() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let task = r.defer(a, "work").unwrap();
        assert!(r.retire_owner(a, false).is_err());
        assert!(r.consume(task, a).is_err());
        r.begin(task, a, "work").unwrap();
        assert!(r.begin(task, a, "work").is_err());
        r.complete(task, a).unwrap();
        assert!(r.complete(task, a).is_err());
        assert!(r.retire_owner(a, false).is_err());
        r.consume(task, a).unwrap();
        assert!(r.consume(task, a).is_err());
        r.retire_owner(a, false).unwrap();
        assert!(r.is_empty());
    }
    #[test]
    fn wrong_owner_and_producer_cannot_reparent_or_partially_start_work() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let task = r.defer(a, "first").unwrap();
        let before = r.clone();
        assert!(r.begin(task, b, "first").is_err());
        assert_eq!(r, before);
        assert!(r.begin(task, a, "other").is_err());
        assert_eq!(r, before);
        r.begin(task, a, "first").unwrap();
        assert!(r.complete(task, b).is_err());
        r.complete(task, a).unwrap();
        assert!(r.consume(task, b).is_err());
        r.consume(task, a).unwrap();
        r.retire_owner(a, false).unwrap();
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
    }
    #[test]
    fn exhausted_identity_admission_is_atomic_and_never_reuses_retired_ids() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let first = r.defer(a, "work").unwrap();
        r.abandon(first);
        let second = r.defer(a, "work").unwrap();
        assert!(second.0 > first.0);
        r.retire_owner(a, true).unwrap();
        let b = owner(&mut r);
        assert!(b.0 > a.0);
        r.next_task = u64::MAX;
        let before = r.clone();
        assert!(r.defer(b, "work").is_err());
        assert_eq!(r, before);
        r.next_owner = u64::MAX;
        let before = r.clone();
        assert!(r.prepare_owner().is_err());
        assert_eq!(r, before);
    }
    #[test]
    fn emergency_retirement_is_not_completion_and_preserves_other_owners() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let cold = r.defer(a, "cold").unwrap();
        let running = r.defer(a, "running").unwrap();
        r.begin(running, a, "running").unwrap();
        let other = r.defer(b, "other").unwrap();
        r.retire_owner(a, true).unwrap();
        assert!(r.complete(running, a).is_err());
        assert!(r.consume(cold, a).is_err());
        r.begin(other, b, "other").unwrap();
        r.complete(other, b).unwrap();
        r.consume(other, b).unwrap();
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
    }
}
