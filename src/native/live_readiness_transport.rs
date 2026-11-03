// Private loopback resource used by the source-scheduler differential fixture.
use std::{
    io::{self, Read, Write},
    time::{Duration, Instant},
};
struct Wire {
    // The registered source is destroyed before its poller.
    server: mio::net::TcpStream,
    client: std::net::TcpStream,
    poll: mio::Poll,
}
pub struct LiveTransport {
    wire: RefCell<Option<Wire>>,
    registered: Cell<bool>,
    phase: Cell<usize>,
    id: usize,
    trace: Rc<RefCell<Vec<String>>>,
}
impl LiveTransport {
    pub fn new(id: usize, trace: Rc<RefCell<Vec<String>>>) -> Rc<Self> {
        Rc::new(Self {
            wire: RefCell::new(None),
            registered: Cell::new(false),
            phase: Cell::new(0),
            id,
            trace,
        })
    }
    pub fn arm(&self, generation: u64) -> Result<(), String> {
        let token = usize::try_from(generation).map_err(|_| "E_MODEL: token overflow")?;
        assert!(self.registered.get());
        let mut wire = self.wire.borrow_mut();
        let wire = wire.as_mut().unwrap();
        wire.poll
            .registry()
            .reregister(&mut wire.server, mio::Token(token), mio::Interest::READABLE)
            .map_err(|e| e.to_string())
    }
    pub fn offer(&self, generation: u64) -> Result<(), String> {
        assert!(self.registered.get());
        let phase = self.phase.get();
        assert!(phase < 3);
        self.phase.set(phase + 1);
        if phase == 1 {
            // Deliberately empty hint, to exercise WouldBlock without inventing data.
            self.trace
                .borrow_mut()
                .push(format!("empty hint {}", self.id));
            return Ok(());
        }
        let bytes = if phase == 0 { b"A" } else { b"B" };
        let mut wire = self.wire.borrow_mut();
        let wire = wire.as_mut().unwrap();
        wire.client.write_all(bytes).map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(2);
        let mut events = mio::Events::with_capacity(8);
        let mut observed = false;
        loop {
            assert!(
                Instant::now() < until,
                "fixture peer/readiness barrier timed out"
            );
            match wire.poll.poll(
                &mut events,
                Some(until.saturating_duration_since(Instant::now())),
            ) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.to_string()),
            }
            observed |= events.iter().any(|e| e.token().0 == generation as usize);
            // Test-only peer barrier: make the scripted byte observable before
            // admitting its event. This peek consumes no bytes, and does not run
            // adapter/parser code. Real unscripted ordering is not being claimed.
            let mut peek = [0; 1];
            match wire.server.peek(&mut peek) {
                Ok(1) if observed => {
                    assert_eq!(&peek, bytes);
                    break;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Ok(_) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        self.trace
            .borrow_mut()
            .push(format!("os notice {} {generation}", self.id));
        Ok(())
    }
    pub fn drain(&self) -> Vec<u8> {
        assert!(self.registered.get());
        let wire = self.wire.borrow();
        let wire = wire.as_ref().unwrap();
        let mut result = vec![];
        let mut buffer = [0; 8];
        loop {
            self.trace.borrow_mut().push(format!("io read {}", self.id));
            match (&wire.server).read(&mut buffer) {
                Ok(0) => panic!("fixture peer closed prematurely"),
                Ok(n) => result.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("fixture read: {e}"),
            }
        }
        assert!(result.len() <= 1);
        result
    }
    pub fn require_unregistered(&self) {
        assert!(!self.registered.get());
    }
}
impl Drop for LiveTransport {
    fn drop(&mut self) {
        self.require_unregistered();
        if self.wire.get_mut().take().is_some() {
            self.trace
                .borrow_mut()
                .push(format!("socket drop {}", self.id));
        }
    }
}
pub struct LiveRegistration {
    pub transport: Rc<LiveTransport>,
    request: String,
    alive: Rc<Cell<bool>>,
}
impl LiveRegistration {
    pub fn new(resource: &Probe, request: &str) -> Result<Self, String> {
        let transport = &resource.transport;
        assert!(resource.alive.get());
        assert!(!transport.registered.get());
        assert!(transport.wire.borrow().is_none());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let client = std::net::TcpStream::connect_timeout(
            &listener.local_addr().map_err(|e| e.to_string())?,
            Duration::from_secs(2),
        )
        .map_err(|e| e.to_string())?;
        client.set_nodelay(true).map_err(|e| e.to_string())?;
        client
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        let (server, _) = listener.accept().map_err(|e| e.to_string())?;
        server.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut server = mio::net::TcpStream::from_std(server);
        let poll = mio::Poll::new().map_err(|e| e.to_string())?;
        poll.registry()
            .register(&mut server, mio::Token(0), mio::Interest::READABLE)
            .map_err(|e| e.to_string())?;
        *transport.wire.borrow_mut() = Some(Wire {
            server,
            client,
            poll,
        });
        transport.registered.set(true);
        resource
            .trace
            .borrow_mut()
            .push(format!("register {request}"));
        Ok(Self {
            transport: transport.clone(),
            request: request.into(),
            alive: resource.alive.clone(),
        })
    }
}
impl Drop for LiveRegistration {
    fn drop(&mut self) {
        assert!(self.alive.get());
        assert!(self.transport.registered.replace(false));
        let mut wire = self.transport.wire.borrow_mut();
        let wire = wire.as_mut().unwrap();
        // This private fixture fails on OS removal errors. It does not prescribe
        // a public policy for fallible registration cleanup during owner faults.
        wire.poll
            .registry()
            .deregister(&mut wire.server)
            .expect("fixture deregister");
        self.transport
            .trace
            .borrow_mut()
            .push(format!("unregister {}", self.request));
    }
}
pub fn live_read_count() -> usize {
    TRACE.with(|t| {
        t.borrow()
            .iter()
            .filter(|s| s.starts_with("io read "))
            .count()
    })
}
