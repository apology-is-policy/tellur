use serde_json::{Value as Json, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
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
        let root = std::env::temp_dir().join(format!(
            "tellur-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn program() -> CheckedProgram {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("applications/settings");
    let manifest = Manifest::from_json_at(
        &fs::read_to_string(root.join("manifest.json")).unwrap(),
        &root.join("manifest.json"),
    )
    .unwrap();
    compile_file(root.join("main.tellur"), manifest).unwrap()
}
fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn opaque(ty: &str, token: &str) -> Value {
    Value::Opaque {
        ty: ty.into(),
        token: token.into(),
    }
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
    }
}
fn scenario(calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: "serve".into(),
        arguments: vec![
            text("settings.json"),
            Value::U32(0),
            Value::U32(1),
            Value::U32(100),
        ],
        calls,
    }
}
fn listener() -> ScriptCall {
    call(
        "http_listen",
        "listening",
        vec![opaque("http_listener", "listener"), text("127.0.0.1:1234")],
    )
}
fn accepted() -> ScriptCall {
    call(
        "http_accept",
        "accepted",
        vec![opaque("http_connection", "connection")],
    )
}
fn sub(n: u32) -> ScriptCall {
    call("checked_sub", "difference", vec![Value::U32(n)])
}
fn headers() -> Value {
    Value::Construct {
        ty: "map<String,String>".into(),
        name: "map".into(),
        fields: vec![text("host"), text("127.0.0.1:1234")],
    }
}
fn string_map(entries: &[(&str, &str)]) -> Value {
    let entries = entries
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    Value::Construct {
        ty: "map<String,String>".into(),
        name: "map".into(),
        fields: entries
            .into_iter()
            .flat_map(|(k, v)| [text(k), text(v)])
            .collect(),
    }
}
fn failures() -> Vec<(Scenario, &'static str, Option<u32>)> {
    let mut cases = vec![
        (
            scenario(vec![call("http_listen", "failed", vec![text("occupied")])]),
            "failed",
            None,
        ),
        (
            scenario(vec![
                listener(),
                sub(0),
                call("http_accept", "idle", vec![]),
                call("http_listener_release", "released", vec![]),
            ]),
            "idle",
            None,
        ),
        (
            scenario(vec![
                listener(),
                sub(0),
                call("http_accept", "failed", vec![text("accept failure")]),
                call("http_listener_release", "released", vec![]),
            ]),
            "failed",
            None,
        ),
    ];
    for (receive, fields, status) in [
        ("malformed", vec![text("bad framing")], 400),
        ("headers_too_large", vec![], 431),
        ("body_too_large", vec![], 413),
        ("unsupported_transfer", vec![], 501),
        ("unsupported_expectation", vec![], 417),
        ("timed_out", vec![], 408),
    ] {
        cases.push((
            scenario(vec![
                listener(),
                sub(0),
                accepted(),
                call("http_receive", receive, fields),
                call("http_send", "sent", vec![]),
                call("http_connection_close", "closed", vec![]),
                call("checked_sub", "underflow", vec![]),
                call("http_listener_release", "released", vec![]),
            ]),
            "completed",
            Some(status),
        ));
    }
    for (receive_failed, close_failed) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        cases.push((
            scenario(vec![
                listener(),
                sub(0),
                accepted(),
                if receive_failed {
                    call("http_receive", "failed", vec![text("receive failure")])
                } else {
                    call("http_receive", "disconnected", vec![])
                },
                if close_failed {
                    call(
                        "http_connection_close",
                        "failed",
                        vec![text("cleanup failure")],
                    )
                } else {
                    call("http_connection_close", "closed", vec![])
                },
                call("checked_sub", "underflow", vec![]),
                call("http_listener_release", "released", vec![]),
            ]),
            "completed",
            None,
        ));
    }
    let received = call(
        "http_receive",
        "request",
        vec![text("GET"), text("/missing"), headers(), text("")],
    );
    cases.push((
        scenario(vec![
            listener(),
            sub(0),
            accepted(),
            received,
            call("http_send", "failed", vec![text("send failure")]),
            call(
                "http_connection_close",
                "failed",
                vec![text("cleanup failure")],
            ),
            call("checked_sub", "underflow", vec![]),
            call("http_listener_release", "released", vec![]),
        ]),
        "completed",
        Some(404),
    ));
    let mut idle_after_failure = scenario(vec![
        listener(),
        sub(1),
        accepted(),
        call("http_receive", "failed", vec![text("read failure")]),
        call("http_connection_close", "closed", vec![]),
        sub(0),
        call("http_accept", "idle", vec![]),
        call("http_listener_release", "released", vec![]),
    ]);
    idle_after_failure.arguments[2] = Value::U32(2);
    cases.push((idle_after_failure, "idle", None));
    for outcome in ["write_failed", "commit_uncertain", "too_large"] {
        let received = call(
            "http_receive",
            "request",
            vec![
                text("PUT"),
                text("/settings"),
                string_map(&[
                    ("host", "127.0.0.1:1234"),
                    ("content-type", "application/json"),
                ]),
                text(r#"{"key":"theme","value":"dark"}"#),
            ],
        );
        cases.push((
            scenario(vec![
                listener(),
                sub(0),
                accepted(),
                received,
                call(
                    "json_decode",
                    "decoded",
                    vec![string_map(&[("key", "theme"), ("value", "dark")])],
                ),
                call("text_trim", "trimmed", vec![text("theme")]),
                call("file_read", "missing", vec![]),
                call("json_encode", "encoded", vec![text(r#"{"theme":"dark"}"#)]),
                call(
                    "file_replace",
                    outcome,
                    if outcome == "too_large" {
                        vec![]
                    } else {
                        vec![text("storage detail")]
                    },
                ),
                call("http_send", "sent", vec![]),
                call("http_connection_close", "closed", vec![]),
                call("checked_sub", "underflow", vec![]),
                call("http_listener_release", "released", vec![]),
            ]),
            "completed",
            Some(if outcome == "too_large" { 413 } else { 500 }),
        ));
    }
    cases
}
fn report(program: &CheckedProgram, scenario: &Scenario) -> Json {
    serde_json::to_value(program.run(scenario).unwrap()).unwrap()
}
#[test]
fn scripted_http_failures_preserve_cleanup_order_and_both_errors() {
    let p = program();
    let expanded = p.expanded("serve").unwrap();
    for (scenario, expected, status) in failures() {
        let result = report(&p, &scenario);
        assert_eq!(result["outcome"], expected);
        assert_eq!(result, report(&expanded, &scenario));
        let events = result["events"].as_array().unwrap();
        if let Some(status) = status {
            let sent = events
                .iter()
                .find(|e| e["operation"] == "http_send")
                .unwrap();
            assert_eq!(sent["arguments"][1], json!({"kind":"u32","value":status}));
            if scenario
                .calls
                .iter()
                .any(|c| c.outcome == "commit_uncertain")
            {
                assert!(
                    sent["arguments"][2]["value"]
                        .as_str()
                        .unwrap()
                        .contains("commit_uncertain: reload before retrying")
                );
            }
        }
        if expected == "completed" {
            for call in &scenario.calls {
                if call.outcome == "failed" {
                    let Value::String(message) = &call.fields[0] else {
                        panic!()
                    };
                    assert!(events.iter().any(|e| e["message"] == *message), "{result}");
                }
            }
        }
    }
}
#[test]
fn http_entry_rejects_invalid_configuration_without_host_calls() {
    let p = program();
    for (port, limit, idle) in [
        (65536, 1, 100),
        (0, 0, 100),
        (0, 1000001, 100),
        (0, 1, 0),
        (0, 1, 60001),
    ] {
        let mut s = scenario(vec![]);
        s.arguments = vec![
            text("settings.json"),
            Value::U32(port),
            Value::U32(limit),
            Value::U32(idle),
        ];
        assert_eq!(p.run(&s).unwrap().outcome, "invalid_configuration");
    }
}

struct Server {
    child: Option<Child>,
    stdout: Option<BufReader<ChildStdout>>,
    authority: String,
}
impl Server {
    fn start(binary: &Path, data: &Path, budget: usize, idle_ms: u32) -> Self {
        let child = Command::new(binary)
            .args(["--entry", "serve", "--"])
            .arg(data)
            .args(["0", &budget.to_string(), &idle_ms.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut server = Self {
            child: Some(child),
            stdout: None,
            authority: String::new(),
        };
        let stdout = server.child.as_mut().unwrap().stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let result = reader.read_line(&mut line);
            let _ = send.send((reader, line, result));
        });
        let (reader, line, result) = receive
            .recv_timeout(Duration::from_secs(10))
            .expect("server startup timed out");
        result.unwrap();
        assert!(line.starts_with("http://127.0.0.1:"), "{line}");
        server.authority = line.trim().strip_prefix("http://").unwrap().into();
        server.stdout = Some(reader);
        server
    }
    fn finish(mut self, code: i32, outcome: &str) {
        let until = Instant::now() + Duration::from_secs(15);
        while self.child.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < until, "server did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = self.child.take().unwrap().wait_with_output().unwrap();
        let mut tail = String::new();
        self.stdout
            .take()
            .unwrap()
            .read_to_string(&mut tail)
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{tail}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(tail.contains(outcome), "{tail}");
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn exchange(authority: &str, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(authority).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = vec![];
    let result = stream.read_to_end(&mut response);
    if let Err(e) = result {
        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset, "{e}");
    }
    response
}
fn request(authority: &str, method: &str, target: &str, body: &str, extra: &str) -> Vec<u8> {
    format!("{method} {target} HTTP/1.1\r\nHost: {authority}\r\nContent-Length: {}\r\n{extra}\r\n{body}",body.len()).into_bytes()
}
fn response(bytes: &[u8], expected: u32, head: bool) -> String {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("{text:?}"));
    assert!(
        headers.starts_with(&format!("HTTP/1.1 {expected} ")),
        "{text}"
    );
    let length: usize = headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse()
        .unwrap();
    if head {
        assert!(body.is_empty());
    } else {
        assert_eq!(body.len(), length, "{text}");
    }
    assert!(headers.contains("Connection: close"));
    body.into()
}

#[test]
fn native_http_serves_persistent_settings_and_rejects_invalid_requests() {
    let p = program();
    let f = Fixture::new();
    let binary = f.0.join("server");
    p.build_native("serve", &binary, 0).unwrap();
    let data = f.0.join("settings.json");
    let cases = [
        ("GET", "/settings", "", "", 200),
        (
            "PUT",
            "/settings",
            r#"{"key":"theme","value":"dark 🦉"}"#,
            "Content-Type: application/json\r\n",
            200,
        ),
        (
            "POST",
            "/lookup",
            r#"{"key":"theme"}"#,
            "Content-Type: application/json\r\n",
            200,
        ),
        (
            "POST",
            "/lookup",
            r#"{"key":"absent"}"#,
            "Content-Type: application/json\r\n",
            404,
        ),
        (
            "PUT",
            "/settings",
            r#"{"key":" bad ","value":"x"}"#,
            "Content-Type: application/json\r\n",
            400,
        ),
        (
            "PUT",
            "/settings",
            r#"{"key":"x","value":1}"#,
            "Content-Type: application/json\r\n",
            400,
        ),
        (
            "PUT",
            "/settings",
            r#"{"key":"x","key":"y"}"#,
            "Content-Type: application/json\r\n",
            400,
        ),
        (
            "PUT",
            "/settings",
            r#"{"key":"x","value":"y","extra":"z"}"#,
            "Content-Type: application/json\r\n",
            400,
        ),
        ("PUT", "/settings", r#"{"key":"x","value":"y"}"#, "", 415),
        (
            "PUT",
            "/settings",
            r#"{"key":"x","value":"y"}"#,
            "Content-Type: application/json\r\nOrigin: https://example.invalid\r\n",
            403,
        ),
        ("GET", "/settings", "x", "", 400),
        ("GET", "/missing", "", "", 404),
        ("GET", "/settings?query=1", "", "", 404),
        ("POST", "/settings", "", "", 405),
        ("GET", "/lookup", "", "", 405),
        ("HEAD", "/settings", "", "", 405),
        ("GET", "/settings", "", "", 200),
    ];
    let server = Server::start(&binary, &data, cases.len(), 3000);
    for (i, (method, target, body, extra, status)) in cases.iter().enumerate() {
        let bytes = exchange(
            &server.authority,
            &request(&server.authority, method, target, body, extra),
        );
        let body = response(&bytes, *status, *method == "HEAD");
        if i == 0 {
            assert_eq!(serde_json::from_str::<Json>(&body).unwrap(), json!({}));
        }
        if i == 2 {
            assert_eq!(
                serde_json::from_str::<Json>(&body).unwrap(),
                json!({"value":"dark 🦉"})
            );
        }
        if *status == 405 {
            assert!(String::from_utf8_lossy(&bytes).contains("Allow: "));
        }
    }
    server.finish(0, "completed");
    assert_eq!(
        serde_json::from_str::<Json>(&fs::read_to_string(&data).unwrap()).unwrap(),
        json!({"theme":"dark 🦉"})
    );

    let server = Server::start(&binary, &data, 1, 3000);
    let body = response(
        &exchange(
            &server.authority,
            &request(&server.authority, "GET", "/settings", "", ""),
        ),
        200,
        false,
    );
    assert_eq!(
        serde_json::from_str::<Json>(&body).unwrap(),
        json!({"theme":"dark 🦉"})
    );
    server.finish(0, "completed");

    fs::write(&data, "invalid JSON").unwrap();
    let server = Server::start(&binary, &data, 2, 3000);
    response(
        &exchange(
            &server.authority,
            &request(&server.authority, "GET", "/settings", "", ""),
        ),
        500,
        false,
    );
    response(
        &exchange(
            &server.authority,
            &request(
                &server.authority,
                "PUT",
                "/settings",
                r#"{"key":"x","value":"y"}"#,
                "Content-Type: application/json\r\n",
            ),
        ),
        500,
        false,
    );
    server.finish(0, "completed");
    assert_eq!(fs::read_to_string(&data).unwrap(), "invalid JSON");

    let server = Server::start(&binary, &data, 1, 50);
    let address = server.authority.clone();
    server.finish(0, "idle");
    let occupied = TcpListener::bind(&address).unwrap();
    let output = Command::new(&binary)
        .args(["--json", "--entry", "serve", "--"])
        .arg(&data)
        .args([
            &occupied.local_addr().unwrap().port().to_string(),
            "1",
            "100",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "failed");
}

#[test]
fn native_http_bounds_framing_deadlines_and_disconnects() {
    let p = program();
    let f = Fixture::new();
    let binary = f.0.join("server");
    p.build_native("serve", &binary, 0).unwrap();
    let data = f.0.join("settings.json");
    let server = Server::start(&binary, &data, 17, 3000);
    let a = &server.authority;
    let malformed = vec![
        ("GET /settings HTTP/1.1\r\n\r\n".to_string(), 400),
        (
            format!("GET /settings HTTP/1.1\r\nHost: {a}\r\nHost: {a}\r\n\r\n"),
            400,
        ),
        (
            format!(
                "PUT /settings HTTP/1.1\r\nHost: {a}\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n"
            ),
            400,
        ),
        (
            format!("GET /settings HTTP/1.1\r\nHost: {a}\r\nContent-Length: +1\r\n\r\nx"),
            400,
        ),
        (
            format!(
                "PUT /settings HTTP/1.1\r\nHost: {a}\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n"
            ),
            400,
        ),
        (
            format!(
                "PUT /settings HTTP/1.1\r\nHost: {a}\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
            ),
            501,
        ),
        (
            format!("PUT /settings HTTP/1.1\r\nHost: {a}\r\nExpect: 100-continue\r\n\r\n"),
            417,
        ),
        (
            format!("PUT /settings HTTP/1.1\r\nHost: {a}\r\nContent-Length: 1048577\r\n\r\n"),
            413,
        ),
        (
            format!("PUT /settings HTTP/1.1\r\nHost: {a}\r\nContent-Length: 50\r\n\r\nshort"),
            400,
        ),
        (format!("GET /settings HTTP/1.1\nHost: {a}\n\n"), 400),
        (format!("GET /settings HTTP/1.0\r\nHost: {a}\r\n\r\n"), 400),
        (
            "GET /settings HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n".to_string(),
            403,
        ),
        (
            format!(
                "GET /settings HTTP/1.1\r\nHost: {a}\r\nX-Long: {}\r\n\r\n",
                "a".repeat(17000)
            ),
            431,
        ),
        (
            format!(
                "GET /settings HTTP/1.1\r\nHost: {a}\r\n{}\r\n",
                (0..65).map(|i| format!("X-{i}: x\r\n")).collect::<String>()
            ),
            431,
        ),
    ];
    for (raw, status) in malformed {
        response(&exchange(a, raw.as_bytes()), status, false);
    }
    // An empty disconnected client consumes one budget slot and is closed.
    drop(TcpStream::connect(a).unwrap());
    // A deadline covers the entire request, rather than restarting per read.
    let mut slow = TcpStream::connect(a).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    let start = Instant::now();
    slow.write_all(b"GET").unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    slow.write_all(b" ").unwrap();
    let mut bytes = vec![];
    slow.read_to_end(&mut bytes).unwrap();
    response(&bytes, 408, false);
    assert!(start.elapsed() < Duration::from_millis(2900));
    // Only the first pipelined request is executed.
    let first = request(a, "GET", "/settings", "", "");
    let second = request(
        a,
        "PUT",
        "/settings",
        r#"{"key":"hidden","value":"write"}"#,
        "Content-Type: application/json\r\n",
    );
    let bytes = exchange(a, &[first, second].concat());
    response(&bytes, 200, false);
    server.finish(0, "completed");
    assert!(!data.exists());
}

#[test]
fn native_http_scripted_failures_match_expansion_at_both_optimization_levels() {
    let p = program();
    let f = Fixture::new();
    for expanded in [false, true] {
        let program = if expanded {
            p.expanded("serve").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = f.0.join(format!("script-{expanded}-{opt}"));
            program.build_native("serve", &binary, opt).unwrap();
            for (i, (scenario, expected, _)) in failures().into_iter().enumerate() {
                let path = f.0.join(format!("case-{i}.json"));
                fs::write(&path, serde_json::to_vec(&scenario).unwrap()).unwrap();
                let output = Command::new(&binary)
                    .arg("--scenario")
                    .arg(path)
                    .output()
                    .unwrap();
                let actual: Json = serde_json::from_slice(&output.stdout)
                    .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
                assert_eq!(actual, report(&p, &scenario));
                assert_eq!(
                    output.status.code(),
                    Some(if expected == "failed" || expected.ends_with("_failures") {
                        1
                    } else {
                        0
                    })
                );
            }
        }
    }
}

#[test]
fn native_http_source_fault_sends_500_then_serves_next_request() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("applications/settings");
    let m = Manifest::from_json_at(
        &fs::read_to_string(root.join("manifest.json")).unwrap(),
        &root.join("manifest.json"),
    )
    .unwrap();
    // Fault injection belongs to this test's source loader, never the shipped route.
    let p=tellur::modules::compile_with_loader(root.join("main.tellur"),m,|path| {
        let mut source=fs::read_to_string(path)?;
        if path.ends_with("http.tellur") {
            source=source.replace("exits response faults {", "exits response faults { delta (target == \"/injected-fault\") {true=>broken,false=>valid} [broken]: fault \"injected routing invariant\"; [valid]:");
        }
        if path.ends_with("server.tellur") {
            source = source.replace("delta http_send(", "delta checked_send(");
            source.push_str(r#"
                control checked_send(ref http_connection connection, u32 status, ref string body,
                    ref string media, ref string allow) exits sent, failed {string message} {
                    delta http_send(connection,status,body,media,allow) {
                        sent=>sent, failed {string message}=>failed
                    }
                    [failed]:exit failed {message};
                    [sent]:delta (status==404) {true=>broken,false=>healthy}
                    [broken]:fault "injected post-response invariant";
                    [healthy]:exit sent;
                }
            "#);
        }
        Ok(source)
    }).unwrap();
    let f = Fixture::new();
    let binary = f.0.join("isolated");
    p.build_native("serve", &binary, 0).unwrap();
    let server = Server::start(&binary, &f.0.join("settings.json"), 3, 3000);
    let first = response(
        &exchange(
            &server.authority,
            &request(&server.authority, "GET", "/injected-fault", "", ""),
        ),
        500,
        false,
    );
    assert_eq!(first, "Internal server error");
    assert!(!first.contains("invariant"));
    response(
        &exchange(
            &server.authority,
            &request(&server.authority, "GET", "/settings", "", ""),
        ),
        200,
        false,
    );
    // The 404 has already been written. A second HTTP response would violate
    // Content-Length and this assertion; contingency must only close now.
    response(
        &exchange(
            &server.authority,
            &request(&server.authority, "GET", "/missing-after-write", "", ""),
        ),
        404,
        false,
    );
    server.finish(0, "completed");
}
