use tellur::{
    compile,
    extract::{Extraction, extract_tail},
    ir::Value,
    manifest::Manifest,
    runtime::Scenario,
};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn extract(source: &str, marker: &str) -> Extraction {
    let line = source.lines().position(|l| l.contains(marker)).unwrap() + 1;
    extract_tail(source, m(), "main", line, "extracted").unwrap()
}
fn same(source: &str, extraction: &Extraction, args: Vec<Value>) {
    let a = compile(source, m()).unwrap();
    let b = compile(&extraction.source, m()).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: args,
        calls: vec![],
    };
    let expected = a.run(&scenario).unwrap();
    assert_eq!(expected, b.run(&scenario).unwrap());
    assert_eq!(
        expected,
        b.expanded("main").unwrap().run(&scenario).unwrap()
    );
}
const OWNED: &str = r#"// Preserve this header.
control main(bool available) exits auto {
    string prefix = "before";
    emit(prefix);
    string text = "payload";
    exit delta available {
        true => ready {text},
        false => empty {}
    };
}
// Preserve this other control.
control other() exits done {exit done;}
"#;
#[test]
fn extracts_owned_tail_with_typed_inputs_and_preserves_surrounding_source() {
    let e = extract(OWNED, "exit delta available");
    assert_eq!(
        e.inputs
            .iter()
            .map(|i| (&*i.name, &*i.ty))
            .collect::<Vec<_>>(),
        vec![("available", "Bool"), ("text", "String")]
    );
    assert!(e.source.contains(
        "control extracted(bool available, string text) exits { ready { string value_0 }, empty }"
    ));
    assert!(
        e.source
            .starts_with(OWNED.split("    exit delta").next().unwrap())
    );
    assert!(
        e.source
            .contains("// Preserve this other control.\ncontrol other() exits done {exit done;}")
    );
    assert!(e.source.contains("    exit delta available {\n        true => ready {text},\n        false => empty {}\n    };"));
    assert!(
        e.source
            .lines()
            .nth(e.helper_line - 1)
            .unwrap()
            .starts_with("control extracted")
    );
    for flag in [false, true] {
        same(OWNED, &e, vec![Value::Bool(flag)]);
    }
}
#[test]
fn mutable_input_is_restored_as_an_explicit_mutable_local_without_name_collision() {
    let source = r#"control main() exits auto {
    mutable string text="before";
    string extracted_text="keep";
    text=concat(text,extracted_text);
    exit done {text};
}
"#;
    let e = extract(source, "text=concat");
    assert!(e.source.contains("mutable string text = extracted_text_;"));
    same(source, &e, vec![]);
}
#[test]
fn borrowed_input_return_keeps_its_owner_origin() {
    let source = "control main(&string text) exits auto {\n    exit found {text};\n}\n";
    let e = extract(source, "exit found");
    let before = compile(source, m()).unwrap();
    let after = compile(&e.source, m()).unwrap();
    assert_eq!(
        before.region("main").unwrap().borrow_returns,
        after.region("main").unwrap().borrow_returns
    );
    assert_eq!(e.inputs[0].ty, "&String");
    let harness = "control run() exits auto {string owner=\"hello\";exit delta main(&owner) {found {auto text}=>done {clone(text)}};}";
    let scenario = Scenario {
        entry: "run".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        compile(&format!("{source}{harness}"), m())
            .unwrap()
            .run(&scenario)
            .unwrap(),
        compile(&format!("{}{harness}", e.source), m())
            .unwrap()
            .expanded("run")
            .unwrap()
            .run(&scenario)
            .unwrap()
    );
}
#[test]
fn resource_inputs_are_transferred_without_cloning_or_discarding() {
    let source =
        "control main(handle file) exits ready {handle file} {\n    exit ready {file};\n}\n";
    let e = extract(source, "exit ready");
    same(
        source,
        &e,
        vec![Value::Opaque {
            ty: "handle".into(),
            token: "file".into(),
        }],
    );
    assert!(e.source.contains("forward extracted(file)"));
}
#[test]
fn copied_tail_contains_local_paths_and_whole_loops() {
    let source = "control main(bool flag) exits done {\n    delta flag {true=>yes,false=>no}\n    [yes]:emit(\"yes\");\n    [yes|no]:exit done;\n}\n";
    let e = extract(source, "delta flag");
    for flag in [false, true] {
        same(source, &e, vec![Value::Bool(flag)]);
    }
    let source = "control main() exits done {\n    loop repeat(bool flag=true) {\n        delta flag {true=>again,false=>end}\n        [again]:again repeat(false);\n        [end]:exit done;\n    }\n}\n";
    let e = extract(source, "loop repeat");
    same(source, &e, vec![]);
    assert!(e.inputs.is_empty());
    let source = "control main() exits done {\n    path selected = entry;\n    [selected]:emit(\"alias\");exit done;\n}\n";
    let e = extract(source, "path selected");
    same(source, &e, vec![]);
}
#[test]
fn named_outcomes_and_inline_pattern_locals_keep_their_scopes() {
    let source = "outcome message {ready {string text},empty}\ncontrol main(bool flag) exits message {\n    message saved=delta flag {true=>ready {\"hello\"},false=>empty {}};\n    exit delta saved {ready {auto text}=>ready {text},empty=>forward};\n}\n";
    let e = extract(source, "exit delta saved");
    assert_eq!(e.inputs.len(), 1);
    assert_eq!(e.inputs[0].name, "saved");
    assert!(
        e.source
            .contains("control extracted(message saved) exits message")
    );
    for flag in [false, true] {
        same(source, &e, vec![Value::Bool(flag)]);
    }
}
#[test]
fn external_path_dependencies_partial_moves_and_live_alias_pairs_are_rejected() {
    let cases = [
        (
            "control main(bool flag) exits done {\n delta flag {true=>yes,false=>no}\n [entry]:emit(\"cut\");\n [yes|no]:exit done;\n}\n",
            3,
        ),
        (
            "control main() exits done {\n string owner=\"hello\";\n auto view=&owner;\n emit(view);exit done;\n}\n",
            4,
        ),
        (
            "control main(bool flag) exits done {\n delta flag {true=>yes,false=>no}\n [yes]:emit(\"cut\");\n [entry]:exit done;\n}\n",
            3,
        ),
    ];
    for (s, line) in cases {
        compile(s, m()).unwrap();
        assert_eq!(
            extract_tail(s, m(), "main", line, "helper")
                .unwrap_err()
                .code,
            "E_EXTRACT"
        );
    }
    // Moving a partially consumed record as a whole into a helper is forbidden.
    let mut manifest = m();
    manifest.types.push(tellur::manifest::TypeDef::Record {
        name: "pair".into(),
        fields: vec![
            tellur::manifest::Field {
                name: "left".into(),
                ty: "String".into(),
            },
            tellur::manifest::Field {
                name: "right".into(),
                ty: "String".into(),
            },
        ],
    });
    let source = "control main(pair value) exits auto {\n string left=value.left;\n exit ready {value.right};\n}\n";
    compile(source, manifest.clone()).unwrap();
    assert!(
        extract_tail(source, manifest, "main", 3, "helper")
            .unwrap_err()
            .message
            .contains("E_PARTIAL_MOVE")
    );
}
#[test]
fn unicode_crlf_comments_and_entry_selectors_preserve_text_boundaries() {
    let source = "// 😀\r\ncontrol main() exits done {\r\n    [entry]: emit(\"😀 braces { }\"); // Keep me.\r\n    exit done;\r\n}\r\n";
    let e = extract(source, "[entry]");
    assert!(
        e.source
            .starts_with("// 😀\r\ncontrol main() exits done {\r\n")
    );
    assert!(
        e.source
            .contains("emit(\"😀 braces { }\"); // Keep me.\r\n")
    );
    same(source, &e, vec![]);
}
#[test]
fn invalid_selection_and_name_collisions_are_diagnosed_without_modifying_source() {
    for (line, name) in [
        (0, "helper"),
        (999, "helper"),
        (2, "helper"),
        (6, "main"),
        (6, "u32_text"),
        (6, "bad-name"),
    ] {
        assert_eq!(
            extract_tail(OWNED, m(), "main", line, name)
                .unwrap_err()
                .code,
            "E_EXTRACT"
        );
    }
    let same_line = "control main() exits done {\n emit(\"one\");emit(\"two\");exit done;\n}\n";
    assert!(
        extract_tail(same_line, m(), "main", 2, "helper")
            .unwrap_err()
            .message
            .contains("multiple statements")
    );
}
#[test]
fn private_inferred_capture_types_require_a_named_boundary() {
    let source = "control main() exits done {\n auto saved=delta true {true=>ready {1},false=>empty {}};\n exit delta saved {ready {auto n}=>done {},empty=>done {}};\n}\n";
    assert!(
        extract_tail(source, m(), "main", 3, "helper")
            .unwrap_err()
            .message
            .contains("named outcome")
    );
}
#[test]
fn extraction_cli_emits_checked_source_and_machine_readable_report() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let file = std::env::temp_dir().join(format!("path-extract-{}.path", std::process::id()));
    std::fs::write(&file, OWNED).unwrap();
    for report in [false, true] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"));
        command
            .arg("extract-tail")
            .arg(&file)
            .arg(root.join("playground/manifest.json"))
            .args(["main", "6", "extracted"]);
        if report {
            command.arg("--report");
        }
        let out = command.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        if report {
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(
                value["claim"],
                "rechecked_tail_extraction_preserving_public_contracts"
            );
        } else {
            compile(std::str::from_utf8(&out.stdout).unwrap(), m()).unwrap();
        }
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), OWNED);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn runnable_example_preserves_native_behavior_at_o0_o2_before_and_after_expansion() {
    use std::{fs, process::Command};
    let source = include_str!("../playground/extraction/original.tellur");
    let extracted = extract_tail(source, m(), "greeting", 12, "finish_greeting").unwrap();
    assert_eq!(
        extracted.source,
        include_str!("../playground/extraction/extracted.tellur")
    );
    let original = compile(source, m()).unwrap();
    let candidate = compile(&extracted.source, m()).unwrap();
    let expanded = candidate
        .expanded("main")
        .unwrap()
        .expanded("greeting")
        .unwrap();
    let scenarios: Vec<Scenario> =
        serde_json::from_str(include_str!("../playground/extraction/scenarios.json")).unwrap();
    let expected = scenarios
        .iter()
        .map(|s| serde_json::to_value(original.run(s).unwrap()).unwrap())
        .collect::<Vec<_>>();
    let root = std::env::temp_dir().join(format!("path-extraction-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for (name, program) in [
        ("original", original),
        ("extracted", candidate),
        ("expanded", expanded),
    ] {
        for opt in [0, 2] {
            let binary = root.join(format!("{name}-{opt}"));
            program.build_native("main", &binary, opt).unwrap();
            for (scenario, expected) in scenarios.iter().zip(&expected) {
                assert_eq!(
                    serde_json::to_value(program.run(scenario).unwrap()).unwrap(),
                    *expected
                );
                let file = root.join("scenario.json");
                fs::write(&file, serde_json::to_vec(scenario).unwrap()).unwrap();
                let output = Command::new(&binary)
                    .arg("--scenario")
                    .arg(file)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                    *expected,
                    "{name} O{opt} {}",
                    scenario.entry
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
