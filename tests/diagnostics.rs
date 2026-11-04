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
fn type_annotation_ranges_keep_batches_first_error_apis_and_separate_contract_ranges() {
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
    let source = "pub control probe() exits auto {exit done;}";
    let shape = tellur::compile(source, manifest).unwrap_err();
    assert_eq!(shape.code, "E_PUBLIC_CONTRACT");
    assert_eq!(
        json!({"start":shape.span,"end":shape.end.as_deref().unwrap()}),
        selected_range(source, "exits auto")
    );
}

#[test]
fn loop_type_annotations_select_written_types_without_retargeting_initializers() {
    let f = Fixture::new();
    for (source, selected, code) in [
        (
            "control main() exits done {loop work(vector<string, u32> entries=[]) {exit done;}}",
            "vector<string, u32>",
            "E_MANIFEST",
        ),
        (
            "control main() exits done {supervised loop work(ref absent_type value=true) {exit done;}}",
            "ref absent_type",
            "E_MANIFEST",
        ),
        (
            "region main() exits Done {loop work(value: vector<String,U32> =[]) {exit Done;}}",
            "vector<String,U32>",
            "E_MANIFEST",
        ),
        (
            "control main() exits done {loop work(u32 number=true) {exit done;}}",
            "true",
            "E_TYPE",
        ),
        (
            "control main() exits done {loop work(vector<u32> values=[1,true]) {exit done;}}",
            "true",
            "E_TYPE",
        ),
        (
            "control main() exits done {\r\n loop work(\r\n mutable vector<\r\n // 😀\r\n string, u32\r\n > entries=[]) {exit done;}\r\n}",
            "vector<\r\n // 😀\r\n string, u32\r\n >",
            "E_MANIFEST",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let e = &report["diagnostics"][0];
        assert_eq!(e["code"], code, "{report}");
        assert_eq!(e["range"], selected_range(source, selected), "{report}");
        assert_eq!(e["position"], e["range"]["start"]);
    }
}

#[test]
fn loop_type_annotation_ranges_survive_import_offsets_lookup_and_generic_preparation() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "struct hidden {u32 value;} pub struct visible {u32 value;}",
    );
    f.write("main.tellur","import \"library.tellur\" as library; control main() exits done {library.probe();exit done;}");
    for (header, annotation, code) in [
        ("", "vector<string,u32>", "E_MANIFEST"),
        (
            "import \"types.tellur\" as types;\r\n",
            "types.hidden",
            "E_VISIBILITY",
        ),
        (
            "struct box<t> {t value;}\r\n",
            "box<u32,string>",
            "E_GENERIC",
        ),
        ("", "outcome<missing>", "E_MANIFEST"),
    ] {
        let source = format!(
            "{header}// 😀\r\npub control probe() exits done {{\r\n loop work(\r\n {annotation} value=true\r\n ) {{exit done;}}\r\n}}"
        );
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let e = &report["diagnostics"][0];
        assert_eq!(e["code"], code, "{source}: {report}");
        assert!(
            e["path"].as_str().unwrap().ends_with("library.tellur"),
            "{report}"
        );
        assert_eq!(e["range"], selected_range(&source, annotation), "{report}");
    }
    let generic = "control probe<t>(t input) exits done {loop work(vector<t,u32> value=[]) {drop(input);exit done;}} control main() exits done {probe<string>(\"x\");exit done;}";
    f.write("main.tellur", generic);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(generic, "vector<t,u32>"),
        "{report}"
    );
}

#[test]
fn loop_type_ranges_keep_body_batches_and_structural_error_locations() {
    let source = "control first() exits done {loop work(vector<string,u32> items=[]) {exit done;}}\ncontrol second() exits done {loop work(ref absent value=true) {exit done;}}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    for (e, annotation) in errors
        .errors
        .iter()
        .zip(["vector<string,u32>", "ref absent"])
    {
        let range = selected_range(source, annotation);
        assert_eq!(serde_json::to_value(e.span).unwrap(), range["start"]);
        assert_eq!(
            serde_json::to_value(e.end.as_ref().unwrap()).unwrap(),
            range["end"]
        );
    }
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    let invalid = "control main() exits done {loop work(u32 n=1, u32 n=2) {exit done;}}";
    let e = compile(
        invalid,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_NAME");
    assert_eq!(e.span.column, invalid.find("loop").unwrap() + 1);
    assert!(e.end.is_none());
}

#[test]
fn loop_type_ranges_cover_generic_producer_arguments_and_preserve_again_expressions() {
    let f = Fixture::new();
    for (source, selected, code) in [
        (
            "control maker<t>() exits done {exit done;} control main() exits done {loop work(outcome<maker<u32,string>> value=true) {exit done;}}",
            "outcome<maker<u32,string>>",
            "E_GENERIC",
        ),
        (
            "control main() exits done {loop work(u32 number=1) {again work(true);}}",
            "true",
            "E_TYPE",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(report["diagnostics"][0]["code"], code, "{report}");
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(source, selected),
            "{report}"
        );
    }
}

#[test]
fn local_type_annotations_select_types_and_preserve_initializer_errors() {
    let f = Fixture::new();
    for (body, selected, code) in [
        (
            "vector<string,u32> value=[];",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "mutable vector<\r\n // 😀\r\n string, u32\r\n > // trailing\r\n value=[];",
            "vector<\r\n // 😀\r\n string, u32\r\n >",
            "E_MANIFEST",
        ),
        ("&Absent value=true;", "&Absent", "E_MANIFEST"),
        (
            "ref mutable absent value=true;",
            "ref mutable absent",
            "E_MANIFEST",
        ),
        (
            "vector<string,u32> value=delta produce() {done => done {}};",
            "vector<string,u32>",
            "E_MANIFEST",
        ),
        (
            "u32 value=delta produce() {done => done {}};",
            "u32",
            "E_OUTCOME",
        ),
        ("u32 value=true;", "true", "E_TYPE"),
        ("vector<u32> values=[1,true];", "true", "E_TYPE"),
        (
            "string text=\"x\";ref u32 value=borrow text;",
            "borrow text",
            "E_TYPE",
        ),
        ("string value=u32_text(true);", "true", "E_TYPE"),
    ] {
        let source = format!(
            "control produce() exits done {{exit done;}} control main() exits done {{{body}exit done;}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{source}: {report}");
        assert_eq!(
            error["range"],
            selected_range(&source, selected),
            "{source}: {report}"
        );
        assert_eq!(error["position"], error["range"]["start"]);
    }
}

#[test]
fn local_type_ranges_cover_capture_adapters_imports_and_generic_preparation() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "struct hidden {u32 n;} pub struct visible {u32 n;}",
    );
    f.write("main.tellur", "import \"library.tellur\" as library;control main() exits done {library.probe();exit done;}");
    for (header, annotation, code) in [
        (
            "import \"types.tellur\" as types;\r\n",
            "types.hidden",
            "E_VISIBILITY",
        ),
        (
            "struct box<t> {t value;}\r\n",
            "box<u32,string>",
            "E_GENERIC",
        ),
        (
            "control maker<t>() exits done {exit done;}\r\n",
            "outcome<maker<u32,string>>",
            "E_GENERIC",
        ),
    ] {
        for initializer in [
            "true",
            "capture produce()",
            "delta produce() {done => done {}}",
        ] {
            let source = format!(
                "{header}// 😀\r\ncontrol produce() exits done {{exit done;}}\r\npub control probe() exits done {{\r\n {annotation} result={initializer};exit done;\r\n}}"
            );
            f.write("library.tellur", &source);
            let report = f.report(&["check", "main.tellur", "--json"], false);
            let error = &report["diagnostics"][0];
            assert_eq!(error["code"], code, "{source}: {report}");
            assert!(
                error["path"].as_str().unwrap().ends_with("library.tellur"),
                "{report}"
            );
            assert_eq!(
                error["range"],
                selected_range(&source, annotation),
                "{source}: {report}"
            );
        }
    }
    for initializer in ["[]", "delta produce() {done => done {}}"] {
        let source = format!(
            "control produce() exits done {{exit done;}} control probe<t>(t input) exits done {{vector<t,u32> result={initializer};drop(input);exit done;}} control main() exits done {{probe<string>(\"x\");exit done;}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(report["diagnostics"][0]["code"], "E_MANIFEST", "{report}");
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(&source, "vector<t,u32>"),
            "{report}"
        );
    }
}

#[test]
fn local_type_ranges_preserve_batches_first_errors_and_capture_mismatch_points() {
    let source = "control first() exits done {vector<string,u32> values=[];exit done;}\ncontrol second() exits done {ref absent value=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    for (error, annotation) in errors
        .errors
        .iter()
        .zip(["vector<string,u32>", "ref absent"])
    {
        let range = selected_range(source, annotation);
        assert_eq!(serde_json::to_value(error.span).unwrap(), range["start"]);
        assert_eq!(
            serde_json::to_value(error.end.as_ref().unwrap()).unwrap(),
            range["end"]
        );
    }
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    // Capture compatibility is not type validation. Preserve its existing point
    // rather than assigning the declared type's range to an initializer mismatch.
    let source = "control produce() exits done {exit done;} control main() exits done {u32 value=capture produce();exit done;}";
    let error = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code, "E_TYPE");
    assert_eq!(error.span.column, source.find("u32 value").unwrap() + 1);
    assert!(error.end.is_none());
}

#[test]
fn public_type_ranges_select_authored_interfaces_after_body_checking() {
    let f = Fixture::new();
    for (source, selected) in [
        (
            "struct hidden {u32 n;} pub struct box {pub vector<hidden> value;}",
            "vector<hidden>",
        ),
        (
            "struct hidden {u32 n;} pub variant choice {yes {option<hidden> value},no}",
            "option<hidden>",
        ),
        (
            "struct hidden {u32 n;} pub outcome answer {yes {option<hidden> value},no}",
            "option<hidden>",
        ),
        (
            "struct hidden {u32 n;} pub control probe(ref hidden value) exits done {drop(value);exit done;}",
            "ref hidden",
        ),
        (
            "struct hidden {u32 n;} pub control probe() exits ready {option<hidden> value} {exit ready {none {}};}",
            "option<hidden>",
        ),
        (
            "struct hidden {u32 n;} pub struct box<t> {pub map<string,hidden> values;t data;}",
            "map<string,hidden>",
        ),
        (
            "control produce() exits done {exit done;}pub control consume(outcome<produce> value) exits done {exit delta value {done => done {}};}",
            "outcome<produce>",
        ),
        (
            "struct hidden {u32 n;} pub control probe<t>(ref t input) exits ready {option<hidden> value} {drop(input);exit ready {none {}};}",
            "option<hidden>",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(
            report["diagnostics"][0]["code"], "E_VISIBILITY",
            "{source}: {report}"
        );
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(source, selected),
            "{source}: {report}"
        );
    }
}

#[test]
fn public_type_ranges_preserve_imported_multiline_spelling_and_aliases() {
    let f = Fixture::new();
    f.write("types.tellur", "pub struct visible {pub u32 n;}");
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library; control main() exits done {exit done;}",
    );
    for (body, selected) in [
        (
            "pub struct box {pub map<\r\n // 😀\r\n string,\r\n hidden\r\n > data;}",
            "map<\r\n // 😀\r\n string,\r\n hidden\r\n >",
        ),
        (
            "pub control probe(ref\r\n hidden value) exits done {drop(value);exit done;}",
            "ref\r\n hidden",
        ),
        (
            "pub control probe() exits ready {vector<hidden> values} {exit ready {[]};}",
            "vector<hidden>",
        ),
        (
            "pub struct box {pub pair<types.visible,hidden> value;}",
            "pair<types.visible,hidden>",
        ),
    ] {
        let source = format!(
            "import \"types.tellur\" as types;\r\nstruct hidden {{u32 n;}}\r\npub struct pair<a,b> {{pub a left;pub b right;}}\r\n// 😀\r\n{body}"
        );
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_VISIBILITY", "{report}");
        assert!(
            error["path"].as_str().unwrap().ends_with("library.tellur"),
            "{report}"
        );
        assert_eq!(
            error["range"],
            selected_range(&source, selected),
            "{report}"
        );
    }
}

#[test]
fn public_type_ranges_keep_visibility_phase_order_and_named_contract_locations() {
    let f = Fixture::new();
    // A later public data field is still checked before control signatures.
    let source = "struct hidden {u32 n;}pub control probe(ref hidden value) exits done {drop(value);exit done;}pub struct box {pub vector<hidden> values;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "vector<hidden>")
    );
    let errors = check_errors(source);
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(first, errors.errors[0]);
    // A body error still stops checking before this late visibility phase.
    let source = "struct hidden {u32 n;}pub control probe(ref hidden value) exits done {u32 count=true;drop(value);exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_TYPE");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "true")
    );
    // Source data validation still precedes generated producer outcome types.
    let source =
        "control produce() exits done {exit done;}pub struct box {pub outcome<produce> value;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_MANIFEST");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "outcome<produce>")
    );
    // Whole named-contract references retain their own authored type annotation.
    let source = "outcome hidden {done}pub control probe() exits hidden {exit done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_VISIBILITY");
    let at = source.rfind("hidden").unwrap();
    assert_eq!(
        report["diagnostics"][0]["range"],
        json!({"start":scalar_position(source,at),"end":scalar_position(source,at+6)})
    );
}

#[test]
fn named_completion_type_ranges_cover_generic_and_trait_validation() {
    let f = Fixture::new();
    for (source, selected, code) in [
        (
            "outcome reply<t> {done} control probe() exits reply<u32,bool> {exit done;}",
            "reply<u32,bool>",
            "E_GENERIC",
        ),
        (
            "outcome hidden<t> {done} pub control probe() exits hidden< // 😀\r\n u32\r\n > {exit done;}",
            "hidden< // 😀\r\n u32\r\n >",
            "E_VISIBILITY",
        ),
        (
            "outcome hidden<t> {done} pub control probe<t>() exits hidden<t> {exit done;}",
            "hidden<t>",
            "E_VISIBILITY",
        ),
        (
            "outcome reply<t> {done} trait reader {control read(ref self input) exits reply<u32,bool>;}",
            "reply<u32,bool>",
            "E_TRAIT",
        ),
        (
            "outcome hidden<t> {done} pub trait reader {control read(ref self input) exits hidden<u32>;}",
            "hidden<u32>",
            "E_VISIBILITY",
        ),
        (
            "trait reader {control read(ref self input) exits vector<u32>;}",
            "vector<u32>",
            "E_TRAIT_SIGNATURE",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{source}: {report}");
        // A generic name can occur in both declaration and use; select the use.
        let at = source.rfind(selected).unwrap();
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(source,at),"end":scalar_position(source,at+selected.len())}),
            "{source}: {report}"
        );
    }
}

#[test]
fn named_completion_type_ranges_preserve_imports_and_exclude_origins() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub outcome reply<t> {done} outcome hidden {done}",
    );
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library; control main() exits done {exit done;}",
    );
    for (body, selected, code) in [
        (
            "pub control probe(ref string input) exits types.missing[input] {exit done;}",
            "types.missing",
            "E_NAME",
        ),
        (
            "pub control probe() exits types.hidden {exit done;}",
            "types.hidden",
            "E_VISIBILITY",
        ),
        (
            "pub control probe() exits types.reply<\r\n // 😀\r\n u32, bool\r\n > {exit done;}",
            "types.reply<\r\n // 😀\r\n u32, bool\r\n >",
            "E_GENERIC",
        ),
        (
            "pub trait reader {control read(ref self input) exits types.reply<\r\n u32,bool\r\n >;}",
            "types.reply<\r\n u32,bool\r\n >",
            "E_TRAIT",
        ),
        (
            "outcome hidden<t> {done} pub control probe<t>() exits hidden< t > {exit done;}",
            "hidden< t >",
            "E_VISIBILITY",
        ),
    ] {
        let source = format!("import \"types.tellur\" as types;\r\n// 😀\r\n{body}");
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert!(
            error["path"].as_str().unwrap().ends_with("library.tellur"),
            "{report}"
        );
        assert_eq!(
            error["range"],
            selected_range(&source, selected),
            "{report}"
        );
    }
}

#[test]
fn named_completion_type_ranges_keep_origin_and_body_errors_separate() {
    let source =
        "outcome reply {done} control probe(ref string input) exits reply[input] {exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors[0].code, "E_ORIGIN");
    assert_eq!(
        errors.errors[0].span.column,
        source.find("[input]").unwrap() + 1
    );
    assert_eq!(
        errors.errors[0].end.as_ref().unwrap().column,
        source.find("[input]").unwrap() + 8
    );
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    let source =
        "outcome hidden {done} pub control probe() exits hidden {u32 value=true;exit done;}";
    let error = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code, "E_TYPE");
    assert_eq!(error.span.column, source.find("true").unwrap() + 1);
    assert_eq!(error.end.unwrap().column, source.find("true").unwrap() + 5);
    for exits in ["done", "{done}", "auto"] {
        let source = format!("control probe() exits {exits} {{exit done;}}");
        compile(
            &source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn named_completion_origin_ranges_cover_binding_inputs_and_authority() {
    let f = Fixture::new();
    for (source, mapping, code) in [
        (
            "outcome reply['a] {ready {ref string value <- 'a}} control probe(ref string input) exits reply[\r\n input, // 😀\r\n input\r\n] {exit ready {input};}",
            "[\r\n input, // 😀\r\n input\r\n]",
            "E_ORIGIN",
        ),
        (
            "control probe(ref string input) exits done[input] {exit done;}",
            "[input]",
            "E_ORIGIN",
        ),
        (
            "outcome reply['a] {ready {ref string value <- 'a}} control probe(ref string input) exits reply[missing] {exit ready {input};}",
            "[missing]",
            "E_LIFETIME",
        ),
        (
            "outcome reply['a] {ready {ref string value <- 'a}} control probe(string input) exits reply[input] {exit ready {borrow input};}",
            "[input]",
            "E_LIFETIME",
        ),
        (
            "outcome reply['a] {ready {ref mutable string value <- 'a}} control probe(ref string input) exits reply[input] {exit ready {input};}",
            "[input]",
            "E_LIFETIME",
        ),
        (
            "struct view['a] {ref string value <- 'a;} outcome reply['a] {ready {view['a] value}} control probe(ref string input) exits reply[missing] {exit ready {view {value:input}};}",
            "[missing]",
            "E_ORIGIN",
        ),
        (
            "struct view['a] {ref string value <- 'a;} outcome reply['a] {ready {ref string value <- 'a}} control probe(view input) exits reply[input['missing]] {exit ready {input.value};}",
            "[input['missing]]",
            "E_LIFETIME",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(error["range"], selected_range(source, mapping), "{report}");
        assert_eq!(error["position"], error["range"]["start"]);
    }
}

#[test]
fn named_completion_origin_ranges_survive_imports_generics_and_traits() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    let schema = "pub outcome reply<t>['a] {ready {ref t value <- 'a}}\r\n";
    for body in [
        "pub control probe<t>(ref t input) exits reply<t>[missing] {exit ready {input};}",
        "pub control probe(ref string input) exits reply<string>[missing] {exit ready {input};}",
        "pub trait reader {control read(ref self input) exits reply<self>[missing];}",
    ] {
        let source = format!("{schema}// 😀\r\n{body}");
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_LIFETIME", "{report}");
        assert!(error["path"].as_str().unwrap().ends_with("library.tellur"));
        assert_eq!(
            error["range"],
            selected_range(&source, "[missing]"),
            "{report}"
        );
    }
}

#[test]
fn named_completion_origin_ranges_keep_missing_contract_and_schema_locations_distinct() {
    for source in [
        "pub control probe(ref string input) exits ready {ref string value} {exit ready {input};}",
        "outcome reply['a] {ready {ref string value <- 'a}} pub control probe(ref string input) exits reply {exit ready {input};}",
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1);
        assert_eq!(errors.errors[0].code, "E_PUBLIC_CONTRACT");
        let selected = if source.contains("exits reply") {
            "reply {exit"
        } else {
            "ref string value"
        };
        let at = source.find(selected).unwrap();
        let length = if selected == "reply {exit" { 5 } else { 10 };
        assert_eq!(errors.errors[0].span.column, at + 1);
        assert_eq!(
            errors.errors[0].end.as_ref().unwrap().column,
            at + 1 + length
        );
    }
    let source = "outcome reply['a] {ready {ref string value <- 'missing}} control probe(ref string input) exits reply[input] {exit ready {input};}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].code, "E_ORIGIN");
    let at = source.find("<- 'missing").unwrap();
    assert_eq!(errors.errors[0].span.column, at + 1);
    assert_eq!(
        errors.errors[0].end.as_ref().unwrap().column,
        at + 1 + "<- 'missing".len()
    );
    let source = "outcome reply['a] {ready {ref string value <- 'a}} control probe(ref string input) exits reply[input] {u32 wrong=true;exit ready {input};}";
    let errors = check_errors(source);
    assert_eq!(errors.errors[0].code, "E_TYPE");
    assert_eq!(
        errors.errors[0].span.column,
        source.find("true").unwrap() + 1
    );
    assert_eq!(
        errors.errors[0].end.as_ref().unwrap().column,
        source.find("true").unwrap() + 5
    );
}

#[test]
fn return_origin_ranges_select_from_clauses_and_contained_selectors() {
    let f = Fixture::new();
    for (source, selected) in [
        (
            "control probe(ref string input) exits ready {ref string value from missing} {exit ready {input};}",
            "from missing",
        ),
        (
            "control probe(ref string input) exits ready {ref string first from input,ref string second from // 😀\r\n missing // outside\r\n} {exit ready {input,input};}",
            "from // 😀\r\n missing",
        ),
        (
            "control probe(ref string input) exits first {ref string value from input},second {ref string value from missing} {exit first {input};}",
            "from missing",
        ),
        (
            "control probe(string input) exits ready {ref string value from input} {exit ready {borrow input};}",
            "from input",
        ),
        (
            "control probe(ref string input) exits ready {u32 value from input} {exit ready {0};}",
            "from input",
        ),
        (
            "control probe(ref string input) exits ready {ref mutable string value from input} {exit ready {input};}",
            "from input",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(view input) exits ready {ref string value from input['missing]} {exit ready {input.value};}",
            "from input['missing]",
        ),
        (
            "region probe(input: &String) exits Ready(value: &String from missing) {[Entry]:exit Ready(input);}",
            "from missing",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_LIFETIME", "{report}");
        assert_eq!(error["range"], selected_range(source, selected), "{report}");
    }
}

#[test]
fn return_origin_ranges_preserve_imported_generic_and_trait_signatures() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    for body in [
        "pub control probe<t>(ref t input) exits ready {ref t value from missing} {exit ready {input};}",
        "pub control probe(ref string input) exits ready {ref string value from missing} {exit ready {input};}",
        "pub trait reader {control read(ref self input) exits ready {ref self value from missing};}",
        "trait reader {control read(ref self input) exits ready {ref self value from input};} struct value {u32 n;} implement reader for value {control read(ref self input) exits ready {ref self value from missing} {exit ready {input};}}",
    ] {
        let source = format!("// 😀\r\n{body}");
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_LIFETIME", "{report}");
        assert!(error["path"].as_str().unwrap().ends_with("library.tellur"));
        assert_eq!(
            error["range"],
            selected_range(&source, "from missing"),
            "{report}"
        );
    }
}

#[test]
fn return_origin_range_batches_keep_first_error_order_and_missing_contract_locations() {
    let source = "control first(ref string input) exits ready {ref string value from missing} {exit ready {input};}\ncontrol second(ref string input) exits ready {ref mutable string value from input} {exit ready {input};}\ncontrol body() exits done {u32 value=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    let repaired = source
        .replace("from missing", "from input")
        .replace("ref mutable string value", "ref string value");
    let errors = check_errors(&repaired);
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].code, "E_TYPE");
    for source in [
        "pub control probe(ref string input) exits ready {ref string value} {exit ready {input};}",
        "trait reader {control read(ref self input) exits ready {ref self value};}",
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1);
        if source.starts_with("pub") {
            let at = source.find("ref string value").unwrap();
            assert_eq!(errors.errors[0].span.column, at + 1);
            assert_eq!(errors.errors[0].end.as_ref().unwrap().column, at + 11);
        } else {
            // Trait origin requirements remain a separate validation path.
            assert!(errors.errors[0].end.is_none(), "{errors}");
        }
    }
}

#[test]
fn payload_origin_ranges_select_brackets_separately_from_types_and_bindings() {
    let f = Fixture::new();
    for (source, selected) in [
        (
            "struct view['a] {ref string value <- 'a;} control probe(ref string input) exits ready {view[missing] value} {exit ready {view {value:input}};}",
            "[missing]",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(ref string input) exits ready {view[input,input] value} {exit ready {view {value:input}};}",
            "[input,input]",
        ),
        (
            "control probe(ref string input) exits ready {u32[input] value} {exit ready {0};}",
            "[input]",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(string input) exits ready {view[input] value} {exit ready {view {value:borrow input}};}",
            "[input]",
        ),
        (
            "struct view['a] {ref mutable string value <- 'a;} control probe(ref string input) exits ready {view[input] value} {exit ready {view {value:input}};}",
            "[input]",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(view input) exits ready {view[input['missing]] value} {exit ready {input};}",
            "[input['missing]]",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(ref string input) exits ready {view[input] first,view[ // 😀\r\n missing\r\n] second // outside\r\n} {exit ready {view {value:input},view {value:input}};}",
            "[ // 😀\r\n missing\r\n]",
        ),
        (
            "struct view['a] {ref string value <- 'a;} region probe(input: &String) exits Ready(value: view[missing]) {[Entry]:exit Ready(view {value:input});}",
            "[missing]",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_ORIGIN", "{report}");
        assert_eq!(error["range"], selected_range(source, selected), "{report}");
    }
}

#[test]
fn payload_origin_ranges_survive_imports_generics_and_trait_signatures() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    let schema = "pub struct view<t>['a] {pub ref t value <- 'a;}\r\n";
    for body in [
        "pub control probe<t>(ref t input) exits ready {view<t>[missing] value} {exit ready {view<t> {value:input}};}",
        "pub control probe(ref string input) exits ready {view<string>[missing] value} {exit ready {view<string> {value:input}};}",
        "pub trait reader {control read(ref self input) exits ready {view<self>[missing] value};}",
        "trait reader {control read(ref self input) exits ready {view<self>[input] value};} struct value {u32 n;} implement reader for value {control read(ref self input) exits ready {view<self>[missing] value} {exit ready {view<self> {value:input}};}}",
    ] {
        let source = format!("{schema}// 😀\r\n{body}");
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_ORIGIN", "{report}");
        assert!(error["path"].as_str().unwrap().ends_with("library.tellur"));
        assert_eq!(
            error["range"],
            selected_range(&source, "[missing]"),
            "{report}"
        );
    }
}

#[test]
fn payload_origin_ranges_preserve_storage_clauses_named_contracts_and_error_order() {
    let f = Fixture::new();
    for (source, selected, code) in [
        (
            "struct view['a] {ref string value <- 'a;} control probe(ref view input) exits ready {ref view[input['a]] value from missing} {exit ready {input};}",
            "from missing",
            "E_LIFETIME",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(ref view input) exits ready {ref view[missing] value from input} {exit ready {input};}",
            "[missing]",
            "E_ORIGIN",
        ),
        (
            "struct view['a] {ref string value <- 'a;} outcome reply['a] {ready {view['a] value}} control probe(ref string input) exits reply[missing] {exit ready {view {value:input}};}",
            "[missing]",
            "E_ORIGIN",
        ),
        (
            "struct view['a] {ref string value <- 'a;} control probe(ref string input) exits ready {view[input] value} {u32 wrong=true;exit ready {view {value:input}};}",
            "true",
            "E_TYPE",
        ),
        (
            "control probe(ref string input) exits ready {unknown[input] value} {exit ready {input};}",
            "unknown",
            "E_MANIFEST",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(error["range"], selected_range(source, selected), "{report}");
    }
    let source = "struct view['a] {ref string value <- 'a;}\ncontrol first(ref string input) exits ready {view[missing] value} {exit ready {view {value:input}};}\ncontrol second(ref string input) exits ready {view[missing] value} {exit ready {view {value:input}};}\ncontrol body() exits done {u32 wrong=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    for source in [
        "struct view['a] {ref string value <- 'a;} pub control probe(ref string input) exits ready {view value} {exit ready {view {value:input}};}",
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        let at = source.find("view value").unwrap();
        assert_eq!(errors.errors[0].span.column, at + 1);
        assert_eq!(errors.errors[0].end.as_ref().unwrap().column, at + 5);
    }
}

#[test]
fn schema_origin_ranges_select_storage_arrows_and_referent_brackets() {
    let f = Fixture::new();
    let view = "struct view['a] {ref string value <- 'a;}";
    for (declaration, selected) in [
        (
            "struct bad['a] {ref string value <- 'missing;}",
            "<- 'missing",
        ),
        (
            "struct bad['a] {value: &String <- 'missing;}",
            "<- 'missing",
        ),
        (
            "struct bad['a] {ref string value <- // 😀\r\n 'missing // outside\r\n;}",
            "<- // 😀\r\n 'missing",
        ),
        ("struct bad['a] {view['missing] value;}", "['missing]"),
        (
            "struct bad['a] {ref view['a] value <- 'missing;}",
            "<- 'missing",
        ),
        (
            "struct bad['a] {ref view['missing] value <- 'a;}",
            "['missing]",
        ),
        (
            "struct bad['a] {ref view['first] value <- 'second;}",
            "<- 'second",
        ),
        ("struct bad['a] {view[value] value;}", "[value]"),
        ("struct bad {ref string value <- 'a;}", "<- 'a"),
        ("struct bad['a] {u32['a] value;}", "['a]"),
        ("struct bad['a] {view['a,'a] value;}", "['a,'a]"),
        ("struct bad['a] {ref view['a,'a] value <- 'a;}", "['a,'a]"),
        ("struct bad['a] {ref view value <- 'a;}", "<- 'a"),
        (
            "variant bad['a] {some {ref string value <- 'missing},none}",
            "<- 'missing",
        ),
        (
            "outcome bad['a] {some {ref view['missing] value <- 'a},none}",
            "['missing]",
        ),
    ] {
        let source = format!("{view}\n{declaration}\ncontrol main() exits done {{exit done;}}");
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_ORIGIN", "{source}: {report}");
        assert_eq!(
            error["range"],
            {
                let at = view.len() + 1 + declaration.rfind(selected).unwrap();
                json!({"start":scalar_position(&source, at),"end":scalar_position(&source, at+selected.len())})
            },
            "{source}: {report}"
        );
    }
}

#[test]
fn schema_origin_ranges_survive_imported_generic_data_and_named_outcomes() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    for (body, selected) in [
        (
            "pub struct bad<t>['a] {pub ref t value <- 'missing;}",
            "<- 'missing",
        ),
        (
            "pub variant bad<t>['a] {some {ref view<t>['missing] value <- 'a},none}",
            "['missing]",
        ),
        (
            "pub outcome bad<t>['a] {some {ref view<t>['a] value <- // 😀\r\n 'missing},none}",
            "<- // 😀\r\n 'missing",
        ),
    ] {
        let source =
            format!("pub struct view<t>['a] {{pub ref t value <- 'a;}}\r\n// imported\r\n{body}");
        f.write("library.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_ORIGIN", "{report}");
        assert!(error["path"].as_str().unwrap().ends_with("library.tellur"));
        assert_eq!(
            error["range"],
            selected_range(&source, selected),
            "{report}"
        );
    }
}

#[test]
fn schema_origin_ranges_preserve_shared_preparation_order_and_missing_mapping_fallbacks() {
    let source = "struct first['a] {ref string value <- 'missing;}\nstruct second['b] {ref string value <- 'other;}\ncontrol main() exits done {u32 wrong=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert_eq!(errors.errors[0].code, "E_ORIGIN");
    assert!(errors.errors[0].message.contains("'missing"));
    assert!(errors.errors[0].end.is_some());
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
    for source in ["struct view['a] {ref string value;} control main() exits done {exit done;}"] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, "E_ORIGIN");
        assert!(errors.errors[0].end.is_none(), "{errors}");
    }
}

#[test]
fn schema_group_ranges_select_duplicate_occurrences_and_unused_declarations() {
    let f = Fixture::new();
    for (declaration, selected, message) in [
        (
            "struct view['a,'a] {ref string value <- 'a;}",
            "'a",
            "duplicate",
        ),
        (
            "struct view['a,' // 😀\r\n a // outside\r\n] {ref string value <- 'a;}",
            "' // 😀\r\n a",
            "duplicate",
        ),
        (
            "struct view['a,'unused] {ref string value <- 'a;}",
            "'unused",
            "not used",
        ),
        (
            "struct view['a,' // 😀\r\n unused // outside\r\n] {ref string value <- 'a;}",
            "' // 😀\r\n unused",
            "not used",
        ),
        (
            "struct view['a,'unused] {value: &String <- 'a;}",
            "'unused",
            "not used",
        ),
        (
            "variant view['a,'a] {ready {ref string value <- 'a},none}",
            "'a",
            "duplicate",
        ),
        (
            "variant view['a,'unused] {ready {ref string value <- 'a},none}",
            "'unused",
            "not used",
        ),
        (
            "outcome view['a,'a] {ready {ref string value <- 'a},none}",
            "'a",
            "duplicate",
        ),
        (
            "outcome view['a,'unused] {ready {ref string value <- 'a},none}",
            "'unused",
            "not used",
        ),
        (
            "struct view['a,'unused,'later] {ref string value <- 'a;}",
            "'unused",
            "not used",
        ),
    ] {
        let source = format!("{declaration}\ncontrol main() exits done {{exit done;}}");
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_ORIGIN", "{source}: {report}");
        assert!(
            error["message"].as_str().unwrap().contains(message),
            "{report}"
        );
        let header = &source[..source.find('{').unwrap()];
        let at = header.rfind(selected).unwrap();
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+selected.len())}),
            "{source}: {report}"
        );
    }
}

#[test]
fn schema_group_ranges_survive_transitive_imports_and_unused_generic_definitions() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    f.write(
        "library.tellur",
        "import \"types.tellur\" as types;pub control work() exits done {exit done;}",
    );
    for (declaration, selected) in [
        ("pub struct view<t>['a,'a] {pub ref t value <- 'a;}", "'a"),
        (
            "pub struct view<t>['a,'unused] {pub ref t value <- 'a;}",
            "'unused",
        ),
        (
            "pub variant view<t>['a,' // 😀\r\n a] {ready {ref t value <- 'a},none}",
            "' // 😀\r\n a",
        ),
        (
            "pub variant view<t>['a,'unused] {ready {ref t value <- 'a},none}",
            "'unused",
        ),
        (
            "pub outcome view<t>['a,'a] {ready {ref t value <- 'a},none}",
            "'a",
        ),
        (
            "pub outcome view<t>['a,' // 😀\r\n unused] {ready {ref t value <- 'a},none}",
            "' // 😀\r\n unused",
        ),
    ] {
        let source = format!("// original 😀\r\n{declaration}");
        f.write("types.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_ORIGIN", "{report}");
        assert!(error["path"].as_str().unwrap().ends_with("types.tellur"));
        let at = source[..source.find('{').unwrap()].rfind(selected).unwrap();
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+selected.len())}),
            "{report}"
        );
    }
}

#[test]
fn schema_group_ranges_preserve_field_priority_and_global_schema_limits() {
    let f = Fixture::new();
    for (source, selected, message, count) in [
        (
            "struct view['a,'a] {ref string value <- 'missing;} control main() exits done {u32 wrong=true;exit done;}",
            "'a]",
            "duplicate",
            1,
        ),
        (
            "struct view['a,'unused] {ref string value <- 'missing;} control main() exits done {u32 wrong=true;exit done;}",
            "<- 'missing",
            "unknown",
            1,
        ),
        (
            "struct view['a,'unused] {ref string value <- 'a;} struct second['b,'b] {ref string value <- 'b;} control main() exits done {u32 wrong=true;exit done;}",
            "'unused",
            "not used",
            2,
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), count, "{errors}");
        assert!(errors.errors[0].message.contains(message));
        let first = compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first, errors.errors[0]);
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let at = source.find(selected).unwrap();
        let len = selected.trim_end_matches(']').len();
        assert_eq!(
            report["diagnostics"][0]["range"],
            json!({"start":scalar_position(source,at),"end":scalar_position(source,at+len)}),
            "{report}"
        );
    }
    let groups = (0..257)
        .map(|n| format!("'g{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "struct view[{groups}] {{ref string value <- 'g0;}} control main() exits done {{exit done;}}"
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.contains("exceeds 256"));
    assert!(errors.errors[0].end.is_none());
}

#[test]
fn schema_recovery_collects_independent_failures_without_body_errors() {
    let source = "struct first['a,'a] {ref string value <- 'a;}\nstruct second['b,'unused] {ref string value <- 'b;}\noutcome third['c] {ready {ref string value <- 'missing}}\ncontrol main() exits done {u32 wrong=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 3, "{errors}");
    assert!(!errors.limit_reached);
    assert!(
        errors
            .errors
            .iter()
            .all(|e| e.code == "E_ORIGIN" && e.end.is_some())
    );
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn schema_recovery_suppresses_transitive_and_diamond_dependents() {
    for generic in [false, true] {
        let source = if generic {
            "struct root<t>['a] {option<middle<t>>['a] value;}\nstruct middle<t>['a] {leaf<t>['a] value;}\nstruct leaf<t>['a,'a] {ref t value <- 'a;}\nstruct sibling<t>['a] {ref leaf<t>['a] value <- 'a;}\nstruct independent['b,'unused] {ref string value <- 'b;}\ncontrol main() exits done {exit done;}"
        } else {
            "struct root['a] {middle['a] value;}\nstruct middle['a] {leaf['a] value;}\nstruct leaf['a,'a] {ref string value <- 'a;}\nstruct sibling['a] {option<leaf>['a] value;}\nstruct independent['b,'unused] {ref string value <- 'b;}\ncontrol main() exits done {exit done;}"
        };
        for traits in [
            "",
            "trait marker {control inspect(ref self input) exits done;}",
        ] {
            let source = format!("{source}\n{traits}");
            let errors = check_errors(&source);
            assert_eq!(errors.errors.len(), 2, "{source}: {errors}");
            assert!(errors.errors[0].message.contains("duplicate"));
            assert_eq!(errors.errors[0].span.line, 3);
            assert!(errors.errors[1].message.contains("not used"));
            assert_eq!(errors.errors[1].span.line, 5);
        }
    }
}

#[test]
fn schema_recovery_retains_valid_shared_children_and_never_publishes_partial_schemas() {
    let source = "struct leaf['a] {ref string value <- 'a;}\nstruct first['a,'unused] {leaf['a] value;}\nstruct valid['a] {leaf['a] value;}\noutcome second['b,'b] {ready {ref string value <- 'b}}\ncontrol main() exits done {exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        [2, 4]
    );
    let repaired = source
        .replace("first['a,'unused]", "first['a]")
        .replace("second['b,'b]", "second['b]");
    compile(
        &repaired,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let errors = check_errors(&source.replace("exit done;", "u32 wrong=true;exit done;"));
    assert_eq!(errors.errors.len(), 2);
    assert!(errors.errors.iter().all(|e| e.code == "E_ORIGIN"));
}

#[test]
fn schema_recovery_preserves_cycles_limits_and_earlier_phase_barriers() {
    let independent =
        "struct other['b,'b] {ref string value <- 'b;} control main() exits done {exit done;}";
    let cycle = format!("struct cycle['a] {{cycle['a] value;}} {independent}");
    let errors = check_errors(&cycle);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors[0].message.contains("cyclic"));
    let groups = (0..257)
        .map(|n| format!("'g{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("struct huge[{groups}] {{ref string value <- 'g0;}} {independent}");
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors[0].message.contains("exceeds 256"));
    assert!(errors.errors[0].end.is_none());
    for (prefix, code) in [
        (
            "pub control wrong() exits auto {exit done;}",
            "E_PUBLIC_CONTRACT",
        ),
        (
            "control wrong<t: missing>() exits done {exit done;}",
            "E_BOUND",
        ),
    ] {
        let errors = check_errors(&format!("{prefix}{source}"));
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code);
    }
}

#[test]
fn schema_recovery_uses_the_shared_budget_and_preserves_first_error() {
    for count in [31, 32, 40] {
        let mut source = (0..count)
            .map(|n| format!("struct bad_{n}['a,'a] {{ref string value <- 'a;}}\n"))
            .collect::<String>();
        source.push_str("control main() exits done {u32 wrong=true;exit done;}");
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), count.min(32));
        assert_eq!(errors.limit_reached, count >= 32);
        assert!(errors.errors.iter().all(|e| e.code == "E_ORIGIN"));
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
}

#[test]
fn schema_recovery_cli_maps_independent_imports_to_original_ranges() {
    let f = Fixture::new();
    f.write("main.tellur", "import \"first.tellur\" as first;import \"second.tellur\" as second;control main() exits done {u32 wrong=true;exit done;}");
    let first = "// 😀\r\npub struct first<t>['a,' // 😀\r\n a] {pub ref t value <- 'a;}";
    let second = "pub outcome second<t>['b,'unused] {ready {ref t value <- 'b}}";
    f.write("first.tellur", first);
    f.write("second.tellur", second);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (error, (path, source, selected)) in errors.iter().zip([
        ("first.tellur", first, "' // 😀\r\n a"),
        ("second.tellur", second, "'unused"),
    ]) {
        assert_eq!(error["code"], "E_ORIGIN");
        assert!(error["path"].as_str().unwrap().ends_with(path));
        assert_eq!(error["range"], selected_range(source, selected));
    }
}

#[test]
fn schema_recovery_keeps_ambiguous_identities_and_no_origin_layouts_separate() {
    // Identity validation has not run yet. Do not claim independence for aliases
    // that may refer to either same-named declaration.
    let source = "struct same['a,'a] {ref string value <- 'a;} struct same['b,'b] {ref string value <- 'b;} struct other['c,'c] {ref string value <- 'c;} control main() exits done {exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert!(errors.errors[0].message.contains("duplicate origin"));
    let source = "struct recursive {recursive value;} control main() exits done {exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_ne!(errors.errors[0].code, "E_ORIGIN");
}

#[test]
fn schema_recovery_bounds_deep_dependencies_without_repeating_the_failed_chain() {
    let mut source = (0..66)
        .map(|n| format!("struct node_{n}['a] {{node_{}['a] value;}}\n", n + 1))
        .collect::<String>();
    source.push_str("struct node_66['a] {ref string value <- 'a;}\nstruct separate['b,'b] {ref string value <- 'b;}\ncontrol main() exits done {exit done;}");
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors[0].message.contains("excessively deep"));
    assert!(errors.errors[0].end.is_none());
    assert!(errors.errors[1].message.contains("duplicate"));
}

#[test]
fn contributor_ranges_select_producing_and_forwarding_occurrences() {
    let f = Fixture::new();
    let source = "control leaf() exits auto {\r\n    exit // 😀\r\n        added; // outside\r\n}\r\ncontrol middle() exits auto {\r\n    forward // 😀\r\n        leaf(); // outside\r\n}\r\ncontrol main() exits done {delta middle() {}}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let notes = report["diagnostics"][0]["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2, "{report}");
    for (note, selected) in notes.iter().zip([
        "exit // 😀\r\n        added;",
        "forward // 😀\r\n        leaf();",
    ]) {
        assert_eq!(note["range"], selected_range(source, selected));
        assert_eq!(note["position"], note["range"]["start"]);
    }
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].notes.iter().all(|n| n.end.is_some()));
    let first = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first, errors.errors[0]);
}

#[test]
fn contributor_ranges_follow_transitive_imports_and_generic_definition_spelling() {
    let f = Fixture::new();
    let leaf = "pub control leaf<t>(t input) exits added {t value} {\r\n    exit // 😀\r\n        added {input};\r\n}";
    let middle = "import \"leaf.tellur\" as leaf;\ncontrol middle() exits auto {\n    forward leaf.leaf<string>(\"x\");\n}\npub control work() exits done {delta middle() {}}";
    f.write("leaf.tellur", leaf);
    f.write("middle.tellur", middle);
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle; control main() exits done {middle.work();exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let notes = report["diagnostics"][0]["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2, "{report}");
    for (note, (file, source, selected)) in notes.iter().zip([
        ("leaf.tellur", leaf, "exit // 😀\r\n        added {input};"),
        ("middle.tellur", middle, "forward leaf.leaf<string>(\"x\");"),
    ]) {
        assert!(note["path"].as_str().unwrap().ends_with(file));
        assert_eq!(note["range"], selected_range(source, selected), "{report}");
    }
}

#[test]
fn contributor_ranges_preserve_declared_contract_and_omission_bounds() {
    let mut source = "control f0() exits auto {exit changed;}\n".to_owned();
    for i in 1..45 {
        source.push_str(&format!(
            "control f{i}() exits auto {{forward f{}();}}\n",
            i - 1
        ));
    }
    source.push_str("control main() exits done {delta f44() {}}");
    let errors = check_errors(&source);
    let notes = &errors.errors[0].notes;
    assert_eq!(notes.len(), 33);
    assert!(notes[..32].iter().all(|n| n.end.is_some()));
    assert!(notes[32].message.contains("omitted"));
    assert!(notes[32].end.is_none());
    let errors = check_errors(
        "control leaf() exits first,second {exit first;} control main() exits done {delta leaf() {first=>exit done}}",
    );
    let note = errors.errors[0]
        .notes
        .iter()
        .find(|n| n.message.contains("declared"))
        .unwrap();
    assert_eq!(note.span.column, 16);
    assert_eq!(note.end.as_ref().unwrap().column, 34);
}

#[test]
fn contributor_ranges_select_nested_outcome_expressions_without_surrounding_statements() {
    let f = Fixture::new();
    let source = "control leaf() exits auto {exit first;} control main() exits done {auto saved=capture leaf();auto mapped=delta saved {first=>second // 😀\r\n {}};exit delta mapped {}; }";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let notes = report["diagnostics"][0]["notes"].as_array().unwrap();
    let note = notes
        .iter()
        .find(|n| {
            n["message"]
                .as_str()
                .unwrap()
                .contains("`second` is constructed")
        })
        .unwrap();
    assert_eq!(
        note["range"],
        selected_range(source, "second // 😀\r\n {}"),
        "{report}"
    );
    let received = notes
        .iter()
        .find(|n| n["message"].as_str().unwrap().contains("received"))
        .unwrap();
    assert_eq!(
        received["range"],
        selected_range(source, "auto saved=capture leaf();"),
        "{report}"
    );
}

#[test]
fn public_shape_ranges_select_written_inference_clauses_in_authored_order() {
    let f = Fixture::new();
    for (source, selected) in [
        ("pub control one() exits auto {exit done;}", "exits auto"),
        ("pub region one() exits auto {exit done;}", "exits auto"),
        (
            "pub async control one() exits auto {exit done;}",
            "exits auto",
        ),
        (
            "pub control one<t>(t input) exits // 😀\r\n auto {exit ready {input};}",
            "exits // 😀\r\n auto",
        ),
        (
            "pub control one() exits ready // 😀\r\n ... {exit ready {1};}",
            "ready // 😀\r\n ...",
        ),
        (
            "pub control one() exits {plain, zebra..., alpha...} {exit plain;}",
            "zebra...",
        ),
        (
            "pub control one() exits plain {u32 value}, zebra..., alpha... {exit plain {1};}",
            "zebra...",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        assert_eq!(errors[0]["code"], "E_PUBLIC_CONTRACT");
        assert_eq!(
            errors[0]["range"],
            selected_range(source, selected),
            "{report}"
        );
        let errors = check_errors(source);
        assert_eq!(
            compile(
                source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
}

#[test]
fn public_shape_ranges_preserve_transitive_imports_and_independent_repairs() {
    let f = Fixture::new();
    let first =
        "// 😀\r\npub control first<t>(t input) exits // clause\r\n auto {exit ready {input};}";
    let second = "import \"first.tellur\" as first;\npub control second<t>(t input) exits empty, ready // 😀\r\n ... {exit ready {input};}";
    f.write("first.tellur", first);
    f.write("second.tellur", second);
    f.write(
        "main.tellur",
        "import \"second.tellur\" as second; control main() exits done {u32 wrong=true;exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (path, source, selected) in [
        ("first.tellur", first, "exits // clause\r\n auto"),
        ("second.tellur", second, "ready // 😀\r\n ..."),
    ] {
        let error = errors
            .iter()
            .find(|error| error["path"].as_str().unwrap().ends_with(path))
            .unwrap();
        assert_eq!(error["code"], "E_PUBLIC_CONTRACT");
        assert!(error["path"].as_str().unwrap().ends_with(path));
        assert_eq!(error["range"], selected_range(source, selected), "{report}");
    }
    f.write(
        "first.tellur",
        &first.replace("exits // clause\r\n auto", "exits ready {t value}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert!(
        report["diagnostics"][0]["path"]
            .as_str()
            .unwrap()
            .ends_with("second.tellur")
    );
    f.write(
        "second.tellur",
        &second.replace("ready // 😀\r\n ...", "ready {t value}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_TYPE");
}

#[test]
fn public_shape_ranges_keep_phase_priority_budget_and_private_inference() {
    let mut source = (0..40)
        .map(|n| format!("pub control bad_{n}() exits auto {{exit done;}}\n"))
        .collect::<String>();
    source.push_str("struct bad['a,'a] {ref string value <- 'a;} control main() exits done {u32 wrong=true;exit done;}");
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32);
    assert!(errors.limit_reached);
    assert!(
        errors
            .errors
            .iter()
            .all(|e| e.code == "E_PUBLIC_CONTRACT" && e.end.is_some())
    );
    for source in [
        "control one() exits auto {exit done;}",
        "control one() exits ready... {exit ready {1};}",
        "control one(bool flag) exits plain {u32 value}, ready... {delta flag {true=>exit plain {1},false=>exit ready {2}}}",
    ] {
        compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap();
    }
    for source in [
        "pub region old() exits ready... {exit ready(1);}",
        "trait reader {control read() exits ready...;}",
        "trait reader {control read() exits auto;}",
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1);
        assert_ne!(errors.errors[0].code, "E_PUBLIC_CONTRACT");
    }
}

#[test]
fn missing_public_origin_ranges_select_result_types_and_named_completion_uses() {
    let f = Fixture::new();
    for (source, selected, message) in [
        (
            "pub control one(ref string input) exits ready {ref // 😀\r\n string value} {exit ready {input};}",
            "ref // 😀\r\n string",
            "borrowed result",
        ),
        (
            "pub control one(ref mutable u32 input) exits ready {ref mutable u32 value} {exit ready {input};}",
            "ref mutable u32",
            "borrowed result",
        ),
        (
            "pub control one<t>(ref t input) exits ready {ref // result\n t value} {exit ready {input};}",
            "ref // result\n t",
            "borrowed result",
        ),
        (
            "pub struct view['a] {ref string text <- 'a;} pub control one(ref string input) exits ready {view value} {exit ready {view {input}};}",
            "view value",
            "aggregate result",
        ),
        (
            "pub struct view['a] {ref string text <- 'a;} pub control one(ref view input) exits ready {ref // result\n view value from input} {exit ready {input};}",
            "ref // result\n view",
            "aggregate result",
        ),
        (
            "pub outcome reply<t>['a] {ready {ref t value <- 'a}} pub control one<t>(ref t input) exits reply // 😀\r\n <t> {exit ready {input};}",
            "reply // 😀\r\n <t>",
            "borrowed result",
        ),
        (
            "pub struct view['a] {ref string text <- 'a;} pub outcome reply['a] {ready {view['a] value}} pub control one(ref string input) exits reply {exit ready {view {input}};}",
            "reply {exit",
            "aggregate result",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        assert_eq!(errors[0]["code"], "E_PUBLIC_CONTRACT", "{report}");
        assert!(
            errors[0]["message"].as_str().unwrap().contains(message),
            "{report}"
        );
        // Use a unique context to distinguish result and use sites from inputs/schema names.
        let expected = if selected == "view value" || selected == "reply {exit" {
            let at = source.find(selected).unwrap();
            let len = if selected == "view value" { 4 } else { 5 };
            json!({"start":scalar_position(source,at),"end":scalar_position(source,at+len)})
        } else if selected == "ref mutable u32" {
            let at = source.rfind(selected).unwrap();
            json!({"start":scalar_position(source,at),"end":scalar_position(source,at+selected.len())})
        } else {
            selected_range(source, selected)
        };
        assert_eq!(errors[0]["range"], expected, "{report}");
        let errors = check_errors(source);
        assert_eq!(
            compile(
                source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
}

#[test]
fn missing_public_origin_ranges_keep_transitive_schema_and_use_files_distinct() {
    let f = Fixture::new();
    f.write(
        "schema.tellur",
        "pub outcome reply<t>['a] {ready {ref t value <- 'a}}",
    );
    let worker = "import \"schema.tellur\" as schema;\r\npub control one(ref string input) exits schema.reply // 😀\r\n <string> {exit ready {input};}\r\npub control two(ref string input) exits ready {ref // result\r\n string value} {exit ready {input};}";
    f.write("worker.tellur", worker);
    f.write(
        "main.tellur",
        "import \"worker.tellur\" as worker; control main() exits done {u32 wrong=true;exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (error, selected) in errors.iter().zip([
        "schema.reply // 😀\r\n <string>",
        "ref // result\r\n string",
    ]) {
        assert_eq!(error["code"], "E_PUBLIC_CONTRACT");
        assert!(error["path"].as_str().unwrap().ends_with("worker.tellur"));
        assert_eq!(error["range"], selected_range(worker, selected));
    }
    let partial = worker.replace("<string> {exit", "<string>[input] {exit");
    f.write("worker.tellur", &partial);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(&partial, "ref // result\r\n string")
    );
    f.write(
        "worker.tellur",
        &partial.replace("string value}", "string value from input}"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_TYPE");
}

#[test]
fn missing_public_origin_ranges_preserve_first_result_and_preparation_barriers() {
    let source = "pub control one(ref string input) exits ready {ref string first, ref string second} {exit ready {input,input};}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.contains("`first`"));
    let repaired = source.replace("first,", "first from input,");
    assert!(
        check_errors(&repaired).errors[0]
            .message
            .contains("`second`")
    );
    let mut many = (0..40).map(|n| format!("pub control bad_{n}(ref string input) exits ready {{ref string value}} {{exit ready {{input}};}}\n")).collect::<String>();
    let errors = check_errors(&many);
    assert_eq!(errors.errors.len(), 32);
    assert!(errors.limit_reached);
    assert!(
        errors
            .errors
            .iter()
            .all(|e| e.code == "E_PUBLIC_CONTRACT" && e.end.is_some())
    );
    many.push_str("struct bad['a,'a] {ref string value <- 'a;}");
    assert_eq!(check_errors(&many).errors[0].code, "E_ORIGIN");
    many.push_str("pub control shape() exits auto {exit done;}");
    assert_eq!(check_errors(&many).errors[0].code, "E_PUBLIC_CONTRACT");
    assert!(
        check_errors(&many).errors[0]
            .message
            .contains("explicit exit labels")
    );
    // Written invalid bindings retain their existing bracket/from selection and code.
    let f = Fixture::new();
    for (source, selected) in [
        (
            "pub control one(ref string input) exits ready {ref string value from absent} {exit ready {input};}",
            "from absent",
        ),
        (
            "pub outcome reply['a] {ready {ref string value <- 'a}} pub control one(ref string input) exits reply[input,input] {exit ready {input};}",
            "[input,input]",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_ne!(report["diagnostics"][0]["code"], "E_PUBLIC_CONTRACT");
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(source, selected),
            "{report}"
        );
    }
}

#[test]
fn declared_contributor_ranges_select_complete_written_contracts() {
    let f = Fixture::new();
    for (definition, selected, call) in [
        (
            "control leaf() exits first, second {exit first;}",
            "exits first, second",
            "leaf()",
        ),
        (
            "region leaf() exits {first, second} {exit first;}",
            "exits {first, second}",
            "leaf()",
        ),
        (
            "control leaf<t>(t input) exits first {t value}, second {exit first {input};}",
            "exits first {t value}, second",
            "leaf<u32>(1)",
        ),
        (
            "outcome reply {first,second} control leaf() exits // 😀\r\n reply {exit first;}",
            "exits // 😀\r\n reply",
            "leaf()",
        ),
        (
            "control leaf() exits first, second faults {exit first;}",
            "exits first, second",
            "leaf()",
        ),
    ] {
        let source = format!("{definition} control main() exits done {{delta {call} {{}}}}");
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(report["diagnostics"][0]["code"], "E_UNCOVERED", "{report}");
        let notes = report["diagnostics"][0]["notes"].as_array().unwrap();
        let declared = notes
            .iter()
            .filter(|n| n["message"].as_str().unwrap().contains("declared"))
            .collect::<Vec<_>>();
        assert!(!declared.is_empty(), "{report}");
        for note in declared {
            assert_eq!(note["range"], selected_range(&source, selected), "{report}");
        }
        // Produced exits still select the body occurrence, never the header.
        for note in notes
            .iter()
            .filter(|n| n["message"].as_str().unwrap().contains("produced"))
        {
            assert_ne!(note["range"], selected_range(&source, selected));
        }
    }
}

#[test]
fn declared_contributor_ranges_keep_named_schema_and_transitive_generic_use_sites_distinct() {
    let f = Fixture::new();
    f.write(
        "schema.tellur",
        "pub outcome reply<t> {first {t value},second}",
    );
    let worker = "import \"schema.tellur\" as schema;\r\npub control leaf<t>(t input) exits // 😀\r\n schema.reply<t> {exit first {input};}";
    f.write("worker.tellur", worker);
    f.write("main.tellur", "import \"worker.tellur\" as worker;control main() exits done {delta worker.leaf<u32>(1) {first {u32 value}=>exit done}}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_UNCOVERED", "{report}");
    let note = report["diagnostics"][0]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["message"].as_str().unwrap().contains("declared"))
        .unwrap();
    assert!(note["path"].as_str().unwrap().ends_with("worker.tellur"));
    assert_eq!(
        note["range"],
        selected_range(worker, "exits // 😀\r\n schema.reply<t>")
    );
}

#[test]
fn declared_contributor_ranges_keep_shared_header_notes_distinct_and_bounded() {
    let labels = (0..40)
        .map(|n| format!("label_{n:02}"))
        .collect::<Vec<_>>()
        .join(",");
    let clause = format!("exits {labels}");
    let source = format!(
        "control leaf() {clause} {{exit label_00;}} control main() exits done {{delta leaf() {{}}}}"
    );
    let f = Fixture::new();
    f.write("main.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let notes = report["diagnostics"][0]["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 33, "{report}");
    assert!(notes[32]["message"].as_str().unwrap().contains("omitted"));
    assert!(notes[32]["range"].is_null());
    let declared = notes[..32]
        .iter()
        .filter(|n| n["message"].as_str().unwrap().contains("declared"))
        .collect::<Vec<_>>();
    assert!(declared.len() >= 30, "{report}");
    let unique = declared
        .iter()
        .map(|n| n["message"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), declared.len());
    for note in declared {
        assert_eq!(note["range"], selected_range(&source, &clause));
    }
    let errors = check_errors(&source);
    assert_eq!(
        compile(
            &source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn implementation_preparation_batches_report_one_error_per_unused_implementation() {
    let source = "trait reader {type item;control read(self input) exits done;}\nimplement reader for u32 {type item=u32;}\nimplement reader for string {type item=string;type item=string;control read(self input) exits done {exit done;}}\nimplement reader for bool {type item=bool;control read<t>(self input) exits done {exit done;}}\nimplement reader for bytes {type item=bytes;control read(self input) exits auto {exit done;}}\ncontrol main() exits done {u32 wrong=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(
        errors.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [
            "E_TRAIT",
            "E_TRAIT",
            "E_TRAIT_SIGNATURE",
            "E_TRAIT_SIGNATURE"
        ],
        "{errors}"
    );
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| e.span.line)
            .collect::<Vec<_>>(),
        [2, 3, 4, 5]
    );
    assert!(!errors.limit_reached);
    assert_eq!(
        compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn implementation_preparation_batches_replay_repairs_before_associated_and_body_checks() {
    let missing = "trait reader {control read(self input) exits done;}\nimplement reader for u32 {}\nimplement reader for string {}";
    assert_eq!(check_errors(missing).errors.len(), 2);
    let partial = missing.replace("implement reader for u32 {}", "implement reader for u32 {control read(self input) exits done {u32 wrong=true;exit done;}}");
    let errors = check_errors(&partial);
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].span.line, 3);
    assert!(errors.errors[0].message.contains("supply exactly"));
    let repaired = partial.replace(
        "implement reader for string {}",
        "implement reader for string {control read(self input) exits done {exit done;}}",
    );
    assert_eq!(check_errors(&repaired).errors[0].code, "E_TYPE");
    compile(
        &repaired.replace("u32 wrong=true;", ""),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let source = "trait source {type item;} implement source for u32 {type item=source<u32>.item;} implement source for string {}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.contains("supply exactly"));
    let errors = check_errors(&source.replace("for string {}", "for string {type item=string;}"));
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.contains("cycl"), "{errors}");
}

#[test]
fn implementation_preparation_batches_preserve_ambiguous_identity_first_error_order() {
    for (source, expected) in [
        (
            "trait reader {control read(self input) exits done;} implement reader for u32 {} implement reader for u32 {} implement reader for string {}",
            "supply exactly",
        ),
        (
            "trait reader {control read(self input) exits done;} implement reader for u32 {control read(self input) exits done {exit done;}} implement reader for u32 {control read(self input) exits done {exit done;}} implement reader for string {}",
            "duplicate implementation",
        ),
        (
            "trait reader {control read(self input) exits done;} implement reader for u32 {} implement absent for string {} implement reader for bool {}",
            "supply exactly",
        ),
        (
            "trait reader {control read(self input) exits done;} implement absent for u32 {} implement reader for string {} implement reader for bool {}",
            "unknown trait",
        ),
        (
            "trait reader {control read(self input) exits done;} implement reader for ref u32 {} implement reader for string {}",
            "concrete owned types",
        ),
        (
            "trait reader {control read(self input) exits done;} implement reader for unknown {} implement reader for string {}",
            "unknown or unapplied type",
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert!(!errors.limit_reached);
        assert!(errors.errors[0].message.contains(expected), "{errors}");
        assert_eq!(
            compile(
                source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
}

#[test]
fn implementation_preparation_batches_preserve_phase_barriers_and_budgets() {
    let mut source = "trait reader {control read(self input) exits done;}\n".to_owned();
    for n in 0..40 {
        source.push_str(&format!(
            "struct item_{n} {{u32 value;}} implement reader for item_{n} {{}}\n"
        ));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32);
    assert!(errors.limit_reached);
    assert!(
        errors
            .errors
            .iter()
            .all(|e| e.code == "E_TRAIT" && e.message.contains("supply exactly"))
    );
    let errors = check_errors(&format!(
        "struct invalid['a,'a] {{ref string value <- 'a;}} {source}"
    ));
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].code, "E_ORIGIN");
    let errors = check_errors(&format!(
        "trait invalid {{control read() exits auto;}} {source}"
    ));
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].code, "E_TRAIT_SIGNATURE");
    let source = format!(
        "trait marker {{}} {}",
        "implement marker for u32 {}\n".repeat(1025)
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 1);
    assert!(!errors.limit_reached);
    assert!(errors.errors[0].message.contains("1024-implementation"));
    // Conformance still waits for every local implementation header to prepare.
    let source = "trait reader {control read(self input) exits done;} implement reader for u32 {control read(ref self input) exits done {exit done;}} implement reader for string {}";
    assert!(
        check_errors(source).errors[0]
            .message
            .contains("supply exactly")
    );
    let repaired = source.replace(
        "for string {}",
        "for string {control read(self input) exits done {exit done;}}",
    );
    assert_eq!(check_errors(&repaired).errors[0].code, "E_TRAIT_SIGNATURE");
}

#[test]
fn implementation_preparation_batches_preserve_imported_origins_and_coherence_barriers() {
    let f = Fixture::new();
    f.write(
        "contract.tellur",
        "pub trait reader {control read(self input) exits done;}",
    );
    for file in ["first.tellur", "second.tellur"] {
        f.write(file,"import \"contract.tellur\" as contract;\r\nstruct local {u32 value;}\r\nimplement contract.reader for local {}\r\n");
    }
    f.write("main.tellur", "import \"first.tellur\" as first; import \"second.tellur\" as second;control main() exits done {exit done;}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for (error, file) in errors.iter().zip(["first.tellur", "second.tellur"]) {
        assert_eq!(error["code"], "E_TRAIT");
        assert!(error["path"].as_str().unwrap().ends_with(file));
        assert_eq!(error["position"], json!({"line":3,"column":1}));
    }
    // A foreign target disables recovery without preempting the earlier local error.
    f.write(
        "second.tellur",
        "import \"contract.tellur\" as contract;implement contract.reader for u32 {}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_TRAIT");
    f.write("first.tellur", "import \"contract.tellur\" as contract;struct local {u32 value;}implement contract.reader for local {control read(self input) exits done {exit done;}}");
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_COHERENCE");
}

#[test]
fn associated_normalization_batches_preserve_canonical_root_order_and_first_error() {
    let source = "trait source {type item;}\nimplement source for u32 {type item=ref string;}\nimplement source for string {type item=ref u32;}\ncontrol main() exits done {u32 wrong=true;exit done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line, e.span.column))
            .collect::<Vec<_>>(),
        [("E_LIFETIME", 3, 40), ("E_LIFETIME", 2, 37)]
    );
    assert!(!errors.limit_reached);
    assert_eq!(
        compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn associated_normalization_batches_suppress_lexical_diamonds_and_replay_repairs() {
    let source = "trait source {type a;type b;type c;type d;}\nimplement source for u32 {\ntype a=b;\ntype b=ref string;\ntype c=vector<b>;\ntype d=ref u32;\n}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        [("E_LIFETIME", 3), ("E_LIFETIME", 6)],
        "{errors}"
    );
    let partial = source.replace("b=ref string", "b=string");
    let errors = check_errors(&partial);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].span.line, 6);
    let fixed = format!(
        "{} control main() exits auto {{source<u32>.a a=\"x\";source<u32>.c c=[\"y\"];exit done {{a,c}};}}",
        partial.replace("d=ref u32", "d=u32")
    );
    let p = compile(
        &fixed,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let scenario = tellur::runtime::Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&scenario).unwrap(),
        p.expanded("main").unwrap().run(&scenario).unwrap()
    );
}

#[test]
fn associated_normalization_batches_retire_cross_implementation_cycles() {
    let source = "trait source {type a;type b;}\nimplement source for u32 {\ntype a=source<string>.a;\ntype b=ref u32;\n}\nimplement source for string {\ntype a=source<u32>.a;\ntype b=ref string;\n}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        [("E_TRAIT", 7), ("E_LIFETIME", 8), ("E_LIFETIME", 4)],
        "{errors}"
    );
    assert!(errors.errors[0].message.contains("cyclic"));
    // Two roots sharing one missing qualified selection report its first failure once.
    let source = "trait source {type a;type b;type c;type d;} implement source for u32 {type a=source<u32>.missing;type b=source<u32>.missing;type c=source<string>.a;type d=ref u32;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 3, "{errors}");
    assert!(errors.errors[0].message.contains("unknown associated type"));
    assert!(errors.errors[1].message.contains("no implementation"));
    assert_eq!(errors.errors[2].code, "E_LIFETIME");
}

#[test]
fn associated_normalization_batches_follow_normalized_selection_targets() {
    let source = "trait lookup {type item;} implement lookup for string {type item=string;}\ntrait source {type value;type target;type other;}\nimplement source for u32 {\ntype value=lookup<target>.item;\ntype target=ref string;\ntype other=ref bool;\n}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        [("E_LIFETIME", 4), ("E_LIFETIME", 6)],
        "{errors}"
    );
    let repaired = source
        .replace("target=ref string", "target=string")
        .replace("other=ref bool", "other=bool");
    compile(&format!("{repaired} control main() exits done {{source<u32>.value text=\"valid\";drop(text);exit done;}}"),Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()).unwrap();
    // A target can normalize successfully while the selected value itself fails.
    let errors = check_errors(
        &repaired
            .replace("item=string", "item=ref string")
            .replace("other=bool", "other=ref bool"),
    );
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_LIFETIME"));
}

#[test]
fn associated_normalization_batches_keep_depth_type_size_and_diagnostic_budgets() {
    let mut declaration = String::new();
    let mut assignments = String::new();
    for n in 0..70 {
        declaration.push_str(&format!("type a{n};"));
        assignments.push_str(&format!(
            "type a{n}={};",
            if n == 69 {
                "u32".into()
            } else {
                format!("a{}", n + 1)
            }
        ));
    }
    let source = format!(
        "trait source {{{declaration}type independent;}} implement source for u32 {{{assignments}type independent=ref u32;}}"
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 3, "{errors}");
    assert!(
        errors.errors[0].message.contains("deep") || errors.errors[0].message.contains("depth")
    );
    assert!(errors.errors[1].message.contains("depth"));
    assert_eq!(errors.errors[2].code, "E_LIFETIME");
    let source = format!(
        "trait source {{type a;type b;}} implement source for u32 {{type a={};type b=ref u32;}}",
        "x".repeat(16385)
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors[0].message.contains("type-size budget"));
    assert_eq!(errors.errors[1].code, "E_LIFETIME");
    let mut source = "trait source {type item;}\n".to_owned();
    for n in 0..40 {
        source.push_str(&format!("struct value_{n} {{u32 value;}} implement source for value_{n} {{type item=ref u32;}}\n"));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32, "{errors}");
    assert!(errors.limit_reached);
    assert!(errors.errors.iter().all(|e| e.code == "E_LIFETIME"));
}

#[test]
fn associated_normalization_batches_keep_preparation_and_conformance_barriers() {
    let source = "trait source {type item:copy;control read(self input) exits done;} implement source for u32 {type item=string;control read(ref self input) exits done {exit done;}} implement source for string {type item=ref string;control read(self input) exits done {exit done;}}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_LIFETIME");
    let errors = check_errors(&source.replace("item=ref string", "item=u32"));
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_BOUND");
    let errors = check_errors(
        &source
            .replace("item=ref string", "item=u32")
            .replace("item=string", "item=u32"),
    );
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_TRAIT_SIGNATURE");
    for (prefix, code) in [
        (
            "struct invalid['a,'a] {ref string value <- 'a;} ",
            "E_ORIGIN",
        ),
        (
            "trait invalid {control read() exits auto;} ",
            "E_TRAIT_SIGNATURE",
        ),
        (
            "trait missing {type item;} implement missing for bool {} ",
            "E_TRAIT",
        ),
        ("implement absent for bool {} ", "E_TRAIT"),
    ] {
        let errors = check_errors(&format!("{prefix}{source}"));
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code);
    }
}

#[test]
fn associated_normalization_batches_keep_transitive_imported_points_and_first_error() {
    let f = Fixture::new();
    for file in ["first.tellur", "second.tellur"] {
        f.write(file,"trait source {type item;}\r\n// 😀\r\nimplement source for u32 {type item=ref string;}\r\n");
    }
    f.write(
        "middle.tellur",
        "import \"first.tellur\" as first;import \"second.tellur\" as second;",
    );
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for error in errors {
        assert_eq!(error["code"], "E_LIFETIME");
        assert_eq!(error["position"], json!({"line":3,"column":37}));
    }
    let paths = errors
        .iter()
        .map(|e| {
            PathBuf::from(e["path"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        ["first.tellur".to_owned(), "second.tellur".to_owned()]
            .into_iter()
            .collect()
    );
    assert!(report["summary"].is_null());
    let first = tellur::compile_file(
        f.root.join("main.tellur"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        first.path.to_string_lossy(),
        errors[0]["path"].as_str().unwrap()
    );
    assert_eq!(first.diagnostic.span.line, 3);
    assert_eq!(first.diagnostic.code, "E_LIFETIME");
}

#[test]
fn implementation_permission_batches_preserve_canonical_order_and_method_priority() {
    let source = "trait a_async {control first(self input) exits done;control second() exits done;}\ntrait b_fault {control first(self input) exits done;control second() exits done;}\ntrait c_contingent {control first(self input) exits done;control second() exits done;}\nimplement c_contingent for u32 {@contingent control first(self input) exits done {exit done;} @contingent control second() exits done {exit done;}}\nimplement b_fault for u32 {control second() exits done faults {exit done;} async control first(self input) exits done {exit done;}}\nimplement a_async for u32 {async control first(self input) exits done faults {exit done;} control second() exits done faults {exit done;}}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        [("E_ASYNC", 6), ("E_FAULT_CONTRACT", 5), ("E_CONTINGENT", 4)],
        "{errors}"
    );
    assert!(!errors.limit_reached);
    assert_eq!(
        compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
    // Within the same implementation methods retain written order, not trait order;
    // within one method, async equality still precedes a fault permission error.
    let later = source.replace(
        "async control first(self input) exits done faults",
        "control first(self input) exits done faults",
    );
    assert_eq!(check_errors(&later).errors[0].code, "E_FAULT_CONTRACT");
}

#[test]
fn implementation_permission_batches_replay_before_bounds_conformance_and_bodies() {
    let source = "trait reader {type item:copy;control read(self input) exits done;}\nimplement reader for u32 {type item=string;async control read(ref self input) exits done {u32 wrong=true;exit done;}}\nimplement reader for string {type item=u32;control read(self input) exits done faults {exit done;}}";
    let errors = check_errors(source);
    assert_eq!(
        errors.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["E_FAULT_CONTRACT", "E_ASYNC"],
        "{errors}"
    );
    let partial = source.replace("async control read", "control read");
    let errors = check_errors(&partial);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_FAULT_CONTRACT");
    let repaired = partial.replace("done faults", "done");
    assert_eq!(check_errors(&repaired).errors[0].code, "E_BOUND");
    let repaired = repaired.replace("item=string", "item=u32");
    assert_eq!(check_errors(&repaired).errors[0].code, "E_TRAIT_SIGNATURE");
    let repaired = repaired.replace("ref self input", "self input");
    assert_eq!(check_errors(&repaired).errors[0].code, "E_TYPE");
    compile(
        &repaired.replace("u32 wrong=true;", ""),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn implementation_permission_batches_precede_implementation_origin_conformance() {
    let source = "trait reader {control read(ref self input) exits found {ref self value from input};} implement reader for u32 {async control read(ref self input) exits found {ref self value} {exit found {input};}} implement reader for string {async control read(ref self input) exits found {ref self value} {exit found {input};}}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_ASYNC"));
    let repaired = source.replace("async control read", "control read");
    let errors = check_errors(&repaired);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_LIFETIME"));
    compile(
        &repaired.replace(
            "exits found {ref self value}",
            "exits found {ref self value from input}",
        ),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn implementation_permission_batches_preserve_earlier_phases_and_budget() {
    let source = "trait reader {control read(self input) exits done;} implement reader for u32 {async control read(self input) exits done {exit done;}} implement reader for string {async control read(self input) exits done {exit done;}}";
    for (prefix, code) in [
        (
            "struct invalid['a,'a] {ref string value <- 'a;} ",
            "E_ORIGIN",
        ),
        (
            "trait invalid {control read() exits auto;} ",
            "E_TRAIT_SIGNATURE",
        ),
        (
            "trait missing {type item;} implement missing for bool {} ",
            "E_TRAIT",
        ),
        (
            "trait source {type item;} implement source for bool {type item=ref string;} ",
            "E_LIFETIME",
        ),
        ("implement absent for bool {} ", "E_TRAIT"),
    ] {
        let errors = check_errors(&format!("{prefix}{source}"));
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code);
    }
    let mut source = "trait reader {control read(self input) exits done;}\n".to_owned();
    for n in 0..40 {
        source.push_str(&format!("struct value_{n} {{u32 value;}} implement reader for value_{n} {{async control read(self input) exits done {{exit done;}}}}\n"));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32, "{errors}");
    assert!(errors.limit_reached);
    assert!(errors.errors.iter().all(|e| e.code == "E_ASYNC"));
}

#[test]
fn implementation_permission_batches_preserve_equality_subsets_and_generic_propagation() {
    for (declaration, implementation, expected) in [
        (
            "async control read(self input) exits done;",
            "control read(self input) exits done {exit done;}",
            "E_ASYNC",
        ),
        (
            "control read(self input) exits done;",
            "async control read(self input) exits done {exit done;}",
            "E_ASYNC",
        ),
        (
            "control read(self input) exits done;",
            "control read(self input) exits done faults {exit done;}",
            "E_FAULT_CONTRACT",
        ),
        (
            "control read(self input) exits done;",
            "@contingent control read(self input) exits done {exit done;}",
            "E_CONTINGENT",
        ),
    ] {
        let source = format!(
            "trait reader {{{declaration}}} implement reader for u32 {{{implementation}}} implement reader for string {{{implementation}}}"
        );
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 2, "{errors}");
        assert!(errors.errors.iter().all(|e| e.code == expected));
    }
    let source = "trait reader {async control read(self input) exits done faults;} implement reader for u32 {async control read(self input) exits done {exit done;}} async control work<t:reader>(t input) exits done faults {await reader<t>.read(input);exit done;} trait cleanup {@contingent control finish(self input) exits done;} implement cleanup for string {control finish(self input) exits done {emit(input);exit done;}} @contingent control finish<t:cleanup>(t input) exits done {cleanup<t>.finish(input);exit done;} control main() exits done faults {run work<u32>(1);finish<string>(\"finished\");exit done;}";
    let p = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let scenario = tellur::runtime::Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let report = p.run(&scenario).unwrap();
    assert_eq!(report.events.len(), 1);
    assert_eq!(report, p.expanded("main").unwrap().run(&scenario).unwrap());
}

#[test]
fn implementation_permission_batches_preserve_transitive_imported_points() {
    let f = Fixture::new();
    f.write(
        "contract.tellur",
        "pub trait reader {control read(self input) exits done;}",
    );
    for file in ["first.tellur", "second.tellur"] {
        f.write(file,"import \"contract.tellur\" as contract;\r\nstruct local {u32 value;}\r\nimplement contract.reader for local {\r\n    async control read(self input) exits done {exit done;}\r\n}");
    }
    f.write(
        "middle.tellur",
        "import \"first.tellur\" as first;import \"second.tellur\" as second;",
    );
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for error in errors {
        assert_eq!(error["code"], "E_ASYNC");
        assert_eq!(error["position"], json!({"line":4,"column":5}));
    }
    let paths = errors
        .iter()
        .map(|e| {
            PathBuf::from(e["path"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        ["first.tellur".to_owned(), "second.tellur".to_owned()]
            .into_iter()
            .collect()
    );
    assert!(report["summary"].is_null());
    let first = tellur::compile_file(
        f.root.join("main.tellur"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        first.path.to_string_lossy(),
        errors[0]["path"].as_str().unwrap()
    );
    assert_eq!(first.diagnostic.code, "E_ASYNC");
    assert_eq!(first.diagnostic.span.line, 4);
}

#[test]
fn implementation_conformance_batches_mix_capability_signature_and_origin_failures() {
    let source = "struct pair['a,'b] {ref string left <- 'a;ref string right <- 'b;}\ntrait a_bound {type item:copy;} implement a_bound for u32 {type item=string;}\ntrait b_parameter {control act(self input) exits done;} implement b_parameter for u32 {control act(ref self input) exits done {exit done;}}\ntrait c_payload {control act() exits ready {u32 value};} implement c_payload for u32 {control act() exits ready {string value} {exit ready {\"x\"};}}\ntrait d_origin {control pick(ref string left,ref string right) exits ready {ref string value from left};} implement d_origin for u32 {control pick(ref string a,ref string b) exits ready {ref string value from b} {exit ready {b};}}\ntrait e_group {control pick(ref pair input) exits ready {pair[input['a],input['b]] value};} implement e_group for u32 {control pick(ref pair owner) exits ready {pair[owner['b],owner['a]] result} {exit ready {pair {owner.right,owner.left}};}}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        [
            ("E_BOUND", 2),
            ("E_TRAIT_SIGNATURE", 3),
            ("E_TRAIT_SIGNATURE", 4),
            ("E_LIFETIME", 5),
            ("E_ORIGIN", 6)
        ],
        "{errors}"
    );
    assert!(!errors.limit_reached);
    assert_eq!(
        compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn implementation_conformance_batches_preserve_local_priority_and_method_declaration_order() {
    let source = "trait reader {control first(self input) exits done;control second() exits ready {u32 value};}\nimplement reader for u32 {\ncontrol second() exits ready {string value} {exit ready {\"x\"};}\ncontrol first(ref self input) exits done {exit done;}\n}\nimplement reader for string {\ncontrol second() exits ready {string value} {exit ready {\"x\"};}\ncontrol first(ref self input) exits done {exit done;}\n}";
    let errors = check_errors(source);
    assert_eq!(
        errors
            .errors
            .iter()
            .map(|e| (e.code, e.span.line))
            .collect::<Vec<_>>(),
        [("E_TRAIT_SIGNATURE", 8), ("E_TRAIT_SIGNATURE", 4)],
        "{errors}"
    );
    let source = source
        .replace("trait reader {", "trait reader {type item:copy;")
        .replace(
            "implement reader for u32 {",
            "implement reader for u32 {type item=string;",
        )
        .replace(
            "implement reader for string {",
            "implement reader for string {type item=string;",
        );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_BOUND"));
}

#[test]
fn implementation_conformance_batches_replay_repairs_before_body_errors() {
    let source = "trait reader {type item:copy;control read(self input) exits done;}\nimplement reader for u32 {type item=string;control read(ref self input) exits done {u32 wrong=true;exit done;}}\nimplement reader for string {type item=string;control read(self input) exits done {exit done;}}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(errors.errors.iter().all(|e| e.code == "E_BOUND"));
    let partial = source.replace("for string {type item=string", "for string {type item=u32");
    let errors = check_errors(&partial);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert_eq!(errors.errors[0].code, "E_BOUND");
    assert_eq!(errors.errors[0].span.line, 2);
    let repaired = partial.replace("item=string", "item=u32");
    assert_eq!(check_errors(&repaired).errors[0].code, "E_TRAIT_SIGNATURE");
    let repaired = repaired.replace("ref self input", "self input");
    assert_eq!(check_errors(&repaired).errors[0].code, "E_TYPE");
    compile(
        &repaired.replace("u32 wrong=true;", ""),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn implementation_conformance_batches_resolve_named_contracts_and_keep_generic_witnesses() {
    let source = "outcome expected {ready {u32 value},empty} outcome alternative {empty,ready {u32 different}} trait reader {control read(self input) exits expected;} implement reader for u32 {control read(ref self input) exits alternative {exit ready {1};}} implement reader for string {control read(ref self input) exits alternative {exit ready {2};}} control use<t:reader>(t input) exits expected {forward reader<t>.read(input);} control main() exits auto {forward use<string>(\"x\");}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 2, "{errors}");
    assert!(
        errors.errors.iter().all(|e| e.code == "E_TRAIT_SIGNATURE"),
        "{errors}"
    );
    assert!(!errors.to_string().contains("_path_poly_"), "{errors}");
    let fixed = source.replace("ref self input", "self input");
    let p = compile(
        &fixed,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let scenario = tellur::runtime::Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let report = p.run(&scenario).unwrap();
    assert_eq!(report.fields, vec![tellur::ir::Value::U32(2)]);
    assert_eq!(report, p.expanded("main").unwrap().run(&scenario).unwrap());
    // Abstract signatures for unused methods must not require concrete regions in
    // the dependency-reachable symbolic checking unit.
    let extra = fixed
        .replace(
            "trait reader {",
            "trait reader {control unused() exits done;",
        )
        .replace(
            "implement reader for u32 {",
            "implement reader for u32 {control unused() exits done {exit done;}",
        )
        .replace(
            "implement reader for string {",
            "implement reader for string {control unused() exits done {exit done;}",
        );
    compile(
        &extra,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn implementation_conformance_batches_keep_earlier_barriers_and_budget() {
    let source = "trait reader {type item:copy;} implement reader for u32 {type item=string;} implement reader for string {type item=string;}";
    for (prefix, code) in [
        (
            "struct invalid['a,'a] {ref string value <- 'a;} ",
            "E_ORIGIN",
        ),
        (
            "trait invalid {control read() exits auto;} ",
            "E_TRAIT_SIGNATURE",
        ),
        (
            "trait missing {type item;} implement missing for bool {} ",
            "E_TRAIT",
        ),
        (
            "trait source {type item;} implement source for bool {type item=ref string;} ",
            "E_LIFETIME",
        ),
        (
            "trait operation {control act() exits done;} implement operation for bool {async control act() exits done {exit done;}} ",
            "E_ASYNC",
        ),
    ] {
        let errors = check_errors(&format!("{prefix}{source}"));
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code);
    }
    let mut source = "trait reader {type item:copy;}\n".to_owned();
    for n in 0..40 {
        source.push_str(&format!(
            "struct value_{n} {{u32 value;}} implement reader for value_{n} {{type item=string;}}\n"
        ));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32, "{errors}");
    assert!(errors.limit_reached);
    assert!(errors.errors.iter().all(|e| e.code == "E_BOUND"));
}

#[test]
fn implementation_conformance_batches_preserve_transitive_imported_points() {
    let f = Fixture::new();
    f.write("contract.tellur", "pub trait reader {type item:copy;}");
    for file in ["first.tellur", "second.tellur"] {
        f.write(file,"import \"contract.tellur\" as contract;\r\nstruct local {u32 value;}\r\nimplement contract.reader for local {\r\n    type item=string;\r\n}");
    }
    f.write(
        "middle.tellur",
        "import \"first.tellur\" as first;import \"second.tellur\" as second;",
    );
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let errors = report["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{report}");
    for error in errors {
        assert_eq!(error["code"], "E_BOUND");
        assert_eq!(error["position"], json!({"line":4,"column":15}));
    }
    let paths = errors
        .iter()
        .map(|e| {
            PathBuf::from(e["path"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        ["first.tellur".to_owned(), "second.tellur".to_owned()]
            .into_iter()
            .collect()
    );
    assert!(report["summary"].is_null());
    let first = tellur::compile_file(
        f.root.join("main.tellur"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        first.path.to_string_lossy(),
        errors[0]["path"].as_str().unwrap()
    );
    assert_eq!(first.diagnostic.code, "E_BOUND");
    assert_eq!(first.diagnostic.span.line, 4);
}

#[test]
fn implementation_completion_ranges_select_written_inline_contracts() {
    let f = Fixture::new();
    for (expected, actual, selected) in [
        (
            "ready {u32 value}",
            "ready {string value}",
            "exits ready {string value}",
        ),
        (
            "ready",
            "other, extra {u32 value}",
            "exits other, extra {u32 value}",
        ),
        (
            "ready",
            "// 😀\r\n ready {\r\n string value\r\n }, absent",
            "exits // 😀\r\n ready {\r\n string value\r\n }, absent",
        ),
    ] {
        let source = format!(
            "trait reader {{control read() exits {expected} faults;}}\r\nimplement reader for u32 {{control read() exits {actual} // outside\r\n faults {{fault \"body\";}}}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        assert_eq!(errors[0]["code"], "E_TRAIT_SIGNATURE", "{report}");
        assert_eq!(
            errors[0]["range"],
            selected_range(&source, selected),
            "{report}"
        );
        assert_eq!(errors[0]["position"], errors[0]["range"]["start"]);
    }
}

#[test]
fn implementation_completion_ranges_select_direct_and_contained_origin_contracts() {
    let f = Fixture::new();
    for (source, selected, code) in [
        (
            "trait reader {control pick(ref string left,ref string right) exits ready {ref string value from left};} implement reader for u32 {control pick(ref string a,ref string b) exits ready {ref string value from // 😀\r\n b} {exit ready {b};}}",
            "exits ready {ref string value from // 😀\r\n b}",
            "E_LIFETIME",
        ),
        (
            "struct pair['a,'b] {ref string left <- 'a;ref string right <- 'b;} trait reader {control get(ref pair input) exits ready {pair[input['a],input['b]] value};} implement reader for u32 {control get(ref pair owner) exits ready {pair[owner['b],owner['a]] result} {exit ready {pair {owner.right,owner.left}};}}",
            "exits ready {pair[owner['b],owner['a]] result}",
            "E_ORIGIN",
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        assert_eq!(report["diagnostics"][0]["code"], code, "{report}");
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(source, selected),
            "{report}"
        );
        let batch = check_errors(source);
        assert_eq!(
            compile(
                source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            batch.errors[0]
        );
    }
}

#[test]
fn implementation_completion_ranges_preserve_imported_generic_named_use_sites() {
    let f = Fixture::new();
    f.write("contracts.tellur", "pub outcome reply<t> {ready {t value}} pub outcome borrowed<t>['a] {ready {ref t value <- 'a}}");
    f.write(
        "main.tellur",
        "import \"worker.tellur\" as worker;control main() exits done {exit done;}",
    );
    for (body, selected, code) in [
        (
            "trait reader {control read(self input) exits c.reply<u32>;} implement reader for u32 {control read(self owned) exits // 😀\r\n c.reply<string> {exit ready {\"x\"};}} control call<t:reader>(t input) exits c.reply<u32> {forward reader<t>.read(input);}",
            "exits // 😀\r\n c.reply<string>",
            "E_TRAIT_SIGNATURE",
        ),
        (
            "trait reader {control read(ref string left,ref string right) exits c.borrowed<string>[left];} implement reader for u32 {control read(ref string a,ref string b) exits c.borrowed< // 😀\r\n string>[b] {exit ready {b};}}",
            "exits c.borrowed< // 😀\r\n string>[b]",
            "E_LIFETIME",
        ),
    ] {
        let source = format!("import \"contracts.tellur\" as c;\r\n{body}");
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        assert_eq!(
            error["range"],
            selected_range(&source, selected),
            "{report}"
        );
        assert!(!report.to_string().contains("_path_poly_"));
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(first.diagnostic.code, code);
        assert_eq!(
            first.diagnostic.span.line,
            error["position"]["line"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.unwrap().column,
            error["range"]["end"]["column"].as_u64().unwrap() as usize
        );
    }
}

#[test]
fn implementation_completion_ranges_keep_parameter_priority_child_ranges_and_fault_only_fallback() {
    let f = Fixture::new();
    for (source, selected, annotation) in [
        (
            "trait reader {control read(u32 input) exits ready {u32 value};} implement reader for u32 {control read(ref u32 input) exits ready {string value} {exit ready {\"x\"};}}",
            "control read(ref u32 input)",
            Some("ref u32"),
        ),
        (
            "trait reader {control read() exits done faults;} implement reader for u32 {control read() faults {fault \"x\";}}",
            "control read() faults",
            None,
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_TRAIT_SIGNATURE", "{report}");
        if let Some(annotation) = annotation {
            assert_eq!(
                error["range"],
                selected_range(source, annotation),
                "{report}"
            );
        } else {
            assert!(error["range"].is_null(), "{report}");
            assert_eq!(
                error["position"],
                scalar_position(source, source.find(selected).unwrap())
            );
        }
    }
    let source = "trait reader {control read(ref string input) exits ready {ref string value from input};} implement reader for u32 {control read(ref string owner) exits ready {ref string value from missing} {exit ready {owner};}}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_LIFETIME", "{report}");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "from missing")
    );
}

#[test]
fn implementation_completion_ranges_preserve_batch_order_and_budget() {
    let mut source = "trait reader {control read() exits ready {u32 value};}\n".to_owned();
    for n in (0..40).rev() {
        source.push_str(&format!("struct value_{n:02} {{u32 value;}} implement reader for value_{n:02} {{control read() exits ready {{string result_{n:02}}} {{exit ready {{\"x\"}};}}}}\n"));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32, "{errors}");
    assert!(errors.limit_reached);
    for (n, error) in errors.errors.iter().enumerate() {
        let selected = format!("exits ready {{string result_{n:02}}}");
        let range = selected_range(&source, &selected);
        assert_eq!(error.code, "E_TRAIT_SIGNATURE");
        assert_eq!(
            error.span.line,
            range["start"]["line"].as_u64().unwrap() as usize
        );
        assert_eq!(
            error.span.column,
            range["start"]["column"].as_u64().unwrap() as usize
        );
        assert_eq!(
            error.end.as_ref().unwrap().column,
            range["end"]["column"].as_u64().unwrap() as usize
        );
    }
    assert_eq!(
        compile(
            &source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn implementation_parameter_ranges_select_first_differing_written_type() {
    let f = Fixture::new();
    for (expected, actual, selected) in [
        ("self input", "ref self owner", "ref self"),
        (
            "ref self input",
            "ref mutable self owner",
            "ref mutable self",
        ),
        ("self input", "owner: &self", "&self"),
        (
            "u32 first,u32 second,u32 third",
            "u32 a,string b,bool c",
            "string",
        ),
        (
            "vector<u32> input",
            "vector< // 😀\r\n string> owner",
            "vector< // 😀\r\n string>",
        ),
        (
            "self input",
            "ref // 😀\r\n self owner",
            "ref // 😀\r\n self",
        ),
    ] {
        let source = format!(
            "trait reader {{control read({expected}) exits done;}}\r\nimplement reader for u32 {{control read({actual}) exits done {{exit done;}}}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_TRAIT_SIGNATURE", "{report}");
        assert_eq!(
            error["range"],
            selected_range(&source, selected),
            "{report}"
        );
        assert_eq!(error["position"], error["range"]["start"]);
    }
}

#[test]
fn implementation_parameter_ranges_keep_imported_aliases_and_generic_callers() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    let source = "// 😀\r\ntrait reader {type item;control read(vector<item> input) exits done;}\r\nimplement reader for u32 {type item=u32;control read(ref vector< // 😀\r\n item> owner) exits done {exit done;}}\r\ncontrol call<t:reader>(vector<reader<t>.item> input) exits done {reader<t>.read(input);exit done;}";
    f.write("worker.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        1,
        "{report}"
    );
    let error = &report["diagnostics"][0];
    assert_eq!(error["code"], "E_TRAIT_SIGNATURE", "{report}");
    assert_eq!(
        error["path"],
        f.root.join("worker.tellur").to_string_lossy().as_ref()
    );
    assert_eq!(
        error["range"],
        selected_range(source, "ref vector< // 😀\r\n item>")
    );
    assert!(!report.to_string().contains("_path_poly_"));
    let first = tellur::compile_file(
        f.root.join("main.tellur"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first.path, f.root.join("worker.tellur"));
    assert_eq!(
        first.diagnostic.span.line,
        error["position"]["line"].as_u64().unwrap() as usize
    );
    assert_eq!(
        first.diagnostic.end.as_ref().unwrap().column,
        error["range"]["end"]["column"].as_u64().unwrap() as usize
    );
    // Repair ownership and let the now-owned implementation consume its input.
    let fixed = source.replace("read(ref vector<", "read(vector<").replace(
        "owner) exits done {exit done;}",
        "owner) exits done {drop(owner);exit done;}",
    );
    f.write("worker.tellur", &fixed);
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn implementation_parameter_ranges_preserve_arity_fallback_and_origin_priority() {
    let f = Fixture::new();
    for (expected, actual) in [
        ("u32 input", ""),
        ("", "u32 input"),
        ("u32 input", "string first,u32 second"),
    ] {
        let source = format!(
            "trait reader {{control read({expected}) exits done;}} implement reader for u32 {{control read({actual}) exits done {{exit done;}}}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_TRAIT_SIGNATURE", "{report}");
        assert!(error["range"].is_null(), "{report}");
        assert_eq!(
            error["position"],
            scalar_position(&source, source.rfind("control read").unwrap())
        );
    }
    for (origin, selected) in [
        ("b", "exits ready {ref string value from b}"),
        ("missing", "from missing"),
    ] {
        let source = format!(
            "trait reader {{control read(ref string left,ref string right,u32 n) exits ready {{ref string value from left}};}} implement reader for u32 {{control read(ref string a,ref string b,bool n) exits ready {{ref string value from {origin}}} {{exit ready {{b}};}}}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(report["diagnostics"][0]["code"], "E_LIFETIME", "{report}");
        assert_eq!(
            report["diagnostics"][0]["range"],
            selected_range(&source, selected)
        );
    }
}

#[test]
fn implementation_parameter_ranges_preserve_batch_order_and_budget() {
    let mut source = "trait reader {control read(u32 input) exits done;}\n".to_owned();
    for n in (0..40).rev() {
        source.push_str(&format!("struct value_{n:02} {{u32 value;}} implement reader for value_{n:02} {{control read(ref u32 input) exits done {{exit done;}}}}\n"));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32, "{errors}");
    assert!(errors.limit_reached);
    for (n, error) in errors.errors.iter().enumerate() {
        assert_eq!(error.code, "E_TRAIT_SIGNATURE");
        assert_eq!(error.span.line, 41 - n);
        assert_eq!(
            error.end.as_ref().unwrap().column - error.span.column,
            "ref u32".len()
        );
    }
    assert_eq!(
        compile(
            &source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
}

#[test]
fn associated_assignment_ranges_select_complete_written_types() {
    let f = Fixture::new();
    for (bounds, value, code) in [
        ("", "ref // 😀\r\n mutable string", "E_LIFETIME"),
        (":copy", "option< // 😀\r\n string>", "E_BOUND"),
        ("", "reader<string>.item", "E_TRAIT"),
        ("", "reader<u32>.missing", "E_TRAIT"),
    ] {
        let source = format!(
            "trait reader {{type item{bounds};}}\r\nimplement reader for u32 {{type item = {value} // outside\r\n ;}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(error["range"], selected_range(&source, value), "{report}");
        assert_eq!(error["position"], error["range"]["start"]);
        let batch = check_errors(&source);
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            batch.errors[0]
        );
    }
}

#[test]
fn associated_assignment_ranges_preserve_normalization_roots_and_suppress_dependents() {
    let f = Fixture::new();
    for (source, selected, codes) in [
        (
            "trait reader {type a;type b;type c;type d;} implement reader for u32 {type a=b;type b=ref string;type c=vector<b>;type d=ref u32;}",
            vec!["b;type b", "ref u32"],
            vec!["E_LIFETIME", "E_LIFETIME"],
        ),
        (
            "trait reader {type a;type b;type c;} implement reader for u32 {type a=reader<string>.a;type b=ref u32;type c=a;} implement reader for string {type a=reader<u32>.a;type b=ref string;type c=a;}",
            vec!["reader<u32>.a", "ref string", "ref u32"],
            vec!["E_TRAIT", "E_LIFETIME", "E_LIFETIME"],
        ),
    ] {
        f.write("main.tellur", source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), selected.len(), "{report}");
        for ((error, selected), code) in errors.iter().zip(selected).zip(codes) {
            let at = source.find(selected).unwrap();
            let length = if selected.starts_with("b;") {
                1
            } else {
                selected.len()
            };
            assert_eq!(error["code"], code, "{report}");
            assert_eq!(
                error["range"],
                json!({"start":scalar_position(source,at),"end":scalar_position(source,at+length)}),
                "{report}"
            );
        }
    }
}

#[test]
fn associated_assignment_ranges_preserve_imported_use_sites_and_module_resolution() {
    let f = Fixture::new();
    f.write("types.tellur", "pub trait source {type item;} implement source for string {type item=string;} struct hidden {u32 value;}");
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    for (bounds, value, code) in [
        (":copy", "t.source<string>.item", "E_BOUND"),
        ("", "t.source<bool>.item", "E_TRAIT"),
        ("", "vector< // 😀\r\n t.missing>", "E_NAME"),
        ("", "t.hidden", "E_VISIBILITY"),
    ] {
        let source = format!(
            "import \"types.tellur\" as t;\r\ntrait reader {{type item{bounds};}}\r\nimplement reader for u32 {{type item={value};}}\r\ncontrol call<t:reader>(reader<t>.item input) exits ready {{reader<t>.item value}} {{exit ready {{input}};}}"
        );
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        assert_eq!(error["range"], selected_range(&source, value), "{report}");
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(first.diagnostic.code, code);
        assert_eq!(
            first.diagnostic.span.line,
            error["position"]["line"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.as_ref().unwrap().column,
            error["range"]["end"]["column"].as_u64().unwrap() as usize
        );
    }
}

#[test]
fn associated_assignment_ranges_preserve_identity_and_phase_priority() {
    let source = "trait reader {type item:copy;control read() exits done;} implement reader for u32 {type item=ref string;async control read() exits done {exit done;}}";
    assert_eq!(check_errors(source).errors[0].code, "E_LIFETIME");
    let permission = source.replace("ref string", "string");
    let error = &check_errors(&permission).errors[0];
    assert_eq!(error.code, "E_ASYNC");
    assert!(error.end.is_none());
    let bound = permission.replace("async control", "control");
    let error = &check_errors(&bound).errors[0];
    assert_eq!(error.code, "E_BOUND");
    assert!(error.end.is_some());
    let duplicate = format!(
        "{bound} implement reader for u32 {{type item=u32;control read() exits done {{exit done;}}}}"
    );
    let error = &check_errors(&duplicate).errors[0];
    assert_eq!(error.code, "E_COHERENCE");
    assert!(error.end.is_none());
    compile(
        &bound.replace("item=string", "item=u32"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn associated_assignment_ranges_keep_canonical_order_and_error_budget() {
    for (bound, value, code) in [
        ("", "ref u32", "E_LIFETIME"),
        (":copy", "string", "E_BOUND"),
    ] {
        let mut source = format!("trait reader {{type item{bound};}}\n");
        for n in (0..40).rev() {
            source.push_str(&format!("struct value_{n:02} {{u32 n;}} implement reader for value_{n:02} {{type item={value};}}\n"));
        }
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 32, "{errors}");
        assert!(errors.limit_reached);
        for (n, error) in errors.errors.iter().enumerate() {
            assert_eq!(error.code, code);
            assert_eq!(error.span.line, 41 - n);
            assert_eq!(
                error.end.as_ref().unwrap().column - error.span.column,
                value.len()
            );
        }
    }
}

#[test]
fn associated_concrete_ranges_select_written_types_and_aliases() {
    let f = Fixture::new();
    for (prefix, assignments, selected, code) in [
        (
            "",
            "type a=option< // 😀\r\n vector<missing>>;",
            "option< // 😀\r\n vector<missing>>",
            "E_MANIFEST",
        ),
        (
            "",
            "type a=b;type b=vector<missing>;",
            "b;type b",
            "E_MANIFEST",
        ),
        (
            "trait z_source {type item;} implement z_source for u32 {type item=vector<missing>;}",
            "type a=z_source<u32>.item;",
            "z_source<u32>.item",
            "E_MANIFEST",
        ),
        (
            "struct hidden {u32 value;}",
            "type a=vector< // 😀\r\n hidden>;",
            "vector< // 😀\r\n hidden>",
            "E_VISIBILITY",
        ),
        (
            "struct hidden {u32 value;}",
            "type a=b;type b=hidden;",
            "b;type b",
            "E_VISIBILITY",
        ),
        (
            "struct hidden {u32 value;} trait z_source {type item;} implement z_source for u32 {type item=hidden;}",
            "type a=z_source<u32>.item;",
            "z_source<u32>.item",
            "E_VISIBILITY",
        ),
    ] {
        let declaration = if assignments.contains("type b=") {
            "type a;type b;"
        } else {
            "type a;"
        };
        let source = format!(
            "{prefix}\r\npub trait reader {{{declaration}}}\r\nimplement reader for u32 {{{assignments}}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(
            errors.len(),
            if prefix.contains("vector<missing>") {
                2
            } else {
                1
            },
            "{report}"
        );
        let error = &errors[0];
        assert_eq!(error["code"], code, "{report}");
        let at = source.find(selected).unwrap();
        let length = if selected.starts_with("b;") {
            1
        } else {
            selected.len()
        };
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+length)}),
            "{report}"
        );
        assert_eq!(error["position"], error["range"]["start"]);
        let batch = check_errors(&source);
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            batch.errors[0]
        );
    }
}

#[test]
fn associated_concrete_ranges_preserve_imports_and_generic_witnesses() {
    let f = Fixture::new();
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    for (value, code) in [
        ("vector< // 😀\r\n missing>", "E_MANIFEST"),
        ("option< // 😀\r\n hidden>", "E_VISIBILITY"),
    ] {
        let source = format!(
            "struct hidden {{u32 value;}}\r\npub trait reader {{type item;control read(self input) exits done;}}\r\nimplement reader for u32 {{type item={value};control read(self local) exits done {{exit done;}}}}\r\ncontrol call<t:reader>(t input) exits done {{reader<t>.read(input);exit done;}}"
        );
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        assert_eq!(error["range"], selected_range(&source, value), "{report}");
        assert!(!report.to_string().contains("_path_poly_"));
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(first.diagnostic.code, code);
        assert_eq!(
            first.diagnostic.span.line,
            error["position"]["line"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.as_ref().unwrap().column,
            error["range"]["end"]["column"].as_u64().unwrap() as usize
        );
        f.write("worker.tellur", &source.replace(value, "u32"));
        f.report(&["check", "main.tellur", "--json"], true);
    }
}

#[test]
fn associated_concrete_ranges_preserve_bound_type_body_and_visibility_priority() {
    let source = "struct hidden {u32 value;} pub trait reader {type a;type b:copy;type c;control read(self input) exits done;} implement reader for u32 {type a=vector<missing>;type b=string;type c=hidden;control read(ref self local) exits done {u32 wrong=true;exit done;}}";
    let mut repaired = source.to_owned();
    for (code, from, to) in [
        ("E_BOUND", "type b=string", "type b=u32"),
        ("E_MANIFEST", "vector<missing>", "u32"),
        ("E_TRAIT_SIGNATURE", "ref self local", "self local"),
        ("E_TYPE", "u32 wrong=true;", ""),
        ("E_VISIBILITY", "type c=hidden", "type c=u32"),
    ] {
        let errors = check_errors(&repaired);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code, "{errors}");
        repaired = repaired.replace(from, to);
    }
    compile(
        &repaired,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let target =
        "trait reader {type item;} implement reader for missing {type item=vector<missing>; }";
    let error = &check_errors(target).errors[0];
    assert_eq!(error.code, "E_TRAIT");
    assert_eq!(error.span.column, target.find("for ").unwrap() + 5);
    assert_eq!(
        error.end.as_ref().unwrap().column - error.span.column,
        "missing".len()
    );
}

#[test]
fn associated_concrete_ranges_keep_canonical_batches_and_visibility_barrier() {
    let mut source = "pub trait reader {type a;type b;}\n".to_owned();
    for n in (0..40).rev() {
        source.push_str(&format!("pub struct value_{n:02} {{u32 n;}} implement reader for value_{n:02} {{type a=vector<missing>;type b=missing;}}\n"));
    }
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 32, "{errors}");
    assert!(errors.limit_reached);
    for (n, error) in errors.errors.iter().enumerate() {
        assert_eq!(error.code, "E_MANIFEST");
        assert_eq!(error.span.line, 41 - n);
        assert_eq!(
            error.end.as_ref().unwrap().column - error.span.column,
            "vector<missing>".len()
        );
    }
    assert_eq!(
        compile(
            &source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        errors.errors[0]
    );
    let private = format!(
        "struct hidden {{u32 n;}}\n{}",
        source.replace("missing", "hidden")
    );
    let errors = check_errors(&private);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert!(!errors.limit_reached);
    assert_eq!(errors.errors[0].code, "E_VISIBILITY");
    assert_eq!(errors.errors[0].span.line, 42);
    assert_eq!(
        errors.errors[0].end.as_ref().unwrap().column - errors.errors[0].span.column,
        "vector<hidden>".len()
    );
}

#[test]
fn associated_instantiation_ranges_select_written_types_and_aliases() {
    let f = Fixture::new();
    for (prefix, assignment, selected, message) in [
        (
            "",
            "type a=box< // 😀\r\n u32,bool>;",
            "box< // 😀\r\n u32,bool>",
            "expects 1",
        ),
        ("", "type a=box;", "box;}", "requires explicit"),
        (
            "",
            "type a=option<box<missing>>;",
            "option<box<missing>>",
            "unknown type",
        ),
        (
            "",
            "type a=b;type b=box<u32,bool>;",
            "b;type b",
            "expects 1",
        ),
        (
            "trait z_source {type item;} implement z_source for bool {type item=box<u32,bool>;}",
            "type a=z_source<bool>.item;",
            "z_source<bool>.item",
            "expects 1",
        ),
    ] {
        let members = if assignment.contains("type b=") {
            "type a;type b;"
        } else {
            "type a;"
        };
        let source = format!(
            "struct box<t> {{t value;}} {prefix}\r\ntrait reader {{{members}}}\r\nimplement reader for u32 {{{assignment}}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        assert_eq!(errors[0]["code"], "E_GENERIC", "{report}");
        assert!(
            errors[0]["message"].as_str().unwrap().contains(message),
            "{report}"
        );
        let at = source.find(selected).unwrap();
        let length = if selected == "b;type b" {
            1
        } else if selected == "box;}" {
            3
        } else {
            selected.len()
        };
        assert_eq!(
            errors[0]["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+length)}),
            "{report}"
        );
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            check_errors(&source).errors[0]
        );
    }
}

#[test]
fn associated_instantiation_ranges_preserve_imported_aliases_and_witnesses() {
    let f = Fixture::new();
    f.write("types.tellur", "pub struct box<t> {t value;}");
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let value = "types.box< // 😀\r\n u32,bool>";
    let source = format!(
        "import \"types.tellur\" as types;\r\npub trait reader {{type item;control read(self input) exits done;}}\r\nimplement reader for u32 {{type item={value};control read(self local) exits done {{exit done;}}}}\r\ncontrol call<t:reader>(t input) exits done {{reader<t>.read(input);exit done;}}"
    );
    f.write("worker.tellur", &source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let error = &report["diagnostics"][0];
    assert_eq!(error["code"], "E_GENERIC", "{report}");
    assert_eq!(
        error["path"],
        f.root.join("worker.tellur").to_string_lossy().as_ref()
    );
    assert_eq!(error["range"], selected_range(&source, value), "{report}");
    assert!(!report.to_string().contains("_path_poly_"));
    let first = tellur::compile_file(
        f.root.join("main.tellur"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(first.path, f.root.join("worker.tellur"));
    assert_eq!(first.diagnostic.code, "E_GENERIC");
    assert_eq!(
        first.diagnostic.span.line,
        error["position"]["line"].as_u64().unwrap() as usize
    );
    assert_eq!(
        first.diagnostic.end.as_ref().unwrap().column,
        error["range"]["end"]["column"].as_u64().unwrap() as usize
    );
    f.write("worker.tellur", &source.replace(value, "types.box<u32>"));
    f.report(&["check", "main.tellur", "--json"], true);
}

#[test]
fn associated_instantiation_ranges_preserve_declaration_signature_and_body_priority() {
    let f = Fixture::new();
    let base = "struct box<t> {t value;} trait reader {type item:copy;} implement reader for u32 {type item=box<u32,bool>;}";
    for (prefix, selected) in [
        ("struct broken {missing field;}", "missing"),
        (
            "control earlier(box<u32,bool> value) exits done {exit done;}",
            "box<u32,bool> value",
        ),
        (
            "control earlier() exits done {box<u32,bool> value=1;exit done;}",
            "box<u32,bool> value",
        ),
    ] {
        let source = format!("{prefix}\r\n{base}");
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], "E_GENERIC", "{report}");
        let start = source.find(selected).unwrap();
        let length = if selected == "missing" {
            7
        } else {
            "box<u32,bool>".len()
        };
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,start),"end":scalar_position(&source,start+length)}),
            "{report}"
        );
    }
    assert_eq!(check_errors(base).errors[0].code, "E_GENERIC");
    let bound = base.replace("box<u32,bool>", "box<string>");
    assert_eq!(check_errors(&bound).errors[0].code, "E_BOUND");
    compile(
        &base.replace("box<u32,bool>", "box<u32>"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    // A malformed implementation target is required before its assigned types.
    let target = "struct box<t> {t value;} trait reader {type item;} implement reader for box<u32,bool> {type item=box<missing>;}";
    let error = &check_errors(target).errors[0];
    assert_eq!(error.code, "E_TRAIT", "{error}");
    assert!(error.message.contains("wrong arity"));
    assert_eq!(error.span.column, target.find("for ").unwrap() + 5);
    assert_eq!(
        error.end.as_ref().unwrap().column - error.span.column,
        "box<u32,bool>".len()
    );
}

#[test]
fn associated_instantiation_ranges_keep_single_error_canonical_order_and_repairs() {
    let mut source = "struct box<t> {t value;} trait reader {type item;}\n".to_owned();
    for n in (0..40).rev() {
        source.push_str(&format!("struct value_{n:02} {{u32 n;}} implement reader for value_{n:02} {{type item=box<u32,bool>;}}\n"));
    }
    for fixed in 0..2 {
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert!(!errors.limit_reached);
        let error = &errors.errors[0];
        assert_eq!(error.code, "E_GENERIC");
        assert_eq!(error.span.line, 41 - fixed);
        assert_eq!(
            error.end.as_ref().unwrap().column - error.span.column,
            "box<u32,bool>".len()
        );
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            *error
        );
        source = source.replace(
            &format!("for value_{fixed:02} {{type item=box<u32,bool>"),
            &format!("for value_{fixed:02} {{type item=box<u32>"),
        );
    }
    compile(
        &source.replace("box<u32,bool>", "box<u32>"),
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}

#[test]
fn implementation_target_ranges_select_complete_written_types() {
    let f = Fixture::new();
    for target in [
        "missing",
        "box",
        "box< // 😀\r\n u32,bool>",
        "ref // 😀\r\n mutable u32",
        "array<u32,string>",
        "source<u32>.item",
    ] {
        let source = format!(
            "struct box<t> {{t value;}} trait source {{type item;}} trait reader {{}}\r\nimplement reader for {target} // outside\r\n{{}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        assert_eq!(errors[0]["code"], "E_TRAIT", "{report}");
        let at = source.find("implement reader for ").unwrap() + "implement reader for ".len();
        assert_eq!(
            errors[0]["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+target.len())}),
            "{report}"
        );
        assert_eq!(errors[0]["position"], errors[0]["range"]["start"]);
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            check_errors(&source).errors[0]
        );
    }
}

#[test]
fn implementation_target_ranges_preserve_imported_lookup_and_privacy() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub struct item {u32 value;} pub struct box<t> {t value;} struct hidden {u32 value;}",
    );
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    for (target, code) in [
        ("types.missing", "E_NAME"),
        ("types.hidden", "E_VISIBILITY"),
        ("types.box< // 😀\r\n u32,bool>", "E_TRAIT"),
        ("ref mutable types.item", "E_TRAIT"),
    ] {
        let source = format!(
            "import \"types.tellur\" as types;\r\ntrait reader {{}}\r\nimplement reader for {target} {{}}"
        );
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        assert_eq!(error["range"], selected_range(&source, target), "{report}");
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(first.diagnostic.code, code);
        assert_eq!(
            first.diagnostic.span.line,
            error["position"]["line"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.as_ref().unwrap().column,
            error["range"]["end"]["column"].as_u64().unwrap() as usize
        );
        f.write("worker.tellur", &source.replace(target, "types.item"));
        f.report(&["check", "main.tellur", "--json"], true);
    }
}

#[test]
fn implementation_target_ranges_keep_identity_coherence_and_member_priority() {
    let source = "trait reader {control read(self input) exits done;} implement reader for u32 {} implement reader for missing {}";
    let error = &check_errors(source).errors[0];
    assert!(error.message.contains("supply exactly"));
    assert!(error.end.is_none());
    let fixed = source.replace(
        "for u32 {}",
        "for u32 {control read(self input) exits done {exit done;}}",
    );
    let error = &check_errors(&fixed).errors[0];
    assert!(error.message.contains("unknown or unapplied"));
    assert!(error.end.is_some());
    for (source, code, message) in [
        (
            "trait reader {} implement absent for missing {}",
            "E_TRAIT",
            "unknown trait",
        ),
        (
            "trait reader {} implement reader for u32 {} implement reader for u32 {} implement reader for missing {}",
            "E_COHERENCE",
            "duplicate implementation",
        ),
        (
            "trait reader {control read(self input) exits done;} implement reader for u32 {control read(self input) exits auto {exit done;}} implement reader for missing {}",
            "E_TRAIT_SIGNATURE",
            "explicit",
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code, "{errors}");
        assert!(errors.errors[0].message.contains(message), "{errors}");
        if message == "unknown trait" {
            assert_eq!(
                errors.errors[0].span.column,
                source.find("absent").unwrap() + 1
            );
            assert_eq!(
                errors.errors[0].end.as_ref().unwrap().column - errors.errors[0].span.column,
                "absent".len()
            );
        } else {
            assert!(errors.errors[0].end.is_none(), "{errors}");
        }
    }
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub trait reader {} pub struct item {u32 value;}",
    );
    let source = "import \"types.tellur\" as types;\r\nimplement types.reader for types.item {}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    let error = &report["diagnostics"][0];
    assert_eq!(error["code"], "E_COHERENCE", "{report}");
    assert_eq!(error["position"], json!({"line":2,"column":1}));
    assert!(error["range"].is_null());
}

#[test]
fn implementation_target_ranges_keep_invalid_identity_order_and_budgets() {
    let mut source = "trait reader {}\n".to_owned();
    for n in (0..40).rev() {
        source.push_str(&format!("implement reader for missing_{n:02} {{}}\n"));
    }
    for fixed in 0..2 {
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert!(!errors.limit_reached);
        let error = &errors.errors[0];
        assert_eq!(error.code, "E_TRAIT");
        assert_eq!(error.span.line, 2 + fixed);
        assert_eq!(
            error.end.as_ref().unwrap().column - error.span.column,
            "missing_39".len()
        );
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            *error
        );
        source = source.replace(
            &format!("missing_{:02}", 39 - fixed),
            if fixed == 0 { "u32" } else { "string" },
        );
    }
    let limited = format!(
        "trait reader {{}}\n{}",
        "implement reader for missing {}\n".repeat(1025)
    );
    let errors = check_errors(&limited);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert!(!errors.limit_reached);
    assert!(errors.errors[0].message.contains("1024-implementation"));
    assert_eq!(errors.errors[0].span.line, 1026);
    assert!(errors.errors[0].end.is_none());
}

#[test]
fn implementation_trait_ranges_select_written_names_and_keep_core_first_errors() {
    let f = Fixture::new();
    for (name, code) in [
        ("absent", "E_TRAIT"),
        ("reader.missing", "E_TRAIT"),
        ("reader // 😀\r\n . missing", "E_TRAIT"),
        ("operation", "E_TYPE"),
    ] {
        let source = format!(
            "trait reader {{}} control operation() exits done {{exit done;}}\r\nimplement {name} // outside\r\nfor u32 {{}}"
        );
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        assert_eq!(errors[0]["code"], code, "{report}");
        let at = source.find("implement ").unwrap() + "implement ".len();
        assert_eq!(
            errors[0]["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+name.len())}),
            "{report}"
        );
        // Without module resolution, a control used as a trait also reaches core lookup.
        let batch = check_errors(&source);
        assert_eq!(batch.errors[0].code, "E_TRAIT");
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            batch.errors[0]
        );
    }
}

#[test]
fn implementation_trait_ranges_preserve_imported_lookup_privacy_and_kinds() {
    let f = Fixture::new();
    f.write(
        "types.tellur",
        "pub trait reader {} trait hidden {} pub control operation() exits done {exit done;}",
    );
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    for (name, code) in [
        ("types.missing", "E_NAME"),
        ("types // 😀\r\n . missing", "E_NAME"),
        ("types.hidden", "E_VISIBILITY"),
        ("types.operation", "E_TYPE"),
        ("types", "E_NAME"),
        ("absent", "E_TRAIT"),
    ] {
        let source = format!(
            "import \"types.tellur\" as types;\r\nstruct local {{u32 value;}}\r\nimplement {name} for local {{}}"
        );
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(error["code"], code, "{report}");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        let at = source.find("implement ").unwrap() + "implement ".len();
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+name.len())}),
            "{report}"
        );
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(first.diagnostic.code, code);
        assert_eq!(
            first.diagnostic.span.line,
            error["position"]["line"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.as_ref().unwrap().column,
            error["range"]["end"]["column"].as_u64().unwrap() as usize
        );
        let fixed = format!(
            "import \"types.tellur\" as types;struct local {{u32 value;}} implement types.reader for local {{}}"
        );
        f.write("worker.tellur", &fixed);
        f.report(&["check", "main.tellur", "--json"], true);
    }
}

#[test]
fn implementation_trait_ranges_keep_trait_target_member_and_coherence_priority() {
    let source = "trait reader {control read(self input) exits done;} implement reader for u32 {} implement absent for missing {}";
    let error = &check_errors(source).errors[0];
    assert!(error.message.contains("supply exactly"));
    assert!(error.end.is_none());
    let fixed = source.replace(
        "for u32 {}",
        "for u32 {control read(self input) exits done {exit done;}}",
    );
    let error = &check_errors(&fixed).errors[0];
    assert!(error.message.contains("unknown trait"));
    assert_eq!(error.span.column, fixed.find("absent").unwrap() + 1);
    let target = fixed.replace("implement absent", "implement reader");
    let error = &check_errors(&target).errors[0];
    assert!(error.message.contains("unknown or unapplied"));
    assert_eq!(error.span.column, target.find("missing").unwrap() + 1);
    let duplicate = "trait reader {} implement reader for u32 {} implement reader for u32 {} implement absent for missing {}";
    let error = &check_errors(duplicate).errors[0];
    assert_eq!(error.code, "E_COHERENCE");
    assert!(error.end.is_none());
    let f = Fixture::new();
    f.write("types.tellur", "pub trait reader {} trait hidden {}");
    let source = "import \"types.tellur\" as types;implement types.hidden for types.missing {}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_VISIBILITY");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "types.hidden")
    );
    f.write(
        "main.tellur",
        &source.replace("types.hidden", "types.reader"),
    );
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_NAME");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "types.missing")
    );
}

#[test]
fn implementation_trait_ranges_keep_invalid_identity_order_and_budgets() {
    let mut source = "trait reader {}\n".to_owned();
    for n in (0..40).rev() {
        source.push_str(&format!(
            "struct item_{n:02} {{u32 value;}} implement missing_{n:02} for item_{n:02} {{}}\n"
        ));
    }
    for fixed in 0..2 {
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert!(!errors.limit_reached);
        let error = &errors.errors[0];
        assert_eq!(error.code, "E_TRAIT");
        assert_eq!(error.span.line, 2 + fixed);
        assert_eq!(
            error.end.as_ref().unwrap().column - error.span.column,
            "missing_39".len()
        );
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            *error
        );
        source = source.replace(&format!("missing_{:02}", 39 - fixed), "reader");
    }
    let limited = format!(
        "trait reader {{}}\n{}",
        "implement missing for u32 {}\n".repeat(1025)
    );
    let errors = check_errors(&limited);
    assert_eq!(errors.errors.len(), 1, "{errors}");
    assert!(!errors.limit_reached);
    assert!(errors.errors[0].message.contains("1024-implementation"));
    assert_eq!(errors.errors[0].span.line, 1026);
    assert!(errors.errors[0].end.is_none());
}

#[test]
fn trait_member_duplicate_ranges_select_identifiers_in_existing_check_order() {
    let f = Fixture::new();
    let marker = "// selected 😀\r\n ";
    for source in [
        format!("trait reader {{type item;type {marker}item:copy;}}"),
        format!("trait reader {{type item_name2;type {marker}item_name2;type item_name2;}}"),
        format!(
            "trait reader {{control read_value2() exits done;control {marker}read_value2() exits done;}}"
        ),
        format!(
            "trait reader {{async control read() exits done;async control {marker}read() exits done;}}"
        ),
        format!("trait reader {{control {marker}item() exits done;type item;}}"),
        format!(
            "trait reader {{control read() exits done;control read() exits done;type item;type {marker}item;}}"
        ),
        format!(
            "trait reader {{type item;}} implement reader for u32 {{type item=u32;type {marker}item=string;}}"
        ),
        format!(
            "trait reader {{async control read() exits done;}} implement reader for u32 {{async control read() exits done {{exit done;}} async control {marker}read() exits done {{exit done;}}}}"
        ),
        format!(
            "trait reader {{type item;}} implement reader for u32 {{control {marker}item() exits done {{exit done;}} type item=u32;}}"
        ),
        format!(
            "trait reader {{type item;control read() exits done;}} implement reader for u32 {{control read() exits done {{exit done;}} control read() exits done {{exit done;}} type item=u32; type {marker}item=string;}}"
        ),
    ] {
        f.write("main.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let errors = report["diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{report}");
        let at = source.find(marker).unwrap() + marker.len();
        let name = source[at..]
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap();
        assert_eq!(errors[0]["code"], "E_TRAIT");
        assert_eq!(
            errors[0]["message"],
            format!("duplicate trait member or parameter `{name}`")
        );
        assert_eq!(
            errors[0]["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+name.len())})
        );
        let batch = check_errors(&source);
        assert_eq!(batch.errors.len(), 1);
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            batch.errors[0]
        );
    }
}

#[test]
fn trait_member_duplicate_ranges_preserve_imported_coordinates_and_repairs() {
    let f = Fixture::new();
    f.write("types.tellur", "pub struct wrapper {u32 value;}");
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let valid = "import \"types.tellur\" as types;\r\npub trait reader {type item:copy;control read(self input) exits done;}\r\nimplement reader for u32 {type item=u32;control read(self local) exits done {string text=\"😀\";drop(text);exit done;}}\r\npub control work() exits done {reader<u32>.read(1);exit done;}";
    for (needle, replacement, selected) in [
        (
            "type item:copy;",
            "type item:copy;type item:drop;",
            "item:drop",
        ),
        (
            "control read(self input) exits done;",
            "control read(self input) exits done;control read(self second) exits done;",
            "read(self second)",
        ),
        (
            "type item=u32;",
            "type item=u32;type item=types.wrapper;",
            "item=types.wrapper",
        ),
        (
            "drop(text);exit done;}}",
            "drop(text);exit done;}control read(self second) exits done {exit done;}}",
            "read(self second)",
        ),
    ] {
        let source = valid.replace(needle, replacement);
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        assert_eq!(error["code"], "E_TRAIT", "{report}");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        let at = source.find(selected).unwrap();
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+4)})
        );
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(
            first.diagnostic.span.column,
            error["position"]["column"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.unwrap().column,
            first.diagnostic.span.column + 4
        );
        f.write("worker.tellur", valid);
        f.report(&["check", "main.tellur", "--json"], true);
    }
}

#[test]
fn trait_member_duplicate_ranges_keep_declaration_identity_and_coherence_priorities() {
    let source = "implement absent for missing {} trait reader {type item;type item:copy;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.contains("duplicate trait member"));
    let fixed = source.replace("type item:copy;", "");
    assert!(
        check_errors(&fixed).errors[0]
            .message
            .contains("unknown trait")
    );

    let source = "trait reader {type item;} implement reader for u32 {type item=u32;type item=string;} implement absent for missing {}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.contains("duplicate trait member"));
    assert_eq!(
        errors.errors[0].span.column,
        source.find("item=string").unwrap() + 1
    );
    let fixed = source.replace("type item=string;", "");
    assert!(
        check_errors(&fixed).errors[0]
            .message
            .contains("unknown trait")
    );

    for (source, message, code) in [
        (
            "trait reader {type item;} implement reader for missing {type item=u32;type item=string;}",
            "unknown or unapplied",
            "E_TRAIT",
        ),
        (
            "trait reader {type item;} implement reader for u32 {} implement reader for string {type item=u32;type item=string;}",
            "supply exactly",
            "E_TRAIT",
        ),
        (
            "trait reader {type item;} implement reader for u32 {type item=u32;} implement reader for u32 {type item=u32;} implement reader for string {type item=u32;type item=string;}",
            "duplicate implementation",
            "E_COHERENCE",
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors[0].code, code, "{errors}");
        assert!(errors.errors[0].message.contains(message), "{errors}");
        if message != "unknown or unapplied" {
            assert!(errors.errors[0].end.is_none());
        }
    }
}

#[test]
fn trait_member_duplicate_ranges_keep_independent_batches_and_budgets() {
    for implementations in [false, true] {
        let mut source = if implementations {
            "trait reader {type item;}\n".to_owned()
        } else {
            String::new()
        };
        for n in (0..36).rev() {
            if implementations {
                source.push_str(&format!("struct item_{n:02} {{u32 value;}} implement reader for item_{n:02} {{type item=u32;type item=string;}}\n"));
            } else {
                source.push_str(&format!("trait reader_{n:02} {{type item;type item:copy;control read() exits done;control read() exits done;}}\n"));
            }
        }
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 32, "{errors}");
        assert!(errors.limit_reached);
        for (n, error) in errors.errors.iter().enumerate() {
            assert_eq!(error.code, "E_TRAIT");
            assert_eq!(error.message, "duplicate trait member or parameter `item`");
            assert_eq!(error.span.line, n + 1 + usize::from(implementations));
            assert_eq!(error.end.as_ref().unwrap().column, error.span.column + 4);
        }
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
    let source = format!(
        "trait reader {{type item;}}\n{}",
        "implement reader for u32 {type item=u32;type item=string;}\n".repeat(1025)
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 1);
    assert!(!errors.limit_reached);
    assert!(errors.errors[0].message.contains("1024-implementation"));
    assert!(errors.errors[0].end.is_none());
}

#[test]
fn trait_member_duplicate_ranges_keep_signature_checks_separate() {
    for source in [
        "trait reader {control read(u32 item,u32 item) exits done;}",
        "trait reader {control read() exits done,done;}",
        "trait reader {control read() exits done(u32 item,u32 item);}",
        "trait reader {control read(u32 item) exits done;} implement reader for u32 {control read(u32 item,u32 item) exits done {exit done;}}",
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, "E_TRAIT", "{errors}");
        assert!(
            errors.errors[0]
                .message
                .contains("duplicate trait member or parameter"),
            "{errors}"
        );
        if errors.errors[0].message.ends_with("`item`") {
            let at = source.rfind("item").unwrap();
            assert_eq!(errors.errors[0].span.column, at + 1);
            assert_eq!(errors.errors[0].end.as_ref().unwrap().column, at + 5);
        } else {
            let at = source.rfind("done").unwrap();
            assert_eq!(errors.errors[0].span.column, at + 1);
            assert_eq!(errors.errors[0].end.as_ref().unwrap().column, at + 5);
        }
    }
}

#[test]
fn trait_signature_duplicate_ranges_select_only_written_field_names() {
    let f = Fixture::new();
    let marker = "// selected 😀\r\n ";
    for (parameters, exits) in [
        (format!("u32 item, u32 {marker}item"), "done".into()),
        (format!("item:u32, {marker}item:u32"), "done".into()),
        (
            format!("ref u32 item_long2, ref u32 {marker}item_long2"),
            "done".into(),
        ),
        ("".into(), format!("done(u32 value,u32 {marker}value)")),
        ("".into(), format!("done(value:u32,{marker}value:u32)")),
        (
            "ref u32 input".into(),
            format!("done(ref u32 value from input,ref u32 {marker}value from input)"),
        ),
    ] {
        for implementation in [false, true] {
            let signature = format!("control read({parameters}) exits {exits}");
            let source = if implementation {
                format!(
                    "trait reader {{control read() exits done;}} implement reader for u32 {{{signature} {{exit done;}}}}"
                )
            } else {
                format!("trait reader {{{signature};}}")
            };
            f.write("main.tellur", &source);
            let report = f.report(&["check", "main.tellur", "--json"], false);
            let errors = report["diagnostics"].as_array().unwrap();
            assert_eq!(errors.len(), 1, "{report}");
            let at = source.find(marker).unwrap() + marker.len();
            let name = source[at..]
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap();
            assert_eq!(errors[0]["code"], "E_TRAIT");
            assert_eq!(
                errors[0]["message"],
                format!("duplicate trait member or parameter `{name}`")
            );
            assert_eq!(
                errors[0]["range"],
                json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+name.len())})
            );
            let batch = check_errors(&source);
            assert_eq!(
                compile(
                    &source,
                    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
                )
                .unwrap_err(),
                batch.errors[0]
            );
        }
    }
}

#[test]
fn trait_signature_duplicate_ranges_preserve_imported_coordinates_and_repairs() {
    let f = Fixture::new();
    f.write("types.tellur", "pub struct wrapper {u32 value;}");
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let valid = "import \"types.tellur\" as types;\r\npub trait reader {control read(self input) exits done;}\r\nimplement reader for u32 {control read(self first) exits done {string note=\"😀\";drop(note);exit done;}} implement reader for string {control read(self second) exits done {exit done;}}\r\npub control work() exits done {reader<u32>.read(1);reader<string>.read(\"x\");exit done;}";
    for (needle, replacement, selected) in [
        ("read(self input)", "read(self arg2,self arg2)", "arg2)"),
        (
            "exits done;",
            "exits done(u32 item,item:types.wrapper);",
            "item:types.wrapper",
        ),
        (
            "read(self first)",
            "read(arg3:self,arg3:self)",
            "arg3:self)",
        ),
        (
            "read(self second) exits done",
            "read(self second) exits done(u32 part,u32 part)",
            "part)",
        ),
    ] {
        let source = valid.replace(needle, replacement);
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        assert_eq!(error["code"], "E_TRAIT");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        let at = source.find(selected).unwrap();
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+4)})
        );
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(
            first.diagnostic.span.column,
            error["position"]["column"].as_u64().unwrap() as usize
        );
        assert_eq!(
            first.diagnostic.end.as_ref().unwrap().column,
            first.diagnostic.span.column + 4
        );
        f.write("worker.tellur", valid);
        f.report(&["check", "main.tellur", "--json"], true);
    }
}

#[test]
fn trait_signature_duplicate_ranges_keep_profile_phase_and_identity_priorities() {
    let profile = "trait reader {control read<t>(u32 item,u32 item) exits done;}";
    let error = &check_errors(profile).errors[0];
    assert_eq!(error.code, "E_TRAIT_SIGNATURE");
    assert!(error.end.is_none());
    let source =
        "trait reader {control read(u32 item,u32 item) exits done(u32 value,u32 value),done;}";
    let error = &check_errors(source).errors[0];
    assert!(error.message.ends_with("`item`"));
    assert!(error.end.is_some());
    let fixed = source.replace("u32 item,u32 item", "u32 item");
    let error = &check_errors(&fixed).errors[0];
    assert!(error.message.ends_with("`done`"));
    assert_eq!(error.span.column, fixed.rfind("done").unwrap() + 1);
    assert_eq!(error.end.as_ref().unwrap().column, error.span.column + 4);
    let fixed = fixed.replace(",done;", ";");
    let error = &check_errors(&fixed).errors[0];
    assert!(error.message.ends_with("`value`"));
    assert!(error.end.is_some());
    let source = "trait reader {type item;type item;control read(u32 value,u32 value) exits done;}";
    assert!(check_errors(source).errors[0].message.ends_with("`item`"));
    let source = "implement absent for missing {} trait reader {control read(u32 item,u32 item) exits done;}";
    assert!(check_errors(source).errors[0].message.ends_with("`item`"));
    let source = "trait reader {control read(u32 item) exits done;} implement reader for u32 {control read(u32 item,u32 item) exits done {exit done;}} implement absent for missing {}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.ends_with("`item`"));
    let fixed = source.replace("read(u32 item,u32 item)", "read(u32 item)");
    assert!(
        check_errors(&fixed).errors[0]
            .message
            .contains("unknown trait")
    );
    let source = "trait reader {control read(u32 item) exits done;} implement reader for missing {control read(u32 item,u32 item) exits done {exit done;}}";
    assert!(
        check_errors(source).errors[0]
            .message
            .contains("unknown or unapplied")
    );
    let f = Fixture::new();
    f.write("types.tellur", "pub struct wrapper {u32 value;}");
    let source = "import \"types.tellur\" as types;trait reader {control read(u32 item,types.missing item) exits done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_NAME");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "types.missing")
    );
}

#[test]
fn trait_signature_duplicate_ranges_keep_independent_batches_and_limits() {
    for implementations in [false, true] {
        let mut source = if implementations {
            "trait reader {control read(u32 item) exits done;}\n".to_owned()
        } else {
            String::new()
        };
        for n in (0..36).rev() {
            if implementations {
                source.push_str(&format!("struct value_{n:02} {{u32 value;}} implement reader for value_{n:02} {{control read(u32 item,u32 item) exits done(u32 part,u32 part) {{exit done;}}}}\n"));
            } else {
                source.push_str(&format!("trait reader_{n:02} {{control read(u32 item,u32 item) exits done(u32 part,u32 part);}}\n"));
            }
        }
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 32, "{errors}");
        assert!(errors.limit_reached);
        for (n, error) in errors.errors.iter().enumerate() {
            assert_eq!(error.span.line, n + 1 + usize::from(implementations));
            assert!(error.message.ends_with("`item`"));
            assert_eq!(error.end.as_ref().unwrap().column, error.span.column + 4);
        }
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
    let source = format!(
        "trait reader {{control read(u32 item) exits done;}}\n{}",
        "implement reader for u32 {control read(u32 item,u32 item) exits done {exit done;}}\n"
            .repeat(1025)
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 1);
    assert!(!errors.limit_reached);
    assert!(errors.errors[0].message.contains("1024-implementation"));
    assert!(errors.errors[0].end.is_none());
}

#[test]
fn trait_signature_duplicate_ranges_leave_ordinary_controls_separate() {
    for (source, code, message) in [
        (
            "control ordinary(u32 item,u32 item) exits done {exit done;}",
            "E_NAME",
            "duplicate parameter",
        ),
        (
            "control ordinary() exits done(u32 item,u32 item) {exit done(1,2);}",
            "E_NAME",
            "duplicate exit field",
        ),
        (
            "trait reader {control ordinary() exits done,done;}",
            "E_TRAIT",
            "duplicate trait member or parameter `done`",
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code);
        assert_eq!(errors.errors[0].message, message);
        if code == "E_TRAIT" {
            let at = source.rfind("done").unwrap();
            assert_eq!(errors.errors[0].span.column, at + 1);
            assert_eq!(errors.errors[0].end.as_ref().unwrap().column, at + 5);
        } else {
            assert!(errors.errors[0].end.is_none());
        }
    }
}

#[test]
fn trait_exit_duplicate_ranges_select_complete_written_names() {
    let f = Fixture::new();
    let marker = "// selected 😀\r\n ";
    for (canonical, written, payload) in [
        ("done", "done", ""),
        ("finished_long2", "finished_long2", "(u32 value)"),
        ("reply.done", "reply.done", "{u32 value}"),
        ("reply.done", "reply // 😀\r\n . done", "(u32 value)"),
    ] {
        for implementation in [false, true] {
            for wrapped in [false, true] {
                let exits =
                    format!("{canonical}{payload}, {marker}{written} // trailing\r\n {payload}");
                let exits = if wrapped {
                    format!("{{{exits}}}")
                } else {
                    exits
                };
                let signature = format!("control read() exits {exits}");
                let source = if implementation {
                    format!(
                        "trait reader {{control read() exits done;}} implement reader for u32 {{{signature} {{exit done;}}}}"
                    )
                } else {
                    format!("trait reader {{{signature};}}")
                };
                f.write("main.tellur", &source);
                let report = f.report(&["check", "main.tellur", "--json"], false);
                let errors = report["diagnostics"].as_array().unwrap();
                assert_eq!(errors.len(), 1, "{report}");
                let at = source.find(marker).unwrap() + marker.len();
                assert_eq!(errors[0]["code"], "E_TRAIT");
                assert_eq!(
                    errors[0]["message"],
                    format!("duplicate trait member or parameter `{canonical}`")
                );
                assert_eq!(
                    errors[0]["range"],
                    json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+written.len())})
                );
                let batch = check_errors(&source);
                assert_eq!(
                    compile(
                        &source,
                        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
                    )
                    .unwrap_err(),
                    batch.errors[0]
                );
            }
        }
    }
}

#[test]
fn trait_exit_duplicate_ranges_preserve_imported_coordinates_and_repairs() {
    let f = Fixture::new();
    f.write("middle.tellur", "import \"worker.tellur\" as worker;");
    f.write(
        "main.tellur",
        "import \"middle.tellur\" as middle;control main() exits done {exit done;}",
    );
    let valid = "pub trait reader {control read(self input) exits done;}\r\nimplement reader for u32 {control read(self first) exits done {string note=\"😀\";drop(note);exit done;}} implement reader for string {control read(self second) exits done {exit done;}}\r\npub control work() exits done {reader<u32>.read(1);reader<string>.read(\"x\");exit done;}";
    for (needle, replacement, selected) in [
        ("exits done;", "exits done, // selected\r\n done;", "done"),
        (
            "read(self first) exits done",
            "read(self first) exits {done, // selected\r\n done}",
            "done",
        ),
        (
            "read(self second) exits done",
            "read(self second) exits reply.done, // selected\r\n reply.done",
            "reply.done",
        ),
        (
            "read(self second) exits done",
            "read(self second) exits {reply.done, // selected\r\n reply // 😀\r\n .done}",
            "reply // 😀\r\n .done",
        ),
    ] {
        let source = valid.replace(needle, replacement);
        f.write("worker.tellur", &source);
        let report = f.report(&["check", "main.tellur", "--json"], false);
        let error = &report["diagnostics"][0];
        assert_eq!(
            report["diagnostics"].as_array().unwrap().len(),
            1,
            "{report}"
        );
        assert_eq!(error["code"], "E_TRAIT");
        assert_eq!(
            error["path"],
            f.root.join("worker.tellur").to_string_lossy().as_ref()
        );
        let at = source.find("// selected\r\n ").unwrap() + "// selected\r\n ".len();
        assert!(source[at..].starts_with(selected));
        assert_eq!(
            error["range"],
            json!({"start":scalar_position(&source,at),"end":scalar_position(&source,at+selected.len())})
        );
        let first = tellur::compile_file(
            f.root.join("main.tellur"),
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap_err();
        assert_eq!(first.path, f.root.join("worker.tellur"));
        assert_eq!(
            json!({"line":first.diagnostic.span.line,"column":first.diagnostic.span.column}),
            error["position"]
        );
        let end = first.diagnostic.end.as_ref().unwrap();
        assert_eq!(
            json!({"line":end.line,"column":end.column}),
            error["range"]["end"]
        );
        f.write("worker.tellur", valid);
        f.report(&["check", "main.tellur", "--json"], true);
    }
}

#[test]
fn trait_exit_duplicate_ranges_keep_profile_phase_and_identity_priorities() {
    let profile = "trait reader {control read<t>() exits done,done;}";
    let error = &check_errors(profile).errors[0];
    assert_eq!(error.code, "E_TRAIT_SIGNATURE");
    assert!(error.end.is_none());
    let source =
        "trait reader {control read(u32 item,u32 item) exits done(u32 value,u32 value),done;}";
    let error = &check_errors(source).errors[0];
    assert!(error.message.ends_with("`item`"));
    let fixed = source.replace("u32 item,u32 item", "u32 item");
    let error = &check_errors(&fixed).errors[0];
    assert!(error.message.ends_with("`done`"));
    assert_eq!(error.span.column, fixed.rfind("done").unwrap() + 1);
    let fixed = fixed.replace(",done;", ";");
    assert!(check_errors(&fixed).errors[0].message.ends_with("`value`"));
    let source = "trait reader {type item;type item;control read() exits done,done;}";
    assert!(check_errors(source).errors[0].message.ends_with("`item`"));
    let source = "implement absent for missing {} trait reader {control read() exits done,done;}";
    assert!(check_errors(source).errors[0].message.ends_with("`done`"));
    let source = "trait reader {control read() exits done;} implement reader for u32 {control read() exits done,done {exit done;}} implement absent for missing {}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert!(errors.errors[0].message.ends_with("`done`"));
    let fixed = source.replace("exits done,done", "exits done");
    assert!(
        check_errors(&fixed).errors[0]
            .message
            .contains("unknown trait")
    );
    let source = "trait reader {control read() exits done;} implement reader for missing {control read() exits done,done {exit done;}}";
    assert!(
        check_errors(source).errors[0]
            .message
            .contains("unknown or unapplied")
    );
    let source = "trait reader {control read() exits done;} implement reader for u32 {control read() exits done {exit done;}} implement reader for u32 {control read() exits done,done {exit done;}}";
    assert!(check_errors(source).errors[0].message.ends_with("`done`"));
    let fixed = source.replace("exits done,done", "exits done");
    let error = &check_errors(&fixed).errors[0];
    assert_eq!(error.code, "E_COHERENCE");
    assert!(error.end.is_none());
    let f = Fixture::new();
    f.write("types.tellur", "pub struct wrapper {u32 value;}");
    let source = "import \"types.tellur\" as types;trait reader {control read() exits done(types.missing item),done;}";
    f.write("main.tellur", source);
    let report = f.report(&["check", "main.tellur", "--json"], false);
    assert_eq!(report["diagnostics"][0]["code"], "E_NAME");
    assert_eq!(
        report["diagnostics"][0]["range"],
        selected_range(source, "types.missing")
    );
}

#[test]
fn trait_exit_duplicate_ranges_keep_independent_batches_and_limits() {
    for implementations in [false, true] {
        let mut source = if implementations {
            "trait reader {control read() exits done;control other() exits done;}\n".to_owned()
        } else {
            String::new()
        };
        for n in (0..36).rev() {
            if implementations {
                source.push_str(&format!("struct value_{n:02} {{u32 value;}} implement reader for value_{n:02} {{control read() exits done(u32 part,u32 part),done {{exit done;}} control other() exits done,done {{exit done;}}}}\n"));
            } else {
                source.push_str(&format!("trait reader_{n:02} {{control read() exits done(u32 part,u32 part),done;control other() exits done,done;}}\n"));
            }
        }
        let errors = check_errors(&source);
        assert_eq!(errors.errors.len(), 32, "{errors}");
        assert!(errors.limit_reached);
        for (n, error) in errors.errors.iter().enumerate() {
            assert_eq!(error.span.line, n + 1 + usize::from(implementations));
            assert!(error.message.ends_with("`done`"));
            assert_eq!(error.end.as_ref().unwrap().column, error.span.column + 4);
            let line = source.lines().nth(error.span.line - 1).unwrap();
            assert_eq!(error.span.column, line.find(",done").unwrap() + 2);
        }
        assert_eq!(
            compile(
                &source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err(),
            errors.errors[0]
        );
    }
    let source = format!(
        "trait reader {{control read() exits done;}}\n{}",
        "implement reader for u32 {control read() exits done,done {exit done;}}\n".repeat(1025)
    );
    let errors = check_errors(&source);
    assert_eq!(errors.errors.len(), 1);
    assert!(!errors.limit_reached);
    assert!(errors.errors[0].message.contains("1024-implementation"));
    assert!(errors.errors[0].end.is_none());
}

#[test]
fn trait_exit_duplicate_ranges_leave_ordinary_and_nominal_validation_separate() {
    for (source, code, message) in [
        (
            "control ordinary() exits done,done {exit done;}",
            "E_NAME",
            "duplicate exit",
        ),
        (
            "outcome result {done,done} trait reader {control read() exits result;}",
            "E_MANIFEST",
            "invalid or duplicate name `done`",
        ),
        (
            "variant result {done,done} trait reader {control read() exits done;}",
            "E_MANIFEST",
            "invalid or duplicate name `done`",
        ),
    ] {
        let errors = check_errors(source);
        assert_eq!(errors.errors.len(), 1, "{errors}");
        assert_eq!(errors.errors[0].code, code);
        assert_eq!(errors.errors[0].message, message);
        assert!(errors.errors[0].end.is_none());
    }
}

#[test]
fn trait_exit_duplicate_ranges_keep_trait_validation_before_variant_schemas() {
    let source = "variant result {done,done} trait reader {control read() exits done,done;}";
    let errors = check_errors(source);
    assert_eq!(errors.errors.len(), 1);
    assert_eq!(errors.errors[0].code, "E_TRAIT");
    assert_eq!(
        errors.errors[0].span.column,
        source.rfind("done").unwrap() + 1
    );
}
