//! Actual typed static libraries, linked to the precompiled C runtime.
#![cfg(all(feature = "llvm", target_os = "macos", target_arch = "aarch64"))]
use serde_json::{Value as Json, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{CheckedProgram, compile, manifest::Manifest};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "tellur-llvm-resource-{}-{}",
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
    fn program(&self, source: &str, manifest: &Json) -> CheckedProgram {
        let p = compile(
            source,
            Manifest::from_json_at(&manifest.to_string(), &self.root.join("manifest.json"))
                .unwrap(),
        )
        .unwrap();
        let codes = p
            .regions()
            .find(|r| r.name == "main")
            .unwrap()
            .exits
            .iter()
            .map(|o| (o.name.clone(), 0))
            .collect();
        p.with_entry_exit_codes("main", codes).unwrap()
    }
    fn build(&self, p: &CheckedProgram, name: &str, llvm: bool, opt: u8) -> PathBuf {
        let path = self.root.join(format!("{name}-{llvm}-{opt}"));
        if llvm {
            p.build_llvm("main", &path, opt)
        } else {
            p.build_native("main", &path, opt)
        }
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
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
pub struct Counter {path:String, value:u32, local:std::rc::Rc<std::cell::Cell<u32>>}
impl Drop for Counter {fn drop(&mut self) {use std::io::Write; let mut file=std::fs::OpenOptions::new().create(true).append(true).open(&self.path).unwrap();writeln!(file,"drop {}",self.value).unwrap();}}
pub fn make(path:&str)->rust_api::make_outcome {rust_api::make_outcome::made(Counter{path:path.into(),value:7,local:std::rc::Rc::new(std::cell::Cell::new(0))})}
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

fn execute(
    binary: &Path,
    log: &Path,
    extra: &[&str],
    fuel: Option<u64>,
) -> (Option<i32>, Vec<u8>, Vec<u8>, String) {
    if log.exists() {
        fs::remove_file(log).unwrap();
    }
    let mut command = Command::new(binary);
    command.arg("--json");
    if let Some(fuel) = fuel {
        command.args(["--fuel", &fuel.to_string()]);
    }
    let output = command.arg("--").arg(log).args(extra).output().unwrap();
    (
        output.status.code(),
        output.stdout,
        output.stderr,
        fs::read_to_string(log).unwrap_or_default(),
    )
}
fn compare(
    p: &CheckedProgram,
    f: &Fixture,
    label: &str,
    extra: &[&str],
    fuel: bool,
    expected_status: i32,
    expected_drops: &str,
) {
    let log = f.root.join("drops");
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let name = format!("{label}-{expanded}");
            let rust = f.build(&p, &name, false, opt);
            let llvm = f.build(&p, &name, true, opt);
            let a = execute(&rust, &log, extra, None);
            let b = execute(&llvm, &log, extra, None);
            assert_eq!(a, b, "{name}/{opt}");
            assert_eq!(
                b.0,
                Some(expected_status),
                "{}",
                String::from_utf8_lossy(&b.2)
            );
            assert_eq!(b.3, expected_drops);
            if fuel {
                for fuel in 0..=24 {
                    let a = execute(&rust, &log, extra, Some(fuel));
                    let b = execute(&llvm, &log, extra, Some(fuel));
                    assert_eq!(
                        (&a.0, &a.1, &a.2),
                        (&b.0, &b.1, &b.2),
                        "{name}/{opt}/fuel={fuel}"
                    );
                    if a.0 == Some(0) {
                        assert_eq!(a.3, b.3, "{name}/{opt}/fuel={fuel}");
                    } else {
                        // Exceptional teardown promises release, not destructor
                        // order. Compare every owner exactly once while retaining
                        // byte-for-byte fault/report and normal-close comparisons.
                        let mut left = a.3.lines().collect::<Vec<_>>();
                        let mut right = b.3.lines().collect::<Vec<_>>();
                        left.sort();
                        right.sort();
                        assert_eq!(left, right, "{name}/{opt}/fuel={fuel}");
                    }
                }
            }
        }
    }
}
#[test]
fn synchronous_resources_keep_stable_shared_views_transfer_and_failed_cleanup() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let p = f.program(RESOURCE_SOURCE, &resource_config());
    compare(&p, &f, "handoff", &[], true, 0, "drop 7\ndrop 14\n");
}
#[test]
fn scopes_constructors_and_retained_child_frames_release_each_owner_once() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let mut m = resource_config();
    m["types"][0]["fault"] = json!("release");
    let cases = [
        (
            "escape",
            r#"control main(string path) exits auto {forward make(borrow path);}"#,
            0,
            "drop 7\n",
        ),
        (
            "source-fault",
            r#"control child(counter owned) faults {fault "broken";} control main(string path) faults {delta make(borrow path) {made {counter owned}=>made} [made]:child(owned);}"#,
            1,
            "drop 7\n",
        ),
        (
            "constructor",
            r#"
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
}"#,
            23,
            "drop 7\ndrop 7\ndrop 7\n",
        ),
        (
            "child",
            r#"
async control child(counter owned) exits ready {counter returned} {yield;exit ready {owned};}
async control startup(string path) exits done {
 delta make(borrow path) {made {counter owned}=>made}
 [made]:auto task=spawn child(owned);cancel task;yield;cancel task;
 delta await task {ready {counter returned}=>ready}
 [ready]:delta inspect(borrow returned,borrow returned) {number {u32 n}=>inspected}
 [inspected]:delta close(returned,false) {closed=>closed,failed=>failed}
 [closed | failed]:exit done;
}
control main(string path) exits done {forward run startup(path);}"#,
            0,
            "drop 7\n",
        ),
    ];
    // Validate every executable entry before starting the expensive linked matrix.
    let programs = cases
        .into_iter()
        .map(|(label, source, status, drops)| (label, f.program(source, &m), status, drops))
        .collect::<Vec<_>>();
    m["primitives"][1]["contingent"] = json!(true);
    m["primitives"][3]["contingent"] = json!(true);
    let p = f.program(
        r#"
boundary child(counter retained,string path) retains retained {
 delta make(borrow path) {made {counter local}=>ready}
 [ready]:fault "request broke";
} contingency(string reason) {
 delta inspect(borrow retained,borrow retained) {number {u32 n}=>ready}
 [ready]:delta close(retained,false) {closed=>closed,failed=>failed}
 [closed | failed]:terminate;
}
control main(string path) exits done {
 delta make(borrow path) {made {counter parent}=>first}
 [first]:delta make(borrow path) {made {counter retained}=>second}
 [second]:supervise child(retained,clone(borrow path));
 delta inspect(borrow parent,borrow parent) {number {u32 n}=>observed}
 [observed]:delta close(parent,false) {closed=>closed,failed=>failed}
 [closed | failed]:exit done;
}"#,
        &m,
    );
    for (label, program, status, drops) in programs {
        compare(&program, &f, label, &[], true, status, drops);
        eprintln!("validated scope case {label}: Rust/LLVM original/expanded O0/O2 and fuel cuts");
    }
    compare(
        &p,
        &f,
        "contingency",
        &[],
        true,
        0,
        "drop 7\ndrop 7\ndrop 7\n",
    );
}
#[test]
fn retained_file_modules_live_and_scripted_paths_preserve_host_policy() {
    let f = Fixture::new();
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/retained_file");
    let mut m: Json =
        serde_json::from_str(include_str!("../examples/retained_file/manifest.json")).unwrap();
    m["rust"]["modules"] = json!({"files":example.join("host.rs")});
    // An unused binding does not impose an extra Rust alias requirement.
    m["types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"unused","kind":"opaque","linear":true}));
    m["rust"]["resources"]["unused"] = json!("Unused");
    fs::write(
        f.root.join("host.rs"),
        "pub use self::files::{InputFile,file_open,file_chunk,file_release};",
    )
    .unwrap();
    let p = f.program(include_str!("../examples/retained_file/main.tellur"), &m);
    let input = f.root.join("input");
    let scenario = f.root.join("script.json");
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let rust = f.build(&p, &format!("file-{expanded}"), false, opt);
            let llvm = f.build(&p, &format!("file-{expanded}"), true, opt);
            for contents in [Some(b"hello\0\xff".as_slice()), Some(b"".as_slice()), None] {
                if let Some(bytes) = contents {
                    fs::write(&input, bytes).unwrap();
                } else {
                    fs::remove_file(&input).unwrap();
                }
                let run = |binary: &Path| {
                    Command::new(binary)
                        .args(["--json", "--"])
                        .arg(&input)
                        .output()
                        .unwrap()
                };
                let a = run(&rust);
                let b = run(&llvm);
                assert_eq!(
                    (a.status.code(), a.stdout, a.stderr),
                    (b.status.code(), b.stdout.clone(), b.stderr)
                );
                let report: Json = serde_json::from_slice(&b.stdout).unwrap();
                assert_eq!(
                    report["outcome"],
                    match contents {
                        Some(b) if b.is_empty() => "end",
                        Some(_) => "data",
                        None => "open_failed",
                    }
                );
            }
            for (outcome, fields) in [
                ("end", json!([])),
                ("failed", json!([{"kind":"string","value":"read failure"}])),
            ] {
                fs::write(&scenario,json!({"entry":"main","arguments":[{"kind":"string","value":"no real file"}],"calls":[
                    {"operation":"file_open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"input_file","token":"scripted"}}]},
                    {"operation":"file_chunk","outcome":outcome,"fields":fields},
                    {"operation":"file_release","outcome":"released","fields":[]}
                ]}).to_string()).unwrap();
                let run = |binary: &Path| {
                    Command::new(binary)
                        .arg("--scenario")
                        .arg(&scenario)
                        .output()
                        .unwrap()
                };
                let a = run(&rust);
                let b = run(&llvm);
                assert_eq!(
                    (a.status.code(), a.stdout, a.stderr),
                    (b.status.code(), b.stdout, b.stderr)
                );
            }
        }
    }
}
#[test]
fn live_resource_json_cannot_forge_direct_or_nested_owners() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"), RESOURCE_HOST).unwrap();
    let m = resource_config();
    for (label, source, value) in [
        (
            "direct",
            "control main(counter owned) exits auto {forward close(owned,false);}",
            json!({"kind":"opaque","value":{"ty":"counter","token":"rust:0"}}),
        ),
        (
            "nested",
            "struct box {counter owned;} control main(box value) exits auto {forward close(value.owned,false);}",
            json!({"kind":"construct","value":{"ty":"box","name":"box","fields":[{"kind":"opaque","value":{"ty":"counter","token":"rust:0"}}]}}),
        ),
    ] {
        let p = f.program(source, &m);
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let llvm = f.build(&p, &format!("{label}-{expanded}"), true, opt);
                let output = Command::new(&llvm)
                    .args(["--json", "--"])
                    .arg(value.to_string())
                    .output()
                    .unwrap();
                assert!(!output.status.success());
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("live resource arguments cannot be supplied as JSON")
                );
            }
        }
    }
}
