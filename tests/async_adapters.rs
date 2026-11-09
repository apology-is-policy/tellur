//! Rust adapters for async host operations. Native executables start a bound
//! operation once and poll it until completion (E1g.2b), re-polling only at idle
//! points after its delay or when its descriptor becomes readable (E1g.3).
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
        let p = p.clone().with_entry_exit_codes("main", codes).unwrap();
        // Polled adapters add the internal readiness dependency; resolve it offline.
        p.lock_native("main", &self.root.join(format!("{name}-lock")))
            .unwrap();
        p.build_native("main", &binary, 0).unwrap();
        // macOS checks a newly linked executable on its first launch, one binary at a
        // time: about 0.4 s here, and several seconds while other tests launch fresh
        // binaries. Launch once untimed so the timing bounds below measure the program.
        let help = Command::new(&binary).arg("--help").output().unwrap();
        assert!(help.status.success());
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
pub struct Counter { path: String, name: String }
impl Drop for Counter { fn drop(&mut self) { log(&self.path, format!("{} drop", self.name)); } }
pub fn make(path: &str) -> rust_api::make_outcome {
    rust_api::make_outcome::made(Counter { path: path.into(), name: "counter".into() })
}
pub fn close(value: Counter) -> rust_api::close_outcome {
    drop(value);
    rust_api::close_outcome::closed
}
pub struct Ticking { label: String, remaining: u32, path: String }
impl Drop for Ticking { fn drop(&mut self) { log(&self.path, format!("state drop {}", self.label)); } }
pub fn tick(counter: &Counter, label: String) -> Box<dyn rust_api::tick_operation> {
    let remaining = match label.as_str() { "hold" => 50, "slow" => 3, "fast" => 1, _ => 0 };
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
    let p = p.with_entry_exit_codes("main", codes).unwrap();
    p.lock_native("main", &f.root.join("mismatch-lock"))
        .unwrap();
    assert!(p.build_native("main", &f.root.join("mismatch"), 0).is_err());
}

fn channel_config() -> Json {
    json!({"version":"0.1",
    "types":[{"name":"channel","kind":"opaque","linear":true,"fault":"release"}],
    "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"channel":"Channel"}},
    "primitives":[
        {"name":"open_channel","parameters":[{"name":"path","type":"string","mode":"borrow"},
            {"name":"name","type":"string","mode":"borrow"},{"name":"delay","type":"u32","mode":"value"}],
         "outcomes":[{"name":"opened","fields":[{"name":"value","type":"channel"}]}],
         "effects":["open"],"returns":true,"rust":{"execution":"immediate"}},
        {"name":"receive","async":true,
         "parameters":[{"name":"channel","type":"channel","mode":"borrow"}],
         "outcomes":[{"name":"data","fields":[{"name":"text","type":"string"}]},{"name":"closed","fields":[]}],
         "effects":["receive"],"returns":true,"rust":{"execution":"poll"}},
        {"name":"close_channel","parameters":[{"name":"value","type":"channel","mode":"consume"}],
         "outcomes":[{"name":"closed","fields":[]}],
         "effects":["close"],"returns":true,"rust":{"execution":"immediate"}}
    ]})
}

const CHANNEL_HOST: &str = r#"
use crate::rust_api;
use std::{io::{Read, Write}, os::unix::net::UnixStream, time::Duration};
fn log(path: &str, line: String) {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}
pub struct Channel { reader: UnixStream, path: String, name: String }
impl Drop for Channel { fn drop(&mut self) { log(&self.path, format!("channel drop {}", self.name)); } }
pub fn open_channel(path: &str, name: &str, delay: u32) -> rust_api::open_channel_outcome {
    let (reader, mut writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    let message = name.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay.into()));
        let _ = writer.write_all(message.as_bytes());
        std::thread::sleep(Duration::from_secs(5));
    });
    rust_api::open_channel_outcome::opened(Channel { reader, path: path.into(), name: name.into() })
}
pub fn close_channel(value: Channel) -> rust_api::close_channel_outcome {
    drop(value);
    rust_api::close_channel_outcome::closed
}
pub struct Receiving { path: String, name: String, polls: u32 }
impl Drop for Receiving {
    fn drop(&mut self) { log(&self.path, format!("state drop {} after {} polls", self.name, self.polls)); }
}
pub fn receive(channel: &Channel) -> Box<dyn rust_api::receive_operation> {
    Box::new(Receiving { path: channel.path.clone(), name: channel.name.clone(), polls: 0 })
}
impl rust_api::receive_operation for Receiving {
    fn poll(&mut self, channel: &Channel) -> rust_api::Poll<rust_api::receive_outcome> {
        self.polls += 1;
        let mut buffer = [0u8; 64];
        match (&channel.reader).read(&mut buffer) {
            Ok(0) => rust_api::Poll::Ready(rust_api::receive_outcome::closed),
            Ok(n) => rust_api::Poll::Ready(rust_api::receive_outcome::data(
                String::from_utf8_lossy(&buffer[..n]).into_owned(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                rust_api::Poll::Pending(rust_api::Wait::readable(&channel.reader))
            }
            Err(e) => panic!("{e}"),
        }
    }
}
"#;

const CHANNELS: &str = r#"
async control listen(ref channel c) exits done {
    delta await receive(c) {
        data { string text } => got,
        closed => ended
    }

    [got]: emit(text);
    [got | ended]: exit done;
}
async control startup(ref channel slow, ref channel fast) exits done {
    auto first = spawn listen(slow);
    auto second = spawn listen(fast);
    emit("queued");
    await first;
    await second;
    exit done;
}
control main(string path) exits done {
    delta open_channel(borrow path, borrow "slow", 150) {
        opened { channel slow } => slow_ready
    }
    [slow_ready]:
        delta open_channel(borrow path, borrow "fast", 20) {
            opened { channel fast } => fast_ready
        }
    [slow_ready & fast_ready]:
        run startup(borrow slow, borrow fast);
        delta close_channel(move slow) {
            closed => slow_closed
        }
    [slow_ready & fast_ready & slow_closed]:
        delta close_channel(move fast) {
            closed => fast_closed
        }
    [slow_ready & fast_ready & slow_closed & fast_closed]: exit done;
}
"#;

#[test]
fn readable_descriptors_wake_polled_operations_without_retry_loops() {
    let f = Fixture::new("channels");
    fs::write(f.root.join("host.rs"), CHANNEL_HOST).unwrap();
    let p = compile(CHANNELS, f.manifest(&channel_config())).unwrap();
    let binary = f.build(&p, "channels");
    let started = std::time::Instant::now();
    let (ok, result, log) = f.run(&binary, "channels.log");
    assert!(ok, "{result}");
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert_eq!(
        messages(&result["events"]),
        [
            "open_channel",
            "open_channel",
            "queued",
            "receive:fast",
            "fast",
            "receive:slow",
            "slow",
            "close_channel",
            "close_channel"
        ]
    );
    // Each operation polls once at start (WouldBlock) and once when readable.
    assert_eq!(
        log,
        [
            "state drop fast after 2 polls",
            "state drop slow after 2 polls",
            "channel drop slow",
            "channel drop fast"
        ]
    );
    // An owning fault stops a registered operation without waiting for its data.
    let faulting = CHANNELS
        .replace(
            "    auto second = spawn listen(fast);",
            "    auto second = spawn failing();",
        )
        .replace(
            "async control startup(ref channel slow, ref channel fast) exits done {",
            "async control failing() exits done faults {\n    yield;\n    fault \"sibling failed\";\n}\nasync control startup(ref channel slow, ref channel fast) exits done faults {",
        )
        .replace("open_channel(borrow path, borrow \"slow\", 150)", "open_channel(borrow path, borrow \"slow\", 3000)")
        .replace("control main(string path) exits done {", "control main(string path) exits done faults {");
    let p = compile(&faulting, f.manifest(&channel_config())).unwrap();
    let binary = f.build(&p, "channel-fault");
    let started = std::time::Instant::now();
    let (ok, report, log) = f.run(&binary, "channel-fault.log");
    assert!(!ok);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "stopped operation waited for data"
    );
    assert!(
        report["message"]
            .as_str()
            .unwrap()
            .contains("sibling failed"),
        "{report}"
    );
    assert_eq!(log[0], "state drop slow after 1 polls");
    assert!(log.contains(&"channel drop slow".to_string()), "{log:?}");
}

/// `split` is a polled operation whose completion owns a new resource.
fn split_config() -> Json {
    let mut config = config();
    config["primitives"].as_array_mut().unwrap().push(json!(
        {"name":"split","async":true,
         "parameters":[{"name":"counter","type":"counter","mode":"borrow"},
                       {"name":"label","type":"string","mode":"consume"}],
         "outcomes":[{"name":"parted","fields":[{"name":"piece","type":"counter"}]}],
         "effects":["split"],"returns":true,"rust":{"execution":"poll"}}));
    config
}
const SPLIT_HOST: &str = r#"
pub struct Splitting { label: String, remaining: u32, path: String }
pub fn split(counter: &Counter, label: String) -> Box<dyn rust_api::split_operation> {
    let remaining = if label == "slow" { 3 } else { 1 };
    Box::new(Splitting { label, remaining, path: counter.path.clone() })
}
impl rust_api::split_operation for Splitting {
    fn poll(&mut self, _: &Counter) -> rust_api::Poll<rust_api::split_outcome> {
        if self.remaining == 0 {
            log(&self.path, format!("parted {}", self.label));
            let name = format!("piece {}", self.label);
            return rust_api::Poll::Ready(rust_api::split_outcome::parted(Counter { path: self.path.clone(), name }));
        }
        self.remaining -= 1;
        rust_api::Poll::Pending(rust_api::Wait::after(Duration::from_millis(2)))
    }
}
"#;
const SPLITS: &str = r#"
async control part(ref counter c, string label) exits done {
    delta await split(c, move label) {
        parted { counter piece } => parted
    }

    [parted]:
        yield;
        delta close(move piece) {
            closed => closed
        }
    [parted & closed]: exit done;
}
async control startup(ref counter c) exits done {
    auto first = spawn part(c, "slow");
    auto second = spawn part(c, "fast");
    await first;
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

#[test]
fn polled_operations_return_owned_resources_released_by_faults() {
    let f = Fixture::new("split");
    fs::write(f.root.join("host.rs"), format!("{HOST}{SPLIT_HOST}")).unwrap();
    let p = compile(SPLITS, f.manifest(&split_config())).unwrap();
    let binary = f.build(&p, "split");
    let (ok, result, log) = f.run(&binary, "split.log");
    assert!(ok, "{result}");
    assert_eq!(
        log,
        [
            "parted fast",
            "piece fast drop",
            "parted slow",
            "piece slow drop",
            "counter drop"
        ]
    );
    // Scripted resource results match the reference executor.
    let scenario = f.root.join("split.json");
    let piece = |token: &str| json!({"kind":"opaque","value":{"ty":"counter","token":token}});
    fs::write(
        &scenario,
        json!({"entry":"main","arguments":[{"kind":"string","value":"unused"}],"calls":[
            {"operation":"make","outcome":"made","fields":[piece("c")]},
            {"operation":"split","outcome":"parted","fields":[piece("a")],"waits":2},
            {"operation":"split","outcome":"parted","fields":[piece("b")]},
            {"operation":"close","outcome":"closed","fields":[]},
            {"operation":"close","outcome":"closed","fields":[]},
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
    let s: tellur::runtime::Scenario =
        serde_json::from_slice(&fs::read(&scenario).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<Json>(&output.stdout).unwrap(),
        serde_json::to_value(p.run(&s).unwrap()).unwrap()
    );
    // A sibling fault stops a member that owns a returned resource across a
    // handoff; the resource is released before the borrowed supplier.
    let faulting = SPLITS
        .replace(
            "    auto second = spawn part(c, \"fast\");",
            "    auto second = spawn failing(c);",
        )
        .replace("\"slow\"", "\"fast\"")
        .replace(
            "async control startup(ref counter c) exits done {",
            "async control failing(ref counter c) exits done faults {\n    delta await tick(c, \"slow\") {\n        ticked { string label } => ticked\n    }\n\n    [ticked]: fault \"sibling failed\";\n}\nasync control startup(ref counter c) exits done faults {",
        )
        .replace(
            "[parted]:\n        yield;",
            "[parted]:\n        delta await tick(c, \"hold\") {\n            ticked { string waited_label } => waited\n        }",
        )
        .replace("[parted & closed]", "[parted & waited & closed]")
        .replace("control main(string path) exits done {", "control main(string path) exits done faults {");
    let p = compile(&faulting, f.manifest(&split_config())).unwrap();
    let binary = f.build(&p, "split-fault");
    let (ok, report, log) = f.run(&binary, "split-fault.log");
    assert!(!ok);
    assert!(
        report["message"]
            .as_str()
            .unwrap()
            .contains("sibling failed"),
        "{report}"
    );
    let at = |line: &str| {
        log.iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("{line}: {log:?}"))
    };
    assert!(at("parted fast") < at("state drop hold"), "{log:?}");
    assert!(at("state drop hold") < at("piece fast drop"), "{log:?}");
    assert_eq!(log.last().unwrap(), "counter drop");
    // Without a fault disposition the async result is rejected by the manifest.
    let mut unsettled = split_config();
    unsettled["types"][0]
        .as_object_mut()
        .unwrap()
        .remove("fault");
    let error = Manifest::from_json_at(&unsettled.to_string(), &f.root.join("manifest.json"))
        .expect_err("resource result without fault disposition");
    assert!(
        error
            .message
            .contains("returns a resource without a fault disposition"),
        "{}",
        error.message
    );
}

/// `retire` is a polled operation that consumes a resource; a stopped operation
/// drops it with its state.
fn retire_config() -> Json {
    let mut config = config();
    config["primitives"].as_array_mut().unwrap().push(json!(
        {"name":"retire","async":true,
         "parameters":[{"name":"value","type":"counter","mode":"consume"},
                       {"name":"label","type":"string","mode":"consume"}],
         "outcomes":[{"name":"retired","fields":[]}],
         "effects":["retire"],"returns":true,"rust":{"execution":"poll"}}));
    config
}
const RETIRE_HOST: &str = r#"
pub struct Retiring { value: Option<Counter>, label: String, remaining: u32 }
impl Drop for Retiring {
    fn drop(&mut self) {
        if let Some(value) = &self.value { log(&value.path, format!("retiring drop {}", self.label)); }
    }
}
pub fn retire(value: Counter, label: String) -> Box<dyn rust_api::retire_operation> {
    let remaining = if label == "hold" { 50 } else { 2 };
    Box::new(Retiring { value: Some(value), label, remaining })
}
impl rust_api::retire_operation for Retiring {
    fn poll(&mut self) -> rust_api::Poll<rust_api::retire_outcome> {
        if self.remaining == 0 {
            let value = self.value.take().unwrap();
            log(&value.path, format!("retired {}", self.label));
            drop(value);
            return rust_api::Poll::Ready(rust_api::retire_outcome::retired);
        }
        self.remaining -= 1;
        rust_api::Poll::Pending(rust_api::Wait::after(Duration::from_millis(2)))
    }
}
"#;
const RETIRES: &str = r#"
async control finish(string path, string label) exits done {
    delta make(borrow path) {
        made { counter c } => made
    }

    [made]:
        delta await retire(move c, move label) {
            retired => retired
        }
    [made & retired]: exit done;
}
async control startup(ref string path) exits done {
    auto first = spawn finish(clone(path), "slow");
    auto second = spawn finish(clone(path), "fast");
    await first;
    await second;
    exit done;
}
control main(string path) exits done {
    run startup(borrow path);
    exit done;
}
"#;

#[test]
fn polled_operations_consume_resources_and_drop_them_when_stopped() {
    let f = Fixture::new("retire");
    fs::write(f.root.join("host.rs"), format!("{HOST}{RETIRE_HOST}")).unwrap();
    let p = compile(RETIRES, f.manifest(&retire_config())).unwrap();
    let binary = f.build(&p, "retire");
    let (ok, result, log) = f.run(&binary, "retire.log");
    assert!(ok, "{result}");
    assert_eq!(
        log,
        [
            "retired slow",
            "counter drop",
            "retired fast",
            "counter drop"
        ]
    );
    let scenario = f.root.join("retire.json");
    let counter = |token: &str| json!({"kind":"opaque","value":{"ty":"counter","token":token}});
    fs::write(
        &scenario,
        json!({"entry":"main","arguments":[{"kind":"string","value":"unused"}],"calls":[
            {"operation":"make","outcome":"made","fields":[counter("a")]},
            {"operation":"make","outcome":"made","fields":[counter("b")]},
            {"operation":"retire","outcome":"retired","fields":[],"waits":2},
            {"operation":"retire","outcome":"retired","fields":[]}
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
    let s: tellur::runtime::Scenario =
        serde_json::from_slice(&fs::read(&scenario).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<Json>(&output.stdout).unwrap(),
        serde_json::to_value(p.run(&s).unwrap()).unwrap()
    );
    // A sibling fault stops the pending operation, which owns the consumed counter.
    let faulting = RETIRES
        .replace(
            "    auto second = spawn finish(clone(path), \"fast\");\n",
            "    auto second = spawn failing();\n",
        )
        .replace("\"slow\"", "\"hold\"")
        .replace(
            "async control startup(ref string path) exits done {",
            "async control failing() exits done faults {\n    yield;\n    fault \"sibling failed\";\n}\nasync control startup(ref string path) exits done faults {",
        )
        .replace("control main(string path) exits done {", "control main(string path) exits done faults {");
    let p = compile(&faulting, f.manifest(&retire_config())).unwrap();
    let binary = f.build(&p, "retire-fault");
    let (ok, report, log) = f.run(&binary, "retire-fault.log");
    assert!(!ok);
    assert!(
        report["message"]
            .as_str()
            .unwrap()
            .contains("sibling failed"),
        "{report}"
    );
    assert_eq!(log, ["retiring drop hold", "counter drop"]);
    // Without a fault disposition a consumed resource is rejected.
    let mut unsettled = retire_config();
    unsettled["types"][0]
        .as_object_mut()
        .unwrap()
        .remove("fault");
    let error = Manifest::from_json_at(&unsettled.to_string(), &f.root.join("manifest.json"))
        .expect_err("consumed resource without fault disposition");
    assert!(
        error
            .message
            .contains("consumes a resource without a fault disposition"),
        "{}",
        error.message
    );
}

/// `push` writes more than a socket buffer holds to a slowly read pipe; it parks
/// on write readiness instead of blocking the executor or polling on a timer.
fn pipe_config() -> Json {
    json!({"version":"0.1",
    "types":[{"name":"pipe","kind":"opaque","linear":true,"fault":"release"}],
    "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"pipe":"Pipe"}},
    "primitives":[
        {"name":"open_pipe","parameters":[{"name":"path","type":"string","mode":"borrow"}],
         "outcomes":[{"name":"opened","fields":[{"name":"value","type":"pipe"}]}],
         "effects":["open"],"returns":true,"rust":{"execution":"immediate"}},
        {"name":"push","async":true,
         "parameters":[{"name":"pipe","type":"pipe","mode":"borrow"}],
         "outcomes":[{"name":"pushed","fields":[]}],
         "effects":["push"],"returns":true,"rust":{"execution":"poll"}},
        {"name":"close_pipe","parameters":[{"name":"value","type":"pipe","mode":"consume"}],
         "outcomes":[{"name":"closed","fields":[]}],
         "effects":["close"],"returns":true,"rust":{"execution":"immediate"}}
    ]})
}
const PIPE_HOST: &str = r#"
use crate::rust_api;
use std::{io::{Read, Write}, os::unix::net::UnixStream, time::Duration};
fn log(path: &str, line: String) {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}
const SIZE: usize = 4 << 20;
pub struct Pipe { writer: UnixStream, path: String, reader: Option<std::thread::JoinHandle<usize>> }
impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
    }
}
pub fn open_pipe(path: &str) -> rust_api::open_pipe_outcome {
    let (mut reader, writer) = UnixStream::pair().unwrap();
    writer.set_nonblocking(true).unwrap();
    let reader = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        let mut total = 0;
        let mut chunk = [0; 65536];
        while total < SIZE {
            match reader.read(&mut chunk) { Ok(0) | Err(_) => break, Ok(n) => total += n }
        }
        total
    });
    rust_api::open_pipe_outcome::opened(Pipe { writer, path: path.into(), reader: Some(reader) })
}
pub fn close_pipe(value: Pipe) -> rust_api::close_pipe_outcome {
    drop(value);
    rust_api::close_pipe_outcome::closed
}
pub struct Pushing { written: usize, blocked: bool }
pub fn push(_: &Pipe) -> Box<dyn rust_api::push_operation> {
    Box::new(Pushing { written: 0, blocked: false })
}
impl rust_api::push_operation for Pushing {
    fn poll(&mut self, pipe: &Pipe) -> rust_api::Poll<rust_api::push_outcome> {
        let chunk = [7u8; 65536];
        while self.written < SIZE {
            let end = (SIZE - self.written).min(chunk.len());
            match (&pipe.writer).write(&chunk[..end]) {
                Ok(n) => self.written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if !std::mem::replace(&mut self.blocked, true) { log(&pipe.path, "blocked".into()); }
                    return rust_api::Poll::Pending(rust_api::Wait::writable(&pipe.writer).or_after(Duration::from_secs(10)));
                }
                Err(e) => panic!("{e}"),
            }
        }
        log(&pipe.path, "pushed".into());
        rust_api::Poll::Ready(rust_api::push_outcome::pushed)
    }
}
"#;
const PUSHES: &str = r#"
async control send_all(ref pipe p) exits done {
    delta await push(p) {
        pushed => pushed
    }

    [pushed]:
        emit("pushed");
        exit done;
}
async control other() exits done {
    emit("other");
    exit done;
}
async control startup(ref pipe p) exits done {
    auto writer = spawn send_all(p);
    auto quick = spawn other();
    await writer;
    await quick;
    exit done;
}
control main(string path) exits done {
    delta open_pipe(borrow path) {
        opened { pipe p } => opened
    }

    [opened]:
        run startup(borrow p);
        delta close_pipe(move p) {
            closed => closed
        }
    [opened & closed]: exit done;
}
"#;

#[test]
fn writable_descriptors_wake_polled_writes() {
    let f = Fixture::new("pipe");
    fs::write(f.root.join("host.rs"), PIPE_HOST).unwrap();
    let p = compile(PUSHES, f.manifest(&pipe_config())).unwrap();
    let binary = f.build(&p, "pipe");
    let started = std::time::Instant::now();
    let (ok, result, log) = f.run(&binary, "pipe.log");
    assert!(ok, "{result}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "write readiness did not wake the parked operation"
    );
    assert_eq!(log, ["blocked", "pushed"]);
    assert_eq!(
        messages(&result["events"]),
        ["open_pipe", "other", "push", "pushed", "close_pipe"]
    );
}

/// Polled adapters are spawned directly (0.227): `spawn tick(..) into set` and
/// `defer tick(..)` start the generated controls, and `run tick(..)` enters one
/// from synchronous code.
const SPAWNED: &str = r#"
async control startup(string path) exits done {
    tasks<tick> active = tasks();
    spawn tick("slow") into active;
    spawn tick("fast") into active;
    auto later = defer tick("cold");
    delta await later {
        ticked { string label } => joined
    }
    [joined]:
        emit(label);
        loop reap(tasks<tick> pending = move active) {
            delta await next pending {
                ticked { string finished } => member,
                empty => drained
            }

            [member]:
                emit(finished);
                again reap(pending);
            [drained]: exit done;
        }
}
control main(string path) exits done {
    delta run tick("first") {
        ticked { string label } => ran
    }
    [ran]:
        emit(label);
        forward run startup(path);
}
"#;
fn spawn_config() -> Json {
    json!({"version":"0.1","types":[],
    "rust":{"source":"host.rs","lockfile":"Cargo.lock"},
    "primitives":[
        {"name":"tick","async":true,
         "parameters":[{"name":"label","type":"string","mode":"consume"}],
         "outcomes":[{"name":"ticked","fields":[{"name":"label","type":"string"}]}],
         "effects":["tick"],"returns":true,"rust":{"execution":"poll"}}
    ]})
}
const SPAWN_HOST: &str = r#"
use crate::rust_api;
use std::time::Duration;
pub struct Ticking { label: String, remaining: u32 }
pub fn tick(label: String) -> Box<dyn rust_api::tick_operation> {
    let remaining = match label.as_str() { "slow" => 6, "fast" => 1, _ => 0 };
    Box::new(Ticking { label, remaining })
}
impl rust_api::tick_operation for Ticking {
    fn poll(&mut self) -> rust_api::Poll<rust_api::tick_outcome> {
        if self.remaining == 0 {
            return rust_api::Poll::Ready(rust_api::tick_outcome::ticked(format!("{}!", self.label)));
        }
        self.remaining -= 1;
        rust_api::Poll::Pending(rust_api::Wait::after(Duration::from_millis(3)))
    }
}
"#;

#[test]
fn polled_operations_are_spawned_into_sets_deferred_and_run() {
    let f = Fixture::new("spawned");
    fs::write(f.root.join("host.rs"), SPAWN_HOST).unwrap();
    let p = compile(SPAWNED, f.manifest(&spawn_config())).unwrap();
    let binary = f.build(&p, "spawned");
    let (ok, result, _) = f.run(&binary, "spawned.log");
    assert!(ok, "{result}");
    let emitted: Vec<_> = messages(&result["events"])
        .into_iter()
        .filter(|m| !m.starts_with("tick:"))
        .collect();
    // The cold operation starts at its await, while both members are pending.
    assert_eq!(emitted, ["first!", "cold!", "fast!", "slow!"]);
}

const CANCELLED: &str = r#"
async control step(ref counter c, string label) exits done, stopped {
    emit(borrow label);
    delta await tick(c, move label) {
        ticked { string result } => ticked,
        cancelled => cancelled
    }

    [ticked]:
        emit(result);
        exit done;
    [cancelled]:
        emit("cancelled");
        exit stopped;
}
async control startup(ref counter c) exits done {
    auto held = spawn step(c, "hold");
    auto fast = spawn step(c, "fast");
    delta await fast {
        done => fast_done,
        stopped => fast_stopped
    }

    [fast_done | fast_stopped]: cancel held;
    delta await held {
        done => finished,
        stopped => halted
    }

    [finished]:
        emit("finished");
        exit done;
    [halted]:
        emit("halted");
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

#[test]
fn cancellation_stops_a_pending_polled_operation_and_drops_its_state() {
    let f = Fixture::new("cancelled");
    let p = compile(CANCELLED, f.manifest(&config())).unwrap();
    let binary = f.build(&p, "cancelled");
    let (ok, result, log) = f.run(&binary, "cancelled.log");
    assert!(ok, "{result}");
    assert_eq!(
        messages(&result["events"]),
        [
            "make",
            "hold",
            "fast",
            "tick:fast!",
            "fast!",
            "cancelled",
            "halted",
            "close"
        ]
    );
    // The held operation is stopped while pending: its state drops before the
    // supplier, without completing.
    assert_eq!(log.first().map(String::as_str), Some("start hold"));
    let stop = log.iter().position(|l| l == "state drop hold").unwrap();
    assert!(!log.iter().any(|l| l == "poll hold 0"), "{log:?}");
    assert_eq!(log.last().map(String::as_str), Some("counter drop"));
    assert!(stop < log.len() - 1);
}
