use super::*;
use serde_json::Value as Json;

#[test]
fn compiled_owner_await_sibling_faults_restore_lexical_and_named_parent_trackers() {
    let root = std::env::temp_dir().join(format!("tellur-owner-await-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_owner_await.tellur"),
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
        let failing = ids["failing"];
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            let phase=std::env::var("TELLUR_OWNER_PHASE").unwrap().parse::<usize>().unwrap();
            let under_await=std::env::var("TELLUR_AWAITED_OWNER").unwrap()=="true";
            drive_owner_await(host,&MODEL_{entry},&MODEL_{failing},args,phase,under_await,&quotas)
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        assert_eq!(generated.matches(&normal).count(), 1);
        let generated = generated.replacen(&normal, &replacement, 1).replacen(
            CONTEXTS,
            &format!("{CONTEXTS}\n{}", include_str!("owner_await_model.rs")),
            1,
        );
        let mut baseline = BTreeMap::new();
        let mut owner_reports = BTreeMap::new();
        for opt in [0, 2] {
            let binary = root.join(format!("owner-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for under_await in [false, true] {
                for named in [false, true] {
                    for phase in 0..=4 {
                        for quotas in ["99", "1", "0,3,1"] {
                            let output = Command::new(&binary)
                                .args([
                                    "--json",
                                    "--fuel",
                                    "1000",
                                    "--",
                                    &named.to_string(),
                                    &(phase == 2).to_string(),
                                ])
                                .env("TELLUR_OWNER_PHASE", phase.to_string())
                                .env("TELLUR_AWAITED_OWNER", under_await.to_string())
                                .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                                .output()
                                .unwrap();
                            let context = format!(
                                "expanded={is_expanded},opt={opt},awaited_owner={under_await},named={named},phase={phase},quotas={quotas}"
                            );
                            assert!(
                                output.status.success(),
                                "{context}: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                            let observed: Json = serde_json::from_slice(&output.stdout).unwrap();
                            if opt == 0 && quotas == "99" {
                                baseline.insert((under_await, named, phase), observed.clone());
                            } else {
                                assert_eq!(
                                    &observed,
                                    &baseline[&(under_await, named, phase)],
                                    "{context}"
                                );
                            }
                            let events = observed["events"].as_array().unwrap();
                            assert_eq!(events.last().unwrap()["kind"], "owner_settled");
                            let reports: Vec<_> = events
                                .iter()
                                .filter(|e| {
                                    e["kind"] == "fault"
                                        && e["report"]["message"] == "E_FAULT: sibling fault"
                                })
                                .collect();
                            assert_eq!(reports.len(), 1, "{context}");
                            let report = &reports[0]["report"];
                            assert_eq!(report["cause"]["boundary"], "failing");
                            if let Some(expected) = owner_reports.get(&under_await) {
                                assert_eq!(report, expected, "{context}");
                            } else {
                                owner_reports.insert(under_await, report.clone());
                            }
                            let messages: Vec<_> = events
                                .iter()
                                .filter(|e| e["kind"] == "emit")
                                .map(|e| e["message"].as_str().unwrap())
                                .collect();
                            assert!(!messages.contains(&"owner continued"));
                            assert_eq!(
                                messages.iter().filter(|m| **m == "owner handler").count(),
                                1
                            );
                            assert_eq!(
                                messages.iter().filter(|m| **m == "nested handler").count(),
                                usize::from(phase == 2)
                            );
                            assert!(!observed.to_string().contains("nested handler resumed"));
                        }
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
