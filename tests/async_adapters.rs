//! E1g.2b: Rust adapters for async host operations. Native executables start a
//! bound operation once and poll it until completion, re-polling only when its
//! adapter's delay has elapsed and no other source work is runnable.
use serde_json::{Value as Json, json};
use std::{fs, path::PathBuf, process::Command};
use tellur::{CheckedProgram, compile, manifest::Manifest};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-async-adapter-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.lock"),
            include_str!("../examples/retained_file/Cargo.lock"),
        )
        .unwrap();
        fs::write(root.join("host.rs"), HOST).unwrap();
        Self { root }
    }
    fn manifest(&self, value: &Json) -> Manifest {
        Manifest::from_json_at(&value.to_string(), &self.root.join("manifest.json")).unwrap()
    }
    fn build(&self, p: &CheckedProgram, name: &str) -> PathBuf {
        let binary = self.root.join(name);
        let codes = p
            .region("main")
            .unwrap()
            .exits
            .iter()
            .map(|o| (o.name.clone(), 0))
            .collect();
        p.clone()
            .with_entry_exit_codes("main", codes)
            .unwrap()
            .build_native("main", &binary, 0)
            .unwrap();
        binary
    }
    /// Run live (no scenario), returning JSON stdout or stderr and the adapter log.
    fn run(&self, binary: &PathBuf, log: &str) -> (bool, Json, Vec<String>) {
        let log = self.root.join(log);
        let output = Command::new(binary)
            .args(["--json", "--"])
            .arg(&log)
            .output()
            .unwrap();
        let text = if output.status.success() {
            &output.stdout
        } else {
            &output.stderr
        };
        let value = serde_json::from_slice(text)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(text)));
        let lines = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        (output.status.success(), value, lines)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn config() -> Json {
    json!({"version":"0.1",
    "types":[{"name":"counter","kind":"opaque","linear":true,"fault":"release"}],
    "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"counter":"Counter"}},
    "primitives":[
        {"name":"make","parameters":[{"name":"path","type":"string","mode":"borrow"}],
         "outcomes":[{"name":"made","fields":[{"name":"value","type":"counter"}]}],
         "effects":["make"],"returns":true,"rust":{"execution":"immediate"}},
        {"name":"tick","async":true,
         "parameters":[{"name":"counter","type":"counter","mode":"borrow"},
                       {"name":"label","type":"string","mode":"consume"}],
         "outcomes":[{"name":"ticked","fields":[{"name":"label","type":"string"}]}],
         "effects":["tick"],"returns":true,"rust":{"execution":"poll"}},
        {"name":"close","parameters":[{"name":"value","type":"counter","mode":"consume"}],
         "outcomes":[{"name":"closed","fields":[]}],
         "effects":["close"],"returns":true,"rust":{"execution":"immediate"}}
    ]})
}

const HOST: &str = r#"
use crate::rust_api;
use std::time::Duration;
fn log(path: &str, line: String) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}
pub struct Counter { path: String }
impl Drop for Counter { fn drop(&mut self) { log(&self.path, "counter drop".into()); } }
pub fn make(path: &str) -> rust_api::make_outcome {
    rust_api::make_outcome::made(Counter { path: path.into() })
}
pub fn close(value: Counter) -> rust_api::close_outcome {
    drop(value);
    rust_api::close_outcome::closed
}
pub struct Ticking { label: String, remaining: u32, path: String }
impl Drop for Ticking { fn drop(&mut self) { log(&self.path, format!("state drop {}", self.label)); } }
pub fn tick(counter: &Counter, label: String) -> Box<dyn rust_api::tick_operation> {
    let remaining = match label.as_str() { "slow" => 3, "fast" => 1, _ => 0 };
    log(&counter.path, format!("start {label}"));
    Box::new(Ticking { label, remaining, path: counter.path.clone() })
}
impl rust_api::tick_operation for Ticking {
    fn poll(&mut self, counter: &Counter) -> rust_api::Poll<rust_api::tick_outcome> {
        assert_eq!(counter.path, self.path);
        log(&self.path, format!("poll {} {}", self.label, self.remaining));
        if self.remaining == 0 {
            return rust_api::Poll::Ready(rust_api::tick_outcome::ticked(format!("{}!", self.label)));
        }
        self.remaining -= 1;
        rust_api::Poll::Pending(rust_api::Wait::after(Duration::from_millis(2)))
    }
}
"#;

const CONCURRENT: &str = r#"
async control step(ref counter c, string label) exits done {
    emit(borrow label);
    delta await tick(c, move label) {
        ticked { string result } => ticked
    }

    [ticked]:
        emit(result);
        exit done;
}
async control startup(ref counter c) exits done {
    auto first = spawn step(c, "slow");
    auto second = spawn step(c, "fast");
    emit("queued");
    await first;
    emit("first joined");
    await second;
    exit done;
}
control main(string path) exits done {
    delta make(borrow path) {
        made { counter c } => made
    }

    [made]:
        run startup(borrow c);
        delta close(move c) {
            closed => closed
        }
    [made & closed]: exit done;
}
"#;

fn messages(events: &Json) -> Vec<String> {
    events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| match e["kind"].as_str().unwrap() {
            "emit" => e["message"].as_str().unwrap().to_owned(),
            _ => format!(
                "{}{}",
                e["operation"].as_str().unwrap(),
                e["fields"]
                    .get(0)
                    .and_then(|f| f["value"].as_str())
                    .map(|v| format!(":{v}"))
                    .unwrap_or_default()
            ),
        })
        .collect()
}

#[test]
fn polled_rust_operations_interleave_and_drop_state_before_suppliers() {
    let f = Fixture::new("concurrent");
    let p = compile(CONCURRENT, f.manifest(&config())).unwrap();
    let binary = f.build(&p, "concurrent");
    let (ok, result, log) = f.run(&binary, "concurrent.log");
    assert!(ok, "{result}");
    assert_eq!(result["outcome"], "done");
    assert_eq!(
        messages(&result["events"]),
        [
            "make",
            "queued",
            "slow",
            "fast",
            "tick:fast!",
            "fast!",
            "tick:slow!",
            "slow!",
            "first joined",
            "close"
        ]
    );
    assert_eq!(
        log,
        [
            "start slow",
            "poll slow 3",
            "start fast",
            "poll fast 1",
            "poll slow 2",
            "poll fast 0",
            "state drop fast",
            "poll slow 1",
            "poll slow 0",
            "state drop slow",
            "counter drop"
        ]
    );
    // A scenario still scripts the operation and never executes the adapter.
    let scenario = f.root.join("scenario.json");
    fs::write(
        &scenario,
        json!({"entry":"main","arguments":[{"kind":"string","value":"unused"}],"calls":[
            {"operation":"make","outcome":"made","fields":[{"kind":"opaque","value":{"ty":"counter","token":"c"}}]},
            {"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"one"}],"waits":1},
            {"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"two"}]},
            {"operation":"close","outcome":"closed","fields":[]}
        ]})
        .to_string(),
    )
    .unwrap();
    let output = Command::new(&binary)
        .arg("--scenario")
        .arg(&scenario)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scripted: Json = serde_json::from_slice(&output.stdout).unwrap();
    let s: tellur::runtime::Scenario =
        serde_json::from_slice(&fs::read(&scenario).unwrap()).unwrap();
    assert_eq!(scripted, serde_json::to_value(p.run(&s).unwrap()).unwrap());
}

#[test]
fn sequential_and_stopped_polled_operations_keep_resource_order() {
    let f = Fixture::new("sequential");
    let sequential = CONCURRENT.replace(
        "    auto first = spawn step(c, \"slow\");\n    auto second = spawn step(c, \"fast\");\n    emit(\"queued\");\n    await first;\n    emit(\"first joined\");\n    await second;\n",
        "    await step(c, \"slow\");\n",
    );
    let p = compile(&sequential, f.manifest(&config())).unwrap();
    let binary = f.build(&p, "sequential");
    let (ok, result, log) = f.run(&binary, "sequential.log");
    assert!(ok, "{result}");
    assert_eq!(
        messages(&result["events"]),
        ["make", "slow", "tick:slow!", "slow!", "close"]
    );
    assert_eq!(
        log,
        [
            "start slow",
            "poll slow 3",
            "poll slow 2",
            "poll slow 1",
            "poll slow 0",
            "state drop slow",
            "counter drop"
        ]
    );
    // An owning fault stops the pending operation: its state drops before the
    // borrowed counter, and no completion event is published.
    let faulting = CONCURRENT
        .replace(
            "    auto second = spawn step(c, \"fast\");",
            "    auto second = spawn failing();",
        )
        .replace(
            "async control startup(ref counter c) exits done {",
            "async control failing() exits done faults {\n    emit(\"failing\");\n    yield;\n    fault \"sibling failed\";\n}\nasync control startup(ref counter c) exits done faults {",
        )
        .replace("control main(string path) exits done {", "control main(string path) exits done faults {");
    let p = compile(&faulting, f.manifest(&config())).unwrap();
    let binary = f.build(&p, "faulting");
    let (ok, report, log) = f.run(&binary, "faulting.log");
    assert!(!ok);
    assert!(
        report["message"]
            .as_str()
            .unwrap()
            .contains("sibling failed"),
        "{report}"
    );
    assert_eq!(
        log,
        [
            "start slow",
            "poll slow 3",
            "state drop slow",
            "counter drop"
        ]
    );
}

#[test]
fn adapter_bindings_match_the_async_contract() {
    let f = Fixture::new("contracts");
    let mut wrong = config();
    wrong["primitives"][1]["rust"] = json!({"execution":"immediate"});
    let error = Manifest::from_json_at(&wrong.to_string(), &f.root.join("manifest.json"))
        .expect_err("immediate async binding");
    assert!(
        error.message.contains("use `\"execution\": \"poll\"`"),
        "{}",
        error.message
    );
    let mut wrong = config();
    wrong["primitives"][0]["rust"] = json!({"execution":"poll"});
    assert!(Manifest::from_json_at(&wrong.to_string(), &f.root.join("manifest.json")).is_err());
    // The adapter's start/poll signatures are checked by Rust.
    fs::write(
        f.root.join("host.rs"),
        HOST.replace(
            "pub fn tick(counter: &Counter, label: String)",
            "pub fn tick(counter: &Counter, label: u32)",
        ),
    )
    .unwrap();
    let p = compile(CONCURRENT, f.manifest(&config())).unwrap();
    let codes = [("done".to_string(), 0)].into_iter().collect();
    assert!(
        p.with_entry_exit_codes("main", codes)
            .unwrap()
            .build_native("main", &f.root.join("mismatch"), 0)
            .is_err()
    );
}
