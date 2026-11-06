mod support;

use serde_json::{Value as Json, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{
    CheckedProgram, compile_file,
    ir::Value,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tellur-json-library-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/json_file/manifest.json");
    let mut manifest = Manifest::from_json_at(&fs::read_to_string(&path).unwrap(), &path).unwrap();
    manifest.launch.clear();
    manifest
}
fn library_program(f: &Fixture) -> CheckedProgram {
    fs::write(
        f.0.join("json_file.tellur"),
        include_str!("../library/json_file.tellur"),
    )
    .unwrap();
    fs::write(f.0.join("main.tellur"), r#"
        import "json_file.tellur" as file;
        control read(string path, u32 limit) exits file.load_result {
            forward file.load(borrow path, limit);
        }
        control write(string path, map<string, string> values, u32 limit) exits file.replace_result {
            forward file.replace(borrow path, borrow values, limit);
        }
    "#).unwrap();
    compile_file(f.0.join("main.tellur"), manifest()).unwrap()
}
fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn map() -> Value {
    Value::Construct {
        ty: "map<String,String>".into(),
        name: "map".into(),
        fields: vec![text("a"), text("b")],
    }
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits: 0,
    }
}

#[test]
fn library_preserves_missing_corruption_preparation_and_uncertain_commit() {
    let f = Fixture::new();
    let p = library_program(&f);
    let mut cases = Vec::new();
    for (outcome, fields) in [
        ("missing", vec![]),
        ("too_large", vec![]),
        ("read_failed", vec![text("read")]),
    ] {
        cases.push((
            "read",
            vec![text("data.json"), Value::U32(17)],
            vec![call("file_read", outcome, fields.clone())],
            outcome,
            fields,
        ));
    }
    for (outcome, expected, fields) in [
        ("decoded", "loaded", vec![map()]),
        ("invalid", "invalid", vec![text("duplicate")]),
    ] {
        cases.push((
            "read",
            vec![text("data.json"), Value::U32(17)],
            vec![
                call("file_read", "read", vec![text("json")]),
                call("json_decode", outcome, fields.clone()),
            ],
            expected,
            fields,
        ));
    }
    cases.push((
        "write",
        vec![text("data.json"), map(), Value::U32(17)],
        vec![call("json_encode", "encode_failed", vec![text("encode")])],
        "encode_failed",
        vec![text("encode")],
    ));
    for (outcome, expected, fields) in [
        ("replaced", "saved", vec![]),
        ("too_large", "too_large", vec![]),
        ("write_failed", "write_failed", vec![text("prepare")]),
        (
            "commit_uncertain",
            "commit_uncertain",
            vec![text("rename; cleanup failed")],
        ),
    ] {
        cases.push((
            "write",
            vec![text("data.json"), map(), Value::U32(17)],
            vec![
                call("json_encode", "encoded", vec![text("json")]),
                call("file_replace", outcome, fields.clone()),
            ],
            expected,
            fields,
        ));
    }
    for (entry, arguments, calls, expected, fields) in cases {
        let scenario = Scenario {
            entry: entry.into(),
            arguments,
            calls,
        };
        let result = p.run(&scenario).unwrap();
        assert_eq!(result.outcome, expected);
        assert_eq!(result.fields, fields);
        assert_eq!(result, p.expanded(entry).unwrap().run(&scenario).unwrap());
    }
}

#[test]
fn shared_library_builds_without_http_and_enforces_exact_byte_limits() {
    let f = Fixture::new();
    let p = support::scenario_executable(&library_program(&f));
    let binary = f.0.join("json-file");
    p.build_native("read", &binary, 0).unwrap();
    let data = f.0.join("data.json");
    let path = data.to_str().unwrap();
    let run = |entry: &str, args: &[&str]| -> Json {
        let output = Command::new(&binary)
            .args(["--json", "--entry", entry, "--"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    assert_eq!(run("read", &[path, "0"])["outcome"], "missing");
    fs::write(&data, "{}").unwrap();
    assert_eq!(run("read", &[path, "2"])["outcome"], "loaded");
    assert_eq!(run("read", &[path, "1"])["outcome"], "too_large");
    fs::write(&data, "{\"é\":\"🙂\"}").unwrap();
    let size = fs::metadata(&data).unwrap().len();
    assert_eq!(run("read", &[path, &size.to_string()])["outcome"], "loaded");
    assert_eq!(
        run("read", &[path, &(size - 1).to_string()])["outcome"],
        "too_large"
    );
    for invalid in [r#"{"a":"first","a":"second"}"#, r#"{"a":7}"#, "{} trailing"] {
        fs::write(&data, invalid).unwrap();
        assert_eq!(run("read", &[path, "100"])["outcome"], "invalid");
    }
    fs::write(&data, [0xff]).unwrap();
    assert_eq!(run("read", &[path, "1"])["outcome"], "read_failed");
    let values = serde_json::to_string(&map()).unwrap();
    let encoded = serde_json::to_string_pretty(&json!({"a":"b"})).unwrap();
    assert_eq!(
        run("write", &[path, &values, &(encoded.len() - 1).to_string()])["outcome"],
        "too_large"
    );
    assert_eq!(fs::read(&data).unwrap(), [0xff]);
    assert_eq!(
        run("write", &[path, &values, &encoded.len().to_string()])["outcome"],
        "saved"
    );
    assert_eq!(fs::read_to_string(&data).unwrap(), encoded);
    assert_eq!(run("read", &[path, "100"])["outcome"], "loaded");
    let directory = f.0.join("directory");
    fs::create_dir(&directory).unwrap();
    assert_eq!(
        run("write", &[directory.to_str().unwrap(), &values, "100"])["outcome"],
        "commit_uncertain"
    );
    assert!(!fs::read_dir(&f.0).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tellur-file-")
    }));
}
