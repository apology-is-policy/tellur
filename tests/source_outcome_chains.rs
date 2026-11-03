use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
const SOURCE: &str = include_str!("../playground/outcome_chain_spawn.tellur");
const GUARDS: &str = include_str!("fixtures/source_outcome_chain_guards.tellur");
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

fn empty(source: &str) -> String {
    source
        .replace("some { borrow mutable owner }", "none {}")
        .replace("choice.held { borrow mutable owner }", "choice.empty {}")
        .replace(
            "auto moved = move child;",
            "owner=\"available\";auto moved = move child;",
        )
}
#[test]
fn active_and_empty_outcome_chains_preserve_schedules() {
    assert_eq!(run(SOURCE), ["left", "right"]);
    for source in [GUARDS] {
        assert_eq!(run(source), ["after"]);
        assert_eq!(run(&empty(source)), Vec::<String>::new());
    }
}
#[test]
fn outcome_chain_captures_cover_shared_exclusive_owned_and_borrowed_roots() {
    for original in [GUARDS] {
        for shared in [false, true] {
            for owned in [false, true] {
                let mut s = original.to_owned();
                if owned {
                    s = s
                        .replace("lend(ref mutable chain input)", "lend(chain input)")
                        .replace("borrow mutable deref input.item", "input.item")
                        .replace("spawn lend(borrow mutable link)", "spawn lend(move link)");
                }
                if shared {
                    s = s
                        .replace("ref mutable ", "ref ")
                        .replace("borrow mutable ", "borrow ")
                        .replace("deref text = \"after\";", "");
                }
                assert_eq!(run(&s), [if shared { "before" } else { "after" }]);
            }
        }
    }
}
#[test]
fn outcome_holders_and_suppliers_survive_every_child_phase() {
    for original in [GUARDS] {
        for handoff in ["", "yield;", "yield;yield;"] {
            let s = original.replace("    yield;\n    yield;", handoff);
            for marker in [
                "auto moved = move child;",
                "[ready]: delta value",
                "emit(text);",
            ] {
                for action in ["consume(saved);", "drop(link);", "owner=\"bad\";"] {
                    rejects(&s.replace(marker, &format!("{action}{marker}")), "E_LOAN");
                }
            }
        }
    }
}
#[test]
fn outcome_chain_empty_tags_keep_holder_storage_without_absent_supplier_loans() {
    for original in [GUARDS] {
        let s = empty(original);
        run(&s);
        for marker in ["auto moved = move child;", "delta value"] {
            for action in ["consume(saved);", "drop(link);"] {
                rejects(&s.replace(marker, &format!("{action}{marker}")), "E_LOAN");
            }
        }
    }
}
#[test]
fn outcome_shared_copies_and_selected_origins_release_unselected_suppliers() {
    run(SOURCE);
    let s = SOURCE.replace(
        "auto copy = move a;",
        "auto copy=a;auto another=move a;drop(another);",
    );
    run(&s);
    for action in ["consume<string>(first);", "left=\"bad\";"] {
        rejects(
            &s.replace("delta copy", &format!("{action}delta copy")),
            "E_LOAN",
        );
    }
    rejects(
        &s.replace("emit(detached);", "left=\"bad\";emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn exclusive_outcome_results_keep_moves_and_detached_guards() {
    for source in [GUARDS] {
        let s = source.replace(
            "[ready]: delta value",
            "[ready]:auto transferred=move value;delta transferred",
        );
        run(&s);
        rejects(
            &s.replace("delta transferred", "drop(value);delta transferred"),
            "E_MOVED",
        );
        let moved = source
            .replace("borrow mutable link);", "move carrier);")
            .replace(
                "auto child =",
                "ref mutable chain carrier=borrow mutable link;auto child =",
            );
        run(&moved);
        rejects(
            &moved.replace(
                "auto moved = move child;",
                "drop(carrier);auto moved = move child;",
            ),
            "E_MOVED",
        );
        rejects(
            &source.replace("emit(text);", "drop(link);emit(text);"),
            "E_LOAN",
        );
    }
}
#[test]
fn outcome_chain_whole_borrowed_and_owned_chain_results_cross_repeated_joins() {
    for source in [GUARDS] {
        let whole = source
            .replace(
                "exit ready { borrow mutable deref input.item };",
                "exit ready { input };",
            )
            .replace("delta value {", "delta value.item {");
        run(&whole);
        let owned=whole.replace("lend(ref mutable chain input)", "lend(chain input)")
            .replace("spawn lend(borrow mutable link)", "spawn lend(move link)")
            .replace("[ready]: delta value.item", "[ready]:auto again=spawn lend(move value);delta await again {ready {auto returned}=>returned}[returned]:delta returned.item");
        run(&owned);
        rejects(
            &owned.replace("emit(text);", "consume(saved);emit(text);"),
            "E_LOAN",
        );
    }
}
#[test]
fn shared_outer_outcome_authority_cannot_mutate_exclusive_leaves() {
    for source in [GUARDS] {
        let s = source
            .replace("lend(ref mutable chain", "lend(ref chain")
            .replace("spawn lend(borrow mutable link)", "spawn lend(borrow link)")
            .replace("borrow mutable deref input.item", "borrow deref input.item")
            .replace("deref text = \"after\";", "");
        assert_eq!(run(&s), ["before"]);
        rejects(
            &s.replace("emit(text);", "deref text=\"bad\";emit(text);"),
            "E_MUTABILITY",
        );
    }
}
#[test]
fn generic_outcome_child_forwarding_preserves_storage_and_contained_origins() {
    let leaf = SOURCE.replace("async control choose<t>", "async control leaf<t>");
    let s = format!(
        "{leaf}\nasync control choose<t>(pair<t> input,bool left) exits left {{ref view<t>[input['a]] value from input['left_holder]}},right {{ref view<t>[input['b]] value from input['right_holder]}} {{auto child=spawn leaf<t>(input,left);forward await child;}}"
    );
    assert_eq!(run(&s), ["left", "right"]);
}
#[test]
fn independent_outcome_siblings_keep_disjoint_exclusive_permissions() {
    let s = r#"
outcome view['a] {held {ref mutable string text <- 'a},empty}
struct chain['holder,'a] {ref mutable view['a] item <- 'holder;}
async control edit(ref mutable chain input) exits done {yield;delta input.item {held {auto text}=>held,empty=>empty}[held]:deref text="edited";emit(text);[entry]:exit done;}
async control startup() exits done {
mutable string a="a";mutable string b="b";
mutable view first=view.held {borrow mutable a};mutable view second=view.held {borrow mutable b};
mutable chain left=chain {borrow mutable first};mutable chain right=chain {borrow mutable second};
auto l=spawn edit(borrow mutable left);auto r=spawn edit(borrow mutable right);
await l;drop(left);consume(first);a="free";await r;drop(right);consume(second);b="free";exit done;
 }control consume(view input) exits done {delta input {held {auto ignored}=>held,empty=>empty}[entry]:exit done;}control main() exits done {forward run startup();}
"#;
    assert_eq!(run(s), ["edited", "edited"]);
    rejects(
        &s.replace(
            "spawn edit(borrow mutable right)",
            "spawn edit(borrow mutable left)",
        ),
        "E_LOAN",
    );
}
#[test]
fn joined_and_unjoined_outcome_fault_results_settle_before_contingency() {
    let s = include_str!("fixtures/source_outcome_chain_fault.tellur");
    let joined = s
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    for source in [s, joined.as_str()] {
        assert_eq!(
            run(source),
            ["result completed", "sibling started", "owner contingency"]
        );
    }
}

#[test]
fn owning_handling_survives_active_empty_and_shared_inspection() {
    for original in [GUARDS, empty(GUARDS).as_str()] {
        for shared in [false, true] {
            let s = if shared {
                original
                    .replace("ref mutable ", "ref ")
                    .replace("borrow mutable ", "borrow ")
                    .replace("deref text = \"after\";", "")
            } else {
                original.to_owned()
            };
            run(&s);
            rejects(&s.replace("consume(saved);", ""), "E_OUTCOME");
            rejects(&s.replace("consume(saved);", "drop(saved);"), "E_RESOURCE");
            rejects(
                &s.replace(
                    "consume(saved);",
                    "auto duplicate=clone(borrow saved);consume(duplicate);consume(saved);",
                ),
                "E_CLONE",
            );
        }
    }
}
#[test]
fn nested_outcome_chains_keep_each_owning_obligation() {
    let s = include_str!("fixtures/source_outcome_chain_nested.tellur");
    assert_eq!(run(s), ["nested"]);
    rejects(&s.replace("consume(saved);", ""), "E_OUTCOME");
    rejects(
        &s.replace("delta value {", "consume(saved);delta value {"),
        "E_LOAN",
    );
    let detached = s
        .replace(
            "[entry]: consume(saved);",
            "[empty|none|absent]:consume(saved);[entry]:",
        )
        .replace("emit(text);", "consume(saved);emit(text);");
    assert_eq!(run(&detached), ["nested"]);
    for constructor in [
        "holder.empty {}",
        "holder.wrapped { none {} }",
        "holder.wrapped { some { reply.empty {} } }",
    ] {
        let empty = s
            .replace(
                "holder.wrapped { some { reply.held { borrow owner } } }",
                constructor,
            )
            .replace("auto child =", "owner=\"available\";auto child =");
        assert_eq!(run(&empty), Vec::<String>::new());
        rejects(&empty.replace("consume(saved);", ""), "E_OUTCOME");
    }
}
#[test]
fn owned_outcome_chain_roots_transfer_handling_through_repeated_joins() {
    let s = include_str!("fixtures/source_outcome_chain_owned.tellur");
    assert_eq!(run(s), ["owned"]);
    rejects(&s.replace("consume(saved);", ""), "E_OUTCOME");
    rejects(
        &s.replace("auto second =", "drop(returned);auto second ="),
        "E_RESOURCE",
    );
    rejects(
        &s.replace(
            "auto second =",
            "auto duplicate=clone(borrow returned);auto second =",
        ),
        "E_CLONE",
    );
    rejects(
        &s.replace("emit(text);", "consume(saved);emit(text);"),
        "E_LOAN",
    );
    for marker in ["delta await child", "auto second =", "delta await second"] {
        rejects(
            &s.replace(marker, &format!("consume(saved);{marker}")),
            "E_LOAN",
        );
    }
}
#[test]
fn outcome_chains_keep_nominal_identity_and_local_storage_escape_checks() {
    rejects(
        "outcome reply['a] {held {ref string text <- 'a},empty}struct chain['holder,'a] {ref reply['a] item <- 'holder;}async control escape(ref string input) exits ready... {reply local=reply.held {input};exit ready {chain {borrow local}};}",
        "E_LIFETIME",
    );
    let s = GUARDS.replace(
        "mutable choice saved = choice.held",
        "mutable other saved = other.held",
    );
    rejects(
        &format!("outcome other['a] {{held {{ref mutable string text <- 'a}},empty}}{s}"),
        "E_TYPE",
    );
    let anonymous = "control view(ref string input) exits found {ref string text from input} {exit found {input};}async control lend(ref outcome<view> input) exits ready {ref outcome<view> value from input} {exit ready {input};}async control caller(ref outcome<view> input) exits done {auto child=spawn lend(input);delta await child {ready {auto value}=>ready}[ready]:exit done;}";
    rejects(anonymous, "E_MANIFEST");
}
