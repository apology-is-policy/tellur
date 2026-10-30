mod support;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{
    fault::{Phase, Report, Site, Tracker},
    ir::Value,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-fault-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn program(&self, source: &str) -> tellur::CheckedProgram {
        fs::write(self.root.join("main.tellur"), source).unwrap();
        fs::write(
            self.root.join("manifest.json"),
            include_str!("../playground/manifest.json"),
        )
        .unwrap();
        tellur::compile_file(
            self.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
const SOURCE: &str = "control leaf(string text) exits done {\n delta parse_u32(text) {number {u32 value}=>exit done, malformed=>exit done, overflow=>exit done}\n}\ncontrol main() exits done {\n delta leaf(\"1\") {done=>first}\n [first]:forward leaf(\"2\");\n}";
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![ScriptCall {
            operation: "parse_u32".into(),
            outcome: "number".into(),
            fields: vec![Value::U32(1)],
        }],
    }
}
#[test]
fn second_invocation_has_fresh_identity_and_first_failure_keeps_the_active_chain() {
    let f = Fixture::new();
    let p = f.program(SOURCE);
    let failure = p.run_detailed(&scenario(), 100).unwrap_err();
    assert_eq!(failure.diagnostic.code, "E_DRIVER");
    let report = failure.report;
    assert_eq!(report.phase, Phase::Execution);
    assert_eq!(
        report.activations.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(report.activations[1].parent, Some(1));
    assert_eq!(report.activations[1].control, "leaf");
    assert_eq!(report.activations[1].called_at.as_ref().unwrap().line, 6);
    assert_eq!(report.site.as_ref().unwrap().line, 2);
    assert_eq!(report.site.as_ref().unwrap().control, "leaf");
    assert_eq!(
        p.run(&scenario()).unwrap_err().code,
        failure.diagnostic.code
    );
    let mut complete = scenario();
    complete.calls.push(complete.calls[0].clone());
    assert_eq!(p.run_detailed(&complete, 100).unwrap().outcome, "done");
    assert_eq!(
        p.run_detailed(&scenario(), 100)
            .unwrap_err()
            .report
            .activations[0]
            .id,
        1,
        "a separate execution has a fresh boundary"
    );
}
#[test]
fn imported_sites_and_expansion_origins_remain_source_based() {
    let f = Fixture::new();
    fs::write(
        f.root.join("library.tellur"),
        SOURCE.split("\ncontrol main").next().unwrap().replacen(
            "control leaf",
            "pub control leaf",
            1,
        ),
    )
    .unwrap();
    let p = f.program(
        "import \"library.tellur\" as lib;\ncontrol main() exits done {forward lib.leaf(\"1\");}",
    );
    let mut s = scenario();
    s.calls.clear();
    let report = p.run_detailed(&s, 100).unwrap_err().report;
    assert!(
        report
            .site
            .as_ref()
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("library.tellur")
    );
    assert_eq!(report.site.as_ref().unwrap().line, 2);
    assert_eq!(report.activations[1].called_at.as_ref().unwrap().line, 2);
    let expanded = p
        .expanded("main")
        .unwrap()
        .run_detailed(&s, 100)
        .unwrap_err()
        .report;
    assert_eq!(expanded.activations.len(), 1);
    assert_eq!(expanded.site.as_ref().unwrap().control, "leaf");
    assert!(!expanded.site.as_ref().unwrap().expansion_calls.is_empty());
    assert!(
        !serde_json::to_string(&expanded)
            .unwrap()
            .contains("_path_m_")
    );
}
#[test]
fn entry_and_result_validation_do_not_invent_active_invocations() {
    let f = Fixture::new();
    let p = f.program("control main() exits done {exit done;}");
    let mut s = scenario();
    s.arguments.push(Value::U32(7));
    let report = p.run_detailed(&s, 100).unwrap_err().report;
    assert_eq!(report.phase, Phase::EntryValidation);
    assert!(report.activations.is_empty());
    assert!(report.site.is_none());
    s.arguments.clear();
    let report = p.run_detailed(&s, 100).unwrap_err().report;
    assert_eq!(report.phase, Phase::ResultValidation);
    assert!(report.activations.is_empty());
    assert!(report.site.is_none());
    s.calls.clear();
    assert_eq!(p.run_detailed(&s, 100).unwrap().outcome, "done");
}
#[test]
fn fuel_exhaustion_identifies_the_candidate_instruction_without_becoming_an_outcome() {
    let f = Fixture::new();
    let p = f.program("control main() exits done {loop retry() {again retry();}}");
    let s = Scenario {
        calls: vec![],
        ..scenario()
    };
    let failure = p.run_detailed(&s, 20).unwrap_err();
    assert_eq!(failure.diagnostic.code, "E_FUEL");
    assert_eq!(failure.report.phase, Phase::Execution);
    assert_eq!(failure.report.activations.len(), 1);
    assert!(failure.report.site.is_some());
    assert!(failure.report.message.contains("not a language outcome"));
}
#[test]
fn tracker_reports_bounded_root_and_leaf_context_and_cannot_resume_after_failure() {
    let site = Site {
        control: "work".into(),
        path: None,
        line: 1,
        column: 2,
        label: None,
        expansion_calls: vec![],
        omitted_expansion_calls: 0,
    };
    let mut tracker = Tracker::new(BTreeMap::from([(7, site.clone())]));
    for _ in 0..70 {
        tracker.enter("work").unwrap();
        tracker.at(7);
    }
    let report = tracker.failure("work", Phase::Execution, &"😀".repeat(5000));
    assert_eq!(report.activations.len(), 32);
    assert_eq!(report.omitted_activations, 38);
    assert_eq!(report.activations[0].id, 1);
    assert_eq!(report.activations[31].id, 70);
    assert_eq!(report.message.chars().count(), 4096);
    assert!(report.message_truncated);
    assert_eq!(report.site, Some(site));
    assert!(tracker.enter("another").is_err());
    assert_eq!(
        serde_json::from_str::<Report>(&serde_json::to_string(&report).unwrap()).unwrap(),
        report
    );
}
#[test]
fn native_reports_match_observed_reference_context_at_both_optimization_levels_and_after_expansion()
{
    let f = Fixture::new();
    let program = f.program(SOURCE);
    let scenario = scenario();
    let input = f.root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario).unwrap()).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            program.expanded("main").unwrap()
        } else {
            program.clone()
        };
        let reference = p.run_detailed(&scenario, 100).unwrap_err().report;
        for opt in [0, 2] {
            let binary = f.root.join(format!("program-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let output = Command::new(&binary)
                .arg("--scenario")
                .arg(&input)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(1),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stdout.is_empty());
            let actual: Report = serde_json::from_slice(&output.stderr)
                .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr)));
            assert_eq!(actual.format, reference.format);
            assert_eq!(actual.phase, reference.phase);
            assert_eq!(actual.activations, reference.activations);
            assert_eq!(actual.site, reference.site);
            assert!(actual.message.contains("E_DRIVER"));
            let fuel = Command::new(&binary)
                .args(["--json", "--fuel", "0"])
                .output()
                .unwrap();
            let fuel: Report = serde_json::from_slice(&fuel.stderr).unwrap();
            assert!(fuel.message.contains("E_FUEL"));
            assert_eq!(fuel.activations.len(), 1);
            assert_eq!(fuel.activations[0].id, 1);
            let plain = Command::new(&binary)
                .args(["--fuel", "0"])
                .output()
                .unwrap();
            assert!(
                String::from_utf8_lossy(&plain.stderr).contains("execution boundary `main` failed")
            );
        }
    }
}
#[test]
fn cli_run_serializes_only_execution_failure_when_requested_and_keeps_stdout_empty() {
    let f = Fixture::new();
    f.program(SOURCE);
    fs::write(
        f.root.join("scenario.json"),
        serde_json::to_vec(&scenario()).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .current_dir(&f.root)
        .args([
            "run",
            "main.tellur",
            "manifest.json",
            "scenario.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let report: Report = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(report.activations[1].id, 3);
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .current_dir(&f.root)
        .args(["run", "main.tellur", "manifest.json", "scenario.json"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("activation 3: leaf"));
}

#[test]
fn imported_entry_failures_use_the_same_source_name_in_both_executors() {
    let f = Fixture::new();
    fs::write(
        f.root.join("library.tellur"),
        SOURCE.split("\ncontrol main").next().unwrap().replacen(
            "control leaf",
            "pub control leaf",
            1,
        ),
    )
    .unwrap();
    let p = f.program(
        "import \"library.tellur\" as lib;control main() exits done {forward lib.leaf(\"1\");}",
    );
    let entry = p.source_files()[1]
        .names
        .iter()
        .find(|(_, local)| *local == "leaf")
        .unwrap()
        .0
        .clone();
    let s = Scenario {
        entry: entry.clone(),
        arguments: vec![Value::String("1".into())],
        calls: vec![],
    };
    let expected = p.run_detailed(&s, 100).unwrap_err().report;
    assert_eq!(expected.boundary, "leaf");
    let input = f.root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&s).unwrap()).unwrap();
    let binary = f.root.join("imported");
    support::scenario_executable(&p)
        .build_native(&entry, &binary, 0)
        .unwrap();
    let result = Command::new(&binary)
        .arg("--scenario")
        .arg(&input)
        .output()
        .unwrap();
    let actual: Report = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(actual.boundary, expected.boundary);
    assert_eq!(actual.activations, expected.activations);
    let call = ScriptCall {
        operation: "parse_u32".into(),
        outcome: "number".into(),
        fields: vec![Value::U32(1)],
    };
    let extra = Scenario {
        calls: vec![call.clone(), call],
        ..s
    };
    fs::write(&input, serde_json::to_vec(&extra).unwrap()).unwrap();
    let result = Command::new(&binary)
        .arg("--scenario")
        .arg(&input)
        .output()
        .unwrap();
    let actual: Report = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(actual.boundary, "leaf");
    assert_eq!(actual.phase, Phase::ResultValidation);
    assert!(actual.activations.is_empty());
}
#[test]
fn unknown_entry_text_is_not_parsed_as_a_generic_control_identity() {
    let f = Fixture::new();
    let p = f.program("control main() exits done {exit done;}");
    let name = format!("{}u32{}", "unknown<".repeat(1000), ">".repeat(1000));
    let s = Scenario {
        entry: name.clone(),
        arguments: vec![],
        calls: vec![],
    };
    let failure = p.run_detailed(&s, 100).unwrap_err();
    assert_eq!(failure.report.phase, Phase::EntryValidation);
    assert_eq!(failure.report.boundary, name);
    assert!(failure.report.message_truncated);
}
