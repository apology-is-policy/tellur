//! Integrated obligation retirement; these are sequential executions, not a scheduler.
use super::*;

fn checked(source: &str) -> CheckedProgram {
    crate::compile(
        source,
        Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
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
fn assert_retired(m: &Machine<'_>) {
    assert!(m.shared.tasks.is_empty());
    assert!(m.shared.values.is_empty());
    assert!(m.context.stack.is_empty());
    assert!(m.context.frame.owner.is_none());
}
fn messages<'a>(m: &'a Machine<'_>) -> Vec<&'a str> {
    m.shared
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn moved_cold_handles_keep_their_invocation_across_normal_lexical_boundaries() {
    let p = checked(
        r#"
async control work(string text) exits done { emit(borrow text); exit done; }
async control start() exits done {
    auto outer = defer work("outer");
    auto moved = move outer;
    fault boundary {
        auto inner = defer work("inner");
        await inner;
    } contingency [] { escalate; }
    fault boundary { await moved; } contingency [] { escalate; }
    exit done;
}
control main() exits done { forward run start(); }
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    let mut peak_tasks = 0;
    while !m.drive(1) {
        let (owners, tasks) = m.shared.tasks.live_counts();
        assert!(owners <= 3);
        peak_tasks = peak_tasks.max(tasks);
    }
    assert_eq!(peak_tasks, 2);
    assert_eq!(messages(m), ["inner", "outer"]);
    assert!(matches!(m.context.flow, Some(Ok(Flow::Exit(ref n, _))) if n == "done"));
    assert_retired(m);
}

#[test]
fn nested_awaits_retire_children_without_retiring_the_callers_pending_work() {
    let p = checked(
        r#"
async control leaf(ref string text) exits ready { ref string text from text } {
    emit(text); exit ready {text};
}
async control relay(ref string input) exits ready { ref string value from input } {
    auto child = defer leaf(input);
    forward await child;
}
async control start() exits done {
    string text = "borrowed";
    auto first = defer relay(borrow text);
    auto second = defer leaf(borrow text);
    delta await first { ready {auto value} => first_ready }
    [first_ready]: drop(value);
        delta await second { ready {auto other} => second_ready }
    [second_ready]: emit(other); drop(other); drop(text); exit done;
}
control main() exits done { forward run start(); }
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 300);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    let mut peak = 0;
    while !m.drive(1) {
        peak = peak.max(m.shared.tasks.live_counts().1);
    }
    assert_eq!(peak, 3);
    assert_eq!(messages(m), ["borrowed", "borrowed", "borrowed"]);
    assert!(matches!(m.context.flow, Some(Ok(Flow::Exit(ref n, _))) if n == "done"));
    assert_retired(m);
}

#[test]
fn repeated_fault_recovery_retires_cold_and_running_tasks_without_normal_completion() {
    for body in ["fault \"before await\";", "await pending;"] {
        let p = checked(&format!(
            r#"
async control work() faults {{ emit("started"); fault "child fault"; }}
async control start() faults {{
    supervised loop attempts {{
        fault boundary {{
            auto pending = defer work();
            {body}
        }} contingency [] {{ recover again attempts; }}
    }}
}}
control main() faults {{ run start(); }}
"#
        ));
        let s = scenario();
        let mut session = p.start_execution(&s, 600);
        let m = session.machine.as_mut().unwrap();
        m.enter(&s).unwrap();
        let mut recoveries = 0;
        while !m.drive(1) {
            let (owners, tasks) = m.shared.tasks.live_counts();
            assert!(owners <= 3, "retired invocation identities accumulated");
            assert!(tasks <= 1, "retired task identities accumulated");
            if !m.context.contingencies.is_empty() {
                assert_eq!(tasks, 0, "tasks must retire before contingency");
                recoveries += 1;
            }
        }
        assert!(recoveries > 10);
        assert!(matches!(m.context.flow, Some(Err(ref d)) if d.code == "E_FUEL"));
        if body.starts_with("fault") {
            assert!(messages(m).is_empty());
        } else {
            assert!(messages(m).len() > 10);
        }
        assert_retired(m);
    }
}

#[test]
fn await_admission_failure_retires_the_started_obligation_and_caller() {
    let p = checked(
        r#"
async control work() exits done { emit("must not run"); exit done; }
async control start() exits done {
    auto pending = defer work(); emit("captured"); await pending; exit done;
}
control main() exits done { forward run start(); }
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    while m.shared.events.is_empty() {
        assert!(!m.drive(1));
    }
    assert_eq!(m.shared.tasks.live_counts(), (2, 1));
    m.shared.next_base = usize::MAX;
    assert!(m.drive(200));
    assert!(
        matches!(m.context.flow, Some(Err(ref d)) if d.message.contains("storage address space exhausted"))
    );
    assert_eq!(messages(m), ["captured"]);
    assert_retired(m);
}

#[test]
fn fault_retirement_in_one_context_preserves_another_invocations_cold_task() {
    let p = checked(
        r#"
async control work(string text) exits done { emit(borrow text); exit done; }
async control start(string text, bool broken) exits done {
    auto pending = defer work(text);
    emit("paused");
    delta broken { true => fault "isolated", false => ready }
    [ready]: await pending; exit done;
}
control main() exits done { forward run start("unused", false); }
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.call(
        "start",
        vec![Value::String("survives".into()), Value::Bool(false)],
    )
    .unwrap();
    while m.shared.events.is_empty() {
        assert!(!m.drive(1));
    }
    let live_values = m.shared.values.clone();
    let independent = Context::new(m.context.activations.child());
    let suspended = std::mem::replace(&mut m.context, independent);
    m.call(
        "start",
        vec![Value::String("abandoned".into()), Value::Bool(true)],
    )
    .unwrap();
    while m.shared.events.len() < 2 {
        assert!(!m.drive(1));
    }
    assert_eq!(m.shared.tasks.live_counts(), (2, 2));
    assert!(m.drive(100));
    assert!(matches!(m.context.flow, Some(Err(ref d)) if d.code == "E_FAULT"));
    assert_eq!(m.shared.tasks.live_counts(), (1, 1));
    assert_eq!(m.shared.values, live_values);
    // Independent harness contexts are not source siblings in a shared fault
    // boundary. The failed context is never resumed.
    m.context = suspended;
    assert!(m.drive(100));
    assert_eq!(messages(m), ["paused", "paused", "survives"]);
    assert_retired(m);
}
