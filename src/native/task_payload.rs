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

/// Captures queued by an await or explicit host start. Dropping this does not
/// settle the task: the owner's obligation survives until delivery or unwind.
#[derive(Debug)]
pub struct QueuedTask {
    binding: TaskBinding,
    wait: Option<WaitId>,
    started_handle: bool,
    dispatched: bool,
    arguments: Option<Vec<V>>,
}
impl QueuedTask {
    pub(super) fn source_handle(&self, ty: &str) -> (TaskId, V) {
        (
            self.binding.task,
            V::Task {
                binding: Some(self.binding.clone()),
                ty: ty.into(),
                operation: self.binding.producer.name.into(),
                arguments: vec![],
            },
        )
    }
    pub(super) fn dependencies(&self, values: &Frames) -> Dependencies {
        match &self.arguments {
            Some(arguments) => Dependencies::describe(values, arguments),
            None => Dependencies::Unknown("queued captures have already transferred".into()),
        }
    }
    pub(super) fn retire(mut self, host: &mut Host) -> R<()> {
        self.abandon(host);
        if let Some(arguments) = self.arguments.take() {
            retire_tasks(host, &arguments, true)?;
            host.release_frame(&arguments.into_iter().map(Some).collect::<Vec<_>>());
        }
        Ok(())
    }
    pub(super) fn prepare_context<'a>(
        &'a self,
        host: &Host,
        parent: &Activation,
        retained: bool,
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
        if retained {
            if !self.started_handle || self.wait.is_some() {
                return Err("E_NATIVE: retained start requires unclaimed work".into());
            }
            host.tasks
                .require_queued(self.binding.task, self.binding.owner)?;
        } else {
            if self.started_handle || self.wait.is_none() {
                return Err("E_NATIVE: await admission requires its original wait".into());
            }
            host.tasks.require_dispatch(self.binding.task)?;
        }
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
        host.tasks.require_delivery(
            self.binding.task,
            self.binding.owner,
            self.wait
                .ok_or("E_NATIVE: started completion has not been claimed")?,
        )
    }
    pub(super) fn deliver(&self, host: &mut Host) {
        host.tasks
            .deliver(self.binding.task, self.binding.owner, self.wait.unwrap())
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
        self.select(host)?;
        Ok((self.binding.producer, self.take_arguments()))
    }
}
impl QueuedTask {
    pub(super) fn take_arguments(&mut self) -> Vec<V> {
        self.arguments.take().expect("prevalidated captures")
    }
    pub(super) fn dispatch_retained(&mut self, host: &mut Host) -> R<()> {
        if !self.started_handle || self.dispatched {
            return Err("E_NATIVE: task is not an unselected retained start".into());
        }
        self.select(host)
    }
    pub(super) fn select(&mut self, host: &mut Host) -> R<()> {
        if !self.dispatched {
            host.tasks.dispatch(self.binding.task)?;
            self.dispatched = true;
        }
        Ok(())
    }
    pub(super) fn prepare_join(&self, host: &Host, producer: &'static Control) -> R<WaitId> {
        if !self.started_handle
            || self.wait.is_some()
            || !std::ptr::eq(self.binding.producer, producer)
            || !Arc::ptr_eq(&self.binding.registry, &host.resources.identity)
        {
            return Err("E_NATIVE: started task producer or join authority differs".into());
        }
        host.tasks
            .prepare_join(self.binding.task, self.binding.owner, producer.name)
    }
    pub(super) fn claim(&mut self, host: &mut Host, producer: &'static Control) -> R<WaitId> {
        self.prepare_join(host, producer)?;
        let wait = host
            .tasks
            .join(self.binding.task, self.binding.owner, producer.name)?;
        self.wait = Some(wait);
        Ok(wait)
    }
    pub(super) fn require_join(&self, wait: Option<WaitId>) -> R<()> {
        if self.started_handle != wait.is_some() || (self.started_handle && self.wait != wait) {
            return Err("E_NATIVE: completion requires its consuming started-task join".into());
        }
        Ok(())
    }
}
impl Activation {
    pub(super) fn source_task_identity(
        &self,
        host: &Host,
        value: &V,
        producer: &'static Control,
    ) -> R<TaskId> {
        self.require_task_host(host)?;
        let V::Task {
            binding: Some(binding),
            arguments,
            ..
        } = value
        else {
            return Err("E_NATIVE: source join requires a bound task".into());
        };
        if !arguments.is_empty()
            || !Arc::ptr_eq(&binding.registry, &self.registry)
            || binding.owner != self.owner
            || !std::ptr::eq(binding.producer, producer)
        {
            return Err("E_NATIVE: source join authority differs".into());
        }
        Ok(binding.task)
    }

    pub(super) fn require_task_host(&self, host: &Host) -> R<()> {
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
    /// Queue eagerly evaluated direct-await arguments without an intermediate
    /// cold handle. Failed admission leaves the caller's arguments intact.
    pub fn queue_invocation(
        &self,
        host: &mut Host,
        producer: &'static Control,
        arguments: &mut Vec<V>,
    ) -> R<QueuedTask> {
        self.require_task_host(host)?;
        if producer.constructor || arguments.len() != producer.parameters.len() {
            return Err("E_NATIVE: invalid direct await producer or arguments".into());
        }
        let (task, wait) = host.tasks.queue_await(self.owner, producer.name)?;
        Ok(QueuedTask {
            binding: TaskBinding {
                registry: self.registry.clone(),
                owner: self.owner,
                task,
                producer,
            },
            wait: Some(wait),
            started_handle: false,
            dispatched: false,
            arguments: Some(std::mem::take(arguments)),
        })
    }
    fn cold_binding(
        &self,
        host: &Host,
        producer: &'static Control,
        value: Option<&V>,
    ) -> R<TaskBinding> {
        self.require_task_host(host)?;
        let Some(V::Task {
            binding: Some(binding),
            operation,
            arguments,
            ..
        }) = value
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
        Ok(binding.clone())
    }
    pub(super) fn prepare_cold_task(
        &self,
        host: &Host,
        producer: &'static Control,
        value: Option<&V>,
    ) -> R<(TaskBinding, WaitId)> {
        let binding = self.cold_binding(host, producer, value)?;
        let wait = host
            .tasks
            .prepare_schedule(binding.task, self.owner, producer.name)?;
        Ok((binding, wait))
    }
    /// Queue a cold task without allocating a completion wait or running code.
    /// The executor must retain these captures before selecting any context.
    pub fn queue_started(
        &self,
        host: &mut Host,
        producer: &'static Control,
        value: &mut Option<V>,
    ) -> R<QueuedTask> {
        let binding = self.cold_binding(host, producer, value.as_ref())?;
        host.tasks
            .enqueue(binding.task, self.owner, producer.name)?;
        let Some(V::Task { arguments, .. }) = value.take() else {
            unreachable!()
        };
        Ok(QueuedTask {
            binding,
            wait: None,
            started_handle: true,
            dispatched: false,
            arguments: Some(arguments),
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
        let (binding, _) = self.prepare_cold_task(host, producer, value.as_ref())?;
        let wait = host
            .tasks
            .schedule(binding.task, self.owner, producer.name)?;
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
            wait: Some(wait),
            started_handle: false,
            dispatched: false,
            arguments: Some(arguments),
        })
    }
}
#[cfg(test)]
#[path = "task_payload_tests.rs"]
mod tests;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PreparedAwait {
    descriptor: ColdAwait,
    storage: super::super::frames::Storage,
    binding: TaskBinding,
    wait: WaitId,
}
impl Machine {
    pub(super) fn prepare_cold_await(
        &self,
        host: &Host,
        values: &Frames,
    ) -> R<Option<PreparedAwait>> {
        let Some(descriptor) = self.next_access().checked_cold_await()? else {
            return Ok(None);
        };
        let a = self
            .stack
            .last()
            .ok_or("E_NATIVE: cold await has no invocation")?;
        if a.pending.is_some() {
            return Err("E_NATIVE: cold await is already pending".into());
        }
        let storage = values.local_storage(&a.frame, descriptor.slot)?;
        let index = values.require_storage(&storage)?;
        let (binding, wait) =
            a.prepare_cold_task(host, descriptor.producer, values[index].as_ref())?;
        Ok(Some(PreparedAwait {
            descriptor,
            storage,
            binding,
            wait,
        }))
    }
}
