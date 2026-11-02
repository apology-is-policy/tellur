//! Emergency settlement through checked continuations; source spawn is still gated.
use super::*;

fn program() -> CheckedProgram {
    crate::compile(
        r#"
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
"#,
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
fn messages(m: &Machine<'_>) -> Vec<String> {
    m.shared
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}
fn until_message(m: &mut Machine<'_>, text: &str) {
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
fn queue_await(m: &mut Machine<'_>) {
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
fn retired(m: &Machine<'_>) {
    assert!(m.settlements.is_empty());
    assert!(m.shared.values.is_empty());
    assert!(m.shared.tasks.is_empty());
    assert!(m.shared.ready.is_empty());
    assert!(m.shared.waiting.is_empty());
    assert!(m.shared.suspended.calls.is_empty());
    assert_eq!(m.shared.domains.count(), 0);
    assert!(m.context.domain.is_none());
}

#[test]
fn boundary_waits_for_prepared_waiting_and_ready_borrowers_without_running_normal_code() {
    // Three different retained states, each with two overlapping borrowed children
    // and an unrelated waiting root. These are explicit harness admissions.
    for (mode, quota) in ["prepared", "waiting", "ready"]
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
        if mode != "prepared" {
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
        expected.extend(["left".into(), "right".into(), "recovered".into()]);
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
        assert_eq!(m.shared.domains.count(), 1);
        for token in tokens {
            assert!(m.signal_readiness(&token).is_err());
        }
        assert_eq!(
            m.shared.waiting.len(),
            1,
            "independent execution was drained"
        );
        m.signal_readiness(&independent).unwrap();
        m.dispatch_ready().unwrap();
        assert!(m.drive(400));
        assert_eq!(messages(m).last().unwrap(), "finished");
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
