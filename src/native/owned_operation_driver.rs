// Included in the private native operation model after original identity checks.
fn poll_owned(
    c: &mut Contexts<'_>,
    ticket: &Ticket,
    bytes: &[u8],
) -> R<Option<super::super::super::Outcome>> {
    // Validate the original host/invocation/domain/supplier/resource before
    // splitting disjoint fields for one fresh, checked shared Rust borrow.
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
            Ok(None)
        }
        None => {
            assert!(op.owned.poll(probe, b"duplicate").is_err());
            let crate::rust_api::sample_outcome::ready(text) = op.owned.take()?;
            assert!(op.owned.take().is_err());
            assert!(op.owned.poll(probe, b"retired").is_err());
            Ok(Some(("ready".into(), vec![V::String(text)])))
        }
    }
}
