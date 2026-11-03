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
const SOURCE: &str = include_str!("../playground/carrier_spawn.tellur");
#[test]
fn flat_carrier_captures_preserve_schedule_moves_and_selected_origins() {
    let p = compile(SOURCE, manifest()).unwrap();
    let expected = p.run_completion(&scenario(), 1000).unwrap();
    let json = serde_json::to_value(&expected).unwrap();
    let messages: Vec<_> = json["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "queued",
            "other capture",
            "editing",
            "released at join",
            "selected",
            "edited in child",
            "released after edit"
        ]
    );
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let input = scenario();
            let mut session = p.start_execution(&input, 1000);
            assert!(matches!(session.advance(0), ExecutionProgress::Paused));
            let mut completed = false;
            for _ in 0..1000 {
                match session.advance(quota) {
                    ExecutionProgress::Paused => {}
                    ExecutionProgress::Complete(result) => {
                        assert_eq!(result.unwrap(), expected);
                        completed = true;
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            }
            assert!(completed);
        }
    }
}
#[test]
fn carrier_capture_and_result_loans_cover_every_child_phase() {
    for handoff in ["", "yield;", "yield;yield;"] {
        let source = SOURCE.replace("    yield;\n    yield;", handoff);
        compile(&source, manifest())
            .unwrap()
            .run(&scenario())
            .unwrap();
        for (marker, action) in [
            ("emit(\"queued\");", "left=\"conflict\";"),
            ("auto moved = move selected;", "right=\"conflict\";"),
            ("auto moved = move selected;", "text=\"conflict\";"),
            ("emit(value);", "left=\"conflict\";"),
            ("emit(changed);", "text=\"conflict\";"),
        ] {
            let bad = source.replace(marker, &format!("{action}{marker}"));
            assert_eq!(
                compile(&bad, manifest()).unwrap_err().code,
                "E_LOAN",
                "{action} at {marker}"
            );
        }
    }
}
#[test]
fn shared_carriers_copy_but_exclusive_carriers_transfer() {
    let source = SOURCE
        .replace("spawn first(pair)", "spawn first(move pair)")
        .replace("emit(\"queued\");", "emit(pair.right);emit(\"queued\");");
    compile(&source, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
    let bad = SOURCE.replace("emit(\"queued\");", "emit(view.text);emit(\"queued\");");
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_MOVED");
}

#[test]
fn flat_capture_keeps_detached_guard_and_exclusive_parent_authority() {
    let source = include_str!("fixtures/source_carrier_guards.tellur");
    let p = compile(source, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.events.len(), 3);
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        expected
    );
    for marker in ["auto moved = move pending;", "deref value = \"after\";"] {
        for invalid in [
            "drop(saved);",
            "owner=\"bad\";",
            "deref carrier.text=\"bad\";",
            "deref detached=\"bad\";",
        ] {
            let bad = source.replacen(marker, &format!("{invalid}{marker}"), 1);
            assert_eq!(
                compile(&bad, manifest()).unwrap_err().code,
                "E_LOAN",
                "{invalid}"
            );
        }
    }
}

#[test]
fn carrier_fault_settles_awaited_owner_and_suppresses_stopped_handlers() {
    let source = include_str!("fixtures/source_carrier_fault.tellur");
    let p = compile(source, manifest()).unwrap();
    for p in [p.clone(), p.expanded("main").unwrap()] {
        let result = serde_json::to_value(p.run_completion(&scenario(), 1000).unwrap()).unwrap();
        assert_eq!(result["status"], 23);
        let messages: Vec<_> = result["events"]
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

#[test]
fn shared_siblings_and_disjoint_exclusive_fields_keep_instance_loans() {
    let source = r#"
struct view['a] {ref string text <- 'a;}
struct edit_view['a] {ref mutable string text <- 'a;}
struct pair {string left;string right;}
async control display(view input) exits done {yield;emit(input.text);exit done;}
async control edit(edit_view input) exits done {yield;deref input.text="edited";exit done;}
async control startup() exits done {
    mutable string text="shared";view saved=view {borrow text};
    auto first=spawn display(saved);auto second=spawn display(saved);drop(saved);
    yield;await first;await second;text="available";
    mutable pair values=pair {"left","right"};
    auto left=spawn edit(edit_view {borrow mutable values.left});
    auto right=spawn edit(edit_view {borrow mutable values.right});
    await left;values.left="released";await right;values.right="released";exit done;
}
control main() exits done {forward run startup();}
"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    for bad in [
        source.replace(
            "await first;await second;text=\"available\";",
            "await first;text=\"available\";await second;",
        ),
        source.replace("borrow mutable values.right", "borrow mutable values.left"),
        source.replace(
            "await right;values.right=\"released\";",
            "values.right=\"released\";await right;",
        ),
    ] {
        assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    }
}

#[test]
fn concrete_carrier_forwarding_selects_alternative_result_origins() {
    let source = r#"
struct view<t>['a,'b] {ref t left <- 'a;ref t right <- 'b;}
async control select<t>(view<t> input,bool choose) exits left {ref t value from input['a]}, right {ref t value from input['b]} {
    yield;delta choose {true=>left,false=>right}
    [left]:exit left {input.left};[right]:exit right {input.right};
}
async control relay<t>(view<t> input,bool choose) exits left {ref t value from input['a]}, right {ref t value from input['b]} {
    auto child=spawn select<t>(input,choose);forward await child;
}
async control startup() exits done {
    mutable string a="left";mutable string b="right";
    auto child=spawn relay<string>(view<string> {borrow a,borrow b},true);
    auto result=capture await child;
    delta result {left {auto value}=>left,right {auto other}=>right}
    [left]:b="released";emit(value);a="free";exit done;
    [right]:a="released";emit(other);b="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    for choose in ["true", "false"] {
        let source = source.replace("borrow b},true)", &format!("borrow b}},{choose})"));
        let p = compile(&source, manifest()).unwrap();
        assert_eq!(
            p.run(&scenario()).unwrap(),
            p.expanded("main").unwrap().run(&scenario()).unwrap()
        );
        for bad in [
            source.replace("[left]:b=", "[left]:a="),
            source.replace("[right]:a=", "[right]:b="),
        ] {
            assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
        }
    }
}

#[test]
fn record_reference_chains_are_admitted_but_borrowed_handle_wrappers_stay_gated() {
    for ty in ["indirect"] {
        let source = format!(
            "struct view['a] {{ref string text <- 'a;}} struct nested['a] {{view['a] value;}} struct indirect['storage,'a] {{ref view['a] value <- 'storage;}} async control work({ty} input) exits done {{exit done;}} async control caller({ty} input) exits done {{auto child=spawn work(input);await child;exit done;}}"
        );
        compile(&source, manifest()).unwrap();
    }
    let wrapped=SOURCE.replace("auto moved = move selected;","auto wrapped=some {selected};delta wrapped {some {auto moved}=>ready,none=>empty} [empty]:exit done;[ready]:");
    assert_eq!(
        compile(&wrapped, manifest()).unwrap_err().code,
        "E_MANIFEST"
    );
}

#[test]
fn carrier_capture_constructors_stage_before_queueing_and_reject_later_aliases() {
    let source = r#"
struct view['a] {ref string text <- 'a;constructor(ref string input)[input] {emit("constructed");self.text=input;}}
struct marker {string text;constructor(ref mutable string input) {deref input="changed";self.text="marker";}}
async control show(view input,marker other) exits done {emit(input.text);exit done;}
async control startup() exits done {
    mutable string owner="owner";mutable string scratch="scratch";
    auto pending=spawn show(view(borrow owner),marker(borrow mutable scratch));
    emit(borrow scratch);await pending;owner="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    let p = compile(source, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let value = serde_json::to_value(result).unwrap();
    let messages: Vec<_> = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(messages, ["constructed", "changed", "owner"]);
    let bad = source.replace(
        "marker(borrow mutable scratch)",
        "marker(borrow mutable owner)",
    );
    assert!(matches!(
        compile(&bad, manifest()).unwrap_err().code,
        "E_ALIAS" | "E_LOAN"
    ));
}

#[test]
fn carrier_capture_owned_fields_still_require_producer_stop_dispositions() {
    let source = r#"
struct job['a] {ref string text <- 'a;handle owned;}
async control finish(job input) exits done {
    yield;
    delta close(input.owned) {closed=>closed,failed {close_error e}=>failed}
    [closed|failed]:emit(input.text);exit done;
}
async control startup(handle owned,ref string text) exits done {
    auto pending=spawn finish(job {text,owned});await pending;exit done;
}
"#;
    compile(&source.replace("spawn ", "defer "), manifest()).unwrap();
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, "E_ASYNC");
    assert!(error.message.contains("fault dispositions"));
    compile(
        &source
            .replace("    yield;", "")
            .replace("[closed|failed]:emit", "[closed|failed]:yield;emit"),
        manifest(),
    )
    .unwrap();
}
