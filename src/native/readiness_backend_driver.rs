// A tentative backend insertion is owned locally until runtime wait publication.
fn register_backend(
    backend: &backend_model::Backend,
    c: &mut Contexts<'_>,
    key: &ContextKey,
    failure: backend_model::Failure,
) -> R<Readiness> {
    let t = ticket(c, key);
    let op = operation(c, &t)?;
    assert!(op.registration.is_none());
    let reservation = backend.prepare(op, failure)?;
    // On any runtime-preflight rejection the reservation is dropped here. Its
    // backend interest is removed before returning to the still-live operation.
    let token = c.park_for_readiness(key)?;
    let subscription = reservation.commit(token.clone());
    // park_for_readiness changes only task/wait metadata, not the validated frame.
    operation(c, &t)
        .expect("wait publication changed its operation")
        .registration = Some(subscription);
    Ok(token)
}
