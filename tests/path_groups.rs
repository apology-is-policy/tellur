mod support;
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, Execution, Scenario, ScriptCall},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario(arguments: Vec<Value>) -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments,
        calls: vec![],
    }
}
fn run(source: &str, arguments: Vec<Value>) -> tellur::runtime::Execution {
    let program = compile(source, manifest()).unwrap();
    let input = scenario(arguments);
    let result = program.run(&input).unwrap();
    assert_eq!(
        result,
        program.expanded("main").unwrap().run(&input).unwrap()
    );
    result
}
fn emitted(result: &Execution) -> Vec<&str> {
    result
        .events
        .iter()
        .map(|event| match event {
            Event::Emit { message } => message.as_str(),
            event => panic!("unexpected event: {event:?}"),
        })
        .collect()
}
const GROUPED: &str = r#"control main(bool flag,bool brief) exits done {
    delta flag {true=>selected,false=>other}
[other]: emit("other");
[selected]: {
    string text="selected";
    delta brief {true=>short,false=>long}
    [short]: emit("short");
    [long]: {emit("long");}
    emit("selected continuation");
    path inside=entry;
}
[inside]: emit(text);
[other]: {}
    exit done;
}"#;

#[test]
fn groups_restore_the_enclosing_entry_and_keep_values_and_paths_visible() {
    for flag in [true, false] {
        for brief in [true, false] {
            let result = run(GROUPED, vec![Value::Bool(flag), Value::Bool(brief)]);
            let expected = if flag {
                vec![
                    if brief { "short" } else { "long" },
                    "selected continuation",
                    "selected",
                ]
            } else {
                vec!["other"]
            };
            assert_eq!(emitted(&result), expected);
        }
    }
}

#[test]
fn grouping_does_not_change_unbraced_extent_or_make_a_completion() {
    let grouped = "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]: {emit(\"yes\");} exit done;}";
    run(grouped, vec![Value::Bool(false)]);
    let flat = grouped.replace("{emit(\"yes\");}", "emit(\"yes\");");
    assert_eq!(compile(&flat, manifest()).unwrap_err().code, "E_UNCOVERED");
    let empty = "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]: {} [no]:exit done;}";
    assert_eq!(compile(empty, manifest()).unwrap_err().code, "E_UNCOVERED");
    assert_eq!(
        compile("control main() exits done {{exit done;}}", manifest())
            .unwrap_err()
            .code,
        "E_PARSE"
    );
}

#[test]
fn nested_groups_intersect_masks_and_entry_without_allowing_escape() {
    let source = "control main(bool a,bool b) exits done {delta a {true=>a_yes,false=>a_no} delta b {true=>b_yes,false=>b_no} [a_yes]: {[b_yes]: {emit(\"both\");}[entry]:emit(\"a\");} exit done;}";
    for a in [true, false] {
        for b in [true, false] {
            let result = run(source, vec![Value::Bool(a), Value::Bool(b)]);
            assert_eq!(
                emitted(&result),
                if a {
                    if b { vec!["both", "a"] } else { vec!["a"] }
                } else {
                    vec![]
                }
            );
        }
    }
    let error=compile("control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]: {[no]:emit(\"escape\");} exit done;}",manifest()).unwrap_err();
    assert_eq!(error.code, "E_EMPTY");
}

#[test]
fn groups_keep_borrows_and_move_conditions_in_the_enclosing_scope() {
    let source = "control main() exits done {mutable string owner=\"before\";[entry]: {auto reference=borrow mutable owner;} deref reference=\"after\";emit(reference);exit done;}";
    assert_eq!(emitted(&run(source, vec![])), vec!["after"]);
    assert_eq!(
        compile(
            &source.replace("deref reference=\"after\";", "owner=\"wrong\";"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    assert!(
        compile(
            &source
                .replace("[entry]: {", "[entry] over &: {")
                .replace("} deref", "} [entry]: deref"),
            manifest()
        )
        .is_err()
    );
    let moves = "control main(bool flag) exits done {string text=\"owned\";delta flag {true=>yes,false=>no}[yes]: {emit(text);}[no]: {emit(text);} exit done;}";
    for flag in [true, false] {
        assert_eq!(emitted(&run(moves, vec![Value::Bool(flag)])), vec!["owned"]);
    }
    assert_eq!(
        compile(
            &moves.replace("[no]: {emit(text);}", "emit(text);"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_MOVED"
    );
}

#[test]
fn group_end_does_not_drop_or_complete_resources() {
    let invalid = "control main(handle file) exits done {[entry]: {handle held=file;} exit done;}";
    assert_eq!(compile(invalid, manifest()).unwrap_err().code, "E_RESOURCE");
    let source = "control main(handle file) exits auto {[entry]: {handle held=file;} exit delta close(held) {closed=>done {},failed {auto error}=>failed {}};}";
    let program = compile(source, manifest()).unwrap();
    for failed in [false, true] {
        let input = Scenario {
            entry: "main".into(),
            arguments: vec![Value::Opaque {
                ty: "handle".into(),
                token: "file".into(),
            }],
            calls: vec![ScriptCall {
                operation: "close".into(),
                outcome: if failed { "failed" } else { "closed" }.into(),
                fields: if failed {
                    vec![Value::Opaque {
                        ty: "close_error".into(),
                        token: "error".into(),
                    }]
                } else {
                    vec![]
                },
            }],
        };
        let result = program.run(&input).unwrap();
        assert_eq!(
            result,
            program.expanded("main").unwrap().run(&input).unwrap()
        );
        assert_eq!(result.events.len(), 1);
    }
}

#[test]
fn grouped_loops_remain_tail_selected_and_cannot_be_lexically_nested() {
    let source = "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]: {loop once() {exit done;}}[no]:exit done;}";
    for flag in [true, false] {
        run(source, vec![Value::Bool(flag)]);
    }
    assert_eq!(
        compile(
            &source.replace(
                "loop once() {exit done;}",
                "loop once() {exit done;} emit(\"after\");"
            ),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOOP"
    );
    assert_eq!(
        compile(
            "control main() exits done {loop outer() {[entry]: {loop inner() {exit done;}}}}",
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOOP"
    );
}

#[test]
fn grouped_module_and_generic_bindings_survive_the_closing_brace() {
    let root = std::env::temp_dir().join(format!("tellur-group-modules-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("library.tellur"),"pub control identity<t>(t input) exits ready {t value} {[entry]: {t output=input;} exit ready {output};}").unwrap();
    std::fs::write(root.join("main.tellur"),"import \"library.tellur\" as lib; control main() exits auto {[entry]: {delta lib.identity<string>(\"hello\") {ready {auto value}=>ready}} [ready]:exit done {value};}").unwrap();
    let program = tellur::compile_file(root.join("main.tellur"), manifest()).unwrap();
    let input = scenario(vec![]);
    let result = program.run(&input).unwrap();
    assert_eq!(result.fields, vec![Value::String("hello".into())]);
    assert_eq!(
        result,
        program.expanded("main").unwrap().run(&input).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn extraction_preserves_complete_groups_and_rejects_an_interior_cut() {
    let source = "control main(bool flag) exits done {\n    delta flag {true=>yes,false=>no}\n    [yes]: {\n        emit(\"yes\");\n    }\n    exit done;\n}\n";
    let extracted =
        tellur::extract::extract_tail(source, manifest(), "main", 2, "extracted").unwrap();
    for flag in [true, false] {
        assert_eq!(
            run(source, vec![Value::Bool(flag)]),
            run(&extracted.source, vec![Value::Bool(flag)])
        );
    }
    assert_eq!(
        tellur::extract::extract_tail(source, manifest(), "main", 4, "extracted")
            .unwrap_err()
            .code,
        "E_EXTRACT"
    );
    let source = "control main() exits done {\n    [entry]: {\n        string message=\"kept\";\n    }\n    emit(message);\n    exit done;\n}\n";
    let extracted =
        tellur::extract::extract_tail(source, manifest(), "main", 5, "extracted").unwrap();
    assert_eq!(run(source, vec![]), run(&extracted.source, vec![]));
}

#[test]
fn native_groups_preserve_reference_and_expanded_selection() {
    let program = compile(GROUPED, manifest()).unwrap();
    let expanded = program.expanded("main").unwrap();
    let root = std::env::temp_dir().join(format!("tellur-group-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (index, program) in [program.clone(), expanded].iter().enumerate() {
        for opt in [0, 2] {
            let binary = root.join(format!("group-{index}-{opt}"));
            support::scenario_executable(program)
                .build_native("main", &binary, opt)
                .unwrap();
            for flag in [true, false] {
                for brief in [true, false] {
                    let expected = program
                        .run(&scenario(vec![Value::Bool(flag), Value::Bool(brief)]))
                        .unwrap();
                    let actual = std::process::Command::new(&binary)
                        .args([
                            "--json",
                            "--",
                            if flag { "true" } else { "false" },
                            if brief { "true" } else { "false" },
                        ])
                        .output()
                        .unwrap();
                    assert!(actual.status.success(), "{actual:?}");
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                }
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
