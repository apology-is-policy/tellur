//! Conservative local reference liveness. The checker retains outer-loop loans at
//! backedges; this traversal only shortens straight-line local lifetimes.
use crate::syntax::{Expr, Item, Statement};
use std::collections::BTreeSet;
pub fn expression(expr: &Expr, names: &mut BTreeSet<String>) {
    match expr {
        Expr::Name(n) => {
            names.insert(n.clone());
        }
        Expr::Call(_, args) | Expr::Construct(_, args) | Expr::List(args) => {
            for arg in args {
                expression(arg, names)
            }
        }
        Expr::NamedRecord(_, fields) => {
            for (_, value) in fields {
                expression(value, names);
            }
        }
        Expr::Borrow(e) | Expr::BorrowMutable(e) | Expr::Deref(e) | Expr::Unary(e) => {
            expression(e, names)
        }
        Expr::Project(e, field) => {
            if let Some(mut path) = path(e) {
                path.push('.');
                path.push_str(field);
                names.insert(path);
            } else {
                expression(e, names);
            }
        }
        Expr::Binary(_, a, b) => {
            expression(a, names);
            expression(b, names);
        }
        _ => {}
    }
}
pub fn outcome(expr: &crate::syntax::OutcomeExpr, names: &mut BTreeSet<String>) {
    match expr {
        crate::syntax::OutcomeExpr::Forward(_) => {}
        crate::syntax::OutcomeExpr::Construct(_, _, args) => {
            for arg in args {
                expression(arg, names)
            }
        }
        crate::syntax::OutcomeExpr::Delta(_, operand, arms) => {
            expression(operand, names);
            for arm in arms {
                outcome(&arm.result, names);
            }
        }
    }
}
pub fn statement(stmt: &Statement, names: &mut BTreeSet<String>) {
    match stmt {
        Statement::ExitName(name) => {
            names.insert(name.clone());
        }
        Statement::Capture(_, _, e, _) | Statement::Forward(e) | Statement::Invoke(e) => {
            expression(e, names)
        }
        Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) => outcome(e, names),
        Statement::Bind(_, _, e, _)
        | Statement::Drop(e)
        | Statement::Emit(e)
        | Statement::Delta(_, e, _) => expression(e, names),
        Statement::Assign(a, b) => {
            expression(a, names);
            expression(b, names);
        }
        Statement::Exit(_, args) | Statement::Again(_, args) => {
            for arg in args {
                expression(arg, names)
            }
        }
    }
}
pub fn items(body: &[Item], names: &mut BTreeSet<String>) {
    for item in body {
        match item {
            Item::Guard(_, statements) => {
                for (_, s) in statements {
                    statement(s, names)
                }
            }
            Item::Over(_, _, body) => items(body, names),
            Item::Loop(_, _, params, body) => {
                for p in params {
                    expression(&p.initial, names);
                }
                items(body, names);
            }
            Item::Alias(..) => {}
        }
    }
}

fn path(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(n) => Some(n.clone()),
        Expr::Project(e, f) => Some(format!("{}.{f}", path(e)?)),
        // Explicit dereference may cross a reference into another owner. Keep
        // the base alive conservatively rather than inventing a storage path.
        _ => None,
    }
}
pub(crate) fn overlaps(a: &str, b: &str) -> bool {
    a == b
        || a.strip_prefix(b)
            .is_some_and(|s| s.starts_with('.') || s.starts_with('['))
        || b.strip_prefix(a)
            .is_some_and(|s| s.starts_with('.') || s.starts_with('['))
}
