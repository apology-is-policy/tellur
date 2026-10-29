//! Local record storage keeps loans attached to their reference fields. The
//! pointee access remains independent of that storage, so moving a view cannot
//! move its owner. Call/return boundaries are deliberately checked separately.
use super::*;

type ReferenceSource = (usize, String, Loan, Option<ResolvedPlace>);
pub(super) struct StoredValue {
    pub(super) expression: Expr,
    pub(super) ty: String,
    pub(super) loans: Vec<(Vec<usize>, Id)>,
}

impl Checker<'_> {
    pub(super) fn snapshot_carrier_contents(
        &mut self,
        target: &ResolvedPlace,
        domain: Formula,
    ) -> Vec<(Vec<usize>, Loan)> {
        let candidates: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter_map(|loan| {
                loan.holder
                    .as_ref()
                    .filter(|h| {
                        h.root == target.place.root
                            && h.indirect == target.place.indirect
                            && h.fields.starts_with(&target.place.fields)
                    })
                    .map(|h| (h.fields[target.place.fields.len()..].to_vec(), loan.clone()))
            })
            .collect();
        candidates
            .into_iter()
            .filter_map(|(fields, mut loan)| {
                loan.active = self.bdd.and(loan.active, domain);
                loan.defined = self.bdd.and(loan.defined, domain);
                (loan.defined != FALSE).then_some((fields, loan))
            })
            .collect()
    }

    /// Retained dependencies participate in conflict checks while their reference
    /// is packed, without reading an inactive variant or its runtime address.
    pub(super) fn retained_loans(&mut self) -> Vec<Loan> {
        fn append(loan: &Loan, active: Formula, bdd: &mut Bdd, out: &mut Vec<Loan>) {
            let active = bdd.and(active, loan.active);
            for (_, child) in &loan.contents {
                append(child, active, bdd, out);
            }
            let mut leaf = loan.clone();
            leaf.active = active;
            leaf.contents.clear();
            out.push(leaf);
        }
        let mut out = vec![];
        for loan in self.scopes.iter().flat_map(|s| s.loans.values()) {
            append(loan, TRUE, &mut self.bdd, &mut out);
        }
        out
    }

    /// Rebase saved referent dependencies to the reference slot received by delta.
    /// Tagged leaves stay abstract until their own alternative is selected.
    pub(super) fn restore_carrier_contents(
        &mut self,
        root: Id,
        ty: &str,
        contents: &[(Vec<usize>, Loan)],
        domain: Formula,
        span: Span,
    ) -> Result<Vec<(Id, Expr)>> {
        fn leaves(
            m: &Manifest,
            ty: &str,
            fields: Vec<usize>,
            out: &mut Vec<(Vec<usize>, Option<SumLeaf>)>,
        ) {
            if manifest::reference_inner(ty).is_some() {
                out.push((fields, None));
            } else if let Some(manifest::TypeDef::Record { fields: fs, .. }) = m.type_def(ty) {
                for (i, f) in fs.iter().enumerate() {
                    let mut path = fields.clone();
                    path.push(i);
                    leaves(m, &f.ty, path, out);
                }
            } else if let Some(outcomes) = m.outcomes(ty) {
                for outcome in outcomes {
                    for (payload, field) in outcome.fields.iter().enumerate() {
                        let mut nested = vec![];
                        leaves(m, &field.ty, vec![], &mut nested);
                        for (inner, next) in nested {
                            out.push((
                                fields.clone(),
                                Some(SumLeaf {
                                    outcome: outcome.name.clone(),
                                    payload,
                                    fields: inner,
                                    nested: next.map(Box::new),
                                }),
                            ));
                        }
                    }
                }
            }
        }
        let mut paths = vec![];
        leaves(self.manifest, ty, vec![], &mut paths);
        let mut bindings = vec![];
        for (fields, tag) in paths {
            let mut origins = vec![];
            let mut parents = vec![];
            let mut nested = vec![];
            for (_, loan) in contents
                .iter()
                .filter(|(f, l)| *f == fields && l.sum_leaf == tag)
            {
                for origin in &loan.origins {
                    if !origins.contains(origin) {
                        origins.push(origin.clone());
                    }
                }
                for id in loan.parents.iter().copied().chain([loan.id]) {
                    if !parents.contains(&id) {
                        parents.push(id);
                    }
                }
                nested.extend(loan.contents.clone());
            }
            // A new tag test establishes the payload on its own domain, not the
            // producer's path atoms. Missing alternatives still have a checked
            // reference shape, with no invented external origin.
            let address = if let Some(tag) = &tag {
                self.receive_tagged(root, ty, &fields, tag, origins, parents, domain, span)?
            } else {
                let (address, value) = self.receive_stored(
                    "$retained",
                    root,
                    ty,
                    &fields,
                    origins,
                    parents,
                    domain,
                    span,
                )?;
                bindings.push((address, value));
                address
            };
            let restored = self
                .scopes
                .last_mut()
                .unwrap()
                .loans
                .values_mut()
                .find(|l| l.target.place.root == address)
                .unwrap();
            restored.contents = nested.clone();
            if tag.is_none() {
                let inner = restored.target.ty.clone();
                bindings
                    .extend(self.restore_carrier_contents(address, &inner, &nested, domain, span)?);
            }
        }
        self.attach_received_contents(root);
        Ok(bindings)
    }

    pub(super) fn stored_loan(&self, place: &ResolvedPlace) -> Option<(usize, String, Loan)> {
        self.scopes.iter().enumerate().find_map(|(scope, s)| {
            s.loans.iter().find_map(|(name, loan)| {
                (loan.holder.as_ref() == Some(&place.place))
                    .then(|| (scope, name.clone(), loan.clone()))
            })
        })
    }

    pub(super) fn reference_source(
        &self,
        expression: &syntax::Expr,
        span: Span,
    ) -> Result<Option<ReferenceSource>> {
        expression.checked(span, |span| {
            if let syntax::ExprKind::Name(name) = &expression.kind
                && let Some((scope, loan)) = self.find_loan(name)
            {
                return Ok(Some((scope, name.clone(), loan.clone(), None)));
            }
            if let Some(place) = self.resolve_place(expression, span)?
                && manifest::reference_inner(&place.ty).is_some()
                && let Some((scope, name, loan)) = self.stored_loan(&place)
            {
                return Ok(Some((scope, name, loan, Some(place))));
            }
            Ok(None)
        })
    }

    pub(super) fn transfer_reference(
        &mut self,
        source: &ReferenceSource,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        let (scope, name, loan, holder) = source;
        if !loan.mutable {
            return Ok(());
        }
        if self.loop_context.as_ref().is_some_and(|l| *scope < l.scope) {
            return Err(self.error(
                "E_LOOP_CAPTURE",
                span,
                "cannot transfer an outer reference inside a repeating loop",
            ));
        }
        if let Some(holder) = holder {
            self.use_place(holder.clone(), domain, true, span)?;
        }
        self.scopes[*scope].loans.get_mut(name).unwrap().active =
            self.bdd.minus(loan.active, domain);
        Ok(())
    }

    pub(super) fn stash_stored_loan(&mut self, mut loan: Loan, domain: Formula) -> Id {
        loan.id = self.fresh();
        loan.active = domain;
        loan.defined = domain;
        loan.holder = None;
        loan.target.via = None;
        loan.target.carriers.clear();
        let id = loan.id;
        self.scopes
            .last_mut()
            .unwrap()
            .loans
            .insert(format!("$stored_{id}"), loan);
        id
    }

    pub(super) fn stored_type(&self, e: &syntax::Expr, span: Span) -> Result<Option<String>> {
        e.checked(span, |span| {
            if let Some(p) = self.resolve_place(e, span)? {
                return Ok(Some(p.ty));
            }
            if let syntax::ExprKind::Name(n)
            | syntax::ExprKind::Call(n, _)
            | syntax::ExprKind::Construct(n, _) = &e.kind
                && let Ok((ty, _, _)) = self.manifest.constructor_visible(n, |ty| {
                    self.visibility
                        .constructor_visible(&self.result.name, ty, n)
                })
            {
                return Ok(Some(ty));
            }
            if let syntax::ExprKind::Call(n, args) | syntax::ExprKind::Construct(n, args) = &e.kind
                && let [arg] = args.as_slice()
                && self.manifest.option_constructor_visible(n, |ty| {
                    self.visibility
                        .constructor_visible(&self.result.name, ty, n)
                }) == Some("Some")
            {
                let ty = match &arg.kind {
                    syntax::ExprKind::Borrow(inner) | syntax::ExprKind::BorrowMutable(inner) => {
                        self.resolve_place(inner, span)?.map(|p| {
                            format!(
                                "&{}{}",
                                if matches!(arg.kind, syntax::ExprKind::BorrowMutable(_)) {
                                    "mutable "
                                } else {
                                    ""
                                },
                                p.ty
                            )
                        })
                    }
                    _ => self
                        .reference_source(arg, span)?
                        .map(|(_, _, l, _)| {
                            format!(
                                "&{}{}",
                                if l.mutable { "mutable " } else { "" },
                                l.target.ty
                            )
                        })
                        .or(self.stored_type(arg, span)?),
                };
                if let Some(ty) = ty.filter(|t| self.manifest.contains_references(t)) {
                    return Ok(Some(format!("Option<{ty}>")));
                }
            }
            Ok(match &e.kind {
                syntax::ExprKind::NamedRecord(n, _)
                | syntax::ExprKind::Construct(n, _)
                | syntax::ExprKind::Call(n, _)
                    if matches!(
                        self.manifest.type_def(n),
                        Some(manifest::TypeDef::Record { .. })
                    ) =>
                {
                    Some(n.clone())
                }
                syntax::ExprKind::Call(n, args) if n == "clone" => match args.as_slice() {
                    [
                        syntax::Expr {
                            kind: syntax::ExprKind::Borrow(e),
                            ..
                        },
                    ] => self.stored_type(e, span)?,
                    _ => None,
                },
                _ => None,
            })
        })
    }

    pub(super) fn stored_value(
        &mut self,
        e: &syntax::Expr,
        expected: &str,
        domain: Formula,
        span: Span,
        moving: bool,
    ) -> Result<StoredValue> {
        e.checked(span, |span| {
        if let Some((mutable, inner)) = manifest::reference_inner(expected) {
            let source = self.reference_source(e, span)?;
            if let Some((_, _, loan, _)) = &source {
                let actual = format!(
                    "&{}{}",
                    if loan.mutable { "mutable " } else { "" },
                    loan.target.ty
                );
                self.expect_type(&actual, expected, span)?;
            }
            let target = self.reference_argument(e, mutable, domain, span)?;
            self.expect_type(&target.ty, inner, span)?;
            let expression = Expr::Reference(target.place.clone(), expected.into());
            let origins = self.origins(&target);
            let parents = if let Some(source) = &source {
                self.transfer_reference(source, domain, span)?;
                source.2.parents.clone()
            } else {
                self.parents(&target)
            };
            let contents = self.snapshot_carrier_contents(&target, domain);
            let loan = Loan {
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
            };
            let id = self.stash_stored_loan(loan, domain);
            return Ok(StoredValue {
                expression,
                ty: expected.into(),
                loans: vec![(vec![], id)],
            });
        }
        if !self.manifest.contains_references(expected) {
            let (expression, ty) = self.expr_expected(e, domain, moving, span, Some(expected))?;
            self.expect_type(&ty, expected, span)?;
            return Ok(StoredValue {
                expression,
                ty,
                loans: vec![],
            });
        }
        self.manifest.check_local_storage(expected)?;
        if let syntax::ExprKind::Call(n, args) = &e.kind
            && n == "clone"
            && let [syntax::Expr { kind: syntax::ExprKind::Borrow(value), .. }] = args.as_slice()
        {
            if !self.manifest.is_cloneable(expected) {
                return Err(self.error(
                    "E_CLONE",
                    span,
                    "records containing exclusive references cannot be cloned",
                ));
            }
            return self.stored_value(value, expected, domain, span, false);
        }
        if let Some(place) = self.resolve_place(e, span)? {
            self.expect_type(&place.ty, expected, span)?;
            let borrowed_place = &place;
            let loans = self
                .scopes
                .iter()
                .enumerate()
                .flat_map(|(scope, s)| {
                    s.loans.iter().filter_map(move |(name, loan)| {
                        let place = borrowed_place;
                        loan.holder
                            .as_ref()
                            .filter(|h| {
                                h.root == place.place.root
                                    && h.indirect == place.place.indirect
                                    && h.fields.starts_with(&place.place.fields)
                            })
                            .map(|h| {
                                (
                                    scope,
                                    name.clone(),
                                    loan.clone(),
                                    h.fields[place.place.fields.len()..].to_vec(),
                                )
                            })
                    })
                })
                .collect::<Vec<_>>();
            let (expression, ty) = self.use_place(place, domain, moving, span)?;
            let transfer = moving && self.moves(&ty);
            let mut contents = vec![];
            for (scope, name, loan, path) in loans {
                let needed = self.bdd.and(domain, loan.defined);
                let unavailable = self.bdd.minus(needed, loan.active);
                if unavailable != FALSE {
                    return Err(self.fail_domain(
                        "E_MOVED",
                        span,
                        "record contains a moved or released reference",
                        unavailable,
                    ));
                }
                let mut access = loan.target.clone();
                access.via = Some(loan.id);
                self.check_loans(&access, needed, loan.mutable, span)?;
                if transfer {
                    self.scopes[scope].loans.get_mut(&name).unwrap().active =
                        self.bdd.minus(loan.active, domain);
                }
                contents.push((path, self.stash_stored_loan(loan, needed)));
            }
            return Ok(StoredValue {
                expression,
                ty,
                loans: contents,
            });
        }
        if let Some(c) = crate::collection_support::Collection::parse(expected) {
            let args = match &e.kind {
                syntax::ExprKind::List(args)
                    if !matches!(c, crate::collection_support::Collection::Map(..)) =>
                {
                    args
                }
                syntax::ExprKind::Construct(name, args)
                    if name == "map"
                        && args.is_empty()
                        && matches!(c, crate::collection_support::Collection::Map(..)) =>
                {
                    args
                }
                _ => {
                    return Err(self.error(
                        "E_COLLECTION",
                        span,
                        "borrowed collections use a typed list or map {} initializer",
                    ));
                }
            };
            c.fields(args.len()).ok_or_else(|| {
                self.error(
                    "E_ARITY",
                    span,
                    "array literal length does not match its type",
                )
            })?;
            let mut values = vec![];
            let mut loans = vec![];
            for arg in args {
                let value = self.stored_value(arg, c.element(), domain, span, true)?;
                values.push(value.expression);
                loans.extend(
                    value
                        .loans
                        .into_iter()
                        .map(|(_, id)| (vec![crate::collections::ELEMENT], id)),
                );
            }
            return Ok(StoredValue {
                expression: Expr::Construct(expected.into(), c.name().into(), values),
                ty: expected.into(),
                loans,
            });
        }
        if self.manifest.outcomes(expected).is_some() {
            return self.construct_sum(e, expected, domain, span);
        }
        let Some(manifest::TypeDef::Record { fields, .. }) =
            self.manifest.type_def(expected).cloned()
        else {
            return Err(self.error(
                "E_UNSUPPORTED",
                span,
                "stored references currently require a local struct",
            ));
        };
        let (name, initializers): (&str, Vec<(usize, &syntax::Expr)>) = match &e.kind {
            syntax::ExprKind::NamedRecord(name, initializers) => {
                let mut seen = BTreeSet::new();
                let mut ordered = vec![];
                for (field, value) in initializers {
                    let index = fields.iter().position(|f| f.name == *field).ok_or_else(|| self.error("E_FIELD", span, format!("unknown field `{field}`")))?;
                    if !seen.insert(index) { return Err(self.error("E_FIELD", span, format!("duplicate field `{field}`"))); }
                    ordered.push((index, value));
                }
                (name, ordered)
            }
            syntax::ExprKind::Construct(name, args) | syntax::ExprKind::Call(name, args) => (name, args.iter().enumerate().collect()),
            _ => return Err(self.error("E_UNSUPPORTED", span, "reference-bearing records must be constructed, copied, cloned or moved from a local place")),
        };
        self.expect_type(name, expected, span)?;
        self.visibility.construct(&self.result.name, name, span)?;
        if initializers.len() != fields.len() {
            return Err(self.error(
                "E_ARITY",
                span,
                "record initializer must supply every field exactly once",
            ));
        }
        let mut values = vec![];
        let mut loans = vec![];
        for (index, value) in initializers {
            let stored = self.stored_value(value, &fields[index].ty, domain, span, true)?;
            values.push((index, stored.expression));
            loans.extend(stored.loans.into_iter().map(|(mut path, id)| {
                path.insert(0, index);
                (path, id)
            }));
        }
        Ok(StoredValue {
            expression: Expr::Record(name.into(), values),
            ty: expected.into(),
            loans,
        })
        })
    }

    pub(super) fn bind_stored(
        &mut self,
        name: &str,
        annotation: Option<&str>,
        e: &syntax::Expr,
        mutable: bool,
        selection: (Guard, Formula),
        span: Span,
    ) -> Result<bool> {
        let (guard, domain) = selection;
        let ty = annotation.map(str::to_owned).or(self.stored_type(e, span)?);
        let Some(ty) = ty.filter(|t| {
            manifest::reference_inner(t).is_none() && self.manifest.contains_references(t)
        }) else {
            return Ok(false);
        };
        if self.find_value(name).is_some() || self.find_loan(name).is_some() {
            return Err(self.error("E_UNSUPPORTED", span, "reference-bearing record bindings must have unique names; merging storage across paths needs a contained-origin contract"));
        }
        let stored = self.stored_value(e, &ty, domain, span, true)?;
        let root = self.define(name, &stored.ty, domain, span)?;
        self.scopes
            .last_mut()
            .unwrap()
            .values
            .get_mut(name)
            .unwrap()
            .mutable = mutable;
        for (fields, id) in stored.loans {
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
                fields,
                indirect: false,
            });
        }
        self.emit(guard, Action::Bind(root, stored.expression), span);
        Ok(true)
    }

    pub(super) fn drop_stored(
        &mut self,
        e: &syntax::Expr,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<bool> {
        let Some(place) = self.resolve_place(e, span)? else {
            return Ok(false);
        };
        if !self.manifest.contains_references(&place.ty) {
            return Ok(false);
        }
        if self.manifest.linear(&place.ty) {
            return Err(self.error(
                "E_RESOURCE",
                span,
                "drop cannot discard a must-complete record",
            ));
        }
        if self
            .loop_context
            .as_ref()
            .is_some_and(|l| place.scope < l.scope)
        {
            return Err(self.error(
                "E_LOOP_CAPTURE",
                span,
                "cannot release an outer reference-bearing record inside a repeating loop",
            ));
        }
        // Explicit release invalidates this holder even when its fields are Copy.
        // References to the holder's storage must therefore have ended first.
        self.check_loans(&place, domain, true, span)?;
        let (value, _) = self.use_place(place.clone(), domain, true, span)?;
        let ids = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter(|loan| {
                loan.holder.as_ref().is_some_and(|h| {
                    place.place.overlaps(h) && h.fields.starts_with(&place.place.fields)
                })
            })
            .map(|l| l.id)
            .collect::<Vec<_>>();
        for id in ids {
            let loan = self.loan(id).clone();
            let needed = self.bdd.and(domain, loan.defined);
            let missing = self.bdd.minus(needed, loan.active);
            if missing != FALSE {
                return Err(self.fail_domain(
                    "E_MOVED",
                    span,
                    "stored reference was moved or released",
                    missing,
                ));
            }
            if loan.mutable {
                let mut target = loan.target.clone();
                target.via = Some(id);
                self.check_loans(&target, needed, true, span)?;
            }
            for s in &mut self.scopes {
                if let Some(loan) = s.loans.values_mut().find(|l| l.id == id) {
                    loan.active = self.bdd.minus(loan.active, domain);
                }
            }
        }
        self.emit(guard, Action::Drop(value), span);
        Ok(true)
    }
}

impl Checker<'_> {
    pub(super) fn exit_stored(
        &mut self,
        name: &str,
        payload: usize,
        arg: &syntax::Expr,
        domain: Formula,
        span: Span,
        expected: Option<&str>,
    ) -> Result<Option<(Expr, String)>> {
        let ty = expected.map(str::to_owned).or(self.stored_type(arg, span)?);
        let Some(ty) = ty.filter(|t| {
            manifest::reference_inner(t).is_none() && self.manifest.contains_references(t)
        }) else {
            return Ok(None);
        };
        crate::origins::result_type(&ty, self.origin_schemas, self.manifest, span)?;
        let stored = self.stored_value(arg, &ty, domain, span, true)?;
        for (fields, id) in &stored.loans {
            let loan = self.loan(*id).clone();
            self.record_stored_return(name, payload, &ty, fields, &loan, span)?;
        }
        Ok(Some((stored.expression, stored.ty)))
    }

    pub(super) fn exit_carrier_contents(
        &mut self,
        outcome: &str,
        payload: usize,
        target: &ResolvedPlace,
        span: Span,
    ) -> Result<()> {
        let loans: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter_map(|loan| {
                loan.holder
                    .as_ref()
                    .filter(|h| {
                        h.root == target.place.root
                            && h.indirect == target.place.indirect
                            && h.fields.starts_with(&target.place.fields)
                    })
                    .map(|h| (h.fields[target.place.fields.len()..].to_vec(), loan.clone()))
            })
            .collect();
        for (fields, loan) in loans {
            self.record_stored_return(outcome, payload, &target.ty, &fields, &loan, span)?;
        }
        Ok(())
    }
    pub(super) fn record_stored_return(
        &mut self,
        name: &str,
        payload: usize,
        ty: &str,
        fields: &[usize],
        loan: &Loan,
        span: Span,
    ) -> Result<()> {
        self.record_nested_return(name, payload, ty, &[], fields, loan, span)
    }
    #[allow(clippy::too_many_arguments)]
    fn record_nested_return(
        &mut self,
        name: &str,
        payload: usize,
        ty: &str,
        through: &[ReferenceStep],
        fields: &[usize],
        loan: &Loan,
        span: Span,
    ) -> Result<()> {
        let mut sources = vec![];
        for origin in &loan.origins {
            let source = self.return_source(origin, span)?;
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        if let Some(declared) = self.declared_stored.iter().find(|r| {
            r.outcome == name
                && r.payload == payload
                && r.through == through
                && r.fields == fields
                && r.sum_leaf == loan.sum_leaf
        }) {
            if sources.iter().any(|s| {
                !declared
                    .sources
                    .iter()
                    .any(|d| crate::origins::same_root(d, s))
            }) {
                return Err(self.error(
                    "E_LIFETIME",
                    span,
                    "returned payload field does not borrow its declared origin input",
                ));
            }
            sources = declared.sources.clone();
        }
        if let Some(previous) = self.result.stored_returns.iter_mut().find(|r| {
            r.outcome == name
                && r.payload == payload
                && r.through == through
                && r.fields == fields
                && r.sum_leaf == loan.sum_leaf
        }) {
            for source in sources {
                if !previous.sources.contains(&source) {
                    previous.sources.push(source);
                }
            }
        } else {
            let schema = self.origin_schemas.get(ty).unwrap();
            let group = schema.groups[schema
                .leaves
                .iter()
                .find(|l| l.through == through && l.fields == fields && l.sum_leaf == loan.sum_leaf)
                .unwrap()
                .group
                .unwrap()]
            .clone();
            self.result.stored_returns.push(StoredReturn {
                through: through.to_vec(),
                group,
                outcome: name.into(),
                payload,
                fields: fields.to_vec(),
                sum_leaf: loan.sum_leaf.clone(),
                sources,
            });
        }
        let mut nested = through.to_vec();
        nested.push(ReferenceStep {
            fields: fields.to_vec(),
            sum_leaf: loan.sum_leaf.clone(),
        });
        for (fields, child) in &loan.contents {
            self.record_nested_return(name, payload, ty, &nested, fields, child, span)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn receive_tagged(
        &mut self,
        root: Id,
        ty: &str,
        fields: &[usize],
        leaf: &SumLeaf,
        origins: Vec<LoanOrigin>,
        parents: Vec<Id>,
        domain: Formula,
        span: Span,
    ) -> Result<Id> {
        let mut ty = ty.to_owned();
        for field in fields {
            ty = self
                .manifest
                .field_at(&ty, *field)
                .ok_or_else(|| self.error("E_ORIGIN", span, "invalid tagged origin holder"))?
                .ty
                .clone();
        }
        let ty = leaf
            .reference_type(&ty, self.manifest)
            .ok_or_else(|| self.error("E_ORIGIN", span, "invalid tagged reference origin"))?;
        let (mutable, inner) = manifest::reference_inner(&ty)
            .ok_or_else(|| self.error("E_ORIGIN", span, "tagged origin must end at a reference"))?;
        let inner = inner.to_owned();
        let key = format!("$tagged_origin_{}", self.fresh());
        let address = self.define(&key, &ty, domain, span)?;
        let id = self.stash_stored_loan(
            Loan {
                contents: vec![],
                sum_leaf: Some(leaf.clone()),
                id: 0,
                target: ResolvedPlace {
                    carriers: vec![],
                    scope: self.scopes.len() - 1,
                    name: key,
                    place: Place {
                        root: address,
                        fields: vec![],
                        indirect: true,
                    },
                    ty: inner,
                    via: None,
                },
                origins,
                parents,
                mutable,
                active: domain,
                defined: domain,
                holder: None,
            },
            domain,
        );
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
            fields: fields.to_vec(),
            indirect: manifest::reference_inner(&self.result.slots[&root].ty).is_some(),
        });
        // The address is an abstract origin only. The actual reference is loaded
        // from its active payload by delta, never by an unconditional projection.
        Ok(address)
    }

    /// Materialize a reference field into a private address slot. The public
    /// holder owns the permission; the private slot supplies only runtime access.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn receive_stored(
        &mut self,
        name: &str,
        root: Id,
        ty: &str,
        fields: &[usize],
        origins: Vec<LoanOrigin>,
        parents: Vec<Id>,
        domain: Formula,
        span: Span,
    ) -> Result<(Id, Expr)> {
        let indirect = manifest::reference_inner(&self.result.slots[&root].ty).is_some();
        let mut leaf_ty = ty;
        for field in fields {
            leaf_ty = &self
                .manifest
                .field_at(leaf_ty, *field)
                .ok_or_else(|| self.error("E_LIFETIME", span, "invalid returned reference field"))?
                .ty;
        }
        let (mutable, inner) = manifest::reference_inner(leaf_ty).ok_or_else(|| {
            self.error(
                "E_LIFETIME",
                span,
                "returned origin is not a reference field",
            )
        })?;
        let (inner, leaf_ty) = (inner.to_owned(), leaf_ty.to_owned());
        let key = format!("$stored_address_{}", self.fresh());
        let slot = self.define(&key, &leaf_ty, domain, span)?;
        let id = self.stash_stored_loan(
            Loan {
                contents: vec![],
                sum_leaf: None,
                id: 0,
                target: ResolvedPlace {
                    carriers: vec![],
                    scope: self.scopes.len() - 1,
                    name: key,
                    place: Place {
                        root: slot,
                        fields: vec![],
                        indirect: true,
                    },
                    ty: inner,
                    via: None,
                },
                origins,
                parents,
                mutable,
                active: domain,
                defined: domain,
                holder: None,
            },
            domain,
        );
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
            fields: fields.to_vec(),
            indirect,
        });
        let _ = name; // The holder's nominal slot retains its source name.
        Ok((
            slot,
            Expr::Place(Place {
                root,
                fields: fields.to_vec(),
                indirect,
            }),
        ))
    }
}

impl Checker<'_> {
    pub(super) fn return_source(&self, origin: &LoanOrigin, span: Span) -> Result<BorrowSource> {
        if let Some((parameter, contained)) = self.contained_inputs.get(&origin.place.root) {
            return Ok(BorrowSource {
                guard: origin.guard,
                parameter: *parameter,
                contained: Some(contained.clone()),
                fields: origin.place.fields.clone(),
                opaque: origin.opaque,
            });
        }
        let parameter = self.result.parameters.iter().position(|p| p.id == origin.place.root && manifest::reference_inner(&p.ty).is_some())
            .filter(|_| origin.place.indirect)
            .ok_or_else(|| self.error("E_LIFETIME", span, "returned reference must borrow an input reference or contained origin; local or owned parameter storage would be destroyed"))?;
        Ok(BorrowSource {
            guard: origin.guard,
            parameter,
            contained: None,
            fields: origin.place.fields.clone(),
            opaque: origin.opaque,
        })
    }
    pub(super) fn initialize_contained_inputs(&mut self, span: Span) -> Result<()> {
        for (parameter, slot) in self.result.parameters.clone().iter().enumerate() {
            let ty = manifest::reference_inner(&slot.ty).map_or(slot.ty.as_str(), |(_, ty)| ty);
            let Some(schema) = self.origin_schemas.get(ty).filter(|s| !s.groups.is_empty()) else {
                continue;
            };
            let entries = schema
                .leaves
                .iter()
                .map(|leaf| {
                    (
                        leaf.through.clone(),
                        ReferenceStep {
                            fields: leaf.fields.clone(),
                            sum_leaf: leaf.sum_leaf.clone(),
                        },
                        vec![],
                        vec![],
                    )
                })
                .collect::<Vec<_>>();
            let (bindings, addresses) =
                self.receive_public_contents(slot.id, ty, &entries, TRUE, span)?;
            for (id, expr) in bindings {
                self.emit(Guard::True, Action::Bind(id, expr), span);
            }
            let mut initialized = vec![];
            for leaf in &schema.leaves {
                let mut path = leaf.through.clone();
                path.push(ReferenceStep {
                    fields: leaf.fields.clone(),
                    sum_leaf: leaf.sum_leaf.clone(),
                });
                let address = addresses.iter().find(|(p, _)| *p == path).unwrap().1;
                self.contained_inputs.insert(
                    address,
                    (
                        parameter,
                        ContainedOrigin {
                            through: leaf.through.clone(),
                            group: schema.groups[leaf.group.unwrap()].clone(),
                            sum_leaf: leaf.sum_leaf.clone(),
                            fields: leaf.fields.clone(),
                        },
                    ),
                );
                initialized.push((leaf, address));
            }
            // Every received reference field is a distinct permission. The
            // caller has already checked construction/transfer of the record.
            let family = self.fresh();
            for (member, (leaf, address)) in initialized.iter().enumerate() {
                let origins = initialized
                    .iter()
                    .filter(|(other, _)| other.group == leaf.group)
                    .map(|(_, address)| LoanOrigin {
                        permissions: vec![Permission::new(family, member)],
                        guard: false,
                        place: Place {
                            root: *address,
                            fields: vec![],
                            indirect: true,
                        },
                        opaque: initialized
                            .iter()
                            .filter(|(other, _)| other.group == leaf.group)
                            .count()
                            > 1,
                    })
                    .collect();
                let loan = self.scopes[0]
                    .loans
                    .values_mut()
                    .find(|l| l.target.place.root == *address)
                    .unwrap();
                loan.origins = origins;
            }
            self.refresh_public_contents(&addresses);
            self.attach_received_contents(slot.id);
        }
        Ok(())
    }
    pub(super) fn stored_argument(
        &mut self,
        arg: &syntax::Expr,
        ty: &str,
        domain: Formula,
        span: Span,
    ) -> Result<Expr> {
        arg.checked(span, |span| {
            Ok(self.stored_value(arg, ty, domain, span, true)?.expression)
        })
    }
    pub(super) fn call_origin_targets(
        &mut self,
        source: &BorrowSource,
        arg: &syntax::Expr,
        param: &manifest::Parameter,
        domain: Formula,
        span: Span,
    ) -> Result<Vec<ResolvedPlace>> {
        let Some(contained) = source.contained.as_ref().filter(|c| c.sum_leaf.is_some()) else {
            return self
                .call_origin_target(source, arg, param, domain, span)
                .map(|t| vec![t]);
        };
        let arg = match &arg.kind {
            syntax::ExprKind::Call(n, args) if n == "clone" => match args.as_slice() {
                [
                    syntax::Expr {
                        kind: syntax::ExprKind::Borrow(value),
                        ..
                    },
                ] => value.as_ref(),
                _ => arg,
            },
            _ => arg,
        };
        let holder = if let Some((mutable, _)) = manifest::reference_inner(&param.ty) {
            self.reference_argument(arg, mutable, domain, span)?
        } else {
            self.resolve_place(arg, span)?.ok_or_else(|| self.error("E_UNSUPPORTED", span, "bind this borrowed variant locally before a call that returns its contained origins"))?
        };
        let mut fields = holder.place.fields.clone();
        fields.extend(&contained.fields);
        let loans: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter(|l| {
                l.sum_leaf == contained.sum_leaf
                    && l.holder.as_ref().is_some_and(|h| {
                        h.root == holder.place.root
                            && h.fields == fields
                            && h.indirect == holder.place.indirect
                    })
            })
            .cloned()
            .collect();
        let mut targets = vec![];
        for loan in loans {
            let needed = self.bdd.and(domain, loan.defined);
            if needed == FALSE {
                continue;
            }
            let mut target = loan.target;
            target.carriers.extend(holder.carriers.iter().copied());
            target.carriers.extend(holder.via);
            target.via = Some(loan.id);
            self.check_loans(&target, needed, false, span)?;
            targets.push(target);
        }
        Ok(targets)
    }
    pub(super) fn call_origin_target(
        &mut self,
        source: &BorrowSource,
        arg: &syntax::Expr,
        param: &manifest::Parameter,
        domain: Formula,
        span: Span,
    ) -> Result<ResolvedPlace> {
        if let Some(contained) = &source.contained {
            let arg = match &arg.kind {
                syntax::ExprKind::Call(n, args) if n == "clone" => match args.as_slice() {
                    [
                        syntax::Expr {
                            kind: syntax::ExprKind::Borrow(value),
                            ..
                        },
                    ] => value.as_ref(),
                    _ => arg,
                },
                _ => arg,
            };
            let mut holder = if let Some((mutable, _)) = manifest::reference_inner(&param.ty) {
                self.reference_argument(arg, mutable, domain, span)?
            } else if let Some(holder) = self.resolve_place(arg, span)? {
                holder
            } else {
                let mut expression = arg;
                let mut ty = param.ty.as_str();
                for index in &contained.fields {
                    let field = self.manifest.field_at(ty, *index).ok_or_else(|| {
                        self.error("E_LIFETIME", span, "invalid contained origin projection")
                    })?;
                    expression = match &expression.kind {
                        syntax::ExprKind::NamedRecord(_, fields) => fields.iter().find(|(n, _)| n == &field.name).map(|(_, e)| e),
                        syntax::ExprKind::Construct(_, values) | syntax::ExprKind::Call(_, values) => values.get(*index),
                        _ => None,
                    }.ok_or_else(|| self.error("E_UNSUPPORTED", span, "bind this reference-bearing expression locally before returning its contained origins"))?;
                    ty = &field.ty;
                }
                let mutable = manifest::reference_inner(ty)
                    .ok_or_else(|| {
                        self.error("E_LIFETIME", span, "invalid contained reference type")
                    })?
                    .0;
                return self.reference_argument(expression, mutable, domain, span);
            };
            for field in &contained.fields {
                let definition = self.manifest.field_at(&holder.ty, *field).ok_or_else(|| {
                    self.error("E_LIFETIME", span, "invalid contained origin projection")
                })?;
                holder.ty = definition.ty.clone();
                holder.place.fields.push(*field);
            }
            let (_, _, loan) = self
                .stored_loan(&holder)
                .ok_or_else(|| self.error("E_LIFETIME", span, "missing contained input loan"))?;
            let mut target = loan.target;
            target.carriers.extend(holder.carriers);
            target.carriers.extend(holder.via);
            target.via = Some(loan.id);
            self.check_loans(&target, domain, false, span)?;
            Ok(target)
        } else {
            let (mutable, _) =
                manifest::reference_inner(&param.ty).expect("reference return origin");
            self.reference_argument(arg, mutable, domain, span)
        }
    }
}

impl Checker<'_> {
    /// A borrow of a carrier includes access to each stored permission, not only
    /// the record's allocation. Validate existing children before lending it.
    pub(super) fn check_carrier_contents(
        &mut self,
        carrier: &ResolvedPlace,
        domain: Formula,
        mutable: bool,
        span: Span,
    ) -> Result<()> {
        if !self.manifest.contains_references(&carrier.ty) {
            return Ok(());
        }
        let loans: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter(|l| {
                l.holder.as_ref().is_some_and(|h| {
                    h.root == carrier.place.root
                        && h.indirect == carrier.place.indirect
                        && h.fields.starts_with(&carrier.place.fields)
                })
            })
            .cloned()
            .collect();
        for loan in loans {
            let mut target = loan.target.clone();
            target.via = Some(loan.id);
            target.carriers.extend(carrier.carriers.iter().copied());
            target.carriers.extend(carrier.via);
            let needed = self.bdd.and(domain, loan.defined);
            self.check_loans(&target, needed, mutable && loan.mutable, span)?;
        }
        Ok(())
    }
}
