//! Emergency settlement through checked continuations; source spawn is still gated.
use super::*;

const SOURCE: &str = r#"
async control leaf(ref mutable string text) exits ready { ref mutable string value from text } {
    emit(text);
    deref text = "finished";
    exit ready { text };
}
async control relay(ref mutable string text) exits ready { ref mutable string value from text } {
    auto inner = defer leaf(text);
    forward await inner;
}
async control worker(ref mutable string text) exits done {
    auto pending = defer relay(text);
    emit("captured");
    delta await pending {
        ready { auto value } => received
    }
    [received]: emit(value);
        exit done;
}
control independent() exits done {
    mutable string own = "isolated";
    run worker(borrow mutable own);
    exit done;
}
async control nested(string initial) exits done {
    fault boundary {
        mutable string text = move initial;
        auto pending = defer relay(borrow mutable text);
        delta await pending {
            ready { auto value } => received
        }
        [received]: emit(value);
            exit done;
    } contingency [] {
        emit("nested handler must not run");
        escalate;
    }
}
boundary stopped_request(string initial, string retained) retains retained {
    run nested(move initial);
    terminate;
}
contingency(string reason) {
    emit(borrow retained);
    terminate;
}
control named_nested() exits done {
    supervise stopped_request("nested loan", "named handler must not run");
    exit done;
}
control main() exits done {
    mutable string left = "left";
    mutable string right = "right";
    fault boundary {
        emit("owner ready");
        fault "original failure";
    } contingency [left, right] {
        emit(borrow left);
        emit(borrow right);
        left = "recovered";
        emit(borrow left);
        terminate process 0;
    }
}
"#;

fn program() -> CheckedProgram {
    crate::compile(
        SOURCE,
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
pub(super) fn messages(m: &Machine<'_>) -> Vec<String> {
    m.shared
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}
pub(super) fn until_message(m: &mut Machine<'_>, text: &str) {
    let previous = messages(m).len();
    for _ in 0..400 {
        assert!(!m.drive(1));
        if messages(m)[previous..].iter().any(|s| s == text) {
            return;
        }
    }
    panic!("message was not reached: {text}");
}
fn argument(m: &Machine<'_>, name: &str) -> Value {
    let slot = m
        .shared
        .program
        .region("main")
        .unwrap()
        .slots
        .values()
        .find(|s| s.name == name)
        .unwrap();
    Value::Reference {
        ty: "&mutable String".into(),
        place: Place {
            root: m.context.frame.base + slot.id,
            fields: vec![],
            indirect: false,
        },
    }
}
pub(super) fn queue_await(m: &mut Machine<'_>) {
    let Continuation::Block { code, next } = m.context.stack.pop().unwrap() else {
        panic!("await instruction expected")
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
pub(super) fn retired(m: &Machine<'_>) {
    assert!(m.settlements.is_empty());
    assert!(m.shared.values.is_empty());
    assert!(m.shared.tasks.is_empty());
    assert!(m.shared.ready.is_empty());
    assert!(m.shared.waiting.is_empty());
    assert!(m.shared.completed.is_empty());
    assert!(m.shared.suspended.calls.is_empty());
    assert_eq!(m.shared.domains.count(), 0);
    assert!(m.context.domain.is_none());
}

#[test]
fn boundary_waits_for_prepared_waiting_and_ready_borrowers_without_running_normal_code() {
    // Four retained states, including completed results with parked awaiting callers,
    // and an unrelated waiting root. These are explicit harness admissions.
    for (mode, quota) in ["prepared", "waiting", "ready", "completed"]
        .into_iter()
        .flat_map(|mode| [1, 400].map(|quota| (mode, quota)))
    {
        let p = program();
        let s = scenario();
        let mut baseline = p.start_execution(&s, 400);
        let base = baseline.machine.as_mut().unwrap();
        base.enter(&s).unwrap();
        until_message(base, "owner ready");
        let baseline_fuel = base.shared.steps_left;
        assert!(base.drive(400));
        let fault_fuel = baseline_fuel - base.shared.steps_left;
        let expected_fault = base
            .shared
            .events
            .iter()
            .find(|e| matches!(e, Event::Fault { .. }))
            .unwrap()
            .clone();
        let mut session = p.start_execution(&s, 400);
        let m = session.machine.as_mut().unwrap();
        m.enter(&s).unwrap();
        until_message(m, "owner ready");
        let args = [argument(m, "left"), argument(m, "right")];
        let domain = m.context.domain.unwrap();
        let separate = Context::new(m.context.activations.child());
        let owner = std::mem::replace(&mut m.context, separate);
        m.call("independent", vec![]).unwrap();
        until_message(m, "isolated");
        let independent = m.park_for_readiness().unwrap();
        for arg in args {
            let mut child = Context::new(owner.activations.child());
            child.domain = Some(domain);
            m.context = child;
            m.call("worker", vec![arg]).unwrap();
            until_message(m, "captured");
            queue_await(m);
        }
        let mut tokens = vec![];
        if mode == "completed" {
            for _ in 0..2 {
                let task = m.shared.tasks.next_ready().unwrap();
                let ReadyCall::Start { context, .. } = m.shared.ready.get_mut(&task).unwrap()
                else {
                    panic!()
                };
                assert!(matches!(
                    context.stack.pop(),
                    Some(Continuation::ResumeAwait { .. })
                ));
                let owner = context.task.unwrap().1;
                m.dispatch_ready().unwrap();
                assert!(m.drive(400));
                m.retain_task_completion(task, owner).unwrap();
            }
            assert_eq!(m.shared.completed.len(), 2);
        } else if mode != "prepared" {
            m.dispatch_ready().unwrap();
            until_message(m, "left");
            tokens.push(m.park_for_readiness().unwrap());
            m.dispatch_ready().unwrap();
            until_message(m, "right");
            tokens.push(m.park_for_readiness().unwrap());
            if mode == "ready" {
                for token in &tokens {
                    m.signal_readiness(token).unwrap();
                }
            }
        }
        let before = messages(m);
        let fuel = m.shared.steps_left;
        m.context = owner;
        assert!((0..400).any(|_| m.drive(quota)));
        assert!(matches!(m.context.flow, Some(Ok(Flow::Process(0)))));
        let mut expected = before;
        if mode == "completed" {
            expected.extend(["finished".into(), "finished".into(), "recovered".into()]);
        } else {
            expected.extend(["left".into(), "right".into(), "recovered".into()]);
        }
        assert_eq!(
            messages(m),
            expected,
            "normal child code ran in {mode} settlement"
        );
        assert_eq!(fuel - m.shared.steps_left, fault_fuel);
        let faults: Vec<_> = m
            .shared
            .events
            .iter()
            .filter(|e| matches!(e, Event::Fault { .. }))
            .collect();
        assert_eq!(faults.len(), 1);
        assert_eq!(
            serde_json::to_value(faults[0]).unwrap(),
            serde_json::to_value(&expected_fault).unwrap()
        );
        assert!(m.settlements.is_empty());
        assert!(m.shared.completed.is_empty());
        assert_eq!(m.shared.domains.count(), 0);
        for token in tokens {
            assert!(m.signal_readiness(&token).is_err());
        }
        // The owning fault isolates its domain; the handler's subsequent
        // process termination then also retires the independent waiting root.
        assert!(m.shared.waiting.is_empty());
        assert!(m.signal_readiness(&independent).is_err());
        assert!(m.dispatch_ready().is_err());
        retired(m);
    }
}

#[test]
fn nested_sibling_dispositions_run_without_entering_its_contingency() {
    for (target, ready) in ["nested", "named_nested"]
        .into_iter()
        .flat_map(|target| [false, true].map(|ready| (target, ready)))
    {
        let p = program();
        let s = scenario();
        let mut session = p.start_execution(&s, 400);
        let m = session.machine.as_mut().unwrap();
        m.enter(&s).unwrap();
        until_message(m, "owner ready");
        let domain = m.context.domain.unwrap();
        let mut child = Context::new(m.context.activations.child());
        child.domain = Some(domain);
        let owner = std::mem::replace(&mut m.context, child);
        m.call(
            target,
            if target == "nested" {
                vec![Value::String("nested loan".into())]
            } else {
                vec![]
            },
        )
        .unwrap();
        until_message(m, "nested loan");
        let nested = m.context.domain.unwrap();
        assert_ne!(nested, domain);
        let token = m.park_for_readiness().unwrap();
        if ready {
            m.signal_readiness(&token).unwrap();
        }
        m.context = owner;
        assert!((0..400).any(|_| m.drive(1)));
        assert!(matches!(m.context.flow, Some(Ok(Flow::Process(0)))));
        assert_eq!(
            messages(m),
            ["owner ready", "nested loan", "left", "right", "recovered"]
        );
        assert_eq!(
            m.shared
                .events
                .iter()
                .filter(|e| matches!(e, Event::Fault { .. }))
                .count(),
            1
        );
        assert!(m.signal_readiness(&token).is_err());
        retired(m);
    }
}

#[test]
fn repeated_sibling_settlement_recovers_without_reusing_waits_or_retaining_old_frames() {
    const REQUESTS: usize = 24;
    let source = format!(
        "{}{}",
        SOURCE.split("control main()").next().unwrap(),
        r#"
control main() exits done {
    supervised loop requests {
        delta next_request() {
            more => serve,
            finished => exit done
        }
        [serve]:
            mutable string left = "left";
            mutable string right = "right";
            fault boundary {
                emit("owner ready");
                fault "request failure";
            } contingency [left, right] {
                left = "retired";
                emit(borrow left);
                emit(borrow right);
                emit("settled");
                recover again requests;
            }
    }
}
"#
    );
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../playground/manifest.json")).unwrap();
    manifest["primitives"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name":"next_request", "parameters":[], "outcomes":[
                {"name":"more","fields":[]}, {"name":"finished","fields":[]}
            ], "effects":[], "returns":true
        }));
    let p = crate::compile(&source, Manifest::from_json(&manifest.to_string()).unwrap()).unwrap();
    for p in [p.clone(), p.expanded("main").unwrap()] {
        let mut s = scenario();
        s.calls = (0..=REQUESTS)
            .map(|i| ScriptCall {
                operation: "next_request".into(),
                outcome: if i == REQUESTS { "finished" } else { "more" }.into(),
                fields: vec![],
            })
            .collect();
        let mut session = p.start_execution(&s, 10_000);
        let m = session.machine.as_mut().unwrap();
        m.enter(&s).unwrap();
        until_message(m, "owner ready");
        let independent_context = Context::new(m.context.activations.child());
        let owner = std::mem::replace(&mut m.context, independent_context);
        m.call("independent", vec![]).unwrap();
        until_message(m, "isolated");
        let independent = m.park_for_readiness().unwrap();
        let independent_counts = m.shared.tasks.live_counts();
        m.context = owner;
        let mut stale = vec![];
        let mut settled_counts = None;
        for request in 0..REQUESTS {
            assert_eq!(m.shared.cursor, request + 1);
            let domain = m.context.domain.unwrap();
            let args = [argument(m, "left"), argument(m, "right")];
            let empty = Context::new(m.context.activations.child());
            let owner = std::mem::replace(&mut m.context, empty);
            for arg in args {
                let mut child = Context::new(owner.activations.child());
                child.domain = Some(domain);
                m.context = child;
                m.call("worker", vec![arg]).unwrap();
                until_message(m, "captured");
                queue_await(m);
            }
            let mut tokens = vec![];
            if request % 3 != 0 {
                for value in ["left", "right"] {
                    m.dispatch_ready().unwrap();
                    until_message(m, value);
                    tokens.push(m.park_for_readiness().unwrap());
                }
                if request % 3 == 2 {
                    for token in &tokens {
                        m.signal_readiness(token).unwrap();
                    }
                }
            }
            let registry = m.shared.tasks.clone();
            for token in &stale {
                assert!(m.signal_readiness(token).is_err());
            }
            assert_eq!(
                m.shared.tasks, registry,
                "old wakes changed a later request"
            );
            let before = messages(m);
            let fuel = m.shared.steps_left;
            m.context = owner;
            until_message(m, "settled");
            assert_eq!(
                &messages(m)[before.len()..],
                ["retired", "right", "settled"]
            );
            assert!(m.shared.steps_left < fuel);
            assert_eq!(m.shared.tasks.live_counts(), independent_counts);
            assert_eq!(m.shared.waiting.len(), 1);
            assert!(m.shared.ready.is_empty());
            assert!(m.settlements.is_empty());
            assert_eq!(m.shared.domains.count(), 3); // request, supervisor, unrelated root
            let counts = (
                m.shared.values.len(),
                m.shared.suspended.calls.len(),
                m.context.stack.len(),
            );
            assert_eq!(
                *settled_counts.get_or_insert(counts),
                counts,
                "completed requests retained storage"
            );
            stale.extend(tokens);
            for token in &stale {
                assert!(m.signal_readiness(token).is_err());
            }
            if request + 1 < REQUESTS {
                until_message(m, "owner ready");
            }
        }
        assert!(m.drive(400));
        assert!(matches!(&m.context.flow, Some(Ok(Flow::Exit(name,_))) if name=="done"));
        assert_eq!(m.shared.cursor, REQUESTS + 1);
        assert_eq!(
            m.shared
                .events
                .iter()
                .filter(|e| matches!(e, Event::Fault { .. }))
                .count(),
            REQUESTS
        );
        assert_eq!(m.shared.domains.count(), 1);
        assert_eq!(m.shared.tasks.live_counts().0 + 1, independent_counts.0);
        m.signal_readiness(&independent).unwrap();
        m.dispatch_ready().unwrap();
        assert!(m.drive(400));
        assert_eq!(messages(m).last().unwrap(), "finished");
        retired(m);
    }
}
