//! Struct construction procedures lower to checked owning controls. Their field
//! slots are unavailable until initialized; no partially formed record exists.
use crate::{Diagnostic, Result, Span, syntax::*};
use std::collections::BTreeMap;
const SUFFIX: &str = "$constructor";
pub(crate) const COMPLETED: &str = "$constructed";
pub(crate) fn control_name(ty: &str) -> String {
    if let Some((base, args)) = crate::collection_support::generic(ty) {
        format!("{base}{SUFFIX}<{}>", args.join(","))
    } else {
        format!("{ty}{SUFFIX}")
    }
}
pub(crate) fn is_control(name: &str) -> bool {
    crate::collection_support::generic(name)
        .map_or(name, |(base, _)| base)
        .ends_with(SUFFIX)
}
pub(crate) fn field_name(name: &str) -> String {
    format!("$field_{name}")
}
fn error(at: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_CONSTRUCTOR", at, message)
}
fn expressions(
    e: &mut Expr,
    at: Span,
    f: &mut impl FnMut(&mut Expr, Span) -> Result<()>,
) -> Result<()> {
    let at = e.span(at);
    f(e, at)?;
    match &mut e.kind {
        ExprKind::Call(_, args) | ExprKind::Construct(_, args) | ExprKind::List(args) => {
            for a in args {
                expressions(a, at, f)?;
            }
        }
        ExprKind::NamedRecord(_, fields) => {
            for (_, e) in fields {
                expressions(e, at, f)?;
            }
        }
        ExprKind::Borrow(e)
        | ExprKind::BorrowMutable(e)
        | ExprKind::Move(e)
        | ExprKind::Deref(e)
        | ExprKind::Project(e, _)
        | ExprKind::Unary(e) => expressions(e, at, f)?,
        ExprKind::Binary(_, a, b) => {
            expressions(a, at, f)?;
            expressions(b, at, f)?;
        }
        _ => (),
    }
    Ok(())
}
fn outcomes(e: &mut OutcomeExpr, f: &mut impl FnMut(&mut Expr, Span) -> Result<()>) -> Result<()> {
    match e {
        OutcomeExpr::Forward(_) => (),
        OutcomeExpr::Construct(at, _, args) => {
            for e in args {
                expressions(e, *at, f)?;
            }
        }
        OutcomeExpr::Delta(at, e, arms) => {
            expressions(e, *at, f)?;
            for arm in arms {
                outcomes(&mut arm.result, f)?;
            }
        }
    }
    Ok(())
}
fn statement(
    s: &mut Statement,
    at: Span,
    rewrite: &mut impl FnMut(&mut Statement, Span) -> Result<()>,
    f: &mut impl FnMut(&mut Expr, Span) -> Result<()>,
) -> Result<()> {
    rewrite(s, at)?;
    match s {
        Statement::Bind(_, _, e, _)
        | Statement::Capture(_, _, e, _)
        | Statement::Invoke(e)
        | Statement::Supervise(e)
        | Statement::Emit(e)
        | Statement::Drop(e)
        | Statement::Fault(e)
        | Statement::Forward(e) => expressions(e, at, f)?,
        Statement::Assign(a, b) => {
            expressions(a, at, f)?;
            expressions(b, at, f)?;
        }
        Statement::Exit(_, args) | Statement::Again(_, args) => {
            for e in args {
                expressions(e, at, f)?;
            }
        }
        Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) => outcomes(e, f)?,
        Statement::Boundary { body, handler, .. } => {
            items(body, rewrite, f)?;
            items(handler, rewrite, f)?;
        }
        Statement::Delta(_, e, arms) => {
            expressions(e, at, f)?;
            for arm in arms {
                items(&mut arm.body, rewrite, f)?;
                if let Some(t) = &mut arm.termination {
                    statement(&mut t.1, t.0, rewrite, f)?;
                }
            }
        }
        _ => (),
    }
    Ok(())
}
fn items(
    body: &mut [Item],
    rewrite: &mut impl FnMut(&mut Statement, Span) -> Result<()>,
    f: &mut impl FnMut(&mut Expr, Span) -> Result<()>,
) -> Result<()> {
    for item in body {
        match item {
            Item::Guard(_, ss) => {
                for (at, s) in ss {
                    statement(s, *at, rewrite, f)?;
                }
            }
            Item::Group(_, _, b) | Item::Over(_, _, b) => items(b, rewrite, f)?,
            Item::Loop(at, _, ps, b, _) => {
                for p in ps {
                    expressions(&mut p.initial, *at, f)?;
                }
                items(b, rewrite, f)?;
            }
            _ => (),
        }
    }
    Ok(())
}
fn self_field(e: &Expr) -> Option<Vec<String>> {
    match &e.kind {
        ExprKind::Name(name) if name == "self" => Some(vec![]),
        ExprKind::Name(name) if name.starts_with("self.") => {
            Some(name[5..].split('.').map(str::to_owned).collect())
        }
        ExprKind::Project(e, name) => self_field(e).map(|mut path| {
            path.push(name.clone());
            path
        }),
        _ => None,
    }
}
pub(crate) fn prepare(unit: &mut Unit) -> Result<()> {
    for data in &unit.data {
        let Some(constructor) = &data.constructor else {
            continue;
        };
        if !data.origin_parameters.is_empty() {
            return Err(error(
                constructor.span,
                "constructors for reference-bearing records need an origin contract; use an ordinary construction control for now",
            ));
        }
        let DataBody::Struct(fields) = &data.body else {
            return Err(error(constructor.span, "only structs declare constructors"));
        };
        let fields: BTreeMap<_, _> = fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();
        let mut control = constructor.clone();
        control.public &= data.public;
        if control.parameters.iter().any(|p| p.name == "self") {
            return Err(error(
                control.span,
                "self names the record under construction and cannot be a parameter",
            ));
        }
        let ty = if data.type_parameters.is_empty() {
            data.name.clone()
        } else {
            format!("{}<{}>", data.name, data.type_parameters.join(","))
        };
        control.exits = vec![Outcome {
            name: COMPLETED.into(),
            fields: vec![Field {
                type_span: data.name_span,
                origins: vec![],
                referent_origins: vec![],
                name: "value".into(),
                ty,
            }],
        }];
        let mut initialize = |s: &mut Statement, at| {
            match s {
                Statement::Exit(..)
                | Statement::ExitName(_)
                | Statement::ExitOutcome(_)
                | Statement::Forward(_) => {
                    return Err(error(
                        at,
                        "constructors complete implicitly after initializing every field; fallible results belong to ordinary controls",
                    ));
                }
                Statement::Boundary { .. }
                | Statement::Supervise(_)
                | Statement::Recover(_)
                | Statement::Terminate
                | Statement::TerminateProcess(_)
                | Statement::Escalate => {
                    return Err(error(
                        at,
                        "construction faults propagate to the caller; isolation and recovery belong outside the constructor",
                    ));
                }
                Statement::Assign(target, value) => {
                    if let Some(path) = self_field(target) {
                        if path.len() != 1 {
                            return Err(error(at, "initialize one complete self.field at a time"));
                        }
                        let ty = fields.get(&path[0]).ok_or_else(|| {
                            error(at, format!("unknown constructor field `{}`", path[0]))
                        })?;
                        *s = Statement::Bind(
                            field_name(&path[0]),
                            Some(ty.clone()),
                            value.clone(),
                            false,
                        );
                    }
                }
                _ => (),
            }
            Ok(())
        };
        let mut access = |e: &mut Expr, at| {
            if let Some(path) = self_field(e) {
                let Some(first) = path.first() else {
                    return Err(error(
                        at,
                        "a partial self value cannot be read, borrowed, moved or passed to another control",
                    ));
                };
                if !fields.contains_key(first) {
                    return Err(error(at, format!("unknown constructor field `{first}`")));
                }
                let mut value = ExprKind::Name(field_name(first)).at(e.range);
                for suffix in &path[1..] {
                    value = ExprKind::Project(Box::new(value), suffix.clone()).at(e.range);
                }
                *e = value;
            }
            Ok(())
        };
        items(&mut control.body, &mut initialize, &mut access)?;
        unit.regions.push(control);
    }
    Ok(())
}
pub(crate) fn rewrite_calls(
    unit: &mut Unit,
    visibility: &crate::modules::Visibility,
) -> Result<()> {
    let constructors: BTreeMap<_, _> = unit
        .data
        .iter()
        .filter_map(|d| d.constructor.as_ref().map(|c| (d.name.clone(), c.public)))
        .collect();
    for region in &mut unit.regions {
        let caller = region.name.clone();
        items(&mut region.body, &mut |_, _| Ok(()), &mut |e, at| {
            if let ExprKind::Call(name, _) = &mut e.kind {
                let base = crate::collection_support::generic(name)
                    .map_or(name.as_str(), |(base, _)| base);
                if let Some(public) = constructors.get(base) {
                    if !public && visibility.external(&caller, base) {
                        return Err(Diagnostic::new(
                            "E_PRIVATE",
                            at,
                            "this struct's constructor is private; use a public construction control",
                        ));
                    }
                    *name = control_name(name);
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Whether evaluating this expression may enter a source constructor.
pub(crate) fn has_call(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Call(n, args) => is_control(n) || args.iter().any(has_call),
        ExprKind::Construct(_, args) | ExprKind::List(args) => args.iter().any(has_call),
        ExprKind::NamedRecord(_, fs) => fs.iter().any(|(_, e)| has_call(e)),
        ExprKind::Borrow(e)
        | ExprKind::BorrowMutable(e)
        | ExprKind::Move(e)
        | ExprKind::Deref(e)
        | ExprKind::Project(e, _)
        | ExprKind::Unary(e) => has_call(e),
        ExprKind::Binary(_, a, b) => has_call(a) || has_call(b),
        _ => false,
    }
}
