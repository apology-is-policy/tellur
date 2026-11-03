// Replaces sample polling in the private SourcePump; the unchanged owned parser
// runs only under positive host quota and returns to the source queue on Wait.
pub fn poll_ready(&mut self, c: &mut Contexts<'_>, key: &ContextKey) -> R<bool> {
    if !self.eligible.remove(&key.id) {
        return Ok(false);
    }
    let t = self.tickets[&key.id].clone();
    if operation(c, &t).is_err() {
        return Ok(false);
    }
    let before = (c.host.fuel, c.host.events.clone(), c.host.cursor);
    if let Some(until) = self.deadlines.remove(&key.id) {
        let io_before = observed(c, &t)?;
        assert!(matches!(c.advance_retained(key, 1)?, ContextPoll::Paused));
        assert_eq!(io_before, observed(c, &t)?);
        let resource = http_resource(c, &t)?;
        c.host
            .resources
            .get::<Probe>(&resource, "http_connection")?
            .wake(until);
    }
    for _ in 0..10000 {
        let progress = poll_http(c, &t)?;
        assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
        match progress {
            crate::rust_host::operation::Progress::Again => {}
            crate::rust_host::operation::Progress::Wait(until) => {
                assert!(self.deadlines.insert(key.id, until).is_none());
                self.park(c, key)?;
                return Ok(true);
            }
            crate::rust_host::operation::Progress::Complete(value) => {
                let result = crate::encode_http_result(value)?;
                let args = vec![http_resource(c, &t)?];
                let (label, fields) = c.host.finish_http_model("http_receive", args, result)?;
                operation(c, &t)?.complete(&label, fields)?;
                assert!(operation(c, &t)?.complete("failed", vec![]).is_err());
                assert!(poll_http(c, &t).is_err());
                self.retired_tickets
                    .push(self.tickets.remove(&key.id).unwrap());
                return Ok(false);
            }
        }
    }
    panic!("bounded HTTP poll did not reach Wait or Complete")
}
