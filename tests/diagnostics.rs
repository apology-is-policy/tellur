use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{DiagnosticWitness, compile, manifest::Manifest};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-diagnostics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let f = Self {
            root: fs::canonicalize(root).unwrap(),
        };
        f.write("manifest.json", include_str!("../playground/manifest.json"));
        f
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn report(&self, args: &[&str], ok: bool) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if ok { 0 } else { 1 }),
            "{output:?}"
        );
        assert!(output.stderr.is_empty(), "{output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["schema"], "tellur.check.v1");
        assert_eq!(report["ok"], ok);
        assert_eq!(report["position_encoding"], "one_based_unicode_scalars");
        if !ok {
            assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
        }
        report
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn typed_witnesses_preserve_structural_selected_paths_and_entry() {
    let source =
        "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]:exit done;}";
    let m = || Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let error = compile(source, m()).unwrap_err();
    assert_eq!(error.code, "E_UNCOVERED");
    assert_eq!(
        error.witness.as_deref(),
        Some(&DiagnosticWitness::StructuralMayReach {
            control: "main".into(),
            selected_paths: vec!["no".into()]
        })
    );
    assert!(error.message.contains("not a proven feasible input"));
    let error = compile("control main() exits done {}", m()).unwrap_err();
    assert_eq!(
        error.witness.as_deref(),
        Some(&DiagnosticWitness::StructuralMayReach {
            control: "main".into(),
            selected_paths: vec![]
        })
    );
    assert!(error.message.ends_with("Entry"));
}
#[test]
fn successful_json_checks_report_counts_and_no_diagnostics() {
    let f = Fixture::new();
    f.write("main.tellur","control main() exits done {exit done;} control identity<t>(t input) exits ready {t value} {exit ready {input};}");
    let report = f.report(&["check", "main.tellur", "--json"], true);
    assert_eq!(
        report["summary"],
        json!({"controls":1,"generic_definitions":1})
    );
    assert_eq!(report["diagnostics"], json!([]));
    assert_eq!(
        report,
        f.report(&["check", "--json", "main.tellur", "manifest.json"], true)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_pathc"))
        .current_dir(&f.root)
        .args(["check", "main.tellur", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        report
    );
}
#[test]
fn json_positions_use_unicode_scalars_and_preserve_crlf_lines() {
    let f = Fixture::new();
    let line = "emit(\"😀\"); @";
    f.write(
        "main.tellur",
        &format!("control main() exits done {{\r\n{line}\r\nexit done;}}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let e = &report["diagnostics"][0];
    assert_eq!(e["code"], "E_LEX");
    assert_eq!(
        e["position"],
        json!({"line":2,"column":line.chars().position(|c| c=='@').unwrap()+1})
    );
    assert_eq!(e["path"], f.root.join("main.tellur").to_str().unwrap());
    assert_eq!(e["witness"], Value::Null);
}
#[test]
fn reports_keep_typed_witnesses_and_original_imported_control_names() {
    let f = Fixture::new();
    f.write("library.tellur","pub control broken(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]:exit done;}");
    f.write(
        "main.tellur",
        "import \"library.tellur\" as lib;control main() exits done {exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let e = &report["diagnostics"][0];
    assert_eq!(e["code"], "E_UNCOVERED");
    assert_eq!(e["path"], f.root.join("library.tellur").to_str().unwrap());
    assert_eq!(e["witness"]["kind"], "structural_may_reach");
    assert_eq!(e["witness"]["selected_paths"], json!(["no"]));
    assert!(
        e["witness"]["control"]
            .as_str()
            .unwrap()
            .ends_with("library.tellur::broken")
    );
    assert!(!report.to_string().contains("_path_m_"));
}
#[test]
fn json_notes_preserve_stored_adapter_locations_across_files() {
    let f = Fixture::new();
    f.write(
        "library.tellur",
        "pub control source() exits missing {exit missing;}",
    );
    f.write("main.tellur","import \"library.tellur\" as lib;\ncontrol main() exits done {\nauto saved=capture lib.source();\nauto adapted=delta saved {missing=>unavailable {}};\ndelta adapted {}exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let e = &report["diagnostics"][0];
    assert_eq!(e["code"], "E_ARMS");
    let notes = e["notes"].as_array().unwrap();
    assert!(notes.iter().any(
        |n| n["path"] == f.root.join("library.tellur").to_str().unwrap()
            && n["position"]["line"] == 1
    ));
    assert!(notes.iter().any(
        |n| n["path"] == f.root.join("main.tellur").to_str().unwrap() && n["position"]["line"] == 4
    ));
    assert_eq!(e["witness"], Value::Null);
}
#[test]
fn manifest_io_and_usage_failures_also_produce_one_json_envelope() {
    let f = Fixture::new();
    f.write("main.tellur", "control main() exits done {exit done;}");
    for args in [
        vec!["check", "--json"],
        vec!["check", "main.tellur", "--json", "--json"],
        vec!["check", "main.tellur", "--json", "--unknown"],
    ] {
        let e = f.report(&args, false);
        assert_eq!(e["diagnostics"][0]["code"], "E_USAGE");
        assert!(e["diagnostics"][0]["position"].is_null());
    }
    let e = f.report(&["check", "main.tellur", "absent.json", "--json"], false);
    assert_eq!(e["diagnostics"][0]["code"], "E_IO");
    assert!(e["diagnostics"][0]["position"].is_null());
    f.write("manifest.json", "{");
    let e = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(e["diagnostics"][0]["code"], "E_MANIFEST");
    assert_eq!(
        e["diagnostics"][0]["path"],
        f.root.join("manifest.json").to_str().unwrap()
    );
    fs::remove_file(f.root.join("manifest.json")).unwrap();
    assert_eq!(
        f.report(&["check", "main.tellur", "--json"], false)["diagnostics"][0]["code"],
        "E_CONFIG"
    );
}
#[test]
fn manifest_discovery_works_for_bare_relative_sources_in_nested_directories() {
    let f = Fixture::new();
    f.write(
        "nested/main.tellur",
        "control main() exits done {exit done;}",
    );
    for args in [
        vec!["check", "main.tellur"],
        vec!["check", "main.tellur", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
            .current_dir(f.root.join("nested"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

#[cfg(unix)]
#[test]
fn json_and_text_checks_discover_the_same_manifest_through_a_source_symlink() {
    let f = Fixture::new();
    f.write("real/main.tellur", "control main() exits done {exit done;}");
    f.write("real/manifest.json", "{");
    std::os::unix::fs::symlink(f.root.join("real/main.tellur"), f.root.join("alias.tellur"))
        .unwrap();
    f.report(&["check", "alias.tellur", "--json"], true);
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .current_dir(&f.root)
        .args(["check", "alias.tellur"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
