//! Whole-session administrative retirement, without fabricated source faults.
use super::settlement_tests::{messages, queue_await, retired, until_message};
use super::*;

fn program(expanded: bool) -> CheckedProgram {
    let mut program = crate::compile(
        include_str!("../../tests/fixtures/process_retained.tellur"),
        Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    if expanded {
        for name in program
            .regions()
            .map(|r| r.name.clone())
            .collect::<Vec<_>>()
        {
            program = program.expanded(&name).unwrap();
        }
    }
    program
}

#[test]
fn process_stop_retires_delayed_results_ready_waiting_and_unstarted_roots() {
    for expanded in [false, true] {
        let p = program(expanded);
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        };
        let mut baseline = p.start_execution(&s, 1000);
        let base = baseline.machine.as_mut().unwrap();
        base.enter(&s).unwrap();
        until_message(base, "process armed");
        let before = base.shared.steps_left;
        assert!(base.drive(1000));
        let shutdown_fuel = before - base.shared.steps_left;
        let expected_fault = base
            .shared
            .events
            .iter()
            .find(|e| matches!(e, Event::Fault { .. }))
            .unwrap()
            .clone();
        for quotas in [vec![1], vec![0, 2, 0, 7], vec![3, 1, 9]] {
            let mut session = p.start_execution(&s, 1000);
            let m = session.machine.as_mut().unwrap();
            m.enter(&s).unwrap();
            until_message(m, "process armed");
            let owner = std::mem::replace(&mut m.context, Context::new(Default::default()));
            let mut tokens = vec![];
            let mut completion = None;
            for mode in ["completed", "waiting", "ready", "prepared"] {
                m.context = Context::new(owner.activations.child());
                m.call("start", vec![Value::String(mode.into())]).unwrap();
                until_message(m, "queued");
                queue_await(m);
                if mode == "completed" {
                    let task = m.shared.tasks.next_ready().unwrap();
                    let ReadyCall::Start { context, .. } = m.shared.ready.get_mut(&task).unwrap()
                    else {
                        panic!()
                    };
                    let Some(Continuation::ResumeAwait { wait, .. }) = context.stack.pop() else {
                        panic!()
                    };
                    let parent = context.task.unwrap().1;
                    m.dispatch_ready().unwrap();
                    assert!(m.drive(1000));
                    m.retain_task_completion(task, parent).unwrap();
                    completion = Some((task, parent, wait));
                } else if mode != "prepared" {
                    m.dispatch_ready().unwrap();
                    until_message(m, "child");
                    let token = m.park_for_readiness().unwrap();
                    if mode == "ready" {
                        m.signal_readiness(&token).unwrap();
                    }
                    tokens.push(token);
                }
            }
            assert_eq!(m.shared.completed.len(), 1);
            assert_eq!(m.shared.waiting.len(), 1);
            assert_eq!(m.shared.ready.len(), 2);
            assert_eq!(m.shared.suspended.calls.len(), 4);
            let before = messages(m);
            let fuel = m.shared.steps_left;
            m.context = owner;
            assert!(
                quotas
                    .iter()
                    .copied()
                    .cycle()
                    .take(1000)
                    .any(|q| m.drive(q))
            );
            assert!(matches!(m.context.flow, Some(Ok(Flow::Process(23)))));
            assert_eq!(m.process_status, Some(23));
            assert_eq!(fuel - m.shared.steps_left, shutdown_fuel);
            assert_eq!(
                messages(m),
                [before, vec!["process selected".into()]].concat()
            );
            let faults = m
                .shared
                .events
                .iter()
                .filter(|e| matches!(e, Event::Fault { .. }))
                .collect::<Vec<_>>();
            assert_eq!(faults.len(), 1);
            assert_eq!(
                serde_json::to_value(faults[0]).unwrap(),
                serde_json::to_value(&expected_fault).unwrap()
            );
            retired(m);
            let snapshot = (
                m.shared.tasks.clone(),
                m.shared.domains.clone(),
                m.shared.steps_left,
                m.shared.cursor,
                messages(m),
                m.shared.next_base,
            );
            for token in tokens {
                assert!(m.signal_readiness(&token).is_err());
            }
            let (task, parent, wait) = completion.unwrap();
            assert!(m.deliver_task_completion(task, parent, wait).is_err());
            assert!(m.retain_task_completion(task, parent).is_err());
            assert!(m.dispatch_ready().is_err());
            assert!(m.park_for_readiness().is_err());
            assert!(m.call("start", vec![Value::String("late".into())]).is_err());
            assert!(m.drive(0));
            assert_eq!(
                (
                    m.shared.tasks.clone(),
                    m.shared.domains.clone(),
                    m.shared.steps_left,
                    m.shared.cursor,
                    messages(m),
                    m.shared.next_base
                ),
                snapshot
            );
            retired(m);
        }
    }
}
