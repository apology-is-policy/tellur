//! Private deterministic adapter pump at the source scheduler boundary. The
//! fixture's awaited leaf is an adapter shim, not admission for synchronous
//! primitives to suspend. Production source/manifest behavior stays unchanged.
use super::*;
use std::collections::VecDeque;

#[derive(Default)]
pub(in crate::runtime::continuation) struct SourceOperations {
    trace: Trace,
    waiting: VecDeque<(TaskId, Readiness)>,
    retired_wakes: Vec<Readiness>,
    polls: BTreeMap<TaskId, usize>,
    reverse: bool,
    early: bool,
    early_sent: bool,
    idle_pumps: usize,
    peak_waiting: usize,
    checked_contingencies: usize,
}
impl Machine<'_> {
    /// Nonblocking signals append eligibility; they do not select a context.
    /// Idle pumping happens only after the source runnable queue is exhausted.
    pub(in crate::runtime::continuation) fn pump_source_operations(
        &mut self,
        idle: bool,
    ) -> Result<bool> {
        let Some(mut model) = self.shared.scheduler.operations.take() else {
            return Ok(false);
        };
        let mut ready = vec![];
        let mut remaining = VecDeque::new();
        while let Some((task, token)) = model.waiting.pop_front() {
            if self.shared.waiting.contains_key(&task) {
                remaining.push_back((task, token));
            } else {
                assert!(self.signal_readiness(&token).is_err());
                model.retired_wakes.push(token);
            }
        }
        model.waiting = remaining;
        let early = model.early && !model.early_sent && model.waiting.len() >= 2;
        if idle || early {
            if idle {
                model.idle_pumps += 1;
            }
            if early {
                model.early_sent = true;
            }
            // An early batch can become ready behind an already-runnable faulting
            // sibling. An idle pump admits one signal per deterministic event.
            let count = if early {
                model.waiting.len()
            } else {
                1.min(model.waiting.len())
            };
            for _ in 0..count {
                ready.push(if model.reverse {
                    model.waiting.pop_back().unwrap()
                } else {
                    model.waiting.pop_front().unwrap()
                });
            }
        }
        let before = (
            self.shared.steps_left,
            self.shared.events.clone(),
            self.shared.values.clone(),
            self.shared.cursor,
        );
        for (task, token) in &ready {
            self.signal_readiness(token)?;
            assert!(self.signal_readiness(token).is_err());
            self.queue_operation_readiness(*task);
            model.retired_wakes.push(token.clone());
        }
        assert_eq!(
            before,
            (
                self.shared.steps_left,
                self.shared.events.clone(),
                self.shared.values.clone(),
                self.shared.cursor
            )
        );
        let signalled = !ready.is_empty();
        self.shared.scheduler.operations = Some(model);
        Ok(signalled)
    }
    fn park_source_operation(&mut self) -> Result<()> {
        let task = self.context.task.unwrap().0;
        let token = self.park_for_readiness()?;
        let model = self.shared.scheduler.operations.as_mut().unwrap();
        assert!(!model.waiting.iter().any(|(id, _)| *id == task));
        model.waiting.push_back((task, token));
        model.peak_waiting = model.peak_waiting.max(self.shared.waiting.len());
        self.select_source()
    }
    fn drive_source_operations(&mut self, mut quota: usize) -> bool {
        if quota == 0 {
            return self.drive(0);
        }
        for _ in 0..2000 {
            if self.context.flow.is_none() && at_sample(self) && self.shared.steps_left > 0 {
                assert!(
                    self.context.task.is_some(),
                    "adapter shim must have an admitted async invocation"
                );
                let trace = self
                    .shared
                    .scheduler
                    .operations
                    .as_ref()
                    .unwrap()
                    .trace
                    .clone();
                prepare(self, &trace);
                quota -= 1;
                if let Err(diagnostic) = self.park_source_operation() {
                    self.context.flow = Some(Err(diagnostic));
                }
            } else if self.context.flow.is_none() && pending(self).is_ok() {
                let task = self.context.task.unwrap().0;
                let model = self.shared.scheduler.operations.as_mut().unwrap();
                let phase = model.polls.entry(task).or_default();
                let chunk = ["A", "", "B"][*phase];
                *phase += 1;
                let done = *phase == 3;
                let before = (
                    self.shared.steps_left,
                    self.shared.events.clone(),
                    self.shared.cursor,
                );
                poll(self, chunk).unwrap();
                assert_eq!(
                    before,
                    (
                        self.shared.steps_left,
                        self.shared.events.clone(),
                        self.shared.cursor
                    )
                );
                if done {
                    assert_eq!(pending(self).unwrap().partial, "AB");
                    complete(self).unwrap();
                } else if let Err(diagnostic) = self.park_source_operation() {
                    self.context.flow = Some(Err(diagnostic));
                }
            } else {
                if self.context.flow.is_none()
                    && self.shared.steps_left > 0
                    && matches!(self.context.stack.last(),Some(Continuation::Block {code,next})
                        if code.get(*next).is_some_and(|i| i.guard.evaluate(&self.context.frame.facts)
                            && matches!(&i.action,Action::Emit(Expr::Value(Value::String(s))) if s=="owning contingency")))
                {
                    let model = self.shared.scheduler.operations.as_mut().unwrap();
                    let trace = model.trace.borrow();
                    for request in ["left", "right"] {
                        assert_eq!(
                            trace
                                .iter()
                                .filter(|(name, kind)| name == request && kind == "retired")
                                .count(),
                            1,
                            "all stopped operations must retire before the first owning handler effect"
                        );
                    }
                    assert!(
                        !trace
                            .iter()
                            .any(|(name, kind)| name == "other" && kind == "retired")
                    );
                    model.checked_contingencies += 1;
                }
                let before = self.shared.steps_left;
                if self.drive(1) {
                    return true;
                }
                if self.shared.steps_left < before {
                    quota -= 1;
                }
            }
            if quota == 0 {
                return false;
            }
        }
        panic!("source operation model did not progress")
    }
}

fn source_program() -> CheckedProgram {
    let text = include_str!("../../tests/fixtures/source_pending_operation.tellur");
    // Reuse the exact checked adapter signature from the earlier operation model.
    let manifest = operation_manifest();
    crate::compile(text, manifest).unwrap()
}
fn run(
    p: &CheckedProgram,
    stop: bool,
    fuel: usize,
    quotas: &[usize],
    reverse: bool,
    early: bool,
) -> (RunResult, Trace, usize) {
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(stop)],
        calls: vec![],
    };
    let mut session = p.start_execution(&s, fuel);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    let trace = Trace::default();
    m.shared.scheduler.operations = Some(SourceOperations {
        trace: trace.clone(),
        reverse,
        early,
        ..Default::default()
    });
    let mut finished = false;
    for quota in quotas.iter().copied().cycle().take(2000) {
        if m.drive_source_operations(quota) {
            finished = true;
            break;
        }
    }
    assert!(finished, "fuel={fuel}, stop={stop}");
    m.prune_source();
    // Prune stopped registrations and prove every saved generation is now stale.
    m.pump_source_operations(false).unwrap();
    let model = m.shared.scheduler.operations.take().unwrap();
    assert!(model.waiting.is_empty());
    for token in model.retired_wakes {
        assert!(m.signal_readiness(&token).is_err());
    }
    settlement_tests::retired(m);
    m.assert_source_retired();
    let peak = model.peak_waiting;
    if fuel == 1000 {
        assert!(peak >= 2);
        assert_eq!(model.checked_contingencies, usize::from(stop));
        assert!(
            model.idle_pumps > 0,
            "independent pending operation must remain resumable"
        );
        let messages = settlement_tests::messages(m);
        let first = messages
            .iter()
            .position(|s| s == "uninterrupted first")
            .unwrap();
        assert_eq!(messages[first + 1], "uninterrupted second");
        assert!(!messages.iter().any(|s| s.contains("nested contingency")));
        assert!(messages.iter().any(|s| s == "independent joined"));
        for request in ["left", "right", "other"] {
            let events = trace.borrow();
            let count = |kind: &str| {
                events
                    .iter()
                    .filter(|(n, k)| n == request && k == kind)
                    .count()
            };
            assert_eq!(count("prepared"), 1);
            if stop && request != "other" {
                assert_eq!(count("retired"), 1);
                assert_eq!(count("completed"), 0);
                assert_eq!(
                    count("polled"),
                    0,
                    "fault runs before eligible stopped operations"
                );
            } else {
                assert_eq!(count("completed"), 1);
                assert_eq!(count("polled"), 3);
                assert_eq!(count("retired"), 0);
            }
        }
        assert_eq!(
            messages
                .iter()
                .filter(|s| *s == "owning contingency")
                .count(),
            usize::from(stop)
        );
        if stop {
            assert!(
                !messages
                    .iter()
                    .any(|s| s == "left joined" || s == "all joined")
            );
        }
    }
    (session.machine.take().unwrap().finish("main"), trace, peak)
}
fn observation(result: RunResult) -> serde_json::Value {
    match result {
        Ok(result) => serde_json::to_value(result).unwrap(),
        Err(failure) => {
            serde_json::json!({"report":failure.report,"events":failure.events,"diagnostic":format!("{:?}",failure.diagnostic)})
        }
    }
}
#[test]
fn source_pending_operations_wait_without_runnable_work_and_keep_explicit_turns() {
    let p = source_program();
    for stop in [false, true] {
        for reverse in [false, true] {
            for early in [false, true] {
                let expected = observation(run(&p, stop, 1000, &[1], reverse, early).0);
                assert!(expected.get("outcome").is_some(), "{expected}");
                for quotas in [&[0, 2, 0, 7][..], &[3, 1, 9][..]] {
                    assert_eq!(
                        observation(run(&p, stop, 1000, quotas, reverse, early).0),
                        expected
                    );
                }
            }
        }
    }
}
#[test]
fn source_pending_operations_preserve_fuel_and_quota_behavior_after_expansion() {
    let p = source_program();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    // Expansion intentionally records its call-site annotations in reports.
    // Compare exact reports across quotas within each representation, retaining
    // those annotations. Expansion changes instruction/fuel boundaries, so
    // compare completed source effects and dispositions across forms.
    for stop in [false, true] {
        for reverse in [false, true] {
            for early in [false, true] {
                let mut original_final = None;
                for candidate in [&p, &expanded] {
                    let mut finished = false;
                    for fuel in 0..200 {
                        let expected =
                            observation(run(candidate, stop, fuel, &[1], reverse, early).0);
                        for quotas in [&[0, 2, 0, 7][..], &[3, 1, 9][..]] {
                            assert_eq!(
                                observation(run(candidate, stop, fuel, quotas, reverse, early).0),
                                expected,
                                "fuel={fuel},stop={stop},reverse={reverse},early={early}"
                            );
                        }
                        if expected.get("outcome").is_some() {
                            let mut effects = expected;
                            for event in effects["events"].as_array_mut().unwrap() {
                                if event["kind"] == "fault" {
                                    *event = serde_json::json!({"kind":"fault","message":event["report"]["message"],"phase":event["report"]["phase"]});
                                }
                            }
                            if let Some(original) = &original_final {
                                assert_eq!(&effects, original);
                            } else {
                                original_final = Some(effects);
                            }
                            finished = true;
                            break;
                        }
                    }
                    assert!(finished);
                }
            }
        }
    }
}
