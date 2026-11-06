//! E1g.1: manifest async host operations, admitted by direct `await` only and
//! executed by the reference interpreter with scripted readiness.
use tellur::{
    CheckedProgram, compile,
    ir::{Operation, Value},
    manifest::Manifest,
    runtime::{Event, ExecutionProgress, RunCompletion, Scenario, ScriptCall},
};

fn manifest_json(extra: serde_json::Value) -> String {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../playground/manifest.json")).unwrap();
    let primitives = config["primitives"].as_array_mut().unwrap();
    primitives.push(serde_json::json!({
        "name": "tick", "async": true,
        "parameters": [{"name": "label", "type": "string", "mode": "consume"}],
        "outcomes": [{"name": "ticked", "fields": [{"name": "label", "type": "string"}]}],
        "effects": ["clock.tick"], "returns": true
    }));
    if let Some(extra) = extra.as_array() {
        primitives.extend(extra.iter().cloned());
    }
    config.to_string()
}
fn manifest() -> Manifest {
    Manifest::from_json(&manifest_json(serde_json::json!([]))).unwrap()
}
fn checked(source: &str) -> CheckedProgram {
    compile(source, manifest()).unwrap_or_else(|e| panic!("{e}\n{source}"))
}
fn rejected(source: &str) -> tellur::Diagnostic {
    compile(source, manifest()).expect_err(source)
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>, waits: u32) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits,
    }
}
fn tick(label: &str, waits: u32) -> ScriptCall {
    call("tick", "ticked", vec![Value::String(label.into())], waits)
}
fn scenario(calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls,
    }
}
fn trace(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            Event::Emit { message } => message.clone(),
            Event::Primitive {
                operation, fields, ..
            } => match fields.as_slice() {
                [Value::String(text)] => format!("{operation}:{text}"),
                _ => operation.clone(),
            },
            other => format!("{other:?}"),
        })
        .collect()
}

const CONCURRENT: &str = r#"
async control step(string label) exits done {
    emit(borrow label);
    delta await tick(move label) {
        ticked { string result } => ticked
    }

    [ticked]:
        emit(result);
        exit done;
}
async control startup() exits done {
    auto first = spawn step("one");
    auto second = spawn step("two");
    emit("queued");
    await first;
    emit("first joined");
    await second;
    exit done;
}
control main() exits done {
    forward run startup();
}
"#;

#[test]
fn awaited_host_operations_complete_in_scripted_readiness_order() {
    let p = checked(CONCURRENT);
    let wrapper = p.region("tick$host").expect("generated host control");
    assert!(wrapper.asynchronous);
    let cases = [
        (
            (0, 0),
            vec![
                "queued",
                "one",
                "two",
                "tick:one",
                "one",
                "tick:two",
                "two",
                "first joined",
            ],
        ),
        (
            (2, 0),
            vec![
                "queued",
                "one",
                "two",
                "tick:two",
                "two",
                "tick:one",
                "one",
                "first joined",
            ],
        ),
        (
            (1, 1),
            vec![
                "queued",
                "one",
                "two",
                "tick:one",
                "one",
                "first joined",
                "tick:two",
                "two",
            ],
        ),
        (
            (1, 3),
            vec![
                "queued",
                "one",
                "two",
                "tick:one",
                "one",
                "first joined",
                "tick:two",
                "two",
            ],
        ),
    ];
    for ((first, second), expected) in cases {
        let s = scenario(vec![tick("unused", first), tick("unused", second)]);
        // Scripted completions are bound in preparation order; events publish at completion.
        let s = Scenario {
            calls: vec![tick("one", first), tick("two", second)],
            ..s
        };
        let result = p.run(&s).unwrap();
        assert_eq!(result.outcome, "done");
        assert_eq!(trace(&result.events), expected, "waits {first}/{second}");
        let expanded = p.expanded("main").unwrap().run(&s).unwrap();
        assert_eq!(expanded.events, result.events);
    }
}

#[test]
fn pending_host_operations_preserve_fuel_and_quota_results() {
    let p = checked(CONCURRENT);
    let s = scenario(vec![tick("one", 2), tick("two", 1)]);
    let expected = p.run(&s).unwrap();
    let mut completed = false;
    for fuel in 0..200 {
        let reference = p.run_completion(&s, fuel);
        for quota in [1, 2, 7] {
            let mut execution = p.start_execution(&s, fuel);
            let result = loop {
                match execution.advance(quota) {
                    ExecutionProgress::Paused => {}
                    ExecutionProgress::Complete(result) => break result,
                    ExecutionProgress::Finished => unreachable!(),
                }
            };
            match (&reference, &result) {
                (Ok(RunCompletion::Outcome(a)), Ok(RunCompletion::Outcome(b))) => {
                    assert_eq!(a.events, b.events)
                }
                (Err(a), Err(b)) => {
                    assert_eq!(a.diagnostic, b.diagnostic);
                    assert_eq!(a.events, b.events);
                }
                _ => panic!("quota {quota} changed the result at fuel {fuel}"),
            }
        }
        if let Ok(RunCompletion::Outcome(result)) = reference {
            assert_eq!(result.events, expected.events);
            completed = true;
            break;
        }
    }
    assert!(completed);
}

#[test]
fn sequential_awaits_and_borrowed_resources_complete_once() {
    let source = r#"
async control fetch(ref handle input) exits got { bytes data }, ended {} {
    delta await receive(input) {
        data { bytes data } => received,
        closed => ended,
        failed { read_error error } => broken
    }

    [received]: exit got { data };
    [ended | broken]: exit ended;
}
control main() exits done, missing {
    delta open("input") {
        opened { handle input } => ready,
        not_found => absent,
        denied => blocked
    }

    [absent | blocked]: exit missing;
    [ready]:
        delta run fetch(borrow input) {
            got { bytes data } => fetched,
            ended => ended
        }
    [ready]:
        delta close(move input) {
            closed => closed,
            failed { close_error error } => failed
        }
    [ready]: exit done;
}
"#;
    let p = checked(source);
    let handle = Value::Opaque {
        ty: "handle".into(),
        token: "h1".into(),
    };
    for waits in [0, 3] {
        let s = scenario(vec![
            call("open", "opened", vec![handle.clone()], 0),
            call(
                "receive",
                "data",
                vec![Value::Bytes(b"body".to_vec())],
                waits,
            ),
            call("close", "closed", vec![], 0),
        ]);
        let result = p.run(&s).unwrap();
        assert_eq!(result.outcome, "done");
        let receive = result
            .events
            .iter()
            .filter(|e| matches!(e, Event::Primitive { operation, .. } if operation == "receive"))
            .collect::<Vec<_>>();
        assert_eq!(receive.len(), 1);
        let Event::Primitive { arguments, .. } = receive[0] else {
            unreachable!()
        };
        assert_eq!(arguments, &vec![handle.clone()]);
    }
}

#[test]
fn owning_faults_stop_pending_host_operations_without_events() {
    let source = r#"
async control step(string label) exits done {
    delta await tick(move label) {
        ticked { string result } => ticked
    }

    [ticked]:
        emit(result);
        exit done;
}
async control failing() exits done faults {
    emit("failing");
    yield;
    yield;
    yield;
    fault "sibling failed";
}
async control startup() exits done faults {
    auto pending = spawn step("one");
    auto failure = spawn failing();
    await pending;
    await failure;
    exit done;
}
control main() exits done faults {
    forward run startup();
}
"#;
    let p = checked(source);
    // Still pending at the sibling fault: the operation stops and publishes nothing.
    let s = scenario(vec![tick("one", 9)]);
    let failure = p.run_detailed(&s, 10_000).unwrap_err();
    assert!(
        failure.diagnostic.message.contains("sibling failed"),
        "{failure:?}"
    );
    assert_eq!(trace(&failure.events), vec!["failing"]);
    // Awaiting is a FIFO handoff: the ready operation runs after the queued sibling.
    let s = scenario(vec![tick("one", 0)]);
    let failure = p.run_detailed(&s, 10_000).unwrap_err();
    assert!(
        failure.diagnostic.message.contains("sibling failed"),
        "{failure:?}"
    );
    assert_eq!(trace(&failure.events), vec!["failing", "tick:one", "one"]);
}

#[test]
fn script_errors_name_the_host_operation() {
    let p = checked(CONCURRENT);
    let missing = p.run(&scenario(vec![tick("one", 1)])).unwrap_err();
    assert!(
        missing.message.contains("script exhausted before `tick`"),
        "{missing}"
    );
    let sync = checked("control main() exits done { emit(\"x\"); exit done; }");
    let wrong = checked(
        r#"control main() exits done {
    delta open("x") {
        opened { handle h } => opened,
        not_found => absent,
        denied => blocked
    }
    [opened]: delta close(move h) { closed => closed, failed { close_error e } => failed }
    [opened]: exit done;
    [absent | blocked]: exit done;
}"#,
    );
    drop(sync);
    let error = wrong
        .run(&scenario(vec![call("open", "not_found", vec![], 1)]))
        .unwrap_err();
    assert!(
        error
            .message
            .contains("scripted waits require an async host operation"),
        "{error}"
    );
}

#[test]
fn host_operations_are_admitted_only_by_direct_await() {
    for (body, message) in [
        (
            "delta tick(\"x\") { ticked { string label } => ticked }",
            "async host operation `tick` must be invoked with await inside an async control",
        ),
        (
            "delta run tick(\"x\") { ticked { string label } => ticked }",
            "run of async host operation `tick` is not yet admitted",
        ),
        (
            "auto pending = defer tick(\"x\");",
            "defer and spawn of async host operation `tick` are not yet admitted",
        ),
        (
            "auto pending = spawn tick(\"x\");",
            "defer and spawn of async host operation `tick` are not yet admitted",
        ),
    ] {
        let source =
            format!("async control work() exits done {{\n    {body}\n    exit done;\n}}\n");
        let error = rejected(&source);
        assert_eq!(error.code, "E_ASYNC", "{source}");
        assert!(
            error.message.contains(message),
            "{}\n{source}",
            error.message
        );
        assert_eq!(error.span.line, 2, "{source}");
    }
    let error = rejected(
        "control work() exits done {\n    delta await tick(\"x\") { ticked { string label } => ticked }\n    exit done;\n}\n",
    );
    assert_eq!(error.code, "E_ASYNC");
    assert!(error.message.contains("require an async control"));
}

#[test]
fn unused_host_operations_add_no_controls_and_wrappers_are_not_entries() {
    let p = checked("control main() exits done { exit done; }");
    assert!(p.region("tick$host").is_none());
    let p = checked(CONCURRENT);
    let mut s = scenario(vec![]);
    s.entry = "tick$host".into();
    assert!(p.run(&s).is_err());
    // Callers await the generated control; its name cannot be written in source.
    let p = checked(
        "async control work() exits auto {\n    forward await tick(\"x\");\n}\ncontrol main() exits done { exit done; }\n",
    );
    let work = p.region("work").unwrap();
    let awaited = work
        .code
        .iter()
        .find_map(|i| match &i.action {
            tellur::ir::Action::Decide {
                operation: Operation::Await(name),
                ..
            } => Some(name.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(awaited, "tick$host");
}

#[test]
fn native_builds_reject_async_host_operations_explicitly() {
    let p = checked(CONCURRENT);
    let error = p.native_source("main").unwrap_err();
    assert_eq!(error.code, "E_BACKEND");
    assert!(
        error
            .message
            .contains("native builds do not yet support async host operation `tick`"),
        "{error}"
    );
    // An unused declaration keeps native generation available.
    let p = checked(
        "outcome program_result {\n    @exit 0 done\n}\ncontrol main() exits program_result {\n    exit done;\n}\n",
    );
    p.native_source("main").unwrap();
}

#[test]
fn manifest_validation_limits_the_first_async_profile() {
    let cases = [
        (
            serde_json::json!({"name":"bad","async":true,"parameters":[{"name":"text","type":"string","mode":"borrow"}],"outcomes":[{"name":"done","fields":[]}],"effects":[],"returns":true}),
            "parameter `text` borrows data",
        ),
        (
            serde_json::json!({"name":"bad","async":true,"parameters":[{"name":"h","type":"handle","mode":"consume"}],"outcomes":[{"name":"done","fields":[]}],"effects":[],"returns":true}),
            "parameter `h` consumes a resource",
        ),
        (
            serde_json::json!({"name":"bad","async":true,"parameters":[{"name":"text","type":"string","mode":"value"}],"outcomes":[{"name":"done","fields":[]}],"effects":[],"returns":true}),
            "parameter `text` passes non-Copy data by value",
        ),
        (
            serde_json::json!({"name":"bad","async":true,"parameters":[],"outcomes":[{"name":"opened","fields":[{"name":"h","type":"handle"}]}],"effects":[],"returns":true}),
            "outcome field `opened.h` must be owned data",
        ),
        (
            serde_json::json!({"name":"bad","async":true,"contingent":true,"parameters":[],"outcomes":[{"name":"done","fields":[]}],"effects":[],"returns":true}),
            "cannot be pure or contingent",
        ),
    ];
    for (primitive, message) in cases {
        let error =
            Manifest::from_json(&manifest_json(serde_json::json!([primitive]))).expect_err(message);
        assert!(error.message.contains(message), "{}", error.message);
        assert!(
            error.message.contains("async host operation `bad`"),
            "{}",
            error.message
        );
    }
}
