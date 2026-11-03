//! Resource audit for checked source inline boundary handoff schedules.
use super::*;

#[test]
fn checked_boundary_handoffs_release_borrowers_before_inline_resource_owners() {
    native_models(false);
}

#[test]
fn checked_helper_spawned_borrowers_release_before_inline_resource_owners() {
    native_models(true);
}

fn native_models(spawned: bool) {
    let root = std::env::temp_dir().join(format!(
        "tellur-boundary-handoff-resources-{spawned}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let manifest = resume_model::resource_tests::resource_manifest(&root);
    let host = fs::read_to_string(root.join("host.rs")).unwrap();
    fs::write(root.join("host.rs"),host.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let value=Counter { path:path.into(), id };value.record(\"make\");rust_api::make_outcome::made(value)"
    )).unwrap();
    for named in [false, true] {
        for awaited in if spawned {
            vec![true]
        } else {
            vec![false, true]
        } {
            let p = if spawned {
                crate::boundary_handoff_model::spawned_resource_program(manifest.clone(), named)
            } else {
                crate::boundary_handoff_model::resource_program(manifest.clone(), named, awaited)
            }
            .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
            .unwrap();
            for (expanded, p) in [(false, p.clone()), (true, p.expanded("main").unwrap())] {
                let support = include_str!("source_scheduler.rs");
                let entry = "drive_source(host, args, control, || usize::MAX)";
                assert_eq!(support.matches(entry).count(), 1);
                let replacement = "{let values=std::env::var(\"TELLUR_TEST_QUOTAS\").unwrap();let mut quotas=values.split(',').map(|s|s.parse::<usize>().unwrap()).cycle();drive_source(host,args,control,||quotas.next().unwrap())}";
                let generated = p.native_source("main").unwrap();
                assert_eq!(generated.matches(support).count(), 1);
                let generated =
                    generated.replacen(support, &support.replacen(entry, replacement, 1), 1);
                for opt in [0, 2] {
                    let binary = root.join(format!("owner-{named}-{awaited}-{expanded}-{opt}"));
                    let project = p.native_project("main", &binary).unwrap();
                    fs::write(&project.2, &generated).unwrap();
                    p.finish_native_build(project, opt).unwrap();
                    let log = root.join("owners.log");
                    for stop in [false, true] {
                        for fuel in 0..180 {
                            let run = |quotas: &str| {
                                if log.exists() {
                                    fs::remove_file(&log).unwrap();
                                }
                                let out = Command::new(&binary)
                                    .args([
                                        "--json",
                                        "--fuel",
                                        &fuel.to_string(),
                                        "--",
                                        log.to_str().unwrap(),
                                        &stop.to_string(),
                                    ])
                                    .env("TELLUR_TEST_QUOTAS", quotas)
                                    .output()
                                    .unwrap();
                                (out, fs::read_to_string(&log).unwrap_or_default())
                            };
                            let (out, owners) = run("99");
                            for quota in ["1", "0,3,1"] {
                                let (actual, log) = run(quota);
                                assert_eq!(actual.status.code(), out.status.code());
                                assert_eq!(actual.stdout, out.stdout);
                                assert_eq!(actual.stderr, out.stderr);
                                assert_eq!(
                                    log, owners,
                                    "{named}/{awaited}/{expanded}/{opt}/{stop}/{fuel}/{quota}"
                                );
                            }
                            for id in [3, 4, 5] {
                                let count = |kind: &str| {
                                    owners
                                        .lines()
                                        .filter(|s| *s == format!("{kind} {id}"))
                                        .count()
                                };
                                assert!(count("make") <= 1);
                                assert_eq!(
                                    count("make"),
                                    count("drop"),
                                    "{named}/{awaited}/{expanded}/{opt}/{stop}/{fuel}: {owners}"
                                );
                            }
                            if out.status.code() == Some(if stop { 23 } else { 0 }) {
                                assert!(owners.contains("borrow 4"), "{owners}");
                                assert!(
                                    owners.find("drop 5").unwrap() < owners.find("drop 4").unwrap(),
                                    "{owners}"
                                );
                                assert!(
                                    owners.find("drop 4").unwrap()
                                        < owners.find("close 3").unwrap(),
                                    "{owners}"
                                );
                                assert!(owners.ends_with("close 3\ndrop 3\n"), "{owners}");
                                for id in [4, 5] {
                                    assert_eq!(
                                        owners.contains(&format!("close {id}")),
                                        !stop,
                                        "{owners}"
                                    );
                                }
                                let result: serde_json::Value =
                                    serde_json::from_slice(&out.stdout).unwrap();
                                let faults = result["events"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .filter(|e| e["kind"] == "fault")
                                    .count();
                                assert_eq!(faults, usize::from(stop), "{result}");
                                break;
                            }
                            assert!(
                                fuel < 179,
                                "{named}/{awaited}/{expanded}/{opt}/{stop}: {}",
                                String::from_utf8_lossy(&out.stderr)
                            );
                        }
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_boundary_resource_models_retire_reference_storage_at_every_fuel_cut() {
    reference_models(false);
}

#[test]
fn checked_helper_spawned_resource_models_retire_reference_storage_at_every_fuel_cut() {
    reference_models(true);
}

fn reference_models(spawned: bool) {
    use crate::runtime::{Event, RunCompletion, Scenario, ScriptCall};
    let root = std::env::temp_dir().join(format!(
        "tellur-boundary-handoff-reference-resources-{spawned}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let manifest = resume_model::resource_tests::resource_manifest(&root);
    for named in [false, true] {
        for awaited in if spawned {
            vec![true]
        } else {
            vec![false, true]
        } {
            let p = if spawned {
                crate::boundary_handoff_model::spawned_resource_program(manifest.clone(), named)
            } else {
                crate::boundary_handoff_model::resource_program(manifest.clone(), named, awaited)
            };
            for p in [p.clone(), p.expanded("main").unwrap()] {
                for stop in [false, true] {
                    let mut calls: Vec<_> = [3, 4, 5]
                        .into_iter()
                        .map(|id| ScriptCall {
                            operation: "make".into(),
                            outcome: "made".into(),
                            fields: vec![Value::Opaque {
                                ty: "counter".into(),
                                token: id.to_string(),
                            }],
                        })
                        .collect();
                    let effects = if stop {
                        vec!["inspect", "close"]
                    } else {
                        vec!["inspect", "inspect", "close", "close", "close"]
                    };
                    calls.extend(effects.into_iter().map(|name| ScriptCall {
                        operation: name.into(),
                        outcome: "done".into(),
                        fields: vec![],
                    }));
                    let scenario = Scenario {
                        entry: "main".into(),
                        arguments: vec![Value::String("owners.log".into()), Value::Bool(stop)],
                        calls,
                    };
                    let result = crate::runtime::census_model(&p, &scenario);
                    let events = match result {
                        RunCompletion::Process(result) => {
                            assert!(stop);
                            assert_eq!(result.status, 23);
                            result.events
                        }
                        RunCompletion::Outcome(result) => {
                            assert!(!stop);
                            assert_eq!(result.outcome, "done");
                            result.events
                        }
                    };
                    let faults = events
                        .iter()
                        .filter(|e| matches!(e, Event::Fault { .. }))
                        .count();
                    assert_eq!(faults, usize::from(stop));
                    let closed: Vec<_> = events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Primitive {
                                operation,
                                arguments,
                                ..
                            } if operation == "close" => {
                                let Value::Opaque { token, .. } = &arguments[0] else {
                                    panic!("{arguments:?}")
                                };
                                Some(token.as_str())
                            }
                            _ => None,
                        })
                        .collect();
                    assert_eq!(closed, if stop { vec!["3"] } else { vec!["5", "4", "3"] });
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
