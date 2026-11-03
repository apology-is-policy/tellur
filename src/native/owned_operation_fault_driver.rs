// Private fault experiment: leave a completed typed candidate in its operation.
fn poll_pending_owned(c: &mut Contexts<'_>, ticket: &Ticket, bytes: &[u8]) -> R<bool> {
    let resource = operation(c, ticket)?.resource.clone();
    let probe = c.host.resources.get::<Probe>(&resource, "probe")?;
    let frame = c
        .parked
        .get_mut(&ticket.context.id)
        .unwrap()
        .machine
        .stack
        .last_mut()
        .unwrap();
    let Some(Pending::Adapter(op)) = &mut frame.pending else {
        unreachable!()
    };
    match op.owned.poll(probe, bytes)? {
        Some(interest) => {
            assert_eq!(interest.deadline, 57);
            assert!(op.owned.take().is_err());
            Ok(false)
        }
        None => {
            assert!(op.owned.poll(probe, b"replay").is_err());
            // Keep the generated typed outcome in Ready until owning settlement.
            Ok(true)
        }
    }
}
fn registration_failure(c: &mut Contexts<'_>, ticket: &Ticket) -> R<()> {
    let trace = operation(c, ticket)?.lease.trace.clone();
    let before = (
        c.host.fuel,
        c.host.events.clone(),
        c.host.cursor,
        trace.borrow().clone(),
        format!("{:?}", &*c.values),
        c.waits.len(),
    );
    let invalid = Ticket {
        context: ticket.context.clone(),
        operation: Arc::new(()),
    };
    assert!(poll_pending_owned(c, &invalid, b"foreign").is_err());
    let generation = c.next_readiness;
    c.next_readiness = u64::MAX;
    assert!(c.park_for_readiness(&ticket.context).is_err());
    assert_eq!(c.next_readiness, u64::MAX);
    c.next_readiness = generation;
    assert_eq!(
        before,
        (
            c.host.fuel,
            c.host.events.clone(),
            c.host.cursor,
            trace.borrow().clone(),
            format!("{:?}", &*c.values),
            c.waits.len()
        )
    );
    Ok(())
}
