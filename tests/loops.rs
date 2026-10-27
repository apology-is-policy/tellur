use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, Scenario, ScriptCall},
};
const RETRY: &str = include_str!("../examples/fetch_retry.tellur");
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../examples/manifest.json")).unwrap()
}
fn checked(s: &str) -> tellur::CheckedProgram {
    compile(s, manifest()).unwrap()
}
fn opaque(t: &str) -> Value {
    Value::Opaque {
        ty: t.into(),
        token: t.into(),
    }
}
fn call(op: &str, out: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: op.into(),
        outcome: out.into(),
        fields,
    }
}
fn failed_attempt(n: u32) -> Vec<ScriptCall> {
    vec![
        call("fetch", "Unavailable", vec![opaque("NetworkError")]),
        call("checked_mul", "Product", vec![Value::U32(n * 100)]),
        call("pause", "Elapsed", vec![]),
        call("checked_add", "Sum", vec![Value::U32(n + 1)]),
    ]
}
fn scenario(calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: "fetch_retry".into(),
        arguments: vec![Value::String("https://example.test".into())],
        calls,
    }
}
#[test]
fn c17_three_attempts_two_pauses() {
    let program = checked(RETRY);
    let mut calls = failed_attempt(1);
    calls.extend(failed_attempt(2));
    calls.push(call("fetch", "Received", vec![opaque("Response")]));
    let s = scenario(calls);
    let result = program.run(&s).unwrap();
    assert_eq!(result.outcome, "Done");
    let pauses: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Primitive {
                operation,
                arguments,
                ..
            } if operation == "pause" => Some(arguments.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(pauses, vec![vec![Value::U32(100)], vec![Value::U32(200)]]);
    assert_eq!(
        result,
        program.expanded("fetch_retry").unwrap().run(&s).unwrap()
    );
}
#[test]
fn c18_cancellation_stops_immediately() {
    let p = checked(RETRY);
    for calls in [
        vec![call("fetch", "Cancelled", vec![])],
        vec![
            call("fetch", "Unavailable", vec![opaque("NetworkError")]),
            call("checked_mul", "Product", vec![Value::U32(100)]),
            call("pause", "Cancelled", vec![]),
        ],
    ] {
        assert_eq!(p.run(&scenario(calls)).unwrap().outcome, "Cancelled");
    }
}
#[test]
fn exhausted_and_arithmetic_failures() {
    let p = checked(RETRY);
    let mut calls = failed_attempt(1);
    calls.extend(failed_attempt(2));
    calls.push(call("fetch", "Unavailable", vec![opaque("NetworkError")]));
    assert_eq!(p.run(&scenario(calls)).unwrap().outcome, "Failed");
    let calls = vec![
        call("fetch", "Unavailable", vec![opaque("NetworkError")]),
        call("checked_mul", "Overflow", vec![]),
    ];
    assert_eq!(p.run(&scenario(calls)).unwrap().outcome, "ArithmeticFault");
    let mut calls = failed_attempt(1);
    *calls.last_mut().unwrap() = call("checked_add", "Overflow", vec![]);
    assert_eq!(p.run(&scenario(calls)).unwrap().outcome, "ArithmeticFault");
}
#[test]
fn c19_missing_restart_has_witness() {
    let e = compile(
        &RETRY.replace("[NextReady]: again Attempt(next);", ""),
        manifest(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_UNCOVERED");
    assert!(e.message.contains("NextReady"));
}
#[test]
fn c20_historical_reference_is_rejected() {
    let e = compile(
        &RETRY.replace("[Entry]:", "[Attempt.Received]:"),
        manifest(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_PATH");
}
#[test]
fn invalid_loop_structure() {
    for source in [
        "region x() exits Done { again L(); }",
        "region x() exits Done { loop L() { [Entry]: again L(); } [Entry]: exit Done; }",
        "region x() exits Done { loop L() { loop M() { [Entry]: again M(); } } }",
        "region x() exits Done { [Entry]: again L(); }",
    ] {
        assert!(compile(source, manifest()).is_err());
    }
}
#[test]
fn again_type_and_arity_are_checked() {
    for args in ["", "true", "1, 2"] {
        let source = format!(
            "region x() exits Done {{ loop L(n: U32 = 1) {{ [Entry]: again L({args}); }} }}"
        );
        assert!(compile(&source, manifest()).is_err());
    }
}
#[test]
fn loop_initializers_are_simultaneous_outer_expressions() {
    assert!(
        compile(
            "region x() exits Done { loop L(a: U32 = 1, b: U32 = a) { [Entry]: exit Done; } }",
            manifest()
        )
        .is_err()
    );
}
#[test]
fn restarts_reset_atoms_and_values() {
    let source = "region x() exits Done(value: U32) { loop L(first: Bool = true) { [Entry]: delta first -> [true => First, false => Later]; [First]: v := 1; again L(false); [Later]: v := 2; exit Done(v); } }";
    let p = checked(source);
    let s = Scenario {
        entry: "x".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&s).unwrap().fields, vec![Value::U32(2)]);
}
#[test]
fn simultaneous_restart_arguments() {
    let p = checked(
        "region x() exits Done(a: U32,b: U32) { loop L(a: U32 = 1,b: U32 = 2,first: Bool = true) { [Entry]: delta first -> [true => First,false => Later]; [First]: again L(b,a,false); [Later]: exit Done(a,b); } }",
    );
    let s = Scenario {
        entry: "x".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&s).unwrap().fields,
        vec![Value::U32(2), Value::U32(1)]
    );
}
#[test]
fn outer_linear_resources_must_be_transferred() {
    let error=compile("region x(h: Handle) exits Done { loop L() { [Entry]: delta close(h) -> [Closed => C, Failed(e) => F]; [Entry]: again L(); } }",manifest()).unwrap_err();
    assert_eq!(error.code, "E_RESOURCE");
    let source = "region x(h: Handle) exits Done { loop L(current: Handle = h,first: Bool = true) { [Entry]: delta first -> [true => First,false => Later]; [First]: again L(current,false); [Later]: delta close(current) -> [Closed => C,Failed(e) => F]; [C | F]: exit Done; } }";
    let p = checked(source);
    let s = Scenario {
        entry: "x".into(),
        arguments: vec![opaque("Handle")],
        calls: vec![call("close", "Closed", vec![])],
    };
    assert_eq!(p.run(&s).unwrap().outcome, "Done");
}
#[test]
fn restarting_with_consumed_resource_is_rejected() {
    let source = "region x(h: Handle) exits Done { loop L(current: Handle = h) { [Entry]: delta close(current) -> [Closed => C,Failed(e) => F]; [C | F]: again L(current); } }";
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_MOVED");
}
#[test]
fn scoped_loop_leaves_other_histories_available() {
    let p = checked(
        "region x(flag: Bool) exits Done { [Entry]: delta flag -> [true => A,false => B]; [A] over &: { loop L() { [Entry]: again L(); } } [B]: exit Done; }",
    );
    let mut s = Scenario {
        entry: "x".into(),
        arguments: vec![Value::Bool(false)],
        calls: vec![],
    };
    assert_eq!(p.run(&s).unwrap().outcome, "Done");
    s.arguments = vec![Value::Bool(true)];
    assert_eq!(p.run_with_limit(&s, 30).unwrap_err().code, "E_FUEL");
}
#[test]
fn loop_enters_only_remaining_live_histories() {
    checked(
        "region x(flag: Bool) exits Done { [Entry]: delta flag -> [true => A,false => B]; [A]: exit Done; loop L() { [Entry]: exit Done; } }",
    );
}
#[test]
fn expansion_of_looping_callee_resumes_caller() {
    let p = checked(&format!(
        "{RETRY}\nregion caller(url: String) exits Done {{ [Entry]: delta fetch_retry(url) -> [Done(r) => OK, Failed(e) => Bad, Cancelled => Cancel, ArithmeticFault => Fault]; [Entry]: emit(\"after retry\"); exit Done; }}"
    ));
    let mut calls = failed_attempt(1);
    calls.push(call("fetch", "Received", vec![opaque("Response")]));
    let mut s = scenario(calls);
    s.entry = "caller".into();
    let result = p.run(&s).unwrap();
    assert_eq!(result, p.expanded("caller").unwrap().run(&s).unwrap());
    assert_eq!(
        result.events.last(),
        Some(&Event::Emit {
            message: "after retry".into()
        })
    );
}
#[test]
fn expansion_inside_loop_resets_inlined_facts_and_does_not_exit_caller_loop() {
    for callee in [
        "region identity(x: Bool) exits A, B { [Entry]: delta x -> [true => A,false => B]; [A]: exit A; [B]: exit B; }",
        "region identity(x: Bool) exits A, B { loop I() { [Entry]: delta x -> [true => A,false => B]; [A]: exit A; [B]: exit B; } }",
    ] {
        let p = checked(&format!(
            "{callee}\nregion x() exits Done {{ loop L(first: Bool = true) {{ [Entry]: delta identity(first) -> [A => A,B => B]; [A]: emit(\"first\"); again L(false); [B]: emit(\"second\"); exit Done; }} }}"
        ));
        let s = Scenario {
            entry: "x".into(),
            arguments: vec![],
            calls: vec![],
        };
        let result = p.run(&s).unwrap();
        assert_eq!(result.events.len(), 2);
        assert_eq!(result, p.expanded("x").unwrap().run(&s).unwrap());
    }
}
#[test]
fn recursion_hidden_in_loop_is_rejected() {
    let source =
        "region x() exits Done { loop L() { [Entry]: delta x() -> [Done => D]; [D]: exit Done; } }";
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_RECURSION");
}
