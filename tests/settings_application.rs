use serde_json::{Value as Json, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tellur::{
    compile_file,
    ir::Value,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};
fn program() -> tellur::CheckedProgram {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("applications/settings");
    let manifest = Manifest::from_json_at(
        &fs::read_to_string(root.join("manifest.json")).unwrap(),
        &root.join("manifest.json"),
    )
    .unwrap();
    compile_file(root.join("main.tellur"), manifest).unwrap()
}
fn text(s: &str) -> Value {
    Value::String(s.into())
}
fn call(op: &str, out: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: op.into(),
        outcome: out.into(),
        fields,
    }
}
fn failure_cases() -> Vec<(Scenario, &'static str)> {
    let args = vec![text("settings.json"), text("theme"), text("dark")];
    let base = vec![
        call("text_trim", "trimmed", vec![text("theme")]),
        call("file_read", "missing", vec![]),
        call("json_encode", "encoded", vec![text("encoded")]),
    ];
    let mut cases = vec![];
    for (out, expected, fields) in [
        ("write_failed", "storage_failed", vec![text("disk full")]),
        (
            "commit_uncertain",
            "commit_uncertain",
            vec![text("rename uncertain; cleanup failed")],
        ),
        ("too_large", "too_large", vec![]),
        ("replaced", "saved", vec![]),
    ] {
        let mut calls = base.clone();
        calls.push(call("file_replace", out, fields));
        cases.push((
            Scenario {
                entry: "set".into(),
                arguments: args.clone(),
                calls,
            },
            expected,
        ));
    }
    for (out, expected, fields) in [
        (
            "read_failed",
            "storage_failed",
            vec![text("permission denied")],
        ),
        ("too_large", "too_large", vec![]),
    ] {
        cases.push((
            Scenario {
                entry: "set".into(),
                arguments: args.clone(),
                calls: vec![base[0].clone(), call("file_read", out, fields)],
            },
            expected,
        ));
    }
    cases.push((
        Scenario {
            entry: "set".into(),
            arguments: vec![text("settings.json"), text(" bad "), text("x")],
            calls: vec![call("text_trim", "trimmed", vec![text("bad")])],
        },
        "invalid_key",
    ));
    cases.push((
        Scenario {
            entry: "set".into(),
            arguments: args,
            calls: vec![
                base[0].clone(),
                call("file_read", "read", vec![text("invalid")]),
                call("json_decode", "invalid", vec![text("invalid JSON")]),
            ],
        },
        "invalid_file",
    ));
    cases
}
#[test]
fn settings_policy_keeps_failures_and_uncertain_effects_explicit() {
    let p = program();
    let expanded = p.expanded("set").unwrap();
    for (scenario, expected) in failure_cases() {
        let result = p.run(&scenario).unwrap();
        assert_eq!(result.outcome, expected);
        assert_eq!(result, expanded.run(&scenario).unwrap());
    }
}
fn status(outcome: &str) -> i32 {
    match outcome {
        "done" | "saved" | "replaced" => 0,
        "invalid_key" => 2,
        "missing" => 3,
        "commit_uncertain" => 4,
        _ => 1,
    }
}
fn run(binary: &Path, entry: &str, args: &[&str]) -> Json {
    let output = Command::new(binary)
        .args(["--json", "--entry", entry, "--"])
        .args(args)
        .output()
        .unwrap();
    let report: Json = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    assert_eq!(
        output.status.code(),
        Some(status(report["outcome"].as_str().unwrap()))
    );
    report
}
#[test]
fn native_settings_persists_reloads_and_matches_scripted_failures() {
    let p = program();
    let root = std::env::temp_dir().join(format!("tellur-settings-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        for opt in [0, 2] {
            let program = if expanded {
                p.expanded("set").unwrap()
            } else {
                p.clone()
            };
            let binary = root.join(format!("settings-{expanded}-{opt}"));
            program.build_native("main", &binary, opt).unwrap();
            let data = root.join(format!("data-{expanded}-{opt}.json"));
            let path = data.to_str().unwrap();
            assert_eq!(run(&binary, "get", &[path, "theme"])["outcome"], "missing");
            assert_eq!(
                run(&binary, "set", &[path, "theme", "dark"])["outcome"],
                "saved"
            );
            assert_eq!(
                serde_json::from_str::<Json>(&fs::read_to_string(&data).unwrap()).unwrap(),
                json!({"theme":"dark"})
            );
            let read = run(&binary, "get", &[path, "theme"]);
            assert_eq!(read["outcome"], "done");
            assert!(
                read["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["message"] == "dark")
            );
            let saved = fs::read(&data).unwrap();
            assert_eq!(
                run(&binary, "set", &[path, " bad ", "x"])["outcome"],
                "invalid_key"
            );
            assert_eq!(fs::read(&data).unwrap(), saved);
            let huge = "x".repeat(1048577);
            fs::write(&data, huge).unwrap();
            assert_eq!(run(&binary, "get", &[path, "theme"])["outcome"], "failed");
            for invalid in ["{", r#"{"x":1}"#, r#"{"x":"first","x":"second"}"#] {
                fs::write(&data, invalid).unwrap();
                assert_eq!(
                    run(&binary, "set", &[path, "theme", "x"])["outcome"],
                    "invalid_file"
                );
                assert_eq!(fs::read_to_string(&data).unwrap(), invalid);
            }
            for (i, (scenario, expected)) in failure_cases().iter().enumerate() {
                let file = root.join(format!("case-{i}.json"));
                fs::write(&file, serde_json::to_vec(scenario).unwrap()).unwrap();
                let output = Command::new(&binary)
                    .arg("--scenario")
                    .arg(file)
                    .output()
                    .unwrap();
                assert_eq!(output.status.code(), Some(status(expected)));
                assert_eq!(
                    serde_json::from_slice::<Json>(&output.stdout).unwrap(),
                    serde_json::to_value(p.run(scenario).unwrap()).unwrap()
                );
            }
            assert!(!fs::read_dir(&root).unwrap().any(|p| {
                p.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tellur-settings-")
            }));
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_file_replacement_reports_size_preparation_and_uncertain_commit_failures() {
    let root = std::env::temp_dir().join(format!("tellur-settings-write-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("applications/settings");
    let manifest = Manifest::from_json_at(
        &fs::read_to_string(directory.join("manifest.json")).unwrap(),
        &directory.join("manifest.json"),
    )
    .unwrap();
    let mut manifest = manifest;
    manifest.launch.clear();
    let p=tellur::compile("control main(string path,string text) exits auto {forward file_replace(borrow path,borrow text,4);}",manifest).unwrap();
    let p = p
        .with_entry_exit_codes(
            "main",
            [
                ("replaced", 0),
                ("write_failed", 1),
                ("too_large", 1),
                ("commit_uncertain", 4),
            ]
            .into_iter()
            .map(|(n, c)| (n.into(), c))
            .collect(),
        )
        .unwrap();
    let binary = root.join("write");
    p.build_native("main", &binary, 0).unwrap();
    let data = root.join("data.json");
    fs::write(&data, "old").unwrap();
    assert_eq!(
        run(&binary, "main", &[data.to_str().unwrap(), "large"])["outcome"],
        "too_large"
    );
    assert_eq!(fs::read_to_string(&data).unwrap(), "old");
    assert_eq!(
        run(
            &binary,
            "main",
            &[root.join("missing/file").to_str().unwrap(), "new"]
        )["outcome"],
        "write_failed"
    );
    let directory = root.join("destination-directory");
    fs::create_dir(&directory).unwrap();
    assert_eq!(
        run(&binary, "main", &[directory.to_str().unwrap(), "new"])["outcome"],
        "commit_uncertain"
    );
    assert!(directory.is_dir());
    assert_eq!(
        run(&binary, "main", &[data.to_str().unwrap(), "new"])["outcome"],
        "replaced"
    );
    assert_eq!(fs::read_to_string(&data).unwrap(), "new");
    assert!(!fs::read_dir(&root).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tellur-settings-")
    }));
    fs::remove_dir_all(root).unwrap();
}
