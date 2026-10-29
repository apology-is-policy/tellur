use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{
    explain::{self, Details, Query, Report},
    manifest::Manifest,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-explain #😀-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("manifest.json"),
            include_str!("../playground/manifest.json"),
        )
        .unwrap();
        Self { root }
    }
    fn manifest(&self) -> Manifest {
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
    }
    fn write(&self, source: &str) {
        fs::write(self.root.join("entry.tellur"), source).unwrap();
    }
    fn query(&self, source: &str, query: Query) -> Result<Report, String> {
        self.write(source);
        explain::file(self.root.join("entry.tellur"), self.manifest(), query)
    }
    fn selection(&self, source: &str, marker: &str) -> Report {
        self.query(source, at(source, marker)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn at(source: &str, marker: &str) -> Query {
    let before = &source[..source.find(marker).unwrap()];
    Query::Selection {
        line: before.bytes().filter(|&c| c == b'\n').count() + 1,
        column: before.rsplit('\n').next().unwrap().chars().count() + 1,
    }
}
fn outcome(control: &str, outcome: &str) -> Query {
    Query::Outcome {
        control: control.into(),
        outcome: outcome.into(),
    }
}
fn selection(report: &Report) -> &explain::Selection {
    let Details::Selection { selection, .. } = &report.result else {
        panic!("not a selection")
    };
    selection
}
#[test]
fn private_outcome_queries_reuse_snapshot_evidence_and_source_links() {
    let f = Fixture::new();
    let source = "control leaf() exits auto {exit missing;}    control lookup() exits failed {exit delta leaf() {missing=>failed {}};}";
    let report = f.query(source, outcome("lookup", "failed")).unwrap();
    let Details::Outcome { evidence, .. } = report.result else {
        panic!()
    };
    assert!(
        evidence
            .contributors
            .iter()
            .any(|c| c.control == "@entry::leaf" && c.outcome == "missing")
    );
    f.write(&source.replace("    control lookup", "pub control lookup"));
    let snapshot =
        tellur::modules::absorption_snapshot(f.root.join("entry.tellur"), f.manifest()).unwrap();
    assert_eq!(
        evidence,
        snapshot.controls["@entry::lookup"].outcomes["failed"]
    );
    assert!(report.markdown.contains("not an execution sequence"));
    assert!(report.markdown.contains("%23%F0%9F%98%80"));
    assert!(!report.markdown.contains("_path_m_"));
}
#[test]
fn aliases_groups_empty_selections_and_unicode_have_scalar_source_ranges() {
    let f = Fixture::new();
    let source = "control main(bool flag) exits done {\r\n emit(\"😀\"); delta flag {true=>yes,false=>no}\r\n path impossible=yes & no;\r\n [yes]: { emit(\"chosen\"); }\r\n exit done;\r\n}";
    let alias = f.selection(source, "path impossible");
    assert!(selection(&alias).structurally_empty);
    assert_eq!(selection(&alias).condition, "yes & no");
    let chosen = f.selection(source, "chosen");
    assert_eq!(selection(&chosen).condition, "yes");
    assert_eq!(selection(&chosen).decision_arms[0].location.line, 2);
    assert_eq!(selection(&chosen).decision_arms[0].location.column, 25);
    assert_eq!(
        selection(&f.selection(source, "exit done")).condition,
        "entry"
    );
    let delta = f.selection(source, "delta flag");
    let Details::Selection { range, .. } = delta.result else {
        panic!()
    };
    assert_eq!(range.start.column, 13);
    assert!(!selection(&chosen).condition_truncated);
}
#[test]
fn generic_and_trait_selections_are_source_based_while_generic_contributors_are_rejected() {
    let f = Fixture::new();
    let source = "control choose<t>(t input,bool flag) exits ready {t value} {delta flag {true=>left,false=>right} [left|right]:exit ready {input};}\ntrait worker {control run(ref self input,bool flag) exits done;}\nstruct record {u32 value;}\nimplement worker for record {control run(ref self input,bool flag) exits done {delta flag {true=>go,false=>exit done} [go]:exit done;}}\ncontrol main() exits done {exit done;}";
    let report = f.selection(source, "exit ready {input}");
    assert_eq!(selection(&report).condition, "left | right");
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("_path_poly")
    );
    assert_eq!(selection(&f.selection(source, "[go]")).condition, "go");
    assert!(
        f.query(source, outcome("choose", "ready"))
            .unwrap_err()
            .contains("generic definition root")
    );
}
#[test]
fn nested_local_outcomes_disambiguate_names_and_loops_report_one_iteration() {
    let f = Fixture::new();
    let source = "control main(bool a,bool b) exits done {delta a {true=>go,false=>exit done} [go]:exit delta b {true=>delta a {true=>done {},false=>done {}},false=>done {}};}";
    let report = f.selection(source, "done {}");
    assert_eq!(selection(&report).condition.matches("true@").count(), 2);
    assert!(selection(&report).decision_arms.iter().any(|d| d.implicit));
    let source = "control main(bool flag) exits done {delta flag {true=>exit done,false=>continue}loop retry(bool state=true) {delta state {true=>again_path,false=>exit done}[again_path]:again retry(false);}}";
    let report = f.selection(source, "again retry");
    assert_eq!(selection(&report).iteration.as_deref(), Some("retry"));
    assert!(report.markdown.contains("not the whole control entry"));
}
#[test]
fn selection_and_contributor_limits_are_explicit() {
    let f = Fixture::new();
    let mut source = "control main(bool flag) exits done {\n".to_owned();
    for i in 0..60 {
        source.push_str(&format!("delta flag {{true=>yes_{i},false=>no_{i}}}\n"));
    }
    let mask = (0..60)
        .map(|i| format!("yes_{i}"))
        .collect::<Vec<_>>()
        .join(" | ");
    source.push_str(&format!("[{mask}]:emit(\"selected\");[entry]:exit done;}}"));
    let report = f.selection(&source, "selected");
    let result = selection(&report);
    assert!(result.condition_truncated && result.origins_truncated);
    assert_eq!(result.decision_arms.len(), 8);
    assert!(result.condition.contains('…'));
    let mut source = String::new();
    for i in 0..40 {
        source.push_str(&format!("control leaf_{i}() exits done {{exit done;}}\n"));
    }
    source.push_str("control main() exits done {");
    for i in 0..40 {
        source.push_str(&format!("delta leaf_{i}() {{done=>step_{i}}}[step_{i}]:"));
    }
    source.push_str("exit done;}");
    let report = f.query(&source, outcome("main", "done")).unwrap();
    let Details::Outcome { evidence, .. } = report.result else {
        panic!()
    };
    assert!(evidence.truncated);
    assert_eq!(evidence.contributors.len(), 32);
    assert!(report.markdown.contains("**Truncated:"));
}
#[test]
fn queries_read_each_current_module_once_and_map_imported_contributors() {
    let f = Fixture::new();
    let lib = f.root.join("library.tellur");
    fs::write(&lib, "pub control leaf() exits missing {exit missing;}").unwrap();
    let source = "import \"library.tellur\" as lib;control lookup() exits failed {exit delta lib.leaf() {malformed=>failed {}};}";
    f.write(source);
    let mut reads = BTreeMap::new();
    let report = explain::with_loader(
        f.root.join("entry.tellur"),
        f.manifest(),
        outcome("lookup", "failed"),
        |p| {
            *reads.entry(p.to_owned()).or_insert(0) += 1;
            if p.file_name().unwrap() == "library.tellur" {
                Ok("pub control leaf() exits malformed {exit malformed;}".into())
            } else {
                fs::read_to_string(p)
            }
        },
    )
    .unwrap();
    assert_eq!(reads.len(), 2);
    assert!(reads.values().all(|n| *n == 1));
    let Details::Outcome { evidence, .. } = report.result else {
        panic!()
    };
    let imported = evidence
        .contributors
        .iter()
        .find(|c| c.control == "file:library.tellur::leaf")
        .unwrap();
    assert_eq!(imported.location.line, 1);
    assert_eq!(imported.location.module, "file:library.tellur");
    assert_eq!(imported.outcome, "malformed");
    assert!(report.markdown.contains("library.tellur#L1)"));
}
#[test]
fn invalid_sources_positions_and_targets_produce_no_partial_report() {
    let f = Fixture::new();
    let source = "struct record {u32 n;}control main() exits done { // comment\nexit done;}";
    for query in [
        at(source, "comment"),
        at(source, "record"),
        Query::Selection { line: 0, column: 1 },
        Query::Selection {
            line: 90,
            column: 1,
        },
        outcome("missing", "done"),
        outcome("record", "done"),
        outcome("main", "missing"),
        outcome("lib.main", "done"),
    ] {
        assert!(f.query(source, query).is_err());
    }
    assert!(
        f.query(
            &source.replace("exit done;", "exit wrong;"),
            outcome("main", "done")
        )
        .unwrap_err()
        .contains("E_")
    );
    fs::write(
        f.root.join("broken.tellur"),
        "control unused<t>(t value) exits done {emit(unknown);exit done;}",
    )
    .unwrap();
    assert!(
        f.query(
            &("import \"broken.tellur\" as broken;".to_owned() + source),
            outcome("main", "done")
        )
        .is_err()
    );
}
#[test]
fn target_list_includes_private_controls_but_excludes_types_and_generic_roots() {
    let f = Fixture::new();
    f.write("struct record {u32 n;}control identity<t>(t n) exits ready {t n} {exit ready {n};}control private() exits done {exit done;}pub control public() exits done {exit done;}");
    let program = tellur::check_file(f.root.join("entry.tellur"), f.manifest()).unwrap();
    let names: Vec<_> = explain::targets(&program)
        .into_iter()
        .map(|t| (t.control, t.outcome))
        .collect();
    assert_eq!(
        names,
        vec![
            ("private".into(), "done".into()),
            ("public".into(), "done".into())
        ]
    );
}
#[test]
fn cli_outputs_markdown_or_json_and_rejects_malformed_requests() {
    let f = Fixture::new();
    f.write("control main() exits done {exit done;}");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_tellurc"))
            .current_dir(&f.root)
            .args(["explain", "entry.tellur"])
            .args(args)
            .output()
            .unwrap()
    };
    let plain = run(&["--control", "main", "--outcome", "done"]);
    assert!(
        plain.status.success(),
        "{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert!(String::from_utf8_lossy(&plain.stdout).contains("# Tellur outcome contributors"));
    let json = run(&["--at", "1:28", "--json"]);
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["result"]["selection"]["condition"], "entry");
    for args in [
        vec!["--at", "1"],
        vec!["--at", "0:1"],
        vec!["--control", "main"],
        vec!["--control", "main", "--outcome", "done", "--at", "1:28"],
        vec!["--at", "1:28", "--json", "--json"],
    ] {
        let result = run(&args);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn settings_private_validation_and_public_failure_queries_are_useful_without_execution() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("applications/settings/settings.tellur");
    let manifest = root.join("applications/settings/manifest.json");
    let manifest =
        Manifest::from_json_at(&fs::read_to_string(&manifest).unwrap(), &manifest).unwrap();
    let report = explain::file(
        &source,
        manifest.clone(),
        outcome("validate_key", "invalid"),
    )
    .unwrap();
    assert!(report.markdown.contains("`@entry::validate_key`"));
    let report = explain::file(&source, manifest, outcome("set", "storage_failed")).unwrap();
    let Details::Outcome { evidence, .. } = report.result else {
        panic!()
    };
    for outcome in ["read_failed", "encode_failed", "write_failed"] {
        assert!(
            evidence.contributors.iter().any(|c| c.outcome == outcome),
            "missing {outcome}"
        );
    }
    assert!(!evidence.truncated);
}
