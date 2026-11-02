//! Retained host lifecycle only: no source scheduler or executable boundary resume.
use super::*;
use std::{cell::RefCell, rc::Rc};

struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
fn make(h: &mut Host, id: u32, drops: &Rc<RefCell<Vec<u32>>>) -> V {
    h.resources
        .insert("probe", Probe(id, drops.clone()))
        .unwrap()
}
fn host() -> Host {
    let mut h = Host::new(json!({}), None, true, Some(20));
    h.enter("main").unwrap();
    h
}
fn report(h: &mut Host, message: &str) -> fault::Report {
    h.activations
        .failure("main", fault::Phase::Execution, message)
}

#[test]
fn retained_boundary_handover_releases_before_handler_and_keeps_original_cause() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let keep = make(&mut h, 1, &drops);
    let input = make(&mut h, 2, &drops);
    let state = h.begin_boundary("request", [&keep, &input], true).unwrap();
    // Moving the lifecycle out of its admission call retains the same owners.
    let mut pending = vec![Box::new(state)];
    h.tick(1, 1).unwrap();
    let _stranded = make(&mut h, 3, &drops);
    h.tick(2, 1).unwrap();
    assert!(drops.borrow().is_empty());
    assert_eq!(h.fuel, Some(18));
    let mut state = pending.pop().unwrap();
    let message = h.fault::<()>(V::String("original".into())).unwrap_err();
    let retained = h.resources.owned_tokens([&keep]).unwrap();
    let original = h.boundary_fault(&mut state, &message, &retained).unwrap();
    let mut destroyed = drops.borrow().clone();
    destroyed.sort();
    assert_eq!(destroyed, [2, 3]);
    assert_eq!(h.resources.get::<Probe>(&keep, "probe").unwrap().0, 1);
    assert_eq!(original.activations[0].control, "request");
    h.enter_contingency(&mut state).unwrap();
    assert!(h.enter_contingency(&mut state).is_err());
    let error = h.escalate::<()>().unwrap_err();
    assert_eq!(error, message);
    assert!(h.boundary_fault(&mut state, &error, &retained).is_err());
    assert_eq!(h.events.len(), 1);
    h.finish_boundary(&mut state, &Err::<(), _>(error.clone()))
        .unwrap();
    assert!(h.finish_boundary(&mut state, &Ok(())).is_err());
    assert!(h.contingencies.is_empty());
    assert!(h.resources.scopes.is_empty() && h.resources.values.is_empty());
    assert_eq!(drops.borrow().len(), 3);
    let final_report = report(&mut h, &error);
    assert_eq!(final_report.kind, fault::Kind::SourceFault);
    let handler = final_report.cause.unwrap();
    assert_eq!(handler.activations[0].control, "request$contingency");
    assert_eq!(*handler.cause.unwrap(), original);
}

#[test]
fn foreign_nested_and_retired_boundary_records_cannot_finalize_live_context() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let owned = make(&mut h, 1, &drops);
    let mut outer = h.begin_boundary("outer", [&owned], true).unwrap();
    let mut inner = h.begin_boundary("inner", [], true).unwrap();
    let new = make(&mut h, 2, &drops);
    let mut foreign = host();
    assert!(foreign.finish_boundary(&mut inner, &Ok(())).is_err());
    assert!(h.finish_boundary(&mut outer, &Ok(())).is_err());
    assert!(drops.borrow().is_empty());
    assert_eq!(h.resources.scopes.len(), 2);
    h.finish_boundary(&mut inner, &Ok(())).unwrap();
    assert_eq!(h.resources.values.len(), 2);
    assert!(
        h.resources
            .values
            .values()
            .all(|r| r.scope == Some(outer.scope.id))
    );
    h.finish_boundary(&mut outer, &Ok(())).unwrap();
    assert!(h.resources.values.values().all(|r| r.scope.is_none()));
    assert!(h.finish_boundary(&mut inner, &Ok(())).is_err());
    assert!(h.events.is_empty() && h.contingencies.is_empty());
    h.enter("resumed").unwrap();
    h.leave();
    let observed = report(&mut h, "observation");
    assert_eq!(observed.activations.len(), 1);
    assert_eq!(observed.activations[0].control, "main");
    h.release_frame(&[Some(owned), Some(new)]);
    let mut destroyed = drops.borrow().clone();
    destroyed.sort();
    assert_eq!(destroyed, [1, 2]);
}

#[test]
fn retained_nested_handlers_preserve_cause_and_restore_outer_recovery_context() {
    for escalate in [false, true] {
        let mut h = host();
        let mut outer = h.begin_boundary("outer", [], true).unwrap();
        let mut inner = h.begin_boundary("inner", [], true).unwrap();
        let message = h.fault::<()>(V::String("first".into())).unwrap_err();
        let first = h
            .boundary_fault(&mut inner, &message, &BTreeSet::new())
            .unwrap();
        h.enter_contingency(&mut inner).unwrap();
        let error = if escalate {
            h.escalate::<()>().unwrap_err()
        } else {
            h.fault::<()>(V::String("handler failed".into()))
                .unwrap_err()
        };
        h.finish_boundary(&mut inner, &Err::<(), _>(error.clone()))
            .unwrap();
        let second = h
            .boundary_fault(&mut outer, &error, &BTreeSet::new())
            .unwrap();
        assert_eq!(second.message, error);
        assert_eq!(
            **second.cause.as_ref().unwrap().cause.as_ref().unwrap(),
            first
        );
        h.enter_contingency(&mut outer).unwrap();
        let recovery = Ok(Completion::Recover(7, vec![V::Bool(false)]));
        h.finish_boundary(&mut outer, &recovery).unwrap();
        assert!(matches!(recovery, Ok(Completion::Recover(7, _))));
        assert!(!h.source_faulted());
        assert!(h.contingencies.is_empty() && h.resources.scopes.is_empty());
        assert_eq!(h.events.len(), 2);
        h.enter("after recovery").unwrap();
        h.leave();
        assert!(report(&mut h, "observation").cause.is_none());
    }
}

#[test]
fn named_boundary_retirement_preserves_host_errors_without_handler_entry() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let input = make(&mut h, 1, &drops);
    let mut state = h.begin_boundary("named", [&input], false).unwrap();
    h.enter("named work").unwrap();
    assert!(h.enter_contingency(&mut state).is_err());
    assert!(
        h.boundary_fault(&mut state, "E_HOST: failed", &BTreeSet::new())
            .is_err()
    );
    assert!(h.events.is_empty());
    let result: R<()> = Err("E_HOST: failed".into());
    h.finish_boundary(&mut state, &result).unwrap();
    assert_eq!(*drops.borrow(), [1]);
    assert!(!h.source_faulted());
    let observed = report(&mut h, "E_HOST: failed");
    assert_eq!(observed.kind, fault::Kind::ReturnedError);
    assert_eq!(observed.cause.unwrap().activations[0].control, "named work");
    assert!(h.contingencies.is_empty() && h.resources.scopes.is_empty());
}
