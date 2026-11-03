use super::super::dependencies::Interference;
use super::super::reference_evidence::{Origin, attach_checked};
use super::*;

pub(super) fn host() -> Host {
    Host::new(serde_json::json!({}), None, true, Some(1000))
}
pub(super) fn pair() -> V {
    V::Construct {
        ty: "pair".into(),
        name: "pair".into(),
        fields: vec![V::U32(7), V::U32(9)],
    }
}
pub(super) fn borrow(c: &Contexts<'_>, root: usize, field: usize) -> V {
    let value = super::super::super::reference(&c.values, root, &[field], false, "&U32").unwrap();
    attach_checked(
        &c.values,
        value,
        &[Origin {
            root,
            fields: if field == 0 { &[0] } else { &[1] },
            indirect: false,
            guard: false,
            opaque: false,
        }],
        true,
        true,
    )
}
fn plan(a: &Activation) -> AccessPlan {
    AccessPlan {
        transition: Transition::Local,
        accesses: if a.pc == 0 {
            &[Access {
                slot: Some(0),
                fields: &[0],
                indirect: false,
                kind: AccessKind::Write,
            }]
        } else {
            &[Access {
                slot: Some(0),
                fields: &[1],
                indirect: false,
                kind: AccessKind::Write,
            }]
        },
        delivery: &[],
        opaque: false,
    }
}
fn idle(_: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    Ok(Step::Continue)
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
static ROOT: Control = Control {
    name: "census_root",
    slots: 2,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: Some(plan),
    resume: idle,
};
static CHILD: Control = Control {
    name: "census_child",
    resume: child,
    ..ROOT
};
fn start(c: &mut Contexts<'_>) -> (ContextKey, usize) {
    let root = c.start(&ROOT, vec![pair()]).unwrap();
    let base = c.parked[&root.id].machine.stack[0].base();
    (root, base)
}
pub(super) fn snapshot(c: &Contexts<'_>) -> String {
    let contexts: Vec<_> = c
        .parked
        .iter()
        .chain(c.completed.iter().map(|(id, x)| (id, &x.parked)))
        .map(|(id, p)| {
            let frames: Vec<_> = p
                .machine
                .stack
                .iter()
                .map(|a| {
                    let pending = match &a.pending {
                        Some(Pending::Await(p)) => format!(
                            "{:?}:{:?}:{:?}:{}:{:?}:{}",
                            p.owner,
                            p.domain,
                            p.child,
                            p.queued.is_some(),
                            p.dependencies,
                            p.result.is_some()
                        ),
                        Some(_) => "other".into(),
                        None => "none".into(),
                    };
                    (a.owner, a.base(), a.pc, &a.facts, pending)
                })
                .collect();
            (
                id,
                frames,
                &p.dependencies,
                &p.host.tracker,
                p.parent,
                p.parent_invocation,
                p.awaiting,
            )
        })
        .collect();
    format!(
        "{:?}",
        (
            &*c.values,
            &c.host.tasks,
            &c.host.events,
            c.host.fuel,
            &c.host.resources.scopes,
            contexts,
            &c.waits,
            c.domains.len(),
            c.domains.member_count()
        )
    )
}
fn query(c: &Contexts<'_>, root: &ContextKey) -> BorrowerInterference {
    let before = snapshot(c);
    let result = c.borrower_interference(root);
    for _ in 0..3 {
        assert_eq!(c.borrower_interference(root), result);
    }
    assert_eq!(snapshot(c), before);
    result
}
fn assert_disjoint(
    c: &Contexts<'_>,
    root: &ContextKey,
    contexts: usize,
    handoffs: usize,
    deliveries: usize,
) {
    assert_eq!(
        query(c, root),
        BorrowerInterference::Disjoint {
            contexts,
            handoffs,
            deliveries
        }
    );
}
fn assert_inconclusive(c: &Contexts<'_>, root: &ContextKey) {
    assert!(matches!(
        query(c, root),
        BorrowerInterference::Inconclusive(_)
    ));
}

#[test]
fn sibling_overlap_and_later_incomplete_evidence_cannot_be_hidden_by_one_clear_child() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, base) = start(&mut c);
    let left = c
        .start_child(&root, &CHILD, vec![borrow(&c, base, 0)])
        .unwrap();
    let right = c
        .start_child(&root, &CHILD, vec![borrow(&c, base, 1)])
        .unwrap();
    assert_eq!(c.child_interference(&root, &right), Interference::Disjoint);
    assert!(matches!(
        query(&c, &root),
        BorrowerInterference::Overlap {
            contexts: 2,
            handoffs: 0,
            deliveries: 0,
            ..
        }
    ));
    let saved = c.parked[&right.id].dependencies.clone();
    c.parked.get_mut(&right.id).unwrap().dependencies = Dependencies::Unknown("missing".into());
    assert_inconclusive(&c, &root);
    c.parked.get_mut(&right.id).unwrap().dependencies = saved;
    c.advance_retained(&left, 2).unwrap();
    assert!(matches!(
        query(&c, &root),
        BorrowerInterference::Overlap { contexts: 2, .. }
    ));
    c.join(Some(&root), &left).unwrap();
    assert_disjoint(&c, &root, 1, 0, 0);
    let before = snapshot(&c);
    assert!(c.advance_retained(&root, 1).is_err());
    assert_eq!(snapshot(&c), before);
    c.advance_retained(&right, 2).unwrap();
    c.join(Some(&root), &right).unwrap();
    assert_disjoint(&c, &root, 0, 0, 0);
    c.terminate().unwrap();
}

#[test]
fn descendant_and_readiness_membership_stays_covered_through_completion() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, base) = start(&mut c);
    c.parked.get_mut(&root.id).unwrap().machine.stack[0].pc = 1;
    let child = c
        .start_child(&root, &CHILD, vec![borrow(&c, base, 0)])
        .unwrap();
    c.advance_retained(&child, 1).unwrap();
    let descendant = c
        .start_child(&child, &CHILD, vec![borrow(&c, base, 1)])
        .unwrap();
    let ready = c.park_for_readiness(&child).unwrap();
    assert_eq!(c.child_interference(&root, &child), Interference::Disjoint);
    assert!(matches!(
        query(&c, &root),
        BorrowerInterference::Overlap { contexts: 2, .. }
    ));
    c.advance_retained(&descendant, 2).unwrap();
    assert!(matches!(
        query(&c, &root),
        BorrowerInterference::Overlap { contexts: 2, .. }
    ));
    c.join(Some(&child), &descendant).unwrap();
    assert_disjoint(&c, &root, 1, 0, 0);
    c.signal_readiness(&ready).unwrap();
    c.advance_retained(&child, 1).unwrap();
    assert_disjoint(&c, &root, 1, 0, 0);
    c.join(Some(&root), &child).unwrap();
    c.terminate().unwrap();
}

#[test]
fn independent_membership_census_rejects_missing_duplicate_and_reparented_records() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, base) = start(&mut c);
    let child = c
        .start_child(&root, &CHILD, vec![borrow(&c, base, 1)])
        .unwrap();
    let hidden = c.parked.remove(&child.id).unwrap();
    assert_inconclusive(&c, &root);
    c.parked.insert(child.id, hidden);
    let other = c.start(&ROOT, vec![pair()]).unwrap();
    // Context topology must agree with the independent membership topology.
    c.parked.get_mut(&child.id).unwrap().parent = Some(other.id);
    assert_inconclusive(&c, &root);
    c.parked.get_mut(&child.id).unwrap().parent = Some(root.id);
    let root_member = &c.parked[&root.id].machine.member;
    let child_member = &c.parked[&child.id].machine.member;
    assert!(
        c.domains
            .require_descendants(
                root_member,
                &[(root_member, child_member), (root_member, child_member)]
            )
            .is_err()
    );
    let unattached = c
        .domains
        .attach(&c.parked[&root.id].machine.current, Some(root_member))
        .unwrap();
    assert_inconclusive(&c, &root);
    c.domains.detach(&unattached).unwrap();
    assert_disjoint(&c, &root, 1, 0, 0);
    // Unrelated incomplete input metadata is outside this parent's subtree.
    c.parked.get_mut(&other.id).unwrap().dependencies = Dependencies::Unknown("other root".into());
    assert_disjoint(&c, &root, 1, 0, 0);
    c.terminate().unwrap();
}

fn await_plan(_: &Activation) -> AccessPlan {
    AccessPlan {
        transition: Transition::AwaitTask,
        accesses: &[Access {
            slot: Some(0),
            fields: &[],
            indirect: false,
            kind: AccessKind::Shared,
        }],
        delivery: &[Access {
            slot: Some(1),
            fields: &[],
            indirect: false,
            kind: AccessKind::Write,
        }],
        opaque: true,
    }
}
fn deliver(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let (_, mut fields) = result.outcome()?;
    v[a.base() + 1] = fields.pop();
    Ok(())
}
fn awaiting(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc != 0 {
        return Ok(Step::Continue);
    }
    let value = super::super::super::reference(v, a.base(), &[0], false, "&U32")?;
    let value = attach_checked(
        v,
        value,
        &[Origin {
            root: a.base(),
            fields: &[0],
            indirect: false,
            guard: false,
            opaque: false,
        }],
        true,
        true,
    );
    let mut args = vec![value];
    let mut cold = Some(a.defer_task(h, &CHILD, "pending", &mut args)?);
    let task = a.queue_task(h, &CHILD, &mut cold)?;
    a.pc = 1;
    Ok(Step::Await { task, deliver })
}
static AWAITING: Control = Control {
    name: "census_await",
    access: Some(await_plan),
    resume: awaiting,
    ..ROOT
};
pub(super) fn prepare_await(c: &mut Contexts<'_>) -> ContextKey {
    let root = c.start(&AWAITING, vec![pair()]).unwrap();
    assert!(matches!(
        c.advance_retained(&root, 1).unwrap(),
        ContextPoll::Await
    ));
    root
}

#[test]
fn joined_pending_delivery_retains_coverage_and_exact_owner_domain_without_child_membership() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let root = prepare_await(&mut c);
    assert_inconclusive(&c, &root);
    let child = c.admit_await(&root).unwrap();
    c.advance_retained(&child, 2).unwrap();
    assert_inconclusive(&c, &root);
    c.return_await(&root, &child).unwrap();
    assert_disjoint(&c, &root, 0, 0, 1);
    let original = c.await_continuation(&root).unwrap().dependencies.clone();
    c.await_continuation(&root).unwrap().dependencies =
        Dependencies::Unknown("lost result evidence".into());
    assert_inconclusive(&c, &root);
    c.await_continuation(&root).unwrap().dependencies = original;
    let other = c.start(&ROOT, vec![pair()]).unwrap();
    let other_owner = c.parked[&other.id].machine.stack[0].owner;
    let original_owner = c.await_continuation(&root).unwrap().owner;
    c.await_continuation(&root).unwrap().owner = other_owner;
    assert_inconclusive(&c, &root);
    c.await_continuation(&root).unwrap().owner = original_owner;
    let domain = c.await_continuation(&root).unwrap().domain.clone();
    c.await_continuation(&root).unwrap().domain = c.parked[&other.id].machine.current.clone();
    assert_inconclusive(&c, &root);
    c.await_continuation(&root).unwrap().domain = domain;
    let sibling = c.start_child(&root, &CHILD, vec![V::U32(7)]).unwrap();
    assert_disjoint(&c, &root, 1, 0, 1);
    assert!(c.advance_retained(&root, 1).is_err());
    c.advance_retained(&sibling, 2).unwrap();
    c.join(Some(&root), &sibling).unwrap();
    // Preserve the live borrowed result after its child identity has disappeared.
    c.await_continuation(&root).unwrap().access.delivery = &[Access {
        slot: Some(0),
        fields: &[0],
        indirect: false,
        kind: AccessKind::Write,
    }];
    assert!(matches!(
        query(&c, &root),
        BorrowerInterference::Overlap {
            contexts: 0,
            deliveries: 1,
            ..
        }
    ));
    c.terminate().unwrap();
}

#[test]
fn descendant_queued_handoff_live_await_and_joined_result_are_all_counted() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, _) = start(&mut c);
    let child = c.start_child(&root, &AWAITING, vec![pair()]).unwrap();
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Await
    ));
    assert_disjoint(&c, &root, 1, 1, 0);
    let grandchild = c.admit_await(&child).unwrap();
    assert_disjoint(&c, &root, 2, 1, 0);
    c.advance_retained(&grandchild, 2).unwrap();
    assert_disjoint(&c, &root, 2, 1, 0);
    c.return_await(&child, &grandchild).unwrap();
    assert_disjoint(&c, &root, 1, 0, 1);
    c.terminate().unwrap();
}

fn four_accesses(_: &Activation) -> AccessPlan {
    const A: Access = Access {
        slot: Some(0),
        fields: &[0],
        indirect: false,
        kind: AccessKind::Read,
    };
    AccessPlan {
        transition: Transition::Local,
        accesses: &[A, A, A, A],
        delivery: &[],
        opaque: false,
    }
}
static BUDGET_ROOT: Control = Control {
    access: Some(four_accesses),
    ..ROOT
};

#[test]
fn aggregate_validation_comparison_and_registry_budgets_fail_closed_without_mutation() {
    use super::super::access_resolution::StoragePlace;
    use super::super::dependencies::Dependency;
    use super::super::reference_evidence::Origins;
    for entries in [600, 1200] {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let root = c
            .start(
                if entries == 600 { &BUDGET_ROOT } else { &ROOT },
                vec![pair()],
            )
            .unwrap();
        let a = &c.parked[&root.id].machine.stack[0];
        let place = StoragePlace {
            storage: c.values.local_storage(&a.frame, 0).unwrap(),
            fields: vec![1],
        };
        for _ in 0..30 {
            let child = c.start_child(&root, &CHILD, vec![V::U32(7)]).unwrap();
            c.parked.get_mut(&child.id).unwrap().dependencies = Dependencies::Complete {
                reachable: vec![
                    Dependency {
                        place: place.clone(),
                        mutable: false
                    };
                    entries
                ],
                origins: Origins::Complete(vec![]),
            };
        }
        assert!(
            matches!(query(&c,&root),BorrowerInterference::Inconclusive(message) if message.contains(if entries == 600 { "aggregate interference comparison limit" } else { "aggregate dependency validation limit" }))
        );
        c.terminate().unwrap();
    }
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let (root, _) = start(&mut c);
    let owner = &c.parked[&root.id].machine;
    let mut members = vec![];
    for _ in 0..4096 {
        members.push(
            c.domains
                .attach(&owner.current, Some(&owner.member))
                .unwrap(),
        );
    }
    assert!(
        matches!(query(&c,&root),BorrowerInterference::Inconclusive(message) if message.contains("census limit"))
    );
    for member in members {
        c.domains.detach(&member).unwrap();
    }
    assert_disjoint(&c, &root, 0, 0, 0);
    c.terminate().unwrap();
}

#[test]
fn census_preserves_real_resource_owners_until_fault_settlement() {
    use std::{cell::RefCell, rc::Rc};
    struct Probe(Rc<RefCell<usize>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }
    fn fault(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
        h.fault(V::String("borrower failed".into()))
    }
    static FAULT: Control = Control {
        name: "census_fault",
        resume: fault,
        ..CHILD
    };
    let drops = Rc::new(RefCell::new(0));
    let mut h = host();
    let resource = h.resources.insert("probe", Probe(drops.clone())).unwrap();
    let value = V::Construct {
        ty: "pair".into(),
        name: "pair".into(),
        fields: vec![V::U32(1), resource],
    };
    let mut c = Contexts::new(&mut h);
    let root = c.start(&ROOT, vec![value]).unwrap();
    let child = c.start_child(&root, &FAULT, vec![V::Unit]).unwrap();
    assert_disjoint(&c, &root, 1, 0, 0);
    assert_eq!(*drops.borrow(), 0);
    assert!(c.advance_retained(&root, 1).is_err());
    assert_eq!(*drops.borrow(), 0);
    c.advance_retained(&child, 1).unwrap();
    assert_eq!(*drops.borrow(), 1);
    assert_inconclusive(&c, &root);
    c.terminate().unwrap();
    assert_eq!(*drops.borrow(), 1);
}
