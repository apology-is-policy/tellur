//! Live fault-boundary identities, separate from invocations and diagnostic traces.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DomainId(u64);
#[derive(Clone, Copy, Debug)]
pub(super) struct Scope {
    pub id: DomainId,
    pub parent: Option<DomainId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Domain {
    parent: Option<DomainId>,
    stopped: bool,
}
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) struct Domains {
    next: u64,
    live: BTreeMap<DomainId, Domain>,
}
impl Domains {
    pub(super) fn prepare(&self, parent: Option<DomainId>) -> Result<Scope> {
        if let Some(parent) = parent {
            self.require_open(parent)?;
        }
        Ok(Scope {
            id: DomainId(
                self.next
                    .checked_add(1)
                    .ok_or_else(|| error("fault domain identity space exhausted"))?,
            ),
            parent,
        })
    }
    pub(super) fn commit(&mut self, scope: Scope) {
        assert_eq!(self.next.checked_add(1), Some(scope.id.0));
        self.next = scope.id.0;
        assert!(
            self.live
                .insert(
                    scope.id,
                    Domain {
                        parent: scope.parent,
                        stopped: false
                    }
                )
                .is_none()
        );
    }
    pub(super) fn require_open(&self, mut id: DomainId) -> Result<()> {
        loop {
            let domain = self
                .live
                .get(&id)
                .ok_or_else(|| error("fault domain has retired"))?;
            if domain.stopped {
                return Err(error("fault domain has stopped normal execution"));
            }
            match domain.parent {
                Some(parent) => id = parent,
                None => return Ok(()),
            }
        }
    }
    pub(super) fn stop(&mut self, id: DomainId) -> Result<()> {
        self.live
            .get_mut(&id)
            .ok_or_else(|| error("fault domain has retired"))?
            .stopped = true;
        Ok(())
    }
    pub(super) fn require_no_children(&self, id: DomainId) -> Result<()> {
        if !self.live.contains_key(&id) {
            return Err(error("fault domain has retired"));
        }
        if self.live.values().any(|domain| domain.parent == Some(id)) {
            return Err(error("fault domain still has live nested boundaries"));
        }
        Ok(())
    }
    pub(super) fn close(&mut self, scope: Scope) -> Result<()> {
        self.require_no_children(scope.id)?;
        if self.live[&scope.id].parent != scope.parent {
            return Err(error("fault domain parent differs"));
        }
        self.live.remove(&scope.id);
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn count(&self) -> usize {
        self.live.len()
    }
    #[cfg(test)]
    pub(super) fn exhaust(&mut self) {
        self.next = u64::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(d: &mut Domains, parent: Option<DomainId>) -> Scope {
        let scope = d.prepare(parent).unwrap();
        d.commit(scope);
        scope
    }
    #[test]
    fn stopped_domains_gate_descendants_without_stopping_independent_roots() {
        let mut d = Domains::default();
        let root = open(&mut d, None);
        let child = open(&mut d, Some(root.id));
        let other = open(&mut d, None);
        d.stop(root.id).unwrap();
        let before = d.clone();
        assert!(d.require_open(child.id).is_err());
        assert!(d.prepare(Some(child.id)).is_err());
        assert!(d.close(root).is_err());
        assert_eq!(d, before);
        d.require_open(other.id).unwrap();
        d.close(child).unwrap();
        d.close(root).unwrap();
        d.close(other).unwrap();
        assert_eq!(d.count(), 0);
        assert!(d.require_open(root.id).is_err());
    }
    #[test]
    fn handled_child_does_not_reopen_itself_or_poison_its_parent() {
        let mut d = Domains::default();
        let root = open(&mut d, None);
        let child = open(&mut d, Some(root.id));
        d.stop(child.id).unwrap();
        d.stop(child.id).unwrap();
        assert!(d.require_open(child.id).is_err());
        d.require_open(root.id).unwrap();
        d.close(child).unwrap();
        let replacement = open(&mut d, Some(root.id));
        assert!(replacement.id.0 > child.id.0);
        let before = d.clone();
        assert!(d.close(child).is_err());
        assert_eq!(d, before);
        d.exhaust();
        let before = d.clone();
        assert!(d.prepare(Some(root.id)).is_err());
        assert_eq!(d, before);
        d.close(replacement).unwrap();
        d.close(root).unwrap();
        assert_eq!(d.count(), 0);
    }
}
