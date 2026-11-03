//! Immutable storage resolution, not captured-loan analysis or selection permission.
use super::super::frames::Storage;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoragePlace {
    pub storage: Storage,
    pub fields: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAccess {
    pub place: StoragePlace,
    pub kind: AccessKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPlan {
    pub transition: Transition,
    pub accesses: Vec<ResolvedAccess>,
    pub delivery: Vec<ResolvedAccess>,
    pub opaque: bool,
}

impl StoragePlace {
    /// Validate this path without cloning values or following contained references.
    /// Whole-slot writes/retirement may address uninitialized storage. Other paths
    /// must already exist, including the parent aggregate of a field write.
    pub(super) fn require(&self, values: &Frames, kind: AccessKind) -> R<()> {
        values.require_storage(&self.storage)?;
        if self.fields.is_empty() && matches!(kind, AccessKind::Write | AccessKind::Retire) {
            return Ok(());
        }
        self.value(values).map(|_| ())
    }
    pub(super) fn value<'a>(&self, values: &'a Frames) -> R<&'a V> {
        let root = values.require_storage(&self.storage)?;
        let mut value = values[root]
            .as_ref()
            .ok_or("E_NATIVE: access requires an initialized place")?;
        for index in &self.fields {
            let V::Construct { fields, .. } = value else {
                return Err("E_NATIVE: access path crosses a non-aggregate".into());
            };
            value = fields
                .get(*index)
                .ok_or("E_NATIVE: access field is outside its aggregate")?;
        }
        Ok(value)
    }
}

fn resolve(
    accesses: &[Access],
    values: &Frames,
    frame: &Frame,
    destination: Option<usize>,
) -> R<Vec<ResolvedAccess>> {
    resolve_checked(accesses, values, frame, destination, false)
}

fn resolve_checked(
    accesses: &[Access],
    values: &Frames,
    frame: &Frame,
    destination: Option<usize>,
    checked_references: bool,
) -> R<Vec<ResolvedAccess>> {
    let mut result = Vec::new();
    let mut remaining = 4096usize;
    for access in accesses {
        let storage = match access.slot {
            Some(slot) => values.local_storage(frame, slot)?,
            None => values.storage_at(
                destination.ok_or("E_NATIVE: access requires a constructor destination")?,
            )?,
        };
        let place = if access.indirect {
            let holder = StoragePlace {
                storage,
                fields: vec![],
            };
            holder.require(values, AccessKind::Read)?;
            let root = values.require_storage(&holder.storage)?;
            let Some(V::Reference {
                ty,
                root,
                fields,
                evidence,
            }) = &values[root]
            else {
                return Err("E_NATIVE: indirect access requires a reference holder".into());
            };
            if checked_references
                && !evidence.target.as_ref().is_some_and(|target| {
                    values.require_storage(&target.storage).ok() == Some(*root)
                        && target.fields == *fields
                })
            {
                return Err(
                    "E_NATIVE: callback reference formation identity is unavailable".into(),
                );
            }
            let mutable = if ty.starts_with("&mutable ") {
                true
            } else if ty.starts_with('&') {
                false
            } else {
                return Err("E_NATIVE: indirect access has an invalid reference type".into());
            };
            if !mutable
                && matches!(
                    access.kind,
                    AccessKind::Move
                        | AccessKind::Exclusive
                        | AccessKind::Write
                        | AccessKind::Retire
                )
            {
                return Err("E_NATIVE: indirect mutation requires an exclusive reference".into());
            }
            if checked_references {
                remaining = remaining
                    .checked_sub(
                        2usize
                            .saturating_add(fields.len())
                            .saturating_add(access.fields.len()),
                    )
                    .ok_or("E_NATIVE: resolved callback access query limit exceeded")?;
            }
            let mut path = fields.clone();
            path.extend_from_slice(access.fields);
            result.push(ResolvedAccess {
                place: holder,
                kind: AccessKind::Read,
            });
            StoragePlace {
                storage: values.storage_at(*root)?,
                fields: path,
            }
        } else {
            if checked_references {
                remaining = remaining
                    .checked_sub(1usize.saturating_add(access.fields.len()))
                    .ok_or("E_NATIVE: resolved callback access query limit exceeded")?;
            }
            StoragePlace {
                storage,
                fields: access.fields.to_vec(),
            }
        };
        place.require(values, access.kind)?;
        result.push(ResolvedAccess {
            place,
            kind: access.kind,
        });
    }
    Ok(result)
}

impl AccessPlan {
    fn resolve(
        &self,
        values: &Frames,
        frame: &Frame,
        destination: Option<usize>,
    ) -> R<ResolvedPlan> {
        // Validate even an empty/unknown plan against its actual activation.
        values.frame_values(frame)?;
        Ok(ResolvedPlan {
            transition: self.transition,
            accesses: resolve(self.accesses, values, frame, destination)?,
            delivery: resolve(self.delivery, values, frame, destination)?,
            opaque: self.opaque,
        })
    }
}
impl Machine {
    /// Fresh query-only resolution; legacy host references without a formation
    /// identity cannot prove disjointness after slot reuse.
    pub(super) fn resolve_interference_access(&self, values: &Frames) -> R<ResolvedPlan> {
        let frame = self
            .stack
            .last()
            .ok_or("E_NATIVE: query has no live invocation")?;
        values.frame_values(&frame.frame)?;
        let plan = self.next_access();
        if plan.opaque
            || !matches!(
                plan.transition,
                Transition::Skip | Transition::Local | Transition::Delivery
            )
            || !plan.delivery.is_empty()
        {
            return Err("E_NATIVE: callback transition has no complete local access plan".into());
        }
        if plan
            .accesses
            .iter()
            .try_fold(0usize, |n, a| n.checked_add(1 + a.fields.len()))
            .is_none_or(|n| n > 4096)
        {
            return Err("E_NATIVE: callback access query limit exceeded".into());
        }
        Ok(ResolvedPlan {
            transition: plan.transition,
            accesses: resolve_checked(
                plan.accesses,
                values,
                &frame.frame,
                frame.destination,
                true,
            )?,
            delivery: vec![],
            opaque: false,
        })
    }
    /// Resolve the current symbolic plan against live storage without evaluating
    /// source expressions. Requery after state changes: allocation liveness alone
    /// does not prove a reference holder or field path still has the same meaning.
    pub fn resolve_access(&self, values: &Frames) -> R<ResolvedPlan> {
        let frame = self
            .stack
            .last()
            .ok_or("E_NATIVE: access query has no live invocation")?;
        self.next_access()
            .resolve(values, &frame.frame, frame.destination)
    }
}

#[cfg(test)]
#[path = "access_resolution_tests.rs"]
mod tests;
