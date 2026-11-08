//! Generated source FIFO and private operation-readiness differential evidence.
use super::*;

fn resource_manifest(root: &Path) -> Manifest {
    let mut config = serde_json::to_value(manifest(root)).unwrap();
    config["primitives"].as_array_mut().unwrap().push(json!({
        "name":"checkpoint","parameters":[],"outcomes":[{"name":"done","fields":[]}],
        "effects":["checkpoint"],"returns":true,"contingent":true,"rust":{"execution":"immediate"}
    }));
    let host=HOST
        .replace("pub struct Probe {","pub struct Probe { pub id:usize,")
        .replace("push(\"resource drop\".into())","push(format!(\"resource drop {}\",self.id))")
        .replace("trace.borrow_mut().push(\"make\".into());","let id=trace.borrow().iter().filter(|s|s.starts_with(\"make \")).count();trace.borrow_mut().push(format!(\"make {id}\"));")
        .replace("Probe {alive:","Probe {id,alive:")
        .replace("push(\"close\".into())","push(format!(\"close {}\",value.id))")
        .replace("format!(\"response to {request}\")","format!(\"rust:{}:AB\",value.id)");
    fs::write(root.join("host.rs"),format!(r#"{host}
pub fn checkpoint()->rust_api::checkpoint_outcome {{
    TRACE.with(|t|{{
        let mut trace=t.borrow_mut();
        for request in ["left","right"] {{assert!(trace.iter().any(|s|s==&format!("drop {{request}}")));}}
        for id in 2..6 {{assert!(trace.iter().any(|s|s==&format!("resource drop {{id}}")));}}
        for id in 0..2 {{assert!(!trace.iter().any(|s|s==&format!("resource drop {{id}}")));}}
        trace.push("checkpoint".into());
    }});
    rust_api::checkpoint_outcome::done
}}
"#)).unwrap();
    Manifest::from_json_at(&config.to_string(), &root.join("manifest.json")).unwrap()
}
fn pump_source(p: &CheckedProgram) -> String {
    let mut code = source(p, true);
    let driver = include_str!("pending_operation_driver.rs");
    assert_eq!(code.matches(driver).count(), 1);
    code = code.replacen(
        driver,
        &format!("{driver}\n{}", include_str!("source_operation_pump.rs")),
        1,
    );
    let original = include_str!("source_scheduler.rs");
    assert_eq!(code.matches(original).count(), 1);
    code.replacen(original, &pump_scheduler(), 1)
}
pub(super) fn pump_scheduler() -> String {
    let original = include_str!("source_scheduler.rs");
    let mut scheduler = original.to_string();
    let replace = |s: &mut String, from: &str, to: &str| {
        assert_eq!(s.matches(from).count(), 1, "{from}");
        *s = s.replacen(from, to, 1);
    };
    replace(
        &mut scheduler,
        "    drive_source(host, args, control, || usize::MAX)",
        r#"    let q:Vec<usize>=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse().unwrap()).collect();
    assert!(q.iter().any(|q|*q>0));
    let mut turns=q.into_iter().cycle();
    drive_source(host,args,control,||turns.next().unwrap())"#,
    );
    replace(
        &mut scheduler,
        "    let result = loop {",
        "    let mut pump=super::adapter_operation_model::SourcePump::new();\n    let result = loop {",
    );
    replace(
        &mut scheduler,
        "        } else {\n            loop {",
        "        } else {\n            for key in pump.signal(&mut contexts,false)? {push(&mut runnable,key);}\n            loop {",
    );
    replace(
        &mut scheduler,
        "                    None => {\n                        return Err",
        "                    None => {\n                        let ready=pump.signal(&mut contexts,true)?;\n                        if !ready.is_empty() {for key in ready {push(&mut runnable,key);}continue;}\n                        return Err",
    );
    replace(
        &mut scheduler,
        "        let poll = contexts.advance_retained(&key, quota())?;",
        "        let quantum=quota();\n        if quantum>0 && pump.poll_ready(&mut contexts,&key)? {continue;}\n        let poll = contexts.advance_retained(&key, quantum)?;",
    );
    replace(
        &mut scheduler,
        "            ContextPoll::Paused => selected = Some(key),",
        "            ContextPoll::Paused => {if !pump.park_new(&mut contexts,&key)? {selected=Some(key);}},",
    );
    replace(
        &mut scheduler,
        "    if !contexts.parked.is_empty()",
        "    pump.finish(&mut contexts)?;\n    if !contexts.parked.is_empty()",
    );
    scheduler
}
fn resource_scenario(stop: bool) -> Scenario {
    let call = |operation: &str| ScriptCall {
        operation: operation.into(),
        outcome: "done".into(),
        fields: vec![],
        waits: 0,
    };
    let mut calls = (0..6)
        .map(|id| ScriptCall {
            operation: "make".into(),
            outcome: "made".into(),
            fields: vec![Value::Opaque {
                ty: "probe".into(),
                token: format!("rust:{id}"),
            }],
            waits: 0,
        })
        .collect::<Vec<_>>();
    if stop {
        calls.push(call("checkpoint"));
    }
    for _ in 0..if stop { 1 } else { 3 } {
        for op in ["inspect", "close", "close"] {
            calls.push(call(op));
        }
    }
    Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(stop)],
        calls,
    }
}
fn compare_run(
    p: &CheckedProgram,
    binary: &Path,
    root: &Path,
    stop: bool,
    fuel: usize,
    reverse: bool,
    early: bool,
    quotas: &str,
) -> bool {
    let q = quotas
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect::<Vec<_>>();
    let reference = crate::runtime::source_operation_model(
        p,
        &resource_scenario(stop),
        fuel,
        &q,
        reverse,
        early,
    );
    let trace = root.join("trace.log");
    let events = root.join("events.json");
    let stats = root.join("stats.log");
    let actual = Command::new(binary)
        .args([
            "--json",
            "--fuel",
            &fuel.to_string(),
            "--",
            &stop.to_string(),
        ])
        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
        .env("TELLUR_SOURCE_REVERSE", reverse.to_string())
        .env("TELLUR_SOURCE_EARLY", early.to_string())
        .env("TELLUR_OPERATION_LOG", &trace)
        .env("TELLUR_SOURCE_EVENTS", &events)
        .env("TELLUR_SOURCE_STATS", &stats)
        .output()
        .unwrap();
    let context = format!(
        "{}, stop={stop},fuel={fuel},reverse={reverse},early={early},quotas={quotas}",
        binary.display()
    );
    assert_eq!(
        actual.status.code(),
        Some(if reference.is_ok() { 0 } else { 1 }),
        "{context}: {}",
        String::from_utf8_lossy(&actual.stderr)
    );
    let expected_events = match &reference {
        Ok(result) => {
            let expected = serde_json::to_value(result).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                expected,
                "{context}"
            );
            assert!(actual.stderr.is_empty(), "{context}");
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
        serde_json::from_slice::<serde_json::Value>(&fs::read(&events).unwrap()).unwrap(),
        expected_events,
        "{context}"
    );
    let trace = fs::read_to_string(trace).unwrap();
    let lines = trace.lines().collect::<Vec<_>>();
    let count = |text: &str| lines.iter().filter(|s| **s == text).count();
    let position = |text: &str| lines.iter().position(|s| *s == text).unwrap();
    let created = lines.iter().filter(|s| s.starts_with("make ")).count();
    for id in 0..created {
        assert_eq!(
            count(&format!("resource drop {id}")),
            1,
            "{context}: {trace}"
        );
    }
    for (request, id) in [("other", 1), ("left", 3), ("right", 5)] {
        let n = count(&format!("start {request}"));
        assert!(n <= 1);
        assert_eq!(count(&format!("drop {request}")), n, "{context}");
        if n > 0 {
            assert!(
                position(&format!("drop {request}")) < position(&format!("resource drop {id}")),
                "{context}"
            );
        }
        let completed = count(&format!("sample {request}"));
        assert_eq!(count(&format!("deliver {request}")), completed, "{context}");
        let polled = lines
            .iter()
            .filter(|s| s.starts_with(&format!("poll {request} ")))
            .count();
        assert!(polled <= 3 * n);
        if completed > 0 {
            assert_eq!(polled, 3);
        }
    }
    if reference.is_ok() {
        assert_eq!(created, 6);
        let stats = fs::read_to_string(stats)
            .unwrap()
            .split_whitespace()
            .map(|s| s.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        assert!(stats[0] > 0 && stats[1] >= 2);
        assert_eq!(count("sample other"), 1);
        for request in ["left", "right"] {
            assert_eq!(count(&format!("sample {request}")), usize::from(!stop));
        }
        assert_eq!(count("checkpoint"), usize::from(stop));
    }
    reference.is_ok()
}
#[test]
fn generated_source_pending_operations_preserve_turns_faults_and_resources() {
    let root = std::env::temp_dir().join(format!("tellur-source-pending-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_pending_operation.tellur"),
        resource_manifest(&root),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("pending-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, pump_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for stop in [false, true] {
                for reverse in [false, true] {
                    for early in [false, true] {
                        let mut finished = false;
                        for fuel in 0..200 {
                            for quotas in ["1", "0,2,0,7", "3,1,9"] {
                                finished = compare_run(
                                    &p, &binary, &root, stop, fuel, reverse, early, quotas,
                                );
                            }
                            if finished {
                                break;
                            }
                        }
                        assert!(finished);
                        assert!(compare_run(
                            &p, &binary, &root, stop, 1000, reverse, early, "0,2,0,7"
                        ));
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[path = "owned_operation_tests.rs"]
mod owned_contract_tests;
