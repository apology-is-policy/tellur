//! The oracle: the instrumented semantics of `docs/v2/core.md`, section 6,
//! implemented directly.
//!
//! It runs a typed core program and is stuck on every violation the checker
//! is meant to exclude. Each rule below carries the name of its clause, and
//! each way of being stuck carries its number from the table of section 6.10.
//! Nothing here is fast, and nothing is meant to be: this is the definition
//! that the checker and the compiled code are tested against.
//!
//! Slice 1: scalars, tests, decisions on defined and extern controls, loops,
//! exits, obligations and `⊥`. Every value has kind `copy`, so no slot is
//! ever moved and no borrow tag exists yet.

use crate::host::Host;
use crate::int::Int;
use crate::typed::{
    Arithmetic, BodyId, Command, Comparison, Control, ControlBody, ControlId, Expr, Program,
    Selector, SiteId, Type, VarId, VarRef,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Value {
    Bool(bool),
    Int(Int),
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(value) => write!(formatter, "{value}"),
            Value::Int(value) => write!(formatter, "{value}"),
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

    pub fn fits(&self, value: &Value, ty: Type) -> bool {
        match (value, ty) {
            (Value::Bool(_), Type::Bool) => true,
            (Value::Int(value), Type::Int(ty)) => {
                let (least, greatest) = ty.bounds(&self.usize_max);
                *value >= least && *value <= greatest
            }
            _ => false,
        }
    }
}

/// One step of the host that had an effect ([E-Extern]).
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
}

/// The parameters of a signature and the payload of one alternative.
struct SignatureEnv<'a> {
    parameters: &'a [Value],
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
}

/// One run of one body ([6.2]).
struct Scope {
    body: BodyId,
    pc: usize,
    slots: Vec<(VarId, Value)>,
    history: Vec<(SiteId, usize)>,
}

/// One call of a control: the scope of its body and one for each loop that
/// is entered, and the arguments, which a contract condition reads as they
/// were at entry ([E-Formula]).
struct Activation {
    control: ControlId,
    arguments: Vec<Value>,
    scopes: Vec<Scope>,
}

struct ActivationEnv<'a> {
    body: &'a ControlBody,
    activation: &'a Activation,
}

impl Env for ActivationEnv<'_> {
    /// [E-Var]: a name is looked up from the innermost scope outward, and a
    /// name without a slot is stuck.
    fn get(&self, var: VarRef) -> Result<Value, Fail> {
        let VarRef::Local(id) = var else {
            return fail(10, "E-Var", "a body refers to a name of a signature");
        };
        for scope in self.activation.scopes.iter().rev() {
            if let Some((_, value)) = scope.slots.iter().find(|(slot, _)| *slot == id) {
                return Ok(value.clone());
            }
        }
        fail(
            2,
            "E-Var",
            format!("`{}` has no value on this history", self.body.vars[id].name),
        )
    }
}

struct Evaluator<'a> {
    program: &'a Program,
    config: &'a Config,
}

fn integer(value: Value) -> Result<Int, Fail> {
    match value {
        Value::Int(value) => Ok(value),
        Value::Bool(_) => fail(10, "E-Arith", "a bool stands where an integer is needed"),
    }
}

fn boolean(value: Value) -> Result<bool, Fail> {
    match value {
        Value::Bool(value) => Ok(value),
        Value::Int(_) => fail(10, "E-Test", "an integer stands where a bool is needed"),
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
        (Value::Bool(a), Value::Bool(b)) => match comparison {
            Comparison::Equal => a == b,
            Comparison::NotEqual => a != b,
            _ => return fail(10, "E-Arith", "bools are not ordered"),
        },
        _ => return fail(10, "E-Arith", "a comparison of a bool with an integer"),
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

impl Evaluator<'_> {
    /// `e ⇓ v`, for an expression of the program ([6.4]). Arithmetic belongs
    /// to a type and is stuck outside it ([E-Arith]).
    fn value(&self, expr: &Expr, env: &dyn Env) -> Result<Value, Fail> {
        Ok(match expr {
            Expr::Boolean(value) => Value::Bool(*value),
            Expr::Integer(value, _) => Value::Int(value.clone()),
            Expr::Var(var, _) => env.get(*var)?,
            Expr::Not(operand) => Value::Bool(!boolean(self.value(operand, env)?)?),
            Expr::Negate(operand, ty) => {
                let operand = integer(self.value(operand, env)?)?;
                let result = operand.negate();
                if !self
                    .config
                    .fits(&Value::Int(result.clone()), Type::Int(*ty))
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
                    .fits(&Value::Int(result.clone()), Type::Int(*ty))
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
            // [E-Bool]: the right operand is evaluated only where it decides
            // the result.
            Expr::And(left, right) => {
                Value::Bool(boolean(self.value(left, env)?)? && boolean(self.value(right, env)?)?)
            }
            Expr::Or(left, right) => {
                Value::Bool(boolean(self.value(left, env)?)? || boolean(self.value(right, env)?)?)
            }
            // [E-Fn]: arguments left to right, the precondition, the body.
            Expr::Call(id, arguments, _) => {
                let function = &self.program.functions[*id];
                let mut values = Vec::new();
                for argument in arguments {
                    values.push(self.value(argument, env)?);
                }
                let inner = SignatureEnv {
                    parameters: &values,
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
    /// integers, total, and with no obligation of its own. A quotient by zero
    /// is 0 and a remainder by zero is the dividend; the core leaves both
    /// values open and asks only that they depend on the operands alone.
    fn term(&self, expr: &Expr, env: &dyn Env) -> Result<Value, Fail> {
        Ok(match expr {
            Expr::Boolean(value) => Value::Bool(*value),
            Expr::Integer(value, _) => Value::Int(value.clone()),
            Expr::Var(var, _) => env.get(*var)?,
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
            // Both sides are terms of the formula, and both are read.
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
            // A function is transparent to the logic ([I8]): its body, on the
            // argument terms.
            Expr::Call(id, arguments, _) => {
                let mut values = Vec::new();
                for argument in arguments {
                    values.push(self.term(argument, env)?);
                }
                let inner = SignatureEnv {
                    parameters: &values,
                    fields: &[],
                };
                self.term(&self.program.functions[*id].body, &inner)?
            }
        })
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
    /// Where the statement being run is, for the report of a stuck state.
    location: (String, u32),
}

/// What a step of the host leads to.
enum Answered {
    Completed(usize, Vec<Value>),
    Ended(End),
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
        let control = self.stack.last().expect("an activation").control;
        self.program.controls[control]
            .body
            .as_ref()
            .expect("a defined control")
    }

    fn scope(&mut self) -> &mut Scope {
        self.stack
            .last_mut()
            .expect("an activation")
            .scopes
            .last_mut()
            .expect("a scope")
    }

    fn values(&self, exprs: &[Expr]) -> Result<Vec<Value>, Fail> {
        let activation = self.stack.last().expect("an activation");
        let env = ActivationEnv {
            body: self.body(),
            activation,
        };
        let evaluator = self.evaluator();
        exprs
            .iter()
            .map(|expr| evaluator.value(expr, &env))
            .collect()
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
        let name = &self.body().vars[var].name;
        let scope = self.scope();
        if scope.slots.iter().any(|(slot, _)| *slot == var) {
            return fail(
                3,
                "E-Bind",
                format!("`{name}` is bound twice in one run of its body"),
            );
        }
        scope.slots.push((var, value));
        Ok(())
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
        self.location = (self.program.controls[activation.control].name.clone(), line);
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
        let slots = head.parameters.iter().copied().zip(values).collect();
        let activation = self.stack.last_mut().expect("an activation");
        activation.scopes.push(Scope {
            body,
            pc: 0,
            slots,
            history: Vec::new(),
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

    /// [E-Decide]: the waiting decision completes with an alternative of the
    /// operation's contract and its payload.
    fn complete(&mut self, alternative: usize, payload: Vec<Value>) -> Result<(), Fail> {
        let body = self.body();
        let scope = self.scope();
        let statement = &body.bodies[scope.body].statements[scope.pc];
        let Command::Decide {
            site,
            operation,
            arms,
            ..
        } = &statement.command
        else {
            return fail(
                10,
                "E-Decide",
                "an operation completes where no decision waits",
            );
        };
        let Some(position) = arms.iter().position(|arm| arm.alternative == alternative) else {
            let callee = &self.program.controls[*operation];
            return fail(
                10,
                "E-Decide",
                format!(
                    "`{}` completed with `{}`, which the decision does not list",
                    callee.name, callee.contract[alternative].name
                ),
            );
        };
        scope.history.push((*site, position));
        scope.pc += 1;
        for (var, value) in arms[position].vars.iter().zip(payload) {
            self.bind(*var, value)?;
        }
        self.count("E-Decide");
        Ok(())
    }

    /// [E-Extern], [E-Effect], [E-Host]: one step of the host.
    fn external(&mut self, id: ControlId, arguments: Vec<Value>) -> Result<Answered, Fail> {
        let callee: &Control = &self.program.controls[id];
        if let Some(precondition) = &callee.precondition {
            let env = SignatureEnv {
                parameters: &arguments,
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
        for effect in &callee.uses {
            for activation in &self.stack {
                let caller = &self.program.controls[activation.control];
                if !caller.uses.contains(effect) {
                    return fail(
                        9,
                        "E-Effect",
                        format!(
                            "`{}` has the effect `{effect}`, which `{}` does not allow",
                            callee.name, caller.name
                        ),
                    );
                }
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
                .all(|(value, field)| self.config.fits(value, field.ty));
        if !typed {
            return violation(format!(
                "`{}` answered `{} {{{}}}`, which is not a payload of that alternative",
                callee.name,
                answer.alternative,
                list(&answer.payload)
            ));
        }
        if let Some(condition) = &alternative.condition {
            let env = SignatureEnv {
                parameters: &arguments,
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
        match &statement.command {
            Command::Bind { var, value } => {
                let value = self.values(std::slice::from_ref(value))?.remove(0);
                self.bind(*var, value)?;
                self.scope().pc += 1;
                self.count("E-Bind");
            }
            Command::Assign { var, value } => {
                let value = self.values(std::slice::from_ref(value))?.remove(0);
                let name = &body.vars[*var].name;
                let activation = self.stack.last_mut().expect("an activation");
                let slot = activation
                    .scopes
                    .iter_mut()
                    .rev()
                    .find_map(|scope| scope.slots.iter_mut().find(|(slot, _)| slot == var));
                match slot {
                    Some(slot) => slot.1 = value,
                    None => {
                        return fail(
                            2,
                            "E-Assign",
                            format!("`{name}` has no slot on this history"),
                        );
                    }
                }
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
                let value = self.values(std::slice::from_ref(condition))?.remove(0);
                let alternative = if boolean(value)? { 0 } else { 1 };
                let scope = self.scope();
                scope.history.push((*site, alternative));
                scope.pc += 1;
                self.count("E-Test");
            }
            Command::Decide {
                operation,
                arguments,
                ..
            } => {
                let arguments = self.values(arguments)?;
                let callee = &self.program.controls[*operation];
                if callee.body.is_none() {
                    match self.external(*operation, arguments)? {
                        Answered::Ended(end) => return Ok(Some(end)),
                        Answered::Completed(alternative, payload) => {
                            self.complete(alternative, payload)?;
                        }
                    }
                    return Ok(None);
                }
                // [E-Call]
                if let Some(precondition) = &callee.precondition {
                    let env = SignatureEnv {
                        parameters: &arguments,
                        fields: &[],
                    };
                    if !self.evaluator().holds(precondition, &env)? {
                        return fail(
                            7,
                            "E-Call",
                            format!(
                                "the precondition of `{}` is false for ({})",
                                callee.name,
                                list(&arguments)
                            ),
                        );
                    }
                }
                self.push(*operation, arguments);
                self.count("E-Call");
            }
            Command::Loop {
                body: inner,
                initial,
                ..
            } => {
                let values = self.values(initial)?;
                if let Err(stuck) = self.enter(*inner, values, "E-Loop") {
                    // The loop statement is where the invariant fails.
                    self.stack.last_mut().expect("an activation").scopes.pop();
                    return Err(stuck);
                }
                self.count("E-Loop");
            }
            Command::Again {
                body: target,
                arguments,
            } => {
                let values = self.values(arguments)?;
                let activation = self.stack.last_mut().expect("an activation");
                // [E-Scope] for every scope up to and including the loop's.
                while let Some(scope) = activation.scopes.pop() {
                    if scope.body == *target {
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
                let values = self.values(payload)?;
                let activation = self.stack.last_mut().expect("an activation");
                activation.scopes.pop();
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
                scope.history.push((*site, *alternative));
                scope.pc += 1;
                for (var, value) in arms[*alternative].iter().zip(values) {
                    self.bind(*var, value)?;
                }
                self.count("E-Leave");
            }
            Command::Exit {
                alternative,
                payload,
            } => {
                let values = self.values(payload)?;
                let activation = self.stack.last().expect("an activation");
                let control = &self.program.controls[activation.control];
                let entry = &control.contract[*alternative];
                if let Some(condition) = &entry.condition {
                    let env = SignatureEnv {
                        parameters: &activation.arguments,
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
                // [E-Scope] for every scope of the activation; in this slice
                // no value holds an obligation.
                self.stack.pop();
                self.count("E-Exit");
                if self.stack.is_empty() {
                    return Ok(Some(End::Done {
                        alternative: entry.name.clone(),
                        payload: values,
                    }));
                }
                self.locate();
                self.complete(*alternative, values)?;
            }
            Command::Impossible => return fail(6, "E-Impossible", "`⊥` runs"),
        }
        Ok(None)
    }

    fn push(&mut self, control: ControlId, arguments: Vec<Value>) {
        // The parameters are the first names of the body.
        let slots = arguments.iter().cloned().enumerate().collect();
        let scope = Scope {
            body: 0,
            pc: 0,
            slots,
            history: Vec::new(),
        };
        self.stack.push(Activation {
            control,
            arguments,
            scopes: vec![scope],
        });
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
            .all(|(value, parameter)| config.fits(value, parameter.ty));
    if !typed {
        return refuse(format!(
            "({}) are not arguments of `{entry}`",
            list(&arguments)
        ));
    }
    if let Some(precondition) = &control.precondition {
        let env = SignatureEnv {
            parameters: &arguments,
            fields: &[],
        };
        match machine.evaluator().holds(precondition, &env) {
            Ok(true) => {}
            _ => return refuse(format!("the precondition of `{entry}` is false")),
        }
    }
    machine.push(id, arguments);
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
