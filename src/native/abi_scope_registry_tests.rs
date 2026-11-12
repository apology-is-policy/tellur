use super::*;
use std::sync::OnceLock;

const fn slots(data: &'static [usize]) -> CompiledSlots {
    CompiledSlots {
        data: data.as_ptr(),
        count: data.len(),
    }
}
unsafe extern "C" fn reset(p: *mut u8) {
    unsafe {
        *p = 0;
    }
}
unsafe extern "C" fn finish(
    c: *mut Context<'_>,
    _: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, args, count);
    }
    2
}
const CONTROL: CompiledControl = CompiledControl {
    producer: b"registry".as_ptr(),
    producer_len: 8,
    name: b"registry".as_ptr(),
    name_len: 8,
    slots: 4,
    parameters: [0].as_ptr(),
    parameter_count: 1,
    frame_size: 1,
    frame_align: 1,
    frame_prefix: 1,
    callback: Some(finish),
    children: std::ptr::null(),
    child_count: 0,
    retained: std::ptr::null(),
    retained_count: 0,
};
const BOUNDARY: CompiledBoundary = CompiledBoundary {
    name: b"lexical".as_ptr(),
    name_len: 7,
    body: 1,
    handler: 2,
    after: 3,
    retained: slots(&[0]),
    retire: slots(&[1]),
    work: slots(&[1]),
    handling: slots(&[2]),
    fault_slot: 2,
};
const LOOP: CompiledLoop = CompiledLoop {
    target: 9,
    body: 1,
    after: 3,
    locals: slots(&[0, 1, 2]),
    parameters: slots(&[0]),
    reset_facts: Some(reset),
};
const SCOPES: CompiledScopes = CompiledScopes {
    states: 4,
    boundaries: [BOUNDARY].as_ptr(),
    boundary_count: 1,
    loops: [LOOP].as_ptr(),
    loop_count: 1,
};
const ENTRY: CompiledControlV3 = CompiledControlV3 {
    control: CONTROL,
    scopes: &SCOPES,
};
static GOOD: [CompiledControlV3; 1] = [ENTRY];

fn check_bad(table: &'static [CompiledControlV3], registry: &'static OnceLock<CompiledRegistry>) {
    assert!(
        unsafe { CompiledRegistry::register_v3(registry, 3, table.as_ptr(), table.len()) }.is_err()
    );
    assert!(
        registry.get().is_none(),
        "invalid metadata published an identity"
    );
}
#[test]
fn compiled_v3_scope_spans_reject_before_registry_publication() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static BAD: [[CompiledControlV3; 1]; 9] = [
        [CompiledControlV3 {
            scopes: 1usize as *const CompiledScopes,
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                states: 0,
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                boundary_count: 65537,
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                boundaries: std::ptr::null(),
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                boundaries: 1usize as *const CompiledBoundary,
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                loop_count: 65537,
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                loops: std::ptr::null(),
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            scopes: &CompiledScopes {
                loops: 1usize as *const CompiledLoop,
                ..SCOPES
            },
            ..ENTRY
        }],
        [CompiledControlV3 {
            control: CompiledControl {
                producer: b"tick$host".as_ptr(),
                producer_len: 9,
                ..CONTROL
            },
            ..ENTRY
        }],
    ];
    for table in &BAD {
        check_bad(table, &REGISTRY);
    }
    let good = unsafe { CompiledRegistry::register_v3(&REGISTRY, 3, GOOD.as_ptr(), 1) }.unwrap();
    assert_eq!(good.layout(0).unwrap().scopes.unwrap().states, 4);
}
#[test]
fn compiled_v3_lexical_states_names_and_roles_reject_atomically() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static BAD: [CompiledBoundary; 15] = [
        CompiledBoundary {
            body: 0,
            ..BOUNDARY
        },
        CompiledBoundary {
            handler: 4,
            ..BOUNDARY
        },
        CompiledBoundary {
            after: 1,
            ..BOUNDARY
        },
        CompiledBoundary {
            handler: 1,
            ..BOUNDARY
        },
        CompiledBoundary {
            after: 2,
            ..BOUNDARY
        },
        CompiledBoundary {
            name_len: 0,
            ..BOUNDARY
        },
        CompiledBoundary {
            name: std::ptr::null(),
            ..BOUNDARY
        },
        CompiledBoundary {
            name: [255].as_ptr(),
            name_len: 1,
            ..BOUNDARY
        },
        CompiledBoundary {
            fault_slot: 4,
            ..BOUNDARY
        },
        CompiledBoundary {
            fault_slot: 1,
            ..BOUNDARY
        },
        CompiledBoundary {
            retained: slots(&[1]),
            ..BOUNDARY
        },
        CompiledBoundary {
            retained: slots(&[2]),
            ..BOUNDARY
        },
        CompiledBoundary {
            work: slots(&[0]),
            ..BOUNDARY
        },
        CompiledBoundary {
            retire: slots(&[1, 1]),
            ..BOUNDARY
        },
        CompiledBoundary {
            handling: slots(&[2, 4]),
            ..BOUNDARY
        },
    ];
    // Local tables cannot be registered under the module-static contract. Read
    // preflight directly here; the publication tests above use fully static data.
    for boundary in &BAD {
        let raw = CompiledScopes {
            boundaries: boundary,
            ..SCOPES
        };
        assert!(unsafe { CompiledScopeData::read(&raw, CONTROL.slots) }.is_err());
        assert!(REGISTRY.get().is_none());
    }
    static BAD_TABLE: [CompiledControlV3; 1] = [CompiledControlV3 {
        scopes: &CompiledScopes {
            boundaries: [CompiledBoundary {
                work: slots(&[0]),
                ..BOUNDARY
            }]
            .as_ptr(),
            ..SCOPES
        },
        ..ENTRY
    }];
    check_bad(&BAD_TABLE, &REGISTRY);
}
#[test]
fn compiled_v3_loop_states_targets_slots_and_reset_reject_atomically() {
    static BAD: [CompiledLoop; 12] = [
        CompiledLoop { body: 0, ..LOOP },
        CompiledLoop { body: 4, ..LOOP },
        CompiledLoop { after: 0, ..LOOP },
        CompiledLoop { after: 4, ..LOOP },
        CompiledLoop { after: 1, ..LOOP },
        CompiledLoop {
            locals: slots(&[0, 0]),
            ..LOOP
        },
        CompiledLoop {
            locals: slots(&[0, 4]),
            ..LOOP
        },
        CompiledLoop {
            parameters: slots(&[1, 1]),
            ..LOOP
        },
        CompiledLoop {
            parameters: slots(&[3]),
            ..LOOP
        },
        CompiledLoop {
            parameters: CompiledSlots {
                data: std::ptr::null(),
                count: 1,
            },
            ..LOOP
        },
        CompiledLoop {
            locals: CompiledSlots {
                data: 1usize as *const usize,
                count: 1,
            },
            ..LOOP
        },
        CompiledLoop {
            reset_facts: None,
            ..LOOP
        },
    ];
    for entry in &BAD {
        let raw = CompiledScopes {
            loops: entry,
            ..SCOPES
        };
        assert!(unsafe { CompiledScopeData::read(&raw, CONTROL.slots) }.is_err());
    }
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static DUP: [CompiledControlV3; 1] = [CompiledControlV3 {
        scopes: &CompiledScopes {
            loops: [LOOP, LOOP].as_ptr(),
            loop_count: 2,
            ..SCOPES
        },
        ..ENTRY
    }];
    check_bad(&DUP, &REGISTRY);
}
#[test]
fn compiled_v3_and_v2_have_separate_stride_and_module_identity() {
    static OLD: [CompiledControl; 1] = [CONTROL];
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    assert!(
        unsafe { CompiledRegistry::register_v3(&REGISTRY, 2, OLD.as_ptr().cast(), 1) }.is_err()
    );
    assert!(unsafe { CompiledRegistry::register_v3(&REGISTRY, 3, std::ptr::null(), 1) }.is_err());
    assert!(unsafe { CompiledRegistry::register_v3(&REGISTRY, 3, GOOD.as_ptr(), 0) }.is_err());
    assert!(unsafe { CompiledRegistry::register_v3(&REGISTRY, 3, GOOD.as_ptr(), 65537) }.is_err());
    assert!(REGISTRY.get().is_none());
    let old = unsafe { CompiledRegistry::register(&REGISTRY, 2, OLD.as_ptr(), 1) }.unwrap();
    assert!(old.layout(0).unwrap().scopes.is_none());
    assert!(unsafe { CompiledRegistry::register_v3(&REGISTRY, 3, GOOD.as_ptr(), 1) }.is_err());
    assert!(std::ptr::eq(old, REGISTRY.get().unwrap()));
    static NEW: OnceLock<CompiledRegistry> = OnceLock::new();
    let new = unsafe { CompiledRegistry::register_v3(&NEW, 3, GOOD.as_ptr(), 1) }.unwrap();
    assert!(new.layout(0).unwrap().scopes.is_some());
    assert!(unsafe { CompiledRegistry::register(&NEW, 2, OLD.as_ptr(), 1) }.is_err());
}
fn registered_run(registry: &'static CompiledRegistry) -> [usize; 4] {
    let layout = registry.layout(0).unwrap();
    layout.validate().unwrap();
    let scopes = layout.scopes.unwrap();
    let value = "stable owned buffer".repeat(100);
    let pointer = value.as_ptr();
    let mut frame = 99u8;
    let mut host = Host::new(json!({}), None, true, Some(10));
    let result =
        unsafe { execution::run_abi_entry(&mut host, vec![V::String(value)], layout, &mut frame) }
            .unwrap()
            .outcome()
            .unwrap();
    let V::String(value) = &result.1[0] else {
        panic!()
    };
    assert_eq!(value.as_ptr(), pointer);
    assert_eq!(frame, 99); // unused registered loops do not reset a frame
    assert!(
        host.tasks.is_empty()
            && host.resources.values.is_empty()
            && host.resources.scopes.is_empty()
    );
    [
        layout as *const _ as usize,
        scopes as *const _ as usize,
        scopes.boundaries[0] as *const _ as usize,
        scopes.loops[0].control as *const _ as usize,
    ]
}
#[test]
fn compiled_v3_concurrent_registration_keeps_owned_scope_identities_stable() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    let handles = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let registry =
                    unsafe { CompiledRegistry::register_v3(&REGISTRY, 3, GOOD.as_ptr(), 1) }
                        .unwrap();
                registered_run(registry)
            })
        })
        .collect::<Vec<_>>();
    let values = handles
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert!(values.iter().all(|value| *value == values[0]));
    let registry = REGISTRY.get().unwrap();
    for _ in 0..20 {
        assert_eq!(registered_run(registry), values[0]);
    }
    let data = registry.compiled[0].scopes.as_ref().unwrap();
    assert!(std::ptr::eq(data.layout().loops[0].control, &data.loops[0]));
    assert!(std::ptr::eq(
        data.layout().boundaries[0],
        &data.boundaries[0]
    ));
}

#[test]
fn compiled_v3_entry_rejects_bad_metadata_storage_and_arity_before_capture_transfer() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static BAD: [CompiledControlV3; 1] = [CompiledControlV3 {
        scopes: &CompiledScopes {
            states: 0,
            ..SCOPES
        },
        ..ENTRY
    }];
    for attempt in 0..6 {
        let mut host = Host::new(json!({}), None, true, Some(10));
        let mut context = Context {
            selected_frame: None,
            selected_activation: None,
            program: None,
            resume_at: None,
            request: None,
            physical_arena: None,
            host: &mut host,
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
        };
        let text = format!("capture {attempt}").repeat(40);
        let pointer = text.as_ptr();
        let handle = context.put(V::String(text));
        let mut frame = 99u8;
        if attempt == 5 {
            unsafe {
                tellur_rt_v3_run_entry(
                    &mut context,
                    BAD.as_ptr(),
                    1,
                    0,
                    &mut frame,
                    1,
                    1,
                    &handle,
                    1,
                    1,
                );
            }
        } else {
            context.perform(|c| unsafe {
                run_compiled_entry(
                    c,
                    || {
                        CompiledRegistry::register_v3(
                            &REGISTRY,
                            3,
                            if attempt == 0 {
                                BAD.as_ptr()
                            } else {
                                GOOD.as_ptr()
                            },
                            1,
                        )
                    },
                    if attempt == 4 { 1 } else { 0 },
                    &mut frame,
                    if attempt == 2 { 0 } else { 1 },
                    1,
                    &handle,
                    if attempt == 3 { 0 } else { 1 },
                    if attempt == 1 { 0 } else { 1 },
                )
            });
        }
        assert!(context.error.is_some());
        let V::String(text) = context.get(handle).unwrap() else {
            panic!()
        };
        assert_eq!(text.as_ptr(), pointer);
        assert_eq!(frame, 99);
        assert!(context.result.is_none() && context.request.is_none());
        if attempt < 2 {
            assert!(REGISTRY.get().is_none());
        }
        unsafe {
            tellur_rt_v1_emit(&mut context, handle);
        }
        assert!(context.host.events.is_empty());
        assert!(context.host.tasks.is_empty() && context.host.resources.scopes.is_empty());
    }
}
