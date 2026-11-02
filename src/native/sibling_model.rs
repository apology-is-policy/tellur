//! Explicit test harness for related compiled contexts; no source scheduling syntax.
use super::*;

fn until_marker(contexts: &mut Contexts<'_>, key: &ContextKey, marker: &str) -> R<()> {
    let start = contexts.host.events.len();
    for _ in 0..500 {
        assert!(matches!(contexts.advance(key, 1)?, ContextPoll::Paused));
        if contexts.host.events[start..]
            .iter()
            .any(|event| event["kind"] == "emit" && event["message"] == marker)
        {
            return Ok(());
        }
    }
    Err("E_NATIVE: sibling harness marker not reached".into())
}
fn borrowed_counter(contexts: &Contexts<'_>, key: &ContextKey) -> V {
    for frame in contexts.parked[&key.id].machine.stack.iter().rev() {
        for (offset, value) in contexts
            .values
            .frame_values(&frame.frame)
            .unwrap()
            .iter()
            .enumerate()
            .rev()
        {
            if matches!(value, Some(V::Opaque { ty, .. }) if ty == "counter") {
                return V::Reference {
                    ty: "&counter".into(),
                    root: frame.base() + offset,
                    fields: vec![],
                };
            }
        }
    }
    panic!("expected live counter owner in retained frame")
}
fn complete(contexts: &mut Contexts<'_>, key: &ContextKey, quotas: &[usize]) -> R<()> {
    for quota in quotas.iter().copied().cycle().take(1000) {
        match contexts.advance(key, quota)? {
            ContextPoll::Paused => {}
            ContextPoll::Complete(Completion::Outcome((name, fields))) => {
                assert_eq!(name, "done");
                assert!(fields.is_empty());
                return Ok(());
            }
            _ => return Err("E_NATIVE: sibling harness expected normal completion".into()),
        }
    }
    Err("E_NATIVE: sibling harness failed to complete".into())
}

pub fn drive_siblings(
    host: &mut Host,
    controls: [&'static Control; 11],
    path: String,
    mode: &str,
    quotas: &[usize],
) -> R<Completion> {
    let [
        parent_control,
        sibling_control,
        descendant_control,
        failing_control,
        unstarted_control,
        independent_control,
        recovering_control,
        unhandled_control,
        ancestor_control,
        named_handler_control,
        lexical_handler_control,
    ] = controls;
    let fail = mode != "normal";
    let fuel = mode.starts_with("fuel");
    let cascade = mode == "fuel-parent" || mode == "fuel-peer";
    let escaping = mode == "root";
    let handling = mode.starts_with("handler-");
    let sibling_control = match mode {
        "handler-named" => named_handler_control,
        "handler-lexical" => lexical_handler_control,
        _ => sibling_control,
    };
    let parent_control = if escaping {
        unhandled_control
    } else {
        parent_control
    };
    host.enter("sibling harness")?;
    host.contingencies.push("host contingency marker".into());
    let mut contexts = Contexts::new(host);
    let initial_fuel = contexts.host.fuel;
    for round in 0..3 {
        let begin = contexts.host.events.len();
        let ancestor = if cascade {
            let key = contexts.start(ancestor_control, vec![V::String(path.clone())])?;
            until_marker(&mut contexts, &key, "owner ready")?;
            Some(key)
        } else {
            None
        };
        let peer = if mode == "fuel-peer" {
            let (made, mut fields) = contexts
                .host
                .primitive("make", vec![V::String(path.clone()), V::U32(7)])?;
            assert_eq!(made, "made");
            Some(contexts.start_child(
                ancestor.as_ref().unwrap(),
                unstarted_control,
                vec![fields.remove(0)],
            )?)
        } else {
            None
        };
        let parent = if let Some(ancestor) = &ancestor {
            contexts.start_child(ancestor, parent_control, vec![V::String(path.clone())])?
        } else {
            contexts.start(parent_control, vec![V::String(path.clone())])?
        };
        until_marker(&mut contexts, &parent, "owner ready")?;
        let other = contexts.start(independent_control, vec![V::String(path.clone())])?;
        until_marker(&mut contexts, &other, "independent ready")?;
        let parent_loan = borrowed_counter(&contexts, &parent);
        let sibling = contexts.start_child(
            &parent,
            sibling_control,
            vec![parent_loan, V::String(path.clone())],
        )?;
        until_marker(
            &mut contexts,
            &sibling,
            if handling {
                "sibling handler ready"
            } else {
                "sibling ready"
            },
        )?;
        // No new child admission into the handler is needed to stop active
        // contingency work: this child borrows the original parent instead.
        let borrower_parent = if handling { &parent } else { &sibling };
        let nested_loan = borrowed_counter(&contexts, borrower_parent);
        let descendant =
            contexts.start_child(borrower_parent, descendant_control, vec![nested_loan])?;
        until_marker(&mut contexts, &descendant, "descendant ready")?;
        let loan = borrowed_counter(&contexts, &parent);
        let recovering = contexts.start_child(&parent, recovering_control, vec![loan])?;
        complete(&mut contexts, &recovering, quotas)?;
        assert!(
            contexts.parked.contains_key(&sibling.id)
                && contexts.parked.contains_key(&descendant.id)
        );
        let (made, mut fields) = contexts
            .host
            .primitive("make", vec![V::String(path.clone()), V::U32(5)])?;
        assert_eq!(made, "made");
        let unstarted = contexts.start_child(&parent, unstarted_control, vec![fields.remove(0)])?;
        let before = (
            contexts.host.fuel,
            contexts.host.events.len(),
            contexts.values.len(),
            contexts.domains.len(),
        );
        assert!(contexts.advance(&parent, 1).is_err());
        if !handling {
            assert!(contexts.advance(&sibling, 1).is_err());
        }
        assert_eq!(
            before,
            (
                contexts.host.fuel,
                contexts.host.events.len(),
                contexts.values.len(),
                contexts.domains.len()
            )
        );
        if fail {
            let loan = borrowed_counter(&contexts, &parent);
            let failing = contexts.start_child(&parent, failing_control, vec![loan])?;
            if fuel {
                contexts.host.fuel = Some(0);
            }
            let mut failed = false;
            for quota in quotas.iter().copied().cycle().take(1000) {
                match contexts.advance(&failing, quota)? {
                    ContextPoll::Paused => {}
                    ContextPoll::Failed(report) => {
                        if fuel {
                            assert!(report.message.starts_with("E_FUEL"));
                            assert_eq!(report.kind, fault::Kind::ReturnedError);
                        } else {
                            assert_eq!(report.message, "E_FAULT: child failed");
                        }
                        contexts.host.events.push(serde_json::json!({"kind":"sibling_failure", "report":report, "round":round}));
                        failed = true;
                        break;
                    }
                    _ => panic!("faulting child completed normally"),
                }
            }
            assert!(failed);
            for retired in [&failing, &sibling, &descendant, &unstarted] {
                assert!(contexts.advance(retired, 0).is_err());
                assert!(
                    contexts
                        .start_child(retired, descendant_control, vec![])
                        .is_err()
                );
            }
            let arguments = if fuel {
                vec![]
            } else {
                vec![borrowed_counter(&contexts, &parent)]
            };
            let before = (
                contexts.next,
                contexts.domains.member_count(),
                contexts.host.resources.values.len(),
                contexts.host.events.len(),
            );
            assert!(
                contexts
                    .start_child(&parent, failing_control, arguments)
                    .is_err()
            );
            assert_eq!(
                before,
                (
                    contexts.next,
                    contexts.domains.member_count(),
                    contexts.host.resources.values.len(),
                    contexts.host.events.len()
                )
            );
        } else {
            complete(&mut contexts, &descendant, quotas)?;
            complete(&mut contexts, &sibling, quotas)?;
            complete(&mut contexts, &unstarted, quotas)?;
        }
        let terminal_owner = ancestor.as_ref().unwrap_or(&parent);
        if cascade {
            assert!(contexts.advance(&parent, 0).is_err());
            if let Some(peer) = &peer {
                assert!(contexts.advance(peer, 0).is_err());
            }
        }
        if fuel || escaping {
            let before = (
                contexts.host.fuel,
                contexts.host.events.len(),
                contexts.terminal.len(),
            );
            assert!(matches!(
                contexts.advance(terminal_owner, 0)?,
                ContextPoll::Paused
            ));
            let foreign = ContextKey {
                session: Arc::new(()),
                id: terminal_owner.id,
            };
            assert!(contexts.advance(&foreign, 1).is_err());
            assert_eq!(
                before,
                (
                    contexts.host.fuel,
                    contexts.host.events.len(),
                    contexts.terminal.len()
                )
            );
            let mut delivered = false;
            for quota in quotas.iter().copied().cycle().take(1000) {
                match contexts.advance(terminal_owner, quota)? {
                    ContextPoll::Paused => {}
                    ContextPoll::Failed(report) => {
                        assert_eq!(
                            report.boundary,
                            if cascade {
                                ancestor_control.name
                            } else {
                                parent_control.name
                            }
                        );
                        assert_eq!(
                            report.kind,
                            if fuel {
                                fault::Kind::ReturnedError
                            } else {
                                fault::Kind::SourceFault
                            }
                        );
                        contexts.host.events.push(serde_json::json!({"kind":"owner_failure", "report":report, "round":round}));
                        delivered = true;
                        break;
                    }
                    _ => panic!("terminal owner completed normally"),
                }
            }
            assert!(delivered && contexts.advance(terminal_owner, 0).is_err());
        } else {
            complete(&mut contexts, &parent, quotas)?;
        }
        if fuel {
            assert!(contexts.parked.contains_key(&other.id));
            assert_eq!(contexts.host.resources.values.len(), 1);
            let ContextPoll::Failed(report) = contexts.advance(&other, 1)? else {
                panic!("shared fuel limit")
            };
            assert!(report.message.starts_with("E_FUEL"));
            assert_eq!(report.kind, fault::Kind::ReturnedError);
        } else {
            complete(&mut contexts, &other, quotas)?;
        }
        let messages: Vec<_> = contexts.host.events[begin..]
            .iter()
            .filter(|event| event["kind"] == "emit")
            .map(|event| event["message"].as_str().unwrap())
            .collect();
        assert!(
            !messages
                .iter()
                .any(|message| message.starts_with("forbidden"))
        );
        assert!(messages.contains(&"local recovered"));
        assert_eq!(messages.contains(&"sibling handler ready"), handling);
        assert_eq!(messages.contains(&"owner handler"), fail && !fuel);
        for normal in [
            "owner continued",
            "sibling continued",
            "descendant continued",
            "unstarted body",
        ] {
            assert_eq!(messages.contains(&normal), !fail);
        }
        assert_eq!(messages.contains(&"independent complete"), !fuel);
        assert_eq!(messages.contains(&"owner complete"), !escaping && !fuel);
        assert!(contexts.parked.is_empty() && contexts.values.is_empty());
        assert!(contexts.terminal.is_empty());
        assert_eq!(
            (contexts.domains.len(), contexts.domains.member_count()),
            (0, 0)
        );
        assert!(
            contexts.host.resources.values.is_empty() && contexts.host.resources.scopes.is_empty()
        );
        assert_eq!(contexts.host.contingencies, ["host contingency marker"]);
        let baseline = contexts.host.activations.failure(
            "sibling harness",
            fault::Phase::Execution,
            "host observation",
        );
        assert_eq!(baseline.activations.len(), 1);
        assert_eq!(baseline.activations[0].control, "sibling harness");
        assert!(baseline.cause.is_none() && !contexts.host.source_faulted());
        if fuel {
            contexts.host.fuel = initial_fuel;
        }
    }
    contexts.host.contingencies.pop();
    contexts.host.leave();
    contexts.host.events.push(
        serde_json::json!({"kind":"siblings_settled", "fault":fail, "mode":mode, "rounds":3}),
    );
    Ok(Completion::Outcome(("done".into(), vec![])))
}
