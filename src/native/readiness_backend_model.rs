// Deterministic backend model, embedded only in the generated owned-operation test.
// Notifications retain opaque IDs, never adapter state or resource references.
use super::*;
use std::collections::VecDeque;
#[derive(Clone)]
pub struct Event {
    backend: Rc<()>,
    id: u64,
}
struct Entry {
    request: String,
    deadline: u64,
    resource: V,
    trace: Trace,
    alive: Rc<Cell<bool>>,
    token: Option<Readiness>,
    delivered: bool,
}
struct State {
    identity: Rc<()>,
    next: u64,
    entries: BTreeMap<u64, Entry>,
    queue: VecDeque<Event>,
}
#[derive(Clone)]
pub struct Backend(Rc<RefCell<State>>);
#[derive(Clone, Copy)]
pub enum Failure {
    None,
    BeforeInsert,
    AfterInsert,
}
pub struct Subscription {
    backend: Backend,
    id: u64,
}
pub struct Reservation(Option<Subscription>);
impl Drop for Subscription {
    fn drop(&mut self) {
        let entry = self
            .backend
            .0
            .borrow_mut()
            .entries
            .remove(&self.id)
            .unwrap();
        assert!(entry.alive.get(), "subscription outlived supplier");
        entry
            .trace
            .borrow_mut()
            .push(format!("unsubscribe {} {}", entry.request, self.id));
    }
}
impl Reservation {
    pub fn commit(mut self, token: Readiness) -> Subscription {
        let subscription = self.0.take().unwrap();
        let mut backend = subscription.backend.0.borrow_mut();
        let entry = backend.entries.get_mut(&subscription.id).unwrap();
        assert!(entry.token.is_none());
        entry.token = Some(token);
        drop(backend);
        subscription
    }
}
impl Backend {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(State {
            identity: Rc::new(()),
            next: 0,
            entries: BTreeMap::new(),
            queue: VecDeque::new(),
        })))
    }
    pub fn active(&self) -> usize {
        self.0.borrow().entries.len()
    }
    pub fn prepare(&self, operation: &Operation, failure: Failure) -> R<Reservation> {
        if matches!(failure, Failure::BeforeInsert) {
            return Err("E_MODEL: backend rejected before insertion".into());
        }
        let mut state = self.0.borrow_mut();
        let id = state
            .next
            .checked_add(1)
            .ok_or("E_MODEL: backend IDs exhausted")?;
        assert_eq!(operation.interest.deadline, operation.deadline);
        assert!(operation.lease.alive.get());
        state.entries.insert(
            id,
            Entry {
                request: operation.lease.request.clone(),
                deadline: operation.interest.deadline,
                resource: operation.resource.clone(),
                trace: operation.lease.trace.clone(),
                alive: operation.lease.alive.clone(),
                token: None,
                delivered: false,
            },
        );
        state.next = id;
        operation
            .lease
            .trace
            .borrow_mut()
            .push(format!("subscribe {} {id}", operation.lease.request));
        let event = Event {
            backend: state.identity.clone(),
            id,
        };
        // Simulate a backend event already queued before registration is committed.
        state.queue.push_back(event.clone());
        drop(state);
        let reservation = Reservation(Some(Subscription {
            backend: self.clone(),
            id,
        }));
        assert!(
            self.deliver(&event).is_err(),
            "tentative interest became eligible"
        );
        if matches!(failure, Failure::AfterInsert) {
            // Reservation Drop removes the inserted interest even after an event
            // has escaped into the backend queue. That event must remain stale.
            return Err("E_MODEL: backend rejected after insertion".into());
        }
        Ok(reservation)
    }
    fn deliver(&self, event: &Event) -> R<Readiness> {
        let mut state = self.0.borrow_mut();
        if !Rc::ptr_eq(&state.identity, &event.backend) {
            return Err("E_MODEL: foreign backend event".into());
        }
        let entry = state
            .entries
            .get_mut(&event.id)
            .ok_or("E_MODEL: removed backend interest")?;
        let token = entry
            .token
            .as_ref()
            .ok_or("E_MODEL: interest not committed")?;
        if entry.delivered {
            return Err("E_MODEL: duplicate backend event".into());
        }
        assert!(entry.alive.get());
        assert_eq!(entry.deadline, 57);
        assert!(matches!(entry.resource, V::Opaque { .. }));
        entry.delivered = true;
        Ok(token.clone())
    }
    pub fn signal(&self, expected: &Readiness) -> R<Readiness> {
        // Exercise queued failures/old generations while newer registrations
        // are still live; rejection must not consume any replacement interest.
        let stale = {
            let state = self.0.borrow();
            state
                .queue
                .iter()
                .filter(|event| {
                    state
                        .entries
                        .get(&event.id)
                        .is_none_or(|entry| entry.delivered)
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let active = self.active();
        for event in stale {
            assert!(self.deliver(&event).is_err());
        }
        assert_eq!(self.active(), active);
        // The test scheduler chooses when this deterministic backend reports the
        // selected interest; queued events for other interests stay queued.
        let event = {
            let mut state = self.0.borrow_mut();
            let id = *state
                .entries
                .iter()
                .find(|(_, entry)| {
                    entry
                        .token
                        .as_ref()
                        .is_some_and(|t| RcOrArc::same(t, expected))
                })
                .map(|(id, _)| id)
                .ok_or("E_MODEL: missing backend interest")?;
            let at = state.queue.iter().position(|event| event.id == id).unwrap();
            state.queue.remove(at).unwrap()
        };
        let foreign = Event {
            backend: Rc::new(()),
            id: event.id,
        };
        assert!(self.deliver(&foreign).is_err());
        let token = self.deliver(&event)?;
        assert!(self.deliver(&event).is_err());
        self.0.borrow_mut().queue.push_back(event);
        Ok(token)
    }
    pub fn finish(&self) {
        assert_eq!(self.active(), 0);
        loop {
            let event = self.0.borrow_mut().queue.pop_front();
            let Some(event) = event else { break };
            assert!(self.deliver(&event).is_err());
        }
    }
}
// Readiness session identity is an Arc, independent of backend identity.
struct RcOrArc;
impl RcOrArc {
    fn same(a: &Readiness, b: &Readiness) -> bool {
        Arc::ptr_eq(&a.session, &b.session)
            && a.context == b.context
            && a.generation == b.generation
    }
}
