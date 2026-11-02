//! Retained native contexts, completions and domain-local/session-wide retirement.
use super::super::{ContextRoot, HostContext, fault};
use super::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct ContextKey {
    session: Arc<()>,
    id: u64,
}
#[derive(Clone, Debug)]
pub struct Readiness {
    session: Arc<()>,
    context: u64,
    generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct WaitState {
    generation: u64,
    signalled: bool,
    task: Option<super::super::tasks::Readiness>,
}
struct Parked {
    machine: Machine,
    host: HostContext,
    root: ContextRoot,
    name: &'static str,
    parent: Option<u64>,
    started: bool,
    task: Option<QueuedTask>,
    sequential: bool,
    awaiting: Option<u64>,
}
struct Completed {
    parked: Parked,
    outcome: Completion,
}
pub enum ContextPoll {
    Paused,
    Await,
    Resumed(ContextKey),
    Ready,
    Complete(Completion),
    Failed(fault::Report),
}
pub struct Contexts<'a> {
    host: &'a mut Host,
    values: Frames,
    domains: Domains,
    identity: Arc<()>,
    next: u64,
    parked: BTreeMap<u64, Parked>,
    terminal: BTreeMap<u64, ContextPoll>,
    completed: BTreeMap<u64, Completed>,
    terminated: bool,
    next_readiness: u64,
    waits: BTreeMap<u64, WaitState>,
}
impl<'a> Contexts<'a> {
    pub fn new(host: &'a mut Host) -> Self {
        Self {
            host,
            values: Frames::default(),
            domains: Domains::default(),
            identity: Arc::new(()),
            next: 0,
            parked: BTreeMap::new(),
            terminal: BTreeMap::new(),
            completed: BTreeMap::new(),
            terminated: false,
            next_readiness: 0,
            waits: BTreeMap::new(),
        }
    }
    pub fn start(&mut self, control: &'static Control, arguments: Vec<V>) -> R<ContextKey> {
        self.require_live()?;
        let id = self
            .next
            .checked_add(1)
            .ok_or("E_NATIVE: context identity exhausted")?;
        if control.constructor || arguments.len() != control.parameters.len() {
            return Err("E_NATIVE: invalid independent context admission".into());
        }
        // Validate identity capacity before transferring any input owners.
        self.domains.prepare(None)?;
        self.domains.require_member_capacity()?;
        self.host.tasks.prepare_owner()?;
        let (mut root, mut context) = self.host.prepare_context(&arguments)?;
        self.host.exchange_context(&mut context)?;
        let started = Machine::start(
            self.host,
            &mut self.values,
            &mut self.domains,
            control,
            arguments,
        );
        if started.is_err() {
            self.host.finish_context(&mut root, true)?;
        }
        self.host.exchange_context(&mut context)?;
        let mut machine = started?;
        machine.retain_completion = true;
        self.next = id;
        self.parked.insert(
            id,
            Parked {
                machine,
                host: context,
                root,
                name: control.name,
                parent: None,
                started: false,
                task: None,
                sequential: false,
                awaiting: None,
            },
        );
        Ok(ContextKey {
            session: self.identity.clone(),
            id,
        })
    }
    fn require_live(&self) -> R<()> {
        if self.terminated {
            return Err("E_NATIVE: process session has terminated".into());
        }
        Ok(())
    }
    fn require(&self, key: &ContextKey) -> R<()> {
        self.require_live()?;
        if !Arc::ptr_eq(&self.identity, &key.session) || !self.parked.contains_key(&key.id) {
            return Err("E_NATIVE: context is foreign or retired".into());
        }
        let parked = &self.parked[&key.id];
        parked.machine.require_domain(&self.domains)?;
        self.host.require_context(&parked.host, &parked.root)
    }
    pub fn start_child(
        &mut self,
        parent: &ContextKey,
        control: &'static Control,
        arguments: Vec<V>,
    ) -> R<ContextKey> {
        self.require(parent)?;
        self.require_independent_children(parent)?;
        if self.waits.contains_key(&parent.id) {
            return Err(
                "E_NATIVE: waiting or unselected ready parent cannot admit children".into(),
            );
        }
        let owner = self.parked[&parent.id].machine.current.clone();
        let membership = self.parked[&parent.id].machine.member.clone();
        self.domains.require_open(&owner, &[])?;
        self.domains.require_member_capacity()?;
        self.host.tasks.prepare_owner()?;
        let id = self
            .next
            .checked_add(1)
            .ok_or("E_NATIVE: context identity exhausted")?;
        if control.constructor || arguments.len() != control.parameters.len() {
            return Err("E_NATIVE: invalid related context admission".into());
        }
        // Derive the child's diagnostics from the original selected parent.
        let parent_state = &mut self.parked.get_mut(&parent.id).unwrap().host;
        self.host.exchange_context(parent_state)?;
        let prepared = self.host.prepare_context(&arguments);
        self.host.exchange_context(parent_state)?;
        let (mut root, mut context) = prepared?;
        self.host.exchange_context(&mut context)?;
        let started = Machine::start_related(
            self.host,
            &mut self.values,
            &mut self.domains,
            control,
            arguments,
            &owner,
            &membership,
        );
        if started.is_err() {
            self.host.finish_context(&mut root, true)?;
        }
        self.host.exchange_context(&mut context)?;
        let mut machine = started?;
        machine.retain_completion = true;
        self.next = id;
        self.parked.insert(
            id,
            Parked {
                machine,
                host: context,
                root,
                name: control.name,
                parent: Some(parent.id),
                started: false,
                task: None,
                sequential: false,
                awaiting: None,
            },
        );
        Ok(ContextKey {
            session: self.identity.clone(),
            id,
        })
    }
    /// Admit the next queued child without moving captures on any failed check.
    /// This is explicit host selection, not a source scheduler handoff.
    pub fn start_task(
        &mut self,
        parent: &ContextKey,
        queued: &mut Option<QueuedTask>,
    ) -> R<ContextKey> {
        self.require(parent)?;
        self.require_independent_children(parent)?;
        self.start_queued(parent, queued, false)
    }
    fn require_independent_children(&self, parent: &ContextKey) -> R<()> {
        let parked = &self.parked[&parent.id];
        if parked.sequential || parked.awaiting.is_some() {
            return Err("E_NATIVE: sequential await cannot mix independent child contexts".into());
        }
        Ok(())
    }
    /// Sequential cold await carries the caller's diagnostic chain, rather than
    /// creating a distinct child fault cause. Source callbacks are wired separately.
    pub fn start_await(
        &mut self,
        parent: &ContextKey,
        queued: &mut Option<QueuedTask>,
    ) -> R<ContextKey> {
        self.require(parent)?;
        let parked = &self.parked[&parent.id];
        if (parked.parent.is_some() && !parked.sequential)
            || parked.awaiting.is_some()
            || self.domains.has_children(&parked.machine.member)?
        {
            return Err("E_NATIVE: sequential await requires an exclusive caller chain".into());
        }
        self.start_queued(parent, queued, true)
    }
    fn start_queued(
        &mut self,
        parent: &ContextKey,
        queued: &mut Option<QueuedTask>,
        sequential: bool,
    ) -> R<ContextKey> {
        self.require(parent)?;
        if self.waits.contains_key(&parent.id) {
            return Err("E_NATIVE: waiting parent cannot admit a task".into());
        }
        let parked = &self.parked[&parent.id];
        parked.machine.require_runnable(&self.domains)?;
        let activation = parked
            .machine
            .stack
            .last()
            .ok_or("E_NATIVE: task parent has no invocation")?;
        let task = queued
            .as_ref()
            .ok_or("E_NATIVE: queued task already admitted")?;
        let (control, arguments) = task.prepare_context(self.host, activation)?;
        if control.constructor
            || arguments.len() != control.parameters.len()
            || control.parameters.iter().any(|slot| *slot >= control.slots)
            || control
                .parameters
                .iter()
                .enumerate()
                .any(|(i, slot)| control.parameters[..i].contains(slot))
        {
            return Err("E_NATIVE: invalid queued producer layout".into());
        }
        let id = self
            .next
            .checked_add(1)
            .ok_or("E_NATIVE: context identity exhausted")?;
        let domain = parked.machine.current.clone();
        self.domains.require_open(&domain, &[])?;
        self.domains.require_member_capacity()?;
        let root_domain = self.domains.scope(&domain)?;
        let parent_member = parked.machine.member.clone();
        let owner = self.host.tasks.prepare_owner()?;
        self.host.resources.current_scope()?;
        // Validate the whole capture owner graph, including foreign and duplicate handles.
        self.host.resources.result_tokens(arguments)?;
        let scope = self.host.resources.prepare_scope_at(arguments, None)?;
        let frame = self.values.prepare(control.slots)?;
        let tracker = if sequential {
            let tracker = &mut self.parked.get_mut(&parent.id).unwrap().host.tracker;
            // enter fails before altering the active chain. All other admission
            // checks have passed before the original tracker leaves the caller.
            tracker.enter(control.name)?;
            std::mem::take(tracker)
        } else {
            let mut tracker = parked.host.tracker.child();
            tracker.enter(control.name)?;
            tracker
        };
        // All recoverable checks precede dispatch and capture/resource transfer.
        // Exclusive context access prevents an intervening admission.
        let mut task = queued.take().unwrap();
        let (_, arguments) = task.dispatch(self.host).expect("prevalidated ready task");
        let frame = self.values.commit(frame).expect("prevalidated child frame");
        self.host.tasks.commit_owner(owner);
        let resource_scope = self.host.resources.register_scope(scope);
        let member = self
            .domains
            .attach(&domain, Some(&parent_member))
            .expect("prevalidated domain membership");
        for (slot, value) in control.parameters.iter().zip(arguments) {
            self.values[frame.base() + slot] = Some(value);
        }
        let activation = Activation {
            frame,
            owner,
            registry: self.host.resources.identity.clone(),
            control,
            pending: None,
            scopes: vec![],
            facts: vec![false; control.atoms],
            pc: 0,
            destination: None,
        };
        let machine = Machine {
            stack: vec![activation],
            finished: false,
            root: root_domain,
            current: domain,
            member,
            owns_root: false,
            retain_completion: true,
            fault: None,
            handover: vec![],
            fault_target: None,
        };
        self.parked.insert(
            id,
            Parked {
                machine,
                host: HostContext {
                    registry: self.host.resources.identity.clone(),
                    tracker,
                    contingencies: vec![],
                    scope: Some(resource_scope.clone()),
                },
                root: ContextRoot {
                    scope: resource_scope,
                    retired: false,
                },
                name: control.name,
                parent: Some(parent.id),
                started: false,
                task: Some(task),
                sequential,
                awaiting: None,
            },
        );
        self.next = id;
        if sequential {
            self.parked.get_mut(&parent.id).unwrap().awaiting = Some(id);
        }
        Ok(ContextKey {
            session: self.identity.clone(),
            id,
        })
    }
    pub fn park_for_readiness(&mut self, key: &ContextKey) -> R<Readiness> {
        self.require(key)?;
        let parked = &self.parked[&key.id];
        if !parked.started
            || parked.awaiting.is_some()
            || parked.machine.fault.is_some()
            || !parked.host.contingencies.is_empty()
            || self.waits.contains_key(&key.id)
        {
            return Err("E_NATIVE: readiness requires a live normal selected continuation".into());
        }
        self.domains.require_open(&parked.machine.current, &[])?;
        let generation = self
            .next_readiness
            .checked_add(1)
            .ok_or("E_NATIVE: readiness generation exhausted")?;
        let task = parked
            .task
            .as_ref()
            .map(|task| task.prepare_readiness(self.host))
            .transpose()?;
        if let Some(readiness) = &task {
            parked.task.as_ref().unwrap().park(self.host, readiness)?;
        }
        self.waits.insert(
            key.id,
            WaitState {
                generation,
                signalled: false,
                task,
            },
        );
        self.next_readiness = generation;
        Ok(Readiness {
            session: self.identity.clone(),
            context: key.id,
            generation,
        })
    }
    pub fn signal_readiness(&mut self, token: &Readiness) -> R<()> {
        let key = ContextKey {
            session: token.session.clone(),
            id: token.context,
        };
        self.require(&key)?;
        self.domains
            .require_open(&self.parked[&key.id].machine.current, &[])?;
        let state = self
            .waits
            .get_mut(&key.id)
            .ok_or("E_NATIVE: context has no readiness registration")?;
        if state.generation != token.generation || state.signalled {
            return Err("E_NATIVE: stale or already signalled readiness".into());
        }
        // Signalling only changes eligibility. The original machine and host
        // context stay parked until an explicit positive-quota selection.
        if let Some(readiness) = &state.task {
            self.host.tasks.wake(readiness)?;
        }
        state.signalled = true;
        Ok(())
    }
    pub fn advance(&mut self, key: &ContextKey, quota: usize) -> R<ContextPoll> {
        let result = self.advance_retained(key, quota)?;
        if matches!(result, ContextPoll::Ready) {
            let parent = self.completed[&key.id].parked.parent.map(|id| ContextKey {
                session: self.identity.clone(),
                id,
            });
            return Ok(ContextPoll::Complete(self.join(parent.as_ref(), key)?));
        }
        Ok(result)
    }
    fn await_continuation(&mut self, key: &ContextKey) -> R<&mut AwaitContinuation> {
        self.require(key)?;
        match self
            .parked
            .get_mut(&key.id)
            .unwrap()
            .machine
            .stack
            .last_mut()
            .and_then(|a| a.pending.as_mut())
        {
            Some(Pending::Await(pending)) => Ok(pending),
            _ => Err("E_NATIVE: context has no compiled await continuation".into()),
        }
    }
    /// Admit the captured source handoff once. A failed preflight restores the
    /// exact payload in the original activation, permitting retry or unwind.
    pub fn admit_await(&mut self, parent: &ContextKey) -> R<ContextKey> {
        let pending = self.await_continuation(parent)?;
        if pending.child.is_some() || pending.result.is_some() || pending.queued.is_none() {
            return Err("E_NATIVE: await handoff is already admitted or resolved".into());
        }
        let mut queued = pending.queued.take();
        let result = self.start_await(parent, &mut queued);
        let pending = self
            .await_continuation(parent)
            .expect("retained await caller");
        pending.queued = queued;
        if let Ok(child) = &result {
            pending.child = Some(child.clone());
        }
        result
    }
    /// Record an admission failure for normal caller unwind on selection. The
    /// pending captures remain available until that unwind retires them.
    pub fn reject_await(&mut self, parent: &ContextKey, message: String) -> R<()> {
        let pending = self.await_continuation(parent)?;
        if pending.child.is_some() || pending.result.is_some() || pending.queued.is_none() {
            return Err("E_NATIVE: await handoff is already admitted or resolved".into());
        }
        pending.result = Some(Err(message));
        Ok(())
    }
    fn callback_child(&self, parent: u64, child: &ContextKey) -> bool {
        matches!(self.parked.get(&parent).and_then(|p| p.machine.stack.last()).and_then(|a| a.pending.as_ref()),
            Some(Pending::Await(p)) if p.result.is_none() && p.child.as_ref().is_some_and(|key| key.id == child.id && Arc::ptr_eq(&key.session, &child.session)))
    }
    /// Adopt a normal result and retire its wait before queuing the original
    /// one-use delivery. The callback runs administratively on caller selection.
    pub fn return_await(&mut self, parent: &ContextKey, child: &ContextKey) -> R<()> {
        self.require(parent)?;
        if !self.callback_child(parent.id, child) {
            return Err("E_NATIVE: completion does not match the compiled await".into());
        }
        let result = self.join_inner(Some(parent), child, true)?;
        self.await_continuation(parent)?.result = Some(Ok(result));
        Ok(())
    }
    pub fn advance_retained(&mut self, key: &ContextKey, quota: usize) -> R<ContextPoll> {
        self.require_live()?;
        if !Arc::ptr_eq(&self.identity, &key.session) {
            return Err("E_NATIVE: context belongs to another session".into());
        }
        if self.terminal.contains_key(&key.id) {
            return Ok(if quota == 0 {
                ContextPoll::Paused
            } else {
                self.terminal.remove(&key.id).unwrap()
            });
        }
        self.require(key)?;
        if self
            .waits
            .get(&key.id)
            .is_some_and(|state| !state.signalled)
        {
            return Err("E_NATIVE: context is waiting for readiness".into());
        }
        self.parked[&key.id]
            .machine
            .require_runnable(&self.domains)?;
        // This bounded runtime only resumes a parent after its children settle.
        // General source scheduling/await handoffs are a separate integration gate.
        if self
            .domains
            .has_children(&self.parked[&key.id].machine.member)?
        {
            return Err("E_NATIVE: parent still awaits retained child contexts".into());
        }
        if quota == 0 {
            return Ok(ContextPoll::Paused);
        }
        if self
            .waits
            .get(&key.id)
            .is_some_and(|wait| wait.task.is_some())
        {
            self.parked[&key.id]
                .task
                .as_ref()
                .unwrap()
                .resume(self.host)?;
        }
        self.waits.remove(&key.id);
        let mut parked = self.parked.remove(&key.id).unwrap();
        parked.started = true;
        self.host.exchange_context(&mut parked.host)?;
        let mut settlement = None;
        let mut completed = None;
        let mut result =
            match parked
                .machine
                .advance(self.host, &mut self.values, &mut self.domains, quota)
            {
                Ok(Poll::Paused) => ContextPoll::Paused,
                Ok(Poll::Await) => ContextPoll::Await,
                Ok(Poll::Settling(domain)) => {
                    let message = parked.machine.fault.as_ref().unwrap().clone();
                    let report = self.host.activations.failure(
                        parked.name,
                        fault::Phase::Execution,
                        &message,
                    );
                    settlement = Some((domain, message, report, self.host.source_faulted()));
                    ContextPoll::Paused
                }
                Ok(Poll::Complete(completion @ Completion::Outcome(_))) => {
                    if let Some(task) = &parked.task {
                        task.complete(self.host)?;
                    }
                    completed = Some(completion);
                    ContextPoll::Ready
                }
                Ok(Poll::Complete(completion)) => {
                    // Outgoing owners survive normal completion; failed roots below
                    // release stranded temporary owners as well as retired frames.
                    if let Some(task) = &parked.task {
                        task.abandon(self.host);
                    }
                    self.host.finish_context(
                        &mut parked.root,
                        matches!(completion, Completion::Process(_)),
                    )?;
                    ContextPoll::Complete(completion)
                }
                Err(message) => {
                    let report = self.host.activations.failure(
                        parked.name,
                        fault::Phase::Execution,
                        &message,
                    );
                    if let Some(task) = &parked.task {
                        task.abandon(self.host);
                    }
                    self.host.finish_context(&mut parked.root, true)?;
                    if parked.parent.is_some() {
                        settlement = Some((
                            parked.machine.root.key.clone(),
                            message,
                            report.clone(),
                            self.host.source_faulted(),
                        ));
                    }
                    ContextPoll::Failed(report)
                }
            };
        self.host.exchange_context(&mut parked.host)?;
        if matches!(result, ContextPoll::Complete(Completion::Process(_))) {
            self.terminate()?;
            return Ok(result);
        }
        // Administrative/non-outcome completion and a terminal child failure do
        // not pass through join. Return their chain without delivering an outcome.
        let mut origin = key.id;
        if parked.sequential && matches!(result, ContextPoll::Complete(_) | ContextPoll::Failed(_))
        {
            origin = parked.parent.unwrap();
            self.restore_await(origin, key.id, &mut parked.host.tracker);
            if matches!(result, ContextPoll::Complete(_)) && self.callback_child(origin, key) {
                let parent = ContextKey {
                    session: self.identity.clone(),
                    id: origin,
                };
                let ContextPoll::Complete(completion) =
                    std::mem::replace(&mut result, ContextPoll::Resumed(parent.clone()))
                else {
                    unreachable!()
                };
                // Administrative completion propagates through the caller; it
                // never invokes ordinary outcome delivery or fabricates fields.
                self.await_continuation(&parent)?.result = Some(Ok(completion));
            }
        }
        if let Some(outcome) = completed {
            self.completed.insert(key.id, Completed { parked, outcome });
        } else if matches!(result, ContextPoll::Paused | ContextPoll::Await) {
            self.parked.insert(key.id, parked);
        }
        if let Some((domain, message, report, source_fault)) = settlement {
            self.settle(&domain, origin, &message, report.clone(), source_fault)?;
            if let Some(terminal) = self.terminal.remove(&key.id) {
                result = terminal;
            } else if !self.parked.contains_key(&key.id) {
                result = ContextPoll::Failed(report);
            }
        }
        Ok(result)
    }
    fn require_completed(&self, key: &ContextKey) -> R<&Completed> {
        self.require_live()?;
        if !Arc::ptr_eq(&self.identity, &key.session) {
            return Err("E_NATIVE: completed context belongs to another session".into());
        }
        let completed = self
            .completed
            .get(&key.id)
            .ok_or("E_NATIVE: context has no retained completion")?;
        let parked = &completed.parked;
        if !parked.machine.finished || !parked.machine.stack.is_empty() {
            return Err("E_NATIVE: completion still has executable frames".into());
        }
        parked.machine.require_finish_root(&self.domains)?;
        self.host.require_context(&parked.host, &parked.root)?;
        Ok(completed)
    }
    /// Consume a completed result once; early wait registration is separate.
    pub fn join(&mut self, parent: Option<&ContextKey>, key: &ContextKey) -> R<Completion> {
        self.join_inner(parent, key, false)
    }
    fn join_inner(
        &mut self,
        parent: Option<&ContextKey>,
        key: &ContextKey,
        callback: bool,
    ) -> R<Completion> {
        let completed = self.require_completed(key)?;
        let parked = &completed.parked;
        self.domains.require_open(&parked.machine.root.key, &[])?;
        if parent.map(|p| p.id) != parked.parent {
            return Err("E_NATIVE: completion belongs to another parent".into());
        }
        if parent.is_some_and(|p| self.callback_child(p.id, key)) != callback {
            return Err("E_NATIVE: completion requires its compiled await delivery".into());
        }
        if let Some(parent) = parent {
            self.require(parent)?;
            let parent = &self.parked[&parent.id];
            if !self
                .domains
                .same(&parent.machine.current, &parked.machine.root.key)
            {
                return Err("E_NATIVE: completion join crosses its owning fault domain".into());
            }
        }
        if let Some(task) = &parked.task {
            let parent = parent.ok_or("E_NATIVE: task completion requires its parent")?;
            let activation = self.parked[&parent.id]
                .machine
                .stack
                .last()
                .ok_or("E_NATIVE: task parent has retired")?;
            task.require_delivery(self.host, activation)?;
        }
        if parked.sequential && self.parked[&parked.parent.unwrap()].awaiting != Some(key.id) {
            return Err("E_NATIVE: sequential completion has lost its original caller".into());
        }
        let receiver = parent.map(|parent| {
            let parked = &self.parked[&parent.id];
            (&parked.host, &parked.root)
        });
        let completed = self.completed.get_mut(&key.id).unwrap();
        let Completion::Outcome((_, fields)) = &completed.outcome else {
            return Err("E_NATIVE: retained result is not a normal outcome".into());
        };
        self.host.adopt_context_result(
            &completed.parked.host,
            &mut completed.parked.root,
            receiver,
            fields,
        )?;
        // Domain retirement was prevalidated before host ownership changed.
        completed.parked.machine.finish_root(&mut self.domains)?;
        let mut completed = self.completed.remove(&key.id).unwrap();
        if let Some(task) = &completed.parked.task {
            task.deliver(self.host);
        }
        if completed.parked.sequential {
            self.restore_await(
                completed.parked.parent.unwrap(),
                key.id,
                &mut completed.parked.host.tracker,
            );
        }
        Ok(completed.outcome)
    }
    fn restore_await(&mut self, parent: u64, child: u64, tracker: &mut fault::Tracker) {
        let caller = self
            .parked
            .get_mut(&parent)
            .expect("retained sequential caller");
        assert_eq!(caller.awaiting, Some(child));
        caller.host.tracker = std::mem::take(tracker);
        caller.awaiting = None;
    }
    /// Move a failing sequential chain back through its parked callers before
    /// retiring their frames. A distinct child keeps the existing causal report.
    fn restore_fault_chain(&mut self, origin: u64, owner: u64) -> bool {
        let mut cursor = origin;
        while cursor != owner {
            let Some(child) = self.parked.get(&cursor) else {
                return false;
            };
            if !child.sequential {
                return false;
            }
            let parent = child.parent.unwrap();
            if self
                .parked
                .get(&parent)
                .is_none_or(|p| p.awaiting != Some(cursor))
            {
                return false;
            }
            cursor = parent;
        }
        if origin != owner {
            let tracker = std::mem::take(&mut self.parked.get_mut(&origin).unwrap().host.tracker);
            let caller = self.parked.get_mut(&owner).unwrap();
            caller.host.tracker = tracker;
            caller.awaiting = None;
        }
        true
    }
    fn retire_completed(&mut self, id: u64) -> R<()> {
        let mut completed = self
            .completed
            .remove(&id)
            .ok_or("E_NATIVE: missing stopped completion")?;
        completed
            .parked
            .machine
            .require_finish_root(&self.domains)?;
        // Retire result references before any supplying executable frame.
        if let Some(task) = &completed.parked.task {
            task.abandon(self.host);
        }
        drop(completed.outcome);
        self.host.exchange_context(&mut completed.parked.host)?;
        self.host.finish_context(&mut completed.parked.root, true)?;
        completed.parked.machine.finish_root(&mut self.domains)?;
        self.host.exchange_context(&mut completed.parked.host)?;
        Ok(())
    }
    fn terminate(&mut self) -> R<()> {
        self.terminated = true;
        self.terminal.clear();
        self.waits.clear();
        while let Some(id) = self.completed.keys().next_back().copied() {
            self.retire_completed(id)?;
        }
        // Administrative process completion retires every context, including
        // independent roots and active handlers, before the status is delivered.
        // Admission order always places children after their supplying parent.
        while let Some((_, mut parked)) = self.parked.pop_last() {
            if let Some(task) = &parked.task {
                task.abandon(self.host);
            }
            self.host.exchange_context(&mut parked.host)?;
            parked.machine.abort(
                self.host,
                &mut self.values,
                &mut self.domains,
                "E_PROCESS: process terminated",
            )?;
            self.host.finish_context(&mut parked.root, true)?;
            self.host.exchange_context(&mut parked.host)?;
        }
        Ok(())
    }
    fn settle(
        &mut self,
        domain: &DomainKey,
        origin: u64,
        message: &str,
        report: fault::Report,
        source_fault: bool,
    ) -> R<()> {
        let owner = self
            .parked
            .iter()
            .find_map(|(id, parked)| {
                parked
                    .machine
                    .owns_domain(&self.domains, domain)
                    .then_some(*id)
            })
            .ok_or("E_NATIVE: stopped domain has no retained owning continuation")?;
        let sequential = self.restore_fault_chain(origin, owner);
        self.domains.stop(domain)?;
        self.waits.remove(&owner);
        let completed: Vec<_> = self
            .completed
            .iter()
            .rev()
            .filter_map(|(id, result)| {
                self.domains
                    .require_member(domain, &result.parked.machine.root.key)
                    .is_ok()
                    .then_some(*id)
            })
            .collect();
        for id in completed {
            self.retire_completed(id)?;
        }
        // Child IDs are allocated after their parent. Reverse order retires
        // every borrower descendant before the frame that supplied its storage.
        let stopped: Vec<_> = self
            .parked
            .iter()
            .rev()
            .filter_map(|(id, parked)| {
                (*id != owner
                    && self
                        .domains
                        .require_member(domain, &parked.machine.root.key)
                        .is_ok())
                .then_some(*id)
            })
            .collect();
        for id in stopped {
            self.waits.remove(&id);
            let mut parked = self.parked.remove(&id).unwrap();
            if let Some(task) = &parked.task {
                task.abandon(self.host);
            }
            self.host.exchange_context(&mut parked.host)?;
            parked
                .machine
                .abort(self.host, &mut self.values, &mut self.domains, message)?;
            self.host.finish_context(&mut parked.root, true)?;
            self.host.exchange_context(&mut parked.host)?;
        }
        let mut parked = self.parked.remove(&owner).unwrap();
        self.host.exchange_context(&mut parked.host)?;
        if source_fault {
            self.host.activations.source_fault();
        }
        if origin != owner && !sequential {
            self.host.activations.with_cause(report);
        }
        parked.machine.fault = None;
        parked.machine.fault_target = Some(domain.clone());
        let result = parked.machine.unwind_error(
            self.host,
            &mut self.values,
            &mut self.domains,
            message.into(),
            true,
        );
        let mut next = None;
        if parked.machine.finished {
            if let Some(task) = &parked.task {
                task.abandon(self.host);
            }
            let error = result
                .as_ref()
                .err()
                .ok_or("E_NATIVE: failed owner lost its terminal error")?;
            let terminal =
                self.host
                    .activations
                    .failure(parked.name, fault::Phase::Execution, error);
            if parked.parent.is_some() {
                next = Some((
                    parked.machine.root.key.clone(),
                    error.clone(),
                    terminal,
                    self.host.source_faulted(),
                ));
            } else {
                assert!(
                    self.terminal
                        .insert(owner, ContextPoll::Failed(terminal))
                        .is_none()
                );
            }
            self.host.finish_context(&mut parked.root, true)?;
        } else if let Some(message) = &parked.machine.fault {
            next = Some((
                parked.machine.current.clone(),
                message.clone(),
                self.host
                    .activations
                    .failure(parked.name, fault::Phase::Execution, message),
                self.host.source_faulted(),
            ));
        }
        self.host.exchange_context(&mut parked.host)?;
        let finished = parked.machine.finished;
        if !parked.machine.finished {
            self.parked.insert(owner, parked);
        }
        if let Some((domain, message, report, source_fault)) = next {
            self.settle(&domain, owner, &message, report, source_fault)?;
        } else if !finished {
            result?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "task_context_tests.rs"]
mod task_tests;

#[cfg(test)]
#[path = "sequential_await_tests.rs"]
mod sequential_await_tests;

#[cfg(test)]
#[path = "await_handoff_tests.rs"]
mod await_handoff_tests;
