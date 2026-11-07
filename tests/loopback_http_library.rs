use serde_json::{Value as Json, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tellur::{
    CheckedProgram,
    ir::Value,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tellur-http-library-{}-{}",
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
fn example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/http_hello")
}
fn program(edit: impl Fn(&mut String)) -> Result<CheckedProgram, tellur::modules::ModuleError> {
    let root = example();
    let manifest = Manifest::from_json_at(
        &fs::read_to_string(root.join("manifest.json")).unwrap(),
        &root.join("manifest.json"),
    )
    .unwrap();
    tellur::modules::compile_with_loader(root.join("main.tellur"), manifest, |path| {
        let mut source = fs::read_to_string(path)?;
        if path.ends_with("main.tellur") {
            edit(&mut source);
        }
        Ok(source)
    })
}
fn text(s: &str) -> Value {
    Value::String(s.into())
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits: 0,
    }
}
fn listener() -> ScriptCall {
    call(
        "http_listen",
        "listening",
        vec![
            Value::Opaque {
                ty: "http_listener".into(),
                token: "listener".into(),
            },
            text("127.0.0.1:1234"),
        ],
    )
}
fn scenario(calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![Value::U32(0), Value::U32(1), Value::U32(20)],
        calls,
    }
}

#[test]
fn public_transport_outcomes_preserve_cleanup_and_independent_errors() {
    let p = program(|_| {}).unwrap();
    let expanded = p.expanded("main").unwrap();
    for (received, fields, status) in [
        ("malformed", vec![text("invalid framing")], Some(400)),
        ("headers_too_large", vec![], Some(431)),
        ("body_too_large", vec![], Some(413)),
        ("unsupported_transfer", vec![], Some(501)),
        ("unsupported_expectation", vec![], Some(417)),
        ("timed_out", vec![], Some(408)),
        ("disconnected", vec![], None),
        ("failed", vec![text("read failed")], None),
    ] {
        let mut calls = vec![
            listener(),
            call("checked_sub", "difference", vec![Value::U32(0)]),
            call(
                "http_accept",
                "accepted",
                vec![Value::Opaque {
                    ty: "http_connection".into(),
                    token: "connection".into(),
                }],
            ),
            call("http_receive", received, fields),
        ];
        if status.is_some() {
            calls.push(call("http_send", "failed", vec![text("write failed")]));
        }
        calls.extend([
            call(
                "http_connection_finish",
                "failed",
                vec![text("close failed")],
            ),
            call("checked_sub", "underflow", vec![]),
            call("http_listener_release", "released", vec![]),
        ]);
        let s = scenario(calls);
        let actual = serde_json::to_value(p.run(&s).unwrap()).unwrap();
        assert_eq!(
            actual,
            serde_json::to_value(expanded.run(&s).unwrap()).unwrap()
        );
        assert_eq!(actual["outcome"], "completed");
        let events = actual["events"].as_array().unwrap();
        assert!(events.iter().any(|e| e["message"] == "close failed"));
        if let Some(status) = status {
            let sent = events
                .iter()
                .find(|e| e["operation"] == "http_send")
                .unwrap();
            assert_eq!(sent["arguments"][1], json!({"kind":"u32","value":status}));
            assert!(events.iter().any(|e| e["message"] == "write failed"));
        }
        if received == "failed" {
            assert!(events.iter().any(|e| e["message"] == "read failed"));
        }
    }
    for (outcome, fields) in [("idle", vec![]), ("failed", vec![text("accept failed")])] {
        let s = scenario(vec![
            listener(),
            call("checked_sub", "difference", vec![Value::U32(0)]),
            call("http_accept", outcome, fields),
            call("http_listener_release", "released", vec![]),
        ]);
        assert_eq!(p.run(&s).unwrap().outcome, outcome);
    }
    assert_eq!(
        p.run(&scenario(vec![call(
            "http_listen",
            "failed",
            vec![text("occupied")]
        )]))
        .unwrap()
        .outcome,
        "failed"
    );
}

#[test]
fn imported_transport_keeps_completion_and_contingency_permissions() {
    let missing_release = program(|s| *s = s.replace("http.release(listener);", ""));
    assert_eq!(missing_release.unwrap_err().diagnostic.code, "E_RESOURCE");
    let ordinary_in_handler = program(|s| {
        *s=s.replace("auto result = capture http.reply_failure(borrow connection);", "auto result = capture run http.send(borrow connection, 500, \"error\", \"text/plain; charset=utf-8\", \"\");")
    });
    // Ordinary transmission is async since 0.225, so contingency code cannot enter it.
    let diagnostic = ordinary_in_handler.unwrap_err().diagnostic;
    assert_eq!(diagnostic.code, "E_ASYNC");
    assert!(
        diagnostic
            .message
            .contains("not available in contingency code"),
        "{diagnostic:?}"
    );
    let moved_twice = program(|s| {
        *s = s.replace(
            "http.release(listener);",
            "http.release(listener);http.release(listener);",
        )
    });
    assert_eq!(moved_twice.unwrap_err().diagnostic.code, "E_MOVED");
}

struct Service {
    child: Child,
    authority: String,
    tail: mpsc::Receiver<String>,
}
impl Service {
    fn start(binary: &Path, budget: u32) -> Self {
        let mut child = Command::new(binary)
            .args(["--", "0", &budget.to_string(), "3000"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (ready, address) = mpsc::channel();
        let (send, tail) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let _ = ready.send(first);
            let mut rest = String::new();
            let _ = reader.read_to_string(&mut rest);
            let _ = send.send(rest);
        });
        let mut service = Self {
            child,
            authority: String::new(),
            tail,
        };
        let line = address
            .recv_timeout(Duration::from_secs(10))
            .expect("server startup timeout");
        service.authority = line
            .trim()
            .strip_prefix("http://")
            .unwrap_or_else(|| panic!("{line}"))
            .into();
        service
    }
    fn exchange(&self, method: &str, target: &str, extra: &str, status: u32) -> String {
        let mut stream = TcpStream::connect(&self.authority).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(
                format!(
                    "{method} {target} HTTP/1.1\r\nHost: {}\r\nContent-Length: 0\r\n{extra}\r\n",
                    self.authority
                )
                .as_bytes(),
            )
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{response}"
        );
        let length: usize = headers
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        if method == "HEAD" {
            assert!(body.is_empty());
            assert!(length > 0);
        } else {
            assert_eq!(body.len(), length, "{response}");
        }
        assert_eq!(
            response.matches("HTTP/1.1 ").count(),
            1,
            "no second response: {response}"
        );
        if status == 405 {
            assert!(headers.contains("Allow: GET, HEAD"));
        }
        body.into()
    }
    fn finish(mut self) -> String {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < until, "server did not retire");
            std::thread::sleep(Duration::from_millis(10));
        }
        let tail = self.tail.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(tail.contains("completed"));
        tail
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn independent_native_service_reuses_transport_with_its_own_routes() {
    let p = program(|_| {}).unwrap();
    let f = Fixture::new();
    let binary = f.0.join("hello");
    p.build_native("main", &binary, 0).unwrap();
    let server = Service::start(&binary, 5);
    assert_eq!(
        server.exchange("GET", "/hello", "", 200),
        "Hello from Tellur 🙂"
    );
    server.exchange("HEAD", "/hello", "", 200);
    server.exchange("POST", "/hello", "", 405);
    server.exchange("GET", "/other", "", 404);
    server.exchange("GET", "/hello", "Origin: http://example.invalid\r\n", 403);
    server.finish();
    let idle = Command::new(&binary)
        .args(["--json", "--", "0", "1", "10"])
        .output()
        .unwrap();
    assert!(idle.status.success());
    assert_eq!(
        serde_json::from_slice::<Json>(&idle.stdout).unwrap()["outcome"],
        "idle"
    );
    let invalid = Command::new(&binary)
        .args(["--json", "--", "65536", "1", "10"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let report: Json = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(report["outcome"], "invalid_configuration");
    assert!(report["events"].as_array().unwrap().is_empty());
}

#[test]
fn imported_contingency_handles_fault_then_restarts_without_a_second_response() {
    let p = program(|s| {
        // Test-only invariant failures: no fault endpoint is shipped.
        *s = s.replace(
            "exit response { 404, \"Unknown endpoint\", \"\" }",
            "fault \"injected routing invariant\"",
        );
        *s = s.replace(
            "[response]: forward run http.send(",
            "[response]: forward run checked_send(",
        );
        s.push_str(
            r#"
            async control checked_send(ref http_connection connection, u32 status, string body,
                string media, string allow) exits http.send_result {
                delta await http.send(connection,status,move body,move media,move allow) {
                    sent=>sent, failed {string message}=>exit failed {message}
                }
                [sent]:delta (status==405) {true=>fault "injected after response",false=>exit sent}
            }
        "#,
        );
    })
    .unwrap();
    let f = Fixture::new();
    let binary = f.0.join("isolated");
    p.build_native("main", &binary, 2).unwrap();
    let server = Service::start(&binary, 3);
    assert_eq!(
        server.exchange("GET", "/injected", "", 500),
        "Internal server error"
    );
    assert_eq!(
        server.exchange("GET", "/hello", "", 200),
        "Hello from Tellur 🙂"
    );
    server.exchange("POST", "/hello", "", 405);
    server.finish();
}

#[test]
fn transport_rejects_a_second_receive_without_consuming_or_replacing_the_first() {
    let p = program(|s| {
        *s = s.replace("delta run http.receive(connection)", "delta checked_receive(connection)");
        s.push_str(r#"
            control checked_receive(ref http_connection connection) exits http.receive_result {
                auto first = capture run http.receive(connection);
                delta run http.receive(connection) {
                    request {string method, string target, map<string,string> headers, string body} => received,
                    malformed {string message} => malformed,
                    headers_too_large => headers_large, body_too_large => body_large,
                    unsupported_transfer => transfer, unsupported_expectation => expectation,
                    timed_out => timeout, disconnected => disconnected,
                    failed {string repeated_message} => rejected
                }
                [rejected]:emit(repeated_message);
                [entry]:exit first;
            }
        "#);
    }).unwrap();
    let f = Fixture::new();
    let binary = f.0.join("receive-once");
    p.build_native("main", &binary, 0).unwrap();
    let server = Service::start(&binary, 1);
    assert_eq!(
        server.exchange("GET", "/hello", "", 200),
        "Hello from Tellur 🙂"
    );
    let tail = server.finish();
    assert!(tail.contains("request reception already started"), "{tail}");
}
