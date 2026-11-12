use super::*;
use execution::{Activation, Control, Step};
use std::cell::RefCell;
use std::mem::{align_of, offset_of, size_of};

thread_local! {
    static DROPS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static HANDLERS: RefCell<Vec<(u32, String)>> = const { RefCell::new(Vec::new()) };
}
struct Probe(u32);
impl Drop for Probe {
    fn drop(&mut self) {
        DROPS.with(|d| d.borrow_mut().push(self.0));
    }
}
fn owner(h: &mut Host, id: u32) -> V {
    h.resources.insert("probe", Probe(id)).unwrap()
}
fn mode(h: &Host) -> u32 {
    h.manifest["mode"].as_u64().unwrap() as u32
}
fn rtick(h: &mut Host, id: usize) -> R<()> {
    h.at(id);
    h.tick(id, 1)
}
fn failure(h: &mut Host, message: &str) -> R<()> {
    h.fault(V::String(message.into()))
}
fn release(h: &mut Host, v: V) {
    h.release_frame(&[Some(v)]);
}
fn handled(h: &mut Host, number: V, text: V, message: V) -> R<()> {
    let V::U32(number) = number else {
        panic!("retained order")
    };
    let V::String(text) = text else {
        panic!("retained text")
    };
    assert_eq!(text, "retained".repeat(50));
    // All ordinary borrowers and local owners retire before emergency work.
    assert!(h.resources.values.is_empty());
    HANDLERS.with(|v| v.borrow_mut().push((number, text)));
    h.emit(message)?;
    if mode(h) == 3 {
        return failure(h, "handler failure");
    }
    if mode(h) == 5 {
        return h.escalate();
    }
    Ok(())
}
#[repr(C)]
#[derive(Default)]
struct Leaf {
    pc: u32,
    host: HostFrame,
}
#[repr(C)]
#[derive(Default)]
struct HostFrame {
    pc: u32,
}
#[repr(C)]
#[derive(Default)]
struct Work {
    pc: u32,
    child: Leaf,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    temporary: u64,
    work: Work,
    handler: Leaf,
}
unsafe fn store(c: *mut Context<'_>, slot: usize, v: u64) {
    unsafe { tellur_rt_v1_store(c, slot, std::ptr::null(), 0, v) }
}
unsafe fn take(c: *mut Context<'_>, slot: usize) -> u64 {
    unsafe { tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 1) }
}
unsafe fn finish_probe(c: *mut Context<'_>, slot: usize) {
    unsafe {
        let value = take(c, slot);
        (&mut *c).perform(|c| {
            let value = c.take(value)?;
            release(c.host, value);
            Ok(())
        });
    }
}
unsafe fn tick(c: *mut Context<'_>, id: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, id, id, 1);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn fault(c: *mut Context<'_>, text: &str) {
    unsafe {
        let value = tellur_rt_v1_value(c, 3, 0, text.as_ptr(), text.len());
        tellur_rt_v1_fault(c, value);
    }
}
unsafe fn discard(c: *mut Context<'_>, args: *const u64, count: usize, set: bool) {
    assert_eq!(count, 1);
    let mut remaining = 0;
    unsafe {
        tellur_rt_v1_route(
            c,
            *args,
            b"done".as_ptr(),
            4,
            if set {
                &mut remaining
            } else {
                std::ptr::null_mut()
            },
            usize::from(set),
        );
        if set {
            store(c, 3, remaining);
        }
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
            assert_eq!(count, 1);
            store(c, 0, *args);
            let text = "unselected parent local".repeat(20);
            f.temporary = text.as_ptr() as u64;
            let handle = (&mut *c).put(V::String(text));
            store(c, 1, handle);
        } else {
            assert_eq!(count, 0); // Settlement does not deliver an outcome handle.
            let c = &*c;
            let V::String(text) = c.storage.values()[c.local(1).unwrap()].as_ref().unwrap() else {
                panic!()
            };
            assert_eq!(text.as_ptr() as u64, f.temporary);
        }
        if !tick(c, f.pc as usize + 1) {
            return 0;
        }
        if f.pc < 2 {
            let text = (&mut *c).put(V::String("retained".repeat(50)));
            let number = (&mut *c).put(V::U32(f.pc));
            tellur_rt_v1_supervise_children(c, 0, 1, [text, number].as_ptr(), 2);
            f.pc += 1;
            return 12;
        }
        tellur_rt_v1_release_local(c, 1);
        let result = take(c, 0);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [result].as_ptr(), 1);
        f.pc = 3;
        2
    }
}
unsafe extern "C" fn work(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Work>();
        let m = mode((&*c).host);
        if f.pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !tick(c, 10) {
                return 0;
            }
            let probe = owner((&mut *c).host, 1);
            let v = (&mut *c).put(probe);
            store(c, 2, v);
            if m == 1 {
                fault(c, "work failure");
                return 0;
            }
            if m == 0 {
                f.pc = 4;
                return 1;
            }
            if m == 9 {
                let v = (&mut *c)
                    .empty_task_set(&LEAF_CONTROL, "$tasks<leaf>", "$started<leaf>")
                    .unwrap();
                store(c, 3, v);
            }
            f.pc = 1;
        }
        if f.pc == 1 || f.pc == 2 {
            let ordinal = f.pc;
            if !tick(c, 10 + ordinal as usize) {
                return 0;
            }
            let loan = tellur_rt_v1_reference(c, 2, std::ptr::null(), 0, 0, b"&probe".as_ptr(), 6);
            let probe = owner((&mut *c).host, 1 + ordinal);
            let value = (&mut *c).put(probe);
            let fail = (&mut *c).put(V::Bool([2, 3, 5, 6, 7, 9, 14].contains(&m) && ordinal == 1));
            if m < 7 || m == 11 || m == 12 {
                tellur_rt_v1_await(c, 0, [loan, value, fail].as_ptr(), 3);
                f.pc = 4;
                return 3;
            }
            if m == 9 {
                let set = take(c, 3);
                (&mut *c).perform(|c| {
                    c.spawn_into_control(
                        &LEAF,
                        "$task<leaf>",
                        "$started<leaf>",
                        "$tasks<leaf>",
                        3,
                        set,
                        &[loan, value, fail],
                    )
                });
            } else {
                (&mut *c).perform(|c| {
                    c.spawn_control(
                        &LEAF,
                        "$task<leaf>",
                        "$started<leaf>",
                        2 + ordinal as usize,
                        &[loan, value, fail],
                    )
                });
            }
            f.pc += 1;
            return if m == 9 { 10 } else { 6 };
        }
        if f.pc == 3 {
            if !tick(c, 13) {
                return 0;
            }
            if m == 10 {
                fault(c, "work failure");
                return 0;
            }
            if m == 8 {
                let child = take(c, 3);
                (&mut *c).perform(|c| c.cancel_control(&LEAF_CONTROL, 3, child));
                f.pc = 5;
                return 8;
            }
            f.pc = 5;
            return 1;
        }
        if f.pc == 5 || f.pc == 6 {
            if !tick(c, f.pc as usize + 10) {
                return 0;
            }
            let value = take(c, (f.pc - 2) as usize);
            if m == 9 {
                (&mut *c).perform(|c| {
                    c.next_control(&LEAF_CONTROL, "$tasks<leaf>", "$started<leaf>", value)
                        .map(|_| ())
                });
                f.pc = 7;
                return 11;
            }
            (&mut *c).perform(|c| c.join_control(&LEAF_CONTROL, value));
            f.pc = if f.pc == 5 { 7 } else { 8 };
            return 7;
        }
        if f.pc == 7 {
            discard(c, args, count, m == 9);
            f.pc = 6;
            return 4;
        }
        if f.pc == 8 || (f.pc == 4 && m != 0) {
            discard(c, args, count, false);
        }
        if !tick(c, 14) {
            return 0;
        }
        finish_probe(c, 2);
        tellur_rt_v1_settled(c);
        f.pc = 9;
        2
    }
}
unsafe extern "C" fn leaf(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 3);
            for i in 0..3 {
                store(c, i, *args.add(i));
            }
            if !tick(c, 20) {
                return 0;
            }
            f.pc = 1;
            if mode((&*c).host) >= 11 {
                tellur_rt_v1_await(c, 0, std::ptr::null(), 0);
                return 3;
            }
            return 1;
        }
        if mode((&*c).host) >= 11 {
            discard(c, args, count, false);
        }
        if !tick(c, 21) {
            return 0;
        }
        if mode((&*c).host) == 4 {
            (&mut *c).perform::<()>(|_| Err("E_HOST: malformed scripted response".into()));
            return 0;
        }
        let fail = take(c, 2);
        if tellur_rt_v1_bool(c, fail) != 0 {
            fault(c, "leaf failure");
            return 0;
        }
        if mode((&*c).host) == 8 {
            let flag = (&*c).host.task_cancelled();
            (&mut *c).host.emit(V::String(flag.to_string())).unwrap();
        }
        tellur_rt_v1_release_local(c, 0);
        finish_probe(c, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn handler(
    c: *mut Context<'_>,
    _p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 3);
        for i in 0..3 {
            store(c, i, *args.add(i));
        }
        if !tick(c, 30) {
            return 0;
        }
        let handles = [take(c, 0), take(c, 1), take(c, 2)];
        (&mut *c).perform(|c| {
            let mut values = c.take_many(&handles)?.into_iter();
            handled(
                c.host,
                values.next().unwrap(),
                values.next().unwrap(),
                values.next().unwrap(),
            )
        });
        tellur_rt_v1_settled(c);
        2
    }
}
macro_rules! control {
    ($id:ident, $name:literal, $slots:expr, $params:expr, $retained:expr, $resume:ident) => {
        static $id: Control = Control {
            name: $name,
            slots: $slots,
            atoms: 0,
            parameters: $params,
            retained: $retained,
            constructor: false,
            access: None,
            resume: $resume,
        };
    };
}
control!(PARENT_CONTROL, "parent", 2, &[0], &[], resume_invocation);
control!(WORK_CONTROL, "work", 5, &[0, 1], &[1, 0], resume_invocation);
control!(LEAF_CONTROL, "leaf", 3, &[0, 1, 2], &[], resume_invocation);
control!(
    HANDLER_CONTROL,
    "handler",
    3,
    &[0, 1, 2],
    &[],
    resume_invocation
);
static LEAF: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &LEAF_CONTROL,
    callback: leaf,
    size: size_of::<Leaf>(),
    align: align_of::<Leaf>(),
    prefix: offset_of!(Leaf, host),
    children: &[InlineChild {
        offset: offset_of!(Leaf, host),
        layout: &HOST,
    }],
};
static HANDLER: InvocationLayout = InvocationLayout {
    control: &HANDLER_CONTROL,
    callback: handler,
    ..LEAF
};
static WORK: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &WORK_CONTROL,
    callback: work,
    size: size_of::<Work>(),
    align: align_of::<Work>(),
    prefix: offset_of!(Work, child),
    children: &[InlineChild {
        offset: offset_of!(Work, child),
        layout: &LEAF,
    }],
};
static PARENT: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &PARENT_CONTROL,
    callback: parent,
    size: size_of::<Parent>(),
    align: align_of::<Parent>(),
    prefix: offset_of!(Parent, work),
    children: &[
        InlineChild {
            offset: offset_of!(Parent, work),
            layout: &WORK,
        },
        InlineChild {
            offset: offset_of!(Parent, handler),
            layout: &HANDLER,
        },
    ],
};

// Independent original-runtime continuations provide the oracle. No ABI lifecycle
// hooks or physical frames are used on this side of the comparison.
fn rust_parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    rtick(h, a.pc + 1)?;
    if a.pc < 2 {
        let round = a.pc as u32;
        a.pc += 1;
        return Ok(Step::Supervise {
            work: &RUST_WORK,
            handler: &RUST_HANDLER,
            arguments: vec![V::String("retained".repeat(50)), V::U32(round)],
        });
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
fn rust_delivery(_: &mut Activation, _: &mut Frames, result: Completion) -> R<()> {
    let (name, fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert!(fields.is_empty());
    Ok(())
}
fn rust_set_delivery(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let (name, mut fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert_eq!(fields.len(), 1);
    v[a.base() + 3] = fields.pop();
    Ok(())
}
fn rust_work(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let base = a.base();
    let m = mode(h);
    if a.pc == 0 {
        rtick(h, 10)?;
        v[base + 2] = Some(owner(h, 1));
        if m == 1 {
            failure(h, "work failure")?;
        }
        if m == 0 {
            a.pc = 4;
            return Ok(Step::Yield);
        }
        if m == 9 {
            v[base + 3] = Some(V::Construct {
                ty: "$tasks<leaf>".into(),
                name: "tasks".into(),
                fields: vec![],
            });
        }
        a.pc = 1;
    }
    if a.pc == 1 || a.pc == 2 {
        let ordinal = a.pc;
        rtick(h, 10 + ordinal)?;
        let mut args = vec![
            V::Reference {
                ty: "&probe".into(),
                root: base + 2,
                fields: vec![],
                evidence: Default::default(),
            },
            owner(h, 1 + ordinal as u32),
            V::Bool([2, 3, 5, 6, 7, 9, 14].contains(&m) && ordinal == 1),
        ];
        if m < 7 || m == 11 || m == 12 {
            a.pc = 4;
            return Ok(Step::Await {
                task: a.queue_invocation(h, &RUST_LEAF, &mut args)?,
                deliver: rust_delivery,
            });
        }
        let mut cold = Some(a.defer_task(h, &RUST_LEAF, "$task<leaf>", &mut args)?);
        let task = a.queue_started(h, &RUST_LEAF, &mut cold)?;
        a.pc += 1;
        return if m == 9 {
            Ok(Step::SpawnInto {
                task,
                set: v[base + 3].take().unwrap(),
                slot: 3,
                ty: "$started<leaf>",
            })
        } else {
            Ok(Step::Spawn {
                task,
                slot: 2 + ordinal,
                ty: "$started<leaf>",
            })
        };
    }
    if a.pc == 3 {
        rtick(h, 13)?;
        if m == 10 {
            failure(h, "work failure")?;
        }
        a.pc = 5;
        if m == 8 {
            a.cancel_tasks(h, &RUST_LEAF, v[base + 3].as_ref().unwrap())?;
            return Ok(Step::Cancel);
        }
        return Ok(Step::Yield);
    }
    if a.pc == 5 || a.pc == 6 {
        rtick(h, a.pc + 10)?;
        let value = v[base + a.pc - 2].take().unwrap();
        if m == 9 {
            a.pc = 7;
            return Ok(Step::Next {
                set: value,
                producer: &RUST_LEAF,
                deliver: rust_set_delivery,
            });
        }
        a.pc = if a.pc == 5 { 7 } else { 8 };
        return Ok(Step::Join {
            value,
            producer: &RUST_LEAF,
            deliver: rust_delivery,
        });
    }
    if a.pc == 7 {
        a.pc = 6;
        return Ok(Step::Continue);
    }
    rtick(h, 14)?;
    release(h, v[base + 2].take().unwrap());
    Ok(Step::Complete(Completion::Settled))
}
fn rust_leaf(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        rtick(h, 20)?;
        a.pc = 1;
        if mode(h) >= 11 {
            return Ok(Step::Await {
                task: a.queue_invocation(h, &RUST_HOST, &mut vec![])?,
                deliver: rust_delivery,
            });
        }
        return Ok(Step::Yield);
    }
    rtick(h, 21)?;
    if mode(h) == 4 {
        return Err("E_HOST: malformed scripted response".into());
    }
    let fail = v[a.base() + 2].take().unwrap();
    if fail == V::Bool(true) {
        failure(h, "leaf failure")?;
    }
    if mode(h) == 8 {
        h.emit(V::String(h.task_cancelled().to_string()))?;
    }
    v[a.base()] = None;
    release(h, v[a.base() + 1].take().unwrap());
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
fn rust_handler(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    rtick(h, 30)?;
    handled(
        h,
        v[a.base()].take().unwrap(),
        v[a.base() + 1].take().unwrap(),
        v[a.base() + 2].take().unwrap(),
    )?;
    Ok(Step::Complete(Completion::Settled))
}
control!(RUST_PARENT, "parent", 1, &[0], &[], rust_parent);
control!(RUST_WORK, "work", 5, &[0, 1], &[1, 0], rust_work);
control!(RUST_LEAF, "leaf", 3, &[0, 1, 2], &[], rust_leaf);
control!(RUST_HANDLER, "handler", 3, &[0, 1, 2], &[], rust_handler);
fn observation(h: &mut Host, result: R<Completion>, buffer: *const u8) -> Json {
    let result = match result {
        Ok(done) => {
            let (name, values) = done.outcome().unwrap();
            let V::String(s) = &values[0] else { panic!() };
            assert_eq!(s.as_ptr(), buffer);
            json!({"outcome": name, "fields": values})
        }
        Err(e) => json!({"fault": h.activations.failure("parent", fault::Phase::Execution, &e)}),
    };
    assert!(
        h.tasks.is_empty() && h.resources.values.is_empty() && h.resources.scopes.is_empty(),
        "mode={} wrapper={} fuel={:?} result={result} resources={} scopes={} tasks_empty={}",
        h.manifest,
        h.manifest["wrapper"],
        h.fuel,
        h.resources.values.len(),
        h.resources.scopes.len(),
        h.tasks.is_empty()
    );
    json!({"result": result, "events": h.events, "fuel": h.fuel, "cursor": h.cursor,
        "drops": DROPS.with(|d| d.borrow().clone()), "handlers": HANDLERS.with(|d| d.borrow().clone())})
}
fn reset() {
    DROPS.with(|d| d.borrow_mut().clear());
    HANDLERS.with(|d| d.borrow_mut().clear());
}
fn exercise(modes: &[u32], wrappers: &[u32]) {
    for &wrapper in wrappers {
        for &mode in modes {
            for fuel in 0..=40 {
                reset();
                let text = "parent buffer 😀".repeat(80);
                let ptr = text.as_ptr();
                let mut h = Host::new(
                    manifest(mode, wrapper, fuel, false),
                    script(mode),
                    true,
                    Some(fuel),
                );
                let result = execution::run_source_entry(
                    &mut h,
                    vec![V::String(text)],
                    if wrapper == 0 {
                        &RUST_PARENT
                    } else {
                        &RUST_WRAPPER
                    },
                );
                let expected = observation(&mut h, result, ptr);
                for quota in [1, 3, usize::MAX] {
                    for arena_owned in [false, true] {
                        reset();
                        let text = "parent buffer 😀".repeat(80);
                        let ptr = text.as_ptr();
                        let mut h = Host::new(
                            manifest(mode, wrapper, fuel, true),
                            script(mode),
                            true,
                            Some(fuel),
                        );
                        let mut frame = Wrapper::default();
                        let layout = if wrapper == 0 { &PARENT } else { &WRAPPER };
                        let arena = physical_frames::Arena::default();
                        let invocation = if arena_owned {
                            unsafe { Invocation::in_arena(&arena, layout) }.unwrap()
                        } else if wrapper == 0 {
                            unsafe { Invocation::new(&PARENT, &mut frame.parent) }.unwrap()
                        } else {
                            unsafe { Invocation::new(&WRAPPER, &mut frame) }.unwrap()
                        };
                        let storage = invocation.arena.clone();
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
                            observation(&mut h, result, ptr),
                            expected,
                            "mode={mode}/wrapper={wrapper}/fuel={fuel}/quota={quota}/arena={arena_owned}"
                        );
                        assert_eq!(storage.counts().2, 0);
                        assert_eq!(arena.counts().2, 0);
                    }
                }
            }
        }
    }
}
#[test]
fn named_supervision_matches_original_work_handler_fuel_and_owned_retirement() {
    exercise(&[0, 1, 2, 3, 4, 5, 6], &[0]);
}
#[test]
fn named_supervision_settles_spawned_cancelled_and_set_members_before_handover() {
    exercise(&[7, 8, 9, 10], &[0]);
}

#[test]
fn supervision_preflight_preserves_handles_and_rejects_bad_child_roles() {
    let mut host = Host::new(json!({}), None, true, None);
    let mut frames = Frames::default();
    let mut physical = Parent::default();
    let mut invocation = unsafe { Invocation::new(&PARENT, &mut physical) }.unwrap();
    let mut c = invocation
        .state
        .take()
        .unwrap()
        .select(&mut host, &mut frames);
    c.program = Some(invocation.program);
    let one = c.put(V::String("one".into()));
    let two = c.put(V::U32(2));
    for (work, handler, args) in [
        (0, 0, vec![one, two]),
        (1, 0, vec![one, two]),
        (99, 1, vec![one, two]),
        (0, 99, vec![one, two]),
        (0, 1, vec![one]),
        (0, 1, vec![one, one]),
        (0, 1, vec![one, u64::MAX]),
    ] {
        unsafe {
            tellur_rt_v1_supervise_children(&mut c, work, handler, args.as_ptr(), args.len());
        }
        assert!(c.error.take().is_some());
        assert!(c.get(one).is_ok() && c.get(two).is_ok() && c.request.is_none());
        assert!(c.host.resources.scopes.is_empty() && c.host.tasks.is_empty());
    }
    unsafe {
        tellur_rt_v1_supervise_children(&mut c, 0, 1, [one, two].as_ptr(), 2);
    }
    assert!(c.error.is_none() && c.get(one).is_err() && c.get(two).is_err());
    assert!(
        matches!(&c.request, Some(RequestedInvocation::Supervise { arguments, .. }) if arguments.len() == 2)
    );
    let untouched = c.put(V::String("still owned".into()));
    unsafe {
        tellur_rt_v1_supervise_children(&mut c, 0, 1, [untouched, untouched].as_ptr(), 2);
    }
    assert!(
        c.error
            .take()
            .unwrap()
            .contains("duplicate ABI continuation")
    );
    assert!(c.get(untouched).is_ok());
    assert!(c.host.resources.scopes.is_empty() && c.host.tasks.is_empty());
}

#[test]
fn supervision_administrative_hooks_validate_identity_phase_and_pin_lifetimes() {
    let arena = physical_frames::Arena::default();
    let mut parent = unsafe { Invocation::in_arena(&arena, &PARENT) }.unwrap();
    let work = parent.program.child(0).unwrap();
    let handler = parent.program.child(1).unwrap();
    parent.supervision = Some(InvocationSupervision {
        work,
        handler,
        phase: SupervisionPhase::Prepared,
    });
    assert!(parent.take_supervised(&WORK_CONTROL, false).is_err());
    parent.phase = InvocationPhase::Awaiting;
    assert!(parent.settle_supervision().is_err());
    assert!(parent.take_supervised(&HANDLER_CONTROL, true).is_err());
    assert!(parent.take_supervised(&HANDLER_CONTROL, false).is_err());
    assert_eq!(arena.counts().2, 1);
    let work = parent.take_supervised(&WORK_CONTROL, false).unwrap();
    assert!(work.settlement);
    assert_eq!(arena.counts().2, 2);
    assert!(parent.take_supervised(&WORK_CONTROL, false).is_err());
    drop(work);
    let handler = parent.take_supervised(&HANDLER_CONTROL, true).unwrap();
    assert!(handler.settlement);
    assert_eq!(arena.counts().2, 2);
    assert!(parent.take_supervised(&HANDLER_CONTROL, true).is_err());
    drop(handler);
    let before = parent.state.as_ref().unwrap().outcomes.len();
    parent.settle_supervision().unwrap();
    assert_eq!(parent.phase, InvocationPhase::Active);
    assert!(parent.delivered.is_empty());
    assert_eq!(parent.state.as_ref().unwrap().outcomes.len(), before);
    assert!(parent.settle_supervision().is_err());
    drop(parent);
    assert_eq!(arena.counts().2, 0);
}

#[repr(C)]
#[derive(Default)]
struct Wrapper {
    pc: u32,
    parent: Parent,
}
unsafe extern "C" fn wrapper(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Wrapper>();
        let form = (&*c).host.manifest["wrapper"].as_u64().unwrap();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 40) {
                return 0;
            }
            let value = take(c, 0);
            if form >= 3 {
                (&mut *c).perform(|c| {
                    c.spawn_control(&PARENT, "$task<parent>", "$started<parent>", 0, &[value])
                });
                f.pc = 1;
                return 6;
            }
            f.pc = 3;
            if form == 1 {
                tellur_rt_v1_run_child(c, 0, [value].as_ptr(), 1);
                return 5;
            }
            tellur_rt_v1_await(c, 0, [value].as_ptr(), 1);
            return 3;
        }
        if f.pc == 1 || f.pc == 2 {
            if !tick(c, 40 + f.pc as usize) {
                return 0;
            }
            let value = take(c, 0);
            if f.pc == 1 && form == 4 {
                (&mut *c).perform(|c| c.cancel_control(&PARENT_CONTROL, 0, value));
                f.pc = 2;
                return 8;
            }
            (&mut *c).perform(|c| c.join_control(&PARENT_CONTROL, value));
            f.pc = 3;
            return 7;
        }
        assert_eq!((f.pc, count), (3, 1));
        let mut field = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut field, 1);
        store(c, 0, field);
        if !tick(c, 43) {
            return 0;
        }
        let value = take(c, 0);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        2
    }
}
control!(WRAPPER_CONTROL, "wrapper", 1, &[0], &[], resume_invocation);
static WRAPPER: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &WRAPPER_CONTROL,
    callback: wrapper,
    size: size_of::<Wrapper>(),
    align: align_of::<Wrapper>(),
    prefix: offset_of!(Wrapper, parent),
    children: &[InlineChild {
        offset: offset_of!(Wrapper, parent),
        layout: &PARENT,
    }],
};
fn rust_wrapper_delivery(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let (name, mut fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert_eq!(fields.len(), 1);
    v[a.base()] = fields.pop();
    Ok(())
}
fn rust_wrapper(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let form = h.manifest["wrapper"].as_u64().unwrap();
    if a.pc == 0 {
        rtick(h, 40)?;
        let mut args = vec![v[a.base()].take().unwrap()];
        if form >= 3 {
            let mut cold = Some(a.defer_task(h, &RUST_PARENT, "$task<parent>", &mut args)?);
            let task = a.queue_started(h, &RUST_PARENT, &mut cold)?;
            a.pc = 1;
            return Ok(Step::Spawn {
                task,
                slot: 0,
                ty: "$started<parent>",
            });
        }
        a.pc = 3;
        if form == 1 {
            return Ok(Step::Call {
                control: &RUST_PARENT,
                arguments: args,
                destination: None,
                deliver: rust_wrapper_delivery,
            });
        }
        return Ok(Step::Await {
            task: a.queue_invocation(h, &RUST_PARENT, &mut args)?,
            deliver: rust_wrapper_delivery,
        });
    }
    if a.pc == 1 || a.pc == 2 {
        rtick(h, 40 + a.pc)?;
        if a.pc == 1 && form == 4 {
            a.cancel_tasks(h, &RUST_PARENT, v[a.base()].as_ref().unwrap())?;
            a.pc = 2;
            return Ok(Step::Cancel);
        }
        a.pc = 3;
        return Ok(Step::Join {
            value: v[a.base()].take().unwrap(),
            producer: &RUST_PARENT,
            deliver: rust_wrapper_delivery,
        });
    }
    rtick(h, 43)?;
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
control!(RUST_WRAPPER, "wrapper", 1, &[0], &[], rust_wrapper);
#[test]
fn supervision_survives_inline_awaited_spawned_and_cancelled_ancestors() {
    exercise(&[0, 2, 3, 4, 7, 8, 9], &[1, 2, 3, 4]);
}

fn manifest(mode: u32, wrapper: u32, fuel: usize, abi: bool) -> Json {
    json!({"mode":mode,"wrapper":wrapper,"initial_fuel":fuel,"abi":abi,
        "primitives":[{"name":"tick","async":true,"returns":true,"parameters":[],
            "outcomes":[{"name":"done","fields":[]}]}]})
}
fn script(mode: u32) -> Option<Vec<ScriptCall>> {
    (mode >= 11).then(|| {
        (0..4)
            .map(|_| ScriptCall {
                operation: "tick".into(),
                outcome: if mode == 12 { "invalid" } else { "done" }.into(),
                fields: vec![],
                waits: 3,
            })
            .collect()
    })
}
unsafe extern "C" fn host_leaf(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<HostFrame>();
        if f.pc == 0 {
            assert_eq!(count, 0);
            (&mut *c).perform(|c| {
                c.host_control(
                    &HOST_CONTROL,
                    HostLeaf {
                        operation: "tick",
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
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        2
    }
}
control!(HOST_CONTROL, "host", 0, &[], &[], resume_invocation);
static HOST: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &HOST_CONTROL,
    callback: host_leaf,
    size: size_of::<HostFrame>(),
    align: align_of::<HostFrame>(),
    prefix: size_of::<HostFrame>(),
    children: &[],
};
fn rust_host_delivery(
    _: &mut Activation,
    _: &mut Frames,
    outcome: String,
    fields: Vec<V>,
) -> R<()> {
    assert!(outcome == "done" || outcome == "cancelled");
    assert!(fields.is_empty());
    Ok(())
}
fn rust_host(_: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Host {
            operation: "tick",
            arguments: vec![],
            deliver: rust_host_delivery,
            cancellable: true,
        });
    }
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
control!(RUST_HOST, "host", 0, &[], &[], rust_host);
#[test]
fn supervision_preserves_host_waits_cancellation_and_non_source_failure_authority() {
    exercise(&[11, 12, 13, 14], &[0, 1, 2, 3, 4]);
}

unsafe extern "C" fn wrong_completion(
    c: *mut Context<'_>,
    _: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        if count == 0 {
            tellur_rt_v1_settled(c);
        } else {
            for i in 0..count {
                store(c, i, *args.add(i));
            }
            tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        }
        2
    }
}
#[test]
fn settlement_is_only_admitted_for_named_work_and_handler_invocations() {
    static WRONG_WORK: InvocationLayout = InvocationLayout {
        callback: wrong_completion,
        ..WORK
    };
    static WRONG_PARENT: InvocationLayout = InvocationLayout {
        children: &[
            InlineChild {
                offset: offset_of!(Parent, work),
                layout: &WRONG_WORK,
            },
            InlineChild {
                offset: offset_of!(Parent, handler),
                layout: &HANDLER,
            },
        ],
        ..PARENT
    };
    static ORDINARY: InvocationLayout = InvocationLayout {
        callback: wrong_completion,
        ..HOST
    };
    for named in [false, true] {
        reset();
        let mut h = Host::new(manifest(0, 0, 40, true), None, true, Some(40));
        let arena = physical_frames::Arena::default();
        let layout = if named { &WRONG_PARENT } else { &ORDINARY };
        let invocation = unsafe { Invocation::in_arena(&arena, layout) }.unwrap();
        let args = if named {
            vec![V::String("caller".into())]
        } else {
            vec![]
        };
        let result = unsafe { execution::run_abi_invocation(&mut h, args, layout, invocation, 1) };
        assert!(result.err().unwrap().contains("completion kind differs"));
        assert!(h.tasks.is_empty() && h.resources.scopes.is_empty());
        assert!(HANDLERS.with(|v| v.borrow().is_empty()));
        assert_eq!(arena.counts().2, 0);
    }
}
