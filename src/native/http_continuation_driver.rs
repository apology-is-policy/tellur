// Included in the private model; no public asynchronous primitive admission.
fn http_resource(c: &mut Contexts<'_>, t: &Ticket) -> R<V> {
    Ok(operation(c, t)?.resource.clone())
}
fn observed(c: &mut Contexts<'_>, t: &Ticket) -> R<(usize, usize, std::time::Instant)> {
    let resource = http_resource(c, t)?;
    Ok(c.host
        .resources
        .get::<Probe>(&resource, "http_connection")?
        .observed())
}
fn poll_http(c: &mut Contexts<'_>, t: &Ticket) -> R<crate::rust_host::operation::Progress> {
    // Ticket validation covers the original invocation, domain and supplier storage.
    let resource = http_resource(c, t)?;
    let frame = c
        .parked
        .get_mut(&t.context.id)
        .unwrap()
        .machine
        .stack
        .last_mut()
        .unwrap();
    let Some(Pending::Adapter(op)) = &mut frame.pending else {
        unreachable!()
    };
    let connection = c
        .host
        .resources
        .get::<Probe>(&resource, "http_connection")?;
    op.owned.poll(connection)
}
fn receive_pending(
    c: &mut Contexts<'_>,
    selected: &ContextKey,
    retired_wakes: &mut Vec<Readiness>,
    completed_tickets: &mut Vec<Ticket>,
) -> R<()> {
    let t = ticket(c, selected);
    for _ in 0..10000 {
        let before = (c.host.fuel, c.host.events.clone(), c.host.cursor);
        let progress = poll_http(c, &t)?;
        assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
        match progress {
            crate::rust_host::operation::Progress::Again => {}
            crate::rust_host::operation::Progress::Wait(until) => {
                let wake = c.park_for_readiness(selected)?;
                let io_before = observed(c, &t)?;
                assert!(c.advance_retained(selected, 1).is_err());
                c.signal_readiness(&wake)?;
                assert!(c.signal_readiness(&wake).is_err());
                assert!(matches!(
                    c.advance_retained(selected, 0)?,
                    ContextPoll::Paused
                ));
                assert!(matches!(
                    c.advance_retained(selected, 1)?,
                    ContextPoll::Paused
                ));
                assert_eq!(io_before, observed(c, &t)?);
                assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
                assert!(c.signal_readiness(&wake).is_err());
                retired_wakes.push(wake);
                let resource = http_resource(c, &t)?;
                c.host
                    .resources
                    .get::<Probe>(&resource, "http_connection")?
                    .wake(until);
            }
            crate::rust_host::operation::Progress::Complete(value) => {
                let result = crate::encode_http_result(value)?;
                let arguments = vec![http_resource(c, &t)?];
                // Reject malformed delivery without emitting an event or losing the candidate.
                assert!(
                    c.host
                        .finish_http_model(
                            "http_receive",
                            arguments.clone(),
                            ("request".into(), vec![V::String("missing fields".into())])
                        )
                        .is_err()
                );
                assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
                let (label, fields) =
                    c.host
                        .finish_http_model("http_receive", arguments, result)?;
                operation(c, &t)?.complete(&label, fields)?;
                assert!(operation(c, &t)?.complete("failed", vec![]).is_err());
                assert!(poll_http(c, &t).is_err());
                completed_tickets.push(t);
                return Ok(());
            }
        }
    }
    panic!("bounded receive failed to complete")
}
