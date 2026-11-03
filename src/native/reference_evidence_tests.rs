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
