mod support;
use tellur::{
    compile,
    manifest::Manifest,
    runtime::{Event, Scenario},
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
fn checked(s: &str) -> tellur::CheckedProgram {
    compile(s, manifest()).unwrap()
}
fn rejects(s: &str, code: &str) {
    let e = match compile(s, manifest()) {
        Err(e) => e,
        Ok(_) => panic!("expected {code} for {s}"),
    };
    assert_eq!(e.code, code, "{e}");
}
const RECOVERY: &str = r#"
control work(bool broken) exits done {
 delta broken {true=>fault "request failed",false=>exit done}
}
@contingent control report(ref fault_report info) exits done {emit(borrow info.message);exit done;}
control main() exits done {
 supervised loop requestor(mutable bool first=true) {
    bool broken=first;
    first=false;
    mutable string context="request context";
    fault boundary {
        auto view=borrow context;
        work(broken);
        emit(view);
        exit done;
    } contingency [context] {
        auto edit=borrow mutable context;
        deref edit="retired work";
        emit(edit);
        report(@fault);
        recover again requestor;
    }
 }
}
"#;

#[test]
fn recovery_preserves_preboundary_supervisor_state_and_discards_request_continuation() {
    let p = checked(RECOVERY);
    assert!(!p.region("main").unwrap().faults);
    let result = p.run(&scenario()).unwrap();
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
        ["retired work", "request failed", "request context"]
    );
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::Fault { .. }))
            .count(),
        1
    );
    assert_eq!(result.outcome, "done");
    let expanded = p.expanded("main").unwrap();
    let after = expanded.run(&scenario()).unwrap();
    assert_eq!(after.outcome, result.outcome);
    assert_eq!(
        after
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Emit { message } => Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        messages
    );
}

#[test]
fn handlers_have_only_retained_owners_and_administrative_continuations() {
    rejects(
        &RECOVERY.replace("report(@fault);", "emit(context); emit(broken);"),
        "E_CONTINGENCY_CAPTURE",
    );
    rejects(
        &RECOVERY.replace("recover again requestor;", "exit done;"),
        "E_CONTINGENCY_EXIT",
    );
    rejects(
        &RECOVERY.replace("recover again requestor;", "again requestor(false);"),
        "E_CONTINGENCY_EXIT",
    );
    rejects(
        &RECOVERY.replace("recover again requestor;", "terminate;"),
        "E_BOUNDARY",
    );
    rejects(
        &RECOVERY.replace("recover again requestor;", ""),
        "E_UNCOVERED",
    );
    rejects(
        &RECOVERY.replace("recover again requestor;", "recover again unrelated;"),
        "E_RECOVER",
    );
    rejects(&RECOVERY.replace("supervised loop", "loop"), "E_RECOVER");
    rejects(
        &RECOVERY.replace("work(broken);", "work(first);"),
        "E_SUPERVISOR_STATE",
    );
    rejects(
        &format!(
            "pub control ordinary() exits done {{exit done;}}{}",
            RECOVERY.replace("report(@fault);", "ordinary();")
        ),
        "E_CONTINGENT",
    );
}

#[test]
fn captures_exist_at_entry_remain_whole_and_cannot_retain_outer_loans() {
    rejects(
        &RECOVERY.replace("contingency [context]", "contingency [view]"),
        "E_BOUNDARY_CAPTURE",
    );
    rejects(
        &RECOVERY.replace("work(broken);", "emit(context); work(broken);"),
        "E_LOAN",
    );
    rejects(
        &RECOVERY
            .replace("auto view=borrow context;", "string taken=move context;")
            .replace("emit(view);", "emit(taken);"),
        "E_BOUNDARY_RETAIN",
    );
    rejects(
        &RECOVERY
            .replace(
                "fault boundary {",
                "auto outside=borrow context; fault boundary {",
            )
            .replace("auto view=borrow context;", "auto view=outside;"),
        "E_LOAN",
    );
}

#[test]
fn normal_boundaries_fall_through_but_handler_failures_escape_to_the_nearest_outer_boundary() {
    let p = checked(
        r#"control main() exits done {
        fault boundary {emit("normal");} contingency [] {escalate;}
        emit("continued");exit done;
    }"#,
    );
    assert_eq!(p.run(&scenario()).unwrap().events.len(), 2);
    let p = checked(
        r#"control main() faults {
        fault boundary {
            fault boundary {fault "original";} contingency [] {emit(borrow @fault.message);fault "cleanup";}
        } contingency [] {emit(borrow @fault.message);escalate;}
    }"#,
    );
    let failure = p.run_detailed(&scenario(), 10000).unwrap_err();
    assert!(failure.report.to_string().contains("cleanup"));
    assert!(failure.report.to_string().contains("original"));
    assert_eq!(
        failure
            .events
            .iter()
            .filter(|e| matches!(e, Event::Fault { .. }))
            .count(),
        2
    );
}

#[test]
fn native_and_expanded_recovery_match_reference() {
    use std::{fs, process::Command};
    let p = checked(RECOVERY);
    let root = std::env::temp_dir().join(format!("tellur-lexical-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let case = root.join("case.json");
    fs::write(&case, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let expected = p.run(&scenario()).unwrap();
        let binary = root.join(format!("recovery-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        let output = Command::new(binary)
            .args(["--scenario", case.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

const PROCESS: &str = r#"
control main() exits done {
    fault boundary {fault "startup failed";} contingency [] {
        emit(borrow @fault.message);
        terminate process 23;
    }
}
"#;
#[test]
fn process_termination_is_entry_only_and_never_an_outcome() {
    use tellur::runtime::RunCompletion;
    let p = checked(PROCESS);
    assert!(p.region("main").unwrap().entry_only());
    assert!(!p.region("main").unwrap().faults);
    let RunCompletion::Process(r) = p.run_completion(&scenario(), 10000).unwrap() else {
        panic!("invented outcome");
    };
    assert_eq!(r.status, 23);
    let json = serde_json::to_value(&r).unwrap();
    assert!(json.get("outcome").is_none());
    assert_eq!(p.run(&scenario()).unwrap_err().code, "E_PROCESS_EXIT");
    rejects(
        &format!("{PROCESS} control caller() exits done {{main();exit done;}}"),
        "E_ENTRY_ONLY",
    );
    rejects(
        "control main() exits done {terminate process 2;}",
        "E_PROCESS_EXIT",
    );
    rejects(&PROCESS.replace("23", "256"), "E_PROCESS_EXIT");
    rejects(&PROCESS.replace("23", "1 + 2"), "E_PARSE");
    rejects(
        &PROCESS
            .replace(
                "fault boundary {fault",
                "fault boundary {fault boundary {fault",
            )
            .replace("\n}\n", "} contingency [] {escalate;}\n}\n"),
        "E_PROCESS_EXIT",
    );
    let changed = checked(&PROCESS.replace("23", "24"));
    let agreement = tellur::algebra::compare_scenarios(&p, &changed, &[scenario()], 10000).unwrap();
    assert!(!agreement.observations_match);
}

#[test]
fn process_status_and_structured_report_match_native_and_expansion() {
    use std::{fs, process::Command};
    let p = checked(PROCESS);
    let root = std::env::temp_dir().join(format!("tellur-process-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let case = root.join("case.json");
    fs::write(&case, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let expected = p.run_completion(&scenario(), 10000).unwrap();
        let binary = root.join(format!("process-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        let output = Command::new(binary)
            .args(["--scenario", case.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn retained_resources_are_completed_and_supervisor_owners_survive_recovery() {
    use tellur::{ir::Value, runtime::ScriptCall};
    let mut m = manifest();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "close")
        .unwrap()
        .contingent = true;
    let source = r#"control main(handle listener) exits done {
        supervised loop requests(handle server=listener,mutable bool first=true) {
            delta first {true=>first_request,false=>finished}
            [first_request]:first=false;
                delta acquire() {opened {handle connection}=>opened}
                [opened]:fault boundary {fault "failed";} contingency [connection] {
                    delta close(connection) {closed=>closed,failed{close_error error}=>failed}
                    [closed|failed]:recover again requests;
                }
            [finished]:delta close(server) {closed=>closed,failed{close_error error}=>failed}
            [closed|failed]:exit done;
        }
    }"#;
    m.primitives.push(tellur::manifest::Primitive {
        returns: true,
        rust: None,
        name: "acquire".into(),
        parameters: vec![],
        outcomes: vec![tellur::manifest::Outcome {
            name: "opened".into(),
            fields: vec![tellur::manifest::Field {
                name: "connection".into(),
                ty: "handle".into(),
            }],
        }],
        effects: vec![],
        pure: false,
        contingent: false,
    });
    let p = compile(source, m.clone()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: ["listener"]
            .iter()
            .map(|t| Value::Opaque {
                ty: "handle".into(),
                token: t.to_string(),
            })
            .collect(),
        calls: std::iter::once(ScriptCall {
            operation: "acquire".into(),
            outcome: "opened".into(),
            fields: vec![Value::Opaque {
                ty: "handle".into(),
                token: "connection".into(),
            }],
        })
        .chain((0..2).map(|_| ScriptCall {
            operation: "close".into(),
            outcome: "closed".into(),
            fields: vec![],
        }))
        .collect(),
    };
    let result = p.run(&s).unwrap();
    assert_eq!(result.outcome, "done");
    let closed: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Primitive {
                operation,
                arguments,
                ..
            } = e
            {
                if operation == "close" {
                    Some(arguments[0].clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        closed,
        vec![
            Value::Opaque {
                ty: "handle".into(),
                token: "connection".into()
            },
            s.arguments[0].clone()
        ]
    );
    let no_close=source.replace("delta close(connection) {closed=>closed,failed{close_error error}=>failed}\n                    [closed|failed]:", "");
    assert_eq!(
        compile(&no_close, m.clone()).unwrap_err().code,
        "E_RESOURCE"
    );
    assert_eq!(
        compile(
            &source.replace("contingency [connection]", "contingency []"),
            m.clone()
        )
        .unwrap_err()
        .code,
        "E_FAULT_RESOURCE"
    );
    assert_eq!(
        compile(
            &source.replace("recover again requests;", "terminate process 2;"),
            m.clone()
        )
        .unwrap_err()
        .code,
        "E_FAULT_RESOURCE"
    );
    assert_eq!(
        compile(&source.replace("recover again requests;", "escalate;"), m)
            .unwrap_err()
            .code,
        "E_FAULT_RESOURCE"
    );
}

#[test]
fn handler_cannot_import_old_paths_or_mutate_fault_report() {
    rejects(
        &RECOVERY.replace("report(@fault);", "deref @fault.message=\"forged\";"),
        "E_BORROW",
    );
    rejects(
        &RECOVERY.replace(
            "report(@fault);",
            "auto forged=borrow mutable @fault.message;",
        ),
        "E_MUTABILITY",
    );
    rejects(
        &RECOVERY
            .replace(
                "work(broken);",
                "delta broken {true=>past,false=>other}[past|other]:work(broken);",
            )
            .replace("report(@fault);", "[past]:report(@fault);"),
        "E_PATH",
    );
    rejects(
        &RECOVERY.replace("contingency [context]", "contingency [context, context]"),
        "E_BOUNDARY_CAPTURE",
    );
}

#[test]
fn nested_boundaries_preserve_outer_supervisor_values_and_bounded_fault_metadata() {
    let source = RECOVERY
        .replace(
            "supervised loop requestor",
            "string shared=\"kept configuration\"; supervised loop requestor",
        )
        .replace(
            "mutable string context=\"request context\";",
            "mutable string context=clone(borrow shared);",
        )
        .replace(
            "work(broken);",
            "fault boundary {work(broken);} contingency [] {escalate;}",
        );
    let p = checked(&source);
    let r = p.run(&scenario()).unwrap();
    assert!(matches!(r.events.last(),Some(Event::Emit{message}) if message=="kept configuration"));
    p.expanded("main").unwrap().run(&scenario()).unwrap();
    rejects(
        &source.replace("work(broken);", "emit(borrow shared);work(broken);"),
        "E_SUPERVISOR_STATE",
    );
    checked(&PROCESS.replace("emit(borrow @fault.message);", "u32 frames=vector_length(borrow @fault.stack);u32 sites=vector_length(borrow @fault.sites);"));
}

#[test]
fn expansion_certificates_cannot_change_recovery_state_or_retirement_sets() {
    use tellur::ir::{Action, Expr, Instruction, Value};
    fn walk(code: &mut [Instruction], f: &mut impl FnMut(&mut Action)) {
        for i in code {
            f(&mut i.action);
            match &mut i.action {
                Action::Loop { body, .. } => walk(body, f),
                Action::Boundary { body, handler, .. } => {
                    walk(body, f);
                    walk(handler, f);
                }
                _ => (),
            }
        }
    }
    let p = checked(RECOVERY);
    let original = p.expansion_artifact("main").unwrap();
    for mutation in 0..4 {
        let mut artifact = original.clone();
        let mut changed = false;
        walk(&mut artifact.region.code, &mut |a| match a {
            Action::Recover { args, .. } if mutation == 0 => {
                args[0] = Expr::Value(Value::Bool(true));
                changed = true;
            }
            Action::Boundary { retained, .. } if mutation == 1 => {
                retained.clear();
                changed = true;
            }
            Action::Boundary { retire, .. } if mutation == 2 => {
                retire.clear();
                changed = true;
            }
            Action::Loop { supervised, .. } if mutation == 3 => {
                *supervised = false;
                changed = true;
            }
            _ => (),
        });
        assert!(changed);
        assert_eq!(
            p.accept_expansion(&artifact).unwrap_err().code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
}
