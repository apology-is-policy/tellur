//! Exercise retirement beneath the selection guard with real retained borrowers.
use super::*;
use std::{cell::RefCell, rc::Rc};

struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
fn reset(a: &mut Activation, v: &mut Frames, _: Vec<V>) -> R<()> {
    v[a.base() + 1] = None;
    Ok(())
}
static LOOP: LoopControl = LoopControl {
    target: 0,
    body: 11,
    after: 10,
    locals: &[1],
    reset,
};
static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "inner",
    body: 15,
    handler: 15,
    after: 10,
    retained: &[0],
    retire: &[1],
    work: &[1],
    handling: &[2],
    fault_slot: 3,
};
fn deliver(_: &mut Activation, _: &mut Frames, _: Completion) -> R<()> {
    Ok(())
}
fn parent(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    Ok(match a.pc {
        10 => Step::Complete(Completion::Outcome((
            "done".into(),
            vec![v[a.base()].as_ref().unwrap().clone()],
        ))),
        11 => Step::Complete(Completion::Again(0, vec![])),
        12 => Step::Complete(Completion::Recover(0, vec![])),
        13 => Step::Complete(Completion::Complete(0)),
        14 => Step::Loop {
            control: &LOOP,
            arguments: vec![],
        },
        15 => Step::BoundaryEnd(&BOUNDARY),
        16 => Step::Complete(Completion::Constructed),
        20 => Step::Boundary(&BOUNDARY),
        21 => Step::Supervise {
            work: &WORK,
            handler: &WORK,
            arguments: vec![V::U32(7)],
        },
        22 | 23 => {
            let constructor = a.pc == 23;
            a.pc = 10;
            Step::Call {
                control: if constructor { &CONSTRUCTOR } else { &WORK },
                arguments: vec![V::U32(7)],
                destination: constructor.then_some(a.base() + 4),
                deliver,
            }
        }
        _ => unreachable!(),
    })
}
fn work(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.control.constructor {
        Ok(Step::Complete(Completion::Constructed))
    } else {
        Ok(Step::Complete(Completion::Outcome((
            "done".into(),
            vec![v[a.base()].as_ref().unwrap().clone()],
        ))))
    }
}
static PARENT: Control = Control {
    name: "parent",
    slots: 5,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    resume: parent,
};
static WORK: Control = Control {
    name: "work",
    resume: work,
    ..PARENT
};
static CONSTRUCTOR: Control = Control {
    name: "constructor",
    constructor: true,
    ..WORK
};
fn child(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()],
    ))))
}
static CHILD: Control = Control {
    name: "borrower",
    slots: 2,
    parameters: &[0, 1],
    resume: child,
    ..PARENT
};

// Deliberately bypass Contexts::advance_retained's earlier parent-selection guard.
// These callbacks do not move operands; this checks runtime transition preflight,
// not permission to execute arbitrary generated source while children are live.
fn raw_step(c: &mut Contexts<'_>, key: &ContextKey) -> R<(bool, Option<Completion>)> {
    let p = c.parked.get_mut(&key.id).unwrap();
    c.host.exchange_context(&mut p.host)?;
    let result = p.machine.step(c.host, &mut c.values, &mut c.domains);
    c.host.exchange_context(&mut p.host)?;
    result
}
fn pc(c: &mut Contexts<'_>, key: &ContextKey, pc: usize) {
    c.parked
        .get_mut(&key.id)
        .unwrap()
        .machine
        .stack
        .last_mut()
        .unwrap()
        .pc = pc;
}
fn snapshot(c: &Contexts<'_>, key: &ContextKey) -> String {
    let p = &c.parked[&key.id];
    format!(
        "{:?}",
        (
            &*c.values,
            &c.host.tasks,
            &p.host.tracker,
            p.machine
                .stack
                .iter()
                .map(|a| (a.owner, a.base(), a.pc, a.scopes.len(), a.pending.is_some()))
                .collect::<Vec<_>>(),
            &p.machine.current,
            c.domains.len(),
            c.domains.member_count(),
            &c.host.resources.scopes,
            &c.host.events,
            c.host.fuel,
        )
    )
}
fn empty(c: &Contexts<'_>) {
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.values.is_empty());
    assert!(c.host.tasks.is_empty() && c.host.resources.values.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
}

#[test]
fn runtime_retirement_rejects_live_waiting_and_completed_borrowers_before_mutation() {
    // Root return, loop restart/recovery/exit/entry, lexical end, outer return
    // through a lexical scope, named work, synchronous call and constructor.
    for transition in 0..10 {
        for phase in 0..3 {
            let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
            let drops = Rc::new(RefCell::new(vec![]));
            let value = h
                .resources
                .insert("probe", Probe(0, drops.clone()))
                .unwrap();
            let mut c = Contexts::new(&mut h);
            let root = c.start(&PARENT, vec![value]).unwrap();
            if (1..=3).contains(&transition) {
                pc(&mut c, &root, 14);
                raw_step(&mut c, &root).unwrap();
            }
            if transition == 5 || transition == 6 {
                pc(&mut c, &root, 20);
                raw_step(&mut c, &root).unwrap();
            }
            if transition >= 7 {
                pc(&mut c, &root, transition + 14);
                raw_step(&mut c, &root).unwrap();
            } else {
                pc(
                    &mut c,
                    &root,
                    if transition == 6 { 10 } else { 10 + transition },
                );
            }
            let base = c.parked[&root.id].machine.stack.last().unwrap().base();
            c.values[base + 1] = Some(V::U32(91));
            let child_owner = c
                .host
                .resources
                .insert("probe", Probe(1, drops.clone()))
                .unwrap();
            let borrower = c
                .start_child(
                    &root,
                    &CHILD,
                    vec![
                        child_owner,
                        V::Reference {
                            ty: "&value".into(),
                            root: base + 1,
                            fields: vec![],
                        },
                    ],
                )
                .unwrap();
            let wake = if phase == 1 {
                assert!(matches!(
                    c.advance_retained(&borrower, 1).unwrap(),
                    ContextPoll::Paused
                ));
                Some(c.park_for_readiness(&borrower).unwrap())
            } else {
                None
            };
            if phase == 2 {
                assert!(matches!(
                    c.advance_retained(&borrower, 2).unwrap(),
                    ContextPoll::Ready
                ));
            }
            let before = snapshot(&c, &root);
            assert!(c.advance_retained(&root, 1).is_err());
            assert_eq!(snapshot(&c, &root), before);
            let error = raw_step(&mut c, &root)
                .err()
                .expect("retirement must reject");
            assert!(
                error.contains("retained borrower children"),
                "transition={transition} phase={phase}: {error}"
            );
            assert_eq!(
                snapshot(&c, &root),
                before,
                "transition={transition} phase={phase}"
            );
            assert_eq!(c.values[base + 1], Some(V::U32(91)));
            assert!(drops.borrow().is_empty());
            c.parked[&root.id]
                .machine
                .require_domain(&c.domains)
                .unwrap();
            c.terminate().unwrap();
            if let Some(wake) = wake {
                assert!(c.signal_readiness(&wake).is_err());
            }
            assert!(c.join(Some(&root), &borrower).is_err());
            empty(&c);
            assert_eq!(*drops.borrow(), [1, 0]);
        }
    }
}

#[test]
fn joining_the_last_result_allows_parent_retirement_and_preserves_borrowed_storage() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let drops = Rc::new(RefCell::new(vec![]));
    let value = h
        .resources
        .insert("probe", Probe(0, drops.clone()))
        .unwrap();
    let mut c = Contexts::new(&mut h);
    let root = c.start(&PARENT, vec![value]).unwrap();
    pc(&mut c, &root, 10);
    let base = c.parked[&root.id].machine.stack[0].base();
    c.values[base + 1] = Some(V::U32(91));
    let value = c
        .host
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    let child = c
        .start_child(
            &root,
            &CHILD,
            vec![
                value,
                V::Reference {
                    ty: "&value".into(),
                    root: base + 1,
                    fields: vec![],
                },
            ],
        )
        .unwrap();
    assert!(matches!(
        c.advance_retained(&child, 2).unwrap(),
        ContextPoll::Ready
    ));
    assert!(raw_step(&mut c, &root).is_err());
    let (_, fields) = c.join(Some(&root), &child).unwrap().outcome().unwrap();
    let V::Reference { root: borrowed, .. } = fields[1] else {
        panic!()
    };
    assert_eq!(c.values[borrowed], Some(V::U32(91)));
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), [1]);
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Ready
    ));
    let (_, fields) = c.join(None, &root).unwrap().outcome().unwrap();
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    empty(&c);
    assert_eq!(*drops.borrow(), [1, 0]);
}

#[test]
fn final_frame_checks_domain_authority_before_retiring_even_for_retained_results() {
    for retained in [false, true] {
        let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
        let drops = Rc::new(RefCell::new(vec![]));
        let value = h
            .resources
            .insert("probe", Probe(0, drops.clone()))
            .unwrap();
        let mut c = Contexts::new(&mut h);
        let root = c.start(&PARENT, vec![value]).unwrap();
        pc(&mut c, &root, 10);
        let m = &mut c.parked.get_mut(&root.id).unwrap().machine;
        m.retain_completion = retained;
        // A domain without a member is still an outstanding lifetime obligation.
        let nested = c.domains.prepare(Some(&m.current)).unwrap();
        c.domains.commit(&nested).unwrap();
        let before = snapshot(&c, &root);
        assert!(
            raw_step(&mut c, &root)
                .err()
                .unwrap()
                .contains("live nested boundaries")
        );
        assert_eq!(snapshot(&c, &root), before);
        assert!(!c.parked[&root.id].machine.finished);
        assert!(drops.borrow().is_empty());
        c.domains.close(&nested, &nested.key).unwrap();
        c.terminate().unwrap();
        empty(&c);
        assert_eq!(*drops.borrow(), [0]);
    }
}

#[test]
fn runtime_retirement_failure_hands_off_settlement_before_supplier_unwind() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let drops = Rc::new(RefCell::new(vec![]));
    let value = h
        .resources
        .insert("probe", Probe(0, drops.clone()))
        .unwrap();
    let mut c = Contexts::new(&mut h);
    let root = c.start(&PARENT, vec![value]).unwrap();
    pc(&mut c, &root, 10);
    let base = c.parked[&root.id].machine.stack[0].base();
    let value = c
        .host
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    let child = c
        .start_child(
            &root,
            &CHILD,
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
    let p = c.parked.get_mut(&root.id).unwrap();
    c.host.exchange_context(&mut p.host).unwrap();
    // Exercise the real Machine error path, deliberately below the public guard.
    let Poll::Settling(domain) = p
        .machine
        .advance(c.host, &mut c.values, &mut c.domains, 1)
        .unwrap()
    else {
        panic!("borrowers must settle before unwinding the supplier")
    };
    let message = p.machine.fault.clone().unwrap();
    let report = c
        .host
        .activations
        .failure("parent", fault::Phase::Execution, &message);
    assert_eq!(p.machine.stack.len(), 1);
    assert!(c.values[base].is_some() && drops.borrow().is_empty());
    assert!(!c.host.source_faulted());
    c.host.exchange_context(&mut p.host).unwrap();
    c.settle(&domain, root.id, &message, report, false).unwrap();
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Failed(_)
    ));
    assert!(c.host.events.is_empty());
    assert!(c.join(Some(&root), &child).is_err());
    empty(&c);
    assert_eq!(*drops.borrow(), [1, 0]);
}
