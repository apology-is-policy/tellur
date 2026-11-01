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
fn named_completion_binding_errors_batch_before_signatures_and_bodies() {
    let source = "outcome one['a] {ready {ref string value <- 'a}}\noutcome pair['a,'b] {ready {ref string first <- 'a,ref string second <- 'b}}\ncontrol first(ref string input) exits one[input,input] {exit ready {input};}\ncontrol second(ref string left,ref string right) exits pair[left] {exit ready {left,right};}\ncontrol signature(u32 x,u32 x) exits done {exit done;}\ncontrol body() exits done {u32 bad=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        vec![("E_ORIGIN", 3), ("E_ORIGIN", 4)],
        "{errors}"
    );
    assert!(
        errors.errors[0]
            .message
            .contains("requires 1 origin arguments")
    );
    assert!(
        errors.errors[1]
            .message
            .contains("requires 2 origin arguments")
    );
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    let partial = source.replace("one[input,input]", "one[input]");
    let errors = check_errors(&partial);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].span.line, 4);
    let repaired = partial.replace("pair[left]", "pair[left,right]");
    let errors = check_errors(&repaired);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(
        (errors.errors[0].code, errors.errors[0].span.line),
        ("E_NAME", 5)
    );
    let errors = check_errors(&repaired.replace("u32 x,u32 x", "u32 x,u32 other"));
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(
        (errors.errors[0].code, errors.errors[0].span.line),
        ("E_TYPE", 6)
    );
}

#[test]
fn binding_batches_reject_origins_on_nullary_contracts_and_bare_exits() {
    let source = "outcome completion {done}\ncontrol named(ref string input) exits completion[input] {exit done;}\ncontrol bare(ref string input) exits done[input] {exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_ORIGIN"));
    assert!(
        errors.errors[0]
            .message
            .contains("requires 0 origin arguments")
    );
    assert!(
        errors.errors[1]
            .message
            .contains("source-declared named outcome")
    );
    compile(
        &source.replace("[input]", ""),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn invalid_shared_origin_schema_still_stops_before_independent_bindings() {
    let source = "outcome bad['a] {found {ref string value <- 'missing}}\ncontrol first(ref string input) exits bad[input,input] {exit found {input};}\ncontrol second(ref string input) exits bad[input,input] {exit found {input};}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_ORIGIN");
    assert_eq!(errors.errors[0].span.line, 1);
    assert!(!errors.limit_reached);
}

#[test]
fn named_completion_binding_budget_is_shared_by_cli_and_library() {
    let source="outcome view['a] {found {ref string value <- 'a}}\n".to_owned()+&(0..40).map(|i| format!("control bad_{i}(ref string input) exits view[input,input] {{exit found {{input}};}}\n")).collect::<String>();
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32);
    assert!(errors.limit_reached);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        (2..=33).collect::<Vec<_>>()
    );
    let f = Fixture::new();
    f.write("main.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 32);
    assert_eq!(report["diagnostics_limited"], true);
    assert!(report["summary"].is_null());
}

#[test]
fn imported_completion_binding_errors_keep_original_names_and_file_points() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"views🙂.tellur\" as views;control main() exits done {exit done;}",
    );
    f.write("views🙂.tellur","pub outcome view['a] {found {ref string value <- 'a}}\npub control first(ref string input) exits view[input,input] {exit found {input};}\npub control second(ref string input) exits view[input,input] {exit found {input};}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (i, error) in errors.iter().enumerate() {
        assert_eq!(error["code"], "E_ORIGIN");
        assert_eq!(error["position"]["line"], i + 2);
        assert!(error["path"].as_str().unwrap().ends_with("views🙂.tellur"));
        assert!(error["message"].as_str().unwrap().contains("view"));
    }
    assert!(!report.to_string().contains("_path_m_"));
    let first = tellur::compile_file(
        f.root.join("main.tellur"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first.diagnostic.code, "E_ORIGIN");
    assert_eq!(first.diagnostic.span.line, 2);
}

#[test]
fn unused_generic_completion_bindings_batch_without_specialization_leaks() {
    let source = "outcome view<t>['a] {found {ref t value <- 'a}}\ncontrol first<t>(ref t input) exits view<t>[input,input] {exit found {input};}\ncontrol second<t>(ref t input) exits view<t>[input,input] {exit found {input};}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_ORIGIN"));
    assert!(!errors.to_string().contains("_path_poly_"), "{errors}");
    compile(
        &source.replace("[input,input]", "[input]"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn public_shape_errors_batch_before_dependent_preparation_and_bodies() {
    let source = "pub control first() exits auto {exit done;}\npub control second() exits ready... {exit ready {1};}\ncontrol body() exits done {u32 bad=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        vec![("E_PUBLIC_CONTRACT", 1), ("E_PUBLIC_CONTRACT", 2)]
    );
    assert!(!errors.limit_reached);
    let partial = source.replace("exits auto", "exits done");
    let errors = check_errors(&partial);
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].span.line, 2);
    let repaired = partial.replace("ready...", "ready {u32 value}");
    let errors = check_errors(&repaired);
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(
        (errors.errors[0].code, errors.errors[0].span.line),
        ("E_TYPE", 3)
    );
}

#[test]
fn signature_errors_batch_one_per_control_without_partial_body_checking() {
    let source = "control first(u32 input,u32 input) exits done {exit done;}\npub control second(ref string input) exits found {ref string a,ref string b} {exit found {input,input};}\ncontrol third(ref string input) exits found {ref string value from missing} {exit found {input};}\ncontrol body() exits done {u32 bad=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        vec![("E_NAME", 1), ("E_PUBLIC_CONTRACT", 2), ("E_LIFETIME", 3)],
        "{errors}"
    );
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    let repaired = source
        .replace("u32 input,u32 input", "u32 input,u32 other")
        .replace(
            "ref string a,ref string b",
            "ref string a from input,ref string b from input",
        )
        .replace("from missing", "from input");
    let errors = check_errors(&repaired);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(
        (errors.errors[0].code, errors.errors[0].span.line),
        ("E_TYPE", 4)
    );
}

#[test]
fn each_declaration_phase_respects_the_diagnostic_budget() {
    for public in [true, false] {
        let source = (0..40)
            .map(|i| {
                if public {
                    format!("pub control bad_{i}() exits auto {{exit done;}}\n")
                } else {
                    format!("control bad_{i}(u32 input,u32 input) exits done {{exit done;}}\n")
                }
            })
            .collect::<String>();
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 32, "{errors}");
        assert!(errors.limit_reached);
        assert_eq!(
            errors
                .errors
                .iter()
                .map(|e| e.span.line)
                .collect::<Vec<_>>(),
            (1..=32).collect::<Vec<_>>()
        );
    }
}

#[test]
fn imported_declaration_batches_keep_source_names_locations_and_first_error_api() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"bad.tellur\" as bad;control main() exits done {exit done;}",
    );
    f.write("bad.tellur", "// 🙂\npub control first(ref string input) exits found {ref string value} {exit found {input};}\npub control second(u32 input,u32 input) exits done {exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    assert_eq!(errors[0]["code"], "E_PUBLIC_CONTRACT");
    assert_eq!(errors[1]["code"], "E_NAME");
    assert_eq!(errors[0]["position"]["line"], 2);
    assert_eq!(errors[1]["position"]["line"], 3);
    assert!(
        errors
            .iter()
            .all(|e| e["path"].as_str().unwrap().ends_with("bad.tellur"))
    );
    assert!(!report.to_string().contains("_path_m_"));
    assert!(report["summary"].is_null());
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let first = tellur::compile_file(f.root.join("main.tellur"), manifest).unwrap_err();
    assert_eq!(first.diagnostic.code, "E_PUBLIC_CONTRACT");
    assert_eq!(first.diagnostic.span.line, 2);
}

#[test]
fn generic_public_shapes_batch_but_shared_layout_failures_still_stop_the_phase() {
    let errors = check_errors(
        "pub control one<t>(t input) exits auto {exit ready {input};}\npub control two<t>(t input) exits ready... {exit ready {input};}",
    );
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_PUBLIC_CONTRACT"));
    let errors = check_errors(
        "struct broken {missing field;}\ncontrol first(u32 x,u32 x) exits done {exit done;}\ncontrol second(u32 x,u32 x) exits done {exit done;}",
    );
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_MANIFEST");
    assert!(!errors.limit_reached);
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
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1);
        assert_eq!(errors.errors[0].code, code);
        assert!(!errors.limit_reached);
    }
}

#[test]
fn imported_statement_batches_keep_ranges_and_share_the_cli_budget() {
    let f = Fixture::new();
    f.write("library.tellur", "pub control broken() exits done {\n    u32 first=true;\n    string second=7;\n    exit done;\n}");
    f.write("main.tellur", "import \"library.tellur\" as lib;\ncontrol main() exits done {forward lib.broken();}\ncontrol independent() exits done {u32 wrong=false;exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 3, "{report}");
    let imported = errors
        .iter()
        .filter(|e| e["path"].as_str().unwrap().ends_with("library.tellur"))
        .collect::<Vec<_>>();
    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0]["range"]["start"], json!({"line":2,"column":15}));
    assert_eq!(imported[1]["range"]["start"], json!({"line":3,"column":19}));
    assert!(report["summary"].is_null());
    let body = (0..40)
        .map(|i| format!("u32 bad_{i}=true;\n"))
        .collect::<String>();
    f.write(
        "library.tellur",
        &format!("pub control broken() exits done {{\n{body}exit done;}}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 32);
    assert_eq!(report["diagnostics_limited"], true);
}

#[test]
fn imported_nested_branches_keep_their_authored_source_ranges() {
    let f = Fixture::new();
    f.write("library.tellur", "pub control broken(bool flag) exits done {\n delta flag {true=>yes,false=>no}\n [yes]: {u32 first=true;exit done;}\n [no] over &: {string second=7;exit done;}\n}");
    f.write(
        "main.tellur",
        "import \"library.tellur\" as lib;\ncontrol main() exits done {forward lib.broken(true);}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    assert_eq!(errors[0]["range"]["start"], json!({"line":3,"column":20}));
    assert_eq!(errors[1]["range"]["start"], json!({"line":4,"column":30}));
    for error in errors {
        assert!(error["path"].as_str().unwrap().ends_with("library.tellur"));
        assert_eq!(error["code"], "E_TYPE");
    }
    assert!(report["summary"].is_null());
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
    f.write("main.tellur","import \"library.tellur\" as lib;\ncontrol main() exits done {\nauto saved=capture lib.source();\nauto adapted=delta saved {missing=>unavailable {}};\nexit delta adapted {};}");
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

#[test]
fn constructor_call_preparation_batches_independent_bodies_and_stops_later_phases() {
    let f = Fixture::new();
    f.write("types.tellur", "pub struct box<t> {t value;constructor(t input) {self.value=move input;}}pub control create<t>(t input) exits ready {box<t> value} {exit ready {box<t>(move input)};}");
    let source = "// 🙂 constructor privacy\r\nimport \"types.tellur\" as types;\r\ncontrol first() exits done {auto one=types.box<string>(\"one\");auto two=types.box<string>(\"two\");drop(one);drop(two);exit done;}\r\ncontrol generic<t>(t input) exits ready {types.box<t> value} {exit ready {types.box<t>(move input)};}\r\nstruct wrapper {types.box<string> value;constructor() {self.value=types.box<string>(\"wrapped\");}}\r\ncontrol caller() exits done {first();exit done;}\r\ncontrol signature(u32 same,u32 same) exits done {exit done;}\r\ncontrol body() exits done {u32 invalid=true;exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 3, "{report}");
    assert_eq!(report["diagnostics_limited"], false);
    assert!(report["summary"].is_null());
    for selected in [
        "types.box<string>(\"one\")",
        "types.box<t>(move input)",
        "types.box<string>(\"wrapped\")",
    ] {
        let position = scalar_position(source, source.find(selected).unwrap());
        assert!(
            errors.iter().any(|e| e["position"] == position),
            "{selected}: {report}"
        );
    }
    assert!(
        errors.iter().all(|e| e["code"] == "E_PRIVATE"
            && e["path"] == f.root.join("main.tellur").to_str().unwrap()),
        "{report}"
    );
    assert!(
        !report.to_string().contains("_path_") && !report.to_string().contains("$constructor"),
        "{report}"
    );
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let first = tellur::compile_file(f.root.join("main.tellur"), manifest).unwrap_err();
    assert_eq!(first.diagnostic.code, "E_PRIVATE");
    assert_eq!(first.diagnostic.span.line, 3);
    // Repairing an earlier private call exposes the next one in that body, not
    // the unrelated signature/body phase. Unused generic bodies are checked too.
    let repaired = source.replace(
        "auto one=types.box<string>(\"one\");",
        "string one=\"one\";",
    );
    f.write("main.tellur", &repaired);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 3);
    assert_eq!(
        report["diagnostics"][0]["position"],
        scalar_position(
            &repaired,
            repaired.find("types.box<string>(\"two\")").unwrap()
        )
    );
    // Once the privacy phase is valid, the next declaration phase becomes visible.
    f.write("types.tellur", "pub struct box<t> {t value;pub constructor(t input) {self.value=move input;}}pub control create<t>(t input) exits ready {box<t> value} {exit ready {box<t>(move input)};}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        1,
        "{report}"
    );
    assert_eq!(report["diagnostics"][0]["code"], "E_NAME");
    assert_eq!(report["diagnostics"][0]["position"]["line"], 7);
}

#[test]
fn constructor_call_preparation_keeps_transitive_module_locations() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub struct message {string value;constructor(string input) {self.value=move input;}}",
    );
    for (file, alias) in [("one.tellur", "data"), ("two.tellur", "types")] {
        f.write(file, &format!("// nested 🙂\nimport \"types.tellur\" as {alias};\npub control make() exits ready {{{alias}.message value}} {{exit ready {{{alias}.message(\"hello\")}};}}"));
    }
    f.write("main.tellur", "import \"one.tellur\" as one;import \"two.tellur\" as two;control main() exits done {exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for file in ["one.tellur", "two.tellur"] {
        assert!(
            errors
                .iter()
                .any(|e| e["path"] == f.root.join(file).to_str().unwrap()
                    && e["position"]["line"] == 3
                    && e["code"] == "E_PRIVATE"),
            "{report}"
        );
    }
    assert!(!report.to_string().contains("_path_"), "{report}");
}

#[test]
fn constructor_call_preparation_preserves_earlier_phase_barriers() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub struct message {string value;constructor(string input) {self.value=move input;}}",
    );
    let base = "import \"types.tellur\" as types;control first() exits done {auto a=types.message(\"a\");drop(a);exit done;}control second() exits done {auto b=types.message(\"b\");drop(b);exit done;}";
    for (extra, code) in [
        (
            "pub control invalid() exits auto {exit done;}",
            "E_PUBLIC_CONTRACT",
        ),
        (
            "struct invalid {string value;constructor() {exit done;}}",
            "E_CONSTRUCTOR",
        ),
        (
            "trait invalid {control run(self input) exits done;}implement invalid for string {}",
            "E_TRAIT",
        ),
    ] {
        f.write("main.tellur", &format!("{base}{extra}"));
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        assert_eq!(report["diagnostics"][0]["code"], code, "{report}");
    }
}

#[test]
fn constructor_call_preparation_uses_the_shared_diagnostic_budget() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub struct message {string value;constructor(string input) {self.value=move input;}}",
    );
    let source = "import \"types.tellur\" as types;\n".to_owned() + &(0..40).map(|n| format!("control bad_{n}() exits done {{auto value=types.message(\"hello\");drop(value);exit done;}}\n")).collect::<String>();
    f.write("main.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 32, "{report}");
    assert_eq!(report["diagnostics_limited"], true);
    assert!(report["summary"].is_null());
    for (n, error) in errors.iter().enumerate() {
        assert_eq!(error["code"], "E_PRIVATE");
        assert_eq!(error["position"]["line"], n + 2);
    }
}

#[test]
fn constructor_call_preparation_covers_methods_and_isolation_bodies() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub struct message {string value;constructor(string input) {self.value=move input;}}",
    );
    let source = "import \"types.tellur\" as types;\ntrait builder {control make(self input) exits ready {types.message value};}\nimplement builder for string {control make(self input) exits ready {types.message value} {exit ready {types.message(move input)};}}\nboundary request() {auto a=types.message(\"work\");drop(a);terminate;} contingency(string reason) {auto b=types.message(reason);drop(b);terminate;}\ncontrol lexical() exits done {fault boundary {auto c=types.message(\"inner\");drop(c);} contingency [] {terminate process 1;}exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 4, "{report}");
    for selected in [
        "types.message(move input)",
        "types.message(\"work\")",
        "types.message(reason)",
        "types.message(\"inner\")",
    ] {
        assert!(
            errors.iter().any(|e| e["code"] == "E_PRIVATE"
                && e["position"] == scalar_position(source, source.find(selected).unwrap())),
            "{selected}: {report}"
        );
    }
}

#[test]
fn generic_bound_preparation_batches_unknown_and_private_requirements() {
    let source = "trait hidden {}\ncontrol first<t:missing>(t input) exits ready {t value} {exit ready {input};}\ncontrol second<t:other>(t input) exits ready {t value} {exit ready {input};}\npub control third<t:hidden>(t input) exits ready {t value} {exit ready {input};}\ncontrol later() exits done {u32 bad=true;exit done;}";
    let batch = check_errors(source);
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["E_BOUND", "E_BOUND", "E_VISIBILITY"],
        "{batch}"
    );
    assert_eq!(
        batch.errors.iter().map(|e| e.span.line).collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert!(!batch.limit_reached);
    let m = || Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    assert_eq!(tellur::compile(source, m()).unwrap_err(), batch.errors[0]);
    let repaired = source
        .replace("t:missing", "t:copy")
        .replace("t:other", "t:drop");
    let batch = check_errors(&repaired);
    assert_eq!(batch.errors.len(), 1);
    assert_eq!(batch.errors[0].code, "E_VISIBILITY");
    let repaired = repaired.replace("trait hidden", "pub trait hidden");
    let batch = check_errors(&repaired);
    assert_eq!(batch.errors.len(), 1);
    assert_eq!(batch.errors[0].code, "E_TYPE");
    assert_eq!(batch.errors[0].span.line, 5);
}

#[test]
fn generic_bound_preparation_detects_alias_duplicates_without_partial_witnesses() {
    let f = Fixture::new();
    f.write(
        "traits.tellur",
        "pub trait marker {}implement marker for string {}",
    );
    let source = "// 🙂 aliases resolve to one trait\r\nimport \"traits.tellur\" as first;import \"traits.tellur\" as second;\r\npub control one<t:first.marker+second.marker>(t input) exits ready {t value} {exit ready {input};}\r\npub control two<t:second.marker+first.marker>(t input) exits ready {t value} {exit ready {input};}\r\ncontrol main() exits done {delta one<string>(\"hello\") {ready {auto value}=>ready} [ready]:drop(value);exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        2,
        "{report}"
    );
    for (index, e) in report["diagnostics"].as_array().unwrap().iter().enumerate() {
        assert_eq!(e["code"], "E_BOUND");
        assert_eq!(e["position"]["line"], index + 3);
        assert_eq!(e["path"], f.root.join("main.tellur").to_str().unwrap());
        assert!(
            e["message"]
                .as_str()
                .unwrap()
                .contains("duplicate requirement")
        );
    }
    assert!(report["summary"].is_null());
    assert!(!report.to_string().contains("_path_"));
    let partial = source.replace("t:first.marker+second.marker", "t:first.marker");
    f.write("main.tellur", &partial);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["position"]["line"], 4);
    f.write(
        "main.tellur",
        &partial.replace("t:second.marker+first.marker", "t:second.marker"),
    );
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn generic_bound_preparation_preserves_module_locations_and_first_error_api() {
    let f = Fixture::new();
    for (name, tag) in [("one.tellur", "private_one"), ("two.tellur", "private_two")] {
        f.write(name, &format!("trait {tag} {{}}\npub control keep<t:{tag}>(t input) exits ready {{t value}} {{exit ready {{input}};}}"));
    }
    f.write("main.tellur", "import \"one.tellur\" as one;import \"two.tellur\" as two;control main() exits done {exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for file in ["one.tellur", "two.tellur"] {
        assert!(
            errors
                .iter()
                .any(|e| e["path"] == f.root.join(file).to_str().unwrap()
                    && e["code"] == "E_VISIBILITY"
                    && e["position"]["line"] == 2),
            "{report}"
        );
    }
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let first = tellur::compile_file(f.root.join("main.tellur"), manifest).unwrap_err();
    assert_eq!(first.diagnostic.code, "E_VISIBILITY");
    assert_eq!(first.diagnostic.span.line, 2);
    assert_eq!(first.path, f.root.join("one.tellur"));
}

#[test]
fn generic_bound_preparation_has_strict_phase_barriers_and_one_error_per_control() {
    let base = "control first<a:missing,b:other>(a left,b right) exits ready {a first,b second} {exit ready {left,right};}\ncontrol second<t:absent>(t input) exits ready {t value} {exit ready {input};}";
    let batch = check_errors(base);
    assert_eq!(batch.errors.len(), 2, "{batch}");
    let earlier = format!("{base}\npub control invalid() exits auto {{exit done;}}");
    let batch = check_errors(&earlier);
    assert_eq!(batch.errors.len(), 1);
    assert_eq!(batch.errors[0].code, "E_PUBLIC_CONTRACT");
    let later = format!(
        "{base}\ntrait invalid {{control run(self input) exits done;}}implement invalid for string {{}}"
    );
    let batch = check_errors(&later);
    assert_eq!(batch.errors.len(), 2, "{batch}");
    assert!(batch.errors.iter().all(|e| e.code == "E_BOUND"));
    let repaired = later
        .replace("a:missing", "a:drop")
        .replace("b:other", "b:drop")
        .replace("t:absent", "t:drop");
    let batch = check_errors(&repaired);
    assert_eq!(batch.errors.len(), 1);
    assert_eq!(batch.errors[0].code, "E_TRAIT");
    // A module lookup failure is an earlier barrier, not a fabricated trait.
    let f = Fixture::new();
    f.write(
        "main.tellur",
        &format!("import \"absent.tellur\" as missing;{base}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_IMPORT");
}

#[test]
fn generic_bound_preparation_uses_the_shared_budget() {
    for unknown in [true, false] {
        let source="trait hidden {}\n".to_owned()+&(0..40).map(|n| format!("pub control bad_{n}<t:{}>(t input) exits ready {{t value}} {{exit ready {{input}};}}\n",if unknown {"missing"} else {"hidden"})).collect::<String>();
        let batch = check_errors(&source);
        assert_eq!(batch.errors.len(), 32, "{batch}");
        assert!(batch.limit_reached);
        for (n, e) in batch.errors.iter().enumerate() {
            assert_eq!(e.code, if unknown { "E_BOUND" } else { "E_VISIBILITY" });
            assert_eq!(e.span.line, n + 2);
        }
    }
}

#[test]
fn inferred_stored_adapter_batches_keep_generic_import_ranges_and_first_error_api() {
    let f = Fixture::new();
    let library = "// 🙂 inferred arms\r\npub control choose<t>(t input,bool flag) exits left {t value,u32 count},right {t value,u32 count} {\r\nauto saved=delta flag {\r\ntrue=>left {input,absent_first},\r\nfalse=>right {input,absent_second}\r\n};exit saved;}";
    f.write("jobs.tellur", library);
    f.write("main.tellur","import \"jobs.tellur\" as jobs;control main() exits auto {forward jobs.choose<string>(\"owned\",true);}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (e, selected) in errors.iter().zip(["absent_first", "absent_second"]) {
        assert_eq!(e["code"], "E_NAME");
        assert_eq!(e["range"], selected_range(library, selected));
        assert_eq!(e["path"], f.root.join("jobs.tellur").to_str().unwrap());
    }
    assert!(report["summary"].is_null());
    let m = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let first = tellur::compile_file(f.root.join("main.tellur"), m).unwrap_err();
    assert_eq!(first.diagnostic.code, "E_NAME");
    assert_eq!(first.diagnostic.span.line, 4);
    f.write(
        "jobs.tellur",
        &library
            .replace("absent_first", "1")
            .replace("absent_second", "2"),
    );
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn inferred_exit_adapter_batches_keep_original_ranges_and_first_error_compatibility() {
    let f = Fixture::new();
    let library = "// 🙂 inferred exits\r\ncontrol choose<t>(t input,bool flag) exits auto {\r\nexit delta flag {\r\ntrue=>ready {input,absent_first},\r\nfalse=>ready {input,absent_second}\r\n};}\r\npub control start() exits ready {string input,u32 value_1} {forward choose<string>(\"hello\",true);}";
    f.write("jobs.tellur", library);
    f.write(
        "main.tellur",
        "import \"jobs.tellur\" as jobs;control main() exits auto {forward jobs.start();}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (error, name) in errors.iter().zip(["absent_first", "absent_second"]) {
        assert_eq!(error["code"], "E_NAME");
        assert_eq!(error["range"], selected_range(library, name));
        assert_eq!(error["path"], f.root.join("jobs.tellur").to_str().unwrap());
    }
    assert!(report["summary"].is_null());
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let first = tellur::compile_file(f.root.join("main.tellur"), manifest).unwrap_err();
    assert_eq!(first.diagnostic.code, "E_NAME");
    assert_eq!(first.diagnostic.span.line, 4);
    f.write(
        "jobs.tellur",
        &library
            .replace("absent_first", "1")
            .replace("absent_second", "2"),
    );
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn trait_declaration_batches_report_one_error_per_unused_trait() {
    let source = "trait first {control one(unknown value) exits done;control two() exits auto;}\ntrait second {control read() exits auto;}\ntrait third {control peek(ref string input) exits ready {ref string value};}\ntrait fourth {type item;control item() exits done;}\ncontrol caller() exits done {u32 bad=true;exit done;}";
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let batch = tellur::check_source(source, manifest.clone()).unwrap_err();
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["E_TRAIT", "E_TRAIT_SIGNATURE", "E_LIFETIME", "E_TRAIT"],
        "{batch}"
    );
    assert_eq!(
        batch.errors.iter().map(|e| e.span.line).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(!batch.limit_reached);
    assert_eq!(
        tellur::compile(source, manifest).unwrap_err(),
        batch.errors[0]
    );
}

#[test]
fn trait_declaration_batches_preserve_transitive_imports_and_visibility() {
    let f = Fixture::new();
    for (file, name) in [("first.tellur", "one"), ("second.tellur", "two")] {
        f.write(file,&format!("// 🙂 original\r\nstruct hidden {{string text;}}\r\npub trait {name} {{\r\ncontrol read(hidden input) exits done;\r\n}}\r\n"));
    }
    f.write("middle.tellur","import \"first.tellur\" as first;import \"second.tellur\" as second;pub control run() exits done {exit done;}");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {forward middle.run();}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (error, file) in errors.iter().zip(["first.tellur", "second.tellur"]) {
        assert_eq!(error["code"], "E_VISIBILITY");
        assert_eq!(error["path"], f.root.join(file).to_str().unwrap());
        assert_eq!(error["position"]["line"], 4);
    }
    assert!(report["summary"].is_null());
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let first = tellur::compile_file(f.root.join("main.tellur"), manifest).unwrap_err();
    assert_eq!(first.diagnostic.code, "E_VISIBILITY");
    for file in ["first.tellur", "second.tellur"] {
        let path = f.root.join(file);
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("struct hidden", "pub struct hidden"),
        )
        .unwrap();
    }
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn trait_declaration_batches_check_named_shapes_and_origin_contracts() {
    let f = Fixture::new();
    let source = "outcome reply<t> {ready {t value}}struct record {u32 value;}\ntrait one {control read() exits reply<string,u32>;}\ntrait two {control read() exits record;}\ntrait three {control peek(ref string input) exits ready {ref string value};}\ncontrol main() exits done {exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(
        errors
            .iter()
            .map(|e| e["code"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["E_TRAIT", "E_TRAIT_SIGNATURE", "E_LIFETIME"],
        "{report}"
    );
    f.write(
        "main.tellur",
        &source
            .replace("reply<string,u32>", "reply<string>")
            .replace("exits record;", "exits done;")
            .replace("ref string value}", "ref string value from input}"),
    );
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn trait_declaration_batches_preserve_shared_identity_schema_and_later_phase_barriers() {
    let invalid = "trait one {control read() exits auto;}trait two {control read() exits auto;}";
    for (extra, code) in [
        (
            "pub control bad() exits auto {exit done;}",
            "E_PUBLIC_CONTRACT",
        ),
        ("trait one {}", "E_TRAIT"),
        ("struct bad['a] {ref string value <- 'missing;}", "E_ORIGIN"),
    ] {
        let source = format!("{invalid}{extra}");
        let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
        let batch = tellur::check_source(&source, manifest).unwrap_err();
        assert_eq!(batch.errors.len(), 1, "{batch}");
        assert_eq!(batch.errors[0].code, code, "{batch}");
    }
    let source = format!(
        "{invalid}implement one for string {{}}control broken() exits done {{u32 bad=true;exit done;}}"
    );
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let batch = tellur::check_source(&source, manifest).unwrap_err();
    assert_eq!(batch.errors.len(), 2, "{batch}");
    assert!(
        batch.errors.iter().all(|e| e.code == "E_TRAIT_SIGNATURE"),
        "{batch}"
    );
}

#[test]
fn trait_declaration_batches_share_the_diagnostic_budget() {
    let f = Fixture::new();
    let source = (0..40)
        .map(|n| format!("trait broken_{n} {{control read() exits auto;}}\n"))
        .collect::<String>()
        + "control main() exits done {exit done;}";
    f.write("main.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 32, "{report}");
    assert_eq!(report["diagnostics_limited"], true);
    for (n, error) in errors.iter().enumerate() {
        assert_eq!(error["code"], "E_TRAIT_SIGNATURE");
        assert_eq!(error["position"]["line"], n + 1);
    }
}

#[test]
fn constructor_definition_batches_cover_origins_partial_self_and_unused_generics() {
    let source = "struct origin['a] {ref string value <- 'a;constructor(ref string input) {self.value=input;}}\nstruct partial {string value;constructor() {emit(self);self.value=\"x\";}}\nstruct generic<t> {t value;constructor(t input) {exit done;self.value=input;}}\nstruct nested {string value;constructor() {self.value.part=\"x\";}}\ncontrol later() exits done {u32 bad=true;exit done;}";
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let batch = tellur::check_source(source, manifest.clone()).unwrap_err();
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [
            "E_ORIGIN",
            "E_CONSTRUCTOR",
            "E_CONSTRUCTOR",
            "E_CONSTRUCTOR"
        ],
        "{batch}"
    );
    assert_eq!(
        batch.errors.iter().map(|e| e.span.line).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(
        tellur::compile(source, manifest).unwrap_err(),
        batch.errors[0]
    );
}

#[test]
fn constructor_definition_batches_keep_one_error_per_constructor_and_stage_later_checks() {
    let f = Fixture::new();
    let source = "struct first {string text;constructor() {self.unknown=\"x\";exit done;}}\nstruct second {string text;constructor() {fault boundary {} contingency [] {terminate process 1;}}}\nstruct later {string text;constructor() {}}\ncontrol main() exits done {u32 bad=true;exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    assert!(errors.iter().all(|e| e["code"] == "E_CONSTRUCTOR"));
    assert!(report["summary"].is_null());
    let partial = source.replace("self.unknown=\"x\";", "self.text=\"x\";");
    f.write("main.tellur", &partial);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        2,
        "{report}"
    );
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("implicitly")
    );
}

#[test]
fn constructor_definition_batches_preserve_first_failing_file_and_transitive_locations() {
    let f = Fixture::new();
    let broken = "// 🙂 source\r\npub struct one {string value;pub constructor() {self.absent=\"x\";}}\r\npub struct two {string value;pub constructor() {self.absent=\"y\";}}\r\n";
    f.write("first.tellur", broken);
    f.write("second.tellur", broken);
    f.write(
        "middle.tellur",
        "import \"first.tellur\" as first;import \"second.tellur\" as second;",
    );
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    for file in ["first.tellur", "second.tellur"] {
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 2, "{report}");
        for (error, line) in errors.iter().zip([2, 3]) {
            assert_eq!(error["code"], "E_CONSTRUCTOR");
            assert_eq!(error["position"]["line"], line);
            assert_eq!(error["path"], f.root.join(file).to_str().unwrap());
        }
        let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
        let first = tellur::compile_file(f.root.join("main.tellur"), manifest).unwrap_err();
        assert_eq!(first.path, f.root.join(file));
        assert_eq!(first.diagnostic.span.line, 2);
        f.write(file, &broken.replace("self.absent", "self.value"));
    }
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn constructor_definition_batches_keep_parse_import_and_resolution_boundaries() {
    let f = Fixture::new();
    let broken = "struct one {string text;constructor() {exit done;}}struct two {string text;constructor() {exit done;}}";
    f.write("main.tellur", &format!("{broken}control malformed("));
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_PARSE");
    // Preparation precedes symbol registration/import traversal for this file.
    f.write(
        "main.tellur",
        &format!(
            "import \"absent.tellur\" as absent;{broken}pub control bad() exits auto {{exit done;}}"
        ),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        2,
        "{report}"
    );
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["code"] == "E_CONSTRUCTOR")
    );
    f.write("main.tellur", "import \"absent.tellur\" as absent;");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_IMPORT");
}

#[test]
fn constructor_definition_batches_share_one_file_budget() {
    let f = Fixture::new();
    let source = (0..40)
        .map(|n| format!("struct broken_{n} {{string text;constructor() {{exit done;}}}}\n"))
        .collect::<String>();
    f.write("main.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 32, "{report}");
    assert_eq!(report["diagnostics_limited"], true);
    for (n, error) in errors.iter().enumerate() {
        assert_eq!(error["code"], "E_CONSTRUCTOR");
        assert_eq!(error["position"]["line"], n + 1);
    }
}

#[test]
fn type_annotation_ranges_cover_declarations_signatures_traits_and_patterns() {
    let f = Fixture::new();
    for (source, selected, code) in [
        (
            "struct data {vector<string,u32> values;}",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "variant data {ready {vector<string,u32> values}}",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "variant data {first {u32 value}, second {vector<string,u32> value}}",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "outcome data {ready {vector<string,u32> values}}",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "control probe(ref absent_type input) exits done {exit done;}",
            "ref absent_type",
            "E_MANIFEST",
        ),
        (
            "control probe() exits ready {ref absent_type value} {exit ready {};}",
            "ref absent_type",
            "E_MANIFEST",
        ),
        (
            "control probe<t>(vector<t,u32> input) exits done {drop(input);exit done;}",
            "vector<t,u32>",
            "E_MANIFEST",
        ),
        (
            "trait reader {control probe(vector<string,u32> input) exits done;}",
            "vector<string,u32>",
            "E_TRAIT",
        ),
        (
            "struct data {string text;constructor(vector<string,u32> input) {self.text=\"x\";}}",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "region probe(input: vector<String,U32>) exits Done {exit Done;}",
            "vector<String,U32>",
            "E_MANIFEST",
        ),
        (
            "control produce() exits ready {u32 value} {exit ready {1};}control main() exits done {delta produce() {ready {bool value}=>known} [known]: exit done;}",
            "bool",
            "E_TYPE",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{source}: {report}");
        assert_eq!(
            error["range"],
            selected_range(source, selected),
            "{source}: {report}"
        );
        assert_eq!(error["position"], error["range"]["start"]);
    }
}

#[test]
fn type_annotation_ranges_preserve_written_comments_multiline_and_legacy_types() {
    let f = Fixture::new();
    for (source, selected) in [
        (
            "// 😀\r\ncontrol probe( ref // 😃\r\n absent_type input // after\r\n) exits done {exit done;}",
            "ref // 😃\r\n absent_type",
        ),
        (
            "struct data {\r\n    vector<\r\n        // 😀\r\n        string,\r\n        u32\r\n    > values; // after\r\n}",
            "vector<\r\n        // 😀\r\n        string,\r\n        u32\r\n    >",
        ),
        (
            "region probe(input: & // 🙂\n absent_type) exits Done {exit Done;}",
            "& // 🙂\n absent_type",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(source, selected),
            "{report}"
        );
    }
}

#[test]
fn type_annotation_ranges_preserve_transitive_imported_aliases_and_visibility() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "struct hidden {string text;}pub struct visible {pub string text;}",
    );
    for (annotation, code) in [
        ("vector<types.hidden>", "E_VISIBILITY"),
        ("vector<types.absent>", "E_NAME"),
    ] {
        let library = format!(
            "import \"types.tellur\" as types;\r\n// 🙂\r\npub control probe(\r\n    {annotation} input\r\n) exits done {{drop(input);exit done;}}\r\n"
        );
        f.write("library.tellur", &library);
        f.write("middle.tellur", "import \"library.tellur\" as lib;");
        f.write(
            "main.tellur",
            "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
        );
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(
            error["path"],
            f.root.join("library.tellur").to_str().unwrap()
        );
        assert_eq!(error["range"], selected_range(&library, annotation));
    }
}

#[test]
fn type_annotation_ranges_keep_original_generic_definition_and_named_payload_locations() {
    let f = Fixture::new();
    let library =
        "// original definition\r\npub struct bag<t> {\r\n    vector<t,u32> values;\r\n}\r\n";
    f.write("library.tellur", library);
    f.write("main.tellur", "import \"library.tellur\" as lib;control probe(lib.bag<string> input) exits done {drop(input);exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"][0]["path"],
        f.root.join("library.tellur").to_str().unwrap()
    );
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(library, "vector<t,u32>")
    );
    let library = "struct hidden {string text;}\noutcome reply {ready {hidden value}}\npub trait reader {control probe() exits reply;}";
    f.write("library.tellur", library);
    f.write(
        "main.tellur",
        "import \"library.tellur\" as lib;control main() exits done {exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_VISIBILITY", "{report}");
    let start = library.find("hidden value").unwrap();
    assert_eq!(
        report["diagnostics"][0]["range"],
        json!({"start":scalar_position(library,start),"end":scalar_position(library,start+6)})
    );
}

#[test]
fn type_annotation_ranges_keep_batches_first_error_apis_and_non_type_points() {
    let source = "control first(\n ref absent_first input\n) exits done {exit done;}\ncontrol second() exits ready {\n absent_second value\n} {exit ready {}; }";
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let batch = tellur::check_source(source, manifest.clone()).unwrap_err();
    assert_eq!(batch.errors.len(), 2, "{batch}");
    for (e, selected) in batch
        .errors
        .iter()
        .zip(["ref absent_first", "absent_second"])
    {
        assert_eq!(e.code, "E_MANIFEST");
        let range = json!({"start":{"line":e.span.line,"column":e.span.column},"end":e.end.as_deref().unwrap()});
        assert_eq!(range, selected_range(source, selected));
    }
    assert_eq!(
        tellur::compile(source, manifest.clone()).unwrap_err(),
        batch.errors[0]
    );
    let point =
        tellur::compile("pub control probe() exits auto {exit done;}", manifest).unwrap_err();
    assert_eq!(point.code, "E_PUBLIC_CONTRACT");
    assert!(
        point.end.is_none(),
        "unrelated contract-shape diagnostics remain points"
    );
}
