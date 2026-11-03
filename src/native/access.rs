//! Conservative symbolic accesses for one checked callback point, never permission.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Read,
    Move,
    Shared,
    Exclusive,
    Write,
    Retire,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Access {
    root: Option<Id>,
    fields: Vec<usize>,
    indirect: bool,
    kind: Kind,
}
#[derive(Debug)]
struct Plan {
    transition: &'static str,
    accesses: Vec<Access>,
    delivery: Vec<Access>,
    opaque: bool,
}
impl Plan {
    fn new(transition: &'static str) -> Self {
        Self {
            transition,
            accesses: vec![],
            delivery: vec![],
            opaque: false,
        }
    }
    fn place(&mut self, p: &Place, kind: Kind) {
        self.accesses.push(Access {
            root: Some(p.root),
            fields: p.fields.clone(),
            indirect: p.indirect,
            kind,
        });
    }
    fn slot(&mut self, id: Id, kind: Kind) {
        self.place(
            &Place {
                root: id,
                fields: vec![],
                indirect: false,
            },
            kind,
        );
    }
    fn expressions<'a>(&mut self, xs: impl IntoIterator<Item = &'a Expr>) {
        for x in xs {
            self.expression(x);
        }
    }
    fn expression(&mut self, e: &Expr) {
        match e {
            Expr::WithReferenceEvidence(value, _) => self.expression(value),
            Expr::Value(_) => {}
            Expr::Place(p) => self.place(p, Kind::Read),
            Expr::Move(p) => self.place(p, Kind::Move),
            Expr::Reference(p, ty) => match crate::manifest::reference_inner(ty) {
                Some((true, _)) => self.place(p, Kind::Exclusive),
                Some((false, _)) => self.place(p, Kind::Shared),
                None => self.opaque = true,
            },
            Expr::Project(e, _) | Expr::Not(e) => self.expression(e),
            Expr::Length(e) => {
                self.expression(e);
                self.opaque = true;
            }
            Expr::Pure(_, args) => {
                self.expressions(args);
                self.opaque = true;
            }
            Expr::Construct(_, _, args) => self.expressions(args),
            Expr::Record(_, fields) => self.expressions(fields.iter().map(|(_, e)| e)),
            // Both sides are conservative for short-circuit boolean expressions.
            Expr::Binary(_, a, b) => {
                self.expression(a);
                self.expression(b);
            }
        }
    }
}
type Loops = BTreeMap<Id, (Vec<Id>, Vec<Id>)>;
fn loops(code: &[Instruction], out: &mut Loops) {
    for i in code {
        match &i.action {
            Action::Loop {
                id,
                local_slots,
                parameters,
                body,
                ..
            } => {
                out.insert(
                    *id,
                    (
                        local_slots.clone(),
                        parameters.iter().map(|(id, _)| *id).collect(),
                    ),
                );
                loops(body, out);
            }
            Action::Boundary { body, handler, .. } => {
                loops(body, out);
                loops(handler, out);
            }
            _ => {}
        }
    }
}
fn plan(action: &Action, region: &Region, constructors: &BTreeSet<&str>) -> Plan {
    let mut p = Plan::new("Local");
    match action {
        Action::Bind(id, e) => {
            p.expression(e);
            p.slot(*id, Kind::Write);
        }
        Action::Assign(place, e) => {
            p.expression(e);
            p.place(place, Kind::Write);
        }
        Action::Drop(e) => p.expression(e),
        Action::Release(ids) => {
            for id in ids {
                p.slot(*id, Kind::Retire);
            }
        }
        Action::Emit(e) | Action::Fault(e) => {
            p.expression(e);
            p.opaque = true;
            p.transition = if matches!(action, Action::Emit(_)) {
                "Effect"
            } else {
                "Fault"
            };
        }
        Action::Exit(_, args) => {
            p.expressions(args);
            p.transition = "Return";
            p.opaque = true;
            for id in region.slots.keys() {
                p.slot(*id, Kind::Retire);
            }
            if region.is_constructor() {
                p.accesses.push(Access {
                    root: None,
                    fields: vec![],
                    indirect: false,
                    kind: Kind::Write,
                });
            }
        }
        Action::Terminate | Action::Escalate | Action::TerminateProcess(_) => {
            p.transition = match action {
                Action::Terminate => "Return",
                Action::Escalate => "Fault",
                _ => "Process",
            };
            p.opaque = true;
            for id in region.slots.keys() {
                p.slot(*id, Kind::Retire);
            }
        }
        Action::Boundary {
            retained, retire, ..
        } => {
            p.transition = "Boundary";
            p.opaque = true;
            for id in retained.iter().chain(retire) {
                p.slot(*id, Kind::Read);
            }
        }
        Action::Supervise { args, .. } => {
            p.expressions(args);
            p.transition = "Boundary";
            p.opaque = true;
        }
        Action::Loop {
            parameters,
            local_slots,
            ..
        } => {
            p.transition = "Loop";
            p.opaque = true;
            p.expressions(parameters.iter().map(|(_, e)| e));
            for id in local_slots {
                p.slot(*id, Kind::Retire);
            }
            for (id, _) in parameters {
                p.slot(*id, Kind::Write);
            }
        }
        Action::Again { target, args } | Action::Recover { target, args } => {
            p.transition = if matches!(action, Action::Again { .. }) {
                "Restart"
            } else {
                "Recover"
            };
            p.opaque = true;
            p.expressions(args);
            let mut targets = Loops::new();
            loops(&region.code, &mut targets);
            if let Some((locals, parameters)) = targets.get(target) {
                for id in locals {
                    p.slot(*id, Kind::Retire);
                }
                for id in parameters {
                    p.slot(*id, Kind::Write);
                }
            }
        }
        Action::Complete {
            bindings,
            release,
            unwind,
            ..
        } => {
            p.expressions(bindings.iter().map(|(_, e)| e));
            for (id, _) in bindings {
                p.slot(*id, Kind::Write);
            }
            for id in release {
                p.slot(*id, Kind::Retire);
            }
            if unwind.is_some() {
                p.transition = "Complete";
                p.opaque = true;
            }
        }
        Action::Decide {
            operation,
            args,
            arms,
        } => {
            match operation {
                Operation::Variant(e) | Operation::BorrowedVariant(e) => p.expression(e),
                _ => p.expressions(args),
            }
            let delayed = match operation {
                Operation::Region(_)
                | Operation::Run(_)
                | Operation::Await(_)
                | Operation::AwaitTask(_)
                | Operation::Join(_)
                | Operation::Spawn(_)
                | Operation::Yield => true,
                Operation::Defer(_)
                | Operation::Primitive(_)
                | Operation::Collection(_)
                | Operation::Variant(_)
                | Operation::BorrowedVariant(_) => false,
            };
            p.transition = match operation {
                Operation::Region(_) => "Call",
                Operation::Run(_) => "Run",
                Operation::Await(_) => "Await",
                Operation::AwaitTask(_) => "AwaitTask",
                Operation::Defer(_) => "Defer",
                Operation::Primitive(_) | Operation::Collection(_) => "Effect",
                _ => "Local",
            };
            p.opaque = delayed
                || matches!(
                    operation,
                    Operation::Primitive(_)
                        | Operation::Collection(_)
                        | Operation::BorrowedVariant(_)
                )
                || p.opaque;
            // Constructors write through their explicit destination during their
            // own return callback; their delivery only updates caller path facts.
            let constructor = matches!(operation, Operation::Region(name) if constructors.contains(name.as_str()));
            if !constructor {
                for id in arms.iter().flat_map(|a| &a.slots) {
                    let access = Access {
                        root: Some(*id),
                        fields: vec![],
                        indirect: false,
                        kind: Kind::Write,
                    };
                    if delayed {
                        p.delivery.push(access);
                    } else {
                        p.accesses.push(access);
                    }
                }
            }
        }
    }
    p
}
fn accesses(xs: &[Access], layout: &FrameLayout) -> String {
    let items = xs
        .iter()
        .map(|a| {
            format!(
                "Access{{slot:{:?},fields:&{:?},indirect:{},kind:AccessKind::{:?}}}",
                a.root.map(|id| layout.slots[&id]),
                a.fields,
                a.indirect,
                a.kind,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("&[{items}]")
}
fn render(p: &Plan, layout: &FrameLayout) -> String {
    format!(
        "AccessPlan{{cold_await:None,transition:Transition::{},accesses:{},delivery:{},opaque:{}}}",
        p.transition,
        accesses(&p.accesses, layout),
        accesses(&p.delivery, layout),
        p.opaque
    )
}
pub(super) fn instruction(
    i: &Instruction,
    region: &Region,
    layout: &FrameLayout,
    constructors: &BTreeSet<&str>,
    names: &BTreeMap<&str, usize>,
) -> String {
    let mut description = render(&plan(&i.action, region, constructors), layout);
    if let Action::Decide {
        operation: Operation::AwaitTask(name),
        args,
        ..
    } = &i.action
        && let [Expr::Move(place)] = args.as_slice()
        && !place.indirect
        && place.fields.is_empty()
    {
        description.push_str(&format!(
            ".with_cold_await({},&MODEL_{})",
            layout.slots[&place.root],
            names[name.as_str()]
        ));
    }
    format!(
        "if {} {{ {} }} else {{ AccessPlan::SKIP }}",
        guard(&i.guard, layout),
        description
    )
}
pub(super) fn boundary_end(i: &Instruction, layout: &FrameLayout) -> String {
    let Action::Boundary {
        work_slots,
        handler_slots,
        ..
    } = &i.action
    else {
        unreachable!()
    };
    let mut p = Plan::new("BoundaryEnd");
    p.opaque = true;
    for id in work_slots.iter().chain(handler_slots) {
        p.slot(*id, Kind::Retire);
    }
    render(&p, layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn region() -> Region {
        let p = crate::compile(
            "control main() exits done { exit done; }",
            crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
                .unwrap(),
        )
        .unwrap();
        let mut r = p.region("main").unwrap().clone();
        r.slots = [8, 21, 55]
            .into_iter()
            .map(|id| {
                (
                    id,
                    Slot {
                        id,
                        name: format!("s{id}"),
                        ty: "String".into(),
                    },
                )
            })
            .collect();
        r
    }
    fn place(root: Id, indirect: bool, fields: &[usize]) -> Place {
        Place {
            root,
            indirect,
            fields: fields.to_vec(),
        }
    }
    #[test]
    fn expression_plans_preserve_written_order_fields_indirection_and_borrow_modes() {
        let mut p = Plan::new("Local");
        p.expression(&Expr::Record(
            "pair".into(),
            vec![
                (1, Expr::Move(place(55, false, &[2]))),
                (
                    0,
                    Expr::Construct(
                        "inner".into(),
                        "inner".into(),
                        vec![
                            Expr::Reference(place(8, true, &[0, 1]), "&mutable String".into()),
                            Expr::Reference(place(21, false, &[]), "&String".into()),
                            Expr::Binary(
                                "&&".into(),
                                Box::new(Expr::Value(Value::Bool(false))),
                                Box::new(Expr::Place(place(8, true, &[3]))),
                            ),
                        ],
                    ),
                ),
            ],
        ));
        assert_eq!(
            p.accesses,
            vec![
                Access {
                    root: Some(55),
                    fields: vec![2],
                    indirect: false,
                    kind: Kind::Move
                },
                Access {
                    root: Some(8),
                    fields: vec![0, 1],
                    indirect: true,
                    kind: Kind::Exclusive
                },
                Access {
                    root: Some(21),
                    fields: vec![],
                    indirect: false,
                    kind: Kind::Shared
                },
                Access {
                    root: Some(8),
                    fields: vec![3],
                    indirect: true,
                    kind: Kind::Read
                },
            ]
        );
        assert!(!p.opaque);
        let rendered = render(&p, &FrameLayout::new(&region()));
        assert!(rendered.contains("slot:Some(2),fields:&[2]"));
        assert!(rendered.contains("slot:Some(0),fields:&[0, 1],indirect:true"));
        p.expression(&Expr::Length(Box::new(Expr::Place(place(21, false, &[])))));
        assert!(
            p.opaque,
            "referent accesses are not inferred from the outer value type"
        );
    }
    #[test]
    fn simultaneous_bindings_are_read_before_writes_and_retirement() {
        let r = region();
        let p = plan(
            &Action::Complete {
                atom: 0,
                bindings: vec![
                    (8, Expr::Move(place(21, false, &[]))),
                    (21, Expr::Move(place(8, false, &[]))),
                ],
                unwind: None,
                release: vec![55],
            },
            &r,
            &BTreeSet::new(),
        );
        assert_eq!(
            p.accesses
                .iter()
                .map(|a| (a.root.unwrap(), a.kind))
                .collect::<Vec<_>>(),
            [
                (21, Kind::Move),
                (8, Kind::Move),
                (8, Kind::Write),
                (21, Kind::Write),
                (55, Kind::Retire)
            ]
        );
        assert!(!p.opaque);
        let p = plan(
            &Action::Assign(place(8, true, &[2]), Expr::Move(place(55, false, &[]))),
            &r,
            &BTreeSet::new(),
        );
        assert_eq!(p.accesses[0].kind, Kind::Move);
        assert_eq!(
            p.accesses[1],
            Access {
                root: Some(8),
                fields: vec![2],
                indirect: true,
                kind: Kind::Write
            }
        );
    }
    #[test]
    fn call_kinds_and_delivery_writes_remain_separate_from_argument_evaluation() {
        let r = region();
        for (operation, expected) in [
            (Operation::Region("work".into()), "Call"),
            (Operation::Run("work".into()), "Run"),
            (Operation::Await("work".into()), "Await"),
            (Operation::AwaitTask("work".into()), "AwaitTask"),
            (Operation::Defer("work".into()), "Defer"),
        ] {
            let p = plan(
                &Action::Decide {
                    operation,
                    args: vec![Expr::Move(place(8, false, &[]))],
                    arms: vec![Arm {
                        outcome: "done".into(),
                        atom: 0,
                        slots: vec![21, 55],
                    }],
                },
                &r,
                &BTreeSet::new(),
            );
            assert_eq!(p.transition, expected);
            assert_eq!(p.accesses[0].kind, Kind::Move);
            if expected == "Defer" {
                assert!(!p.opaque && p.delivery.is_empty());
                assert_eq!(p.accesses.len(), 3);
            } else {
                assert!(p.opaque);
                assert_eq!(p.accesses.len(), 1);
                assert_eq!(
                    p.delivery
                        .iter()
                        .map(|a| a.root.unwrap())
                        .collect::<Vec<_>>(),
                    [21, 55]
                );
            }
        }
    }
    #[test]
    fn loop_restart_stages_swapped_arguments_without_retiring_outer_storage() {
        let program = crate::compile(
            include_str!("../../tests/fixtures/native_resume_loops.tellur"),
            crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
                .unwrap(),
        )
        .unwrap();
        let region = program.region("startup").unwrap();
        let Action::Loop { body, .. } = &region
            .code
            .iter()
            .find(|i| matches!(i.action, Action::Loop { .. }))
            .unwrap()
            .action
        else {
            unreachable!()
        };
        let restart = body
            .iter()
            .find(|i| matches!(i.action, Action::Again { .. }))
            .unwrap();
        let p = plan(&restart.action, region, &BTreeSet::new());
        let slot = |name: &str| region.slots.values().find(|s| s.name == name).unwrap().id;
        let movements = p.accesses.iter().filter(|a| a.kind == Kind::Move);
        assert_eq!(
            movements.map(|a| a.root.unwrap()).collect::<Vec<_>>(),
            [slot("right"), slot("left")]
        );
        let writes = p.accesses.iter().filter(|a| a.kind == Kind::Write);
        assert_eq!(
            writes.map(|a| a.root.unwrap()).collect::<Vec<_>>(),
            [slot("first"), slot("left"), slot("right")]
        );
        assert!(!p.accesses.iter().any(|a| a.root == Some(slot("text"))));
        let last_move = p
            .accesses
            .iter()
            .rposition(|a| a.kind == Kind::Move)
            .unwrap();
        let first_retire = p
            .accesses
            .iter()
            .position(|a| a.kind == Kind::Retire)
            .unwrap();
        let last_retire = p
            .accesses
            .iter()
            .rposition(|a| a.kind == Kind::Retire)
            .unwrap();
        let first_write = p
            .accesses
            .iter()
            .position(|a| a.kind == Kind::Write)
            .unwrap();
        assert!(last_move < first_retire && last_retire < first_write);
        assert_eq!(p.transition, "Restart");
        assert!(
            p.opaque,
            "crossed scopes still need dynamic retirement checks"
        );
    }
    #[test]
    fn constructor_returns_name_caller_destination_and_retire_all_local_slots() {
        let p = crate::compile("struct message { string text; constructor(string text) { self.text = move text; } } control main() exits done { exit done; }", crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap()).unwrap();
        let r = p.regions().find(|r| r.is_constructor()).unwrap();
        let exit = plan(
            &Action::Exit("constructed".into(), vec![Expr::Value(Value::Unit)]),
            r,
            &BTreeSet::new(),
        );
        assert_eq!(exit.transition, "Return");
        assert!(exit.opaque);
        assert_eq!(
            exit.accesses
                .iter()
                .filter(|a| a.kind == Kind::Retire)
                .count(),
            r.slots.len()
        );
        assert_eq!(
            exit.accesses.last().unwrap(),
            &Access {
                root: None,
                fields: vec![],
                indirect: false,
                kind: Kind::Write
            }
        );
    }
}
