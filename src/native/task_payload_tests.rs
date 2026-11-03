use super::*;
use std::{cell::RefCell, rc::Rc};
fn leaf(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
static OWNER: Control = Control {
    name: "owner",
    slots: 2,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    access: None,
    resume: leaf,
};
static PRODUCER: Control = Control {
    name: "producer",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: leaf,
};
static IMPOSTOR: Control = Control {
    name: "producer",
    slots: 3,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: leaf,
};
static CONSTRUCTOR: Control = Control {
    name: "constructor",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: true,
    access: None,
    resume: leaf,
};
fn host() -> Host {
    Host::new(serde_json::json!({}), None, true, Some(100))
}
fn start(h: &mut Host, v: &mut Frames, d: &mut Domains) -> Machine {
    Machine::start(h, v, d, &OWNER, vec![]).unwrap()
}
fn reject(a: &Activation, h: &mut Host, p: &'static Control, v: &mut Option<V>) {
    let before = h.tasks.clone();
    let value = v.clone();
    assert!(a.queue_task(h, p, v).is_err());
    assert_eq!(h.tasks, before);
    assert_eq!(*v, value);
}
fn reject_started(a: &Activation, h: &mut Host, p: &'static Control, v: &mut Option<V>) {
    let before = h.tasks.clone();
    let value = v.clone();
    assert!(a.queue_started(h, p, v).is_err());
    assert_eq!(h.tasks, before);
    assert_eq!(*v, value);
}
#[test]
fn bound_captures_reject_foreign_hosts_owners_producers_and_unbound_awaits() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = start(&mut h, &mut v, &mut d);
    let mut n = start(&mut h, &mut v, &mut d);
    let mut other = host();
    let mut ov = Frames::default();
    let mut od = Domains::default();
    let mut om = start(&mut other, &mut ov, &mut od);
    let mut args = vec![V::String("owned".into()), V::U32(4)];
    let before = other.tasks.clone();
    assert!(
        m.stack[0]
            .defer_task(&mut other, &PRODUCER, "pending", &mut args)
            .is_err()
    );
    assert_eq!(other.tasks, before);
    assert_eq!(args.len(), 2);
    let mut payload = Some(
        m.stack[0]
            .defer_task(&mut h, &PRODUCER, "pending", &mut args)
            .unwrap(),
    );
    assert!(args.is_empty());
    assert!(
        super::super::super::await_arguments(vec![payload.clone().unwrap()], "producer").is_err()
    );
    reject(&n.stack[0], &mut h, &PRODUCER, &mut payload);
    reject_started(&n.stack[0], &mut h, &PRODUCER, &mut payload);
    reject(&om.stack[0], &mut other, &PRODUCER, &mut payload);
    reject_started(&om.stack[0], &mut other, &PRODUCER, &mut payload);
    reject(&m.stack[0], &mut h, &IMPOSTOR, &mut payload);
    reject_started(&m.stack[0], &mut h, &IMPOSTOR, &mut payload);
    let valid = payload.clone();
    if let Some(V::Task { operation, .. }) = &mut payload {
        *operation = "different".into();
    }
    reject(&m.stack[0], &mut h, &PRODUCER, &mut payload);
    reject_started(&m.stack[0], &mut h, &PRODUCER, &mut payload);
    payload = valid.clone();
    if let Some(V::Task { arguments, .. }) = &mut payload {
        arguments.pop();
    }
    reject(&m.stack[0], &mut h, &PRODUCER, &mut payload);
    reject_started(&m.stack[0], &mut h, &PRODUCER, &mut payload);
    payload = valid.clone();
    if let Some(V::Task { binding, .. }) = &mut payload {
        *binding = None;
    }
    reject(&m.stack[0], &mut h, &PRODUCER, &mut payload);
    reject_started(&m.stack[0], &mut h, &PRODUCER, &mut payload);
    let mut queued = m.stack[0]
        .queue_task(&mut h, &PRODUCER, &mut valid.clone())
        .unwrap();
    let mut stale = valid;
    reject(&m.stack[0], &mut h, &PRODUCER, &mut stale);
    reject_started(&m.stack[0], &mut h, &PRODUCER, &mut stale);
    let state = other.tasks.clone();
    assert!(queued.dispatch(&mut other).is_err());
    assert_eq!(other.tasks, state);
    m.abort(&mut h, &mut v, &mut d, "stop").unwrap();
    let state = h.tasks.clone();
    assert!(queued.dispatch(&mut h).is_err());
    assert_eq!(h.tasks, state);
    n.abort(&mut h, &mut v, &mut d, "stop").unwrap();
    om.abort(&mut other, &mut ov, &mut od, "stop").unwrap();
    assert!(h.tasks.is_empty() && other.tasks.is_empty());
}
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
#[test]
fn queued_payloads_move_buffers_and_owned_captures_once_in_fifo_order() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = start(&mut h, &mut v, &mut d);
    let drops = Rc::new(RefCell::new(vec![]));
    let supplier = make(&mut h, 0, &drops);
    let mut previous = vec![];
    for round in 0..16 {
        let text = "capture".repeat(1024);
        let ptr = text.as_ptr();
        let own = make(&mut h, round + 1, &drops);
        let mut args = vec![
            V::Construct {
                ty: "bundle".into(),
                name: "bundle".into(),
                fields: vec![V::String(text), own],
            },
            V::Reference {
                evidence: Default::default(),
                ty: "&probe".into(),
                root: usize::MAX,
                fields: vec![],
            },
        ];
        let a = m.stack[0]
            .defer_task(&mut h, &PRODUCER, "pending", &mut args)
            .unwrap();
        let mut bargs = vec![V::U32(round), V::Unit];
        let b = m.stack[0]
            .defer_task(&mut h, &PRODUCER, "pending", &mut bargs)
            .unwrap();
        let mut qa = m.stack[0]
            .queue_task(&mut h, &PRODUCER, &mut Some(a))
            .unwrap();
        let mut qb = m.stack[0]
            .queue_task(&mut h, &PRODUCER, &mut Some(b))
            .unwrap();
        assert!(!previous.contains(&qa.binding.task));
        previous.push(qa.binding.task);
        let before = h.tasks.clone();
        assert!(qb.dispatch(&mut h).is_err());
        assert_eq!(h.tasks, before);
        assert!(qb.arguments.is_some());
        let (producer, mut captured) = qa.dispatch(&mut h).unwrap();
        assert!(std::ptr::eq(producer, &PRODUCER));
        let V::Construct { fields, .. } = &captured[0] else {
            panic!()
        };
        let V::String(text) = &fields[0] else {
            panic!()
        };
        assert_eq!(ptr, text.as_ptr());
        assert!(matches!(
            captured[1],
            V::Reference {
                root: usize::MAX,
                ..
            }
        ));
        let before = h.tasks.clone();
        assert!(qa.dispatch(&mut h).is_err());
        assert_eq!(h.tasks, before);
        assert!(
            h.tasks
                .deliver(qa.binding.task, qa.binding.owner, qa.wait.unwrap())
                .is_err()
        );
        assert_eq!(h.tasks, before);
        h.tasks.complete(qa.binding.task, qa.binding.owner).unwrap();
        h.tasks
            .deliver(qa.binding.task, qa.binding.owner, qa.wait.unwrap())
            .unwrap();
        h.tasks.consume(qa.binding.task, qa.binding.owner).unwrap();
        h.release_frame(&captured.drain(..).map(Some).collect::<Vec<_>>());
        assert_eq!(*drops.borrow(), (1..=round + 1).collect::<Vec<_>>());
        qb.dispatch(&mut h).unwrap();
        h.tasks.complete(qb.binding.task, qb.binding.owner).unwrap();
        h.tasks
            .deliver(qb.binding.task, qb.binding.owner, qb.wait.unwrap())
            .unwrap();
        h.tasks.consume(qb.binding.task, qb.binding.owner).unwrap();
    }
    assert!(h.resources.get::<Probe>(&supplier, "probe").is_ok());
    assert!(matches!(
        m.advance(&mut h, &mut v, &mut d, 1).unwrap(),
        Poll::Complete(Completion::Outcome(_))
    ));
    assert!(h.tasks.is_empty() && v.is_empty());
    assert_eq!(h.fuel, Some(99));
    h.release_frame(&[Some(supplier)]);
    assert_eq!(drops.borrow().last(), Some(&0));
}
#[test]
fn cold_and_wait_capacity_failures_preserve_captures_and_protocol() {
    for wait_exhausted in [false, true] {
        let mut h = host();
        let mut v = Frames::default();
        let mut d = Domains::default();
        let mut m = start(&mut h, &mut v, &mut d);
        let drops = Rc::new(RefCell::new(vec![]));
        let own = make(&mut h, 1, &drops);
        let mut args = vec![own, V::U32(42)];
        let original = args.clone();
        let state = h.tasks.clone();
        assert!(
            m.stack[0]
                .defer_task(&mut h, &CONSTRUCTOR, "pending", &mut args)
                .is_err()
        );
        assert_eq!(args, original);
        assert_eq!(h.tasks, state);
        args.pop();
        assert!(
            m.stack[0]
                .defer_task(&mut h, &PRODUCER, "pending", &mut args)
                .is_err()
        );
        assert_eq!(h.tasks, state);
        args.push(V::U32(42));
        if wait_exhausted {
            let mut payload = Some(
                m.stack[0]
                    .defer_task(&mut h, &PRODUCER, "pending", &mut args)
                    .unwrap(),
            );
            h.tasks.exhaust_wait_space();
            reject(&m.stack[0], &mut h, &PRODUCER, &mut payload);
            h.release_frame(&[payload]);
        } else {
            h.tasks.exhaust_task_space();
            let before = h.tasks.clone();
            assert!(
                m.stack[0]
                    .defer_task(&mut h, &PRODUCER, "pending", &mut args)
                    .is_err()
            );
            assert_eq!(h.tasks, before);
            assert_eq!(args, original);
            assert!(drops.borrow().is_empty());
            h.release_frame(&args.drain(..).map(Some).collect::<Vec<_>>());
        }
        m.abort(&mut h, &mut v, &mut d, "stop").unwrap();
        assert_eq!(*drops.borrow(), [1]);
        assert!(h.tasks.is_empty());
        assert_eq!(h.fuel, Some(100));
    }
}
#[test]
fn unawaited_bound_payloads_release_owned_leaves_on_emergency_return() {
    let mut h = host();
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = start(&mut h, &mut v, &mut d);
    let drops = Rc::new(RefCell::new(vec![]));
    let supplier = make(&mut h, 0, &drops);
    let mut args = vec![
        make(&mut h, 1, &drops),
        V::Reference {
            evidence: Default::default(),
            ty: "&probe".into(),
            root: usize::MAX,
            fields: vec![],
        },
    ];
    let payload = m.stack[0]
        .defer_task(&mut h, &PRODUCER, "pending", &mut args)
        .unwrap();
    let mut stale = Some(payload.clone());
    v[m.stack[0].base()] = Some(payload);
    assert!(
        m.advance(&mut h, &mut v, &mut d, 1)
            .err()
            .unwrap()
            .contains("outstanding task obligations")
    );
    assert_eq!(*drops.borrow(), [1]);
    assert!(h.tasks.is_empty() && v.is_empty());
    let mut next = start(&mut h, &mut v, &mut d);
    reject(&next.stack[0], &mut h, &PRODUCER, &mut stale);
    assert!(h.resources.get::<Probe>(&supplier, "probe").is_ok());
    next.abort(&mut h, &mut v, &mut d, "stop").unwrap();
    h.release_frame(&[Some(supplier)]);
    assert_eq!(*drops.borrow(), [1, 0]);
}

#[test]
fn cold_payload_moves_into_a_lexical_boundary_without_reparenting() {
    let mut h = host();
    let mut values = Frames::default();
    let mut domains = Domains::default();
    let mut m = start(&mut h, &mut values, &mut domains);
    let owner = m.stack[0].owner;
    let mut arguments = vec![V::String("outer capture".into()), V::Unit];
    let mut payload = Some(
        m.stack[0]
            .defer_task(&mut h, &PRODUCER, "pending", &mut arguments)
            .unwrap(),
    );
    let mut boundary = h
        .begin_boundary("inner", std::iter::empty::<&V>(), true)
        .unwrap();
    let mut queued: super::super::QueuedTask = m.stack[0]
        .queue_task(&mut h, &PRODUCER, &mut payload)
        .unwrap();
    assert_eq!(queued.binding.owner, owner);
    assert!(payload.is_none());
    let (_, captured) = queued.dispatch(&mut h).unwrap();
    assert_eq!(captured, [V::String("outer capture".into()), V::Unit]);
    h.tasks.complete(queued.binding.task, owner).unwrap();
    h.tasks
        .deliver(queued.binding.task, owner, queued.wait.unwrap())
        .unwrap();
    h.tasks.consume(queued.binding.task, owner).unwrap();
    h.finish_boundary(&mut boundary, &Ok(())).unwrap();
    assert!(matches!(
        m.advance(&mut h, &mut values, &mut domains, 1).unwrap(),
        Poll::Complete(Completion::Outcome(_))
    ));
    assert!(h.tasks.is_empty() && values.is_empty());
    assert_eq!(h.fuel, Some(99));
}

#[test]
fn direct_await_queue_preserves_arguments_and_resources_on_rejection() {
    for invalid in 0..6 {
        let mut h = host();
        let mut v = Frames::default();
        let mut d = Domains::default();
        let mut m = start(&mut h, &mut v, &mut d);
        let drops = Rc::new(RefCell::new(vec![]));
        let mut args = vec![make(&mut h, 1, &drops), V::Unit];
        match invalid {
            0 => h.tasks.exhaust_task_space(),
            1 => h.tasks.exhaust_wait_space(),
            2 => {
                args.pop();
            }
            3 => h.tasks.retire_owner(m.stack[0].owner, true).unwrap(),
            _ => {}
        }
        let before = args.clone();
        let registry = h.tasks.clone();
        let producer = if invalid == 4 {
            &CONSTRUCTOR
        } else {
            &PRODUCER
        };
        if invalid == 5 {
            let mut other = host();
            let foreign = other.tasks.clone();
            assert!(
                m.stack[0]
                    .queue_invocation(&mut other, producer, &mut args)
                    .is_err()
            );
            assert_eq!(other.tasks, foreign);
        } else {
            assert!(
                m.stack[0]
                    .queue_invocation(&mut h, producer, &mut args)
                    .is_err()
            );
        }
        assert_eq!(args, before);
        assert_eq!(h.tasks, registry);
        assert_eq!(h.fuel, Some(100));
        assert!(drops.borrow().is_empty());
        h.release_frame(&args.drain(..).map(Some).collect::<Vec<_>>());
        // The stale-owner case deliberately retired its registry entry already.
        if invalid != 3 {
            m.abort(&mut h, &mut v, &mut d, "stop").unwrap();
        }
        assert_eq!(*drops.borrow(), [1]);
    }
}
