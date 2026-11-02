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

/// Retire obligations carried by discarded owned values, without following loans
/// or applying resource dispositions. Check the whole graph before any mutation.
pub(super) fn retire_tasks<'a>(
    host: &mut Host,
    values: impl IntoIterator<Item = &'a V>,
    abrupt: bool,
) -> R<()> {
    fn visit(
        host: &Host,
        value: &V,
        abrupt: bool,
        tasks: &mut std::collections::BTreeSet<TaskId>,
    ) -> R<()> {
        match value {
            V::Task {
                binding, arguments, ..
            } => {
                if let Some(binding) = binding {
                    if !Arc::ptr_eq(&binding.registry, &host.resources.identity) {
                        return Err("E_NATIVE: discarded task belongs to another host".into());
                    }
                    if !abrupt && host.tasks.contains(binding.task) {
                        return Err("E_NATIVE: pending task cannot be discarded normally".into());
                    }
                    tasks.insert(binding.task);
                }
                for value in arguments {
                    visit(host, value, abrupt, tasks)?;
                }
            }
            V::Construct { fields, .. } => {
                for value in fields {
                    visit(host, value, abrupt, tasks)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut tasks = std::collections::BTreeSet::new();
    for value in values {
        visit(host, value, abrupt, &mut tasks)?;
    }
    for task in tasks {
        host.tasks.abandon(task);
    }
    Ok(())
}

/// Captures queued by a consuming await. Dropping this does not settle the task:
/// the owner's outstanding obligation remains until completion or emergency unwind.
#[derive(Debug)]
pub struct QueuedTask {
    binding: TaskBinding,
    wait: WaitId,
    arguments: Option<Vec<V>>,
}
impl QueuedTask {
    pub(super) fn prepare_context<'a>(
        &'a self,
        host: &Host,
        parent: &Activation,
    ) -> R<(&'static Control, &'a [V])> {
        parent.require_task_host(host)?;
        if !Arc::ptr_eq(&self.binding.registry, &parent.registry)
            || self.binding.owner != parent.owner
        {
            return Err("E_NATIVE: queued task belongs to another invocation".into());
        }
        let args = self
            .arguments
            .as_deref()
            .ok_or("E_NATIVE: queued captures already dispatched")?;
        host.tasks.require_dispatch(self.binding.task)?;
        Ok((self.binding.producer, args))
    }
    pub(super) fn complete(&self, host: &mut Host) -> R<()> {
        host.tasks.complete(self.binding.task, self.binding.owner)
    }
    pub(super) fn require_delivery(&self, host: &Host, parent: &Activation) -> R<()> {
        parent.require_task_host(host)?;
        if self.binding.owner != parent.owner
            || !Arc::ptr_eq(&self.binding.registry, &parent.registry)
        {
            return Err("E_NATIVE: task completion belongs to another invocation".into());
        }
        host.tasks
            .require_delivery(self.binding.task, self.binding.owner, self.wait)
    }
    pub(super) fn deliver(&self, host: &mut Host) {
        host.tasks
            .deliver(self.binding.task, self.binding.owner, self.wait)
            .expect("prevalidated completion wait");
        host.tasks
            .consume(self.binding.task, self.binding.owner)
            .expect("delivered task consumption");
    }
    pub(super) fn abandon(&self, host: &mut Host) {
        host.tasks.abandon(self.binding.task);
    }
    pub(super) fn prepare_readiness(&self, host: &Host) -> R<super::super::tasks::Readiness> {
        host.tasks
            .prepare_readiness(self.binding.task, self.binding.owner)
    }
    pub(super) fn park(
        &self,
        host: &mut Host,
        readiness: &super::super::tasks::Readiness,
    ) -> R<()> {
        host.tasks.park(self.binding.owner, readiness)
    }
    pub(super) fn resume(&self, host: &mut Host) -> R<()> {
        host.tasks.dispatch(self.binding.task)
    }
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
