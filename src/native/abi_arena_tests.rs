use super::*;
use std::mem::{align_of, offset_of, size_of};

#[test]
fn physical_arena_batches_aligned_frames_and_recycles_only_after_the_last_pin() {
    let arena = physical_frames::Arena::default();
    let frames = (0..256)
        .map(|_| arena.allocate(24, 8).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(arena.counts(), (2, 256, 256));
    let addresses = frames
        .iter()
        .map(|p| p.address() as usize)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(addresses.len(), 256);
    let pins = frames
        .iter()
        .map(|p| p.retain().unwrap())
        .collect::<Vec<_>>();
    for frame in &frames {
        assert_eq!(frame.address() as usize % 8, 0);
        unsafe { std::ptr::write_bytes(frame.address(), 0xa5, 24) };
    }
    drop(frames);
    let extra = arena.allocate(24, 8).unwrap();
    assert!(!addresses.contains(&(extra.address() as usize)));
    for pin in &pins {
        assert!(
            unsafe { std::slice::from_raw_parts(pin.address(), 24) }
                .iter()
                .all(|b| *b == 0xa5)
        );
    }
    drop(extra);
    drop(pins);
    assert_eq!(arena.counts(), (2, 257, 0));
    for _ in 0..10_000 {
        let frame = arena.allocate(24, 8).unwrap();
        assert!(
            unsafe { std::slice::from_raw_parts(frame.address(), 24) }
                .iter()
                .all(|b| *b == 0)
        );
        unsafe { std::ptr::write_bytes(frame.address(), 0xff, 24) };
    }
    assert_eq!(arena.counts(), (2, 257, 0));
    let large = arena.allocate(9000, 8192).unwrap();
    assert_eq!(large.address() as usize % 8192, 0);
    let tiny = arena.allocate(17, 256).unwrap();
    assert_eq!(tiny.address() as usize % 256, 0);
    assert_ne!(large.address(), tiny.address());
    drop(large);
    drop(tiny);
    assert_eq!(arena.counts().2, 0);
}

#[test]
fn physical_arena_rejects_bad_extents_without_mutating_live_storage() {
    let arena = physical_frames::Arena::default();
    let frame = arena.allocate(16, 8).unwrap();
    unsafe { *frame.address() = 91 };
    let before = arena.counts();
    for (size, align) in [
        (0, 8),
        (1, 0),
        (1, 3),
        (usize::MAX, 8),
        (isize::MAX as usize, 4096),
    ] {
        assert!(arena.allocate(size, align).is_err());
        assert_eq!(arena.counts(), before);
        assert_eq!(unsafe { *frame.address() }, 91);
    }
    static BAD: InvocationLayout = InvocationLayout {
        align: 3,
        ..AWAIT_LAYOUT
    };
    assert!(unsafe { Invocation::in_arena(&arena, &BAD) }.is_err());
    assert_eq!(arena.counts(), before);
}

#[test]
fn physical_pins_keep_storage_alive_after_the_arena_handle_and_other_roots_retire() {
    let first = physical_frames::Arena::default();
    let second = physical_frames::Arena::default();
    let a = first.allocate(32, 16).unwrap();
    let b = second.allocate(32, 16).unwrap();
    assert_ne!(a.address(), b.address());
    unsafe {
        *a.address() = 1;
        *b.address() = 2;
    }
    let retained = a.retain().unwrap();
    drop(first);
    drop(a);
    drop(second);
    assert_eq!(unsafe { *retained.address() }, 1);
    assert_eq!(unsafe { *b.address() }, 2);
    drop(retained);
    drop(b);
}

#[repr(C)]
#[derive(Default)]
struct Child {
    pc: u32,
    fail: u32,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    child: Child,
}
thread_local! {
    static OBSERVE: std::cell::RefCell<Option<physical_frames::Arena>> = const { std::cell::RefCell::new(None) };
}
unsafe extern "C" fn child(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    OBSERVE.with(|a| {
        if let Some(arena) = &*a.borrow() {
            // Root invocation, this inline child and the test's observation pin.
            assert_eq!(arena.counts().2, 3);
        }
    });
    unsafe {
        let frame = &mut *p.cast::<Child>();
        if frame.pc == 0 {
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_tick(c, 11, 11, 1);
            frame.pc = 1;
            return 1;
        }
        assert_eq!((frame.pc, count), (1, 0));
        tellur_rt_v1_tick(c, 12, 12, 1);
        frame.pc = 2;
        if frame.fail != 0 {
            let message = tellur_rt_v1_value(c, 3, 0, b"arena child failed".as_ptr(), 18);
            tellur_rt_v1_fault(c, message);
            return 0;
        }
        let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        2
    }
}
unsafe fn parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
    run: bool,
) -> u32 {
    unsafe {
        let frame = &mut *p.cast::<Parent>();
        if frame.pc == 0 {
            assert_eq!(count, 2);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            frame.child.fail = tellur_rt_v1_bool(c, *args.add(1)) as u32;
            tellur_rt_v1_tick(c, 1, 1, 1);
            let loan = tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
            frame.pc = 1;
            if run {
                tellur_rt_v1_run_child(c, 0, [loan].as_ptr(), 1);
                return 5;
            }
            tellur_rt_v1_await(c, 0, [loan].as_ptr(), 1);
            return 3;
        }
        assert_eq!((frame.pc, count), (1, 1));
        let mut loan = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut loan, 1);
        tellur_rt_v1_release(c, loan);
        tellur_rt_v1_tick(c, 2, 2, 1);
        let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        frame.pc = 2;
        2
    }
}
unsafe extern "C" fn run_parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe { parent(c, p, args, count, true) }
}
unsafe extern "C" fn await_parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe { parent(c, p, args, count, false) }
}
static CHILD_CONTROL: execution::Control = execution::Control {
    name: "arena_child",
    slots: 1,
    parameters: &[0],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static PARENT_CONTROL: execution::Control = execution::Control {
    name: "arena_parent",
    slots: 2,
    parameters: &[0, 1],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static CHILD: InvocationLayout = InvocationLayout {
    control: &CHILD_CONTROL,
    callback: child,
    size: size_of::<Child>(),
    align: align_of::<Child>(),
    prefix: size_of::<Child>(),
    children: &[],
};
static AWAIT_LAYOUT: InvocationLayout = InvocationLayout {
    control: &PARENT_CONTROL,
    callback: await_parent,
    size: size_of::<Parent>(),
    align: align_of::<Parent>(),
    prefix: offset_of!(Parent, child),
    children: &[InlineChild {
        offset: offset_of!(Parent, child),
        layout: &CHILD,
    }],
};
static RUN_LAYOUT: InvocationLayout = InvocationLayout {
    callback: run_parent,
    ..AWAIT_LAYOUT
};

#[test]
fn arena_invocations_and_inline_descendants_preserve_faults_owners_fuel_and_quotas() {
    let arena = physical_frames::Arena::default();
    for layout in [&AWAIT_LAYOUT, &RUN_LAYOUT] {
        for fail in [false, true] {
            for fuel in 0..=6 {
                let mut expected = None;
                for quota in [1, 3, usize::MAX] {
                    for allocated in [false, true] {
                        let mut stack = Parent::default();
                        let invocation = if allocated {
                            unsafe { Invocation::in_arena(&arena, layout) }.unwrap()
                        } else {
                            unsafe { Invocation::new(layout, &mut stack) }.unwrap()
                        };
                        let pointer = invocation.program.address.cast::<Parent>();
                        let observed = invocation.storage.as_ref().map(|p| p.retain().unwrap());
                        OBSERVE.with(|a| *a.borrow_mut() = allocated.then(|| arena.clone()));
                        let owner = "original arena borrower 😀".repeat(25);
                        let buffer = owner.as_ptr();
                        let mut host = Host::new(json!({}), None, true, Some(fuel));
                        let result = unsafe {
                            execution::run_abi_invocation(
                                &mut host,
                                vec![V::String(owner), V::Bool(fail)],
                                layout,
                                invocation,
                                quota,
                            )
                        };
                        OBSERVE.with(|a| a.borrow_mut().take());
                        let report = match result {
                            Ok(value) => {
                                assert!(!fail && fuel >= 4);
                                let (_, fields) = value.outcome().unwrap();
                                let V::String(text) = &fields[0] else {
                                    panic!()
                                };
                                assert_eq!(text.as_ptr(), buffer);
                                format!("done:{text}")
                            }
                            Err(error) => {
                                assert!(
                                    fail || fuel < 4,
                                    "{error}; fail={fail}, fuel={fuel}, allocated={allocated}, quota={quota}"
                                );
                                if fail && fuel >= 3 {
                                    assert!(error.contains("arena child failed"), "{error}");
                                }
                                error
                            }
                        };
                        let frame = unsafe { &*pointer };
                        let actual = (report, host.fuel, frame.pc, frame.child.pc);
                        if let Some(want) = &expected {
                            assert_eq!(&actual, want);
                        } else {
                            expected = Some(actual);
                        }
                        assert_eq!(arena.counts().2, usize::from(allocated));
                        drop(observed);
                        assert_eq!(arena.counts().2, 0);
                        assert!(
                            host.tasks.is_empty()
                                && host.resources.scopes.is_empty()
                                && host.resources.values.is_empty()
                        );
                    }
                }
            }
        }
    }
    assert_eq!(arena.counts(), (1, 1, 0));
}

#[test]
fn arena_child_admission_failure_retires_the_prepared_pin_without_running_the_child() {
    let arena = physical_frames::Arena::default();
    for exhaust_wait in [false, true] {
        let mut host = Host::new(json!({}), None, true, Some(10));
        if exhaust_wait {
            host.tasks.exhaust_wait_space();
        } else {
            host.tasks.exhaust_task_space();
        }
        let invocation = unsafe { Invocation::in_arena(&arena, &AWAIT_LAYOUT) }.unwrap();
        let observed = invocation.storage.as_ref().unwrap().retain().unwrap();
        let pointer = invocation.program.address.cast::<Parent>();
        let result = unsafe {
            execution::run_abi_invocation(
                &mut host,
                vec![V::String("owner".into()), V::Bool(false)],
                &AWAIT_LAYOUT,
                invocation,
                1,
            )
        };
        assert!(result.err().unwrap().contains("exhausted"));
        assert_eq!(unsafe { (*pointer).child.pc }, 0);
        assert_eq!(arena.counts(), (1, 1, 1));
        drop(observed);
        assert_eq!(arena.counts(), (1, 1, 0));
        assert!(
            host.tasks.is_empty()
                && host.resources.scopes.is_empty()
                && host.resources.values.is_empty()
        );
    }
}
