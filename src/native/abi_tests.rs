use super::*;

fn context(host: &mut Host) -> Context<'_> {
    Context {
        host,
        storage: FrameStorage::Owned(Frames::default()),
        frame: None,
        values: vec![],
        outcomes: vec![],
        error: None,
        result: None,
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
        "borrow",
        "async",
        "nonreturning",
    ] {
        let mut manifest = primitive_manifest();
        match case {
            "borrow" => manifest["primitives"][0]["parameters"][0]["mode"] = json!("borrow"),
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
