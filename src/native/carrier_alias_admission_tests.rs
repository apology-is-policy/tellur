use super::super::reference_evidence::{Origin, Origins, attach_checked};
use super::borrower_coverage_tests::{host, snapshot};
use super::*;

static ACCESSES: [Access; 5] = [
    Access {
        slot: Some(3),
        fields: &[],
        indirect: false,
        kind: AccessKind::Write,
    },
    Access {
        slot: Some(2),
        fields: &[],
        indirect: false,
        kind: AccessKind::Move,
    },
    Access {
        slot: Some(0),
        fields: &[],
        indirect: false,
        kind: AccessKind::Write,
    },
    Access {
        slot: Some(2),
        fields: &[1],
        indirect: false,
        kind: AccessKind::Write,
    },
    Access {
        slot: Some(1),
        fields: &[],
        indirect: false,
        kind: AccessKind::Write,
    },
];
fn plan(a: &Activation) -> AccessPlan {
    AccessPlan {
        transition: Transition::Local,
        accesses: std::slice::from_ref(&ACCESSES[a.pc]),
        delivery: &[],
        opaque: false,
    }
}
fn parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    assert_eq!(
        a.pc, 0,
        "ancestor or stored owner access must remain blocked"
    );
    h.tick(1, 1)?;
    v[a.base() + 3] = Some(V::U32(42));
    a.pc = 1;
    Ok(Step::Continue)
}
fn child(_: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    if a.pc == 0 {
        a.pc = 1;
        return Ok(Step::Continue);
    }
    Ok(Step::Complete(Completion::Outcome((
        "done".into(),
        vec![v[a.base()].take().unwrap()],
    ))))
}
static PARENT: Control = Control {
    name: "carrier_alias_parent",
    slots: 4,
    atoms: 0,
    parameters: &[0, 1, 2, 3],
    retained: &[],
    constructor: false,
    access: Some(plan),
    resume: parent,
};
static CHILD: Control = Control {
    name: "carrier_alias_child",
    slots: 1,
    parameters: &[0],
    access: None,
    resume: child,
    ..PARENT
};
fn origin(root: usize, fields: &'static [usize]) -> Origin {
    Origin {
        root,
        fields,
        indirect: false,
        guard: false,
        opaque: false,
    }
}
fn record(name: &str, fields: Vec<V>) -> V {
    V::Construct {
        ty: name.into(),
        name: name.into(),
        fields,
    }
}

#[test]
fn projected_carrier_alias_keeps_broad_holder_and_unreachable_ancestor_owners() {
    for state in 0..3 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let root = c
            .start(&PARENT, vec![V::U32(7), V::U32(8), V::Unit, V::U32(0)])
            .unwrap();
        let b = c.parked[&root.id].machine.stack[0].base();
        let reference = |c: &Contexts<'_>, offset| {
            attach_checked(
                &c.values,
                super::super::super::reference(&c.values, b + offset, &[], false, "&U32").unwrap(),
                &[origin(b + offset, &[])],
                true,
                true,
            )
        };
        c.values[b + 2] = Some(record(
            "envelope",
            vec![record("view", vec![reference(&c, 0)]), reference(&c, 1)],
        ));
        let projected = attach_checked(
            &c.values,
            super::super::super::reference(&c.values, b + 2, &[0], false, "&view").unwrap(),
            &[
                origin(b + 2, &[0]),
                origin(b + 2, &[]),
                origin(b, &[]),
                origin(b + 1, &[]),
            ],
            true,
            true,
        );
        let child = c.start_child(&root, &CHILD, vec![projected]).unwrap();
        let Dependencies::Complete {
            reachable,
            origins: Origins::Complete(origins),
        } = c.dependencies(&child).unwrap()
        else {
            panic!()
        };
        // The second pointee is retained only by broad static ancestry, not by
        // the runtime graph rooted at the projected view.
        assert!(
            reachable
                .iter()
                .all(|d| c.values.require_storage(&d.place.storage).unwrap() != b + 1)
        );
        assert!(
            origins
                .iter()
                .any(|o| c.values.require_storage(&o.place.storage).unwrap() == b + 1)
        );
        if state == 1 {
            c.advance_retained(&child, 1).unwrap();
            c.park_for_readiness(&child).unwrap();
        }
        if state == 2 {
            c.advance_retained(&child, 2).unwrap();
        }
        let p = c.advance_disjoint(&root, 99);
        assert_eq!((p.instructions, p.transitions), (1, 1));
        assert!(matches!(p.stop, AdmissionStop::Blocked(_)));
        assert_eq!(c.values[b + 3], Some(V::U32(42)));
        for pc in 1..5 {
            c.parked.get_mut(&root.id).unwrap().machine.stack[0].pc = pc;
            let before = snapshot(&c);
            assert!(matches!(
                c.borrower_interference(&root),
                BorrowerInterference::Overlap { .. }
            ));
            let p = c.advance_disjoint(&root, 99);
            assert_eq!((p.instructions, p.transitions), (0, 0));
            assert!(matches!(p.stop, AdmissionStop::Blocked(_)));
            assert_eq!(snapshot(&c), before);
        }
        c.terminate().unwrap();
        assert!(c.values.is_empty());
    }
}
