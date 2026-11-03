//! Reference counterpart of the private generated owning-fault schedule.
use super::*;
pub(crate) fn pending_fault_model(
    p: &CheckedProgram,
    fuel: usize,
    mode: u8,
    quotas: &[usize],
    independent: bool,
) -> RunResult {
    assert!(quotas.iter().any(|q| *q > 0));
    let mut calls: Vec<_> = (0..4)
        .map(|id| ScriptCall {
            operation: "make".into(),
            outcome: "made".into(),
            fields: vec![Value::Opaque {
                ty: "probe".into(),
                token: format!("rust:{id}"),
            }],
        })
        .collect();
    for operation in ["checkpoint", "close"] {
        calls.push(ScriptCall {
            operation: operation.into(),
            outcome: "done".into(),
            fields: vec![],
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
    let mut tokens = vec![];
    for (control, request) in [("lexical", "left"), ("named", "right")] {
        let mut child = Context::new(owner.activations.child());
        child.domain = Some(domain);
        m.context = child;
        m.call(control, vec![Value::String(request.into())])
            .unwrap();
        prepare(m, &trace);
        poll(m, "partial").unwrap();
        if mode == 2 {
            let Some(Continuation::AdapterOperation(op)) = m.context.stack.last_mut() else {
                unreachable!()
            };
            op.candidate = Some(vec![Value::String("discarded candidate".into())]);
        }
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
    assert_eq!(m.shared.events, events);
    assert_eq!(m.shared.steps_left, fuel);
    let mut finished = m.drive(1); // Exactly the owner's already-checked fault statement.
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
    assert!(!m.shared.events.iter().any(|e|matches!(e,Event::Primitive{operation,..} if operation=="sample" || operation=="inspect")));
    assert!(
        !settlement_tests::messages(m)
            .iter()
            .any(|s| s.starts_with("forbidden"))
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
