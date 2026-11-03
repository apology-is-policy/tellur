use crate::rust_api;
use rust_api::http_receive_outcome as ReceiveOutcome;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    io::{self, Read},
    time::{Duration, Instant},
};
#[derive(Clone)]
enum Input {
    Bytes(Vec<u8>),
    Interrupted,
    Blocked,
    Eof,
    Error(io::ErrorKind),
}
#[derive(Clone)]
struct Event {
    at: u64,
    cost: u64,
    input: Input,
}
struct Wire {
    now: Instant,
    until: Option<Instant>,
    events: VecDeque<Event>,
    offset: usize,
    consumed: usize,
}
struct Stream {
    base: Instant,
    wire: RefCell<Wire>,
}
impl Stream {
    fn now(&self) -> Instant {
        self.wire.borrow().now
    }
    fn set_read_timeout(&self, remaining: Option<Duration>) -> io::Result<()> {
        let mut w = self.wire.borrow_mut();
        w.until = remaining.map(|d| w.now + d);
        Ok(())
    }
    fn try_read(&self, output: &mut [u8]) -> io::Result<usize> {
        self.read_mode(output, false)
    }
    fn read_mode(&self, output: &mut [u8], blocking: bool) -> io::Result<usize> {
        let mut w = self.wire.borrow_mut();
        loop {
            let Some(event) = w.events.front().cloned() else {
                if blocking {
                    w.now = w.until.unwrap();
                    return Err(io::ErrorKind::TimedOut.into());
                }
                return Err(io::ErrorKind::WouldBlock.into());
            };
            let at = self.base + Duration::from_millis(event.at);
            if at > w.now {
                if !blocking {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                if at >= w.until.unwrap() {
                    w.now = w.until.unwrap();
                    return Err(io::ErrorKind::TimedOut.into());
                }
                w.now = at;
            }
            let blocked = matches!(&event.input, Input::Blocked);
            let result = match event.input {
                Input::Bytes(data) => {
                    let n = output.len().min(data.len() - w.offset);
                    assert!(n > 0);
                    output[..n].copy_from_slice(&data[w.offset..w.offset + n]);
                    w.offset += n;
                    w.consumed += n;
                    if w.offset < data.len() {
                        return Ok(n);
                    }
                    Ok(n)
                }
                Input::Interrupted => Err(io::ErrorKind::Interrupted.into()),
                Input::Blocked => Err(io::ErrorKind::WouldBlock.into()),
                Input::Eof => Ok(0),
                Input::Error(kind) => Err(io::Error::new(kind, "injected read error")),
            };
            w.events.pop_front();
            w.offset = 0;
            w.now += Duration::from_millis(event.cost);
            if blocking && blocked {
                if w.now >= w.until.unwrap() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                continue;
            }
            return result;
        }
    }
    fn wake(&self, deadline: Instant) {
        let mut w = self.wire.borrow_mut();
        let next = w
            .events
            .front()
            .map(|e| self.base + Duration::from_millis(e.at))
            .unwrap_or(deadline);
        w.now = w.now.max(next.min(deadline));
    }
}
impl Read for &Stream {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.read_mode(output, true)
    }
}
struct Connection {
    stream: Stream,
    receive_started: Cell<bool>,
    head: Cell<bool>,
}
impl Connection {
    fn new(events: Vec<Event>) -> Self {
        let base = Instant::now();
        Self {
            stream: Stream {
                base,
                wire: RefCell::new(Wire {
                    now: base,
                    until: None,
                    events: events.into(),
                    offset: 0,
                    consumed: 0,
                }),
            },
            receive_started: Cell::new(false),
            head: Cell::new(false),
        }
    }
}
fn outcome(value: ReceiveOutcome) -> serde_json::Value {
    use ReceiveOutcome::*;
    match value {
        request(method, target, headers, body) => {
            serde_json::json!(["request", method, target, headers, body])
        }
        malformed(message) => serde_json::json!(["malformed", message]),
        failed(message) => serde_json::json!(["failed", message]),
        headers_too_large => serde_json::json!(["headers_too_large"]),
        body_too_large => serde_json::json!(["body_too_large"]),
        unsupported_transfer => serde_json::json!(["unsupported_transfer"]),
        unsupported_expectation => serde_json::json!(["unsupported_expectation"]),
        timed_out => serde_json::json!(["timed_out"]),
        disconnected => serde_json::json!(["disconnected"]),
    }
}
fn event(at: u64, input: Input) -> Event {
    Event { at, cost: 0, input }
}
fn compare(events: Vec<Event>, expected: &str) -> usize {
    let ordinary = Connection::new(events.clone());
    let reference = outcome(baseline::http_receive(&ordinary));
    assert_eq!(reference[0], expected, "reference: {reference}");
    let c = Connection::new(events);
    let mut state = operation::ReceiveState::start(&c);
    let deadline = c.stream.base + IO_TIMEOUT;
    let mut waits = 0;
    let actual = loop {
        match state.step(&c).unwrap() {
            operation::Progress::Again => {}
            operation::Progress::Wait(until) => {
                assert_eq!(until, deadline);
                waits += 1;
                assert!(waits < 10000);
                c.stream.wake(until);
            }
            operation::Progress::Complete(value) => break outcome(value),
        }
    };
    assert_eq!(actual, reference);
    assert_eq!(c.head.get(), ordinary.head.get());
    assert_eq!(
        c.stream.wire.borrow().consumed,
        ordinary.stream.wire.borrow().consumed
    );
    assert!(state.step(&c).is_err());
    let consumed = c.stream.wire.borrow().consumed;
    let mut second = operation::ReceiveState::start(&c);
    let operation::Progress::Complete(value) = second.step(&c).unwrap() else {
        panic!("duplicate receive waited")
    };
    assert_eq!(outcome(value), outcome(baseline::http_receive(&ordinary)));
    assert_eq!(c.stream.wire.borrow().consumed, consumed);
    waits
}
pub fn run() {
    let cases:Vec<(Vec<u8>,&str)>=vec![
        (b"GET /hello HTTP/1.1\r\nHost: local\r\n\r\n".to_vec(),"request"),
        ("POST / HTTP/1.1\r\nHost: local\r\nContent-Length: 4\r\n\r\n🙂ignored".as_bytes().to_vec(),"request"),
        (b"HEAD / HTTP/1.1\r\nHost: local\r\n\r\n".to_vec(),"request"),
        (b"GET / HTTP/1.1\nHost: local\n\n".to_vec(),"malformed"),
        (b"GET / HTTP/1.0\r\nHost: local\r\n\r\n".to_vec(),"malformed"),
        (b"GET / HTTP/1.1\r\n\r\n".to_vec(),"malformed"),
        (b"HEAD / HTTP/1.1\r\nHost: a\r\nHOST: b\r\n\r\n".to_vec(),"malformed"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nX: \xff\r\n\r\n".to_vec(),"malformed"),
        (b"GET //host HTTP/1.1\r\nHost: local\r\n\r\n".to_vec(),"malformed"),
        (b"GET /#fragment HTTP/1.1\r\nHost: local\r\n\r\n".to_vec(),"malformed"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),"malformed"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),"unsupported_transfer"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nExpect: 100-continue\r\n\r\n".to_vec(),"unsupported_expectation"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nContent-Length: -1\r\n\r\n".to_vec(),"malformed"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nContent-Length: 18446744073709551616\r\n\r\n".to_vec(),"body_too_large"),
        (b"GET / HTTP/1.1\r\nHost: local\r\nContent-Length: 1048577\r\n\r\n".to_vec(),"body_too_large"),
        (b"POST / HTTP/1.1\r\nHost: local\r\nContent-Length: 1\r\n\r\n\xff".to_vec(),"malformed"),
    ];
    let mut runs = 0;
    for (bytes, expected) in cases {
        for split in 0..=bytes.len() {
            for delayed in [false, true] {
                let mut events = vec![event(0, Input::Interrupted), event(0, Input::Blocked)];
                if split > 0 {
                    events.push(event(
                        if delayed { 7 } else { 0 },
                        Input::Bytes(bytes[..split].to_vec()),
                    ));
                }
                events.push(event(if delayed { 11 } else { 0 }, Input::Interrupted));
                events.push(event(if delayed { 13 } else { 0 }, Input::Blocked));
                if split < bytes.len() {
                    events.push(event(
                        if delayed { 19 } else { 0 },
                        Input::Bytes(bytes[split..].to_vec()),
                    ));
                }
                events.push(event(30, Input::Eof));
                assert!(compare(events, expected) > 0);
                runs += 1;
            }
        }
    }
    for (prefix, expected) in [
        (b"".as_slice(), "disconnected"),
        (b"HEAD / HTTP/1.1\r\nHost:", "malformed"),
        (
            b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\na",
            "malformed",
        ),
    ] {
        let mut events = vec![];
        if !prefix.is_empty() {
            events.push(event(0, Input::Bytes(prefix.to_vec())));
        }
        events.push(event(1, Input::Eof));
        compare(events, expected);
        runs += 1;
    }
    for kind in [
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::TimedOut,
        io::ErrorKind::PermissionDenied,
    ] {
        let expected = match kind {
            io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => "disconnected",
            io::ErrorKind::TimedOut => "timed_out",
            _ => "failed",
        };
        compare(vec![event(0, Input::Error(kind))], expected);
        runs += 1;
    }
    // Repeated empty readiness and Interrupted cannot move the original deadline.
    compare(
        vec![
            event(300, Input::Blocked),
            event(700, Input::Interrupted),
            event(1500, Input::Blocked),
            event(
                2100,
                Input::Bytes(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec()),
            ),
        ],
        "timed_out",
    );
    runs += 1;
    compare(
        vec![
            event(
                1999,
                Input::Bytes(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\na".to_vec()),
            ),
            event(2001, Input::Bytes(b"b".to_vec())),
        ],
        "timed_out",
    );
    runs += 1;
    // A completed buffer is handled before checking whether another read may start.
    compare(
        vec![Event {
            at: 0,
            cost: 2100,
            input: Input::Bytes(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec()),
        }],
        "request",
    );
    runs += 1;
    compare(
        vec![Event {
            at: 0,
            cost: 2100,
            input: Input::Bytes(b"HEAD / HTTP/1.1\r\n".to_vec()),
        }],
        "timed_out",
    );
    runs += 1;
    let prefix = "GET / HTTP/1.1\r\nHost: x\r\nX: ";
    let exact = format!(
        "{prefix}{}\r\n\r\n",
        "a".repeat(HEADER_LIMIT - prefix.len() - 4)
    );
    assert_eq!(exact.len(), HEADER_LIMIT);
    compare(vec![event(0, Input::Bytes(exact.into_bytes()))], "request");
    runs += 1;
    let huge_header = format!(
        "GET / HTTP/1.1\r\nHost: x\r\nX: {}",
        "a".repeat(HEADER_LIMIT)
    );
    compare(
        vec![event(0, Input::Bytes(huge_header.into_bytes()))],
        "headers_too_large",
    );
    runs += 1;
    let many = format!(
        "GET / HTTP/1.1\r\nHost: x\r\n{}\r\n",
        (0..64).map(|i| format!("X{i}: a\r\n")).collect::<String>()
    );
    compare(
        vec![event(0, Input::Bytes(many.into_bytes()))],
        "headers_too_large",
    );
    runs += 1;
    let maximum = format!(
        "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: {BODY_LIMIT}\r\n\r\n{}",
        "a".repeat(BODY_LIMIT)
    );
    compare(
        vec![event(0, Input::Bytes(maximum.into_bytes()))],
        "request",
    );
    runs += 1;
    println!("receive comparisons: {runs}");
}
