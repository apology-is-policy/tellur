//! Summarize an effect-free owned element -> singleton vector -> output loop.
use super::*;

pub(super) struct Singletons<'a> {
    pub source: usize,
    pub destination: usize,
    pub ty: &'a str,
}
fn local(expr: &Expr, moved: bool) -> Option<Id> {
    let p = match expr {
        Expr::Move(p) => p,
        Expr::Place(p) if !moved => p,
        _ => return None,
    };
    (!p.indirect && p.fields.is_empty()).then_some(p.root)
}
pub(super) fn analyze(inst: &Instruction) -> Option<Singletons<'_>> {
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
    let position = |slot| parameters.iter().position(|(p, _)| *p == slot);
    let mut facts = BTreeSet::new();
    let mut taken = None;
    let mut singleton = None;
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
            } if name == "vector_take" && taken.is_none() => {
                let [source] = args.as_slice() else {
                    return None;
                };
                let source = local(source, true)?;
                position(source)?;
                let arm = arms.iter().find(|a| a.outcome == "item")?;
                let [item, tail] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_atoms.contains(&arm.atom)
                    || !arm.slots.iter().all(|s| local_slots.contains(s))
                {
                    return None;
                }
                taken = Some((source, *item, *tail));
                facts.insert(arm.atom);
            }
            Action::Bind(slot, Expr::Construct(ty, name, values))
                if singleton.is_none() && taken.is_some() =>
            {
                let [value] = values.as_slice() else {
                    return None;
                };
                if name != "vector"
                    || !ty.starts_with("vector<")
                    || !local_slots.contains(slot)
                    || local(value, false)? != taken?.1
                {
                    return None;
                }
                singleton = Some((*slot, ty.as_str()));
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_push" && singleton.is_some() && destination.is_none() => {
                let [target, value] = args.as_slice() else {
                    return None;
                };
                let Expr::Reference(p, ty) = target.without_reference_evidence() else {
                    return None;
                };
                if p.indirect
                    || !p.fields.is_empty()
                    || !ty.starts_with("&mutable vector<")
                    || p.root == taken?.0
                    || local(value, true)? != singleton?.0
                {
                    return None;
                }
                position(p.root)?;
                let [arm] = arms.as_slice() else {
                    return None;
                };
                if arm.outcome != "pushed"
                    || !arm.slots.is_empty()
                    || !local_atoms.contains(&arm.atom)
                {
                    return None;
                }
                destination = Some(p.root);
                facts.insert(arm.atom);
            }
            Action::Again { target, args } if target == id && args.len() == 2 => {
                let (source, _, tail) = taken?;
                let destination = destination?;
                let source = position(source)?;
                let target = position(destination)?;
                if local(&args[source], true)? != tail || local(&args[target], true)? != destination
                {
                    return None;
                }
                return Some(Singletons {
                    source,
                    destination: target,
                    ty: singleton?.1,
                });
            }
            _ => return None,
        }
    }
    None
}
