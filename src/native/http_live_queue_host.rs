// Private controlled peer transport. ReceiveState below is included unchanged.
use crate::rust_api;
use rust_api::http_receive_outcome as ReceiveOutcome;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{self, Read, Write},
    rc::Rc,
    time::{Duration, Instant},
};
type Trace = Rc<RefCell<Vec<String>>>;
thread_local! {
    static TRACE:Trace=Default::default();
    static READS:Cell<usize>=const {Cell::new(0)};
    static NEXT:Cell<usize>=const {Cell::new(1)};
    static WAITS:RefCell<BTreeMap<(u64,u64),String>>=Default::default();
}
pub fn read_count() -> usize {
    READS.with(Cell::get)
}
pub fn retire_wait(context: u64, generation: u64) {
    if let Some(name) = WAITS.with(|w| w.borrow_mut().remove(&(context, generation))) {
        TRACE.with(|t| {
            t.borrow_mut()
                .push(format!("logical retire {name} {context} {generation}"))
        });
    }
}
pub struct TraceGuard;
impl Drop for TraceGuard {
    fn drop(&mut self) {
        assert!(WAITS.with(|w| w.borrow().is_empty()));
        if let Ok(path) = std::env::var("TELLUR_OPERATION_LOG") {
            TRACE.with(|t| std::fs::write(path, t.borrow().join("\n")).unwrap());
        }
    }
}
struct Wire {
    server: mio::net::TcpStream,
    client: std::net::TcpStream,
    poll: mio::Poll,
}
struct Socket {
    wire: RefCell<Wire>,
    registered: Cell<bool>,
    reads: Cell<usize>,
    bytes: Cell<usize>,
    clock: Cell<Instant>,
    chunks: RefCell<std::collections::VecDeque<Vec<u8>>>,
    timeout: bool,
    name: String,
    trace: Trace,
}
impl Socket {
    fn offer(&self, chunk: &[u8], token: Option<usize>, until: Instant) -> io::Result<()> {
        let mut wire = self.wire.borrow_mut();
        wire.client.write_all(chunk)?;
        let mut events = mio::Events::with_capacity(8);
        let mut noticed = token.is_none();
        loop {
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "controlled peer barrier",
                ));
            }
            if let Some(token) = token {
                match wire.poll.poll(
                    &mut events,
                    Some(until.saturating_duration_since(Instant::now())),
                ) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
                noticed |= events.iter().any(|e| e.token().0 == token);
            }
            // A non-consuming test barrier establishes the scripted byte visibility.
            // It never calls the parser and is not a production scheduling policy.
            let mut peek = vec![0; chunk.len()];
            match wire.server.peek(&mut peek) {
                Ok(n) if n == chunk.len() && noticed => {
                    assert_eq!(peek, chunk);
                    return Ok(());
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e),
            }
            std::thread::yield_now();
        }
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        assert!(!self.registered.get());
        self.trace
            .borrow_mut()
            .push(format!("socket drop {}", self.name));
    }
}
struct Stream(Rc<Socket>);
impl Stream {
    fn now(&self) -> Instant {
        let now = Instant::now();
        self.0.clock.set(now);
        now
    }
    fn try_read(&self, out: &mut [u8]) -> io::Result<usize> {
        let s = &self.0;
        s.reads.set(s.reads.get() + 1);
        READS.with(|r| r.set(r.get() + 1));
        let result = (&s.wire.borrow().server).read(out);
        if let Ok(n) = result {
            s.bytes.set(s.bytes.get() + n);
        }
        if result
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::WouldBlock)
        {
            s.trace.borrow_mut().push(format!("would block {}", s.name));
        }
        result
    }
}
pub struct Connection {
    stream: Stream,
    receive_started: Cell<bool>,
    head: Cell<bool>,
    pub alive: Rc<Cell<bool>>,
    pub trace: Trace,
    pub name: String,
}
impl Connection {
    pub fn observed(&self) -> (usize, usize, Instant) {
        let s = &self.stream.0;
        (s.reads.get(), s.bytes.get(), s.clock.get())
    }
    pub fn wake(&self, _until: Instant) {
        panic!("simulated wake used by live source driver")
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        assert!(self.alive.replace(false));
        assert!(!self.stream.0.registered.get());
        self.trace.borrow_mut().push(format!(
            "resource drop {} head={} bytes={}",
            self.name,
            self.head.get(),
            self.stream.0.bytes.get()
        ));
    }
}
#[derive(Clone)]
pub struct Notice {
    backend: Rc<()>,
    token: usize,
}
struct Registration {
    socket: Rc<Socket>,
    backend: Rc<()>,
    token: usize,
    delivered: bool,
    wait: Option<(u64, u64)>,
}
impl Registration {
    fn new(socket: &Rc<Socket>) -> io::Result<Self> {
        assert!(!socket.registered.get());
        let token = NEXT.with(|n| {
            let token = n.get();
            n.set(token.checked_add(1).expect("backend generation exhausted"));
            token
        });
        let mut wire = socket.wire.borrow_mut();
        let Wire { server, poll, .. } = &mut *wire;
        poll.registry()
            .register(server, mio::Token(token), mio::Interest::READABLE)?;
        socket.registered.set(true);
        socket
            .trace
            .borrow_mut()
            .push(format!("register {}", socket.name));
        Ok(Self {
            socket: socket.clone(),
            backend: Rc::new(()),
            token,
            delivered: false,
            wait: None,
        })
    }
    fn arm(&mut self, context: u64, generation: u64) -> io::Result<Notice> {
        if let Some(wait) = self.wait {
            assert!(!WAITS.with(|w| w.borrow().contains_key(&wait)));
        }
        let old = Notice {
            backend: self.backend.clone(),
            token: self.token,
        };
        let token = NEXT.with(|n| {
            let t = n.get();
            n.set(t.checked_add(1).expect("backend generation exhausted"));
            t
        });
        let mut wire = self.socket.wire.borrow_mut();
        let Wire { server, poll, .. } = &mut *wire;
        poll.registry()
            .reregister(server, mio::Token(token), mio::Interest::READABLE)?;
        drop(wire);
        self.token = token;
        self.delivered = false;
        self.wait = Some((context, generation));
        assert!(WAITS.with(|w| {
            w.borrow_mut()
                .insert((context, generation), self.socket.name.clone())
                .is_none()
        }));
        assert!(!self.claim(&old));
        self.socket.trace.borrow_mut().push(format!(
            "logical arm {} {context} {generation}",
            self.socket.name
        ));
        Ok(Notice {
            backend: self.backend.clone(),
            token,
        })
    }
    fn claim(&mut self, notice: &Notice) -> bool {
        if !Rc::ptr_eq(&self.backend, &notice.backend)
            || self.token != notice.token
            || self.delivered
        {
            return false;
        }
        self.delivered = true;
        true
    }
    fn hint(&mut self, notice: &Notice, until: Instant) -> io::Result<()> {
        assert!(
            self.wait
                .is_some_and(|key| WAITS.with(|w| w.borrow().contains_key(&key)))
        );
        assert!(!self.claim(&Notice {
            backend: Rc::new(()),
            token: notice.token
        }));
        let chunk = self
            .socket
            .chunks
            .borrow_mut()
            .pop_front()
            .expect("unexpected additional parser wait");
        if self.socket.timeout && self.socket.chunks.borrow().is_empty() {
            let mut wire = self.socket.wire.borrow_mut();
            let mut events = mio::Events::with_capacity(8);
            while Instant::now() < until {
                match wire.poll.poll(
                    &mut events,
                    Some(until.saturating_duration_since(Instant::now())),
                ) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            self.socket
                .trace
                .borrow_mut()
                .push(format!("real deadline {}", self.socket.name));
        } else if chunk.is_empty() {
            self.socket
                .trace
                .borrow_mut()
                .push(format!("empty hint {}", self.socket.name));
        } else {
            self.socket.offer(&chunk, Some(self.token), until)?;
            self.socket
                .trace
                .borrow_mut()
                .push(format!("os notice {} {}", self.socket.name, self.token));
        }
        assert!(self.claim(notice));
        assert!(!self.claim(notice));
        Ok(())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(wait) = self.wait {
            assert!(
                !WAITS.with(|w| w.borrow().contains_key(&wait)),
                "physical registration removed before logical wait"
            );
        }
        let mut wire = self.socket.wire.borrow_mut();
        let Wire { server, poll, .. } = &mut *wire;
        poll.registry()
            .deregister(server)
            .expect("private fixture deregister");
        assert!(self.socket.registered.replace(false));
        self.socket
            .trace
            .borrow_mut()
            .push(format!("unregister {}", self.socket.name));
    }
}
pub struct OwnedReceive {
    registration: Option<Registration>,
    state: Option<operation::ReceiveState>,
    alive: Rc<Cell<bool>>,
    trace: Trace,
    name: String,
    until: Option<Instant>,
    duplicate: bool,
}
impl OwnedReceive {
    pub fn start(c: &Connection) -> Self {
        let duplicate = c.receive_started.get();
        let state = operation::ReceiveState::start(c);
        let until = if duplicate {
            None
        } else {
            Some(c.stream.0.clock.get() + IO_TIMEOUT)
        };
        c.trace.borrow_mut().push(format!("state start {}", c.name));
        Self {
            registration: None,
            state: Some(state),
            alive: c.alive.clone(),
            trace: c.trace.clone(),
            name: c.name.clone(),
            until,
            duplicate,
        }
    }
    pub fn poll(&mut self, c: &Connection) -> Result<operation::Progress, String> {
        assert!(self.alive.get());
        assert!(Rc::ptr_eq(&self.alive, &c.alive));
        let before = c.observed();
        let progress = self.state.as_mut().ok_or("state consumed")?.step(c)?;
        if self.duplicate {
            assert_eq!(before, c.observed());
            assert!(
                matches!(&progress,operation::Progress::Complete(ReceiveOutcome::failed(message)) if message=="request reception already started")
            );
            self.trace
                .borrow_mut()
                .push(format!("duplicate no read {}", self.name));
        }
        match &progress {
            operation::Progress::Wait(until) => {
                assert_eq!(Some(*until), self.until);
                self.trace
                    .borrow_mut()
                    .push(format!("wait original deadline {}", self.name));
            }
            operation::Progress::Complete(_) => {
                drop(self.registration.take());
                self.state.take();
                self.trace
                    .borrow_mut()
                    .push(format!("state consumed {}", self.name));
            }
            operation::Progress::Again => {}
        }
        Ok(progress)
    }
    pub fn arm(&mut self, c: &Connection, context: u64, generation: u64) -> Result<Notice, String> {
        assert!(Rc::ptr_eq(&self.alive, &c.alive));
        if self.registration.is_none() {
            self.registration = Some(Registration::new(&c.stream.0).map_err(|e| e.to_string())?);
        }
        self.registration
            .as_mut()
            .unwrap()
            .arm(context, generation)
            .map_err(|e| e.to_string())
    }
    pub fn hint(&mut self, notice: &Notice) -> Result<(), String> {
        self.registration
            .as_mut()
            .unwrap()
            .hint(notice, self.until.unwrap())
            .map_err(|e| e.to_string())
    }
    pub fn reject(&mut self, notice: &Notice) {
        assert!(!self.registration.as_mut().unwrap().claim(notice));
    }
}
impl Drop for OwnedReceive {
    fn drop(&mut self) {
        assert!(self.alive.get());
        drop(self.registration.take());
        self.state.take();
        self.trace
            .borrow_mut()
            .push(format!("state drop {}", self.name));
    }
}
pub fn open_case(name: String) -> rust_api::open_case_outcome {
    let instant = (name == "left" && std::env::var("TELLUR_HTTP_INSTANT").unwrap() == "true")
        || (name != "left" && std::env::var("TELLUR_LIVE_DEADLINE").is_ok_and(|v| v == "true"));
    let timeout = name == "left" && std::env::var("TELLUR_HTTP_CASE").unwrap() == "timed_out";
    assert!(
        !timeout || !instant,
        "real deadline fixture requires a pending receive"
    );
    let body = format!("{name}:🙂");
    let packet=format!("HEAD /{name} HTTP/1.1\r\nHost: local\r\nX-Name: {name}\r\nContent-Length: {}\r\n\r\n{body}",body.len()).into_bytes();
    let waits = match name.as_str() {
        "left" => 2,
        "right" => 4,
        "other" => 7,
        _ => panic!("unknown connection"),
    };
    let mut chunks = vec![vec![]; waits];
    if waits == 2 {
        chunks[0] = packet[..packet.len() - 2].to_vec();
    } else {
        let split = 10;
        chunks[0] = packet[..split].to_vec();
        chunks[2] = packet[split..packet.len() - 2].to_vec();
    }
    chunks[waits - 1] = packet[packet.len() - 2..].to_vec();
    if timeout {
        chunks = vec![b"HEAD / HTTP/1.1\r\nHost:".to_vec(), vec![]];
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client = std::net::TcpStream::connect_timeout(
        &listener.local_addr().unwrap(),
        Duration::from_secs(2),
    )
    .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.set_nodelay(true).unwrap();
    let (server, _) = listener.accept().unwrap();
    server.set_nonblocking(true).unwrap();
    let trace = TRACE.with(Clone::clone);
    let socket = Rc::new(Socket {
        wire: RefCell::new(Wire {
            server: mio::net::TcpStream::from_std(server),
            client,
            poll: mio::Poll::new().unwrap(),
        }),
        registered: Cell::new(false),
        reads: Cell::new(0),
        bytes: Cell::new(0),
        clock: Cell::new(Instant::now()),
        chunks: RefCell::new(chunks.into()),
        timeout,
        name: name.clone(),
        trace: trace.clone(),
    });
    if instant {
        socket
            .offer(&packet, None, Instant::now() + Duration::from_secs(2))
            .unwrap();
        socket.chunks.borrow_mut().clear();
    }
    trace.borrow_mut().push(format!("make {name}"));
    rust_api::open_case_outcome::opened(Connection {
        stream: Stream(socket),
        receive_started: Cell::new(false),
        head: Cell::new(false),
        alive: Rc::new(Cell::new(true)),
        trace,
        name,
    })
}
pub fn checkpoint() -> rust_api::checkpoint_outcome {
    TRACE.with(|t| {
        let mut trace = t.borrow_mut();
        for name in ["left", "right"] {
            assert!(trace.iter().any(|s| s == &format!("state drop {name}")));
            assert!(
                trace
                    .iter()
                    .any(|s| s.starts_with(&format!("resource drop {name} ")))
            );
            assert!(trace.iter().any(|s| s == &format!("socket drop {name}")));
        }
        assert!(!trace.iter().any(|s| s.starts_with("resource drop other ")));
        trace.push("checkpoint".into());
    });
    rust_api::checkpoint_outcome::done
}
pub fn close_test(connection: Connection) -> rust_api::close_test_outcome {
    drop(connection);
    rust_api::close_test_outcome::done
}
pub fn http_receive(connection: &Connection) -> ReceiveOutcome {
    baseline::http_receive(connection)
}
