//! The erased semantics: section 9 of `docs/v2/core.md`, implemented
//! directly.
//!
//! This is what release code does, written as a second interpreter. It is
//! the oracle with the bookkeeping removed:
//!
//! - **[R1]** Slots hold values. There are no moved marks and no tags.
//! - **[R2]** `⊢`, invariants, preconditions, the conditions of alternatives
//!   and the allowances of effects are not evaluated. Integer arithmetic and
//!   indexing are unchecked. `⊥` has no behavior.
//! - **[R3]** Histories remain, as one bit per atom that some selector or
//!   destruction condition mentions.
//! - **[R4]** At the end of a scope the names destroyed are those whose
//!   availability formula holds on the current history ([O5], [O6]). No value
//!   carries a flag of its own.
//! - **[R5]** Under a fault, resources declared `on fault release` are released
//!   under the same condition.

use crate::host::Host;
use crate::int::Int;
use crate::oracle::{Config, Event, Value, list};
use crate::syntax::escape_string;
use crate::typed::{
    Arithmetic, BodyId, BorrowMode, BoundaryId, BuiltinOp, Command, Comparison, ControlBody,
    ControlId, Expr, IntType, Kind, Operation, Place, PlaceStep, Program, Selector, SiteId, Type,
    VarId, VarRef,
};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug)]
pub enum End {
    Done {
        alternative: String,
        payload: Vec<Value>,
    },
    Faulted(String),
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
            End::Faulted(message) => {
                write!(formatter, "faulted \"{}\"", escape_string(message))
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

enum Names<'a> {
    Slots(&'a [Value]),
    Arguments(&'a [Value]),
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

    fn word(self, value: &Value, ty: IntType) -> u64 {
        match value {
            Value::Int(v) => self.shape(ty).wrap(v.to_i128().unwrap_or(0) as u64),
            Value::Bool(b) => u64::from(*b),
            _ => 0,
        }
    }

    fn int_value(self, word: u64, ty: IntType) -> Value {
        let shape = self.shape(ty);
        if shape.signed {
            Value::Int(Int::from_i128(i128::from(shape.number(word))))
        } else {
            Value::Int(Int::from_u64(shape.wrap(word)))
        }
    }

    fn normalize(self, value: &Value, ty: &Type) -> Value {
        match (value, ty.deref_type()) {
            (Value::Bool(b), _) => Value::Bool(*b),
            (Value::Int(_), Type::Int(int_ty)) => {
                self.int_value(self.word(value, *int_ty), *int_ty)
            }
            (other, _) => other.clone(),
        }
    }

    fn eval(self, expr: &Expr, names: &Names) -> Result<Value, String> {
        Ok(match expr {
            Expr::Boolean(value) => Value::Bool(*value),
            Expr::Integer(value, ty) => {
                self.int_value(self.word(&Value::Int(value.clone()), *ty), *ty)
            }
            Expr::String(s) => Value::String(s.clone()),
            Expr::Var(var, _) => match (var, names) {
                (VarRef::Local(id), Names::Slots(slots)) => slots[*id].clone(),
                (VarRef::Parameter(index), Names::Arguments(arguments)) => {
                    arguments[*index].clone()
                }
                _ => return Err("a name of a signature stands in a body".to_string()),
            },
            Expr::Old(_, _) => return Err("`old` evaluated in erased code".to_string()),
            Expr::Move(inner, _) => self.eval(inner, names)?,
            Expr::Borrow(_, place, _) => {
                let Names::Slots(slots) = names else {
                    return Err("borrow in function".to_string());
                };
                let mut current = slots[place.root].clone();
                for step in &place.steps {
                    match step {
                        PlaceStep::Field(_, fidx, _) => {
                            let Value::Struct { mut fields, .. } = current else {
                                return Err("field access on non-struct".to_string());
                            };
                            current = fields.swap_remove(*fidx);
                        }
                        PlaceStep::Index(idx_expr, _) => {
                            let Value::Vector(mut items) = current else {
                                return Err("index on non-vector".to_string());
                            };
                            let idx =
                                self.word(&self.eval(idx_expr, names)?, IntType::Usize) as usize;
                            if idx >= items.len() {
                                return Err("out-of-bounds index".to_string());
                            }
                            current = items.swap_remove(idx);
                        }
                    }
                }
                current
            }
            Expr::Field(base, _, fidx, _) => {
                let Value::Struct { mut fields, .. } = self.eval(base, names)? else {
                    return Err("field access on non-struct".to_string());
                };
                fields.swap_remove(*fidx)
            }
            Expr::Index(base, idx_expr, _) => {
                let Value::Vector(mut items) = self.eval(base, names)? else {
                    return Err("index on non-vector".to_string());
                };
                let idx = self.word(&self.eval(idx_expr, names)?, IntType::Usize) as usize;
                if idx >= items.len() {
                    return Err("out-of-bounds index".to_string());
                }
                items.swap_remove(idx)
            }
            Expr::Struct(sid, fields, _) => {
                let mut vals = Vec::with_capacity(fields.len());
                for f in fields {
                    vals.push(self.eval(f, names)?);
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
                    vals.push(self.eval(f, names)?);
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
                    vals.push(self.eval(item, names)?);
                }
                Value::Vector(vals)
            }
            Expr::EmptyMap(_) => Value::Map(BTreeMap::new()),
            Expr::Len(place) => {
                let len = match self.eval(place, names)? {
                    Value::Vector(v) => v.len() as u64,
                    Value::String(s) => s.len() as u64,
                    Value::Map(m) => m.len() as u64,
                    _ => 0,
                };
                self.int_value(len, IntType::Usize)
            }
            Expr::Clone(place, _) => self.eval(place, names)?,
            Expr::Trim(arg) => {
                let Value::String(s) = self.eval(arg, names)? else {
                    return Err("`trim` on non-string".to_string());
                };
                Value::String(s.trim().to_string())
            }
            Expr::Not(operand) => match self.eval(operand, names)? {
                Value::Bool(b) => Value::Bool(!b),
                _ => Value::Bool(false),
            },
            Expr::Negate(operand, ty) => {
                let w = self.word(&self.eval(operand, names)?, *ty);
                self.int_value(self.shape(*ty).wrap(w.wrapping_neg()), *ty)
            }
            Expr::Arithmetic(operator, left, right, ty) => {
                let shape = self.shape(*ty);
                let left = self.word(&self.eval(left, names)?, *ty);
                let right = self.word(&self.eval(right, names)?, *ty);
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
                self.int_value(shape.wrap(result), *ty)
            }
            Expr::Compare(comparison, left, right) => {
                let l_ty = left.ty();
                let l_val = self.eval(left, names)?;
                let r_val = self.eval(right, names)?;
                let res = if let Type::Int(int_ty) = l_ty.deref_type() {
                    let shape = self.shape(*int_ty);
                    let lw = self.word(&l_val, *int_ty);
                    let rw = self.word(&r_val, *int_ty);
                    let ord = if shape.signed {
                        shape.number(lw).cmp(&shape.number(rw))
                    } else {
                        lw.cmp(&rw)
                    };
                    match comparison {
                        Comparison::Equal => ord.is_eq(),
                        Comparison::NotEqual => ord.is_ne(),
                        Comparison::Less => ord.is_lt(),
                        Comparison::LessEqual => ord.is_le(),
                        Comparison::Greater => ord.is_gt(),
                        Comparison::GreaterEqual => ord.is_ge(),
                    }
                } else {
                    match comparison {
                        Comparison::Equal => l_val == r_val,
                        Comparison::NotEqual => l_val != r_val,
                        _ => false,
                    }
                };
                Value::Bool(res)
            }
            Expr::And(left, right) => {
                let Value::Bool(l) = self.eval(left, names)? else {
                    return Ok(Value::Bool(false));
                };
                if !l {
                    Value::Bool(false)
                } else {
                    self.eval(right, names)?
                }
            }
            Expr::Or(left, right) => {
                let Value::Bool(l) = self.eval(left, names)? else {
                    return Ok(Value::Bool(false));
                };
                if l {
                    Value::Bool(true)
                } else {
                    self.eval(right, names)?
                }
            }
            Expr::Call(id, arguments, _) => {
                let mut vals = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    vals.push(self.eval(argument, names)?);
                }
                self.eval(&self.program.functions[*id].body, &Names::Arguments(&vals))?
            }
        })
    }
}

/// Static layout for a `ControlBody` under `[R3]`, `[R4]` and `[R5]`.
struct Plan {
    /// [R3]: the bit of each atom that a selector or destruction/fault-release
    /// condition mentions, by site and alternative.
    bit: Vec<Vec<Option<usize>>>,
    bits: usize,
    /// The bits of the sites of each body. A new run of the body clears them.
    own: Vec<Vec<usize>>,
    /// [R4], [R5]: static availability selector `D(x) ∧ ¬M(x)` for each
    /// `(body_id, statement_pc, var_id)` after statement `pc`'s arguments
    /// have moved, used to decide resource release on fault unwind with zero
    /// runtime moved flags.
    avail_at: Vec<Vec<Vec<Option<Selector>>>>,
}

fn sel_or(a: Option<Selector>, b: Selector) -> Option<Selector> {
    match a {
        None => Some(b),
        Some(Selector::Top) => Some(Selector::Top),
        Some(prev) => {
            if matches!(b, Selector::Top) {
                Some(Selector::Top)
            } else {
                Some(Selector::Or(Box::new(prev), Box::new(b)))
            }
        }
    }
}

fn sel_and_not(a: Option<Selector>, b: &Selector) -> Option<Selector> {
    let prev = a?;
    if matches!(b, Selector::Top) {
        None
    } else {
        Some(Selector::And(
            Box::new(prev),
            Box::new(Selector::Not(Box::new(b.clone()))),
        ))
    }
}

fn collect_consumed_vars(
    program: &Program,
    body: &ControlBody,
    expr: &Expr,
    consumed: bool,
    out: &mut Vec<VarId>,
) {
    match expr {
        Expr::Var(VarRef::Local(id), _) => {
            if consumed && program.kind(&body.vars[*id].ty) != Kind::Copy {
                out.push(*id);
            }
        }
        Expr::Move(inner, _) => collect_consumed_vars(program, body, inner, true, out),
        Expr::Struct(_, fields, _) | Expr::Variant(_, _, fields, _) | Expr::Vector(fields, _) => {
            for f in fields {
                collect_consumed_vars(program, body, f, true, out);
            }
        }
        Expr::Field(base, _, _, _)
        | Expr::Len(base)
        | Expr::Clone(base, _)
        | Expr::Trim(base)
        | Expr::Not(base)
        | Expr::Negate(base, _) => {
            collect_consumed_vars(program, body, base, false, out);
        }
        Expr::Index(base, idx, _)
        | Expr::Arithmetic(_, base, idx, _)
        | Expr::Compare(_, base, idx)
        | Expr::And(base, idx)
        | Expr::Or(base, idx) => {
            collect_consumed_vars(program, body, base, false, out);
            collect_consumed_vars(program, body, idx, false, out);
        }
        Expr::Call(_, args, _) => {
            for a in args {
                collect_consumed_vars(program, body, a, false, out);
            }
        }
        Expr::Borrow(_, place, _) => {
            for step in &place.steps {
                if let PlaceStep::Index(idx, _) = step {
                    collect_consumed_vars(program, body, idx, false, out);
                }
            }
        }
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Var(_, _)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => {}
    }
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

fn compute_body_availability(
    program: &Program,
    body: &ControlBody,
    body_id: BodyId,
    param_count: usize,
    mut defined: Vec<Option<Selector>>,
    mut moved: Vec<Option<Selector>>,
    avail_at: &mut [Vec<Vec<Option<Selector>>>],
) -> (Vec<Option<Selector>>, Vec<Option<Selector>>) {
    if body_id == 0 {
        for slot in defined.iter_mut().take(param_count.min(body.vars.len())) {
            *slot = Some(Selector::Top);
        }
    } else if let Some(head) = &body.bodies[body_id].head {
        for &p in &head.parameters {
            defined[p] = Some(Selector::Top);
            moved[p] = None;
        }
    }

    let stmts = &body.bodies[body_id].statements;
    let mut body_avail = Vec::with_capacity(stmts.len());

    for stmt in stmts {
        let g = &stmt.selector;
        // 1. Move variables consumed by `stmt.command`'s arguments.
        let mut consumed = Vec::new();
        match &stmt.command {
            Command::Bind { value, .. }
            | Command::Unpack { value, .. }
            | Command::Assign { value, .. }
            | Command::AssignPlace { value, .. } => {
                collect_consumed_vars(program, body, value, true, &mut consumed);
            }
            Command::Capture {
                operation,
                arguments,
                ..
            }
            | Command::Decide {
                op_kind: operation,
                arguments,
                ..
            } => match operation {
                Operation::Control(cid) => {
                    let callee = &program.controls[*cid];
                    for (arg, param) in arguments.iter().zip(&callee.parameters) {
                        let is_val = !matches!(param.ty, Type::Ref(_, _)) && !arg.lends();
                        collect_consumed_vars(program, body, arg, is_val, &mut consumed);
                    }
                }
                Operation::Builtin { op, .. } => {
                    for (i, arg) in arguments.iter().enumerate() {
                        let is_val = match op {
                            BuiltinOp::Push => i == 1,
                            BuiltinOp::Take => true,
                            BuiltinOp::Insert => i == 1 || i == 2,
                            _ => false,
                        };
                        collect_consumed_vars(program, body, arg, is_val, &mut consumed);
                    }
                }
                Operation::Outcome(_) => {
                    for arg in arguments {
                        collect_consumed_vars(program, body, arg, true, &mut consumed);
                    }
                }
                Operation::Variant { by_value, .. } => {
                    for arg in arguments {
                        collect_consumed_vars(program, body, arg, *by_value, &mut consumed);
                    }
                }
            },
            Command::Loop { initial, .. }
            | Command::Supervise {
                arguments: initial, ..
            }
            | Command::Again {
                arguments: initial, ..
            }
            | Command::Leave {
                payload: initial, ..
            }
            | Command::Exit {
                payload: initial, ..
            } => {
                for arg in initial {
                    collect_consumed_vars(program, body, arg, true, &mut consumed);
                }
            }
            Command::Boundary { captures, .. } => {
                for &cvar in captures {
                    if program.kind(&body.vars[cvar].ty) != Kind::Copy {
                        consumed.push(cvar);
                    }
                }
            }
            Command::Oblige(_)
            | Command::Test { .. }
            | Command::Fault { .. }
            | Command::Impossible
            | Command::Terminate(_)
            | Command::Escalate
            | Command::Recover { .. } => {}
        }
        for var in consumed {
            moved[var] = sel_or(moved[var].take(), g.clone());
        }

        // 2. Record availability `D(x) ∧ ¬M(x)` at this statement (after args moved).
        let mut snap = vec![None; body.vars.len()];
        for v in 0..body.vars.len() {
            if let Some(d) = &defined[v] {
                snap[v] = match &moved[v] {
                    None => Some(d.clone()),
                    Some(Selector::Top) => None,
                    Some(m) => Some(Selector::And(
                        Box::new(d.clone()),
                        Box::new(Selector::Not(Box::new(m.clone()))),
                    )),
                };
            }
        }
        body_avail.push(snap);

        // 3. Apply post-command bindings / assignments / nested bodies.
        match &stmt.command {
            Command::Bind { var, .. } | Command::Capture { var, .. } => {
                defined[*var] = sel_or(defined[*var].take(), g.clone());
            }
            Command::Unpack { vars, .. } => {
                for &var in vars {
                    defined[var] = sel_or(defined[var].take(), g.clone());
                }
            }
            Command::Assign { var, .. } => {
                moved[*var] = sel_and_not(moved[*var].take(), g);
            }
            Command::Decide { site, arms, .. } => {
                for (pos, arm) in arms.iter().enumerate() {
                    let atom = Selector::Atom(*site, pos);
                    for &var in &arm.vars {
                        defined[var] = sel_or(defined[var].take(), atom.clone());
                    }
                }
            }
            Command::Loop {
                site,
                body: inner,
                arms,
                ..
            } => {
                let _ = compute_body_availability(
                    program,
                    body,
                    *inner,
                    param_count,
                    defined.clone(),
                    moved.clone(),
                    avail_at,
                );
                for (pos, arm_vars) in arms.iter().enumerate() {
                    let atom = Selector::Atom(*site, pos);
                    for &var in arm_vars {
                        defined[var] = sel_or(defined[var].take(), atom.clone());
                    }
                }
            }
            Command::Boundary {
                body: b1,
                contingency_vars,
                message_var,
                contingency: b2,
                ..
            } => {
                let _ = compute_body_availability(
                    program,
                    body,
                    *b1,
                    param_count,
                    defined.clone(),
                    moved.clone(),
                    avail_at,
                );
                let mut b2_def = vec![None; body.vars.len()];
                let b2_mov = vec![None; body.vars.len()];
                for &cv in contingency_vars {
                    b2_def[cv] = Some(Selector::Top);
                }
                if let Some(mv) = message_var {
                    b2_def[*mv] = Some(Selector::Top);
                }
                let _ = compute_body_availability(
                    program,
                    body,
                    *b2,
                    param_count,
                    b2_def,
                    b2_mov,
                    avail_at,
                );
            }
            _ => {}
        }
    }

    avail_at[body_id] = body_avail;
    (defined, moved)
}

fn plan(program: &Program, body: &ControlBody, param_count: usize) -> Plan {
    let mut plan = Plan {
        bit: body
            .sites
            .iter()
            .map(|site| vec![None; site.alternatives.len()])
            .collect(),
        bits: 0,
        own: vec![Vec::new(); body.bodies.len()],
        avail_at: vec![Vec::new(); body.bodies.len()],
    };
    let defined = vec![None; body.vars.len()];
    let moved = vec![None; body.vars.len()];
    let _ = compute_body_availability(
        program,
        body,
        0,
        param_count,
        defined,
        moved,
        &mut plan.avail_at,
    );

    for inner in &body.bodies {
        for statement in &inner.statements {
            mention(&statement.selector, &mut plan);
        }
    }
    // [R3]: also allocate bits for any atom mentioned in a destruction /
    // fault-release availability formula of a variable that can hold a resource.
    let mut avail_selectors = Vec::new();
    for body_snaps in &plan.avail_at {
        for snap in body_snaps {
            for (var_id, sel) in snap.iter().enumerate() {
                if program.holds_obligation(&body.vars[var_id].ty)
                    && let Some(s) = sel
                {
                    avail_selectors.push(s.clone());
                }
            }
        }
    }
    for s in &avail_selectors {
        mention(s, &mut plan);
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
enum FrameKind {
    Normal,
    Loop {
        entry_parameters: Vec<Value>,
    },
    LexicalBoundary {
        site: SiteId,
        parent_body: BodyId,
        parent_pc: usize,
        captures: Vec<Value>,
        contingency_body: BodyId,
        contingency_vars: Vec<VarId>,
        message_var: Option<VarId>,
    },
    LexicalContingency {
        parent_body: BodyId,
        parent_pc: usize,
        fault_message: String,
    },
}

#[derive(Clone, Debug)]
struct BodyFrame {
    body: BodyId,
    kind: FrameKind,
}

#[derive(Clone, Debug)]
enum ActKind {
    Control(ControlId),
    Boundary {
        id: BoundaryId,
        in_contingency: Option<String>,
    },
}

/// One call of a control or named boundary.
struct Activation {
    kind: ActKind,
    slots: Vec<Value>,
    bits: Vec<bool>,
    positions: Vec<usize>,
    frames: Vec<BodyFrame>,
    writebacks: Vec<(VarId, EvalPlace)>,
}

struct Machine<'a> {
    program: &'a Program,
    config: &'a Config,
    arith: Arith<'a>,
    control_plans: &'a [Option<Plan>],
    boundary_plans: &'a [(Plan, Plan)],
    host: &'a mut dyn Host,
    stack: Vec<Activation>,
    trace: Vec<Event>,
}

impl<'a> Machine<'a> {
    fn top(&mut self) -> &mut Activation {
        self.stack.last_mut().expect("an activation")
    }

    fn current_body(&self) -> BodyId {
        self.stack
            .last()
            .expect("an activation")
            .frames
            .last()
            .expect("a frame")
            .body
    }

    fn laid_out(&self) -> (&'a ControlBody, &'a Plan) {
        let act = self.stack.last().expect("an activation");
        match &act.kind {
            ActKind::Control(cid) => (
                self.program.controls[*cid]
                    .body
                    .as_ref()
                    .expect("a defined control"),
                self.control_plans[*cid].as_ref().expect("a plan"),
            ),
            ActKind::Boundary { id, in_contingency } => {
                if in_contingency.is_some() {
                    (
                        &self.program.boundaries[*id].contingency,
                        &self.boundary_plans[*id].1,
                    )
                } else {
                    (
                        &self.program.boundaries[*id].body,
                        &self.boundary_plans[*id].0,
                    )
                }
            }
        }
    }

    fn push_control(
        &mut self,
        control: ControlId,
        arguments: &[Value],
        writebacks: Vec<(VarId, EvalPlace)>,
    ) {
        let body = self.program.controls[control]
            .body
            .as_ref()
            .expect("a defined control");
        let plan = self.control_plans[control].as_ref().expect("a plan");
        let mut slots = vec![Value::Int(Int::zero()); body.vars.len()];
        slots[..arguments.len()].clone_from_slice(arguments);
        self.stack.push(Activation {
            kind: ActKind::Control(control),
            slots,
            bits: vec![false; plan.bits],
            positions: vec![0; body.bodies.len()],
            frames: vec![BodyFrame {
                body: 0,
                kind: FrameKind::Normal,
            }],
            writebacks,
        });
    }

    fn eval_expr(&self, expr: &Expr) -> Result<Value, String> {
        let names = Names::Slots(&self.stack.last().expect("an activation").slots);
        self.arith.eval(expr, &names)
    }

    fn eval_place(&self, place: &Place) -> Result<EvalPlace, String> {
        let mut steps = Vec::with_capacity(place.steps.len());
        for step in &place.steps {
            match step {
                PlaceStep::Field(_, fidx, _) => steps.push(EvalStep::Field(*fidx)),
                PlaceStep::Index(idx_expr, _) => {
                    let v = self.eval_expr(idx_expr)?;
                    let idx = self.arith.word(&v, IntType::Usize) as usize;
                    steps.push(EvalStep::Index(idx));
                }
            }
        }
        Ok(EvalPlace {
            root: place.root,
            steps,
        })
    }

    fn read_eval_place(&self, place: &EvalPlace) -> Result<Value, String> {
        let slots = &self.stack.last().expect("an activation").slots;
        let mut current = &slots[place.root];
        for step in &place.steps {
            match step {
                EvalStep::Field(fidx) => {
                    let Value::Struct { fields, .. } = current else {
                        return Err("field access on non-struct".to_string());
                    };
                    current = &fields[*fidx];
                }
                EvalStep::Index(idx) => {
                    let Value::Vector(items) = current else {
                        return Err("index on non-vector".to_string());
                    };
                    let Some(elem) = items.get(*idx) else {
                        return Err("out-of-bounds index".to_string());
                    };
                    current = elem;
                }
            }
        }
        Ok(current.clone())
    }

    fn write_eval_place(&mut self, place: &EvalPlace, new_val: Value) -> Result<(), String> {
        let top = self.top();
        if place.steps.is_empty() {
            top.slots[place.root] = new_val;
            return Ok(());
        }
        let mut root_val = top.slots[place.root].clone();
        update_nested_erased(&mut root_val, &place.steps, new_val)?;
        top.slots[place.root] = root_val;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn eval_operands(
        &self,
        exprs: &[Expr],
        param_types: Option<&[Type]>,
    ) -> Result<(Vec<Value>, Vec<(VarId, EvalPlace)>), String> {
        let mut results: Vec<Option<Value>> = vec![None; exprs.len()];
        let mut writebacks = Vec::new();
        let is_lending = |i: usize, e: &Expr| -> bool {
            e.lends()
                || param_types
                    .and_then(|tys| tys.get(i))
                    .is_some_and(|ty| matches!(ty, Type::Ref(_, _)))
        };
        for (i, expr) in exprs.iter().enumerate() {
            if !is_lending(i, expr) {
                results[i] = Some(self.eval_expr(expr)?);
            }
        }
        for (i, expr) in exprs.iter().enumerate() {
            if is_lending(i, expr) {
                if let Expr::Borrow(BorrowMode::Exclusive, place, _) = expr {
                    let eval_place = self.eval_place(place)?;
                    let val = self.read_eval_place(&eval_place)?;
                    writebacks.push((i, eval_place));
                    results[i] = Some(val);
                } else {
                    results[i] = Some(self.eval_expr(expr)?);
                }
            }
        }
        Ok((
            results.into_iter().map(|v| v.expect("evaluated")).collect(),
            writebacks,
        ))
    }

    /// A new run of the body of a loop.
    fn enter(&mut self, target: BodyId, values: &[Value]) {
        let (body, plan) = self.laid_out();
        let head = body.bodies[target].head.as_ref().expect("a loop body");
        let top = self.top();
        for bit in &plan.own[target] {
            top.bits[*bit] = false;
        }
        for (var, val) in head.parameters.iter().zip(values) {
            top.slots[*var] = val.clone();
        }
        top.positions[target] = 0;
        top.frames.push(BodyFrame {
            body: target,
            kind: FrameKind::Loop {
                entry_parameters: values.to_vec(),
            },
        });
    }

    /// The waiting decision or capture completes.
    fn complete(&mut self, alternative: usize, payload: &[Value]) -> Result<(), String> {
        let (body, plan) = self.laid_out();
        let program = self.program;
        let current = self.current_body();
        let top = self.top();
        let statement = &body.bodies[current].statements[top.positions[current]];
        match &statement.command {
            Command::Decide { site, arms, .. } => {
                let Some(position) = arms.iter().position(|arm| arm.alternative == alternative)
                else {
                    return Err("an alternative that the decision does not list".to_string());
                };
                if let Some(bit) = plan.bit[*site][position] {
                    top.bits[bit] = true;
                }
                for (var, val) in arms[position].vars.iter().zip(payload) {
                    top.slots[*var] = val.clone();
                }
                top.positions[current] += 1;
                Ok(())
            }
            Command::Capture {
                var, outcome_id, ..
            } => {
                let alt_name = program.outcomes[*outcome_id].alternatives[alternative]
                    .name
                    .clone();
                top.slots[*var] = Value::Outcome {
                    ty: *outcome_id,
                    alternative,
                    alternative_name: alt_name,
                    payload: payload.to_vec(),
                };
                top.positions[current] += 1;
                Ok(())
            }
            _ => Err("an operation completes where no decision or capture waits".to_string()),
        }
    }

    /// [R4], [R5]: releases fault-releasable resources in `value`.
    fn release_value_resources(&mut self, value: &Value) {
        match value {
            Value::Resource { ty, name, .. } => {
                if self.program.resources[*ty].on_fault_release {
                    self.trace.push(Event {
                        control: format!("{name}.release"),
                        arguments: vec![value.clone()],
                        alternative: "released".to_string(),
                        payload: Vec::new(),
                    });
                }
            }
            Value::Outcome { payload, .. }
            | Value::Struct {
                fields: payload, ..
            }
            | Value::Variant {
                fields: payload, ..
            }
            | Value::Vector(payload) => {
                for item in payload {
                    self.release_value_resources(item);
                }
            }
            Value::Map(entries) => {
                for (k, v) in entries {
                    self.release_value_resources(k);
                    self.release_value_resources(v);
                }
            }
            Value::Bool(_) | Value::Int(_) | Value::String(_) => {}
        }
    }

    /// [R4], [R5]: unwinds one body frame using only static history-bit
    /// availability formulas (`plan.avail_at[frame_body][pc][var]`), without
    /// any runtime moved flags.
    fn unwind_frame(&mut self, frame_body: BodyId, exclude: &[VarId]) {
        let (body, plan) = self.laid_out();
        let act = self.stack.last().expect("an activation");
        let pc = act.positions[frame_body].min(plan.avail_at[frame_body].len().saturating_sub(1));
        let mut to_release = Vec::new();
        if let Some(snap) = plan.avail_at[frame_body].get(pc) {
            for (var_id, var) in body.vars.iter().enumerate().rev() {
                if var.body != frame_body || exclude.contains(&var_id) {
                    continue;
                }
                if !self.program.holds_obligation(&var.ty) {
                    continue;
                }
                if let Some(sel) = &snap[var_id]
                    && selects(sel, plan, &act.bits)
                {
                    to_release.push(act.slots[var_id].clone());
                }
            }
        }
        for val in to_release {
            self.release_value_resources(&val);
        }
    }

    fn enter_named_contingency(&mut self, id: BoundaryId, message: String) {
        let retains = self.program.boundaries[id].retains.clone();
        while self.stack.last().expect("an activation").frames.len() > 1 {
            let frame = self
                .stack
                .last_mut()
                .expect("an activation")
                .frames
                .pop()
                .unwrap();
            self.unwind_frame(frame.body, &[]);
        }
        let root_frame = self
            .stack
            .last_mut()
            .expect("an activation")
            .frames
            .pop()
            .unwrap();
        let retained_vals: Vec<Value> = retains
            .iter()
            .map(|&v| self.stack.last().unwrap().slots[v].clone())
            .collect();
        self.unwind_frame(root_frame.body, &retains);

        let b2_body = &self.program.boundaries[id].contingency;
        let b2_plan = &self.boundary_plans[id].1;
        let mut b2_slots = vec![Value::Int(Int::zero()); b2_body.vars.len()];
        for (i, val) in retained_vals.into_iter().enumerate() {
            b2_slots[i] = val;
        }
        b2_slots[retains.len()] = Value::String(message.clone());

        let act = self.top();
        act.kind = ActKind::Boundary {
            id,
            in_contingency: Some(message),
        };
        act.slots = b2_slots;
        act.bits = vec![false; b2_plan.bits];
        act.positions = vec![0; b2_body.bodies.len()];
        act.frames = vec![BodyFrame {
            body: 0,
            kind: FrameKind::Normal,
        }];
    }

    fn trigger_unwind(&mut self, message: String) -> Result<Option<End>, String> {
        loop {
            let is_named_b1 = matches!(
                self.stack.last().expect("an activation").kind,
                ActKind::Boundary {
                    in_contingency: None,
                    ..
                }
            );
            if is_named_b1 {
                let ActKind::Boundary { id, .. } = self.stack.last().expect("an activation").kind
                else {
                    unreachable!();
                };
                self.enter_named_contingency(id, message);
                return Ok(None);
            }

            if !self.stack.last().expect("an activation").frames.is_empty() {
                let frame = self
                    .stack
                    .last_mut()
                    .expect("an activation")
                    .frames
                    .pop()
                    .unwrap();
                self.unwind_frame(frame.body, &[]);
                if let FrameKind::LexicalBoundary {
                    parent_body,
                    parent_pc,
                    captures,
                    contingency_body,
                    contingency_vars,
                    message_var,
                    ..
                } = frame.kind
                {
                    let own_bits = self.laid_out().1.own[contingency_body].clone();
                    let top = self.top();
                    for bit in own_bits {
                        top.bits[bit] = false;
                    }
                    for (var, val) in contingency_vars.into_iter().zip(captures) {
                        top.slots[var] = val;
                    }
                    if let Some(mvar) = message_var {
                        top.slots[mvar] = Value::String(message.clone());
                    }
                    top.positions[contingency_body] = 0;
                    top.frames.push(BodyFrame {
                        body: contingency_body,
                        kind: FrameKind::LexicalContingency {
                            parent_body,
                            parent_pc,
                            fault_message: message,
                        },
                    });
                    return Ok(None);
                }
                continue;
            }

            let popped = self.stack.pop().expect("an activation");
            if self.stack.is_empty() {
                return Ok(Some(End::Faulted(message)));
            }
            for (param_var, caller_place) in &popped.writebacks {
                let _ = self.write_eval_place(caller_place, popped.slots[*param_var].clone());
            }
        }
    }

    fn execute_builtin(
        &mut self,
        op: BuiltinOp,
        target: Option<&Place>,
        arguments: &[Expr],
    ) -> Result<(usize, Vec<Value>), String> {
        let (mut vals, _) = self.eval_operands(arguments, None)?;
        match op {
            BuiltinOp::Push => {
                let place = self.eval_place(target.expect("push target"))?;
                let elem = vals.remove(1);
                let Value::Vector(mut items) = vals.remove(0) else {
                    return Err("`push` on non-vector".to_string());
                };
                items.push(elem);
                self.write_eval_place(&place, Value::Vector(items))?;
                Ok((0, Vec::new()))
            }
            BuiltinOp::Pop => {
                let place = self.eval_place(target.expect("pop target"))?;
                let Value::Vector(mut items) = vals.remove(0) else {
                    return Err("`pop` on non-vector".to_string());
                };
                if let Some(last) = items.pop() {
                    self.write_eval_place(&place, Value::Vector(items))?;
                    Ok((0, vec![last]))
                } else {
                    Ok((1, Vec::new()))
                }
            }
            BuiltinOp::Take => {
                let Value::Vector(mut items) = vals.remove(0) else {
                    return Err("`take` on non-vector".to_string());
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
                    return Err("`get` on non-map".to_string());
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
                    return Err("`insert` on non-map".to_string());
                };
                let prev = entries.insert(key, value);
                self.write_eval_place(&place, Value::Map(entries))?;
                match prev {
                    None => Ok((0, Vec::new())),
                    Some(old) => Ok((1, vec![old])),
                }
            }
            BuiltinOp::Remove => {
                let place = self.eval_place(target.expect("remove target"))?;
                let key = vals.remove(1);
                let Value::Map(mut entries) = vals.remove(0) else {
                    return Err("`remove` on non-map".to_string());
                };
                let prev = entries.remove(&key);
                self.write_eval_place(&place, Value::Map(entries))?;
                match prev {
                    Some(old) => Ok((0, vec![old])),
                    None => Ok((1, Vec::new())),
                }
            }
            BuiltinOp::EntryAt => {
                let pos = self.arith.word(&vals.remove(1), IntType::Usize) as usize;
                let Value::Map(entries) = vals.remove(0) else {
                    return Err("`entry_at` on non-map".to_string());
                };
                if pos >= entries.len() {
                    return Ok((1, Vec::new()));
                }
                let (k, v) = entries.iter().nth(pos).unwrap();
                let next = self.arith.int_value((pos + 1) as u64, IntType::Usize);
                Ok((0, vec![k.clone(), v.clone(), next]))
            }
            BuiltinOp::Emit => {
                let text = vals.remove(0);
                self.trace.push(Event {
                    control: "emit".to_string(),
                    arguments: vec![text],
                    alternative: "done".to_string(),
                    payload: Vec::new(),
                });
                Ok((0, Vec::new()))
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn execute_operation(
        &mut self,
        op_kind: &Operation,
        arguments: &[Expr],
    ) -> Result<Option<Result<(usize, Vec<Value>), End>>, String> {
        let program = self.program;
        let arith = self.arith;
        match op_kind {
            Operation::Control(callee_id) => {
                let callee = &program.controls[*callee_id];
                let param_types: Vec<Type> =
                    callee.parameters.iter().map(|p| p.ty.clone()).collect();
                let (values, writebacks) = self.eval_operands(arguments, Some(&param_types))?;
                if callee.body.is_some() {
                    self.push_control(*callee_id, &values, writebacks);
                    return Ok(None);
                }
                let answer = match self.host.call(callee, &values, self.config) {
                    Ok(answer) => answer,
                    Err(detail) => return Ok(Some(Err(End::HostFailed(detail)))),
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
                let payload: Vec<Value> = answer
                    .payload
                    .iter()
                    .zip(fields)
                    .map(|(value, field)| arith.normalize(value, &field.ty))
                    .collect();
                if !callee.is_pure() {
                    self.trace.push(Event {
                        control: callee.name.clone(),
                        arguments: values,
                        alternative: answer.alternative,
                        payload: payload.clone(),
                    });
                }
                Ok(Some(Ok((index, payload))))
            }
            Operation::Builtin { op, target, .. } => {
                let (alt, payload) = self.execute_builtin(*op, target.as_ref(), arguments)?;
                Ok(Some(Ok((alt, payload))))
            }
            Operation::Outcome(_) => {
                let val = self.eval_expr(&arguments[0])?;
                let Value::Outcome {
                    alternative,
                    payload,
                    ..
                } = val
                else {
                    return Err("`outcome` on non-outcome value".to_string());
                };
                Ok(Some(Ok((alternative, payload))))
            }
            Operation::Variant { .. } => {
                let val = self.eval_expr(&arguments[0])?;
                let Value::Variant {
                    constructor,
                    fields,
                    ..
                } = val
                else {
                    return Err("variant decision on non-variant value".to_string());
                };
                Ok(Some(Ok((constructor, fields))))
            }
        }
    }

    fn step(&mut self) -> Result<Option<End>, String> {
        let (body, plan) = self.laid_out();
        let program = self.program;
        let current = self.current_body();
        let position = self.stack.last().expect("an activation").positions[current];
        let Some(statement) = body.bodies[current].statements.get(position) else {
            let last_kind = self
                .stack
                .last()
                .expect("an activation")
                .frames
                .last()
                .expect("a frame")
                .kind
                .clone();
            if let FrameKind::LexicalBoundary {
                site,
                parent_body,
                parent_pc,
                ..
            } = last_kind
            {
                let top = self.top();
                top.frames.pop();
                if let Some(bit) = plan.bit[site][0] {
                    top.bits[bit] = true;
                }
                top.positions[parent_body] = parent_pc + 1;
                return Ok(None);
            }
            return Err("the end of a body".to_string());
        };
        if !selects(
            &statement.selector,
            plan,
            &self.stack.last().expect("an activation").bits,
        ) {
            self.top().positions[current] += 1;
            return Ok(None);
        }
        let command = statement.command.clone();
        match &command {
            Command::Bind { var, value } | Command::Assign { var, value } => {
                let val = self.eval_expr(value)?;
                let top = self.top();
                top.slots[*var] = val;
                top.positions[current] += 1;
            }
            Command::Unpack { vars, value, .. } => {
                let Value::Struct { fields, .. } = self.eval_expr(value)? else {
                    return Err("unpacking non-struct".to_string());
                };
                let top = self.top();
                for (var, fval) in vars.iter().zip(fields) {
                    top.slots[*var] = fval;
                }
                top.positions[current] += 1;
            }
            Command::Capture {
                operation,
                arguments,
                ..
            } => {
                if let Some(res) = self.execute_operation(operation, arguments)? {
                    match res {
                        Err(end) => return Ok(Some(end)),
                        Ok((alt, payload)) => self.complete(alt, &payload)?,
                    }
                }
            }
            Command::AssignPlace { target, value } => {
                let val = self.eval_expr(value)?;
                let eval_place = self.eval_place(target)?;
                self.write_eval_place(&eval_place, val)?;
                self.top().positions[current] += 1;
            }
            // [R2]: not evaluated.
            Command::Oblige(_) => self.top().positions[current] += 1,
            Command::Test { site, condition } => {
                let val = self.eval_expr(condition)?;
                let alternative = if matches!(val, Value::Bool(true)) {
                    0
                } else {
                    1
                };
                let top = self.top();
                if let Some(bit) = plan.bit[*site][alternative] {
                    top.bits[bit] = true;
                }
                top.positions[current] += 1;
            }
            Command::Decide {
                op_kind, arguments, ..
            } => {
                if let Some(res) = self.execute_operation(op_kind, arguments)? {
                    match res {
                        Err(end) => return Ok(Some(end)),
                        Ok((alt, payload)) => self.complete(alt, &payload)?,
                    }
                }
            }
            Command::Loop {
                body: inner,
                initial,
                ..
            } => {
                let (values, _) = self.eval_operands(initial, None)?;
                self.enter(*inner, &values);
            }
            Command::Boundary {
                site,
                body: b1,
                captures,
                contingency_vars,
                message_var,
                contingency: b2,
            } => {
                let cap_vals: Vec<Value> = captures
                    .iter()
                    .map(|&v| self.stack.last().unwrap().slots[v].clone())
                    .collect();
                let own_bits = plan.own[*b1].clone();
                let top = self.top();
                for bit in own_bits {
                    top.bits[bit] = false;
                }
                top.positions[*b1] = 0;
                top.frames.push(BodyFrame {
                    body: *b1,
                    kind: FrameKind::LexicalBoundary {
                        site: *site,
                        parent_body: current,
                        parent_pc: position,
                        captures: cap_vals,
                        contingency_body: *b2,
                        contingency_vars: contingency_vars.clone(),
                        message_var: *message_var,
                    },
                });
            }
            Command::Supervise {
                boundary,
                arguments,
            } => {
                let (values, _) = self.eval_operands(arguments, None)?;
                let b1_body = &program.boundaries[*boundary].body;
                let b1_plan = &self.boundary_plans[*boundary].0;
                let mut slots = vec![Value::Int(Int::zero()); b1_body.vars.len()];
                slots[..values.len()].clone_from_slice(&values);
                self.stack.push(Activation {
                    kind: ActKind::Boundary {
                        id: *boundary,
                        in_contingency: None,
                    },
                    slots,
                    bits: vec![false; b1_plan.bits],
                    positions: vec![0; b1_body.bodies.len()],
                    frames: vec![BodyFrame {
                        body: 0,
                        kind: FrameKind::Normal,
                    }],
                    writebacks: Vec::new(),
                });
            }
            Command::Again {
                body: target,
                arguments,
            } => {
                let (values, _) = self.eval_operands(arguments, None)?;
                while let Some(frame) = self.top().frames.pop() {
                    if frame.body == *target {
                        break;
                    }
                }
                self.enter(*target, &values);
            }
            Command::Leave {
                alternative,
                payload,
            } => {
                let (values, _) = self.eval_operands(payload, None)?;
                while let Some(frame) = self.top().frames.pop() {
                    if matches!(frame.kind, FrameKind::Loop { .. }) {
                        break;
                    }
                }
                let parent = self.current_body();
                let top = self.top();
                let statement = &body.bodies[parent].statements[top.positions[parent]];
                let Command::Loop { site, arms, .. } = &statement.command else {
                    return Err("a loop is left where no loop statement waits".to_string());
                };
                if let Some(bit) = plan.bit[*site][*alternative] {
                    top.bits[bit] = true;
                }
                for (var, val) in arms[*alternative].iter().zip(&values) {
                    top.slots[*var] = val.clone();
                }
                top.positions[parent] += 1;
            }
            Command::Exit {
                alternative,
                payload,
            } => {
                let (values, _) = self.eval_operands(payload, None)?;
                let popped = self.stack.pop().expect("an activation");
                if self.stack.is_empty() {
                    let ActKind::Control(control_id) = popped.kind else {
                        return Err("boundary exited as control".to_string());
                    };
                    let entry = &program.controls[control_id].contract[*alternative];
                    return Ok(Some(End::Done {
                        alternative: entry.name.clone(),
                        payload: values,
                    }));
                }
                for (param_var, caller_place) in &popped.writebacks {
                    self.write_eval_place(caller_place, popped.slots[*param_var].clone())?;
                }
                self.complete(*alternative, &values)?;
            }
            Command::Fault { message, .. } => {
                let Value::String(msg) = self.eval_expr(message)? else {
                    return Err("fault message is not a string".to_string());
                };
                if let Some(FrameKind::LexicalContingency {
                    parent_body,
                    parent_pc,
                    ..
                }) = self
                    .stack
                    .last()
                    .and_then(|a| a.frames.last())
                    .map(|f| &f.kind)
                {
                    let (parent_body, parent_pc) = (*parent_body, *parent_pc);
                    let frame = self.top().frames.pop().unwrap();
                    self.unwind_frame(frame.body, &[]);
                    self.top().positions[parent_body] = parent_pc + 1;
                    return self.trigger_unwind(msg);
                }
                if matches!(
                    self.stack.last().expect("an activation").kind,
                    ActKind::Boundary {
                        in_contingency: Some(_),
                        ..
                    }
                ) {
                    while let Some(frame) = self.top().frames.pop() {
                        self.unwind_frame(frame.body, &[]);
                    }
                    let _ = self.stack.pop();
                    if self.stack.is_empty() {
                        return Ok(Some(End::Faulted(msg)));
                    }
                    return self.trigger_unwind(msg);
                }
                return self.trigger_unwind(msg);
            }
            // [R2]: no code is generated for it.
            Command::Impossible => return Err("`⊥` has no behavior".to_string()),
            Command::Terminate(status) => {
                let status_val = match status {
                    Some(expr) => Some(self.eval_expr(expr)?),
                    None => None,
                };
                if let Some(FrameKind::LexicalContingency {
                    parent_body,
                    parent_pc,
                    ..
                }) = self
                    .stack
                    .last()
                    .and_then(|a| a.frames.last())
                    .map(|f| &f.kind)
                {
                    let (parent_body, parent_pc) = (*parent_body, *parent_pc);
                    self.top().frames.pop();
                    if let Some(v) = status_val {
                        self.stack.pop();
                        return Ok(Some(End::Done {
                            alternative: "terminated".to_string(),
                            payload: vec![v],
                        }));
                    }
                    self.top().positions[parent_body] = parent_pc + 1;
                    return Ok(None);
                }
                self.stack.pop();
                let caller_body = self.current_body();
                self.top().positions[caller_body] += 1;
            }
            Command::Escalate => {
                if let Some(FrameKind::LexicalContingency {
                    parent_body,
                    parent_pc,
                    fault_message,
                }) = self
                    .stack
                    .last()
                    .and_then(|a| a.frames.last())
                    .map(|f| f.kind.clone())
                {
                    let frame = self.top().frames.pop().unwrap();
                    self.unwind_frame(frame.body, &[]);
                    self.top().positions[parent_body] = parent_pc + 1;
                    return self.trigger_unwind(fault_message);
                }
                let ActKind::Boundary {
                    in_contingency: Some(msg),
                    ..
                } = self.stack.last().expect("an activation").kind.clone()
                else {
                    return Err("`escalate` outside contingency".to_string());
                };
                while let Some(frame) = self.top().frames.pop() {
                    self.unwind_frame(frame.body, &[]);
                }
                self.stack.pop();
                if self.stack.is_empty() {
                    return Ok(Some(End::Faulted(msg)));
                }
                return self.trigger_unwind(msg);
            }
            Command::Recover { body: target_loop } => {
                let mut entry_params = None;
                while let Some(frame) = self.top().frames.pop() {
                    if frame.body == *target_loop {
                        if let FrameKind::Loop { entry_parameters } = frame.kind {
                            entry_params = Some(entry_parameters);
                        }
                        break;
                    }
                }
                let Some(params) = entry_params else {
                    return Err("`recover` target loop not found".to_string());
                };
                self.enter(*target_loop, &params);
            }
        }
        Ok(None)
    }
}

fn update_nested_erased(
    current: &mut Value,
    steps: &[EvalStep],
    new_val: Value,
) -> Result<(), String> {
    let (first, rest) = steps.split_first().expect("non-empty steps");
    match first {
        EvalStep::Field(fidx) => {
            let Value::Struct { fields, .. } = current else {
                return Err("field assignment on non-struct".to_string());
            };
            if rest.is_empty() {
                fields[*fidx] = new_val;
                Ok(())
            } else {
                update_nested_erased(&mut fields[*fidx], rest, new_val)
            }
        }
        EvalStep::Index(idx) => {
            let Value::Vector(items) = current else {
                return Err("index assignment on non-vector".to_string());
            };
            let Some(elem) = items.get_mut(*idx) else {
                return Err("out-of-bounds index assignment".to_string());
            };
            if rest.is_empty() {
                *elem = new_val;
                Ok(())
            } else {
                update_nested_erased(elem, rest, new_val)
            }
        }
    }
}

/// Runs the control `entry` on `arguments` under the erased semantics.
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
    let control_plans: Vec<Option<Plan>> = program
        .controls
        .iter()
        .map(|c| {
            c.body
                .as_ref()
                .map(|b| plan(program, b, c.parameters.len()))
        })
        .collect();
    let boundary_plans: Vec<(Plan, Plan)> = program
        .boundaries
        .iter()
        .map(|b| {
            (
                plan(program, &b.body, b.parameters.len()),
                plan(program, &b.contingency, b.retains.len() + 1),
            )
        })
        .collect();
    let arith = Arith {
        program,
        usize_bits,
    };
    let normalized_args: Vec<Value> = arguments
        .iter()
        .zip(&control.parameters)
        .map(|(value, parameter)| arith.normalize(value, &parameter.ty))
        .collect();
    let mut machine = Machine {
        program,
        config,
        arith,
        control_plans: &control_plans,
        boundary_plans: &boundary_plans,
        host,
        stack: Vec::new(),
        trace: Vec::new(),
    };
    machine.push_control(id, &normalized_args, Vec::new());
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
