//! Prove an effect-free rejected prefix of a synchronous u32 vector search.
//! The first candidate (including a possible predicate error) runs normally.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Predicate {
    pub field: usize,
    pub stop: Option<u32>,
}

// Follow the false arms of the concrete predicate. Other paths can have
// arbitrary effects: the bulk traversal stops before any of them is selected.
pub(super) fn predicate(region: &Region) -> Option<Predicate> {
    let [rule, element] = region.parameters.as_slice() else {
        return None;
    };
    if element.ty != "&U32"
        || region.asynchronous
        || region.boundary.is_some()
        || region.contingent
        || region.contingency
    {
        return None;
    }
    let mut facts = BTreeSet::new();
    let mut field = None;
    let mut stop = None;
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
                let Expr::Place(value) = left.as_ref() else {
                    return None;
                };
                if !value.indirect || value.root != element.id || !value.fields.is_empty() {
                    return None;
                }
                match (op.as_str(), right.as_ref()) {
                    (">=", Expr::Place(p))
                        if p.indirect && p.root == rule.id && field.is_none() =>
                    {
                        let [index] = p.fields.as_slice() else {
                            return None;
                        };
                        field = Some(*index);
                    }
                    ("==", Expr::Value(Value::U32(n))) if stop.is_none() => stop = Some(*n),
                    _ => return None,
                }
                let arm = arms
                    .iter()
                    .find(|a| a.outcome == "false" && a.slots.is_empty())?;
                facts.insert(arm.atom);
            }
            Action::Exit(out, values) if out == "skipped" && values.is_empty() => {
                return Some(Predicate {
                    field: field?,
                    stop,
                });
            }
            _ => return None,
        }
    }
    None
}

fn local_value(expr: &Expr, slot: Id) -> bool {
    matches!(expr, Expr::Place(p) if !p.indirect && p.fields.is_empty() && p.root == slot)
}

fn reborrow(expr: &Expr) -> Option<Id> {
    match expr.without_reference_evidence() {
        Expr::Reference(p, _) if p.indirect && p.fields.is_empty() => Some(p.root),
        _ => None,
    }
}

pub(super) struct Search {
    pub vector: Id,
    pub rule: Id,
    pub predicate: Predicate,
}

pub(super) fn analyze(
    inst: &Instruction,
    predicates: &BTreeMap<String, Predicate>,
) -> Option<Search> {
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
    let [(index, _)] = parameters.as_slice() else {
        return None;
    };
    let mut facts = BTreeSet::new();
    let mut vector = None;
    let mut element = None;
    let mut search = None;
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
                if !local_value(position, *index) || local_slots.contains(&source) {
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
            } if search.is_none() => {
                let [rule, value] = args.as_slice() else {
                    return None;
                };
                let rule = reborrow(rule)?;
                if local_slots.contains(&rule) || reborrow(value)? != element? {
                    return None;
                }
                let arm = arms
                    .iter()
                    .find(|a| a.outcome == "skipped" && a.slots.is_empty())?;
                facts.insert(arm.atom);
                search = Some(Search {
                    vector: vector?,
                    rule,
                    predicate: *predicates.get(name)?,
                });
            }
            Action::Decide {
                operation: Operation::Primitive(name),
                args,
                arms,
            } if search.is_some() && next.is_none() && name == "checked_add" => {
                let [position, Expr::Value(Value::U32(1))] = args.as_slice() else {
                    return None;
                };
                if !local_value(position, *index) {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "sum")?;
                let [slot] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(slot) {
                    return None;
                }
                next = Some(*slot);
                facts.insert(arm.atom);
            }
            Action::Again { target, args } if target == id => {
                let [position] = args.as_slice() else {
                    return None;
                };
                return local_value(position, next?).then_some(search?);
            }
            _ => return None,
        }
    }
    None
}
