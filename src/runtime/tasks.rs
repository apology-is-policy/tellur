//! Reference-value retirement around the backend-shared task protocol.
use super::*;
#[path = "../task_support.rs"]
mod protocol;
pub(super) use protocol::{OwnerId, Readiness, Registry, TaskId, WaitId};

pub(super) fn retire_value(registry: &mut Registry, value: Value, abrupt: bool) -> Result<()> {
    match value {
        Value::Task { id, arguments, .. } => {
            if !abrupt && registry.contains(TaskId(id)) {
                return Err(error("pending task cannot be discarded normally"));
            }
            registry.abandon(TaskId(id));
            for value in arguments {
                retire_value(registry, value, abrupt)?;
            }
        }
        Value::Construct { fields, .. } => {
            for value in fields {
                retire_value(registry, value, abrupt)?;
            }
        }
        _ => {}
    }
    Ok(())
}
