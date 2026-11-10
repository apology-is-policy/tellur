use super::*;

#[test]
fn value_transfers_validate_the_whole_batch_before_consumption() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut c = Context {
        host: &mut host,
        values: vec![],
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
