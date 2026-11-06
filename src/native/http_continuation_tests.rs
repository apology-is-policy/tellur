//! Actual HTTP signature and typed outcomes through retained checked continuations.
use super::*;

fn replace(text: &mut String, from: &str, to: &str) {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    *text = text.replacen(from, to, 1);
}
fn http_manifest(root: &Path) -> Manifest {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = repo.join("examples/http_hello");
    let original: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(example.join("manifest.json")).unwrap()).unwrap();
    let receive = original["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "http_receive")
        .unwrap()
        .clone();
    fs::copy(example.join("Cargo.lock"), root.join("Cargo.lock")).unwrap();
    let driver = include_str!("../../tests/fixtures/http_receive_driver.rs");
    let mut host = driver[..driver.find("fn outcome(").unwrap()].to_string();
    replace(
        &mut host,
        "struct Connection {",
        "pub struct Connection {\npub alive: Rc<Cell<bool>>,\npub trace: Rc<RefCell<Vec<String>>>,",
    );
    replace(
        &mut host,
        "            stream: Stream {",
        "            alive: Rc::new(Cell::new(true)), trace: TRACE.with(Clone::clone),\n            stream: Stream {",
    );
    replace(
        &mut host,
        "    consumed: usize,",
        "    consumed: usize,\n    reads: usize,",
    );
    replace(
        &mut host,
        "                    consumed: 0,",
        "                    consumed: 0,\n                    reads: 0,",
    );
    replace(
        &mut host,
        "        loop {\n            let Some(event)",
        "        w.reads += 1;\n        loop {\n            let Some(event)",
    );
    let library = include_str!("../../library/host/loopback_http.rs");
    let constants = &library[library.find("const HEADER_LIMIT:").unwrap()
        ..library.find("pub fn http_listen(").unwrap()];
    let failure =
        &library[library.find("fn io_failure(").unwrap()..library.find("fn read_before(").unwrap()];
    let baseline = &library
        [library.find("fn read_before(").unwrap()..library.find("pub fn http_send(").unwrap()];
    let before = baseline.find("pub fn http_receive(").unwrap();
    let read = baseline[..before]
        .replace("&std::net::TcpStream", "&Stream")
        .replace("std::time::Instant::now()", "stream.now()");
    let receive_body =
        baseline[before..].replace("std::time::Instant::now()", "connection.stream.now()");
    host.push_str(&format!("\n{constants}\n{failure}\nmod baseline {{use super::*;{read}\n{receive_body}}}\npub mod operation {{ {} }}\n{}",
        include_str!("../../tests/fixtures/http_receive_operation.rs"),include_str!("http_continuation_host.rs")));
    fs::write(root.join("host.rs"), host).unwrap();
    Manifest::from_json_at(&json!({
        "version":"0.1", "types":[{"name":"http_connection","kind":"opaque","linear":true,"fault":"release"}],
        "rust":{"source":"host.rs","lockfile":"Cargo.lock","dependencies":original["rust"]["dependencies"],"resources":{"http_connection":"Connection"}},
        "primitives":[
            {"name":"open_case","parameters":[{"name":"name","type":"string","mode":"consume"}],"outcomes":[{"name":"opened","fields":[{"name":"connection","type":"http_connection"}]}],"effects":["open"],"returns":true,"rust":{"execution":"immediate"}},
            receive,
            {"name":"close_test","parameters":[{"name":"connection","type":"http_connection","mode":"consume"}],"outcomes":[{"name":"done","fields":[]}],"effects":["close"],"returns":true,"rust":{"execution":"immediate"}}
        ]
    }).to_string(), &root.join("manifest.json")).unwrap()
}
fn http_source(p: &CheckedProgram) -> String {
    let mut code = source(p, true);
    replace(
        &mut code,
        &continuations::pending_operation_source(p),
        &continuations::pending_receive_source(p),
    );
    replace(
        &mut code,
        "type Probe = crate::rust_host::Probe;",
        "type Probe = crate::rust_host::Connection;",
    );
    code = code
        .replace(
            "get::<Probe>(&resource, \"probe\")",
            "get::<Probe>(&resource, \"http_connection\")",
        )
        .replace(
            "get::<Probe>(&self.resource, \"probe\")",
            "get::<Probe>(&self.resource, \"http_connection\")",
        );
    replace(
        &mut code,
        "let [V::Reference { root, fields, .. }, V::String(_)]",
        "let [V::Reference { root, fields, .. }]",
    );
    replace(
        &mut code,
        "expected resource loan and owned request",
        "expected one shared connection loan",
    );
    replace(
        &mut code,
        "        let V::String(request) = arguments.pop().unwrap() else {\n            unreachable!()\n        };",
        "        let request = String::from(\"http\");",
    );
    replace(
        &mut code,
        "    lease: Lease,",
        "    owned: crate::rust_host::OwnedReceive,\n    lease: Lease,",
    );
    replace(
        &mut code,
        "            lease: Lease {",
        "            owned: crate::rust_host::OwnedReceive::start(probe),\n            lease: Lease {",
    );
    replace(
        &mut code,
        "        if label != \"ready\" || !matches!(fields.as_slice(), [V::String(_)]) {\n            return Err(\"E_MODEL: invalid completion contract\".into());\n        }",
        "        // The generated encoder and finish_http_model validate this complete contract.",
    );
    let old = include_str!("pending_operation_driver.rs");
    let mut driver = old.to_string();
    replace(
        &mut driver,
        "        if pending {",
        "        let quantum = turns.next().unwrap();\n        if pending && quantum == 0 {\n            let t=ticket(&c,&selected); let before=observed(&mut c,&t)?;\n            assert!(matches!(c.advance_retained(&selected,0)?, ContextPoll::Paused));\n            assert_eq!(before,observed(&mut c,&t)?); continue;\n        }\n        if pending {",
    );
    let start = driver
        .find("            let t = ticket(&c, &selected);")
        .unwrap();
    let end = driver[start..]
        .find("        match c.advance_retained")
        .unwrap()
        + start;
    driver.replace_range(start..end,"            receive_pending(&mut c,&selected,&mut retired_wakes,&mut completed_tickets)?;\n        }\n");
    replace(
        &mut driver,
        "c.advance_retained(&selected, turns.next().unwrap())",
        "c.advance_retained(&selected, quantum)",
    );
    replace(
        &mut code,
        old,
        &format!("{}\n{driver}", include_str!("http_continuation_driver.rs")),
    );
    // Use the generated Rust outcome encoders, including the canonical map layout.
    let arms = code
        .lines()
        .filter(|l| {
            l.trim_start()
                .starts_with("rust_api::http_receive_outcome::")
        })
        .collect::<Vec<_>>();
    assert_eq!(arms.len(), 9);
    let encoder = format!(
        "\nfn encode_http_result(result:rust_api::http_receive_outcome)->R<Outcome>{{match result {{ {} }} }}\n",
        arms.join("\n")
    );
    code.push_str(&encoder);
    let begin = SUPPORT.find("    pub fn primitive(").unwrap();
    let end = SUPPORT[begin..].find("    fn check_contract(").unwrap() + begin;
    let mut finish = SUPPORT[begin..end].to_string();
    replace(
        &mut finish,
        "pub fn primitive(&mut self, name: &str, args: Vec<V>)",
        "pub fn finish_http_model(&mut self, name: &str, args: Vec<V>, result: Outcome)",
    );
    let begin = finish.find("        let (outcome, values) = if").unwrap();
    let end = finish[begin..]
        .find("        let o = p[\"outcomes\"]")
        .unwrap()
        + begin;
    finish.replace_range(begin..end, "        let (outcome, values) = result;\n");
    replace(
        &mut code,
        "    fn check_contract(",
        &format!("{finish}\n    fn check_contract("),
    );
    replace(
        &mut code,
        "    baseline::http_receive(connection)",
        "    panic!(\"synchronous receive replayed\")",
    );
    code
}
#[test]
fn http_outcomes_and_deadlines_match_retained_continuations_at_all_fuel_cuts() {
    let root =
        std::env::temp_dir().join(format!("tellur-http-continuation-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_pending_http.tellur"),
        http_manifest(&root),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    let mut comparisons = 0;
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let normal = root.join(format!("normal-{expanded}-{opt}"));
            let pending = root.join(format!("pending-{expanded}-{opt}"));
            for (binary, model) in [(&normal, false), (&pending, true)] {
                let project = p.native_project("main", binary).unwrap();
                fs::write(
                    &project.2,
                    if model {
                        http_source(&p)
                    } else {
                        source(&p, false)
                    },
                )
                .unwrap();
                p.finish_native_build(project, opt).unwrap();
            }
            for (name, label) in [
                ("request", "request"),
                ("split", "request"),
                ("malformed", "malformed"),
                ("headers_too_large", "headers_too_large"),
                ("body_too_large", "body_too_large"),
                ("unsupported_transfer", "unsupported_transfer"),
                ("unsupported_expectation", "unsupported_expectation"),
                ("timed_out", "timed_out"),
                ("body_deadline", "timed_out"),
                ("disconnected", "disconnected"),
                ("failed", "failed"),
                ("incomplete", "malformed"),
            ] {
                let run = |binary: &Path, fuel: usize, quotas: &str| {
                    let output = Command::new(binary)
                        .args(["--json", "--fuel", &fuel.to_string(), "--", name])
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .env("TELLUR_OPERATION_LOG", root.join("trace.log"))
                        .output()
                        .unwrap();
                    let log = fs::read_to_string(root.join("trace.log")).unwrap_or_default();
                    (output, log)
                };
                let (full, _) = run(&normal, 1000, "1");
                assert!(
                    full.status.success(),
                    "{}",
                    String::from_utf8_lossy(&full.stderr)
                );
                let full: serde_json::Value = serde_json::from_slice(&full.stdout).unwrap();
                let events = full["events"].as_array().unwrap();
                let calls = events
                    .iter()
                    .filter(|e| {
                        e["kind"] == "primitive"
                            && ["open_case", "http_receive", "close_test"]
                                .iter()
                                .any(|n| e["operation"] == *n)
                    })
                    .map(|e| ScriptCall {
                        operation: e["operation"].as_str().unwrap().into(),
                        outcome: e["outcome"].as_str().unwrap().into(),
                        fields: serde_json::from_value(e["fields"].clone()).unwrap(),
                        waits: 0,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(calls.len(), 4);
                assert_eq!(calls[1].outcome, label);
                assert_eq!(calls[2].outcome, "failed");
                assert_eq!(
                    calls[2].fields,
                    vec![Value::String("request reception already started".into())]
                );
                let scenario = Scenario {
                    entry: "main".into(),
                    arguments: vec![Value::String(name.into())],
                    calls,
                };
                let mut finished = false;
                for fuel in 0..100 {
                    let reference = p.run_completion(&scenario, fuel);
                    let (expected, resource_log) = run(&normal, fuel, "1");
                    match &reference {
                        Ok(value) => assert_eq!(
                            serde_json::from_slice::<serde_json::Value>(&expected.stdout).unwrap(),
                            serde_json::to_value(value).unwrap()
                        ),
                        Err(error) => {
                            assert_eq!(expected.status.code(), Some(1));
                            let mut report: crate::fault::Report =
                                serde_json::from_slice(&expected.stderr).unwrap();
                            normalize_fuel(&mut report, &error.report);
                            assert_eq!(report, error.report);
                        }
                    }
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        let (actual, log) = run(&pending, fuel, quotas);
                        let context = format!(
                            "{name} expanded={expanded} O{opt} fuel={fuel} quotas={quotas}"
                        );
                        assert_eq!(
                            actual.status.code(),
                            expected.status.code(),
                            "{context}: {}",
                            String::from_utf8_lossy(&actual.stderr)
                        );
                        assert_eq!(actual.stdout, expected.stdout, "{context}");
                        assert_eq!(actual.stderr, expected.stderr, "{context}");
                        let resource = log
                            .lines()
                            .filter(|l| l.starts_with("resource drop"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        assert_eq!(resource, resource_log, "{context}");
                        let count = |s: &str| log.lines().filter(|l| *l == s).count();
                        let started = count("state start");
                        assert_eq!(count("state consumed"), started, "{context}");
                        assert_eq!(count("state drop"), started, "{context}");
                        assert_eq!(count("deliver http"), started, "{context}");
                        if started > 0 {
                            assert!(
                                log.rfind("state drop").unwrap()
                                    < log.find("resource drop").unwrap(),
                                "{context}"
                            );
                        }
                        comparisons += 1;
                    }
                    if reference.is_ok() {
                        finished = true;
                        break;
                    }
                }
                assert!(finished);
            }
        }
    }
    eprintln!("HTTP continuation comparisons: {comparisons}");
    fs::remove_dir_all(root).unwrap();
}

#[path = "http_source_queue_tests.rs"]
mod source_queue_tests;
