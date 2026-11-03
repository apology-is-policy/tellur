use super::super::reference_evidence::{Origin, Origins, attach_checked};
use super::borrower_coverage_tests::{host, snapshot};
use super::*;

static ACCESSES: [Access; 4] = [
    Access {
        slot: Some(2),
        fields: &[],
        indirect: false,
        kind: AccessKind::Write,
    },
    Access {
        slot: Some(1),
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
        slot: Some(1),
        fields: &[1],
        indirect: false,
        kind: AccessKind::Write,
    },
];
fn plan(a: &Activation) -> AccessPlan {
    AccessPlan {
        cold_await: None,
        transition: Transition::Local,
        accesses: std::slice::from_ref(&ACCESSES[a.pc]),
        delivery: &[],
        opaque: false,
    }
}
fn parent(h: &mut Host, v: &mut Frames, a: &mut Activation) -> R<Step> {
    assert_eq!(a.pc, 0, "guarded owner/holder callback must not run");
    h.tick(1, 1)?;
    v[a.base() + 2] = Some(V::U32(42));
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
    name: "guarded_parent",
    slots: 3,
    atoms: 0,
    parameters: &[0, 1, 2],
    retained: &[],
    constructor: false,
    access: Some(plan),
    resume: parent,
};
static CHILD: Control = Control {
    name: "guarded_child",
    slots: 1,
    parameters: &[0],
    access: None,
    resume: child,
    ..PARENT
};

#[test]
fn complete_local_guards_preserve_whole_holders_across_waiting_and_completed_children() {
    for state in 0..3 {
        let mut h = host();
        let mut c = Contexts::new(&mut h);
        let root = c
            .start(&PARENT, vec![V::U32(7), V::Unit, V::U32(0)])
            .unwrap();
        let b = c.parked[&root.id].machine.stack[0].base();
        let value =
            super::super::super::reference(&c.values, b, &[], false, "&mutable U32").unwrap();
        c.values[b + 1] = Some(V::Construct {
            ty: "view".into(),
            name: "view".into(),
            fields: vec![value.clone(), V::U32(9)],
        });
        let origins = [
            Origin {
                root: b,
                fields: &[],
                indirect: false,
                guard: false,
                opaque: false,
            },
            Origin {
                root: b + 1,
                fields: &[0],
                indirect: false,
                guard: true,
                opaque: true,
            },
            Origin {
                root: b + 1,
                fields: &[],
                indirect: false,
                guard: false,
                opaque: false,
            },
            Origin {
                root: b + 1,
                fields: &[],
                indirect: false,
                guard: true,
                opaque: true,
            },
        ];
        let value = attach_checked(&c.values, value, &origins, true, true);
        let child = c.start_child(&root, &CHILD, vec![value]).unwrap();
        assert!(matches!(
            c.dependencies(&child).unwrap(),
            Dependencies::Complete {
                origins: Origins::Complete(_),
                ..
            }
        ));
        if state == 1 {
            c.advance_retained(&child, 1).unwrap();
            c.park_for_readiness(&child).unwrap();
        }
        if state == 2 {
            c.advance_retained(&child, 2).unwrap();
        }
        let progress = c.advance_disjoint(&root, 99);
        assert_eq!((progress.instructions, progress.transitions), (1, 1));
        assert!(matches!(progress.stop, AdmissionStop::Blocked(_)));
        assert_eq!(c.values[b + 2], Some(V::U32(42)));
        for pc in 1..4 {
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
