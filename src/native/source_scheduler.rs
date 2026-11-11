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
/// A caller suspended by `await next`; members stay in set order.
struct SetWait {
    parent: ContextKey,
    set: V,
    members: Vec<TaskId>,
    producer: &'static Control,
    deliver: Deliver,
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
    quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut contexts = Contexts::new(host);
    contexts.source_checked = true;
    let root = contexts.start_entry(control, args)?;
    drive_contexts(contexts, root, quota)
}
#[cfg(any(test, feature = "runtime-abi"))]
pub unsafe fn run_abi_entry<T>(
    host: &mut Host,
    args: Vec<V>,
    layout: &'static super::super::super::abi::InvocationLayout,
    program: &mut T,
) -> R<Completion> {
    unsafe { drive_abi(host, args, layout, program, || usize::MAX) }
}
#[cfg(any(test, feature = "runtime-abi"))]
pub(super) unsafe fn drive_abi<T>(
    host: &mut Host,
    args: Vec<V>,
    layout: &'static super::super::super::abi::InvocationLayout,
    program: &mut T,
    quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut contexts = Contexts::new(host);
    contexts.source_checked = true;
    let root = unsafe { contexts.start_abi_entry(layout, args, program)? };
    drive_contexts(contexts, root, quota)
}
fn drive_contexts(
    mut contexts: Contexts<'_>,
    root: ContextKey,
    mut quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut selected = Some(root.clone());
    let mut runnable = VecDeque::new();
    let mut handles: BTreeMap<TaskId, StartedTask> = BTreeMap::new();
    let mut joins: BTreeMap<u64, (ContextKey, TaskJoin, Deliver)> = BTreeMap::new();
    let mut awaits: BTreeMap<u64, ContextKey> = BTreeMap::new();
    let mut set_waits: Vec<SetWait> = Vec::new();
    // Source completion order of started children, keyed by context identity.
    let mut completions: BTreeMap<u64, u64> = BTreeMap::new();
    let mut clock = 0u64;
    let result = loop {
        let key = if let Some(key) = selected.take() {
            key
        } else {
            loop {
                // Idle point: no live source work remains, so one pending host
                // operation becomes ready in FIFO order.
                if !runnable.iter().any(|r| match r {
                    Runnable::Run(k) | Runnable::Await(k) => live(&contexts, k),
                }) {
                    // A launch signal is delivered before readiness.
                    if contexts.host.deliver_script_signals()?
                        || contexts.host.deliver_live_signal()
                    {
                        contexts.host.take_signal_wake();
                        for woken in contexts.wake_cancelled()? {
                            push(&mut runnable, woken);
                        }
                        continue;
                    }
                    if let Some(key) = contexts.signal_host_operation()? {
                        runnable.push_back(Runnable::Run(key));
                    } else if contexts.host.live_signal_pending() {
                        continue;
                    }
                }
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
        let poll = contexts.advance_retained(&key, quota())?;
        // A signal delivered during this turn wakes observing operations first.
        contexts.host.deliver_live_signal();
        if contexts.host.take_signal_wake() {
            for woken in contexts.wake_cancelled()? {
                push(&mut runnable, woken);
            }
        }
        match poll {
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
                    Some(SourceHandoff::SpawnInto {
                        task,
                        set,
                        slot,
                        ty,
                    }) => {
                        let V::Construct {
                            ty: set_ty,
                            name,
                            mut fields,
                        } = set
                        else {
                            return Err("E_NATIVE: task set spawn requires an owned set".into());
                        };
                        let (id, value) = task.source_handle(ty);
                        let child = contexts.retain_task(&key, &mut Some(task))?;
                        push(&mut runnable, child.context().clone());
                        assert!(handles.insert(id, child).is_none());
                        fields.push(value);
                        let parent = &contexts.parked[&key.id];
                        let frame = parent.machine.stack.last().unwrap();
                        contexts.values[frame.base() + slot] = Some(V::Construct {
                            ty: set_ty,
                            name,
                            fields,
                        });
                        selected = Some(key); // spawn queues; it is not a source handoff.
                    }
                    Some(SourceHandoff::Next {
                        set,
                        producer,
                        deliver,
                    }) => {
                        let V::Construct { fields, .. } = &set else {
                            return Err("E_NATIVE: await next requires an owned set".into());
                        };
                        let frame = contexts.parked[&key.id].machine.stack.last().unwrap();
                        let members = fields
                            .iter()
                            .map(|member| {
                                frame.source_task_identity(contexts.host, member, producer)
                            })
                            .collect::<R<Vec<_>>>()?;
                        set_waits.push(SetWait {
                            parent: key,
                            set,
                            members,
                            producer,
                            deliver,
                        });
                    }
                    Some(SourceHandoff::Yield) => push(&mut runnable, key),
                    Some(SourceHandoff::Cancel) => {
                        // Not a handoff: pending operations that observe the
                        // request rejoin the FIFO in their wait order.
                        for woken in contexts.wake_cancelled()? {
                            push(&mut runnable, woken);
                        }
                        selected = Some(key);
                    }
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
                clock += 1;
                completions.insert(key.id, clock);
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
        // A pending host operation parks its task; quota pauses never do.
        if let Some(current) = selected.clone()
            && contexts.host_operation_waiting(&current).is_some()
        {
            contexts.park_host_operation(&current)?;
            selected = None;
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
        // A set waiter joins its earliest completed member, then rejoins the FIFO.
        let mut index = 0;
        while index < set_waits.len() {
            let earliest = set_waits[index]
                .members
                .iter()
                .enumerate()
                .filter_map(|(position, id)| {
                    let context = handles.get(id)?.context().id;
                    contexts.completed.contains_key(&context).then(|| {
                        (
                            completions.get(&context).copied().unwrap_or(u64::MAX),
                            position,
                        )
                    })
                })
                .min();
            let Some((_, position)) = earliest else {
                index += 1;
                continue;
            };
            let mut wait = set_waits.remove(index);
            let id = wait.members.remove(position);
            let mut task = handles.remove(&id);
            let join = contexts.claim_task(&wait.parent, wait.producer, &mut task)?;
            completions.remove(&join.context().id);
            let (outcome, mut payload) = contexts
                .join_task(&wait.parent, &mut Some(join))?
                .outcome()?;
            let V::Construct { fields, .. } = &mut wait.set else {
                unreachable!()
            };
            fields.remove(position);
            payload.push(wait.set);
            let activation = contexts
                .parked
                .get_mut(&wait.parent.id)
                .unwrap()
                .machine
                .stack
                .last_mut()
                .unwrap();
            (wait.deliver)(
                activation,
                &mut contexts.values,
                Completion::Outcome((outcome, payload)),
            )?;
            push(&mut runnable, wait.parent);
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
        // A root chain parked by run of a pending host operation is in no join,
        // await or set wait. When settlement finishes it with a terminal failure,
        // its readiness registration is gone: select it to report that failure.
        let terminal: Vec<_> = contexts.terminal.keys().copied().collect();
        for id in terminal {
            let queued = runnable.iter().any(|r| match r {
                Runnable::Run(k) | Runnable::Await(k) => k.id == id,
            });
            if !queued && selected.as_ref().is_none_or(|s| s.id != id) {
                push(
                    &mut runnable,
                    ContextKey {
                        session: contexts.identity.clone(),
                        id,
                    },
                );
            }
        }
        // A restored owner may already have an old runnable/await queue entry.
        // It is now the active chain, so retain no duplicate position that could
        // turn later local recovery or a host pause into an implicit handoff.
        if let Some(selected) = &selected {
            runnable.retain(|r| match r {
                Runnable::Run(key) | Runnable::Await(key) => key.id != selected.id,
            });
        }
        // Settlement that abandons a member or its waiting owner retires the wait.
        let mut index = 0;
        while index < set_waits.len() {
            let wait = &set_waits[index];
            if live(&contexts, &wait.parent)
                && wait
                    .members
                    .iter()
                    .all(|id| contexts.host.tasks.contains(*id))
            {
                index += 1;
                continue;
            }
            let wait = set_waits.remove(index);
            if live(&contexts, &wait.parent)
                && selected.as_ref().is_none_or(|s| s.id != wait.parent.id)
            {
                push(&mut runnable, wait.parent);
            }
        }
        handles.retain(|id, _| contexts.host.tasks.contains(*id));
        completions.retain(|id, _| contexts.completed.contains_key(id));
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
