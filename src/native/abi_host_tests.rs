use super::*;
use std::mem::{align_of, size_of};

#[repr(C)]
#[derive(Default)]
struct Frame {
    pc: u32,
    mode: u32,
}
const LEAF: HostLeaf = HostLeaf {
    operation: "tick",
    cancellable: true,
};
unsafe extern "C" fn step(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Frame>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            tellur_rt_v1_tick(c, 1, 1, 1);
            if tellur_rt_v1_ok(c) == 0 {
                return 0;
            }
            let input = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 0);
            let context = &mut *c;
            if f.mode == 1 {
                context.host.cancel_entry();
            }
            if f.mode == 2 {
                let cursor = context.host.cursor;
                for handles in [vec![], vec![input, input], vec![0], vec![u64::MAX]] {
                    assert!(context.host_control(&CONTROL, LEAF, &handles).is_err());
                    assert!(context.get(input).is_ok() && context.request.is_none());
                }
                assert!(context.host_control(&OTHER, LEAF, &[input]).is_err());
                for operation in ["missing", "sync", "wrong_arity"] {
                    assert!(
                        context
                            .host_control(&CONTROL, HostLeaf { operation, ..LEAF }, &[input])
                            .is_err()
                    );
                }
                let selected = context.selected_activation.take();
                assert!(context.host_control(&CONTROL, LEAF, &[input]).is_err());
                context.selected_activation = selected;
                let program = context.program.take();
                assert!(context.host_control(&CONTROL, LEAF, &[input]).is_err());
                context.program = program;
                let base = context.outcome_base;
                context.outcome_base = u64::MAX;
                assert!(context.host_control(&CONTROL, LEAF, &[input]).is_err());
                context.outcome_base = base;
                assert!(context.get(input).is_ok() && context.request.is_none());
                assert_eq!(context.host.cursor, cursor);
            }
            context.perform(|c| c.host_control(&CONTROL, LEAF, &[input]));
            assert!(context.get(input).is_err());
            assert!(context.host_control(&CONTROL, LEAF, &[]).is_err());
            f.pc = 1;
            return 9;
        }
        assert_eq!((f.pc, count), (1, 1));
        if f.mode == 1 {
            tellur_rt_v1_route(c, *args, b"cancelled".as_ptr(), 9, std::ptr::null_mut(), 0);
        } else {
            let mut output = 0;
            tellur_rt_v1_route(c, *args, b"ticked".as_ptr(), 6, &mut output, 1);
            tellur_rt_v1_store(c, 1, std::ptr::null(), 0, output);
        }
        tellur_rt_v1_tick(c, 2, 2, 1);
        if tellur_rt_v1_ok(c) == 0 {
            return 0;
        }
        if f.mode == 3 {
            let error = tellur_rt_v1_value(c, 3, 0, b"after host".as_ptr(), 10);
            tellur_rt_v1_fault(c, error);
            return 0;
        }
        let owner = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [owner].as_ptr(), 1);
        f.pc = 2;
        2
    }
}
static CONTROL: execution::Control = execution::Control {
    name: "tick",
    slots: 2,
    parameters: &[0],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static OTHER: execution::Control = execution::Control {
    name: "wrong",
    slots: 2,
    parameters: &[0],
    atoms: 0,
    retained: &[],
    constructor: false,
    resume: resume_invocation,
    access: None,
};
static LAYOUT: InvocationLayout = InvocationLayout {
    scopes: None,
    control: &CONTROL,
    callback: step,
    size: size_of::<Frame>(),
    align: align_of::<Frame>(),
    prefix: size_of::<Frame>(),
    children: &[],
};
fn manifest() -> Json {
    json!({"types":[],"primitives":[
        {"name":"tick","async":true,"returns":true,"parameters":[{"name":"input","type":"String","mode":"borrow"}],"outcomes":[{"name":"ticked","fields":[{"name":"text","type":"String"}]}]},
        {"name":"sync","async":false,"returns":true,"parameters":[{}]},
        {"name":"wrong_arity","async":true,"returns":true,"parameters":[]}
    ]})
}
#[test]
fn host_leaf_preserves_borrowed_owner_and_original_readiness_retirement_across_quotas() {
    for mode in 0..5 {
        for waits in [0, 1, 9] {
            for fuel in 0..=3 {
                let mut expected = None;
                for quota in [1, 3, usize::MAX] {
                    let original = "retained owner 😀".repeat(40);
                    let buffer = original.as_ptr();
                    let calls = if mode == 1 {
                        vec![]
                    } else {
                        vec![ScriptCall {
                            operation: "tick".into(),
                            outcome: "ticked".into(),
                            fields: vec![if mode == 4 {
                                V::Bool(true)
                            } else {
                                V::String("result".into())
                            }],
                            waits,
                        }]
                    };
                    let mut host = Host::new(manifest(), Some(calls), true, Some(fuel));
                    let mut frame = Frame {
                        mode,
                        ..Frame::default()
                    };
                    let invocation = unsafe { Invocation::new(&LAYOUT, &mut frame) }.unwrap();
                    let arena = invocation.arena.clone();
                    let result = unsafe {
                        execution::run_abi_invocation(
                            &mut host,
                            vec![V::String(original)],
                            &LAYOUT,
                            invocation,
                            quota,
                        )
                    };
                    if fuel >= 2 && mode < 3 {
                        assert!(
                            result.is_ok(),
                            "{mode}/{waits}/{fuel}/{quota}: {:?}",
                            result.as_ref().err()
                        );
                    }
                    let report = match result {
                        Ok(value) => {
                            let (outcome, fields) = value.outcome().unwrap();
                            assert_eq!(outcome, "done");
                            let V::String(text) = &fields[0] else {
                                panic!()
                            };
                            assert_eq!(text.as_ptr(), buffer);
                            "done".to_owned()
                        }
                        Err(error) => error,
                    };
                    if fuel >= 2 {
                        assert_eq!(host.cursor, usize::from(mode != 1 && mode != 4));
                        assert_eq!(host.events.len(), usize::from(mode != 1 && mode != 4));
                    }
                    let actual = (
                        report,
                        host.fuel,
                        host.cursor,
                        host.events.clone(),
                        frame.pc,
                    );
                    if let Some(want) = &expected {
                        assert_eq!(&actual, want, "{mode}/{waits}/{fuel}/{quota}");
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
}
