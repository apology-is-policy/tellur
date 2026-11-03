//! Conservative capture descriptions. Unknown metadata never changes execution.
use super::access_resolution::StoragePlace;
use super::reference_evidence::Origins;
use super::{Frames, R, V};

const LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub place: StoragePlace,
    pub mutable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dependencies {
    /// Resolved runtime graph plus explicitly incomplete checked origins.
    /// Neither part grants permission; symbolic origin paths are not accesses.
    Partial {
        reachable: Vec<Dependency>,
        origins: Origins,
    },
    /// Complete conservative dependency coverage for the supported checked forms,
    /// still requiring invocation/domain authority and access interference checks.
    Complete {
        reachable: Vec<Dependency>,
        origins: Origins,
    },
    Unknown(String),
}
impl Dependencies {
    fn resolved(reachable: Vec<Dependency>, origins: Origins) -> Self {
        if origins.is_complete() {
            Self::Complete { reachable, origins }
        } else {
            Self::Partial { reachable, origins }
        }
    }
    pub fn describe(values: &Frames, captures: &[V]) -> Self {
        match describe(values, captures, LIMIT) {
            Ok((reachable, origins)) => Self::resolved(reachable, origins),
            Err(message) => Self::Unknown(message),
        }
    }
    /// Allocation liveness only, not current holder meaning or access permission.
    pub fn require_live(&self, values: &Frames) -> R<()> {
        let (Self::Partial {
            reachable: dependencies,
            origins,
        }
        | Self::Complete {
            reachable: dependencies,
            origins,
        }) = self
        else {
            return Err("E_NATIVE: capture dependencies are unresolved".into());
        };
        for dependency in dependencies {
            values.require_storage(&dependency.place.storage)?;
        }
        origins.require_live(values)
    }
    /// Retain every input dependency through result ownership. Do not infer a
    /// narrower result-origin contract from dynamic values alone.
    pub(super) fn extend(&mut self, other: Self) {
        let old = std::mem::replace(self, Self::Unknown("dependency merge in progress".into()));
        *self = match (old, other) {
            (Self::Unknown(message), _) | (_, Self::Unknown(message)) => Self::Unknown(message),
            (
                Self::Partial {
                    reachable: mut entries,
                    mut origins,
                }
                | Self::Complete {
                    reachable: mut entries,
                    mut origins,
                },
                Self::Partial {
                    reachable: other,
                    origins: other_origins,
                }
                | Self::Complete {
                    reachable: other,
                    origins: other_origins,
                },
            ) => {
                origins.extend(other_origins);
                for dependency in other {
                    if let Some(old) = entries.iter_mut().find(|d| d.place == dependency.place) {
                        old.mutable |= dependency.mutable;
                    } else {
                        entries.push(dependency);
                    }
                }
                if entries
                    .iter()
                    .map(|d| 1 + d.place.fields.len())
                    .sum::<usize>()
                    > LIMIT
                {
                    Self::Unknown("capture dependency limit exceeded".into())
                } else {
                    Self::resolved(entries, origins)
                }
            }
        };
    }
}

fn describe(values: &Frames, captures: &[V], limit: usize) -> R<(Vec<Dependency>, Origins)> {
    if captures.len() > limit {
        return Err("capture dependency limit exceeded".into());
    }
    // Iterative traversal bounds stack use. Revisited referents are expanded only
    // when new exclusive authority strengthens a previous shared description.
    let mut work: Vec<_> = captures.iter().rev().map(|v| (v, true)).collect();
    let mut entries: Vec<Dependency> = Vec::new();
    let mut remaining = limit;
    let mut origins = Origins::Complete(vec![]);
    while let Some((value, authority)) = work.pop() {
        remaining = remaining
            .checked_sub(1)
            .ok_or("capture dependency limit exceeded")?;
        match value {
            V::Reference {
                ty,
                root,
                fields,
                evidence,
            } => {
                remaining = remaining
                    .checked_sub(evidence.origins.cost())
                    .ok_or("captured checked origin limit exceeded")?;
                origins.extend(evidence.origins.clone());
                if evidence.target.is_none() {
                    origins.extend(Origins::default());
                }
                let mutable = if let Some(inner) = ty.strip_prefix("&mutable ") {
                    if inner.is_empty() {
                        return Err("invalid captured reference type".into());
                    }
                    authority
                } else if let Some(inner) = ty.strip_prefix('&') {
                    if inner.is_empty() {
                        return Err("invalid captured reference type".into());
                    }
                    false
                } else {
                    return Err("invalid captured reference type".into());
                };
                remaining = remaining
                    .checked_sub(fields.len())
                    .ok_or("capture dependency path limit exceeded")?;
                let place = if let Some(target) = &evidence.target {
                    if values.require_storage(&target.storage)? != *root || target.fields != *fields
                    {
                        return Err(
                            "captured reference does not match its formation identity".into()
                        );
                    }
                    target.clone()
                } else {
                    StoragePlace {
                        storage: values.storage_at(*root)?,
                        fields: fields.clone(),
                    }
                };
                let referent = place.value(values)?;
                if let Some(old) = entries.iter_mut().find(|d| d.place == place) {
                    if old.mutable || !mutable {
                        continue;
                    }
                    old.mutable = true;
                } else {
                    entries.push(Dependency { place, mutable });
                }
                if work.len() >= remaining {
                    return Err("capture dependency limit exceeded".into());
                }
                work.push((referent, mutable));
            }
            V::Construct { fields, .. }
            | V::Task {
                arguments: fields, ..
            } => {
                if fields.len() > remaining.saturating_sub(work.len()) {
                    return Err("capture dependency limit exceeded".into());
                }
                work.extend(fields.iter().rev().map(|v| (v, authority)));
            }
            _ => {}
        }
    }
    Ok((entries, origins))
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
