use super::*;
use serde_json::Value as Json;

#[test]
fn related_compiled_children_settle_borrowers_before_the_owning_contingency() {
    let root = std::env::temp_dir().join(format!("tellur-native-siblings-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = super::resource_tests::resource_manifest(&root);
    let host = super::resource_tests::HOST.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let counter=Counter { path: path.into(), id }; counter.record(\"make\"); rust_api::make_outcome::made(counter)",
    );
    fs::write(root.join("host.rs"), host).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_siblings.tellur"),
        manifest,
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = original.clone();
    for name in original
        .regions()
        .map(|region| region.name.clone())
        .collect::<Vec<_>>()
    {
        expanded = expanded.expanded(&name).unwrap();
    }
    let log = root.join("owners.log");
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        let names: BTreeMap<_, _> = program
            .regions()
            .enumerate()
            .map(|(id, region)| (region.name.as_str(), id))
            .collect();
        let entry = names["main"];
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let controls = [
            "main",
            "sibling",
            "descendant",
            "failing",
            "unstarted",
            "independent",
            "recovering",
            "root_unhandled",
            "ancestor",
            "sibling_named_handler",
            "sibling_lexical_handler",
        ]
        .map(|name| format!("&MODEL_{}", names[name]))
        .join(",");
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,mut args:Vec<V>)->R<Completion>{{
            let V::String(path)=args.remove(0) else {{panic!("path")}};
            let mode=std::env::var("TELLUR_SIBLING_MODE").unwrap();
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_siblings(host,[{controls}],path,&mode,&quotas)
        }}"#
        );
        let source = source(&program, "main").unwrap();
        assert_eq!(source.matches(&normal).count(), 1);
        let source = source.replacen(&normal, &replacement, 1);
        for opt in [0, 2] {
            let binary = root.join(format!("siblings-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &source).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for mode in [
                "normal",
                "fault",
                "root",
                "fuel",
                "fuel-parent",
                "fuel-peer",
                "handler-named",
                "handler-lexical",
            ] {
                let fail = mode != "normal";
                let fuel = mode.starts_with("fuel");
                let handling = mode.starts_with("handler-");
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    if log.exists() {
                        fs::remove_file(&log).unwrap();
                    }
                    let output = Command::new(&binary)
                        .args(["--json", "--fuel", "10000", "--", log.to_str().unwrap()])
                        .env("TELLUR_SIBLING_MODE", mode)
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "expanded={is_expanded},opt={opt},mode={mode},quotas={quotas}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let output: Json = serde_json::from_slice(&output.stdout).unwrap();
                    let events = output["events"].as_array().unwrap();
                    assert_eq!(events.last().unwrap()["kind"], "siblings_settled");
                    assert_eq!(events.last().unwrap()["rounds"], 3);
                    let expected = if handling {
                        "make 1\nmake 3\nborrow 1\nmake 2\nborrow 1\nmake 5\nborrow 1\ndrop 5\ndrop 2\nclose 1\ndrop 1\nborrow 3\nclose 3\ndrop 3\n"
                    } else if mode == "fuel-peer" {
                        "make 6\nmake 7\nmake 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\ndrop 5\ndrop 4\ndrop 2\ndrop 1\ndrop 7\ndrop 6\ndrop 3\n"
                    } else if mode == "fuel-parent" {
                        "make 6\nmake 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\ndrop 5\ndrop 4\ndrop 2\ndrop 1\ndrop 6\ndrop 3\n"
                    } else if fuel {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\ndrop 5\ndrop 4\ndrop 2\ndrop 1\ndrop 3\n"
                    } else if fail {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\nborrow 1\ndrop 5\ndrop 4\ndrop 2\nclose 1\ndrop 1\nborrow 3\nclose 3\ndrop 3\n"
                    } else {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\nborrow 4\nborrow 4\nclose 4\ndrop 4\nclose 2\ndrop 2\nclose 5\ndrop 5\nborrow 1\nclose 1\ndrop 1\nborrow 3\nclose 3\ndrop 3\n"
                    };
                    assert_eq!(fs::read_to_string(&log).unwrap(), expected.repeat(3));
                    let faults: Vec<_> = events
                        .iter()
                        .filter(|event| event["kind"] == "fault")
                        .collect();
                    assert_eq!(
                        faults.len(),
                        if handling {
                            9
                        } else if fail && !fuel {
                            6
                        } else {
                            3
                        }
                    );
                    assert_eq!(
                        faults
                            .iter()
                            .filter(|event| event["report"]["message"] == "E_FAULT: sibling local")
                            .count(),
                        if handling { 3 } else { 0 }
                    );
                    assert_eq!(
                        faults
                            .iter()
                            .filter(|event| event["report"]["message"] == "E_FAULT: local child")
                            .count(),
                        3
                    );
                    if fail && !fuel {
                        for (round, fault) in faults
                            .iter()
                            .filter(|event| event["report"]["message"] == "E_FAULT: child failed")
                            .enumerate()
                        {
                            let original = events
                                .iter()
                                .find(|event| {
                                    event["kind"] == "sibling_failure" && event["round"] == round
                                })
                                .unwrap();
                            assert_eq!(fault["report"]["cause"], original["report"]);
                            assert_eq!(fault["report"]["message"], "E_FAULT: child failed");
                            assert!(
                                original["report"]["activations"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .any(|activation| activation["control"] == "failing")
                            );
                        }
                    }
                    let terminal: Vec<_> = events
                        .iter()
                        .filter(|event| event["kind"] == "owner_failure")
                        .collect();
                    assert_eq!(terminal.len(), if fuel || mode == "root" { 3 } else { 0 });
                    for (round, terminal) in terminal.iter().enumerate() {
                        let original = &events
                            .iter()
                            .find(|event| {
                                event["kind"] == "sibling_failure" && event["round"] == round
                            })
                            .unwrap()["report"];
                        assert_ne!(&terminal["report"], original);
                        let mut current = &terminal["report"];
                        let mut found = false;
                        while !current.is_null() {
                            if current == original {
                                found = true;
                            }
                            current = &current["cause"];
                        }
                        assert!(found, "terminal owner lost the original child cause");
                        if fuel {
                            assert!(
                                terminal["report"]["message"]
                                    .as_str()
                                    .unwrap()
                                    .starts_with("E_FUEL")
                            );
                            assert!(terminal["report"].get("kind").is_none());
                        } else {
                            assert_eq!(terminal["report"]["kind"], "source_fault");
                        }
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn process_termination_settles_all_compiled_contexts_before_status_delivery() {
    let root = std::env::temp_dir().join(format!("tellur-native-process-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = super::resource_tests::resource_manifest(&root);
    let host = super::resource_tests::HOST.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let counter=Counter { path: path.into(), id }; counter.record(\"make\"); rust_api::make_outcome::made(counter)",
    );
    fs::write(root.join("host.rs"), host).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_siblings.tellur"),
        manifest,
    )
    .unwrap()
    .with_entry_exit_codes("process_entry", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = original.clone();
    for name in original
        .regions()
        .map(|r| r.name.clone())
        .collect::<Vec<_>>()
    {
        expanded = expanded.expanded(&name).unwrap();
    }
    let log = root.join("owners.log");
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        let names: BTreeMap<_, _> = program
            .regions()
            .enumerate()
            .map(|(id, r)| (r.name.as_str(), id))
            .collect();
        let entry = names["process_entry"];
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let controls = [
            "main",
            "sibling",
            "descendant",
            "unstarted",
            "independent",
            "sibling_named_handler",
            "sibling_lexical_handler",
            "process_entry",
            "pending_owner",
            "failing",
        ]
        .map(|name| format!("&MODEL_{}", names[name]))
        .join(",");
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,mut args:Vec<V>)->R<Completion>{{
            let V::String(path)=args.remove(0) else {{panic!("path")}};
            let mode=std::env::var("TELLUR_PROCESS_MODE").unwrap();
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_process(host,[{controls}],path,&mode,&quotas)
        }}"#
        );
        let source = source(&program, "process_entry").unwrap();
        assert_eq!(source.matches(&normal).count(), 1);
        let source = source.replacen(&normal, &replacement, 1);
        for opt in [0, 2] {
            let binary = root.join(format!("process-{is_expanded}-{opt}"));
            let project = program.native_project("process_entry", &binary).unwrap();
            fs::write(&project.2, &source).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for mode in ["work", "named", "lexical"] {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    if log.exists() {
                        fs::remove_file(&log).unwrap();
                    }
                    let output = Command::new(&binary)
                        .args(["--json", "--fuel", "10000", "--", log.to_str().unwrap()])
                        .env("TELLUR_PROCESS_MODE", mode)
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    assert_eq!(
                        output.status.code(),
                        Some(23),
                        "expanded={is_expanded},opt={opt},mode={mode},quotas={quotas}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let output: Json = serde_json::from_slice(&output.stdout).unwrap();
                    assert_eq!(output["termination"], "process");
                    assert_eq!(output["status"], 23);
                    assert!(output.get("outcome").is_none());
                    let events = output["events"].as_array().unwrap();
                    assert_eq!(events.last().unwrap()["kind"], "process_settled");
                    assert_eq!(
                        events
                            .iter()
                            .filter(|e| e["kind"] == "process_settled")
                            .count(),
                        3
                    );
                    let faults: Vec<_> = events.iter().filter(|e| e["kind"] == "fault").collect();
                    assert_eq!(faults.len(), if mode == "work" { 3 } else { 6 });
                    for fault in faults
                        .iter()
                        .filter(|e| e["report"]["message"] == "E_FAULT: process stop")
                    {
                        assert!(fault["report"].get("cause").is_none());
                    }
                    let expected = if mode == "work" {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 4\nmake 5\nmake 9\nborrow 9\ndrop 9\nmake 8\ndrop 8\ndrop 5\ndrop 4\ndrop 2\ndrop 3\ndrop 1\n"
                    } else {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 5\nmake 9\nborrow 9\ndrop 9\nmake 8\ndrop 8\ndrop 5\ndrop 2\ndrop 3\ndrop 1\n"
                    };
                    assert_eq!(fs::read_to_string(&log).unwrap(), expected.repeat(3));
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
