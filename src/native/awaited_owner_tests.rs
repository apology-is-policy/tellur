//! Independent descendants of an awaited invocation keep its original ownership.
use super::*;
fn awaited(
    c: &mut Contexts<'_>,
    parent: &ContextKey,
    control: &'static Control,
    args: Vec<V>,
) -> ContextKey {
    let mut queued = queue(c, parent, control, args);
    c.start_await(parent, &mut queued).unwrap()
}
fn unjoined(
    c: &mut Contexts<'_>,
    parent: &ContextKey,
    control: &'static Control,
    mut args: Vec<V>,
) -> StartedTask {
    let a = &c.parked[&parent.id].machine.stack[0];
    let mut cold = Some(a.defer_task(c.host, control, "pending", &mut args).unwrap());
    let mut queued = Some(a.queue_started(c.host, control, &mut cold).unwrap());
    c.retain_task(parent, &mut queued).unwrap()
}
fn borrowed(c: &Contexts<'_>, parent: &ContextKey) -> V {
    V::Reference {
        evidence: Default::default(),
        ty: "&probe".into(),
        root: c.parked[&parent.id].machine.stack[0].base(),
        fields: vec![],
    }
}
#[test]
fn awaited_invocations_own_raw_queued_and_unjoined_children_until_every_result_is_delivered() {
    for route in 0..3 {
        for early in [false, true] {
            let mut h = host();
            let mut c = Contexts::new(&mut h);
            let drops = Rc::new(RefCell::new(vec![]));
            let own = make(&mut c, 0, &drops);
            let root = c.start(&OWNER, vec![own]).unwrap();
            let own = make(&mut c, 1, &drops);
            let carrier = V::Construct {
                ty: "carrier".into(),
                name: "carrier".into(),
                fields: vec![own, borrowed(&c, &root)],
            };
            let parent = awaited(&mut c, &root, &OWNER, vec![carrier]);
            assert!(c.parked[&parent.id].sequential);
            let chain = |tracker: &fault::Tracker| {
                format!("{tracker:?}")
                    .split(", identities:")
                    .next()
                    .unwrap()
                    .to_owned()
            };
            let tracker = chain(&c.parked[&parent.id].host.tracker);
            let own = make(&mut c, 2, &drops);
            let args = vec![own, borrowed(&c, &parent)];
            let mut handle = None;
            let child = match route {
                0 => c.start_child(&parent, &CHILD, args).unwrap(),
                1 => {
                    let mut queued = queue(&mut c, &parent, &CHILD, args);
                    c.start_task(&parent, &mut queued).unwrap()
                }
                _ => {
                    handle = Some(unjoined(&mut c, &parent, &CHILD, args));
                    handle.as_ref().unwrap().context().clone()
                }
            };
            assert_eq!(chain(&c.parked[&parent.id].host.tracker), tracker);
            assert!(c.advance_retained(&parent, 99).is_err());
            assert!(c.advance_retained(&root, 99).is_err());
            let mut join = if route == 2 && early {
                Some(c.claim_task(&parent, &CHILD, &mut handle).unwrap())
            } else {
                None
            };
            c.advance_retained(&child, 1).unwrap();
            let wake = c.park_for_readiness(&child).unwrap();
            c.signal_readiness(&wake).unwrap();
            c.advance_retained(&child, 1).unwrap();
            assert!(drops.borrow().is_empty());
            assert!(c.advance_retained(&parent, 99).is_err());
            let before = snapshot(&c);
            assert!(c.join(Some(&root), &child).is_err());
            if route == 2 {
                assert!(c.claim_task(&root, &CHILD, &mut handle).is_err());
            }
            assert_eq!(snapshot(&c), before);
            let payload = if route == 2 {
                if !early {
                    join = Some(c.claim_task(&parent, &CHILD, &mut handle).unwrap());
                }
                c.join_task(&parent, &mut join)
                    .unwrap()
                    .outcome()
                    .unwrap()
                    .1
            } else {
                fields(&mut c, Some(&parent), &child)
            };
            release(&mut c, payload);
            assert_eq!(*drops.borrow(), [2]);
            c.advance_retained(&parent, 2).unwrap();
            let result = fields(&mut c, Some(&root), &parent);
            release(&mut c, result);
            assert_eq!(*drops.borrow(), [2, 1]);
            c.advance_retained(&root, 2).unwrap();
            let result = fields(&mut c, None, &root);
            release(&mut c, result);
            assert_eq!(*drops.borrow(), [2, 1, 0]);
            assert_eq!(c.host.fuel, Some(994));
            empty(&c);
        }
    }
}
#[test]
fn awaited_owner_fault_settles_completed_waiting_nested_and_ready_descendants_before_recovery() {
    for mode in 0..4 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let own = make(&mut c, 0, &drops);
        let root = c.start(&OWNER, vec![own]).unwrap();
        let own = make(&mut c, 1, &drops);
        let parent = awaited(
            &mut c,
            &root,
            if mode == 0 { &OWNER } else { &SUPERVISED },
            vec![own],
        );
        if mode != 0 {
            c.advance_retained(&parent, 1).unwrap();
        }
        let failed = unjoined(&mut c, &parent, &FAULT, vec![]);
        let fault = failed.context().clone();
        c.dispatch_task(&fault).unwrap();
        let mut children = vec![];
        let mut wake = None;
        for phase in 0..4 {
            let own = make(&mut c, phase + 2, &drops);
            let (control, args) = if phase == 2 {
                (&SUPERVISED, vec![own])
            } else {
                (&CHILD, vec![own, borrowed(&c, &parent)])
            };
            let handle = unjoined(&mut c, &parent, control, args);
            let child = handle.context().clone();
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
        if mode == 3 {
            let process = c.start(&PROCESS, vec![]).unwrap();
            assert!(matches!(
                c.advance_retained(&process, 1).unwrap(),
                ContextPoll::Complete(Completion::Process(23))
            ));
        } else {
            if mode == 2 {
                c.host.fuel = Some(0);
            }
            assert!(matches!(
                c.advance_retained(&fault, 1).unwrap(),
                ContextPoll::Failed(_)
            ));
            assert!(!c.host.events.iter().any(|e| e.get("handled").is_some()));
            if mode == 1 {
                // Only this owning contingency recovers; the root still awaits
                // this exact invocation and all stopped nested handlers stay silent.
                assert_eq!(c.parked[&root.id].awaiting, Some(parent.id));
                assert_eq!(drops.borrow().last(), Some(&1));
                c.advance_retained(&parent, 1).unwrap();
                assert!(fields(&mut c, Some(&root), &parent).is_empty());
                assert_eq!(
                    c.host
                        .events
                        .iter()
                        .filter(|e| e.get("handled").is_some())
                        .count(),
                    1
                );
                c.advance_retained(&root, 2).unwrap();
                let result = fields(&mut c, None, &root);
                release(&mut c, result);
            } else {
                let ContextPoll::Failed(report) = c.advance_retained(&root, 1).unwrap() else {
                    panic!()
                };
                assert_eq!(
                    report
                        .activations
                        .iter()
                        .map(|a| a.control.as_str())
                        .collect::<Vec<_>>(),
                    if mode == 2 {
                        vec!["owner", "supervised"]
                    } else {
                        // Both the root and its awaited spawning invocation are
                        // causal frames, despite sharing the same control name.
                        vec!["owner", "owner"]
                    }
                );
                assert_ne!(report.activations[0].id, report.activations[1].id);
                assert_eq!(report.activations[1].parent, Some(report.activations[0].id));
                let mut cause = report.cause.unwrap();
                if mode == 2 {
                    assert_eq!(cause.boundary, "owning");
                    assert_eq!(
                        cause
                            .activations
                            .iter()
                            .map(|a| a.control.as_str())
                            .collect::<Vec<_>>(),
                        ["owning"]
                    );
                    cause = cause
                        .cause
                        .expect("owning boundary preserves original child fault");
                }
                assert_eq!(cause.boundary, "fault");
                assert_eq!(
                    cause
                        .activations
                        .iter()
                        .map(|a| a.control.as_str())
                        .collect::<Vec<_>>(),
                    ["fault"]
                );
                assert!(!c.host.events.iter().any(|e| e.get("handled").is_some()));
            }
            c.host.fuel = Some(10);
            c.advance_retained(&independent, 2).unwrap();
            assert_eq!(fields(&mut c, None, &independent), [V::U32(91)]);
        }
        let order = drops.borrow();
        assert_eq!(order.len(), 6);
        let mut child_drops = order[..4].to_vec();
        child_drops.sort();
        assert_eq!(child_drops, [2, 3, 4, 5]);
        assert_eq!(&order[4..], &[1, 0]);
        drop(order);
        assert!(c.signal_readiness(&wake.unwrap()).is_err());
        for mut handle in children.into_iter().chain([failed]).map(Some) {
            assert!(c.claim_task(&parent, &CHILD, &mut handle).is_err());
        }
        empty(&c);
    }
}
