use super::*;
use execution::{Activation, BoundaryControl, Control, LoopControl, Step};
use std::cell::RefCell;
use std::mem::{align_of, offset_of, size_of};

thread_local! { static DROPS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) }; }
struct Probe(u32);
impl Drop for Probe {
    fn drop(&mut self) {
        DROPS.with(|drops| drops.borrow_mut().push(self.0));
    }
}
fn mode(h: &Host) -> u32 {
    h.manifest["mode"].as_u64().unwrap() as u32
}
fn tick(h: &mut Host, pc: usize) -> R<()> {
    h.at(pc);
    h.tick(pc, 1)
}
fn boolean(v: &V) -> bool {
    let V::Bool(value) = v else {
        panic!("not bool")
    };
    *value
}
fn reference(root: usize) -> V {
    V::Reference {
        ty: "&probe".into(),
        root,
        fields: vec![],
        evidence: Default::default(),
    }
}
fn probe(h: &mut Host) -> V {
    h.resources.insert("probe", Probe(1)).unwrap()
}
fn consume(h: &mut Host, v: V) -> R<()> {
    h.resources.take::<Probe>(&v, "probe").map(drop)
}
fn reset(a: &mut Activation, v: &mut Frames, args: Vec<V>) -> R<()> {
    assert_eq!(args.len(), 3);
    for slot in LOOP.locals {
        v[a.base() + slot] = None;
    }
    a.facts[0] = false;
    for (slot, value) in [2, 3, 4].into_iter().zip(args) {
        v[a.base() + slot] = Some(value);
    }
    Ok(())
}
static LOOP: LoopControl = LoopControl {
    target: 19,
    body: 1,
    after: 6,
    locals: &[2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    reset,
};
static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "parent$boundary@12",
    body: 2,
    handler: 5,
    after: 4,
    retained: &[6],
    retire: &[5, 7, 9],
    work: &[5, 7, 9],
    handling: &[8],
    fault_slot: 8,
};
static INNER: BoundaryControl = BoundaryControl {
    name: "parent$boundary@13",
    body: 7,
    handler: 9,
    after: 8,
    retained: &[6],
    retire: &[5, 7, 9],
    work: &[5, 7, 9],
    handling: &[11],
    fault_slot: 11,
};
fn delivered(_: &mut Activation, _: &mut Frames, result: Completion) -> R<()> {
    let (name, fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert!(fields.is_empty());
    Ok(())
}
fn rust_child(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, 20)?;
        a.pc = 1;
        return Ok(Step::Yield);
    }
    if a.pc == 1 && (10..=11).contains(&mode(h)) {
        a.pc = 2;
        return Ok(Step::Await {
            task: a.queue_invocation(h, &RUST_HOST, &mut vec![])?,
            deliver: delivered,
        });
    }
    tick(h, 21)?;
    if mode(h) == 4 {
        return Err("E_HOST: invalid driver reply".into());
    }
    if mode(h) == 2 && boolean(v[a.base() + 1].as_ref().unwrap()) {
        h.fault(V::String("child failed".into()))?;
    }
    v[a.base()] = None;
    v[a.base() + 1] = None;
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
fn rust_parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let b = a.base();
    let m = mode(h);
    Ok(match a.pc {
        0 => {
            tick(h, 10)?;
            Step::Loop {
                control: &LOOP,
                arguments: vec![
                    V::Bool(true),
                    v[b].take().unwrap(),
                    v[b + 1].take().unwrap(),
                ],
            }
        }
        1 => {
            tick(h, 11)?;
            assert!(!a.facts[0]);
            a.facts[0] = true;
            let first = boolean(v[b + 2].as_ref().unwrap());
            if m == 0 {
                if first {
                    return Ok(Step::Complete(Completion::Again(
                        19,
                        vec![
                            V::Bool(false),
                            v[b + 4].take().unwrap(),
                            v[b + 3].take().unwrap(),
                        ],
                    )));
                }
                return Ok(Step::Complete(Completion::Complete(19)));
            }
            v[b + 2] = Some(V::Bool(false));
            v[b + 10] = Some(V::Bool(false));
            v[b + 5] = Some(V::Bool(first));
            v[b + 6] = Some(V::String("request context".into()));
            v[b + 7] = Some(probe(h));
            Step::Boundary(&BOUNDARY)
        }
        2 | 7 => {
            if a.pc == 2 && [8, 9].contains(&m) {
                tick(h, 12)?;
                return Ok(Step::Boundary(&INNER));
            }
            tick(h, if a.pc == 7 { 17 } else { 12 })?;
            if [12, 13].contains(&m) && boolean(v[b + 5].as_ref().unwrap()) {
                let mut args = vec![reference(b + 7), v[b + 5].as_ref().unwrap().clone()];
                let mut cold = Some(a.defer_task(h, &RUST_CHILD, "$task<child>", &mut args)?);
                let task = a.queue_started(h, &RUST_CHILD, &mut cold)?;
                a.pc = 10;
                return Ok(if m == 12 {
                    Step::Spawn {
                        task,
                        slot: 9,
                        ty: "$started<child>",
                    }
                } else {
                    Step::SpawnInto {
                        task,
                        set: V::Construct {
                            ty: "$tasks<child>".into(),
                            name: "tasks".into(),
                            fields: vec![],
                        },
                        slot: 9,
                        ty: "$started<child>",
                    }
                });
            }
            a.pc = 3;
            Step::Await {
                task: a.queue_invocation(
                    h,
                    &RUST_CHILD,
                    &mut vec![reference(b + 7), v[b + 5].as_ref().unwrap().clone()],
                )?,
                deliver: delivered,
            }
        }
        3 => {
            tick(h, 13)?;
            let first = boolean(v[b + 5].as_ref().unwrap());
            if first && ![5, 6].contains(&m) {
                h.fault(V::String("work failed".into()))?;
            }
            consume(h, v[b + 7].take().unwrap())?;
            if m == 5 {
                return Ok(Step::Complete(Completion::Complete(19)));
            }
            if m == 6 && first {
                return Ok(Step::Complete(Completion::Again(
                    19,
                    vec![
                        V::Bool(false),
                        v[b + 4].take().unwrap(),
                        v[b + 3].take().unwrap(),
                    ],
                )));
            }
            Step::BoundaryEnd(if [8, 9].contains(&m) {
                &INNER
            } else {
                &BOUNDARY
            })
        }
        10 => {
            a.pc = 11;
            Step::Yield
        }
        11 => {
            tick(h, 19)?;
            h.fault(V::String("parent with borrowing sibling".into()))?;
            unreachable!()
        }
        8 => Step::BoundaryEnd(&BOUNDARY),
        4 => {
            tick(h, 14)?;
            Step::Complete(Completion::Complete(19))
        }
        5 | 9 => {
            tick(h, if a.pc == 9 { 18 } else { 15 })?;
            if a.pc == 9 && m == 9 {
                h.fault(V::String("inner handler failed".into()))?;
            }

            assert!(h.resources.values.is_empty());
            h.emit(v[b + 6].take().unwrap())?;
            if m == 3 {
                h.fault(V::String("handler failed".into()))?;
            }
            if m == 7 {
                return Ok(Step::Complete(Completion::Process(23)));
            }
            Step::Complete(Completion::Recover(
                19,
                vec![
                    v[b + 10].take().unwrap(),
                    v[b + 3].take().unwrap(),
                    v[b + 4].take().unwrap(),
                ],
            ))
        }
        6 => {
            tick(h, 16)?;
            Step::Complete(Completion::Outcome((
                "done".into(),
                vec![v[b + 3].take().unwrap(), v[b + 4].take().unwrap()],
            )))
        }
        _ => unreachable!(),
    })
}
#[repr(C)]
#[derive(Default)]
struct Child {
    pc: u32,
    host: HostFrame,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    fact: bool,
    child: Child,
}
unsafe extern "C" fn reset_facts(p: *mut u8) {
    unsafe {
        (*p.cast::<Parent>()).fact = false;
    }
}
static SCOPES: ScopeLayout = ScopeLayout {
    states: 12,
    boundaries: &[&BOUNDARY, &INNER],
    loops: &[LoopLayout {
        control: &LOOP,
        parameters: &[2, 3, 4],
        reset_facts,
    }],
};
unsafe fn put(c: *mut Context<'_>, value: V) -> u64 {
    unsafe { (&mut *c).perform(|c| Ok(c.put(value))) }
}
unsafe fn take(c: *mut Context<'_>, slot: usize) -> u64 {
    unsafe { tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 1) }
}
unsafe fn store(c: *mut Context<'_>, slot: usize, v: u64) {
    unsafe { tellur_rt_v1_store(c, slot, std::ptr::null(), 0, v) }
}
unsafe fn at(c: *mut Context<'_>, pc: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, pc, pc, 1);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn bool_slot(c: *mut Context<'_>, slot: usize) -> bool {
    unsafe {
        let c = &*c;
        boolean(c.storage.values()[c.local(slot).unwrap()].as_ref().unwrap())
    }
}
unsafe fn transfer(c: *mut Context<'_>, kind: u32, args: &[u64]) -> u32 {
    unsafe {
        tellur_rt_v1_loop(c, 0, kind, args.as_ptr(), args.len());
    }
    15 + kind
}
unsafe extern "C" fn child(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Child>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !at(c, 20) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        if f.pc == 1 && (10..=11).contains(&mode((&*c).host)) {
            f.pc = 2;
            tellur_rt_v1_await(c, 0, std::ptr::null(), 0);
            return 3;
        }
        if f.pc == 2 {
            assert_eq!(count, 1);
            tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, std::ptr::null_mut(), 0);
        }
        if !at(c, 21) {
            return 0;
        }
        let m = mode((&*c).host);
        if m == 4 {
            (&mut *c).perform::<()>(|_| Err("E_HOST: invalid driver reply".into()));
            return 0;
        }
        if m == 2 && bool_slot(c, 1) {
            let message = put(c, V::String("child failed".into()));
            tellur_rt_v1_fault(c, message);
            return 0;
        }
        tellur_rt_v1_release_local(c, 0);
        tellur_rt_v1_release_local(c, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
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
        f.pc = tellur_rt_v1_resume_state(c, f.pc);
        if tellur_rt_v1_ok(c) == 0 {
            return 0;
        }
        let m = mode((&*c).host);
        match f.pc {
            0 => {
                assert_eq!(count, 2);
                store(c, 0, *args);
                store(c, 1, *args.add(1));
                if !at(c, 10) {
                    return 0;
                }
                let args = [put(c, V::Bool(true)), take(c, 0), take(c, 1)];
                transfer(c, 0, &args)
            }
            1 => {
                if !at(c, 11) {
                    return 0;
                }
                assert!(!f.fact);
                f.fact = true;
                let first = bool_slot(c, 2);
                if m == 0 {
                    if first {
                        let args = [put(c, V::Bool(false)), take(c, 4), take(c, 3)];
                        return transfer(c, 1, &args);
                    }
                    return transfer(c, 3, &[]);
                }
                store(c, 2, put(c, V::Bool(false)));
                store(c, 10, put(c, V::Bool(false)));
                store(c, 5, put(c, V::Bool(first)));
                store(c, 6, put(c, V::String("request context".into())));
                let resource = (&mut *c).perform(|c| {
                    let v = probe(c.host);
                    Ok(c.put(v))
                });
                store(c, 7, resource);
                tellur_rt_v1_scope(c, 0, 0);
                13
            }
            2 | 7 => {
                if f.pc == 2 && [8, 9].contains(&m) {
                    if !at(c, 12) {
                        return 0;
                    }
                    tellur_rt_v1_scope(c, 1, 0);
                    return 13;
                }
                if !at(c, if f.pc == 7 { 17 } else { 12 }) {
                    return 0;
                }
                let borrowed = (&mut *c).perform(|c| Ok(c.put(reference(c.local(7)?))));
                let args = [borrowed, put(c, V::Bool(bool_slot(c, 5)))];
                if [12, 13].contains(&m) && bool_slot(c, 5) {
                    f.pc = 10;
                    if m == 12 {
                        (&mut *c).perform(|c| {
                            c.spawn_control(&CHILD, "$task<child>", "$started<child>", 9, &args)
                        });
                        return 6;
                    }
                    let set = (&mut *c).perform(|c| {
                        c.empty_task_set(&ABI_CHILD, "$tasks<child>", "$started<child>")
                    });
                    (&mut *c).perform(|c| {
                        c.spawn_into_control(
                            &CHILD,
                            "$task<child>",
                            "$started<child>",
                            "$tasks<child>",
                            9,
                            set,
                            &args,
                        )
                    });
                    return 10;
                }
                f.child = Child::default();
                f.pc = 3;
                tellur_rt_v1_await(c, 0, args.as_ptr(), args.len());
                3
            }
            3 => {
                assert_eq!(count, 1);
                tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, std::ptr::null_mut(), 0);
                if !at(c, 13) {
                    return 0;
                }
                let first = bool_slot(c, 5);
                if first && ![5, 6].contains(&m) {
                    let v = put(c, V::String("work failed".into()));
                    tellur_rt_v1_fault(c, v);
                    return 0;
                }
                let v = take(c, 7);
                (&mut *c).perform(|c| {
                    let v = c.take(v)?;
                    consume(c.host, v)
                });
                if m == 5 {
                    return transfer(c, 3, &[]);
                }
                if m == 6 && first {
                    let args = [put(c, V::Bool(false)), take(c, 4), take(c, 3)];
                    return transfer(c, 1, &args);
                }
                tellur_rt_v1_scope(c, if [8, 9].contains(&m) { 1 } else { 0 }, 1);
                14
            }
            10 => {
                f.pc = 11;
                1
            }
            11 => {
                if !at(c, 19) {
                    return 0;
                }
                let v = put(c, V::String("parent with borrowing sibling".into()));
                tellur_rt_v1_fault(c, v);
                0
            }
            8 => {
                tellur_rt_v1_scope(c, 0, 1);
                14
            }
            4 => {
                if !at(c, 14) {
                    return 0;
                }
                transfer(c, 3, &[])
            }
            5 | 9 => {
                if !at(c, if f.pc == 9 { 18 } else { 15 }) {
                    return 0;
                }
                if f.pc == 9 && m == 9 {
                    let v = put(c, V::String("inner handler failed".into()));
                    tellur_rt_v1_fault(c, v);
                    return 0;
                }
                assert!((&*c).host.resources.values.is_empty());
                let v = take(c, 6);
                tellur_rt_v1_emit(c, v);
                if m == 3 {
                    let v = put(c, V::String("handler failed".into()));
                    tellur_rt_v1_fault(c, v);
                    return 0;
                }
                if m == 7 {
                    tellur_rt_v1_process(c, 23);
                    return 2;
                }
                let args = [take(c, 10), take(c, 3), take(c, 4)];
                transfer(c, 2, &args)
            }
            6 => {
                if !at(c, 16) {
                    return 0;
                }
                let args = [take(c, 3), take(c, 4)];
                tellur_rt_v1_complete(c, b"done".as_ptr(), 4, args.as_ptr(), 2);
                2
            }
            _ => unreachable!(),
        }
    }
}
static RUST_PARENT: Control = Control {
    name: "parent",
    slots: 12,
    atoms: 1,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: rust_parent,
};
static RUST_CHILD: Control = Control {
    name: "child",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    resume: rust_child,
    ..RUST_PARENT
};
static ABI_PARENT: Control = Control {
    resume: resume_invocation,
    ..RUST_PARENT
};
static ABI_CHILD: Control = Control {
    resume: resume_invocation,
    ..RUST_CHILD
};
static CHILD: InvocationLayout = InvocationLayout {
    control: &ABI_CHILD,
    callback: child,
    size: size_of::<Child>(),
    align: align_of::<Child>(),
    prefix: offset_of!(Child, host),
    children: &[InlineChild {
        offset: offset_of!(Child, host),
        layout: &HOST,
    }],
    scopes: None,
};
static PARENT: InvocationLayout = InvocationLayout {
    control: &ABI_PARENT,
    callback: parent,
    size: size_of::<Parent>(),
    align: align_of::<Parent>(),
    prefix: offset_of!(Parent, child),
    children: &[InlineChild {
        offset: offset_of!(Parent, child),
        layout: &CHILD,
    }],
    scopes: Some(&SCOPES),
};
fn arguments() -> (Vec<V>, [*const u8; 2]) {
    let a = "first owned buffer".repeat(90);
    let b = "second owned buffer".repeat(120);
    let pointers = [a.as_ptr(), b.as_ptr()];
    (vec![V::String(a), V::String(b)], pointers)
}
fn observation(h: &mut Host, result: R<Completion>, pointers: [*const u8; 2]) -> Json {
    let result = match result {
        Ok(Completion::Outcome((name, fields))) => {
            let expected = if [0, 6].contains(&mode(h)) {
                [pointers[1], pointers[0]]
            } else {
                pointers
            };
            for (v, p) in fields.iter().zip(expected) {
                let V::String(text) = v else { panic!() };
                assert_eq!(text.as_ptr(), p);
            }
            json!({"outcome":name,"fields":fields})
        }
        Ok(Completion::Process(status)) => json!({"process":status}),
        Ok(_) => panic!("unexpected completion"),
        Err(error) => {
            json!({"fault":h.activations.failure("parent",fault::Phase::Execution,&error)})
        }
    };
    assert!(h.resources.values.is_empty() && h.resources.scopes.is_empty() && h.tasks.is_empty());
    json!({"result":result,"events":h.events,"fuel":h.fuel,"drops":DROPS.with(|v|v.borrow().clone())})
}
fn exercise(modes: &[u32]) {
    exercise_wrapped(modes, &[0]);
}
fn exercise_wrapped(modes: &[u32], forms: &[u32]) {
    exercise_with_layouts(modes, forms, &PARENT, &WRAPPER);
}
fn exercise_with_layouts(
    modes: &[u32],
    forms: &[u32],
    parent_layout: &'static InvocationLayout,
    wrapper_layout: &'static InvocationLayout,
) {
    for &form in forms {
        for &mode in modes {
            for fuel in 0..=35 {
                DROPS.with(|v| v.borrow_mut().clear());
                let (args, pointers) = arguments();
                let mut h = Host::new(manifest(mode, form), script(mode), true, Some(fuel));
                let result = execution::run_source_entry(
                    &mut h,
                    args,
                    if form == 0 {
                        &RUST_PARENT
                    } else {
                        &RUST_WRAPPER
                    },
                );
                let expected = observation(&mut h, result, pointers);
                for quota in [1, 3, usize::MAX] {
                    for arena in [false, true] {
                        DROPS.with(|v| v.borrow_mut().clear());
                        let (args, pointers) = arguments();
                        let mut h = Host::new(manifest(mode, form), script(mode), true, Some(fuel));
                        let mut frame = Wrapper::default();
                        let layout = if form == 0 {
                            parent_layout
                        } else {
                            wrapper_layout
                        };
                        let pool = physical_frames::Arena::default();
                        let invocation = if arena {
                            unsafe { Invocation::in_arena(&pool, layout) }
                        } else if form == 0 {
                            unsafe { Invocation::new(parent_layout, &mut frame.parent) }
                        } else {
                            unsafe { Invocation::new(wrapper_layout, &mut frame) }
                        }
                        .unwrap();
                        let storage = invocation.arena.clone();
                        let result = unsafe {
                            execution::run_abi_invocation(&mut h, args, layout, invocation, quota)
                        };
                        assert_eq!(storage.counts().2, 0);
                        assert_eq!(pool.counts().2, 0);
                        assert_eq!(
                            observation(&mut h, result, pointers),
                            expected,
                            "mode={mode} form={form} fuel={fuel} quota={quota} arena={arena}"
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn retained_loop_swaps_preserve_buffers_and_clear_physical_facts() {
    exercise(&[0]);
}
#[test]
fn recovery_preserves_boundary_entry_parameters_and_retires_borrowing_children() {
    exercise(&[1, 2]);
}
#[test]
fn loop_transfers_close_work_scopes_before_reentry_or_expanded_completion() {
    exercise(&[5, 6]);
}
#[test]
fn recovery_bypasses_driver_errors_and_never_reenters_failed_handlers() {
    exercise(&[3, 4, 7]);
}

#[test]
fn nested_contingency_recovery_closes_intervening_scopes_and_preserves_secondary_causes() {
    exercise(&[8, 9]);
}

#[test]
fn loop_metadata_rejects_aliased_targets_states_and_invalid_slot_sets() {
    static SAME_TARGET: LoopControl = LoopControl { body: 2, ..LOOP };
    static BAD_STATE: LoopControl = LoopControl { after: 12, ..LOOP };
    static SAME_STATE: LoopControl = LoopControl { after: 1, ..LOOP };
    static BAD_LOCAL: LoopControl = LoopControl {
        locals: &[2, 3, 4, 12],
        ..LOOP
    };
    static DUP_LOCAL: LoopControl = LoopControl {
        locals: &[2, 3, 4, 4],
        ..LOOP
    };
    static CASES: [&[LoopLayout]; 8] = [
        &[
            LoopLayout {
                control: &LOOP,
                parameters: &[2, 3, 4],
                reset_facts,
            },
            LoopLayout {
                control: &LOOP,
                parameters: &[2, 3, 4],
                reset_facts,
            },
        ],
        &[
            LoopLayout {
                control: &LOOP,
                parameters: &[2, 3, 4],
                reset_facts,
            },
            LoopLayout {
                control: &SAME_TARGET,
                parameters: &[2, 3, 4],
                reset_facts,
            },
        ],
        &[LoopLayout {
            control: &BAD_STATE,
            parameters: &[2, 3, 4],
            reset_facts,
        }],
        &[LoopLayout {
            control: &SAME_STATE,
            parameters: &[2, 3, 4],
            reset_facts,
        }],
        &[LoopLayout {
            control: &BAD_LOCAL,
            parameters: &[2, 3, 4],
            reset_facts,
        }],
        &[LoopLayout {
            control: &DUP_LOCAL,
            parameters: &[2, 3, 4],
            reset_facts,
        }],
        &[LoopLayout {
            control: &LOOP,
            parameters: &[2, 2, 4],
            reset_facts,
        }],
        &[LoopLayout {
            control: &LOOP,
            parameters: &[0, 3, 4],
            reset_facts,
        }],
    ];
    for loops in CASES {
        let scopes = Box::leak(Box::new(ScopeLayout { loops, ..SCOPES }));
        let layout = InvocationLayout {
            scopes: Some(scopes),
            ..PARENT
        };
        assert!(layout.validate().is_err());
    }
    assert!(PARENT.validate().is_ok());
}
thread_local! { static REJECTIONS: RefCell<usize> = const {RefCell::new(0)}; }
unsafe fn rejected(c: *mut Context<'_>, index: usize, kind: u32, args: &[u64]) {
    unsafe {
        let pending = (&*c).request.as_ref().map(RequestedInvocation::tag);
        let values = serde_json::to_value(&(&*c).values).unwrap();
        let events = (&*c).host.events.len();
        let scopes = (&*c).host.resources.scopes.len();
        tellur_rt_v1_loop(c, index, kind, args.as_ptr(), args.len());
        assert!((&*c).error.is_some());
        tellur_rt_v1_emit(c, u64::MAX);
        assert_eq!(serde_json::to_value(&(&*c).values).unwrap(), values);
        assert_eq!(
            (&*c).request.as_ref().map(RequestedInvocation::tag),
            pending
        );
        assert_eq!((&*c).host.events.len(), events);
        assert_eq!((&*c).host.resources.scopes.len(), scopes);
        (&mut *c).error = None;
        REJECTIONS.with(|n| *n.borrow_mut() += 1);
    }
}
unsafe extern "C" fn checked_parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let pc = tellur_rt_v1_resume_state(c, (*p.cast::<Parent>()).pc);
        rejected(c, 99, 0, &[]);
        rejected(c, 0, 4, &[]);
        if pc == 0 {
            rejected(c, 0, 3, &[]);
        }
        if [1, 5, 6].contains(&pc) {
            let values = [
                put(c, V::Bool(false)),
                put(c, V::String("a".into())),
                put(c, V::String("b".into())),
            ];
            rejected(c, 0, if pc == 6 { 1 } else { 0 }, &values);
            rejected(c, 0, 1, &values[..2]);
            if pc == 5 {
                rejected(c, 0, 1, &values);
                rejected(c, 0, 3, &[]);
            } else {
                rejected(c, 0, 2, &values);
            }
            if pc == 1 {
                rejected(c, 0, 1, &[values[0], values[1], values[1]]);
                rejected(c, 0, 1, &[values[0], values[1], u64::MAX]);
                let extra = put(c, V::U32(17));
                rejected(c, 0, 1, &values);
                tellur_rt_v1_drop(c, extra);
                let stale = put(c, V::Unit);
                tellur_rt_v1_drop(c, stale);
                rejected(c, 0, 1, &[values[0], values[1], stale]);
                static FOREIGN: LoopControl = LoopControl { ..LOOP };
                assert!(
                    (&*c)
                        .selected_activation
                        .unwrap()
                        .abi_validate_loop(&FOREIGN, 1)
                        .is_err()
                );
            }
            for v in values {
                tellur_rt_v1_drop(c, v);
            }
        }
        let tag = parent(c, p, args, count);
        if (15..=18).contains(&tag) {
            rejected(c, 0, 0, &[]);
        }
        tag
    }
}
#[test]
fn loop_requests_preflight_identity_role_arity_and_all_handles_before_transfer() {
    static CHECKED: InvocationLayout = InvocationLayout {
        callback: checked_parent,
        ..PARENT
    };
    for mode in [0, 1] {
        REJECTIONS.with(|n| *n.borrow_mut() = 0);
        DROPS.with(|v| v.borrow_mut().clear());
        let (args, pointers) = arguments();
        let mut h = Host::new(json!({"mode":mode}), None, true, None);
        let mut frame = Parent::default();
        let invocation = unsafe { Invocation::new(&CHECKED, &mut frame) }.unwrap();
        let result =
            unsafe { execution::run_abi_invocation(&mut h, args, &CHECKED, invocation, 1) };
        assert_eq!(
            observation(&mut h, result, pointers)["result"]["outcome"],
            "done"
        );
        REJECTIONS.with(|n| assert!(*n.borrow() >= 29));
    }
}
#[test]
fn loop_lifecycle_rejects_stale_or_unfinished_transfers_without_fact_changes() {
    static FOREIGN: LoopControl = LoopControl { ..LOOP };
    let mut frame = Parent {
        fact: true,
        ..Default::default()
    };
    let mut invocation = unsafe { Invocation::new(&PARENT, &mut frame) }.unwrap();
    assert!(invocation.validate_loop(&LOOP, Some(3)).is_err());
    invocation.phase = InvocationPhase::Active;
    assert!(invocation.validate_loop(&FOREIGN, Some(3)).is_err());
    assert!(invocation.validate_loop(&LOOP, Some(2)).is_err());
    invocation.validate_loop(&LOOP, Some(3)).unwrap();
    invocation
        .state
        .as_mut()
        .unwrap()
        .values
        .push(Some(V::Unit));
    assert!(invocation.validate_loop(&LOOP, Some(3)).is_err());
    invocation.state.as_mut().unwrap().values.clear();
    invocation.state.as_mut().unwrap().error = Some("sticky work failure".into());
    assert!(invocation.validate_loop(&LOOP, None).is_err());
    assert_eq!(
        invocation.state.as_ref().unwrap().error.as_deref(),
        Some("sticky work failure")
    );
    invocation.state.as_mut().unwrap().error = None;
    invocation.redirect_scope(1).unwrap();
    assert!(invocation.validate_loop(&LOOP, Some(3)).is_err());
    assert!(frame.fact);
}

#[repr(C)]
#[derive(Default)]
struct HostFrame {
    pc: u32,
}
fn manifest(mode: u32, form: u32) -> Json {
    json!({"mode":mode,"wrapper":form,"primitives":[{"name":"tick","async":true,"returns":true,"parameters":[],"outcomes":[{"name":"done","fields":[]}]}]})
}
fn script(mode: u32) -> Option<Vec<ScriptCall>> {
    ((10..=11).contains(&mode)).then(|| {
        (0..2)
            .map(|_| ScriptCall {
                operation: "tick".into(),
                outcome: if mode == 11 { "invalid" } else { "done" }.into(),
                fields: vec![],
                waits: 3,
            })
            .collect()
    })
}
fn rust_host_delivery(_: &mut Activation, _: &mut Frames, name: String, fields: Vec<V>) -> R<()> {
    assert!(name == "done" || name == "cancelled");
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
unsafe extern "C" fn host(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe { host_step(c, p, args, count, &ABI_HOST) }
}
unsafe fn host_step(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
    producer: &'static Control,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<HostFrame>();
        if f.pc == 0 {
            assert_eq!(count, 0);
            (&mut *c).perform(|c| {
                c.host_control(
                    producer,
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
static RUST_HOST: Control = Control {
    name: "host",
    slots: 0,
    parameters: &[],
    resume: rust_host,
    ..RUST_CHILD
};
static ABI_HOST: Control = Control {
    resume: resume_invocation,
    ..RUST_HOST
};
static HOST: InvocationLayout = InvocationLayout {
    control: &ABI_HOST,
    callback: host,
    size: size_of::<HostFrame>(),
    align: align_of::<HostFrame>(),
    prefix: size_of::<HostFrame>(),
    children: &[],
    scopes: None,
};
#[repr(C)]
#[derive(Default)]
struct Wrapper {
    pc: u32,
    parent: Parent,
}
fn wrapper_delivery(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let (name, fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert_eq!(fields.len(), 2);
    for (slot, value) in fields.into_iter().enumerate() {
        v[a.base() + slot] = Some(value);
    }
    Ok(())
}
fn rust_wrapper(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let form = h.manifest["wrapper"].as_u64().unwrap();
    let b = a.base();
    if a.pc == 0 {
        tick(h, 40)?;
        let mut args = vec![v[b].take().unwrap(), v[b + 1].take().unwrap()];
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
        return Ok(if form == 1 {
            Step::Call {
                control: &RUST_PARENT,
                arguments: args,
                destination: None,
                deliver: wrapper_delivery,
            }
        } else {
            Step::Await {
                task: a.queue_invocation(h, &RUST_PARENT, &mut args)?,
                deliver: wrapper_delivery,
            }
        });
    }
    if a.pc == 1 || a.pc == 2 {
        tick(h, 40 + a.pc)?;
        if a.pc == 1 && form == 4 {
            a.cancel_tasks(h, &RUST_PARENT, v[b].as_ref().unwrap())?;
            a.pc = 2;
            return Ok(Step::Cancel);
        }
        a.pc = 3;
        return Ok(Step::Join {
            value: v[b].take().unwrap(),
            producer: &RUST_PARENT,
            deliver: wrapper_delivery,
        });
    }
    tick(h, 43)?;
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[b].take().unwrap(), v[b + 1].take().unwrap()],
    ))))
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
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !at(c, 40) {
                return 0;
            }
            let values = [take(c, 0), take(c, 1)];
            if form >= 3 {
                (&mut *c).perform(|c| {
                    c.spawn_control(&PARENT, "$task<parent>", "$started<parent>", 0, &values)
                });
                f.pc = 1;
                return 6;
            }
            f.pc = 3;
            if form == 1 {
                tellur_rt_v1_run_child(c, 0, values.as_ptr(), 2);
                return 5;
            }
            tellur_rt_v1_await(c, 0, values.as_ptr(), 2);
            return 3;
        }
        if f.pc == 1 || f.pc == 2 {
            if !at(c, 40 + f.pc as usize) {
                return 0;
            }
            let value = take(c, 0);
            if f.pc == 1 && form == 4 {
                (&mut *c).perform(|c| c.cancel_control(&ABI_PARENT, 0, value));
                f.pc = 2;
                return 8;
            }
            (&mut *c).perform(|c| c.join_control(&ABI_PARENT, value));
            f.pc = 3;
            return 7;
        }
        assert_eq!(count, 1);
        let mut fields = [0; 2];
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, fields.as_mut_ptr(), 2);
        store(c, 0, fields[0]);
        store(c, 1, fields[1]);
        if !at(c, 43) {
            return 0;
        }
        let values = [take(c, 0), take(c, 1)];
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, values.as_ptr(), 2);
        2
    }
}
static RUST_WRAPPER: Control = Control {
    name: "wrapper",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: rust_wrapper,
};
static ABI_WRAPPER: Control = Control {
    resume: resume_invocation,
    ..RUST_WRAPPER
};
static WRAPPER: InvocationLayout = InvocationLayout {
    control: &ABI_WRAPPER,
    callback: wrapper,
    size: size_of::<Wrapper>(),
    align: align_of::<Wrapper>(),
    prefix: offset_of!(Wrapper, parent),
    children: &[InlineChild {
        offset: offset_of!(Wrapper, parent),
        layout: &PARENT,
    }],
    scopes: None,
};
#[test]
fn recovery_frames_compose_with_inline_awaited_spawned_and_cancelled_ancestors() {
    exercise_wrapped(&[0, 1, 2, 3, 5, 6, 8, 9], &[1, 2, 3, 4]);
}
#[test]
fn recovering_loops_preserve_host_readiness_cancellation_and_driver_failures() {
    exercise_wrapped(&[10, 11], &[0, 1, 2, 3, 4]);
}
#[test]
fn recovery_retires_borrowing_spawned_and_task_set_siblings_before_reusing_loop_slots() {
    exercise_wrapped(&[12, 13], &[0, 1, 2, 3, 4]);
}
#[repr(C)]
#[derive(Default)]
struct Supervisor {
    pc: u32,
    work: Parent,
    handler: Child,
}
fn rust_named_work(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 6 {
        tick(h, 16)?;
        v[a.base() + 3] = None;
        v[a.base() + 4] = None;
        return Ok(Step::Complete(Completion::Settled));
    }
    rust_parent(h, v, a)
}
unsafe extern "C" fn named_work(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let pc = tellur_rt_v1_resume_state(c, (*p.cast::<Parent>()).pc);
        if pc == 6 {
            if !at(c, 16) {
                return 0;
            }
            tellur_rt_v1_release_local(c, 3);
            tellur_rt_v1_release_local(c, 4);
            tellur_rt_v1_settled(c);
            return 2;
        }
        parent(c, p, args, count)
    }
}
fn rust_named_handler(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, 51)?;
    h.emit(v[a.base()].take().unwrap())?;
    Ok(Step::Complete(Completion::Settled))
}
unsafe extern "C" fn named_handler(
    c: *mut Context<'_>,
    _: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 1);
        store(c, 0, *args);
        if !at(c, 51) {
            return 0;
        }
        let v = take(c, 0);
        tellur_rt_v1_emit(c, v);
        tellur_rt_v1_settled(c);
        2
    }
}
fn rust_supervisor(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, 50)?;
        a.pc = 1;
        return Ok(Step::Supervise {
            work: &RUST_WORK,
            handler: &RUST_HANDLER,
            arguments: arguments().0,
        });
    }
    tick(h, 52)?;
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()],
    ))))
}
unsafe extern "C" fn supervisor(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Supervisor>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !at(c, 50) {
                return 0;
            }
            let values = arguments()
                .0
                .into_iter()
                .map(|v| put(c, v))
                .collect::<Vec<_>>();
            f.pc = 1;
            tellur_rt_v1_supervise_children(c, 0, 1, values.as_ptr(), 2);
            return 12;
        }
        assert_eq!(count, 0);
        if !at(c, 52) {
            return 0;
        }
        let args = [take(c, 0), take(c, 1)];
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, args.as_ptr(), 2);
        2
    }
}
static RUST_WORK: Control = Control {
    resume: rust_named_work,
    ..RUST_PARENT
};
static RUST_HANDLER: Control = Control {
    name: "handler",
    slots: 1,
    parameters: &[0],
    resume: rust_named_handler,
    ..RUST_CHILD
};
static RUST_SUPERVISOR: Control = Control {
    name: "supervisor",
    resume: rust_supervisor,
    ..RUST_WRAPPER
};
static ABI_WORK: Control = Control {
    resume: resume_invocation,
    ..RUST_WORK
};
static ABI_HANDLER: Control = Control {
    resume: resume_invocation,
    ..RUST_HANDLER
};
static ABI_SUPERVISOR: Control = Control {
    resume: resume_invocation,
    ..RUST_SUPERVISOR
};
static WORK: InvocationLayout = InvocationLayout {
    control: &ABI_WORK,
    callback: named_work,
    ..PARENT
};
static HANDLER: InvocationLayout = InvocationLayout {
    control: &ABI_HANDLER,
    callback: named_handler,
    ..CHILD
};
static SUPERVISOR: InvocationLayout = InvocationLayout {
    control: &ABI_SUPERVISOR,
    callback: supervisor,
    size: size_of::<Supervisor>(),
    align: align_of::<Supervisor>(),
    prefix: offset_of!(Supervisor, work),
    children: &[
        InlineChild {
            offset: offset_of!(Supervisor, work),
            layout: &WORK,
        },
        InlineChild {
            offset: offset_of!(Supervisor, handler),
            layout: &HANDLER,
        },
    ],
    scopes: None,
};
#[test]
fn named_work_runs_recovering_loops_and_settles_only_after_local_scope_retirement() {
    exercise_named(&SUPERVISOR);
}
fn exercise_named(layout: &'static InvocationLayout) {
    for mode in [1, 2, 3, 8, 9, 10, 11, 12, 13] {
        for fuel in 0..=40 {
            DROPS.with(|v| v.borrow_mut().clear());
            let (args, pointers) = arguments();
            let mut h = Host::new(manifest(mode, 0), script(mode), true, Some(fuel));
            let result = execution::run_source_entry(&mut h, args, &RUST_SUPERVISOR);
            let expected = observation(&mut h, result, pointers);
            for quota in [1, 3, usize::MAX] {
                for arena in [false, true] {
                    DROPS.with(|v| v.borrow_mut().clear());
                    let (args, pointers) = arguments();
                    let mut h = Host::new(manifest(mode, 0), script(mode), true, Some(fuel));
                    let mut frame = Supervisor::default();
                    let pool = physical_frames::Arena::default();
                    let invocation = if arena {
                        unsafe { Invocation::in_arena(&pool, layout) }
                    } else {
                        unsafe { Invocation::new(layout, &mut frame) }
                    }
                    .unwrap();
                    let storage = invocation.arena.clone();
                    let result = unsafe {
                        execution::run_abi_invocation(&mut h, args, layout, invocation, quota)
                    };
                    assert_eq!(
                        observation(&mut h, result, pointers),
                        expected,
                        "mode={mode} fuel={fuel} quota={quota} arena={arena}"
                    );
                    assert_eq!(pool.counts().2, 0);
                    assert_eq!(storage.counts().2, 0);
                }
            }
        }
    }
}

// The same callbacks and original-continuation oracle also run through C-safe
// module registration, whose original control identities are newly owned by the
// registry. No private InvocationLayout is substituted for a registered root.
const fn compiled_slots(data: &'static [usize]) -> CompiledSlots {
    CompiledSlots {
        data: data.as_ptr(),
        count: data.len(),
    }
}
const fn compiled_boundary(raw: &'static BoundaryControl) -> CompiledBoundary {
    CompiledBoundary {
        name: raw.name.as_ptr(),
        name_len: raw.name.len(),
        body: raw.body as u32,
        handler: raw.handler as u32,
        after: raw.after as u32,
        retained: compiled_slots(raw.retained),
        retire: compiled_slots(raw.retire),
        work: compiled_slots(raw.work),
        handling: compiled_slots(raw.handling),
        fault_slot: raw.fault_slot,
    }
}
static REGISTERED_SCOPES: CompiledScopes = CompiledScopes {
    states: 12,
    boundaries: [compiled_boundary(&BOUNDARY), compiled_boundary(&INNER)].as_ptr(),
    boundary_count: 2,
    loops: [CompiledLoop {
        target: LOOP.target,
        body: LOOP.body as u32,
        after: LOOP.after as u32,
        locals: compiled_slots(LOOP.locals),
        parameters: compiled_slots(&[2, 3, 4]),
        reset_facts: Some(reset_facts),
    }]
    .as_ptr(),
    loop_count: 1,
};
const fn compiled_control(
    layout: &'static InvocationLayout,
    children: &'static [CompiledChild],
    scopes: *const CompiledScopes,
) -> CompiledControlV3 {
    let c = layout.control;
    CompiledControlV3 {
        control: CompiledControl {
            producer: c.name.as_ptr(),
            producer_len: c.name.len(),
            name: c.name.as_ptr(),
            name_len: c.name.len(),
            slots: c.slots,
            parameters: c.parameters.as_ptr(),
            parameter_count: c.parameters.len(),
            frame_size: layout.size,
            frame_align: layout.align,
            frame_prefix: layout.prefix,
            callback: Some(layout.callback),
            children: children.as_ptr(),
            child_count: children.len(),
            retained: c.retained.as_ptr(),
            retained_count: c.retained.len(),
        },
        scopes,
    }
}
static REGISTERED_CONTROLS: [CompiledControlV3; 7] = [
    compiled_control(
        &PARENT,
        &[CompiledChild {
            control: 1,
            offset: offset_of!(Parent, child),
        }],
        &REGISTERED_SCOPES,
    ),
    compiled_control(
        &CHILD,
        &[CompiledChild {
            control: 2,
            offset: offset_of!(Child, host),
        }],
        std::ptr::null(),
    ),
    CompiledControlV3 {
        control: CompiledControl {
            producer: b"tick$host$cancel".as_ptr(),
            producer_len: 16,
            callback: Some(registered_host),
            ..compiled_control(&HOST, &[], std::ptr::null()).control
        },
        scopes: std::ptr::null(),
    },
    compiled_control(
        &WRAPPER,
        &[CompiledChild {
            control: 0,
            offset: offset_of!(Wrapper, parent),
        }],
        std::ptr::null(),
    ),
    compiled_control(
        &WORK,
        &[CompiledChild {
            control: 1,
            offset: offset_of!(Parent, child),
        }],
        &REGISTERED_SCOPES,
    ),
    compiled_control(
        &HANDLER,
        &[CompiledChild {
            control: 2,
            offset: offset_of!(Child, host),
        }],
        std::ptr::null(),
    ),
    compiled_control(
        &SUPERVISOR,
        &[
            CompiledChild {
                control: 4,
                offset: offset_of!(Supervisor, work),
            },
            CompiledChild {
                control: 5,
                offset: offset_of!(Supervisor, handler),
            },
        ],
        std::ptr::null(),
    ),
];
fn registered() -> &'static CompiledRegistry {
    static REGISTRY: std::sync::OnceLock<CompiledRegistry> = std::sync::OnceLock::new();
    unsafe {
        CompiledRegistry::register_v3(
            &REGISTRY,
            3,
            REGISTERED_CONTROLS.as_ptr(),
            REGISTERED_CONTROLS.len(),
        )
    }
    .unwrap()
}
#[test]
fn compiled_v3_recovery_matches_original_scopes_buffers_facts_and_host_failures() {
    let registry = registered();
    exercise_with_layouts(
        &(0..=13).collect::<Vec<_>>(),
        &[0],
        registry.layout(0).unwrap(),
        registry.layout(3).unwrap(),
    );
}
#[test]
fn compiled_v3_recovery_preserves_inline_and_awaited_ancestry() {
    let registry = registered();
    exercise_with_layouts(
        &[1, 2, 3, 8, 9, 10, 11, 12, 13],
        &[1, 2],
        registry.layout(0).unwrap(),
        registry.layout(3).unwrap(),
    );
}
#[test]
fn compiled_v3_named_work_settles_after_registered_local_scopes() {
    exercise_named(registered().layout(6).unwrap());
}

unsafe extern "C" fn registered_host(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe { host_step(c, p, args, count, registered().layout(2).unwrap().control) }
}
