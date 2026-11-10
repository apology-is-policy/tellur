use super::*;

fn context(host: &mut Host) -> Context<'_> {
    Context {
        host,
        values: vec![],
        outcomes: vec![],
        error: None,
        result: None,
    }
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
    let mut c = Context {
        host: &mut host,
        values: vec![],
        outcomes: vec![],
        error: None,
        result: None,
    };
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
    let mut c = Context {
        host: &mut host,
        values: vec![],
        outcomes: vec![],
        error: None,
        result: None,
    };
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
    let mut c = Context {
        host: &mut host,
        values: vec![],
        outcomes: vec![],
        error: None,
        result: None,
    };
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
