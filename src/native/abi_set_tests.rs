use super::*;
use std::cell::RefCell;
use std::mem::{align_of, size_of};

const SET: &str = "$tasks<set_member>";
const STARTED: &str = "$started<set_member>";
const COLD: &str = "$task<set_member>";
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    mode: u32,
    spawned: u32,
    reaped: u32,
    waits: u32,
    cancels: u32,
    host: bool,
}
#[repr(C)]
#[derive(Default)]
struct Child {
    pc: u32,
    waits: u32,
}
thread_local! {
    static OBSERVED: RefCell<Vec<(String, bool)>> = const { RefCell::new(Vec::new()) };
    static REAPED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
unsafe fn tick(c: *mut Context<'_>, id: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, id, id, 1);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn local(c: *mut Context<'_>, slot: usize) -> u64 {
    unsafe { tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 1) }
}
unsafe fn store(c: *mut Context<'_>, slot: usize, value: u64) {
    unsafe { tellur_rt_v1_store(c, slot, std::ptr::null(), 0, value) }
}
unsafe fn fault(c: *mut Context<'_>, message: &str) {
    unsafe {
        let value = tellur_rt_v1_value(c, 3, 0, message.as_ptr(), message.len());
        tellur_rt_v1_fault(c, value);
    }
}
unsafe fn observe(c: *mut Context<'_>) {
    unsafe {
        let context = &mut *c;
        let label = context.storage.values()[context.local(1).unwrap()]
            .as_ref()
            .unwrap();
        let V::String(label) = label else { panic!() };
        OBSERVED.with(|events| {
            events
                .borrow_mut()
                .push((label.clone(), context.host.task_cancelled()))
        });
    }
}
unsafe extern "C" fn child(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Child>();
        if f.pc == 0 {
            assert_eq!(count, 4);
            for slot in 0..4 {
                store(c, slot, *args.add(slot));
            }
            f.pc = 1;
        }
        if !tick(c, 20) {
            return 0;
        }
        observe(c);
        let context = &mut *c;
        let V::U32(rounds) = context.storage.values()[context.local(2).unwrap()]
            .as_ref()
            .unwrap()
        else {
            panic!()
        };
        if f.waits < *rounds {
            f.waits += 1;
            return 1;
        }
        let fail = local(c, 3);
        if tellur_rt_v1_bool(c, fail) != 0 {
            fault(c, "set child failed");
            return 0;
        }
        let loan = local(c, 0);
        let label = local(c, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [label, loan].as_ptr(), 2);
        2
    }
}
unsafe extern "C" fn host_child(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Child>();
        if f.pc == 0 {
            assert_eq!(count, 4);
            for slot in 0..4 {
                store(c, slot, *args.add(slot));
            }
            if !tick(c, 20) {
                return 0;
            }
            observe(c);
            let inputs = (0..4)
                .map(|slot| {
                    if slot == 0 {
                        tellur_rt_v1_deref(c, slot, std::ptr::null(), 0, 0)
                    } else {
                        tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 0)
                    }
                })
                .collect::<Vec<_>>();
            (&mut *c).perform(|c| {
                c.host_control(
                    &HOST_CONTROL,
                    HostLeaf {
                        operation: "tick",
                        cancellable: true,
                    },
                    &inputs,
                )
            });
            f.pc = 1;
            return 9;
        }
        assert_eq!(count, 1);
        let context = &mut *c;
        let name = context.outcome(*args).unwrap().0.clone();
        assert!(name == "ticked" || name == "cancelled");
        tellur_rt_v1_route(c, *args, name.as_ptr(), name.len(), std::ptr::null_mut(), 0);
        if !tick(c, 21) {
            return 0;
        }
        observe(c);
        let loan = local(c, 0);
        let label = local(c, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [label, loan].as_ptr(), 2);
        2
    }
}
fn invalid_sets(context: &mut Context<'_>, producer: &'static execution::Control, handle: u64) {
    let value = context.get(handle).unwrap().clone();
    let V::Construct { fields, .. } = &value else {
        panic!()
    };
    assert!(!fields.is_empty());
    let mut cases = vec![V::Bool(false)];
    for (ty, name, members) in [
        ("$tasks<other>", "tasks", fields.clone()),
        (SET, "other", fields.clone()),
        (SET, "tasks", vec![fields[0].clone(), fields[0].clone()]),
        (SET, "tasks", vec![fields[0].clone(), V::Bool(false)]),
    ] {
        cases.push(V::Construct {
            ty: ty.into(),
            name: name.into(),
            fields: members,
        });
    }
    for change_type in [false, true] {
        let mut member = fields[0].clone();
        let V::Task { ty, operation, .. } = &mut member else {
            panic!()
        };
        if change_type {
            *ty = "$started<wrong>".into();
        } else {
            *operation = "wrong".into();
        }
        cases.push(V::Construct {
            ty: SET.into(),
            name: "tasks".into(),
            fields: vec![member],
        });
    }
    let before = context.host.tasks.clone();
    let counts = context.physical_arena.unwrap().counts();
    for bad in cases {
        let bad = context.put(bad);
        assert!(context.next_control(producer, SET, STARTED, bad).is_err());
        assert!(
            context
                .cancel_set_control(producer, SET, STARTED, 1, bad)
                .is_err()
        );
        assert!(
            context.get(bad).is_ok() && context.get(handle).is_ok() && context.request.is_none()
        );
        assert_eq!(context.host.tasks, before);
        assert_eq!(context.physical_arena.unwrap().counts(), counts);
        context.take(bad).unwrap();
    }
    assert!(
        context
            .next_control(&OTHER_CONTROL, SET, STARTED, handle)
            .is_err()
    );
    let activation = context.selected_activation.unwrap();
    let foreign = Host::new(json!({}), None, true, None);
    assert!(
        activation
            .abi_validate_task_set(&foreign, producer, SET, STARTED, &value, true)
            .is_err()
    );
    // A different/empty registry cannot make a stale bound member valid.
    context.host.tasks = foreign.tasks.clone();
    assert!(
        context
            .next_control(producer, SET, STARTED, handle)
            .is_err()
    );
    context.host.tasks = before.clone();
    let base = context.outcome_base;
    context.outcome_base = u64::MAX;
    assert!(
        context
            .next_control(producer, SET, STARTED, handle)
            .is_err()
    );
    context.outcome_base = base;
    context.host.tasks.exhaust_wait_space();
    let exhausted = context.host.tasks.clone();
    assert!(
        context
            .next_control(producer, SET, STARTED, handle)
            .is_err()
    );
    assert_eq!(context.host.tasks, exhausted);
    assert!(context.get(handle).is_ok() && context.request.is_none());
    context.host.tasks = before;
}
unsafe extern "C" fn parent(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Parent>();
        let layout = if f.host { &HOST_CHILD } else { &CHILD };
        let producer = layout.control;
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            let context = &mut *c;
            let base = context.value_base;
            context.value_base = u64::MAX;
            assert!(context.empty_task_set(producer, SET, STARTED).is_err());
            context.value_base = base;
            let value = context.empty_task_set(producer, SET, STARTED).unwrap();
            store(c, 1, value);
            f.pc = 1;
        }
        if f.pc == 2 {
            assert_eq!(count, 1);
            let mut fields = [0; 3];
            tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, fields.as_mut_ptr(), 3);
            let context = &mut *c;
            let V::String(label) = context.take(fields[0]).unwrap() else {
                panic!()
            };
            REAPED.with(|events| events.borrow_mut().push(label));
            tellur_rt_v1_release(c, fields[1]);
            store(c, 1, fields[2]);
            f.reaped += 1;
            f.pc = 1;
        }
        if !tick(c, 1) {
            return 0;
        }
        if f.mode == 8 {
            let value = local(c, 1);
            let context = &mut *c;
            context.host.tasks.exhaust_wait_space();
            context.outcome_base = u64::MAX;
            let before = context.host.tasks.clone();
            let physical = context.physical_arena.unwrap().counts();
            if f.cancels == 0 {
                context
                    .cancel_set_control(producer, SET, STARTED, 1, value)
                    .unwrap();
                f.cancels = 1;
                return 8;
            }
            assert!(!context.next_control(producer, SET, STARTED, value).unwrap());
            assert!(context.get(value).is_err() && context.request.is_none());
            assert_eq!(context.host.tasks, before);
            assert_eq!(context.physical_arena.unwrap().counts(), physical);
        } else {
            if (f.mode == 7 && f.spawned == 1 && f.cancels == 0)
                || ((4..=6).contains(&f.mode) && f.spawned == 2 && f.cancels < 2)
            {
                let waits = match f.mode {
                    5 => 2,
                    6 => 12,
                    _ => 0,
                };
                if f.waits < waits {
                    f.waits += 1;
                    return 1;
                }
                let value = local(c, 1);
                let context = &mut *c;
                // Cancelling never allocates completion waits or runs a child.
                let before = context.host.tasks.clone();
                context.host.tasks.exhaust_wait_space();
                let counts = context.physical_arena.unwrap().counts();
                context
                    .cancel_set_control(producer, SET, STARTED, 1, value)
                    .unwrap();
                context.host.tasks = before;
                assert_eq!(context.physical_arena.unwrap().counts(), counts);
                f.cancels += 1;
                return 8;
            }
            if f.spawned < 2 {
                OBSERVED.with(|events| assert!(events.borrow().is_empty()));
                let loan =
                    tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
                let context = &mut *c;
                let label = context.put(V::String(
                    if f.spawned == 0 { "slow" } else { "fast" }.into(),
                ));
                let rounds = context.put(V::U32(if f.spawned == 0 { 3 } else { 0 }));
                let fail = context.put(V::Bool(f.mode == 3 && f.spawned == 1));
                let value = local(c, 1);
                let inputs = [loan, label, rounds, fail];
                let context = &mut *c;
                if f.mode == 9 {
                    let before = context.host.tasks.clone();
                    let counts = context.physical_arena.unwrap().counts();
                    for (slot, captures) in [
                        (0, inputs.to_vec()),
                        (99, inputs.to_vec()),
                        (1, vec![loan]),
                        (1, vec![loan, label, rounds, loan]),
                        (1, vec![loan, label, rounds, value]),
                        (1, vec![loan, label, rounds, u64::MAX]),
                    ] {
                        assert!(
                            context
                                .spawn_into_control(
                                    layout, COLD, STARTED, SET, slot, value, &captures
                                )
                                .is_err()
                        );
                        assert!(
                            inputs.iter().all(|h| context.get(*h).is_ok())
                                && context.get(value).is_ok()
                                && context.request.is_none()
                        );
                        assert_eq!(context.host.tasks, before);
                        assert_eq!(context.physical_arena.unwrap().counts(), counts);
                    }
                }
                context.perform(|c| {
                    c.spawn_into_control(layout, COLD, STARTED, SET, 1, value, &inputs)
                });
                f.spawned += 1;
                return 10;
            }
            if f.mode == 1 || (f.mode == 2 && f.reaped == 1) {
                fault(c, "set parent failed");
                return 0;
            }
            let value = local(c, 1);
            let context = &mut *c;
            if f.mode == 9 && f.reaped == 0 {
                invalid_sets(context, producer, value);
            }
            let V::Construct { fields, .. } = context.get(value).unwrap() else {
                panic!()
            };
            assert_eq!(fields.len(), (2 - f.reaped) as usize);
            let mut pending = false;
            context.perform(|c| {
                pending = c.next_control(producer, SET, STARTED, value)?;
                Ok(())
            });
            if context.error.is_some() {
                return 0;
            }
            if pending {
                f.pc = 2;
                return 11;
            }
        }
        let owner = local(c, 0);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [owner].as_ptr(), 1);
        f.pc = 3;
        2
    }
}
macro_rules! control {
    ($name:ident, $label:literal, $slots:expr, $params:expr) => {
        static $name: execution::Control = execution::Control {
            name: $label,
            slots: $slots,
            parameters: $params,
            atoms: 0,
            retained: &[],
            constructor: false,
            resume: resume_invocation,
            access: None,
        };
    };
}
control!(CHILD_CONTROL, "member", 4, &[0, 1, 2, 3]);
control!(HOST_CONTROL, "host member", 4, &[0, 1, 2, 3]);
control!(OTHER_CONTROL, "other", 4, &[0, 1, 2, 3]);
control!(PARENT_CONTROL, "parent", 2, &[0]);
macro_rules! layout {
    ($name:ident, $control:ident, $callback:ident, $frame:ty) => {
        static $name: InvocationLayout = InvocationLayout {
            control: &$control,
            callback: $callback,
            size: size_of::<$frame>(),
            align: align_of::<$frame>(),
            prefix: size_of::<$frame>(),
            children: &[],
        };
    };
}
layout!(CHILD, CHILD_CONTROL, child, Child);
layout!(HOST_CHILD, HOST_CONTROL, host_child, Child);
layout!(PARENT, PARENT_CONTROL, parent, Parent);
fn manifest() -> Json {
    json!({"types":[], "primitives":[{"name":"tick","async":true,"returns":true,
        "parameters":[{"name":"owner","type":"String","mode":"borrow"},
                      {"name":"label","type":"String","mode":"borrow"},
                      {"name":"rounds","type":"U32","mode":"borrow"},
                      {"name":"fail","type":"Bool","mode":"borrow"}],
        "outcomes":[{"name":"ticked","fields":[]}]}]})
}
fn exercise(modes: &[u32], host_leaf: bool) {
    for &mode in modes {
        for fuel in 0..=40 {
            let mut expected = None;
            for quota in [1, 3, usize::MAX] {
                OBSERVED.with(|v| v.borrow_mut().clear());
                REAPED.with(|v| v.borrow_mut().clear());
                let owner = "task-set caller loan 😀".repeat(40);
                let buffer = owner.as_ptr();
                let mut frame = Parent {
                    mode,
                    host: host_leaf,
                    ..Parent::default()
                };
                let invocation = unsafe { Invocation::new(&PARENT, &mut frame) }.unwrap();
                let arena = invocation.arena.clone();
                let script = host_leaf.then(|| {
                    (if mode == 6 { [0, 0] } else { [3, 0] })
                        .into_iter()
                        .map(|waits| ScriptCall {
                            operation: "tick".into(),
                            outcome: "ticked".into(),
                            fields: vec![],
                            waits,
                        })
                        .collect()
                });
                let mut host = Host::new(manifest(), script, true, Some(fuel));
                let result = unsafe {
                    execution::run_abi_invocation(
                        &mut host,
                        vec![V::String(owner)],
                        &PARENT,
                        invocation,
                        quota,
                    )
                };
                let reaped = REAPED.with(|v| v.borrow().clone());
                let observed = OBSERVED.with(|v| v.borrow().clone());
                if fuel == 40 {
                    if [1, 2, 3].contains(&mode) {
                        assert!(
                            result
                                .as_ref()
                                .err()
                                .is_some_and(|e| e.contains("set parent failed")
                                    || e.contains("set child failed")),
                            "mode={mode}: {:?}",
                            result.as_ref().err()
                        );
                    } else {
                        assert!(
                            result.is_ok(),
                            "mode={mode}/host={host_leaf}: {:?}",
                            result.as_ref().err()
                        );
                        assert_eq!(frame.reaped, if mode == 8 { 0 } else { 2 });
                        if [0, 9].contains(&mode) {
                            assert_eq!(reaped, ["fast", "slow"]);
                        }
                        if mode == 4 {
                            assert!(observed.iter().all(|(_, flag)| *flag));
                        }
                        if mode == 5 {
                            assert!(
                                observed.iter().any(|(_, flag)| *flag)
                                    && observed.iter().any(|(_, flag)| !flag)
                            );
                        }
                        if mode == 6 {
                            assert!(observed.iter().all(|(_, flag)| !flag));
                        }
                        if mode == 7 {
                            assert!(
                                observed
                                    .iter()
                                    .all(|(label, flag)| *flag == (label == "slow"))
                            );
                        }
                    }
                }
                let report = match result {
                    Ok(value) => {
                        let (outcome, fields) = value.outcome().unwrap();
                        assert_eq!(outcome, "done");
                        let V::String(text) = &fields[0] else {
                            panic!()
                        };
                        assert_eq!(text.as_ptr(), buffer);
                        "done".into()
                    }
                    Err(error) => error,
                };
                let actual = (
                    report,
                    host.fuel,
                    host.cursor,
                    host.events.clone(),
                    frame.pc,
                    frame.spawned,
                    frame.reaped,
                    frame.waits,
                    frame.cancels,
                    observed,
                    reaped,
                );
                if let Some(want) = &expected {
                    assert_eq!(&actual, want, "{mode}/{host_leaf}/{fuel}/{quota}");
                } else {
                    expected = Some(actual);
                }
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
#[test]
fn task_sets_preserve_completion_order_loans_and_fault_retirement_across_quotas() {
    exercise(&[0, 1, 2, 3], false);
}
#[test]
fn task_set_cancellation_preserves_obligations_and_only_flags_existing_members() {
    exercise(&[4, 5, 6, 7], false);
}
#[test]
fn task_set_preflight_is_atomic_and_empty_next_needs_no_wait_or_outcome() {
    exercise(&[8, 9], false);
}
#[test]
fn host_task_set_members_share_original_readiness_cancellation_and_remainder_delivery() {
    exercise(&[0, 1, 2, 4, 5, 6, 7, 9], true);
}
#[test]
fn task_set_capacity_failure_retires_queued_frames_without_running_children() {
    for exhaust_wait in [false, true] {
        OBSERVED.with(|v| v.borrow_mut().clear());
        let mut frame = Parent::default();
        let invocation = unsafe { Invocation::new(&PARENT, &mut frame) }.unwrap();
        let arena = invocation.arena.clone();
        let mut host = Host::new(manifest(), None, true, Some(40));
        if exhaust_wait {
            host.tasks.exhaust_wait_space();
        } else {
            host.tasks.exhaust_task_space();
        }
        let result = unsafe {
            execution::run_abi_invocation(
                &mut host,
                vec![V::String("owner".into())],
                &PARENT,
                invocation,
                1,
            )
        };
        assert!(result.err().unwrap().contains("exhausted"));
        OBSERVED.with(|v| assert!(v.borrow().is_empty()));
        assert_eq!(arena.counts().2, 0);
        assert!(
            host.tasks.is_empty()
                && host.resources.scopes.is_empty()
                && host.resources.values.is_empty()
        );
    }
}
