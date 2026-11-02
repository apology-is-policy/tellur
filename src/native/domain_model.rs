//! Test-only native fault-domain identity and lexical lifetime model.
use std::{collections::BTreeMap, sync::Arc};

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug)]
pub struct DomainKey {
    registry: Arc<()>,
    id: u64,
}
#[derive(Clone, Debug)]
pub struct DomainScope {
    pub key: DomainKey,
    pub parent: Option<DomainKey>,
}
#[derive(Default)]
pub struct Domains {
    identity: Arc<()>,
    next: u64,
    live: BTreeMap<u64, Option<u64>>,
}
impl Domains {
    pub fn prepare(&self, parent: Option<&DomainKey>) -> Result<DomainScope> {
        if let Some(parent) = parent {
            self.require(parent)?;
        }
        let id = self
            .next
            .checked_add(1)
            .ok_or("E_NATIVE: fault domain identity exhausted")?;
        Ok(DomainScope {
            key: DomainKey {
                registry: self.identity.clone(),
                id,
            },
            parent: parent.cloned(),
        })
    }
    pub fn commit(&mut self, scope: &DomainScope) -> Result<()> {
        if !Arc::ptr_eq(&scope.key.registry, &self.identity)
            || self.next.checked_add(1) != Some(scope.key.id)
        {
            return Err("E_NATIVE: stale or foreign fault domain admission".into());
        }
        if let Some(parent) = &scope.parent {
            self.require(parent)?;
        }
        self.live
            .insert(scope.key.id, scope.parent.as_ref().map(|key| key.id));
        self.next = scope.key.id;
        Ok(())
    }
    pub fn require(&self, key: &DomainKey) -> Result<()> {
        if !Arc::ptr_eq(&key.registry, &self.identity) || !self.live.contains_key(&key.id) {
            return Err("E_NATIVE: fault domain is foreign or retired".into());
        }
        Ok(())
    }
    pub fn require_member(&self, owner: &DomainKey, current: &DomainKey) -> Result<()> {
        self.require(owner)?;
        self.require(current)?;
        let mut id = current.id;
        loop {
            if id == owner.id {
                return Ok(());
            }
            id = self.live[&id].ok_or("E_NATIVE: current fault domain belongs to another owner")?;
        }
    }
    pub fn require_selected(&self, scope: &DomainScope, current: &DomainKey) -> Result<()> {
        self.require(&scope.key)?;
        self.require(current)?;
        if let Some(parent) = &scope.parent {
            self.require(parent)?;
        }
        if current.id != scope.key.id
            || self.live[&scope.key.id] != scope.parent.as_ref().map(|key| key.id)
        {
            return Err("E_NATIVE: fault domain is not the selected scope".into());
        }
        Ok(())
    }
    pub fn require_quiescent(&self, scope: &DomainScope, current: &DomainKey) -> Result<()> {
        self.require_selected(scope, current)?;
        if self
            .live
            .values()
            .any(|parent| *parent == Some(scope.key.id))
        {
            return Err("E_NATIVE: fault domain still has live nested boundaries".into());
        }
        Ok(())
    }
    pub fn close(&mut self, scope: &DomainScope, current: &DomainKey) -> Result<()> {
        self.require_quiescent(scope, current)?;
        self.live.remove(&scope.key.id);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.live.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(domains: &mut Domains, parent: Option<&DomainKey>) -> DomainScope {
        let scope = domains.prepare(parent).unwrap();
        domains.commit(&scope).unwrap();
        scope
    }

    #[test]
    fn domain_membership_and_retirement_preserve_independent_roots() {
        let mut domains = Domains::default();
        let first = open(&mut domains, None);
        let child = open(&mut domains, Some(&first.key));
        let other = open(&mut domains, None);
        domains.require_member(&first.key, &child.key).unwrap();
        assert!(domains.require_selected(&child, &first.key).is_err());
        assert!(domains.require_member(&other.key, &child.key).is_err());
        assert!(domains.close(&first, &first.key).is_err());
        assert!(domains.close(&child, &other.key).is_err());
        assert_eq!(domains.len(), 3);
        domains.close(&child, &child.key).unwrap();
        domains.close(&first, &first.key).unwrap();
        assert!(domains.require_member(&first.key, &child.key).is_err());
        assert!(domains.close(&child, &child.key).is_err());
        domains.require(&other.key).unwrap();
        domains.close(&other, &other.key).unwrap();
        assert_eq!(domains.len(), 0);
    }

    #[test]
    fn foreign_forged_and_stale_domain_transitions_reject_before_mutation() {
        let mut domains = Domains::default();
        let first = open(&mut domains, None);
        let mut foreign = Domains::default();
        let foreign_root = open(&mut foreign, None);
        assert_eq!(first.key.id, foreign_root.key.id);
        assert!(domains.prepare(Some(&foreign_root.key)).is_err());
        assert!(
            domains
                .require_member(&first.key, &foreign_root.key)
                .is_err()
        );
        assert!(domains.close(&foreign_root, &first.key).is_err());
        let stale = domains.prepare(Some(&first.key)).unwrap();
        let other = open(&mut domains, None);
        assert!(domains.commit(&stale).is_err());
        let child = open(&mut domains, Some(&first.key));
        let mut forged = child.clone();
        forged.parent = Some(other.key.clone());
        assert!(domains.close(&forged, &child.key).is_err());
        forged.parent = Some(foreign_root.key);
        assert!(domains.close(&forged, &child.key).is_err());
        assert_eq!(domains.len(), 3);
        domains.close(&child, &child.key).unwrap();
        domains.close(&first, &first.key).unwrap();
        domains.close(&other, &other.key).unwrap();
        let prepared = domains.prepare(None).unwrap();
        assert!(foreign.commit(&prepared).is_err());
    }

    #[test]
    fn repeated_domain_lifetimes_are_bounded_and_exhaustion_is_atomic() {
        let mut domains = Domains::default();
        let mut last = 0;
        for _ in 0..64 {
            let root = open(&mut domains, None);
            let child = open(&mut domains, Some(&root.key));
            assert!(root.key.id > last);
            last = child.key.id;
            domains.close(&child, &child.key).unwrap();
            domains.close(&root, &root.key).unwrap();
            assert_eq!(domains.len(), 0);
        }
        let root = open(&mut domains, None);
        domains.next = u64::MAX;
        let before = domains.live.clone();
        assert!(domains.prepare(None).is_err());
        assert!(domains.prepare(Some(&root.key)).is_err());
        assert_eq!(domains.live, before);
        domains.close(&root, &root.key).unwrap();
        assert_eq!(domains.len(), 0);
    }
}
