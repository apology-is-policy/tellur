use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/borrowed_outcome_spawn_results.tellur");
const GUARDS: &str = include_str!("fixtures/source_borrowed_outcome_result_guards.tellur");
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
    let error = match compile(source, manifest()) {
        Err(error) => error,
        Ok(_) => panic!("expected {code}, accepted:\n{source}"),
    };
    assert_eq!(error.code, code, "{error}\n{source}");
}
#[test]
fn whole_outcome_results_preserve_selected_empty_and_generic_schedules() {
    assert_eq!(run(SOURCE), ["left", "right", "empty", "edited", "none"]);
    let leaf = SOURCE.replace("async control first<t>", "async control leaf<t>");
    let forwarded = format!(
        "{leaf}\nasync control first<t>(ref choice<t> input) exits ready {{ref choice<t> value from input}} {{auto child=spawn leaf<t>(input);forward await child;}}"
    );
    assert_eq!(
        run(&forwarded),
        ["left", "right", "empty", "edited", "none"]
    );
}
#[test]
fn whole_outcome_holder_and_selected_supplier_loans_survive_child_phases() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("        yield;\n        yield;", handoff);
        run(&source);
        for (marker, action) in [
            (
                "auto moved = move pending;",
                "consume_choice<string>(saved);",
            ),
            ("auto moved = move pending;", "left=\"bad\";"),
            ("auto moved = move pending;", "right=\"bad\";"),
            ("auto copy = move result;", "consume_choice<string>(saved);"),
            ("emit(a);", "left=\"bad\";"),
            ("emit(b);", "right=\"bad\";"),
        ] {
            assert!(source.contains(marker));
            rejects(
                &source.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
}
#[test]
fn shared_whole_outcome_copies_retain_holder_until_matching_then_detach_leaves() {
    let source = SOURCE.replace(
        "auto copy = move result;",
        "auto copy=result;auto other=move result;drop(other);",
    );
    run(&source);
    rejects(
        &source.replace("delta copy {", "consume_choice<string>(saved);delta copy {"),
        "E_LOAN",
    );
}
const SIMPLE: &str = r#"
outcome reply['a] {held {ref mutable string text <- 'a},empty}
control consume(reply input) exits done {delta input {held {auto ignored}=>held,empty=>empty}[entry]:exit done;}
async control lend(ref mutable reply input) exits ready {ref mutable reply[input['a]] value from input} {yield;exit ready {input};}
async control startup() exits done {
 mutable string owner="before";mutable reply saved=reply.held {borrow mutable owner};
 auto child=spawn lend(borrow mutable saved);yield;yield;
 delta await child {ready {auto result}=>ready}
 [ready]:delta result {held {auto text}=>held,empty=>empty}
 [held]:deref text="after";emit(text);
 [entry]:consume(saved);owner="free";exit done;
}
control main() exits done {forward run startup();}
"#;
#[test]
fn returned_outcome_borrows_never_discharge_owning_caller_handling() {
    for mode in [
        SIMPLE.to_owned(),
        SIMPLE
            .replace("ref mutable reply", "ref reply")
            .replace("lend(borrow mutable saved)", "lend(borrow saved)")
            .replace("deref text=\"after\";", ""),
    ] {
        for source in [
            mode.clone(),
            mode.replace("reply.held {borrow mutable owner}", "reply.empty {}"),
        ] {
            run(&source);
            rejects(&source.replace("consume(saved);", ""), "E_OUTCOME");
            rejects(
                &source.replace("consume(saved);", "drop(saved);"),
                "E_RESOURCE",
            );
            rejects(
                &source.replace(
                    "consume(saved);",
                    "auto copied=clone(borrow saved);consume(saved);",
                ),
                "E_CLONE",
            );
            rejects(
                &source.replace("consume(saved);", "auto wrapped=some {saved};"),
                "E_OUTCOME",
            );
            rejects(
                &source.replace("delta result {", "consume(saved);delta result {"),
                "E_LOAN",
            );
        }
    }
}
#[test]
fn projected_whole_outcomes_keep_exclusive_holder_and_parent_guards() {
    assert_eq!(run(GUARDS), ["after"]);
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoff);
        run(&source);
        for marker in [
            "auto moved = move pending;",
            "delta value {",
            "deref text = \"after\";",
        ] {
            for action in ["consume_holder(saved);", "owner=\"bad\";", "drop(carrier);"] {
                rejects(
                    &source.replace(marker, &format!("{action}{marker}")),
                    "E_LOAN",
                );
            }
        }
    }
    rejects(&GUARDS.replace("consume_holder(saved);", ""), "E_OUTCOME");
}
#[test]
fn exclusive_whole_outcome_moves_and_detached_leaves_preserve_authority() {
    let moved = GUARDS.replace("spawn lend(carrier)", "spawn lend(move carrier)");
    run(&moved);
    rejects(
        &moved.replace(
            "auto moved = move pending;",
            "drop(carrier);auto moved = move pending;",
        ),
        "E_MOVED",
    );
    let result = GUARDS.replace(
        "delta value {",
        "auto transferred=move value;delta transferred {",
    );
    run(&result);
    rejects(
        &result.replace("delta transferred {", "drop(value);delta transferred {"),
        "E_MOVED",
    );
    let detached = GUARDS
        .replace(
            "deref text = \"after\";",
            "ref mutable string detached=borrow mutable deref text;deref detached=\"after\";",
        )
        .replace("emit(text);", "emit(detached);");
    run(&detached);
    rejects(
        &detached.replace("emit(detached);", "consume_holder(saved);emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn shared_whole_outcome_results_restrict_exclusive_contents_without_losing_guards() {
    let source = GUARDS
        .replace("lend(ref mutable holder", "lend(ref holder")
        .replace("ref mutable choice value", "ref choice value")
        .replace("borrow mutable input.item", "borrow input.item")
        .replace("deref text = \"after\";", "");
    assert_eq!(run(&source), ["before"]);
    rejects(
        &source.replace("emit(text);", "deref text=\"bad\";emit(text);"),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace("emit(text);", "consume_holder(saved);emit(text);"),
        "E_LOAN",
    );
}
#[test]
fn empty_whole_outcomes_keep_holder_and_handling_without_supplier_loans() {
    let source = GUARDS
        .replace("choice.held { borrow mutable owner }", "choice.empty {}")
        .replace(
            "auto moved = move pending;",
            "owner=\"available\";auto moved = move pending;",
        );
    assert_ne!(source, GUARDS);
    assert_eq!(run(&source), Vec::<String>::new());
    rejects(&source.replace("consume_holder(saved);", ""), "E_OUTCOME");
    for marker in ["auto moved = move pending;", "delta value {"] {
        rejects(
            &source.replace(marker, &format!("consume_holder(saved);{marker}")),
            "E_LOAN",
        );
    }
}
#[test]
fn sibling_whole_outcome_results_keep_independent_loans() {
    let source = r#"
outcome reply['a] {held {ref mutable string text <- 'a},empty}
struct pair['a,'b] {reply['a] left;reply['b] right;}
control consume(reply input) exits done {delta input {held {auto ignored}=>held,empty=>empty}[entry]:exit done;}
async control lend(ref mutable reply input) exits ready {ref mutable reply value from input} {yield;exit ready {input};}
async control startup() exits done {
 mutable string a="a";mutable string b="b";
 mutable pair saved=pair {reply.held {borrow mutable a},reply.held {borrow mutable b}};
 auto left=spawn lend(borrow mutable saved.left);auto right=spawn lend(borrow mutable saved.right);
 delta await left {ready {auto l}=>left}
 [left]:delta l {held {auto x}=>lp,empty=>ln} [lp]:deref x="left";emit(x);
 [lp | ln]:consume(saved.left);delta await right {ready {auto r}=>right}
 [right]:delta r {held {auto y}=>rp,empty=>rn} [rp]:deref y="right";emit(y);
 [entry]:consume(saved.right);a="free";b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    assert_eq!(run(source), ["left", "right"]);
    rejects(
        &source.replace("borrow mutable saved.right", "borrow mutable saved.left"),
        "E_LOAN",
    );
    rejects(
        &source.replace(
            "delta await right",
            "consume(saved.right);delta await right",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(x);", "consume(saved.left);emit(x);"),
        "E_LOAN",
    );
}
#[test]
fn nested_whole_outcomes_keep_outer_handling_even_when_inner_absent() {
    let nested = include_str!("fixtures/source_borrowed_outcome_result_nested.tellur");
    for replacement in [
        "holder.wrapped { some { reply.held { borrow owner } } }",
        "holder.wrapped { some {reply.empty {}} }",
        "holder.wrapped {none {}}",
        "holder.empty {}",
    ] {
        let source = nested.replace(
            "holder.wrapped { some { reply.held { borrow owner } } }",
            replacement,
        );
        run(&source);
        rejects(&source.replace("consume(saved);", ""), "E_OUTCOME");
        rejects(
            &source.replace("delta value {", "consume(saved);delta value {"),
            "E_LOAN",
        );
    }
}
#[test]
fn whole_outcome_fault_results_settle_before_owning_contingency() {
    let source = include_str!("fixtures/source_borrowed_outcome_result_fault.tellur");
    let joined = source
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    assert_ne!(source, joined);
    for source in [source, joined.as_str()] {
        assert_eq!(
            run(source),
            ["result completed", "sibling started", "owner contingency"]
        );
    }
}
#[test]
fn whole_outcome_results_preserve_nominal_identity_local_escape_and_deeper_limits() {
    let anonymous = "control view(ref string input) exits found {ref string text from input} {exit found {input};}async control lend(ref outcome<view> input) exits ready {ref outcome<view> value from input} {exit ready {input};}async control caller(ref outcome<view> input) exits done {auto child=spawn lend(input);delta await child {ready {auto value}=>ready}[ready]:exit done;}";
    for source in [anonymous.to_owned(), anonymous.replace("spawn ", "defer ")] {
        rejects(&source, "E_MANIFEST");
    }

    rejects(
        "outcome choice['a] {held {ref string text <- 'a},empty}async control escape(ref string input) exits ready... {choice local=choice.held {input};exit ready {borrow local};}",
        "E_LIFETIME",
    );
    rejects(
        "outcome choice['a] {held {ref string text <- 'a},empty}outcome other['a] {held {ref string text <- 'a},empty}async control lend(ref choice input) exits ready {ref choice value from input} {exit ready {input};}async control caller(ref other input) exits done {auto child=spawn lend(input);delta await child {ready {auto value}=>ready}[ready]:exit done;}",
        "E_TYPE",
    );
    rejects(
        "outcome choice['a] {held {ref string text <- 'a},empty}async control lend(ref option<ref choice> input) exits ready {ref option<ref choice> value from input} {exit ready {input};}async control caller(ref option<ref choice> input) exits done {auto child=spawn lend(input);delta await child {ready {auto value}=>ready}[ready]:exit done;}",
        "E_ASYNC",
    );
}
