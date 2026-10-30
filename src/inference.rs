//! Check callees before callers so `exits auto` uses checked, closed contracts.
use crate::{
    Diagnostic, Result,
    syntax::{Expr, ExprKind, Item, Region, Statement},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Dependencies {
    pub order: Vec<usize>,
    pub callees: Vec<Vec<usize>>,
}

pub(crate) fn dependency_order(
    regions: &[Region],
    manifest: &crate::manifest::Manifest,
) -> Result<Dependencies> {
    fn expr(e: &Expr, calls: &mut BTreeSet<String>, manifest: &crate::manifest::Manifest) {
        if let ExprKind::Call(name, _)
        | ExprKind::Construct(name, _)
        | ExprKind::NamedRecord(name, _) = &e.kind
        {
            type_calls(
                if name.ends_with('>') {
                    name
                } else {
                    name.rsplit_once('.')
                        .map_or(name.as_str(), |(base, _)| base)
                },
                calls,
                manifest,
            );
        }
        match &e.kind {
            ExprKind::Call(name, args) => {
                calls.insert(name.clone());
                for a in args {
                    expr(a, calls, manifest);
                }
            }
            ExprKind::Construct(_, args) | ExprKind::List(args) => {
                for a in args {
                    expr(a, calls, manifest);
                }
            }
            ExprKind::NamedRecord(_, fields) => {
                for (_, value) in fields {
                    expr(value, calls, manifest);
                }
            }
            ExprKind::Project(e, _)
            | ExprKind::Unary(e)
            | ExprKind::Borrow(e)
            | ExprKind::BorrowMutable(e)
            | ExprKind::Deref(e) => expr(e, calls, manifest),
            ExprKind::Binary(_, a, b) => {
                expr(a, calls, manifest);
                expr(b, calls, manifest);
            }
            _ => {}
        }
    }
    fn outcome(
        e: &crate::syntax::OutcomeExpr,
        calls: &mut BTreeSet<String>,
        manifest: &crate::manifest::Manifest,
    ) {
        match e {
            crate::syntax::OutcomeExpr::Forward(_) => {}
            crate::syntax::OutcomeExpr::Construct(_, _, args) => {
                for a in args {
                    expr(a, calls, manifest)
                }
            }
            crate::syntax::OutcomeExpr::Delta(_, operand, arms) => {
                expr(operand, calls, manifest);
                for arm in arms {
                    for field in &arm.pattern.bindings {
                        type_calls(&field.ty, calls, manifest);
                    }
                    outcome(&arm.result, calls, manifest);
                }
            }
        }
    }
    fn type_calls(ty: &str, calls: &mut BTreeSet<String>, manifest: &crate::manifest::Manifest) {
        fn visit(
            ty: &str,
            calls: &mut BTreeSet<String>,
            manifest: &crate::manifest::Manifest,
            seen: &mut BTreeSet<String>,
        ) {
            if !seen.insert(ty.into()) {
                return;
            }
            if let Some((_, inner)) = crate::manifest::reference_inner(ty) {
                visit(inner, calls, manifest, seen);
                return;
            }
            if let Some(producer) = crate::manifest::outcome_inner(ty) {
                calls.insert(producer.into());
                return;
            }
            if let Some((_, args)) = crate::collection_support::generic(ty) {
                for arg in args {
                    visit(arg, calls, manifest, seen);
                }
            }
            match manifest.type_def(ty) {
                Some(crate::manifest::TypeDef::Record { fields, .. }) => {
                    for field in fields {
                        visit(&field.ty, calls, manifest, seen);
                    }
                }
                Some(
                    crate::manifest::TypeDef::Variant { constructors, .. }
                    | crate::manifest::TypeDef::Outcome { constructors, .. },
                ) => {
                    for field in constructors.iter().flat_map(|o| &o.fields) {
                        visit(&field.ty, calls, manifest, seen);
                    }
                }
                _ => {}
            }
        }
        visit(ty, calls, manifest, &mut BTreeSet::new());
    }
    fn statement_calls(
        statement: &Statement,
        calls: &mut BTreeSet<String>,
        manifest: &crate::manifest::Manifest,
    ) {
        if let Statement::Delta(_, _, arms) = statement {
            for arm in arms {
                for field in &arm.bindings {
                    type_calls(&field.ty, calls, manifest);
                }
                if let Some(exit) = &arm.termination {
                    statement_calls(&exit.1, calls, manifest);
                }
            }
        }
        if let Statement::Bind(_, Some(ty), _, _)
        | Statement::Capture(_, Some(ty), _, _)
        | Statement::BindOutcome(_, Some(ty), _, _) = statement
        {
            type_calls(ty, calls, manifest);
        }
        match statement {
            Statement::ExitName(_) => {}
            Statement::Capture(_, _, e, _) | Statement::Forward(e) | Statement::Invoke(e) => {
                expr(e, calls, manifest)
            }
            Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) => {
                outcome(e, calls, manifest)
            }
            Statement::Bind(_, _, e, _)
            | Statement::Drop(e)
            | Statement::Emit(e)
            | Statement::Fault(e)
            | Statement::Delta(_, e, _) => expr(e, calls, manifest),
            Statement::Assign(a, b) => {
                expr(a, calls, manifest);
                expr(b, calls, manifest);
            }
            Statement::Exit(_, args) | Statement::Again(_, args) => {
                for a in args {
                    expr(a, calls, manifest);
                }
            }
        }
    }
    fn items(body: &[Item], calls: &mut BTreeSet<String>, manifest: &crate::manifest::Manifest) {
        for item in body {
            match item {
                Item::Guard(_, statements) => {
                    for (_, statement) in statements {
                        statement_calls(statement, calls, manifest);
                    }
                }
                Item::Loop(_, _, params, body) => {
                    for p in params {
                        type_calls(&p.field.ty, calls, manifest);
                        expr(&p.initial, calls, manifest);
                    }
                    items(body, calls, manifest);
                }
                Item::Group(_, _, body) | Item::Over(_, _, body) => items(body, calls, manifest),
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
            items(&r.body, &mut calls, manifest);
            for p in &r.parameters {
                type_calls(&p.ty, &mut calls, manifest);
            }
            for o in &r.exits {
                for f in &o.fields {
                    type_calls(&f.ty, &mut calls, manifest);
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
    Ok(Dependencies {
        order,
        callees: edges,
    })
}
