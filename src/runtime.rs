//! Deterministic reference interpreter. Primitive adapters are scripted trusted inputs.
use crate::{
    Diagnostic, Result, Span,
    ir::*,
    manifest::{Manifest, TypeDef},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptCall {
    pub operation: String,
    pub outcome: String,
    pub fields: Vec<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub entry: String,
    pub arguments: Vec<Value>,
    pub calls: Vec<ScriptCall>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Event {
    Fault {
        report: Box<crate::fault::Report>,
    },
    Primitive {
        operation: String,
        arguments: Vec<Value>,
        outcome: String,
        fields: Vec<Value>,
        effects: Vec<String>,
    },
    Emit {
        message: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Execution {
    pub outcome: String,
    pub fields: Vec<Value>,
    pub events: Vec<Event>,
}
/// Administrative termination is separate from ordinary outcome completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProcessTermination {
    pub termination: &'static str,
    pub status: u8,
    pub events: Vec<Event>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RunCompletion {
    Outcome(Execution),
    Process(ProcessTermination),
}
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_DRIVER", Span::default(), message)
}
pub(crate) fn validate(value: &Value, expected: &str, manifest: &Manifest) -> Result<()> {
    if value.ty() != expected {
        return Err(error(format!(
            "driver supplied {}, expected {expected}",
            value.ty()
        )));
    }
    match value {
        Value::Reference { place, .. } => {
            if crate::manifest::reference_inner(expected).is_none() || place.indirect {
                return Err(error("invalid internal reference value"));
            }
        }
        Value::Opaque { ty, .. } => {
            if !matches!(manifest.type_def(ty), Some(TypeDef::Opaque { .. })) {
                return Err(error("opaque value requires opaque manifest type"));
            }
        }
        Value::Construct { ty, name, fields } => {
            if let Some(c) = crate::collection_support::Collection::parse(ty) {
                if !manifest.valid_type(ty) || name != c.name() {
                    return Err(error("invalid collection type or constructor"));
                }
                let types = c
                    .fields(fields.len())
                    .ok_or_else(|| error("invalid collection length"))?;
                for (value, ty) in fields.iter().zip(types) {
                    validate(value, &ty, manifest)?;
                }
                if matches!(c, crate::collection_support::Collection::Map(..))
                    && !crate::collection_support::sorted_map(fields)
                {
                    return Err(error("map keys must be unique and sorted"));
                }
                return Ok(());
            }
            let expected = if let Some(TypeDef::Record { fields, .. }) = manifest.type_def(ty) {
                if name != ty {
                    return Err(error("invalid record constructor"));
                }
                fields.clone()
            } else {
                if ty == "Bool" {
                    return Err(error("Bool requires a boolean value, not a constructor"));
                }
                manifest
                    .outcomes(ty)
                    .and_then(|outcomes| outcomes.into_iter().find(|o| o.name == *name))
                    .ok_or_else(|| error(format!("invalid constructor {ty}.{name}")))?
                    .fields
            };
            if fields.len() != expected.len() {
                return Err(error("invalid constructor payload arity"));
            }
            for (value, field) in fields.iter().zip(expected) {
                validate(value, &field.ty, manifest)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn address(place: &Place, values: &BTreeMap<Id, Value>) -> Result<Place> {
    if !place.indirect {
        return Ok(place.clone());
    }
    let Some(Value::Reference { place: target, .. }) = values.get(&place.root) else {
        return Err(error("invalid reference address"));
    };
    let mut target = target.clone();
    target.fields.extend(&place.fields);
    Ok(target)
}
fn place_mut<'a>(place: &Place, values: &'a mut BTreeMap<Id, Value>) -> Result<&'a mut Value> {
    let place = address(place, values)?;
    let mut value = values
        .get_mut(&place.root)
        .ok_or_else(|| error("unavailable mutable place"))?;
    for index in &place.fields {
        let Value::Construct { fields, .. } = value else {
            return Err(error("invalid mutable projection"));
        };
        value = fields
            .get_mut(*index)
            .ok_or_else(|| error("invalid field index"))?;
    }
    Ok(value)
}
fn evaluate(expr: &Expr, values: &mut BTreeMap<Id, Value>, base: Id) -> Result<Value> {
    match expr {
        Expr::Length(e) => {
            let Value::Reference { place, .. } = evaluate(e, values, base)? else {
                return Err(error("invalid collection length argument"));
            };
            Ok(Value::U32(
                crate::collection_support::length(place_mut(&place, values)?).map_err(error)?,
            ))
        }
        Expr::Value(value) => Ok(value.clone()),
        Expr::Pure(name, args) => {
            let args = args
                .iter()
                .map(|e| evaluate(e, values, base))
                .collect::<Result<Vec<_>>>()?;
            match (name.as_str(), args.as_slice()) {
                ("u32_text", [Value::U32(n)]) => Ok(Value::String(n.to_string())),
                ("concat", [Value::String(a), Value::String(b)]) => {
                    Ok(Value::String(format!("{a}{b}")))
                }
                _ => Err(error("invalid pure expression contract")),
            }
        }
        Expr::Reference(place, ty) => Ok(Value::Reference {
            ty: ty.clone(),
            place: address(&shift(place, base), values)?,
        }),
        Expr::Move(place) => {
            let place = address(&shift(place, base), values)?;
            if place.fields.is_empty() {
                values
                    .remove(&place.root)
                    .ok_or_else(|| error("unavailable moved value"))
            } else {
                Ok(std::mem::replace(place_mut(&place, values)?, Value::Unit))
            }
        }
        Expr::Place(place) => {
            let place = address(&shift(place, base), values)?;
            let mut value = values.get(&place.root).ok_or_else(|| {
                error(format!(
                    "interpreter invariant: slot {} is unavailable",
                    place.root
                ))
            })?;
            for field in &place.fields {
                let Value::Construct { fields, .. } = value else {
                    return Err(error("interpreter invariant: non-record place"));
                };
                value = fields
                    .get(*field)
                    .ok_or_else(|| error("interpreter invariant: invalid field index"))?;
            }
            Ok(value.clone())
        }
        Expr::Project(e, field) => {
            let Value::Construct { fields, .. } = evaluate(e, values, base)? else {
                return Err(error("interpreter invariant: non-record projection"));
            };
            fields
                .get(*field)
                .cloned()
                .ok_or_else(|| error("interpreter invariant: invalid projection"))
        }
        Expr::Construct(ty, name, fields) => Ok(Value::Construct {
            ty: ty.clone(),
            name: name.clone(),
            fields: fields
                .iter()
                .map(|e| evaluate(e, values, base))
                .collect::<Result<_>>()?,
        }),
        Expr::Record(ty, initializers) => {
            let mut fields = vec![Value::Unit; initializers.len()];
            for (index, expr) in initializers {
                let value = evaluate(expr, values, base)?;
                *fields
                    .get_mut(*index)
                    .ok_or_else(|| error("interpreter invariant: invalid record initializer"))? =
                    value;
            }
            Ok(Value::Construct {
                ty: ty.clone(),
                name: ty.clone(),
                fields,
            })
        }
        Expr::Not(expr) => match evaluate(expr, values, base)? {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            _ => Err(error("interpreter invariant: ! operand")),
        },
        Expr::Binary(op, a, b) => {
            let a = evaluate(a, values, base)?;
            if op == "&&" && a == Value::Bool(false) {
                return Ok(Value::Bool(false));
            }
            if op == "||" && a == Value::Bool(true) {
                return Ok(Value::Bool(true));
            }
            let b = evaluate(b, values, base)?;
            let answer = match (op.as_str(), a, b) {
                ("==", a, b) => a == b,
                ("!=", a, b) => a != b,
                ("&&", Value::Bool(a), Value::Bool(b)) => a && b,
                ("||", Value::Bool(a), Value::Bool(b)) => a || b,
                ("<", Value::U32(a), Value::U32(b)) => a < b,
                ("<=", Value::U32(a), Value::U32(b)) => a <= b,
                (">", Value::U32(a), Value::U32(b)) => a > b,
                (">=", Value::U32(a), Value::U32(b)) => a >= b,
                _ => return Err(error("interpreter invariant: invalid binary operands")),
            };
            Ok(Value::Bool(answer))
        }
    }
}
fn shift(place: &Place, base: Id) -> Place {
    Place {
        root: base + place.root,
        ..place.clone()
    }
}
enum Flow {
    Process(u8),
    Recover(usize, Vec<Value>),
    Terminate,
    Exit(String, Vec<Value>),
    Again(usize, Vec<Value>),
    Complete(usize),
    Fallthrough,
}

/// Administrative settlement is not a source outcome, even inside the runtime.
enum Completion {
    Process(u8),
    Outcome(String, Vec<Value>),
    Settled,
}
impl Completion {
    fn outcome(self) -> Result<(String, Vec<Value>)> {
        match self {
            Self::Outcome(name, values) => Ok((name, values)),
            Self::Settled | Self::Process(_) => Err(error(
                "administrative settlement cannot enter an outcome continuation",
            )),
        }
    }
    fn settled(self) -> Result<()> {
        match self {
            Self::Settled => Ok(()),
            Self::Outcome(..) | Self::Process(_) => Err(error(
                "a supervised boundary cannot return an ordinary outcome",
            )),
        }
    }
}

struct Machine<'a> {
    next_base: Id,
    contingencies: Vec<Diagnostic>,
    program: &'a CheckedProgram,
    calls: &'a [ScriptCall],
    cursor: usize,
    events: Vec<Event>,
    steps_left: usize,
    activations: crate::fault::Tracker,
    phase: crate::fault::Phase,
}
impl Machine<'_> {
    fn invoke(
        &mut self,
        name: &str,
        args: Vec<Value>,
        values: &mut BTreeMap<Id, Value>,
        handover: &mut Vec<Value>,
    ) -> Result<Completion> {
        let region = self
            .program
            .region(name)
            .ok_or_else(|| error(format!("unknown entry region `{name}`")))?;
        if args.len() != region.parameters.len() {
            return Err(error("wrong entry argument count"));
        }
        let base = self.next_base;
        self.next_base = self
            .next_base
            .checked_add(self.program.next_id.max(1))
            .ok_or_else(|| error("activation storage address space exhausted"))?;
        let mut facts = BTreeSet::new();
        for (param, value) in region.parameters.iter().zip(args) {
            validate(&value, &param.ty, self.program.manifest())?;
            values.insert(base + param.id, value);
        }
        self.activations
            .enter(&self.program.runtime_name(name))
            .map_err(error)?;
        self.phase = crate::fault::Phase::Execution;
        let flow = self.execute(&region.code, values, &mut facts, base);
        if flow.is_err() && self.activations.is_source_fault() {
            if let Some(boundary) = &region.boundary {
                for id in &boundary.retained {
                    handover.push(
                        values
                            .remove(&(base + id))
                            .ok_or_else(|| error("retained boundary resource is unavailable"))?,
                    );
                }
            }
        }
        for id in region.slots.keys() {
            values.remove(&(base + id));
        }
        match flow? {
            Flow::Process(status) => {
                self.activations.leave();
                Ok(Completion::Process(status))
            }
            Flow::Terminate => {
                self.activations.leave();
                Ok(Completion::Settled)
            }
            Flow::Exit(outcome, fields) => {
                self.activations.leave();
                Ok(Completion::Outcome(outcome, fields))
            }
            _ => Err(error("interpreter invariant: checked region fell through")),
        }
    }
    fn supervise(
        &mut self,
        name: &str,
        args: Vec<Value>,
        values: &mut BTreeMap<Id, Value>,
    ) -> Result<()> {
        let handler = self
            .program
            .region(name)
            .and_then(|r| r.boundary.as_ref())
            .ok_or_else(|| error("invalid boundary"))?
            .handler
            .clone();
        let child = self.activations.child();
        let mut parent = std::mem::replace(&mut self.activations, child);
        let mut retained = vec![];
        let result = self.invoke(name, args, values, &mut retained);
        let result = match result {
            Err(diagnostic) if self.activations.is_source_fault() => {
                let report = self.activations.failure(
                    &self.program.runtime_name(name),
                    crate::fault::Phase::Execution,
                    &format!("{}: {}", diagnostic.code, diagnostic.message),
                );
                self.events.push(Event::Fault {
                    report: Box::new(report.clone()),
                });
                let mut handler_tracker = self.activations.child();
                handler_tracker.with_cause(report);
                self.activations = handler_tracker;
                retained.push(Value::String(diagnostic.message.clone()));
                self.contingencies.push(diagnostic);
                let answer = self
                    .invoke(&handler, retained, values, &mut vec![])
                    .and_then(Completion::settled);
                self.contingencies.pop();
                answer
            }
            other => other.and_then(Completion::settled),
        };
        parent.absorb(&self.activations);
        if let Err(diagnostic) = &result {
            if self.activations.is_source_fault() {
                parent.source_fault();
            }
            let report = self.activations.failure(
                &self.program.runtime_name(name),
                crate::fault::Phase::Execution,
                &format!("{}: {}", diagnostic.code, diagnostic.message),
            );
            parent.with_cause(report);
        }
        self.activations = parent;
        result
    }
    fn execute(
        &mut self,
        code: &[Instruction],
        values: &mut BTreeMap<Id, Value>,
        facts: &mut BTreeSet<Id>,
        base: Id,
    ) -> Result<Flow> {
        for instruction in code {
            self.activations.at(instruction.id);
            if self.steps_left == 0 {
                return Err(Diagnostic::new(
                    "E_FUEL",
                    instruction.provenance.span,
                    "reference interpreter step limit reached; this is a host limit, not a language outcome",
                ));
            }
            self.steps_left -= 1;
            if !instruction.guard.evaluate(facts) {
                continue;
            }
            match &instruction.action {
                Action::Boundary {
                    body,
                    handler,
                    retained: _,
                    retire,
                    work_slots,
                    handler_slots,
                    fault_slot,
                } => {
                    let child = self.activations.child();
                    let mut parent = std::mem::replace(&mut self.activations, child);
                    let name = format!(
                        "{}$boundary@{}",
                        instruction.provenance.region, instruction.provenance.span.line
                    );
                    self.activations.enter(&name).map_err(error)?;
                    let result = self.execute(body, values, facts, base);
                    let result = match result {
                        Err(diagnostic) if self.activations.is_source_fault() => {
                            let report = self.activations.failure(
                                &name,
                                crate::fault::Phase::Execution,
                                &format!("{}: {}", diagnostic.code, diagnostic.message),
                            );
                            self.events.push(Event::Fault {
                                report: Box::new(report.clone()),
                            });
                            for id in retire {
                                values.remove(&(base + id));
                            }
                            let (message, stack, paths) = report.source_fields();
                            let strings = |xs: Vec<String>| Value::Construct {
                                ty: "vector<String>".into(),
                                name: "vector".into(),
                                fields: xs.into_iter().map(Value::String).collect(),
                            };
                            values.insert(
                                base + fault_slot,
                                Value::Construct {
                                    ty: "fault_report".into(),
                                    name: "fault_report".into(),
                                    fields: vec![
                                        Value::String(message),
                                        strings(stack),
                                        strings(paths),
                                    ],
                                },
                            );
                            let mut tracker = self.activations.child();
                            tracker.with_cause(report);
                            self.activations = tracker;
                            self.activations
                                .enter(&format!("{name}$contingency"))
                                .map_err(error)?;
                            self.contingencies.push(diagnostic);
                            let result = self.execute(handler, values, facts, base);
                            self.contingencies.pop();
                            result
                        }
                        other => other,
                    };
                    for id in work_slots.iter().chain(handler_slots) {
                        values.remove(&(base + id));
                    }
                    parent.absorb(&self.activations);
                    if let Err(diagnostic) = &result {
                        if self.activations.is_source_fault() {
                            parent.source_fault();
                        }
                        parent.with_cause(self.activations.failure(
                            &name,
                            crate::fault::Phase::Execution,
                            &format!("{}: {}", diagnostic.code, diagnostic.message),
                        ));
                    }
                    self.activations = parent;
                    match result? {
                        Flow::Fallthrough => {}
                        flow => return Ok(flow),
                    }
                }
                Action::Recover { target, args } => {
                    return Ok(Flow::Recover(
                        *target,
                        args.iter()
                            .map(|e| evaluate(e, values, base))
                            .collect::<Result<_>>()?,
                    ));
                }
                Action::Terminate => return Ok(Flow::Terminate),
                Action::TerminateProcess(status) => return Ok(Flow::Process(*status)),
                Action::Escalate => {
                    self.activations.source_fault();
                    return Err(self
                        .contingencies
                        .last()
                        .cloned()
                        .ok_or_else(|| error("escalate outside contingency"))?);
                }
                Action::Supervise { boundary, args } => {
                    let args = args
                        .iter()
                        .map(|e| evaluate(e, values, base))
                        .collect::<Result<Vec<_>>>()?;
                    self.supervise(boundary, args, values)?;
                }
                Action::Again { target, args } => {
                    return Ok(Flow::Again(
                        *target,
                        args.iter()
                            .map(|e| evaluate(e, values, base))
                            .collect::<Result<_>>()?,
                    ));
                }
                Action::Loop {
                    supervised: _,
                    id,
                    parameters,
                    local_slots,
                    local_atoms,
                    body,
                } => {
                    let mut arguments = parameters
                        .iter()
                        .map(|(_, e)| evaluate(e, values, base))
                        .collect::<Result<Vec<_>>>()?;
                    loop {
                        for slot in local_slots {
                            values.remove(&(base + slot));
                        }
                        for atom in local_atoms {
                            facts.remove(atom);
                        }
                        for ((slot, _), value) in parameters.iter().zip(arguments) {
                            values.insert(base + *slot, value);
                        }
                        match self.execute(body, values, facts, base)? {
                            Flow::Again(target, next) | Flow::Recover(target, next)
                                if target == *id =>
                            {
                                arguments = next
                            }
                            Flow::Complete(target) if target == *id => break,
                            Flow::Fallthrough => {
                                return Err(error("interpreter invariant: loop fell through"));
                            }
                            flow => return Ok(flow),
                        }
                    }
                }
                Action::Bind(id, expr) => {
                    let value = evaluate(expr, values, base)?;
                    values.insert(base + *id, value);
                }
                Action::Assign(place, expr) => {
                    let value = evaluate(expr, values, base)?;
                    let place = address(&shift(place, base), values)?;
                    if place.fields.is_empty() {
                        values.insert(place.root, value);
                    } else {
                        *place_mut(&place, values)? = value;
                    }
                }
                Action::Drop(expr) => {
                    drop(evaluate(expr, values, base)?);
                }
                Action::Release(ids) => {
                    for id in ids {
                        values.remove(&(base + id));
                    }
                }
                Action::Emit(expr) => {
                    let Value::String(message) = evaluate(expr, values, base)? else {
                        return Err(error("interpreter invariant: emit type"));
                    };
                    self.events.push(Event::Emit { message });
                }
                Action::Fault(expr) => {
                    let Value::String(message) = evaluate(expr, values, base)? else {
                        return Err(error("interpreter invariant: fault type"));
                    };
                    self.activations.source_fault();
                    return Err(Diagnostic::new(
                        "E_FAULT",
                        instruction.provenance.span,
                        message,
                    ));
                }
                Action::Exit(outcome, args) => {
                    return Ok(Flow::Exit(
                        outcome.clone(),
                        args.iter()
                            .map(|a| evaluate(a, values, base))
                            .collect::<Result<_>>()?,
                    ));
                }
                Action::Complete {
                    atom,
                    bindings,
                    unwind,
                    release,
                } => {
                    let payloads = bindings
                        .iter()
                        .map(|(id, e)| Ok((*id, evaluate(e, values, base)?)))
                        .collect::<Result<Vec<_>>>()?;
                    for (id, value) in payloads {
                        values.insert(base + id, value);
                    }
                    for id in release {
                        values.remove(&(base + id));
                    }
                    facts.insert(*atom);
                    if let Some(target) = unwind {
                        return Ok(Flow::Complete(*target));
                    }
                }
                Action::Decide {
                    operation,
                    args,
                    arms,
                } => {
                    let args = args
                        .iter()
                        .map(|a| evaluate(a, values, base))
                        .collect::<Result<Vec<_>>>()?;
                    let (outcome, fields) = match operation {
                        Operation::Collection(name) => collection(name, args, values)?,
                        Operation::Region(name) => {
                            self.invoke(name, args, values, &mut vec![])?.outcome()?
                        }
                        Operation::Variant(expr) => match evaluate(expr, values, base)? {
                            Value::Bool(b) => (b.to_string(), vec![]),
                            Value::Construct { name, fields, .. } => (name, fields),
                            _ => return Err(error("interpreter invariant: decision operand")),
                        },
                        Operation::BorrowedVariant(expr) => {
                            let Value::Reference { ty, place } = evaluate(expr, values, base)?
                            else {
                                return Err(error("invalid borrowed decision address"));
                            };
                            let outcomes = self
                                .program
                                .manifest
                                .borrowed_outcomes(&ty)
                                .ok_or_else(|| error("invalid borrowed decision type"))?;
                            match place_mut(&place, values)?.clone() {
                                Value::Bool(b) => (b.to_string(), vec![]),
                                Value::Construct {
                                    name,
                                    fields: payloads,
                                    ..
                                } => {
                                    let outcome = outcomes
                                        .iter()
                                        .find(|o| o.name == name)
                                        .ok_or_else(|| error("invalid borrowed decision tag"))?;
                                    let fields = outcome
                                        .fields
                                        .iter()
                                        .enumerate()
                                        .map(|(i, f)| {
                                            let place = if let Value::Reference { place, .. } =
                                                &payloads[i]
                                            {
                                                place.clone()
                                            } else {
                                                let mut place = place.clone();
                                                place.fields.push(i);
                                                place
                                            };
                                            Value::Reference {
                                                ty: f.ty.clone(),
                                                place,
                                            }
                                        })
                                        .collect();
                                    (name, fields)
                                }
                                _ => return Err(error("invalid borrowed decision storage")),
                            }
                        }
                        Operation::Primitive(name) => {
                            let primitive = self
                                .program
                                .manifest
                                .primitives
                                .iter()
                                .find(|p| p.name == *name)
                                .unwrap();
                            if primitive.pure {
                                let expression = Expr::Pure(
                                    name.clone(),
                                    args.into_iter().map(Expr::Value).collect(),
                                );
                                (
                                    primitive.outcomes[0].name.clone(),
                                    vec![evaluate(&expression, values, base)?],
                                )
                            } else {
                                let call = self.calls.get(self.cursor).ok_or_else(|| {
                                    error(format!("script exhausted before `{name}`"))
                                })?;
                                if call.operation != *name {
                                    return Err(error(format!(
                                        "expected script operation `{name}`, found `{}`",
                                        call.operation
                                    )));
                                }
                                self.cursor += 1;
                                let outcome = primitive
                                    .outcomes
                                    .iter()
                                    .find(|o| o.name == call.outcome)
                                    .ok_or_else(|| {
                                        error(format!(
                                            "undeclared driver outcome `{}`",
                                            call.outcome
                                        ))
                                    })?;
                                if outcome.fields.len() != call.fields.len() {
                                    return Err(error("driver outcome payload arity mismatch"));
                                }
                                for (field, value) in outcome.fields.iter().zip(&call.fields) {
                                    validate(value, &field.ty, &self.program.manifest)?;
                                }
                                self.events.push(Event::Primitive {
                                    operation: name.clone(),
                                    arguments: args,
                                    outcome: call.outcome.clone(),
                                    fields: call.fields.clone(),
                                    effects: primitive.effects.clone(),
                                });
                                (call.outcome.clone(), call.fields.clone())
                            }
                        }
                    };
                    let arm = arms
                        .iter()
                        .find(|arm| arm.outcome == outcome)
                        .ok_or_else(|| error("interpreter invariant: missing decision arm"))?;
                    if fields.len() != arm.slots.len() {
                        return Err(error("interpreter invariant: payload arity"));
                    }
                    facts.insert(arm.atom);
                    for (slot, value) in arm.slots.iter().zip(fields) {
                        values.insert(base + *slot, value);
                    }
                }
            }
        }
        Ok(Flow::Fallthrough)
    }
}
#[derive(Debug)]
pub struct Failure {
    pub diagnostic: Diagnostic,
    pub report: crate::fault::Report,
    /// Observed effects before termination, for finite execution comparisons.
    /// These are deliberately separate from the bounded diagnostic report.
    pub events: Vec<Event>,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.report.fmt(f)
    }
}
impl std::error::Error for Failure {}
impl CheckedProgram {
    pub fn run(&self, scenario: &Scenario) -> Result<Execution> {
        self.run_with_limit(scenario, 1_000_000)
    }
    pub fn run_with_limit(&self, scenario: &Scenario, steps: usize) -> Result<Execution> {
        self.run_detailed(scenario, steps)
            .map_err(|failure| failure.diagnostic)
    }
    /// A fresh execution boundary, with observed invocation context on failure.
    /// Only explicit supervision isolates source faults; this never catches panics.
    pub fn run_detailed(
        &self,
        scenario: &Scenario,
        steps: usize,
    ) -> std::result::Result<Execution, Box<Failure>> {
        match self.run_completion(scenario, steps)? {
            RunCompletion::Outcome(result) => Ok(result),
            RunCompletion::Process(result) => {
                let diagnostic = Diagnostic::new(
                    "E_PROCESS_EXIT",
                    Span::default(),
                    format!(
                        "entry terminated the process with status {}; use run_completion to receive administrative results",
                        result.status
                    ),
                );
                let report = crate::fault::Tracker::new(self.runtime_sites()).failure(
                    &scenario.entry,
                    crate::fault::Phase::ResultValidation,
                    &diagnostic.message,
                );
                Err(Box::new(Failure {
                    diagnostic,
                    report,
                    events: result.events,
                }))
            }
        }
    }
    /// Run an executable entry, preserving process termination as a distinct result.
    pub fn run_completion(
        &self,
        scenario: &Scenario,
        steps: usize,
    ) -> std::result::Result<RunCompletion, Box<Failure>> {
        let mut machine = Machine {
            next_base: 0,
            contingencies: vec![],
            program: self,
            calls: &scenario.calls,
            cursor: 0,
            events: vec![],
            steps_left: steps,
            activations: crate::fault::Tracker::new(self.runtime_sites()),
            phase: crate::fault::Phase::EntryValidation,
        };
        let result = (|| {
            if self
                .region(&scenario.entry)
                .is_some_and(|r| r.boundary.is_some() || r.contingency)
            {
                return Err(error(
                    "boundaries are entered using supervise, not as executable controls",
                ));
            }
            if self
                .region(&scenario.entry)
                .is_some_and(|r| r.parameters.iter().any(|p| p.ty.starts_with('&')))
            {
                return Err(error("entry controls cannot require borrowed arguments"));
            }
            let completion = machine.invoke(
                &scenario.entry,
                scenario.arguments.clone(),
                &mut BTreeMap::new(),
                &mut vec![],
            )?;
            machine.phase = crate::fault::Phase::ResultValidation;
            if machine.cursor != scenario.calls.len() {
                return Err(error(format!(
                    "{} unconsumed script calls",
                    scenario.calls.len() - machine.cursor
                )));
            }
            let events = std::mem::take(&mut machine.events);
            match completion {
                Completion::Outcome(outcome, fields) => Ok(RunCompletion::Outcome(Execution {
                    outcome,
                    fields,
                    events,
                })),
                Completion::Process(status) => Ok(RunCompletion::Process(ProcessTermination {
                    termination: "process",
                    status,
                    events,
                })),
                Completion::Settled => {
                    Err(error("entry settled without an executable disposition"))
                }
            }
        })();
        result.map_err(|diagnostic| {
            Box::new(Failure {
                events: std::mem::take(&mut machine.events),
                report: machine.activations.failure(
                    &self
                        .region(&scenario.entry)
                        .map(|r| self.runtime_name(&r.name))
                        .unwrap_or_else(|| scenario.entry.clone()),
                    machine.phase,
                    &format!("{}: {}", diagnostic.code, diagnostic.message),
                ),
                diagnostic,
            })
        })
    }
}

impl crate::collection_support::CollectionValue for Value {
    fn parts(&self) -> Option<(&str, &str, &[Self])> {
        if let Self::Construct { ty, name, fields } = self {
            Some((ty, name, fields))
        } else {
            None
        }
    }
    fn parts_mut(&mut self) -> Option<&mut Vec<Self>> {
        if let Self::Construct { fields, .. } = self {
            Some(fields)
        } else {
            None
        }
    }
    fn construct(ty: String, name: String, fields: Vec<Self>) -> Self {
        Self::Construct { ty, name, fields }
    }
    fn number(&self) -> Option<u32> {
        if let Self::U32(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    fn key_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            (Self::U32(a), Self::U32(b)) => Some(a.cmp(b)),
            (Self::String(a), Self::String(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }
}

fn collection(
    name: &str,
    args: Vec<Value>,
    values: &mut BTreeMap<Id, Value>,
) -> Result<(String, Vec<Value>)> {
    let mut args = args.into_iter();
    let mut first = args.next().ok_or_else(|| error("missing collection"))?;
    let address = if let Value::Reference { place, .. } = &first {
        Some(place.clone())
    } else {
        None
    };
    let container = if let Some(p) = &address {
        place_mut(p, values)?
    } else {
        &mut first
    };
    let (outcome, payloads) =
        crate::collection_support::operate(name, container, args.collect()).map_err(error)?;
    let fields = payloads
        .into_iter()
        .map(|p| match p {
            crate::collection_support::Payload::Owned(v) => Ok(v),
            crate::collection_support::Payload::Borrow(index, ty) => {
                let mut place = address
                    .clone()
                    .ok_or_else(|| error("borrow of collection temporary"))?;
                place.fields.push(index);
                Ok(Value::Reference { ty, place })
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((outcome, fields))
}
