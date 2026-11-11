use super::*;
use std::mem::{align_of, offset_of, size_of};
use std::sync::OnceLock;

#[repr(C)]
#[derive(Default)]
struct Leaf {
    pc: u32,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    child: Leaf,
}
unsafe extern "C" fn leaf(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_tick(c, 11, 11, 1);
            f.pc = 1;
            return 1;
        }
        assert_eq!((f.pc, count), (1, 0));
        tellur_rt_v1_tick(c, 12, 12, 1);
        let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe { parent_step(c, p, args, count, true) }
}
unsafe extern "C" fn run_parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe { parent_step(c, p, args, count, false) }
}
unsafe fn parent_step(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
    handoff: bool,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Parent>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_tick(c, 1, 1, 1);
            let loan = tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
            if handoff {
                tellur_rt_v1_await(c, 0, [loan].as_ptr(), 1);
            } else {
                tellur_rt_v1_run_child(c, 0, [loan].as_ptr(), 1);
            }
            f.pc = 1;
            return if handoff { 3 } else { 5 };
        }
        assert_eq!((f.pc, count), (1, 1));
        let mut loan = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut loan, 1);
        tellur_rt_v1_release(c, loan);
        tellur_rt_v1_tick(c, 2, 2, 1);
        let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        f.pc = 2;
        2
    }
}
const LEAF: CompiledControl = CompiledControl {
    producer: b"internal_leaf".as_ptr(),
    producer_len: 13,
    name: b"leaf".as_ptr(),
    name_len: 4,
    slots: 1,
    parameters: [0].as_ptr(),
    parameter_count: 1,
    frame_size: size_of::<Leaf>(),
    frame_align: align_of::<Leaf>(),
    frame_prefix: size_of::<Leaf>(),
    callback: Some(leaf),
    children: std::ptr::null(),
    child_count: 0,
};
const PARENT: CompiledControl = CompiledControl {
    producer: b"internal_parent".as_ptr(),
    producer_len: 15,
    name: b"parent".as_ptr(),
    name_len: 6,
    slots: 1,
    parameters: [0].as_ptr(),
    parameter_count: 1,
    frame_size: size_of::<Parent>(),
    frame_align: align_of::<Parent>(),
    frame_prefix: offset_of!(Parent, child),
    callback: Some(parent),
    children: [CompiledChild {
        control: 1,
        offset: offset_of!(Parent, child),
    }]
    .as_ptr(),
    child_count: 1,
};
static TABLE: [CompiledControl; 2] = [PARENT, LEAF];

fn run(registry: &'static CompiledRegistry, text: String) -> (usize, usize) {
    let mut host = Host::new(json!({}), None, true, Some(10));
    let mut frame = Parent::default();
    let owner = text.as_ptr();
    let layout = registry.layout(0).unwrap();
    let result =
        unsafe { execution::run_abi_entry(&mut host, vec![V::String(text)], layout, &mut frame) }
            .unwrap()
            .outcome()
            .unwrap();
    let V::String(text) = &result.1[0] else {
        panic!()
    };
    assert_eq!(text.as_ptr(), owner);
    assert_eq!((frame.pc, frame.child.pc), (2, 2));
    assert_eq!(host.fuel, Some(6));
    assert!(
        host.tasks.is_empty()
            && host.resources.scopes.is_empty()
            && host.resources.values.is_empty()
    );
    (
        layout as *const _ as usize,
        layout.control as *const _ as usize,
    )
}
#[test]
fn compiled_module_descriptors_reuse_stable_authority_across_concurrent_and_repeated_launches() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    let threads = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let registry = unsafe {
                    CompiledRegistry::register(&REGISTRY, 1, TABLE.as_ptr(), TABLE.len())
                }
                .unwrap();
                run(registry, format!("owner {i}").repeat(40))
            })
        })
        .collect::<Vec<_>>();
    let identities = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert!(identities.iter().all(|pair| *pair == identities[0]));
    let registry =
        unsafe { CompiledRegistry::register(&REGISTRY, 1, TABLE.as_ptr(), TABLE.len()) }.unwrap();
    assert_eq!(run(registry, "again".repeat(40)), identities[0]);
    assert_eq!(registry.compiled[1].cold_type, "$task<internal_leaf>");
    assert_eq!(registry.compiled[1].started_type, "$started<internal_leaf>");
    assert_eq!(registry.compiled[1].set_type, "$tasks<internal_leaf>");
    assert_eq!(registry.compiled[0].set_type, "$tasks<internal_parent>");
    assert_eq!(registry.layout(1).unwrap().control.name, "leaf");
    assert!(std::ptr::eq(
        registry.layout(0).unwrap().children[0].layout,
        registry.layout(1).unwrap()
    ));
    assert!(registry.layout(2).is_err());
    static OTHER: [CompiledControl; 2] = [PARENT, LEAF];
    assert!(
        unsafe { CompiledRegistry::register(&REGISTRY, 1, OTHER.as_ptr(), OTHER.len()) }.is_err()
    );
    assert!(
        unsafe { CompiledRegistry::register(&REGISTRY, 2, TABLE.as_ptr(), TABLE.len()) }.is_err()
    );
    assert_eq!(run(registry, "after rejection".repeat(40)), identities[0]);
}
#[test]
fn compiled_module_preflight_rejects_bad_layouts_before_publishing_control_identity() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static BAD: [[CompiledControl; 2]; 17] = [
        [
            CompiledControl {
                producer: std::ptr::null(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                producer_len: 0,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                producer: [0xff].as_ptr(),
                producer_len: 1,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                callback: None,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                name: b"\xff".as_ptr(),
                name_len: 1,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                name: std::ptr::null(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                frame_size: 0,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                frame_align: 3,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                frame_prefix: size_of::<Parent>() + 1,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                parameters: [1].as_ptr(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                slots: 2,
                parameters: [0, 0].as_ptr(),
                parameter_count: 2,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                children: [CompiledChild {
                    control: 2,
                    offset: 4,
                }]
                .as_ptr(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                children: [CompiledChild {
                    control: 1,
                    offset: 0,
                }]
                .as_ptr(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                children: [CompiledChild {
                    control: 1,
                    offset: 5,
                }]
                .as_ptr(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                children: [CompiledChild {
                    control: 1,
                    offset: usize::MAX,
                }]
                .as_ptr(),
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                children: [
                    CompiledChild {
                        control: 1,
                        offset: 4,
                    },
                    CompiledChild {
                        control: 1,
                        offset: 4,
                    },
                ]
                .as_ptr(),
                child_count: 2,
                ..PARENT
            },
            LEAF,
        ],
        [
            CompiledControl {
                frame_prefix: 0,
                children: [CompiledChild {
                    control: 0,
                    offset: 0,
                }]
                .as_ptr(),
                ..PARENT
            },
            LEAF,
        ],
    ];
    assert!(unsafe { CompiledRegistry::register(&REGISTRY, 1, std::ptr::null(), 1) }.is_err());
    assert!(unsafe { CompiledRegistry::register(&REGISTRY, 1, TABLE.as_ptr(), 0) }.is_err());
    assert!(unsafe { CompiledRegistry::register(&REGISTRY, 1, TABLE.as_ptr(), 65537) }.is_err());
    for (index, table) in BAD.iter().enumerate() {
        assert!(
            unsafe { CompiledRegistry::register(&REGISTRY, 1, table.as_ptr(), table.len()) }
                .is_err(),
            "{index}"
        );
        assert!(REGISTRY.get().is_none(), "{index}");
    }
    let registry =
        unsafe { CompiledRegistry::register(&REGISTRY, 1, TABLE.as_ptr(), TABLE.len()) }.unwrap();
    run(registry, "valid after all failures".repeat(40));
}

#[test]
fn compiled_run_child_keeps_the_same_context_until_its_source_yield() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static TABLE_RUN: [CompiledControl; 2] = [
        CompiledControl {
            callback: Some(run_parent),
            ..PARENT
        },
        LEAF,
    ];
    let registry =
        unsafe { CompiledRegistry::register(&REGISTRY, 1, TABLE_RUN.as_ptr(), 2) }.unwrap();
    run(registry, "inline run owner".repeat(40));
    let mut host = Host::new(json!({}), None, true, Some(10));
    let mut frame = Parent::default();
    let mut contexts = execution::Contexts::new(&mut host);
    let key = unsafe {
        contexts.start_abi_entry(
            registry.layout(0).unwrap(),
            vec![V::String("value".into())],
            &mut frame,
        )
    }
    .unwrap();
    assert!(matches!(
        contexts.advance(&key, 1).unwrap(),
        execution::ContextPoll::Paused
    ));
    // Parent's call pushes an activation; it does not queue a separate awaited
    // context. The next selected instruction is the child's explicit source yield.
    assert!(matches!(
        contexts.advance(&key, 1).unwrap(),
        execution::ContextPoll::Await
    ));
    // A host-owned Contexts session is explicitly driven to completion; dropping
    // a suspended session is not its normal completion/retirement operation.
    // The raw host API leaves the source-yield marker for the source driver, so
    // the child's return may report Await once before the parent's completion.
    let mut result = None;
    for _ in 0..2 {
        match contexts.advance(&key, usize::MAX).unwrap() {
            execution::ContextPoll::Complete(value) => {
                result = Some(value);
                break;
            }
            execution::ContextPoll::Await => {}
            _ => panic!("same-context child stopped without progress"),
        }
    }
    let result = result.expect("same-context child did not complete after its yield");
    assert_eq!(
        result.outcome().unwrap(),
        ("done".into(), vec![V::String("value".into())])
    );
    drop(contexts);
    assert_eq!((frame.pc, frame.child.pc), (2, 2));
    assert!(host.tasks.is_empty() && host.resources.scopes.is_empty());
}
fn launch_context(host: &mut Host) -> Context<'_> {
    Context {
        selected_frame: None,
        selected_activation: None,
        program: None,
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
fn compiled_entry_borrows_physical_storage_and_rejects_bad_extent_before_captures() {
    for attempt in 0..3 {
        let mut host = Host::new(json!({}), None, true, Some(10));
        let mut frame = Parent::default();
        let mut context = launch_context(&mut host);
        let text = format!("launch {attempt}").repeat(40);
        let pointer = text.as_ptr();
        let value = context.put(V::String(text));
        unsafe {
            tellur_rt_v1_run_entry(
                &mut context,
                TABLE.as_ptr(),
                2,
                0,
                (&mut frame as *mut Parent).cast(),
                size_of::<Parent>(),
                align_of::<Parent>(),
                [value].as_ptr(),
                1,
                [1, 3, usize::MAX][attempt],
            )
        };
        let result = context.finish().unwrap().outcome().unwrap();
        let V::String(text) = &result.1[0] else {
            panic!()
        };
        assert_eq!(text.as_ptr(), pointer);
        drop(context);
        assert_eq!(host.fuel, Some(6));
        assert!(host.tasks.is_empty() && host.resources.scopes.is_empty());
    }
    let mut host = Host::new(json!({}), None, true, Some(10));
    let mut frame = Parent::default();
    let mut context = launch_context(&mut host);
    let value = context.put(V::String("untouched".into()));
    unsafe {
        tellur_rt_v1_run_entry(
            &mut context,
            TABLE.as_ptr(),
            2,
            0,
            (&mut frame as *mut Parent).cast(),
            size_of::<Parent>() - 1,
            align_of::<Parent>(),
            [value].as_ptr(),
            1,
            usize::MAX,
        )
    };
    assert!(context.error.as_deref().unwrap().contains("physical frame"));
    assert!(matches!(context.get(value).unwrap(), V::String(s) if s == "untouched"));
    assert_eq!((frame.pc, frame.child.pc), (0, 0));
    assert_eq!(context.host.fuel, Some(10));
    assert!(context.host.events.is_empty() && context.host.resources.scopes.is_empty());
}

#[test]
fn host_wrapper_metadata_uses_exact_module_static_producer_and_cancellation_variant() {
    static REGISTRY: OnceLock<CompiledRegistry> = OnceLock::new();
    static HOSTS: [CompiledControl; 2] = [
        CompiledControl {
            producer: b"tick$host".as_ptr(),
            producer_len: 9,
            name: b"display".as_ptr(),
            name_len: 7,
            ..LEAF
        },
        CompiledControl {
            producer: b"tick$host$cancel".as_ptr(),
            producer_len: 16,
            ..LEAF
        },
    ];
    static EMPTY: [CompiledControl; 1] = [CompiledControl {
        producer: b"$host".as_ptr(),
        producer_len: 5,
        ..LEAF
    }];
    assert!(unsafe { CompiledRegistry::register(&REGISTRY, 1, EMPTY.as_ptr(), 1) }.is_err());
    assert!(REGISTRY.get().is_none());
    let registry = unsafe { CompiledRegistry::register(&REGISTRY, 1, HOSTS.as_ptr(), 2) }.unwrap();
    for (index, cancellable) in [false, true].into_iter().enumerate() {
        let host = registry.compiled[index].host.unwrap();
        assert_eq!(host.operation, "tick");
        assert_eq!(host.cancellable, cancellable);
        assert_eq!(host.operation.as_ptr(), HOSTS[index].producer);
    }
    assert_eq!(registry.layout(0).unwrap().control.name, "display");
    for _ in 0..20 {
        let again = unsafe { CompiledRegistry::register(&REGISTRY, 1, HOSTS.as_ptr(), 2) }.unwrap();
        assert!(std::ptr::eq(registry, again));
        assert_eq!(
            again.compiled[0].host.unwrap().operation.as_ptr(),
            HOSTS[0].producer
        );
    }
}
