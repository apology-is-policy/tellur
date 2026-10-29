#[allow(dead_code)]
#[path = "../src/native/runtime.rs"]
mod host;
use host::{Host, V};
use serde_json::{Value as Json, json};
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
