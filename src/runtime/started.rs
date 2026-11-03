//! Independent queue ownership and consuming completion authority. These private
//! operations do not select tasks or turn host quotas into source handoffs.
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct StartedOrigin<'a> {
    execution: Arc<()>,
    task: TaskId,
    owner: OwnerId,
    domain: DomainId,
    producer: &'a Region,
}
/// Neither handle is cloneable; only the executor constructs their authority.
pub(super) struct StartedTask<'a>(StartedOrigin<'a>);
pub(super) struct StartedJoin<'a> {
    origin: StartedOrigin<'a>,
    wait: WaitId,
}
impl<'a> Machine<'a> {
    fn started_caller(&self) -> Result<(OwnerId, DomainId)> {
        self.require_live_process()?;
        if self.context.flow.is_some()
            || !self.context.contingencies.is_empty()
            || !self.settlements.is_empty()
        {
            return Err(error("started task requires a normal owning invocation"));
        }
        let owner = self
            .context
            .frame
            .owner
            .ok_or_else(|| error("task owner has retired"))?;
        let domain = self
            .context
            .domain
            .ok_or_else(|| error("task owner has no fault domain"))?;
        self.shared.domains.require_open(domain)?;
        Ok((owner, domain))
    }
    fn require_started_origin(&self, origin: &StartedOrigin<'_>) -> Result<()> {
        let (owner, domain) = self.started_caller()?;
        if !Arc::ptr_eq(&self.shared.identity, &origin.execution) {
            return Err(error("started task belongs to another execution"));
        }
        if owner != origin.owner {
            return Err(error("started task belongs to another invocation"));
        }
        if domain != origin.domain {
            return Err(error("started join crosses its spawning fault domain"));
        }
        Ok(())
    }
    /// Validate before moving any captures. Queueing creates neither a completion
    /// wait nor a suspended caller, and preserves the caller's diagnostic segment.
    pub(super) fn queue_started(
        &mut self,
        producer: &str,
        cold: &mut Option<Value>,
    ) -> Result<StartedTask<'a>> {
        let (owner, domain) = self.started_caller()?;
        let Some(Value::Task {
            id,
            operation,
            arguments,
            ..
        }) = cold.as_ref()
        else {
            return Err(error("started admission requires a cold task"));
        };
        if operation != producer {
            return Err(error("pending task producer differs"));
        }
        let region = self.prepare_call_region(producer, arguments)?;
        let origin = StartedOrigin {
            execution: self.shared.identity.clone(),
            task: TaskId(*id),
            owner,
            domain,
            producer: region,
        };
        // enqueue validates original invocation/producer and Cold phase before
        // its only mutation. Everything after that commit is an infallible move.
        self.shared.tasks.enqueue(origin.task, owner, producer)?;
        let Some(Value::Task { arguments, .. }) = cold.take() else {
            unreachable!()
        };
        let invocation = PreparedInvocation {
            region,
            arguments,
            diagnostic_name: self.shared.program.runtime_name(producer),
        };
        let mut child = Context::new(self.context.activations.child());
        child.task = Some((origin.task, owner));
        child.domain = Some(domain);
        child.started = Some(origin.clone());
        child.phase = self.context.phase.clone();
        assert!(
            self.shared
                .ready
                .insert(
                    origin.task,
                    ReadyCall::Start {
                        invocation,
                        context: child
                    }
                )
                .is_none()
        );
        Ok(StartedTask(origin))
    }
    /// Claims may precede dispatch or follow completion. A nested child boundary
    /// cannot change the originating domain held by this opaque authority.
    pub(super) fn claim_started(
        &mut self,
        producer: &str,
        handle: &mut Option<StartedTask<'a>>,
    ) -> Result<StartedJoin<'a>> {
        let task = handle
            .as_ref()
            .ok_or_else(|| error("started handle already consumed"))?;
        self.require_started_origin(&task.0)?;
        let expected = self
            .shared
            .program
            .region(producer)
            .ok_or_else(|| error("unknown started producer"))?;
        if !std::ptr::eq(expected, task.0.producer) {
            return Err(error("started task producer differs"));
        }
        let wait = self
            .shared
            .tasks
            .join(task.0.task, task.0.owner, producer)?;
        Ok(StartedJoin {
            origin: handle.take().unwrap().0,
            wait,
        })
    }
    /// Deliver and consume once, without replacing the original caller tracker.
    /// Rejections preserve the join, result fields and registry obligation.
    pub(super) fn join_started(&mut self, join: &mut Option<StartedJoin<'a>>) -> Result<Flow> {
        let authority = join
            .as_ref()
            .ok_or_else(|| error("started join already consumed"))?;
        self.require_started_origin(&authority.origin)?;
        let origin = &authority.origin;
        let context = self
            .shared
            .completed
            .get(&origin.task)
            .ok_or_else(|| error("no retained started completion"))?;
        let retained = context
            .started
            .as_ref()
            .ok_or_else(|| error("completion is not independent started work"))?;
        if !Arc::ptr_eq(&retained.execution, &origin.execution)
            || retained.task != origin.task
            || retained.owner != origin.owner
            || retained.domain != origin.domain
            || !std::ptr::eq(retained.producer, origin.producer)
            || context.task != Some((origin.task, origin.owner))
            || context.domain != Some(origin.domain)
        {
            return Err(error("started completion origin differs"));
        }
        self.shared
            .tasks
            .deliver(origin.task, origin.owner, authority.wait)?;
        self.shared
            .tasks
            .consume(origin.task, origin.owner)
            .expect("validated delivered completion");
        let mut completed = self.shared.completed.remove(&origin.task).unwrap();
        join.take();
        Ok(completed
            .flow
            .take()
            .unwrap()
            .expect("only normal outcomes are retained"))
    }
}

#[cfg(test)]
#[path = "started_tests.rs"]
mod tests;
