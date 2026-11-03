//! Query evidence cannot run callbacks, transfer ownership or relax either gate.
use super::super::dependencies::Interference;
use super::super::reference_evidence::{Origin, attach_checked};
use super::*;

fn plan(a: &Activation) -> AccessPlan {
    let accesses = match a.pc {
        0 => &[Access {
            slot: Some(0),
            fields: &[0],
            indirect: false,
            kind: AccessKind::Read,
        }][..],
        1 => &[Access {
            slot: Some(0),
            fields: &[1],
            indirect: false,
            kind: AccessKind::Write,
        }][..],
        3 => &[Access {
            slot: Some(1),
            fields: &[],
            indirect: true,
            kind: AccessKind::Read,
        }][..],
        _ => &[Access {
            slot: Some(0),
            fields: &[],
            indirect: false,
            kind: AccessKind::Retire,
        }][..],
    };
    AccessPlan {
        transition: Transition::Local,
        accesses,
        delivery: &[],
        opaque: false,
    }
}
fn finish(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
fn parent(_: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 10 {
        a.pc = 1;
        Ok(Step::Call {
            control: &PARENT,
            arguments: vec![pair()],
            destination: None,
            deliver,
        })
    } else if a.pc == 11 {
        Ok(Step::Boundary(&BOUNDARY))
    } else {
        Ok(Step::Continue)
    }
}
fn deliver(_: &mut Activation, _: &mut Frames, _: Completion) -> R<()> {
    Ok(())
}
static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "query_boundary",
    body: 1,
    handler: 1,
    after: 1,
    retained: &[],
    retire: &[],
    work: &[],
    handling: &[],
    fault_slot: 1,
};
static PARENT: Control = Control {
    name: "query_parent",
    slots: 2,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: Some(plan),
    resume: parent,
};
static CHILD: Control = Control {
    name: "query_child",
    resume: finish,
    ..PARENT
};
fn pair() -> V {
    V::Construct {
        ty: "pair".into(),
        name: "pair".into(),
        fields: vec![V::U32(7), V::U32(9)],
    }
}
fn borrow(c: &Contexts<'_>, root: usize, broad: bool) -> V {
    let value =
        super::super::super::reference(&c.values, root, &[0], false, "&mutable U32").unwrap();
    attach_checked(
        &c.values,
        value,
        &[Origin {
            root,
            fields: if broad { &[] } else { &[0] },
            indirect: false,
            guard: false,
            opaque: false,
        }],
        true,
        true,
    )
}
fn setup(c: &mut Contexts<'_>, broad: bool) -> (ContextKey, ContextKey) {
    let root = c.start(&PARENT, vec![pair()]).unwrap();
    let base = c.parked[&root.id].machine.stack[0].base();
    let child = c
        .start_child(&root, &CHILD, vec![borrow(c, base, broad)])
        .unwrap();
    (root, child)
}
fn pc(c: &mut Contexts<'_>, root: &ContextKey, pc: usize) {
    c.parked
        .get_mut(&root.id)
        .unwrap()
        .machine
        .stack
        .last_mut()
        .unwrap()
        .pc = pc;
}
fn snapshot(c: &Contexts<'_>) -> String {
    let frames: Vec<_> = c
        .parked
        .values()
        .map(|p| {
            (
                p.machine
                    .stack
                    .iter()
                    .map(|a| (a.owner, a.base(), a.pc, &a.facts, a.pending.is_some()))
                    .collect::<Vec<_>>(),
                &p.host.tracker,
                &p.dependencies,
            )
        })
        .collect();
    let completed: Vec<_> = c
        .completed
        .values()
        .map(|p| {
            (
                &p.parked.dependencies,
                match &p.outcome {
                    Completion::Outcome((label, fields)) => format!("{label}:{fields:?}"),
                    _ => "administrative".into(),
                },
            )
        })
        .collect();
    format!(
        "{:?}",
        (
            &*c.values,
            &c.host.tasks,
            c.host.fuel,
            &c.host.events,
            &c.host.resources.scopes,
            frames,
            completed,
            &c.waits,
            c.domains.len(),
            c.domains.member_count()
        )
    )
}
fn query(c: &Contexts<'_>, root: &ContextKey, child: &ContextKey) -> Interference {
    let before = snapshot(c);
    let result = c.child_interference(root, child);
    for _ in 0..3 {
        assert_eq!(c.child_interference(root, child), result);
    }
    assert_eq!(snapshot(c), before);
    result
}
fn inconclusive(c: &Contexts<'_>, root: &ContextKey, child: &ContextKey) {
    assert!(matches!(
        query(c, root, child),
        Interference::Inconclusive(_)
    ));
}

#[test]
fn exact_fields_broad_ancestors_and_completed_results_do_not_authorize_selection() {
    for broad in [false, true] {
        let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
        let mut c = Contexts::new(&mut h);
        let (root, child) = setup(&mut c, broad);
        for completed in [false, true] {
            if completed {
                assert!(matches!(
                    c.advance_retained(&child, 1).unwrap(),
                    ContextPoll::Ready
                ));
            }
            for index in 0..3 {
                pc(&mut c, &root, index);
                let result = query(&c, &root, &child);
                if index == 1 && !broad {
                    assert_eq!(result, Interference::Disjoint);
                } else {
                    assert!(matches!(result, Interference::Overlap { .. }));
                }
                let before = snapshot(&c);
                for quota in [0, 1, 99] {
                    assert!(c.advance_retained(&root, quota).is_err());
                    let p = c.parked.get_mut(&root.id).unwrap();
                    c.host.exchange_context(&mut p.host).unwrap();
                    assert!(
                        p.machine
                            .advance(c.host, &mut c.values, &mut c.domains, quota)
                            .is_err()
                    );
                    c.host.exchange_context(&mut p.host).unwrap();
                    assert_eq!(snapshot(&c), before);
                }
            }
        }
        c.join(Some(&root), &child).unwrap();
        inconclusive(&c, &root, &child);
        c.terminate().unwrap();
    }
}

#[test]
fn partial_unknown_foreign_stopped_and_wrong_invocation_queries_are_inconclusive() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let mut c = Contexts::new(&mut h);
    let (root, child) = setup(&mut c, false);
    let original = c.parked[&child.id].dependencies.clone();
    for dependencies in [
        Dependencies::Unknown("probe".into()),
        Dependencies::Partial {
            reachable: vec![],
            origins: Default::default(),
        },
    ] {
        c.parked.get_mut(&child.id).unwrap().dependencies = dependencies;
        inconclusive(&c, &root, &child);
    }
    c.parked.get_mut(&child.id).unwrap().dependencies = original;
    let foreign = ContextKey {
        session: Arc::new(()),
        id: child.id,
    };
    inconclusive(&c, &root, &foreign);
    inconclusive(&c, &foreign, &child);
    let other = c.start(&PARENT, vec![pair()]).unwrap();
    inconclusive(&c, &other, &child);
    // Deliberately enter a new invocation below the gate: equal context/domain
    // identity is insufficient, including when the child has already completed.
    c.advance_retained(&child, 1).unwrap();
    pc(&mut c, &root, 10);
    let p = c.parked.get_mut(&root.id).unwrap();
    c.host.exchange_context(&mut p.host).unwrap();
    p.machine
        .step(c.host, &mut c.values, &mut c.domains)
        .unwrap();
    c.host.exchange_context(&mut p.host).unwrap();
    inconclusive(&c, &root, &child);
    assert!(matches!(
        c.borrower_interference(&root),
        BorrowerInterference::Inconclusive(_)
    ));
    let domain = c.parked[&other.id].machine.current.clone();
    c.domains.stop(&domain).unwrap();
    inconclusive(&c, &other, &child);
    c.terminate().unwrap();
}

#[test]
fn callback_indirection_requires_matching_formation_identity_after_allocation_reuse() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let mut c = Contexts::new(&mut h);
    let (root, child) = setup(&mut c, false);
    let base = c.parked[&root.id].machine.stack[0].base();
    let frame = c.values.enter(1).unwrap();
    c.values[frame.base()] = Some(V::U32(17));
    let reference =
        super::super::super::reference(&c.values, frame.base(), &[], false, "&U32").unwrap();
    c.values[base + 1] = Some(reference);
    pc(&mut c, &root, 3);
    assert_eq!(query(&c, &root, &child), Interference::Disjoint);
    c.values.retire(&frame).unwrap();
    let replacement = c.values.enter(1).unwrap();
    assert_eq!(replacement.base(), frame.base());
    c.values[replacement.base()] = Some(V::U32(19));
    inconclusive(&c, &root, &child);
    c.values[base + 1] = None;
    c.values.retire(&replacement).unwrap();
    c.terminate().unwrap();
}

#[test]
fn pending_normal_delivery_compares_only_result_writes_not_consumed_arguments() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let mut c = Contexts::new(&mut h);
    let (root, child) = setup(&mut c, false);
    let domain = c.parked[&root.id].machine.current.clone();
    let a = c
        .parked
        .get_mut(&root.id)
        .unwrap()
        .machine
        .stack
        .last_mut()
        .unwrap();
    a.pending = Some(Pending::Await(AwaitContinuation {
        owner: a.owner,
        domain,
        queued: None,
        child: None,
        result: Some(Ok(Completion::Outcome(("done".into(), vec![])))),
        deliver,
        access: AccessPlan {
            transition: Transition::AwaitTask,
            accesses: &[Access {
                slot: Some(0),
                fields: &[],
                indirect: false,
                kind: AccessKind::Move,
            }],
            delivery: &[Access {
                slot: Some(1),
                fields: &[],
                indirect: false,
                kind: AccessKind::Write,
            }],
            opaque: true,
        },
        dependencies: Dependencies::describe(&c.values, &[]),
    }));
    assert_eq!(query(&c, &root, &child), Interference::Disjoint);
    let p = c.await_continuation(&root).unwrap();
    p.result = Some(Ok(Completion::Constructed));
    inconclusive(&c, &root, &child);
    c.await_continuation(&root).unwrap().result = None;
    inconclusive(&c, &root, &child);
    c.terminate().unwrap();
}

#[test]
fn complete_queries_validate_all_identities_and_preserve_conservative_access_modes() {
    use super::super::access_resolution::{ResolvedAccess, ResolvedPlan, StoragePlace};
    use super::super::dependencies::Dependency;
    use super::super::reference_evidence::{BoundOrigin, Origins};
    let mut values = Frames::default();
    let frame = values.enter(2).unwrap();
    values[frame.base()] = Some(pair());
    values[frame.base() + 1] = Some(pair());
    let place = StoragePlace {
        storage: values.local_storage(&frame, 0).unwrap(),
        fields: vec![0],
    };
    let mut dependencies = Dependencies::Complete {
        reachable: vec![Dependency {
            place: place.clone(),
            mutable: false,
        }],
        origins: Origins::Complete(vec![BoundOrigin {
            place: place.clone(),
            guard: false,
            opaque: false,
        }]),
    };
    let mut plan = ResolvedPlan {
        transition: Transition::Local,
        accesses: vec![ResolvedAccess {
            place: place.clone(),
            kind: AccessKind::Read,
        }],
        delivery: vec![],
        opaque: false,
    };
    let before = format!("{:?}", &*values);
    for kind in [
        AccessKind::Read,
        AccessKind::Shared,
        AccessKind::Exclusive,
        AccessKind::Move,
        AccessKind::Write,
        AccessKind::Retire,
    ] {
        plan.accesses[0].kind = kind;
        assert!(matches!(
            dependencies.interference(&values, &plan),
            Interference::Overlap { .. }
        ));
    }
    plan.accesses[0].place.storage = values.local_storage(&frame, 1).unwrap();
    assert_eq!(
        dependencies.interference(&values, &plan),
        Interference::Disjoint
    );
    for transition in [
        Transition::Call,
        Transition::Run,
        Transition::Await,
        Transition::AwaitTask,
        Transition::Boundary,
        Transition::BoundaryEnd,
        Transition::Loop,
        Transition::Restart,
        Transition::Recover,
        Transition::Return,
        Transition::Complete,
        Transition::Fault,
        Transition::Process,
        Transition::Unknown,
        Transition::Effect,
        Transition::Defer,
    ] {
        plan.transition = transition;
        assert!(matches!(
            dependencies.interference(&values, &plan),
            Interference::Inconclusive(_)
        ));
    }
    plan.transition = Transition::Local;
    plan.opaque = true;
    assert!(matches!(
        dependencies.interference(&values, &plan),
        Interference::Inconclusive(_)
    ));
    plan.opaque = false;
    plan.delivery = plan.accesses.clone();
    assert!(matches!(
        dependencies.interference(&values, &plan),
        Interference::Inconclusive(_)
    ));
    plan.delivery.clear();
    plan.accesses[0].place = place.clone();
    // A valid overlapping prefix must not conceal stale/foreign later origins.
    let stale = values.enter(1).unwrap();
    let storage = values.local_storage(&stale, 0).unwrap();
    values.retire(&stale).unwrap();
    let mut foreign = Frames::default();
    let frame_other = foreign.enter(1).unwrap();
    for storage in [storage, foreign.local_storage(&frame_other, 0).unwrap()] {
        if let Dependencies::Complete {
            origins: Origins::Complete(origins),
            ..
        } = &mut dependencies
        {
            origins.truncate(1);
            origins.push(BoundOrigin {
                place: StoragePlace {
                    storage,
                    fields: vec![],
                },
                guard: false,
                opaque: false,
            });
        }
        assert!(matches!(
            dependencies.interference(&values, &plan),
            Interference::Inconclusive(_)
        ));
    }
    let dependencies = Dependencies::Complete {
        reachable: vec![
            Dependency {
                place: place.clone(),
                mutable: false
            };
            300
        ],
        origins: Origins::Complete(vec![]),
    };
    plan.accesses = vec![
        ResolvedAccess {
            place: StoragePlace {
                storage: values.local_storage(&frame, 1).unwrap(),
                fields: vec![]
            },
            kind: AccessKind::Read
        };
        300
    ];
    assert!(
        matches!(dependencies.interference(&values, &plan), Interference::Inconclusive(message) if message.contains("comparison limit"))
    );
    assert_eq!(format!("{:?}", &*values), before);
}

#[test]
fn current_nested_boundary_and_waiting_owner_cannot_supply_query_authority() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let mut c = Contexts::new(&mut h);
    let (root, child) = setup(&mut c, false);
    c.parked.get_mut(&root.id).unwrap().awaiting = Some(child.id);
    inconclusive(&c, &root, &child);
    c.parked.get_mut(&root.id).unwrap().awaiting = None;
    pc(&mut c, &root, 11);
    let p = c.parked.get_mut(&root.id).unwrap();
    c.host.exchange_context(&mut p.host).unwrap();
    p.machine
        .step(c.host, &mut c.values, &mut c.domains)
        .unwrap();
    c.host.exchange_context(&mut p.host).unwrap();
    assert!(
        matches!(query(&c, &root, &child), Interference::Inconclusive(message) if message.contains("owning fault domain"))
    );
    assert!(matches!(
        c.borrower_interference(&root),
        BorrowerInterference::Inconclusive(_)
    ));
    c.terminate().unwrap();
}
