//! Bounded editor facts captured during checking, outside executable IR/certificates.
use crate::{
    Span,
    ir::{Guard, Id},
    syntax::SelectionKind,
};
use std::collections::BTreeMap;
const MAX_NODES: usize = 96;
const MAX_DEPTH: usize = 48;
const MAX_ORIGINS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fact {
    pub id: Id,
    pub name: String,
    pub outcome: String,
    pub span: Span,
    pub implicit: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Condition {
    Entry,
    Fact(Fact),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
    Omitted,
}
impl Condition {
    fn capture(
        guard: &Guard,
        facts: &BTreeMap<Id, Fact>,
        budget: &mut usize,
        depth: usize,
    ) -> Self {
        if *budget == 0 || depth == MAX_DEPTH {
            return Self::Omitted;
        }
        *budget -= 1;
        match guard {
            Guard::True => Self::Entry,
            Guard::Atom(id) => facts
                .get(id)
                .cloned()
                .map(Self::Fact)
                .unwrap_or(Self::Omitted),
            Guard::And(a, b) | Guard::Or(a, b) => {
                let a = Self::capture(a, facts, budget, depth + 1);
                let b = Self::capture(b, facts, budget, depth + 1);
                let and = matches!(guard, Guard::And(..));
                // Only exact Boolean identities; never assume data feasibility.
                if a == b && a != Self::Omitted {
                    return a;
                }
                match (&a, &b, and) {
                    (Self::Entry, _, true) => b,
                    (_, Self::Entry, true) => a,
                    (Self::Entry, _, false) | (_, Self::Entry, false) => Self::Entry,
                    _ => Self::join(a, b, and),
                }
            }
        }
    }
    fn join(a: Self, b: Self, and: bool) -> Self {
        fn flatten(c: Condition, and: bool, terms: &mut Vec<Condition>) {
            match c {
                Condition::And(a, b) if and => {
                    flatten(*a, and, terms);
                    flatten(*b, and, terms);
                }
                Condition::Or(a, b) if !and => {
                    flatten(*a, and, terms);
                    flatten(*b, and, terms);
                }
                c => {
                    // An omitted subtree is not known equal to another omitted subtree.
                    let incomplete = c.facts(&mut BTreeMap::new());
                    if incomplete || !terms.contains(&c) {
                        terms.push(c);
                    }
                }
            }
        }
        let mut terms = vec![];
        flatten(a, and, &mut terms);
        flatten(b, and, &mut terms);
        terms
            .into_iter()
            .reduce(|a, b| {
                if and {
                    Self::And(Box::new(a), Box::new(b))
                } else {
                    Self::Or(Box::new(a), Box::new(b))
                }
            })
            .unwrap()
    }
    fn facts<'a>(&'a self, result: &mut BTreeMap<Id, &'a Fact>) -> bool {
        match self {
            Self::Fact(f) => {
                result.insert(f.id, f);
                false
            }
            Self::And(a, b) | Self::Or(a, b) => a.facts(result) | b.facts(result),
            Self::Omitted => true,
            Self::Entry => false,
        }
    }
    fn text(&self, parent: u8, facts: &BTreeMap<Id, &Fact>, offset: usize) -> String {
        let (precedence, text) = match self {
            Self::Entry => (3, "entry".into()),
            Self::Omitted => (3, "…".into()),
            Self::Fact(f) => {
                let duplicate = facts.values().filter(|other| other.name == f.name).count() > 1;
                (
                    3,
                    if duplicate {
                        format!(
                            "{}@{}:{}",
                            f.name,
                            f.span.line.saturating_sub(offset),
                            f.span.column
                        )
                    } else {
                        f.name.clone()
                    },
                )
            }
            Self::And(a, b) => (
                2,
                format!(
                    "{} & {}",
                    a.text(2, facts, offset),
                    b.text(2, facts, offset)
                ),
            ),
            Self::Or(a, b) => (
                1,
                format!(
                    "{} | {}",
                    a.text(1, facts, offset),
                    b.text(1, facts, offset)
                ),
            ),
        };
        if precedence < parent {
            format!("({text})")
        } else {
            text
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct SelectedHistory {
    pub control: String,
    pub span: Span,
    pub kind: SelectionKind,
    condition: Condition,
    iteration: Option<String>,
    empty: bool,
    contingency: bool,
}
impl SelectedHistory {
    pub fn summary(&self, offset: usize) -> crate::explain::Selection {
        let mut facts = BTreeMap::new();
        let condition_truncated = self.condition.facts(&mut facts);
        crate::explain::Selection {
            contingency: self.contingency,
            kind: match self.kind {
                SelectionKind::Statement => "statement",
                SelectionKind::Outcome => "outcome",
                SelectionKind::Arm => "arm",
                SelectionKind::Scope => "scope",
                SelectionKind::Alias => "alias",
                SelectionKind::Loop => "loop",
            }
            .into(),
            condition: self.condition.text(0, &facts, offset),
            structurally_empty: self.empty,
            iteration: self.iteration.clone(),
            decision_arms: facts
                .values()
                .take(MAX_ORIGINS)
                .map(|f| crate::explain::DecisionArm {
                    name: f.name.clone(),
                    outcome: f.outcome.clone(),
                    implicit: f.implicit,
                    location: Span {
                        line: f.span.line.saturating_sub(offset),
                        column: f.span.column,
                    },
                })
                .collect(),
            condition_truncated,
            origins_truncated: facts.len() > MAX_ORIGINS,
            limits: crate::explain::SelectionLimits {
                condition_nodes: MAX_NODES,
                condition_depth: MAX_DEPTH,
                decision_arms: MAX_ORIGINS,
            },
        }
    }
    pub fn capture(
        control: String,
        span: Span,
        kind: SelectionKind,
        guard: &Guard,
        facts: &BTreeMap<Id, Fact>,
        iteration: Option<String>,
        empty: bool,
        contingency: bool,
    ) -> Self {
        let mut budget = MAX_NODES;
        Self {
            control,
            span,
            kind,
            condition: Condition::capture(guard, facts, &mut budget, 0),
            iteration,
            empty,
            contingency,
        }
    }
    pub fn markdown(&self, offset: usize) -> String {
        let mut facts = BTreeMap::new();
        let limited = self.condition.facts(&mut facts);
        let title = match self.kind {
            SelectionKind::Alias => "Path alias",
            SelectionKind::Loop => "Loop entry selection",
            _ => "Selected histories",
        };
        let mut text = format!(
            "**{title}**\n\n```text\n{}\n```",
            self.condition.text(0, &facts, offset)
        );
        if self.empty {
            text.push_str("\n\nThis selection is structurally empty.");
        }
        if self.contingency {
            text.push_str("\n\nContingency context: `entry` means contingency entry after failed work retires, not ordinary control entry. These selections cannot resume the failed work.");
        }
        if let Some(name) = &self.iteration {
            text.push_str(&format!("\n\nWithin one iteration of `{name}`. Entry already includes the conditions needed to reach the loop; it is not the whole control entry."));
        } else if self.kind == SelectionKind::Loop {
            text.push_str("\n\nOnly histories still live at this loop enter it.");
        }
        if self.kind == SelectionKind::Alias {
            text.push_str("\n\nDefines a path selection; declaring the alias executes no work.");
        }
        if !facts.is_empty() {
            text.push_str("\n\nDecision arms:");
            for f in facts.values().take(MAX_ORIGINS) {
                let label = if f.implicit {
                    " (local outcome fact)"
                } else {
                    ""
                };
                text.push_str(&format!(
                    "\n- `{}`{label}: `{}` at line {}, column {}.",
                    f.name,
                    f.outcome,
                    f.span.line.saturating_sub(offset),
                    f.span.column
                ));
            }
        }
        if limited || facts.len() > MAX_ORIGINS {
            text.push_str("\n\nDisplay limited; omitted conditions/origins are not implied to be unconditional.");
        }
        text.push_str("\n\nResolved structural selection, not a proven feasible input. Aliases are expanded; a union executes shared work once.");
        text
    }
}
