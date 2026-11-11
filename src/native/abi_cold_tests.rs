use super::*;
use std::cell::RefCell;
use std::mem::{align_of, offset_of, size_of};
use std::rc::Rc;

#[repr(C)]
#[derive(Default)]
struct Child {
    pc: u32,
}
#[repr(C)]
#[derive(Default)]
struct Parent {
    pc: u32,
    mode: u32,
    child: Child,
    other: Child,
}
unsafe fn tick(c: *mut Context<'_>, id: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, id, id, 1);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe extern "C" fn child(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let frame = &mut *p.cast::<Child>();
        if frame.pc == 0 {
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            if !tick(c, 3) {
                return 0;
            }
            frame.pc = 1;
            return 1;
        }
        assert_eq!((frame.pc, count), (1, 0));
        if !tick(c, 4) {
            return 0;
        }
        let loan = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [loan].as_ptr(), 1);
        frame.pc = 2;
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
            assert_eq!(count, 1);
            tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args);
            if !tick(c, 1) {
                return 0;
            }
            let ty = if f.mode == 3 { "&probe" } else { "&String" };
            let loan = tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, ty.as_ptr(), ty.len());
            let ctx = &mut *c;
            if f.mode == 2 {
                let before = ctx.host.tasks.clone();
                assert!(
                    ctx.defer_control(&CHILD_CONTROL, "$task<child>", &[])
                        .is_err()
                );
                assert!(ctx.get(loan).is_ok());
                assert_eq!(ctx.host.tasks, before);
                ctx.host.tasks.exhaust_task_space();
                let exhausted = ctx.host.tasks.clone();
                assert!(
                    ctx.defer_control(&CHILD_CONTROL, "$task<child>", &[loan])
                        .is_err()
                );
                assert!(ctx.get(loan).is_ok());
                assert_eq!(ctx.host.tasks, exhausted);
                ctx.host.tasks = before;
            }
            let pending = ctx
                .defer_control(&CHILD_CONTROL, "$task<child>", &[loan])
                .unwrap();
            assert!(ctx.get(loan).is_err());
            tellur_rt_v1_store(c, 1, std::ptr::null(), 0, pending);
            assert_eq!(f.child.pc, 0); // capture did not execute the producer
            f.pc = 1;
            return 4;
        }
        if f.pc == 1 {
            assert_eq!(count, 0);
            if !tick(c, 2) {
                return 0;
            }
            if f.mode == 1 || f.mode == 3 {
                let message = tellur_rt_v1_value(c, 3, 0, b"before cold await".as_ptr(), 17);
                tellur_rt_v1_fault(c, message);
                return 0;
            }
            let pending = tellur_rt_v1_local(c, 1, std::ptr::null(), 0, 1);
            if f.mode == 2 {
                let before = (*c).host.tasks.clone();
                for index in [99, 1] {
                    tellur_rt_v1_await_task(c, index, pending);
                    assert!((*c).error.take().is_some());
                    assert!((*c).request.is_none() && (*c).get(pending).is_ok());
                    assert_eq!((*c).host.tasks, before);
                }
                let foreign = Host::new(json!({}), None, true, Some(10));
                assert!(
                    (*c).selected_activation
                        .unwrap()
                        .abi_validate_cold_task(
                            &foreign,
                            &CHILD_CONTROL,
                            (*c).get(pending).unwrap()
                        )
                        .is_err()
                );
                (*c).host.tasks.exhaust_wait_space();
                let exhausted = (*c).host.tasks.clone();
                tellur_rt_v1_await_task(c, 0, pending);
                assert!((*c).error.take().is_some());
                assert!((*c).request.is_none() && (*c).get(pending).is_ok());
                assert_eq!((*c).host.tasks, exhausted);
                (*c).host.tasks = before;
            }
            tellur_rt_v1_await_task(c, 0, pending);
            assert!((*c).get(pending).is_err());
            if f.mode == 2 {
                tellur_rt_v1_await_task(c, 0, pending);
                assert!((*c).error.take().unwrap().contains("duplicate"));
            }
            f.pc = 2;
            return 3;
        }
        assert_eq!((f.pc, count), (2, 1));
        let mut loan = 0;
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut loan, 1);
        tellur_rt_v1_release(c, loan);
        if !tick(c, 5) {
            return 0;
        }
        let owner = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [owner].as_ptr(), 1);
        f.pc = 3;
        2
    }
}
const fn control(name: &'static str, slots: usize) -> execution::Control {
    execution::Control {
        name,
        slots,
        atoms: 0,
        parameters: &[0],
        retained: &[],
        constructor: false,
        access: None,
        resume: resume_invocation,
    }
}
static CHILD_CONTROL: execution::Control = control("child", 1);
static OTHER_CONTROL: execution::Control = control("other", 1);
static PARENT_CONTROL: execution::Control = control("parent", 2);
static CHILD: InvocationLayout = InvocationLayout {
    control: &CHILD_CONTROL,
    callback: child,
    size: size_of::<Child>(),
    align: align_of::<Child>(),
    prefix: size_of::<Child>(),
    children: &[],
};
static OTHER: InvocationLayout = InvocationLayout {
    control: &OTHER_CONTROL,
    callback: child,
    size: size_of::<Child>(),
    align: align_of::<Child>(),
    prefix: size_of::<Child>(),
    children: &[],
};
static PARENT: InvocationLayout = InvocationLayout {
    control: &PARENT_CONTROL,
    callback: parent,
    size: size_of::<Parent>(),
    align: align_of::<Parent>(),
    prefix: offset_of!(Parent, child),
    children: &[
        InlineChild {
            offset: offset_of!(Parent, child),
            layout: &CHILD,
        },
        InlineChild {
            offset: offset_of!(Parent, other),
            layout: &OTHER,
        },
    ],
};
#[test]
fn cold_abi_capture_and_consumption_preserve_identity_fault_fuel_and_host_quotas() {
    for mode in [0, 1, 2] {
        for fuel in 0..=7 {
            let mut expected = None;
            for quota in [1, 3, usize::MAX] {
                let mut host = Host::new(json!({}), None, true, Some(fuel));
                let mut frame = Parent {
                    mode,
                    ..Parent::default()
                };
                let owner = "stable original buffer 😀".repeat(50);
                let pointer = owner.as_ptr();
                let invocation = unsafe { Invocation::new(&PARENT, &mut frame) }.unwrap();
                let result = unsafe {
                    execution::run_abi_invocation(
                        &mut host,
                        vec![V::String(owner)],
                        &PARENT,
                        invocation,
                        quota,
                    )
                };
                let report = match result {
                    Ok(value) => {
                        assert!(mode != 1 && fuel >= 5);
                        let (name, fields) = value.outcome().unwrap();
                        assert_eq!(name, "done");
                        let V::String(value) = &fields[0] else {
                            panic!()
                        };
                        assert_eq!(value.as_ptr(), pointer);
                        assert_eq!((frame.pc, frame.child.pc), (3, 2));
                        format!("done:{value}")
                    }
                    Err(error) => {
                        assert!(mode == 1 || fuel < 5);
                        if mode == 1 {
                            assert_eq!(frame.child.pc, 0);
                        }
                        error
                    }
                };
                let actual = (report, host.fuel, frame.pc, frame.child.pc);
                if let Some(want) = &expected {
                    assert_eq!(&actual, want);
                } else {
                    expected = Some(actual);
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
#[derive(Debug)]
struct Probe(Rc<RefCell<Vec<&'static str>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.0.borrow_mut().push("owner");
    }
}
#[test]
fn cold_abi_fault_retires_unstarted_borrowers_before_the_original_resource_owner() {
    for fuel in 0..=4 {
        let drops = Rc::new(RefCell::new(vec![]));
        let mut host = Host::new(json!({}), None, true, Some(fuel));
        let owner = host
            .resources
            .insert("probe", Probe(drops.clone()))
            .unwrap();
        let mut frame = Parent {
            mode: 3,
            ..Parent::default()
        };
        let result =
            unsafe { execution::run_abi_entry(&mut host, vec![owner], &PARENT, &mut frame) };
        assert!(result.is_err());
        assert_eq!(frame.child.pc, 0);
        assert_eq!(*drops.borrow(), ["owner"]);
        assert!(
            host.tasks.is_empty()
                && host.resources.scopes.is_empty()
                && host.resources.values.is_empty()
        );
    }
}
