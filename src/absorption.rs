//! Bounded source-contributor evidence, independent of executable IR and public contracts.
use crate::{Diagnostic, DiagnosticNote, Result, Span, contracts, ir::*};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const LIMIT: usize = 32;
pub(crate) const OMITTED: &str = "additional outcome contributors omitted (diagnostic limit)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Declared,
    Produced,
    Constructed,
    Received,
    Forwarded,
    Omitted,
}

/// Compiler identities and combined-source coordinates. Never serialized directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Raw {
    pub kind: Kind,
    pub control: String,
    pub operation: Option<String>,
    pub outcome: String,
    pub span: Span,
}
impl Raw {
    pub(crate) fn diagnostic(&self) -> DiagnosticNote {
        let message = match self.kind {
            Kind::Declared => format!(
                "outcome `{}` is declared by `{}` here",
                self.outcome, self.control
            ),
            Kind::Produced => format!(
                "outcome `{}` is produced by `{}` here (structural contributor)",
                self.outcome, self.control
            ),
            Kind::Constructed => format!(
                "outcome `{}` is constructed by `{}` here (structural contributor)",
                self.outcome, self.control
            ),
            Kind::Received | Kind::Forwarded => format!(
                "outcome `{}` is {} from `{}` by `{}` here",
                self.outcome,
                if self.kind == Kind::Received {
                    "received"
                } else {
                    "forwarded"
                },
                self.operation.as_deref().unwrap(),
                self.control
            ),
            Kind::Omitted => OMITTED.into(),
        };
        DiagnosticNote {
            span: self.span,
            path: None,
            message,
        }
    }
    pub(crate) fn omitted(&self) -> Self {
        Self {
            kind: Kind::Omitted,
            ..self.clone()
        }
    }
}
pub(crate) fn append(notes: &mut Vec<Raw>, note: Raw) {
    if notes.contains(&note) {
        return;
    }
    let omission = notes.iter().position(|n| n.kind == Kind::Omitted);
    if note.kind == Kind::Omitted {
        if omission.is_none() {
            notes.push(note);
        }
    } else if notes.len() - usize::from(omission.is_some()) < LIMIT {
        notes.insert(omission.unwrap_or(notes.len()), note);
    } else if omission.is_none() {
        notes.push(note.omitted());
    }
}

pub const FORMAT: &str = "tellur.outcome-absorption";
pub const VERSION: u32 = 1;
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_ABSORPTION", Span { line: 1, column: 1 }, message)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub module: String,
    pub line: usize,
    pub column: usize,
}

/// One source occurrence; a summary is a set of contributors, not an execution trace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contributor {
    pub kind: Kind,
    pub control: String,
    pub operation: Option<String>,
    pub outcome: String,
    pub site: String,
    pub location: Location,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub contributors: Vec<Contributor>,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// Generic templates need a concrete instantiation before their bodies have IR.
    pub generic_definition_only: bool,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub outcomes: BTreeMap<String, Evidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format: String,
    pub version: u32,
    pub analysis: Analysis,
    pub contracts: contracts::Snapshot,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub controls: BTreeMap<String, Control>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    pub profile: String,
    pub per_outcome_limit: usize,
    pub limitations: Vec<String>,
}
impl Default for Analysis {
    fn default() -> Self {
        Self {
            profile: "structural_source_contributors_v1".into(),
            per_outcome_limit: LIMIT,
            limitations: [
                "value_feasibility",
                "complete_execution_histories",
                "loop_carried_values",
                "indirect_storage",
                "dynamic_collections",
                "cross_call_payloads",
                "uninstantiated_generic_definitions",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Difference {
    pub format: &'static str,
    pub version: u32,
    pub public_contracts_equal: bool,
    pub recorded_evidence_equal: bool,
    pub truncated: bool,
    pub contract_changes: Vec<contracts::Change>,
    pub evidence_changes: Vec<contracts::Change>,
}
impl Snapshot {
    pub fn from_json(text: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(text)
            .map_err(|e| error(format!("invalid absorption snapshot: {e}")))?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(error("unsupported absorption snapshot format or version"));
        }
        if self.analysis != Analysis::default() {
            return Err(error("unsupported contributor analysis profile or limits"));
        }
        self.contracts.validate()?;
        for control in self.controls.values() {
            if control.generic_definition_only && !control.outcomes.is_empty() {
                return Err(error(
                    "generic definition cannot claim concrete contributor evidence",
                ));
            }
            for evidence in control.outcomes.values() {
                if evidence.contributors.len() > LIMIT {
                    return Err(error("contributor evidence exceeds its recorded limit"));
                }
                let mut keys = BTreeSet::new();
                for c in &evidence.contributors {
                    if c.kind == Kind::Omitted
                        || c.location.line == 0
                        || c.location.column == 0
                        || matches!(c.kind, Kind::Received | Kind::Forwarded)
                            != c.operation.is_some()
                        || !keys.insert(key(c))
                    {
                        return Err(error("invalid or duplicate contributor evidence"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn compare(&self, after: &Self) -> Result<Difference> {
        self.validate()?;
        after.validate()?;
        let contracts = self.contracts.compare(&after.contracts)?;
        let before_evidence = self.comparable();
        let after_evidence = after.comparable();
        let evidence_changes = contracts::changes(&before_evidence, &after_evidence);
        Ok(Difference {
            format: "tellur.outcome-absorption-diff",
            version: VERSION,
            public_contracts_equal: contracts.equal,
            recorded_evidence_equal: evidence_changes.is_empty(),
            truncated: [self, after].iter().any(|s| {
                s.controls
                    .values()
                    .any(|c| c.outcomes.values().any(|e| e.truncated))
            }),
            contract_changes: contracts.changes,
            evidence_changes,
        })
    }
    fn comparable(&self) -> serde_json::Value {
        // Locations assist navigation but do not make whitespace edits into evidence changes.
        let mut controls = BTreeMap::new();
        for (name, control) in &self.controls {
            let mut outcomes = BTreeMap::new();
            for (outcome, evidence) in &control.outcomes {
                let contributors: Vec<_> = evidence
                    .contributors
                    .iter()
                    .map(key)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|(kind, control, operation, outcome, site)| {
                        serde_json::json!({
                            "kind": kind, "control": control, "operation": operation,
                            "outcome": outcome, "site": site,
                        })
                    })
                    .collect();
                outcomes.insert(
                    outcome,
                    serde_json::json!({
                        "contributors": contributors, "truncated": evidence.truncated,
                    }),
                );
            }
            controls.insert(name, serde_json::json!({
                "generic_definition_only": control.generic_definition_only, "outcomes": outcomes,
            }));
        }
        serde_json::to_value(controls).unwrap()
    }
}
fn key(c: &Contributor) -> (Kind, &str, Option<&str>, &str, &str) {
    (
        c.kind,
        &c.control,
        c.operation.as_deref(),
        &c.outcome,
        &c.site,
    )
}

pub(crate) fn checked(
    program: &CheckedProgram,
    contracts: contracts::Snapshot,
) -> Result<Snapshot> {
    let sources = program.source_files();
    let base = sources[0].path.parent().unwrap();
    let mut modules = vec![];
    let mut names = BTreeMap::new();
    for (i, source) in sources.iter().enumerate() {
        let module = if i == 0 {
            "@entry".into()
        } else {
            format!("file:{}", contracts::relative(&source.path, base)?)
        };
        for (global, local) in &source.names {
            names.insert(global.as_str(), format!("{module}::{local}"));
        }
        modules.push(module);
    }
    fn source_name(value: &str, names: &BTreeMap<&str, String>) -> String {
        if let Some((application, member)) = crate::collection_support::qualified_member(value) {
            return format!("{}.{member}", source_name(application, names));
        }
        if let Some((base, arguments)) = crate::collection_support::generic(value) {
            return format!(
                "{}<{}>",
                source_name(base, names),
                arguments
                    .iter()
                    .map(|a| source_name(a, names))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        names
            .get(value)
            .cloned()
            .unwrap_or_else(|| crate::manifest::display_type(value))
    }
    let name = |value: &str| source_name(value, &names);
    fn spans(code: &[Instruction], result: &mut BTreeMap<(usize, usize), Option<String>>) {
        for i in code {
            if !matches!(
                i.action,
                Action::Emit(_) | Action::Drop(_) | Action::Release(_)
            ) {
                let label = result
                    .entry((i.provenance.span.line, i.provenance.span.column))
                    .or_default();
                if i.provenance.site.is_some() {
                    *label = i.provenance.site.clone();
                }
            }
            if let Action::Loop { body, .. } = &i.action {
                spans(body, result);
            }
        }
    }
    let sites: BTreeMap<_, _> = program
        .regions()
        .map(|r| {
            let mut locations = BTreeMap::new();
            spans(&r.code, &mut locations);
            (
                r.name.as_str(),
                locations
                    .into_iter()
                    .enumerate()
                    .map(|(i, (span, label))| {
                        (
                            span,
                            match label {
                                Some(label) => format!("{label}@source:{}", i + 1),
                                None => format!("source:{}", i + 1),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect();
    let mut controls = BTreeMap::new();
    for (module, contents) in &contracts.modules {
        for (local, declaration) in &contents.declarations {
            let contracts::Declaration::Control(control) = declaration else {
                continue;
            };
            let public_name = format!("{module}::{local}");
            if !control.type_parameters.is_empty() {
                controls.insert(
                    public_name,
                    Control {
                        generic_definition_only: true,
                        outcomes: BTreeMap::new(),
                    },
                );
                continue;
            }
            let internal = names
                .iter()
                .find(|(_, n)| **n == public_name)
                .map(|(n, _)| *n)
                .ok_or_else(|| error("public control has no source identity"))?;
            let raw = program
                .contributors
                .get(internal)
                .ok_or_else(|| error("public control has no checked contributor summary"))?;
            let mut outcomes = BTreeMap::new();
            for (outcome, notes) in raw {
                let mut evidence = Evidence {
                    contributors: vec![],
                    truncated: false,
                };
                for note in notes {
                    if note.kind == Kind::Omitted {
                        evidence.truncated = true;
                        continue;
                    }
                    let index = sources
                        .iter()
                        .rposition(|s| s.start_line <= note.span.line)
                        .unwrap_or(0);
                    let site = if note.kind == Kind::Declared {
                        "declaration".into()
                    } else {
                        sites
                            .get(note.control.as_str())
                            .and_then(|m| m.get(&(note.span.line, note.span.column)))
                            .ok_or_else(|| error("contributor source site is missing"))?
                            .clone()
                    };
                    evidence.contributors.push(Contributor {
                        kind: note.kind,
                        control: name(&note.control),
                        operation: note.operation.as_deref().map(name),
                        outcome: note.outcome.clone(),
                        site,
                        location: Location {
                            module: modules[index].clone(),
                            line: note
                                .span
                                .line
                                .saturating_sub(sources[index].start_line - 1)
                                .max(1),
                            column: note.span.column.max(1),
                        },
                    });
                }
                evidence.contributors.sort_by(|a, b| key(a).cmp(&key(b)));
                outcomes.insert(outcome.clone(), evidence);
            }
            controls.insert(
                public_name,
                Control {
                    generic_definition_only: false,
                    outcomes,
                },
            );
        }
    }
    let result = Snapshot {
        format: FORMAT.into(),
        version: VERSION,
        analysis: Analysis::default(),
        contracts,
        controls,
    };
    result.validate()?;
    Ok(result)
}
