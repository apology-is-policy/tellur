// Private source-adapter experiment. Not emitted by the production backend.
// Runtime metadata, resource identity and readiness generations stay outside this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadInterest {
    pub deadline: u64,
}

pub enum Poll<T> {
    Wait(ReadInterest),
    Complete(T),
}

// Associated types cannot depend on any individual preparation/poll borrow.
// A generated binding chooses the concrete owned input and outcome types. 'static
// alone does not prove plain data, resource dispositions, effects or bounded Drop.
pub trait Adapter {
    type Resource: 'static;
    type State: 'static;
    type Outcome: 'static;

    fn prepare(resource: &Self::Resource, request: String, deadline: u64) -> Self::State;
    fn poll(
        state: &mut Self::State,
        resource: &Self::Resource,
        bytes: &[u8],
    ) -> Poll<Self::Outcome>;
}

enum Phase<A: Adapter> {
    Active(A::State),
    Ready(A::Outcome),
    Retired,
}

pub struct Operation<A: Adapter> {
    phase: Phase<A>,
}

impl<A: Adapter> Operation<A> {
    pub fn start(resource: &A::Resource, request: String, deadline: u64) -> Self {
        Self {
            phase: Phase::Active(A::prepare(resource, request, deadline)),
        }
    }

    /// None means a retained typed completion; Some requests another registration.
    pub fn poll(
        &mut self,
        resource: &A::Resource,
        bytes: &[u8],
    ) -> Result<Option<ReadInterest>, &'static str> {
        let Phase::Active(state) = &mut self.phase else {
            return Err("E_MODEL: operation is completed or retired");
        };
        match A::poll(state, resource, bytes) {
            Poll::Wait(interest) => Ok(Some(interest)),
            Poll::Complete(outcome) => {
                // Destroy state before source result adoption or backing-resource release.
                self.phase = Phase::Ready(outcome);
                Ok(None)
            }
        }
    }

    pub fn take(&mut self) -> Result<A::Outcome, &'static str> {
        if !matches!(self.phase, Phase::Ready(_)) {
            return Err("E_MODEL: operation has no pending completion");
        }
        let Phase::Ready(outcome) = std::mem::replace(&mut self.phase, Phase::Retired) else {
            unreachable!()
        };
        Ok(outcome)
    }
    // Dropping Active destroys only adapter state. Dropping Ready discards its
    // owned result. The owning runtime must do either before releasing suppliers.
}
