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
            include_str!("../applications/settings/Cargo.lock"),
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
