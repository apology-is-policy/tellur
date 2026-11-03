// Explicit test driver over production callbacks; no source task switching on quota.
pub fn drive_direct_probe(
    host: &mut Host,
    args: Vec<V>,
    control: &'static Control,
    mut next_quota: impl FnMut() -> usize,
) -> R<Completion> {
    let mut direct = 0;
    let mut cold = 0;
    let mut contexts = Contexts::new(host);
    let root = contexts.start_entry(control, args)?;
    let mut chain = vec![root.clone()];
    let result = loop {
        let quota = next_quota();
        let selected = chain.last().expect("live sequential entry").clone();
        match contexts.advance_retained(&selected, quota)? {
            ContextPoll::Paused => {}
            ContextPoll::Await => {
                let pending = contexts.await_continuation(&selected)?;
                assert!(
                    pending.child.is_none() && pending.result.is_none() && pending.queued.is_some()
                );
                match pending.access.transition {
                    Transition::Await => {
                        direct += 1;
                        assert!(pending.access.opaque);
                    }
                    Transition::AwaitTask => cold += 1,
                    _ => panic!("source await must retain its exact operation"),
                }
                let events = contexts.host.events.clone();
                let fuel = contexts.host.fuel;
                assert!(matches!(
                    contexts.advance_retained(&selected, 99)?,
                    ContextPoll::Await
                ));
                assert_eq!(contexts.host.events, events);
                assert_eq!(contexts.host.fuel, fuel);
                let child = contexts.admit_await(&selected)?;
                assert!(contexts.admit_await(&selected).is_err());
                chain.push(child);
            }
            ContextPoll::Ready if selected.id == root.id => {
                break Ok(contexts.join(None, &root)?);
            }
            ContextPoll::Ready => {
                chain.pop();
                contexts.return_await(chain.last().unwrap(), &selected)?;
            }
            ContextPoll::Resumed(parent) => {
                chain.pop();
                assert_eq!(chain.last().unwrap().id, parent.id);
            }
            ContextPoll::Complete(result) => break Ok(result),
            ContextPoll::Failed(_) if selected.id == root.id => {
                break Err(contexts.entry_error.take().expect("original entry error"));
            }
            ContextPoll::Failed(_) => {
                // Owning settlement can retire several descendants and recover
                // an ancestor. Never resume a stale child or replay its await.
                while chain
                    .last()
                    .is_some_and(|key| key.id != root.id && !contexts.parked.contains_key(&key.id))
                {
                    chain.pop();
                }
            }
        }
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
        return Err("E_NATIVE: executable entry did not settle".into());
    }
    assert_eq!((direct, cold), (3, 2));
    result
}
