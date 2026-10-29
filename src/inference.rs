//! Check callees before callers so `exits auto` uses checked, closed contracts.
use crate::{
    Diagnostic, Result,
    syntax::{Expr, ExprKind, Item, Region, Statement},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn dependency_order(regions: &[Region]) -> Result<Vec<usize>> {
    fn expr(e: &Expr, calls: &mut BTreeSet<String>) {
        match &e.kind {
            ExprKind::Call(name, args) => {
                calls.insert(name.clone());
                for a in args {
                    expr(a, calls);
                }
            }
            ExprKind::Construct(_, args) | ExprKind::List(args) => {
                for a in args {
                    expr(a, calls);
                }
            }
            ExprKind::NamedRecord(_, fields) => {
                for (_, value) in fields {
                    expr(value, calls);
                }
            }
            ExprKind::Project(e, _)
            | ExprKind::Unary(e)
            | ExprKind::Borrow(e)
            | ExprKind::BorrowMutable(e)
            | ExprKind::Deref(e) => expr(e, calls),
            ExprKind::Binary(_, a, b) => {
                expr(a, calls);
                expr(b, calls);
            }
            _ => {}
        }
    }
    fn outcome(e: &crate::syntax::OutcomeExpr, calls: &mut BTreeSet<String>) {
        match e {
            crate::syntax::OutcomeExpr::Forward(_) => {}
            crate::syntax::OutcomeExpr::Construct(_, _, args) => {
                for a in args {
                    expr(a, calls)
                }
            }
            crate::syntax::OutcomeExpr::Delta(_, operand, arms) => {
                expr(operand, calls);
                for arm in arms {
                    outcome(&arm.result, calls);
                }
            }
        }
    }
    fn type_calls(ty: &str, calls: &mut BTreeSet<String>) {
        if let Some((_, t)) = crate::manifest::reference_inner(ty) {
            return type_calls(t, calls);
        }
        if let Some(p) = crate::manifest::outcome_inner(ty) {
            calls.insert(p.into());
            return;
        }
        if let Some((_, args)) = crate::collection_support::generic(ty) {
            for a in args {
                type_calls(a, calls);
            }
        }
    }
    fn items(body: &[Item], calls: &mut BTreeSet<String>) {
        for item in body {
            match item {
                Item::Guard(_, statements) => {
                    for (_, statement) in statements {
                        if let Statement::Bind(_, Some(ty), _, _)
                        | Statement::Capture(_, Some(ty), _, _)
                        | Statement::BindOutcome(_, Some(ty), _, _) = statement
                        {
                            type_calls(ty, calls);
                        }
                        match statement {
                            Statement::ExitName(_) => {}
                            Statement::Capture(_, _, e, _)
                            | Statement::Forward(e)
                            | Statement::Invoke(e) => expr(e, calls),
                            Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) => {
                                outcome(e, calls)
                            }
                            Statement::Bind(_, _, e, _)
                            | Statement::Drop(e)
                            | Statement::Emit(e)
                            | Statement::Delta(_, e, _) => expr(e, calls),
                            Statement::Assign(a, b) => {
                                expr(a, calls);
                                expr(b, calls);
                            }
                            Statement::Exit(_, args) | Statement::Again(_, args) => {
                                for a in args {
                                    expr(a, calls);
                                }
                            }
                        }
                    }
                }
                Item::Loop(_, _, params, body) => {
                    for p in params {
                        expr(&p.initial, calls);
                    }
                    items(body, calls);
                }
                Item::Over(_, _, body) => items(body, calls),
                Item::Alias(..) => {}
            }
        }
    }
    fn visit(
        i: usize,
        regions: &[Region],
        edges: &[Vec<usize>],
        states: &mut [u8],
        order: &mut Vec<usize>,
    ) -> Result<()> {
        match states[i] {
            2 => return Ok(()),
            1 => {
                return Err(Diagnostic::new(
                    "E_RECURSION",
                    regions[i].span,
                    format!(
                        "cyclic control call graph at `{}`; recursive contracts are not supported",
                        regions[i].name
                    ),
                ));
            }
            _ => {}
        }
        states[i] = 1;
        for &j in &edges[i] {
            visit(j, regions, edges, states, order)?;
        }
        states[i] = 2;
        order.push(i);
        Ok(())
    }
    let names: BTreeMap<_, _> = regions
        .iter()
        .enumerate()
        .map(|(i, r)| (r.name.as_str(), i))
        .collect();
    let edges: Vec<Vec<usize>> = regions
        .iter()
        .map(|r| {
            let mut calls = BTreeSet::new();
            items(&r.body, &mut calls);
            for p in &r.parameters {
                type_calls(&p.ty, &mut calls);
            }
            for o in &r.exits {
                for f in &o.fields {
                    type_calls(&f.ty, &mut calls);
                }
            }
            calls
                .iter()
                .filter_map(|n| names.get(n.as_str()).copied())
                .collect()
        })
        .collect();
    let mut order = vec![];
    let mut states = vec![0; regions.len()];
    for i in 0..regions.len() {
        visit(i, regions, &edges, &mut states, &mut order)?;
    }
    Ok(order)
}
