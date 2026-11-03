use super::*;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn record(fields: Vec<V>) -> V {
    V::Construct {
        ty: "record".into(),
        name: "record".into(),
        fields,
    }
}
fn access(slot: Option<usize>, fields: &'static [usize], kind: AccessKind) -> Access {
    Access {
        slot,
        fields,
        kind,
        indirect: false,
    }
}

#[test]
fn resolved_inputs_and_uninitialized_destinations_preserve_storage_and_resource_owners() {
    struct Probe(Rc<RefCell<usize>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }
    let drops = Rc::new(RefCell::new(0));
    let mut host = Host::new(json!({}), None, true, Some(20));
    let mut values = Frames::default();
    let prefix = values.enter(2).unwrap();
    let frame = values.enter(3).unwrap();
    let text = "same allocation".repeat(100);
    let allocation = text.as_ptr();
    values[frame.base()] = Some(record(vec![
        record(vec![V::String(text)]),
        host.resources
            .insert("probe", Probe(drops.clone()))
            .unwrap(),
    ]));
    let plan = AccessPlan {
        cold_await: None,
        transition: Transition::Local,
        accesses: &[
            Access {
                slot: Some(0),
                fields: &[0, 0],
                indirect: false,
                kind: AccessKind::Move,
            },
            Access {
                slot: Some(1),
                fields: &[],
                indirect: false,
                kind: AccessKind::Write,
            },
            Access {
                slot: Some(2),
                fields: &[],
                indirect: false,
                kind: AccessKind::Retire,
            },
        ],
        delivery: &[],
        opaque: false,
    };
    let before = format!("{:?}", &*values);
    let resolved = plan.resolve(&values, &frame, None).unwrap();
    assert_eq!(
        resolved.accesses.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AccessKind::Move, AccessKind::Write, AccessKind::Retire]
    );
    assert_eq!(resolved.accesses[0].place.fields, [0, 0]);
    assert_eq!(
        values
            .require_storage(&resolved.accesses[1].place.storage)
            .unwrap(),
        frame.base() + 1
    );
    for _ in 0..4 {
        assert_eq!(plan.resolve(&values, &frame, None).unwrap(), resolved);
    }
    assert_eq!(format!("{:?}", &*values), before);
    let V::Construct { fields, .. } = values[frame.base()].as_ref().unwrap() else {
        panic!()
    };
    let V::Construct { fields, .. } = &fields[0] else {
        panic!()
    };
    let V::String(text) = &fields[0] else {
        panic!()
    };
    assert_eq!(text.as_ptr(), allocation);
    assert_eq!(*drops.borrow(), 0);
    assert_eq!(host.fuel, Some(20));
    assert!(host.events.is_empty() && host.tasks.is_empty());
    host.release_frame(values.frame_values(&frame).unwrap());
    values.retire(&frame).unwrap();
    values.retire(&prefix).unwrap();
    assert_eq!(*drops.borrow(), 1);
}

#[test]
fn indirect_accesses_keep_holder_reads_and_compose_paths_without_following_nested_references() {
    let mut values = Frames::default();
    let owner = values.enter(2).unwrap();
    let frame = values.enter(1).unwrap();
    values[owner.base()] = Some(record(vec![record(vec![V::U32(7), V::U32(9)])]));
    values[owner.base() + 1] = Some(V::U32(11));
    for (ty, kinds) in [
        ("&record", vec![AccessKind::Read, AccessKind::Shared]),
        (
            "&mutable record",
            vec![
                AccessKind::Read,
                AccessKind::Move,
                AccessKind::Shared,
                AccessKind::Exclusive,
                AccessKind::Write,
            ],
        ),
    ] {
        values[frame.base()] = Some(V::Reference {
            evidence: Default::default(),
            ty: ty.into(),
            root: owner.base(),
            fields: vec![0],
        });
        for kind in kinds {
            let a = Access {
                indirect: true,
                ..access(Some(0), &[1], kind)
            };
            let result = resolve(&[a], &values, &frame, None).unwrap();
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].kind, AccessKind::Read);
            assert_eq!(
                result[0].place.storage,
                values.local_storage(&frame, 0).unwrap()
            );
            assert!(result[0].place.fields.is_empty());
            assert_eq!(result[1].kind, kind);
            assert_eq!(result[1].place.fields, [0, 1]);
            assert_eq!(
                result[1].place.storage,
                values.local_storage(&owner, 0).unwrap()
            );
        }
    }
    // Reading a stored reference is not reading its referent. Dependency traversal
    // for captured reference carriers is a separate analysis.
    values[owner.base()] = Some(record(vec![V::Reference {
        evidence: Default::default(),
        ty: "&U32".into(),
        root: owner.base() + 1,
        fields: vec![],
    }]));
    values[frame.base()] = Some(V::Reference {
        evidence: Default::default(),
        ty: "&record".into(),
        root: owner.base(),
        fields: vec![],
    });
    let result = resolve(
        &[Access {
            indirect: true,
            ..access(Some(0), &[0], AccessKind::Read)
        }],
        &values,
        &frame,
        None,
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[1].place.fields, [0]);
    assert_eq!(
        result[1].place.storage,
        values.local_storage(&owner, 0).unwrap()
    );
}

#[test]
fn invalid_accesses_reject_atomically_without_running_or_consuming_prior_operands() {
    let mut values = Frames::default();
    let owner = values.enter(1).unwrap();
    let frame = values.enter(3).unwrap();
    values[owner.base()] = Some(record(vec![V::String("keep".into())]));
    values[frame.base()] = Some(V::U32(8));
    values[frame.base() + 1] = Some(V::Reference {
        evidence: Default::default(),
        ty: "&record".into(),
        root: owner.base(),
        fields: vec![],
    });
    let moved = access(Some(0), &[], AccessKind::Move);
    for invalid in [
        access(Some(2), &[], AccessKind::Read),
        access(Some(2), &[0], AccessKind::Write),
        access(Some(0), &[0], AccessKind::Read),
        access(Some(usize::MAX), &[], AccessKind::Write),
        access(None, &[], AccessKind::Write),
        Access {
            indirect: true,
            ..access(Some(0), &[], AccessKind::Read)
        },
        Access {
            indirect: true,
            ..access(Some(1), &[1], AccessKind::Read)
        },
        Access {
            indirect: true,
            ..access(Some(1), &[0], AccessKind::Write)
        },
        Access {
            indirect: true,
            ..access(Some(1), &[0], AccessKind::Exclusive)
        },
    ] {
        let before = values.to_vec();
        assert!(resolve(&[moved, invalid], &values, &frame, None).is_err());
        assert_eq!(*values, before);
    }
    let read = Access {
        indirect: true,
        ..access(Some(1), &[], AccessKind::Read)
    };
    for (ty, root) in [("bad", owner.base()), ("&U32", usize::MAX)] {
        values[frame.base() + 1] = Some(V::Reference {
            evidence: Default::default(),
            ty: ty.into(),
            root,
            fields: vec![],
        });
        let before = values.to_vec();
        assert!(resolve(&[moved, read], &values, &frame, None).is_err());
        assert_eq!(*values, before);
    }
    let mut other = Frames::default();
    let foreign = other.enter(3).unwrap();
    assert!(
        AccessPlan::UNKNOWN
            .resolve(&values, &foreign, None)
            .is_err()
    );
    values.retire(&frame).unwrap();
    let replacement = values.enter(3).unwrap();
    assert_eq!(replacement.base(), frame.base());
    assert!(AccessPlan::SKIP.resolve(&values, &frame, None).is_err());
    assert!(
        AccessPlan::SKIP
            .resolve(&values, &replacement, None)
            .is_ok()
    );
}

#[test]
fn constructor_destination_and_deferred_writes_resolve_in_their_own_frames() {
    let mut values = Frames::default();
    let caller = values.enter(2).unwrap();
    let callee = values.enter(1).unwrap();
    values[callee.base()] = Some(V::String("result".into()));
    let plan = AccessPlan {
        cold_await: None,
        transition: Transition::Return,
        accesses: &[
            Access {
                slot: Some(0),
                fields: &[],
                indirect: false,
                kind: AccessKind::Move,
            },
            Access {
                slot: None,
                fields: &[],
                indirect: false,
                kind: AccessKind::Write,
            },
        ],
        delivery: &[],
        opaque: true,
    };
    let result = plan
        .resolve(&values, &callee, Some(caller.base() + 1))
        .unwrap();
    assert!(result.opaque);
    assert_eq!(
        result.accesses[0].place.storage,
        values.local_storage(&callee, 0).unwrap()
    );
    assert_eq!(
        result.accesses[1].place.storage,
        values.local_storage(&caller, 1).unwrap()
    );
    let mut pending = plan;
    pending.transition = Transition::AwaitTask;
    pending.accesses = &[];
    pending.delivery = &[Access {
        slot: Some(0),
        fields: &[],
        indirect: false,
        kind: AccessKind::Write,
    }];
    let result = pending.resolve(&values, &caller, None).unwrap();
    assert!(result.accesses.is_empty());
    assert_eq!(
        result.delivery[0].place.storage,
        values.local_storage(&caller, 0).unwrap()
    );
    values.retire(&caller).unwrap();
    assert!(
        plan.resolve(&values, &callee, Some(caller.base() + 1))
            .is_err()
    );
    assert!(
        values
            .require_storage(&result.delivery[0].place.storage)
            .is_err()
    );
    let reused = values.enter(2).unwrap();
    assert_eq!(reused.base(), caller.base());
    assert!(
        values
            .require_storage(&result.delivery[0].place.storage)
            .is_err()
    );
    assert!(plan.resolve(&values, &callee, Some(usize::MAX)).is_err());
}

#[test]
fn strict_query_budget_counts_inherited_paths_and_holder_reads_before_allocation() {
    let mut values = Frames::default();
    let owner = values.enter(1).unwrap();
    let frame = values.enter(1).unwrap();
    values[owner.base()] = Some(record(vec![record(vec![V::U32(7)])]));
    values[frame.base()] = Some(
        super::super::super::reference(&values, owner.base(), &[0, 0], false, "&U32").unwrap(),
    );
    let input = Access {
        slot: Some(0),
        fields: &[],
        indirect: true,
        kind: AccessKind::Read,
    };
    let before = format!("{:?}", &*values);
    assert!(resolve_checked(&vec![input; 1024], &values, &frame, None, true).is_ok());
    assert!(
        resolve_checked(&vec![input; 1025], &values, &frame, None, true)
            .unwrap_err()
            .contains("resolved callback access query limit")
    );
    assert_eq!(format!("{:?}", &*values), before);
}
