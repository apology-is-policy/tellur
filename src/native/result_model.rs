//! Delayed native result ownership under explicit harness selection only.
use super::sibling_model::{borrowed_counter, complete, until_marker};
use super::*;

fn retained(contexts: &mut Contexts<'_>, key: &ContextKey, quotas: &[usize]) -> R<()> {
    for quota in quotas.iter().copied().cycle().take(1000) {
        match contexts.advance_retained(key, quota)? {
            ContextPoll::Paused => {}
            ContextPoll::Ready => return Ok(()),
            _ => return Err("E_NATIVE: expected retained normal result".into()),
        }
    }
    Err("E_NATIVE: result model did not complete".into())
}
fn observed(contexts: &Contexts<'_>) -> serde_json::Value {
    serde_json::json!({
        "execution": super::sibling_model::observed(contexts),
        "completed": contexts.completed.iter().map(|(id,c)| (*id, format!("{:?}",
            match &c.outcome {Completion::Outcome((name,fields)) => (name,fields), _=>panic!()})))
            .collect::<Vec<_>>(),
        "resources": contexts.host.resources.values.iter().map(|(token,r)| (token,r.scope)).collect::<Vec<_>>(),
        "scopes": contexts.host.resources.scopes,
        "waits": format!("{:?}", contexts.waits),
        "selected": contexts.host.resources.current,
    })
}
fn check_rejections(
    contexts: &mut Contexts<'_>,
    parent: &ContextKey,
    other: &ContextKey,
    child: &ContextKey,
) -> R<()> {
    let before = observed(contexts);
    assert!(contexts.join(None, child).is_err());
    assert!(contexts.join(Some(other), child).is_err());
    let mut foreign = child.clone();
    foreign.session = Arc::new(());
    assert!(contexts.join(Some(parent), &foreign).is_err());
    let mut foreign_parent = parent.clone();
    foreign_parent.session = Arc::new(());
    assert!(contexts.join(Some(&foreign_parent), child).is_err());
    assert!(contexts.advance_retained(child, 0).is_err());
    assert!(contexts.advance(child, 1).is_err());
    assert!(contexts.park_for_readiness(child).is_err());
    assert!(contexts.advance(parent, 1).is_err());
    // A valid nested domain still cannot receive a result from its outer owner.
    let original = contexts.parked[&parent.id].machine.current.clone();
    let nested = contexts.domains.prepare(Some(&original))?;
    contexts.domains.commit(&nested)?;
    contexts.parked.get_mut(&parent.id).unwrap().machine.current = nested.key.clone();
    assert!(contexts.join(Some(parent), child).is_err());
    contexts.parked.get_mut(&parent.id).unwrap().machine.current = original;
    contexts.domains.close(&nested, &nested.key)?;
    // Lost original resource storage must fail without consuming the payload.
    let scope = contexts
        .completed
        .get_mut(&child.id)
        .unwrap()
        .parked
        .host
        .scope
        .take();
    assert!(contexts.join(Some(parent), child).is_err());
    contexts
        .completed
        .get_mut(&child.id)
        .unwrap()
        .parked
        .host
        .scope = scope;
    assert_eq!(observed(contexts), before);
    Ok(())
}
fn consume(
    contexts: &mut Contexts<'_>,
    parent: &ContextKey,
    child: &ContextKey,
    control: &'static Control,
    quotas: &[usize],
) -> R<()> {
    let outcome = contexts.join(Some(parent), child)?;
    let Completion::Outcome((name, fields)) = outcome else {
        panic!()
    };
    assert_eq!(name, "ready");
    assert_eq!(fields.len(), 2);
    // The owned leaf moves to the original parent scope; borrowed storage does not.
    let tokens = contexts.host.resources.owned_tokens(&fields)?;
    assert_eq!(tokens.len(), 1);
    let parent_scope = contexts.parked[&parent.id].host.scope.as_ref().unwrap().id;
    for token in tokens {
        assert_eq!(
            contexts.host.resources.values[&token].scope,
            Some(parent_scope)
        );
    }
    let V::Reference {
        root, fields: path, ..
    } = &fields[1]
    else {
        panic!("result borrow")
    };
    let borrowed = crate::read_place(&contexts.values, *root, path, false)?;
    let V::Opaque { token, .. } = borrowed else {
        panic!("borrow source")
    };
    assert_eq!(
        contexts.host.resources.values[&token].scope,
        Some(parent_scope)
    );
    let before = observed(contexts);
    assert!(contexts.join(Some(parent), child).is_err());
    assert_eq!(observed(contexts), before);
    let consumer = contexts.start_child(parent, control, fields)?;
    complete(contexts, &consumer, quotas)
}

pub fn drive_results(
    host: &mut Host,
    controls: [&'static Control; 6],
    path: String,
    mode: &str,
    quotas: &[usize],
) -> R<Completion> {
    let [owner, producer, consumer, failing, process, root_result] = controls;
    let rounds = if mode == "process" { 1 } else { 3 };
    for _ in 0..rounds {
        let mut contexts = Contexts::new(host);
        let first = contexts.start(owner, vec![V::String(path.clone()), V::U32(1)])?;
        until_marker(&mut contexts, &first, "owner ready")?;
        let other = contexts.start(owner, vec![V::String(path.clone()), V::U32(3)])?;
        until_marker(&mut contexts, &other, "owner ready")?;
        let mut children = vec![];
        for (parent, id) in [(&first, 2), (&first, 4), (&other, 5)] {
            let input = borrowed_counter(&contexts, parent);
            let child = contexts.start_child(
                parent,
                producer,
                vec![V::String(path.clone()), input, V::U32(id)],
            )?;
            let before = observed(&contexts);
            assert!(contexts.join(Some(parent), &child).is_err());
            assert_eq!(observed(&contexts), before);
            retained(&mut contexts, &child, quotas)?;
            let completed = &contexts.completed[&child.id];
            assert!(completed.parked.machine.stack.is_empty());
            assert_eq!(
                contexts
                    .host
                    .resources
                    .values
                    .values()
                    .filter(|r| r.scope == Some(completed.parked.root.scope.id))
                    .count(),
                1
            );
            children.push(child);
        }
        let root = contexts.start(root_result, vec![V::String(path.clone())])?;
        retained(&mut contexts, &root, quotas)?;
        assert_eq!(contexts.completed.len(), 4);
        assert_eq!(contexts.domains.member_count(), 6);
        check_rejections(&mut contexts, &first, &other, &children[0])?;
        if mode == "process" {
            let stop = contexts.start(process, vec![])?;
            let mut status = None;
            for quota in quotas.iter().copied().cycle().take(1000) {
                match contexts.advance(&stop, quota)? {
                    ContextPoll::Paused => {}
                    ContextPoll::Complete(Completion::Process(code)) => {
                        status = Some(code);
                        break;
                    }
                    _ => panic!("process completion"),
                }
            }
            assert_eq!(status, Some(23));
            assert!(contexts.completed.is_empty());
            assert!(contexts.parked.is_empty());
            assert!(contexts.values.is_empty());
            assert!(contexts.host.tasks.is_empty());
            assert_eq!(contexts.domains.member_count(), 0);
            assert_eq!(contexts.domains.len(), 0);
            assert!(contexts.host.resources.values.is_empty());
            assert!(contexts.host.resources.scopes.is_empty());
            assert!(contexts.join(None, &root).is_err());
            assert!(contexts.join(Some(&first), &children[0]).is_err());
            contexts
                .host
                .events
                .push(serde_json::json!({"kind":"results_settled","rounds":rounds}));
            return Ok(Completion::Process(23));
        }
        if mode == "fault" || mode == "joined-fault" {
            if mode == "joined-fault" {
                let completion = contexts.join(Some(&first), &children[0])?;
                // The owning boundary now retains this transferred resource even
                // before a following instruction puts the outcome into a frame.
                drop(completion);
            }
            let input = borrowed_counter(&contexts, &first);
            let faulting = contexts.start_child(&first, failing, vec![input])?;
            let mut failed = false;
            for quota in quotas.iter().copied().cycle().take(1000) {
                match contexts.advance(&faulting, quota)? {
                    ContextPoll::Paused => {}
                    ContextPoll::Failed(report) => {
                        assert_eq!(report.message, "E_FAULT: child failed");
                        failed = true;
                        break;
                    }
                    _ => panic!("expected failure"),
                }
            }
            assert!(failed);
            assert_eq!(contexts.completed.len(), 2); // Independent child and root survive.
            for child in &children[..2] {
                assert!(contexts.join(Some(&first), child).is_err());
            }
        } else {
            let order = if mode == "reverse" { [1, 0] } else { [0, 1] };
            for index in order {
                consume(&mut contexts, &first, &children[index], consumer, quotas)?;
            }
        }
        complete(&mut contexts, &first, quotas)?;
        consume(&mut contexts, &other, &children[2], consumer, quotas)?;
        complete(&mut contexts, &other, quotas)?;
        let Completion::Outcome((_, fields)) = contexts.join(None, &root)? else {
            panic!()
        };
        contexts.host.primitive("close", fields)?;
        assert!(contexts.join(None, &root).is_err());
        assert!(contexts.completed.is_empty());
        assert!(contexts.parked.is_empty());
        assert!(contexts.values.is_empty());
        assert!(contexts.host.tasks.is_empty());
        assert_eq!(contexts.domains.member_count(), 0);
        assert_eq!(contexts.domains.len(), 0);
        assert!(contexts.host.resources.values.is_empty());
        assert!(contexts.host.resources.scopes.is_empty());
    }
    host.events
        .push(serde_json::json!({"kind":"results_settled","rounds":rounds}));
    Ok(Completion::Outcome(("done".into(), vec![])))
}
