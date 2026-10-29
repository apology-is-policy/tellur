use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{DiagnosticWitness, compile, manifest::Manifest};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn scalar_position(source: &str, offset: usize) -> Value {
    let prefix = &source[..offset];
    json!({"line":prefix.bytes().filter(|b| *b == b'\n').count()+1,
        "column":prefix.rsplit('\n').next().unwrap().chars().count()+1})
}
fn selected_range(source: &str, selected: &str) -> Value {
    let start = source.find(selected).unwrap();
    json!({"start":scalar_position(source,start),"end":scalar_position(source,start+selected.len())})
}

#[test]
fn expression_diagnostics_select_arguments_initializers_and_complete_calls() {
    let f = Fixture::new();
    for (body, selected, code) in [
        (
            r#"u32 value = "wrong\n"; exit done;"#,
            r#""wrong\n""#,
            "E_TYPE",
        ),
        (
            "string owner = \"x\"; ref u32 alias = borrow owner; drop(alias); exit done;",
            "borrow owner",
            "E_TYPE",
        ),
        (
            "mutable u32 value = 1; value = true; exit done;",
            "true",
            "E_TYPE",
        ),
        (
            "u32 value = (\"wrong\"); exit done;",
            "(\"wrong\")",
            "E_TYPE",
        ),
        ("forward accept(\n  true\n);", "true", "E_TYPE"),
        ("forward accept(\n);", "accept(\n)", "E_ARITY"),
        (
            "string text = \"x\"; emit(text.missing); exit done;",
            "text.missing",
            "E_FIELD",
        ),
        (
            "vector<u32> values = [1, true]; exit done;",
            "true",
            "E_TYPE",
        ),
        (
            "string text = \"x\"; emit(text); emit(text); exit done;",
            "text); exit",
            "E_MOVED",
        ),
        (
            "delta (1 < 2) && (3 < \"no\") {true=>yes,false=>no} [yes | no]: exit done;",
            "\"no\"",
            "E_TYPE",
        ),
    ] {
        let source = format!(
            "control accept(u32 value) exits done {{ exit done; }}\ncontrol main() exits done {{\n {body}\n}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{body}: {error}");
        let expected = if code == "E_MOVED" {
            let start = source.find(selected).unwrap();
            json!({"start":scalar_position(&source,start),"end":scalar_position(&source,start+4)})
        } else {
            selected_range(&source, selected)
        };
        assert_eq!(error["range"], expected, "{body}: {error}");
        assert_eq!(error["position"], expected["start"]);
    }
}

#[test]
fn expression_ranges_preserve_unicode_crlf_and_multiline_literals() {
    let f = Fixture::new();
    let source = "control main() exits done {\r\n emit(\"😀\"); u32 n = \"line 😀\r\nsecond\";\r\n exit done;\r\n}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "\"line 😀\r\nsecond\"")
    );
}

#[test]
fn imported_generic_expression_ranges_use_original_source_coordinates() {
    let f = Fixture::new();
    let library = "// source coordinates survive generic checking\npub control bad<t: drop>(t value) exits done {\n drop(value);\n u32 n = \"wrong\";\n exit done;\n}";
    f.write("library.tellur", library);
    f.write(
        "main.tellur",
        "import \"library.tellur\" as lib;\ncontrol main() exits done {exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let error = &report["diagnostics"][0];
    assert_eq!(error["code"], "E_TYPE", "{error}");
    assert_eq!(
        error["path"],
        f.root.join("library.tellur").to_string_lossy().as_ref()
    );
    assert_eq!(error["range"], selected_range(library, "\"wrong\""));
}

#[test]
fn parser_diagnostics_select_complete_tokens_and_zero_width_eof() {
    let f = Fixture::new();
    for (source, selected) in [
        (
            "control main() exits done { exit done unexpected; }",
            "unexpected",
        ),
        ("control main() exits done { exit done;", ""),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_PARSE");
        let expected = if selected.is_empty() {
            let end = scalar_position(source, source.len());
            json!({"start":end,"end":end})
        } else {
            selected_range(source, selected)
        };
        assert_eq!(error["range"], expected);
    }
}
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
            assert!(!report["diagnostics"].as_array().unwrap().is_empty());
        }
        report
    }
}

fn check_errors(source: &str) -> tellur::Diagnostics {
    tellur::check_source(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err()
}

#[test]
fn independent_bodies_report_in_source_order_and_skip_failed_call_chains() {
    let source = "control caller() exits done {forward middle();}\ncontrol independent() exits done {string value = 7; exit done;}\ncontrol middle() exits auto {forward broken();}\ncontrol broken() exits auto {u32 value = true; exit ready {value};}\ncontrol typed(outcome<broken> saved) exits done {delta saved {} exit done;}";
    let errors = check_errors(source);
    assert!(!errors.limit_reached);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        vec![("E_TYPE", 2), ("E_TYPE", 4)]
    );
    assert!(errors.errors.iter().all(|e| e.end.is_some()));
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
}

#[test]
fn dependencies_in_loop_and_pattern_types_also_block_recovery() {
    let source = "control looped() exits done {loop work(vector<outcome<broken>> values=[]) {u32 bad=true;exit done;}}\ncontrol patterned(bool flag) exits done {delta flag {true {outcome<broken> pending}=>yes,false=>no}[yes|no]:exit done;}\ncontrol broken() exits auto {u32 value=true;exit ready {value};}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].span.line, 3);
    assert_eq!(errors.errors[0].code, "E_TYPE");
}

#[test]
fn failed_ownership_and_inference_state_does_not_poison_other_bodies() {
    let source = "control good() exits auto {exit ready {7};}\ncontrol broken() exits auto {auto saved = capture good(); string value = 1; exit saved;}\ncontrol valid() exits done {auto saved = capture good(); exit delta saved {ready {u32 number} => done {}};}\ncontrol another() exits done {mutable string owner=\"x\"; auto alias=borrow mutable owner; string value=2; emit(alias); exit done;}\ncontrol also_valid() exits done {mutable string owner=\"x\"; auto alias=borrow mutable owner; deref alias=\"y\"; emit(alias); exit done;}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        vec![("E_TYPE", 2), ("E_TYPE", 4)]
    );
}

#[test]
fn independent_generic_definitions_are_reported_without_internal_type_names() {
    let source = "control first<t>(t input) exits ready {t value} {u32 n=true;exit ready {input};}\ncontrol second<t>(t input) exits ready {t value} {string n=7;exit ready {input};}\ncontrol main() exits done {exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2);
    for (i, error) in errors.errors.iter().enumerate() {
        assert_eq!(error.code, "E_TYPE");
        assert_eq!(error.span.line, i + 1);
        assert!(error.message.starts_with("generic definition check:"));
        assert!(!error.message.contains("_path_poly_"));
    }
}

#[test]
fn generic_witness_batches_use_authored_parameter_names() {
    let source = "control first<t: drop>(t input,bool flag) exits done {drop(input);delta flag {true=>yes,false=>no}[yes]:exit done;}\ncontrol second<u: drop>(u input,bool flag) exits done {drop(input);delta flag {true=>left,false=>right}[right]:exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2);
    for (error, (name, path)) in errors
        .errors
        .iter()
        .zip([("first<t>", "no"), ("second<u>", "left")])
    {
        assert_eq!(
            error.witness.as_deref(),
            Some(&DiagnosticWitness::StructuralMayReach {
                control: name.into(),
                selected_paths: vec![path.into()]
            })
        );
    }
}

#[test]
fn independent_structural_witnesses_keep_their_own_controls_and_paths() {
    let source = "control first(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]:exit done;}\ncontrol second(bool flag) exits done {delta flag {true=>left,false=>right}[right]:exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2);
    for (error, (name, path)) in errors
        .errors
        .iter()
        .zip([("first", "no"), ("second", "left")])
    {
        assert_eq!(error.code, "E_UNCOVERED");
        assert_eq!(
            error.witness.as_deref(),
            Some(&DiagnosticWitness::StructuralMayReach {
                control: name.into(),
                selected_paths: vec![path.into()]
            })
        );
    }
}

#[test]
fn json_and_text_checks_report_imported_errors_and_preserve_ranges() {
    let f = Fixture::new();
    let left = "pub control left() exits done {u32 value=\"😀\";exit done;}";
    let right = "pub control right() exits done {string value=9;exit done;}";
    f.write("left.tellur", left);
    f.write("right.tellur", right);
    f.write("main.tellur","import \"left.tellur\" as a;\nimport \"right.tellur\" as b;\ncontrol main() exits done {forward a.left();}\ncontrol other() exits done {forward b.right();}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics_limited"], false);
    assert!(report["summary"].is_null());
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    for (name, source, selected) in [
        ("left.tellur", left, "\"😀\""),
        ("right.tellur", right, "9"),
    ] {
        let error = errors
            .iter()
            .find(|e| e["path"] == f.root.join(name).to_string_lossy().as_ref())
            .unwrap();
        assert_eq!(error["range"], selected_range(source, selected));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .current_dir(&f.root)
        .args(["check", "main.tellur"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert_eq!(text.matches("E_TYPE").count(), 2);
    assert!(text.contains("left.tellur"));
    assert!(text.contains("right.tellur"));
}

#[test]
fn diagnostic_collection_is_bounded_and_explicit_about_the_limit() {
    let f = Fixture::new();
    let source = (0..40)
        .map(|i| format!("control broken_{i}() exits done {{u32 value=true;exit done;}}\n"))
        .collect::<String>();
    f.write("main.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics_limited"], true);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        tellur::diagnostics::MAX_DIAGNOSTICS
    );
    let errors = check_errors(&source);
    assert!(errors.limit_reached);
    assert!(errors.to_string().contains("diagnostic limit reached"));
    assert_eq!(errors.errors.last().unwrap().span.line, 32);
}

#[test]
fn parser_and_shared_declaration_failures_remain_single_errors() {
    for (source, code) in [
        (
            "control first() exits done {u32 n=true;exit done;}\ncontrol broken() exits done {",
            "E_PARSE",
        ),
        (
            "pub control inferred() exits auto {exit done;}\ncontrol other() exits done {u32 n=true;exit done;}",
            "E_PUBLIC_CONTRACT",
        ),
        (
            "control one() exits done {u32 a=true;string b=2;exit done;}",
            "E_TYPE",
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1);
        assert_eq!(errors.errors[0].code, code);
        assert!(!errors.limit_reached);
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
    let line = "emit(\"😀\"); #";
    f.write(
        "main.tellur",
        &format!("control main() exits done {{\r\n{line}\r\nexit done;}}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let e = &report["diagnostics"][0];
    assert_eq!(e["code"], "E_LEX");
    assert_eq!(
        e["position"],
        json!({"line":2,"column":line.chars().position(|c| c=='#').unwrap()+1})
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
