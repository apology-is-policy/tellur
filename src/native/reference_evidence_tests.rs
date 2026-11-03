use super::super::super::{borrowed_variant, collection, read_place, reference, take_place};
use super::super::dependencies::Dependencies;
use super::*;

fn origin(root: usize, fields: &'static [usize], indirect: bool, guard: bool) -> Origin {
    Origin {
        root,
        fields,
        indirect,
        guard,
        opaque: guard,
    }
}
fn record(ty: &str, fields: Vec<V>) -> V {
    V::Construct {
        ty: ty.into(),
        name: ty.split('<').next().unwrap().into(),
        fields,
    }
}
fn evidence(value: &V) -> &ReferenceEvidence {
    let V::Reference { evidence, .. } = value else {
        panic!("reference expected")
    };
    evidence
}
fn origins(value: &V) -> &[BoundOrigin] {
    let Origins::Partial(origins) = &evidence(value).origins else {
        panic!("partial origins expected")
    };
    origins
}

#[test]
fn complete_carrier_root_cannot_promote_partial_or_stale_stored_references() {
    let mut values = Frames::default();
    let frame = values.enter(2).unwrap();
    let b = frame.base();
    values[b] = Some(V::String("owner".into()));
    let leaf = attach_checked(
        &values,
        reference(&values, b, &[], false, "&String").unwrap(),
        &[origin(b, &[], false, false)],
        true,
        true,
    );
    values[b + 1] = Some(record("view", vec![leaf.clone()]));
    let carrier = attach_checked(
        &values,
        reference(&values, b + 1, &[], false, "&view").unwrap(),
        &[
            origin(b + 1, &[], false, false),
            origin(b, &[], false, false),
        ],
        true,
        true,
    );
    let Dependencies::Complete {
        reachable,
        origins: Origins::Complete(origins),
    } = Dependencies::describe(&values, &[carrier.clone()])
    else {
        panic!("complete local graph");
    };
    assert_eq!(reachable.len(), 2);
    assert_eq!(origins.len(), 2);
    // Even exhaustive static root origins cannot conceal an incomplete incoming
    // formation proof at a reachable stored reference.
    let mut partial = leaf.clone();
    let V::Reference { evidence, .. } = &mut partial else {
        unreachable!()
    };
    evidence.origins = Origins::default();
    values[b + 1] = Some(record("view", vec![partial]));
    assert!(matches!(
        Dependencies::describe(&values, &[carrier.clone()]),
        Dependencies::Partial { .. }
    ));
    let foreign = Frames::default();
    let mut stale = leaf;
    let V::Reference { evidence, .. } = &mut stale else {
        unreachable!()
    };
    evidence.target = Some(StoragePlace {
        storage: {
            let mut foreign = foreign;
            let f = foreign.enter(1).unwrap();
            foreign.local_storage(&f, 0).unwrap()
        },
        fields: vec![],
    });
    values[b + 1] = Some(record("view", vec![stale]));
    assert!(matches!(
        Dependencies::describe(&values, &[carrier]),
        Dependencies::Unknown(_)
    ));
}

#[test]
fn detached_guards_survive_local_moves_reborrows_and_shared_attenuation() {
    let mut values = Frames::default();
    let f = values.enter(4).unwrap();
    let b = f.base();
    values[b] = Some(V::String("same allocation".repeat(100)));
    let reference = reference(&values, b, &[], false, "&mutable String").unwrap();
    values[b + 1] = Some(record(
        "view",
        vec![
            reference.clone(),
            super::super::super::reference(&values, b, &[], false, "&String").unwrap(),
        ],
    ));
    let detached = attach(
        &values,
        reference.clone(),
        &[
            origin(b, &[], false, false),
            origin(b + 1, &[], false, true),
        ],
    );
    assert_eq!(
        reference, detached,
        "metadata does not affect value equality"
    );
    assert_ne!(evidence(&reference), evidence(&detached));
    values[b + 2] = Some(detached);
    let shared = super::super::super::reference(&values, b + 2, &[], true, "&String").unwrap();
    assert_eq!(origins(&shared).len(), 2);
    assert!(
        origins(&shared)
            .iter()
            .any(|o| o.guard && o.place.storage == values.local_storage(&f, 1).unwrap())
    );
    let Dependencies::Partial {
        reachable,
        origins: checked,
    } = Dependencies::describe(&values, &[shared.clone()])
    else {
        panic!()
    };
    assert_eq!(reachable.len(), 1);
    assert!(
        !reachable[0].mutable,
        "guard flags do not confer mutable authority"
    );
    assert_eq!(checked, evidence(&shared).origins);
    values[b + 3] = Some(take_place(&mut values, b + 2, &[], false).unwrap());
    let copied = read_place(&values, b + 3, &[], false).unwrap();
    assert_eq!(evidence(&copied), evidence(values[b + 3].as_ref().unwrap()));
    assert!(values[b + 2].is_none());
    let from_shared_field = read_place(&values, b + 1, &[1], false).unwrap();
    assert!(
        origins(&from_shared_field).is_empty(),
        "copy does not invent holder origins"
    );
}

#[test]
fn symbolic_origins_bind_allocations_without_projecting_inactive_payloads() {
    let mut values = Frames::default();
    let f = values.enter(3).unwrap();
    let b = f.base();
    values[b] = Some(V::String("live".into()));
    values[b + 1] = Some(record("selected_empty", vec![]));
    values[b + 2] = Some(reference(&values, b + 1, &[], false, "&sum").unwrap());
    let before = format!("{values:?}", values = &*values);
    let value = attach(
        &values,
        reference(&values, b, &[], false, "&String").unwrap(),
        &[origin(b + 2, &[999, 123], true, true)],
    );
    let checked = origins(&value);
    assert_eq!(checked[0].place.fields, [999, 123]);
    assert_eq!(
        checked[0].place.storage,
        values.local_storage(&f, 1).unwrap()
    );
    Dependencies::describe(&values, &[value])
        .require_live(&values)
        .unwrap();
    assert_eq!(format!("{values:?}", values = &*values), before);
}

#[test]
fn formation_identities_cannot_be_rebound_after_reuse_or_in_a_foreign_arena() {
    let mut values = Frames::default();
    let owner = values.enter(1).unwrap();
    values[owner.base()] = Some(V::String("old".into()));
    let old = reference(&values, owner.base(), &[], false, "&String").unwrap();
    values.retire(&owner).unwrap();
    let replacement = values.enter(1).unwrap();
    assert_eq!(replacement.base(), owner.base());
    values[replacement.base()] = Some(V::String("new".into()));
    assert!(matches!(
        Dependencies::describe(&values, &[old.clone()]),
        Dependencies::Unknown(_)
    ));
    let holder = values.enter(1).unwrap();
    values[holder.base()] = Some(old.clone());
    let rebound = reference(&values, holder.base(), &[], true, "&String").unwrap();
    assert!(matches!(
        evidence(&rebound).origins,
        Origins::Unavailable(_)
    ));
    assert!(
        Dependencies::describe(&values, &[rebound])
            .require_live(&values)
            .is_err()
    );
    let mut foreign = Frames::default();
    let f = foreign.enter(1).unwrap();
    foreign[f.base()] = Some(V::String("foreign".into()));
    assert!(matches!(
        Dependencies::describe(&foreign, &[old]),
        Dependencies::Unknown(_)
    ));
}

#[test]
fn sum_and_collection_results_preserve_provenance_and_only_selected_paths() {
    let mut values = Frames::default();
    let f = values.enter(4).unwrap();
    let b = f.base();
    values[b] = Some(V::String("leaf".into()));
    let leaf = attach(
        &values,
        reference(&values, b, &[], false, "&mutable String").unwrap(),
        &[origin(b, &[], false, false)],
    );
    values[b + 1] = Some(record("selected", vec![leaf, V::U32(7)]));
    values[b + 2] = Some(V::Unit);
    let sum = attach(
        &values,
        reference(&values, b + 1, &[], false, "&sum").unwrap(),
        &[origin(b + 2, &[], false, true)],
    );
    let (_, payloads) = borrowed_variant(&values, sum).unwrap();
    assert_eq!(payloads[0].ty(), "&String");
    assert_eq!(origins(&payloads[0]).len(), 2);
    assert_eq!(evidence(&payloads[1]).target.as_ref().unwrap().fields, [1]);
    assert!(origins(&payloads[1])[0].guard);
    values[b + 3] = Some(record("vector<String>", vec![V::String("item".into())]));
    let vector = attach(
        &values,
        reference(&values, b + 3, &[], false, "&vector<String>").unwrap(),
        &[origin(b + 2, &[], false, true)],
    );
    let (_, item) = collection("vector_get", vec![vector, V::U32(0)], &mut values).unwrap();
    assert_eq!(evidence(&item[0]).target.as_ref().unwrap().fields, [0]);
    assert!(origins(&item[0])[0].guard);
    // The shared-reference collection returns an owned copy, without its holder.
    let shared = reference(&values, b, &[], false, "&String").unwrap();
    values[b + 3] = Some(record("vector<&String>", vec![shared]));
    let vector = attach(
        &values,
        reference(&values, b + 3, &[], false, "&vector<&String>").unwrap(),
        &[origin(b + 2, &[], false, true)],
    );
    let (_, item) = collection("vector_get", vec![vector, V::U32(0)], &mut values).unwrap();
    assert!(origins(&item[0]).is_empty());
    assert_eq!(
        evidence(&item[0]).target.as_ref().unwrap().storage,
        values.local_storage(&f, 0).unwrap()
    );
}

#[test]
fn unavailable_and_bounded_evidence_never_changes_execution() {
    let mut values = Frames::default();
    let f = values.enter(1).unwrap();
    values[f.base()] = Some(V::String("source".into()));
    let plain = reference(&values, f.base(), &[], false, "&String").unwrap();
    let invalid = attach(
        &values,
        plain.clone(),
        &[origin(usize::MAX, &[], true, true)],
    );
    assert!(matches!(
        evidence(&invalid).origins,
        Origins::Unavailable(_)
    ));
    let again = attach(&values, invalid, &[origin(f.base(), &[], false, false)]);
    assert_eq!(plain, again);
    assert!(matches!(evidence(&again).origins, Origins::Unavailable(_)));
    assert!(
        Dependencies::describe(&values, &[again])
            .require_live(&values)
            .is_err()
    );
    let guarded = attach(
        &values,
        plain.clone(),
        &[origin(f.base(), &[], false, true)],
    );
    assert!(
        matches!(
            Dependencies::describe(&values, &vec![guarded; LIMIT / 2]),
            Dependencies::Unknown(_)
        ),
        "inherited origins count toward the capture traversal budget"
    );
    let descriptions: Vec<_> = (0..=LIMIT)
        .map(|_| origin(f.base(), &[], false, false))
        .collect();
    let bounded = attach(&values, plain.clone(), &descriptions);
    assert_eq!(plain, bounded);
    assert!(matches!(
        evidence(&bounded).origins,
        Origins::Unavailable(_)
    ));
    assert_eq!(
        read_place(&values, f.base(), &[], false).unwrap(),
        V::String("source".into())
    );
}

#[test]
fn complete_direct_origins_require_complete_incoming_reborrow_evidence() {
    let mut values = Frames::default();
    let frame = values.enter(2).unwrap();
    let b = frame.base();
    values[b] = Some(V::String("source".into()));
    let direct = attach_checked(
        &values,
        reference(&values, b, &[], false, "&mutable String").unwrap(),
        &[origin(b, &[], false, false)],
        true,
        true,
    );
    assert!(evidence(&direct).origins.is_complete());
    assert!(matches!(
        Dependencies::describe(&values, &[direct.clone()]),
        Dependencies::Complete { .. }
    ));
    values[b + 1] = Some(direct);
    let inherited = attach_checked(
        &values,
        reference(&values, b + 1, &[], true, "&String").unwrap(),
        &[origin(b + 1, &[], true, false)],
        true,
        false,
    );
    assert!(evidence(&inherited).origins.is_complete());
    let Dependencies::Complete { reachable, .. } = Dependencies::describe(&values, &[inherited])
    else {
        panic!()
    };
    assert!(!reachable[0].mutable);
    // A local complete annotation cannot fill the caller's missing origin proof.
    values[b + 1] = Some(reference(&values, b, &[], false, "&String").unwrap());
    let partial = attach_checked(
        &values,
        reference(&values, b + 1, &[], true, "&String").unwrap(),
        &[origin(b + 1, &[], true, false)],
        true,
        false,
    );
    assert!(!evidence(&partial).origins.is_complete());
    assert!(matches!(
        Dependencies::describe(&values, &[partial]),
        Dependencies::Partial { .. }
    ));
}

#[test]
fn completion_union_preserves_completeness_only_when_every_input_is_complete() {
    let mut values = Frames::default();
    let frame = values.enter(1).unwrap();
    let b = frame.base();
    values[b] = Some(V::String("owner".into()));
    let complete = attach_checked(
        &values,
        reference(&values, b, &[], false, "&String").unwrap(),
        &[origin(b, &[], false, false)],
        true,
        true,
    );
    let complete = Dependencies::describe(&values, &[complete]);
    let mut combined = complete.clone();
    combined.extend(complete.clone());
    assert!(matches!(combined, Dependencies::Complete { .. }));
    let partial = Dependencies::describe(
        &values,
        &[reference(&values, b, &[], false, "&String").unwrap()],
    );
    combined.extend(partial);
    assert!(matches!(combined, Dependencies::Partial { .. }));
    combined.extend(complete);
    assert!(matches!(combined, Dependencies::Partial { .. }));
    combined.extend(Dependencies::Unknown("missing".into()));
    assert!(matches!(combined, Dependencies::Unknown(_)));
}

#[test]
fn guarded_completeness_requires_fresh_local_paired_annotations() {
    let mut values = Frames::default();
    let f = values.enter(3).unwrap();
    let b = f.base();
    values[b] = Some(V::U32(7));
    let raw = reference(&values, b, &[], false, "&mutable U32").unwrap();
    values[b + 1] = Some(record("view", vec![raw.clone()]));
    values[b + 2] = Some(reference(&values, b + 1, &[], false, "&mutable view").unwrap());
    let valid = [
        origin(b, &[], false, false),
        origin(b + 1, &[], false, true),
    ];
    let local = attach_checked(&values, raw.clone(), &valid, true, true);
    assert!(matches!(evidence(&local).origins, Origins::Complete(_)));
    assert!(matches!(
        Dependencies::describe(&values, &[local]),
        Dependencies::Complete { .. }
    ));
    let received = attach_checked(&values, raw.clone(), &valid, true, false);
    assert!(matches!(evidence(&received).origins, Origins::Partial(_)));
    for kind in 0..3 {
        let mut bad = origin(b + 1, &[], false, true);
        if kind == 0 {
            bad.opaque = false;
        }
        if kind == 1 {
            bad.guard = false;
        }
        if kind == 2 {
            bad.root = b + 2;
            bad.indirect = true;
        }
        let value = attach_checked(&values, raw.clone(), &[bad], true, true);
        assert!(matches!(evidence(&value).origins, Origins::Unavailable(_)));
        assert!(matches!(
            Dependencies::describe(&values, &[value]),
            Dependencies::Partial {
                origins: Origins::Unavailable(_),
                ..
            }
        ));
    }
    values.retire(&f).unwrap();
}

#[test]
fn incoming_field_transport_preserves_complete_and_partial_evidence_exactly() {
    let mut values = Frames::default();
    let frame = values.enter(4).unwrap();
    let b = frame.base();
    values[b] = Some(V::String("owner".into()));
    values[b + 1] = Some(V::Unit);
    let leaf = attach_checked(
        &values,
        reference(&values, b, &[], false, "&String").unwrap(),
        &[
            origin(b, &[], false, false),
            origin(b + 1, &[], false, true),
        ],
        true,
        true,
    );
    for complete in [true, false] {
        let mut incoming = leaf.clone();
        if !complete {
            let V::Reference { evidence, .. } = &mut incoming else {
                unreachable!()
            };
            let Origins::Complete(entries) = &evidence.origins else {
                unreachable!()
            };
            evidence.origins = Origins::Partial(entries.clone());
        }
        values[b + 2] = Some(record("view", vec![incoming.clone()]));
        values[b + 3] = Some(reference(&values, b + 2, &[], false, "&view").unwrap());
        let before = format!("{:?}", &*values);
        for (root, indirect) in [(b + 2, false), (b + 3, true)] {
            let copied = read_place(&values, root, &[0], indirect).unwrap();
            let transported = inherit_checked(&values, copied.clone());
            assert_eq!(copied, transported);
            assert_eq!(evidence(&transported), evidence(&incoming));
            assert_eq!(evidence(&transported).origins.is_complete(), complete);
        }
        assert_eq!(format!("{:?}", &*values), before);
    }
}

#[test]
fn incoming_transport_never_repairs_missing_foreign_or_mismatched_metadata() {
    let mut values = Frames::default();
    let frame = values.enter(2).unwrap();
    values[frame.base()] = Some(V::String("owner".into()));
    let complete = attach_checked(
        &values,
        reference(&values, frame.base(), &[], false, "&String").unwrap(),
        &[origin(frame.base(), &[], false, false)],
        true,
        true,
    );
    let mut foreign = Frames::default();
    let other = foreign.enter(1).unwrap();
    let foreign_storage = foreign.local_storage(&other, 0).unwrap();
    for mutation in 0..7 {
        let mut incoming = complete.clone();
        let V::Reference {
            evidence: proof,
            root,
            ..
        } = &mut incoming
        else {
            unreachable!()
        };
        match mutation {
            0 => proof.target = None,
            1 => proof.target.as_mut().unwrap().storage = foreign_storage.clone(),
            2 => proof.target.as_mut().unwrap().fields.push(0),
            3 => *root += 1,
            4 => {
                let Origins::Complete(entries) = &mut proof.origins else {
                    unreachable!()
                };
                entries[0].place.storage = foreign_storage.clone();
            }
            5 => proof.origins = Origins::Unavailable("absent proof".into()),
            _ => {
                let Origins::Complete(entries) = &mut proof.origins else {
                    unreachable!()
                };
                entries[0].place.fields = vec![0; LIMIT];
            }
        }
        let transported = inherit_checked(&values, incoming.clone());
        assert_eq!(
            incoming, transported,
            "metadata cannot change source equality"
        );
        assert!(matches!(
            evidence(&transported).origins,
            Origins::Unavailable(_)
        ));
    }
    let unannotated = reference(&values, frame.base(), &[], false, "&String").unwrap();
    assert!(matches!(
        evidence(&inherit_checked(&values, unannotated)).origins,
        Origins::Partial(_)
    ));
    let retired = values.enter(1).unwrap();
    let mut stale_origin = complete.clone();
    let V::Reference {
        evidence: proof, ..
    } = &mut stale_origin
    else {
        unreachable!()
    };
    let Origins::Complete(entries) = &mut proof.origins else {
        unreachable!()
    };
    entries[0].place.storage = values.local_storage(&retired, 0).unwrap();
    values.retire(&retired).unwrap();
    assert!(matches!(
        evidence(&inherit_checked(&values, stale_origin)).origins,
        Origins::Unavailable(_)
    ));
    values.retire(&frame).unwrap();
    let replacement = values.enter(2).unwrap();
    assert_eq!(replacement.base(), frame.base());
    let stale = inherit_checked(&values, complete);
    assert!(matches!(evidence(&stale).origins, Origins::Unavailable(_)));
}

fn guarded_fixture() -> (Frames, super::super::Frame, V, Vec<Origin>) {
    let mut values = Frames::default();
    let frame = values.enter(5).unwrap();
    let b = frame.base();
    values[b] = Some(record("pair", vec![V::U32(1), V::U32(2)]));
    values[b + 1] = Some(V::U32(3));
    let formed = |values: &Frames, root, ty| {
        let mut v = reference(values, root, &[], false, ty).unwrap();
        let V::Reference { evidence, .. } = &mut v else {
            unreachable!()
        };
        evidence.origins = Origins::Complete(vec![]);
        v
    };
    let leaf = formed(&values, b, "&mutable pair");
    let sibling = formed(&values, b + 1, "&U32");
    values[b + 2] = Some(record("view", vec![leaf.clone(), sibling]));
    values[b + 3] = Some(formed(&values, b + 2, "&mutable view"));
    values[b + 4] = Some(leaf);
    let selected = reference(&values, b + 4, &[0], true, "&U32").unwrap();
    let origins = vec![
        origin(b + 4, &[], true, false),
        origin(b + 3, &[0], true, true),
        origin(b + 3, &[], true, false),
        origin(b + 3, &[], true, true),
    ];
    (values, frame, selected, origins)
}
#[test]
fn guarded_input_attachment_retains_hidden_owners_and_never_promotes_partial_evidence() {
    for partial in 0..5 {
        let (mut values, frame, mut selected, origins) = guarded_fixture();
        let b = frame.base();
        let downgrade = |v: &mut V| {
            let V::Reference { evidence, .. } = v else {
                unreachable!()
            };
            evidence.origins = Origins::Partial(vec![]);
        };
        match partial {
            1 => downgrade(&mut selected),
            2 => downgrade(values[b + 4].as_mut().unwrap()),
            3 => downgrade(values[b + 3].as_mut().unwrap()),
            4 => {
                let V::Construct { fields, .. } = values[b + 2].as_mut().unwrap() else {
                    unreachable!()
                };
                downgrade(&mut fields[1]);
            }
            _ => {}
        }
        let before = format!("{:?}", &*values);
        let attached = attach_guarded_input(&values, selected, &origins, b + 4, b + 3, &[0]);
        assert_eq!(before, format!("{:?}", &*values));
        let result = &evidence(&attached).origins;
        assert_eq!(result.is_complete(), partial == 0, "{result:?}");
        let (Origins::Complete(entries) | Origins::Partial(entries)) = result else {
            panic!("{result:?}")
        };
        assert!(
            entries
                .iter()
                .any(|o| values.require_storage(&o.place.storage).unwrap() == b + 1)
        );
        assert!(entries.iter().any(
            |o| values.require_storage(&o.place.storage).unwrap() == b + 2
                && o.guard
                && o.opaque
                && o.place.fields.is_empty()
        ));
    }
}
#[test]
fn guarded_input_attachment_rejects_unrelated_missing_and_weakened_proofs_atomically() {
    for mutation in 0..11 {
        let (mut values, frame, mut selected, mut origins) = guarded_fixture();
        let b = frame.base();
        let mut path = &[0][..];
        match mutation {
            0 => {
                let V::Reference { evidence, .. } = values[b + 4].as_mut().unwrap() else {
                    unreachable!()
                };
                evidence.target = None;
            }
            1 => {
                let V::Reference { evidence, .. } = values[b + 3].as_mut().unwrap() else {
                    unreachable!()
                };
                evidence.target = None;
            }
            2 => selected = reference(&values, b + 1, &[], false, "&U32").unwrap(),
            3 => {
                let V::Construct { fields, .. } = values[b + 2].as_mut().unwrap() else {
                    unreachable!()
                };
                fields.swap(0, 1);
            }
            4 => path = &[1],
            5 => {
                origins.pop();
            }
            6 => origins[1].opaque = false,
            8 => {
                let mut foreign = Frames::default();
                let f = foreign.enter(1).unwrap();
                let V::Reference { evidence, .. } = &mut selected else {
                    unreachable!()
                };
                evidence.origins = Origins::Complete(vec![BoundOrigin {
                    place: StoragePlace {
                        storage: foreign.storage_at(f.base()).unwrap(),
                        fields: vec![],
                    },
                    guard: false,
                    opaque: false,
                }]);
            }
            9 => {
                let V::Reference { evidence, .. } = &mut selected else {
                    unreachable!()
                };
                evidence.target = None;
            }
            10 => {
                let V::Construct { fields, .. } = values[b + 2].as_mut().unwrap() else {
                    unreachable!()
                };
                let V::Reference { ty, .. } = &mut fields[0] else {
                    unreachable!()
                };
                *ty = "&pair".into();
            }
            _ => {
                let V::Reference { ty, .. } = values[b + 3].as_mut().unwrap() else {
                    unreachable!()
                };
                *ty = "&view".into();
            }
        }
        let before = format!("{:?}", &*values);
        let attached = attach_guarded_input(&values, selected, &origins, b + 4, b + 3, path);
        assert!(
            matches!(evidence(&attached).origins, Origins::Unavailable(_)),
            "{mutation}: {:?}",
            evidence(&attached)
        );
        assert_eq!(before, format!("{:?}", &*values));
    }
}

fn carrier_fixture() -> (Frames, super::super::Frame, V, Vec<Origin>, Vec<InputLeaf>) {
    let mut values = Frames::default();
    let f = values.enter(6).unwrap();
    let b = f.base();
    values[b] = Some(V::U32(1));
    values[b + 1] = Some(V::U32(2));
    let formed = |values: &Frames, root, ty| {
        let mut v = reference(values, root, &[], false, ty).unwrap();
        let V::Reference { evidence, .. } = &mut v else {
            unreachable!()
        };
        evidence.origins = Origins::Complete(vec![]);
        v
    };
    let left = formed(&values, b, "&U32");
    let right = formed(&values, b + 1, "&U32");
    values[b + 2] = Some(record(
        "envelope",
        vec![record("view", vec![left.clone()]), right.clone()],
    ));
    values[b + 3] = Some(formed(&values, b + 2, "&envelope"));
    values[b + 4] = Some(left);
    values[b + 5] = Some(right);
    let selected = reference(&values, b + 3, &[0], true, "&view").unwrap();
    let origins = vec![
        origin(b + 3, &[], true, false),
        origin(b + 3, &[0], true, false),
        origin(b + 4, &[], true, false),
        origin(b + 5, &[], true, false),
    ];
    let inputs = vec![
        InputLeaf {
            slot: b + 4,
            fields: &[0, 0],
        },
        InputLeaf {
            slot: b + 5,
            fields: &[1],
        },
    ];
    (values, f, selected, origins, inputs)
}
#[test]
fn carrier_input_attachment_keeps_broad_owners_and_partial_caller_evidence() {
    for partial in 0..5 {
        let (mut values, f, mut selected, origins, inputs) = carrier_fixture();
        let b = f.base();
        let downgrade = |v: &mut V| {
            let V::Reference { evidence, .. } = v else {
                unreachable!()
            };
            evidence.origins = Origins::Partial(vec![]);
        };
        match partial {
            1 => downgrade(&mut selected),
            2 => downgrade(values[b + 3].as_mut().unwrap()),
            3 => downgrade(values[b + 5].as_mut().unwrap()),
            4 => {
                let V::Construct { fields, .. } = values[b + 2].as_mut().unwrap() else {
                    unreachable!()
                };
                downgrade(&mut fields[1]);
            }
            _ => {}
        }
        let before = format!("{:?}", &*values);
        let value = attach_carrier_input(&values, selected, &origins, b + 3, &inputs);
        assert_eq!(before, format!("{:?}", &*values));
        let result = &evidence(&value).origins;
        assert_eq!(result.is_complete(), partial == 0, "{result:?}");
        let (Origins::Complete(entries) | Origins::Partial(entries)) = result else {
            panic!("{result:?}")
        };
        assert!(
            entries
                .iter()
                .any(|o| values.require_storage(&o.place.storage).unwrap() == b + 1)
        );
        assert!(entries.iter().any(
            |o| values.require_storage(&o.place.storage).unwrap() == b + 2
                && o.place.fields.is_empty()
        ));
    }
}
#[test]
fn carrier_input_attachment_rejects_missing_changed_and_duplicate_fields_without_effects() {
    for mutation in 0..8 {
        let (mut values, f, mut selected, mut origins, mut inputs) = carrier_fixture();
        let b = f.base();
        match mutation {
            0 => {
                inputs.pop();
            }
            1 => inputs[1].fields = &[0, 0],
            2 => inputs[1].slot = b + 4,
            3 => {
                let V::Construct { fields, .. } = values[b + 2].as_mut().unwrap() else {
                    unreachable!()
                };
                fields[1] = V::Unit;
            }
            4 => {
                let V::Reference { evidence, .. } = values[b + 5].as_mut().unwrap() else {
                    unreachable!()
                };
                evidence.target = None;
            }
            5 => {
                origins.remove(0);
            }
            6 => {
                let V::Reference { ty, .. } = &mut selected else {
                    unreachable!()
                };
                *ty = "&mutable view".into();
            }
            _ => {
                let V::Reference { evidence, .. } = &mut selected else {
                    unreachable!()
                };
                evidence.target = None;
            }
        };
        let before = format!("{:?}", &*values);
        let value = attach_carrier_input(&values, selected, &origins, b + 3, &inputs);
        assert!(
            matches!(evidence(&value).origins, Origins::Unavailable(_)),
            "{mutation}: {:?}",
            evidence(&value)
        );
        assert_eq!(before, format!("{:?}", &*values));
    }
}
