//! The phase 3 checker (`docs/v2/core.md`, section 7; `docs/v2/facts.md`).
//!
//! For each function, named boundary, and defined control of a
//! [`typed::Program`], [`check`] verifies:
//!
//! - **[K1] to [K8]** Live histories, structure `Ω`, loop post-iteration facts
//!   and coverage (`Θ ⊨ ¬Lₙ₊₁`, [K7]).
//! - **[N1] to [N5]** Conditional facts `Ψ`, SSA renaming on mutation and loop
//!   entry, and exact entailment backed by the independent certificate kernel
//!   ([`crate::kernel::validate`], [V1]).
//! - **[C2]** Decision acknowledgement (every alternative of the operation's
//!   contract is listed).
//! - **[O0] to [O6]** Ownership and availability (`D(x)` and `M(x)` on
//!   histories: operand order [O0], no double bind [O1], available at use [O2],
//!   move-on-consume and no partial moves [O3], no discarded `must`
//!   temporaries [O3a], overwrite/component assignment rules [O4], scope-exit
//!   obligations [O5], outer names across loops [O6]).
//! - **[P1] to [P8]** Proof obligations: `⊢ φ` ([P1]), loop invariants at
//!   entry and `↑` ([P2]), preconditions of functions and controls ([P3]),
//!   contract exit conditions `⇐ φ` with `old(y)` ([P4]), arithmetic bounds,
//!   non-zero divisors, and vector index bounds ([P5]), unreachable `⊥` ([P6]),
//!   left-to-right and short-circuit order ([P7]), and function bodies ([P8]).
//! - **[F1] to [F6]** Fault permissions ([F1]), fault-releasable values at
//!   faulting statements ([F2]), effects within `uses` ([F3]), `assume ⊥`
//!   ([F4]), lexical boundaries ([F5]), and named boundaries ([F6]).

use crate::bdd::{self, Formula};
use crate::facts::{AtomTerm, Certificate, Counterexample, Decision, LinearTerm, Solver, TermId};
use crate::int::Int;
use crate::kernel;
use crate::typed::{
    Alternative, Arithmetic, BodyId, BorrowMode, BoundaryDef, BuiltinOp, Command, Comparison,
    Control, ControlBody, Expr, Function, FunctionId, IntType, Kind, Operation, Place, PlaceStep,
    Program, Selector, SiteId, StructId, Type, VarId, VarRef,
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

/// Checks every function, named boundary, and defined control of `program`.
pub fn check(program: &Program) -> Result<CheckedProgram, Error> {
    let mut certificates = Vec::new();
    let mut warnings = Vec::new();
    for (id, function) in program.functions.iter().enumerate() {
        check_function(program, id, function, &mut certificates)?;
    }
    let mut boundary_may_fault: Vec<bool> =
        program.boundaries.iter().map(|b| b.may_fault).collect();
    for (bid, boundary) in program.boundaries.iter().enumerate() {
        let may_fault = check_boundary(
            program,
            boundary,
            &boundary_may_fault,
            &mut certificates,
            &mut warnings,
        )?;
        boundary_may_fault[bid] = may_fault;
    }
    for control in &program.controls {
        if control.body.is_some() {
            check_control(
                program,
                control,
                &boundary_may_fault,
                &mut certificates,
                &mut warnings,
            )?;
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
    old_parameters: Vec<LinearTerm>,
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

    fn lookup_old(&self, idx: usize) -> LinearTerm {
        self.old_parameters
            .get(idx)
            .cloned()
            .unwrap_or_else(|| LinearTerm::from_i128(0))
    }
}

fn subst_place(place: &Place, args: &[Expr]) -> Place {
    Place {
        root: place.root,
        root_ty: place.root_ty.clone(),
        steps: place
            .steps
            .iter()
            .map(|step| match step {
                PlaceStep::Field(sid, fidx, ty) => PlaceStep::Field(*sid, *fidx, ty.clone()),
                PlaceStep::Index(idx, ty) => {
                    PlaceStep::Index(subst_parameters(idx, args), ty.clone())
                }
            })
            .collect(),
        ty: place.ty.clone(),
    }
}

fn unfold_place(program: &Program, place: &Place) -> Place {
    Place {
        root: place.root,
        root_ty: place.root_ty.clone(),
        steps: place
            .steps
            .iter()
            .map(|step| match step {
                PlaceStep::Field(sid, fidx, ty) => PlaceStep::Field(*sid, *fidx, ty.clone()),
                PlaceStep::Index(idx, ty) => {
                    PlaceStep::Index(unfold_functions(program, idx), ty.clone())
                }
            })
            .collect(),
        ty: place.ty.clone(),
    }
}

/// Unfolds non-recursive function calls in `expr` ([I8]).
fn unfold_functions(program: &Program, expr: &Expr) -> Expr {
    match expr {
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Var(_, _)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => expr.clone(),
        Expr::Move(inner, ty) => Expr::Move(Box::new(unfold_functions(program, inner)), ty.clone()),
        Expr::Borrow(mode, place, ty) => {
            Expr::Borrow(*mode, unfold_place(program, place), ty.clone())
        }
        Expr::Field(base, sid, fidx, ty) => Expr::Field(
            Box::new(unfold_functions(program, base)),
            *sid,
            *fidx,
            ty.clone(),
        ),
        Expr::Index(base, idx, ty) => Expr::Index(
            Box::new(unfold_functions(program, base)),
            Box::new(unfold_functions(program, idx)),
            ty.clone(),
        ),
        Expr::Struct(sid, args, ty) => Expr::Struct(
            *sid,
            args.iter().map(|a| unfold_functions(program, a)).collect(),
            ty.clone(),
        ),
        Expr::Variant(vid, cidx, args, ty) => Expr::Variant(
            *vid,
            *cidx,
            args.iter().map(|a| unfold_functions(program, a)).collect(),
            ty.clone(),
        ),
        Expr::Vector(args, ty) => Expr::Vector(
            args.iter().map(|a| unfold_functions(program, a)).collect(),
            ty.clone(),
        ),
        Expr::Len(inner) => Expr::Len(Box::new(unfold_functions(program, inner))),
        Expr::Clone(inner, ty) => {
            Expr::Clone(Box::new(unfold_functions(program, inner)), ty.clone())
        }
        Expr::Trim(inner) => Expr::Trim(Box::new(unfold_functions(program, inner))),
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
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => expr.clone(),
        Expr::Var(VarRef::Parameter(idx), _) => args[*idx].clone(),
        Expr::Var(_, _) => expr.clone(),
        Expr::Move(inner, ty) => Expr::Move(Box::new(subst_parameters(inner, args)), ty.clone()),
        Expr::Borrow(mode, place, ty) => Expr::Borrow(*mode, subst_place(place, args), ty.clone()),
        Expr::Field(base, sid, fidx, ty) => Expr::Field(
            Box::new(subst_parameters(base, args)),
            *sid,
            *fidx,
            ty.clone(),
        ),
        Expr::Index(base, idx, ty) => Expr::Index(
            Box::new(subst_parameters(base, args)),
            Box::new(subst_parameters(idx, args)),
            ty.clone(),
        ),
        Expr::Struct(sid, items, ty) => Expr::Struct(
            *sid,
            items.iter().map(|a| subst_parameters(a, args)).collect(),
            ty.clone(),
        ),
        Expr::Variant(vid, cidx, items, ty) => Expr::Variant(
            *vid,
            *cidx,
            items.iter().map(|a| subst_parameters(a, args)).collect(),
            ty.clone(),
        ),
        Expr::Vector(items, ty) => Expr::Vector(
            items.iter().map(|a| subst_parameters(a, args)).collect(),
            ty.clone(),
        ),
        Expr::Len(inner) => Expr::Len(Box::new(subst_parameters(inner, args))),
        Expr::Clone(inner, ty) => Expr::Clone(Box::new(subst_parameters(inner, args)), ty.clone()),
        Expr::Trim(inner) => Expr::Trim(Box::new(subst_parameters(inner, args))),
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
            ty.clone(),
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

fn int_bounds_for_type(ty: &Type) -> (Option<Int>, Option<Int>) {
    match ty.deref_type() {
        Type::Int(IntType::Usize) => (Some(Int::zero()), None),
        Type::Int(int_ty) => {
            let (lo, hi) = int_ty.literal_bounds();
            (Some(lo), Some(hi))
        }
        _ => (None, None),
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
        Expr::Var(_, ty) | Expr::Old(_, ty) => int_bounds_for_type(ty),
        Expr::Move(inner, _) | Expr::Clone(inner, _) => interval_unfolded(inner, constants),
        Expr::Borrow(_, place, _) => int_bounds_for_type(&place.ty),
        Expr::Len(_) => (Some(Int::zero()), None),
        Expr::Field(base, _, _, ty) => {
            if matches!(&**base, Expr::Struct(_, _, _)) {
                (None, None)
            } else {
                int_bounds_for_type(ty)
            }
        }
        Expr::Index(base, _, ty) => {
            if matches!(&**base, Expr::Vector(_, _)) {
                (None, None)
            } else {
                int_bounds_for_type(ty)
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

fn int_and_bool_info(ty: &Type) -> (Option<IntType>, bool) {
    match ty.deref_type() {
        Type::Int(it) => (Some(*it), false),
        Type::Bool => (None, true),
        _ => (None, false),
    }
}

fn translate_field_term(
    solver: &mut Solver,
    base: LinearTerm,
    sid: StructId,
    fidx: usize,
    fty: &Type,
) -> LinearTerm {
    if let Some(bid) = base.as_var()
        && let AtomTerm::StructCtor(c_sid, fields) = &solver.terms[bid]
        && *c_sid == sid
        && fidx < fields.len()
    {
        return fields[fidx].clone();
    }
    let (int_ty, is_bool) = int_and_bool_info(fty);
    let is_value_term = solver.is_value_term_linear(&base);
    let id = solver.intern_term(AtomTerm::Field {
        base,
        struct_id: sid,
        field_idx: fidx,
        int_ty,
        is_bool,
        is_value_term,
    });
    solver.register_sort(id, fty.deref_type());
    solver.instantiate_all();
    LinearTerm::var(id)
}

fn translate_index_term(
    solver: &mut Solver,
    base: LinearTerm,
    index: LinearTerm,
    ety: &Type,
) -> LinearTerm {
    let (int_ty, is_bool) = int_and_bool_info(ety);
    let is_value_term = solver.is_value_term_linear(&base);
    let id = solver.intern_term(AtomTerm::Index {
        base,
        index,
        int_ty,
        is_bool,
        is_value_term,
    });
    solver.register_sort(id, ety.deref_type());
    solver.instantiate_all();
    LinearTerm::var(id)
}

fn translate_place(program: &Program, solver: &mut Solver, place: &Place, env: &Env) -> LinearTerm {
    let mut cur = env.lookup(VarRef::Local(place.root));
    for step in &place.steps {
        match step {
            PlaceStep::Field(sid, fidx, fty) => {
                cur = translate_field_term(solver, cur, *sid, *fidx, fty);
            }
            PlaceStep::Index(idx_expr, ety) => {
                let idx_term = translate_term(program, solver, idx_expr, env);
                cur = translate_index_term(solver, cur, idx_term, ety);
            }
        }
    }
    cur
}

/// Computes `upd(root_term, steps, new_val)` per `[I9]`.
fn functional_update(
    program: &Program,
    solver: &mut Solver,
    cur_term: LinearTerm,
    cur_ty: &Type,
    steps: &[PlaceStep],
    new_val: LinearTerm,
    env: &Env,
) -> LinearTerm {
    let Some((first, rest)) = steps.split_first() else {
        return new_val;
    };
    match first {
        PlaceStep::Field(sid, fidx, fty) => {
            let sdef = &program.structs[*sid];
            let sub_old = translate_field_term(solver, cur_term.clone(), *sid, *fidx, fty);
            let updated_field =
                functional_update(program, solver, sub_old, fty, rest, new_val, env);
            let mut fields = Vec::with_capacity(sdef.fields.len());
            for (idx, field_param) in sdef.fields.iter().enumerate() {
                if idx == *fidx {
                    fields.push(updated_field.clone());
                } else {
                    fields.push(translate_field_term(
                        solver,
                        cur_term.clone(),
                        *sid,
                        idx,
                        &field_param.ty,
                    ));
                }
            }
            let cid = solver.intern_term(AtomTerm::StructCtor(*sid, fields));
            solver.register_sort(cid, cur_ty.deref_type());
            solver.instantiate_all();
            LinearTerm::var(cid)
        }
        PlaceStep::Index(idx_expr, ety) => {
            let idx_term = translate_term(program, solver, idx_expr, env);
            let sub_old = translate_index_term(solver, cur_term.clone(), idx_term.clone(), ety);
            let updated_elem = functional_update(program, solver, sub_old, ety, rest, new_val, env);
            let uid = solver.intern_term(AtomTerm::Update {
                base: cur_term,
                index: idx_term,
                value: updated_elem,
            });
            solver.register_sort(uid, cur_ty.deref_type());
            solver.instantiate_all();
            LinearTerm::var(uid)
        }
    }
}

fn ensure_struct_fields(program: &Program, solver: &mut Solver, sid: StructId, term: &LinearTerm) {
    if let Some(tid) = term.as_var()
        && matches!(solver.terms[tid], AtomTerm::StructCtor(_, _))
    {
        return;
    }
    for (fidx, field) in program.structs[sid].fields.iter().enumerate() {
        let _ = translate_field_term(solver, term.clone(), sid, fidx, &field.ty);
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
        Expr::String(s) => {
            let id = solver.intern_term(AtomTerm::StrLit(s.clone()));
            solver.register_sort(id, &Type::String);
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Var(vr, _) => env.lookup(*vr),
        Expr::Old(idx, _) => env.lookup_old(*idx),
        Expr::Move(inner, _) | Expr::Clone(inner, _) => translate_term(program, solver, inner, env),
        Expr::Borrow(_, place, _) => translate_place(program, solver, place, env),
        Expr::Field(base, sid, fidx, fty) => {
            let bt = translate_term(program, solver, base, env);
            translate_field_term(solver, bt, *sid, *fidx, fty)
        }
        Expr::Index(base, idx, ety) => {
            let bt = translate_term(program, solver, base, env);
            let it = translate_term(program, solver, idx, env);
            translate_index_term(solver, bt, it, ety)
        }
        Expr::Struct(sid, items, ty) => {
            let fields: Vec<LinearTerm> = items
                .iter()
                .map(|item| translate_term(program, solver, item, env))
                .collect();
            let id = solver.intern_term(AtomTerm::StructCtor(*sid, fields));
            solver.register_sort(id, ty);
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Variant(vid, cidx, items, ty) => {
            let fields: Vec<LinearTerm> = items
                .iter()
                .map(|item| translate_term(program, solver, item, env))
                .collect();
            let num_ctors = program.variants[*vid].constructors.len();
            let id = solver.intern_term(AtomTerm::VariantCtor {
                variant_id: *vid,
                ctor_idx: *cidx,
                fields,
                num_ctors,
            });
            solver.register_sort(id, ty);
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Vector(items, ty) => {
            let elems: Vec<LinearTerm> = items
                .iter()
                .map(|item| translate_term(program, solver, item, env))
                .collect();
            let id = solver.intern_term(AtomTerm::VecLit(elems));
            solver.register_sort(id, ty);
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::EmptyMap(ty) => {
            let id = solver.intern_term(AtomTerm::EmptyMap);
            solver.register_sort(id, ty);
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Len(inner) => {
            let bt = translate_term(program, solver, inner, env);
            let id = solver.intern_term(AtomTerm::Len(bt));
            solver.instantiate_all();
            LinearTerm::var(id)
        }
        Expr::Trim(inner) => {
            let bt = translate_term(program, solver, inner, env);
            let id = solver.intern_term(AtomTerm::Trim(bt));
            solver.register_sort(id, &Type::String);
            solver.instantiate_all();
            LinearTerm::var(id)
        }
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
        Expr::Not(_) | Expr::Compare(_, _, _) | Expr::And(_, _) | Expr::Or(_, _) => {
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
        Expr::Call(_, _, _) => unreachable!("unfolded above"),
    }
}

/// Translates a Boolean expression or formula into a BDD [`Formula`] ([S4],
/// [I6], [I8]).
fn translate_formula(program: &Program, solver: &mut Solver, expr: &Expr, env: &Env) -> Formula {
    let unfolded = unfold_functions(program, expr);
    match &unfolded {
        Expr::Boolean(true) => bdd::TRUE,
        Expr::Boolean(false) => bdd::FALSE,
        Expr::Var(_, _)
        | Expr::Old(_, _)
        | Expr::Move(_, _)
        | Expr::Clone(_, _)
        | Expr::Borrow(_, _, _)
        | Expr::Field(_, _, _, _)
        | Expr::Index(_, _, _) => {
            let term = translate_term(program, solver, &unfolded, env);
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
            if *l.ty().deref_type() == Type::Bool {
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
                if let Type::Struct(sid, _) = l.ty().deref_type()
                    && matches!(cmp, Comparison::Equal | Comparison::NotEqual)
                {
                    ensure_struct_fields(program, solver, *sid, &tl);
                    ensure_struct_fields(program, solver, *sid, &tr);
                }
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

#[allow(clippy::too_many_arguments)]
fn check_index_bounds(
    solver: &mut Solver,
    base_term: &LinearTerm,
    idx_term: &LinearTerm,
    guard: Formula,
    control: &str,
    line: u32,
    visible_vars: &[(String, TermId, bool)],
    in_loop: bool,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
) -> Result<(), Error> {
    let len_id = solver.intern_term(AtomTerm::Len(base_term.clone()));
    solver.instantiate_all();
    let len_term = LinearTerm::var(len_id);
    let ge_zero = solver.le(&LinearTerm::from_i128(0), idx_term);
    let lt_len = solver.lt(idx_term, &len_term);
    let in_bounds = solver.bdd.and(ge_zero, lt_len);
    require_formula(
        solver,
        guard,
        in_bounds,
        control,
        line,
        "P5",
        "vector index may be out of bounds",
        visible_vars,
        in_loop,
        certificates,
    )?;
    solver.add_fact(guard, in_bounds);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_place_obligations(
    program: &Program,
    solver: &mut Solver,
    place: &Place,
    guard: Formula,
    env: &Env,
    control: &str,
    line: u32,
    visible_vars: &[(String, TermId, bool)],
    in_loop: bool,
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
) -> Result<(), Error> {
    let mut cur = env.lookup(VarRef::Local(place.root));
    for step in &place.steps {
        match step {
            PlaceStep::Field(sid, fidx, fty) => {
                cur = translate_field_term(solver, cur, *sid, *fidx, fty);
            }
            PlaceStep::Index(idx_expr, ety) => {
                check_expr_obligations(
                    program,
                    solver,
                    idx_expr,
                    guard,
                    env,
                    control,
                    line,
                    visible_vars,
                    in_loop,
                    certificates,
                )?;
                let idx_term = translate_term(program, solver, idx_expr, env);
                check_index_bounds(
                    solver,
                    &cur,
                    &idx_term,
                    guard,
                    control,
                    line,
                    visible_vars,
                    in_loop,
                    certificates,
                )?;
                cur = translate_index_term(solver, cur, idx_term, ety);
            }
        }
    }
    Ok(())
}

/// Checks `[P5]` (arithmetic range, non-zero divisor, vector index bounds) and
/// `[P3]` (function preconditions) for `expr` under history guard `guard`, in
/// the evaluation order of `[P7]` and `[E-Bool]`.
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
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Var(_, _)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => Ok(()),
        Expr::Move(inner, _)
        | Expr::Field(inner, _, _, _)
        | Expr::Len(inner)
        | Expr::Clone(inner, _)
        | Expr::Trim(inner)
        | Expr::Not(inner) => check_expr_obligations(
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
        Expr::Borrow(_, place, _) => check_place_obligations(
            program,
            solver,
            place,
            guard,
            env,
            control,
            line,
            visible_vars,
            in_loop,
            certificates,
        ),
        Expr::Index(base, idx, _) => {
            check_expr_obligations(
                program,
                solver,
                base,
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
                idx,
                guard,
                env,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )?;
            let base_term = translate_term(program, solver, base, env);
            let idx_term = translate_term(program, solver, idx, env);
            check_index_bounds(
                solver,
                &base_term,
                &idx_term,
                guard,
                control,
                line,
                visible_vars,
                in_loop,
                certificates,
            )
        }
        Expr::Struct(_, items, _) | Expr::Variant(_, _, items, _) | Expr::Vector(items, _) => {
            for item in items {
                check_expr_obligations(
                    program,
                    solver,
                    item,
                    guard,
                    env,
                    control,
                    line,
                    visible_vars,
                    in_loop,
                    certificates,
                )?;
            }
            Ok(())
        }
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
                    parameters: arg_terms.clone(),
                    old_parameters: arg_terms,
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

fn check_expr_pure_ownership(
    program: &Program,
    expr: &Expr,
    consumed: bool,
    control: &str,
    line: u32,
) -> Result<(), Error> {
    match expr {
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Var(_, _)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => Ok(()),
        Expr::Borrow(_, place, _) => {
            for step in &place.steps {
                if let PlaceStep::Index(idx, _) = step {
                    check_expr_pure_ownership(program, idx, false, control, line)?;
                }
            }
            Ok(())
        }
        Expr::Move(inner, _) => check_expr_pure_ownership(program, inner, true, control, line),
        Expr::Field(base, _, _, fty) => {
            check_expr_pure_ownership(program, base, false, control, line)?;
            if consumed && program.kind(fty) != Kind::Copy {
                return Err(Error {
                    control: control.to_string(),
                    line,
                    clause: "O3",
                    message: "cannot move out of a struct field of non-copy type".to_string(),
                    counterexample: None,
                });
            }
            Ok(())
        }
        Expr::Index(base, idx, ety) => {
            check_expr_pure_ownership(program, base, false, control, line)?;
            check_expr_pure_ownership(program, idx, false, control, line)?;
            if consumed && program.kind(ety) != Kind::Copy {
                return Err(Error {
                    control: control.to_string(),
                    line,
                    clause: "O3",
                    message: "cannot move out of a vector element of non-copy type".to_string(),
                    counterexample: None,
                });
            }
            Ok(())
        }
        Expr::Struct(_, items, ty) | Expr::Variant(_, _, items, ty) | Expr::Vector(items, ty) => {
            if !consumed && program.kind(ty) == Kind::Must {
                return Err(Error {
                    control: control.to_string(),
                    line,
                    clause: "O3a",
                    message: "temporary value of kind `must` cannot be discarded".to_string(),
                    counterexample: None,
                });
            }
            for item in items {
                check_expr_pure_ownership(program, item, true, control, line)?;
            }
            Ok(())
        }
        Expr::Len(inner)
        | Expr::Clone(inner, _)
        | Expr::Trim(inner)
        | Expr::Negate(inner, _)
        | Expr::Not(inner) => check_expr_pure_ownership(program, inner, false, control, line),
        Expr::Arithmetic(_, l, r, _)
        | Expr::Compare(_, l, r)
        | Expr::And(l, r)
        | Expr::Or(l, r) => {
            check_expr_pure_ownership(program, l, false, control, line)?;
            check_expr_pure_ownership(program, r, false, control, line)
        }
        Expr::Call(fn_id, args, _) => {
            let param_tys = &program.functions[*fn_id].parameters;
            for (i, arg) in args.iter().enumerate() {
                let lends = arg.lends()
                    || param_tys
                        .get(i)
                        .is_some_and(|p| matches!(p.ty, Type::Ref(_, _)));
                check_expr_pure_ownership(program, arg, !lends, control, line)?;
            }
            Ok(())
        }
    }
}

fn alloc_type_term_in(solver: &mut Solver, name: String, ty: &Type) -> TermId {
    let base_ty = ty.deref_type();
    let id = match base_ty {
        Type::Int(int_ty) => solver.fresh_var(name, Some(*int_ty), true),
        Type::Bool => {
            let id = solver.fresh_var(name, None, true);
            let t = LinearTerm::var(id);
            let lo = solver.le(&LinearTerm::from_i128(0), &t);
            let hi = solver.le(&t, &LinearTerm::from_i128(1));
            let both = solver.bdd.and(lo, hi);
            solver.add_structure(both);
            id
        }
        _ => solver.fresh_var(name, None, true),
    };
    solver.register_sort(id, base_ty);
    solver.instantiate_all();
    id
}

fn register_program_structs(program: &Program, solver: &mut Solver) {
    for (sid, s) in program.structs.iter().enumerate() {
        solver.register_struct_arity(sid, s.fields.len());
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
    register_program_structs(program, &mut solver);
    let mut param_terms = Vec::new();
    let mut visible_vars = Vec::new();

    for param in &function.parameters {
        let term_id = alloc_type_term_in(&mut solver, param.name.clone(), &param.ty);
        param_terms.push(LinearTerm::var(term_id));
        visible_vars.push((
            param.name.clone(),
            term_id,
            *param.ty.deref_type() == Type::Bool,
        ));
    }

    let env = Env {
        locals: HashMap::new(),
        parameters: param_terms.clone(),
        old_parameters: param_terms,
        fields: Vec::new(),
        constants: HashMap::new(),
    };

    if let Some(pre) = &function.precondition {
        let pre_formula = translate_formula(program, &mut solver, pre, &env);
        solver.add_fact(bdd::TRUE, pre_formula);
    }

    check_expr_pure_ownership(program, &function.body, true, &function.name, 1)?;
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
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => {}
        Expr::Var(VarRef::Local(id), _) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Expr::Var(_, _) => {}
        Expr::Borrow(_, place, _) => {
            if !out.contains(&place.root) {
                out.push(place.root);
            }
            for step in &place.steps {
                if let crate::typed::PlaceStep::Index(idx, _) = step {
                    collect_read_vars(idx, out);
                }
            }
        }
        Expr::Move(inner, _)
        | Expr::Field(inner, _, _, _)
        | Expr::Len(inner)
        | Expr::Clone(inner, _)
        | Expr::Trim(inner)
        | Expr::Negate(inner, _)
        | Expr::Not(inner) => collect_read_vars(inner, out),
        Expr::Index(l, r, _)
        | Expr::Arithmetic(_, l, r, _)
        | Expr::Compare(_, l, r)
        | Expr::And(l, r)
        | Expr::Or(l, r) => {
            collect_read_vars(l, out);
            collect_read_vars(r, out);
        }
        Expr::Struct(_, args, _)
        | Expr::Variant(_, _, args, _)
        | Expr::Vector(args, _)
        | Expr::Call(_, args, _) => {
            for arg in args {
                collect_read_vars(arg, out);
            }
        }
    }
}

fn collect_operand_modified(arg: &Expr, out: &mut HashSet<VarId>) {
    match arg {
        Expr::Borrow(BorrowMode::Exclusive, place, _) => {
            out.insert(place.root);
        }
        Expr::Var(VarRef::Local(id), Type::Ref(BorrowMode::Exclusive, _)) => {
            out.insert(*id);
        }
        _ => {}
    }
}

fn collect_modified_vars(body: &ControlBody, body_id: BodyId, out: &mut HashSet<VarId>) {
    for stmt in &body.bodies[body_id].statements {
        match &stmt.command {
            Command::Assign { var, .. } => {
                out.insert(*var);
            }
            Command::AssignPlace { target, .. } => {
                out.insert(target.root);
            }
            Command::Decide {
                op_kind, arguments, ..
            } => {
                if let Operation::Builtin {
                    target: Some(place),
                    ..
                } = op_kind
                {
                    out.insert(place.root);
                }
                for arg in arguments {
                    collect_operand_modified(arg, out);
                }
            }
            Command::Capture {
                operation,
                arguments,
                ..
            } => {
                if let Operation::Builtin {
                    target: Some(place),
                    ..
                } = operation
                {
                    out.insert(place.root);
                }
                for arg in arguments {
                    collect_operand_modified(arg, out);
                }
            }
            Command::Loop { body: inner, .. } => {
                collect_modified_vars(body, *inner, out);
            }
            Command::Boundary {
                body: b1,
                contingency: b2,
                ..
            } => {
                collect_modified_vars(body, *b1, out);
                collect_modified_vars(body, *b2, out);
            }
            _ => {}
        }
    }
}

/// Identifies single-binder immutable locals bound to constant expressions ([I3]).
fn find_constant_locals(
    program: &Program,
    body: &ControlBody,
    param_count: usize,
) -> HashMap<VarId, Int> {
    let mut bind_counts: HashMap<VarId, usize> = HashMap::new();
    let mut bind_exprs: HashMap<VarId, Expr> = HashMap::new();
    for (idx, _) in body.vars.iter().enumerate() {
        bind_counts.insert(idx, 0);
    }
    for param_idx in 0..param_count.min(body.vars.len()) {
        // Parameters of the control/boundary are not constant expressions ([I3]).
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
                    if !matches!(value, Expr::Borrow(BorrowMode::Exclusive, _, _)) {
                        bind_exprs.insert(*var, value.clone());
                    }
                }
                Command::Unpack { vars, .. } => {
                    for &v in vars {
                        *bind_counts.entry(v).or_insert(0) += 2;
                    }
                }
                Command::Capture { var, .. } => {
                    *bind_counts.entry(*var).or_insert(0) += 2;
                }
                Command::Boundary {
                    contingency_vars,
                    message_var,
                    ..
                } => {
                    for &v in contingency_vars {
                        *bind_counts.entry(v).or_insert(0) += 2;
                    }
                    if let Some(mv) = message_var {
                        *bind_counts.entry(*mv).or_insert(0) += 2;
                    }
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
                    _ => false,
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

#[derive(Clone)]
struct CapturedInfo {
    operation: Operation,
    old_args: Vec<LinearTerm>,
    post_args: Vec<LinearTerm>,
}

#[derive(Clone)]
enum FrozenStep {
    Field(StructId, usize, Type),
    Index(LinearTerm, Type),
}

#[derive(Clone)]
struct FrozenPlace {
    root: VarId,
    root_ty: Type,
    steps: Vec<FrozenStep>,
}

type LeaveRecord = (
    usize,
    Formula,
    Vec<LinearTerm>,
    Vec<Vec<(Formula, TermId)>>,
    Vec<Formula>,
    Vec<Formula>,
);

struct BodyChecker<'a> {
    program: &'a Program,
    owner_name: String,
    allowed_uses: Vec<String>,
    contract: Vec<Alternative>,
    param_types: Vec<Type>,
    body: &'a ControlBody,
    boundary_may_fault: &'a [bool],
    solver: Solver,
    /// BDD variable index for each `(SiteId, alternative_position)`.
    site_atoms: Vec<Vec<usize>>,
    /// `D(x)` for each `VarId` ([7.4]).
    defined: Vec<Formula>,
    /// `M(x)` for each `VarId` ([7.4]).
    moved: Vec<Formula>,
    /// Disjoint `(history_guard, logical_term_id)` versions of each `VarId` ([I1], [N5]).
    versions: Vec<Vec<(Formula, TermId)>>,
    /// Entry logical terms for the control/boundary parameters ([P4]).
    entry_parameters: Vec<LinearTerm>,
    /// Constant local map ([I3]).
    constants: HashMap<VarId, Int>,
    /// Captured operation metadata for outcome variables ([I12]).
    captured: HashMap<VarId, Vec<(Formula, CapturedInfo)>>,
    /// Frozen place abbreviated by a local borrow variable ([I7], [I9]).
    local_borrows: HashMap<VarId, FrozenPlace>,
    /// Entry availability snapshot `D(v) ∧ ¬M(v)` per loop body ([O6]).
    loop_entry_avail: HashMap<BodyId, Vec<Formula>>,
    /// Stack of enclosing active body scopes (outermost first).
    active_scopes: Vec<BodyId>,
    /// Whether this scope permits faults (`faults` signature or inside `B₁`).
    permits_faults: bool,
    /// Whether currently checking a `contingency` body `B₂` ([F5], [F6]).
    in_contingency: bool,
    /// Retained variables in a named boundary `B₁` ([F6]).
    retained_vars: Vec<VarId>,
    /// Set to `true` when checking a named boundary's `B₂` and a feasible
    /// `escalate` or `fault` occurs ([F6]).
    boundary_can_fault: bool,
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
        alloc_type_term_in(&mut self.solver, info.name.clone(), &info.ty)
    }

    fn alloc_type_term(&mut self, name: &str, ty: &Type) -> TermId {
        alloc_type_term_in(&mut self.solver, name.to_string(), ty)
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

    fn avail(&mut self, var: VarId) -> Formula {
        let not_moved = self.solver.bdd.not(self.moved[var]);
        self.solver.bdd.and(self.defined[var], not_moved)
    }

    fn is_feasible(
        &mut self,
        guard: Formula,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> bool {
        if guard == bdd::FALSE {
            return false;
        }
        matches!(
            self.solver.decide(guard, visible_vars, in_loop),
            Decision::Refuted(_)
        )
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
                *self.body.vars[var].ty.deref_type() == Type::Bool,
            ));
        }
        (
            Env {
                locals,
                parameters: self.entry_parameters.clone(),
                old_parameters: self.entry_parameters.clone(),
                fields: Vec::new(),
                constants: self.constants.clone(),
            },
            visible,
        )
    }

    fn check_available(
        &mut self,
        var: VarId,
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        let avail_var = self.avail(var);
        let msg = if self.moved[var] != bdd::FALSE {
            format!(
                "`{}` may not be available (unbound or already moved) on this history",
                self.body.vars[var].name
            )
        } else {
            format!(
                "`{}` may not be bound on this history",
                self.body.vars[var].name
            )
        };
        require_formula(
            &mut self.solver,
            r_i,
            avail_var,
            &self.owner_name,
            line,
            "O2",
            msg,
            visible_vars,
            in_loop,
            self.certificates,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn check_consume_expr(
        &mut self,
        expr: &Expr,
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        match expr {
            Expr::Var(VarRef::Local(id), ty) => {
                self.check_available(*id, r_i, line, visible_vars, in_loop)?;
                if self.program.kind(ty) != Kind::Copy {
                    self.moved[*id] = self.solver.bdd.or(self.moved[*id], r_i);
                }
                Ok(())
            }
            Expr::Move(inner, _) => {
                self.check_consume_expr(inner, r_i, line, visible_vars, in_loop)
            }
            Expr::Field(base, _, _, fty) => {
                self.check_read_expr(base, r_i, line, visible_vars, in_loop)?;
                if self.program.kind(fty) != Kind::Copy
                    && self.is_feasible(r_i, visible_vars, in_loop)
                {
                    return Err(Error {
                        control: self.owner_name.clone(),
                        line,
                        clause: "O3",
                        message: "cannot move out of a struct field of non-copy type".to_string(),
                        counterexample: None,
                    });
                }
                Ok(())
            }
            Expr::Index(base, idx, ety) => {
                self.check_read_expr(base, r_i, line, visible_vars, in_loop)?;
                self.check_read_expr(idx, r_i, line, visible_vars, in_loop)?;
                if self.program.kind(ety) != Kind::Copy
                    && self.is_feasible(r_i, visible_vars, in_loop)
                {
                    return Err(Error {
                        control: self.owner_name.clone(),
                        line,
                        clause: "O3",
                        message: "cannot move out of a vector element of non-copy type".to_string(),
                        counterexample: None,
                    });
                }
                Ok(())
            }
            Expr::Struct(_, items, _) | Expr::Variant(_, _, items, _) | Expr::Vector(items, _) => {
                for item in items {
                    self.check_consume_expr(item, r_i, line, visible_vars, in_loop)?;
                }
                Ok(())
            }
            Expr::Borrow(_, place, _) => {
                self.check_available(place.root, r_i, line, visible_vars, in_loop)?;
                for step in &place.steps {
                    if let PlaceStep::Index(idx, _) = step {
                        self.check_read_expr(idx, r_i, line, visible_vars, in_loop)?;
                    }
                }
                Ok(())
            }
            other => self.check_read_expr(other, r_i, line, visible_vars, in_loop),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_read_expr(
        &mut self,
        expr: &Expr,
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        match expr {
            Expr::Boolean(_)
            | Expr::Integer(_, _)
            | Expr::String(_)
            | Expr::Old(_, _)
            | Expr::EmptyMap(_) => Ok(()),
            Expr::Var(VarRef::Local(id), _) => {
                self.check_available(*id, r_i, line, visible_vars, in_loop)
            }
            Expr::Var(_, _) => Ok(()),
            Expr::Borrow(_, place, _) => {
                self.check_available(place.root, r_i, line, visible_vars, in_loop)?;
                for step in &place.steps {
                    if let PlaceStep::Index(idx, _) = step {
                        self.check_read_expr(idx, r_i, line, visible_vars, in_loop)?;
                    }
                }
                Ok(())
            }
            Expr::Struct(_, items, ty)
            | Expr::Variant(_, _, items, ty)
            | Expr::Vector(items, ty) => {
                if self.program.kind(ty) == Kind::Must
                    && self.is_feasible(r_i, visible_vars, in_loop)
                {
                    return Err(Error {
                        control: self.owner_name.clone(),
                        line,
                        clause: "O3a",
                        message: "temporary value of kind `must` cannot be discarded".to_string(),
                        counterexample: None,
                    });
                }
                for item in items {
                    self.check_consume_expr(item, r_i, line, visible_vars, in_loop)?;
                }
                Ok(())
            }
            Expr::Move(inner, _)
            | Expr::Field(inner, _, _, _)
            | Expr::Len(inner)
            | Expr::Clone(inner, _)
            | Expr::Trim(inner)
            | Expr::Negate(inner, _)
            | Expr::Not(inner) => self.check_read_expr(inner, r_i, line, visible_vars, in_loop),
            Expr::Index(l, r, _)
            | Expr::Arithmetic(_, l, r, _)
            | Expr::Compare(_, l, r)
            | Expr::And(l, r)
            | Expr::Or(l, r) => {
                self.check_read_expr(l, r_i, line, visible_vars, in_loop)?;
                self.check_read_expr(r, r_i, line, visible_vars, in_loop)
            }
            Expr::Call(_, args, _) => {
                for arg in args {
                    self.check_read_expr(arg, r_i, line, visible_vars, in_loop)?;
                }
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
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
            self.check_available(var, r_i, line, visible_vars, in_loop)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn check_operands_o0(
        &mut self,
        arguments: &[Expr],
        param_types: Option<&[Type]>,
        r_i: Formula,
        env: &Env,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        let is_lending = |i: usize, arg: &Expr| -> bool {
            arg.lends()
                || param_types
                    .and_then(|tys| tys.get(i))
                    .is_some_and(|ty| matches!(ty, Type::Ref(_, _)))
        };
        // [O0] Pass 1: non-lending operands left to right, each consumed before the next.
        for (i, arg) in arguments.iter().enumerate() {
            if !is_lending(i, arg) {
                check_expr_obligations(
                    self.program,
                    &mut self.solver,
                    arg,
                    r_i,
                    env,
                    &self.owner_name,
                    line,
                    visible_vars,
                    in_loop,
                    self.certificates,
                )?;
                self.check_consume_expr(arg, r_i, line, visible_vars, in_loop)?;
            }
        }
        // [O0] Pass 2: lending operands left to right.
        for (i, arg) in arguments.iter().enumerate() {
            if is_lending(i, arg) {
                check_expr_obligations(
                    self.program,
                    &mut self.solver,
                    arg,
                    r_i,
                    env,
                    &self.owner_name,
                    line,
                    visible_vars,
                    in_loop,
                    self.certificates,
                )?;
                self.check_read_expr(arg, r_i, line, visible_vars, in_loop)?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn check_scope_obligations(
        &mut self,
        scope_bodies: &[BodyId],
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        if r_i == bdd::FALSE {
            return Ok(());
        }
        for var in 0..self.body.vars.len() {
            if !scope_bodies.contains(&self.body.vars[var].body) {
                continue;
            }
            if self.program.kind(&self.body.vars[var].ty) != Kind::Must {
                continue;
            }
            let avail_var = self.avail(var);
            let live_must = self.solver.bdd.and(r_i, avail_var);
            require_entailment(
                &mut self.solver,
                live_must,
                &self.owner_name,
                line,
                "O5",
                format!(
                    "`{}` holds an unconsumed obligation when its scope ends",
                    self.body.vars[var].name
                ),
                visible_vars,
                in_loop,
                self.certificates,
            )?;
        }
        Ok(())
    }

    fn check_may_fault(
        &mut self,
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        // [F1]: Control or enclosing boundary permits faults.
        if !self.permits_faults {
            require_entailment(
                &mut self.solver,
                r_i,
                &self.owner_name,
                line,
                "F1",
                format!(
                    "`{}` may fault here without declaring `faults`",
                    self.owner_name
                ),
                visible_vars,
                in_loop,
                self.certificates,
            )?;
        }
        // [F6]: In a named boundary's `B₁`, every retained parameter stays available at a fault.
        for &ret_var in &self.retained_vars.clone() {
            let avail_ret = self.avail(ret_var);
            require_formula(
                &mut self.solver,
                r_i,
                avail_ret,
                &self.owner_name,
                line,
                "F6",
                format!(
                    "retained parameter `{}` may already be moved where a fault can occur",
                    self.body.vars[ret_var].name
                ),
                visible_vars,
                in_loop,
                self.certificates,
            )?;
        }
        // [F2]: Every non-fault-releasable name in scope must already be consumed.
        for var in 0..self.body.vars.len() {
            if !self.active_scopes.contains(&self.body.vars[var].body) {
                continue;
            }
            if self.retained_vars.contains(&var) {
                continue;
            }
            if self.program.is_fault_releasable(&self.body.vars[var].ty) {
                continue;
            }
            let avail_var = self.avail(var);
            let live_non_rel = self.solver.bdd.and(r_i, avail_var);
            require_entailment(
                &mut self.solver,
                live_non_rel,
                &self.owner_name,
                line,
                "F2",
                format!(
                    "`{}` holds a non-fault-releasable value where a fault can occur",
                    self.body.vars[var].name
                ),
                visible_vars,
                in_loop,
                self.certificates,
            )?;
        }
        Ok(())
    }

    fn check_effects(
        &mut self,
        effects: &[String],
        op_name: &str,
        r_i: Formula,
        line: u32,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        let mut checked_feasible = None;
        for effect in effects {
            if !self.allowed_uses.contains(effect) {
                let feasible = *checked_feasible
                    .get_or_insert_with(|| self.is_feasible(r_i, visible_vars, in_loop));
                if feasible {
                    return Err(Error {
                        control: self.owner_name.clone(),
                        line,
                        clause: "F3",
                        message: format!(
                            "`{op_name}` uses effect `{effect}`, which `{}` does not allow",
                            self.owner_name
                        ),
                        counterexample: None,
                    });
                }
            }
        }
        Ok(())
    }

    fn bind_value_term(&mut self, var: VarId, r_i: Formula, val_term: LinearTerm) -> TermId {
        let term_id = self.alloc_var_term(var);
        if *self.body.vars[var].ty.deref_type() == Type::Bool {
            let b = self.solver.le(&LinearTerm::from_i128(1), &val_term);
            self.solver.add_def_bool(r_i, term_id, b);
        } else {
            self.solver.add_def_linear(r_i, term_id, val_term);
        }
        self.set_version(var, r_i, term_id);
        term_id
    }

    fn bind_value(&mut self, var: VarId, r_i: Formula, expr: &Expr, env: &Env) -> TermId {
        let term_id = self.alloc_var_term(var);
        if *self.body.vars[var].ty.deref_type() == Type::Bool {
            let phi = translate_formula(self.program, &mut self.solver, expr, env);
            self.solver.add_def_bool(r_i, term_id, phi);
        } else {
            let val_term = translate_term(self.program, &mut self.solver, expr, env);
            self.solver.add_def_linear(r_i, term_id, val_term);
        }
        self.set_version(var, r_i, term_id);
        term_id
    }

    fn apply_place_write(&mut self, place: &Place, guard: Formula, new_val: LinearTerm, env: &Env) {
        if guard == bdd::FALSE {
            return;
        }
        if let Some(frozen) = self.local_borrows.get(&place.root).cloned() {
            let cur_root_term = env
                .locals
                .get(&frozen.root)
                .cloned()
                .unwrap_or_else(|| LinearTerm::from_i128(0));
            let updated_sub = if place.steps.is_empty() {
                new_val.clone()
            } else {
                let mut sub_term = cur_root_term.clone();
                let mut sub_ty = frozen.root_ty.deref_type().clone();
                for fstep in &frozen.steps {
                    match fstep {
                        FrozenStep::Field(sid, fidx, fty) => {
                            sub_term =
                                translate_field_term(&mut self.solver, sub_term, *sid, *fidx, fty);
                            sub_ty = fty.clone();
                        }
                        FrozenStep::Index(idx_t, ety) => {
                            sub_term = translate_index_term(
                                &mut self.solver,
                                sub_term,
                                idx_t.clone(),
                                ety,
                            );
                            sub_ty = ety.clone();
                        }
                    }
                }
                functional_update(
                    self.program,
                    &mut self.solver,
                    sub_term,
                    &sub_ty,
                    &place.steps,
                    new_val.clone(),
                    env,
                )
            };
            let new_root = functional_update_frozen(
                self.program,
                &mut self.solver,
                cur_root_term,
                frozen.root_ty.deref_type(),
                &frozen.steps,
                updated_sub,
            );
            let _ = self.bind_value_term(frozen.root, guard, new_root);
            let _ = self.bind_value_term(place.root, guard, new_val);
            return;
        }
        let root_term = env
            .locals
            .get(&place.root)
            .cloned()
            .unwrap_or_else(|| LinearTerm::from_i128(0));
        let root_ty = self.body.vars[place.root].ty.deref_type().clone();
        let updated = functional_update(
            self.program,
            &mut self.solver,
            root_term,
            &root_ty,
            &place.steps,
            new_val,
            env,
        );
        let _ = self.bind_value_term(place.root, guard, updated);
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_builtin_facts(
        &mut self,
        op: BuiltinOp,
        target: Option<&Place>,
        arg_terms: &[LinearTerm],
        alt_pos: usize,
        atom: Formula,
        field_terms: &[LinearTerm],
        env: &Env,
    ) {
        match op {
            BuiltinOp::Push => {
                if alt_pos == 0
                    && let Some(place) = target
                {
                    let v_old = translate_place(self.program, &mut self.solver, place, env);
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(v_old));
                    let v_new_tid = self.alloc_type_term("_vec", &place.ty);
                    let v_new = LinearTerm::var(v_new_tid);
                    let len_new_id = self.solver.intern_term(AtomTerm::Len(v_new.clone()));
                    self.solver.instantiate_all();
                    let plus_one = LinearTerm::var(len_old_id).add(&LinearTerm::from_i128(1));
                    let eq_len = self.solver.eq(&LinearTerm::var(len_new_id), &plus_one);
                    self.solver.add_fact(atom, eq_len);
                    self.apply_place_write(place, atom, v_new, env);
                }
            }
            BuiltinOp::Pop => {
                if let Some(place) = target {
                    let v_old = translate_place(self.program, &mut self.solver, place, env);
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(v_old));
                    self.solver.instantiate_all();
                    let len_old = LinearTerm::var(len_old_id);
                    if alt_pos == 0 {
                        let non_empty = self.solver.le(&LinearTerm::from_i128(1), &len_old);
                        self.solver.add_fact(atom, non_empty);
                        let v_new_tid = self.alloc_type_term("_vec", &place.ty);
                        let v_new = LinearTerm::var(v_new_tid);
                        let len_new_id = self.solver.intern_term(AtomTerm::Len(v_new.clone()));
                        self.solver.instantiate_all();
                        let minus_one = len_old.sub(&LinearTerm::from_i128(1));
                        let eq_len = self.solver.eq(&LinearTerm::var(len_new_id), &minus_one);
                        self.solver.add_fact(atom, eq_len);
                        self.apply_place_write(place, atom, v_new, env);
                    } else {
                        let is_empty = self.solver.eq(&len_old, &LinearTerm::from_i128(0));
                        self.solver.add_fact(atom, is_empty);
                    }
                }
            }
            BuiltinOp::Take => {
                if let Some(v_old) = arg_terms.first() {
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(v_old.clone()));
                    self.solver.instantiate_all();
                    let len_old = LinearTerm::var(len_old_id);
                    if alt_pos == 0 {
                        let non_empty = self.solver.le(&LinearTerm::from_i128(1), &len_old);
                        self.solver.add_fact(atom, non_empty);
                        if let Some(tail) = field_terms.get(1) {
                            let len_tail_id = self.solver.intern_term(AtomTerm::Len(tail.clone()));
                            self.solver.instantiate_all();
                            let minus_one = len_old.sub(&LinearTerm::from_i128(1));
                            let eq_len = self.solver.eq(&LinearTerm::var(len_tail_id), &minus_one);
                            self.solver.add_fact(atom, eq_len);
                        }
                    } else {
                        let is_empty = self.solver.eq(&len_old, &LinearTerm::from_i128(0));
                        self.solver.add_fact(atom, is_empty);
                    }
                }
            }
            BuiltinOp::Insert => {
                if let Some(place) = target {
                    let m_old = translate_place(self.program, &mut self.solver, place, env);
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(m_old));
                    self.solver.instantiate_all();
                    let len_old = LinearTerm::var(len_old_id);
                    let m_new_tid = self.alloc_type_term("_map", &place.ty);
                    let m_new = LinearTerm::var(m_new_tid);
                    let len_new_id = self.solver.intern_term(AtomTerm::Len(m_new.clone()));
                    self.solver.instantiate_all();
                    if alt_pos == 0 {
                        let plus_one = len_old.add(&LinearTerm::from_i128(1));
                        let eq_len = self.solver.eq(&LinearTerm::var(len_new_id), &plus_one);
                        self.solver.add_fact(atom, eq_len);
                    } else {
                        let non_empty = self.solver.le(&LinearTerm::from_i128(1), &len_old);
                        let eq_len = self.solver.eq(&LinearTerm::var(len_new_id), &len_old);
                        let both = self.solver.bdd.and(non_empty, eq_len);
                        self.solver.add_fact(atom, both);
                    }
                    self.apply_place_write(place, atom, m_new, env);
                }
            }
            BuiltinOp::Remove => {
                if let Some(place) = target
                    && alt_pos == 0
                {
                    let m_old = translate_place(self.program, &mut self.solver, place, env);
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(m_old));
                    self.solver.instantiate_all();
                    let len_old = LinearTerm::var(len_old_id);
                    let non_empty = self.solver.le(&LinearTerm::from_i128(1), &len_old);
                    let m_new_tid = self.alloc_type_term("_map", &place.ty);
                    let m_new = LinearTerm::var(m_new_tid);
                    let len_new_id = self.solver.intern_term(AtomTerm::Len(m_new.clone()));
                    self.solver.instantiate_all();
                    let minus_one = len_old.sub(&LinearTerm::from_i128(1));
                    let eq_len = self.solver.eq(&LinearTerm::var(len_new_id), &minus_one);
                    let both = self.solver.bdd.and(non_empty, eq_len);
                    self.solver.add_fact(atom, both);
                    self.apply_place_write(place, atom, m_new, env);
                }
            }
            BuiltinOp::Get => {
                if alt_pos == 0
                    && let Some(m_old) = arg_terms.first()
                {
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(m_old.clone()));
                    self.solver.instantiate_all();
                    let non_empty = self
                        .solver
                        .le(&LinearTerm::from_i128(1), &LinearTerm::var(len_old_id));
                    self.solver.add_fact(atom, non_empty);
                }
            }
            BuiltinOp::EntryAt => {
                if let (Some(m_old), Some(pos)) = (arg_terms.first(), arg_terms.get(1)) {
                    let len_old_id = self.solver.intern_term(AtomTerm::Len(m_old.clone()));
                    self.solver.instantiate_all();
                    let len_old = LinearTerm::var(len_old_id);
                    if alt_pos == 0 {
                        let in_range = self.solver.lt(pos, &len_old);
                        self.solver.add_fact(atom, in_range);
                        if let Some(next_term) = field_terms.get(2) {
                            let plus_one = pos.add(&LinearTerm::from_i128(1));
                            let eq_next = self.solver.eq(next_term, &plus_one);
                            self.solver.add_fact(atom, eq_next);
                        }
                    } else {
                        let at_end = self.solver.le(&len_old, pos);
                        self.solver.add_fact(atom, at_end);
                    }
                }
            }
            BuiltinOp::Emit => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn warn_unreachable_arms(
        &mut self,
        op_name: &str,
        line: u32,
        statements: &[crate::typed::Statement],
        contract: &[Alternative],
        arms: &[crate::typed::Arm],
        arm_atoms: &[Formula],
        is_pure: bool,
        always_check: bool,
        r_i: Formula,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) {
        let mut checked_r_i_feasible: Option<bool> = None;
        for (pos, arm) in arms.iter().enumerate() {
            let atom = arm_atoms[pos];
            let contract_alt = &contract[arm.alternative];
            if !always_check && contract_alt.condition.is_none() && !is_pure {
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
            let r_i_feasible = *checked_r_i_feasible
                .get_or_insert_with(|| self.is_feasible(r_i, visible_vars, in_loop));
            if !r_i_feasible {
                break;
            }
            if let Decision::Proved(cert) = self.solver.decide(atom, visible_vars, in_loop) {
                kernel::validate(&cert).expect("valid U16 certificate");
                self.certificates
                    .push((self.owner_name.clone(), line, "U16", cert));
                self.warnings.push(crate::surface::Warning {
                    control: self.owner_name.clone(),
                    line,
                    clause: "U16",
                    message: format!(
                        "the alternative `{}` of `{op_name}` cannot happen here; mark its arm `→ ⊥`",
                        contract_alt.name
                    ),
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_loop_repeat_scope(
        &mut self,
        target_body: BodyId,
        r_i: Formula,
        line: u32,
        label: &str,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Result<(), Error> {
        let Some(pos) = self.active_scopes.iter().position(|&b| b == target_body) else {
            return Ok(());
        };
        let ended: Vec<BodyId> = self.active_scopes[pos..].to_vec();
        self.check_scope_obligations(&ended, r_i, line, visible_vars, in_loop)?;
        let outer_bodies: Vec<BodyId> = self.active_scopes[..pos].to_vec();
        if let Some(entry_avail) = self.loop_entry_avail.get(&target_body).cloned() {
            for (var, &entry_a) in entry_avail.iter().enumerate().take(self.body.vars.len()) {
                if !outer_bodies.contains(&self.body.vars[var].body) {
                    continue;
                }
                let cur_a = self.avail(var);
                let eq_entry = self.solver.bdd.iff(cur_a, entry_a);
                require_formula(
                    &mut self.solver,
                    r_i,
                    eq_entry,
                    &self.owner_name,
                    line,
                    "O6",
                    format!(
                        "outer `{}` does not match its loop-entry availability at `{label}`",
                        self.body.vars[var].name
                    ),
                    visible_vars,
                    in_loop,
                    self.certificates,
                )?;
            }
        }
        Ok(())
    }

    fn check_body(
        &mut self,
        body_id: BodyId,
        live: Formula,
        in_loop: bool,
        loop_leaves: &mut HashMap<BodyId, Vec<LeaveRecord>>,
    ) -> Result<(), Error> {
        self.check_body_inner(body_id, live, in_loop, loop_leaves, false)
            .map(|_| ())
    }

    fn check_body_inner(
        &mut self,
        body_id: BodyId,
        mut live: Formula,
        in_loop: bool,
        loop_leaves: &mut HashMap<BodyId, Vec<LeaveRecord>>,
        allow_fallthrough: bool,
    ) -> Result<Formula, Error> {
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
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        value,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.check_consume_expr(value, r_i, line, &visible_vars, in_loop)?;
                    // [O1]: Θ ⊨ ¬(Rᵢ ∧ D(x))
                    let d_var = self.defined[*var];
                    let double_bind = self.solver.bdd.and(r_i, d_var);
                    require_entailment(
                        &mut self.solver,
                        double_bind,
                        &self.owner_name,
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
                    let not_ri = self.solver.bdd.not(r_i);
                    self.moved[*var] = self.solver.bdd.and(self.moved[*var], not_ri);
                    if let Expr::Borrow(_, place, _) = value {
                        let frozen = freeze_place(self.program, &mut self.solver, place, &env);
                        self.local_borrows.insert(*var, frozen);
                    }
                }
                Command::Unpack {
                    struct_id,
                    vars,
                    value,
                } => {
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        value,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.check_consume_expr(value, r_i, line, &visible_vars, in_loop)?;
                    let val_term = translate_term(self.program, &mut self.solver, value, &env);
                    let mut field_terms = Vec::with_capacity(vars.len());
                    for (fidx, &var_id) in vars.iter().enumerate() {
                        let d_var = self.defined[var_id];
                        let double_bind = self.solver.bdd.and(r_i, d_var);
                        require_entailment(
                            &mut self.solver,
                            double_bind,
                            &self.owner_name,
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
                        let fty = self.program.structs[*struct_id].fields[fidx].ty.clone();
                        let ft = translate_field_term(
                            &mut self.solver,
                            val_term.clone(),
                            *struct_id,
                            fidx,
                            &fty,
                        );
                        let tid = self.bind_value_term(var_id, r_i, ft);
                        self.defined[var_id] = self.solver.bdd.or(self.defined[var_id], r_i);
                        let not_ri = self.solver.bdd.not(r_i);
                        self.moved[var_id] = self.solver.bdd.and(self.moved[var_id], not_ri);
                        field_terms.push(LinearTerm::var(tid));
                    }
                    let cid = self
                        .solver
                        .intern_term(AtomTerm::StructCtor(*struct_id, field_terms));
                    self.solver.instantiate_all();
                    let eq = self.solver.eq(&val_term, &LinearTerm::var(cid));
                    self.solver.add_fact(r_i, eq);
                }
                Command::Assign { var, value } => {
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        value,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.check_consume_expr(value, r_i, line, &visible_vars, in_loop)?;
                    // [O4]: Θ ⊨ Rᵢ → D(x)
                    let d_var = self.defined[*var];
                    require_formula(
                        &mut self.solver,
                        r_i,
                        d_var,
                        &self.owner_name,
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
                    if self.program.kind(self.body.vars[*var].ty.deref_type()) == Kind::Must {
                        let m_var = self.moved[*var];
                        require_formula(
                            &mut self.solver,
                            r_i,
                            m_var,
                            &self.owner_name,
                            line,
                            "O4",
                            format!(
                                "`{}` holds an unconsumed `must` value and cannot be overwritten",
                                self.body.vars[*var].name
                            ),
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    if matches!(self.body.vars[*var].ty, Type::Ref(BorrowMode::Exclusive, _)) {
                        let new_val = translate_term(self.program, &mut self.solver, value, &env);
                        let place = Place {
                            root: *var,
                            root_ty: self.body.vars[*var].ty.clone(),
                            steps: Vec::new(),
                            ty: self.body.vars[*var].ty.deref_type().clone(),
                        };
                        self.apply_place_write(&place, r_i, new_val, &env);
                    } else {
                        self.bind_value(*var, r_i, value, &env);
                    }
                    let not_ri = self.solver.bdd.not(r_i);
                    self.moved[*var] = self.solver.bdd.and(self.moved[*var], not_ri);
                }
                Command::AssignPlace { target, value } => {
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        value,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.check_consume_expr(value, r_i, line, &visible_vars, in_loop)?;
                    self.check_available(target.root, r_i, line, &visible_vars, in_loop)?;
                    for step in &target.steps {
                        if let PlaceStep::Index(idx_expr, _) = step {
                            self.check_read_expr(idx_expr, r_i, line, &visible_vars, in_loop)?;
                        }
                    }
                    check_place_obligations(
                        self.program,
                        &mut self.solver,
                        target,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    if self.program.kind(&target.ty) == Kind::Must
                        && self.is_feasible(r_i, &visible_vars, in_loop)
                    {
                        return Err(Error {
                            control: self.owner_name.clone(),
                            line,
                            clause: "O4",
                            message: "cannot assign to a place of kind `must`".to_string(),
                            counterexample: None,
                        });
                    }
                    let new_val = translate_term(self.program, &mut self.solver, value, &env);
                    self.apply_place_write(target, r_i, new_val, &env);
                }
                Command::Oblige(formula) => {
                    self.check_reads(formula, r_i, line, &visible_vars, in_loop)?;
                    let phi = translate_formula(self.program, &mut self.solver, formula, &env);
                    // [P1]: Θ; Rᵢ ⊨ φ
                    require_formula(
                        &mut self.solver,
                        r_i,
                        phi,
                        &self.owner_name,
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
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        condition,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.check_read_expr(condition, r_i, line, &visible_vars, in_loop)?;
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
                Command::Capture {
                    var,
                    outcome_id: _,
                    operation,
                    arguments,
                } => {
                    let old_args: Vec<LinearTerm> = arguments
                        .iter()
                        .map(|a| translate_term(self.program, &mut self.solver, a, &env))
                        .collect();
                    let cap_param_tys: Option<Vec<Type>> = match operation {
                        Operation::Control(cid) => Some(
                            self.program.controls[*cid]
                                .parameters
                                .iter()
                                .map(|p| p.ty.clone())
                                .collect(),
                        ),
                        _ => None,
                    };
                    self.check_operands_o0(
                        arguments,
                        cap_param_tys.as_deref(),
                        r_i,
                        &env,
                        line,
                        &visible_vars,
                        in_loop,
                    )?;

                    let mut post_args = old_args.clone();
                    match operation {
                        Operation::Control(cid) => {
                            let callee = &self.program.controls[*cid];
                            if self.in_contingency
                                && !callee.contingent
                                && self.is_feasible(r_i, &visible_vars, in_loop)
                            {
                                return Err(Error {
                                    control: self.owner_name.clone(),
                                    line,
                                    clause: "F5",
                                    message: format!(
                                        "contingency may not call non-contingent `{}`",
                                        callee.name
                                    ),
                                    counterexample: None,
                                });
                            }
                            if callee.faults {
                                self.check_may_fault(r_i, line, &visible_vars, in_loop)?;
                            }
                            let uses = callee.uses.clone();
                            let callee_name = callee.name.clone();
                            self.check_effects(
                                &uses,
                                &callee_name,
                                r_i,
                                line,
                                &visible_vars,
                                in_loop,
                            )?;

                            if let Some(pre) = &self.program.controls[*cid].precondition.clone() {
                                let call_env = Env {
                                    locals: HashMap::new(),
                                    parameters: old_args.clone(),
                                    old_parameters: old_args.clone(),
                                    fields: Vec::new(),
                                    constants: HashMap::new(),
                                };
                                let pre_formula = translate_formula(
                                    self.program,
                                    &mut self.solver,
                                    pre,
                                    &call_env,
                                );
                                require_formula(
                                    &mut self.solver,
                                    r_i,
                                    pre_formula,
                                    &self.owner_name,
                                    line,
                                    "P3",
                                    format!(
                                        "precondition of `{}` may not hold",
                                        self.program.controls[*cid].name
                                    ),
                                    &visible_vars,
                                    in_loop,
                                    self.certificates,
                                )?;
                                self.solver.add_fact(r_i, pre_formula);
                            }

                            let param_tys: Vec<Type> = self.program.controls[*cid]
                                .parameters
                                .iter()
                                .map(|p| p.ty.clone())
                                .collect();
                            for (pidx, (arg, pty)) in arguments.iter().zip(&param_tys).enumerate() {
                                if matches!(pty, Type::Ref(BorrowMode::Exclusive, _)) {
                                    let inner_ty = pty.deref_type();
                                    let z_tid = self.alloc_type_term("_mut", inner_ty);
                                    let z_term = LinearTerm::var(z_tid);
                                    post_args[pidx] = z_term.clone();
                                    match arg {
                                        Expr::Borrow(BorrowMode::Exclusive, place, _) => {
                                            self.apply_place_write(place, r_i, z_term, &env);
                                        }
                                        Expr::Var(VarRef::Local(vid), _) => {
                                            let place = Place {
                                                root: *vid,
                                                root_ty: pty.clone(),
                                                steps: Vec::new(),
                                                ty: inner_ty.clone(),
                                            };
                                            self.apply_place_write(&place, r_i, z_term, &env);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Operation::Builtin {
                            op: BuiltinOp::Emit,
                            ..
                        } => {
                            let uses = vec!["console.write".to_string()];
                            self.check_effects(&uses, "emit", r_i, line, &visible_vars, in_loop)?;
                        }
                        _ => {}
                    }

                    // [O1] on `var`.
                    let d_var = self.defined[*var];
                    let double_bind = self.solver.bdd.and(r_i, d_var);
                    require_entailment(
                        &mut self.solver,
                        double_bind,
                        &self.owner_name,
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
                    let tid = self.alloc_var_term(*var);
                    self.set_version(*var, r_i, tid);
                    self.defined[*var] = self.solver.bdd.or(self.defined[*var], r_i);
                    let not_ri = self.solver.bdd.not(r_i);
                    self.moved[*var] = self.solver.bdd.and(self.moved[*var], not_ri);
                    self.captured.entry(*var).or_default().push((
                        r_i,
                        CapturedInfo {
                            operation: operation.clone(),
                            old_args,
                            post_args,
                        },
                    ));
                }
                Command::Decide {
                    site,
                    operation: _,
                    op_kind,
                    arguments,
                    arms,
                } => {
                    let contract = op_kind.contract(self.program);
                    let op_name = op_kind.name(self.program).to_string();
                    // [C2]: Every alternative of the operation's contract is listed.
                    if arms.len() != contract.len() {
                        return Err(Error {
                            control: self.owner_name.clone(),
                            line,
                            clause: "C2",
                            message: format!(
                                "decision on `{op_name}` lists {} of {} alternatives",
                                arms.len(),
                                contract.len()
                            ),
                            counterexample: None,
                        });
                    }

                    let old_arg_terms: Vec<LinearTerm> = arguments
                        .iter()
                        .map(|a| translate_term(self.program, &mut self.solver, a, &env))
                        .collect();
                    let mut post_arg_terms = old_arg_terms.clone();

                    // Check [O0], [F5], [F1]/[F2]/[F6], [F3], [P3] depending on operation kind.
                    match op_kind {
                        Operation::Control(cid) => {
                            let callee = &self.program.controls[*cid];
                            if self.in_contingency
                                && !callee.contingent
                                && self.is_feasible(r_i, &visible_vars, in_loop)
                            {
                                return Err(Error {
                                    control: self.owner_name.clone(),
                                    line,
                                    clause: "F5",
                                    message: format!(
                                        "contingency may not call non-contingent `{}`",
                                        callee.name
                                    ),
                                    counterexample: None,
                                });
                            }
                            let decide_param_tys: Vec<Type> =
                                callee.parameters.iter().map(|p| p.ty.clone()).collect();
                            self.check_operands_o0(
                                arguments,
                                Some(&decide_param_tys),
                                r_i,
                                &env,
                                line,
                                &visible_vars,
                                in_loop,
                            )?;
                            if self.program.controls[*cid].faults {
                                self.check_may_fault(r_i, line, &visible_vars, in_loop)?;
                            }
                            let uses = self.program.controls[*cid].uses.clone();
                            self.check_effects(&uses, &op_name, r_i, line, &visible_vars, in_loop)?;

                            if let Some(pre) = &self.program.controls[*cid].precondition.clone() {
                                let call_env = Env {
                                    locals: HashMap::new(),
                                    parameters: old_arg_terms.clone(),
                                    old_parameters: old_arg_terms.clone(),
                                    fields: Vec::new(),
                                    constants: HashMap::new(),
                                };
                                let pre_formula = translate_formula(
                                    self.program,
                                    &mut self.solver,
                                    pre,
                                    &call_env,
                                );
                                require_formula(
                                    &mut self.solver,
                                    r_i,
                                    pre_formula,
                                    &self.owner_name,
                                    line,
                                    "P3",
                                    format!("precondition of `{op_name}` may not hold"),
                                    &visible_vars,
                                    in_loop,
                                    self.certificates,
                                )?;
                                self.solver.add_fact(r_i, pre_formula);
                            }

                            let param_tys: Vec<Type> = self.program.controls[*cid]
                                .parameters
                                .iter()
                                .map(|p| p.ty.clone())
                                .collect();
                            for (pidx, (arg, pty)) in arguments.iter().zip(&param_tys).enumerate() {
                                if matches!(pty, Type::Ref(BorrowMode::Exclusive, _)) {
                                    let inner_ty = pty.deref_type();
                                    let z_tid = self.alloc_type_term("_mut", inner_ty);
                                    let z_term = LinearTerm::var(z_tid);
                                    post_arg_terms[pidx] = z_term.clone();
                                    match arg {
                                        Expr::Borrow(BorrowMode::Exclusive, place, _) => {
                                            self.apply_place_write(place, r_i, z_term, &env);
                                        }
                                        Expr::Var(VarRef::Local(vid), _) => {
                                            let place = Place {
                                                root: *vid,
                                                root_ty: pty.clone(),
                                                steps: Vec::new(),
                                                ty: inner_ty.clone(),
                                            };
                                            self.apply_place_write(&place, r_i, z_term, &env);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Operation::Builtin { op, .. } => {
                            self.check_operands_o0(
                                arguments,
                                None,
                                r_i,
                                &env,
                                line,
                                &visible_vars,
                                in_loop,
                            )?;
                            if *op == BuiltinOp::Emit {
                                let uses = vec!["console.write".to_string()];
                                self.check_effects(
                                    &uses,
                                    "emit",
                                    r_i,
                                    line,
                                    &visible_vars,
                                    in_loop,
                                )?;
                            }
                        }
                        Operation::Outcome(_) => {
                            self.check_operands_o0(
                                arguments,
                                None,
                                r_i,
                                &env,
                                line,
                                &visible_vars,
                                in_loop,
                            )?;
                        }
                        Operation::Variant { by_value, .. } => {
                            if let Some(arg) = arguments.first() {
                                check_expr_obligations(
                                    self.program,
                                    &mut self.solver,
                                    arg,
                                    r_i,
                                    &env,
                                    &self.owner_name,
                                    line,
                                    &visible_vars,
                                    in_loop,
                                    self.certificates,
                                )?;
                                if *by_value {
                                    self.check_consume_expr(
                                        arg,
                                        r_i,
                                        line,
                                        &visible_vars,
                                        in_loop,
                                    )?;
                                } else {
                                    self.check_read_expr(arg, r_i, line, &visible_vars, in_loop)?;
                                }
                            }
                        }
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

                    let (is_pure, pure_cid) = match op_kind {
                        Operation::Control(cid) if self.program.controls[*cid].is_pure() => {
                            (true, Some(*cid))
                        }
                        _ => (false, None),
                    };
                    let pure_alt_term = pure_cid.map(|cid| {
                        let id = self
                            .solver
                            .intern_term(AtomTerm::PureAlt(cid, old_arg_terms.clone()));
                        self.solver.instantiate_all();
                        LinearTerm::var(id)
                    });

                    // Snapshot captured entries if deciding an outcome variable.
                    let outcome_captures = match op_kind {
                        Operation::Outcome(_) => match arguments.first() {
                            Some(Expr::Var(VarRef::Local(v), _)) => self.captured.get(v).cloned(),
                            Some(Expr::Move(inner, _)) => match &**inner {
                                Expr::Var(VarRef::Local(v), _) => self.captured.get(v).cloned(),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };

                    for (pos, arm) in arms.iter().enumerate() {
                        let atom = arm_atoms[pos];
                        let contract_alt = &contract[arm.alternative];
                        let mut field_terms = Vec::new();

                        for (field_idx, &var_id) in arm.vars.iter().enumerate() {
                            // [O1]: Θ ⊨ ¬(d.a ∧ D(x))
                            let d_var = self.defined[var_id];
                            let double_bind = self.solver.bdd.and(atom, d_var);
                            require_entailment(
                                &mut self.solver,
                                double_bind,
                                &self.owner_name,
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
                            let not_atom = self.solver.bdd.not(atom);
                            self.moved[var_id] = self.solver.bdd.and(self.moved[var_id], not_atom);

                            if let Some(cid) = pure_cid {
                                let pay_id = self.solver.intern_term(AtomTerm::PurePay(
                                    cid,
                                    arm.alternative,
                                    field_idx,
                                    old_arg_terms.clone(),
                                ));
                                self.solver.instantiate_all();
                                let eq = self.solver.eq(&v_term, &LinearTerm::var(pay_id));
                                self.solver.add_fact(atom, eq);
                            }
                            field_terms.push(v_term);
                        }

                        if let Some(alt_term) = &pure_alt_term {
                            let eq = self
                                .solver
                                .eq(alt_term, &LinearTerm::from_i128(arm.alternative as i128));
                            self.solver.add_fact(atom, eq);
                        }

                        match op_kind {
                            Operation::Control(_) => {
                                if let Some(cond) = &contract_alt.condition {
                                    let cond_env = Env {
                                        locals: HashMap::new(),
                                        parameters: post_arg_terms.clone(),
                                        old_parameters: old_arg_terms.clone(),
                                        fields: field_terms,
                                        constants: HashMap::new(),
                                    };
                                    let cond_formula = translate_formula(
                                        self.program,
                                        &mut self.solver,
                                        cond,
                                        &cond_env,
                                    );
                                    self.solver.add_fact(atom, cond_formula);
                                }
                            }
                            Operation::Builtin { op, target, .. } => {
                                self.apply_builtin_facts(
                                    *op,
                                    target.as_ref(),
                                    &old_arg_terms,
                                    arm.alternative,
                                    atom,
                                    &field_terms,
                                    &env,
                                );
                            }
                            Operation::Variant { id: vid, .. } => {
                                // [I11]: `d.c ⇒ x = V.c {ȳ}`
                                if let Some(scrutinee) = old_arg_terms.first() {
                                    let num_ctors = self.program.variants[*vid].constructors.len();
                                    let ctor_id = self.solver.intern_term(AtomTerm::VariantCtor {
                                        variant_id: *vid,
                                        ctor_idx: arm.alternative,
                                        fields: field_terms,
                                        num_ctors,
                                    });
                                    self.solver.instantiate_all();
                                    let eq = self.solver.eq(scrutinee, &LinearTerm::var(ctor_id));
                                    self.solver.add_fact(atom, eq);
                                }
                            }
                            Operation::Outcome(_) => {
                                // [I12]: Conditions of the captured operation over capture arguments.
                                if let Some(entries) = &outcome_captures {
                                    for (cap_guard, info) in entries {
                                        let g = self.solver.bdd.and(*cap_guard, atom);
                                        if g == bdd::FALSE {
                                            continue;
                                        }
                                        match &info.operation {
                                            Operation::Control(cid) => {
                                                if let Some(cond) = &self.program.controls[*cid]
                                                    .contract[arm.alternative]
                                                    .condition
                                                {
                                                    let cond_env = Env {
                                                        locals: HashMap::new(),
                                                        parameters: info.post_args.clone(),
                                                        old_parameters: info.old_args.clone(),
                                                        fields: field_terms.clone(),
                                                        constants: HashMap::new(),
                                                    };
                                                    let cf = translate_formula(
                                                        self.program,
                                                        &mut self.solver,
                                                        cond,
                                                        &cond_env,
                                                    );
                                                    self.solver.add_fact(g, cf);
                                                }
                                            }
                                            Operation::Builtin { op, .. } => {
                                                self.apply_builtin_facts(
                                                    *op,
                                                    None,
                                                    &info.old_args,
                                                    arm.alternative,
                                                    g,
                                                    &field_terms,
                                                    &env,
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                } else if let Some(cond) = &contract_alt.condition {
                                    let max_p = max_param_index(cond);
                                    let mut fresh_params = Vec::with_capacity(max_p);
                                    for p in 0..max_p {
                                        let tid = self.solver.fresh_var(
                                            format!("_out_arg{p}"),
                                            None,
                                            false,
                                        );
                                        fresh_params.push(LinearTerm::var(tid));
                                    }
                                    let cond_env = Env {
                                        locals: HashMap::new(),
                                        parameters: fresh_params.clone(),
                                        old_parameters: fresh_params,
                                        fields: field_terms,
                                        constants: HashMap::new(),
                                    };
                                    let cf = translate_formula(
                                        self.program,
                                        &mut self.solver,
                                        cond,
                                        &cond_env,
                                    );
                                    self.solver.add_fact(atom, cf);
                                }
                            }
                        }
                    }

                    let always_check_u16 = matches!(op_kind, Operation::Outcome(_))
                        || matches!(
                            op_kind,
                            Operation::Builtin {
                                op: BuiltinOp::Pop
                                    | BuiltinOp::Take
                                    | BuiltinOp::Insert
                                    | BuiltinOp::Remove
                                    | BuiltinOp::Get
                                    | BuiltinOp::EntryAt,
                                ..
                            }
                        );
                    self.warn_unreachable_arms(
                        &op_name,
                        line,
                        &statements,
                        &contract,
                        arms,
                        &arm_atoms,
                        is_pure,
                        always_check_u16,
                        r_i,
                        &visible_vars,
                        in_loop,
                    );

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
                    self.check_operands_o0(initial, None, r_i, &env, line, &visible_vars, in_loop)?;

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
                            &self.owner_name,
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
                    let saved_moved = self.moved.clone();
                    let saved_versions = self.versions.clone();
                    let mut entry_avail = Vec::with_capacity(self.body.vars.len());
                    for v in 0..self.body.vars.len() {
                        entry_avail.push(self.avail(v));
                    }
                    self.loop_entry_avail.insert(*loop_body, entry_avail);

                    // [N5], [I9]: Give fresh logical names to loop parameters
                    // and to outer names that the loop body can change.
                    let mut modified_outer = HashSet::new();
                    collect_modified_vars(self.body, *loop_body, &mut modified_outer);
                    let extra_roots: Vec<VarId> = modified_outer
                        .iter()
                        .filter_map(|v| self.local_borrows.get(v).map(|fp| fp.root))
                        .collect();
                    modified_outer.extend(extra_roots);
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
                        let not_ri = self.solver.bdd.not(r_i);
                        self.moved[param_var] = self.solver.bdd.and(self.moved[param_var], not_ri);
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
                    self.active_scopes.push(*loop_body);
                    // [K6]: Check the loop body for one generic iteration with L₁ = R_d.
                    self.check_body_inner(*loop_body, r_i, true, loop_leaves, false)?;
                    self.active_scopes.pop();
                    self.loop_entry_avail.remove(loop_body);
                    let leaves = loop_leaves.remove(loop_body).unwrap_or_default();

                    // Restore outer definedness and link post-loop facts ([K8], [O6]).
                    self.defined = saved_defined;
                    self.moved = saved_moved;
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
                        for (leave_alt, r_j, _, _, _, _) in &leaves {
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
                    for (_, r_j, _, leave_versions, _, _) in &leaves {
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

                    // [O6](b): `□ a` leaves must agree on every outer variable's availability.
                    for (alt_idx, &atom) in arm_atoms.iter().enumerate() {
                        for var in 0..self.body.vars.len() {
                            if !self.active_scopes.contains(&self.body.vars[var].body) {
                                continue;
                            }
                            let mut same_as_entry_bad = bdd::FALSE;
                            let mut moved_bad = bdd::FALSE;
                            let mut avail_bad = bdd::FALSE;
                            let entry_a = self.avail(var);
                            let mut has_leaf = false;
                            for (leave_alt, r_j, _, _, l_def, l_mov) in &leaves {
                                if *leave_alt != alt_idx {
                                    continue;
                                }
                                has_leaf = true;
                                let not_lmov = self.solver.bdd.not(l_mov[var]);
                                let l_avail = self.solver.bdd.and(l_def[var], not_lmov);
                                let eq_entry = self.solver.bdd.iff(l_avail, entry_a);
                                let not_eq_entry = self.solver.bdd.not(eq_entry);
                                let b_entry = self.solver.bdd.and(*r_j, not_eq_entry);
                                same_as_entry_bad = self.solver.bdd.or(same_as_entry_bad, b_entry);

                                let b_mov = self.solver.bdd.and(*r_j, l_avail);
                                moved_bad = self.solver.bdd.or(moved_bad, b_mov);

                                let not_lav = self.solver.bdd.not(l_avail);
                                let b_av = self.solver.bdd.and(*r_j, not_lav);
                                avail_bad = self.solver.bdd.or(avail_bad, b_av);
                            }
                            if !has_leaf || same_as_entry_bad == bdd::FALSE {
                                continue;
                            }
                            if let Decision::Proved(cert) =
                                self.solver
                                    .decide(same_as_entry_bad, &visible_vars, in_loop)
                            {
                                kernel::validate(&cert).expect("valid O6 certificate");
                                self.certificates
                                    .push((self.owner_name.clone(), line, "O6", cert));
                                continue;
                            }
                            if let Decision::Proved(cert) =
                                self.solver.decide(moved_bad, &visible_vars, in_loop)
                            {
                                kernel::validate(&cert).expect("valid O6 certificate");
                                self.certificates
                                    .push((self.owner_name.clone(), line, "O6", cert));
                                self.moved[var] = self.solver.bdd.or(self.moved[var], atom);
                                continue;
                            }
                            if let Decision::Proved(cert) =
                                self.solver.decide(avail_bad, &visible_vars, in_loop)
                            {
                                kernel::validate(&cert).expect("valid O6 certificate");
                                self.certificates
                                    .push((self.owner_name.clone(), line, "O6", cert));
                                self.defined[var] = self.solver.bdd.or(self.defined[var], atom);
                                let not_atom = self.solver.bdd.not(atom);
                                self.moved[var] = self.solver.bdd.and(self.moved[var], not_atom);
                                continue;
                            }
                            require_entailment(
                                &mut self.solver,
                                same_as_entry_bad,
                                &self.owner_name,
                                line,
                                "O6",
                                format!(
                                    "loop leaves disagree on ownership of outer `{}`",
                                    self.body.vars[var].name
                                ),
                                &visible_vars,
                                in_loop,
                                self.certificates,
                            )?;
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
                                &self.owner_name,
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
                            for (leave_alt, r_j, payload_terms, _, _, _) in &leaves {
                                if *leave_alt == alt_idx
                                    && let Some(val_term) = payload_terms.get(field_pos)
                                {
                                    if *self.body.vars[var_id].ty.deref_type() == Type::Bool {
                                        let b = self.solver.le(&LinearTerm::from_i128(1), val_term);
                                        self.solver.add_def_bool(*r_j, out_tid, b);
                                    } else {
                                        self.solver.add_def_linear(*r_j, out_tid, val_term.clone());
                                    }
                                }
                            }
                            self.set_version(var_id, atom, out_tid);
                            self.defined[var_id] = self.solver.bdd.or(self.defined[var_id], atom);
                            let not_atom = self.solver.bdd.not(atom);
                            self.moved[var_id] = self.solver.bdd.and(self.moved[var_id], not_atom);
                        }
                    }

                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ (gᵢ → d.a₁ ∨ … ∨ d.aₙ)
                    let g_imp_any = self.solver.bdd.implies(g_i, any_arm);
                    live = self.solver.bdd.and(live, g_imp_any);
                }
                Command::Boundary {
                    site,
                    body: b1_body,
                    captures,
                    contingency_vars,
                    message_var,
                    contingency: b2_body,
                } => {
                    // [F5]: Captured names must own their values, be available, and move.
                    let mut cap_terms = Vec::with_capacity(captures.len());
                    for &cap_var in captures {
                        self.check_available(cap_var, r_i, line, &visible_vars, in_loop)?;
                        let cap_term = env
                            .locals
                            .get(&cap_var)
                            .cloned()
                            .unwrap_or_else(|| LinearTerm::from_i128(0));
                        cap_terms.push(cap_term);
                        if self.program.kind(&self.body.vars[cap_var].ty) != Kind::Copy {
                            self.moved[cap_var] = self.solver.bdd.or(self.moved[cap_var], r_i);
                        }
                    }

                    let outer_defined = self.defined.clone();
                    let outer_moved = self.moved.clone();
                    let outer_versions = self.versions.clone();
                    let outer_permits_faults = self.permits_faults;
                    let outer_in_contingency = self.in_contingency;

                    // Check `B₁` with fault permission enabled ([F5]).
                    self.active_scopes.push(*b1_body);
                    self.permits_faults = true;
                    self.in_contingency = false;
                    let b1_exit_live =
                        self.check_body_inner(*b1_body, r_i, in_loop, loop_leaves, true)?;
                    let (_, b1_visible) = self.current_env();
                    self.check_scope_obligations(
                        &[*b1_body],
                        b1_exit_live,
                        line,
                        &b1_visible,
                        in_loop,
                    )?;
                    self.active_scopes.pop();
                    let b1_end_defined = self.defined.clone();
                    let b1_end_moved = self.moved.clone();
                    let b1_end_versions = self.versions.clone();

                    // Check `B₂` (`contingency`) with outer state reset and captures + message bound.
                    self.defined = outer_defined.clone();
                    self.moved = outer_moved.clone();
                    self.versions = outer_versions.clone();
                    let mut b1_modified = HashSet::new();
                    collect_modified_vars(self.body, *b1_body, &mut b1_modified);
                    let b1_extra_roots: Vec<VarId> = b1_modified
                        .iter()
                        .filter_map(|v| self.local_borrows.get(v).map(|fp| fp.root))
                        .collect();
                    b1_modified.extend(b1_extra_roots);
                    for &var in &b1_modified {
                        if self.active_scopes.contains(&self.body.vars[var].body)
                            && self.defined[var] != bdd::FALSE
                        {
                            let fresh_tid = self.alloc_var_term(var);
                            let on_b2 = self.solver.bdd.and(self.defined[var], r_i);
                            self.set_version(var, on_b2, fresh_tid);
                        }
                    }
                    for (cvar, cap_term) in contingency_vars.iter().zip(&cap_terms) {
                        let _ = self.bind_value_term(*cvar, r_i, cap_term.clone());
                        self.defined[*cvar] = self.solver.bdd.or(self.defined[*cvar], r_i);
                        let not_ri = self.solver.bdd.not(r_i);
                        self.moved[*cvar] = self.solver.bdd.and(self.moved[*cvar], not_ri);
                    }
                    if let Some(mvar) = message_var {
                        let mtid = self.alloc_var_term(*mvar);
                        self.set_version(*mvar, r_i, mtid);
                        self.defined[*mvar] = self.solver.bdd.or(self.defined[*mvar], r_i);
                        let not_ri = self.solver.bdd.not(r_i);
                        self.moved[*mvar] = self.solver.bdd.and(self.moved[*mvar], not_ri);
                    }
                    self.active_scopes.push(*b2_body);
                    self.permits_faults = outer_permits_faults;
                    self.in_contingency = true;
                    self.check_body_inner(*b2_body, r_i, in_loop, loop_leaves, false)?;
                    self.active_scopes.pop();

                    // Restore flags and merge `B₁` fallthrough state with bypassed `!g_i` histories.
                    self.permits_faults = outer_permits_faults;
                    self.in_contingency = outer_in_contingency;
                    let bypassed = self.solver.bdd.minus(live, g_i);
                    for v in 0..self.body.vars.len() {
                        let left_d = self.solver.bdd.and(outer_defined[v], bypassed);
                        let right_d = self.solver.bdd.and(b1_end_defined[v], b1_exit_live);
                        self.defined[v] = self.solver.bdd.or(left_d, right_d);

                        let left_m = self.solver.bdd.and(outer_moved[v], bypassed);
                        let right_m = self.solver.bdd.and(b1_end_moved[v], b1_exit_live);
                        self.moved[v] = self.solver.bdd.or(left_m, right_m);

                        let mut merged = Vec::new();
                        for (g, tid) in &outer_versions[v] {
                            let mg = self.solver.bdd.and(*g, bypassed);
                            if mg != bdd::FALSE {
                                merged.push((mg, *tid));
                            }
                        }
                        for (g, tid) in &b1_end_versions[v] {
                            let mg = self.solver.bdd.and(*g, b1_exit_live);
                            if mg != bdd::FALSE {
                                merged.push((mg, *tid));
                            }
                        }
                        self.versions[v] = merged;
                    }
                    let d_done = self.atom(*site, 0);
                    let iff = self.solver.bdd.iff(d_done, b1_exit_live);
                    self.solver.add_structure(iff);
                    let g_imp_done = self.solver.bdd.implies(g_i, d_done);
                    live = self.solver.bdd.and(live, g_imp_done);
                }
                Command::Supervise {
                    boundary: bid,
                    arguments,
                } => {
                    let bdef = &self.program.boundaries[*bid];
                    let bname = bdef.name.clone();
                    if self.in_contingency && self.is_feasible(r_i, &visible_vars, in_loop) {
                        return Err(Error {
                            control: self.owner_name.clone(),
                            line,
                            clause: "F5",
                            message: format!("contingency may not supervise boundary `{bname}`"),
                            counterexample: None,
                        });
                    }
                    let b_param_tys: Vec<Type> =
                        bdef.parameters.iter().map(|p| p.ty.clone()).collect();
                    self.check_operands_o0(
                        arguments,
                        Some(&b_param_tys),
                        r_i,
                        &env,
                        line,
                        &visible_vars,
                        in_loop,
                    )?;
                    let uses = bdef.uses.clone();
                    self.check_effects(&uses, &bname, r_i, line, &visible_vars, in_loop)?;
                    if self.boundary_may_fault[*bid] {
                        self.check_may_fault(r_i, line, &visible_vars, in_loop)?;
                    }
                }
                Command::Again {
                    body: target_body,
                    arguments,
                } => {
                    self.check_operands_o0(
                        arguments,
                        None,
                        r_i,
                        &env,
                        line,
                        &visible_vars,
                        in_loop,
                    )?;
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
                            &self.owner_name,
                            line,
                            "P2",
                            format!("invariant of loop `{}` may not hold at `↑`", head.name),
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    self.check_loop_repeat_scope(
                        *target_body,
                        r_i,
                        line,
                        "↑",
                        &visible_vars,
                        in_loop,
                    )?;
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Leave {
                    alternative,
                    payload,
                } => {
                    self.check_operands_o0(payload, None, r_i, &env, line, &visible_vars, in_loop)?;
                    let payload_terms: Vec<LinearTerm> = payload
                        .iter()
                        .map(|e| translate_term(self.program, &mut self.solver, e, &env))
                        .collect();
                    let target_loop_pos = self
                        .active_scopes
                        .iter()
                        .rposition(|&b| self.body.bodies[b].head.is_some());
                    if let Some(pos) = target_loop_pos {
                        let target_loop = self.active_scopes[pos];
                        let ended: Vec<BodyId> = self.active_scopes[pos..].to_vec();
                        self.check_scope_obligations(&ended, r_i, line, &visible_vars, in_loop)?;
                        if let Some(list) = loop_leaves.get_mut(&target_loop) {
                            list.push((
                                *alternative,
                                r_i,
                                payload_terms,
                                self.versions.clone(),
                                self.defined.clone(),
                                self.moved.clone(),
                            ));
                        }
                    }
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Exit {
                    alternative,
                    payload,
                } => {
                    self.check_operands_o0(payload, None, r_i, &env, line, &visible_vars, in_loop)?;
                    let entry = &self.contract[*alternative];
                    // [P4]: Contract condition `⇐ φₐ` at `■`.
                    if let Some(cond) = &entry.condition {
                        let payload_terms: Vec<LinearTerm> = payload
                            .iter()
                            .map(|e| translate_term(self.program, &mut self.solver, e, &env))
                            .collect();
                        let exit_parameters: Vec<LinearTerm> = self
                            .param_types
                            .iter()
                            .enumerate()
                            .map(|(pidx, pty)| {
                                if matches!(pty, Type::Ref(BorrowMode::Exclusive, _)) {
                                    env.locals
                                        .get(&pidx)
                                        .cloned()
                                        .unwrap_or_else(|| self.entry_parameters[pidx].clone())
                                } else {
                                    self.entry_parameters[pidx].clone()
                                }
                            })
                            .collect();
                        let exit_env = Env {
                            locals: HashMap::new(),
                            parameters: exit_parameters,
                            old_parameters: self.entry_parameters.clone(),
                            fields: payload_terms,
                            constants: HashMap::new(),
                        };
                        let cond_formula =
                            translate_formula(self.program, &mut self.solver, cond, &exit_env);
                        require_formula(
                            &mut self.solver,
                            r_i,
                            cond_formula,
                            &self.owner_name,
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
                    let all_scopes = self.active_scopes.clone();
                    self.check_scope_obligations(&all_scopes, r_i, line, &visible_vars, in_loop)?;
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Fault {
                    message,
                    assumed: _,
                } => {
                    check_expr_obligations(
                        self.program,
                        &mut self.solver,
                        message,
                        r_i,
                        &env,
                        &self.owner_name,
                        line,
                        &visible_vars,
                        in_loop,
                        self.certificates,
                    )?;
                    self.check_consume_expr(message, r_i, line, &visible_vars, in_loop)?;
                    self.check_may_fault(r_i, line, &visible_vars, in_loop)?;
                    if self.in_contingency && self.is_feasible(r_i, &visible_vars, in_loop) {
                        self.boundary_can_fault = true;
                    }
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Escalate => {
                    self.check_may_fault(r_i, line, &visible_vars, in_loop)?;
                    if self.in_contingency && self.is_feasible(r_i, &visible_vars, in_loop) {
                        self.boundary_can_fault = true;
                    }
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Terminate(opt_expr) => {
                    if let Some(expr) = opt_expr {
                        check_expr_obligations(
                            self.program,
                            &mut self.solver,
                            expr,
                            r_i,
                            &env,
                            &self.owner_name,
                            line,
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                        self.check_consume_expr(expr, r_i, line, &visible_vars, in_loop)?;
                    }
                    let all_scopes = self.active_scopes.clone();
                    self.check_scope_obligations(&all_scopes, r_i, line, &visible_vars, in_loop)?;
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Recover { body: target_body } => {
                    if let Some(inv) = self.body.bodies[*target_body]
                        .head
                        .as_ref()
                        .expect("a loop head")
                        .invariant
                        .clone()
                    {
                        let inv_rec = translate_formula(self.program, &mut self.solver, &inv, &env);
                        require_formula(
                            &mut self.solver,
                            r_i,
                            inv_rec,
                            &self.owner_name,
                            line,
                            "P2",
                            "loop invariant may not hold at `recover ↑`",
                            &visible_vars,
                            in_loop,
                            self.certificates,
                        )?;
                    }
                    self.check_loop_repeat_scope(
                        *target_body,
                        r_i,
                        line,
                        "recover ↑",
                        &visible_vars,
                        in_loop,
                    )?;
                    // [K3]: Lᵢ₊₁ = Lᵢ ∧ ¬gᵢ
                    live = self.solver.bdd.minus(live, g_i);
                }
                Command::Impossible => {
                    // [P6]: Θ ⊨ ¬Rᵢ
                    require_entailment(
                        &mut self.solver,
                        r_i,
                        &self.owner_name,
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

        if !allow_fallthrough {
            // [K7] Coverage: Θ ⊨ ¬Lₙ₊₁
            let (_, visible_vars) = self.current_env();
            require_entailment(
                &mut self.solver,
                live,
                &self.owner_name,
                last_line,
                "K7",
                "a feasible history reaches the end of the body without a disposition",
                &visible_vars,
                in_loop,
                self.certificates,
            )?;
        }
        Ok(live)
    }
}

fn freeze_place(program: &Program, solver: &mut Solver, place: &Place, env: &Env) -> FrozenPlace {
    let steps = place
        .steps
        .iter()
        .map(|s| match s {
            PlaceStep::Field(sid, fidx, ty) => FrozenStep::Field(*sid, *fidx, ty.clone()),
            PlaceStep::Index(idx_expr, ty) => {
                let it = translate_term(program, solver, idx_expr, env);
                FrozenStep::Index(it, ty.clone())
            }
        })
        .collect();
    FrozenPlace {
        root: place.root,
        root_ty: place.root_ty.clone(),
        steps,
    }
}

fn functional_update_frozen(
    program: &Program,
    solver: &mut Solver,
    cur_term: LinearTerm,
    cur_ty: &Type,
    steps: &[FrozenStep],
    new_val: LinearTerm,
) -> LinearTerm {
    let Some((first, rest)) = steps.split_first() else {
        return new_val;
    };
    match first {
        FrozenStep::Field(sid, fidx, fty) => {
            let sdef = &program.structs[*sid];
            let sub_old = translate_field_term(solver, cur_term.clone(), *sid, *fidx, fty);
            let updated_field =
                functional_update_frozen(program, solver, sub_old, fty, rest, new_val);
            let mut fields = Vec::with_capacity(sdef.fields.len());
            for (idx, field_param) in sdef.fields.iter().enumerate() {
                if idx == *fidx {
                    fields.push(updated_field.clone());
                } else {
                    fields.push(translate_field_term(
                        solver,
                        cur_term.clone(),
                        *sid,
                        idx,
                        &field_param.ty,
                    ));
                }
            }
            let cid = solver.intern_term(AtomTerm::StructCtor(*sid, fields));
            solver.register_sort(cid, cur_ty.deref_type());
            solver.instantiate_all();
            LinearTerm::var(cid)
        }
        FrozenStep::Index(idx_term, ety) => {
            let sub_old = translate_index_term(solver, cur_term.clone(), idx_term.clone(), ety);
            let updated_elem =
                functional_update_frozen(program, solver, sub_old, ety, rest, new_val);
            let uid = solver.intern_term(AtomTerm::Update {
                base: cur_term,
                index: idx_term.clone(),
                value: updated_elem,
            });
            solver.register_sort(uid, cur_ty.deref_type());
            solver.instantiate_all();
            LinearTerm::var(uid)
        }
    }
}

fn max_param_index(expr: &Expr) -> usize {
    match expr {
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::EmptyMap(_)
        | Expr::Var(VarRef::Local(_), _)
        | Expr::Var(VarRef::Field(_), _) => 0,
        Expr::Var(VarRef::Parameter(idx), _) | Expr::Old(idx, _) => *idx + 1,
        Expr::Borrow(_, place, _) => place
            .steps
            .iter()
            .filter_map(|s| match s {
                PlaceStep::Index(e, _) => Some(max_param_index(e)),
                PlaceStep::Field(_, _, _) => None,
            })
            .max()
            .unwrap_or(0),
        Expr::Move(inner, _)
        | Expr::Field(inner, _, _, _)
        | Expr::Len(inner)
        | Expr::Clone(inner, _)
        | Expr::Trim(inner)
        | Expr::Negate(inner, _)
        | Expr::Not(inner) => max_param_index(inner),
        Expr::Index(l, r, _)
        | Expr::Arithmetic(_, l, r, _)
        | Expr::Compare(_, l, r)
        | Expr::And(l, r)
        | Expr::Or(l, r) => max_param_index(l).max(max_param_index(r)),
        Expr::Struct(_, items, _)
        | Expr::Variant(_, _, items, _)
        | Expr::Vector(items, _)
        | Expr::Call(_, items, _) => items.iter().map(max_param_index).max().unwrap_or(0),
    }
}

fn solver_for_control_body(program: &Program, body: &ControlBody) -> (Solver, Vec<Vec<usize>>) {
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
    let mut solver = Solver::new(history_names);
    register_program_structs(program, &mut solver);
    (solver, site_atoms)
}

fn check_boundary(
    program: &Program,
    boundary: &BoundaryDef,
    boundary_may_fault: &[bool],
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
    warnings: &mut Vec<crate::surface::Warning>,
) -> Result<bool, Error> {
    // 1. Check `B₁` (`boundary.body`) with `permits_faults = true` and `retains` checked ([F6]).
    let (solver1, site_atoms1) = solver_for_control_body(program, &boundary.body);
    let var_count1 = boundary.body.vars.len();
    let constants1 = find_constant_locals(program, &boundary.body, boundary.parameters.len());
    let mut checker1 = BodyChecker {
        program,
        owner_name: boundary.name.clone(),
        allowed_uses: boundary.uses.clone(),
        contract: Vec::new(),
        param_types: boundary.parameters.iter().map(|p| p.ty.clone()).collect(),
        body: &boundary.body,
        boundary_may_fault,
        solver: solver1,
        site_atoms: site_atoms1,
        defined: vec![bdd::FALSE; var_count1],
        moved: vec![bdd::FALSE; var_count1],
        versions: vec![Vec::new(); var_count1],
        entry_parameters: Vec::new(),
        constants: constants1,
        captured: HashMap::new(),
        local_borrows: HashMap::new(),
        loop_entry_avail: HashMap::new(),
        active_scopes: vec![0],
        permits_faults: true,
        in_contingency: false,
        retained_vars: boundary.retains.clone(),
        boundary_can_fault: false,
        certificates,
        warnings,
    };
    for param_idx in 0..boundary.parameters.len() {
        let tid = checker1.alloc_var_term(param_idx);
        checker1.defined[param_idx] = bdd::TRUE;
        checker1.versions[param_idx] = vec![(bdd::TRUE, tid)];
        checker1.entry_parameters.push(LinearTerm::var(tid));
    }
    let mut loop_leaves1 = HashMap::new();
    checker1.check_body(0, bdd::TRUE, false, &mut loop_leaves1)?;

    // 2. Check `B₂` (`boundary.contingency`) with `retains` + `message` bound ([F6]).
    let cont_param_count = boundary.retains.len() + 1;
    let (solver2, site_atoms2) = solver_for_control_body(program, &boundary.contingency);
    let var_count2 = boundary.contingency.vars.len();
    let constants2 = find_constant_locals(program, &boundary.contingency, cont_param_count);
    let mut checker2 = BodyChecker {
        program,
        owner_name: format!("{} contingency", boundary.name),
        allowed_uses: boundary.uses.clone(),
        contract: Vec::new(),
        param_types: boundary
            .retains
            .iter()
            .map(|&r| boundary.parameters[r].ty.clone())
            .chain(std::iter::once(Type::String))
            .collect(),
        body: &boundary.contingency,
        boundary_may_fault,
        solver: solver2,
        site_atoms: site_atoms2,
        defined: vec![bdd::FALSE; var_count2],
        moved: vec![bdd::FALSE; var_count2],
        versions: vec![Vec::new(); var_count2],
        entry_parameters: Vec::new(),
        constants: constants2,
        captured: HashMap::new(),
        local_borrows: HashMap::new(),
        loop_entry_avail: HashMap::new(),
        active_scopes: vec![0],
        permits_faults: true,
        in_contingency: true,
        retained_vars: Vec::new(),
        boundary_can_fault: false,
        certificates,
        warnings,
    };
    for param_idx in 0..cont_param_count.min(var_count2) {
        let tid = checker2.alloc_var_term(param_idx);
        checker2.defined[param_idx] = bdd::TRUE;
        checker2.versions[param_idx] = vec![(bdd::TRUE, tid)];
        checker2.entry_parameters.push(LinearTerm::var(tid));
    }
    let mut loop_leaves2 = HashMap::new();
    checker2.check_body(0, bdd::TRUE, false, &mut loop_leaves2)?;
    Ok(checker2.boundary_can_fault)
}

fn check_control(
    program: &Program,
    control: &Control,
    boundary_may_fault: &[bool],
    certificates: &mut Vec<(String, u32, &'static str, Certificate)>,
    warnings: &mut Vec<crate::surface::Warning>,
) -> Result<(), Error> {
    let body = control.body.as_ref().expect("a defined control");
    let (solver, site_atoms) = solver_for_control_body(program, body);
    let var_count = body.vars.len();
    let constants = find_constant_locals(program, body, control.parameters.len());

    let mut checker = BodyChecker {
        program,
        owner_name: control.name.clone(),
        allowed_uses: control.uses.clone(),
        contract: control.contract.clone(),
        param_types: control.parameters.iter().map(|p| p.ty.clone()).collect(),
        body,
        boundary_may_fault,
        solver,
        site_atoms,
        defined: vec![bdd::FALSE; var_count],
        moved: vec![bdd::FALSE; var_count],
        versions: vec![Vec::new(); var_count],
        entry_parameters: Vec::new(),
        constants,
        captured: HashMap::new(),
        local_borrows: HashMap::new(),
        loop_entry_avail: HashMap::new(),
        active_scopes: vec![0],
        permits_faults: control.faults,
        in_contingency: false,
        retained_vars: Vec::new(),
        boundary_can_fault: false,
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
