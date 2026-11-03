use super::super::super::reference;
use super::*;

fn record(fields: Vec<V>) -> V {
    V::Construct {
        ty: "holder".into(),
        name: "holder".into(),
        fields,
    }
}
fn formed(values: &Frames, root: usize, fields: &[usize], ty: &str, complete: bool) -> V {
    let mut value = reference(values, root, fields, false, ty).unwrap();
    let V::Reference { evidence, .. } = &mut value else {
        unreachable!()
    };
    evidence.origins = if complete {
        Origins::Complete(vec![])
    } else {
        Origins::Partial(vec![])
    };
    value
}
fn fixture() -> (Frames, super::super::Frame, V) {
    let mut values = Frames::default();
    let f = values.enter(4).unwrap();
    let b = f.base();
    values[b] = Some(V::U32(1));
    values[b + 1] = Some(V::U32(2));
    values[b + 2] = Some(V::Unit);
    values[b + 3] = Some(record(vec![
        formed(&values, b, &[], "&mutable U32", true),
        record(vec![formed(&values, b + 1, &[], "&U32", true)]),
    ]));
    let mut holder = formed(&values, b + 3, &[], "&mutable holder", true);
    let V::Reference { evidence, .. } = &mut holder else {
        unreachable!()
    };
    // Inherited origins remain symbolic, even when their path is not currently
    // projectable. Snapshot traversal follows actual values only.
    evidence.origins = Origins::Complete(vec![BoundOrigin {
        place: StoragePlace {
            storage: values.storage_at(b + 2).unwrap(),
            fields: vec![999],
        },
        guard: true,
        opaque: true,
    }]);
    (values, f, holder)
}

#[test]
fn holder_snapshot_retains_full_graph_and_symbolic_ancestry_without_effects() {
    let (values, f, holder) = fixture();
    let b = f.base();
    let before = format!("{:?}:{holder:?}", &*values);
    let origins = Dependencies::holder_origins(&values, &holder);
    let Origins::Complete(entries) = &origins else {
        panic!("{origins:?}")
    };
    for root in [b, b + 1, b + 3] {
        assert!(entries.iter().any(
            |o| values.require_storage(&o.place.storage).unwrap() == root
                && o.place.fields.is_empty()
                && !o.guard
                && !o.opaque
        ));
    }
    assert!(entries.iter().any(|o| o.guard
        && o.opaque
        && values.require_storage(&o.place.storage).unwrap() == b + 3));
    assert!(entries.iter().any(|o| o.place.fields == [999] && o.guard));
    assert_eq!(before, format!("{:?}:{holder:?}", &*values));
    assert_eq!(origins, Dependencies::holder_origins(&values, &holder));
}

#[test]
fn holder_snapshot_requires_every_formation_and_never_promotes_partial_proof() {
    for mutation in 0..9 {
        let (mut values, f, mut holder) = fixture();
        let b = f.base();
        let mut foreign = Frames::default();
        let other = foreign.enter(1).unwrap();
        if mutation == 0 {
            let V::Reference { evidence, .. } = &mut holder else {
                unreachable!()
            };
            evidence.origins = Origins::Partial(vec![]);
        } else if mutation == 1 {
            let V::Construct { fields, .. } = values[b + 3].as_mut().unwrap() else {
                unreachable!()
            };
            let V::Reference { evidence, .. } = &mut fields[0] else {
                unreachable!()
            };
            evidence.origins = Origins::Partial(vec![]);
        } else if mutation == 2 {
            let V::Reference { evidence, .. } = &mut holder else {
                unreachable!()
            };
            evidence.target = None;
        } else if mutation == 3 {
            let V::Construct { fields, .. } = values[b + 3].as_mut().unwrap() else {
                unreachable!()
            };
            let V::Reference { evidence, .. } = &mut fields[0] else {
                unreachable!()
            };
            evidence.target = None;
        } else if mutation == 4 {
            let V::Construct { fields, .. } = values[b + 3].as_mut().unwrap() else {
                unreachable!()
            };
            let V::Reference { evidence, .. } = &mut fields[0] else {
                unreachable!()
            };
            evidence.target.as_mut().unwrap().storage = foreign.storage_at(other.base()).unwrap();
        } else if mutation == 5 {
            let V::Reference { fields, .. } = &mut holder else {
                unreachable!()
            };
            fields.push(0);
        } else if mutation == 6 {
            let V::Reference { evidence, .. } = &mut holder else {
                unreachable!()
            };
            let Origins::Complete(origins) = &mut evidence.origins else {
                unreachable!()
            };
            origins[0].place.storage = foreign.storage_at(other.base()).unwrap();
        } else if mutation == 7 {
            values.retire(&f).unwrap();
            let reused = values.enter(4).unwrap();
            assert_eq!(reused.base(), b);
        } else {
            holder = formed(&values, b, &[], "&U32", true);
        }
        let before = format!("{:?}:{holder:?}", &*values);
        let result = Dependencies::holder_origins(&values, &holder);
        if mutation < 2 {
            assert!(
                matches!(result, Origins::Partial(_)),
                "{mutation}: {result:?}"
            );
        } else {
            assert!(
                matches!(result, Origins::Unavailable(_)),
                "{mutation}: {result:?}"
            );
        }
        assert_eq!(before, format!("{:?}:{holder:?}", &*values));
    }
}

#[test]
fn holder_snapshot_bounds_graph_work_and_handles_repeated_or_cyclic_storage() {
    let mut values = Frames::default();
    let f = values.enter(2).unwrap();
    let b = f.base();
    values[b] = Some(V::Unit);
    let leaf = formed(&values, b, &[], "&U32", true);
    values[b + 1] = Some(record(vec![leaf.clone(), leaf]));
    let holder = formed(&values, b + 1, &[], "&holder", true);
    let Origins::Complete(origins) = Dependencies::holder_origins(&values, &holder) else {
        panic!()
    };
    assert_eq!(
        origins
            .iter()
            .filter(|o| values.require_storage(&o.place.storage).unwrap() == b)
            .count(),
        1
    );
    values[b + 1] = Some(record(vec![holder.clone()]));
    assert!(matches!(
        Dependencies::holder_origins(&values, &holder),
        Origins::Complete(_)
    ));
    values[b + 1] = Some(record(vec![V::Unit; LIMIT]));
    assert!(matches!(
        Dependencies::holder_origins(&values, &holder),
        Origins::Unavailable(_)
    ));
    // The final flattened union has its own origin budget as well as the walk.
    values[b + 1] = Some(record(vec![]));
    let mut large = holder.clone();
    let V::Reference { evidence, .. } = &mut large else {
        unreachable!()
    };
    evidence.origins = Origins::Complete(vec![BoundOrigin {
        place: StoragePlace {
            storage: values.storage_at(b).unwrap(),
            fields: vec![0; LIMIT],
        },
        guard: false,
        opaque: false,
    }]);
    assert!(matches!(
        Dependencies::holder_origins(&values, &large),
        Origins::Unavailable(_)
    ));
    assert!(matches!(
        Dependencies::holder_origins(&values, &V::Unit),
        Origins::Unavailable(_)
    ));
}
