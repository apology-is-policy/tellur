//! Constructor expressions and path-sensitive initialization. Calls are ordinary
//! IR decisions with one private completion; partially initialized fields remain
//! independent activation owners until successful assembly.
use super::*;
use crate::constructors::{COMPLETED, field_name, has_call, is_control};
impl Checker<'_> {
    pub(super) fn initialize_reference_field(
        &mut self,
        name: &str,
        ty: &str,
        value: &syntax::Expr,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        let stored = self.stored_value(value, ty, domain, span, true)?;
        let root = self.define(name, ty, domain, span)?;
        self.emit(guard.clone(), Action::Bind(root, stored.expression), span);
        for (fields, id) in stored.loans {
            let incoming = self.loan(id).clone();
            for scope in &mut self.scopes {
                if let Some(loan) = scope.loans.values_mut().find(|loan| loan.id == id) {
                    loan.active = FALSE;
                }
            }
            self.initialize_field_loan(
                name,
                Place {
                    root,
                    fields,
                    indirect: false,
                },
                incoming,
                true,
                &guard,
                domain,
                span,
            )?;
        }
        Ok(())
    }

    /// One runtime address per initialized reference leaf, independent of which
    /// path supplied it. Provenance conservatively joins across disjoint paths.
    /// Nested referent loans use the same rule; tagged payloads stay abstract
    /// until their delta establishes the active alternative.
    #[allow(clippy::too_many_arguments)]
    fn initialize_field_loan(
        &mut self,
        field: &str,
        holder: Place,
        incoming: Loan,
        materialize: bool,
        guard: &Guard,
        domain: Formula,
        span: Span,
    ) -> Result<Loan> {
        let previous = self.scopes[0]
            .loans
            .values()
            .find(|loan| {
                loan.holder.as_ref() == Some(&holder) && loan.sum_leaf == incoming.sum_leaf
            })
            .cloned();
        let key = previous
            .as_ref()
            .map(|loan| loan.target.name.clone())
            .unwrap_or_else(|| format!("{field}$reference_{}", self.fresh()));
        let ty = format!(
            "&{}{}",
            if incoming.mutable { "mutable " } else { "" },
            incoming.target.ty
        );
        let already = self
            .find_value(&key)
            .map_or(FALSE, |(_, value)| value.defined);
        let fresh = self.bdd.minus(domain, already);
        let address = if fresh != FALSE || self.find_value(&key).is_none() {
            self.define(&key, &ty, fresh, span)?
        } else {
            self.find_value(&key).unwrap().1.slot.id
        };
        let materialize = materialize && incoming.sum_leaf.is_none();
        if materialize && domain != FALSE {
            self.emit(
                guard.clone(),
                Action::Bind(address, Expr::Place(holder.clone())),
                span,
            );
        }
        let contents = {
            let mut contents = vec![];
            for (fields, child) in &incoming.contents {
                let child_domain = self.bdd.and(domain, child.defined);
                let child = self.initialize_field_loan(
                    field,
                    Place {
                        root: address,
                        fields: fields.clone(),
                        indirect: true,
                    },
                    child.clone(),
                    materialize,
                    guard,
                    child_domain,
                    span,
                )?;
                contents.push((fields.clone(), child));
            }
            contents
        };
        let mut loan = previous.unwrap_or_else(|| {
            let mut loan = incoming.clone();
            loan.id = self.fresh();
            loan.active = FALSE;
            loan.defined = FALSE;
            loan.origins.clear();
            loan.parents.clear();
            loan.contents.clear();
            loan
        });
        for origin in incoming.origins {
            if !loan.origins.contains(&origin) {
                loan.origins.push(origin);
            }
        }
        for parent in incoming.parents {
            if !loan.parents.contains(&parent) {
                loan.parents.push(parent);
            }
        }
        for (fields, child) in contents {
            if let Some((_, previous)) = loan
                .contents
                .iter_mut()
                .find(|(path, previous)| *path == fields && previous.id == child.id)
            {
                *previous = child;
            } else {
                loan.contents.push((fields, child));
            }
        }
        loan.active = self.bdd.or(loan.active, domain);
        loan.defined = self.bdd.or(loan.defined, domain);
        loan.holder = Some(holder);
        loan.target = ResolvedPlace {
            scope: 0,
            name: key,
            place: Place {
                root: address,
                fields: vec![],
                indirect: true,
            },
            ty: incoming.target.ty,
            via: None,
            carriers: vec![],
        };
        self.scopes[0]
            .loans
            .insert(format!("$stored_{}", loan.id), loan.clone());
        Ok(loan)
    }

    pub(super) fn initialize_constructor(
        &mut self,
        source: &syntax::Region,
    ) -> Result<Option<(String, Vec<manifest::Field>)>> {
        if !is_control(&source.name) {
            return Ok(None);
        }
        let ty = source.exits[0].fields[0].ty.clone();
        crate::origins::result_type(&ty, self.origin_schemas, self.manifest, source.span)?;
        let Some(manifest::TypeDef::Record { fields, .. }) = self.manifest.type_def(&ty) else {
            return Err(self.error(
                "E_CONSTRUCTOR",
                source.span,
                "constructor result must be a struct",
            ));
        };
        let fields = fields.clone();
        for field in &fields {
            let name = field_name(&field.name);
            self.define(&name, &field.ty, FALSE, source.span)?;
            self.future_names.insert(name);
        }
        Ok(Some((ty, fields)))
    }
    pub(super) fn finish_constructor(
        &mut self,
        ty: &str,
        fields: &[manifest::Field],
        span: Span,
    ) -> Result<()> {
        let live = self.bdd.not(self.dead);
        if live == FALSE {
            return Ok(());
        }
        self.expire_loans(&fields.iter().map(|f| field_name(&f.name)).collect());
        let value = syntax::ExprKind::NamedRecord(
            ty.into(),
            fields
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        syntax::ExprKind::Name(field_name(&f.name)).at(Default::default()),
                    )
                })
                .collect(),
        )
        .at(Default::default());
        self.statement(
            Guard::True,
            live,
            &Statement::Exit(COMPLETED.into(), vec![value]),
            span,
        )
    }
    pub(super) fn constructor_value(
        &mut self,
        call: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<(Expr, String)> {
        let local = self.constructor_local(call, domain, span)?;
        self.expr(&local, domain, true, span)
    }
    pub(super) fn constructor_local(
        &mut self,
        call: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<syntax::Expr> {
        let syntax::ExprKind::Call(name, args) = &call.kind else {
            unreachable!()
        };
        let sig = self
            .signature(name, args, span)?
            .ok_or_else(|| self.error("E_CONSTRUCTOR", span, "unknown constructor"))?;
        let ty = sig.outcomes[0].fields[0].ty.clone();
        let local = format!("$construction_{}", self.fresh());
        let arm = syntax::Arm {
            span,
            outcome: COMPLETED.into(),
            bindings: vec![syntax::Field {
                type_span: span,
                origins: vec![],
                referent_origins: vec![],
                name: local.clone(),
                ty: ty.clone(),
            }],
            atom: format!("{local}_ready"),
            termination: None,
            body: vec![],
        };
        let previous = std::mem::replace(&mut self.constructing, true);
        let result = self.decision(
            self.expression_guard.clone(),
            domain,
            &None,
            call,
            &[arm],
            span,
            None,
            None,
        );
        self.constructing = previous;
        result?;
        Ok(syntax::ExprKind::Name(local).at(call.range))
    }
    /// Own earlier values in slots before evaluating subsequent constructors.
    /// Do not consume these slots until the entire expression list is ready:
    /// a fault in a later argument must still settle every earlier owner.
    pub(super) fn stage_values(
        &mut self,
        args: &[syntax::Expr],
        expected: &[Option<String>],
        domain: Formula,
        span: Span,
    ) -> Result<Option<Vec<syntax::Expr>>> {
        let projections = args
            .iter()
            .map(|arg| self.projection_type(arg, span))
            .collect::<Result<Vec<_>>>()?;
        if !args.iter().any(has_call) && projections.iter().all(Option::is_none) {
            return Ok(None);
        }
        // Use the expected or inferred reference mode for pending payloads.
        // Each completed value replaces its initializer before the next check;
        // only the result's retained origins survive a completed nested call.
        let mut parameters = vec![];
        for ((arg, annotation), projection) in args.iter().zip(expected).zip(&projections) {
            let reference = annotation
                .as_deref()
                .or(projection.as_deref())
                .and_then(manifest::reference_inner)
                .map(|(mutable, _)| mutable)
                .or(self
                    .reference_source(arg, span)?
                    .map(|(_, _, loan, _)| loan.mutable));
            parameters.push(manifest::Parameter {
                name: "value".into(),
                ty: annotation.clone().unwrap_or_else(|| "auto".into()),
                mode: if reference == Some(false) {
                    Mode::Borrow
                } else {
                    Mode::Consume
                },
            });
        }
        let first_initializer_loan = *self.next;
        let mut result = vec![];
        for (index, (arg, annotation)) in args.iter().zip(expected).enumerate() {
            self.next_argument_aliases(&result, arg, &parameters, domain, span)?;
            let projected = self.materialize_projection(arg, domain, span)?;
            let arg = projected.as_ref().unwrap_or(arg);
            // Reference arguments retain their addresses. Prefix checks protect
            // them throughout evaluation of every subsequent initializer.
            if annotation
                .as_deref()
                .and_then(manifest::reference_inner)
                .is_some()
                || matches!(
                    arg.kind,
                    syntax::ExprKind::Borrow(_) | syntax::ExprKind::BorrowMutable(_)
                )
                || self.reference_source(arg, span)?.is_some()
            {
                result.push(arg.clone());
                self.expire_initializer_loans(first_initializer_loan, &result);
                continue;
            }
            let name = format!("$expression_{}", self.fresh());
            self.statement_inner(
                self.expression_guard.clone(),
                domain,
                &Statement::Bind(name.clone(), annotation.clone(), arg.clone(), false),
                arg.span(span),
            )?;
            let mut staged = syntax::ExprKind::Name(name).at(arg.range);
            staged.payload_name = Some(inferred_payload_name(arg, index));
            result.push(staged);
            self.expire_initializer_loans(first_initializer_loan, &result);
        }
        Ok(Some(result))
    }
    /// Only anonymous loans created while evaluating this list can expire here.
    /// Existing caller/outer-initializer loans retain their lifetimes. Pending
    /// values keep their contained references and carrier storage dependencies;
    /// ownership and fault dispositions are independent of this loan retirement.
    pub(super) fn expire_initializer_loans(&mut self, first: Id, pending: &[syntax::Expr]) {
        let mut live = BTreeSet::new();
        for value in pending {
            crate::liveness::expression(value, &mut live);
        }
        self.expire_loans_since(&live, first);
    }
    pub(super) fn freeze_value(
        &mut self,
        value: Expr,
        ty: &str,
        domain: Formula,
        span: Span,
    ) -> Result<Expr> {
        let name = format!("$expression_{}", self.fresh());
        let slot = self.define(&name, ty, domain, span)?;
        self.emit(
            self.expression_guard.clone(),
            Action::Bind(slot, value),
            span,
        );
        let (value, _) = self.expr(
            &syntax::ExprKind::Name(name).at(Default::default()),
            domain,
            true,
            span,
        )?;
        Ok(value)
    }
    pub(super) fn effectful_boolean(
        &mut self,
        op: &str,
        left: &syntax::Expr,
        right: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<(Expr, String)> {
        let prefix = format!("$boolean_{}", self.fresh());
        let arms: Vec<_> = ["true", "false"]
            .into_iter()
            .map(|tag| syntax::Arm {
                span,
                outcome: tag.into(),
                bindings: vec![],
                atom: format!("{prefix}_{tag}"),
                termination: None,
                body: vec![],
            })
            .collect();
        let outer = self.expression_guard.clone();
        let condition = format!("{prefix}_condition");
        self.statement_inner(
            outer.clone(),
            domain,
            &Statement::Bind(condition.clone(), Some("Bool".into()), left.clone(), false),
            span,
        )?;
        self.decision(
            outer.clone(),
            domain,
            &None,
            &syntax::ExprKind::Name(condition).at(left.range),
            &arms,
            span,
            None,
            None,
        )?;
        for (tag, evaluate) in [("true", op == "&&"), ("false", op == "||")] {
            let (guard, selected) = self.path(&format!("{prefix}_{tag}")).unwrap();
            let value = if evaluate {
                right.clone()
            } else {
                syntax::ExprKind::Bool(tag == "true").at(Default::default())
            };
            self.statement(
                outer.clone().and(guard),
                selected,
                &Statement::Bind(prefix.clone(), Some("Bool".into()), value, false),
                span,
            )?;
        }
        self.expr(
            &syntax::ExprKind::Name(prefix).at(Default::default()),
            domain,
            true,
            span,
        )
    }
}
