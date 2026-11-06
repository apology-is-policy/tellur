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
