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
    access: None,
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
                    "await:{:?}:{}:{}:{:?}",
                    p.child,
                    p.queued.is_some(),
                    p.result.is_some(),
                    p.dependencies
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
            &p.dependencies,
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
    expected: &[&str],
    expect_guards: bool,
    expect_complete: bool,
) -> R<Completion> {
    let mut c = Contexts::new(host);
    let root = c.start_entry(control, args)?;
    let mut chain = vec![root.clone()];
    let mut checked = 0;
    let mut nested = false;
    let mut delivery = false;
    let mut access_kinds = std::collections::BTreeSet::new();
    let mut indirect = false;
    let mut destination = false;
    let mut captured = false;
    let mut complete_capture = false;
    let mut complete_delivery = false;
    let mut guarded_capture = false;
    let mut guarded_delivery = false;
    let mut covered = false;
    let mut covered_delivery = false;
    let mut uncovered = false;
    let mut admitted = 0;
    let mut admitted_delivery = false;
    let every_point = quotas == [1];
    let mut quotas = quotas.iter().cycle();
    let result = loop {
        let selected = chain.last().unwrap().clone();
        let quota = *quotas.next().unwrap();
        let mut admitted_this_turn = false;
        if probes {
            let snapshot = selection_snapshot(&c, &selected);
            let m = &c.parked[&selected.id].machine;
            let description = m.next_access();
            let resolved = m.resolve_access(&c.values)?;
            let dependencies = c.dependencies(&selected)?;
            dependencies.require_live(&c.values)?;
            complete_capture |= matches!(dependencies, Dependencies::Complete { reachable, .. } if !reachable.is_empty());
            guarded_capture |= has_checked_guard(dependencies);
            captured |= matches!(dependencies, Dependencies::Partial { reachable: entries, .. } | Dependencies::Complete { reachable: entries, .. } if !entries.is_empty());
            if let Some(Pending::Await(p)) = &m.stack.last().unwrap().pending {
                p.dependencies.require_live(&c.values)?;
                complete_delivery |=
                    p.result.is_some() && matches!(p.dependencies, Dependencies::Complete { .. });
                guarded_delivery |= p.result.is_some() && has_checked_guard(&p.dependencies);
            }
            assert_ne!(description.transition, Transition::Unknown);
            assert_eq!(resolved.transition, description.transition);
            assert_eq!(resolved.opaque, description.opaque);
            indirect |= description.accesses.iter().any(|a| a.indirect);
            destination |= description.accesses.iter().any(|a| a.slot.is_none());
            for a in resolved.accesses.iter().chain(&resolved.delivery) {
                c.values.require_storage(&a.place.storage)?;
            }
            for _ in 0..3 {
                assert_eq!(m.next_access(), description);
                assert_eq!(m.resolve_access(&c.values)?, resolved);
                assert_eq!(c.dependencies(&selected)?, dependencies);
            }
            assert_eq!(selection_snapshot(&c, &selected), snapshot);
            access_kinds.insert(format!("{:?}", description.transition));
        }
        if probes && selected.id == root.id {
            let m = &c.parked[&root.id].machine;
            nested |= m.stack.len() > 1;
            delivery |= matches!(&m.stack.last().unwrap().pending, Some(Pending::Await(p)) if p.result.is_some());
            let borrower = c.start_child(&root, &SELECTION_CHILD, vec![])?;
            let before = selection_snapshot(&c, &root);
            let interference = c.child_interference(&root, &borrower);
            let coverage = c.borrower_interference(&root);
            for _ in 0..3 {
                assert_eq!(c.child_interference(&root, &borrower), interference);
                assert_eq!(c.borrower_interference(&root), coverage);
                assert_eq!(selection_snapshot(&c, &root), before);
            }
            // A fresh empty child has complete empty dependencies. Supported
            // local callbacks can be disjoint. Queries alone grant no progress;
            // only the explicit one-transition path below performs admission.
            let description = c.parked[&root.id].machine.next_access();
            if !description.opaque
                && matches!(
                    description.transition,
                    Transition::Skip | Transition::Local | Transition::Delivery
                )
                && description.delivery.is_empty()
            {
                assert_eq!(interference, dependencies::Interference::Disjoint);
            } else {
                assert!(matches!(
                    interference,
                    dependencies::Interference::Inconclusive(_)
                ));
            }
            match &coverage {
                BorrowerInterference::Disjoint {
                    contexts,
                    handoffs,
                    deliveries,
                }
                | BorrowerInterference::Overlap {
                    contexts,
                    handoffs,
                    deliveries,
                    ..
                } => {
                    covered = true;
                    covered_delivery |= *deliveries != 0;
                    assert_eq!((*contexts, *handoffs), (1, 0));
                    let pending = matches!(&c.parked[&root.id].machine.stack.last().unwrap().pending,
                        Some(Pending::Await(p)) if p.result.is_some());
                    assert_eq!(*deliveries, usize::from(pending));
                }
                BorrowerInterference::Inconclusive(_) => {
                    uncovered = true;
                }
            }
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
            let progress = c.advance_disjoint(&root, quota);
            assert!(progress.instructions <= quota);
            assert!(matches!(
                progress.stop,
                AdmissionStop::Quota | AdmissionStop::TransitionLimit | AdmissionStop::Blocked(_)
            ));
            if quota != 0 && matches!(coverage, BorrowerInterference::Disjoint { .. }) {
                assert!(progress.transitions > 0);
                admitted += progress.transitions;
                admitted_delivery |= matches!(coverage,
                    BorrowerInterference::Disjoint { deliveries, .. } if deliveries != 0);
                admitted_this_turn = true;
            } else {
                assert_eq!((progress.instructions, progress.transitions), (0, 0));
                assert!(if quota == 0 {
                    matches!(progress.stop, AdmissionStop::Quota)
                } else {
                    matches!(progress.stop, AdmissionStop::Blocked(_))
                });
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
        if admitted_this_turn {
            continue;
        }
        match c.advance_retained(&selected, quota)? {
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
    if probes && every_point {
        assert!(
            admitted > 0,
            "real compiled callbacks must advance beside a retained child"
        );
        if expect_complete {
            assert!(
                admitted_delivery,
                "complete pending delivery must advance exactly once"
            );
        }
        assert!(
            covered && uncovered,
            "fixtures must exercise complete and inconclusive aggregate queries"
        );
        if expect_complete {
            assert!(
                covered_delivery,
                "complete pending delivery must enter aggregate coverage"
            );
        }
        if expect_complete {
            assert!(
                complete_capture && complete_delivery,
                "complete direct dependencies must survive capture and pending delivery"
            );
        }
        if expect_guards {
            assert!(
                guarded_capture && guarded_delivery,
                "checked guards must reach admission and pending delivery"
            );
        }
        assert!(
            captured,
            "fixture must retain borrowed capture dependencies"
        );
        assert!(
            indirect,
            "fixture must resolve a real indirect callback access"
        );
        if expected.contains(&"Call") {
            assert!(
                destination,
                "constructor fixture must resolve its caller destination"
            );
        }
        for kind in expected {
            assert!(
                access_kinds.contains(*kind),
                "missing {kind}: {access_kinds:?}"
            );
        }
    }
    assert!(c.parked.is_empty() && c.completed.is_empty() && c.values.is_empty());
    assert!(c.host.tasks.is_empty() && c.waits.is_empty() && c.entry.is_none());
    assert_eq!((c.domains.len(), c.domains.member_count()), (0, 0));
    Ok(result)
}

fn has_checked_guard(dependencies: &Dependencies) -> bool {
    matches!(dependencies,
        Dependencies::Partial { origins: super::reference_evidence::Origins::Partial(origins), .. }
        | Dependencies::Complete { origins: super::reference_evidence::Origins::Complete(origins), .. }
        if origins.iter().any(|o| o.guard))
}
