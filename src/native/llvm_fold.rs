//! Prove an effect-free advancing prefix with a scalar owned accumulator.
//! Terminal steps retain the authored reducer, outcomes and fault locations.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Reducer {
    pub field: usize,
    pub stop: Option<u32>,
}

fn local(expr: &Expr, slot: Id) -> bool {
    matches!(expr, Expr::Place(p) if !p.indirect && p.fields.is_empty() && p.root == slot)
}
fn read(expr: &Expr, slot: Id) -> bool {
    matches!(expr, Expr::Place(p) if p.indirect && p.fields.is_empty() && p.root == slot)
}
fn reborrow(expr: &Expr) -> Option<Id> {
    match expr.without_reference_evidence() {
        Expr::Reference(p, _) if p.indirect && p.fields.is_empty() => Some(p.root),
        _ => None,
    }
}

pub(super) fn reducer(region: &Region) -> Option<Reducer> {
    let [rule, current, item] = region.parameters.as_slice() else {
        return None;
    };
    if current.ty != "U32"
        || item.ty != "&U32"
        || region.asynchronous
        || region.boundary.is_some()
        || region.contingent
        || region.contingency
    {
        return None;
    }
    let mut facts = BTreeSet::new();
    let mut stop = None;
    let mut sum = None;
    let mut field = None;
    for inst in &region.code {
        if !inst.guard.evaluate(&facts) {
            continue;
        }
        match &inst.action {
            Action::Decide {
                operation: Operation::Variant(Expr::Binary(op, left, right)),
                args,
                arms,
            } if args.is_empty() => {
                match (op.as_str(), right.as_ref()) {
                    ("==", Expr::Value(Value::U32(n)))
                        if sum.is_none() && stop.is_none() && read(left, item.id) =>
                    {
                        stop = Some(*n)
                    }
                    (">=", Expr::Place(p))
                        if sum.is_some_and(|id| local(left, id))
                            && p.indirect
                            && p.root == rule.id
                            && field.is_none() =>
                    {
                        let [index] = p.fields.as_slice() else {
                            return None;
                        };
                        field = Some(*index);
                    }
                    _ => return None,
                }
                facts.insert(
                    arms.iter()
                        .find(|a| a.outcome == "false" && a.slots.is_empty())?
                        .atom,
                );
            }
            Action::Decide {
                operation: Operation::Primitive(name),
                args,
                arms,
            } if name == "checked_add" && sum.is_none() => {
                let [a, b] = args.as_slice() else {
                    return None;
                };
                if !local(a, current.id) || !read(b, item.id) {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "sum")?;
                let [value] = arm.slots.as_slice() else {
                    return None;
                };
                sum = Some(*value);
                facts.insert(arm.atom);
            }
            Action::Exit(out, values) if out == "advanced" => {
                let [value] = values.as_slice() else {
                    return None;
                };
                if !local(value, sum?) {
                    return None;
                }
                return Some(Reducer {
                    field: field?,
                    stop,
                });
            }
            _ => return None,
        }
    }
    None
}

pub(super) struct Fold {
    pub vector: Id,
    pub rule: Id,
    pub reducer: Reducer,
}

pub(super) fn analyze(inst: &Instruction, reducers: &BTreeMap<String, Reducer>) -> Option<Fold> {
    let Action::Loop {
        id,
        parameters,
        local_atoms,
        local_slots,
        body,
        ..
    } = &inst.action
    else {
        return None;
    };
    let [(index, _), (state, _)] = parameters.as_slice() else {
        return None;
    };
    let mut facts = BTreeSet::new();
    let mut vector = None;
    let mut element = None;
    let mut fold = None;
    let mut updated = None;
    let mut next = None;
    for step in body {
        if !map_scan::selected(&step.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        match &step.action {
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if vector.is_none() && name == "vector_get" => {
                let [source, position] = args.as_slice() else {
                    return None;
                };
                let source = reborrow(source)?;
                if !local(position, *index) || local_slots.contains(&source) {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "found")?;
                let [value] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(value) {
                    return None;
                }
                vector = Some(source);
                element = Some(*value);
                facts.insert(arm.atom);
            }
            Action::Decide {
                operation: Operation::Region(name),
                args,
                arms,
            } if fold.is_none() => {
                let [rule, current, value] = args.as_slice() else {
                    return None;
                };
                let rule = reborrow(rule)?;
                if local_slots.contains(&rule)
                    || !local(current, *state)
                    || reborrow(value)? != element?
                {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "advanced")?;
                let [value] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(value) {
                    return None;
                }
                updated = Some(*value);
                facts.insert(arm.atom);
                fold = Some(Fold {
                    vector: vector?,
                    rule,
                    reducer: *reducers.get(name)?,
                });
            }
            Action::Decide {
                operation: Operation::Primitive(name),
                args,
                arms,
            } if fold.is_some() && next.is_none() && name == "checked_add" => {
                let [position, Expr::Value(Value::U32(1))] = args.as_slice() else {
                    return None;
                };
                if !local(position, *index) {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "sum")?;
                let [value] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(value) {
                    return None;
                }
                next = Some(*value);
                facts.insert(arm.atom);
            }
            Action::Again { target, args } if target == id => {
                let [position, current] = args.as_slice() else {
                    return None;
                };
                if !local(position, next?) || !local(current, updated?) {
                    return None;
                }
                return fold;
            }
            _ => return None,
        }
    }
    None
}
