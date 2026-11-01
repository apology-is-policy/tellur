//! Fresh reference-bearing records need addressable storage before their fields
//! can use the ordinary place and contained-loan rules. Materialize only when
//! evaluation reaches the projection, never during type or alias inspection.
use super::*;

impl Checker<'_> {
    fn projection_parts<'e>(
        &self,
        expression: &'e syntax::Expr,
        span: Span,
    ) -> Result<Option<(&'e syntax::Expr, Vec<&'e str>, String)>> {
        use syntax::ExprKind as S;
        let mut root = expression;
        let mut fields = vec![];
        while let S::Project(base, field) = &root.kind {
            fields.push(field.as_str());
            root = base;
        }
        if fields.is_empty() {
            return Ok(None);
        }
        let ty = match &root.kind {
            S::Call(name, args) if crate::constructors::is_control(name) => self
                .signature(name, args, span)?
                .map(|s| s.outcomes[0].fields[0].ty.clone()),
            S::Construct(name, _) | S::NamedRecord(name, _)
                if matches!(
                    self.manifest.type_def(name),
                    Some(manifest::TypeDef::Record { .. })
                ) =>
            {
                Some(name.clone())
            }
            _ => None,
        };
        let Some(ty) = ty.filter(|ty| self.manifest.contains_references(ty)) else {
            return Ok(None);
        };
        fields.reverse();
        Ok(Some((root, fields, ty)))
    }

    pub(super) fn projection_type(
        &self,
        expression: &syntax::Expr,
        span: Span,
    ) -> Result<Option<String>> {
        use syntax::ExprKind as S;
        match &expression.kind {
            S::Move(inner) => return self.projection_type(inner, span),
            S::Deref(inner) => {
                return self
                    .projection_type(inner, span)?
                    .map(|ty| {
                        manifest::reference_inner(&ty)
                            .map(|(_, inner)| inner.to_owned())
                            .ok_or_else(|| {
                                self.error(
                                    "E_BORROW",
                                    expression.span(span),
                                    "dereference requires a reference",
                                )
                            })
                    })
                    .transpose();
            }
            _ => {}
        }
        let Some((_, fields, mut ty)) = self.projection_parts(expression, span)? else {
            return Ok(None);
        };
        let mut owned = true;
        for field in fields {
            if let Some((_, inner)) = manifest::reference_inner(&ty) {
                ty = inner.into();
                owned = false;
            }
            let (index, selected) = self.manifest.record_field(&ty, field).ok_or_else(|| {
                self.error(
                    "E_FIELD",
                    expression.span(span),
                    format!("type `{ty}` has no record field `{field}`"),
                )
            })?;
            self.visibility
                .field(&self.result.name, &ty, field, expression.span(span))?;
            if owned
                && let Some(manifest::TypeDef::Record { fields, .. }) = self.manifest.type_def(&ty)
                && fields
                    .iter()
                    .enumerate()
                    .any(|(i, f)| i != index && self.manifest.linear(&f.ty))
            {
                return Err(self.error("E_TEMPORARY", expression.span(span), "bind this temporary before projecting: unselected fields contain resources or outcomes that still need completion"));
            }
            ty = selected.ty.clone();
        }
        Ok(Some(ty))
    }

    pub(super) fn materialize_projection(
        &mut self,
        expression: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<Option<syntax::Expr>> {
        use syntax::ExprKind as S;
        if self.projection_type(expression, span)?.is_none() {
            return Ok(None);
        }
        let kind = match &expression.kind {
            S::Move(inner) => S::Move(Box::new(
                self.materialize_projection(inner, domain, span)?.unwrap(),
            )),
            S::Deref(inner) => S::Deref(Box::new(
                self.materialize_projection(inner, domain, span)?.unwrap(),
            )),
            _ => {
                let (root, fields, ty) = self.projection_parts(expression, span)?.unwrap();
                let name = format!("$projection_{}", self.fresh());
                self.statement_inner(
                    self.expression_guard.clone(),
                    domain,
                    &Statement::Bind(name.clone(), Some(ty.into()), root.clone(), false),
                    root.span(span),
                )?;
                let mut local = S::Name(name).at(root.range);
                for field in fields {
                    local = S::Project(Box::new(local), field.into()).at(expression.range);
                }
                local.kind
            }
        };
        let mut local = kind.at(expression.range);
        local.payload_name = expression.payload_name.clone();
        Ok(Some(local))
    }
}
