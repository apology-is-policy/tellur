//! Stable public interfaces and bounded diagnostic contributors for direct forwarding.
use super::*;
use crate::DiagnosticNote;
const NOTE_LIMIT: usize = 32;

pub(super) fn public_shape(region: &syntax::Region) -> Result<()> {
    if region.public && (region.infer_exits || !region.infer_payloads.is_empty()) {
        return Err(Diagnostic::new(
            "E_PUBLIC_CONTRACT",
            region.name_span,
            "public controls require explicit exit labels and payload types or a named outcome contract; keep exits auto and label... on private controls",
        ));
    }
    Ok(())
}
pub(super) fn public_origins(
    region: &syntax::Region,
    schemas: &crate::origins::Schemas,
) -> Result<()> {
    if !region.public {
        return Ok(());
    }
    for outcome in &region.exits {
        for (index, field) in outcome.fields.iter().enumerate() {
            let reference = manifest::reference_inner(&field.ty);
            if reference.is_some()
                && !region
                    .return_origins
                    .contains_key(&(outcome.name.clone(), index))
            {
                return Err(Diagnostic::new(
                    "E_PUBLIC_CONTRACT",
                    region.name_span,
                    format!(
                        "public borrowed result `{}` requires an explicit from input origin",
                        field.name
                    ),
                ));
            }
            let ty = reference.map_or(field.ty.as_str(), |(_, t)| t);
            if schemas.get(ty).is_some_and(|s| !s.leaves.is_empty()) && field.origins.is_empty() {
                return Err(Diagnostic::new(
                    "E_PUBLIC_CONTRACT",
                    region.name_span,
                    format!(
                        "public aggregate result `{}` requires explicit bracket origin bindings",
                        field.name
                    ),
                ));
            }
        }
    }
    Ok(())
}
fn append(notes: &mut Vec<DiagnosticNote>, note: DiagnosticNote) {
    if notes.contains(&note) {
        return;
    }
    if notes.len() < NOTE_LIMIT {
        notes.push(note);
    } else if notes.len() == NOTE_LIMIT {
        notes.push(DiagnosticNote {
            message: "additional outcome contributors omitted (diagnostic limit)".into(),
            ..note
        });
    }
}
impl Checker<'_> {
    pub(super) fn record_contributor(&mut self, outcome: &str, span: Span) {
        let notes = self
            .forwarding
            .as_ref()
            .and_then(|f| f.get(outcome))
            .cloned()
            .unwrap_or_else(|| {
                vec![DiagnosticNote {
                    span,
                    path: None,
                    message: format!(
                        "outcome `{outcome}` is produced by `{}` here",
                        self.result.name
                    ),
                }]
            });
        let contributors = self.contributors.entry(outcome.into()).or_default();
        for note in notes {
            append(contributors, note);
        }
    }
    pub(super) fn forwarded_contributors(
        &self,
        callee: &str,
        signature: &Signature,
        span: Span,
    ) -> BTreeMap<String, Vec<DiagnosticNote>> {
        signature
            .outcomes
            .iter()
            .map(|outcome| {
                let mut notes = signature
                    .contributors
                    .get(&outcome.name)
                    .cloned()
                    .unwrap_or_default();
                append(
                    &mut notes,
                    DiagnosticNote {
                        span,
                        path: None,
                        message: format!(
                            "outcome `{}` is forwarded from `{callee}` by `{}` here",
                            outcome.name, self.result.name
                        ),
                    },
                );
                (outcome.name.clone(), notes)
            })
            .collect()
    }
    pub(super) fn with_contributors(
        &self,
        mut diagnostic: Diagnostic,
        signature: &Signature,
        outcomes: &[String],
    ) -> Diagnostic {
        for outcome in outcomes {
            for note in signature.contributors.get(outcome).into_iter().flatten() {
                append(&mut diagnostic.notes, note.clone());
            }
        }
        diagnostic
    }
}
