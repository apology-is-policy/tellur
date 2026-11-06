//! Internal context interleavings, not a source scheduler. The harness keeps owners
//! suspended until their borrowers finish and never resumes a source-faulted chain.
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
        entry: "owner".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn until_events(machine: &mut Machine<'_>, count: usize) {
    for _ in 0..100 {
        if machine.shared.events.len() == count {
            return;
        }
        assert!(
            !machine.drive(1),
            "context finished before the requested pause"
        );
    }
    panic!("context never reached the requested event");
}
fn strings<'m>(machine: &'m Machine<'_>) -> Vec<&'m str> {
    machine
        .shared
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect()
}
const OWNERS: &str = r#"
control owner() exits ready {string left,string right} {
    mutable string left="left";mutable string right="right";
    emit("owner paused");emit(borrow left);emit(borrow right);exit ready {left,right};
}
control edit(ref mutable string target,string replacement) exits done {
    deref target=move replacement;emit(target);exit done;
}
"#;

#[test]
fn interleaved_contexts_share_storage_without_colliding_or_retiring_other_frames() {
    let p = checked(OWNERS);
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    until_events(m, 1);
    let owner_base = m.context.frame.base;
    let address = |name: &str| {
        owner_base
            + p.region("owner")
                .unwrap()
                .slots
                .values()
                .find(|slot| slot.name == name)
                .unwrap()
                .id
    };
    let reference = |name: &str| Value::Reference {
        ty: "&mutable String".into(),
        place: Place {
            root: address(name),
            fields: vec![],
            indirect: false,
        },
    };
    let child_a = Context::new(m.context.activations.child());
    let child_b = Context::new(m.context.activations.child());
    let mut owner = std::mem::replace(&mut m.context, child_a);
    m.call(
        "edit",
        vec![reference("left"), Value::String("edited left".into())],
    )
    .unwrap();
    let left_base = m.context.frame.base;
    let mut left = std::mem::replace(&mut m.context, child_b);
    m.call(
        "edit",
        vec![reference("right"), Value::String("edited right".into())],
    )
    .unwrap();
    let right_base = m.context.frame.base;
    assert!(left_base >= owner_base + p.next_id);
    assert!(right_base >= left_base + p.next_id);
    until_events(m, 2);
    assert_eq!(
        m.shared.values[&address("right")],
        Value::String("edited right".into())
    );
    // Swapping chains does not copy the arena or redirect either borrowed address.
    std::mem::swap(&mut m.context, &mut left);
    assert!(m.drive(100));
    assert!(matches!(m.context.flow,Some(Ok(Flow::Exit(ref name,_))) if name=="done"));
    assert!(
        !m.shared
            .values
            .keys()
            .any(|id| *id >= left_base && *id < left_base + p.next_id)
    );
    assert!(
        m.shared
            .values
            .keys()
            .any(|id| *id >= right_base && *id < right_base + p.next_id)
    );
    assert_eq!(
        m.shared.values[&address("left")],
        Value::String("edited left".into())
    );
    std::mem::swap(&mut m.context, &mut left);
    assert!(m.drive(100));
    assert_eq!(m.shared.values.len(), 2);
    std::mem::swap(&mut m.context, &mut owner);
    assert!(m.drive(100));
    let Some(Ok(Flow::Exit(outcome, values))) = m.context.flow.take() else {
        panic!("owner did not complete")
    };
    assert_eq!(outcome, "ready");
    assert_eq!(
        values,
        vec![
            Value::String("edited left".into()),
            Value::String("edited right".into())
        ]
    );
    assert_eq!(
        strings(m),
        [
            "owner paused",
            "edited right",
            "edited left",
            "edited left",
            "edited right"
        ]
    );
    assert!(m.shared.values.is_empty());
    assert_eq!(m.shared.next_base, 3 * p.next_id);
}

#[test]
fn interleaved_decisions_keep_facts_local_and_consume_one_script_history() {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../../playground/manifest.json")).unwrap();
    config["primitives"].as_array_mut().unwrap().push(serde_json::json!({"name":"sample","parameters":[],"outcomes":[{"name":"ready","fields":[{"name":"text","type":"string"}]}],"effects":["sample"],"returns":true}));
    let p = crate::compile(
        r#"
control work(bool choice) exits done {
    delta choice {true=>yes,false=>no}
    [yes]:emit("yes");[no]:emit("no");
    [entry]:delta sample() {ready {string text}=>received}
    [received]:emit(borrow text);exit done;
}"#,
        Manifest::from_json(&config.to_string()).unwrap(),
    )
    .unwrap();
    let s = Scenario {
        entry: "work".into(),
        arguments: vec![],
        calls: vec![
            ScriptCall {
                operation: "sample".into(),
                outcome: "ready".into(),
                fields: vec![Value::String("first sample".into())],
                waits: 0,
            },
            ScriptCall {
                operation: "sample".into(),
                outcome: "ready".into(),
                fields: vec![Value::String("second sample".into())],
                waits: 0,
            },
        ],
    };
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    let other = Context::new(m.context.activations.child());
    m.call("work", vec![Value::Bool(true)]).unwrap();
    until_events(m, 1);
    let mut first = std::mem::replace(&mut m.context, other);
    m.call("work", vec![Value::Bool(false)]).unwrap();
    until_events(m, 2);
    assert_ne!(m.context.frame.facts, first.frame.facts);
    assert!(m.drive(100));
    assert_eq!(m.shared.cursor, 1);
    std::mem::swap(&mut m.context, &mut first);
    assert!(m.drive(100));
    assert_eq!(m.shared.cursor, 2);
    assert_eq!(strings(m), ["yes", "no", "first sample", "second sample"]);
    assert_eq!(
        m.shared
            .events
            .iter()
            .filter(|e| matches!(e, Event::Primitive { .. }))
            .count(),
        2
    );
    assert!(m.shared.values.is_empty());
}

#[test]
fn selecting_another_context_never_refills_the_execution_fuel_budget() {
    let p = checked("control spin() faults {loop repeat {emit(\"tick\");again repeat;}}");
    let s = scenario();
    let mut session = p.start_execution(&s, 4);
    let m = session.machine.as_mut().unwrap();
    let other = Context::new(m.context.activations.child());
    m.call("spin", vec![]).unwrap();
    assert!(!m.drive(1));
    assert_eq!(m.shared.steps_left, 3);
    let mut first = std::mem::replace(&mut m.context, other);
    m.call("spin", vec![]).unwrap();
    assert!(!m.drive(1));
    assert_eq!(m.shared.steps_left, 2);
    assert!(m.drive(100));
    assert_eq!(m.shared.steps_left, 0);
    assert!(matches!(&m.context.flow,Some(Err(e)) if e.code=="E_FUEL"));
    let events = m.shared.events.clone();
    std::mem::swap(&mut m.context, &mut first);
    assert!(m.drive(100));
    assert!(matches!(&m.context.flow,Some(Err(e)) if e.code=="E_FUEL"));
    assert_eq!(m.shared.events, events);
    assert_eq!(m.shared.steps_left, 0);
    assert!(m.shared.values.is_empty());
}

#[test]
fn failed_child_admission_preserves_the_suspended_owner_and_identity_allocator() {
    let p = checked(OWNERS);
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    until_events(m, 1);
    let child = Context::new(m.context.activations.child());
    let owner = std::mem::replace(&mut m.context, child);
    let before = m.shared.values.clone();
    let history = m.shared.events.clone();
    let fuel = m.shared.steps_left;
    assert!(
        m.prepare_call(
            "edit",
            vec![
                Value::String("not a reference".into()),
                Value::String("unused".into())
            ]
        )
        .is_err()
    );
    let target = p
        .region("owner")
        .unwrap()
        .slots
        .values()
        .find(|s| s.name == "left")
        .unwrap()
        .id;
    let prepared = m
        .prepare_call(
            "edit",
            vec![
                Value::Reference {
                    ty: "&mutable String".into(),
                    place: Place {
                        root: target,
                        fields: vec![],
                        indirect: false,
                    },
                },
                Value::String("unused".into()),
            ],
        )
        .unwrap();
    let allocated = m.shared.next_base;
    m.shared.next_base = usize::MAX;
    assert!(
        m.start_call(prepared)
            .unwrap_err()
            .message
            .contains("storage address space exhausted")
    );
    assert_eq!(m.shared.values, before);
    assert_eq!(m.shared.events, history);
    assert_eq!(m.shared.steps_left, fuel);
    assert!(m.context.stack.is_empty());
    assert!(m.context.flow.is_none());
    let mut probe = m.context.activations.child();
    probe.enter("probe").unwrap();
    assert_eq!(
        probe
            .failure("probe", crate::fault::Phase::Execution, "test")
            .activations[0]
            .id,
        2
    );
    m.shared.next_base = allocated;
    m.context = owner;
    assert!(m.drive(100));
    assert_eq!(strings(m), ["owner paused", "left", "right"]);
    assert!(m.shared.values.is_empty());
}

#[test]
fn contingency_and_recovery_state_stays_with_its_suspended_context() {
    let p = checked(
        r#"
control request() exits done {
    supervised loop attempts(mutable bool first=true) {
        bool broken=first;first=false;
        fault boundary {
            delta broken {true=>fault "isolated",false=>exit done}
        } contingency [] {recover again attempts;}
    }
}
control other() exits done {emit("other");exit done;}
"#,
    );
    let s = scenario();
    let mut session = p.start_execution(&s, 200);
    let m = session.machine.as_mut().unwrap();
    let other = Context::new(m.context.activations.child());
    m.call("request", vec![]).unwrap();
    for _ in 0..100 {
        if !m.context.contingencies.is_empty() {
            break;
        }
        assert!(!m.drive(1));
    }
    assert_eq!(m.context.contingencies.len(), 1);
    let live_slots = m.shared.values.clone();
    let mut request = std::mem::replace(&mut m.context, other);
    assert!(m.context.contingencies.is_empty());
    assert!(m.context.handover.is_empty());
    m.call("other", vec![]).unwrap();
    assert!(m.drive(100));
    assert_eq!(m.shared.values, live_slots);
    std::mem::swap(&mut m.context, &mut request);
    assert!(m.drive(100));
    assert!(matches!(m.context.flow,Some(Ok(Flow::Exit(ref name,_))) if name=="done"));
    assert!(m.context.contingencies.is_empty());
    assert!(m.shared.values.is_empty());
    assert!(
        matches!(m.shared.events.first(),Some(Event::Fault{report}) if report.message=="E_FAULT: isolated")
    );
    assert_eq!(strings(m), ["other"]);
}
