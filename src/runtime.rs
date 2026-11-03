//! Deterministic reference interpreter. Primitive adapters are scripted trusted inputs.
use crate::{
    Diagnostic, Result, Span,
    ir::*,
    manifest::{Manifest, TypeDef},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod domains;
mod tasks;

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
        Value::Task { .. } => {
            return Err(error("pending tasks cannot cross a host value boundary"));
        }
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
        Expr::WithReferenceEvidence(value, _) => evaluate(value, values, base),
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
                .into_iter()
                .nth(*field)
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

mod continuation;
pub use continuation::{ExecutionProgress, ExecutionSession};
#[cfg(test)]
pub(crate) use continuation::{
    HttpResponse, census_model, http_candidate_fault_model, http_source_operation_model,
    http_source_operation_sequence_model, pending_fault_model, source_operation_model,
};

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
        let mut execution = self.start_execution(scenario, steps);
        loop {
            match execution.advance(usize::MAX) {
                ExecutionProgress::Paused => {}
                ExecutionProgress::Complete(result) => return result,
                ExecutionProgress::Finished => unreachable!("fresh execution completed once"),
            }
        }
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
