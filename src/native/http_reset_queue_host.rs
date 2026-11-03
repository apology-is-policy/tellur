// Only the controlled peer changes; readiness still never reads the receiver.
impl Socket {
    fn reset_peer(&self, token: usize, until: Instant) -> io::Result<()> {
        let mut wire = self.wire.borrow_mut();
        abort_peer(wire.client.take().expect("peer closes once"));
        self.trace
            .borrow_mut()
            .push(format!("abort peer {}", self.name));
        let mut events = mio::Events::with_capacity(8);
        while Instant::now() < until {
            match wire.poll.poll(
                &mut events,
                Some(until.saturating_duration_since(Instant::now())),
            ) {
                Ok(()) => {
                    if events.iter().any(|e| e.token().0 == token) {
                        self.trace
                            .borrow_mut()
                            .push(format!("reset notice {} {token}", self.name));
                        return Ok(());
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "reset readiness barrier",
        ))
    }
}
