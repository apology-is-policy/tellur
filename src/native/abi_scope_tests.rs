use super::*;
use execution::{Activation, BoundaryControl, Control, Step};
use std::cell::RefCell;
use std::mem::{align_of, offset_of, size_of};

thread_local! { static DROPS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) }; }
struct Probe(u32);
impl Drop for Probe {
    fn drop(&mut self) {
        DROPS.with(|v| v.borrow_mut().push(self.0));
    }
}
fn mode(h: &Host) -> u32 {
    h.manifest["mode"].as_u64().unwrap() as u32
}
fn tick(h: &mut Host, id: usize) -> R<()> {
    h.at(id);
    h.tick(id, 1)
}
fn fault(h: &mut Host, text: &str) -> R<()> {
    h.fault(V::String(text.into()))
}
fn resource(h: &mut Host) -> V {
    h.resources.insert("probe", Probe(1)).unwrap()
}
fn reference(root: usize) -> V {
    V::Reference {
        ty: "&probe".into(),
        root,
        fields: vec![],
        evidence: Default::default(),
    }
}
fn report(h: &mut Host, v: &Frames, base: usize, slot: usize) -> R<()> {
    assert!(h.resources.values.is_empty());
    let V::Construct { fields, .. } = v[base + slot].as_ref().unwrap() else {
        panic!("fault report missing")
    };
    h.emit(fields[0].clone())?;
    h.emit(v[base + 1].as_ref().unwrap().clone())
}
static OUTER: BoundaryControl = BoundaryControl {
    name: "parent$boundary@10",
    body: 1,
    handler: 6,
    after: 5,
    retained: &[1],
    retire: &[2, 5],
    work: &[2, 5],
    handling: &[3],
    fault_slot: 3,
};
static INNER: BoundaryControl = BoundaryControl {
    name: "parent$boundary@11",
    body: 2,
    handler: 7,
    after: 4,
    retained: &[1],
    retire: &[2, 5],
    work: &[2, 5],
    handling: &[4],
    fault_slot: 4,
};
static SCOPES: ScopeLayout = ScopeLayout {
    states: 10,
    boundaries: &[&OUTER, &INNER],
};
#[repr(C)]
#[derive(Default)]
struct Leaf {
    pc: u32,
    host: HostFrame,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    stale: u64,
    buffer: usize,
    rounds: u32,
    child: Leaf,
    work: Leaf,
    handler: Leaf,
}
unsafe fn store(c: *mut Context<'_>, slot: usize, value: u64) {
    unsafe { tellur_rt_v1_store(c, slot, std::ptr::null(), 0, value) }
}
unsafe fn take(c: *mut Context<'_>, slot: usize) -> u64 {
    unsafe { tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 1) }
}
unsafe fn at(c: *mut Context<'_>, id: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, id, id, 1);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn fail(c: *mut Context<'_>, message: &str) {
    unsafe {
        let v = tellur_rt_v1_value(c, 3, 0, message.as_ptr(), message.len());
        tellur_rt_v1_fault(c, v);
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
        let pc = tellur_rt_v1_resume_state(c, f.pc);
        f.pc = pc;
        if tellur_rt_v1_ok(c) == 0 {
            return 0;
        }
        let m = mode((&*c).host);
        if pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            let c = &*c;
            let V::String(text) = c.storage.values()[c.local(0).unwrap()].as_ref().unwrap() else {
                panic!()
            };
            f.buffer = text.as_ptr() as usize;
        } else {
            let c = &*c;
            let V::String(text) = c.storage.values()[c.local(0).unwrap()].as_ref().unwrap() else {
                panic!()
            };
            assert_eq!(text.as_ptr() as usize, f.buffer);
        }
        match pc {
            0 => {
                if !at(c, 10) {
                    return 0;
                }
                tellur_rt_v1_scope(c, 0, 0);
                13
            }
            1 => {
                if !at(c, 11) {
                    return 0;
                }
                let v = (&mut *c).perform(|c| {
                    let v = resource(c.host);
                    Ok(c.put(v))
                });
                store(c, 2, v);
                f.pc = 2;
                if (6..=7).contains(&m) {
                    tellur_rt_v1_scope(c, 1, 0);
                    13
                } else {
                    1
                }
            }
            2 => {
                if !at(c, 12) {
                    return 0;
                }
                if [1, 3, 4, 6, 7].contains(&m) {
                    // A stranded borrowed temporary must disappear before owners.
                    f.stale = (&mut *c).perform(|c| Ok(c.put(reference(c.local(2)?))));
                    fail(c, "work fault");
                    return 0;
                }
                if (8..=9).contains(&m) {
                    f.pc = 3;
                    tellur_rt_v1_supervise_children(c, 1, 2, std::ptr::null(), 0);
                    return 12;
                }
                let borrow = (&mut *c).perform(|c| Ok(c.put(reference(c.local(2)?))));
                // Generated callers reset a retired inline child before reuse.
                f.child = Leaf::default();
                if m == 16 || m == 17 {
                    if m == 16 {
                        (&mut *c).perform(|c| {
                            c.spawn_control(&LEAF, "$task<leaf>", "$started<leaf>", 5, &[borrow])
                        });
                        f.pc = 8;
                        return 6;
                    }
                    let set = (&mut *c)
                        .perform(|c| c.empty_task_set(&ABI_LEAF, "$tasks<leaf>", "$started<leaf>"));
                    (&mut *c).perform(|c| {
                        c.spawn_into_control(
                            &LEAF,
                            "$task<leaf>",
                            "$started<leaf>",
                            "$tasks<leaf>",
                            5,
                            set,
                            &[borrow],
                        )
                    });
                    f.pc = 8;
                    return 10;
                }
                f.pc = 3;
                tellur_rt_v1_await(c, 0, [borrow].as_ptr(), 1);
                3
            }
            3 => {
                assert_eq!(count, 1);
                tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, std::ptr::null_mut(), 0);
                if !at(c, 13) {
                    return 0;
                }
                let v = take(c, 2);
                (&mut *c).perform(|c| {
                    let v = c.take(v)?;
                    c.host.release_frame(&[Some(v)]);
                    Ok(())
                });
                if m == 11 {
                    let value = take(c, 0);
                    tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
                    return 2;
                }
                f.pc = 4;
                4
            }
            4 => {
                tellur_rt_v1_scope(c, 0, 1);
                14
            }
            5 => {
                if !at(c, 15) {
                    return 0;
                }
                if m == 10 && f.rounds < 2 {
                    f.rounds += 1;
                    tellur_rt_v1_scope(c, 0, 0);
                    return 13;
                }
                let v = take(c, 0);
                tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [v].as_ptr(), 1);
                2
            }
            8 => {
                f.pc = 9;
                1
            }
            9 => {
                if !at(c, 18) {
                    return 0;
                }
                fail(c, "parent with pending sibling");
                0
            }
            6 | 7 => {
                assert_eq!(count, 0);
                assert!((&*c).get(f.stale).is_err());
                if !at(c, 10 + pc as usize) {
                    return 0;
                }
                (&mut *c).perform(|c| {
                    report(
                        c.host,
                        c.storage.values(),
                        c.local(0)?,
                        if pc == 6 { 3 } else { 4 },
                    )
                });
                if m == 3 || (pc == 7 && m == 7) {
                    fail(c, "handler fault");
                    return 0;
                }
                if m == 4 || pc == 7 {
                    tellur_rt_v1_escalate(c);
                    return 0;
                }
                tellur_rt_v1_process(c, if m == 2 { 23 } else { 0 });
                2
            }
            _ => {
                tellur_rt_v1_invalid_resume(c);
                0
            }
        }
    }
}
unsafe extern "C" fn leaf(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !at(c, 20) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        let m = mode((&*c).host);
        if f.pc == 1 && m >= 13 {
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
        if m == 2 || m == 15 {
            fail(c, "child fault");
            return 0;
        }
        if m == 5 {
            (&mut *c).perform::<()>(|_| Err("E_HOST: bad response".into()));
            return 0;
        }
        tellur_rt_v1_release_local(c, 0);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
        2
    }
}
macro_rules! control {
    ($id:ident,$name:literal,$slots:expr,$params:expr,$resume:ident) => {
        static $id: Control = Control {
            name: $name,
            slots: $slots,
            atoms: 0,
            parameters: $params,
            retained: &[],
            constructor: false,
            access: None,
            resume: $resume,
        };
    };
}
control!(ABI_PARENT, "parent", 6, &[0, 1], resume_invocation);
control!(ABI_LEAF, "leaf", 1, &[0], resume_invocation);
static LEAF: InvocationLayout = InvocationLayout {
    control: &ABI_LEAF,
    callback: leaf,
    size: size_of::<Leaf>(),
    align: align_of::<Leaf>(),
    prefix: offset_of!(Leaf, host),
    children: &[InlineChild {
        offset: offset_of!(Leaf, host),
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
    children: &[
        InlineChild {
            offset: offset_of!(Parent, child),
            layout: &LEAF,
        },
        InlineChild {
            offset: offset_of!(Parent, work),
            layout: &NAMED,
        },
        InlineChild {
            offset: offset_of!(Parent, handler),
            layout: &NAMED_HANDLER,
        },
    ],
    scopes: Some(&SCOPES),
};
fn rust_delivery(_: &mut Activation, _: &mut Frames, result: Completion) -> R<()> {
    let (name, fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert!(fields.is_empty());
    Ok(())
}
fn rust_leaf(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, 20)?;
        a.pc = 1;
        return Ok(Step::Yield);
    }
    if a.pc == 1 && mode(h) >= 13 {
        a.pc = 2;
        return Ok(Step::Await {
            task: a.queue_invocation(h, &RUST_HOST, &mut vec![])?,
            deliver: rust_delivery,
        });
    }
    tick(h, 21)?;
    if mode(h) == 2 || mode(h) == 15 {
        fault(h, "child fault")?;
    }
    if mode(h) == 5 {
        return Err("E_HOST: bad response".into());
    }
    v[a.base()] = None;
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
fn rust_parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    let b = a.base();
    let m = mode(h);
    match a.pc {
        0 => {
            tick(h, 10)?;
            Ok(Step::Boundary(&OUTER))
        }
        1 => {
            tick(h, 11)?;
            v[b + 2] = Some(resource(h));
            a.pc = 2;
            if (6..=7).contains(&m) {
                Ok(Step::Boundary(&INNER))
            } else {
                Ok(Step::Yield)
            }
        }
        2 => {
            tick(h, 12)?;
            if [1, 3, 4, 6, 7].contains(&m) {
                fault(h, "work fault")?;
            }
            a.pc = 3;
            if (8..=9).contains(&m) {
                return Ok(Step::Supervise {
                    work: &RUST_NAMED,
                    handler: &RUST_NAMED_HANDLER,
                    arguments: vec![],
                });
            }
            if m == 16 || m == 17 {
                let mut arguments = vec![reference(b + 2)];
                let mut cold = Some(a.defer_task(h, &RUST_LEAF, "$task<leaf>", &mut arguments)?);
                let task = a.queue_started(h, &RUST_LEAF, &mut cold)?;
                a.pc = 8;
                return if m == 16 {
                    Ok(Step::Spawn {
                        task,
                        slot: 5,
                        ty: "$started<leaf>",
                    })
                } else {
                    Ok(Step::SpawnInto {
                        task,
                        set: V::Construct {
                            ty: "$tasks<leaf>".into(),
                            name: "tasks".into(),
                            fields: vec![],
                        },
                        slot: 5,
                        ty: "$started<leaf>",
                    })
                };
            }
            Ok(Step::Await {
                task: a.queue_invocation(h, &RUST_LEAF, &mut vec![reference(b + 2)])?,
                deliver: rust_delivery,
            })
        }
        3 => {
            tick(h, 13)?;
            h.release_frame(&[v[b + 2].take()]);
            if m == 11 {
                return Ok(Step::Complete(Completion::Outcome((
                    "done".into(),
                    vec![v[b].take().unwrap()],
                ))));
            }
            a.pc = 4;
            Ok(Step::Continue)
        }
        4 => Ok(Step::BoundaryEnd(&OUTER)),
        5 => {
            tick(h, 15)?;
            if m == 10 {
                // This test-only counter is not a source slot owned by a scope.
                let round = h.manifest["round"].as_u64().unwrap_or(0);
                if round < 2 {
                    h.manifest["round"] = json!(round + 1);
                    return Ok(Step::Boundary(&OUTER));
                }
            }
            Ok(Step::Complete(Completion::Outcome((
                "done".into(),
                vec![v[b].take().unwrap()],
            ))))
        }
        8 => {
            a.pc = 9;
            Ok(Step::Yield)
        }
        9 => {
            tick(h, 18)?;
            fault(h, "parent with pending sibling")?;
            unreachable!()
        }
        6 | 7 => {
            tick(h, 10 + a.pc)?;
            report(h, v, b, if a.pc == 6 { 3 } else { 4 })?;
            if m == 3 || (a.pc == 7 && m == 7) {
                fault(h, "handler fault")?;
            }
            if m == 4 || a.pc == 7 {
                h.escalate()?;
            }
            Ok(Step::Complete(Completion::Process(if m == 2 {
                23
            } else {
                0
            })))
        }
        _ => unreachable!(),
    }
}
control!(RUST_PARENT, "parent", 6, &[0, 1], rust_parent);
control!(RUST_LEAF, "leaf", 1, &[0], rust_leaf);
fn observation(h: &mut Host, result: R<Completion>, buffer: *const u8) -> Json {
    let result = match result {
        Ok(Completion::Outcome((name, fields))) => {
            let V::String(text) = &fields[0] else {
                panic!()
            };
            assert_eq!(text.as_ptr(), buffer);
            json!({"outcome":name,"fields":fields})
        }
        Ok(Completion::Process(status)) => json!({"process":status}),
        Ok(_) => panic!("unexpected settlement"),
        Err(error) => {
            json!({"fault":h.activations.failure("parent",fault::Phase::Execution,&error)})
        }
    };
    assert!(h.resources.values.is_empty() && h.resources.scopes.is_empty() && h.tasks.is_empty());
    json!({"result":result,"events":h.events,"fuel":h.fuel,"drops":DROPS.with(|v|v.borrow().clone())})
}
fn args() -> (Vec<V>, *const u8) {
    let text = "preserved parent buffer".repeat(50);
    let p = text.as_ptr();
    (
        vec![V::String(text), V::String("retained context".into())],
        p,
    )
}
fn exercise(modes: &[u32]) {
    exercise_wrapped(modes, &[0]);
}
fn exercise_wrapped(modes: &[u32], forms: &[u32]) {
    for &form in forms {
        for &m in modes {
            for fuel in 0..=40 {
                DROPS.with(|v| v.borrow_mut().clear());
                let (a, p) = args();
                let mut h = Host::new(manifest(m, form), script(m), true, Some(fuel));
                let result = execution::run_source_entry(
                    &mut h,
                    a,
                    if form == 0 {
                        &RUST_PARENT
                    } else {
                        &RUST_WRAPPER
                    },
                );
                let expected = observation(&mut h, result, p);
                for quota in [1, 3, usize::MAX] {
                    for arena in [false, true] {
                        DROPS.with(|v| v.borrow_mut().clear());
                        let (a, p) = args();
                        let mut h = Host::new(manifest(m, form), script(m), true, Some(fuel));
                        let mut frame = Wrapper::default();
                        let pool = physical_frames::Arena::default();
                        let layout = if form == 0 { &PARENT } else { &WRAPPER };
                        let invocation = if arena {
                            unsafe { Invocation::in_arena(&pool, layout) }
                        } else if form == 0 {
                            unsafe { Invocation::new(&PARENT, &mut frame.parent) }
                        } else {
                            unsafe { Invocation::new(&WRAPPER, &mut frame) }
                        }
                        .unwrap();
                        let storage = invocation.arena.clone();
                        let result = unsafe {
                            execution::run_abi_invocation(&mut h, a, layout, invocation, quota)
                        };
                        assert_eq!(storage.counts().2, 0);
                        assert_eq!(
                            observation(&mut h, result, p),
                            expected,
                            "mode={m} form={form} fuel={fuel} quota={quota} arena={arena}"
                        );
                        assert_eq!(pool.counts().2, 0);
                    }
                }
            }
        }
    }
}
#[test]
fn lexical_scope_frames_match_original_fuel_borrow_retirement_and_process_dispositions() {
    exercise(&[0, 1, 2, 3, 4, 5]);
}
#[test]
fn nested_lexical_handlers_preserve_frame_and_secondary_fault_authority() {
    exercise(&[6, 7]);
}

#[test]
fn lexical_scope_metadata_rejects_invalid_states_slots_and_aliases_before_admission() {
    static BAD_STATE: BoundaryControl = BoundaryControl { body: 10, ..OUTER };
    static BAD_DUPLICATE: BoundaryControl = BoundaryControl {
        retained: &[1, 1],
        ..OUTER
    };
    static BAD_SLOT: BoundaryControl = BoundaryControl {
        work: &[6],
        ..OUTER
    };
    static BAD_ROLES: BoundaryControl = BoundaryControl {
        retire: &[1, 2],
        ..OUTER
    };
    static BAD_FAULT: BoundaryControl = BoundaryControl {
        fault_slot: 4,
        ..OUTER
    };
    static BAD_OVERLAP: BoundaryControl = BoundaryControl { after: 1, ..OUTER };
    static CASES: [ScopeLayout; 8] = [
        ScopeLayout {
            states: 0,
            boundaries: &[&OUTER],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&BAD_STATE],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&BAD_DUPLICATE],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&BAD_SLOT],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&BAD_ROLES],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&BAD_FAULT],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&BAD_OVERLAP],
        },
        ScopeLayout {
            states: 8,
            boundaries: &[&OUTER, &OUTER],
        },
    ];
    for scopes in &CASES {
        let layout = InvocationLayout {
            scopes: Some(scopes),
            ..PARENT
        };
        assert!(layout.validate().is_err());
    }
    assert!(PARENT.validate().is_ok());
}
thread_local! { static REJECTIONS: RefCell<usize> = const { RefCell::new(0) }; }
unsafe fn rejected(c: *mut Context<'_>, index: usize, end: u32) {
    unsafe {
        let pending = (&*c).request.as_ref().map(RequestedInvocation::tag);
        let scopes = (&*c).host.resources.scopes.len();
        tellur_rt_v1_scope(c, index, end);
        assert!((&*c).error.is_some());
        // Sticky failure prevents a subsequent attempted effect.
        let events = (&*c).host.events.len();
        tellur_rt_v1_emit(c, u64::MAX);
        assert_eq!((&*c).host.events.len(), events);
        assert_eq!((&*c).host.resources.scopes.len(), scopes);
        assert_eq!(
            (&*c).request.as_ref().map(RequestedInvocation::tag),
            pending
        );
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
        match pc {
            0 => {
                rejected(c, 99, 0);
                rejected(c, 0, 1);
                rejected(c, 0, 2);
            }
            1 => rejected(c, 0, 0),
            4 => rejected(c, 1, 1),
            5 | 6 => rejected(c, 0, 1),
            _ => {}
        }
        let tag = parent(c, p, args, count);
        if pc == 0 {
            rejected(c, 0, 0);
        }
        tag
    }
}
static CHECKED_PARENT: InvocationLayout = InvocationLayout {
    callback: checked_parent,
    ..PARENT
};
#[test]
fn lexical_scope_requests_reject_duplicates_unselected_ends_and_bad_ids_without_effects() {
    for (m, expected, rejections) in [
        (
            0,
            json!({"outcome":"done", "fields":[V::String("preserved parent buffer".repeat(50))]}),
            7,
        ),
        (1, json!({"process":0}), 6),
    ] {
        DROPS.with(|v| v.borrow_mut().clear());
        REJECTIONS.with(|n| *n.borrow_mut() = 0);
        let (arguments, buffer) = args();
        let mut h = Host::new(json!({"mode":m}), None, true, None);
        let mut frame = Parent::default();
        let invocation = unsafe { Invocation::new(&CHECKED_PARENT, &mut frame) }.unwrap();
        let result = unsafe {
            execution::run_abi_invocation(&mut h, arguments, &CHECKED_PARENT, invocation, 1)
        };
        assert_eq!(observation(&mut h, result, buffer)["result"], expected);
        REJECTIONS.with(|n| assert_eq!(*n.borrow(), rejections));
    }
}
#[test]
fn lexical_redirect_requires_valid_lifecycle_and_discarded_handles_never_recur() {
    let pool = physical_frames::Arena::default();
    let mut invocation = unsafe { Invocation::in_arena(&pool, &PARENT) }.unwrap();
    assert!(invocation.redirect_scope(1).is_err());
    assert!(invocation.enter_scope_handler(6).is_err());
    invocation.phase = InvocationPhase::Active;
    assert!(invocation.redirect_scope(10).is_err());
    invocation.redirect_scope(1).unwrap();
    assert!(invocation.redirect_scope(2).is_err());
    let state = invocation.state.as_mut().unwrap();
    state.values.push(Some(V::String("abandoned value".into())));
    state.outcomes.push(Some((
        "done".into(),
        vec![V::String("abandoned result".into())],
    )));
    state.error = Some("E_FAULT: work".into());
    invocation.delivered.push(1);
    invocation.abandon_scope_work().unwrap();
    let state = invocation.state.as_ref().unwrap();
    assert!(state.values.is_empty() && state.outcomes.is_empty() && state.error.is_some());
    assert_eq!((state.value_base, state.outcome_base), (1, 1));
    assert!(invocation.delivered.is_empty() && invocation.resume_at.is_none());
    assert!(invocation.redirect_scope(6).is_err());
    for pc in [0, 10, usize::MAX] {
        assert!(invocation.enter_scope_handler(pc).is_err());
        assert!(invocation.phase == InvocationPhase::Failed);
        assert_eq!(
            invocation.state.as_ref().unwrap().error.as_deref(),
            Some("E_FAULT: work")
        );
        assert!(invocation.resume_at.is_none());
    }
    invocation.enter_scope_handler(6).unwrap();
    assert_eq!(invocation.resume_at, Some(6));
    assert!(invocation.enter_scope_handler(6).is_err());
    let mut h = Host::new(json!({}), None, true, None);
    let mut values = Frames::default();
    let mut c = invocation.state.take().unwrap().select(&mut h, &mut values);
    assert!(c.error.is_none());
    assert_eq!(c.put(V::Unit), 2);
    assert!(c.get(1).is_err());
    c.require_outcome_room().unwrap();
    assert_eq!(c.put_outcome(("done".into(), vec![])), 2);
    assert!(c.outcome(1).is_err());
    drop(c);
    drop(invocation);
    assert_eq!(pool.counts().2, 0);
}

unsafe extern "C" fn named(
    c: *mut Context<'_>,
    p: *mut u8,
    _args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 0);
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            if !at(c, 30) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        if !at(c, 31) {
            return 0;
        }
        fail(c, "named work");
        0
    }
}
unsafe extern "C" fn named_handler(
    c: *mut Context<'_>,
    _p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 1);
        store(c, 0, *args);
        if !at(c, 32) {
            return 0;
        }
        let message = take(c, 0);
        tellur_rt_v1_emit(c, message);
        if mode((&*c).host) == 8 {
            tellur_rt_v1_escalate(c);
        } else {
            fail(c, "named handler");
        }
        0
    }
}
control!(ABI_NAMED, "named", 0, &[], resume_invocation);
control!(
    ABI_NAMED_HANDLER,
    "named_handler",
    1,
    &[0],
    resume_invocation
);
static NAMED: InvocationLayout = InvocationLayout {
    control: &ABI_NAMED,
    callback: named,
    ..LEAF
};
static NAMED_HANDLER: InvocationLayout = InvocationLayout {
    control: &ABI_NAMED_HANDLER,
    callback: named_handler,
    ..LEAF
};
fn rust_named(h: &mut Host, _v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, 30)?;
        a.pc = 1;
        return Ok(Step::Yield);
    }
    tick(h, 31)?;
    fault(h, "named work")?;
    unreachable!()
}
fn rust_named_handler(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, 32)?;
    h.emit(v[a.base()].take().unwrap())?;
    if mode(h) == 8 {
        h.escalate()?;
    } else {
        fault(h, "named handler")?;
    }
    unreachable!()
}
control!(RUST_NAMED, "named", 0, &[], rust_named);
control!(
    RUST_NAMED_HANDLER,
    "named_handler",
    1,
    &[0],
    rust_named_handler
);
#[test]
fn lexical_handover_after_named_handler_failure_reuses_only_the_parent_frame() {
    exercise(&[8, 9]);
}

#[repr(C)]
#[derive(Default)]
struct Wrapper {
    pc: u32,
    parent: Parent,
    work: LexicalWork,
    handler: HostFrame,
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
            if form == 6 {
                (&mut *c).perform(|c| {
                    c.spawn_control(
                        &OUTSIDE_FAULT,
                        "$task<outside>",
                        "$started<outside>",
                        2,
                        &[],
                    )
                });
                f.pc = 5;
                return 6;
            }
            if form == 5 {
                let values = args_for_named(c);
                tellur_rt_v1_supervise_children(c, 1, 2, values.as_ptr(), 2);
                f.pc = 4;
                return 12;
            }
            let value = take(c, 0);
            let context = take(c, 1);
            if form >= 3 {
                (&mut *c).perform(|c| {
                    c.spawn_control(
                        &PARENT,
                        "$task<parent>",
                        "$started<parent>",
                        0,
                        &[value, context],
                    )
                });
                f.pc = 1;
                return 6;
            }
            f.pc = 3;
            if form == 1 {
                tellur_rt_v1_run_child(c, 0, [value, context].as_ptr(), 2);
                return 5;
            }
            tellur_rt_v1_await(c, 0, [value, context].as_ptr(), 2);
            return 3;
        }
        if f.pc == 5 {
            if !at(c, 45) {
                return 0;
            }
            let values = [take(c, 0), take(c, 1)];
            f.pc = 3;
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
        if f.pc == 4 {
            assert_eq!(count, 0);
            if !at(c, 44) {
                return 0;
            }
            let value = take(c, 0);
            tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
            return 2;
        }
        assert_eq!((f.pc, count), (3, 1));
        let mut field = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut field, 1);
        store(c, 0, field);
        if !at(c, 43) {
            return 0;
        }
        let value = take(c, 0);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
        2
    }
}
control!(WRAPPER_CONTROL, "wrapper", 3, &[0, 1], resume_invocation);
static WRAPPER: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &WRAPPER_CONTROL,
    callback: wrapper,
    size: size_of::<Wrapper>(),
    align: align_of::<Wrapper>(),
    prefix: offset_of!(Wrapper, parent),
    children: &[
        InlineChild {
            offset: offset_of!(Wrapper, parent),
            layout: &PARENT,
        },
        InlineChild {
            offset: offset_of!(Wrapper, work),
            layout: &LEXICAL_WORK,
        },
        InlineChild {
            offset: offset_of!(Wrapper, handler),
            layout: &OUTSIDE_HANDLER,
        },
    ],
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
        tick(h, 40)?;
        if form == 6 {
            let mut args = vec![];
            let mut cold =
                Some(a.defer_task(h, &RUST_OUTSIDE_FAULT, "$task<outside>", &mut args)?);
            let task = a.queue_started(h, &RUST_OUTSIDE_FAULT, &mut cold)?;
            a.pc = 5;
            return Ok(Step::Spawn {
                task,
                slot: 2,
                ty: "$started<outside>",
            });
        }
        if form == 5 {
            a.pc = 4;
            return Ok(Step::Supervise {
                work: &RUST_LEXICAL_WORK,
                handler: &RUST_OUTSIDE_HANDLER,
                arguments: args().0,
            });
        }
        let mut args = vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()];
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
    if a.pc == 5 {
        tick(h, 45)?;
        let mut args = vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()];
        a.pc = 3;
        return Ok(Step::Await {
            task: a.queue_invocation(h, &RUST_PARENT, &mut args)?,
            deliver: rust_wrapper_delivery,
        });
    }
    if a.pc == 1 || a.pc == 2 {
        tick(h, 40 + a.pc)?;
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
    tick(h, if a.pc == 4 { 44 } else { 43 })?;
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
control!(RUST_WRAPPER, "wrapper", 3, &[0, 1], rust_wrapper);

#[repr(C)]
#[derive(Default)]
struct HostFrame {
    pc: u32,
}
fn manifest(m: u32, form: u32) -> Json {
    json!({"mode":m,"wrapper":form,"primitives":[{"name":"tick","async":true,"returns":true,"parameters":[],"outcomes":[{"name":"done","fields":[]}]}]})
}
fn script(m: u32) -> Option<Vec<ScriptCall>> {
    (m >= 13).then(|| {
        vec![ScriptCall {
            operation: "tick".into(),
            outcome: if m == 14 { "invalid" } else { "done" }.into(),
            fields: vec![],
            waits: 3,
        }]
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
control!(HOST_CONTROL, "host", 0, &[], resume_invocation);
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
control!(RUST_HOST, "host", 0, &[], rust_host);

#[test]
fn lexical_scopes_repeat_after_retirement_and_outcome_exits_close_work() {
    exercise(&[10, 11]);
}
#[test]
fn lexical_frames_survive_inline_awaited_spawned_and_cancelled_ancestors() {
    exercise_wrapped(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], &[1, 2, 3, 4]);
}
#[test]
fn lexical_handover_preserves_host_waits_cancellation_and_driver_failures() {
    exercise_wrapped(&[13, 14, 15], &[0, 1, 2, 3, 4]);
}

#[repr(C)]
#[derive(Default)]
struct LexicalWork {
    pc: u32,
    parent: Parent,
}
unsafe fn args_for_named(c: *mut Context<'_>) -> [u64; 2] {
    unsafe {
        let mut values = args().0.into_iter();
        [
            (&mut *c).put(values.next().unwrap()),
            (&mut *c).put(values.next().unwrap()),
        ]
    }
}
unsafe extern "C" fn lexical_work(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<LexicalWork>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !at(c, 50) {
                return 0;
            }
            let values = [take(c, 0), take(c, 1)];
            f.pc = 1;
            tellur_rt_v1_run_child(c, 0, values.as_ptr(), 2);
            return 5;
        }
        assert_eq!(count, 1);
        let mut value = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut value, 1);
        tellur_rt_v1_drop(c, value);
        if !at(c, 51) {
            return 0;
        }
        tellur_rt_v1_settled(c);
        2
    }
}
control!(
    ABI_LEXICAL_WORK,
    "lexical_work",
    2,
    &[0, 1],
    resume_invocation
);
static LEXICAL_WORK: InvocationLayout = InvocationLayout {
    control: &ABI_LEXICAL_WORK,
    callback: lexical_work,
    size: size_of::<LexicalWork>(),
    align: align_of::<LexicalWork>(),
    prefix: offset_of!(LexicalWork, parent),
    children: &[InlineChild {
        offset: offset_of!(LexicalWork, parent),
        layout: &PARENT,
    }],
    scopes: None,
};
fn rust_lexical_work(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, 50)?;
        a.pc = 1;
        return Ok(Step::Call {
            control: &RUST_PARENT,
            arguments: vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()],
            destination: None,
            deliver: rust_wrapper_delivery,
        });
    }
    v[a.base()] = None;
    tick(h, 51)?;
    Ok(Step::Complete(Completion::Settled))
}
control!(
    RUST_LEXICAL_WORK,
    "lexical_work",
    2,
    &[0, 1],
    rust_lexical_work
);
unsafe extern "C" fn outside_handler(
    c: *mut Context<'_>,
    _: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 1);
        store(c, 0, *args);
        if !at(c, 52) {
            return 0;
        }
        let value = take(c, 0);
        tellur_rt_v1_emit(c, value);
        tellur_rt_v1_settled(c);
        2
    }
}
control!(
    ABI_OUTSIDE_HANDLER,
    "outside_handler",
    1,
    &[0],
    resume_invocation
);
static OUTSIDE_HANDLER: InvocationLayout = InvocationLayout {
    control: &ABI_OUTSIDE_HANDLER,
    callback: outside_handler,
    ..HOST
};
fn rust_outside_handler(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    tick(h, 52)?;
    h.emit(v[a.base()].take().unwrap())?;
    Ok(Step::Complete(Completion::Settled))
}
control!(
    RUST_OUTSIDE_HANDLER,
    "outside_handler",
    1,
    &[0],
    rust_outside_handler
);
#[test]
fn named_work_contains_lexical_scopes_and_handles_only_escaping_faults() {
    exercise_wrapped(&[0, 3, 4, 5, 7, 9, 10, 11, 13, 14], &[5]);
}
#[test]
fn lexical_domains_retire_borrowing_siblings_and_set_members_before_handler() {
    exercise_wrapped(&[16, 17], &[0, 1, 2, 3, 4]);
}

unsafe extern "C" fn outside_fault(
    c: *mut Context<'_>,
    p: *mut u8,
    _: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        assert_eq!(count, 0);
        let f = &mut *p.cast::<HostFrame>();
        if f.pc == 0 {
            if !at(c, 60) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        if !at(c, 61) {
            return 0;
        }
        fail(c, "outside owner fault");
        0
    }
}
control!(ABI_OUTSIDE_FAULT, "outside", 0, &[], resume_invocation);
static OUTSIDE_FAULT: InvocationLayout = InvocationLayout {
    control: &ABI_OUTSIDE_FAULT,
    callback: outside_fault,
    ..HOST
};
fn rust_outside_fault(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        tick(h, 60)?;
        a.pc = 1;
        return Ok(Step::Yield);
    }
    tick(h, 61)?;
    fault(h, "outside owner fault")?;
    unreachable!()
}
control!(RUST_OUTSIDE_FAULT, "outside", 0, &[], rust_outside_fault);
#[test]
fn lexical_handler_cannot_intercept_an_outside_owning_domain_fault() {
    exercise_wrapped(&[0, 13], &[6]);
}
