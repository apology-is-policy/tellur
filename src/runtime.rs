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
fn evaluate(expr: &Expr, values: &mut BTreeMap<Id, Value>) -> Result<Value> {
    match expr {
        Expr::Length(e) => {
            let Value::Reference { place, .. } = evaluate(e, values)? else {
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
                .map(|e| evaluate(e, values))
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
            place: address(place, values)?,
        }),
        Expr::Move(place) => {
            let place = address(place, values)?;
            if place.fields.is_empty() {
                values
                    .remove(&place.root)
                    .ok_or_else(|| error("unavailable moved value"))
            } else {
                Ok(std::mem::replace(place_mut(&place, values)?, Value::Unit))
            }
        }
        Expr::Place(place) => {
            let place = address(place, values)?;
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
            let Value::Construct { fields, .. } = evaluate(e, values)? else {
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
                .map(|e| evaluate(e, values))
                .collect::<Result<_>>()?,
        }),
        Expr::Record(ty, initializers) => {
            let mut fields = vec![Value::Unit; initializers.len()];
            for (index, expr) in initializers {
                let value = evaluate(expr, values)?;
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
        Expr::Not(expr) => match evaluate(expr, values)? {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            _ => Err(error("interpreter invariant: ! operand")),
        },
        Expr::Binary(op, a, b) => {
            let a = evaluate(a, values)?;
            if op == "&&" && a == Value::Bool(false) {
                return Ok(Value::Bool(false));
            }
            if op == "||" && a == Value::Bool(true) {
                return Ok(Value::Bool(true));
            }
            let b = evaluate(b, values)?;
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
enum Flow {
    Exit(String, Vec<Value>),
    Again(usize, Vec<Value>),
    Complete(usize),
    Fallthrough,
}

struct Machine<'a> {
    program: &'a CheckedProgram,
    calls: &'a [ScriptCall],
    cursor: usize,
    events: Vec<Event>,
    steps_left: usize,
}
impl Machine<'_> {
    fn invoke(
        &mut self,
        name: &str,
        args: Vec<Value>,
        values: &mut BTreeMap<Id, Value>,
    ) -> Result<(String, Vec<Value>)> {
        let region = self
            .program
            .region(name)
            .ok_or_else(|| error(format!("unknown entry region `{name}`")))?;
        if args.len() != region.parameters.len() {
            return Err(error("wrong entry argument count"));
        }
        let mut facts = BTreeSet::new();
        for (param, value) in region.parameters.iter().zip(args) {
            validate(&value, &param.ty, self.program.manifest())?;
            values.insert(param.id, value);
        }
        let flow = self.execute(&region.code, values, &mut facts)?;
        for id in region.slots.keys() {
            values.remove(id);
        }
        match flow {
            Flow::Exit(outcome, fields) => Ok((outcome, fields)),
            _ => Err(error("interpreter invariant: checked region fell through")),
        }
    }
    fn execute(
        &mut self,
        code: &[Instruction],
        values: &mut BTreeMap<Id, Value>,
        facts: &mut BTreeSet<Id>,
    ) -> Result<Flow> {
        for instruction in code {
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
                Action::Again { target, args } => {
                    return Ok(Flow::Again(
                        *target,
                        args.iter()
                            .map(|e| evaluate(e, values))
                            .collect::<Result<_>>()?,
                    ));
                }
                Action::Loop {
                    id,
                    parameters,
                    local_slots,
                    local_atoms,
                    body,
                } => {
                    let mut arguments = parameters
                        .iter()
                        .map(|(_, e)| evaluate(e, values))
                        .collect::<Result<Vec<_>>>()?;
                    loop {
                        for slot in local_slots {
                            values.remove(slot);
                        }
                        for atom in local_atoms {
                            facts.remove(atom);
                        }
                        for ((slot, _), value) in parameters.iter().zip(arguments) {
                            values.insert(*slot, value);
                        }
                        match self.execute(body, values, facts)? {
                            Flow::Again(target, next) if target == *id => arguments = next,
                            Flow::Complete(target) if target == *id => break,
                            Flow::Fallthrough => {
                                return Err(error("interpreter invariant: loop fell through"));
                            }
                            flow => return Ok(flow),
                        }
                    }
                }
                Action::Bind(id, expr) => {
                    let value = evaluate(expr, values)?;
                    values.insert(*id, value);
                }
                Action::Assign(place, expr) => {
                    let value = evaluate(expr, values)?;
                    let place = address(place, values)?;
                    if place.fields.is_empty() {
                        values.insert(place.root, value);
                    } else {
                        *place_mut(&place, values)? = value;
                    }
                }
                Action::Drop(expr) => {
                    drop(evaluate(expr, values)?);
                }
                Action::Release(ids) => {
                    for id in ids {
                        values.remove(id);
                    }
                }
                Action::Emit(expr) => {
                    let Value::String(message) = evaluate(expr, values)? else {
                        return Err(error("interpreter invariant: emit type"));
                    };
                    self.events.push(Event::Emit { message });
                }
                Action::Exit(outcome, args) => {
                    return Ok(Flow::Exit(
                        outcome.clone(),
                        args.iter()
                            .map(|a| evaluate(a, values))
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
                        .map(|(id, e)| Ok((*id, evaluate(e, values)?)))
                        .collect::<Result<Vec<_>>>()?;
                    for (id, value) in payloads {
                        values.insert(id, value);
                    }
                    for id in release {
                        values.remove(id);
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
                        .map(|a| evaluate(a, values))
                        .collect::<Result<Vec<_>>>()?;
                    let (outcome, fields) = match operation {
                        Operation::Collection(name) => collection(name, args, values)?,
                        Operation::Region(name) => self.invoke(name, args, values)?,
                        Operation::Variant(expr) => match evaluate(expr, values)? {
                            Value::Bool(b) => (b.to_string(), vec![]),
                            Value::Construct { name, fields, .. } => (name, fields),
                            _ => return Err(error("interpreter invariant: decision operand")),
                        },
                        Operation::BorrowedVariant(expr) => {
                            let Value::Reference { ty, place } = evaluate(expr, values)? else {
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
                                    vec![evaluate(&expression, values)?],
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
                        values.insert(*slot, value);
                    }
                }
            }
        }
        Ok(Flow::Fallthrough)
    }
}
impl CheckedProgram {
    pub fn run(&self, scenario: &Scenario) -> Result<Execution> {
        self.run_with_limit(scenario, 1_000_000)
    }
    pub fn run_with_limit(&self, scenario: &Scenario, steps: usize) -> Result<Execution> {
        if self
            .region(&scenario.entry)
            .is_some_and(|r| r.parameters.iter().any(|p| p.ty.starts_with('&')))
        {
            return Err(error("entry controls cannot require borrowed arguments"));
        }
        let mut machine = Machine {
            program: self,
            calls: &scenario.calls,
            cursor: 0,
            events: vec![],
            steps_left: steps,
        };
        let (outcome, fields) = machine.invoke(
            &scenario.entry,
            scenario.arguments.clone(),
            &mut BTreeMap::new(),
        )?;
        if machine.cursor != scenario.calls.len() {
            return Err(error(format!(
                "{} unconsumed script calls",
                scenario.calls.len() - machine.cursor
            )));
        }
        Ok(Execution {
            outcome,
            fields,
            events: machine.events,
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
