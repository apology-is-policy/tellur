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
const SOURCE: &str = include_str!("../playground/carrier_spawn_results.tellur");
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    p
}
#[test]
fn flat_results_preserve_field_loans_and_schedules_across_join() {
    let p = same(SOURCE);
    let expected = p.run_completion(&scenario(), 1000).unwrap();
    let value = serde_json::to_value(&expected).unwrap();
    let messages: Vec<_> = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "queued",
            "left",
            "right",
            "owned result",
            "edited in child",
            "released after result"
        ]
    );
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
fn carrier_results_hold_backing_in_queued_running_completed_and_adopted_phases() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let text = SOURCE.replace("    yield;\n    yield;", handoff);
        same(&text);
        for (marker, bad) in [
            ("auto moved = move first;", "left=\"bad\";"),
            ("auto moved = move first;", "text=\"bad\";"),
            ("emit(result.left);", "left=\"bad\";"),
            ("emit(result.right);", "right=\"bad\";"),
            ("emit(edited.text);", "text=\"bad\";"),
        ] {
            assert_eq!(
                compile(&text.replace(marker, &format!("{bad}{marker}")), manifest())
                    .unwrap_err()
                    .code,
                "E_LOAN",
                "{marker}"
            );
        }
    }
}
#[test]
fn carrier_results_keep_detached_guards_and_parent_authority() {
    let source = include_str!("fixtures/source_carrier_result_guards.tellur");
    same(source);
    for marker in [
        "auto moved = move pending;",
        "deref value.text = \"after\";",
    ] {
        for bad in [
            "drop(saved);",
            "owner=\"bad\";",
            "deref carrier.text=\"bad\";",
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
fn completed_and_joined_carrier_results_settle_before_owning_contingency() {
    let source = include_str!("fixtures/source_carrier_result_fault.tellur");
    let joined = source
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    assert_ne!(source, joined);
    for source in [source, joined.as_str()] {
        let p = compile(source, manifest()).unwrap();
        for p in [p.clone(), p.expanded("main").unwrap()] {
            let value = serde_json::to_value(p.run_completion(&scenario(), 1000).unwrap()).unwrap();
            assert_eq!(value["status"], 23);
            let messages: Vec<_> = value["events"]
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
fn generic_carrier_result_alternatives_adopt_only_selected_origins() {
    let source = r#"
struct view<t>['a] {ref t text <- 'a;}
async control select<t>(ref t a,ref t b,bool yes) exits first {view<t>[a] value}, second {view<t>[b] value} {
    yield;delta yes {true=>yes,false=>no} [yes]:exit first {view<t> {a}};[no]:exit second {view<t> {b}};
}
async control relay<t>(ref t a,ref t b,bool yes) exits first {view<t>[a] value}, second {view<t>[b] value} {
    auto child=spawn select<t>(a,b,yes);forward await child;
}
async control startup() exits done {
    mutable string a="a";mutable string b="b";
    auto child=spawn relay<string>(borrow a,borrow b,true);
    auto result=capture await child;
    delta result {first {auto first}=>first,second {auto second}=>second}
    [first]:b="free";emit(first.text);a="free";exit done;
    [second]:a="free";emit(second.text);b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    for source in [
        source.to_owned(),
        source.replace("borrow b,true)", "borrow b,false)"),
    ] {
        same(&source);
        for bad in [
            source.replace("[first]:b=", "[first]:a="),
            source.replace("[second]:a=", "[second]:b="),
        ] {
            assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
        }
    }
}
#[test]
fn copying_shared_results_and_moving_exclusive_results_keep_permissions() {
    let copy = SOURCE.replace(
        "emit(result.left);",
        "auto copied=result;emit(result.left);emit(copied.left);",
    );
    same(&copy);
    let moved = SOURCE
        .replace(
            "emit(borrow edited.label);",
            "auto delivered=move edited;emit(borrow delivered.label);",
        )
        .replace("emit(edited.text);", "emit(delivered.text);");
    same(&moved);
    let bad = moved.replace(
        "emit(delivered.text);",
        "emit(edited.text);emit(delivered.text);",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_MOVED");
    let borrowed = SOURCE
        .replace(
            "emit(borrow edited.label);",
            "ref edit_view shared=borrow edited;emit(borrow edited.label);",
        )
        .replace(
            "emit(edited.text);",
            "deref shared.text=\"bad\";emit(borrow deref shared.text);",
        );
    assert_eq!(
        compile(&borrowed, manifest()).unwrap_err().code,
        "E_MUTABILITY"
    );
}
#[test]
fn child_owned_storage_cannot_escape_in_a_result_carrier() {
    for body in [
        "string local=\"local\";exit ready {view {borrow local}};",
        "exit ready {view {borrow owned}};",
    ] {
        let source = format!(
            "struct view['a] {{ref string text <- 'a;}}async control bad(string owned,ref string input) exits ready {{view[input] value}} {{{body}}}"
        );
        assert!(compile(&source, manifest()).is_err());
    }
}
#[test]
fn borrowed_and_nested_result_carriers_stay_gated() {
    for source in [
        "struct view['a] {ref string text <- 'a;} async control bad(ref view input) exits ready {ref view value from input} {exit ready {input};} async control start(ref view input) exits done {auto child=spawn bad(input);await child;exit done;}",
        "struct view['a] {ref string text <- 'a;} struct nested['a] {view['a] value;} async control bad(ref string input) exits ready {nested[input] value} {exit ready {nested {view {input}}};} async control start(ref string input) exits done {auto child=spawn bad(input);await child;exit done;}",
        "async control bad(ref string input) exits ready {option<ref string>[input] value} {exit ready {some {input}};} async control start(ref string input) exits done {auto child=spawn bad(input);await child;exit done;}",
    ] {
        assert_eq!(
            compile(source, manifest()).unwrap_err().code,
            "E_ASYNC",
            "{source}"
        );
    }
}
