// Polled request reception for the bounded loopback HTTP profile. The awaiting
// Tellur task parks while the socket would block; other tasks keep running. The
// profile, limits and the single two-second deadline match the blocking baseline.
use super::http::Connection;
use crate::rust_api::{self, http_receive_outcome::*};
use std::{collections::BTreeMap, io::Read};

const HEADER_LIMIT: usize = 16384;
const BODY_LIMIT: usize = 1048576;
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

struct Head {
    offset: usize,
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    length: usize,
}
pub struct Receiving {
    // A duplicate handle restores blocking mode for responses, even when stopped.
    stream: Option<std::net::TcpStream>,
    input: Vec<u8>,
    head: Option<Head>,
    until: std::time::Instant,
    immediate: Option<rust_api::http_receive_outcome>,
}
impl Drop for Receiving {
    fn drop(&mut self) {
        if let Some(stream) = &self.stream {
            let _ = stream.set_nonblocking(false);
        }
    }
}
pub fn http_receive(connection: &Connection) -> Box<dyn rust_api::http_receive_operation> {
    let mut state = Receiving {
        stream: None,
        input: Vec::new(),
        head: None,
        until: std::time::Instant::now() + IO_TIMEOUT,
        immediate: None,
    };
    if connection.receive_started.replace(true) {
        state.immediate = Some(failed("request reception already started".into()));
        return Box::new(state);
    }
    match connection
        .stream
        .try_clone()
        .and_then(|stream| stream.set_nonblocking(true).map(|()| stream))
    {
        Ok(stream) => state.stream = Some(stream),
        Err(e) => state.immediate = Some(failed(format!("nonblocking receive: {e}"))),
    }
    Box::new(state)
}
fn io_failure(e: std::io::Error) -> rust_api::http_receive_outcome {
    match e.kind() {
        std::io::ErrorKind::TimedOut => timed_out,
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => disconnected,
        _ => failed(e.to_string()),
    }
}
/// Parse the buffered head: None while incomplete, or a terminal outcome.
fn parse_head(
    connection: &Connection,
    input: &[u8],
) -> Option<Result<Head, rust_api::http_receive_outcome>> {
    let mut slots = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut slots);
    let result = parsed.parse(input);
    connection.head.set(parsed.method == Some("HEAD"));
    let offset = match result {
        Ok(httparse::Status::Complete(offset)) => offset,
        Ok(httparse::Status::Partial) if input.len() >= HEADER_LIMIT => {
            return Some(Err(headers_too_large));
        }
        Ok(httparse::Status::Partial) => return None,
        Err(httparse::Error::TooManyHeaders) => return Some(Err(headers_too_large)),
        Err(e) => return Some(Err(malformed(e.to_string()))),
    };
    if offset > HEADER_LIMIT {
        return Some(Err(headers_too_large));
    }
    // httparse tolerates lone LF; this bounded profile requires CRLF.
    if input[..offset].iter().enumerate().any(|(i, b)| {
        (*b == b'\n' && (i == 0 || input[i - 1] != b'\r'))
            || (*b == b'\r' && input.get(i + 1) != Some(&b'\n'))
    }) {
        return Some(Err(malformed("headers require CRLF".into())));
    }
    if parsed.version != Some(1) {
        return Some(Err(malformed("HTTP/1.1 required".into())));
    }
    let mut headers = BTreeMap::new();
    for header in parsed.headers.iter() {
        let value = match std::str::from_utf8(header.value) {
            Ok(v) => v.trim_matches([' ', '\t']),
            Err(_) => return Some(Err(malformed("header is not UTF-8".into()))),
        };
        if headers
            .insert(header.name.to_ascii_lowercase(), value.to_owned())
            .is_some()
        {
            return Some(Err(malformed("duplicate headers are unsupported".into())));
        }
    }
    if headers.get("host").is_none_or(|v: &String| v.is_empty()) {
        return Some(Err(malformed("one Host header is required".into())));
    }
    let target = parsed.path.unwrap().to_owned();
    if !target.starts_with('/') || target.starts_with("//") || target.contains('#') {
        return Some(Err(malformed("origin-form request target required".into())));
    }
    if headers.contains_key("transfer-encoding") && headers.contains_key("content-length") {
        return Some(Err(malformed("conflicting message framing".into())));
    }
    if headers.contains_key("transfer-encoding") {
        return Some(Err(unsupported_transfer));
    }
    if headers.contains_key("expect") {
        return Some(Err(unsupported_expectation));
    }
    let length = match headers.get("content-length") {
        None => 0,
        Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => match s.parse::<u64>()
        {
            Ok(n) if n <= BODY_LIMIT as u64 => n as usize,
            _ => return Some(Err(body_too_large)),
        },
        Some(_) => return Some(Err(malformed("invalid Content-Length".into()))),
    };
    Some(Ok(Head {
        offset,
        method: parsed.method.unwrap().to_owned(),
        target,
        headers,
        length,
    }))
}
impl Receiving {
    fn step(&mut self, connection: &Connection) -> rust_api::Poll<rust_api::http_receive_outcome> {
        if let Some(outcome) = self.immediate.take() {
            return rust_api::Poll::Ready(outcome);
        }
        loop {
            if self.head.is_none() {
                match parse_head(connection, &self.input) {
                    Some(Ok(head)) => self.head = Some(head),
                    Some(Err(outcome)) => return rust_api::Poll::Ready(outcome),
                    None => {}
                }
            }
            if let Some(head) = &self.head
                && self.input.len() >= head.offset + head.length
            {
                let head = self.head.take().unwrap();
                let body = self.input[head.offset..head.offset + head.length].to_vec();
                return rust_api::Poll::Ready(match String::from_utf8(body) {
                    Ok(body) => request(head.method, head.target, head.headers, body),
                    Err(_) => malformed("body is not UTF-8".into()),
                });
            }
            // As in the blocking profile, the deadline is checked before every read.
            let now = std::time::Instant::now();
            if now >= self.until {
                return rust_api::Poll::Ready(timed_out);
            }
            let limit = match &self.head {
                None => 2048.min(HEADER_LIMIT - self.input.len()),
                Some(head) => 4096.min(head.offset + head.length - self.input.len()),
            };
            let mut chunk = [0; 4096];
            let mut stream = self.stream.as_ref().unwrap();
            match stream.read(&mut chunk[..limit]) {
                Ok(0) => {
                    return rust_api::Poll::Ready(match &self.head {
                        None if self.input.is_empty() => disconnected,
                        None => malformed("incomplete headers".into()),
                        Some(_) => malformed("incomplete body".into()),
                    });
                }
                Ok(n) => self.input.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return rust_api::Poll::Pending(
                        rust_api::Wait::readable(self.stream.as_ref().unwrap())
                            .or_after(self.until - now),
                    );
                }
                Err(e) => return rust_api::Poll::Ready(io_failure(e)),
            }
        }
    }
}
impl rust_api::http_receive_operation for Receiving {
    fn poll(&mut self, connection: &Connection) -> rust_api::Poll<rust_api::http_receive_outcome> {
        let progress = self.step(connection);
        if matches!(progress, rust_api::Poll::Ready(_))
            && let Some(stream) = self.stream.take()
        {
            // Responses use blocking writes with their own timeouts.
            if let Err(e) = stream.set_nonblocking(false) {
                return rust_api::Poll::Ready(failed(format!("blocking response mode: {e}")));
            }
        }
        progress
    }
}
