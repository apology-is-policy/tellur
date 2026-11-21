//! The typed core.
//!
//! `resolve` produces this form from the syntax tree. Every name is an index,
//! every expression has its type, and a type is never raw text once resolved.
//! The oracle runs this form, and the checker of phase 3 judges it.

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

    pub fn is_signed(self) -> bool {
        matches!(
            self,
            IntType::I8 | IntType::I16 | IntType::I32 | IntType::I64
        )
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

pub type FunctionId = usize;
pub type ControlId = usize;
pub type VarId = usize;
pub type SiteId = usize;
pub type BodyId = usize;
pub type StructId = usize;
pub type VariantId = usize;
pub type ResourceId = usize;
pub type OutcomeId = usize;
pub type BoundaryId = usize;

/// Usage kind of a type (`docs/v2/core.md` section 4.1, `copy < drop < must`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Kind {
    Copy,
    Drop,
    Must,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Copy => "copy",
            Kind::Drop => "drop",
            Kind::Must => "must",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum BorrowMode {
    Shared,
    Exclusive,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Type {
    Bool,
    Int(IntType),
    String,
    /// Unresolved named type in `syntax::Program` before `resolve::resolve`.
    Named(String),
    /// Unresolved `outcome⟨name⟩` in `syntax::Program` before `resolve::resolve`.
    OutcomeNamed(String),
    Struct(StructId, String),
    Variant(VariantId, String),
    Resource(ResourceId, String),
    Vector(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Outcome(OutcomeId, String),
    Ref(BorrowMode, Box<Type>),
}

impl Type {
    pub fn from_name(name: &str) -> Option<Type> {
        if name == "bool" {
            return Some(Type::Bool);
        }
        if name == "string" {
            return Some(Type::String);
        }
        IntType::from_name(name).map(Type::Int)
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self, Type::Bool | Type::Int(_))
    }

    /// Unwraps a reference type `&τ` or `&~τ` to `τ` ([W8]).
    pub fn deref_type(&self) -> &Type {
        match self {
            Type::Ref(_, inner) => inner,
            other => other,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => formatter.write_str("bool"),
            Type::Int(ty) => formatter.write_str(ty.name()),
            Type::String => formatter.write_str("string"),
            Type::Named(name)
            | Type::Struct(_, name)
            | Type::Variant(_, name)
            | Type::Resource(_, name) => formatter.write_str(name),
            Type::OutcomeNamed(name) | Type::Outcome(_, name) => {
                write!(formatter, "outcome⟨{name}⟩")
            }
            Type::Vector(elem) => write!(formatter, "[{elem}]"),
            Type::Map(k, v) => write!(formatter, "{k} ⇀ {v}"),
            Type::Ref(BorrowMode::Shared, inner) => write!(formatter, "&{inner}"),
            Type::Ref(BorrowMode::Exclusive, inner) => write!(formatter, "&~{inner}"),
        }
    }
}

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

/// A place `p ::= x | p.x | p[e]` (`docs/v2/core.md` section 3).
#[derive(Clone, Debug)]
pub struct Place {
    pub root: VarId,
    pub root_ty: Type,
    pub steps: Vec<PlaceStep>,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub enum PlaceStep {
    Field(StructId, usize, Type),
    Index(Expr, Type),
}

#[derive(Clone, Debug)]
pub enum Expr {
    Boolean(bool),
    Integer(Int, IntType),
    String(String),
    Var(VarRef, Type),
    /// Value of an `&~` parameter at entry in a contract condition (`old(y)`, [W7]).
    Old(usize, Type),
    /// Explicit move mark `▾p` ([U11], [G7a]).
    Move(Box<Expr>, Type),
    /// Explicit borrow operand `&p` or `&~p` ([E-Borrow], [U11]).
    Borrow(BorrowMode, Place, Type),
    Field(Box<Expr>, StructId, usize, Type),
    Index(Box<Expr>, Box<Expr>, Type),
    Struct(StructId, Vec<Expr>, Type),
    Variant(VariantId, usize, Vec<Expr>, Type),
    Vector(Vec<Expr>, Type),
    EmptyMap(Type),
    Len(Box<Expr>),
    Clone(Box<Expr>, Type),
    Trim(Box<Expr>),
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
            Expr::String(_) | Expr::Trim(_) => Type::String,
            Expr::Len(_) => Type::Int(IntType::Usize),
            Expr::Var(_, ty)
            | Expr::Old(_, ty)
            | Expr::Move(_, ty)
            | Expr::Borrow(_, _, ty)
            | Expr::Field(_, _, _, ty)
            | Expr::Index(_, _, ty)
            | Expr::Struct(_, _, ty)
            | Expr::Variant(_, _, _, ty)
            | Expr::Vector(_, ty)
            | Expr::EmptyMap(ty)
            | Expr::Clone(_, ty)
            | Expr::Call(_, _, ty) => ty.clone(),
        }
    }

    /// Whether this operand lends (`&p` or `&~p`) for `[E-Operands]`.
    pub fn lends(&self) -> bool {
        matches!(self, Expr::Borrow(_, _, _))
    }
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub public: bool,
    pub is_copy: bool,
    pub kind: Kind,
    pub fault_releasable: bool,
    pub holds_obligation: bool,
    pub clonable: bool,
    pub comparable: bool,
    pub fields: Vec<Parameter>,
}

#[derive(Clone, Debug)]
pub struct VariantConstructor {
    pub name: String,
    pub fields: Vec<Parameter>,
}

#[derive(Clone, Debug)]
pub struct VariantDef {
    pub name: String,
    pub public: bool,
    pub is_copy: bool,
    pub kind: Kind,
    pub fault_releasable: bool,
    pub holds_obligation: bool,
    pub clonable: bool,
    pub comparable: bool,
    pub constructors: Vec<VariantConstructor>,
}

#[derive(Clone, Debug)]
pub struct ResourceDef {
    pub name: String,
    pub on_fault_release: bool,
}

#[derive(Clone, Debug)]
pub struct OutcomeDef {
    pub name: String,
    pub public: bool,
    pub fault_releasable: bool,
    pub alternatives: Vec<Alternative>,
}

#[derive(Clone, Debug)]
pub struct BoundaryDef {
    pub name: String,
    pub parameters: Vec<Parameter>,
    /// Indices into `parameters` that are retained for `contingency` ([F6]).
    pub retains: Vec<usize>,
    pub body: ControlBody,
    pub message: String,
    pub contingency: ControlBody,
    pub may_fault: bool,
    pub uses: Vec<String>,
    pub line: u32,
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
    pub contingent: bool,
    pub faults: bool,
    pub parameters: Vec<Parameter>,
    /// A formula over the parameters.
    pub precondition: Option<Expr>,
    pub contract: Vec<Alternative>,
    pub uses: Vec<String>,
    /// `None` for an extern control.
    pub body: Option<ControlBody>,
}

impl Control {
    /// Whether the operation is a function of its arguments ([I10]): no
    /// effects and no resource types in parameters or payloads.
    pub fn is_pure(&self) -> bool {
        if !self.uses.is_empty() {
            return false;
        }
        let has_resource = |ty: &Type| {
            fn check(t: &Type) -> bool {
                match t {
                    Type::Resource(_, _) => true,
                    Type::Ref(_, inner) | Type::Vector(inner) => check(inner),
                    Type::Map(k, v) => check(k) || check(v),
                    _ => false,
                }
            }
            check(ty)
        };
        if self.parameters.iter().any(|p| has_resource(&p.ty)) {
            return false;
        }
        for alt in &self.contract {
            if alt.fields.iter().any(|f| has_resource(&f.ty)) {
                return false;
            }
        }
        true
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
    /// `Some` for a loop body, `None` for the control's body or a boundary body.
    pub head: Option<LoopHead>,
}

/// The bodies of one control (or named boundary). `bodies[0]` is the root
/// body; the others are loop bodies or lexical boundary/contingency bodies.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    Top,
    /// A site and the index of one of its alternatives.
    Atom(SiteId, usize),
    Not(Box<Selector>),
    And(Box<Selector>, Box<Selector>),
    Or(Box<Selector>, Box<Selector>),
}

/// One listed alternative of a decision: the index of the alternative in the
/// operation's contract, and the names its payload is bound to.
#[derive(Clone, Debug)]
pub struct Arm {
    pub alternative: usize,
    pub vars: Vec<VarId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinOp {
    Push,
    Pop,
    Take,
    Get,
    Insert,
    Remove,
    EntryAt,
    Emit,
}

impl BuiltinOp {
    pub fn name(self) -> &'static str {
        match self {
            BuiltinOp::Push => "push",
            BuiltinOp::Pop => "pop",
            BuiltinOp::Take => "take",
            BuiltinOp::Get => "get",
            BuiltinOp::Insert => "insert",
            BuiltinOp::Remove => "remove",
            BuiltinOp::EntryAt => "entry_at",
            BuiltinOp::Emit => "emit",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Operation {
    Control(ControlId),
    Builtin {
        op: BuiltinOp,
        /// For `Push`, `Pop`, `Insert`, `Remove`: the mutable target place.
        target: Option<Place>,
        contract: Vec<Alternative>,
    },
    /// Decides a captured outcome value (`Δ outcome(x)`).
    Outcome(OutcomeId),
    /// Decides a variant value (`Δ V(e)`).
    Variant {
        id: VariantId,
        by_value: bool,
    },
}

impl Operation {
    pub fn name<'a>(&'a self, program: &'a Program) -> &'a str {
        match self {
            Operation::Control(id) => &program.controls[*id].name,
            Operation::Builtin { op, .. } => op.name(),
            Operation::Outcome(_) => "outcome",
            Operation::Variant { id, .. } => &program.variants[*id].name,
        }
    }

    pub fn contract<'a>(&'a self, program: &'a Program) -> Vec<Alternative> {
        match self {
            Operation::Control(id) => program.controls[*id].contract.clone(),
            Operation::Builtin { contract, .. } => contract.clone(),
            Operation::Outcome(id) => program.outcomes[*id].alternatives.clone(),
            Operation::Variant { id, .. } => program.variants[*id]
                .constructors
                .iter()
                .map(|c| Alternative {
                    name: c.name.clone(),
                    fields: c.fields.clone(),
                    condition: None,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    Bind {
        var: VarId,
        value: Expr,
    },
    /// `S {x̄} := e` ([E-Unpack]).
    Unpack {
        struct_id: StructId,
        vars: Vec<VarId>,
        value: Expr,
    },
    /// `[~]x := capture ω(ō)` ([E-Capture], [C4]).
    Capture {
        var: VarId,
        outcome_id: OutcomeId,
        operation: Operation,
        arguments: Vec<Expr>,
    },
    Assign {
        var: VarId,
        value: Expr,
    },
    /// `p ← e` where `p` has field or index steps ([E-Assign], [I9]).
    AssignPlace {
        target: Place,
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
        op_kind: Operation,
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
    /// Lexical fault boundary `d : boundary {B₁} contingency [x̄] (m) {B₂}` ([E-Boundary], [F5]).
    Boundary {
        site: SiteId,
        body: BodyId,
        /// Variables in the enclosing scope moved into the boundary's care.
        captures: Vec<VarId>,
        /// Corresponding variables bound in `contingency`.
        contingency_vars: Vec<VarId>,
        /// Optional fault message variable bound in `contingency`.
        message_var: Option<VarId>,
        contingency: BodyId,
    },
    /// `supervise b(ō)` ([E-Supervise], [F6]).
    Supervise {
        boundary: BoundaryId,
        arguments: Vec<Expr>,
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
    /// `fault e` or `assume ⊥ "reason"` ([E-Fault], [F1], [F2], [F4]).
    Fault {
        message: Expr,
        assumed: bool,
    },
    Impossible,
    /// `terminate` in a boundary ([E-Contingency], [E-Supervise], [F5], [F6]).
    Terminate(Option<Expr>),
    /// `escalate` in a boundary contingency ([E-Contingency], [E-Supervise], [F5], [F6]).
    Escalate,
    /// `recover ↑ ℓ` in a lexical boundary contingency ([E-Contingency], [F5]).
    Recover {
        body: BodyId,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub structs: Vec<StructDef>,
    pub variants: Vec<VariantDef>,
    pub resources: Vec<ResourceDef>,
    pub outcomes: Vec<OutcomeDef>,
    pub boundaries: Vec<BoundaryDef>,
    pub functions: Vec<Function>,
    pub controls: Vec<Control>,
}

impl Program {
    pub fn control(&self, name: &str) -> Option<ControlId> {
        self.controls
            .iter()
            .position(|control| control.name == name)
    }

    pub fn boundary(&self, name: &str) -> Option<BoundaryId> {
        self.boundaries
            .iter()
            .position(|boundary| boundary.name == name)
    }

    /// Usage kind of `ty` (`copy < drop < must`, [T1]).
    pub fn kind(&self, ty: &Type) -> Kind {
        match ty {
            Type::Bool | Type::Int(_) | Type::Ref(BorrowMode::Shared, _) => Kind::Copy,
            Type::String | Type::Ref(BorrowMode::Exclusive, _) => Kind::Drop,
            Type::Struct(id, _) => self.structs[*id].kind,
            Type::Variant(id, _) => self.variants[*id].kind,
            Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => Kind::Must,
            Type::Vector(elem) => Kind::Drop.max(self.kind(elem)),
            Type::Map(k, v) => Kind::Drop.max(self.kind(k)).max(self.kind(v)),
            Type::Named(_) => Kind::Drop,
        }
    }

    /// Whether `ty` is fault-releasable ([T3]).
    pub fn is_fault_releasable(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool | Type::Int(_) | Type::String | Type::Ref(_, _) => true,
            Type::Struct(id, _) => self.structs[*id].fault_releasable,
            Type::Variant(id, _) => self.variants[*id].fault_releasable,
            Type::Resource(id, _) => self.resources[*id].on_fault_release,
            Type::Outcome(id, _) => self.outcomes[*id].fault_releasable,
            Type::Vector(elem) => self.is_fault_releasable(elem),
            Type::Map(k, v) => self.is_fault_releasable(k) && self.is_fault_releasable(v),
            Type::Named(_) | Type::OutcomeNamed(_) => false,
        }
    }

    /// Whether `ty` holds an obligation ([U11]): a resource, a captured outcome,
    /// or an aggregate with a component that does.
    pub fn holds_obligation(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool | Type::Int(_) | Type::String | Type::Ref(_, _) => false,
            Type::Struct(id, _) => self.structs[*id].holds_obligation,
            Type::Variant(id, _) => self.variants[*id].holds_obligation,
            Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => true,
            Type::Vector(elem) => self.holds_obligation(elem),
            Type::Map(k, v) => self.holds_obligation(k) || self.holds_obligation(v),
            Type::Named(_) => false,
        }
    }

    /// Whether `ty` can be cloned by `clone(p)` ([E-Build]).
    pub fn is_clonable(&self, ty: &Type) -> bool {
        let base = ty.deref_type();
        match base {
            Type::Bool | Type::Int(_) | Type::String => true,
            Type::Struct(id, _) => self.structs[*id].clonable,
            Type::Variant(id, _) => self.variants[*id].clonable,
            Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => false,
            Type::Vector(elem) => self.is_clonable(elem),
            Type::Map(k, v) => self.is_clonable(k) && self.is_clonable(v),
            Type::Ref(_, _) | Type::Named(_) => false,
        }
    }

    /// Whether `ty` supports `=` and `≠` ([I6]).
    pub fn is_comparable(&self, ty: &Type) -> bool {
        let base = ty.deref_type();
        match base {
            Type::Bool | Type::Int(_) | Type::String => true,
            Type::Struct(id, _) => self.structs[*id].comparable,
            Type::Variant(id, _) => self.variants[*id].comparable,
            _ => false,
        }
    }
}
