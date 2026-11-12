use super::*;
use std::ptr::{null, null_mut};

fn manifest(ty: &str) -> Json {
    json!({"version":"0.1","types":[],"rust":{"source":"host.rs","lockfile":"Cargo.lock"},
        "primitives":[{"name":"inspect","parameters":[{"name":"input","type":ty,"mode":"borrow"}],
        "outcomes":[{"name":"ready","fields":[{"name":"output","type":ty}]}],
        "effects":[],"returns":true,"rust":{"execution":"immediate"}}]})
}
fn descriptor(call: AdapterCallback) -> AdapterDescriptor {
    AdapterDescriptor {
        version: 1,
        flags: 0,
        call: Some(call),
    }
}
unsafe extern "C" fn echo(c: *mut AdapterContext, args: *const u64, count: usize) {
    unsafe { tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, args, count) }
}
fn binding(ty: &str, callback: AdapterCallback) -> AdapterBinding {
    AdapterBinding::new(&manifest(ty), "inspect", descriptor(callback)).unwrap()
}
fn context(ty: &str, values: Vec<V>) -> (AdapterContext<'static>, Vec<u64>) {
    let m = manifest(ty);
    let handles = reserve_handles(values.len()).unwrap();
    (
        AdapterContext {
            signature: m["primitives"][0].clone(),
            manifest: m,
            values: handles.iter().copied().zip(values).collect(),
            result: None,
            error: None,
            resources: None,
            owner: None,
            owned: BTreeSet::new(),
        },
        handles,
    )
}
fn vector(fields: Vec<V>) -> V {
    V::Construct {
        ty: "vector<String>".into(),
        name: "vector".into(),
        fields,
    }
}
#[test]
fn descriptors_and_inputs_preflight_before_argument_transfer() {
    let m = manifest("String");
    for d in [
        AdapterDescriptor {
            version: 0,
            ..descriptor(echo)
        },
        AdapterDescriptor {
            version: 2,
            ..descriptor(echo)
        },
        AdapterDescriptor {
            flags: 1,
            ..descriptor(echo)
        },
        AdapterDescriptor {
            call: None,
            ..descriptor(echo)
        },
    ] {
        assert!(AdapterBinding::new(&m, "inspect", d).is_err());
    }
    assert!(AdapterBinding::new(&m, "missing", descriptor(echo)).is_err());
    for mode in 0..7 {
        let mut m = m.clone();
        match mode {
            0 => m["primitives"][0]["async"] = json!(true),
            1 => m["primitives"][0]["rust"]["execution"] = json!("poll"),
            2 => m["primitives"][0]["pure"] = json!(true),
            3 => m["primitives"][0]["parameters"][0]["mode"] = json!("exclusive"),
            4 => m["primitives"][0]["parameters"][0]["type"] = json!("file"),
            5 => m["primitives"][0]["outcomes"][0]["fields"][0]["type"] = json!("&String"),
            _ => m["rust"] = Json::Null,
        }
        assert!(
            AdapterBinding::new(&m, "inspect", descriptor(echo)).is_err(),
            "{mode}"
        );
    }
    let b = binding("String", echo);
    for mut args in [
        vec![],
        vec![V::Bool(true)],
        vec![V::String("kept".into()), V::Unit],
    ] {
        let before = serde_json::to_value(&args).unwrap();
        assert!(b.invoke(&mut args).is_err());
        assert_eq!(serde_json::to_value(&args).unwrap(), before);
    }
}
#[test]
fn transferred_owned_buffers_preserve_identity_and_selected_payloads() {
    for ty in [
        "String",
        "Bytes",
        "vector<String>",
        "map<String,vector<String>>",
    ] {
        let text = "owned destination".repeat(200);
        let allocation = text.as_ptr();
        let mut value = V::String(text);
        if ty == "Bytes" {
            let V::String(s) = value else { unreachable!() };
            value = V::Bytes(s.into_bytes());
        }
        if ty == "vector<String>" {
            value = vector(vec![value]);
        }
        if ty.starts_with("map") {
            value = V::Construct {
                ty: ty.into(),
                name: "map".into(),
                fields: vec![V::String("key".into()), vector(vec![value])],
            };
        }
        let mut args = vec![value];
        let (name, mut result) = binding(ty, echo).invoke(&mut args).unwrap();
        assert_eq!(name, "ready");
        assert!(args.is_empty());
        fn pointer(v: &V) -> *const u8 {
            match v {
                V::String(s) => s.as_ptr(),
                V::Bytes(b) => b.as_ptr(),
                V::Construct { fields, .. } => pointer(fields.last().unwrap()),
                _ => panic!(),
            }
        }
        assert_eq!(pointer(&result.remove(0)), allocation);
    }
}
#[test]
fn scalar_and_byte_access_copy_only_into_caller_storage() {
    for value in [
        V::Unit,
        V::Bool(true),
        V::U32(u32::MAX),
        V::String("é\0🦀".into()),
        V::Bytes(vec![0, 255, 1]),
        V::String(String::new()),
    ] {
        let ty = value.ty().to_owned();
        let (mut c, hs) = context(&ty, vec![value]);
        let h = hs[0];
        unsafe {
            let tag = tellur_adapter_v1_kind(&mut c, h);
            let recreated = if tag < 3 {
                let n = tellur_adapter_v1_number(&mut c, h, tag);
                tellur_adapter_v1_value(&mut c, tag, n, null(), 0)
            } else {
                let len = tellur_adapter_v1_bytes(&mut c, h, tag, null_mut(), 0);
                let mut bytes = vec![0; len];
                assert_eq!(
                    tellur_adapter_v1_bytes(&mut c, h, tag, bytes.as_mut_ptr(), len),
                    len
                );
                tellur_adapter_v1_value(&mut c, tag, 0, bytes.as_ptr(), len)
            };
            assert_eq!(
                serde_json::to_value(c.get(h).unwrap()).unwrap(),
                serde_json::to_value(c.get(recreated).unwrap()).unwrap()
            );
            tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, &recreated, 1);
            assert_eq!(tellur_adapter_v1_ok(&mut c), 1);
        }
    }
}
#[test]
fn collections_unpack_repack_and_validate_sorted_unique_map_keys() {
    let (mut c, hs) = context(
        "vector<String>",
        vec![vector(vec![V::String("a".into()), V::String("b".into())])],
    );
    unsafe {
        assert_eq!(tellur_adapter_v1_kind(&mut c, hs[0]), 5);
        assert_eq!(tellur_adapter_v1_length(&mut c, hs[0]), 2);
        let mut fields = [0; 2];
        tellur_adapter_v1_unpack(&mut c, hs[0], fields.as_mut_ptr(), 2);
        assert!(c.get(hs[0]).is_err());
        let value = tellur_adapter_v1_collection(
            &mut c,
            b"vector<String>".as_ptr(),
            14,
            fields.as_ptr(),
            2,
        );
        assert!(fields.iter().all(|h| c.get(*h).is_err()));
        tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, &value, 1);
        assert!(c.error.is_none() && c.result.is_some());
    }
    for keys in [["z", "a"], ["a", "a"]] {
        let (mut c, hs) = context(
            "map<String,U32>",
            vec![
                V::String(keys[0].into()),
                V::U32(1),
                V::String(keys[1].into()),
                V::U32(2),
            ],
        );
        unsafe {
            assert_eq!(
                tellur_adapter_v1_collection(
                    &mut c,
                    b"map<String,U32>".as_ptr(),
                    15,
                    hs.as_ptr(),
                    4
                ),
                0
            );
        }
        assert!(c.error.as_ref().unwrap().contains("unique and sorted"));
        assert!(hs.iter().all(|h| c.get(*h).is_ok()));
    }
}
#[test]
fn malformed_spans_handles_and_payloads_fail_without_partial_mutation() {
    for mode in 0..12 {
        let (mut c, hs) = context("String", vec![V::String("kept".into())]);
        let h = hs[0];
        let mut bytes = [99; 2];
        unsafe {
            match mode {
                0 => {
                    tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, [h, h].as_ptr(), 2);
                }
                1 => {
                    tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, null(), 1);
                }
                2 => {
                    tellur_adapter_v1_complete(
                        &mut c,
                        b"ready".as_ptr(),
                        5,
                        std::ptr::dangling::<u8>().cast(),
                        1,
                    );
                }
                3 => {
                    tellur_adapter_v1_complete(&mut c, b"missing".as_ptr(), 7, &h, 1);
                }
                4 => {
                    tellur_adapter_v1_complete(&mut c, [255].as_ptr(), 1, &h, 1);
                }
                5 => {
                    tellur_adapter_v1_number(&mut c, h, 2);
                }
                6 => {
                    tellur_adapter_v1_bytes(&mut c, h, 3, bytes.as_mut_ptr(), 2);
                }
                7 => {
                    tellur_adapter_v1_value(&mut c, 3, 0, [255].as_ptr(), 1);
                }
                8 => {
                    tellur_adapter_v1_value(&mut c, 2, u64::MAX, null(), 0);
                }
                9 => {
                    tellur_adapter_v1_kind(&mut c, 0);
                }
                10 => {
                    let wrong = tellur_adapter_v1_value(&mut c, 2, 1, null(), 0);
                    tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, &wrong, 1);
                }
                _ => {
                    tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, null(), 0);
                }
            }
            assert_eq!(tellur_adapter_v1_ok(&mut c), 0, "{mode}");
            let first = c.error.clone();
            tellur_adapter_v1_fail(&mut c, b"later".as_ptr(), 5);
            tellur_adapter_v1_drop(&mut c, h);
            assert_eq!(c.error, first);
            assert!(c.get(h).is_ok());
            assert_eq!(bytes, [99; 2]);
            assert!(c.result.is_none());
        }
    }
    let (mut c, hs) = context(
        "vector<String>",
        vec![vector(vec![V::String("kept".into())])],
    );
    let mut output = [42; 2];
    unsafe {
        tellur_adapter_v1_unpack(&mut c, hs[0], output.as_mut_ptr(), 2);
    }
    assert_eq!(output, [42; 2]);
    assert!(c.get(hs[0]).is_ok());
}
#[test]
fn handles_cannot_cross_calls_and_concurrent_bindings_stay_independent() {
    let (mut first, a) = context("String", vec![V::String("first".into())]);
    let (mut second, b) = context("String", vec![V::String("second".into())]);
    assert_ne!(a, b);
    unsafe {
        tellur_adapter_v1_drop(&mut first, a[0]);
        tellur_adapter_v1_kind(&mut first, a[0]);
        tellur_adapter_v1_kind(&mut second, a[0]);
    }
    assert!(first.error.is_some() && second.error.is_some());
    assert!(second.get(b[0]).is_ok());
    let threads = (0..12)
        .map(|i| {
            std::thread::spawn(move || {
                let b = binding("U32", echo);
                for _ in 0..20 {
                    let (_, fields) = b.invoke(&mut vec![V::U32(i)]).unwrap();
                    assert!(matches!(fields.as_slice(),[V::U32(n)] if *n==i));
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
}
fn rust_dispatch(_: &mut Resources, _: &str, args: Vec<V>) -> R<Outcome> {
    Ok(("ready".into(), args))
}
fn foreign_dispatch(_: &mut Resources, _: &str, mut args: Vec<V>) -> R<Outcome> {
    binding("String", echo).invoke(&mut args)
}
fn never_dispatch(_: &mut Resources, _: &str, _: Vec<V>) -> R<Outcome> {
    panic!("script must bypass foreign dispatch")
}
#[test]
fn original_host_preserves_script_precedence_events_fuel_and_call_outcomes() {
    for scripted in [false, true] {
        let mut results = vec![];
        for dispatch in [
            rust_dispatch as RustAdapter,
            foreign_dispatch,
            never_dispatch,
        ] {
            if !scripted && std::ptr::fn_addr_eq(dispatch, never_dispatch as RustAdapter) {
                continue;
            }
            let calls = scripted.then(|| {
                vec![ScriptCall {
                    operation: "inspect".into(),
                    outcome: "ready".into(),
                    fields: vec![V::String("scripted".into())],
                    waits: 0,
                }]
            });
            let mut host = Host::new(manifest("String"), calls, true, Some(3));
            host.rust_adapter = Some(dispatch);
            host.enter("main").unwrap();
            host.tick(1, 1).unwrap();
            let outcome = host
                .primitive("inspect", vec![V::String("live".into())])
                .unwrap();
            host.leave();
            results.push((
                serde_json::to_value(outcome).unwrap(),
                host.events.clone(),
                host.cursor,
                host.fuel,
            ));
        }
        for result in &results[1..] {
            assert_eq!(result, &results[0]);
        }
    }
}
#[test]
fn fallthrough_explicit_failure_and_late_calls_never_become_source_faults() {
    unsafe extern "C" fn fallthrough(_: *mut AdapterContext, _: *const u64, _: usize) {}
    unsafe extern "C" fn fail(c: *mut AdapterContext, _: *const u64, _: usize) {
        unsafe { tellur_adapter_v1_fail(c, b"E_FAULT: foreign".as_ptr(), 16) }
    }
    unsafe extern "C" fn late(c: *mut AdapterContext, a: *const u64, n: usize) {
        unsafe {
            echo(c, a, n);
            tellur_adapter_v1_drop(c, *a)
        }
    }
    for callback in [fallthrough as AdapterCallback, fail, late] {
        let error = binding("String", callback)
            .invoke(&mut vec![V::String("input".into())])
            .unwrap_err();
        assert!(error.starts_with("E_HOST:"));
    }
}
#[test]
fn foreign_panics_remain_fatal() {
    const ENV: &str = "TELLUR_FOREIGN_ADAPTER_PANIC_PROBE";
    unsafe extern "C" fn panic_callback(_: *mut AdapterContext, _: *const u64, _: usize) {
        panic!("foreign adapter fatal probe")
    }
    if std::env::var_os(ENV).is_some() {
        let _ = binding("String", panic_callback).invoke(&mut vec![V::String("input".into())]);
        panic!("foreign panic returned");
    }
    let name = format!(
        "{}::foreign_panics_remain_fatal",
        module_path!().split_once("::").unwrap().1
    );
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture"])
        .env(ENV, "1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("foreign adapter fatal probe"));
}

#[test]
fn multiple_inputs_select_exact_alternatives_and_payload_order() {
    unsafe extern "C" fn select(c: *mut AdapterContext, args: *const u64, count: usize) {
        unsafe {
            assert_eq!(count, 3);
            let args = slice(args, count);
            if tellur_adapter_v1_number(c, args[0], 1) == 1 {
                let fields = [args[2], args[1]];
                tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, fields.as_ptr(), 2);
            } else {
                tellur_adapter_v1_complete(c, b"empty".as_ptr(), 5, null(), 0);
            }
        }
    }
    let mut m = manifest("String");
    m["primitives"][0]["parameters"] = json!([
        {"name":"choose","type":"Bool","mode":"value"},
        {"name":"text","type":"String","mode":"consume"},
        {"name":"number","type":"U32","mode":"value"}]);
    m["primitives"][0]["outcomes"] = json!([
        {"name":"ready","fields":[{"name":"number","type":"U32"},{"name":"text","type":"String"}]},
        {"name":"empty","fields":[]}]);
    for choice in [false, true] {
        let (name, fields) = AdapterBinding::new(&m, "inspect", descriptor(select))
            .unwrap()
            .invoke(&mut vec![
                V::Bool(choice),
                V::String("ordered".into()),
                V::U32(17),
            ])
            .unwrap();
        if choice {
            assert_eq!(name, "ready");
            assert!(matches!(fields.as_slice(), [V::U32(17),V::String(s)] if s=="ordered"));
        } else {
            assert_eq!(name, "empty");
            assert!(fields.is_empty());
        }
    }
}
#[test]
fn identity_exhaustion_is_atomic_and_foreign_errors_bypass_source_faults() {
    let next = AtomicU64::new(u64::MAX - 1);
    assert!(reserve_from(&next, 2).is_err());
    assert_eq!(next.load(Ordering::Relaxed), u64::MAX - 1);
    assert_eq!(reserve_from(&next, 1).unwrap(), vec![u64::MAX]);
    assert!(reserve_from(&next, 1).is_err());
    assert_eq!(next.load(Ordering::Relaxed), u64::MAX);
    assert!(reserve_from(&next, 0).unwrap().is_empty());
    unsafe extern "C" fn fail(c: *mut AdapterContext, _: *const u64, _: usize) {
        unsafe { tellur_adapter_v1_fail(c, b"E_FAULT: foreign".as_ptr(), 16) }
    }
    fn foreign(_: &mut Resources, _: &str, mut args: Vec<V>) -> R<Outcome> {
        binding("String", fail).invoke(&mut args)
    }
    fn original(_: &mut Resources, _: &str, _: Vec<V>) -> R<Outcome> {
        Err(adapter_error("E_FAULT: foreign"))
    }
    let mut results = vec![];
    for dispatch in [foreign as RustAdapter, original] {
        let mut host = Host::new(manifest("String"), None, true, Some(2));
        host.rust_adapter = Some(dispatch);
        host.enter("main").unwrap();
        let error = host
            .primitive("inspect", vec![V::String("input".into())])
            .unwrap_err();
        assert!(!host.source_faulted());
        results.push((error, host.events.clone(), host.cursor, host.fuel));
    }
    assert_eq!(results[0], results[1]);
}
