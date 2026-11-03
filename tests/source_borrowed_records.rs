use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/borrowed_record_spawn.tellur");
const GUARDS: &str = include_str!("fixtures/source_borrowed_record_guards.tellur");
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
fn run(source: &str) -> Vec<String> {
    let p = compile(source, manifest()).unwrap();
    let expected = p.run_completion(&scenario(), 1000).unwrap();
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
    serde_json::to_value(expected).unwrap()["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str().map(str::to_owned))
        .collect()
}
fn rejects(source: &str, code: &str) {
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, code, "{error}\n{source}");
}
#[test]
fn borrowed_records_keep_holder_and_contents_through_explicit_handoffs() {
    assert_eq!(
        run(SOURCE),
        ["queued", "reader", "released at join", "selected", "edited"]
    );
}
#[test]
fn queued_running_and_completed_unjoined_holders_keep_their_loans() {
    for handoffs in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoffs);
        run(&source);
        for action in ["drop(saved);", "left=\"bad\";", "right=\"bad\";"] {
            rejects(
                &source.replace(
                    "auto moved = move child;",
                    &format!("{action}auto moved = move child;"),
                ),
                "E_LOAN",
            );
        }
        // Joining one reader does not release a second reader's storage loan.
        rejects(
            &source.replace("await other;", "drop(saved);await other;"),
            "E_LOAN",
        );
        rejects(
            &source.replace("emit(value);", "left=\"bad\";emit(value);"),
            "E_LOAN",
        );
        for action in [
            "drop(holder);",
            "text=\"bad\";",
            "deref alias.contents.text=\"bad\";",
        ] {
            rejects(
                &source.replace("await worker;", &format!("{action}await worker;")),
                "E_LOAN",
            );
        }
    }
}
#[test]
fn exclusive_return_keeps_holder_guard_until_last_use_and_transfers_aliases() {
    assert_eq!(run(GUARDS), ["after", "available after result"]);
    for marker in ["auto moved = move pending;", "deref value = \"after\";"] {
        for action in [
            "drop(saved);",
            "owner=\"bad\";",
            "deref carrier.item.text=\"bad\";",
        ] {
            rejects(
                &GUARDS.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
    let moved = GUARDS.replace("spawn lend(carrier)", "spawn lend(move carrier)");
    rejects(&moved, "E_MOVED");
    run(&moved.replace("        deref carrier.item.text = \"available after result\";\n        emit(carrier.item.text);\n",""));
}
#[test]
fn shared_return_from_exclusive_contents_keeps_guard_without_write_authority() {
    let source = GUARDS
        .replace(
            "borrow mutable deref input.item.text",
            "borrow deref input.item.text",
        )
        .replace("deref value = \"after\";", "");
    run(&source);
    rejects(
        &source.replace("emit(value);", "drop(saved);emit(value);"),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(value);", "deref value=\"bad\";emit(value);"),
        "E_MUTABILITY",
    );
}
#[test]
fn disjoint_borrowed_record_fields_keep_independent_exclusive_authority() {
    let source = r#"
struct leaf['a] {ref mutable string text <- 'a;}
struct pair['a,'b] {leaf['a] left;leaf['b] right;}
async control edit(ref mutable leaf value) exits done {yield;deref value.text="edited";exit done;}
async control startup() exits done {
 mutable string a="a";mutable string b="b";
 mutable pair saved=pair {leaf {borrow mutable a},leaf {borrow mutable b}};
 auto left=spawn edit(borrow mutable saved.left);
 auto right=spawn edit(borrow mutable saved.right);
 await left;deref saved.left.text="available";await right;
 emit(saved.left.text);emit(saved.right.text);drop(saved);a="free";b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    assert_eq!(run(source), ["available", "edited"]);
    rejects(
        &source.replace("borrow mutable saved.right", "borrow mutable saved.left"),
        "E_LOAN",
    );
    rejects(
        &source.replace("await left;", "deref saved.left.text=\"bad\";await left;"),
        "E_LOAN",
    );
}
#[test]
fn borrowed_record_faults_settle_joined_and_unjoined_results_before_owning_handler() {
    let source = include_str!("fixtures/source_borrowed_record_fault.tellur");
    for source in [
        source.to_owned(),
        source
            .replace(
                "    yield;\n    yield;\n    delta await result",
                "    delta await result",
            )
            .replace("emit(\"stopped parent resumed\");", "yield;yield;"),
    ] {
        assert_eq!(
            run(&source),
            ["result completed", "sibling started", "owner contingency"]
        );
    }
}
#[test]
fn deeper_references_and_whole_borrowed_record_results_stay_gated() {
    for ty in ["ref indirect"] {
        let source = format!(
            "struct leaf['a] {{ref string text <- 'a;}} variant choice['a] {{yes {{leaf['a] value}}}} outcome reply['a] {{yes {{leaf['a] value}}}} struct tagged['a] {{option<leaf>['a] item;}} struct indirect['storage,'a] {{ref leaf['a] item <- 'storage;}} async control work({ty} input) exits done {{exit done;}} async control caller({ty} input) exits done {{auto child=spawn work(input);await child;exit done;}}"
        );
        rejects(&source, "E_ASYNC");
    }
    rejects(
        "struct leaf['a] {ref string text <- 'a;} async control work(ref leaf input) exits ready {ref leaf value from input} {exit ready {input};} async control caller(ref leaf input) exits done {auto child=spawn work(input);delta await child {ready {auto value}=>done} [done]:exit done;}",
        "E_ASYNC",
    );
}
#[test]
fn generic_borrowed_record_contracts_preserve_selected_contained_origins() {
    let source = r#"
struct pair<t>['a,'b] {ref t left <- 'a;ref t right <- 'b;}
async control select<t>(ref pair<t> input,bool choose) exits left {ref t value from input['a]},right {ref t value from input['b]} {
 yield;delta choose {true=>left,false=>right}
 [left]:exit left {input.left};[right]:exit right {input.right};
}
async control startup() exits done {
 mutable string a="a";mutable string b="b";
 pair<string> saved=pair<string> {borrow a,borrow b};
 auto child=spawn select<string>(borrow saved,true);
 auto result=capture await child;drop(saved);
 delta result {left {auto left}=>left,right {auto right}=>right}
 [left]:b="free";emit(left);a="free";exit done;
 [right]:a="free";emit(right);b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    for choose in ["true", "false"] {
        let source = source.replace("borrow saved,true", &format!("borrow saved,{choose}"));
        run(&source);
        rejects(&source.replace("[left]:b=", "[left]:a="), "E_LOAN");
        rejects(&source.replace("[right]:a=", "[right]:b="), "E_LOAN");
    }
}

#[test]
fn exclusive_holder_guards_are_live_before_and_during_child_execution() {
    for handoffs in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoffs);
        run(&source);
        for action in [
            "drop(saved);",
            "owner=\"bad\";",
            "deref carrier.item.text=\"bad\";",
        ] {
            rejects(
                &source.replace(
                    "auto moved = move pending;",
                    &format!("{action}auto moved = move pending;"),
                ),
                "E_LOAN",
            );
        }
    }
}
