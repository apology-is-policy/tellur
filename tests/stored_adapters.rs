mod support;
use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
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
const NESTED: &str = r#"
control main(bool flag) exits auto {
    string prefix = "owned";
    auto result = delta choose(flag) {
        yes => [accepted]: delta choose(flag) {
            yes => ready {prefix}, no => empty {}
        },
        no => ready {prefix}
    };
    emit("stored");
    exit result;
}
control choose(bool flag) exits auto {
    emit("choose");
    exit delta flag {true => yes {}, false => no {}};
}
"#;
#[test]
fn nested_storage_preserves_values_and_selected_effect_order() {
    for flag in [true, false] {
        let r = run(NESTED, flag);
        assert_eq!(r.outcome, "ready");
        assert_eq!(r.fields, vec![Value::String("owned".into())]);
        let messages = r
            .events
            .iter()
            .map(|event| match event {
                tellur::runtime::Event::Emit { message } => message.as_str(),
                _ => panic!("unexpected primitive effect"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            if flag {
                vec!["choose", "choose", "stored"]
            } else {
                vec!["choose", "stored"]
            }
        );
    }
}
#[test]
fn typed_adapter_targets_existing_contract_declared_later() {
    let source = r#"control main(bool flag) exits auto {
        outcome<contract> result = delta flag {true => ready {["hello"]},false => empty {}};
        exit delta forward(result) {ready {auto words}=>done {words},empty=>missing {},failed=>failed {}};
    }
    control forward(outcome<contract> result) exits auto {exit result;}
    control contract() exits ready {vector<string> words}, empty, failed {exit empty;}
    "#;
    assert_eq!(run(source, true).outcome, "done");
    assert_eq!(run(source, false).outcome, "missing");
    rejects(&source.replace("empty {}", "unknown {}"), "E_OUTCOME");
    rejects(
        &source.replace("ready {[\"hello\"]}", "ready {3}"),
        "E_TYPE",
    );
}
#[test]
fn borrowed_inputs_can_be_adapted_to_owned_storage() {
    let source = r#"control main(bool flag) exits auto {
        mutable vector<string> words=["hello"];
        auto result = delta vector_get(&words,0) {
            found {&string word}=>ready {clone(word)},out_of_bounds=>empty {}
        };
        delta vector_push(&mutable words,"later") {pushed=>added}
        [added]: exit result;
    }"#;
    assert_eq!(
        run(source, true).fields,
        vec![Value::String("hello".into())]
    );
    rejects(&source.replace("clone(word)", "word"), "E_LOAN");
}
#[test]
fn result_must_be_handled_and_is_not_copyable() {
    let source = "control main(bool flag) exits auto {auto result=delta flag {true=>ready {},false=>empty {}};exit result;}";
    rejects(&source.replace("exit result;", "exit done;"), "E_OUTCOME");
    rejects(
        &source.replace("exit result;", "drop(result);exit done;"),
        "E_OUTCOME",
    );
    rejects(
        &source.replace("exit result;", "auto copy=clone(&result);exit result;"),
        "E_OUTCOME",
    );
    rejects(
        &source.replace("exit result;", "auto moved=result;exit result;"),
        "E_MOVED",
    );
    rejects(&source.replace("auto result=", "u32 result="), "E_OUTCOME");
}
#[test]
fn resource_payloads_must_be_forwarded_from_each_lexical_arm() {
    let source = r#"control main(handle file,bool flag) exits auto {
        option<handle> input=some {file};
        auto result=delta input {some {auto owned}=>delta flag {true=>ready {owned},false=>ready {owned}},none=>empty {}};
        exit result;
    }"#;
    compile(source, manifest()).unwrap();
    rejects(
        &source.replace("false=>ready {owned}", "false=>empty {}"),
        "E_RESOURCE",
    );
    rejects(
        &source.replace("true=>ready {owned}", "true=>ready {owned,owned}"),
        "E_MOVED",
    );
}
#[test]
fn branches_agree_on_payload_shape_and_bindings_do_not_escape() {
    rejects(
        "control main(bool flag) exits auto {auto r=delta flag {true=>ready {1},false=>ready {\"bad\"}};exit r;}",
        "E_TYPE",
    );
    rejects(
        "control main(bool flag) exits auto {auto r=delta flag {true=>ready {1},false=>ready {}};exit r;}",
        "E_ARITY",
    );
    rejects(
        "control main(bool flag) exits auto {auto r=delta flag {true=>ready {},false=>empty {}};[true]: exit r;}",
        "E_PATH",
    );
    let source = "control main(bool flag) exits auto {auto r=delta flag {true=>ready {1},false=>empty {}};exit delta r {ready {auto n}=>done {n}};}";
    rejects(source, "E_ARMS");
}
#[test]
fn loop_activation_reinitializes_stored_adapter_slots() {
    let source = r#"control main(bool flag) exits auto {
        loop next(bool stop=false) {
            auto r=delta stop {true=>done {3},false=>more {}};
            delta r {done {auto total}=>finished,more=>continue}
            [finished]: exit done {total};
            [continue]: again next(true);
        }
    }"#;
    assert_eq!(run(source, true).fields, vec![Value::U32(3)]);
}
#[test]
fn owning_outcome_can_itself_be_an_adapter_payload() {
    let source = r#"control main(bool flag) exits auto {
        auto first=delta flag {true=>ready {"hello"},false=>empty {}};
        auto second=delta flag {true=>wrapped {first},false=>wrapped {first}};
        exit delta second {wrapped {auto result}=>delta result {ready {auto text}=>done {text},empty=>missing {}}};
    }"#;
    assert_eq!(
        run(source, true).fields,
        vec![Value::String("hello".into())]
    );
    assert_eq!(run(source, false).outcome, "missing");
}
#[test]
fn native_and_expanded_storage_match_at_both_optimization_levels() {
    let p = compile(NESTED, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("path-stored-adapters-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (index, program) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        for opt in [0, 2] {
            let binary = root.join(format!("adapter-{index}-{opt}"));
            support::scenario_executable(program)
                .build_native("main", &binary, opt)
                .unwrap();
            for flag in [true, false] {
                let expected = run(NESTED, flag);
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
