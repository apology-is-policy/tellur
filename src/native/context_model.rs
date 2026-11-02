//! Test-only selection of independent compiled roots over the actual native host.
use super::super::{ResourceScope, Resources, fault};
use super::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct ContextKey {
    session: Arc<()>,
    id: u64,
}
struct HostContext {
    tracker: fault::Tracker,
    contingencies: Vec<String>,
    scope: Option<ResourceScope>,
}
impl HostContext {
    fn current_scope(resources: &Resources) -> R<Option<ResourceScope>> {
        resources
            .current
            .map(|id| {
                let parent = *resources
                    .scopes
                    .get(&id)
                    .ok_or("E_NATIVE: selected context scope has retired")?;
                Ok(ResourceScope {
                    registry: resources.identity.clone(),
                    id,
                    parent,
                })
            })
            .transpose()
    }
    /// Validate both selections before moving any diagnostic/contingency state.
    /// The caller exclusively borrows Host, so no external operation can interleave.
    fn exchange(&mut self, host: &mut Host) -> R<()> {
        let previous = Self::current_scope(&host.resources)?;
        if let Some(scope) = &self.scope {
            host.resources.require_scope(scope)?;
        }
        host.resources.select_scope(self.scope.as_ref())?;
        self.scope = previous;
        std::mem::swap(&mut self.tracker, &mut host.activations);
        std::mem::swap(&mut self.contingencies, &mut host.contingencies);
        Ok(())
    }
}
struct Parked {
    machine: Machine,
    host: HostContext,
    root: ResourceScope,
    name: &'static str,
}
pub enum ContextPoll {
    Paused,
    Complete(Completion),
    Failed(fault::Report),
}
pub struct Contexts<'a> {
    host: &'a mut Host,
    values: Frames,
    identity: Arc<()>,
    next: u64,
    parked: BTreeMap<u64, Parked>,
}
impl<'a> Contexts<'a> {
    pub fn new(host: &'a mut Host) -> Self {
        Self {
            host,
            values: Frames::default(),
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
        // Independent roots have explicit ownership scopes. They do not inherit
        // whichever boundary the host happened to have selected before this call.
        let previous = self.host.resources.current.take();
        let prepared = self.host.resources.prepare_scope(&arguments);
        self.host.resources.current = previous;
        let prepared = prepared?;
        self.host.resources.current = None;
        let root = self.host.resources.commit_scope(prepared);
        self.host.resources.current = previous;
        let mut context = HostContext {
            tracker: self.host.activations.child(),
            contingencies: vec![],
            scope: Some(root.clone()),
        };
        context.exchange(self.host)?;
        let started = Machine::start(self.host, &mut self.values, control, arguments);
        if started.is_err() {
            self.host.resources.finish_scope(&root, true)?;
        }
        context.exchange(self.host)?;
        let machine = started?;
        self.next = id;
        self.parked.insert(
            id,
            Parked {
                machine,
                host: context,
                root,
                name: control.name,
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
        self.host.resources.require_scope(&parked.root)?;
        let scope = parked
            .host
            .scope
            .as_ref()
            .ok_or("E_NATIVE: context has no resource scope")?;
        self.host.resources.require_scope(scope)?;
        let mut ancestor = Some(scope.id);
        while let Some(id) = ancestor {
            if id == parked.root.id {
                return Ok(());
            }
            ancestor = *self
                .host
                .resources
                .scopes
                .get(&id)
                .ok_or("E_NATIVE: context resource ancestor has retired")?;
        }
        Err("E_NATIVE: selected resource scope belongs to another context".into())
    }
    pub fn advance(&mut self, key: &ContextKey, quota: usize) -> R<ContextPoll> {
        self.require(key)?;
        if quota == 0 {
            return Ok(ContextPoll::Paused);
        }
        let mut parked = self.parked.remove(&key.id).unwrap();
        parked.host.exchange(self.host)?;
        let result = match parked.machine.advance(self.host, &mut self.values, quota) {
            Ok(Poll::Paused) => ContextPoll::Paused,
            Ok(Poll::Complete(completion)) => {
                // Outgoing owners survive normal completion; failed roots below
                // release stranded temporary owners as well as retired frames.
                self.host.resources.finish_scope(&parked.root, false)?;
                ContextPoll::Complete(completion)
            }
            Err(message) => {
                let report =
                    self.host
                        .activations
                        .failure(parked.name, fault::Phase::Execution, &message);
                self.host.resources.finish_scope(&parked.root, true)?;
                ContextPoll::Failed(report)
            }
        };
        parked.host.exchange(self.host)?;
        if matches!(result, ContextPoll::Paused) {
            self.parked.insert(key.id, parked);
        }
        Ok(result)
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
        let other = contexts.parked[&keys[1].id].host.scope.clone();
        let original = std::mem::replace(
            &mut contexts.parked.get_mut(&keys[0].id).unwrap().host.scope,
            other,
        );
        assert!(contexts.advance(&keys[0], 1).is_err());
        contexts.parked.get_mut(&keys[0].id).unwrap().host.scope = original;
        let original = contexts.parked[&keys[0].id].root.clone();
        contexts.parked.get_mut(&keys[0].id).unwrap().root.id = u64::MAX;
        assert!(contexts.advance(&keys[0], 1).is_err());
        contexts.parked.get_mut(&keys[0].id).unwrap().root = original;
        assert_eq!(contexts.host.cursor, baseline.0);
        assert_eq!(contexts.host.fuel, baseline.1);
        assert_eq!(contexts.host.events.len(), baseline.2);
        assert_eq!(contexts.parked.len(), keys.len());
    }
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
                        contexts.host.resources.scopes.len()
                    )
                );
            }
            let event = match result {
                ContextPoll::Paused => continue,
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
