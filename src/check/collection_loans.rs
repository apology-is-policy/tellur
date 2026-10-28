//! Local shared-reference collections retain a conservative union of element origins.
use super::*;
use crate::collection_support::Collection;
pub(super) struct CollectionLoans {
    target: ResolvedPlace,
    existing: Vec<Loan>,
    inserted: Vec<Loan>,
    operation: String,
    pub(super) outcomes: Vec<manifest::Outcome>,
}
impl CollectionLoans {
    pub(super) fn result_loans(&self, outcome: &str) -> &[Loan] {
        if self.operation.ends_with("_replace") && outcome == "out_of_bounds" {
            &self.inserted
        } else {
            &self.existing
        }
    }
}
impl Checker<'_> {
    pub(super) fn prepare_collection_loans(
        &mut self,
        operand: &syntax::Expr,
        domain: Formula,
        span: Span,
    ) -> Result<Option<CollectionLoans>> {
        let syntax::Expr::Call(name, args) = operand else {
            return Ok(None);
        };
        if !crate::collections::is_operation(name) {
            return Ok(None);
        }
        let first = args
            .first()
            .ok_or_else(|| self.error("E_ARITY", span, "missing collection argument"))?;
        let ty = self.collection_type(first, span)?;
        if !self.manifest.contains_references(&ty) {
            return Ok(None);
        }
        self.manifest.check_local_storage(&ty)?;
        let c = Collection::parse(&ty)
            .ok_or_else(|| self.error("E_COLLECTION", span, "operation requires a collection"))?;
        let contract = crate::collections::contract(name, &ty).ok_or_else(|| {
            self.error(
                "E_COLLECTION",
                span,
                "operation does not match collection type",
            )
        })?;
        if args.len() != contract.parameters.len() {
            return Err(self.error("E_ARITY", span, "wrong collection argument count"));
        }
        let expression = match first {
            syntax::Expr::Borrow(e) | syntax::Expr::BorrowMutable(e) => e.as_ref(),
            _ => first,
        };
        let target = if let Some((_, _, loan, _)) = self.reference_source(expression, span)? {
            loan.target
        } else {
            self.resolve_place(expression, span)?.ok_or_else(|| {
                self.error(
                    "E_COLLECTION",
                    span,
                    "borrowed collection needs local storage",
                )
            })?
        };
        if target.place.indirect {
            return Err(self.error(
                "E_UNSUPPORTED",
                span,
                "indirect borrowed collections require public collection origin contracts",
            ));
        }
        if !name.ends_with("_get")
            && !name.ends_with("_get_mut")
            && self
                .loop_context
                .as_ref()
                .is_some_and(|l| target.scope < l.scope)
        {
            return Err(self.error(
                "E_LOOP_CAPTURE",
                span,
                "updating outer borrowed collections needs loop origin state",
            ));
        }
        let mut inserted = vec![];
        if matches!(name.rsplit('_').next(), Some("push" | "insert" | "replace")) {
            let stored =
                self.stored_value(args.last().unwrap(), c.element(), domain, span, true)?;
            for (_, id) in stored.loans {
                let loan = self.loan(id).clone();
                let inner_slots = self
                    .scopes
                    .iter()
                    .skip(target.scope + 1)
                    .flat_map(|s| s.values.values().map(|v| v.slot.id))
                    .collect::<BTreeSet<_>>();
                if loan
                    .origins
                    .iter()
                    .any(|o| inner_slots.contains(&o.place.root))
                {
                    return Err(self.error(
                        "E_LIFETIME",
                        span,
                        "collection insertion cannot retain storage from an inner scope",
                    ));
                }
                for scope in &mut self.scopes {
                    for l in scope.loans.values_mut().filter(|l| l.id == id) {
                        l.active = FALSE;
                    }
                }
                inserted.push(loan);
            }
        }
        let existing = self
            .snapshot_carrier_contents(&target, domain)
            .into_iter()
            .map(|(_, l)| l)
            .collect();
        Ok(Some(CollectionLoans {
            target,
            existing,
            inserted,
            operation: name.clone(),
            outcomes: contract.outcomes,
        }))
    }
    pub(super) fn attach_collection_loans(&mut self, root: Id, loans: &[Loan], domain: Formula) {
        for loan in loans {
            let active = self.bdd.and(domain, loan.active);
            let id = self.stash_stored_loan(loan.clone(), active);
            self.scopes
                .last_mut()
                .unwrap()
                .loans
                .get_mut(&format!("$stored_{id}"))
                .unwrap()
                .holder = Some(Place {
                root,
                fields: vec![crate::collections::ELEMENT],
                indirect: false,
            });
        }
    }
    pub(super) fn update_collection_loans(
        &mut self,
        plan: &CollectionLoans,
        outcome: &str,
        domain: Formula,
    ) {
        if matches!(outcome, "pushed" | "inserted" | "replaced") {
            for loan in &plan.inserted {
                let id = self.fresh();
                let mut loan = loan.clone();
                loan.id = id;
                loan.active = domain;
                loan.defined = domain;
                let mut holder = plan.target.place.clone();
                holder.fields.push(crate::collections::ELEMENT);
                loan.holder = Some(holder);
                self.scopes[plan.target.scope]
                    .loans
                    .insert(format!("$stored_{id}"), loan);
            }
        }
        // pop's empty outcome proves no element remains; other dynamic removals
        // retain the union until the collection's last use or explicit drop.
        if plan.operation == "vector_pop" && outcome == "empty" {
            for scope in &mut self.scopes {
                for loan in scope.loans.values_mut() {
                    if plan.existing.iter().any(|old| old.id == loan.id) {
                        loan.active = self.bdd.minus(loan.active, domain);
                        loan.defined = self.bdd.minus(loan.defined, domain);
                    }
                }
            }
        }
    }
}
