//! The phase 3 checker (`docs/v2/core.md`, section 7; `docs/v2/facts.md`).
//!
//! For each function and defined control of a [`typed::Program`], [`check`]
//! verifies:
//!
//! - **[K1] to [K8]** Live histories, structure `Ω`, loop post-iteration facts
//!   and coverage (`Θ ⊨ ¬Lₙ₊₁`, [K7]).
//! - **[N1] to [N5]** Conditional facts `Ψ`, SSA renaming on mutation and loop
//!   entry, and exact entailment backed by the independent certificate kernel
//!   ([`crate::kernel::validate`], [V1]).
//! - **[C2]** Decision acknowledgement (every alternative of the operation's
//!   contract is listed).
//! - **[O1], [O2], [O4], [O6]** Ownership and availability (`D(x)` on
//!   histories: no double bind [O1], defined at read [O2] and assignment [O4],
//!   outer names across loops [O6]).
//! - **[P1] to [P8]** Proof obligations: `⊢ φ` ([P1]), loop invariants at
//!   entry and `↑` ([P2]), preconditions of functions and controls ([P3]),
//!   contract exit conditions `⇐ φ` ([P4]), integer arithmetic bounds and
//!   non-zero divisors ([P5]), unreachable `⊥` ([P6]), left-to-right and
//!   short-circuit order ([P7]), and function bodies ([P8]).
//! - **[F3]** Effects within the control's `uses` clause.

use crate::bdd::{self, Formula};
use crate::facts::{AtomTerm, Certificate, Counterexample, Decision, LinearTerm, Solver, TermId};
use crate::int::Int;
use crate::kernel;
use crate::typed::{
    Arithmetic, BodyId, Command, Comparison, Control, ControlBody, ControlId, Expr, Function,
    FunctionId, IntType, Program, Selector, SiteId, Type, VarId, VarRef,
};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug)]
pub struct Error {
    pub control: String,
    pub line: u32,
    pub clause: &'static str,
    pub message: String,
    pub counterexample: Option<Box<Counterexample>>,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: {} [{}]",
            self.control, self.line, self.message, self.clause
        )?;
        if let Some(cex) = &self.counterexample {
            write!(formatter, " — {cex}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CheckedProgram {
    pub certificates: Vec<(String, u32, &'static str, Certificate)>,
    pub warnings: Vec<crate::surface::Warning>,
}

/// Checks every function and defined control of `program`.
pub fn check(program: &Program) -> Result<CheckedProgram, Error> {
    let mut certificates = Vec::new();
    let mut warnings = Vec::new();
    for (id, function) in program.functions.iter().enumerate() {
        check_function(program, id, function, &mut certificates)?;
    }
    for (id, control) in program.controls.iter().enumerate() {
        if control.body.is_some() {
            check_control(program, id, control, &mut certificates, &mut warnings)?;
        }
    }
    Ok(CheckedProgram {
        certificates,
        warnings,
    })
}

#[derive(Clone)]
struct Env {
    locals: HashMap<VarId, LinearTerm>,
    parameters: Vec<LinearTerm>,
    fields: Vec<LinearTerm>,
    constants: HashMap<VarId, Int>,
}

impl Env {
    fn lookup(&self, var: VarRef) -> LinearTerm {
        match var {
            VarRef::Local(id) => self
                .locals
                .get(&id)
                .cloned()
                .unwrap_or_else(|| LinearTerm::from_i128(0)),
            VarRef::Parameter(idx) => self
                .parameters
                .get(idx)
                .cloned()
                .unwrap_or_else(|| LinearTerm::from_i128(0)),
            VarRef::Field(idx) => self
                .fields
                .get(idx)
                .cloned()
                .unwrap_or_else(|| LinearTerm::from_i128(0)),
        }
    }
}

/// Unfolds non-recursive function calls in `expr` ([I8]).
fn unfold_functions(program: &Program, expr: &Expr) -> Expr {
    match expr {
        Expr::Boolean(_) | Expr::Integer(_, _) | Expr::Var(_, _) => expr.clone(),
        Expr::Negate(inner, ty) => Expr::Negate(Box::new(unfold_functions(program, inner)), *ty),
        Expr::Not(inner) => Expr::Not(Box::new(unfold_functions(program, inner))),
        Expr::Arithmetic(op, l, r, ty) => Expr::Arithmetic(
            *op,
            Box::new(unfold_functions(program, l)),
            Box::new(unfold_functions(program, r)),
            *ty,
        ),
        Expr::Compare(cmp, l, r) => Expr::Compare(
            *cmp,
            Box::new(unfold_functions(program, l)),
            Box::new(unfold_functions(program, r)),
        ),
        Expr::And(l, r) => Expr::And(
            Box::new(unfold_functions(program, l)),
            Box::new(unfold_functions(program, r)),
        ),
        Expr::Or(l, r) => Expr::Or(
            Box::new(unfold_functions(program, l)),
            Box::new(unfold_functions(program, r)),
        ),
        Expr::Call(fn_id, args, _) => {
            let unfolded_args: Vec<Expr> =
                args.iter().map(|a| unfold_functions(program, a)).collect();
            let body = subst_parameters(&program.functions[*fn_id].body, &unfolded_args);
            unfold_functions(program, &body)
        }
    }
}

fn subst_parameters(expr: &Expr, args: &[Expr]) -> Expr {
    match expr {
        Expr::Boolean(_) | Expr::Integer(_, _) => expr.clone(),
        Expr::Var(VarRef::Parameter(idx), _) => args[*idx].clone(),
        Expr::Var(_, _) => expr.clone(),
        Expr::Negate(inner, ty) => Expr::Negate(Box::new(subst_parameters(inner, args)), *ty),
        Expr::Not(inner) => Expr::Not(Box::new(subst_parameters(inner, args))),
        Expr::Arithmetic(op, l, r, ty) => Expr::Arithmetic(
            *op,
            Box::new(subst_parameters(l, args)),
            Box::new(subst_parameters(r, args)),
            *ty,
        ),
        Expr::Compare(cmp, l, r) => Expr::Compare(
            *cmp,
            Box::new(subst_parameters(l, args)),
            Box::new(subst_parameters(r, args)),
        ),
        Expr::And(l, r) => Expr::And(
            Box::new(subst_parameters(l, args)),
            Box::new(subst_parameters(r, args)),
        ),
        Expr::Or(l, r) => Expr::Or(
            Box::new(subst_parameters(l, args)),
            Box::new(subst_parameters(r, args)),
        ),
        Expr::Call(id, call_args, ty) => Expr::Call(
            *id,
            call_args
                .iter()
                .map(|a| subst_parameters(a, args))
                .collect(),
            *ty,
        ),
    }
}

/// Evaluates a constant integer expression ([I3]).
fn eval_const(program: &Program, expr: &Expr, constants: &HashMap<VarId, Int>) -> Option<Int> {
    let unfolded = unfold_functions(program, expr);
    eval_const_unfolded(&unfolded, constants)
}

fn eval_const_unfolded(expr: &Expr, constants: &HashMap<VarId, Int>) -> Option<Int> {
    match expr {
        Expr::Integer(n, _) => Some(n.clone()),
        Expr::Var(VarRef::Local(id), _) => constants.get(id).cloned(),
        Expr::Negate(inner, _) => Some(eval_const_unfolded(inner, constants)?.negate()),
        Expr::Arithmetic(op, l, r, _) => {
            let a = eval_const_unfolded(l, constants)?;
            let b = eval_const_unfolded(r, constants)?;
            match op {
                Arithmetic::Add => Some(a.plus(&b)),
                Arithmetic::Subtract => Some(a.minus(&b)),
                Arithmetic::Multiply => Some(a.times(&b)),
                Arithmetic::Divide => Some(a.divide(&b)?.0),
                Arithmetic::Remainder => Some(a.divide(&b)?.1),
            }
        }
        _ => None,
    }
}

/// Computes the declared interval `[low, high]` of an unfolded integer
/// expression for `[A9]`.
fn interval_unfolded(expr: &Expr, constants: &HashMap<VarId, Int>) -> (Option<Int>, Option<Int>) {
    if let Some(c) = eval_const_unfolded(expr, constants) {
        return (Some(c.clone()), Some(c));
    }
    match expr {
        Expr::Integer(n, _) => (Some(n.clone()), Some(n.clone())),
        Expr::Var(_, Type::Int(ty)) => {
            if *ty == IntType::Usize {
                (Some(Int::zero()), None)
            } else {
                let (lo, hi) = ty.literal_bounds();
                (Some(lo), Some(hi))
            }
        }
        Expr::Negate(inner, _) => {
            let (lo, hi) = interval_unfolded(inner, constants);
            (hi.map(|h| h.negate()), lo.map(|l| l.negate()))
        }
        Expr::Arithmetic(Arithmetic::Add, l, r, _) => {
            let (l1, h1) = interval_unfolded(l, constants);
            let (l2, h2) = interval_unfolded(r, constants);
            (
                l1.zip(l2).map(|(a, b)| a.plus(&b)),
                h1.zip(h2).map(|(a, b)| a.plus(&b)),
            )
        }
        Expr::Arithmetic(Arithmetic::Subtract, l, r, _) => {
            let (l1, h1) = interval_unfolded(l, constants);
            let (l2, h2) = interval_unfolded(r, constants);
            (
                l1.zip(h2).map(|(a, b)| a.minus(&b)),
                h1.zip(l2).map(|(a, b)| a.minus(&b)),
            )
        }
        Expr::Arithmetic(Arithmetic::Multiply, l, r, _) => {
            let (l1, h1) = interval_unfolded(l, constants);
            let (l2, h2) = interval_unfolded(r, constants);
            if let (Some(a), Some(big_a), Some(b), Some(big_b)) = (l1, h1, l2, h2) {
                let p1 = a.times(&b);
                let p2 = a.times(&big_b);
                let p3 = big_a.times(&b);
                let p4 = big_a.times(&big_b);
                let min = p1.clone().min(p2.clone()).min(p3.clone()).min(p4.clone());
                let max = p1.max(p2).max(p3).max(p4);
                (Some(min), Some(max))
            } else {
                (None, None)
            }
        }
        Expr::Arithmetic(Arithmetic::Divide, l, r, _) => {
            if let Some(k) = eval_const_unfolded(r, constants)
                && !k.is_zero()
            {
                let (l1, h1) = interval_unfolded(l, constants);
                if !k.is_negative() {
                    (
                        l1.map(|a| a.divide(&k).unwrap().0),
                        h1.map(|b| b.divide(&k).unwrap().0),
                    )
                } else {
                    (
                        h1.map(|b| b.divide(&k).unwrap().0),
                        l1.map(|a| a.divide(&k).unwrap().0),
                    )
                }
            } else {
                (None, None)
            }
        }
        Expr::Arithmetic(Arithmetic::Remainder, l, r, _) => {
            if let Some(k) = eval_const_unfolded(r, constants)
                && !k.is_zero()
            {
                let (l1, h1) = interval_unfolded(l, constants);
                let abs_k = if k.is_negative() { k.negate() } else { k };
                let max_rem = abs_k.minus(&Int::from_u64(1));
                let lo = if l1.as_ref().is_some_and(|a| !a.is_negative()) {
                    Int::zero()
                } else {
                    max_rem.negate()
                };
                let hi = if h1.as_ref().is_some_and(|b| *b <= Int::zero()) {
                    Int::zero()
                } else {
                    max_rem
                };
                (Some(lo), Some(hi))
            } else {
                (None, None)
            }
        }
        _ => (None, None),
    }
}

/// Translates `expr` into a [`LinearTerm`] in `solver` ([I1] to [I13]).
fn translate_term(program: &Program, solver: &mut Solver, expr: &Expr, env: &Env) -> LinearTerm {
    let unfolded = unfold_functions(program, expr);
    if let Some(c) = eval_const_unfolded(&unfolded, &env.constants) {
        return LinearTerm::constant(c);
    }
    match &unfolded {
        Expr::Boolean(b) => LinearTerm::from_i128(i128::from(*b)),
        Expr::Integer(n, _) => LinearTerm::constant(n.clone()),
        Expr::Var(vr, _) => env.lookup(*vr),
        Expr::Negate(inner, _) => translate_term(program, solver, inner, env).negate(),
        Expr::Arithmetic(Arithmetic::Add, l, r, _) => {
            let tl = translate_term(program, solver, l, env);
            let tr = translate_term(program, solver, r, env);
            tl.add(&tr)
        }
        Expr::Arithmetic(Arithmetic::Subtract, l, r, _) => {
            let tl = translate_term(program, solver, l, env);
            let tr = translate_term(program, solver, r, env);
            tl.sub(&tr)
        }
        Expr::Arithmetic(Arithmetic::Multiply, l, r, _) => {
            if let Some(k) = eval_const_unfolded(l, &env.constants) {
                return translate_term(program, solver, r, env).scale(&k);
            }
            if let Some(k) = eval_const_unfolded(r, &env.constants) {
                return translate_term(program, solver, l, env).scale(&k);
            }
            let tl = translate_term(program, solver, l, env);
            let tr = translate_term(program, solver, r, env);
            let bl = interval_unfolded(l, &env.constants);
            let br = interval_unfolded(r, &env.constants);
            let ((left, left_bounds), (right, right_bounds)) = if tl <= tr {
                ((tl, bl), (tr, br))
            } else {
                ((tr, br), (tl, bl))
            };
            let id = solver.intern_term(AtomTerm::Mul {
                left,
                right,
                left_bounds,
                right_bounds,
            });
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Arithmetic(Arithmetic::Divide, l, r, _) => {
            let tl = translate_term(program, solver, l, env);
            if let Some(k) = eval_const_unfolded(r, &env.constants)
                && !k.is_zero()
            {
                let abs_k = if k.is_negative() {
                    k.negate()
                } else {
                    k.clone()
                };
                if abs_k == Int::from_u64(1) {
                    return if k.is_negative() { tl.negate() } else { tl };
                }
                let id = solver.intern_term(AtomTerm::DivConst(tl, abs_k));
                solver.instantiate_all();
                let q = LinearTerm::var(id);
                return if k.is_negative() { q.negate() } else { q };
            }
            let tr = translate_term(program, solver, r, env);
            let id = solver.intern_term(AtomTerm::Div(tl, tr));
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Arithmetic(Arithmetic::Remainder, l, r, _) => {
            let tl = translate_term(program, solver, l, env);
            if let Some(k) = eval_const_unfolded(r, &env.constants)
                && !k.is_zero()
            {
                let abs_k = if k.is_negative() { k.negate() } else { k };
                if abs_k == Int::from_u64(1) {
                    return LinearTerm::from_i128(0);
                }
                let id = solver.intern_term(AtomTerm::DivConst(tl.clone(), abs_k.clone()));
                solver.instantiate_all();
                let q = LinearTerm::var(id);
                return tl.sub(&q.scale(&abs_k));
            }
            let tr = translate_term(program, solver, r, env);
            let id = solver.intern_term(AtomTerm::Rem(tl, tr));
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        // [I13]: A Boolean formula used where a value term is needed.
        _ => {
            let phi = translate_formula(program, solver, &unfolded, env);
            let b_id = solver.fresh_var("_bool".to_string(), None, false);
            let b_term = LinearTerm::var(b_id);
            let ge_0 = solver.le(&LinearTerm::from_i128(0), &b_term);
            let le_1 = solver.le(&b_term, &LinearTerm::from_i128(1));
            let range = solver.bdd.and(ge_0, le_1);
            solver.add_fact(bdd::TRUE, range);
            solver.add_def_bool(bdd::TRUE, b_id, phi);
            b_term
        }
    }
}

/// Translates a Boolean expression or formula into a BDD [`Formula`] ([S4],
/// [I6], [I8]).
fn translate_formula(program: &Program, solver: &mut Solver, expr: &Expr, env: &Env) -> Formula {
    let unfolded = unfold_functions(program, expr);
    match &unfolded {
        Expr::Boolean(true) => bdd::TRUE,
        Expr::Boolean(false) => bdd::FALSE,
        Expr::Var(vr, _) => {
            let term = env.lookup(*vr);
            solver.le(&LinearTerm::from_i128(1), &term)
        }
        Expr::Not(inner) => {
            let f = translate_formula(program, solver, inner, env);
            solver.bdd.not(f)
        }
        Expr::And(l, r) => {
            let fl = translate_formula(program, solver, l, env);
            let fr = translate_formula(program, solver, r, env);
            solver.bdd.and(fl, fr)
        }
        Expr::Or(l, r) => {
            let fl = translate_formula(program, solver, l, env);
            let fr = translate_formula(program, solver, r, env);
            solver.bdd.or(fl, fr)
        }
        Expr::Compare(cmp, l, r) => {
            if l.ty() == Type::Bool {
                let fl = translate_formula(program, solver, l, env);
                let fr = translate_formula(program, solver, r, env);
                let iff = solver.bdd.iff(fl, fr);
                match cmp {
                    Comparison::Equal => iff,
                    Comparison::NotEqual => solver.bdd.not(iff),
                    _ => bdd::FALSE,
                }
            } else {
                let tl = translate_term(program, solver, l, env);
                let tr = translate_term(program, solver, r, env);
                match cmp {
                    Comparison::Equal => solver.eq(&tl, &tr),
                    Comparison::NotEqual => solver.ne(&tl, &tr),
                    Comparison::Less => solver.lt(&tl, &tr),
                    Comparison::LessEqual => solver.le(&tl, &tr),
                    Comparison::Greater => solver.lt(&tr, &tl),
                    Comparison::GreaterEqual => solver.le(&tr, &tl),
                }
            }
        }
        _ => bdd::FALSE,
    }
}

/// Proves that `negated_goal` is unsatisfiable under `solver`, validates the
/// resulting [`Certificate`] in [`kernel::validate`] ([V1]), and records it.
#[allow(clippy::too_many_arguments)]
fn require_entailment(
    solver: &mut Solver,
    negated_goal: Formula,
    control: &str,
    line: u32,
    clause: &'static str,
    message: impl Into<String>,
    visible_vars: &[(String, TermId, bool)],
    in_loop: bool,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
) -> Result<(), Error> {
    match solver.decide(negated_goal, visible_vars, in_loop) {
        Decision::Proved(cert) => {
            if let Err(kernel_err) = kernel::validate(&cert) {
                return Err(Error {
                    control: control.to_string(),
                    line,
                    clause: "V1",
                    message: format!("certificate rejected by the kernel: {kernel_err}"),
                    counterexample: None,
                });
            }
            certificates.push((control.to_string(), line, clause, cert));
            Ok(())
        }
        Decision::Refuted(cex) => Err(Error {
            control: control.to_string(),
            line,
            clause,
            message: message.into(),
            counterexample: Some(cex),
        }),
    }
}

/// Checks `Θ; guard ⊨ goal` ([N2]).
#[allow(clippy::too_many_arguments)]
fn require_formula(
    solver: &mut Solver,
    guard: Formula,
    goal: Formula,
    control: &str,
    line: u32,
    clause: &'static str,
    message: impl Into<String>,
    visible_vars: &[(String, TermId, bool)],
    in_loop: bool,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
) -> Result<(), Error> {
    let not_goal = solver.bdd.not(goal);
    let negated = solver.bdd.and(guard, not_goal);
    require_entailment(
        solver,
        negated,
        control,
        line,
        clause,
        message,
        visible_vars,
        in_loop,
        certificates,
    )
}

fn type_range_formula(solver: &mut Solver, term: &LinearTerm, ty: IntType) -> Formula {
    if ty == IntType::Usize {
        let low = solver.le(&LinearTerm::from_i128(0), term);
        let umax = LinearTerm::var(solver.usize_max_term);
        let high = solver.le(term, &umax);
        solver.bdd.and(low, high)
    } else {
        let (lo, hi) = ty.literal_bounds();
        let low = solver.le(&LinearTerm::constant(lo), term);
        let high = solver.le(term, &LinearTerm::constant(hi));
        solver.bdd.and(low, high)
    }
}

/// Checks `[P5]` (arithmetic range and non-zero divisor) and `[P3]` (function
/// preconditions) for `expr` under history guard `guard`, in the evaluation
/// order of `[P7]` and `[E-Bool]`.
#[allow(clippy::too_many_arguments)]
fn check_expr_obligations(
    program: &Program,
    solver: &mut Solver,
    expr: &Expr,
    guard: Formula,
    env: &Env,
    control: &str,
    line: u32,
    visible_vars: &[(String, TermId, bool)],
    in_loop: bool,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
) -> Result<(), Error> {
    match expr {
        Expr::Boolean(_) | Expr::Integer(_, _) | Expr::Var(_, _) => Ok(()),
        Expr::Not(inner) => check_expr_obligations(
            program,
            solver,
            inner,
            guard,
            env,
            control,
            line,
            visible_vars,
            in_loop,
            certificates,
        ),
        Expr::Negate(inner, ty) => {
            check_expr_obligations(
                program,
                solver,
                inner,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            let term = translate_term(program, solver, expr, env);
            let fits = type_range_formula(solver, &term, *ty);
            require_formula(
                solver,
                guard,
                fits,
                control,
                line,
                "P5",
                format!("negation may overflow `{}`", ty.name()),
                visible_vars,
                in_loop,
                certificates,
            )?;
            solver.add_fact(guard, fits);
            Ok(())
        }
        Expr::Arithmetic(op, left, right, ty) => {
            check_expr_obligations(
                program,
                solver,
                left,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            check_expr_obligations(
                program,
                solver,
                right,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            if matches!(op, Arithmetic::Divide | Arithmetic::Remainder) {
                let r_term = translate_term(program, solver, right, env);
                let non_zero = solver.ne(&r_term, &LinearTerm::from_i128(0));
                require_formula(
                    solver,
                    guard,
                    non_zero,
                    control,
                    line,
                    "P5",
                    "divisor may be zero",
                    visible_vars,
                    in_loop,
                    certificates,
                )?;
                solver.add_fact(guard, non_zero);
            }
            let term = translate_term(program, solver, expr, env);
            let fits = type_range_formula(solver, &term, *ty);
            require_formula(
                solver,
                guard,
                fits,
                control,
                line,
                "P5",
                format!("arithmetic result may overflow `{}`", ty.name()),
                visible_vars,
                in_loop,
                certificates,
            )?;
            solver.add_fact(guard, fits);
            Ok(())
        }
        Expr::Compare(_, left, right) => {
            check_expr_obligations(
                program,
                solver,
                left,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            check_expr_obligations(
                program,
                solver,
                right,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )
        }
        // [P7], [E-Bool]: The right operand of `∧` is evaluated where the left
        // operand is true, and that of `∨` where the left operand is false.
        Expr::And(left, right) => {
            check_expr_obligations(
                program,
                solver,
                left,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            let left_holds = translate_formula(program, solver, left, env);
            let right_guard = solver.bdd.and(guard, left_holds);
            check_expr_obligations(
                program,
                solver,
                right,
                right_guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )
        }
        Expr::Or(left, right) => {
            check_expr_obligations(
                program,
                solver,
                left,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            let left_holds = translate_formula(program, solver, left, env);
            let not_left = solver.bdd.not(left_holds);
            let right_guard = solver.bdd.and(guard, not_left);
            check_expr_obligations(
                program,
                solver,
                right,
                right_guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )
        }
        Expr::Call(fn_id, arguments, _) => {
            for arg in arguments {
                check_expr_obligations(
                    program,
                    solver,
                    arg,
                    guard,
                    env,
                    control,
                    line,
                    visible_vars,
                    in_loop,
                    certificates,
                )?;
            }
            let callee = &program.functions[*fn_id];
            if let Some(pre) = &callee.precondition {
                let arg_terms: Vec<LinearTerm> = arguments
                    .iter()
                    .map(|a| translate_term(program, solver, a, env))
                    .collect();
                let call_env = Env {
                    locals: HashMap::new(),
                    parameters: arg_terms,
                    fields: Vec::new(),
                    constants: HashMap::new(),
                };
                let pre_formula = translate_formula(program, solver, pre, &call_env);
                require_formula(
                    solver,
                    guard,
                    pre_formula,
                    control,
                    line,
                    "P3",
                    format!("precondition of function `{}` may not hold", callee.name),
                    visible_vars,
                    in_loop,
                    certificates,
                )?;
                solver.add_fact(guard, pre_formula);
            }
            Ok(())
        }
    }
}

/// Checks a [`Function`] declaration ([P8]).
fn check_function(
    program: &Program,
    _id: FunctionId,
    function: &Function,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
) -> Result<(), Error> {
    let mut solver = Solver::new(Vec::new());
    let mut param_terms = Vec::new();
    let mut visible_vars = Vec::new();

    for param in &function.parameters {
        let term_id = match param.ty {
            Type::Int(int_ty) => solver.fresh_var(param.name.clone(), Some(int_ty), true),
            Type::Bool => {
                let id = solver.fresh_var(param.name.clone(), None, false);
                let t = LinearTerm::var(id);
                let lo = solver.le(&LinearTerm::from_i128(0), &t);
                let hi = solver.le(&t, &LinearTerm::from_i128(1));
                let both = solver.bdd.and(lo, hi);
                solver.add_structure(both);
                id
            }
        };
        param_terms.push(LinearTerm::var(term_id));
        visible_vars.push((param.name.clone(), term_id, param.ty == Type::Bool));
    }

    let env = Env {
        locals: HashMap::new(),
        parameters: param_terms,
        fields: Vec::new(),
        constants: HashMap::new(),
    };

    if let Some(pre) = &function.precondition {
        let pre_formula = translate_formula(program, &mut solver, pre, &env);
        solver.add_fact(bdd::TRUE, pre_formula);
    }

    check_expr_obligations(
        program,
        &mut solver,
        &function.body,
        bdd::TRUE,
        &env,
        &function.name,
        1,
        &visible_vars,
        false,
        certificates,
    )
}

fn collect_read_vars(expr: &Expr, out: &mut Vec<VarId>) {
    match expr {
        Expr::Boolean(_) | Expr::Integer(_, _) => {}
        Expr::Var(VarRef::Local(id), _) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Expr::Var(_, _) => {}
        Expr::Negate(inner, _) | Expr::Not(inner) => collect_read_vars(inner, out),
        Expr::Arithmetic(_, l, r, _)
        | Expr::Compare(_, l, r)
        | Expr::And(l, r)
        | Expr::Or(l, r) => {
            collect_read_vars(l, out);
            collect_read_vars(r, out);
        }
        Expr::Call(_, args, _) => {
            for arg in args {
                collect_read_vars(arg, out);
            }
        }
    }
}

fn collect_modified_vars(body: &ControlBody, body_id: BodyId, out: &mut HashSet<VarId>) {
    for stmt in &body.bodies[body_id].statements {
        match &stmt.command {
            Command::Assign { var, .. } => {
                out.insert(*var);
            }
            Command::Loop { body: inner, .. } => {
                collect_modified_vars(body, *inner, out);
            }
            _ => {}
        }
    }
}

/// Identifies single-binder immutable locals bound to constant expressions ([I3]).
fn find_constant_locals(program: &Program, body: &ControlBody) -> HashMap<VarId, Int> {
    let mut bind_counts: HashMap<VarId, usize> = HashMap::new();
    let mut bind_exprs: HashMap<VarId, Expr> = HashMap::new();
    for (idx, _) in body.vars.iter().enumerate() {
        bind_counts.insert(idx, 0);
    }
    for param_idx in 0..body
        .vars
        .iter()
        .take_while(|v| v.body == 0 && !v.mutable)
        .count()
    {
        // Parameters of the control are not constant expressions ([I3]).
        *bind_counts.entry(param_idx).or_insert(0) += 2;
    }
    for b in &body.bodies {
        if let Some(head) = &b.head {
            for &lp in &head.parameters {
                *bind_counts.entry(lp).or_insert(0) += 2;
            }
        }
        for stmt in &b.statements {
            match &stmt.command {
                Command::Bind { var, value } => {
                    *bind_counts.entry(*var).or_insert(0) += 1;
                    bind_exprs.insert(*var, value.clone());
                }
                Command::Decide { arms, .. } => {
                    for arm in arms {
                        for &v in &arm.vars {
                            *bind_counts.entry(v).or_insert(0) += 2;
                        }
                    }
                }
                Command::Loop { arms, .. } => {
                    for arm in arms {
                        for &v in arm {
                            *bind_counts.entry(v).or_insert(0) += 2;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut constants = HashMap::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (&var, &count) in &bind_counts {
            if count == 1
                && !body.vars[var].mutable
                && !constants.contains_key(&var)
                && let Some(expr) = bind_exprs.get(&var)
                && let Some(val) = eval_const(program, expr, &constants)
            {
                let fits = match body.vars[var].ty {
                    Type::Int(int_ty) => {
                        let (lo, hi) = int_ty.literal_bounds();
                        val >= lo && val <= hi
                    }
                    Type::Bool => false,
                };
                if fits {
                    constants.insert(var, val);
                    changed = true;
                }
            }
        }
    }
    constants
}

struct BodyChecker<'a> {
    program: &'a Program,
    control: &'a Control,
    body: &'a ControlBody,
    solver: Solver,
    /// BDD variable index for each `(SiteId, alternative_position)`.
    site_atoms: Vec<Vec<usize>>,
    /// `D(x)` for each `VarId` ([7.4]).
    defined: Vec<Formula>,
    /// Disjoint `(history_guard, logical_term_id)` versions of each `VarId` ([I1], [N5]).
    versions: Vec<Vec<(Formula, TermId)>>,
    /// Entry logical terms for the control's parameters ([P4]).
    entry_parameters: Vec<LinearTerm>,
    /// Constant local map ([I3]).
    constants: HashMap<VarId, Int>,
    certificates: &'a mut Vec<(String, u32, &'static str, Certificate)>,
    warnings: &'a mut Vec<crate::surface::Warning>,
}

impl<'a> BodyChecker<'a> {
    fn atom(&mut self, site: SiteId, alt_pos: usize) -> Formula {
        let var = self.site_atoms[site][alt_pos];
        self.solver.bdd.variable(var)
    }

    fn selector(&mut self, sel: &Selector) -> Formula {
        match sel {
            Selector::Top => bdd::TRUE,
            Selector::Atom(site, alt_pos) => self.atom(*site, *alt_pos),
            Selector::Not(inner) => {
                let f = self.selector(inner);
                self.solver.bdd.not(f)
            }
            Selector::And(l, r) => {
                let fl = self.selector(l);
                let fr = self.selector(r);
                self.solver.bdd.and(fl, fr)
            }
            Selector::Or(l, r) => {
                let fl = self.selector(l);
                let fr = self.selector(r);
                self.solver.bdd.or(fl, fr)
            }
        }
    }

    fn alloc_var_term(&mut self, var: VarId) -> TermId {
        let info = &self.body.vars[var];
        match info.ty {
            Type::Int(int_ty) => self.solver.fresh_var(info.name.clone(), Some(int_ty), true),
            Type::Bool => {
                let id = self.solver.fresh_var(info.name.clone(), None, false);
                let t = LinearTerm::var(id);
                let lo = self.solver.le(&LinearTerm::from_i128(0), &t);
                let hi = self.solver.le(&t, &LinearTerm::from_i128(1));
                let both = self.solver.bdd.and(lo, hi);
                self.solver.add_fact(bdd::TRUE, both);
                id
            }
        }
    }

    fn set_version(&mut self, var: VarId, guard: Formula, term_id: TermId) {
        let mut updated = Vec::new();
        for (old_guard, old_term) in self.versions[var].drain(..) {
            let remaining = self.solver.bdd.minus(old_guard, guard);
            if remaining != bdd::FALSE {
                updated.push((remaining, old_term));
            }
        }
        if guard != bdd::FALSE {
            updated.push((guard, term_id));
        }
        self.versions[var] = updated;
    }

    /// Builds an [`Env`] that maps each currently defined variable to its
    /// logical term (introducing a multiplexer variable when a variable has
    /// more than one version across histories, [I1]).
    fn current_env(&mut self) -> (Env, Vec<(String, TermId, bool)>) {
        let mut locals = HashMap::new();
        let mut visible = Vec::new();
        for var in 0..self.body.vars.len() {
            if self.versions[var].is_empty() {
                continue;
            }
            let term_id = if self.versions[var].len() == 1 {
                self.versions[var][0].1
            } else {
                let mux = self.alloc_var_term(var);
                let entries = self.versions[var].clone();
                let mut union_guard = bdd::FALSE;
                for (h, tid) in entries {
                    union_guard = self.solver.bdd.or(union_guard, h);
                    self.solver.add_def_linear(h, mux, LinearTerm::var(tid));
                }
                self.versions[var] = vec![(union_guard, mux)];
                mux
            };
            locals.insert(var, LinearTerm::var(term_id));
            visible.push((
                self.body.vars[var].name.clone(),
                term_id,
                self.body.vars[var].ty == Type::Bool,
            ));
        }
        (
            Env {
                locals,
                parameters: self.entry_parameters.clone(),
                fields: Vec::new(),
                constants: self.constants.clone(),
            },
            visible,
        )
    }

    /// Checks `[O2]` (variable availability) for every local variable mentioned
    /// in `expr` under history guard `r_i`.
    fn check_reads(
        &mut self,
        expr: &Expr,
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        let mut vars = Vec::new();
        collect_read_vars(expr, &mut vars);
        for var in vars {
            let d_var = self.defined[var];
            require_formula(
                &mut self.solver,
                r_i,
                d_var,
                &self.control.name,
                line,
                "O2",
                format!(
                    "`{}` may not be bound on this history",
                    self.body.vars[var].name
                ),
                visible_vars,
                in_loop,
                self.certificates,
            )?;
        }
        Ok(())
    }

    fn bind_value(&mut self, var: VarId, r_i: Formula, expr: &Expr, env: &Env) -> TermId {
        let term_id = self.alloc_var_term(var);
        match self.body.vars[var].ty {
            Type::Int(_) => {
                let val_term = translate_term(self.program, &mut self.solver, expr, env);
                self.solver.add_def_linear(r_i, term_id, val_term);
            }
            Type::Bool => {
                let phi = translate_formula(self.program, &mut self.solver, expr, env);
                self.solver.add_def_bool(r_i, term_id, phi);
            }
        }
        self.set_version(var, r_i, term_id);
        term_id
    }

    fn check_body(
        &mut self,
        body_id: BodyId,
        mut live: Formula,
        in_loop: bool,
        loop_leaves: &mut HashMap<BodyId, Vec<LeaveRecord>>,
    ) -> Result<(), Error> {
        let statements = self.body.bodies[body_id].statements.clone();
        let last_line = statements.last().map_or(1, |s| s.line);

        for statement in &statements {
            let line = statement.line;
            let g_i = self.selector(&statement.selector);
            // [K2]: Rᵢ = Lᵢ ∧ gᵢ
            let r_i = self.solver.bdd.and(live, g_i);
            let (env, visible_vars) = self.current_env();

            match &statement.command {
                Command::Bind { var, value } => {
                    self.check_reads(value, r_i, line, &visible_vars, in_loop)?;
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        value,
                        r_i,
                        &env,
                        &self.control.name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    // [O1]: Θ ⊨ ¬(Rᵢ ∧ D(x))
                    let d_var = self.defined[*var];
                    let double_bind = self.solver.bdd.and(r_i, d_var);
                    require_entailment(
                        &mut self.solver,
                        double_bind,
                        &self.control.name,
                        line,
                        "O1",
                        format!(
                            "`{}` may already be bound on this history",
                            self.body.vars[*var].name
                        ),
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.bind_value(*var, r_i, value, &env);
                    self.defined[*var] = self.solver.bdd.or(self.defined[*var], r_i);
                }
                Command::Assign { var, value } => {
                    self.check_reads(value, r_i, line, &visible_vars, in_loop)?;
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        value,
                        r_i,
                        &env,
                        &self.control.name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    // [O4]: Θ ⊨ Rᵢ → D(x)
                    let d_var = self.defined[*var];
                    require_formula(
                        &mut self.solver,
                        r_i,
                        d_var,
                        &self.control.name,
                        line,
                        "O4",
                        format!(
                            "`{}` has no slot on this history",
                            self.body.vars[*var].name
                        ),
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.bind_value(*var, r_i, value, &env);
                }
                Command::Oblige(formula) => {
                    self.check_reads(formula, r_i, line, &visible_vars, in_loop)?;
                    let phi = translate_formula(self.program, &mut self.solver, formula, &env);
                    // [P1]: Θ; Rᵢ ⊨ φ
                    require_formula(
                        &mut self.solver,
                        r_i,
                        phi,
                        &self.control.name,
                        line,
                        "P1",
                        "proof obligation may not hold",
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.solver.add_fact(r_i, phi);
                }
                Command::Test { site, condition } => {
                    self.check_reads(condition, r_i, line, &visible_vars, in_loop)?;
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        condition,
                        r_i,
                        &env,
                        &self.control.name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    let d_true = self.atom(*site, 0);
                    let d_false = self.atom(*site, 1);
                    // [K4]: d.true → Rᵢ, d.false → Rᵢ, ¬(d.true ∧ d.false)
                    let t_imp_r = self.solver.bdd.implies(d_true, r_i);
                    let f_imp_r = self.solver.bdd.implies(d_false, r_i);
                    let both = self.solver.bdd.and(d_true, d_false);
                    let not_both = self.solver.bdd.not(both);
                    // [K5]: Rᵢ → d.true ∨ d.false
                    let either = self.solver.bdd.or(d_true, d_false);
                    let r_imp_either = self.solver.bdd.implies(r_i, either);
                    self.solver.add_structure(t_imp_r);
                    self.solver.add_structure(f_imp_r);
                    self.solver.add_structure(not_both);
                    self.solver.add_structure(r_imp_either);

                    // [N4]: d.true ⇒ e, d.false ⇒ ¬e
                    let phi = translate_formula(self.program, &mut self.solver, condition, &env);
                    let not_phi = self.solver.bdd.not(phi);
                    self.solver.add_fact(d_true, phi);
                    self.solver.add_fact(d_false, not_phi);
                }
                Command::Decide {
                    site,
                    operation,
                    arguments,
                    arms,
                } => {
                    let callee = &self.program.controls[*operation];
                    // [C2]: Every alternative of the operation's contract is listed.
                    if arms.len() != callee.contract.len() {
                        return Err(Error {
                            control: self.control.name.clone(),
                            line,
                            clause: "C2",
                            message: format!(
                                "decision on `{}` lists {} of {} alternatives",
                                callee.name,
                                arms.len(),
                                callee.contract.len()
                            ),
                            counterexample: None,
                        });
                    }

                    for arg in arguments {
                        self.check_reads(arg, r_i, line, &visible_vars, in_loop)?;
                        check_expr_obligations(
                            self.program,
                            &mut self.solver,
                            arg,
                            r_i,
                            &env,
                            &self.control.name,
                            line,
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }

                    // [F3]: The effects of the operation are within the `uses`
                    // of the calling control (when Rᵢ is feasible).
                    for effect in &callee.uses {
                        if !self.control.uses.contains(effect) {
                            let feasible = matches!(
                                self.solver.decide(r_i, &visible_vars, in_loop),
                                Decision::Refuted(_)
                            );
                            if feasible {
                                return Err(Error {
                                    control: self.control.name.clone(),
                                    line,
                                    clause: "F3",
                                    message: format!(
                                        "`{}` uses effect `{effect}`, which `{}` does not allow",
                                        callee.name, self.control.name
                                    ),
                                    counterexample: None,
                                });
                            }
                        }
                    }

                    let arg_terms: Vec<LinearTerm> = arguments
                        .iter()
                        .map(|a| translate_term(self.program, &mut self.solver, a, &env))
                        .collect();

                    // [P3]: Precondition of the operation.
                    if let Some(pre) = &callee.precondition {
                        let call_env = Env {
                            locals: HashMap::new(),
                            parameters: arg_terms.clone(),
                            fields: Vec::new(),
                            constants: HashMap::new(),
                        };
                        let pre_formula =
                            translate_formula(self.program, &mut self.solver, pre, &call_env);
                        require_formula(
                            &mut self.solver,
                            r_i,
                            pre_formula,
                            &self.control.name,
                            line,
                            "P3",
                            format!("precondition of `{}` may not hold", callee.name),
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                        self.solver.add_fact(r_i, pre_formula);
                    }

                    let mut any_arm = bdd::FALSE;
                    let mut arm_atoms = Vec::new();
                    for pos in 0..arms.len() {
                        let atom = self.atom(*site, pos);
                        arm_atoms.push(atom);
                        any_arm = self.solver.bdd.or(any_arm, atom);
                        // [K4]: d.a → Rᵢ
                        let imp = self.solver.bdd.implies(atom, r_i);
                        self.solver.add_structure(imp);
                    }
                    // [K4]: ¬(d.a ∧ d.b)
                    for i in 0..arm_atoms.len() {
                        for j in (i + 1)..arm_atoms.len() {
                            let both = self.solver.bdd.and(arm_atoms[i], arm_atoms[j]);
                            let not_both = self.solver.bdd.not(both);
                            self.solver.add_structure(not_both);
                        }
                    }

                    let pure_alt_term = if callee.is_pure() {
                        let id = self
                            .solver
                            .intern_term(AtomTerm::PureAlt(*operation, arg_terms.clone()));
                        self.solver.instantiate_all();
                        Some(LinearTerm::var(id))
                    } else {
                        None
                    };

                    for (pos, arm) in arms.iter().enumerate() {
                        let atom = arm_atoms[pos];
                        let contract_alt = &callee.contract[arm.alternative];
                        let mut field_terms = Vec::new();

                        for (field_idx, &var_id) in arm.vars.iter().enumerate() {
                            // [O1]: Θ ⊨ ¬(d.a ∧ D(x))
                            let d_var = self.defined[var_id];
                            let double_bind = self.solver.bdd.and(atom, d_var);
                            require_entailment(
                                &mut self.solver,
                                double_bind,
                                &self.control.name,
                                line,
                                "O1",
                                format!(
                                    "`{}` may already be bound on this history",
                                    self.body.vars[var_id].name
                                ),
                                &visible_vars,
                                in_loop,
                                self.certificates,
                            )?;
                            let tid = self.alloc_var_term(var_id);
                            let v_term = LinearTerm::var(tid);
                            self.set_version(var_id, atom, tid);
                            self.defined[var_id] = self.solver.bdd.or(self.defined[var_id], atom);

                            // [I10]: For a pure operation, `y = pay_{f, a, i}(args)`.
                            if callee.is_pure() {
                                let pay_id = self.solver.intern_term(AtomTerm::PurePay(
                                    *operation,
                                    arm.alternative,
                                    field_idx,
                                    arg_terms.clone(),
                                ));
                                self.solver.instantiate_all();
                                let eq = self.solver.eq(&v_term, &LinearTerm::var(pay_id));
                                self.solver.add_fact(atom, eq);
                            }
                            field_terms.push(v_term);
                        }

                        // [I10]: `d.a ⇒ alt_f(args) = a`.
                        if let Some(alt_term) = &pure_alt_term {
                            let eq = self
                                .solver
                                .eq(alt_term, &LinearTerm::from_i128(arm.alternative as i128));
                            self.solver.add_fact(atom, eq);
                        }

                        // [N4]: `d.a ⇒ φₐ(args, payload_vars)`.
                        if let Some(cond) = &contract_alt.condition {
                            let cond_env = Env {
                                locals: HashMap::new(),
                                parameters: arg_terms.clone(),
                                fields: field_terms,
                                constants: HashMap::new(),
                            };
                            let cond_formula =
                                translate_formula(self.program, &mut self.solver, cond, &cond_env);
                            self.solver.add_fact(atom, cond_formula);
                        }
                    }

                    // [U16]: When the fact logic proves that an alternative of
                    // a decision cannot happen and the arm is not `⊥`, warn and
                    // suggest `⊥`.
                    let mut checked_r_i_feasible: Option<bool> = None;
                    for (pos, arm) in arms.iter().enumerate() {
                        let atom = arm_atoms[pos];
                        let contract_alt = &callee.contract[arm.alternative];
                        if contract_alt.condition.is_none() && !callee.is_pure() {
                            continue;
                        }
                        let has_impossible = statements.iter().any(|s| {
                            if matches!(s.command, Command::Impossible) {
                                let sel = self.selector(&s.selector);
                                self.solver.bdd.implies(atom, sel) == bdd::TRUE
                            } else {
                                false
                            }
                        });
                        if has_impossible {
                            continue;
                        }
                        let r_i_feasible = *checked_r_i_feasible.get_or_insert_with(|| {
                            matches!(
                                self.solver.decide(r_i, &visible_vars, in_loop),
                                Decision::Refuted(_)
                            )
                        });
                        if !r_i_feasible {
                            break;
                        }
                        if let Decision::Proved(cert) =
                            self.solver.decide(atom, &visible_vars, in_loop)
                        {
                            kernel::validate(&cert).expect("valid U16 certificate");
                            self.certificates
                                .push((self.control.name.clone(), line, "U16", cert));
                            self.warnings.push(crate::surface::Warning {
                                control: self.control.name.clone(),
                                line,
                                clause: "U16",
                                message: format!(
                                    "the alternative `{}` of `{}` cannot happen here; mark its arm `→ ⊥`",
                                    contract_alt.name, callee.name
                                ),
                            });
                        }
                    }

                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ (gᵢ → d.a₁ ∨ … ∨ d.aₙ)
                    let g_imp_any = self.solver.bdd.implies(g_i, any_arm);
                    live = self.solver.bdd.and(live, g_imp_any);
                }
                Command::Loop {
                    site,
                    body: loop_body,
                    initial,
                    arms,
                } => {
                    for init_expr in initial {
                        self.check_reads(init_expr, r_i, line, &visible_vars, in_loop)?;
                        check_expr_obligations(
                            self.program,
                            &mut self.solver,
                            init_expr,
                            r_i,
                            &env,
                            &self.control.name,
                            line,
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }

                    let head = self.body.bodies[*loop_body]
                        .head
                        .as_ref()
                        .expect("a loop head")
                        .clone();

                    // [P2]: Loop invariant holds for the initial arguments at entry.
                    if let Some(inv) = &head.invariant {
                        let mut init_env = env.clone();
                        for (&param_var, init_expr) in head.parameters.iter().zip(initial) {
                            let init_term =
                                translate_term(self.program, &mut self.solver, init_expr, &env);
                            init_env.locals.insert(param_var, init_term);
                        }
                        let inv_entry =
                            translate_formula(self.program, &mut self.solver, inv, &init_env);
                        require_formula(
                            &mut self.solver,
                            r_i,
                            inv_entry,
                            &self.control.name,
                            line,
                            "P2",
                            format!("invariant of loop `{}` may not hold at entry", head.name),
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }

                    // Save outer state before checking the generic iteration ([K6], [N5], [O6]).
                    let saved_defined = self.defined.clone();
                    let saved_versions = self.versions.clone();

                    // [N5], [I9]: Give fresh logical names to loop parameters
                    // and to outer names that the loop body can change.
                    let mut modified_outer = HashSet::new();
                    collect_modified_vars(self.body, *loop_body, &mut modified_outer);
                    for &var in &modified_outer {
                        if self.body.vars[var].body != *loop_body && self.defined[var] != bdd::FALSE
                        {
                            let fresh_tid = self.alloc_var_term(var);
                            let active_on_loop = self.solver.bdd.and(self.defined[var], r_i);
                            self.set_version(var, active_on_loop, fresh_tid);
                        }
                    }

                    for &param_var in &head.parameters {
                        let tid = self.alloc_var_term(param_var);
                        self.defined[param_var] = r_i;
                        self.versions[param_var] = vec![(r_i, tid)];
                    }

                    // [N4]: Inside the loop body, the invariant is assumed on R_d.
                    if let Some(inv) = &head.invariant {
                        let (iter_env, _) = self.current_env();
                        let inv_iter =
                            translate_formula(self.program, &mut self.solver, inv, &iter_env);
                        self.solver.add_fact(r_i, inv_iter);
                    }

                    loop_leaves.insert(*loop_body, Vec::new());
                    // [K6]: Check the loop body for one generic iteration with L₁ = R_d.
                    self.check_body(*loop_body, r_i, true, loop_leaves)?;
                    let leaves = loop_leaves.remove(loop_body).unwrap_or_default();

                    // Restore outer definedness and link post-loop facts ([K8], [O6]).
                    self.defined = saved_defined;
                    self.versions = saved_versions;

                    let mut any_arm = bdd::FALSE;
                    let mut arm_atoms = Vec::new();
                    for alt_idx in 0..arms.len() {
                        let atom = self.atom(*site, alt_idx);
                        arm_atoms.push(atom);
                        any_arm = self.solver.bdd.or(any_arm, atom);
                        // [K4]: d.a → Rᵢ
                        let imp = self.solver.bdd.implies(atom, r_i);
                        self.solver.add_structure(imp);

                        // [K8]: d.a ↔ R_{j₁} ∨ … ∨ R_{jₖ}
                        let mut leave_disj = bdd::FALSE;
                        for (leave_alt, r_j, _, _) in &leaves {
                            if *leave_alt == alt_idx {
                                leave_disj = self.solver.bdd.or(leave_disj, *r_j);
                            }
                        }
                        let iff = self.solver.bdd.iff(atom, leave_disj);
                        self.solver.add_structure(iff);
                    }
                    for i in 0..arm_atoms.len() {
                        for j in (i + 1)..arm_atoms.len() {
                            let both = self.solver.bdd.and(arm_atoms[i], arm_atoms[j]);
                            let not_both = self.solver.bdd.not(both);
                            self.solver.add_structure(not_both);
                        }
                    }

                    // Update modified outer variables on the histories where the loop left ([K8]).
                    for (_, r_j, _, leave_versions) in &leaves {
                        for &var in &modified_outer {
                            if self.body.vars[var].body != *loop_body {
                                let mux = self.alloc_var_term(var);
                                for &(h, tid) in &leave_versions[var] {
                                    let on_leave = self.solver.bdd.and(*r_j, h);
                                    if on_leave != bdd::FALSE {
                                        self.solver.add_def_linear(
                                            on_leave,
                                            mux,
                                            LinearTerm::var(tid),
                                        );
                                    }
                                }
                                self.set_version(var, *r_j, mux);
                            }
                        }
                    }

                    // Bind loop arm payload variables ([O1], [K8]).
                    for (alt_idx, arm_vars) in arms.iter().enumerate() {
                        let atom = arm_atoms[alt_idx];
                        for (field_pos, &var_id) in arm_vars.iter().enumerate() {
                            let d_var = self.defined[var_id];
                            let double_bind = self.solver.bdd.and(atom, d_var);
                            require_entailment(
                                &mut self.solver,
                                double_bind,
                                &self.control.name,
                                line,
                                "O1",
                                format!(
                                    "`{}` may already be bound on this history",
                                    self.body.vars[var_id].name
                                ),
                                &visible_vars,
                                in_loop,
                                self.certificates,
                            )?;
                            let out_tid = self.alloc_var_term(var_id);
                            for (leave_alt, r_j, payload_terms, _) in &leaves {
                                if *leave_alt == alt_idx
                                    && let Some(val_term) = payload_terms.get(field_pos)
                                {
                                    if self.body.vars[var_id].ty == Type::Bool {
                                        let b = self.solver.le(&LinearTerm::from_i128(1), val_term);
                                        self.solver.add_def_bool(*r_j, out_tid, b);
                                    } else {
                                        self.solver.add_def_linear(*r_j, out_tid, val_term.clone());
                                    }
                                }
                            }
                            self.set_version(var_id, atom, out_tid);
                            self.defined[var_id] = self.solver.bdd.or(self.defined[var_id], atom);
                        }
                    }

                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ (gᵢ → d.a₁ ∨ … ∨ d.aₙ)
                    let g_imp_any = self.solver.bdd.implies(g_i, any_arm);
                    live = self.solver.bdd.and(live, g_imp_any);
                }
                Command::Again {
                    body: target_body,
                    arguments,
                } => {
                    for arg in arguments {
                        self.check_reads(arg, r_i, line, &visible_vars, in_loop)?;
                        check_expr_obligations(
                            self.program,
                            &mut self.solver,
                            arg,
                            r_i,
                            &env,
                            &self.control.name,
                            line,
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    let head = self.body.bodies[*target_body]
                        .head
                        .as_ref()
                        .expect("a loop head")
                        .clone();
                    // [P2]: Loop invariant holds for the new arguments at `↑`.
                    if let Some(inv) = &head.invariant {
                        let mut next_env = env.clone();
                        for (&param_var, arg_expr) in head.parameters.iter().zip(arguments) {
                            let arg_term =
                                translate_term(self.program, &mut self.solver, arg_expr, &env);
                            next_env.locals.insert(param_var, arg_term);
                        }
                        let inv_again =
                            translate_formula(self.program, &mut self.solver, inv, &next_env);
                        require_formula(
                            &mut self.solver,
                            r_i,
                            inv_again,
                            &self.control.name,
                            line,
                            "P2",
                            format!("invariant of loop `{}` may not hold at `↑`", head.name),
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Leave {
                    alternative,
                    payload,
                } => {
                    for item in payload {
                        self.check_reads(item, r_i, line, &visible_vars, in_loop)?;
                        check_expr_obligations(
                            self.program,
                            &mut self.solver,
                            item,
                            r_i,
                            &env,
                            &self.control.name,
                            line,
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    let payload_terms: Vec<LinearTerm> = payload
                        .iter()
                        .map(|e| translate_term(self.program, &mut self.solver, e, &env))
                        .collect();
                    if let Some(list) = loop_leaves.get_mut(&body_id) {
                        list.push((*alternative, r_i, payload_terms, self.versions.clone()));
                    }
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Exit {
                    alternative,
                    payload,
                } => {
                    for item in payload {
                        self.check_reads(item, r_i, line, &visible_vars, in_loop)?;
                        check_expr_obligations(
                            self.program,
                            &mut self.solver,
                            item,
                            r_i,
                            &env,
                            &self.control.name,
                            line,
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    let entry = &self.control.contract[*alternative];
                    // [P4]: Contract condition `⇐ φₐ` at `■`.
                    if let Some(cond) = &entry.condition {
                        let payload_terms: Vec<LinearTerm> = payload
                            .iter()
                            .map(|e| translate_term(self.program, &mut self.solver, e, &env))
                            .collect();
                        let exit_env = Env {
                            locals: HashMap::new(),
                            parameters: self.entry_parameters.clone(),
                            fields: payload_terms,
                            constants: HashMap::new(),
                        };
                        let cond_formula =
                            translate_formula(self.program, &mut self.solver, cond, &exit_env);
                        require_formula(
                            &mut self.solver,
                            r_i,
                            cond_formula,
                            &self.control.name,
                            line,
                            "P4",
                            format!(
                                "condition of exit alternative `{}` may not hold",
                                entry.name
                            ),
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Impossible => {
                    // [P6]: Θ ⊨ ¬Rᵢ
                    require_entailment(
                        &mut self.solver,
                        r_i,
                        &self.control.name,
                        line,
                        "P6",
                        "`⊥` may be reachable on this history",
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
            }
        }

        // [K7] Coverage: Θ ⊨ ¬Lₙ₊₁
        let (_, visible_vars) = self.current_env();
        require_entailment(
            &mut self.solver,
            live,
            &self.control.name,
            last_line,
            "K7",
            "a feasible history reaches the end of the body without a disposition",
            &visible_vars,
            in_loop,
            self.certificates,
        )
    }
}

type LeaveRecord = (usize, Formula, Vec<LinearTerm>, Vec<Vec<(Formula, TermId)>>);

fn check_control(
    program: &Program,
    _id: ControlId,
    control: &Control,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
    warnings: &mut Vec<crate::surface::Warning>,
) -> Result<(), Error> {
    let body = control.body.as_ref().expect("a defined control");
    let mut history_names = Vec::new();
    let mut site_atoms = Vec::new();
    for site in &body.sites {
        let mut indices = Vec::new();
        for alt in &site.alternatives {
            indices.push(history_names.len());
            history_names.push((site.name.clone(), alt.clone()));
        }
        site_atoms.push(indices);
    }

    let solver = Solver::new(history_names);
    let var_count = body.vars.len();
    let constants = find_constant_locals(program, body);

    let mut checker = BodyChecker {
        program,
        control,
        body,
        solver,
        site_atoms,
        defined: vec![bdd::FALSE; var_count],
        versions: vec![Vec::new(); var_count],
        entry_parameters: Vec::new(),
        constants,
        certificates,
        warnings,
    };

    for param_idx in 0..control.parameters.len() {
        let tid = checker.alloc_var_term(param_idx);
        checker.defined[param_idx] = bdd::TRUE;
        checker.versions[param_idx] = vec![(bdd::TRUE, tid)];
        checker.entry_parameters.push(LinearTerm::var(tid));
    }

    if let Some(pre) = &control.precondition {
        let (env, _) = checker.current_env();
        let pre_formula = translate_formula(program, &mut checker.solver, pre, &env);
        checker.solver.add_fact(bdd::TRUE, pre_formula);
    }

    let mut loop_leaves = HashMap::new();
    checker.check_body(0, bdd::TRUE, false, &mut loop_leaves)
}
