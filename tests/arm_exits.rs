mod support;
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Execution, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str, arguments: Vec<Value>) -> Execution {
    let program = compile(source, manifest()).unwrap();
    let input = Scenario {
        entry: "main".into(),
        arguments,
        calls: vec![],
    };
    let result = program.run(&input).unwrap();
    assert_eq!(
        result,
        program.expanded("main").unwrap().run(&input).unwrap()
    );
    result
}
fn rejects(source: &str, code: &str) {
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, code, "{error}");
}
const MIXED: &str = r#"control main(bool flag,string input) exits done {string value},absent {
    delta flag {true=>present,false=>exit absent}
    [present]: exit done {input};
}"#;
#[test]
fn direct_exits_preserve_mixed_path_routing_and_inferred_contracts() {
    let flat = MIXED
        .replace("false=>exit absent", "false=>missing")
        .replace("[present]:", "[missing]:exit absent;\n[present]:");
    for flag in [false, true] {
        let args = vec![Value::Bool(flag), Value::String("hello".into())];
        let result = run(MIXED, args.clone());
        assert_eq!(result, run(&flat, args.clone()));
        assert_eq!(
            result,
            run(
                &MIXED.replace("exits done {string value},absent", "exits auto"),
                args
            )
        );
    }
    rejects(&MIXED.replace("[present]:", "[entry]:"), "E_TERMINATED");
    rejects(
        &MIXED.replace("false=>exit absent", "false=>exit unknown"),
        "E_EXIT",
    );
    rejects(
        &MIXED.replace("false=>exit absent", "false=>exit done {false}"),
        "E_TYPE",
    );
    rejects(&MIXED.replace(",false=>exit absent", ""), "E_UNCOVERED");
}
#[test]
fn terminating_payloads_have_private_scopes_and_do_not_export_labels() {
    let source = r#"variant choice {number {u32 value},text {string value}}
control main(choice input) exits number...,text... {
 delta input {number {auto value}=>exit number {value},text {auto value}=>exit text {value}}
}"#;
    for (name, value) in [
        ("number", Value::U32(7)),
        ("text", Value::String("hello".into())),
    ] {
        let result = run(
            source,
            vec![Value::Construct {
                ty: "choice".into(),
                name: name.into(),
                fields: vec![value.clone()],
            }],
        );
        assert_eq!(result.outcome, name);
        assert_eq!(result.fields, vec![value]);
    }
    let source = "control main(option<string> input) exits done {string value} {delta input {some {auto text}=>exit done {text},none=>empty} [empty]: string text=\"absent\";exit done {text};}";
    run(
        source,
        vec![Value::Construct {
            ty: "Option<String>".into(),
            name: "None".into(),
            fields: vec![],
        }],
    );
    rejects(&source.replace("string text=\"absent\";", ""), "E_NAME");
    rejects(&MIXED.replace("[present]:", "[false]:"), "E_PATH");
}
#[test]
fn direct_exits_keep_resource_obligations_and_payload_transfers() {
    rejects(
        "control main(bool flag,handle file) exits done {delta flag {true=>exit done,false=>exit done}}",
        "E_RESOURCE",
    );
    let source = "control main(option<handle> input) exits held {handle file},empty {delta input {some {auto file}=>exit held {file},none=>exit empty}}";
    for present in [false, true] {
        let fields = if present {
            vec![Value::Opaque {
                ty: "handle".into(),
                token: "file".into(),
            }]
        } else {
            vec![]
        };
        let result = run(
            source,
            vec![Value::Construct {
                ty: "Option<handle>".into(),
                name: if present { "Some" } else { "None" }.into(),
                fields: fields.clone(),
            }],
        );
        assert_eq!(result.fields, fields);
    }
    rejects(
        &source.replace("exit held {file}", "exit empty"),
        "E_RESOURCE",
    );
}
const BORROWED: &str = r#"control pick(ref mutable string input,bool flag) exits found {ref mutable string value from input},missing {
    delta flag {true=>exit found {input},false=>exit missing}
}
control main(bool flag) exits done {
    mutable string owner="before";
    delta pick(borrow mutable owner,flag) {found {auto value}=>present,missing=>absent}
    [present]: deref value="after";emit(value);
    [absent]: emit(owner);
    [entry]: exit done;
}"#;
#[test]
fn returned_borrows_retain_origins_and_branch_sensitive_last_uses() {
    for flag in [false, true] {
        run(BORROWED, vec![Value::Bool(flag)]);
    }
    rejects(
        &BORROWED.replace("deref value=\"after\";", "owner=\"early\";"),
        "E_LOAN",
    );
    rejects(
        "control main(bool flag) exits auto {string text=\"local\";delta flag {true=>exit found {borrow text},false=>exit missing}}",
        "E_LIFETIME",
    );
}
#[test]
fn stored_outcomes_nested_adapters_and_nullary_disambiguation_match_exit_statements() {
    let source = "outcome reply {done,missing} control main(bool flag) exits reply {reply saved=done {}; delta flag {true=>exit saved,false=>exit delta saved {done=>missing {},missing=>done {}}}}";
    for flag in [false, true] {
        assert_eq!(
            run(source, vec![Value::Bool(flag)]).outcome,
            if flag { "done" } else { "missing" }
        );
    }
    let shadow = "control main(bool flag) exits done {u32 done=1;delta flag {true=>exit done {},false=>exit done {}}}";
    for flag in [false, true] {
        run(shadow, vec![Value::Bool(flag)]);
    }
    rejects(&shadow.replace("exit done {}", "exit done"), "E_OUTCOME");
}
#[test]
fn paths_named_exit_and_legacy_decisions_remain_compatible() {
    let source = "control main(bool flag) exits done {delta flag {true=>exit,false=>other} [exit|other]:exit done;}";
    for flag in [false, true] {
        run(source, vec![Value::Bool(flag)]);
        run(
            "region main(flag: Bool) exits Done {[entry]:delta flag -> [true=>exit Done,false=>exit Done];}",
            vec![Value::Bool(flag)],
        );
    }
    rejects(
        "control main() exits done {delta true {true=>exit done;,false=>exit done}}",
        "E_PARSE",
    );
}
#[test]
fn grouped_and_looping_exits_keep_the_enclosing_control_contract() {
    let source = "control main(bool flag) exits done {loop once(bool state=flag) {[entry]: {delta state {true=>exit done,false=>retry}} [retry]:again once(true);}}";
    for flag in [false, true] {
        run(source, vec![Value::Bool(flag)]);
    }
    rejects(&source.replace("exit done", "exit unknown"), "E_EXIT");
}
#[test]
fn imported_generic_exit_adapters_are_traversed_and_ranges_map_back() {
    let root = std::env::temp_dir().join(format!("tellur-arm-module-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let library = "pub control select<t>(t input,bool flag) exits ready {t value},missing {\n delta flag {true=>exit delta identity<t>(input) {ready {auto value}=>ready {value}},false=>exit missing}\n}\ncontrol identity<t>(t input) exits auto {exit ready {input};}";
    // Unknown generic t cannot be silently dropped on the false arm.
    std::fs::write(
        root.join("library.tellur"),
        library
            .replace("<t>", "<t: drop>")
            .replace("identity<t: drop>", "identity<t>"),
    )
    .unwrap();
    std::fs::write(root.join("main.tellur"),"import \"library.tellur\" as lib; control main() exits auto {forward lib.select<string>(\"hello\",true);}").unwrap();
    let p = tellur::compile_file(root.join("main.tellur"), manifest()).unwrap();
    assert_eq!(
        p.run(&Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![]
        })
        .unwrap()
        .fields,
        vec![Value::String("hello".into())]
    );
    let bad = library
        .replace("<t>", "<t: drop>")
        .replace("identity<t: drop>", "identity<t>")
        .replace("ready {value}", "ready {false}");
    std::fs::write(root.join("library.tellur"), bad).unwrap();
    let error = tellur::compile_file(root.join("main.tellur"), manifest()).unwrap_err();
    assert_eq!(
        error.path,
        root.join("library.tellur").canonicalize().unwrap()
    );
    assert_eq!(error.diagnostic.span.line, 2);
    assert!(error.diagnostic.end.is_some());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn extraction_keeps_exit_payload_locals_and_external_inputs_distinct() {
    let source = "control main(bool flag,string text) exits done {string value},missing {\n delta flag {true=>ready,false=>exit missing}\n [ready]:exit done {text};\n}";
    let extracted = tellur::extract::extract_tail(source, manifest(), "main", 2, "helper").unwrap();
    for flag in [false, true] {
        let args = vec![Value::Bool(flag), Value::String("hello".into())];
        assert_eq!(run(source, args.clone()), run(&extracted.source, args));
    }
}
#[test]
fn native_direct_exits_agree_before_and_after_expansion() {
    let root = std::env::temp_dir().join(format!("tellur-arm-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (case, source) in [MIXED, BORROWED].iter().enumerate() {
        let p = compile(source, manifest()).unwrap();
        for (mode, p) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
            for opt in [0, 2] {
                let binary = root.join(format!("{case}-{mode}-{opt}"));
                support::scenario_executable(p)
                    .build_native("main", &binary, opt)
                    .unwrap();
                for flag in [false, true] {
                    let mut args = vec![Value::Bool(flag)];
                    let mut cli = vec!["--json", "--", if flag { "true" } else { "false" }];
                    if case == 0 {
                        args.push(Value::String("hello".into()));
                        cli.push("hello");
                    }
                    let actual = std::process::Command::new(&binary)
                        .args(cli)
                        .output()
                        .unwrap();
                    assert!(actual.status.success(), "{actual:?}");
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                        serde_json::to_value(run(source, args)).unwrap()
                    );
                }
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_arm_adapters_participate_in_failed_caller_suppression() {
    let source = "control main() exits done {delta true {true=>exit delta bad() {done=>done {}},false=>exit done}}\ncontrol bad() exits done {u32 value=false;exit done;}\ncontrol other() exits done {string value=1;exit done;}";
    let errors = tellur::check_source(source, manifest()).unwrap_err();
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
}

#[test]
fn mixed_payload_names_have_the_same_meaning_in_either_arm_order() {
    let prefix = "variant choice {number {u32 value},text {string value}} control main(choice input) exits number...,text... {delta input {";
    let suffix = "} [number]:exit number {value};}";
    let a = format!(
        "{prefix}number {{auto value}}=>number,text {{auto value}}=>exit text {{value}}{suffix}"
    );
    let b = format!(
        "{prefix}text {{auto value}}=>exit text {{value}},number {{auto value}}=>number{suffix}"
    );
    for (name, value) in [
        ("number", Value::U32(7)),
        ("text", Value::String("hello".into())),
    ] {
        let args = vec![Value::Construct {
            ty: "choice".into(),
            name: name.into(),
            fields: vec![value],
        }];
        assert_eq!(run(&a, args.clone()), run(&b, args));
    }
    let shared = r#"variant choice {left {ref string value},right {ref string value}}
control main() exits done {string result} {
 string text="hello";
 choice input=choice.left {borrow text};
 delta input {left {auto value}=>selected,right {auto value}=>exit done {clone(value)}}
 [selected]:exit done {clone(value)};
}"#;
    for constructor in ["choice.left", "choice.right"] {
        let source = shared.replace("choice.left", constructor);
        let reversed = source.replace(
            "left {auto value}=>selected,right {auto value}=>exit done {clone(value)}",
            "right {auto value}=>exit done {clone(value)},left {auto value}=>selected",
        );
        assert_eq!(run(&source, vec![]), run(&reversed, vec![]));
    }
    // A name that actually existed before the decision remains an outer binding.
    rejects(
        &a.replace("{delta input", "{string value=\"outer\";delta input"),
        "E_SHADOW",
    );
}
