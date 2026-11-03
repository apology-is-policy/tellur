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
const SOURCE: &str = include_str!("../playground/tagged_spawn.tellur");
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    p
}
#[test]
fn tagged_payloads_preserve_selected_and_empty_schedules() {
    let p = same(SOURCE);
    let expected = p.run_completion(&scenario(), 1000).unwrap();
    let v = serde_json::to_value(&expected).unwrap();
    let messages: Vec<_> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(messages, ["left", "right", "empty", "edited", "none"]);
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
fn tagged_permissions_survive_queue_completion_join_and_selection() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoff);
        same(&source);
        for (marker, bad) in [
            ("auto moved = move pending;", "left=\"bad\";"),
            ("auto moved = move pending;", "right=\"bad\";"),
            ("emit(a);", "left=\"bad\";"),
            ("emit(b);", "right=\"bad\";"),
            ("delta await child", "owner=\"bad\";"),
            ("deref text = \"edited\";", "owner=\"bad\";"),
        ] {
            assert!(source.contains(marker), "{marker}");
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
}
#[test]
fn tagged_result_guards_keep_parent_and_carrier_authority() {
    let source = include_str!("fixtures/source_tagged_guards.tellur");
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
fn tagged_results_settle_before_awaited_owner_contingency() {
    let source = include_str!("fixtures/source_tagged_fault.tellur");
    let joined = source
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    assert_ne!(joined, source);
    for source in [source, joined.as_str()] {
        let p = compile(source, manifest()).unwrap();
        for p in [p.clone(), p.expanded("main").unwrap()] {
            let v = serde_json::to_value(p.run_completion(&scenario(), 1000).unwrap()).unwrap();
            assert_eq!(v["status"], 23);
            let messages: Vec<_> = v["events"]
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
fn tagged_copies_moves_and_independent_sibling_loans_remain_ordinary() {
    let source = r#"
variant choice['a] {held {ref string text <- 'a},empty}
async control relay(choice input) exits ready {choice[input['a]] value} {yield;exit ready {input};}
async control startup() exits done {
    mutable string owner="shared";choice input=choice.held {borrow owner};
    auto first=spawn relay(move input);auto second=spawn relay(input);
    delta input {held {auto alias}=>held,empty=>empty}
    [held]:emit(alias);[held|empty]:delta await first {ready {auto ignored}=>joined} [joined]:drop(ignored);
    delta await second {ready {auto result}=>ready}
    [ready]:delta result {held {auto value}=>value,empty=>none}
    [value]:emit(value);[value|none]:owner="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    same(source);
    assert_eq!(
        compile(
            &source.replace("delta await second", "owner=\"bad\";delta await second"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    let exclusive = source
        .replace("ref string", "ref mutable string")
        .replace("borrow owner", "borrow mutable owner");
    assert_eq!(compile(&exclusive, manifest()).unwrap_err().code, "E_MOVED");
    let one = r#"variant choice['a] {held {ref mutable string text <- 'a},empty}
async control relay(choice input) exits ready {choice[input['a]] value} {yield;exit ready {input};}
async control startup() exits done {mutable string owner="exclusive";auto child=spawn relay(choice.held {borrow mutable owner});delta await child {ready {auto result}=>ready} [ready]:delta result {held {auto text}=>held,empty=>empty} [held]:deref text="updated";emit(text);[held|empty]:owner="free";exit done;}
control main() exits done {forward run startup();}"#;
    same(one);
}
#[test]
fn named_outcomes_borrowed_carriers_and_reference_pointees_stay_gated() {
    for source in [
        "outcome result['a] {ready {ref string text <- 'a}} async control work(result input) exits done {delta input {ready {auto text}=>ready} [ready]:emit(text);exit done;} async control start(result input) exits done {auto child=spawn work(input);await child;exit done;}",
        "struct view['a] {ref string text <- 'a;}async control work(option<ref view> input) exits done {exit done;}async control start(option<ref view> input) exits done {auto child=spawn work(input);await child;exit done;}",
        "variant carrier['storage,'a] {held {ref view['a] value <- 'storage}}struct view['a] {ref string text <- 'a;}async control work(carrier input) exits done {exit done;}async control start(carrier input) exits done {auto child=spawn work(input);await child;exit done;}",
    ] {
        compile(&source.replace("spawn ", "defer "), manifest()).unwrap();
        assert_eq!(
            compile(source, manifest()).unwrap_err().code,
            "E_ASYNC",
            "{source}"
        );
    }
}
#[test]
fn generic_tagged_forwarding_and_disjoint_exclusive_children_keep_origins() {
    let forwarded = SOURCE.replace("async control relay<t>", "async control leaf<t>");
    let forwarded = format!(
        "{forwarded}\nasync control relay<t>(request<t> input) exits ready {{request<t>[input['left],input['right]] value}} {{auto child=spawn leaf<t>(input);forward await child;}}"
    );
    same(&forwarded);
    let source = r#"
struct pair {string a;string b;}
async control edit(option<ref mutable string> input) exits done {yield;delta input {some {auto text}=>some,none=>none} [some]:deref text="edited";[some|none]:exit done;}
async control start() exits done {mutable pair values=pair {"a","b"};auto first=spawn edit(some {borrow mutable values.a});auto second=spawn edit(some {borrow mutable values.b});await first;values.a="free";await second;values.b="free";exit done;}
control main() exits done {forward run start();}
"#;
    same(source);
    for bad in [
        source.replace("borrow mutable values.b", "borrow mutable values.a"),
        source.replace("values.a=\"free\"", "values.b=\"bad\""),
    ] {
        assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    }
}
#[test]
fn tagged_constructor_arguments_stage_before_queueing_and_later_borrows() {
    let source = r#"
struct view['a] {ref string text <- 'a;constructor(ref string input)[input] {emit("constructed");self.text=input;}}
struct marker {string text;constructor(ref mutable string input) {deref input="changed";self.text="marker";}}
async control show(option<view> input,marker other) exits done {yield;delta input {some {auto value}=>some,none=>none} [some]:emit(value.text);[some|none]:exit done;}
async control start() exits done {mutable string owner="owner";mutable string scratch="scratch";auto pending=spawn show(some {view(borrow owner)},marker(borrow mutable scratch));emit(borrow scratch);await pending;owner="free";exit done;}
control main() exits done {forward run start();}
"#;
    let p = same(source);
    let v = serde_json::to_value(p.run_completion(&scenario(), 1000).unwrap()).unwrap();
    let messages: Vec<_> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(messages, ["constructed", "changed", "owner"]);
    assert_eq!(
        compile(
            &source.replace(
                "marker(borrow mutable scratch)",
                "marker(borrow mutable owner)"
            ),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_ALIAS"
    );
}
