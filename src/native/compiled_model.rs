// Explicit sequential fixture driver. Quotas never select a different source task.
pub fn drive(host: &mut Host, args: Vec<V>, control: &'static Control) -> R<Completion> {
    let quotas: Vec<usize> = std::env::var("TELLUR_NATIVE_MODEL_QUOTAS")
        .unwrap_or("1".into())
        .split(',')
        .map(|s| {
            s.parse()
                .map_err(|_| "E_NATIVE: invalid model quota".to_string())
        })
        .collect::<R<_>>()?;
    if !quotas.iter().any(|q| *q > 0) {
        return Err("E_NATIVE: model quotas cannot make progress".into());
    }
    let mut contexts = Contexts::new(host);
    let root = contexts.start_entry(control, args)?;
    let mut chain = vec![root.clone()];
    let result = 'run: loop {
        for quota in &quotas {
            let selected = chain.last().expect("live sequential entry").clone();
            match contexts.advance_retained(&selected, *quota)? {
                ContextPoll::Paused => {}
                ContextPoll::Await => match contexts.admit_await(&selected) {
                    Ok(child) => chain.push(child),
                    Err(error) => contexts.reject_await(&selected, error)?,
                },
                ContextPoll::Ready if selected.id == root.id => {
                    break 'run Ok(contexts.join(None, &root)?);
                }
                ContextPoll::Ready => {
                    chain.pop();
                    contexts.return_await(chain.last().unwrap(), &selected)?;
                }
                ContextPoll::Resumed(parent) => {
                    chain.pop();
                    assert_eq!(chain.last().unwrap().id, parent.id);
                }
                ContextPoll::Complete(result) => break 'run Ok(result),
                ContextPoll::Failed(_) if selected.id == root.id => {
                    break 'run Err(contexts.entry_error.take().expect("original entry error"));
                }
                ContextPoll::Failed(_) => {
                    // Owning settlement can retire several descendants and recover
                    // an ancestor. Never resume a stale child or replay its await.
                    while chain.last().is_some_and(|key| {
                        key.id != root.id && !contexts.parked.contains_key(&key.id)
                    }) {
                        chain.pop();
                    }
                }
            }
        }
    };
    assert!(contexts.parked.is_empty() && contexts.completed.is_empty());
    assert!(contexts.values.is_empty() && contexts.host.tasks.is_empty());
    assert!(contexts.terminal.is_empty() && contexts.waits.is_empty());
    assert_eq!(
        (contexts.domains.len(), contexts.domains.member_count()),
        (0, 0)
    );
    result
}
