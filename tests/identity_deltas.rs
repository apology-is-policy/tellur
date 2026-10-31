mod support;
use tellur::{
    compile,
    manifest::Manifest,
    runtime::{Event, Scenario},
};

fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str) -> tellur::runtime::Execution {
    let program = compile(source, manifest()).unwrap();
    program.verify().unwrap();
    program
        .run(&Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        })
        .unwrap()
}

const PRODUCER: &str = r#"
control request(bool binary) exits has_image {string data}, has_message {string text} {
    delta binary {true=>image,false=>message}
    [image]: exit has_image {"pixels"};
    [message]: exit has_message {"message"};
}
"#;

#[test]
fn omitted_arms_bind_public_payloads_and_grouped_arms_export_path_values() {
    for (binary, expected) in [("true", "pixels"), ("false", "message")] {
        let source = format!(
            r#"{PRODUCER}
control main() exits done {{
    delta request({binary}) {{
        has_image {{auto data}} => {{
            string img = data;
        }}
    }}
    [has_image]: emit(img); exit done;
    [has_message]: emit(text); exit done;
}}"#
        );
        assert_eq!(
            run(&source).events,
            vec![Event::Emit {
                message: expected.into()
            }]
        );
    }
}

#[test]
fn identity_decision_has_no_implicit_exit_or_discarded_history() {
    let source = format!(
        "{PRODUCER} control main() exits done {{delta request(false); [has_image]: emit(data);exit done;}}"
    );
    assert_eq!(
        compile(&source, manifest()).unwrap_err().code,
        "E_UNCOVERED"
    );
    let source = source.replace(
        "[has_image]: emit(data);exit done;",
        "[has_image]: emit(data);exit done; [has_message]:emit(text);exit done;",
    );
    assert_eq!(
        run(&source).events,
        vec![Event::Emit {
            message: "message".into()
        }]
    );
}

#[test]
fn site_paths_disambiguate_repeated_outcomes_without_changing_selections() {
    let source = r#"
control mark(bool input) exits yes, no {
    exit delta input {true=>yes {},false=>no {}};
}
control main() exits done {
    first = delta mark(true);
    second = delta mark(false);
    [first.yes & second.no]: emit("selected"); exit done;
    [first.no | second.yes]: exit done;
}"#;
    assert_eq!(
        run(source).events,
        vec![Event::Emit {
            message: "selected".into()
        }]
    );
    assert_eq!(
        compile(
            &source.replace("first.yes & second.no", "yes & second.no"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_PATH_AMBIGUOUS"
    );
}

#[test]
fn grouped_arm_dependencies_and_source_faults_are_checked_transitively() {
    let source = r#"
control main() exits done {
    delta true {true=>{helper();},false=>exit done}
}
control helper() faults {fault "nested";}
"#;
    let p = compile(source, manifest()).unwrap();
    assert!(p.region("main").unwrap().faults);
    assert_eq!(
        p.run(&Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![]
        })
        .unwrap_err()
        .message,
        "nested"
    );
}

#[test]
fn grouped_arm_borrow_remains_live_in_following_selected_statements() {
    let source = r#"
control peek(ref string input) exits found {ref string view from input} {
    exit found {input};
}
control main() exits done {
    mutable string owner = "kept";
    delta peek(borrow owner) {found {ref auto view}=>{emit(clone(view));}}
    [found]: owner = "changed"; emit(clone(view)); exit done;
}"#;
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, "E_LOAN");
    assert_eq!(
        run(&source.replace("owner = \"changed\";", ""))
            .events
            .len(),
        2
    );
}

#[test]
fn identity_group_formatting_round_trips_without_changing_tokens() {
    let source = format!(
        "{PRODUCER} control main() exits done {{delta request(true){{has_image{{auto data}}=>{{emit(data);exit done;}}}}[has_message]:emit(text);exit done;}}"
    );
    let formatted = tellur::format::source(&source, tellur::format::Options::default()).unwrap();
    assert_eq!(run(&source).events, run(&formatted).events);
}

#[test]
fn sibling_arm_groups_keep_each_others_payload_loans_available() {
    let source = r#"
control choose(bool flag, ref string input)
    exits left {ref string a from input}, right {ref string b from input}
{
    delta flag {true=>first,false=>second}
    [first]:exit left {input};
    [second]:exit right {input};
}
control main() exits done {
    string owner="kept";
    delta choose(false,borrow owner) {
        left {ref auto a}=>{emit(clone(a));},
        right {ref auto b}=>{emit(clone(b));}
    }
    [left|right]:exit done;
}"#;
    assert_eq!(
        run(source).events,
        vec![Event::Emit {
            message: "kept".into()
        }]
    );
}

#[test]
fn inferred_names_are_preserved_when_consistent_and_fall_back_when_ambiguous() {
    let source = r#"
control producer() exits auto {string text="inferred";exit ready {text};}
control main() exits done {delta producer();[ready]:emit(text);exit done;}
"#;
    assert_eq!(
        run(source).events,
        vec![Event::Emit {
            message: "inferred".into()
        }]
    );
    let source = r#"
control producer(bool flag) exits auto {
    delta flag {true=>a,false=>b}
    [a]:string left="left";exit ready {left};
    [b]:string right="right";exit ready {right};
}
control main() exits done {delta producer(false);[ready]:emit(value_0);exit done;}
"#;
    assert_eq!(
        run(source).events,
        vec![Event::Emit {
            message: "right".into()
        }]
    );
    let source = r#"
control producer() exits ready {string text} {exit ready {"forwarded"};}
control relay() exits auto {forward producer();}
control main() exits done {delta relay();[ready]:emit(text);exit done;}
"#;
    assert_eq!(
        run(source).events,
        vec![Event::Emit {
            message: "forwarded".into()
        }]
    );
    let source = r#"
control producer() exits auto {u32 n=7;exit pair {n,n};}
control main() exits done {
    delta producer();
    [pair]: delta (value_0 == value_1) {true=>exit done,false=>fault "wrong payload"}
}
"#;
    assert_eq!(run(source).outcome, "done");
    let source = r#"
control producer() exits auto {
    string left="left";
    string right="right";
    auto result=delta false {true=>ready {left},false=>ready {right}};
    exit result;
}
control main() exits done {delta producer();[ready]:emit(value_0);exit done;}
"#;
    assert_eq!(
        run(source).events,
        vec![Event::Emit {
            message: "right".into()
        }]
    );
}

#[test]
fn imported_implicit_bindings_take_precedence_over_same_named_declarations() {
    use std::{fs, path::Path};
    let root = std::env::temp_dir().join(format!("tellur-identity-module-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("library.tellur"),
        "pub control read() exits ready {string message} {exit ready {\"module\"};}",
    )
    .unwrap();
    let source = "import \"library.tellur\" as library; struct message {} control main() exits done {delta library.read();[ready]:emit(message);exit done;}";
    let path = root.join("main.tellur");
    fs::write(&path, source).unwrap();
    let p = tellur::modules::compile_file(Path::new(&path), manifest()).unwrap();
    assert_eq!(
        p.run(&Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![]
        })
        .unwrap()
        .events,
        vec![Event::Emit {
            message: "module".into()
        }]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_and_expanded_grouped_identity_decisions_match_reference() {
    use std::{fs, process::Command};
    let source = format!(
        "{PRODUCER} control main(bool flag) exits done {{delta request(flag){{has_image{{auto data}}=>{{string img=data;}}}}[has_image]:emit(img);exit done;[has_message]:emit(text);exit done;}}"
    );
    let p = compile(&source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("tellur-identity-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let binary = root.join("run");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    for flag in [true, false] {
        let scenario = Scenario {
            entry: "main".into(),
            arguments: vec![tellur::ir::Value::Bool(flag)],
            calls: vec![],
        };
        let expected = p.run(&scenario).unwrap();
        assert_eq!(
            p.expanded("main").unwrap().run(&scenario).unwrap(),
            expected
        );
        let input = root.join("scenario.json");
        fs::write(&input, serde_json::to_vec(&scenario).unwrap()).unwrap();
        let output = Command::new(&binary)
            .args(["--scenario", input.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    fs::remove_dir_all(root).unwrap();
}
