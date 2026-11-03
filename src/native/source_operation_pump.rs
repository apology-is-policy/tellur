// Included only in the private generated operation model, alongside its ticket API.
pub struct SourcePump {
    tickets: BTreeMap<u64, Ticket>,
    waiting: std::collections::VecDeque<(ContextKey, Readiness)>,
    eligible: std::collections::BTreeSet<u64>,
    old: Vec<Readiness>,
    polls: BTreeMap<u64, usize>,
    early: bool,
    early_sent: bool,
    reverse: bool,
    idle_pumps: usize,
    peak_waiting: usize,
}
impl SourcePump {
    pub fn new() -> Self {
        Self {
            tickets: BTreeMap::new(),
            waiting: Default::default(),
            eligible: Default::default(),
            old: vec![],
            polls: BTreeMap::new(),
            early: std::env::var("TELLUR_SOURCE_EARLY").unwrap() == "true",
            early_sent: false,
            reverse: std::env::var("TELLUR_SOURCE_REVERSE").unwrap() == "true",
            idle_pumps: 0,
            peak_waiting: 0,
        }
    }
    fn park(&mut self, c: &mut Contexts<'_>, key: &ContextKey) -> R<()> {
        let token = c.park_for_readiness(key)?;
        assert!(!self.waiting.iter().any(|(k, _)| k.id == key.id));
        self.waiting.push_back((key.clone(), token));
        self.peak_waiting = self.peak_waiting.max(c.waits.len());
        Ok(())
    }
    pub fn park_new(&mut self, c: &mut Contexts<'_>, key: &ContextKey) -> R<bool> {
        let pending = c
            .parked
            .get(&key.id)
            .and_then(|p| p.machine.stack.last())
            .is_some_and(|a| matches!(&a.pending,Some(Pending::Adapter(op)) if !op.ready()));
        if !pending || self.tickets.contains_key(&key.id) {
            return Ok(false);
        }
        self.tickets.insert(key.id, ticket(c, key));
        self.park(c, key)?;
        Ok(true)
    }
    pub fn signal(&mut self, c: &mut Contexts<'_>, idle: bool) -> R<Vec<ContextKey>> {
        let mut live = std::collections::VecDeque::new();
        while let Some((key, token)) = self.waiting.pop_front() {
            if c.waits
                .get(&key.id)
                .is_some_and(|w| w.generation == token.generation)
            {
                live.push_back((key, token));
            } else {
                assert!(c.signal_readiness(&token).is_err());
                self.old.push(token);
            }
        }
        self.waiting = live;
        let early = self.early && !self.early_sent && self.waiting.len() >= 2;
        let mut ready = vec![];
        let before = (c.host.fuel, c.host.events.clone(), c.host.cursor);
        if idle || early {
            if idle {
                self.idle_pumps += 1;
            }
            if early {
                self.early_sent = true;
            }
            let count = if early {
                self.waiting.len()
            } else {
                1.min(self.waiting.len())
            };
            for _ in 0..count {
                let (key, token) = if self.reverse {
                    self.waiting.pop_back().unwrap()
                } else {
                    self.waiting.pop_front().unwrap()
                };
                c.signal_readiness(&token)?;
                assert!(c.signal_readiness(&token).is_err());
                assert!(self.eligible.insert(key.id));
                ready.push(key);
                self.old.push(token);
            }
        }
        assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
        Ok(ready)
    }
    /// Return true when incomplete polling re-parked the selected context.
    pub fn poll_ready(&mut self, c: &mut Contexts<'_>, key: &ContextKey) -> R<bool> {
        if !self.eligible.remove(&key.id) {
            return Ok(false);
        }
        let t = self.tickets[&key.id].clone();
        if operation(c, &t).is_err() {
            return Ok(false);
        } // Stopped owner may be restored here.
        let before = (c.host.fuel, c.host.events.clone(), c.host.cursor);
        assert!(matches!(c.advance_retained(key, 1)?, ContextPoll::Paused));
        let phase = self.polls.entry(key.id).or_default();
        let chunk = ["A", "", "B"][*phase];
        *phase += 1;
        operation(c, &t)?.poll(chunk)?;
        assert_eq!(before, (c.host.fuel, c.host.events.clone(), c.host.cursor));
        if *phase < 3 {
            self.park(c, key)?;
            return Ok(true);
        }
        let op = operation(c, &t)?;
        assert_eq!(op.partial, "AB");
        assert_eq!(op.deadline, 57);
        let args = vec![op.resource.clone(), V::String(op.lease.request.clone())];
        let (label, fields) = c.host.primitive("sample", args)?;
        operation(c, &t)?.complete(&label, fields)?;
        assert!(
            operation(c, &t)?
                .complete("ready", vec![V::String("duplicate".into())])
                .is_err()
        );
        Ok(false)
    }
    pub fn finish(&mut self, c: &mut Contexts<'_>) -> R<()> {
        assert!(self.signal(c, false)?.is_empty());
        assert!(self.waiting.is_empty());
        for token in &self.old {
            assert!(c.signal_readiness(token).is_err());
        }
        for t in self.tickets.values() {
            assert!(operation(c, t).is_err());
        }
        assert!(c.host.resources.values.is_empty());
        std::fs::write(
            std::env::var("TELLUR_SOURCE_EVENTS").unwrap(),
            serde_json::to_vec(&c.host.events).unwrap(),
        )
        .unwrap();
        std::fs::write(
            std::env::var("TELLUR_SOURCE_STATS").unwrap(),
            format!("{} {}", self.idle_pumps, self.peak_waiting),
        )
        .unwrap();
        self.tickets.clear();
        self.eligible.clear();
        self.old.clear();
        self.polls.clear();
        Ok(())
    }
}
