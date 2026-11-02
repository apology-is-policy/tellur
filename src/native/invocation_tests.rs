//! Invocation ownership is independent of reusable frames and context identities.
use super::*;
use std::{cell::RefCell, rc::Rc};
fn host() -> Host {
    Host::new(serde_json::json!({}), None, true, Some(1000))
}
fn leaf(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
fn deliver(_: &mut Activation, _: &mut Frames, result: Completion) -> R<()> {
    assert!(matches!(result, Completion::Outcome(_)));
    Ok(())
}
fn outer(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Call {
            control: &LEAF,
            arguments: vec![],
            destination: None,
            deliver,
        });
    }
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
fn process(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(3, 1)?;
    Ok(Step::Complete(Completion::Process(23)))
}
fn settled(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(4, 1)?;
    Ok(Step::Complete(Completion::Settled))
}
static LEAF: Control = Control {
    name: "leaf",
    slots: 1,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: leaf,
};
static OUTER: Control = Control {
    name: "outer",
    slots: 1,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: outer,
};
static PROCESS: Control = Control {
    name: "process",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: process,
};
static SETTLED: Control = Control {
    name: "settled",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: settled,
};
static CAPTURE: Control = Control {
    name: "capture",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    resume: leaf,
};
#[test]
fn invocation_owners_distinguish_nested_calls_and_survive_frame_reuse() {
    let mut h = host();
    let mut values = Frames::default();
    let mut domains = Domains::default();
    let mut retired = vec![];
    for _ in 0..32 {
        let mut m = Machine::start(&mut h, &mut values, &mut domains, &OUTER, vec![]).unwrap();
        let parent = m.stack[0].owner;
        assert!(!retired.contains(&parent));
        assert_eq!(m.stack[0].base(), 0);
        assert_eq!(h.tasks.live_counts(), (1, 0));
        let before = h.tasks.clone();
        for old in &retired {
            assert!(h.tasks.retire_owner(*old, false).is_err());
        }
        assert_eq!(h.tasks, before);
        assert!(matches!(
            m.advance(&mut h, &mut values, &mut domains, 1).unwrap(),
            Poll::Paused
        ));
        let child = m.stack[1].owner;
        assert_ne!(child, parent);
        assert!(!retired.contains(&child));
        assert_eq!(h.tasks.live_counts(), (2, 0));
        assert!(matches!(
            m.advance(&mut h, &mut values, &mut domains, 1).unwrap(),
            Poll::Paused
        ));
        assert_eq!(h.tasks.live_counts(), (1, 0));
        assert!(h.tasks.retire_owner(child, false).is_err());
        assert!(matches!(
            m.advance(&mut h, &mut values, &mut domains, 1).unwrap(),
            Poll::Complete(Completion::Outcome(_))
        ));
        assert!(values.is_empty() && h.tasks.is_empty());
        assert_eq!(domains.len(), 0);
        retired.extend([parent, child]);
    }
    assert_eq!(h.fuel, Some(1000 - 32 * 3));
}
#[test]
fn normal_completion_rejects_pending_phases_then_retires_only_its_invocation() {
    for phase in 0..5 {
        let mut h = host();
        let mut values = Frames::default();
        let mut domains = Domains::default();
        let other = h.tasks.prepare_owner().unwrap();
        h.tasks.commit_owner(other);
        let independent = h.tasks.defer(other, "independent").unwrap();
        let mut m = Machine::start(&mut h, &mut values, &mut domains, &LEAF, vec![]).unwrap();
        let owner = m.stack[0].owner;
        let task = h.tasks.defer(owner, "pending").unwrap();
        if phase >= 1 {
            h.tasks.enqueue(task, owner, "pending").unwrap();
        }
        if phase >= 2 {
            h.tasks.dispatch(task).unwrap();
        }
        let wake = if phase == 3 {
            let wake = h.tasks.prepare_readiness(task, owner).unwrap();
            h.tasks.park(owner, &wake).unwrap();
            Some(wake)
        } else {
            None
        };
        if phase == 4 {
            h.tasks.complete(task, owner).unwrap();
        }
        let before = h.tasks.clone();
        assert!(h.tasks.require_retire_owner(owner, false).is_err());
        assert_eq!(h.tasks, before);
        assert_eq!(m.stack.len(), 1);
        let Err(error) = m.advance(&mut h, &mut values, &mut domains, 1) else {
            panic!()
        };
        assert_eq!(
            error,
            "E_NATIVE: invocation has outstanding task obligations"
        );
        assert!(values.is_empty() && m.stack.is_empty());
        assert_eq!(domains.len(), 0);
        assert_eq!(h.tasks.live_counts(), (1, 1));
        assert!(h.tasks.contains(independent));
        assert!(!h.tasks.contains(task));
        if let Some(wake) = wake {
            assert!(h.tasks.wake(&wake).is_err());
        }
        assert!(!h.source_faulted());
        assert_eq!(h.fuel, Some(999));
        h.tasks.retire_owner(other, true).unwrap();
        assert!(h.tasks.is_empty());
    }
}
struct Probe(Rc<RefCell<usize>>);
impl Drop for Probe {
    fn drop(&mut self) {
        *self.0.borrow_mut() += 1;
    }
}
#[test]
fn owner_exhaustion_precedes_frame_and_capture_admission() {
    let mut h = host();
    h.tasks.exhaust_owner_space();
    let before = format!("{:?}", h.activations);
    let mut values = Frames::default();
    let mut domains = Domains::default();
    assert!(
        Machine::start(&mut h, &mut values, &mut domains, &LEAF, vec![])
            .err()
            .unwrap()
            .contains("owner identity space exhausted")
    );
    assert!(values.is_empty() && h.tasks.is_empty());
    assert_eq!(domains.len(), 0);
    assert_eq!(format!("{:?}", h.activations), before);
    let drops = Rc::new(RefCell::new(0));
    let value = h.resources.insert("probe", Probe(drops.clone())).unwrap();
    {
        let mut c = Contexts::new(&mut h);
        assert!(
            c.start(&CAPTURE, vec![value.clone()])
                .unwrap_err()
                .contains("owner identity space exhausted")
        );
    }
    assert_eq!(*drops.borrow(), 0);
    assert!(h.resources.get::<Probe>(&value, "probe").is_ok());
    assert!(h.resources.scopes.is_empty());
    assert!(h.resources.current.is_none());
    assert_eq!(format!("{:?}", h.activations), before);
    h.release_frame(&[Some(value)]);
    assert_eq!(*drops.borrow(), 1);
    assert_eq!(h.fuel, Some(1000));
}
#[test]
fn administrative_completion_retires_obligations_without_fabricating_results() {
    for control in [&PROCESS, &SETTLED] {
        let mut h = host();
        let mut values = Frames::default();
        let mut domains = Domains::default();
        let mut m = Machine::start(&mut h, &mut values, &mut domains, control, vec![]).unwrap();
        let owner = m.stack[0].owner;
        let cold = h.tasks.defer(owner, "cold").unwrap();
        let task = h.tasks.defer(owner, "waiting").unwrap();
        let join = h.tasks.schedule(task, owner, "waiting").unwrap();
        h.tasks.dispatch(task).unwrap();
        let wake = h.tasks.prepare_readiness(task, owner).unwrap();
        h.tasks.park(owner, &wake).unwrap();
        let result = m.advance(&mut h, &mut values, &mut domains, 1).unwrap();
        assert!(matches!(
            result,
            Poll::Complete(Completion::Process(23) | Completion::Settled)
        ));
        assert!(h.tasks.is_empty() && values.is_empty());
        assert_eq!(domains.len(), 0);
        assert!(h.tasks.wake(&wake).is_err());
        assert!(h.tasks.deliver(task, owner, join).is_err());
        assert!(!h.tasks.contains(cold));
        assert!(!h.source_faulted());
        assert_eq!(h.fuel, Some(999));
    }
}
