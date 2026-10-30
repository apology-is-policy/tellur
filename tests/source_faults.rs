mod support;
use std::{fs, process::Command};
use tellur::{compile, fault::Kind, ir::Value, manifest::Manifest, runtime::Scenario};

fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn checked(source: &str) -> tellur::CheckedProgram {
    compile(source, manifest()).unwrap()
}
fn scenario(arguments: Vec<Value>) -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments,
        calls: vec![],
    }
}
fn rejects(source: &str, code: &str) {
    assert_eq!(
        compile(source, manifest()).unwrap_err().code,
        code,
        "{source}"
    );
}

const BRANCH: &str = "control stop(string reason) faults { emit(\"before\"); fault reason; }\ncontrol main(bool valid) exits done { delta valid {true=>ok,false=>bad} [ok]:exit done; [bad]:stop(\"broken invariant\"); }";

#[test]
fn faults_terminate_boundaries_and_private_calls_infer_permission() {
    let p = checked(BRANCH);
    assert!(p.region("main").unwrap().faults);
    assert!(p.region("stop").unwrap().exits.is_empty());
    assert_eq!(
        p.run(&scenario(vec![Value::Bool(true)])).unwrap().outcome,
        "done"
    );
    let failure = p
        .run_detailed(&scenario(vec![Value::Bool(false)]), 100)
        .unwrap_err();
    assert_eq!(failure.diagnostic.code, "E_FAULT");
    assert_eq!(failure.diagnostic.message, "broken invariant");
    assert_eq!(failure.report.kind, Kind::SourceFault);
    assert_eq!(
        failure
            .report
            .activations
            .iter()
            .map(|a| a.control.as_str())
            .collect::<Vec<_>>(),
        ["main", "stop"]
    );
    assert_eq!(failure.events.len(), 1);
    let expanded = p.expanded("main").unwrap();
    let agreement = tellur::algebra::compare_scenarios(
        &p,
        &expanded,
        &[
            scenario(vec![Value::Bool(false)]),
            scenario(vec![Value::Bool(true)]),
        ],
        100,
    )
    .unwrap();
    assert!(agreement.observations_match && agreement.fault_permissions_equal);
}

#[test]
fn public_permissions_and_fault_only_termination_are_checked() {
    checked("pub control main() faults {fault \"broken\";}");
    checked(
        "pub control main(bool ok) exits done faults {delta ok {true=>exit done,false=>fault \"broken\"}}",
    );
    rejects(
        "pub control main() exits done {fault \"broken\";}",
        "E_FAULT_CONTRACT",
    );
    rejects(
        &BRANCH.replace("control main", "pub control main"),
        "E_FAULT_CONTRACT",
    );
    rejects("control main() faults {exit done;}", "E_EXIT");
    rejects("control main() faults {fault 42;}", "E_TYPE");
    rejects(
        "control main() faults {fault \"broken\";emit(\"after\");}",
        "E_TERMINATED",
    );
    rejects(
        "control stop() faults {fault \"broken\";}control main() exits auto {auto result=capture stop();}",
        "E_OUTCOME",
    );
    let p = checked("control stop() faults {fault \"broken\";}control main() exits auto {stop();}");
    assert!(p.region("main").unwrap().exits.is_empty());
    checked("control stop() faults {fault \"broken\";}control main() faults {forward stop();}");
}

#[test]
fn resources_retained_by_callers_require_fault_dispositions() {
    rejects(
        "control main(handle file) faults {fault \"broken\";}",
        "E_FAULT_RESOURCE",
    );
    rejects(
        "control stop() faults {fault \"broken\";}control main(handle file) faults {stop();}",
        "E_FAULT_RESOURCE",
    );
    rejects(
        "struct box {handle file;}control main(box value) faults {fault \"broken\";}",
        "E_FAULT_RESOURCE",
    );
    rejects(
        "outcome result {ready {handle file},missing}control main(result value) faults {fault \"broken\";}",
        "E_FAULT_RESOURCE",
    );
    checked(
        "outcome result {ready {string text},missing}control main(result value) faults {fault \"broken\";}",
    );
    checked(
        "control finish(handle file) faults {delta close(file) {closed=>closed,failed {close_error error}=>failed} [closed | failed]:fault \"after completion\";} control main(handle file) faults {finish(file);}",
    );
}

#[test]
fn generic_and_trait_fault_permissions_survive_universal_checking() {
    checked(
        "control stop<t: drop>(t value) faults {drop(value);fault \"broken\";}control main() faults {stop<string>(\"text\");}",
    );
    rejects(
        "control stop<t>(t value) faults {fault \"broken\";}",
        "E_FAULT_RESOURCE",
    );
    rejects(
        "pub control stop<t:drop>(t value) exits done {drop(value);fault \"broken\";}",
        "E_FAULT_CONTRACT",
    );
    checked(
        "trait task {control run(self input) faults;} implement task for string {control run(self input) faults {fault input;}} control main() faults {task<string>.run(\"broken\");}",
    );
    rejects(
        "trait task {control run(self input) exits done;} implement task for string {control run(self input) exits done {fault input;}}",
        "E_FAULT_CONTRACT",
    );
    checked(
        "trait task {control run(self input) exits done faults;} implement task for string {control run(self input) exits done {exit done;}} control main() exits done {task<string>.run(\"text\");exit done;}",
    );
}

#[test]
fn protocol_faults_are_terminal_and_distinct_from_normal_completion() {
    let p = checked(BRANCH);
    let protocol = p.protocol("main", 100).unwrap();
    let root = protocol.residual(&[]);
    assert!(root.can_complete && root.can_fault);
    let end = protocol.residual(&[
        tellur::protocol::Symbol::Emit,
        tellur::protocol::Symbol::Fault,
    ]);
    assert!(end.prefix_possible && end.faulted && !end.complete && !end.can_complete);
    assert!(end.next_symbols.is_empty());
    let other = checked("control main(bool valid) exits done {exit done;}")
        .protocol("main", 100)
        .unwrap();
    assert!(!protocol.included_in(&other, 100).unwrap().included);
    assert!(
        protocol
            .included_in(
                &p.expanded("main").unwrap().protocol("main", 100).unwrap(),
                100
            )
            .unwrap()
            .included
    );
}

#[test]
fn native_faults_match_reference_and_expansion_at_both_optimization_levels() {
    let root = std::env::temp_dir().join(format!("tellur-source-fault-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("main.tellur");
    fs::write(&source, BRANCH).unwrap();
    let p = tellur::compile_file(&source, manifest()).unwrap();
    let s = scenario(vec![Value::Bool(false)]);
    let scenario_path = root.join("scenario.json");
    fs::write(&scenario_path, serde_json::to_vec(&s).unwrap()).unwrap();
    for expanded in [false, true] {
        for optimization in [0, 2] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            let expected = p.run_detailed(&s, 100).unwrap_err();
            let binary = root.join(format!("fault-{expanded}-{optimization}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, optimization)
                .unwrap();
            let output = Command::new(&binary)
                .args(["--scenario", scenario_path.to_str().unwrap()])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            let report: tellur::fault::Report = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(report, expected.report);
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn finite_comparisons_observe_fault_messages_effects_and_permissions() {
    let first = checked("control main() faults {emit(\"before\");fault \"broken\";}");
    let compare = |source| {
        tellur::algebra::compare_scenarios(&first, &checked(source), &[scenario(vec![])], 100)
            .unwrap()
    };
    assert!(
        compare("control main() faults {emit(\"before\");fault \"broken\";}").observations_match
    );
    assert!(
        !compare("control main() faults {emit(\"before\");fault \"different\";}")
            .observations_match
    );
    assert!(!compare("control main() faults {fault \"broken\";}").observations_match);
    let normal = checked("control main() exits done {exit done;}");
    let permitted = checked("control main() exits done faults {exit done;}");
    let agreement =
        tellur::algebra::compare_scenarios(&normal, &permitted, &[scenario(vec![])], 100).unwrap();
    assert!(agreement.observations_match);
    assert!(!agreement.fault_permissions_equal);
    assert_eq!(
        tellur::algebra::compare_scenarios(&first, &first, &[scenario(vec![])], 1)
            .unwrap_err()
            .code,
        "E_COMPARE_INCONCLUSIVE"
    );
}

#[test]
fn fault_only_extraction_and_bounded_messages_preserve_semantics() {
    let source = "control main(string reason) faults {\n emit(\"before\");\n fault reason;\n}\n";
    let extracted = tellur::extract::extract_tail(source, manifest(), "main", 3, "stop").unwrap();
    assert!(
        extracted
            .source
            .contains("control stop(string reason) faults")
    );
    let p = checked(source);
    let message = "😀".repeat(5000);
    let input = scenario(vec![Value::String(message.clone())]);
    let failure = p.run_detailed(&input, 100).unwrap_err();
    assert!(failure.report.message_truncated);
    assert!(failure.report.message.chars().count() <= 4096);
    assert_eq!(failure.diagnostic.message, message);
    assert!(
        tellur::algebra::compare_scenarios(&p, &checked(&extracted.source), &[input], 100)
            .unwrap()
            .observations_match
    );
}

#[test]
fn fault_permissions_cross_file_imports_and_public_snapshots() {
    let root = std::env::temp_dir().join(format!("tellur-imported-fault-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("library.tellur"),
        "control stop() faults {fault \"broken\";}pub control run() faults {stop();}",
    )
    .unwrap();
    let main = root.join("main.tellur");
    fs::write(
        &main,
        "import \"library.tellur\" as lib;pub control main() faults {lib.run();}",
    )
    .unwrap();
    let p = tellur::compile_file(&main, manifest()).unwrap();
    let failure = p.run_detailed(&scenario(vec![]), 100).unwrap_err();
    assert_eq!(failure.report.kind, Kind::SourceFault);
    assert!(
        failure
            .report
            .site
            .unwrap()
            .path
            .unwrap()
            .ends_with("library.tellur")
    );
    let first = tellur::modules::contract_snapshot(&main, manifest()).unwrap();
    let json = serde_json::to_value(&first).unwrap();
    assert_eq!(
        json["modules"]["@entry"]["declarations"]["main"]["faults"],
        true
    );
    fs::write(
        &main,
        "import \"library.tellur\" as lib;pub control main() exits done {lib.run();}",
    )
    .unwrap();
    assert_eq!(
        tellur::compile_file(&main, manifest())
            .unwrap_err()
            .diagnostic
            .code,
        "E_FAULT_CONTRACT"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fault_transforms_cannot_forge_completion_or_erase_permission() {
    let p = checked(BRANCH);
    let mut artifact = p.expansion_artifact("main").unwrap();
    // Expansion retains fault termination rather than rewriting it to a join.
    assert!(
        artifact
            .region
            .code
            .iter()
            .any(|i| matches!(i.action, tellur::ir::Action::Fault(_)))
    );
    artifact.region.faults = false;
    assert_eq!(
        p.accept_expansion(&artifact).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
    let mut artifact = p.expansion_artifact("main").unwrap();
    for i in &mut artifact.region.code {
        if matches!(i.action, tellur::ir::Action::Fault(_)) {
            i.action = tellur::ir::Action::Exit("done".into(), vec![]);
        }
    }
    assert_eq!(
        p.accept_expansion(&artifact).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}

#[test]
fn retained_caller_resources_and_loop_faults_are_checked_on_their_histories() {
    let finish = "control finish(handle file) faults {delta close(file) {closed=>closed,failed {close_error error}=>failed} [closed | failed]:fault \"after completion\";}";
    rejects(
        &format!("{finish} control main(handle first,handle second) faults {{finish(first);}}"),
        "E_FAULT_RESOURCE",
    );
    let p = checked("control main() faults {loop retry(u32 attempt=0){fault \"loop invariant\";}}");
    assert_eq!(
        p.run_detailed(&scenario(vec![]), 100)
            .unwrap_err()
            .report
            .kind,
        Kind::SourceFault
    );
    assert!(
        p.expanded("main")
            .unwrap()
            .protocol("main", 100)
            .unwrap()
            .residual(&[])
            .can_fault
    );
}
