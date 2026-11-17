//! Prove an effect-free rejected prefix of a synchronous u32 vector search.
//! Independently prove the positive branch before lowering a borrowed hit.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Predicate {
    pub field: usize,
    pub stop: Option<u32>,
    direct_match: bool,
}

// Prove rejection and acceptance separately. Either branch can contain user effects.
pub(super) fn predicate(region: &Region) -> Option<Predicate> {
    let [_, element] = region.parameters.as_slice() else {
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
    let (field, stop) = predicate_path(region, false)?;
    let direct_match = predicate_path(region, true).is_some_and(|(hit_field, hit_stop)| {
        hit_field == field && (hit_stop.is_none() || hit_stop == stop)
    });
    Some(Predicate {
        field,
        stop,
        direct_match,
    })
}

fn predicate_path(region: &Region, matched: bool) -> Option<(usize, Option<u32>)> {
    let [rule, element] = region.parameters.as_slice() else {
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
                let outcome = if matched && op == ">=" {
                    "true"
                } else {
                    "false"
                };
                let arm = arms
                    .iter()
                    .find(|a| a.outcome == outcome && a.slots.is_empty())?;
                facts.insert(arm.atom);
            }
            Action::Exit(out, values)
                if out == (if matched { "matched" } else { "skipped" }) && values.is_empty() =>
            {
                return Some((field?, stop));
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

pub(super) struct Hit<'a> {
    pub source: &'a Expr,
    pub evidence: &'a ReferenceEvidence,
    pub outcome: &'a str,
    pub site: Id,
}

pub(super) struct Search<'a> {
    pub vector: Id,
    pub rule: Id,
    pub predicate: Predicate,
    pub hit: Option<Hit<'a>>,
}

pub(super) fn analyze<'a>(
    inst: &'a Instruction,
    predicates: &BTreeMap<String, Predicate>,
) -> Option<Search<'a>> {
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
    let mut source_expr = None;
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
                source_expr = Some(source);
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
                    hit: None,
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
                if !local_value(position, next?) {
                    return None;
                }
                let mut search = search?;
                if search.predicate.direct_match {
                    search.hit = hit_exit(inst, source_expr?, element?, search.vector, search.rule);
                }
                return Some(search);
            }
            _ => return None,
        }
    }
    None
}

// Walk the selected hit path. The source annotation and the returning annotation
// must bind against inputs that exist before the loop. The result may have a
// different public outcome name. Any intervening user action keeps the checked path.
fn hit_exit<'a>(
    inst: &'a Instruction,
    source: &'a Expr,
    element: Id,
    vector: Id,
    rule: Id,
) -> Option<Hit<'a>> {
    let Action::Loop {
        body, local_atoms, ..
    } = &inst.action
    else {
        return None;
    };
    let input_evidence = |e: &ReferenceEvidence| {
        matches!(
            e,
            ReferenceEvidence::Partial(_) | ReferenceEvidence::Complete(_)
        ) && e
            .origins()
            .iter()
            .all(|o| o.place.root == vector || o.place.root == rule)
    };
    let Expr::WithReferenceEvidence(base, annotation) = source else {
        return None;
    };
    if !input_evidence(annotation)
        || !matches!(base.as_ref(), Expr::Reference(_, ty) if ty == "&vector<U32>")
    {
        return None;
    }
    let mut facts = BTreeSet::new();
    let mut outcomes = ["found", "matched"].into_iter();
    for step in body {
        if !map_scan::selected(&step.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        if let Some(outcome) = outcomes.next() {
            let Action::Decide { arms, .. } = &step.action else {
                return None;
            };
            let arm = arms.iter().find(|a| a.outcome == outcome)?;
            if outcome == "matched" && !arm.slots.is_empty() {
                return None;
            }
            facts.insert(arm.atom);
            continue;
        }
        let Action::Exit(outcome, args) = &step.action else {
            return None;
        };
        let [Expr::WithReferenceEvidence(value, evidence)] = args.as_slice() else {
            return None;
        };
        if matches!(value.as_ref(), Expr::Reference(p, ty) if p.indirect && p.root == element && p.fields.is_empty() && ty == "&U32")
            && input_evidence(evidence)
        {
            return Some(Hit {
                source,
                evidence,
                outcome,
                site: step.id,
            });
        }
        return None;
    }
    None
}
