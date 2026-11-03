//! Bound task/context integration through real shared host storage and ownership.
use super::*;
use std::{cell::RefCell, rc::Rc};
struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
fn host() -> Host {
    Host::new(serde_json::json!({}), None, true, Some(1000))
}
fn returning(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    let fields = a
        .control
        .parameters
        .iter()
        .map(|slot| v[a.base() + slot].take().unwrap())
        .collect();
    Ok(Step::Complete(Completion::Outcome((
        "ready".into(),
        fields,
    ))))
}
fn failing(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    h.fault(V::String("task failed".into()))
}
fn process(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(3, 1)?;
    Ok(Step::Complete(Completion::Process(23)))
}
static OWNER: Control = Control {
    name: "owner",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: returning,
};
static CHILD: Control = Control {
    name: "child",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: returning,
};
static FAULT: Control = Control {
    name: "fault",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    access: None,
    resume: failing,
};
static PROCESS: Control = Control {
    name: "process",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    access: None,
    resume: process,
};
static HUGE: Control = Control {
    name: "huge",
    slots: usize::MAX,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: returning,
};
static BAD: Control = Control {
    name: "bad",
    slots: 1,
    atoms: 0,
    parameters: &[0, 0],
    retained: &[],
    constructor: false,
    access: None,
    resume: returning,
};
fn make(c: &mut Contexts<'_>, id: u32, drops: &Rc<RefCell<Vec<u32>>>) -> V {
    c.host
        .resources
        .insert("probe", Probe(id, drops.clone()))
        .unwrap()
}
fn queue(
    c: &mut Contexts<'_>,
    parent: &ContextKey,
    control: &'static Control,
    mut args: Vec<V>,
) -> Option<QueuedTask> {
    let a = c.parked[&parent.id].machine.stack.last().unwrap();
    let mut payload = Some(a.defer_task(c.host, control, "pending", &mut args).unwrap());
    assert!(args.is_empty());
    Some(a.queue_task(c.host, control, &mut payload).unwrap())
}
fn snapshot(c: &Contexts<'_>) -> serde_json::Value {
    serde_json::json!({"next":c.next,"values":format!("{:?}", &*c.values),"parked":c.parked.keys().collect::<Vec<_>>(),"complete":c.completed.keys().collect::<Vec<_>>(),"waits":format!("{:?}",c.waits),"tasks":format!("{:?}",c.host.tasks),"domains":c.domains.len(),"members":c.domains.member_count(),"fuel":c.host.fuel,"scope_next":c.host.resources.next_scope,"scopes":c.host.resources.scopes,"current":c.host.resources.current,"resources":c.host.resources.values.iter().map(|(k,v)|(k,v.scope)).collect::<Vec<_>>(),"diagnostics":format!("{:?}",c.host.activations)})
}
fn reject(c: &mut Contexts<'_>, parent: &ContextKey, q: &mut Option<QueuedTask>) {
    let before = snapshot(c);
    let task = format!("{q:?}");
    let dependencies = q.as_ref().map(|q| q.dependencies(&c.values));
    assert!(c.start_task(parent, q).is_err());
    assert_eq!(snapshot(c), before);
    assert_eq!(format!("{q:?}"), task);
    assert_eq!(q.as_ref().map(|q| q.dependencies(&c.values)), dependencies);
}

#[test]
fn capture_dependencies_survive_all_admission_routes_readiness_and_retained_results() {
    for route in 0..3 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let own = make(&mut c, 0, &drops);
        let parent = c.start(&OWNER, vec![own]).unwrap();
        let base = c.parked[&parent.id].machine.stack[0].base();
        let captured = make(&mut c, 1, &drops);
        let args = vec![
            captured,
            super::super::reference_evidence::attach(
                &c.values,
                super::super::super::reference(&c.values, base, &[], false, "&probe").unwrap(),
                &[super::super::reference_evidence::Origin {
                    root: base,
                    fields: &[],
                    indirect: false,
                    guard: true,
                    opaque: true,
                }],
            ),
        ];
        let expected = Dependencies::describe(&c.values, &args);
        let child = if route == 0 {
            c.start_child(&parent, &CHILD, args).unwrap()
        } else {
            let mut q = queue(&mut c, &parent, &CHILD, args);
            assert_eq!(q.as_ref().unwrap().dependencies(&c.values), expected);
            if route == 1 {
                c.start_task(&parent, &mut q).unwrap()
            } else {
                c.start_await(&parent, &mut q).unwrap()
            }
        };
        let foreign = ContextKey {
            session: Arc::new(()),
            id: child.id,
        };
        assert!(c.dependencies(&foreign).is_err());
        assert_eq!(c.dependencies(&child).unwrap(), &expected);
        c.advance_retained(&child, 1).unwrap();
        let wake = c.park_for_readiness(&child).unwrap();
        let before = snapshot(&c);
        for _ in 0..4 {
            c.dependencies(&child)
                .unwrap()
                .require_live(&c.values)
                .unwrap();
        }
        assert_eq!(snapshot(&c), before);
        assert_eq!(c.dependencies(&child).unwrap(), &expected);
        c.signal_readiness(&wake).unwrap();
        assert!(matches!(
            c.advance_retained(&child, 1).unwrap(),
            ContextPoll::Ready
        ));
        assert_eq!(c.dependencies(&child).unwrap(), &expected);
        let result = fields(&mut c, Some(&parent), &child);
        assert!(c.dependencies(&child).is_err());
        expected.require_live(&c.values).unwrap();
        release(&mut c, result);
        assert_eq!(*drops.borrow(), [1]);
        c.advance_retained(&parent, 2).unwrap();
        let result = fields(&mut c, None, &parent);
        release(&mut c, result);
        assert!(expected.require_live(&c.values).is_err());
        assert_eq!(*drops.borrow(), [1, 0]);
        empty(&c);
    }
}

#[test]
fn bounded_unknown_dependency_metadata_does_not_change_execution_or_join() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c.start(&OWNER, vec![V::U32(1)]).unwrap();
    let large = V::Construct {
        ty: "wide".into(),
        name: "wide".into(),
        fields: vec![V::U32(7); 4097],
    };
    let child = c.start_child(&parent, &OWNER, vec![large]).unwrap();
    assert!(matches!(
        c.dependencies(&child).unwrap(),
        Dependencies::Unknown(_)
    ));
    assert!(
        c.dependencies(&child)
            .unwrap()
            .require_live(&c.values)
            .is_err()
    );
    assert!(matches!(
        c.advance_retained(&child, 2).unwrap(),
        ContextPoll::Ready
    ));
    assert!(matches!(
        c.dependencies(&child).unwrap(),
        Dependencies::Unknown(_)
    ));
    let result = fields(&mut c, Some(&parent), &child);
    let V::Construct {
        fields: returned, ..
    } = &result[0]
    else {
        panic!()
    };
    assert_eq!(returned.len(), 4097);
    release(&mut c, result);
    c.advance_retained(&parent, 2).unwrap();
    let result = fields(&mut c, None, &parent);
    release(&mut c, result);
    assert!(c.host.events.is_empty());
    assert_eq!(c.host.fuel, Some(996));
    empty(&c);
}
fn fields(c: &mut Contexts<'_>, parent: Option<&ContextKey>, key: &ContextKey) -> Vec<V> {
    let Completion::Outcome((_, fields)) = c.join(parent, key).unwrap() else {
        panic!()
    };
    fields
}
fn release(c: &mut Contexts<'_>, values: Vec<V>) {
    c.host
        .release_frame(&values.into_iter().map(Some).collect::<Vec<_>>());
}
fn empty(c: &Contexts<'_>) {
    assert!(
        c.parked.is_empty()
            && c.completed.is_empty()
            && c.waits.is_empty()
            && c.terminal.is_empty()
            && c.values.is_empty()
            && c.host.tasks.is_empty()
    );
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    assert!(c.host.resources.values.is_empty() && c.host.resources.scopes.is_empty());
    assert!(c.host.resources.current.is_none());
}
#[test]
fn queued_contexts_pair_readiness_and_one_use_results_with_the_original_parent() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let own = make(&mut c, 0, &drops);
    let parent = c.start(&OWNER, vec![own]).unwrap();
    let unrelated = c.start(&OWNER, vec![V::U32(91)]).unwrap();
    let base = c.parked[&parent.id].machine.stack[0].base();
    let captured = make(&mut c, 1, &drops);
    let mut a = queue(
        &mut c,
        &parent,
        &CHILD,
        vec![
            captured,
            V::Reference {
                evidence: Default::default(),
                ty: "&probe".into(),
                root: base,
                fields: vec![],
            },
        ],
    );
    let mut b = queue(&mut c, &parent, &CHILD, vec![V::U32(2), V::Unit]);
    reject(&mut c, &unrelated, &mut a);
    reject(&mut c, &parent, &mut b);
    let child = c.start_task(&parent, &mut a).unwrap();
    assert!(a.is_none());
    reject(&mut c, &parent, &mut a);
    let sibling = c.start_task(&parent, &mut b).unwrap();
    assert!(c.advance_retained(&parent, 1).is_err());
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Paused
    ));
    let wake = c.park_for_readiness(&child).unwrap();
    let before = snapshot(&c);
    assert!(c.advance_retained(&child, 1).is_err());
    assert_eq!(snapshot(&c), before);
    c.advance_retained(&sibling, 1).unwrap();
    let sibling_wake = c.park_for_readiness(&sibling).unwrap();
    c.signal_readiness(&sibling_wake).unwrap();
    c.signal_readiness(&wake).unwrap();
    let before = snapshot(&c);
    assert!(c.advance_retained(&child, 1).is_err());
    assert_eq!(snapshot(&c), before);
    assert!(matches!(
        c.advance_retained(&sibling, 1).unwrap(),
        ContextPoll::Ready
    ));
    let before = snapshot(&c);
    assert!(c.signal_readiness(&wake).is_err());
    assert_eq!(snapshot(&c), before);
    assert!(matches!(
        c.advance_retained(&child, 0).unwrap(),
        ContextPoll::Paused
    ));
    assert_eq!(snapshot(&c), before);
    assert!(matches!(
        c.advance_retained(&child, 1).unwrap(),
        ContextPoll::Ready
    ));
    let before = snapshot(&c);
    assert!(c.join(Some(&unrelated), &child).is_err());
    assert!(c.join(None, &child).is_err());
    assert_eq!(snapshot(&c), before);
    let result = fields(&mut c, Some(&parent), &child);
    assert!(matches!(result[1],V::Reference{root,..} if root==base));
    assert!(drops.borrow().is_empty());
    let before = snapshot(&c);
    assert!(c.join(Some(&parent), &child).is_err());
    assert!(c.signal_readiness(&wake).is_err());
    assert_eq!(snapshot(&c), before);
    release(&mut c, result);
    assert_eq!(*drops.borrow(), [1]);
    assert_eq!(
        fields(&mut c, Some(&parent), &sibling),
        [V::U32(2), V::Unit]
    );
    assert!(matches!(
        c.advance_retained(&parent, 2).unwrap(),
        ContextPoll::Ready
    ));
    let result = fields(&mut c, None, &parent);
    release(&mut c, result);
    assert!(matches!(
        c.advance_retained(&unrelated, 2).unwrap(),
        ContextPoll::Ready
    ));
    assert_eq!(fields(&mut c, None, &unrelated), [V::U32(91)]);
    assert_eq!(*drops.borrow(), [1, 0]);
    assert_eq!(c.host.fuel, Some(992));
    empty(&c);
}
#[test]
fn queued_admission_checks_all_capacity_and_layout_before_capture_transfer() {
    for mode in 0..9 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let parent = c.start(&OWNER, vec![V::Unit]).unwrap();
        let control = match mode {
            0 => &HUGE,
            1 => &BAD,
            _ => &CHILD,
        };
        let own = make(&mut c, 1, &drops);
        let token = own.clone();
        let other = match mode {
            6 => own.clone(),
            7 => V::Opaque {
                ty: "probe".into(),
                token: "rust:foreign".into(),
            },
            _ => V::Unit,
        };
        let mut q = queue(&mut c, &parent, control, vec![own, other]);
        match mode {
            2 => c.next = u64::MAX,
            3 => c.values.exhaust_identity_space(),
            4 => c.host.tasks.exhaust_owner_space(),
            5 => c.host.resources.next_scope = u64::MAX,
            8 => c
                .domains
                .stop(&c.parked[&parent.id].machine.current.clone())
                .unwrap(),
            _ => {}
        }
        reject(&mut c, &parent, &mut q);
        assert!(drops.borrow().is_empty());
        assert!(c.host.resources.get::<Probe>(&token, "probe").is_ok());
        assert_eq!(c.host.tasks.live_counts(), (1, 1));
        assert_eq!(c.host.fuel, Some(1000));
        c.terminate().unwrap();
        assert!(c.host.tasks.is_empty());
        let before = snapshot(&c);
        assert!(c.start_task(&parent, &mut q).is_err());
        assert_eq!(snapshot(&c), before);
        c.host.release_frame(&[Some(token)]);
        assert_eq!(*drops.borrow(), [1]);
        empty(&c);
    }
}
#[test]
fn stopped_task_contexts_retire_results_and_waiters_before_suppliers() {
    for process_stop in [false, true] {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let drops = Rc::new(RefCell::new(vec![]));
        let own = make(&mut c, 0, &drops);
        let parent = c.start(&OWNER, vec![own]).unwrap();
        let base = c.parked[&parent.id].machine.stack[0].base();
        let own = make(&mut c, 1, &drops);
        let mut q = queue(
            &mut c,
            &parent,
            &CHILD,
            vec![
                own,
                V::Reference {
                    evidence: Default::default(),
                    ty: "&probe".into(),
                    root: base,
                    fields: vec![],
                },
            ],
        );
        let done = c.start_task(&parent, &mut q).unwrap();
        assert!(matches!(
            c.advance_retained(&done, 2).unwrap(),
            ContextPoll::Ready
        ));
        let own = make(&mut c, 2, &drops);
        let mut q = queue(
            &mut c,
            &parent,
            &CHILD,
            vec![
                own,
                V::Reference {
                    evidence: Default::default(),
                    ty: "&probe".into(),
                    root: base,
                    fields: vec![],
                },
            ],
        );
        let waiting = c.start_task(&parent, &mut q).unwrap();
        c.advance_retained(&waiting, 1).unwrap();
        let wake = c.park_for_readiness(&waiting).unwrap();
        let independent = c.start(&OWNER, vec![V::U32(91)]).unwrap();
        if process_stop {
            let stop = c.start(&PROCESS, vec![]).unwrap();
            assert!(matches!(
                c.advance_retained(&stop, 1).unwrap(),
                ContextPoll::Complete(Completion::Process(23))
            ));
            empty(&c);
        } else {
            let mut q = queue(&mut c, &parent, &FAULT, vec![]);
            let fault = c.start_task(&parent, &mut q).unwrap();
            assert!(matches!(
                c.advance_retained(&fault, 1).unwrap(),
                ContextPoll::Failed(_)
            ));
            assert!(matches!(
                c.advance_retained(&parent, 1).unwrap(),
                ContextPoll::Failed(_)
            ));
            assert!(matches!(
                c.advance_retained(&independent, 2).unwrap(),
                ContextPoll::Ready
            ));
            assert_eq!(fields(&mut c, None, &independent), [V::U32(91)]);
            empty(&c);
        }
        assert_eq!(*drops.borrow(), [1, 2, 0]);
        let before = snapshot(&c);
        assert!(c.signal_readiness(&wake).is_err());
        assert!(c.join(Some(&parent), &done).is_err());
        assert_eq!(snapshot(&c), before);
    }
}
fn nested(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(4, 1)?;
    a.pc += 1;
    Ok(Step::Call {
        control: &FAULT,
        arguments: vec![],
        destination: None,
        deliver: |_, _, _| Ok(()),
    })
}
static NESTED: Control = Control {
    name: "nested",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    access: None,
    resume: nested,
};
#[test]
fn queued_task_cannot_rebind_to_a_nested_invocation_in_the_same_context() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let parent = c.start(&NESTED, vec![]).unwrap();
    let mut q = queue(&mut c, &parent, &CHILD, vec![V::U32(1), V::Unit]);
    c.advance_retained(&parent, 1).unwrap();
    assert_eq!(c.parked[&parent.id].machine.stack.len(), 2);
    reject(&mut c, &parent, &mut q);
    c.terminate().unwrap();
    empty(&c);
}

static BOUNDARY: BoundaryControl = BoundaryControl {
    name: "owning",
    body: 1,
    handler: 3,
    after: 4,
    retained: &[],
    retire: &[0],
    work: &[0],
    handling: &[],
    fault_slot: 1,
};
fn supervised(h: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(5, 1)?;
    match a.pc {
        0 => Ok(Step::Boundary(&BOUNDARY)),
        1 => {
            a.pc = 2;
            Ok(Step::Continue)
        }
        3 => {
            h.events.push(serde_json::json!({"handled":true}));
            Ok(Step::Complete(Completion::Outcome((
                "handled".into(),
                vec![],
            ))))
        }
        _ => panic!("unexpected owning continuation"),
    }
}
static SUPERVISED: Control = Control {
    name: "supervised",
    slots: 2,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: supervised,
};
#[test]
fn owning_contingency_resumes_only_after_bound_task_resources_and_waits_retire() {
    let mut h = host();
    let mut c = Contexts::new(&mut h);
    let drops = Rc::new(RefCell::new(vec![]));
    let own = make(&mut c, 0, &drops);
    let parent = c.start(&SUPERVISED, vec![own]).unwrap();
    c.advance_retained(&parent, 1).unwrap();
    let base = c.parked[&parent.id].machine.stack[0].base();
    let own = make(&mut c, 1, &drops);
    let mut q = queue(
        &mut c,
        &parent,
        &CHILD,
        vec![
            own,
            V::Reference {
                evidence: Default::default(),
                ty: "&probe".into(),
                root: base,
                fields: vec![],
            },
        ],
    );
    let done = c.start_task(&parent, &mut q).unwrap();
    c.advance_retained(&done, 2).unwrap();
    let own = make(&mut c, 2, &drops);
    let mut q = queue(
        &mut c,
        &parent,
        &CHILD,
        vec![
            own,
            V::Reference {
                evidence: Default::default(),
                ty: "&probe".into(),
                root: base,
                fields: vec![],
            },
        ],
    );
    let waiting = c.start_task(&parent, &mut q).unwrap();
    c.advance_retained(&waiting, 1).unwrap();
    let wake = c.park_for_readiness(&waiting).unwrap();
    let own = make(&mut c, 9, &drops);
    let independent = c.start(&OWNER, vec![own]).unwrap();
    let mut q = queue(&mut c, &parent, &FAULT, vec![]);
    let fault = c.start_task(&parent, &mut q).unwrap();
    assert!(matches!(
        c.advance_retained(&fault, 1).unwrap(),
        ContextPoll::Failed(_)
    ));
    assert_eq!(*drops.borrow(), [1, 2, 0]);
    assert_eq!(c.host.events.len(), 1);
    assert_eq!(c.host.events[0]["kind"], "fault");
    assert!(c.signal_readiness(&wake).is_err());
    assert_eq!(c.host.tasks.live_counts(), (2, 0));
    assert!(matches!(
        c.advance_retained(&parent, 1).unwrap(),
        ContextPoll::Ready
    ));
    assert!(fields(&mut c, None, &parent).is_empty());
    assert_eq!(c.host.events.len(), 2);
    assert_eq!(c.host.events[1], serde_json::json!({"handled":true}));
    c.advance_retained(&independent, 2).unwrap();
    let result = fields(&mut c, None, &independent);
    release(&mut c, result);
    assert_eq!(*drops.borrow(), [1, 2, 0, 9]);
    empty(&c);
}
