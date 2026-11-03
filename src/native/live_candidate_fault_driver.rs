// Included beside the original checked typed-fault poll helper.
fn live_poll(
    backend: &backend_model::Backend,
    c: &mut Contexts<'_>,
    t: &Ticket,
    bytes: &[u8],
) -> R<bool> {
    let before = (c.host.fuel, c.host.events.clone(), c.host.cursor);
    let token = register_backend(backend, c, &t.context, backend_model::Failure::None)?;
    let event = backend.signal(&token, true)?;
    c.signal_readiness(&event)?;
    assert!(c.signal_readiness(&event).is_err());
    assert!(matches!(
        c.advance_retained(&t.context, 1)?,
        ContextPoll::Paused
    ));
    drop(operation(c, t)?.registration.take().expect("poll wait"));
    let complete = poll_pending_owned(c, t, bytes)?;
    assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
    Ok(complete)
}
fn final_live_wait(
    backend: &backend_model::Backend,
    c: &mut Contexts<'_>,
    t: &Ticket,
) -> R<Readiness> {
    let active = backend.active();
    let before = (
        c.host.fuel,
        c.host.events.clone(),
        c.host.cursor,
        c.waits.len(),
        crate::rust_host::live_read_count(),
    );
    for failure in [
        backend_model::Failure::BeforeInsert,
        backend_model::Failure::AfterInsert,
    ] {
        assert!(register_backend(backend, c, &t.context, failure).is_err());
        assert_eq!(backend.active(), active);
    }
    let generation = c.next_readiness;
    c.next_readiness = u64::MAX;
    assert!(register_backend(backend, c, &t.context, backend_model::Failure::None).is_err());
    c.next_readiness = generation;
    assert_eq!(backend.active(), active);
    assert_eq!(
        before,
        (
            c.host.fuel,
            c.host.events.clone(),
            c.host.cursor,
            c.waits.len(),
            crate::rust_host::live_read_count()
        )
    );
    register_backend(backend, c, &t.context, backend_model::Failure::None)
}
