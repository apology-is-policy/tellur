use super::*;
use crate::{
    compile,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};

fn checked(text: &str) -> CheckedProgram {
    compile(
        text,
        Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
    )
    .unwrap()
}

#[test]
fn production_and_probes_keep_named_boundaries_private() {
    for text in [
        "boundary work() {terminate;} contingency(string reason) {terminate;} control main() exits done {supervise work();exit done;}",
        "boundary work() {terminate;} contingency(string reason) {terminate;} control main() exits done {fault boundary {supervise work();} contingency [] {terminate process 1;}exit done;}",
    ] {
        let p = checked(text)
            .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
            .unwrap();
        let model = source(&p, "main").unwrap();
        assert!(model.contains("Step::Supervise"));
        // Named work and contingency remain unavailable as public entry controls.
        assert_eq!(
            model
                .matches("entry controls cannot require borrowed arguments")
                .count(),
            2
        );
        let standard = p.native_source("main").unwrap();
        assert_eq!(standard.matches(EXECUTION).count(), 1);
        assert_eq!(standard.matches(CONTEXTS).count(), 1);
        assert!(!standard.contains("TELLUR_NATIVE_MODEL_QUOTAS"));
        assert!(!standard.contains("context harness"));
        assert!(standard.contains("static MODEL_"));
        assert!(standard.contains("run_entry(host,args,"));
        assert!(!standard.contains("drive(host,args,"));
    }
}

#[test]
fn compiled_named_supervision_preserves_handover_borrowed_awaits_and_repeated_requests() {
    compare_fixture(
        "supervision",
        include_str!("../../tests/fixtures/native_resume_supervision.tellur"),
        [&[7, 9], &[7, 9]],
        400,
        false,
    );
}

#[test]
fn compiled_named_handler_faults_and_escalation_preserve_outer_lexical_causes() {
    compare_fixture(
        "supervision-causes",
        r#"
boundary inner(bool stop) retains stop {
    fault boundary {fault "original";} contingency [] {escalate;}
} contingency(string reason) {
    emit(reason);
    delta stop {true => failed, false => propagated}
    [failed]: fault "handler failed";
    [propagated]: escalate;
}
boundary outer(bool stop) {
    supervise inner(stop);
    terminate;
} contingency(string reason) {emit(reason);escalate;}
control main(bool stop) exits done faults {
    fault boundary {supervise outer(stop);} contingency [] {
        emit(borrow @fault.message);escalate;
    }
    exit done;
}
"#,
        [&[], &[]],
        150,
        false,
    );
}

#[test]
fn compiled_calls_awaits_and_constructors_preserve_every_fuel_cut_and_fault() {
    compare_fixture(
        "calls",
        include_str!("../../tests/fixtures/native_resume.tellur"),
        [&[7], &[7]],
        150,
        false,
    );
}

#[test]
fn compiled_nested_loops_preserve_restarts_expanded_completion_and_faults() {
    compare_fixture(
        "loops",
        include_str!("../../tests/fixtures/native_resume_loops.tellur"),
        [&[7, 9], &[7, 9]],
        350,
        true,
    );
}

#[test]
fn compiled_lexical_recovery_preserves_nested_completion_borrows_and_fault_reports() {
    compare_fixture(
        "boundaries",
        include_str!("../../tests/fixtures/native_resume_boundaries.tellur"),
        [&[7], &[7, 9]],
        400,
        false,
    );
}

#[test]
fn compiled_handler_faults_and_escalation_retain_nested_causes() {
    compare_fixture(
        "causes",
        r#"
control main(bool stop) exits done faults {
    fault boundary {
        fault boundary {fault "original";} contingency [stop] {
            emit(borrow @fault.message);
            delta stop {true => failed, false => propagated}
            [failed]: fault "handler failed";
            [propagated]: escalate;
        }
    } contingency [] {emit(borrow @fault.message);escalate;}
}
"#,
        [&[], &[]],
        150,
        false,
    );
}

#[test]
fn compiled_process_termination_stays_administrative() {
    compare_fixture(
        "process",
        r#"
control main(bool stop) exits done {
    fault boundary {
        delta stop {true => failed, false => ready}
        [failed]: fault "startup failed";
        [ready]: emit("ready");exit done;
    } contingency [] {emit(borrow @fault.message);terminate process 23;}
}
"#,
        [&[], &[]],
        100,
        false,
    );
}

fn compare_fixture(
    label: &str,
    text: &str,
    replies: [&[u32]; 2],
    limit: usize,
    require_unwind: bool,
) {
    let root = std::env::temp_dir().join(format!(
        "tellur-native-resume-{label}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let original = checked(text)
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    // Expansion is control-local and preserves async calls. Expand startup too
    // when testing the loop nested by its synchronous helper's expansion.
    let expanded = if original.region("startup").is_some() {
        original.expanded("startup").unwrap()
    } else {
        original.clone()
    }
    .expanded("main")
    .unwrap();
    for (expanded, p) in [(false, original.clone()), (true, expanded)] {
        if expanded && require_unwind {
            fn has_complete(code: &[Instruction]) -> bool {
                code.iter().any(|i| match &i.action {
                    Action::Complete {
                        unwind: Some(_), ..
                    } => true,
                    Action::Loop { body, .. } => has_complete(body),
                    _ => false,
                })
            }
            assert!(
                p.regions().any(|r| has_complete(&r.code)),
                "expanded loop return must be exercised"
            );
            fn has_nested_loop(code: &[Instruction], inside_loop: bool) -> bool {
                code.iter().any(|i| match &i.action {
                    Action::Loop { body, .. } => inside_loop || has_nested_loop(body, true),
                    _ => false,
                })
            }
            assert!(
                p.regions().any(|r| has_nested_loop(&r.code, false)),
                "expansion must nest a helper loop inside its caller's loop"
            );
        }
        let generated = source(&p, "main").unwrap();
        assert!(generated.contains("state.pc") && generated.contains("Step::Call"));
        for opt in [0, 2] {
            let binary = root.join(format!("resume-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for stop in [false, true] {
                let scenario = Scenario {
                    entry: "main".into(),
                    arguments: vec![Value::Bool(stop)],
                    calls: replies[usize::from(stop)]
                        .iter()
                        .map(|n| ScriptCall {
                            operation: "parse_u32".into(),
                            outcome: "number".into(),
                            fields: vec![Value::U32(*n)],
                            waits: 0,
                        })
                        .collect(),
                };
                let input = root.join("scenario.json");
                fs::write(&input, serde_json::to_vec(&scenario).unwrap()).unwrap();
                let mut reached_end = false;
                for fuel in 0..limit {
                    let expected = p.run_completion(&scenario, fuel);
                    if require_unwind
                        && let Ok(crate::runtime::RunCompletion::Outcome(result)) = &expected
                    {
                        let messages: Vec<_> = result
                            .events
                            .iter()
                            .filter_map(|e| match e {
                                crate::runtime::Event::Emit { message } => Some(message.as_str()),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(messages, ["7", "left", "helper retry", "9", "right"]);
                    }
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        let output = Command::new(&binary)
                            .args([
                                "--scenario",
                                input.to_str().unwrap(),
                                "--fuel",
                                &fuel.to_string(),
                            ])
                            .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                            .output()
                            .unwrap();
                        match &expected {
                            Ok(result) => {
                                let status = match result {
                                    crate::runtime::RunCompletion::Outcome(_) => 0,
                                    crate::runtime::RunCompletion::Process(result) => {
                                        i32::from(result.status)
                                    }
                                };
                                assert_eq!(
                                    output.status.code(),
                                    Some(status),
                                    "{}",
                                    String::from_utf8_lossy(&output.stderr)
                                );
                                assert_eq!(
                                    serde_json::from_slice::<serde_json::Value>(&output.stdout)
                                        .unwrap(),
                                    serde_json::to_value(result).unwrap()
                                );
                            }
                            Err(failure) => {
                                assert_eq!(
                                    output.status.code(),
                                    Some(1),
                                    "{}",
                                    String::from_utf8_lossy(&output.stderr)
                                );
                                assert!(output.stdout.is_empty());
                                let mut actual: crate::fault::Report =
                                    serde_json::from_slice(&output.stderr).unwrap();
                                fn normalize_fuel(
                                    actual: &mut crate::fault::Report,
                                    expected: &crate::fault::Report,
                                ) {
                                    // Native/reference host-limit wording differs; keep
                                    // every identity, site, phase and non-fuel cause exact.
                                    if expected.message.starts_with("E_FUEL") {
                                        assert!(actual.message.starts_with("E_FUEL"));
                                        actual.message = expected.message.clone();
                                    }
                                    if let (Some(actual), Some(expected)) =
                                        (&mut actual.cause, &expected.cause)
                                    {
                                        normalize_fuel(actual, expected);
                                    }
                                }
                                normalize_fuel(&mut actual, &failure.report);
                                assert_eq!(
                                    actual, failure.report,
                                    "expanded={expanded},opt={opt},fuel={fuel},quotas={quotas}"
                                );
                            }
                        }
                    }
                    if expected
                        .as_ref()
                        .map(|_| true)
                        .unwrap_or_else(|e| e.report.kind == crate::fault::Kind::SourceFault)
                    {
                        reached_end = true;
                        break;
                    }
                }
                assert!(
                    reached_end,
                    "all exhaustion cuts and final completion/fault must execute"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn production_and_probes_share_bound_cold_callbacks_without_probe_controls() {
    let p = checked(
        r#"
async control child() exits done {exit done;}
async control startup() exits done {auto pending = defer child();await pending;exit done;}
control main() exits done {run startup();exit done;}
"#,
    )
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let emitted = source(&p, "main").unwrap();
    assert!(emitted.contains("let task=state.defer_task(host,&MODEL_"));
    assert!(emitted.contains("let task=state.queue_task(host,&MODEL_"));
    assert!(emitted.contains("return Ok(Step::Await{task,deliver:"));
    assert!(emitted.contains("contexts.start_entry(control, args)?"));
    let standard = p.native_source("main").unwrap();
    assert!(standard.contains("let task=state.defer_task(host,&MODEL_"));
    assert!(standard.contains("let task=state.queue_task(host,&MODEL_"));
    assert!(!standard.contains("TELLUR_NATIVE_MODEL_QUOTAS"));
    assert!(!standard.contains("fn region_"));
    let callbacks = super::super::continuations::source(&p);
    assert!(standard.contains(&callbacks));
    assert!(emitted.contains(&callbacks));
}

#[test]
fn compiled_direct_awaits_preserve_named_and_lexical_fault_context() {
    let text = include_str!("../../tests/fixtures/native_resume_supervision.tellur").replace(
        "    auto pending = defer visit(context, stop);\n    await pending;",
        "    await visit(context, stop);",
    );
    assert!(text.contains("await visit(context, stop)"));
    compare_fixture("direct-supervision", &text, [&[7, 9], &[7, 9]], 400, false);
}
