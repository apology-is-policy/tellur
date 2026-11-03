// Explicit test scheduling; host quotas never select a source sibling.
fn progress_borrower(_: &mut Host, _: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
static PROGRESS_BORROWER: Control = Control {
    name: "progress_borrower",
    slots: 1,
    atoms: 0,
    parameters: &[0],
    retained: &[],
    constructor: false,
    access: None,
    resume: progress_borrower,
};
pub fn drive_cold_progress(
    host: &mut Host,
    control: &'static Control,
    args: Vec<V>,
    state: usize,
    quotas: &[usize],
) -> R<Completion> {
    let mut c = Contexts::new(host);
    let root = c.start_entry(control, args)?;
    loop {
        let before = c.parked[&root.id].machine.next_access().transition;
        assert!(matches!(c.advance_retained(&root, 1)?, ContextPoll::Paused));
        if before == Transition::Defer {
            break;
        }
    }
    let a = c.parked[&root.id].machine.stack.last().unwrap();
    let args = c
        .values
        .frame_values(&a.frame)?
        .iter()
        .find_map(|v| match v {
            Some(V::Task { arguments, .. }) => Some(arguments.clone()),
            _ => None,
        })
        .expect("source must retain a cold handle");
    assert_eq!(args.len(), 1);
    let sibling = c.start_child(&root, &PROGRESS_BORROWER, args)?;
    assert!(matches!(
        c.dependencies(&sibling)?,
        Dependencies::Complete { .. }
    ));
    let ready = if state == 1 {
        c.advance_retained(&sibling, 1)?;
        Some(c.park_for_readiness(&sibling)?)
    } else {
        None
    };
    if state == 2 {
        assert!(matches!(
            c.advance_retained(&sibling, 2)?,
            ContextPoll::Ready
        ));
    }
    let mut quotas = quotas.iter().cycle();
    let mut local = 0;
    loop {
        let quota = *quotas.next().unwrap();
        let before_events = c.host.events.clone();
        let p = c.advance_disjoint(&root, quota);
        assert_eq!(
            c.host.events, before_events,
            "preparation must not execute child effects"
        );
        local += p.instructions;
        match p.stop {
            AdmissionStop::Quota | AdmissionStop::TransitionLimit => {}
            AdmissionStop::Poll(ContextPoll::Await) => break,
            _ => panic!("owner progress must reach its explicit cold await"),
        }
    }
    assert!(
        local >= 3,
        "local source work and the consuming await must run"
    );
    let child = c.admit_await(&root)?;
    loop {
        match c.advance_retained(&child, *quotas.next().unwrap())? {
            ContextPoll::Paused => {}
            ContextPoll::Ready => break,
            _ => panic!("child must complete normally"),
        }
    }
    c.return_await(&root, &child)?;
    let fuel = c.host.fuel;
    assert!(matches!(
        c.advance_disjoint_one(&root)?,
        ContextPoll::Paused
    ));
    assert_eq!(
        c.host.fuel, fuel,
        "administrative delivery consumes no source instruction"
    );
    assert!(
        matches!(
            c.advance_disjoint(&root, 99).stop,
            AdmissionStop::Blocked(_)
        ),
        "source effects remain gated beside the sibling"
    );
    if let Some(ready) = ready {
        c.signal_readiness(&ready)?;
    }
    if state != 2 {
        assert!(matches!(
            c.advance_retained(&sibling, 2)?,
            ContextPoll::Ready
        ));
    }
    c.join(Some(&root), &sibling)?;
    let result = loop {
        match c.advance_retained(&root, *quotas.next().unwrap())? {
            ContextPoll::Paused => {}
            ContextPoll::Ready => break c.join(None, &root)?,
            _ => panic!("parent must complete normally"),
        }
    };
    assert!(
        c.values.is_empty()
            && c.parked.is_empty()
            && c.completed.is_empty()
            && c.host.tasks.is_empty()
    );
    assert!(c.waits.is_empty() && c.terminal.is_empty());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    Ok(result)
}
