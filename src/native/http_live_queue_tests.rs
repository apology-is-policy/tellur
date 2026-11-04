//! Live HTTP parsing and physical registration in the private source queue.
use super::*;
fn live_manifest(root: &Path) -> Manifest {
    let mut manifest = repeated_manifest(root);
    let rust = manifest.rust.as_mut().unwrap();
    rust.lockfile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_receive_mio.lock");
    rust.dependencies.insert(
        "mio".into(),
        crate::rust_host::Dependency {
            version: "=1.2.4".into(),
            features: vec!["os-poll".into(), "net".into()],
            default_features: true,
        },
    );
    let library = include_str!("../../library/host/loopback_http.rs");
    let constants = &library[library.find("const HEADER_LIMIT:").unwrap()
        ..library.find("pub fn http_listen(").unwrap()];
    let failure =
        &library[library.find("fn io_failure(").unwrap()..library.find("fn read_before(").unwrap()];
    fs::write(
        root.join("host.rs"),
        format!(
            "{}\n{constants}\n{failure}\npub mod operation {{ {} }}",
            include_str!("http_live_queue_host.rs"),
            include_str!("../../tests/fixtures/http_receive_operation.rs")
        ),
    )
    .unwrap();
    manifest
}
fn live_source(p: &CheckedProgram) -> String {
    let mut code = repeated_source(p);
    // Observe actual runtime removal, including emergency settlement, rather than
    // synthesizing cleanup in the fixture's operation wrapper.
    for key in ["key.id", "owner", "id"] {
        replace(
            &mut code,
            &format!("        self.waits.remove(&{key});"),
            &format!(
                "        if let Some(wait)=self.waits.remove(&{key}) {{crate::rust_host::retire_wait({key},wait.generation);}}"
            ),
        );
    }
    replace(
        &mut code,
        "        self.waits.clear();",
        "        for (id,wait) in std::mem::take(&mut self.waits) {crate::rust_host::retire_wait(id,wait.generation);}",
    );
    replace(
        &mut code,
        "pub struct SourcePump {",
        "pub struct SourcePump {\nnotices:BTreeMap<(u64,u64),crate::rust_host::Notice>,\nold_notices:Vec<crate::rust_host::Notice>,",
    );
    replace(
        &mut code,
        "            deadlines: BTreeMap::new(),",
        "            notices:BTreeMap::new(), old_notices:vec![],\n            deadlines: BTreeMap::new(),",
    );
    replace(
        &mut code,
        "        let token = c.park_for_readiness(key)?;",
        r#"        let token = c.park_for_readiness(key)?;
        let t=self.tickets[&key.id].clone();
        let notice=arm_live_http(c,&t,token.generation)?;
        for old in &self.old_notices {operation(c,&t)?.owned.reject(old);}
        assert!(self.notices.insert((key.id,token.generation),notice).is_none());"#,
    );
    replace(
        &mut code,
        "                let reads=crate::rust_host::read_count();\n                c.signal_readiness(&token)?;",
        r#"                let reads=crate::rust_host::read_count();
                let t=self.tickets[&key.id].clone();
                let notice=self.notices.remove(&(key.id,token.generation)).unwrap();
                operation(c,&t)?.owned.hint(&notice)?;
                self.old_notices.push(notice);
                assert_eq!(reads,crate::rust_host::read_count());
                c.signal_readiness(&token)?;"#,
    );
    replace(
        &mut code,
        "                assert!(c.signal_readiness(&token).is_err());\n                self.old.push(token);\n            }",
        r#"                assert!(c.signal_readiness(&token).is_err());
                if let Some(notice)=self.notices.remove(&(key.id,token.generation)) {self.old_notices.push(notice);}
                self.old.push(token);
            }"#,
    );
    let simulated = r#"        let resource = http_resource(c, &t)?;
        c.host
            .resources
            .get::<Probe>(&resource, "http_connection")?
            .wake(until);"#;
    replace(
        &mut code,
        simulated,
        "        let _ = until; // Live clock is never advanced by the scheduler.",
    );
    replace(
        &mut code,
        "        self.deadlines.clear();",
        "        assert!(self.notices.is_empty());\n        self.old_notices.clear();\n        self.deadlines.clear();",
    );
    replace(
        &mut code,
        "fn http_resource(",
        r#"fn arm_live_http(c:&mut Contexts<'_>,t:&Ticket,generation:u64)->R<crate::rust_host::Notice> {
    let resource=http_resource(c,t)?;
    let frame=c.parked.get_mut(&t.context.id).unwrap().machine.stack.last_mut().unwrap();
    let Some(Pending::Adapter(op))=&mut frame.pending else {unreachable!()};
    let connection=c.host.resources.get::<Probe>(&resource,"http_connection")?;
    op.owned.arm(connection,t.context.id,generation)
}
fn http_resource("#,
    );
    code
}
fn live_trace(root: &Path, instant: bool, stop: bool, success: bool) {
    let text = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    let count = |s: &str| lines.iter().filter(|l| **l == s).count();
    for name in ["left", "right", "other"] {
        let register = count(&format!("register {name}"));
        assert!(register <= 1, "{text}");
        assert_eq!(register, count(&format!("unregister {name}")), "{text}");
        assert_eq!(
            count(&format!("make {name}")),
            count(&format!("socket drop {name}")),
            "{text}"
        );
        let arms = lines
            .iter()
            .filter(|l| l.starts_with(&format!("logical arm {name} ")))
            .count();
        let retired = lines
            .iter()
            .filter(|l| l.starts_with(&format!("logical retire {name} ")))
            .count();
        assert_eq!(arms, retired, "{text}");
        if register > 0 {
            let remove = text.find(&format!("unregister {name}")).unwrap();
            assert!(
                text.rfind(&format!("logical retire {name} ")).unwrap() < remove,
                "{text}"
            );
            assert!(
                remove < text.find(&format!("state drop {name}")).unwrap(),
                "{text}"
            );
            assert!(
                remove < text.find(&format!("resource drop {name} ")).unwrap(),
                "{text}"
            );
        }
        if success {
            let never_started = stop && !instant && name == "right";
            assert_eq!(
                register,
                usize::from(!never_started && (name != "left" || !instant))
            );
            if never_started {
                assert_eq!(arms, 0);
            }
            if name == "left" && instant {
                assert_eq!(arms, 0);
            }
        }
    }
    if success && !stop {
        assert!(count("empty hint right") > 0);
        assert!(count("empty hint other") > 0);
    }
}
fn deadline_case(p: &CheckedProgram, binary: &Path, root: &Path, quotas: &str) {
    let mut calls = (0..3)
        .map(|id| ScriptCall {
            operation: "open_case".into(),
            outcome: "opened".into(),
            fields: vec![Value::Opaque {
                ty: "http_connection".into(),
                token: format!("rust:{id}"),
            }],
        })
        .collect::<Vec<_>>();
    for _ in 0..3 {
        calls.push(ScriptCall {
            operation: "close_test".into(),
            outcome: "done".into(),
            fields: vec![],
        });
    }
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(false)],
        calls,
    };
    let plans = BTreeMap::from([
        (
            "left".into(),
            vec![
                response(p, "timed_out", "left", 2),
                HttpResponse {
                    waits: 0,
                    outcome: "failed".into(),
                    fields: vec![Value::String("request reception already started".into())],
                },
            ],
        ),
        ("right".into(), vec![response(p, "request", "right", 0)]),
        ("other".into(), vec![response(p, "request", "other", 0)]),
    ]);
    let names = ["other", "left", "right"]
        .iter()
        .enumerate()
        .map(|(id, n)| (format!("rust:{id}"), n.to_string()))
        .collect();
    let q = quotas
        .split(',')
        .map(|q| q.parse().unwrap())
        .collect::<Vec<_>>();
    let expected = crate::runtime::http_source_operation_sequence_model(
        p, &scenario, 1000, &q, false, false, plans, names,
    )
    .unwrap();
    let started = std::time::Instant::now();
    let output = Command::new(binary)
        .args(["--json", "--fuel", "1000", "--", "false"])
        .env("TELLUR_HTTP_CASE", "timed_out")
        .env("TELLUR_HTTP_INSTANT", "false")
        .env("TELLUR_LIVE_DEADLINE", "true")
        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
        .env("TELLUR_SOURCE_REVERSE", "false")
        .env("TELLUR_SOURCE_EARLY", "false")
        .env("TELLUR_OPERATION_LOG", root.join("trace.log"))
        .env("TELLUR_SOURCE_EVENTS", root.join("events.json"))
        .env("TELLUR_SOURCE_STATS", root.join("stats.log"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(root.join("events.json")).unwrap())
            .unwrap(),
        serde_json::to_value(&expected).unwrap()["events"]
    );
    assert!(started.elapsed() >= std::time::Duration::from_secs(2));
    let trace = fs::read_to_string(root.join("trace.log")).unwrap();
    assert_eq!(
        trace.lines().filter(|l| *l == "real deadline left").count(),
        1
    );
    assert_eq!(
        trace
            .lines()
            .filter(|l| *l == "wait original deadline left")
            .count(),
        2
    );
    assert_eq!(
        trace
            .lines()
            .filter(|l| *l == "duplicate no read left")
            .count(),
        1
    );
    assert!(!trace.contains("register right") && !trace.contains("register other"));
    assert!(trace.find("logical retire left").unwrap() < trace.find("unregister left").unwrap());
    assert!(trace.find("unregister left").unwrap() < trace.find("state drop left").unwrap());
}
#[test]
fn live_http_queue_preserves_repeated_operations_waits_and_owning_faults() {
    let root = std::env::temp_dir().join(format!("tellur-live-http-queue-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_repeated_http.tellur"),
        live_manifest(&root),
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
            let binary = root.join(format!("live-http-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, live_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for (instant, stop) in [(false, false), (true, false), (true, true), (false, true)] {
                for reverse in [false, true] {
                    for early in [false, true] {
                        assert!(compare_repeated(
                            &p, &binary, &root, "request", instant, stop, 1000, "0,2,0,7", reverse,
                            early
                        ));
                        live_trace(&root, instant, stop, true);
                        comparisons += 1;
                    }
                }
            }
            for (instant, stop, reverse, early) in [
                (false, false, false, false),
                (true, true, true, true),
                (false, true, false, false),
                (false, true, true, true),
            ] {
                let mut finished = false;
                for fuel in 0..250 {
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        finished = compare_repeated(
                            &p, &binary, &root, "request", instant, stop, fuel, quotas, reverse,
                            early,
                        );
                        live_trace(&root, instant, stop, finished);
                        comparisons += 1;
                    }
                    if finished {
                        break;
                    }
                }
                assert!(finished);
            }
            for quotas in ["1", "0,2,0,7"] {
                deadline_case(&p, &binary, &root, quotas);
                comparisons += 1;
            }
            eprintln!("live HTTP queue: expanded={expanded}, O{opt}, comparisons={comparisons}");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[path = "http_candidate_fault_tests.rs"]
mod candidate_fault_tests;

#[path = "http_reset_queue_tests.rs"]
mod reset_queue_tests;
