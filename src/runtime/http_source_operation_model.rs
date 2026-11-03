//! Private trusted HTTP completions on the existing cooperative source queue.
//! Readiness counts are explicit test schedules, not an HTTP parser or timer policy.
use super::*;

#[derive(Clone)]
pub(crate) struct HttpResponse {
    pub waits: usize,
    pub outcome: String,
    pub fields: Vec<Value>,
}
type Response = HttpResponse;
#[derive(Default)]
pub(super) struct HttpOperations {
    pub(super) responses: BTreeMap<String, VecDeque<Response>>,
    pub(super) names: BTreeMap<String, String>,
}
impl Machine<'_> {
    pub(super) fn poll_http_source_operation(&mut self) -> Result<()> {
        let op = pending(self)?;
        assert_eq!(op.operation, "http_receive");
        assert_eq!(op.arguments.len(), 1);
        assert_eq!(
            self.shared
                .scheduler
                .operations
                .as_ref()
                .unwrap()
                .http_request(&op.borrowed_text(self)),
            op.request
        );
        op.observe("polled");
        let Some(Continuation::AdapterOperation(op)) = self.context.stack.last_mut() else {
            unreachable!()
        };
        let (response, left) = op.http.as_mut().unwrap();
        if *left > 0 {
            *left -= 1;
            return self.park_source_operation();
        }
        let response = response.clone();
        // Validation must keep the selected operation and its supplier alive on
        // every rejection. Include malformed nested collection payloads.
        let before = (
            self.shared.steps_left,
            self.shared.events.clone(),
            self.shared.values.clone(),
            self.shared.cursor,
        );
        let slots = self.context.stack.len();
        let map_type = self
            .shared
            .program
            .manifest
            .primitives
            .iter()
            .find(|p| p.name == "http_receive")
            .unwrap()
            .outcomes
            .iter()
            .find(|o| o.name == "request")
            .unwrap()
            .fields[2]
            .ty
            .clone();
        for (label, fields) in [
            ("unknown", vec![]),
            ("request", vec![Value::String("missing fields".into())]),
            ("malformed", vec![Value::U32(7)]),
            (
                "request",
                vec![
                    Value::String("GET".into()),
                    Value::String("/".into()),
                    Value::Construct {
                        ty: map_type,
                        name: "map".into(),
                        fields: vec![Value::String("host".into()), Value::U32(7)],
                    },
                    Value::String("".into()),
                ],
            ),
        ] {
            assert!(deliver(self, label, fields).is_err());
            assert_eq!(self.context.stack.len(), slots);
            assert_eq!(pending(self)?.operation, "http_receive");
            assert_eq!(
                before,
                (
                    self.shared.steps_left,
                    self.shared.events.clone(),
                    self.shared.values.clone(),
                    self.shared.cursor
                )
            );
        }
        deliver(self, &response.outcome, response.fields)?;
        assert!(pending(self).is_err(), "result adopted only once");
        Ok(())
    }
}
fn http_program() -> CheckedProgram {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("../../examples/http_hello/manifest.json")).unwrap();
    let mut receive = original["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "http_receive")
        .unwrap()
        .clone();
    receive.as_object_mut().unwrap().remove("rust");
    let manifest=Manifest::from_json(&serde_json::json!({
        "version":"0.1","types":[{"name":"http_connection","kind":"opaque","linear":true,"fault":"release"}],
        "rust":{"source":concat!(env!("CARGO_MANIFEST_DIR"),"/library/host/loopback_http.rs"),"lockfile":concat!(env!("CARGO_MANIFEST_DIR"),"/examples/http_hello/Cargo.lock"),"resources":{"http_connection":"Connection"},"dependencies":original["rust"]["dependencies"]},
        "primitives":[receive,{"name":"close_test","parameters":[{"name":"connection","type":"http_connection","mode":"consume"}],"outcomes":[{"name":"done","fields":[]}],"effects":["close"],"returns":true,"contingent":true}]
    }).to_string()).unwrap();
    crate::compile(
        include_str!("../../tests/fixtures/source_pending_http.tellur"),
        manifest,
    )
    .unwrap()
}
const ALTERNATIVES: [&str; 9] = [
    "request",
    "malformed",
    "headers_too_large",
    "body_too_large",
    "unsupported_transfer",
    "unsupported_expectation",
    "timed_out",
    "disconnected",
    "failed",
];
fn response(p: &CheckedProgram, label: &str, name: &str, waits: usize) -> Response {
    let fields = match label {
        "request" => {
            let primitive = p
                .manifest
                .primitives
                .iter()
                .find(|p| p.name == "http_receive")
                .unwrap();
            let shape = primitive
                .outcomes
                .iter()
                .find(|o| o.name == "request")
                .unwrap();
            vec![
                Value::String("HEAD".into()),
                Value::String(format!("/{name}")),
                Value::Construct {
                    ty: shape.fields[2].ty.clone(),
                    name: "map".into(),
                    fields: vec![
                        Value::String("host".into()),
                        Value::String("local".into()),
                        Value::String("x-name".into()),
                        Value::String(name.into()),
                    ],
                },
                Value::String(format!("{name}:🙂")),
            ]
        }
        "malformed" | "failed" => vec![Value::String(format!("{name}: {label}"))],
        _ => vec![],
    };
    Response {
        waits,
        outcome: label.into(),
        fields,
    }
}
fn run_http(
    p: &CheckedProgram,
    label: &str,
    instant: bool,
    stop: bool,
    fuel: usize,
    quotas: &[usize],
    reverse: bool,
    early: bool,
) -> RunResult {
    assert!(quotas.iter().any(|q| *q > 0));
    let responses = BTreeMap::from([
        (
            "left".into(),
            response(p, label, "left", if instant { 0 } else { 2 }),
        ),
        ("right".into(), response(p, "failed", "right", 4)),
        ("other".into(), response(p, "request", "other", 3)),
    ]);
    let calls = (0..if stop { 1 + usize::from(instant) } else { 3 })
        .map(|_| ScriptCall {
            operation: "close_test".into(),
            outcome: "done".into(),
            fields: vec![],
        })
        .collect();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: std::iter::once(Value::Bool(stop))
            .chain(
                ["other", "left", "right"]
                    .into_iter()
                    .map(|token| Value::Opaque {
                        ty: "http_connection".into(),
                        token: token.into(),
                    }),
            )
            .collect(),
        calls,
    };
    http_source_operation_model(
        p,
        &scenario,
        fuel,
        quotas,
        reverse,
        early,
        responses,
        BTreeMap::new(),
    )
}
/// Explicit identities and independently supplied completion/wait plans for native comparison.
pub(crate) fn http_source_operation_model(
    p: &CheckedProgram,
    scenario: &Scenario,
    fuel: usize,
    quotas: &[usize],
    reverse: bool,
    early: bool,
    responses: BTreeMap<String, HttpResponse>,
    names: BTreeMap<String, String>,
) -> RunResult {
    http_source_operation_sequence_model(
        p,
        scenario,
        fuel,
        quotas,
        reverse,
        early,
        responses
            .into_iter()
            .map(|(name, response)| (name, vec![response]))
            .collect(),
        names,
    )
}
/// The plan queue belongs to each supplier; active progress belongs to the
/// prepared continuation, so a later operation cannot inherit an old countdown.
pub(crate) fn http_source_operation_sequence_model(
    p: &CheckedProgram,
    scenario: &Scenario,
    fuel: usize,
    quotas: &[usize],
    reverse: bool,
    early: bool,
    plans: BTreeMap<String, Vec<HttpResponse>>,
    names: BTreeMap<String, String>,
) -> RunResult {
    let repeated = plans.values().any(|responses| responses.len() > 1);
    let responses = plans
        .iter()
        .map(|(name, responses)| (name.clone(), responses[0].clone()))
        .collect::<BTreeMap<_, _>>();
    let Value::Bool(stop) = scenario.arguments[0] else {
        panic!("missing stop argument")
    };
    let instant = responses["left"].waits == 0;
    let label = responses["left"].outcome.clone();
    let token = |name: &str| {
        names
            .iter()
            .find(|(_, label)| label.as_str() == name)
            .map(|(token, _)| token.clone())
            .unwrap_or_else(|| name.into())
    };
    assert!(quotas.iter().any(|q| *q > 0));
    let mut session = p.start_execution(&scenario, fuel);
    let m = session.machine.as_mut().unwrap();
    m.enter(&scenario).unwrap();
    let trace = Trace::default();
    m.shared.scheduler.operations = Some(SourceOperations {
        trace: trace.clone(),
        reverse,
        early,
        http: Some(HttpOperations {
            responses: plans
                .clone()
                .into_iter()
                .map(|(n, p)| (n, p.into()))
                .collect(),
            names: names.clone(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut finished = false;
    for quota in quotas.iter().copied().cycle().take(10000) {
        if quota == 0 {
            let before = (
                m.shared.steps_left,
                m.shared.events.clone(),
                m.shared.cursor,
                trace.borrow().clone(),
            );
            finished = m.drive_source_operations(0);
            assert_eq!(
                before,
                (
                    m.shared.steps_left,
                    m.shared.events.clone(),
                    m.shared.cursor,
                    trace.borrow().clone()
                )
            );
        } else {
            finished = m.drive_source_operations(quota);
        }
        if finished {
            break;
        }
    }
    assert!(finished, "HTTP model did not progress");
    m.prune_source();
    m.pump_source_operations(false).unwrap();
    let model = m.shared.scheduler.operations.take().unwrap();
    assert!(model.waiting.is_empty());
    for wake in model.retired_wakes {
        assert!(m.signal_readiness(&wake).is_err());
    }
    settlement_tests::retired(m);
    m.assert_source_retired();
    if fuel == 1000 && !repeated {
        assert!(model.peak_waiting >= 2);
        assert!(model.idle_pumps > 0);
        assert_eq!(model.checked_contingencies, usize::from(stop));
        let messages = settlement_tests::messages(m);
        assert!(!messages.iter().any(|s| s.contains("nested contingency")));
        assert!(
            messages.iter().any(|s| s == "independent joined"),
            "{label} instant={instant} stop={stop} events={:?} flow={:?}",
            m.shared.events,
            m.context.flow.as_ref().and_then(|r| r.as_ref().err())
        );
        let index = messages
            .iter()
            .position(|s| s == "uninterrupted first")
            .unwrap();
        assert_eq!(messages[index + 1], "uninterrupted second");
        if instant {
            let ready = m.shared.events.iter().position(|e| matches!(e, Event::Primitive {operation, arguments, ..} if operation == "http_receive" && arguments == &vec![Value::Opaque {ty:"http_connection".into(),token:token("left")}])).unwrap();
            let sibling = m
                .shared
                .events
                .iter()
                .position(|e| matches!(e, Event::Emit {message} if message == "fault child queued"))
                .unwrap();
            assert!(
                ready < sibling,
                "immediately ready operation must stay selected"
            );
        }
        let t = trace.borrow();
        for name in ["left", "right", "other"] {
            let count = |kind: &str| t.iter().filter(|(n, k)| n == name && k == kind).count();
            let completed = !stop || name == "other" || (instant && name == "left");
            assert_eq!(count("prepared"), 1);
            assert_eq!(count("completed"), usize::from(completed));
            assert_eq!(count("retired"), usize::from(!completed));
            if completed {
                assert_eq!(count("polled"), responses[name].waits + 1);
                assert_eq!(
                    count("waiting"),
                    responses[name].waits,
                    "ready HTTP must not acquire a synthetic wait"
                );
                let event=m.shared.events.iter().find(|e|matches!(e,Event::Primitive{operation,arguments,..} if operation=="http_receive" && arguments==&vec![Value::Opaque{ty:"http_connection".into(),token:token(name)}])).unwrap();
                let Event::Primitive {
                    outcome, fields, ..
                } = event
                else {
                    unreachable!()
                };
                assert_eq!(outcome, &responses[name].outcome);
                assert_eq!(fields, &responses[name].fields);
            }
        }
    }
    if fuel == 1000 && repeated {
        let messages = settlement_tests::messages(m);
        assert!(messages.iter().any(|s| s == "independent joined"));
        assert!(!messages.iter().any(|s| s.contains("nested contingency")));
        assert_eq!(model.checked_contingencies, usize::from(stop));
        let t = trace.borrow();
        let count =
            |name: &str, kind: &str| t.iter().filter(|(n, k)| n == name && k == kind).count();
        assert_eq!(count("left", "prepared"), if stop { 1 } else { 2 });
        assert_eq!(count("left", "completed"), if stop { 1 } else { 2 });
        assert_eq!(count("left", "waiting"), responses["left"].waits);
        assert_eq!(
            count("left", "polled"),
            responses["left"].waits + if stop { 1 } else { 2 }
        );
        assert_eq!(count("right", "prepared"), 1);
        assert_eq!(count("right", "retired"), usize::from(stop));
        assert_eq!(count("right", "completed"), usize::from(!stop));
        assert_eq!(count("other", "completed"), 1);
        assert_eq!(count("other", "retired"), 0);
        assert_eq!(
            messages
                .iter()
                .filter(|s| s.as_str() == "first completed")
                .count(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .filter(|s| s.as_str() == "duplicate completed")
                .count(),
            usize::from(!stop)
        );
        if !stop {
            let received = m
                .shared
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Primitive {
                        operation,
                        arguments,
                        outcome,
                        fields,
                        ..
                    } if operation == "http_receive"
                        && arguments
                            == &vec![Value::Opaque {
                                ty: "http_connection".into(),
                                token: token("left"),
                            }] =>
                    {
                        Some((outcome, fields))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(received.len(), 2);
            for (i, (label, fields)) in received.into_iter().enumerate() {
                assert_eq!(label, &plans["left"][i].outcome);
                assert_eq!(fields, &plans["left"][i].fields);
            }
        }
    }
    session.machine.take().unwrap().finish("main")
}
#[test]
fn http_source_queue_preserves_all_nine_typed_outcomes_and_immediate_readiness() {
    let p = http_program();
    for label in ALTERNATIVES {
        for instant in [false, true] {
            for stop in [false, true] {
                for reverse in [false, true] {
                    for early in [false, true] {
                        let expected = observation(run_http(
                            &p,
                            label,
                            instant,
                            stop,
                            1000,
                            &[1],
                            reverse,
                            early,
                        ));
                        assert!(
                            expected.get("outcome").is_some(),
                            "{label} instant={instant} stop={stop}: {expected}"
                        );
                        for quotas in [&[0, 2, 0, 7][..], &[3, 1, 9][..]] {
                            assert_eq!(
                                observation(run_http(
                                    &p, label, instant, stop, 1000, quotas, reverse, early
                                )),
                                expected
                            );
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn http_source_queue_preserves_fuel_faults_and_quota_turns_after_expansion() {
    let p = http_program();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    let mut comparisons = 0;
    for label in ["request", "timed_out", "failed"] {
        for instant in [false, true] {
            for stop in [false, true] {
                for reverse in [false, true] {
                    for early in [false, true] {
                        let mut normal_effects = None;
                        for program in [&p, &expanded] {
                            let mut finished = false;
                            for fuel in 0..250 {
                                let expected = observation(run_http(
                                    program,
                                    label,
                                    instant,
                                    stop,
                                    fuel,
                                    &[1],
                                    reverse,
                                    early,
                                ));
                                for quotas in [&[0, 2, 0, 7][..], &[3, 1, 9][..]] {
                                    assert_eq!(
                                        observation(run_http(
                                            program, label, instant, stop, fuel, quotas, reverse,
                                            early
                                        )),
                                        expected,
                                        "{label}, instant={instant}, stop={stop}, reverse={reverse}, early={early}, fuel={fuel}"
                                    );
                                    comparisons += 1;
                                }
                                if expected.get("outcome").is_some() {
                                    let mut effects = expected;
                                    for event in effects["events"].as_array_mut().unwrap() {
                                        if event["kind"] == "fault" {
                                            *event = serde_json::json!({"kind":"fault","message":event["report"]["message"],"phase":event["report"]["phase"]});
                                        }
                                    }
                                    if let Some(normal) = &normal_effects {
                                        assert_eq!(&effects, normal);
                                    } else {
                                        normal_effects = Some(effects);
                                    }
                                    finished = true;
                                    break;
                                }
                            }
                            assert!(
                                finished,
                                "{label} instant={instant} stop={stop} reverse={reverse} early={early}"
                            );
                        }
                    }
                }
            }
        }
    }
    eprintln!("HTTP source reference comparisons: {comparisons}");
}

#[path = "http_candidate_fault_model.rs"]
mod candidate_fault;
pub(crate) use candidate_fault::http_candidate_fault_model;
