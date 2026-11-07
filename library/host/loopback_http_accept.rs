// Polled connection acceptance for the bounded loopback HTTP profile. The
// awaiting Tellur task parks until the nonblocking listener is readable or the
// idle timeout elapses; other tasks keep running. Outcomes and limits match the
// blocking baseline in loopback_http.rs.
use super::http::{Connection, Listener};
use crate::rust_api::{self, http_accept_outcome::*};

pub struct Accepting {
    until: Option<std::time::Instant>,
    immediate: Option<rust_api::http_accept_outcome>,
}
pub fn http_accept(_: &Listener, idle_ms: u32) -> Box<dyn rust_api::http_accept_operation> {
    if idle_ms == 0 || idle_ms > 60000 {
        return Box::new(Accepting {
            until: None,
            immediate: Some(failed("idle timeout outside 1..60000 ms".into())),
        });
    }
    Box::new(Accepting {
        until: Some(std::time::Instant::now() + std::time::Duration::from_millis(idle_ms.into())),
        immediate: None,
    })
}
impl rust_api::http_accept_operation for Accepting {
    fn poll(&mut self, listener: &Listener) -> rust_api::Poll<rust_api::http_accept_outcome> {
        if let Some(outcome) = self.immediate.take() {
            return rust_api::Poll::Ready(outcome);
        }
        let until = self.until.unwrap();
        loop {
            // As in the blocking profile, the deadline is checked before every accept.
            let now = std::time::Instant::now();
            if now >= until {
                return rust_api::Poll::Ready(idle);
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    // Responses and close use blocking writes with their own timeouts.
                    if let Err(e) = stream.set_nonblocking(false) {
                        return rust_api::Poll::Ready(failed(e.to_string()));
                    }
                    return rust_api::Poll::Ready(accepted(Connection::new(stream)));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return rust_api::Poll::Pending(
                        rust_api::Wait::readable(listener).or_after(until - now),
                    );
                }
                Err(e) => return rust_api::Poll::Ready(failed(e.to_string())),
            }
        }
    }
}
