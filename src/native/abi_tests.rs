use super::*;

fn context(host: &mut Host) -> Context<'_> {
    Context {
        selected_frame: None,
        selected_activation: None,
        program: None,
        resume_at: None,
        request: None,
        physical_arena: None,
        host,
        storage: FrameStorage::Owned(Frames::default()),
        frame: None,
        destination: None,
        values: vec![],
        outcomes: vec![],
        value_base: 0,
        outcome_base: 0,
        error: None,
        result: None,
        lexical: vec![],
        next_lexical: 0,
    }
}

// A carrier, its borrowed holder and the original private leaf are separate
// locals, exactly as in checked input materialization.
fn carrier_locals(c: &mut Context<'_>, complete: bool) {
    unsafe {
        tellur_rt_v1_frame(c, 4);
        let owner = c.put(V::String("owner".into()));
        tellur_rt_v1_store(c, 0, std::ptr::null(), 0, owner);
        let leaf = tellur_rt_v1_reference(
            c,
            0,
            std::ptr::null(),
            0,
            0,
            b"&mutable String".as_ptr(),
            15,
        );
        let leaf = if complete {
            annotate(c, leaf, &[1, 1, 0, 0, 0])
        } else {
            leaf
        };
        let reference = c.take(leaf).unwrap();
        let carrier = c.put(V::Construct {
            ty: "Editor".into(),
            name: "Editor".into(),
            fields: vec![reference.clone()],
        });
        tellur_rt_v1_store(c, 1, std::ptr::null(), 0, carrier);
        let holder = tellur_rt_v1_reference(
            c,
            1,
            std::ptr::null(),
            0,
            0,
            b"&mutable Editor".as_ptr(),
            15,
        );
        let holder = if complete {
            annotate(c, holder, &[1, 1, 1, 0, 0])
        } else {
            holder
        };
        tellur_rt_v1_store(c, 2, std::ptr::null(), 0, holder);
        let leaf = c.put(reference);
        tellur_rt_v1_store(c, 3, std::ptr::null(), 0, leaf);
    }
}
const GUARDED: &[usize] = &[3, 0, 13, 3, 1, 0, 2, 7, 1, 0, 2, 1, 0, 2, 7, 0, 3, 2, 1, 0];
const CARRIER: &[usize] = &[4, 0, 6, 2, 1, 0, 3, 1, 0, 2, 1, 3, 1, 0];
fn origins(value: &V) -> &execution::reference_evidence::Origins {
    let V::Reference { evidence, .. } = value else {
        panic!("expected reference")
    };
    &evidence.origins
}
#[test]
fn guarded_and_carrier_abi_preserve_runtime_evidence_and_do_not_promote_partial_inputs() {
    use execution::reference_evidence::{
        InputLeaf, Origin, attach_carrier_input, attach_guarded_input,
    };
    for complete in [false, true] {
        let mut host = Host::new(json!({}), None, true, Some(0));
        let mut c = context(&mut host);
        carrier_locals(&mut c, complete);
        for (slot, words) in [(3, GUARDED), (2, CARRIER)] {
            let handle = unsafe { tellur_rt_v1_local(&mut c, slot, std::ptr::null(), 0, 0) };
            let value = c.get(handle).unwrap().clone();
            let origin = |root, fields, guard| Origin {
                root,
                fields,
                indirect: true,
                guard,
                opaque: guard,
            };
            let expected = if slot == 3 {
                attach_guarded_input(
                    c.storage.values(),
                    value,
                    &[
                        origin(3, &[], false),
                        origin(2, &[0], true),
                        origin(2, &[], false),
                        origin(2, &[], true),
                    ],
                    3,
                    2,
                    &[0],
                )
            } else {
                attach_carrier_input(
                    c.storage.values(),
                    value,
                    &[origin(2, &[], false), origin(3, &[], false)],
                    2,
                    &[InputLeaf {
                        slot: 3,
                        fields: &[0],
                    }],
                )
            };
            let result = unsafe { annotate(&mut c, handle, words) };
            assert!(c.get(handle).is_err());
            assert_eq!(c.get(result).unwrap(), &expected);
            assert_eq!(origins(c.get(result).unwrap()).is_complete(), complete);
        }
        assert!(c.error.is_none());
        assert_eq!(c.host.fuel, Some(0));
    }
}
#[test]
fn carrier_metadata_failures_are_unavailable_and_do_not_change_execution() {
    use execution::reference_evidence::Origins;
    // Structurally complete but missing actual leaf, repeated leaf, wrong path,
    // or incomplete holder guard: these are metadata failures, not ABI faults.
    for words in [
        &[4, 0, 3, 2, 1, 0, 2, 0][..],
        &[4, 0, 6, 2, 1, 0, 3, 1, 0, 2, 2, 3, 1, 0, 3, 1, 0],
        &[4, 0, 6, 2, 1, 0, 3, 1, 0, 2, 1, 3, 1, 9],
        &[3, 0, 3, 3, 1, 0, 3, 2, 1, 0],
    ] {
        let mut host = Host::new(json!({}), None, true, None);
        let mut c = context(&mut host);
        carrier_locals(&mut c, true);
        let slot = if words[0] == 3 { 3 } else { 2 };
        let handle = unsafe { tellur_rt_v1_local(&mut c, slot, std::ptr::null(), 0, 0) };
        let result = unsafe { annotate(&mut c, handle, words) };
        assert!(
            matches!(origins(c.get(result).unwrap()), Origins::Unavailable(_)),
            "{words:?}"
        );
        assert!(c.error.is_none());
        assert_eq!(
            read_place(c.storage.values(), 3, &[], true).unwrap(),
            V::String("owner".into())
        );
    }
}
#[test]
fn carrier_child_results_retain_caller_identities_and_reject_retired_storage() {
    unsafe extern "C" fn child(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 2);
            tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_store(ctx, 1, std::ptr::null(), 0, *args.add(1));
            let value = tellur_rt_v1_reference(
                ctx,
                0,
                std::ptr::null(),
                0,
                1,
                b"&mutable Editor".as_ptr(),
                15,
            );
            let value = annotate(ctx, value, &[4, 0, 6, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0]);
            tellur_rt_v1_complete(ctx, b"ready".as_ptr(), 5, [value].as_ptr(), 1);
        }
    }
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    carrier_locals(&mut c, true);
    unsafe {
        let holder = tellur_rt_v1_local(&mut c, 2, std::ptr::null(), 0, 0);
        let leaf = tellur_rt_v1_local(&mut c, 3, std::ptr::null(), 0, 0);
        let result = tellur_rt_v1_call(
            &mut c,
            child,
            b"child".as_ptr(),
            5,
            [holder, leaf].as_ptr(),
            2,
        );
        let mut payload = [0];
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                result,
                b"ready".as_ptr(),
                5,
                payload.as_mut_ptr(),
                1
            ),
            1
        );
        let value = c.get(payload[0]).unwrap().clone();
        let execution::reference_evidence::Origins::Complete(bound) = origins(&value) else {
            panic!("{value:?}")
        };
        assert!(!bound.is_empty());
        assert!(
            bound
                .iter()
                .all(|o| c.storage.values().require_storage(&o.place.storage).is_ok())
        );
        assert_eq!(c.storage.values().len(), 4);
        let frame = c.frame.take().unwrap();
        c.storage.values_mut().retire(&frame).unwrap();
        carrier_locals(&mut c, true);
        assert!(bound.iter().all(|o| {
            c.storage
                .values()
                .require_storage(&o.place.storage)
                .is_err()
        }));
        let stale = c.put(value);
        let stale = annotate(&mut c, stale, CARRIER);
        assert!(matches!(
            origins(c.get(stale).unwrap()),
            execution::reference_evidence::Origins::Unavailable(_)
        ));
        assert!(c.error.is_none());
    }
}

#[test]
fn reference_reborrows_return_from_child_frames_and_mutate_live_caller_storage() {
    unsafe extern "C" fn reborrow(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 1);
            tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, *args);
            let result = tellur_rt_v1_reference(
                ctx,
                0,
                std::ptr::null(),
                0,
                1,
                b"&mutable String".as_ptr(),
                15,
            );
            let result = annotate(ctx, result, &[1, 0, 0, 1, 0]);
            tellur_rt_v1_complete(ctx, b"ready".as_ptr(), 5, [result].as_ptr(), 1);
        }
    }
    let mut host = Host::new(record_manifest(), None, true, Some(0));
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let owner = c.put(pair());
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
        let parent_identity = c.storage.values().storage_at(c.local(0).unwrap()).unwrap();
        let reference = tellur_rt_v1_reference(
            &mut c,
            0,
            [0].as_ptr(),
            1,
            0,
            b"&mutable String".as_ptr(),
            15,
        );
        let reference = annotate(&mut c, reference, &[1, 1, 0, 0, 1, 0]);
        let result = tellur_rt_v1_call(
            &mut c,
            reborrow,
            b"reborrow".as_ptr(),
            8,
            [reference].as_ptr(),
            1,
        );
        let mut payload = [0];
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                result,
                b"ready".as_ptr(),
                5,
                payload.as_mut_ptr(),
                1
            ),
            1
        );
        assert_eq!(c.storage.values().len(), 2);
        let V::Reference {
            evidence,
            root,
            fields,
            ..
        } = c.get(payload[0]).unwrap()
        else {
            panic!()
        };
        assert_eq!(*root, 0);
        assert_eq!(fields, &[0]);
        assert_eq!(evidence.target.as_ref().unwrap().storage, parent_identity);
        assert!(evidence.origins.is_complete());
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, payload[0]);
        let changed = c.put(V::String("changed through returned borrow".into()));
        tellur_rt_v1_assign_through(&mut c, 1, std::ptr::null(), 0, changed);
        let text = tellur_rt_v1_deref(&mut c, 1, std::ptr::null(), 0, 0);
        assert_eq!(
            c.take(text).unwrap(),
            V::String("changed through returned borrow".into())
        );
        assert_eq!(
            read_place(c.storage.values(), 0, &[0], false).unwrap(),
            V::String("changed through returned borrow".into())
        );
        assert_eq!(c.host.fuel, Some(0));
        assert!(c.error.is_none());
    }
}

#[test]
fn reference_metadata_never_promotes_partial_incoming_or_fails_source_evaluation() {
    use execution::reference_evidence::Origins;
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let owner = c.put(V::U32(12));
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
        let plain = tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, b"&U32".as_ptr(), 4);
        let partial = annotate(&mut c, plain, &[1, 0, 0, 0, 0]);
        let partial = annotate(&mut c, partial, &[2, 0]);
        let V::Reference { evidence, .. } = c.get(partial).unwrap() else {
            panic!()
        };
        assert!(!evidence.origins.is_complete());
        // A structurally valid origin referring through an unavailable local is
        // metadata-unavailable, not a new source error or loss of the operand.
        let unavailable = annotate(&mut c, partial, &[1, 0, 1, 1, 0]);
        let V::Reference { evidence, root, .. } = c.get(unavailable).unwrap() else {
            panic!()
        };
        assert!(matches!(evidence.origins, Origins::Unavailable(_)));
        assert_eq!(*root, 0);
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, unavailable);
        let value = tellur_rt_v1_deref(&mut c, 1, std::ptr::null(), 0, 0);
        assert_eq!(c.take(value).unwrap(), V::U32(12));
        assert!(c.error.is_none());
    }
}

#[test]
fn malformed_reference_descriptors_and_indirect_destinations_preserve_arguments() {
    const DESCRIPTORS: &[&[usize]] = &[
        &[],
        &[1],
        &[9, 0],
        &[2, 1],
        &[2, 0, 0],
        &[1, 0, 0],
        &[1, 0, 0, 8, 0],
        &[1, 0, 0, 0, 1],
        &[1, 0, 9, 0, 0],
        &[3, 1, 0],
        &[3, 0],
        &[3, 0, usize::MAX],
        &[3, 0, 0],
        &[3, 0, 0, 0, 0, 2, 0],
        &[3, 0, 0, 0, 9, 0],
        &[4, 0, 0],
        &[4, 0, 0, 0],
        &[4, 0, 0, 0, usize::MAX],
        &[4, 0, 0, 0, 1, 0],
        &[4, 0, 0, 0, 1, 0, usize::MAX],
        &[4, 0, 0, 0, 0, 0, 0],
        &[4, 0, 0, 9, 0],
        &[4, 0, 0, 0, 1, 9, 0],
    ];
    for descriptor in DESCRIPTORS {
        let mut host = Host::new(json!({}), None, true, None);
        let mut c = context(&mut host);
        unsafe {
            tellur_rt_v1_frame(&mut c, 1);
            let reference =
                tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, b"&U32".as_ptr(), 4);
            assert_eq!(
                tellur_rt_v1_evidence(&mut c, reference, descriptor.as_ptr(), descriptor.len()),
                0,
                "{descriptor:?}"
            );
            assert!(c.get(reference).is_ok());
        }
    }
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let owner = c.put(V::U32(12));
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
        let reference = tellur_rt_v1_reference(
            &mut c,
            0,
            std::ptr::null(),
            0,
            0,
            b"&mutable U32".as_ptr(),
            12,
        );
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, reference);
        let retained = c.put(V::U32(99));
        tellur_rt_v1_assign_through(&mut c, 1, [0].as_ptr(), 1, retained);
        assert!(c.error.as_ref().unwrap().contains("projection"));
        assert_eq!(c.get(retained).unwrap(), &V::U32(99));
        assert_eq!(c.storage.values()[0], Some(V::U32(12)));
    }
}

#[test]
fn borrowed_decisions_keep_payloads_in_owner_storage() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let owned = V::Construct {
            ty: "Option<String>".into(),
            name: "Some".into(),
            fields: vec![V::String("owned".into())],
        };
        let value = c.put(owned.clone());
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, value);
        let reference = tellur_rt_v1_reference(
            &mut c,
            0,
            std::ptr::null(),
            0,
            0,
            b"&Option<String>".as_ptr(),
            15,
        );
        let reference = annotate(&mut c, reference, &[1, 1, 0, 0, 0]);
        let outcome = tellur_rt_v1_borrowed_variant(&mut c, reference);
        let mut fields = [0];
        assert_eq!(
            tellur_rt_v1_route(&mut c, outcome, b"Some".as_ptr(), 4, fields.as_mut_ptr(), 1),
            1
        );
        assert!(
            matches!(c.get(fields[0]).unwrap(), V::Reference {root:0,fields,..} if fields==&[0])
        );
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, fields[0]);
        let text = tellur_rt_v1_deref(&mut c, 1, std::ptr::null(), 0, 0);
        assert_eq!(c.take(text).unwrap(), V::String("owned".into()));
        assert_eq!(c.storage.values()[0], Some(owned));
        assert!(c.error.is_none());
    }
}

#[test]
fn stable_locals_survive_replacement_moves_and_partial_reinitialization() {
    let mut host = Host::new(record_manifest(), None, true, Some(0));
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let root = c.local(0).unwrap();
        let identity = c.storage.values().storage_at(root).unwrap();
        let original = c.put(pair());
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, original);
        let copied = tellur_rt_v1_local(&mut c, 0, [1].as_ptr(), 1, 0);
        assert_eq!(c.take(copied).unwrap(), V::U32(7));
        let moved = tellur_rt_v1_local(&mut c, 0, [0].as_ptr(), 1, 1);
        assert_eq!(c.get(moved).unwrap(), &V::String("original".into()));
        assert_eq!(
            read_place(c.storage.values(), root, &[0], false).unwrap(),
            V::Unit
        );
        tellur_rt_v1_store(&mut c, 0, [0].as_ptr(), 1, moved);
        let whole = tellur_rt_v1_local(&mut c, 0, std::ptr::null(), 0, 1);
        assert!(c.storage.values()[root].is_none());
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, whole);
        let replacement = c.put(V::U32(42));
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, replacement);
        let next = c.put(V::U32(93));
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, next);
        assert_eq!(c.storage.values().require_storage(&identity).unwrap(), root);
        assert_eq!(
            read_place(c.storage.values(), root, &[], false).unwrap(),
            V::U32(93)
        );
        assert_eq!(
            read_place(c.storage.values(), c.local(1).unwrap(), &[], false).unwrap(),
            pair()
        );
        assert!(
            [original, moved, whole, replacement, next]
                .iter()
                .all(|h| c.get(*h).is_err())
        );
        tellur_rt_v1_release_local(&mut c, 0);
        tellur_rt_v1_release_local(&mut c, 0);
        assert!(c.storage.values()[root].is_none());
        assert_eq!(c.host.fuel, Some(0), "storage adds no source fuel ticks");
        assert_eq!(tellur_rt_v1_ok(&mut c), 1);
    }
}

#[test]
fn local_admission_and_transfers_fail_before_mutating_either_owner() {
    for case in [
        "missing",
        "duplicate",
        "slot",
        "field",
        "stale",
        "mode",
        "release",
    ] {
        let mut host = Host::new(record_manifest(), None, true, None);
        let mut c = context(&mut host);
        let held = c.put(V::String("retained".into()));
        unsafe {
            if case != "missing" {
                tellur_rt_v1_frame(&mut c, 1);
                let initial = c.put(pair());
                tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, initial);
            }
            match case {
                "missing" => tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, held),
                "duplicate" => tellur_rt_v1_frame(&mut c, 5),
                "slot" => tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, held),
                "field" => tellur_rt_v1_store(&mut c, 0, [9].as_ptr(), 1, held),
                "stale" => tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, 0),
                "mode" => {
                    assert_eq!(tellur_rt_v1_local(&mut c, 0, std::ptr::null(), 0, 2), 0);
                }
                "release" => tellur_rt_v1_release_local(&mut c, usize::MAX),
                _ => unreachable!(),
            }
            assert!(c.error.is_some(), "{case}");
            let error = c.error.clone();
            tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, held);
            tellur_rt_v1_release_local(&mut c, 0);
            tellur_rt_v1_emit(&mut c, held);
            assert_eq!(c.error, error);
            assert_eq!(c.get(held).unwrap(), &V::String("retained".into()));
            assert!(c.host.events.is_empty());
            if case != "missing" {
                assert_eq!(c.storage.values().len(), 1);
                assert_eq!(c.storage.values()[0], Some(pair()));
            }
        }
    }
}

#[test]
fn child_frames_retire_on_completion_fault_and_fallthrough_without_touching_parent() {
    unsafe extern "C" fn child(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 3);
            let c = &mut *ctx;
            assert_eq!(c.local(0).unwrap(), 2);
            assert_eq!(c.storage.values()[0], Some(V::U32(99)));
            let mode = match c.get(*args).unwrap() {
                V::U32(n) => *n,
                _ => panic!(),
            };
            tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, *args);
            let value = tellur_rt_v1_value(ctx, 3, 0, b"child".as_ptr(), 5);
            tellur_rt_v1_store(ctx, 2, std::ptr::null(), 0, value);
            match mode {
                0 => {
                    let moved = tellur_rt_v1_local(ctx, 2, std::ptr::null(), 0, 1);
                    tellur_rt_v1_complete(ctx, b"ready".as_ptr(), 5, [moved].as_ptr(), 1);
                }
                1 => {
                    let msg = tellur_rt_v1_local(ctx, 2, std::ptr::null(), 0, 1);
                    tellur_rt_v1_fault(ctx, msg);
                }
                2 => {}
                _ => unreachable!(),
            }
        }
    }
    for mode in 0..3 {
        let mut host = Host::new(json!({}), None, true, None);
        host.enter("root").unwrap();
        let mut c = context(&mut host);
        unsafe {
            tellur_rt_v1_frame(&mut c, 2);
            let parent = c.put(V::U32(99));
            tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, parent);
            let arg = c.put(V::U32(mode));
            let outcome = tellur_rt_v1_call(&mut c, child, b"child".as_ptr(), 5, [arg].as_ptr(), 1);
            assert_eq!(c.storage.values().len(), 2, "{mode}");
            assert_eq!(c.storage.values()[0], Some(V::U32(99)));
            if mode == 0 {
                assert_eq!(
                    c.outcome(outcome).unwrap(),
                    &("ready".into(), vec![V::String("child".into())])
                );
                // The next invocation can reuse retired offsets while parent storage stays live.
                let arg = c.put(V::U32(0));
                assert_ne!(
                    tellur_rt_v1_call(&mut c, child, b"child".as_ptr(), 5, [arg].as_ptr(), 1),
                    0
                );
                assert_eq!(c.storage.values().len(), 2);
            } else {
                assert_eq!(outcome, 0);
                let error = c.error.as_ref().unwrap();
                assert!(error.contains(if mode == 1 {
                    "E_FAULT: child"
                } else {
                    "fell through"
                }));
                let report = c
                    .host
                    .activations
                    .failure("root", fault::Phase::Execution, error);
                assert_eq!(
                    report
                        .activations
                        .iter()
                        .map(|a| a.control.as_str())
                        .collect::<Vec<_>>(),
                    ["root", "child"]
                );
            }
        }
    }
}

#[test]
fn retired_frame_identities_cannot_alias_reused_offsets_and_empty_frames_finish() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 1) };
    let old = c.storage.values().storage_at(c.local(0).unwrap()).unwrap();
    c.result = Some(Completion::Outcome(("done".into(), vec![])));
    c.finish().unwrap();
    assert!(c.storage.values().is_empty());
    unsafe { tellur_rt_v1_frame(&mut c, 1) };
    assert_eq!(c.local(0).unwrap(), 0);
    assert!(c.storage.values().require_storage(&old).is_err());
    assert!(c.finish().is_err());
    assert!(c.storage.values().is_empty());
    unsafe { tellur_rt_v1_frame(&mut c, 0) };
    c.error = Some("E_FAULT: original".into());
    assert_eq!(c.finish().err().unwrap(), "E_FAULT: original");
    assert!(c.frame.is_none() && c.storage.values().is_empty());
}

// The ABI requires immutable module-lifetime metadata, including in tests.
unsafe fn annotate(ctx: *mut Context<'_>, value: u64, words: &'static [usize]) -> u64 {
    unsafe { tellur_rt_v1_evidence(ctx, value, words.as_ptr(), words.len()) }
}

unsafe extern "C" fn echo(ctx: *mut Context<'_>, args: *const u64, count: usize) {
    unsafe { tellur_rt_v1_complete(ctx, b"ready".as_ptr(), 5, args, count) };
}

unsafe extern "C" fn forward(ctx: *mut Context<'_>, args: *const u64, count: usize) {
    unsafe {
        let outcome = tellur_rt_v1_call(ctx, echo, b"echo".as_ptr(), 4, args, count);
        let mut fields = vec![0; count];
        if tellur_rt_v1_route(
            ctx,
            outcome,
            b"ready".as_ptr(),
            5,
            fields.as_mut_ptr(),
            count,
        ) == 1
        {
            tellur_rt_v1_complete(ctx, b"done".as_ptr(), 4, fields.as_ptr(), count);
        }
    }
}

#[test]
fn nested_callbacks_transfer_scalar_payloads_without_replacing_the_parent_completion() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    let retained = c.put(V::String("caller local".into()));
    let arguments = [
        c.put(V::String("😀\0".into())),
        c.put(V::Bytes(vec![0, 255])),
    ];
    unsafe {
        let outcome = tellur_rt_v1_call(
            &mut c,
            forward,
            b"forward".as_ptr(),
            7,
            arguments.as_ptr(),
            arguments.len(),
        );
        assert_eq!(tellur_rt_v1_ok(&mut c), 1);
        assert!(c.result.is_none());
        assert!(arguments.iter().all(|h| c.get(*h).is_err()));
        assert_eq!(c.get(retained).unwrap(), &V::String("caller local".into()));
        let mut fields = [0; 2];
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                outcome,
                b"other".as_ptr(),
                5,
                fields.as_mut_ptr(),
                2
            ),
            0
        );
        assert!(c.outcome(outcome).is_ok());
        assert_eq!(fields, [0, 0]);
        assert_eq!(
            tellur_rt_v1_route(&mut c, outcome, b"done".as_ptr(), 4, fields.as_mut_ptr(), 2),
            1
        );
        assert_eq!(c.get(fields[0]).unwrap(), &V::String("😀\0".into()));
        assert_eq!(c.get(fields[1]).unwrap(), &V::Bytes(vec![0, 255]));
        assert!(c.outcome(outcome).is_err());
        let next = tellur_rt_v1_call(&mut c, echo, b"echo".as_ptr(), 4, std::ptr::null(), 0);
        assert!(next > outcome);
        assert_eq!(
            tellur_rt_v1_route(&mut c, next, b"ready".as_ptr(), 5, std::ptr::null_mut(), 0),
            1
        );
        tellur_rt_v1_complete(&mut c, b"finished".as_ptr(), 8, fields.as_ptr(), 2);
        assert!(c.result.is_some());
        assert_eq!(
            tellur_rt_v1_route(&mut c, outcome, b"done".as_ptr(), 4, fields.as_mut_ptr(), 2),
            0
        );
        assert!(c.error.as_ref().unwrap().contains("consumed ABI outcome"));
    }
}

#[test]
fn call_and_outcome_shape_failures_do_not_publish_partial_transfers() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    let h = c.put(V::String("still owned".into()));
    unsafe {
        assert_eq!(
            tellur_rt_v1_call(&mut c, echo, b"echo".as_ptr(), 4, [h, h].as_ptr(), 2),
            0
        );
    }
    assert!(c.get(h).is_ok());
    assert!(c.outcomes.is_empty());
    assert!(c.error.unwrap().contains("duplicate ABI value transfer"));

    let mut c = context(&mut host);
    c.outcomes
        .push(Some(("ready".into(), vec![V::U32(7), V::Bool(true)])));
    let mut output = [99];
    unsafe {
        assert_eq!(
            tellur_rt_v1_route(&mut c, 1, b"ready".as_ptr(), 5, output.as_mut_ptr(), 1),
            0
        );
    }
    assert_eq!(output, [99]);
    assert!(c.values.is_empty());
    assert_eq!(c.outcome(1).unwrap().1.len(), 2);
    assert!(c.error.unwrap().contains("payload count mismatch"));
}

#[test]
fn failed_callbacks_keep_the_nested_activation_and_stop_the_caller() {
    unsafe extern "C" fn fail(ctx: *mut Context<'_>, _: *const u64, _: usize) {
        unsafe {
            let message = tellur_rt_v1_value(ctx, 3, 0, b"stopped".as_ptr(), 7);
            tellur_rt_v1_fault(ctx, message);
        }
    }
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("root").unwrap();
    let mut c = context(&mut host);
    let later = c.put(V::String("unreachable".into()));
    unsafe {
        assert_eq!(
            tellur_rt_v1_call(&mut c, fail, b"child".as_ptr(), 5, std::ptr::null(), 0),
            0
        );
        tellur_rt_v1_emit(&mut c, later);
    }
    let error = c.error.clone().unwrap();
    assert_eq!(error, "E_FAULT: stopped");
    assert!(c.host.events.is_empty());
    assert!(c.outcomes.is_empty());
    let report = c
        .host
        .activations
        .failure("root", fault::Phase::Execution, &error);
    assert_eq!(
        report
            .activations
            .iter()
            .map(|a| a.control.as_str())
            .collect::<Vec<_>>(),
        ["root", "child"]
    );
}

#[test]
fn value_transfers_validate_the_whole_batch_before_consumption() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    let first = c.put(V::String("owner".into()));
    let second = c.put(V::U32(42));
    assert!(
        c.take_many(&[first, first])
            .unwrap_err()
            .contains("duplicate")
    );
    assert!(c.take_many(&[first, 0]).unwrap_err().contains("invalid"));
    assert!(
        c.take_many(&[first, u64::MAX])
            .unwrap_err()
            .contains("invalid")
    );
    assert_eq!(c.get(first).unwrap(), &V::String("owner".into()));
    assert_eq!(
        c.take_many(&[second, first]).unwrap(),
        vec![V::U32(42), V::String("owner".into())]
    );
    assert!(c.take(first).is_err());
    let next = c.put(V::Bool(true));
    assert!(next > second);
    assert!(
        c.get(first).is_err(),
        "consumed handles cannot alias a later value"
    );
}

#[test]
fn ffi_failure_is_sticky_and_blocks_later_effects_and_completion() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    let message = c.put(V::String("must not emit".into()));
    unsafe {
        tellur_rt_v1_drop(&mut c, 0);
        let original = c.error.clone();
        assert_eq!(tellur_rt_v1_copy(&mut c, message), 0);
        tellur_rt_v1_emit(&mut c, message);
        tellur_rt_v1_complete(&mut c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        assert_eq!(tellur_rt_v1_ok(&mut c), 0);
        assert_eq!(c.error, original);
        assert!(c.result.is_none());
        assert!(c.host.events.is_empty());
        assert!(c.get(message).is_ok());
    }
}

#[test]
fn ffi_values_preserve_unicode_nuls_bytes_and_reject_invalid_scalars() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    unsafe {
        let text = "😀\0quoted \"";
        let h = tellur_rt_v1_value(&mut c, 3, 0, text.as_ptr(), text.len());
        assert_eq!(c.take(h).unwrap(), V::String(text.into()));
        let bytes = [0, 255, 128];
        let h = tellur_rt_v1_value(&mut c, 4, 0, bytes.as_ptr(), bytes.len());
        assert_eq!(c.take(h).unwrap(), V::Bytes(bytes.to_vec()));
        tellur_rt_v1_release(&mut c, 0);
        assert_eq!(tellur_rt_v1_ok(&mut c), 1);
        assert_eq!(
            tellur_rt_v1_value(&mut c, 2, u64::MAX, std::ptr::null(), 0),
            0
        );
        assert!(c.error.as_ref().unwrap().contains("invalid ABI scalar"));
    }
}

fn record_manifest() -> Json {
    json!({"types":[
        {"kind":"record","name":"pair","fields":[{"name":"text","type":"String"},{"name":"n","type":"U32"}]},
        {"kind":"record","name":"nested","fields":[{"name":"inner","type":"pair"}]},
        {"kind":"record","name":"empty","fields":[]}
    ]})
}
fn pair() -> V {
    V::Construct {
        ty: "pair".into(),
        name: "pair".into(),
        fields: vec![V::String("original".into()), V::U32(7)],
    }
}

#[test]
fn records_transfer_fields_without_cloning_and_reinitialize_partial_moves() {
    let mut host = Host::new(record_manifest(), None, true, None);
    let mut c = context(&mut host);
    let text = String::from("owned 😀\0text");
    let allocation = text.as_ptr();
    let args = [c.put(V::String(text)), c.put(V::U32(9))];
    unsafe {
        let pair = tellur_rt_v1_record(&mut c, b"pair".as_ptr(), 4, args.as_ptr(), 2);
        let root = tellur_rt_v1_record(&mut c, b"nested".as_ptr(), 6, [pair].as_ptr(), 1);
        assert!(args.iter().chain([pair].iter()).all(|h| c.get(*h).is_err()));
        let copy = tellur_rt_v1_field(&mut c, root, [0, 1].as_ptr(), 2, 0);
        assert_eq!(c.take(copy).unwrap(), V::U32(9));
        let moved = tellur_rt_v1_field(&mut c, root, [0, 0].as_ptr(), 2, 1);
        let V::String(text) = c.get(moved).unwrap() else {
            panic!()
        };
        assert_eq!(text.as_ptr(), allocation);
        assert_eq!(project(c.get(root).unwrap(), &[0, 0]).unwrap(), V::Unit);
        tellur_rt_v1_assign(&mut c, root, [0, 0].as_ptr(), 2, moved);
        assert!(c.get(moved).is_err());
        let temporary = tellur_rt_v1_project(&mut c, root, 0);
        let text = tellur_rt_v1_project(&mut c, temporary, 0);
        let V::String(value) = c.get(text).unwrap() else {
            panic!()
        };
        assert_eq!(value.as_ptr(), allocation);
        assert!(c.get(root).is_err() && c.get(temporary).is_err());
        let empty = tellur_rt_v1_record(&mut c, b"empty".as_ptr(), 5, std::ptr::null(), 0);
        tellur_rt_v1_drop(&mut c, empty);
        tellur_rt_v1_drop(&mut c, text);
        assert_eq!(tellur_rt_v1_ok(&mut c), 1);
    }
}

#[test]
fn invalid_record_shapes_do_not_consume_arguments() {
    for case in ["arity", "type", "unknown", "duplicate", "stale", "zero"] {
        let mut host = Host::new(record_manifest(), None, true, None);
        let mut c = context(&mut host);
        let text = c.put(V::String("keep".into()));
        let number = c.put(V::U32(3));
        let stale = c.put(V::U32(0));
        c.take(stale).unwrap();
        let args = match case {
            "arity" => vec![text],
            "type" => vec![number, text],
            "duplicate" => vec![text, text],
            "stale" => vec![text, stale],
            "zero" => vec![text, 0],
            _ => vec![text, number],
        };
        let ty = if case == "unknown" { "absent" } else { "pair" };
        unsafe {
            assert_eq!(
                tellur_rt_v1_record(&mut c, ty.as_ptr(), ty.len(), args.as_ptr(), args.len()),
                0,
                "{case}"
            );
        }
        assert_eq!(c.get(text).unwrap(), &V::String("keep".into()), "{case}");
        assert_eq!(c.get(number).unwrap(), &V::U32(3), "{case}");
        assert!(c.error.is_some(), "{case}");
    }
}

#[test]
fn invalid_field_operations_leave_roots_and_rhs_owned() {
    for case in [
        "read", "move", "assign", "project", "deep", "alias", "stale", "zero", "mode",
    ] {
        let mut host = Host::new(record_manifest(), None, true, None);
        let mut c = context(&mut host);
        let root = c.put(pair());
        let rhs = c.put(V::String("replacement".into()));
        let stale = c.put(V::Unit);
        c.take(stale).unwrap();
        unsafe {
            match case {
                "read" => {
                    tellur_rt_v1_field(&mut c, root, [2].as_ptr(), 1, 0);
                }
                "move" => {
                    tellur_rt_v1_field(&mut c, root, [2].as_ptr(), 1, 1);
                }
                "assign" => tellur_rt_v1_assign(&mut c, root, [2].as_ptr(), 1, rhs),
                "project" => {
                    tellur_rt_v1_project(&mut c, root, 2);
                }
                "deep" => tellur_rt_v1_assign(&mut c, root, [0, 0].as_ptr(), 2, rhs),
                "alias" => tellur_rt_v1_assign(&mut c, root, [0].as_ptr(), 1, root),
                "stale" => tellur_rt_v1_assign(&mut c, root, [0].as_ptr(), 1, stale),
                "zero" => tellur_rt_v1_assign(&mut c, 0, [0].as_ptr(), 1, rhs),
                "mode" => {
                    tellur_rt_v1_field(&mut c, root, [0].as_ptr(), 1, 2);
                }
                _ => unreachable!(),
            }
        }
        assert_eq!(c.get(root).unwrap(), &pair(), "{case}");
        assert_eq!(
            c.get(rhs).unwrap(),
            &V::String("replacement".into()),
            "{case}"
        );
        assert!(c.error.is_some(), "{case}");
    }
}

fn tagged_manifest() -> Json {
    let mut manifest = record_manifest();
    manifest["types"].as_array_mut().unwrap().extend([
        json!({"kind":"variant","name":"choice","constructors":[{"name":"present","fields":[{"name":"value","type":"pair"}]},{"name":"absent","fields":[]}]}),
        json!({"kind":"outcome","name":"answer","constructors":[{"name":"ready","fields":[{"name":"first","type":"String"},{"name":"second","type":"String"}]},{"name":"empty","fields":[]}]})
    ]);
    manifest
}

#[test]
fn tagged_construction_and_decisions_transfer_selected_payloads_once() {
    let mut host = Host::new(tagged_manifest(), None, true, None);
    let mut c = context(&mut host);
    unsafe {
        let owned = c.put(pair());
        let variant = tellur_rt_v1_construct(
            &mut c,
            b"choice".as_ptr(),
            6,
            b"present".as_ptr(),
            7,
            [owned].as_ptr(),
            1,
        );
        let option = tellur_rt_v1_construct(
            &mut c,
            b"Option<choice>".as_ptr(),
            14,
            b"Some".as_ptr(),
            4,
            [variant].as_ptr(),
            1,
        );
        assert!(c.get(owned).is_err() && c.get(variant).is_err());
        let outcome = tellur_rt_v1_variant(&mut c, option);
        assert!(c.get(option).is_err());
        let mut output = [0];
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                outcome,
                b"None".as_ptr(),
                4,
                std::ptr::null_mut(),
                0
            ),
            0
        );
        assert_eq!(
            tellur_rt_v1_route(&mut c, outcome, b"Some".as_ptr(), 4, output.as_mut_ptr(), 1),
            1
        );
        let outcome = tellur_rt_v1_variant(&mut c, output[0]);
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                outcome,
                b"present".as_ptr(),
                7,
                output.as_mut_ptr(),
                1
            ),
            1
        );
        assert_eq!(c.take(output[0]).unwrap(), pair());
        for (ty, name) in [
            ("Option<choice>", "None"),
            ("choice", "absent"),
            ("answer", "empty"),
        ] {
            let value = tellur_rt_v1_construct(
                &mut c,
                ty.as_ptr(),
                ty.len(),
                name.as_ptr(),
                name.len(),
                std::ptr::null(),
                0,
            );
            let outcome = tellur_rt_v1_variant(&mut c, value);
            assert_eq!(
                tellur_rt_v1_route(
                    &mut c,
                    outcome,
                    name.as_ptr(),
                    name.len(),
                    std::ptr::null_mut(),
                    0
                ),
                1
            );
            assert!(c.outcome(outcome).is_err());
        }
        let string = String::from("owning outcome");
        let allocation = string.as_ptr();
        let arguments = [c.put(V::String(string)), c.put(V::String("second".into()))];
        let value = tellur_rt_v1_construct(
            &mut c,
            b"answer".as_ptr(),
            6,
            b"ready".as_ptr(),
            5,
            arguments.as_ptr(),
            2,
        );
        let outcome = tellur_rt_v1_variant(&mut c, value);
        let mut output = [0; 2];
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                outcome,
                b"ready".as_ptr(),
                5,
                output.as_mut_ptr(),
                2
            ),
            1
        );
        let V::String(value) = c.get(output[0]).unwrap() else {
            panic!()
        };
        assert_eq!(value.as_ptr(), allocation);
        assert_eq!(tellur_rt_v1_ok(&mut c), 1);
    }
}

#[test]
fn malformed_tagged_transfers_do_not_consume_values_or_publish_outcomes() {
    for case in ["tag", "arity", "duplicate", "stale", "decision", "shape"] {
        let mut host = Host::new(tagged_manifest(), None, true, None);
        let mut c = context(&mut host);
        let first = c.put(V::String("retained".into()));
        let second = c.put(V::String("other".into()));
        let stale = c.put(V::Unit);
        c.take(stale).unwrap();
        unsafe {
            if case == "decision" || case == "shape" {
                let value = if case == "decision" {
                    V::U32(1)
                } else {
                    V::Construct {
                        ty: "choice".into(),
                        name: "present".into(),
                        fields: vec![],
                    }
                };
                let h = c.put(value.clone());
                assert_eq!(tellur_rt_v1_variant(&mut c, h), 0);
                assert_eq!(c.get(h).unwrap(), &value);
            } else {
                let name = if case == "tag" { "absent" } else { "ready" };
                let args = match case {
                    "arity" => vec![first],
                    "duplicate" => vec![first, first],
                    "stale" => vec![first, stale],
                    _ => vec![first, second],
                };
                assert_eq!(
                    tellur_rt_v1_construct(
                        &mut c,
                        b"answer".as_ptr(),
                        6,
                        name.as_ptr(),
                        name.len(),
                        args.as_ptr(),
                        args.len()
                    ),
                    0
                );
            }
        }
        assert_eq!(
            c.get(first).unwrap(),
            &V::String("retained".into()),
            "{case}"
        );
        assert_eq!(c.get(second).unwrap(), &V::String("other".into()), "{case}");
        assert!(c.outcomes.is_empty());
        assert!(c.error.is_some());
    }
}

fn file_manifest() -> Json {
    let mut m = json!({"types":[{"kind":"opaque","name":"handle","linear":true},{"kind":"opaque","name":"read_error","linear":false},{"kind":"opaque","name":"close_error","linear":false}],"primitives":[]});
    let ty = |t: &str| {
        if ["Handle", "ReadError", "CloseError"].contains(&t) {
            snake_name(t)
        } else {
            t.to_owned()
        }
    };
    for name in ["open", "read", "close"] {
        let (parameters, outcomes, effects) = contract(name).unwrap();
        let parameters = parameters
            .iter()
            .enumerate()
            .map(|(i, (t, mode))| json!({"name":format!("arg{i}"),"type":ty(t),"mode":mode}))
            .collect::<Vec<_>>();
        let outcomes=outcomes.iter().map(|(name,_,fields)|json!({"name":snake_name(name),"fields":fields.iter().enumerate().map(|(i,t)|json!({"name":format!("field{i}"),"type":ty(t)})).collect::<Vec<_>>()})).collect::<Vec<_>>();
        m["primitives"].as_array_mut().unwrap().push(json!({"name":name,"parameters":parameters,"outcomes":outcomes,"effects":effects,"returns":true}));
    }
    m
}
fn abi_file(name: &str) -> std::path::PathBuf {
    let file = std::env::temp_dir().join(format!("tellur-abi-{name}-{}", std::process::id()));
    std::fs::write(&file, b"retained owner\0").unwrap();
    file
}
#[test]
fn live_borrowed_primitive_snapshots_keep_the_original_file_until_consumed() {
    let file = abi_file("borrow-close");
    let mut host = Host::new(file_manifest(), None, true, Some(0));
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 1);
        let path = c.put(V::String(file.to_str().unwrap().into()));
        let opened = tellur_rt_v1_primitive(&mut c, b"open".as_ptr(), 4, [path].as_ptr(), 1);
        let mut payload = [0];
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                opened,
                b"opened".as_ptr(),
                6,
                payload.as_mut_ptr(),
                1
            ),
            1
        );
        let owner = c.get(payload[0]).unwrap().clone();
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, payload[0]);
        for expected in [b"retained owner\0".to_vec(), vec![]] {
            let snapshot = tellur_rt_v1_local(&mut c, 0, std::ptr::null(), 0, 0);
            let read = tellur_rt_v1_primitive(&mut c, b"read".as_ptr(), 4, [snapshot].as_ptr(), 1);
            assert!(c.get(snapshot).is_err());
            assert_eq!(c.storage.values()[0], Some(owner.clone()));
            assert_eq!(c.host.files.len(), 1);
            assert_eq!(
                tellur_rt_v1_route(&mut c, read, b"data".as_ptr(), 4, payload.as_mut_ptr(), 1),
                1
            );
            assert_eq!(c.take(payload[0]).unwrap(), V::Bytes(expected));
        }
        let moved = tellur_rt_v1_local(&mut c, 0, std::ptr::null(), 0, 1);
        let closed = tellur_rt_v1_primitive(&mut c, b"close".as_ptr(), 5, [moved].as_ptr(), 1);
        assert_eq!(
            tellur_rt_v1_route(
                &mut c,
                closed,
                b"closed".as_ptr(),
                6,
                std::ptr::null_mut(),
                0
            ),
            1
        );
        assert!(c.storage.values()[0].is_none() && c.host.files.is_empty());
        assert!(c.get(moved).is_err());
        assert!(c.error.is_none());
        assert_eq!(c.host.fuel, Some(0));
        assert_eq!(
            c.host
                .events
                .iter()
                .map(|e| e["operation"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["open", "read", "read", "close"]
        );
    }
    drop(c);
    drop(host);
    std::fs::remove_file(file).unwrap();
}
#[test]
fn invalid_borrowed_primitive_batches_preserve_physical_resource_and_file_position() {
    let file = abi_file("invalid-borrow");
    for malformed in ["arity", "type", "stale"] {
        let mut host = Host::new(file_manifest(), None, true, None);
        let (_, values) = host
            .primitive("open", vec![V::String(file.to_str().unwrap().into())])
            .unwrap();
        let owner = values[0].clone();
        let mut c = context(&mut host);
        let handle = c.put(owner.clone());
        let stale = c.put(V::Unit);
        c.take(stale).unwrap();
        let wrong = c.put(V::String("wrong".into()));
        let args = match malformed {
            "arity" => vec![handle, handle],
            "type" => vec![wrong],
            _ => vec![stale],
        };
        assert_eq!(
            unsafe {
                tellur_rt_v1_primitive(&mut c, b"read".as_ptr(), 4, args.as_ptr(), args.len())
            },
            0
        );
        assert_eq!(c.get(handle).unwrap(), &owner);
        assert!(c.get(wrong).is_ok());
        assert_eq!(c.host.files.len(), 1);
        assert_eq!(c.host.events.len(), 1);
        drop(c);
        assert_eq!(
            host.primitive("read", vec![owner.clone()]).unwrap().1,
            vec![V::Bytes(b"retained owner\0".to_vec())]
        );
        host.primitive("close", vec![owner]).unwrap();
        assert!(host.files.is_empty());
    }
    std::fs::remove_file(file).unwrap();
}

fn primitive_manifest() -> Json {
    json!({"types":[],"primitives":[{"name":"checked_add","parameters":[
        {"name":"a","type":"U32","mode":"value"},{"name":"b","type":"U32","mode":"value"}],
        "outcomes":[{"name":"sum","fields":[{"name":"value","type":"U32"}]},{"name":"overflow","fields":[]}],
        "effects":[],"returns":true}]})
}

#[test]
fn primitive_dispatch_preserves_tracing_and_consumes_each_argument_once() {
    let mut host = Host::new(primitive_manifest(), None, true, Some(0));
    let mut c = context(&mut host);
    let args = [c.put(V::U32(7)), c.put(V::U32(9))];
    unsafe {
        let outcome = tellur_rt_v1_primitive(&mut c, b"checked_add".as_ptr(), 11, args.as_ptr(), 2);
        assert!(args.iter().all(|h| c.get(*h).is_err()));
        let mut field = [0];
        assert_eq!(
            tellur_rt_v1_route(&mut c, outcome, b"sum".as_ptr(), 3, field.as_mut_ptr(), 1),
            1
        );
        assert_eq!(c.take(field[0]).unwrap(), V::U32(16));
        assert_eq!(tellur_rt_v1_ok(&mut c), 1);
    }
    assert_eq!(c.host.events.len(), 1);
    assert_eq!(c.host.events[0]["kind"], "primitive");
    assert_eq!(c.host.events[0]["operation"], "checked_add");
    assert_eq!(
        c.host.events[0]["arguments"],
        json!([{"kind":"u32","value":7},{"kind":"u32","value":9}])
    );
    assert_eq!(c.host.fuel, Some(0), "ABI dispatch adds no fuel tick");
}

#[test]
fn malformed_primitive_calls_do_not_transfer_or_invoke_the_host() {
    for case in [
        "duplicate",
        "stale",
        "zero",
        "arity",
        "type",
        "unknown",
        "adapter",
        "async",
        "nonreturning",
    ] {
        let mut manifest = primitive_manifest();
        match case {
            "adapter" => manifest["primitives"][0]["rust"] = json!({"operation":"custom"}),
            "async" => manifest["primitives"][0]["async"] = json!(true),
            "nonreturning" => manifest["primitives"][0]["returns"] = json!(false),
            _ => {}
        }
        let mut host = Host::new(manifest, None, true, None);
        let mut c = context(&mut host);
        let first = c.put(if case == "type" {
            V::String("invalid".into())
        } else {
            V::U32(1)
        });
        let second = c.put(V::U32(2));
        let stale = c.put(V::Unit);
        c.take(stale).unwrap();
        let args = match case {
            "duplicate" => vec![first, first],
            "stale" => vec![first, stale],
            "zero" => vec![first, 0],
            "arity" => vec![first],
            _ => vec![first, second],
        };
        let name = if case == "unknown" {
            "missing"
        } else {
            "checked_add"
        };
        unsafe {
            assert_eq!(
                tellur_rt_v1_primitive(
                    &mut c,
                    name.as_ptr(),
                    name.len(),
                    args.as_ptr(),
                    args.len()
                ),
                0,
                "{case}"
            );
        }
        assert!(c.get(first).is_ok() && c.get(second).is_ok(), "{case}");
        assert!(c.outcomes.is_empty() && c.host.events.is_empty(), "{case}");
        assert!(c.error.is_some());
    }
}

#[test]
fn primitive_host_failure_remains_sticky_without_publishing_payloads() {
    let script = vec![ScriptCall {
        operation: "checked_add".into(),
        outcome: "undeclared".into(),
        fields: vec![],
        waits: 0,
    }];
    let mut host = Host::new(primitive_manifest(), Some(script), true, None);
    let mut c = context(&mut host);
    let args = [c.put(V::U32(1)), c.put(V::U32(2))];
    let later = c.put(V::String("must not emit".into()));
    unsafe {
        assert_eq!(
            tellur_rt_v1_primitive(&mut c, b"checked_add".as_ptr(), 11, args.as_ptr(), 2),
            0
        );
        let original = c.error.clone();
        tellur_rt_v1_emit(&mut c, later);
        assert_eq!(c.error, original);
    }
    assert!(c.get(later).is_ok() && c.outcomes.is_empty() && c.host.events.is_empty());
    assert!(c.error.unwrap().contains("undeclared outcome"));
}

#[test]
fn collection_literal_shapes_validate_before_transferring_any_fields() {
    for (ty, name, values) in [
        ("array<U32,2>", "array", vec![V::U32(1)]),
        ("vector<U32>", "array", vec![V::U32(1)]),
        ("vector<U32>", "vector", vec![V::U32(1), V::Bool(true)]),
        ("map<U32,String>", "map", vec![V::U32(1)]),
        (
            "map<U32,String>",
            "map",
            vec![
                V::U32(2),
                V::String("b".into()),
                V::U32(1),
                V::String("a".into()),
            ],
        ),
        (
            "map<U32,String>",
            "map",
            vec![
                V::U32(1),
                V::String("a".into()),
                V::U32(1),
                V::String("b".into()),
            ],
        ),
    ] {
        let mut host = Host::new(json!({}), None, true, Some(0));
        let mut c = context(&mut host);
        let handles = values.iter().cloned().map(|v| c.put(v)).collect::<Vec<_>>();
        assert_eq!(
            unsafe {
                tellur_rt_v1_construct(
                    &mut c,
                    ty.as_ptr(),
                    ty.len(),
                    name.as_ptr(),
                    name.len(),
                    handles.as_ptr(),
                    handles.len(),
                )
            },
            0
        );
        assert!(c.error.is_some());
        for (&h, v) in handles.iter().zip(&values) {
            assert_eq!(c.get(h).unwrap(), v);
        }
        assert!(c.outcomes.is_empty());
        assert_eq!(c.host.fuel, Some(0));
    }
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    let h = c.put(V::U32(3));
    assert!(c.construct("vector<U32>", "vector", &[h, h]).is_err());
    assert!(c.get(h).is_ok());
    assert!(c.construct("vector<U32>", "vector", &[h, 999]).is_err());
    assert!(c.get(h).is_ok());
    let fixed = c.construct("array<U32,1>", "array", &[h]).unwrap();
    assert!(c.get(h).is_err());
    let nested = c
        .construct("vector<array<U32,1>>", "vector", &[fixed])
        .unwrap();
    assert!(c.get(fixed).is_err());
    validate(c.get(nested).unwrap(), "vector<array<U32,1>>", &json!({})).unwrap();
    let empty = c.construct("map<String,U32>", "map", &[]).unwrap();
    validate(c.get(empty).unwrap(), "map<String,U32>", &json!({})).unwrap();
}

#[test]
fn collection_abi_keeps_element_addresses_and_validates_batches_before_mutation() {
    let mut host = Host::new(json!({}), None, true, Some(0));
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let a = c.put(V::String("first".into()));
        let b = c.put(V::String("second".into()));
        let owner = c.construct("vector<String>", "vector", &[a, b]).unwrap();
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
        let ty = "&mutable vector<String>";
        let reference =
            tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, ty.as_ptr(), ty.len());
        let reference = annotate(&mut c, reference, &[1, 1, 0, 0, 0]);
        let index = c.put(V::U32(1));
        let name = "vector_get_mut";
        let outcome = tellur_rt_v1_collection(
            &mut c,
            name.as_ptr(),
            name.len(),
            [reference, index].as_ptr(),
            2,
        );
        assert!(c.get(reference).is_err() && c.get(index).is_err());
        let result = c.outcome(outcome).unwrap();
        assert_eq!(result.0, "found");
        let V::Reference {
            root,
            fields,
            evidence,
            ..
        } = &result.1[0]
        else {
            panic!()
        };
        assert_eq!(*root, c.local(0).unwrap());
        assert_eq!(fields, &vec![1]);
        assert!(evidence.origins.is_complete());
        let element = c.put(result.1[0].clone());
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, element);
        let value = c.put(V::String("changed".into()));
        tellur_rt_v1_assign_through(&mut c, 1, std::ptr::null(), 0, value);
        let reference =
            tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, ty.as_ptr(), ty.len());
        let length = tellur_rt_v1_length(&mut c, reference);
        assert_eq!(c.get(length).unwrap(), &V::U32(2));
        assert!(c.get(reference).is_err());
        assert_eq!(
            read_place(c.storage.values(), c.local(0).unwrap(), &[1], false).unwrap(),
            V::String("changed".into())
        );
        assert!(c.error.is_none());
        assert_eq!(c.host.fuel, Some(0));
    }
    // Invalid later handles must not consume a live owner reference or mutate it.
    for duplicate in [false, true] {
        let mut host = Host::new(json!({}), None, true, Some(0));
        let mut c = context(&mut host);
        unsafe {
            tellur_rt_v1_frame(&mut c, 1);
            let owner = c.construct("vector<U32>", "vector", &[]).unwrap();
            tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
            let ty = "&mutable vector<U32>";
            let reference =
                tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, ty.as_ptr(), ty.len());
            let args = [reference, if duplicate { reference } else { 999 }];
            let name = "vector_push";
            assert_eq!(
                tellur_rt_v1_collection(&mut c, name.as_ptr(), name.len(), args.as_ptr(), 2),
                0
            );
            assert!(c.get(reference).is_ok());
            assert!(c.outcomes.is_empty());
            let value = read_place(c.storage.values(), c.local(0).unwrap(), &[], false).unwrap();
            assert_eq!(collection_support::length(&value).unwrap(), 0);
            assert_eq!(c.host.fuel, Some(0));
        }
    }
}

#[test]
fn consumed_temporary_tables_stay_bounded_without_reusing_value_or_outcome_ids() {
    let mut host = Host::new(json!({}), None, false, None);
    let mut c = context(&mut host);
    let (mut previous_value, mut previous_outcome) = (0, 0);
    for n in 0..100_000 {
        let value = c.put(V::U32(n));
        assert!(value > previous_value);
        assert!(c.get(previous_value).is_err());
        assert_eq!(c.take(value).unwrap(), V::U32(n));
        let outcome = c.put_outcome(("done".into(), vec![]));
        assert!(outcome > previous_outcome);
        assert!(c.outcome(previous_outcome).is_err());
        assert_eq!(
            unsafe {
                tellur_rt_v1_route(
                    &mut c,
                    outcome,
                    b"done".as_ptr(),
                    4,
                    std::ptr::null_mut(),
                    0,
                )
            },
            1
        );
        unsafe { tellur_rt_v1_tick(&mut c, 0, 1, 1) };
        assert!(c.error.is_none());
        assert!(c.values.is_empty() && c.outcomes.is_empty());
        assert!(c.values.capacity() <= 4 && c.outcomes.capacity() <= 4);
        previous_value = value;
        previous_outcome = outcome;
    }
    assert_eq!((c.value_base, c.outcome_base), (100_000, 100_000));
    assert!(c.get(previous_value).is_err() && c.outcome(previous_outcome).is_err());
}

#[test]
fn temporary_retirement_and_exhaustion_preserve_live_batches_and_first_failure() {
    let mut host = Host::new(json!({}), None, false, None);
    let mut c = context(&mut host);
    let value = c.put(V::String("live".into()));
    assert!(
        c.retire_temporaries()
            .unwrap_err()
            .contains("live ABI temporary")
    );
    assert_eq!(c.value_base, 0);
    assert_eq!(c.get(value).unwrap(), &V::String("live".into()));
    c.take(value).unwrap();
    let outcome = c.put_outcome(("done".into(), vec![V::U32(7)]));
    assert!(c.retire_temporaries().is_err());
    assert_eq!((c.value_base, c.outcome_base), (0, 0));
    // Exhaustion is checked before an outcome is consumed or payload is published.
    c.value_base = u64::MAX - 1;
    let mut payload = 91;
    assert_eq!(
        unsafe { tellur_rt_v1_route(&mut c, outcome, b"done".as_ptr(), 4, &mut payload, 1) },
        0
    );
    assert_eq!(payload, 91);
    assert!(c.outcome(outcome).is_ok());
    let first = c.error.clone();
    assert_eq!(c.put(V::Unit), 0);
    assert_eq!(c.error, first);
    assert_eq!(c.values.len(), 1);
    c.error = None;
    c.value_base = 0;
    assert_eq!(
        unsafe { tellur_rt_v1_route(&mut c, outcome, b"done".as_ptr(), 4, &mut payload, 1) },
        1
    );
    assert_eq!(c.take(payload).unwrap(), V::U32(7));
    c.retire_temporaries().unwrap();
    assert!(c.get(value).is_err() && c.outcome(outcome).is_err());
    c.value_base = u64::MAX;
    c.outcome_base = u64::MAX;
    assert_eq!(c.put(V::Unit), 0);
    assert_eq!(c.put_outcome(("done".into(), vec![])), 0);
    assert!(c.values.is_empty() && c.outcomes.is_empty());
    // A corrupt/exhausted base cannot partially retire either table.
    c.error = None;
    c.values.push(None);
    c.outcomes.push(None);
    assert!(c.retire_temporaries().is_err());
    assert_eq!((c.value_base, c.outcome_base), (u64::MAX, u64::MAX));
    assert_eq!((c.values.len(), c.outcomes.len()), (1, 1));
    let mut host = Host::new(json!({}), None, false, None);
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 1);
        let number = c.put(V::U32(1));
        let owner = c.construct("vector<U32>", "vector", &[number]).unwrap();
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
        let ty = "&mutable vector<U32>";
        let reference =
            tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, ty.as_ptr(), ty.len());
        let value = c.put(V::U32(2));
        c.outcome_base = u64::MAX;
        let name = "vector_push";
        assert_eq!(
            tellur_rt_v1_collection(
                &mut c,
                name.as_ptr(),
                name.len(),
                [reference, value].as_ptr(),
                2
            ),
            0
        );
        assert!(c.get(reference).is_ok() && c.get(value).is_ok());
        assert!(c.outcomes.is_empty());
        assert_eq!(
            collection_support::length(
                &read_place(c.storage.values(), c.local(0).unwrap(), &[], false).unwrap()
            )
            .unwrap(),
            1
        );
    }
}

#[test]
fn retiring_temporaries_keeps_frame_owners_and_reference_evidence_stable() {
    let mut host = Host::new(json!({}), None, false, None);
    let mut c = context(&mut host);
    unsafe {
        tellur_rt_v1_frame(&mut c, 2);
        let owner = c.put(V::String("stable".into()));
        tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, owner);
        let reference =
            tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
        let reference = annotate(&mut c, reference, &[1, 1, 0, 0, 0]);
        let expected = c.get(reference).unwrap().clone();
        tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, reference);
        for _ in 0..1000 {
            tellur_rt_v1_tick(&mut c, 0, 1, 1);
            let borrowed = tellur_rt_v1_local(&mut c, 1, std::ptr::null(), 0, 0);
            assert_eq!(c.take(borrowed).unwrap(), expected);
            let read = tellur_rt_v1_local(&mut c, 0, std::ptr::null(), 0, 0);
            assert_eq!(c.take(read).unwrap(), V::String("stable".into()));
            assert!(c.get(owner).is_err());
        }
        assert!(c.error.is_none());
        assert_eq!(c.storage.values().len(), 2);
        assert_eq!(
            read_place(c.storage.values(), c.local(0).unwrap(), &[], false).unwrap(),
            V::String("stable".into())
        );
    }
}

#[test]
fn temporary_field_reads_assignment_and_projection_resolve_retired_handle_bases() {
    let mut host = Host::new(record_manifest(), None, false, None);
    let mut c = context(&mut host);
    let stale = c.put(V::Unit);
    c.take(stale).unwrap();
    c.retire_temporaries().unwrap();
    unsafe {
        let owner = c.put(pair());
        let field = tellur_rt_v1_field(&mut c, owner, [1].as_ptr(), 1, 0);
        assert_eq!(c.take(field).unwrap(), V::U32(7));
        let rhs = c.put(V::String("replacement".into()));
        tellur_rt_v1_assign(&mut c, owner, [0].as_ptr(), 1, rhs);
        assert!(c.get(rhs).is_err());
        let projected = tellur_rt_v1_project(&mut c, owner, 0);
        assert_eq!(c.take(projected).unwrap(), V::String("replacement".into()));
        assert!(c.get(owner).is_err());
        assert!(c.error.is_none());
        c.retire_temporaries().unwrap();
        let live = c.put(pair());
        let before = c.get(live).unwrap().clone();
        assert_eq!(tellur_rt_v1_project(&mut c, stale, 0), 0);
        assert_eq!(c.get(live).unwrap(), &before);
    }
}

unsafe extern "C" fn constructor_forward(ctx: *mut Context<'_>, args: *const u64, _: usize) {
    unsafe {
        tellur_rt_v1_frame(ctx, 1);
        let partial = tellur_rt_v1_value(ctx, 3, 0, b"partial".as_ptr(), 7);
        tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, partial);
        tellur_rt_v1_constructed(ctx, *args);
    }
}

#[test]
fn constructor_destination_delivery_preserves_caller_storage_without_outcome_handles() {
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("root").unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 3) };
    c.storage.values_mut()[0] = Some(V::String("caller".into()));
    let owner = c
        .storage
        .values()
        .local_storage(c.frame.as_ref().unwrap(), 0)
        .unwrap();
    let reference = unsafe {
        tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7)
    };
    let reference = c.take(reference).unwrap();
    let value = c.put(V::Construct {
        ty: "view".into(),
        name: "view".into(),
        fields: vec![reference],
    });
    c.outcome_base = u64::MAX; // Constructors need no outcome capacity.
    unsafe {
        tellur_rt_v1_construct_call(
            &mut c,
            constructor_forward,
            b"view.constructor".as_ptr(),
            16,
            &value,
            1,
            1,
        )
    };
    assert!(c.error.is_none(), "{:?}", c.error);
    assert!(c.result.is_none());
    assert!(c.outcomes.is_empty());
    assert!(c.get(value).is_err());
    assert_eq!(c.storage.values().require_storage(&owner).unwrap(), 0);
    assert_eq!(c.storage.values().len(), 3); // Child partial field retired.
    let reference = unsafe { tellur_rt_v1_local(&mut c, 1, [0].as_ptr(), 1, 0) };
    unsafe { tellur_rt_v1_store(&mut c, 2, std::ptr::null(), 0, reference) };
    let text = unsafe { tellur_rt_v1_deref(&mut c, 2, std::ptr::null(), 0, 0) };
    assert_eq!(c.get(text).unwrap(), &V::String("caller".into()));
    assert!(c.error.is_none());
}

#[test]
fn constructor_destination_and_argument_checks_precede_transfer() {
    for (slot, occupied, duplicate) in [(2, false, false), (0, true, false), (0, false, true)] {
        let mut host = Host::new(json!({}), None, true, None);
        let mut c = context(&mut host);
        unsafe { tellur_rt_v1_frame(&mut c, 1) };
        if occupied {
            c.storage.values_mut()[0] = Some(V::String("keep".into()));
        }
        let value = c.put(V::String("argument".into()));
        let args = if duplicate {
            vec![value, value]
        } else {
            vec![value]
        };
        unsafe {
            tellur_rt_v1_construct_call(
                &mut c,
                constructor_forward,
                b"ctor".as_ptr(),
                4,
                args.as_ptr(),
                args.len(),
                slot,
            )
        };
        assert!(c.error.is_some());
        assert!(c.get(value).is_ok());
        assert_eq!(
            c.storage.values()[0],
            occupied.then(|| V::String("keep".into()))
        );
        assert!(c.outcomes.is_empty());
    }
    for retired in [false, true] {
        let mut host = Host::new(json!({}), None, true, None);
        let mut c = context(&mut host);
        let mut other = Frames::default();
        let frame = other.enter(1).unwrap();
        c.destination = Some(other.local_storage(&frame, 0).unwrap());
        if retired {
            other.retire(&frame).unwrap();
        }
        c.storage = FrameStorage::Owned(if retired { other } else { Frames::default() });
        let replacement = c.storage.values_mut().enter(1).unwrap();
        c.frame = Some(replacement);
        let value = c.put(V::String("not transferred".into()));
        unsafe { tellur_rt_v1_constructed(&mut c, value) };
        assert!(c.error.as_ref().unwrap().contains("foreign or retired"));
        assert!(c.get(value).is_ok());
        assert!(c.storage.values()[0].is_none());
    }
}

#[test]
fn failed_constructor_callbacks_retire_fields_and_revoke_malformed_delivery() {
    unsafe extern "C" fn fail_before(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 1);
            tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, *args);
            let message = tellur_rt_v1_value(ctx, 3, 0, b"first failure".as_ptr(), 13);
            tellur_rt_v1_fault(ctx, message);
            tellur_rt_v1_constructed(ctx, 999);
        }
    }
    unsafe extern "C" fn fail_after(ctx: *mut Context<'_>, args: *const u64, count: usize) {
        unsafe {
            constructor_forward(ctx, args, count);
            let message = tellur_rt_v1_value(ctx, 3, 0, b"first failure".as_ptr(), 13);
            tellur_rt_v1_fault(ctx, message);
        }
    }
    unsafe extern "C" fn fallthrough(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 1);
            tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, *args);
        }
    }
    for (callback, expected) in [
        (fail_before as Callback, "E_FAULT: first failure"),
        (fail_after as Callback, "E_FAULT: first failure"),
        (
            fallthrough as Callback,
            "E_NATIVE: compiled ABI control fell through",
        ),
        (
            echo as Callback,
            "E_NATIVE: constructor requires destination completion",
        ),
    ] {
        let mut host = Host::new(json!({}), None, true, None);
        host.enter("root").unwrap();
        let mut c = context(&mut host);
        unsafe { tellur_rt_v1_frame(&mut c, 2) };
        c.storage.values_mut()[0] = Some(V::String("caller".into()));
        let value = c.put(V::String("partial field".into()));
        unsafe {
            tellur_rt_v1_construct_call(
                &mut c,
                callback,
                b"broken.constructor".as_ptr(),
                18,
                &value,
                1,
                1,
            )
        };
        assert_eq!(c.error.as_deref(), Some(expected));
        assert_eq!(
            &**c.storage.values(),
            &[Some(V::String("caller".into())), None]
        );
        assert!(c.outcomes.is_empty());
        let report = c
            .host
            .activations
            .failure("root", fault::Phase::Execution, expected);
        assert_eq!(
            report
                .activations
                .iter()
                .map(|a| a.control.as_str())
                .collect::<Vec<_>>(),
            ["root", "broken.constructor"]
        );
    }
}

#[test]
fn constructor_completion_rejects_missing_and_duplicate_destinations_without_transfer() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    let value = c.put(V::String("kept".into()));
    unsafe { tellur_rt_v1_constructed(&mut c, value) };
    assert!(
        c.error
            .as_ref()
            .unwrap()
            .contains("missing constructor destination")
    );
    assert!(c.get(value).is_ok());
    c.error = None;
    unsafe { tellur_rt_v1_frame(&mut c, 1) };
    c.destination = Some(
        c.storage
            .values()
            .local_storage(c.frame.as_ref().unwrap(), 0)
            .unwrap(),
    );
    unsafe { tellur_rt_v1_constructed(&mut c, value) };
    assert!(c.error.is_none());
    let second = c.put(V::String("second".into()));
    unsafe { tellur_rt_v1_constructed(&mut c, second) };
    assert!(
        c.error
            .as_ref()
            .unwrap()
            .contains("duplicate ABI completion")
    );
    assert!(c.get(second).is_ok());
    assert_eq!(c.storage.values()[0], Some(V::String("kept".into())));
}

unsafe extern "C" fn boundary_work(ctx: *mut Context<'_>, args: *const u64, _: usize) {
    unsafe {
        tellur_rt_v1_frame(ctx, 3);
        for slot in 0..2 {
            tellur_rt_v1_store(ctx, slot, std::ptr::null(), 0, *args.add(slot));
        }
        let fail = tellur_rt_v1_local(ctx, 0, std::ptr::null(), 0, 0);
        if tellur_rt_v1_bool(ctx, fail) != 0 {
            let message = tellur_rt_v1_value(ctx, 3, 0, b"request failed".as_ptr(), 14);
            tellur_rt_v1_fault(ctx, message);
        } else {
            tellur_rt_v1_settled(ctx);
        }
    }
}
unsafe extern "C" fn boundary_handler(ctx: *mut Context<'_>, args: *const u64, _: usize) {
    unsafe {
        tellur_rt_v1_frame(ctx, 2);
        for slot in 0..2 {
            tellur_rt_v1_store(ctx, slot, std::ptr::null(), 0, *args.add(slot));
        }
        for slot in 0..2 {
            let value = tellur_rt_v1_local(ctx, slot, std::ptr::null(), 0, 1);
            tellur_rt_v1_emit(ctx, value);
        }
        tellur_rt_v1_settled(ctx);
    }
}
unsafe fn supervise(
    ctx: &mut Context<'_>,
    work: Callback,
    handler: Callback,
    args: &[u64],
    retained: &[usize],
) {
    unsafe {
        tellur_rt_v1_supervise(
            ctx,
            work,
            handler,
            b"request".as_ptr(),
            7,
            b"request$contingency".as_ptr(),
            19,
            args.as_ptr(),
            args.len(),
            retained.as_ptr(),
            retained.len(),
        )
    };
}

#[test]
fn named_supervision_settles_without_outcomes_and_preserves_independent_caller_storage() {
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("main").unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 1) };
    c.storage.values_mut()[0] = Some(V::String("caller".into()));
    c.outcome_base = u64::MAX;
    for fail in [true, false, true] {
        let args = [c.put(V::Bool(fail)), c.put(V::String("retained".into()))];
        unsafe { supervise(&mut c, boundary_work, boundary_handler, &args, &[1]) };
        assert!(c.error.is_none(), "{:?}", c.error);
        assert!(c.result.is_none() && c.outcomes.is_empty());
        assert!(args.iter().all(|h| c.get(*h).is_err()));
        assert_eq!(&**c.storage.values(), &[Some(V::String("caller".into()))]);
        assert!(c.host.resources.scopes.is_empty() && c.host.contingencies.is_empty());
        assert!(!c.host.source_faulted());
    }
    let messages = c
        .host
        .events
        .iter()
        .filter_map(|e| e.get("message"))
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            &json!("retained"),
            &json!("request failed"),
            &json!("retained"),
            &json!("request failed")
        ]
    );
    assert_eq!(
        c.host
            .events
            .iter()
            .filter(|e| e["kind"] == "fault")
            .count(),
        2
    );
}

#[test]
fn named_supervision_bypasses_non_source_errors_and_never_reenters_its_handler() {
    unsafe extern "C" fn limit(ctx: *mut Context<'_>, _: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 0);
            tellur_rt_v1_tick(ctx, 0, 1, 1);
        }
    }
    unsafe extern "C" fn escalate(ctx: *mut Context<'_>, _: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 0);
            tellur_rt_v1_escalate(ctx);
        }
    }
    unsafe extern "C" fn secondary(ctx: *mut Context<'_>, _: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 0);
            let value = tellur_rt_v1_value(ctx, 3, 0, b"cleanup failed".as_ptr(), 14);
            tellur_rt_v1_fault(ctx, value);
        }
    }
    for (handler, expected) in [
        (escalate as Callback, "E_FAULT: request failed"),
        (secondary as Callback, "E_FAULT: cleanup failed"),
    ] {
        let mut host = Host::new(json!({}), None, true, None);
        host.enter("main").unwrap();
        let mut c = context(&mut host);
        let args = [c.put(V::Bool(true)), c.put(V::String("retained".into()))];
        unsafe { supervise(&mut c, boundary_work, handler, &args, &[1]) };
        assert_eq!(c.error.as_deref(), Some(expected));
        assert!(c.host.source_faulted());
        assert!(c.host.resources.scopes.is_empty() && c.host.contingencies.is_empty());
        assert!(c.storage.values().is_empty());
        assert_eq!(c.host.events.len(), 1);
        let report = c
            .host
            .activations
            .failure("main", fault::Phase::Execution, expected);
        assert!(
            report
                .cause
                .unwrap()
                .cause
                .unwrap()
                .message
                .contains("request failed")
        );
    }
    let mut host = Host::new(json!({}), None, true, Some(0));
    host.enter("main").unwrap();
    let mut c = context(&mut host);
    unsafe { supervise(&mut c, limit, boundary_handler, &[], &[]) };
    assert!(c.error.as_ref().unwrap().starts_with("E_FUEL"));
    assert!(!c.host.source_faulted());
    assert!(c.host.events.is_empty());
    assert!(c.host.resources.scopes.is_empty() && c.storage.values().is_empty());
}

#[test]
fn named_supervision_validates_batches_and_malformed_completions() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = context(&mut host);
    for retained in [vec![0, 0], vec![0, 1, 2]] {
        let args = [c.put(V::Bool(false)), c.put(V::String("kept".into()))];
        unsafe { supervise(&mut c, boundary_work, boundary_handler, &args, &retained) };
        assert!(
            c.error
                .as_ref()
                .unwrap()
                .contains("invalid ABI boundary retained set")
        );
        assert!(args.iter().all(|h| c.get(*h).is_ok()));
        assert!(c.host.resources.scopes.is_empty());
        c.error = None;
    }
    let owner = c.put(V::String("kept".into()));
    unsafe {
        supervise(
            &mut c,
            boundary_work,
            boundary_handler,
            &[owner, owner],
            &[],
        )
    };
    assert!(c.get(owner).is_ok());
    assert!(c.host.resources.scopes.is_empty());
    c.error = None;
    unsafe { supervise(&mut c, echo, boundary_handler, &[], &[]) };
    assert!(c.error.as_ref().unwrap().contains("ordinary outcome"));
    assert!(c.host.resources.scopes.is_empty() && c.host.events.is_empty());
    c.error = None;
    let args = [c.put(V::Bool(true)), c.put(V::String("kept".into()))];
    unsafe { supervise(&mut c, boundary_work, boundary_handler, &args, &[99]) };
    assert!(c.error.as_ref().unwrap().contains("outside its activation"));
    assert!(c.storage.values().is_empty() && c.host.resources.scopes.is_empty());
    assert!(c.host.events.is_empty()); // Malformed handover never enters cleanup.
}

#[test]
fn named_supervision_retires_borrowers_and_releases_only_abandoned_resources_before_cleanup() {
    use std::{cell::RefCell, rc::Rc};
    struct Probe(u32, Rc<RefCell<Vec<u32>>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    unsafe extern "C" fn work(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        unsafe {
            tellur_rt_v1_frame(ctx, 3);
            let c = &mut *ctx;
            let owner = c.get(*args).unwrap().clone();
            let drops = c
                .host
                .resources
                .get::<Probe>(&owner, "probe")
                .unwrap()
                .1
                .clone();
            let abandoned = c.host.resources.insert("probe", Probe(2, drops)).unwrap();
            let abandoned = c.put(abandoned);
            tellur_rt_v1_store(ctx, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_store(ctx, 2, std::ptr::null(), 0, abandoned);
            let borrow =
                tellur_rt_v1_reference(ctx, 0, std::ptr::null(), 0, 0, b"&probe".as_ptr(), 6);
            tellur_rt_v1_store(ctx, 1, std::ptr::null(), 0, borrow);
            let message = tellur_rt_v1_value(ctx, 3, 0, b"broken".as_ptr(), 6);
            tellur_rt_v1_fault(ctx, message);
        }
    }
    unsafe extern "C" fn cleanup(ctx: *mut Context<'_>, args: *const u64, _: usize) {
        let c = unsafe { &mut *ctx };
        c.perform(|c| {
            if c.storage.values().len() != 1 {
                return Err("work frame still live".into());
            }
            let owner = c.take(unsafe { *args })?;
            let drops = c.host.resources.get::<Probe>(&owner, "probe")?.1.clone();
            if *drops.borrow() != [2] {
                return Err("abandoned resource not retired before handler".into());
            }
            let V::Opaque { token, .. } = owner else {
                return Err("missing owner".into());
            };
            c.host.resources.values.remove(&token);
            c.take(unsafe { *args.add(1) })?;
            Ok(())
        });
        unsafe { tellur_rt_v1_settled(ctx) };
    }
    let drops = Rc::new(RefCell::new(vec![]));
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("main").unwrap();
    let parent = host
        .resources
        .insert("probe", Probe(0, drops.clone()))
        .unwrap();
    let owner = host
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 1) };
    c.storage.values_mut()[0] = Some(parent.clone());
    let owner = c.put(owner);
    unsafe { supervise(&mut c, work, cleanup, &[owner], &[0]) };
    assert!(c.error.is_none(), "{:?}", c.error);
    assert_eq!(*drops.borrow(), [2, 1]);
    assert!(c.host.resources.get::<Probe>(&parent, "probe").is_ok());
    assert!(c.host.resources.scopes.is_empty());
    assert_eq!(c.storage.values().len(), 1);
}

unsafe fn lexical(c: &mut Context<'_>, layout: &[usize], counts: [usize; 4], fault: usize) -> u64 {
    unsafe {
        tellur_rt_v1_lexical_begin(
            c,
            b"main$boundary@1".as_ptr(),
            15,
            layout.as_ptr(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            fault,
        )
    }
}
#[test]
fn lexical_abi_retires_only_scoped_storage_and_preserves_outward_jump_arguments() {
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("main").unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 5) };
    for i in 0..5 {
        c.storage.values_mut()[i] = Some(V::String(format!("slot{i}")));
    }
    let frame = c
        .storage
        .values()
        .local_storage(c.frame.as_ref().unwrap(), 0)
        .unwrap();
    let id = unsafe { lexical(&mut c, &[0, 1, 2, 3], [1, 1, 1, 1], 3) };
    assert_ne!(id, 0);
    assert!(c.error.is_none(), "{:?}", c.error);
    let arg = c.put(V::String("restart".into()));
    unsafe { tellur_rt_v1_lexical_end(&mut c, id) };
    assert!(c.error.is_none(), "{:?}", c.error);
    assert_eq!(c.take(arg).unwrap(), V::String("restart".into()));
    assert_eq!(c.storage.values().require_storage(&frame).unwrap(), 0);
    assert_eq!(
        &**c.storage.values(),
        &[
            Some(V::String("slot0".into())),
            Some(V::String("slot1".into())),
            None,
            None,
            Some(V::String("slot4".into()))
        ]
    );
    assert!(c.lexical.is_empty() && c.host.resources.scopes.is_empty());
    assert!(c.result.is_none());
}
#[test]
fn lexical_abi_fault_handover_retires_temporaries_and_resources_before_handler() {
    use std::{cell::RefCell, rc::Rc};
    struct Probe(u32, Rc<RefCell<Vec<u32>>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let drops = Rc::new(RefCell::new(vec![]));
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("main").unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 5) };
    for (slot, id) in [(4, 0), (0, 1), (1, 2)] {
        let owner = c
            .host
            .resources
            .insert("probe", Probe(id, drops.clone()))
            .unwrap();
        c.storage.values_mut()[slot] = Some(owner);
    }
    let id = unsafe { lexical(&mut c, &[0, 1, 2, 2, 3], [1, 2, 1, 1], 3) };
    let temporary =
        unsafe { tellur_rt_v1_reference(&mut c, 0, std::ptr::null(), 0, 0, b"&probe".as_ptr(), 6) };
    let borrower = c.get(temporary).unwrap().clone();
    c.storage.values_mut()[2] = Some(borrower);
    let message = c.put(V::String("broken".into()));
    unsafe { tellur_rt_v1_fault(&mut c, message) };
    assert_eq!(unsafe { tellur_rt_v1_lexical_catch(&mut c, id) }, 1);
    assert!(c.error.is_none(), "{:?}", c.error);
    assert!(c.get(temporary).is_err());
    assert!(c.storage.values()[1].is_none() && c.storage.values()[2].is_none());
    assert_eq!(*drops.borrow(), [2]);
    let V::Construct { fields, .. } = c.storage.values()[3].as_ref().unwrap() else {
        panic!("fault report")
    };
    assert_eq!(fields[0], V::String("broken".into()));
    assert_eq!(fields.len(), 3);
    let V::Opaque { token, .. } = c.storage.values_mut()[0].take().unwrap() else {
        panic!("owner")
    };
    c.host.resources.values.remove(&token);
    unsafe {
        tellur_rt_v1_process(&mut c, 23);
        tellur_rt_v1_lexical_end(&mut c, id)
    };
    assert!(matches!(c.result, Some(Completion::Process(23))));
    assert_eq!(*drops.borrow(), [2, 1]);
    assert!(c.storage.values()[4].is_some());
    assert!(c.storage.values()[3].is_none());
    assert!(c.host.resources.scopes.is_empty() && c.host.contingencies.is_empty());
    assert!(!c.host.source_faulted());
}
#[test]
fn lexical_abi_nested_faults_keep_causes_and_non_source_errors_bypass_handover() {
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("main").unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 2) };
    let outer = unsafe { lexical(&mut c, &[0], [0, 0, 0, 1], 0) };
    let inner = unsafe { lexical(&mut c, &[1], [0, 0, 0, 1], 1) };
    let first = c.put(V::String("first".into()));
    unsafe { tellur_rt_v1_fault(&mut c, first) };
    assert_eq!(unsafe { tellur_rt_v1_lexical_catch(&mut c, inner) }, 1);
    let second = c.put(V::String("second".into()));
    unsafe {
        tellur_rt_v1_fault(&mut c, second);
        tellur_rt_v1_lexical_end(&mut c, inner)
    };
    assert_eq!(unsafe { tellur_rt_v1_lexical_catch(&mut c, outer) }, 1);
    assert_eq!(c.host.events.len(), 2);
    let report = &c.host.events[1]["report"];
    assert!(report.to_string().contains("first") && report.to_string().contains("second"));
    c.error = Some("E_FUEL: interrupted cleanup".into());
    assert_eq!(unsafe { tellur_rt_v1_lexical_catch(&mut c, outer) }, 0);
    unsafe { tellur_rt_v1_lexical_end(&mut c, outer) };
    assert_eq!(c.error.as_deref(), Some("E_FUEL: interrupted cleanup"));
    assert!(!c.host.source_faulted());
    assert_eq!(c.host.events.len(), 2);
    assert!(c.lexical.is_empty() && c.host.resources.scopes.is_empty());
}
#[test]
fn lexical_abi_rejects_bad_layouts_stale_scopes_and_unfinished_completion() {
    let mut host = Host::new(json!({}), None, true, None);
    host.enter("main").unwrap();
    let mut c = context(&mut host);
    unsafe { tellur_rt_v1_frame(&mut c, 2) };
    c.storage.values_mut()[0] = Some(V::String("owner".into()));
    for (layout, counts, fault) in [
        (vec![0, 0, 1], [2, 0, 0, 1], 1),
        (vec![0, 0, 1], [1, 1, 0, 1], 1),
        (vec![99, 1], [1, 0, 0, 1], 1),
        (vec![0], [1, 0, 0, 0], 1),
    ] {
        assert_eq!(unsafe { lexical(&mut c, &layout, counts, fault) }, 0);
        assert!(c.error.take().is_some());
        assert!(c.lexical.is_empty() && c.host.resources.scopes.is_empty());
        assert_eq!(c.storage.values()[0], Some(V::String("owner".into())));
    }
    let first = unsafe { lexical(&mut c, &[1], [0, 0, 0, 1], 1) };
    unsafe { tellur_rt_v1_lexical_end(&mut c, first) };
    let second = unsafe { lexical(&mut c, &[1], [0, 0, 0, 1], 1) };
    assert_ne!(first, second);
    unsafe { tellur_rt_v1_lexical_end(&mut c, first) };
    assert!(c.error.take().unwrap().contains("not selected"));
    assert_eq!(c.lexical.len(), 1);
    unsafe { tellur_rt_v1_process(&mut c, 256) };
    assert!(c.error.take().unwrap().contains("invalid ABI process"));
    assert!(c.result.is_none());
    assert!(c.finish().err().unwrap().contains("unfinished ABI lexical"));
    assert!(
        c.lexical.is_empty() && c.host.resources.scopes.is_empty() && c.storage.values().is_empty()
    );
}

#[test]
fn scalar_search_projects_the_checked_loan_into_its_original_owner() {
    for complete in [false, true] {
        let mut host = Host::new(json!({}), None, false, None);
        host.activations.enter("search").unwrap();
        let mut c = context(&mut host);
        unsafe { tellur_rt_v1_frame(&mut c, 2) };
        let vector = V::Construct {
            ty: "vector<U32>".into(),
            name: "vector".into(),
            fields: vec![V::U32(9), V::U32(42)],
        };
        let owner = c.local(0).unwrap();
        c.storage.values_mut()[owner] = Some(V::Construct {
            ty: "record".into(),
            name: "record".into(),
            fields: vec![V::Unit, vector],
        });
        let loan = reference(c.storage.values(), owner, &[1], false, "&vector<U32>").unwrap();
        let mut handle = c.put(loan);
        if complete {
            handle = unsafe { annotate(&mut c, handle, &[1, 1, 0, 0, 1, 1]) };
        }
        let input = c.get(handle).unwrap().clone();
        let (_, mut expected) = collection(
            "vector_get",
            vec![input.clone(), V::U32(1)],
            c.storage.values_mut(),
        )
        .unwrap();
        let position = c.put(V::U32(1));
        let found = unsafe { tellur_rt_v1_search_loan(&mut c, handle, position, 70) };
        let actual = c.take(found).unwrap();
        let expected = expected.pop().unwrap();
        assert_eq!(actual, expected);
        let (
            V::Reference {
                evidence: actual, ..
            },
            V::Reference {
                evidence: expected, ..
            },
        ) = (&actual, &expected)
        else {
            panic!("projected loan required")
        };
        assert_eq!(actual, expected);
        assert_eq!(actual.origins.is_complete(), complete);
        assert!(c.get(handle).is_err() && c.get(position).is_err());
        // On an unaddressable candidate the original index is still usable by the checked path.
        let source = c.put(input);
        let past_end = c.put(V::U32(2));
        assert_eq!(
            unsafe { tellur_rt_v1_search_loan(&mut c, source, past_end, 70) },
            0
        );
        assert_eq!(c.take(past_end).unwrap(), V::U32(2));
        assert!(c.error.is_none());
        c.host.activations.enter("after").unwrap();
        let report = c
            .host
            .activations
            .failure("test", fault::Phase::Execution, "done");
        assert_eq!(report.activations.last().unwrap().id, 3);
    }
}

#[test]
fn bulk_vector_transfer_preserves_every_owned_element_and_bound() {
    let vector = |tokens: &[&str]| V::Construct {
        ty: "vector<handle>".into(),
        name: "vector".into(),
        fields: tokens
            .iter()
            .map(|token| V::Opaque {
                ty: "handle".into(),
                token: (*token).into(),
            })
            .collect(),
    };
    for (bound, expected_source, expected_destination) in [
        (None, vec![], vec!["left", "a", "b", "c"]),
        (Some(0), vec![], vec!["left", "a", "b", "c"]),
        (Some(1), vec!["a", "b", "c"], vec!["left"]),
        (Some(2), vec!["b", "c"], vec!["left", "a"]),
        (Some(8), vec![], vec!["left", "a", "b", "c"]),
    ] {
        let mut host = Host::new(json!({}), None, false, None);
        let mut c = context(&mut host);
        let source = c.put(vector(&["a", "b", "c"]));
        let destination = c.put(vector(&["left"]));
        let limit = bound.map_or(0, |bound| c.put(V::U32(bound)));
        unsafe {
            tellur_rt_v1_vector_transfer(&mut c, source, destination, limit);
        }
        assert!(c.error.is_none(), "{:?}", c.error);
        assert_eq!(c.take(source).unwrap(), vector(&expected_source));
        assert_eq!(c.take(destination).unwrap(), vector(&expected_destination));
        if limit != 0 {
            assert!(c.get(limit).is_err());
        }
    }
    // Fuel and observable modes execute the checked graph with the original owners.
    for (trace, fuel) in [(true, None), (false, Some(0)), (false, Some(100))] {
        let mut host = Host::new(json!({}), None, trace, fuel);
        let mut c = context(&mut host);
        let source = c.put(vector(&["a"]));
        let destination = c.put(vector(&[]));
        unsafe {
            tellur_rt_v1_vector_transfer(&mut c, source, destination, 0);
        }
        assert!(c.error.is_none());
        assert_eq!(c.take(source).unwrap(), vector(&["a"]));
        assert_eq!(c.take(destination).unwrap(), vector(&[]));
        assert_eq!(c.host.fuel, fuel);
    }
}

#[test]
fn contiguous_prefix_transfer_moves_nonclone_owners_once() {
    use std::{cell::RefCell, rc::Rc};
    #[repr(align(128))]
    struct Owner(usize, Rc<RefCell<Vec<usize>>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    for length in 0..33 {
        for count in 0..=length {
            for destination_length in [0, 1, 7] {
                for spare in [0, 64] {
                    let drops = Rc::new(RefCell::new(Vec::new()));
                    let mut source = Vec::with_capacity(length + spare);
                    source.extend((0..length).map(|id| Owner(id, drops.clone())));
                    let mut destination = Vec::with_capacity(destination_length + spare);
                    destination.extend(
                        (length..length + destination_length).map(|id| Owner(id, drops.clone())),
                    );
                    append_vector_prefix(&mut source, &mut destination, count);
                    assert_eq!(
                        source.iter().map(|v| v.0).collect::<Vec<_>>(),
                        (count..length).collect::<Vec<_>>()
                    );
                    assert_eq!(
                        destination.iter().map(|v| v.0).collect::<Vec<_>>(),
                        (length..length + destination_length)
                            .chain(0..count)
                            .collect::<Vec<_>>()
                    );
                    assert!(drops.borrow().is_empty());
                    drop(source);
                    drop(destination);
                    drops.borrow_mut().sort_unstable();
                    assert_eq!(
                        *drops.borrow(),
                        (0..length + destination_length).collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

#[test]
fn contiguous_prefix_transfer_handles_zero_size_and_invalid_count() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Empty;
    impl Drop for Empty {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut source: Vec<_> = (0..9).map(|_| Empty).collect();
    let mut destination: Vec<_> = (0..3).map(|_| Empty).collect();
    append_vector_prefix(&mut source, &mut destination, 4);
    assert_eq!((source.len(), destination.len()), (5, 7));
    assert_eq!(DROPS.load(Ordering::Relaxed), 0);
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        append_vector_prefix(&mut source, &mut destination, 6);
    }));
    assert!(rejected.is_err());
    assert_eq!((source.len(), destination.len()), (5, 7));
    append_vector_prefix(&mut source, &mut destination, 5);
    assert_eq!((source.len(), destination.len()), (0, 12));
    drop(source);
    drop(destination);
    assert_eq!(DROPS.load(Ordering::Relaxed), 12);
}

#[test]
fn direct_collection_length_matches_reference_projection_and_errors() {
    let vector = V::Construct {
        ty: "vector<handle>".into(),
        name: "vector".into(),
        fields: vec![V::Opaque {
            ty: "handle".into(),
            token: "owned".into(),
        }],
    };
    let nested = V::Construct {
        ty: "batch".into(),
        name: "batch".into(),
        fields: vec![vector.clone()],
    };
    let cases = [
        (Some(vector), 0, vec![]),
        (Some(nested.clone()), 0, vec![0]),
        (Some(nested.clone()), 0, vec![1]),
        (Some(nested), 1, vec![]),
        (None, 0, vec![]),
        (Some(V::U32(4)), 0, vec![0]),
        (Some(V::U32(4)), 0, vec![]),
        (
            Some(V::Construct {
                ty: "array<u32,2>".into(),
                name: "array".into(),
                fields: vec![V::U32(1), V::U32(2)],
            }),
            0,
            vec![],
        ),
        (
            Some(V::Construct {
                ty: "map<string,u32>".into(),
                name: "map".into(),
                fields: vec![V::String("a".into()), V::U32(1)],
            }),
            0,
            vec![],
        ),
    ];
    for (value, slot, path) in cases {
        let evaluate = |direct| {
            let mut host = Host::new(json!({}), None, false, None);
            let mut c = context(&mut host);
            c.frame = Some(c.storage.values_mut().enter(1).unwrap());
            let root = c.local(0).unwrap();
            c.storage.values_mut()[root] = value.clone();
            let result = unsafe {
                if direct {
                    tellur_rt_v1_local_length(&mut c, slot, path.as_ptr(), path.len())
                } else {
                    let ty = "&vector<handle>";
                    let reference = tellur_rt_v1_reference(
                        &mut c,
                        slot,
                        path.as_ptr(),
                        path.len(),
                        0,
                        ty.as_ptr(),
                        ty.len(),
                    );
                    tellur_rt_v1_length(&mut c, reference)
                }
            };
            assert_eq!(c.storage.values()[root], value);
            match c.error.clone() {
                Some(error) => Err(error),
                None => c.take(result),
            }
        };
        assert_eq!(
            evaluate(true),
            evaluate(false),
            "slot={slot}, path={path:?}"
        );
    }
}

#[test]
fn arithmetic_operand_guard_preserves_fallbacks_and_manifest_mutations() {
    for modern in [false, true] {
        for op in 0..4 {
            let name = ["checked_add", "checked_sub", "checked_mul", "checked_div"][op];
            let (params, outcomes, effects) = contract(name).unwrap();
            let primitive = json!({"name":name,"parameters":params.iter().map(|(ty, mode)|json!({"type":ty,"mode":mode})).collect::<Vec<_>>(),
                "outcomes":outcomes.iter().map(|(old,new,fields)|json!({"name":if modern {new} else {old},"fields":fields.iter().map(|ty|json!({"type":ty})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                "effects":effects,"returns":true});
            for case in [
                "live",
                "trace",
                "script",
                "fuel",
                "pure",
                "adapter",
                "async",
                "nonreturning",
                "contract",
                "duplicate",
                "stale",
                "type",
                "spelling",
            ] {
                let mut host = Host::new(
                    json!({"types":[],"primitives":[primitive.clone()]}),
                    None,
                    false,
                    None,
                );
                match case {
                    "trace" => host.trace = true,
                    "script" => host.script = Some(vec![]),
                    "fuel" => host.fuel = Some(100),
                    "pure" => host.manifest["primitives"][0]["pure"] = json!(true),
                    "adapter" => host.manifest["primitives"][0]["rust"] = json!({}),
                    "async" => host.manifest["primitives"][0]["async"] = json!(true),
                    "nonreturning" => host.manifest["primitives"][0]["returns"] = json!(false),
                    "contract" => host.manifest["primitives"][0]["effects"] = json!(["changed"]),
                    _ => {}
                }
                let mut c = context(&mut host);
                let left = c.put(V::U32(u32::MAX));
                let right = c.put(if case == "type" {
                    V::Bool(true)
                } else {
                    V::U32(1)
                });
                if case == "stale" {
                    c.take(right).unwrap();
                }
                let supplied = if case == "duplicate" { left } else { right };
                let spelling = if case == "spelling" { !modern } else { modern };
                let mut output = [123, 456];
                let selected = unsafe {
                    tellur_rt_v1_arithmetic_operands(
                        &mut c,
                        op as u32,
                        u32::from(spelling),
                        left,
                        supplied,
                        output.as_mut_ptr(),
                    )
                };
                assert_eq!(
                    selected,
                    u32::from(case == "live"),
                    "{name} {modern} {case}"
                );
                assert!(c.error.is_none());
                if case == "live" {
                    assert_eq!(output, [u32::MAX, 1]);
                    assert!(c.get(left).is_err() && c.get(right).is_err());
                    // A later manifest mutation must disable the same fast call.
                    c.host.manifest["primitives"][0]["effects"] = json!(["changed"]);
                    let args = [c.put(V::U32(1)), c.put(V::U32(2))];
                    assert_eq!(
                        unsafe {
                            tellur_rt_v1_arithmetic_operands(
                                &mut c,
                                op as u32,
                                u32::from(modern),
                                args[0],
                                args[1],
                                output.as_mut_ptr(),
                            )
                        },
                        0
                    );
                    assert!(args.iter().all(|h| c.get(*h).is_ok()));
                } else {
                    assert_eq!(output, [123, 456]);
                    assert!(c.get(left).is_ok());
                    assert_eq!(c.get(right).is_ok(), case != "stale");
                }
            }
        }
    }
}

#[test]
fn scalar_fold_prefix_updates_only_complete_advances_and_preserves_owners() {
    for (items, limit, start, initial, end, total) in [
        (vec![1, 2, 3], 10, 0, 0, 3, 6),
        (vec![1, 2, 3], 3, 0, 0, 1, 1),
        (vec![1, 0, 3], 10, 0, 0, 1, 1),
        (vec![u32::MAX - 1, 3], u32::MAX, 0, 0, 1, u32::MAX - 1),
        (vec![1, 2, 3], 10, 1, 4, 3, 9),
        (vec![], 10, 0, 4, 0, 4),
        (vec![1], 10, u32::MAX, 4, u32::MAX, 4),
    ] {
        for mode in ["live", "trace", "script", "fuel", "contract"] {
            let mut host = Host::new(
                primitive_manifest(),
                None,
                mode == "trace",
                if mode == "fuel" { Some(100) } else { None },
            );
            if mode == "script" {
                host.script = Some(vec![]);
            }
            if mode == "contract" {
                host.manifest["primitives"][0]["effects"] = json!(["changed"]);
            }
            let mut c = context(&mut host);
            let owner = V::Construct {
                ty: "vector<U32>".into(),
                name: "vector".into(),
                fields: items.iter().copied().map(V::U32).collect(),
            };
            unsafe {
                tellur_rt_v1_frame(&mut c, 4);
                let vector = c.put(owner.clone());
                tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, vector);
                let rule = c.put(V::Construct {
                    ty: "rule".into(),
                    name: "rule".into(),
                    fields: vec![V::U32(limit)],
                });
                tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, rule);
                for (slot, ty) in [(0, "&vector<U32>"), (1, "&rule")] {
                    let reference = tellur_rt_v1_reference(
                        &mut c,
                        slot,
                        std::ptr::null(),
                        0,
                        0,
                        ty.as_ptr(),
                        ty.len(),
                    );
                    tellur_rt_v1_store(&mut c, slot + 2, std::ptr::null(), 0, reference);
                }
                let position = c.put(V::U32(start));
                let state = c.put(V::U32(initial));
                tellur_rt_v1_fold_prefix(&mut c, 2, 3, position, state, 0, 0);
                assert_eq!(
                    c.get(position).unwrap(),
                    &V::U32(if mode == "live" { end } else { start })
                );
                assert_eq!(
                    c.get(state).unwrap(),
                    &V::U32(if mode == "live" { total } else { initial })
                );
                assert_eq!(
                    super::super::slot(c.storage.values(), c.local(0).unwrap()).unwrap(),
                    &owner
                );
                assert!(c.error.is_none());
                // Invalid/speculative projections leave the checked path responsible for errors.
                tellur_rt_v1_fold_prefix(&mut c, 2, 3, position, state, 99, 0);
                assert!(c.error.is_none());
            }
        }
    }
}

#[test]
fn scalar_partition_prefix_moves_stably_and_preserves_fallback_owners() {
    let vector = |fields| V::Construct {
        ty: "vector<U32>".into(),
        name: "vector".into(),
        fields,
    };
    for input in [
        vec![],
        vec![1, 4, 2, 3],
        vec![0, 2],
        vec![4, 1, 0, 2],
        vec![u32::MAX, 1],
    ] {
        for mode in [
            "live",
            "trace",
            "script",
            "fuel",
            "alias",
            "projection",
            "type",
        ] {
            let mut host = Host::new(
                json!({"primitives":[]}),
                None,
                mode == "trace",
                (mode == "fuel").then_some(100),
            );
            if mode == "script" {
                host.script = Some(vec![]);
            }
            let mut c = context(&mut host);
            unsafe {
                tellur_rt_v1_frame(&mut c, 2);
                let rule = c.put(V::Construct {
                    ty: "pivot".into(),
                    name: "pivot".into(),
                    fields: vec![V::U32(3)],
                });
                tellur_rt_v1_store(&mut c, 0, std::ptr::null(), 0, rule);
                let ty = "&pivot";
                let reference = tellur_rt_v1_reference(
                    &mut c,
                    0,
                    std::ptr::null(),
                    0,
                    0,
                    ty.as_ptr(),
                    ty.len(),
                );
                tellur_rt_v1_store(&mut c, 1, std::ptr::null(), 0, reference);
                let original = vector(input.iter().copied().map(V::U32).collect());
                let source = c.put(original.clone());
                let left = c.put(vector(vec![V::U32(8)]));
                let right_value = if mode == "type" {
                    V::U32(7)
                } else {
                    vector(vec![V::U32(9)])
                };
                let right = c.put(right_value.clone());
                tellur_rt_v1_partition_prefix(
                    &mut c,
                    source,
                    left,
                    if mode == "alias" { left } else { right },
                    1,
                    if mode == "projection" { 9 } else { 0 },
                    0,
                );
                if mode == "live" {
                    let end = input.iter().position(|&n| n == 0).unwrap_or(input.len());
                    assert_eq!(
                        c.get(source).unwrap(),
                        &vector(input[end..].iter().copied().map(V::U32).collect())
                    );
                    assert_eq!(
                        c.get(left).unwrap(),
                        &vector(
                            std::iter::once(8)
                                .chain(input[..end].iter().copied().filter(|&n| n < 3))
                                .map(V::U32)
                                .collect()
                        )
                    );
                    assert_eq!(
                        c.get(right).unwrap(),
                        &vector(
                            std::iter::once(9)
                                .chain(input[..end].iter().copied().filter(|&n| n >= 3))
                                .map(V::U32)
                                .collect()
                        )
                    );
                } else {
                    assert_eq!(c.get(source).unwrap(), &original);
                    assert_eq!(c.get(left).unwrap(), &vector(vec![V::U32(8)]));
                    assert_eq!(c.get(right).unwrap(), &right_value);
                }
                assert!(c.error.is_none());
            }
        }
    }
}

#[test]
fn stable_merge_prefix_preserves_owners_ties_and_fallback_state() {
    let record = |key, id| V::Construct {
        ty: "record".into(),
        name: "record".into(),
        fields: vec![V::U32(key), V::U32(id)],
    };
    let vector = |fields| V::Construct {
        ty: "vector<record>".into(),
        name: "vector".into(),
        fields,
    };
    for (a, b) in [
        (vec![], vec![1]),
        (vec![1], vec![]),
        (vec![1, 2, 2, 7], vec![0, 2, 4]),
        (vec![7, 8], vec![1, 2]),
        (vec![0, 1], vec![2, 3]),
        (vec![u32::MAX], vec![u32::MAX]),
    ] {
        for mode in [
            "live",
            "trace",
            "script",
            "fuel",
            "alias",
            "projection",
            "type",
            "opaque",
            "malformed",
        ] {
            let mut host = Host::new(
                json!({"primitives":[]}),
                None,
                mode == "trace",
                (mode == "fuel").then_some(100),
            );
            if mode == "script" {
                host.script = Some(vec![]);
            }
            let mut c = context(&mut host);
            let av: Vec<_> = a
                .iter()
                .enumerate()
                .map(|(i, &k)| record(k, i as u32))
                .collect();
            let mut bv: Vec<_> = b
                .iter()
                .enumerate()
                .map(|(i, &k)| record(k, 100 + i as u32))
                .collect();
            if !bv.is_empty() && matches!(mode, "opaque" | "malformed") {
                if let V::Construct { fields, .. } = &mut bv[0] {
                    if mode == "opaque" {
                        fields.push(V::Opaque {
                            ty: "handle".into(),
                            token: "owned".into(),
                        });
                    } else {
                        fields[0] = V::String("bad key".into());
                    }
                }
            }
            let original_a = vector(av.clone());
            let original_b = vector(bv.clone());
            let initial = if mode == "type" {
                V::U32(8)
            } else {
                vector(vec![record(99, 999)])
            };
            let left = c.put(original_a.clone());
            let right = c.put(original_b.clone());
            let result = c.put(initial.clone());
            unsafe {
                tellur_rt_v1_merge_prefix(
                    &mut c,
                    left,
                    if mode == "alias" { left } else { right },
                    result,
                    if mode == "projection" { 4 } else { 0 },
                );
            }
            if mode == "live" {
                let (mut i, mut j) = (0, 0);
                let mut expected = vec![record(99, 999)];
                while i < a.len() && j < b.len() {
                    if a[i] <= b[j] {
                        expected.push(av[i].clone());
                        i += 1;
                    } else {
                        expected.push(bv[j].clone());
                        j += 1;
                    }
                }
                assert_eq!(c.get(left).unwrap(), &vector(av[i..].to_vec()));
                assert_eq!(c.get(right).unwrap(), &vector(bv[j..].to_vec()));
                assert_eq!(c.get(result).unwrap(), &vector(expected));
            } else {
                assert_eq!(c.get(left).unwrap(), &original_a);
                assert_eq!(c.get(right).unwrap(), &original_b);
                assert_eq!(c.get(result).unwrap(), &initial);
            }
            assert!(c.error.is_none());
        }
    }
}

#[test]
fn singleton_runs_transfer_resources_once_and_preserve_fallback_owners() {
    use std::{cell::Cell, rc::Rc};
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for n in [0, 1, 7] {
        for mode in [
            "live",
            "trace",
            "script",
            "fuel",
            "alias",
            "type",
            "constructor",
        ] {
            let drops = Rc::new(Cell::new(0));
            let mut host = Host::new(
                json!({}),
                None,
                mode == "trace",
                (mode == "fuel").then_some(100),
            );
            if mode == "script" {
                host.script = Some(vec![]);
            }
            let mut c = context(&mut host);
            let values: Vec<_> = (0..n)
                .map(|_| {
                    c.host
                        .resources
                        .insert("probe", Owner(drops.clone()))
                        .unwrap()
                })
                .collect();
            let first = c
                .host
                .resources
                .insert("probe", Owner(drops.clone()))
                .unwrap();
            let vector = |fields| V::Construct {
                ty: "vector<probe>".into(),
                name: "vector".into(),
                fields,
            };
            let nested = |fields| V::Construct {
                ty: if mode == "type" {
                    "vector<vector<wrong>>"
                } else {
                    "vector<vector<probe>>"
                }
                .into(),
                name: "vector".into(),
                fields,
            };
            let source = c.put(vector(values.clone()));
            let initial = vec![vector(vec![first.clone()])];
            let destination = c.put(nested(initial.clone()));
            let ty = if mode == "constructor" {
                "vector<wrong>"
            } else {
                "vector<probe>"
            };
            unsafe {
                tellur_rt_v1_singletons(
                    &mut c,
                    source,
                    if mode == "alias" { source } else { destination },
                    ty.as_ptr(),
                    ty.len(),
                );
            }
            assert!(c.error.is_none());
            assert_eq!(drops.get(), 0);
            if mode == "live" {
                assert_eq!(c.get(source).unwrap(), &vector(vec![]));
                let expected = initial
                    .into_iter()
                    .chain(values.iter().cloned().map(|v| vector(vec![v])))
                    .collect();
                assert_eq!(c.get(destination).unwrap(), &nested(expected));
            } else {
                assert_eq!(c.get(source).unwrap(), &vector(values));
                assert_eq!(c.get(destination).unwrap(), &nested(initial));
            }
            let V::Construct {
                fields: mut owners, ..
            } = c.take(source).unwrap()
            else {
                panic!()
            };
            let V::Construct { fields: runs, .. } = c.take(destination).unwrap() else {
                panic!()
            };
            for run in runs {
                let V::Construct { fields, .. } = run else {
                    panic!()
                };
                owners.extend(fields);
            }
            assert_eq!(owners.len(), n + 1);
            for owner in owners {
                drop(c.host.resources.take::<Owner>(&owner, "probe").unwrap());
            }
            assert_eq!(drops.get(), n + 1);
            assert!(c.host.resources.get::<Owner>(&first, "probe").is_err());
        }
    }
    // Empty records remain distinct elements even though they carry no fields.
    let mut host = Host::new(json!({}), None, false, None);
    let mut c = context(&mut host);
    let empty = V::Construct {
        ty: "unit".into(),
        name: "unit".into(),
        fields: vec![],
    };
    let vector = |fields| V::Construct {
        ty: "vector<unit>".into(),
        name: "vector".into(),
        fields,
    };
    let source = c.put(vector(vec![empty.clone(); 3]));
    let destination = c.put(V::Construct {
        ty: "vector<vector<unit>>".into(),
        name: "vector".into(),
        fields: vec![],
    });
    let ty = "vector<unit>";
    unsafe {
        tellur_rt_v1_singletons(&mut c, source, destination, ty.as_ptr(), ty.len());
    }
    assert_eq!(c.get(source).unwrap(), &vector(vec![]));
    let V::Construct { fields, .. } = c.take(destination).unwrap() else {
        panic!()
    };
    assert_eq!(fields, vec![vector(vec![empty]); 3]);
}

#[test]
fn singleton_runs_compact_fixed_fields_and_preserve_collection_reserve() {
    use std::{cell::Cell, rc::Rc};
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for (ty, name, compact) in [
        ("record", "record", true),
        ("array<probe,1>", "array", true),
        ("vector<probe>", "vector", false),
        ("map<string,probe>", "map", false),
    ] {
        let drops = Rc::new(Cell::new(0));
        let mut host = Host::new(json!({}), None, false, None);
        let resource = host
            .resources
            .insert("probe", Owner(drops.clone()))
            .unwrap();
        let mut fields = Vec::with_capacity(8);
        if name == "map" {
            fields.push(V::String("key".into()));
        }
        fields.push(resource.clone());
        let length = fields.len();
        let reserve = fields.capacity();
        let mut c = context(&mut host);
        let source_ty = format!("vector<{ty}>");
        let source = c.put(V::Construct {
            ty: source_ty.clone(),
            name: "vector".into(),
            fields: vec![V::Construct {
                ty: ty.into(),
                name: name.into(),
                fields,
            }],
        });
        let destination = c.put(V::Construct {
            ty: format!("vector<{source_ty}>"),
            name: "vector".into(),
            fields: vec![],
        });
        unsafe {
            tellur_rt_v1_singletons(
                &mut c,
                source,
                destination,
                source_ty.as_ptr(),
                source_ty.len(),
            );
        }
        assert!(c.error.is_none());
        assert_eq!(drops.get(), 0);
        let V::Construct { fields: runs, .. } = c.take(destination).unwrap() else {
            panic!()
        };
        let V::Construct { fields: run, .. } = &runs[0] else {
            panic!()
        };
        let V::Construct { fields, .. } = &run[0] else {
            panic!()
        };
        assert_eq!(fields.len(), length);
        assert_eq!(fields.capacity(), if compact { length } else { reserve });
        assert_eq!(fields.last(), Some(&resource));
        drop(c.host.resources.take::<Owner>(&resource, "probe").unwrap());
        assert_eq!(drops.get(), 1);
    }
}

#[test]
fn summarized_join_moves_resources_once_and_falls_back_without_partial_transfers() {
    use std::{cell::Cell, rc::Rc};
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let vector = |ty: &str, fields| V::Construct {
        ty: ty.into(),
        name: "vector".into(),
        fields,
    };
    for destination in [0, 1] {
        for (a, b) in [(0, 0), (0, 3), (3, 0), (2, 3)] {
            let drops = Rc::new(Cell::new(0));
            let mut host = Host::new(json!({}), None, false, None);
            host.enter("root").unwrap();
            let mut c = context(&mut host);
            let owners: Vec<_> = (0..a + b)
                .map(|_| {
                    c.host
                        .resources
                        .insert("probe", Owner(drops.clone()))
                        .unwrap()
                })
                .collect();
            let left = c.put(vector("vector<probe>", owners[..a].to_vec()));
            let right = c.put(vector("vector<probe>", owners[a..].to_vec()));
            let args = if destination == 0 {
                [left, right]
            } else {
                [right, left]
            };
            let result = unsafe {
                tellur_rt_v1_join_call(
                    &mut c,
                    echo,
                    b"arbitrary".as_ptr(),
                    9,
                    args.as_ptr(),
                    2,
                    destination,
                    b"combined".as_ptr(),
                    8,
                )
            };
            assert!(c.error.is_none());
            assert_eq!(drops.get(), 0);
            let (name, values) = c.outcome(result).unwrap();
            assert_eq!(name, "combined");
            let [V::Construct { fields, .. }] = values.as_slice() else {
                panic!()
            };
            assert_eq!(fields, &owners);
            assert!(c.get(left).is_err() && c.get(right).is_err());
            for owner in owners {
                drop(c.host.resources.take::<Owner>(&owner, "probe").unwrap());
            }
            assert_eq!(drops.get(), a + b);
            c.host.enter("after").unwrap();
            let report = c
                .host
                .activations
                .failure("test", fault::Phase::Execution, "done");
            assert_eq!(report.activations.last().unwrap().id, 3);
        }
    }
    for mode in [
        "trace",
        "script",
        "fuel",
        "type",
        "shape",
        "alias",
        "stale",
        "destination",
        "count",
        "outcome_limit",
    ] {
        let mut host = Host::new(
            json!({}),
            None,
            mode == "trace",
            (mode == "fuel").then_some(50),
        );
        if mode == "script" {
            host.script = Some(vec![]);
        }
        let mut c = context(&mut host);
        let left = c.put(vector("vector<U32>", vec![V::U32(7)]));
        let right = c.put(if mode == "shape" {
            V::U32(8)
        } else {
            vector(
                if mode == "type" {
                    "vector<Bool>"
                } else {
                    "vector<U32>"
                },
                vec![],
            )
        });
        if mode == "outcome_limit" {
            c.outcome_base = u64::MAX;
        }
        let args = [
            left,
            if mode == "alias" {
                left
            } else if mode == "stale" {
                999
            } else {
                right
            },
        ];
        let result = unsafe {
            tellur_rt_v1_join_call(
                &mut c,
                echo,
                b"join".as_ptr(),
                4,
                args.as_ptr(),
                if mode == "count" { 1 } else { 2 },
                if mode == "destination" { 2 } else { 0 },
                b"combined".as_ptr(),
                8,
            )
        };
        if matches!(mode, "alias" | "stale" | "outcome_limit") {
            assert_eq!(result, 0);
            assert!(c.error.is_some());
            assert!(c.get(left).is_ok() && c.get(right).is_ok());
        } else {
            assert!(c.error.is_none(), "{mode}: {:?}", c.error);
            let (name, values) = c.outcome(result).unwrap();
            assert_eq!(name, "ready", "{mode}");
            assert_eq!(values.len(), if mode == "count" { 1 } else { 2 });
        }
    }
}

#[test]
fn owned_vector_take_preserves_resources_and_checked_fallbacks() {
    use std::{cell::Cell, rc::Rc};
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for observed in [false, true] {
        for size in [0, 1, 5] {
            let drops = Rc::new(Cell::new(0));
            let mut host = Host::new(json!({}), None, observed, None);
            let mut c = context(&mut host);
            let owners: Vec<_> = (0..size)
                .map(|_| {
                    c.host
                        .resources
                        .insert("ticket", Owner(drops.clone()))
                        .unwrap()
                })
                .collect();
            let mut value = V::Construct {
                ty: "vector<ticket>".into(),
                name: "vector".into(),
                fields: owners.clone(),
            };
            for expected in owners.iter().map(Some).chain(std::iter::once(None)) {
                let handle = c.put(value);
                let result = unsafe {
                    tellur_rt_v1_collection(&mut c, b"vector_take".as_ptr(), 11, &handle, 1)
                };
                assert!(c.error.is_none(), "{:?}", c.error);
                assert!(c.get(handle).is_err());
                assert_eq!(drops.get(), 0);
                let (name, fields) = c.outcome(result).unwrap();
                if let Some(expected) = expected {
                    assert_eq!(name, "item");
                    assert_eq!(&fields[0], expected);
                    value = fields[1].clone();
                } else {
                    assert_eq!(name, "empty");
                    assert!(fields.is_empty());
                    break;
                }
            }
            for owner in owners {
                drop(c.host.resources.take::<Owner>(&owner, "ticket").unwrap());
            }
            assert_eq!(drops.get(), size);
        }
    }
    // Compare native success/error and residual ownership to the shared path.
    for (ty, name, fields) in [
        ("vector<U32>", "vector", vec![V::U32(2), V::U32(7)]),
        (
            "vector<vector<U32>>",
            "vector",
            vec![V::Construct {
                ty: "vector<U32>".into(),
                name: "vector".into(),
                fields: vec![],
            }],
        ),
        ("vector< U32 >", "vector", vec![]),
        ("vector<U32>", "different", vec![V::U32(1)]),
        ("array<U32,1>", "array", vec![V::U32(1)]),
        ("vector<>", "vector", vec![]),
        ("vector<U32,Bool>", "vector", vec![]),
        ("vector<U32>>", "vector", vec![]),
    ] {
        for mode in [
            "ordinary",
            "stale",
            "extra",
            "aliased",
            "outcome_limit",
            "script",
            "fuel",
        ] {
            let evaluate = |trace| {
                let mut host = Host::new(json!({}), None, trace, (mode == "fuel").then_some(20));
                if mode == "script" {
                    host.script = Some(vec![]);
                }
                let mut c = context(&mut host);
                let handle = c.put(V::Construct {
                    ty: ty.into(),
                    name: name.into(),
                    fields: fields.clone(),
                });
                let extra = c.put(V::U32(99));
                if mode == "outcome_limit" {
                    c.outcome_base = u64::MAX;
                }
                let arguments = [
                    if mode == "stale" { 999 } else { handle },
                    if mode == "aliased" { handle } else { extra },
                ];
                let count = if matches!(mode, "extra" | "aliased") {
                    2
                } else {
                    1
                };
                let outcome = unsafe {
                    tellur_rt_v1_collection(
                        &mut c,
                        b"vector_take".as_ptr(),
                        11,
                        arguments.as_ptr(),
                        count,
                    )
                };
                (
                    c.error.clone(),
                    c.outcome(outcome).ok().cloned(),
                    c.get(handle).ok().cloned(),
                    c.get(extra).ok().cloned(),
                )
            };
            assert_eq!(evaluate(false), evaluate(true), "{ty} {name} {mode}");
        }
    }
}

#[test]
fn vector_reversal_preserves_resource_identity_and_checked_fallbacks() {
    use std::{cell::Cell, rc::Rc};
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for n in [0, 1, 7] {
        for mode in [
            "live",
            "trace",
            "script",
            "fuel",
            "alias",
            "type",
            "malformed",
            "constructor",
            "stale",
        ] {
            let drops = Rc::new(Cell::new(0));
            let mut host = Host::new(
                json!({}),
                None,
                mode == "trace",
                (mode == "fuel").then_some(100),
            );
            if mode == "script" {
                host.script = Some(vec![]);
            }
            let mut c = context(&mut host);
            let values: Vec<_> = (0..n)
                .map(|_| {
                    c.host
                        .resources
                        .insert("probe", Owner(drops.clone()))
                        .unwrap()
                })
                .collect();
            let first = c
                .host
                .resources
                .insert("probe", Owner(drops.clone()))
                .unwrap();
            let vector = |fields| V::Construct {
                ty: if mode == "malformed" {
                    "vector<probe,wrong>"
                } else {
                    "vector<probe>"
                }
                .into(),
                name: if mode == "constructor" {
                    "wrong"
                } else {
                    "vector"
                }
                .into(),
                fields,
            };
            let source = c.put(vector(values.clone()));
            let mut initial = vector(vec![first.clone()]);
            if mode == "type" {
                let V::Construct { ty, .. } = &mut initial else {
                    panic!()
                };
                *ty = "vector<other>".into();
            }
            let destination = c.put(initial.clone());
            let stale = c.put(V::U32(1));
            c.take(stale).unwrap();
            unsafe {
                tellur_rt_v1_vector_reverse(
                    &mut c,
                    if mode == "stale" { stale } else { source },
                    if mode == "alias" { source } else { destination },
                );
            }
            assert_eq!(
                c.error.is_some(),
                matches!(mode, "alias" | "stale"),
                "{mode}: {:?}",
                c.error
            );
            assert_eq!(drops.get(), 0);
            if mode == "live" {
                assert_eq!(c.get(source).unwrap(), &vector(vec![]));
                assert_eq!(
                    c.get(destination).unwrap(),
                    &vector(
                        std::iter::once(first.clone())
                            .chain(values.iter().rev().cloned())
                            .collect()
                    )
                );
            } else {
                assert_eq!(c.get(source).unwrap(), &vector(values));
                assert_eq!(c.get(destination).unwrap(), &initial);
            }
            let V::Construct {
                fields: mut owners, ..
            } = c.take(source).unwrap()
            else {
                panic!()
            };
            let V::Construct { fields, .. } = c.take(destination).unwrap() else {
                panic!()
            };
            owners.extend(fields);
            assert_eq!(owners.len(), n + 1);
            for owner in owners {
                drop(c.host.resources.take::<Owner>(&owner, "probe").unwrap());
            }
            assert_eq!(drops.get(), n + 1);
            assert!(c.host.resources.get::<Owner>(&first, "probe").is_err());
        }
    }
}
