// Explicit test admission, followed by one child's cold-await chain. Quota
// partitions cannot change this schedule or enable source spawn/yield.
pub fn drive_mixed_await(
    host: &mut Host,
    root_control: &'static Control,
    child_control: &'static Control,
    args: Vec<V>,
    quotas: &[usize],
) -> R<Completion> {
    let mut c = Contexts::new(host);
    let root = c.start(root_control, vec![V::Bool(false)])?;
    let child = c.start_child(&root, child_control, args)?;
    let mut chain = vec![root.clone(), child];
    let mut quotas = quotas.iter().cycle();
    loop {
        let key = chain.last().unwrap().clone();
        match c.advance_retained(&key, *quotas.next().unwrap())? {
            ContextPoll::Paused => {}
            ContextPoll::Await => chain.push(c.admit_await(&key)?),
            ContextPoll::Ready => {
                chain.pop();
                if chain.len() > 1 {
                    c.return_await(chain.last().unwrap(), &key)?;
                } else {
                    let completion = c.join(chain.last(), &key)?;
                    let Completion::Outcome((name, fields)) = completion else {
                        panic!("mixed await requires an ordinary outcome")
                    };
                    assert_eq!(name, "done");
                    assert!(fields.is_empty());
                    if chain.is_empty() {
                        break;
                    }
                    c.host
                        .events
                        .push(serde_json::json!({"kind":"child_joined"}));
                }
            }
            ContextPoll::Failed(report) => {
                if key.id == root.id {
                    c.host
                        .events
                        .push(serde_json::json!({"kind":"mixed_failed","report":report}));
                    break;
                }
                // Local recovery retains the worker; escaping faults leave only
                // the root's terminal report. Never select a retired descendant.
                while chain
                    .last()
                    .is_some_and(|k| k.id != root.id && !c.parked.contains_key(&k.id))
                {
                    chain.pop();
                }
            }
            _ => panic!("unexpected mixed await completion"),
        }
    }
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.terminal.is_empty());
    assert!(c.values.is_empty() && c.waits.is_empty() && c.host.tasks.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    c.host
        .events
        .push(serde_json::json!({"kind":"mixed_settled","fuel":c.host.fuel}));
    Ok(Completion::Outcome(("done".into(), vec![])))
}
