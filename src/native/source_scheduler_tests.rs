//! Differential source execution using the exact production emitter and scheduler.
use super::*;
#[test]
fn compiled_source_normal_schedule() {
    compare(
        "normal",
        include_str!("../../playground/structured_tasks.tellur"),
    );
}
#[test]
fn compiled_source_task_tree() {
    compare(
        "tree",
        include_str!("../../tests/fixtures/source_task_tree.tellur"),
    );
}
#[test]
fn compiled_source_sibling_fault() {
    compare(
        "fault",
        include_str!("../../tests/fixtures/source_task_fault.tellur"),
    );
}
#[test]
fn compiled_source_grandchild_fault() {
    compare(
        "grandchild_fault",
        include_str!("../../tests/fixtures/source_task_grandchild_fault.tellur"),
    );
}
fn compare(label: &str, text: &str) {
    let root = std::env::temp_dir().join(format!(
        "tellur-source-schedules-{label}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        text,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    for (expanded, program) in [
        (false, original.clone()),
        (true, original.expanded("main").unwrap()),
    ] {
        let generated = program.native_source("main").unwrap();
        let support = include_str!("source_scheduler.rs");
        assert_eq!(generated.matches(support).count(), 1);
        // Replace only the external quota supplier, never generated source
        // callbacks, queue/claim/delivery operations or settlement machinery.
        let entry = "drive_source(host, args, control, || usize::MAX)";
        assert_eq!(support.matches(entry).count(), 1);
        let test_support=support.replacen(entry,
                "{let values=std::env::var(\"TELLUR_TEST_QUOTAS\").unwrap();let mut quotas=values.split(',').map(|s|s.parse::<usize>().unwrap()).cycle();drive_source(host,args,control,||quotas.next().unwrap())}",1);
        let generated = generated.replacen(support, &test_support, 1);
        for opt in [0, 2] {
            let binary = root.join(format!("{label}-{expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for fuel in 0..100 {
                let expected = program.run_completion(
                    &crate::runtime::Scenario {
                        entry: "main".into(),
                        arguments: vec![],
                        calls: vec![],
                    },
                    fuel,
                );
                for quotas in ["99", "1", "0,3,1"] {
                    let out = Command::new(&binary)
                        .args(["--json", "--fuel", &fuel.to_string()])
                        .env("TELLUR_TEST_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    match &expected {
                        Ok(result) => {
                            let status = match result {
                                crate::runtime::RunCompletion::Outcome(_) => 0,
                                crate::runtime::RunCompletion::Process(p) => i32::from(p.status),
                            };
                            assert_eq!(
                                out.status.code(),
                                Some(status),
                                "{label}: {}",
                                String::from_utf8_lossy(&out.stderr)
                            );
                            let observed: serde_json::Value =
                                serde_json::from_slice(&out.stdout).unwrap();
                            assert_eq!(
                                observed,
                                serde_json::to_value(result).unwrap(),
                                "{label} expanded={expanded} opt={opt} fuel={fuel} quotas={quotas}"
                            );
                        }
                        Err(failure) => {
                            assert_eq!(
                                out.status.code(),
                                Some(1),
                                "{label}: {}",
                                String::from_utf8_lossy(&out.stderr)
                            );
                            assert!(out.stdout.is_empty());
                            let mut observed: crate::fault::Report =
                                serde_json::from_slice(&out.stderr).unwrap();
                            fn normalize(
                                actual: &mut crate::fault::Report,
                                expected: &crate::fault::Report,
                            ) {
                                if expected.message.starts_with("E_FUEL") {
                                    assert!(actual.message.starts_with("E_FUEL"));
                                    actual.message = expected.message.clone();
                                }
                                if let (Some(actual), Some(expected)) =
                                    (&mut actual.cause, &expected.cause)
                                {
                                    normalize(actual, expected);
                                }
                            }
                            normalize(&mut observed, &failure.report);
                            assert_eq!(
                                observed, failure.report,
                                "{label} expanded={expanded} opt={opt} fuel={fuel} quotas={quotas}"
                            );
                        }
                    }
                }
                if expected.is_ok() {
                    break;
                }
                assert!(fuel < 99, "source fixture did not finish");
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiled_source_resources_retire_queued_running_and_completed_owners() {
    let root = std::env::temp_dir().join(format!("tellur-source-resource-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = super::resume_model::resource_tests::resource_manifest(&root);
    let host = fs::read_to_string(root.join("host.rs")).unwrap();
    fs::write(root.join("host.rs"),host.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let value=Counter { path:path.into(), id };value.record(\"make\");rust_api::make_outcome::made(value)"
    )).unwrap();
    let source = r#"
async control produce(counter value) exits ready {counter result} {exit ready {value};}
async control sibling(counter value) exits done {
    fault boundary {inspect(borrow value);yield;close(value);}
    contingency [] {escalate;}
    exit done;
}
async control fail(bool stop) exits done faults {
    delta stop {true=>failed,false=>ok}
    [failed]:fault "stop children";
    [ok]:exit done;
}
async control startup(string path,bool stop) exits done faults {
    delta make(borrow path,1) {made {counter first}=>first}
    [first]:delta make(borrow path,2) {made {counter second}=>second}
    [second]:auto result=spawn produce(first);
    auto worker=spawn sibling(second);
    auto failure=spawn fail(stop);
    yield;
    delta await result {ready {counter value}=>ready}
    [ready]:close(value);await worker;await failure;exit done;
}
control main(string path,bool stop) exits done {
    delta make(borrow path,3) {made {counter owner}=>ready}
    [ready]:fault boundary {run startup(path,stop);}
    contingency [owner] {close(owner);terminate process 23;}
    [ready]:close(owner);exit done;
}
"#;
    let p = crate::compile(source, manifest)
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    for (expanded, p) in [(false, p.clone()), (true, p.expanded("main").unwrap())] {
        let support = include_str!("source_scheduler.rs");
        let entry = "drive_source(host, args, control, || usize::MAX)";
        assert_eq!(support.matches(entry).count(), 1);
        let replacement = "{let values=std::env::var(\"TELLUR_TEST_QUOTAS\").unwrap();let mut quotas=values.split(',').map(|s|s.parse::<usize>().unwrap()).cycle();drive_source(host,args,control,||quotas.next().unwrap())}";
        let generated = p.native_source("main").unwrap().replacen(
            support,
            &support.replacen(entry, replacement, 1),
            1,
        );
        for opt in [0, 2] {
            let binary = root.join(format!("resource-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            p.finish_native_build(project, opt).unwrap();
            let log = root.join("owners.log");
            for stop in [false, true] {
                for fuel in 0..150 {
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
                    let (expected, owners) = run("99");
                    for quotas in ["1", "0,3,1"] {
                        let (out, actual) = run(quotas);
                        assert_eq!(
                            out.status.code(),
                            expected.status.code(),
                            "{expanded}/{opt}/{stop}/{fuel}/{quotas}"
                        );
                        assert_eq!(out.stdout, expected.stdout);
                        assert_eq!(out.stderr, expected.stderr);
                        assert_eq!(actual, owners);
                    }
                    for id in [1, 2, 3] {
                        let count = |kind: &str| {
                            owners
                                .lines()
                                .filter(|l| *l == format!("{kind} {id}"))
                                .count()
                        };
                        assert!(count("make") <= 1);
                        assert_eq!(count("make"), count("drop"), "fuel={fuel}: {owners}");
                    }
                    if expected.status.code() == Some(if stop { 23 } else { 0 }) {
                        assert!(owners.ends_with("close 3\ndrop 3\n"), "{owners}");
                        if stop {
                            assert!(
                                owners.contains("borrow 2\ndrop 1\ndrop 2\nclose 3"),
                                "{owners}"
                            );
                            assert!(!owners.contains("close 1") && !owners.contains("close 2"));
                        } else {
                            assert!(owners.contains("close 1") && owners.contains("close 2"));
                        }
                        break;
                    }
                    assert!(fuel < 149, "{}", String::from_utf8_lossy(&expected.stderr));
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
