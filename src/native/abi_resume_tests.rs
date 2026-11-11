use super::*;
use std::{cell::RefCell, rc::Rc};

fn selected(host: &mut Host) -> Context<'_> {
    Context {
        host,
        storage: FrameStorage::Owned(Frames::default()),
        frame: None,
        destination: None,
        values: vec![],
        outcomes: vec![],
        value_base: 0,
        outcome_base: 0,
        error: None,
        result: None,
        lexical: vec![],
        next_lexical: 0,
    }
}
fn host() -> Host {
    let mut host = Host::new(json!({}), None, true, Some(20));
    host.enter("caller").unwrap();
    host.contingencies.push("caller context".into());
    host
}
fn observed(host: &Host) -> (Vec<Json>, Option<usize>, Option<u64>, Vec<String>, String) {
    (
        host.events.clone(),
        host.fuel,
        host.resources.current,
        host.contingencies.clone(),
        format!("{:?}", host.activations),
    )
}

#[repr(C)]
#[derive(Default)]
struct ValueFrame {
    pc: u32,
    temporary: u64,
}
unsafe extern "C" fn values_resume(
    c: *mut Context<'_>,
    frame: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    let c = unsafe { &mut *c };
    let f = unsafe { &mut *frame.cast::<ValueFrame>() };
    match f.pc {
        0 => {
            assert_eq!(count, 2);
            let args = unsafe { slice(args, count) };
            unsafe {
                tellur_rt_v1_store(c, 0, std::ptr::null(), 0, args[0]);
                tellur_rt_v1_store(c, 1, std::ptr::null(), 0, args[1]);
                tellur_rt_v1_tick(c, 17, 3, 5);
                f.temporary = tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1);
            }
            f.pc = 1;
            1
        }
        1 => {
            assert_eq!(count, 0);
            assert!(c.get(f.temporary).is_ok());
            let borrowed = unsafe { tellur_rt_v1_local(c, 1, std::ptr::null(), 0, 1) };
            unsafe {
                tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [f.temporary, borrowed].as_ptr(), 2);
            }
            assert!(c.get(f.temporary).is_err());
            f.pc = 2;
            2
        }
        _ => panic!("a completed frame was resumed"),
    }
}
#[test]
fn retained_abi_values_and_borrows_survive_callback_borrows_without_restarting() {
    let mut h = host();
    let mut caller = selected(&mut h);
    unsafe { tellur_rt_v1_frame(&mut caller, 2) };
    let owner = caller.put(V::String("borrowed owner".into()));
    unsafe { tellur_rt_v1_store(&mut caller, 0, std::ptr::null(), 0, owner) };
    let reference = unsafe {
        tellur_rt_v1_reference(
            &mut caller,
            0,
            std::ptr::null(),
            0,
            0,
            b"&String".as_ptr(),
            7,
        )
    };
    let reference = caller.take(reference).unwrap();
    let text = "transferred buffer".repeat(100);
    let original = text.as_ptr();
    let mut frame = ValueFrame::default();
    let ptr = (&mut frame as *mut ValueFrame).cast();
    let mut root = unsafe {
        RetainedRoot::new(
            caller.host,
            caller.storage.values_mut(),
            "producer",
            2,
            vec![V::String(text), reference.clone()],
            &mut frame,
            values_resume,
        )
        .unwrap()
    };
    let before = observed(caller.host);
    assert_eq!(
        root.resume(caller.host, caller.storage.values_mut(), ptr)
            .unwrap(),
        ResumeStatus::Yielded
    );
    assert_eq!(caller.host.fuel, Some(19));
    let after = observed(caller.host);
    assert_eq!(
        (&after.0, &after.2, &after.3, &after.4),
        (
            &before.0,
            &before.2,
            &before.3,
            &before.4.replace("identities: 1", "identities: 2")
        )
    );
    assert_eq!(
        root.resume(caller.host, caller.storage.values_mut(), ptr)
            .unwrap(),
        ResumeStatus::Complete
    );
    assert_eq!(caller.host.fuel, Some(19)); // Resumption did not repeat the source tick.
    let result = root
        .finish(caller.host, caller.storage.values_mut())
        .unwrap()
        .outcome()
        .unwrap();
    assert_eq!(result.0, "done");
    let V::String(value) = &result.1[0] else {
        panic!()
    };
    assert_eq!(value.as_ptr(), original);
    assert_eq!(result.1[1], reference);
    let handle = caller.put(result.1[1].clone());
    unsafe { tellur_rt_v1_store(&mut caller, 1, std::ptr::null(), 0, handle) };
    let read = unsafe { tellur_rt_v1_deref(&mut caller, 1, std::ptr::null(), 0, 0) };
    assert_eq!(
        caller.take(read).unwrap(),
        V::String("borrowed owner".into())
    );
    assert!(caller.host.resources.scopes.is_empty());
    assert_eq!(caller.storage.values().len(), 2);
    drop(root);
    assert_eq!(frame.pc, 2);
}

unsafe extern "C" fn simple_resume(
    c: *mut Context<'_>,
    f: *mut u8,
    _: *const u64,
    count: usize,
) -> u32 {
    assert_eq!(count, 0);
    let f = unsafe { &mut *f.cast::<u32>() };
    *f += 1;
    if *f == 1 {
        return 1;
    }
    unsafe { tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0) };
    2
}
#[test]
fn retained_abi_checks_host_arena_program_frame_and_phase_before_work() {
    let mut h = host();
    let mut foreign = host();
    let mut frames = Frames::default();
    let mut other_frames = Frames::default();
    let mut frame = 0u32;
    let ptr = (&mut frame as *mut u32).cast();
    let mut root = unsafe {
        RetainedRoot::new(
            &mut h,
            &mut frames,
            "producer",
            0,
            vec![],
            &mut frame,
            simple_resume,
        )
        .unwrap()
    };
    let before = observed(&h);
    let other = observed(&foreign);
    assert!(
        root.resume(&mut foreign, &mut frames, ptr)
            .unwrap_err()
            .contains("foreign")
    );
    assert!(
        root.resume(&mut h, &mut other_frames, ptr)
            .unwrap_err()
            .contains("foreign")
    );
    assert!(
        root.resume(&mut h, &mut frames, std::ptr::null_mut())
            .unwrap_err()
            .contains("identity")
    );
    assert!(
        root.finish(&mut h, &mut frames)
            .err()
            .unwrap()
            .contains("not completed")
    );
    assert_eq!(observed(&h), before);
    assert_eq!(observed(&foreign), other);
    assert_eq!(
        root.resume(&mut h, &mut frames, ptr).unwrap(),
        ResumeStatus::Yielded
    );
    assert!(root.finish(&mut h, &mut frames).is_err());
    assert_eq!(
        root.resume(&mut h, &mut frames, ptr).unwrap(),
        ResumeStatus::Complete
    );
    let ready = observed(&h);
    assert!(
        root.resume(&mut h, &mut frames, ptr)
            .unwrap_err()
            .contains("cannot resume")
    );
    assert_eq!(observed(&h), ready);
    root.finish(&mut h, &mut frames).unwrap();
    assert!(
        root.resume(&mut h, &mut frames, ptr)
            .unwrap_err()
            .contains("retired")
    );
    assert!(root.finish(&mut h, &mut frames).is_err());
    assert!(root.stop(&mut h, &mut frames).is_err());
    assert!(frames.is_empty() && h.resources.scopes.is_empty());
    drop(root);
    assert_eq!(frame, 2);
}

#[repr(C)]
struct FailureFrame {
    mode: u32,
    pc: u32,
}
unsafe extern "C" fn failing_resume(
    c: *mut Context<'_>,
    f: *mut u8,
    _: *const u64,
    count: usize,
) -> u32 {
    assert_eq!(count, 0);
    let c = unsafe { &mut *c };
    let f = unsafe { &mut *f.cast::<FailureFrame>() };
    f.pc += 1;
    unsafe { tellur_rt_v1_tick(c, 31, 7, 9) };
    if c.error.is_some() {
        return 0;
    }
    match f.mode {
        0 => 0, // Failure without a diagnostic.
        1 => 2, // Completion without an outcome.
        2 => {
            unsafe { tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0) };
            1 // Cannot both yield and publish completion.
        }
        4 => {
            // A completed callback must close its lexical scopes before adoption.
            unsafe {
                tellur_rt_v1_lexical_begin(c, b"scope".as_ptr(), 5, [1].as_ptr(), 0, 0, 0, 1, 1);
                tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0);
            }
            2
        }
        _ => {
            let message = c.put(V::String("original source failure".into()));
            unsafe { tellur_rt_v1_fault(c, message) };
            let ignored = c.put(V::String("must not emit".into()));
            unsafe { tellur_rt_v1_emit(c, ignored) };
            2 // The original error wins even over a contradictory step tag.
        }
    }
}
#[test]
fn retained_abi_failed_and_malformed_steps_retire_without_replacing_the_cause() {
    for (mode, fuel, message) in [
        (0, 20, "malformed"),
        (1, 20, "malformed"),
        (2, 20, "malformed"),
        (4, 20, "malformed"),
        (3, 20, "original source failure"),
        (3, 0, "host instruction limit"),
    ] {
        let mut h = host();
        h.fuel = Some(fuel);
        let mut frames = Frames::default();
        let mut frame = FailureFrame { mode, pc: 0 };
        let ptr = (&mut frame as *mut FailureFrame).cast();
        let before = format!("{:?}", h.activations);
        let mut root = unsafe {
            RetainedRoot::new(
                &mut h,
                &mut frames,
                "producer",
                2,
                vec![],
                &mut frame,
                failing_resume,
            )
            .unwrap()
        };
        assert_eq!(
            root.resume(&mut h, &mut frames, ptr).unwrap(),
            ResumeStatus::Failed
        );
        let error = root.finish(&mut h, &mut frames).err().unwrap();
        assert!(error.contains(message), "{error}");
        let report = root.failure().unwrap();
        assert_eq!(report.activations[0].control, "producer");
        assert!(report.message.contains(message), "{:?}", report);
        assert_eq!(
            report.kind == fault::Kind::SourceFault,
            mode == 3 && fuel > 0
        );
        // Active caller state is restored; the shared identity allocator advanced
        // for the producer and, in mode 4, its lexical scope. Neither diagnostic
        // id may be reused after retirement.
        assert_eq!(
            format!("{:?}", h.activations),
            before.replace(
                "identities: 1",
                if mode == 4 {
                    "identities: 3"
                } else {
                    "identities: 2"
                }
            )
        );
        assert!(h.events.iter().all(|e| e["kind"] != "emit"));
        assert!(frames.is_empty() && h.resources.scopes.is_empty());
        drop(root);
        assert_eq!(frame.pc, 1);
    }
}

struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
#[repr(C)]
struct OwnerFrame {
    pc: u32,
    return_owner: bool,
}
unsafe extern "C" fn owner_resume(
    c: *mut Context<'_>,
    f: *mut u8,
    args: *const u64,
    count: usize,
) -> u32 {
    let c = unsafe { &mut *c };
    let f = unsafe { &mut *f.cast::<OwnerFrame>() };
    if f.pc == 0 {
        assert_eq!(count, 1);
        unsafe { tellur_rt_v1_store(c, 0, std::ptr::null(), 0, *args) };
        f.pc = 1;
        return 1;
    }
    assert_eq!(count, 0);
    if f.return_owner {
        let value = unsafe { tellur_rt_v1_local(c, 0, std::ptr::null(), 0, 1) };
        unsafe { tellur_rt_v1_complete(c, b"done".as_ptr(), 4, [value].as_ptr(), 1) };
    } else {
        unsafe { tellur_rt_v1_complete(c, b"done".as_ptr(), 4, std::ptr::null(), 0) };
    }
    f.pc = 2;
    2
}
#[test]
fn retained_abi_adopts_owned_results_and_stops_cold_suspended_or_unadopted_work_once() {
    for stop_at in [0, 1, 2, 3] {
        let drops = Rc::new(RefCell::new(vec![]));
        let mut h = host();
        let mut frames = Frames::default();
        let outside = h
            .resources
            .insert("probe", Probe(0, drops.clone()))
            .unwrap();
        let owner = h
            .resources
            .insert("probe", Probe(1, drops.clone()))
            .unwrap();
        let mut frame = OwnerFrame {
            pc: 0,
            return_owner: true,
        };
        let ptr = (&mut frame as *mut OwnerFrame).cast();
        let mut root = unsafe {
            RetainedRoot::new(
                &mut h,
                &mut frames,
                "producer",
                1,
                vec![owner],
                &mut frame,
                owner_resume,
            )
            .unwrap()
        };
        for _ in 0..stop_at.min(2) {
            root.resume(&mut h, &mut frames, ptr).unwrap();
        }
        if stop_at < 3 {
            root.stop(&mut h, &mut frames).unwrap();
            assert!(root.failure().is_none());
            assert_eq!(*drops.borrow(), [1]);
        } else {
            let (_, fields) = root.finish(&mut h, &mut frames).unwrap().outcome().unwrap();
            assert!(drops.borrow().is_empty());
            assert_eq!(h.resources.get::<Probe>(&fields[0], "probe").unwrap().0, 1);
            h.release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
            assert_eq!(*drops.borrow(), [1]);
        }
        assert_eq!(h.resources.get::<Probe>(&outside, "probe").unwrap().0, 0);
        h.release_frame(&[Some(outside)]);
        assert_eq!(*drops.borrow(), [1, 0]);
        assert!(frames.is_empty() && h.resources.scopes.is_empty());
        drop(root);
        assert_eq!(frame.pc, stop_at.min(2));
    }
}
#[test]
fn retained_abi_rejects_unreturned_owners_and_failed_admission_does_not_leak_scopes() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut frames = Frames::default();
    let owner = h
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    let mut frame = OwnerFrame {
        pc: 0,
        return_owner: false,
    };
    // Duplicated consumed owners fail before frame/resource admission.
    let before = observed(&h);
    assert!(
        unsafe {
            RetainedRoot::new(
                &mut h,
                &mut frames,
                "producer",
                1,
                vec![owner.clone(), owner.clone()],
                &mut frame,
                owner_resume,
            )
        }
        .is_err()
    );
    assert_eq!(observed(&h), before);
    assert!(h.resources.scopes.is_empty() && frames.is_empty());
    let ptr = (&mut frame as *mut OwnerFrame).cast();
    let mut root = unsafe {
        RetainedRoot::new(
            &mut h,
            &mut frames,
            "producer",
            1,
            vec![owner],
            &mut frame,
            owner_resume,
        )
        .unwrap()
    };
    root.resume(&mut h, &mut frames, ptr).unwrap();
    root.resume(&mut h, &mut frames, ptr).unwrap();
    assert!(
        root.finish(&mut h, &mut frames)
            .err()
            .unwrap()
            .contains("result ownership")
    );
    assert_eq!(
        root.failure().unwrap().phase,
        fault::Phase::ResultValidation
    );
    assert_eq!(root.failure().unwrap().activations[0].control, "producer");
    assert_eq!(*drops.borrow(), [1]);
    assert!(h.resources.scopes.is_empty() && frames.is_empty());
}

#[test]
fn retained_abi_interleaves_independent_roots_and_retires_only_the_failed_root() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut frames = Frames::default();
    let first = h
        .resources
        .insert("probe", Probe(1, drops.clone()))
        .unwrap();
    let second = h
        .resources
        .insert("probe", Probe(2, drops.clone()))
        .unwrap();
    let mut a_frame = OwnerFrame {
        pc: 0,
        return_owner: false,
    };
    let mut b_frame = OwnerFrame {
        pc: 0,
        return_owner: true,
    };
    let a_ptr = (&mut a_frame as *mut OwnerFrame).cast();
    let b_ptr = (&mut b_frame as *mut OwnerFrame).cast();
    let mut a = unsafe {
        RetainedRoot::new(
            &mut h,
            &mut frames,
            "A",
            1,
            vec![first],
            &mut a_frame,
            owner_resume,
        )
        .unwrap()
    };
    let mut b = unsafe {
        RetainedRoot::new(
            &mut h,
            &mut frames,
            "B",
            1,
            vec![second],
            &mut b_frame,
            owner_resume,
        )
        .unwrap()
    };
    assert_eq!(
        a.resume(&mut h, &mut frames, a_ptr).unwrap(),
        ResumeStatus::Yielded
    );
    assert_eq!(
        b.resume(&mut h, &mut frames, b_ptr).unwrap(),
        ResumeStatus::Yielded
    );
    assert_eq!(
        a.resume(&mut h, &mut frames, a_ptr).unwrap(),
        ResumeStatus::Complete
    );
    assert!(
        a.finish(&mut h, &mut frames)
            .err()
            .unwrap()
            .contains("result ownership")
    );
    assert_eq!(*drops.borrow(), [1]);
    // Reusing A's retired slot must not rebind B's still-live allocation identity.
    let replacement = frames.enter(1).unwrap();
    frames[replacement.base()] = Some(V::String("unrelated".into()));
    assert_eq!(
        b.resume(&mut h, &mut frames, b_ptr).unwrap(),
        ResumeStatus::Complete
    );
    let (_, fields) = b.finish(&mut h, &mut frames).unwrap().outcome().unwrap();
    assert!(b.failure().is_none());
    assert_eq!(h.resources.get::<Probe>(&fields[0], "probe").unwrap().0, 2);
    assert_eq!(
        frames[replacement.base()],
        Some(V::String("unrelated".into()))
    );
    assert_eq!(h.contingencies, ["caller context"]);
    assert!(!h.source_faulted());
    h.release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    frames.retire(&replacement).unwrap();
    assert_eq!(*drops.borrow(), [1, 2]);
    assert!(frames.is_empty() && h.resources.scopes.is_empty());
}
