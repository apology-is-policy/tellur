use super::*;
use execution::{Activation, Control, Step};
use serde_json::{Value as Json, json};
use std::mem::{align_of, offset_of, size_of};

#[repr(C)]
#[derive(Default)]
struct Leaf {
    pc: u32,
}
#[repr(C)]
#[derive(Default)]
struct Constructor {
    pc: u32,
    leaf: Leaf,
}
#[repr(C)]
#[derive(Default)]
struct Root {
    pc: u32,
    constructor: Constructor,
}

unsafe fn tick(c: *mut Context<'_>, n: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, n, n + 10, 2);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn store(c: *mut Context<'_>, slot: usize, value: u64) {
    unsafe { tellur_rt_v1_store(c, slot, std::ptr::null(), 0, value) }
}
unsafe fn take(c: *mut Context<'_>, slot: usize) -> u64 {
    unsafe { tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 1) }
}
unsafe extern "C" fn root(
    c: *mut Context<'_>,
    frame: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *frame.cast::<Root>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 1) {
                return 0;
            }
            // Constructor delivery cannot allocate even one outcome handle.
            (*c).outcome_base = u64::MAX;
            let value = take(c, 0);
            tellur_rt_v1_construct_child(c, 0, 1, [value].as_ptr(), 1);
            f.pc = 1;
            return 19;
        }
        assert_eq!(f.pc, 1);
        assert_eq!(count, 0);
        assert!((*c).outcomes.is_empty());
        if !tick(c, 6) {
            return 0;
        }
        let value = take(c, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn constructor(
    c: *mut Context<'_>,
    frame: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *frame.cast::<Constructor>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 2) {
                return 0;
            }
            tellur_rt_v1_run_child(c, 0, std::ptr::null(), 0);
            f.pc = 1;
            return 5;
        }
        assert_eq!(f.pc, 1);
        assert_eq!(count, 1);
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, std::ptr::null_mut(), 0);
        if !tick(c, 5) {
            return 0;
        }
        let value = take(c, 0);
        tellur_rt_v1_constructed(c, value);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn leaf(c: *mut Context<'_>, frame: *mut u8, _: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *frame.cast::<Leaf>();
        assert_eq!(count, 0);
        if f.pc == 0 {
            if !tick(c, 3) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        if !tick(c, 4) {
            return 0;
        }
        if (&(*c).host.manifest)["fail"] == true {
            let message = (*c).put(V::String("constructor child failed".into()));
            tellur_rt_v1_fault(c, message);
            return 0;
        }
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        f.pc = 2;
        2
    }
}
macro_rules! control {
    ($id:ident,$name:expr,$slots:expr,$parameters:expr,$constructor:expr,$resume:ident) => {
        static $id: Control = Control {
            name: $name,
            slots: $slots,
            atoms: 0,
            parameters: $parameters,
            retained: &[],
            constructor: $constructor,
            access: None,
            resume: $resume,
        };
    };
}
control!(ROOT_CONTROL, "root", 2, &[0], false, resume_invocation);
control!(
    CONSTRUCTOR_CONTROL,
    "box.constructor",
    1,
    &[0],
    true,
    resume_invocation
);
control!(LEAF_CONTROL, "leaf", 0, &[], false, resume_invocation);
static LEAF: InvocationLayout = InvocationLayout {
    control: &LEAF_CONTROL,
    callback: leaf,
    size: size_of::<Leaf>(),
    align: align_of::<Leaf>(),
    prefix: size_of::<Leaf>(),
    children: &[],
    scopes: None,
};
static CONSTRUCTOR: InvocationLayout = InvocationLayout {
    control: &CONSTRUCTOR_CONTROL,
    callback: constructor,
    size: size_of::<Constructor>(),
    align: align_of::<Constructor>(),
    prefix: offset_of!(Constructor, leaf),
    children: &[InlineChild {
        offset: offset_of!(Constructor, leaf),
        layout: &LEAF,
    }],
    scopes: None,
};
static ROOT: InvocationLayout = InvocationLayout {
    control: &ROOT_CONTROL,
    callback: root,
    size: size_of::<Root>(),
    align: align_of::<Root>(),
    prefix: offset_of!(Root, constructor),
    children: &[InlineChild {
        offset: offset_of!(Root, constructor),
        layout: &CONSTRUCTOR,
    }],
    scopes: None,
};

fn source_tick(h: &mut Host, n: usize) -> R<()> {
    h.at(n);
    h.tick(n + 10, 2)
}
fn rust_root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let base = a.base();
    let pc = a.pc;
    a.pc += 1;
    match pc {
        0 => {
            source_tick(h, 1)?;
            Ok(Step::Call {
                control: &RUST_CONSTRUCTOR,
                arguments: vec![v[base].take().unwrap()],
                destination: Some(base + 1),
                deliver: deliver_constructor,
            })
        }
        1 => {
            source_tick(h, 6)?;
            Ok(Step::Complete(Completion::Outcome((
                "done".into(),
                vec![v[base + 1].take().unwrap()],
            ))))
        }
        _ => unreachable!(),
    }
}
fn deliver_constructor(_: &mut Activation, _: &mut Frames, c: Completion) -> R<()> {
    c.constructed()
}
fn deliver_leaf(_: &mut Activation, _: &mut Frames, c: Completion) -> R<()> {
    let (n, values) = c.outcome()?;
    assert_eq!(n, "done");
    assert!(values.is_empty());
    Ok(())
}
fn rust_constructor(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let pc = a.pc;
    a.pc += 1;
    match pc {
        0 => {
            source_tick(h, 2)?;
            Ok(Step::Call {
                control: &RUST_LEAF,
                arguments: vec![],
                destination: None,
                deliver: deliver_leaf,
            })
        }
        1 => {
            source_tick(h, 5)?;
            v[a.destination.unwrap()] = v[a.base()].take();
            Ok(Step::Complete(Completion::Constructed))
        }
        _ => unreachable!(),
    }
}
fn rust_leaf(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    let pc = a.pc;
    a.pc += 1;
    match pc {
        0 => {
            source_tick(h, 3)?;
            Ok(Step::Yield)
        }
        1 => {
            source_tick(h, 4)?;
            if h.manifest["fail"] == true {
                h.fault(V::String("constructor child failed".into()))?;
            }
            Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
        }
        _ => unreachable!(),
    }
}
control!(RUST_ROOT, "root", 2, &[0], false, rust_root);
control!(
    RUST_CONSTRUCTOR,
    "box.constructor",
    1,
    &[0],
    true,
    rust_constructor
);
control!(RUST_LEAF, "leaf", 0, &[], false, rust_leaf);
fn host(fuel: usize, fail: bool) -> Host {
    Host::new(json!({"fail":fail}), None, true, Some(fuel))
}
fn observed(h: &mut Host, result: R<Completion>, buffer: *const u8) -> Json {
    let result = match result {
        Ok(Completion::Outcome((name, fields))) => {
            let V::String(text) = &fields[0] else {
                panic!()
            };
            assert_eq!(text.as_ptr(), buffer);
            json!({"outcome":name,"fields":fields})
        }
        Ok(_) => panic!(),
        Err(error) => json!({"fault":h.activations.failure("root",fault::Phase::Execution,&error)}),
    };
    assert!(h.tasks.is_empty() && h.resources.scopes.is_empty() && h.resources.values.is_empty());
    json!({"result":result,"fuel":h.fuel,"events":h.events})
}
#[test]
fn retained_constructor_calls_preserve_destinations_buffers_faults_fuel_and_quotas() {
    for fail in [false, true] {
        for fuel in 0..=10 {
            let text = "original owned constructor buffer".repeat(30);
            let buffer = text.as_ptr();
            let mut h = host(fuel, fail);
            let result = execution::run_source_entry(&mut h, vec![V::String(text)], &RUST_ROOT);
            let expected = observed(&mut h, result, buffer);
            for quota in [1, 3, usize::MAX] {
                for arena in [false, true] {
                    let text = "original owned constructor buffer".repeat(30);
                    let buffer = text.as_ptr();
                    let mut h = host(fuel, fail);
                    let mut frame = Root::default();
                    let pool = physical_frames::Arena::default();
                    let invocation = if arena {
                        unsafe { Invocation::in_arena(&pool, &ROOT) }
                    } else {
                        unsafe { Invocation::new(&ROOT, &mut frame) }
                    }
                    .unwrap();
                    let result = unsafe {
                        execution::run_abi_invocation(
                            &mut h,
                            vec![V::String(text)],
                            &ROOT,
                            invocation,
                            quota,
                        )
                    };
                    assert_eq!(
                        observed(&mut h, result, buffer),
                        expected,
                        "{fail}/{fuel}/{quota}/{arena}"
                    );
                    assert_eq!(pool.counts().2, 0);
                }
            }
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct CheckedRoot {
    root: Root,
    duplicate: Constructor,
    ordinary: Leaf,
}
control!(
    TWO_ARGUMENT_CONTROL,
    "two.constructor",
    2,
    &[0, 1],
    true,
    resume_invocation
);
static TWO_ARGUMENTS: InvocationLayout = InvocationLayout {
    control: &TWO_ARGUMENT_CONTROL,
    ..CONSTRUCTOR
};
static CHECKED_ROOT: InvocationLayout = InvocationLayout {
    callback: checked_root,
    size: size_of::<CheckedRoot>(),
    children: &[
        InlineChild {
            offset: offset_of!(Root, constructor),
            layout: &CONSTRUCTOR,
        },
        InlineChild {
            offset: offset_of!(CheckedRoot, duplicate),
            layout: &TWO_ARGUMENTS,
        },
        InlineChild {
            offset: offset_of!(CheckedRoot, ordinary),
            layout: &LEAF,
        },
    ],
    ..ROOT
};
unsafe fn assert_rejected(c: *mut Context<'_>, handle: u64, request: impl FnOnce()) {
    unsafe {
        let before = (&*c).request.as_ref().map(RequestedInvocation::tag);
        let events = (&*c).host.events.len();
        request();
        assert!((&*c).error.is_some());
        assert!((&*c).get(handle).is_ok());
        tellur_rt_v1_emit(c, handle);
        assert_eq!((&*c).host.events.len(), events);
        assert_eq!((&*c).request.as_ref().map(RequestedInvocation::tag), before);
        (&mut *c).error = None;
    }
}
unsafe extern "C" fn checked_root(
    c: *mut Context<'_>,
    frame: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let pc = (*frame.cast::<Root>()).pc;
        if pc == 0 {
            let value = *args;
            for (child, slot, argc) in [(99, 1, 1), (2, 1, 1), (0, 2, 1), (0, 1, 0)] {
                assert_rejected(c, value, || {
                    tellur_rt_v1_construct_child(c, child, slot, [value].as_ptr(), argc)
                });
            }
            assert_rejected(c, value, || {
                tellur_rt_v1_construct_child(c, 1, 1, [value, value].as_ptr(), 2)
            });
            let occupied = (&mut *c).put(V::Unit);
            store(c, 1, occupied);
            assert_rejected(c, value, || {
                tellur_rt_v1_construct_child(c, 0, 1, [value].as_ptr(), 1)
            });
            let occupied = take(c, 1);
            (&mut *c).take(occupied).unwrap();
            assert_rejected(c, value, || {
                tellur_rt_v1_run_child(c, 0, [value].as_ptr(), 1)
            });
            assert_rejected(c, value, || tellur_rt_v1_await(c, 0, [value].as_ptr(), 1));
            assert_rejected(c, value, || {
                tellur_rt_v1_supervise_children(c, 0, 2, [value].as_ptr(), 1)
            });
            assert!(
                (&mut *c)
                    .defer_control(&CONSTRUCTOR_CONTROL, "$task<constructor>", &[value])
                    .is_err()
            );
            assert!(
                (&mut *c)
                    .spawn_control(
                        &CONSTRUCTOR,
                        "$task<constructor>",
                        "$started<constructor>",
                        1,
                        &[value]
                    )
                    .is_err()
            );
            assert!(
                (&mut *c)
                    .empty_task_set(
                        &CONSTRUCTOR_CONTROL,
                        "$tasks<constructor>",
                        "$started<constructor>"
                    )
                    .is_err()
            );
            assert!((&*c).get(value).is_ok());
        }
        let tag = root(c, frame, args, count);
        if pc == 0 {
            let extra = (&mut *c).put(V::Unit);
            assert_rejected(c, extra, || {
                tellur_rt_v1_construct_child(c, 0, 1, [extra].as_ptr(), 1)
            });
            (&mut *c).take(extra).unwrap();
        }
        tag
    }
}
#[test]
fn retained_constructor_requests_preflight_all_roles_destinations_and_captures() {
    let mut h = host(100, false);
    let mut frame = CheckedRoot::default();
    let text = "atomic constructor capture".to_owned();
    let buffer = text.as_ptr();
    let invocation = unsafe { Invocation::new(&CHECKED_ROOT, &mut frame) }.unwrap();
    let result = unsafe {
        execution::run_abi_invocation(&mut h, vec![V::String(text)], &CHECKED_ROOT, invocation, 1)
    };
    assert_eq!(
        observed(&mut h, result, buffer)["result"]["outcome"],
        "done"
    );
}
#[test]
fn retained_constructors_cannot_be_roots_or_spawned_arena_tasks() {
    let mut frame = Constructor::default();
    let pool = physical_frames::Arena::default();
    assert!(CONSTRUCTOR.validate().is_ok());
    assert!(unsafe { Invocation::new(&CONSTRUCTOR, &mut frame) }.is_err());
    assert!(unsafe { Invocation::in_arena(&pool, &CONSTRUCTOR) }.is_err());
    assert_eq!(pool.counts(), (0, 0, 0));
    static BAD: Control = Control {
        retained: &[0],
        ..CONSTRUCTOR_CONTROL
    };
    let layout = InvocationLayout {
        control: &BAD,
        ..CONSTRUCTOR
    };
    assert!(layout.validate().is_err());
}
#[test]
fn retained_constructor_child_attachment_preserves_role_and_stable_destination() {
    let pool = physical_frames::Arena::default();
    let mut parent = unsafe { Invocation::in_arena(&pool, &ROOT) }.unwrap();
    let mut values = Frames::default();
    let frame = values.enter(2).unwrap();
    let destination = values.local_storage(&frame, 1).unwrap();
    parent.call = Some(parent.program.child(0).unwrap());
    parent.call_storage = Some(parent.storage.as_ref().unwrap().retain().unwrap());
    parent.construction = Some(destination.clone());
    assert!(parent.take_call(&CONSTRUCTOR_CONTROL).is_err());
    parent.phase = InvocationPhase::Awaiting;
    assert!(parent.take_call(&LEAF_CONTROL).is_err());
    let destination_saved = parent.construction.take();
    assert!(parent.take_call(&CONSTRUCTOR_CONTROL).is_err());
    parent.construction = destination_saved;
    let child = parent.take_call(&CONSTRUCTOR_CONTROL).unwrap();
    assert_eq!(
        child.state.as_ref().unwrap().destination.as_ref(),
        Some(&destination)
    );
    assert_eq!(parent.construction.as_ref(), Some(&destination));
    assert_eq!(pool.counts().2, 2);
    assert!(parent.take_call(&CONSTRUCTOR_CONTROL).is_err());
    values.retire(&frame).unwrap();
    let reused = values.enter(2).unwrap();
    assert_eq!(reused.base(), frame.base());
    assert!(
        values
            .require_storage(child.state.as_ref().unwrap().destination.as_ref().unwrap())
            .is_err()
    );
    drop(child);
    drop(parent);
    assert_eq!(pool.counts().2, 0);
    values.retire(&reused).unwrap();
}

unsafe extern "C" fn borrowed_root(
    c: *mut Context<'_>,
    frame: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *frame.cast::<Root>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 1) {
                return 0;
            }
            let ty = if (&(*c).host.manifest)["exclusive"] == true {
                "&mutable String"
            } else {
                "&String"
            };
            let reference =
                tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, ty.as_ptr(), ty.len());
            tellur_rt_v1_construct_child(c, 0, 1, [reference].as_ptr(), 1);
            f.pc = 1;
            return 19;
        }
        assert_eq!(count, 0);
        if !tick(c, 6) {
            return 0;
        }
        let c = &mut *c;
        let root = c.local(1).unwrap();
        let value = read_place(c.storage.values(), root, &[], true).unwrap();
        c.host.emit(value).unwrap();
        tellur_rt_v1_release_local(c, 1);
        let owner = take(c, 0);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [owner].as_ptr(), 1);
        f.pc = 2;
        2
    }
}
fn rust_borrowed_root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let base = a.base();
    let pc = a.pc;
    a.pc += 1;
    match pc {
        0 => {
            source_tick(h, 1)?;
            let ty = if h.manifest["exclusive"] == true {
                "&mutable String"
            } else {
                "&String"
            };
            Ok(Step::Call {
                control: &RUST_CONSTRUCTOR,
                arguments: vec![V::Reference {
                    ty: ty.into(),
                    root: base,
                    fields: vec![],
                    evidence: Default::default(),
                }],
                destination: Some(base + 1),
                deliver: deliver_constructor,
            })
        }
        1 => {
            source_tick(h, 6)?;
            h.emit(read_place(v, base + 1, &[], true)?)?;
            v[base + 1] = None;
            Ok(Step::Complete(Completion::Outcome((
                "done".into(),
                vec![v[base].take().unwrap()],
            ))))
        }
        _ => unreachable!(),
    }
}
control!(
    RUST_BORROWED_ROOT,
    "root",
    2,
    &[0],
    false,
    rust_borrowed_root
);
static BORROWED_ROOT: InvocationLayout = InvocationLayout {
    callback: borrowed_root,
    ..ROOT
};
#[test]
fn retained_constructor_results_keep_shared_and_exclusive_caller_storage_through_handoffs() {
    for exclusive in [false, true] {
        for fail in [false, true] {
            for fuel in 0..=10 {
                let text = "borrowed constructor owner".repeat(40);
                let buffer = text.as_ptr();
                let mut h = host(fuel, fail);
                h.manifest["exclusive"] = json!(exclusive);
                let result =
                    execution::run_source_entry(&mut h, vec![V::String(text)], &RUST_BORROWED_ROOT);
                let expected = observed(&mut h, result, buffer);
                for quota in [1, 3, usize::MAX] {
                    for arena in [false, true] {
                        let text = "borrowed constructor owner".repeat(40);
                        let buffer = text.as_ptr();
                        let mut h = host(fuel, fail);
                        h.manifest["exclusive"] = json!(exclusive);
                        let mut frame = Root::default();
                        let pool = physical_frames::Arena::default();
                        let invocation = if arena {
                            unsafe { Invocation::in_arena(&pool, &BORROWED_ROOT) }
                        } else {
                            unsafe { Invocation::new(&BORROWED_ROOT, &mut frame) }
                        }
                        .unwrap();
                        let result = unsafe {
                            execution::run_abi_invocation(
                                &mut h,
                                vec![V::String(text)],
                                &BORROWED_ROOT,
                                invocation,
                                quota,
                            )
                        };
                        assert_eq!(
                            observed(&mut h, result, buffer),
                            expected,
                            "exclusive={exclusive} fail={fail} fuel={fuel} quota={quota} arena={arena}"
                        );
                        assert_eq!(pool.counts().2, 0);
                    }
                }
            }
        }
    }
}
unsafe extern "C" fn malformed_constructor(
    c: *mut Context<'_>,
    _: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 1);
        store(c, 0, *args);
        if !tick(c, 2) {
            return 0;
        }
        let value = take(c, 0);
        let mode = (&(*c).host.manifest)["mode"].as_u64().unwrap();
        if mode == 0 {
            tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
            return 2;
        }
        if mode == 1 {
            tellur_rt_v1_constructed(c, u64::MAX);
            return 2;
        }
        if mode == 2 {
            (&mut *c).take(value).unwrap();
            return 2;
        }
        tellur_rt_v1_constructed(c, value);
        match mode {
            3 => 1, // A constructor cannot publish and then yield.
            4 => {
                let duplicate = (&mut *c).put(V::String("duplicate".into()));
                tellur_rt_v1_constructed(c, duplicate);
                2
            }
            5 => {
                let failure = (&mut *c).put(V::String("fault after publication".into()));
                tellur_rt_v1_fault(c, failure);
                0
            }
            6 => {
                tellur_rt_v1_run_child(c, 0, std::ptr::null(), 0);
                5
            }
            _ => unreachable!(),
        }
    }
}
static MALFORMED_CONSTRUCTOR: InvocationLayout = InvocationLayout {
    callback: malformed_constructor,
    ..CONSTRUCTOR
};
static MALFORMED_ROOT: InvocationLayout = InvocationLayout {
    children: &[InlineChild {
        offset: offset_of!(Root, constructor),
        layout: &MALFORMED_CONSTRUCTOR,
    }],
    ..ROOT
};
#[test]
fn malformed_retained_constructor_callbacks_never_deliver_an_outcome_or_partial_value() {
    for (mode, message) in [
        (0, "constructor requires destination completion"),
        (1, "invalid or consumed ABI value handle"),
        (2, "malformed retained ABI step"),
        (3, "malformed retained ABI step"),
        (4, "duplicate ABI completion"),
        (5, "fault after publication"),
        (6, "duplicate ABI continuation request"),
    ] {
        for quota in [1, 3, usize::MAX] {
            let mut h = host(100, false);
            h.manifest["mode"] = json!(mode);
            let mut frame = Root::default();
            let invocation = unsafe { Invocation::new(&MALFORMED_ROOT, &mut frame) }.unwrap();
            let result = unsafe {
                execution::run_abi_invocation(
                    &mut h,
                    vec![V::String("partial owner".into())],
                    &MALFORMED_ROOT,
                    invocation,
                    quota,
                )
            };
            assert!(
                result.err().unwrap().contains(message),
                "mode={mode} quota={quota}"
            );
            assert_eq!(frame.pc, 1);
            assert!(
                h.tasks.is_empty()
                    && h.resources.scopes.is_empty()
                    && h.resources.values.is_empty()
            );
        }
    }
}
#[test]
fn abandoning_constructor_work_clears_pending_delivery_but_keeps_its_caller_destination() {
    let mut values = Frames::default();
    let frame = values.enter(2).unwrap();
    let caller = values.local_storage(&frame, 0).unwrap();
    let destination = values.local_storage(&frame, 1).unwrap();
    let mut physical = Root::default();
    let mut invocation = unsafe { Invocation::new(&ROOT, &mut physical) }.unwrap();
    invocation.state.as_mut().unwrap().destination = Some(caller.clone());
    invocation.construction = Some(destination);
    invocation.call = Some(invocation.program.child(0).unwrap());
    invocation.phase = InvocationPhase::Awaiting;
    invocation.abandon_scope_work().unwrap();
    assert!(invocation.call.is_none() && invocation.construction.is_none());
    assert_eq!(
        invocation.state.as_ref().unwrap().destination.as_ref(),
        Some(&caller)
    );
    assert_eq!(invocation.phase, InvocationPhase::Failed);
    values.retire(&frame).unwrap();
}

#[repr(C)]
#[derive(Default)]
struct Nested {
    pc: u32,
    inner: Constructor,
}
#[repr(C)]
#[derive(Default)]
struct NestedRoot {
    pc: u32,
    outer: Nested,
}
control!(
    NESTED_CONTROL,
    "outer.constructor",
    2,
    &[0],
    true,
    resume_invocation
);
static NESTED: InvocationLayout = InvocationLayout {
    control: &NESTED_CONTROL,
    callback: nested,
    size: size_of::<Nested>(),
    align: align_of::<Nested>(),
    prefix: offset_of!(Nested, inner),
    children: &[InlineChild {
        offset: offset_of!(Nested, inner),
        layout: &CONSTRUCTOR,
    }],
    scopes: None,
};
static NESTED_ROOT: InvocationLayout = InvocationLayout {
    size: size_of::<NestedRoot>(),
    align: align_of::<NestedRoot>(),
    prefix: offset_of!(NestedRoot, outer),
    children: &[InlineChild {
        offset: offset_of!(NestedRoot, outer),
        layout: &NESTED,
    }],
    ..ROOT
};
unsafe extern "C" fn nested(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Nested>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
        } else {
            assert_eq!(count, 0);
        }
        if f.pc < 2 {
            if !tick(c, 7 + f.pc as usize) {
                return 0;
            }
            let value = take(c, if f.pc == 0 { 0 } else { 1 });
            f.inner = Constructor::default();
            f.pc += 1;
            tellur_rt_v1_construct_child(c, 0, 1, [value].as_ptr(), 1);
            return 19;
        }
        assert_eq!(f.pc, 2);
        if !tick(c, 9) {
            return 0;
        }
        let value = take(c, 1);
        tellur_rt_v1_constructed(c, value);
        2
    }
}
fn rust_nested(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let pc = a.pc;
    a.pc += 1;
    if pc < 2 {
        source_tick(h, 7 + pc)?;
        return Ok(Step::Call {
            control: &RUST_CONSTRUCTOR,
            arguments: vec![v[a.base() + if pc == 0 { 0 } else { 1 }].take().unwrap()],
            destination: Some(a.base() + 1),
            deliver: deliver_constructor,
        });
    }
    source_tick(h, 9)?;
    v[a.destination.unwrap()] = v[a.base() + 1].take();
    Ok(Step::Complete(Completion::Constructed))
}
control!(RUST_NESTED, "outer.constructor", 2, &[0], true, rust_nested);
fn rust_nested_root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let step = if h.manifest["borrowed"] == true {
        rust_borrowed_root(h, v, a)?
    } else {
        rust_root(h, v, a)?
    };
    Ok(match step {
        Step::Call {
            arguments,
            destination,
            deliver,
            ..
        } => Step::Call {
            control: &RUST_NESTED,
            arguments,
            destination,
            deliver,
        },
        other => other,
    })
}
control!(RUST_NESTED_ROOT, "root", 2, &[0], false, rust_nested_root);
static NESTED_BORROWED_ROOT: InvocationLayout = InvocationLayout {
    callback: borrowed_root,
    ..NESTED_ROOT
};
#[test]
fn nested_repeated_constructors_keep_ancestor_destinations_and_original_owned_or_borrowed_values() {
    for borrowed in [false, true] {
        for fail in [false, true] {
            for fuel in 0..=20 {
                let mut h = host(fuel, fail);
                h.manifest["borrowed"] = json!(borrowed);
                h.manifest["exclusive"] = json!(true);
                let text = "nested constructor buffer".repeat(30);
                let buffer = text.as_ptr();
                let result =
                    execution::run_source_entry(&mut h, vec![V::String(text)], &RUST_NESTED_ROOT);
                let expected = observed(&mut h, result, buffer);
                for quota in [1, 3, usize::MAX] {
                    for arena in [false, true] {
                        let layout = if borrowed {
                            &NESTED_BORROWED_ROOT
                        } else {
                            &NESTED_ROOT
                        };
                        let pool = physical_frames::Arena::default();
                        let mut frame = NestedRoot::default();
                        let invocation = if arena {
                            unsafe { Invocation::in_arena(&pool, layout) }
                        } else {
                            unsafe { Invocation::new(layout, &mut frame) }
                        }
                        .unwrap();
                        let mut h = host(fuel, fail);
                        h.manifest["exclusive"] = json!(true);
                        let text = "nested constructor buffer".repeat(30);
                        let buffer = text.as_ptr();
                        let result = unsafe {
                            execution::run_abi_invocation(
                                &mut h,
                                vec![V::String(text)],
                                layout,
                                invocation,
                                quota,
                            )
                        };
                        assert_eq!(
                            observed(&mut h, result, buffer),
                            expected,
                            "{borrowed}/{fail}/{fuel}/{quota}/{arena}"
                        );
                        assert_eq!(pool.counts().2, 0);
                    }
                }
            }
        }
    }
}

fn host_mode(h: &Host) -> u64 {
    h.manifest["mode"].as_u64().unwrap()
}
unsafe extern "C" fn waiting_leaf(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 0);
            if !tick(c, 3) {
                return 0;
            }
            (&mut *c).perform(|c| {
                if host_mode(c.host) == 1 {
                    c.host.cancel_entry();
                }
                c.host_control(
                    &LEAF_CONTROL,
                    HostLeaf {
                        operation: "pause",
                        cancellable: true,
                    },
                    &[],
                )
            });
            f.pc = 1;
            return 9;
        }
        assert_eq!(count, 1);
        if tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, std::ptr::null_mut(), 0) == 0 {
            tellur_rt_v1_route(c, *args, b"cancelled".as_ptr(), 9, std::ptr::null_mut(), 0);
        }
        if !tick(c, 4) {
            return 0;
        }
        if host_mode((&*c).host) == 3 {
            let message = (&mut *c).put(V::String("after constructor host wait".into()));
            tellur_rt_v1_fault(c, message);
            return 0;
        }
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        2
    }
}
fn deliver_host(_: &mut Activation, _: &mut Frames, name: String, fields: Vec<V>) -> R<()> {
    assert!(name == "done" || name == "cancelled");
    assert!(fields.is_empty());
    Ok(())
}
fn rust_waiting_leaf(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        a.pc = 1;
        source_tick(h, 3)?;
        if host_mode(h) == 1 {
            h.cancel_entry();
        }
        return Ok(Step::Host {
            operation: "pause",
            arguments: vec![],
            deliver: deliver_host,
            cancellable: true,
        });
    }
    source_tick(h, 4)?;
    if host_mode(h) == 3 {
        h.fault(V::String("after constructor host wait".into()))?;
    }
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
control!(RUST_WAITING_LEAF, "leaf", 0, &[], false, rust_waiting_leaf);
fn rust_waiting_constructor(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    Ok(match rust_constructor(h, v, a)? {
        Step::Call {
            arguments,
            destination,
            deliver,
            ..
        } => Step::Call {
            control: &RUST_WAITING_LEAF,
            arguments,
            destination,
            deliver,
        },
        other => other,
    })
}
control!(
    RUST_WAITING_CONSTRUCTOR,
    "box.constructor",
    1,
    &[0],
    true,
    rust_waiting_constructor
);
fn rust_waiting_root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    Ok(match rust_borrowed_root(h, v, a)? {
        Step::Call {
            arguments,
            destination,
            deliver,
            ..
        } => Step::Call {
            control: &RUST_WAITING_CONSTRUCTOR,
            arguments,
            destination,
            deliver,
        },
        other => other,
    })
}
control!(RUST_WAITING_ROOT, "root", 2, &[0], false, rust_waiting_root);
static WAITING_LEAF: InvocationLayout = InvocationLayout {
    callback: waiting_leaf,
    ..LEAF
};
static WAITING_CONSTRUCTOR: InvocationLayout = InvocationLayout {
    children: &[InlineChild {
        offset: offset_of!(Constructor, leaf),
        layout: &WAITING_LEAF,
    }],
    ..CONSTRUCTOR
};
static WAITING_ROOT: InvocationLayout = InvocationLayout {
    callback: borrowed_root,
    children: &[InlineChild {
        offset: offset_of!(Root, constructor),
        layout: &WAITING_CONSTRUCTOR,
    }],
    ..ROOT
};
fn waiting_host(mode: u64, waits: u32, fuel: usize) -> Host {
    let calls = if mode == 1 {
        vec![]
    } else {
        vec![ScriptCall {
            operation: "pause".into(),
            outcome: if mode == 2 { "invalid" } else { "done" }.into(),
            fields: vec![],
            waits,
        }]
    };
    Host::new(
        json!({"mode":mode,"exclusive":true,"primitives":[{"name":"pause","async":true,"returns":true,"parameters":[],"outcomes":[{"name":"done","fields":[]}]}]}),
        Some(calls),
        true,
        Some(fuel),
    )
}
#[test]
fn retained_constructor_destinations_survive_pending_host_operations_cancellation_and_driver_faults()
 {
    for mode in 0..4 {
        for waits in [0, 3, 9] {
            for fuel in 0..=10 {
                let text = "host waiting owner".repeat(30);
                let buffer = text.as_ptr();
                let mut h = waiting_host(mode, waits, fuel);
                let result =
                    execution::run_source_entry(&mut h, vec![V::String(text)], &RUST_WAITING_ROOT);
                let expected = observed(&mut h, result, buffer);
                for quota in [1, 3, usize::MAX] {
                    for arena in [false, true] {
                        let pool = physical_frames::Arena::default();
                        let mut frame = Root::default();
                        let invocation = if arena {
                            unsafe { Invocation::in_arena(&pool, &WAITING_ROOT) }
                        } else {
                            unsafe { Invocation::new(&WAITING_ROOT, &mut frame) }
                        }
                        .unwrap();
                        let text = "host waiting owner".repeat(30);
                        let buffer = text.as_ptr();
                        let mut h = waiting_host(mode, waits, fuel);
                        let result = unsafe {
                            execution::run_abi_invocation(
                                &mut h,
                                vec![V::String(text)],
                                &WAITING_ROOT,
                                invocation,
                                quota,
                            )
                        };
                        assert_eq!(
                            observed(&mut h, result, buffer),
                            expected,
                            "{mode}/{waits}/{fuel}/{quota}/{arena}"
                        );
                        assert_eq!(pool.counts().2, 0);
                    }
                }
            }
        }
    }
}

thread_local! {static DROPS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
struct PartialField;
impl Drop for PartialField {
    fn drop(&mut self) {
        DROPS.with(|n| n.set(n.get() + 1));
    }
}
fn partial_field(h: &mut Host) -> V {
    h.resources.insert("partial", PartialField).unwrap()
}
static CONSTRUCTOR_SCOPE: execution::BoundaryControl = execution::BoundaryControl {
    name: "box.constructor$boundary@2",
    body: 1,
    handler: 4,
    after: 3,
    retained: &[0],
    retire: &[1],
    work: &[1],
    handling: &[2],
    fault_slot: 2,
};
static CONSTRUCTOR_SCOPES: ScopeLayout = ScopeLayout {
    loops: &[],
    states: 5,
    boundaries: &[&CONSTRUCTOR_SCOPE],
};
control!(
    RECOVERING_CONTROL,
    "box.constructor",
    3,
    &[0],
    true,
    resume_invocation
);
static RECOVERING_CONSTRUCTOR: InvocationLayout = InvocationLayout {
    control: &RECOVERING_CONTROL,
    callback: recovering_constructor,
    scopes: Some(&CONSTRUCTOR_SCOPES),
    ..CONSTRUCTOR
};
static RECOVERING_ROOT: InvocationLayout = InvocationLayout {
    children: &[InlineChild {
        offset: offset_of!(Root, constructor),
        layout: &RECOVERING_CONSTRUCTOR,
    }],
    ..ROOT
};
unsafe extern "C" fn recovering_constructor(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Constructor>();
        f.pc = tellur_rt_v1_resume_state(c, f.pc);
        match f.pc {
            0 => {
                assert_eq!(count, 1);
                store(c, 0, *args);
                if !tick(c, 2) {
                    return 0;
                }
                tellur_rt_v1_scope(c, 0, 0);
                13
            }
            1 => {
                if !tick(c, 7) {
                    return 0;
                }
                let value = (&mut *c).perform(|c| {
                    let v = partial_field(c.host);
                    Ok(c.put(v))
                });
                store(c, 1, value);
                f.pc = 2;
                tellur_rt_v1_run_child(c, 0, std::ptr::null(), 0);
                5
            }
            2 => {
                assert_eq!(count, 1);
                tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, std::ptr::null_mut(), 0);
                if !tick(c, 8) {
                    return 0;
                }
                let v = take(c, 1);
                (&mut *c).perform(|c| {
                    let v = c.take(v)?;
                    c.host.release_frame(&[Some(v)]);
                    Ok(())
                });
                tellur_rt_v1_scope(c, 0, 1);
                14
            }
            3 => {
                if !tick(c, 5) {
                    return 0;
                }
                let v = take(c, 0);
                tellur_rt_v1_constructed(c, v);
                2
            }
            4 => {
                assert_eq!(count, 0);
                assert_eq!(DROPS.with(|n| n.get()), 1);
                assert!((&*c).host.resources.values.is_empty());
                if !tick(c, 9) {
                    return 0;
                }
                let v = take(c, 0);
                tellur_rt_v1_constructed(c, v);
                2
            }
            _ => unreachable!(),
        }
    }
}
fn rust_recovering_constructor(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    match a.pc {
        0 => {
            source_tick(h, 2)?;
            Ok(Step::Boundary(&CONSTRUCTOR_SCOPE))
        }
        1 => {
            source_tick(h, 7)?;
            v[a.base() + 1] = Some(partial_field(h));
            a.pc = 2;
            Ok(Step::Call {
                control: &RUST_LEAF,
                arguments: vec![],
                destination: None,
                deliver: deliver_leaf,
            })
        }
        2 => {
            source_tick(h, 8)?;
            h.release_frame(&[v[a.base() + 1].take()]);
            Ok(Step::BoundaryEnd(&CONSTRUCTOR_SCOPE))
        }
        3 => {
            source_tick(h, 5)?;
            v[a.destination.unwrap()] = v[a.base()].take();
            Ok(Step::Complete(Completion::Constructed))
        }
        4 => {
            assert_eq!(DROPS.with(|n| n.get()), 1);
            assert!(h.resources.values.is_empty());
            source_tick(h, 9)?;
            v[a.destination.unwrap()] = v[a.base()].take();
            Ok(Step::Complete(Completion::Constructed))
        }
        _ => unreachable!(),
    }
}
control!(
    RUST_RECOVERING_CONSTRUCTOR,
    "box.constructor",
    3,
    &[0],
    true,
    rust_recovering_constructor
);
fn rust_recovering_root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    Ok(match rust_root(h, v, a)? {
        Step::Call {
            arguments,
            destination,
            deliver,
            ..
        } => Step::Call {
            control: &RUST_RECOVERING_CONSTRUCTOR,
            arguments,
            destination,
            deliver,
        },
        other => other,
    })
}
control!(
    RUST_RECOVERING_ROOT,
    "root",
    2,
    &[0],
    false,
    rust_recovering_root
);
#[test]
fn retained_constructor_local_recovery_retires_partial_resources_before_completing_its_original_destination()
 {
    for fail in [false, true] {
        for fuel in 0..=12 {
            DROPS.with(|n| n.set(0));
            let mut h = host(fuel, fail);
            let text = "recovered constructor owner".repeat(30);
            let buffer = text.as_ptr();
            let result =
                execution::run_source_entry(&mut h, vec![V::String(text)], &RUST_RECOVERING_ROOT);
            let expected = observed(&mut h, result, buffer);
            let drops = DROPS.with(|n| n.get());
            for quota in [1, 3, usize::MAX] {
                for arena in [false, true] {
                    DROPS.with(|n| n.set(0));
                    let pool = physical_frames::Arena::default();
                    let mut frame = Root::default();
                    let invocation = if arena {
                        unsafe { Invocation::in_arena(&pool, &RECOVERING_ROOT) }
                    } else {
                        unsafe { Invocation::new(&RECOVERING_ROOT, &mut frame) }
                    }
                    .unwrap();
                    let mut h = host(fuel, fail);
                    let text = "recovered constructor owner".repeat(30);
                    let buffer = text.as_ptr();
                    let result = unsafe {
                        execution::run_abi_invocation(
                            &mut h,
                            vec![V::String(text)],
                            &RECOVERING_ROOT,
                            invocation,
                            quota,
                        )
                    };
                    assert_eq!(
                        observed(&mut h, result, buffer),
                        expected,
                        "{fail}/{fuel}/{quota}/{arena}"
                    );
                    assert_eq!(DROPS.with(|n| n.get()), drops);
                    assert_eq!(pool.counts().2, 0);
                }
            }
        }
    }
}

unsafe extern "C" fn checked_constructor(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        if (*p.cast::<Constructor>()).pc == 0 {
            assert_eq!(count, 1);
            let value = *args;
            let error = (&mut *c)
                .host_control(
                    &CONSTRUCTOR_CONTROL,
                    HostLeaf {
                        operation: "pause",
                        cancellable: true,
                    },
                    &[value],
                )
                .unwrap_err();
            assert!(error.contains("producer or arity"));
            assert!((&*c).get(value).is_ok());
            assert!((&*c).request.is_none());
        }
        constructor(c, p, args, count)
    }
}
static CHECKED_CONSTRUCTOR: InvocationLayout = InvocationLayout {
    callback: checked_constructor,
    ..CONSTRUCTOR
};
static HOST_ROLE_ROOT: InvocationLayout = InvocationLayout {
    children: &[InlineChild {
        offset: offset_of!(Root, constructor),
        layout: &CHECKED_CONSTRUCTOR,
    }],
    ..ROOT
};
#[test]
fn constructor_cannot_impersonate_a_generated_host_leaf_or_consume_its_captures() {
    let mut h = host(100, false);
    h.manifest["primitives"] = json!([{"name":"pause","async":true,"returns":true,"parameters":[{"name":"text","type":"String","mode":"move"}],"outcomes":[{"name":"done","fields":[]}]}]);
    let mut frame = Root::default();
    let invocation = unsafe { Invocation::new(&HOST_ROLE_ROOT, &mut frame) }.unwrap();
    let text = "host role capture".to_owned();
    let buffer = text.as_ptr();
    let result = unsafe {
        execution::run_abi_invocation(
            &mut h,
            vec![V::String(text)],
            &HOST_ROLE_ROOT,
            invocation,
            1,
        )
    };
    assert_eq!(
        observed(&mut h, result, buffer)["result"]["outcome"],
        "done"
    );
}
