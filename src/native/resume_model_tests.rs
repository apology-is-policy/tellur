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
fn model_gates_unimplemented_continuations_without_changing_standard_emission() {
    for text in [
        "control main() exits done {loop work {fault boundary {emit(\"work\");} contingency [] {terminate process 1;}exit done;}}",
        "control main() exits done {fault boundary {emit(\"work\");} contingency [] {terminate process 1;}exit done;}",
    ] {
        let p = checked(text)
            .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
            .unwrap();
        assert!(
            source(&p, "main")
                .unwrap_err()
                .message
                .contains("does not yet support")
        );
        assert!(!p.native_source("main").unwrap().contains("struct Machine"));
    }
}

#[test]
fn compiled_calls_awaits_and_constructors_preserve_every_fuel_cut_and_fault() {
    compare_fixture(
        "calls",
        include_str!("../../tests/fixtures/native_resume.tellur"),
        vec![7],
        150,
        false,
    );
}

#[test]
fn compiled_nested_loops_preserve_restarts_expanded_completion_and_faults() {
    compare_fixture(
        "loops",
        include_str!("../../tests/fixtures/native_resume_loops.tellur"),
        vec![7, 9],
        350,
        true,
    );
}

fn compare_fixture(label: &str, text: &str, replies: Vec<u32>, limit: usize, require_unwind: bool) {
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
    let expanded = if require_unwind {
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
                    calls: replies
                        .iter()
                        .map(|n| ScriptCall {
                            operation: "parse_u32".into(),
                            outcome: "number".into(),
                            fields: vec![Value::U32(*n)],
                        })
                        .collect(),
                };
                let input = root.join("scenario.json");
                fs::write(&input, serde_json::to_vec(&scenario).unwrap()).unwrap();
                let mut reached_end = false;
                for fuel in 0..limit {
                    let expected = p.run_detailed(&scenario, fuel);
                    if require_unwind && let Ok(result) = &expected {
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
                                assert!(
                                    output.status.success(),
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
                                if failure.report.message.starts_with("E_FUEL") {
                                    assert!(actual.message.starts_with("E_FUEL"));
                                    // Existing native/reference host-limit text differs;
                                    // retain exact identities, sites, phases and causes.
                                    actual.message = failure.report.message.clone();
                                }
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
