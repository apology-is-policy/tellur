// Included only by the compiled pending-adapter fixture overlay, inside its model.
pub fn drive_pending(host: &mut Host, args: Vec<V>, control: &'static Control) -> R<Completion> {
    let quotas: Vec<usize> = std::env::var("TELLUR_NATIVE_MODEL_QUOTAS")
        .unwrap_or("1".into())
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert!(quotas.iter().any(|n| *n > 0));
    let mut turns = quotas.into_iter().cycle();
    let mut c = Contexts::new(host);
    let root = c.start_entry(control, args)?;
    let mut chain = vec![root.clone()];
    let mut retired_wakes = vec![];
    let mut completed_tickets = vec![];
    let result = loop {
        let selected = chain.last().unwrap().clone();
        let pending = c
            .parked
            .get(&selected.id)
            .and_then(|p| p.machine.stack.last())
            .is_some_and(|a| matches!(&a.pending, Some(Pending::Adapter(op)) if !op.ready()));
        if pending {
            let t = ticket(&c, &selected);
            for bytes in ["A", "", "B"] {
                let before = (c.host.fuel, c.host.events.len(), c.host.cursor);
                operation(&mut c, &t)?.poll(bytes)?;
                let wake = c.park_for_readiness(&selected)?;
                assert!(c.advance_retained(&selected, 1).is_err());
                c.signal_readiness(&wake)?;
                assert!(c.signal_readiness(&wake).is_err());
                assert!(matches!(
                    c.advance_retained(&selected, 0)?,
                    ContextPoll::Paused
                ));
                assert!(matches!(
                    c.advance_retained(&selected, 1)?,
                    ContextPoll::Paused
                ));
                assert!(c.signal_readiness(&wake).is_err());
                assert_eq!(before, (c.host.fuel, c.host.events.len(), c.host.cursor));
                retired_wakes.push(wake);
            }
            let op = operation(&mut c, &t)?;
            assert_eq!(op.partial, "AB");
            assert_eq!(op.deadline, 57);
            let arguments = vec![op.resource.clone(), V::String(op.lease.request.clone())];
            // Run the real typed Rust adapter once, with the originally evaluated
            // operands. The checked arm is delivered only by the next machine step.
            let (label, fields) = c.host.primitive("sample", arguments)?;
            operation(&mut c, &t)?.complete(&label, fields)?;
            assert!(
                operation(&mut c, &t)?
                    .complete("ready", vec![V::String("twice".into())])
                    .is_err()
            );
            completed_tickets.push(t);
        }
        match c.advance_retained(&selected, turns.next().unwrap())? {
            ContextPoll::Paused => {}
            ContextPoll::Await => match c.admit_await(&selected) {
                Ok(child) => chain.push(child),
                Err(error) => c.reject_await(&selected, error)?,
            },
            ContextPoll::Ready if selected.id == root.id => break Ok(c.join(None, &root)?),
            ContextPoll::Ready => {
                chain.pop();
                c.return_await(chain.last().unwrap(), &selected)?;
            }
            ContextPoll::Resumed(parent) => {
                chain.pop();
                assert_eq!(chain.last().unwrap().id, parent.id);
            }
            ContextPoll::Complete(result) => break Ok(result),
            ContextPoll::Failed(_) if selected.id == root.id => {
                break Err(c.entry_error.take().expect("original entry error"));
            }
            ContextPoll::Failed(_) => {
                while chain
                    .last()
                    .is_some_and(|k| k.id != root.id && !c.parked.contains_key(&k.id))
                {
                    chain.pop();
                }
            }
        }
    };
    for wake in retired_wakes {
        assert!(c.signal_readiness(&wake).is_err());
    }
    for t in completed_tickets {
        assert!(operation(&mut c, &t).is_err());
    }
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.values.is_empty());
    assert!(c.host.tasks.is_empty() && c.terminal.is_empty() && c.waits.is_empty());
    assert_eq!(c.domains.len(), 0);
    assert_eq!(c.domains.member_count(), 0);
    result
}
