//! The core language as it is written: the syntax tree of the core text form
//! and its printer.
//!
//! This is the grammar of section 3 of `docs/v2/core.md`, restricted to the
//! constructs of the current slice. Names are still text here; `resolve`
//! turns the tree into the typed core.

use crate::int::Int;
use crate::typed::Type;
use std::fmt::Write;

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug)]
pub enum Declaration {
    Function(Function),
    Control(Control),
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub precondition: Option<Expr>,
    pub result: Type,
    pub body: Expr,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Alternative {
    pub name: String,
    pub fields: Vec<Parameter>,
    pub condition: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Control {
    pub name: String,
    pub public: bool,
    pub external: bool,
    pub parameters: Vec<Parameter>,
    pub precondition: Option<Expr>,
    pub contract: Vec<Alternative>,
    pub uses: Vec<String>,
    pub body: Vec<Statement>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Statement {
    pub selector: Selector,
    pub command: Command,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub enum Selector {
    Top,
    Atom(String, String),
    Not(Box<Selector>),
    And(Box<Selector>, Box<Selector>),
    Or(Box<Selector>, Box<Selector>),
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub alternative: String,
    pub names: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LoopParameter {
    pub mutable: bool,
    pub name: String,
    pub ty: Type,
    pub initial: Expr,
}

#[derive(Clone, Debug)]
pub enum Command {
    Bind {
        mutable: bool,
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    Assign {
        name: String,
        value: Expr,
    },
    Oblige(Expr),
    Test {
        site: String,
        condition: Expr,
    },
    Decide {
        site: String,
        operation: String,
        arguments: Vec<Expr>,
        arms: Vec<Arm>,
    },
    Loop {
        site: String,
        name: String,
        parameters: Vec<LoopParameter>,
        invariant: Option<Expr>,
        body: Vec<Statement>,
        arms: Vec<Arm>,
    },
    Again {
        name: String,
        arguments: Vec<Expr>,
    },
    Leave {
        alternative: String,
        payload: Vec<Expr>,
    },
    Exit {
        alternative: String,
        payload: Vec<Expr>,
    },
    Impossible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unary {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binary {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl Binary {
    /// Loosest first, as section 4.3 of the surface document orders them.
    pub fn level(self) -> u8 {
        match self {
            Binary::Or => 1,
            Binary::And => 2,
            Binary::Equal
            | Binary::NotEqual
            | Binary::Less
            | Binary::LessEqual
            | Binary::Greater
            | Binary::GreaterEqual => 3,
            Binary::Add | Binary::Subtract => 4,
            Binary::Multiply | Binary::Divide | Binary::Remainder => 5,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Binary::Or => "∨",
            Binary::And => "∧",
            Binary::Equal => "=",
            Binary::NotEqual => "≠",
            Binary::Less => "<",
            Binary::LessEqual => "≤",
            Binary::Greater => ">",
            Binary::GreaterEqual => "≥",
            Binary::Add => "+",
            Binary::Subtract => "-",
            Binary::Multiply => "*",
            Binary::Divide => "÷",
            Binary::Remainder => "mod",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Integer(Int),
    Boolean(bool),
    Name(String),
    Unary(Unary, Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

// Printing. The output is the glyph form, and reading it back gives the same
// tree.

const UNARY_LEVEL: u8 = 6;

fn expr_level(expr: &Expr) -> u8 {
    match expr {
        Expr::Binary(operator, _, _) => operator.level(),
        Expr::Unary(_, _) => UNARY_LEVEL,
        Expr::Integer(value) if value.is_negative() => UNARY_LEVEL,
        _ => 7,
    }
}

fn print_expr(out: &mut String, expr: &Expr, minimum: u8) {
    let level = expr_level(expr);
    if level < minimum {
        out.push('(');
    }
    match expr {
        Expr::Integer(value) => {
            let _ = write!(out, "{value}");
        }
        Expr::Boolean(value) => {
            let _ = write!(out, "{value}");
        }
        Expr::Name(name) => out.push_str(name),
        Expr::Unary(operator, operand) => {
            out.push_str(match operator {
                Unary::Negate => "-",
                Unary::Not => "¬",
            });
            print_expr(out, operand, UNARY_LEVEL);
        }
        Expr::Binary(operator, left, right) => {
            // Comparisons do not associate, so both sides bind tighter. The
            // other operators associate to the left.
            let left_minimum = if level == 3 { level + 1 } else { level };
            print_expr(out, left, left_minimum);
            let _ = write!(out, " {} ", operator.glyph());
            print_expr(out, right, level + 1);
        }
        Expr::Call(name, arguments) => {
            out.push_str(name);
            out.push('(');
            print_list(out, arguments);
            out.push(')');
        }
    }
    if level < minimum {
        out.push(')');
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        print_expr(&mut s, self, 0);
        formatter.write_str(&s)
    }
}

fn print_list(out: &mut String, items: &[Expr]) {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        print_expr(out, item, 0);
    }
}

fn selector_level(selector: &Selector) -> u8 {
    match selector {
        Selector::Or(_, _) => 1,
        Selector::And(_, _) => 2,
        Selector::Not(_) => 3,
        Selector::Top | Selector::Atom(_, _) => 4,
    }
}

fn print_selector(out: &mut String, selector: &Selector, minimum: u8) {
    let level = selector_level(selector);
    if level < minimum {
        out.push('(');
    }
    match selector {
        Selector::Top => out.push('⊤'),
        Selector::Atom(site, alternative) => {
            let _ = write!(out, "{site}.{alternative}");
        }
        Selector::Not(inner) => {
            out.push('¬');
            print_selector(out, inner, 3);
        }
        Selector::And(left, right) => {
            print_selector(out, left, 2);
            out.push_str(" ∧ ");
            print_selector(out, right, 3);
        }
        Selector::Or(left, right) => {
            print_selector(out, left, 1);
            out.push_str(" ∨ ");
            print_selector(out, right, 2);
        }
    }
    if level < minimum {
        out.push(')');
    }
}

fn print_parameters(out: &mut String, parameters: &[Parameter]) {
    for (index, parameter) in parameters.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{} : {}", parameter.name, parameter.ty);
    }
}

fn print_arms(out: &mut String, arms: &[Arm]) {
    for (index, arm) in arms.iter().enumerate() {
        out.push_str(if index == 0 { " ▸ " } else { " │ " });
        let _ = write!(out, "{} {{{}}}", arm.alternative, arm.names.join(", "));
    }
}

fn print_payload(out: &mut String, payload: &[Expr]) {
    out.push_str(" {");
    print_list(out, payload);
    out.push('}');
}

fn print_statements(out: &mut String, statements: &[Statement], indent: usize) {
    for statement in statements {
        let pad = " ".repeat(indent);
        out.push_str(&pad);
        print_selector(out, &statement.selector, 0);
        out.push_str(" ▷ ");
        match &statement.command {
            Command::Bind {
                mutable,
                name,
                ty,
                value,
            } => {
                if *mutable {
                    out.push('~');
                }
                out.push_str(name);
                if let Some(ty) = ty {
                    let _ = write!(out, " : {ty}");
                }
                out.push_str(" := ");
                print_expr(out, value, 0);
            }
            Command::Assign { name, value } => {
                let _ = write!(out, "{name} ← ");
                print_expr(out, value, 0);
            }
            Command::Oblige(formula) => {
                out.push_str("⊢ ");
                print_expr(out, formula, 0);
            }
            Command::Test { site, condition } => {
                let _ = write!(out, "{site} : ? ");
                print_expr(out, condition, 0);
            }
            Command::Decide {
                site,
                operation,
                arguments,
                arms,
            } => {
                let _ = write!(out, "{site} : Δ {operation}(");
                print_list(out, arguments);
                out.push(')');
                print_arms(out, arms);
            }
            Command::Loop {
                site,
                name,
                parameters,
                invariant,
                body,
                arms,
            } => {
                let _ = write!(out, "{site} : ↻ {name}(");
                for (index, parameter) in parameters.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    if parameter.mutable {
                        out.push('~');
                    }
                    let _ = write!(out, "{} : {} := ", parameter.name, parameter.ty);
                    print_expr(out, &parameter.initial, 0);
                }
                out.push_str(") {\n");
                if let Some(invariant) = invariant {
                    let _ = write!(out, "{pad}    ⊢ ");
                    print_expr(out, invariant, 0);
                    out.push('\n');
                }
                print_statements(out, body, indent + 4);
                let _ = write!(out, "{pad}}}");
                print_arms(out, arms);
            }
            Command::Again { name, arguments } => {
                let _ = write!(out, "↑ {name}(");
                print_list(out, arguments);
                out.push(')');
            }
            Command::Leave {
                alternative,
                payload,
            } => {
                let _ = write!(out, "□ {alternative}");
                print_payload(out, payload);
            }
            Command::Exit {
                alternative,
                payload,
            } => {
                let _ = write!(out, "■ {alternative}");
                print_payload(out, payload);
            }
            Command::Impossible => out.push('⊥'),
        }
        out.push('\n');
    }
}

fn print_signature(out: &mut String, control: &Control) {
    if let Some(precondition) = &control.precondition {
        out.push_str(" ⊢ ");
        print_expr(out, precondition, 0);
    }
    out.push_str(" ■ {");
    for (index, alternative) in control.contract.iter().enumerate() {
        out.push_str(if index == 0 { " " } else { " │ " });
        let _ = write!(out, "{} {{", alternative.name);
        print_parameters(out, &alternative.fields);
        out.push('}');
        if let Some(condition) = &alternative.condition {
            out.push_str(" ⇐ ");
            print_expr(out, condition, 0);
        }
    }
    out.push_str(" }");
    if !control.uses.is_empty() {
        let _ = write!(out, " uses {}", control.uses.join(", "));
    }
}

impl Program {
    /// The program in the glyph form of the core text.
    pub fn print(&self) -> String {
        let mut out = String::new();
        for (index, declaration) in self.declarations.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            match declaration {
                Declaration::Function(function) => {
                    let _ = write!(out, "function {}(", function.name);
                    print_parameters(&mut out, &function.parameters);
                    out.push(')');
                    if let Some(precondition) = &function.precondition {
                        out.push_str(" ⊢ ");
                        print_expr(&mut out, precondition, 0);
                    }
                    let _ = write!(out, " : {} := ", function.result);
                    print_expr(&mut out, &function.body, 0);
                    out.push('\n');
                }
                Declaration::Control(control) => {
                    if control.public {
                        out.push_str("pub ");
                    }
                    if control.external {
                        out.push_str("extern ");
                    }
                    let _ = write!(out, "control {}(", control.name);
                    print_parameters(&mut out, &control.parameters);
                    out.push(')');
                    print_signature(&mut out, control);
                    out.push('\n');
                    print_statements(&mut out, &control.body, 0);
                }
            }
        }
        out
    }
}
