//! Local cold tasks retain owned and borrowed captures, including carrier contents.
//! Captures retain parameter identity so completion loans can select their origins.
use super::*;

pub(super) fn handoffs(body: &[Instruction], signatures: &BTreeMap<String, Signature>) -> bool {
    body.iter().any(|instruction| match &instruction.action {
        Action::Decide { operation, .. } => match operation {
            Operation::Await(_)
            | Operation::AwaitTask(_)
            | Operation::Join(_)
            | Operation::Yield => true,
            Operation::Region(name) | Operation::Run(name) => {
                signatures.get(name).is_none_or(|s| s.handoffs)
            }
            _ => false,
        },
        Action::Supervise { boundary, .. } => signatures.get(boundary).is_none_or(|s| s.handoffs),
        Action::Loop { body, .. } => handoffs(body, signatures),
        Action::Boundary { body, handler, .. } => {
            handoffs(body, signatures) || handoffs(handler, signatures)
        }
        _ => false,
    })
}

/// Callees are checked before callers. Classify the emitted operations rather
/// than authored syntax so imports, specialization and constructor staging all
/// use the same proof. Unchecked/abstract signatures never grant permission.
pub(super) fn inline_boundary_free(
    region: &Region,
    signatures: &BTreeMap<String, Signature>,
) -> bool {
    fn code(body: &[Instruction], signatures: &BTreeMap<String, Signature>) -> bool {
        body.iter().all(|instruction| match &instruction.action {
            Action::Decide {
                operation: Operation::Region(name) | Operation::Run(name),
                ..
            } => signatures.get(name).is_some_and(|s| s.inline_boundary_free),
            // Awaited/spawned invocations have their own retained context. Their
            // boundaries do not cross the current invocation's child handles.
            Action::Decide { .. } => true,
            Action::Loop { body, .. } => code(body, signatures),
            Action::Boundary { .. }
            | Action::Supervise { .. }
            | Action::Recover { .. }
            | Action::Terminate
            | Action::TerminateProcess(_)
            | Action::Escalate
            | Action::Complete { .. } => false,
            _ => true,
        })
    }
    !region.contingency && region.boundary.is_none() && code(&region.code, signatures)
}

impl Checker<'_> {
    /// An enclosing owner's fault suppresses this invocation's contingencies.
    /// Count every live owner, including protected/retained boundary storage;
    /// ordinary fault permission's exemptions do not apply to being stopped.
    /// This records admission metadata, not an escaping `faults` contract.
    pub(super) fn record_stop_obligations(&mut self, domain: Formula) {
        for scope in &self.scopes {
            for binding in scope.values.values() {
                for (path, owned) in &binding.owned {
                    if self.bdd.and(domain, *owned) == FALSE {
                        continue;
                    }
                    let mut ty = binding.slot.ty.as_str();
                    for field in path {
                        ty = &self.manifest.field_at(ty, *field).unwrap().ty;
                    }
                    self.stoppable &= self.manifest.permits_fault(ty);
                }
            }
        }
    }

    fn started_owners(&self) -> Vec<(Formula, BTreeSet<String>)> {
        let mut result = vec![];
        for scope in &self.scopes {
            for binding in scope.values.values() {
                for (path, owned) in &binding.owned {
                    // Record ownership is field-sensitive; sums/collections stay
                    // sealed until their payload is selected and transferred.
                    let mut ty = binding.slot.ty.as_str();
                    for field in path {
                        ty = &self.manifest.field_at(ty, *field).unwrap().ty;
                    }
                    let producers = crate::async_calls::started_producers(self.manifest, ty);
                    if !producers.is_empty() {
                        result.push((*owned, producers));
                    }
                }
            }
        }
        result
    }
    pub(super) fn started_fault_permission(&mut self, domain: Formula, span: Span) -> Result<()> {
        for (owned, producers) in self.started_owners() {
            if self.bdd.and(domain, owned) != FALSE
                && producers.iter().any(|p| self.signatures[p].faults)
            {
                return self.fault_permission(domain, span);
            }
        }
        Ok(())
    }
    pub(super) fn require_no_started(&mut self, domain: Formula, span: Span) -> Result<()> {
        for (owned, _) in self.started_owners() {
            let live = self.bdd.and(domain, owned);
            if live != FALSE {
                return Err(self.fail_domain("E_ASYNC",span,
                    "await started children before a synchronous call that may enter a fault boundary; checked awaits and helpers without inline boundaries remain available",live));
            }
        }
        Ok(())
    }

    pub(super) fn handoff_beside_started(
        &mut self,
        stoppable: bool,
        kind: &str,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        if !stoppable {
            for (owned, _) in self.started_owners() {
                let live = self.bdd.and(domain, owned);
                if live != FALSE {
                    return Err(self.fail_domain("E_ASYNC", span,
                        &format!("{kind} beside started children requires fault dispositions for producer owners retained across handoffs; join the children first or complete those owners before handing off"), live));
                }
            }
        }
        self.started_fault_permission(domain, span)
    }

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
        let started = operand.call_mode == syntax::CallMode::Spawn;
        if started {
            if !sig.stoppable {
                return Err(self.error("E_ASYNC", span,
                    "spawn requires fault dispositions for every owner retained across a handoff in its producer or callees; complete such owners before awaiting or yielding"));
            }
            // Owned carriers preserve their leaf loans and guards until join;
            // selected result leaves retain their declared captured origins.
            if sig
                .parameters
                .iter()
                .any(|p| !self.manifest.supports_spawn_payload(&p.ty))
                || sig
                    .outcomes
                    .iter()
                    .flat_map(|o| &o.fields)
                    .any(|p| !self.manifest.supports_spawn_payload(&p.ty))
            {
                return Err(self.error(
                    "E_ASYNC",
                    span,
                    "spawn accepts reference-free values, direct borrows of reference-free storage, and owned record/option/variant/named-outcome captures/results with direct borrowed leaves; borrowed carriers, anonymous outcomes and reference-bearing collections remain gated",
                ));
            }
            if sig.faults {
                self.fault_permission(domain, span)?;
            }
        }
        let (values, loans) = self.task_arguments(args, &sig.parameters, domain, span)?;
        let ty = if started {
            manifest::started_type(operation)
        } else {
            manifest::task_type(operation)
        };
        if self.manifest.type_def(&ty).is_none() {
            self.manifest.types.push(manifest::TypeDef::Opaque {
                name: ty.clone(),
                linear: true,
                fault: None,
            });
            self.manifest.task_captures.insert(
                ty.clone(),
                sig.parameters
                    .iter()
                    .map(|p| p.ty.clone())
                    .chain(
                        sig.outcomes
                            .iter()
                            .filter(|_| started)
                            .flat_map(|o| &o.fields)
                            .map(|f| f.ty.clone()),
                    )
                    .collect(),
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
                operation: if started {
                    Operation::Spawn(operation.clone())
                } else {
                    Operation::Defer(operation.clone())
                },
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
                let expression = self.reference_expression(&target, p.ty.clone());
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
                values.push(expression);
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
    ) -> Result<(Operation, Expr, Vec<manifest::Outcome>)> {
        let call = self.pending_call(operand, span)?.unwrap();
        let syntax::ExprKind::Call(name, _) = call.kind else {
            unreachable!()
        };
        let sig = self.signatures[&name].clone();
        let mut value = operand.clone();
        value.call_mode = syntax::CallMode::Direct;
        let ty = self.stored_type(&value, span)?.unwrap();
        let started = manifest::is_started(&ty);
        self.record_stop_obligations(domain);
        self.stoppable &= sig.stoppable;
        if !started {
            self.handoff_beside_started(sig.stoppable, "await", domain, span)?;
        }
        self.started_fault_permission(domain, span)?;
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
        Ok((
            if started {
                Operation::Join(name)
            } else {
                Operation::AwaitTask(name)
            },
            stored.expression,
            sig.outcomes,
        ))
    }
}
