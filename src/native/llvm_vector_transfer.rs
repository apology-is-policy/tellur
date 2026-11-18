//! Recognize a consuming vector prefix appended to another owned vector.
//! Only the successful, effect-free iteration is summarized. The original graph
//! still handles exhaustion, a reached bound and observable execution modes.
use super::*;

pub(super) struct Transfer<'a> {
    pub source: usize,
    pub destination: usize,
    pub limit: Option<&'a Expr>,
}

fn local(expr: &Expr, moved: bool) -> Option<Id> {
    let place = match expr {
        Expr::Move(place) => place,
        Expr::Place(place) if !moved => place,
        _ => return None,
    };
    (!place.indirect && place.fields.is_empty()).then_some(place.root)
}

fn reference(expr: &Expr, mutable: bool) -> Option<Id> {
    let Expr::Reference(place, ty) = expr.without_reference_evidence() else {
        return None;
    };
    let prefix = if mutable {
        "&mutable vector<"
    } else {
        "&vector<"
    };
    (!place.indirect && place.fields.is_empty() && ty.starts_with(prefix)).then_some(place.root)
}

pub(super) fn analyze(inst: &Instruction) -> Option<Transfer<'_>> {
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
    if parameters.len() != 2 {
        return None;
    }
    let position = |slot| {
        parameters
            .iter()
            .position(|(parameter, _)| *parameter == slot)
    };
    let mut facts = BTreeSet::new();
    let mut bound = None;
    let mut taken = None;
    let mut destination = None;
    for instruction in body {
        if !map_scan::selected(&instruction.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        match &instruction.action {
            Action::Decide {
                operation: Operation::Variant(Expr::Binary(op, left, right)),
                args,
                arms,
            } if op == "==" && args.is_empty() && bound.is_none() && taken.is_none() => {
                let Expr::Length(vector) = left.as_ref() else {
                    return None;
                };
                let accumulator = reference(vector, false)?;
                position(accumulator)?;
                let limit = local(right, false)?;
                if !matches!(right.as_ref(), Expr::Place(_)) || local_slots.contains(&limit) {
                    return None;
                }
                let arm = arms
                    .iter()
                    .find(|arm| arm.outcome == "false" && arm.slots.is_empty())?;
                if !local_atoms.contains(&arm.atom) {
                    return None;
                }
                facts.insert(arm.atom);
                bound = Some((accumulator, right.as_ref()));
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_take" && taken.is_none() && destination.is_none() => {
                let [source] = args.as_slice() else {
                    return None;
                };
                let source = local(source, true)?;
                position(source)?;
                let arm = arms.iter().find(|arm| arm.outcome == "item")?;
                let [item, tail] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_atoms.contains(&arm.atom)
                    || !arm.slots.iter().all(|slot| local_slots.contains(slot))
                {
                    return None;
                }
                facts.insert(arm.atom);
                taken = Some((source, *item, *tail));
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_push" && destination.is_none() => {
                let (source, item, _) = taken?;
                let [target, value] = args.as_slice() else {
                    return None;
                };
                let target = reference(target, true)?;
                position(target)?;
                if target == source
                    || local(value, false)? != item
                    || bound.is_some_and(|(accumulator, _)| accumulator != target)
                {
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
                facts.insert(arm.atom);
                destination = Some(target);
            }
            Action::Again { target, args } if target == id && args.len() == 2 => {
                let (source, _, tail) = taken?;
                let destination = destination?;
                let source_position = position(source)?;
                let destination_position = position(destination)?;
                if local(&args[source_position], true)? != tail
                    || local(&args[destination_position], true)? != destination
                {
                    return None;
                }
                return Some(Transfer {
                    source: source_position,
                    destination: destination_position,
                    limit: bound.map(|(_, limit)| limit),
                });
            }
            _ => return None,
        }
    }
    None
}
