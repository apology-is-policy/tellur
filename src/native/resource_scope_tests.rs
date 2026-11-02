//! Native host membership tests; explicit internal selection, not a source scheduler.
use super::*;
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct State {
    drops: RefCell<Vec<u32>>,
    independent: RefCell<Option<(ResourceScope, V)>>,
}
struct Probe(u32, Rc<State>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.1.drops.borrow_mut().push(self.0);
    }
}
fn make(r: &mut Resources, id: u32, s: &Rc<State>) -> V {
    r.insert("probe", Probe(id, s.clone())).unwrap()
}
fn begin(r: &mut Resources, values: &[V]) -> ResourceScope {
    let p = r.prepare_scope(values).unwrap();
    r.commit_scope(p)
}
fn current(r: &Resources) -> ResourceScope {
    let id = r.current.unwrap();
    ResourceScope {
        registry: r.identity.clone(),
        id,
        parent: r.scopes[&id],
    }
}
fn dropped(s: &State) -> Vec<u32> {
    let mut v = s.drops.borrow().clone();
    v.sort();
    v
}
fn interleave(h: &mut Host, s: &Rc<State>) {
    let own = current(&h.resources);
    h.resources.select_scope(None).unwrap();
    let independent = begin(&mut h.resources, &[]);
    let value = make(&mut h.resources, 4, s);
    *s.independent.borrow_mut() = Some((independent, value));
    h.resources.select_scope(Some(&own)).unwrap();
}
fn finish_independent(h: &mut Host, s: &State) {
    let (scope, value) = s.independent.borrow_mut().take().unwrap();
    assert_eq!(h.resources.get::<Probe>(&value, "probe").unwrap().0, 4);
    h.resources.select_scope(Some(&scope)).unwrap();
    h.resources.finish_scope(&scope, true).unwrap();
    assert!(h.resources.scopes.is_empty());
    assert!(h.resources.current.is_none());
}
#[test]
fn resource_scope_admission_and_stale_foreign_exhausted_selection_are_atomic() {
    let s = Rc::new(State::default());
    let mut r = Resources::default();
    let owned = make(&mut r, 1, &s);
    let borrowed = make(&mut r, 2, &s);
    let V::Opaque { token, .. } = &borrowed else {
        panic!()
    };
    let forged = V::Opaque {
        ty: "wrong".into(),
        token: token.clone(),
    };
    assert!(r.prepare_scope([&owned, &forged]).is_err());
    assert!(r.values.values().all(|v| v.scope.is_none()));
    assert!(r.scopes.is_empty());
    assert_eq!(r.next_scope, 0);
    let scope = begin(
        &mut r,
        &[V::Task {
            ty: "task".into(),
            operation: "cold".into(),
            arguments: vec![
                owned.clone(),
                V::Reference {
                    ty: "&probe".into(),
                    root: 0,
                    fields: vec![],
                },
            ],
        }],
    );
    let mut other = Resources::default();
    let foreign = begin(&mut other, &[]);
    assert!(r.select_scope(Some(&foreign)).is_err());
    assert_eq!(r.current, Some(scope.id));
    let nested = begin(&mut r, &[]);
    assert!(r.finish_scope(&scope, true).is_err());
    r.select_scope(Some(&scope)).unwrap();
    assert!(r.finish_scope(&scope, true).is_err());
    assert!(dropped(&s).is_empty());
    r.select_scope(Some(&nested)).unwrap();
    r.finish_scope(&nested, false).unwrap();
    r.finish_scope(&scope, true).unwrap();
    assert_eq!(dropped(&s), [1]);
    assert!(r.select_scope(Some(&scope)).is_err());
    assert!(r.finish_scope(&scope, true).is_err());
    assert_eq!(r.get::<Probe>(&borrowed, "probe").unwrap().0, 2);
    let replacement = begin(&mut r, &[]);
    assert!(replacement.id > scope.id);
    r.finish_scope(&replacement, false).unwrap();
    r.next_scope = u64::MAX;
    assert!(r.prepare_scope([&borrowed]).is_err());
    assert!(r.current.is_none());
    assert!(r.scopes.is_empty());
    assert!(r.values.values().all(|v| v.scope.is_none()));
    assert_eq!(dropped(&s), [1]);
    drop(r);
    assert_eq!(dropped(&s), [1, 2]);
}
#[test]
fn lexical_cleanup_preserves_later_independent_allocations_and_retained_owners() {
    let s = Rc::new(State::default());
    let mut h = Host::new(json!({}), None, true, None);
    let retained = make(&mut h.resources, 1, &s);
    let input = make(&mut h.resources, 2, &s);
    let borrowed = make(&mut h.resources, 6, &s);
    let initial = vec![
        Some(retained),
        Some(input),
        Some(V::Reference {
            ty: "&probe".into(),
            root: 9,
            fields: vec![],
        }),
        None,
    ];
    let mut values = Frames::default();
    let _frame = values.enter(initial.len()).unwrap();
    for (slot, value) in values.iter_mut().zip(initial) {
        *slot = value;
    }
    let mut atoms = vec![];
    h.enter("main").unwrap();
    let result = h
        .lexical(
            &mut values,
            &mut atoms,
            0,
            "request",
            &[0],
            &[1, 2],
            &[1, 2],
            &[3],
            3,
            |h, _, _| {
                let _stranded_argument = make(&mut h.resources, 3, &s);
                interleave(h, &s);
                h.fault(V::String("failure".into()))
            },
            |h, values, _| {
                assert_eq!(dropped(&s), [2, 3]);
                let keep = values[0].take().unwrap();
                assert_eq!(h.resources.get::<Probe>(&keep, "probe").unwrap().0, 1);
                assert_eq!(h.resources.get::<Probe>(&borrowed, "probe").unwrap().0, 6);
                let (_, independent) = s.independent.borrow().as_ref().unwrap().clone();
                assert_eq!(
                    h.resources.get::<Probe>(&independent, "probe").unwrap().0,
                    4
                );
                drop(h.resources.take::<Probe>(&keep, "probe").unwrap());
                let _handler_temporary = make(&mut h.resources, 5, &s);
                Ok(Completion::Process(23))
            },
        )
        .unwrap();
    assert!(matches!(result, Completion::Process(23)));
    assert_eq!(dropped(&s), [1, 2, 3, 5]);
    assert_eq!(h.events.iter().filter(|e| e["kind"] == "fault").count(), 1);
    finish_independent(&mut h, &s);
    drop(h);
    assert_eq!(dropped(&s), [1, 2, 3, 4, 5, 6]);
}
fn named_work(
    h: &mut Host,
    _: &mut Frames,
    mut args: Vec<V>,
    retained: &mut Vec<V>,
) -> R<Completion> {
    let keep = args.remove(0);
    let s = h.resources.get::<Probe>(&keep, "probe")?.1.clone();
    retained.push(keep);
    // The second owned input and this newly allocated owner are no longer in a
    // generated frame. Membership must settle both before the handler starts.
    let _stranded = make(&mut h.resources, 3, &s);
    interleave(h, &s);
    h.fault(V::String("named failure".into()))
}
fn named_handler(h: &mut Host, _: &mut Frames, mut args: Vec<V>, _: &mut Vec<V>) -> R<Completion> {
    let keep = args.remove(0);
    let s = h.resources.get::<Probe>(&keep, "probe")?.1.clone();
    assert_eq!(dropped(&s), [2, 3]);
    assert_eq!(args, vec![V::String("named failure".into())]);
    drop(h.resources.take::<Probe>(&keep, "probe")?);
    Ok(Completion::Settled)
}
#[test]
fn named_cleanup_settles_transferred_and_stranded_inputs_before_contingency() {
    let s = Rc::new(State::default());
    let mut h = Host::new(json!({}), None, true, None);
    let kept = make(&mut h.resources, 1, &s);
    let input = make(&mut h.resources, 2, &s);
    h.enter("main").unwrap();
    h.supervise(
        &mut Frames::default(),
        vec![kept, input],
        named_work,
        named_handler,
        "request",
    )
    .unwrap();
    assert_eq!(dropped(&s), [1, 2, 3]);
    assert_eq!(h.resources.scopes.len(), 1);
    finish_independent(&mut h, &s);
    drop(h);
    assert_eq!(dropped(&s), [1, 2, 3, 4]);
}
#[test]
fn normal_lexical_exit_preserves_owned_task_results_and_retires_only_live_metadata() {
    let s = Rc::new(State::default());
    let mut h = Host::new(json!({}), None, true, None);
    let input = make(&mut h.resources, 1, &s);
    let initial = vec![Some(input), None];
    let mut values = Frames::default();
    let _frame = values.enter(initial.len()).unwrap();
    for (slot, value) in values.iter_mut().zip(initial) {
        *slot = value;
    }
    let mut atoms = vec![];
    h.enter("main").unwrap();
    let result = h
        .lexical(
            &mut values,
            &mut atoms,
            0,
            "request",
            &[],
            &[0],
            &[0],
            &[1],
            1,
            |h, v, _| {
                let new = make(&mut h.resources, 2, &s);
                Ok(Completion::Outcome((
                    "ready".into(),
                    vec![V::Task {
                        ty: "pending".into(),
                        operation: "later".into(),
                        arguments: vec![v[0].take().unwrap(), new],
                    }],
                )))
            },
            |_, _, _| panic!("normal work entered contingency"),
        )
        .unwrap();
    let (_, mut fields) = result.outcome().unwrap();
    let task = fields.remove(0);
    assert!(dropped(&s).is_empty());
    assert!(h.resources.scopes.is_empty());
    assert!(h.resources.values.values().all(|r| r.scope.is_none()));
    values[0] = Some(task);
    h.lexical(
        &mut values,
        &mut atoms,
        0,
        "next",
        &[],
        &[0],
        &[0],
        &[1],
        1,
        |h, _, _| h.fault(V::String("next failure".into())),
        |_, _, _| {
            assert_eq!(dropped(&s), [1, 2]);
            Ok(Completion::Process(0))
        },
    )
    .unwrap();
    assert!(h.resources.scopes.is_empty());
    assert!(h.resources.values.is_empty());
    drop(h);
    assert_eq!(dropped(&s), [1, 2]);
}
