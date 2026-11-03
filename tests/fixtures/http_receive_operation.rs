// Private owned receive state. The test supplies a deterministic Connection.
use super::*;
enum Phase {
    Headers(Vec<u8>),
    Body {
        method: String,
        target: String,
        headers: BTreeMap<String, String>,
        length: usize,
        body: Vec<u8>,
    },
    Immediate(Option<ReceiveOutcome>),
    Done,
}
pub enum Progress {
    Again,
    Wait(Instant),
    Complete(ReceiveOutcome),
}
pub struct ReceiveState {
    phase: Phase,
    deadline: Option<Instant>,
}
impl ReceiveState {
    pub fn start(connection: &Connection) -> Self {
        if connection.receive_started.replace(true) {
            return Self {
                phase: Phase::Immediate(Some(ReceiveOutcome::failed(
                    "request reception already started".into(),
                ))),
                deadline: None,
            };
        }
        Self {
            phase: Phase::Headers(vec![]),
            deadline: Some(connection.stream.now() + IO_TIMEOUT),
        }
    }
    fn complete(&mut self, outcome: ReceiveOutcome) -> Result<Progress, &'static str> {
        self.phase = Phase::Done;
        Ok(Progress::Complete(outcome))
    }
    pub fn step(&mut self, connection: &Connection) -> Result<Progress, &'static str> {
        use ReceiveOutcome::*;
        loop {
            match &mut self.phase {
                Phase::Done => return Err("receive operation already completed"),
                Phase::Immediate(outcome) => {
                    let outcome = outcome.take().unwrap();
                    return self.complete(outcome);
                }
                Phase::Headers(input) => {
                    assert!(input.len() <= HEADER_LIMIT);
                    let mut slots = [httparse::EMPTY_HEADER; 64];
                    let mut parsed = httparse::Request::new(&mut slots);
                    let result = parsed.parse(input);
                    connection.head.set(parsed.method == Some("HEAD"));
                    match result {
                        Ok(httparse::Status::Complete(offset)) => {
                            if offset > HEADER_LIMIT {
                                return self.complete(headers_too_large);
                            }
                            if input[..offset].iter().enumerate().any(|(i, b)| {
                                (*b == b'\n' && (i == 0 || input[i - 1] != b'\r'))
                                    || (*b == b'\r' && input.get(i + 1) != Some(&b'\n'))
                            }) {
                                return self.complete(malformed("headers require CRLF".into()));
                            }
                            if parsed.version != Some(1) {
                                return self.complete(malformed("HTTP/1.1 required".into()));
                            }
                            let mut headers = BTreeMap::new();
                            for header in parsed.headers.iter() {
                                let value = match std::str::from_utf8(header.value) {
                                    Ok(v) => v.trim_matches([' ', '\t']),
                                    Err(_) => {
                                        return self
                                            .complete(malformed("header is not UTF-8".into()));
                                    }
                                };
                                if headers
                                    .insert(header.name.to_ascii_lowercase(), value.to_owned())
                                    .is_some()
                                {
                                    return self.complete(malformed(
                                        "duplicate headers are unsupported".into(),
                                    ));
                                }
                            }
                            if headers.get("host").is_none_or(|v| v.is_empty()) {
                                return self
                                    .complete(malformed("one Host header is required".into()));
                            }
                            let method = parsed.method.unwrap().to_owned();
                            let target = parsed.path.unwrap().to_owned();
                            if !target.starts_with('/')
                                || target.starts_with("//")
                                || target.contains('#')
                            {
                                return self.complete(malformed(
                                    "origin-form request target required".into(),
                                ));
                            }
                            if headers.contains_key("transfer-encoding")
                                && headers.contains_key("content-length")
                            {
                                return self
                                    .complete(malformed("conflicting message framing".into()));
                            }
                            if headers.contains_key("transfer-encoding") {
                                return self.complete(unsupported_transfer);
                            }
                            if headers.contains_key("expect") {
                                return self.complete(unsupported_expectation);
                            }
                            let length = match headers.get("content-length") {
                                None => 0,
                                Some(s)
                                    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) =>
                                {
                                    match s.parse::<u64>() {
                                        Ok(n) if n <= BODY_LIMIT as u64 => n as usize,
                                        _ => return self.complete(body_too_large),
                                    }
                                }
                                Some(_) => {
                                    return self
                                        .complete(malformed("invalid Content-Length".into()));
                                }
                            };
                            let body = input[offset..].iter().copied().take(length).collect();
                            self.phase = Phase::Body {
                                method,
                                target,
                                headers,
                                length,
                                body,
                            };
                            continue;
                        }
                        Ok(httparse::Status::Partial) => {
                            if input.len() >= HEADER_LIMIT {
                                return self.complete(headers_too_large);
                            }
                        }
                        Err(httparse::Error::TooManyHeaders) => {
                            return self.complete(headers_too_large);
                        }
                        Err(e) => return self.complete(malformed(e.to_string())),
                    }
                }
                Phase::Body {
                    method,
                    target,
                    headers,
                    length,
                    body,
                } => {
                    assert!(body.len() <= *length && *length <= BODY_LIMIT);
                    if body.len() == *length {
                        let outcome = match String::from_utf8(std::mem::take(body)) {
                            Ok(body) => request(
                                std::mem::take(method),
                                std::mem::take(target),
                                std::mem::take(headers),
                                body,
                            ),
                            Err(_) => malformed("body is not UTF-8".into()),
                        };
                        return self.complete(outcome);
                    }
                }
            }
            // Like read_before, check expiry only when another read is required.
            let deadline = self.deadline.unwrap();
            if connection.stream.now() >= deadline {
                return self.complete(timed_out);
            }
            let mut chunk = [0; 4096];
            let limit = match &self.phase {
                Phase::Headers(input) => 2048.min(HEADER_LIMIT - input.len()),
                Phase::Body { length, body, .. } => 4096.min(length - body.len()),
                _ => unreachable!(),
            };
            match connection.stream.try_read(&mut chunk[..limit]) {
                Ok(0) => {
                    let outcome = match &self.phase {
                        Phase::Headers(input) if input.is_empty() => disconnected,
                        Phase::Headers(_) => malformed("incomplete headers".into()),
                        Phase::Body { .. } => malformed("incomplete body".into()),
                        _ => unreachable!(),
                    };
                    return self.complete(outcome);
                }
                Ok(n) => {
                    match &mut self.phase {
                        Phase::Headers(input) => input.extend_from_slice(&chunk[..n]),
                        Phase::Body { body, .. } => body.extend_from_slice(&chunk[..n]),
                        _ => unreachable!(),
                    }
                    return Ok(Progress::Again);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    return Ok(Progress::Again);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(Progress::Wait(deadline));
                }
                Err(e) => return self.complete(io_failure(e)),
            }
        }
    }
}
