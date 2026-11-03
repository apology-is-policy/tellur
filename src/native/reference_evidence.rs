//! Bound checked origins. Even complete coverage grants no execution permission.
use super::access_resolution::StoragePlace;
use super::{Frames, R, V};

pub(super) const LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundOrigin {
    /// Only the allocation is resolved. Fields remain symbolic checked paths;
    /// they may describe abstract tagged storage and must not be dereferenced.
    pub place: StoragePlace,
    pub guard: bool,
    pub opaque: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origins {
    Partial(Vec<BoundOrigin>),
    Complete(Vec<BoundOrigin>),
    Unavailable(String),
}
impl Default for Origins {
    fn default() -> Self {
        Self::Partial(vec![])
    }
}
impl Origins {
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }
    pub(crate) fn extend(&mut self, other: Self) {
        let complete = self.is_complete() && other.is_complete();
        let old = std::mem::take(self);
        *self = match (old, other) {
            (Self::Unavailable(message), _) | (_, Self::Unavailable(message)) => {
                Self::Unavailable(message)
            }
            (
                Self::Partial(mut entries) | Self::Complete(mut entries),
                Self::Partial(other) | Self::Complete(other),
            ) => {
                for origin in other {
                    if !entries.contains(&origin) {
                        entries.push(origin);
                    }
                }
                if complete {
                    Self::Complete(entries)
                } else {
                    Self::Partial(entries)
                }
            }
        };
        self.bound();
    }
    fn bound(&mut self) {
        if let Self::Partial(entries) | Self::Complete(entries) = self {
            if entries
                .iter()
                .try_fold(0usize, |n, o| n.checked_add(1 + o.place.fields.len()))
                .is_none_or(|n| n > LIMIT)
            {
                *self = Self::Unavailable("checked origin limit exceeded".into());
            }
        }
    }
    pub(super) fn cost(&self) -> usize {
        match self {
            Self::Partial(entries) | Self::Complete(entries) => {
                entries.iter().fold(0usize, |n, o| {
                    n.saturating_add(1).saturating_add(o.place.fields.len())
                })
            }
            Self::Unavailable(_) => 1,
        }
    }
    pub(super) fn require_live(&self, values: &Frames) -> R<()> {
        let (Self::Partial(entries) | Self::Complete(entries)) = self else {
            return Err("E_NATIVE: checked origins are unavailable".into());
        };
        for origin in entries {
            values.require_storage(&origin.place.storage)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceEvidence {
    /// Snapshot at formation, never reconstructed after allocation reuse.
    pub target: Option<StoragePlace>,
    pub origins: Origins,
}
impl ReferenceEvidence {
    pub fn projected(&self, fields: &[usize]) -> Self {
        let mut evidence = self.clone();
        if let Some(target) = &mut evidence.target {
            if target.fields.len().saturating_add(fields.len()) > LIMIT {
                evidence.target = None;
                evidence.origins = Origins::Unavailable("reference path limit exceeded".into());
            } else {
                target.fields.extend_from_slice(fields);
            }
        }
        evidence
    }
    pub fn from_place(values: &Frames, root: usize, fields: &[usize], indirect: bool) -> Self {
        if indirect {
            if let Some(Some(V::Reference {
                evidence,
                root: address,
                fields: path,
                ..
            })) = values.get(root)
            {
                if evidence.target.as_ref().is_some_and(|target| {
                    values.require_storage(&target.storage).ok() == Some(*address)
                        && target.fields == *path
                }) {
                    return evidence.projected(fields);
                }
            }
            return Self {
                target: None,
                origins: Origins::Unavailable("indirect origin holder is unavailable".into()),
            };
        }
        match values.storage_at(root) {
            Ok(storage) => Self {
                target: Some(StoragePlace {
                    storage,
                    fields: vec![],
                }),
                origins: Origins::default(),
            }
            .projected(fields),
            Err(_) => Self {
                target: None,
                origins: Origins::Unavailable(
                    "reference formation allocation is unavailable".into(),
                ),
            },
        }
    }
}

pub struct Origin {
    pub root: usize,
    pub fields: &'static [usize],
    pub indirect: bool,
    pub guard: bool,
    pub opaque: bool,
}
/// Attach metadata after evaluating the reference operand exactly once. Failures
/// affect descriptions only; no source error, value clone or disposition occurs.
pub fn attach(values: &Frames, value: V, origins: &[Origin]) -> V {
    attach_checked(values, value, origins, false, false)
}
/// Preserve, but never manufacture, the evidence carried by a copied input leaf.
/// Invalid metadata makes descriptions unavailable without changing evaluation.
pub fn inherit_checked(values: &Frames, mut value: V) -> V {
    if let V::Reference {
        root,
        fields,
        evidence,
        ..
    } = &mut value
    {
        let valid = evidence.target.as_ref().is_some_and(|target| {
            target.fields.len() <= LIMIT
                && target.fields == *fields
                && values.require_storage(&target.storage).ok() == Some(*root)
        });
        evidence.origins.bound();
        if !valid || evidence.origins.require_live(values).is_err() {
            evidence.origins =
                Origins::Unavailable("incoming reference evidence is unavailable".into());
        }
    }
    value
}
/// `fresh` is emitted only for a direct reference operand, whose local dependencies
/// are fully described by its annotation. Reborrows and received values must merge
/// their incoming evidence, so a callee annotation cannot promote a partial caller.
pub fn attach_checked(
    values: &Frames,
    mut value: V,
    origins: &[Origin],
    complete: bool,
    fresh: bool,
) -> V {
    if let V::Reference { evidence, .. } = &mut value {
        // The supported static guarded closure is a fresh local formation. An
        // incoming value must not be promoted by a guarded callee annotation.
        let complete = complete && (fresh || !origins.iter().any(|o| o.guard));
        let bound = bind(values, origins, complete).unwrap_or_else(Origins::Unavailable);
        if complete && fresh {
            evidence.origins = bound;
        } else {
            evidence.origins.extend(bound);
        }
    }
    value
}
/// Attach a checked exclusive input relationship after exactly-once operand
/// evaluation. Validate the original private leaf against the real holder field,
/// then retain the entire holder graph. Rejections affect metadata only.
pub fn attach_guarded_input(
    values: &Frames,
    mut value: V,
    origins: &[Origin],
    leaf: usize,
    holder: usize,
    fields: &[usize],
) -> V {
    let bound = guarded_input(values, &value, origins, leaf, holder, fields)
        .unwrap_or_else(Origins::Unavailable);
    if let V::Reference { evidence, .. } = &mut value {
        evidence.origins.extend(bound);
    }
    value
}
fn formed_target<'a>(values: &Frames, value: &'a V) -> R<&'a StoragePlace> {
    let V::Reference {
        root,
        fields,
        evidence,
        ..
    } = value
    else {
        return Err("guarded input requires a reference".into());
    };
    if evidence.origins.cost() > LIMIT {
        return Err("guarded input origin limit exceeded".into());
    }
    evidence.origins.require_live(values)?;
    let target = evidence
        .target
        .as_ref()
        .ok_or("guarded input formation is unavailable")?;
    if fields.len() > LIMIT
        || target.fields != *fields
        || values.require_storage(&target.storage)? != *root
    {
        return Err("guarded input does not match its formation identity".into());
    }
    Ok(target)
}
fn guarded_input(
    values: &Frames,
    value: &V,
    origins: &[Origin],
    leaf: usize,
    holder: usize,
    fields: &[usize],
) -> R<Origins> {
    if fields.is_empty() || fields.len() > LIMIT || origins.len() > LIMIT {
        return Err("unsupported guarded input path".into());
    }
    let get = |slot| {
        values
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or_else(|| "guarded input storage is unavailable".to_string())
    };
    let incoming = get(leaf)?;
    let aggregate = get(holder)?;
    if !matches!(incoming, V::Reference {ty, ..} if ty.starts_with("&mutable "))
        || !matches!(aggregate, V::Reference {ty, ..} if ty.starts_with("&mutable "))
    {
        return Err("guarded input requires exclusive leaf and holder".into());
    }
    let original = formed_target(values, incoming)?;
    let base = formed_target(values, aggregate)?;
    if base.fields.len().saturating_add(fields.len()) > LIMIT {
        return Err("guarded input path limit exceeded".into());
    }
    let mut field = base.clone();
    field.fields.extend_from_slice(fields);
    let stored = field.value(values)?;
    if !matches!((stored,incoming), (V::Reference{ty:a,..},V::Reference{ty:b,..}) if a==b)
        || formed_target(values, stored)? != original
    {
        return Err("guarded input field no longer matches its original leaf".into());
    }
    let selected = formed_target(values, value)?;
    if selected.storage != original.storage || !selected.fields.starts_with(&original.fields) {
        return Err("guarded input operand is outside its original leaf".into());
    }
    // The descriptor is separate from symbolic origins. Both the exact field
    // guard and whole aggregate coverage must remain explicit in the annotation.
    let has = |slot, path: &[usize], guard| {
        origins.iter().any(|o| {
            o.root == slot
                && o.indirect
                && o.fields == path
                && o.guard == guard
                && o.opaque == guard
        })
    };
    if !has(leaf, &[], false)
        || !has(holder, fields, true)
        || !has(holder, &[], false)
        || !has(holder, &[], true)
        || origins.iter().any(|o| {
            o.guard
                && !(o.indirect && o.root == holder && (o.fields.is_empty() || o.fields == fields))
        })
    {
        return Err("guarded input lacks exact field and broad holder coverage".into());
    }
    let mut bound = bind_origins(values, origins, true, Some((holder, fields)))?;
    bound.extend(super::dependencies::Dependencies::holder_origins(
        values, aggregate,
    ));
    if let V::Reference { evidence, .. } = incoming {
        bound.extend(evidence.origins.clone());
    }
    bound.require_live(values)?;
    Ok(bound)
}
fn bind(values: &Frames, origins: &[Origin], complete: bool) -> R<Origins> {
    bind_origins(values, origins, complete, None)
}
fn bind_origins(
    values: &Frames,
    origins: &[Origin],
    complete: bool,
    holder: Option<(usize, &[usize])>,
) -> R<Origins> {
    if complete
        && origins.iter().any(|o| {
            o.guard != o.opaque
                || (o.guard
                    && o.indirect
                    && !holder.is_some_and(|(root, fields)| {
                        o.root == root && (o.fields.is_empty() || o.fields == fields)
                    }))
        })
    {
        return Err("unsupported complete guarded origin".into());
    }
    let mut result = if complete {
        Origins::Complete(vec![])
    } else {
        Origins::default()
    };
    let mut remaining = LIMIT;
    for origin in origins {
        remaining = remaining
            .checked_sub(1 + origin.fields.len())
            .ok_or("checked origin limit exceeded")?;
        let mut evidence =
            ReferenceEvidence::from_place(values, origin.root, origin.fields, origin.indirect);
        let target = evidence
            .target
            .take()
            .ok_or("checked origin allocation is unavailable")?;
        values.require_storage(&target.storage)?;
        remaining = remaining
            .checked_sub(evidence.origins.cost())
            .ok_or("inherited checked origin limit exceeded")?;
        if origin.indirect {
            result.extend(evidence.origins);
        }
        let wrap = if complete {
            Origins::Complete
        } else {
            Origins::Partial
        };
        result.extend(wrap(vec![BoundOrigin {
            place: target,
            guard: origin.guard,
            opaque: origin.opaque,
        }]));
    }
    Ok(result)
}

#[cfg(test)]
#[path = "reference_evidence_tests.rs"]
mod tests;
