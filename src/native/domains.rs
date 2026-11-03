//! Native fault-domain identity, retained membership and lexical lifetime.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

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
#[derive(Clone, Debug)]
pub struct Membership {
    registry: Arc<()>,
    id: u64,
}
struct Member {
    owner: u64,
    parent: Option<u64>,
}
#[derive(Default)]
pub struct Domains {
    identity: Arc<()>,
    next: u64,
    live: BTreeMap<u64, Option<u64>>,
    stopped: BTreeSet<u64>,
    next_member: u64,
    members: BTreeMap<u64, Member>,
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
        self.require_quiescent_except(scope, current, None)
    }
    fn require_quiescent_except(
        &self,
        scope: &DomainScope,
        current: &DomainKey,
        member: Option<&Membership>,
    ) -> Result<()> {
        self.require_selected(scope, current)?;
        if self
            .live
            .values()
            .any(|parent| *parent == Some(scope.key.id))
        {
            return Err("E_NATIVE: fault domain still has live nested boundaries".into());
        }
        if self.members.iter().any(|(id, entry)| {
            entry.owner == scope.key.id && member.is_none_or(|member| member.id != *id)
        }) {
            return Err("E_NATIVE: fault domain still has retained contexts".into());
        }
        Ok(())
    }
    pub fn close(&mut self, scope: &DomainScope, current: &DomainKey) -> Result<()> {
        self.require_quiescent(scope, current)?;
        self.live.remove(&scope.key.id);
        self.stopped.remove(&scope.key.id);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.live.len()
    }
    pub fn same(&self, left: &DomainKey, right: &DomainKey) -> bool {
        Arc::ptr_eq(&left.registry, &right.registry) && left.id == right.id
    }
    pub fn same_membership(&self, left: &Membership, right: &Membership) -> bool {
        Arc::ptr_eq(&left.registry, &right.registry) && left.id == right.id
    }
    pub fn scope(&self, key: &DomainKey) -> Result<DomainScope> {
        self.require(key)?;
        Ok(DomainScope {
            key: key.clone(),
            parent: self.live[&key.id].map(|id| DomainKey {
                registry: self.identity.clone(),
                id,
            }),
        })
    }
    pub fn require_open(&self, key: &DomainKey, handling: &[&DomainKey]) -> Result<()> {
        self.require(key)?;
        let mut id = Some(key.id);
        while let Some(current) = id {
            if self.stopped.contains(&current)
                && !handling
                    .iter()
                    .any(|key| Arc::ptr_eq(&key.registry, &self.identity) && key.id == current)
            {
                return Err("E_NATIVE: fault domain has stopped normal execution".into());
            }
            id = self.live[&current];
        }
        Ok(())
    }
    pub fn stop(&mut self, key: &DomainKey) -> Result<()> {
        self.require(key)?;
        self.stopped.insert(key.id);
        Ok(())
    }
    pub fn require_member_capacity(&self) -> Result<()> {
        self.next_member
            .checked_add(1)
            .ok_or("E_NATIVE: context membership identity exhausted")?;
        Ok(())
    }
    pub fn attach(&mut self, owner: &DomainKey, parent: Option<&Membership>) -> Result<Membership> {
        self.require_open(owner, &[])?;
        self.require_member_capacity()?;
        if let Some(parent) = parent {
            let entry = self.member(parent)?;
            let key = DomainKey {
                registry: self.identity.clone(),
                id: entry.owner,
            };
            self.require_member(&key, owner)?;
        }
        self.next_member += 1;
        let token = Membership {
            registry: self.identity.clone(),
            id: self.next_member,
        };
        self.members.insert(
            token.id,
            Member {
                owner: owner.id,
                parent: parent.map(|parent| parent.id),
            },
        );
        Ok(token)
    }
    fn member(&self, member: &Membership) -> Result<&Member> {
        if !Arc::ptr_eq(&member.registry, &self.identity) {
            return Err("E_NATIVE: foreign context membership".into());
        }
        self.members
            .get(&member.id)
            .ok_or_else(|| "E_NATIVE: retired context membership".into())
    }
    pub fn require_membership(&self, member: &Membership, owner: &DomainKey) -> Result<()> {
        self.require(owner)?;
        if self.member(member)?.owner != owner.id {
            return Err("E_NATIVE: context membership has another owner".into());
        }
        Ok(())
    }
    /// Verify a parent-before-child context census against the independent
    /// membership registry. Missing, duplicate or reparented borrowers invalidate
    /// the observation; this never detaches or grants execution authority.
    pub fn require_descendants(
        &self,
        root: &Membership,
        edges: &[(&Membership, &Membership)],
    ) -> Result<()> {
        self.member(root)?;
        let mut seen = BTreeSet::from([root.id]);
        for (parent, child) in edges {
            self.member(parent)?;
            let entry = self.member(child)?;
            if !seen.contains(&parent.id)
                || entry.parent != Some(parent.id)
                || !seen.insert(child.id)
            {
                return Err("E_NATIVE: borrower census has a duplicate or mismatched edge".into());
            }
        }
        if self.members.iter().any(|(id, entry)| {
            entry.parent.is_some_and(|parent| seen.contains(&parent)) && !seen.contains(id)
        }) {
            return Err("E_NATIVE: borrower census omits a retained membership".into());
        }
        Ok(())
    }
    pub fn detach(&mut self, member: &Membership) -> Result<()> {
        self.require_no_children(member)?;
        self.members.remove(&member.id);
        Ok(())
    }
    pub fn has_children(&self, member: &Membership) -> Result<bool> {
        self.member(member)?;
        Ok(self
            .members
            .values()
            .any(|entry| entry.parent == Some(member.id)))
    }
    pub fn require_no_children(&self, member: &Membership) -> Result<()> {
        if self.has_children(member)? {
            return Err("E_NATIVE: context still has retained borrower children".into());
        }
        Ok(())
    }
    pub fn require_finish_root(
        &self,
        scope: &DomainScope,
        current: &DomainKey,
        member: &Membership,
    ) -> Result<()> {
        self.require_membership(member, &scope.key)?;
        self.require_quiescent_except(scope, current, Some(member))?;
        self.require_no_children(member)
    }
    pub fn finish_root(
        &mut self,
        scope: &DomainScope,
        current: &DomainKey,
        member: &Membership,
    ) -> Result<()> {
        self.require_finish_root(scope, current, member)?;
        self.detach(member)?;
        self.close(scope, current)
    }
    pub fn has_others(&self, domain: &DomainKey, member: &Membership) -> Result<bool> {
        self.require(domain)?;
        self.member(member)?;
        Ok(self.members.iter().any(|(id, entry)| {
            *id != member.id
                && self
                    .require_member(
                        domain,
                        &DomainKey {
                            registry: self.identity.clone(),
                            id: entry.owner,
                        },
                    )
                    .is_ok()
        }))
    }
    pub fn member_count(&self) -> usize {
        self.members.len()
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

    #[test]
    fn related_members_settle_before_owners_and_cannot_close_shared_domains() {
        let mut domains = Domains::default();
        let root = open(&mut domains, None);
        let parent = domains.attach(&root.key, None).unwrap();
        let boundary = open(&mut domains, Some(&root.key));
        let child = domains.attach(&boundary.key, Some(&parent)).unwrap();
        let grandchild = domains.attach(&boundary.key, Some(&child)).unwrap();
        assert!(domains.same(&domains.scope(&boundary.key).unwrap().key, &boundary.key));
        assert!(domains.has_others(&boundary.key, &parent).unwrap());
        assert!(domains.has_others(&boundary.key, &child).unwrap());
        assert!(domains.close(&boundary, &boundary.key).is_err());
        assert!(domains.detach(&parent).is_err());
        assert!(domains.detach(&child).is_err());
        assert!(domains.finish_root(&root, &root.key, &parent).is_err());
        assert_eq!(domains.member_count(), 3);
        domains.detach(&grandchild).unwrap();
        domains.detach(&child).unwrap();
        assert!(!domains.has_others(&boundary.key, &parent).unwrap());
        assert!(domains.detach(&child).is_err());
        domains.close(&boundary, &boundary.key).unwrap();
        domains.finish_root(&root, &root.key, &parent).unwrap();
        assert_eq!((domains.len(), domains.member_count()), (0, 0));
    }

    #[test]
    fn stopped_domains_reject_admission_and_normal_work_but_allow_owning_handler() {
        let mut domains = Domains::default();
        let root = open(&mut domains, None);
        let nested = open(&mut domains, Some(&root.key));
        let other = open(&mut domains, None);
        let member = domains.attach(&root.key, None).unwrap();
        domains.stop(&root.key).unwrap();
        let next = domains.next_member;
        assert!(domains.attach(&nested.key, Some(&member)).is_err());
        assert_eq!(domains.next_member, next);
        assert!(domains.require_open(&nested.key, &[]).is_err());
        domains.require_open(&nested.key, &[&root.key]).unwrap();
        domains.require_open(&other.key, &[]).unwrap();
        domains.close(&nested, &nested.key).unwrap();
        domains.finish_root(&root, &root.key, &member).unwrap();
        assert!(domains.stop(&root.key).is_err());
        domains.next_member = u64::MAX;
        assert!(domains.attach(&other.key, None).is_err());
        assert_eq!(domains.member_count(), 0);
        domains.close(&other, &other.key).unwrap();
        assert!(domains.stopped.is_empty());
    }
}
