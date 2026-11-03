use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/borrowed_tagged_spawn_results.tellur");
const GUARDS: &str = include_str!("fixtures/source_borrowed_tagged_result_guards.tellur");
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
fn whole_tagged_results_preserve_selected_and_empty_schedules() {
    assert_eq!(run(SOURCE), ["left", "right", "empty", "edited", "none"]);
}
#[test]
fn whole_tagged_holder_and_supplier_loans_survive_every_child_phase() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("        yield;\n        yield;", handoff);
        run(&source);
        for (marker, action) in [
            ("auto moved = move pending;", "drop(saved);"),
            ("auto moved = move pending;", "left=\"bad\";"),
            ("auto moved = move pending;", "right=\"bad\";"),
            ("auto copy = move result;", "drop(saved);"),
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
fn shared_whole_tagged_copies_keep_storage_until_last_use_and_detach_selected_leaves() {
    run(SOURCE);
    let copies = SOURCE.replace(
        "auto copy = move result;",
        "auto copy = result;auto other=move result;drop(other);",
    );
    run(&copies);
    rejects(
        &copies.replace("delta copy {", "drop(saved);delta copy {"),
        "E_LOAN",
    );
    rejects(
        &SOURCE.replace("emit(a);", "left=\"bad\";emit(a);"),
        "E_LOAN",
    );
}
#[test]
fn projected_whole_option_results_keep_exclusive_holder_guards() {
    assert_eq!(run(GUARDS), ["after"]);
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoff);
        run(&source);
        for marker in [
            "auto moved = move pending;",
            "delta value {",
            "deref text = \"after\";",
        ] {
            for action in ["drop(saved);", "owner=\"bad\";", "drop(carrier);"] {
                rejects(
                    &source.replace(marker, &format!("{action}{marker}")),
                    "E_LOAN",
                );
            }
        }
    }
}
#[test]
fn exclusive_whole_tagged_result_moves_and_detached_reborrows_preserve_authority() {
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
        &detached.replace("emit(detached);", "drop(saved);emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn shared_whole_tagged_results_restrict_exclusive_contents_without_losing_guards() {
    let source = GUARDS
        .replace("lend(ref mutable holder", "lend(ref holder")
        .replace(
            "ref mutable option<choice> value",
            "ref option<choice> value",
        )
        .replace("borrow mutable input.item", "borrow input.item")
        .replace("deref text = \"after\";", "");
    run(&source);
    rejects(
        &source.replace("emit(text);", "deref text=\"bad\";emit(text);"),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace("emit(text);", "drop(saved);emit(text);"),
        "E_LOAN",
    );
}
#[test]
fn empty_whole_tagged_results_keep_storage_without_inventing_supplier_loans() {
    for replacement in ["some {choice.empty {}}", "none {}"] {
        let source = GUARDS
            .replace("some { choice.held { borrow mutable owner } }", replacement)
            .replace(
                "auto moved = move pending;",
                "owner=\"available\";auto moved = move pending;",
            );
        assert_ne!(source, GUARDS);
        assert_eq!(run(&source), Vec::<String>::new());
        for marker in ["auto moved = move pending;", "delta value {"] {
            rejects(
                &source.replace(marker, &format!("drop(saved);{marker}")),
                "E_LOAN",
            );
        }
    }
}
#[test]
fn disjoint_whole_option_results_keep_independent_sibling_authority() {
    let source = r#"
struct pair['a,'b] {option<ref mutable string>['a] left;option<ref mutable string>['b] right;}
async control lend(ref mutable option<ref mutable string> input) exits ready {ref mutable option<ref mutable string> value from input} {yield;exit ready {input};}
async control startup() exits done {
 mutable string a="a";mutable string b="b";
 mutable pair saved=pair {some {borrow mutable a},some {borrow mutable b}};
 auto left=spawn lend(borrow mutable saved.left);auto right=spawn lend(borrow mutable saved.right);
 delta await left {ready {auto l}=>left}
 [left]:delta l {some {auto x}=>lp,none=>ln} [lp]:deref x="left";emit(x);
 [lp | ln]:delta await right {ready {auto r}=>right}
 [right]:delta r {some {auto y}=>rp,none=>rn} [rp]:deref y="right";emit(y);
 [entry]:drop(saved);a="free";b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    assert_eq!(run(source), ["left", "right"]);
    rejects(
        &source.replace("borrow mutable saved.right", "borrow mutable saved.left"),
        "E_LOAN",
    );
    rejects(
        &source.replace("delta await right", "drop(saved.right);delta await right"),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(x);", "drop(saved);emit(x);"),
        "E_LOAN",
    );
}
#[test]
fn whole_variant_results_preserve_nested_outcome_handling_and_generic_forwarding() {
    let nested = include_str!("fixtures/source_borrowed_tagged_result_outcomes.tellur");
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
        rejects(
            &source.replace("delta value {", "consume(saved);delta value {"),
            "E_LOAN",
        );
        if replacement.contains("reply.") {
            rejects(&source.replace("consume(saved);", ""), "E_OUTCOME");
        }
    }
    let source = SOURCE.replace("async control first<t>", "async control leaf<t>");
    let source = format!(
        "{source}\nasync control first<t>(ref choice<t> input) exits ready {{ref choice<t> value from input}} {{auto child=spawn leaf<t>(input);forward await child;}}"
    );
    assert_eq!(run(&source), ["left", "right", "empty", "edited", "none"]);
}
#[test]
fn whole_borrowed_tagged_fault_results_settle_before_owning_contingency() {
    let source = include_str!("fixtures/source_borrowed_tagged_result_fault.tellur");
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
fn whole_tagged_results_preserve_nominal_identity_and_escape_limits() {
    rejects(
        "variant choice['a] {held {ref string text <- 'a},empty}async control escape(ref string input) exits ready... {choice local=choice.held {input};exit ready {borrow local};}",
        "E_LIFETIME",
    );
    rejects(
        "variant choice['a] {held {ref string text <- 'a},empty}variant other['a] {held {ref string text <- 'a},empty}async control lend(ref choice input) exits ready {ref choice value from input} {exit ready {input};}async control caller(ref other input) exits done {auto child=spawn lend(input);delta await child {ready {auto value}=>ready}[ready]:exit done;}",
        "E_TYPE",
    );
    for ty in ["option<ref choice>"] {
        let source = format!(
            "variant choice['a] {{held {{ref string text <- 'a}},empty}}outcome reply['a] {{held {{ref string text <- 'a}},empty}}async control lend(ref {ty} input) exits ready {{ref {ty} value from input}} {{exit ready {{input}};}}async control caller(ref {ty} input) exits done {{auto child=spawn lend(input);delta await child {{ready {{auto value}}=>ready}}[ready]:exit done;}}"
        );
        rejects(&source, "E_ASYNC");
    }
}
