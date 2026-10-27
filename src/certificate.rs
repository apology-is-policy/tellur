//! Translation validation for one narrow rewrite: acyclic control-call expansion.
//! The checker consumes an explicit correspondence and never calls the expander.
//! Acceptance establishes conformance to these rewrite rules, not a mechanized
//! soundness proof of the rules, front end, foreign adapters, or native backend.
use crate::{
    Diagnostic, Result, Span,
    bdd::{Bdd, Formula, TRUE},
    ir::*,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallExpansion {
    /// Instruction being replaced, including transient copies of nested calls.
    pub call: Id,
    pub callee: String,
    pub slots: BTreeMap<Id, Id>,
    pub atoms: BTreeMap<Id, Id>,
    pub loops: BTreeMap<Id, Id>,
    pub instructions: BTreeMap<Id, Id>,
    /// One binding instruction per callee parameter, in declaration order.
    pub parameters: Vec<Id>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpansionArtifact {
    pub version: u32,
    pub entry: String,
    pub next_id: Id,
    pub region: Region,
    pub calls: Vec<CallExpansion>,
}
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_EXPANSION_CERTIFICATE", Span::default(), message)
}
fn ids(code: &[Instruction], instructions: &mut BTreeSet<Id>, loops: &mut BTreeSet<Id>) {
    for i in code {
        instructions.insert(i.id);
        if let Action::Loop { id, body, .. } = &i.action {
            loops.insert(*id);
            ids(body, instructions, loops);
        }
    }
}
fn keys(map: &BTreeMap<Id, Id>, expected: impl Iterator<Item = Id>) -> Result<()> {
    if map.keys().copied().collect::<BTreeSet<_>>() != expected.collect() {
        return Err(error(
            "correspondence has missing or extra source identities",
        ));
    }
    Ok(())
}
// This substitution is local to the checker. Sharing the expander's remapping
// functions would make their bugs invisible to translation validation.
fn place(p: &Place, slots: &BTreeMap<Id, Id>) -> Place {
    Place {
        root: slots[&p.root],
        indirect: p.indirect,
        fields: p.fields.clone(),
    }
}
fn substitute(e: &Expr, slots: &BTreeMap<Id, Id>) -> Expr {
    let sub = |e: &Expr| substitute(e, slots);
    match e {
        Expr::Value(v) => Expr::Value(v.clone()),
        Expr::Place(p) => Expr::Place(place(p, slots)),
        Expr::Move(p) => Expr::Move(place(p, slots)),
        Expr::Reference(p, t) => Expr::Reference(place(p, slots), t.clone()),
        Expr::Project(e, i) => Expr::Project(Box::new(sub(e)), *i),
        Expr::Construct(t, n, args) => {
            Expr::Construct(t.clone(), n.clone(), args.iter().map(sub).collect())
        }
        Expr::Pure(n, args) => Expr::Pure(n.clone(), args.iter().map(sub).collect()),
        Expr::Length(e) => Expr::Length(Box::new(sub(e))),
        Expr::Record(t, fields) => Expr::Record(
            t.clone(),
            fields.iter().map(|(i, e)| (*i, sub(e))).collect(),
        ),
        Expr::Not(e) => Expr::Not(Box::new(sub(e))),
        Expr::Binary(op, a, b) => Expr::Binary(op.clone(), Box::new(sub(a)), Box::new(sub(b))),
    }
}
fn renamed(g: &Guard, atoms: &BTreeMap<Id, Id>) -> Guard {
    match g {
        Guard::True => Guard::True,
        Guard::Atom(a) => Guard::Atom(atoms[a]),
        Guard::And(a, b) => renamed(a, atoms).and(renamed(b, atoms)),
        Guard::Or(a, b) => Guard::Or(Box::new(renamed(a, atoms)), Box::new(renamed(b, atoms))),
    }
}
struct Boundary<'a> {
    call: &'a Instruction,
    arms: &'a [Arm],
    map: &'a CallExpansion,
    calls: Vec<Id>,
    sites: Vec<String>,
}
impl Boundary<'_> {
    fn instruction(&self, i: &Instruction, enclosing_loop: Option<Id>) -> Result<Instruction> {
        let m = self.map;
        let sub = |e: &Expr| substitute(e, &m.slots);
        let action = match &i.action {
            Action::Exit(name, args) => {
                let arm = self
                    .arms
                    .iter()
                    .find(|a| a.outcome == *name)
                    .ok_or_else(|| error("exit has no caller continuation"))?;
                if arm.slots.len() != args.len() {
                    return Err(error("completion payload arity differs"));
                }
                Action::Complete {
                    atom: arm.atom,
                    bindings: arm
                        .slots
                        .iter()
                        .copied()
                        .zip(args.iter().map(sub))
                        .collect(),
                    unwind: enclosing_loop,
                    release: m.slots.values().copied().collect(),
                }
            }
            Action::Loop {
                id,
                parameters,
                local_slots,
                local_atoms,
                body,
            } => Action::Loop {
                id: m.loops[id],
                parameters: parameters
                    .iter()
                    .map(|(id, e)| (m.slots[id], sub(e)))
                    .collect(),
                local_slots: local_slots.iter().map(|id| m.slots[id]).collect(),
                local_atoms: local_atoms.iter().map(|id| m.atoms[id]).collect(),
                body: body
                    .iter()
                    .map(|i| self.instruction(i, Some(m.loops[id])))
                    .collect::<Result<_>>()?,
            },
            Action::Again { target, args } => Action::Again {
                target: m.loops[target],
                args: args.iter().map(sub).collect(),
            },
            Action::Bind(id, e) => Action::Bind(m.slots[id], sub(e)),
            Action::Assign(p, e) => Action::Assign(place(p, &m.slots), sub(e)),
            Action::Drop(e) => Action::Drop(sub(e)),
            Action::Emit(e) => Action::Emit(sub(e)),
            Action::Release(ids) => Action::Release(ids.iter().map(|id| m.slots[id]).collect()),
            Action::Decide {
                operation,
                args,
                arms,
            } => Action::Decide {
                operation: match operation {
                    Operation::Variant(e) => Operation::Variant(sub(e)),
                    Operation::BorrowedVariant(e) => Operation::BorrowedVariant(sub(e)),
                    other => other.clone(),
                },
                args: args.iter().map(sub).collect(),
                arms: arms
                    .iter()
                    .map(|a| Arm {
                        outcome: a.outcome.clone(),
                        atom: m.atoms[&a.atom],
                        slots: a.slots.iter().map(|id| m.slots[id]).collect(),
                    })
                    .collect(),
            },
            Action::Complete {
                atom,
                bindings,
                unwind,
                release,
            } => Action::Complete {
                atom: m.atoms[atom],
                bindings: bindings
                    .iter()
                    .map(|(id, e)| (m.slots[id], sub(e)))
                    .collect(),
                unwind: unwind.map(|id| m.loops[&id]),
                release: release.iter().map(|id| m.slots[id]).collect(),
            },
        };
        let mut provenance = i.provenance.clone();
        provenance.calls = self
            .calls
            .iter()
            .chain(&i.provenance.calls)
            .copied()
            .collect();
        provenance.call_sites = self
            .sites
            .iter()
            .chain(&i.provenance.call_sites)
            .cloned()
            .collect();
        Ok(Instruction {
            id: m.instructions[&i.id],
            action,
            provenance,
            // The producer distributes context over the guard. Here we factor
            // it once, then compare the Boolean functions using canonical BDDs.
            guard: self.call.guard.clone().and(renamed(&i.guard, &m.atoms)),
        })
    }
}
struct Validator<'a> {
    source: &'a CheckedProgram,
    records: BTreeMap<Id, &'a CallExpansion>,
    used: BTreeSet<Id>,
    slots: BTreeMap<Id, Slot>,
    atoms: BTreeMap<Id, String>,
}
impl Validator<'_> {
    fn sequence(&mut self, code: &[Instruction]) -> Result<Vec<Instruction>> {
        let mut output = vec![];
        for original in code {
            let mut i = original.clone();
            match &mut i.action {
                Action::Loop {
                    body,
                    local_slots,
                    local_atoms,
                    ..
                } => {
                    let slots = self.slots.keys().copied().collect::<BTreeSet<_>>();
                    let atoms = self.atoms.keys().copied().collect::<BTreeSet<_>>();
                    *body = self.sequence(body)?;
                    local_slots.extend(self.slots.keys().filter(|id| !slots.contains(id)).copied());
                    local_atoms.extend(self.atoms.keys().filter(|id| !atoms.contains(id)).copied());
                    output.push(i);
                }
                Action::Decide {
                    operation: Operation::Region(name),
                    args,
                    arms,
                } => {
                    if !self.used.insert(i.id) {
                        return Err(error("call correspondence reused"));
                    }
                    let record = *self
                        .records
                        .get(&i.id)
                        .ok_or_else(|| error(format!("missing expansion of call #{}", i.id)))?;
                    if record.callee != *name {
                        return Err(error("callee substitution differs"));
                    }
                    let callee = self
                        .source
                        .region(name)
                        .ok_or_else(|| error("unknown callee"))?;
                    for slot in callee.slots.values() {
                        let id = record.slots[&slot.id];
                        self.slots.insert(
                            id,
                            Slot {
                                id,
                                name: slot.name.clone(),
                                ty: slot.ty.clone(),
                            },
                        );
                    }
                    for (id, name) in &callee.atoms {
                        self.atoms.insert(record.atoms[id], name.clone());
                    }
                    let calls = i
                        .provenance
                        .calls
                        .iter()
                        .copied()
                        .chain([i.provenance.definition])
                        .collect::<Vec<_>>();
                    let mut sites = i.provenance.call_sites.clone();
                    sites.push(format!(
                        "{}::{}#{}",
                        i.provenance.region,
                        i.provenance.site.as_deref().unwrap_or(name),
                        i.provenance.definition
                    ));
                    if args.len() != callee.parameters.len() {
                        return Err(error("call argument arity differs"));
                    }
                    for ((parameter, arg), id) in callee
                        .parameters
                        .iter()
                        .zip(args.iter())
                        .zip(&record.parameters)
                    {
                        let mut provenance = i.provenance.clone();
                        provenance.calls = calls.clone();
                        provenance.call_sites = sites.clone();
                        output.push(Instruction {
                            id: *id,
                            guard: i.guard.clone(),
                            action: Action::Bind(record.slots[&parameter.id], arg.clone()),
                            provenance,
                        });
                    }
                    let boundary = Boundary {
                        call: original,
                        arms,
                        map: record,
                        calls,
                        sites,
                    };
                    let substituted = callee
                        .code
                        .iter()
                        .map(|i| boundary.instruction(i, None))
                        .collect::<Result<Vec<_>>>()?;
                    output.extend(self.sequence(&substituted)?);
                }
                _ => output.push(i),
            }
        }
        Ok(output)
    }
}
fn formula(g: &Guard, b: &mut Bdd) -> Formula {
    match g {
        Guard::True => TRUE,
        Guard::Atom(id) => b.variable(*id),
        Guard::And(a, c) => {
            let a = formula(a, b);
            let c = formula(c, b);
            b.and(a, c)
        }
        Guard::Or(a, c) => {
            let a = formula(a, b);
            let c = formula(c, b);
            b.or(a, c)
        }
    }
}
fn canonical_guards(code: &mut [Instruction], b: &mut Bdd) {
    for i in code {
        i.guard = Guard::Atom(formula(&i.guard, b));
        if let Action::Loop { body, .. } = &mut i.action {
            canonical_guards(body, b);
        }
    }
}
impl CheckedProgram {
    /// Validate a candidate against this checked source graph. An artifact alone
    /// never grants CheckedProgram status, and the producer is not called here.
    pub fn accept_expansion(&self, artifact: &ExpansionArtifact) -> Result<Self> {
        if artifact.version != 1 {
            return Err(error("unsupported expansion artifact version"));
        }
        self.verify()?;
        let root = self
            .region(&artifact.entry)
            .ok_or_else(|| error("unknown source entry"))?;
        let mut records = BTreeMap::new();
        let mut fresh = BTreeSet::new();
        for r in &artifact.calls {
            if records.insert(r.call, r).is_some() {
                return Err(error("duplicate call correspondence"));
            }
            let callee = self
                .region(&r.callee)
                .ok_or_else(|| error("unknown correspondence callee"))?;
            keys(&r.slots, callee.slots.keys().copied())?;
            keys(&r.atoms, callee.atoms.keys().copied())?;
            let (mut instructions, mut loops) = (BTreeSet::new(), BTreeSet::new());
            ids(&callee.code, &mut instructions, &mut loops);
            keys(&r.instructions, instructions.into_iter())?;
            keys(&r.loops, loops.into_iter())?;
            if r.parameters.len() != callee.parameters.len() {
                return Err(error("parameter binding correspondence differs"));
            }
            for id in r
                .slots
                .values()
                .chain(r.atoms.values())
                .chain(r.loops.values())
                .chain(r.instructions.values())
                .chain(&r.parameters)
            {
                if *id < self.next_id || *id >= artifact.next_id || !fresh.insert(*id) {
                    return Err(error(
                        "fresh identities overlap or lie outside the allocation interval",
                    ));
                }
            }
        }
        if artifact.next_id.checked_sub(self.next_id) != Some(fresh.len()) {
            return Err(error("allocation interval contains unaccounted identities"));
        }
        let mut v = Validator {
            source: self,
            records,
            used: BTreeSet::new(),
            slots: root.slots.clone(),
            atoms: root.atoms.clone(),
        };
        let mut expected = root.clone();
        expected.code = v.sequence(&root.code)?;
        if v.used.len() != v.records.len() {
            return Err(error("unused call correspondence"));
        }
        expected.slots = v.slots;
        expected.atoms = v.atoms;
        let mut candidate = artifact.region.clone();
        let mut bdd = Bdd::default();
        canonical_guards(&mut expected.code, &mut bdd);
        canonical_guards(&mut candidate.code, &mut bdd);
        if candidate != expected {
            return Err(error(
                "candidate graph differs from the certified rewrite (boundary, order, guards, payloads, loops, release or provenance)",
            ));
        }
        let mut result = self.clone();
        result
            .regions
            .insert(artifact.entry.clone(), artifact.region.clone());
        result.next_id = artifact.next_id;
        result.verify()?;
        Ok(result)
    }
}
