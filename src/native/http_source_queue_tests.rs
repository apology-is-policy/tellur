//! Owned HTTP parsing on the existing checked source queue, against an independent
//! reference completion/readiness plan. No public asynchronous host admission.
use super::*;
use crate::runtime::{HttpResponse, http_source_operation_model};

fn queue_manifest(root: &Path) -> Manifest {
    let manifest = http_manifest(root);
    let mut host = fs::read_to_string(root.join("host.rs")).unwrap();
    replace(
        &mut host,
        "pub struct Connection {",
        "pub struct Connection {\npub name: String,",
    );
    replace(
        &mut host,
        "            alive: Rc::new(Cell::new(true)),",
        "            name: String::new(),\n            alive: Rc::new(Cell::new(true)),",
    );
    replace(
        &mut host,
        "        w.reads += 1;",
        "        w.reads += 1;\n        READS.with(|n|n.set(n.get()+1));",
    );
    replace(
        &mut host,
        "pub struct OwnedReceive {",
        "pub struct OwnedReceive {\nname: String,",
    );
    replace(
        &mut host,
        "            state: Some(operation::ReceiveState::start(connection)),",
        "            name: connection.name.clone(),\n            state: Some(operation::ReceiveState::start(connection)),",
    );
    replace(
        &mut host,
        "push(\"state start\".into())",
        "push(format!(\"state start {}\",connection.name))",
    );
    for message in ["state consumed", "state drop", "wait original deadline"] {
        replace(
            &mut host,
            &format!("push(\"{message}\".into())"),
            &format!("push(format!(\"{message} {{}}\",self.name))"),
        );
    }
    replace(
        &mut host,
        "\"resource drop head={} bytes={}\",",
        "\"resource drop {} head={} bytes={}\",\n            self.name,",
    );
    let start = host.find("pub fn open_case(").unwrap();
    let end = host[start..].find("pub fn close_test(").unwrap() + start;
    host.replace_range(start..end, include_str!("http_source_queue_host.rs"));
    fs::write(root.join("host.rs"), host).unwrap();
    let mut config = serde_json::to_value(manifest).unwrap();
    config["primitives"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == "close_test")
        .unwrap()["contingent"] = json!(true);
    config["primitives"].as_array_mut().unwrap().push(json!({"name":"checkpoint","parameters":[],"outcomes":[{"name":"done","fields":[]}],"effects":["checkpoint"],"returns":true,"contingent":true,"rust":{"execution":"immediate"}}));
    Manifest::from_json_at(&config.to_string(), &root.join("manifest.json")).unwrap()
}
fn queue_source(p: &CheckedProgram) -> String {
    let mut code = http_source(p);
    replace(
        &mut code,
        "        let request = String::from(\"http\");",
        "        let request = probe.name.clone();",
    );
    let mut pump = include_str!("source_operation_pump.rs").to_string();
    replace(
        &mut pump,
        "pub struct SourcePump {",
        "pub struct SourcePump {\ndeadlines: BTreeMap<u64,std::time::Instant>,\nretired_tickets: Vec<Ticket>,",
    );
    replace(
        &mut pump,
        "            tickets: BTreeMap::new(),",
        "            deadlines: BTreeMap::new(),\n            retired_tickets: vec![],\n            tickets: BTreeMap::new(),",
    );
    replace(
        &mut pump,
        "        self.park(c, key)?;\n        Ok(true)",
        "        assert!(self.eligible.insert(key.id));\n        Ok(false)",
    );
    // Completed tickets stay valid until adoption; remove only the active-map
    // entry, retaining their identities for rejection checks on replacements.
    replace(
        &mut pump,
        "        self.tickets.insert(key.id, ticket(c, key));",
        r#"
        let next=ticket(c,key);
        let replacements=self.retired_tickets.iter().filter(|old|old.context.id==key.id).count();
        for old in &self.retired_tickets {
            if old.context.id==key.id {assert!(operation(c,old).is_err());}
        }
        if replacements>0 {
            let op=operation(c,&next)?;
            op.lease.trace.borrow_mut().push(format!("replacement {} {}",key.id,op.lease.request));
        }
        assert!(self.tickets.insert(key.id,next).is_none());"#,
    );
    replace(
        &mut pump,
        "        self.peak_waiting = self.peak_waiting.max(c.waits.len());",
        r#"
        let before=(c.waits.clone(),c.host.fuel,c.host.events.clone(),c.host.cursor,crate::rust_host::read_count());
        for old in &self.old { assert!(c.signal_readiness(old).is_err()); }
        for old in &self.retired_tickets {assert!(operation(c,old).is_err());}
        assert_eq!(before,(c.waits.clone(),c.host.fuel,c.host.events.clone(),c.host.cursor,crate::rust_host::read_count()));
        if self.retired_tickets.iter().any(|old|old.context.id==key.id) {
            let t=self.tickets[&key.id].clone();
            let op=operation(c,&t)?;
            let stale=self.old.iter().filter(|w|w.context==key.id).count();
            op.lease.trace.borrow_mut().push(format!("replacement wait {} {} stale={stale}",key.id,op.lease.request));
        }
        self.peak_waiting = self.peak_waiting.max(c.waits.len());"#,
    );
    replace(
        &mut pump,
        "        for t in self.tickets.values() {",
        "        for t in self.tickets.values().chain(self.retired_tickets.iter()) {",
    );
    let start = pump.find("    pub fn poll_ready(").unwrap();
    let end = pump[start..].find("    pub fn finish(").unwrap() + start;
    pump.replace_range(start..end, include_str!("http_source_queue_poll.rs"));
    replace(
        &mut pump,
        "                c.signal_readiness(&token)?;",
        "                let reads=crate::rust_host::read_count();\n                c.signal_readiness(&token)?;\n                assert_eq!(reads,crate::rust_host::read_count());",
    );
    replace(
        &mut pump,
        "        self.tickets.clear();",
        "        self.deadlines.clear();\n        self.retired_tickets.clear();\n        self.tickets.clear();",
    );
    replace(
        &mut code,
        "pub fn drive_pending(",
        &format!("{pump}\npub fn drive_pending("),
    );
    let mut scheduler = super::super::source_tests::pump_scheduler();
    replace(
        &mut scheduler,
        "        let poll = contexts.advance_retained(&key, quantum)?;",
        "        let reads=crate::rust_host::read_count();\n        let poll=contexts.advance_retained(&key, quantum)?;\n        if quantum==0 {assert_eq!(reads,crate::rust_host::read_count());}",
    );
    replace(&mut code, include_str!("source_scheduler.rs"), &scheduler);
    code
}
fn response(p: &CheckedProgram, label: &str, name: &str, waits: usize) -> HttpResponse {
    let string = |s: &str| Value::String(s.into());
    let fields = match label {
        "request" => {
            let shape = p
                .manifest
                .primitives
                .iter()
                .find(|p| p.name == "http_receive")
                .unwrap()
                .outcomes
                .iter()
                .find(|o| o.name == "request")
                .unwrap();
            let body = format!("{name}:🙂");
            vec![
                string("HEAD"),
                string(&format!("/{name}")),
                Value::Construct {
                    ty: shape.fields[2].ty.clone(),
                    name: "map".into(),
                    fields: vec![
                        string("content-length"),
                        string(&body.len().to_string()),
                        string("host"),
                        string("local"),
                        string("x-name"),
                        string(name),
                    ],
                },
                string(&body),
            ]
        }
        "malformed" => vec![string("HTTP/1.1 required")],
        "failed" => vec![string("injected read error")],
        _ => vec![],
    };
    HttpResponse {
        waits,
        outcome: label.into(),
        fields,
    }
}
fn compare_queue(
    p: &CheckedProgram,
    binary: &Path,
    root: &Path,
    label: &str,
    instant: bool,
    stop: bool,
    fuel: usize,
    quotas: &str,
    reverse: bool,
    early: bool,
) -> bool {
    let mut calls = (0..3)
        .map(|id| ScriptCall {
            operation: "open_case".into(),
            outcome: "opened".into(),
            fields: vec![Value::Opaque {
                ty: "http_connection".into(),
                token: format!("rust:{id}"),
            }],
            waits: 0,
        })
        .collect::<Vec<_>>();
    // Left has completed and closed before the fault only in the immediate case.
    if stop && instant {
        calls.push(ScriptCall {
            operation: "close_test".into(),
            outcome: "done".into(),
            fields: vec![],
            waits: 0,
        });
    }
    if stop {
        calls.push(ScriptCall {
            operation: "checkpoint".into(),
            outcome: "done".into(),
            fields: vec![],
            waits: 0,
        });
    }
    for _ in 0..if stop { 1 } else { 3 } {
        calls.push(ScriptCall {
            operation: "close_test".into(),
            outcome: "done".into(),
            fields: vec![],
            waits: 0,
        });
    }
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(stop)],
        calls,
    };
    let plans = BTreeMap::from([
        (
            "left".into(),
            response(p, label, "left", if instant { 0 } else { 2 }),
        ),
        ("right".into(), response(p, "failed", "right", 4)),
        ("other".into(), response(p, "request", "other", 7)),
    ]);
    let names = ["other", "left", "right"]
        .iter()
        .enumerate()
        .map(|(id, name)| (format!("rust:{id}"), name.to_string()))
        .collect();
    let q = quotas
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect::<Vec<_>>();
    let reference =
        http_source_operation_model(p, &scenario, fuel, &q, reverse, early, plans, names);
    let actual = Command::new(binary)
        .args([
            "--json",
            "--fuel",
            &fuel.to_string(),
            "--",
            &stop.to_string(),
        ])
        .env("TELLUR_HTTP_CASE", label)
        .env("TELLUR_HTTP_INSTANT", instant.to_string())
        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
        .env("TELLUR_SOURCE_REVERSE", reverse.to_string())
        .env("TELLUR_SOURCE_EARLY", early.to_string())
        .env("TELLUR_OPERATION_LOG", root.join("trace.log"))
        .env("TELLUR_SOURCE_EVENTS", root.join("events.json"))
        .env("TELLUR_SOURCE_STATS", root.join("stats.log"))
        .output()
        .unwrap();
    let context = format!(
        "{} {label} instant={instant} stop={stop} fuel={fuel} quotas={quotas} reverse={reverse} early={early}",
        binary.display()
    );
    assert_eq!(
        actual.status.code(),
        Some(if reference.is_ok() { 0 } else { 1 }),
        "{context}: {}",
        String::from_utf8_lossy(&actual.stderr)
    );
    let events = match &reference {
        Ok(result) => {
            assert!(actual.stderr.is_empty(), "{context}");
            let expected = serde_json::to_value(result).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                expected,
                "{context}"
            );
            expected["events"].clone()
        }
        Err(failure) => {
            assert!(actual.stdout.is_empty(), "{context}");
            let mut report: crate::fault::Report = serde_json::from_slice(&actual.stderr).unwrap();
            normalize_fuel(&mut report, &failure.report);
            assert_eq!(report, failure.report, "{context}");
            serde_json::to_value(&failure.events).unwrap()
        }
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(root.join("events.json")).unwrap())
            .unwrap(),
        events,
        "{context}"
    );
    let trace = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = trace.lines().collect::<Vec<_>>();
    let count = |s: &str| lines.iter().filter(|line| **line == s).count();
    for name in ["left", "right", "other"] {
        let started = count(&format!("state start {name}"));
        assert!(started <= 1, "{context}");
        assert_eq!(count(&format!("state drop {name}")), started, "{context}");
        let made = count(&format!("make {name}"));
        let drops = lines
            .iter()
            .filter(|l| l.starts_with(&format!("resource drop {name} ")))
            .count();
        assert_eq!(made, drops, "{context}");
        if started > 0 {
            let state = trace.find(&format!("state drop {name}")).unwrap();
            let resource = trace.find(&format!("resource drop {name} ")).unwrap();
            assert!(state < resource, "{context}");
        }
        if reference.is_ok() {
            let completed = !stop || name == "other" || (instant && name == "left");
            assert_eq!(
                count(&format!("state consumed {name}")),
                usize::from(completed),
                "{context}"
            );
            assert_eq!(
                count(&format!("deliver {name}")),
                usize::from(completed),
                "{context}"
            );
            if completed {
                let waits = match name {
                    "left" => {
                        if instant {
                            0
                        } else {
                            2
                        }
                    }
                    "right" => 4,
                    _ => 7,
                };
                assert_eq!(
                    count(&format!("wait original deadline {name}")),
                    waits,
                    "{context}"
                );
            }
        }
    }
    reference.is_ok()
}
#[test]
fn native_http_queue_matches_reference_outcomes_waits_and_owning_faults() {
    let root = std::env::temp_dir().join(format!("tellur-http-queue-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_pending_http.tellur"),
        queue_manifest(&root),
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
            let binary = root.join(format!("http-queue-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, queue_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for label in [
                "request",
                "malformed",
                "headers_too_large",
                "body_too_large",
                "unsupported_transfer",
                "unsupported_expectation",
                "timed_out",
                "disconnected",
                "failed",
            ] {
                for instant in [false, true] {
                    for stop in [false, true] {
                        for reverse in [false, true] {
                            for early in [false, true] {
                                assert!(compare_queue(
                                    &p, &binary, &root, label, instant, stop, 1000, "0,2,0,7",
                                    reverse, early
                                ));
                                comparisons += 1;
                            }
                        }
                    }
                }
            }
            for (label, instant, stop, reverse, early) in [
                ("request", false, false, false, false),
                ("request", true, true, true, true),
                ("timed_out", false, false, true, true),
                ("failed", false, true, false, true),
            ] {
                let mut finished = false;
                for fuel in 0..200 {
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        finished = compare_queue(
                            &p, &binary, &root, label, instant, stop, fuel, quotas, reverse, early,
                        );
                        comparisons += 1;
                    }
                    if finished {
                        break;
                    }
                }
                assert!(finished);
            }
            eprintln!("native HTTP queue: expanded={expanded}, O{opt}, comparisons={comparisons}");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[path = "http_source_repeat_tests.rs"]
mod repeat_tests;
