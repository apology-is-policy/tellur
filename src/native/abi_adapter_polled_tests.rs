use super::*;
use std::{cell::RefCell, ptr::null_mut, rc::Rc};

type Trace = Rc<RefCell<Vec<String>>>;
struct Owner {
    name: &'static str,
    trace: Trace,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.trace.borrow_mut().push(self.name.into());
    }
}
unsafe extern "C" fn drop_owner(value: *mut c_void) {
    drop(unsafe { Box::from_raw(value.cast::<Owner>()) });
}
struct State {
    owned: Option<Owner>,
    trace: Trace,
    shared_pointer: usize,
    previous_handle: u64,
    polls: u32,
}
impl Drop for State {
    fn drop(&mut self) {
        self.trace.borrow_mut().push("state".into());
    }
}
unsafe extern "C" fn drop_state(value: *mut c_void) {
    drop(unsafe { Box::from_raw(value.cast::<State>()) });
}
unsafe extern "C" fn poll(state: *mut c_void, c: *mut AdapterContext, args: *const u64, n: usize) {
    assert_eq!(n, 1);
    unsafe {
        let state = &mut *state.cast::<State>();
        let handle = *args;
        assert_ne!(handle, state.previous_handle);
        state.previous_handle = handle;
        let pointer = tellur_adapter_v1_resource_borrow(c, handle, b"counter".as_ptr(), 7);
        assert_eq!(pointer as usize, state.shared_pointer);
        assert_eq!((*pointer.cast::<Owner>()).name, "supplier");
        state.polls += 1;
        state
            .trace
            .borrow_mut()
            .push(format!("poll{}", state.polls));
        if state.polls == 1 {
            tellur_adapter_v1_pending(
                c,
                &WaitDescriptor {
                    version: 1,
                    flags: 3,
                    seconds: 2,
                    nanoseconds: 19,
                    descriptor: 42,
                },
            );
        } else {
            let mut value = ResourceDescriptor {
                version: 1,
                flags: 0,
                value: Box::into_raw(Box::new(state.owned.take().unwrap())).cast(),
                drop: Some(drop_owner),
            };
            let handle = tellur_adapter_v1_resource_publish(c, b"counter".as_ptr(), 7, &mut value);
            assert!(value.value.is_null());
            tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, &handle, 1);
        }
    }
}
unsafe extern "C" fn start(c: *mut AdapterContext, args: *const u64, n: usize) {
    unsafe { start_with_poll(c, args, n, poll) };
}
unsafe fn start_with_poll(
    c: *mut AdapterContext,
    args: *const u64,
    n: usize,
    callback: OperationPoll,
) {
    assert_eq!(n, 2);
    unsafe {
        let mut descriptor = std::mem::MaybeUninit::uninit();
        tellur_adapter_v1_resource_take(
            c,
            *args.add(1),
            b"counter".as_ptr(),
            7,
            descriptor.as_mut_ptr(),
        );
        assert_eq!(tellur_adapter_v1_ok(c), 1);
        let descriptor = descriptor.assume_init();
        let owned = *Box::from_raw(descriptor.value.cast::<Owner>());
        let shared = tellur_adapter_v1_resource_borrow(c, *args, b"counter".as_ptr(), 7);
        let state = State {
            trace: owned.trace.clone(),
            owned: Some(owned),
            shared_pointer: shared as usize,
            previous_handle: *args,
            polls: 0,
        };
        let mut descriptor = OperationDescriptor {
            version: 1,
            flags: 0,
            state: Box::into_raw(Box::new(state)).cast(),
            poll: Some(callback),
            drop: Some(drop_state),
        };
        tellur_adapter_v1_operation_publish(c, &mut descriptor);
        assert!(descriptor.state.is_null());
    }
}
unsafe extern "C" fn late_start(c: *mut AdapterContext, args: *const u64, n: usize) {
    unsafe {
        start(c, args, n);
        tellur_adapter_v1_fail(c, b"late start".as_ptr(), 10);
    }
}
fn manifest() -> Json {
    json!({"version":"0.1", "types":[{"name":"counter","kind":"opaque","linear":true,"fault":"release"}],
        "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"counter":"Counter"}},
        "primitives":[{"name":"step","async":true,"returns":true,"rust":{"execution":"poll"},
            "parameters":[{"name":"supplier","type":"counter","mode":"borrow"},{"name":"owned","type":"counter","mode":"consume"}],
            "outcomes":[{"name":"ready","fields":[{"name":"value","type":"counter"}]}]}]})
}
fn binding(call: AdapterCallback) -> PolledBinding {
    PolledBinding::new(
        &manifest(),
        "step",
        AdapterDescriptor {
            version: 1,
            flags: 0,
            call: Some(call),
        },
    )
    .unwrap()
}
fn fixture() -> (Resources, Arc<()>, Vec<V>, Trace) {
    fixture_for(Arc::new(()))
}
fn fixture_for(owner: Arc<()>) -> (Resources, Arc<()>, Vec<V>, Trace) {
    let mut resources = Resources::default();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let args = ["supplier", "consumed"]
        .into_iter()
        .map(|name| {
            resources
                .insert(
                    "counter",
                    ForeignResource {
                        value: Box::into_raw(Box::new(Owner {
                            name,
                            trace: trace.clone(),
                        }))
                        .cast(),
                        drop: drop_owner,
                        owner: owner.clone(),
                    },
                )
                .unwrap()
        })
        .collect();
    (resources, owner, args, trace)
}

#[test]
fn polled_owners_use_fresh_shared_handles_and_retire_after_readiness() {
    let (mut resources, owner, mut args, trace) = fixture();
    let supplier = args[0].clone();
    let consumed = args[1].clone();
    let mut operation = binding(start)
        .start(&mut args, &mut resources, &owner)
        .unwrap();
    assert!(args.is_empty());
    assert!(
        resources
            .get::<ForeignResource>(&consumed, "counter")
            .is_err()
    );
    assert!(operation.poll(&mut Resources::default()).is_err());
    assert!(trace.borrow().is_empty());
    let HostProgress::Pending { delay, descriptor } = operation.poll(&mut resources).unwrap()
    else {
        panic!()
    };
    assert_eq!(delay, Some(Duration::new(2, 19)));
    assert_eq!(
        descriptor,
        Some(Descriptor {
            fd: 42,
            writable: false
        })
    );
    let HostProgress::Ready((name, mut fields)) = operation.poll(&mut resources).unwrap() else {
        panic!()
    };
    assert_eq!(name, "ready");
    assert_eq!(*trace.borrow(), ["poll1", "poll2"]);
    assert!(operation.poll(&mut resources).is_err());
    // HostSource owns this boundary: deregister before destroying adapter state.
    trace.borrow_mut().push("deregister".into());
    drop(operation);
    assert_eq!(*trace.borrow(), ["poll1", "poll2", "deregister", "state"]);
    let returned = fields.remove(0);
    drop(
        resources
            .take::<ForeignResource>(&returned, "counter")
            .unwrap(),
    );
    drop(
        resources
            .take::<ForeignResource>(&supplier, "counter")
            .unwrap(),
    );
    assert_eq!(
        *trace.borrow(),
        [
            "poll1",
            "poll2",
            "deregister",
            "state",
            "consumed",
            "supplier"
        ]
    );

    let (mut resources, owner, mut args, trace) = fixture();
    let mut operation = binding(start)
        .start(&mut args, &mut resources, &owner)
        .unwrap();
    assert!(matches!(
        operation.poll(&mut resources),
        Ok(HostProgress::Pending { .. })
    ));
    drop(operation);
    assert_eq!(*trace.borrow(), ["poll1", "state", "consumed"]);
    drop(resources);
    assert_eq!(*trace.borrow(), ["poll1", "state", "consumed", "supplier"]);
}

#[test]
fn polled_start_preflight_preserves_owners_and_failed_publication_drops_state() {
    let (mut resources, owner, args, trace) = fixture();
    let mut alias = vec![args[0].clone(), args[0].clone()];
    assert!(
        binding(start)
            .start(&mut alias, &mut resources, &owner)
            .is_err()
    );
    assert_eq!(alias.len(), 2);
    let mut wrong_owner = args.clone();
    assert!(
        binding(start)
            .start(&mut wrong_owner, &mut resources, &Arc::new(()))
            .is_err()
    );
    assert_eq!(wrong_owner.len(), 2);
    assert!(trace.borrow().is_empty());
    let mut args = args;
    assert!(
        binding(late_start)
            .start(&mut args, &mut resources, &owner)
            .is_err()
    );
    assert!(args.is_empty());
    assert_eq!(*trace.borrow(), ["state", "consumed"]);
    drop(resources);
    assert_eq!(*trace.borrow(), ["state", "consumed", "supplier"]);
    let mut m = manifest();
    m["types"][0]["fault"] = Json::Null;
    assert!(
        PolledBinding::new(
            &m,
            "step",
            AdapterDescriptor {
                version: 1,
                flags: 0,
                call: Some(start)
            }
        )
        .is_err()
    );
    m = manifest();
    m["primitives"][0]["parameters"][0]["type"] = json!("String");
    assert!(
        PolledBinding::new(
            &m,
            "step",
            AdapterDescriptor {
                version: 1,
                flags: 0,
                call: Some(start)
            }
        )
        .is_err()
    );
}

fn context() -> AdapterContext<'static> {
    AdapterContext {
        manifest: manifest(),
        signature: manifest()["primitives"][0].clone(),
        values: BTreeMap::new(),
        result: None,
        error: None,
        resources: None,
        owner: None,
        owned: BTreeSet::new(),
        progress: Progress::Starting(None),
    }
}
fn allocation(trace: &Trace) -> OperationDescriptor {
    OperationDescriptor {
        version: 1,
        flags: 0,
        state: Box::into_raw(Box::new(State {
            owned: None,
            trace: trace.clone(),
            shared_pointer: 0,
            previous_handle: 0,
            polls: 0,
        }))
        .cast(),
        poll: Some(poll),
        drop: Some(drop_state),
    }
}
#[test]
fn operation_descriptor_adoption_is_unambiguous_on_malformed_duplicate_and_late_calls() {
    for mode in 0..4 {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut c = context();
        let mut descriptor = allocation(&trace);
        match mode {
            0 => descriptor.version = 2,
            1 => descriptor.flags = 1,
            2 => descriptor.poll = None,
            _ => descriptor.drop = None,
        }
        unsafe { tellur_adapter_v1_operation_publish(&mut c, &mut descriptor) };
        assert!(c.error.is_some());
        assert!(!descriptor.state.is_null());
        assert!(trace.borrow().is_empty());
        unsafe { drop_state(descriptor.state) };
        assert_eq!(*trace.borrow(), ["state"]);
    }
    for mode in 0..3 {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut c = context();
        if mode == 0 {
            c.error = Some("prior".into());
        }
        if mode == 1 {
            c.progress = Progress::Polling(None);
        }
        if mode == 2 {
            let mut first = allocation(&trace);
            unsafe { tellur_adapter_v1_operation_publish(&mut c, &mut first) };
            assert!(first.state.is_null());
        }
        let mut descriptor = allocation(&trace);
        unsafe { tellur_adapter_v1_operation_publish(&mut c, &mut descriptor) };
        assert!(descriptor.state.is_null());
        assert!(c.error.is_some());
        assert_eq!(trace.borrow().len(), 1);
        drop(c);
        assert_eq!(trace.borrow().len(), if mode == 2 { 2 } else { 1 });
    }
    let mut c = context();
    unsafe { tellur_adapter_v1_operation_publish(&mut c, null_mut()) };
    assert!(c.error.is_some());
    let mut c = context();
    unsafe { tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, std::ptr::null(), 0) };
    assert!(c.error.as_ref().unwrap().contains("start must publish"));
}

#[test]
fn pending_waits_validate_flags_units_interests_and_callback_phase() {
    let valid = WaitDescriptor {
        version: 1,
        flags: 1,
        seconds: 0,
        nanoseconds: 1,
        descriptor: 0,
    };
    for flags in [1, 2, 3, 4, 5] {
        let mut c = context();
        c.progress = Progress::Polling(None);
        let wait = WaitDescriptor {
            flags,
            nanoseconds: if flags & 1 != 0 { 19 } else { 0 },
            descriptor: if flags & 6 != 0 { 7 } else { 0 },
            ..valid
        };
        unsafe { tellur_adapter_v1_pending(&mut c, &wait) };
        assert!(c.error.is_none());
        assert!(c.progress.finished());
        unsafe { tellur_adapter_v1_pending(&mut c, &wait) };
        assert!(c.error.is_some());
    }
    for wait in [
        WaitDescriptor {
            version: 2,
            ..valid
        },
        WaitDescriptor { flags: 0, ..valid },
        WaitDescriptor { flags: 8, ..valid },
        WaitDescriptor { flags: 6, ..valid },
        WaitDescriptor {
            nanoseconds: 1_000_000_000,
            ..valid
        },
        WaitDescriptor {
            descriptor: 1,
            ..valid
        },
        WaitDescriptor {
            flags: 2,
            seconds: 1,
            nanoseconds: 0,
            descriptor: 7,
            ..valid
        },
        WaitDescriptor {
            flags: 2,
            nanoseconds: 0,
            descriptor: -1,
            ..valid
        },
    ] {
        let mut c = context();
        c.progress = Progress::Polling(None);
        unsafe { tellur_adapter_v1_pending(&mut c, &wait) };
        assert!(c.error.is_some());
        assert!(!c.progress.finished());
    }
    let mut c = context();
    unsafe { tellur_adapter_v1_pending(&mut c, &valid) };
    assert!(c.error.as_ref().unwrap().contains("requires a poll"));
}

#[test]
fn polled_contract_keeps_existing_data_resource_and_cancellation_restrictions() {
    let descriptor = AdapterDescriptor {
        version: 1,
        flags: 0,
        call: Some(start),
    };
    for mode in 0..6 {
        let mut m = manifest();
        match mode {
            0 => m["primitives"][0]["contingent"] = json!(true),
            1 => m["primitives"][0]["returns"] = json!(false),
            2 => m["primitives"][0]["outcomes"][0]["name"] = json!("cancelled"),
            3 => {
                m["primitives"][0]["parameters"] = json!([]);
                m["types"][0]["fault"] = Json::Null;
            }
            4 => {
                m["primitives"][0]["parameters"] =
                    json!([{"name":"text","type":"String","mode":"value"}])
            }
            _ => m["primitives"][0]["async"] = json!(false),
        }
        assert!(
            PolledBinding::new(&m, "step", descriptor).is_err(),
            "mode={mode}"
        );
    }
    let mut m = manifest();
    m["primitives"][0]["parameters"] = json!([{"name":"text","type":"String","mode":"consume"},{"name":"number","type":"U32","mode":"value"}]);
    assert!(PolledBinding::new(&m, "step", descriptor).is_ok());
    assert!(AdapterBinding::new(&m, "step", descriptor).is_err());
}

/// Drive the same Host operation and retained machine as generated controls. Only
/// descriptor readiness is simulated; the registry, cancellation, event and drop
/// paths here are the runtime's own.
#[test]
fn retained_host_deregisters_foreign_wait_before_ready_or_cancelled_destruction() {
    use execution::{Activation, Control, HostWait, Machine, Poll, Step};

    struct Readiness(Trace);
    impl ReadinessBackend for Readiness {
        fn register(&mut self, token: u64, fd: Descriptor) -> R<()> {
            assert_eq!((token, fd.fd, fd.writable), (1, 42, false));
            self.0.borrow_mut().push("register".into());
            Ok(())
        }
        fn deregister(&mut self, fd: i32) -> R<()> {
            assert_eq!(fd, 42);
            self.0.borrow_mut().push("deregister".into());
            Ok(())
        }
        fn wait(&mut self, _: Option<Duration>) -> R<Vec<u64>> {
            unreachable!("the test supplies the token")
        }
    }
    fn deliver(a: &mut Activation, v: &mut Frames, name: String, mut fields: Vec<V>) -> R<()> {
        match name.as_str() {
            "ready" if fields.len() == 1 => {
                v[a.base() + 1] = fields.pop();
                a.pc = 1;
            }
            "cancelled" if fields.is_empty() => a.pc = 2,
            _ => panic!("unexpected delivery: {name}"),
        }
        Ok(())
    }
    fn resume(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
        let base = a.base();
        Ok(match a.pc {
            0 => Step::Host {
                operation: "step",
                arguments: vec![
                    v[base].as_ref().unwrap().clone(),
                    v[base + 1].take().unwrap(),
                ],
                deliver,
                cancellable: true,
            },
            1 => Step::Complete(Completion::Outcome((
                "ready".into(),
                vec![v[base + 1].take().unwrap()],
            ))),
            2 => Step::Complete(Completion::Outcome(("cancelled".into(), vec![]))),
            _ => unreachable!(),
        })
    }
    static CONTROL: Control = Control {
        name: "poll_fixture",
        slots: 2,
        atoms: 0,
        parameters: &[0, 1],
        retained: &[],
        constructor: false,
        access: None,
        resume,
    };
    unsafe extern "C" fn excessive(
        _: *mut c_void,
        c: *mut AdapterContext,
        _: *const u64,
        _: usize,
    ) {
        unsafe {
            tellur_adapter_v1_pending(
                c,
                &WaitDescriptor {
                    version: 1,
                    flags: 1,
                    seconds: u64::MAX,
                    nanoseconds: 0,
                    descriptor: 0,
                },
            );
        }
    }
    unsafe extern "C" fn start_excessive(c: *mut AdapterContext, a: *const u64, n: usize) {
        unsafe { start_with_poll(c, a, n, excessive) };
    }
    for (cancel, huge_delay) in [(false, false), (true, false), (false, true)] {
        let registry = registry(if huge_delay { start_excessive } else { start });
        let (resources, _owner, args, trace) = fixture_for(registry.owner.clone());
        let mut host = Host::new(manifest(), None, true, None);
        host.resources = resources;
        registry.install(&mut host).unwrap();
        host.readiness = Some(Box::new(Readiness(trace.clone())));
        let mut frames = Frames::default();
        let mut domains = Default::default();
        let mut machine =
            Machine::start(&mut host, &mut frames, &mut domains, &CONTROL, args).unwrap();
        if huge_delay {
            let failure = machine.advance(&mut host, &mut frames, &mut domains, 10);
            assert!(matches!(failure, Err(e) if e.contains("delay exceeds the host clock range")));
            assert!(host.events.is_empty());
            drop(host);
            assert_eq!(*trace.borrow(), ["state", "consumed", "supplier"]);
            continue;
        }
        assert!(matches!(
            machine
                .advance(&mut host, &mut frames, &mut domains, 10)
                .unwrap(),
            Poll::Paused
        ));
        assert!(matches!(
            machine.host_waiting(),
            Some(HostWait::Readable {
                token: 1,
                at: Some(_)
            })
        ));
        assert_eq!(*trace.borrow(), ["poll1", "register"]);
        assert!(host.events.is_empty());
        if cancel {
            host.cancel_entry();
        } else {
            machine.host_readable(&[2]);
            assert!(matches!(
                machine.host_waiting(),
                Some(HostWait::Readable { .. })
            ));
            machine.host_readable(&[1]);
            assert!(matches!(machine.host_waiting(), Some(HostWait::Due)));
        }
        machine.host_ready(false);
        let Poll::Complete(result) = machine
            .advance(&mut host, &mut frames, &mut domains, 10)
            .unwrap()
        else {
            panic!("missing completion")
        };
        let (name, fields) = result.outcome().unwrap();
        assert_eq!(name, if cancel { "cancelled" } else { "ready" });
        assert_eq!(fields.len(), usize::from(!cancel));
        let expected = if cancel {
            vec!["poll1", "register", "deregister", "state", "consumed"]
        } else {
            assert_eq!(host.events[0]["outcome"], "ready");
            assert!(
                host.resources
                    .get::<ForeignResource>(&fields[0], "counter")
                    .is_ok()
            );
            vec!["poll1", "register", "poll2", "deregister", "state"]
        };
        assert_eq!(host.events.len(), usize::from(!cancel));
        assert_eq!(&trace.borrow()[..expected.len()], expected.as_slice());
        drop(host);
        let t = trace.borrow();
        for name in ["supplier", "consumed", "state", "deregister"] {
            assert_eq!(t.iter().filter(|item| *item == name).count(), 1, "{t:?}");
        }
    }
}

fn registry(call: AdapterCallback) -> Arc<AdapterRegistry> {
    let op = AdapterOperation {
        name: b"step".as_ptr(),
        name_len: 4,
        callback: AdapterDescriptor {
            version: 1,
            flags: 1,
            call: Some(call),
        },
    };
    let table = AdapterTable {
        version: 2,
        flags: 0,
        operations: &op,
        count: 1,
    };
    unsafe { AdapterRegistry::prepare(&manifest(), &table) }.unwrap()
}

#[test]
fn stale_supplier_fails_before_foreign_poll_and_terminal_state_retires_once() {
    let (mut resources, owner, mut args, trace) = fixture();
    let supplier = args[0].clone();
    let mut op = binding(start)
        .start(&mut args, &mut resources, &owner)
        .unwrap();
    assert!(matches!(
        op.poll(&mut resources),
        Ok(HostProgress::Pending { .. })
    ));
    let held = resources
        .take::<ForeignResource>(&supplier, "counter")
        .unwrap();
    assert!(matches!(op.poll(&mut resources), Err(e) if e.contains("consumed resource handle")));
    assert!(matches!(op.poll(&mut resources), Err(e) if e.contains("finished operation")));
    drop(op);
    drop(held);
    assert_eq!(*trace.borrow(), ["poll1", "state", "consumed", "supplier"]);
}

#[test]
fn failed_poll_retires_once_without_publishing_a_ready_owner() {
    unsafe extern "C" fn silent(_: *mut c_void, _: *mut AdapterContext, _: *const u64, _: usize) {}
    unsafe extern "C" fn late(state: *mut c_void, c: *mut AdapterContext, a: *const u64, n: usize) {
        // Select the ready path, publish the returned owner, then violate the protocol.
        unsafe {
            (*state.cast::<State>()).polls = 1;
            poll(state, c, a, n);
            tellur_adapter_v1_fail(c, b"late poll".as_ptr(), 9);
        }
    }
    unsafe extern "C" fn start_silent(c: *mut AdapterContext, a: *const u64, n: usize) {
        unsafe { start_with_poll(c, a, n, silent) };
    }
    unsafe extern "C" fn start_late(c: *mut AdapterContext, a: *const u64, n: usize) {
        unsafe { start_with_poll(c, a, n, late) };
    }
    for (call, expected, before, after) in [
        (
            start_silent as AdapterCallback,
            "without progress",
            vec![],
            vec!["state", "consumed", "supplier"],
        ),
        (
            start_late as AdapterCallback,
            "used a completed invocation",
            vec!["poll2", "consumed"],
            vec!["poll2", "consumed", "state", "supplier"],
        ),
    ] {
        let (mut resources, owner, mut args, trace) = fixture();
        let mut op = binding(call)
            .start(&mut args, &mut resources, &owner)
            .unwrap();
        assert!(matches!(op.poll(&mut resources), Err(e) if e.contains(expected)));
        assert!(matches!(op.poll(&mut resources), Err(e) if e.contains("finished operation")));
        assert_eq!(trace.borrow().as_slice(), before);
        assert_eq!(resources.values.len(), 1);
        drop(op);
        drop(resources);
        assert_eq!(trace.borrow().as_slice(), after);
    }
}
