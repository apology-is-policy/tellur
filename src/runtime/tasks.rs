//! Invocation-owned tasks, ready admission and one-use completion waits.
//! Captures and continuations stay in the executor; no external I/O readiness yet.
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OwnerId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TaskId(pub(super) u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WaitId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Cold,
    Ready,
    Running,
    Completed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Task {
    owner: OwnerId,
    operation: String,
    phase: Phase,
    waiter: Option<WaitId>,
}
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) struct Registry {
    next_owner: u64,
    next_task: u64,
    next_wait: u64,
    owners: BTreeMap<OwnerId, BTreeSet<TaskId>>,
    tasks: BTreeMap<TaskId, Task>,
    ready: VecDeque<TaskId>,
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
                waiter: None,
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
    pub(super) fn schedule(
        &mut self,
        id: TaskId,
        owner: OwnerId,
        operation: &str,
    ) -> Result<WaitId> {
        if self.require(id, owner, Phase::Cold)?.operation != operation {
            return Err(error("pending task producer differs"));
        }
        let wait = WaitId(
            self.next_wait
                .checked_add(1)
                .ok_or_else(|| error("completion wait identity space exhausted"))?,
        );
        self.next_wait = wait.0;
        let task = self.tasks.get_mut(&id).unwrap();
        task.phase = Phase::Ready;
        task.waiter = Some(wait);
        self.ready.push_back(id);
        Ok(wait)
    }
    pub(super) fn next_ready(&self) -> Option<TaskId> {
        self.ready.front().copied()
    }
    pub(super) fn dispatch(&mut self, id: TaskId) -> Result<()> {
        if self.ready.front() != Some(&id)
            || !self.tasks.get(&id).is_some_and(|t| t.phase == Phase::Ready)
        {
            return Err(error("task is not next in the ready queue"));
        }
        self.ready.pop_front();
        self.tasks.get_mut(&id).unwrap().phase = Phase::Running;
        Ok(())
    }
    pub(super) fn complete(&mut self, id: TaskId, owner: OwnerId) -> Result<()> {
        self.require(id, owner, Phase::Running)?;
        self.tasks.get_mut(&id).unwrap().phase = Phase::Completed;
        Ok(())
    }
    /// Completion delivery is administrative, not a new source outcome. A late
    /// or repeated signal cannot resume another task or deliver the payload twice.
    pub(super) fn deliver(&mut self, id: TaskId, owner: OwnerId, wait: WaitId) -> Result<()> {
        if self.require(id, owner, Phase::Completed)?.waiter != Some(wait) {
            return Err(error("completion wait is stale or already delivered"));
        }
        self.tasks.get_mut(&id).unwrap().waiter = None;
        Ok(())
    }
    pub(super) fn consume(&mut self, id: TaskId, owner: OwnerId) -> Result<()> {
        if self.require(id, owner, Phase::Completed)?.waiter.is_some() {
            return Err(error("task completion has not been delivered"));
        }
        self.abandon(id);
        Ok(())
    }
    /// Emergency retirement does not manufacture a completion or run a cold body.
    pub(super) fn abandon(&mut self, id: TaskId) {
        if let Some(task) = self.tasks.remove(&id) {
            if task.phase == Phase::Ready {
                self.ready.retain(|queued| *queued != id);
            }
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
        self.ready.retain(|task| self.tasks.contains_key(task));
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn live_counts(&self) -> (usize, usize) {
        (self.owners.len(), self.tasks.len())
    }
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.owners.is_empty() && self.tasks.is_empty() && self.ready.is_empty()
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
        let wait = r.schedule(task, a, "work").unwrap();
        assert!(r.complete(task, a).is_err());
        r.dispatch(task).unwrap();
        assert!(r.schedule(task, a, "work").is_err());
        r.complete(task, a).unwrap();
        assert!(r.complete(task, a).is_err());
        assert!(r.retire_owner(a, false).is_err());
        assert!(r.consume(task, a).is_err());
        r.deliver(task, a, wait).unwrap();
        assert!(r.deliver(task, a, wait).is_err());
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
        assert!(r.schedule(task, b, "first").is_err());
        assert_eq!(r, before);
        assert!(r.schedule(task, a, "other").is_err());
        assert_eq!(r, before);
        let wait = r.schedule(task, a, "first").unwrap();
        r.dispatch(task).unwrap();
        assert!(r.complete(task, b).is_err());
        r.complete(task, a).unwrap();
        assert!(r.consume(task, b).is_err());
        assert!(r.consume(task, a).is_err());
        r.deliver(task, a, wait).unwrap();
        assert!(r.deliver(task, a, wait).is_err());
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
        r.schedule(running, a, "running").unwrap();
        r.dispatch(running).unwrap();
        let other = r.defer(b, "other").unwrap();
        r.retire_owner(a, true).unwrap();
        assert!(r.complete(running, a).is_err());
        assert!(r.consume(cold, a).is_err());
        let wait = r.schedule(other, b, "other").unwrap();
        r.dispatch(other).unwrap();
        r.complete(other, b).unwrap();
        r.deliver(other, b, wait).unwrap();
        r.consume(other, b).unwrap();
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn ready_dispatch_is_ordered_and_retirement_removes_queued_work() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let first = r.defer(a, "first").unwrap();
        let second = r.defer(b, "second").unwrap();
        r.schedule(first, a, "first").unwrap();
        let wait = r.schedule(second, b, "second").unwrap();
        let before = r.clone();
        assert!(r.dispatch(second).is_err());
        assert_eq!(r, before);
        assert!(r.retire_owner(a, false).is_err());
        r.retire_owner(a, true).unwrap();
        assert!(r.dispatch(first).is_err());
        r.dispatch(second).unwrap();
        assert!(r.dispatch(second).is_err());
        r.complete(second, b).unwrap();
        r.deliver(second, b, wait).unwrap();
        r.consume(second, b).unwrap();
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn wait_exhaustion_and_stale_signals_never_change_ready_or_completion_state() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let first = r.defer(a, "work").unwrap();
        let old_wait = r.schedule(first, a, "work").unwrap();
        r.abandon(first);
        let second = r.defer(a, "work").unwrap();
        let wait = r.schedule(second, a, "work").unwrap();
        assert_ne!(wait, old_wait);
        r.dispatch(second).unwrap();
        r.complete(second, a).unwrap();
        let before = r.clone();
        assert!(r.deliver(second, a, old_wait).is_err());
        assert!(r.deliver(first, a, old_wait).is_err());
        assert_eq!(r, before);
        r.deliver(second, a, wait).unwrap();
        r.consume(second, a).unwrap();
        let third = r.defer(a, "work").unwrap();
        r.next_wait = u64::MAX;
        let before = r.clone();
        assert!(r.schedule(third, a, "work").is_err());
        assert_eq!(r, before);
        r.retire_owner(a, true).unwrap();
        assert!(r.is_empty());
    }
}
