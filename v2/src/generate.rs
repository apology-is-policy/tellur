//! Random core programs for slice 1, and a count of the constructs they reach.
//!
//! Every generated program is built as a [`syntax::Program`] so that it can be
//! printed as core text, read back by [`crate::text::parse`], and resolved by
//! [`crate::resolve::resolve`].
//!
//! Two generators are provided:
//!
//! - [`valid`] produces programs that are well-typed and never stuck in the
//!   oracle on any valid execution input, under both 32-bit and 64-bit
//!   `usize`.
//! - [`free`] produces arbitrary well-resolved programs whose expressions,
//!   selectors, obligations, effects and decision arms are not constrained to
//!   avoid runtime stuck states.

use crate::host::draw_value;
use crate::int::Int;
use crate::oracle::{Config, Value, signature_holds};
use crate::rng::Rng;
use crate::syntax::{
    Alternative, Arm, Binary, Command, Control, Declaration, Expr, Function, LoopParameter,
    Parameter, Program, Selector, Statement, Unary,
};
use crate::typed::{self, IntType, Type};
use std::collections::BTreeMap;

/// Every syntactic construct of slice 1.
pub const CONSTRUCTS: [&str; 61] = [
    "function",
    "function-where",
    "control",
    "pub-control",
    "extern-control",
    "control-where",
    "alt-unit",
    "alt-payload",
    "alt-when",
    "uses",
    "type-bool",
    "type-u8",
    "type-u16",
    "type-u32",
    "type-u64",
    "type-i8",
    "type-i16",
    "type-i32",
    "type-i64",
    "type-usize",
    "selector-top",
    "selector-atom",
    "selector-not",
    "selector-and",
    "selector-or",
    "bind",
    "bind-mutable",
    "bind-typed",
    "bind-inferred",
    "assign",
    "oblige",
    "test",
    "decide",
    "loop",
    "loop-invariant",
    "loop-mutable-param",
    "again",
    "leave",
    "leave-payload",
    "exit",
    "exit-payload",
    "impossible",
    "expr-int",
    "expr-bool",
    "expr-name",
    "expr-call",
    "unary-negate",
    "unary-not",
    "binary-or",
    "binary-and",
    "binary-eq",
    "binary-ne",
    "binary-lt",
    "binary-le",
    "binary-gt",
    "binary-ge",
    "binary-add",
    "binary-sub",
    "binary-mul",
    "binary-div",
    "binary-mod",
];

const TYPES: [Type; 10] = [
    Type::Bool,
    Type::Int(IntType::U8),
    Type::Int(IntType::U16),
    Type::Int(IntType::U32),
    Type::Int(IntType::U64),
    Type::Int(IntType::I8),
    Type::Int(IntType::I16),
    Type::Int(IntType::I32),
    Type::Int(IntType::I64),
    Type::Int(IntType::Usize),
];

const SIGNED_TYPES: [IntType; 4] = [IntType::I8, IntType::I16, IntType::I32, IntType::I64];

const COMPARISONS: [Binary; 6] = [
    Binary::Equal,
    Binary::NotEqual,
    Binary::Less,
    Binary::LessEqual,
    Binary::Greater,
    Binary::GreaterEqual,
];

const ARITHMETIC_OPS: [Binary; 5] = [
    Binary::Add,
    Binary::Subtract,
    Binary::Multiply,
    Binary::Divide,
    Binary::Remainder,
];

/// Counts how often each construct of [`CONSTRUCTS`] appears in observed
/// programs.
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    pub counts: BTreeMap<&'static str, u64>,
}

impl Coverage {
    pub fn new() -> Coverage {
        Coverage::default()
    }

    fn hit(&mut self, construct: &'static str) {
        *self.counts.entry(construct).or_insert(0) += 1;
    }

    fn observe_type(&mut self, ty: Type) {
        self.hit(match ty {
            Type::Bool => "type-bool",
            Type::Int(IntType::U8) => "type-u8",
            Type::Int(IntType::U16) => "type-u16",
            Type::Int(IntType::U32) => "type-u32",
            Type::Int(IntType::U64) => "type-u64",
            Type::Int(IntType::I8) => "type-i8",
            Type::Int(IntType::I16) => "type-i16",
            Type::Int(IntType::I32) => "type-i32",
            Type::Int(IntType::I64) => "type-i64",
            Type::Int(IntType::Usize) => "type-usize",
        });
    }

    fn observe_selector(&mut self, selector: &Selector) {
        match selector {
            Selector::Top => self.hit("selector-top"),
            Selector::Atom(_, _) => self.hit("selector-atom"),
            Selector::Not(inner) => {
                self.hit("selector-not");
                self.observe_selector(inner);
            }
            Selector::And(left, right) => {
                self.hit("selector-and");
                self.observe_selector(left);
                self.observe_selector(right);
            }
            Selector::Or(left, right) => {
                self.hit("selector-or");
                self.observe_selector(left);
                self.observe_selector(right);
            }
        }
    }

    fn observe_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Integer(_) => self.hit("expr-int"),
            Expr::Boolean(_) => self.hit("expr-bool"),
            Expr::Name(_) => self.hit("expr-name"),
            Expr::Call(_, arguments) => {
                self.hit("expr-call");
                for argument in arguments {
                    self.observe_expr(argument);
                }
            }
            Expr::Unary(operator, operand) => {
                self.hit(match operator {
                    Unary::Negate => "unary-negate",
                    Unary::Not => "unary-not",
                });
                self.observe_expr(operand);
            }
            Expr::Binary(operator, left, right) => {
                self.hit(match operator {
                    Binary::Or => "binary-or",
                    Binary::And => "binary-and",
                    Binary::Equal => "binary-eq",
                    Binary::NotEqual => "binary-ne",
                    Binary::Less => "binary-lt",
                    Binary::LessEqual => "binary-le",
                    Binary::Greater => "binary-gt",
                    Binary::GreaterEqual => "binary-ge",
                    Binary::Add => "binary-add",
                    Binary::Subtract => "binary-sub",
                    Binary::Multiply => "binary-mul",
                    Binary::Divide => "binary-div",
                    Binary::Remainder => "binary-mod",
                });
                self.observe_expr(left);
                self.observe_expr(right);
            }
        }
    }

    fn observe_statements(&mut self, statements: &[Statement]) {
        for statement in statements {
            self.observe_selector(&statement.selector);
            match &statement.command {
                Command::Bind {
                    mutable, ty, value, ..
                } => {
                    self.hit("bind");
                    if *mutable {
                        self.hit("bind-mutable");
                    }
                    if let Some(ty) = ty {
                        self.hit("bind-typed");
                        self.observe_type(*ty);
                    } else {
                        self.hit("bind-inferred");
                    }
                    self.observe_expr(value);
                }
                Command::Assign { value, .. } => {
                    self.hit("assign");
                    self.observe_expr(value);
                }
                Command::Oblige(formula) => {
                    self.hit("oblige");
                    self.observe_expr(formula);
                }
                Command::Test { condition, .. } => {
                    self.hit("test");
                    self.observe_expr(condition);
                }
                Command::Decide { arguments, .. } => {
                    self.hit("decide");
                    for argument in arguments {
                        self.observe_expr(argument);
                    }
                }
                Command::Loop {
                    parameters,
                    invariant,
                    body,
                    ..
                } => {
                    self.hit("loop");
                    for parameter in parameters {
                        if parameter.mutable {
                            self.hit("loop-mutable-param");
                        }
                        self.observe_type(parameter.ty);
                        self.observe_expr(&parameter.initial);
                    }
                    if let Some(invariant) = invariant {
                        self.hit("loop-invariant");
                        self.observe_expr(invariant);
                    }
                    self.observe_statements(body);
                }
                Command::Again { arguments, .. } => {
                    self.hit("again");
                    for argument in arguments {
                        self.observe_expr(argument);
                    }
                }
                Command::Leave { payload, .. } => {
                    self.hit("leave");
                    if !payload.is_empty() {
                        self.hit("leave-payload");
                    }
                    for item in payload {
                        self.observe_expr(item);
                    }
                }
                Command::Exit { payload, .. } => {
                    self.hit("exit");
                    if !payload.is_empty() {
                        self.hit("exit-payload");
                    }
                    for item in payload {
                        self.observe_expr(item);
                    }
                }
                Command::Impossible => self.hit("impossible"),
            }
        }
    }

    /// Records every construct that `program` uses.
    pub fn observe(&mut self, program: &Program) {
        for declaration in &program.declarations {
            match declaration {
                Declaration::Function(function) => {
                    self.hit("function");
                    for parameter in &function.parameters {
                        self.observe_type(parameter.ty);
                    }
                    if let Some(precondition) = &function.precondition {
                        self.hit("function-where");
                        self.observe_expr(precondition);
                    }
                    self.observe_type(function.result);
                    self.observe_expr(&function.body);
                }
                Declaration::Control(control) => {
                    if control.external {
                        self.hit("extern-control");
                    } else {
                        self.hit("control");
                    }
                    if control.public {
                        self.hit("pub-control");
                    }
                    for parameter in &control.parameters {
                        self.observe_type(parameter.ty);
                    }
                    if let Some(precondition) = &control.precondition {
                        self.hit("control-where");
                        self.observe_expr(precondition);
                    }
                    for alternative in &control.contract {
                        if alternative.fields.is_empty() {
                            self.hit("alt-unit");
                        } else {
                            self.hit("alt-payload");
                            for field in &alternative.fields {
                                self.observe_type(field.ty);
                            }
                        }
                        if let Some(condition) = &alternative.condition {
                            self.hit("alt-when");
                            self.observe_expr(condition);
                        }
                    }
                    if !control.uses.is_empty() {
                        self.hit("uses");
                    }
                    self.observe_statements(&control.body);
                }
            }
        }
    }

    /// The constructs of [`CONSTRUCTS`] that have not been seen yet.
    pub fn missing(&self) -> Vec<&'static str> {
        CONSTRUCTS
            .iter()
            .copied()
            .filter(|name| !self.counts.contains_key(name))
            .collect()
    }
}

fn int(value: i128) -> Expr {
    if value < 0 {
        Expr::Unary(
            Unary::Negate,
            Box::new(Expr::Integer(Int::from_i128(-value))),
        )
    } else {
        Expr::Integer(Int::from_i128(value))
    }
}

fn name(text: &str) -> Expr {
    Expr::Name(text.to_string())
}

fn bin(operator: Binary, left: Expr, right: Expr) -> Expr {
    Expr::Binary(operator, Box::new(left), Box::new(right))
}

fn is_signed(ty: IntType) -> bool {
    matches!(ty, IntType::I8 | IntType::I16 | IntType::I32 | IntType::I64)
}

/// An expression of `ty` in `[0, 7]` (unsigned) or `[-7, 7]` (signed) that
/// mentions at least one variable from `vars` so `resolve` knows its type.
fn small_atom(rng: &mut Rng, ty: IntType, vars: &[&str]) -> Expr {
    let var = name(rng.pick(vars));
    let reduced = bin(Binary::Remainder, var, int(8));
    if is_signed(ty) && rng.chance(1, 3) {
        Expr::Unary(Unary::Negate, Box::new(reduced))
    } else if rng.chance(1, 4) {
        let k = rng.range(0, 7);
        bin(Binary::Remainder, bin(Binary::Add, reduced, int(k)), int(8))
    } else {
        reduced
    }
}

/// An arithmetic expression of `ty` over `vars` whose mathematical result is
/// always in `[0, 49]` (unsigned) or `[-49, 49]` (signed), and whose divisors
/// are never zero.
fn safe_arith(rng: &mut Rng, ty: IntType, vars: &[&str]) -> Expr {
    let left = small_atom(rng, ty, vars);
    let right = small_atom(rng, ty, vars);
    match *rng.pick(&ARITHMETIC_OPS) {
        Binary::Add => bin(Binary::Add, left, right),
        Binary::Subtract => {
            if is_signed(ty) {
                bin(Binary::Subtract, left, right)
            } else {
                bin(Binary::Subtract, bin(Binary::Add, left, int(7)), right)
            }
        }
        Binary::Multiply => bin(Binary::Multiply, left, right),
        Binary::Divide => {
            let divisor = rng.range(1, 7);
            bin(Binary::Divide, left, int(divisor))
        }
        Binary::Remainder => {
            let divisor = rng.range(1, 7);
            bin(Binary::Remainder, left, int(divisor))
        }
        _ => left,
    }
}

fn safe_reduced(rng: &mut Rng, ty: IntType, vars: &[&str]) -> Expr {
    bin(Binary::Remainder, safe_arith(rng, ty, vars), int(8))
}

fn stmt(selector: Selector, command: Command) -> Statement {
    Statement {
        selector,
        command,
        line: 1,
    }
}

/// Generates a core program that is valid by construction: it resolves, and no
/// valid execution is stuck in the oracle under 32-bit or 64-bit `usize`.
pub fn valid(rng: &mut Rng) -> Program {
    let int_ty = *rng.pick(&IntType::ALL);
    let int_ty2 = *rng.pick(&IntType::ALL);
    let signed_ty = *rng.pick(&SIGNED_TYPES);
    let cmp1 = *rng.pick(&COMPARISONS);
    let cmp2 = *rng.pick(&COMPARISONS);

    // 1. `f_calc(a : int_ty, b : int_ty) : int_ty` — always in [-7, 7] / [0, 7].
    let f_calc = Function {
        name: "f_calc".to_string(),
        parameters: vec![
            Parameter {
                name: "a".to_string(),
                ty: Type::Int(int_ty),
            },
            Parameter {
                name: "b".to_string(),
                ty: Type::Int(int_ty),
            },
        ],
        precondition: None,
        result: Type::Int(int_ty),
        body: safe_reduced(rng, int_ty, &["a", "b"]),
        line: 1,
    };

    // 2. `f_guard(a : int_ty) ⊢ a > 0 : int_ty` — has a precondition.
    let f_guard = Function {
        name: "f_guard".to_string(),
        parameters: vec![Parameter {
            name: "a".to_string(),
            ty: Type::Int(int_ty),
        }],
        precondition: Some(bin(Binary::Greater, name("a"), int(0))),
        result: Type::Int(int_ty),
        body: Expr::Call(
            "f_calc".to_string(),
            vec![name("a"), safe_reduced(rng, int_ty, &["a"])],
        ),
        line: 1,
    };

    // 3. `f_pred(s : signed_ty, flag : bool) : bool` — exercises unary `-`, `¬`,
    // comparisons, `∧` and `∨`.
    let neg_s = Expr::Unary(
        Unary::Negate,
        Box::new(bin(Binary::Remainder, name("s"), int(8))),
    );
    let f_pred = Function {
        name: "f_pred".to_string(),
        parameters: vec![
            Parameter {
                name: "s".to_string(),
                ty: Type::Int(signed_ty),
            },
            Parameter {
                name: "flag".to_string(),
                ty: Type::Bool,
            },
        ],
        precondition: None,
        result: Type::Bool,
        body: bin(
            Binary::Or,
            bin(
                Binary::And,
                Expr::Unary(Unary::Not, Box::new(name("flag"))),
                bin(cmp1, neg_s.clone(), int(0)),
            ),
            bin(cmp2, neg_s, bin(Binary::Remainder, name("s"), int(8))),
        ),
        line: 1,
    };

    // 4. Pure extern control `ext_pure(a : int_ty) ⊢ a > 0`.
    let ext_pure = Control {
        name: "ext_pure".to_string(),
        public: false,
        external: true,
        parameters: vec![Parameter {
            name: "a".to_string(),
            ty: Type::Int(int_ty),
        }],
        precondition: Some(bin(Binary::Greater, name("a"), int(0))),
        contract: vec![
            Alternative {
                name: "ok".to_string(),
                fields: vec![Parameter {
                    name: "v".to_string(),
                    ty: Type::Int(int_ty),
                }],
                condition: Some(bin(
                    Binary::And,
                    bin(Binary::GreaterEqual, name("v"), int(-7)),
                    bin(Binary::LessEqual, name("v"), int(7)),
                )),
            },
            Alternative {
                name: "empty".to_string(),
                fields: Vec::new(),
                condition: None,
            },
        ],
        uses: Vec::new(),
        body: Vec::new(),
        line: 1,
    };

    // 5. Effectful extern control `ext_io(b : int_ty2) uses io`.
    let ext_io = Control {
        name: "ext_io".to_string(),
        public: false,
        external: true,
        parameters: vec![Parameter {
            name: "b".to_string(),
            ty: Type::Int(int_ty2),
        }],
        precondition: None,
        contract: vec![
            Alternative {
                name: "val".to_string(),
                fields: vec![Parameter {
                    name: "w".to_string(),
                    ty: Type::Int(int_ty2),
                }],
                condition: Some(bin(
                    Binary::And,
                    bin(Binary::GreaterEqual, name("w"), int(0)),
                    bin(Binary::LessEqual, name("w"), int(7)),
                )),
            },
            Alternative {
                name: "closed".to_string(),
                fields: Vec::new(),
                condition: None,
            },
        ],
        uses: vec!["io".to_string()],
        body: Vec::new(),
        line: 1,
    };

    // 6. Defined helper control `helper(x : int_ty, s : signed_ty, flag : bool) ⊢ x > 0`.
    let helper = Control {
        name: "helper".to_string(),
        public: false,
        external: false,
        parameters: vec![
            Parameter {
                name: "x".to_string(),
                ty: Type::Int(int_ty),
            },
            Parameter {
                name: "s".to_string(),
                ty: Type::Int(signed_ty),
            },
            Parameter {
                name: "flag".to_string(),
                ty: Type::Bool,
            },
        ],
        precondition: Some(bin(Binary::Greater, name("x"), int(0))),
        contract: vec![
            Alternative {
                name: "hit".to_string(),
                fields: vec![Parameter {
                    name: "r".to_string(),
                    ty: Type::Int(int_ty),
                }],
                condition: Some(bin(
                    Binary::And,
                    bin(Binary::GreaterEqual, name("r"), int(-14)),
                    bin(Binary::LessEqual, name("r"), int(14)),
                )),
            },
            Alternative {
                name: "miss".to_string(),
                fields: Vec::new(),
                condition: None,
            },
        ],
        uses: Vec::new(),
        body: vec![
            stmt(
                Selector::Top,
                Command::Test {
                    site: "ht".to_string(),
                    condition: bin(Binary::And, name("flag"), Expr::Boolean(true)),
                },
            ),
            stmt(
                Selector::Atom("ht".to_string(), "true".to_string()),
                Command::Bind {
                    mutable: false,
                    name: " checked_pred".trim().to_string(),
                    ty: Some(Type::Bool),
                    value: Expr::Call("f_pred".to_string(), vec![name("s"), name("flag")]),
                },
            ),
            stmt(
                Selector::Atom("ht".to_string(), "true".to_string()),
                Command::Exit {
                    alternative: "hit".to_string(),
                    payload: vec![Expr::Call("f_guard".to_string(), vec![name("x")])],
                },
            ),
            stmt(
                Selector::Atom("ht".to_string(), "false".to_string()),
                Command::Exit {
                    alternative: "miss".to_string(),
                    payload: Vec::new(),
                },
            ),
        ],
        line: 1,
    };

    // 7. Entry control `pub control main(...) uses io`.
    let t0_true = Selector::Atom("t0".to_string(), "true".to_string());
    let t0_false = Selector::Atom("t0".to_string(), "false".to_string());
    let not_t0_true = Selector::Not(Box::new(t0_true.clone()));
    let contrad = Selector::And(Box::new(t0_true.clone()), Box::new(t0_false.clone()));
    let pos_true = Selector::Atom("pos".to_string(), "true".to_string());

    let lt_true = Selector::Atom("lt".to_string(), "true".to_string());
    let lt_false = Selector::Atom("lt".to_string(), "false".to_string());
    let loop_contrad = Selector::And(Box::new(lt_true.clone()), Box::new(lt_false.clone()));

    let main_body = vec![
        stmt(
            Selector::Top,
            Command::Bind {
                mutable: false,
                name: "small_a".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: bin(Binary::Remainder, name("a"), int(8)),
            },
        ),
        stmt(
            Selector::Top,
            Command::Bind {
                mutable: true,
                name: "acc".to_string(),
                ty: None,
                value: Expr::Call("f_calc".to_string(), vec![name("a"), name("small_a")]),
            },
        ),
        stmt(
            Selector::Top,
            Command::Bind {
                mutable: false,
                name: "neg_s".to_string(),
                ty: None,
                value: Expr::Unary(
                    Unary::Negate,
                    Box::new(bin(Binary::Remainder, name("s"), int(8))),
                ),
            },
        ),
        stmt(
            Selector::Top,
            Command::Bind {
                mutable: true,
                name: "loop_start".to_string(),
                ty: Some(Type::Int(IntType::U32)),
                value: int(2),
            },
        ),
        stmt(
            Selector::Top,
            Command::Test {
                site: "t0".to_string(),
                condition: name("flag"),
            },
        ),
        // Always skipped in the valid program; when `DropGuard` drops the
        // selector to `⊤`, the mutant is stuck at `X6 E-Impossible`.
        stmt(contrad.clone(), Command::Impossible),
        // Always skipped in the valid program; when `DropGuard` drops the
        // selector to `⊤`, `loop_start` becomes 9 and the loop is stuck at
        // `X7 E-Loop`.
        stmt(
            Selector::And(Box::new(t0_true.clone()), Box::new(not_t0_true.clone())),
            Command::Assign {
                name: "loop_start".to_string(),
                value: int(9),
            },
        ),
        // Disjoint binders of `shared` ([W4]); dropping `¬t0.true` hits `X3 E-Bind`.
        stmt(
            t0_true.clone(),
            Command::Bind {
                mutable: false,
                name: "shared".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: name("small_a"),
            },
        ),
        stmt(
            not_t0_true,
            Command::Bind {
                mutable: false,
                name: "shared".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: safe_reduced(rng, int_ty, &["a", "small_a"]),
            },
        ),
        // Branch-local bindings; dropping `t0.true` on the read or assignment
        // hits `X2 E-Var` and `X2 E-Assign` when `flag` is false.
        stmt(
            t0_true.clone(),
            Command::Bind {
                mutable: false,
                name: "only_true".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: name("shared"),
            },
        ),
        stmt(
            t0_true.clone(),
            Command::Bind {
                mutable: true,
                name: "mut_true".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: name("shared"),
            },
        ),
        stmt(
            t0_true.clone(),
            Command::Bind {
                mutable: false,
                name: "read_true".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: name("only_true"),
            },
        ),
        stmt(
            t0_true.clone(),
            Command::Assign {
                name: "mut_true".to_string(),
                value: name("shared"),
            },
        ),
        // Guarded obligation; dropping `t0.true` hits `X7 E-Oblige` when `flag` is false.
        stmt(
            t0_true.clone(),
            Command::Oblige(bin(
                Binary::And,
                bin(Binary::Equal, name("flag"), Expr::Boolean(true)),
                bin(
                    Binary::Equal,
                    Expr::Call("f_calc".to_string(), vec![name("a"), name("small_a")]),
                    Expr::Call("f_calc".to_string(), vec![name("a"), name("small_a")]),
                ),
            )),
        ),
        // Guard for positive `a`; dropping `pos.true` on the statements below
        // hits `X7 E-Arith`, `X7 E-Fn`, `X7 E-Extern` and `X7 E-Call` when `a = 0`.
        stmt(
            Selector::Top,
            Command::Test {
                site: "pos".to_string(),
                condition: bin(Binary::Greater, name("a"), int(0)),
            },
        ),
        stmt(
            pos_true.clone(),
            Command::Bind {
                mutable: false,
                name: "quotient".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: bin(Binary::Divide, name("small_a"), name("a")),
            },
        ),
        stmt(
            pos_true.clone(),
            Command::Bind {
                mutable: false,
                name: "from_fn".to_string(),
                ty: Some(Type::Int(int_ty)),
                value: Expr::Call("f_guard".to_string(), vec![name("a")]),
            },
        ),
        stmt(
            pos_true.clone(),
            Command::Decide {
                site: "d_ext".to_string(),
                operation: "ext_pure".to_string(),
                arguments: vec![name("a")],
                arms: vec![
                    Arm {
                        alternative: "ok".to_string(),
                        names: vec!["ev".to_string()],
                    },
                    Arm {
                        alternative: "empty".to_string(),
                        names: Vec::new(),
                    },
                ],
            },
        ),
        stmt(
            Selector::Atom("d_ext".to_string(), "ok".to_string()),
            Command::Assign {
                name: "acc".to_string(),
                value: Expr::Call("f_calc".to_string(), vec![name("acc"), name("ev")]),
            },
        ),
        stmt(
            pos_true,
            Command::Decide {
                site: "d_help".to_string(),
                operation: "helper".to_string(),
                arguments: vec![name("a"), name("s"), name("flag")],
                arms: vec![
                    Arm {
                        alternative: "hit".to_string(),
                        names: vec!["hv".to_string()],
                    },
                    Arm {
                        alternative: "miss".to_string(),
                        names: Vec::new(),
                    },
                ],
            },
        ),
        stmt(
            Selector::Atom("d_help".to_string(), "hit".to_string()),
            Command::Assign {
                name: "acc".to_string(),
                value: Expr::Call("f_calc".to_string(), vec![name("acc"), name("hv")]),
            },
        ),
        // Effectful extern call; dropping `io` from `main`'s `uses` hits `X9 E-Effect`.
        stmt(
            Selector::Top,
            Command::Decide {
                site: "d_io".to_string(),
                operation: "ext_io".to_string(),
                arguments: vec![name("b")],
                arms: vec![
                    Arm {
                        alternative: "val".to_string(),
                        names: vec!["wv".to_string()],
                    },
                    Arm {
                        alternative: "closed".to_string(),
                        names: Vec::new(),
                    },
                ],
            },
        ),
        // Loop with invariant, mutable parameter, `↑` and `□`.
        stmt(
            Selector::Top,
            Command::Loop {
                site: "lp".to_string(),
                name: "step".to_string(),
                parameters: vec![
                    LoopParameter {
                        mutable: false,
                        name: "rem".to_string(),
                        ty: Type::Int(IntType::U32),
                        initial: name("loop_start"),
                    },
                    LoopParameter {
                        mutable: true,
                        name: "cur".to_string(),
                        ty: Type::Int(int_ty),
                        initial: name("acc"),
                    },
                ],
                invariant: Some(bin(Binary::LessEqual, name("rem"), int(3))),
                body: vec![
                    stmt(
                        Selector::Top,
                        Command::Test {
                            site: "lt".to_string(),
                            condition: bin(Binary::Greater, name("rem"), int(0)),
                        },
                    ),
                    // Always skipped; dropping the selector hits `X7 E-Again`.
                    stmt(
                        loop_contrad,
                        Command::Again {
                            name: "step".to_string(),
                            arguments: vec![int(9), name("cur")],
                        },
                    ),
                    stmt(
                        lt_true.clone(),
                        Command::Assign {
                            name: "cur".to_string(),
                            value: Expr::Call(
                                "f_calc".to_string(),
                                vec![name("cur"), name("small_a")],
                            ),
                        },
                    ),
                    stmt(
                        Selector::And(Box::new(lt_true.clone()), Box::new(t0_false)),
                        Command::Leave {
                            alternative: "stopped".to_string(),
                            payload: Vec::new(),
                        },
                    ),
                    stmt(
                        Selector::And(Box::new(lt_true), Box::new(t0_true)),
                        Command::Again {
                            name: "step".to_string(),
                            arguments: vec![
                                bin(Binary::Subtract, name("rem"), int(1)),
                                name("cur"),
                            ],
                        },
                    ),
                    stmt(
                        lt_false,
                        Command::Leave {
                            alternative: "finished".to_string(),
                            payload: vec![name("cur")],
                        },
                    ),
                ],
                arms: vec![
                    Arm {
                        alternative: "finished".to_string(),
                        names: vec!["loop_out".to_string()],
                    },
                    Arm {
                        alternative: "stopped".to_string(),
                        names: Vec::new(),
                    },
                ],
            },
        ),
        stmt(
            Selector::Or(
                Box::new(Selector::Atom("lp".to_string(), "finished".to_string())),
                Box::new(Selector::Atom("lp".to_string(), "stopped".to_string())),
            ),
            Command::Assign {
                name: "acc".to_string(),
                value: Expr::Call("f_calc".to_string(), vec![name("acc"), name("shared")]),
            },
        ),
        // Guarded exit with outcome condition; dropping `fit.true` hits `X7 E-Exit`
        // when `a > 14`, and removing either exit hits `X1 E-End`.
        stmt(
            Selector::Top,
            Command::Test {
                site: "fit".to_string(),
                condition: bin(
                    Binary::And,
                    bin(Binary::GreaterEqual, name("a"), int(0)),
                    bin(Binary::LessEqual, name("a"), int(7)),
                ),
            },
        ),
        stmt(
            Selector::Atom("fit".to_string(), "true".to_string()),
            Command::Exit {
                alternative: "bounded".to_string(),
                payload: vec![name("a")],
            },
        ),
        stmt(
            Selector::Atom("fit".to_string(), "false".to_string()),
            Command::Exit {
                alternative: "fallback".to_string(),
                payload: vec![name("b")],
            },
        ),
    ];

    let main_control = Control {
        name: "main".to_string(),
        public: true,
        external: false,
        parameters: vec![
            Parameter {
                name: "a".to_string(),
                ty: Type::Int(int_ty),
            },
            Parameter {
                name: "b".to_string(),
                ty: Type::Int(int_ty2),
            },
            Parameter {
                name: "s".to_string(),
                ty: Type::Int(signed_ty),
            },
            Parameter {
                name: "flag".to_string(),
                ty: Type::Bool,
            },
        ],
        precondition: None,
        contract: vec![
            Alternative {
                name: "bounded".to_string(),
                fields: vec![Parameter {
                    name: "res".to_string(),
                    ty: Type::Int(int_ty),
                }],
                condition: Some(bin(
                    Binary::And,
                    bin(Binary::GreaterEqual, name("res"), int(-14)),
                    bin(Binary::LessEqual, name("res"), int(14)),
                )),
            },
            Alternative {
                name: "fallback".to_string(),
                fields: vec![Parameter {
                    name: "code".to_string(),
                    ty: Type::Int(int_ty2),
                }],
                condition: None,
            },
        ],
        uses: vec!["io".to_string()],
        body: main_body,
        line: 1,
    };

    Program {
        declarations: vec![
            Declaration::Function(f_calc),
            Declaration::Function(f_guard),
            Declaration::Function(f_pred),
            Declaration::Control(ext_pure),
            Declaration::Control(ext_io),
            Declaration::Control(helper),
            Declaration::Control(main_control),
        ],
    }
}

/// Builds a suite of runs `(entry_control, arguments)` for `program` that
/// satisfy each entry control's precondition and exercise both branches of
/// guards in [`valid`] programs.
pub fn sample_runs(
    rng: &mut Rng,
    program: &typed::Program,
    config: &Config,
) -> Vec<(String, Vec<Value>)> {
    let mut out = Vec::new();
    for control in &program.controls {
        if control.body.is_none() {
            continue;
        }
        // Targeted inputs for the 4-parameter `main` of `valid`.
        if control.name == "main" && control.parameters.len() == 4 {
            for (a, b, s, flag) in [
                (3, 2, -3, true),
                (0, 4, 2, false),
                (20, 1, -1, true),
                (20, 5, 0, false),
            ] {
                let candidate = vec![
                    Value::Int(Int::from_i128(a)),
                    Value::Int(Int::from_i128(b)),
                    Value::Int(Int::from_i128(s)),
                    Value::Bool(flag),
                ];
                let fits = candidate
                    .iter()
                    .zip(&control.parameters)
                    .all(|(v, p)| config.fits(v, p.ty));
                if fits {
                    out.push((control.name.clone(), candidate));
                }
            }
        }
        for _ in 0..16 {
            let candidate: Vec<Value> = control
                .parameters
                .iter()
                .map(|parameter| draw_value(rng, parameter.ty, &[], config))
                .collect();
            let valid_entry = match &control.precondition {
                None => true,
                Some(pre) => signature_holds(program, config, pre, &candidate, &[]),
            };
            if valid_entry {
                out.push((control.name.clone(), candidate));
                if out.len() >= 6 {
                    break;
                }
            }
        }
    }
    out
}

// Free program generator: arbitrary well-resolved programs that need not pass
// the checker or avoid stuck states in the oracle.

#[derive(Clone)]
struct VarInfo {
    name: String,
    ty: Type,
    mutable: bool,
}

#[derive(Clone)]
struct SiteInfo {
    name: String,
    alternatives: Vec<String>,
}

fn free_literal(rng: &mut Rng, ty: IntType) -> Expr {
    let (least, greatest) = ty.literal_bounds();
    let low = least.to_i128().unwrap_or(i128::from(i64::MIN));
    let high = greatest.to_i128().unwrap_or(i128::from(u64::MAX));
    let value = match rng.below(4) {
        0 => rng.range(0.max(low), 9.min(high)),
        1 => rng.range((-9).max(low), 9.min(high)),
        2 => *rng.pick(&[low, 0.clamp(low, high), 1.clamp(low, high), high]),
        _ => rng.range((-500).max(low), 500.min(high)),
    };
    int(value)
}

fn free_expr(
    rng: &mut Rng,
    ty: Type,
    vars: &[VarInfo],
    functions: &[Function],
    depth: u8,
    need_typed: bool,
) -> Expr {
    let matching_vars: Vec<&VarInfo> = vars.iter().filter(|var| var.ty == ty).collect();
    let matching_fns: Vec<&Function> = functions.iter().filter(|f| f.result == ty).collect();

    if need_typed {
        if let Some(var) = matching_vars.first() {
            if depth == 0 || rng.chance(1, 2) {
                return name(&var.name);
            }
        } else if let Some(func) = matching_fns.first() {
            let args = func
                .parameters
                .iter()
                .map(|p| free_expr(rng, p.ty, vars, functions, 0, false))
                .collect();
            return Expr::Call(func.name.clone(), args);
        }
    }

    if depth == 0 {
        if !matching_vars.is_empty() && rng.chance(2, 3) {
            return name(&rng.pick(&matching_vars).name);
        }
        return match ty {
            Type::Bool => Expr::Boolean(rng.chance(1, 2)),
            Type::Int(int_ty) => free_literal(rng, int_ty),
        };
    }

    match ty {
        Type::Bool => match rng.below(6) {
            0 if !matching_vars.is_empty() => name(&rng.pick(&matching_vars).name),
            1 if !matching_fns.is_empty() => {
                let func = *rng.pick(&matching_fns);
                let args = func
                    .parameters
                    .iter()
                    .map(|p| free_expr(rng, p.ty, vars, functions, depth - 1, false))
                    .collect();
                Expr::Call(func.name.clone(), args)
            }
            2 => Expr::Unary(
                Unary::Not,
                Box::new(free_expr(
                    rng,
                    Type::Bool,
                    vars,
                    functions,
                    depth - 1,
                    false,
                )),
            ),
            3 => {
                let op = if rng.chance(1, 2) {
                    Binary::And
                } else {
                    Binary::Or
                };
                bin(
                    op,
                    free_expr(rng, Type::Bool, vars, functions, depth - 1, false),
                    free_expr(rng, Type::Bool, vars, functions, depth - 1, false),
                )
            }
            _ => {
                // Pick an operand type for a comparison. If it is an integer type
                // other than `u32`, at least one side must carry a typed variable
                // or function call so `resolve` infers that type.
                let available_types: Vec<Type> = vars.iter().map(|v| v.ty).collect();
                let operand_ty = if !available_types.is_empty() {
                    *rng.pick(&available_types)
                } else {
                    Type::Int(IntType::U32)
                };
                let cmp = if operand_ty == Type::Bool {
                    if rng.chance(1, 2) {
                        Binary::Equal
                    } else {
                        Binary::NotEqual
                    }
                } else {
                    *rng.pick(&COMPARISONS)
                };
                let need = operand_ty != Type::Bool && operand_ty != Type::Int(IntType::U32);
                let left = free_expr(rng, operand_ty, vars, functions, depth - 1, need);
                let right = free_expr(rng, operand_ty, vars, functions, depth - 1, false);
                bin(cmp, left, right)
            }
        },
        Type::Int(int_ty) => match rng.below(5) {
            0 if !matching_vars.is_empty() => name(&rng.pick(&matching_vars).name),
            1 if !matching_fns.is_empty() => {
                let func = *rng.pick(&matching_fns);
                let args = func
                    .parameters
                    .iter()
                    .map(|p| free_expr(rng, p.ty, vars, functions, depth - 1, false))
                    .collect();
                Expr::Call(func.name.clone(), args)
            }
            2 if is_signed(int_ty) && !matching_vars.is_empty() => Expr::Unary(
                Unary::Negate,
                Box::new(name(&rng.pick(&matching_vars).name)),
            ),
            _ => {
                let op = *rng.pick(&ARITHMETIC_OPS);
                let left = free_expr(rng, ty, vars, functions, depth - 1, need_typed);
                let right = free_expr(rng, ty, vars, functions, depth - 1, false);
                bin(op, left, right)
            }
        },
    }
}

fn free_selector(rng: &mut Rng, sites: &[SiteInfo], depth: u8) -> Selector {
    if sites.is_empty() || (depth == 0 && rng.chance(1, 3)) {
        return Selector::Top;
    }
    if depth == 0 {
        let site = rng.pick(sites);
        return Selector::Atom(site.name.clone(), rng.pick(&site.alternatives).clone());
    }
    match rng.below(5) {
        0 => Selector::Top,
        1 => {
            let site = rng.pick(sites);
            Selector::Atom(site.name.clone(), rng.pick(&site.alternatives).clone())
        }
        2 => Selector::Not(Box::new(free_selector(rng, sites, depth - 1))),
        3 => Selector::And(
            Box::new(free_selector(rng, sites, depth - 1)),
            Box::new(free_selector(rng, sites, depth - 1)),
        ),
        _ => Selector::Or(
            Box::new(free_selector(rng, sites, depth - 1)),
            Box::new(free_selector(rng, sites, depth - 1)),
        ),
    }
}

/// Generates a free well-resolved [`Program`] that reaches the constructs and
/// stuck states of slice 1 without checking obligations or coverage.
pub fn free(rng: &mut Rng) -> Program {
    let mut functions = Vec::new();
    let fn_count = 1 + rng.index(2);
    for index in 0..fn_count {
        let param_ty = *rng.pick(&TYPES);
        let result_ty = *rng.pick(&TYPES);
        let param_name = "p0".to_string();
        let vars = vec![VarInfo {
            name: param_name.clone(),
            ty: param_ty,
            mutable: false,
        }];
        let precondition = if rng.chance(1, 2) {
            Some(free_expr(rng, Type::Bool, &vars, &functions, 1, false))
        } else {
            None
        };
        let body = free_expr(rng, result_ty, &vars, &functions, 1, false);
        functions.push(Function {
            name: format!("fn{index}"),
            parameters: vec![Parameter {
                name: param_name,
                ty: param_ty,
            }],
            precondition,
            result: result_ty,
            body,
            line: 1,
        });
    }

    let mut controls = Vec::new();
    // An extern control with two alternatives.
    let ext_param_ty = *rng.pick(&TYPES);
    let ext_field_ty = *rng.pick(&TYPES);
    let ext_vars = vec![
        VarInfo {
            name: "ep".to_string(),
            ty: ext_param_ty,
            mutable: false,
        },
        VarInfo {
            name: "ef".to_string(),
            ty: ext_field_ty,
            mutable: false,
        },
    ];
    controls.push(Control {
        name: "ext0".to_string(),
        public: false,
        external: true,
        parameters: vec![Parameter {
            name: "ep".to_string(),
            ty: ext_param_ty,
        }],
        precondition: if rng.chance(1, 2) {
            Some(free_expr(
                rng,
                Type::Bool,
                &ext_vars[..1],
                &functions,
                1,
                false,
            ))
        } else {
            None
        },
        contract: vec![
            Alternative {
                name: "some".to_string(),
                fields: vec![Parameter {
                    name: "ef".to_string(),
                    ty: ext_field_ty,
                }],
                condition: if rng.chance(1, 2) {
                    Some(free_expr(rng, Type::Bool, &ext_vars, &functions, 1, false))
                } else {
                    None
                },
            },
            Alternative {
                name: "none".to_string(),
                fields: Vec::new(),
                condition: None,
            },
        ],
        uses: if rng.chance(1, 2) {
            vec!["io".to_string()]
        } else {
            Vec::new()
        },
        body: Vec::new(),
        line: 1,
    });

    // A defined control `entry`.
    let p0_ty = *rng.pick(&TYPES);
    let p1_ty = *rng.pick(&TYPES);
    let out_ty = *rng.pick(&TYPES);
    let mut vars = vec![
        VarInfo {
            name: "a0".to_string(),
            ty: p0_ty,
            mutable: false,
        },
        VarInfo {
            name: "a1".to_string(),
            ty: p1_ty,
            mutable: false,
        },
    ];
    let mut sites: Vec<SiteInfo> = Vec::new();
    let mut statements = Vec::new();
    let mut next_id = 0usize;

    let step_count = 4 + rng.index(5);
    for _ in 0..step_count {
        let selector = free_selector(rng, &sites, 1);
        match rng.below(8) {
            0 => {
                // Bind a local (sometimes mutable, sometimes inferred).
                let mutable = rng.chance(1, 2);
                let inferred = rng.chance(1, 3);
                let ty = if inferred {
                    vars.first().map_or(Type::Int(IntType::U32), |v| v.ty)
                } else {
                    *rng.pick(&TYPES)
                };
                let need = inferred && ty != Type::Bool && ty != Type::Int(IntType::U32);
                let value = free_expr(rng, ty, &vars, &functions, 1, need);
                let var_name = format!("v{next_id}");
                next_id += 1;
                vars.push(VarInfo {
                    name: var_name.clone(),
                    ty,
                    mutable,
                });
                statements.push(stmt(
                    selector,
                    Command::Bind {
                        mutable,
                        name: var_name,
                        ty: if inferred { None } else { Some(ty) },
                        value,
                    },
                ));
            }
            1 => {
                // Assign to an existing mutable variable, if one exists.
                let mutables: Vec<VarInfo> = vars.iter().filter(|v| v.mutable).cloned().collect();
                if !mutables.is_empty() {
                    let chosen = rng.pick(&mutables);
                    let value = free_expr(rng, chosen.ty, &vars, &functions, 1, false);
                    statements.push(stmt(
                        selector,
                        Command::Assign {
                            name: chosen.name.clone(),
                            value,
                        },
                    ));
                } else {
                    statements.push(stmt(
                        selector,
                        Command::Oblige(free_expr(rng, Type::Bool, &vars, &functions, 1, false)),
                    ));
                }
            }
            2 => {
                statements.push(stmt(
                    selector,
                    Command::Oblige(free_expr(rng, Type::Bool, &vars, &functions, 1, false)),
                ));
            }
            3 => {
                let site = format!("s{next_id}");
                next_id += 1;
                let condition = free_expr(rng, Type::Bool, &vars, &functions, 1, false);
                sites.push(SiteInfo {
                    name: site.clone(),
                    alternatives: vec!["true".to_string(), "false".to_string()],
                });
                statements.push(stmt(selector, Command::Test { site, condition }));
            }
            4 => {
                // Decide on `ext0`, optionally omitting the second arm so [E-Decide]
                // can get stuck at X10.
                let site = format!("s{next_id}");
                next_id += 1;
                let arg = free_expr(rng, ext_param_ty, &vars, &functions, 1, false);
                let payload_var = format!("v{next_id}");
                next_id += 1;
                vars.push(VarInfo {
                    name: payload_var.clone(),
                    ty: ext_field_ty,
                    mutable: false,
                });
                let mut arms = vec![Arm {
                    alternative: "some".to_string(),
                    names: vec![payload_var],
                }];
                let mut listed = vec!["some".to_string()];
                if rng.chance(1, 2) {
                    arms.push(Arm {
                        alternative: "none".to_string(),
                        names: Vec::new(),
                    });
                    listed.push("none".to_string());
                }
                sites.push(SiteInfo {
                    name: site.clone(),
                    alternatives: listed,
                });
                statements.push(stmt(
                    selector,
                    Command::Decide {
                        site,
                        operation: "ext0".to_string(),
                        arguments: vec![arg],
                        arms,
                    },
                ));
            }
            5 => {
                // Loop with its own scope.
                let site = format!("s{next_id}");
                let loop_name = format!("lp{next_id}");
                let lp_var = format!("lv{next_id}");
                let out_var = format!("lo{next_id}");
                next_id += 1;
                let lp_ty = *rng.pick(&TYPES);
                let lp_mutable = rng.chance(1, 2);
                let initial = free_expr(rng, lp_ty, &vars, &functions, 1, false);
                let mut inner_vars: Vec<VarInfo> = vars.clone();
                inner_vars.push(VarInfo {
                    name: lp_var.clone(),
                    ty: lp_ty,
                    mutable: lp_mutable,
                });
                let invariant = if rng.chance(1, 2) {
                    Some(free_expr(
                        rng,
                        Type::Bool,
                        &inner_vars,
                        &functions,
                        1,
                        false,
                    ))
                } else {
                    None
                };
                let mut inner_sites = sites.clone();
                let test_site = format!("ls{next_id}");
                next_id += 1;
                let cond = free_expr(rng, Type::Bool, &inner_vars, &functions, 1, false);
                inner_sites.push(SiteInfo {
                    name: test_site.clone(),
                    alternatives: vec!["true".to_string(), "false".to_string()],
                });
                let again_arg = free_expr(rng, lp_ty, &inner_vars, &functions, 1, false);
                let body = vec![
                    stmt(
                        Selector::Top,
                        Command::Test {
                            site: test_site.clone(),
                            condition: cond,
                        },
                    ),
                    stmt(
                        Selector::Atom(test_site.clone(), "true".to_string()),
                        Command::Again {
                            name: loop_name.clone(),
                            arguments: vec![again_arg],
                        },
                    ),
                    stmt(
                        Selector::Atom(test_site, "false".to_string()),
                        Command::Leave {
                            alternative: "done".to_string(),
                            payload: vec![name(&lp_var)],
                        },
                    ),
                ];
                let _ = inner_vars;
                vars.push(VarInfo {
                    name: out_var.clone(),
                    ty: lp_ty,
                    mutable: false,
                });
                sites.push(SiteInfo {
                    name: site.clone(),
                    alternatives: vec!["done".to_string()],
                });
                statements.push(stmt(
                    selector,
                    Command::Loop {
                        site,
                        name: loop_name,
                        parameters: vec![LoopParameter {
                            mutable: lp_mutable,
                            name: lp_var,
                            ty: lp_ty,
                            initial,
                        }],
                        invariant,
                        body,
                        arms: vec![Arm {
                            alternative: "done".to_string(),
                            names: vec![out_var],
                        }],
                    },
                ));
            }
            6 => {
                statements.push(stmt(selector, Command::Impossible));
            }
            _ => {
                if rng.chance(1, 2) {
                    let payload = free_expr(rng, out_ty, &vars, &functions, 1, false);
                    statements.push(stmt(
                        selector,
                        Command::Exit {
                            alternative: "out".to_string(),
                            payload: vec![payload],
                        },
                    ));
                } else {
                    statements.push(stmt(
                        selector,
                        Command::Exit {
                            alternative: " halt".trim().to_string(),
                            payload: Vec::new(),
                        },
                    ));
                }
            }
        }
    }
    if rng.chance(1, 2) {
        let payload = free_expr(rng, out_ty, &vars, &functions, 1, false);
        statements.push(stmt(
            free_selector(rng, &sites, 1),
            Command::Exit {
                alternative: "out".to_string(),
                payload: vec![payload],
            },
        ));
    }

    let sig_vars = vec![
        VarInfo {
            name: "a0".to_string(),
            ty: p0_ty,
            mutable: false,
        },
        VarInfo {
            name: "a1".to_string(),
            ty: p1_ty,
            mutable: false,
        },
        VarInfo {
            name: "res".to_string(),
            ty: out_ty,
            mutable: false,
        },
    ];
    controls.push(Control {
        name: "entry".to_string(),
        public: rng.chance(1, 2),
        external: false,
        parameters: vec![
            Parameter {
                name: "a0".to_string(),
                ty: p0_ty,
            },
            Parameter {
                name: "a1".to_string(),
                ty: p1_ty,
            },
        ],
        precondition: if rng.chance(1, 3) {
            Some(free_expr(
                rng,
                Type::Bool,
                &sig_vars[..2],
                &functions,
                1,
                false,
            ))
        } else {
            None
        },
        contract: vec![
            Alternative {
                name: "out".to_string(),
                fields: vec![Parameter {
                    name: "res".to_string(),
                    ty: out_ty,
                }],
                condition: if rng.chance(1, 2) {
                    Some(free_expr(rng, Type::Bool, &sig_vars, &functions, 1, false))
                } else {
                    None
                },
            },
            Alternative {
                name: "halt".to_string(),
                fields: Vec::new(),
                condition: None,
            },
        ],
        uses: if rng.chance(1, 2) {
            vec!["io".to_string()]
        } else {
            Vec::new()
        },
        body: statements,
        line: 1,
    });

    let mut declarations = Vec::new();
    for function in functions {
        declarations.push(Declaration::Function(function));
    }
    for control in controls {
        declarations.push(Declaration::Control(control));
    }
    Program { declarations }
}
