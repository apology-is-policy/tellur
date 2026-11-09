//! Milestone 5 acceptance specimen (`playground/cancellation.tellur`): two
//! concurrent receives, cancellation of one, borrowed handles held across
//! suspension and a cleanup failure. Every schedule must reach a declared
//! completion, join every child, keep both loans until the join, and agree across
//! the reference executor, its certified expansion, small host quotas and a native
//! executable.
use std::{path::PathBuf, process::Command, sync::OnceLock};
use tellur::{
    CheckedProgram, compile, compile_file,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, ExecutionProgress, RunCompletion, Scenario, ScriptCall},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn program() -> CheckedProgram {
    compile_file(&root().join("playground/cancellation.tellur"), manifest()).unwrap()
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>, waits: u32) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits,
    }
}
fn handle(token: &str) -> Value {
    Value::Opaque {
        ty: "handle".into(),
        token: token.into(),
    }
}
fn opened(token: &str) -> ScriptCall {
    call("open", "opened", vec![handle(token)], 0)
}
fn data(text: &str, waits: u32) -> ScriptCall {
    call("receive", "data", vec![Value::Bytes(text.into())], waits)
}
fn decoded(text: &str) -> ScriptCall {
    call("decode_utf8", "text", vec![Value::String(text.into())], 0)
}
fn closed() -> ScriptCall {
    call("close", "closed", vec![], 0)
}
fn close_failed() -> ScriptCall {
    call(
        "close",
        "failed",
        vec![Value::Opaque {
            ty: "close_error".into(),
            token: "disk".into(),
        }],
        0,
    )
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
            Event::Primitive { operation, .. } => operation.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}
fn native() -> &'static PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("tellur-specimen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("specimen");
        program().build_native("main", &binary, 0).unwrap();
        binary
    })
}
/// Runs every executor; returns the outcome, its process status and the trace.
fn run(s: &Scenario) -> (String, Vec<String>) {
    let p = program();
    let result = p.run(s).unwrap_or_else(|e| panic!("{e}"));
    let expanded = p.expanded("main").unwrap().run(s).unwrap();
    assert_eq!(
        expanded.events, result.events,
        "expansion changed the history"
    );
    assert_eq!(expanded.outcome, result.outcome);
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
    let input = native().with_extension(format!("{:x}.json", fastrand(s)));
    std::fs::write(&input, serde_json::to_vec(s).unwrap()).unwrap();
    let output = Command::new(native())
        .arg("--scenario")
        .arg(&input)
        .output()
        .unwrap();
    let status = match result.outcome.as_str() {
        "answered" => 0,
        "unanswered" => 3,
        "unavailable" => 2,
        _ => 1,
    };
    assert_eq!(
        output.status.code(),
        Some(status),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let native: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        native,
        serde_json::to_value(&result).unwrap(),
        "native differs"
    );
    (result.outcome, trace(&result.events))
}
fn fastrand(s: &Scenario) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(s).unwrap().hash(&mut hasher);
    hasher.finish()
}

#[test]
fn the_slower_receive_is_cancelled_and_still_joined_before_both_handles_close() {
    // The backup is still pending when the primary answers: it stops without a
    // completion event and reports `stopped` through its own outcome.
    for waits in [1, 3, 9] {
        let (outcome, events) = run(&scenario(vec![
            opened("primary"),
            opened("backup"),
            data("hello", 0),
            data("late", waits),
            closed(),
            closed(),
            decoded("hello"),
        ]));
        assert_eq!(outcome, "answered");
        assert_eq!(
            events,
            [
                "open",
                "open",
                "receive",
                "The backup stopped.",
                "close",
                "close",
                "decode_utf8",
                "hello"
            ],
            "waits {waits}"
        );
    }
}

#[test]
fn a_receive_that_completed_first_wins_the_race_with_its_cancellation() {
    // Both receives complete before the request is observed; the backup's ordinary
    // outcome is delivered and handled.
    let (outcome, events) = run(&scenario(vec![
        opened("primary"),
        opened("backup"),
        data("hello", 0),
        data("late", 0),
        closed(),
        closed(),
        decoded("hello"),
    ]));
    assert_eq!(outcome, "answered");
    assert_eq!(
        events,
        [
            "open",
            "open",
            "receive",
            "receive",
            "The backup answered before its cancellation was observed.",
            "close",
            "close",
            "decode_utf8",
            "hello"
        ]
    );
}

#[test]
fn the_backup_answers_when_the_primary_ends_without_data() {
    let (outcome, events) = run(&scenario(vec![
        opened("primary"),
        opened("backup"),
        call("receive", "closed", vec![], 2),
        data("backup", 0),
        closed(),
        closed(),
        decoded("backup"),
    ]));
    assert_eq!(outcome, "answered");
    assert_eq!(
        events,
        [
            "open",
            "open",
            "receive",
            "receive",
            "close",
            "close",
            "decode_utf8",
            "backup"
        ]
    );
    let (outcome, _) = run(&scenario(vec![
        opened("primary"),
        opened("backup"),
        call("receive", "closed", vec![], 0),
        call("receive", "closed", vec![], 0),
        closed(),
        closed(),
    ]));
    assert_eq!(outcome, "unanswered");
}

#[test]
fn cleanup_failures_after_cancellation_stay_visible() {
    // Both closes are attempted after the join; any failure is the result.
    for (first, second) in [
        (close_failed(), closed()),
        (closed(), close_failed()),
        (close_failed(), close_failed()),
    ] {
        let (outcome, events) = run(&scenario(vec![
            opened("primary"),
            opened("backup"),
            data("hello", 0),
            data("late", 4),
            first,
            second,
        ]));
        assert_eq!(outcome, "cleanup_failed");
        assert_eq!(
            events,
            [
                "open",
                "open",
                "receive",
                "The backup stopped.",
                "close",
                "close",
                "Closing a handle failed."
            ]
        );
    }
}

#[test]
fn a_loan_cannot_end_before_its_borrower_is_joined() {
    let source = std::fs::read_to_string(root().join("playground/cancellation.tellur")).unwrap();
    let edit = |from: &str, to: &str| {
        assert_eq!(source.matches(from).count(), 1, "{from}");
        source.replacen(from, to, 1)
    };
    // Closing a handle while a started reader still borrows it is rejected.
    let consumed = edit(
        "    auto answer = capture await first_answer(borrow primary, borrow backup);\n",
        "    auto pending = spawn first_answer(borrow primary, borrow backup);\n    \
         delta close(primary) {\n        closed => early_closed,\n        \
         failed { close_error early_error } => early_failed\n    }\n    \
         auto answer = capture await pending;\n",
    );
    let error = compile(&consumed, manifest()).expect_err("closed while borrowed");
    assert_eq!(error.code, "E_LOAN", "{error:?}");
    // A cancelled child must still be joined.
    let abandoned = edit(
        "        cancel second;\n",
        "        cancel second;\n        exit answered { data };\n",
    );
    let error = compile(&abandoned, manifest()).expect_err("abandoned child");
    assert_eq!(error.code, "E_RESOURCE", "{error:?}");
    assert!(error.message.contains("`second`"), "{error:?}");
    // Diagnostics name the host operation, never its generated control.
    assert!(
        error.notes.iter().all(|n| !n.message.contains('$')),
        "{error:?}"
    );
}
