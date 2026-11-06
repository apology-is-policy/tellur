//! Controlled owning fault before HTTP source-arm adoption, not a source handoff.
use super::*;
pub(crate) fn http_candidate_fault_model(
    p: &CheckedProgram,
    fuel: usize,
    mode: u8,
    quotas: &[usize],
    independent: bool,
    responses: BTreeMap<String, HttpResponse>,
) -> RunResult {
    assert!(mode < 4 && quotas.iter().any(|q| *q > 0));
    let mut calls = (0..4)
        .map(|id| ScriptCall {
            operation: "open_case".into(),
            outcome: "opened".into(),
            fields: vec![Value::Opaque {
                ty: "http_connection".into(),
                token: format!("rust:{id}"),
            }],
            waits: 0,
        })
        .collect::<Vec<_>>();
    for operation in ["checkpoint", "close_test"] {
        calls.push(ScriptCall {
            operation: operation.into(),
            outcome: "done".into(),
            fields: vec![],
            waits: 0,
        });
    }
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls,
    };
    let mut session = p.start_execution(&s, 10000);
    let m = session.machine.as_mut().unwrap();
    m.enter(&s).unwrap();
    settlement_tests::until_message(m, "owner ready");
    let domain = m.context.domain.unwrap();
    let owner = std::mem::replace(&mut m.context, Context::new(Default::default()));
    let other = if independent {
        m.call("independent", vec![]).unwrap();
        settlement_tests::until_message(m, "independent ready");
        Some(m.park_for_readiness().unwrap())
    } else {
        None
    };
    let trace = Trace::default();
    m.shared.scheduler.operations = Some(SourceOperations {
        trace: trace.clone(),
        http: Some(HttpOperations {
            responses: responses
                .iter()
                .map(|(n, r)| (n.clone(), vec![r.clone()].into()))
                .collect(),
            names: BTreeMap::from([
                ("rust:1".into(), "left".into()),
                ("rust:3".into(), "right".into()),
            ]),
        }),
        ..Default::default()
    });
    let mut tokens = vec![];
    for (control, request) in [("lexical", "left"), ("named", "right")] {
        let mut child = Context::new(owner.activations.child());
        child.domain = Some(domain);
        m.context = child;
        m.call(control, vec![Value::String(request.into())])
            .unwrap();
        prepare_operation(m, &trace, "http_receive");
        pending(m).unwrap().observe("polled");
        if mode >= 2 {
            let response = &responses[request];
            let args = pending(m).unwrap().arguments.clone();
            let Some(Continuation::AdapterOperation(op)) = m.context.stack.last_mut() else {
                unreachable!()
            };
            op.candidate = Some(response.fields.clone());
            op.observe(if mode == 2 {
                "typed held"
            } else {
                "encoded held"
            });
            if mode == 3 {
                // Match the existing native private finish helper's publication
                // point, but leave the pending operation and its arms unadopted.
                let signature = p
                    .manifest
                    .primitives
                    .iter()
                    .find(|p| p.name == "http_receive")
                    .unwrap();
                m.shared.events.push(Event::Primitive {
                    operation: "http_receive".into(),
                    arguments: args,
                    outcome: response.outcome.clone(),
                    fields: response.fields.clone(),
                    effects: signature.effects.clone(),
                });
            }
        }
        // A host-side retained-context hold, separate from physical I/O interest.
        tokens.push(m.park_for_readiness().unwrap());
    }
    if mode > 0 {
        for token in &tokens {
            m.signal_readiness(token).unwrap();
        }
    }
    m.context = owner;
    m.shared.steps_left = fuel;
    let events = m.shared.events.clone();
    assert!(!m.drive(0));
    assert_eq!(events, m.shared.events);
    assert_eq!(m.shared.steps_left, fuel);
    let mut finished = m.drive(1);
    for quota in quotas.iter().copied().cycle().take(400) {
        if finished {
            break;
        }
        finished = m.drive(quota);
    }
    assert!(finished);
    for token in &tokens {
        assert!(m.signal_readiness(token).is_err());
    }
    assert!(
        !settlement_tests::messages(m)
            .iter()
            .any(|s| s.starts_with("forbidden"))
    );
    assert_eq!(
        m.shared
            .events
            .iter()
            .filter(|e| matches!(e,Event::Primitive{operation,..} if operation=="http_receive"))
            .count(),
        if mode == 3 { 2 } else { 0 }
    );
    for name in ["left", "right"] {
        for kind in ["prepared", "polled", "retired"] {
            assert_eq!(
                trace
                    .borrow()
                    .iter()
                    .filter(|(n, k)| n == name && k == kind)
                    .count(),
                1
            );
        }
    }
    if let Some(other) = other {
        assert!(matches!(m.context.flow, Some(Ok(Flow::Exit(..)))));
        assert_eq!(m.shared.waiting.len(), 1);
        let terminal = std::mem::replace(&mut m.context, Context::new(Default::default()));
        m.signal_readiness(&other).unwrap();
        m.dispatch_ready().unwrap();
        assert!(quotas.iter().copied().cycle().take(400).any(|q| m.drive(q)));
        assert!(matches!(m.context.flow, Some(Ok(Flow::Exit(..)))));
        assert!(m.signal_readiness(&other).is_err());
        m.context = terminal;
    }
    settlement_tests::retired(m);
    session.machine.take().unwrap().finish("main")
}
