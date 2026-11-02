//! Bound cold captures and queue admission; context dispatch/delivery is separate.
use super::super::tasks::{OwnerId, TaskId, WaitId};
use super::*;
use std::sync::Arc;

/// Opaque authority stays attached when the checked value moves locally.
/// The host and exact compiled producer prevent numeric-ID and name collisions.
#[derive(Clone)]
pub struct TaskBinding {
    registry: Arc<()>,
    owner: OwnerId,
    task: TaskId,
    producer: &'static Control,
}
impl std::fmt::Debug for TaskBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskBinding")
            .field("owner", &self.owner)
            .field("task", &self.task)
            .field("producer", &self.producer.name)
            .finish_non_exhaustive()
    }
}
impl PartialEq for TaskBinding {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.registry, &other.registry)
            && self.owner == other.owner
            && self.task == other.task
            && std::ptr::eq(self.producer, other.producer)
    }
}
impl Eq for TaskBinding {}

/// Captures queued by a consuming await. Dropping this does not settle the task:
/// the owner's outstanding obligation remains until completion or emergency unwind.
pub struct QueuedTask {
    binding: TaskBinding,
    wait: WaitId,
    arguments: Option<Vec<V>>,
}
impl QueuedTask {
    /// Select only the head of the shared ready queue; no producer body is run.
    /// A failed/duplicate selection leaves both captures and protocol unchanged.
    pub fn dispatch(&mut self, host: &mut Host) -> R<(&'static Control, Vec<V>)> {
        if !Arc::ptr_eq(&self.binding.registry, &host.resources.identity)
            || self.arguments.is_none()
        {
            return Err("E_NATIVE: queued task is foreign or already dispatched".into());
        }
        host.tasks.dispatch(self.binding.task)?;
        Ok((self.binding.producer, self.arguments.take().unwrap()))
    }
}
impl Activation {
    fn require_task_host(&self, host: &Host) -> R<()> {
        if !Arc::ptr_eq(&self.registry, &host.resources.identity) {
            return Err("E_NATIVE: invocation belongs to another host".into());
        }
        host.tasks.require_retire_owner(self.owner, true)
    }
    /// Register a cold obligation before moving any captures. Source checking
    /// supplies types/permissions; this layer checks live owner and producer arity.
    pub fn defer_task(
        &self,
        host: &mut Host,
        producer: &'static Control,
        ty: &str,
        arguments: &mut Vec<V>,
    ) -> R<V> {
        self.require_task_host(host)?;
        if producer.constructor || arguments.len() != producer.parameters.len() {
            return Err("E_NATIVE: invalid cold task producer or arguments".into());
        }
        let task = host.tasks.defer(self.owner, producer.name)?;
        Ok(V::Task {
            binding: Some(TaskBinding {
                registry: self.registry.clone(),
                owner: self.owner,
                task,
                producer,
            }),
            ty: ty.into(),
            operation: producer.name.into(),
            arguments: std::mem::take(arguments),
        })
    }
    /// Validate original owner/producer and queue+wait capacity before consuming
    /// the handle. No lexical-boundary identity is captured by a cold value.
    pub fn queue_task(
        &self,
        host: &mut Host,
        producer: &'static Control,
        value: &mut Option<V>,
    ) -> R<QueuedTask> {
        self.require_task_host(host)?;
        let Some(V::Task {
            binding: Some(binding),
            operation,
            arguments,
            ..
        }) = value.as_ref()
        else {
            return Err("E_NATIVE: await requires a bound pending task".into());
        };
        if !Arc::ptr_eq(&binding.registry, &self.registry)
            || binding.owner != self.owner
            || !std::ptr::eq(binding.producer, producer)
            || operation != producer.name
            || producer.constructor
            || arguments.len() != producer.parameters.len()
        {
            return Err("E_NATIVE: pending task owner or producer differs".into());
        }
        let wait = host.tasks.schedule(binding.task, self.owner, operation)?;
        let Some(V::Task {
            binding: Some(binding),
            arguments,
            ..
        }) = value.take()
        else {
            unreachable!()
        };
        Ok(QueuedTask {
            binding,
            wait,
            arguments: Some(arguments),
        })
    }
}
#[cfg(test)]
#[path = "task_payload_tests.rs"]
mod tests;
