//! Invocation-owned tasks, ready admission and one-use completion waits.
//! Captures and continuations stay in the executor; readiness never completes a task.
use super::*;
use std::collections::VecDeque;
use std::sync::Arc;

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
    Waiting(u64),
    Completed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Join {
    Unclaimed,
    Waiting(WaitId),
    Delivered,
}
/// Dedicated execution identity. Late tokens retain it, preventing allocator reuse.
/// This is neither a frame address nor an observed diagnostic activation identity.
#[derive(Default, Clone, Debug)]
struct ExecutionIdentity(Arc<()>);
impl PartialEq for ExecutionIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ExecutionIdentity {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Readiness {
    execution: ExecutionIdentity,
    generation: u64,
    task: TaskId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Task {
    owner: OwnerId,
    operation: String,
    phase: Phase,
    join: Join,
}
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) struct Registry {
    next_owner: u64,
    next_task: u64,
    next_wait: u64,
    next_readiness: u64,
    identity: ExecutionIdentity,
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
                join: Join::Unclaimed,
            },
        );
        self.owners.get_mut(&owner).unwrap().insert(id);
        Ok(id)
    }
    fn require_owner(&self, id: TaskId, owner: OwnerId) -> Result<&Task> {
        let task = self
            .tasks
            .get(&id)
            .ok_or_else(|| error("task is unknown or already consumed"))?;
        if task.owner != owner {
            return Err(error("task belongs to another invocation"));
        }
        Ok(task)
    }
    fn require(&self, id: TaskId, owner: OwnerId, phase: Phase) -> Result<&Task> {
        let task = self.require_owner(id, owner)?;
        if task.phase != phase {
            return Err(error("invalid task lifecycle transition"));
        }
        Ok(task)
    }
    /// Queue work without attaching an awaiting caller or running its body.
    pub(super) fn enqueue(&mut self, id: TaskId, owner: OwnerId, operation: &str) -> Result<()> {
        if self.require(id, owner, Phase::Cold)?.operation != operation {
            return Err(error("pending task producer differs"));
        }
        self.tasks.get_mut(&id).unwrap().phase = Phase::Ready;
        self.ready.push_back(id);
        Ok(())
    }
    fn prepare_wait(&self) -> Result<WaitId> {
        self.next_wait
            .checked_add(1)
            .map(WaitId)
            .ok_or_else(|| error("completion wait identity space exhausted"))
    }
    fn commit_join(&mut self, id: TaskId, wait: WaitId) {
        assert_eq!(self.next_wait.checked_add(1), Some(wait.0));
        let task = self.tasks.get_mut(&id).unwrap();
        assert_eq!(task.join, Join::Unclaimed);
        task.join = Join::Waiting(wait);
        self.next_wait = wait.0;
    }
    /// Source spawn wiring follows separately. A join may be registered before
    /// dispatch, during a readiness wait, or after completion; it never queues work.
    #[allow(dead_code)]
    pub(super) fn join(&mut self, id: TaskId, owner: OwnerId, operation: &str) -> Result<WaitId> {
        let task = self.require_owner(id, owner)?;
        if task.operation != operation {
            return Err(error("pending task producer differs"));
        }
        if task.phase == Phase::Cold || task.join != Join::Unclaimed {
            return Err(error(
                "task is cold or its completion has already been claimed",
            ));
        }
        let wait = self.prepare_wait()?;
        self.commit_join(id, wait);
        Ok(wait)
    }
    /// Existing cold await atomically queues work and attaches its one-use join.
    pub(super) fn schedule(
        &mut self,
        id: TaskId,
        owner: OwnerId,
        operation: &str,
    ) -> Result<WaitId> {
        let task = self.require(id, owner, Phase::Cold)?;
        if task.operation != operation {
            return Err(error("pending task producer differs"));
        }
        if task.join != Join::Unclaimed {
            return Err(error("task completion has already been claimed"));
        }
        // Capacity is checked before enqueue so failure cannot leave ready work.
        let wait = self.prepare_wait()?;
        self.enqueue(id, owner, operation)?;
        self.commit_join(id, wait);
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
    /// Preflight before transferring the original running context into wait storage.
    pub(super) fn prepare_readiness(&self, id: TaskId, owner: OwnerId) -> Result<Readiness> {
        self.require(id, owner, Phase::Running)?;
        Ok(Readiness {
            execution: self.identity.clone(),
            generation: self
                .next_readiness
                .checked_add(1)
                .ok_or_else(|| error("readiness generation space exhausted"))?,
            task: id,
        })
    }
    pub(super) fn park(&mut self, owner: OwnerId, token: &Readiness) -> Result<()> {
        self.require(token.task, owner, Phase::Running)?;
        if token.execution != self.identity
            || self.next_readiness.checked_add(1) != Some(token.generation)
        {
            return Err(error(
                "readiness admission is stale or belongs to another execution",
            ));
        }
        self.next_readiness = token.generation;
        self.tasks.get_mut(&token.task).unwrap().phase = Phase::Waiting(token.generation);
        Ok(())
    }
    pub(super) fn waiting_task(&self, token: &Readiness) -> Result<TaskId> {
        if token.execution != self.identity {
            return Err(error("readiness belongs to another execution"));
        }
        if !self
            .tasks
            .get(&token.task)
            .is_some_and(|task| task.phase == Phase::Waiting(token.generation))
        {
            return Err(error("readiness is unknown, stale or already signalled"));
        }
        Ok(token.task)
    }
    pub(super) fn wake(&mut self, token: &Readiness) -> Result<()> {
        let id = self.waiting_task(token)?;
        self.tasks.get_mut(&id).unwrap().phase = Phase::Ready;
        self.ready.push_back(id);
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn exhaust_readiness_space(&mut self) {
        self.next_readiness = u64::MAX;
    }
    pub(super) fn complete(&mut self, id: TaskId, owner: OwnerId) -> Result<()> {
        self.require(id, owner, Phase::Running)?;
        self.tasks.get_mut(&id).unwrap().phase = Phase::Completed;
        Ok(())
    }
    /// Completion delivery is administrative, not a new source outcome. A late
    /// or repeated signal cannot resume another task or deliver the payload twice.
    pub(super) fn deliver(&mut self, id: TaskId, owner: OwnerId, wait: WaitId) -> Result<()> {
        if self.require(id, owner, Phase::Completed)?.join != Join::Waiting(wait) {
            return Err(error("completion wait is stale or already delivered"));
        }
        self.tasks.get_mut(&id).unwrap().join = Join::Delivered;
        Ok(())
    }
    pub(super) fn consume(&mut self, id: TaskId, owner: OwnerId) -> Result<()> {
        if self.require(id, owner, Phase::Completed)?.join != Join::Delivered {
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
    pub(super) fn has_started_children(&self, owner: OwnerId) -> bool {
        self.owners.get(&owner).is_some_and(|tasks| {
            tasks.iter().any(|id| {
                matches!(
                    self.tasks[id].phase,
                    Phase::Ready | Phase::Running | Phase::Waiting(_) | Phase::Completed
                )
            })
        })
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
    fn queue_and_join_are_independent_before_during_and_after_execution() {
        for phase in [
            Phase::Ready,
            Phase::Running,
            Phase::Waiting(1),
            Phase::Completed,
        ] {
            let mut r = Registry::default();
            let a = owner(&mut r);
            let task = r.defer(a, "work").unwrap();
            let cold = r.clone();
            assert!(r.join(task, a, "work").is_err());
            assert_eq!(r, cold);
            r.enqueue(task, a, "work").unwrap();
            assert_eq!(r.next_wait, 0);
            assert_eq!(r.tasks[&task].join, Join::Unclaimed);
            let queued = r.clone();
            assert!(r.enqueue(task, a, "work").is_err());
            assert_eq!(r, queued);
            if phase != Phase::Ready {
                r.dispatch(task).unwrap();
            }
            let readiness = if matches!(phase, Phase::Waiting(_)) {
                let token = r.prepare_readiness(task, a).unwrap();
                r.park(a, &token).unwrap();
                Some(token)
            } else {
                None
            };
            if phase == Phase::Completed {
                r.complete(task, a).unwrap();
            }
            let queue = r.ready.clone();
            let before_phase = r.tasks[&task].phase;
            let wait = r.join(task, a, "work").unwrap();
            assert_eq!(r.ready, queue);
            assert_eq!(r.tasks[&task].phase, before_phase);
            let joined = r.clone();
            assert!(r.join(task, a, "work").is_err());
            assert!(r.retire_owner(a, false).is_err());
            assert!(r.consume(task, a).is_err());
            assert_eq!(r, joined);
            if let Some(token) = readiness {
                r.wake(&token).unwrap();
                r.dispatch(task).unwrap();
            } else if phase == Phase::Ready {
                r.dispatch(task).unwrap();
            }
            if phase != Phase::Completed {
                r.complete(task, a).unwrap();
            }
            r.deliver(task, a, wait).unwrap();
            let delivered = r.clone();
            assert!(r.join(task, a, "work").is_err());
            assert!(r.deliver(task, a, wait).is_err());
            assert_eq!(r, delivered);
            r.consume(task, a).unwrap();
            r.retire_owner(a, false).unwrap();
            assert!(r.is_empty());
        }
    }
    #[test]
    fn completed_unjoined_tasks_still_require_owned_completion_delivery() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let task = r.defer(a, "work").unwrap();
        r.enqueue(task, a, "work").unwrap();
        r.dispatch(task).unwrap();
        r.complete(task, a).unwrap();
        let completed = r.clone();
        assert!(r.consume(task, a).is_err());
        assert!(r.retire_owner(a, false).is_err());
        assert!(r.join(task, b, "work").is_err());
        assert!(r.join(task, a, "other").is_err());
        assert!(r.schedule(task, a, "work").is_err());
        assert!(r.deliver(task, a, WaitId(1)).is_err());
        assert_eq!(r, completed);
        let wait = r.join(task, a, "work").unwrap();
        assert!(r.next_ready().is_none());
        let joined = r.clone();
        assert!(r.deliver(task, b, wait).is_err());
        assert!(r.deliver(task, a, WaitId(wait.0 + 1)).is_err());
        assert_eq!(r, joined);
        r.deliver(task, a, wait).unwrap();
        r.consume(task, a).unwrap();
        r.retire_owner(a, false).unwrap();
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
    }
    #[test]
    fn exhausted_join_admission_preserves_queue_wait_and_completed_obligations() {
        for phase in [
            Phase::Cold,
            Phase::Ready,
            Phase::Running,
            Phase::Waiting(1),
            Phase::Completed,
        ] {
            let mut r = Registry::default();
            let a = owner(&mut r);
            let task = r.defer(a, "work").unwrap();
            r.next_wait = u64::MAX;
            // Queue admission allocates no join identity, even at this limit.
            if phase != Phase::Cold {
                r.enqueue(task, a, "work").unwrap();
            }
            if matches!(phase, Phase::Running | Phase::Waiting(_) | Phase::Completed) {
                r.dispatch(task).unwrap();
            }
            if matches!(phase, Phase::Waiting(_)) {
                let token = r.prepare_readiness(task, a).unwrap();
                r.park(a, &token).unwrap();
            }
            if phase == Phase::Completed {
                r.complete(task, a).unwrap();
            }
            let before = r.clone();
            let failure = if phase == Phase::Cold {
                r.schedule(task, a, "work")
            } else {
                r.join(task, a, "work")
            };
            assert!(
                failure
                    .unwrap_err()
                    .message
                    .contains("completion wait identity space exhausted")
            );
            assert_eq!(r, before);
            r.retire_owner(a, true).unwrap();
            assert!(r.is_empty());
        }
    }
    #[test]
    fn emergency_retirement_invalidates_unjoined_work_without_touching_other_joins() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let completed = r.defer(a, "completed").unwrap();
        r.enqueue(completed, a, "completed").unwrap();
        r.dispatch(completed).unwrap();
        r.complete(completed, a).unwrap();
        let waiting = r.defer(a, "waiting").unwrap();
        r.enqueue(waiting, a, "waiting").unwrap();
        r.dispatch(waiting).unwrap();
        let token = r.prepare_readiness(waiting, a).unwrap();
        r.park(a, &token).unwrap();
        let queued = r.defer(a, "queued").unwrap();
        r.enqueue(queued, a, "queued").unwrap();
        let other = r.defer(b, "other").unwrap();
        let join = r.schedule(other, b, "other").unwrap();
        let survivor = r.tasks[&other].clone();
        r.retire_owner(a, true).unwrap();
        assert_eq!(r.tasks[&other], survivor);
        assert_eq!(r.next_ready(), Some(other));
        assert!(r.wake(&token).is_err());
        assert!(r.join(completed, a, "completed").is_err());
        assert!(r.dispatch(queued).is_err());
        r.dispatch(other).unwrap();
        r.complete(other, b).unwrap();
        r.deliver(other, b, join).unwrap();
        r.consume(other, b).unwrap();
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
    }
    #[test]
    fn readiness_cycles_do_not_complete_or_discharge_task_obligations() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let task = r.defer(a, "work").unwrap();
        assert!(r.prepare_readiness(task, a).is_err());
        let completion = r.schedule(task, a, "work").unwrap();
        r.dispatch(task).unwrap();
        let first = r.prepare_readiness(task, a).unwrap();
        r.park(a, &first).unwrap();
        let before = r.clone();
        assert!(r.complete(task, a).is_err());
        assert!(r.deliver(task, a, completion).is_err());
        assert!(r.retire_owner(a, false).is_err());
        assert!(r.prepare_readiness(task, a).is_err());
        assert_eq!(r, before);
        r.wake(&first).unwrap();
        assert!(r.complete(task, a).is_err());
        r.dispatch(task).unwrap();
        let second = r.prepare_readiness(task, a).unwrap();
        assert!(second.generation > first.generation);
        r.park(a, &second).unwrap();
        let before = r.clone();
        assert!(r.wake(&first).is_err());
        assert_eq!(r, before);
        r.wake(&second).unwrap();
        let before = r.clone();
        assert!(r.wake(&second).is_err());
        assert_eq!(r, before);
        r.dispatch(task).unwrap();
        r.complete(task, a).unwrap();
        r.deliver(task, a, completion).unwrap();
        r.consume(task, a).unwrap();
        assert!(r.wake(&second).is_err());
        r.retire_owner(a, false).unwrap();
        assert!(r.is_empty());
    }
    #[test]
    fn readiness_admission_rejects_foreign_stale_and_exhausted_tokens_atomically() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let task = r.defer(a, "work").unwrap();
        r.schedule(task, a, "work").unwrap();
        r.dispatch(task).unwrap();
        let token = r.prepare_readiness(task, a).unwrap();
        let before = r.clone();
        assert!(r.park(b, &token).is_err());
        assert_eq!(r, before);
        let mut foreign = Registry::default();
        let fa = owner(&mut foreign);
        let ft = foreign.defer(fa, "work").unwrap();
        foreign.schedule(ft, fa, "work").unwrap();
        foreign.dispatch(ft).unwrap();
        let wrong = foreign.prepare_readiness(ft, fa).unwrap();
        assert_eq!(wrong.generation, token.generation);
        assert_eq!(wrong.task, token.task);
        assert!(r.park(a, &wrong).is_err());
        assert_eq!(r, before);
        r.park(a, &token).unwrap();
        let before = r.clone();
        assert!(r.wake(&wrong).is_err());
        assert_eq!(r, before);
        r.wake(&token).unwrap();
        r.dispatch(task).unwrap();
        let before = r.clone();
        assert!(r.park(a, &token).is_err());
        assert_eq!(r, before);
        r.exhaust_readiness_space();
        let before = r.clone();
        assert!(r.prepare_readiness(task, a).is_err());
        assert_eq!(r, before);
    }
    #[test]
    fn emergency_retirement_invalidates_waiting_and_ready_registrations() {
        let mut r = Registry::default();
        let a = owner(&mut r);
        let b = owner(&mut r);
        let mut tokens = vec![];
        for owner in [a, b] {
            let task = r.defer(owner, "work").unwrap();
            r.schedule(task, owner, "work").unwrap();
            r.dispatch(task).unwrap();
            let token = r.prepare_readiness(task, owner).unwrap();
            r.park(owner, &token).unwrap();
            tokens.push(token);
        }
        r.retire_owner(a, true).unwrap();
        assert!(r.wake(&tokens[0]).is_err());
        assert_eq!(r.waiting_task(&tokens[1]).unwrap(), tokens[1].task);
        r.wake(&tokens[1]).unwrap();
        r.abandon(tokens[1].task);
        assert!(r.next_ready().is_none());
        assert!(r.wake(&tokens[1]).is_err());
        r.retire_owner(b, false).unwrap();
        assert!(r.is_empty());
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
