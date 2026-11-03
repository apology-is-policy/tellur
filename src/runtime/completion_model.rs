//! Runtime completion storage exercised through checked reference frames.
//! Explicit harness cuts separate queueing, completion and joining; host quotas
//! never perform this scheduling, and production await still resumes immediately.
use super::*;

struct Completed;
impl Completed {
    fn publish(&mut self, m: &mut Machine<'_>) -> Result<TaskId> {
        let (task, owner) = m.context.task.unwrap();
        m.retain_task_completion(task, owner)?;
        Ok(task)
    }
    fn deliver<'a>(
        &mut self,
        m: &mut Machine<'a>,
        task: TaskId,
        owner: OwnerId,
        wait: WaitId,
    ) -> Result<Context<'a>> {
        m.deliver_task_completion(task, owner, wait)
    }
}
fn program(expanded: bool) -> CheckedProgram {
    let p = crate::compile(
        include_str!("../../tests/fixtures/task_completion_model.tellur"),
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
/// Model queue-only admission at the checked await instruction. The checked
/// caller remains retained in the harness until its explicit completion join.
fn queue<'a>(m: &mut Machine<'a>, initial: &str) -> (TaskId, Context<'a>) {
    m.context = Context::new(m.context.activations.child());
    m.call("start", vec![Value::String(initial.into())])
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
    assert!(instruction.guard.evaluate(&m.context.frame.facts));
    m.context.activations.at(instruction.id);
    m.shared.steps_left -= 1;
    m.context.stack.push(Continuation::Block {
        code,
        next: next + 1,
    });
    let mut values = m.arguments(args).unwrap();
    let Some(Value::Task {
        id,
        operation,
        arguments,
        ..
    }) = values.pop()
    else {
        panic!()
    };
    assert!(values.is_empty());
    assert_eq!(operation, *name);
    let task = TaskId(id);
    let owner = m.context.frame.owner.unwrap();
    let invocation = m.prepare_call(name, arguments).unwrap();
    m.shared.tasks.enqueue(task, owner, name).unwrap();
    m.context
        .stack
        .push(Continuation::TaskDecision { task, arms });
    let mut child = Context::new(std::mem::take(&mut m.context.activations));
    child.task = Some((task, owner));
    child.domain = m.context.domain;
    child.phase = m.context.phase.clone();
    let parent = std::mem::replace(&mut m.context, Context::new(Default::default()));
    assert!(
        m.shared
            .ready
            .insert(
                task,
                ReadyCall::Start {
                    invocation,
                    context: child
                }
            )
            .is_none()
    );
    (task, parent)
}
fn restore<'a>(m: &mut Machine<'a>, mut parent: Context<'a>, mut completed: Context<'a>) {
    assert!(completed.stack.is_empty() && completed.frame.owner.is_none());
    parent.activations = std::mem::take(&mut completed.activations);
    parent.phase = completed.phase;
    parent.handover = completed.handover;
    parent.flow = completed.flow;
    m.context = parent;
}
fn retired(m: &Machine<'_>, _completed: &Completed) {
    assert!(m.shared.completed.is_empty());
    assert!(m.shared.tasks.is_empty());
    assert!(m.shared.values.is_empty());
    assert!(m.shared.ready.is_empty());
    assert!(m.shared.waiting.is_empty());
    assert!(m.shared.suspended.calls.is_empty());
    assert_eq!(m.shared.domains.count(), 0);
    assert!(m.context.stack.is_empty() && m.context.frame.owner.is_none());
}
#[test]
fn delayed_joins_retain_owned_and_exclusive_borrowed_payloads_after_child_frames_retire() {
    for expanded in [false, true] {
        for reverse in [false, true] {
            for quota in [1, 4, 100] {
                for join_early in [false, true] {
                    let p = program(expanded);
                    let s = scenario();
                    let mut session = p.start_execution(&s, 300);
                    let m = session.machine.as_mut().unwrap();
                    let (left, left_parent) = queue(m, "left");
                    let (right, right_parent) = queue(m, "right");
                    let owners = [
                        left_parent.frame.owner.unwrap(),
                        right_parent.frame.owner.unwrap(),
                    ];
                    let mut parents = BTreeMap::from([(left, left_parent), (right, right_parent)]);
                    let mut joins = BTreeMap::new();
                    if join_early {
                        for (task, owner) in [left, right].into_iter().zip(owners) {
                            let before = m.shared.tasks.clone();
                            let wait = m.shared.tasks.prepare_join(task, owner, "leaf").unwrap();
                            assert_eq!(m.shared.tasks, before);
                            assert_eq!(m.shared.tasks.join(task, owner, "leaf").unwrap(), wait);
                            joins.insert(task, wait);
                        }
                    }
                    let before_keys: Vec<_> = m.shared.values.keys().copied().collect();
                    let mut completed = Completed;
                    for expected in [left, right] {
                        m.dispatch_ready().unwrap();
                        while !m.drive(quota) {}
                        let (task, owner) = m.context.task.unwrap();
                        let registry = m.shared.tasks.clone();
                        let values = m.shared.values.clone();
                        let fuel = m.shared.steps_left;
                        let other = if owner == owners[0] {
                            owners[1]
                        } else {
                            owners[0]
                        };
                        assert!(m.retain_task_completion(task, other).is_err());
                        let domain = m.context.domain.take();
                        assert!(m.retain_task_completion(task, owner).is_err());
                        m.context.domain = domain;
                        assert!(
                            matches!(m.context.flow, Some(Ok(Flow::Exit(_, ref fields))) if fields.len() == 2)
                        );
                        assert_eq!(m.shared.tasks, registry);
                        assert_eq!(m.shared.values, values);
                        assert_eq!(m.shared.steps_left, fuel);
                        assert_eq!(completed.publish(m).unwrap(), expected);
                        let registry = m.shared.tasks.clone();
                        assert!(m.retain_task_completion(task, owner).is_err());
                        assert_eq!(m.shared.tasks, registry);
                    }
                    assert_eq!(messages(m), ["queued", "queued", "child", "child"]);
                    assert_eq!(
                        m.shared.values.keys().copied().collect::<Vec<_>>(),
                        before_keys
                    );
                    let roots: Vec<_> = m
                        .shared
                        .completed
                        .values()
                        .map(|c| {
                            let Some(Ok(Flow::Exit(label, fields))) = &c.flow else {
                                panic!()
                            };
                            assert_eq!(label, "ready");
                            assert_eq!(fields[0], Value::String("payload".into()));
                            let Value::Reference { place, .. } = &fields[1] else {
                                panic!()
                            };
                            assert_eq!(
                                m.shared.values[&place.root],
                                Value::String("edited".into())
                            );
                            place.root
                        })
                        .collect();
                    assert_ne!(roots[0], roots[1]);
                    if !join_early {
                        for (task, owner) in [left, right].into_iter().zip(owners) {
                            let before = m.shared.tasks.clone();
                            let wait = m.shared.tasks.prepare_join(task, owner, "leaf").unwrap();
                            assert_eq!(m.shared.tasks, before);
                            assert_eq!(m.shared.tasks.join(task, owner, "leaf").unwrap(), wait);
                            joins.insert(task, wait);
                        }
                    }
                    for owner in owners {
                        assert!(m.shared.tasks.retire_owner(owner, false).is_err());
                    }
                    let order = if reverse {
                        [right, left]
                    } else {
                        [left, right]
                    };
                    for task in order {
                        let parent = parents.remove(&task).unwrap();
                        let owner = parent.frame.owner.unwrap();
                        let wait = joins[&task];
                        let crossed = joins[&if task == left { right } else { left }];
                        let registry = m.shared.tasks.clone();
                        let values = m.shared.values.clone();
                        let fuel = m.shared.steps_left;
                        let other = if owner == owners[0] {
                            owners[1]
                        } else {
                            owners[0]
                        };
                        let held = m.shared.completed.remove(&task).unwrap();
                        assert!(completed.deliver(m, task, owner, wait).is_err());
                        assert_eq!(m.shared.tasks, registry);
                        m.shared.completed.insert(task, held);
                        assert!(completed.deliver(m, task, other, wait).is_err());
                        assert!(completed.deliver(m, task, owner, crossed).is_err());
                        assert!(
                            matches!(m.shared.completed[&task].flow, Some(Ok(Flow::Exit(_, ref fields))) if fields.len() == 2 && fields[0] == Value::String("payload".into()))
                        );
                        assert_eq!(m.shared.tasks, registry);
                        assert_eq!(m.shared.values, values);
                        assert_eq!(m.shared.steps_left, fuel);
                        let payload = completed.deliver(m, task, owner, wait).unwrap();
                        assert!(completed.deliver(m, task, owner, wait).is_err());
                        restore(m, parent, payload);
                        while !m.drive(quota) {}
                    }
                    assert_eq!(
                        messages(m),
                        [
                            "queued", "queued", "child", "child", "payload", "edited", "released",
                            "payload", "edited", "released"
                        ]
                    );
                    retired(m, &completed);
                }
            }
        }
    }
}
#[test]
fn stopped_completion_retires_before_its_owner_and_preserves_an_independent_join() {
    for expanded in [false, true] {
        let p = program(expanded);
        let s = scenario();
        let mut session = p.start_execution(&s, 300);
        let m = session.machine.as_mut().unwrap();
        let (failed, failed_parent) = queue(m, "failed");
        let (survivor, survivor_parent) = queue(m, "survivor");
        let mut completed = Completed;
        for task in [failed, survivor] {
            m.dispatch_ready().unwrap();
            while !m.drive(1) {}
            assert_eq!(completed.publish(m).unwrap(), task);
        }
        let owner = failed_parent.frame.owner.unwrap();
        let wait = m.shared.tasks.join(failed, owner, "leaf").unwrap();
        m.shared
            .domains
            .stop(failed_parent.domain.unwrap())
            .unwrap();
        let registry = m.shared.tasks.clone();
        let values = m.shared.values.clone();
        assert!(completed.deliver(m, failed, owner, wait).is_err());
        assert!(m.shared.tasks.retire_owner(owner, false).is_err());
        assert_eq!(m.shared.tasks, registry);
        assert_eq!(m.shared.values, values);
        assert!(
            m.require_domain_quiescent(failed_parent.domain.unwrap())
                .is_err()
        );
        let mut failed_parent = failed_parent;
        failed_parent.activations =
            std::mem::take(&mut m.shared.completed.get_mut(&failed).unwrap().activations);
        failed_parent.flow = Some(Err(error("modeled owning stop")));
        m.context = failed_parent;
        // Production emergency drain must remove the pending payload before the
        // caller's frame retires; the harness does not perform that cleanup.
        assert!(m.shared.completed.contains_key(&failed));
        assert!(m.drive(100));
        assert!(m.context.flow.as_ref().unwrap().is_err());
        assert!(!m.shared.values.is_empty());
        assert_eq!(m.shared.completed.len(), 1);
        assert!(!m.shared.completed.contains_key(&failed));
        assert!(m.shared.completed.contains_key(&survivor));
        let owner = survivor_parent.frame.owner.unwrap();
        let wait = m.shared.tasks.join(survivor, owner, "leaf").unwrap();
        let payload = completed.deliver(m, survivor, owner, wait).unwrap();
        restore(m, survivor_parent, payload);
        assert!(m.drive(100));
        assert_eq!(
            messages(m),
            [
                "queued", "queued", "child", "child", "payload", "edited", "released"
            ]
        );
        retired(m, &completed);
    }
}
