use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/borrowed_tagged_spawn.tellur");
const GUARDS: &str = include_str!("fixtures/source_borrowed_tagged_guards.tellur");
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
fn borrowed_tagged_payloads_preserve_selected_and_empty_schedules() {
    assert_eq!(run(SOURCE), ["left", "right", "empty", "edited", "none"]);
}
#[test]
fn borrowed_tagged_loans_cover_queue_execution_completion_and_selected_results() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoff);
        run(&source);
        for (marker, action) in [
            ("auto moved = move pending;", "drop(saved);"),
            ("auto moved = move pending;", "left=\"bad\";"),
            ("auto moved = move pending;", "right=\"bad\";"),
            ("emit(a);", "left=\"bad\";"),
            ("emit(b);", "right=\"bad\";"),
            ("await child;", "drop(saved);"),
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
#[test]
fn tagged_exclusive_results_retain_holder_guard_and_reborrow_ancestry() {
    assert_eq!(run(GUARDS), ["after"]);
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoff);
        run(&source);
        for marker in ["auto moved = move pending;", "deref value = \"after\";"] {
            for action in ["drop(saved);", "owner=\"bad\";", "drop(carrier);"] {
                assert!(source.contains(marker));
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
}
#[test]
fn shared_reborrows_of_tagged_exclusive_contents_keep_guards_without_write_authority() {
    let source = GUARDS
        .replace("borrow mutable deref text", "borrow deref text")
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
fn empty_borrowed_tags_keep_holder_storage_without_inventing_supplier_loans() {
    for replacement in ["some {choice.empty {}}", "none {}"] {
        let source = GUARDS
            .replace("some { choice.held { borrow mutable owner } }", replacement)
            .replace(
                "auto moved = move pending;",
                "owner=\"available\";auto moved = move pending;",
            );
        assert_ne!(source, GUARDS);
        assert_eq!(run(&source), Vec::<String>::new());
        rejects(
            &source.replace(
                "auto moved = move pending;",
                "drop(saved);auto moved = move pending;",
            ),
            "E_LOAN",
        );
    }
}
#[test]
fn borrowed_tagged_siblings_and_disjoint_exclusive_fields_keep_independent_loans() {
    let source = r#"
variant choice['a] {held {ref string text <- 'a},empty}
struct pair['a,'b] {option<ref mutable string>['a] left;option<ref mutable string>['b] right;}
async control inspect(ref choice input) exits done {yield;delta input {held {auto text}=>held,empty=>empty} [held]:emit(text);[entry]:exit done;}
async control edit(ref mutable option<ref mutable string> input) exits done {yield;delta input {some {auto text}=>some,none=>none} [some]:deref text="edited";[entry]:exit done;}
async control startup() exits done {
 mutable string shared="shared";choice saved=choice.held {borrow shared};
 auto first=spawn inspect(borrow saved);auto second=spawn inspect(borrow saved);
 await first;await second;drop(saved);shared="free";
 mutable string a="a";mutable string b="b";
 mutable pair values=pair {some {borrow mutable a},some {borrow mutable b}};
 auto left=spawn edit(borrow mutable values.left);auto right=spawn edit(borrow mutable values.right);
 await left;drop(values.left);a="free";await right;drop(values.right);b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    assert_eq!(run(source), ["shared", "shared"]);
    rejects(
        &source.replace(
            "await first;await second;",
            "await first;drop(saved);await second;",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("borrow mutable values.right", "borrow mutable values.left"),
        "E_LOAN",
    );
    rejects(
        &source.replace(
            "await right;drop(values.right);",
            "drop(values.right);await right;",
        ),
        "E_LOAN",
    );
}
#[test]
fn borrowed_tagged_faults_settle_joined_and_unjoined_results_before_owning_handler() {
    let source = include_str!("fixtures/source_borrowed_tagged_fault.tellur");
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
fn deeper_record_pointees_and_whole_borrowed_tags_are_admitted() {
    for ty in ["ref option<ref leaf>"] {
        let source = format!(
            "struct leaf['a] {{ref string text <- 'a;}} outcome reply['a] {{held {{leaf['a] value}}}} struct wrapped['a] {{reply['a] value;}} async control work({ty} input) exits done {{exit done;}} async control caller({ty} input) exits done {{auto child=spawn work(input);await child;exit done;}}"
        );
        compile(&source, manifest()).unwrap();
    }
    compile(
        "variant choice['a] {held {ref string text <- 'a},empty} async control lend(ref choice input) exits ready {ref choice value from input} {exit ready {input};} async control caller(ref choice input) exits done {auto child=spawn lend(input);delta await child {ready {auto value}=>ready} [ready]:exit done;}",
        manifest(),
    ).unwrap();
}
#[test]
fn borrowed_tagged_generic_child_forwarding_preserves_selected_origins() {
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
