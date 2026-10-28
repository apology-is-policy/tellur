use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, fs, process::Command};
use tellur::{compile, compile_file, manifest::Manifest};

fn manifest() -> Manifest {
    Manifest::from_json(r#"{"version":"0.1","types":[],"primitives":[]}"#).unwrap()
}
fn codes(values: &[(&str, u8)]) -> BTreeMap<String, u8> {
    values.iter().map(|(n, c)| ((*n).into(), *c)).collect()
}

#[test]
fn libraries_need_no_status_but_executables_require_every_declared_alternative() {
    let p = compile("control main() exits done, unused {exit done;}", manifest()).unwrap();
    assert_eq!(p.native_source("main").unwrap_err().code, "E_EXIT_CODE");
    let e = p
        .with_entry_exit_codes("main", codes(&[("done", 0)]))
        .unwrap_err();
    assert!(e.message.contains("unused"));
    let p = p
        .with_entry_exit_codes("main", codes(&[("done", 0), ("unused", 3)]))
        .unwrap();
    assert!(p.native_source("main").is_ok());
    assert!(
        p.with_entry_exit_codes("main", codes(&[("typo", 1)]))
            .unwrap_err()
            .message
            .contains("unknown outcome")
    );
    assert!(
        p.entry_exit_codes("typo")
            .unwrap_err()
            .message
            .contains("unknown executable entry")
    );
    let borrowed = compile("outcome result {@exit 0 done} control peek(ref string text) exits result {emit(text);exit done;}", manifest()).unwrap();
    assert!(
        borrowed
            .entry_exit_codes("peek")
            .unwrap_err()
            .message
            .contains("borrowed arguments")
    );
}

#[test]
fn maintained_playgrounds_declare_complete_executable_contracts() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest =
        Manifest::from_json(&fs::read_to_string(root.join("playground/manifest.json")).unwrap())
            .unwrap();
    let mut pending = vec![root.join("playground")];
    let mut checked = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().unwrap() != "build"
                    && !path.file_name().unwrap().to_string_lossy().starts_with('.')
                {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "tellur") {
                let p = compile_file(&path, manifest.clone()).unwrap();
                if p.region("main").is_some() {
                    p.native_source("main")
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    checked += 1;
                }
                if path.file_name().unwrap() == "port.tellur" {
                    assert_eq!(
                        p.entry_exit_codes("parse_port").unwrap(),
                        codes(&[("ready", 0), ("invalid", 2)])
                    );
                }
            }
        }
    }
    assert!(checked >= 40, "only checked {checked} examples");
}

#[test]
fn final_contract_controls_status_without_changing_forward_compatibility() {
    let p = compile(
        r#"
        outcome inner { @exit 9 ready { string text } }
        outcome outer { @exit 0 ready { string text } }
        control leaf() exits inner {exit ready { "value" };}
        control main() exits outer {forward leaf();}
        control inferred() exits auto {forward leaf();}
        control stored() exits outer {auto saved = capture leaf(); exit saved;}
    "#,
        manifest(),
    )
    .unwrap();
    assert_eq!(p.entry_exit_codes("leaf").unwrap(), codes(&[("ready", 9)]));
    for entry in ["main", "stored"] {
        assert_eq!(p.entry_exit_codes(entry).unwrap(), codes(&[("ready", 0)]));
        assert_eq!(
            p.expanded(entry).unwrap().entry_exit_codes(entry).unwrap(),
            codes(&[("ready", 0)])
        );
    }
    // Inference carries the shape, never hidden host policy from a producer.
    assert!(p.entry_exit_codes("inferred").is_err());
}

#[test]
fn overrides_replace_defaults_and_complete_partial_annotations() {
    let p = compile(
        "outcome result {@exit 0 done, missing} control main() exits result {exit done;}",
        manifest(),
    )
    .unwrap();
    assert!(p.entry_exit_codes("main").is_err());
    let p = p
        .with_entry_exit_codes("main", codes(&[("missing", 0)]))
        .unwrap();
    assert_eq!(
        p.entry_exit_codes("main").unwrap(),
        codes(&[("done", 0), ("missing", 0)])
    );
    let p = p
        .with_entry_exit_codes("main", codes(&[("done", 7), ("missing", 255)]))
        .unwrap();
    assert_eq!(
        p.entry_exit_codes("main").unwrap(),
        codes(&[("done", 7), ("missing", 255)])
    );
}

#[test]
fn annotations_are_validated_at_their_source_location() {
    for annotation in [
        "@exit 256",
        "@exit nope",
        "@exit 1u32",
        "@unknown 0",
        "@exit 0 @exit 1",
    ] {
        let source = format!("outcome result {{\n  {annotation} done\n}}");
        let e = compile(&source, manifest()).unwrap_err();
        assert_eq!(e.code, "E_EXIT_CODE", "{annotation}: {e}");
        assert_eq!(e.span.line, 2);
    }
    assert!(
        compile(
            "outcome result {@exit 0 done, @exit 255 failed}",
            manifest()
        )
        .is_ok()
    );
    assert!(compile("variant result {@exit 0 done}", manifest()).is_err());
}

#[test]
fn generic_outcome_instances_keep_their_annotations() {
    let p = compile("outcome result<t> {@exit 0 ready {t value}, @exit 4 missing} control main() exits result<string> {exit ready {\"yes\"};}", manifest()).unwrap();
    assert_eq!(
        p.entry_exit_codes("main").unwrap(),
        codes(&[("ready", 0), ("missing", 4)])
    );
}

#[test]
fn manifest_policies_reject_duplicate_keys_bad_ranges_and_unknown_entries() {
    for launch in [
        r#"{"main":{"exit_codes":{"done":256}}}"#,
        r#"{"main":{"exit_codes":{"done":-1}}}"#,
        r#"{"main":{"exit_codes":{"done":0,"done":1}}}"#,
        r#"{"main":{"exit_codes":{}},"main":{"exit_codes":{}}}"#,
        r#"{"main":{"exit_codes":{},"typo":true}}"#,
    ] {
        let text = format!(r#"{{"version":"0.1","types":[],"primitives":[],"launch":{launch}}}"#);
        assert!(Manifest::from_json(&text).is_err(), "{launch}");
    }
    let mut m = manifest();
    m.launch.insert(
        "typo".into(),
        tellur::launch::EntryConfiguration {
            exit_codes: codes(&[("done", 0)]),
        },
    );
    let p = compile(
        "outcome result {@exit 0 done} control main() exits result {exit done;}",
        m,
    )
    .unwrap();
    assert!(
        p.native_source("main")
            .unwrap_err()
            .message
            .contains("typo")
    );
}

#[test]
fn imported_contracts_use_module_identity_and_aliases_preserve_defaults() {
    let root = std::env::temp_dir().join(format!("tellur-status-modules-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("a.tellur"),
        "pub outcome result {@exit 0 success}",
    )
    .unwrap();
    fs::write(
        root.join("b.tellur"),
        "pub outcome result {@exit 7 success}",
    )
    .unwrap();
    fs::write(
        root.join("main.tellur"),
        r#"
        import "a.tellur" as a; import "a.tellur" as alias; import "b.tellur" as b;
        control main() exits a.result {exit success;}
        control second() exits b.result {exit success;}
        control third() exits alias.result {exit success;}
    "#,
    )
    .unwrap();
    let p = compile_file(root.join("main.tellur"), manifest()).unwrap();
    for (entry, code) in [("main", 0), ("second", 7), ("third", 0)] {
        assert_eq!(
            p.entry_exit_codes(entry).unwrap(),
            codes(&[("success", code)])
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_statuses_preserve_payloads_adapters_and_expansion_at_o0_o2() {
    let source = r#"
        outcome producer {@exit 9 missing}
        outcome result {@exit 0 done {string text}, @exit 23 missing {string text}}
        control leaf() exits producer {exit missing;}
        control main(bool available) exits result {
            leaf();
            exit delta available {true => done {"present"}, false => missing {"absent"}};
        }
        control hidden() exits done {exit done;}
        outcome largest {@exit 255 done}
        control maximum() exits largest {exit done;}
        control fault() exits result {loop forever() {again forever();}}
    "#;
    let p = compile(source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("tellur-status-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("program-{expanded}-{opt}"));
            p.build_native("main", &binary, opt).unwrap();
            for (arg, status, name, payload) in [
                ("true", 0, "done", "present"),
                ("false", 23, "missing", "absent"),
            ] {
                let output = Command::new(&binary)
                    .args(["--json", "--", arg])
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(status),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let report: Json = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(report["outcome"], name);
                assert_eq!(report["fields"][0]["value"], payload);
            }
            let maximum = Command::new(&binary)
                .args(["--entry", "maximum"])
                .output()
                .unwrap();
            assert_eq!(maximum.status.code(), Some(255));
            let help = Command::new(&binary).arg("--help").output().unwrap();
            assert!(help.status.success());
            assert!(!String::from_utf8_lossy(&help.stdout).contains("hidden"));
            let output = Command::new(&binary)
                .args(["--entry", "fault", "--fuel", "10"])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(String::from_utf8_lossy(&output.stderr).contains("E_FUEL"));
            let output = Command::new(&binary)
                .args(["--entry", "hidden"])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(String::from_utf8_lossy(&output.stderr).contains("E_ENTRY"));
        }
    }
    // The compiler command must return the child's numeric status without turning
    // a declared completion into a compiler error, including --json execution.
    fs::write(root.join("main.tellur"), source).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_string(&manifest()).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .args([
            "exec",
            root.join("main.tellur").to_str().unwrap(),
            "--",
            "--json",
            "--",
            "false",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], json!("missing"));
    fs::remove_dir_all(root).unwrap();
}
