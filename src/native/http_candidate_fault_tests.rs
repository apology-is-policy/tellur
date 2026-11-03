//! Actual HTTP candidates disposed by original-owner fault routing before adoption.
use super::*;
fn candidate_manifest(root: &Path) -> Manifest {
    let manifest = live_manifest(root);
    let mut host = fs::read_to_string(root.join("host.rs")).unwrap();
    replace(
        &mut host,
        "        _ => panic!(\"unknown connection\"),",
        "        _ => 7,",
    );
    replace(
        &mut host,
        "    let waits = match name.as_str() {",
        r#"    let packet = if std::env::var("TELLUR_HTTP_CANDIDATE_LABEL").unwrap()=="malformed" {format!("HEAD /{name} HTTP/1.0\r\nHost: local\r\n\r\n").into_bytes()} else {packet};
    let waits = match name.as_str() {"#,
    );
    replace(
        &mut host,
        "s.starts_with(\"resource drop other \")",
        "s.starts_with(\"resource drop owner \")",
    );
    replace(
        &mut host,
        "        trace.push(\"checkpoint\".into());",
        r#"        assert!(trace.iter().any(|s|s.starts_with("resource drop retained ")));
        for name in ["left","right"] {
            let typed=trace.iter().any(|s|s==&format!("typed candidate {name}"));
            let encoded=trace.iter().any(|s|s==&format!("candidate encode {name}"));
            assert_eq!(trace.iter().filter(|s|*s==&format!("typed candidate drop {name}")).count(),usize::from(typed&&!encoded));
            assert_eq!(trace.iter().filter(|s|*s==&format!("encoded candidate drop {name}")).count(),usize::from(encoded));
        }
        trace.push("checkpoint".into());"#,
    );
    host.push_str(include_str!("http_candidate_fault_host.rs"));
    fs::write(root.join("host.rs"), host).unwrap();
    manifest
}
fn candidate_source(p: &CheckedProgram) -> String {
    let mut code = live_source(p);
    replace(
        &mut code,
        "    owned: crate::rust_host::OwnedReceive,",
        "    owned: crate::rust_host::OwnedReceive,\n    candidate:Option<crate::rust_host::HttpCandidate>,",
    );
    replace(
        &mut code,
        "            owned: crate::rust_host::OwnedReceive::start(probe),",
        "            owned: crate::rust_host::OwnedReceive::start(probe),\n            candidate:None,",
    );
    let mut driver = include_str!("pending_fault_driver.rs").to_string();
    let from = r#"        operation(&mut c, &t)?.poll("partial")?;
        if mode == 2 {
            operation(&mut c, &t)?
                .complete("ready", vec![V::String("discarded candidate".into())])?;
        }
        wakes.push(c.park_for_readiness(&t.context)?);"#;
    replace(
        &mut driver,
        from,
        "        wakes.push(prepare_http_fault(&mut c,&t,mode)?);",
    );
    replace(
        &mut driver,
        "    if mode > 0 {",
        r#"    for t in &tickets {
        let reads=crate::rust_host::read_count();
        assert!(c.advance_retained(&t.context,0).is_err());
        assert_eq!(reads,crate::rust_host::read_count());
    }
    if mode > 0 {"#,
    );
    replace(
        &mut driver,
        "    for t in &tickets {\n        assert!(operation(&mut c, t).is_err());\n    }",
        "    for t in &tickets {\n        assert!(operation(&mut c, t).is_err());\n        assert!(poll_http(&mut c,t).is_err());\n    }",
    );
    replace(
        &mut code,
        "fn http_resource(",
        &format!(
            "{}\n{driver}\nfn http_resource(",
            include_str!("http_candidate_fault_driver.rs")
        ),
    );
    replace(
        &mut code,
        "pub use adapter_operation_model::drive_pending;",
        "pub use adapter_operation_model::{drive_pending,drive_pending_fault};",
    );
    replace(
        &mut code,
        "AdmissionStop, drive_pending};",
        "AdmissionStop, drive_pending, drive_pending_fault};",
    );
    let id = |name: &str| p.regions.keys().position(|r| r == name).unwrap();
    replace(
        &mut code,
        &format!("drive_pending(host,args,&MODEL_{})", id("main")),
        &format!(
            "drive_pending_fault(host,[&MODEL_{},&MODEL_{},&MODEL_{},&MODEL_{}])",
            id("main"),
            id("lexical"),
            id("named"),
            id("independent")
        ),
    );
    code
}
fn compare_candidate(
    p: &CheckedProgram,
    binary: &Path,
    root: &Path,
    label: &str,
    fuel: usize,
    mode: u8,
    quotas: &str,
    independent: bool,
) -> bool {
    let q = quotas
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect::<Vec<_>>();
    let responses = ["left", "right"]
        .into_iter()
        .map(|name| (name.into(), response(p, label, name, 0)))
        .collect();
    let reference =
        crate::runtime::http_candidate_fault_model(p, fuel, mode, &q, independent, responses);
    let actual = Command::new(binary)
        .args(["--json", "--fuel", "10000"])
        .env("TELLUR_HTTP_CASE", "request")
        .env("TELLUR_HTTP_INSTANT", "false")
        .env("TELLUR_LIVE_DEADLINE", "false")
        .env("TELLUR_HTTP_CANDIDATE_LABEL", label)
        .env("TELLUR_FAULT_FUEL", fuel.to_string())
        .env("TELLUR_FAULT_MODE", mode.to_string())
        .env("TELLUR_FAULT_INDEPENDENT", independent.to_string())
        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
        .env("TELLUR_OPERATION_LOG", root.join("trace.log"))
        .env("TELLUR_FAULT_EVENTS", root.join("events.json"))
        .output()
        .unwrap();
    let context = format!(
        "{} {label} fuel={fuel} mode={mode} quotas={quotas} independent={independent}",
        binary.display()
    );
    assert_eq!(
        actual.status.code(),
        Some(if reference.is_ok() { 0 } else { 1 }),
        "{context}: {}",
        String::from_utf8_lossy(&actual.stderr)
    );
    let expected = match &reference {
        Ok(result) => {
            assert!(actual.stderr.is_empty());
            let expected = serde_json::to_value(result).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                expected,
                "{context}"
            );
            expected["events"].clone()
        }
        Err(failure) => {
            assert!(actual.stdout.is_empty());
            let mut report: crate::fault::Report = serde_json::from_slice(&actual.stderr).unwrap();
            normalize_fuel(&mut report, &failure.report);
            assert_eq!(report, failure.report, "{context}");
            serde_json::to_value(&failure.events).unwrap()
        }
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(root.join("events.json")).unwrap())
            .unwrap(),
        expected,
        "{context}"
    );
    let trace = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = trace.lines().collect::<Vec<_>>();
    let count = |s: &str| lines.iter().filter(|l| **l == s).count();
    for name in ["left", "right"] {
        for kind in [
            "start",
            "state start",
            "state drop",
            "register",
            "unregister",
        ] {
            assert_eq!(count(&format!("{kind} {name}")), 1, "{context}: {trace}");
        }
        let removed = trace.find(&format!("unregister {name}")).unwrap();
        let state = trace.find(&format!("state drop {name}")).unwrap();
        let supplier = trace.find(&format!("resource drop {name} ")).unwrap();
        assert!(removed < state && state < supplier, "{context}: {trace}");
        assert_eq!(
            count(&format!("typed candidate {name}")),
            usize::from(mode >= 2)
        );
        assert_eq!(
            count(&format!("typed candidate drop {name}")),
            usize::from(mode == 2)
        );
        assert_eq!(
            count(&format!("encoded candidate drop {name}")),
            usize::from(mode == 3)
        );
        assert_eq!(
            count(&format!("candidate encode {name}")),
            usize::from(mode == 3)
        );
        if mode >= 2 {
            let kind = if mode == 2 {
                "typed candidate drop"
            } else {
                "encoded candidate drop"
            };
            let payload = trace.find(&format!("{kind} {name}")).unwrap();
            assert!(removed < payload && payload < supplier);
            assert!(trace.find(&format!("state consumed {name}")).unwrap() < payload);
        }
        assert_eq!(count(&format!("deliver {name}")), 0);
        let armed = lines
            .iter()
            .filter(|l| l.starts_with(&format!("logical arm {name} ")))
            .count();
        let retired = lines
            .iter()
            .filter(|l| l.starts_with(&format!("logical retire {name} ")))
            .count();
        assert_eq!(armed, retired);
        assert!(trace.rfind(&format!("logical retire {name} ")).unwrap() < removed);
    }
    for name in ["owner", "left", "retained", "right"] {
        assert_eq!(count(&format!("make {name}")), 1);
        assert_eq!(count(&format!("socket drop {name}")), 1);
    }
    if let Some(checkpoint) = trace.find("checkpoint") {
        for name in ["left", "retained", "right"] {
            assert!(trace.find(&format!("resource drop {name} ")).unwrap() < checkpoint);
        }
        assert!(checkpoint < trace.find("resource drop owner ").unwrap());
    }
    reference.is_ok()
}
#[test]
fn live_http_candidates_retire_before_owning_fault_contingency() {
    let root = std::env::temp_dir().join(format!("tellur-http-candidate-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_http_candidate_fault.tellur"),
        candidate_manifest(&root),
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
            let binary = root.join(format!("http-candidate-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, candidate_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for label in ["request", "malformed"] {
                for mode in 0..4 {
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        let mut finished = false;
                        for fuel in 0..64 {
                            finished = compare_candidate(
                                &p, &binary, &root, label, fuel, mode, quotas, false,
                            );
                            comparisons += 1;
                            if finished {
                                break;
                            }
                        }
                        assert!(finished);
                        assert!(compare_candidate(
                            &p, &binary, &root, label, 64, mode, quotas, true
                        ));
                        comparisons += 1;
                    }
                }
            }
            eprintln!(
                "HTTP candidate fault: expanded={expanded}, O{opt}, comparisons={comparisons}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
