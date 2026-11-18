//! Prove each effect-free advancing binary-search path. The empty/terminal
//! iteration remains in the checked graph, including its effects and faults.
use super::*;

fn local(expr: &Expr, id: Id) -> bool {
    matches!(expr, Expr::Place(p) if !p.indirect && p.fields.is_empty() && p.root == id)
}
fn borrowed(expr: &Expr) -> Option<Id> {
    match expr.without_reference_evidence() {
        Expr::Reference(p, _) if p.indirect && p.fields.is_empty() => Some(p.root),
        _ => None,
    }
}

pub(super) fn comparator(region: &Region) -> Option<bool> {
    let [_, left, right] = region.parameters.as_slice() else {
        return None;
    };
    if left.ty != "&U32"
        || right.ty != "&U32"
        || region.asynchronous
        || region.boundary.is_some()
        || region.contingent
        || region.contingency
    {
        return None;
    }
    for descending in [false, true] {
        let correct = [-1, 0, 1].into_iter().all(|ordering| {
            let mut facts = BTreeSet::new();
            for inst in &region.code {
                if !inst.guard.evaluate(&facts) { continue; }
                match &inst.action {
                    Action::Decide { operation: Operation::Variant(Expr::Binary(op,a,b)), args, arms } if args.is_empty() => {
                        let read = |e: &Expr, id| matches!(e, Expr::Place(p) if p.indirect && p.root == id && p.fields.is_empty());
                        if !read(a,left.id) || !read(b,right.id) { return false; }
                        let yes = match op.as_str() {
                            "<" => ordering < 0, ">" => ordering > 0, "==" => ordering == 0,
                            "<=" => ordering <= 0, ">=" => ordering >= 0, "!=" => ordering != 0,
                            _ => return false,
                        };
                        let Some(arm) = arms.iter().find(|a| a.outcome == if yes {"true"} else {"false"} && a.slots.is_empty()) else {return false};
                        facts.insert(arm.atom);
                    }
                    Action::Exit(out, values) => {
                        let order = if descending { -ordering } else { ordering };
                        return values.is_empty() && out == match order {-1=>"before",0=>"equal",_=>"after"};
                    }
                    _ => return false,
                }
            }
            false
        });
        if correct {
            return Some(descending);
        }
    }
    None
}

#[derive(PartialEq, Eq)]
pub(super) struct Bounds {
    pub vector: Id,
    pub rule: Id,
    pub needle: Id,
    pub upper: Id,
    pub descending: bool,
}

pub(super) fn analyze(inst: &Instruction, comparators: &BTreeMap<String, bool>) -> Option<Bounds> {
    let first = path(inst, comparators, "before", false)?;
    for outcome in ["before", "equal", "after"] {
        for upper in [false, true] {
            if path(inst, comparators, outcome, upper)? != first {
                return None;
            }
        }
    }
    Some(first)
}

fn path(
    inst: &Instruction,
    comparators: &BTreeMap<String, bool>,
    outcome: &str,
    upper_value: bool,
) -> Option<Bounds> {
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
    let [(start, _), (count, _)] = parameters.as_slice() else {
        return None;
    };
    let mut facts = BTreeSet::new();
    let (mut checked, mut half, mut middle, mut element, mut next, mut tail, mut rest) =
        (false, None, None, None, None, None, None);
    let (mut vector, mut rule, mut needle, mut descending, mut upper) =
        (None, None, None, None, None);
    // The upper flag is used only on equality, so resolve its identity from the
    // loop's boolean decision and still prove that decision on both equal paths.
    for step in body {
        if let Action::Decide {
            operation: Operation::Variant(Expr::Place(p)),
            args,
            ..
        } = &step.action
            && !p.indirect
            && p.fields.is_empty()
            && args.is_empty()
            && !local_slots.contains(&p.root)
        {
            if upper.replace(p.root).is_some() {
                return None;
            }
        }
    }
    for step in body {
        if !map_scan::selected(&step.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        let arm = match &step.action {
            Action::Decide {
                operation: Operation::Variant(Expr::Binary(op, a, b)),
                args,
                arms,
            } if !checked
                && op == "=="
                && args.is_empty()
                && local(a, *count)
                && matches!(b.as_ref(), Expr::Value(Value::U32(0))) =>
            {
                checked = true;
                let arm = arms
                    .iter()
                    .find(|a| a.outcome == "false" && a.slots.is_empty())?;
                arm
            }
            Action::Decide {
                operation: Operation::Primitive(name),
                args,
                arms,
            } if checked => {
                let [a, b] = args.as_slice() else { return None };
                let result = match name.as_str() {
                    "checked_div"
                        if half.is_none()
                            && local(a, *count)
                            && matches!(b, Expr::Value(Value::U32(2))) =>
                    {
                        "quotient"
                    }
                    "checked_add"
                        if half.is_some()
                            && middle.is_none()
                            && local(a, *start)
                            && local(b, half?) =>
                    {
                        "sum"
                    }
                    "checked_add"
                        if element.is_some()
                            && descending.is_some()
                            && next.is_none()
                            && local(a, middle?)
                            && matches!(b, Expr::Value(Value::U32(1))) =>
                    {
                        "sum"
                    }
                    "checked_sub"
                        if next.is_some()
                            && tail.is_none()
                            && local(a, *count)
                            && local(b, half?) =>
                    {
                        "difference"
                    }
                    "checked_sub"
                        if tail.is_some()
                            && rest.is_none()
                            && local(a, tail?)
                            && matches!(b, Expr::Value(Value::U32(1))) =>
                    {
                        "difference"
                    }
                    _ => return None,
                };
                let arm = arms.iter().find(|a| a.outcome == result)?;
                let [value] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(value) {
                    return None;
                }
                if half.is_none() {
                    half = Some(*value)
                } else if middle.is_none() {
                    middle = Some(*value)
                } else if next.is_none() {
                    next = Some(*value)
                } else if tail.is_none() {
                    tail = Some(*value)
                } else {
                    rest = Some(*value)
                }
                arm
            }
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_get" && middle.is_some() && element.is_none() => {
                let [source, index] = args.as_slice() else {
                    return None;
                };
                if !local(index, middle?) {
                    return None;
                }
                let source = borrowed(source)?;
                if local_slots.contains(&source) {
                    return None;
                }
                vector = Some(source);
                let arm = arms.iter().find(|a| a.outcome == "found")?;
                let [value] = arm.slots.as_slice() else {
                    return None;
                };
                if !local_slots.contains(value) {
                    return None;
                }
                element = Some(*value);
                arm
            }
            Action::Decide {
                operation: Operation::Region(name),
                args,
                arms,
            } if element.is_some() && descending.is_none() => {
                let [r, a, b] = args.as_slice() else {
                    return None;
                };
                let (r, b) = (borrowed(r)?, borrowed(b)?);
                if borrowed(a)? != element? || local_slots.contains(&r) || local_slots.contains(&b)
                {
                    return None;
                }
                rule = Some(r);
                needle = Some(b);
                descending = Some(*comparators.get(name)?);
                arms.iter()
                    .find(|a| a.outcome == outcome && a.slots.is_empty())?
            }
            Action::Decide {
                operation: Operation::Variant(value),
                args,
                arms,
            } if descending.is_some()
                && outcome == "equal"
                && args.is_empty()
                && local(value, upper?) =>
            {
                arms.iter().find(|a| {
                    a.outcome == if upper_value { "true" } else { "false" } && a.slots.is_empty()
                })?
            }
            Action::Again { target, args } if target == id && descending.is_some() => {
                let [a, b] = args.as_slice() else { return None };
                let advances = outcome == "before" || (outcome == "equal" && upper_value);
                if advances {
                    if !local(a, next?) || !local(b, rest?) {
                        return None;
                    }
                } else if next.is_some() || !local(a, *start) || !local(b, half?) {
                    return None;
                }
                return Some(Bounds {
                    vector: vector?,
                    rule: rule?,
                    needle: needle?,
                    upper: upper?,
                    descending: descending?,
                });
            }
            _ => return None,
        };
        if !local_atoms.contains(&arm.atom) {
            return None;
        }
        facts.insert(arm.atom);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_proof_requires_every_advancing_branch_and_comparison() {
        let comparator = include_str!("../../experiments/ordered_bounds/lower_bound.vn")
            .split_once("outcome result")
            .unwrap()
            .0
            .lines()
            .filter(|s| !s.starts_with("import "))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("ordering.comparator", "comparator");
        let interface = include_str!("../../library/ordering.vn")
            .split_once("pub control")
            .unwrap()
            .0
            .lines()
            .filter(|s| !s.starts_with("import "))
            .collect::<Vec<_>>()
            .join("\n");
        let library = include_str!("../../library/ordered_search.vn")
            .lines()
            .filter(|s| !s.starts_with("import "))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("ordering.comparator", "comparator");
        let manifest = crate::manifest::Manifest::from_json(include_str!(
            "../../examples/ordered_search/manifest.json"
        ))
        .unwrap();
        for mode in [
            "plain",
            "descending",
            "renamed",
            "compare_effect",
            "left_effect",
            "right_effect",
            "wrong_shrink",
            "wrong_middle",
            "terminal_effect",
        ] {
            let mut text = format!(
                "{interface}\n{comparator}\n{library}\ncontrol main(ref vector<u32> source,ref ascending rule,ref u32 needle,bool upper) exits position {{u32 index}} {{forward bound<ascending>(source,rule,needle,upper);}}"
            );
            match mode {
                "descending" => {
                    text = text.replace("* a < * b", "* a > * b").replace(
                        "* a > * b) {\n            true => after",
                        "* a < * b) {\n            true => after",
                    )
                }
                "renamed" => {
                    text = text
                        .replace("bound<", "renamed<")
                        .replace("search", "walk")
                        .replace("compare", "comparison")
                }
                "compare_effect" => {
                    text = text.replace(
                        "delta (* a < * b)",
                        "emit(\"comparison\"); delta (* a < * b)",
                    )
                }
                "left_effect" => {
                    text = text.replace(
                        "[greater | equal_low]: again",
                        "[greater | equal_low]: emit(\"left\"); again",
                    )
                }
                "right_effect" => {
                    text = text.replace("[advanced]: again", "[advanced]: emit(\"right\"); again")
                }
                "wrong_shrink" => {
                    text = text.replace("checked_sub(tail, 1)", "checked_sub(tail, 0)")
                }
                "wrong_middle" => {
                    text = text.replace("checked_add(start, half)", "checked_add(start, count)")
                }
                "terminal_effect" => text = text
                    .replace("true => exit position { start }", "true => finished")
                    .replace(
                        "[halve]:",
                        "[finished]: emit(\"finished\"); exit position {start};\n        [halve]:",
                    ),
                _ => {}
            }
            let program =
                crate::compile(&text, manifest.clone()).unwrap_or_else(|e| panic!("{mode}: {e:?}"));
            let comparators = program
                .regions()
                .filter_map(|r| Some((r.name.clone(), super::comparator(r)?)))
                .collect();
            let plans = program
                .regions()
                .flat_map(|r| &r.code)
                .filter_map(|i| analyze(i, &comparators))
                .collect::<Vec<_>>();
            assert_eq!(
                plans.len(),
                usize::from(matches!(
                    mode,
                    "plain" | "descending" | "renamed" | "terminal_effect"
                )),
                "{mode}"
            );
            if mode == "descending" {
                assert!(plans[0].descending);
            }
        }
    }
}
