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
    let mut h = Host::new(json!({}), None, true, Some(200));
    h.enter("main").unwrap();
    h.contingencies.push("host marker".into());
    h
}
fn observations(
    h: &Host,
) -> (
    usize,
    Option<usize>,
    Vec<Json>,
    Option<u64>,
    usize,
    u64,
    Vec<String>,
    String,
) {
    (
        h.cursor,
        h.fuel,
        h.events.clone(),
        h.resources.current,
        h.resources.values.len(),
        h.resources.next_scope,
        h.contingencies.clone(),
        format!("{:?}", h.activations),
    )
}

#[test]
fn paired_context_selection_restores_handlers_and_preserves_independent_real_owners() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let owner = make(&mut h, 1, &drops);
    let borrowed = make(&mut h, 2, &drops);
    let other = make(&mut h, 3, &drops);
    let loan = V::Reference {
        evidence: Default::default(),
        ty: "&probe".into(),
        root: 0,
        fields: vec![],
    };
    let (mut a, mut first) = h.prepare_context([&owner, &loan]).unwrap();
    let (mut b, mut second) = h.prepare_context([&other]).unwrap();
    assert!(h.resources.current.is_none());
    let V::Opaque { token, .. } = &borrowed else {
        panic!()
    };
    assert!(h.resources.values[token].scope.is_none());
    h.require_context(&first, &a).unwrap();
    h.exchange_context(&mut first).unwrap();
    assert!(h.contingencies.is_empty());
    h.enter("A").unwrap();
    let mut boundary = h.begin_boundary("A boundary", [&owner], true).unwrap();
    let _temporary = make(&mut h, 4, &drops);
    let message = h.fault::<()>(V::String("A failed".into())).unwrap_err();
    let keep = h.resources.owned_tokens([&owner]).unwrap();
    let original = h.boundary_fault(&mut boundary, &message, &keep).unwrap();
    assert_eq!(*drops.borrow(), [4]);
    h.enter_contingency(&mut boundary).unwrap();
    h.tick(1, 1).unwrap();
    h.exchange_context(&mut first).unwrap();
    assert_eq!(h.contingencies, ["host marker"]);
    assert!(!h.source_faulted());
    assert!(h.resources.current.is_none());
    h.exchange_context(&mut second).unwrap();
    assert!(h.contingencies.is_empty());
    h.enter("B").unwrap();
    let later = make(&mut h, 5, &drops);
    h.tick(2, 1).unwrap();
    h.exchange_context(&mut second).unwrap();
    h.require_context(&first, &a).unwrap();
    h.exchange_context(&mut first).unwrap();
    assert_eq!(h.escalate::<()>().unwrap_err(), message);
    h.finish_boundary(&mut boundary, &Err::<(), _>(message.clone()))
        .unwrap();
    h.finish_context(&mut a, true).unwrap();
    let failed = h
        .activations
        .failure("A", fault::Phase::Execution, &message);
    assert_eq!(failed.activations[0].control, "A");
    assert_eq!(
        **failed.cause.as_ref().unwrap().cause.as_ref().unwrap(),
        original
    );
    h.exchange_context(&mut first).unwrap();
    assert_eq!(*drops.borrow(), [4, 1]);
    h.require_context(&second, &b).unwrap();
    h.exchange_context(&mut second).unwrap();
    assert_eq!(h.resources.get::<Probe>(&other, "probe").unwrap().0, 3);
    assert_eq!(h.resources.get::<Probe>(&later, "probe").unwrap().0, 5);
    assert!(!h.source_faulted());
    h.release_frame(&[Some(other), Some(later)]);
    h.leave();
    h.finish_context(&mut b, false).unwrap();
    h.exchange_context(&mut second).unwrap();
    assert_eq!(h.resources.get::<Probe>(&borrowed, "probe").unwrap().0, 2);
    h.release_frame(&[Some(borrowed)]);
    assert_eq!(*drops.borrow(), [4, 1, 3, 5, 2]);
    assert!(h.resources.scopes.is_empty() && h.resources.values.is_empty());
    assert_eq!(h.fuel, Some(198));
    assert_eq!(h.contingencies, ["host marker"]);
    h.enter("still live host").unwrap();
    h.leave();
}

#[test]
fn context_ownership_scope_and_retirement_checks_precede_state_changes() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut foreign = host();
    let mut outer = h.begin_boundary("host boundary", [], true).unwrap();
    let (mut root, mut context) = h.prepare_context([]).unwrap();
    let (mut other, mut other_context) = h.prepare_context([]).unwrap();
    assert_eq!(h.resources.current, Some(outer.scope.id));
    assert!(root.scope.parent.is_none());
    let before = observations(&h);
    assert!(h.require_context(&context, &other).is_err());
    assert!(h.finish_context(&mut root, true).is_err());
    assert_eq!(observations(&h), before);
    let before = observations(&foreign);
    assert!(foreign.exchange_context(&mut context).is_err());
    assert_eq!(observations(&foreign), before);
    h.exchange_context(&mut context).unwrap();
    // Even a record holding a scope-less host context is tied to its host.
    let (mut isolated, mut isolated_context) = foreign.prepare_context([]).unwrap();
    foreign.exchange_context(&mut isolated_context).unwrap();
    let before = observations(&h);
    assert!(h.exchange_context(&mut isolated_context).is_err());
    assert_eq!(observations(&h), before);
    foreign.finish_context(&mut isolated, true).unwrap();
    foreign.exchange_context(&mut isolated_context).unwrap();
    let mut nested = h.begin_boundary("nested", [], true).unwrap();
    let outgoing = make(&mut h, 1, &drops);
    let before = observations(&h);
    assert!(h.finish_context(&mut root, true).is_err());
    assert_eq!(observations(&h), before);
    assert!(drops.borrow().is_empty());
    h.finish_boundary(&mut nested, &Ok(())).unwrap();
    h.finish_context(&mut root, false).unwrap();
    let before = observations(&h);
    assert!(h.finish_context(&mut root, true).is_err());
    assert_eq!(observations(&h), before);
    h.exchange_context(&mut context).unwrap();
    assert!(h.require_context(&context, &root).is_err());
    assert_eq!(h.resources.get::<Probe>(&outgoing, "probe").unwrap().0, 1);
    let V::Opaque { token, .. } = &outgoing else {
        panic!()
    };
    assert!(h.resources.values[token].scope.is_none());
    h.exchange_context(&mut other_context).unwrap();
    h.finish_context(&mut other, true).unwrap();
    h.exchange_context(&mut other_context).unwrap();
    h.finish_boundary(&mut outer, &Ok(())).unwrap();
    h.release_frame(&[Some(outgoing)]);
    assert_eq!(*drops.borrow(), [1]);
    assert!(h.resources.scopes.is_empty());
}

#[test]
fn context_admission_failure_preserves_selection_ownership_and_diagnostic_allocator() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let owned = make(&mut h, 1, &drops);
    let V::Opaque { token, .. } = &owned else {
        panic!()
    };
    let forged = V::Opaque {
        ty: "wrong".into(),
        token: token.clone(),
    };
    let before = observations(&h);
    assert!(h.prepare_context([&owned, &forged]).is_err());
    assert_eq!(observations(&h), before);
    h.resources.next_scope = u64::MAX;
    let before = observations(&h);
    assert!(h.prepare_context([&owned]).is_err());
    assert_eq!(observations(&h), before);
    assert!(h.resources.values[token].scope.is_none());
    h.resources.next_scope = 0;
    h.resources.current = Some(u64::MAX);
    let before = observations(&h);
    assert!(h.prepare_context([&owned]).is_err());
    assert_eq!(observations(&h), before);
    h.resources.current = None;
    let (mut root, mut context) = h.prepare_context([&owned]).unwrap();
    let original = context.scope.as_ref().unwrap().clone();
    context.scope.as_mut().unwrap().id = u64::MAX;
    let before = observations(&h);
    assert!(h.exchange_context(&mut context).is_err());
    assert_eq!(observations(&h), before);
    context.scope = Some(original);
    h.exchange_context(&mut context).unwrap();
    h.finish_context(&mut root, true).unwrap();
    h.exchange_context(&mut context).unwrap();
    assert_eq!(*drops.borrow(), [1]);
}

#[test]
fn repeated_context_retirement_shares_unique_diagnostic_ids_and_bounds_live_state() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut identities = BTreeSet::new();
    let mut previous_scope = 0;
    for round in 0..32 {
        let (mut a, mut first) = h.prepare_context([]).unwrap();
        let (mut b, mut second) = h.prepare_context([]).unwrap();
        assert!(a.scope.id > previous_scope && b.scope.id > a.scope.id);
        previous_scope = b.scope.id;
        h.exchange_context(&mut first).unwrap();
        h.enter("A").unwrap();
        let _a = make(&mut h, round * 2, &drops);
        h.tick(1, 1).unwrap();
        h.exchange_context(&mut first).unwrap();
        h.exchange_context(&mut second).unwrap();
        h.enter("B").unwrap();
        let _b = make(&mut h, round * 2 + 1, &drops);
        h.tick(2, 1).unwrap();
        h.exchange_context(&mut second).unwrap();
        for (root, context, name) in [(&mut a, &mut first, "A"), (&mut b, &mut second, "B")] {
            h.require_context(context, root).unwrap();
            h.exchange_context(context).unwrap();
            let report = h
                .activations
                .failure(name, fault::Phase::Execution, "retire");
            assert_eq!(report.activations.len(), 1);
            assert!(identities.insert(report.activations[0].id));
            h.finish_context(root, true).unwrap();
            h.exchange_context(context).unwrap();
        }
        assert!(h.resources.values.is_empty() && h.resources.scopes.is_empty());
        assert_eq!(drops.borrow().len(), (round as usize + 1) * 2);
        assert_eq!(h.contingencies, ["host marker"]);
        assert!(!h.source_faulted());
    }
    assert_eq!(identities.len(), 64);
    assert_eq!(h.fuel, Some(136));
    h.enter("host survived").unwrap();
    h.leave();
}
