//! Local cold tasks retain owned captures and direct borrowed leaf captures.
//! Contained references and returned loans need a separate origin-transfer slice.
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
            let inner = manifest::reference_inner(&p.ty).map_or(p.ty.as_str(), |(_, t)| t);
            self.manifest.contains_references(inner)
        }) {
            return Err(operand.locate(self.error("E_ASYNC", span,
                "deferred captures support owned values and direct references to reference-free storage; contained reference captures are not implemented"), span));
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
        let (values, loans) = self.task_arguments(args, &sig.parameters, domain, span)?;
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
        if !loans.is_empty() && self.find_value(name).is_some() {
            return Err(self.error(
                "E_ASYNC",
                span,
                "borrowed task bindings require unique local names",
            ));
        }
        let id = self.define(name, &ty, domain, span)?;
        for loan_id in loans {
            let loan = self
                .scopes
                .last_mut()
                .unwrap()
                .loans
                .values_mut()
                .find(|l| l.id == loan_id)
                .unwrap();
            // These are hidden captures, not source-projectable record fields.
            // No result-origin transfer uses individual captures in this slice.
            loan.holder = Some(Place {
                root: id,
                fields: vec![],
                indirect: false,
            });
        }
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
    fn task_arguments(
        &mut self,
        args: &[syntax::Expr],
        parameters: &[manifest::Parameter],
        domain: Formula,
        span: Span,
    ) -> Result<(Vec<Expr>, Vec<Id>)> {
        if args.len() != parameters.len() {
            return Err(self.error("E_ARITY", span, "wrong deferred argument count"));
        }
        // Do not inherit a synchronous temporary's lifetime. Only references to
        // existing storage are admitted before constructor/projection staging.
        for (arg, p) in args.iter().zip(parameters) {
            if manifest::reference_inner(&p.ty).is_some() {
                let value = if let syntax::ExprKind::Move(value) = &arg.kind {
                    value.as_ref()
                } else {
                    arg
                };
                if crate::constructors::has_call(value) {
                    return Err(arg.locate(self.error("E_ASYNC", span,
                        "deferred borrows require existing storage; bind the owner or reference before defer"), span));
                }
            }
        }
        let staged = self.stage_values(
            args,
            &parameters
                .iter()
                .map(|p| Some(p.ty.clone()))
                .collect::<Vec<_>>(),
            domain,
            span,
        )?;
        let args = staged.as_deref().unwrap_or(args);
        self.argument_aliases(args, parameters, domain, span)?;
        let mut values = vec![];
        let mut loans = vec![];
        for (arg, p) in args.iter().zip(parameters) {
            if let Some((mutable, inner)) = manifest::reference_inner(&p.ty) {
                let (arg, transfer) = if let syntax::ExprKind::Move(value) = &arg.kind {
                    (value.as_ref(), true)
                } else {
                    (arg, false)
                };
                let source = self.reference_source(arg, span)?;
                let target = self.reference_argument(arg, mutable, domain, span)?;
                self.expect_type(&target.ty, inner, span)?;
                let origins = self.origins(&target);
                let parents = if transfer {
                    let source = source.as_ref().ok_or_else(|| {
                        self.error(
                            "E_MOVE",
                            span,
                            "move into a reference capture requires an existing reference",
                        )
                    })?;
                    self.transfer_reference(source, domain, span)?;
                    source.2.parents.clone()
                } else {
                    self.parents(&target)
                };
                values.push(Expr::Reference(target.place.clone(), p.ty.clone()));
                loans.push(self.stash_stored_loan(
                    Loan {
                        contents: vec![],
                        sum_leaf: None,
                        id: 0,
                        target,
                        origins,
                        parents,
                        mutable,
                        active: domain,
                        defined: domain,
                        holder: None,
                    },
                    domain,
                ));
            } else {
                // No contained references reach this branch. Existing argument
                // checking preserves owning transfer, constructor order and faults.
                values.extend(self.arguments(
                    std::slice::from_ref(arg),
                    std::slice::from_ref(p),
                    domain,
                    span,
                )?);
            }
        }
        Ok((values, loans))
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
        let ty = manifest::task_type(&name);
        let stored = self.stored_value(&value, &ty, domain, span, true)?;
        self.contingent_call(false, span)?;
        if sig.faults {
            self.fault_permission(domain, span)?;
            if sig.outcomes.is_empty() {
                self.terminate_fault(domain);
            }
        }
        // The synchronous drive retains backing storage until completion. No
        // borrowed result is admitted yet, so no captured loan survives delivery.
        for (_, id) in stored.loans {
            for scope in &mut self.scopes {
                if let Some(loan) = scope.loans.values_mut().find(|l| l.id == id) {
                    loan.active = self.bdd.minus(loan.active, domain);
                }
            }
        }
        Ok((name, stored.expression, sig.outcomes))
    }
}
