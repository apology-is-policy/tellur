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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WaitState {
    generation: u64,
    signalled: bool,
}
struct Parked {
    machine: Machine,
    host: HostContext,
    root: ContextRoot,
    name: &'static str,
    parent: Option<u64>,
    started: bool,
}
struct Completed {
    parked: Parked,
    outcome: Completion,
}
pub enum ContextPoll {
    Paused,
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
            },
        );
        Ok(ContextKey {
            session: self.identity.clone(),
            id,
        })
    }
    pub fn park_for_readiness(&mut self, key: &ContextKey) -> R<Readiness> {
        self.require(key)?;
        let parked = &self.parked[&key.id];
        if !parked.started
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
        self.waits.insert(
            key.id,
            WaitState {
                generation,
                signalled: false,
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
                    completed = Some(completion);
                    ContextPoll::Ready
                }
                Ok(Poll::Complete(completion)) => {
                    // Outgoing owners survive normal completion; failed roots below
                    // release stranded temporary owners as well as retired frames.
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
        if let Some(outcome) = completed {
            self.completed.insert(key.id, Completed { parked, outcome });
        } else if matches!(result, ContextPoll::Paused) {
            self.parked.insert(key.id, parked);
        }
        if let Some((domain, message, report, source_fault)) = settlement {
            self.settle(&domain, key.id, &message, report.clone(), source_fault)?;
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
        let completed = self.require_completed(key)?;
        let parked = &completed.parked;
        self.domains.require_open(&parked.machine.root.key, &[])?;
        if parent.map(|p| p.id) != parked.parent {
            return Err("E_NATIVE: completion belongs to another parent".into());
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
        let completed = self.completed.remove(&key.id).unwrap();
        Ok(completed.outcome)
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
        if origin != owner {
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
