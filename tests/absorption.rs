use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{
    absorption::{Kind, Snapshot},
    manifest::Manifest,
    modules,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-absorption-{}-{}",
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
    fn write(&self, name: &str, text: &str) {
        fs::write(self.root.join(name), text).unwrap();
    }
    fn snapshot(&self, source: &str) -> Snapshot {
        self.write("entry.tellur", source);
        modules::absorption_snapshot(
            self.root.join("entry.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_or_else(|e| panic!("{e}\n{source}"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
const BEFORE: &str = "control leaf(bool valid) exits auto {exit delta valid {true=>ready {},false=>missing {}};} pub control lookup(bool valid) exits ready, failed {exit delta leaf(valid) {ready=>ready {},missing=>failed {}};}";
const AFTER: &str = "control leaf(bool valid) exits auto {exit delta valid {true=>ready {},false=>malformed {}};} pub control lookup(bool valid) exits ready, failed {exit delta leaf(valid) {ready=>ready {},malformed=>failed {}};}";
fn has(s: &Snapshot, control: &str, exit: &str, source: &str, outcome: &str) -> bool {
    s.controls[control].outcomes[exit]
        .contributors
        .iter()
        .any(|c| c.control == source && c.outcome == outcome)
}
#[test]
fn changed_absorbed_failure_is_visible_under_identical_public_contracts() {
    let f = Fixture::new();
    let before = f.snapshot(BEFORE);
    let after = f.snapshot(AFTER);
    let diff = before.compare(&after).unwrap();
    assert!(diff.public_contracts_equal);
    assert!(!diff.recorded_evidence_equal);
    assert!(!diff.truncated);
    assert!(has(
        &before,
        "@entry::lookup",
        "failed",
        "@entry::leaf",
        "missing"
    ));
    assert!(has(
        &after,
        "@entry::lookup",
        "failed",
        "@entry::leaf",
        "malformed"
    ));
    assert!(!has(
        &after,
        "@entry::lookup",
        "ready",
        "@entry::leaf",
        "malformed"
    ));
    assert_eq!(
        before,
        Snapshot::from_json(&serde_json::to_string(&before).unwrap()).unwrap()
    );
    let changed_contract = f.snapshot(
        &AFTER
            .replace("ready, failed", "ready, unavailable")
            .replace("=>failed {}", "=>unavailable {}"),
    );
    assert!(
        !before
            .compare(&changed_contract)
            .unwrap()
            .public_contracts_equal
    );
}
#[test]
fn added_failure_under_existing_exit_is_reported_even_when_leaf_is_private() {
    let f = Fixture::new();
    let after = BEFORE
        .replace(
            "false=>missing {}",
            "false=>delta valid {true=>missing {},false=>malformed {}}",
        )
        .replace(
            "missing=>failed {}",
            "missing=>failed {},malformed=>failed {}",
        );
    let report = f.snapshot(BEFORE).compare(&f.snapshot(&after)).unwrap();
    assert!(report.public_contracts_equal && !report.recorded_evidence_equal);
}
#[test]
fn captured_adapters_forwarding_and_label_merges_retain_origins() {
    let f = Fixture::new();
    let source = "control leaf(bool b) exits auto {exit delta b {true=>ready {},false=>missing {}};} control adapter(bool b) exits auto {auto saved=capture leaf(b);auto moved=saved;auto mapped=delta moved {ready=>available {},missing=>unavailable {}};exit mapped;} pub control lookup(bool b) exits available, unavailable {forward adapter(b);}";
    let result = f.snapshot(source);
    assert!(has(
        &result,
        "@entry::lookup",
        "unavailable",
        "@entry::leaf",
        "missing"
    ));
    assert!(!has(
        &result,
        "@entry::lookup",
        "unavailable",
        "@entry::leaf",
        "ready"
    ));
    assert!(result.controls["@entry::lookup"].outcomes["unavailable"].contributors.iter().any(|c|c.kind==Kind::Forwarded&&c.operation.as_deref()==Some("@entry::adapter")));
    let merged = source
        .replace(
            "ready=>available {},missing=>unavailable {}",
            "ready=>unavailable {},missing=>unavailable {}",
        )
        .replace("available, unavailable", "unavailable");
    let result = f.snapshot(&merged);
    assert!(has(
        &result,
        "@entry::lookup",
        "unavailable",
        "@entry::leaf",
        "ready"
    ));
    assert!(has(
        &result,
        "@entry::lookup",
        "unavailable",
        "@entry::leaf",
        "missing"
    ));
}
#[test]
fn overwritten_and_path_disjoint_storage_does_not_supply_stale_origins() {
    let f = Fixture::new();
    let source = "variant choice {old,new} pub control lookup(bool b) exits done {mutable choice value=choice.old {};delta b {true=>a,false=>b}[a]:value=choice.new {};[a]:delta value {old=>old,new=>new}[b | old | new]:exit done;}";
    let result = f.snapshot(source);
    assert!(!has(
        &result,
        "@entry::lookup",
        "done",
        "@entry::lookup",
        "old"
    ));
    assert!(has(
        &result,
        "@entry::lookup",
        "done",
        "@entry::lookup",
        "new"
    ));
    let borrowed = source.replace("delta value", "delta borrow value");
    assert!(
        f.snapshot(&borrowed)
            .compare(&result)
            .unwrap()
            .recorded_evidence_equal
    );
}
#[test]
fn repeated_invocations_have_distinct_sites_and_formatting_only_changes_locations() {
    let f = Fixture::new();
    let source = "control leaf() exits ready {exit ready;}pub control lookup() exits done {first = delta leaf() {ready=>one}[one]:second = delta leaf() {ready=>two}[two]:exit done;}";
    let result = f.snapshot(source);
    let calls: Vec<_> = result.controls["@entry::lookup"].outcomes["done"]
        .contributors
        .iter()
        .filter(|c| c.kind == Kind::Received)
        .collect();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].site, calls[1].site);
    assert!(calls.iter().any(|c| c.site.starts_with("first@")));
    assert!(calls.iter().any(|c| c.site.starts_with("second@")));
    let formatted = f.snapshot(&tellur::format::source(source, Default::default()).unwrap());
    assert_ne!(result, formatted);
    assert!(result.compare(&formatted).unwrap().recorded_evidence_equal);
    let private =
        f.snapshot(&(source.to_owned() + "control unrelated() exits other {exit other;}"));
    assert!(result.compare(&private).unwrap().recorded_evidence_equal);
}
#[test]
fn module_relocation_aliases_and_declaration_order_preserve_evidence() {
    let a = Fixture::new();
    let b = Fixture::new();
    for f in [&a, &b] {
        f.write(
            "leaf.tellur",
            "pub control leaf() exits ready {exit ready;}",
        );
    }
    let source =
        "import \"leaf.tellur\" as lib; pub control lookup() exits ready {forward lib.leaf();}";
    let before = a.snapshot(source);
    let after = b.snapshot(&source.replace("lib", "renamed"));
    assert_eq!(before, b.snapshot(source));
    let diff = before.compare(&after).unwrap();
    assert!(diff.public_contracts_equal && diff.recorded_evidence_equal);
    assert!(has(
        &before,
        "@entry::lookup",
        "ready",
        "file:leaf.tellur::leaf",
        "ready"
    ));
    let before = a.snapshot(BEFORE);
    let (leaf, public) = BEFORE.split_once(" pub ").unwrap();
    assert!(
        before
            .compare(&a.snapshot(&format!("pub {public}\n{leaf}")))
            .unwrap()
            .recorded_evidence_equal
    );
}
#[test]
fn generic_roots_are_explicitly_unavailable_and_concrete_callees_are_named() {
    let f = Fixture::new();
    let source = "pub control identity<t>(t value) exits ready {t value} {exit ready {value};} pub control lookup() exits ready {u32 value} {forward identity<u32>(7);}";
    let result = f.snapshot(source);
    assert!(result.controls["@entry::identity"].generic_definition_only);
    assert!(result.controls["@entry::identity"].outcomes.is_empty());
    assert!(
        result.controls["@entry::lookup"].outcomes["ready"]
            .contributors
            .iter()
            .any(|c| c.control.starts_with("@entry::identity<"))
    );
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains("_path_"));
}
#[test]
fn loop_initializers_are_not_claimed_as_sources_for_later_iterations() {
    let f = Fixture::new();
    let source = "variant choice {old,new} pub control lookup() exits done {loop retry(choice current=choice.old {}) {delta current {old=>old,new=>new}[old]:again retry(choice.new {});[new]:exit done;}}";
    let result = f.snapshot(source);
    assert!(!has(
        &result,
        "@entry::lookup",
        "done",
        "@entry::lookup",
        "old"
    ));
    assert!(
        result
            .analysis
            .limitations
            .contains(&"loop_carried_values".into())
    );
}
#[test]
fn trait_method_names_are_qualified_without_capturing_unrelated_control_names() {
    let f = Fixture::new();
    let source = "pub struct value {pub u32 number;} pub trait worker {control work(self input) exits done;} implement worker for value {control work(self input) exits done {exit done;}} pub control lookup(value input) exits done {forward worker<value>.work(input);}";
    let before = f.snapshot(source);
    let after = f.snapshot(&(source.to_owned() + "control work() exits unused {exit unused;}"));
    assert!(before.compare(&after).unwrap().recorded_evidence_equal);
    assert!(
        before.controls["@entry::lookup"].outcomes["done"]
            .contributors
            .iter()
            .any(|c| c.control == "@entry::worker<@entry::value>.work")
    );
}
#[test]
fn truncation_is_explicit_and_cannot_pass_a_check_even_when_recorded_evidence_matches() {
    let f = Fixture::new();
    let mut source = "control f0() exits ready {exit ready;}".to_owned();
    for i in 1..45 {
        source.push_str(&format!(
            "control f{i}() exits ready {{forward f{}();}}",
            i - 1
        ));
    }
    source.push_str("pub control lookup() exits ready {forward f44();}");
    let result = f.snapshot(&source);
    let evidence = &result.controls["@entry::lookup"].outcomes["ready"];
    assert!(evidence.truncated);
    assert_eq!(evidence.contributors.len(), 32);
    assert!(result.compare(&result).unwrap().truncated);
    let json = serde_json::to_string(&result).unwrap();
    f.write("limited.json", &json);
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .arg("absorption-diff")
        .arg(f.root.join("limited.json"))
        .arg(f.root.join("limited.json"))
        .arg("--check")
        .output()
        .unwrap();
    assert!(!output.status.success());
}
#[test]
fn invalid_sources_and_forged_or_unsupported_snapshots_are_refused() {
    let f = Fixture::new();
    let snapshot = f.snapshot(BEFORE);
    let text = serde_json::to_string(&snapshot).unwrap();
    for changed in [
        text.replacen("\"version\":1", "\"version\":99", 1),
        text.replacen("\"format\":", "\"unknown\":true,\"format\":", 1),
        text.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
        text.replace("\"per_outcome_limit\":32", "\"per_outcome_limit\":33"),
    ] {
        assert!(Snapshot::from_json(&changed).is_err());
    }
    f.write(
        "entry.tellur",
        "pub control broken() exits auto {exit done;}",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .arg("absorption-snapshot")
        .arg(f.root.join("entry.tellur"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
#[test]
fn cli_exports_and_separately_reports_contract_and_contributor_changes() {
    let f = Fixture::new();
    f.snapshot(BEFORE);
    let compiler = env!("CARGO_BIN_EXE_tellurc");
    let output = Command::new(compiler)
        .arg("absorption-snapshot")
        .arg(f.root.join("entry.tellur"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    Snapshot::from_json(&text).unwrap();
    f.write("before.json", &text);
    f.write(
        "after.json",
        &serde_json::to_string(&f.snapshot(AFTER)).unwrap(),
    );
    let diff = |file: &str, check: bool| {
        let mut c = Command::new(compiler);
        c.arg("absorption-diff")
            .arg(f.root.join("before.json"))
            .arg(f.root.join(file));
        if check {
            c.arg("--check");
        }
        c.output().unwrap()
    };
    assert!(diff("before.json", true).status.success());
    assert!(diff("after.json", false).status.success());
    let changed = diff("after.json", true);
    assert!(!changed.status.success());
    let value: serde_json::Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(value["public_contracts_equal"], true);
    assert_eq!(value["recorded_evidence_equal"], false);
}
#[test]
fn settings_new_internal_failure_is_visible_without_changing_its_public_baseline() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("applications/settings/settings.tellur");
    let manifest_path = path.with_file_name("manifest.json");
    let manifest =
        Manifest::from_json_at(&fs::read_to_string(&manifest_path).unwrap(), &manifest_path)
            .unwrap();
    let source = fs::read_to_string(&path).unwrap();
    let before = modules::absorption_snapshot(&path, manifest.clone()).unwrap();
    let baseline = Snapshot::from_json(include_str!(
        "../applications/settings/settings.absorption.json"
    ))
    .unwrap();
    let baseline_diff = baseline.compare(&before).unwrap();
    assert!(
        baseline_diff.public_contracts_equal
            && baseline_diff.recorded_evidence_equal
            && !baseline_diff.truncated
    );
    let changed = source
        .replace("exits valid, invalid {", "exits valid, invalid, blank {")
        .replace(
            "false => invalid {}",
            "false => delta (normalized == \"\") {true=>blank {},false=>invalid {}}",
        )
        .replace(
            "invalid => exit invalid_key",
            "invalid => exit invalid_key, blank => exit invalid_key",
        );
    let after = modules::absorption_snapshot_with_loader(&path, manifest, |p| {
        if p == path {
            Ok(changed.clone())
        } else {
            fs::read_to_string(p)
        }
    })
    .unwrap();
    let diff = before.compare(&after).unwrap();
    assert!(diff.public_contracts_equal && !diff.recorded_evidence_equal);
    assert!(has(
        &after,
        "@entry::set",
        "invalid_key",
        "@entry::validate_key",
        "blank"
    ));
}
#[test]
fn all_maintained_sources_export_and_roundtrip_bounded_evidence() {
    fn walk(path: &Path, count: &mut usize) {
        for item in fs::read_dir(path).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                if !["build", "target", ".tellur-build"]
                    .contains(&path.file_name().unwrap().to_str().unwrap())
                {
                    walk(&path, count);
                }
            } else if path.extension().is_some_and(|e| e == "tellur") {
                let m = path
                    .parent()
                    .unwrap()
                    .ancestors()
                    .map(|p| p.join("manifest.json"))
                    .find(|p| p.is_file())
                    .unwrap();
                let snapshot = modules::absorption_snapshot(
                    &path,
                    Manifest::from_json_at(&fs::read_to_string(&m).unwrap(), &m).unwrap(),
                )
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(
                    snapshot,
                    Snapshot::from_json(&serde_json::to_string(&snapshot).unwrap()).unwrap()
                );
                *count += 1;
            }
        }
    }
    let mut count = 0;
    for folder in ["playground", "examples", "applications"] {
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join(folder),
            &mut count,
        );
    }
    assert!(count >= 61);
    println!("Exported and roundtripped absorption snapshots for {count} maintained sources.");
}
