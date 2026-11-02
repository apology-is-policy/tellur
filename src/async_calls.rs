//! Source admission for the sequential async profile. Keep invocation
//! permissions before lowering: await/run/defer cannot become an ordinary call by
//! argument staging, constructor rewriting or generic specialization.
use crate::{Diagnostic, Result, Span, syntax::*};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Usage {
    Value,
    Operation,
    Initializer,
}
struct Admission<'a> {
    asynchronous: bool,
    controls: &'a BTreeSet<String>,
}
impl Admission<'_> {
    fn expression(&self, e: &Expr, usage: Usage, contingent: bool, at: Span) -> Result<()> {
        let at = e.span(at);
        let fail = |message| Diagnostic::new("E_ASYNC", at, message).with_range(e.range);
        if let ExprKind::Call(name, args) = &e.kind {
            let asynchronous = self.controls.contains(name);
            if matches!(e.call_mode, CallMode::Run | CallMode::Await) && usage != Usage::Operation {
                return Err(fail(
                    "run and await expose outcomes; use delta, capture, forward or a call statement",
                ));
            }
            match e.call_mode {
                CallMode::Direct if asynchronous => {
                    return Err(fail(if self.asynchronous {
                        "an async control must be invoked with await"
                    } else {
                        "an async control must be entered with run"
                    }));
                }
                CallMode::Run | CallMode::Await | CallMode::Defer if !asynchronous => {
                    return Err(fail("run, await and defer require an async control"));
                }
                CallMode::Run if self.asynchronous => {
                    return Err(fail(
                        "run belongs in synchronous controls; use await inside an async control",
                    ));
                }
                CallMode::Await | CallMode::Defer if !self.asynchronous => {
                    return Err(fail(
                        "await/defer require an async control; use run for synchronous entry",
                    ));
                }
                CallMode::Run | CallMode::Await | CallMode::Defer if contingent => {
                    return Err(fail(
                        "async invocation is not available in contingency code",
                    ));
                }
                CallMode::Defer if usage != Usage::Initializer => {
                    return Err(fail(
                        "defer currently requires a local auto binding initializer",
                    ));
                }
                _ => {}
            }
            for arg in args {
                self.expression(arg, Usage::Value, contingent, at)?;
            }
        } else {
            if e.call_mode == CallMode::Await && matches!(e.kind, ExprKind::Name(_)) {
                if usage != Usage::Operation || !self.asynchronous || contingent {
                    return Err(fail(
                        "await a pending task only in an async control's ordinary operation context",
                    ));
                }
                return Ok(());
            }
            if e.call_mode != CallMode::Direct {
                return Err(fail("run and await require a direct async control call"));
            }
            match &e.kind {
                ExprKind::List(args) | ExprKind::Construct(_, args) => {
                    for arg in args {
                        self.expression(arg, Usage::Value, contingent, at)?;
                    }
                }
                ExprKind::NamedRecord(_, fields) => {
                    for (_, arg) in fields {
                        self.expression(arg, Usage::Value, contingent, at)?;
                    }
                }
                ExprKind::Borrow(arg)
                | ExprKind::BorrowMutable(arg)
                | ExprKind::Move(arg)
                | ExprKind::Deref(arg)
                | ExprKind::Project(arg, _)
                | ExprKind::Unary(arg) => {
                    self.expression(arg, Usage::Value, contingent, at)?;
                }
                ExprKind::Binary(_, left, right) => {
                    self.expression(left, Usage::Value, contingent, at)?;
                    self.expression(right, Usage::Value, contingent, at)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn outcome(&self, e: &OutcomeExpr, contingent: bool) -> Result<()> {
        match e {
            OutcomeExpr::Delta(at, call, arms) => {
                self.expression(call, Usage::Operation, contingent, *at)?;
                for arm in arms {
                    self.outcome(&arm.result, contingent)?;
                }
            }
            OutcomeExpr::Construct(at, _, values) => {
                for value in values {
                    self.expression(value, Usage::Value, contingent, *at)?;
                }
            }
            OutcomeExpr::Forward(_) => {}
        }
        Ok(())
    }
    fn statement(&self, s: &Statement, contingent: bool, at: Span) -> Result<()> {
        match s {
            Statement::Invoke(e) | Statement::Forward(e) | Statement::Capture(_, _, e, _) => {
                self.expression(e, Usage::Operation, contingent, at)?;
            }
            Statement::Delta(_, e, arms) => {
                self.expression(e, Usage::Operation, contingent, at)?;
                for arm in arms {
                    self.items(&arm.body, contingent)?;
                    if let Some(termination) = &arm.termination {
                        self.statement(&termination.1, contingent, termination.0)?;
                    }
                }
            }
            Statement::Bind(_, _, e, _) => {
                self.expression(e, Usage::Initializer, contingent, at)?
            }
            Statement::Drop(e)
            | Statement::Emit(e)
            | Statement::Fault(e)
            | Statement::Supervise(e) => {
                self.expression(e, Usage::Value, contingent, at)?;
            }
            Statement::Assign(left, right) => {
                self.expression(left, Usage::Value, contingent, at)?;
                self.expression(right, Usage::Value, contingent, at)?;
            }
            Statement::Exit(_, values) | Statement::Again(_, values) => {
                for value in values {
                    self.expression(value, Usage::Value, contingent, at)?;
                }
            }
            Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) => {
                self.outcome(e, contingent)?;
            }
            Statement::Boundary { body, handler, .. } => {
                self.items(body, contingent)?;
                self.items(handler, true)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn items(&self, items: &[Item], contingent: bool) -> Result<()> {
        for item in items {
            match item {
                Item::Guard(_, statements) => {
                    for (at, statement) in statements {
                        self.statement(statement, contingent, *at)?;
                    }
                }
                Item::Group(_, _, body) | Item::Over(_, _, body) => self.items(body, contingent)?,
                Item::Loop(at, _, parameters, body, _) => {
                    for parameter in parameters {
                        self.expression(&parameter.initial, Usage::Value, contingent, *at)?;
                    }
                    self.items(body, contingent)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}
pub(crate) fn validate(region: &Region, controls: &BTreeSet<String>) -> Result<()> {
    if region.asynchronous && (region.contingency || region.contingent_contract == Some(true)) {
        return Err(Diagnostic::new(
            "E_ASYNC",
            region.name_span,
            "async controls cannot be contingent in the direct async profile",
        ));
    }
    Admission {
        asynchronous: region.asynchronous,
        controls,
    }
    .items(
        &region.body,
        region.contingency || region.contingent_contract == Some(true),
    )
}

/// Task captures are an implementation-private local type in this first slice.
/// Recursively inspect inferred result layouts so wrappers cannot erase that bound.
pub(crate) fn contains_task(manifest: &crate::manifest::Manifest, ty: &str) -> bool {
    fn visit(m: &crate::manifest::Manifest, ty: &str, seen: &mut BTreeSet<String>) -> bool {
        if !seen.insert(ty.into()) {
            return false;
        }
        if crate::manifest::task_inner(ty).is_some() {
            return true;
        }
        if let Some((_, inner)) = crate::manifest::reference_inner(ty) {
            return visit(m, inner, seen);
        }
        if let Some(inner) = crate::manifest::option_inner(ty) {
            return visit(m, inner, seen);
        }
        if let Some(c) = crate::collection_support::Collection::parse(ty) {
            return c.types().iter().any(|ty| visit(m, ty, seen));
        }
        match m.type_def(ty) {
            Some(crate::manifest::TypeDef::Record { fields, .. }) => {
                fields.iter().any(|f| visit(m, &f.ty, seen))
            }
            Some(
                crate::manifest::TypeDef::Variant { constructors, .. }
                | crate::manifest::TypeDef::Outcome { constructors, .. },
            ) => constructors
                .iter()
                .flat_map(|o| &o.fields)
                .any(|f| visit(m, &f.ty, seen)),
            _ => false,
        }
    }
    visit(manifest, ty, &mut BTreeSet::new())
}
