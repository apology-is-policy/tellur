//! Private pending-operation lifetime model; no public adapter ABI or source handoff.
use super::super::super::{frames::Storage, tasks::OwnerId};
use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Trace = Rc<RefCell<Vec<String>>>;
struct Probe {
    alive: Rc<Cell<bool>>,
    trace: Trace,
}
impl Drop for Probe {
    fn drop(&mut self) {
        assert!(self.alive.replace(false), "resource destroyed twice");
        self.trace.borrow_mut().push("resource drop".into());
    }
}
struct Lease {
    alive: Rc<Cell<bool>>,
    trace: Trace,
    request: String,
}
impl Drop for Lease {
    fn drop(&mut self) {
        assert!(self.alive.get(), "operation outlived its borrowed resource");
        self.trace
            .borrow_mut()
            .push(format!("drop {}", self.request));
    }
}
/// Retains both an actual host object and the identities that authorize its loan.
/// The partial buffer and fixed deadline belong to this record, not a callback stack.
pub struct Operation {
    identity: Arc<()>,
    owner: OwnerId,
    registry: Arc<()>,
    domain: DomainKey,
    activation: Storage,
    supplier: Storage,
    resource: V,
    arguments: Vec<V>,
    lease: Lease,
    partial: String,
    deadline: u64,
    result: Option<Completion>,
    delivery: Deliver,
}
impl Operation {
    pub fn new(
        h: &Host,
        v: &Frames,
        a: &Activation,
        domain: DomainKey,
        mut arguments: Vec<V>,
        delivery: Deliver,
    ) -> R<Self> {
        let [V::Reference { root, fields, .. }, V::String(_)] = arguments.as_slice() else {
            return Err("E_MODEL: expected resource loan and owned request".into());
        };
        if !fields.is_empty() {
            return Err("E_MODEL: expected whole resource supplier".into());
        }
        let supplier = v.storage_at(*root)?;
        let resource = v[*root].as_ref().ok_or("E_MODEL: empty supplier")?.clone();
        let probe = h.resources.get::<Probe>(&resource, "probe")?;
        let V::String(request) = arguments.pop().unwrap() else {
            unreachable!()
        };
        probe.trace.borrow_mut().push(format!("start {request}"));
        Ok(Self {
            identity: Arc::new(()),
            owner: a.owner,
            registry: a.registry.clone(),
            domain,
            activation: v.local_storage(&a.frame, 0)?,
            supplier,
            resource,
            arguments,
            lease: Lease {
                alive: probe.alive.clone(),
                trace: probe.trace.clone(),
                request,
            },
            partial: String::new(),
            deadline: 57,
            result: None,
            delivery,
        })
    }
    pub fn require(
        &self,
        h: &Host,
        v: &Frames,
        a: &Activation,
        domains: &Domains,
        domain: &DomainKey,
    ) -> R<()> {
        a.require_task_host(h)?;
        if self.owner != a.owner
            || !Arc::ptr_eq(&self.registry, &a.registry)
            || !domains.same(&self.domain, domain)
            || self.activation != v.local_storage(&a.frame, 0)?
        {
            return Err("E_MODEL: operation invocation/domain changed".into());
        }
        let root = v.require_storage(&self.supplier)?;
        if v[root].as_ref() != Some(&self.resource) {
            return Err("E_MODEL: borrowed resource identity changed".into());
        }
        let probe = h.resources.get::<Probe>(&self.resource, "probe")?;
        if !Rc::ptr_eq(&probe.alive, &self.lease.alive) || !probe.alive.get() {
            return Err("E_MODEL: borrowed host object changed".into());
        }
        Dependencies::describe(v, &self.arguments).require_live(v)?;
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.result.is_some()
    }
    fn poll(&mut self, bytes: &str) -> R<()> {
        if self.ready() {
            return Err("E_MODEL: operation already completed".into());
        }
        self.partial.push_str(bytes);
        self.lease.trace.borrow_mut().push(format!(
            "poll {} {} @{}",
            self.lease.request, self.partial, self.deadline
        ));
        Ok(())
    }
    fn complete(&mut self, label: &str, fields: Vec<V>) -> R<()> {
        if self.ready() {
            return Err("E_MODEL: duplicate operation completion".into());
        }
        if label != "ready" || !matches!(fields.as_slice(), [V::String(_)]) {
            return Err("E_MODEL: invalid completion contract".into());
        }
        self.result = Some(Completion::Outcome((label.into(), fields)));
        Ok(())
    }
    pub fn deliver(mut self, v: &mut Frames, a: &mut Activation) -> R<()> {
        let result = self.result.take().ok_or("E_MODEL: result not ready")?;
        self.lease
            .trace
            .borrow_mut()
            .push(format!("deliver {}", self.lease.request));
        let delivery = self.delivery;
        // The adapter is no longer pending when source result adoption begins.
        drop(self);
        delivery(a, v, result)
    }
}

#[derive(Clone)]
struct Ticket {
    context: ContextKey,
    operation: Arc<()>,
}
fn operation<'a>(c: &'a mut Contexts<'_>, ticket: &Ticket) -> R<&'a mut Operation> {
    c.require(&ticket.context)?;
    let machine = &mut c.parked.get_mut(&ticket.context.id).unwrap().machine;
    let frame = machine.stack.last_mut().ok_or("E_MODEL: no invocation")?;
    let Some(Pending::Adapter(op)) = &frame.pending else {
        return Err("E_MODEL: no pending operation".into());
    };
    if !Arc::ptr_eq(&op.identity, &ticket.operation) {
        return Err("E_MODEL: foreign operation identity".into());
    }
    op.require(c.host, &c.values, frame, &c.domains, &machine.current)?;
    let Some(Pending::Adapter(op)) = &mut frame.pending else {
        unreachable!()
    };
    Ok(op)
}
fn ticket(c: &Contexts<'_>, key: &ContextKey) -> Ticket {
    let Some(Pending::Adapter(op)) = &c.parked[&key.id].machine.stack.last().unwrap().pending
    else {
        panic!("missing operation")
    };
    Ticket {
        context: key.clone(),
        operation: op.identity.clone(),
    }
}
fn returning(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(1, 1)?;
    Ok(Step::Complete(Completion::Outcome((
        "ready".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
fn working(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.tick(2, 1)?;
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Adapter {
            arguments: vec![
                v[a.base()].as_ref().unwrap().clone(),
                v[a.base() + 1].take().unwrap(),
            ],
            deliver: |a, v, outcome| {
                let Completion::Outcome((_, mut fields)) = outcome else {
                    unreachable!()
                };
                assert!(v[a.base() + 1].is_none(), "request was replayed");
                v[a.base() + 1] = fields.pop();
                Ok(())
            },
        });
    }
    Ok(Step::Complete(Completion::Outcome((
        "ready".into(),
        vec![v[a.base()].take().unwrap(), v[a.base() + 1].take().unwrap()],
    ))))
}
fn failing(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(3, 1)?;
    h.fault(V::String("operation sibling failed".into()))
}
fn stopping(h: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    h.tick(4, 1)?;
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
    name: "operation",
    slots: 2,
    atoms: 0,
    parameters: &[0, 1],
    retained: &[],
    constructor: false,
    access: None,
    resume: working,
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
    resume: stopping,
};
fn owner(c: &mut Contexts<'_>, trace: &Trace) -> ContextKey {
    let resource = c
        .host
        .resources
        .insert(
            "probe",
            Probe {
                alive: Rc::new(Cell::new(true)),
                trace: trace.clone(),
            },
        )
        .unwrap();
    c.start(&OWNER, vec![resource]).unwrap()
}
fn child(c: &mut Contexts<'_>, parent: &ContextKey, name: &str) -> ContextKey {
    let base = c.parked[&parent.id].machine.stack[0].base();
    let loan = reference_evidence::attach(
        &c.values,
        super::super::super::reference(&c.values, base, &[], false, "&probe").unwrap(),
        &[reference_evidence::Origin {
            root: base,
            fields: &[],
            indirect: false,
            guard: true,
            opaque: true,
        }],
    );
    c.start_child(parent, &CHILD, vec![loan, V::String(name.into())])
        .unwrap()
}
fn fields(c: &mut Contexts<'_>, parent: Option<&ContextKey>, key: &ContextKey) -> Vec<V> {
    let Completion::Outcome((label, fields)) = c.join(parent, key).unwrap() else {
        panic!("expected normal completion")
    };
    assert_eq!(label, "ready");
    fields
}
fn empty(c: &Contexts<'_>) {
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.waits.is_empty());
    assert!(c.host.resources.values.is_empty() && c.host.tasks.is_empty());
    assert_eq!(c.domains.len(), 0);
    assert_eq!(c.domains.member_count(), 0);
    assert!(c.values.iter().all(Option::is_none));
}
fn snapshot(c: &Contexts<'_>) -> String {
    format!(
        "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
        &*c.values,
        c.host.fuel,
        c.parked.keys(),
        c.completed.keys(),
        c.waits,
        c.host.tasks,
        c.parked
            .iter()
            .map(|(id, p)| (
                id,
                p.machine
                    .stack
                    .iter()
                    .map(|a| (
                        a.pc,
                        match &a.pending {
                            Some(Pending::Adapter(op)) => Some((
                                op.partial.clone(),
                                op.deadline,
                                op.ready(),
                                op.lease.trace.borrow().clone()
                            )),
                            _ => None,
                        }
                    ))
                    .collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>()
    )
}

#[test]
fn overlapping_operations_retain_partial_state_and_deliver_once() {
    for reverse in [false, true] {
        for quota in [1, 3, 99] {
            let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
            let mut c = Contexts::new(&mut h);
            let trace = Trace::default();
            let parent = owner(&mut c, &trace);
            let left = child(&mut c, &parent, "left");
            let right = child(&mut c, &parent, "right");
            let mut children = [left, right];
            for key in &children {
                assert!(matches!(
                    c.advance_retained(key, quota).unwrap(),
                    ContextPoll::Paused
                ));
                let base = c.parked[&key.id].machine.stack[0].base();
                assert!(c.values[base + 1].is_none());
                let before = snapshot(&c);
                assert!(matches!(
                    c.advance_retained(key, quota).unwrap(),
                    ContextPoll::Paused
                ));
                assert_eq!(snapshot(&c), before);
            }
            if reverse {
                children.reverse();
            }
            for key in &children {
                let t = ticket(&c, key);
                for chunk in ["A", "", "B"] {
                    let fuel = c.host.fuel;
                    operation(&mut c, &t).unwrap().poll(chunk).unwrap();
                    let wake = c.park_for_readiness(key).unwrap();
                    let before = snapshot(&c);
                    assert!(c.advance_retained(key, quota).is_err());
                    assert_eq!(snapshot(&c), before);
                    c.signal_readiness(&wake).unwrap();
                    let before = snapshot(&c);
                    assert!(c.signal_readiness(&wake).is_err());
                    assert!(matches!(
                        c.advance_retained(key, 0).unwrap(),
                        ContextPoll::Paused
                    ));
                    assert_eq!(snapshot(&c), before);
                    assert!(matches!(
                        c.advance_retained(key, quota).unwrap(),
                        ContextPoll::Paused
                    ));
                    assert!(c.signal_readiness(&wake).is_err());
                    assert_eq!(c.host.fuel, fuel);
                }
                let op = operation(&mut c, &t).unwrap();
                assert_eq!(op.partial, "AB");
                let answer = format!("{}:{}", op.lease.request, op.partial);
                op.complete("ready", vec![V::String(answer.clone())])
                    .unwrap();
                let before = snapshot(&c);
                assert!(
                    operation(&mut c, &t)
                        .unwrap()
                        .complete("ready", vec![V::String("twice".into())])
                        .is_err()
                );
                assert!(operation(&mut c, &t).unwrap().poll("late").is_err());
                assert_eq!(snapshot(&c), before);
                assert!(matches!(
                    c.advance_retained(key, quota).unwrap(),
                    ContextPoll::Ready
                ));
                assert!(c.advance_retained(&parent, quota).is_err());
                let result = fields(&mut c, Some(&parent), key);
                assert!(matches!(result[0], V::Reference { .. }));
                assert_eq!(result[1], V::String(answer));
                assert!(operation(&mut c, &t).is_err());
                assert!(c.join(Some(&parent), key).is_err());
            }
            assert!(matches!(
                c.advance_retained(&parent, quota).unwrap(),
                ContextPoll::Ready
            ));
            let result = fields(&mut c, None, &parent);
            c.host
                .release_frame(&result.into_iter().map(Some).collect::<Vec<_>>());
            let log = trace.borrow();
            assert_eq!(log.iter().filter(|x| x.starts_with("start ")).count(), 2);
            assert_eq!(log.iter().filter(|x| x.starts_with("deliver ")).count(), 2);
            assert_eq!(log.iter().filter(|x| x.starts_with("drop ")).count(), 2);
            assert_eq!(log.last().unwrap(), "resource drop");
            assert_eq!(c.host.fuel, Some(95));
            empty(&c);
        }
    }
}

#[test]
fn operation_and_completion_rejections_preserve_pending_state() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let mut c = Contexts::new(&mut h);
    let trace = Trace::default();
    let parent = owner(&mut c, &trace);
    let first = child(&mut c, &parent, "first");
    let second = child(&mut c, &parent, "second");
    c.advance_retained(&first, 1).unwrap();
    c.advance_retained(&second, 1).unwrap();
    let t = ticket(&c, &first);
    operation(&mut c, &t).unwrap().poll("partial").unwrap();
    for (label, fields) in [
        ("wrong", vec![V::String("ok".into())]),
        ("ready", vec![]),
        ("ready", vec![V::U32(1)]),
        ("ready", vec![V::String("a".into()), V::String("b".into())]),
    ] {
        let before = snapshot(&c);
        assert!(
            operation(&mut c, &t)
                .unwrap()
                .complete(label, fields)
                .is_err()
        );
        assert_eq!(snapshot(&c), before);
    }
    for bad in [
        Ticket {
            context: second.clone(),
            operation: t.operation.clone(),
        },
        Ticket {
            context: ContextKey {
                session: Arc::new(()),
                id: first.id,
            },
            operation: t.operation.clone(),
        },
        Ticket {
            context: first.clone(),
            operation: Arc::new(()),
        },
    ] {
        let before = snapshot(&c);
        assert!(operation(&mut c, &bad).is_err());
        assert_eq!(snapshot(&c), before);
    }
    let previous = c.next_readiness;
    c.next_readiness = u64::MAX;
    let before = snapshot(&c);
    assert!(c.park_for_readiness(&first).is_err());
    assert_eq!(snapshot(&c), before);
    c.next_readiness = previous;
    c.terminate().unwrap();
    assert_eq!(trace.borrow().last().unwrap(), "resource drop");
    empty(&c);
}

#[test]
fn fault_and_process_stop_destroy_operations_before_real_suppliers() {
    for mode in 0..3 {
        for process in [false, true] {
            for quota in [1, 99] {
                let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
                let mut c = Contexts::new(&mut h);
                let trace = Trace::default();
                let parent = owner(&mut c, &trace);
                let children = [
                    child(&mut c, &parent, "left"),
                    child(&mut c, &parent, "right"),
                ];
                let unrelated = c.start(&OWNER, vec![V::U32(91)]).unwrap();
                let mut wakes = vec![];
                let mut tickets = vec![];
                for key in &children {
                    c.advance_retained(key, quota).unwrap();
                    let t = ticket(&c, key);
                    operation(&mut c, &t).unwrap().poll("partial").unwrap();
                    if mode == 2 {
                        operation(&mut c, &t)
                            .unwrap()
                            .complete("ready", vec![V::String("unused".into())])
                            .unwrap();
                    }
                    wakes.push(c.park_for_readiness(key).unwrap());
                    tickets.push(t);
                }
                if mode > 0 {
                    for wake in &wakes {
                        c.signal_readiness(wake).unwrap();
                    }
                }
                if process {
                    let stop = c.start(&PROCESS, vec![]).unwrap();
                    assert!(matches!(
                        c.advance_retained(&stop, quota).unwrap(),
                        ContextPoll::Complete(Completion::Process(23))
                    ));
                } else {
                    let fault = c.start_child(&parent, &FAULT, vec![]).unwrap();
                    assert!(matches!(
                        c.advance_retained(&fault, quota).unwrap(),
                        ContextPoll::Failed(_)
                    ));
                    assert!(matches!(
                        c.advance_retained(&parent, quota).unwrap(),
                        ContextPoll::Failed(_)
                    ));
                    assert!(matches!(
                        c.advance_retained(&unrelated, quota).unwrap(),
                        ContextPoll::Ready
                    ));
                    assert_eq!(fields(&mut c, None, &unrelated), [V::U32(91)]);
                }
                let before = snapshot(&c);
                for wake in &wakes {
                    assert!(c.signal_readiness(wake).is_err());
                }
                for t in &tickets {
                    assert!(operation(&mut c, t).is_err());
                }
                assert_eq!(snapshot(&c), before);
                assert_eq!(
                    *trace.borrow(),
                    [
                        "start left",
                        "poll left partial @57",
                        "start right",
                        "poll right partial @57",
                        "drop right",
                        "drop left",
                        "resource drop"
                    ]
                );
                empty(&c);
            }
        }
    }
}

#[test]
fn operation_delivery_is_administrative_at_every_fuel_cutoff() {
    for fuel in 0..=4 {
        for quota in [1, 3, 99] {
            let mut h = Host::new(serde_json::json!({}), None, true, Some(fuel));
            let mut c = Contexts::new(&mut h);
            let trace = Trace::default();
            let parent = owner(&mut c, &trace);
            let key = child(&mut c, &parent, "request");
            let first = c.advance_retained(&key, quota).unwrap();
            if fuel == 0 {
                assert!(matches!(first, ContextPoll::Failed(_)));
                assert!(matches!(
                    c.advance_retained(&parent, quota).unwrap(),
                    ContextPoll::Failed(_)
                ));
                assert_eq!(*trace.borrow(), ["resource drop"]);
            } else {
                assert!(matches!(first, ContextPoll::Paused));
                let t = ticket(&c, &key);
                for _ in 0..3 {
                    operation(&mut c, &t).unwrap().poll("").unwrap();
                    let wake = c.park_for_readiness(&key).unwrap();
                    c.signal_readiness(&wake).unwrap();
                    assert!(matches!(
                        c.advance_retained(&key, quota).unwrap(),
                        ContextPoll::Paused
                    ));
                    assert_eq!(c.host.fuel, Some(fuel - 1));
                }
                operation(&mut c, &t)
                    .unwrap()
                    .complete("ready", vec![V::String("done".into())])
                    .unwrap();
                let outcome = c.advance_retained(&key, quota).unwrap();
                if fuel == 1 {
                    assert!(matches!(outcome, ContextPoll::Failed(_)));
                    assert!(matches!(
                        c.advance_retained(&parent, quota).unwrap(),
                        ContextPoll::Failed(_)
                    ));
                } else {
                    assert!(matches!(outcome, ContextPoll::Ready));
                    assert_eq!(
                        fields(&mut c, Some(&parent), &key)[1],
                        V::String("done".into())
                    );
                    let outcome = c.advance_retained(&parent, quota).unwrap();
                    if fuel == 2 {
                        assert!(matches!(outcome, ContextPoll::Failed(_)));
                    } else {
                        assert!(matches!(outcome, ContextPoll::Ready));
                        let result = fields(&mut c, None, &parent);
                        c.host
                            .release_frame(&result.into_iter().map(Some).collect::<Vec<_>>());
                        assert_eq!(c.host.fuel, Some(fuel - 3));
                    }
                }
                let log = trace.borrow();
                assert_eq!(log.iter().filter(|x| *x == "deliver request").count(), 1);
                assert_eq!(log.iter().filter(|x| *x == "drop request").count(), 1);
                assert_eq!(log.last().unwrap(), "resource drop");
            }
            empty(&c);
        }
    }
}

#[test]
fn pending_operation_keeps_original_host_invocation_domain_and_supplier() {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(100));
    let mut c = Contexts::new(&mut h);
    let trace = Trace::default();
    let parent = owner(&mut c, &trace);
    let key = child(&mut c, &parent, "original");
    c.advance_retained(&key, 1).unwrap();
    let t = ticket(&c, &key);
    let other_trace = Trace::default();
    let other = owner(&mut c, &other_trace);
    let foreign = Host::new(serde_json::json!({}), None, true, Some(100));
    let original = &c.parked[&key.id].machine;
    let original_frame = original.stack.last().unwrap();
    let unrelated = &c.parked[&other.id].machine;
    let Some(Pending::Adapter(op)) = &original_frame.pending else {
        unreachable!()
    };
    let before = snapshot(&c);
    assert!(
        op.require(
            &foreign,
            &c.values,
            original_frame,
            &c.domains,
            &original.current
        )
        .is_err()
    );
    assert!(
        op.require(
            c.host,
            &c.values,
            unrelated.stack.last().unwrap(),
            &c.domains,
            &original.current
        )
        .is_err()
    );
    assert!(
        op.require(
            c.host,
            &c.values,
            original_frame,
            &c.domains,
            &unrelated.current
        )
        .is_err()
    );
    assert_eq!(snapshot(&c), before);

    // Replacing a supplier slot with another live object of the same Rust type
    // must not silently redirect the pending operation's loan.
    let base = c.parked[&parent.id].machine.stack[0].base();
    let other_base = c.parked[&other.id].machine.stack[0].base();
    c.values.swap(base, other_base);
    let before = snapshot(&c);
    assert!(operation(&mut c, &t).is_err());
    assert_eq!(snapshot(&c), before);
    c.values.swap(base, other_base);
    operation(&mut c, &t)
        .unwrap()
        .poll("still original")
        .unwrap();
    c.terminate().unwrap();
    assert_eq!(
        *trace.borrow(),
        [
            "start original",
            "poll original still original @57",
            "drop original",
            "resource drop"
        ]
    );
    assert_eq!(*other_trace.borrow(), ["resource drop"]);
    empty(&c);
}
