//! Explicit host queue/claim/delivery authority. Source spawn wiring is separate.
use super::super::super::tasks::WaitId;
use super::*;

/// Move-only completion ownership. The key permits explicit host selection, but
/// cannot claim or deliver this task's result without consuming this handle.
#[derive(Debug)]
pub struct StartedTask {
    key: ContextKey,
}
impl StartedTask {
    pub fn context(&self) -> &ContextKey {
        &self.key
    }
}
#[derive(Debug)]
pub struct TaskJoin {
    pub(super) key: ContextKey,
    pub(super) wait: WaitId,
}
impl TaskJoin {
    pub fn context(&self) -> &ContextKey {
        &self.key
    }
}
impl Contexts<'_> {
    /// Retain captures, domain membership and result ownership while leaving the
    /// task Ready and unclaimed. Neither its body nor another context runs here.
    pub fn retain_task(
        &mut self,
        parent: &ContextKey,
        queued: &mut Option<QueuedTask>,
    ) -> R<StartedTask> {
        self.require(parent)?;
        self.require_independent_children(parent)?;
        let key = self.start_queued(parent, queued, false, true)?;
        Ok(StartedTask { key })
    }
    /// Select a retained Ready task without running a callback. Selection order
    /// is the shared FIFO order; duplicate selection is rejected administratively.
    pub fn dispatch_task(&mut self, key: &ContextKey) -> R<()> {
        self.require(key)?;
        let child = self.parked.get_mut(&key.id).unwrap();
        child.machine.require_runnable(&self.domains)?;
        self.domains.require_open(&child.machine.current, &[])?;
        child
            .task
            .as_mut()
            .ok_or("E_NATIVE: context is not a queued task")?
            .dispatch_retained(self.host)
    }
    fn prepare_task_join(
        &self,
        parent: &ContextKey,
        producer: &'static Control,
        task: &StartedTask,
    ) -> R<WaitId> {
        self.require(parent)?;
        let owner = &self.parked[&parent.id];
        owner.machine.require_runnable(&self.domains)?;
        self.domains.require_open(&owner.machine.current, &[])?;
        if self.waits.contains_key(&parent.id) || owner.awaiting.is_some() {
            return Err("E_NATIVE: waiting owner cannot claim a started task".into());
        }
        let activation = owner
            .machine
            .stack
            .last()
            .ok_or("E_NATIVE: task owner has retired")?;
        activation.require_task_host(self.host)?;
        let child = if self.parked.contains_key(&task.key.id) {
            self.require(&task.key)?;
            &self.parked[&task.key.id]
        } else {
            &self.require_completed(&task.key)?.parked
        };
        if child.parent != Some(parent.id) || child.parent_invocation != Some(activation.owner) {
            return Err("E_NATIVE: started task belongs to another invocation".into());
        }
        if !self
            .domains
            .same(&owner.machine.current, &child.machine.root.key)
        {
            return Err("E_NATIVE: started join crosses its owning fault domain".into());
        }
        self.domains.require_open(&child.machine.root.key, &[])?;
        child
            .task
            .as_ref()
            .ok_or("E_NATIVE: context is not a started task")?
            .prepare_join(self.host, producer)
    }
    /// Claim once in Ready/Running/Waiting/Completed without scheduling anything.
    /// Every recoverable check precedes handle consumption and wait allocation.
    pub fn claim_task(
        &mut self,
        parent: &ContextKey,
        producer: &'static Control,
        task: &mut Option<StartedTask>,
    ) -> R<TaskJoin> {
        let handle = task
            .as_ref()
            .ok_or("E_NATIVE: started handle already consumed")?;
        let wait = self.prepare_task_join(parent, producer, handle)?;
        let child = if let Some(child) = self.parked.get_mut(&handle.key.id) {
            child
        } else {
            &mut self.completed.get_mut(&handle.key.id).unwrap().parked
        };
        assert_eq!(
            child.task.as_mut().unwrap().claim(self.host, producer)?,
            wait
        );
        Ok(TaskJoin {
            key: task.take().unwrap().key,
            wait,
        })
    }
    /// Transfer the retained normal result once. Failed early/foreign/domain
    /// delivery preserves the join handle, payload and all resource obligations.
    pub fn join_task(&mut self, parent: &ContextKey, join: &mut Option<TaskJoin>) -> R<Completion> {
        self.require(parent)?;
        let owner = &self.parked[&parent.id];
        owner.machine.require_runnable(&self.domains)?;
        self.domains.require_open(&owner.machine.current, &[])?;
        if self.waits.contains_key(&parent.id) || owner.awaiting.is_some() {
            return Err("E_NATIVE: waiting owner cannot deliver a started task".into());
        }
        let authority = join
            .as_ref()
            .ok_or("E_NATIVE: started join already consumed")?;
        let result = self.join_inner(Some(parent), &authority.key, false, Some(authority))?;
        join.take();
        Ok(result)
    }
}
