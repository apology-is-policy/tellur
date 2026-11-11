use super::super::super::abi::*;
use super::*;
use std::mem::{align_of, offset_of, size_of};

#[repr(C)]
#[derive(Default)]
struct Leaf {
    pc: u32,
    fail: u32,
}
#[repr(C)]
#[derive(Default)]
struct Middle {
    pc: u32,
    leaf: Leaf,
}
#[repr(C)]
#[derive(Default)]
struct Root {
    pc: u32,
    middle: Middle,
}

unsafe fn tick(c: *mut Context<'_>, id: usize) -> bool {
    unsafe {
        tellur_rt_v1_tick(c, id, id + 10, 2);
        tellur_rt_v1_ok(c) != 0
    }
}
unsafe fn store(c: *mut Context<'_>, slot: usize, value: u64) {
    unsafe { tellur_rt_v1_store(c, slot, std::ptr::null(), 0, value) }
}
unsafe fn take(c: *mut Context<'_>, slot: usize) -> u64 {
    unsafe { tellur_rt_v1_local(c, slot, std::ptr::null(), 0, 1) }
}
unsafe fn route(c: *mut Context<'_>, args: *const u64, count: usize) -> u64 {
    assert_eq!(count, 1);
    let mut value = 0;
    unsafe {
        tellur_rt_v1_route(c, *args, b"done".as_ptr(), 4, &mut value, 1);
    }
    value
}
unsafe fn complete(c: *mut Context<'_>, value: u64) {
    unsafe {
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1);
    }
}
unsafe extern "C" fn root(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Root>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 1) {
                return 0;
            }
            let borrowed =
                tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
            tellur_rt_v1_await(c, 0, [borrowed].as_ptr(), 1);
            f.pc = 1;
            return 3;
        }
        assert_eq!(f.pc, 1);
        let reference = route(c, args, count);
        store(c, 1, reference);
        if !tick(c, 2) {
            return 0;
        }
        tellur_rt_v1_release_local(c, 1);
        let owner = take(c, 0);
        complete(c, owner);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn middle(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Middle>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 3) {
                return 0;
            }
            let reference = take(c, 0);
            tellur_rt_v1_await(c, 0, [reference].as_ptr(), 1);
            f.pc = 1;
            return 3;
        }
        assert_eq!(f.pc, 1);
        let reference = route(c, args, count);
        store(c, 0, reference);
        if !tick(c, 4) {
            return 0;
        }
        let reference = take(c, 0);
        complete(c, reference);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn leaf(c: *mut Context<'_>, p: *mut u8, args: *const u64, count: usize) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
            if !tick(c, 5) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        assert_eq!(f.pc, 1);
        assert_eq!(count, 0);
        if !tick(c, 6) {
            return 0;
        }
        if f.fail != 0 {
            let message = tellur_rt_v1_value(c, 3, 0, b"leaf fault".as_ptr(), 10);
            tellur_rt_v1_fault(c, message);
            return 0;
        }
        let reference = take(c, 0);
        complete(c, reference);
        f.pc = 2;
        2
    }
}
static ROOT_CONTROL: Control = Control {
    name: "root",
    slots: 2,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static MIDDLE_CONTROL: Control = Control {
    name: "middle",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static LEAF_CONTROL: Control = Control {
    name: "leaf",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static LEAF: InvocationLayout = InvocationLayout {
    control: &LEAF_CONTROL,
    callback: leaf,
    size: size_of::<Leaf>(),
    align: align_of::<Leaf>(),
    prefix: size_of::<Leaf>(),
    children: &[],
};
static MIDDLE: InvocationLayout = InvocationLayout {
    control: &MIDDLE_CONTROL,
    callback: middle,
    size: size_of::<Middle>(),
    align: align_of::<Middle>(),
    prefix: offset_of!(Middle, leaf),
    children: &[InlineChild {
        offset: offset_of!(Middle, leaf),
        layout: &LEAF,
    }],
};
static ROOT: InvocationLayout = InvocationLayout {
    control: &ROOT_CONTROL,
    callback: root,
    size: size_of::<Root>(),
    align: align_of::<Root>(),
    prefix: offset_of!(Root, middle),
    children: &[InlineChild {
        offset: offset_of!(Root, middle),
        layout: &MIDDLE,
    }],
};
fn host(fuel: usize, fail: bool) -> Host {
    Host::new(serde_json::json!({"fail":fail}), None, true, Some(fuel))
}
fn empty(h: &Host) {
    assert!(h.tasks.is_empty() && h.resources.scopes.is_empty() && h.resources.values.is_empty());
}
#[test]
fn abi_inline_children_use_the_existing_source_driver_and_preserve_owned_buffer() {
    for quotas in [vec![1], vec![0, 1, 0, 2], vec![99]] {
        let mut h = host(20, false);
        let text = "owned caller buffer".repeat(50);
        let original = text.as_ptr();
        let mut program = Root::default();
        let mut qs = quotas.iter().cycle();
        let result = unsafe {
            source_scheduler::drive_abi(&mut h, vec![V::String(text)], &ROOT, &mut program, || {
                *qs.next().unwrap()
            })
        }
        .unwrap()
        .outcome()
        .unwrap();
        let V::String(text) = &result.1[0] else {
            panic!()
        };
        assert_eq!(text.as_ptr(), original);
        assert_eq!(
            (program.pc, program.middle.pc, program.middle.leaf.pc),
            (2, 2, 2)
        );
        assert_eq!(h.fuel, Some(14));
        empty(&h);
    }
}
#[test]
fn abi_await_admission_preserves_parent_domain_identity_and_one_use_delivery() {
    let mut h = host(20, false);
    let mut program = Root::default();
    let mut c = Contexts::new(&mut h);
    let root = unsafe { c.start_abi_entry(&ROOT, vec![V::String("origin".into())], &mut program) }
        .unwrap();
    assert!(matches!(
        c.advance_retained(&root, 99).unwrap(),
        ContextPoll::Await
    ));
    let original = c.parked[&root.id].machine.stack.last().unwrap().owner;
    let domain = c.parked[&root.id].machine.current.clone();
    let fuel = c.host.fuel;
    let next = c.next;
    c.next = u64::MAX;
    assert!(c.admit_await(&root).is_err());
    assert_eq!(c.host.fuel, fuel);
    c.next = next;
    let middle = c.admit_await(&root).unwrap();
    assert_eq!(c.parked[&middle.id].parent_invocation, Some(original));
    assert!(
        c.domains
            .same(&domain, &c.parked[&middle.id].machine.current)
    );
    assert!(c.admit_await(&root).is_err());
    assert!(c.advance_retained(&root, 1).is_err());
    assert!(matches!(
        c.advance_retained(&middle, 99).unwrap(),
        ContextPoll::Await
    ));
    let leaf = c.admit_await(&middle).unwrap();
    assert!(matches!(
        c.advance_retained(&leaf, 99).unwrap(),
        ContextPoll::Await
    ));
    assert!(matches!(
        c.parked
            .get_mut(&leaf.id)
            .unwrap()
            .machine
            .source_handoff
            .take(),
        Some(SourceHandoff::Yield)
    ));
    assert!(matches!(
        c.advance_retained(&leaf, 99).unwrap(),
        ContextPoll::Ready
    ));
    assert!(c.return_await(&root, &leaf).is_err());
    c.return_await(&middle, &leaf).unwrap();
    assert!(c.return_await(&middle, &leaf).is_err());
    assert!(matches!(
        c.advance_retained(&middle, 99).unwrap(),
        ContextPoll::Ready
    ));
    c.return_await(&root, &middle).unwrap();
    assert!(matches!(
        c.advance_retained(&root, 99).unwrap(),
        ContextPoll::Ready
    ));
    assert!(matches!(
        c.join(None, &root).unwrap(),
        Completion::Outcome(_)
    ));
    assert!(c.advance_retained(&leaf, 1).is_err());
    assert!(c.values.is_empty() && c.parked.is_empty() && c.completed.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    drop(c);
    empty(&h);
}

fn rust_tick(h: &mut Host, id: usize) -> R<()> {
    h.at(id);
    h.tick(id + 10, 2)
}
fn rust_deliver(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let (name, mut fields) = result.outcome()?;
    assert_eq!(name, "done");
    assert_eq!(fields.len(), 1);
    let slot = if a.control.name == "root" { 1 } else { 0 };
    v[a.base() + slot] = fields.pop();
    Ok(())
}
fn rust_root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        rust_tick(h, 1)?;
        a.pc = 1;
        let mut args = vec![V::Reference {
            ty: "&String".into(),
            root: a.base(),
            fields: vec![],
            evidence: Default::default(),
        }];
        return Ok(Step::Await {
            task: a.queue_invocation(h, &RUST_MIDDLE, &mut args)?,
            deliver: rust_deliver,
        });
    }
    rust_tick(h, 2)?;
    v[a.base() + 1] = None;
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
fn rust_middle(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        rust_tick(h, 3)?;
        a.pc = 1;
        let mut args = vec![v[a.base()].take().unwrap()];
        return Ok(Step::Await {
            task: a.queue_invocation(h, &RUST_LEAF, &mut args)?,
            deliver: rust_deliver,
        });
    }
    rust_tick(h, 4)?;
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
fn rust_leaf(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        rust_tick(h, 5)?;
        a.pc = 1;
        return Ok(Step::Yield);
    }
    rust_tick(h, 6)?;
    if h.manifest["fail"] == true {
        return h.fault(V::String("leaf fault".into()));
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
static RUST_ROOT: Control = Control {
    name: "root",
    slots: 2,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: rust_root,
};
static RUST_MIDDLE: Control = Control {
    name: "middle",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: rust_middle,
};
static RUST_LEAF: Control = Control {
    name: "leaf",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: rust_leaf,
};
fn observed(h: &mut Host, result: R<Completion>) -> serde_json::Value {
    match result {
        Ok(done) => {
            serde_json::json!({"done":done.outcome().unwrap(),"events":h.events,"fuel":h.fuel})
        }
        Err(e) => {
            serde_json::json!({"fault":h.activations.failure("root",super::super::super::fault::Phase::Execution,&e),"events":h.events,"fuel":h.fuel})
        }
    }
}
#[test]
fn abi_nested_faults_and_every_fuel_cut_match_the_existing_rust_source_chain() {
    for fail in [false, true] {
        for fuel in 0..9 {
            let mut baseline = host(fuel, fail);
            let result = source_scheduler::run_source_entry(
                &mut baseline,
                vec![V::String("same".into())],
                &RUST_ROOT,
            );
            let expected = observed(&mut baseline, result);
            empty(&baseline);
            for quotas in [vec![1], vec![0, 1, 0, 3], vec![99]] {
                let mut h = host(fuel, fail);
                let mut f = Root::default();
                f.middle.leaf.fail = u32::from(fail);
                let mut q = quotas.iter().cycle();
                let result = unsafe {
                    source_scheduler::drive_abi(
                        &mut h,
                        vec![V::String("same".into())],
                        &ROOT,
                        &mut f,
                        || *q.next().unwrap(),
                    )
                };
                assert_eq!(
                    observed(&mut h, result),
                    expected,
                    "fail={fail},fuel={fuel},quotas={quotas:?}"
                );
                empty(&h);
            }
        }
    }
}

use std::{cell::RefCell, rc::Rc};
struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
#[repr(C)]
#[derive(Default)]
struct Owners {
    pc: u32,
    leaf: Leaf,
}
unsafe extern "C" fn owner_root(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Owners>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !tick(c, 1) {
                return 0;
            }
            let reference =
                tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&probe".as_ptr(), 6);
            let owner = take(c, 1);
            tellur_rt_v1_await(c, 0, [reference, owner].as_ptr(), 2);
            f.pc = 1;
            return 3;
        }
        assert_eq!(f.pc, 1);
        let owner = route(c, args, count);
        store(c, 1, owner);
        if !tick(c, 2) {
            return 0;
        }
        let owners = [take(c, 0), take(c, 1)];
        tellur_rt_v1_complete(c, b"done".as_ptr(), 4, owners.as_ptr(), 2);
        f.pc = 2;
        2
    }
}
unsafe extern "C" fn owner_leaf(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Leaf>();
        if f.pc == 0 {
            assert_eq!(count, 2);
            store(c, 0, *args);
            store(c, 1, *args.add(1));
            if !tick(c, 5) {
                return 0;
            }
            f.pc = 1;
            return 1;
        }
        assert_eq!(count, 0);
        if !tick(c, 6) {
            return 0;
        }
        if f.fail != 0 {
            let message = tellur_rt_v1_value(c, 3, 0, b"leaf fault".as_ptr(), 10);
            tellur_rt_v1_fault(c, message);
            return 0;
        }
        let owner = take(c, 1);
        complete(c, owner);
        f.pc = 2;
        2
    }
}
static OWNER_ROOT_CONTROL: Control = Control {
    name: "owner",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static OWNER_LEAF_CONTROL: Control = Control {
    name: "borrower",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static OWNER_LEAF: InvocationLayout = InvocationLayout {
    control: &OWNER_LEAF_CONTROL,
    callback: owner_leaf,
    size: size_of::<Leaf>(),
    align: align_of::<Leaf>(),
    prefix: size_of::<Leaf>(),
    children: &[],
};
static OWNERS: InvocationLayout = InvocationLayout {
    control: &OWNER_ROOT_CONTROL,
    callback: owner_root,
    size: size_of::<Owners>(),
    align: align_of::<Owners>(),
    prefix: offset_of!(Owners, leaf),
    children: &[InlineChild {
        offset: offset_of!(Owners, leaf),
        layout: &OWNER_LEAF,
    }],
};
fn owner(h: &mut Host, n: u32, drops: &Rc<RefCell<Vec<u32>>>) -> V {
    h.resources
        .insert("probe", Probe(n, drops.clone()))
        .unwrap()
}
#[test]
fn abi_child_resources_adopt_once_or_retire_before_parent_on_fault_and_stop() {
    for mode in 0..5 {
        let drops = Rc::new(RefCell::new(vec![]));
        let mut h = host(30, false);
        let outside = owner(&mut h, 0, &drops);
        let args = vec![owner(&mut h, 1, &drops), owner(&mut h, 2, &drops)];
        let mut f = Owners::default();
        f.leaf.fail = u32::from(mode == 1);
        if mode < 2 {
            let result = unsafe { source_scheduler::run_abi_entry(&mut h, args, &OWNERS, &mut f) };
            if mode == 0 {
                let (_, mut fields) = result.unwrap().outcome().unwrap();
                assert!(drops.borrow().is_empty());
                h.release_frame(&[Some(fields.pop().unwrap()), Some(fields.pop().unwrap())]);
            } else {
                assert!(result.err().unwrap().contains("leaf fault"));
            }
        } else {
            let mut c = Contexts::new(&mut h);
            let root = unsafe { c.start_abi_entry(&OWNERS, args, &mut f) }.unwrap();
            assert!(matches!(
                c.advance_retained(&root, 99).unwrap(),
                ContextPoll::Await
            ));
            if mode > 2 {
                let child = c.admit_await(&root).unwrap();
                assert!(matches!(
                    c.advance_retained(&child, 99).unwrap(),
                    ContextPoll::Await
                ));
                if mode == 4 {
                    c.parked
                        .get_mut(&child.id)
                        .unwrap()
                        .machine
                        .source_handoff
                        .take();
                    assert!(matches!(
                        c.advance_retained(&child, 99).unwrap(),
                        ContextPoll::Ready
                    ));
                }
            }
            c.terminate().unwrap();
            assert!(c.values.is_empty());
            drop(c);
            assert_eq!(f.pc, 1); // No cancelled/normal continuation is fabricated by stop.
        }
        assert_eq!(*drops.borrow(), [2, 1], "mode={mode}");
        assert!(h.resources.scopes.is_empty() && h.tasks.is_empty());
        h.release_frame(&[Some(outside)]);
        assert_eq!(*drops.borrow(), [2, 1, 0]);
        empty(&h);
    }
}
static BAD_ROOT: InvocationLayout = InvocationLayout {
    control: &ROOT_CONTROL,
    callback: root,
    size: size_of::<Root>(),
    align: align_of::<Root>(),
    prefix: offset_of!(Root, middle),
    children: &[InlineChild {
        offset: 0,
        layout: &MIDDLE,
    }],
};
#[test]
fn abi_layout_rejection_and_foreign_contexts_leave_admission_and_effects_unchanged() {
    let mut h = host(20, false);
    let mut too_small = 0u8;
    let mut bad = Root::default();
    let mut good = Root::default();
    let mut c = Contexts::new(&mut h);
    assert!(
        unsafe { c.start_abi_entry(&ROOT, vec![V::String("owner".into())], &mut too_small) }
            .is_err()
    );
    assert!(
        unsafe { c.start_abi_entry(&BAD_ROOT, vec![V::String("owner".into())], &mut bad) }.is_err()
    );
    assert!(
        c.values.is_empty()
            && c.parked.is_empty()
            && c.host.resources.scopes.is_empty()
            && c.host.tasks.is_empty()
    );
    let key =
        unsafe { c.start_abi_entry(&ROOT, vec![V::String("owner".into())], &mut good) }.unwrap();
    let foreign = ContextKey {
        session: Arc::new(()),
        id: key.id,
    };
    let fuel = c.host.fuel;
    assert!(c.advance_retained(&foreign, 1).is_err());
    assert_eq!(c.host.fuel, fuel);
    c.terminate().unwrap();
    drop(c);
    assert_eq!(good.pc, 0);
    empty(&h);
}

unsafe extern "C" fn repeat_root(
    c: *mut Context<'_>,
    p: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    unsafe {
        let f = &mut *p.cast::<Root>();
        if f.pc == 0 {
            assert_eq!(count, 1);
            store(c, 0, *args);
        } else {
            let reference = route(c, args, count);
            store(c, 1, reference);
        }
        if !tick(
            c,
            if f.pc == 0 {
                1
            } else if f.pc == 1 {
                2
            } else {
                7
            },
        ) {
            return 0;
        }
        if f.pc > 0 {
            tellur_rt_v1_release_local(c, 1);
        }
        if f.pc < 2 {
            // The prior child has delivered and retired before its physical
            // by-value field is initialized for a new invocation.
            f.middle = Middle::default();
            let reference =
                tellur_rt_v1_reference(c, 0, std::ptr::null(), 0, 0, b"&String".as_ptr(), 7);
            tellur_rt_v1_await(c, 0, [reference].as_ptr(), 1);
            f.pc += 1;
            3
        } else {
            let owner = take(c, 0);
            complete(c, owner);
            f.pc = 3;
            2
        }
    }
}
static REPEAT_CONTROL: Control = Control {
    name: "repeat",
    slots: 2,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static REPEAT: InvocationLayout = InvocationLayout {
    control: &REPEAT_CONTROL,
    callback: repeat_root,
    size: size_of::<Root>(),
    align: align_of::<Root>(),
    prefix: offset_of!(Root, middle),
    children: &[InlineChild {
        offset: offset_of!(Root, middle),
        layout: &MIDDLE,
    }],
};
#[test]
fn abi_by_value_storage_can_be_reused_after_the_original_child_delivers() {
    let mut h = host(20, false);
    let mut f = Root::default();
    let result = unsafe {
        source_scheduler::run_abi_entry(&mut h, vec![V::String("twice".into())], &REPEAT, &mut f)
    }
    .unwrap()
    .outcome()
    .unwrap();
    assert_eq!(result.1, vec![V::String("twice".into())]);
    assert_eq!(f.pc, 3);
    assert_eq!(h.fuel, Some(9));
    empty(&h);
}
unsafe extern "C" fn malformed(
    c: *mut Context<'_>,
    p: *mut u8,
    _: *const u64,
    count: usize,
) -> u32 {
    assert_eq!(count, 0);
    unsafe {
        match *p.cast::<u32>() {
            0 => 0,
            1 => 2,
            2 => {
                tellur_rt_v1_frame(c, 0);
                2
            }
            _ => {
                tellur_rt_v1_await(c, 0, std::ptr::null(), 0);
                3
            }
        }
    }
}
static MALFORMED_CONTROL: Control = Control {
    name: "malformed",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    access: None,
    resume: resume_invocation,
};
static MALFORMED: InvocationLayout = InvocationLayout {
    control: &MALFORMED_CONTROL,
    callback: malformed,
    size: size_of::<u32>(),
    align: align_of::<u32>(),
    prefix: size_of::<u32>(),
    children: &[],
};
#[test]
fn abi_malformed_callbacks_retire_the_actual_invocation_without_source_outcome() {
    for (mut mode, message) in [
        (0u32, "malformed"),
        (1, "malformed"),
        (2, "duplicate ABI frame"),
        (3, "unknown inline ABI child"),
    ] {
        let mut h = host(20, false);
        let result =
            unsafe { source_scheduler::run_abi_entry(&mut h, vec![], &MALFORMED, &mut mode) };
        assert!(result.err().unwrap().contains(message));
        assert_eq!(h.fuel, Some(20));
        empty(&h);
    }
}
