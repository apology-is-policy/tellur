//! Test-only selection of independent compiled roots over the actual native host.
use super::super::{ContextRoot, HostContext, fault};
use super::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct ContextKey {
    session: Arc<()>,
    id: u64,
}
struct Parked {
    machine: Machine,
    host: HostContext,
    root: ContextRoot,
    name: &'static str,
    parent: Option<u64>,
}
pub enum ContextPoll {
    Paused,
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
        }
    }
    pub fn start(&mut self, control: &'static Control, arguments: Vec<V>) -> R<ContextKey> {
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
        let machine = started?;
        self.next = id;
        self.parked.insert(
            id,
            Parked {
                machine,
                host: context,
                root,
                name: control.name,
                parent: None,
            },
        );
        Ok(ContextKey {
            session: self.identity.clone(),
            id,
        })
    }
    fn require(&self, key: &ContextKey) -> R<()> {
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
        let owner = self.parked[&parent.id].machine.current.clone();
        let membership = self.parked[&parent.id].machine.member.clone();
        self.domains.require_open(&owner, &[])?;
        self.domains.require_member_capacity()?;
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
        let machine = started?;
        self.next = id;
        self.parked.insert(
            id,
            Parked {
                machine,
                host: context,
                root,
                name: control.name,
                parent: Some(parent.id),
            },
        );
        Ok(ContextKey {
            session: self.identity.clone(),
            id,
        })
    }
    pub fn advance(&mut self, key: &ContextKey, quota: usize) -> R<ContextPoll> {
        self.require(key)?;
        self.parked[&key.id]
            .machine
            .require_runnable(&self.domains)?;
        // This bounded harness only resumes a parent after its children settle.
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
        let mut parked = self.parked.remove(&key.id).unwrap();
        self.host.exchange_context(&mut parked.host)?;
        let mut settlement = None;
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
                Ok(Poll::Complete(completion)) => {
                    // Outgoing owners survive normal completion; failed roots below
                    // release stranded temporary owners as well as retired frames.
                    self.host.finish_context(&mut parked.root, false)?;
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
        if matches!(result, ContextPoll::Paused) {
            self.parked.insert(key.id, parked);
        }
        if let Some((domain, message, report, source_fault)) = settlement {
            self.settle(&domain, key.id, &message, report.clone(), source_fault)?;
            if !self.parked.contains_key(&key.id) {
                result = ContextPoll::Failed(report);
            }
        }
        Ok(result)
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
        if parked.machine.finished {
            self.host.finish_context(&mut parked.root, true)?;
        }
        self.host.exchange_context(&mut parked.host)?;
        if !parked.machine.finished {
            self.parked.insert(owner, parked);
        }
        // Terminal failure is reported by the caller's original fault event.
        if let Err(error) = result {
            if error != message {
                return Err(error);
            }
        }
        Ok(())
    }
}

/// Explicit harness schedule, not a Tellur task scheduler. Each job is an
/// independent root with separately supplied arguments, never a cloned owner.
pub fn drive_contexts(
    host: &mut Host,
    jobs: Vec<(&'static Control, Vec<V>)>,
    quotas: &[usize],
    reverse: bool,
) -> R<Completion> {
    if !quotas.iter().any(|n| *n > 0) {
        return Err("E_NATIVE: context harness cannot make progress".into());
    }
    // A nonempty host diagnostic/contingency baseline must survive every root.
    host.enter("context harness")?;
    host.contingencies.push("host contingency marker".into());
    let mut contexts = Contexts::new(host);
    let baseline = (
        contexts.host.cursor,
        contexts.host.fuel,
        contexts.host.events.len(),
        contexts.host.resources.scopes.len(),
    );
    contexts.next = u64::MAX;
    assert!(
        contexts
            .start(jobs[0].0, vec![])
            .unwrap_err()
            .contains("context identity exhausted")
    );
    contexts.next = 0;
    assert_eq!(
        baseline,
        (
            contexts.host.cursor,
            contexts.host.fuel,
            contexts.host.events.len(),
            contexts.host.resources.scopes.len()
        )
    );
    let keys = jobs
        .into_iter()
        .map(|(control, args)| contexts.start(control, args))
        .collect::<R<Vec<_>>>()?;
    let mut order: Vec<_> = (0..keys.len()).collect();
    if reverse {
        order.reverse();
    }
    let foreign = ContextKey {
        session: Arc::new(()),
        id: keys[0].id,
    };
    assert!(contexts.advance(&foreign, 1).is_err());
    assert_eq!(contexts.host.cursor, baseline.0);
    assert_eq!(contexts.host.fuel, baseline.1);
    assert_eq!(contexts.host.events.len(), baseline.2);
    if keys.len() > 1 {
        let other = contexts.parked[&keys[1].id].machine.current.clone();
        let original = std::mem::replace(
            &mut contexts
                .parked
                .get_mut(&keys[0].id)
                .unwrap()
                .machine
                .current,
            other,
        );
        assert!(contexts.advance(&keys[0], 0).is_err());
        contexts
            .parked
            .get_mut(&keys[0].id)
            .unwrap()
            .machine
            .current = original;
        let other = contexts.parked[&keys[1].id].host.scope.clone();
        let original = std::mem::replace(
            &mut contexts.parked.get_mut(&keys[0].id).unwrap().host.scope,
            other,
        );
        assert!(contexts.advance(&keys[0], 1).is_err());
        contexts.parked.get_mut(&keys[0].id).unwrap().host.scope = original;
        let original = contexts.parked[&keys[0].id].root.scope.clone();
        contexts.parked.get_mut(&keys[0].id).unwrap().root.scope.id = u64::MAX;
        assert!(contexts.advance(&keys[0], 1).is_err());
        contexts.parked.get_mut(&keys[0].id).unwrap().root.scope = original;
        assert_eq!(contexts.host.cursor, baseline.0);
        assert_eq!(contexts.host.fuel, baseline.1);
        assert_eq!(contexts.host.events.len(), baseline.2);
        assert_eq!(contexts.parked.len(), keys.len());
    }
    let foreign_domain = Domains::default().prepare(None)?;
    let original = std::mem::replace(
        &mut contexts
            .parked
            .get_mut(&keys[0].id)
            .unwrap()
            .machine
            .current,
        foreign_domain.key,
    );
    assert!(contexts.advance(&keys[0], 0).is_err());
    contexts
        .parked
        .get_mut(&keys[0].id)
        .unwrap()
        .machine
        .current = original;
    let mut done = vec![false; keys.len()];
    let mut turns = 0;
    while done.iter().any(|v| !v) {
        for &index in &order {
            if done[index] {
                continue;
            }
            let quota = quotas[turns % quotas.len()];
            let snapshot = (
                contexts.host.cursor,
                contexts.host.fuel,
                contexts.host.events.len(),
                contexts.host.resources.values.len(),
                contexts.host.resources.scopes.len(),
                contexts.domains.len(),
            );
            let result = contexts.advance(&keys[index], quota)?;
            if quota == 0 {
                assert_eq!(
                    snapshot,
                    (
                        contexts.host.cursor,
                        contexts.host.fuel,
                        contexts.host.events.len(),
                        contexts.host.resources.values.len(),
                        contexts.host.resources.scopes.len(),
                        contexts.domains.len()
                    )
                );
            }
            let event = match result {
                ContextPoll::Paused => {
                    // A live ancestor is not the selected nested boundary. Reject
                    // such a parked-state mismatch even when no work is requested.
                    let parked = contexts.parked.get_mut(&keys[index].id).unwrap();
                    let original = std::mem::replace(
                        &mut parked.machine.current,
                        parked.machine.root.key.clone(),
                    );
                    if contexts
                        .domains
                        .require_selected(&parked.machine.root, &original)
                        .is_err()
                    {
                        assert!(contexts.advance(&keys[index], 0).is_err());
                    }
                    contexts
                        .parked
                        .get_mut(&keys[index].id)
                        .unwrap()
                        .machine
                        .current = original;
                    contexts.require(&keys[index])?;
                    continue;
                }
                ContextPoll::Complete(Completion::Outcome((outcome, fields))) => {
                    serde_json::json!({"kind":"context_complete","context":index,"outcome":outcome,"fields":fields})
                }
                ContextPoll::Complete(Completion::Process(status)) => {
                    serde_json::json!({"kind":"context_process","context":index,"status":status})
                }
                ContextPoll::Failed(report) => {
                    serde_json::json!({"kind":"context_failed","context":index,"report":report})
                }
                ContextPoll::Complete(_) => {
                    return Err("E_NATIVE: invalid independent root completion".into());
                }
            };
            done[index] = true;
            contexts.host.events.push(event);
            assert!(contexts.advance(&keys[index], 0).is_err());
        }
        turns += 1;
    }
    assert!(contexts.values.is_empty());
    assert_eq!(contexts.domains.len(), 0);
    assert_eq!(contexts.domains.member_count(), 0);
    assert_eq!(contexts.host.resources.scopes.len(), baseline.3);
    assert_eq!(contexts.host.contingencies, ["host contingency marker"]);
    let baseline_report = contexts.host.activations.failure(
        "context harness",
        fault::Phase::Execution,
        "host observation",
    );
    assert_eq!(baseline_report.activations.len(), 1);
    assert_eq!(baseline_report.activations[0].control, "context harness");
    assert!(baseline_report.cause.is_none());
    assert!(!contexts.host.source_faulted());
    contexts.host.contingencies.pop();
    contexts.host.leave();
    contexts.host.events.push(serde_json::json!({
        "kind":"context_summary", "cursor":contexts.host.cursor,
        "script_calls":contexts.host.script.as_ref().map(Vec::len),
        "fuel":contexts.host.fuel, "resources":contexts.host.resources.values.len()
    }));
    // At a fuel cutoff some independent roots can fail before their scripted
    // input. Preserve the cursor above; this harness reports partial execution
    // instead of letting launch's single-root unused-script check hide it.
    contexts.host.script = None;
    Ok(Completion::Outcome(("done".into(), vec![])))
}
