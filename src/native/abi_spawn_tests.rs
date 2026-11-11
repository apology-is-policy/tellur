use super::*;
use std::cell::Cell;
use std::mem::{align_of, offset_of, size_of};

#[repr(C)]
#[derive(Default)]
struct Leaf {
    pc: u32,
}
#[repr(C)]
#[derive(Default)]
struct Child {
    pc: u32,
    leaf: Leaf,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    mode: u32,
    waits: u32,
}
thread_local! { static LEAF_CALLS: Cell<usize> = const { Cell::new(0) }; }
unsafe fn tick(c: *mut Context<'_>, id: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, id, id, 1);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn fault(c: *mut Context<'_>, message: &str) {
    unsafe {
        let value = tellur_rt_v1_value(c, 3, 0, message.as_ptr(), message.len());
        tellur_rt_v1_fault(c, value);
    }
}
unsafe fn route(c: *mut Context<'_>, args: *const u64) -> u64 {
    unsafe {
        let mut value = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut value, 1);
        value
    }
}
unsafe extern "C" fn leaf(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        LEAF_CALLS.with(|n| n.set(n.get() + 1));
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            if !tick(c, 20) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        assert_eq!(count, 0);
        if !tick(c, 21) {
            return 0;
        }
        let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn child(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Child>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_store(c, 1, std::ptr::null(), 0, *args.add(1));
            if !tick(c, 10) {
                return 0;
            }
            let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
            tellur_rt_v1_await(c, 0, [value].as_ptr(), 1);
            f.pc = 1;
            return 3;
        }
        assert_eq!(count, 1);
        let value = route(c, args);
        tellur_rt_v1_store(c, 0, std::ptr::null(), 0, value);
        if !tick(c, 11) {
            return 0;
        }
        let fail = tellur_rt_v1_local(c, 1, std::ptr::null(), 0, 1);
        if tellur_rt_v1_bool(c, fail) != 0 {
            fault(c, "spawn child failed");
            return 0;
        }
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
    unsafe {
        let f = &mut *p.cast::<Parent>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_store(c, 1, std::ptr::null(), 0, *args.add(1));
        }
        if f.pc <= 1 {
            if !tick(c, f.pc as usize + 1) {
                return 0;
            }
            let loan = tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
            let fail = if f.pc == 0 {
                tellur_rt_v1_local(c, 1, std::ptr::null(), 0, 1)
            } else {
                tellur_rt_v1_value(c, 1, 0, std::ptr::null(), 0)
            };
            let context = &mut *c;
            if f.mode == 4 && f.pc == 0 {
                let before = context.host.tasks.clone();
                let arena = context.physical_arena.unwrap().clone();
                let counts = arena.counts();
                for (slot, handles) in [
                    (0, vec![loan, fail]),
                    (99, vec![loan, fail]),
                    (2, vec![loan]),
                    (2, vec![loan, loan]),
                ] {
                    assert!(
                        context
                            .spawn_control(
                                &CHILD,
                                "$task<internal_child>",
                                "$started<internal_child>",
                                slot,
                                &handles
                            )
                            .is_err()
                    );
                    assert!(
                        context.get(loan).is_ok()
                            && context.get(fail).is_ok()
                            && context.request.is_none()
                    );
                    assert_eq!(context.host.tasks, before);
                    assert_eq!(arena.counts(), counts);
                }
            }
            context.perform(|c| {
                c.spawn_control(
                    &CHILD,
                    "$task<internal_child>",
                    "$started<internal_child>",
                    2 + f.pc as usize,
                    &[loan, fail],
                )
            });
            f.pc += 1;
            return 6;
        }
        if f.pc == 2 {
            if f.waits == 0 {
                LEAF_CALLS.with(|n| assert_eq!(n.get(), 0));
            }
            if !tick(c, 3) {
                return 0;
            }
            if f.mode == 1 {
                fault(c, "parent before joins");
                return 0;
            }
            if (f.mode == 2 && f.waits < 1) || (f.mode == 3 && f.waits < 4) {
                f.waits += 1;
                return 1;
            }
            if f.mode == 3 {
                fault(c, "parent after children");
                return 0;
            }
            let value = tellur_rt_v1_local(c, 3, std::ptr::null(), 0, 1);
            if f.mode == 4 {
                let context = &mut *c;
                let before = context.host.tasks.clone();
                assert!(context.join_control(&LEAF_CONTROL, value).is_err());
                assert!(context.get(value).is_ok() && context.request.is_none());
                assert_eq!(context.host.tasks, before);
                context.host.tasks.exhaust_wait_space();
                let exhausted = context.host.tasks.clone();
                assert!(context.join_control(&CHILD_CONTROL, value).is_err());
                assert!(context.get(value).is_ok() && context.request.is_none());
                assert_eq!(context.host.tasks, exhausted);
                context.host.tasks = before;
            }
            (&mut *c).perform(|c| c.join_control(&CHILD_CONTROL, value));
            f.pc = 3;
            return 7;
        }
        assert_eq!(count, 1);
        let value = route(c, args);
        tellur_rt_v1_release(c, value);
        if f.pc == 3 {
            if !tick(c, 4) {
                return 0;
            }
            let value = tellur_rt_v1_local(c, 2, std::ptr::null(), 0, 1);
            (&mut *c).perform(|c| c.join_control(&CHILD_CONTROL, value));
            f.pc = 4;
            return 7;
        }
        if !tick(c, 5) {
            return 0;
        }
        let value = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        f.pc = 5;
        2
    }
}
static LEAF_CONTROL: execution::Control = execution::Control {
    name: "leaf",
    slots: 1,
    parameters: &[0],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static CHILD_CONTROL: execution::Control = execution::Control {
    name: "child",
    slots: 2,
    parameters: &[0, 1],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static PARENT_CONTROL: execution::Control = execution::Control {
    name: "parent",
    slots: 4,
    parameters: &[0, 1],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static LEAF: InvocationLayout = InvocationLayout {
    control: &LEAF_CONTROL,
    callback: leaf,
    size: size_of::<Leaf>(),
    align: align_of::<Leaf>(),
    prefix: size_of::<Leaf>(),
    children: &[],
};
static CHILD: InvocationLayout = InvocationLayout {
    control: &CHILD_CONTROL,
    callback: child,
    size: size_of::<Child>(),
    align: align_of::<Child>(),
    prefix: offset_of!(Child, leaf),
    children: &[InlineChild {
        offset: offset_of!(Child, leaf),
        layout: &LEAF,
    }],
};
static PARENT: InvocationLayout = InvocationLayout {
    control: &PARENT_CONTROL,
    callback: parent,
    size: size_of::<Parent>(),
    align: align_of::<Parent>(),
    prefix: size_of::<Parent>(),
    children: &[],
};

#[test]
fn spawned_arena_frames_keep_original_owners_and_retire_siblings_and_inline_children() {
    for mode in 0..=4 {
        for fail in [false, true] {
            for fuel in 0..=19 {
                let mut expected = None;
                for quota in [1, 3, usize::MAX] {
                    LEAF_CALLS.with(|n| n.set(0));
                    let mut frame = Parent {
                        mode,
                        ..Parent::default()
                    };
                    let invocation = unsafe { Invocation::new(&PARENT, &mut frame) }.unwrap();
                    let arena = invocation.arena.clone();
                    let original = "spawned caller loan 😀".repeat(40);
                    let buffer = original.as_ptr();
                    let mut host = Host::new(json!({}), None, true, Some(fuel));
                    let result = unsafe {
                        execution::run_abi_invocation(
                            &mut host,
                            vec![V::String(original), V::Bool(fail)],
                            &PARENT,
                            invocation,
                            quota,
                        )
                    };
                    if fuel == 19 && !fail && ![1, 3].contains(&mode) {
                        assert!(
                            result.is_ok(),
                            "{mode}/{quota}: {}",
                            result.as_ref().err().unwrap()
                        );
                    }
                    if fuel == 19 && [1, 3].contains(&mode) && !fail {
                        assert!(result.as_ref().err().is_some_and(|e| e.contains("parent ")));
                    }
                    let report = match result {
                        Ok(value) => {
                            assert!(!fail && mode != 1 && mode != 3);
                            let (_, values) = value.outcome().unwrap();
                            let V::String(text) = &values[0] else {
                                panic!()
                            };
                            assert_eq!(text.as_ptr(), buffer);
                            format!("done:{text}")
                        }
                        Err(error) => error,
                    };
                    let actual = (
                        report,
                        host.fuel,
                        frame.pc,
                        frame.waits,
                        LEAF_CALLS.with(Cell::get),
                    );
                    if let Some(want) = &expected {
                        assert_eq!(&actual, want, "{mode}/{fail}/{fuel}/{quota}");
                    } else {
                        expected = Some(actual);
                    }
                    let (chunks, slots, pins) = arena.counts();
                    assert_eq!(pins, 0);
                    assert!(chunks <= 1 && slots <= 2);
                    if mode == 0 && fuel == 19 && !fail {
                        assert_eq!((chunks, slots, pins), (1, 2, 0));
                    }
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

#[test]
fn failed_spawn_or_join_admission_retires_unselected_frames_without_running_children() {
    for exhaust_wait in [false, true] {
        LEAF_CALLS.with(|n| n.set(0));
        let mut frame = Parent::default();
        let invocation = unsafe { Invocation::new(&PARENT, &mut frame) }.unwrap();
        let arena = invocation.arena.clone();
        let mut host = Host::new(json!({}), None, true, Some(20));
        if exhaust_wait {
            host.tasks.exhaust_wait_space();
        } else {
            host.tasks.exhaust_task_space();
        }
        let result = unsafe {
            execution::run_abi_invocation(
                &mut host,
                vec![V::String("original owner".into()), V::Bool(false)],
                &PARENT,
                invocation,
                1,
            )
        };
        assert!(result.err().unwrap().contains("exhausted"));
        assert_eq!(LEAF_CALLS.with(Cell::get), 0);
        assert_eq!(arena.counts().2, 0);
        assert!(
            host.tasks.is_empty()
                && host.resources.scopes.is_empty()
                && host.resources.values.is_empty(),
            "wait={exhaust_wait}: tasks_empty={} scopes={:?} values={:?}",
            host.tasks.is_empty(),
            host.resources.scopes.len(),
            host.resources.values.len()
        );
    }
}
