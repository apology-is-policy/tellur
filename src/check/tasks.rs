//! Local cold tasks retain owned and borrowed captures, including carrier contents.
//! Captures retain parameter identity so completion loans can select their origins.
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
        for (fields, loan_id) in loans {
            let loan = self
                .scopes
                .last_mut()
                .unwrap()
                .loans
                .values_mut()
                .find(|l| l.id == loan_id)
                .unwrap();
            // Hidden capture paths survive local moves but are never exposed as
            // source-projectable fields or interpreted as runtime task layout.
            loan.holder = Some(Place {
                root: id,
                fields,
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
    ) -> Result<(Vec<Expr>, Vec<(Vec<usize>, Id)>)> {
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
        for (parameter, (arg, p)) in args.iter().zip(parameters).enumerate() {
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
                let contents = self.snapshot_carrier_contents(&target, domain);
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
                values.push(Self::describe_reference(
                    Expr::Reference(target.place.clone(), p.ty.clone()),
                    &origins,
                ));
                let id = self.stash_stored_loan(
                    Loan {
                        contents,
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
                );
                loans.push((vec![parameter], id));
            } else if self.manifest.contains_references(&p.ty) {
                let stored = self.stored_value(arg, &p.ty, domain, span, true)?;
                values.push(stored.expression);
                loans.extend(stored.loans.into_iter().map(|(mut fields, id)| {
                    fields.insert(0, parameter);
                    (fields, id)
                }));
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
    /// Snapshot a captured input's external dependencies before consuming the
    /// task. The ordinary completion machinery projects and partitions these
    /// origins by result leaf and selected outcome, just as for a direct call.
    pub(super) fn task_origin_dependencies(
        &mut self,
        operand: &syntax::Expr,
        source: &BorrowSource,
        parameter: &manifest::Parameter,
        domain: Formula,
        span: Span,
    ) -> Result<Vec<(Vec<LoanOrigin>, Vec<Id>)>> {
        let mut value = operand.clone();
        value.call_mode = syntax::CallMode::Direct;
        let holder = self
            .resolve_place(&value, span)?
            .ok_or_else(|| self.error("E_ASYNC", span, "await requires an owned pending task"))?;
        let captures = self.snapshot_carrier_contents(&holder, domain);
        if let Some(contained) = &source.contained {
            // An owned argument has field/tag holders beneath its parameter
            // prefix. A borrowed carrier keeps that tree inside its direct loan,
            // alongside the authority that permits access to its contents.
            // Tagged leaves are selected abstractly, never read at runtime here.
            let mut contents = if manifest::reference_inner(&parameter.ty).is_some() {
                captures
                    .into_iter()
                    .filter(|(path, loan)| *path == [source.parameter] && loan.sum_leaf.is_none())
                    .flat_map(|(_, loan)| {
                        let mut parents = loan.parents;
                        if loan.mutable {
                            parents.push(loan.id);
                        }
                        loan.contents
                            .into_iter()
                            .map(move |(fields, child)| (fields, child, parents.clone()))
                    })
                    .collect::<Vec<_>>()
            } else {
                captures
                    .into_iter()
                    .filter_map(|(path, loan)| {
                        path.strip_prefix(&[source.parameter])
                            .map(|fields| (fields.to_vec(), loan, Vec::new()))
                    })
                    .collect::<Vec<_>>()
            };
            for step in &contained.through {
                contents = contents
                    .into_iter()
                    .filter(|(fields, loan, _)| {
                        *fields == step.fields && loan.sum_leaf == step.sum_leaf
                    })
                    .flat_map(|(_, loan, mut parents)| {
                        parents.extend(loan.parents.iter().copied());
                        if loan.mutable {
                            parents.push(loan.id);
                        }
                        loan.contents
                            .into_iter()
                            .map(move |(fields, child)| (fields, child, parents.clone()))
                    })
                    .collect();
            }
            return Ok(contents
                .into_iter()
                .filter(|(fields, loan, _)| {
                    *fields == contained.fields && loan.sum_leaf == contained.sum_leaf
                })
                .map(|(_, loan, mut parents)| {
                    parents.extend(loan.parents);
                    if loan.mutable {
                        parents.push(loan.id);
                    }
                    parents.sort_unstable();
                    parents.dedup();
                    (loan.origins, parents)
                })
                .collect());
        }
        let dependencies = captures
            .into_iter()
            .filter(|(path, _)| *path == [source.parameter])
            .map(|(_, loan)| {
                let mut parents = loan.parents;
                parents.push(loan.id);
                (loan.origins, parents)
            })
            .collect::<Vec<_>>();
        if dependencies.is_empty() {
            return Err(self.error("E_LIFETIME", span, "missing captured result origin"));
        }
        Ok(dependencies)
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
        // The sequential drive retains backing storage until completion. The
        // decision has already snapshotted result dependencies; retire captures
        // before attaching only the selected completion's result loans.
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
