// Harness-only scheduling at an existing checked cold await. The generated child
// and delivery callbacks are unchanged; this does not implement source spawn.
pub fn drive_started_probe(
    host: &mut Host,
    args: Vec<V>,
    control: &'static Control,
    phase: u8,
    mut quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut contexts = Contexts::new(host);
    let root = contexts.start_entry(control, args)?;
    let mut handoffs = 0;
    loop {
        match contexts.advance_retained(&root, quota())? {
            ContextPoll::Paused => (),
            ContextPoll::Ready => {
                let result = contexts.join(None, &root)?;
                assert_eq!(handoffs, 1);
                assert!(contexts.host.tasks.is_empty() && contexts.values.is_empty());
                assert!(contexts.parked.is_empty() && contexts.completed.is_empty());
                assert_eq!(
                    (contexts.domains.len(), contexts.domains.member_count()),
                    (0, 0)
                );
                return Ok(result);
            }
            ContextPoll::Await => {
                handoffs += 1;
                let mut queued = contexts.await_continuation(&root)?.queued.take();
                let activation = contexts.parked[&root.id].machine.stack.last().unwrap();
                let producer = queued
                    .as_ref()
                    .unwrap()
                    .prepare_context(contexts.host, activation, true)?
                    .0;
                let events = contexts.host.events.clone();
                let fuel = contexts.host.fuel;
                let mut handle = Some(contexts.retain_task(&root, &mut queued)?);
                let child = handle.as_ref().unwrap().context().clone();
                let mut join = None;
                assert!(queued.is_none());
                assert!(matches!(
                    contexts.advance_retained(&child, 0)?,
                    ContextPoll::Paused
                ));
                assert_eq!(contexts.host.events, events);
                assert_eq!(contexts.host.fuel, fuel);
                if phase > 0 {
                    contexts.dispatch_task(&child)?;
                }
                let wake = if phase == 2 {
                    assert!(matches!(
                        contexts.advance_retained(&child, 1)?,
                        ContextPoll::Paused
                    ));
                    Some(contexts.park_for_readiness(&child)?)
                } else {
                    None
                };
                if phase < 3 {
                    join = Some(contexts.claim_task(&root, producer, &mut handle)?);
                    assert!(contexts.join_task(&root, &mut join).is_err());
                }
                if let Some(wake) = wake {
                    contexts.signal_readiness(&wake)?;
                }
                loop {
                    match contexts.advance_retained(&child, quota())? {
                        ContextPoll::Paused => (),
                        ContextPoll::Ready => break,
                        _ => panic!("unexpected started child poll"),
                    }
                }
                if phase == 3 {
                    assert!(contexts.join(Some(&root), &child).is_err());
                    join = Some(contexts.claim_task(&root, producer, &mut handle)?);
                }
                assert!(contexts.advance_retained(&root, 99).is_err());
                let dependencies = contexts.dependencies(&child)?.clone();
                let result = contexts.join_task(&root, &mut join)?;
                assert!(contexts.join_task(&root, &mut join).is_err());
                assert!(handle.is_none());
                let pending = contexts.await_continuation(&root)?;
                pending.dependencies.extend(dependencies);
                pending.result = Some(Ok(result));
            }
            _ => panic!("unexpected started probe owner poll"),
        }
    }
}
