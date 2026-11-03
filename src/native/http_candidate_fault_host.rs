// Own the actual generated payload, never a copy used only as a drop witness.
pub struct HttpCandidate {
    value: Option<ReceiveOutcome>,
    alive: Rc<Cell<bool>>,
    trace: Trace,
    name: String,
}
impl HttpCandidate {
    fn check(&self) {
        assert!(self.alive.get());
        match self.value.as_ref().unwrap() {
            ReceiveOutcome::request(method, target, headers, body) => {
                assert_eq!(method, "HEAD");
                assert_eq!(target, &format!("/{}", self.name));
                assert_eq!(body, &format!("{}:🙂", self.name));
                assert_eq!(
                    headers,
                    &BTreeMap::from([
                        ("content-length".into(), body.len().to_string()),
                        ("host".into(), "local".into()),
                        ("x-name".into(), self.name.clone())
                    ])
                );
            }
            ReceiveOutcome::malformed(message) => assert_eq!(message, "HTTP/1.1 required"),
            _ => panic!("unexpected typed HTTP candidate"),
        }
    }
    pub fn new(value: ReceiveOutcome, c: &Connection) -> Self {
        assert!(!c.stream.0.registered.get());
        let candidate = Self {
            value: Some(value),
            alive: c.alive.clone(),
            trace: c.trace.clone(),
            name: c.name.clone(),
        };
        candidate.check();
        candidate
            .trace
            .borrow_mut()
            .push(format!("typed candidate {}", candidate.name));
        candidate
    }
    pub fn encode(&mut self) -> Result<ReceiveOutcome, String> {
        if self.value.is_none() {
            return Err("candidate consumed".into());
        }
        self.check();
        self.trace
            .borrow_mut()
            .push(format!("candidate encode {}", self.name));
        Ok(self.value.take().unwrap())
    }
}
impl Drop for HttpCandidate {
    fn drop(&mut self) {
        assert!(self.alive.get());
        if self.value.is_some() {
            self.check();
            self.trace
                .borrow_mut()
                .push(format!("typed candidate drop {}", self.name));
        } else {
            self.trace
                .borrow_mut()
                .push(format!("empty candidate drop {}", self.name));
        }
    }
}
