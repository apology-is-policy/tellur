//! Checked source driver. Direct child captures retain loans until consuming join;
//! selected direct result loans retain their origins. Inline helpers retain stopped-owner
//! obligations. Inline boundaries remain gated. Host APIs keep stricter gates.
use super::super::super::tasks::TaskId;
use super::*;
use std::collections::VecDeque;

enum Runnable {
    Run(ContextKey),
    Await(ContextKey),
}
fn live(contexts: &Contexts<'_>, key: &ContextKey) -> bool {
    contexts.parked.contains_key(&key.id) || contexts.terminal.contains_key(&key.id)
}
fn push(queue: &mut VecDeque<Runnable>, key: ContextKey) {
    if !queue.iter().any(|r| match r {
        Runnable::Run(k) | Runnable::Await(k) => k.id == key.id,
    }) {
        queue.push_back(Runnable::Run(key));
    }
}
/// This entry is emitted only from checked programs. It is not host permission to
/// resume an arbitrary callback beside borrowed children.
pub fn run_source_entry(host: &mut Host, args: Vec<V>, control: &'static Control) -> R<Completion> {
    drive_source(host, args, control, || usize::MAX)
}
fn drive_source(
    host: &mut Host,
    args: Vec<V>,
    control: &'static Control,
    mut quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut contexts = Contexts::new(host);
    contexts.source_checked = true;
    let root = contexts.start_entry(control, args)?;
    let mut selected = Some(root.clone());
    let mut runnable = VecDeque::new();
    let mut handles: BTreeMap<TaskId, StartedTask> = BTreeMap::new();
    let mut joins: BTreeMap<u64, (ContextKey, TaskJoin, Deliver)> = BTreeMap::new();
    let mut awaits: BTreeMap<u64, ContextKey> = BTreeMap::new();
    let result = loop {
        let key = if let Some(key) = selected.take() {
            key
        } else {
            loop {
                match runnable.pop_front() {
                    Some(Runnable::Run(key)) if live(&contexts, &key) => break key,
                    Some(Runnable::Await(parent)) if live(&contexts, &parent) => {
                        // A sibling can settle this owner before its queued
                        // await is dispatched. Select its restored handler or
                        // terminal failure; never re-admit the abandoned call.
                        let queued = contexts.parked.get(&parent.id).is_some_and(|p| {
                            matches!(p.machine.stack.last().and_then(|a| a.pending.as_ref()),
                                Some(Pending::Await(p)) if p.queued.is_some()
                                    && p.child.is_none() && p.result.is_none())
                        });
                        if !queued {
                            break parent;
                        }
                        match contexts.admit_await(&parent) {
                            Ok(child) => {
                                awaits.insert(child.id, parent);
                                break child;
                            }
                            Err(error) => {
                                contexts.reject_await(&parent, error)?;
                                break parent;
                            }
                        }
                    }
                    Some(_) => {}
                    None => {
                        return Err("E_NATIVE: source task wait has no runnable producer".into());
                    }
                }
            }
        };
        match contexts.advance_retained(&key, quota())? {
            ContextPoll::Paused => selected = Some(key),
            ContextPoll::Await => {
                let handoff = contexts
                    .parked
                    .get_mut(&key.id)
                    .unwrap()
                    .machine
                    .source_handoff
                    .take();
                match handoff {
                    Some(SourceHandoff::Spawn { task, slot, ty }) => {
                        let (id, value) = task.source_handle(ty);
                        let child = contexts.retain_task(&key, &mut Some(task))?;
                        push(&mut runnable, child.context().clone());
                        assert!(handles.insert(id, child).is_none());
                        let parent = &contexts.parked[&key.id];
                        let frame = parent.machine.stack.last().unwrap();
                        contexts.values[frame.base() + slot] = Some(value);
                        selected = Some(key); // spawn queues; it is not a source handoff.
                    }
                    Some(SourceHandoff::Yield) => push(&mut runnable, key),
                    Some(SourceHandoff::Join {
                        value,
                        producer,
                        deliver,
                    }) => {
                        let frame = contexts.parked[&key.id].machine.stack.last().unwrap();
                        let id = frame.source_task_identity(contexts.host, &value, producer)?;
                        let mut task = handles.remove(&id);
                        let join = contexts.claim_task(&key, producer, &mut task)?;
                        let child = join.context().id;
                        assert!(joins.insert(child, (key, join, deliver)).is_none());
                    }
                    None => runnable.push_back(Runnable::Await(key)),
                }
            }
            ContextPoll::Ready if key.id == root.id => break Ok(contexts.join(None, &root)?),
            ContextPoll::Ready => {
                if let Some(parent) = awaits.remove(&key.id) {
                    contexts.return_await(&parent, &key)?;
                    selected = Some(parent);
                } // Independent completion remains retained until its consuming join.
            }
            ContextPoll::Resumed(parent) => {
                awaits.remove(&key.id);
                selected = Some(parent);
            }
            ContextPoll::Complete(result) => break Ok(result),
            ContextPoll::Failed(_) if key.id == root.id => {
                break Err(contexts
                    .entry_error
                    .take()
                    .ok_or("E_NATIVE: missing entry failure")?);
            }
            ContextPoll::Failed(_) => {}
        }
        // A completion only wakes its consuming caller; it does not replace the
        // selected source chain or force an implicit switch at a host quota.
        let completed: Vec<_> = joins
            .keys()
            .filter(|id| contexts.completed.contains_key(id))
            .copied()
            .collect();
        for id in completed {
            let (parent, join, deliver) = joins.remove(&id).unwrap();
            let result = contexts.join_task(&parent, &mut Some(join))?;
            let activation = contexts
                .parked
                .get_mut(&parent.id)
                .unwrap()
                .machine
                .stack
                .last_mut()
                .unwrap();
            deliver(activation, &mut contexts.values, result)?;
            push(&mut runnable, parent);
        }
        // Owning settlement can retire many descendants and recover an ancestor.
        // Retired joins/awaits do not resume normal code or deliver phantom results.
        let stopped: Vec<_> = joins
            .keys()
            .filter(|id| !contexts.parked.contains_key(id) && !contexts.completed.contains_key(id))
            .copied()
            .collect();
        for id in stopped {
            let (parent, _, _) = joins.remove(&id).unwrap();
            if live(&contexts, &parent) {
                push(&mut runnable, parent);
            }
        }
        let stopped: Vec<_> = awaits
            .keys()
            .filter(|id| !contexts.parked.contains_key(id) && !contexts.completed.contains_key(id))
            .copied()
            .collect();
        for id in stopped {
            let parent = awaits.remove(&id).unwrap();
            if live(&contexts, &parent) {
                push(&mut runnable, parent);
            }
        }
        handles.retain(|id, _| contexts.host.tasks.contains(*id));
    };
    if !contexts.parked.is_empty()
        || !contexts.completed.is_empty()
        || !contexts.values.is_empty()
        || !contexts.host.tasks.is_empty()
        || !contexts.terminal.is_empty()
        || !contexts.waits.is_empty()
        || contexts.domains.len() != 0
        || contexts.domains.member_count() != 0
    {
        return Err("E_NATIVE: source entry did not settle".into());
    }
    result
}
