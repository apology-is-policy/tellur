// Test-only retained HTTP candidates under an explicitly selected owning fault.
impl Drop for Operation {
    fn drop(&mut self) {
        if let Some(Completion::Outcome((label, fields))) = self.result.take() {
            assert!(self.lease.alive.get());
            match (label.as_str(), fields.as_slice()) {
                (
                    "request",
                    [
                        V::String(method),
                        V::String(target),
                        V::Construct {
                            name,
                            fields: headers,
                            ..
                        },
                        V::String(body),
                    ],
                ) => {
                    assert_eq!(method, "HEAD");
                    assert_eq!(target, &format!("/{}", self.lease.request));
                    assert_eq!(body, &format!("{}:🙂", self.lease.request));
                    assert_eq!(name, "map");
                    assert_eq!(
                        headers,
                        &vec![
                            V::String("content-length".into()),
                            V::String(body.len().to_string()),
                            V::String("host".into()),
                            V::String("local".into()),
                            V::String("x-name".into()),
                            V::String(self.lease.request.clone())
                        ]
                    );
                }
                ("malformed", [V::String(message)]) => assert_eq!(message, "HTTP/1.1 required"),
                _ => panic!("unexpected encoded HTTP candidate"),
            }
            self.lease
                .trace
                .borrow_mut()
                .push(format!("encoded candidate drop {}", self.lease.request));
        }
    }
}
fn prepare_http_fault(c: &mut Contexts<'_>, t: &Ticket, mode: u8) -> R<Readiness> {
    let before = (c.host.fuel, c.host.events.clone(), c.host.cursor);
    let mut old = vec![];
    let mut partial = false;
    for _ in 0..1000 {
        let progress = poll_http(c, t)?;
        assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
        match progress {
            crate::rust_host::operation::Progress::Again => {}
            crate::rust_host::operation::Progress::Wait(_until) => {
                let wake = c.park_for_readiness(&t.context)?;
                let notice = arm_live_http(c, t, wake.generation)?;
                for previous in &old {
                    assert!(c.signal_readiness(previous).is_err());
                }
                if partial && mode < 2 {
                    if mode == 1 {
                        let reads = crate::rust_host::read_count();
                        operation(c, t)?.owned.hint(&notice)?;
                        assert_eq!(reads, crate::rust_host::read_count());
                    }
                    return Ok(wake);
                }
                operation(c, t)?.owned.hint(&notice)?;
                c.signal_readiness(&wake)?;
                assert!(c.signal_readiness(&wake).is_err());
                let observed_before = observed(c, t)?;
                assert!(matches!(
                    c.advance_retained(&t.context, 0)?,
                    ContextPoll::Paused
                ));
                assert_eq!(observed_before, observed(c, t)?);
                assert!(matches!(
                    c.advance_retained(&t.context, 1)?,
                    ContextPoll::Paused
                ));
                assert_eq!(observed_before, observed(c, t)?);
                old.push(wake);
                partial = true;
            }
            crate::rust_host::operation::Progress::Complete(value) => {
                assert!(mode >= 2);
                assert!(poll_http(c, t).is_err());
                let resource = http_resource(c, t)?;
                let candidate = crate::rust_host::HttpCandidate::new(
                    value,
                    c.host
                        .resources
                        .get::<Probe>(&resource, "http_connection")?,
                );
                operation(c, t)?.candidate = Some(candidate);
                if mode == 3 {
                    let op = operation(c, t)?;
                    let candidate = op.candidate.as_mut().unwrap();
                    let value = candidate.encode()?;
                    assert!(candidate.encode().is_err());
                    let encoded = crate::encode_http_result(value)?;
                    let (label, fields) =
                        c.host
                            .finish_http_model("http_receive", vec![resource], encoded)?;
                    operation(c, t)?.complete(&label, fields)?;
                    assert!(operation(c, t)?.complete("failed", vec![]).is_err());
                }
                // Administrative retention for the controlled fault schedule. No
                // physical subscription is re-created after parser completion.
                let wake = c.park_for_readiness(&t.context)?;
                for previous in &old {
                    assert!(c.signal_readiness(previous).is_err());
                }
                return Ok(wake);
            }
        }
    }
    panic!("HTTP candidate preparation bound")
}
