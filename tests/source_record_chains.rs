use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/record_chain_spawn.tellur");
const GUARDS: &str = include_str!("fixtures/source_record_chain_guards.tellur");
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
fn record_chains_preserve_selected_shared_and_exclusive_schedules() {
    assert_eq!(run(SOURCE), ["left", "right"]);
    assert_eq!(run(GUARDS), ["after", "available"]);
    assert_eq!(
        run(include_str!("fixtures/source_record_chain_results.tellur")),
        ["returned"]
    );
}
#[test]
fn every_record_holder_and_supplier_survives_queued_running_and_completed_children() {
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
        rejects(
            &source.replace("auto copy = move a;", "drop(first);auto copy = move a;"),
            "E_LOAN",
        );
        rejects(
            &source.replace("emit(b.text);", "drop(second);emit(b.text);"),
            "E_LOAN",
        );
    }
}
#[test]
fn shared_record_chain_copies_and_detached_leaves_keep_selected_origins() {
    let source = SOURCE
        .replace(
            "spawn choose<string>(links,",
            "spawn choose<string>(move links,",
        )
        .replace(
            "auto copy = move a;",
            "auto copy=a;auto another=move a;drop(another);",
        );
    run(&source);
    rejects(
        &source.replace(
            "ref string detached = copy.text;",
            "drop(first);ref string detached = copy.text;",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(detached);", "left=\"bad\";emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn multiple_exclusive_reference_edges_preserve_all_holder_guards() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = GUARDS.replace("    yield;\n    yield;", handoff);
        run(&source);
        for marker in [
            "auto moved = move pending;",
            "ref mutable string detached",
            "emit(detached);",
        ] {
            for action in [
                "drop(outer);",
                "drop(middle);",
                "drop(saved);",
                "drop(carrier);",
                "owner=\"bad\";",
            ] {
                rejects(
                    &source.replace(marker, &format!("{action}{marker}")),
                    "E_LOAN",
                );
            }
        }
    }
}
#[test]
fn exclusive_reference_chain_transfers_keep_mutable_ancestry() {
    let moved = GUARDS.replace("spawn lend(carrier)", "spawn lend(move carrier)");
    run(&moved);
    rejects(
        &moved.replace(
            "auto moved = move pending;",
            "drop(carrier);auto moved = move pending;",
        ),
        "E_MOVED",
    );
    let transferred=GUARDS.replace("ref mutable string detached = borrow mutable deref value.text;", "auto transferred=move value;ref mutable string detached=borrow mutable deref transferred.text;");
    run(&transferred);
    rejects(
        &transferred.replace(
            "auto transferred=move value;",
            "auto transferred=move value;drop(value);",
        ),
        "E_MOVED",
    );
}
#[test]
fn shared_outer_chain_cannot_write_exclusive_inner_fields() {
    let shared = GUARDS
        .replace("lend(ref mutable root", "lend(ref root")
        .replace(
            "borrow mutable deref input.inner.item",
            "borrow deref input.inner.item",
        )
        .replace(
            "ref mutable string detached = borrow mutable deref value.text;",
            "ref string detached=borrow deref value.text;",
        )
        .replace("deref detached = \"after\";", "");
    assert_eq!(run(&shared), ["before", "available"]);
    rejects(
        &shared.replace("emit(detached);", "deref detached=\"bad\";emit(detached);"),
        "E_MUTABILITY",
    );
    rejects(
        &shared.replace("emit(detached);", "drop(middle);emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn owned_chain_results_keep_holders_through_repeated_spawn_and_join() {
    let source = include_str!("fixtures/source_record_chain_results.tellur");
    run(source);
    rejects(
        &source.replace("delta await pending", "drop(original);delta await pending"),
        "E_MOVED",
    );
    for marker in [
        "delta await pending",
        "mutable link returned",
        "deref value.text = \"returned\";",
    ] {
        for action in ["drop(saved);", "owner=\"bad\";"] {
            rejects(
                &source.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
    rejects(
        &source.replace("emit(value.text);", "drop(returned);emit(value.text);"),
        "E_LOAN",
    );
}
#[test]
fn whole_borrowed_chain_results_keep_storage_and_contained_reference_edges() {
    let source = GUARDS
        .replace(
            "exit ready { borrow mutable deref input.inner.item };",
            "exit ready { input };",
        )
        .replace(
            "borrow mutable deref value.text",
            "borrow mutable deref value.inner.item.text",
        );
    assert_ne!(source, GUARDS);
    assert_eq!(run(&source), ["after", "available"]);
    for action in ["drop(outer);", "drop(middle);", "drop(saved);"] {
        rejects(
            &source.replace("emit(detached);", &format!("{action}emit(detached);")),
            "E_LOAN",
        );
    }
}
#[test]
fn independent_record_chain_siblings_keep_disjoint_authority() {
    let source = r#"
struct view['a] {ref mutable string text <- 'a;}
struct link['holder,'a] {ref mutable view['a] item <- 'holder;}
struct pair['left,'a,'right,'b] {link['left,'a] left;link['right,'b] right;}
async control lend(ref mutable link input) exits ready... {yield;exit ready {borrow mutable deref input.item};}
async control startup() exits done {
 mutable string a="a";mutable string b="b";
 mutable view first=view {borrow mutable a};mutable view second=view {borrow mutable b};
 mutable pair saved=pair {link {borrow mutable first},link {borrow mutable second}};
 auto left=spawn lend(borrow mutable saved.left);auto right=spawn lend(borrow mutable saved.right);
 delta await left {ready {auto l}=>left}
 [left]:deref l.text="left";emit(l.text);delta await right {ready {auto r}=>right}
 [right]:deref r.text="right";emit(r.text);drop(saved);drop(first);drop(second);a="free";b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    assert_eq!(run(source), ["left", "right"]);
    rejects(
        &source.replace("borrow mutable saved.right", "borrow mutable saved.left"),
        "E_LOAN",
    );
    rejects(
        &source.replace("delta await right", "drop(second);delta await right"),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(l.text);", "drop(saved.left);emit(l.text);"),
        "E_LOAN",
    );
}
#[test]
fn generic_child_forwarding_preserves_record_chain_result_origins() {
    let leaf = SOURCE.replace("async control choose<t>", "async control leaf<t>");
    let source = format!(
        "{leaf}\nasync control choose<t>(pair<t> input,bool left) exits left {{ref view<t>[input['a]] value from input['left_holder]}},right {{ref view<t>[input['b]] value from input['right_holder]}} {{auto child=spawn leaf<t>(input,left);forward await child;}}"
    );
    assert_eq!(run(&source), ["left", "right"]);
    rejects(
        &source.replace("emit(detached);", "left=\"bad\";emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn completed_and_joined_record_chain_fault_results_settle_before_contingency() {
    let source = include_str!("fixtures/source_record_chain_fault.tellur");
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
fn record_chain_admission_keeps_tagged_nodes_and_local_escape_gated() {
    let defs = "struct view['a] {ref string text <- 'a;}struct chain['holder,'a] {ref view['a] item <- 'holder;}variant tag['a] {held {ref string text <- 'a},empty}struct indirect['holder,'a] {ref tag['a] item <- 'holder;}struct mixed['holder,'a] {chain['holder,'a] item;option<ref string>['a] tagged;}";
    for ty in [
        "option<chain>",
        "ref option<chain>",
        "indirect",
        "ref indirect",
        "mixed",
        "ref mixed",
    ] {
        let source = format!(
            "{defs} async control work({ty} input) exits done {{exit done;}}async control caller({ty} input) exits done {{auto child=spawn work(input);await child;exit done;}}"
        );
        rejects(&source, "E_ASYNC");
    }
    rejects(
        "struct view['a] {ref string text <- 'a;}struct link['holder,'a] {ref view['a] item <- 'holder;}async control escape(ref string input) exits ready... {view local=view {input};link result=link {borrow local};exit ready {result};}",
        "E_LIFETIME",
    );
}
