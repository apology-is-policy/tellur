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
        "auto child=spawn work();sync();await child;exit done;",
        "auto child=spawn work();await work();await child;exit done;",
        "auto child=spawn work();auto cold=defer work();await cold;await child;exit done;",
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
        "async control work(ref string text) exits done {exit done;} async control parent() exits done {string text=\"text\";auto child=spawn work(borrow text);await child;exit done;}",
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
    for statement in ["helper();", "fault boundary {} contingency [] {escalate;}"] {
        let text = format!(
            "{prefix} async control startup() exits done faults {{auto pending=spawn work();auto wrapped=some {{some {{pending}}}};{statement}{suffix}}}"
        );
        let error = compile(&text, manifest()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{error:?}");
    }
    for handoff in ["yield;", "await other;"] {
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
