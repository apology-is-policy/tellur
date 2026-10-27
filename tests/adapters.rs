use tellur::{
    compile,
    ir::{Action, Value},
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
fn run(source: &str, arguments: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments,
        calls: vec![],
    };
    let r = p.run(&s).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    r
}
const ADAPTER: &str = r#"control main(bool valid,bool big) exits auto {
    exit delta parse(valid) {
        number { u32 value } => [parsed]: delta increment(value,big) {
            sum { u32 value } => [incremented]: ready {value},
            overflow => [addition_overflow]: too_large {}
        },
        malformed => invalid {},
        overflow => [parse_overflow]: too_large {}
    };
}
control parse(bool valid) exits auto {
    exit delta valid {true => number {7},false => malformed {}};
    // The caller must handle only the checked contract. No unused declared exit here.
}
control increment(u32 value,bool big) exits auto {
    exit delta big {true => overflow {},false => sum {value}};
}"#;
#[test]
fn omitted_entry_and_explicit_contract_body_are_unambiguous() {
    let source = r#"control main() exits done {string message} {
        string message="hello";
        exit done {message};
    }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("hello".into())]
    );
    run("control main() exits done { exit done; }", vec![]);
    run("control main() exits { done } { exit done; }", vec![]);
    run("control main() exits done {} { exit done; }", vec![]);
    run(
        "control main() exits done { [entry] over &: { exit done; } }",
        vec![],
    );
    run(
        "control main() exits done { loop once() { exit done; } }",
        vec![],
    );
}
#[test]
fn flat_selectors_still_own_their_following_statements() {
    rejects(
        "control main(bool flag) exits done { delta flag {true=>yes,false=>no} [yes]: emit(\"yes\"); exit done; }",
        "E_UNCOVERED",
    );
    let source = "control main(bool flag) exits done { delta flag {true=>yes,false=>no} [yes]: emit(\"yes\"); [entry]: exit done; }";
    run(source, vec![Value::Bool(false)]);
}
#[test]
fn nested_adapters_infer_union_and_enforce_exhaustiveness() {
    rejects(ADAPTER, "E_ARMS");
    let source = ADAPTER.replace("        overflow => [parse_overflow]: too_large {}", "");
    // Existing scope rules prohibit shadowing an enclosing payload binding.
    rejects(&source, "E_SHADOW");
    let source = source.replace(
        "sum { u32 value } => [incremented]: ready {value}",
        "sum { u32 next } => [incremented]: ready {next}",
    );
    assert_eq!(
        run(&source, vec![Value::Bool(true), Value::Bool(false)]).fields,
        vec![Value::U32(7)]
    );
    assert_eq!(
        run(&source, vec![Value::Bool(true), Value::Bool(true)]).outcome,
        "too_large"
    );
    assert_eq!(
        run(&source, vec![Value::Bool(false), Value::Bool(false)]).outcome,
        "invalid"
    );
}
#[test]
fn sibling_inline_payload_bindings_have_independent_scopes() {
    let source = r#"control choose(bool flag) exits auto {exit delta flag {true=>text {"hello"},false=>number {4}};}
    control main(bool flag) exits auto {exit delta choose(flag) {text {string value}=>done {value},number {u32 value}=>done {u32_text(value)}};}"#;
    assert_eq!(
        run(source, vec![Value::Bool(true)]).fields,
        vec![Value::String("hello".into())]
    );
    assert_eq!(
        run(source, vec![Value::Bool(false)]).fields,
        vec![Value::String("4".into())]
    );
}
#[test]
fn returned_borrows_and_resource_payloads_are_preserved() {
    let source = r#"control first(&vector<string> words) exits auto {
        exit delta vector_get(words,0) {found {&string word}=>[available]:found {word},out_of_bounds=>[empty]:empty {}};
    }
    control main() exits auto {vector<string> words=["hello"];exit delta first(&words) {found {auto word}=>done {clone(word)},empty=>done {"empty"}};}"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("hello".into())]
    );
    rejects(
        "control main() exits auto {vector<string> words=[\"local\"];exit delta vector_get(&words,0) {found {auto word}=>found {word},out_of_bounds=>empty {}};}",
        "E_LIFETIME",
    );
    let source = "control main(handle h) exits auto {exit delta some {some {handle file}=>done {file},none=>empty {}};}";
    let _ = source;
    let source = "control main(handle h) exits auto {option<handle> wrapped=some {h};exit delta wrapped {some {handle file}=>done {file},none=>empty {}};}";
    let handle = Value::Opaque {
        ty: "handle".into(),
        token: "h".into(),
    };
    assert_eq!(run(source, vec![handle.clone()]).fields, vec![handle]);
    rejects(&source.replace("done {file}", "done {}"), "E_RESOURCE");
}
#[test]
fn invalid_adapter_shapes_and_reference_aliases_are_rejected() {
    rejects(
        "control main(bool b) exits auto {exit delta b {true=>done {1},false=>done {\"bad\"}};}",
        "E_TYPE",
    );
    rejects(
        "control main(bool b) exits auto {exit delta b {true=>done {},false=>done {1}};}",
        "E_INFER",
    );
    rejects(
        "control main(bool b) exits auto {exit delta b {true=>done,false=>done {}};}",
        "E_PARSE",
    );
    rejects(
        "control main(bool b) exits done {exit delta b {true=>other {},false=>done {}};}",
        "E_EXIT",
    );
    rejects(
        "control main(&mutable string value,bool b) exits auto {exit delta b {true=>done {value,value},false=>done {value,value}};}",
        "E_LOAN",
    );
}
#[test]
fn selected_nested_effect_runs_once_and_keeps_both_failure_origins() {
    let source = r#"control main(string filename) exits auto {
        exit delta open(filename) {
            opened {handle file}=>delta close(file) {closed=>done {},failed {auto error}=>failed {}},
            not_found=>failed {}, denied=>failed {}
        };
    }"#;
    let p = compile(source, manifest()).unwrap();
    for opened in [false, true] {
        let calls = if opened {
            vec![
                ScriptCall {
                    operation: "open".into(),
                    outcome: "opened".into(),
                    fields: vec![Value::Opaque {
                        ty: "handle".into(),
                        token: "f".into(),
                    }],
                },
                ScriptCall {
                    operation: "close".into(),
                    outcome: "closed".into(),
                    fields: vec![],
                },
            ]
        } else {
            vec![ScriptCall {
                operation: "open".into(),
                outcome: "not_found".into(),
                fields: vec![],
            }]
        };
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![Value::String("file".into())],
            calls,
        };
        let r = p.run(&s).unwrap();
        assert_eq!(r.events.len(), if opened { 2 } else { 1 });
        assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    }
    assert!(
        p.region("main")
            .unwrap()
            .atoms
            .values()
            .any(|n| n == "not_found")
    );
}
#[test]
fn native_inline_adapters_preserve_invocation_identity_and_borrows() {
    let source = r#"control borrow_text(&string text,bool use_it) exits auto {exit delta use_it {true=>found {text},false=>empty {}};}
    control choose(bool flag) exits auto {exit delta flag {true=>yes {},false=>no {}};}
    control main(bool flag) exits auto {
        string owner="hello";
        exit delta choose(flag) {
            yes=>delta borrow_text(&owner,flag) {found {auto ref}=>ready {clone(ref)},empty=>empty {}},
            no=>delta choose(flag) {yes=>unexpected {},no=>empty {}}
        };
    }"#;
    let p = compile(source, manifest()).unwrap();
    let expanded = p.expanded("main").unwrap();
    let invocations = expanded
        .region("main")
        .unwrap()
        .code
        .iter()
        .filter(|i| matches!(i.action, Action::Decide { .. }))
        .map(|i| i.provenance.calls.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(invocations.len() >= 3);
    let root = std::env::temp_dir().join(format!("path-adapters-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (index, program) in [p.clone(), expanded].iter().enumerate() {
        for opt in [0, 2] {
            let binary = root.join(format!("adapter-{index}-{opt}"));
            program.build_native("main", &binary, opt).unwrap();
            for flag in [true, false] {
                let expected = p
                    .run(&Scenario {
                        entry: "main".into(),
                        arguments: vec![Value::Bool(flag)],
                        calls: vec![],
                    })
                    .unwrap();
                let actual = std::process::Command::new(&binary)
                    .args(["--json", "--", if flag { "true" } else { "false" }])
                    .output()
                    .unwrap();
                assert!(
                    actual.status.success(),
                    "{}",
                    String::from_utf8_lossy(&actual.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
