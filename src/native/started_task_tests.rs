//! The production host API owns unjoined captures/results, not this test harness.
use super::super::super::{StartedTask, TaskJoin};
use super::*;

fn unjoined(
    c: &mut Contexts<'_>,
    parent: &ContextKey,
    producer: &'static Control,
    mut args: Vec<V>,
) -> Option<StartedTask> {
    let a = c.parked[&parent.id].machine.stack.last().unwrap();
    let mut cold = Some(
        a.defer_task(c.host, producer, "pending", &mut args)
            .unwrap(),
    );
    let mut queued = Some(a.queue_started(c.host, producer, &mut cold).unwrap());
    assert!(args.is_empty() && cold.is_none());
    let key = c.retain_task(parent, &mut queued).unwrap();
    assert!(queued.is_none());
    Some(key)
}
fn joined(c: &mut Contexts<'_>, parent: &ContextKey, join: &mut Option<TaskJoin>) -> Vec<V> {
    let Completion::Outcome((_, fields)) = c.join_task(parent, join).unwrap() else {
        panic!()
    };
    assert!(join.is_none());
    fields
}
fn reject_claim(
    c: &mut Contexts<'_>,
    parent: &ContextKey,
    producer: &'static Control,
    handle: &mut Option<StartedTask>,
) {
    let before = snapshot(c);
    let held = format!("{handle:?}");
    assert!(c.claim_task(parent, producer, handle).is_err());
    assert_eq!(snapshot(c), before);
    assert_eq!(format!("{handle:?}"), held);
}
#[test]
fn unjoined_children_claim_once_in_every_phase_without_running_or_rescheduling() {
    for phase in 0..4 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let own = make(&mut c, 0, &drops);
        let parent = c.start(&OWNER, vec![own]).unwrap();
        let base = c.parked[&parent.id].machine.stack[0].base();
        let own = make(&mut c, 1, &drops);
        let mut handle = unjoined(
            &mut c,
            &parent,
            &CHILD,
            vec![
                own,
                V::Reference {
                    evidence: Default::default(),
                    ty: "&probe".into(),
                    root: base,
                    fields: vec![],
                },
            ],
        );
        let child = handle.as_ref().unwrap().context().clone();
        let mut later = unjoined(&mut c, &parent, &OWNER, vec![V::U32(91)]);
        let second = later.as_ref().unwrap().context().clone();
        let queued = snapshot(&c);
        assert_eq!(c.host.fuel, Some(1000));
        assert!(c.host.events.is_empty());
        assert!(c.advance_retained(&second, 1).is_err());
        assert!(c.advance_retained(&parent, 100).is_err());
        assert!(c.park_for_readiness(&child).is_err());
        assert!(matches!(
            c.advance_retained(&child, 0).unwrap(),
            ContextPoll::Paused
        ));
        assert_eq!(snapshot(&c), queued);
        if phase > 0 {
            c.advance_retained(&child, 1).unwrap();
        }
        let wake = if phase == 2 {
            Some(c.park_for_readiness(&child).unwrap())
        } else {
            None
        };
        if phase == 3 {
            c.advance_retained(&child, 1).unwrap();
        }
        let fuel = c.host.fuel;
        let ready = c.host.tasks.next_ready();
        let before = snapshot(&c);
        assert!(c.join(Some(&parent), &child).is_err());
        assert_eq!(snapshot(&c), before);
        let mut join = Some(c.claim_task(&parent, &CHILD, &mut handle).unwrap());
        assert!(handle.is_none());
        reject_claim(&mut c, &parent, &CHILD, &mut handle);
        assert_eq!(c.host.tasks.next_ready(), ready);
        assert_eq!(c.host.fuel, fuel);
        assert!(drops.borrow().is_empty());
        if phase != 3 {
            let before = snapshot(&c);
            assert!(c.join_task(&parent, &mut join).is_err());
            assert_eq!(snapshot(&c), before);
            assert!(join.is_some());
        }
        if let Some(wake) = &wake {
            c.signal_readiness(wake).unwrap();
            // The other queued task is now ahead; joining never rearranges it.
            assert!(c.advance_retained(&child, 1).is_err());
        }
        if phase != 0 {
            c.advance_retained(&second, 2).unwrap();
        }
        if phase != 3 {
            c.advance_retained(&child, 2).unwrap();
        }
        let before = snapshot(&c);
        assert!(c.join(Some(&parent), &child).is_err());
        assert!(c.advance_retained(&parent, 2).is_err());
        assert_eq!(snapshot(&c), before);
        let result = joined(&mut c, &parent, &mut join);
        assert!(matches!(result[1], V::Reference{root,..} if root==base));
        assert!(c.join_task(&parent, &mut join).is_err());
        release(&mut c, result);
        assert_eq!(*drops.borrow(), [1]);
        if phase == 0 {
            c.advance_retained(&second, 2).unwrap();
        }
        let mut other = Some(c.claim_task(&parent, &OWNER, &mut later).unwrap());
        assert_eq!(joined(&mut c, &parent, &mut other), [V::U32(91)]);
        c.advance_retained(&parent, 2).unwrap();
        let result = fields(&mut c, None, &parent);
        release(&mut c, result);
        assert_eq!(*drops.borrow(), [1, 0]);
        assert_eq!(c.host.fuel, Some(994));
        empty(&c);
    }
}
static COLLISION: Control = Control {
    name: "child",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: returning,
};
#[test]
fn started_join_rejects_foreign_stale_crossed_and_exhausted_authority_atomically() {
    for phase in 0..4 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let parent = c.start(&OWNER, vec![V::Unit]).unwrap();
        let unrelated = c.start(&OWNER, vec![V::Unit]).unwrap();
        let mut handle = unjoined(&mut c, &parent, &CHILD, vec![V::U32(1), V::Unit]);
        let child = handle.as_ref().unwrap().context().clone();
        if phase > 0 {
            c.advance_retained(&child, 1).unwrap();
        }
        if phase == 2 {
            c.park_for_readiness(&child).unwrap();
        }
        if phase == 3 {
            c.advance_retained(&child, 1).unwrap();
        }
        reject_claim(&mut c, &unrelated, &CHILD, &mut handle);
        reject_claim(&mut c, &parent, &COLLISION, &mut handle);
        let foreign = ContextKey {
            session: Arc::new(()),
            id: parent.id,
        };
        reject_claim(&mut c, &foreign, &CHILD, &mut handle);
        let original = c.parked[&parent.id].machine.current.clone();
        let crossed = c.parked[&unrelated.id].machine.current.clone();
        c.parked.get_mut(&parent.id).unwrap().machine.current = crossed;
        reject_claim(&mut c, &parent, &CHILD, &mut handle);
        c.parked.get_mut(&parent.id).unwrap().machine.current = original;
        let owner = c.parked[&parent.id].machine.stack[0].owner;
        let other = c.parked[&unrelated.id].machine.stack[0].owner;
        c.parked.get_mut(&parent.id).unwrap().machine.stack[0].owner = other;
        reject_claim(&mut c, &parent, &CHILD, &mut handle);
        c.parked.get_mut(&parent.id).unwrap().machine.stack[0].owner = owner;
        let mut foreign_host = host();
        let mut foreign_contexts = Contexts::new(&mut foreign_host);
        let foreign_parent = foreign_contexts.start(&OWNER, vec![V::Unit]).unwrap();
        reject_claim(&mut foreign_contexts, &foreign_parent, &CHILD, &mut handle);
        foreign_contexts.terminate().unwrap();
        c.host.tasks.exhaust_wait_space();
        reject_claim(&mut c, &parent, &CHILD, &mut handle);
        c.terminate().unwrap();
        reject_claim(&mut c, &parent, &CHILD, &mut handle);
        empty(&c);
    }
}
#[test]
fn unjoined_queue_retention_checks_capacity_before_capture_transfer_and_ignores_wait_capacity() {
    for mode in 0..9 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let parent = c.start(&OWNER, vec![V::Unit]).unwrap();
        let own = make(&mut c, 1, &drops);
        let producer = if mode == 0 { &BAD } else { &CHILD };
        let other = if mode == 1 { own.clone() } else { V::Unit };
        let a = &c.parked[&parent.id].machine.stack[0];
        let mut args = vec![own, other];
        let mut cold = Some(
            a.defer_task(c.host, producer, "pending", &mut args)
                .unwrap(),
        );
        c.host.tasks.exhaust_wait_space();
        let mut queued = Some(a.queue_started(c.host, producer, &mut cold).unwrap());
        assert!(cold.is_none());
        match mode {
            2 => c.next = u64::MAX,
            3 => c.values.exhaust_identity_space(),
            4 => c.host.tasks.exhaust_owner_space(),
            5 => c.host.resources.next_scope = u64::MAX,
            6 => c
                .domains
                .stop(&c.parked[&parent.id].machine.current.clone())
                .unwrap(),
            7 => {
                c.park_for_readiness(&parent).err();
                c.advance_retained(&parent, 1).unwrap();
                c.park_for_readiness(&parent).unwrap();
            }
            _ => (),
        }
        if mode == 8 {
            let mut task = Some(c.retain_task(&parent, &mut queued).unwrap());
            assert!(queued.is_none());
            reject_claim(&mut c, &parent, producer, &mut task);
        } else {
            let before = snapshot(&c);
            let held = format!("{queued:?}");
            assert!(c.retain_task(&parent, &mut queued).is_err());
            assert_eq!(snapshot(&c), before);
            assert_eq!(format!("{queued:?}"), held);
            queued.take().unwrap().retire(c.host).unwrap();
        }
        c.terminate().unwrap();
        assert_eq!(*drops.borrow(), [1]);
        empty(&c);
    }
}
#[test]
fn unjoined_ready_running_waiting_and_completed_children_settle_before_owning_contingency() {
    for process_stop in [false, true] {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let own = make(&mut c, 0, &drops);
        let parent = c.start(&SUPERVISED, vec![own]).unwrap();
        c.advance_retained(&parent, 1).unwrap();
        let base = c.parked[&parent.id].machine.stack[0].base();
        let mut children = vec![];
        let mut wake = None;
        // Dispatch fault first in FIFO order without running its callback yet.
        let mut failed = unjoined(&mut c, &parent, &FAULT, vec![]);
        let fault = failed.as_ref().unwrap().context().clone();
        c.dispatch_task(&fault).unwrap();
        assert!(c.dispatch_task(&fault).is_err());
        for phase in 0..4 {
            let own = make(&mut c, phase + 1, &drops);
            let (producer, args) = if phase == 2 {
                // This stopped sibling owns a nested contingency. It must only
                // release resources; its handler must never emit `handled`.
                (&SUPERVISED, vec![own])
            } else {
                (
                    &CHILD,
                    vec![
                        own,
                        V::Reference {
                            evidence: Default::default(),
                            ty: "&probe".into(),
                            root: base,
                            fields: vec![],
                        },
                    ],
                )
            };
            let handle = unjoined(&mut c, &parent, producer, args);
            let child = handle.as_ref().unwrap().context().clone();
            if phase != 3 {
                c.advance_retained(&child, 1).unwrap();
            }
            if phase == 0 {
                c.advance_retained(&child, 1).unwrap();
            }
            if phase == 1 {
                wake = Some(c.park_for_readiness(&child).unwrap());
            }
            children.push(handle);
        }
        let independent = c.start(&OWNER, vec![V::U32(91)]).unwrap();
        if process_stop {
            let stop = c.start(&PROCESS, vec![]).unwrap();
            assert!(matches!(
                c.advance_retained(&stop, 1).unwrap(),
                ContextPoll::Complete(Completion::Process(23))
            ));
        } else {
            assert!(matches!(
                c.advance_retained(&fault, 1).unwrap(),
                ContextPoll::Failed(_)
            ));
            let order = drops.borrow();
            assert_eq!(order.last(), Some(&0));
            let mut children = order[..4].to_vec();
            children.sort();
            assert_eq!(children, [1, 2, 3, 4]);
            drop(order);
            assert_eq!(c.host.events.len(), 1);
            assert!(c.host.events[0]["kind"] == "fault");
            c.advance_retained(&parent, 1).unwrap();
            assert!(fields(&mut c, None, &parent).is_empty());
            assert_eq!(c.host.events[1], serde_json::json!({"handled":true}));
            c.advance_retained(&independent, 2).unwrap();
            assert_eq!(fields(&mut c, None, &independent), [V::U32(91)]);
        }
        let order = drops.borrow();
        assert_eq!(order.len(), 5);
        assert_eq!(order.last(), Some(&0));
        let mut retired_children = order[..4].to_vec();
        retired_children.sort();
        assert_eq!(retired_children, [1, 2, 3, 4]);
        drop(order);
        assert!(c.signal_readiness(&wake.unwrap()).is_err());
        reject_claim(&mut c, &parent, &FAULT, &mut failed);
        for mut handle in children {
            reject_claim(&mut c, &parent, &CHILD, &mut handle);
        }
        empty(&c);
    }
}

#[test]
fn claimed_delivery_rechecks_parent_invocation_domain_and_session_before_adoption() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let parent = c.start(&OWNER, vec![V::Unit]).unwrap();
    let independent = c.start(&OWNER, vec![V::Unit]).unwrap();
    let own = make(&mut c, 1, &drops);
    let mut task = unjoined(&mut c, &parent, &CHILD, vec![own, V::Unit]);
    let child = task.as_ref().unwrap().context().clone();
    c.advance_retained(&child, 2).unwrap();
    let mut join = Some(c.claim_task(&parent, &CHILD, &mut task).unwrap());
    let reject = |c: &mut Contexts<'_>, parent: &ContextKey, join: &mut Option<TaskJoin>| {
        let before = snapshot(c);
        let handle = format!("{join:?}");
        assert!(c.join_task(parent, join).is_err());
        assert_eq!(snapshot(c), before);
        assert_eq!(format!("{join:?}"), handle);
    };
    reject(&mut c, &independent, &mut join);
    let foreign = ContextKey {
        session: Arc::new(()),
        id: parent.id,
    };
    reject(&mut c, &foreign, &mut join);
    let original = c.parked[&parent.id].machine.current.clone();
    c.parked.get_mut(&parent.id).unwrap().machine.current =
        c.parked[&independent.id].machine.current.clone();
    reject(&mut c, &parent, &mut join);
    c.parked.get_mut(&parent.id).unwrap().machine.current = original;
    let owner = c.parked[&parent.id].machine.stack[0].owner;
    c.parked.get_mut(&parent.id).unwrap().machine.stack[0].owner =
        c.parked[&independent.id].machine.stack[0].owner;
    reject(&mut c, &parent, &mut join);
    c.parked.get_mut(&parent.id).unwrap().machine.stack[0].owner = owner;
    assert!(drops.borrow().is_empty());
    let result = joined(&mut c, &parent, &mut join);
    release(&mut c, result);
    assert_eq!(*drops.borrow(), [1]);
    c.terminate().unwrap();
    empty(&c);
}
