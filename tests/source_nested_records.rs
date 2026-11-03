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
const SOURCE: &str = include_str!("../playground/nested_spawn.tellur");
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    p
}
#[test]
fn nested_records_preserve_schedules_and_partial_result_moves() {
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
            "shared",
            "queued",
            "worker",
            "shared",
            "worker",
            "left edited",
            "right edited"
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
fn nested_leaf_loans_survive_queued_completed_and_partially_moved_records() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoff);
        same(&source);
        for (marker, bad) in [
            ("auto moved = move child;", "left=\"bad\";"),
            ("auto moved = move child;", "right=\"bad\";"),
            ("auto fields = move result.values;", "left=\"bad\";"),
            ("emit(selected.text);", "left=\"bad\";"),
            ("emit(fields.right.text);", "right=\"bad\";"),
        ] {
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
    let bad = SOURCE.replace(
        "emit(selected.text);",
        "emit(fields.left.text);emit(selected.text);",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_MOVED");
}
#[test]
fn nested_detached_result_guards_preserve_exclusive_ancestry() {
    let source = include_str!("fixtures/source_nested_guards.tellur");
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
fn nested_results_settle_before_awaited_owner_contingency() {
    let text = include_str!("fixtures/source_nested_fault.tellur");
    let joined = text
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    assert_ne!(text, joined);
    for source in [text, joined.as_str()] {
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
fn generic_nested_records_select_independent_result_leaves() {
    let source = r#"
struct leaf<t>['a] {ref t text <- 'a;}
struct pair<t>['a,'b] {leaf<t>['a] left;leaf<t>['b] right;}
struct envelope<t>['a,'b] {pair<t>['a,'b] values;}
async control select<t>(envelope<t> input,bool yes) exits first {leaf<t>[input['a]] value},second {leaf<t>[input['b]] value} {
    yield;delta yes {true=>yes,false=>no} [yes]:exit first {input.values.left};[no]:exit second {input.values.right};
}
async control relay<t>(envelope<t> input,bool yes) exits first {leaf<t>[input['a]] value},second {leaf<t>[input['b]] value} {
    auto child=spawn select<t>(input,yes);forward await child;
}
async control startup() exits done {
    mutable string a="a";mutable string b="b";
    auto child=spawn relay<string>(envelope<string> {pair<string> {leaf<string> {borrow a},leaf<string> {borrow b}}},true);
    auto result=capture await child;delta result {first {auto first}=>first,second {auto second}=>second}
    [first]:b="free";emit(first.text);a="free";exit done;
    [second]:a="free";emit(second.text);b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    for source in [source.to_owned(), source.replace("}}},true)", "}}},false)")] {
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
fn nested_shared_copies_and_exclusive_reuse_keep_ordinary_rules() {
    let source = SOURCE.replace("spawn inspect(observer)", "spawn inspect(move observer)");
    same(&source);
    let exclusive = r#"
struct leaf['a] {ref mutable string text <- 'a;}
struct inner['a] {leaf['a] item;}
struct outer['a] {inner['a] item;}
async control work(outer input) exits done {yield;exit done;}
async control startup() exits done {
    mutable string owner="owner";outer input=outer {inner {leaf {borrow mutable owner}}};
    auto child=spawn work(move input);await child;owner="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    same(exclusive);
    assert_eq!(
        compile(
            &exclusive.replace("await child;", "emit(input.item.item.text);await child;"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_MOVED"
    );
    // A returned exclusive subrecord cannot be read through its moved source.
    let bad = SOURCE.replace(
        "auto selected = move fields.left;",
        "auto selected = move fields.left;emit(fields.left.text);",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_MOVED");
}
#[test]
fn record_reference_recursion_is_admitted_while_collections_keep_origin_limits() {
    for ty in ["ref leaf", "vector<ref string>"] {
        let source = format!(
            "struct leaf['a] {{ref string text <- 'a;}}struct inner['a] {{{ty} value;}}struct outer['a] {{inner['a] value;}}async control work(outer input) exits done {{exit done;}}async control start(outer input) exits done {{auto child=spawn work(input);await child;exit done;}}"
        );
        // Every reference-bearing field needs its existing declared group mapping.
        let source = source
            .replace("ref leaf value;", "ref leaf['a] value <- 'a;")
            .replace("vector<ref string> value;", "vector<ref string>['a] value;");
        let expected = if ty.starts_with("vector") {
            // Collection-contained origin signatures are not admitted even cold.
            assert_eq!(
                compile(&source.replace("spawn ", "defer "), manifest())
                    .unwrap_err()
                    .code,
                "E_ORIGIN"
            );
            "E_ORIGIN"
        } else {
            compile(&source.replace("spawn ", "defer "), manifest()).unwrap();
            compile(&source, manifest()).unwrap();
            continue;
        };
        assert_eq!(
            compile(&source, manifest()).unwrap_err().code,
            expected,
            "{source}"
        );
    }
}

#[test]
fn nested_sibling_instances_keep_shared_and_disjoint_exclusive_loans() {
    let source = r#"
struct leaf['a] {ref string text <- 'a;}struct view['a] {leaf['a] item;}
struct edit_leaf['a] {ref mutable string text <- 'a;}struct edit_view['a] {edit_leaf['a] item;}
struct pair {string left;string right;}
async control display(view input) exits done {yield;emit(input.item.text);exit done;}
async control edit(edit_view input) exits done {yield;deref input.item.text="edited";exit done;}
async control startup() exits done {
    mutable string text="shared";view saved=view {leaf {borrow text}};
    auto first=spawn display(saved);auto second=spawn display(saved);drop(saved);
    yield;await first;await second;text="free";
    mutable pair values=pair {"left","right"};
    auto left=spawn edit(edit_view {edit_leaf {borrow mutable values.left}});
    auto right=spawn edit(edit_view {edit_leaf {borrow mutable values.right}});
    await left;values.left="free";await right;values.right="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    same(source);
    for bad in [
        source.replace(
            "await first;await second;text=\"free\";",
            "await first;text=\"free\";await second;",
        ),
        source.replace("borrow mutable values.right", "borrow mutable values.left"),
        source.replace(
            "await right;values.right=\"free\";",
            "values.right=\"free\";await right;",
        ),
    ] {
        assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    }
}
