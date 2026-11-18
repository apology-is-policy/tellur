//! Summarize only the effect-free pop/push advancing path of a reversal.
//! Exhaustion and every terminal effect still run through the checked graph.
use super::*;

pub(super) struct Reverse {
    pub source: usize,
    pub destination: usize,
}

fn local(expr: &Expr, moved: bool) -> Option<Id> {
    let place = match expr {
        Expr::Move(place) => place,
        Expr::Place(place) if !moved => place,
        _ => return None,
    };
    (!place.indirect && place.fields.is_empty()).then_some(place.root)
}

fn reference(expr: &Expr) -> Option<Id> {
    let Expr::Reference(place, ty) = expr.without_reference_evidence() else {
        return None;
    };
    (!place.indirect && place.fields.is_empty() && ty.starts_with("&mutable vector<"))
        .then_some(place.root)
}

pub(super) fn analyze(inst: &Instruction) -> Option<Reverse> {
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
    let mut popped = None;
    let mut destination = None;
    for instruction in body {
        if !map_scan::selected(&instruction.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        match &instruction.action {
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_pop" && popped.is_none() && destination.is_none() => {
                let [source] = args.as_slice() else {
                    return None;
                };
                let source = reference(source)?;
                position(source)?;
                let arm = arms.iter().find(|arm| arm.outcome == "item")?;
                let [item] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_atoms.contains(&arm.atom) || !local_slots.contains(item) {
                    return None;
                }
                facts.insert(arm.atom);
                popped = Some((source, *item));
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_push" && destination.is_none() => {
                let (source, item) = popped?;
                let [target, value] = args.as_slice() else {
                    return None;
                };
                let target = reference(target)?;
                position(target)?;
                if target == source || local(value, false)? != item {
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
                let (source, _) = popped?;
                let destination = destination?;
                let source_position = position(source)?;
                let destination_position = position(destination)?;
                if local(&args[source_position], true)? != source
                    || local(&args[destination_position], true)? != destination
                {
                    return None;
                }
                return Some(Reverse {
                    source: source_position,
                    destination: destination_position,
                });
            }
            _ => return None,
        }
    }
    None
}
