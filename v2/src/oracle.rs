//! The oracle: the instrumented semantics of `docs/v2/core.md`, section 6,
//! implemented directly.
//!
//! It runs a typed core program and is stuck on every violation the checker
//! is meant to exclude. Each rule below carries the name of its clause, and
//! each way of being stuck carries its number from the table of section 6.10.

use crate::host::Host;
use crate::int::Int;
use crate::syntax::escape_string;
use crate::typed::{
    Arithmetic, BodyId, BorrowMode, BoundaryId, BuiltinOp, Command, Comparison, Control,
    ControlBody, ControlId, Expr, Kind, Operation, OutcomeId, Place, PlaceStep, Program,
    ResourceId, Selector, SiteId, StructId, Type, VarId, VarRef, VariantId,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Value {
    Bool(bool),
    Int(Int),
    String(String),
    Struct {
        ty: StructId,
        name: String,
        fields: Vec<Value>,
    },
    Variant {
        ty: VariantId,
        name: String,
        constructor: usize,
        constructor_name: String,
        fields: Vec<Value>,
    },
    Vector(Vec<Value>),
    Map(BTreeMap<Value, Value>),
    Resource {
        ty: ResourceId,
        name: String,
        id: u64,
    },
    Outcome {
        ty: OutcomeId,
        alternative: usize,
        alternative_name: String,
        payload: Vec<Value>,
    },
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(value) => write!(formatter, "{value}"),
            Value::Int(value) => write!(formatter, "{value}"),
            Value::String(value) => write!(formatter, "\"{}\"", escape_string(value)),
            Value::Struct { name, fields, .. } => {
                write!(formatter, "{name} {{{}}}", list(fields))
            }
            Value::Variant {
                name,
                constructor_name,
                fields,
                ..
            } => {
                write!(formatter, "{name}.{constructor_name} {{{}}}", list(fields))
            }
            Value::Vector(items) => write!(formatter, "[{}]", list(items)),
            Value::Map(entries) => {
                if entries.is_empty() {
                    formatter.write_str("∅")
                } else {
                    let parts: Vec<String> =
                        entries.iter().map(|(k, v)| format!("{k} ↦ {v}")).collect();
                    write!(formatter, "{{{}}}", parts.join(", "))
                }
            }
            Value::Resource { name, id, .. } => write!(formatter, "{name}#{id}"),
            Value::Outcome {
                alternative_name,
                payload,
                ..
            } => write!(
                formatter,
                "outcome.{alternative_name} {{{}}}",
                list(payload)
            ),
        }
    }
}

pub fn list(values: &[Value]) -> String {
    values
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The parameters of a run that the program text does not fix.
#[derive(Clone, Debug)]
pub struct Config {
    /// The greatest `usize`. The fact logic bounds it between `2^32 - 1` and
    /// `2^64 - 1` and proves for every value between ([A2]); a run has one.
    pub usize_max: Int,
    /// The number of steps after which a run is reported as not ending.
    pub fuel: u64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            usize_max: Int::power_of_two(64).minus(&Int::from_u64(1)),
            fuel: 1_000_000,
        }
    }
}

impl Config {
    pub fn with_usize_bits(bits: u32) -> Config {
        Config {
            usize_max: Int::power_of_two(bits).minus(&Int::from_u64(1)),
            ..Config::default()
        }
    }

    pub fn fits(&self, value: &Value, ty: &Type) -> bool {
        match (value, ty.deref_type()) {
            (Value::Bool(_), Type::Bool) => true,
            (Value::Int(value), Type::Int(ty)) => {
                let (least, greatest) = ty.bounds(&self.usize_max);
                *value >= least && *value <= greatest
            }
            (Value::String(_), Type::String) => true,
            (Value::Struct { ty: sid, .. }, Type::Struct(exp_sid, _)) => sid == exp_sid,
            (Value::Variant { ty: vid, .. }, Type::Variant(exp_vid, _)) => vid == exp_vid,
            (Value::Vector(items), Type::Vector(elem_ty)) => {
                items.iter().all(|item| self.fits(item, elem_ty))
            }
            (Value::Map(entries), Type::Map(k_ty, v_ty)) => entries
                .iter()
                .all(|(k, v)| self.fits(k, k_ty) && self.fits(v, v_ty)),
            (Value::Resource { ty: rid, .. }, Type::Resource(exp_rid, _)) => rid == exp_rid,
            (Value::Outcome { ty: oid, .. }, Type::Outcome(exp_oid, _)) => oid == exp_oid,
            _ => false,
        }
    }

    pub fn fits_in_program(&self, program: &Program, value: &Value, ty: &Type) -> bool {
        match (value, ty.deref_type()) {
            (Value::Bool(_), Type::Bool) => true,
            (Value::Int(value), Type::Int(ty)) => {
                let (least, greatest) = ty.bounds(&self.usize_max);
                *value >= least && *value <= greatest
            }
            (Value::String(_), Type::String) => true,
            (
                Value::Struct {
                    ty: sid, fields, ..
                },
                Type::Struct(exp_sid, _),
            ) if sid == exp_sid => {
                let st = &program.structs[*sid];
                fields.len() == st.fields.len()
                    && fields
                        .iter()
                        .zip(&st.fields)
                        .all(|(v, f)| self.fits_in_program(program, v, &f.ty))
            }
            (
                Value::Variant {
                    ty: vid,
                    constructor,
                    fields,
                    ..
                },
                Type::Variant(exp_vid, _),
            ) if vid == exp_vid => {
                let var = &program.variants[*vid];
                let Some(ctor) = var.constructors.get(*constructor) else {
                    return false;
                };
                fields.len() == ctor.fields.len()
                    && fields
                        .iter()
                        .zip(&ctor.fields)
                        .all(|(v, f)| self.fits_in_program(program, v, &f.ty))
            }
            (Value::Vector(items), Type::Vector(elem_ty)) => items
                .iter()
                .all(|item| self.fits_in_program(program, item, elem_ty)),
            (Value::Map(entries), Type::Map(k_ty, v_ty)) => entries.iter().all(|(k, v)| {
                self.fits_in_program(program, k, k_ty) && self.fits_in_program(program, v, v_ty)
            }),
            (Value::Resource { ty: rid, .. }, Type::Resource(exp_rid, _)) => rid == exp_rid,
            (
                Value::Outcome {
                    ty: oid,
                    alternative,
                    payload,
                    ..
                },
                Type::Outcome(exp_oid, _),
            ) if oid == exp_oid => {
                let oc = &program.outcomes[*oid];
                let Some(alt) = oc.alternatives.get(*alternative) else {
                    return false;
                };
                payload.len() == alt.fields.len()
                    && payload
                        .iter()
                        .zip(&alt.fields)
                        .all(|(v, f)| self.fits_in_program(program, v, &f.ty))
            }
            _ => false,
        }
    }
}

/// One step of the host that had an effect ([E-Extern], [E-Unwind]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Event {
    pub control: String,
    pub arguments: Vec<Value>,
    pub alternative: String,
    pub payload: Vec<Value>,
}

impl fmt::Display for Event {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}({}) -> {} {{{}}}",
            self.control,
            list(&self.arguments),
            self.alternative,
            list(&self.payload)
        )
    }
}

/// A stuck configuration: which row of the table of section 6.10, the clause
/// that has no successor, and where.
#[derive(Clone, Debug)]
pub struct Stuck {
    pub state: u8,
    pub clause: &'static str,
    pub control: String,
    pub line: u32,
    pub detail: String,
}

/// How an execution ends ([6.11]), and the ways a run can fail to be one.
#[derive(Clone, Debug)]
pub enum End {
    Done {
        alternative: String,
        payload: Vec<Value>,
    },
    /// A fault unwound out of the entry control ([E-Root]).
    Faulted(String),
    Stuck(Stuck),
    /// The host broke a contract ([E-Host]). Not a stuck state.
    HostViolation(String),
    /// The host of this run had no answer. A defect of the test, not of the
    /// program.
    HostFailed(String),
    /// The fuel ran out. The instrumented profile's stand-in for divergence.
    OutOfFuel,
    /// The entry or its arguments do not start an execution (section 8).
    NotAnExecution(String),
}

impl fmt::Display for End {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            End::Done {
                alternative,
                payload,
            } => {
                write!(formatter, "done {alternative} {{{}}}", list(payload))
            }
            End::Faulted(message) => {
                write!(formatter, "faulted \"{}\"", escape_string(message))
            }
            End::Stuck(stuck) => write!(
                formatter,
                "stuck X{} {} at {}:{}: {}",
                stuck.state, stuck.clause, stuck.control, stuck.line, stuck.detail
            ),
            End::HostViolation(detail) => write!(formatter, "violation: {detail}"),
            End::HostFailed(detail) => write!(formatter, "host-failed: {detail}"),
            End::OutOfFuel => formatter.write_str("fuel"),
            End::NotAnExecution(detail) => write!(formatter, "not-an-execution: {detail}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Run {
    pub end: End,
    pub trace: Vec<Event>,
    pub steps: u64,
    /// How often each rule of the semantics was applied, by clause.
    pub rules: BTreeMap<&'static str, u64>,
}

struct Fail {
    state: u8,
    clause: &'static str,
    detail: String,
}

fn fail<T>(state: u8, clause: &'static str, detail: impl Into<String>) -> Result<T, Fail> {
    Err(Fail {
        state,
        clause,
        detail: detail.into(),
    })
}

trait Env {
    fn get(&self, var: VarRef) -> Result<Value, Fail>;
    fn get_old(&self, index: usize) -> Result<Value, Fail>;
}

/// The parameters of a signature and the payload of one alternative.
struct SignatureEnv<'a> {
    parameters: &'a [Value],
    old_parameters: &'a [Value],
    fields: &'a [Value],
}

impl Env for SignatureEnv<'_> {
    fn get(&self, var: VarRef) -> Result<Value, Fail> {
        let value = match var {
            VarRef::Parameter(index) => self.parameters.get(index),
            VarRef::Field(index) => self.fields.get(index),
            VarRef::Local(_) => None,
        };
        match value {
            Some(value) => Ok(value.clone()),
            None => fail(
                10,
                "E-Formula",
                "a signature refers to a name it does not have",
            ),
        }
    }

    fn get_old(&self, index: usize) -> Result<Value, Fail> {
        match self.old_parameters.get(index) {
            Some(value) => Ok(value.clone()),
            None => fail(
                10,
                "E-Formula",
                "a signature refers to an old parameter it does not have",
            ),
        }
    }
}

/// A slot in a scope (`docs/v2/core.md` section 6.1): available with value `v`,
/// or moved (`◌`).
#[derive(Clone, Debug)]
enum Slot {
    Available(Value),
    Moved,
}

/// Concrete evaluated path steps for writing back to a place.
#[derive(Clone, Debug)]
enum EvalStep {
    Field(usize),
    Index(usize),
}

#[derive(Clone, Debug)]
struct EvalPlace {
    root: VarId,
    steps: Vec<EvalStep>,
}

#[derive(Clone, Debug)]
enum ScopeKind {
    Normal,
    Loop {
        entry_parameters: Vec<Value>,
    },
    LexicalBoundary {
        site: SiteId,
        parent_pc: usize,
        captures: Vec<Value>,
        contingency_body: BodyId,
        contingency_vars: Vec<VarId>,
        message_var: Option<VarId>,
    },
    LexicalContingency {
        parent_pc: usize,
        fault_message: String,
    },
}

/// One run of one body ([6.2]).
struct Scope {
    body: BodyId,
    pc: usize,
    slots: Vec<(VarId, Slot)>,
    history: Vec<(SiteId, usize)>,
    kind: ScopeKind,
}

#[derive(Clone, Debug)]
enum ActivationKind {
    Control(ControlId),
    Boundary {
        id: BoundaryId,
        in_contingency: Option<String>,
    },
}

/// One call of a control or named boundary.
struct Activation {
    kind: ActivationKind,
    /// Arguments as they were at entry (`old(y)` in contract conditions, [E-Formula]).
    arguments: Vec<Value>,
    /// Writeback mappings `(param_var_id, caller_place)` for `&~` parameters.
    writebacks: Vec<(VarId, EvalPlace)>,
    scopes: Vec<Scope>,
}

struct ActivationEnv<'a> {
    body: &'a ControlBody,
    activation: &'a Activation,
}

impl Env for ActivationEnv<'_> {
    /// [E-Var]: a name is looked up from the innermost scope outward, and a
    /// name without a slot or with a moved slot `◌` is stuck.
    fn get(&self, var: VarRef) -> Result<Value, Fail> {
        let VarRef::Local(id) = var else {
            return fail(10, "E-Var", "a body refers to a name of a signature");
        };
        for scope in self.activation.scopes.iter().rev() {
            if let Some((_, slot)) = scope.slots.iter().find(|(slot, _)| *slot == id) {
                return match slot {
                    Slot::Available(value) => Ok(value.clone()),
                    Slot::Moved => fail(
                        2,
                        "E-Var",
                        format!(
                            "`{}` has been moved on this history",
                            self.body.vars[id].name
                        ),
                    ),
                };
            }
        }
        fail(
            2,
            "E-Var",
            format!("`{}` has no value on this history", self.body.vars[id].name),
        )
    }

    fn get_old(&self, index: usize) -> Result<Value, Fail> {
        match self.activation.arguments.get(index) {
            Some(v) => Ok(v.clone()),
            None => fail(10, "E-Formula", "no entry argument for `old`"),
        }
    }
}

struct Evaluator<'a> {
    program: &'a Program,
    config: &'a Config,
}

fn integer(value: Value) -> Result<Int, Fail> {
    match value {
        Value::Int(value) => Ok(value),
        _ => fail(
            10,
            "E-Arith",
            "a non-integer stands where an integer is needed",
        ),
    }
}

fn boolean(value: Value) -> Result<bool, Fail> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => fail(10, "E-Test", "a non-bool stands where a bool is needed"),
    }
}

fn compare(comparison: Comparison, left: &Value, right: &Value) -> Result<bool, Fail> {
    Ok(match (left, right) {
        (Value::Int(a), Value::Int(b)) => match comparison {
            Comparison::Equal => a == b,
            Comparison::NotEqual => a != b,
            Comparison::Less => a < b,
            Comparison::LessEqual => a <= b,
            Comparison::Greater => a > b,
            Comparison::GreaterEqual => a >= b,
        },
        (a, b) => match comparison {
            Comparison::Equal => a == b,
            Comparison::NotEqual => a != b,
            _ => return fail(10, "E-Arith", "values of this type are not ordered"),
        },
    })
}

fn sign(operator: Arithmetic) -> &'static str {
    match operator {
        Arithmetic::Add => "+",
        Arithmetic::Subtract => "-",
        Arithmetic::Multiply => "*",
        Arithmetic::Divide => "÷",
        Arithmetic::Remainder => "mod",
    }
}

fn value_len(value: &Value) -> Result<Int, Fail> {
    match value {
        Value::Vector(items) => Ok(Int::from_u64(items.len() as u64)),
        Value::String(s) => Ok(Int::from_u64(s.len() as u64)),
        Value::Map(entries) => Ok(Int::from_u64(entries.len() as u64)),
        _ => fail(10, "E-Arith", "`len` applied to a value without a length"),
    }
}

impl Evaluator<'_> {
    /// `e ⇓ v` in a pure read context (such as inside a function body).
    fn value(&self, expr: &Expr, env: &dyn Env) -> Result<Value, Fail> {
        Ok(match expr {
            Expr::Boolean(value) => Value::Bool(*value),
            Expr::Integer(value, _) => Value::Int(value.clone()),
            Expr::String(value) => Value::String(value.clone()),
            Expr::Var(var, _) => env.get(*var)?,
            Expr::Old(idx, _) => env.get_old(*idx)?,
            Expr::Move(inner, _) => self.value(inner, env)?,
            Expr::Borrow(_, place, _) => {
                let mut current = env.get(VarRef::Local(place.root))?;
                for step in &place.steps {
                    match step {
                        PlaceStep::Field(_, fidx, _) => {
                            let Value::Struct { mut fields, .. } = current else {
                                return fail(10, "E-Field", "field access on non-struct");
                            };
                            current = fields.swap_remove(*fidx);
                        }
                        PlaceStep::Index(idx_expr, _) => {
                            let Value::Vector(mut items) = current else {
                                return fail(10, "E-Index", "index on non-vector");
                            };
                            let idx = integer(self.value(idx_expr, env)?)?;
                            let Some(i) = idx
                                .to_i128()
                                .filter(|&i| i >= 0 && (i as usize) < items.len())
                            else {
                                return fail(
                                    7,
                                    "E-Index",
                                    format!(
                                        "index {idx} is out of bounds for length {}",
                                        items.len()
                                    ),
                                );
                            };
                            current = items.swap_remove(i as usize);
                        }
                    }
                }
                current
            }
            Expr::Field(base, _, fidx, _) => {
                let base_val = self.value(base, env)?;
                let Value::Struct { mut fields, .. } = base_val else {
                    return fail(10, "E-Field", "field access on non-struct");
                };
                fields.swap_remove(*fidx)
            }
            Expr::Index(base, idx_expr, _) => {
                let base_val = self.value(base, env)?;
                let Value::Vector(mut items) = base_val else {
                    return fail(10, "E-Index", "index on non-vector");
                };
                let idx = integer(self.value(idx_expr, env)?)?;
                let Some(i) = idx
                    .to_i128()
                    .filter(|&i| i >= 0 && (i as usize) < items.len())
                else {
                    return fail(
                        7,
                        "E-Index",
                        format!("index {idx} is out of bounds for length {}", items.len()),
                    );
                };
                items.swap_remove(i as usize)
            }
            Expr::Struct(sid, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.value(f, env)?);
                }
                Value::Struct {
                    ty: *sid,
                    name: self.program.structs[*sid].name.clone(),
                    fields: vals,
                }
            }
            Expr::Variant(vid, cidx, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.value(f, env)?);
                }
                let vdef = &self.program.variants[*vid];
                Value::Variant {
                    ty: *vid,
                    name: vdef.name.clone(),
                    constructor: *cidx,
                    constructor_name: vdef.constructors[*cidx].name.clone(),
                    fields: vals,
                }
            }
            Expr::Vector(items, _) => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.value(item, env)?);
                }
                Value::Vector(vals)
            }
            Expr::EmptyMap(_) => Value::Map(BTreeMap::new()),
            Expr::Len(place) => Value::Int(value_len(&self.value(place, env)?)?),
            Expr::Clone(place, _) => self.value(place, env)?,
            Expr::Trim(arg) => {
                let Value::String(s) = self.value(arg, env)? else {
                    return fail(10, "E-Fn", "`trim` expects a string");
                };
                Value::String(s.trim().to_string())
            }
            Expr::Not(operand) => Value::Bool(!boolean(self.value(operand, env)?)?),
            Expr::Negate(operand, ty) => {
                let operand = integer(self.value(operand, env)?)?;
                let result = operand.negate();
                if !self
                    .config
                    .fits(&Value::Int(result.clone()), &Type::Int(*ty))
                {
                    return fail(
                        7,
                        "E-Arith",
                        format!("-{operand} = {result} is not a value of {}", ty.name()),
                    );
                }
                Value::Int(result)
            }
            Expr::Arithmetic(operator, left, right, ty) => {
                let left = integer(self.value(left, env)?)?;
                let right = integer(self.value(right, env)?)?;
                let result = match operator {
                    Arithmetic::Add => left.plus(&right),
                    Arithmetic::Subtract => left.minus(&right),
                    Arithmetic::Multiply => left.times(&right),
                    Arithmetic::Divide | Arithmetic::Remainder => {
                        let Some((quotient, remainder)) = left.divide(&right) else {
                            return fail(
                                7,
                                "E-Arith",
                                format!("{left} {} 0: the divisor is zero", sign(*operator)),
                            );
                        };
                        if *operator == Arithmetic::Divide {
                            quotient
                        } else {
                            remainder
                        }
                    }
                };
                if !self
                    .config
                    .fits(&Value::Int(result.clone()), &Type::Int(*ty))
                {
                    return fail(
                        7,
                        "E-Arith",
                        format!(
                            "{left} {} {right} = {result} is not a value of {}",
                            sign(*operator),
                            ty.name()
                        ),
                    );
                }
                Value::Int(result)
            }
            Expr::Compare(comparison, left, right) => {
                let left = self.value(left, env)?;
                let right = self.value(right, env)?;
                Value::Bool(compare(*comparison, &left, &right)?)
            }
            Expr::And(left, right) => {
                Value::Bool(boolean(self.value(left, env)?)? && boolean(self.value(right, env)?)?)
            }
            Expr::Or(left, right) => {
                Value::Bool(boolean(self.value(left, env)?)? || boolean(self.value(right, env)?)?)
            }
            Expr::Call(id, arguments, _) => {
                let function = &self.program.functions[*id];
                let mut values = Vec::new();
                for argument in arguments {
                    values.push(self.value(argument, env)?);
                }
                let inner = SignatureEnv {
                    parameters: &values,
                    old_parameters: &values,
                    fields: &[],
                };
                if let Some(precondition) = &function.precondition
                    && !self.holds(precondition, &inner)?
                {
                    return fail(
                        7,
                        "E-Fn",
                        format!(
                            "the precondition of `{}` is false for ({})",
                            function.name,
                            list(&values)
                        ),
                    );
                }
                self.value(&function.body, &inner)?
            }
        })
    }

    /// The value of a term of a formula ([E-Formula]): over the mathematical
    /// integers, total, and with no obligation of its own.
    fn term(&self, expr: &Expr, env: &dyn Env) -> Result<Value, Fail> {
        Ok(match expr {
            Expr::Boolean(value) => Value::Bool(*value),
            Expr::Integer(value, _) => Value::Int(value.clone()),
            Expr::String(value) => Value::String(value.clone()),
            Expr::Var(var, _) => env.get(*var)?,
            Expr::Old(idx, _) => env.get_old(*idx)?,
            Expr::Move(inner, _) => self.term(inner, env)?,
            Expr::Borrow(_, place, _) => {
                let mut current = env.get(VarRef::Local(place.root))?;
                for step in &place.steps {
                    match step {
                        PlaceStep::Field(_, fidx, _) => {
                            if let Value::Struct { mut fields, .. } = current {
                                current = fields.swap_remove(*fidx);
                            }
                        }
                        PlaceStep::Index(idx_expr, elem_ty) => {
                            if let Value::Vector(mut items) = current {
                                let idx = integer(self.term(idx_expr, env)?)?;
                                current = match idx
                                    .to_i128()
                                    .filter(|&i| i >= 0 && (i as usize) < items.len())
                                {
                                    Some(i) => items.swap_remove(i as usize),
                                    None => self.default_formula_value(elem_ty),
                                };
                            }
                        }
                    }
                }
                current
            }
            Expr::Field(base, _, fidx, _) => {
                let Value::Struct { mut fields, .. } = self.term(base, env)? else {
                    return fail(10, "E-Formula", "field access on non-struct in formula");
                };
                fields.swap_remove(*fidx)
            }
            Expr::Index(base, idx_expr, elem_ty) => {
                let Value::Vector(mut items) = self.term(base, env)? else {
                    return fail(10, "E-Formula", "index on non-vector in formula");
                };
                let idx = integer(self.term(idx_expr, env)?)?;
                // [E-Formula]: an out-of-range index in a formula denotes a
                // deterministic value of the element type.
                match idx
                    .to_i128()
                    .filter(|&i| i >= 0 && (i as usize) < items.len())
                {
                    Some(i) => items.swap_remove(i as usize),
                    None => self.default_formula_value(elem_ty),
                }
            }
            Expr::Struct(sid, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.term(f, env)?);
                }
                Value::Struct {
                    ty: *sid,
                    name: self.program.structs[*sid].name.clone(),
                    fields: vals,
                }
            }
            Expr::Variant(vid, cidx, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.term(f, env)?);
                }
                let vdef = &self.program.variants[*vid];
                Value::Variant {
                    ty: *vid,
                    name: vdef.name.clone(),
                    constructor: *cidx,
                    constructor_name: vdef.constructors[*cidx].name.clone(),
                    fields: vals,
                }
            }
            Expr::Vector(items, _) => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.term(item, env)?);
                }
                Value::Vector(vals)
            }
            Expr::EmptyMap(_) => Value::Map(BTreeMap::new()),
            Expr::Len(place) => Value::Int(value_len(&self.term(place, env)?)?),
            Expr::Clone(place, _) => self.term(place, env)?,
            Expr::Trim(arg) => {
                let Value::String(s) = self.term(arg, env)? else {
                    return fail(10, "E-Formula", "`trim` expects a string");
                };
                Value::String(s.trim().to_string())
            }
            Expr::Not(operand) => Value::Bool(!boolean(self.term(operand, env)?)?),
            Expr::Negate(operand, _) => Value::Int(integer(self.term(operand, env)?)?.negate()),
            Expr::Arithmetic(operator, left, right, _) => {
                let left = integer(self.term(left, env)?)?;
                let right = integer(self.term(right, env)?)?;
                Value::Int(match operator {
                    Arithmetic::Add => left.plus(&right),
                    Arithmetic::Subtract => left.minus(&right),
                    Arithmetic::Multiply => left.times(&right),
                    Arithmetic::Divide => left.divide(&right).map_or(Int::zero(), |pair| pair.0),
                    Arithmetic::Remainder => {
                        left.divide(&right).map_or(left.clone(), |pair| pair.1)
                    }
                })
            }
            Expr::Compare(comparison, left, right) => {
                let left = self.term(left, env)?;
                let right = self.term(right, env)?;
                Value::Bool(compare(*comparison, &left, &right)?)
            }
            Expr::And(left, right) => {
                let left = boolean(self.term(left, env)?)?;
                let right = boolean(self.term(right, env)?)?;
                Value::Bool(left && right)
            }
            Expr::Or(left, right) => {
                let left = boolean(self.term(left, env)?)?;
                let right = boolean(self.term(right, env)?)?;
                Value::Bool(left || right)
            }
            Expr::Call(id, arguments, _) => {
                let mut values = Vec::new();
                for argument in arguments {
                    values.push(self.term(argument, env)?);
                }
                let inner = SignatureEnv {
                    parameters: &values,
                    old_parameters: &values,
                    fields: &[],
                };
                self.term(&self.program.functions[*id].body, &inner)?
            }
        })
    }

    fn default_formula_value(&self, ty: &Type) -> Value {
        match ty.deref_type() {
            Type::Bool => Value::Bool(false),
            Type::Int(_) => Value::Int(Int::zero()),
            Type::String => Value::String(String::new()),
            Type::Struct(sid, name) => {
                let fields = self.program.structs[*sid]
                    .fields
                    .iter()
                    .map(|f| self.default_formula_value(&f.ty))
                    .collect();
                Value::Struct {
                    ty: *sid,
                    name: name.clone(),
                    fields,
                }
            }
            Type::Variant(vid, name) => {
                let ctor = &self.program.variants[*vid].constructors[0];
                let fields = ctor
                    .fields
                    .iter()
                    .map(|f| self.default_formula_value(&f.ty))
                    .collect();
                Value::Variant {
                    ty: *vid,
                    name: name.clone(),
                    constructor: 0,
                    constructor_name: ctor.name.clone(),
                    fields,
                }
            }
            Type::Vector(_) => Value::Vector(Vec::new()),
            Type::Map(_, _) => Value::Map(BTreeMap::new()),
            Type::Resource(rid, name) => Value::Resource {
                ty: *rid,
                name: name.clone(),
                id: 0,
            },
            Type::Outcome(oid, _) => {
                let alt = &self.program.outcomes[*oid].alternatives[0];
                let payload = alt
                    .fields
                    .iter()
                    .map(|f| self.default_formula_value(&f.ty))
                    .collect();
                Value::Outcome {
                    ty: *oid,
                    alternative: 0,
                    alternative_name: alt.name.clone(),
                    payload,
                }
            }
            _ => Value::Int(Int::zero()),
        }
    }

    fn holds(&self, formula: &Expr, env: &dyn Env) -> Result<bool, Fail> {
        boolean(self.term(formula, env)?)
    }
}

struct Machine<'a> {
    program: &'a Program,
    config: &'a Config,
    host: &'a mut dyn Host,
    stack: Vec<Activation>,
    trace: Vec<Event>,
    steps: u64,
    rules: BTreeMap<&'static str, u64>,
    /// What each pure extern control answered, by its arguments ([E-Host]).
    answers: HashMap<(ControlId, Vec<Value>), (usize, Vec<Value>)>,
    /// Resource table: `(ResourceId, handle_id)` -> `true` while open, `false` once closed ([E-Resource]).
    resources: BTreeMap<(ResourceId, u64), bool>,
    /// Where the statement being run is, for the report of a stuck state.
    location: (String, u32),
}

/// What a step of the host leads to.
enum Answered {
    Completed(usize, Vec<Value>),
    Ended(End),
}

fn collect_resources(value: &Value, out: &mut Vec<(ResourceId, u64)>) {
    match value {
        Value::Resource { ty, id, .. } => out.push((*ty, *id)),
        Value::Struct { fields, .. }
        | Value::Variant { fields, .. }
        | Value::Vector(fields)
        | Value::Outcome {
            payload: fields, ..
        } => {
            for f in fields {
                collect_resources(f, out);
            }
        }
        Value::Map(entries) => {
            for (k, v) in entries {
                collect_resources(k, out);
                collect_resources(v, out);
            }
        }
        Value::Bool(_) | Value::Int(_) | Value::String(_) => {}
    }
}

impl<'a> Machine<'a> {
    fn count(&mut self, clause: &'static str) {
        *self.rules.entry(clause).or_insert(0) += 1;
    }

    fn evaluator(&self) -> Evaluator<'a> {
        Evaluator {
            program: self.program,
            config: self.config,
        }
    }

    fn body(&self) -> &'a ControlBody {
        let act = self.stack.last().expect("an activation");
        match &act.kind {
            ActivationKind::Control(cid) => self.program.controls[*cid]
                .body
                .as_ref()
                .expect("a defined control"),
            ActivationKind::Boundary { id, in_contingency } => {
                if in_contingency.is_some() {
                    &self.program.boundaries[*id].contingency
                } else {
                    &self.program.boundaries[*id].body
                }
            }
        }
    }

    fn current_name(&self) -> String {
        let act = self.stack.last().expect("an activation");
        match &act.kind {
            ActivationKind::Control(cid) => self.program.controls[*cid].name.clone(),
            ActivationKind::Boundary { id, .. } => self.program.boundaries[*id].name.clone(),
        }
    }

    fn scope(&mut self) -> &mut Scope {
        self.stack
            .last_mut()
            .expect("an activation")
            .scopes
            .last_mut()
            .expect("a scope")
    }

    /// Whether `value` contains an open resource or a captured outcome ([6.8]).
    fn contains_obligation(&self, value: &Value) -> bool {
        match value {
            Value::Resource { ty, id, .. } => {
                self.resources.get(&(*ty, *id)).copied().unwrap_or(false)
            }
            Value::Outcome { .. } => true,
            Value::Struct { fields, .. }
            | Value::Variant { fields, .. }
            | Value::Vector(fields) => fields.iter().any(|f| self.contains_obligation(f)),
            Value::Map(entries) => entries
                .iter()
                .any(|(k, v)| self.contains_obligation(k) || self.contains_obligation(v)),
            Value::Bool(_) | Value::Int(_) | Value::String(_) => false,
        }
    }

    /// Evaluates an expression `expr` in the current activation, moving slots
    /// when `consumed` is true and the variable's type does not have kind
    /// `copy` ([E-Var], [O3]), and checking temporary obligations when
    /// `consumed` is false ([E-Temp]).
    fn eval_expr(&mut self, expr: &Expr, consumed: bool) -> Result<Value, Fail> {
        let is_temp = matches!(
            expr,
            Expr::Struct(_, _, _)
                | Expr::Variant(_, _, _, _)
                | Expr::Vector(_, _)
                | Expr::EmptyMap(_)
                | Expr::Clone(_, _)
                | Expr::Call(_, _, _)
        );
        let value = match expr {
            Expr::Boolean(v) => Value::Bool(*v),
            Expr::Integer(v, _) => Value::Int(v.clone()),
            Expr::String(v) => Value::String(v.clone()),
            Expr::Var(VarRef::Local(id), ty) => {
                let var_name = self.body().vars[*id].name.clone();
                let is_copy = self.program.kind(ty) == Kind::Copy;
                let activation = self.stack.last_mut().expect("an activation");
                let mut found: Option<&mut Slot> = None;
                for scope in activation.scopes.iter_mut().rev() {
                    if let Some((_, slot)) = scope.slots.iter_mut().find(|(s, _)| *s == *id) {
                        found = Some(slot);
                        break;
                    }
                }
                let Some(slot) = found else {
                    return fail(
                        2,
                        "E-Var",
                        format!("`{var_name}` has no value on this history"),
                    );
                };
                match slot {
                    Slot::Moved => {
                        return fail(
                            2,
                            "E-Var",
                            format!("`{var_name}` has been moved on this history"),
                        );
                    }
                    Slot::Available(val) => {
                        let out = val.clone();
                        if consumed && !is_copy {
                            *slot = Slot::Moved;
                        }
                        self.count("E-Var");
                        out
                    }
                }
            }
            Expr::Var(_, _) | Expr::Old(_, _) => {
                return fail(10, "E-Var", "a body refers to a name of a signature");
            }
            Expr::Move(inner, _) => self.eval_expr(inner, true)?,
            Expr::Borrow(_, place, _) => {
                let eval_place = self.eval_place(place)?;
                self.read_eval_place(&eval_place)?
            }
            Expr::Field(base, _, fidx, fty) => {
                if consumed && self.program.kind(fty) != Kind::Copy {
                    return fail(
                        2,
                        "E-Field",
                        "cannot partially move a non-copy field out of a struct",
                    );
                }
                let base_val = self.eval_expr(base, false)?;
                let Value::Struct { mut fields, .. } = base_val else {
                    return fail(10, "E-Field", "field access on non-struct");
                };
                self.count("E-Field");
                fields.swap_remove(*fidx)
            }
            Expr::Index(base, idx_expr, ety) => {
                if consumed && self.program.kind(ety) != Kind::Copy {
                    return fail(
                        2,
                        "E-Index",
                        "cannot partially move a non-copy element out of a vector",
                    );
                }
                let base_val = self.eval_expr(base, false)?;
                let idx_val = integer(self.eval_expr(idx_expr, false)?)?;
                let Value::Vector(mut items) = base_val else {
                    return fail(10, "E-Index", "index on non-vector");
                };
                let Some(i) = idx_val
                    .to_i128()
                    .filter(|&i| i >= 0 && (i as usize) < items.len())
                else {
                    return fail(
                        7,
                        "E-Index",
                        format!(
                            "index {idx_val} is out of bounds for length {}",
                            items.len()
                        ),
                    );
                };
                self.count("E-Index");
                items.swap_remove(i as usize)
            }
            Expr::Struct(sid, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.eval_expr(f, true)?);
                }
                self.count("E-Build");
                Value::Struct {
                    ty: *sid,
                    name: self.program.structs[*sid].name.clone(),
                    fields: vals,
                }
            }
            Expr::Variant(vid, cidx, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.eval_expr(f, true)?);
                }
                self.count("E-Build");
                let vdef = &self.program.variants[*vid];
                Value::Variant {
                    ty: *vid,
                    name: vdef.name.clone(),
                    constructor: *cidx,
                    constructor_name: vdef.constructors[*cidx].name.clone(),
                    fields: vals,
                }
            }
            Expr::Vector(items, _) => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.eval_expr(item, true)?);
                }
                self.count("E-Build");
                Value::Vector(vals)
            }
            Expr::EmptyMap(_) => {
                self.count("E-Build");
                Value::Map(BTreeMap::new())
            }
            Expr::Len(place) => {
                let v = self.eval_expr(place, false)?;
                Value::Int(value_len(&v)?)
            }
            Expr::Clone(place, _) => {
                let v = self.eval_expr(place, false)?;
                self.count("E-Build");
                v
            }
            Expr::Trim(arg) => {
                let Value::String(s) = self.eval_expr(arg, false)? else {
                    return fail(10, "E-Fn", "`trim` expects a string");
                };
                Value::String(s.trim().to_string())
            }
            Expr::Not(operand) => Value::Bool(!boolean(self.eval_expr(operand, false)?)?),
            Expr::Negate(operand, ty) => {
                let operand = integer(self.eval_expr(operand, false)?)?;
                let result = operand.negate();
                if !self
                    .config
                    .fits(&Value::Int(result.clone()), &Type::Int(*ty))
                {
                    return fail(
                        7,
                        "E-Arith",
                        format!("-{operand} = {result} is not a value of {}", ty.name()),
                    );
                }
                Value::Int(result)
            }
            Expr::Arithmetic(operator, left, right, ty) => {
                let left = integer(self.eval_expr(left, false)?)?;
                let right = integer(self.eval_expr(right, false)?)?;
                let result = match operator {
                    Arithmetic::Add => left.plus(&right),
                    Arithmetic::Subtract => left.minus(&right),
                    Arithmetic::Multiply => left.times(&right),
                    Arithmetic::Divide | Arithmetic::Remainder => {
                        let Some((quotient, remainder)) = left.divide(&right) else {
                            return fail(
                                7,
                                "E-Arith",
                                format!("{left} {} 0: the divisor is zero", sign(*operator)),
                            );
                        };
                        if *operator == Arithmetic::Divide {
                            quotient
                        } else {
                            remainder
                        }
                    }
                };
                if !self
                    .config
                    .fits(&Value::Int(result.clone()), &Type::Int(*ty))
                {
                    return fail(
                        7,
                        "E-Arith",
                        format!(
                            "{left} {} {right} = {result} is not a value of {}",
                            sign(*operator),
                            ty.name()
                        ),
                    );
                }
                Value::Int(result)
            }
            Expr::Compare(comparison, left, right) => {
                let left = self.eval_expr(left, false)?;
                let right = self.eval_expr(right, false)?;
                Value::Bool(compare(*comparison, &left, &right)?)
            }
            Expr::And(left, right) => Value::Bool(
                boolean(self.eval_expr(left, false)?)? && boolean(self.eval_expr(right, false)?)?,
            ),
            Expr::Or(left, right) => Value::Bool(
                boolean(self.eval_expr(left, false)?)? || boolean(self.eval_expr(right, false)?)?,
            ),
            Expr::Call(id, arguments, _) => {
                let function = &self.program.functions[*id];
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    values.push(self.eval_expr(argument, false)?);
                }
                let inner = SignatureEnv {
                    parameters: &values,
                    old_parameters: &values,
                    fields: &[],
                };
                if let Some(precondition) = &function.precondition
                    && !self.evaluator().holds(precondition, &inner)?
                {
                    return fail(
                        7,
                        "E-Fn",
                        format!(
                            "the precondition of `{}` is false for ({})",
                            function.name,
                            list(&values)
                        ),
                    );
                }
                self.evaluator().value(&function.body, &inner)?
            }
        };
        // [E-Temp]: A temporary value that was read and not consumed is
        // destroyed when its reader is done; if it contains an obligation, stuck.
        if !consumed && is_temp && self.contains_obligation(&value) {
            return fail(
                5,
                "E-Temp",
                "a temporary value containing an obligation was not consumed",
            );
        }
        Ok(value)
    }

    fn eval_place(&mut self, place: &Place) -> Result<EvalPlace, Fail> {
        let mut steps = Vec::with_capacity(place.steps.len());
        for step in &place.steps {
            match step {
                PlaceStep::Field(_, fidx, _) => steps.push(EvalStep::Field(*fidx)),
                PlaceStep::Index(idx_expr, _) => {
                    let idx_val = integer(self.eval_expr(idx_expr, false)?)?;
                    let Some(i) = idx_val.to_i128().filter(|&i| i >= 0) else {
                        return fail(7, "E-Index", format!("index {idx_val} is negative"));
                    };
                    steps.push(EvalStep::Index(i as usize));
                }
            }
        }
        Ok(EvalPlace {
            root: place.root,
            steps,
        })
    }

    fn read_eval_place(&self, place: &EvalPlace) -> Result<Value, Fail> {
        let var_name = &self.body().vars[place.root].name;
        let activation = self.stack.last().expect("an activation");
        let mut root_val: Option<&Value> = None;
        for scope in activation.scopes.iter().rev() {
            if let Some((_, slot)) = scope.slots.iter().find(|(s, _)| *s == place.root) {
                match slot {
                    Slot::Available(v) => {
                        root_val = Some(v);
                        break;
                    }
                    Slot::Moved => {
                        return fail(
                            2,
                            "E-Var",
                            format!("`{var_name}` has been moved on this history"),
                        );
                    }
                }
            }
        }
        let Some(mut current) = root_val else {
            return fail(
                2,
                "E-Var",
                format!("`{var_name}` has no value on this history"),
            );
        };
        for step in &place.steps {
            match step {
                EvalStep::Field(fidx) => {
                    let Value::Struct { fields, .. } = current else {
                        return fail(10, "E-Field", "field access on non-struct");
                    };
                    current = &fields[*fidx];
                }
                EvalStep::Index(idx) => {
                    let Value::Vector(items) = current else {
                        return fail(10, "E-Index", "index on non-vector");
                    };
                    let Some(elem) = items.get(*idx) else {
                        return fail(
                            7,
                            "E-Index",
                            format!("index {idx} is out of bounds for length {}", items.len()),
                        );
                    };
                    current = elem;
                }
            }
        }
        Ok(current.clone())
    }

    /// Writes `new_val` at `place` in the top activation ([E-Assign]).
    /// If `check_replaced_obligation` is true, checks that the overwritten
    /// value does not contain an obligation (X5 E-Assign).
    fn write_eval_place(
        &mut self,
        place: &EvalPlace,
        new_val: Value,
        check_replaced_obligation: bool,
    ) -> Result<(), Fail> {
        let var_name = self.body().vars[place.root].name.clone();
        let activation = self.stack.last_mut().expect("an activation");
        let slot = activation
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.slots.iter_mut().find(|(s, _)| *s == place.root));
        let Some((_, slot)) = slot else {
            return fail(
                2,
                "E-Assign",
                format!("`{var_name}` has no slot on this history"),
            );
        };
        if place.steps.is_empty() {
            if check_replaced_obligation && let Slot::Available(old_val) = slot {
                let old_clone = old_val.clone();
                if self.contains_obligation(&old_clone) {
                    return fail(
                        5,
                        "E-Assign",
                        format!("overwriting `{var_name}` destroys an uncompleted obligation"),
                    );
                }
            }
            let activation = self.stack.last_mut().expect("an activation");
            let (_, slot) = activation
                .scopes
                .iter_mut()
                .rev()
                .find_map(|scope| scope.slots.iter_mut().find(|(s, _)| *s == place.root))
                .unwrap();
            *slot = Slot::Available(new_val);
            return Ok(());
        }

        // Component assignment: root must be available.
        let mut root_val = match slot {
            Slot::Available(v) => v.clone(),
            Slot::Moved => {
                return fail(
                    2,
                    "E-Assign",
                    format!("`{var_name}` has been moved on this history"),
                );
            }
        };
        let replaced = update_nested(&mut root_val, &place.steps, new_val)?;
        if check_replaced_obligation && self.contains_obligation(&replaced) {
            return fail(
                5,
                "E-Assign",
                format!("overwriting part of `{var_name}` destroys an uncompleted obligation"),
            );
        }
        let activation = self.stack.last_mut().expect("an activation");
        let (_, slot) = activation
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.slots.iter_mut().find(|(s, _)| *s == place.root))
            .unwrap();
        *slot = Slot::Available(root_val);
        Ok(())
    }

    /// Evaluates operands according to `[E-Operands]`: operands that do not
    /// lend are evaluated first, left to right, and operands that lend (`&p` or
    /// `&~p`) afterwards, left to right.
    #[allow(clippy::type_complexity)]
    fn eval_operands(
        &mut self,
        exprs: &[Expr],
        param_types: Option<&[Type]>,
    ) -> Result<(Vec<Value>, Vec<(VarId, EvalPlace)>), Fail> {
        let mut results: Vec<Option<Value>> = vec![None; exprs.len()];
        let mut writebacks = Vec::new();
        let is_lending = |i: usize, e: &Expr| -> bool {
            e.lends()
                || param_types
                    .and_then(|tys| tys.get(i))
                    .is_some_and(|ty| matches!(ty, Type::Ref(_, _)))
        };
        // Pass 1: non-lending operands (consumed by value).
        for (i, expr) in exprs.iter().enumerate() {
            if !is_lending(i, expr) {
                results[i] = Some(self.eval_expr(expr, true)?);
            }
        }
        // Pass 2: lending operands (read or exclusively borrowed).
        for (i, expr) in exprs.iter().enumerate() {
            if is_lending(i, expr) {
                if let Expr::Borrow(BorrowMode::Exclusive, place, _) = expr {
                    let eval_place = self.eval_place(place)?;
                    let val = self.read_eval_place(&eval_place)?;
                    writebacks.push((i, eval_place));
                    results[i] = Some(val);
                } else {
                    results[i] = Some(self.eval_expr(expr, false)?);
                }
                self.count("E-Borrow");
            }
        }
        if exprs.len() > 1 && exprs.iter().enumerate().any(|(i, e)| is_lending(i, e)) {
            self.count("E-Operands");
        }
        let values = results.into_iter().map(|v| v.expect("evaluated")).collect();
        Ok((values, writebacks))
    }

    fn formula(&self, formula: &Expr) -> Result<bool, Fail> {
        let activation = self.stack.last().expect("an activation");
        let env = ActivationEnv {
            body: self.body(),
            activation,
        };
        self.evaluator().holds(formula, &env)
    }

    /// [E-Bind]: a name gets its slot once in a run of its body.
    fn bind(&mut self, var: VarId, value: Value) -> Result<(), Fail> {
        let name = self.body().vars[var].name.clone();
        let scope = self.scope();
        if scope.slots.iter().any(|(slot, _)| *slot == var) {
            return fail(
                3,
                "E-Bind",
                format!("`{name}` is bound twice in one run of its body"),
            );
        }
        scope.slots.push((var, Slot::Available(value)));
        Ok(())
    }

    /// [E-Scope]: ends a scope normally; stuck with X5 if any available slot
    /// contains an uncompleted obligation.
    fn end_scope(&mut self, scope: &Scope) -> Result<(), Fail> {
        for (var_id, slot) in &scope.slots {
            if let Slot::Available(val) = slot
                && self.contains_obligation(val)
            {
                let var_name = &self.body().vars[*var_id].name;
                return fail(
                    5,
                    "E-Scope",
                    format!(
                        "`{var_name}` holds an uncompleted obligation `{val}` when its scope ends"
                    ),
                );
            }
        }
        if let ScopeKind::LexicalBoundary { captures, .. } = &scope.kind {
            for val in captures {
                if self.contains_obligation(val) {
                    return fail(
                        5,
                        "E-Scope",
                        format!(
                            "boundary capture `{val}` holds an uncompleted obligation when the boundary ends"
                        ),
                    );
                }
            }
        }
        self.count("E-Scope");
        Ok(())
    }

    /// [E-Unwind]: releases fault-releasable obligations in `value` and gets
    /// stuck with X8 on any open resource not declared `on fault release`.
    fn unwind_value(&mut self, value: &Value) -> Result<(), Fail> {
        match value {
            Value::Resource { ty, name, id } => {
                if self.resources.get(&(*ty, *id)).copied().unwrap_or(false) {
                    if !self.program.resources[*ty].on_fault_release {
                        return fail(
                            8,
                            "E-Unwind",
                            format!(
                                "open resource `{name}#{id}` is not declared `on fault release`"
                            ),
                        );
                    }
                    self.resources.insert((*ty, *id), false);
                    self.trace.push(Event {
                        control: format!("{name}.release"),
                        arguments: vec![value.clone()],
                        alternative: "released".to_string(),
                        payload: Vec::new(),
                    });
                }
                Ok(())
            }
            Value::Outcome { payload, .. } => {
                for item in payload {
                    self.unwind_value(item)?;
                }
                Ok(())
            }
            Value::Struct { fields, .. }
            | Value::Variant { fields, .. }
            | Value::Vector(fields) => {
                for item in fields {
                    self.unwind_value(item)?;
                }
                Ok(())
            }
            Value::Map(entries) => {
                for (k, v) in entries {
                    self.unwind_value(k)?;
                    self.unwind_value(v)?;
                }
                Ok(())
            }
            Value::Bool(_) | Value::Int(_) | Value::String(_) => Ok(()),
        }
    }

    fn unwind_scope(&mut self, scope: &Scope) -> Result<(), Fail> {
        for (_, slot) in scope.slots.iter().rev() {
            if let Slot::Available(val) = slot {
                self.unwind_value(val)?;
            }
        }
        self.count("E-Unwind");
        Ok(())
    }

    /// Whether the current position in the top activation is permitted to fault
    /// ([E-Fault], [E-Unwind]).
    fn current_permits_faults(&self) -> bool {
        let act = self.stack.last().expect("an activation");
        if act
            .scopes
            .iter()
            .any(|s| matches!(s.kind, ScopeKind::LexicalBoundary { .. }))
        {
            return true;
        }
        match &act.kind {
            ActivationKind::Control(cid) => self.program.controls[*cid].faults,
            ActivationKind::Boundary {
                in_contingency: None,
                ..
            } => true,
            ActivationKind::Boundary {
                in_contingency: Some(_),
                ..
            } => false,
        }
    }

    /// [E-Fault], [E-Unwind], [E-Root], [E-Contingency], [E-Supervise]: unwinds
    /// a fault with `message` until a boundary catches it or the root pops.
    fn unwind_fault(&mut self, message: String) -> Result<Option<End>, Fail> {
        loop {
            while let Some(top_scope) = self.stack.last_mut().expect("an activation").scopes.pop() {
                self.unwind_scope(&top_scope)?;
                if let ScopeKind::LexicalBoundary {
                    parent_pc,
                    captures,
                    contingency_body,
                    contingency_vars,
                    message_var,
                    ..
                } = top_scope.kind
                {
                    // [E-Contingency]: enter lexical contingency `B₂`.
                    let mut slots = Vec::with_capacity(contingency_vars.len() + 1);
                    for (var, val) in contingency_vars.into_iter().zip(captures) {
                        slots.push((var, Slot::Available(val)));
                    }
                    if let Some(mvar) = message_var {
                        slots.push((mvar, Slot::Available(Value::String(message.clone()))));
                    }
                    self.stack
                        .last_mut()
                        .expect("an activation")
                        .scopes
                        .push(Scope {
                            body: contingency_body,
                            pc: 0,
                            slots,
                            history: Vec::new(),
                            kind: ScopeKind::LexicalContingency {
                                parent_pc,
                                fault_message: message,
                            },
                        });
                    self.count("E-Contingency");
                    return Ok(None);
                }
            }

            // Pop activation (and apply `&~` writebacks if any).
            let popped = self.stack.pop().expect("an activation");
            if self.stack.is_empty() {
                self.count("E-Root");
                return Ok(Some(End::Faulted(message)));
            }
            for (param_var, caller_place) in &popped.writebacks {
                if let Some(root_scope) = popped.scopes.first()
                    && let Some((_, Slot::Available(val))) =
                        root_scope.slots.iter().find(|(s, _)| *s == *param_var)
                {
                    let _ = self.write_eval_place(caller_place, val.clone(), false);
                }
            }
            self.locate();

            // Check if the caller activation is a named boundary in `B₁`!
            if let ActivationKind::Boundary {
                id,
                in_contingency: None,
            } = self.stack.last().expect("an activation").kind
            {
                return self.enter_named_contingency(id, message);
            }

            // Otherwise the caller is a control (or a named boundary contingency)
            // waiting at a decision/capture/supervise.
            if !self.current_permits_faults() {
                let caller_name = self.current_name();
                return fail(
                    8,
                    "E-Unwind",
                    format!(
                        "a fault reached `{caller_name}`, which does not permit `faults` and has no enclosing boundary"
                    ),
                );
            }
            if let ActivationKind::Boundary {
                id,
                in_contingency: None,
            } = self.stack.last().expect("an activation").kind
            {
                return self.enter_named_contingency(id, message);
            }
        }
    }

    /// Unwinds the current activation up to the nearest boundary (lexical or
    /// named) or root.
    fn trigger_unwind(&mut self, message: String) -> Result<Option<End>, Fail> {
        loop {
            let is_named_b1 = matches!(
                self.stack.last().expect("an activation").kind,
                ActivationKind::Boundary {
                    in_contingency: None,
                    ..
                }
            );
            if is_named_b1 {
                let ActivationKind::Boundary { id, .. } =
                    self.stack.last().expect("an activation").kind
                else {
                    unreachable!();
                };
                return self.enter_named_contingency(id, message);
            }

            let has_scopes = !self.stack.last().expect("an activation").scopes.is_empty();
            if has_scopes {
                let top_scope = self
                    .stack
                    .last_mut()
                    .expect("an activation")
                    .scopes
                    .pop()
                    .unwrap();
                self.unwind_scope(&top_scope)?;
                if let ScopeKind::LexicalBoundary {
                    parent_pc,
                    captures,
                    contingency_body,
                    contingency_vars,
                    message_var,
                    ..
                } = top_scope.kind
                {
                    let mut slots = Vec::with_capacity(contingency_vars.len() + 1);
                    for (var, val) in contingency_vars.into_iter().zip(captures) {
                        slots.push((var, Slot::Available(val)));
                    }
                    if let Some(mvar) = message_var {
                        slots.push((mvar, Slot::Available(Value::String(message.clone()))));
                    }
                    self.stack
                        .last_mut()
                        .expect("an activation")
                        .scopes
                        .push(Scope {
                            body: contingency_body,
                            pc: 0,
                            slots,
                            history: Vec::new(),
                            kind: ScopeKind::LexicalContingency {
                                parent_pc,
                                fault_message: message,
                            },
                        });
                    self.count("E-Contingency");
                    return Ok(None);
                }
                continue;
            }

            return self.unwind_fault(message);
        }
    }

    /// [E-Supervise]: unwinds `B₁` of named boundary `id` except for its
    /// retained parameters `ȳ`, and starts `B₂` (`contingency`).
    fn enter_named_contingency(
        &mut self,
        id: BoundaryId,
        message: String,
    ) -> Result<Option<End>, Fail> {
        let retains = self.program.boundaries[id].retains.clone();
        // Unwind any inner loop scopes of `B₁` first.
        while self.stack.last().expect("an activation").scopes.len() > 1 {
            let inner = self
                .stack
                .last_mut()
                .expect("an activation")
                .scopes
                .pop()
                .unwrap();
            self.unwind_scope(&inner)?;
        }
        let mut root_scope = self
            .stack
            .last_mut()
            .expect("an activation")
            .scopes
            .pop()
            .expect("root scope of named boundary");
        let mut retained_values = Vec::with_capacity(retains.len());
        for &param_var in &retains {
            let param_name = self.program.boundaries[id].parameters[param_var]
                .name
                .clone();
            let slot = root_scope.slots.iter_mut().find(|(s, _)| *s == param_var);
            match slot {
                Some((_, s @ Slot::Available(_))) => {
                    let Slot::Available(val) = std::mem::replace(s, Slot::Moved) else {
                        unreachable!();
                    };
                    retained_values.push(val);
                }
                _ => {
                    return fail(
                        2,
                        "E-Supervise",
                        format!(
                            "retained parameter `{param_name}` is not available when boundary `{}` faults",
                            self.program.boundaries[id].name
                        ),
                    );
                }
            }
        }
        // Unwind all remaining non-retained slots of `B₁`.
        self.unwind_scope(&root_scope)?;

        let mut b2_slots = Vec::with_capacity(retained_values.len() + 1);
        for (idx, val) in retained_values.into_iter().enumerate() {
            b2_slots.push((idx, Slot::Available(val)));
        }
        let msg_var = retains.len();
        b2_slots.push((msg_var, Slot::Available(Value::String(message.clone()))));
        let act = self.stack.last_mut().expect("an activation");
        act.kind = ActivationKind::Boundary {
            id,
            in_contingency: Some(message),
        };
        act.scopes.push(Scope {
            body: 0,
            pc: 0,
            slots: b2_slots,
            history: Vec::new(),
            kind: ScopeKind::Normal,
        });
        self.count("E-Supervise");
        Ok(None)
    }

    fn selects(&self, selector: &Selector) -> bool {
        match selector {
            Selector::Top => true,
            Selector::Atom(site, alternative) => {
                let activation = self.stack.last().expect("an activation");
                activation
                    .scopes
                    .iter()
                    .any(|scope| scope.history.contains(&(*site, *alternative)))
            }
            Selector::Not(inner) => !self.selects(inner),
            Selector::And(left, right) => self.selects(left) && self.selects(right),
            Selector::Or(left, right) => self.selects(left) || self.selects(right),
        }
    }

    fn locate(&mut self) {
        let activation = self.stack.last().expect("an activation");
        let scope = activation.scopes.last().expect("a scope");
        let statements = &self.body().bodies[scope.body].statements;
        let line = match statements.get(scope.pc).or(statements.last()) {
            Some(statement) => statement.line,
            None => 0,
        };
        self.location = (self.current_name(), line);
    }

    /// [E-Loop], [E-Again]: a fresh scope for the body of a loop, with its
    /// parameters bound and the invariant true for them.
    fn enter(
        &mut self,
        body: BodyId,
        values: Vec<Value>,
        clause: &'static str,
    ) -> Result<(), Fail> {
        let head = self.body().bodies[body].head.as_ref().expect("a loop body");
        let slots = head
            .parameters
            .iter()
            .copied()
            .zip(values.iter().cloned().map(Slot::Available))
            .collect();
        let activation = self.stack.last_mut().expect("an activation");
        activation.scopes.push(Scope {
            body,
            pc: 0,
            slots,
            history: Vec::new(),
            kind: ScopeKind::Loop {
                entry_parameters: values,
            },
        });
        if let Some(invariant) = &head.invariant
            && !self.formula(invariant)?
        {
            return fail(
                7,
                clause,
                format!("the invariant of the loop `{}` is false", head.name),
            );
        }
        Ok(())
    }

    /// [E-Decide], [E-Capture]: the waiting decision or capture completes with
    /// an alternative of the operation's contract and its payload.
    fn complete(&mut self, alternative: usize, payload: Vec<Value>) -> Result<(), Fail> {
        let body = self.body();
        let scope = self.scope();
        let statement = &body.bodies[scope.body].statements[scope.pc];
        match &statement.command {
            Command::Decide {
                site,
                op_kind,
                arms,
                ..
            } => {
                let site = *site;
                let arms = arms.clone();
                let contract = op_kind.contract(self.program);
                let op_name = op_kind.name(self.program).to_string();
                let Some(position) = arms.iter().position(|arm| arm.alternative == alternative)
                else {
                    return fail(
                        10,
                        "E-Decide",
                        format!(
                            "`{op_name}` completed with `{}`, which the decision does not list",
                            contract[alternative].name
                        ),
                    );
                };
                let scope = self.scope();
                scope.history.push((site, position));
                scope.pc += 1;
                for (var, value) in arms[position].vars.iter().zip(payload) {
                    self.bind(*var, value)?;
                }
                self.count("E-Decide");
                Ok(())
            }
            Command::Capture {
                var, outcome_id, ..
            } => {
                let var = *var;
                let outcome_id = *outcome_id;
                let alt_name = self.program.outcomes[outcome_id].alternatives[alternative]
                    .name
                    .clone();
                let out_val = Value::Outcome {
                    ty: outcome_id,
                    alternative,
                    alternative_name: alt_name,
                    payload,
                };
                self.bind(var, out_val)?;
                self.scope().pc += 1;
                self.count("E-Capture");
                Ok(())
            }
            _ => fail(
                10,
                "E-Decide",
                "an operation completes where no decision or capture waits",
            ),
        }
    }

    fn check_effects(&self, uses: &[String], callee_name: &str) -> Result<(), Fail> {
        for effect in uses {
            for activation in &self.stack {
                if let ActivationKind::Control(cid) = activation.kind {
                    let caller = &self.program.controls[cid];
                    if !caller.uses.contains(effect) {
                        return fail(
                            9,
                            "E-Effect",
                            format!(
                                "`{callee_name}` has the effect `{effect}`, which `{}` does not allow",
                                caller.name
                            ),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// [E-Extern], [E-Effect], [E-Host], [E-Resource]: one step of the host.
    fn external(&mut self, id: ControlId, arguments: Vec<Value>) -> Result<Answered, Fail> {
        let callee: &Control = &self.program.controls[id];
        if let Some(precondition) = &callee.precondition {
            let env = SignatureEnv {
                parameters: &arguments,
                old_parameters: &arguments,
                fields: &[],
            };
            if !self.evaluator().holds(precondition, &env)? {
                return fail(
                    7,
                    "E-Extern",
                    format!(
                        "the precondition of `{}` is false for ({})",
                        callee.name,
                        list(&arguments)
                    ),
                );
            }
        }
        self.check_effects(&callee.uses, &callee.name)?;

        // [E-Resource]: track resources passed by value vs by borrow.
        let mut consumed_resources = Vec::new();
        for (arg, param) in arguments.iter().zip(&callee.parameters) {
            let mut arg_res = Vec::new();
            collect_resources(arg, &mut arg_res);
            for &(rid, handle_id) in &arg_res {
                if !self
                    .resources
                    .get(&(rid, handle_id))
                    .copied()
                    .unwrap_or(false)
                {
                    return fail(
                        5,
                        "E-Resource",
                        format!(
                            "resource `{}#{handle_id}` passed to `{}` is not open",
                            self.program.resources[rid].name, callee.name
                        ),
                    );
                }
            }
            if !matches!(param.ty, Type::Ref(_, _)) {
                consumed_resources.extend(arg_res);
            }
        }

        let answer = match self.host.call(callee, &arguments, self.config) {
            Ok(answer) => answer,
            Err(detail) => return Ok(Answered::Ended(End::HostFailed(detail))),
        };
        let violation = |detail: String| Ok(Answered::Ended(End::HostViolation(detail)));
        let Some(index) = callee
            .contract
            .iter()
            .position(|entry| entry.name == answer.alternative)
        else {
            return violation(format!(
                "`{}` answered `{}`, which its contract does not list",
                callee.name, answer.alternative
            ));
        };
        let alternative = &callee.contract[index];
        let typed = answer.payload.len() == alternative.fields.len()
            && answer
                .payload
                .iter()
                .zip(&alternative.fields)
                .all(|(value, field)| self.config.fits_in_program(self.program, value, &field.ty));
        if !typed {
            return violation(format!(
                "`{}` answered `{} {{{}}}`, which is not a payload of that alternative",
                callee.name,
                answer.alternative,
                list(&answer.payload)
            ));
        }

        // [E-Resource]: resources taken by value are closed unless returned in
        // the payload; new resources in the payload are recorded as open.
        for &(rid, handle_id) in &consumed_resources {
            self.resources.insert((rid, handle_id), false);
        }
        let mut returned_resources = Vec::new();
        for val in &answer.payload {
            collect_resources(val, &mut returned_resources);
        }
        for &(rid, handle_id) in &returned_resources {
            if consumed_resources.contains(&(rid, handle_id)) {
                self.resources.insert((rid, handle_id), true);
            } else if let std::collections::btree_map::Entry::Vacant(e) =
                self.resources.entry((rid, handle_id))
            {
                e.insert(true);
            } else {
                return violation(format!(
                    "`{}` returned an already-used resource handle `{}#{handle_id}`",
                    callee.name, self.program.resources[rid].name
                ));
            }
        }
        if !consumed_resources.is_empty() || !returned_resources.is_empty() {
            self.count("E-Resource");
        }

        if let Some(condition) = &alternative.condition {
            let env = SignatureEnv {
                parameters: &arguments,
                old_parameters: &arguments,
                fields: &answer.payload,
            };
            if !self.evaluator().holds(condition, &env)? {
                return violation(format!(
                    "`{}` answered `{} {{{}}}` for ({}), and the condition of the alternative is false",
                    callee.name,
                    answer.alternative,
                    list(&answer.payload),
                    list(&arguments)
                ));
            }
        }
        if callee.is_pure() {
            let key = (id, arguments.clone());
            let given = (index, answer.payload.clone());
            match self.answers.get(&key) {
                Some(earlier) if *earlier != given => {
                    return violation(format!(
                        "`{}` has no effect and answered ({}) in two ways",
                        callee.name,
                        list(&arguments)
                    ));
                }
                Some(_) => {}
                None => {
                    self.answers.insert(key, given);
                }
            }
        } else {
            self.trace.push(Event {
                control: callee.name.clone(),
                arguments,
                alternative: answer.alternative.clone(),
                payload: answer.payload.clone(),
            });
        }
        self.count("E-Extern");
        Ok(Answered::Completed(index, answer.payload))
    }

    fn execute_builtin(
        &mut self,
        op: BuiltinOp,
        target: Option<&Place>,
        arguments: &[Expr],
    ) -> Result<(usize, Vec<Value>), Fail> {
        let (mut vals, _) = self.eval_operands(arguments, None)?;
        match op {
            BuiltinOp::Push => {
                let place = self.eval_place(target.expect("push target"))?;
                let elem = vals.remove(1);
                let Value::Vector(mut items) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`push` on non-vector");
                };
                items.push(elem);
                self.write_eval_place(&place, Value::Vector(items), false)?;
                Ok((0, Vec::new()))
            }
            BuiltinOp::Pop => {
                let place = self.eval_place(target.expect("pop target"))?;
                let Value::Vector(mut items) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`pop` on non-vector");
                };
                if let Some(last) = items.pop() {
                    self.write_eval_place(&place, Value::Vector(items), false)?;
                    Ok((0, vec![last]))
                } else {
                    Ok((1, Vec::new()))
                }
            }
            BuiltinOp::Take => {
                let Value::Vector(mut items) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`take` on non-vector");
                };
                if items.is_empty() {
                    Ok((1, Vec::new()))
                } else {
                    let head = items.remove(0);
                    Ok((0, vec![head, Value::Vector(items)]))
                }
            }
            BuiltinOp::Get => {
                let key = vals.remove(1);
                let Value::Map(entries) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`get` on non-map");
                };
                match entries.get(&key) {
                    Some(v) => Ok((0, vec![v.clone()])),
                    None => Ok((1, Vec::new())),
                }
            }
            BuiltinOp::Insert => {
                let place = self.eval_place(target.expect("insert target"))?;
                let value = vals.remove(2);
                let key = vals.remove(1);
                let Value::Map(mut entries) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`insert` on non-map");
                };
                let prev = entries.insert(key, value);
                self.write_eval_place(&place, Value::Map(entries), false)?;
                match prev {
                    None => Ok((0, Vec::new())),
                    Some(old) => Ok((1, vec![old])),
                }
            }
            BuiltinOp::Remove => {
                let place = self.eval_place(target.expect("remove target"))?;
                let key = vals.remove(1);
                let Value::Map(mut entries) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`remove` on non-map");
                };
                let prev = entries.remove(&key);
                self.write_eval_place(&place, Value::Map(entries), false)?;
                match prev {
                    Some(old) => Ok((0, vec![old])),
                    None => Ok((1, Vec::new())),
                }
            }
            BuiltinOp::EntryAt => {
                let pos = integer(vals.remove(1))?;
                let Value::Map(entries) = vals.remove(0) else {
                    return fail(10, "E-Extern", "`entry_at` on non-map");
                };
                let Some(idx) = pos
                    .to_i128()
                    .filter(|&i| i >= 0 && (i as usize) < entries.len())
                else {
                    return Ok((1, Vec::new()));
                };
                let (k, v) = entries.iter().nth(idx as usize).unwrap();
                let next = pos.plus(&Int::from_u64(1));
                Ok((0, vec![k.clone(), v.clone(), Value::Int(next)]))
            }
            BuiltinOp::Emit => {
                self.check_effects(&["console.write".to_string()], "emit")?;
                let text = vals.remove(0);
                self.trace.push(Event {
                    control: "emit".to_string(),
                    arguments: vec![text],
                    alternative: "done".to_string(),
                    payload: Vec::new(),
                });
                self.count("E-Extern");
                Ok((0, Vec::new()))
            }
        }
    }

    fn execute_operation(
        &mut self,
        op_kind: &Operation,
        arguments: &[Expr],
    ) -> Result<Option<Answered>, Fail> {
        match op_kind {
            Operation::Control(callee_id) => {
                let callee = &self.program.controls[*callee_id];
                let param_types: Vec<Type> =
                    callee.parameters.iter().map(|p| p.ty.clone()).collect();
                let (arg_values, writebacks) = self.eval_operands(arguments, Some(&param_types))?;
                let callee = &self.program.controls[*callee_id];
                if callee.body.is_none() {
                    return Ok(Some(self.external(*callee_id, arg_values)?));
                }
                // [E-Call]
                if let Some(precondition) = &callee.precondition {
                    let env = SignatureEnv {
                        parameters: &arg_values,
                        old_parameters: &arg_values,
                        fields: &[],
                    };
                    if !self.evaluator().holds(precondition, &env)? {
                        return fail(
                            7,
                            "E-Call",
                            format!(
                                "the precondition of `{}` is false for ({})",
                                callee.name,
                                list(&arg_values)
                            ),
                        );
                    }
                }
                self.push_control(*callee_id, arg_values, writebacks);
                self.count("E-Call");
                Ok(None)
            }
            Operation::Builtin { op, target, .. } => {
                let (alt, payload) = self.execute_builtin(*op, target.as_ref(), arguments)?;
                Ok(Some(Answered::Completed(alt, payload)))
            }
            Operation::Outcome(_) => {
                let val = self.eval_expr(&arguments[0], true)?;
                let Value::Outcome {
                    alternative,
                    payload,
                    ..
                } = val
                else {
                    return fail(10, "E-Outcome", "`outcome` on non-outcome value");
                };
                self.count("E-Outcome");
                Ok(Some(Answered::Completed(alternative, payload)))
            }
            Operation::Variant { by_value, .. } => {
                let val = self.eval_expr(&arguments[0], *by_value)?;
                let Value::Variant {
                    constructor,
                    fields,
                    ..
                } = val
                else {
                    return fail(10, "E-Outcome", "variant decision on non-variant value");
                };
                self.count("E-Outcome");
                Ok(Some(Answered::Completed(constructor, fields)))
            }
        }
    }

    /// One step. `Ok(None)` when the configuration has a successor.
    fn step(&mut self) -> Result<Option<End>, Fail> {
        self.locate();
        let body = self.body();
        let (body_id, pc) = {
            let scope = self.scope();
            (scope.body, scope.pc)
        };
        let statements = &body.bodies[body_id].statements;
        let Some(statement) = statements.get(pc) else {
            // [E-Boundary]: `B₁` of a lexical boundary may fall off its end,
            // which completes site `d` with its single alternative.
            if matches!(self.scope().kind, ScopeKind::LexicalBoundary { .. }) {
                let top_scope = self
                    .stack
                    .last_mut()
                    .expect("an activation")
                    .scopes
                    .pop()
                    .unwrap();
                self.end_scope(&top_scope)?;
                let ScopeKind::LexicalBoundary {
                    site, parent_pc, ..
                } = top_scope.kind
                else {
                    unreachable!();
                };
                let parent = self.scope();
                parent.history.push((site, 0));
                parent.pc = parent_pc + 1;
                return Ok(None);
            }
            // [E-End]: a body never falls off its end.
            return fail(
                1,
                "E-End",
                "the body reaches its end and the history is not disposed",
            );
        };
        if !self.selects(&statement.selector) {
            self.count("E-Skip");
            self.scope().pc += 1;
            return Ok(None);
        }
        let command = statement.command.clone();
        match &command {
            Command::Bind { var, value } => {
                let value = self.eval_expr(value, true)?;
                self.bind(*var, value)?;
                self.scope().pc += 1;
                self.count("E-Bind");
            }
            Command::Unpack { vars, value, .. } => {
                let val = self.eval_expr(value, true)?;
                let Value::Struct { fields, .. } = val else {
                    return fail(10, "E-Unpack", "unpacking a non-struct value");
                };
                for (var, fval) in vars.iter().zip(fields) {
                    self.bind(*var, fval)?;
                }
                self.scope().pc += 1;
                self.count("E-Unpack");
            }
            Command::Capture {
                operation,
                arguments,
                ..
            } => {
                if let Some(answered) = self.execute_operation(operation, arguments)? {
                    match answered {
                        Answered::Ended(end) => return Ok(Some(end)),
                        Answered::Completed(alternative, payload) => {
                            self.complete(alternative, payload)?;
                        }
                    }
                }
            }
            Command::Assign { var, value } => {
                let value = self.eval_expr(value, true)?;
                let place = EvalPlace {
                    root: *var,
                    steps: Vec::new(),
                };
                self.write_eval_place(&place, value, true)?;
                self.scope().pc += 1;
                self.count("E-Assign");
            }
            Command::AssignPlace { target, value } => {
                let value = self.eval_expr(value, true)?;
                let eval_place = self.eval_place(target)?;
                self.write_eval_place(&eval_place, value, true)?;
                self.scope().pc += 1;
                self.count("E-Assign");
            }
            Command::Oblige(formula) => {
                if !self.formula(formula)? {
                    return fail(7, "E-Oblige", "the obligation is false");
                }
                self.scope().pc += 1;
                self.count("E-Oblige");
            }
            Command::Test { site, condition } => {
                let value = self.eval_expr(condition, false)?;
                let alternative = if boolean(value)? { 0 } else { 1 };
                let scope = self.scope();
                scope.history.push((*site, alternative));
                scope.pc += 1;
                self.count("E-Test");
            }
            Command::Decide {
                op_kind, arguments, ..
            } => {
                if let Some(answered) = self.execute_operation(op_kind, arguments)? {
                    match answered {
                        Answered::Ended(end) => return Ok(Some(end)),
                        Answered::Completed(alternative, payload) => {
                            self.complete(alternative, payload)?;
                        }
                    }
                }
            }
            Command::Loop {
                body: inner,
                initial,
                ..
            } => {
                let (values, _) = self.eval_operands(initial, None)?;
                if let Err(stuck) = self.enter(*inner, values, "E-Loop") {
                    self.stack.last_mut().expect("an activation").scopes.pop();
                    return Err(stuck);
                }
                self.count("E-Loop");
            }
            Command::Boundary {
                site,
                body: b1,
                captures,
                contingency_vars,
                message_var,
                contingency: b2,
            } => {
                let mut cap_values = Vec::with_capacity(captures.len());
                for &cvar in captures {
                    let cty = self.body().vars[cvar].ty.clone();
                    let val = self.eval_expr(&Expr::Var(VarRef::Local(cvar), cty), true)?;
                    cap_values.push(val);
                }
                let parent_pc = self.scope().pc;
                self.stack
                    .last_mut()
                    .expect("an activation")
                    .scopes
                    .push(Scope {
                        body: *b1,
                        pc: 0,
                        slots: Vec::new(),
                        history: Vec::new(),
                        kind: ScopeKind::LexicalBoundary {
                            site: *site,
                            parent_pc,
                            captures: cap_values,
                            contingency_body: *b2,
                            contingency_vars: contingency_vars.clone(),
                            message_var: *message_var,
                        },
                    });
                self.count("E-Boundary");
            }
            Command::Supervise {
                boundary,
                arguments,
            } => {
                let (values, _) = self.eval_operands(arguments, None)?;
                let slots = values
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| (i, Slot::Available(v)))
                    .collect();
                self.stack.push(Activation {
                    kind: ActivationKind::Boundary {
                        id: *boundary,
                        in_contingency: None,
                    },
                    arguments: Vec::new(),
                    writebacks: Vec::new(),
                    scopes: vec![Scope {
                        body: 0,
                        pc: 0,
                        slots,
                        history: Vec::new(),
                        kind: ScopeKind::Normal,
                    }],
                });
                self.count("E-Supervise");
            }
            Command::Again {
                body: target,
                arguments,
            } => {
                let (values, _) = self.eval_operands(arguments, None)?;
                while let Some(scope) = self.stack.last_mut().expect("an activation").scopes.pop() {
                    let done = scope.body == *target;
                    self.end_scope(&scope)?;
                    if done {
                        break;
                    }
                }
                self.enter(*target, values, "E-Again")?;
                self.count("E-Again");
            }
            Command::Leave {
                alternative,
                payload,
            } => {
                let (values, _) = self.eval_operands(payload, None)?;
                loop {
                    let scope = self
                        .stack
                        .last_mut()
                        .expect("an activation")
                        .scopes
                        .pop()
                        .expect("a loop scope");
                    let is_loop = matches!(scope.kind, ScopeKind::Loop { .. });
                    self.end_scope(&scope)?;
                    if is_loop {
                        break;
                    }
                }
                let body = self.body();
                let scope = self.scope();
                let Command::Loop { site, arms, .. } =
                    &body.bodies[scope.body].statements[scope.pc].command
                else {
                    return fail(
                        10,
                        "E-Leave",
                        "a loop is left where no loop statement waits",
                    );
                };
                let site = *site;
                let arm_vars = arms[*alternative].clone();
                scope.history.push((site, *alternative));
                scope.pc += 1;
                for (var, value) in arm_vars.iter().zip(values) {
                    self.bind(*var, value)?;
                }
                self.count("E-Leave");
            }
            Command::Exit {
                alternative,
                payload,
            } => {
                let (values, _) = self.eval_operands(payload, None)?;
                let act = self.stack.last().expect("an activation");
                let ActivationKind::Control(control_id) = act.kind else {
                    return fail(10, "E-Exit", "`■` inside a named boundary");
                };
                let control = &self.program.controls[control_id];
                let entry = &control.contract[*alternative];
                if let Some(condition) = &entry.condition {
                    // Completion parameter values (reading updated slots for `&~` parameters).
                    let mut final_params = act.arguments.clone();
                    if let Some(root_scope) = act.scopes.first() {
                        for (idx, param) in control.parameters.iter().enumerate() {
                            if matches!(param.ty, Type::Ref(BorrowMode::Exclusive, _))
                                && let Some((_, Slot::Available(v))) =
                                    root_scope.slots.iter().find(|(s, _)| *s == idx)
                            {
                                final_params[idx] = v.clone();
                            }
                        }
                    }
                    let env = SignatureEnv {
                        parameters: &final_params,
                        old_parameters: &act.arguments,
                        fields: &values,
                    };
                    if !self.evaluator().holds(condition, &env)? {
                        return fail(
                            7,
                            "E-Exit",
                            format!(
                                "the condition of `{}` is false for the payload {{{}}}",
                                entry.name,
                                list(&values)
                            ),
                        );
                    }
                }
                // [E-Scope] for every scope of the activation, innermost first.
                let mut scopes =
                    std::mem::take(&mut self.stack.last_mut().expect("an activation").scopes);
                let root_slots = scopes.first().map(|s| s.slots.clone()).unwrap_or_default();
                while let Some(scope) = scopes.pop() {
                    self.end_scope(&scope)?;
                }
                let popped = self.stack.pop().expect("an activation");
                self.count("E-Exit");
                if self.stack.is_empty() {
                    let alt_name = self.program.controls[control_id].contract[*alternative]
                        .name
                        .clone();
                    return Ok(Some(End::Done {
                        alternative: alt_name,
                        payload: values,
                    }));
                }
                // Write back any `&~` parameters to the caller's places.
                for (param_var, caller_place) in &popped.writebacks {
                    if let Some((_, Slot::Available(val))) =
                        root_slots.iter().find(|(s, _)| *s == *param_var)
                    {
                        self.write_eval_place(caller_place, val.clone(), false)?;
                    }
                }
                self.locate();
                self.complete(*alternative, values)?;
            }
            Command::Fault { message, .. } => {
                let Value::String(msg) = self.eval_expr(message, false)? else {
                    return fail(10, "E-Fault", "fault message is not a string");
                };
                self.count("E-Fault");
                // If inside a contingency body `B₂`, a new `fault` ends `B₂` and
                // propagates outward.
                if let Some(ScopeKind::LexicalContingency { parent_pc, .. }) = self
                    .stack
                    .last()
                    .and_then(|a| a.scopes.last())
                    .map(|s| &s.kind)
                {
                    let parent_pc = *parent_pc;
                    let b2_scope = self
                        .stack
                        .last_mut()
                        .expect("an activation")
                        .scopes
                        .pop()
                        .unwrap();
                    self.unwind_scope(&b2_scope)?;
                    self.scope().pc = parent_pc + 1;
                    if !self.current_permits_faults() {
                        return fail(
                            8,
                            "E-Fault",
                            format!("`{}` does not permit `faults`", self.current_name()),
                        );
                    }
                    return self.trigger_unwind(msg);
                }
                if matches!(
                    self.stack.last().expect("an activation").kind,
                    ActivationKind::Boundary {
                        in_contingency: Some(_),
                        ..
                    }
                ) {
                    while let Some(s) = self.stack.last_mut().expect("an activation").scopes.pop() {
                        self.unwind_scope(&s)?;
                    }
                    return self.unwind_fault(msg);
                }
                if !self.current_permits_faults() {
                    return fail(
                        8,
                        "E-Fault",
                        format!(
                            "`{}` does not permit `faults` and no boundary encloses this statement",
                            self.current_name()
                        ),
                    );
                }
                return self.trigger_unwind(msg);
            }
            Command::Impossible => return fail(6, "E-Impossible", "`⊥` runs"),
            Command::Terminate(status) => {
                let status_val = match status {
                    Some(expr) => Some(self.eval_expr(expr, false)?),
                    None => None,
                };
                // Case 1: inside a lexical contingency body `B₂`.
                if matches!(self.scope().kind, ScopeKind::LexicalContingency { .. }) {
                    let b2_scope = self
                        .stack
                        .last_mut()
                        .expect("an activation")
                        .scopes
                        .pop()
                        .unwrap();
                    self.end_scope(&b2_scope)?;
                    let ScopeKind::LexicalContingency { parent_pc, .. } = b2_scope.kind else {
                        unreachable!();
                    };
                    if let Some(v) = status_val {
                        let mut scopes = std::mem::take(
                            &mut self.stack.last_mut().expect("an activation").scopes,
                        );
                        while let Some(s) = scopes.pop() {
                            self.end_scope(&s)?;
                        }
                        self.stack.pop();
                        return Ok(Some(End::Done {
                            alternative: "terminated".to_string(),
                            payload: vec![v],
                        }));
                    }
                    self.scope().pc = parent_pc + 1;
                    return Ok(None);
                }
                // Case 2: inside a named boundary (`B₁` or `B₂`).
                let mut scopes =
                    std::mem::take(&mut self.stack.last_mut().expect("an activation").scopes);
                while let Some(s) = scopes.pop() {
                    self.end_scope(&s)?;
                }
                self.stack.pop();
                self.scope().pc += 1;
                self.count("E-Supervise");
            }
            Command::Escalate => {
                // Case 1: inside a lexical contingency `B₂`.
                if matches!(self.scope().kind, ScopeKind::LexicalContingency { .. }) {
                    let b2_scope = self
                        .stack
                        .last_mut()
                        .expect("an activation")
                        .scopes
                        .pop()
                        .unwrap();
                    self.unwind_scope(&b2_scope)?;
                    let ScopeKind::LexicalContingency {
                        parent_pc,
                        fault_message,
                    } = b2_scope.kind
                    else {
                        unreachable!();
                    };
                    self.scope().pc = parent_pc + 1;
                    if !self.current_permits_faults() {
                        return fail(
                            8,
                            "E-Unwind",
                            format!("`{}` does not permit `faults`", self.current_name()),
                        );
                    }
                    return self.trigger_unwind(fault_message);
                }
                // Case 2: inside a named boundary contingency `B₂`.
                let ActivationKind::Boundary {
                    in_contingency: Some(msg),
                    ..
                } = self.stack.last().expect("an activation").kind.clone()
                else {
                    return fail(10, "E-Contingency", "`escalate` outside contingency");
                };
                while let Some(s) = self.stack.last_mut().expect("an activation").scopes.pop() {
                    self.unwind_scope(&s)?;
                }
                return self.unwind_fault(msg);
            }
            Command::Recover { body: target_loop } => {
                // Ends scopes up to and including `target_loop` by [E-Scope] and
                // restarts `target_loop` with the parameters it had at entry.
                let mut entry_params = None;
                while let Some(scope) = self.stack.last_mut().expect("an activation").scopes.pop() {
                    let is_target = scope.body == *target_loop;
                    if is_target
                        && let ScopeKind::Loop {
                            ref entry_parameters,
                        } = scope.kind
                    {
                        entry_params = Some(entry_parameters.clone());
                    }
                    self.end_scope(&scope)?;
                    if is_target {
                        break;
                    }
                }
                let Some(params) = entry_params else {
                    return fail(10, "E-Contingency", "`recover` target loop not found");
                };
                self.enter(*target_loop, params, "E-Again")?;
                self.count("E-Contingency");
            }
        }
        Ok(None)
    }

    fn push_control(
        &mut self,
        control: ControlId,
        arguments: Vec<Value>,
        writebacks: Vec<(VarId, EvalPlace)>,
    ) {
        let slots = arguments
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, v)| (i, Slot::Available(v)))
            .collect();
        let scope = Scope {
            body: 0,
            pc: 0,
            slots,
            history: Vec::new(),
            kind: ScopeKind::Normal,
        };
        self.stack.push(Activation {
            kind: ActivationKind::Control(control),
            arguments,
            writebacks,
            scopes: vec![scope],
        });
    }
}

fn update_nested(current: &mut Value, steps: &[EvalStep], new_val: Value) -> Result<Value, Fail> {
    let (first, rest) = steps.split_first().expect("non-empty steps");
    match first {
        EvalStep::Field(fidx) => {
            let Value::Struct { fields, .. } = current else {
                return fail(10, "E-Assign", "field assignment on non-struct");
            };
            if rest.is_empty() {
                Ok(std::mem::replace(&mut fields[*fidx], new_val))
            } else {
                update_nested(&mut fields[*fidx], rest, new_val)
            }
        }
        EvalStep::Index(idx) => {
            let Value::Vector(items) = current else {
                return fail(10, "E-Assign", "index assignment on non-vector");
            };
            let len = items.len();
            let Some(elem) = items.get_mut(*idx) else {
                return fail(
                    7,
                    "E-Index",
                    format!("index {idx} is out of bounds for length {len}"),
                );
            };
            if rest.is_empty() {
                Ok(std::mem::replace(elem, new_val))
            } else {
                update_nested(elem, rest, new_val)
            }
        }
    }
}

/// Whether `formula`, over the parameters of a signature and the fields of one
/// alternative, holds for `arguments` and `payload` ([E-Formula]).
pub fn signature_holds(
    program: &Program,
    config: &Config,
    formula: &Expr,
    arguments: &[Value],
    payload: &[Value],
) -> bool {
    let env = SignatureEnv {
        parameters: arguments,
        old_parameters: arguments,
        fields: payload,
    };
    let evaluator = Evaluator { program, config };
    evaluator.holds(formula, &env).unwrap_or(false)
}

/// Runs the control `entry` on `arguments` until the execution ends, is
/// stuck, or the fuel runs out.
pub fn run(
    program: &Program,
    entry: &str,
    arguments: Vec<Value>,
    host: &mut dyn Host,
    config: &Config,
) -> Run {
    let mut machine = Machine {
        program,
        config,
        host,
        stack: Vec::new(),
        trace: Vec::new(),
        steps: 0,
        rules: BTreeMap::new(),
        answers: HashMap::new(),
        resources: BTreeMap::new(),
        location: (String::new(), 0),
    };
    let refuse = |detail: String| Run {
        end: End::NotAnExecution(detail),
        trace: Vec::new(),
        steps: 0,
        rules: BTreeMap::new(),
    };
    let Some(id) = program.control(entry) else {
        return refuse(format!("`{entry}` is not a control"));
    };
    let control = &program.controls[id];
    if control.body.is_none() {
        return refuse(format!("`{entry}` is an extern control"));
    }
    let typed = arguments.len() == control.parameters.len()
        && arguments
            .iter()
            .zip(&control.parameters)
            .all(|(value, parameter)| config.fits_in_program(program, value, &parameter.ty));
    if !typed {
        return refuse(format!(
            "({}) are not arguments of `{entry}`",
            list(&arguments)
        ));
    }
    if let Some(precondition) = &control.precondition {
        let env = SignatureEnv {
            parameters: &arguments,
            old_parameters: &arguments,
            fields: &[],
        };
        match machine.evaluator().holds(precondition, &env) {
            Ok(true) => {}
            _ => return refuse(format!("the precondition of `{entry}` is false")),
        }
    }
    // Any resource passed as an initial argument to `entry` starts open.
    for arg in &arguments {
        let mut res = Vec::new();
        collect_resources(arg, &mut res);
        for (rid, handle_id) in res {
            machine.resources.insert((rid, handle_id), true);
        }
    }
    machine.push_control(id, arguments, Vec::new());
    let end = loop {
        if machine.steps >= config.fuel {
            break End::OutOfFuel;
        }
        machine.steps += 1;
        match machine.step() {
            Ok(None) => {}
            Ok(Some(end)) => break end,
            Err(stuck) => {
                break End::Stuck(Stuck {
                    state: stuck.state,
                    clause: stuck.clause,
                    control: machine.location.0.clone(),
                    line: machine.location.1,
                    detail: stuck.detail,
                });
            }
        }
    };
    Run {
        end,
        trace: machine.trace,
        steps: machine.steps,
        rules: machine.rules,
    }
}
