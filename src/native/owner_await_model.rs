// Explicit test stop points do not introduce a source scheduler.
pub fn drive_owner_await(
    host: &mut Host,
    control: &'static Control,
    failing: &'static Control,
    args: Vec<V>,
    phase: usize,
    quotas: &[usize],
) -> R<Completion> {
    let mut c = Contexts::new(host);
    let root = c.start(control, args)?;
    let failed = loop {
        if c.parked[&root.id]
            .machine
            .next_access()
            .cold_await
            .is_some()
        {
            let failed = c.start_child(&root, failing, vec![])?;
            let progress = c.advance_disjoint(&root, 99);
            assert_eq!((progress.instructions, progress.transitions), (1, 1));
            assert!(matches!(
                progress.stop,
                AdmissionStop::Poll(ContextPoll::Await)
            ));
            break failed;
        }
        assert!(matches!(c.advance_retained(&root, 1)?, ContextPoll::Paused));
    };
    if phase > 0 {
        let child = c.admit_await(&root)?;
        let mut chain = vec![root.clone(), child];
        let marker = if phase == 2 {
            "nested handler"
        } else {
            "nested work"
        };
        loop {
            let selected = chain.last().unwrap().clone();
            match c.advance_retained(&selected, 1)? {
                ContextPoll::Paused => {}
                ContextPoll::Await => chain.push(c.admit_await(&selected)?),
                ContextPoll::Ready => {
                    chain.pop();
                    if chain.len() == 1 {
                        assert!(phase >= 3);
                        if phase == 4 {
                            c.return_await(&root, &selected)?;
                        }
                        break;
                    }
                    c.return_await(chain.last().unwrap(), &selected)?;
                }
                _ => panic!("awaited work must remain available until selected stop"),
            }
            if phase < 3
                && c.host
                    .events
                    .iter()
                    .any(|e| e["kind"] == "emit" && e["message"] == marker)
            {
                break;
            }
        }
    }
    assert!(matches!(
        c.advance_retained(&failed, 1)?,
        ContextPoll::Failed(_)
    ));
    let mut quotas = quotas.iter().cycle();
    loop {
        match c.advance_retained(&root, *quotas.next().unwrap())? {
            ContextPoll::Paused => {}
            ContextPoll::Failed(report) => {
                c.host
                    .events
                    .push(serde_json::json!({"kind":"owner_failed","report":report}));
                break;
            }
            _ => panic!("owner must finish its original fault boundary"),
        }
    }
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.terminal.is_empty());
    assert!(c.values.is_empty() && c.waits.is_empty() && c.host.tasks.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    c.host
        .events
        .push(serde_json::json!({"kind":"owner_settled","fuel":c.host.fuel}));
    Ok(Completion::Outcome(("done".into(), vec![])))
}
