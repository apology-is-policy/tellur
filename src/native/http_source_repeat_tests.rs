//! Repeated operations in one retained async source context.
use super::*;
fn repeated_manifest(root: &Path) -> Manifest {
    let manifest = queue_manifest(root);
    let mut host = fs::read_to_string(root.join("host.rs")).unwrap();
    replace(
        &mut host,
        "\"right\" => \"failed\",",
        "\"right\" => \"request\",",
    );
    replace(
        &mut host,
        "pub struct OwnedReceive {",
        "pub struct OwnedReceive {\n    duplicate: bool,",
    );
    replace(
        &mut host,
        "            name: connection.name.clone(),",
        "            duplicate: connection.receive_started.get(),\n            name: connection.name.clone(),",
    );
    replace(
        &mut host,
        "        let progress = self",
        "        let before=connection.observed();\n        let progress = self",
    );
    replace(
        &mut host,
        "        match &progress {",
        r#"        if self.duplicate {
            assert_eq!(before,connection.observed());
            assert!(matches!(&progress,operation::Progress::Complete(rust_api::http_receive_outcome::failed(message)) if message=="request reception already started"));
            self.trace.borrow_mut().push(format!("duplicate no read {}",self.name));
        }
        match &progress {"#,
    );
    fs::write(root.join("host.rs"), host).unwrap();
    manifest
}
fn repeated_source(p: &CheckedProgram) -> String {
    let mut code = queue_source(p);
    replace(
        &mut code,
        "    let mut pump=super::adapter_operation_model::SourcePump::new();\n    let result = loop {",
        "    let mut pump=super::adapter_operation_model::SourcePump::new();\n    let mut turns=0usize;\n    let result = loop {\n        turns+=1; assert!(turns<10000, \"bounded repeated HTTP driver made no progress\");",
    );
    code
}

fn compare_repeated(
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
        })
        .collect::<Vec<_>>();
    if stop {
        calls.push(ScriptCall {
            operation: "checkpoint".into(),
            outcome: "done".into(),
            fields: vec![],
        });
    }
    for _ in 0..if stop { 1 } else { 3 } {
        calls.push(ScriptCall {
            operation: "close_test".into(),
            outcome: "done".into(),
            fields: vec![],
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
            vec![
                response(p, label, "left", if instant { 0 } else { 2 }),
                HttpResponse {
                    waits: 0,
                    outcome: "failed".into(),
                    fields: vec![Value::String("request reception already started".into())],
                },
            ],
        ),
        ("right".into(), vec![response(p, "request", "right", 4)]),
        ("other".into(), vec![response(p, "request", "other", 7)]),
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
    let reference = crate::runtime::http_source_operation_sequence_model(
        p, &scenario, fuel, &q, reverse, early, plans, names,
    );
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
    let count = |s: &str| lines.iter().filter(|l| **l == s).count();
    for name in ["left", "right", "other"] {
        let started = count(&format!("state start {name}"));
        assert!(started <= if name == "left" { 2 } else { 1 }, "{context}");
        assert_eq!(count(&format!("state drop {name}")), started, "{context}");
        assert_eq!(
            count(&format!("make {name}")),
            lines
                .iter()
                .filter(|l| l.starts_with(&format!("resource drop {name} ")))
                .count(),
            "{context}"
        );
        if started > 0 {
            assert!(
                trace.rfind(&format!("state drop {name}")).unwrap()
                    < trace.find(&format!("resource drop {name} ")).unwrap(),
                "{context}"
            );
        }
        if reference.is_ok() {
            let completed = if name == "left" {
                if stop { 1 } else { 2 }
            } else if name == "right" {
                usize::from(!stop)
            } else {
                1
            };
            assert_eq!(
                count(&format!("state consumed {name}")),
                completed,
                "{context}"
            );
            assert_eq!(count(&format!("deliver {name}")), completed, "{context}");
        }
    }
    if reference.is_ok() {
        let replacements = lines
            .iter()
            .filter_map(|l| l.strip_prefix("replacement "))
            .filter(|l| !l.starts_with("wait "))
            .collect::<Vec<_>>();
        assert_eq!(
            replacements.len(),
            if stop { 1 } else { 2 },
            "{context}: {trace}"
        );
        let first = replacements[0].split_whitespace().collect::<Vec<_>>();
        assert_eq!(first[1], "right");
        if !stop {
            let duplicate = replacements[1].split_whitespace().collect::<Vec<_>>();
            assert_eq!(
                duplicate[0], first[0],
                "same retained context must host all three operations"
            );
            assert_eq!(duplicate[1], "left");
            assert_eq!(count("duplicate no read left"), 1);
            if !instant {
                assert!(
                    lines.iter().any(|l| l
                        .starts_with(&format!("replacement wait {} right stale=", first[0]))
                        && !l.ends_with("stale=0")),
                    "{context}: {trace}"
                );
            }
        }
        assert_eq!(
            count("wait original deadline left"),
            if instant { 0 } else { 2 }
        );
        if !stop {
            assert_eq!(count("wait original deadline right"), 4);
        }
        assert_eq!(count("wait original deadline other"), 7);
        if stop {
            let checkpoint = trace.find("checkpoint").unwrap();
            for name in ["left", "right"] {
                assert!(trace.find(&format!("resource drop {name} ")).unwrap() < checkpoint);
            }
            assert!(trace.find("resource drop other ").unwrap() > checkpoint);
        }
    }
    reference.is_ok()
}
#[test]
fn native_http_repeated_context_rejects_previous_operations_and_waits() {
    let root = std::env::temp_dir().join(format!("tellur-http-repeat-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_repeated_http.tellur"),
        repeated_manifest(&root),
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
            let binary = root.join(format!("repeat-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, repeated_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for label in ["request", "timed_out"] {
                for (instant, stop) in [(false, false), (true, false), (true, true)] {
                    for reverse in [false, true] {
                        for early in [false, true] {
                            for quotas in ["1", "0,2,0,7", "3,1,9"] {
                                assert!(compare_repeated(
                                    &p, &binary, &root, label, instant, stop, 1000, quotas,
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
            ] {
                let mut finished = false;
                for fuel in 0..250 {
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        finished = compare_repeated(
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
            eprintln!(
                "native repeated HTTP: expanded={expanded}, O{opt}, comparisons={comparisons}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[path = "http_live_queue_tests.rs"]
mod live_queue_tests;
