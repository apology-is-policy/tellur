//! Constructor expressions and path-sensitive initialization. Calls are ordinary
//! IR decisions with one private completion; partially initialized fields remain
//! independent activation owners until successful assembly.
use super::*;
use crate::constructors::{COMPLETED, field_name, has_call, is_control};
impl Checker<'_> {
    pub(super) fn initialize_constructor(
        &mut self,
        source: &syntax::Region,
    ) -> Result<Option<(String, Vec<manifest::Field>)>> {
        if !is_control(&source.name) {
            return Ok(None);
        }
        let ty = source.exits[0].fields[0].ty.clone();
        if self.manifest.contains_references(&ty) {
            return Err(self.error("E_CONSTRUCTOR", source.span, "constructors currently produce owning records; use an ordinary control with an origin contract for reference-bearing results"));
        }
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
        self.expr(
            &syntax::ExprKind::Name(local).at(call.range),
            domain,
            true,
            span,
        )
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
        if !args.iter().any(has_call) {
            return Ok(None);
        }
        // Check the original list before staging erases nested call accesses.
        // In particular, an earlier returned borrow must conflict with a later
        // constructor's exclusive borrow of the same owner.
        let mut parameters = vec![];
        for (arg, annotation) in args.iter().zip(expected) {
            let reference = annotation
                .as_deref()
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
        self.argument_aliases(args, &parameters, domain, span)?;
        let mut result = vec![];
        for (index, (arg, annotation)) in args.iter().zip(expected).enumerate() {
            // Reference arguments retain their addresses. The full-list alias
            // check protects them through later argument evaluation.
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
        }
        Ok(Some(result))
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
