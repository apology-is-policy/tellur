use super::borrower_coverage_tests::{borrow, host, pair, snapshot};
use super::*;

fn deliver(a: &mut Activation, v: &mut Frames, value: Completion) -> R<()> {
    assert!(!a.facts[0]);
    a.facts[0] = true;
    let (_, mut fields) = value.outcome()?;
    assert_eq!(fields.len(), 1);
    v[a.base() + 3] = fields.pop();
    Ok(())
}
fn plan(a: &Activation) -> AccessPlan {
    if a.pc == 0 {
        return AccessPlan {
            cold_await: None,
            transition: Transition::Local,
            accesses: &[Access {
                slot: Some(2),
                fields: &[],
                indirect: false,
                kind: AccessKind::Write,
            }],
            delivery: &[],
            opaque: false,
        };
    }
    if a.pc != 1 {
        return AccessPlan::UNKNOWN;
    }
    AccessPlan {
        cold_await: None,
        transition: Transition::AwaitTask,
        accesses: &[Access {
            slot: Some(1),
            fields: &[],
            indirect: false,
            kind: AccessKind::Move,
        }],
        delivery: &[Access {
            slot: Some(3),
            fields: &[],
            indirect: false,
            kind: AccessKind::Write,
        }],
        opaque: true,
    }
    .with_cold_await(1, &CHILD)
}
fn parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    match a.pc {
        0 => {
            v[a.base() + 2] = Some(V::U32(42));
            a.pc = 1;
            Ok(Step::Continue)
        }
        1 => {
            let mut handle = v[a.base() + 1].take();
            let task = a.queue_task(h, &CHILD, &mut handle)?;
            a.pc = 2;
            Ok(Step::Await { task, deliver })
        }
        _ => {
            assert!(a.facts[0]);
            Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
        }
    }
}
fn child(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
static PARENT: Control = Control {
    name: "await_owner",
    slots: 4,
    atoms: 1,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: Some(plan),
    resume: parent,
};
static CHILD: Control = Control {
    name: "await_child",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: child,
};
static OTHER: Control = Control { ..CHILD }; // Same name, distinct compiled producer.
fn cold(c: &mut Contexts<'_>, root: &ContextKey, producer: &'static Control, value: V) -> V {
    let a = &c.parked[&root.id].machine.stack[0];
    a.defer_task(c.host, producer, "pending", &mut vec![value])
        .unwrap()
}
fn setup(c: &mut Contexts<'_>) -> (ContextKey, ContextKey) {
    let root = c.start(&PARENT, vec![pair()]).unwrap();
    let base = c.parked[&root.id].machine.stack[0].base();
    c.values[base + 1] = Some(cold(c, &root, &CHILD, borrow(c, base, 0)));
    let sibling = c
        .start_child(&root, &CHILD, vec![borrow(c, base, 1)])
        .unwrap();
    (root, sibling)
}
fn reject(c: &mut Contexts<'_>, root: &ContextKey) {
    let before = snapshot(c);
    assert!(c.advance_disjoint_one(root).is_err());
    let progress = c.advance_disjoint(root, 99);
    assert_eq!((progress.instructions, progress.transitions), (0, 0));
    assert!(matches!(progress.stop, AdmissionStop::Blocked(_)));
    assert_eq!(snapshot(c), before);
}
#[test]
fn cold_await_admission_stops_at_handoff_and_keeps_delivery_separate() {
    for state in 0..3 {
        for quota in [1, 99] {
            let mut h = host();
            let mut c = Contexts::new(&mut h);
            let (root, sibling) = setup(&mut c);
            let readiness = if state == 1 {
                c.advance_retained(&sibling, 1).unwrap();
                Some(c.park_for_readiness(&sibling).unwrap())
            } else {
                None
            };
            if state == 2 {
                c.advance_retained(&sibling, 2).unwrap();
            }
            let before = snapshot(&c);
            assert!(matches!(
                c.advance_disjoint(&root, 0).stop,
                AdmissionStop::Quota
            ));
            assert_eq!(snapshot(&c), before);
            let mut progress = c.advance_disjoint(&root, quota);
            if quota == 1 {
                assert!(matches!(progress.stop, AdmissionStop::Quota));
                progress = c.advance_disjoint(&root, quota);
            }
            assert!(matches!(
                progress.stop,
                AdmissionStop::Poll(ContextPoll::Await)
            ));
            let a = &c.parked[&root.id].machine.stack[0];
            let base = a.base();
            assert_eq!(a.pc, 2);
            assert!(!a.facts[0]);
            assert!(c.values[base + 1].is_none());
            assert!(c.values[base + 3].is_none());
            reject(&mut c, &root);
            let awaited = c.admit_await(&root).unwrap();
            assert!(c.admit_await(&root).is_err());
            c.advance_retained(&awaited, 2).unwrap();
            c.return_await(&root, &awaited).unwrap();
            let fuel = c.host.fuel;
            assert!(matches!(
                c.advance_disjoint_one(&root).unwrap(),
                ContextPoll::Paused
            ));
            assert_eq!(c.host.fuel, fuel);
            assert!(c.parked[&root.id].machine.stack[0].facts[0]);
            assert!(matches!(c.values[base + 3], Some(V::Reference { .. })));
            reject(&mut c, &root);
            if let Some(token) = readiness {
                c.signal_readiness(&token).unwrap();
            }
            if state != 2 {
                c.advance_retained(&sibling, 2).unwrap();
            }
            c.join(Some(&root), &sibling).unwrap();
            c.advance_retained(&root, 1).unwrap();
            c.join(None, &root).unwrap();
            assert!(
                c.values.is_empty()
                    && c.host.tasks.is_empty()
                    && c.parked.is_empty()
                    && c.completed.is_empty()
            );
        }
    }
}
#[test]
fn cold_await_admission_rejects_overlap_partial_and_invalid_handle_before_moves() {
    for kind in 0..10 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let (root, sibling) = setup(&mut c);
        c.advance_disjoint_one(&root).unwrap();
        let base = c.parked[&root.id].machine.stack[0].base();
        match kind {
            0 => {
                c.parked.get_mut(&sibling.id).unwrap().dependencies =
                    Dependencies::Unknown("missing".into())
            }
            1 => {
                let mut value = borrow(&c, base, 1);
                if let V::Reference { evidence, .. } = &mut value {
                    evidence.origins = Default::default();
                }
                c.parked.get_mut(&sibling.id).unwrap().dependencies =
                    Dependencies::describe(&c.values, &[value]);
            }
            2 => {
                let value =
                    super::super::super::reference(&c.values, base + 1, &[], false, "&pending")
                        .unwrap();
                let value = super::super::reference_evidence::attach_checked(
                    &c.values,
                    value,
                    &[super::super::reference_evidence::Origin {
                        root: base + 1,
                        fields: &[],
                        indirect: false,
                        guard: false,
                        opaque: false,
                    }],
                    true,
                    true,
                );
                c.parked.get_mut(&sibling.id).unwrap().dependencies =
                    Dependencies::describe(&c.values, &[value]);
            }
            3 => c.host.tasks.exhaust_wait_space(),
            4 => c.values[base + 1] = Some(cold(&mut c, &root, &OTHER, V::Unit)),
            5 => {
                let other = c.start(&PARENT, vec![pair()]).unwrap();
                c.values[base + 1] = Some(cold(&mut c, &other, &CHILD, V::Unit));
            }
            6 => {
                if let Some(V::Task { operation, .. }) = &mut c.values[base + 1] {
                    *operation = "different".into();
                }
            }
            7 => {
                if let Some(V::Task { arguments, .. }) = &mut c.values[base + 1] {
                    arguments.clear();
                }
            }
            8 => {
                let mut foreign_host = host();
                let mut foreign = Contexts::new(&mut foreign_host);
                let other = foreign.start(&PARENT, vec![pair()]).unwrap();
                c.values[base + 1] = Some(cold(&mut foreign, &other, &CHILD, V::Unit));
                foreign.terminate().unwrap();
            }
            _ => c.values[base + 1] = Some(V::Unit),
        }
        reject(&mut c, &root);
        c.terminate().unwrap();
    }
}
fn malformed(a: &Activation) -> AccessPlan {
    let mut p = plan(a);
    match a.facts.iter().position(|x| *x).unwrap() {
        0 => p.cold_await = None,
        1 => p.cold_await.as_mut().unwrap().slot = 0,
        2 => p.transition = Transition::Await,
        3 => p.opaque = true,
        _ => {
            p.delivery = &[Access {
                slot: Some(3),
                fields: &[],
                indirect: false,
                kind: AccessKind::Retire,
            }]
        }
    }
    p
}
static MALFORMED: Control = Control {
    atoms: 5,
    access: Some(malformed),
    ..PARENT
};
#[test]
fn cold_await_admission_requires_exact_operand_and_handoff_descriptor() {
    for kind in 0..5 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let (root, _) = setup(&mut c);
        c.advance_disjoint_one(&root).unwrap();
        let a = &mut c.parked.get_mut(&root.id).unwrap().machine.stack[0];
        a.control = &MALFORMED;
        a.facts = vec![false; 5];
        a.facts[kind] = true;
        reject(&mut c, &root);
        c.terminate().unwrap();
    }
}
#[test]
fn cold_await_witness_rejects_replaced_handle_or_changed_wait_capacity() {
    for capacity in [false, true] {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let (root, _) = setup(&mut c);
        c.advance_disjoint_one(&root).unwrap();
        let witness = c.admit_one_callback(&root).unwrap();
        if capacity {
            c.host.tasks.exhaust_wait_space();
        } else {
            let base = c.parked[&root.id].machine.stack[0].base();
            c.values[base + 1] = Some(cold(&mut c, &root, &CHILD, V::Unit));
        }
        let before = snapshot(&c);
        let p = c.parked.get_mut(&root.id).unwrap();
        c.host.exchange_context(&mut p.host).unwrap();
        assert!(matches!(
            p.machine
                .advance_admitted_one(c.host, &mut c.values, &mut c.domains, witness),
            OneCallbackPoll::Rejected(_)
        ));
        c.host.exchange_context(&mut p.host).unwrap();
        assert_eq!(snapshot(&c), before);
        c.terminate().unwrap();
    }
}
#[test]
fn cold_await_actual_callback_failure_settles_without_consuming_a_handoff() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, _) = setup(&mut c);
    c.advance_disjoint_one(&root).unwrap();
    c.host.fuel = Some(0);
    let p = c.advance_disjoint(&root, 99);
    assert_eq!((p.instructions, p.transitions), (1, 1));
    assert!(matches!(
        p.stop,
        AdmissionStop::Fault(ContextPoll::Failed(_))
    ));
    assert!(c.values.is_empty() && c.host.tasks.is_empty());
    c.terminate().unwrap();
}
