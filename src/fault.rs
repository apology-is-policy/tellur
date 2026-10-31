//! Runtime failure context shared by the reference executor and generated programs.
//! Activation identities describe invocations; they are not storage/frame addresses.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
/// accumulate history. Once a report is captured, this tracker cannot resume.
#[derive(Default, Debug)]
pub struct Tracker {
    cause: Option<Box<Report>>,
    kind: Kind,
    sites: BTreeMap<usize, Site>,
    active: Vec<Active>,
    next: u64,
    failed: bool,
}
impl Tracker {
    pub fn is_source_fault(&self) -> bool {
        self.kind == Kind::SourceFault
    }
    pub fn child(&self) -> Self {
        Self {
            sites: self.sites.clone(),
            next: self.next,
            ..Self::default()
        }
    }
    pub fn absorb(&mut self, child: &Self) {
        self.next = self.next.max(child.next);
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
        self.next = self
            .next
            .checked_add(1)
            .ok_or("activation identity space exhausted")?;
        self.active.push(Active {
            id: self.next,
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
