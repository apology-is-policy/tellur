//! E1e.1 cancellation: `cancel child;` and `cancel set;` request cancellation of
//! started work. A host operation entered by await or run observes it only where
//! its decision lists the `cancelled` alternative; joins never interrupt.
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    process::Command,
    sync::Mutex,
};
use tellur::{
    CheckedProgram, compile,
    ir::Value,
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
fn tick(label: &str, waits: u32) -> ScriptCall {
    ScriptCall {
        operation: "tick".into(),
        outcome: "ticked".into(),
        fields: vec![Value::String(label.into())],
        waits,
    }
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
/// One native executable per checked program, built on first use.
fn native(p: &CheckedProgram) -> PathBuf {
    static BUILT: Mutex<BTreeMap<u64, PathBuf>> = Mutex::new(BTreeMap::new());
    let codes = p
        .region("main")
        .unwrap()
        .exits
        .iter()
        .map(|o| (o.name.clone(), 0))
        .collect();
    let p = p.clone().with_entry_exit_codes("main", codes).unwrap();
    let mut hasher = DefaultHasher::new();
    p.native_source("main").unwrap().hash(&mut hasher);
    let key = hasher.finish();
    let mut built = BUILT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    built
        .entry(key)
        .or_insert_with(|| {
            let root = std::env::temp_dir().join(format!("tellur-cancel-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let binary = root.join(format!("program-{key:x}"));
            p.build_native("main", &binary, 0).unwrap();
            binary
        })
        .clone()
}
fn native_run(p: &CheckedProgram, s: &Scenario) -> serde_json::Value {
    let binary = native(p);
    let input = binary.with_extension(format!("{}.json", std::process::id()));
    std::fs::write(&input, serde_json::to_vec(s).unwrap()).unwrap();
    let output = Command::new(&binary)
        .arg("--scenario")
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
/// Runs the reference executor, its expansion, every small host quota and the
/// native executable; all must agree.
fn run(p: &CheckedProgram, s: &Scenario) -> Vec<String> {
    let result = p.run(s).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(result.outcome, "done");
    assert_eq!(
        native_run(p, s),
        serde_json::to_value(&result).unwrap(),
        "native differs"
    );
    let expanded = p.expanded("main").unwrap().run(s).unwrap();
    assert_eq!(expanded.events, result.events);
    for quota in [1, 2, 5] {
        let mut execution = p.start_execution(s, 100_000);
        let paused = loop {
            match execution.advance(quota) {
                ExecutionProgress::Paused => {}
                ExecutionProgress::Complete(result) => break result,
                ExecutionProgress::Finished => unreachable!(),
            }
        };
        let Ok(RunCompletion::Outcome(paused)) = paused else {
            panic!("quota {quota} changed the result");
        };
        assert_eq!(paused.events, result.events, "quota {quota}");
    }
    trace(&result.events)
}

const WORKER: &str = r#"
async control worker(string label) exits done, stopped {
    emit(borrow label);
    delta await tick(move label) {
        ticked { string result } => ticked,
        cancelled => cancelled
    }

    [ticked]:
        emit(concat("got ", result));
        exit done;
    [cancelled]:
        emit("cancelled");
        exit stopped;
}
"#;
fn with_worker(rest: &str) -> String {
    format!("{WORKER}{rest}")
}
fn handle_program(yields: usize) -> String {
    with_worker(&format!(
        r#"
async control startup() exits done {{
    auto child = spawn worker("one");
    {}
    cancel child;
    cancel child;
    delta await child {{
        done => finished,
        stopped => halted
    }}

    [finished]:
        emit("finished");
        exit done;
    [halted]:
        emit("halted");
        exit done;
}}
control main() exits done {{
    forward run startup();
}}
"#,
        "yield;\n    ".repeat(yields)
    ))
}

#[test]
fn cancelling_a_child_stops_its_observing_host_operation() {
    // Pending when cancelled: the operation stops without publishing an event.
    let p = checked(&handle_program(3));
    for waits in [1, 2, 9] {
        assert_eq!(
            run(&p, &scenario(vec![tick("one", waits)])),
            ["one", "cancelled", "halted"],
            "waits {waits}"
        );
    }
    // Completed before the request: the ordinary outcome is delivered.
    assert_eq!(
        run(&p, &scenario(vec![tick("one", 0)])),
        ["one", "tick:one", "got one", "finished"]
    );
    // Cancelled before the operation starts: no script entry is consumed.
    let p = checked(&handle_program(1));
    assert_eq!(run(&p, &scenario(vec![])), ["one", "cancelled", "halted"]);
    let unconsumed = p.run(&scenario(vec![tick("one", 0)])).unwrap_err();
    assert!(unconsumed.message.contains("unconsumed"), "{unconsumed:?}");
}

#[test]
fn operations_without_the_alternative_and_joins_are_not_interrupted() {
    let p = checked(
        r#"
async control worker(string label) exits done {
    delta await tick(move label) {
        ticked { string result } => ticked
    }

    [ticked]:
        emit(result);
        yield;
        emit("after");
        exit done;
}
async control startup() exits done {
    auto child = spawn worker("one");
    yield;
    yield;
    yield;
    cancel child;
    await child;
    emit("joined");
    exit done;
}
control main() exits done {
    forward run startup();
}
"#,
    );
    assert_eq!(
        run(&p, &scenario(vec![tick("one", 2)])),
        ["tick:one", "one", "after", "joined"]
    );
}

#[test]
fn cancelling_a_set_flags_current_members_only() {
    let p = checked(&with_worker(
        r#"
async control startup() exits done {
    tasks<worker> active = tasks();
    spawn worker("a") into active;
    spawn worker("b") into active;
    yield;
    yield;
    yield;
    cancel active;
    spawn worker("late") into active;
    loop reap(tasks<worker> pending = move active) {
        delta await next pending {
            done => finished,
            stopped => halted,
            empty => drained
        }

        [finished]:
            emit("finished");
            again reap(pending);
        [halted]:
            emit("halted");
            again reap(pending);
        [drained]: exit done;
    }
}
control main() exits done {
    forward run startup();
}
"#,
    ));
    let events = run(
        &p,
        &scenario(vec![tick("a", 5), tick("b", 5), tick("late", 1)]),
    );
    assert_eq!(
        events,
        [
            "a",
            "b",
            "cancelled",
            "cancelled",
            "late",
            "halted",
            "halted",
            "tick:late",
            "got late",
            "finished"
        ]
    );
}

#[test]
fn cancellation_reaches_descendants_including_later_children() {
    let p = checked(&with_worker(
        r#"
async control middle() exits done, stopped {
    yield;
    yield;
    yield;
    auto grandchild = spawn worker("deep");
    delta await grandchild {
        done => finished,
        stopped => halted
    }

    [finished]: exit done;
    [halted]: exit stopped;
}
async control startup() exits done {
    auto child = spawn middle();
    yield;
    cancel child;
    delta await child {
        done => finished,
        stopped => halted
    }

    [finished]:
        emit("child finished");
        exit done;
    [halted]:
        emit("child halted");
        exit done;
}
control main() exits done {
    forward run startup();
}
"#,
    ));
    assert_eq!(
        run(&p, &scenario(vec![])),
        ["deep", "cancelled", "child halted"]
    );
}

#[test]
fn an_entry_run_may_list_the_alternative_but_has_no_cancelling_owner() {
    let p = checked(
        r#"
control main() exits done {
    delta run tick("direct") {
        ticked { string label } => ticked,
        cancelled => cancelled
    }

    [ticked]:
        emit(label);
        exit done;
    [cancelled]: exit done;
}
"#,
    );
    assert_eq!(
        run(&p, &scenario(vec![tick("direct", 2)])),
        ["tick:direct", "direct"]
    );
}

#[test]
fn cancellation_is_checked_where_it_is_written() {
    let cases = [
        (
            "control main() exits done {
    auto value = 1;
    cancel value;
    exit done;
}",
            "cancel requires an async control",
        ),
        (
            &with_worker(
                r#"async control startup() exits done {
    auto later = defer worker("one");
    cancel later;
    await later;
    exit done;
}
control main() exits done { forward run startup(); }"#,
            ),
            "deferred tasks have not started",
        ),
        (
            "async control startup() exits done {
    auto value = 1;
    cancel value;
    exit done;
}
control main() exits done { forward run startup(); }",
            "cancel requires a local started child",
        ),
        (
            &with_worker(
                r#"async control startup() exits done {
    delta await worker("one") {
        done => finished,
        stopped => halted,
        cancelled => cancelled
    }

    [finished | halted | cancelled]: exit done;
}
control main() exits done { forward run startup(); }"#,
            ),
            "unknown outcome `cancelled`",
        ),
    ];
    for (source, message) in cases {
        let error = rejected(source);
        assert!(
            error.message.contains(message),
            "{error:?}\nexpected {message}\n{source}"
        );
    }
    let error = Manifest::from_json(&manifest_json(serde_json::json!([{
        "name": "wait", "async": true, "parameters": [],
        "outcomes": [{"name": "cancelled", "fields": []}],
        "effects": [], "returns": true
    }])))
    .expect_err("reserved outcome");
    assert!(
        error.message.contains("reserved for cancellation"),
        "{error:?}"
    );
}

#[test]
fn a_cancelled_task_observes_operations_its_synchronous_helpers_run() {
    let p = checked(
        r#"
control helper() exits done, stopped {
    delta run tick("helper") {
        ticked { string label } => ticked,
        cancelled => cancelled
    }

    [ticked]:
        emit(label);
        exit done;
    [cancelled]:
        emit("helper cancelled");
        exit stopped;
}
async control work() exits done, stopped {
    forward helper();
}
async control startup() exits done {
    auto child = spawn work();
    yield;
    cancel child;
    delta await child {
        done => finished,
        stopped => halted
    }

    [finished]:
        emit("finished");
        exit done;
    [halted]:
        emit("halted");
        exit done;
}
control main() exits done {
    forward run startup();
}
"#,
    );
    assert_eq!(
        run(&p, &scenario(vec![tick("helper", 3)])),
        ["helper cancelled", "halted"]
    );
    assert_eq!(
        run(&p, &scenario(vec![tick("helper", 0)])),
        ["tick:helper", "helper", "finished"]
    );
}
