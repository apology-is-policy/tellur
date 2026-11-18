//! Prove a whole consuming join, including its empty-source terminal path.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Join {
    pub source: usize,
    pub destination: usize,
    pub outcome: String,
}
fn moved(expr: &Expr) -> Option<Id> {
    let Expr::Move(p) = expr else { return None };
    (!p.indirect && p.fields.is_empty()).then_some(p.root)
}
pub(super) fn analyze(region: &Region) -> Option<Join> {
    let [left, right] = region.parameters.as_slice() else {
        return None;
    };
    let [exit] = region.exits.as_slice() else {
        return None;
    };
    if region.asynchronous
        || region.boundary.is_some()
        || region.contingency
        || left.ty != right.ty
        || !left.ty.starts_with("vector<")
        || exit.fields.len() != 1
        || exit.fields[0].ty != left.ty
        || !region.borrow_returns.is_empty()
        || !region.stored_returns.is_empty()
    {
        return None;
    }
    let [inst] = region.code.as_slice() else {
        return None;
    };
    if inst.guard != Guard::True {
        return None;
    }
    let Action::Loop {
        supervised: false,
        parameters,
        local_atoms,
        body,
        ..
    } = &inst.action
    else {
        return None;
    };
    let transfer = vector_transfer::analyze(inst)?;
    if transfer.limit.is_some() {
        return None;
    }
    let argument = |index: usize| {
        let slot = moved(&parameters[index].1)?;
        region.parameters.iter().position(|p| p.id == slot)
    };
    let source = argument(transfer.source)?;
    let destination = argument(transfer.destination)?;
    if source == destination {
        return None;
    }
    let mut facts = BTreeSet::new();
    let mut exhausted = false;
    for step in body {
        if !map_scan::selected(&step.guard, &inst.guard, local_atoms, &facts)? {
            continue;
        }
        match &step.action {
            Action::Decide {
                operation: Operation::Collection(name),
                args,
                arms,
            } if name == "vector_take" && !exhausted => {
                let [value] = args.as_slice() else {
                    return None;
                };
                if moved(value)? != parameters[transfer.source].0 || arms.len() != 2 {
                    return None;
                }
                let empty = arms
                    .iter()
                    .find(|a| a.outcome == "empty" && a.slots.is_empty())?;
                if !local_atoms.contains(&empty.atom) {
                    return None;
                }
                facts.insert(empty.atom);
                exhausted = true;
            }
            Action::Exit(name, args) if exhausted && name == &exit.name => {
                let [value] = args.as_slice() else {
                    return None;
                };
                if moved(value)? != parameters[transfer.destination].0 {
                    return None;
                }
                return Some(Join {
                    source,
                    destination,
                    outcome: name.clone(),
                });
            }
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_join_requires_effect_free_entry_advancement_and_terminal_path() {
        let source = include_str!("../../library/sequence_batches.vn");
        let source = format!(
            "control join{}",
            source.split_once("pub control join<item>").unwrap().1
        )
        .replace("vector<item>", "vector<u32>")
        .replace("item next", "u32 next");
        for (mode, source, expected) in [
            ("plain", source.clone(), true),
            (
                "renamed",
                source.replace("control join", "control renamed"),
                true,
            ),
            (
                "entry_effect",
                source.replace("    loop append", "    emit(\"entry\");\n    loop append"),
                false,
            ),
            (
                "advance_effect",
                source.replace("[got]: delta", "[got]: emit(\"item\"); delta"),
                false,
            ),
            (
                "terminal_effect",
                source
                    .replace("empty => exit joined { result }", "empty => ended")
                    .replace(
                        "[got]:",
                        "[ended]: emit(\"empty\"); exit joined { result };\n        [got]:",
                    ),
                false,
            ),
        ] {
            let manifest = crate::manifest::Manifest::from_json(
                r#"{"version":"0.1","types":[],"primitives":[]}"#,
            )
            .unwrap();
            let checked = crate::compile(&source, manifest).unwrap();
            let region = checked
                .region(if mode == "renamed" { "renamed" } else { "join" })
                .unwrap();
            assert_eq!(analyze(region).is_some(), expected, "{mode}: {region:#?}");
            if let Some(join) = analyze(region) {
                assert_eq!(
                    (join.destination, join.source, join.outcome.as_str()),
                    (0, 1, "joined")
                );
            }
        }
    }
}
