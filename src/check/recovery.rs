//! Diagnostic-only replay with path-scoped uncertainty. Successful checking takes
//! the normal path; after a failure we start a fresh checker and omit known bad
//! statements and their dependencies. No replay can publish a checked program.
use super::*;

#[derive(Default)]
pub(super) struct Recovery {
    pub omitted: Vec<Span>,
    pub candidate: Option<Span>,
    pub uncertain: Formula,
    blocked: Formula,
    pub barriers: usize,
    tainted: BTreeMap<String, Formula>,
}

impl Recovery {
    pub fn new(omitted: Vec<Span>) -> Self {
        Self {
            omitted,
            ..Self::default()
        }
    }
}

pub(super) fn leaf(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Bind(..)
            | Statement::Assign(..)
            | Statement::Drop(..)
            | Statement::Emit(..)
            | Statement::Invoke(..)
    )
}

fn terminal(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Exit(..)
            | Statement::ExitName(..)
            | Statement::Forward(..)
            | Statement::Again(..)
    )
}

fn root(name: &str) -> String {
    name.split(['.', '[']).next().unwrap_or(name).to_owned()
}

impl Checker<'_> {
    fn recovery_names(&self, statement: &Statement) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        crate::liveness::statement(statement, &mut names);
        if let Statement::Bind(name, ..) = statement {
            names.insert(name.clone());
        }
        names.into_iter().map(|name| root(&name)).collect()
    }

    fn taint(&mut self, mut names: BTreeSet<String>, domain: Formula) {
        // Field-level recovery would need the same precision as the loan checker.
        // Instead, conservatively connect all reference carriers and their owners.
        // This includes stored/nested references and aliases of aliases, even when
        // a carrier's lexical name differs from its storage slot's display name.
        let mut borrowed = BTreeSet::new();
        fn loan_names(loan: &Loan, slots: &BTreeMap<Id, Slot>, names: &mut BTreeSet<String>) {
            names.insert(root(&loan.target.name));
            for place in std::iter::once(&loan.target.place)
                .chain(loan.origins.iter().map(|origin| &origin.place))
                .chain(loan.holder.iter())
            {
                if let Some(slot) = slots.get(&place.root) {
                    names.insert(root(&slot.name));
                }
            }
            for (_, nested) in &loan.contents {
                loan_names(nested, slots, names);
            }
        }
        for scope in &self.scopes {
            for (name, binding) in &scope.values {
                if self.manifest.contains_references(&binding.slot.ty) {
                    borrowed.insert(root(name));
                    borrowed.insert(root(&binding.slot.name));
                }
            }
            for (name, loan) in &scope.loans {
                borrowed.insert(root(name));
                loan_names(loan, &self.result.slots, &mut borrowed);
            }
        }
        if !names.is_disjoint(&borrowed) {
            names.extend(borrowed);
        }
        for name in names {
            let previous = self.recovery.tainted.get(&name).copied().unwrap_or(FALSE);
            let uncertain = self.bdd.or(previous, domain);
            self.recovery.tainted.insert(name, uncertain);
        }
        self.recovery.uncertain = self.bdd.or(self.recovery.uncertain, domain);
    }

    pub(super) fn recovery_uncertain(&mut self, domain: Formula) -> bool {
        self.bdd.and(self.recovery.uncertain, domain) != FALSE
    }

    pub(super) fn recovery_block(&mut self, domain: Formula) {
        self.recovery.uncertain = self.bdd.or(self.recovery.uncertain, domain);
        self.recovery.blocked = self.bdd.or(self.recovery.blocked, domain);
    }

    pub(super) fn recovery_blocked(&mut self, domain: Formula) -> bool {
        if self.bdd.and(self.recovery.blocked, domain) == FALSE {
            return false;
        }
        // Never check a reduced selector as though it were the authored selector.
        // A join touching a stalled history stalls its entire selected domain.
        self.recovery_block(domain);
        true
    }

    pub(super) fn recovery_mask(
        &mut self,
        mask: &Mask,
        span: Span,
        context: Formula,
    ) -> Result<Option<(Guard, Formula)>> {
        match self.mask(mask, span) {
            Ok((guard, domain)) => Ok(Some((guard, self.bdd.and(context, domain)))),
            Err(_) if self.recovery.uncertain != FALSE => {
                // An omitted decision may not have introduced its path names.
                // Stop this body rather than invent missing-path diagnostics.
                self.recovery_block(context);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// False omits a failed/dependent statement or a stalled history. Independent
    /// histories keep their original guards and full ownership checking.
    pub(super) fn recovery_statement(
        &mut self,
        statement: &Statement,
        span: Span,
        domain: Formula,
    ) -> bool {
        if self.recovery_blocked(domain) {
            let names = self.recovery_names(statement);
            self.taint(names, domain);
            return false;
        }
        if !leaf(statement) && self.recovery_uncertain(domain) {
            self.recovery_block(domain);
            return false;
        }
        let omitted = self.recovery.omitted.contains(&span);
        if omitted || self.recovery_uncertain(domain) {
            let names = self.recovery_names(statement);
            let dependent = names.iter().any(|name| {
                self.recovery
                    .tainted
                    .get(name)
                    .is_some_and(|&tainted| self.bdd.and(tainted, domain) != FALSE)
            });
            if omitted || dependent {
                self.taint(names, domain);
                if !leaf(statement) {
                    self.recovery_block(domain);
                }
                return false;
            }
        }
        true
    }

    pub(super) fn recovery_error(&mut self, statement: &Statement, span: Span, error: &Diagnostic) {
        if self.recovery.candidate.is_none()
            && self.recovery.barriers == 0
            && self.boundaries.is_empty()
            && self.handler.is_none()
            && self.result.boundary.is_none()
            && !self.result.contingency
            && (leaf(statement) || terminal(statement))
            && !matches!(error.code, "E_FAULT_RESOURCE" | "E_TERMINATED" | "E_EMPTY")
        {
            self.recovery.candidate = Some(span);
        }
    }
}
