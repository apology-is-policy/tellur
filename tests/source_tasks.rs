use tellur::{
    compile,
    manifest::Manifest,
    runtime::{Event, ExecutionProgress, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
const NORMAL: &str = include_str!("../playground/structured_tasks.tellur");
#[test]
fn source_handoffs_preserve_schedule_across_quotas_and_expansion() {
    let p = compile(NORMAL, manifest()).unwrap();
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 2, 7, usize::MAX] {
            let scenario = scenario();
            let mut session = p.start_execution(&scenario, 1000);
            let mut polls = 0;
            let result = loop {
                polls += 1;
                assert!(polls < 1000);
                match session.advance(quota) {
                    ExecutionProgress::Complete(r) => break r.unwrap(),
                    ExecutionProgress::Paused => {}
                    _ => panic!(),
                }
            };
            let tellur::runtime::RunCompletion::Outcome(result) = result else {
                panic!()
            };
            let messages: Vec<_> = result
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Emit { message } => Some(message.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                messages,
                [
                    "queued",
                    "first",
                    "second",
                    "parent",
                    "first",
                    "second",
                    "joined first",
                    "joined second"
                ]
            );
        }
    }
}
#[test]
fn native_source_handoffs_match_reference() {
    use std::{collections::BTreeMap, fs, process::Command};
    let root = std::env::temp_dir().join(format!("tellur-source-tasks-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    let p = compile(NORMAL, manifest())
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let binary = root.join("program");
    p.build_native("main", &binary, 0).unwrap();
    let out = Command::new(&binary)
        .arg("--scenario")
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::to_value(p.run(&scenario()).unwrap()).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}
const FAULT: &str = include_str!("../tests/fixtures/source_task_fault.tellur");
#[test]
fn child_fault_stops_siblings_before_only_the_owning_contingency() {
    let p = compile(FAULT, manifest()).unwrap();
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let s = scenario();
            let mut session = p.start_execution(&s, 1000);
            let result = loop {
                match session.advance(quota) {
                    ExecutionProgress::Complete(r) => break r.unwrap(),
                    ExecutionProgress::Paused => {}
                    _ => panic!(),
                }
            };
            let tellur::runtime::RunCompletion::Process(result) = result else {
                panic!()
            };
            assert_eq!(result.status, 23);
            let messages: Vec<_> = result
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Emit { message } => Some(message.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(messages, ["queued", "sibling", "owner contingency"]);
        }
    }
}
#[test]
fn native_child_fault_matches_reference() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("tellur-source-fault-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    let p = compile(FAULT, manifest())
        .unwrap()
        .with_entry_exit_codes(
            "main",
            std::collections::BTreeMap::from([("done".into(), 0)]),
        )
        .unwrap();
    let binary = root.join("program");
    p.build_native("main", &binary, 0).unwrap();
    let out = Command::new(&binary)
        .arg("--scenario")
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = serde_json::to_value(p.run_completion(&scenario(), 1000).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
        expected
    );
    fs::remove_dir_all(root).unwrap();
}
const MIXED: &str = include_str!("../tests/fixtures/source_task_tree.tellur");
#[test]
fn owned_tasks_compose_with_borrowed_cold_awaits_and_invocation_owned_grandchildren() {
    let p = compile(MIXED, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let messages: Vec<_> = expected
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        messages,
        [
            "peer",
            "leaf",
            "peer",
            "changed",
            "grandchild",
            "grandchild"
        ]
    );
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        expected
    );
}
#[test]
fn started_handles_require_consumption_and_do_not_erase_the_admitted_profile() {
    let prefix =
        r#"async control work() exits done {exit done;} control sync() exits done {exit done;}"#;
    for body in [
        "auto child=spawn work();exit done;",
        "auto child=spawn work();drop(child);exit done;",
        "auto child=spawn work();auto copy=clone(borrow child);await child;exit done;",
        "auto child=spawn work();await child;await child;exit done;",
        "auto child=spawn work();delta true {true=>yes,false=>no} [yes]:await child;exit done;[no]:exit done;",
        "auto child=spawn work();fault boundary {await child;} contingency [] {escalate;}exit done;",
    ] {
        let text = format!("{prefix} async control startup() exits done faults {{{body}}}");
        assert!(compile(&text, manifest()).is_err(), "{body}");
    }
    for text in [
        "control main() exits done {yield;exit done;}",
        "async control work() exits done {terminate;} async control parent() exits done {auto child=spawn work();await child;exit done;}",
        "async control work() {terminate process 23;} async control parent() exits done {auto child=spawn work();await child;exit done;}",
        "async control work() exits done {exit done;} control main() exits done {auto child=spawn work();await child;exit done;}",
        "async control work() exits done {exit done;} async control factory() exits auto {auto child=spawn work();exit ready {child};}",
    ] {
        assert!(compile(text, manifest()).is_err(), "{text}");
    }
    let no_permission = "async control fail() faults {fault \"failure\";} pub async control parent() exits done {auto child=spawn fail();await child;}";
    assert_eq!(
        compile(no_permission, manifest()).unwrap_err().code,
        "E_FAULT_CONTRACT"
    );
}

#[test]
fn parent_await_preserves_borrowed_storage_and_unjoined_completions() {
    let p = compile(
        include_str!("../playground/parent_await.tellur"),
        manifest(),
    )
    .unwrap();
    let expected = p.run(&scenario()).unwrap();
    let messages: Vec<_> = expected
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        messages,
        [
            "worker started",
            "editor started",
            "worker done",
            "edited",
            "editor started"
        ]
    );
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let s = scenario();
            let mut session = p.start_execution(&s, 1000);
            for polls in 0..1000 {
                assert!(matches!(session.advance(0), ExecutionProgress::Paused));
                if let ExecutionProgress::Complete(r) = session.advance(quota) {
                    let tellur::runtime::RunCompletion::Outcome(result) = r.unwrap() else {
                        panic!()
                    };
                    assert_eq!(result, expected);
                    break;
                }
                assert!(polls < 999);
            }
        }
    }
}

#[test]
fn sibling_fault_restores_queued_and_nested_parent_awaits() {
    let source = include_str!("fixtures/source_parent_await_fault.tellur");
    for delay in 0..3 {
        for cold in [false, true] {
            let mut source = source.replacen("yield;\n    yield;", &"yield;".repeat(delay), 1);
            if cold {
                source = source.replace(
                    "await nested(borrow mutable text);",
                    "auto cold=defer nested(borrow mutable text);await cold;",
                );
            }
            let p = compile(&source, manifest()).unwrap();
            for p in [p.clone(), p.expanded("main").unwrap()] {
                let tellur::runtime::RunCompletion::Process(result) =
                    p.run_completion(&scenario(), 1000).unwrap()
                else {
                    panic!()
                };
                assert_eq!(result.status, 23);
                let messages: Vec<_> = result
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Emit { message } => Some(message.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    messages,
                    if delay == 2 {
                        vec!["borrower started", "owner contingency"]
                    } else {
                        vec!["owner contingency"]
                    }
                );
                let report = result
                    .events
                    .iter()
                    .find_map(|e| match e {
                        Event::Fault { report } => Some(report),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    report
                        .cause
                        .as_ref()
                        .unwrap()
                        .activations
                        .last()
                        .unwrap()
                        .control,
                    "fail"
                );
                assert!(
                    !report
                        .activations
                        .iter()
                        .any(|a| ["nested", "leaf"].contains(&a.control.as_str())),
                    "{report:?}"
                );
                assert!(
                    report.activations.iter().any(|a| a.control == "startup"),
                    "{report:?}"
                );
            }
        }
    }
}
#[test]
fn pending_results_and_yield_keep_fault_disposition_obligations() {
    let prefix = r#"async control fail() faults {fault "failure";}
async control produce() exits auto {forward open("path");}
"#;
    let text = format!(
        r#"{prefix} async control parent() faults {{
        auto result=spawn produce(); auto failure=spawn fail();
        auto saved=capture await result; await failure;
    }}"#
    );
    let error = compile(&text, manifest()).unwrap_err();
    assert_eq!(error.code, "E_FAULT_RESOURCE");
    assert!(error.message.contains("`result`"), "{error:?}");
    let text = format!(
        r#"{prefix} async control parent() faults {{
        auto failure=spawn fail();
        delta open("path") {{opened {{handle h}}=>ready,not_found=>missing,denied=>denied}}
        [ready]:yield; delta close(h) {{closed=>closed,failed {{close_error error}}=>failed}}
        [closed|failed|missing|denied]:await failure;
    }}"#
    );
    let error = compile(&text, manifest()).unwrap_err();
    assert_eq!(error.code, "E_FAULT_RESOURCE");
    assert!(error.message.contains("`h`"), "{error:?}");
}

#[test]
fn wrapped_started_handles_keep_domain_and_handoff_fault_checks() {
    let prefix = r#"
async control work() exits done {yield;exit done;}
async control fail() faults {yield;fault "failed child";}
control helper() exits done {emit("helper");exit done;}
control handoff() exits done {forward run work();}
"#;
    let suffix = r#"
    delta wrapped {some {auto inner}=>inner,none=>empty}
    [inner]:delta inner {some {auto child}=>ready,none=>empty_inner}
    [ready]:await child;
    [ready|empty|empty_inner]:helper();exit done;
"#;
    let body =
        format!("auto pending=spawn work();auto wrapped=some {{some {{pending}}}};yield;{suffix}");
    let source = format!(
        "{prefix} async control startup() exits done {{{body}}} control main() exits done {{forward run startup();}}"
    );
    let p = compile(&source, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        expected
    );
    assert!(matches!(expected.events.last(),Some(Event::Emit{message}) if message=="helper"));
    for statement in ["fault boundary {} contingency [] {escalate;}"] {
        let text = format!(
            "{prefix} async control startup() exits done faults {{auto pending=spawn work();auto wrapped=some {{some {{pending}}}};{statement}{suffix}}}"
        );
        let error = compile(&text, manifest()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{error:?}");
    }
    for handoff in [
        "yield;",
        "await other;",
        "handoff();",
        "await work();",
        "auto cold=defer work();await cold;",
    ] {
        let text = format!(
            r#"{prefix} async control startup() faults {{
            auto pending=spawn fail();auto wrapped=some {{pending}};auto other=spawn work();
            delta open("path") {{opened {{handle h}}=>ready,not_found=>missing,denied=>denied}}
            [ready]:{handoff} delta close(h) {{closed=>closed,failed {{close_error e}}=>failed}}
            [closed|failed|missing|denied]:delta wrapped {{some {{auto child}}=>child,none=>none}}
            [child]:await child;
            [none]:fault "none";
        }}"#
        );
        let error = compile(&text, manifest()).unwrap_err();
        assert_eq!(error.code, "E_FAULT_RESOURCE", "{error:?}");
        assert!(error.message.contains("`h`"));
    }
}

const HELPERS: &str = include_str!("../playground/structured_helpers.tellur");
#[test]
fn synchronous_helpers_preserve_parent_borrows_and_explicit_handoffs() {
    let p = compile(HELPERS, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let messages: Vec<_> = expected
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        messages,
        [
            "queued",
            "constructed",
            "updated",
            "updated",
            "child started",
            "updated",
            "child finished",
            "updated"
        ]
    );
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let s = scenario();
            let mut session = p.start_execution(&s, 1000);
            loop {
                assert!(matches!(session.advance(0), ExecutionProgress::Paused));
                match session.advance(quota) {
                    ExecutionProgress::Complete(r) => {
                        assert_eq!(
                            serde_json::to_value(r.unwrap()).unwrap(),
                            serde_json::to_value(&expected).unwrap()
                        );
                        break;
                    }
                    ExecutionProgress::Paused => {}
                    _ => panic!(),
                }
            }
        }
    }
}
#[test]
fn transitive_inline_boundaries_allow_checked_helper_handoffs() {
    let prefix = "async control worker() exits done {fault boundary {} contingency [] {escalate;} yield;exit done;}";
    for (helper, call) in [
        (
            "control inner() exits done {run worker();exit done;} control helper() exits done {inner();exit done;}",
            "helper();",
        ),
        (
            "control helper<t>() exits done {run worker();exit done;}",
            "helper<u32>();",
        ),
        (
            "control helper() exits done {delta false {true=>handoff,false=>normal} [handoff]:run worker(); [handoff|normal]:exit done;}",
            "helper();",
        ),
        (
            "struct box {string text;constructor() {run worker();self.text=\"value\";}}",
            "auto value=box();drop(value);",
        ),
        (
            "control helper() exits ready {string text} {run worker();exit ready {\"value\"};}",
            "auto saved=capture helper();delta saved {ready {string text}=>ready} [ready]:drop(text);",
        ),
    ] {
        let source = format!(
            "{prefix} {helper} async control startup() exits done {{auto child=spawn worker();{call}await child;exit done;}}"
        );
        compile(&source, manifest()).unwrap_or_else(|e| panic!("{source}\n{e}"));
    }
}
#[test]
fn resolved_imports_and_trait_calls_use_checked_helper_bodies() {
    use std::fs;
    let root = std::env::temp_dir().join(format!("tellur-helper-imports-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for mode in 0..3 {
        let body = match mode {
            0 => "emit(\"helper\");",
            1 => "run worker();",
            _ => "fault boundary {} contingency [] {escalate;}",
        };
        let faults = if mode == 2 { " faults" } else { "" };
        fs::write(root.join("helpers.tellur"), format!("async control worker() exits done {{yield;exit done;}} pub control helper() exits done{faults} {{{body}exit done;}}")).unwrap();
        fs::write(root.join("main.tellur"), "import \"helpers.tellur\" as helpers; async control worker() exits done {exit done;} async control startup() exits done {auto child=spawn worker();helpers.helper();await child;exit done;} control main() exits done {forward run startup();}").unwrap();
        let result = tellur::compile_file(root.join("main.tellur"), manifest());
        {
            assert_eq!(
                result.unwrap().run(&scenario()).unwrap().events.len(),
                usize::from(mode == 0)
            );
        }
        let source = format!(
            "trait helper {{control call() exits done{faults};}} implement helper for u32 {{control call() exits done{faults} {{{body}exit done;}}}} async control worker() exits done {{yield;exit done;}} async control startup() exits done {{auto child=spawn worker();helper<u32>.call();await child;exit done;}} control main() exits done {{forward run startup();}}"
        );
        let result = compile(&source, manifest());
        {
            assert_eq!(
                result.unwrap().run(&scenario()).unwrap().events.len(),
                usize::from(mode == 0)
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn helper_faults_keep_unjoined_result_dispositions_and_stop_queued_children() {
    let source = "async control produce() exits auto {forward open(\"path\");} control fail() faults {fault \"failed\";} async control parent() faults {auto child=spawn produce();fail();}";
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, "E_FAULT_RESOURCE", "{error:?}");
    assert!(error.message.contains("`child`"));
    let p = compile(
        include_str!("fixtures/source_helper_fault.tellur"),
        manifest(),
    )
    .unwrap();
    for p in [p.clone(), p.expanded("main").unwrap()] {
        let tellur::runtime::RunCompletion::Process(r) =
            p.run_completion(&scenario(), 1000).unwrap()
        else {
            panic!()
        };
        assert_eq!(r.status, 23);
        let messages: Vec<_> = r
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Emit { message } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(messages, ["helper", "owner contingency"]);
    }
}

fn stop_manifest() -> Manifest {
    let mut m = manifest();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "close")
        .unwrap()
        .contingent = true;
    m
}
const CLOSE_HANDLE: &str = r#"
@contingent control finish(handle h) exits done {
    delta close(h) {closed=>closed,failed {close_error e}=>failed}
    [closed|failed]:exit done;
}
async control pause() exits done {yield;exit done;}
"#;
fn opened_body(body: &str) -> String {
    format!(
        r#"delta open("path") {{opened {{handle h}}=>opened,not_found=>missing,denied=>denied}}
    [opened]:{body}
    [opened|missing|denied]:exit done;"#
    )
}
fn spawn_producer(extra: &str, body: &str, mode: &str) -> String {
    format!(
        "{CLOSE_HANDLE} {extra} async control producer() exits done faults {{{body}}} async control startup() exits done faults {{auto child={mode} producer();await child;exit done;}}"
    )
}
#[test]
fn spawn_rejects_unsettleable_owners_in_suspended_producers_and_callees() {
    for (extra, body) in [
        ("", "yield;finish(h);"),
        (
            "struct bundle {handle owned;string tag;}",
            "auto value=bundle {h,\"tag\"};yield;finish(value.owned);",
        ),
        (
            "boundary request() {run pause();terminate;} contingency(string reason) {terminate;}",
            "supervise request();finish(h);",
        ),
        (
            "boundary request(handle h) retains h {run pause();finish(h);terminate;} contingency(string reason) {finish(h);terminate;}",
            "supervise request(h);",
        ),
        (
            "",
            "fault boundary {yield;finish(h);} contingency [h] {finish(h);escalate;}",
        ),
        ("", "await pause();finish(h);"),
        ("", "auto pending=defer pause();await pending;finish(h);"),
        (
            "async control complete_first(handle h) exits done {finish(h);yield;exit done;}",
            "await complete_first(h);",
        ),
        (
            "async control complete_first(handle h) exits done {finish(h);yield;exit done;}",
            "auto pending=defer complete_first(h);await pending;",
        ),
        (
            "control helper() exits done {run pause();exit done;}",
            "helper();finish(h);",
        ),
    ] {
        let body = opened_body(body);
        // The same sequential cold producer remains legal: no enclosing spawned
        // owner can abandon its protected storage during a sibling fault.
        compile(&spawn_producer(extra, &body, "defer"), stop_manifest())
            .unwrap_or_else(|e| panic!("{extra}\n{body}\n{e}"));
        let error = compile(&spawn_producer(extra, &body, "spawn"), stop_manifest()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{extra}\n{body}\n{error}");
        assert!(error.message.contains("fault dispositions"), "{error}");
    }
}
#[test]
fn spawn_stop_proof_follows_field_moves_and_transferred_callee_owners() {
    for (extra, body) in [
        ("", "finish(h);yield;"),
        (
            "struct bundle {handle owned;string tag;}",
            "auto value=bundle {h,\"tag\"};finish(value.owned);yield;",
        ),
        (
            "control helper(handle h) exits done {finish(h);run pause();exit done;}",
            "helper(h);",
        ),
        (
            "boundary request() {terminate;} contingency(string reason) {terminate;} control helper() exits done {supervise request();exit done;}",
            "helper();finish(h);yield;",
        ),
    ] {
        compile(
            &spawn_producer(extra, &opened_body(body), "spawn"),
            stop_manifest(),
        )
        .unwrap();
    }
}
#[test]
fn stop_proof_reaches_awaited_producers_and_generic_helpers() {
    for body in [
        "forward await producer();",
        "auto cold=defer producer();await cold;exit done;",
        "helper<u32>();exit done;",
    ] {
        let producer = opened_body("yield;finish(h);");
        let source = format!(
            "{CLOSE_HANDLE} async control producer() exits done {{{producer}}} control helper<t>() exits done {{run producer();exit done;}} async control wrapper() exits done {{{body}}} async control startup() exits done {{auto child=spawn wrapper();await child;exit done;}}"
        );
        let error = compile(&source, stop_manifest()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{error}");
        assert!(error.message.contains("fault dispositions"), "{error}");
    }
}

#[test]
fn parent_await_checks_stopped_producer_obligations_and_keeps_sequential_repair() {
    for awaited in [
        "await producer();",
        "auto cold=defer producer();await cold;",
    ] {
        let body = opened_body("yield;finish(h);");
        let source = format!(
            "{CLOSE_HANDLE} async control producer() exits done {{{body}}} async control startup() exits done {{auto child=spawn pause();{awaited}await child;exit done;}}"
        );
        let error = compile(&source, stop_manifest()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{error}");
        assert!(error.message.contains("fault dispositions"), "{error}");
        let repaired = source.replace(
            &format!("{awaited}await child;"),
            &format!("await child;{awaited}"),
        );
        compile(&repaired, stop_manifest()).unwrap();
    }
}

#[test]
fn cooperative_helpers_preserve_inline_entry_borrows_and_results() {
    let source = include_str!("../playground/cooperative_helpers.tellur");
    let p = compile(source, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let messages: Vec<_> = expected
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        messages,
        [
            "helper started",
            "inline started",
            "worker started",
            "helper returned",
            "edited",
            "worker finished"
        ]
    );
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let s = scenario();
            let mut session = p.start_execution(&s, 1000);
            loop {
                assert!(matches!(session.advance(0), ExecutionProgress::Paused));
                if let ExecutionProgress::Complete(result) = session.advance(quota) {
                    let tellur::runtime::RunCompletion::Outcome(result) = result.unwrap() else {
                        panic!()
                    };
                    assert_eq!(result, expected);
                    break;
                }
            }
        }
    }
}

#[test]
fn sibling_fault_finds_owners_below_inline_helpers_in_every_retained_location() {
    let source = include_str!("fixtures/source_inline_helper_fault.tellur");
    for body in [
        "yield;yield;",
        "auto nested=spawn idle();await nested;",
        "await leaf(text);",
        "auto cold=defer leaf(text);await cold;",
    ] {
        for early in [false, true] {
            let source = source.replace(
                "emit(\"inline started\");\n    yield;\n    yield;",
                &format!("emit(\"inline started\");{body}"),
            );
            let source = if early {
                source.replacen("yield;", "", 1)
            } else {
                source
            };
            let p = compile(&source, manifest()).unwrap();
            for p in [p.clone(), p.expanded("main").unwrap()] {
                let tellur::runtime::RunCompletion::Process(result) =
                    p.run_completion(&scenario(), 1000).unwrap()
                else {
                    panic!()
                };
                assert_eq!(result.status, 23);
                let messages: Vec<_> = result
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Emit { message } => Some(message.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(messages.last(), Some(&"owner contingency"));
                assert!(
                    !messages.iter().any(|m| m.contains("must not")),
                    "{messages:?}"
                );
                let report = result
                    .events
                    .iter()
                    .find_map(|e| match e {
                        Event::Fault { report } => Some(report),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    report
                        .cause
                        .as_ref()
                        .unwrap()
                        .activations
                        .last()
                        .unwrap()
                        .control,
                    "fail"
                );
                assert!(
                    !report.activations.iter().any(|a| a.control == "leaf"),
                    "{report:?}"
                );
                for fuel in 0..100 {
                    let s = scenario();
                    let mut session = p.start_execution(&s, fuel);
                    let result = loop {
                        if let ExecutionProgress::Complete(result) = session.advance(3) {
                            break result;
                        }
                    };
                    if result.is_ok() {
                        break;
                    }
                    assert!(
                        result.unwrap_err().report.message.starts_with("E_FUEL"),
                        "{body}/{early}/{fuel}"
                    );
                    assert!(fuel < 99);
                }
            }
        }
    }
}

#[test]
fn cooperative_helpers_check_stop_obligations_after_transferred_inputs() {
    let safe = "async control finish(handle h) exits done {delta close(h){closed=>closed,failed {close_error e}=>failed} [closed|failed]:yield;exit done;} control helper(handle h) exits done {forward run finish(h);}";
    let caller = "async control worker() exits done {yield;exit done;} async control startup(handle h) exits done {auto child=spawn worker();helper(h);await child;exit done;}";
    compile(&format!("{safe}{caller}"), manifest()).unwrap();
    let unsafe_body = safe
        .replace("delta close(h)", "yield;delta close(h)")
        .replace("[closed|failed]:yield;", "[closed|failed]:");
    let error = compile(&format!("{unsafe_body}{caller}"), manifest()).unwrap_err();
    assert_eq!(error.code, "E_ASYNC");
    assert!(error.message.contains("call beside started children"));
    compile(
        &format!(
            "{unsafe_body}{}",
            caller.replace("helper(h);await child;", "await child;helper(h);")
        ),
        manifest(),
    )
    .unwrap();
}

#[test]
fn transitive_generic_and_constructor_helpers_allow_checked_handoffs() {
    let prefix = "async control worker() exits done {yield;exit done;}";
    for (helper, call) in [
        (
            "control inner() exits done {run worker();exit done;} control helper() exits done {forward inner();}",
            "helper();",
        ),
        (
            "control helper<t>() exits done {forward run worker();}",
            "helper<u32>();",
        ),
        (
            "control helper() exits done {delta false {true=>handoff,false=>normal} [handoff]:run worker(); [handoff|normal]:exit done;}",
            "helper();",
        ),
        (
            "struct box {string text;constructor() {run worker();self.text=\"value\";}}",
            "auto value=box();drop(value);",
        ),
        (
            "control helper() exits ready {string text} {run worker();exit ready {\"value\"};}",
            "auto saved=capture helper();delta saved {ready {string text}=>ready} [ready]:drop(text);",
        ),
    ] {
        let source = format!(
            "{prefix} {helper} async control startup() exits done {{auto child=spawn worker();{call}await child;exit done;}} control main() exits done {{forward run startup();}}"
        );
        let p = compile(&source, manifest()).unwrap();
        let expected = p.run(&scenario()).unwrap();
        assert_eq!(
            p.expanded("main").unwrap().run(&scenario()).unwrap(),
            expected
        );
    }
}
