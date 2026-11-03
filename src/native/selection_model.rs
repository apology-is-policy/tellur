// Test-only adversarial selections over the generated production callbacks.
fn selection_child(_: &mut Host, _: &mut Frames, _: &mut Activation) -> R<Step> {
    Ok(Step::Complete(Completion::Outcome(("done".into(), vec![]))))
}
static SELECTION_CHILD: Control = Control {
    name: "selection_probe",
    slots: 0,
    atoms: 0,
    parameters: &[],
    retained: &[],
    constructor: false,
    resume: selection_child,
};
fn selection_snapshot(c: &Contexts<'_>, root: &ContextKey) -> String {
    let p = &c.parked[&root.id];
    let frames: Vec<_> = p
        .machine
        .stack
        .iter()
        .map(|a| {
            let pending = match &a.pending {
                None => "none".into(),
                Some(Pending::Call(_)) => "call".into(),
                Some(Pending::Supervise(_)) => "supervise".into(),
                Some(Pending::Await(p)) => format!(
                    "await:{:?}:{}:{}",
                    p.child,
                    p.queued.is_some(),
                    p.result.is_some()
                ),
            };
            (a.owner, a.base(), a.pc, &a.facts, a.scopes.len(), pending)
        })
        .collect();
    format!(
        "{:?}",
        (
            &*c.values,
            &c.host.tasks,
            &p.host.tracker,
            frames,
            c.host.fuel,
            &c.host.events,
            &c.host.resources.scopes,
            c.domains.len(),
            c.domains.member_count()
        )
    )
}
pub fn drive_selection(
    host: &mut Host,
    control: &'static Control,
    args: Vec<V>,
    probes: bool,
    quotas: &[usize],
) -> R<Completion> {
    let mut c = Contexts::new(host);
    let root = c.start_entry(control, args)?;
    let mut chain = vec![root.clone()];
    let mut checked = 0;
    let mut nested = false;
    let mut delivery = false;
    let mut quotas = quotas.iter().cycle();
    let result = loop {
        let selected = chain.last().unwrap().clone();
        if probes && selected.id == root.id {
            let m = &c.parked[&root.id].machine;
            nested |= m.stack.len() > 1;
            delivery |= matches!(&m.stack.last().unwrap().pending, Some(Pending::Await(p)) if p.result.is_some());
            let borrower = c.start_child(&root, &SELECTION_CHILD, vec![])?;
            let before = selection_snapshot(&c, &root);
            for quota in [1, 0, 99] {
                let p = c.parked.get_mut(&root.id).unwrap();
                c.host.exchange_context(&mut p.host)?;
                let error = p
                    .machine
                    .advance(c.host, &mut c.values, &mut c.domains, quota)
                    .err()
                    .expect("selection must reject");
                assert!(error.contains("retained borrower children"));
                c.host.exchange_context(&mut p.host)?;
                assert_eq!(selection_snapshot(&c, &root), before);
            }
            assert!(matches!(
                c.advance_retained(&borrower, 1)?,
                ContextPoll::Ready
            ));
            let (_, fields) = c.join(Some(&root), &borrower)?.outcome()?;
            assert!(fields.is_empty());
            checked += 1;
        }
        match c.advance_retained(&selected, *quotas.next().unwrap())? {
            ContextPoll::Paused => {}
            ContextPoll::Await => chain.push(c.admit_await(&selected)?),
            ContextPoll::Ready if selected.id == root.id => break c.join(None, &root)?,
            ContextPoll::Ready => {
                chain.pop();
                c.return_await(chain.last().unwrap(), &selected)?;
            }
            _ => panic!("normal selection fixture must complete"),
        }
    };
    assert!(!probes || (checked > 5 && nested && delivery));
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.values.is_empty());
    assert!(c.host.tasks.is_empty() && c.waits.is_empty() && c.entry.is_none());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    Ok(result)
}
