//! Exact diagnostic parity between retained cold awaits and the synchronous chain.
use super::*;
use std::{cell::RefCell, rc::Rc};
fn host() -> Host {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(1000));
    h.activations = fault::Tracker::new(
        (1..=4)
            .map(|id| {
                (
                    id,
                    fault::Site {
                        control: format!("site_{id}"),
                        path: Some("await.tellur".into()),
                        line: id,
                        column: 3,
                        label: Some(format!("path_{id}")),
                        expansion_calls: vec!["expanded".into()],
                        omitted_expansion_calls: 0,
                    },
                )
            })
            .collect(),
    );
    h
}
fn tick(h: &mut Host, a: &Activation) -> R<()> {
    let site = match a.control.name {
        "caller" => 1,
        "middle" => 2,
        _ => 3,
    };
    h.at(site);
    h.tick(site, 3)
}
fn args(v: &mut Frames, a: &Activation) -> Vec<V> {
    vec![
        v[a.base()].as_ref().unwrap().clone(),
        v[a.base() + 1].take().unwrap(),
    ]
}
fn deliver(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let Completion::Outcome((_, mut fields)) = result else {
        return Err("missing outcome".into());
    };
    v[a.base() + 1] = fields.pop();
    Ok(())
}
fn caller(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, a)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Call {
            control: &MIDDLE,
            arguments: args(v, a),
            destination: None,
            deliver,
        });
    }
    h.fault(V::String("caller after join".into()))
}
fn middle(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, a)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Call {
            control: &LEAF,
            arguments: args(v, a),
            destination: None,
            deliver,
        });
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base() + 1].take().unwrap()],
    ))))
}
fn leaf(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, a)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    if matches!(v[a.base()], Some(V::Bool(true))) {
        return h.fault(V::String("leaf failed".into()));
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base() + 1].take().unwrap()],
    ))))
}
fn passive(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, a)?;
        a.pc = 1;
        return Ok(Step::Continue);
    }
    if a.control.name == "caller" {
        caller(h, v, a)
    } else {
        middle(h, v, a)
    }
}
static CALLER: Control = Control {
    name: "caller",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: caller,
};
static MIDDLE: Control = Control {
    name: "middle",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: middle,
};
static LEAF: Control = Control {
    name: "leaf",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: leaf,
};
static PARKED_CALLER: Control = Control {
    name: "caller",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: passive,
};
static PARKED_MIDDLE: Control = Control {
    name: "middle",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: passive,
};
fn queue(
    c: &mut Contexts<'_>,
    p: &ContextKey,
    control: &'static Control,
    mut values: Vec<V>,
) -> Option<QueuedTask> {
    let a = c.parked[&p.id].machine.stack.last().unwrap();
    let mut value = Some(
        a.defer_task(c.host, control, "pending", &mut values)
            .unwrap(),
    );
    Some(a.queue_task(c.host, control, &mut value).unwrap())
}
fn start(c: &mut Contexts<'_>, p: &ContextKey, control: &'static Control) -> ContextKey {
    let a = c.parked[&p.id].machine.stack.last().unwrap();
    let values = args(&mut c.values, a);
    let mut q = queue(c, p, control, values);
    c.start_await(p, &mut q).unwrap()
}
fn join(c: &mut Contexts<'_>, p: &ContextKey, k: &ContextKey) {
    let result = c.join(Some(p), k).unwrap();
    deliver(
        c.parked
            .get_mut(&p.id)
            .unwrap()
            .machine
            .stack
            .last_mut()
            .unwrap(),
        &mut c.values,
        result,
    )
    .unwrap();
}
fn baseline(
    control: &'static Control,
    fail: bool,
) -> (fault::Report, Vec<serde_json::Value>, Option<usize>) {
    baseline_fuel(control, fail, 1000)
}
fn baseline_fuel(
    control: &'static Control,
    fail: bool,
    fuel: usize,
) -> (fault::Report, Vec<serde_json::Value>, Option<usize>) {
    let mut h = host();
    h.fuel = Some(fuel);
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = Machine::start(
        &mut h,
        &mut v,
        &mut d,
        control,
        vec![V::Bool(fail), V::Unit],
    )
    .unwrap();
    let error = loop {
        match m.advance(&mut h, &mut v, &mut d, 1) {
            Ok(Poll::Paused) => {}
            Err(e) => break e,
            _ => panic!("expected fault"),
        }
    };
    let report = h
        .activations
        .failure(control.name, fault::Phase::Execution, &error);
    assert!(v.is_empty() && h.tasks.is_empty());
    (report, h.events.clone(), h.fuel)
}
fn terminal(c: &mut Contexts<'_>, p: &ContextKey) -> fault::Report {
    let ContextPoll::Failed(r) = c.advance_retained(p, 1).unwrap() else {
        panic!("expected fault")
    };
    r
}
#[test]
fn nested_cold_await_success_readiness_and_fault_preserve_exact_call_chain() {
    for fail in [false, true] {
        let expected = baseline(&CALLER, fail);
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let parent = c
            .start(&PARKED_CALLER, vec![V::Bool(fail), V::Unit])
            .unwrap();
        c.advance_retained(&parent, 1).unwrap();
        let mid = start(&mut c, &parent, &PARKED_MIDDLE);
        c.advance_retained(&mid, 1).unwrap();
        let child = start(&mut c, &mid, &LEAF);
        assert!(c.park_for_readiness(&parent).is_err());
        assert!(c.advance_retained(&parent, 1).is_err());
        c.advance_retained(&child, 1).unwrap();
        let wake = c.park_for_readiness(&child).unwrap();
        c.signal_readiness(&wake).unwrap();
        assert!(matches!(
            c.advance_retained(&child, 0).unwrap(),
            ContextPoll::Paused
        ));
        let child_result = c.advance_retained(&child, 1).unwrap();
        if !fail {
            assert!(matches!(child_result, ContextPoll::Ready));
            join(&mut c, &mid, &child);
            assert!(matches!(
                c.advance_retained(&mid, 1).unwrap(),
                ContextPoll::Ready
            ));
            join(&mut c, &parent, &mid);
        } else {
            assert!(matches!(child_result, ContextPoll::Failed(_)));
        }
        let report = terminal(&mut c, &parent);
        assert_eq!(report, expected.0);
        assert_eq!(c.host.events, expected.1);
        assert_eq!(c.host.fuel, expected.2);
        assert!(
            c.host.tasks.is_empty()
                && c.parked.is_empty()
                && c.completed.is_empty()
                && c.values.is_empty()
        );
        assert_eq!(c.domains.len(), 0);
        assert!(c.signal_readiness(&wake).is_err());
    }
}
static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "lexical",
    body: 1,
    handler: 3,
    after: 4,
    retained: &[],
    retire: &[1],
    work: &[1],
    handling: &[],
    fault_slot: 2,
};
fn bounded(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, a)?;
    match a.pc {
        0 => Ok(Step::Boundary(&BOUNDARY)),
        1 => {
            a.pc = 2;
            Ok(Step::Call {
                control: &MIDDLE,
                arguments: args(v, a),
                destination: None,
                deliver,
            })
        }
        3 => {
            h.at(4);
            h.fault(V::String("handler failed".into()))
        }
        _ => panic!("expected fault"),
    }
}
fn parked_bounded(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 1 {
        tick(h, a)?;
        a.pc = 2;
        Ok(Step::Continue)
    } else {
        bounded(h, v, a)
    }
}
static BOUNDED: Control = Control {
    name: "caller",
    slots: 3,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: bounded,
};
static PARKED_BOUNDED: Control = Control {
    name: "caller",
    slots: 3,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: parked_bounded,
};
#[test]
fn nested_await_restores_owning_boundary_chain_before_handler_failure() {
    let expected = baseline(&BOUNDED, true);
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c
        .start(&PARKED_BOUNDED, vec![V::Bool(true), V::Unit])
        .unwrap();
    c.advance_retained(&parent, 2).unwrap();
    let mid = start(&mut c, &parent, &PARKED_MIDDLE);
    c.advance_retained(&mid, 1).unwrap();
    let child = start(&mut c, &mid, &LEAF);
    c.advance_retained(&child, 2).unwrap();
    assert_eq!(c.host.tasks.live_counts(), (1, 0));
    assert_eq!(c.host.events, expected.1);
    assert!(c.parked[&parent.id].awaiting.is_none());
    let report = terminal(&mut c, &parent);
    assert_eq!(report, expected.0);
    assert_eq!(c.host.fuel, expected.2);
    assert!(c.parked.is_empty() && c.host.tasks.is_empty() && c.values.is_empty());
}
struct Probe(Rc<RefCell<usize>>);
impl Drop for Probe {
    fn drop(&mut self) {
        *self.0.borrow_mut() += 1;
    }
}
#[test]
fn sequential_join_adopts_owners_and_keeps_original_borrow_storage() {
    let drops = Rc::new(RefCell::new(0));
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c
        .start(&PARKED_CALLER, vec![V::Bool(false), V::Unit])
        .unwrap();
    c.advance_retained(&parent, 1).unwrap();
    let base = c.parked[&parent.id].machine.stack[0].base();
    let capture = c
        .host
        .resources
        .insert("probe", Probe(drops.clone()))
        .unwrap();
    let value = V::Construct {
        ty: "carrier".into(),
        name: "carrier".into(),
        fields: vec![
            capture,
            V::Reference {
                ty: "&bool".into(),
                root: base,
                fields: vec![],
            },
        ],
    };
    let mut q = queue(&mut c, &parent, &LEAF, vec![V::Bool(false), value]);
    let child = c.start_await(&parent, &mut q).unwrap();
    c.advance_retained(&child, 2).unwrap();
    assert!(c.join(None, &child).is_err());
    assert_eq!(*drops.borrow(), 0);
    let Completion::Outcome((_, fields)) = c.join(Some(&parent), &child).unwrap() else {
        panic!()
    };
    assert_eq!(*drops.borrow(), 0);
    assert!(c.parked[&parent.id].awaiting.is_none());
    let V::Construct { fields: inside, .. } = &fields[0] else {
        panic!()
    };
    assert!(matches!(inside[1],V::Reference{root,..} if root==base));
    assert!(matches!(c.values[base], Some(V::Bool(false))));
    c.host
        .release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), 1);
    assert!(c.join(Some(&parent), &child).is_err());
    terminal(&mut c, &parent);
    assert!(c.host.tasks.is_empty());
}
fn snapshot(c: &Contexts<'_>) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{}|{}",
        c.host.tasks,
        &*c.values,
        c.host.resources.current,
        c.host.fuel,
        c.next,
        c.parked
            .iter()
            .map(|(id, p)| format!(
                "{id}:{}:{:?}:{:?}",
                p.sequential, p.awaiting, p.host.tracker
            ))
            .collect::<Vec<_>>()
            .join(";")
    )
}
#[test]
fn sequential_admission_rejects_mixed_children_and_failed_tracker_before_transfer() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c
        .start(&PARKED_CALLER, vec![V::Bool(false), V::Unit])
        .unwrap();
    let other = c
        .start_child(&parent, &LEAF, vec![V::Bool(false), V::Unit])
        .unwrap();
    let mut q = queue(&mut c, &parent, &LEAF, vec![V::Bool(false), V::Unit]);
    let before = snapshot(&c);
    let capture = format!("{q:?}");
    assert!(c.start_await(&parent, &mut q).is_err());
    assert_eq!(snapshot(&c), before);
    assert_eq!(format!("{q:?}"), capture);
    c.advance_retained(&other, 2).unwrap();
    c.join(Some(&parent), &other).unwrap();
    c.parked.get_mut(&parent.id).unwrap().host.tracker.failure(
        "test",
        fault::Phase::Execution,
        "failed",
    );
    let before = snapshot(&c);
    assert!(
        c.start_await(&parent, &mut q)
            .unwrap_err()
            .contains("already failed")
    );
    assert_eq!(snapshot(&c), before);
    assert_eq!(format!("{q:?}"), capture);
    c.terminate().unwrap();

    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c
        .start(&PARKED_CALLER, vec![V::Bool(false), V::Unit])
        .unwrap();
    let mut q = queue(&mut c, &parent, &LEAF, vec![V::Bool(false), V::Unit]);
    let child = c.start_await(&parent, &mut q).unwrap();
    for key in [&parent, &child] {
        let before = snapshot(&c);
        assert!(
            c.start_child(key, &LEAF, vec![V::Bool(false), V::Unit])
                .is_err()
        );
        assert!(c.start_task(key, &mut None).is_err());
        assert_eq!(snapshot(&c), before);
    }
    c.terminate().unwrap();
    assert!(c.host.tasks.is_empty() && c.values.is_empty());
}

fn administrative(_: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    Ok(Step::Complete(if a.control.name == "process" {
        Completion::Process(23)
    } else {
        Completion::Settled
    }))
}
static PROCESS: Control = Control {
    name: "process",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: administrative,
};
static SETTLED: Control = Control {
    name: "settled",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: administrative,
};
#[test]
fn administrative_await_and_process_retirement_do_not_fabricate_outcomes() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c
        .start(&PARKED_CALLER, vec![V::Bool(false), V::Unit])
        .unwrap();
    c.advance_retained(&parent, 1).unwrap();
    let mut q = queue(&mut c, &parent, &SETTLED, vec![]);
    let child = c.start_await(&parent, &mut q).unwrap();
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Complete(Completion::Settled)
    ));
    assert!(c.join(Some(&parent), &child).is_err());
    assert!(c.parked[&parent.id].awaiting.is_none());
    assert_eq!(c.host.tasks.live_counts(), (1, 0));
    let report = terminal(&mut c, &parent);
    assert_eq!(report.activations.len(), 1);
    assert_eq!(report.activations[0].control, "caller");
    assert_eq!(report.activations[0].id, 1);
    assert!(report.cause.is_none());

    for completed in [false, true] {
        let drops = Rc::new(RefCell::new(0));
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let owner = c
            .host
            .resources
            .insert("probe", Probe(drops.clone()))
            .unwrap();
        let parent = c
            .start(&PARKED_CALLER, vec![V::Bool(false), owner])
            .unwrap();
        c.advance_retained(&parent, 1).unwrap();
        let base = c.parked[&parent.id].machine.stack[0].base();
        let capture = c
            .host
            .resources
            .insert("probe", Probe(drops.clone()))
            .unwrap();
        let value = V::Construct {
            ty: "carrier".into(),
            name: "carrier".into(),
            fields: vec![
                capture,
                V::Reference {
                    ty: "&probe".into(),
                    root: base + 1,
                    fields: vec![],
                },
            ],
        };
        let mut q = queue(&mut c, &parent, &LEAF, vec![V::Bool(false), value]);
        let child = c.start_await(&parent, &mut q).unwrap();
        c.advance_retained(&child, 1).unwrap();
        let wake = c.park_for_readiness(&child).unwrap();
        if completed {
            c.signal_readiness(&wake).unwrap();
            c.advance_retained(&child, 1).unwrap();
        }
        let stop = c.start(&PROCESS, vec![]).unwrap();
        assert!(matches!(
            c.advance_retained(&stop, 1).unwrap(),
            ContextPoll::Complete(Completion::Process(23))
        ));
        assert_eq!(*drops.borrow(), 2);
        assert!(c.host.events.is_empty());
        assert!(
            c.host.tasks.is_empty()
                && c.values.is_empty()
                && c.parked.is_empty()
                && c.completed.is_empty()
                && c.waits.is_empty()
        );
        assert_eq!(c.domains.len(), 0);
        assert!(c.signal_readiness(&wake).is_err());
        assert!(c.join(Some(&parent), &child).is_err());
    }
}

fn recovering(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc < 3 {
        return bounded(h, v, a);
    }
    tick(h, a)?;
    match a.pc {
        3 => Ok(Step::BoundaryEnd(&BOUNDARY)),
        4 => {
            a.pc = 5;
            Ok(Step::Call {
                control: &LEAF,
                arguments: vec![V::Bool(true), V::Unit],
                destination: None,
                deliver,
            })
        }
        _ => panic!("expected new child fault"),
    }
}
fn parked_recovering(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 1 || a.pc == 4 {
        tick(h, a)?;
        a.pc += 1;
        Ok(Step::Continue)
    } else {
        recovering(h, v, a)
    }
}
static RECOVERING: Control = Control {
    name: "caller",
    slots: 3,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: recovering,
};
static PARKED_RECOVERING: Control = Control {
    name: "caller",
    slots: 3,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    resume: parked_recovering,
};
#[test]
fn recovered_await_can_start_a_new_chain_with_original_caller_and_fresh_child_identity() {
    let expected = baseline(&RECOVERING, true);
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c
        .start(&PARKED_RECOVERING, vec![V::Bool(true), V::Unit])
        .unwrap();
    c.advance_retained(&parent, 2).unwrap();
    let mid = start(&mut c, &parent, &PARKED_MIDDLE);
    c.advance_retained(&mid, 1).unwrap();
    let child = start(&mut c, &mid, &LEAF);
    c.advance_retained(&child, 2).unwrap();
    assert!(c.parked[&parent.id].awaiting.is_none());
    c.advance_retained(&parent, 1).unwrap();
    assert_eq!(c.parked[&parent.id].machine.stack[0].pc, 5);
    let mut q = queue(&mut c, &parent, &LEAF, vec![V::Bool(true), V::Unit]);
    let next = c.start_await(&parent, &mut q).unwrap();
    c.advance_retained(&next, 2).unwrap();
    let report = terminal(&mut c, &parent);
    assert_eq!(report, expected.0);
    assert_eq!(c.host.events, expected.1);
    assert_eq!(c.host.fuel, expected.2);
    assert_eq!(
        report.activations.iter().map(|a| a.id).collect::<Vec<_>>(),
        [1, 6]
    );
    assert!(report.cause.is_none());
    assert!(c.host.tasks.is_empty() && c.values.is_empty());
}

#[test]
fn await_fuel_exhaustion_keeps_host_failure_kind_and_original_chain_at_each_return_cut() {
    for fuel in 2..=6 {
        let expected = baseline_fuel(&CALLER, false, fuel);
        let mut h = host();
        h.fuel = Some(fuel);
        let mut c = Contexts::new(&mut h);
        let parent = c
            .start(&PARKED_CALLER, vec![V::Bool(false), V::Unit])
            .unwrap();
        c.advance_retained(&parent, 1).unwrap();
        let mid = start(&mut c, &parent, &PARKED_MIDDLE);
        c.advance_retained(&mid, 1).unwrap();
        let child = start(&mut c, &mid, &LEAF);
        if matches!(c.advance_retained(&child, 2).unwrap(), ContextPoll::Ready) {
            join(&mut c, &mid, &child);
            if matches!(c.advance_retained(&mid, 1).unwrap(), ContextPoll::Ready) {
                join(&mut c, &parent, &mid);
            }
        }
        let report = terminal(&mut c, &parent);
        assert_eq!(report, expected.0, "fuel={fuel}");
        assert_eq!(c.host.events, expected.1);
        assert_eq!(c.host.fuel, expected.2);
        assert!(c.host.tasks.is_empty() && c.values.is_empty());
        assert_eq!(c.domains.len(), 0);
    }
}
