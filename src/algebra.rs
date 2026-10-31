//! Executable checks supporting the semantic draft. Boundary embeddings compare
//! outcome shapes only; scenario agreement is finite evidence, never a proof of
//! behavioral equivalence, refinement, divergence, or provenance preservation.
use crate::{
    CheckedProgram, Diagnostic, Result, Span,
    manifest::Outcome,
    runtime::{Execution, Scenario},
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundaryMismatch {
    Duplicate(String),
    Missing(String),
    Arity {
        outcome: String,
        source: usize,
        target: usize,
    },
    Type {
        outcome: String,
        field: usize,
        source: String,
        target: String,
    },
}
impl BoundaryMismatch {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Duplicate(_) | Self::Missing(_) => "E_OUTCOME",
            Self::Arity { .. } => "E_ARITY",
            Self::Type { .. } => "E_TYPE",
        }
    }
}
impl std::fmt::Display for BoundaryMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate(n) => write!(f, "duplicate outcome `{n}` in boundary"),
            Self::Missing(n) => write!(f, "outcome `{n}` is absent from the target contract"),
            Self::Arity {
                outcome,
                source,
                target,
            } => write!(
                f,
                "outcome `{outcome}` carries {source} fields, but the target requires {target}"
            ),
            Self::Type {
                outcome,
                field,
                source,
                target,
            } => write!(
                f,
                "outcome `{outcome}` payload {} requires {target}, found {source}",
                field + 1
            ),
        }
    }
}
impl std::error::Error for BoundaryMismatch {}

/// Exact, label-preserving inclusion of a checked outcome boundary. Field names
/// are documentary; field order and types (including reference modes) are exact.
/// No resource, lifetime, effect, nominal value conversion, or behavior proof is
/// implied by this shape check. The checker must validate each actual transfer.
pub fn outcome_embedding(
    source: &[Outcome],
    target: &[Outcome],
) -> std::result::Result<Vec<usize>, BoundaryMismatch> {
    for boundary in [source, target] {
        let mut names = BTreeSet::new();
        for o in boundary {
            if !names.insert(&o.name) {
                return Err(BoundaryMismatch::Duplicate(o.name.clone()));
            }
        }
    }
    source
        .iter()
        .map(|o| {
            let index = target
                .iter()
                .position(|t| t.name == o.name)
                .ok_or_else(|| BoundaryMismatch::Missing(o.name.clone()))?;
            let t = &target[index];
            if o.fields.len() != t.fields.len() {
                return Err(BoundaryMismatch::Arity {
                    outcome: o.name.clone(),
                    source: o.fields.len(),
                    target: t.fields.len(),
                });
            }
            for (i, (a, b)) in o.fields.iter().zip(&t.fields).enumerate() {
                if a.ty != b.ty {
                    return Err(BoundaryMismatch::Type {
                        outcome: o.name.clone(),
                        field: i,
                        source: a.ty.clone(),
                        target: b.ty.clone(),
                    });
                }
            }
            Ok(index)
        })
        .collect()
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct CaseDifference {
    pub scenario_index: usize,
    pub entry: String,
    pub before: Observation,
    pub after: Observation,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum Observation {
    Completed(Execution),
    Terminated(crate::runtime::ProcessTermination),
    Faulted {
        fault: String,
        events: Vec<crate::runtime::Event>,
    },
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScenarioAgreement {
    pub claim: &'static str,
    pub cases: usize,
    pub checked_entries: Vec<String>,
    pub step_limit_per_execution: usize,
    pub declared_outcome_shapes_equal: bool,
    pub fault_permissions_equal: bool,
    pub observations_match: bool,
    pub differences: Vec<CaseDifference>,
}
fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, Span::default(), message)
}
// Equal type names alone do not establish a shared type universe across source
// revisions. Conservatively compare reachable layout definitions as well.
fn common_type(
    a: &crate::manifest::Manifest,
    b: &crate::manifest::Manifest,
    ty: &str,
    seen: &mut BTreeSet<String>,
) -> bool {
    if !seen.insert(ty.into()) {
        return true;
    }
    if let Some((_, inner)) = crate::manifest::reference_inner(ty) {
        return common_type(a, b, inner, seen);
    }
    if let Some(inner) = crate::manifest::option_inner(ty) {
        return common_type(a, b, inner, seen);
    }
    if let Some(c) = crate::collection_support::Collection::parse(ty) {
        return c.types().iter().all(|t| common_type(a, b, t, seen));
    }
    if a.type_def(ty) != b.type_def(ty) {
        return false;
    }
    use crate::manifest::TypeDef;
    match a.type_def(ty) {
        Some(TypeDef::Record { fields, .. }) => {
            fields.iter().all(|f| common_type(a, b, &f.ty, seen))
        }
        Some(TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. }) => {
            constructors
                .iter()
                .flat_map(|o| &o.fields)
                .all(|f| common_type(a, b, &f.ty, seen))
        }
        _ => true,
    }
}
/// Compare complete observed executions for exactly the supplied inputs/scripts.
/// Invalid drivers, host/interpreter errors and exhausted fuel are inconclusive
/// and return Err, even if both programs would fail in the same way. Borrowed entry
/// parameters require an owning caller harness so mutations can be observed.
pub fn compare_scenarios(
    before: &CheckedProgram,
    after: &CheckedProgram,
    cases: &[Scenario],
    step_limit: usize,
) -> Result<ScenarioAgreement> {
    if cases.is_empty() || step_limit == 0 {
        return Err(error(
            "E_COMPARE",
            "comparison requires scenarios and a positive execution step limit",
        ));
    }
    let mut shapes_equal = true;
    let mut fault_permissions_equal = true;
    let mut entries = BTreeSet::new();
    let mut differences = vec![];
    for (index, scenario) in cases.iter().enumerate() {
        entries.insert(scenario.entry.clone());
        let a = before.region(&scenario.entry).ok_or_else(|| {
            error(
                "E_COMPARE_BOUNDARY",
                format!("before has no entry `{}`", scenario.entry),
            )
        })?;
        let b = after.region(&scenario.entry).ok_or_else(|| {
            error(
                "E_COMPARE_BOUNDARY",
                format!("after has no entry `{}`", scenario.entry),
            )
        })?;
        if a.parameters
            .iter()
            .map(|p| &p.ty)
            .ne(b.parameters.iter().map(|p| &p.ty))
            || a.parameters
                .iter()
                .any(|p| crate::manifest::reference_inner(&p.ty).is_some())
            || a.parameters.iter().any(|p| {
                !common_type(
                    before.manifest(),
                    after.manifest(),
                    &p.ty,
                    &mut BTreeSet::new(),
                )
            })
        {
            return Err(error(
                "E_COMPARE_BOUNDARY",
                "comparison requires matching owning entry parameter types and layouts; use an owning caller harness for borrowed controls",
            ));
        }
        shapes_equal &= outcome_embedding(&a.exits, &b.exits).is_ok()
            && outcome_embedding(&b.exits, &a.exits).is_ok();
        fault_permissions_equal &= a.faults == b.faults;
        shapes_equal &= a.exits.iter().flat_map(|o| &o.fields).all(|f| {
            common_type(
                before.manifest(),
                after.manifest(),
                &f.ty,
                &mut BTreeSet::new(),
            )
        });
        let execute = |p: &CheckedProgram, side: &str| match p.run_completion(scenario, step_limit)
        {
            Ok(crate::runtime::RunCompletion::Outcome(execution)) => {
                Ok(Observation::Completed(execution))
            }
            Ok(crate::runtime::RunCompletion::Process(result)) => {
                Ok(Observation::Terminated(result))
            }
            Err(failure) if failure.report.kind == crate::fault::Kind::SourceFault => {
                Ok(Observation::Faulted {
                    fault: failure.diagnostic.message,
                    events: failure.events,
                })
            }
            Err(failure) => Err(error(
                "E_COMPARE_INCONCLUSIVE",
                format!("scenario {index}, {side}: {}", failure.diagnostic),
            )),
        };
        let left = execute(before, "before")?;
        let right = execute(after, "after")?;
        if left != right {
            differences.push(CaseDifference {
                scenario_index: index,
                entry: scenario.entry.clone(),
                before: left,
                after: right,
            });
        }
    }
    Ok(ScenarioAgreement {
        claim: "agreement_on_supplied_scenarios_only",
        cases: cases.len(),
        checked_entries: entries.into_iter().collect(),
        step_limit_per_execution: step_limit,
        declared_outcome_shapes_equal: shapes_equal,
        fault_permissions_equal,
        observations_match: differences.is_empty(),
        differences,
    })
}
