mod support;
use serde_json::{Value as Json, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{compile, manifest::Manifest};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-rust-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.lock"),
            include_str!("../examples/retained_file/Cargo.lock"),
        )
        .unwrap();
        Self { root }
    }
    fn manifest(&self, value: &Json) -> Manifest {
        Manifest::from_json_at(&value.to_string(), &self.root.join("manifest.json")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn config() -> Json {
    json!({"version":"0.1","types":[],"rust":{"source":"host.rs","lockfile":"Cargo.lock"},"primitives":[{
        "name":"inspect","parameters":[{"name":"value","type":"string","mode":"borrow"}],
        "outcomes":[{"name":"ready","fields":[{"name":"value","type":"string"}]}],
        "effects":[],"returns":true,"rust":{"execution":"immediate"}
    }]})
}
#[test]
fn adapter_contracts_require_explicit_supported_binding_metadata() {
    for (edit, code) in [
        (0, "E_RUST_ADAPTER"),
        (1, "E_MANIFEST"),
        (2, "E_RUST_ADAPTER"),
        (3, "E_RUST_ADAPTER"),
        (4, "E_RUST_ADAPTER"),
        (5, "E_RUST_ADAPTER"),
    ] {
        let mut m = config();
        match edit {
            0 => {
                m.as_object_mut().unwrap().remove("rust");
            }
            1 => m["primitives"][0]["rust"]["execution"] = json!("async"),
            2 => m["primitives"][0]["outcomes"][0]["fields"][0]["type"] = json!("ref string"),
            3 => m["primitives"][0]["pure"] = json!(true),
            4 => m["rust"]["dependencies"] = json!({"serde":{"version":"1"}}),
            5 => m["primitives"][0]["name"] = json!("self"),
            _ => unreachable!(),
        }
        assert_eq!(
            Manifest::from_json(&m.to_string()).unwrap_err().code,
            code,
            "case {edit}"
        );
    }
}
#[test]
fn checking_does_not_read_or_execute_rust_and_paths_follow_the_manifest() {
    let f = Fixture::new();
    let manifest = f.manifest(&config());
    assert_eq!(
        manifest.rust.as_ref().unwrap().source,
        f.root.join("host.rs")
    );
    let p = compile("control main() exits done {exit done;}", manifest).unwrap();
    assert!(
        support::scenario_executable(&p)
            .native_source("main")
            .unwrap_err()
            .message
            .contains("cannot read Rust adapter")
    );
    let unbased = compile(
        "control main() exits done {exit done;}",
        Manifest::from_json(&config().to_string()).unwrap(),
    )
    .unwrap();
    assert!(
        support::scenario_executable(&unbased)
            .native_source("main")
            .unwrap_err()
            .message
            .contains("unresolved")
    );
}
#[test]
fn explicit_module_paths_are_validated_without_reading_sources() {
    let f = Fixture::new();
    let mut m = config();
    m["rust"]["modules"] = json!({"helpers":"shared files/é.rs"});
    let manifest = f.manifest(&m);
    assert_eq!(
        manifest.rust.as_ref().unwrap().modules["helpers"],
        f.root.join("shared files/é.rs")
    );
    fs::write(
        f.root.join("host.rs"),
        "// the child has not been created yet",
    )
    .unwrap();
    let p = compile("control main() exits done {exit done;}", manifest).unwrap();
    let error = support::scenario_executable(&p)
        .native_source("main")
        .unwrap_err();
    assert_eq!(error.code, "E_RUST_ADAPTER");
    assert!(error.message.contains("module `helpers`"));
    assert!(error.message.contains("shared files/é.rs"));
    for (name, path) in [
        ("self", "helper.rs"),
        ("nested::name", "helper.rs"),
        ("_", "helper.rs"),
        ("helper", ""),
        ("helper", "Cargo.lock"),
    ] {
        m["rust"]["modules"] = json!({name:path});
        assert_eq!(
            Manifest::from_json(&m.to_string()).unwrap_err().code,
            "E_RUST_ADAPTER",
            "{name}: {path}"
        );
    }
    let duplicate = config().to_string().replace(
        "\"source\":\"host.rs\"",
        "\"source\":\"host.rs\",\"modules\":{\"helper\":\"a.rs\",\"helper\":\"b.rs\"}",
    );
    assert_eq!(
        Manifest::from_json(&duplicate).unwrap_err().code,
        "E_MANIFEST"
    );
}

#[test]
fn native_modules_embed_explicit_sources_and_keep_wrapper_checks() {
    let f = Fixture::new();
    let mut m = config();
    // Keyword module names work as raw Rust identifiers; paths are independent
    // of the generated output directory and the process working directory.
    m["rust"]["modules"] = json!({"type":"shared files/é.rs", "suffix":"suffix.rs"});
    fs::create_dir(f.root.join("shared files")).unwrap();
    fs::write(f.root.join("host.rs"), "pub use self::r#type::inspect;").unwrap();
    let child = f.root.join("shared files/é.rs");
    fs::write(&child, "//! Embedded child documentation.\npub fn inspect(value:&str)->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(format!(\"{}{}\",value,super::suffix::text()))}\n// trailing comment").unwrap();
    fs::write(
        f.root.join("suffix.rs"),
        "pub fn text()-> &'static str {\"!\"}",
    )
    .unwrap();
    let p = compile(
        "control main(string input) exits auto {forward inspect(borrow input);}",
        f.manifest(&m),
    )
    .unwrap();
    let before = support::scenario_executable(&p)
        .native_source("main")
        .unwrap();
    assert!(before.contains("mod r#type {"));
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        let binary = f.root.join(format!("module-{expanded}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, 0)
            .unwrap();
        let output = Command::new(binary)
            .args(["--json", "--", "hello"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Json = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report["fields"][0],
            json!({"kind":"string","value":"hello!"})
        );
    }
    fs::write(
        f.root.join("suffix.rs"),
        "pub fn text()-> &'static str {\"?\"}",
    )
    .unwrap();
    assert_ne!(
        before,
        support::scenario_executable(&p)
            .native_source("main")
            .unwrap()
    );
    fs::write(&child, "pub fn inspect(_:u32)->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(String::new())}").unwrap();
    let error = support::scenario_executable(&p)
        .build_native("main", &f.root.join("wrong-module"), 0)
        .unwrap_err();
    assert_eq!(error.code, "E_BACKEND");
    assert!(error.message.contains("mismatched types"), "{error}");
}
#[test]
fn native_rust_signature_mismatch_is_a_build_error() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"),"pub fn inspect(value:u32)->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(value.to_string())}").unwrap();
    let p = compile(
        "control main() exits done {exit done;}",
        f.manifest(&config()),
    )
    .unwrap();
    let e = support::scenario_executable(&p)
        .build_native("main", &f.root.join("wrong"), 0)
        .unwrap_err();
    assert_eq!(e.code, "E_BACKEND");
    assert!(e.message.contains("mismatched types"), "{e}");
}
#[test]
fn native_dependencies_are_locked_and_rust_library_calls_round_trip() {
    let f = Fixture::new();
    let mut m = config();
    m["rust"]["dependencies"] = json!({"memchr":{"version":"=2.8.3","default_features":false}});
    fs::write(f.root.join("host.rs"),"pub fn inspect(value:&str)->crate::rust_api::inspect_outcome {let at=memchr::memchr(b'!',value.as_bytes()).unwrap();crate::rust_api::inspect_outcome::ready(value[..at].to_owned())}").unwrap();
    let p = compile(
        "control main(string input) exits ready {string value} {forward inspect(borrow input);}",
        f.manifest(&m),
    )
    .unwrap();
    let locked = fs::read(f.root.join("Cargo.lock")).unwrap();
    let e = support::scenario_executable(&p)
        .build_native("main", &f.root.join("out"), 0)
        .unwrap_err();
    assert_eq!(e.code, "E_BACKEND");
    assert!(e.message.contains("locked"), "{e}");
    assert_eq!(fs::read(f.root.join("Cargo.lock")).unwrap(), locked);
    support::scenario_executable(&p)
        .lock_native("main", &f.root.join("out"))
        .unwrap();
    let new_lock = fs::read(f.root.join("Cargo.lock")).unwrap();
    assert_ne!(locked, new_lock);
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = f.root.join(format!("out-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let result = Command::new(binary)
                .args(["--json", "--", "hello!"])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let actual: Json = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(actual["outcome"], "ready");
            assert_eq!(
                actual["fields"][0],
                json!({"kind":"string","value":"hello"})
            );
        }
    }
    assert_eq!(fs::read(f.root.join("Cargo.lock")).unwrap(), new_lock);
}
#[test]
fn native_owned_and_borrowed_collections_use_concrete_rust_types() {
    let f = Fixture::new();
    let mut m = config();
    m["primitives"][0]["parameters"] = json!([
        {"name":"data","type":"bytes","mode":"borrow"},
        {"name":"items","type":"vector<u32>","mode":"borrow"},
        {"name":"map","type":"map<bool,vector<string>>","mode":"consume"},
        {"name":"unit","type":"unit","mode":"value"}
    ]);
    m["primitives"][0]["outcomes"][0]["fields"] = json!([
        {"name":"data","type":"bytes"},{"name":"items","type":"vector<u32>"},
        {"name":"map","type":"map<bool,vector<string>>"},{"name":"unit","type":"unit"}
    ]);
    fs::write(f.root.join("host.rs"),"pub fn inspect(data:&[u8],items:&[u32],map:std::collections::BTreeMap<bool,Vec<String>>,unit:())->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(data.to_vec(),items.to_vec(),map,unit)}").unwrap();
    let p=compile("control main(bytes data) exits auto {vector<u32> items=[1,2];mutable map<bool,vector<string>> values=map {};delta map_insert(borrow mutable values,true,[\"hi\"]) {inserted=>inserted,replaced {vector<string> previous}=>replaced}[inserted | replaced]:forward inspect(borrow data,borrow items,values,unit);}",f.manifest(&m)).unwrap();
    let binary = f.root.join("out");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    let result = Command::new(binary)
        .args(["--json", "--", r#"{"kind":"bytes","value":[1,255]}"#])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let actual: Json = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(actual["fields"][0], json!({"kind":"bytes","value":[1,255]}));
    assert_eq!(
        actual["fields"][1]["value"]["fields"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        actual["fields"][2]["value"]["fields"][0],
        json!({"kind":"bool","value":true})
    );
    assert_eq!(actual["fields"][3], json!({"kind":"unit"}));
}

#[test]
fn native_panics_are_not_recoverable_outcomes_and_scripts_do_not_execute_adapters() {
    let f = Fixture::new();
    fs::write(
        f.root.join("host.rs"),
        "pub fn inspect(_: &str)->crate::rust_api::inspect_outcome {panic!(\"host failure\")}",
    )
    .unwrap();
    let p = compile(
        "control main() exits auto {forward inspect(borrow \"input\");}",
        f.manifest(&config()),
    )
    .unwrap();
    let binary = f.root.join("panic");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    let result = Command::new(&binary).arg("--json").output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("host failure"));
    let scenario = f.root.join("scenario.json");
    fs::write(&scenario,json!({"entry":"main","arguments":[],"calls":[{"operation":"inspect","outcome":"ready","fields":[{"kind":"string","value":"scripted"}]}]}).to_string()).unwrap();
    let result = Command::new(&binary)
        .arg("--scenario")
        .arg(scenario)
        .output()
        .unwrap();
    assert!(result.status.success());
    let result: Json = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["fields"][0]["value"], "scripted");
}

fn resource_config() -> Json {
    json!({"version":"0.1", "types":[{"name":"counter","kind":"opaque","linear":true}],
    "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"counter":"Counter"}},
    "primitives":[
        {"name":"make","parameters":[{"name":"path","type":"string","mode":"borrow"}],"outcomes":[{"name":"made","fields":[{"name":"value","type":"counter"}]}],"effects":["make"],"returns":true,"rust":{"execution":"immediate"}},
        {"name":"inspect","parameters":[{"name":"left","type":"counter","mode":"borrow"},{"name":"right","type":"counter","mode":"borrow"}],"outcomes":[{"name":"number","fields":[{"name":"value","type":"u32"}]}],"effects":[],"returns":true,"rust":{"execution":"immediate"}},
        {"name":"handoff","parameters":[{"name":"left","type":"counter","mode":"borrow"},{"name":"right","type":"counter","mode":"consume"}],"outcomes":[{"name":"retained","fields":[{"name":"value","type":"counter"}]}],"effects":["handoff"],"returns":true,"rust":{"execution":"immediate"}},
        {"name":"close","parameters":[{"name":"value","type":"counter","mode":"consume"},{"name":"fail","type":"bool","mode":"value"}],"outcomes":[{"name":"closed","fields":[]},{"name":"failed","fields":[]}],"effects":["close"],"returns":true,"rust":{"execution":"immediate"}}
    ]})
}
const RESOURCE_HOST: &str = r#"
use crate::rust_api;
pub struct Counter {path:String, value:u32}
impl Drop for Counter {fn drop(&mut self) {use std::io::Write; let mut file=std::fs::OpenOptions::new().create(true).append(true).open(&self.path).unwrap();writeln!(file,"drop {}",self.value).unwrap();}}
pub fn make(path:&str)->rust_api::make_outcome {rust_api::make_outcome::made(Counter{path:path.into(),value:7})}
pub fn inspect(left:&Counter,right:&Counter)->rust_api::inspect_outcome {assert!(std::ptr::eq(left,right));rust_api::inspect_outcome::number(left.value+right.value)}
pub fn handoff(left:&Counter,mut right:Counter)->rust_api::handoff_outcome {right.value+=left.value;rust_api::handoff_outcome::retained(right)}
pub fn close(value:Counter,fail:bool)->rust_api::close_outcome {drop(value);if fail {rust_api::close_outcome::failed}else{rust_api::close_outcome::closed}}
"#;
const RESOURCE_SOURCE: &str = r#"
control move_between(ref counter left, counter right) exits retained {counter value} {
 forward handoff(left,right);
}
control main(string path) exits auto {
 delta make(borrow path) {made {counter left}=>left_ready}
 [left_ready]: delta make(borrow path) {made {counter right}=>right_ready}
 [right_ready]:
  delta inspect(borrow left,borrow left) {number {u32 value}=>observed}
 [observed]:
  delta move_between(borrow left,right) {retained {counter moved}=>handed}
 [handed]:
  delta close(left,false) {closed=>left_closed,failed=>left_failed}
 [handed]:
  delta close(moved,true) {closed=>right_closed,failed=>right_failed}
 [left_closed & right_closed]: exit done {value};
 [left_failed | right_failed]: exit failed {value};
}
"#;

#[test]
fn resource_bindings_require_linear_opaque_direct_boundary_types() {
    for case in 0..7 {
        let mut m = resource_config();
        match case {
            0 => m["types"][0]["linear"] = json!(false),
            1 => m["rust"]["resources"] = json!({"missing":"Counter"}),
            2 => m["rust"]["resources"] = json!({"counter":"std::fs::File"}),
            3 => m["types"][0] = json!({"name":"counter","kind":"record","fields":[]}),
            4 => m["primitives"][0]["outcomes"][0]["fields"][0]["type"] = json!("vector<counter>"),
            5 => m["primitives"][1]["parameters"][0]["type"] = json!("ref counter"),
            6 => m["primitives"][1]["parameters"][0]["mode"] = json!("value"),
            _ => unreachable!(),
        }
        let e = Manifest::from_json(&m.to_string()).unwrap_err();
        assert_eq!(
            e.code,
            if case == 6 {
                "E_MANIFEST"
            } else {
                "E_RUST_ADAPTER"
            },
            "case {case}: {e}"
        );
    }
    let m = resource_config().to_string().replace(
        "\"resources\":{\"counter\":\"Counter\"}",
        "\"resources\":{\"counter\":\"Counter\",\"counter\":\"Other\"}",
    );
    assert!(
        Manifest::from_json(&m)
            .unwrap_err()
            .message
            .contains("duplicate")
    );
}

#[test]
fn resource_ownership_is_checked_without_reading_the_adapter() {
    let f = Fixture::new();
    compile(RESOURCE_SOURCE, f.manifest(&resource_config())).unwrap();
    for body in [
        "exit done;",
        "drop value; exit done;",
        "auto copy=clone value; exit done;",
        "delta close(value,false) {closed=>closed,failed=>failed} [closed | failed]: delta inspect(borrow value,borrow value) {number {u32 n}=>observed} [observed]: exit done;",
        "delta handoff(borrow value,value) {retained {counter again}=>retained} [retained]: exit done {again};",
    ] {
        let source = format!(
            "control main(string path) exits auto {{delta make(borrow path) {{made {{counter value}}=>made}} [made]: {body}}}"
        );
        assert!(
            compile(&source, f.manifest(&resource_config())).is_err(),
            "{body}"
        );
    }
}

#[test]
fn retained_rust_objects_survive_borrows_transfer_and_explicit_failed_cleanup() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let p = compile(RESOURCE_SOURCE, f.manifest(&resource_config())).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = f.root.join(format!("resources-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let log = f.root.join(format!("drops-{expanded}-{opt}"));
            let result = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(fs::read_to_string(&log).unwrap(), "drop 7\ndrop 14\n");
            let actual: Json = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(actual["outcome"], "failed");
            assert_eq!(actual["fields"][0], json!({"kind":"u32","value":14}));
            let events = actual["events"].as_array().unwrap();
            let old = &events[1]["fields"][0];
            let new = &events[3]["fields"][0];
            assert_ne!(
                old, new,
                "consumed and returned resources receive fresh identities"
            );
        }
    }
}

#[test]
fn resource_host_teardown_releases_escaped_objects_and_fuel_faults() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let p = compile(
        "control main(string path) exits auto {forward make(borrow path);}",
        f.manifest(&resource_config()),
    )
    .unwrap();
    let binary = f.root.join("escape");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    let log = f.root.join("escaped");
    let result = Command::new(&binary)
        .args(["--json", "--"])
        .arg(&log)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
    let p = compile(RESOURCE_SOURCE, f.manifest(&resource_config())).unwrap();
    let binary = f.root.join("fuel");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    let mut observed = false;
    for fuel in 1..20 {
        let log = f.root.join(format!("fuel-{fuel}"));
        let result = Command::new(&binary)
            .args(["--json", "--fuel", &fuel.to_string(), "--"])
            .arg(&log)
            .output()
            .unwrap();
        if !result.status.success() && log.is_file() {
            let report: tellur::fault::Report = serde_json::from_slice(&result.stderr).unwrap();
            assert!(report.message.contains("E_FUEL"));
            assert_eq!(report.phase, tellur::fault::Phase::Execution);
            assert!(!report.activations.is_empty());
            assert!(result.stdout.is_empty());
            let text = fs::read_to_string(log).unwrap();
            assert!(text == "drop 7\n" || text == "drop 7\ndrop 7\n");
            observed = true;
            break;
        }
    }
    assert!(observed, "exercise a fault after resource acquisition");
}

#[test]
fn live_entry_rejects_nested_resource_tokens_before_acquisition() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let source = r#"
    struct box {counter value;}
    control main(box supplied,string path) exits auto {
      delta make(borrow path) {made {counter fresh}=>acquired}
      [acquired]: exit delta close(supplied.value,false) {
        closed=>done {fresh}, failed=>done {fresh}
      };
    }"#;
    let p = compile(source, f.manifest(&resource_config())).unwrap();
    let binary = f.root.join("inject");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    let log = f.root.join("must-not-exist");
    let forged = json!({"kind":"construct","value":{"ty":"box","name":"box","fields":[{"kind":"opaque","value":{"ty":"counter","token":"rust:0"}}]}});
    let result = Command::new(&binary)
        .args(["--json", "--", &forged.to_string()])
        .arg(&log)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("live resource arguments cannot be supplied as JSON"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!log.exists(), "no wrapper may run before rejection");
}

#[test]
fn retained_file_example_runs_live_and_scripted_failure_paths() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/retained_file");
    let manifest = Manifest::from_json_at(
        &fs::read_to_string(root.join("manifest.json")).unwrap(),
        &root.join("manifest.json"),
    )
    .unwrap();
    let p = tellur::compile_file(root.join("main.tellur"), manifest).unwrap();
    let f = Fixture::new();
    let binary = f.root.join("file");
    p.build_native("main", &binary, 0).unwrap();
    let input = f.root.join("input");
    fs::write(&input, b"hello").unwrap();
    let result = Command::new(&binary)
        .args(["--json", "--"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let actual: Json = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(actual["outcome"], "data");
    assert_eq!(
        actual["fields"][0],
        json!({"kind":"bytes","value":b"hello"})
    );
    for (out, fields, expected) in [
        ("end", json!([]), "end"),
        (
            "failed",
            json!([{"kind":"string","value":"read failure"}]),
            "read_failed",
        ),
    ] {
        let scenario = json!({"entry":"main","arguments":[{"kind":"string","value":"no real file"}],"calls":[
            {"operation":"file_open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"input_file","token":"scripted"}}]},
            {"operation":"file_chunk","outcome":out,"fields":fields},
            {"operation":"file_release","outcome":"released","fields":[]}
        ]});
        let scenario = scenario.to_string();
        fs::write(f.root.join("scenario.json"), &scenario).unwrap();
        let reference = p.run(&serde_json::from_str(&scenario).unwrap()).unwrap();
        assert_eq!(reference.outcome, expected);
        let result = Command::new(&binary)
            .arg("--scenario")
            .arg(f.root.join("scenario.json"))
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(if out == "failed" { 1 } else { 0 }),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual: Json = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(actual["outcome"], expected);
        assert_eq!(actual["events"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn resource_types_and_operations_can_be_reexported_from_an_explicit_module() {
    let f = Fixture::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/retained_file");
    let mut config: Json =
        serde_json::from_str(include_str!("../examples/retained_file/manifest.json")).unwrap();
    config["rust"]["modules"] = json!({"files":root.join("host.rs")});
    fs::write(
        f.root.join("host.rs"),
        "pub use self::files::{InputFile, file_open, file_chunk, file_release};",
    )
    .unwrap();
    let p = compile(
        include_str!("../examples/retained_file/main.tellur"),
        f.manifest(&config),
    )
    .unwrap();
    let binary = f.root.join("reexported-resource");
    p.build_native("main", &binary, 0).unwrap();
    let input = f.root.join("input.txt");
    fs::write(&input, b"owned module resource").unwrap();
    let output = Command::new(&binary)
        .args(["--json", "--"])
        .arg(input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "data");
    assert_eq!(
        report["fields"][0],
        json!({"kind":"bytes","value":b"owned module resource"})
    );
}

#[test]
fn explicit_fault_release_checks_ownership_and_releases_real_retained_owners() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let m = f.manifest(&config);
    for source in [
        "control main(counter owned) faults {fault \"broken\";}",
        "struct box {counter owned;}control main(box owned) faults {fault \"broken\";}",
        "outcome result {ready {counter owned},missing}control main(result value) faults {fault \"broken\";}",
        "control main(option<counter> owned) faults {fault \"broken\";}",
        "control stop(ref counter input) faults {fault \"broken\";}control main(counter owned) faults {stop(borrow owned);}",
    ] {
        compile(source, m.clone()).unwrap();
    }
    assert_eq!(
        compile(
            "control main(counter owned) exits done {exit done;}",
            m.clone()
        )
        .unwrap_err()
        .code,
        "E_RESOURCE"
    );
    let source = "control stop(counter child) faults {fault \"broken\";}control main(string path) faults {delta make(borrow path) {made {counter parent}=>first} [first]:delta make(borrow path) {made {counter child}=>second} [second]:stop(child);}";
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, m).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        let binary = f.root.join(format!("source-fault-{expanded}"));
        // No normal outcomes means no process-outcome mapping is needed.
        p.build_native("main", &binary, 2).unwrap();
        let log = f.root.join(format!("source-fault-drops-{expanded}"));
        let result = Command::new(&binary)
            .args(["--json", "--"])
            .arg(&log)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        let report: tellur::fault::Report = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(report.kind, tellur::fault::Kind::SourceFault);
        assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\ndrop 7\n");
    }
    for invalid in ["no_rust", "not_linear", "no_owner"] {
        let mut value = config.clone();
        match invalid {
            "no_rust" => {
                value.as_object_mut().unwrap().remove("rust");
            }
            "not_linear" => value["types"][0]["linear"] = json!(false),
            _ => value["rust"]["resources"] = json!({}),
        }
        assert!(
            Manifest::from_json_at(&value.to_string(), &f.root.join("manifest.json")).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn contingency_handover_releases_child_locals_and_preserves_parent_resources() {
    let f = Fixture::new();
    let host=RESOURCE_HOST.replace("pub fn close(value:Counter,fail:bool)->rust_api::close_outcome {drop(value);", "pub fn close(value:Counter,fail:bool)->rust_api::close_outcome {use std::io::Write;writeln!(std::fs::OpenOptions::new().append(true).create(true).open(&value.path).unwrap(),\"close\").unwrap();drop(value);");
    fs::write(f.root.join("host.rs"), host).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    config["primitives"][1]["contingent"] = json!(true);
    config["primitives"][3]["contingent"] = json!(true);
    let source = r#"
    boundary child(counter retained, string path) retains retained {
        delta make(borrow path) {made {counter local}=>ready}
        [ready]:fault "request broke";
    } contingency(string reason) {
        delta inspect(borrow retained,borrow retained) {number {u32 value}=>ready}
        [ready]:emit("resource alive");
        delta close(retained,false) {closed=>closed,failed=>failed}
        [closed|failed]:terminate;
    }
    control main(string path) exits done {
        delta make(borrow path) {made {counter parent}=>parent_ready}
        [parent_ready]:delta make(borrow path) {made {counter retained}=>child_ready}
        [child_ready]:supervise child(retained,clone(borrow path));
        delta inspect(borrow parent,borrow parent) {number {u32 value}=>observed}
        [observed]:emit("resource alive");
        delta close(parent,false) {closed=>closed,failed=>failed}
        [closed|failed]:exit done;
    }"#;
    let p = compile(source, f.manifest(&config)).unwrap();
    let binary = f.root.join("contingency");
    support::scenario_executable(&p)
        .build_native("main", &binary, 2)
        .unwrap();
    let log = f.root.join("drops");
    let output = Command::new(binary)
        .args(["--json", "--"])
        .arg(&log)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["outcome"], "done");
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "drop 7\nclose\ndrop 7\nclose\ndrop 7\n"
    );
}

#[test]
fn constructor_faults_release_initialized_fields_and_pending_arguments_once() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
 struct partial {counter first;counter second;string unfinished;
  constructor(string path) {
   delta make(borrow path) {made {counter value}=>first}
   [first]:self.first=move value;
   delta make(borrow path) {made {counter next}=>second}
   [second]:self.second=move next;fault "constructor failed";
  }
 }
 struct pending {counter owned;partial value;constructor(counter owned,partial value){self.owned=move owned;self.value=move value;}}
 control main(string path) exits done {
  fault boundary {
   delta make(borrow path) {made {counter retained}=>made}
   [made]:auto value=pending(move retained,partial(path));
   fault "unreachable";
  } contingency [] {terminate process 23;}
 }
 "#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config)).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = f.root.join(format!("constructor-fault-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        let log = f.root.join(format!("constructor-drop-{opt}"));
        let output = Command::new(binary)
            .args(["--json", "--"])
            .arg(&log)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\ndrop 7\ndrop 7\n");
    }
}

#[test]
fn completed_constructor_transfers_fault_ownership_to_caller() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
 struct complete {
  counter owned;
  constructor(counter owned) {self.owned=move owned;}
 }
 control main(string path) exits done {
  fault boundary {
   delta make(borrow path) {made {counter owned}=>made}
   [made]:auto value=complete(move owned);
   fault "after successful construction";
  } contingency [] {terminate process 23;}
 }
 "#;
    let p = compile(source, f.manifest(&config)).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = f.root.join(format!("completed-constructor-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        let log = f.root.join(format!("completed-constructor-drop-{opt}"));
        let output = Command::new(binary)
            .args(["--json", "--"])
            .arg(&log)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
    }
}

#[test]
fn borrowed_constructor_faults_release_resources_before_and_after_delivery() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
struct borrowed_owner['a] {
    ref string text <- 'a;
    counter owned;
    constructor(ref string text, counter owned, bool broken)[text] {
        self.text = text;
        self.owned = move owned;
        delta broken {true => fault "partial", false => complete}
        [complete]: emit(self.text);
    }
}

control main(string path, bool broken) exits done {
    fault boundary {
        string text = "external reference";
        delta make(borrow path) {made {counter owned} => made}
        [made]: auto value = borrowed_owner(borrow text, move owned, broken);
        fault "after delivery";
    } contingency [] {terminate process 23;}
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config)).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = f.root.join(format!("borrowed-constructor-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        for broken in ["true", "false"] {
            let log = f
                .root
                .join(format!("borrowed-constructor-{opt}-{broken}.log"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
        }
    }
}

#[test]
fn raw_borrowed_assembly_faults_keep_pending_and_delivered_resources_owned() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
struct borrowed_owner['a] {
    ref string text <- 'a;
    counter owned;
    constructor(ref string text, counter owned)[text] {
        self.text = text;
        self.owned = move owned;
    }
}
struct marker {
    bool done;
    constructor(bool broken) {
        delta broken {true => fault "later initializer", false => complete}
        [complete]: self.done = true;
    }
}
struct group['a] {counter first; borrowed_owner['a] second; marker third;}
variant packet['a] {packed {group['a] value}, empty}
control main(string path, bool broken) exits done {
    fault boundary {
        string text = "external reference";
        delta make(borrow path) {made {counter first} => first}
        [first]: delta make(borrow path) {made {counter second} => second}
        [second]: packet value = packed {group {
            first: move first,
            second: borrowed_owner(borrow text, move second),
            third: marker(broken)
        }};
        fault "after assembly";
    } contingency [] {terminate process 23;}
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config)).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = f.root.join(format!("raw-borrowed-assembly-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        for broken in ["true", "false"] {
            let log = f
                .root
                .join(format!("raw-borrowed-assembly-{opt}-{broken}.log"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\ndrop 7\n");
        }
    }
}

#[test]
fn pending_raw_resource_needs_a_fault_disposition_even_when_assembly_never_completes() {
    let f = Fixture::new();
    let source = r#"
struct stop {bool value;constructor(){fault "last initializer";}}
struct pending['a] {counter owned;ref string text <- 'a;stop last;}
control main(string path) exits done {counter remaining} {
    delta make(borrow path) {made {counter owned}=>made}
    [made]: string text="live";
    auto value=pending {move owned,borrow text,stop()};
    exit done {value.owned};
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    compile(source, f.manifest(&config)).unwrap();
}

#[test]
fn projected_resource_keeps_fault_ownership_before_and_after_delivery() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
struct owner['a] {
    ref string text <- 'a;
    counter owned;
    constructor(ref string input,counter owned,bool broken)[input] {
        self.text=input;
        self.owned=move owned;
        delta broken {true=>fault "partial",false=>complete}
    }
}
control main(string path,bool broken) exits done {
    fault boundary {
        string text="reference";
        delta make(borrow path) {made {counter value}=>made}
        [made]: auto projected=owner(borrow text,move value,broken).owned;
        fault "after projection";
    } contingency [] {terminate process 23;}
}
"#;
    assert_eq!(
        compile(
            &source.replace("broken).owned", "broken).text"),
            f.manifest(&config)
        )
        .unwrap_err()
        .code,
        "E_TEMPORARY"
    );
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config)).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = f.root.join(format!("projected-resource-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        for broken in ["true", "false"] {
            let log = f
                .root
                .join(format!("projected-resource-{opt}-{broken}.log"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
        }
    }
}

#[test]
fn sequential_initializer_retirement_preserves_pending_resource_fault_ownership() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
struct mark {bool yes;constructor(ref mutable string input){deref input="changed";self.yes=true;}}
struct stop {bool yes;constructor(bool broken){delta broken {true=>fault "last initializer",false=>complete}[complete]:self.yes=true;}}
struct pending['a] {counter owned;mark first;mark second;ref string text <- 'a;stop last;}
control main(string path,bool broken) exits done {
    fault boundary {
        delta make(borrow path) {made {counter owned}=>made}
        [made]: mutable string text="before";
        auto value=pending {move owned,mark(borrow mutable text),mark(borrow mutable text),borrow text,stop(broken)};
        fault "after assembly";
    } contingency [] {terminate process 23;}
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config)).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = f.root.join(format!("sequential-resource-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        for broken in ["true", "false"] {
            let log = f
                .root
                .join(format!("sequential-resource-{opt}-{broken}.log"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
        }
    }
}

#[test]
fn fault_settlement_releases_unstarted_task_captures_before_contingency() {
    let f = Fixture::new();
    let host=RESOURCE_HOST.replace("pub fn close(value:Counter,fail:bool)->rust_api::close_outcome {drop(value);", "pub fn close(value:Counter,fail:bool)->rust_api::close_outcome {use std::io::Write;writeln!(std::fs::OpenOptions::new().append(true).create(true).open(&value.path).unwrap(),\"close\").unwrap();drop(value);");
    fs::write(f.root.join("host.rs"), host).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    config["primitives"][3]["contingent"] = json!(true);
    let source = r#"
async control finish(counter owned) exits done {
    delta close(owned,false) {closed=>closed,failed=>failed}
    [closed|failed]:exit done;
}
async control fail_with_pending(counter owned) faults {
    auto pending=defer finish(owned);
    fault "before await";
}
control main(string path) exits done {
    delta make(borrow path) {made {counter parent}=>parent}
    [parent]:delta make(borrow path) {made {counter child}=>child}
    [child]:fault boundary {run fail_with_pending(child);} contingency [parent] {
        delta close(parent,false) {closed=>closed,failed=>failed}
        [closed|failed]:terminate process 23;
    }
}
"#;
    let p = compile(source, f.manifest(&config))
        .unwrap()
        .with_entry_exit_codes(
            "main",
            std::collections::BTreeMap::from([("done".into(), 0)]),
        )
        .unwrap();
    let mut rejected = config.clone();
    rejected["types"][0]
        .as_object_mut()
        .unwrap()
        .remove("fault");
    assert_eq!(
        compile(source, f.manifest(&rejected)).unwrap_err().code,
        "E_FAULT_RESOURCE"
    );
    for (index, p) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        let binary = f.root.join(format!("pending-{index}"));
        p.build_native("main", &binary, 0).unwrap();
        let log = f.root.join(format!("pending-drops-{index}"));
        let output = Command::new(&binary).arg("--").arg(&log).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\nclose\ndrop 7\n");
    }
}

#[test]
fn pending_borrows_keep_retained_resource_owners_until_normal_or_fault_settlement() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
async control examine(ref counter value) exits done {
    delta inspect(value,value) {number {u32 n}=>ready}
    [ready]:exit done;
}
async control start(counter owned,bool broken) exits done faults {
    auto pending=defer examine(borrow owned);
    delta broken {true=>fail,false=>finish}
    [fail]:fault "before await";
    [finish]:await pending;
        delta close(owned,false) {closed=>closed,failed=>failed}
    [closed|failed]:exit done;
}
control main(string path,bool broken) exits done {
    fault boundary {
        delta make(borrow path) {made {counter owned}=>made}
        [made]:run start(owned,broken);
    } contingency [] {terminate process 23;}
    exit done;
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config))
        .unwrap()
        .with_entry_exit_codes(
            "main",
            std::collections::BTreeMap::from([("done".into(), 0)]),
        )
        .unwrap();
    for (index, p) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        let binary = f.root.join(format!("pending-borrow-{index}"));
        p.build_native("main", &binary, 0).unwrap();
        for broken in [false, true] {
            let log = f.root.join(format!("pending-borrow-{index}-{broken}"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken.to_string())
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(if broken { 23 } else { 0 }),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
            let report: Json = serde_json::from_slice(&output.stdout).unwrap();
            let events = report["events"].as_array().unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e["operation"] == "inspect")
                    .count(),
                if broken { 0 } else { 1 }
            );
        }
    }
}

#[test]
fn owned_contained_task_captures_keep_resource_dispositions_and_release_once() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
struct retained['a] {ref counter value <- 'a;}
async control examine(retained input) exits done {
    delta inspect(input.value,input.value) {number {u32 n}=>ready}
    [ready]:exit done;
}
async control start(counter owned,bool broken) exits done faults {
    retained input=retained {borrow owned};auto pending=defer examine(input);drop(input);
    delta broken {true=>fail,false=>finish}
    [fail]:fault "before await";
    [finish]:await pending;
        delta close(owned,false) {closed=>closed,failed=>failed}
    [closed|failed]:exit done;
}
control main(string path,bool broken) exits done {
    fault boundary {
        delta make(borrow path) {made {counter owned}=>made}
        [made]:run start(owned,broken);
    } contingency [] {terminate process 23;}
    exit done;
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config))
        .unwrap()
        .with_entry_exit_codes(
            "main",
            std::collections::BTreeMap::from([("done".into(), 0)]),
        )
        .unwrap();
    for (index, p) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        let binary = f.root.join(format!("contained-borrow-{index}"));
        p.build_native("main", &binary, 0).unwrap();
        for broken in [false, true] {
            let log = f.root.join(format!("contained-borrow-{index}-{broken}"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken.to_string())
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(if broken { 23 } else { 0 }),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
            let report: Json = serde_json::from_slice(&output.stdout).unwrap();
            let events = report["events"].as_array().unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e["operation"] == "inspect")
                    .count(),
                if broken { 0 } else { 1 }
            );
        }
    }
}

#[test]
fn owned_task_record_retains_both_resource_and_reference_obligations() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut config = resource_config();
    config["types"][0]["fault"] = json!("release");
    let source = r#"
struct retained['a] {counter owned;ref string label <- 'a;}
async control examine(retained input) exits done {
    delta inspect(borrow input.owned,borrow input.owned) {number {u32 n}=>ready}
    [ready]:emit(input.label);delta close(input.owned,false) {closed=>closed,failed=>failed}
    [closed|failed]:exit done;
}
async control start(counter owned,bool broken) exits done faults {
    string label="retained";retained input=retained {owned,borrow label};auto pending=defer examine(input);
    delta broken {true=>fail,false=>finish}
    [fail]:fault "before await";
    [finish]:await pending;exit done;
}
control main(string path,bool broken) exits done {
    fault boundary {
        delta make(borrow path) {made {counter owned}=>made}
        [made]:run start(owned,broken);
    } contingency [] {terminate process 23;}
    exit done;
}
"#;
    assert_eq!(
        compile(source, f.manifest(&resource_config()))
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
    let p = compile(source, f.manifest(&config))
        .unwrap()
        .with_entry_exit_codes(
            "main",
            std::collections::BTreeMap::from([("done".into(), 0)]),
        )
        .unwrap();
    for (index, p) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        let binary = f.root.join(format!("contained-owned-{index}"));
        p.build_native("main", &binary, 0).unwrap();
        for broken in [false, true] {
            let log = f.root.join(format!("contained-owned-{index}-{broken}"));
            let output = Command::new(&binary)
                .args(["--json", "--"])
                .arg(&log)
                .arg(broken.to_string())
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(if broken { 23 } else { 0 }),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(log).unwrap(), "drop 7\n");
            let report: Json = serde_json::from_slice(&output.stdout).unwrap();
            let events = report["events"].as_array().unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e["operation"] == "inspect")
                    .count(),
                if broken { 0 } else { 1 }
            );
        }
    }
}
