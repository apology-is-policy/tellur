//! Started queue/join composition through checked reference frames.
use super::*;

fn program(expanded: bool) -> CheckedProgram {
    let p = crate::compile(
        include_str!("../../tests/fixtures/reference_started_tasks.tellur"),
        Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
    )
    .unwrap();
    if expanded {
        p.expanded("start").unwrap()
    } else {
        p
    }
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
fn snapshot(m: &Machine<'_>) -> serde_json::Value {
    serde_json::json!({"values":m.shared.values,"tasks":format!("{:?}",m.shared.tasks),"domains":format!("{:?}",m.shared.domains),"fuel":m.shared.steps_left,"events":m.shared.events,"ready":m.shared.ready.keys().map(|id|id.0).collect::<Vec<_>>(),"waiting":m.shared.waiting.keys().map(|id|id.0).collect::<Vec<_>>(),"completed":m.shared.completed.keys().map(|id|id.0).collect::<Vec<_>>(),"suspended":m.shared.suspended.calls.len(),"tracker":format!("{:?}",m.context.activations),"frame":m.context.frame.base,"owner":format!("{:?}",m.context.frame.owner)})
}
fn prepare<'a>(m: &mut Machine<'a>, initial: &str, fail: bool) -> (Option<Value>, &'a [Arm]) {
    m.context = Context::new(m.context.activations.child());
    m.call(
        "start",
        vec![Value::String(initial.into()), Value::Bool(fail)],
    )
    .unwrap();
    let count = messages(m).len();
    while messages(m).len() == count {
        assert!(!m.drive(1));
    }
    let Continuation::Block { code, next } = m.context.stack.pop().unwrap() else {
        panic!()
    };
    let instruction = &code[next];
    let Action::Decide {
        operation: Operation::AwaitTask(name),
        args,
        arms,
    } = &instruction.action
    else {
        panic!()
    };
    assert_eq!(name, "leaf");
    assert!(instruction.guard.evaluate(&m.context.frame.facts));
    m.context.activations.at(instruction.id);
    m.shared.steps_left -= 1;
    m.context.stack.push(Continuation::Block {
        code,
        next: next + 1,
    });
    let mut values = m.arguments(args).unwrap();
    let cold = values.pop();
    assert!(values.is_empty());
    (cold, arms)
}
fn park<'a>(m: &mut Machine<'a>) -> Context<'a> {
    std::mem::replace(&mut m.context, Context::new(Default::default()))
}
fn queue<'a>(
    m: &mut Machine<'a>,
    initial: &str,
    fail: bool,
) -> (Option<StartedTask<'a>>, Context<'a>, &'a [Arm]) {
    let (mut cold, arms) = prepare(m, initial, fail);
    let chain = format!("{:?}", m.context.activations);
    let fuel = m.shared.steps_left;
    let ready = m.shared.tasks.clone();
    let waits = m.shared.suspended.calls.len();
    let handle = m.queue_started("leaf", &mut cold).unwrap();
    assert!(cold.is_none());
    assert_eq!(format!("{:?}", m.context.activations), chain);
    assert_eq!(m.shared.steps_left, fuel);
    assert_eq!(m.shared.suspended.calls.len(), waits);
    assert_ne!(m.shared.tasks, ready);
    (Some(handle), park(m), arms)
}
fn publish(m: &mut Machine<'_>, quota: usize) {
    while !m.drive(quota) {}
    let (task, owner) = m.context.task.unwrap();
    m.retain_task_completion(task, owner).unwrap();
}
fn finish(m: &mut Machine<'_>, arms: &[Arm], flow: Flow) {
    let Flow::Exit(outcome, fields) = flow else {
        panic!()
    };
    m.bind(arms, outcome, fields).unwrap();
    while !m.drive(3) {}
}
fn empty(m: &Machine<'_>) {
    assert!(m.shared.tasks.is_empty() && m.shared.values.is_empty());
    assert!(
        m.shared.ready.is_empty()
            && m.shared.waiting.is_empty()
            && m.shared.completed.is_empty()
            && m.shared.suspended.calls.is_empty()
    );
    assert_eq!(m.shared.domains.count(), 0);
}
#[test]
fn started_claims_preserve_spawning_domain_in_every_phase_and_join_without_replacing_parent_chain()
{
    for expanded in [false, true] {
        for phase in 0..5 {
            for quota in [1, 4, 100] {
                let p = program(expanded);
                let s = scenario();
                let mut session = p.start_execution(&s, 500);
                let m = session.machine.as_mut().unwrap();
                let (mut handle, mut parent, arms) = queue(m, "initial", false);
                let task = handle.as_ref().unwrap().0.task;
                let owner = parent.frame.owner.unwrap();
                let root = parent.domain.unwrap();
                let mut join = None;
                if phase == 0 {
                    m.context = parent;
                    join = Some(m.claim_started("leaf", &mut handle).unwrap());
                    parent = park(m);
                }
                m.dispatch_ready().unwrap();
                if phase > 0 && phase < 4 {
                    // The first emit is inside the child's lexical boundary. Claims use
                    // its spawning domain, never the child's temporarily current domain.
                    if phase == 3 {
                        while m.shared.suspended.calls.is_empty() {
                            assert!(!m.drive(1));
                        }
                        assert!(m.context.started.is_none());
                        assert!(
                            m.shared
                                .suspended
                                .calls
                                .values()
                                .any(|call| call.context.started.is_some())
                        );
                    } else {
                        while messages(m).last().unwrap() != "inside" {
                            assert!(!m.drive(1));
                        }
                        assert_ne!(m.context.domain, Some(root));
                    }
                    let wake = if phase == 2 {
                        Some(m.park_for_readiness().unwrap())
                    } else {
                        None
                    };
                    let child = if phase != 2 { Some(park(m)) } else { None };
                    m.context = parent;
                    join = Some(m.claim_started("leaf", &mut handle).unwrap());
                    parent = park(m);
                    if let Some(wake) = wake {
                        m.signal_readiness(&wake).unwrap();
                        m.dispatch_ready().unwrap();
                    } else {
                        m.context = child.unwrap();
                    }
                }
                publish(m, quota);
                m.context = parent;
                if phase == 4 {
                    join = Some(m.claim_started("leaf", &mut handle).unwrap());
                }
                assert!(handle.is_none());
                let before = snapshot(m);
                assert!(
                    m.deliver_task_completion(task, owner, join.as_ref().unwrap().wait)
                        .is_err()
                );
                assert_eq!(snapshot(m), before);
                let tracker = format!("{:?}", m.context.activations);
                assert!(m.shared.tasks.retire_owner(owner, false).is_err());
                let flow = m.join_started(&mut join).unwrap();
                assert!(join.is_none());
                assert_eq!(format!("{:?}", m.context.activations), tracker);
                assert!(m.join_started(&mut join).is_err());
                finish(m, arms, flow);
                assert_eq!(
                    messages(m),
                    [
                        "queued", "inside", "leaving", "worker", "payload", "edited", "released"
                    ]
                );
                empty(m);
            }
        }
    }
}
#[test]
fn queue_rejection_preserves_captures_storage_and_original_diagnostics() {
    let p = program(false);
    let s = scenario();
    let mut session = p.start_execution(&s, 300);
    let m = session.machine.as_mut().unwrap();
    let (mut cold, arms) = prepare(m, "initial", false);
    for malformed in [false, true] {
        let saved = cold.clone();
        if malformed {
            let Some(Value::Task { arguments, .. }) = &mut cold else {
                panic!()
            };
            arguments[0] = Value::Bool(false);
        }
        let before = snapshot(m);
        let captures = cold.clone();
        assert!(
            m.queue_started(if malformed { "leaf" } else { "worker" }, &mut cold)
                .is_err()
        );
        assert_eq!(cold, captures);
        assert_eq!(snapshot(m), before);
        cold = saved;
    }
    let owner = m.context.frame.owner.unwrap();
    let other = m.shared.tasks.prepare_owner().unwrap();
    m.shared.tasks.commit_owner(other);
    m.context.frame.owner = Some(other);
    let before = snapshot(m);
    let captures = cold.clone();
    assert!(m.queue_started("leaf", &mut cold).is_err());
    assert_eq!(cold, captures);
    assert_eq!(snapshot(m), before);
    m.context.frame.owner = Some(owner);
    m.shared.tasks.retire_owner(other, false).unwrap();
    let mut handle = Some(m.queue_started("leaf", &mut cold).unwrap());
    let parent = park(m);
    m.dispatch_ready().unwrap();
    publish(m, 1);
    m.context = parent;
    let mut join = Some(m.claim_started("leaf", &mut handle).unwrap());
    let flow = m.join_started(&mut join).unwrap();
    finish(m, arms, flow);
    empty(m);
}
#[test]
fn foreign_crossed_stale_and_early_joins_preserve_opaque_authority_and_payloads() {
    let p = program(false);
    let s = scenario();
    let mut a = p.start_execution(&s, 500);
    let mut b = p.start_execution(&s, 500);
    let m = a.machine.as_mut().unwrap();
    let n = b.machine.as_mut().unwrap();
    let (mut handle, parent, arms) = queue(m, "left", false);
    let (mut foreign, foreign_parent, foreign_arms) = queue(n, "foreign", false);
    assert_eq!(
        handle.as_ref().unwrap().0.task,
        foreign.as_ref().unwrap().0.task
    );
    m.context = parent;
    n.context = foreign_parent;
    for name in ["worker", "missing"] {
        let before = snapshot(m);
        assert!(m.claim_started(name, &mut handle).is_err());
        assert!(handle.is_some());
        assert_eq!(snapshot(m), before);
    }
    let before = snapshot(n);
    assert!(n.claim_started("leaf", &mut handle).is_err());
    assert_eq!(snapshot(n), before);
    assert!(handle.is_some());
    let owner = m.context.frame.owner.unwrap();
    let other = m.shared.tasks.prepare_owner().unwrap();
    m.shared.tasks.commit_owner(other);
    m.context.frame.owner = Some(other);
    let before = snapshot(m);
    assert!(m.claim_started("leaf", &mut handle).is_err());
    assert_eq!(snapshot(m), before);
    m.context.frame.owner = Some(owner);
    m.shared.tasks.retire_owner(other, false).unwrap();
    let root = m.context.domain.unwrap();
    let nested = m.shared.domains.prepare(Some(root)).unwrap();
    m.shared.domains.commit(nested);
    m.context.domain = Some(nested.id);
    let before = snapshot(m);
    assert!(m.claim_started("leaf", &mut handle).is_err());
    assert_eq!(snapshot(m), before);
    m.context.domain = Some(root);
    m.shared.domains.close(nested).unwrap();
    let mut join = Some(m.claim_started("leaf", &mut handle).unwrap());
    assert!(handle.is_none());
    let before = snapshot(m);
    assert!(m.join_started(&mut join).is_err());
    assert_eq!(snapshot(m), before);
    assert!(join.is_some());
    assert!(m.claim_started("leaf", &mut handle).is_err());
    let parent = park(m);
    m.dispatch_ready().unwrap();
    publish(m, 2);
    m.context = parent;
    let before = snapshot(n);
    assert!(n.join_started(&mut join).is_err());
    assert_eq!(snapshot(n), before);
    let nested = m.shared.domains.prepare(Some(root)).unwrap();
    m.shared.domains.commit(nested);
    m.context.domain = Some(nested.id);
    let before = snapshot(m);
    assert!(m.join_started(&mut join).is_err());
    assert_eq!(snapshot(m), before);
    assert!(join.is_some());
    m.context.domain = Some(root);
    m.shared.domains.close(nested).unwrap();
    let flow = m.join_started(&mut join).unwrap();
    let before = snapshot(m);
    assert!(m.join_started(&mut join).is_err());
    assert_eq!(snapshot(m), before);
    finish(m, arms, flow);
    empty(m);
    let parent = park(n);
    n.dispatch_ready().unwrap();
    publish(n, 3);
    n.context = parent;
    let mut join = Some(n.claim_started("leaf", &mut foreign).unwrap());
    let flow = n.join_started(&mut join).unwrap();
    finish(n, foreign_arms, flow);
    empty(n);
}
#[test]
fn exhausted_join_capacity_preserves_unjoined_work_until_emergency_retirement() {
    let p = program(false);
    let s = scenario();
    let mut session = p.start_execution(&s, 300);
    let m = session.machine.as_mut().unwrap();
    let (mut handle, parent, _) = queue(m, "initial", false);
    m.context = parent;
    m.shared.tasks.exhaust_wait_space();
    let before = snapshot(m);
    assert!(m.claim_started("leaf", &mut handle).is_err());
    assert!(handle.is_some());
    assert_eq!(snapshot(m), before);
    m.context.flow = Some(Err(error("owning stop after wait exhaustion")));
    assert!(m.drive(100));
    empty(m);
    assert!(m.claim_started("leaf", &mut handle).is_err());
    assert_eq!(messages(m), ["queued"]);
}
#[test]
fn emergency_retirement_clears_ready_waiting_and_completed_started_work_without_child_handlers() {
    for expanded in [false, true] {
        for phase in 0..4 {
            for process in [false, true] {
                for claimed in [false, true] {
                    let p = program(expanded);
                    let s = scenario();
                    let mut session = p.start_execution(&s, 500);
                    let m = session.machine.as_mut().unwrap();
                    let (mut handle, mut parent, _) = queue(m, "initial", false);
                    let mut join = None;
                    if claimed {
                        m.context = parent;
                        join = Some(m.claim_started("leaf", &mut handle).unwrap());
                        parent = park(m);
                    }
                    let mut wake = None;
                    if phase > 0 {
                        m.dispatch_ready().unwrap();
                        if phase == 1 {
                            while messages(m).last().unwrap() != "inside" {
                                assert!(!m.drive(1));
                            }
                            wake = Some(m.park_for_readiness().unwrap());
                        } else if phase == 2 {
                            publish(m, 2);
                        } else {
                            while m.shared.suspended.calls.is_empty() {
                                assert!(!m.drive(1));
                            }
                            wake = Some(m.park_for_readiness().unwrap());
                        }
                    }
                    m.context = parent;
                    let before = messages(m);
                    let fuel = m.shared.steps_left;
                    m.context.flow = Some(if process {
                        Ok(Flow::Process(23))
                    } else {
                        m.context.activations.source_fault();
                        Err(error("owning source stop"))
                    });
                    assert!(m.drive(100));
                    assert_eq!(messages(m), before);
                    assert_eq!(m.shared.steps_left, fuel);
                    empty(m);
                    if process {
                        assert_eq!(m.process_status, Some(23));
                        assert!(matches!(m.context.flow, Some(Ok(Flow::Process(23)))));
                    } else {
                        assert!(m.context.flow.as_ref().unwrap().is_err());
                    }
                    if let Some(wake) = wake {
                        assert!(m.signal_readiness(&wake).is_err());
                    }
                    assert!(m.claim_started("leaf", &mut handle).is_err());
                    assert!(m.join_started(&mut join).is_err());
                }
            }
        }
    }
}
