//! Diagnostic-only replay of straight-line statements. Successful checking takes
//! the normal path; after a failure we start a fresh checker and omit known bad
//! statements and their dependencies. No replay can publish a checked program.
use super::*;

#[derive(Default)]
pub(super) struct Recovery {
    pub omitted: Vec<Span>,
    pub candidate: Option<Span>,
    pub active: bool,
    pub depth: usize,
    tainted: BTreeSet<String>,
}

impl Recovery {
    pub fn new(omitted: Vec<Span>) -> Self {
        Self {
            omitted,
            ..Self::default()
        }
    }
}

fn leaf(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Bind(..)
            | Statement::Assign(..)
            | Statement::Drop(..)
            | Statement::Emit(..)
            | Statement::Invoke(..)
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

    fn taint(&mut self, names: BTreeSet<String>) {
        self.recovery.tainted.extend(names);
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
        if !self.recovery.tainted.is_disjoint(&borrowed) {
            self.recovery.tainted.extend(borrowed);
        }
    }

    /// None ends this diagnostic replay at a structural/termination boundary.
    /// Some(false) skips a failed statement or a dependent statement.
    pub(super) fn recovery_statement(&mut self, statement: &Statement, span: Span) -> Option<bool> {
        if self.recovery.active && !leaf(statement) {
            return None;
        }
        let omitted = self.recovery.omitted.contains(&span);
        if omitted || self.recovery.active {
            let names = self.recovery_names(statement);
            if omitted || !names.is_disjoint(&self.recovery.tainted) {
                self.recovery.active = true;
                self.taint(names);
                return Some(false);
            }
        }
        Some(true)
    }

    pub(super) fn recovery_error(&mut self, statement: &Statement, span: Span, error: &Diagnostic) {
        // A faulting call checks *all* live resources, not just its arguments.
        // Once a statement is omitted, those global obligations are uncertain.
        if self.recovery.depth == 1
            && leaf(statement)
            && !matches!(error.code, "E_FAULT_RESOURCE" | "E_TERMINATED" | "E_EMPTY")
        {
            self.recovery.candidate = Some(span);
        }
    }
}
