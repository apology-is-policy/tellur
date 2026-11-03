//! Differential source execution using the exact production emitter and scheduler.
use super::*;
#[test]
fn compiled_source_nested_records_values() {
    compare(
        "nested_values",
        include_str!("../../playground/nested_spawn.tellur"),
    );
}
#[test]
fn compiled_source_nested_records_guards() {
    compare(
        "nested_guards",
        include_str!("../../tests/fixtures/source_nested_guards.tellur"),
    );
}
#[test]
fn compiled_source_nested_records_fault() {
    compare(
        "nested_fault",
        include_str!("../../tests/fixtures/source_nested_fault.tellur"),
    );
}
#[test]
fn compiled_source_nested_records_joined_fault() {
    let source = include_str!("../../tests/fixtures/source_nested_fault.tellur")
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    compare("nested_joined_fault", &source);
}
#[test]
fn compiled_source_nested_records_resources() {
    source_resources(SourceResources::NestedRecords);
}
#[test]
fn compiled_source_flat_result_values() {
    compare(
        "flat_result_values",
        include_str!("../../playground/carrier_spawn_results.tellur"),
    );
}
#[test]
fn compiled_source_flat_result_guards() {
    compare(
        "flat_result_guards",
        include_str!("../../tests/fixtures/source_carrier_result_guards.tellur"),
    );
}
#[test]
fn compiled_source_flat_result_fault() {
    compare(
        "flat_result_fault",
        include_str!("../../tests/fixtures/source_carrier_result_fault.tellur"),
    );
}
#[test]
fn compiled_source_flat_result_joined_fault() {
    let source = include_str!("../../tests/fixtures/source_carrier_result_fault.tellur")
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    compare("flat_result_joined_fault", &source);
}
#[test]
fn compiled_source_flat_result_resources() {
    source_resources(SourceResources::CarrierResult);
}
#[test]
fn compiled_source_flat_carrier_captures() {
    compare(
        "carrier_captures",
        include_str!("../../playground/carrier_spawn.tellur"),
    );
}
#[test]
fn compiled_source_flat_carrier_guards() {
    compare(
        "carrier_guards",
        include_str!("../../tests/fixtures/source_carrier_guards.tellur"),
    );
}
#[test]
fn compiled_source_flat_carrier_fault() {
    compare(
        "carrier_fault",
        include_str!("../../tests/fixtures/source_carrier_fault.tellur"),
    );
}
#[test]
fn compiled_source_flat_carrier_resources() {
    source_resources(SourceResources::CarrierCapture);
}
#[test]
fn compiled_source_direct_borrowed_result_guards() {
    let source = include_str!("../../playground/guarded_task_captures.tellur")
        .replace("defer ", "spawn ")
        .replace(
            "auto moved = move pending;",
            "yield;yield;auto moved = move pending;",
        );
    compare("borrowed_result_guards", &source);
}
#[test]
fn compiled_source_direct_borrowed_results() {
    compare(
        "borrowed_results",
        include_str!("../../playground/borrowed_spawn_results.tellur"),
    );
}
#[test]
fn compiled_source_direct_borrowed_result_fault() {
    compare(
        "borrowed_result_fault",
        include_str!("../../tests/fixtures/source_borrowed_result_fault.tellur"),
    );
}
#[test]
fn compiled_source_direct_borrowed_result_joined_fault() {
    let text = include_str!("../../tests/fixtures/source_borrowed_result_fault.tellur")
        .replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;");
    compare("borrowed_result_joined_fault", &text);
}
#[test]
fn compiled_source_direct_borrowed_result_awaited_owner_fault() {
    let source = include_str!("../../tests/fixtures/source_borrowed_result_fault.tellur")
        .replace(
            "control main()",
            "async control outer() exits done faults {await startup();exit done;} control main()",
        )
        .replace("run startup();", "run outer();");
    compare("borrowed_result_awaited_owner_fault", &source);
}
#[test]
fn compiled_source_direct_borrowed_result_resources() {
    source_resources(SourceResources::BorrowedResult);
}
#[test]
fn compiled_source_borrowed_captures() {
    compare(
        "borrowed_captures",
        include_str!("../../playground/borrowed_spawn.tellur"),
    );
}
#[test]
fn compiled_source_borrowed_capture_fault() {
    compare(
        "borrowed_capture_fault",
        include_str!("../../tests/fixtures/source_borrowed_task_fault.tellur"),
    );
}
#[test]
fn compiled_source_borrowed_capture_resources() {
    source_resources(SourceResources::BorrowedCapture);
}
#[test]
fn compiled_source_cooperative_helpers() {
    compare(
        "cooperative_helpers",
        include_str!("../../playground/cooperative_helpers.tellur"),
    );
}
#[test]
fn compiled_source_inline_helper_yield_fault() {
    inline_helper_fault("yield;yield;", false);
}
#[test]
fn compiled_source_inline_helper_join_fault() {
    inline_helper_fault("auto nested=spawn idle();await nested;", false);
}
#[test]
fn compiled_source_inline_helper_await_fault() {
    inline_helper_fault("await leaf(text);", false);
}
#[test]
fn compiled_source_inline_helper_queued_await_fault() {
    inline_helper_fault("await leaf(text);", true);
}
fn inline_helper_fault(body: &str, early: bool) {
    let source = include_str!("../../tests/fixtures/source_inline_helper_fault.tellur");
    let source = source.replace(
        "emit(\"inline started\");\n    yield;\n    yield;",
        &format!("emit(\"inline started\");{body}"),
    );
    let source = if early {
        source.replacen("yield;", "", 1)
    } else {
        source
    };
    compare(
        &format!(
            "inline-helper-{}-{early}",
            if body.starts_with("yield") {
                "yield"
            } else if body.starts_with("auto") {
                "join"
            } else {
                "await"
            }
        ),
        &source,
    );
}
#[test]
fn compiled_source_inline_helper_resource_retirement() {
    source_resources(SourceResources::InlineHelper);
}
#[test]
fn compiled_source_parent_await() {
    compare(
        "parent_await",
        include_str!("../../playground/parent_await.tellur"),
    );
}
#[test]
fn compiled_source_parent_await_fault() {
    compare(
        "parent_await_fault",
        include_str!("../../tests/fixtures/source_parent_await_fault.tellur"),
    );
}
#[test]
fn compiled_source_queued_parent_await_fault() {
    let source = include_str!("../../tests/fixtures/source_parent_await_fault.tellur").replacen(
        "yield;\n    yield;",
        "",
        1,
    );
    compare("queued_parent_await_fault", &source);
}
#[test]
fn compiled_source_cold_parent_await_fault() {
    let source = include_str!("../../tests/fixtures/source_parent_await_fault.tellur").replace(
        "await nested(borrow mutable text);",
        "auto cold=defer nested(borrow mutable text);await cold;",
    );
    compare("cold_parent_await_fault", &source);
}
#[test]
fn compiled_source_uninterrupted_helpers() {
    compare(
        "helpers",
        include_str!("../../playground/structured_helpers.tellur"),
    );
}
#[test]
fn compiled_source_helper_fault() {
    compare(
        "helper_fault",
        include_str!("../../tests/fixtures/source_helper_fault.tellur"),
    );
}
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
                let scenario = crate::runtime::Scenario {
                    entry: "main".into(),
                    arguments: vec![],
                    calls: vec![],
                };
                let mut session = program.start_execution(&scenario, fuel);
                let mut polls = 0;
                let expected = loop {
                    polls += 1;
                    assert!(polls < 1000, "{label}: fuel={fuel}, expanded={expanded}");
                    match session.advance(99) {
                        crate::runtime::ExecutionProgress::Paused => {}
                        crate::runtime::ExecutionProgress::Complete(result) => break result,
                        other => panic!("unexpected source progress: {other:?}"),
                    }
                };
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
                                    assert!(actual.message.starts_with("E_FUEL"), "{actual:?}");
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
    source_resources(SourceResources::Normal);
}
#[test]
fn compiled_source_helper_fault_retires_suspended_and_completed_owners() {
    source_resources(SourceResources::HelperFault);
}
#[test]
fn compiled_source_parent_await_retires_borrowers_before_owner_contingency() {
    source_resources(SourceResources::ParentAwait);
}
#[derive(Clone, Copy, Debug)]
enum SourceResources {
    Normal,
    HelperFault,
    ParentAwait,
    InlineHelper,
    BorrowedCapture,
    CarrierCapture,
    CarrierResult,
    NestedRecords,
    BorrowedResult,
}
fn source_resources(mode: SourceResources) {
    let helper_fault = matches!(mode, SourceResources::HelperFault);
    let parent_await = matches!(
        mode,
        SourceResources::ParentAwait
            | SourceResources::InlineHelper
            | SourceResources::BorrowedCapture
            | SourceResources::CarrierCapture
            | SourceResources::CarrierResult
            | SourceResources::NestedRecords
            | SourceResources::BorrowedResult
    );
    let inline_helper = matches!(mode, SourceResources::InlineHelper);
    let borrowed_spawn = matches!(
        mode,
        SourceResources::BorrowedCapture
            | SourceResources::CarrierCapture
            | SourceResources::NestedRecords
    );
    let root = std::env::temp_dir().join(format!(
        "tellur-source-resource-{mode:?}-{}",
        std::process::id()
    ));
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
    let source = if parent_await {
        let source = source
            .replace(
                "inspect(borrow value);yield;close(value);",
                "inspect(borrow value);yield;yield;yield;yield;close(value);",
            )
            .replace("[failed]:fault", "[failed]:yield;yield;fault")
            .replace(
                "startup(string path,bool stop)",
                "startup(string path,bool stop,ref counter parent)",
            )
            .replace(
                "run startup(path,stop)",
                "run startup(path,stop,borrow owner)",
            )
            .replace(
                "yield;\n    delta await result",
                "await observe(parent,path);\n    delta await result",
            );
        format!(
            r#"{source}
async control observe(ref counter parent,string path) exits done {{
    delta make(borrow path,4) {{made {{counter local}}=>made}}
    [made]:inspect(parent);fault boundary {{yield;yield;yield;}}
    contingency [] {{emit("must not handle stopped observer");escalate;}}
    [made]:close(local);exit done;
}}
"#
        )
    } else if helper_fault {
        let source = source
            .replace("spawn fail(stop)", "spawn fail(false)")
            .replace(
                "yield;\n    delta await result",
                "yield; fail_helper(stop);\n    delta await result",
            );
        format!(
            "{source}\ncontrol fail_helper(bool stop) exits done faults {{\n    delta stop {{true=>failed,false=>ok}}\n    [failed]:fault \"helper failed\";\n    [ok]:exit done;\n}}"
        )
    } else {
        source.to_owned()
    };
    let source = if inline_helper {
        let source = source
            .replace("await observe(parent,path);", "helper(parent,path);")
            .replace("make(borrow path,4)", "make(borrow path,5)");
        format!(
            r#"{source}
control helper(ref counter parent,string path) exits done {{
    inspect(parent);
    delta make(borrow path,4) {{made {{counter local}}=>made}}
    [made]:run helper_work(borrow local,path);close(local);exit done;
}}
async control helper_work(ref counter local,string path) exits done {{
    await observe(local,path);exit done;
}}
"#
        )
    } else {
        source
    };
    let source = if borrowed_spawn {
        source
            .replace(
                "async control sibling(counter value)",
                "async control sibling(ref counter parent,counter value)",
            )
            .replace(
                "fault boundary {inspect(borrow value);",
                "inspect(parent);fault boundary {inspect(borrow value);",
            )
            .replace("spawn sibling(second)", "spawn sibling(parent,second)")
    } else {
        source
    };
    let source = if matches!(
        mode,
        SourceResources::CarrierCapture | SourceResources::NestedRecords
    ) {
        let (before, after) = source.split_once("async control fail").unwrap();
        let before = before
            .replace(
                "sibling(ref counter parent,counter value)",
                "sibling(worker_input input)",
            )
            .replace(
                "inspect(parent);",
                "inspect(input.parent);counter value=move input.value;",
            );
        format!("struct worker_input['parent] {{ref counter parent <- 'parent;counter value;}}\n{before}async control fail{after}")
            .replace("spawn sibling(parent,second)", "spawn sibling(worker_input {parent,second})")
    } else {
        source
    };
    let source = if matches!(mode, SourceResources::BorrowedResult) {
        source.replace("produce(counter value) exits ready {counter result} {exit ready {value};}",
            "produce(counter value,ref counter parent) exits ready {counter result,ref counter retained from parent} {exit ready {value,parent};}")
            .replace("spawn produce(first)", "spawn produce(first,parent)")
            .replace("ready {counter value}=>ready", "ready {counter value,auto retained}=>ready")
            .replace("close(value);await worker;await failure;exit done;", "close(value);await worker;await failure;inspect(retained);exit done;")
    } else {
        source
    };
    let source = if matches!(
        mode,
        SourceResources::CarrierResult | SourceResources::NestedRecords
    ) {
        let source=source.replace("produce(counter value) exits ready {counter result} {exit ready {value};}",
            "produce(counter value,ref counter parent) exits ready {produced[parent] result} {exit ready {produced {value,parent}};}")
            .replace("spawn produce(first)", "spawn produce(first,parent)")
            .replace("ready {counter value}=>ready", "ready {auto value}=>ready")
            .replace("close(value);await worker;await failure;exit done;", "close(value.owned);await worker;await failure;inspect(value.retained);exit done;");
        format!(
            "struct produced['parent] {{counter owned;ref counter retained <- 'parent;}}\n{source}"
        )
    } else {
        source
    };
    let source = if matches!(mode, SourceResources::NestedRecords) {
        source.replace("struct worker_input['parent] {ref counter parent <- 'parent;counter value;}",
            "struct worker_leaf['parent] {ref counter parent <- 'parent;counter value;}struct worker_input['parent] {worker_leaf['parent] nested;}")
            .replace("input.parent", "input.nested.parent")
            .replace("input.value", "input.nested.value")
            .replace("worker_input {parent,second}", "worker_input {worker_leaf {parent,second}}")
            .replace("struct produced['parent] {counter owned;ref counter retained <- 'parent;}",
                "struct result_leaf['parent] {counter owned;ref counter retained <- 'parent;}struct produced['parent] {result_leaf['parent] nested;}")
            .replace("produced {value,parent}", "produced {result_leaf {value,parent}}")
            .replace("value.owned", "value.nested.owned")
            .replace("value.retained", "value.nested.retained")
    } else {
        source
    };
    let p = crate::compile(&source, manifest)
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
                    for id in 1..=if inline_helper {
                        5
                    } else if parent_await {
                        4
                    } else {
                        3
                    } {
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
                        if stop && parent_await {
                            assert!(
                                !String::from_utf8_lossy(&expected.stdout)
                                    .contains("must not handle stopped observer")
                            );
                            assert!(owners.contains("borrow 3"), "{owners}");
                            if inline_helper {
                                assert!(owners.contains("borrow 4"), "{owners}");
                                assert!(
                                    owners.find("drop 5").unwrap() < owners.find("drop 4").unwrap(),
                                    "{owners}"
                                );
                                assert!(!owners.contains("close 5"), "{owners}");
                            }
                            for id in [1, 2, 4] {
                                assert!(
                                    owners.find(&format!("drop {id}")).unwrap()
                                        < owners.find("close 3").unwrap(),
                                    "{owners}"
                                );
                                assert!(!owners.contains(&format!("close {id}")), "{owners}");
                            }
                        } else if stop {
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

#[test]
fn compiled_source_completes_unsettleable_capture_before_handoff() {
    let root =
        std::env::temp_dir().join(format!("tellur-source-stop-proof-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut manifest = super::resume_model::resource_tests::resource_manifest(&root);
    for ty in &mut manifest.types {
        if let crate::manifest::TypeDef::Opaque { name, fault, .. } = ty {
            if name == "counter" {
                *fault = None;
            }
        }
    }
    let host = fs::read_to_string(root.join("host.rs")).unwrap();
    fs::write(root.join("host.rs"), host.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let value=Counter { path:path.into(), id };value.record(\"make\");rust_api::make_outcome::made(value)"
    )).unwrap();
    let source = r#"
control finish(counter value) exits done {close(value);exit done;}
async control worker(counter value) exits done {finish(value);yield;exit done;}
async control startup(string path) exits done {
    delta make(borrow path,1) {made {counter value}=>made}
    [made]:auto child=spawn worker(value);await child;exit done;
}
control main(string path) exits done {forward run startup(path);}
"#;
    let rejected = source.replace("finish(value);yield;", "yield;finish(value);");
    let error = crate::compile(&rejected, manifest.clone()).unwrap_err();
    assert_eq!(error.code, "E_ASYNC");
    assert!(error.message.contains("fault dispositions"));
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
            let binary = root.join(format!("stop-proof-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            p.finish_native_build(project, opt).unwrap();
            let log = root.join("owners.log");
            for fuel in 0..100 {
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
                        ])
                        .env("TELLUR_TEST_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    (out, fs::read_to_string(&log).unwrap_or_default())
                };
                let (expected, owners) = run("99");
                for quotas in ["1", "0,3,1"] {
                    let (out, actual) = run(quotas);
                    assert_eq!(out.status.code(), expected.status.code());
                    assert_eq!(out.stdout, expected.stdout);
                    assert_eq!(out.stderr, expected.stderr);
                    assert_eq!(actual, owners, "{expanded}/{opt}/{fuel}/{quotas}");
                }
                let count =
                    |kind: &str| owners.lines().filter(|l| *l == format!("{kind} 1")).count();
                assert!(count("make") <= 1);
                assert_eq!(count("make"), count("drop"), "{owners}");
                if expected.status.success() {
                    assert_eq!(owners, "make 1\nclose 1\ndrop 1\n");
                    break;
                }
                assert!(fuel < 99, "{}", String::from_utf8_lossy(&expected.stderr));
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
