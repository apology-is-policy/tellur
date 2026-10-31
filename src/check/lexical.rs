use super::*;

#[derive(Clone)]
pub(super) struct BoundaryContext {
    retained: Vec<Id>,
    prior_slots: BTreeSet<Id>,
    first_loan: Id,
    protected: Vec<Id>,
}
#[derive(Clone)]
pub(super) struct HandlerContext {
    retained: Vec<Id>,
    prior_slots: BTreeSet<Id>,
    first_loan: Id,
    scope: usize,
    supervisor: Option<LoopContext>,
    protected: Vec<Id>,
}

impl Checker<'_> {
    pub(super) fn protected_state(&self) -> Vec<Id> {
        self.handler
            .as_ref()
            .map(|h| h.protected.clone())
            .or_else(|| self.boundaries.last().map(|b| b.protected.clone()))
            .unwrap_or_default()
    }
    pub(super) fn retained_for_fault(&self) -> Vec<Id> {
        self.result
            .boundary
            .iter()
            .flat_map(|b| b.retained.iter())
            .chain(self.boundaries.iter().flat_map(|b| &b.retained))
            .copied()
            .collect()
    }
    pub(super) fn boundary_access(&self, place: &ResolvedPlace, span: Span) -> Result<()> {
        let root = place.place.root;
        if let Some(handler) = &self.handler {
            if (handler.prior_slots.contains(&root) && !handler.retained.contains(&root))
                || place.via.is_some_and(|id| id < handler.first_loan)
            {
                return Err(self.error(
                    "E_CONTINGENCY_CAPTURE",
                    span,
                    "contingency can access only its retained owners, @fault and local values",
                ));
            }
        } else if let Some(boundary) = self.boundaries.last() {
            if boundary.protected.contains(&root) {
                return Err(self.error("E_SUPERVISOR_STATE", span, "supervisor state stays outside protected work; prepare an owned request input before entering the boundary"));
            }
            if place.via.is_some_and(|id| id < boundary.first_loan)
                || (boundary.prior_slots.contains(&root)
                    && self
                        .manifest
                        .contains_references(&self.result.slots[&root].ty))
            {
                return Err(self.error(
                    "E_BOUNDARY_BORROW",
                    span,
                    "outer references cannot enter isolated work; transfer an owned request input",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn handler_resources(&mut self, domain: Formula, span: Span) -> Result<()> {
        let handler = self.handler.clone().unwrap();
        for (index, scope) in self.scopes.iter().enumerate() {
            for binding in scope.values.values() {
                if index < handler.scope && !handler.retained.contains(&binding.slot.id) {
                    continue;
                }
                for owned in binding.owned.values() {
                    let live = self.bdd.and(domain, *owned);
                    if live != FALSE {
                        return Err(self.fail_domain("E_RESOURCE", span, &format!("contingency must complete or transfer `{}` before its terminal disposition",binding.slot.name),live));
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn terminate_process(
        &mut self,
        status: u8,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        if self.handler.is_none()
            || !self.boundaries.is_empty()
            || self.result.boundary.is_some()
            || self.result.contingency
        {
            return Err(self.error("E_PROCESS_EXIT", span, "process termination belongs only to the executable entry's outermost lexical contingency"));
        }
        self.handler_resources(domain, span)?;
        let handler = self.handler.clone().unwrap();
        for scope in &self.scopes[..handler.scope] {
            for binding in scope.values.values() {
                if handler.retained.contains(&binding.slot.id) {
                    continue;
                }
                for (fields, owned) in &binding.owned {
                    let live = self.bdd.and(domain, *owned);
                    if live == FALSE {
                        continue;
                    }
                    let mut ty = binding.slot.ty.as_str();
                    for field in fields {
                        ty = &self.manifest.field_at(ty, *field).unwrap().ty;
                    }
                    if !self.manifest.permits_fault(ty) {
                        return Err(self.fail_domain("E_FAULT_RESOURCE",span,"process termination would abandon an owner without a fault disposition",live));
                    }
                }
            }
        }
        self.dead = self.bdd.or(self.dead, domain);
        self.emit(guard, Action::TerminateProcess(status), span);
        Ok(())
    }
    pub(super) fn recover(
        &mut self,
        name: &str,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        let handler = self.handler.clone().ok_or_else(|| {
            self.error(
                "E_RECOVER",
                span,
                "recover belongs to a lexical contingency",
            )
        })?;
        let supervisor = handler
            .supervisor
            .as_ref()
            .filter(|l| l.name == name)
            .ok_or_else(|| {
                self.error(
                    "E_RECOVER",
                    span,
                    "recover must name the enclosing supervised loop",
                )
            })?;
        self.handler_resources(domain, span)?;
        // Other abandoned owners need their declared fault disposition. The
        // immutable recovery edge cannot transmit handler-computed payloads.
        for scope in &self.scopes[..handler.scope] {
            for binding in scope.values.values() {
                if handler.protected.contains(&binding.slot.id)
                    || handler.retained.contains(&binding.slot.id)
                {
                    continue;
                }
                for (fields, owned) in &binding.owned {
                    let live = self.bdd.and(domain, *owned);
                    if live == FALSE {
                        continue;
                    }
                    let mut ty = binding.slot.ty.as_str();
                    for field in fields {
                        ty = &self.manifest.field_at(ty, *field).unwrap().ty;
                    }
                    if !self.manifest.permits_fault(ty) {
                        return Err(self.fail_domain(
                            "E_FAULT_RESOURCE",
                            span,
                            "recovery would abandon an owner without a fault disposition",
                            live,
                        ));
                    }
                }
            }
        }
        let args = supervisor
            .parameters
            .iter()
            .zip(&handler.protected)
            .map(|(p, id)| {
                let place = Place {
                    root: *id,
                    fields: vec![],
                    indirect: false,
                };
                if self.moves(&p.ty) {
                    Expr::Move(place)
                } else {
                    Expr::Place(place)
                }
            })
            .collect();
        self.dead = self.bdd.or(self.dead, domain);
        self.emit(
            guard,
            Action::Recover {
                target: supervisor.id,
                args,
            },
            span,
        );
        Ok(())
    }
    pub(super) fn lexical_boundary(
        &mut self,
        body: &[Item],
        captures: &[String],
        handler: &[Item],
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        if self.handler.is_some() {
            return Err(self.error(
                "E_CONTINGENT",
                span,
                "a contingency cannot establish new protected work",
            ));
        }
        self.contingent_call(false, span)?;
        let supervisor = self.loop_context.clone().filter(|l| l.supervised);
        let mut protected = vec![];
        if let Some(loop_) = &supervisor {
            for p in &loop_.parameters {
                let value = syntax::ExprKind::Name(p.name.clone()).synthetic();
                let place = self.resolve_place(&value, span)?.unwrap();
                if self.manifest.contains_references(&place.ty) {
                    return Err(self.error(
                        "E_BOUNDARY_BORROW",
                        span,
                        "supervisor state must own its data",
                    ));
                }
                if !self.protected_state().contains(&place.place.root) {
                    self.use_place(place.clone(), domain, false, span)?;
                    self.check_loans(&place, domain, true, span)?;
                }
                protected.push(place.place.root);
            }
        }
        if let Some(loop_) = &supervisor {
            // Values from outside the loop belong to its supervisor as well.
            // They are not restart arguments, but must survive every iteration.
            protected.extend(
                self.scopes[..loop_.scope]
                    .iter()
                    .flat_map(|s| s.values.values().map(|b| b.slot.id)),
            );
        }
        let mut retained = vec![];
        for name in captures {
            let value = syntax::ExprKind::Name(name.clone()).synthetic();
            let place = self.resolve_place(&value, span)?.ok_or_else(|| {
                self.error(
                    "E_BOUNDARY_CAPTURE",
                    span,
                    format!("capture `{name}` requires a named owner"),
                )
            })?;
            if !place.place.fields.is_empty()
                || place.via.is_some()
                || self.manifest.contains_references(&place.ty)
                || protected.contains(&place.place.root)
                || retained.contains(&place.place.root)
            {
                return Err(self.error("E_BOUNDARY_CAPTURE",span,"captures must be distinct whole owned request values, separate from supervisor state"));
            }
            self.use_place(place.clone(), domain, false, span)?;
            self.check_loans(&place, domain, true, span)?;
            retained.push(place.place.root);
        }
        let prior_slots: BTreeSet<_> = self
            .scopes
            .iter()
            .flat_map(|s| s.values.values().map(|b| b.slot.id))
            .collect();
        let prior_result_slots: BTreeSet<_> = self.result.slots.keys().copied().collect();
        let mut keep = self.retained_for_fault();
        keep.extend(&retained);
        keep.extend(&protected);
        let before_scopes = self.scopes.clone();
        let before_dead = self.dead;
        let outer_code = std::mem::take(&mut self.result.code);
        let first_loan = *self.next;
        self.boundaries.push(BoundaryContext {
            retained: retained.clone(),
            prior_slots: prior_slots.clone(),
            first_loan,
            protected: protected.clone(),
        });
        self.scopes.push(Scope::default());
        self.scopes
            .last_mut()
            .unwrap()
            .paths
            .insert("Entry".into(), (Guard::True, domain));
        let work_scope = self.scopes.len() - 1;
        self.items(body, Guard::True, domain)?;
        let live = self.bdd.minus(domain, self.dead);
        self.resources(live, span, Some(work_scope))?;
        self.scopes.pop();
        self.boundaries.pop();
        let work_slots: Vec<_> = self
            .result
            .slots
            .keys()
            .filter(|id| !prior_result_slots.contains(id))
            .copied()
            .collect();
        let mut retire: Vec<_> = prior_slots
            .iter()
            .filter(|id| !keep.contains(id))
            .copied()
            .collect();
        retire.extend(&work_slots);
        let normal_scopes = std::mem::replace(&mut self.scopes, before_scopes);
        for scope in &mut self.scopes {
            scope.paths.clear();
            scope.site_aliases.clear();
            scope.ambiguous_paths.clear();
            scope.sites.clear();
        }
        let normal_dead = std::mem::replace(&mut self.dead, before_dead);
        let checked_body = std::mem::take(&mut self.result.code);
        self.scopes.push(Scope::default());
        self.scopes
            .last_mut()
            .unwrap()
            .paths
            .insert("Entry".into(), (Guard::True, domain));
        let handler_scope = self.scopes.len() - 1;
        let handler_start = *self.next;
        let fault_slot = self.define("$fault_report", "fault_report", domain, span)?;
        let fault_place = self
            .resolve_place(
                &syntax::ExprKind::Name("$fault_report".into()).synthetic(),
                span,
            )?
            .unwrap();
        let origins = self.origins(&fault_place);
        self.add_loan("$fault", fault_place, origins, vec![], false, domain);
        self.handler = Some(HandlerContext {
            retained: retained.clone(),
            prior_slots,
            first_loan: handler_start,
            scope: handler_scope,
            supervisor,
            protected,
        });
        self.items(handler, Guard::True, domain)?;
        let live = self.bdd.minus(domain, self.dead);
        if live != FALSE {
            return Err(self.fail_domain(
                "E_UNCOVERED",
                span,
                "contingency has no terminal recovery, escalation or process disposition",
                live,
            ));
        }
        self.handler = None;
        let checked_handler = std::mem::replace(&mut self.result.code, outer_code);
        let handler_slots = self
            .result
            .slots
            .keys()
            .filter(|id| **id >= handler_start)
            .copied()
            .collect();
        self.scopes = normal_scopes;
        self.dead = normal_dead;
        self.emit(
            guard,
            Action::Boundary {
                body: checked_body,
                handler: checked_handler,
                retained,
                retire,
                work_slots,
                handler_slots,
                fault_slot,
            },
            span,
        );
        Ok(())
    }
}
