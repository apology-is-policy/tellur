// Polled response transmission and connection completion for the bounded
// loopback HTTP profile. The awaiting Tellur task parks while the socket would
// block (writable for responses, readable while draining); other tasks keep
// running. Validation, limits and deadlines match the blocking baseline.
use super::http::{Connection, response};
use crate::rust_api;
use std::io::{Read, Write};

const HEADER_LIMIT: usize = 16384;
const BODY_LIMIT: usize = 1048576;
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

/// A nonblocking duplicate of the connection's stream. Dropping it restores
/// blocking mode for later operations, including after a stop.
struct Nonblocking(std::net::TcpStream);
impl Nonblocking {
    fn new(connection: &Connection) -> std::io::Result<Self> {
        let stream = connection.stream.try_clone()?;
        stream.set_nonblocking(true)?;
        Ok(Self(stream))
    }
}
impl Drop for Nonblocking {
    fn drop(&mut self) {
        let _ = self.0.set_nonblocking(false);
    }
}

pub struct Sending {
    stream: Option<Nonblocking>,
    bytes: Vec<u8>,
    written: usize,
    until: std::time::Instant,
    immediate: Option<rust_api::http_send_outcome>,
}
pub fn http_send(
    connection: &Connection,
    status: u32,
    body: String,
    media: String,
    allow: String,
) -> Box<dyn rust_api::http_send_operation> {
    let mut state = Sending {
        stream: None,
        bytes: Vec::new(),
        written: 0,
        until: std::time::Instant::now() + IO_TIMEOUT,
        immediate: None,
    };
    match response(connection, status, &body, &media, &allow) {
        Ok(bytes) => state.bytes = bytes,
        Err(message) => {
            state.immediate = Some(rust_api::http_send_outcome::failed(message));
            return Box::new(state);
        }
    }
    match Nonblocking::new(connection) {
        Ok(stream) => state.stream = Some(stream),
        Err(e) => {
            state.immediate = Some(rust_api::http_send_outcome::failed(format!(
                "nonblocking send: {e}"
            )));
        }
    }
    Box::new(state)
}
impl rust_api::http_send_operation for Sending {
    fn poll(&mut self, _: &Connection) -> rust_api::Poll<rust_api::http_send_outcome> {
        use rust_api::http_send_outcome::*;
        if let Some(outcome) = self.immediate.take() {
            return rust_api::Poll::Ready(outcome);
        }
        let progress = loop {
            if self.written == self.bytes.len() {
                break rust_api::Poll::Ready(sent);
            }
            // As in the blocking profile, the deadline is checked before every write.
            let now = std::time::Instant::now();
            if now >= self.until {
                break rust_api::Poll::Ready(failed(
                    std::io::Error::from(std::io::ErrorKind::TimedOut).to_string(),
                ));
            }
            let mut stream = &self.stream.as_ref().unwrap().0;
            match stream.write(&self.bytes[self.written..]) {
                Ok(0) => {
                    break rust_api::Poll::Ready(failed(
                        std::io::Error::from(std::io::ErrorKind::WriteZero).to_string(),
                    ));
                }
                Ok(n) => self.written += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return rust_api::Poll::Pending(
                        rust_api::Wait::writable(stream).or_after(self.until - now),
                    );
                }
                Err(e) => break rust_api::Poll::Ready(failed(e.to_string())),
            }
        };
        // Restore blocking mode before the result is delivered.
        self.stream = None;
        progress
    }
}

pub struct Finishing {
    connection: Option<Connection>,
    remaining: usize,
    until: std::time::Instant,
    immediate: Option<rust_api::http_connection_finish_outcome>,
}
fn finish_failure(e: std::io::Error) -> rust_api::http_connection_finish_outcome {
    if e.kind() == std::io::ErrorKind::NotConnected {
        rust_api::http_connection_finish_outcome::closed
    } else {
        rust_api::http_connection_finish_outcome::failed(e.to_string())
    }
}
/// Half-close writes, then drain at most 1 MiB + 16 KiB for at most 100 ms, and
/// release the descriptor, as the blocking `http_connection_close` does.
pub fn http_connection_finish(
    connection: Connection,
) -> Box<dyn rust_api::http_connection_finish_operation> {
    let mut state = Finishing {
        connection: None,
        remaining: BODY_LIMIT + HEADER_LIMIT,
        until: std::time::Instant::now() + DRAIN_TIMEOUT,
        immediate: None,
    };
    if let Err(e) = connection.stream.set_nonblocking(true) {
        state.immediate = Some(finish_failure(e));
        return Box::new(state);
    }
    match connection.stream.shutdown(std::net::Shutdown::Write) {
        Ok(()) => state.connection = Some(connection),
        Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
            state.immediate = Some(rust_api::http_connection_finish_outcome::closed);
        }
        Err(e) => {
            state.immediate = Some(finish_failure(std::io::Error::new(
                e.kind(),
                format!("write-half shutdown: {e}"),
            )));
        }
    }
    Box::new(state)
}
impl rust_api::http_connection_finish_operation for Finishing {
    fn poll(&mut self) -> rust_api::Poll<rust_api::http_connection_finish_outcome> {
        use rust_api::http_connection_finish_outcome::*;
        if let Some(outcome) = self.immediate.take() {
            return rust_api::Poll::Ready(outcome);
        }
        let mut chunk = [0; 4096];
        let progress = loop {
            let now = std::time::Instant::now();
            if self.remaining == 0 || now >= self.until {
                break closed;
            }
            let limit = self.remaining.min(chunk.len());
            let mut stream = &self.connection.as_ref().unwrap().stream;
            match stream.read(&mut chunk[..limit]) {
                Ok(0) => break closed,
                Ok(n) => self.remaining -= n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return rust_api::Poll::Pending(
                        rust_api::Wait::readable(stream).or_after(self.until - now),
                    );
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::NotConnected
                    ) =>
                {
                    break closed;
                }
                Err(e) => {
                    break failed(format!("draining connection: {e}"));
                }
            }
        };
        // The descriptor is released before the result is delivered.
        self.connection = None;
        rust_api::Poll::Ready(progress)
    }
}
