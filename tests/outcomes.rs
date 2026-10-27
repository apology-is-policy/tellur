use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
fn run(source: &str, flag: bool) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(flag)],
        calls: vec![],
    };
    let r = p.run(&s).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    r
}
const PRODUCER: &str = r#"control choose(bool flag) exits auto {emit("produce");exit delta flag {true=>ready {"value"},false=>empty {}};}"#;
const PROGRAM: &str = r#"control main(bool flag) exits auto {
    outcome<choose> saved=capture choose(flag);
    emit("between");
    exit delta saved {ready {string value}=>done {value},empty=>missing {}};
}"#;
#[test]
fn capture_executes_now_and_handling_does_not_replay_the_operation() {
    let source = format!("{PROGRAM}\n{PRODUCER}");
    for flag in [true, false] {
        let r = run(&source, flag);
        assert_eq!(r.outcome, if flag { "done" } else { "missing" });
        assert_eq!(
            r.events,
            vec![
                Event::Emit {
                    message: "produce".into()
                },
                Event::Emit {
                    message: "between".into()
                }
            ]
        );
    }
}
#[test]
fn complete_outcomes_can_move_through_controls_and_forward() {
    let source = format!(
        r#"control main(bool flag) exits auto {{ auto saved=capture choose(flag); auto moved=saved; exit delta forward(moved) {{ready {{auto value}}=>done {{value}},empty=>missing {{}}}}; }}
    control forward(outcome<choose> saved) exits auto {{exit saved;}}
    {PRODUCER}"#
    );
    assert_eq!(
        run(&source, true).fields,
        vec![Value::String("value".into())]
    );
    assert_eq!(run(&source, false).outcome, "missing");
    rejects(
        &source.replace("auto moved=saved;", "auto moved=saved; auto twice=saved;"),
        "E_MOVED",
    );
}
#[test]
fn even_an_empty_or_copy_payload_outcome_must_be_handled() {
    for body in [
        "exit done;",
        "drop(saved);exit done;",
        "auto copy=clone(&saved);exit done;",
    ] {
        rejects(
            &format!(
                "control main(bool flag) exits done {{auto saved=capture choose(flag);{body}}}{PRODUCER}"
            ),
            "E_OUTCOME",
        );
    }
    rejects(
        "control choose() exits empty {exit empty;} control main() exits done {auto saved=capture choose();exit done;}",
        "E_OUTCOME",
    );
    rejects(
        "control choose() exits ready {u32 n} {exit ready {7};} control main() exits done {auto saved=capture choose();exit done;}",
        "E_OUTCOME",
    );
}
#[test]
fn explicit_capture_annotations_and_handling_contracts_are_checked() {
    rejects(
        &format!(
            "control main(bool flag) exits done {{string saved=capture choose(flag);exit done;}}{PRODUCER}"
        ),
        "E_TYPE",
    );
    rejects(
        &format!(
            "control main(bool flag) exits done {{auto saved=capture choose(flag);delta saved {{empty=>no}} [no]:exit done;}}{PRODUCER}"
        ),
        "E_ARMS",
    );
    rejects(
        "control view(&string text) exits auto {exit found {text};} control main() exits done {string text=\"local\";auto saved=capture view(&text);exit done;}",
        "E_OUTCOME",
    );
    rejects(
        "control view(&string text) exits auto {exit found {text};} control main(outcome<view> saved) exits auto {exit saved;}",
        "E_MANIFEST",
    );
}
#[test]
fn wrapping_a_completed_outcome_does_not_erase_its_obligation() {
    rejects(
        &format!(
            "control main(bool flag) exits done {{auto saved=capture choose(flag);auto wrapped=some {{saved}};exit done;}}{PRODUCER}"
        ),
        "E_OUTCOME",
    );
    let source = format!(
        r#"control main(bool flag) exits auto {{auto saved=capture choose(flag);vector<outcome<choose>> queue=[saved];
        loop drain(vector<outcome<choose>> pending=queue) {{
            delta vector_take(pending) {{item {{auto result,auto rest}}=>item,empty=>end}}
            [item]:delta result {{ready {{auto value}}=>ready,empty=>empty}}
            [ready]:emit(value); [ready|empty]:again drain(rest); [end]:exit done;
        }}
    }} {PRODUCER}"#
    );
    assert_eq!(run(&source, true).outcome, "done");
    assert_eq!(run(&source, false).outcome, "done");
}
#[test]
fn capture_resource_outcomes_still_requires_payload_completion() {
    let source = "control acquire(handle file) exits auto {exit opened {file};} control main(handle file) exits done {auto result=capture acquire(file);exit delta result {opened {auto handle}=>done {}};}";
    rejects(source, "E_RESOURCE");
    compile(
        &source
            .replace("exits done {auto result", "exits auto {auto result")
            .replace("=>done {}", "=>done {handle}"),
        manifest(),
    )
    .unwrap();
}
#[test]
fn capture_does_not_pollute_ordinary_constructor_names_or_allow_forgery() {
    let source = "control choose() exits some {u32 value} {exit some {1};} control main() exits auto {auto regular=some {2};auto saved=capture choose();delta saved {some {auto value}=>ready} [ready]:exit done {regular,value};}";
    compile(source, manifest()).unwrap();
    rejects(
        "control choose() exits ready {exit ready;} control main() exits auto {outcome<choose> forged=ready {};exit forged;}",
        "E_NAME",
    );
    rejects(
        "control choose(outcome<choose> saved) exits auto {exit saved;}",
        "E_RECURSION",
    );
}
#[test]
fn captured_results_forward_nullary_labels_without_losing_obligations() {
    let source = format!(
        "control main(bool flag) exits auto {{auto done=capture choose(flag);exit done;}}{PRODUCER}"
    );
    assert_eq!(run(&source, false).outcome, "empty");
    rejects(&source.replace("exit done;", "exit done {};"), "E_OUTCOME");
}
#[test]
fn native_capture_and_delayed_handling_preserve_effect_order_after_expansion() {
    let source = format!("{PROGRAM}\n{PRODUCER}");
    let p = compile(&source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("path-outcomes-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (i, program) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        for opt in [0, 2] {
            let binary = root.join(format!("outcome-{i}-{opt}"));
            program.build_native("main", &binary, opt).unwrap();
            for flag in [true, false] {
                let expected = run(&source, flag);
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
#[test]
fn captured_load_keeps_read_close_failures_and_order_before_later_effect() {
    let m = Manifest::from_json(include_str!("../examples/manifest.json")).unwrap();
    let source = format!(
        "{}\ncontrol main(string path) exits auto {{auto loaded=capture load(path);emit(\"after load\");exit loaded;}}",
        include_str!("../examples/load.tellur")
    );
    let p = compile(&source, m).unwrap();
    let mut s: Scenario =
        serde_json::from_str(include_str!("../examples/double-failure.json")).unwrap();
    s.entry = "main".into();
    let r = p.run(&s).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    assert_eq!(r.outcome, "Failed");
    let Value::Construct { name, fields, .. } = &r.fields[0] else {
        panic!()
    };
    assert_eq!(name, "Both");
    assert_eq!(fields.len(), 2);
    assert!(matches!(&r.events[2],Event::Primitive{operation,..} if operation=="close"));
    assert_eq!(
        r.events.last(),
        Some(&Event::Emit {
            message: "after load".into()
        })
    );
    let expanded = p.expanded("main").unwrap();
    let names = &expanded.region("main").unwrap().atoms;
    assert!(names.values().any(|n| n.contains("ReadBad")));
    assert!(names.values().any(|n| n.contains("CloseBad")));
}
#[test]
fn two_captures_of_one_control_remain_independent() {
    let source = format!(
        r#"control main(bool flag) exits auto {{auto first=capture choose(flag);auto second=capture choose(false);
        exit delta first {{ready {{auto value}}=>delta second {{ready {{auto other}}=>unexpected {{}},empty=>done {{value}}}},empty=>delta second {{ready {{auto other}}=>unexpected {{}},empty=>empty {{}}}}}};
    }} {PRODUCER}"#
    );
    assert_eq!(run(&source, true).outcome, "done");
    assert_eq!(run(&source, false).outcome, "empty");
}
#[test]
fn path_disjoint_capture_bindings_must_agree_on_mutability() {
    rejects(
        &format!(
            "control main(bool flag) exits auto {{delta flag {{true=>yes,false=>no}} [yes]:mutable auto saved=capture choose(true); [no]:auto saved=capture choose(false); [entry]:exit saved;}}{PRODUCER}"
        ),
        "E_MUTABILITY",
    );
}
#[test]
fn manifest_primitive_capture_produces_one_owned_result() {
    let r = run(
        "control main(bool flag) exits auto {auto saved=capture u32_text(7);exit delta saved {text {auto text}=>done {text}};}",
        true,
    );
    assert_eq!(r.fields, vec![Value::String("7".into())]);
    assert!(r.events.is_empty());
}
#[test]
fn forwarding_accepts_a_larger_explicit_exit_contract_but_not_a_renaming() {
    let source = format!(
        "control main(bool flag) exits ready {{string value}},empty,extra {{auto saved=capture choose(flag);exit saved;}}{PRODUCER}"
    );
    assert_eq!(run(&source, true).outcome, "ready");
    assert_eq!(run(&source, false).outcome, "empty");
    rejects(
        &source.replacen(
            "exits ready {string value}",
            "exits renamed {string value}",
            1,
        ),
        "E_EXIT",
    );
}
