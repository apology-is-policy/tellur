use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
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
const SOURCE: &str = include_str!("../playground/outcome_spawn.tellur");
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    p
}
#[test]
fn outcome_payloads_preserve_selected_and_empty_schedules() {
    let p = same(SOURCE);
    let expected = p.run_completion(&scenario(), 1000).unwrap();
    let v = serde_json::to_value(&expected).unwrap();
    let messages: Vec<_> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(messages, ["left", "right", "empty", "edited", "none"]);
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let input = scenario();
            let mut session = p.start_execution(&input, 1000);
            assert!(matches!(session.advance(0), ExecutionProgress::Paused));
            let mut done = false;
            for _ in 0..1000 {
                match session.advance(quota) {
                    ExecutionProgress::Paused => {}
                    ExecutionProgress::Complete(result) => {
                        assert_eq!(result.unwrap(), expected);
                        done = true;
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            }
            assert!(done);
        }
    }
}
#[test]
fn outcome_permissions_survive_queue_completion_join_and_selection() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoff);
        same(&source);
        for (marker, bad) in [
            ("auto moved = move pending;", "left=\"bad\";"),
            ("auto moved = move pending;", "right=\"bad\";"),
            ("emit(a);", "left=\"bad\";"),
            ("emit(b);", "right=\"bad\";"),
            ("delta await child", "owner=\"bad\";"),
            ("deref text = \"edited\";", "owner=\"bad\";"),
        ] {
            assert!(source.contains(marker), "{marker}");
            assert_eq!(
                compile(
                    &source.replace(marker, &format!("{bad}{marker}")),
                    manifest()
                )
                .unwrap_err()
                .code,
                "E_LOAN",
                "{marker}"
            );
        }
    }
}
#[test]
fn outcome_result_guards_keep_parent_and_carrier_authority() {
    let source = include_str!("fixtures/source_outcome_guards.tellur");
    same(source);
    for marker in [
        "auto moved = move pending;",
        "deref value.item.text = \"after\";",
    ] {
        for bad in [
            "drop(saved);",
            "owner=\"bad\";",
            "deref carrier.item.text=\"bad\";",
            "deref detached=\"bad\";",
        ] {
            assert_eq!(
                compile(
                    &source.replacen(marker, &format!("{bad}{marker}"), 1),
                    manifest()
                )
                .unwrap_err()
                .code,
                "E_LOAN",
                "{bad}"
            );
        }
    }
}
#[test]
fn outcome_results_settle_before_awaited_owner_contingency() {
    let source = include_str!("fixtures/source_outcome_fault.tellur");
    let joined = source
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    assert_ne!(joined, source);
    for source in [source, joined.as_str()] {
        let p = compile(source, manifest()).unwrap();
        for p in [p.clone(), p.expanded("main").unwrap()] {
            let v = serde_json::to_value(p.run_completion(&scenario(), 1000).unwrap()).unwrap();
            assert_eq!(v["status"], 23);
            let messages: Vec<_> = v["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|e| e["message"].as_str())
                .collect();
            assert_eq!(
                messages,
                ["result completed", "sibling started", "owner contingency"]
            );
        }
    }
}
#[test]
fn outcome_captures_and_results_always_transfer_handling_obligations() {
    let source = r#"
outcome reply_value['a] {value {ref string text <- 'a},empty}
async control relay(reply_value input) exits ready {reply_value[input['a]] result} {yield;exit ready {input};}
async control startup() exits done {
    string owner="text";reply_value input=reply_value.value {borrow owner};
    auto child=spawn relay(move input);yield;yield;
    delta await child {ready {auto result}=>ready}
    [ready]:delta result {value {auto text}=>value,empty=>empty}
    [value]:emit(text);[value|empty]:exit done;
}
control main() exits done {forward run startup();}
"#;
    for source in [
        source.to_owned(),
        source.replace("reply_value.value {borrow owner}", "reply_value.empty {}"),
    ] {
        same(&source);
        for (body, code) in [
            ("exit done;", "E_OUTCOME"),
            ("drop(result);exit done;", "E_RESOURCE"),
            ("auto duplicate=clone(borrow result);exit done;", "E_CLONE"),
            ("auto wrapped=some {result};exit done;", "E_OUTCOME"),
        ] {
            let a = source.find("    [ready]:delta result").unwrap();
            let bad = format!(
                "{}[ready]:{body}}}\ncontrol main() exits done {{forward run startup();}}",
                &source[..a]
            );
            assert_eq!(compile(&bad, manifest()).unwrap_err().code, code, "{body}");
        }
        assert_eq!(
            compile(
                &source.replace(
                    "yield;yield;",
                    "delta input {value {auto old}=>old,empty=>empty} [old|empty]:yield;yield;"
                ),
                manifest()
            )
            .unwrap_err()
            .code,
            "E_MOVED"
        );
    }
}
#[test]
fn outcome_nested_fields_and_forwarding_keep_selected_origins() {
    let source = SOURCE.replace("async control relay<t>", "async control leaf<t>");
    let source = format!(
        "{source}\nasync control relay<t>(request<t> input) exits ready {{request<t>[input['left],input['right]] value}} {{auto child=spawn leaf<t>(input);forward await child;}}"
    );
    same(&source);
    let broken = source.replace(
        "auto selected = move value.selected;",
        "drop(value.selected);auto selected = move value.selected;",
    );
    assert_eq!(compile(&broken, manifest()).unwrap_err().code, "E_RESOURCE");
    let source = r#"
outcome answer['a] {ready {ref mutable string value <- 'a},empty}
async control relay(option<answer> input) exits ready {option<answer>[input['a]] value} {yield;exit ready {input};}
async control start() exits done {mutable string owner="before";auto child=spawn relay(some {answer.ready {borrow mutable owner}});delta await child {ready {auto result}=>ready} [ready]:delta result {some {auto answer}=>some,none=>none} [some]:delta answer {ready {auto text}=>text,empty=>empty} [text]:deref text="after";emit(text);[text|empty|none]:owner="free";exit done;}
control main() exits done {forward run start();}
"#;
    same(source);
    assert_eq!(
        compile(
            &source.replace("deref text=", "owner=\"bad\";deref text="),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}
#[test]
fn anonymous_contract_bounds_and_borrowed_outcome_admission_are_distinct() {
    let anonymous = "control view(ref string input) exits found {ref string text from input} {exit found {input};}async control relay(outcome<view> input) exits auto {exit input;}async control start(outcome<view> input) exits done {auto child=spawn relay(input);delta await child {found {auto value}=>found} [found]:emit(value);exit done;}";
    for source in [anonymous.to_owned(), anonymous.replace("spawn ", "defer ")] {
        assert_eq!(compile(&source, manifest()).unwrap_err().code, "E_MANIFEST");
    }
    let borrowed = "outcome result['a] {ready {ref string value <- 'a}}async control work(ref result input) exits done {exit done;}async control start(ref result input) exits done {auto child=spawn work(input);await child;exit done;}";
    compile(&borrowed.replace("spawn ", "defer "), manifest()).unwrap();
    compile(borrowed, manifest()).unwrap();
}
#[test]
fn named_outcome_identity_and_no_implicit_copy_survive_spawn() {
    let source = r#"
outcome first['a] {ready {ref string value <- 'a},empty}
outcome second['a] {ready {ref string value <- 'a},empty}
async control relay(first input) exits ready {first[input['a]] value} {yield;exit ready {input};}
async control start() exits done {string owner="shared";first input=first.ready {borrow owner};auto child=spawn relay(input);delta await child {ready {auto value}=>ready} [ready]:delta value {ready {auto text}=>text,empty=>empty} [text]:emit(text);[text|empty]:exit done;}
control main() exits done {forward run start();}
"#;
    same(source);
    assert_eq!(
        compile(
            &source.replace("first input=first.ready", "second input=second.ready"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_TYPE"
    );
    assert_eq!(
        compile(
            &source.replace(
                "delta await child",
                "auto again=spawn relay(input);await again;delta await child"
            ),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_MOVED"
    );
}
