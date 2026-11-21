//! The typed core.
//!
//! `resolve` produces this form from the syntax tree. Every name is an index,
//! every expression has its type, and a type is never text. The oracle runs
//! this form, and the checker of phase 3 judges it.

use crate::int::Int;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum IntType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Usize,
}

impl IntType {
    pub const ALL: [IntType; 9] = [
        IntType::U8,
        IntType::U16,
        IntType::U32,
        IntType::U64,
        IntType::I8,
        IntType::I16,
        IntType::I32,
        IntType::I64,
        IntType::Usize,
    ];

    pub fn name(self) -> &'static str {
        match self {
            IntType::U8 => "u8",
            IntType::U16 => "u16",
            IntType::U32 => "u32",
            IntType::U64 => "u64",
            IntType::I8 => "i8",
            IntType::I16 => "i16",
            IntType::I32 => "i32",
            IntType::I64 => "i64",
            IntType::Usize => "usize",
        }
    }

    pub fn from_name(name: &str) -> Option<IntType> {
        IntType::ALL.into_iter().find(|ty| ty.name() == name)
    }

    /// The least and the greatest value of the type. The greatest `usize` is
    /// a parameter of the run: the fact logic bounds it and does not fix it
    /// ([A2]).
    pub fn bounds(self, usize_max: &Int) -> (Int, Int) {
        let unsigned = |bits: u32| {
            (
                Int::zero(),
                Int::power_of_two(bits).minus(&Int::from_u64(1)),
            )
        };
        let signed = |bits: u32| {
            let half = Int::power_of_two(bits - 1);
            (half.negate(), half.minus(&Int::from_u64(1)))
        };
        match self {
            IntType::U8 => unsigned(8),
            IntType::U16 => unsigned(16),
            IntType::U32 => unsigned(32),
            IntType::U64 => unsigned(64),
            IntType::I8 => signed(8),
            IntType::I16 => signed(16),
            IntType::I32 => signed(32),
            IntType::I64 => signed(64),
            IntType::Usize => (Int::zero(), usize_max.clone()),
        }
    }

    /// The bounds that do not depend on the target, for literals: a `usize`
    /// literal must fit the narrowest address width.
    pub fn literal_bounds(self) -> (Int, Int) {
        self.bounds(&Int::power_of_two(32).minus(&Int::from_u64(1)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Type {
    Bool,
    Int(IntType),
}

impl Type {
    pub fn from_name(name: &str) -> Option<Type> {
        if name == "bool" {
            return Some(Type::Bool);
        }
        IntType::from_name(name).map(Type::Int)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Type::Bool => "bool",
            Type::Int(ty) => ty.name(),
        })
    }
}

pub type FunctionId = usize;
pub type ControlId = usize;
pub type VarId = usize;
pub type SiteId = usize;
pub type BodyId = usize;

/// What a name in an expression stands for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarRef {
    /// A name of a control body: a parameter, a local, a loop parameter or a
    /// payload name.
    Local(VarId),
    /// A parameter, in a precondition, a contract condition or a function.
    Parameter(usize),
    /// A payload field, in the condition of its alternative.
    Field(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arithmetic {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Boolean(bool),
    Integer(Int, IntType),
    Var(VarRef, Type),
    Negate(Box<Expr>, IntType),
    Not(Box<Expr>),
    Arithmetic(Arithmetic, Box<Expr>, Box<Expr>, IntType),
    Compare(Comparison, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Call(FunctionId, Vec<Expr>, Type),
}

impl Expr {
    pub fn ty(&self) -> Type {
        match self {
            Expr::Boolean(_)
            | Expr::Not(_)
            | Expr::Compare(_, _, _)
            | Expr::And(_, _)
            | Expr::Or(_, _) => Type::Bool,
            Expr::Integer(_, ty) | Expr::Negate(_, ty) | Expr::Arithmetic(_, _, _, ty) => {
                Type::Int(*ty)
            }
            Expr::Var(_, ty) | Expr::Call(_, _, ty) => *ty,
        }
    }
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
    /// A formula over the parameters.
    pub precondition: Option<Expr>,
    pub result: Type,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct Alternative {
    pub name: String,
    pub fields: Vec<Parameter>,
    /// A formula over the parameters and the fields.
    pub condition: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Control {
    pub name: String,
    pub public: bool,
    pub parameters: Vec<Parameter>,
    /// A formula over the parameters.
    pub precondition: Option<Expr>,
    pub contract: Vec<Alternative>,
    pub uses: Vec<String>,
    /// `None` for an extern control.
    pub body: Option<ControlBody>,
}

impl Control {
    /// Whether the operation is a function of its arguments ([I10]). In this
    /// slice that is every control without an effect.
    pub fn is_pure(&self) -> bool {
        self.uses.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Var {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    /// The body the name belongs to ([W4]).
    pub body: BodyId,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub name: String,
    pub alternatives: Vec<String>,
    /// The body whose statement the site is.
    pub body: BodyId,
}

#[derive(Clone, Debug)]
pub struct LoopHead {
    pub name: String,
    pub site: SiteId,
    pub parameters: Vec<VarId>,
    /// A formula over the loop's parameters and the names around the loop.
    pub invariant: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Body {
    pub statements: Vec<Statement>,
    pub parent: Option<BodyId>,
    /// `None` for the body of the control.
    pub head: Option<LoopHead>,
}

/// The bodies of one control. `bodies[0]` is the control's own; the others
/// are loop bodies. The parameters are the first names.
#[derive(Clone, Debug)]
pub struct ControlBody {
    pub vars: Vec<Var>,
    pub sites: Vec<Site>,
    pub bodies: Vec<Body>,
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
    /// A site and the index of one of its alternatives.
    Atom(SiteId, usize),
    Not(Box<Selector>),
    And(Box<Selector>, Box<Selector>),
    Or(Box<Selector>, Box<Selector>),
}

/// One listed alternative of a decision: the index of the alternative in the
/// operation's contract, and the names its payload is bound to. The position
/// of the arm is the index of the alternative at the site.
#[derive(Clone, Debug)]
pub struct Arm {
    pub alternative: usize,
    pub vars: Vec<VarId>,
}

#[derive(Clone, Debug)]
pub enum Command {
    Bind {
        var: VarId,
        value: Expr,
    },
    Assign {
        var: VarId,
        value: Expr,
    },
    Oblige(Expr),
    /// The site has the alternatives `true` (0) and `false` (1).
    Test {
        site: SiteId,
        condition: Expr,
    },
    Decide {
        site: SiteId,
        operation: ControlId,
        arguments: Vec<Expr>,
        arms: Vec<Arm>,
    },
    /// `arms[i]` holds the payload names of the site's alternative `i`.
    Loop {
        site: SiteId,
        body: BodyId,
        initial: Vec<Expr>,
        arms: Vec<Vec<VarId>>,
    },
    Again {
        body: BodyId,
        arguments: Vec<Expr>,
    },
    /// The index is that of the alternative at the innermost loop's site.
    Leave {
        alternative: usize,
        payload: Vec<Expr>,
    },
    /// The index is that of the alternative in the control's contract.
    Exit {
        alternative: usize,
        payload: Vec<Expr>,
    },
    Impossible,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub functions: Vec<Function>,
    pub controls: Vec<Control>,
}

impl Program {
    pub fn control(&self, name: &str) -> Option<ControlId> {
        self.controls
            .iter()
            .position(|control| control.name == name)
    }
}
