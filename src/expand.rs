use crate::{
    Diagnostic, Result, Span,
    certificate::{CallExpansion, ExpansionArtifact},
    ir::*,
};
use std::collections::{BTreeMap, BTreeSet};

fn guard(g: &Guard, context: &Guard, atoms: &BTreeMap<Id, Id>) -> Guard {
    match g {
        Guard::True => context.clone(),
        Guard::Atom(id) => context.clone().and(Guard::Atom(atoms[id])),
        Guard::And(a, b) => guard(a, context, atoms).and(guard(b, context, atoms)),
        Guard::Or(a, b) => Guard::Or(
            Box::new(guard(a, context, atoms)),
            Box::new(guard(b, context, atoms)),
        ),
    }
}
fn expr(e: &Expr, slots: &BTreeMap<Id, Id>) -> Expr {
    match e {
        Expr::Reference(p, ty) => Expr::Reference(
            Place {
                indirect: p.indirect,
                root: slots[&p.root],
                fields: p.fields.clone(),
            },
            ty.clone(),
        ),
        Expr::Length(e) => Expr::Length(Box::new(expr(e, slots))),
        Expr::Value(v) => Expr::Value(v.clone()),
        Expr::Pure(name, args) => {
            Expr::Pure(name.clone(), args.iter().map(|e| expr(e, slots)).collect())
        }
        Expr::Place(p) => Expr::Place(Place {
            indirect: p.indirect,
            root: slots[&p.root],
            fields: p.fields.clone(),
        }),
        Expr::Move(p) => Expr::Move(Place {
            indirect: p.indirect,
            root: slots[&p.root],
            fields: p.fields.clone(),
        }),
        Expr::Project(e, field) => Expr::Project(Box::new(expr(e, slots)), *field),
        Expr::Construct(t, n, args) => Expr::Construct(
            t.clone(),
            n.clone(),
            args.iter().map(|e| expr(e, slots)).collect(),
        ),
        Expr::Record(t, fields) => Expr::Record(
            t.clone(),
            fields.iter().map(|(i, e)| (*i, expr(e, slots))).collect(),
        ),
        Expr::Not(e) => Expr::Not(Box::new(expr(e, slots))),
        Expr::Binary(op, a, b) => Expr::Binary(
            op.clone(),
            Box::new(expr(a, slots)),
            Box::new(expr(b, slots)),
        ),
    }
}
struct Instance<'a> {
    slots: BTreeMap<Id, Id>,
    atoms: BTreeMap<Id, Id>,
    loops: BTreeMap<Id, Id>,
    context: &'a Guard,
    arms: &'a [Arm],
    calls: Vec<Id>,
    call_sites: Vec<String>,
}
struct Expander<'a> {
    program: &'a CheckedProgram,
    next: Id,
    output: Region,
    calls: Vec<CallExpansion>,
}
impl Expander<'_> {
    fn fresh(&mut self) -> Id {
        let id = self.next;
        self.next += 1;
        id
    }
    fn loop_ids(&mut self, code: &[Instruction], ids: &mut BTreeMap<Id, Id>) {
        for i in code {
            if let Action::Loop { id, body, .. } = &i.action {
                ids.insert(*id, self.fresh());
                self.loop_ids(body, ids);
            }
        }
    }
    fn copy(
        &mut self,
        original: &Instruction,
        instance: &Instance<'_>,
        inside_loop: Option<Id>,
        copied: &mut BTreeMap<Id, Id>,
    ) -> Instruction {
        let s = &instance.slots;
        let a = &instance.atoms;
        let action = match &original.action {
            Action::Exit(name, args) => {
                let arm = instance.arms.iter().find(|a| a.outcome == *name).unwrap();
                Action::Complete {
                    atom: arm.atom,
                    bindings: arm
                        .slots
                        .iter()
                        .copied()
                        .zip(args.iter().map(|e| expr(e, s)))
                        .collect(),
                    unwind: inside_loop,
                    release: s.values().copied().collect(),
                }
            }
            Action::Again { target, args } => Action::Again {
                target: instance.loops[target],
                args: args.iter().map(|e| expr(e, s)).collect(),
            },
            Action::Loop {
                id,
                parameters,
                local_slots,
                local_atoms,
                body,
            } => {
                let id = instance.loops[id];
                Action::Loop {
                    id,
                    parameters: parameters
                        .iter()
                        .map(|(id, e)| (s[id], expr(e, s)))
                        .collect(),
                    local_slots: local_slots.iter().map(|id| s[id]).collect(),
                    local_atoms: local_atoms.iter().map(|id| a[id]).collect(),
                    body: body
                        .iter()
                        .map(|i| self.copy(i, instance, Some(id), copied))
                        .collect(),
                }
            }
            Action::Bind(id, e) => Action::Bind(s[id], expr(e, s)),
            Action::Assign(p, e) => Action::Assign(
                Place {
                    indirect: p.indirect,
                    root: s[&p.root],
                    fields: p.fields.clone(),
                },
                expr(e, s),
            ),
            Action::Drop(e) => Action::Drop(expr(e, s)),
            Action::Release(ids) => Action::Release(ids.iter().map(|id| s[id]).collect()),
            Action::Emit(e) => Action::Emit(expr(e, s)),
            Action::Decide {
                operation,
                args,
                arms,
            } => Action::Decide {
                operation: match operation {
                    Operation::Variant(e) => Operation::Variant(expr(e, s)),
                    Operation::BorrowedVariant(e) => Operation::BorrowedVariant(expr(e, s)),
                    other => other.clone(),
                },
                args: args.iter().map(|e| expr(e, s)).collect(),
                arms: arms
                    .iter()
                    .map(|arm| Arm {
                        outcome: arm.outcome.clone(),
                        atom: a[&arm.atom],
                        slots: arm.slots.iter().map(|id| s[id]).collect(),
                    })
                    .collect(),
            },
            Action::Complete {
                atom,
                bindings,
                unwind,
                release,
            } => Action::Complete {
                atom: a[atom],
                bindings: bindings.iter().map(|(id, e)| (s[id], expr(e, s))).collect(),
                unwind: unwind.map(|id| instance.loops[&id]),
                release: release.iter().map(|id| s[id]).collect(),
            },
        };
        let mut provenance = original.provenance.clone();
        let mut calls = instance.calls.clone();
        calls.extend(provenance.calls);
        provenance.calls = calls;
        provenance
            .call_sites
            .splice(0..0, instance.call_sites.clone());
        let id = self.fresh();
        copied.insert(original.id, id);
        Instruction {
            id,
            guard: guard(&original.guard, instance.context, a),
            action,
            provenance,
        }
    }
    fn instruction(&mut self, mut instruction: Instruction) {
        if let Action::Loop {
            body,
            local_slots,
            local_atoms,
            ..
        } = &mut instruction.action
        {
            let old_slots: BTreeSet<_> = self.output.slots.keys().copied().collect();
            let old_atoms: BTreeSet<_> = self.output.atoms.keys().copied().collect();
            let enclosing = std::mem::take(&mut self.output.code);
            for i in std::mem::take(body) {
                self.instruction(i);
            }
            *body = std::mem::replace(&mut self.output.code, enclosing);
            local_slots.extend(
                self.output
                    .slots
                    .keys()
                    .filter(|id| !old_slots.contains(id))
                    .copied(),
            );
            local_atoms.extend(
                self.output
                    .atoms
                    .keys()
                    .filter(|id| !old_atoms.contains(id))
                    .copied(),
            );
        }
        if let Action::Decide {
            operation: Operation::Region(name),
            args,
            arms,
        } = &instruction.action
        {
            let callee = self.program.region(name).unwrap();
            let mut slots = BTreeMap::new();
            let mut atoms = BTreeMap::new();
            for slot in callee.slots.values() {
                let id = self.fresh();
                slots.insert(slot.id, id);
                self.output.slots.insert(
                    id,
                    Slot {
                        id,
                        name: slot.name.clone(),
                        ty: slot.ty.clone(),
                    },
                );
            }
            for (atom, name) in &callee.atoms {
                let id = self.fresh();
                atoms.insert(*atom, id);
                self.output.atoms.insert(id, name.clone());
            }
            let mut calls = instruction.provenance.calls.clone();
            calls.push(instruction.provenance.definition);
            let mut call_sites = instruction.provenance.call_sites.clone();
            call_sites.push(format!(
                "{}::{}#{}",
                instruction.provenance.region,
                instruction.provenance.site.as_deref().unwrap_or(name),
                instruction.provenance.definition
            ));
            let mut parameters = vec![];
            for (param, arg) in callee.parameters.iter().zip(args) {
                let id = self.fresh();
                parameters.push(id);
                self.output.code.push(Instruction {
                    id,
                    guard: instruction.guard.clone(),
                    action: Action::Bind(slots[&param.id], arg.clone()),
                    provenance: Provenance {
                        definition: instruction.provenance.definition,
                        region: instruction.provenance.region.clone(),
                        site: instruction.provenance.site.clone(),
                        call_sites: call_sites.clone(),
                        calls: calls.clone(),
                        span: instruction.provenance.span,
                    },
                });
            }
            let mut loops = BTreeMap::new();
            self.loop_ids(&callee.code, &mut loops);
            let instance = Instance {
                slots,
                atoms,
                loops,
                context: &instruction.guard,
                arms,
                calls,
                call_sites,
            };
            let mut instructions = BTreeMap::new();
            let copies = callee
                .code
                .iter()
                .map(|i| self.copy(i, &instance, None, &mut instructions))
                .collect::<Vec<_>>();
            self.calls.push(CallExpansion {
                call: instruction.id,
                callee: name.clone(),
                slots: instance.slots,
                atoms: instance.atoms,
                loops: instance.loops,
                instructions,
                parameters,
            });
            for copy in copies {
                self.instruction(copy);
            }
        } else {
            self.output.code.push(instruction);
        }
    }
}
impl CheckedProgram {
    /// Expand and independently validate the finite rewrite derivation.
    pub fn expanded(&self, entry: &str) -> Result<Self> {
        self.accept_expansion(&self.make_expansion_artifact(entry)?)
    }
    /// A portable candidate graph and its explicit rewrite correspondence.
    pub fn expansion_artifact(&self, entry: &str) -> Result<ExpansionArtifact> {
        let artifact = self.make_expansion_artifact(entry)?;
        self.accept_expansion(&artifact)?;
        Ok(artifact)
    }

    /// Inline acyclic calls, retaining loops as finite cyclic structures.
    fn make_expansion_artifact(&self, entry: &str) -> Result<ExpansionArtifact> {
        let region = self.region(entry).ok_or_else(|| {
            Diagnostic::new(
                "E_ENTRY",
                Span::default(),
                format!("unknown entry `{entry}`"),
            )
        })?;
        let mut output = region.clone();
        output.code.clear();
        let mut expander = Expander {
            program: self,
            next: self.next_id,
            output,
            calls: vec![],
        };
        for i in &region.code {
            expander.instruction(i.clone());
        }
        Ok(ExpansionArtifact {
            version: 1,
            entry: entry.into(),
            next_id: expander.next,
            region: expander.output,
            calls: expander.calls,
        })
    }
}
