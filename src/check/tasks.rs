//! Local cold tasks with owned captures. References remain explicitly rejected
//! until the task owns persistent loan holders and public capture contracts exist.
use super::*;
impl Checker<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn defer_task(
        &mut self,
        name: &str,
        annotation: Option<&str>,
        operand: &syntax::Expr,
        mutable: bool,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        if annotation.is_some() || mutable {
            return Err(self.error(
                "E_ASYNC",
                span,
                "pending tasks currently use immutable local auto bindings",
            ));
        }
        let syntax::ExprKind::Call(operation, args) = &operand.kind else {
            unreachable!()
        };
        let sig = self
            .signature(operation, args, span)?
            .ok_or_else(|| self.error("E_ASYNC", span, "unknown deferred control"))?;
        if sig.parameters.iter().any(|p| {
            manifest::reference_inner(&p.ty).is_some() || self.manifest.contains_references(&p.ty)
        }) {
            return Err(operand.locate(self.error("E_ASYNC", span,
                "deferred captures currently require owned values without references; borrowed task captures are not implemented"), span));
        }
        if !sig.borrow_returns.is_empty() || !sig.stored_returns.is_empty() {
            return Err(self.error(
                "E_ASYNC",
                span,
                "deferred borrowed results require captured-loan propagation",
            ));
        }
        self.contingent_call(false, span)?;
        // Arguments are evaluated/transferred now. Only initializer faults apply;
        // callee body fault permission is checked later, when the task is awaited.
        let values = self.arguments(args, &sig.parameters, domain, span)?;
        let ty = manifest::task_type(operation);
        if self.manifest.type_def(&ty).is_none() {
            self.manifest.types.push(manifest::TypeDef::Opaque {
                name: ty.clone(),
                linear: true,
                fault: None,
            });
            self.manifest.task_captures.insert(
                ty.clone(),
                sig.parameters.iter().map(|p| p.ty.clone()).collect(),
            );
        }
        let id = self.define(name, &ty, domain, span)?;
        let atom = self.fresh();
        self.result.atoms.insert(atom, "$pending".into());
        self.emit(
            guard,
            Action::Decide {
                operation: Operation::Defer(operation.clone()),
                args: values,
                arms: vec![ir::Arm {
                    outcome: "$pending".into(),
                    atom,
                    slots: vec![id],
                }],
            },
            span,
        );
        Ok(())
    }
    /// Resolve the future completion contract without consuming the handle.
    pub(super) fn pending_call(
        &self,
        operand: &syntax::Expr,
        span: Span,
    ) -> Result<Option<syntax::Expr>> {
        if operand.call_mode != syntax::CallMode::Await
            || !matches!(operand.kind, syntax::ExprKind::Name(_))
        {
            return Ok(None);
        }
        let ty = self
            .stored_type(operand, span)?
            .ok_or_else(|| self.error("E_ASYNC", span, "await requires an owned pending task"))?;
        let name = manifest::task_inner(&ty)
            .ok_or_else(|| self.error("E_ASYNC", span, "await requires an owned pending task"))?;
        Ok(Some(
            syntax::ExprKind::Call(name.into(), vec![]).at(operand.range),
        ))
    }
    pub(super) fn await_task(
        &mut self,
        operand: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<(String, Expr, Vec<manifest::Outcome>)> {
        let call = self.pending_call(operand, span)?.unwrap();
        let syntax::ExprKind::Call(name, _) = call.kind else {
            unreachable!()
        };
        let sig = self.signatures[&name].clone();
        let mut value = operand.clone();
        value.call_mode = syntax::CallMode::Direct;
        let (value, _) = self.expr(&value, domain, true, span)?;
        self.contingent_call(false, span)?;
        if sig.faults {
            self.fault_permission(domain, span)?;
            if sig.outcomes.is_empty() {
                self.terminate_fault(domain);
            }
        }
        Ok((name, value, sig.outcomes))
    }
}
