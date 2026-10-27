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
