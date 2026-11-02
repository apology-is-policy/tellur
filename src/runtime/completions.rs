//! Normal task results and diagnostic handover remain owned until a one-use join
//! or emergency retirement. Faults never enter this table as ordinary outcomes.
use super::*;

impl<'a> Machine<'a> {
    pub(super) fn retain_task_completion(&mut self, task: TaskId, owner: OwnerId) -> Result<()> {
        self.require_live_process()?;
        if self.shared.completed.contains_key(&task)
            || self.context.task != Some((task, owner))
            || !self.context.stack.is_empty()
            || self.context.frame.owner.is_some()
            || !matches!(self.context.flow, Some(Ok(Flow::Exit(..))))
        {
            return Err(error("task has no new completed payload"));
        }
        let domain = self
            .context
            .domain
            .ok_or_else(|| error("completion has no fault domain"))?;
        self.shared.domains.require_open(domain)?;
        self.shared.tasks.complete(task, owner)?;
        let context = std::mem::replace(&mut self.context, Context::new(Default::default()));
        assert!(self.shared.completed.insert(task, context).is_none());
        Ok(())
    }
    pub(super) fn deliver_task_completion(
        &mut self,
        task: TaskId,
        owner: OwnerId,
        wait: WaitId,
    ) -> Result<Context<'a>> {
        self.require_live_process()?;
        let context = self
            .shared
            .completed
            .get(&task)
            .ok_or_else(|| error("no retained task completion"))?;
        if context.task != Some((task, owner)) {
            return Err(error("completion belongs to another invocation"));
        }
        self.shared.domains.require_open(context.domain.unwrap())?;
        // Reject before taking either the registry obligation or original values.
        self.shared.tasks.deliver(task, owner, wait)?;
        Ok(self.shared.completed.remove(&task).unwrap())
    }
    fn retire_completion_fields(&mut self, context: &mut Context<'a>) {
        let Some(Ok(Flow::Exit(_, fields))) = context.flow.take() else {
            unreachable!()
        };
        for field in fields {
            retire_value(&mut self.shared.tasks, field, true)
                .expect("scripted completion retirement is infallible");
        }
    }
    /// Some(None) retires an unjoined result; Some(Some(caller)) also returns its
    /// already-parked caller for emergency unwind. Neither runs normal caller code.
    pub(super) fn settle_completed_task(
        &mut self,
        domain: Option<DomainId>,
    ) -> Option<Option<Context<'a>>> {
        let task = self.shared.completed.iter().find_map(|(task, context)| {
            domain
                .is_none_or(|domain| {
                    self.shared
                        .domains
                        .contains(domain, context.domain.unwrap())
                })
                .then_some(*task)
        })?;
        let mut completed = self.shared.completed.remove(&task).unwrap();
        // An undelivered outcome cannot lend owned fields to another invocation.
        // End its external result borrows before retiring their source frames.
        self.retire_completion_fields(&mut completed);
        self.shared.tasks.abandon(task);
        let caller = self
            .shared
            .suspended
            .calls
            .iter()
            .find_map(|(id, call)| (call.task == task).then_some((*id, call.wait)));
        Some(caller.map(|(id, wait)| {
            let mut caller = self
                .shared
                .suspended
                .take(id, task, wait)
                .expect("matched suspended completion caller");
            caller.activations = completed.activations;
            caller.phase = completed.phase;
            caller.handover = completed.handover;
            caller
        }))
    }
    pub(super) fn complete_await(
        &mut self,
        task: TaskId,
        owner: OwnerId,
        wait: WaitId,
        flow: Flow,
    ) -> Result<Flow> {
        self.context.flow = Some(Ok(flow));
        if let Err(diagnostic) = self.retain_task_completion(task, owner) {
            let mut context =
                std::mem::replace(&mut self.context, Context::new(Default::default()));
            self.retire_completion_fields(&mut context);
            self.shared.tasks.abandon(task);
            self.context = context;
            return Err(diagnostic);
        }
        match self.deliver_task_completion(task, owner, wait) {
            Ok(context) => {
                self.context = context;
                self.context.flow.take().unwrap()
            }
            Err(diagnostic) => {
                // A checked sequential await prevalidates its paired caller. If
                // an internal invariant still fails, restore diagnostic ownership
                // and settle its payload before propagating the error.
                let mut context = self.shared.completed.remove(&task).unwrap();
                self.retire_completion_fields(&mut context);
                self.shared.tasks.abandon(task);
                self.context = context;
                Err(diagnostic)
            }
        }
    }
}
