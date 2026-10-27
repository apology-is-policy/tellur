use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
const SOURCE: &str = r#"
struct box<t> { t value; }
control duplicate<t: clone>(&t value) exits ready { t value } { exit ready {clone(value)}; }
control repeat<t: copy>(t value) exits ready {t first, t second} { exit ready {value,value}; }
control discard<t: drop>(t value) exits done { exit done; }
control main() exits auto {
    box<string> original=box<string>{value:"Ada"};
    delta duplicate<box<string>>(&original) {ready {auto copied}=>copied}
    [copied]: emit(copied.value); discard<box<string>>(original);
    [copied]: forward repeat<u32>(7);
}
"#;
#[test]
fn explicit_permissions_work_with_inference_borrows_and_field_derivation() {
    let p = compile(SOURCE, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.fields, vec![Value::U32(7), Value::U32(7)]);
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(
        p.generic_controls()
            .iter()
            .find(|c| c.name == "duplicate")
            .unwrap()
            .type_bounds["t"],
        vec!["clone"]
    );
}
#[test]
fn definitions_get_exactly_their_declared_permissions() {
    for (bound, body, code) in [
        ("drop", "exit ready {clone(&value)};", "E_CLONE"),
        ("clone", "exit ready {value,value};", "E_MOVED"),
        ("drop", "exit ready {value,value};", "E_MOVED"),
    ] {
        for caller in ["", "control main() exits auto {forward bad<u32>(1);}"] {
            let e = compile(
                &format!("control bad<t: {bound}>(t value) exits auto {{{body}}} {caller}"),
                manifest(),
            )
            .unwrap_err();
            assert_eq!(e.code, code, "{e}");
            assert!(e.message.contains("generic definition check"), "{e}");
        }
    }
    let e=compile("struct box<t>{t value;} control bad<t: drop>(&box<t> value) exits auto {exit ready {clone(value)};}",manifest()).unwrap_err();
    assert_eq!(e.code, "E_CLONE", "{e}");
}
#[test]
fn implication_is_checked_for_nested_generic_calls_before_callee_bodies() {
    for (outer, inner, valid) in [
        ("copy", "clone", true),
        ("copy", "drop", true),
        ("clone", "drop", true),
        ("drop", "clone", false),
        ("clone", "copy", false),
    ] {
        let source = format!(
            "control inner<t: {inner}>(t value) exits done {{exit done;}}\ncontrol outer<t: {outer}>(t value) exits done {{forward inner<t>(value);}}"
        );
        let r = compile(&source, manifest());
        if valid {
            r.unwrap();
        } else {
            let e = r.unwrap_err();
            assert_eq!(e.code, "E_BOUND", "{e}");
            assert_eq!(e.span.line, 2);
        }
    }
    let e=compile("control inner<t: drop>(t value) exits done {exit done;} control outer<t>(t value) exits done {forward inner<t>(value);}",manifest()).unwrap_err();
    assert_eq!(e.code, "E_BOUND", "{e}");
}
#[test]
fn concrete_arguments_obey_bounds_even_when_phantom_and_unused() {
    for (ty, bound, valid) in [
        ("u32", "copy", true),
        ("string", "clone", true),
        ("string", "copy", false),
        ("handle", "drop", false),
        ("answer", "drop", false),
        ("answer", "clone", false),
        ("vector<string>", "clone", true),
        ("vector<u32>", "copy", false),
        ("array<u32,2>", "copy", true),
        ("array<string,2>", "copy", false),
        ("box<string>", "clone", true),
        ("box<handle>", "drop", false),
        ("optional<string>", "clone", true),
        ("optional<handle>", "clone", false),
        ("map<string,string>", "clone", true),
        ("option<answer>", "drop", false),
    ] {
        let source = format!(
            "struct box<t>{{t value;}} variant optional<t>{{some {{t value}},none}} outcome answer{{yes,no}}\ncontrol need<t: {bound}>() exits done {{exit done;}}\ncontrol main() exits done {{forward need<{ty}>();}}"
        );
        let r = compile(&source, manifest());
        if valid {
            r.unwrap();
        } else {
            let e = r.unwrap_err();
            assert_eq!(e.code, "E_BOUND", "{ty}: {bound}: {e}");
            assert_eq!(e.span.line, 3);
        }
    }
}
#[test]
fn combinations_and_sealed_bound_names_are_validated() {
    compile("control ok<a: copy + clone + drop,b: drop>(a first,b second) exits auto {exit ready {first,first};}",manifest()).unwrap();
    for bounds in ["copy + copy", "unknown", "clone + something"] {
        let e = compile(
            &format!("control bad<t: {bounds}>() exits done {{exit done;}}"),
            manifest(),
        )
        .unwrap_err();
        assert_eq!(e.code, "E_BOUND", "{e}");
    }
    let json = r#"{"version":"0.1","types":[],"primitives":[],"abstract_types":{"handle":{"copy":true,"clone":true,"drop":true}}}"#;
    assert!(Manifest::from_json(json).is_err());
}
#[test]
fn bounds_do_not_erase_outcome_obligations_or_change_nominal_identity() {
    let e=compile("control need<t: drop>(t value) exits done {exit done;} outcome answer {ready} control main() exits done {forward need<answer>(answer.ready {});}",manifest()).unwrap_err();
    assert_eq!(e.code, "E_BOUND", "{e}");
    let e=compile("control bad<a: copy,b: copy>(a first,b second) exits ready {a first,a second} {exit ready {first,second};}",manifest()).unwrap_err();
    assert_eq!(e.code, "E_TYPE", "{e}");
}
#[test]
fn bounded_controls_match_native_and_expanded_execution() {
    use std::{fs, process::Command};
    let p = compile(SOURCE, manifest()).unwrap();
    let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("path-capabilities-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let file = root.join("scenario.json");
    fs::write(&file, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, program) in [
        ("original", p.clone()),
        ("expanded", p.expanded("main").unwrap()),
    ] {
        for opt in [0, 2] {
            let bin = root.join(format!("{name}-{opt}"));
            program.build_native("main", &bin, opt).unwrap();
            let out = Command::new(bin)
                .arg("--scenario")
                .arg(&file)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
                expected
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_region_syntax_cannot_bypass_generic_move_permissions() {
    let e = compile(
        "region bad<t: clone>(t value) exits auto {exit ready {value,value};}",
        manifest(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_GENERIC");
    assert!(e.message.contains("control syntax"));
}
#[test]
fn bounded_fields_cannot_hide_moves_or_grant_copy_to_vectors() {
    for (ty, body, code) in [
        ("box<t>", "exit ready {value.value,value.value};", "E_MOVED"),
        ("vector<t>", "exit ready {value,value};", "E_MOVED"),
    ] {
        let s = format!(
            "struct box<t>{{t value;}} control bad<t: clone>({ty} value) exits auto {{{body}}}"
        );
        let e = compile(&s, manifest()).unwrap_err();
        assert_eq!(e.code, code, "{e}");
    }
    compile(
        "control dispose<t: drop>(t value) exits done {drop(value);exit done;}",
        manifest(),
    )
    .unwrap();
    let p = compile(
        include_str!("../playground/capabilities.tellur"),
        manifest(),
    )
    .unwrap();
    p.run(&scenario()).unwrap();
}
