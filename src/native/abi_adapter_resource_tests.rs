use super::*;
use std::{
    cell::Cell,
    ptr::{null, null_mut},
    rc::Rc,
};

struct Owner {
    drops: Rc<Cell<u32>>,
    number: u32,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
unsafe extern "C" fn destroy(value: *mut std::ffi::c_void) {
    drop(unsafe { Box::from_raw(value.cast::<Owner>()) });
}
fn allocation(drops: &Rc<Cell<u32>>) -> ResourceDescriptor {
    ResourceDescriptor {
        version: 1,
        flags: 0,
        value: Box::into_raw(Box::new(Owner {
            drops: drops.clone(),
            number: 42,
        }))
        .cast(),
        drop: Some(destroy),
    }
}
fn manifest() -> Json {
    let mut m = json!({"version":"0.1", "types":[
        {"name":"counter","kind":"opaque","linear":true},
        {"name":"other","kind":"opaque","linear":true}],
        "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"counter":"Counter","other":"Other"}},"primitives":[]});
    for (name, modes, output) in [
        ("make", vec![], "counter"),
        ("inspect", vec!["borrow", "borrow"], "U32"),
        ("handoff", vec!["consume"], "counter"),
        ("close", vec!["consume"], "Unit"),
        ("alias", vec!["consume", "borrow"], "Unit"),
    ] {
        let parameters = modes
            .into_iter()
            .enumerate()
            .map(|(i, mode)| json!({"name":format!("p{i}"),"type":"counter","mode":mode}))
            .collect::<Vec<_>>();
        m["primitives"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":name,"parameters":parameters,
            "outcomes":[{"name":"ready","fields":[{"name":"value","type":output}]}],
            "effects":[],"returns":true,"rust":{"execution":"immediate"}}));
    }
    m
}
fn binding(m: &Json, name: &str, call: AdapterCallback) -> AdapterBinding {
    AdapterBinding::new(
        m,
        name,
        AdapterDescriptor {
            version: 1,
            flags: 0,
            call: Some(call),
        },
    )
    .unwrap()
}
fn context<'a>(resources: &'a mut Resources, owner: &Arc<()>) -> AdapterContext<'a> {
    let manifest = manifest();
    AdapterContext {
        signature: manifest["primitives"][0].clone(),
        manifest,
        values: BTreeMap::new(),
        result: None,
        error: None,
        resources: Some(resources),
        owner: Some(owner.clone()),
        owned: BTreeSet::new(),
    }
}
fn seed(resources: &mut Resources, owner: &Arc<()>, drops: &Rc<Cell<u32>>) -> V {
    let mut c = context(resources, owner);
    let mut descriptor = allocation(drops);
    let handle = unsafe {
        tellur_adapter_v1_resource_publish(&mut c, b"counter".as_ptr(), 7, &mut descriptor)
    };
    assert!(descriptor.value.is_null());
    assert!(c.error.is_none(), "{:?}", c.error);
    unsafe { tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, &handle, 1) };
    c.result.take().unwrap().1.remove(0)
}
unsafe extern "C" fn echo(c: *mut AdapterContext, args: *const u64, count: usize) {
    unsafe { tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, args, count) };
}
unsafe extern "C" fn inspect(c: *mut AdapterContext, args: *const u64, count: usize) {
    assert_eq!(count, 2);
    unsafe {
        let a = tellur_adapter_v1_resource_borrow(c, *args, b"counter".as_ptr(), 7);
        let b = tellur_adapter_v1_resource_borrow(c, *args.add(1), b"counter".as_ptr(), 7);
        assert!(!a.is_null());
        assert_eq!(a, b);
        let number = (*a.cast::<Owner>()).number;
        let h = tellur_adapter_v1_value(c, 2, number.into(), null(), 0);
        tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, &h, 1);
    }
}
unsafe extern "C" fn handoff(c: *mut AdapterContext, args: *const u64, _: usize) {
    unsafe {
        let mut descriptor = std::mem::MaybeUninit::uninit();
        tellur_adapter_v1_resource_take(c, *args, b"counter".as_ptr(), 7, descriptor.as_mut_ptr());
        assert_eq!(tellur_adapter_v1_ok(c), 1);
        let mut descriptor = descriptor.assume_init();
        // The adapter owns the allocation between take and publish. Publication
        // gives the same allocation a new source-visible resource identity.
        let h = tellur_adapter_v1_resource_publish(c, b"counter".as_ptr(), 7, &mut descriptor);
        assert!(descriptor.value.is_null());
        tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, &h, 1);
    }
}
unsafe extern "C" fn close(c: *mut AdapterContext, args: *const u64, _: usize) {
    unsafe {
        let mut descriptor = std::mem::MaybeUninit::uninit();
        tellur_adapter_v1_resource_take(c, *args, b"counter".as_ptr(), 7, descriptor.as_mut_ptr());
        assert_eq!(tellur_adapter_v1_ok(c), 1);
        let descriptor = descriptor.assume_init();
        descriptor.drop.unwrap()(descriptor.value);
        let h = tellur_adapter_v1_value(c, 0, 0, null(), 0);
        tellur_adapter_v1_complete(c, b"ready".as_ptr(), 5, &h, 1);
    }
}
#[test]
fn shared_pointer_identity_owned_handoff_and_non_send_host_teardown() {
    let m = manifest();
    let owner = Arc::new(());
    let drops = Rc::new(Cell::new(0));
    let mut resources = Resources::default();
    let value = seed(&mut resources, &owner, &drops);
    let original = resources
        .get::<ForeignResource>(&value, "counter")
        .unwrap()
        .value;
    let (_, fields) = binding(&m, "inspect", inspect)
        .invoke_with_resources(
            &mut vec![value.clone(), value.clone()],
            Some((&mut resources, &owner)),
        )
        .unwrap();
    assert!(matches!(fields.as_slice(), [V::U32(42)]));
    assert_eq!(drops.get(), 0);
    let (_, mut fields) = binding(&m, "handoff", handoff)
        .invoke_with_resources(&mut vec![value.clone()], Some((&mut resources, &owner)))
        .unwrap();
    let returned = fields.remove(0);
    assert!(resources.get::<ForeignResource>(&value, "counter").is_err());
    assert_eq!(
        resources
            .get::<ForeignResource>(&returned, "counter")
            .unwrap()
            .value,
        original
    );
    binding(&m, "close", close)
        .invoke_with_resources(&mut vec![returned], Some((&mut resources, &owner)))
        .unwrap();
    assert_eq!(drops.get(), 1);
    seed(&mut resources, &owner, &drops);
    drop(resources);
    assert_eq!(drops.get(), 2);
}
#[test]
fn resource_batches_preflight_aliases_types_tokens_and_registry_owners() {
    let m = manifest();
    let owner = Arc::new(());
    let drops = Rc::new(Cell::new(0));
    let mut resources = Resources::default();
    let value = seed(&mut resources, &owner, &drops);
    let mut foreign = Resources::default();
    let other_host = seed(&mut foreign, &owner, &drops);
    for (operation, args, selected_owner) in [
        ("alias", vec![value.clone(), value.clone()], owner.clone()),
        ("handoff", vec![value.clone()], Arc::new(())),
        ("handoff", vec![other_host], owner.clone()),
        (
            "handoff",
            vec![V::Opaque {
                ty: "other".into(),
                token: match &value {
                    V::Opaque { token, .. } => token.clone(),
                    _ => unreachable!(),
                },
            }],
            owner.clone(),
        ),
        (
            "handoff",
            vec![V::Opaque {
                ty: "counter".into(),
                token: "symbolic".into(),
            }],
            owner.clone(),
        ),
    ] {
        let mut args = args;
        let before = serde_json::to_value(&args).unwrap();
        assert!(
            binding(&m, operation, echo)
                .invoke_with_resources(&mut args, Some((&mut resources, &selected_owner)))
                .is_err()
        );
        assert_eq!(serde_json::to_value(args).unwrap(), before);
        assert_eq!(drops.get(), 0);
        assert!(resources.get::<ForeignResource>(&value, "counter").is_ok());
    }
    assert!(binding(&m, "make", echo).invoke(&mut vec![]).is_err());
    assert!(
        binding(&m, "handoff", echo)
            .invoke(&mut vec![value.clone()])
            .is_err()
    );
    binding(&m, "close", close)
        .invoke_with_resources(&mut vec![value.clone()], Some((&mut resources, &owner)))
        .unwrap();
    assert!(
        binding(&m, "handoff", echo)
            .invoke_with_resources(&mut vec![value], Some((&mut resources, &owner)))
            .is_err()
    );
    assert_eq!(drops.get(), 1);
    drop(foreign);
    assert_eq!(drops.get(), 2);
}
#[test]
fn publication_handshake_balances_malformed_failed_and_late_descriptors() {
    for variant in 0..9 {
        let owner = Arc::new(());
        let drops = Rc::new(Cell::new(0));
        let mut resources = Resources::default();
        let mut c = context(&mut resources, &owner);
        let mut d = allocation(&drops);
        let mut ty: &[u8] = b"counter";
        match variant {
            0 => d.version = 0,
            1 => d.flags = 1,
            2 => d.drop = None,
            3 => ty = b"unknown",
            4 => ty = &[255],
            5 => unsafe { tellur_adapter_v1_fail(&mut c, b"prior".as_ptr(), 5) },
            6 => {
                let h = unsafe {
                    tellur_adapter_v1_resource_publish(&mut c, b"counter".as_ptr(), 7, &mut d)
                };
                unsafe { tellur_adapter_v1_complete(&mut c, b"ready".as_ptr(), 5, &h, 1) };
                d = allocation(&drops);
            }
            7 => c.owner = None,
            _ => {}
        }
        let h =
            unsafe { tellur_adapter_v1_resource_publish(&mut c, ty.as_ptr(), ty.len(), &mut d) };
        if variant < 3 || variant == 7 {
            assert!(!d.value.is_null());
            assert_eq!(drops.get(), 0);
            unsafe { destroy(d.value) };
        } else {
            assert!(d.value.is_null());
        }
        if variant == 8 {
            assert_ne!(h, 0);
            assert_eq!(drops.get(), 0);
        } else {
            assert_eq!(h, 0);
            assert!(c.error.is_some());
        }
        drop(c);
        assert!(resources.values.is_empty());
        assert_eq!(
            drops.get(),
            if variant == 6 { 2 } else { 1 },
            "variant {variant}"
        );
    }
    for bad in [null_mut(), 1usize as *mut ResourceDescriptor] {
        let mut resources = Resources::default();
        let owner = Arc::new(());
        let mut c = context(&mut resources, &owner);
        assert_eq!(
            unsafe { tellur_adapter_v1_resource_publish(&mut c, b"counter".as_ptr(), 7, bad) },
            0
        );
        assert!(c.error.is_some());
    }
}
unsafe extern "C" fn take_shared(c: *mut AdapterContext, args: *const u64, _: usize) {
    let mut output = ResourceDescriptor {
        version: 91,
        flags: 92,
        value: null_mut(),
        drop: None,
    };
    unsafe { tellur_adapter_v1_resource_take(c, *args, b"counter".as_ptr(), 7, &mut output) };
    assert_eq!(output.version, 91);
    assert_eq!(output.flags, 92);
    assert!(output.value.is_null());
}
unsafe extern "C" fn incomplete(_: *mut AdapterContext, _: *const u64, _: usize) {}
unsafe extern "C" fn late(c: *mut AdapterContext, args: *const u64, count: usize) {
    unsafe {
        echo(c, args, count);
        tellur_adapter_v1_value(c, 0, 0, null(), 0);
    }
}
#[test]
fn shared_permissions_and_incomplete_or_late_calls_preserve_exact_ownership() {
    let mut m = manifest();
    m["primitives"][1]["outcomes"][0]["fields"][0]["type"] = json!("counter");
    m["primitives"][1]["parameters"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let owner = Arc::new(());
    let drops = Rc::new(Cell::new(0));
    let mut resources = Resources::default();
    let value = seed(&mut resources, &owner, &drops);
    for callback in [take_shared as AdapterCallback, echo, incomplete] {
        assert!(
            binding(&m, "inspect", callback)
                .invoke_with_resources(&mut vec![value.clone()], Some((&mut resources, &owner)))
                .is_err()
        );
        assert_eq!(drops.get(), 0);
        assert!(resources.get::<ForeignResource>(&value, "counter").is_ok());
    }
    for callback in [incomplete as AdapterCallback, late] {
        let value = seed(&mut resources, &owner, &drops);
        assert!(
            binding(&m, "handoff", callback)
                .invoke_with_resources(&mut vec![value.clone()], Some((&mut resources, &owner)))
                .is_err()
        );
        assert!(resources.get::<ForeignResource>(&value, "counter").is_err());
    }
    assert_eq!(drops.get(), 2);
    drop(resources);
    assert_eq!(drops.get(), 3);
}
#[test]
fn take_preflight_and_explicit_discard_never_partially_transfer() {
    for variant in 0..4 {
        let owner = Arc::new(());
        let drops = Rc::new(Cell::new(0));
        let mut resources = Resources::default();
        let mut c = context(&mut resources, &owner);
        let mut d = allocation(&drops);
        let h =
            unsafe { tellur_adapter_v1_resource_publish(&mut c, b"counter".as_ptr(), 7, &mut d) };
        let pointer =
            unsafe { tellur_adapter_v1_resource_borrow(&mut c, h, b"counter".as_ptr(), 7) };
        assert!(!pointer.is_null());
        let mut output = ResourceDescriptor {
            version: 91,
            flags: 92,
            value: null_mut(),
            drop: None,
        };
        unsafe {
            match variant {
                0 => tellur_adapter_v1_resource_take(&mut c, h, b"other".as_ptr(), 5, &mut output),
                1 => tellur_adapter_v1_resource_take(&mut c, h, b"counter".as_ptr(), 7, null_mut()),
                2 => tellur_adapter_v1_resource_take(
                    &mut c,
                    h + 1,
                    b"counter".as_ptr(),
                    7,
                    &mut output,
                ),
                _ => tellur_adapter_v1_drop(&mut c, h),
            }
        }
        assert_eq!(output.version, 91);
        assert_eq!(drops.get(), u32::from(variant == 3));
        drop(c);
        assert_eq!(drops.get(), 1);
        assert!(resources.values.is_empty());
    }
}
#[test]
fn registered_resource_calls_are_launch_local_under_concurrent_execution() {
    std::thread::scope(|scope| {
        let tasks = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let m = manifest();
                    let operations = [
                        ("make", echo as AdapterCallback),
                        ("inspect", inspect),
                        ("handoff", handoff),
                        ("close", close),
                        ("alias", echo),
                    ]
                    .map(|(name, call)| AdapterOperation {
                        name: name.as_ptr(),
                        name_len: name.len(),
                        callback: AdapterDescriptor {
                            version: 1,
                            flags: 0,
                            call: Some(call),
                        },
                    });
                    let table = AdapterTable {
                        version: 1,
                        flags: 0,
                        operations: operations.as_ptr(),
                        count: operations.len(),
                    };
                    let registry = unsafe { AdapterRegistry::prepare(&m, &table) }.unwrap();
                    for _ in 0..5 {
                        let drops = Rc::new(Cell::new(0));
                        let mut host = Host::new(m.clone(), None, true, Some(10));
                        registry.install(&mut host).unwrap();
                        let value = seed(&mut host.resources, &registry.owner, &drops);
                        host.primitive("inspect", vec![value.clone(), value.clone()])
                            .unwrap();
                        let (_, mut fields) = host.primitive("handoff", vec![value]).unwrap();
                        host.primitive("close", vec![fields.remove(0)]).unwrap();
                        assert_eq!(drops.get(), 1);
                        assert_eq!(host.events.len(), 3);
                    }
                })
            })
            .collect::<Vec<_>>();
        for task in tasks {
            task.join().unwrap();
        }
    });
}
