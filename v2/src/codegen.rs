//! Phase 4: The typed LLVM IR and C ABI backend for slice 1 (`docs/redesign-plan.md`
//! Phase 4; `docs/v2/core.md` section 9, `[R1]` to `[R3]`).
//!
//! Compiles a checked [`typed::Program`] directly to textual LLVM IR (and C ABI
//! declarations for `extern` host adapters) under the erased semantics:
//!
//! - **[R1]** Every variable slot is a native scalar stack slot (`alloca i1`,
//!   `i8`, `i16`, `i32`, `i64`), promoted to SSA registers by LLVM's `mem2reg`.
//!   There are no dynamic value wrappers, no moved marks and no borrow tags.
//! - **[R2]** `⊢` (`Oblige`), loop invariants, function preconditions, control
//!   preconditions, outcome conditions `⇐` and effect allowances emit zero
//!   code. Arithmetic is direct machine wrapping arithmetic (`add`, `sub`,
//!   `mul`, `udiv`/`sdiv`, `urem`/`srem`). `⊥` (`Impossible`) and decision arms
//!   guarded by `⊥` emit `unreachable`.
//! - **[R3]** Only `(site, alternative)` atoms that some selector in the
//!   control mentions get an `i1` history slot. Atoms never mentioned in any
//!   selector allocate no bit and emit no store.

use crate::erased;
use crate::host::Answer;
use crate::int::Int;
use crate::oracle::{Event, Value};
use crate::typed::{
    Arithmetic, BodyId, Command, Comparison, Control, ControlBody, ControlId, Expr, Function,
    FunctionId, IntType, Program, Selector, SiteId, Type, VarId, VarRef,
};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicU64, Ordering};

/// A single native test run specification for [`run_native_batch`].
#[derive(Clone, Debug)]
pub struct NativeRunSpec {
    pub entry: String,
    pub arguments: Vec<Value>,
    pub arithmetic: bool,
    pub answers: Vec<(String, Answer)>,
}

/// Which atoms of a [`ControlBody`] are mentioned by at least one selector
/// (`[R3]`).
struct HistoryPlan {
    bit: Vec<Vec<Option<usize>>>,
    bits: usize,
    own: Vec<Vec<usize>>,
}

fn mention(selector: &Selector, plan: &mut HistoryPlan) {
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

fn is_direct_impossible_arm(stmt: &crate::typed::Statement) -> bool {
    matches!(stmt.command, Command::Impossible) && matches!(stmt.selector, Selector::Atom(_, _))
}

fn build_plan(body: &ControlBody) -> HistoryPlan {
    let mut plan = HistoryPlan {
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
            // [R2], [R3]: `⊢` obligations are not evaluated, and `site.alt ▷ ⊥`
            // branches directly to `unreachable` in the decision's switch, so
            // neither needs a runtime history bit.
            if matches!(statement.command, Command::Oblige(_))
                || is_direct_impossible_arm(statement)
            {
                continue;
            }
            mention(&statement.selector, &mut plan);
        }
    }
    for (site, bits) in body.sites.iter().zip(&plan.bit) {
        plan.own[site.body].extend(bits.iter().flatten());
    }
    plan
}

fn llvm_int_ty(ty: IntType, usize_bits: u32) -> &'static str {
    match ty {
        IntType::U8 | IntType::I8 => "i8",
        IntType::U16 | IntType::I16 => "i16",
        IntType::U32 | IntType::I32 => "i32",
        IntType::U64 | IntType::I64 => "i64",
        IntType::Usize => {
            if usize_bits <= 32 {
                "i32"
            } else {
                "i64"
            }
        }
    }
}

fn llvm_ty(ty: &Type, usize_bits: u32) -> &'static str {
    match ty {
        Type::Bool => "i1",
        Type::Int(int_ty) => llvm_int_ty(*int_ty, usize_bits),
        _ => "ptr",
    }
}

/// C ABI representation type (`bool` is passed as `i8` across C ABI boundaries).
fn c_abi_llvm_ty(ty: &Type, usize_bits: u32) -> &'static str {
    match ty {
        Type::Bool => "i8",
        Type::Int(int_ty) => llvm_int_ty(*int_ty, usize_bits),
        _ => "ptr",
    }
}

fn c_header_ty(ty: &Type, usize_bits: u32) -> &'static str {
    match ty {
        Type::Bool => "uint8_t",
        Type::Int(IntType::U8) => "uint8_t",
        Type::Int(IntType::U16) => "uint16_t",
        Type::Int(IntType::U32) => "uint32_t",
        Type::Int(IntType::U64) => "uint64_t",
        Type::Int(IntType::I8) => "int8_t",
        Type::Int(IntType::I16) => "int16_t",
        Type::Int(IntType::I32) => "int32_t",
        Type::Int(IntType::I64) => "int64_t",
        Type::Int(IntType::Usize) => {
            if usize_bits <= 32 {
                "uint32_t"
            } else {
                "uint64_t"
            }
        }
        _ => "void *",
    }
}

/// Emits the typed C header for `program`, declaring outcome tag constants,
/// `extern` host adapter prototypes and public defined control entry points.
pub fn emit_c_header(program: &Program, usize_bits: u32) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "/* Generated by Villanelle v2 (Phase 4 typed C ABI). */"
    );
    let _ = writeln!(out, "#pragma once");
    let _ = writeln!(out, "#include <stdint.h>");
    let _ = writeln!(out);

    for control in &program.controls {
        let upper_ctrl = control.name.to_ascii_uppercase();
        for (alt_idx, alt) in control.contract.iter().enumerate() {
            let upper_alt = alt.name.to_ascii_uppercase();
            let _ = writeln!(out, "#define VN2_{upper_ctrl}_{upper_alt} {alt_idx}");
        }
        let mut params = Vec::new();
        for param in &control.parameters {
            params.push(format!(
                "{} {}",
                c_header_ty(&param.ty, usize_bits),
                param.name
            ));
        }
        for (alt_idx, alt) in control.contract.iter().enumerate() {
            for (field_idx, field) in alt.fields.iter().enumerate() {
                params.push(format!(
                    "{} *out_{}_{}_{}",
                    c_header_ty(&field.ty, usize_bits),
                    alt_idx,
                    alt.name,
                    field_idx
                ));
            }
        }
        let param_list = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        let prefix = if control.body.is_none() {
            "vn2_ext"
        } else {
            "vn2_call"
        };
        let _ = writeln!(out, "int32_t {prefix}_{}({param_list});", control.name);
        let _ = writeln!(out);
    }
    out
}

/// Emits the textual LLVM IR module for all functions and controls of `program`.
pub fn emit_llvm(program: &Program, usize_bits: u32) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "; Villanelle v2 Phase 4 typed LLVM IR (usize_bits = {usize_bits})"
    );
    let _ = writeln!(out);

    // 1. Emit extern control declarations (`@vn2_ext_<name>`).
    for control in &program.controls {
        if control.body.is_none() {
            let sig = c_abi_signature_types(control, usize_bits);
            let _ = writeln!(
                out,
                "declare i32 @vn2_ext_{}({})",
                control.name,
                sig.join(", ")
            );
        }
    }
    if program.controls.iter().any(|c| c.body.is_none()) {
        let _ = writeln!(out);
    }

    // 2. Emit pure functions (`@vn2_fn_<id>`).
    for (id, function) in program.functions.iter().enumerate() {
        emit_function(&mut out, program, id, function, usize_bits);
        let _ = writeln!(out);
    }

    // 3. Emit defined controls (`@vn2_ctrl_<id>`) and their C-ABI wrappers
    //    (`@vn2_call_<name>`).
    for (id, control) in program.controls.iter().enumerate() {
        if control.body.is_some() {
            emit_control(&mut out, program, id, control, usize_bits);
            let _ = writeln!(out);
            emit_control_c_wrapper(&mut out, id, control, usize_bits);
            let _ = writeln!(out);
        }
    }

    out
}

fn c_abi_signature_types(control: &Control, usize_bits: u32) -> Vec<String> {
    let mut sig = Vec::new();
    for param in &control.parameters {
        sig.push(c_abi_llvm_ty(&param.ty, usize_bits).to_string());
    }
    for alt in &control.contract {
        for _ in &alt.fields {
            sig.push("ptr".to_string());
        }
    }
    sig
}

struct IrEmitter<'a> {
    program: &'a Program,
    usize_bits: u32,
    allocas: Vec<String>,
    code: String,
    next_reg: usize,
    next_label: usize,
}

enum VarAccess {
    /// Function parameters: `%p0, %p1, ...` (already SSA values).
    DirectParams,
    /// Control local slots: `%slot_0, %slot_1, ...` (`alloca` pointers).
    ControlSlots,
}

impl<'a> IrEmitter<'a> {
    fn new(program: &'a Program, usize_bits: u32) -> Self {
        IrEmitter {
            program,
            usize_bits,
            allocas: Vec::new(),
            code: String::new(),
            next_reg: 0,
            next_label: 0,
        }
    }

    fn fresh_reg(&mut self) -> String {
        let r = format!("%r{}", self.next_reg);
        self.next_reg += 1;
        r
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let l = format!("{prefix}_{}", self.next_label);
        self.next_label += 1;
        l
    }

    fn alloc_temp(&mut self, ty: &str) -> String {
        let ptr = format!("%tmp{}", self.next_reg);
        self.next_reg += 1;
        self.allocas.push(format!("  {ptr} = alloca {ty}"));
        ptr
    }

    fn emit_expr(&mut self, expr: &Expr, access: &VarAccess) -> String {
        match expr {
            Expr::Boolean(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Expr::Integer(val, int_ty) => {
                let bits = match int_ty {
                    IntType::U8 | IntType::I8 => 8,
                    IntType::U16 | IntType::I16 => 16,
                    IntType::U32 | IntType::I32 => 32,
                    IntType::U64 | IntType::I64 => 64,
                    IntType::Usize => self.usize_bits,
                };
                let raw = val.to_i128().unwrap_or(0) as u128;
                let masked = if bits >= 64 {
                    raw as u64
                } else {
                    (raw as u64) & ((1u64 << bits) - 1)
                };
                format!("{masked}")
            }
            Expr::Var(var_ref, ty) => {
                let ll_ty = llvm_ty(ty, self.usize_bits);
                match (var_ref, access) {
                    (VarRef::Parameter(idx), VarAccess::DirectParams) => format!("%p{idx}"),
                    (VarRef::Local(id), VarAccess::ControlSlots) => {
                        let reg = self.fresh_reg();
                        let _ = writeln!(self.code, "  {reg} = load {ll_ty}, ptr %slot_{id}");
                        reg
                    }
                    _ => unreachable!("resolved body/function variable access mismatch"),
                }
            }
            Expr::Not(inner) => {
                let v = self.emit_expr(inner, access);
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = xor i1 {v}, true");
                reg
            }
            Expr::Negate(inner, int_ty) => {
                let v = self.emit_expr(inner, access);
                let ll_ty = llvm_int_ty(*int_ty, self.usize_bits);
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = sub {ll_ty} 0, {v}");
                reg
            }
            Expr::Arithmetic(op, left, right, int_ty) => {
                let l = self.emit_expr(left, access);
                let r = self.emit_expr(right, access);
                let ll_ty = llvm_int_ty(*int_ty, self.usize_bits);
                let instr = match op {
                    Arithmetic::Add => "add",
                    Arithmetic::Subtract => "sub",
                    Arithmetic::Multiply => "mul",
                    Arithmetic::Divide => {
                        if int_ty.is_signed() {
                            "sdiv"
                        } else {
                            "udiv"
                        }
                    }
                    Arithmetic::Remainder => {
                        if int_ty.is_signed() {
                            "srem"
                        } else {
                            "urem"
                        }
                    }
                };
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = {instr} {ll_ty} {l}, {r}");
                reg
            }
            Expr::Compare(cmp, left, right) => {
                let l = self.emit_expr(left, access);
                let r = self.emit_expr(right, access);
                let op_ty = left.ty();
                let ll_ty = llvm_ty(&op_ty, self.usize_bits);
                let signed = matches!(op_ty, Type::Int(int_ty) if int_ty.is_signed());
                let pred = match (cmp, signed) {
                    (Comparison::Equal, _) => "eq",
                    (Comparison::NotEqual, _) => "ne",
                    (Comparison::Less, true) => "slt",
                    (Comparison::Less, false) => "ult",
                    (Comparison::LessEqual, true) => "sle",
                    (Comparison::LessEqual, false) => "ule",
                    (Comparison::Greater, true) => "sgt",
                    (Comparison::Greater, false) => "ugt",
                    (Comparison::GreaterEqual, true) => "sge",
                    (Comparison::GreaterEqual, false) => "uge",
                };
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = icmp {pred} {ll_ty} {l}, {r}");
                reg
            }
            // [E-Bool]: short-circuit evaluation.
            Expr::And(left, right) => {
                let tmp = self.alloc_temp("i1");
                let _ = writeln!(self.code, "  store i1 false, ptr {tmp}");
                let l = self.emit_expr(left, access);
                let rhs_lbl = self.fresh_label("and_rhs");
                let end_lbl = self.fresh_label("and_end");
                let _ = writeln!(self.code, "  br i1 {l}, label %{rhs_lbl}, label %{end_lbl}");
                let _ = writeln!(self.code, "{rhs_lbl}:");
                let r = self.emit_expr(right, access);
                let _ = writeln!(self.code, "  store i1 {r}, ptr {tmp}");
                let _ = writeln!(self.code, "  br label %{end_lbl}");
                let _ = writeln!(self.code, "{end_lbl}:");
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = load i1, ptr {tmp}");
                reg
            }
            Expr::Or(left, right) => {
                let tmp = self.alloc_temp("i1");
                let _ = writeln!(self.code, "  store i1 true, ptr {tmp}");
                let l = self.emit_expr(left, access);
                let rhs_lbl = self.fresh_label("or_rhs");
                let end_lbl = self.fresh_label("or_end");
                let _ = writeln!(self.code, "  br i1 {l}, label %{end_lbl}, label %{rhs_lbl}");
                let _ = writeln!(self.code, "{rhs_lbl}:");
                let r = self.emit_expr(right, access);
                let _ = writeln!(self.code, "  store i1 {r}, ptr {tmp}");
                let _ = writeln!(self.code, "  br label %{end_lbl}");
                let _ = writeln!(self.code, "{end_lbl}:");
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = load i1, ptr {tmp}");
                reg
            }
            Expr::Call(fn_id, args, ret_ty) => {
                let callee = &self.program.functions[*fn_id];
                let mut arg_pieces = Vec::new();
                for (arg, param) in args.iter().zip(&callee.parameters) {
                    let v = self.emit_expr(arg, access);
                    arg_pieces.push(format!("{} {v}", llvm_ty(&param.ty, self.usize_bits)));
                }
                let ret_ll = llvm_ty(ret_ty, self.usize_bits);
                let reg = self.fresh_reg();
                let _ = writeln!(
                    self.code,
                    "  {reg} = call fastcc {ret_ll} @vn2_fn_{fn_id}({})",
                    arg_pieces.join(", ")
                );
                reg
            }
            _ => "0".to_string(),
        }
    }

    fn emit_selector(&mut self, selector: &Selector, plan: &HistoryPlan) -> String {
        match selector {
            Selector::Top => "true".to_string(),
            Selector::Atom(site, alt_pos) => {
                let bit = plan.bit[*site][*alt_pos].expect("mentioned atom has a bit");
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = load i1, ptr %bit_{bit}");
                reg
            }
            Selector::Not(inner) => {
                let v = self.emit_selector(inner, plan);
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = xor i1 {v}, true");
                reg
            }
            Selector::And(l, r) => {
                let lv = self.emit_selector(l, plan);
                let rv = self.emit_selector(r, plan);
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = and i1 {lv}, {rv}");
                reg
            }
            Selector::Or(l, r) => {
                let lv = self.emit_selector(l, plan);
                let rv = self.emit_selector(r, plan);
                let reg = self.fresh_reg();
                let _ = writeln!(self.code, "  {reg} = or i1 {lv}, {rv}");
                reg
            }
        }
    }
}

fn emit_function(
    out: &mut String,
    program: &Program,
    id: FunctionId,
    function: &Function,
    usize_bits: u32,
) {
    let mut params = Vec::new();
    for (idx, param) in function.parameters.iter().enumerate() {
        params.push(format!("{} %p{idx}", llvm_ty(&param.ty, usize_bits)));
    }
    let ret_ll = llvm_ty(&function.result, usize_bits);
    let _ = writeln!(
        out,
        "define internal fastcc {ret_ll} @vn2_fn_{id}({}) alwaysinline nounwind {{",
        params.join(", ")
    );
    let _ = writeln!(out, "entry:");
    let mut emitter = IrEmitter::new(program, usize_bits);
    let res = emitter.emit_expr(&function.body, &VarAccess::DirectParams);
    for alloca in &emitter.allocas {
        let _ = writeln!(out, "{alloca}");
    }
    out.push_str(&emitter.code);
    let _ = writeln!(out, "  ret {ret_ll} {res}");
    let _ = writeln!(out, "}}");
}

fn emit_control(
    out: &mut String,
    program: &Program,
    id: ControlId,
    control: &Control,
    usize_bits: u32,
) {
    let body = control.body.as_ref().expect("a defined control");
    let plan = build_plan(body);

    let mut params = Vec::new();
    for (idx, param) in control.parameters.iter().enumerate() {
        params.push(format!("{} %p{idx}", llvm_ty(&param.ty, usize_bits)));
    }
    for (alt_idx, alt) in control.contract.iter().enumerate() {
        for (field_idx, _) in alt.fields.iter().enumerate() {
            params.push(format!("ptr %out_{alt_idx}_{field_idx}"));
        }
    }

    let _ = writeln!(
        out,
        "define internal fastcc i32 @vn2_ctrl_{id}({}) nounwind {{",
        params.join(", ")
    );

    let mut emitter = IrEmitter::new(program, usize_bits);

    // [R1]: Allocate typed slots for all variables of `body.vars`.
    for (var_id, var) in body.vars.iter().enumerate() {
        let ll_ty = llvm_ty(&var.ty, usize_bits);
        emitter
            .allocas
            .push(format!("  %slot_{var_id} = alloca {ll_ty}"));
    }
    // [R3]: Allocate `i1` bits ONLY for atoms mentioned in some selector.
    for bit_id in 0..plan.bits {
        emitter.allocas.push(format!("  %bit_{bit_id} = alloca i1"));
    }

    // Initialize parameters and history bits in `entry`.
    for (idx, param) in control.parameters.iter().enumerate() {
        let ll_ty = llvm_ty(&param.ty, usize_bits);
        let _ = writeln!(emitter.code, "  store {ll_ty} %p{idx}, ptr %slot_{idx}");
    }
    for (var_id, var) in body.vars.iter().enumerate().skip(control.parameters.len()) {
        let ll_ty = llvm_ty(&var.ty, usize_bits);
        let zero = if var.ty == Type::Bool { "false" } else { "0" };
        let _ = writeln!(emitter.code, "  store {ll_ty} {zero}, ptr %slot_{var_id}");
    }
    for bit_id in 0..plan.bits {
        let _ = writeln!(emitter.code, "  store i1 false, ptr %bit_{bit_id}");
    }
    let _ = writeln!(emitter.code, "  br label %b0_entry");

    // Precompute parent loop info for every loop body `inner_body_id`.
    #[allow(clippy::type_complexity)]
    let mut loop_parents: HashMap<BodyId, (BodyId, usize, SiteId, Vec<Vec<VarId>>)> =
        HashMap::new();
    for (b_id, b) in body.bodies.iter().enumerate() {
        for (s_idx, stmt) in b.statements.iter().enumerate() {
            if let Command::Loop {
                site,
                body: inner,
                arms,
                ..
            } = &stmt.command
            {
                loop_parents.insert(*inner, (b_id, s_idx, *site, arms.clone()));
            }
        }
    }

    // Emit each body `0..body.bodies.len()`.
    for (b_id, b) in body.bodies.iter().enumerate() {
        let _ = writeln!(emitter.code, "b{b_id}_entry:");
        if b_id > 0 {
            // [R3]: Entering a loop body clears its own history bits.
            for &bit_id in &plan.own[b_id] {
                let _ = writeln!(emitter.code, "  store i1 false, ptr %bit_{bit_id}");
            }
        }
        if b.statements.is_empty() {
            let _ = writeln!(emitter.code, "  br label %b{b_id}_end");
        } else {
            let _ = writeln!(emitter.code, "  br label %b{b_id}_s0");
        }

        for (s_idx, stmt) in b.statements.iter().enumerate() {
            let next_lbl = if s_idx + 1 < b.statements.len() {
                format!("b{b_id}_s{}", s_idx + 1)
            } else {
                format!("b{b_id}_end")
            };

            let _ = writeln!(emitter.code, "b{b_id}_s{s_idx}:");

            // [R2]: `Command::Oblige` has no runtime behavior at all, and a
            // direct `site.alt ▷ ⊥` statement is already handled as
            // `unreachable` in the decision's `switch`.
            if matches!(stmt.command, Command::Oblige(_))
                || (is_direct_impossible_arm(stmt)
                    && matches!(stmt.selector, Selector::Atom(s, a) if plan.bit[s][a].is_none()))
            {
                let _ = writeln!(emitter.code, "  br label %{next_lbl}");
                continue;
            }

            if !matches!(stmt.selector, Selector::Top) {
                let sel_reg = emitter.emit_selector(&stmt.selector, &plan);
                let run_lbl = format!("b{b_id}_run{s_idx}");
                let _ = writeln!(
                    emitter.code,
                    "  br i1 {sel_reg}, label %{run_lbl}, label %{next_lbl}"
                );
                let _ = writeln!(emitter.code, "{run_lbl}:");
            }

            match &stmt.command {
                Command::Bind { var, value } | Command::Assign { var, value } => {
                    let v = emitter.emit_expr(value, &VarAccess::ControlSlots);
                    let ll_ty = llvm_ty(&body.vars[*var].ty, usize_bits);
                    let _ = writeln!(emitter.code, "  store {ll_ty} {v}, ptr %slot_{var}");
                    let _ = writeln!(emitter.code, "  br label %{next_lbl}");
                }
                Command::Oblige(_) => unreachable!("handled above"),
                Command::Test { site, condition } => {
                    let cond = emitter.emit_expr(condition, &VarAccess::ControlSlots);
                    if let Some(b_true) = plan.bit[*site][0] {
                        let _ = writeln!(emitter.code, "  store i1 {cond}, ptr %bit_{b_true}");
                    }
                    if let Some(b_false) = plan.bit[*site][1] {
                        let not_reg = emitter.fresh_reg();
                        let _ = writeln!(emitter.code, "  {not_reg} = xor i1 {cond}, true");
                        let _ = writeln!(emitter.code, "  store i1 {not_reg}, ptr %bit_{b_false}");
                    }
                    let _ = writeln!(emitter.code, "  br label %{next_lbl}");
                }
                Command::Decide {
                    site,
                    operation,
                    op_kind: _,
                    arguments,
                    arms,
                } => {
                    let callee = &program.controls[*operation];
                    let is_extern = callee.body.is_none();

                    let mut call_args = Vec::new();
                    for (arg_expr, param) in arguments.iter().zip(&callee.parameters) {
                        let v = emitter.emit_expr(arg_expr, &VarAccess::ControlSlots);
                        if is_extern && param.ty == Type::Bool {
                            let ext = emitter.fresh_reg();
                            let _ = writeln!(emitter.code, "  {ext} = zext i1 {v} to i8");
                            call_args.push(format!("i8 {ext}"));
                        } else {
                            call_args.push(format!("{} {v}", llvm_ty(&param.ty, usize_bits)));
                        }
                    }

                    // Allocate temporary payload slots for the callee's contract.
                    let mut out_slots: Vec<Vec<String>> = Vec::new();
                    for contract_alt in &callee.contract {
                        let mut alt_slots = Vec::new();
                        for field in &contract_alt.fields {
                            let slot_ty = if is_extern {
                                c_abi_llvm_ty(&field.ty, usize_bits)
                            } else {
                                llvm_ty(&field.ty, usize_bits)
                            };
                            let ptr = emitter.alloc_temp(slot_ty);
                            call_args.push(format!("ptr {ptr}"));
                            alt_slots.push(ptr);
                        }
                        out_slots.push(alt_slots);
                    }

                    let tag_reg = emitter.fresh_reg();
                    if is_extern {
                        let _ = writeln!(
                            emitter.code,
                            "  {tag_reg} = call i32 @vn2_ext_{}({})",
                            callee.name,
                            call_args.join(", ")
                        );
                    } else {
                        let _ = writeln!(
                            emitter.code,
                            "  {tag_reg} = call fastcc i32 @vn2_ctrl_{operation}({})",
                            call_args.join(", ")
                        );
                    }

                    let unreach_lbl = emitter.fresh_label(&format!("b{b_id}_s{s_idx}_unreach"));
                    let mut arm_labels = Vec::new();
                    let mut switch_cases = Vec::new();

                    for (pos, arm) in arms.iter().enumerate() {
                        // [R2]: If a subsequent statement in this body is
                        // `site.pos ▷ ⊥`, this arm is proved impossible and
                        // branches directly to `unreachable`!
                        let directly_impossible = b.statements[(s_idx + 1)..].iter().any(|later| {
                            matches!(later.command, Command::Impossible)
                                && later.selector == Selector::Atom(*site, pos)
                        });
                        if directly_impossible && plan.bit[*site][pos].is_none() {
                            switch_cases
                                .push(format!("i32 {}, label %{unreach_lbl}", arm.alternative));
                            arm_labels.push(None);
                        } else {
                            let arm_lbl =
                                emitter.fresh_label(&format!("b{b_id}_s{s_idx}_arm{pos}"));
                            switch_cases.push(format!("i32 {}, label %{arm_lbl}", arm.alternative));
                            arm_labels.push(Some(arm_lbl));
                        }
                    }

                    let _ = writeln!(
                        emitter.code,
                        "  switch i32 {tag_reg}, label %{unreach_lbl} [ {} ]",
                        switch_cases.join(" ")
                    );
                    let _ = writeln!(emitter.code, "{unreach_lbl}:");
                    let _ = writeln!(emitter.code, "  unreachable");

                    for (pos, arm) in arms.iter().enumerate() {
                        let Some(arm_lbl) = &arm_labels[pos] else {
                            continue;
                        };
                        let _ = writeln!(emitter.code, "{arm_lbl}:");
                        if let Some(bit_id) = plan.bit[*site][pos] {
                            let _ = writeln!(emitter.code, "  store i1 true, ptr %bit_{bit_id}");
                        }
                        let contract_alt = &callee.contract[arm.alternative];
                        for (field_idx, &var_id) in arm.vars.iter().enumerate() {
                            let field_ty = &contract_alt.fields[field_idx].ty;
                            let ptr = &out_slots[arm.alternative][field_idx];
                            if is_extern && *field_ty == Type::Bool {
                                let raw = emitter.fresh_reg();
                                let _ = writeln!(emitter.code, "  {raw} = load i8, ptr {ptr}");
                                let b_reg = emitter.fresh_reg();
                                let _ = writeln!(emitter.code, "  {b_reg} = icmp ne i8 {raw}, 0");
                                let _ = writeln!(
                                    emitter.code,
                                    "  store i1 {b_reg}, ptr %slot_{var_id}"
                                );
                            } else {
                                let ll_ty = llvm_ty(field_ty, usize_bits);
                                let val_reg = emitter.fresh_reg();
                                let _ =
                                    writeln!(emitter.code, "  {val_reg} = load {ll_ty}, ptr {ptr}");
                                let _ = writeln!(
                                    emitter.code,
                                    "  store {ll_ty} {val_reg}, ptr %slot_{var_id}"
                                );
                            }
                        }
                        let _ = writeln!(emitter.code, "  br label %{next_lbl}");
                    }
                }
                Command::Loop {
                    body: inner,
                    initial,
                    ..
                } => {
                    let head = body.bodies[*inner].head.as_ref().expect("a loop head");
                    let mut init_vals = Vec::new();
                    for expr in initial {
                        init_vals.push(emitter.emit_expr(expr, &VarAccess::ControlSlots));
                    }
                    for (&param_var, val) in head.parameters.iter().zip(init_vals) {
                        let ll_ty = llvm_ty(&body.vars[param_var].ty, usize_bits);
                        let _ =
                            writeln!(emitter.code, "  store {ll_ty} {val}, ptr %slot_{param_var}");
                    }
                    let _ = writeln!(emitter.code, "  br label %b{inner}_entry");
                }
                Command::Again {
                    body: target,
                    arguments,
                } => {
                    let head = body.bodies[*target].head.as_ref().expect("a loop head");
                    let mut arg_vals = Vec::new();
                    for expr in arguments {
                        arg_vals.push(emitter.emit_expr(expr, &VarAccess::ControlSlots));
                    }
                    for (&param_var, val) in head.parameters.iter().zip(arg_vals) {
                        let ll_ty = llvm_ty(&body.vars[param_var].ty, usize_bits);
                        let _ =
                            writeln!(emitter.code, "  store {ll_ty} {val}, ptr %slot_{param_var}");
                    }
                    let _ = writeln!(emitter.code, "  br label %b{target}_entry");
                }
                Command::Leave {
                    alternative,
                    payload,
                } => {
                    let mut pay_vals = Vec::new();
                    for expr in payload {
                        pay_vals.push(emitter.emit_expr(expr, &VarAccess::ControlSlots));
                    }
                    let (parent_body, loop_stmt_idx, loop_site, loop_arms) = &loop_parents[&b_id];
                    if let Some(bit_id) = plan.bit[*loop_site][*alternative] {
                        let _ = writeln!(emitter.code, "  store i1 true, ptr %bit_{bit_id}");
                    }
                    for (&var_id, val) in loop_arms[*alternative].iter().zip(pay_vals) {
                        let ll_ty = llvm_ty(&body.vars[var_id].ty, usize_bits);
                        let _ = writeln!(emitter.code, "  store {ll_ty} {val}, ptr %slot_{var_id}");
                    }
                    let parent_len = body.bodies[*parent_body].statements.len();
                    let resume_lbl = if loop_stmt_idx + 1 < parent_len {
                        format!("b{parent_body}_s{}", loop_stmt_idx + 1)
                    } else {
                        format!("b{parent_body}_end")
                    };
                    let _ = writeln!(emitter.code, "  br label %{resume_lbl}");
                }
                Command::Exit {
                    alternative,
                    payload,
                } => {
                    let mut pay_vals = Vec::new();
                    for expr in payload {
                        pay_vals.push(emitter.emit_expr(expr, &VarAccess::ControlSlots));
                    }
                    let contract_alt = &control.contract[*alternative];
                    for (field_idx, (field, val)) in
                        contract_alt.fields.iter().zip(pay_vals).enumerate()
                    {
                        let ll_ty = llvm_ty(&field.ty, usize_bits);
                        let _ = writeln!(
                            emitter.code,
                            "  store {ll_ty} {val}, ptr %out_{alternative}_{field_idx}"
                        );
                    }
                    let _ = writeln!(emitter.code, "  ret i32 {alternative}");
                }
                Command::Impossible => {
                    // [R2]: `⊥` has no behavior and emits `unreachable`.
                    let _ = writeln!(emitter.code, "  unreachable");
                }
                _ => {
                    let _ = writeln!(emitter.code, "  br label %{next_lbl}");
                }
            }
        }

        // [K7], [R2]: Reaching the end of a body is impossible in a checked program.
        let _ = writeln!(emitter.code, "b{b_id}_end:");
        let _ = writeln!(emitter.code, "  unreachable");
    }

    let _ = writeln!(out, "entry:");
    for alloca in &emitter.allocas {
        let _ = writeln!(out, "{alloca}");
    }
    out.push_str(&emitter.code);
    let _ = writeln!(out, "}}");
}

fn emit_control_c_wrapper(out: &mut String, id: ControlId, control: &Control, usize_bits: u32) {
    let mut c_params = Vec::new();
    for (idx, param) in control.parameters.iter().enumerate() {
        c_params.push(format!("{} %p{idx}", c_abi_llvm_ty(&param.ty, usize_bits)));
    }
    for (alt_idx, alt) in control.contract.iter().enumerate() {
        for (field_idx, _) in alt.fields.iter().enumerate() {
            c_params.push(format!("ptr %c_out_{alt_idx}_{field_idx}"));
        }
    }

    let _ = writeln!(
        out,
        "define i32 @vn2_call_{}({}) nounwind {{",
        control.name,
        c_params.join(", ")
    );
    let _ = writeln!(out, "entry:");

    let mut internal_args = Vec::new();
    let mut bool_outs = Vec::new();

    for (idx, param) in control.parameters.iter().enumerate() {
        if param.ty == Type::Bool {
            let _ = writeln!(out, "  %b_in_{idx} = icmp ne i8 %p{idx}, 0");
            internal_args.push(format!("i1 %b_in_{idx}"));
        } else {
            internal_args.push(format!("{} %p{idx}", llvm_ty(&param.ty, usize_bits)));
        }
    }

    for (alt_idx, alt) in control.contract.iter().enumerate() {
        for (field_idx, field) in alt.fields.iter().enumerate() {
            if field.ty == Type::Bool {
                let tmp = format!("%b_out_{alt_idx}_{field_idx}");
                let _ = writeln!(out, "  {tmp} = alloca i1");
                internal_args.push(format!("ptr {tmp}"));
                bool_outs.push((alt_idx, field_idx, tmp));
            } else {
                internal_args.push(format!("ptr %c_out_{alt_idx}_{field_idx}"));
            }
        }
    }

    let _ = writeln!(
        out,
        "  %tag = call fastcc i32 @vn2_ctrl_{id}({})",
        internal_args.join(", ")
    );

    for (alt_idx, field_idx, tmp) in bool_outs {
        let _ = writeln!(out, "  %bv_{alt_idx}_{field_idx} = load i1, ptr {tmp}");
        let _ = writeln!(
            out,
            "  %bi_{alt_idx}_{field_idx} = zext i1 %bv_{alt_idx}_{field_idx} to i8"
        );
        let _ = writeln!(
            out,
            "  store i8 %bi_{alt_idx}_{field_idx}, ptr %c_out_{alt_idx}_{field_idx}"
        );
    }

    let _ = writeln!(out, "  ret i32 %tag");
    let _ = writeln!(out, "}}");
}

// ---------------------------------------------------------------------------
// Standalone Native Execution Harness (`clang -x ir -O0` / `-O2`)
// ---------------------------------------------------------------------------

fn value_as_i64(value: &Value, ty: &Type, usize_bits: u32) -> i64 {
    match (value, ty) {
        (Value::Bool(b), _) => i64::from(*b),
        (Value::Int(v), Type::Int(int_ty)) => {
            let bits = match int_ty {
                IntType::U8 | IntType::I8 => 8,
                IntType::U16 | IntType::I16 => 16,
                IntType::U32 | IntType::I32 => 32,
                IntType::U64 | IntType::I64 => 64,
                IntType::Usize => usize_bits,
            };
            let raw = v.to_i128().unwrap_or(0) as u64;
            let masked = if bits == 64 {
                raw
            } else {
                raw & ((1u64 << bits) - 1)
            };
            if int_ty.is_signed() && bits < 64 {
                let shift = 64 - bits;
                ((masked << shift) as i64) >> shift
            } else {
                masked as i64
            }
        }
        (Value::Int(v), Type::Bool) => i64::from(!v.is_zero()),
        _ => 0,
    }
}

fn checked_arith_kind(name: &str) -> Option<&'static str> {
    [
        "checked_add",
        "checked_sub",
        "checked_mul",
        "checked_div",
        "checked_rem",
    ]
    .into_iter()
    .find(|prefix| name.starts_with(*prefix))
}

/// Emits a complete runnable LLVM IR program that executes `specs` natively
/// and prints each run's trace and outcome.
pub fn emit_native_harness_llvm(
    program: &Program,
    specs: &[NativeRunSpec],
    usize_bits: u32,
) -> String {
    let mut ir = emit_llvm(program, usize_bits);

    // Replace `declare i32 @vn2_ext_<name>(...)` with concrete definitions backed
    // by the scripted host table and checked arithmetic.
    let mut filtered_lines = Vec::new();
    for line in ir.lines() {
        if !line.starts_with("declare i32 @vn2_ext_") {
            filtered_lines.push(line.to_string());
        }
    }
    ir = filtered_lines.join("\n");
    ir.push('\n');

    let _ = writeln!(ir, "declare i32 @printf(ptr, ...)");
    let _ = writeln!(ir, "declare void @exit(i32)");
    let _ = writeln!(
        ir,
        "@fmt_event = private unnamed_addr constant [26 x i8] c\"EVENT %d %d %lld %d %lld\\0A\\00\""
    );
    let _ = writeln!(
        ir,
        "@fmt_done = private unnamed_addr constant [22 x i8] c\"DONE %d %d %lld %lld\\0A\\00\""
    );
    let _ = writeln!(
        ir,
        "@fmt_host_fail = private unnamed_addr constant [13 x i8] c\"HOST_FAILED\\0A\\00\""
    );
    let _ = writeln!(ir, "@script_pos = internal global i64 0");
    let _ = writeln!(ir, "@use_arith = internal global i1 false");

    // Flatten all scripted answers across `specs`: each row is `[4 x i64]`:
    // `[control_id, alt_idx, payload_0, payload_1]`.
    let mut flat_answers: Vec<[i64; 4]> = Vec::new();
    let mut run_offsets: Vec<(usize, usize)> = Vec::new();

    for spec in specs {
        let start = flat_answers.len();
        for (ctrl_name, ans) in &spec.answers {
            let ctrl_id = program.control(ctrl_name).expect("known extern control");
            let ctrl = &program.controls[ctrl_id];
            let alt_idx = ctrl
                .contract
                .iter()
                .position(|a| a.name == ans.alternative)
                .expect("known alternative");
            let fields = &ctrl.contract[alt_idx].fields;
            let p0 = ans
                .payload
                .first()
                .map(|v| value_as_i64(v, &fields[0].ty, usize_bits))
                .unwrap_or(0);
            let p1 = ans
                .payload
                .get(1)
                .map(|v| value_as_i64(v, &fields[1].ty, usize_bits))
                .unwrap_or(0);
            flat_answers.push([ctrl_id as i64, alt_idx as i64, p0, p1]);
        }
        let end = flat_answers.len();
        run_offsets.push((start, end));
    }

    let table_len = flat_answers.len().max(1);
    let _ = writeln!(ir, "@script_end = internal global i64 0");
    let mut table_entries = Vec::new();
    if flat_answers.is_empty() {
        table_entries.push("[4 x i64] [i64 -1, i64 0, i64 0, i64 0]".to_string());
    } else {
        for row in &flat_answers {
            table_entries.push(format!(
                "[4 x i64] [i64 {}, i64 {}, i64 {}, i64 {}]",
                row[0], row[1], row[2], row[3]
            ));
        }
    }
    let _ = writeln!(
        ir,
        "@script_table = internal constant [{table_len} x [4 x i64]] [{}]",
        table_entries.join(", ")
    );
    let _ = writeln!(ir);

    // Emit `@vn2_ext_<name>` for every extern control.
    for (ctrl_id, control) in program.controls.iter().enumerate() {
        if control.body.is_some() {
            continue;
        }
        let mut params = Vec::new();
        for (idx, param) in control.parameters.iter().enumerate() {
            params.push(format!("{} %p{idx}", c_abi_llvm_ty(&param.ty, usize_bits)));
        }
        for (alt_idx, alt) in control.contract.iter().enumerate() {
            for (field_idx, _) in alt.fields.iter().enumerate() {
                params.push(format!("ptr %out_{alt_idx}_{field_idx}"));
            }
        }
        let _ = writeln!(
            ir,
            "define i32 @vn2_ext_{}({}) nounwind {{",
            control.name,
            params.join(", ")
        );
        let _ = writeln!(ir, "entry:");

        // If `control` is a checked arithmetic operation and `@use_arith` is true,
        // compute it directly in `i128`!
        if let Some(op_kind) = checked_arith_kind(&control.name)
            && control.parameters.len() == 2
            && control.contract.len() == 2
            && control.contract[0].fields.len() == 1
            && control.contract[1].fields.is_empty()
            && let Type::Int(res_int_ty) = control.contract[0].fields[0].ty
        {
            let _ = writeln!(ir, "  %arith_on = load i1, ptr @use_arith");
            let _ = writeln!(ir, "  br i1 %arith_on, label %do_arith, label %from_script");
            let _ = writeln!(ir, "do_arith:");
            let p0_ty = &control.parameters[0].ty;
            let p1_ty = &control.parameters[1].ty;
            let p0_ll = c_abi_llvm_ty(p0_ty, usize_bits);
            let p1_ll = c_abi_llvm_ty(p1_ty, usize_bits);
            let ext0 = if matches!(p0_ty, Type::Int(t) if t.is_signed()) {
                "sext"
            } else {
                "zext"
            };
            let ext1 = if matches!(p1_ty, Type::Int(t) if t.is_signed()) {
                "sext"
            } else {
                "zext"
            };
            let _ = writeln!(ir, "  %a128 = {ext0} {p0_ll} %p0 to i128");
            let _ = writeln!(ir, "  %b128 = {ext1} {p1_ll} %p1 to i128");
            if op_kind == "checked_div" || op_kind == "checked_rem" {
                let _ = writeln!(ir, "  %z = icmp eq i128 %b128, 0");
                let _ = writeln!(ir, "  br i1 %z, label %host_fail, label %div_ok");
                let _ = writeln!(ir, "div_ok:");
            }
            let arith_instr = match op_kind {
                "checked_add" => "add",
                "checked_sub" => "sub",
                "checked_mul" => "mul",
                "checked_div" => "sdiv",
                "checked_rem" => "srem",
                _ => unreachable!(),
            };
            let _ = writeln!(ir, "  %res128 = {arith_instr} i128 %a128, %b128");
            let usize_max = Int::power_of_two(usize_bits).minus(&Int::from_u64(1));
            let (lo, hi) = res_int_ty.bounds(&usize_max);
            let lo_i128 = lo.to_i128().unwrap();
            let hi_i128 = hi.to_i128().unwrap();
            let _ = writeln!(ir, "  %ge_lo = icmp sge i128 %res128, {lo_i128}");
            let _ = writeln!(ir, "  %le_hi = icmp sle i128 %res128, {hi_i128}");
            let _ = writeln!(ir, "  %fits = and i1 %ge_lo, %le_hi");
            let _ = writeln!(ir, "  br i1 %fits, label %arith_fits, label %arith_fails");
            let _ = writeln!(ir, "arith_fits:");
            let res_ll = c_abi_llvm_ty(&Type::Int(res_int_ty), usize_bits);
            let _ = writeln!(ir, "  %trunc_res = trunc i128 %res128 to {res_ll}");
            let _ = writeln!(ir, "  store {res_ll} %trunc_res, ptr %out_0_0");
            let _ = writeln!(ir, "  ret i32 0");
            let _ = writeln!(ir, "arith_fails:");
            let _ = writeln!(ir, "  ret i32 1");
            let _ = writeln!(ir, "from_script:");
        }

        let _ = writeln!(ir, "  %pos = load i64, ptr @script_pos");
        let _ = writeln!(ir, "  %end = load i64, ptr @script_end");
        let _ = writeln!(ir, "  %has = icmp slt i64 %pos, %end");
        let _ = writeln!(ir, "  br i1 %has, label %check_ctrl, label %host_fail");
        let _ = writeln!(ir, "check_ctrl:");
        let _ = writeln!(
            ir,
            "  %row_ptr = getelementptr [{table_len} x [4 x i64]], ptr @script_table, i64 0, i64 %pos"
        );
        let _ = writeln!(
            ir,
            "  %c_ptr = getelementptr [4 x i64], ptr %row_ptr, i64 0, i64 0"
        );
        let _ = writeln!(ir, "  %c_id = load i64, ptr %c_ptr");
        let _ = writeln!(ir, "  %match = icmp eq i64 %c_id, {ctrl_id}");
        let _ = writeln!(ir, "  br i1 %match, label %consume, label %host_fail");
        let _ = writeln!(ir, "host_fail:");
        let _ = writeln!(ir, "  call i32 (ptr, ...) @printf(ptr @fmt_host_fail)");
        let _ = writeln!(ir, "  call void @exit(i32 0)");
        let _ = writeln!(ir, "  unreachable");
        let _ = writeln!(ir, "consume:");
        let _ = writeln!(ir, "  %next_pos = add i64 %pos, 1");
        let _ = writeln!(ir, "  store i64 %next_pos, ptr @script_pos");
        let _ = writeln!(
            ir,
            "  %a_ptr = getelementptr [4 x i64], ptr %row_ptr, i64 0, i64 1"
        );
        let _ = writeln!(ir, "  %a_i64 = load i64, ptr %a_ptr");
        let _ = writeln!(ir, "  %a_i32 = trunc i64 %a_i64 to i32");
        let _ = writeln!(
            ir,
            "  %p0_ptr = getelementptr [4 x i64], ptr %row_ptr, i64 0, i64 2"
        );
        let _ = writeln!(ir, "  %pay0 = load i64, ptr %p0_ptr");

        // Store payload 0 into every alternative that has a field 0.
        for (alt_idx, alt) in control.contract.iter().enumerate() {
            if let Some(field0) = alt.fields.first() {
                let f_ll = c_abi_llvm_ty(&field0.ty, usize_bits);
                if f_ll == "i64" {
                    let _ = writeln!(ir, "  store i64 %pay0, ptr %out_{alt_idx}_0");
                } else {
                    let _ = writeln!(ir, "  %tr_{alt_idx}_0 = trunc i64 %pay0 to {f_ll}");
                    let _ = writeln!(ir, "  store {f_ll} %tr_{alt_idx}_0, ptr %out_{alt_idx}_0");
                }
            }
        }

        // If effectful, record the event!
        if !control.is_pure() {
            let arg0_i64 = if let Some(param0) = control.parameters.first() {
                let p_ll = c_abi_llvm_ty(&param0.ty, usize_bits);
                if p_ll == "i64" {
                    "%p0".to_string()
                } else {
                    let ext = if matches!(param0.ty, Type::Int(t) if t.is_signed()) {
                        "sext"
                    } else {
                        "zext"
                    };
                    let _ = writeln!(ir, "  %arg0_64 = {ext} {p_ll} %p0 to i64");
                    "%arg0_64".to_string()
                }
            } else {
                "0".to_string()
            };
            let arg_count = control.parameters.len();
            let _ = writeln!(
                ir,
                "  call i32 (ptr, ...) @printf(ptr @fmt_event, i32 {ctrl_id}, i32 {arg_count}, i64 {arg0_i64}, i32 %a_i32, i64 %pay0)"
            );
        }

        let _ = writeln!(ir, "  ret i32 %a_i32");
        let _ = writeln!(ir, "}}");
        let _ = writeln!(ir);
    }

    // Emit `@main` to run all `specs` in order.
    let _ = writeln!(ir, "define i32 @main() nounwind {{");
    let _ = writeln!(ir, "entry:");

    for (run_idx, spec) in specs.iter().enumerate() {
        let (start, end) = run_offsets[run_idx];
        let _ = writeln!(ir, "  store i64 {start}, ptr @script_pos");
        let _ = writeln!(ir, "  store i64 {end}, ptr @script_end");
        let arith_bool = if spec.arithmetic { "true" } else { "false" };
        let _ = writeln!(ir, "  store i1 {arith_bool}, ptr @use_arith");

        let ctrl_id = program.control(&spec.entry).expect("known entry control");
        let control = &program.controls[ctrl_id];

        let mut call_args = Vec::new();
        for (val, param) in spec.arguments.iter().zip(&control.parameters) {
            let ll_ty = llvm_ty(&param.ty, usize_bits);
            match (val, &param.ty) {
                (Value::Bool(b), _) => {
                    call_args.push(format!("i1 {}", if *b { "true" } else { "false" }));
                }
                (Value::Int(_), _) => {
                    let v64 = value_as_i64(val, &param.ty, usize_bits);
                    call_args.push(format!("{ll_ty} {v64}"));
                }
                _ => {
                    call_args.push(format!("{ll_ty} null"));
                }
            }
        }

        for (alt_idx, alt) in control.contract.iter().enumerate() {
            for (field_idx, field) in alt.fields.iter().enumerate() {
                let ll_ty = llvm_ty(&field.ty, usize_bits);
                let slot = format!("%r{run_idx}_out_{alt_idx}_{field_idx}");
                let _ = writeln!(ir, "  {slot} = alloca {ll_ty}");
                let zero = if field.ty == Type::Bool { "false" } else { "0" };
                let _ = writeln!(ir, "  store {ll_ty} {zero}, ptr {slot}");
                call_args.push(format!("ptr {slot}"));
            }
        }

        let _ = writeln!(
            ir,
            "  %tag_{run_idx} = call fastcc i32 @vn2_ctrl_{ctrl_id}({})",
            call_args.join(", ")
        );

        // Select payload values for the returned `%tag_{run_idx}`.
        let _ = writeln!(ir, "  %p0_{run_idx} = alloca i64");
        let _ = writeln!(ir, "  %p1_{run_idx} = alloca i64");
        let _ = writeln!(ir, "  store i64 0, ptr %p0_{run_idx}");
        let _ = writeln!(ir, "  store i64 0, ptr %p1_{run_idx}");

        for (alt_idx, alt) in control.contract.iter().enumerate() {
            if alt.fields.is_empty() {
                continue;
            }
            let _ = writeln!(
                ir,
                "  %is_{run_idx}_{alt_idx} = icmp eq i32 %tag_{run_idx}, {alt_idx}"
            );
            let copy_lbl = format!("copy_{run_idx}_{alt_idx}");
            let after_lbl = format!("after_{run_idx}_{alt_idx}");
            let _ = writeln!(
                ir,
                "  br i1 %is_{run_idx}_{alt_idx}, label %{copy_lbl}, label %{after_lbl}"
            );
            let _ = writeln!(ir, "{copy_lbl}:");
            for (field_idx, field) in alt.fields.iter().take(2).enumerate() {
                let ll_ty = llvm_ty(&field.ty, usize_bits);
                let slot = format!("%r{run_idx}_out_{alt_idx}_{field_idx}");
                let loaded = format!("%ld_{run_idx}_{alt_idx}_{field_idx}");
                let _ = writeln!(ir, "  {loaded} = load {ll_ty}, ptr {slot}");
                let ext_val = if ll_ty == "i64" {
                    loaded
                } else {
                    let ext = if matches!(field.ty, Type::Int(t) if t.is_signed()) {
                        "sext"
                    } else {
                        "zext"
                    };
                    let e_reg = format!("%ex_{run_idx}_{alt_idx}_{field_idx}");
                    let _ = writeln!(ir, "  {e_reg} = {ext} {ll_ty} {loaded} to i64");
                    e_reg
                };
                let target_p = if field_idx == 0 {
                    format!("%p0_{run_idx}")
                } else {
                    format!("%p1_{run_idx}")
                };
                let _ = writeln!(ir, "  store i64 {ext_val}, ptr {target_p}");
            }
            let _ = writeln!(ir, "  br label %{after_lbl}");
            let _ = writeln!(ir, "{after_lbl}:");
        }

        let _ = writeln!(ir, "  %v0_{run_idx} = load i64, ptr %p0_{run_idx}");
        let _ = writeln!(ir, "  %v1_{run_idx} = load i64, ptr %p1_{run_idx}");
        let _ = writeln!(
            ir,
            "  call i32 (ptr, ...) @printf(ptr @fmt_done, i32 {run_idx}, i32 %tag_{run_idx}, i64 %v0_{run_idx}, i64 %v1_{run_idx})"
        );
    }

    let _ = writeln!(ir, "  ret i32 0");
    let _ = writeln!(ir, "}}");
    ir
}

fn decode_word(word: i64, ty: &Type, usize_bits: u32) -> Value {
    match ty {
        Type::Bool => Value::Bool(word != 0),
        Type::Int(int_ty) => {
            if int_ty.is_signed() {
                Value::Int(Int::from_i128(i128::from(word)))
            } else {
                let bits = match int_ty {
                    IntType::U8 => 8,
                    IntType::U16 => 16,
                    IntType::U32 => 32,
                    IntType::U64 => 64,
                    IntType::Usize => usize_bits,
                    _ => unreachable!(),
                };
                let u = word as u64;
                let masked = if bits == 64 {
                    u
                } else {
                    u & ((1u64 << bits) - 1)
                };
                Value::Int(Int::from_u64(masked))
            }
        }
        _ => Value::Int(Int::zero()),
    }
}

/// Compiles `program` with `clang -x ir` at optimization level `opt_level`
/// (`0`, `2`, or `3`), executes all `specs` natively in a single process, and
/// returns the `(erased::End, Vec<Event>)` of each run.
pub fn run_native_batch(
    program: &Program,
    specs: &[NativeRunSpec],
    usize_bits: u32,
    opt_level: u8,
) -> Result<Vec<(erased::End, Vec<Event>)>, String> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let ir = emit_native_harness_llvm(program, specs, usize_bits);

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();
    let ll_path = tmp_dir.join(format!("vn2_native_{pid}_{id}.ll"));
    let bin_path = tmp_dir.join(format!("vn2_native_{pid}_{id}.bin"));

    std::fs::write(&ll_path, &ir).map_err(|e| format!("write {}: {e}", ll_path.display()))?;

    let opt_flag = format!("-O{}", opt_level.min(3));
    let compile = ProcessCommand::new("clang")
        .arg("-x")
        .arg("ir")
        .arg(&ll_path)
        .arg(&opt_flag)
        .arg("-Wno-override-module")
        .arg("-o")
        .arg(&bin_path)
        .output();
    let _ = std::fs::remove_file(&ll_path);

    let compile_out = compile.map_err(|e| format!("failed to invoke clang: {e}"))?;
    if !compile_out.status.success() {
        let _ = std::fs::remove_file(&bin_path);
        return Err(format!(
            "clang {} failed:\n{}\n--- LLVM IR ---\n{ir}",
            opt_flag,
            String::from_utf8_lossy(&compile_out.stderr)
        ));
    }

    let exec_out = ProcessCommand::new(&bin_path)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", bin_path.display()));
    let _ = std::fs::remove_file(&bin_path);
    let exec_out = exec_out?;
    if !exec_out.status.success() {
        return Err(format!(
            "native binary exited with {:?}:\n{}",
            exec_out.status.code(),
            String::from_utf8_lossy(&exec_out.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&exec_out.stdout);
    let mut results = Vec::new();
    let mut current_events = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["EVENT", ctrl_id_s, _arg_count_s, arg0_s, alt_idx_s, pay0_s] => {
                let ctrl_id: usize = ctrl_id_s.parse().unwrap();
                let arg0: i64 = arg0_s.parse().unwrap();
                let alt_idx: usize = alt_idx_s.parse().unwrap();
                let pay0: i64 = pay0_s.parse().unwrap();

                let ctrl = &program.controls[ctrl_id];
                let arguments = ctrl
                    .parameters
                    .first()
                    .map(|p| vec![decode_word(arg0, &p.ty, usize_bits)])
                    .unwrap_or_default();
                let alt = &ctrl.contract[alt_idx];
                let payload = alt
                    .fields
                    .first()
                    .map(|f| vec![decode_word(pay0, &f.ty, usize_bits)])
                    .unwrap_or_default();
                current_events.push(Event {
                    control: ctrl.name.clone(),
                    arguments,
                    alternative: alt.name.clone(),
                    payload,
                });
            }
            ["DONE", run_idx_s, tag_s, v0_s, v1_s] => {
                let run_idx: usize = run_idx_s.parse().unwrap();
                let tag: usize = tag_s.parse().unwrap();
                let v0: i64 = v0_s.parse().unwrap();
                let v1: i64 = v1_s.parse().unwrap();

                let spec = &specs[run_idx];
                let ctrl_id = program.control(&spec.entry).unwrap();
                let ctrl = &program.controls[ctrl_id];
                let alt = &ctrl.contract[tag];
                let mut payload = Vec::new();
                if let Some(f0) = alt.fields.first() {
                    payload.push(decode_word(v0, &f0.ty, usize_bits));
                }
                if let Some(f1) = alt.fields.get(1) {
                    payload.push(decode_word(v1, &f1.ty, usize_bits));
                }
                let events = std::mem::take(&mut current_events);
                results.push((
                    erased::End::Done {
                        alternative: alt.name.clone(),
                        payload,
                    },
                    events,
                ));
            }
            ["HOST_FAILED"] => {
                let events = std::mem::take(&mut current_events);
                results.push((
                    erased::End::HostFailed("native script exhausted or mismatched".to_string()),
                    events,
                ));
            }
            _ => {}
        }
    }

    Ok(results)
}
