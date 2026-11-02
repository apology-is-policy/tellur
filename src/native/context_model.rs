// Explicit independent-root test driver for shared native contexts.
/// Explicit harness schedule, not a Tellur task scheduler. Each job is an
/// independent root with separately supplied arguments, never a cloned owner.
pub fn drive_contexts(
    host: &mut Host,
    jobs: Vec<(&'static Control, Vec<V>)>,
    quotas: &[usize],
    reverse: bool,
) -> R<Completion> {
    if !quotas.iter().any(|n| *n > 0) {
        return Err("E_NATIVE: context harness cannot make progress".into());
    }
    // A nonempty host diagnostic/contingency baseline must survive every root.
    host.enter("context harness")?;
    host.contingencies.push("host contingency marker".into());
    let mut contexts = Contexts::new(host);
    let baseline = (
        contexts.host.cursor,
        contexts.host.fuel,
        contexts.host.events.len(),
        contexts.host.resources.scopes.len(),
    );
    contexts.next = u64::MAX;
    assert!(
        contexts
            .start(jobs[0].0, vec![])
            .unwrap_err()
            .contains("context identity exhausted")
    );
    contexts.next = 0;
    assert_eq!(
        baseline,
        (
            contexts.host.cursor,
            contexts.host.fuel,
            contexts.host.events.len(),
            contexts.host.resources.scopes.len()
        )
    );
    let keys = jobs
        .into_iter()
        .map(|(control, args)| contexts.start(control, args))
        .collect::<R<Vec<_>>>()?;
    let mut order: Vec<_> = (0..keys.len()).collect();
    if reverse {
        order.reverse();
    }
    let foreign = ContextKey {
        session: Arc::new(()),
        id: keys[0].id,
    };
    assert!(contexts.advance(&foreign, 1).is_err());
    assert_eq!(contexts.host.cursor, baseline.0);
    assert_eq!(contexts.host.fuel, baseline.1);
    assert_eq!(contexts.host.events.len(), baseline.2);
    if keys.len() > 1 {
        let other = contexts.parked[&keys[1].id].machine.current.clone();
        let original = std::mem::replace(
            &mut contexts
                .parked
                .get_mut(&keys[0].id)
                .unwrap()
                .machine
                .current,
            other,
        );
        assert!(contexts.advance(&keys[0], 0).is_err());
        contexts
            .parked
            .get_mut(&keys[0].id)
            .unwrap()
            .machine
            .current = original;
        let other = contexts.parked[&keys[1].id].host.scope.clone();
        let original = std::mem::replace(
            &mut contexts.parked.get_mut(&keys[0].id).unwrap().host.scope,
            other,
        );
        assert!(contexts.advance(&keys[0], 1).is_err());
        contexts.parked.get_mut(&keys[0].id).unwrap().host.scope = original;
        let original = contexts.parked[&keys[0].id].root.scope.clone();
        contexts.parked.get_mut(&keys[0].id).unwrap().root.scope.id = u64::MAX;
        assert!(contexts.advance(&keys[0], 1).is_err());
        contexts.parked.get_mut(&keys[0].id).unwrap().root.scope = original;
        assert_eq!(contexts.host.cursor, baseline.0);
        assert_eq!(contexts.host.fuel, baseline.1);
        assert_eq!(contexts.host.events.len(), baseline.2);
        assert_eq!(contexts.parked.len(), keys.len());
    }
    let foreign_domain = Domains::default().prepare(None)?;
    let original = std::mem::replace(
        &mut contexts
            .parked
            .get_mut(&keys[0].id)
            .unwrap()
            .machine
            .current,
        foreign_domain.key,
    );
    assert!(contexts.advance(&keys[0], 0).is_err());
    contexts
        .parked
        .get_mut(&keys[0].id)
        .unwrap()
        .machine
        .current = original;
    let mut done = vec![false; keys.len()];
    let mut chains: Vec<_> = keys.iter().map(|key| vec![key.clone()]).collect();
    let mut turns = 0;
    while done.iter().any(|v| !v) {
        for &index in &order {
            if done[index] {
                continue;
            }
            let quota = quotas[turns % quotas.len()];
            let snapshot = (
                contexts.host.cursor,
                contexts.host.fuel,
                contexts.host.events.len(),
                contexts.host.resources.values.len(),
                contexts.host.resources.scopes.len(),
                contexts.domains.len(),
            );
            let selected = chains[index].last().unwrap().clone();
            let result = contexts.advance_retained(&selected, quota)?;
            if quota == 0 {
                assert_eq!(
                    snapshot,
                    (
                        contexts.host.cursor,
                        contexts.host.fuel,
                        contexts.host.events.len(),
                        contexts.host.resources.values.len(),
                        contexts.host.resources.scopes.len(),
                        contexts.domains.len()
                    )
                );
            }
            let result = match result {
                ContextPoll::Await => {
                    match contexts.admit_await(&selected) {
                        Ok(child) => chains[index].push(child),
                        Err(error) => contexts.reject_await(&selected, error)?,
                    }
                    continue;
                }
                ContextPoll::Ready if selected.id != keys[index].id => {
                    chains[index].pop();
                    contexts.return_await(chains[index].last().unwrap(), &selected)?;
                    continue;
                }
                ContextPoll::Ready => ContextPoll::Complete(contexts.join(None, &selected)?),
                ContextPoll::Resumed(parent) => {
                    chains[index].pop();
                    assert_eq!(chains[index].last().unwrap().id, parent.id);
                    continue;
                }
                ContextPoll::Failed(_) if selected.id != keys[index].id => {
                    while chains[index].last().is_some_and(|key| {
                        key.id != keys[index].id && !contexts.parked.contains_key(&key.id)
                    }) {
                        chains[index].pop();
                    }
                    continue;
                }
                other => other,
            };
            let event = match result {
                ContextPoll::Paused => {
                    // An escaping child can leave its root's terminal report
                    // queued. Zero quota defers that report without live frames.
                    if contexts.terminal.contains_key(&selected.id) {
                        assert_eq!(quota, 0);
                        continue;
                    }
                    // A live ancestor is not the selected nested boundary. Reject
                    // such a parked-state mismatch even when no work is requested.
                    let parked = contexts.parked.get_mut(&selected.id).unwrap();
                    let original = std::mem::replace(
                        &mut parked.machine.current,
                        parked.machine.root.key.clone(),
                    );
                    if contexts
                        .domains
                        .require_selected(&parked.machine.root, &original)
                        .is_err()
                    {
                        assert!(contexts.advance(&selected, 0).is_err());
                    }
                    contexts
                        .parked
                        .get_mut(&selected.id)
                        .unwrap()
                        .machine
                        .current = original;
                    contexts.require(&selected)?;
                    continue;
                }
                ContextPoll::Complete(Completion::Outcome((outcome, fields))) => {
                    serde_json::json!({"kind":"context_complete","context":index,"outcome":outcome,"fields":fields})
                }
                ContextPoll::Complete(Completion::Process(status)) => {
                    serde_json::json!({"kind":"context_process","context":index,"status":status})
                }
                ContextPoll::Failed(report) => {
                    serde_json::json!({"kind":"context_failed","context":index,"report":report})
                }
                ContextPoll::Complete(_)
                | ContextPoll::Ready
                | ContextPoll::Await
                | ContextPoll::Resumed(_) => {
                    return Err("E_NATIVE: invalid independent root completion".into());
                }
            };
            done[index] = true;
            contexts.host.events.push(event);
            assert!(contexts.advance(&keys[index], 0).is_err());
        }
        turns += 1;
    }
    assert!(contexts.values.is_empty());
    assert!(contexts.terminal.is_empty());
    assert_eq!(contexts.domains.len(), 0);
    assert_eq!(contexts.domains.member_count(), 0);
    assert!(contexts.host.tasks.is_empty());
    assert_eq!(contexts.host.resources.scopes.len(), baseline.3);
    assert_eq!(contexts.host.contingencies, ["host contingency marker"]);
    let baseline_report = contexts.host.activations.failure(
        "context harness",
        fault::Phase::Execution,
        "host observation",
    );
    assert_eq!(baseline_report.activations.len(), 1);
    assert_eq!(baseline_report.activations[0].control, "context harness");
    assert!(baseline_report.cause.is_none());
    assert!(!contexts.host.source_faulted());
    contexts.host.contingencies.pop();
    contexts.host.leave();
    contexts.host.events.push(serde_json::json!({
        "kind":"context_summary", "cursor":contexts.host.cursor,
        "script_calls":contexts.host.script.as_ref().map(Vec::len),
        "fuel":contexts.host.fuel, "resources":contexts.host.resources.values.len()
    }));
    // At a fuel cutoff some independent roots can fail before their scripted
    // input. Preserve the cursor above; this harness reports partial execution
    // instead of letting launch's single-root unused-script check hide it.
    contexts.host.script = None;
    Ok(Completion::Outcome(("done".into(), vec![])))
}
