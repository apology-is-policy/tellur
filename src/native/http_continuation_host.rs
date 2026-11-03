// Appended to the deterministic HTTP transport by the private continuation test.
use std::rc::Rc;
thread_local! {static TRACE: Rc<RefCell<Vec<String>>> = Default::default();}
pub struct TraceGuard;
impl Drop for TraceGuard {
    fn drop(&mut self) {
        if let Ok(path) = std::env::var("TELLUR_OPERATION_LOG") {
            TRACE.with(|t| std::fs::write(path, t.borrow().join("\n")).unwrap());
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        assert!(self.alive.replace(false));
        self.trace.borrow_mut().push(format!(
            "resource drop head={} bytes={}",
            self.head.get(),
            self.stream.wire.borrow().consumed
        ));
    }
}
pub struct OwnedReceive {
    state: Option<operation::ReceiveState>,
    alive: Rc<Cell<bool>>,
    trace: Rc<RefCell<Vec<String>>>,
    until: Option<Instant>,
}
impl OwnedReceive {
    pub fn start(connection: &Connection) -> Self {
        connection.trace.borrow_mut().push("state start".into());
        Self {
            state: Some(operation::ReceiveState::start(connection)),
            alive: connection.alive.clone(),
            trace: connection.trace.clone(),
            until: None,
        }
    }
    pub fn poll(&mut self, connection: &Connection) -> Result<operation::Progress, String> {
        assert!(self.alive.get());
        assert!(Rc::ptr_eq(&self.alive, &connection.alive));
        let progress = self
            .state
            .as_mut()
            .ok_or("state already consumed")?
            .step(connection)?;
        match &progress {
            operation::Progress::Wait(until) => {
                assert_eq!(*until, connection.stream.base + IO_TIMEOUT);
                assert_eq!(*self.until.get_or_insert(*until), *until);
                self.trace
                    .borrow_mut()
                    .push("wait original deadline".into());
            }
            operation::Progress::Complete(_) => {
                self.state.take();
                self.trace.borrow_mut().push("state consumed".into());
            }
            operation::Progress::Again => {}
        }
        Ok(progress)
    }
}
impl Drop for OwnedReceive {
    fn drop(&mut self) {
        assert!(self.alive.get());
        self.trace.borrow_mut().push("state drop".into());
    }
}
impl Connection {
    pub fn observed(&self) -> (usize, usize, Instant) {
        let w = self.stream.wire.borrow();
        (w.reads, w.consumed, w.now)
    }
    pub fn wake(&self, deadline: Instant) {
        self.stream.wake(deadline);
    }
}
fn event(at: u64, input: Input) -> Event {
    Event { at, cost: 0, input }
}
pub fn open_case(name: String) -> rust_api::open_case_outcome {
    use Input::*;
    let bytes = |s: &str| Bytes(s.as_bytes().to_vec());
    let events = match name.as_str() {
        "request" => vec![event(
            0,
            bytes("GET /hello HTTP/1.1\r\nHost: local\r\nX-Tag: value\r\n\r\n"),
        )],
        "split" => vec![
            event(0, Interrupted),
            event(0, Blocked),
            event(
                7,
                bytes("HEAD /unicode HTTP/1.1\r\nHost: local\r\nContent-Length: 4\r\n\r\n"),
            ),
            event(11, Bytes(vec![0xf0, 0x9f])),
            event(13, Blocked),
            event(19, Bytes(vec![0x99, 0x82])),
        ],
        "malformed" => vec![event(7, bytes("GET / HTTP/1.0\r\nHost: local\r\n\r\n"))],
        "headers_too_large" => vec![event(
            7,
            bytes(&format!(
                "GET / HTTP/1.1\r\nHost: local\r\nX: {}",
                "x".repeat(HEADER_LIMIT)
            )),
        )],
        "body_too_large" => vec![event(
            7,
            bytes("GET / HTTP/1.1\r\nHost: local\r\nContent-Length: 1048577\r\n\r\n"),
        )],
        "unsupported_transfer" => vec![event(
            7,
            bytes("GET / HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n"),
        )],
        "unsupported_expectation" => vec![event(
            7,
            bytes("GET / HTTP/1.1\r\nHost: local\r\nExpect: 100-continue\r\n\r\n"),
        )],
        "timed_out" => vec![
            event(300, Blocked),
            event(700, Interrupted),
            event(1500, Blocked),
            event(2100, bytes("GET / HTTP/1.1\r\nHost: local\r\n\r\n")),
        ],
        "body_deadline" => vec![
            event(
                1999,
                bytes("POST / HTTP/1.1\r\nHost: local\r\nContent-Length: 2\r\n\r\na"),
            ),
            event(2001, bytes("b")),
        ],
        "disconnected" => vec![event(7, Eof)],
        "failed" => vec![event(7, Error(io::ErrorKind::PermissionDenied))],
        "incomplete" => vec![event(7, bytes("HEAD / HTTP/1.1\r\nHost:")), event(11, Eof)],
        _ => panic!("unknown case"),
    };
    rust_api::open_case_outcome::opened(Connection::new(events))
}
pub fn close_test(connection: Connection) -> rust_api::close_test_outcome {
    drop(connection);
    rust_api::close_test_outcome::done
}
pub fn http_receive(connection: &Connection) -> ReceiveOutcome {
    baseline::http_receive(connection)
}
