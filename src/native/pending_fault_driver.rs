// Included only in the generated fault fixture's private operation module.
fn marker(c: &mut Contexts<'_>, chain: &mut Vec<ContextKey>, text: &str) -> R<()> {
    for _ in 0..500 {
        let key = chain.last().unwrap().clone();
        let begin = c.host.events.len();
        match c.advance_retained(&key, 1)? {
            ContextPoll::Paused => {}
            ContextPoll::Await => chain.push(c.admit_await(&key)?),
            _ => panic!("fixture did not reach marker"),
        }
        if c.host.events[begin..]
            .iter()
            .any(|e| e["kind"] == "emit" && e["message"] == text)
        {
            return Ok(());
        }
    }
    panic!("fixture marker limit")
}
fn prepare_fault_operation(c: &mut Contexts<'_>, start: ContextKey) -> R<Ticket> {
    let mut chain = vec![start];
    for _ in 0..500 {
        let key = chain.last().unwrap().clone();
        if matches!(
            c.parked[&key.id].machine.stack.last().unwrap().pending,
            Some(Pending::Adapter(_))
        ) {
            return Ok(ticket(c, &key));
        }
        match c.advance_retained(&key, 1)? {
            ContextPoll::Paused => {}
            ContextPoll::Await => chain.push(c.admit_await(&key)?),
            _ => panic!("fixture did not reach operation"),
        }
    }
    panic!("fixture operation limit")
}
pub fn drive_pending_fault(host: &mut Host, controls: [&'static Control; 4]) -> R<Completion> {
    let [owner, lexical, named, independent] = controls;
    let fuel = std::env::var("TELLUR_FAULT_FUEL")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let mode = std::env::var("TELLUR_FAULT_MODE")
        .unwrap()
        .parse::<u8>()
        .unwrap();
    let keep_other = std::env::var("TELLUR_FAULT_INDEPENDENT").unwrap() == "true";
    let quotas: Vec<usize> = std::env::var("TELLUR_NATIVE_MODEL_QUOTAS")
        .unwrap()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let mut turns = quotas.into_iter().cycle();
    let mut c = Contexts::new(host);
    let root = c.start_entry(owner, vec![])?;
    marker(&mut c, &mut vec![root.clone()], "owner ready")?;
    let other = if keep_other {
        let key = c.start(independent, vec![])?;
        let mut chain = vec![key];
        marker(&mut c, &mut chain, "independent ready")?;
        let wake = c.park_for_readiness(chain.last().unwrap())?;
        Some((chain, wake))
    } else {
        None
    };
    let mut tickets = vec![];
    let mut wakes = vec![];
    for (control, request) in [(lexical, "left"), (named, "right")] {
        let child = c.start_child(&root, control, vec![V::String(request.into())])?;
        let t = prepare_fault_operation(&mut c, child)?;
        operation(&mut c, &t)?.poll("partial")?;
        if mode == 2 {
            operation(&mut c, &t)?
                .complete("ready", vec![V::String("discarded candidate".into())])?;
        }
        wakes.push(c.park_for_readiness(&t.context)?);
        tickets.push(t);
    }
    if mode > 0 {
        for wake in &wakes {
            c.signal_readiness(wake)?;
        }
    }
    c.host.fuel = Some(fuel);
    // The ordinary host gate remains closed. This fixture explicitly admits only
    // the owner's checked literal fault, with no borrowed operand evaluation.
    assert!(c.advance_retained(&root, 1).is_err());
    let access = c.parked[&root.id].machine.next_access();
    assert_eq!(access.transition, Transition::Fault);
    assert!(access.accesses.is_empty());
    c.source_checked = true;
    assert!(matches!(c.advance_retained(&root, 0)?, ContextPoll::Paused));
    let first = c.advance_retained(&root, 1)?;
    c.source_checked = false;
    let mut selected = Some(first);
    let result = loop {
        let progress = if let Some(first) = selected.take() {
            first
        } else {
            c.advance_retained(&root, turns.next().unwrap())?
        };
        match progress {
            ContextPoll::Paused => {}
            ContextPoll::Ready => break Ok(c.join(None, &root)?),
            ContextPoll::Complete(result) => break Ok(result),
            ContextPoll::Failed(_) => {
                break Err(c.entry_error.take().expect("original owner failure"));
            }
            _ => panic!("invalid owner settlement progress"),
        }
    };
    for wake in &wakes {
        assert!(c.signal_readiness(wake).is_err());
    }
    for t in &tickets {
        assert!(operation(&mut c, t).is_err());
    }
    assert!(!c.host.events.iter().any(|e| {
        e["kind"] == "emit"
            && e["message"]
                .as_str()
                .is_some_and(|s| s.starts_with("forbidden"))
    }));
    if let Some((mut chain, wake)) = other {
        assert!(result.is_ok());
        assert_eq!(c.waits.len(), 1);
        c.signal_readiness(&wake)?;
        for _ in 0..500 {
            let key = chain.last().unwrap().clone();
            match c.advance_retained(&key, turns.next().unwrap())? {
                ContextPoll::Paused => {}
                ContextPoll::Ready if chain.len() == 1 => {
                    c.join(None, &key)?;
                    chain.clear();
                    break;
                }
                ContextPoll::Ready => {
                    chain.pop();
                    c.return_await(chain.last().unwrap(), &key)?;
                }
                _ => panic!("independent context did not survive"),
            }
        }
        assert!(chain.is_empty());
        assert!(c.signal_readiness(&wake).is_err());
    }
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.values.is_empty());
    assert!(c.host.tasks.is_empty() && c.terminal.is_empty() && c.waits.is_empty());
    assert!(c.host.resources.values.is_empty());
    assert_eq!(c.domains.len(), 0);
    assert_eq!(c.domains.member_count(), 0);
    std::fs::write(
        std::env::var("TELLUR_FAULT_EVENTS").unwrap(),
        serde_json::to_vec(&c.host.events).unwrap(),
    )
    .unwrap();
    result
}
