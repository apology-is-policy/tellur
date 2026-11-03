// Private concrete transport. Parser state stays identical to the deterministic test.
use crate::rust_api;
use operation::{Progress, ReceiveState};
use rust_api::http_receive_outcome as ReceiveOutcome;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{self, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    rc::Rc,
    time::{Duration, Instant},
};
type Trace = Rc<RefCell<Vec<&'static str>>>;

struct Backend {
    poll: RefCell<mio::Poll>,
    identity: Rc<()>,
    next: Cell<usize>,
    entries: RefCell<BTreeMap<usize, bool>>,
    errors: RefCell<Vec<String>>,
    registered: Cell<usize>,
    removed: Cell<usize>,
}
#[derive(Clone)]
struct Notice {
    identity: Rc<()>,
    token: usize,
}
impl Backend {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            poll: RefCell::new(mio::Poll::new().unwrap()),
            identity: Rc::new(()),
            next: Cell::new(1),
            entries: RefCell::new(BTreeMap::new()),
            errors: RefCell::new(vec![]),
            registered: Cell::new(0),
            removed: Cell::new(0),
        })
    }
    fn allocate(&self) -> io::Result<usize> {
        let token = self.next.get();
        self.next.set(
            token
                .checked_add(1)
                .ok_or_else(|| io::Error::other("generation exhausted"))?,
        );
        Ok(token)
    }
    fn claim(&self, notice: &Notice) -> bool {
        if !Rc::ptr_eq(&self.identity, &notice.identity) {
            return false;
        }
        let mut entries = self.entries.borrow_mut();
        match entries.get_mut(&notice.token) {
            Some(delivered) if !*delivered => {
                *delivered = true;
                true
            }
            _ => false,
        }
    }
    fn poll(&self, deadline: Instant) -> Vec<Notice> {
        let mut events = mio::Events::with_capacity(16);
        loop {
            // Recompute from the original deadline even after Interrupted/spurious returns.
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.poll.borrow_mut().poll(&mut events, Some(remaining)) {
                Ok(()) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => panic!("poll: {e}"),
            }
        }
        // Close/error flags are only hints. The actual read supplies EOF/errors.
        events
            .iter()
            .map(|e| Notice {
                identity: self.identity.clone(),
                token: e.token().0,
            })
            .collect()
    }
    fn wait_notice(&self, token: usize, deadline: Instant) -> Option<Notice> {
        while Instant::now() < deadline {
            for notice in self.poll(deadline) {
                if notice.token == token && self.entries.borrow().contains_key(&token) {
                    return Some(notice);
                }
                assert!(
                    !self.claim(&notice),
                    "unexpected live interest in single-receiver test"
                );
            }
        }
        None
    }
    fn finished(&self) {
        assert!(self.entries.borrow().is_empty());
        assert!(
            self.errors.borrow().is_empty(),
            "{:?}",
            self.errors.borrow()
        );
        assert_eq!(self.registered.get(), self.removed.get());
        for event in self.poll(Instant::now()) {
            assert!(!self.claim(&event));
        }
    }
}

struct Socket {
    io: RefCell<mio::net::TcpStream>,
    // The poller outlives its socket, including after explicit deregistration.
    backend: Rc<Backend>,
    registered: Cell<bool>,
    trace: Trace,
    reads: Cell<usize>,
    bytes: Cell<usize>,
    blocked: Cell<usize>,
}
impl Drop for Socket {
    fn drop(&mut self) {
        assert!(!self.registered.get());
        self.trace.borrow_mut().push("socket_drop");
    }
}
struct Stream(Rc<Socket>);
impl Stream {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn try_read(&self, output: &mut [u8]) -> io::Result<usize> {
        assert!(self.0.registered.get());
        self.0.reads.set(self.0.reads.get() + 1);
        let result = (&*self.0.io.borrow()).read(output);
        match &result {
            Ok(n) => self.0.bytes.set(self.0.bytes.get() + n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.0.blocked.set(self.0.blocked.get() + 1)
            }
            _ => {}
        }
        result
    }
}
struct Connection {
    stream: Stream,
    receive_started: Cell<bool>,
    head: Cell<bool>,
}
fn pair(backend: &Rc<Backend>) -> (Connection, TcpStream, Trace) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client =
        TcpStream::connect_timeout(&listener.local_addr().unwrap(), Duration::from_secs(2))
            .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.set_nodelay(true).unwrap();
    let (server, _) = listener.accept().unwrap();
    server.set_nonblocking(true).unwrap();
    let trace = Rc::new(RefCell::new(vec![]));
    (
        Connection {
            stream: Stream(Rc::new(Socket {
                io: RefCell::new(mio::net::TcpStream::from_std(server)),
                backend: backend.clone(),
                registered: Cell::new(false),
                trace: trace.clone(),
                reads: Cell::new(0),
                bytes: Cell::new(0),
                blocked: Cell::new(0),
            })),
            receive_started: Cell::new(false),
            head: Cell::new(false),
        },
        client,
        trace,
    )
}

struct Subscription {
    socket: Rc<Socket>,
    token: usize,
}
impl Subscription {
    fn new(socket: &Rc<Socket>) -> io::Result<Self> {
        assert!(!socket.registered.get());
        let b = &socket.backend;
        let token = b.allocate()?;
        b.poll.borrow().registry().register(
            &mut *socket.io.borrow_mut(),
            mio::Token(token),
            mio::Interest::READABLE,
        )?;
        socket.registered.set(true);
        b.entries.borrow_mut().insert(token, false);
        b.registered.set(b.registered.get() + 1);
        socket.trace.borrow_mut().push("register");
        Ok(Self {
            socket: socket.clone(),
            token,
        })
    }
    fn arm(&mut self) -> io::Result<()> {
        let b = &self.socket.backend;
        let token = b.allocate()?;
        b.poll.borrow().registry().reregister(
            &mut *self.socket.io.borrow_mut(),
            mio::Token(token),
            mio::Interest::READABLE,
        )?;
        assert!(b.entries.borrow_mut().remove(&self.token).is_some());
        b.entries.borrow_mut().insert(token, false);
        self.token = token;
        Ok(())
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let b = &self.socket.backend;
        // Invalidate copied notifications even if OS removal reports an error.
        assert!(b.entries.borrow_mut().remove(&self.token).is_some());
        match b
            .poll
            .borrow()
            .registry()
            .deregister(&mut *self.socket.io.borrow_mut())
        {
            Ok(()) => self.socket.trace.borrow_mut().push("unregister"),
            Err(e) => {
                b.errors.borrow_mut().push(e.to_string());
                self.socket.trace.borrow_mut().push("unregister_failed");
            }
        }
        self.socket.registered.set(false);
        b.removed.set(b.removed.get() + 1);
        // Safe Mio owns the handle; failure is recorded and fails finished(), not
        // treated as an infallible cleanup guarantee for a future public adapter.
    }
}

struct Session {
    subscription: Option<Subscription>,
    state: Option<ReceiveState>,
    result: Option<ReceiveOutcome>,
    connection: Connection,
    deadline: Option<Instant>,
}
impl Session {
    fn new(connection: Connection) -> Self {
        let subscription = Subscription::new(&connection.stream.0).unwrap();
        let state = ReceiveState::start(&connection);
        Self {
            subscription: Some(subscription),
            state: Some(state),
            result: None,
            connection,
            deadline: None,
        }
    }
    fn pump(&mut self) -> Option<Instant> {
        for _ in 0..10000 {
            match self.state.as_mut().unwrap().step(&self.connection).unwrap() {
                // Drain actual reads to WouldBlock. This is not a source handoff.
                Progress::Again => {}
                Progress::Wait(until) => {
                    if let Some(original) = self.deadline {
                        assert_eq!(until, original);
                    }
                    self.deadline = Some(until);
                    self.subscription.as_mut().unwrap().arm().unwrap();
                    return Some(until);
                }
                Progress::Complete(value) => {
                    drop(self.subscription.take());
                    drop(self.state.take());
                    self.connection
                        .stream
                        .0
                        .trace
                        .borrow_mut()
                        .push("state_drop");
                    self.result = Some(value);
                    return None;
                }
            }
        }
        panic!("bounded receiver failed to wait or complete");
    }
    fn notice(&self) -> Notice {
        let s = self.subscription.as_ref().unwrap();
        s.socket
            .backend
            .wait_notice(s.token, self.deadline.unwrap())
            .expect("expected readiness before fixed deadline")
    }
    fn accept(&self, notice: &Notice) {
        assert!(self.connection.stream.0.backend.claim(notice));
        assert!(
            !self.connection.stream.0.backend.claim(notice),
            "duplicate notification"
        );
    }
    // A readiness hint may be spurious or a TCP write may arrive in multiple
    // reads. Retry through the same deadline until the expected bytes arrive.
    fn pump_through(&mut self, bytes: usize) -> Option<Instant> {
        loop {
            let progress = self.pump();
            if progress.is_none() || self.connection.stream.0.bytes.get() >= bytes {
                return progress;
            }
            let n = self.notice();
            self.accept(&n);
        }
    }
    fn send(&mut self, peer: &mut TcpStream, bytes: &[u8]) -> Option<Instant> {
        let target = self.connection.stream.0.bytes.get() + bytes.len();
        peer.write_all(bytes).unwrap();
        let n = self.notice();
        self.accept(&n);
        self.pump_through(target)
    }
    fn take(&mut self) -> ReceiveOutcome {
        self.result.take().expect("one owned result")
    }
    fn duplicate(&self) {
        let socket = &self.connection.stream.0;
        let before = (
            socket.reads.get(),
            socket.bytes.get(),
            self.connection.head.get(),
        );
        let mut other = ReceiveState::start(&self.connection);
        let Progress::Complete(ReceiveOutcome::failed(message)) =
            other.step(&self.connection).unwrap()
        else {
            panic!("duplicate receive did not fail")
        };
        assert_eq!(message, "request reception already started");
        assert!(other.step(&self.connection).is_err());
        assert_eq!(
            (
                socket.reads.get(),
                socket.bytes.get(),
                self.connection.head.get()
            ),
            before
        );
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        drop(self.subscription.take());
        if let Some(state) = self.state.take() {
            drop(state);
            self.connection
                .stream
                .0
                .trace
                .borrow_mut()
                .push("state_drop");
        }
        if let Some(result) = self.result.take() {
            drop(result);
            self.connection
                .stream
                .0
                .trace
                .borrow_mut()
                .push("candidate_drop");
        }
    }
}
fn retired(trace: &Trace, candidate: bool) {
    let mut expected = vec!["register", "unregister", "state_drop"];
    if candidate {
        expected.push("candidate_drop");
    }
    expected.push("socket_drop");
    assert_eq!(*trace.borrow(), expected);
}
const HEAD: &[u8] = b"HEAD /live HTTP/1.1\r\nHost: local\r\nContent-Length: 6\r\n\r\n";
const GET: &[u8] = b"GET /live HTTP/1.1\r\nHost: local\r\n\r\n";

fn fragmented() {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    let original = s.pump().unwrap();
    s.duplicate();
    for bytes in [
        &HEAD[..3],
        &HEAD[3..],
        b"a\xc3".as_slice(),
        b"\xa9bc".as_slice(),
    ] {
        assert_eq!(s.send(&mut peer, bytes), Some(original));
        s.duplicate();
    }
    assert!(s.send(&mut peer, b"d").is_none());
    let ReceiveOutcome::request(method, target, headers, body) = s.take() else {
        panic!("expected request")
    };
    assert_eq!(
        (method.as_str(), target.as_str(), body.as_str()),
        ("HEAD", "/live", "aébcd")
    );
    assert_eq!(headers.len(), 2);
    assert_eq!(headers["host"], "local");
    assert_eq!(headers["content-length"], "6");
    assert!(s.connection.head.get());
    assert_eq!(s.connection.stream.0.bytes.get(), HEAD.len() + 6);
    assert!(s.connection.stream.0.blocked.get() >= 5);
    s.duplicate();
    drop(s);
    retired(&trace, false);
    b.finished();
}
fn eof(bytes: &[u8], expected: &str) {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    s.pump().unwrap();
    peer.write_all(bytes).unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    loop {
        let n = s.notice();
        s.accept(&n);
        if s.pump().is_none() {
            break;
        }
    }
    match (s.take(), expected) {
        (ReceiveOutcome::disconnected, "disconnected") => {}
        (ReceiveOutcome::malformed(m), expected) => assert_eq!(m, expected),
        _ => panic!("unexpected EOF outcome"),
    }
    drop(s);
    retired(&trace, false);
    b.finished();
}
fn fixed_deadline() {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    let start = Instant::now();
    let until = s.pump().unwrap();
    assert!(until <= start + IO_TIMEOUT);
    for bytes in [HEAD, b"a", b"b"] {
        // Real timeout wakeups without data cannot reset the operation's budget.
        let pause = Instant::now() + Duration::from_millis(40);
        while Instant::now() < pause {
            for n in b.poll(pause) {
                if b.claim(&n) {
                    assert_eq!(s.pump(), Some(until));
                }
            }
        }
        assert_eq!(s.send(&mut peer, bytes), Some(until));
        s.duplicate();
    }
    while Instant::now() < until {
        let token = s.subscription.as_ref().unwrap().token;
        if let Some(n) = b.wait_notice(token, until) {
            s.accept(&n);
            if s.pump().is_none() {
                break;
            }
        }
    }
    assert!(Instant::now() >= until);
    if s.result.is_none() {
        assert!(s.pump().is_none());
    }
    assert!(matches!(s.take(), ReceiveOutcome::timed_out));
    drop(s);
    retired(&trace, false);
    b.finished();
}
fn stop(signalled: bool) {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    s.pump().unwrap();
    let token = s.subscription.as_ref().unwrap().token;
    let late = if signalled {
        peer.write_all(GET).unwrap();
        let n = s.notice();
        s.accept(&n);
        n
    } else {
        Notice {
            identity: b.identity.clone(),
            token,
        }
    };
    drop(s);
    retired(&trace, false);
    assert!(!b.claim(&late));
    b.finished();
}
fn ready_candidate() {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    s.pump().unwrap();
    peer.write_all(GET).unwrap();
    let late = s.notice();
    s.accept(&late);
    assert!(s.pump_through(GET.len()).is_none());
    assert!(matches!(&s.result, Some(ReceiveOutcome::request(..))));
    assert!(!b.claim(&late));
    drop(s);
    retired(&trace, true);
    b.finished();
}
fn spurious_and_stale() {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    let until = s.pump().unwrap();
    // A current-generation hint with no data must lead to WouldBlock and rearm.
    let synthetic = Notice {
        identity: b.identity.clone(),
        token: s.subscription.as_ref().unwrap().token,
    };
    s.accept(&synthetic);
    assert_eq!(s.pump(), Some(until));
    assert!(!b.claim(&synthetic));
    peer.write_all(&GET[..4]).unwrap();
    let real = s.notice();
    s.accept(&real);
    assert_eq!(s.pump_through(4), Some(until));
    assert!(!b.claim(&real));
    peer.write_all(&GET[4..]).unwrap();
    let current = s.notice();
    assert!(!b.claim(&real));
    s.accept(&current);
    assert!(s.pump_through(GET.len()).is_none());
    assert!(matches!(s.take(), ReceiveOutcome::request(..)));
    drop(s);
    retired(&trace, false);
    b.finished();
}
fn foreign() {
    let a = Backend::new();
    let b = Backend::new();
    let (ca, mut pa, ta) = pair(&a);
    let (cb, mut pb, tb) = pair(&b);
    let mut sa = Session::new(ca);
    let mut sb = Session::new(cb);
    sa.pump().unwrap();
    sb.pump().unwrap();
    pa.write_all(GET).unwrap();
    pb.write_all(GET).unwrap();
    let na = sa.notice();
    let nb = sb.notice();
    assert_eq!(na.token, nb.token);
    assert!(!a.claim(&nb));
    assert!(!b.claim(&na));
    sa.accept(&na);
    sb.accept(&nb);
    assert!(sa.pump_through(GET.len()).is_none());
    assert!(sb.pump_through(GET.len()).is_none());
    drop(sa);
    drop(sb);
    retired(&ta, true);
    retired(&tb, true);
    a.finished();
    b.finished();
}
fn generation_exhaustion() {
    let b = Backend::new();
    let (c, _peer, trace) = pair(&b);
    let mut s = Session::new(c);
    s.pump().unwrap();
    let old = s.subscription.as_ref().unwrap().token;
    let saved = b.next.replace(usize::MAX);
    assert!(s.subscription.as_mut().unwrap().arm().is_err());
    assert_eq!(s.subscription.as_ref().unwrap().token, old);
    assert!(b.entries.borrow().contains_key(&old));
    b.next.set(saved);
    drop(s);
    retired(&trace, false);
    b.finished();
}
#[cfg(unix)]
fn descriptor_reuse() {
    use std::os::fd::AsRawFd;
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    let old_fd = s.connection.stream.0.io.borrow().as_raw_fd();
    s.pump().unwrap();
    peer.write_all(GET).unwrap();
    let late = s.notice();
    drop(s);
    drop(peer);
    retired(&trace, false);
    let mut reused = false;
    for _ in 0..64 {
        let (c, mut peer, trace) = pair(&b);
        let fd = c.stream.0.io.borrow().as_raw_fd();
        let mut current = Session::new(c);
        current.pump().unwrap();
        assert!(!b.claim(&late));
        if fd == old_fd {
            reused = true;
            peer.write_all(GET).unwrap();
            let n = current.notice();
            assert_ne!(n.token, late.token);
            assert!(!b.claim(&late));
            current.accept(&n);
            assert!(current.pump_through(GET.len()).is_none());
            assert!(matches!(current.take(), ReceiveOutcome::request(..)));
        }
        drop(current);
        retired(&trace, false);
        if reused {
            break;
        }
    }
    assert!(reused, "could not exercise actual OS descriptor reuse");
    b.finished();
}

pub fn run() {
    fragmented();
    eof(b"", "disconnected");
    eof(b"HEAD /", "incomplete headers");
    let mut partial = HEAD.to_vec();
    partial.extend_from_slice(b"ab");
    eof(&partial, "incomplete body");
    fixed_deadline();
    stop(false);
    stop(true);
    ready_candidate();
    spurious_and_stale();
    foreign();
    generation_exhaustion();
    descriptor_reuse();
    println!(
        "live receive cases: 12; real descriptor reuse, fixed deadline, registration retirement verified"
    );
}
