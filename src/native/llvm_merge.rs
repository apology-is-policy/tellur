//! Prove every comparison result and both advancing ownership transfers.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Comparator {
    pub field: usize,
}

pub(super) fn comparator(region: &Region) -> Option<Comparator> {
    let [_, left, right] = region.parameters.as_slice() else {
        return None;
    };
    if left.ty != right.ty
        || !left.ty.starts_with('&')
        || left.ty.starts_with("&mutable ")
        || region.asynchronous
        || region.boundary.is_some()
        || region.contingent
        || region.contingency
    {
        return None;
    }
    let before = comparator_path(region, -1)?;
    (comparator_path(region, 0)? == before && comparator_path(region, 1)? == before)
        .then_some(before)
}
fn comparator_path(region: &Region, ordering: i8) -> Option<Comparator> {
    let [_, left, right] = region.parameters.as_slice() else {
        return None;
    };
    let mut facts = BTreeSet::new();
    let mut field = None;
    for inst in &region.code {
        if !inst.guard.evaluate(&facts) {
            continue;
        }
        match &inst.action {
            Action::Decide {
                operation: Operation::Variant(Expr::Binary(op, a, b)),
                args,
                arms,
            } if args.is_empty() => {
                let (Expr::Place(a), Expr::Place(b)) = (a.as_ref(), b.as_ref()) else {
                    return None;
                };
                let [index] = a.fields.as_slice() else {
                    return None;
                };
                if !a.indirect
                    || !b.indirect
                    || a.root != left.id
                    || b.root != right.id
                    || b.fields != a.fields
                    || field.is_some_and(|f| f != *index)
                {
                    return None;
                }
                field = Some(*index);
                let yes = match op.as_str() {
                    "<" => ordering < 0,
                    ">" => ordering > 0,
                    "==" => ordering == 0,
                    "<=" => ordering <= 0,
                    ">=" => ordering >= 0,
                    "!=" => ordering != 0,
                    _ => return None,
                };
                facts.insert(
                    arms.iter()
                        .find(|a| {
                            a.outcome == if yes { "true" } else { "false" } && a.slots.is_empty()
                        })?
                        .atom,
                );
            }
            Action::Exit(out, values)
                if out
                    == match ordering {
                        -1 => "before",
                        0 => "equal",
                        _ => "after",
                    }
                    && values.is_empty() =>
            {
                return Some(Comparator { field: field? });
            }
            _ => return None,
        }
    }
    None
}
fn local(expr: &Expr, moved: bool) -> Option<Id> {
    let p = match expr {
        Expr::Move(p) => p,
        Expr::Place(p) if !moved => p,
        _ => return None,
    };
    (!p.indirect && p.fields.is_empty()).then_some(p.root)
}
fn reference(expr: &Expr, indirect: bool) -> Option<Id> {
    match expr.without_reference_evidence() {
        Expr::Reference(p, _) if p.indirect == indirect && p.fields.is_empty() => Some(p.root),
        _ => None,
    }
}
#[derive(PartialEq, Eq)]
pub(super) struct Merge {
    pub left: usize,
    pub right: usize,
    pub result: usize,
    pub comparator: Comparator,
}
pub(super) fn analyze(
    inst: &Instruction,
    comparators: &BTreeMap<String, Comparator>,
) -> Option<Merge> {
    let before = path(inst, comparators, "before")?;
    (path(inst, comparators, "equal")? == before && path(inst, comparators, "after")? == before)
        .then_some(before)
}
fn path(
    inst: &Instruction,
    comparators: &BTreeMap<String, Comparator>,
    outcome: &str,
) -> Option<Merge> {
    let Action::Loop {
        id,
        parameters,
        local_slots,
        local_atoms,
        body,
        ..
    } = &inst.action
    else {
        return None;
    };
    if parameters.len() != 3 {
        return None;
    }
    let position = |slot| parameters.iter().position(|(p, _)| *p == slot);
    let mut facts = BTreeSet::new();
    let mut heads = Vec::new();
    let mut comparator = None;
    let mut taken = None;
    let mut destination = None;
    for step in body {
        if !map_scan::selected(&step.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        match &step.action {
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_get" && comparator.is_none() && heads.len() < 2 => {
                let [owner, Expr::Value(Value::U32(0))] = args.as_slice() else {
                    return None;
                };
                let owner = reference(owner, false)?;
                position(owner)?;
                if heads.iter().any(|&(p, _)| p == owner) {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "found")?;
                let [head] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(head) || !local_atoms.contains(&arm.atom) {
                    return None;
                }
                heads.push((owner, *head));
                facts.insert(arm.atom);
            }
            Action::Decide {
                operation: Operation::Region(name),
                args,
                arms,
            } if comparator.is_none() && heads.len() == 2 => {
                let [rule, a, b] = args.as_slice() else {
                    return None;
                };
                if local_slots.contains(&reference(rule, true)?)
                    || reference(a, true)? != heads[0].1
                    || reference(b, true)? != heads[1].1
                {
                    return None;
                }
                let arm = arms
                    .iter()
                    .find(|a| a.outcome == outcome && a.slots.is_empty())?;
                if !local_atoms.contains(&arm.atom) {
                    return None;
                }
                comparator = Some(*comparators.get(name)?);
                facts.insert(arm.atom);
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_take" && comparator.is_some() && taken.is_none() => {
                let [source] = args.as_slice() else {
                    return None;
                };
                let source = local(source, true)?;
                if source != heads[usize::from(outcome == "after")].0 {
                    return None;
                }
                let arm = arms.iter().find(|a| a.outcome == "item")?;
                let [value, tail] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_atoms.contains(&arm.atom)
                    || !arm.slots.iter().all(|s| local_slots.contains(s))
                {
                    return None;
                }
                taken = Some((source, *value, *tail));
                facts.insert(arm.atom);
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_push" && taken.is_some() && destination.is_none() => {
                let [target, value] = args.as_slice() else {
                    return None;
                };
                let target = reference(target, false)?;
                position(target)?;
                if heads.iter().any(|&(p, _)| p == target) || local(value, false)? != taken?.1 {
                    return None;
                }
                let [arm] = arms.as_slice() else {
                    return None;
                };
                if arm.outcome != "pushed"
                    || !arm.slots.is_empty()
                    || !local_atoms.contains(&arm.atom)
                {
                    return None;
                }
                destination = Some(target);
                facts.insert(arm.atom);
            }
            Action::Again { target, args } if target == id && args.len() == 3 => {
                let (source, _, tail) = taken?;
                for (i, (parameter, _)) in parameters.iter().enumerate() {
                    if local(&args[i], true)?
                        != if *parameter == source {
                            tail
                        } else {
                            *parameter
                        }
                    {
                        return None;
                    }
                }
                return Some(Merge {
                    left: position(heads[0].0)?,
                    right: position(heads[1].0)?,
                    result: position(destination?)?,
                    comparator: comparator?,
                });
            }
            _ => return None,
        }
    }
    None
}
