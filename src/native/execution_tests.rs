//! Direct shared-runtime probes, without the compiled fixture generator/drivers.
use super::super::{ContextKey, ContextPoll, Contexts, Readiness};
use super::*;
use std::{cell::RefCell, rc::Rc};
struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
fn host() -> Host {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    h.enter("ambient").unwrap();
    h
}
fn owner(c: &mut Contexts<'_>, id: u32, drops: &Rc<RefCell<Vec<u32>>>) -> V {
    c.host
        .resources
        .insert("probe", Probe(id, drops.clone()))
        .unwrap()
}
fn returning(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    let fields = a
        .control
        .parameters
        .iter()
        .map(|i| v[a.base() + i].take().unwrap())
        .collect();
    Ok(Step::Complete(Completion::Outcome((
        "ready".into(),
        fields,
    ))))
}
fn failing(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    h.fault(V::String("child failed".into()))
}
fn terminating(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(3, 1)?;
    Ok(Step::Complete(Completion::Process(23)))
}
static OWNER: Control = Control {
    name: "owner",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    resume: returning,
};
static RESULT: Control = Control {
    name: "result",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: returning,
};
static FAULT: Control = Control {
    name: "faulting",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: failing,
};
static PROCESS: Control = Control {
    name: "process",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: terminating,
};
fn retained(c: &mut Contexts<'_>, k: &ContextKey) {
    assert!(matches!(
        c.advance_retained(k, 2).unwrap(),
        ContextPoll::Ready
    ));
}
fn fields(c: &mut Contexts<'_>, parent: Option<&ContextKey>, k: &ContextKey) -> Vec<V> {
    let Completion::Outcome((name, fields)) = c.join(parent, k).unwrap() else {
        panic!()
    };
    assert_eq!(name, "ready");
    fields
}
fn release(c: &mut Contexts<'_>, values: Vec<V>) {
    c.host
        .release_frame(&values.into_iter().map(Some).collect::<Vec<_>>());
}
fn snapshot(c: &Contexts<'_>) -> serde_json::Value {
    serde_json::json!({"parked":c.parked.keys().collect::<Vec<_>>(),"completed":c.completed.keys().collect::<Vec<_>>(),
        "terminal":c.terminal.keys().collect::<Vec<_>>(),"waits":format!("{:?}",c.waits),"domains":c.domains.len(),"members":c.domains.member_count(),
        "fuel":c.host.fuel,"events":c.host.events,"selected":c.host.resources.current,"scopes":c.host.resources.scopes,
        "owners":c.host.resources.values.iter().map(|(id,r)|(id,r.scope)).collect::<Vec<_>>()})
}
fn empty(c: &mut Contexts<'_>) {
    assert!(
        c.parked.is_empty()
            && c.completed.is_empty()
            && c.terminal.is_empty()
            && c.waits.is_empty()
    );
    assert!(c.values.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    assert!(c.host.resources.values.is_empty() && c.host.resources.scopes.is_empty());
    assert!(c.host.resources.current.is_none());
    assert_eq!(
        c.host
            .activations
            .failure("ambient", fault::Phase::Execution, "probe")
            .activations
            .len(),
        1
    );
}
#[test]
fn shared_context_join_preserves_source_borrows_and_separates_readiness() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let value = owner(&mut c, 1, &drops);
    let parent = c.start(&OWNER, vec![value]).unwrap();
    assert!(matches!(
        c.advance_retained(&parent, 1).unwrap(),
        ContextPoll::Paused
    ));
    let base = c.parked[&parent.id].machine.stack[0].base();
    let value = owner(&mut c, 2, &drops);
    let child = c
        .start_child(
            &parent,
            &RESULT,
            vec![
                value,
                V::Reference {
                    ty: "&probe".into(),
                    root: base,
                    fields: vec![],
                },
            ],
        )
        .unwrap();
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Paused
    ));
    let wake: Readiness = c.park_for_readiness(&child).unwrap();
    let before = snapshot(&c);
    assert!(c.join(Some(&parent), &child).is_err());
    assert!(c.advance_retained(&child, 1).is_err());
    assert_eq!(snapshot(&c), before);
    c.signal_readiness(&wake).unwrap();
    let before = snapshot(&c);
    assert!(matches!(
        c.advance_retained(&child, 0).unwrap(),
        ContextPoll::Paused
    ));
    assert_eq!(snapshot(&c), before);
    assert!(c.signal_readiness(&wake).is_err());
    retained(&mut c, &child);
    let before = snapshot(&c);
    assert!(c.signal_readiness(&wake).is_err());
    assert!(c.advance_retained(&parent, 1).is_err());
    assert!(c.join(None, &child).is_err());
    let mut foreign = parent.clone();
    foreign.session = Arc::new(());
    assert!(c.join(Some(&foreign), &child).is_err());
    assert_eq!(snapshot(&c), before);
    let result = fields(&mut c, Some(&parent), &child);
    let V::Reference {
        root, fields: path, ..
    } = &result[1]
    else {
        panic!()
    };
    let source = super::super::super::read_place(&c.values, *root, path, false).unwrap();
    assert_eq!(
        c.host.resources.get::<Probe>(&source, "probe").unwrap().0,
        1
    );
    let V::Opaque { token, .. } = &result[0] else {
        panic!()
    };
    assert_eq!(
        c.host.resources.values[token].scope,
        c.parked[&parent.id].host.scope.as_ref().map(|s| s.id)
    );
    assert!(c.join(Some(&parent), &child).is_err());
    release(&mut c, result);
    assert_eq!(*drops.borrow(), [2]);
    retained(&mut c, &parent);
    let result = fields(&mut c, None, &parent);
    release(&mut c, result);
    empty(&mut c);
    assert_eq!(*drops.borrow(), [2, 1]);
    assert_eq!(c.host.fuel, Some(96));
}
#[test]
fn shared_fault_retirement_releases_results_before_suppliers_and_keeps_other_roots() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let value = owner(&mut c, 1, &drops);
    let parent = c.start(&OWNER, vec![value]).unwrap();
    let value = owner(&mut c, 2, &drops);
    let child = c.start_child(&parent, &OWNER, vec![value]).unwrap();
    retained(&mut c, &child);
    let value = owner(&mut c, 3, &drops);
    let other = c.start(&OWNER, vec![value]).unwrap();
    retained(&mut c, &other);
    let failed = c.start_child(&parent, &FAULT, vec![]).unwrap();
    assert!(matches!(
        c.advance_retained(&failed, 1).unwrap(),
        ContextPoll::Failed(_)
    ));
    assert_eq!(*drops.borrow(), [2, 1]);
    assert!(c.completed.contains_key(&other.id));
    let before = snapshot(&c);
    assert!(matches!(
        c.advance_retained(&parent, 0).unwrap(),
        ContextPoll::Paused
    ));
    assert_eq!(snapshot(&c), before);
    let ContextPoll::Failed(report) = c.advance_retained(&parent, 1).unwrap() else {
        panic!()
    };
    assert!(report.cause.is_some());
    assert!(c.advance_retained(&parent, 1).is_err());
    assert!(c.join(Some(&parent), &child).is_err());
    let result = fields(&mut c, None, &other);
    release(&mut c, result);
    empty(&mut c);
    assert_eq!(*drops.borrow(), [2, 1, 3]);
    assert_eq!(c.host.fuel, Some(95));
}
#[test]
fn shared_process_retirement_drains_results_waiters_and_unstarted_roots_before_status() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let value = owner(&mut c, 1, &drops);
    let parent = c.start(&OWNER, vec![value]).unwrap();
    let value = owner(&mut c, 2, &drops);
    let child = c.start_child(&parent, &OWNER, vec![value]).unwrap();
    retained(&mut c, &child);
    let value = owner(&mut c, 3, &drops);
    let waiter = c.start(&OWNER, vec![value]).unwrap();
    assert!(matches!(
        c.advance_retained(&waiter, 1).unwrap(),
        ContextPoll::Paused
    ));
    let wake: Readiness = c.park_for_readiness(&waiter).unwrap();
    let value = owner(&mut c, 4, &drops);
    let unstarted = c.start(&OWNER, vec![value]).unwrap();
    let process = c.start(&PROCESS, vec![]).unwrap();
    let fuel = c.host.fuel.unwrap();
    assert!(matches!(
        c.advance_retained(&process, 1).unwrap(),
        ContextPoll::Complete(Completion::Process(23))
    ));
    assert_eq!(*drops.borrow(), [2, 4, 3, 1]);
    assert_eq!(c.host.fuel, Some(fuel - 1));
    empty(&mut c);
    let before = snapshot(&c);
    assert!(c.signal_readiness(&wake).is_err());
    assert!(c.join(Some(&parent), &child).is_err());
    assert!(c.advance_retained(&unstarted, 0).is_err());
    assert!(c.start(&PROCESS, vec![]).is_err());
    assert_eq!(snapshot(&c), before);
}
