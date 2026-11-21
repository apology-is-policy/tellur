//! The core language as it is written: the syntax tree of the core text form
//! and its printer.
//!
//! This is the grammar of section 3 of `docs/v2/core.md`, covering slices 1
//! and 2. Names are still text here; `resolve` turns the tree into the typed
//! core.

use crate::int::Int;
use crate::typed::{BorrowMode, Type};
use std::fmt::Write;

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug)]
pub enum Declaration {
    Struct(StructDecl),
    Variant(VariantDecl),
    Outcome(OutcomeDecl),
    Resource(ResourceDecl),
    Boundary(BoundaryDecl),
    Function(Function),
    Control(Control),
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub name: String,
    pub public: bool,
    pub is_copy: bool,
    pub fields: Vec<Parameter>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct VariantConstructor {
    pub name: String,
    pub fields: Vec<Parameter>,
}

#[derive(Clone, Debug)]
pub struct VariantDecl {
    pub name: String,
    pub public: bool,
    pub is_copy: bool,
    pub constructors: Vec<VariantConstructor>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct OutcomeDecl {
    pub name: String,
    pub public: bool,
    pub alternatives: Vec<Alternative>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct ResourceDecl {
    pub name: String,
    pub on_fault_release: bool,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct BoundaryDecl {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub retains: Vec<String>,
    pub body: Vec<Statement>,
    pub message: String,
    pub contingency: Vec<Statement>,
    pub line: u32,
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
    pub contingent: bool,
    pub faults: bool,
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

/// A place `p ::= x | p.x | p[e]`.
#[derive(Clone, Debug)]
pub struct Place {
    pub root: String,
    pub steps: Vec<PlaceStep>,
}

#[derive(Clone, Debug)]
pub enum PlaceStep {
    Field(String),
    Index(Expr),
}

#[derive(Clone, Debug)]
pub enum Command {
    Bind {
        mutable: bool,
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    /// `S {x̄} := e` ([E-Unpack]).
    Unpack {
        struct_name: String,
        names: Vec<String>,
        value: Expr,
    },
    /// `[~]x [: τ] := capture ω(ō)` ([E-Capture]).
    Capture {
        mutable: bool,
        name: String,
        ty: Option<Type>,
        operation: String,
        arguments: Vec<Expr>,
    },
    Assign {
        name: String,
        value: Expr,
    },
    /// `p ← e` where `p` has field or index steps ([E-Assign]).
    AssignPlace {
        target: Place,
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
    /// `d : boundary {B₁} contingency [x̄] [(m : string)] {B₂}` ([E-Boundary]).
    Boundary {
        site: String,
        body: Vec<Statement>,
        captures: Vec<String>,
        message: Option<String>,
        contingency: Vec<Statement>,
    },
    /// `supervise b(ō)` ([E-Supervise]).
    Supervise {
        boundary: String,
        arguments: Vec<Expr>,
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
    /// `fault e` or `assume ⊥ "reason"` ([E-Fault], [F4]).
    Fault {
        message: Expr,
        assumed: bool,
    },
    Impossible,
    Terminate(Option<Expr>),
    Escalate,
    Recover {
        loop_name: String,
    },
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
    String(String),
    Name(String),
    /// `old(name)` in a contract postcondition ([W7]).
    Old(String),
    /// `▾e` move mark ([U11], [G7a]).
    Move(Box<Expr>),
    /// `&p` or `&~p` borrow operand ([E-Borrow], [U11]).
    Borrow(BorrowMode, Box<Expr>),
    /// `e.field` ([E-Field]).
    Field(Box<Expr>, String),
    /// `p[index]` ([E-Index]).
    Index(Box<Expr>, Box<Expr>),
    /// `S {ē}` ([E-Build]).
    Struct(String, Vec<Expr>),
    /// `V.c {ē}` ([E-Build]).
    Variant(String, String, Vec<Expr>),
    /// `[ē]` ([E-Build]).
    Vector(Vec<Expr>),
    /// `∅` ([E-Build]).
    EmptyMap,
    /// `len(p)` ([A3]).
    Len(Box<Expr>),
    /// `clone(p)` ([E-Build]).
    Clone(Box<Expr>),
    Unary(Unary, Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

// Printing. The output is the glyph form, and reading it back gives the same
// tree.

const UNARY_LEVEL: u8 = 6;
const POSTFIX_LEVEL: u8 = 7;

fn expr_level(expr: &Expr) -> u8 {
    match expr {
        Expr::Binary(operator, _, _) => operator.level(),
        Expr::Unary(_, _) | Expr::Move(_) | Expr::Borrow(_, _) => UNARY_LEVEL,
        Expr::Integer(value) if value.is_negative() => UNARY_LEVEL,
        Expr::Field(_, _) | Expr::Index(_, _) => POSTFIX_LEVEL,
        _ => 8,
    }
}

pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn print_place(out: &mut String, place: &Place) {
    out.push_str(&place.root);
    for step in &place.steps {
        match step {
            PlaceStep::Field(field) => {
                out.push('.');
                out.push_str(field);
            }
            PlaceStep::Index(idx) => {
                out.push('[');
                print_expr(out, idx, 0);
                out.push(']');
            }
        }
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
        Expr::String(value) => {
            let _ = write!(out, "\"{}\"", escape_string(value));
        }
        Expr::Name(name) => out.push_str(name),
        Expr::Old(name) => {
            let _ = write!(out, "old({name})");
        }
        Expr::Move(inner) => {
            out.push('▾');
            print_expr(out, inner, UNARY_LEVEL);
        }
        Expr::Borrow(mode, inner) => {
            out.push_str(match mode {
                BorrowMode::Shared => "&",
                BorrowMode::Exclusive => "&~",
            });
            print_expr(out, inner, UNARY_LEVEL);
        }
        Expr::Field(base, field) => {
            print_expr(out, base, POSTFIX_LEVEL);
            out.push('.');
            out.push_str(field);
        }
        Expr::Index(base, index) => {
            print_expr(out, base, POSTFIX_LEVEL);
            out.push('[');
            print_expr(out, index, 0);
            out.push(']');
        }
        Expr::Struct(name, fields) => {
            let _ = write!(out, "{name} {{");
            print_list(out, fields);
            out.push('}');
        }
        Expr::Variant(ty_name, ctor_name, fields) => {
            let _ = write!(out, "{ty_name}.{ctor_name} {{");
            print_list(out, fields);
            out.push('}');
        }
        Expr::Vector(items) => {
            out.push('[');
            print_list(out, items);
            out.push(']');
        }
        Expr::EmptyMap => out.push('∅'),
        Expr::Len(place) => {
            out.push_str("len(");
            print_expr(out, place, 0);
            out.push(')');
        }
        Expr::Clone(place) => {
            out.push_str("clone(");
            print_expr(out, place, 0);
            out.push(')');
        }
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
            Command::Unpack {
                struct_name,
                names,
                value,
            } => {
                let _ = write!(out, "{struct_name} {{{}}} := ", names.join(", "));
                print_expr(out, value, 0);
            }
            Command::Capture {
                mutable,
                name,
                ty,
                operation,
                arguments,
            } => {
                if *mutable {
                    out.push('~');
                }
                out.push_str(name);
                if let Some(ty) = ty {
                    let _ = write!(out, " : {ty}");
                }
                let _ = write!(out, " := capture {operation}(");
                print_list(out, arguments);
                out.push(')');
            }
            Command::Assign { name, value } => {
                let _ = write!(out, "{name} ← ");
                print_expr(out, value, 0);
            }
            Command::AssignPlace { target, value } => {
                print_place(out, target);
                out.push_str(" ← ");
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
            Command::Boundary {
                site,
                body,
                captures,
                message,
                contingency,
            } => {
                let _ = writeln!(out, "{site} : boundary {{");
                print_statements(out, body, indent + 4);
                let _ = write!(out, "{pad}}} contingency");
                if !captures.is_empty() {
                    let _ = write!(out, " [{}]", captures.join(", "));
                }
                if let Some(msg) = message {
                    let _ = write!(out, " ({msg} : string)");
                }
                out.push_str(" {\n");
                print_statements(out, contingency, indent + 4);
                let _ = write!(out, "{pad}}}");
            }
            Command::Supervise {
                boundary,
                arguments,
            } => {
                let _ = write!(out, "supervise {boundary}(");
                print_list(out, arguments);
                out.push(')');
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
            Command::Fault { message, assumed } => {
                if *assumed {
                    out.push_str("assume ⊥ ");
                } else {
                    out.push_str("fault ");
                }
                print_expr(out, message, 0);
            }
            Command::Impossible => out.push('⊥'),
            Command::Terminate(status) => {
                out.push_str("terminate");
                if let Some(expr) = status {
                    out.push(' ');
                    print_expr(out, expr, 0);
                }
            }
            Command::Escalate => out.push_str("escalate"),
            Command::Recover { loop_name } => {
                let _ = write!(out, "recover ↑ {loop_name}");
            }
        }
        out.push('\n');
    }
}

fn print_contract(out: &mut String, contract: &[Alternative]) {
    out.push('{');
    for (index, alternative) in contract.iter().enumerate() {
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
}

fn print_signature(out: &mut String, control: &Control) {
    if let Some(precondition) = &control.precondition {
        out.push_str(" ⊢ ");
        print_expr(out, precondition, 0);
    }
    out.push_str(" ■ ");
    print_contract(out, &control.contract);
    if control.faults {
        out.push_str(" faults");
    }
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
                Declaration::Struct(st) => {
                    if st.public {
                        out.push_str("pub ");
                    }
                    if st.is_copy {
                        out.push_str("copy ");
                    }
                    let _ = write!(out, "struct {} {{", st.name);
                    print_parameters(&mut out, &st.fields);
                    out.push_str("}\n");
                }
                Declaration::Variant(var) => {
                    if var.public {
                        out.push_str("pub ");
                    }
                    if var.is_copy {
                        out.push_str("copy ");
                    }
                    let _ = write!(out, "variant {} {{", var.name);
                    for (i, ctor) in var.constructors.iter().enumerate() {
                        out.push_str(if i == 0 { " " } else { " │ " });
                        let _ = write!(out, "{} {{", ctor.name);
                        print_parameters(&mut out, &ctor.fields);
                        out.push('}');
                    }
                    out.push_str(" }\n");
                }
                Declaration::Outcome(oc) => {
                    if oc.public {
                        out.push_str("pub ");
                    }
                    let _ = write!(out, "outcome {} ", oc.name);
                    print_contract(&mut out, &oc.alternatives);
                    out.push('\n');
                }
                Declaration::Resource(res) => {
                    let _ = write!(out, "extern resource {}", res.name);
                    if res.on_fault_release {
                        out.push_str(" on fault release");
                    }
                    out.push('\n');
                }
                Declaration::Boundary(b) => {
                    let _ = write!(out, "boundary {}(", b.name);
                    print_parameters(&mut out, &b.parameters);
                    out.push(')');
                    if !b.retains.is_empty() {
                        let _ = write!(out, " retains {}", b.retains.join(", "));
                    }
                    out.push_str(" {\n");
                    print_statements(&mut out, &b.body, 4);
                    let _ = writeln!(out, "}} contingency({} : string) {{", b.message);
                    print_statements(&mut out, &b.contingency, 4);
                    out.push_str("}\n");
                }
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
                    if control.contingent {
                        out.push_str("contingent ");
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
