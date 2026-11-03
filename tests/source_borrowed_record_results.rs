use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/borrowed_record_spawn_results.tellur");
const GUARDS: &str = include_str!("fixtures/source_borrowed_record_result_guards.tellur");
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
fn whole_record_results_keep_shared_and_projected_exclusive_schedules() {
    assert_eq!(run(SOURCE), ["left", "right", "after", "available"]);
}
#[test]
fn whole_record_result_holders_and_suppliers_survive_every_child_phase() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoff);
        run(&source);
        for action in [
            "drop(first);",
            "drop(second);",
            "left=\"bad\";",
            "right=\"bad\";",
        ] {
            rejects(
                &source.replace(
                    "auto moved = move pending;",
                    &format!("{action}auto moved = move pending;"),
                ),
                "E_LOAN",
            );
        }
        for (marker, action) in [
            ("auto copy = a;", "drop(first);"),
            ("auto copy = a;", "left=\"bad\";"),
            ("emit(b.text);", "drop(second);"),
            ("emit(b.text);", "right=\"bad\";"),
        ] {
            rejects(
                &source.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
}
#[test]
fn shared_whole_record_copies_hold_storage_until_final_use_but_detached_leaves_do_not() {
    run(SOURCE);
    rejects(
        &SOURCE.replace(
            "ref string detached = copy.text;",
            "drop(first);ref string detached = copy.text;",
        ),
        "E_LOAN",
    );
    rejects(
        &SOURCE.replace("emit(detached);", "left=\"bad\";emit(detached);"),
        "E_LOAN",
    );
    let moved = SOURCE.replace("auto copy = a;", "auto copy = move a;");
    run(&moved);
    // Moving a shared reference copies its permission under ordinary Copy rules.
    run(&moved.replace(
        "ref string detached = copy.text;",
        "emit(a.text);ref string detached = copy.text;",
    ));
}
#[test]
fn projected_exclusive_record_results_keep_holder_guards_and_alias_ancestry() {
    assert_eq!(run(GUARDS), ["after", "available after result"]);
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoff);
        run(&source);
        for marker in [
            "auto moved = move pending;",
            "deref value.text = \"after\";",
        ] {
            for action in [
                "drop(saved);",
                "owner=\"bad\";",
                "deref carrier.item.text=\"bad\";",
            ] {
                rejects(
                    &source.replace(marker, &format!("{action}{marker}")),
                    "E_LOAN",
                );
            }
        }
    }
    let moved = GUARDS.replace("spawn lend(carrier)", "spawn lend(move carrier)");
    rejects(&moved, "E_MOVED");
    run(&moved.replace("        deref carrier.item.text = \"available after result\";\n        emit(carrier.item.text);\n",""));
    let forwarded = GUARDS
        .replace(
            "deref value.text = \"after\";",
            "auto forwarded=move value;deref forwarded.text=\"after\";",
        )
        .replace("emit(value.text);", "emit(forwarded.text);");
    run(&forwarded);
    rejects(
        &forwarded.replace(
            "emit(forwarded.text);",
            "emit(value.text);emit(forwarded.text);",
        ),
        "E_MOVED",
    );
    let detached = GUARDS
        .replace(
            "deref value.text = \"after\";",
            "ref mutable string detached=borrow mutable deref value.text;deref detached=\"after\";",
        )
        .replace("emit(value.text);", "emit(detached);");
    run(&detached);
    rejects(
        &detached.replace("emit(detached);", "drop(saved);emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn shared_whole_record_results_do_not_gain_write_authority_to_exclusive_contents() {
    let source = GUARDS
        .replace("lend(ref mutable holder", "lend(ref holder")
        .replace("borrow mutable input.item", "borrow input.item")
        .replace("deref value.text = \"after\";", "")
        .replace("emit(value.text);", "emit(borrow deref value.text);");
    run(&source);
    rejects(
        &source.replace(
            "emit(borrow deref value.text);",
            "deref value.text=\"bad\";",
        ),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace(
            "emit(borrow deref value.text);",
            "drop(saved);emit(borrow deref value.text);",
        ),
        "E_LOAN",
    );
}
const SIBLINGS: &str = r#"
struct view['a] {ref mutable string text <- 'a;}
struct pair['a,'b] {view['a] left;view['b] right;}
async control lend(ref mutable view input) exits ready {ref mutable view value from input} {yield;exit ready {input};}
async control startup() exits done {
 mutable string a="a";mutable string b="b";
 mutable pair saved=pair {view {borrow mutable a},view {borrow mutable b}};
 auto left=spawn lend(borrow mutable saved.left);auto right=spawn lend(borrow mutable saved.right);
 delta await left {ready {auto l}=>left}
 [left]:deref l.text="left";emit(l.text);
 delta await right {ready {auto r}=>right}
 [right]:deref r.text="right";emit(r.text);drop(saved);a="free";b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
#[test]
fn independent_whole_record_results_keep_disjoint_holder_identity() {
    assert_eq!(run(SIBLINGS), ["left", "right"]);
    for bad in [
        SIBLINGS.replace("borrow mutable saved.right", "borrow mutable saved.left"),
        SIBLINGS.replace(
            "delta await right",
            "deref saved.right.text=\"bad\";delta await right",
        ),
        SIBLINGS.replace("emit(l.text);", "drop(saved);emit(l.text);"),
    ] {
        rejects(&bad, "E_LOAN");
    }
}
const NESTED: &str = r#"
outcome reply['a] {held {ref string text <- 'a},empty}
struct holder['a] {option<reply>['a] item;}
async control lend(ref holder input) exits ready {ref holder value from input} {yield;exit ready {input};}
control consume(holder input) exits done {delta input.item {some {auto value}=>some,none=>none} [some]:delta value {held {auto text}=>held,empty=>empty} [entry]:exit done;}
async control startup() exits done {
 mutable string owner="nested";holder saved=holder {some {reply.held {borrow owner}}};
 auto child=spawn lend(borrow saved);auto outcome=capture await child;
 delta outcome {ready {auto result}=>ready}
 [ready]:delta borrow result.item {some {auto inner}=>some,none=>none}
 [some]:delta inner {held {auto text}=>held,empty=>empty} [held]:emit(text);
 [entry]:consume(saved);owner="free";exit done;
}
control main() exits done {forward run startup();}
"#;
#[test]
fn whole_record_results_preserve_nested_outcome_tags_and_caller_obligations() {
    for replacement in [
        "some {reply.held {borrow owner}}",
        "some {reply.empty {}}",
        "none {}",
    ] {
        let source = NESTED.replace("some {reply.held {borrow owner}}", replacement);
        run(&source);
        rejects(
            &source.replace("auto outcome=", "consume(saved);auto outcome="),
            "E_LOAN",
        );
        rejects(
            &source.replace(
                "delta borrow result.item",
                "consume(saved);delta borrow result.item",
            ),
            "E_LOAN",
        );
        if replacement != "none {}" {
            rejects(&source.replace("consume(saved);", ""), "E_OUTCOME");
        }
    }
}
#[test]
fn generic_nested_children_forward_whole_record_origin_contracts() {
    let source = SOURCE.replace("async control choose<t>", "async control leaf<t>");
    let source = format!(
        "{source}\nasync control choose<t>(ref view<t> left,ref view<t> right,bool yes) exits left {{ref view<t> value from left}},right {{ref view<t> value from right}} {{auto child=spawn leaf<t>(left,right,yes);forward await child;}}"
    );
    assert_eq!(run(&source), ["left", "right", "after", "available"]);
    rejects(
        &source.replace("auto copy = a;", "drop(first);auto copy = a;"),
        "E_LOAN",
    );
}
#[test]
fn whole_borrowed_record_fault_results_settle_before_owning_contingency() {
    let source = include_str!("fixtures/source_borrowed_record_result_fault.tellur");
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
fn whole_borrowed_record_chains_are_admitted_without_local_escape() {
    rejects(
        "struct view['a] {ref string text <- 'a;} async control escape(ref string input) exits ready... {view local=view {input};exit ready {borrow local};}",
        "E_LIFETIME",
    );
    for ty in ["indirect"] {
        let source = format!(
            "struct view['a] {{ref string text <- 'a;}}variant choice['a] {{held {{ref string text <- 'a}},empty}}outcome reply['a] {{held {{ref string text <- 'a}},empty}}struct indirect['holder,'a] {{ref view['a] item <- 'holder;}}async control lend(ref {ty} input) exits ready {{ref {ty} value from input}} {{exit ready {{input}};}}async control caller(ref {ty} input) exits done {{auto child=spawn lend(input);delta await child {{ready {{auto value}}=>ready}}[ready]:exit done;}}"
        );
        compile(&source, manifest()).unwrap();
    }
}
