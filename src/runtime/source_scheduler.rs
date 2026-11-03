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
    runnable: VecDeque<Runnable<'a>>,
    handles: BTreeMap<TaskId, started::StartedTask<'a>>,
    joins: BTreeMap<TaskId, (Context<'a>, started::StartedJoin<'a>, &'a [Arm])>,
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
    pub(super) fn yield_source(&mut self) -> Result<()> {
        self.shared.scheduler.enabled = true;
        let caller = std::mem::replace(&mut self.context, Context::new(Default::default()));
        self.shared
            .scheduler
            .runnable
            .push_back(Runnable::Context(caller));
        self.select_source()
    }
    pub(super) fn select_source(&mut self) -> Result<()> {
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
                None => return Err(error("source task wait has no runnable producer")),
            }
        }
    }
    pub(super) fn prune_source(&mut self) {
        self.shared
            .scheduler
            .handles
            .retain(|id, _| self.shared.tasks.contains(*id));
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
        self.wake_source_join(task)?;
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
        if self.context.domain != Some(origin.domain) {
            return Ok(false);
        }
        let Some(Err(diagnostic)) = &self.context.flow else {
            return Ok(false);
        };
        let diagnostic = diagnostic.clone();
        let owner = origin.owner;
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
            if let Some(task) = task {
                self.shared.scheduler.joins.remove(&task).unwrap().0
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
        if self
            .context
            .started
            .as_ref()
            .is_some_and(|origin| self.context.domain == Some(origin.domain))
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
            .filter_map(|(id, (c, _, _))| selected(c).then_some((c.task.map_or(0, |t| t.0.0), *id)))
            .max();
        let context = if let Some((_, i)) =
            runnable.filter(|(id, _)| blocked.is_none_or(|(b, _)| *id >= b))
        {
            let Runnable::Context(c) = self.shared.scheduler.runnable.remove(i).unwrap() else {
                unreachable!()
            };
            c
        } else if let Some((_, id)) = blocked {
            self.shared.scheduler.joins.remove(&id).unwrap().0
        } else {
            return false;
        };
        self.select_settlement(context);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_schedules_retire_every_context_at_every_fuel_cut() {
        for text in [
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
        ]
        .into_iter()
        .map(str::to_owned)
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
