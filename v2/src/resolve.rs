//! From the syntax tree to the typed core.
//!
//! This is the part of checking that has to be done before a program can run
//! at all: names are resolved, expressions are typed ([C1]), and the rules of
//! well-formedness that are about the shape of the text are enforced ([W1] to
//! [W5], [W7], and the parts of [C2] and [C3] that are about names and
//! counts). Every error names the clause it comes from.
//!
//! What is left to the oracle, and to the checker of phase 3, is everything
//! that depends on which history runs: coverage, availability, missing arms
//! and proof obligations. A program that passes here can be run, and it is
//! stuck exactly where one of those is violated.

use crate::int::Int;
use crate::syntax;
use crate::typed::{
    Alternative, Arithmetic, Arm, Body, BodyId, Command, Comparison, Control, ControlBody,
    ControlId, Expr, Function, FunctionId, IntType, LoopHead, Parameter, Program, Selector, Site,
    SiteId, Statement, Type, Var, VarId, VarRef,
};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug)]
pub struct Error {
    pub line: u32,
    pub clause: &'static str,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {}: {} [{}]",
            self.line, self.message, self.clause
        )
    }
}

fn fail<T>(line: u32, clause: &'static str, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        line,
        clause,
        message: message.into(),
    })
}

/// An integer literal that nothing else types is a `u32` (surface, section
/// 13, choice 10).
const DEFAULT_INTEGER: Type = Type::Int(IntType::U32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Arithmetic belongs to a type, and a literal must be a value of it.
    Expression,
    /// Arithmetic is that of the mathematical integers ([S4], [E-Formula]).
    Formula,
}

trait Scope {
    fn find(&self, name: &str) -> Option<(VarRef, Type)>;
}

struct SignatureScope<'a> {
    parameters: &'a [Parameter],
    fields: &'a [Parameter],
}

impl Scope for SignatureScope<'_> {
    fn find(&self, name: &str) -> Option<(VarRef, Type)> {
        if let Some(index) = self.fields.iter().position(|field| field.name == name) {
            return Some((VarRef::Field(index), self.fields[index].ty));
        }
        let index = self
            .parameters
            .iter()
            .position(|parameter| parameter.name == name)?;
        Some((VarRef::Parameter(index), self.parameters[index].ty))
    }
}

struct Globals<'a> {
    function_ids: HashMap<&'a str, FunctionId>,
    function_types: Vec<(Vec<Type>, Type)>,
    control_ids: HashMap<&'a str, ControlId>,
}

struct Typer<'a> {
    globals: &'a Globals<'a>,
    scope: &'a dyn Scope,
    mode: Mode,
    line: u32,
}

impl Typer<'_> {
    fn error<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        fail(self.line, "C1", message)
    }

    fn function(&self, name: &str) -> Result<FunctionId, Error> {
        match self.globals.function_ids.get(name) {
            Some(id) => Ok(*id),
            None => self.error(format!("`{name}` is not a function")),
        }
    }

    /// The type an expression has by itself, or `None` for one made of
    /// integer literals only, which takes the type its place expects.
    fn kind(&self, expr: &syntax::Expr) -> Result<Option<Type>, Error> {
        Ok(match expr {
            syntax::Expr::Integer(_) => None,
            syntax::Expr::Boolean(_) => Some(Type::Bool),
            syntax::Expr::Name(name) => match self.scope.find(name) {
                Some((_, ty)) => Some(ty),
                None => return self.error(format!("`{name}` is not a name here")),
            },
            syntax::Expr::Unary(syntax::Unary::Not, _) => Some(Type::Bool),
            syntax::Expr::Unary(syntax::Unary::Negate, operand) => self.kind(operand)?,
            syntax::Expr::Binary(operator, left, right) => {
                if operator.level() <= 3 {
                    return Ok(Some(Type::Bool));
                }
                match (self.kind(left)?, self.kind(right)?) {
                    (Some(a), Some(b)) if a != b => {
                        return self.error(format!(
                            "the operands of `{}` have the types {a} and {b}",
                            operator.glyph()
                        ));
                    }
                    (Some(a), _) | (_, Some(a)) => Some(a),
                    (None, None) => None,
                }
            }
            syntax::Expr::Call(name, _) => {
                Some(self.globals.function_types[self.function(name)?].1)
            }
        })
    }

    fn infer(&self, expr: &syntax::Expr) -> Result<Expr, Error> {
        let ty = self.kind(expr)?.unwrap_or(DEFAULT_INTEGER);
        self.build(expr, ty)
    }

    fn literal(&self, value: Int, expected: Type) -> Result<Expr, Error> {
        let Type::Int(ty) = expected else {
            return self.error(format!(
                "the integer {value} stands where a bool is expected"
            ));
        };
        if self.mode == Mode::Expression {
            let (least, greatest) = ty.literal_bounds();
            if value < least || value > greatest {
                return self.error(format!(
                    "{value} is not a value of {} on every target",
                    ty.name()
                ));
            }
        }
        Ok(Expr::Integer(value, ty))
    }

    fn integer_type(&self, expected: Type, what: &str) -> Result<IntType, Error> {
        match expected {
            Type::Int(ty) => Ok(ty),
            Type::Bool => self.error(format!("{what} stands where a bool is expected")),
        }
    }

    fn build(&self, expr: &syntax::Expr, expected: Type) -> Result<Expr, Error> {
        match expr {
            syntax::Expr::Integer(value) => self.literal(value.clone(), expected),
            syntax::Expr::Boolean(value) => {
                if expected != Type::Bool {
                    return self.error(format!("a bool stands where {expected} is expected"));
                }
                Ok(Expr::Boolean(*value))
            }
            syntax::Expr::Name(name) => {
                let Some((var, ty)) = self.scope.find(name) else {
                    return self.error(format!("`{name}` is not a name here"));
                };
                if ty != expected {
                    return self.error(format!(
                        "`{name}` has the type {ty}, and {expected} is expected"
                    ));
                }
                Ok(Expr::Var(var, ty))
            }
            syntax::Expr::Unary(syntax::Unary::Not, operand) => {
                if expected != Type::Bool {
                    return self.error(format!("`¬` stands where {expected} is expected"));
                }
                Ok(Expr::Not(Box::new(self.build(operand, Type::Bool)?)))
            }
            syntax::Expr::Unary(syntax::Unary::Negate, operand) => {
                let ty = self.integer_type(expected, "a negation")?;
                if let syntax::Expr::Integer(value) = &**operand {
                    return self.literal(value.negate(), expected);
                }
                Ok(Expr::Negate(Box::new(self.build(operand, expected)?), ty))
            }
            syntax::Expr::Binary(operator, left, right) => {
                self.binary(*operator, left, right, expected)
            }
            syntax::Expr::Call(name, arguments) => {
                let id = self.function(name)?;
                let (parameters, result) = &self.globals.function_types[id];
                if *result != expected {
                    return self.error(format!(
                        "`{name}` yields {result}, and {expected} is expected"
                    ));
                }
                if parameters.len() != arguments.len() {
                    return self.error(format!(
                        "`{name}` takes {} arguments, and {} are given",
                        parameters.len(),
                        arguments.len()
                    ));
                }
                let mut built = Vec::new();
                for (argument, ty) in arguments.iter().zip(parameters) {
                    built.push(self.build(argument, *ty)?);
                }
                Ok(Expr::Call(id, built, *result))
            }
        }
    }

    fn binary(
        &self,
        operator: syntax::Binary,
        left: &syntax::Expr,
        right: &syntax::Expr,
        expected: Type,
    ) -> Result<Expr, Error> {
        use syntax::Binary as B;
        let arithmetic = match operator {
            B::Add => Some(Arithmetic::Add),
            B::Subtract => Some(Arithmetic::Subtract),
            B::Multiply => Some(Arithmetic::Multiply),
            B::Divide => Some(Arithmetic::Divide),
            B::Remainder => Some(Arithmetic::Remainder),
            _ => None,
        };
        if let Some(arithmetic) = arithmetic {
            let ty = self.integer_type(expected, "arithmetic")?;
            return Ok(Expr::Arithmetic(
                arithmetic,
                Box::new(self.build(left, expected)?),
                Box::new(self.build(right, expected)?),
                ty,
            ));
        }
        if expected != Type::Bool {
            return self.error(format!(
                "`{}` yields a bool, and {expected} is expected",
                operator.glyph()
            ));
        }
        match operator {
            B::And | B::Or => {
                let left = Box::new(self.build(left, Type::Bool)?);
                let right = Box::new(self.build(right, Type::Bool)?);
                Ok(if operator == B::And {
                    Expr::And(left, right)
                } else {
                    Expr::Or(left, right)
                })
            }
            _ => {
                let operand = match (self.kind(left)?, self.kind(right)?) {
                    (Some(a), Some(b)) if a != b => {
                        return self.error(format!(
                            "the operands of `{}` have the types {a} and {b}",
                            operator.glyph()
                        ));
                    }
                    (Some(a), _) | (_, Some(a)) => a,
                    (None, None) => DEFAULT_INTEGER,
                };
                let comparison = match operator {
                    B::Equal => Comparison::Equal,
                    B::NotEqual => Comparison::NotEqual,
                    B::Less => Comparison::Less,
                    B::LessEqual => Comparison::LessEqual,
                    B::Greater => Comparison::Greater,
                    _ => Comparison::GreaterEqual,
                };
                let ordered = !matches!(comparison, Comparison::Equal | Comparison::NotEqual);
                if operand == Type::Bool && ordered {
                    return self.error(format!("`{}` does not order bools", operator.glyph()));
                }
                Ok(Expr::Compare(
                    comparison,
                    Box::new(self.build(left, operand)?),
                    Box::new(self.build(right, operand)?),
                ))
            }
        }
    }
}

struct PendingLeave {
    statement: usize,
    alternative: usize,
    kinds: Vec<Option<Type>>,
    payload: Vec<syntax::Expr>,
    line: u32,
}

struct LoopFrame {
    name: String,
    parameters: Vec<VarId>,
    alternatives: Vec<String>,
    arity: Vec<usize>,
    leaves: Vec<PendingLeave>,
}

struct Frame {
    body: BodyId,
    names: HashMap<String, VarId>,
    /// Names bound in bodies nested in this one ([W4]).
    nested: HashSet<String>,
    /// The sites a selector of this body may mention so far ([W2]).
    sites: HashMap<String, SiteId>,
    looping: Option<LoopFrame>,
}

struct Builder<'a> {
    globals: &'a Globals<'a>,
    controls: &'a [Control],
    control: ControlId,
    vars: Vec<Var>,
    sites: Vec<Site>,
    bodies: Vec<Body>,
    site_names: HashSet<String>,
    frames: Vec<Frame>,
}

impl Scope for Builder<'_> {
    fn find(&self, name: &str) -> Option<(VarRef, Type)> {
        let id = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| frame.names.get(name))?;
        Some((VarRef::Local(*id), self.vars[*id].ty))
    }
}

impl<'a> Builder<'a> {
    fn typer(&self, mode: Mode, line: u32) -> Typer<'_> {
        Typer {
            globals: self.globals,
            scope: self,
            mode,
            line,
        }
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a frame")
    }

    fn declare(&mut self, name: &str, ty: Type, mutable: bool, line: u32) -> Result<VarId, Error> {
        let current = self.frames.len() - 1;
        if let Some(id) = self.frames[current].names.get(name).copied() {
            let var = &self.vars[id];
            if var.ty != ty || var.mutable != mutable {
                return fail(
                    line,
                    "W4",
                    format!("the binders of `{name}` in one body differ in type or mutability"),
                );
            }
            return Ok(id);
        }
        let outer = self.frames[..current]
            .iter()
            .any(|frame| frame.names.contains_key(name));
        if outer || self.frames[current].nested.contains(name) {
            return fail(
                line,
                "W4",
                format!("`{name}` is bound in two nested bodies"),
            );
        }
        let body = self.frames[current].body;
        self.vars.push(Var {
            name: name.to_string(),
            ty,
            mutable,
            body,
        });
        let id = self.vars.len() - 1;
        self.frames[current].names.insert(name.to_string(), id);
        Ok(id)
    }

    fn new_site(
        &mut self,
        name: &str,
        alternatives: Vec<String>,
        line: u32,
    ) -> Result<SiteId, Error> {
        if !self.site_names.insert(name.to_string()) {
            return fail(
                line,
                "W1",
                format!("the site name `{name}` is used twice in one control"),
            );
        }
        let body = self.frame().body;
        self.sites.push(Site {
            name: name.to_string(),
            alternatives,
            body,
        });
        Ok(self.sites.len() - 1)
    }

    fn selector(&self, selector: &syntax::Selector, line: u32) -> Result<Selector, Error> {
        Ok(match selector {
            syntax::Selector::Top => Selector::Top,
            syntax::Selector::Not(inner) => Selector::Not(Box::new(self.selector(inner, line)?)),
            syntax::Selector::And(left, right) => Selector::And(
                Box::new(self.selector(left, line)?),
                Box::new(self.selector(right, line)?),
            ),
            syntax::Selector::Or(left, right) => Selector::Or(
                Box::new(self.selector(left, line)?),
                Box::new(self.selector(right, line)?),
            ),
            syntax::Selector::Atom(site, alternative) => {
                let Some(id) = self
                    .frames
                    .iter()
                    .rev()
                    .find_map(|frame| frame.sites.get(site))
                else {
                    return fail(
                        line,
                        "W2",
                        format!(
                            "no site `{site}` precedes this statement in its body or around it"
                        ),
                    );
                };
                let Some(index) = self.sites[*id]
                    .alternatives
                    .iter()
                    .position(|name| name == alternative)
                else {
                    return fail(
                        line,
                        "W2",
                        format!("the site `{site}` has no alternative `{alternative}`"),
                    );
                };
                Selector::Atom(*id, index)
            }
        })
    }

    fn push(&mut self, command: Command, selector: Selector, line: u32) {
        let body = self.frame().body;
        self.bodies[body].statements.push(Statement {
            selector,
            command,
            line,
        });
    }

    fn statements(&mut self, statements: &[syntax::Statement]) -> Result<(), Error> {
        for statement in statements {
            let line = statement.line;
            let selector = self.selector(&statement.selector, line)?;
            let command = self.command(&statement.command, line)?;
            self.push(command, selector, line);
        }
        Ok(())
    }

    fn arguments(
        &self,
        what: &str,
        arguments: &[syntax::Expr],
        types: &[Type],
        clause: &'static str,
        line: u32,
    ) -> Result<Vec<Expr>, Error> {
        if arguments.len() != types.len() {
            return fail(
                line,
                clause,
                format!(
                    "{what} takes {} values, and {} are given",
                    types.len(),
                    arguments.len()
                ),
            );
        }
        let typer = self.typer(Mode::Expression, line);
        arguments
            .iter()
            .zip(types)
            .map(|(argument, ty)| typer.build(argument, *ty))
            .collect()
    }

    fn command(&mut self, command: &syntax::Command, line: u32) -> Result<Command, Error> {
        match command {
            syntax::Command::Bind {
                mutable,
                name,
                ty,
                value,
            } => {
                let known = self
                    .frames
                    .last()
                    .and_then(|frame| frame.names.get(name))
                    .copied();
                let expected = ty.or(known.map(|id| self.vars[id].ty));
                let typer = self.typer(Mode::Expression, line);
                let value = match expected {
                    Some(expected) => typer.build(value, expected)?,
                    None => typer.infer(value)?,
                };
                let var = self.declare(name, value.ty(), *mutable, line)?;
                Ok(Command::Bind { var, value })
            }
            syntax::Command::Assign { name, value } => {
                let Some((VarRef::Local(var), ty)) = self.find(name) else {
                    return fail(line, "C1", format!("`{name}` is not a name here"));
                };
                if !self.vars[var].mutable {
                    return fail(line, "C1", format!("`{name}` is not a mutable place"));
                }
                let value = self.typer(Mode::Expression, line).build(value, ty)?;
                Ok(Command::Assign { var, value })
            }
            syntax::Command::Oblige(formula) => Ok(Command::Oblige(
                self.typer(Mode::Formula, line).build(formula, Type::Bool)?,
            )),
            syntax::Command::Test { site, condition } => {
                let condition = self
                    .typer(Mode::Expression, line)
                    .build(condition, Type::Bool)?;
                let id = self.new_site(site, vec!["true".into(), "false".into()], line)?;
                self.frame().sites.insert(site.clone(), id);
                Ok(Command::Test {
                    site: id,
                    condition,
                })
            }
            syntax::Command::Decide {
                site,
                operation,
                arguments,
                arms,
            } => self.decide(site, operation, arguments, arms, line),
            syntax::Command::Loop {
                site,
                name,
                parameters,
                invariant,
                body,
                arms,
            } => self.looping(site, name, parameters, invariant.as_ref(), body, arms, line),
            syntax::Command::Again { name, arguments } => {
                let Some(frame) = self.frames.iter().rev().find(|frame| {
                    frame
                        .looping
                        .as_ref()
                        .is_some_and(|looping| looping.name == *name)
                }) else {
                    return fail(
                        line,
                        "W3",
                        format!("`↑ {name}` is not inside the loop `{name}`"),
                    );
                };
                let body = frame.body;
                let looping = frame.looping.as_ref().expect("a loop frame");
                let types: Vec<Type> = looping
                    .parameters
                    .iter()
                    .map(|var| self.vars[*var].ty)
                    .collect();
                let arguments =
                    self.arguments(&format!("the loop `{name}`"), arguments, &types, "C1", line)?;
                Ok(Command::Again { body, arguments })
            }
            syntax::Command::Leave {
                alternative,
                payload,
            } => {
                let typer = self.typer(Mode::Expression, line);
                let mut kinds = Vec::new();
                for operand in payload {
                    kinds.push(typer.kind(operand)?);
                }
                let statement = self.bodies[self.frames.last().expect("a frame").body]
                    .statements
                    .len();
                let Some(looping) = self.frame().looping.as_mut() else {
                    return fail(line, "W3", "`□` is not inside a loop body");
                };
                let Some(index) = looping
                    .alternatives
                    .iter()
                    .position(|name| name == alternative)
                else {
                    return fail(
                        line,
                        "C3",
                        format!("the loop statement does not list the alternative `{alternative}`"),
                    );
                };
                if looping.arity[index] != payload.len() {
                    return fail(
                        line,
                        "C3",
                        format!(
                            "`{alternative}` has {} payload names at the loop statement, and {} values leave",
                            looping.arity[index],
                            payload.len()
                        ),
                    );
                }
                looping.leaves.push(PendingLeave {
                    statement,
                    alternative: index,
                    kinds,
                    payload: payload.clone(),
                    line,
                });
                // The payload is typed when the loop body is complete, since
                // all `□` of one alternative agree on its types ([C3]).
                Ok(Command::Leave {
                    alternative: index,
                    payload: Vec::new(),
                })
            }
            syntax::Command::Exit {
                alternative,
                payload,
            } => {
                let contract = &self.controls[self.control].contract;
                let Some(index) = contract.iter().position(|entry| entry.name == *alternative)
                else {
                    return fail(
                        line,
                        "C3",
                        format!("`{alternative}` is not an alternative of the control's contract"),
                    );
                };
                let types: Vec<Type> = contract[index]
                    .fields
                    .iter()
                    .map(|field| field.ty)
                    .collect();
                let payload = self.arguments(
                    &format!("the alternative `{alternative}`"),
                    payload,
                    &types,
                    "C3",
                    line,
                )?;
                Ok(Command::Exit {
                    alternative: index,
                    payload,
                })
            }
            syntax::Command::Impossible => Ok(Command::Impossible),
        }
    }

    fn decide(
        &mut self,
        site: &str,
        operation: &str,
        arguments: &[syntax::Expr],
        arms: &[syntax::Arm],
        line: u32,
    ) -> Result<Command, Error> {
        let Some(callee) = self.globals.control_ids.get(operation).copied() else {
            return fail(line, "C1", format!("`{operation}` is not a control"));
        };
        let controls = self.controls;
        let types: Vec<Type> = controls[callee]
            .parameters
            .iter()
            .map(|parameter| parameter.ty)
            .collect();
        let arguments = self.arguments(&format!("`{operation}`"), arguments, &types, "C1", line)?;
        let contract = &controls[callee].contract;
        let mut listed = Vec::new();
        for arm in arms {
            let Some(index) = contract
                .iter()
                .position(|entry| entry.name == arm.alternative)
            else {
                return fail(
                    line,
                    "C2",
                    format!(
                        "`{}` is not an alternative of `{operation}`",
                        arm.alternative
                    ),
                );
            };
            if listed.contains(&index) {
                return fail(
                    line,
                    "C2",
                    format!("the alternative `{}` is listed twice", arm.alternative),
                );
            }
            if arm.names.len() != contract[index].fields.len() {
                return fail(
                    line,
                    "C2",
                    format!(
                        "`{}` has {} payload fields, and {} names are given",
                        arm.alternative,
                        contract[index].fields.len(),
                        arm.names.len()
                    ),
                );
            }
            listed.push(index);
        }
        let names = arms.iter().map(|arm| arm.alternative.clone()).collect();
        let id = self.new_site(site, names, line)?;
        let mut built = Vec::new();
        for (arm, index) in arms.iter().zip(&listed) {
            let mut vars = Vec::new();
            for (name, field) in arm.names.iter().zip(&contract[*index].fields) {
                vars.push(self.declare(name, field.ty, false, line)?);
            }
            built.push(Arm {
                alternative: *index,
                vars,
            });
        }
        self.frame().sites.insert(site.to_string(), id);
        Ok(Command::Decide {
            site: id,
            operation: callee,
            arguments,
            arms: built,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn looping(
        &mut self,
        site: &str,
        name: &str,
        parameters: &[syntax::LoopParameter],
        invariant: Option<&syntax::Expr>,
        body: &[syntax::Statement],
        arms: &[syntax::Arm],
        line: u32,
    ) -> Result<Command, Error> {
        let mut initial = Vec::new();
        for parameter in parameters {
            initial.push(
                self.typer(Mode::Expression, line)
                    .build(&parameter.initial, parameter.ty)?,
            );
        }
        let mut alternatives: Vec<String> = Vec::new();
        for arm in arms {
            if alternatives.contains(&arm.alternative) {
                return fail(
                    line,
                    "C3",
                    format!("the alternative `{}` is listed twice", arm.alternative),
                );
            }
            alternatives.push(arm.alternative.clone());
        }
        let id = self.new_site(site, alternatives.clone(), line)?;
        let parent = self.frame().body;
        self.bodies.push(Body {
            statements: Vec::new(),
            parent: Some(parent),
            head: None,
        });
        let inner = self.bodies.len() - 1;
        self.frames.push(Frame {
            body: inner,
            names: HashMap::new(),
            nested: HashSet::new(),
            sites: HashMap::new(),
            looping: Some(LoopFrame {
                name: name.to_string(),
                parameters: Vec::new(),
                alternatives,
                arity: arms.iter().map(|arm| arm.names.len()).collect(),
                leaves: Vec::new(),
            }),
        });
        let mut vars = Vec::new();
        for parameter in parameters {
            if vars
                .iter()
                .any(|var: &VarId| self.vars[*var].name == parameter.name)
            {
                return fail(
                    line,
                    "W4",
                    format!("the loop parameter `{}` is named twice", parameter.name),
                );
            }
            vars.push(self.declare(&parameter.name, parameter.ty, parameter.mutable, line)?);
        }
        self.frame()
            .looping
            .as_mut()
            .expect("a loop frame")
            .parameters = vars.clone();
        let invariant = match invariant {
            Some(formula) => Some(self.typer(Mode::Formula, line).build(formula, Type::Bool)?),
            None => None,
        };
        self.bodies[inner].head = Some(LoopHead {
            name: name.to_string(),
            site: id,
            parameters: vars,
            invariant,
        });
        self.statements(body)?;

        // All `□` of one alternative agree on the types of its payload ([C3]).
        // A literal takes the type the others give that place.
        let looping = self.frame().looping.take().expect("a loop frame");
        let mut types: Vec<Vec<Type>> = Vec::new();
        for (index, arm) in arms.iter().enumerate() {
            let leaves: Vec<&PendingLeave> = looping
                .leaves
                .iter()
                .filter(|leave| leave.alternative == index)
                .collect();
            if leaves.is_empty() {
                return fail(
                    line,
                    "C3",
                    format!(
                        "no `□` of the loop body uses the listed alternative `{}`",
                        arm.alternative
                    ),
                );
            }
            let mut row = Vec::new();
            for place in 0..arm.names.len() {
                let mut decided = None;
                for leave in &leaves {
                    match (decided, leave.kinds[place]) {
                        (Some(a), Some(b)) if a != b => {
                            return fail(
                                leave.line,
                                "C3",
                                format!(
                                    "the `□ {}` statements differ in a payload type: {a} and {b}",
                                    arm.alternative
                                ),
                            );
                        }
                        (None, Some(b)) => decided = Some(b),
                        _ => {}
                    }
                }
                row.push(decided.unwrap_or(DEFAULT_INTEGER));
            }
            types.push(row);
        }
        for leave in &looping.leaves {
            let payload = self.arguments(
                "the alternative",
                &leave.payload,
                &types[leave.alternative],
                "C3",
                leave.line,
            )?;
            self.bodies[inner].statements[leave.statement].command = Command::Leave {
                alternative: leave.alternative,
                payload,
            };
        }

        let finished = self.frames.pop().expect("a loop frame");
        let outer = self.frame();
        outer.nested.extend(finished.names.into_keys());
        outer.nested.extend(finished.nested);
        let mut bound = Vec::new();
        for (arm, row) in arms.iter().zip(&types) {
            let mut vars = Vec::new();
            for (name, ty) in arm.names.iter().zip(row) {
                vars.push(self.declare(name, *ty, false, line)?);
            }
            bound.push(vars);
        }
        self.frame().sites.insert(site.to_string(), id);
        Ok(Command::Loop {
            site: id,
            body: inner,
            initial,
            arms: bound,
        })
    }
}

fn parameters(
    parameters: &[syntax::Parameter],
    line: u32,
    what: &str,
) -> Result<Vec<Parameter>, Error> {
    let mut out: Vec<Parameter> = Vec::new();
    for parameter in parameters {
        if out.iter().any(|earlier| earlier.name == parameter.name) {
            return fail(
                line,
                "W4",
                format!("{what} `{}` is named twice", parameter.name),
            );
        }
        out.push(Parameter {
            name: parameter.name.clone(),
            ty: parameter.ty,
        });
    }
    Ok(out)
}

fn calls(expr: &Expr, out: &mut Vec<FunctionId>) {
    match expr {
        Expr::Boolean(_) | Expr::Integer(_, _) | Expr::Var(_, _) => {}
        Expr::Negate(inner, _) | Expr::Not(inner) => calls(inner, out),
        Expr::Arithmetic(_, left, right, _)
        | Expr::Compare(_, left, right)
        | Expr::And(left, right)
        | Expr::Or(left, right) => {
            calls(left, out);
            calls(right, out);
        }
        Expr::Call(id, arguments, _) => {
            out.push(*id);
            for argument in arguments {
                calls(argument, out);
            }
        }
    }
}

/// Resolves and types a program, or reports the first error with its clause.
pub fn resolve(program: &syntax::Program) -> Result<Program, Error> {
    let mut source_functions = Vec::new();
    let mut source_controls = Vec::new();
    for declaration in &program.declarations {
        match declaration {
            syntax::Declaration::Function(function) => source_functions.push(function),
            syntax::Declaration::Control(control) => source_controls.push(control),
        }
    }

    let mut globals = Globals {
        function_ids: HashMap::new(),
        function_types: Vec::new(),
        control_ids: HashMap::new(),
    };
    let mut functions: Vec<Function> = Vec::new();
    for function in &source_functions {
        if globals
            .function_ids
            .insert(&function.name, functions.len())
            .is_some()
        {
            return fail(
                function.line,
                "C1",
                format!("`{}` is declared twice", function.name),
            );
        }
        let parameters = parameters(&function.parameters, function.line, "the parameter")?;
        globals.function_types.push((
            parameters.iter().map(|parameter| parameter.ty).collect(),
            function.result,
        ));
        functions.push(Function {
            name: function.name.clone(),
            parameters,
            precondition: None,
            result: function.result,
            body: Expr::Boolean(false),
        });
    }
    let mut controls: Vec<Control> = Vec::new();
    for control in &source_controls {
        let line = control.line;
        if globals.function_ids.contains_key(control.name.as_str())
            || globals
                .control_ids
                .insert(&control.name, controls.len())
                .is_some()
        {
            return fail(line, "C1", format!("`{}` is declared twice", control.name));
        }
        let parameters = parameters(&control.parameters, line, "the parameter")?;
        let mut contract: Vec<Alternative> = Vec::new();
        for alternative in &control.contract {
            if contract
                .iter()
                .any(|earlier| earlier.name == alternative.name)
            {
                return fail(
                    line,
                    "W7",
                    format!("the contract lists `{}` twice", alternative.name),
                );
            }
            let fields = self::parameters(&alternative.fields, line, "the payload field")?;
            if let Some(field) = fields
                .iter()
                .find(|field| parameters.iter().any(|p| p.name == field.name))
            {
                return fail(
                    line,
                    "W7",
                    format!(
                        "the payload field `{}` has the name of a parameter",
                        field.name
                    ),
                );
            }
            contract.push(Alternative {
                name: alternative.name.clone(),
                fields,
                condition: None,
            });
        }
        controls.push(Control {
            name: control.name.clone(),
            public: control.public,
            parameters,
            precondition: None,
            contract,
            uses: control.uses.clone(),
            body: None,
        });
    }

    // Formulas of signatures, and the bodies of functions.
    for (id, function) in source_functions.iter().enumerate() {
        let scope = SignatureScope {
            parameters: &functions[id].parameters,
            fields: &[],
        };
        let typer = |mode| Typer {
            globals: &globals,
            scope: &scope,
            mode,
            line: function.line,
        };
        let precondition = match &function.precondition {
            Some(formula) => Some(typer(Mode::Formula).build(formula, Type::Bool)?),
            None => None,
        };
        let body = typer(Mode::Expression).build(&function.body, function.result)?;
        functions[id].precondition = precondition;
        functions[id].body = body;
    }
    for (id, control) in source_controls.iter().enumerate() {
        let line = control.line;
        let formula = |formula: &syntax::Expr, fields: &[Parameter]| {
            let scope = SignatureScope {
                parameters: &controls[id].parameters,
                fields,
            };
            Typer {
                globals: &globals,
                scope: &scope,
                mode: Mode::Formula,
                line,
            }
            .build(formula, Type::Bool)
        };
        let precondition = match &control.precondition {
            Some(text) => Some(formula(text, &[])?),
            None => None,
        };
        let mut conditions = Vec::new();
        for (index, alternative) in control.contract.iter().enumerate() {
            conditions.push(match &alternative.condition {
                Some(text) => Some(formula(text, &controls[id].contract[index].fields)?),
                None => None,
            });
        }
        controls[id].precondition = precondition;
        for (index, condition) in conditions.into_iter().enumerate() {
            controls[id].contract[index].condition = condition;
        }
    }

    // [W5]: functions are not recursive.
    let mut edges: Vec<Vec<FunctionId>> = Vec::new();
    for function in &functions {
        let mut out = Vec::new();
        calls(&function.body, &mut out);
        if let Some(precondition) = &function.precondition {
            calls(precondition, &mut out);
        }
        edges.push(out);
    }
    for start in 0..functions.len() {
        let mut pending = edges[start].clone();
        let mut seen = HashSet::new();
        while let Some(next) = pending.pop() {
            if next == start {
                return fail(
                    source_functions[start].line,
                    "W5",
                    format!("the function `{}` reaches itself", functions[start].name),
                );
            }
            if seen.insert(next) {
                pending.extend(edges[next].iter().copied());
            }
        }
    }

    // Bodies of controls.
    let mut bodies = Vec::new();
    for (id, control) in source_controls.iter().enumerate() {
        if control.external {
            bodies.push(None);
            continue;
        }
        let mut builder = Builder {
            globals: &globals,
            controls: &controls,
            control: id,
            vars: Vec::new(),
            sites: Vec::new(),
            bodies: vec![Body {
                statements: Vec::new(),
                parent: None,
                head: None,
            }],
            site_names: HashSet::new(),
            frames: vec![Frame {
                body: 0,
                names: HashMap::new(),
                nested: HashSet::new(),
                sites: HashMap::new(),
                looping: None,
            }],
        };
        for parameter in &controls[id].parameters {
            builder.declare(&parameter.name, parameter.ty, false, control.line)?;
        }
        builder.statements(&control.body)?;
        bodies.push(Some(ControlBody {
            vars: builder.vars,
            sites: builder.sites,
            bodies: builder.bodies,
        }));
    }
    for (control, body) in controls.iter_mut().zip(bodies) {
        control.body = body;
    }
    Ok(Program {
        functions,
        controls,
    })
}
