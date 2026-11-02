//! Executable external-readiness protocol model. These are explicit harness cuts,
//! not source suspension points, an I/O adapter or an enabled sibling scheduler.
//! Every retained context executes real checked code against one shared arena.
use super::*;
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct Readiness {
    driver: Arc<()>,
    generation: u64,
}
#[derive(Default)]
struct Driver<'a> {
    next: u64,
    identity: Arc<()>,
    waiting: BTreeMap<u64, Context<'a>>,
    ready: VecDeque<Context<'a>>,
}
impl<'a> Driver<'a> {
    fn park(&mut self, machine: &mut Machine<'a>) -> Result<Readiness> {
        if machine.context.frame.owner.is_none() || machine.context.flow.is_some() {
            return Err(error("readiness model requires a live normal continuation"));
        }
        let token = Readiness {
            driver: self.identity.clone(),
            generation: self
                .next
                .checked_add(1)
                .ok_or_else(|| error("readiness generation space exhausted"))?,
        };
        // Check before moving the original continuation or changing any identity.
        self.next = token.generation;
        let context = std::mem::replace(&mut machine.context, Context::new(Default::default()));
        assert!(self.waiting.insert(token.generation, context).is_none());
        Ok(token)
    }
    fn wake(&mut self, token: &Readiness) -> Result<()> {
        if !Arc::ptr_eq(&self.identity, &token.driver) {
            return Err(error("readiness belongs to another execution driver"));
        }
        let context = self
            .waiting
            .remove(&token.generation)
            .ok_or_else(|| error("stale or duplicate readiness signal"))?;
        self.ready.push_back(context);
        Ok(())
    }
    fn select(&mut self, machine: &mut Machine<'a>) -> Result<()> {
        if machine.context.frame.owner.is_some() || !machine.context.stack.is_empty() {
            return Err(error("readiness cannot preempt an active chain"));
        }
        let context = self
            .ready
            .pop_front()
            .ok_or_else(|| error("no ready continuation"))?;
        machine.context = context;
        Ok(())
    }
    fn empty(&self) -> bool {
        self.waiting.is_empty() && self.ready.is_empty()
    }
}
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
fn retired(m: &Machine<'_>, d: &Driver<'_>) {
    assert!(d.empty());
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
        let mut d = Driver::default();
        m.call("start", vec![Value::String("left".into())]).unwrap();
        cut_after_messages(m, 1);
        let independent = Context::new(m.context.activations.child());
        let left = d.park(m).unwrap();
        m.context = independent;
        m.call("start", vec![Value::String("right".into())])
            .unwrap();
        cut_after_messages(m, 2);
        let right = d.park(m).unwrap();
        assert_ne!(left.generation, right.generation);
        let initial_generation = right.generation;
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
        let registry = m.shared.tasks.clone();
        let values = m.shared.values.clone();
        let events = m.shared.events.clone();
        let fuel = m.shared.steps_left;
        d.wake(&first).unwrap();
        assert!(d.wake(&first).is_err());
        assert!(
            d.wake(&Readiness {
                driver: d.identity.clone(),
                generation: u64::MAX
            })
            .is_err()
        );
        // Readiness changes only eligibility, never the task completion or arena.
        assert_eq!(m.shared.tasks, registry);
        assert_eq!(m.shared.values, values);
        assert_eq!(m.shared.events, events);
        assert_eq!(m.shared.steps_left, fuel);
        d.select(m).unwrap();
        assert!(!m.drive(0));
        assert_eq!(m.shared.events, events);
        cut_after_messages(m, 4);
        assert_eq!(m.shared.cursor, 1);
        let again = d.park(m).unwrap();
        assert!(again.generation > initial_generation);
        assert!(
            d.wake(&first).is_err(),
            "old registration cannot wake a later wait"
        );
        assert_eq!(
            m.shared.suspended.calls.len(),
            2,
            "readiness is not completion"
        );
        d.wake(&second).unwrap();
        d.select(m).unwrap();
        // Wakes may queue another context; they cannot run it inside this stretch.
        d.wake(&again).unwrap();
        assert!(d.select(m).is_err());
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
        d.select(m).unwrap();
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
        assert!(d.wake(&again).is_err());
        assert!(d.wake(&second).is_err());
        assert!(d.select(m).is_err());
        retired(m, &d);
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
    let mut d = Driver {
        next: u64::MAX,
        ..Default::default()
    };
    let registry = m.shared.tasks.clone();
    let values = m.shared.values.clone();
    let owner = m.context.frame.owner;
    let depth = m.context.stack.len();
    let fuel = m.shared.steps_left;
    assert!(d.park(m).is_err());
    assert!(d.empty());
    assert_eq!(m.shared.tasks, registry);
    assert_eq!(m.shared.values, values);
    assert_eq!(m.context.frame.owner, owner);
    assert_eq!(m.context.stack.len(), depth);
    assert_eq!(m.shared.steps_left, fuel);
    assert_eq!(m.shared.cursor, 0);
    assert!(m.drive(100));
    assert_eq!(messages(m), ["waiting", "unused", "first", "first"]);
    assert!(
        d.park(m).is_err(),
        "completed chains cannot register readiness"
    );
    retired(m, &d);
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
    let mut a = Driver::default();
    let mut b = Driver::default();
    let stale = a.park(first).unwrap();
    let current = b.park(second).unwrap();
    assert_eq!(stale.generation, current.generation);
    assert!(!Arc::ptr_eq(&stale.driver, &current.driver));
    let values = second.shared.values.clone();
    let fuel = second.shared.steps_left;
    assert!(b.wake(&stale).is_err());
    assert_eq!(b.waiting.len(), 1);
    assert!(b.ready.is_empty());
    assert_eq!(second.shared.values, values);
    assert_eq!(second.shared.steps_left, fuel);
    a.wake(&stale).unwrap();
    a.select(first).unwrap();
    assert!(first.drive(100));
    retired(first, &a);
    drop(a); // The late token retains its identity, preventing identity reuse.
    assert!(b.wake(&stale).is_err());
    b.wake(&current).unwrap();
    b.select(second).unwrap();
    assert!(second.drive(100));
    retired(second, &b);
}
