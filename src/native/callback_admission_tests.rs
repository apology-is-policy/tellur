use super::borrower_coverage_tests::{borrow, host, pair, prepare_await, snapshot};
use super::*;

fn plan(a: &Activation) -> AccessPlan {
    AccessPlan {
        transition: if a.pc < 2 {
            Transition::Local
        } else {
            Transition::Effect
        },
        accesses: if a.pc == 0 {
            &[Access {
                slot: Some(0),
                fields: &[1],
                indirect: false,
                kind: AccessKind::Write,
            }]
        } else {
            &[Access {
                slot: Some(0),
                fields: &[0],
                indirect: false,
                kind: AccessKind::Write,
            }]
        },
        delivery: &[],
        opaque: a.pc >= 2,
    }
}
fn parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    let V::Construct { fields, .. } = v[a.base()].as_mut().unwrap() else {
        panic!()
    };
    fields[if a.pc == 0 { 1 } else { 0 }] = V::U32(42);
    a.facts[0] = true;
    a.pc += 1;
    Ok(Step::Continue)
}
fn child(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
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
    Ok(Step::Complete(Completion::Outcome(("done".into(), fields))))
}
static PARENT: Control = Control {
    name: "admission_parent",
    slots: 2,
    atoms: 1,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: Some(plan),
    resume: parent,
};
static CHILD: Control = Control {
    name: "admission_child",
    atoms: 0,
    access: None,
    resume: child,
    ..PARENT
};
fn setup(c: &mut Contexts<'_>) -> (ContextKey, ContextKey) {
    let root = c.start(&PARENT, vec![pair()]).unwrap();
    let base = c.parked[&root.id].machine.stack[0].base();
    let child = c
        .start_child(&root, &CHILD, vec![borrow(c, base, 0)])
        .unwrap();
    (root, child)
}
fn reject(c: &mut Contexts<'_>, root: &ContextKey) {
    let before = snapshot(c);
    let started = c.parked.get(&root.id).map(|p| p.started);
    assert!(c.advance_disjoint_one(root).is_err());
    assert_eq!(snapshot(c), before);
    assert_eq!(c.parked.get(&root.id).map(|p| p.started), started);
}

#[test]
fn exactly_one_disjoint_callback_advances_beside_live_waiting_or_completed_children() {
    for state in 0..3 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let (root, child) = setup(&mut c);
        if state == 1 {
            c.advance_retained(&child, 1).unwrap();
            c.park_for_readiness(&child).unwrap();
        }
        if state == 2 {
            c.advance_retained(&child, 2).unwrap();
        }
        let before = snapshot(&c);
        for quota in [0, 1, 99] {
            assert!(c.advance_retained(&root, quota).is_err());
            assert_eq!(snapshot(&c), before);
        }
        let fuel = c.host.fuel;
        assert!(matches!(
            c.advance_disjoint_one(&root).unwrap(),
            ContextPoll::Paused
        ));
        assert_eq!(c.host.fuel, fuel.map(|n| n - 1));
        let a = &c.parked[&root.id].machine.stack[0];
        assert_eq!(a.pc, 1);
        assert!(a.facts[0]);
        assert!(
            matches!(&c.values[a.base()],Some(V::Construct{fields,..}) if fields==&[V::U32(7),V::U32(42)])
        );
        reject(&mut c, &root);
        c.terminate().unwrap();
    }
}

#[test]
fn each_public_admission_rechecks_new_borrowers_incomplete_evidence_and_opaque_callbacks() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let root = c.start(&PARENT, vec![pair()]).unwrap();
    let sibling = c.start_child(&root, &CHILD, vec![V::Unit]).unwrap();
    c.advance_disjoint_one(&root).unwrap();
    let base = c.parked[&root.id].machine.stack[0].base();
    let child = c
        .start_child(&root, &CHILD, vec![borrow(&c, base, 0)])
        .unwrap();
    reject(&mut c, &root);
    c.advance_retained(&child, 2).unwrap();
    c.join(Some(&root), &child).unwrap();
    let saved = c.parked[&sibling.id].dependencies.clone();
    c.parked.get_mut(&sibling.id).unwrap().dependencies = Dependencies::Unknown("probe".into());
    reject(&mut c, &root);
    c.parked.get_mut(&sibling.id).unwrap().dependencies = saved;
    c.advance_disjoint_one(&root).unwrap();
    assert_eq!(c.parked[&root.id].machine.stack[0].pc, 2);
    reject(&mut c, &root);
    c.terminate().unwrap();
}

#[test]
fn lower_admission_rejects_wrong_invocation_changed_facts_or_changed_callback_before_unwind() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, _) = setup(&mut c);
    let other = c.start(&PARENT, vec![pair()]).unwrap();
    for kind in 0..3 {
        let admission = c.admit_one_callback(&root).unwrap();
        let selected = if kind == 2 { &other } else { &root };
        let p = c.parked.get_mut(&selected.id).unwrap();
        if kind == 0 {
            p.machine.stack[0].pc = 1;
        }
        if kind == 1 {
            p.machine.stack[0].facts[0] = true;
        }
        let before = snapshot(&c);
        let p = c.parked.get_mut(&selected.id).unwrap();
        c.host.exchange_context(&mut p.host).unwrap();
        assert!(matches!(
            p.machine
                .advance_admitted_one(c.host, &mut c.values, &mut c.domains, admission),
            OneCallbackPoll::Rejected(_)
        ));
        c.host.exchange_context(&mut p.host).unwrap();
        assert_eq!(snapshot(&c), before);
        let a = &mut c.parked.get_mut(&root.id).unwrap().machine.stack[0];
        a.pc = 0;
        a.facts[0] = false;
    }
    c.terminate().unwrap();
}

#[test]
fn admitted_pending_delivery_stops_before_the_next_callback_and_consumes_no_fuel() {
    fn never(_: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
        panic!("delivery must not run the next callback")
    }
    fn opaque(_: &Activation) -> AccessPlan {
        AccessPlan::UNKNOWN
    }
    static AFTER: Control = Control {
        name: "delivery_next",
        slots: 2,
        atoms: 0,
        parameters: &[0],
        retained: &[],
        constructor: false,
        access: Some(opaque),
        resume: never,
    };
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let root = prepare_await(&mut c);
    let child = c.admit_await(&root).unwrap();
    c.advance_retained(&child, 2).unwrap();
    c.return_await(&root, &child).unwrap();
    c.parked.get_mut(&root.id).unwrap().machine.stack[0].control = &AFTER;
    c.start_child(&root, &CHILD, vec![V::Unit]).unwrap();
    let fuel = c.host.fuel;
    assert!(matches!(
        c.advance_disjoint_one(&root).unwrap(),
        ContextPoll::Paused
    ));
    assert_eq!(c.host.fuel, fuel);
    let a = &c.parked[&root.id].machine.stack[0];
    assert!(a.pending.is_none());
    assert!(matches!(c.values[a.base() + 1], Some(V::Reference { .. })));
    reject(&mut c, &root);
    c.terminate().unwrap();
}

#[test]
fn admitted_fuel_failure_settles_real_borrowers_before_supplier_owners() {
    use std::{cell::RefCell, rc::Rc};
    struct Probe(u32, Rc<RefCell<Vec<u32>>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    static OWNER_CHILD: Control = Control {
        parameters: &[0, 1],
        ..CHILD
    };
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let owner = h
        .resources
        .insert("probe", Probe(0, drops.clone()))
        .unwrap();
    let value = V::Construct {
        ty: "pair".into(),
        name: "pair".into(),
        fields: vec![V::U32(7), owner],
    };
    let mut c = Contexts::new(&mut h);
    let root = c.start(&PARENT, vec![value]).unwrap();
    let base = c.parked[&root.id].machine.stack[0].base();
    let child_owner = c
        .host
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    c.start_child(&root, &OWNER_CHILD, vec![borrow(&c, base, 0), child_owner])
        .unwrap();
    c.host.fuel = Some(0);
    assert!(matches!(
        c.advance_disjoint_one(&root).unwrap(),
        ContextPoll::Failed(_)
    ));
    assert_eq!(*drops.borrow(), [1, 0]);
    assert!(c.values.is_empty());
    assert!(c.advance_disjoint_one(&root).is_err());
    c.terminate().unwrap();
    assert_eq!(*drops.borrow(), [1, 0]);
}
