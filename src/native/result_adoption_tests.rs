//! Shared host adoption with actual owners, independent of scheduler bookkeeping.
use super::*;
use std::{cell::RefCell, rc::Rc};
struct Probe(u32, Rc<RefCell<Vec<u32>>>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}
fn host() -> Host {
    let mut h = Host::new(json!({}), None, true, Some(100));
    h.enter("host").unwrap();
    h.contingencies.push("host marker".into());
    h
}
fn make(h: &mut Host, id: u32, drops: &Rc<RefCell<Vec<u32>>>) -> V {
    h.resources
        .insert("probe", Probe(id, drops.clone()))
        .unwrap()
}
fn scope(h: &Host, v: &V) -> Option<u64> {
    let V::Opaque { token, .. } = v else { panic!() };
    h.resources.values[token].scope
}
fn ambient(h: &Host) -> Json {
    json!({"cursor":h.cursor,"fuel":h.fuel,"events":h.events,"selected":h.resources.current,
        "next":h.resources.next_scope,"contingencies":h.contingencies,"tracker":format!("{:?}",h.activations)})
}
fn observed(h: &Host) -> Json {
    json!({"ambient":ambient(h),"scopes":h.resources.scopes,
        "owners":h.resources.values.iter().map(|(token,r)|(token,r.ty,r.scope)).collect::<Vec<_>>()})
}
fn rejected(h: &mut Host, drops: &Rc<RefCell<Vec<u32>>>, f: impl FnOnce(&mut Host) -> R<()>) {
    let before = observed(h);
    let retired = drops.borrow().clone();
    assert!(f(h).is_err());
    assert_eq!(observed(h), before);
    assert_eq!(*drops.borrow(), retired);
}
fn sorted(drops: &Rc<RefCell<Vec<u32>>>) -> Vec<u32> {
    let mut d = drops.borrow().clone();
    d.sort();
    d
}

#[test]
fn adoption_into_parked_nested_receiver_moves_owners_without_selecting_or_following_borrows() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut outer = h.begin_boundary("ambient", [], true).unwrap();
    let outside = make(&mut h, 9, &drops);
    let owner = make(&mut h, 1, &drops);
    let (mut parent, mut receiver) = h.prepare_context([&owner]).unwrap();
    h.exchange_context(&mut receiver).unwrap();
    let mut boundary = h.begin_boundary("receiver", [&owner], true).unwrap();
    let target = h.resources.current;
    h.exchange_context(&mut receiver).unwrap();
    let (mut root, mut source) = h.prepare_context([]).unwrap();
    h.exchange_context(&mut source).unwrap();
    h.enter("producer").unwrap();
    let a = make(&mut h, 2, &drops);
    let b = make(&mut h, 3, &drops);
    h.leave();
    h.exchange_context(&mut source).unwrap();
    let fields = vec![V::Construct {
        ty: "bundle".into(),
        name: "ready".into(),
        fields: vec![
            a.clone(),
            V::Task {
                ty: "task".into(),
                operation: "later".into(),
                arguments: vec![b.clone()],
            },
            V::Reference {
                ty: "&probe".into(),
                root: usize::MAX,
                fields: vec![99],
            },
            V::Opaque {
                ty: "script".into(),
                token: "opaque:script".into(),
            },
        ],
    }];
    let before = ambient(&h);
    let receiver_tracker = format!("{:?}", receiver.tracker);
    h.adopt_context_result(&source, &mut root, Some((&receiver, &parent)), &fields)
        .unwrap();
    assert_eq!(ambient(&h), before);
    assert_eq!(format!("{:?}", receiver.tracker), receiver_tracker);
    assert_eq!(scope(&h, &a), target);
    assert_eq!(scope(&h, &b), target);
    assert_eq!(scope(&h, &owner), target);
    assert_eq!(scope(&h, &outside), Some(outer.scope.id));
    assert!(root.retired);
    assert!(!h.resources.scopes.contains_key(&root.scope.id));
    assert!(h.require_context(&source, &root).is_err());
    assert!(drops.borrow().is_empty());
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, Some((&receiver, &parent)), &fields)
    });
    h.exchange_context(&mut receiver).unwrap();
    let message = h
        .fault::<()>(V::String("receiver failed".into()))
        .unwrap_err();
    let keep = h.resources.owned_tokens([&owner]).unwrap();
    h.boundary_fault(&mut boundary, &message, &keep).unwrap();
    assert_eq!(sorted(&drops), [2, 3]);
    h.enter_contingency(&mut boundary).unwrap();
    assert_eq!(h.resources.get::<Probe>(&owner, "probe").unwrap().0, 1);
    h.release_frame(&[Some(owner)]);
    h.finish_boundary(&mut boundary, &Err::<(), _>(message))
        .unwrap();
    h.finish_context(&mut parent, false).unwrap();
    h.exchange_context(&mut receiver).unwrap();
    h.release_frame(&[Some(outside)]);
    h.finish_boundary(&mut outer, &Ok(())).unwrap();
    assert_eq!(sorted(&drops), [1, 2, 3, 9]);
    assert!(h.resources.values.is_empty());
    assert!(h.resources.scopes.is_empty());
    assert_eq!(h.fuel, Some(100));
    assert_eq!(h.contingencies, ["host marker"]);
}

#[test]
fn adoption_rejects_crossed_stale_duplicate_and_incomplete_owner_graphs_atomically() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut foreign = host();
    let alien = make(&mut foreign, 8, &drops);
    let (mut foreign_root, mut foreign_context) = foreign.prepare_context([&alien]).unwrap();
    let (mut receiver_root, mut receiver) = h.prepare_context([]).unwrap();
    let (mut root, mut source) = h.prepare_context([]).unwrap();
    h.exchange_context(&mut source).unwrap();
    let a = make(&mut h, 1, &drops);
    let b = make(&mut h, 2, &drops);
    let mut nested = h.begin_boundary("nested", [], true).unwrap();
    h.exchange_context(&mut source).unwrap();
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &[a.clone(), b.clone()])
    });
    h.exchange_context(&mut source).unwrap();
    h.finish_boundary(&mut nested, &Ok(())).unwrap();
    h.exchange_context(&mut source).unwrap();
    let fields = vec![a.clone(), b.clone()];
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &[])
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &[a.clone()])
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &[a.clone(), a.clone(), b.clone()])
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(
            &source,
            &mut root,
            None,
            &[a.clone(), b.clone(), alien.clone()],
        )
    });
    let V::Opaque { token, .. } = &a else {
        panic!()
    };
    let wrong = V::Opaque {
        ty: "other".into(),
        token: token.clone(),
    };
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &[wrong, b.clone()])
    });
    let stale = make(&mut h, 7, &drops);
    drop(h.resources.take::<Probe>(&stale, "probe").unwrap());
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &[a.clone(), b.clone(), stale])
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(
            &source,
            &mut root,
            Some((&foreign_context, &foreign_root)),
            &fields,
        )
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(
            &source,
            &mut root,
            Some((&receiver, &foreign_root)),
            &fields,
        )
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&foreign_context, &mut root, None, &fields)
    });
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&receiver, &mut root, None, &fields)
    });
    // Selected sources and self receivers cannot retire the scope they need.
    h.resources.select_scope(Some(&root.scope)).unwrap();
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &fields)
    });
    h.resources.select_scope(None).unwrap();
    let same_root = ContextRoot {
        scope: root.scope.clone(),
        retired: false,
    };
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, Some((&source, &same_root)), &fields)
    });
    let original_scope = source.scope.take();
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(&source, &mut root, None, &fields)
    });
    source.scope = original_scope;
    assert!(!root.retired);
    // A valid receiver is retired between attempts; it must not accept owners.
    h.exchange_context(&mut receiver).unwrap();
    h.finish_context(&mut receiver_root, true).unwrap();
    h.exchange_context(&mut receiver).unwrap();
    rejected(&mut h, &drops, |h| {
        h.adopt_context_result(
            &source,
            &mut root,
            Some((&receiver, &receiver_root)),
            &fields,
        )
    });
    h.adopt_context_result(&source, &mut root, None, &fields)
        .unwrap();
    assert_eq!(scope(&h, &a), None);
    assert_eq!(scope(&h, &b), None);
    h.release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
    foreign.exchange_context(&mut foreign_context).unwrap();
    foreign.finish_context(&mut foreign_root, true).unwrap();
    foreign.exchange_context(&mut foreign_context).unwrap();
    assert_eq!(sorted(&drops), [1, 2, 7, 8]);
    assert!(h.resources.scopes.is_empty());
    assert!(h.resources.values.is_empty());
}

#[test]
fn current_scope_adoption_releases_on_receiver_fault_and_preserves_independent_roots() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let other = make(&mut h, 3, &drops);
    let (mut independent, mut parked) = h.prepare_context([&other]).unwrap();
    let mut receiver = h.begin_boundary("current receiver", [], true).unwrap();
    let (mut root, mut source) = h.prepare_context([]).unwrap();
    h.exchange_context(&mut source).unwrap();
    let result = make(&mut h, 2, &drops);
    h.exchange_context(&mut source).unwrap();
    let before = ambient(&h);
    h.adopt_context_result(&source, &mut root, None, &[result.clone()])
        .unwrap();
    assert_eq!(ambient(&h), before);
    assert_eq!(scope(&h, &result), Some(receiver.scope.id));
    let message = h.fault::<()>(V::String("stop".into())).unwrap_err();
    h.boundary_fault(&mut receiver, &message, &BTreeSet::new())
        .unwrap();
    assert_eq!(*drops.borrow(), [2]);
    h.finish_boundary(&mut receiver, &Err::<(), _>(message))
        .unwrap();
    assert_eq!(h.resources.get::<Probe>(&other, "probe").unwrap().0, 3);
    h.require_context(&parked, &independent).unwrap();
    h.exchange_context(&mut parked).unwrap();
    h.finish_context(&mut independent, true).unwrap();
    h.exchange_context(&mut parked).unwrap();
    assert_eq!(*drops.borrow(), [2, 3]);
    assert!(h.resources.values.is_empty());
    assert!(h.resources.scopes.is_empty());
}

#[test]
fn repeated_adoption_needs_no_new_scope_and_never_reuses_retired_result_roots() {
    let drops = Rc::new(RefCell::new(vec![]));
    let mut h = host();
    let mut previous = 0;
    for round in 0..32 {
        let (mut receiver_root, mut receiver) = h.prepare_context([]).unwrap();
        let (mut root, mut source) = h.prepare_context([]).unwrap();
        assert!(root.scope.id > receiver_root.scope.id && receiver_root.scope.id > previous);
        previous = root.scope.id;
        h.exchange_context(&mut source).unwrap();
        let result = make(&mut h, round, &drops);
        h.exchange_context(&mut source).unwrap();
        if round == 31 {
            h.resources.next_scope = u64::MAX;
        }
        let before = ambient(&h);
        h.adopt_context_result(
            &source,
            &mut root,
            Some((&receiver, &receiver_root)),
            &[result.clone()],
        )
        .unwrap();
        assert_eq!(ambient(&h), before);
        assert_eq!(h.resources.scopes.len(), 1);
        rejected(&mut h, &drops, |h| {
            h.adopt_context_result(&source, &mut root, None, &[result.clone()])
        });
        h.exchange_context(&mut receiver).unwrap();
        h.finish_context(&mut receiver_root, true).unwrap();
        h.exchange_context(&mut receiver).unwrap();
        assert_eq!(drops.borrow().len(), round as usize + 1);
        assert!(h.resources.scopes.is_empty());
        assert!(h.resources.values.is_empty());
    }
    assert_eq!(sorted(&drops), (0..32).collect::<Vec<_>>());
    assert_eq!(h.fuel, Some(100));
}
