//! Boundary-domain identity, admission and lifecycle; sibling draining is separate.
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
fn cut(m: &mut Machine<'_>, n: usize) {
    while messages(m).len() < n {
        assert!(!m.drive(1));
    }
}
fn retired(m: &Machine<'_>) {
    assert_eq!(m.shared.domains.count(), 0);
    assert!(m.context.domain.is_none());
    assert!(m.shared.tasks.is_empty());
    assert!(m.shared.values.is_empty());
    assert!(m.shared.ready.is_empty());
    assert!(m.shared.waiting.is_empty());
    assert!(m.shared.suspended.calls.is_empty());
}
#[test]
fn nested_named_and_lexical_boundaries_restore_open_parents_after_handling() {
    let p = checked(
        r#"
boundary request() {
    fault boundary {
        emit("inner");
        fault "inner failure";
    } contingency [] {
        escalate;
    }
}
contingency(string reason) {
    emit(reason);
    terminate;
}
control main() exits done {
    supervise request();
    emit("continued");
    exit done;
}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    let root = m.context.domain.unwrap();
    cut(m, 1);
    let inner = m.context.domain.unwrap();
    assert_ne!(inner, root);
    assert_eq!(m.shared.domains.count(), 3);
    cut(m, 2);
    let handler = m.context.domain.unwrap();
    assert_ne!(handler, inner);
    assert_ne!(handler, root);
    assert!(m.shared.domains.require_open(inner).is_err());
    assert!(m.shared.domains.require_open(handler).is_err());
    m.shared.domains.require_open(root).unwrap();
    cut(m, 3);
    assert_eq!(m.context.domain, Some(root));
    assert_eq!(m.shared.domains.count(), 1);
    m.shared.domains.require_open(root).unwrap();
    assert!(m.drive(100));
    assert_eq!(messages(m), ["inner", "inner failure", "continued"]);
    retired(m);
}
#[test]
fn domain_exhaustion_never_installs_a_partial_root_or_nested_boundary() {
    let p = checked(
        r#"
control main() exits done {
    emit("before");
    fault boundary {
        emit("unreachable");
    } contingency [] {
        escalate;
    }
    exit done;
}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.shared.domains.exhaust();
    assert!(
        m.enter(&s)
            .unwrap_err()
            .message
            .contains("fault domain identity")
    );
    assert_eq!(m.shared.next_base, 0);
    assert!(m.context.stack.is_empty());
    retired(m);
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    cut(m, 1);
    let root = m.context.domain.unwrap();
    m.shared.domains.exhaust();
    assert!(m.drive(100));
    assert!(matches!(&m.context.flow,Some(Err(d)) if d.message.contains("fault domain identity")));
    assert_eq!(messages(m), ["before"]);
    assert!(m.shared.domains.require_open(root).is_err());
    retired(m);
}
#[test]
fn stopped_waiting_and_ready_domains_block_normal_selection_without_losing_storage() {
    for ready in [false, true] {
        let p = checked(
            r#"
async control leaf(ref mutable string text) exits ready { ref mutable string value from text } {
    emit("waiting");
    deref text = "changed";
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
        );
        let s = scenario();
        let mut session = p.start_execution(&s, 200);
        let m = session.machine.as_mut().unwrap();
        m.call("start", vec![Value::String("first".into())])
            .unwrap();
        cut(m, 1);
        let independent = Context::new(m.context.activations.child());
        let first_domain = m.context.domain.unwrap();
        let (task, _) = m.context.task.unwrap();
        let first = m.park_for_readiness().unwrap();
        m.context = independent;
        m.call("start", vec![Value::String("second".into())])
            .unwrap();
        cut(m, 2);
        let other_domain = m.context.domain.unwrap();
        let second = m.park_for_readiness().unwrap();
        if ready {
            m.signal_readiness(&first).unwrap();
        }

        m.shared.domains.stop(first_domain).unwrap();
        let registry = m.shared.tasks.clone();
        let values = m.shared.values.clone();
        let events = m.shared.events.clone();
        let fuel = m.shared.steps_left;
        assert!(m.signal_readiness(&first).is_err());
        if ready {
            assert!(m.dispatch_ready().is_err());
        }
        assert!(m.require_domain_quiescent(first_domain).is_err());
        assert_eq!(m.shared.tasks, registry);
        assert_eq!(m.shared.values, values);
        assert_eq!(m.shared.events, events);
        assert_eq!(m.shared.steps_left, fuel);
        m.shared.domains.require_open(other_domain).unwrap();
        // Explicit harness emergency selection. Automatic sibling draining is not
        // implemented by these admission gates; normal dispatch stays blocked.
        m.context = if ready {
            match m.shared.ready.remove(&task).unwrap() {
                ReadyCall::Resume(context) => context,
                _ => panic!("expected resumed context"),
            }
        } else {
            m.shared.waiting.remove(&task).unwrap()
        };
        m.context.flow = Some(Err(error("injected emergency unwind")));
        assert!(m.drive(100));
        assert!(matches!(&m.context.flow, Some(Err(_))));
        assert_eq!(messages(m), ["waiting", "waiting"]);
        assert_eq!(m.shared.domains.count(), 1);
        assert!(m.signal_readiness(&first).is_err());
        m.signal_readiness(&second).unwrap();
        m.dispatch_ready().unwrap();
        assert!(m.drive(100));
        assert_eq!(messages(m), ["waiting", "waiting", "changed"]);
        retired(m);
    }
}
