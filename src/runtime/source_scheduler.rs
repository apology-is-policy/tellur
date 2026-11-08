//! FIFO source scheduling. Only explicit await/yield sites select another chain;
//! host instruction quotas merely pause the currently selected continuation.
use super::*;
use std::collections::VecDeque;

enum Runnable<'a> {
    Task(TaskId),
    Context(Context<'a>),
}
#[derive(Default)]
pub(super) struct Scheduler<'a> {
    pub(super) enabled: bool,
    #[cfg(test)]
    pub(super) operations: Option<adapter_operation_model::SourceOperations>,
    runnable: VecDeque<Runnable<'a>>,
    /// Pending async host operations, signalled one per idle point in FIFO order.
    host_waits: VecDeque<HostWait<'a>>,
    handles: BTreeMap<TaskId, started::StartedTask<'a>>,
    joins: BTreeMap<TaskId, (Context<'a>, started::StartedJoin<'a>, &'a [Arm])>,
    /// Callers suspended by `await next` on a task set with no completed member.
    /// The set stays in the caller's slot, so emergency retirement still sees it.
    set_waits: Vec<SetWait<'a>>,
    /// Source completion order of started children, for `await next`.
    completions: BTreeMap<TaskId, u64>,
    clock: u64,
}
/// A task parks through registry readiness. A root chain entering an operation
/// with run has no task; its context is retained here until signalled.
enum HostWait<'a> {
    Task(TaskId, Readiness),
    Root(Context<'a>),
}
impl<'a> HostWait<'a> {
    fn root(&self) -> Option<&Context<'a>> {
        match self {
            HostWait::Root(context) => Some(context),
            HostWait::Task(..) => None,
        }
    }
}
enum Blocked {
    Join(TaskId),
    Set(usize),
    Host(usize),
}
struct SetWait<'a> {
    context: Context<'a>,
    producer: &'a str,
    slot: Id,
    arms: &'a [Arm],
}
fn contains_owner(context: &Context<'_>, owner: OwnerId) -> bool {
    context.frame.owner == Some(owner)
        || context
            .stack
            .iter()
            .any(|c| matches!(c, Continuation::Call { caller, .. } if caller.owner == Some(owner)))
}
impl<'a> Machine<'a> {
    fn retained_source_context(&mut self, task: TaskId) -> Result<&mut Context<'a>> {
        if let Some(call) = self.shared.ready.get_mut(&task) {
            return Ok(match call {
                ReadyCall::Start { context, .. } | ReadyCall::Resume(context) => context,
            });
        }
        if let Some(context) = self.shared.waiting.get_mut(&task) {
            return Ok(context);
        }
        if let Some(context) = self.shared.completed.get_mut(&task) {
            return Ok(context);
        }
        for runnable in &mut self.shared.scheduler.runnable {
            if let Runnable::Context(context) = runnable
                && context.task.is_some_and(|(id, _)| id == task)
            {
                return Ok(context);
            }
        }
        for (context, _, _) in self.shared.scheduler.joins.values_mut() {
            if context.task.is_some_and(|(id, _)| id == task) {
                return Ok(context);
            }
        }
        for wait in &mut self.shared.scheduler.set_waits {
            if wait.context.task.is_some_and(|(id, _)| id == task) {
                return Ok(&mut wait.context);
            }
        }
        for call in self.shared.suspended.calls.values_mut() {
            if call.context.task.is_some_and(|(id, _)| id == task) {
                return Ok(&mut call.context);
            }
        }
        Err(error("stopped await lost its retained context"))
    }
    /// Recover diagnostic ownership only. Awaited values and borrowers remain
    /// retained for ordinary descendant-first emergency settlement.
    fn restore_source_await(&mut self, owner: OwnerId) -> Result<Context<'a>> {
        let (caller, task, wait) = self
            .shared
            .suspended
            .calls
            .iter()
            .find_map(|(id, call)| {
                contains_owner(&call.context, owner).then_some((*id, call.task, call.wait))
            })
            .ok_or_else(|| error("started fault lost its owning source continuation"))?;
        let completed = self.shared.completed.contains_key(&task);
        if !completed
            && !self.retained_source_context(task)?.stack.iter().any(|c| {
                matches!(c, Continuation::ResumeAwait {task: t, wait: w, caller: c}
                if (*t, *w, *c) == (task, wait, caller))
            })
        {
            return Err(error("stopped await has no matching original caller"));
        }
        let mut current = task;
        let mut frames = 0;
        let mut tracker = loop {
            let next = self.shared.suspended.calls.values().find_map(|call| {
                call.context
                    .task
                    .is_some_and(|(id, _)| id == current)
                    .then_some(call.task)
            });
            let context = self.retained_source_context(current)?;
            let mut saved = None;
            for continuation in &mut context.stack {
                match continuation {
                    Continuation::Call { .. } => frames += 1,
                    Continuation::BoundaryWork { parent, .. }
                    | Continuation::BoundaryHandler { parent, .. }
                    | Continuation::SupervisedWork { parent, .. }
                    | Continuation::SupervisedHandler { parent, .. } => {
                        saved = Some(std::mem::take(parent));
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(saved) = saved {
                break saved;
            }
            if let Some(next) = next {
                current = next;
            } else {
                break std::mem::take(&mut context.activations);
            }
        };
        for _ in 0..frames {
            tracker.leave();
        }
        if !completed {
            for continuation in &mut self.retained_source_context(task)?.stack {
                if matches!(continuation, Continuation::ResumeAwait {task: t, wait: w, caller: c}
                    if (*t, *w, *c) == (task, wait, caller))
                {
                    *continuation = Continuation::StoppedAwait(task);
                    break;
                }
            }
        }
        let mut parent = self.shared.suspended.take(caller, task, wait)?;
        parent.activations = tracker;
        Ok(parent)
    }
    pub(super) fn queue_source_await(&mut self, task: TaskId) -> Result<()> {
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Task(task));
        self.select_source()
    }
    pub(super) fn source_spawn(
        &mut self,
        name: &str,
        args: &[Expr],
        arms: &'a [Arm],
    ) -> Result<()> {
        let arguments = self.arguments(args)?;
        let id = self
            .shared
            .tasks
            .defer(self.context.frame.owner.unwrap(), name)?;
        let mut cold = Some(Value::Task {
            id: id.0,
            ty: crate::manifest::task_type(name),
            operation: name.into(),
            arguments,
        });
        let handle = self.queue_started(name, &mut cold)?;
        self.shared.scheduler.enabled = true;
        self.shared.scheduler.handles.insert(id, handle);
        self.shared.scheduler.runnable.push_back(Runnable::Task(id));
        self.bind(
            arms,
            "$pending".into(),
            vec![Value::Task {
                id: id.0,
                ty: crate::manifest::started_type(name),
                operation: name.into(),
                arguments: vec![],
            }],
        )
    }
    pub(super) fn source_join(&mut self, name: &str, args: &[Expr], arms: &'a [Arm]) -> Result<()> {
        let mut args = self.arguments(args)?;
        let Some(Value::Task {
            id,
            operation,
            arguments,
            ..
        }) = args.pop()
        else {
            return Err(error("started join requires an owned task"));
        };
        if !args.is_empty() || !arguments.is_empty() || operation != name {
            return Err(error("started join payload differs"));
        }
        let id = TaskId(id);
        let mut handle = self.shared.scheduler.handles.remove(&id);
        let join = self.claim_started(name, &mut handle)?;
        let caller = std::mem::replace(&mut self.context, Context::new(Default::default()));
        assert!(
            self.shared
                .scheduler
                .joins
                .insert(id, (caller, join, arms))
                .is_none()
        );
        self.wake_source_join(id)?;
        self.select_source()
    }
    pub(super) fn wake_source_join(&mut self, id: TaskId) -> Result<()> {
        if !self.shared.completed.contains_key(&id) {
            return Ok(());
        }
        let Some((caller, join, arms)) = self.shared.scheduler.joins.remove(&id) else {
            return Ok(());
        };
        let selected = std::mem::replace(&mut self.context, caller);
        let flow = self.join_started(&mut Some(join))?;
        let Flow::Exit(outcome, fields) = flow else {
            unreachable!()
        };
        self.bind(arms, outcome, fields)?;
        let caller = std::mem::replace(&mut self.context, selected);
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Context(caller));
        Ok(())
    }
    pub(super) fn source_spawn_into(
        &mut self,
        name: &str,
        args: &[Expr],
        arms: &'a [Arm],
    ) -> Result<()> {
        let mut arguments = self.arguments(args)?;
        let Some(Value::Construct {
            ty,
            name: constructor,
            mut fields,
        }) = arguments.pop()
        else {
            return Err(error("task set spawn requires an owned set"));
        };
        if ty != crate::manifest::task_set_type(name) {
            return Err(error("task set producer differs"));
        }
        let id = self
            .shared
            .tasks
            .defer(self.context.frame.owner.unwrap(), name)?;
        let mut cold = Some(Value::Task {
            id: id.0,
            ty: crate::manifest::task_type(name),
            operation: name.into(),
            arguments,
        });
        let handle = self.queue_started(name, &mut cold)?;
        self.shared.scheduler.enabled = true;
        self.shared.scheduler.handles.insert(id, handle);
        self.shared.scheduler.runnable.push_back(Runnable::Task(id));
        fields.push(Value::Task {
            id: id.0,
            ty: crate::manifest::started_type(name),
            operation: name.into(),
            arguments: vec![],
        });
        self.bind(
            arms,
            "$spawned".into(),
            vec![Value::Construct {
                ty,
                name: constructor,
                fields,
            }],
        )
    }
    /// `cancel target;` flags a started child or every current set member. It
    /// never suspends; parked host operations that observe it are woken in FIFO order.
    pub(super) fn source_cancel(
        &mut self,
        name: &str,
        args: &[Expr],
        arms: &'a [Arm],
    ) -> Result<()> {
        let mut arguments = self.arguments(args)?;
        let target = arguments
            .pop()
            .filter(|_| arguments.is_empty())
            .ok_or_else(|| error("cancellation requires one task or set"))?;
        match &target {
            Value::Task { id, ty, .. } if *ty == crate::manifest::started_type(name) => {
                self.shared.tasks.cancel(TaskId(*id));
            }
            Value::Construct { ty, fields, .. } if *ty == crate::manifest::task_set_type(name) => {
                for member in fields {
                    let Value::Task { id, .. } = member else {
                        return Err(error("task set member is not a started task"));
                    };
                    self.shared.tasks.cancel(TaskId(*id));
                }
            }
            _ => return Err(error("cancellation target is not a started task")),
        }
        self.bind(arms, "$cancel".into(), vec![target])?;
        self.wake_cancelled()
    }
    fn wake_cancelled(&mut self) -> Result<()> {
        let waits = std::mem::take(&mut self.shared.scheduler.host_waits);
        let mut retained = VecDeque::new();
        for wait in waits {
            match wait {
                HostWait::Task(task, token)
                    if self
                        .shared
                        .waiting
                        .get(&task)
                        .is_some_and(|context| self.observes_cancellation(context)) =>
                {
                    self.signal_readiness(&token)?;
                    self.queue_operation_readiness(task);
                }
                // A root chain observes only a cancelled entry (a launch signal).
                HostWait::Root(context) if self.observes_cancellation(&context) => self
                    .shared
                    .scheduler
                    .runnable
                    .push_back(Runnable::Context(context)),
                wait => retained.push_back(wait),
            }
        }
        self.shared.scheduler.host_waits = retained;
        Ok(())
    }
    /// Deliver scripted launch signals that precede the next script entry. With
    /// `launch.<entry>.signals = "cancel"` the first cancels the entry chain and
    /// wakes pending operations that observe it.
    pub(super) fn deliver_script_signals(&mut self) -> Result<bool> {
        let mut delivered = false;
        while let Some(call) = self.shared.calls.get(self.shared.cursor)
            && call.operation == crate::runtime::SCRIPT_SIGNAL
        {
            let status = crate::runtime::signal_status(&call.outcome)?;
            self.shared.cursor += 1;
            if !self.shared.cancels_on_signal {
                return Err(error(format!(
                    "{} terminates the process with status {status}; declare launch.<entry>.signals = \"cancel\" to cancel the entry instead",
                    call.outcome
                )));
            }
            if self.shared.tasks.root_cancelled() {
                return Err(error(format!(
                    "a second signal ({}) terminates the process with status {status}",
                    call.outcome
                )));
            }
            self.shared.tasks.cancel_root();
            self.wake_cancelled()?;
            delivered = true;
        }
        Ok(delivered)
    }
    /// A task is cancelled when it, or a task beneath which it was created, has a
    /// cancellation request. A root chain has no task.
    pub(super) fn task_cancelled(&self, context: &Context<'a>) -> bool {
        match context.task {
            Some((task, _)) => self.shared.tasks.is_cancelled(task),
            None => self.shared.tasks.root_cancelled(),
        }
    }
    /// A parked chain observes cancellation when its pending operation was entered
    /// through a cancellable generated control and its task is cancelled.
    fn observes_cancellation(&self, context: &Context<'a>) -> bool {
        matches!(
            context.stack.last(),
            Some(Continuation::HostOperation(operation)) if operation.cancellable
        ) && self.task_cancelled(context)
    }
    /// `await next`: an empty set selects `empty` without a handoff. Otherwise the
    /// earliest completed member is joined, or the caller waits for one; either
    /// way the caller then rejoins the source FIFO like any consuming await.
    pub(super) fn source_next(
        &mut self,
        name: &'a str,
        args: &[Expr],
        arms: &'a [Arm],
    ) -> Result<()> {
        let [Expr::Move(place)] = args else {
            return Err(error("await next requires an owned task set"));
        };
        let slot = place.root;
        let members = self.task_set_members(name, slot)?;
        if members.is_empty() {
            self.shared
                .values
                .remove(&(self.context.frame.base + slot))
                .unwrap();
            return self.bind(arms, "empty".into(), vec![]);
        }
        let caller = std::mem::replace(&mut self.context, Context::new(Default::default()));
        self.shared.scheduler.set_waits.push(SetWait {
            context: caller,
            producer: name,
            slot,
            arms,
        });
        let index = self.shared.scheduler.set_waits.len() - 1;
        if let Some(member) = self.earliest_completed(&members) {
            self.deliver_set_member(index, member)?;
        }
        self.select_source()
    }
    fn task_set_members(&self, name: &str, slot: Id) -> Result<Vec<TaskId>> {
        let Some(Value::Construct { ty, fields, .. }) =
            self.shared.values.get(&(self.context.frame.base + slot))
        else {
            return Err(error("await next requires an owned task set"));
        };
        if *ty != crate::manifest::task_set_type(name) {
            return Err(error("task set producer differs"));
        }
        fields
            .iter()
            .map(|member| match member {
                Value::Task { id, .. } => Ok(TaskId(*id)),
                _ => Err(error("task set member is not a started task")),
            })
            .collect()
    }
    fn earliest_completed(&self, members: &[TaskId]) -> Option<TaskId> {
        members
            .iter()
            .filter_map(|id| self.shared.scheduler.completions.get(id).map(|n| (*n, *id)))
            .min()
            .map(|(_, id)| id)
    }
    /// Wake a set waiter whose set contains this newly completed child.
    fn wake_source_set(&mut self, task: TaskId) -> Result<()> {
        let mut found = None;
        for (index, wait) in self.shared.scheduler.set_waits.iter().enumerate() {
            let base = wait.context.frame.base;
            if let Some(Value::Construct { fields, .. }) =
                self.shared.values.get(&(base + wait.slot))
                && fields
                    .iter()
                    .any(|m| matches!(m, Value::Task { id, .. } if TaskId(*id) == task))
            {
                found = Some(index);
                break;
            }
        }
        match found {
            Some(index) => self.deliver_set_member(index, task),
            None => Ok(()),
        }
    }
    fn deliver_set_member(&mut self, index: usize, member: TaskId) -> Result<()> {
        let wait = self.shared.scheduler.set_waits.remove(index);
        let selected = std::mem::replace(&mut self.context, wait.context);
        let key = self.context.frame.base + wait.slot;
        let Some(Value::Construct {
            ty,
            name: constructor,
            mut fields,
        }) = self.shared.values.remove(&key)
        else {
            return Err(error("task set wait lost its set"));
        };
        fields.retain(|m| !matches!(m, Value::Task { id, .. } if TaskId(*id) == member));
        let mut handle = self.shared.scheduler.handles.remove(&member);
        let join = self.claim_started(wait.producer, &mut handle)?;
        self.shared.scheduler.completions.remove(&member);
        let Flow::Exit(outcome, mut payload) = self.join_started(&mut Some(join))? else {
            unreachable!()
        };
        payload.push(Value::Construct {
            ty,
            name: constructor,
            fields,
        });
        self.bind(wait.arms, outcome, payload)?;
        let caller = std::mem::replace(&mut self.context, selected);
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Context(caller));
        Ok(())
    }
    pub(super) fn yield_source(&mut self) -> Result<()> {
        self.shared.scheduler.enabled = true;
        let caller = std::mem::replace(&mut self.context, Context::new(Default::default()));
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Context(caller));
        self.select_source()
    }
    #[cfg(test)]
    pub(super) fn assert_source_retired(&self) {
        assert!(self.shared.scheduler.runnable.is_empty());
        assert!(self.shared.scheduler.handles.is_empty());
        assert!(self.shared.scheduler.joins.is_empty());
        assert!(self.shared.scheduler.set_waits.is_empty());
    }
    pub(super) fn queue_operation_readiness(&mut self, task: TaskId) {
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Task(task));
    }
    pub(super) fn select_source(&mut self) -> Result<()> {
        #[cfg(test)]
        self.pump_source_operations(false)?;
        loop {
            match self.shared.scheduler.runnable.pop_front() {
                Some(Runnable::Task(task)) if self.shared.ready.contains_key(&task) => {
                    // The source FIFO and registry FIFO share task admission order.
                    if self.shared.tasks.next_ready() != Some(task) {
                        return Err(error("source queue and task dispatch order differ"));
                    }
                    return self.dispatch_ready();
                }
                Some(Runnable::Task(_)) => {} // Already retired by owning settlement.
                Some(Runnable::Context(context)) => {
                    self.context = context;
                    return Ok(());
                }
                None => {
                    #[cfg(test)]
                    if self.pump_source_operations(true)? {
                        continue;
                    }
                    // An idle point delivers a scripted launch signal first.
                    if self.deliver_script_signals()? {
                        continue;
                    }
                    if self.signal_host_operation()? {
                        continue;
                    }
                    return Err(error("source task wait has no runnable producer"));
                }
            }
        }
    }
    /// Park the selected host-operation task until a later idle point. Readiness
    /// changes eligibility only; the woken task rejoins the source FIFO.
    pub(super) fn park_host_operation(&mut self) -> Result<()> {
        let wait = if let Some((task, _)) = self.context.task {
            HostWait::Task(task, self.park_for_readiness()?)
        } else {
            // A root chain entered the operation with run. It may own started
            // children, which keep running while it waits.
            self.require_live_process()?;
            if self.context.frame.owner.is_none()
                || self.context.flow.is_some()
                || !self.context.contingencies.is_empty()
            {
                return Err(error("readiness requires a live normal continuation"));
            }
            self.shared.domains.require_open(
                self.context
                    .domain
                    .ok_or_else(|| error("running context has no fault domain"))?,
            )?;
            HostWait::Root(std::mem::replace(
                &mut self.context,
                Context::new(Default::default()),
            ))
        };
        self.shared.scheduler.enabled = true;
        self.shared.scheduler.host_waits.push_back(wait);
        self.select_source()
    }
    fn signal_host_operation(&mut self) -> Result<bool> {
        while let Some(wait) = self.shared.scheduler.host_waits.pop_front() {
            match wait {
                HostWait::Task(task, token) => {
                    // Owning settlement may already have retired this operation's task.
                    if !self.shared.waiting.contains_key(&task) {
                        continue;
                    }
                    self.signal_readiness(&token)?;
                    self.queue_operation_readiness(task);
                }
                HostWait::Root(context) => self
                    .shared
                    .scheduler
                    .runnable
                    .push_back(Runnable::Context(context)),
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub(super) fn prune_source(&mut self) {
        let waiting = &self.shared.waiting;
        self.shared.scheduler.host_waits.retain(|wait| match wait {
            HostWait::Task(task, _) => waiting.contains_key(task),
            HostWait::Root(_) => true,
        });
        self.shared
            .scheduler
            .handles
            .retain(|id, _| self.shared.tasks.contains(*id));
        let completed = &self.shared.completed;
        self.shared
            .scheduler
            .completions
            .retain(|id, _| completed.contains_key(id));
        self.shared.scheduler.runnable.retain(|r| match r {
            Runnable::Task(id) => self.shared.ready.contains_key(id),
            Runnable::Context(_) => true,
        });
    }
    pub(super) fn source_complete(&mut self) -> Result<bool> {
        if !self.shared.scheduler.enabled || self.context.started.is_none() {
            return Ok(false);
        }
        let (task, owner) = self.context.task.unwrap();
        self.retain_task_completion(task, owner)?;
        self.shared.scheduler.clock += 1;
        let clock = self.shared.scheduler.clock;
        self.shared.scheduler.completions.insert(task, clock);
        self.wake_source_join(task)?;
        self.wake_source_set(task)?;
        self.select_source()?;
        Ok(true)
    }
    /// Escaping faults are delivered to the original owner before any sibling or
    /// parent frame retires. Its original continuation can be runnable, joining
    /// a started child, or parked behind a sequential cold/direct await.
    pub(super) fn route_source_fault(&mut self) -> Result<bool> {
        if !self.shared.scheduler.enabled || !self.settlements.is_empty() {
            return Ok(false);
        }
        let Some(origin) = self.context.started.as_ref() else {
            return Ok(false);
        };
        // A child fault already routed to this invocation's owning domain must
        // keep forwarding, even while this parent is inside an inline boundary.
        // Settling here would include its own retained ancestor in the subtree.
        if self.context.domain != Some(origin.domain)
            && self.context.source_fault_target != Some(origin.domain)
        {
            return Ok(false);
        }
        let Some(Err(diagnostic)) = &self.context.flow else {
            return Ok(false);
        };
        let diagnostic = diagnostic.clone();
        let owner = origin.owner;
        let domain = origin.domain;
        let name = self.shared.program.runtime_name(&origin.producer.name);
        let parent = if let Some(index) = self
            .shared
            .scheduler
            .runnable
            .iter()
            .position(|r| matches!(r, Runnable::Context(c) if contains_owner(c, owner)))
        {
            let Runnable::Context(c) = self.shared.scheduler.runnable.remove(index).unwrap() else {
                unreachable!()
            };
            c
        } else {
            let task = self
                .shared
                .scheduler
                .joins
                .iter()
                .find_map(|(task, (c, _, _))| contains_owner(c, owner).then_some(*task));
            let set = self
                .shared
                .scheduler
                .set_waits
                .iter()
                .position(|w| contains_owner(&w.context, owner));
            let host = self
                .shared
                .scheduler
                .host_waits
                .iter()
                .position(|w| w.root().is_some_and(|c| contains_owner(c, owner)));
            let waiting = self
                .shared
                .waiting
                .iter()
                .find_map(|(task, c)| contains_owner(c, owner).then_some(*task));
            if let Some(task) = task {
                self.shared.scheduler.joins.remove(&task).unwrap().0
            } else if let Some(index) = set {
                self.shared.scheduler.set_waits.remove(index).context
            } else if let Some(index) = host {
                // A chain parked by run keeps its pending operation on its stack;
                // the routed fault stops it before the owner's unwind.
                let Some(HostWait::Root(context)) = self.shared.scheduler.host_waits.remove(index)
                else {
                    unreachable!()
                };
                context
            } else if let Some(task) = waiting {
                // A task parked by run resumes running to unwind; its readiness
                // registration is withdrawn so no later signal can select it.
                let index = self
                    .shared
                    .scheduler
                    .host_waits
                    .iter()
                    .position(|w| matches!(w, HostWait::Task(t, _) if *t == task))
                    .ok_or_else(|| error("parked owner has no host readiness"))?;
                let Some(HostWait::Task(_, token)) = self.shared.scheduler.host_waits.remove(index)
                else {
                    unreachable!()
                };
                self.shared.tasks.withdraw(&token)?;
                self.shared.waiting.remove(&task).unwrap()
            } else {
                self.restore_source_await(owner)?
            }
        };
        let source_fault = self.context.activations.is_source_fault();
        let report = self
            .context
            .source_cause
            .take()
            .unwrap_or_else(|| self.report(&name, &diagnostic));
        let child = std::mem::replace(&mut self.context, parent);
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Context(child));
        self.context.source_fault_target = Some(domain);
        self.shared.domains.stop(domain)?;
        if self
            .context
            .started
            .as_ref()
            .is_some_and(|origin| domain == origin.domain)
        {
            self.context.source_cause = Some(report.clone());
        }
        self.context.activations.with_cause(report);
        if source_fault {
            self.context.activations.source_fault();
        }
        self.context.flow = Some(Err(diagnostic));
        self.context.stack.push(Continuation::SourceStop);
        Ok(true)
    }
    pub(super) fn source_retained(&self, domain: DomainId) -> bool {
        self.shared.scheduler.runnable.iter().any(|r| {
            matches!(r,
            Runnable::Context(c) if c.domain == Some(domain))
        }) || self
            .shared
            .scheduler
            .joins
            .values()
            .any(|(c, _, _)| c.domain == Some(domain))
            || self
                .shared
                .scheduler
                .set_waits
                .iter()
                .any(|w| w.context.domain == Some(domain))
            || self
                .shared
                .scheduler
                .host_waits
                .iter()
                .filter_map(HostWait::root)
                .any(|c| c.domain == Some(domain))
    }
    pub(super) fn settle_source(&mut self, domain: Option<DomainId>) -> bool {
        let selected = |c: &Context<'a>| {
            c.domain
                .is_some_and(|id| domain.is_none_or(|d| self.shared.domains.contains(d, id)))
        };
        // Created descendants have greater task IDs. Retire them before owners.
        let runnable = self
            .shared
            .scheduler
            .runnable
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                Runnable::Context(c) if selected(c) => Some((c.task.map_or(0, |t| t.0.0), i)),
                _ => None,
            })
            .max();
        let blocked = self
            .shared
            .scheduler
            .joins
            .iter()
            .filter_map(|(id, (c, _, _))| {
                selected(c).then_some((c.task.map_or(0, |t| t.0.0), Blocked::Join(*id)))
            })
            .chain(
                self.shared
                    .scheduler
                    .set_waits
                    .iter()
                    .enumerate()
                    .filter_map(|(i, w)| {
                        selected(&w.context)
                            .then_some((w.context.task.map_or(0, |t| t.0.0), Blocked::Set(i)))
                    }),
            )
            .chain(
                self.shared
                    .scheduler
                    .host_waits
                    .iter()
                    .enumerate()
                    .filter_map(|(i, w)| {
                        w.root()
                            .filter(|c| selected(c))
                            .map(|c| (c.task.map_or(0, |t| t.0.0), Blocked::Host(i)))
                    }),
            )
            .max_by_key(|(task, _)| *task);
        let context = if let Some((_, i)) =
            runnable.filter(|(id, _)| blocked.as_ref().is_none_or(|(b, _)| *id >= *b))
        {
            let Runnable::Context(c) = self.shared.scheduler.runnable.remove(i).unwrap() else {
                unreachable!()
            };
            c
        } else if let Some((_, waiting)) = blocked {
            match waiting {
                Blocked::Join(id) => self.shared.scheduler.joins.remove(&id).unwrap().0,
                Blocked::Set(i) => self.shared.scheduler.set_waits.remove(i).context,
                Blocked::Host(i) => {
                    let Some(HostWait::Root(context)) = self.shared.scheduler.host_waits.remove(i)
                    else {
                        unreachable!()
                    };
                    context
                }
            }
        } else {
            return false;
        };
        self.select_settlement(context);
        true
    }
}

/// Test-only census for adapter-contract models with explicit scripted effects.
#[cfg(test)]
pub(crate) fn census_model(p: &CheckedProgram, scenario: &Scenario) -> RunCompletion {
    for fuel in 0..256 {
        let mut expected = None;
        let mut completed = None;
        for quota in [1, 3, usize::MAX] {
            let mut session = p.start_execution(scenario, fuel);
            let mut m = session.machine.take().unwrap();
            m.enter(scenario).unwrap();
            let mut polls = 0;
            while !m.drive(quota) {
                polls += 1;
                assert!(polls < 1000, "fuel={fuel}, quota={quota}");
            }
            m.prune_source();
            super::settlement_tests::retired(&m);
            assert!(m.shared.scheduler.runnable.is_empty());
            assert!(m.shared.scheduler.joins.is_empty());
            assert!(m.shared.scheduler.set_waits.is_empty());
            assert!(m.shared.scheduler.handles.is_empty());
            let observation = match m.finish(&scenario.entry) {
                Ok(result) => {
                    let value = serde_json::to_value(&result).unwrap();
                    completed = Some(result);
                    value
                }
                Err(failure) => {
                    assert_eq!(failure.diagnostic.code, "E_FUEL", "{failure:?}");
                    serde_json::json!({"report":failure.report,"events":failure.events})
                }
            };
            if let Some(expected) = &expected {
                assert_eq!(&observation, expected, "fuel={fuel}, quota={quota}");
            } else {
                expected = Some(observation);
            }
        }
        if let Some(result) = completed {
            return result;
        }
    }
    panic!("resource model did not complete");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_boundary_handoffs_retire_every_context_at_every_fuel_cut() {
        checked_boundary_census(crate::boundary_handoff_model::program);
    }
    #[test]
    fn checked_recovered_boundary_handoffs_retire_every_context_at_every_fuel_cut() {
        checked_boundary_census(crate::boundary_handoff_model::recovery_program);
    }
    fn checked_boundary_census(build: fn(bool, bool) -> CheckedProgram) {
        for named in [false, true] {
            for awaited in [false, true] {
                let p = build(named, awaited);
                for p in [p.clone(), p.expanded("main").unwrap()] {
                    for fuel in 0..256 {
                        let mut finished = false;
                        for quota in [1, 3, usize::MAX] {
                            let scenario = Scenario {
                                entry: "main".into(),
                                arguments: vec![],
                                calls: vec![],
                            };
                            let mut session = p.start_execution(&scenario, fuel);
                            let mut m = session.machine.take().unwrap();
                            m.enter(&scenario).unwrap();
                            let mut polls = 0;
                            while !m.drive(quota) {
                                polls += 1;
                                assert!(
                                    polls < 1000,
                                    "named={named}, awaited={awaited}, fuel={fuel}, quota={quota}"
                                );
                            }
                            m.prune_source();
                            super::super::settlement_tests::retired(&m);
                            assert!(
                                m.shared.scheduler.runnable.is_empty()
                                    && m.shared.scheduler.joins.is_empty()
                                    && m.shared.scheduler.set_waits.is_empty()
                                    && m.shared.scheduler.handles.is_empty()
                            );
                            finished = m.finish("main").is_ok();
                        }
                        if finished {
                            break;
                        }
                        assert!(fuel < 255);
                    }
                }
            }
        }
    }
    #[test]
    fn source_schedules_retire_every_context_at_every_fuel_cut() {
        for text in [
            include_str!("../../playground/boundary_helpers.tellur"),
            include_str!("../../playground/boundary_handoff_tasks.tellur"),
            include_str!("../../tests/fixtures/source_boundary_helper_fault.tellur"),
            include_str!("../../playground/structured_tasks.tellur"),
            include_str!("../../tests/fixtures/source_task_tree.tellur"),
            include_str!("../../tests/fixtures/source_task_fault.tellur"),
            include_str!("../../tests/fixtures/source_task_grandchild_fault.tellur"),
            include_str!("../../playground/structured_helpers.tellur"),
            include_str!("../../tests/fixtures/source_helper_fault.tellur"),
            include_str!("../../playground/parent_await.tellur"),
            include_str!("../../playground/cooperative_helpers.tellur"),
            include_str!("../../playground/borrowed_spawn.tellur"),
            include_str!("../../playground/carrier_spawn.tellur"),
            include_str!("../../playground/carrier_spawn_results.tellur"),
            include_str!("../../playground/nested_spawn.tellur"),
            include_str!("../../playground/tagged_spawn.tellur"),
            include_str!("../../playground/outcome_spawn.tellur"),
            include_str!("../../playground/borrowed_record_spawn.tellur"),
            include_str!("../../playground/borrowed_tagged_spawn.tellur"),
            include_str!("../../playground/borrowed_outcome_spawn.tellur"),
            include_str!("../../playground/borrowed_record_spawn_results.tellur"),
            include_str!("../../playground/borrowed_tagged_spawn_results.tellur"),
            include_str!("../../playground/borrowed_outcome_spawn_results.tellur"),
            include_str!("../../playground/record_chain_spawn.tellur"),
            include_str!("../../playground/tagged_chain_spawn.tellur"),
            include_str!("../../playground/outcome_chain_spawn.tellur"),
            include_str!("../../tests/fixtures/source_outcome_chain_guards.tellur"),
            include_str!("../../tests/fixtures/source_outcome_chain_nested.tellur"),
            include_str!("../../tests/fixtures/source_outcome_chain_owned.tellur"),
            include_str!("../../tests/fixtures/source_outcome_chain_fault.tellur"),

            include_str!("../../tests/fixtures/source_tagged_chain_option.tellur"),
            include_str!("../../tests/fixtures/source_tagged_chain_variant.tellur"),
            include_str!("../../tests/fixtures/source_tagged_chain_fault.tellur"),

            include_str!("../../tests/fixtures/source_record_chain_guards.tellur"),
            include_str!("../../tests/fixtures/source_record_chain_results.tellur"),
            include_str!("../../tests/fixtures/source_record_chain_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_outcome_result_guards.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_outcome_result_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_outcome_result_nested.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_tagged_result_guards.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_tagged_result_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_tagged_result_outcomes.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_record_result_tags.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_record_result_guards.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_record_result_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_outcome_guards.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_outcome_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_tagged_guards.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_tagged_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_record_guards.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_record_fault.tellur"),
            include_str!("../../tests/fixtures/source_outcome_guards.tellur"),
            include_str!("../../tests/fixtures/source_outcome_fault.tellur"),
            include_str!("../../tests/fixtures/source_tagged_guards.tellur"),
            include_str!("../../tests/fixtures/source_tagged_fault.tellur"),
            include_str!("../../tests/fixtures/source_nested_guards.tellur"),
            include_str!("../../tests/fixtures/source_nested_fault.tellur"),
            include_str!("../../tests/fixtures/source_carrier_result_guards.tellur"),
            include_str!("../../tests/fixtures/source_carrier_result_fault.tellur"),
            include_str!("../../tests/fixtures/source_carrier_guards.tellur"),
            include_str!("../../tests/fixtures/source_carrier_fault.tellur"),
            include_str!("../../playground/borrowed_spawn_results.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_result_fault.tellur"),
            include_str!("../../tests/fixtures/source_borrowed_task_fault.tellur"),
            include_str!("../../tests/fixtures/source_inline_helper_fault.tellur"),
            include_str!("../../tests/fixtures/source_parent_await_fault.tellur"),
            include_str!("../../playground/task_sets.tellur"),
            include_str!("../../tests/fixtures/source_task_set_fault.tellur"),
            include_str!("../../tests/fixtures/source_task_set_parent_fault.tellur"),
        ]
        .into_iter()
        .map(str::to_owned)
        // Members complete or fault before the parent first awaits its set.
        .chain([
            include_str!("../../playground/task_sets.tellur").replace(
                "    loop reap(",
                "    yield;\n    yield;\n    yield;\n    yield;\n    loop reap(",
            ),
            include_str!("../../tests/fixtures/source_task_set_fault.tellur")
                .replace(
                    "spawn work(\"slow\", false) into active;",
                    "spawn work(\"slow\", false) into active;\n    spawn work(\"fast\", false) into active;",
                )
                .replace(
                    "    loop reap(",
                    "    yield;\n    yield;\n    yield;\n    yield;\n    yield;\n    loop reap(",
                ),
        ])
        .chain(
            [
                "auto nested=spawn idle();await nested;",
                "await leaf(text);",
                "auto cold=defer leaf(text);await cold;",
            ]
            .map(|body| {
                include_str!("../../tests/fixtures/source_inline_helper_fault.tellur").replace(
                    "emit(\"inline started\");\n    yield;\n    yield;",
                    &format!("emit(\"inline started\");{body}"),
                )
            }),
        )
        .chain([
            include_str!("../../playground/guarded_task_captures.tellur")
                .replace("defer ", "spawn ")
                .replace(
                    "auto moved = move pending;",
                    "yield;yield;auto moved = move pending;",
                ),
        ])
        .chain([include_str!("../../tests/fixtures/source_record_chain_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_tagged_chain_option.tellur"),include_str!("../../tests/fixtures/source_tagged_chain_variant.tellur")].into_iter().flat_map(|s| [s.replace("some { borrow mutable owner }", "none {}").replace("choice.held { borrow mutable owner }", "choice.empty {}").replace("auto moved = move child;", "owner=\"available\";auto moved = move child;"),s.replace("exit ready { borrow mutable deref input.item };", "exit ready { input };").replace("delta value {", "delta value.item {")]))
        .chain([include_str!("../../tests/fixtures/source_tagged_chain_fault.tellur").replace("    yield;\n    yield;\n    delta await result", "    delta await result").replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_outcome_chain_guards.tellur")].into_iter().flat_map(|s| [s.replace("choice.held { borrow mutable owner }", "choice.empty {}").replace("auto moved = move child;", "owner=\"available\";auto moved = move child;"),s.replace("exit ready { borrow mutable deref input.item };", "exit ready { input };").replace("delta value {", "delta value.item {")]))
        .chain([include_str!("../../tests/fixtures/source_outcome_chain_fault.tellur").replace("    yield;\n    yield;\n    delta await result", "    delta await result").replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_record_chain_guards.tellur")
            .replace("exit ready { borrow mutable deref input.inner.item };", "exit ready { input };")
            .replace("borrow mutable deref value.text", "borrow mutable deref value.inner.item.text")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_outcome_result_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_tagged_result_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_record_result_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_outcome_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_tagged_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_record_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_outcome_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_tagged_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_nested_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_carrier_result_fault.tellur")
            .replace("    yield;\n    yield;\n    delta await result", "    delta await result")
            .replace("emit(\"stopped parent resumed\");", "yield;yield;")])
        .chain([include_str!("../../tests/fixtures/source_borrowed_result_fault.tellur")
            .replace("control main()", "async control outer() exits done faults {await startup();exit done;} control main()")
            .replace("run startup();", "run outer();")]) {
            let p = crate::compile(
                &text,
                Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
            )
            .unwrap();
            for p in [p.clone(), p.expanded("main").unwrap()] {
                for fuel in 0..256 {
                    let mut finished = false;
                    for quota in [1, 3, usize::MAX] {
                        let scenario = Scenario {
                            entry: "main".into(),
                            arguments: vec![],
                            calls: vec![],
                        };
                        let mut session = p.start_execution(&scenario, fuel);
                        let mut m = session.machine.take().unwrap();
                        m.enter(&scenario).unwrap();
                        let mut polls = 0;
                        while !m.drive(quota) {
                            polls += 1;
                            assert!(
                                polls < 1000,
                                "fuel={fuel}, quota={quota}, owner={:?}, domain={:?}, task={:?}, settlements={}, suspended={}",
                                m.context.frame.owner,
                                m.context.domain,
                                m.context.task,
                                m.settlements.len(),
                                m.shared.suspended.calls.len()
                            );
                        }
                        m.prune_source();
                        assert!(
                            m.shared.values.is_empty() && m.shared.tasks.is_empty(),
                            "fuel={fuel}"
                        );
                        assert!(
                            m.shared.ready.is_empty()
                                && m.shared.waiting.is_empty()
                                && m.shared.completed.is_empty()
                        );
                        assert!(m.shared.suspended.calls.is_empty() && m.settlements.is_empty());
                        assert!(
                            m.shared.scheduler.runnable.is_empty()
                                && m.shared.scheduler.joins.is_empty()
                                && m.shared.scheduler.set_waits.is_empty()
                                && m.shared.scheduler.completions.is_empty()
                                && m.shared.scheduler.handles.is_empty()
                        );
                        assert_eq!(m.shared.domains.count(), 0);
                        finished = m.finish("main").is_ok();
                    }
                    if finished {
                        break;
                    }
                    assert!(fuel < 255);
                }
            }
        }
    }
}
