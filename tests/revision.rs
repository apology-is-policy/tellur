mod support;
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str, entry: &str, arguments: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let scenario = Scenario {
        entry: entry.into(),
        arguments,
        calls: vec![],
    };
    let original = p.run(&scenario).unwrap();
    let expanded = p.expanded(entry).unwrap().run(&scenario).unwrap();
    assert_eq!(original, expanded);
    original
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
#[test]
fn type_first_declarations_provide_context_and_auto_infers() {
    let result = run(
        r#"control main() exits auto {
        [entry]: option<option<u32>> absent = some { none };
            auto present = some { 7 };
            delta absent { some { option<u32> inner } => outer, none => empty }
        [outer]: delta inner { some { auto n } => number, none => missing }
        [number]: exit done { n };
        [empty | missing]:
            delta present { option.some { u32 n } => found, option.none => none }
        [found]: exit done { n };
        [none]: exit done { 0 };
    }"#,
        "main",
        vec![],
    );
    assert_eq!(result.fields, vec![Value::U32(7)]);
}
#[test]
fn inferred_contracts_are_checked_before_forward_callers() {
    let source = r#"control main() exits auto {
        [entry]: delta relay(42) { ready { u32 n } => ready }
        [ready]: exit done { n };
    }
    control relay(u32 n) exits auto {
        [entry]: delta identity(n) { ready { auto value } => ready }
        [ready]: exit ready { value };
    }
    control identity(u32 n) exits auto { [entry]: exit ready { n }; }"#;
    assert_eq!(run(source, "main", vec![]).fields, vec![Value::U32(42)]);
}
#[test]
fn inferred_alternatives_remain_exhaustive_and_payloads_path_checked() {
    rejects(
        "control main(bool b) exits auto { [entry]: delta choose(b) { yes => yes } [yes]: exit done; } control choose(bool b) exits auto { [entry]: delta b { true => yes, false => no } [yes]: exit yes; [no]: exit no; }",
        "E_ARMS",
    );
    rejects(
        "control main(bool b) exits auto { [entry]: delta b { true => yes, false => no } [yes]: u32 x = 1; [entry]: exit done { x }; }",
        "E_UNAVAILABLE",
    );
}
#[test]
fn conflicting_inferred_exit_shapes_and_missing_exits_are_rejected() {
    rejects(
        r#"control main(bool b) exits auto { [entry]: delta b { true => yes, false => no } [yes]: exit done { 1 }; [no]: exit done { "bad" }; }"#,
        "E_TYPE",
    );
    rejects(
        "control main(bool b) exits auto { [entry]: delta b { true => yes, false => no } [yes]: exit done { 1 }; [no]: exit done; }",
        "E_INFER",
    );
    rejects(
        "control main() exits auto { loop forever() { [entry]: again forever(); } }",
        "E_INFER",
    );
    rejects(
        "control main() exits auto { [entry]: emit(\"missing exit\"); }",
        "E_UNCOVERED",
    );
}
#[test]
fn annotated_bindings_and_patterns_must_match_exactly() {
    rejects(
        "control main() exits done { [entry]: string wrong = 1; exit done; }",
        "E_TYPE",
    );
    rejects(
        "control main() exits done { [entry]: delta u32_text(1) { text { u32 wrong } => ready } [ready]: exit done; }",
        "E_TYPE",
    );
    rejects(
        "control main() exits done { [entry]: auto absent = none; exit done; }",
        "E_INFER",
    );
    rejects(
        "control main() exits done { [entry]: auto value; exit done; }",
        "E_PARSE",
    );
    rejects(
        "control main() exits done { [entry]: delta u32_text(1) { text { value } => ready } [ready]: exit done; }",
        "E_PARSE",
    );
}
#[test]
fn pure_expressions_and_direct_decisions_have_identical_values_and_no_events() {
    let result = run(
        r#"control main() exits auto {
        [entry]:
            string message = concat("value=", u32_text(42));
            delta concat("value=", u32_text(42)) { text { auto other } => ready }
        [ready]: exit done { message, other };
    }"#,
        "main",
        vec![],
    );
    assert_eq!(result.fields, vec![Value::String("value=42".into()); 2]);
    assert!(result.events.is_empty());
}
#[test]
fn single_outcome_is_not_implicitly_pure_and_purity_contract_cannot_be_forged() {
    let mut m = manifest();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "u32_text")
        .unwrap()
        .pure = false;
    assert_eq!(
        compile(
            "control main() exits done { [entry]: string s = u32_text(1); exit done; }",
            m
        )
        .unwrap_err()
        .code,
        "E_EFFECT"
    );
    for name in ["open", "checked_add", "u32_text"] {
        let mut m = manifest();
        let p = m.primitives.iter_mut().find(|p| p.name == name).unwrap();
        p.pure = true;
        if name == "u32_text" {
            p.effects.push("write".into());
        }
        assert_eq!(
            compile("control main() exits done { [entry]: exit done; }", m)
                .unwrap_err()
                .code,
            "E_MANIFEST"
        );
    }
}
#[test]
fn brace_constructors_work_without_becoming_calls() {
    let source = "control main() exits done { [entry]: string s = u32_text { 1 }; exit done; }";
    rejects(source, "E_EFFECT");
    let result = run(
        "control main() exits { done { option<u32> value }, } { [entry]: exit done { option.some { 4 } }; }",
        "main",
        vec![],
    );
    assert!(matches!(&result.fields[0], Value::Construct { name, .. } if name == "Some"));
}
#[test]
fn modern_loops_keep_path_reset_and_simultaneous_transfer() {
    let source = r#"control main() exits auto {
        loop exchange(u32 left = 1, u32 right = 2, bool first = true) {
            [entry]: delta first { true => swap, false => finish }
            [swap]: again exchange(right, left, false);
            [finish]: exit done { left, right };
        }
    }"#;
    assert_eq!(
        run(source, "main", vec![]).fields,
        vec![Value::U32(2), Value::U32(1)]
    );
}
#[test]
fn inferred_resource_exits_still_enforce_moves_and_completion() {
    rejects(
        "control bad(handle file) exits auto { [entry]: exit done; }",
        "E_RESOURCE",
    );
    rejects(
        "control bad(handle file) exits auto { [entry]: exit done { file, file }; }",
        "E_MOVED",
    );
    let p = compile(
        "control forward(handle file) exits auto { [entry]: exit owned { file }; }",
        manifest(),
    )
    .unwrap();
    assert_eq!(p.region("forward").unwrap().exits[0].fields[0].ty, "handle");
}
#[test]
fn reserved_future_features_do_not_silently_compile() {
    rejects(
        "async control main() exits done { [entry]: exit done; }",
        "E_UNSUPPORTED",
    );
    for keyword in ["vary", "flux", "await", "spawn", "defer", "capture"] {
        rejects(
            &format!("control main() exits done {{ [entry]: {keyword} value; exit done; }}"),
            "E_UNSUPPORTED",
        );
    }
}
#[test]
fn all_revised_playground_programs_check() {
    for source in [
        include_str!("../playground/hello.tellur"),
        include_str!("../playground/count.tellur"),
        include_str!("../playground/port.tellur"),
        include_str!("../playground/read_file.tellur"),
        include_str!("../playground/ownership.tellur"),
        include_str!("../playground/references.tellur"),
        include_str!("../playground/collections.tellur"),
        include_str!("../playground/adapters.tellur"),
        include_str!("../playground/outcomes.tellur"),
        include_str!("../playground/stored_adapters.tellur"),
        include_str!("../playground/named_outcomes.tellur"),
        include_str!("../playground/forwarding.tellur"),
        include_str!("../playground/protocol/finite_retry.tellur"),
        include_str!("../playground/algebra/original.tellur"),
        include_str!("../playground/algebra/identity.tellur"),
        include_str!("../playground/algebra/reordered.tellur"),
    ] {
        compile(source, manifest()).unwrap();
    }
    assert!(matches!(
        run(include_str!("../playground/hello.tellur"), "main", vec![]).events[0],
        Event::Emit { .. }
    ));
}
#[test]
fn entry_aliases_cannot_be_redefined_and_recursion_is_diagnosed() {
    rejects(
        "control main() exits done { [entry]: delta true { true => entry, false => other } [entry]: exit done; }",
        "E_PATH",
    );
    rejects(
        "control main() exits auto { [entry]: delta main() { done => done } [done]: exit done; }",
        "E_RECURSION",
    );
}

#[test]
fn native_revision_matches_reference_with_pure_calls_options_and_inferred_exits() {
    let source = r#"
control main(bool flag) exits auto {
    [entry]: delta format(flag) { ready { string message, option<u32> number } => ready }
    [ready]: emit(message); exit done { number };
}
control format(bool flag) exits auto {
    [entry]: delta flag { true => first, false => second }
    [first]: string text = concat("first ", u32_text(42)); option<u32> n = none;
    [second]: auto text = concat("second ", u32_text(7)); auto n = some { 7 };
    [entry]: delta concat(text, "!") { text { string message } => ready }
    [ready]: exit ready { message, n };
}"#;
    let p = compile(source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("path-revision-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let program = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("program-{expanded}-{opt}"));
            support::scenario_executable(&program)
                .build_native("main", &binary, opt)
                .unwrap();
            for flag in [false, true] {
                let expected = p
                    .run(&Scenario {
                        entry: "main".into(),
                        arguments: vec![Value::Bool(flag)],
                        calls: vec![],
                    })
                    .unwrap();
                let output = std::process::Command::new(&binary)
                    .args(["--json", "--", if flag { "true" } else { "false" }])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(actual, serde_json::to_value(expected).unwrap());
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
