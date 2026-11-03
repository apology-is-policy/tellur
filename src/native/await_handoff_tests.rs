//! Source-like callbacks exercise the shared await handoff, not external queues.
use super::*;
use std::{cell::RefCell, rc::Rc};
struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
fn host() -> Host {
    Host::new(serde_json::json!({}), None, true, Some(1000))
}
fn make(c: &mut Contexts<'_>, n: u32, drops: &Rc<RefCell<Vec<u32>>>) -> V {
    c.host
        .resources
        .insert("probe", Probe(n, drops.clone()))
        .unwrap()
}
fn empty(c: &Contexts<'_>) {
    assert!(c.values.is_empty() && c.parked.is_empty() && c.completed.is_empty());
    assert!(c.terminal.is_empty() && c.waits.is_empty() && c.host.tasks.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    assert!(c.host.resources.values.is_empty() && c.host.resources.scopes.is_empty());
}
fn deliver(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    assert!(!a.facts[0], "delivery ran twice");
    a.facts[0] = true;
    let (_, mut fields) = result.outcome()?;
    assert!(matches!(fields.pop(), Some(V::Reference { root, .. }) if root == a.base()));
    v[a.base() + 1] = fields.pop();
    assert!(fields.is_empty());
    Ok(())
}
fn bad_delivery(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    deliver(a, v, result)?;
    Err("E_NATIVE: injected delivery rejection".into())
}
fn never_deliver(_: &mut Activation, _: &mut Frames, _: Completion) -> R<()> {
    panic!("an unsuccessful await must never deliver")
}
fn handoff(
    h: &mut Host,
    v: &mut Frames,
    a: &mut Activation,
    producer: &'static Control,
    deliver: Deliver,
) -> R<Step> {
    let mut args = vec![
        v[a.base() + 1].take().unwrap(),
        V::Reference {
            ty: "&probe".into(),
            root: a.base(),
            fields: vec![],
        },
    ];
    let mut cold = Some(a.defer_task(h, producer, "pending", &mut args)?);
    let task = a.queue_task(h, producer, &mut cold)?;
    assert!(cold.is_none() && args.is_empty());
    a.pc += 1;
    Ok(Step::Await { task, deliver })
}
fn parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    if a.pc == 0 {
        let producer = if a.control.name == "invalid" {
            &INVALID
        } else {
            &CHILD
        };
        let delivery = if a.control.name == "delivery_failure" {
            bad_delivery
        } else {
            deliver
        };
        return handoff(h, v, a, producer, delivery);
    }
    assert!(a.facts[0]);
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()],
    ))))
}
fn child(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    Ok(Step::Complete(Completion::Outcome((
        "ready".into(),
        vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()],
    ))))
}
static PARENT: Control = Control {
    name: "parent",
    slots: 2,
    atoms: 1,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: parent,
};
static CHILD: Control = Control {
    name: "child",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: child,
};
static INVALID: Control = Control {
    name: "invalid_child",
    slots: 1,
    atoms: 0,
    parameters: &[0, 0],
    retained: &[],
    constructor: false,
    resume: child,
};
static INVALID_PARENT: Control = Control {
    name: "invalid",
    ..PARENT
};
static DELIVERY_FAILURE: Control = Control {
    name: "delivery_failure",
    ..PARENT
};
fn start(
    c: &mut Contexts<'_>,
    control: &'static Control,
    drops: &Rc<RefCell<Vec<u32>>>,
) -> ContextKey {
    let args = vec![make(c, 0, drops), make(c, 1, drops)];
    let key = c.start(control, args).unwrap();
    assert!(matches!(
        c.advance_retained(&key, 99).unwrap(),
        ContextPoll::Await
    ));
    assert_eq!(c.parked[&key.id].machine.stack.last().unwrap().pc, 1);
    key
}
fn finish_child(c: &mut Contexts<'_>, parent: &ContextKey) -> ContextKey {
    let child = c.admit_await(parent).unwrap();
    assert!(c.admit_await(parent).is_err());
    assert!(c.advance_retained(parent, 1).is_err());
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Paused
    ));
    let ready = c.park_for_readiness(&child).unwrap();
    assert!(c.advance_retained(&child, 1).is_err());
    c.signal_readiness(&ready).unwrap();
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Ready
    ));
    child
}
#[test]
fn await_handoff_retries_admission_and_delivers_once_without_extra_fuel() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let parent = start(&mut c, &PARENT, &drops);
    let before_fuel = c.host.fuel;
    let next = c.next;
    c.next = u64::MAX;
    let tracker = format!("{:?}", c.parked[&parent.id].host.tracker);
    let registry = format!("{:?}", c.host.tasks);
    assert!(c.admit_await(&parent).is_err());
    assert_eq!(format!("{:?}", c.host.tasks), registry);
    assert_eq!(format!("{:?}", c.parked[&parent.id].host.tracker), tracker);
    assert_eq!(c.host.fuel, before_fuel);
    assert!(matches!(
        c.advance_retained(&parent, 0).unwrap(),
        ContextPoll::Paused
    ));
    assert!(matches!(
        c.advance_retained(&parent, 100).unwrap(),
        ContextPoll::Await
    ));
    assert_eq!(c.host.fuel, before_fuel);
    c.next = next;
    let child = finish_child(&mut c, &parent);
    assert!(c.join(Some(&parent), &child).is_err());
    let foreign = ContextKey {
        session: Arc::new(()),
        id: parent.id,
    };
    assert!(c.return_await(&foreign, &child).is_err());
    let foreign_child = ContextKey {
        session: Arc::new(()),
        id: child.id,
    };
    assert!(c.return_await(&parent, &foreign_child).is_err());
    c.return_await(&parent, &child).unwrap();
    assert!(c.return_await(&parent, &child).is_err());
    assert!(c.reject_await(&parent, "late".into()).is_err());
    assert!(drops.borrow().is_empty());
    let fuel = c.host.fuel;
    assert!(matches!(
        c.advance_retained(&parent, 0).unwrap(),
        ContextPoll::Paused
    ));
    assert!(!c.parked[&parent.id].machine.stack.last().unwrap().facts[0]);
    assert!(matches!(
        c.advance_retained(&parent, 1).unwrap(),
        ContextPoll::Ready
    ));
    assert_eq!(c.host.fuel, fuel.map(|n| n - 1));
    let (_, fields) = c.join(None, &parent).unwrap().outcome().unwrap();
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), [0, 1]);
    empty(&c);
}
#[test]
fn rejected_admission_retires_pending_captures_before_their_borrowed_supplier() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let parent = start(&mut c, &INVALID_PARENT, &drops);
    let error = c.admit_await(&parent).err().unwrap();
    assert!(drops.borrow().is_empty());
    let fuel = c.host.fuel;
    c.reject_await(&parent, error).unwrap();
    assert!(c.reject_await(&parent, "duplicate".into()).is_err());
    assert!(c.admit_await(&parent).is_err());
    assert!(matches!(
        c.advance_retained(&parent, 1).unwrap(),
        ContextPoll::Failed(_)
    ));
    assert_eq!(c.host.fuel, fuel);
    assert_eq!(*drops.borrow(), [1, 0]);
    empty(&c);
}
#[test]
fn rejected_delivery_unwinds_the_original_frame_and_adopted_owners() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let parent = start(&mut c, &DELIVERY_FAILURE, &drops);
    let child = finish_child(&mut c, &parent);
    c.return_await(&parent, &child).unwrap();
    let fuel = c.host.fuel;
    let ContextPoll::Failed(report) = c.advance_retained(&parent, 1).unwrap() else {
        panic!()
    };
    assert!(format!("{report:?}").contains("injected delivery rejection"));
    assert_eq!(c.host.fuel, fuel);
    assert_eq!(*drops.borrow(), [0, 1]);
    empty(&c);
}
fn faulting(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    h.fault(V::String("child failed".into()))
}
static FAULT: Control = Control {
    name: "fault",
    resume: faulting,
    ..CHILD
};
static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "lexical",
    body: 1,
    handler: 3,
    after: 4,
    retained: &[0],
    retire: &[1],
    work: &[1],
    handling: &[],
    fault_slot: 2,
};
fn lexical(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    match a.pc {
        0 => Ok(Step::Boundary(&BOUNDARY)),
        1 => handoff(h, v, a, &FAULT, never_deliver),
        2 => panic!("normal await continuation resumed after a fault"),
        3 => {
            assert!(a.pending.is_none());
            assert_eq!(
                h.resources.values.len(),
                1,
                "captured owner still live in contingency"
            );
            assert!(v[a.base()].is_some());
            h.release_frame(&[v[a.base()].take()]);
            Ok(Step::BoundaryEnd(&BOUNDARY))
        }
        4 => Ok(Step::Complete(Completion::Outcome((
            "recovered".into(),
            vec![],
        )))),
        _ => unreachable!(),
    }
}
static LEXICAL: Control = Control {
    name: "lexical_parent",
    slots: 3,
    resume: lexical,
    ..PARENT
};
#[test]
fn escaping_child_fault_discards_delivery_before_the_owning_contingency() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let args = vec![make(&mut c, 0, &drops), make(&mut c, 1, &drops)];
    let parent = c.start(&LEXICAL, args).unwrap();
    assert!(matches!(
        c.advance_retained(&parent, 99).unwrap(),
        ContextPoll::Await
    ));
    let child = c.admit_await(&parent).unwrap();
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Failed(_)
    ));
    assert_eq!(*drops.borrow(), [1]);
    assert!(c.return_await(&parent, &child).is_err());
    assert!(matches!(
        c.advance_retained(&parent, 99).unwrap(),
        ContextPoll::Ready
    ));
    let (_, fields) = c.join(None, &parent).unwrap().outcome().unwrap();
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), [1, 0]);
    empty(&c);
}
fn settled(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    Ok(Step::Complete(Completion::Settled))
}
static SETTLED: Control = Control {
    name: "settled",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: settled,
};
fn administrative(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    assert_eq!(a.pc, 0);
    a.pc = 1;
    let mut args = vec![];
    let mut cold = Some(a.defer_task(h, &SETTLED, "pending", &mut args)?);
    Ok(Step::Await {
        task: a.queue_task(h, &SETTLED, &mut cold)?,
        deliver: never_deliver,
    })
}
static ADMINISTRATIVE: Control = Control {
    name: "administrative",
    resume: administrative,
    ..SETTLED
};
#[test]
fn administrative_child_completion_propagates_without_normal_delivery() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c.start(&ADMINISTRATIVE, vec![]).unwrap();
    assert!(matches!(
        c.advance_retained(&parent, 10).unwrap(),
        ContextPoll::Await
    ));
    let child = c.admit_await(&parent).unwrap();
    let ContextPoll::Resumed(resumed) = c.advance_retained(&child, 1).unwrap() else {
        panic!()
    };
    assert_eq!(resumed.id, parent.id);
    assert!(c.return_await(&parent, &child).is_err());
    let fuel = c.host.fuel;
    assert!(matches!(
        c.advance_retained(&parent, 1).unwrap(),
        ContextPoll::Complete(Completion::Settled)
    ));
    assert_eq!(c.host.fuel, fuel);
    empty(&c);
}
fn process(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(3, 1)?;
    Ok(Step::Complete(Completion::Process(23)))
}
static PROCESS: Control = Control {
    name: "process",
    resume: process,
    ..SETTLED
};
#[test]
fn process_retirement_releases_returned_undelivered_fields_before_suppliers() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let parent = start(&mut c, &PARENT, &drops);
    let child = finish_child(&mut c, &parent);
    c.return_await(&parent, &child).unwrap();
    let process = c.start(&PROCESS, vec![]).unwrap();
    assert!(matches!(
        c.advance_retained(&process, 1).unwrap(),
        ContextPoll::Complete(Completion::Process(23))
    ));
    assert_eq!(*drops.borrow(), [1, 0]);
    assert!(c.advance_retained(&parent, 1).is_err());
    empty(&c);
}

fn chain_deliver(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    assert!(!a.facts[0]);
    a.facts[0] = true;
    let (_, mut fields) = result.outcome()?;
    v[a.base()] = fields.pop();
    Ok(())
}
fn chain(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(7, 3)?;
    let Some(V::U32(depth)) = v[a.base()] else {
        panic!()
    };
    if a.pc == 0 && depth > 0 {
        a.pc = 1;
        let mut args = vec![V::U32(depth - 1)];
        return if std::ptr::eq(a.control, &CHAIN) {
            let mut cold = Some(a.defer_task(h, &CHAIN, "pending", &mut args)?);
            Ok(Step::Await {
                task: a.queue_task(h, &CHAIN, &mut cold)?,
                deliver: chain_deliver,
            })
        } else {
            Ok(Step::Call {
                control: &DIRECT_CHAIN,
                arguments: args,
                destination: None,
                deliver: chain_deliver,
            })
        };
    }
    if a.pc == 1 {
        assert!(a.facts[0]);
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![V::U32(depth + u32::from(a.pc == 1))],
    ))))
}
static CHAIN: Control = Control {
    name: "chain",
    slots: 1,
    atoms: 1,
    parameters: &[0],
    retained: &[],
    constructor: false,
    resume: chain,
};
static DIRECT_CHAIN: Control = Control { ..CHAIN };
fn baseline(
    fuel: usize,
    quotas: &[usize],
) -> (serde_json::Value, Vec<serde_json::Value>, Option<usize>) {
    let mut h = host();
    h.fuel = Some(fuel);
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut machine =
        Machine::start(&mut h, &mut v, &mut d, &DIRECT_CHAIN, vec![V::U32(8)]).unwrap();
    let result = 'run: loop {
        for q in quotas {
            match machine.advance(&mut h, &mut v, &mut d, *q) {
                Ok(Poll::Paused) => {}
                Ok(Poll::Complete(result)) => {
                    assert!(v.is_empty() && h.tasks.is_empty());
                    return (
                        serde_json::json!({"outcome":result.outcome().unwrap()}),
                        h.events.clone(),
                        h.fuel,
                    );
                }
                Err(error) => break 'run error,
                _ => panic!(),
            }
        }
    };
    (
        serde_json::json!({"fault":h.activations.failure("chain", fault::Phase::Execution, &result)}),
        h.events.clone(),
        h.fuel,
    )
}
#[test]
fn nested_callback_awaits_match_direct_reports_and_fuel_across_quota_partitions() {
    for fuel in 0..=18 {
        for quotas in [&[1][..], &[0, 3, 1][..], &[99][..]] {
            let expected = baseline(fuel, quotas);
            let mut h = host();
            h.fuel = Some(fuel);
            let mut c = Contexts::new(&mut h);
            let root = c.start(&CHAIN, vec![V::U32(8)]).unwrap();
            let mut chain = vec![root.clone()];
            let mut quotas = quotas.iter().cycle();
            let result = loop {
                let key = chain.last().unwrap().clone();
                match c.advance_retained(&key, *quotas.next().unwrap()).unwrap() {
                    ContextPoll::Paused => {}
                    ContextPoll::Await => chain.push(c.admit_await(&key).unwrap()),
                    ContextPoll::Ready if chain.len() > 1 => {
                        chain.pop();
                        c.return_await(chain.last().unwrap(), &key).unwrap();
                    }
                    ContextPoll::Ready => {
                        break serde_json::json!({"outcome":c.join(None, &key).unwrap().outcome().unwrap()});
                    }
                    ContextPoll::Failed(report) if key.id == root.id => {
                        break serde_json::json!({"fault":report});
                    }
                    ContextPoll::Failed(_) => {
                        chain.clear();
                        chain.push(root.clone());
                    }
                    _ => panic!(),
                }
            };
            assert_eq!(
                (result, c.host.events.clone(), c.host.fuel),
                expected,
                "fuel={fuel}"
            );
            empty(&c);
        }
    }
}

fn bridge(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(8, 3)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Call {
            control: &CHAIN,
            arguments: vec![V::U32(2)],
            destination: None,
            deliver: chain_deliver,
        });
    }
    assert!(a.facts[0]);
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
static BRIDGE: Control = Control {
    name: "bridge",
    parameters: &[],
    resume: bridge,
    ..CHAIN
};
#[test]
fn await_in_synchronous_callee_delivers_to_its_original_activation() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let root = c.start(&BRIDGE, vec![]).unwrap();
    assert!(matches!(
        c.advance_retained(&root, 99).unwrap(),
        ContextPoll::Await
    ));
    let machine = &c.parked[&root.id].machine;
    assert_eq!(machine.stack.len(), 2);
    assert!(!machine.stack[0].facts[0]);
    let child = c.admit_await(&root).unwrap();
    assert!(matches!(
        c.advance_retained(&child, 99).unwrap(),
        ContextPoll::Await
    ));
    let leaf = c.admit_await(&child).unwrap();
    assert!(matches!(
        c.advance_retained(&leaf, 99).unwrap(),
        ContextPoll::Ready
    ));
    c.return_await(&child, &leaf).unwrap();
    assert!(matches!(
        c.advance_retained(&child, 99).unwrap(),
        ContextPoll::Ready
    ));
    c.return_await(&root, &child).unwrap();
    assert!(!c.parked[&root.id].machine.stack[0].facts[0]);
    // Only the active callee receives the await return. Its normal completion
    // then invokes the independent saved Call delivery in the bridge frame.
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Paused
    ));
    assert_eq!(c.parked[&root.id].machine.stack.len(), 1);
    assert!(c.parked[&root.id].machine.stack[0].facts[0]);
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Ready
    ));
    assert_eq!(
        c.join(None, &root).unwrap().outcome().unwrap(),
        ("done".into(), vec![V::U32(2)])
    );
    assert_eq!(c.host.fuel, Some(993));
    empty(&c);
}

fn mixed_outer(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(0, 1)?;
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
static MIXED_OUTER: Control = Control {
    name: "outer",
    resume: mixed_outer,
    ..SETTLED
};
#[test]
fn independent_child_await_adopts_owners_and_borrows_once_after_atomic_admission() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let root = c.start(&MIXED_OUTER, vec![]).unwrap();
    let args = vec![make(&mut c, 0, &drops), make(&mut c, 1, &drops)];
    let parent = c.start_child(&root, &PARENT, args).unwrap();
    assert!(matches!(
        c.advance_retained(&parent, 99).unwrap(),
        ContextPoll::Await
    ));
    let next = c.next;
    let before = (
        format!("{:?}", c.host.tasks),
        format!("{:?}", c.parked[&parent.id].host.tracker),
        c.host.fuel,
    );
    c.next = u64::MAX;
    assert!(
        c.admit_await(&parent)
            .unwrap_err()
            .contains("identity exhausted")
    );
    assert_eq!(
        (
            format!("{:?}", c.host.tasks),
            format!("{:?}", c.parked[&parent.id].host.tracker),
            c.host.fuel
        ),
        before
    );
    c.next = next;
    let child = finish_child(&mut c, &parent);
    assert!(c.return_await(&root, &child).is_err());
    c.return_await(&parent, &child).unwrap();
    assert!(c.return_await(&parent, &child).is_err());
    assert!(matches!(
        c.advance_retained(&parent, 0).unwrap(),
        ContextPoll::Paused
    ));
    assert!(!c.parked[&parent.id].machine.stack[0].facts[0]);
    assert!(matches!(
        c.advance_retained(&parent, 1).unwrap(),
        ContextPoll::Ready
    ));
    let (_, fields) = c.join(Some(&root), &parent).unwrap().outcome().unwrap();
    assert!(drops.borrow().is_empty());
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), [0, 1]);
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Ready
    ));
    c.join(None, &root).unwrap();
    empty(&c);
}
#[test]
fn sibling_fault_retires_an_independent_childs_pending_await_and_result_before_suppliers() {
    for phase in 0..=3 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let root = c.start(&MIXED_OUTER, vec![]).unwrap();
        let args = vec![make(&mut c, 0, &drops), make(&mut c, 1, &drops)];
        let parent = c.start_child(&root, &PARENT, args).unwrap();
        assert!(matches!(
            c.advance_retained(&parent, 99).unwrap(),
            ContextPoll::Await
        ));
        let mut wake = None;
        let child = if phase == 1 {
            let child = c.admit_await(&parent).unwrap();
            c.advance_retained(&child, 1).unwrap();
            wake = Some(c.park_for_readiness(&child).unwrap());
            Some(child)
        } else if phase >= 2 {
            let child = finish_child(&mut c, &parent);
            if phase == 3 {
                c.return_await(&parent, &child).unwrap();
            }
            Some(child)
        } else {
            None
        };
        let failed = c
            .start_child(&root, &FAULT, vec![V::Unit, V::Unit])
            .unwrap();
        assert!(matches!(
            c.advance_retained(&failed, 1).unwrap(),
            ContextPoll::Failed(_)
        ));
        assert_eq!(*drops.borrow(), [1, 0], "phase={phase}");
        if let Some(wake) = wake {
            assert!(c.signal_readiness(&wake).is_err());
        }
        if let Some(child) = child {
            assert!(c.return_await(&parent, &child).is_err());
        }
        assert!(c.advance_retained(&parent, 1).is_err());
        let ContextPoll::Failed(report) = c.advance_retained(&root, 1).unwrap() else {
            panic!()
        };
        assert_eq!(report.boundary, "outer");
        assert_eq!(report.cause.unwrap().boundary, "fault");
        empty(&c);
    }
}
fn independent_chain(
    retained: bool,
    fuel: usize,
    quotas: &[usize],
) -> (serde_json::Value, Vec<serde_json::Value>, Option<usize>) {
    let mut h = host();
    h.fuel = Some(fuel);
    let mut c = Contexts::new(&mut h);
    let root = c.start(&MIXED_OUTER, vec![]).unwrap();
    let child = c
        .start_child(
            &root,
            if retained { &CHAIN } else { &DIRECT_CHAIN },
            vec![V::U32(8)],
        )
        .unwrap();
    let mut chain = vec![root.clone(), child];
    let mut child_result = None;
    let mut quotas = quotas.iter().cycle();
    let result = loop {
        let key = chain.last().unwrap().clone();
        match c.advance_retained(&key, *quotas.next().unwrap()).unwrap() {
            ContextPoll::Paused => {}
            ContextPoll::Await => chain.push(c.admit_await(&key).unwrap()),
            ContextPoll::Ready if chain.len() > 2 => {
                chain.pop();
                c.return_await(chain.last().unwrap(), &key).unwrap();
            }
            ContextPoll::Ready if chain.len() == 2 => {
                child_result = Some(c.join(Some(&root), &key).unwrap().outcome().unwrap());
                chain.pop();
            }
            ContextPoll::Ready => {
                break serde_json::json!({"outcome":c.join(None, &key).unwrap().outcome().unwrap(), "child":child_result});
            }
            ContextPoll::Failed(report) if key.id == root.id => {
                break serde_json::json!({"fault":report});
            }
            ContextPoll::Failed(_) => {
                chain.clear();
                chain.push(root.clone());
            }
            _ => panic!(),
        }
    };
    empty(&c);
    (result, c.host.events.clone(), c.host.fuel)
}
#[test]
fn independent_nested_cold_awaits_match_direct_children_at_every_fuel_cut() {
    for fuel in 0..=19 {
        for quotas in [&[1][..], &[0, 3, 1][..], &[99][..]] {
            assert_eq!(
                independent_chain(true, fuel, quotas),
                independent_chain(false, fuel, quotas),
                "fuel={fuel},quotas={quotas:?}"
            );
        }
    }
}

#[test]
fn sibling_fault_restores_the_awaiting_owners_original_report_and_cleanup_order() {
    for source_fault in [false, true] {
        let mut expected = None;
        for phase in 0..=3 {
            let mut h = host();
            let mut c = Contexts::new(&mut h);
            let drops = Rc::new(RefCell::new(vec![]));
            let parent = start(&mut c, &PARENT, &drops);
            // Allocate the independent faulting child before cold admission so its
            // complete cause (including identity) is comparable in every phase.
            let failed = c
                .start_child(&parent, &FAULT, vec![V::Unit, V::Unit])
                .unwrap();
            let mut wake = None;
            let child = if phase == 1 {
                let child = c.admit_await(&parent).unwrap();
                c.advance_retained(&child, 1).unwrap();
                wake = Some(c.park_for_readiness(&child).unwrap());
                Some(child)
            } else if phase >= 2 {
                let child = finish_child(&mut c, &parent);
                if phase == 3 {
                    c.return_await(&parent, &child).unwrap();
                }
                Some(child)
            } else {
                None
            };
            if !source_fault {
                c.host.fuel = Some(0);
            }
            c.advance_retained(&failed, 1).unwrap();
            let ContextPoll::Failed(report) = c.advance_retained(&parent, 1).unwrap() else {
                panic!()
            };
            if let Some(expected) = &expected {
                assert_eq!(&report, expected, "phase={phase}");
            } else {
                expected = Some(report);
            }
            assert_eq!(*drops.borrow(), [1, 0], "phase={phase}");
            if let Some(wake) = wake {
                assert!(c.signal_readiness(&wake).is_err());
            }
            if let Some(child) = child {
                assert!(c.return_await(&parent, &child).is_err());
            }
            empty(&c);
        }
    }
}

#[test]
fn prepared_owner_await_preserves_other_children_and_requires_every_result_join() {
    for cold_first in [false, true] {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let parent = start(&mut c, &PARENT, &drops);
        let base = c.parked[&parent.id].machine.stack[0].base();
        let args = vec![
            make(&mut c, 2, &drops),
            V::Reference {
                ty: "&probe".into(),
                root: base,
                fields: vec![],
            },
        ];
        let other = c.start_child(&parent, &CHILD, args).unwrap();
        let before = (format!("{:?}", c.host.tasks), c.host.fuel);
        let next = c.next;
        c.next = u64::MAX;
        assert!(
            c.admit_await(&parent)
                .unwrap_err()
                .contains("identity exhausted")
        );
        assert_eq!((format!("{:?}", c.host.tasks), c.host.fuel), before);
        c.next = next;
        let child = finish_child(&mut c, &parent);
        assert!(matches!(
            c.advance_retained(&other, 2).unwrap(),
            ContextPoll::Ready
        ));
        if cold_first {
            c.return_await(&parent, &child).unwrap();
        }
        assert!(c.advance_retained(&parent, 1).is_err());
        let (_, fields) = c.join(Some(&parent), &other).unwrap().outcome().unwrap();
        assert!(matches!(&fields[1], V::Reference { root, .. } if *root == base));
        c.host
            .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
        assert_eq!(*drops.borrow(), [2]);
        if !cold_first {
            assert!(c.advance_retained(&parent, 1).is_err());
            c.return_await(&parent, &child).unwrap();
        }
        assert!(matches!(
            c.advance_retained(&parent, 0).unwrap(),
            ContextPoll::Paused
        ));
        assert!(!c.parked[&parent.id].machine.stack[0].facts[0]);
        assert!(matches!(
            c.advance_retained(&parent, 1).unwrap(),
            ContextPoll::Ready
        ));
        let (_, fields) = c.join(None, &parent).unwrap().outcome().unwrap();
        c.host
            .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
        assert_eq!(*drops.borrow(), [2, 0, 1]);
        empty(&c);
    }
}

fn fault_owner(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    handoff(h, v, a, &FAULT, never_deliver)
}
static FAULT_OWNER: Control = Control {
    resume: fault_owner,
    ..PARENT
};
#[test]
fn owner_cold_fault_preserves_its_chain_and_retires_completed_sibling_borrowers() {
    for source_fault in [false, true] {
        let mut expected = None;
        for retained_sibling in [false, true] {
            let mut h = host();
            let mut c = Contexts::new(&mut h);
            let drops = Rc::new(RefCell::new(vec![]));
            let parent = start(&mut c, &FAULT_OWNER, &drops);
            let base = c.parked[&parent.id].machine.stack[0].base();
            let args = vec![
                make(&mut c, 2, &drops),
                V::Reference {
                    ty: "&probe".into(),
                    root: base,
                    fields: vec![],
                },
            ];
            let sibling = c.start_child(&parent, &CHILD, args).unwrap();
            c.advance_retained(&sibling, 2).unwrap();
            if !retained_sibling {
                let (_, fields) = c.join(Some(&parent), &sibling).unwrap().outcome().unwrap();
                c.host
                    .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
            }
            let child = c.admit_await(&parent).unwrap();
            if !source_fault {
                c.host.fuel = Some(0);
            }
            c.advance_retained(&child, 1).unwrap();
            let ContextPoll::Failed(report) = c.advance_retained(&parent, 1).unwrap() else {
                panic!()
            };
            assert!(report.cause.is_none());
            if let Some(expected) = &expected {
                assert_eq!(&report, expected);
            } else {
                expected = Some(report);
            }
            assert_eq!(*drops.borrow(), [2, 1, 0]);
            empty(&c);
        }
    }
}

#[test]
fn machine_selection_preserves_pending_delivery_until_other_children_join() {
    let mut h = host();
    let drops = Rc::new(RefCell::new(vec![]));
    let mut c = Contexts::new(&mut h);
    let root = start(&mut c, &PARENT, &drops);
    let base = c.parked[&root.id].machine.stack[0].base();
    let args = vec![
        make(&mut c, 2, &drops),
        V::Reference {
            ty: "&probe".into(),
            root: base,
            fields: vec![],
        },
    ];
    let sibling = c.start_child(&root, &CHILD, args).unwrap();
    let awaited = c.admit_await(&root).unwrap();
    c.advance_retained(&awaited, 2).unwrap();
    c.return_await(&root, &awaited).unwrap();
    let slots = c.values.to_vec();
    let tasks = c.host.tasks.clone();
    let fuel = c.host.fuel;
    for quota in [1, 99, 0] {
        let p = c.parked.get_mut(&root.id).unwrap();
        c.host.exchange_context(&mut p.host).unwrap();
        assert!(
            p.machine
                .advance(c.host, &mut c.values, &mut c.domains, quota)
                .is_err()
        );
        let a = p.machine.stack.last().unwrap();
        assert!(!a.facts[0]);
        assert!(matches!(&a.pending, Some(Pending::Await(p)) if p.result.is_some()));
        c.host.exchange_context(&mut p.host).unwrap();
        assert_eq!(*c.values, slots);
        assert_eq!(c.host.tasks, tasks);
        assert_eq!(c.host.fuel, fuel);
    }
    c.advance_retained(&sibling, 2).unwrap();
    let (_, fields) = c.join(Some(&root), &sibling).unwrap().outcome().unwrap();
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Ready
    ));
    let (_, fields) = c.join(None, &root).unwrap().outcome().unwrap();
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), [2, 0, 1]);
    empty(&c);
}
