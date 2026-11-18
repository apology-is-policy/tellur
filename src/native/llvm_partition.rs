//! Summarize both effect-free classifier paths, preserving the first stop.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Classifier {
    pub field: usize,
    pub stop: Option<u32>,
}

pub(super) fn classifier(region: &Region) -> Option<Classifier> {
    let [_, item] = region.parameters.as_slice() else {
        return None;
    };
    if item.ty != "&U32"
        || region.asynchronous
        || region.boundary.is_some()
        || region.contingent
        || region.contingency
    {
        return None;
    }
    let left = classifier_path(region, true)?;
    (classifier_path(region, false)? == left).then_some(left)
}

fn classifier_path(region: &Region, left: bool) -> Option<Classifier> {
    let [rule, item] = region.parameters.as_slice() else {
        return None;
    };
    let mut facts = BTreeSet::new();
    let mut field = None;
    let mut stop = None;
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
                let Expr::Place(value) = a.as_ref() else {
                    return None;
                };
                if !value.indirect || value.root != item.id || !value.fields.is_empty() {
                    return None;
                }
                let selected = match (op.as_str(), b.as_ref()) {
                    ("==", Expr::Value(Value::U32(n))) if stop.is_none() && field.is_none() => {
                        stop = Some(*n);
                        "false"
                    }
                    ("<", Expr::Place(p)) if p.indirect && p.root == rule.id && field.is_none() => {
                        let [index] = p.fields.as_slice() else {
                            return None;
                        };
                        field = Some(*index);
                        if left { "true" } else { "false" }
                    }
                    _ => return None,
                };
                facts.insert(
                    arms.iter()
                        .find(|a| a.outcome == selected && a.slots.is_empty())?
                        .atom,
                );
            }
            Action::Exit(out, values)
                if out == (if left { "left" } else { "right" }) && values.is_empty() =>
            {
                return Some(Classifier {
                    field: field?,
                    stop,
                });
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
pub(super) struct Partition {
    pub source: usize,
    pub left: usize,
    pub right: usize,
    pub rule: Id,
    pub classifier: Classifier,
}
pub(super) fn analyze(
    inst: &Instruction,
    classifiers: &BTreeMap<String, Classifier>,
) -> Option<Partition> {
    let left = path(inst, classifiers, true)?;
    (path(inst, classifiers, false)? == left).then_some(left)
}
fn path(
    inst: &Instruction,
    classifiers: &BTreeMap<String, Classifier>,
    left: bool,
) -> Option<Partition> {
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
    let mut taken = None;
    let mut rule = None;
    let mut classifier = None;
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
            Action::Decide {
                operation: Operation::Region(name),
                args,
                arms,
            } if classifier.is_none() && taken.is_some() => {
                let [r, value] = args.as_slice() else {
                    return None;
                };
                let r = reference(r, true)?;
                if local_slots.contains(&r) || reference(value, false)? != taken?.1 {
                    return None;
                }
                let arm = arms.iter().find(|a| {
                    a.outcome == (if left { "left" } else { "right" }) && a.slots.is_empty()
                })?;
                if !local_atoms.contains(&arm.atom) {
                    return None;
                }
                rule = Some(r);
                classifier = Some(*classifiers.get(name)?);
                facts.insert(arm.atom);
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_push" && classifier.is_some() && destination.is_none() => {
                let [target, value] = args.as_slice() else {
                    return None;
                };
                let target = reference(target, false)?;
                position(target)?;
                if target == taken?.0 || local(value, false)? != taken?.1 {
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
                let source = position(source)?;
                let destination = position(destination?)?;
                for (i, (parameter, _)) in parameters.iter().enumerate() {
                    if local(&args[i], true)? != (if i == source { tail } else { *parameter }) {
                        return None;
                    }
                }
                let other = (0..3).find(|&i| i != source && i != destination)?;
                return Some(Partition {
                    source,
                    left: if left { destination } else { other },
                    right: if left { other } else { destination },
                    rule: rule?,
                    classifier: classifier?,
                });
            }
            _ => return None,
        }
    }
    None
}
