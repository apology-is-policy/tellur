//! Diagnostic-only lineage over the checked prefix. This never changes typing,
//! exhaustiveness, IR, or execution. Guards retain disjoint writes; tags and field
//! trees keep unrelated aggregate payloads separate. Calls summarize result tags,
//! not argument-to-payload dependencies. Loop-carried and indirect storage are
//! deliberately forgotten rather than reported with stale lineage.
use super::*;
use crate::DiagnosticNote;

type Notes = Vec<(Formula, DiagnosticNote)>;
#[derive(Clone, Default)]
struct ValueTrace {
    tags: BTreeMap<String, TagTrace>,
    fields: BTreeMap<usize, ValueTrace>,
}
#[derive(Clone, Default)]
struct TagTrace {
    notes: Notes,
    fields: BTreeMap<usize, ValueTrace>,
}

fn add(notes: &mut Notes, domain: Formula, note: DiagnosticNote, bdd: &mut Bdd) {
    if domain == FALSE {
        return;
    }
    if let Some((previous, _)) = notes.iter_mut().find(|(_, n)| *n == note) {
        *previous = bdd.or(*previous, domain);
        return;
    }
    let omission = notes
        .iter()
        .position(|(_, n)| n.message == contracts::OMITTED);
    if note.message != contracts::OMITTED
        && notes.len() - usize::from(omission.is_some()) < contracts::NOTE_LIMIT
    {
        notes.insert(omission.unwrap_or(notes.len()), (domain, note));
    } else if let Some(index) = omission {
        notes[index].0 = bdd.or(notes[index].0, domain);
    } else {
        notes.push((
            domain,
            DiagnosticNote {
                message: contracts::OMITTED.into(),
                ..note
            },
        ));
    }
}

fn restrict_notes(notes: &mut Notes, domain: Formula, bdd: &mut Bdd) {
    for (selected, _) in notes.iter_mut() {
        *selected = bdd.and(*selected, domain);
    }
    notes.retain(|(selected, _)| *selected != FALSE);
}
impl ValueTrace {
    fn restrict(&mut self, domain: Formula, bdd: &mut Bdd) {
        for tag in self.tags.values_mut() {
            restrict_notes(&mut tag.notes, domain, bdd);
            for field in tag.fields.values_mut() {
                field.restrict(domain, bdd);
            }
        }
        for field in self.fields.values_mut() {
            field.restrict(domain, bdd);
        }
    }
    fn merge(&mut self, other: Self, bdd: &mut Bdd) {
        for (name, tag) in other.tags {
            let target = self.tags.entry(name).or_default();
            for (domain, note) in tag.notes {
                add(&mut target.notes, domain, note, bdd);
            }
            for (index, field) in tag.fields {
                target.fields.entry(index).or_default().merge(field, bdd);
            }
        }
        for (index, field) in other.fields {
            self.fields.entry(index).or_default().merge(field, bdd);
        }
    }
}
struct Trace<'a> {
    bdd: &'a mut Bdd,
    formulas: &'a BTreeMap<Id, Formula>,
    signatures: &'a BTreeMap<String, Signature>,
    manifest: &'a Manifest,
    forwarded: &'a BTreeMap<Id, Vec<DiagnosticNote>>,
    control: &'a str,
    slots: BTreeMap<Id, ValueTrace>,
    atoms: BTreeMap<Id, Notes>,
    exits: BTreeMap<String, Vec<DiagnosticNote>>,
}
impl Trace<'_> {
    fn formula(&mut self, guard: &Guard) -> Formula {
        match guard {
            Guard::True => TRUE,
            Guard::Atom(id) => self.formulas.get(id).copied().unwrap_or(FALSE),
            Guard::And(a, b) => {
                let a = self.formula(a);
                let b = self.formula(b);
                self.bdd.and(a, b)
            }
            Guard::Or(a, b) => {
                let a = self.formula(a);
                let b = self.formula(b);
                self.bdd.or(a, b)
            }
        }
    }
    fn flow(&mut self, guard: &Guard, domain: Formula) -> Notes {
        let mut notes = vec![];
        match guard {
            Guard::True => {}
            Guard::Atom(id) => notes = self.atoms.get(id).cloned().unwrap_or_default(),
            Guard::And(a, b) | Guard::Or(a, b) => {
                notes = self.flow(a, domain);
                for (domain, note) in self.flow(b, domain) {
                    add(&mut notes, domain, note, self.bdd);
                }
            }
        }
        restrict_notes(&mut notes, domain, self.bdd);
        notes
    }
    fn note(&self, span: Span, message: String) -> DiagnosticNote {
        DiagnosticNote {
            span,
            path: None,
            message,
        }
    }
    fn value(&mut self, expr: &Expr, flow: &Notes, domain: Formula, span: Span) -> ValueTrace {
        let mut result = match expr {
            Expr::Place(place) | Expr::Move(place) => self.place(place),
            Expr::Project(value, index) => self
                .value(value, flow, domain, span)
                .fields
                .remove(index)
                .unwrap_or_default(),
            Expr::Record(_, fields) => ValueTrace {
                fields: fields
                    .iter()
                    .map(|(i, e)| (*i, self.value(e, flow, domain, span)))
                    .collect(),
                ..ValueTrace::default()
            },
            Expr::Construct(ty, _, fields)
                if matches!(
                    self.manifest.type_def(ty),
                    Some(manifest::TypeDef::Record { .. })
                ) =>
            {
                ValueTrace {
                    fields: fields
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (i, self.value(e, flow, domain, span)))
                        .collect(),
                    ..ValueTrace::default()
                }
            }
            Expr::Construct(ty, name, fields) if self.manifest.outcomes(ty).is_some() => {
                let mut notes = flow.clone();
                add(
                    &mut notes,
                    domain,
                    self.note(
                        span,
                        format!(
                            "outcome `{name}` is constructed by `{}` here (structural contributor)",
                            self.control
                        ),
                    ),
                    self.bdd,
                );
                let tag = TagTrace {
                    notes,
                    fields: fields
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (i, self.value(e, flow, domain, span)))
                        .collect(),
                };
                ValueTrace {
                    tags: BTreeMap::from([(name.clone(), tag)]),
                    ..ValueTrace::default()
                }
            }
            // Stored references need alias-aware updates; do not snapshot them.
            _ => ValueTrace::default(),
        };
        result.restrict(domain, self.bdd);
        result
    }
    fn place(&self, place: &Place) -> ValueTrace {
        if place.indirect {
            return ValueTrace::default();
        }
        let mut trace = self.slots.get(&place.root);
        for field in &place.fields {
            trace = trace.and_then(|t| t.fields.get(field));
        }
        trace.cloned().unwrap_or_default()
    }
    fn write(&mut self, place: &Place, value: ValueTrace, domain: Formula) {
        if place.indirect {
            self.forget(domain);
            return;
        }
        let mut target = self.slots.entry(place.root).or_default();
        for field in &place.fields {
            target = target.fields.entry(*field).or_default();
        }
        let retained = self.bdd.not(domain);
        target.restrict(retained, self.bdd);
        target.merge(value, self.bdd);
    }
    fn forget(&mut self, domain: Formula) {
        let retained = self.bdd.not(domain);
        for trace in self.slots.values_mut() {
            trace.restrict(retained, self.bdd);
        }
    }
    fn operation(
        &mut self,
        operation: &Operation,
        flow: &Notes,
        domain: Formula,
        span: Span,
    ) -> ValueTrace {
        match operation {
            Operation::Variant(value) => self.value(value, flow, domain, span),
            Operation::BorrowedVariant(Expr::Reference(place, _)) => {
                let mut value = self.place(place);
                value.restrict(domain, self.bdd);
                value
            }
            Operation::BorrowedVariant(_) => ValueTrace::default(),
            Operation::Region(name) | Operation::Primitive(name) | Operation::Collection(name) => {
                let Some(sig) = self.signatures.get(name) else {
                    return ValueTrace::default();
                };
                let mut value = ValueTrace::default();
                for outcome in &sig.outcomes {
                    let mut notes = flow.clone();
                    for note in sig.contributors.get(&outcome.name).into_iter().flatten() {
                        add(&mut notes, domain, note.clone(), self.bdd);
                    }
                    add(
                        &mut notes,
                        domain,
                        self.note(
                            span,
                            format!(
                                "outcome `{}` is received from `{name}` by `{}` here",
                                outcome.name, self.control
                            ),
                        ),
                        self.bdd,
                    );
                    value.tags.insert(
                        outcome.name.clone(),
                        TagTrace {
                            notes,
                            ..TagTrace::default()
                        },
                    );
                }
                value
            }
        }
    }
    fn walk(&mut self, code: &[Instruction]) {
        for instruction in code {
            let domain = self.formula(&instruction.guard);
            let span = instruction.provenance.span;
            let flow = self.flow(&instruction.guard, domain);
            match &instruction.action {
                Action::Bind(id, expr) => {
                    let value = self.value(expr, &flow, domain, span);
                    self.write(
                        &Place {
                            root: *id,
                            fields: vec![],
                            indirect: false,
                        },
                        value,
                        domain,
                    );
                }
                Action::Assign(place, expr) => {
                    let value = self.value(expr, &flow, domain, span);
                    self.write(place, value, domain);
                }
                Action::Release(slots) => {
                    for id in slots {
                        self.write(
                            &Place {
                                root: *id,
                                fields: vec![],
                                indirect: false,
                            },
                            ValueTrace::default(),
                            domain,
                        );
                    }
                }
                Action::Decide {
                    operation, arms, ..
                } => {
                    let value = self.operation(operation, &flow, domain, span);
                    let mutates = match operation {
                        Operation::Collection(_) => true,
                        Operation::Region(name) | Operation::Primitive(name) => {
                            self.signatures.get(name).is_some_and(|s| {
                                s.parameters
                                    .iter()
                                    .any(|p| self.manifest.contains_exclusive_references(&p.ty))
                            })
                        }
                        _ => false,
                    };
                    if mutates {
                        self.forget(domain);
                    }
                    for arm in arms {
                        let selected = self.formulas.get(&arm.atom).copied().unwrap_or(FALSE);
                        let mut tag = value.tags.get(&arm.outcome).cloned().unwrap_or_default();
                        for (domain, note) in &flow {
                            add(&mut tag.notes, *domain, note.clone(), self.bdd);
                        }
                        restrict_notes(&mut tag.notes, selected, self.bdd);
                        self.atoms.insert(arm.atom, tag.notes);
                        for (index, id) in arm.slots.iter().enumerate() {
                            let mut payload = tag.fields.remove(&index).unwrap_or_default();
                            payload.restrict(selected, self.bdd);
                            self.write(
                                &Place {
                                    root: *id,
                                    fields: vec![],
                                    indirect: false,
                                },
                                payload,
                                selected,
                            );
                        }
                    }
                }
                Action::Exit(name, _) => {
                    let notes = if let Some(notes) = self.forwarded.get(&instruction.id) {
                        let mut notes = notes.clone();
                        for (_, note) in flow {
                            // The specialized forward note already names this call.
                            if note.span != span {
                                contracts::append(&mut notes, note);
                            }
                        }
                        notes
                    } else {
                        let mut notes: Vec<_> = flow.into_iter().map(|(_, n)| n).collect();
                        contracts::append(&mut notes, self.note(span, format!(
                            "outcome `{name}` is produced by `{}` here (structural contributor)", self.control
                        )));
                        notes
                    };
                    let target = self.exits.entry(name.clone()).or_default();
                    for note in notes {
                        contracts::append(target, note);
                    }
                }
                Action::Loop { body, .. } => {
                    // One structural traversal, with unknown loop-carried values.
                    // Initializers alone would give false lineage after a restart.
                    let slots = std::mem::take(&mut self.slots);
                    let atoms = self.atoms.clone();
                    self.walk(body);
                    self.slots = slots;
                    self.atoms = atoms;
                }
                _ => {}
            }
        }
    }
}
impl Checker<'_> {
    fn contributor_trace(&mut self) -> Trace<'_> {
        let mut trace = Trace {
            bdd: &mut self.bdd,
            formulas: &self.atom_formulas,
            signatures: self.signatures,
            manifest: self.manifest,
            forwarded: &self.forwarded_exits,
            control: &self.result.name,
            slots: BTreeMap::new(),
            atoms: BTreeMap::new(),
            exits: BTreeMap::new(),
        };
        trace.walk(&self.result.code);
        trace
    }
    pub(super) fn outcome_contributors(&mut self) -> BTreeMap<String, Vec<DiagnosticNote>> {
        self.contributor_trace().exits
    }
    pub(super) fn with_path_contributors(
        &mut self,
        mut diagnostic: Diagnostic,
        guard: &Guard,
        domain: Formula,
    ) -> Diagnostic {
        for (_, note) in self.contributor_trace().flow(guard, domain) {
            contracts::append(&mut diagnostic.notes, note);
        }
        diagnostic
    }
    pub(super) fn with_operation_contributors(
        &mut self,
        mut diagnostic: Diagnostic,
        operation: &Operation,
        domain: Formula,
        missing: &[String],
    ) -> Diagnostic {
        // A call error already points at the call; avoid a duplicate related note.
        if let Operation::Region(name) | Operation::Primitive(name) = operation {
            if let Some(sig) = self.signatures.get(name) {
                return self.with_contributors(diagnostic, sig, missing);
            }
            return diagnostic;
        }
        let value = self
            .contributor_trace()
            .operation(operation, &vec![], domain, diagnostic.span);
        for name in missing {
            for (_, note) in value.tags.get(name).into_iter().flat_map(|t| &t.notes) {
                contracts::append(&mut diagnostic.notes, note.clone());
            }
        }
        diagnostic
    }
}
