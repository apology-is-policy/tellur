//! Entry handoff returns the host's original tracker, including raw terminal errors.
use super::*;
fn host(fuel: usize) -> Host {
    let mut h = Host::new(serde_json::json!({}), None, true, Some(fuel));
    h.activations = fault::Tracker::new(
        (1..=3)
            .map(|id| {
                (
                    id,
                    fault::Site {
                        control: format!("site_{id}"),
                        path: Some("entry.tellur".into()),
                        line: id,
                        column: 4,
                        label: Some("selected".into()),
                        expansion_calls: vec!["inline-origin".into()],
                        omitted_expansion_calls: 0,
                    },
                )
            })
            .collect(),
    );
    h.enter("ambient_host").unwrap();
    h.at(1);
    h
}
fn deliver(a: &mut Activation, v: &mut Frames, result: Completion) -> R<()> {
    let (_, mut fields) = result.outcome()?;
    v[a.base()] = fields.pop();
    Ok(())
}
fn root(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.at(2);
    h.tick(2, 4)?;
    if a.pc == 0 {
        a.pc = 1;
        let mut args = vec![v[a.base()].take().unwrap()];
        return if std::ptr::eq(a.control, &ROOT) {
            let mut cold = Some(a.defer_task(h, &LEAF, "pending", &mut args)?);
            Ok(Step::Await {
                task: a.queue_task(h, &LEAF, &mut cold)?,
                deliver,
            })
        } else {
            Ok(Step::Call {
                control: &LEAF,
                arguments: args,
                destination: None,
                deliver,
            })
        };
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
fn leaf(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    h.at(3);
    h.tick(3, 4)?;
    let Some(V::String(message)) = v[a.base()].take() else {
        panic!()
    };
    if !message.is_empty() {
        return h.fault(V::String(message));
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![V::U32(42)],
    ))))
}
static ROOT: Control = Control {
    name: "root",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: root,
};
static DIRECT: Control = Control { ..ROOT };
static LEAF: Control = Control {
    name: "leaf",
    resume: leaf,
    ..ROOT
};
fn empty(c: &Contexts<'_>) {
    assert!(
        c.entry.is_none() && c.parked.is_empty() && c.completed.is_empty() && c.terminal.is_empty()
    );
    assert!(c.values.is_empty() && c.host.tasks.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
}
fn baseline(message: &str, fuel: usize) -> (fault::Report, Vec<serde_json::Value>, Option<usize>) {
    let mut h = host(fuel);
    let mut v = Frames::default();
    let mut d = Domains::default();
    let mut m = Machine::start(
        &mut h,
        &mut v,
        &mut d,
        &DIRECT,
        vec![V::String(message.into())],
    )
    .unwrap();
    let (phase, error) = loop {
        match m.advance(&mut h, &mut v, &mut d, 1) {
            Ok(Poll::Paused) => {}
            Ok(Poll::Complete(result)) => {
                assert_eq!(result.outcome().unwrap().1, [V::U32(42)]);
                break (fault::Phase::ResultValidation, "validation probe".into());
            }
            Err(error) => break (fault::Phase::Execution, error),
            _ => panic!(),
        }
    };
    assert!(v.is_empty() && h.tasks.is_empty());
    (
        h.activations.failure("root", phase, &error),
        h.events.clone(),
        h.fuel,
    )
}
#[test]
fn entry_handoff_preserves_ambient_chain_sites_raw_errors_and_truncation() {
    for message in ["".into(), "nested failure".into(), "λ".repeat(5000)] {
        for fuel in 0..=4 {
            let expected = baseline(&message, fuel);
            let mut h = host(fuel);
            let mut c = Contexts::new(&mut h);
            let root = c
                .start_entry(&ROOT, vec![V::String(message.clone())])
                .unwrap();
            let mut selected = root.clone();
            let (phase, error) = loop {
                match c.advance_retained(&selected, 1).unwrap() {
                    ContextPoll::Paused => {}
                    ContextPoll::Await => selected = c.admit_await(&selected).unwrap(),
                    ContextPoll::Ready if selected.id != root.id => {
                        c.return_await(&root, &selected).unwrap();
                        selected = root.clone();
                    }
                    ContextPoll::Ready => {
                        assert_eq!(
                            c.join(None, &root).unwrap().outcome().unwrap().1,
                            [V::U32(42)]
                        );
                        break (fault::Phase::ResultValidation, "validation probe".into());
                    }
                    ContextPoll::Failed(_) if selected.id != root.id => selected = root.clone(),
                    ContextPoll::Failed(report) => {
                        let error = c.entry_error.take().unwrap();
                        if report.message_truncated {
                            assert!(error.chars().count() > 4096);
                        }
                        break (fault::Phase::Execution, error);
                    }
                    _ => panic!(),
                }
            };
            empty(&c);
            assert_eq!(
                (
                    c.host.activations.failure("root", phase, &error),
                    c.host.events.clone(),
                    c.host.fuel
                ),
                expected,
                "fuel={fuel}"
            );
        }
    }
}
#[test]
fn rejected_entry_admission_preserves_the_selected_host_chain() {
    let mut h = host(100);
    let original = format!("{:?}", h.activations);
    let mut c = Contexts::new(&mut h);
    assert!(c.start_entry(&ROOT, vec![]).is_err());
    assert_eq!(format!("{:?}", c.host.activations), original);
    empty(&c);
    let key = c.start_entry(&ROOT, vec![V::String("".into())]).unwrap();
    let tracker = format!("{:?}", c.parked[&key.id].host.tracker);
    assert!(c.start_entry(&ROOT, vec![V::String("".into())]).is_err());
    assert_eq!(format!("{:?}", c.parked[&key.id].host.tracker), tracker);
    // Termination is administrative; it cannot resume the rejected second root.
    c.terminate().unwrap();
    empty(&c);
}

#[test]
fn production_entry_driver_matches_direct_reports_and_returns_without_test_quotas() {
    for message in ["".into(), "nested failure".into(), "λ".repeat(5000)] {
        for fuel in 0..=4 {
            let expected = baseline(&message, fuel);
            let mut h = host(fuel);
            let (phase, error) =
                match super::super::run_entry(&mut h, vec![V::String(message.clone())], &ROOT) {
                    Ok(result) => {
                        assert_eq!(result.outcome().unwrap().1, [V::U32(42)]);
                        (fault::Phase::ResultValidation, "validation probe".into())
                    }
                    Err(error) => (fault::Phase::Execution, error),
                };
            assert!(h.tasks.is_empty() && h.resources.scopes.is_empty());
            assert_eq!(
                (
                    h.activations.failure("root", phase, &error),
                    h.events.clone(),
                    h.fuel
                ),
                expected,
                "fuel={fuel}"
            );
        }
    }
}
