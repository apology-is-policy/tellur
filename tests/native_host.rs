#[allow(dead_code)]
#[path = "../src/native/runtime.rs"]
mod host;
use host::{Host, V};
use serde_json::{Value as Json, json};
#[test]
fn owned_projection_transfers_the_existing_buffer() {
    let text = "owned buffer".repeat(1024);
    let allocation = text.as_ptr();
    let record = V::Construct {
        ty: "message".into(),
        name: "message".into(),
        fields: vec![V::U32(7), V::String(text), V::Unit],
    };
    let V::String(selected) = host::project_owned(record, 1).unwrap() else {
        panic!("expected the string field");
    };
    assert_eq!(selected.as_ptr(), allocation);
    assert_eq!(selected, "owned buffer".repeat(1024));
    assert!(host::project_owned(V::Unit, 0).is_err());
}

#[test]
fn constructor_delivery_is_separate_from_outcomes_and_settlement() {
    use host::Completion;
    assert!(Completion::Constructed.constructed().is_ok());
    assert!(Completion::Constructed.outcome().is_err());
    assert!(Completion::Constructed.settled().is_err());
    for completion in [
        Completion::Outcome(("done".into(), vec![])),
        Completion::Settled,
        Completion::Fallthrough,
        Completion::Process(23),
    ] {
        assert!(completion.constructed().is_err());
    }
}
fn manifest() -> Json {
    serde_json::to_value(
        tellur::manifest::Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap()
}
#[test]
fn live_contracts_are_checked_before_dispatch() {
    let mut m = manifest();
    m["primitives"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == "parse_u32")
        .unwrap()["effects"] = json!(["unexpected"]);
    let mut h = Host::new(m, None, true, None);
    assert!(
        h.primitive("parse_u32", vec![V::String("42".into())])
            .unwrap_err()
            .contains("E_HOST")
    );
    let mut m = manifest();
    m["types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["name"] == "handle")
        .unwrap()["kind"] = json!("record");
    let mut h = Host::new(m, None, true, None);
    assert!(
        h.primitive("open", vec![V::String("anything".into())])
            .unwrap_err()
            .contains("type contract")
    );
}
#[test]
fn live_arithmetic_and_parsing_boundary_cases() {
    let mut h = Host::new(manifest(), None, true, None);
    for (input, out) in [
        ("0", "number"),
        ("00042", "number"),
        ("4294967295", "number"),
        ("4294967296", "overflow"),
        ("", "malformed"),
        (" 1", "malformed"),
        ("+1", "malformed"),
        ("-1", "malformed"),
        ("١", "malformed"),
    ] {
        assert_eq!(
            h.primitive("parse_u32", vec![V::String(input.into())])
                .unwrap()
                .0,
            out
        );
    }
    assert_eq!(
        h.primitive("checked_sub", vec![V::U32(0), V::U32(1)])
            .unwrap()
            .0,
        "underflow"
    );
    assert_eq!(
        h.primitive("checked_mul", vec![V::U32(u32::MAX), V::U32(2)])
            .unwrap()
            .0,
        "overflow"
    );
    assert_eq!(
        h.primitive("checked_div", vec![V::U32(1), V::U32(0)])
            .unwrap()
            .0,
        "zero_divisor"
    );
}
#[test]
fn invalid_path_has_declared_outcome_and_forged_handle_is_host_error() {
    let mut h = Host::new(manifest(), None, true, None);
    assert_eq!(
        h.primitive("open", vec![V::String("invalid\0path".into())])
            .unwrap()
            .0,
        "denied"
    );
    assert!(
        h.primitive(
            "close",
            vec![V::Opaque {
                ty: "handle".into(),
                token: "forged".into()
            }]
        )
        .unwrap_err()
        .contains("E_HOST")
    );
}
#[test]
fn malformed_script_payload_and_exhaustion_are_rejected() {
    let calls=serde_json::from_value(json!([{"operation":"parse_u32","outcome":"number","fields":[{"kind":"string","value":"wrong"}]}])).unwrap();
    let mut h = Host::new(manifest(), Some(calls), true, None);
    assert!(
        h.primitive("parse_u32", vec![V::String("1".into())])
            .unwrap_err()
            .contains("E_DRIVER")
    );
    assert!(
        h.primitive("parse_u32", vec![V::String("1".into())])
            .unwrap_err()
            .contains("exhausted")
    );
}

#[test]
fn retained_resources_check_identity_aliases_and_release_exactly_once() {
    use std::{cell::Cell, rc::Rc};
    struct Tracked(Rc<Cell<usize>>);
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Rc::new(Cell::new(0));
    let mut store = host::Resources::default();
    let first = store.insert("first", Tracked(drops.clone())).unwrap();
    assert!(std::ptr::eq(
        store.get::<Tracked>(&first, "first").unwrap(),
        store.get::<Tracked>(&first, "first").unwrap()
    ));
    store
        .validate_call(&[(&first, "first", false), (&first, "first", false)])
        .unwrap();
    for consumes in [(true, false), (false, true), (true, true)] {
        assert!(
            store
                .validate_call(&[(&first, "first", consumes.0), (&first, "first", consumes.1)])
                .unwrap_err()
                .contains("aliased")
        );
    }
    assert!(
        store
            .take::<String>(&first, "first")
            .unwrap_err()
            .contains("Rust resource type")
    );
    let V::Opaque { token, .. } = &first else {
        panic!()
    };
    let forged = V::Opaque {
        ty: "second".into(),
        token: token.clone(),
    };
    assert!(
        store
            .get::<Tracked>(&forged, "second")
            .err()
            .unwrap()
            .contains("identity/type")
    );
    let mut other = host::Resources::default();
    let second = other.insert("first", Tracked(drops.clone())).unwrap();
    assert_ne!(first, second);
    assert!(other.get::<Tracked>(&first, "first").is_err());
    assert!(
        store
            .validate_call(&[(&first, "first", true), (&second, "first", false)])
            .is_err()
    );
    assert_eq!(drops.get(), 0);
    drop(store.take::<Tracked>(&first, "first").unwrap());
    assert_eq!(drops.get(), 1);
    assert!(store.get::<Tracked>(&first, "first").is_err());
    assert!(store.take::<Tracked>(&first, "first").is_err());
    let third = store.insert("first", Tracked(drops.clone())).unwrap();
    assert_ne!(first, third);
    drop(store);
    drop(other);
    assert_eq!(drops.get(), 3);
}

mod frame_retirement {
    use super::*;
    use std::cell::RefCell;
    thread_local! { static DROPS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) }; }
    struct Probe(u32);
    impl Drop for Probe {
        fn drop(&mut self) {
            DROPS.with(|drops| drops.borrow_mut().push(self.0));
        }
    }
    fn adapter(resources: &mut host::Resources, op: &str, args: Vec<V>) -> host::R<host::Outcome> {
        match (op, args.as_slice()) {
            ("make_probe", [V::U32(id)]) => {
                Ok(("made".into(), vec![resources.insert("probe", Probe(*id))?]))
            }
            ("inspect_probe", [value]) => Ok((
                "number".into(),
                vec![V::U32(resources.get::<Probe>(value, "probe")?.0)],
            )),
            ("close_probe", [value]) => {
                drop(resources.take::<Probe>(value, "probe")?);
                Ok(("closed".into(), vec![]))
            }
            _ => Err("unexpected test adapter call".into()),
        }
    }
    fn new_host() -> Host {
        let m = tellur::manifest::Manifest::from_json(&json!({
            "version":"0.1", "types":[{"name":"probe","kind":"opaque","linear":true,"fault":"release"}],
            "rust":{"source":"test-only.rs","lockfile":"test-only.lock","resources":{"probe":"Probe"}},
            "primitives":[
                {"name":"make_probe","parameters":[{"name":"id","type":"u32","mode":"value"}],"outcomes":[{"name":"made","fields":[{"name":"value","type":"probe"}]}],"effects":[],"returns":true,"rust":{"execution":"immediate"}},
                {"name":"inspect_probe","parameters":[{"name":"value","type":"probe","mode":"borrow"}],"outcomes":[{"name":"number","fields":[{"name":"id","type":"u32"}]}],"effects":[],"returns":true,"rust":{"execution":"immediate"}},
                {"name":"close_probe","parameters":[{"name":"value","type":"probe","mode":"consume"}],"outcomes":[{"name":"closed","fields":[]}],"effects":[],"returns":true,"rust":{"execution":"immediate"}}
            ]
        }).to_string()).unwrap();
        let mut h = Host::new(serde_json::to_value(m).unwrap(), None, true, None);
        h.rust_adapter = Some(adapter);
        h
    }
    fn make(h: &mut Host, id: u32) -> V {
        h.primitive("make_probe", vec![V::U32(id)])
            .unwrap()
            .1
            .pop()
            .unwrap()
    }
    fn inspect(h: &mut Host, value: &V, id: u32) {
        assert_eq!(
            h.primitive("inspect_probe", vec![value.clone()]).unwrap().1,
            vec![V::U32(id)]
        );
    }
    fn drops() -> Vec<u32> {
        DROPS.with(|drops| drops.borrow().clone())
    }
    #[test]
    fn retained_frame_release_does_not_follow_borrows_or_release_other_owners() {
        DROPS.with(|drops| drops.borrow_mut().clear());
        let mut h = new_host();
        let independent = make(&mut h, 0);
        let mut expected = vec![];
        for request in 0..24 {
            let id = 1 + request * 3;
            let retained = make(&mut h, id);
            let first = make(&mut h, id + 1);
            let second = make(&mut h, id + 2);
            // Tokens copied below are observation handles, never extra owners.
            let retired_tokens = [first.clone(), second.clone()];
            let borrower = V::Reference {
                evidence: Default::default(),
                ty: "&probe".into(),
                root: 0,
                fields: vec![],
            };
            let borrowed_frame = vec![Some(V::Task {
                binding: None,
                ty: "pending".into(),
                operation: "unstarted".into(),
                arguments: vec![borrower.clone()],
            })];
            let owned_frame = vec![Some(V::Task {
                binding: None,
                ty: "pending".into(),
                operation: "unstarted".into(),
                arguments: vec![V::Construct {
                    ty: "pair".into(),
                    name: "pair".into(),
                    fields: vec![first, second, borrower],
                }],
            })];
            h.release_frame(&borrowed_frame);
            assert_eq!(drops(), expected, "releasing a borrower released its owner");
            for (offset, token) in retired_tokens.iter().enumerate() {
                inspect(&mut h, token, id + 1 + offset as u32);
            }
            inspect(&mut h, &retained, id);
            inspect(&mut h, &independent, 0);
            h.release_frame(&owned_frame);
            let after = drops();
            assert_eq!(&after[..expected.len()], expected);
            let mut released = after[expected.len()..].to_vec();
            released.sort();
            assert_eq!(
                released,
                [id + 1, id + 2],
                "owned captures were not released exactly once"
            );
            expected = after;
            for token in retired_tokens {
                assert!(h.primitive("inspect_probe", vec![token]).is_err());
            }
            h.release_frame(&owned_frame);
            assert_eq!(
                drops(),
                expected,
                "a stale frame caused duplicate destruction"
            );
            // Contingency may now use/consume its retained owner. The unrelated
            // host object remains usable across every request's explicit release.
            inspect(&mut h, &retained, id);
            inspect(&mut h, &independent, 0);
            h.primitive("close_probe", vec![retained]).unwrap();
            expected.push(id);
            assert_eq!(drops(), expected);
        }
        inspect(&mut h, &independent, 0);
        h.primitive("close_probe", vec![independent]).unwrap();
        expected.push(0);
        drop(h);
        assert_eq!(
            drops(),
            expected,
            "host teardown repeated an explicit release"
        );
    }
}
