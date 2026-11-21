//! The erased semantics: section 9 of `docs/v2/core.md`, implemented
//! directly.
//!
//! This is what release code does, written as a second interpreter. It is
//! the oracle with the bookkeeping removed:
//!
//! - **[R1]** A slot is a machine word. No slot knows whether it was bound.
//! - **[R2]** `⊢`, invariants, preconditions, the conditions of alternatives
//!   and the allowances of effects are not evaluated. Arithmetic is that of
//!   the machine: it wraps at the width of its type.
//! - **[R3]** A history is one bit for each atom that some selector mentions.
//!
//! Where the oracle is stuck, this interpreter goes on, and what it then
//! computes means nothing. Four things have no machine behavior at all, and a
//! run that meets one ends in `undefined`: `⊥`, the end of a body, an
//! alternative that the waiting decision does not list, and a division by
//! zero. The differential harness asserts that none of them is met on a run
//! where the oracle is not stuck.
//!
//! The event trace is recorded here as in the oracle, so that the two
//! observations can be compared. Release code performs the effects and
//! records nothing.

use crate::host::Host;
use crate::int::Int;
use crate::oracle::{Config, Event, Value, list};
use crate::typed::{
    Arithmetic, BodyId, Command, Comparison, ControlBody, ControlId, Expr, IntType, Program,
    Selector, Type, VarRef,
};
use std::fmt;

#[derive(Clone, Debug)]
pub enum End {
    Done {
        alternative: String,
        payload: Vec<Value>,
    },
    /// The run met something that erased code has no behavior for.
    Undefined(String),
    /// The host of this run had no answer.
    HostFailed(String),
    OutOfFuel,
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
            End::Undefined(detail) => write!(formatter, "undefined: {detail}"),
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
}

/// How an integer type lies in a machine word.
#[derive(Clone, Copy)]
struct Shape {
    bits: u32,
    signed: bool,
}

impl Shape {
    fn wrap(self, word: u64) -> u64 {
        if self.bits == 64 {
            word
        } else {
            word & ((1u64 << self.bits) - 1)
        }
    }

    /// The word read as a number in two's complement.
    fn number(self, word: u64) -> i64 {
        let shift = 64 - self.bits;
        ((word << shift) as i64) >> shift
    }
}

/// The arithmetic of the machine.
#[derive(Clone, Copy)]
struct Arith<'a> {
    program: &'a Program,
    usize_bits: u32,
}

/// What the names of an expression stand for.
enum Names<'a> {
    /// The slots of an activation, by the index of the name.
    Slots(&'a [u64]),
    /// The arguments of a function.
    Arguments(&'a [u64]),
}

impl Arith<'_> {
    fn shape(self, ty: IntType) -> Shape {
        let (bits, signed) = match ty {
            IntType::U8 => (8, false),
            IntType::U16 => (16, false),
            IntType::U32 => (32, false),
            IntType::U64 => (64, false),
            IntType::I8 => (8, true),
            IntType::I16 => (16, true),
            IntType::I32 => (32, true),
            IntType::I64 => (64, true),
            IntType::Usize => (self.usize_bits, false),
        };
        Shape { bits, signed }
    }

    fn word(self, value: &Value, ty: Type) -> u64 {
        match (value, ty) {
            (Value::Bool(value), _) => u64::from(*value),
            (Value::Int(value), Type::Int(ty)) => {
                self.shape(ty).wrap(value.to_i128().unwrap_or(0) as u64)
            }
            (Value::Int(value), Type::Bool) => u64::from(!value.is_zero()),
        }
    }

    fn value(self, word: u64, ty: Type) -> Value {
        match ty {
            Type::Bool => Value::Bool(word != 0),
            Type::Int(ty) => {
                let shape = self.shape(ty);
                if shape.signed {
                    Value::Int(Int::from_i128(i128::from(shape.number(word))))
                } else {
                    Value::Int(Int::from_u64(word))
                }
            }
        }
    }

    fn values(self, words: &[u64], types: impl Iterator<Item = Type>) -> Vec<Value> {
        words
            .iter()
            .zip(types)
            .map(|(word, ty)| self.value(*word, ty))
            .collect()
    }

    /// The word an expression computes. Nothing is checked ([R2]); the one
    /// failure is a division by zero, which no machine defines.
    fn eval(self, expr: &Expr, names: &Names) -> Result<u64, String> {
        Ok(match expr {
            Expr::Boolean(value) => u64::from(*value),
            Expr::Integer(value, ty) => self.word(&Value::Int(value.clone()), Type::Int(*ty)),
            Expr::Var(var, _) => match (var, names) {
                (VarRef::Local(id), Names::Slots(slots)) => slots[*id],
                (VarRef::Parameter(index), Names::Arguments(arguments)) => arguments[*index],
                _ => return Err("a name of a signature stands in a body".to_string()),
            },
            Expr::Not(operand) => self.eval(operand, names)? ^ 1,
            Expr::Negate(operand, ty) => self
                .shape(*ty)
                .wrap(self.eval(operand, names)?.wrapping_neg()),
            Expr::Arithmetic(operator, left, right, ty) => {
                let shape = self.shape(*ty);
                let left = self.eval(left, names)?;
                let right = self.eval(right, names)?;
                let result = match operator {
                    Arithmetic::Add => left.wrapping_add(right),
                    Arithmetic::Subtract => left.wrapping_sub(right),
                    Arithmetic::Multiply => left.wrapping_mul(right),
                    Arithmetic::Divide | Arithmetic::Remainder => {
                        if right == 0 {
                            return Err("a division by zero".to_string());
                        }
                        let divide = *operator == Arithmetic::Divide;
                        if shape.signed {
                            let (left, right) = (shape.number(left), shape.number(right));
                            (if divide {
                                left.wrapping_div(right)
                            } else {
                                left.wrapping_rem(right)
                            }) as u64
                        } else if divide {
                            left / right
                        } else {
                            left % right
                        }
                    }
                };
                shape.wrap(result)
            }
            Expr::Compare(comparison, left, right) => {
                let signed = match left.ty() {
                    Type::Int(ty) => Some(self.shape(ty)).filter(|shape| shape.signed),
                    Type::Bool => None,
                };
                let left = self.eval(left, names)?;
                let right = self.eval(right, names)?;
                let ordering = match signed {
                    Some(shape) => shape.number(left).cmp(&shape.number(right)),
                    None => left.cmp(&right),
                };
                u64::from(match comparison {
                    Comparison::Equal => ordering.is_eq(),
                    Comparison::NotEqual => ordering.is_ne(),
                    Comparison::Less => ordering.is_lt(),
                    Comparison::LessEqual => ordering.is_le(),
                    Comparison::Greater => ordering.is_gt(),
                    Comparison::GreaterEqual => ordering.is_ge(),
                })
            }
            // [E-Bool] is behavior, and remains.
            Expr::And(left, right) => {
                if self.eval(left, names)? == 0 {
                    0
                } else {
                    self.eval(right, names)?
                }
            }
            Expr::Or(left, right) => {
                if self.eval(left, names)? != 0 {
                    1
                } else {
                    self.eval(right, names)?
                }
            }
            // The body, on the arguments. The precondition is not evaluated.
            Expr::Call(id, arguments, _) => {
                let mut words = Vec::new();
                for argument in arguments {
                    words.push(self.eval(argument, names)?);
                }
                self.eval(&self.program.functions[*id].body, &Names::Arguments(&words))?
            }
        })
    }
}

/// What is laid out for a control before it runs.
struct Plan {
    /// [R3]: the bit of each atom that a selector mentions, by site and
    /// alternative. An atom that no selector mentions has none.
    bit: Vec<Vec<Option<usize>>>,
    bits: usize,
    /// The bits of the sites of each body. A new run of the body clears them.
    own: Vec<Vec<usize>>,
}

fn mention(selector: &Selector, plan: &mut Plan) {
    match selector {
        Selector::Top => {}
        Selector::Atom(site, alternative) => {
            if plan.bit[*site][*alternative].is_none() {
                plan.bit[*site][*alternative] = Some(plan.bits);
                plan.bits += 1;
            }
        }
        Selector::Not(inner) => mention(inner, plan),
        Selector::And(left, right) | Selector::Or(left, right) => {
            mention(left, plan);
            mention(right, plan);
        }
    }
}

fn plan(body: &ControlBody) -> Plan {
    let mut plan = Plan {
        bit: body
            .sites
            .iter()
            .map(|site| vec![None; site.alternatives.len()])
            .collect(),
        bits: 0,
        own: vec![Vec::new(); body.bodies.len()],
    };
    for inner in &body.bodies {
        for statement in &inner.statements {
            mention(&statement.selector, &mut plan);
        }
    }
    for (site, bits) in body.sites.iter().zip(&plan.bit) {
        plan.own[site.body].extend(bits.iter().flatten());
    }
    plan
}

fn selects(selector: &Selector, plan: &Plan, bits: &[bool]) -> bool {
    match selector {
        Selector::Top => true,
        Selector::Atom(site, alternative) => {
            plan.bit[*site][*alternative].is_some_and(|bit| bits[bit])
        }
        Selector::Not(inner) => !selects(inner, plan, bits),
        Selector::And(left, right) => selects(left, plan, bits) && selects(right, plan, bits),
        Selector::Or(left, right) => selects(left, plan, bits) || selects(right, plan, bits),
    }
}

/// One call of a control. The bodies of a control nest as its text does, so
/// one place for each body's position is all the stack a call needs.
struct Activation {
    control: ControlId,
    slots: Vec<u64>,
    bits: Vec<bool>,
    positions: Vec<usize>,
    current: BodyId,
}

struct Machine<'a> {
    program: &'a Program,
    config: &'a Config,
    arith: Arith<'a>,
    plans: &'a [Option<Plan>],
    host: &'a mut dyn Host,
    stack: Vec<Activation>,
    trace: Vec<Event>,
}

impl<'a> Machine<'a> {
    fn top(&mut self) -> &mut Activation {
        self.stack.last_mut().expect("an activation")
    }

    fn laid_out(&self) -> (&'a ControlBody, &'a Plan) {
        let control = self.stack.last().expect("an activation").control;
        (
            self.program.controls[control]
                .body
                .as_ref()
                .expect("a defined control"),
            self.plans[control].as_ref().expect("a plan"),
        )
    }

    fn push(&mut self, control: ControlId, arguments: &[u64]) {
        let (body, plan) = {
            let body = self.program.controls[control]
                .body
                .as_ref()
                .expect("a defined control");
            (body, self.plans[control].as_ref().expect("a plan"))
        };
        // The parameters are the first names of the body.
        let mut slots = vec![0; body.vars.len()];
        slots[..arguments.len()].copy_from_slice(arguments);
        self.stack.push(Activation {
            control,
            slots,
            bits: vec![false; plan.bits],
            positions: vec![0; body.bodies.len()],
            current: 0,
        });
    }

    fn words(&self, exprs: &[Expr]) -> Result<Vec<u64>, String> {
        let names = Names::Slots(&self.stack.last().expect("an activation").slots);
        exprs
            .iter()
            .map(|expr| self.arith.eval(expr, &names))
            .collect()
    }

    /// A new run of the body of a loop.
    fn enter(&mut self, target: BodyId, words: &[u64]) {
        let (body, plan) = self.laid_out();
        let head = body.bodies[target].head.as_ref().expect("a loop body");
        let top = self.top();
        for bit in &plan.own[target] {
            top.bits[*bit] = false;
        }
        for (var, word) in head.parameters.iter().zip(words) {
            top.slots[*var] = *word;
        }
        top.positions[target] = 0;
        top.current = target;
    }

    /// The waiting decision completes.
    fn complete(&mut self, alternative: usize, payload: &[u64]) -> Result<(), String> {
        let (body, plan) = self.laid_out();
        let top = self.top();
        let current = top.current;
        let statement = &body.bodies[current].statements[top.positions[current]];
        let Command::Decide { site, arms, .. } = &statement.command else {
            return Err("an operation completes where no decision waits".to_string());
        };
        let Some(position) = arms.iter().position(|arm| arm.alternative == alternative) else {
            return Err("an alternative that the decision does not list".to_string());
        };
        if let Some(bit) = plan.bit[*site][position] {
            top.bits[bit] = true;
        }
        for (var, word) in arms[position].vars.iter().zip(payload) {
            top.slots[*var] = *word;
        }
        top.positions[current] += 1;
        Ok(())
    }

    fn step(&mut self) -> Result<Option<End>, String> {
        let (body, plan) = self.laid_out();
        let program = self.program;
        let arith = self.arith;
        let (control, current, position) = {
            let top = self.top();
            (top.control, top.current, top.positions[top.current])
        };
        let Some(statement) = body.bodies[current].statements.get(position) else {
            return Err("the end of a body".to_string());
        };
        if !selects(&statement.selector, plan, &self.top().bits) {
            self.top().positions[current] += 1;
            return Ok(None);
        }
        match &statement.command {
            Command::Bind { var, value } | Command::Assign { var, value } => {
                let word = self.words(std::slice::from_ref(value))?[0];
                let top = self.top();
                top.slots[*var] = word;
                top.positions[current] += 1;
            }
            // [R2]: not evaluated.
            Command::Oblige(_) => self.top().positions[current] += 1,
            Command::Test { site, condition } => {
                let word = self.words(std::slice::from_ref(condition))?[0];
                let alternative = if word != 0 { 0 } else { 1 };
                let top = self.top();
                if let Some(bit) = plan.bit[*site][alternative] {
                    top.bits[bit] = true;
                }
                top.positions[current] += 1;
            }
            Command::Decide {
                operation,
                arguments,
                ..
            } => {
                let words = self.words(arguments)?;
                let callee = &program.controls[*operation];
                if callee.body.is_some() {
                    self.push(*operation, &words);
                    return Ok(None);
                }
                let values = arith.values(&words, callee.parameters.iter().map(|p| p.ty));
                let answer = match self.host.call(callee, &values, self.config) {
                    Ok(answer) => answer,
                    Err(detail) => return Ok(Some(End::HostFailed(detail))),
                };
                let Some(index) = callee
                    .contract
                    .iter()
                    .position(|entry| entry.name == answer.alternative)
                else {
                    return Err(format!(
                        "`{}` answered `{}`, which its contract does not list",
                        callee.name, answer.alternative
                    ));
                };
                let fields = &callee.contract[index].fields;
                if answer.payload.len() != fields.len() {
                    return Err(format!(
                        "`{}` answered a payload of the wrong shape",
                        callee.name
                    ));
                }
                let payload: Vec<u64> = answer
                    .payload
                    .iter()
                    .zip(fields)
                    .map(|(value, field)| arith.word(value, field.ty))
                    .collect();
                if !callee.is_pure() {
                    self.trace.push(Event {
                        control: callee.name.clone(),
                        arguments: values,
                        alternative: answer.alternative,
                        payload: arith.values(&payload, fields.iter().map(|field| field.ty)),
                    });
                }
                self.complete(index, &payload)?;
            }
            Command::Loop {
                body: inner,
                initial,
                ..
            } => {
                let words = self.words(initial)?;
                self.enter(*inner, &words);
            }
            Command::Again {
                body: target,
                arguments,
            } => {
                let words = self.words(arguments)?;
                self.enter(*target, &words);
            }
            Command::Leave {
                alternative,
                payload,
            } => {
                let words = self.words(payload)?;
                let Some(parent) = body.bodies[current].parent else {
                    return Err("a loop is left where no loop was entered".to_string());
                };
                let top = self.top();
                let statement = &body.bodies[parent].statements[top.positions[parent]];
                let Command::Loop { site, arms, .. } = &statement.command else {
                    return Err("a loop is left where no loop statement waits".to_string());
                };
                if let Some(bit) = plan.bit[*site][*alternative] {
                    top.bits[bit] = true;
                }
                for (var, word) in arms[*alternative].iter().zip(&words) {
                    top.slots[*var] = *word;
                }
                top.current = parent;
                top.positions[parent] += 1;
            }
            Command::Exit {
                alternative,
                payload,
            } => {
                let words = self.words(payload)?;
                self.stack.pop();
                if self.stack.is_empty() {
                    let entry = &program.controls[control].contract[*alternative];
                    return Ok(Some(End::Done {
                        alternative: entry.name.clone(),
                        payload: arith.values(&words, entry.fields.iter().map(|field| field.ty)),
                    }));
                }
                self.complete(*alternative, &words)?;
            }
            // [R2]: no code is generated for it.
            Command::Impossible => return Err("`⊥` has no behavior".to_string()),
        }
        Ok(None)
    }
}

/// Runs the control `entry` on `arguments` under the erased semantics. The
/// width of a `usize` is that of the configuration.
pub fn run(
    program: &Program,
    entry: &str,
    arguments: Vec<Value>,
    host: &mut dyn Host,
    config: &Config,
) -> Run {
    let refuse = |detail: String| Run {
        end: End::NotAnExecution(detail),
        trace: Vec::new(),
        steps: 0,
    };
    let Some(usize_bits) = (32..=64)
        .find(|bits| Int::power_of_two(*bits).minus(&Int::from_u64(1)) == config.usize_max)
    else {
        return refuse("no machine word has that greatest `usize`".to_string());
    };
    let Some(id) = program.control(entry) else {
        return refuse(format!("`{entry}` is not a control"));
    };
    let control = &program.controls[id];
    if control.body.is_none() {
        return refuse(format!("`{entry}` is an extern control"));
    }
    if arguments.len() != control.parameters.len() {
        return refuse(format!(
            "({}) are not arguments of `{entry}`",
            list(&arguments)
        ));
    }
    let plans: Vec<Option<Plan>> = program
        .controls
        .iter()
        .map(|control| control.body.as_ref().map(plan))
        .collect();
    let arith = Arith {
        program,
        usize_bits,
    };
    let words: Vec<u64> = arguments
        .iter()
        .zip(&control.parameters)
        .map(|(value, parameter)| arith.word(value, parameter.ty))
        .collect();
    let mut machine = Machine {
        program,
        config,
        arith,
        plans: &plans,
        host,
        stack: Vec::new(),
        trace: Vec::new(),
    };
    machine.push(id, &words);
    let mut steps = 0;
    let end = loop {
        if steps >= config.fuel {
            break End::OutOfFuel;
        }
        steps += 1;
        match machine.step() {
            Ok(None) => {}
            Ok(Some(end)) => break end,
            Err(detail) => break End::Undefined(detail),
        }
    };
    Run {
        end,
        trace: machine.trace,
        steps,
    }
}
