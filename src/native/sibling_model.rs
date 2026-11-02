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
    controls: [&'static Control; 7],
    path: String,
    fail: bool,
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
    ] = controls;
    let mut contexts = Contexts::new(host);
    for round in 0..3 {
        let begin = contexts.host.events.len();
        let parent = contexts.start(parent_control, vec![V::String(path.clone())])?;
        until_marker(&mut contexts, &parent, "owner ready")?;
        let other = contexts.start(independent_control, vec![V::String(path.clone())])?;
        until_marker(&mut contexts, &other, "independent ready")?;
        let parent_loan = borrowed_counter(&contexts, &parent);
        let sibling = contexts.start_child(
            &parent,
            sibling_control,
            vec![parent_loan, V::String(path.clone())],
        )?;
        until_marker(&mut contexts, &sibling, "sibling ready")?;
        let nested_loan = borrowed_counter(&contexts, &sibling);
        let descendant = contexts.start_child(&sibling, descendant_control, vec![nested_loan])?;
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
        assert!(contexts.advance(&sibling, 1).is_err());
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
            let mut failed = false;
            for quota in quotas.iter().copied().cycle().take(1000) {
                match contexts.advance(&failing, quota)? {
                    ContextPoll::Paused => {}
                    ContextPoll::Failed(report) => {
                        assert_eq!(report.message, "E_FAULT: child failed");
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
            let loan = borrowed_counter(&contexts, &parent);
            let before = (
                contexts.next,
                contexts.domains.member_count(),
                contexts.host.resources.values.len(),
                contexts.host.events.len(),
            );
            assert!(
                contexts
                    .start_child(&parent, failing_control, vec![loan])
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
        complete(&mut contexts, &parent, quotas)?;
        complete(&mut contexts, &other, quotas)?;
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
        assert_eq!(messages.contains(&"owner handler"), fail);
        for normal in [
            "owner continued",
            "sibling continued",
            "descendant continued",
            "unstarted body",
        ] {
            assert_eq!(messages.contains(&normal), !fail);
        }
        assert!(messages.contains(&"independent complete") && messages.contains(&"owner complete"));
        assert!(contexts.parked.is_empty() && contexts.values.is_empty());
        assert_eq!(
            (contexts.domains.len(), contexts.domains.member_count()),
            (0, 0)
        );
        assert!(
            contexts.host.resources.values.is_empty() && contexts.host.resources.scopes.is_empty()
        );
    }
    contexts
        .host
        .events
        .push(serde_json::json!({"kind":"siblings_settled", "fault":fail, "rounds":3}));
    Ok(Completion::Outcome(("done".into(), vec![])))
}
