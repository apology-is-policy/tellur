//! Lexical retirement follows owned cold payloads without consuming moved handles.
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
fn never(_: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    panic!("a discarded cold body must not run")
}
static COLD: Control = Control {
    name: "cold",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: never,
};
static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "lexical",
    body: 1,
    handler: 3,
    after: 4,
    retained: &[0],
    retire: &[1, 2],
    work: &[1, 2],
    handling: &[3],
    fault_slot: 4,
};
fn lexical(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    match a.pc {
        0 => Ok(Step::Boundary(&BOUNDARY)),
        1 => h.fault(V::String("work failed".into())),
        2 | 3 => Ok(Step::BoundaryEnd(&BOUNDARY)),
        4 => Ok(Step::Complete(Completion::Outcome(("done".into(), vec![])))),
        _ => unreachable!(),
    }
}
static LEXICAL: Control = Control {
    name: "owner",
    slots: 6,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: lexical,
};
fn setup(h: &mut Host, v: &mut Frames, d: &mut Domains, owner: V) -> Machine {
    let mut m = Machine::start(h, v, d, &LEXICAL, vec![owner]).unwrap();
    assert!(matches!(m.advance(h, v, d, 1).unwrap(), Poll::Paused));
    m
}
fn cold(h: &mut Host, a: &Activation, value: V) -> V {
    a.defer_task(h, &COLD, "pending", &mut vec![value]).unwrap()
}
fn drain(h: &mut Host, a: &Activation, value: V) -> Vec<V> {
    let mut value = Some(value);
    let mut q = a.queue_task(h, &COLD, &mut value).unwrap();
    let (_, args) = q.dispatch(h).unwrap();
    q.complete(h).unwrap();
    q.require_delivery(h, a).unwrap();
    q.deliver(h);
    args
}

#[test]
fn graph_retirement_checks_foreign_payloads_before_mutation_and_does_not_follow_loans() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = setup(&mut h, &mut v, &mut d, V::Unit);
    let local = cold(&mut h, &m.stack[0], V::U32(10));
    let mut other = host();
    let mut ov = Frames::default();
    let mut od = Domains::default();
    let mut om = setup(&mut other, &mut ov, &mut od, V::Unit);
    let foreign = cold(&mut other, &om.stack[0], V::U32(20));
    let graph = V::Construct {
        ty: "carrier".into(),
        name: "carrier".into(),
        fields: vec![local.clone(), foreign],
    };
    let before = h.tasks.clone();
    let foreign_before = other.tasks.clone();
    assert!(
        task_payload::retire_tasks(&mut h, [&graph], true)
            .unwrap_err()
            .contains("another host")
    );
    assert_eq!(h.tasks, before);
    assert_eq!(other.tasks, foreign_before);
    assert!(
        task_payload::retire_tasks(&mut h, [&local], false)
            .unwrap_err()
            .contains("discarded normally")
    );
    assert_eq!(h.tasks, before);
    v[m.stack[0].base() + 5] = Some(local.clone());
    let loan = V::Reference {
        ty: "&pending".into(),
        root: m.stack[0].base() + 5,
        fields: vec![],
    };
    task_payload::retire_tasks(&mut h, [&loan], true).unwrap();
    assert_eq!(h.tasks, before);
    let nested = cold(&mut h, &m.stack[0], local);
    task_payload::retire_tasks(&mut h, [&nested], true).unwrap();
    assert_eq!(h.tasks.live_counts(), (1, 0));
    task_payload::retire_tasks(&mut h, [&nested], true).unwrap();
    m.abort(&mut h, &mut v, &mut d, "test retirement").unwrap();
    om.abort(&mut other, &mut ov, &mut od, "test retirement")
        .unwrap();
    assert!(h.tasks.is_empty() && other.tasks.is_empty());
}

#[test]
fn lexical_fault_retires_cold_borrowers_and_captures_before_handler_without_losing_moved_task() {
    for _ in 0..8 {
        let mut h = host();
        let mut v = Frames::default();
        let mut d = Domains::default();
        let drops = Rc::new(RefCell::new(vec![]));
        let owner = h
            .resources
            .insert("probe", Probe(0, drops.clone()))
            .unwrap();
        let mut m = setup(&mut h, &mut v, &mut d, owner);
        let a = &m.stack[0];
        let base = a.base();
        let captured = h
            .resources
            .insert("probe", Probe(1, drops.clone()))
            .unwrap();
        v[base + 1] = Some(cold(&mut h, a, captured));
        v[base + 2] = Some(cold(
            &mut h,
            a,
            V::Reference {
                ty: "&probe".into(),
                root: base,
                fields: vec![],
            },
        ));
        v[base + 5] = Some(cold(&mut h, a, V::U32(91)));
        assert_eq!(h.tasks.live_counts(), (1, 3));
        assert!(matches!(
            m.advance(&mut h, &mut v, &mut d, 1).unwrap(),
            Poll::Paused
        ));
        assert_eq!(m.stack[0].pc, 3);
        assert_eq!(h.tasks.live_counts(), (1, 1));
        assert_eq!(*drops.borrow(), [1]);
        assert!(v[base + 1].is_none() && v[base + 2].is_none());
        assert_eq!(h.events.len(), 1);
        assert_eq!(h.events[0]["kind"], "fault");
        let kept = v[base + 5].take().unwrap();
        assert_eq!(drain(&mut h, &m.stack[0], kept), [V::U32(91)]);
        assert!(matches!(
            m.advance(&mut h, &mut v, &mut d, 1).unwrap(),
            Poll::Complete(_)
        ));
        assert_eq!(*drops.borrow(), [1, 0]);
        assert!(h.tasks.is_empty() && v.is_empty());
        assert_eq!(d.len(), 0);
    }
}

#[test]
fn normal_scope_exit_rejects_discard_with_original_scope_intact_and_accepts_lexical_move() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = setup(&mut h, &mut v, &mut d, V::Unit);
    let base = m.stack[0].base();
    let drops = Rc::new(RefCell::new(vec![]));
    let capture = h
        .resources
        .insert("probe", Probe(7, drops.clone()))
        .unwrap();
    v[base + 1] = Some(cold(&mut h, &m.stack[0], capture));
    m.stack[0].pc = 2;
    let tasks = h.tasks.clone();
    let slots = v.to_vec();
    assert!(
        m.step(&mut h, &mut v, &mut d)
            .err()
            .unwrap()
            .contains("discarded normally")
    );
    assert_eq!(h.tasks, tasks);
    assert_eq!(*v, slots);
    assert_eq!(m.stack[0].scopes.len(), 1);
    m.require_domain(&d).unwrap();
    v[base + 5] = v[base + 1].take();
    assert!(matches!(
        m.advance(&mut h, &mut v, &mut d, 0).unwrap(),
        Poll::Paused
    ));
    m.step(&mut h, &mut v, &mut d).unwrap();
    assert_eq!(h.tasks, tasks);
    assert!(m.stack[0].scopes.is_empty());
    assert!(drops.borrow().is_empty());
    let moved = v[base + 5].take().unwrap();
    let capture = drain(&mut h, &m.stack[0], moved);
    assert!(drops.borrow().is_empty());
    h.release_frame(&capture.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), [7]);
    assert!(matches!(
        m.advance(&mut h, &mut v, &mut d, 1).unwrap(),
        Poll::Complete(_)
    ));
    assert!(h.tasks.is_empty() && v.is_empty());
    assert_eq!(d.len(), 0);
}

static LOOP: LoopControl = LoopControl {
    target: 0,
    body: 1,
    after: 2,
    locals: &[0, 1],
    reset,
};
fn reset(a: &mut Activation, v: &mut Frames, mut args: Vec<V>) -> R<()> {
    v[a.base()] = args.pop();
    v[a.base() + 1] = None;
    Ok(())
}
fn looping(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    match a.pc {
        0 => Ok(Step::Loop {
            control: &LOOP,
            arguments: vec![],
        }),
        1 => Ok(Step::Complete(Completion::Again(0, vec![]))),
        3 => Ok(Step::Complete(Completion::Recover(
            0,
            vec![v[a.base()].take().unwrap()],
        ))),
        _ => unreachable!(),
    }
}
static LOOPING: Control = Control {
    name: "looping",
    slots: 2,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    access: None,
    resume: looping,
};
#[test]
fn loop_restart_requires_await_while_recovery_retires_only_discarded_captures() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = Machine::start(&mut h, &mut v, &mut d, &LOOPING, vec![]).unwrap();
    m.step(&mut h, &mut v, &mut d).unwrap();
    let drops = Rc::new(RefCell::new(vec![]));
    let base = m.stack[0].base();
    let capture = h
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    v[base] = Some(cold(&mut h, &m.stack[0], V::U32(11)));
    v[base + 1] = Some(cold(&mut h, &m.stack[0], capture));
    let tasks = h.tasks.clone();
    let slots = v.to_vec();
    assert!(
        m.step(&mut h, &mut v, &mut d)
            .err()
            .unwrap()
            .contains("discarded normally")
    );
    assert_eq!(h.tasks, tasks);
    assert_eq!(*v, slots);
    assert!(drops.borrow().is_empty());
    m.stack[0].pc = 3;
    m.step(&mut h, &mut v, &mut d).unwrap();
    assert_eq!(h.tasks.live_counts(), (1, 1));
    assert_eq!(*drops.borrow(), [1]);
    assert_eq!(m.stack[0].pc, 1);
    assert!(v[base + 1].is_none());
    let moved = v[base].take().unwrap();
    assert_eq!(drain(&mut h, &m.stack[0], moved), [V::U32(11)]);
    m.abort(&mut h, &mut v, &mut d, "test retirement").unwrap();
    assert!(h.tasks.is_empty() && v.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn handler_failure_retires_new_cold_obligations_without_reentering_contingency() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let drops = Rc::new(RefCell::new(vec![]));
    let owner = h
        .resources
        .insert("probe", Probe(0, drops.clone()))
        .unwrap();
    let mut m = setup(&mut h, &mut v, &mut d, owner);
    m.advance(&mut h, &mut v, &mut d, 1).unwrap();
    let base = m.stack[0].base();
    let capture = h
        .resources
        .insert("probe", Probe(2, drops.clone()))
        .unwrap();
    v[base + 3] = Some(cold(&mut h, &m.stack[0], capture));
    // Run another fault while the original lexical contingency is active.
    m.stack[0].pc = 1;
    assert!(m.advance(&mut h, &mut v, &mut d, 1).is_err());
    assert_eq!(h.events.len(), 1);
    assert!(h.tasks.is_empty() && v.is_empty());
    assert_eq!(d.len(), 0);
    assert!(h.resources.values.is_empty());
    let mut retired = drops.borrow().clone();
    retired.sort();
    assert_eq!(retired, [0, 2]);
}
