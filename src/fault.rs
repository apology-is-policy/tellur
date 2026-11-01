//! Runtime failure context shared by the reference executor and generated programs.
//! Activation identities describe invocations; they are not storage/frame addresses.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub const MAX_ACTIVATIONS: usize = 32;
pub const MAX_EXPANSION_CALLS: usize = 32;
pub const MAX_MESSAGE_CHARS: usize = 4096;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    ReturnedError,
    SourceFault,
}
impl Kind {
    fn is_returned(&self) -> bool {
        *self == Self::ReturnedError
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub control: String,
    pub path: Option<String>,
    pub line: usize,
    pub column: usize,
    pub label: Option<String>,
    pub expansion_calls: Vec<String>,
    pub omitted_expansion_calls: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    EntryValidation,
    Execution,
    ResultValidation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activation {
    pub id: u64,
    pub parent: Option<u64>,
    pub control: String,
    pub called_at: Option<Site>,
    pub current_site: Option<Site>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<Report>>,
    #[serde(default, skip_serializing_if = "Kind::is_returned")]
    pub kind: Kind,
    pub format: String,
    pub version: u32,
    pub boundary: String,
    pub phase: Phase,
    pub message: String,
    pub message_truncated: bool,
    pub activations: Vec<Activation>,
    pub omitted_activations: usize,
    pub site: Option<Site>,
}
impl Report {
    /// Bounded source-facing diagnostic data, independent of either executor's values.
    pub fn source_fields(&self) -> (String, Vec<String>, Vec<String>) {
        let stack = self
            .activations
            .iter()
            .map(|a| format!("{}#{}", a.control, a.id))
            .collect();
        let paths = self
            .activations
            .iter()
            .filter_map(|a| a.current_site.as_ref())
            .map(|s| {
                format!(
                    "{}:{}:{}{}",
                    s.control,
                    s.line,
                    s.column,
                    s.label
                        .as_ref()
                        .map(|l| format!(" [{l}]"))
                        .unwrap_or_default()
                )
            })
            .collect();
        (
            self.message
                .strip_prefix("E_FAULT: ")
                .unwrap_or(&self.message)
                .into(),
            stack,
            paths,
        )
    }
}
impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}{}",
            self.message,
            if self.message_truncated { "…" } else { "" }
        )?;
        writeln!(
            f,
            "execution boundary `{}` failed ({})",
            self.boundary,
            match self.phase {
                Phase::EntryValidation => "entry validation",
                Phase::Execution => "execution",
                Phase::ResultValidation => "result validation",
            }
        )?;
        for frame in &self.activations {
            write!(f, "  activation {}: {}", frame.id, frame.control)?;
            if let Some(at) = &frame.current_site {
                write!(
                    f,
                    " at {}:{}:{}",
                    at.path.as_deref().unwrap_or("<source>"),
                    at.line,
                    at.column
                )?;
                if at.control != frame.control {
                    write!(f, " (source control {})", at.control)?;
                }
            }
            writeln!(f)?;
        }
        if self.omitted_activations > 0 {
            writeln!(
                f,
                "  {} intervening activations omitted",
                self.omitted_activations
            )?;
        }
        if let Some(site) = &self.site {
            for call in &site.expansion_calls {
                writeln!(f, "  expanded through {call}")?;
            }
            if site.omitted_expansion_calls > 0 {
                writeln!(
                    f,
                    "  {} expansion origins omitted",
                    site.omitted_expansion_calls
                )?;
            }
        }
        if let Some(cause) = &self.cause {
            writeln!(f, "Caused by: {cause}")?;
        }
        write!(
            f,
            "No rollback or resource-protocol completion is asserted."
        )
    }
}
#[derive(Debug)]
struct Active {
    id: u64,
    parent: Option<u64>,
    control: String,
    called_at: Option<usize>,
    current_site: Option<usize>,
}
/// Retains only active invocations. Loop iterations and completed calls do not
/// accumulate history. Child trackers share an execution-local identity allocator,
/// but have independent active chains and fault state. Once a report is captured,
/// this tracker cannot resume. A tracker is diagnostic context, not a task scheduler.
#[derive(Default, Debug)]
pub struct Tracker {
    cause: Option<Box<Report>>,
    kind: Kind,
    sites: BTreeMap<usize, Site>,
    active: Vec<Active>,
    identities: Arc<AtomicU64>,
    failed: bool,
}
impl Tracker {
    pub fn is_source_fault(&self) -> bool {
        self.kind == Kind::SourceFault
    }
    pub fn child(&self) -> Self {
        Self {
            sites: self.sites.clone(),
            identities: Arc::clone(&self.identities),
            ..Self::default()
        }
    }
    /// Keep sequential join compatibility; related trackers already share allocation.
    /// This does not merge active chains, fault state or completed invocation history.
    pub fn absorb(&mut self, child: &Self) {
        self.identities
            .fetch_max(child.identities.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    pub fn with_cause(&mut self, mut report: Report) {
        // A bounded causal chain, even with nested boundary escalation.
        let mut cursor = &mut report;
        for _ in 0..7 {
            let Some(ref mut cause) = cursor.cause else {
                break;
            };
            cursor = cause;
        }
        cursor.cause = None;
        self.cause = Some(Box::new(report));
    }
    pub fn source_fault(&mut self) {
        self.kind = Kind::SourceFault;
    }
    pub fn new(sites: BTreeMap<usize, Site>) -> Self {
        Self {
            sites,
            ..Self::default()
        }
    }
    pub fn enter(&mut self, control: &str) -> Result<(), String> {
        if self.failed {
            return Err("execution boundary has already failed".into());
        }
        // Allocation orders identities only, not program effects or task scheduling.
        // Failed allocation leaves the counter saturated, so IDs can never wrap/recur.
        let id = self
            .identities
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                last.checked_add(1)
            })
            .map_err(|_| "activation identity space exhausted")?
            + 1;
        self.active.push(Active {
            id,
            parent: self.active.last().map(|a| a.id),
            control: control.into(),
            called_at: self.active.last().and_then(|a| a.current_site),
            current_site: None,
        });
        Ok(())
    }
    pub fn at(&mut self, site: usize) {
        if let Some(active) = self.active.last_mut() {
            active.current_site = Some(site);
        }
    }
    pub fn leave(&mut self) {
        self.active.pop();
    }
    pub fn failure(&mut self, boundary: &str, phase: Phase, message: &str) -> Report {
        self.failed = true;
        let omitted = self.active.len().saturating_sub(MAX_ACTIVATIONS);
        let location = |id: Option<usize>| id.and_then(|id| self.sites.get(&id)).cloned();
        let activations = self
            .active
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                omitted == 0
                    || *i < MAX_ACTIVATIONS / 2
                    || *i >= self.active.len() - MAX_ACTIVATIONS / 2
            })
            .map(|(_, a)| Activation {
                id: a.id,
                parent: a.parent,
                control: a.control.clone(),
                called_at: location(a.called_at),
                current_site: location(a.current_site),
            })
            .collect();
        Report {
            cause: self.cause.clone(),
            kind: self.kind.clone(),
            format: "tellur.execution-failure".into(),
            version: 1,
            boundary: boundary.into(),
            phase,
            message: message.chars().take(MAX_MESSAGE_CHARS).collect(),
            message_truncated: message.chars().nth(MAX_MESSAGE_CHARS).is_some(),
            activations,
            omitted_activations: omitted,
            site: location(self.active.last().and_then(|a| a.current_site)),
        }
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn report(tracker: &mut Tracker) -> Report {
        tracker.failure("test", Phase::Execution, "test report")
    }

    #[test]
    fn overlapping_children_share_ids_without_joining_or_merging_active_chains() {
        let mut root = Tracker::default();
        root.enter("root").unwrap();
        let mut first = root.child();
        let mut second = root.child();
        first.enter("first").unwrap();
        second.enter("second").unwrap();
        first.enter("nested").unwrap();
        second.leave();
        second.enter("replacement").unwrap();
        root.enter("parent_helper").unwrap();
        let first = report(&mut first);
        let second = report(&mut second);
        let root = report(&mut root);
        assert_eq!(
            first
                .activations
                .iter()
                .map(|a| (a.id, a.parent))
                .collect::<Vec<_>>(),
            [(2, None), (4, Some(2))]
        );
        assert_eq!(
            second
                .activations
                .iter()
                .map(|a| (a.id, a.parent))
                .collect::<Vec<_>>(),
            [(5, None)]
        );
        assert_eq!(
            root.activations
                .iter()
                .map(|a| (a.id, a.parent))
                .collect::<Vec<_>>(),
            [(1, None), (6, Some(1))]
        );
    }

    #[test]
    fn child_failure_state_is_local_and_does_not_choose_a_scheduling_policy() {
        let mut root = Tracker::default();
        root.enter("root").unwrap();
        let mut failed = root.child();
        let mut separate = root.child();
        failed.enter("work").unwrap();
        failed.source_fault();
        let cause = report(&mut failed);
        assert_eq!(cause.kind, Kind::SourceFault);
        assert!(failed.enter("forbidden_resume").is_err());
        assert!(!separate.is_source_fault());
        separate.enter("independent_context").unwrap();
        let separate_report = report(&mut separate);
        assert_eq!(separate_report.activations[0].id, 3);
        assert!(separate_report.cause.is_none());
        root.absorb(&separate);
        root.absorb(&failed);
        root.enter("later").unwrap();
        let root_report = report(&mut root);
        assert_eq!(root_report.activations[1].id, 4);
        assert!(!root.is_source_fault());
        // The executor, not identity allocation, decides whether a context may run.
    }

    #[test]
    fn nested_children_and_completed_calls_keep_monotonic_ids_and_fresh_roots() {
        let mut root = Tracker::default();
        let mut child = root.child();
        let mut grandchild = child.child();
        for _ in 0..2048 {
            grandchild.enter("completed").unwrap();
            grandchild.leave();
        }
        root.absorb(&grandchild);
        child.absorb(&root);
        root.enter("parent").unwrap();
        child.enter("child").unwrap();
        grandchild.enter("grandchild").unwrap();
        assert_eq!(report(&mut root).activations[0].id, 2049);
        assert_eq!(report(&mut child).activations[0].id, 2050);
        let last = report(&mut grandchild);
        assert_eq!(last.activations.len(), 1);
        assert_eq!(last.activations[0].id, 2051);
        assert_eq!(last.omitted_activations, 0);
        let mut fresh = Tracker::default();
        fresh.enter("fresh_entry").unwrap();
        assert_eq!(report(&mut fresh).activations[0].id, 1);
    }

    #[test]
    fn shared_identity_exhaustion_never_wraps_or_partially_enters() {
        let mut root = Tracker::default();
        root.identities.store(u64::MAX - 1, Ordering::Relaxed);
        let mut first = root.child();
        let mut second = root.child();
        first.enter("last").unwrap();
        assert_eq!(
            second.enter("overflow"),
            Err("activation identity space exhausted".into())
        );
        assert_eq!(
            root.enter("still_exhausted"),
            Err("activation identity space exhausted".into())
        );
        assert!(report(&mut second).activations.is_empty());
        assert!(report(&mut root).activations.is_empty());
        assert_eq!(report(&mut first).activations[0].id, u64::MAX);
    }
}
