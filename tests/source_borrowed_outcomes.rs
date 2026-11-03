use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/borrowed_outcome_spawn.tellur");
const GUARDS: &str = include_str!("fixtures/source_borrowed_outcome_guards.tellur");
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
fn borrowed_outcomes_keep_selected_and_empty_schedules() {
    assert_eq!(run(SOURCE), ["left", "right", "empty", "edited", "none"]);
}
#[test]
fn borrowed_outcome_storage_and_selected_origins_survive_every_child_phase() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("        yield;\n        yield;", handoff);
        run(&source);
        for (marker, action) in [
            (
                "auto moved = move pending;",
                "consume_envelope<string>(saved);",
            ),
            ("auto moved = move pending;", "left=\"bad\";"),
            ("auto moved = move pending;", "right=\"bad\";"),
            ("emit(a);", "left=\"bad\";"),
            ("emit(b);", "right=\"bad\";"),
            ("await child;", "owner=\"bad\";"),
        ] {
            assert!(source.contains(marker));
            rejects(
                &source.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
}
const SIMPLE: &str = r#"
outcome reply['a] {held {ref string text <- 'a},empty}
control consume(reply input) exits done {delta input {held {auto ignored}=>held,empty=>empty} [entry]:exit done;}
async control inspect(ref reply input) exits done {yield;delta input {held {auto text}=>held,empty=>empty} [held]:emit(text);[entry]:exit done;}
async control startup() exits done {
 mutable string owner="text";reply saved=reply.held {borrow owner};
 auto child=spawn inspect(borrow saved);yield;yield;await child;
 consume(saved);owner="free";exit done;
}
control main() exits done {forward run startup();}
"#;
#[test]
fn borrowed_reader_never_discharges_callers_held_or_empty_handling_obligation() {
    for source in [
        SIMPLE.to_owned(),
        SIMPLE.replace("reply.held {borrow owner}", "reply.empty {}"),
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
                "auto duplicate=clone(borrow saved);consume(saved);",
            ),
            "E_CLONE",
        );
        rejects(
            &source.replace("consume(saved);", "auto wrapped=some {saved};"),
            "E_OUTCOME",
        );
        rejects(
            &source.replace("await child;", "consume(saved);await child;"),
            "E_LOAN",
        );
    }
    rejects(
        &SOURCE.replace("consume_envelope<string>(saved);", ""),
        "E_OUTCOME",
    );
}
#[test]
fn outcome_exclusive_results_keep_holder_guards_and_transferred_reborrow_ancestry() {
    assert_eq!(run(GUARDS), ["after"]);
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoff);
        run(&source);
        for marker in ["auto moved = move pending;", "deref value = \"after\";"] {
            for action in ["consume_holder(saved);", "owner=\"bad\";", "drop(carrier);"] {
                rejects(
                    &source.replace(marker, &format!("{action}{marker}")),
                    "E_LOAN",
                );
            }
        }
    }
    let moved = GUARDS.replace("spawn lend(carrier)", "spawn lend(move carrier)");
    run(&moved);
    rejects(
        &moved.replace(
            "auto moved = move pending;",
            "drop(carrier);auto moved = move pending;",
        ),
        "E_MOVED",
    );
    rejects(&GUARDS.replace("consume_holder(saved);", ""), "E_OUTCOME");
}
#[test]
fn shared_reborrow_of_exclusive_outcome_contents_keeps_guard_without_write_authority() {
    let source = GUARDS
        .replace("borrow mutable deref text", "borrow deref text")
        .replace("deref value = \"after\";", "");
    run(&source);
    rejects(
        &source.replace("emit(value);", "consume_holder(saved);emit(value);"),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(value);", "deref value=\"bad\";emit(value);"),
        "E_MUTABILITY",
    );
}
#[test]
fn empty_nested_outcomes_keep_holder_borrow_and_handling_without_supplier_loans() {
    for replacement in ["some {choice.empty {}}", "none {}"] {
        let source = GUARDS
            .replace("some { choice.held { borrow mutable owner } }", replacement)
            .replace(
                "auto moved = move pending;",
                "owner=\"available\";auto moved = move pending;",
            );
        assert_eq!(run(&source), Vec::<String>::new());
        rejects(
            &source.replace(
                "auto moved = move pending;",
                "consume_holder(saved);auto moved = move pending;",
            ),
            "E_LOAN",
        );
        if replacement.contains("choice.empty") {
            rejects(&source.replace("consume_holder(saved);", ""), "E_OUTCOME");
        }
    }
}
#[test]
fn shared_outcome_siblings_keep_distinct_loans_and_nominal_identity() {
    let source = SIMPLE
        .replace(
            "auto child=spawn inspect(borrow saved);",
            "auto child=spawn inspect(borrow saved);auto other=spawn inspect(borrow saved);",
        )
        .replace("await child;", "await child;await other;");
    assert_eq!(run(&source), ["text", "text"]);
    rejects(
        &source.replace("await other;", "consume(saved);await other;"),
        "E_LOAN",
    );
    let wrong = source.replace(
        "reply saved=reply.held",
        "other_reply saved=other_reply.held",
    );
    rejects(
        &format!("outcome other_reply['a] {{held {{ref string text <- 'a}},empty}}{wrong}"),
        "E_TYPE",
    );
}
#[test]
fn borrowed_outcome_faults_settle_joined_and_unjoined_storage_before_owning_handler() {
    let source = include_str!("fixtures/source_borrowed_outcome_fault.tellur");
    let joined = source
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    assert_ne!(joined, source);
    for source in [source, joined.as_str()] {
        assert_eq!(
            run(source),
            ["result completed", "sibling started", "owner contingency"]
        );
    }
}
#[test]
fn deeper_outcome_references_remain_gated_while_whole_results_are_admitted() {
    for ty in ["ref option<ref reply>", "indirect"] {
        let source = format!(
            "outcome reply['a] {{held {{ref string text <- 'a}},empty}} struct indirect['storage,'a] {{ref reply['a] value <- 'storage;}} async control work({ty} input) exits done {{exit done;}} async control caller({ty} input) exits done {{auto child=spawn work(input);await child;exit done;}}"
        );
        rejects(&source, "E_ASYNC");
    }
    compile(
        "outcome reply['a] {held {ref string text <- 'a},empty}async control work(ref reply input) exits ready {ref reply value from input} {exit ready {input};}async control caller(ref reply input) exits done {auto child=spawn work(input);delta await child {ready {auto value}=>ready} [ready]:exit done;}", manifest(),
    ).unwrap();
}
#[test]
fn borrowed_named_outcome_generic_child_forwarding_keeps_selected_origins() {
    let source = SOURCE.replace("async control first<t>", "async control leaf<t>");
    let source = format!(
        "{source}\nasync control first<t>(ref envelope<t> input) exits left {{ref t value from input['a]}},right {{ref t value from input['b]}},empty {{auto child=spawn leaf<t>(input);forward await child;}}"
    );
    assert_eq!(run(&source), ["left", "right", "empty", "edited", "none"]);
    rejects(
        &source.replace("emit(a);", "left=\"bad\";emit(a);"),
        "E_LOAN",
    );
}
