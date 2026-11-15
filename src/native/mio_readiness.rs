#[cfg(unix)]
struct MioReadiness {
    poll: tellur_mio::Poll,
    events: tellur_mio::Events,
}
#[cfg(unix)]
impl ReadinessBackend for MioReadiness {
    fn register(&mut self, token: u64, descriptor: Descriptor) -> R<()> {
        let interest = if descriptor.writable {
            tellur_mio::Interest::WRITABLE
        } else {
            tellur_mio::Interest::READABLE
        };
        self.poll
            .registry()
            .register(
                &mut tellur_mio::unix::SourceFd(&descriptor.fd),
                tellur_mio::Token(token as usize),
                interest,
            )
            .map_err(|e| format!("E_HOST: readiness registration failed: {e}"))
    }
    fn deregister(&mut self, fd: std::os::fd::RawFd) -> R<()> {
        self.poll
            .registry()
            .deregister(&mut tellur_mio::unix::SourceFd(&fd))
            .map_err(|e| format!("E_HOST: readiness deregistration failed: {e}"))
    }
    fn wait(&mut self, timeout: Option<std::time::Duration>) -> R<Vec<u64>> {
        loop {
            match self.poll.poll(&mut self.events, timeout) {
                Ok(()) => return Ok(self.events.iter().map(|e| e.token().0 as u64).collect()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("E_HOST: readiness wait failed: {e}")),
            }
        }
    }
}
#[cfg(unix)]
fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> {
    let poll =
        tellur_mio::Poll::new().map_err(|e| format!("E_HOST: readiness backend failed: {e}"))?;
    Ok(Some(Box::new(MioReadiness {
        poll,
        events: tellur_mio::Events::with_capacity(64),
    })))
}
