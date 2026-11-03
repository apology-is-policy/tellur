use super::super::super::Host;
use super::*;
use std::{cell::RefCell, rc::Rc};

fn reference(root: usize, mutable: bool, fields: &[usize]) -> V {
    V::Reference {
        evidence: Default::default(),
        ty: if mutable { "&mutable value" } else { "&value" }.into(),
        root,
        fields: fields.to_vec(),
    }
}
fn record(fields: Vec<V>) -> V {
    V::Construct {
        ty: "carrier".into(),
        name: "selected".into(),
        fields,
    }
}
fn entries(d: &Dependencies) -> &[Dependency] {
    let Dependencies::Partial {
        reachable: entries, ..
    } = d
    else {
        panic!("{d:?}")
    };
    entries
}

#[test]
fn borrowed_carriers_retain_storage_and_attenuate_contained_permissions() {
    let mut values = Frames::default();
    let f = values.enter(4).unwrap();
    let b = f.base();
    values[b] = Some(V::String("left".into()));
    values[b + 1] = Some(V::String("right".into()));
    values[b + 2] = Some(record(vec![
        reference(b, true, &[]),
        reference(b + 1, false, &[]),
    ]));
    values[b + 3] = Some(record(vec![reference(b + 2, true, &[])]));
    for outer in [false, true] {
        let d = Dependencies::describe(&values, &[reference(b + 3, outer, &[])]);
        d.require_live(&values).unwrap();
        let actual = entries(&d)
            .iter()
            .map(|d| (values.require_storage(&d.place.storage).unwrap(), d.mutable))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [(b + 3, outer), (b + 2, outer), (b, outer), (b + 1, false)]
        );
    }
    let owned = Dependencies::describe(
        &values,
        &[record(vec![
            reference(b, true, &[]),
            reference(b + 1, false, &[]),
        ])],
    );
    assert_eq!(
        entries(&owned).len(),
        2,
        "owned carrier adds no supplier-storage loan"
    );
    assert!(entries(&owned)[0].mutable);
    let strengthened = Dependencies::describe(
        &values,
        &[reference(b + 3, false, &[]), reference(b + 3, true, &[])],
    );
    assert_eq!(
        strengthened,
        Dependencies::describe(&values, &[reference(b + 3, true, &[])])
    );
    let projected = Dependencies::describe(&values, &[reference(b + 2, false, &[1])]);
    assert_eq!(entries(&projected).len(), 2);
    assert_eq!(entries(&projected)[0].place.fields, [1]);
    assert_eq!(
        entries(&projected)[1].place.storage,
        values.local_storage(&f, 1).unwrap()
    );
}

#[test]
fn cyclic_references_terminate_and_strengthening_revisits_shared_paths() {
    let mut values = Frames::default();
    let f = values.enter(2).unwrap();
    let b = f.base();
    values[b] = Some(record(vec![
        reference(b, false, &[]),
        reference(b + 1, true, &[]),
    ]));
    values[b + 1] = Some(record(vec![reference(b, true, &[])]));
    let before = format!("{:?}", &*values);
    let d = Dependencies::describe(
        &values,
        &[reference(b, false, &[]), reference(b, true, &[])],
    );
    assert_eq!(entries(&d).len(), 2);
    assert!(entries(&d).iter().all(|d| d.mutable));
    assert_eq!(format!("{:?}", &*values), before);
    for _ in 0..4 {
        assert_eq!(
            Dependencies::describe(&values, &[reference(b, true, &[])]),
            d
        );
    }
}

#[test]
fn traversal_limits_and_unavailable_places_stay_explicitly_unknown() {
    let mut values = Frames::default();
    let f = values.enter(1).unwrap();
    values[f.base()] = Some(record(vec![V::U32(7)]));
    for input in [
        reference(usize::MAX, false, &[]),
        reference(f.base(), false, &[9]),
        V::Reference {
            evidence: Default::default(),
            ty: "&mutable ".into(),
            root: f.base(),
            fields: vec![],
        },
        record(vec![V::U32(0); LIMIT + 1]),
    ] {
        let before = values.to_vec();
        let d = Dependencies::describe(&values, &[input]);
        assert!(matches!(d, Dependencies::Unknown(_)));
        assert!(d.require_live(&values).is_err());
        assert_eq!(*values, before);
    }
    assert!(describe(&values, &[reference(f.base(), false, &[])], 2).is_err());
    assert!(describe(&values, &[reference(f.base(), false, &[])], 3).is_ok());
    assert!(describe(&values, &[reference(f.base(), false, &[0])], 2).is_err());
    assert!(describe(&values, &[reference(f.base(), false, &[0])], 3).is_ok());
    let mut deep = V::Unit;
    for _ in 0..128 {
        deep = record(vec![deep]);
    }
    assert!(describe(&values, &[deep], 64).is_err());
    let mut known = Dependencies::Partial {
        reachable: vec![],
        origins: Origins::default(),
    };
    known.extend(Dependencies::Unknown("bounded".into()));
    known.extend(Dependencies::Partial {
        reachable: vec![],
        origins: Origins::default(),
    });
    assert_eq!(known, Dependencies::Unknown("bounded".into()));
}

#[test]
fn captured_identities_survive_moves_and_results_without_copying_resource_owners() {
    struct Probe(Rc<RefCell<usize>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }
    let drops = Rc::new(RefCell::new(0));
    let mut host = Host::new(serde_json::json!({}), None, true, None);
    let mut values = Frames::default();
    let f = values.enter(1).unwrap();
    let text = "same storage".repeat(100);
    let allocation = text.as_ptr();
    values[f.base()] = Some(record(vec![V::String(text)]));
    let resource = host
        .resources
        .insert("probe", Probe(drops.clone()))
        .unwrap();
    let captures = vec![record(vec![reference(f.base(), false, &[0]), resource])];
    let moved = captures;
    let mut held = Dependencies::describe(&values, &moved);
    let first = held.clone();
    held.extend(Dependencies::describe(
        &values,
        &[reference(f.base(), true, &[0])],
    ));
    assert_eq!(entries(&held).len(), 1);
    assert!(entries(&held)[0].mutable);
    let V::Construct { fields, .. } = values[f.base()].as_ref().unwrap() else {
        panic!()
    };
    let V::String(text) = &fields[0] else {
        panic!()
    };
    assert_eq!(text.as_ptr(), allocation);
    assert_eq!(*drops.borrow(), 0);
    host.release_frame(&moved.into_iter().map(Some).collect::<Vec<_>>());
    assert_eq!(*drops.borrow(), 1);
    first.require_live(&values).unwrap();
    values.retire(&f).unwrap();
    let reused = values.enter(1).unwrap();
    values[reused.base()] = Some(record(vec![V::String("replacement".into())]));
    assert_eq!(reused.base(), f.base());
    assert!(first.require_live(&values).is_err());
    let fresh = Dependencies::describe(&values, &[reference(reused.base(), false, &[0])]);
    assert_ne!(first, fresh);
    assert_eq!(*drops.borrow(), 1);
}
