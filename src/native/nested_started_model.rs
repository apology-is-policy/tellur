// Explicit test selection composes an awaited owner, an independent child and
// that child's own sequential await. Production source remains sequential.
pub fn drive_nested_started(
    host: &mut Host,
    args: Vec<V>,
    control: &'static Control,
    phase: u8,
    mut quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut c = Contexts::new(host);
    let root = c.start_entry(control, args)?;
    let mut chain = vec![root.clone()];
    let mut started: Option<(
        ContextKey,
        &'static Control,
        Option<StartedTask>,
        Option<TaskJoin>,
    )> = None;
    let mut observed = false;
    loop {
        let selected = chain.last().unwrap().clone();
        match c.advance_retained(&selected, quota())? {
            ContextPoll::Paused => (),
            ContextPoll::Await => {
                if c.await_continuation(&selected)?.access.transition == Transition::AwaitTask {
                    assert!(c.parked[&selected.id].sequential && chain.len() >= 2);
                    assert!(!observed);
                    observed = true;
                    let mut queued = c.await_continuation(&selected)?.queued.take();
                    let activation = c.parked[&selected.id].machine.stack.last().unwrap();
                    let producer = queued
                        .as_ref()
                        .unwrap()
                        .prepare_context(c.host, activation, true)?
                        .0;
                    let before = (c.host.fuel, c.host.events.clone());
                    let mut handle = Some(c.retain_task(&selected, &mut queued)?);
                    let key = handle.as_ref().unwrap().context().clone();
                    assert_eq!((c.host.fuel, c.host.events.clone()), before);
                    assert!(matches!(c.advance_retained(&key, 0)?, ContextPoll::Paused));
                    assert!(c.advance_retained(&selected, 99).is_err());
                    assert!(c.advance_retained(&root, 99).is_err());
                    if phase > 0 {
                        c.dispatch_task(&key)?;
                    }
                    let wake = if phase == 2 {
                        assert!(matches!(c.advance_retained(&key, 1)?, ContextPoll::Paused));
                        Some(c.park_for_readiness(&key)?)
                    } else {
                        None
                    };
                    let join = if phase < 3 {
                        Some(c.claim_task(&selected, producer, &mut handle)?)
                    } else {
                        None
                    };
                    if let Some(wake) = wake {
                        c.signal_readiness(&wake)?;
                    }
                    started = Some((key.clone(), producer, handle, join));
                    chain.push(key);
                } else {
                    chain.push(c.admit_await(&selected)?);
                }
            }
            ContextPoll::Ready if selected.id == root.id => {
                assert!(observed && started.is_none());
                let result = c.join(None, &root)?;
                assert!(c.values.is_empty() && c.host.tasks.is_empty());
                assert!(c.parked.is_empty() && c.completed.is_empty() && c.waits.is_empty());
                assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
                return Ok(result);
            }
            ContextPoll::Ready => {
                chain.pop();
                let parent = chain.last().unwrap();
                if started
                    .as_ref()
                    .is_some_and(|(key, ..)| key.id == selected.id)
                {
                    let (_, producer, mut handle, mut join) = started.take().unwrap();
                    if phase == 3 {
                        join = Some(c.claim_task(parent, producer, &mut handle)?);
                    }
                    let dependencies = c.dependencies(&selected)?.clone();
                    assert!(c.join(Some(parent), &selected).is_err());
                    let result = c.join_task(parent, &mut join)?;
                    assert!(c.join_task(parent, &mut join).is_err());
                    let pending = c.await_continuation(parent)?;
                    pending.dependencies.extend(dependencies);
                    pending.result = Some(Ok(result));
                } else {
                    c.return_await(parent, &selected)?;
                }
            }
            _ => panic!("unexpected normal nested ownership poll"),
        }
    }
}
