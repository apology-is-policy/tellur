use super::*;
use serde_json::Value as Json;

#[test]
fn compiled_independent_child_cold_awaits_preserve_recovery_causes_and_fuel_partitions() {
    let root = std::env::temp_dir().join(format!("tellur-mixed-await-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_mixed_await.tellur"),
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = original.clone();
    for name in original
        .regions()
        .map(|r| r.name.clone())
        .collect::<Vec<_>>()
    {
        expanded = expanded.expanded(&name).unwrap();
    }
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        let ids: BTreeMap<_, _> = program
            .regions()
            .enumerate()
            .map(|(id, r)| (r.name.as_str(), id))
            .collect();
        let entry = ids["main"];
        let worker = ids["worker"];
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_mixed_await(host,&MODEL_{entry},&MODEL_{worker},args,&quotas)
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        assert_eq!(generated.matches(&normal).count(), 1);
        let generated = generated.replacen(&normal, &replacement, 1).replacen(
            CONTEXTS,
            &format!("{CONTEXTS}\n{}", include_str!("mixed_await_model.rs")),
            1,
        );
        let mut baseline = BTreeMap::new();
        for opt in [0, 2] {
            let binary = root.join(format!("mixed-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for stop in [false, true] {
                for quotas in ["99", "1", "0,3,1"] {
                    let mut completed = false;
                    for fuel in 0..120 {
                        let output = Command::new(&binary)
                            .args([
                                "--json",
                                "--fuel",
                                &fuel.to_string(),
                                "--",
                                &stop.to_string(),
                            ])
                            .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                            .output()
                            .unwrap();
                        let context = format!(
                            "expanded={is_expanded},opt={opt},stop={stop},quotas={quotas},fuel={fuel}"
                        );
                        assert!(
                            output.status.success(),
                            "{context}: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        let observed: Json = serde_json::from_slice(&output.stdout).unwrap();
                        if opt == 0 && quotas == "99" {
                            baseline.insert((stop, fuel), observed.clone());
                        } else {
                            assert_eq!(&observed, &baseline[&(stop, fuel)], "{context}");
                        }
                        let events = observed["events"].as_array().unwrap();
                        let summary = events.last().unwrap();
                        assert_eq!(summary["kind"], "mixed_settled");
                        let failed = events.iter().find(|e| e["kind"] == "mixed_failed");
                        if let Some(failed) = failed {
                            assert_eq!(failed["report"]["boundary"], "main");
                            // The independent child's cause must never be named
                            // after its internal cold-await relay or leaf.
                            if !failed["report"]["cause"].is_null() {
                                assert_eq!(failed["report"]["cause"]["boundary"], "worker");
                            }
                        }
                        if summary["fuel"].as_u64().unwrap() > 0 {
                            let messages: Vec<_> = events
                                .iter()
                                .filter(|e| e["kind"] == "emit")
                                .map(|e| e["message"].as_str().unwrap())
                                .collect();
                            assert_eq!(
                                messages,
                                if stop {
                                    vec!["changed", "child invariant", "changed", "changed"]
                                } else {
                                    vec![
                                        "changed",
                                        "child invariant",
                                        "changed",
                                        "changed",
                                        "changed",
                                    ]
                                }
                            );
                            assert_eq!(failed.is_some(), stop);
                            if stop {
                                assert_eq!(
                                    failed.unwrap()["report"]["cause"]["message"],
                                    "E_FAULT: child invariant"
                                );
                            } else {
                                assert!(events.iter().any(|e| e["kind"] == "child_joined"));
                            }
                            completed = true;
                            break;
                        }
                    }
                    assert!(completed);
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
