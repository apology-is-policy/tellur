use super::*;
use std::ptr::null;

fn manifest() -> Json {
    json!({"version":"0.1","types":[],"rust":{"source":"host.rs","lockfile":"Cargo.lock"},
        "primitives":[{"name":"inspect","parameters":[{"name":"input","type":"String","mode":"borrow"}],
        "outcomes":[{"name":"ready","fields":[{"name":"output","type":"String"}]}],
        "effects":[],"returns":true,"rust":{"execution":"immediate"}}]})
}
unsafe extern "C" fn echo(c: *mut AdapterContext, args: *const u64, count: usize) {
    unsafe { tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, args, count) }
}
unsafe extern "C" fn different(c: *mut AdapterContext, _: *const u64, _: usize) {
    unsafe {
        let value = tellur_adapter_v1_value(c, 3, 0, b"other".as_ptr(), 5);
        tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, &value, 1);
    }
}
fn operation(name: &[u8], call: AdapterCallback) -> AdapterOperation {
    AdapterOperation {
        name: name.as_ptr(),
        name_len: name.len(),
        callback: AdapterDescriptor {
            version: 1,
            flags: 0,
            call: Some(call),
        },
    }
}
fn table(operations: &[AdapterOperation]) -> AdapterTable {
    AdapterTable {
        version: 1,
        flags: 0,
        operations: operations.as_ptr(),
        count: operations.len(),
    }
}
fn registry(call: AdapterCallback) -> Arc<AdapterRegistry> {
    unsafe {
        AdapterRegistry::prepare(&manifest(), &table(&[operation(b"inspect", call)])).unwrap()
    }
}
fn host(registry: &Arc<AdapterRegistry>) -> Host {
    let mut host = Host::new(manifest(), None, true, Some(4));
    registry.install(&mut host).unwrap();
    host
}
fn inspect(host: &mut Host) -> String {
    let (_, fields) = host
        .primitive("inspect", vec![V::String("input".into())])
        .unwrap();
    match &fields[0] {
        V::String(s) => s.clone(),
        _ => panic!(),
    }
}
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
#[test]
fn complete_table_preflight_rejects_malformed_or_missing_bindings() {
    let m = manifest();
    let op = operation(b"inspect", echo);
    let ops = [op];
    for bad in [
        AdapterTable {
            version: 0,
            ..table(&ops)
        },
        AdapterTable {
            version: 2,
            ..table(&ops)
        },
        AdapterTable {
            flags: 1,
            ..table(&ops)
        },
        AdapterTable {
            operations: null(),
            ..table(&ops)
        },
        AdapterTable {
            operations: 1usize as *const _,
            ..table(&ops)
        },
        AdapterTable {
            count: usize::MAX,
            ..table(&ops)
        },
        table(&[]),
    ] {
        assert!(unsafe { AdapterRegistry::prepare(&m, &bad) }.is_err());
    }
    assert!(unsafe { AdapterRegistry::prepare(&m, null()) }.is_err());
    assert!(unsafe { AdapterRegistry::prepare(&m, 1usize as *const _) }.is_err());
    for ops in [
        vec![operation(b"", echo)],
        vec![operation(&[255], echo)],
        vec![operation(b"unknown", echo)],
        vec![op, op],
        vec![AdapterOperation { name: null(), ..op }],
        vec![AdapterOperation {
            name_len: usize::MAX,
            ..op
        }],
        vec![AdapterOperation {
            callback: AdapterDescriptor {
                version: 2,
                ..op.callback
            },
            ..op
        }],
        vec![AdapterOperation {
            callback: AdapterDescriptor {
                flags: 1,
                ..op.callback
            },
            ..op
        }],
        vec![AdapterOperation {
            callback: AdapterDescriptor {
                call: None,
                ..op.callback
            },
            ..op
        }],
    ] {
        assert!(unsafe { AdapterRegistry::prepare(&m, &table(&ops)) }.is_err());
    }
    for mode in 0..5 {
        let mut m = m.clone();
        match mode {
            0 => m["primitives"][0]["rust"]["execution"] = json!("poll"),
            1 => m["primitives"][0]["parameters"][0]["type"] = json!("file"),
            2 => {
                let p = m["primitives"][0].clone();
                m["primitives"].as_array_mut().unwrap().push(p);
            }
            3 => {
                let mut p = m["primitives"][0].clone();
                p["name"] = json!("missing");
                m["primitives"].as_array_mut().unwrap().push(p);
            }
            _ => m["primitives"][0]["rust"] = Json::Null,
        }
        assert!(
            unsafe { AdapterRegistry::prepare(&m, &table(&ops)) }.is_err(),
            "{mode}"
        );
    }
    // A rejected candidate does not poison any later registration.
    assert_eq!(inspect(&mut host(&registry(echo))), "input");
}
#[test]
fn copied_metadata_and_atomic_installation_preserve_existing_host_owners() {
    let binding = {
        let name = String::from("inspect");
        let m = manifest();
        unsafe {
            AdapterRegistry::prepare(&m, &table(&[operation(name.as_bytes(), echo)])).unwrap()
        }
    };
    let mut h = Host::new(manifest(), None, true, Some(4));
    let owner = h.resources.insert("private", String::from("kept")).unwrap();
    binding.install(&mut h).unwrap();
    let second = registry(different);
    assert!(second.install(&mut h).is_err());
    assert_eq!(inspect(&mut h), "input");
    assert_eq!(
        h.resources.get::<String>(&owner, "private").unwrap(),
        "kept"
    );
    assert_eq!(
        h.resources.take::<String>(&owner, "private").unwrap(),
        "kept"
    );
    let mut incompatible = Host::new(json!({}), None, true, None);
    assert!(binding.install(&mut incompatible).is_err());
    assert!(incompatible.resources.foreign_adapters.is_none());
    assert!(incompatible.rust_adapter.is_none());
    fn existing(_: &mut Resources, _: &str, _: Vec<V>) -> R<Outcome> {
        panic!("not invoked")
    }
    let mut occupied = Host::new(manifest(), None, true, None);
    occupied.rust_adapter = Some(existing);
    assert!(binding.install(&mut occupied).is_err());
    assert!(occupied.resources.foreign_adapters.is_none());
    assert!(std::ptr::fn_addr_eq(
        occupied.rust_adapter.unwrap(),
        existing as RustAdapter
    ));
    let weak = Arc::downgrade(&binding);
    drop(binding);
    assert!(weak.upgrade().is_some());
    drop(h);
    assert!(weak.upgrade().is_none());
}
#[test]
fn concurrent_and_reentrant_hosts_never_select_another_launch_table() {
    unsafe extern "C" fn reentrant(c: *mut AdapterContext, a: *const u64, n: usize) {
        assert_eq!(inspect(&mut host(&registry(different))), "other");
        unsafe { echo(c, a, n) }
    }
    let a = registry(reentrant);
    let b = registry(different);
    let threads = (0..8)
        .map(|i| {
            let r = if i % 2 == 0 {
                Arc::clone(&a)
            } else {
                Arc::clone(&b)
            };
            std::thread::spawn(move || {
                for _ in 0..20 {
                    let mut h = host(&r);
                    assert_eq!(inspect(&mut h), if i % 2 == 0 { "input" } else { "other" });
                }
            })
        })
        .collect::<Vec<_>>();
    for t in threads {
        t.join().unwrap();
    }
    let mut first = host(&a);
    let mut second = host(&b);
    for _ in 0..4 {
        assert_eq!(inspect(&mut second), "other");
        assert_eq!(inspect(&mut first), "input");
    }
}
#[test]
fn compiled_dispatch_preserves_original_scripts_events_fuel_and_failures() {
    fn direct(_: &mut Resources, _: &str, args: Vec<V>) -> R<Outcome> {
        Ok(("ready".into(), args))
    }
    unsafe extern "C" fn never(_: *mut AdapterContext, _: *const u64, _: usize) {
        panic!("script bypass")
    }
    for scripted in [false, true] {
        for fuel in [0, 1, 4] {
            let mut results = vec![];
            for compiled in [false, true] {
                let calls = scripted.then(|| {
                    vec![ScriptCall {
                        operation: "inspect".into(),
                        outcome: "ready".into(),
                        fields: vec![V::String("script".into())],
                        waits: 0,
                    }]
                });
                let mut h = Host::new(manifest(), calls, true, Some(fuel));
                if compiled {
                    registry(if scripted { never } else { echo })
                        .install(&mut h)
                        .unwrap();
                } else {
                    h.rust_adapter = Some(direct);
                }
                h.enter("main").unwrap();
                let result = h.tick(1, 1).and_then(|_| {
                    if compiled {
                        let mut c = context(&mut h);
                        let value = c.put(V::String("input".into()));
                        let result = unsafe {
                            tellur_rt_v1_primitive(&mut c, b"inspect".as_ptr(), 7, &value, 1)
                        };
                        if let Some(e) = c.error {
                            return Err(e);
                        }
                        Ok(c.outcomes[(result - c.outcome_base - 1) as usize]
                            .take()
                            .unwrap())
                    } else {
                        h.primitive("inspect", vec![V::String("input".into())])
                    }
                });
                h.leave();
                results.push((
                    serde_json::to_value(result).unwrap(),
                    h.events.clone(),
                    h.cursor,
                    h.fuel,
                ));
            }
            assert_eq!(results[0], results[1], "{scripted}/{fuel}");
        }
    }
}
#[test]
fn unsupported_or_malformed_compiled_calls_preserve_input_handles() {
    for installed in [false, true] {
        for mode in 0..3 {
            let mut h = Host::new(manifest(), None, true, None);
            if installed {
                registry(echo).install(&mut h).unwrap();
            }
            let mut c = context(&mut h);
            let value = c.put(if mode == 0 {
                V::Bool(true)
            } else {
                V::String("kept".into())
            });
            let args = if mode == 2 {
                vec![value, value]
            } else {
                vec![value]
            };
            let count = if mode == 1 { 0 } else { args.len() };
            let out = unsafe {
                tellur_rt_v1_primitive(&mut c, b"inspect".as_ptr(), 7, args.as_ptr(), count)
            };
            assert_eq!(out, 0);
            assert!(c.error.is_some());
            assert!(c.get(value).is_ok());
            assert!(c.host.events.is_empty());
        }
    }
}
#[test]
fn invalid_launch_registration_fails_before_entry_or_argument_processing() {
    unsafe extern "C" fn never(_: *mut Context<'_>, _: *const u64, _: usize) {
        panic!("entry reached");
    }
    let info=json!({"abi":1,"manifest":manifest().to_string(),"entry":"main","sites":"{}", "entries":[{"name":"main","display":"main","arguments":[],"exits":[["done",0]]}]}).to_string();
    let callbacks = [never as Callback];
    let invalid = AdapterTable {
        version: 2,
        ..table(&[])
    };
    let error = unsafe {
        launch_program_with_adapters(
            info.as_ptr(),
            info.len(),
            callbacks.as_ptr(),
            1,
            Some(&invalid),
        )
    }
    .unwrap_err();
    assert!(error.contains("incompatible operation table"), "{error}");
    assert_eq!(
        unsafe { tellur_rt_v2_launch(info.as_ptr(), info.len(), callbacks.as_ptr(), 1, &invalid) },
        1
    );
}
#[test]
fn empty_tables_do_not_admit_unregistered_operations_or_allocate_resource_tokens() {
    let mut m = manifest();
    m["primitives"] = json!([]);
    m["rust"] = Json::Null;
    let table = AdapterTable {
        operations: null(),
        ..table(&[])
    };
    let r = unsafe { AdapterRegistry::prepare(&m, &table) }.unwrap();
    let mut h = Host::new(m, None, true, None);
    r.install(&mut h).unwrap();
    assert!(!r.contains("inspect"));
    assert!(h.resources.values.is_empty());
    assert!(dispatch(&mut h.resources, "inspect", vec![]).is_err());
}
