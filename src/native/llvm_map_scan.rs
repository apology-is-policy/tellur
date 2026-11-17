//! Prove the successful path of an ordered traversal. References may be forwarded
//! between iteration locals but cannot escape the back edge. The exhausted path
//! and any iteration with insufficient fuel still execute the original checked IR.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueSource {
    Map(Id, bool),
    Position,
    Key,
    Element,
    Next,
}

#[derive(Clone, Copy)]
pub(super) struct Wrapper {
    ticks: usize,
}

// Only transparent synchronous wrappers that deliver the builtin's item in order.
pub(super) fn wrapper(region: &Region) -> Option<Wrapper> {
    let [map, position] = region.parameters.as_slice() else {
        return None;
    };
    let [decision, exits @ ..] = region.code.as_slice() else {
        return None;
    };
    let Action::Decide {
        operation: Operation::Collection(op),
        args,
        arms,
    } = &decision.action
    else {
        return None;
    };
    if region.asynchronous
        || region.boundary.is_some()
        || decision.guard != Guard::True
        || op != "map_entry_at"
        || args.len() != 2
        || !is_ref(&args[0], map.id)
        || !is_local(&args[1], position.id)
    {
        return None;
    }
    let item = arms.iter().find(|a| a.outcome == "item")?;
    let [key, value, next] = item.slots.as_slice() else {
        return None;
    };
    let facts = BTreeSet::from([item.atom]);
    for (i, inst) in exits.iter().enumerate() {
        if !inst.guard.evaluate(&facts) {
            continue;
        }
        let Action::Exit(out, payload) = &inst.action else {
            return None;
        };
        if out == "item"
            && payload.len() == 3
            && is_ref(&payload[0], *key)
            && is_ref(&payload[1], *value)
            && is_local(&payload[2], *next)
        {
            return Some(Wrapper { ticks: 2 + i });
        }
        return None;
    }
    None
}

fn is_ref(e: &Expr, id: Id) -> bool {
    matches!(e.without_reference_evidence(), Expr::Reference(p, _) if p.indirect && p.root==id && p.fields.is_empty())
}
fn is_local(e: &Expr, id: Id) -> bool {
    matches!(e, Expr::Place(p) if !p.indirect && p.root==id && p.fields.is_empty())
}
// Expanded calls can repeat an enclosing guard on each instruction. It is
// already true on entry. Iteration-local atoms are reset at the back edge.
pub(super) fn selected(
    g: &Guard,
    enclosing: &Guard,
    locals: &[Id],
    facts: &BTreeSet<Id>,
) -> Option<bool> {
    if g == enclosing {
        return Some(true);
    }
    match g {
        Guard::True => Some(true),
        Guard::Atom(a) if locals.contains(a) => Some(facts.contains(a)),
        Guard::And(l, r) => {
            Some(selected(l, enclosing, locals, facts)? && selected(r, enclosing, locals, facts)?)
        }
        Guard::Or(l, r) => {
            Some(selected(l, enclosing, locals, facts)? || selected(r, enclosing, locals, facts)?)
        }
        _ => None,
    }
}

pub(super) enum Traversal {
    Borrowed {
        map: Id,
        indirect: bool,
        wrapped: bool,
    },
    Consuming,
}
pub(super) struct Scan {
    pub kind: Traversal,
    pub ticks: usize,
    pub emits: Vec<u8>,
}

pub(super) fn analyze(inst: &Instruction, wrappers: &BTreeMap<String, Wrapper>) -> Option<Scan> {
    use ValueSource::*;
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
    let [(counter, _)] = parameters.as_slice() else {
        return None;
    };
    let mut sources = BTreeMap::from([(*counter, Position)]);
    let mut atoms = BTreeSet::new();
    let mut scan = None;
    let mut ticks = 0usize;
    let mut emits = Vec::new();
    let enclosing = &inst.guard;
    for inst in body {
        ticks += 1;
        if !selected(&inst.guard, enclosing, local_atoms, &atoms)? {
            continue;
        }
        let source = |e: &Expr| match e.without_reference_evidence() {
            Expr::Place(p) if !p.indirect && p.fields.is_empty() => sources.get(&p.root).copied(),
            Expr::Reference(p, ty) if p.fields.is_empty() => {
                if !local_slots.contains(&p.root) && ty.starts_with("&map<") {
                    Some(Map(p.root, p.indirect))
                } else if p.indirect {
                    sources.get(&p.root).copied()
                } else {
                    None
                }
            }
            _ => None,
        };
        match &inst.action {
            Action::Bind(slot, e) if local_slots.contains(slot) => {
                let v = source(e)?;
                sources.insert(*slot, v);
            }
            Action::Decide {
                operation,
                args,
                arms,
            } if scan.is_none() => {
                let kind = match (operation, args.as_slice()) {
                    (Operation::Collection(name), [Expr::Move(p)])
                        if name == "map_take"
                            && p.root == *counter
                            && !p.indirect
                            && p.fields.is_empty() =>
                    {
                        Traversal::Consuming
                    }
                    (op, [map, pos]) if source(pos)? == Position => {
                        let wrapped = match op {
                            Operation::Collection(name) if name == "map_entry_at" => false,
                            Operation::Region(name) => {
                                ticks += wrappers.get(name)?.ticks;
                                true
                            }
                            _ => return None,
                        };
                        let Map(map, indirect) = source(map)? else {
                            return None;
                        };
                        Traversal::Borrowed {
                            map,
                            indirect,
                            wrapped,
                        }
                    }
                    _ => return None,
                };
                let item = arms.iter().find(|a| a.outcome == "item")?;
                let [key, value, next] = item.slots.as_slice() else {
                    return None;
                };
                if !item.slots.iter().all(|s| local_slots.contains(s))
                    || !local_atoms.contains(&item.atom)
                {
                    return None;
                }
                sources.extend([(*key, Key), (*value, Element), (*next, Next)]);
                atoms.insert(item.atom);
                scan = Some(kind);
            }
            Action::Complete {
                atom,
                bindings,
                unwind: None,
                release,
            } if local_atoms.contains(atom) => {
                if !release.iter().all(|s| local_slots.contains(s)) {
                    return None;
                }
                let values = bindings
                    .iter()
                    .map(|(s, e)| Some((*s, source(e)?)))
                    .collect::<Option<Vec<_>>>()?;
                if !values.iter().all(|(s, _)| local_slots.contains(s)) {
                    return None;
                }
                sources.extend(values);
                for s in release {
                    sources.remove(s);
                }
                atoms.insert(*atom);
            }
            Action::Emit(e) => {
                // 0/1 select the key/value, bit 1 requests u32_text conversion.
                let (read, format) = match e {
                    Expr::Pure(name, args) if name == "u32_text" && args.len() == 1 => {
                        (&args[0], 2)
                    }
                    _ => (e, 0),
                };
                let p = match (read, scan.as_ref()?) {
                    (Expr::Place(p), Traversal::Borrowed { .. }) if p.indirect => p,
                    (Expr::Place(p) | Expr::Move(p), Traversal::Consuming) if !p.indirect => p,
                    _ => return None,
                };
                if !p.fields.is_empty() {
                    return None;
                }
                emits.push(
                    format
                        + match sources.get(&p.root)? {
                            Key => 0,
                            Element => 1,
                            _ => return None,
                        },
                );
            }
            Action::Again { target, args } if target == id && args.len() == 1 => {
                let kind = scan?;
                let next = match (&kind, &args[0]) {
                    (Traversal::Consuming, Expr::Move(p)) if !p.indirect && p.fields.is_empty() => {
                        sources.get(&p.root).copied()
                    }
                    (Traversal::Borrowed { .. }, e) => source(e),
                    _ => None,
                };
                if next != Some(Next) {
                    return None;
                }
                return Some(Scan { kind, ticks, emits });
            }
            _ => return None,
        }
    }
    None
}
