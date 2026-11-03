// Appended only to the test Rust host wrapper, never to a public adapter.
use crate::owned_operation_contract::{Adapter, Poll, ReadInterest};
pub struct ReadSample;
pub struct ReadState {
    request: String,
    partial: Vec<u8>,
    deadline: u64,
    polls: usize,
    alive: Rc<Cell<bool>>,
    trace: Rc<RefCell<Vec<String>>>,
}
impl Drop for ReadState {
    fn drop(&mut self) {
        assert!(self.alive.get(), "owned state outlived its resource");
        self.trace
            .borrow_mut()
            .push(format!("state drop {}", self.request));
    }
}
impl Adapter for ReadSample {
    type Resource = Probe;
    type State = ReadState;
    type Outcome = rust_api::sample_outcome;
    fn prepare(resource: &Probe, request: String, deadline: u64) -> ReadState {
        resource
            .trace
            .borrow_mut()
            .push(format!("state start {request}"));
        ReadState {
            request,
            partial: vec![],
            deadline,
            polls: 0,
            alive: resource.alive.clone(),
            trace: resource.trace.clone(),
        }
    }
    fn poll(state: &mut ReadState, resource: &Probe, bytes: &[u8]) -> Poll<Self::Outcome> {
        assert!(resource.alive.get());
        assert!(Rc::ptr_eq(&state.alive, &resource.alive));
        state.partial.extend_from_slice(bytes);
        state.polls += 1;
        resource.trace.borrow_mut().push(format!(
            "state poll {} {} @{}",
            state.request, state.polls, state.deadline
        ));
        resource.trace.borrow_mut().push(format!(
            "poll {} {} @{}",
            state.request,
            String::from_utf8_lossy(&state.partial),
            state.deadline
        ));
        if state.polls < 3 {
            return Poll::Wait(ReadInterest {
                deadline: state.deadline,
            });
        }
        assert_eq!(state.partial, b"AB");
        resource
            .trace
            .borrow_mut()
            .push(format!("sample {}", state.request));
        Poll::Complete(rust_api::sample_outcome::ready(format!(
            "rust:{}:{}",
            resource.id,
            String::from_utf8(std::mem::take(&mut state.partial)).unwrap()
        )))
    }
}
