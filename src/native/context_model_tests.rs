use super::*;
use serde_json::{Value as Json, json};

fn context_source(program: &CheckedProgram, setup: &str, jobs: &[(&str, &str)]) -> String {
    let ids: BTreeMap<_, _> = program
        .regions()
        .enumerate()
        .map(|(id, r)| (r.name.as_str(), id))
        .collect();
    let entry = ids["main"];
    let normal = format!(
        "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
    );
    let jobs = jobs
        .iter()
        .map(|(name, args)| format!("(&MODEL_{},{args})", ids[name]))
        .collect::<Vec<_>>()
        .join(",");
    let replacement = format!(
        r#"fn entry_{entry}(host:&mut Host,mut args:Vec<V>)->R<Completion>{{
        {setup}
        let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
        let reverse=std::env::var("TELLUR_NATIVE_MODEL_REVERSE").unwrap()=="true";
        drive_contexts(host,vec![{jobs}],&quotas,reverse)
    }}"#
    );
    let source = source(program, "main").unwrap();
    assert_eq!(source.matches(&normal).count(), 1);
    source.replacen(&normal, &replacement, 1)
}

fn reports(event: &Json, visit: &mut impl FnMut(&Json)) {
    if let Some(report) = event.get("report") {
        let mut current = Some(report);
        while let Some(report) = current {
            visit(report);
            current = report.get("cause").filter(|r| !r.is_null());
        }
    }
}

#[test]
fn independent_compiled_contexts_restore_causes_borrows_scripts_and_shared_fuel() {
    let root = std::env::temp_dir().join(format!("tellur-native-contexts-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest =
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_contexts.tellur"),
        manifest,
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = original.clone();
    // Named work and handler bodies are expanded explicitly as well as roots.
    for name in original
        .regions()
        .map(|r| r.name.clone())
        .collect::<Vec<_>>()
    {
        expanded = expanded.expanded(&name).unwrap();
    }
    let input = root.join("scenario.json");
    fs::write(
        &input,
        serde_json::to_vec(&json!({"entry":"main","arguments":[],"calls":[
            {"operation":"parse_u32","outcome":"number","fields":[{"kind":"u32","value":7}]},
            {"operation":"parse_u32","outcome":"number","fields":[{"kind":"u32","value":9}]}
        ]}))
        .unwrap(),
    )
    .unwrap();
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("contexts-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(
                &project.2,
                context_source(&program, "", &[("left", "vec![]"), ("right", "vec![]")]),
            )
            .unwrap();
            program.finish_native_build(project, opt).unwrap();
            for reverse in [false, true] {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    let mut completed = false;
                    for fuel in 0..180 {
                        let output = Command::new(&binary)
                            .args([
                                "--json",
                                "--scenario",
                                input.to_str().unwrap(),
                                "--fuel",
                                &fuel.to_string(),
                            ])
                            .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                            .env("TELLUR_NATIVE_MODEL_REVERSE", reverse.to_string())
                            .output()
                            .unwrap();
                        assert!(
                            output.status.success(),
                            "expanded={is_expanded}, opt={opt}, reverse={reverse}, quotas={quotas}, fuel={fuel}: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        let observed: Json = serde_json::from_slice(&output.stdout).unwrap();
                        let events = observed["events"].as_array().unwrap();
                        let terminals: Vec<_> = events
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e["kind"].as_str(),
                                    Some("context_complete" | "context_failed")
                                )
                            })
                            .collect();
                        assert_eq!(terminals.len(), 2);
                        assert!(terminals.iter().any(|e| e["context"] == 0));
                        assert!(terminals.iter().any(|e| e["context"] == 1));
                        let summary = events.last().unwrap();
                        assert_eq!(summary["kind"], "context_summary");
                        assert_eq!(summary["script_calls"], 2);
                        assert_eq!(summary["resources"], 0);
                        assert!(summary["cursor"].as_u64().unwrap() <= 2);
                        assert!(summary["fuel"].as_u64().unwrap() <= fuel);
                        let mut identities = BTreeMap::new();
                        let mut owners = BTreeMap::new();
                        for event in events {
                            reports(event, &mut |report| {
                                let message = report["message"].as_str().unwrap();
                                let owner = event["context"].as_u64().or_else(|| {
                                    if message.contains("left") {
                                        Some(0)
                                    } else if message.contains("right") {
                                        Some(1)
                                    } else {
                                        None
                                    }
                                });
                                let mut root = None;
                                for activation in report["activations"].as_array().unwrap() {
                                    let name = activation["control"].as_str().unwrap();
                                    if name == "left" || name == "right" {
                                        assert!(
                                            root.replace(name).is_none(),
                                            "independent roots merged"
                                        );
                                    }
                                    let id = activation["id"].as_u64().unwrap();
                                    if let Some(owner) = owner {
                                        if let Some(previous) = owners.insert(id, owner) {
                                            assert_eq!(
                                                previous, owner,
                                                "activation identity reused by another root"
                                            );
                                        }
                                    }
                                    if let Some(previous) =
                                        identities.insert(id, activation.clone())
                                    {
                                        assert_eq!(previous["control"], activation["control"]);
                                        assert_eq!(previous["parent"], activation["parent"]);
                                    }
                                }
                                if event["kind"] == "context_failed" {
                                    let forbidden = if event["context"] == 0 {
                                        "right"
                                    } else {
                                        "left"
                                    };
                                    assert!(
                                        !report["activations"]
                                            .as_array()
                                            .unwrap()
                                            .iter()
                                            .any(|a| a["control"] == forbidden)
                                    );
                                    assert!(
                                        !report["message"].as_str().unwrap().contains(forbidden)
                                    );
                                }
                            });
                        }
                        let left = terminals.iter().find(|e| e["context"] == 0).unwrap();
                        let right = terminals.iter().find(|e| e["context"] == 1).unwrap();
                        if right["kind"] == "context_complete"
                            && left["report"]["message"] == "E_FAULT: left handler failed"
                        {
                            assert_eq!(summary["cursor"], 2);
                            assert_eq!(right["outcome"], "done");
                            let messages: Vec<_> = events
                                .iter()
                                .filter(|e| e["kind"] == "emit")
                                .map(|e| e["message"].as_str().unwrap())
                                .collect();
                            assert_eq!(messages.iter().filter(|&&m| m == "left").count(), 3);
                            assert_eq!(messages.iter().filter(|&&m| m == "right").count(), 3);
                            assert_eq!(messages.iter().filter(|&&m| m == "right done").count(), 1);
                            completed = true;
                            break;
                        }
                        assert_eq!(
                            summary["fuel"], 0,
                            "a fuel cutoff must consume the shared budget"
                        );
                    }
                    assert!(completed, "both independent contexts must settle");
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn independent_compiled_resource_scopes_preserve_other_owners_during_failure() {
    let root = std::env::temp_dir().join(format!(
        "tellur-native-resource-contexts-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let manifest = super::resource_tests::resource_manifest(&root);
    let host=super::resource_tests::HOST.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let counter=Counter { path: path.into(), id }; counter.record(\"make\"); rust_api::make_outcome::made(counter)"
    );
    assert!(host.contains("counter.record(\"make\")"));
    fs::write(root.join("host.rs"), host).unwrap();
    let source = super::resource_tests::SOURCE
        .replace(
            "control main(string path, bool stop) exits done {",
            r#"boundary request(counter parent, counter child, bool stop) retains parent {
    run work(borrow parent, child, stop); close(parent); terminate;
} contingency(string reason) {close(parent);escalate;}
control root(string path, u32 parent_id, u32 child_id, bool stop) exits done faults {"#,
        )
        .replace("make(borrow path, 1)", "make(borrow path, parent_id)")
        .replace("make(borrow path, 2)", "make(borrow path, child_id)")
        .replace(
            r#"[child]: fault boundary {
        run work(borrow parent, child, stop);
    } contingency [parent] {
        close(parent);
        terminate process 23;
    }
    [child]: close(parent); exit done;"#,
            "[child]: supervise request(parent, child, stop); exit done;",
        );
    let source = format!(
        "{source}\ncontrol main(string path) exits done faults {{root(path,1,2,true);exit done;}}"
    );
    assert!(source.contains("supervise request(parent, child, stop)"));
    let original = crate::compile(&source, manifest)
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let expanded = original.expanded("root").unwrap().expanded("main").unwrap();
    let log = root.join("owners.log");
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("contexts-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(
                &project.2,
                context_source(
                    &program,
                    "let V::String(path)=args.remove(0) else {panic!(\"path\")};",
                    &[
                        (
                            "root",
                            "vec![V::String(path.clone()),V::U32(1),V::U32(2),V::Bool(true)]",
                        ),
                        (
                            "root",
                            "vec![V::String(path),V::U32(3),V::U32(4),V::Bool(false)]",
                        ),
                    ],
                ),
            )
            .unwrap();
            program.finish_native_build(project, opt).unwrap();
            for reverse in [false, true] {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    let mut finished = false;
                    for fuel in 0..180 {
                        if log.exists() {
                            fs::remove_file(&log).unwrap();
                        }
                        let output = Command::new(&binary)
                            .args([
                                "--json",
                                "--fuel",
                                &fuel.to_string(),
                                "--",
                                log.to_str().unwrap(),
                            ])
                            .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                            .env("TELLUR_NATIVE_MODEL_REVERSE", reverse.to_string())
                            .output()
                            .unwrap();
                        assert!(
                            output.status.success(),
                            "expanded={is_expanded}, opt={opt}, reverse={reverse}, quotas={quotas}, fuel={fuel}: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        let observed: Json = serde_json::from_slice(&output.stdout).unwrap();
                        let events = observed["events"].as_array().unwrap();
                        let summary = events.last().unwrap();
                        assert_eq!(summary["resources"], 0);
                        let log = fs::read_to_string(&log).unwrap_or_default();
                        let entries: Vec<_> = log.lines().collect();
                        for id in 1..=4 {
                            let positions = |kind: &str| {
                                entries
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, line)| **line == format!("{kind} {id}"))
                                    .map(|(n, _)| n)
                                    .collect::<Vec<_>>()
                            };
                            let made = positions("make");
                            let dropped = positions("drop");
                            let closed = positions("close");
                            let borrowed = positions("borrow");
                            assert_eq!(
                                made.len(),
                                dropped.len(),
                                "every acquired owner must retire: {log}"
                            );
                            assert!(
                                made.len() <= 1 && closed.len() <= 1 && borrowed.len() <= 1,
                                "duplicated action: {log}"
                            );
                            if let Some(&make) = made.first() {
                                assert!(make < dropped[0]);
                            }
                            if let Some(&close) = closed.first() {
                                assert!(close < dropped[0]);
                            }
                            if let Some(&borrow) = borrowed.first() {
                                assert!(borrow < dropped[0]);
                            }
                        }
                        let terminals: Vec<_> = events
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e["kind"].as_str(),
                                    Some("context_complete" | "context_failed")
                                )
                            })
                            .collect();
                        assert_eq!(terminals.len(), 2);
                        let failed = terminals.iter().find(|e| e["context"] == 0).unwrap();
                        let completed = terminals.iter().find(|e| e["context"] == 1).unwrap();
                        if failed["report"]["message"] == "E_FAULT: abandoned children"
                            && completed["kind"] == "context_complete"
                        {
                            for required in [
                                "make 1", "make 2", "make 3", "make 4", "drop 1", "drop 2",
                                "drop 3", "drop 4", "close 1", "close 3", "close 4", "borrow 3",
                            ] {
                                assert_eq!(
                                    entries.iter().filter(|&&line| line == required).count(),
                                    1,
                                    "{log}"
                                );
                            }
                            assert!(
                                !entries.contains(&"close 2") && !entries.contains(&"borrow 1")
                            );
                            assert!(
                                entries.iter().position(|&l| l == "drop 2").unwrap()
                                    < entries.iter().position(|&l| l == "close 1").unwrap()
                            );
                            finished = true;
                            break;
                        }
                        assert_eq!(summary["fuel"], 0);
                    }
                    assert!(finished, "resource contexts must settle");
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
