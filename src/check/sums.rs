//! Local tagged storage keeps each payload loan guarded by its constructor.
use super::stored::StoredValue;
use super::*;
pub(super) type ReturnLoans = BTreeMap<(String, usize, Vec<usize>), (Vec<LoanOrigin>, Vec<Id>)>;
impl Checker<'_> {
    pub(super) fn construct_sum(
        &mut self,
        e: &syntax::Expr,
        ty: &str,
        domain: Formula,
        span: Span,
    ) -> Result<StoredValue> {
        e.checked(span, |span| {
            let (name, args) = match &e.kind {
                syntax::ExprKind::Name(n) => (n.as_str(), &[][..]),
                syntax::ExprKind::Construct(n, args) | syntax::ExprKind::Call(n, args) => {
                    (n.as_str(), args.as_slice())
                }
                _ => {
                    return Err(self.error(
                        "E_UNSUPPORTED",
                        span,
                        "borrowed variants must be constructed or transferred from a local binding",
                    ));
                }
            };
            let outcomes = self.manifest.outcomes(ty).unwrap();
            let outcome = outcomes
                .iter()
                .find(|o| {
                    name == o.name
                        || name == format!("{ty}.{}", o.name)
                        || (manifest::option_inner(ty).is_some()
                            && name.eq_ignore_ascii_case(&o.name))
                })
                .ok_or_else(|| {
                    self.error(
                        "E_CONSTRUCTOR",
                        span,
                        format!("unknown constructor `{name}` for `{ty}`"),
                    )
                })?
                .clone();
            if !self
                .visibility
                .constructor_visible(&self.result.name, ty, name)
            {
                return Err(self.error("E_VISIBILITY", span, "constructor is private"));
            }
            if args.len() != outcome.fields.len() {
                return Err(self.error("E_ARITY", span, "wrong variant payload arity"));
            }
            let mut values = vec![];
            let mut loans = vec![];
            for (payload, (arg, field)) in args.iter().zip(&outcome.fields).enumerate() {
                let value = self.stored_value(arg, &field.ty, domain, span, true)?;
                values.push(value.expression);
                for (fields, id) in value.loans {
                    self.set_sum_leaf(id, &outcome.name, payload, fields);
                    loans.push((vec![], id));
                }
            }
            Ok(StoredValue {
                expression: Expr::Construct(ty.into(), outcome.name, values),
                ty: ty.into(),
                loans,
            })
        })
    }
    fn set_sum_leaf(&mut self, id: Id, outcome: &str, payload: usize, fields: Vec<usize>) {
        let loan = self
            .scopes
            .iter_mut()
            .flat_map(|s| s.loans.values_mut())
            .find(|l| l.id == id)
            .unwrap();
        loan.sum_leaf = Some(SumLeaf {
            nested: loan.sum_leaf.take().map(Box::new),
            outcome: outcome.into(),
            payload,
            fields,
        });
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pack_payload(
        &mut self,
        arg: &syntax::Expr,
        ty: Option<&str>,
        outcome: &str,
        payload: usize,
        domain: Formula,
        span: Span,
    ) -> Result<(Expr, String, Vec<Loan>)> {
        arg.checked(span, |span| {
            if let syntax::ExprKind::Move(value) = &arg.kind {
                return self.pack_payload(value, ty, outcome, payload, domain, span);
            }
            let inferred_reference = if ty.is_none() {
                match &arg.kind {
                    syntax::ExprKind::Borrow(_) | syntax::ExprKind::BorrowMutable(_) => {
                        let mutable = matches!(arg.kind, syntax::ExprKind::BorrowMutable(_));
                        let target = self.reference_argument(arg, mutable, domain, span)?;
                        Some(format!(
                            "&{}{}",
                            if mutable { "mutable " } else { "" },
                            target.ty
                        ))
                    }
                    _ => self.reference_source(arg, span)?.map(|(_, _, l, _)| {
                        format!(
                            "&{}{}",
                            if l.mutable { "mutable " } else { "" },
                            l.target.ty
                        )
                    }),
                }
            } else {
                None
            };
            let ty = ty
                .map(str::to_owned)
                .or(self.stored_type(arg, span)?)
                .or(inferred_reference);
            let Some(ty) = ty.clone().filter(|t| self.manifest.contains_references(t)) else {
                let (e, t) = self.expr_expected(arg, domain, true, span, ty.as_deref())?;
                return Ok((e, t, vec![]));
            };
            let inner = manifest::reference_inner(&ty).map_or(ty.as_str(), |(_, inner)| inner);
            if crate::collection_support::Collection::parse(inner).is_some()
                && self.manifest.contains_references(inner)
            {
                return Err(self.error(
                    "E_UNSUPPORTED",
                    span,
                    "borrowed collections in stored outcomes require collection origin contracts",
                ));
            }
            self.manifest.check_local_storage(&ty)?;
            let value = self.stored_value(arg, &ty, domain, span, true)?;
            let mut loans = vec![];
            for (fields, id) in value.loans {
                self.set_sum_leaf(id, outcome, payload, fields);
                loans.push(self.loan(id).clone());
            }
            Ok((value.expression, ty, loans))
        })
    }
    pub(super) fn install_sum_loans(&mut self, root: Id, loans: Vec<Loan>) {
        for loan in loans {
            let active = loan.active;
            let id = self.stash_stored_loan(loan, active);
            let loan = self
                .scopes
                .last_mut()
                .unwrap()
                .loans
                .values_mut()
                .find(|l| l.id == id)
                .unwrap();
            loan.holder = Some(Place {
                root,
                fields: vec![],
                indirect: false,
            });
        }
    }
    pub(super) fn check_sum_scope(&self, loans: &[Loan], span: Span) -> Result<()> {
        let scope = self.scopes.last().unwrap();
        fn nested<'a>(loan: &'a Loan, out: &mut Vec<&'a Loan>) {
            out.push(loan);
            for (_, child) in &loan.contents {
                nested(child, out);
            }
        }
        let mut all = vec![];
        for loan in loans {
            nested(loan, &mut all);
        }
        for loan in all {
            for origin in &loan.origins {
                if scope
                    .values
                    .values()
                    .any(|b| b.slot.id == origin.place.root)
                {
                    return Err(self.error(
                        "E_LIFETIME",
                        span,
                        "stored outcome cannot retain storage owned by its completed arm",
                    ));
                }
            }
        }
        Ok(())
    }
}
impl Checker<'_> {
    pub(super) fn empty_sum_payloads(&self, ty: &str, returns: &mut ReturnLoans) {
        fn leaves(m: &Manifest, ty: &str, path: Vec<usize>, out: &mut Vec<Vec<usize>>) {
            if manifest::reference_inner(ty).is_some() {
                out.push(path);
            } else if let Some(manifest::TypeDef::Record { fields, .. }) = m.type_def(ty) {
                for (i, f) in fields.iter().enumerate() {
                    let mut p = path.clone();
                    p.push(i);
                    leaves(m, &f.ty, p, out);
                }
            }
        }
        for outcome in self.manifest.outcomes(ty).unwrap() {
            for (payload, field) in outcome.fields.iter().enumerate() {
                let mut paths = vec![];
                leaves(self.manifest, &field.ty, vec![], &mut paths);
                for path in paths {
                    returns
                        .entry((outcome.name.clone(), payload, path))
                        .or_default();
                }
            }
        }
    }
}

impl Checker<'_> {
    pub(super) fn borrowed_sum_target(
        &mut self,
        operand: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<Option<(ResolvedPlace, bool)>> {
        operand.checked(span, |span| {
            let mutable = match &operand.kind {
                syntax::ExprKind::Borrow(_) => Some(false),
                syntax::ExprKind::BorrowMutable(_) => Some(true),
                _ => self
                    .reference_source(operand, span)?
                    .map(|(_, _, l, _)| l.mutable),
            };
            let Some(mutable) = mutable else {
                return Ok(None);
            };
            let target = self.reference_argument(operand, mutable, domain, span)?;
            if self.manifest.outcomes(&target.ty).is_none() {
                return Err(self.error(
                    "E_DECISION",
                    span,
                    "borrowed decision requires a closed sum or bool",
                ));
            }
            Ok(Some((target, mutable)))
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn borrowed_sum_payloads(
        &mut self,
        target: &ResolvedPlace,
        mutable: bool,
        returns: &mut ReturnLoans,
        contents: &mut Vec<(SumLeaf, Loan)>,
        retained: &mut Vec<(SumLeaf, CarrierContents)>,
        domain: Formula,
        span: Span,
    ) -> Result<Vec<manifest::Outcome>> {
        let ty = format!("&{}{}", if mutable { "mutable " } else { "" }, target.ty);
        let outcomes = self.manifest.borrowed_outcomes(&ty).unwrap();
        let original = self.manifest.outcomes(&target.ty).unwrap();
        let loans: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter(|l| l.holder.as_ref() == Some(&target.place) && l.sum_leaf.is_some())
            .cloned()
            .collect();
        let family = self.fresh();
        for outcome in &original {
            for (index, field) in outcome.fields.iter().enumerate() {
                let key = (outcome.name.clone(), index, vec![]);
                if manifest::reference_inner(&field.ty).is_none() {
                    let mut origins = self.origins(target);
                    for origin in &mut origins {
                        // A whole alternative remains intact while payload storage is lent.
                        origin.opaque = true;
                        origin.permissions.push(Permission::new(family, index));
                    }
                    returns.insert(key, (origins, self.parents(target)));
                } else {
                    returns.entry(key).or_default();
                }
            }
        }
        for loan in loans {
            let needed = self.bdd.and(domain, loan.defined);
            if needed == FALSE {
                continue;
            }
            let mut access = loan.target.clone();
            access.via = Some(loan.id);
            access.carriers.extend(target.carriers.iter().copied());
            access.carriers.extend(target.via);
            self.check_loans(&access, needed, mutable && loan.mutable, span)?;
            let leaf_shape = loan.sum_leaf.as_ref().unwrap();
            let direct = manifest::reference_inner(
                &original
                    .iter()
                    .find(|o| o.name == leaf_shape.outcome)
                    .unwrap()
                    .fields[leaf_shape.payload]
                    .ty,
            )
            .is_some();
            let mut origins = if direct {
                self.origins(&access)
            } else {
                loan.origins.clone()
            };
            let mut parents = self.parents(&access);
            if loan.mutable && direct {
                // A reborrow of a stored exclusive permission cannot outlive
                // the storage that owns that permission, even through a shared carrier.
                for mut origin in self.origins(target) {
                    origin.guard = true;
                    origin.opaque = true;
                    if !origins.contains(&origin) {
                        origins.push(origin);
                    }
                }
                for id in self.parents(target) {
                    if !parents.contains(&id) {
                        parents.push(id);
                    }
                }
            }
            let mut child = loan.clone();
            let mut leaf = child.sum_leaf.take().unwrap();
            child.origins = origins.clone();
            child.parents = parents.clone();
            child.active = needed;
            child.sum_leaf = leaf.nested.take().map(|l| *l);
            let field = &original
                .iter()
                .find(|o| o.name == leaf.outcome)
                .unwrap()
                .fields[leaf.payload];
            if manifest::reference_inner(&field.ty).is_some() {
                if !loan.contents.is_empty() {
                    retained.push((leaf.clone(), loan.contents.clone()));
                }
                let entry = returns
                    .get_mut(&(leaf.outcome, leaf.payload, vec![]))
                    .unwrap();
                for origin in origins {
                    if !entry.0.contains(&origin) {
                        entry.0.push(origin);
                    }
                }
                for parent in parents {
                    if !entry.1.contains(&parent) {
                        entry.1.push(parent);
                    }
                }
            } else {
                contents.push((leaf, child));
            }
        }
        Ok(outcomes)
    }
}
