//! Native error adapter for the shared invocation/task/queue/join protocol.
type Result<T> = super::R<T>;
fn error(message: impl Into<String>) -> String {
    format!("E_NATIVE: {}", message.into())
}
#[cfg(test)]
#[path = "../task_support.rs"]
mod protocol;
pub(super) use protocol::{OwnerId, Registry, TaskId, WaitId};
