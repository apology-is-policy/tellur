//! Runtime readiness regressions promoted from the executable protocol model.
//! Harness cuts invoke internal safe points; public source execution stays sequential.
use super::*;

fn program() -> CheckedProgram {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../../playground/manifest.json")).unwrap();
    config["primitives"].as_array_mut().unwrap().push(serde_json::json!({
        "name":"sample","parameters":[],"outcomes":[{"name":"ready","fields":[{"name":"text","type":"string"}]}],"effects":["sample"],"returns":true
    }));
    crate::compile(
        r#"
async control leaf(ref mutable string text)
    exits ready { ref mutable string value from text }
{
    emit("waiting");
    emit(text);
    delta sample() {
        ready { string next } => received
    }
    [received]: deref text = move next;
        emit(text);
        exit ready { text };
}
async control start(string initial) exits done {
    mutable string text = move initial;
    auto pending = defer leaf(borrow mutable text);
    delta await pending {
        ready { auto value } => received
    }
    [received]: emit(value);
        exit done;
}
control main() exits done {
    forward run start("unused");
}
"#,
        Manifest::from_json(&config.to_string()).unwrap(),
    )
    .unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: ["first", "second"]
            .into_iter()
            .map(|text| ScriptCall {
                operation: "sample".into(),
                outcome: "ready".into(),
                fields: vec![Value::String(text.into())],
            })
            .collect(),
    }
}
fn messages<'a>(m: &'a Machine<'_>) -> Vec<&'a str> {
    m.shared
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect()
}
fn cut_after_messages(m: &mut Machine<'_>, count: usize) {
    while messages(m).len() < count {
        assert!(
            !m.drive(1),
            "checked chain completed before modeled I/O cut"
        );
    }
}
fn retired(m: &Machine<'_>) {
    assert!(m.shared.waiting.is_empty());
    assert!(m.context.task.is_none());
    assert!(m.shared.values.is_empty());
    assert!(m.shared.tasks.is_empty());
    assert!(m.shared.ready.is_empty());
    assert!(m.shared.suspended.calls.is_empty());
    assert!(m.context.stack.is_empty());
    assert!(m.context.frame.owner.is_none());
}

#[test]
fn repeated_readiness_preserves_borrowers_and_never_delivers_task_completion() {
    for reverse in [false, true] {
        let p = program();
        let s = scenario();
        let mut session = p.start_execution(&s, 200);
        let m = session.machine.as_mut().unwrap();
        m.call("start", vec![Value::String("left".into())]).unwrap();
        cut_after_messages(m, 1);
        let independent = Context::new(m.context.activations.child());
        let left = m.park_for_readiness().unwrap();
        m.context = independent;
        m.call("start", vec![Value::String("right".into())])
            .unwrap();
        cut_after_messages(m, 2);
        let right = m.park_for_readiness().unwrap();
        assert_ne!(left, right);
        assert_eq!(m.shared.suspended.calls.len(), 2);
        assert_eq!(m.shared.cursor, 0);
        let (first, second) = if reverse {
            (right, left)
        } else {
            (left, right)
        };
        let (first_name, second_name) = if reverse {
            ("right", "left")
        } else {
            ("left", "right")
        };
        let counts = m.shared.tasks.live_counts();
        let values = m.shared.values.clone();
        let events = m.shared.events.clone();
        let fuel = m.shared.steps_left;
        m.signal_readiness(&first).unwrap();
        let registry = m.shared.tasks.clone();
        assert!(m.signal_readiness(&first).is_err());
        assert_eq!(m.shared.tasks, registry);
        // Readiness changes only eligibility, never completion, storage or fuel.
        assert_eq!(m.shared.tasks.live_counts(), counts);
        assert_eq!(m.shared.values, values);
        assert_eq!(m.shared.events, events);
        assert_eq!(m.shared.steps_left, fuel);
        m.dispatch_ready().unwrap();
        assert!(!m.drive(0));
        assert_eq!(m.shared.events, events);
        cut_after_messages(m, 4);
        assert_eq!(m.shared.cursor, 1);
        let again = m.park_for_readiness().unwrap();
        assert_ne!(again, first);
        assert_ne!(again, second);
        assert!(
            m.signal_readiness(&first).is_err(),
            "old registration cannot wake a later wait"
        );
        assert_eq!(
            m.shared.suspended.calls.len(),
            2,
            "readiness is not completion"
        );
        m.signal_readiness(&second).unwrap();
        m.dispatch_ready().unwrap();
        // Wakes may queue another context; they cannot run it inside this stretch.
        m.signal_readiness(&again).unwrap();
        assert!(m.dispatch_ready().is_err());
        assert!(m.drive(100));
        assert_eq!(m.shared.cursor, 2);
        assert_eq!(m.shared.suspended.calls.len(), 1);
        assert_eq!(
            messages(m),
            [
                "waiting",
                "waiting",
                first_name,
                "first",
                second_name,
                "second",
                "second"
            ]
        );
        m.dispatch_ready().unwrap();
        assert!(m.drive(100));
        assert_eq!(
            messages(m),
            [
                "waiting",
                "waiting",
                first_name,
                "first",
                second_name,
                "second",
                "second",
                "first"
            ]
        );
        assert_eq!(
            m.shared.cursor, 2,
            "script input cannot replay on resumption"
        );
        assert!(m.signal_readiness(&again).is_err());
        assert!(m.signal_readiness(&second).is_err());
        assert!(m.dispatch_ready().is_err());
        retired(m);
    }
}

#[test]
fn readiness_exhaustion_keeps_the_active_checked_chain_and_loans_intact() {
    let p = program();
    let mut s = scenario();
    s.calls.truncate(1);
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    cut_after_messages(m, 1);
    m.shared.tasks.exhaust_readiness_space();
    let registry = m.shared.tasks.clone();
    let values = m.shared.values.clone();
    let owner = m.context.frame.owner;
    let depth = m.context.stack.len();
    let fuel = m.shared.steps_left;
    assert!(m.park_for_readiness().is_err());
    assert!(m.shared.waiting.is_empty());
    assert_eq!(m.shared.tasks, registry);
    assert_eq!(m.shared.values, values);
    assert_eq!(m.context.frame.owner, owner);
    assert_eq!(m.context.stack.len(), depth);
    assert_eq!(m.shared.steps_left, fuel);
    assert_eq!(m.shared.cursor, 0);
    assert!(m.drive(100));
    assert_eq!(messages(m), ["waiting", "unused", "first", "first"]);
    assert!(
        m.park_for_readiness().is_err(),
        "completed chains cannot register readiness"
    );
    retired(m);
}

#[test]
fn readiness_from_another_execution_cannot_wake_a_matching_local_generation() {
    let p = program();
    let mut s = scenario();
    s.calls.truncate(1);
    let mut first_session = p.start_execution(&s, 200);
    let mut second_session = p.start_execution(&s, 200);
    let first = first_session.machine.as_mut().unwrap();
    let second = second_session.machine.as_mut().unwrap();
    first.enter(&s).unwrap();
    second.enter(&s).unwrap();
    cut_after_messages(first, 1);
    cut_after_messages(second, 1);
    let stale = first.park_for_readiness().unwrap();
    let current = second.park_for_readiness().unwrap();
    // These are the first registrations in otherwise identical executions.
    assert_ne!(stale, current);
    let values = second.shared.values.clone();
    let fuel = second.shared.steps_left;
    assert!(second.signal_readiness(&stale).is_err());
    assert_eq!(second.shared.waiting.len(), 1);
    assert!(second.shared.ready.is_empty());
    assert_eq!(second.shared.values, values);
    assert_eq!(second.shared.steps_left, fuel);
    first.signal_readiness(&stale).unwrap();
    first.dispatch_ready().unwrap();
    assert!(first.drive(100));
    retired(first);
    drop(first_session); // The token retains its identity after the execution retires.
    assert!(second.signal_readiness(&stale).is_err());
    second.signal_readiness(&current).unwrap();
    second.dispatch_ready().unwrap();
    assert!(second.drive(100));
    retired(second);
}

#[test]
fn missing_wait_storage_does_not_consume_readiness_or_queue_a_context() {
    let p = program();
    let mut s = scenario();
    s.calls.truncate(1);
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    assert!(
        m.park_for_readiness().is_err(),
        "entry is not an admitted child task"
    );
    cut_after_messages(m, 1);
    let (task, _) = m.context.task.unwrap();
    let token = m.park_for_readiness().unwrap();
    let held = m.shared.waiting.remove(&task).unwrap();
    let before = m.shared.tasks.clone();
    assert!(m.signal_readiness(&token).is_err());
    assert_eq!(m.shared.tasks, before);
    assert!(m.shared.ready.is_empty());
    m.shared.waiting.insert(task, held);
    m.signal_readiness(&token).unwrap();
    m.dispatch_ready().unwrap();
    assert!(m.drive(100));
    assert_eq!(messages(m), ["waiting", "unused", "first", "first"]);
    retired(m);
}

#[test]
fn fault_after_resumption_retires_its_chain_and_preserves_an_independent_waiter() {
    let p = crate::compile(
        r#"
control fail(ref mutable string text) faults {
    emit(text);
    fault "after readiness";
}
async control leaf(ref mutable string text) faults {
    fail(text);
}
async control start(string initial) faults {
    mutable string text = move initial;
    auto pending = defer leaf(borrow mutable text);
    await pending;
}
control main() faults {
    run start("unused");
}
"#,
        Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    let mut s = scenario();
    s.calls.clear();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.call("start", vec![Value::String("first".into())])
        .unwrap();
    cut_after_messages(m, 1);
    let independent = Context::new(m.context.activations.child());
    let first_slots: BTreeSet<_> = m.shared.values.keys().copied().collect();
    let first = m.park_for_readiness().unwrap();
    m.context = independent;
    m.call("start", vec![Value::String("second".into())])
        .unwrap();
    cut_after_messages(m, 2);
    let second = m.park_for_readiness().unwrap();
    let second_values: BTreeMap<_, _> = m
        .shared
        .values
        .iter()
        .filter(|(id, _)| !first_slots.contains(id))
        .map(|(id, v)| (*id, v.clone()))
        .collect();
    m.signal_readiness(&first).unwrap();
    m.dispatch_ready().unwrap();
    assert!(m.drive(100));
    assert!(matches!(&m.context.flow, Some(Err(d)) if d.message.contains("after readiness")));
    assert_eq!(m.shared.values, second_values);
    assert_eq!(m.shared.suspended.calls.len(), 1);
    assert_eq!(m.shared.waiting.len(), 1);
    assert!(m.signal_readiness(&first).is_err());
    m.signal_readiness(&second).unwrap();
    m.dispatch_ready().unwrap();
    assert!(m.drive(100));
    assert!(matches!(&m.context.flow, Some(Err(d)) if d.message.contains("after readiness")));
    assert_eq!(messages(m), ["first", "second"]);
    retired(m);
}
