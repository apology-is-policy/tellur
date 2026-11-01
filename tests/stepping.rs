use tellur::{
    CheckedProgram, compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, ExecutionProgress, Failure, RunCompletion, Scenario, ScriptCall},
};
fn checked(source: &str) -> CheckedProgram {
    compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn snapshot(result: &Result<RunCompletion, Box<Failure>>) -> serde_json::Value {
    match result {
        Ok(result) => serde_json::json!({"complete": result}),
        Err(failure) => serde_json::json!({"failure": {
            "code": failure.diagnostic.code, "span": failure.diagnostic.span,
            "message": failure.diagnostic.message, "report": failure.report, "events": failure.events,
        }}),
    }
}
fn partitioned(
    p: &CheckedProgram,
    s: &Scenario,
    fuel: usize,
    sizes: &[usize],
) -> Result<RunCompletion, Box<Failure>> {
    let mut session = p.start_execution(s, fuel);
    assert!(session.events().is_empty());
    assert!(matches!(session.advance(0), ExecutionProgress::Paused));
    let mut prefix = vec![];
    for i in 0..10000 {
        match session.advance(sizes[i % sizes.len()]) {
            ExecutionProgress::Paused => {
                let events = session.events().to_vec();
                assert!(events.starts_with(&prefix));
                assert!(matches!(session.advance(0), ExecutionProgress::Paused));
                assert_eq!(session.events(), events);
                prefix = events;
            }
            ExecutionProgress::Complete(result) => {
                let events = match &result {
                    Ok(RunCompletion::Outcome(r)) => &r.events,
                    Ok(RunCompletion::Process(r)) => &r.events,
                    Err(r) => &r.events,
                };
                assert!(events.starts_with(&prefix));
                assert!(session.events().is_empty());
                assert!(matches!(session.advance(0), ExecutionProgress::Finished));
                assert!(matches!(
                    session.advance(usize::MAX),
                    ExecutionProgress::Finished
                ));
                return result;
            }
            ExecutionProgress::Finished => panic!("result was lost"),
        }
    }
    panic!("partitioned execution did not finish within its fixed fuel");
}
fn partitions(p: &CheckedProgram, s: &Scenario, fuel: usize) {
    let expected = snapshot(&p.run_completion(s, fuel));
    for sizes in [&[1][..], &[3, 0, 2, 1], &[7, 2, 11]] {
        assert_eq!(
            snapshot(&partitioned(p, s, fuel, sizes)),
            expected,
            "fuel={fuel}, sizes={sizes:?}"
        );
    }
}
const SOURCE: &str = r#"
struct message {
 string text;
 constructor(string text) {self.text=move text;}
}
control lend(ref mutable string input) exits lent {ref mutable string value from input} {
 exit lent {input};
}
control revise(ref mutable string input, string text) exits done {
 deref input=move text;
 exit done;
}
control main() exits done {
 auto owned=message("before");
 mutable string parent=move owned.text;
 delta lend(borrow mutable parent) {lent {auto child}=>lent}
 [lent]:revise(child,"after");
 emit(child);
 parent="released";
 emit(borrow parent);
 exit done;
}
"#;
const REQUEST: &str = r#"
control work(bool fail) exits done {
    delta fail { true => bad, false => good }
    [bad]: fault "broken request";
    [good]: exit done;
}
@contingent
control report(ref string context, string reason) exits done {
    emit(clone(context)); emit(reason); exit done;
}
boundary request(bool fail, string context) retains context {
    work(fail);
    emit("normal request");
    terminate;
}
contingency(string reason) {
    report(borrow context, reason);
    terminate;
}
control main() exits done {
    supervise request(true, "retained context");
    supervise request(false, "second context");
    emit("supervisor continued");
    exit done;
}
"#;
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
const PROCESS: &str = r#"
control main() exits done {
    fault boundary {fault "startup failed";} contingency [] {
        emit(borrow @fault.message);
        terminate process 23;
    }
}
"#;

#[test]
fn pauses_preserve_borrowed_constructor_values_nested_calls_and_expansion() {
    let original = checked(SOURCE);
    let s = scenario();
    for p in [original.clone(), original.expanded("main").unwrap()] {
        for fuel in 0..80 {
            partitions(&p, &s, fuel);
        }
        let RunCompletion::Outcome(run) = partitioned(&p, &s, 1000, &[1]).unwrap() else {
            panic!()
        };
        assert_eq!(run.outcome, "done");
        assert_eq!(
            run.events,
            [
                Event::Emit {
                    message: "after".into()
                },
                Event::Emit {
                    message: "released".into()
                }
            ]
        );
    }
}
#[test]
fn partitions_preserve_named_handover_normal_settlement_and_escalation() {
    for source in [
        REQUEST.to_string(),
        REQUEST.replace(
            "report(borrow context, reason);\n    terminate;",
            "report(borrow context, reason);\n    escalate;",
        ),
    ] {
        let p = checked(&source);
        for fuel in 0..80 {
            partitions(&p, &scenario(), fuel);
        }
    }
    let p = checked(REQUEST);
    let RunCompletion::Outcome(run) = partitioned(&p, &scenario(), 1000, &[1]).unwrap() else {
        panic!()
    };
    assert_eq!(
        run.events
            .iter()
            .filter_map(|e| match e {
                Event::Emit { message } => Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            "retained context",
            "broken request",
            "normal request",
            "supervisor continued"
        ]
    );
}
#[test]
fn partitions_preserve_lexical_recovery_contingency_calls_and_process_termination() {
    for source in [RECOVERY, PROCESS] {
        let p = checked(source);
        for p in [p.clone(), p.expanded("main").unwrap()] {
            for fuel in 0..100 {
                partitions(&p, &scenario(), fuel);
            }
        }
    }
    let p = checked(PROCESS);
    let RunCompletion::Process(run) = partitioned(&p, &scenario(), 1000, &[1]).unwrap() else {
        panic!()
    };
    assert_eq!(run.status, 23);
    let p = checked(RECOVERY);
    let RunCompletion::Outcome(run) = partitioned(&p, &scenario(), 1000, &[1]).unwrap() else {
        panic!()
    };
    assert_eq!(
        run.events
            .iter()
            .filter_map(|e| match e {
                Event::Emit { message } => Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["retired work", "request failed", "request context"]
    );
}
#[test]
fn entry_driver_and_result_failures_keep_their_phase_and_cannot_resume() {
    let p = checked(
        "control main(string input) exits done {delta parse_u32(input){number {auto n}=>exit done,malformed=>exit done,overflow=>exit done}}",
    );
    let good = ScriptCall {
        operation: "parse_u32".into(),
        outcome: "number".into(),
        fields: vec![Value::U32(1)],
    };
    let mut scenarios = vec![scenario()];
    let valid = Scenario {
        entry: "main".into(),
        arguments: vec![Value::String("1".into())],
        calls: vec![good.clone()],
    };
    scenarios.push(valid.clone());
    scenarios.push(Scenario {
        entry: "unknown".into(),
        ..valid.clone()
    });
    scenarios.push(Scenario {
        arguments: vec![Value::Bool(true)],
        ..valid.clone()
    });
    scenarios.push(Scenario {
        calls: vec![],
        ..valid.clone()
    });
    scenarios.push(Scenario {
        calls: vec![good.clone(), good.clone()],
        ..valid.clone()
    });
    scenarios.push(Scenario {
        calls: vec![ScriptCall {
            operation: "wrong".into(),
            ..good.clone()
        }],
        ..valid.clone()
    });
    scenarios.push(Scenario {
        calls: vec![ScriptCall {
            outcome: "wrong".into(),
            ..good.clone()
        }],
        ..valid.clone()
    });
    scenarios.push(Scenario {
        calls: vec![ScriptCall {
            fields: vec![Value::Bool(true)],
            ..good.clone()
        }],
        ..valid
    });
    for s in scenarios {
        for fuel in [0, 1, 2, 100] {
            partitions(&p, &s, fuel);
        }
    }
}
#[test]
fn zero_quota_has_no_effect_and_independent_sessions_can_interleave() {
    let p = checked("control main(string input) exits done {emit(borrow input);exit done;}");
    let one = Scenario {
        arguments: vec![Value::String("one".into())],
        ..scenario()
    };
    let two = Scenario {
        arguments: vec![Value::String("two".into())],
        ..scenario()
    };
    let mut first = p.start_execution(&one, 100);
    let mut second = p.start_execution(&two, 100);
    for _ in 0..8 {
        assert!(matches!(first.advance(0), ExecutionProgress::Paused));
        assert!(first.events().is_empty());
    }
    assert!(matches!(first.advance(1), ExecutionProgress::Paused));
    assert!(second.events().is_empty());
    assert!(matches!(second.advance(1), ExecutionProgress::Paused));
    assert_eq!(
        first.events(),
        [Event::Emit {
            message: "one".into()
        }]
    );
    assert_eq!(
        second.events(),
        [Event::Emit {
            message: "two".into()
        }]
    );
    let ExecutionProgress::Complete(b) = second.advance(100) else {
        panic!()
    };
    let ExecutionProgress::Complete(a) = first.advance(100) else {
        panic!()
    };
    assert_eq!(snapshot(&a), snapshot(&p.run_completion(&one, 100)));
    assert_eq!(snapshot(&b), snapshot(&p.run_completion(&two, 100)));
}

#[test]
fn resource_handover_and_failed_cleanup_outcomes_are_preserved_between_steps() {
    let mut m = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "close")
        .unwrap()
        .contingent = true;
    let p = compile(
        r#"
        boundary request(handle owned) retains owned { fault "broken"; }
        contingency(string reason) {
            delta close(owned) { closed => closed, failed { close_error error } => failed }
            [closed | failed]: terminate;
        }
        control main(handle owned) exits done { supervise request(owned); exit done; }
    "#,
        m,
    )
    .unwrap();
    for (outcome, fields) in [
        ("closed", vec![]),
        (
            "failed",
            vec![Value::Opaque {
                ty: "close_error".into(),
                token: "close failed".into(),
            }],
        ),
    ] {
        let s = Scenario {
            arguments: vec![Value::Opaque {
                ty: "handle".into(),
                token: "retained".into(),
            }],
            calls: vec![ScriptCall {
                operation: "close".into(),
                outcome: outcome.into(),
                fields,
            }],
            ..scenario()
        };
        for fuel in 0..30 {
            partitions(&p, &s, fuel);
        }
        let RunCompletion::Outcome(run) = partitioned(&p, &s, 100, &[1]).unwrap() else {
            panic!()
        };
        assert_eq!(run.outcome, "done");
        let closes: Vec<_> = run
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Primitive {
                    operation,
                    arguments,
                    outcome,
                    ..
                } if operation == "close" => Some((arguments, outcome)),
                _ => None,
            })
            .collect();
        assert_eq!(closes.len(), 1);
        assert_eq!(closes[0].0, &s.arguments);
        assert_eq!(closes[0].1, outcome);
    }
}

#[test]
fn nested_boundaries_and_secondary_faults_keep_separate_emergency_histories() {
    let sources = [
        r#"boundary inner() {fault "first";} contingency(string reason){escalate;}
           boundary outer(){supervise inner();emit("unreachable");terminate;}
           contingency(string reason){emit(reason);terminate;}
           control main() exits done {supervise outer();exit done;}"#
            .to_string(),
        REQUEST.replace(
            "report(borrow context, reason);\n    terminate;",
            "fault \"cleanup broke\";",
        ),
        r#"control main() exits done {
            fault boundary {
                fault boundary { fault "first"; } contingency [] { escalate; }
            } contingency [] { emit(borrow @fault.message); terminate process 24; }
        }"#
        .to_string(),
    ];
    for source in sources {
        let p = checked(&source);
        for fuel in 0..80 {
            partitions(&p, &scenario(), fuel);
        }
    }
}

#[test]
fn malformed_later_entry_payload_never_enters_or_emits_under_any_partition() {
    let p = checked(
        "struct box {u32 n;} control main(string text, box item) exits done {emit(text);drop(item);exit done;}",
    );
    let s = Scenario {
        arguments: vec![
            Value::String("must not emit".into()),
            Value::Construct {
                ty: "box".into(),
                name: "box".into(),
                fields: vec![Value::Bool(true)],
            },
        ],
        ..scenario()
    };
    partitions(&p, &s, 100);
    let mut session = p.start_execution(&s, 100);
    assert!(matches!(session.advance(0), ExecutionProgress::Paused));
    let ExecutionProgress::Complete(Err(failure)) = session.advance(1) else {
        panic!()
    };
    assert_eq!(failure.diagnostic.code, "E_DRIVER");
    assert!(failure.diagnostic.message.contains("expected U32"));
    assert!(failure.events.is_empty());
    assert!(failure.report.activations.is_empty());
    assert!(matches!(
        failure.report.phase,
        tellur::fault::Phase::EntryValidation
    ));
    assert!(matches!(session.advance(100), ExecutionProgress::Finished));
}
