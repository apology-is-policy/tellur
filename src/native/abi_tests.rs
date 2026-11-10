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
