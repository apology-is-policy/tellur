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
    assert_eq!(m.shared.domains.count(), 0);
    assert!(m.context.domain.is_none());
    assert!(m.shared.ready.is_empty());
    assert!(m.shared.waiting.is_empty());
    assert!(m.shared.suspended.calls.is_empty());
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
    let mut parked_peak = 0;
    while !m.drive(1) {
        parked_peak = parked_peak.max(m.shared.suspended.calls.len());
        peak = peak.max(m.shared.tasks.live_counts().1);
    }
    assert_eq!(peak, 3);
    assert_eq!(parked_peak, 2);
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
            assert!(
                m.shared.domains.count() <= 2,
                "retired fault domains accumulated"
            );
            assert!(owners <= 3, "retired invocation identities accumulated");
            assert!(tasks <= 1, "retired task identities accumulated");
            if !m.context.contingencies.is_empty() {
                assert_eq!(tasks, 0, "tasks must retire before contingency");
                assert!(
                    m.shared.suspended.calls.is_empty(),
                    "callers must unwind before contingency"
                );
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

#[test]
fn queued_await_parks_the_original_caller_and_delivers_its_borrow_once() {
    let p = checked(
        r#"
async control edit(ref mutable string text)
    exits ready { ref mutable string value from text }
{
    emit("child entered"); deref text = "after"; exit ready {text};
}
async control start() exits done {
    mutable string text = "before";
    auto pending = defer edit(borrow mutable text);
    emit("created");
    delta await pending {ready {auto value} => received}
    [received]: emit(value); drop(value); text = "released"; emit(borrow text); exit done;
}
control main() exits done {forward run start();}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    while m.shared.events.len() < 2 {
        assert!(!m.drive(1));
    }
    let (task, wait, caller) = m
        .context
        .stack
        .iter()
        .find_map(|c| match c {
            Continuation::ResumeAwait { task, wait, caller } => Some((*task, *wait, *caller)),
            _ => None,
        })
        .expect("await must retain a separate parked caller");
    let suspension = caller;
    let caller = m.shared.suspended.get(suspension, task, wait).unwrap();
    assert_ne!(caller.frame.owner, m.context.frame.owner);
    assert!(matches!(
        caller.stack.last(),
        Some(Continuation::TaskDecision { .. })
    ));
    let owner = caller.frame.owner.unwrap();
    let address = caller.frame.base
        + p.region("start")
            .unwrap()
            .slots
            .values()
            .find(|slot| slot.name == "text")
            .unwrap()
            .id;
    assert_eq!(m.shared.values[&address], Value::String("before".into()));
    let registry = m.shared.tasks.clone();
    assert!(m.shared.tasks.deliver(task, owner, wait).is_err());
    assert_eq!(
        m.shared.tasks, registry,
        "early signal must not resume caller"
    );
    assert!(m.drive(200));
    assert_eq!(
        messages(m),
        ["created", "child entered", "after", "released"]
    );
    assert!(m.shared.tasks.deliver(task, owner, wait).is_err());
    assert!(m.shared.suspended.take(suspension, task, wait).is_err());
    assert_retired(m);
}

#[test]
fn independent_parked_callers_reject_crossed_stale_and_exhausted_registrations() {
    // Independent harness contexts, not source siblings. Each executes checked
    // borrowed awaits; selecting one must not consume the other's parked state.
    let p = checked(
        r#"
async control leaf(ref string value) exits found { ref string value from value } {
    emit(value); exit found {value};
}
async control start(string text) exits done {
    auto pending = defer leaf(borrow text);
    delta await pending {found {auto result} => ready}
    [ready]: emit(result); exit done;
}
control main() exits done {forward run start("unused");}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.call("start", vec![Value::String("left".into())]).unwrap();
    while m.shared.suspended.calls.is_empty() {
        assert!(!m.drive(1));
    }
    let independent = Context::new(m.context.activations.child());
    let left = std::mem::replace(&mut m.context, independent);
    m.call("start", vec![Value::String("right".into())])
        .unwrap();
    while m.shared.suspended.calls.len() < 2 {
        assert!(!m.drive(1));
    }
    let parked: Vec<_> = m
        .shared
        .suspended
        .calls
        .iter()
        .map(|(id, call)| (*id, call.task, call.wait))
        .collect();
    let [
        (left_id, left_task, left_wait),
        (right_id, right_task, right_wait),
    ] = parked[..]
    else {
        panic!("both checked callers must be parked");
    };
    let values = m.shared.values.clone();
    let events = m.shared.events.clone();
    let tasks = m.shared.tasks.clone();
    let fuel = m.shared.steps_left;
    assert!(
        m.shared
            .suspended
            .take(left_id, right_task, left_wait)
            .is_err()
    );
    assert!(
        m.shared
            .suspended
            .take(left_id, left_task, right_wait)
            .is_err()
    );
    assert!(
        m.shared
            .suspended
            .take(right_id, left_task, left_wait)
            .is_err()
    );
    m.shared.suspended.next = u64::MAX;
    assert!(m.shared.suspended.prepare().is_err());
    assert_eq!(m.shared.suspended.calls.len(), 2);
    assert_eq!(m.shared.values, values);
    assert_eq!(m.shared.events, events);
    assert_eq!(m.shared.tasks, tasks);
    assert_eq!(m.shared.steps_left, fuel);
    assert!(m.drive(100));
    assert!(
        m.shared
            .suspended
            .take(right_id, right_task, right_wait)
            .is_err()
    );
    assert!(
        m.shared
            .suspended
            .get(left_id, left_task, left_wait)
            .is_ok()
    );
    assert_eq!(m.shared.suspended.calls.len(), 1);
    m.context = left;
    assert!(m.drive(100));
    assert_eq!(messages(m), ["right", "right", "left", "left"]);
    assert_retired(m);
}

#[test]
fn suspension_exhaustion_unwinds_before_queueing_or_running_the_child() {
    let p = checked(
        r#"
async control work(string text) exits done {emit(borrow text); exit done;}
async control start() exits done {
    auto pending = defer work("must not run");
    emit("captured"); await pending; exit done;
}
control main() exits done {forward run start();}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    while m.shared.events.is_empty() {
        assert!(!m.drive(1));
    }
    m.shared.suspended.next = u64::MAX;
    assert!(m.drive(200));
    assert!(
        matches!(m.context.flow, Some(Err(ref d)) if d.message.contains("suspension identity space exhausted"))
    );
    assert_eq!(messages(m), ["captured"]);
    assert_retired(m);
}

#[test]
fn queued_contexts_dispatch_independently_without_preempting_or_replaying_storage() {
    // This white-box harness separates admission from its administrative dispatch.
    // Public host quota pauses still drain Dispatch; they cannot make this split.
    fn queue_next_await(m: &mut Machine<'_>) {
        let Continuation::Block { code, next } = m.context.stack.pop().unwrap() else {
            panic!("expected the checked await instruction");
        };
        let instruction = &code[next];
        assert!(matches!(
            instruction.action,
            Action::Decide {
                operation: Operation::AwaitTask(_),
                ..
            }
        ));
        assert!(instruction.guard.evaluate(&m.context.frame.facts));
        m.context.activations.at(instruction.id);
        m.shared.steps_left -= 1;
        m.context.stack.push(Continuation::Block {
            code,
            next: next + 1,
        });
        assert!(matches!(
            m.instruction(instruction).unwrap(),
            Flow::Fallthrough
        ));
        assert!(matches!(
            m.context.stack.pop(),
            Some(Continuation::Dispatch)
        ));
    }
    let p = checked(
        r#"
async control leaf(ref mutable string text)
    exits ready { ref mutable string value from text }
{
    emit(text); deref text = "edited"; exit ready {text};
}
async control start(string initial) exits done {
    mutable string text = move initial;
    auto pending = defer leaf(borrow mutable text);
    emit("captured");
    delta await pending {ready {auto value} => received}
    [received]: emit(value); exit done;
}
control main() exits done {forward run start("unused");}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.call("start", vec![Value::String("left".into())]).unwrap();
    while m.shared.events.is_empty() {
        assert!(!m.drive(1));
    }
    let independent = Context::new(m.context.activations.child());
    queue_next_await(m);
    m.context = independent;
    m.call("start", vec![Value::String("right".into())])
        .unwrap();
    while m.shared.events.len() < 2 {
        assert!(!m.drive(1));
    }
    queue_next_await(m);
    assert_eq!(m.shared.ready.len(), 2);
    assert_eq!(m.shared.suspended.calls.len(), 2);
    let first = m.shared.tasks.next_ready().unwrap();
    let held = m.shared.ready.remove(&first).unwrap();
    let registry = m.shared.tasks.clone();
    let values = m.shared.values.clone();
    let fuel = m.shared.steps_left;
    let events = m.shared.events.clone();
    assert!(m.dispatch_ready().is_err());
    assert_eq!(m.shared.tasks, registry);
    assert_eq!(m.shared.values, values);
    assert_eq!(m.shared.steps_left, fuel);
    assert_eq!(m.shared.events, events);
    m.shared.ready.insert(first, held);
    m.dispatch_ready().unwrap();
    assert_eq!(m.shared.ready.len(), 1);
    assert!(
        m.dispatch_ready()
            .unwrap_err()
            .message
            .contains("active control chain")
    );
    assert_eq!(m.shared.ready.len(), 1);
    assert!(!m.drive(0));
    assert_eq!(m.shared.events, events);
    assert!(m.drive(100));
    assert_eq!(messages(m), ["captured", "captured", "left", "edited"]);
    assert_eq!(m.shared.ready.len(), 1);
    assert_eq!(m.shared.suspended.calls.len(), 1);
    m.dispatch_ready().unwrap();
    assert!(m.drive(100));
    assert_eq!(
        messages(m),
        ["captured", "captured", "left", "edited", "right", "edited"]
    );
    assert!(m.dispatch_ready().is_err());
    assert_retired(m);
}
