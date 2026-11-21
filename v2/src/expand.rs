//! Call expansion: Theorem 4 of `docs/v2/core.md`.
//!
//! > **Theorem 4 (expansion).** Expanding a decision on a defined control in
//! > place (the control's body with fresh names and sites, its parameters bound
//! > to the arguments, and each exit replaced by the caller's continuation for
//! > that alternative) yields a checked program with the same observations.
//!
//! [`expand`] takes a [`syntax::Program`], resolves it to check well-formedness,
//! and expands every decision on a defined control in place. Any `■` (`Exit`)
//! inside a loop of a callee is first lifted out of the loop through a fresh
//! `□` (`Leave`) alternative so that all exits of a callee stand at the top
//! level of its body before inlining.

use crate::int::Int;
use crate::resolve;
use crate::syntax::{
    Alternative, Arm, Command, Control, Declaration, Expr, LoopParameter, Program, Selector,
    Statement,
};
use crate::typed::{IntType, Type};
use std::collections::{HashMap, HashSet};

/// Expands every decision on a defined control in `program` in place.
pub fn expand(program: &Program) -> Result<Program, resolve::Error> {
    // Ensure the input program is well-resolved before expanding.
    let _ = resolve::resolve(program)?;

    let mut controls_by_name: HashMap<String, Control> = HashMap::new();
    for decl in &program.declarations {
        if let Declaration::Control(control) = decl {
            controls_by_name.insert(control.name.clone(), control.clone());
        }
    }

    let mut next_id = 0usize;
    // Expand defined controls iteratively until no defined control body contains
    // a decision on a defined control.
    let max_rounds = controls_by_name.len().max(1) + 1;
    for _ in 0..max_rounds {
        let snapshot = controls_by_name.clone();
        let mut any_expanded = false;
        for control in controls_by_name.values_mut() {
            if control.external {
                continue;
            }
            if has_defined_call(&control.body, &snapshot) {
                control.body = expand_body(&control.body, &snapshot, &mut next_id);
                any_expanded = true;
            }
        }
        if !any_expanded {
            break;
        }
    }

    let mut declarations = Vec::new();
    for decl in &program.declarations {
        match decl {
            Declaration::Function(f) => declarations.push(Declaration::Function(f.clone())),
            Declaration::Control(c) => {
                declarations.push(Declaration::Control(controls_by_name[&c.name].clone()));
            }
            other => declarations.push(other.clone()),
        }
    }
    Ok(Program { declarations })
}

fn has_defined_call(statements: &[Statement], controls: &HashMap<String, Control>) -> bool {
    for stmt in statements {
        match &stmt.command {
            Command::Decide { operation, .. }
                if controls.get(operation).is_some_and(|c| !c.external) =>
            {
                return true;
            }
            Command::Loop { body, .. } if has_defined_call(body, controls) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn expand_body(
    statements: &[Statement],
    controls: &HashMap<String, Control>,
    next_id: &mut usize,
) -> Vec<Statement> {
    let mut current: Vec<Statement> = statements
        .iter()
        .map(|stmt| match &stmt.command {
            Command::Loop {
                site,
                name,
                parameters,
                invariant,
                body,
                arms,
            } => Statement {
                selector: stmt.selector.clone(),
                command: Command::Loop {
                    site: site.clone(),
                    name: name.clone(),
                    parameters: parameters.clone(),
                    invariant: invariant.clone(),
                    body: expand_body(body, controls, next_id),
                    arms: arms.clone(),
                },
                line: stmt.line,
            },
            _ => stmt.clone(),
        })
        .collect();

    let mut idx = 0usize;
    while idx < current.len() {
        let is_defined_decide = match &current[idx].command {
            Command::Decide { operation, .. } => {
                controls.get(operation).is_some_and(|c| !c.external)
            }
            _ => false,
        };
        if !is_defined_decide {
            idx += 1;
            continue;
        }

        let call_stmt = current.remove(idx);
        let call_guard = call_stmt.selector;
        let call_line = call_stmt.line;
        let Command::Decide {
            site: call_site,
            operation,
            arguments,
            arms: caller_arms,
        } = call_stmt.command
        else {
            unreachable!();
        };

        let callee = &controls[&operation];
        let call_id = *next_id;
        *next_id += 1;

        let (inlined_stmts, alt_selectors) = inline_callee(
            callee,
            &call_guard,
            call_line,
            &arguments,
            &caller_arms,
            call_id,
            next_id,
        );

        let count = inlined_stmts.len();
        for (offset, s) in inlined_stmts.into_iter().enumerate() {
            current.insert(idx + offset, s);
        }

        // Substitute `Selector::Atom(call_site, alt)` in all subsequent
        // statements of this body (and inside nested loops that follow).
        for later in &mut current[(idx + count)..] {
            substitute_site_in_stmt(later, &call_site, &alt_selectors);
        }

        idx += count;
    }

    current
}

fn inline_callee(
    callee: &Control,
    call_guard: &Selector,
    call_line: u32,
    arguments: &[Expr],
    caller_arms: &[Arm],
    call_id: usize,
    next_id: &mut usize,
) -> (Vec<Statement>, HashMap<String, Selector>) {
    // 1. Lift any `Command::Exit` inside loops of `callee` to the top level of
    //    `callee.body`.
    let lifted_body = lift_exits_in_body(&callee.body, &callee.contract, next_id);

    // 2. Build injective renamings for all variables, sites and loop names in
    //    `lifted_body` and `callee.parameters`.
    let mut var_names = HashSet::new();
    let mut site_names = HashSet::new();
    let mut loop_names = HashSet::new();
    for param in &callee.parameters {
        var_names.insert(param.name.clone());
    }
    collect_callee_names(
        &lifted_body,
        &mut var_names,
        &mut site_names,
        &mut loop_names,
    );

    let prefix = format!("_exp{call_id}_");
    let var_map: HashMap<String, String> = var_names
        .into_iter()
        .map(|v| (v.clone(), format!("{prefix}{v}")))
        .collect();
    let site_map: HashMap<String, String> = site_names
        .into_iter()
        .map(|s| (s.clone(), format!("{prefix}{s}")))
        .collect();
    let loop_map: HashMap<String, String> = loop_names
        .into_iter()
        .map(|l| (l.clone(), format!("{prefix}{l}")))
        .collect();

    let mut out = Vec::new();

    // 3. Bind each parameter of `callee` under `call_guard`.
    for (param, arg_expr) in callee.parameters.iter().zip(arguments) {
        out.push(Statement {
            selector: call_guard.clone(),
            command: Command::Bind {
                mutable: false,
                name: var_map[&param.name].clone(),
                ty: Some(param.ty.clone()),
                value: arg_expr.clone(),
            },
            line: call_line,
        });
    }

    // 4. If `callee` has a precondition `⊢ φ`, emit it as an obligation on
    //    `call_guard` so `[P3]` is preserved as `[P1]`.
    if let Some(pre) = &callee.precondition {
        let mut renamed_pre = pre.clone();
        rename_expr(&mut renamed_pre, &var_map);
        out.push(Statement {
            selector: call_guard.clone(),
            command: Command::Oblige(renamed_pre),
            line: call_line,
        });
    }

    // 5. Walk top-level statements of `lifted_body`, tracking relative
    //    selectors on which `callee` has already terminated (`■`, `⊥`, or a
    //    0-arm loop).
    let mut terminated: Vec<Selector> = Vec::new();
    let mut alt_reach: HashMap<String, Vec<Selector>> = HashMap::new();
    for alt in &callee.contract {
        alt_reach.insert(alt.name.clone(), Vec::new());
    }

    for stmt in &lifted_body {
        let mut rel_sel = stmt.selector.clone();
        rename_selector(&mut rel_sel, &site_map);
        let active = build_active_selector(call_guard, &terminated, &rel_sel);

        match &stmt.command {
            Command::Exit {
                alternative,
                payload,
            } => {
                let contract_alt = callee
                    .contract
                    .iter()
                    .find(|a| &a.name == alternative)
                    .expect("a contract alternative");
                let caller_arm = caller_arms
                    .iter()
                    .find(|a| &a.alternative == alternative)
                    .expect("a caller arm");

                let renamed_payload: Vec<Expr> = payload
                    .iter()
                    .map(|e| {
                        let mut c = e.clone();
                        rename_expr(&mut c, &var_map);
                        c
                    })
                    .collect();

                // Bind the caller's arm variables `y_i : field.ty := val_i` on `active`.
                for ((y_name, field_param), val_expr) in caller_arm
                    .names
                    .iter()
                    .zip(&contract_alt.fields)
                    .zip(&renamed_payload)
                {
                    out.push(Statement {
                        selector: active.clone(),
                        command: Command::Bind {
                            mutable: false,
                            name: y_name.clone(),
                            ty: Some(field_param.ty.clone()),
                            value: val_expr.clone(),
                        },
                        line: call_line,
                    });
                }

                // If the contract alternative has `⇐ cond`, check it as an
                // obligation on `active` (`[P4]` -> `[P1]`).
                if let Some(cond) = &contract_alt.condition {
                    let mut cond_map = var_map.clone();
                    for (field_param, y_name) in contract_alt.fields.iter().zip(&caller_arm.names) {
                        cond_map.insert(field_param.name.clone(), y_name.clone());
                    }
                    let mut renamed_cond = cond.clone();
                    rename_expr(&mut renamed_cond, &cond_map);
                    out.push(Statement {
                        selector: active.clone(),
                        command: Command::Oblige(renamed_cond),
                        line: call_line,
                    });
                }

                alt_reach
                    .get_mut(alternative)
                    .expect("known alternative")
                    .push(active);
                terminated.push(rel_sel);
            }
            Command::Impossible => {
                out.push(Statement {
                    selector: active,
                    command: Command::Impossible,
                    line: call_line,
                });
                terminated.push(rel_sel);
            }
            other => {
                let renamed_cmd = rename_command(other, &var_map, &site_map, &loop_map);
                if let Command::Loop { arms, .. } = &renamed_cmd
                    && arms.is_empty()
                {
                    terminated.push(rel_sel);
                }
                out.push(Statement {
                    selector: active,
                    command: renamed_cmd,
                    line: call_line,
                });
            }
        }
    }

    // 6. If an alternative `a` of `callee` had zero `■ a` exits (e.g. its path
    //    in `callee` ended in `⊥`), emit a dead `¬⊤ ▷ y_i : ty := default`
    //    binder for each variable `y_i` in the caller's arm so `y_i` remains
    //    declared in scope if mentioned on `¬⊤` later in the caller.
    let false_sel = Selector::Not(Box::new(Selector::Top));
    let mut alt_selectors = HashMap::new();
    for contract_alt in &callee.contract {
        let reaches = &alt_reach[&contract_alt.name];
        if reaches.is_empty() {
            if let Some(caller_arm) = caller_arms
                .iter()
                .find(|a| a.alternative == contract_alt.name)
            {
                for (y_name, field_param) in caller_arm.names.iter().zip(&contract_alt.fields) {
                    out.push(Statement {
                        selector: false_sel.clone(),
                        command: Command::Bind {
                            mutable: false,
                            name: y_name.clone(),
                            ty: Some(field_param.ty.clone()),
                            value: default_expr(&field_param.ty),
                        },
                        line: call_line,
                    });
                }
            }
            alt_selectors.insert(contract_alt.name.clone(), false_sel.clone());
        } else {
            let mut iter = reaches.iter().cloned();
            let first = iter.next().unwrap();
            let disj = iter.fold(first, |acc, s| Selector::Or(Box::new(acc), Box::new(s)));
            alt_selectors.insert(contract_alt.name.clone(), disj);
        }
    }

    (out, alt_selectors)
}

fn default_expr(ty: &Type) -> Expr {
    match ty {
        Type::Bool => Expr::Boolean(false),
        Type::Int(IntType::U8)
        | Type::Int(IntType::U16)
        | Type::Int(IntType::U32)
        | Type::Int(IntType::U64)
        | Type::Int(IntType::I8)
        | Type::Int(IntType::I16)
        | Type::Int(IntType::I32)
        | Type::Int(IntType::I64)
        | Type::Int(IntType::Usize) => Expr::Integer(Int::from_u64(0)),
        Type::String => Expr::String(String::new()),
        Type::Vector(_) => Expr::Vector(Vec::new()),
        Type::Map(_, _) => Expr::EmptyMap,
        _ => Expr::Integer(Int::from_u64(0)),
    }
}

/// Lifts any `Command::Exit` inside any `Command::Loop` of `statements` out to
/// the top level of `statements`.
fn lift_exits_in_body(
    statements: &[Statement],
    contract: &[Alternative],
    next_id: &mut usize,
) -> Vec<Statement> {
    let mut out = Vec::new();
    for stmt in statements {
        match &stmt.command {
            Command::Loop {
                site,
                name,
                parameters,
                invariant,
                body,
                arms,
            } => {
                // First lift exits inside any nested loops of `body`.
                let inner_lifted = lift_exits_in_body(body, contract, next_id);
                let mut new_loop_body = Vec::new();
                let mut new_arms = arms.clone();
                let mut post_exits: Vec<(String, String, Vec<String>)> = Vec::new();

                for inner_stmt in inner_lifted {
                    if let Command::Exit {
                        alternative,
                        payload,
                    } = inner_stmt.command
                    {
                        let contract_alt = contract
                            .iter()
                            .find(|a| a.name == alternative)
                            .expect("a contract alternative");
                        let lift_id = *next_id;
                        *next_id += 1;
                        let lift_alt = format!("_lift_exit_{lift_id}");

                        let mut leave_payload = Vec::new();
                        let mut arm_vars = Vec::new();
                        for (field_idx, (expr, field_param)) in
                            payload.into_iter().zip(&contract_alt.fields).enumerate()
                        {
                            let val_name = format!("_lift_val_{lift_id}_{field_idx}");
                            let out_name = format!("_lift_out_{lift_id}_{field_idx}");
                            new_loop_body.push(Statement {
                                selector: inner_stmt.selector.clone(),
                                command: Command::Bind {
                                    mutable: false,
                                    name: val_name.clone(),
                                    ty: Some(field_param.ty.clone()),
                                    value: expr,
                                },
                                line: inner_stmt.line,
                            });
                            leave_payload.push(Expr::Name(val_name));
                            arm_vars.push(out_name);
                        }
                        new_loop_body.push(Statement {
                            selector: inner_stmt.selector,
                            command: Command::Leave {
                                alternative: lift_alt.clone(),
                                payload: leave_payload,
                            },
                            line: inner_stmt.line,
                        });
                        new_arms.push(Arm {
                            alternative: lift_alt.clone(),
                            names: arm_vars.clone(),
                        });
                        post_exits.push((lift_alt, alternative, arm_vars));
                    } else {
                        new_loop_body.push(inner_stmt);
                    }
                }

                out.push(Statement {
                    selector: stmt.selector.clone(),
                    command: Command::Loop {
                        site: site.clone(),
                        name: name.clone(),
                        parameters: parameters.clone(),
                        invariant: invariant.clone(),
                        body: new_loop_body,
                        arms: new_arms,
                    },
                    line: stmt.line,
                });

                for (lift_alt, orig_alt, arm_vars) in post_exits {
                    out.push(Statement {
                        selector: Selector::Atom(site.clone(), lift_alt),
                        command: Command::Exit {
                            alternative: orig_alt,
                            payload: arm_vars.into_iter().map(Expr::Name).collect(),
                        },
                        line: stmt.line,
                    });
                }
            }
            _ => out.push(stmt.clone()),
        }
    }
    out
}

fn build_active_selector(
    call_guard: &Selector,
    terminated: &[Selector],
    rel_sel: &Selector,
) -> Selector {
    let mut acc = call_guard.clone();
    for term in terminated {
        let not_term = match term {
            Selector::Top => Selector::Not(Box::new(Selector::Top)),
            other => Selector::Not(Box::new(other.clone())),
        };
        acc = and_sel(acc, not_term);
    }
    and_sel(acc, rel_sel.clone())
}

fn and_sel(left: Selector, right: Selector) -> Selector {
    match (&left, &right) {
        (Selector::Top, _) => right,
        (_, Selector::Top) => left,
        _ => Selector::And(Box::new(left), Box::new(right)),
    }
}

fn collect_callee_names(
    statements: &[Statement],
    vars: &mut HashSet<String>,
    sites: &mut HashSet<String>,
    loops: &mut HashSet<String>,
) {
    for stmt in statements {
        match &stmt.command {
            Command::Bind { name, .. } | Command::Assign { name, .. } => {
                vars.insert(name.clone());
            }
            Command::Test { site, .. } => {
                sites.insert(site.clone());
            }
            Command::Decide { site, arms, .. } => {
                sites.insert(site.clone());
                for arm in arms {
                    for v in &arm.names {
                        vars.insert(v.clone());
                    }
                }
            }
            Command::Loop {
                site,
                name,
                parameters,
                body,
                arms,
                ..
            } => {
                sites.insert(site.clone());
                loops.insert(name.clone());
                for p in parameters {
                    vars.insert(p.name.clone());
                }
                for arm in arms {
                    for v in &arm.names {
                        vars.insert(v.clone());
                    }
                }
                collect_callee_names(body, vars, sites, loops);
            }
            _ => {}
        }
    }
}

fn rename_selector(selector: &mut Selector, site_map: &HashMap<String, String>) {
    match selector {
        Selector::Top => {}
        Selector::Atom(site, _) => {
            if let Some(new_site) = site_map.get(site) {
                *site = new_site.clone();
            }
        }
        Selector::Not(inner) => rename_selector(inner, site_map),
        Selector::And(l, r) | Selector::Or(l, r) => {
            rename_selector(l, site_map);
            rename_selector(r, site_map);
        }
    }
}

fn rename_expr(expr: &mut Expr, var_map: &HashMap<String, String>) {
    match expr {
        Expr::Integer(_) | Expr::Boolean(_) | Expr::String(_) | Expr::EmptyMap => {}
        Expr::Name(name) | Expr::Old(name) => {
            if let Some(new_name) = var_map.get(name) {
                *name = new_name.clone();
            }
        }
        Expr::Unary(_, inner)
        | Expr::Move(inner)
        | Expr::Borrow(_, inner)
        | Expr::Field(inner, _)
        | Expr::Len(inner)
        | Expr::Clone(inner) => rename_expr(inner, var_map),
        Expr::Binary(_, l, r) | Expr::Index(l, r) => {
            rename_expr(l, var_map);
            rename_expr(r, var_map);
        }
        Expr::Call(_, args)
        | Expr::Struct(_, args)
        | Expr::Variant(_, _, args)
        | Expr::Vector(args) => {
            for arg in args {
                rename_expr(arg, var_map);
            }
        }
    }
}

fn rename_command(
    command: &Command,
    var_map: &HashMap<String, String>,
    site_map: &HashMap<String, String>,
    loop_map: &HashMap<String, String>,
) -> Command {
    match command {
        Command::Bind {
            mutable,
            name,
            ty,
            value,
        } => {
            let mut v = value.clone();
            rename_expr(&mut v, var_map);
            Command::Bind {
                mutable: *mutable,
                name: var_map.get(name).cloned().unwrap_or_else(|| name.clone()),
                ty: ty.clone(),
                value: v,
            }
        }
        Command::Assign { name, value } => {
            let mut v = value.clone();
            rename_expr(&mut v, var_map);
            Command::Assign {
                name: var_map.get(name).cloned().unwrap_or_else(|| name.clone()),
                value: v,
            }
        }
        Command::Oblige(formula) => {
            let mut f = formula.clone();
            rename_expr(&mut f, var_map);
            Command::Oblige(f)
        }
        Command::Test { site, condition } => {
            let mut c = condition.clone();
            rename_expr(&mut c, var_map);
            Command::Test {
                site: site_map.get(site).cloned().unwrap_or_else(|| site.clone()),
                condition: c,
            }
        }
        Command::Decide {
            site,
            operation,
            arguments,
            arms,
        } => {
            let new_args = arguments
                .iter()
                .map(|a| {
                    let mut c = a.clone();
                    rename_expr(&mut c, var_map);
                    c
                })
                .collect();
            let new_arms = arms
                .iter()
                .map(|arm| Arm {
                    alternative: arm.alternative.clone(),
                    names: arm
                        .names
                        .iter()
                        .map(|n| var_map.get(n).cloned().unwrap_or_else(|| n.clone()))
                        .collect(),
                })
                .collect();
            Command::Decide {
                site: site_map.get(site).cloned().unwrap_or_else(|| site.clone()),
                operation: operation.clone(),
                arguments: new_args,
                arms: new_arms,
            }
        }
        Command::Loop {
            site,
            name,
            parameters,
            invariant,
            body,
            arms,
        } => {
            let new_params = parameters
                .iter()
                .map(|p| {
                    let mut init = p.initial.clone();
                    rename_expr(&mut init, var_map);
                    LoopParameter {
                        mutable: p.mutable,
                        name: var_map
                            .get(&p.name)
                            .cloned()
                            .unwrap_or_else(|| p.name.clone()),
                        ty: p.ty.clone(),
                        initial: init,
                    }
                })
                .collect();
            let new_inv = invariant.as_ref().map(|inv| {
                let mut c = inv.clone();
                rename_expr(&mut c, var_map);
                c
            });
            let new_body = body
                .iter()
                .map(|s| {
                    let mut sel = s.selector.clone();
                    rename_selector(&mut sel, site_map);
                    Statement {
                        selector: sel,
                        command: rename_command(&s.command, var_map, site_map, loop_map),
                        line: s.line,
                    }
                })
                .collect();
            let new_arms = arms
                .iter()
                .map(|arm| Arm {
                    alternative: arm.alternative.clone(),
                    names: arm
                        .names
                        .iter()
                        .map(|n| var_map.get(n).cloned().unwrap_or_else(|| n.clone()))
                        .collect(),
                })
                .collect();
            Command::Loop {
                site: site_map.get(site).cloned().unwrap_or_else(|| site.clone()),
                name: loop_map.get(name).cloned().unwrap_or_else(|| name.clone()),
                parameters: new_params,
                invariant: new_inv,
                body: new_body,
                arms: new_arms,
            }
        }
        Command::Again { name, arguments } => {
            let new_args = arguments
                .iter()
                .map(|a| {
                    let mut c = a.clone();
                    rename_expr(&mut c, var_map);
                    c
                })
                .collect();
            Command::Again {
                name: loop_map.get(name).cloned().unwrap_or_else(|| name.clone()),
                arguments: new_args,
            }
        }
        Command::Leave {
            alternative,
            payload,
        } => {
            let new_payload = payload
                .iter()
                .map(|a| {
                    let mut c = a.clone();
                    rename_expr(&mut c, var_map);
                    c
                })
                .collect();
            Command::Leave {
                alternative: alternative.clone(),
                payload: new_payload,
            }
        }
        Command::Exit {
            alternative,
            payload,
        } => {
            let new_payload = payload
                .iter()
                .map(|a| {
                    let mut c = a.clone();
                    rename_expr(&mut c, var_map);
                    c
                })
                .collect();
            Command::Exit {
                alternative: alternative.clone(),
                payload: new_payload,
            }
        }
        Command::Impossible => Command::Impossible,
        other => other.clone(),
    }
}

fn substitute_site_in_stmt(
    stmt: &mut Statement,
    site: &str,
    alt_selectors: &HashMap<String, Selector>,
) {
    stmt.selector = substitute_site_in_selector(&stmt.selector, site, alt_selectors);
    if let Command::Loop { body, .. } = &mut stmt.command {
        for child in body {
            substitute_site_in_stmt(child, site, alt_selectors);
        }
    }
}

fn substitute_site_in_selector(
    selector: &Selector,
    site: &str,
    alt_selectors: &HashMap<String, Selector>,
) -> Selector {
    match selector {
        Selector::Top => Selector::Top,
        Selector::Atom(s, alt) => {
            if s == site {
                alt_selectors
                    .get(alt)
                    .cloned()
                    .unwrap_or_else(|| Selector::Not(Box::new(Selector::Top)))
            } else {
                Selector::Atom(s.clone(), alt.clone())
            }
        }
        Selector::Not(inner) => Selector::Not(Box::new(substitute_site_in_selector(
            inner,
            site,
            alt_selectors,
        ))),
        Selector::And(l, r) => Selector::And(
            Box::new(substitute_site_in_selector(l, site, alt_selectors)),
            Box::new(substitute_site_in_selector(r, site, alt_selectors)),
        ),
        Selector::Or(l, r) => Selector::Or(
            Box::new(substitute_site_in_selector(l, site, alt_selectors)),
            Box::new(substitute_site_in_selector(r, site, alt_selectors)),
        ),
    }
}
