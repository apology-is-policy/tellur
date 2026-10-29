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
            assert!(String::from_utf8_lossy(&result.stderr).contains("E_FUEL"));
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
