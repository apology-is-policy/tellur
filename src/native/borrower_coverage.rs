//! Immutable census of context-managed borrowers, not source permission.
use super::super::access_resolution::{ResolvedAccess, StoragePlace};
use super::super::dependencies::Interference;
use super::*;
use std::collections::BTreeSet;

const LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BorrowerInterference {
    Disjoint {
        contexts: usize,
        handoffs: usize,
        deliveries: usize,
    },
    Overlap {
        contexts: usize,
        handoffs: usize,
        deliveries: usize,
        access: ResolvedAccess,
        dependency: StoragePlace,
    },
    Inconclusive(String),
}
impl Contexts<'_> {
    /// Reconcile all registered descendants and retained await payloads before
    /// comparing one freshly resolved callback. Neither a single child nor an
    /// empty context map can certify coverage without membership reconciliation.
    /// Local source aliases/cold values and externally joined results are outside
    /// this context-managed census; source checking remains intact. Query results
    /// grant no execution authority; one-transition admission reruns this census.
    pub fn borrower_interference(&self, parent: &ContextKey) -> BorrowerInterference {
        self.borrower_query(parent)
            .unwrap_or_else(BorrowerInterference::Inconclusive)
    }
    fn borrower_query(&self, parent: &ContextKey) -> R<BorrowerInterference> {
        self.require(parent)?;
        let owner = &self.parked[&parent.id];
        owner.machine.require_runnable(&self.domains)?;
        self.domains.require_open(&owner.machine.current, &[])?;
        if self.waits.contains_key(&parent.id) {
            return Err("E_NATIVE: aggregate query owner is waiting".into());
        }
        let activation = owner
            .machine
            .stack
            .last()
            .ok_or("E_NATIVE: query owner has retired")?;
        activation.require_task_host(self.host)?;
        let plan = owner.machine.resolve_interference_access(&self.values)?;
        // Bound the registry scan as well as traversal, including unrelated roots.
        if self.parked.len().saturating_add(self.completed.len()) > LIMIT
            || self.domains.member_count() > LIMIT
        {
            return Err("E_NATIVE: borrower census limit exceeded".into());
        }
        let mut children: BTreeMap<u64, Vec<(u64, &Parked)>> = BTreeMap::new();
        for (id, parked) in self
            .parked
            .iter()
            .chain(self.completed.iter().map(|(id, c)| (id, &c.parked)))
        {
            if let Some(parent) = parked.parent {
                children.entry(parent).or_default().push((*id, parked));
            }
        }
        let mut queue = vec![(parent.id, owner)];
        let mut seen = BTreeSet::from([parent.id]);
        let mut edges = vec![];
        let mut descriptions = vec![];
        let mut handoffs = 0;
        let mut deliveries = 0;
        let mut frame_count = 0usize;
        let mut cursor = 0;
        while cursor < queue.len() {
            let (id, parked) = queue[cursor];
            cursor += 1;
            if let Some(children) = children.get(&id) {
                for (child_id, child) in children {
                    if !seen.insert(*child_id) {
                        return Err("E_NATIVE: duplicate borrower context".into());
                    }
                    let key = ContextKey {
                        session: self.identity.clone(),
                        id: *child_id,
                    };
                    if self.parked.contains_key(child_id) {
                        self.require(&key)?;
                    } else {
                        self.require_completed(&key)?;
                    }
                    self.domains.require_open(&child.machine.current, &[])?;
                    self.domains
                        .require_member(&owner.machine.current, &child.machine.root.key)?;
                    if id == parent.id
                        && (child.parent_invocation != Some(activation.owner)
                            || !self
                                .domains
                                .same(&owner.machine.current, &child.machine.root.key))
                    {
                        return Err("E_NATIVE: aggregate borrower belongs to another invocation or fault domain".into());
                    }
                    edges.push((&parked.machine.member, &child.machine.member));
                    descriptions.push(&child.dependencies);
                    queue.push((*child_id, child));
                }
            }
            for frame in &parked.machine.stack {
                frame_count += 1;
                if frame_count > LIMIT {
                    return Err("E_NATIVE: borrower activation census limit exceeded".into());
                }
                let Some(Pending::Await(pending)) = &frame.pending else {
                    continue;
                };
                frame.require_task_host(self.host)?;
                if pending.owner != frame.owner
                    || !self.domains.same(&pending.domain, &parked.machine.current)
                    || parked.machine.stack.last().map(|a| a.owner) != Some(frame.owner)
                {
                    return Err(
                        "E_NATIVE: pending borrower has another invocation or fault domain".into(),
                    );
                }
                self.domains.require_open(&pending.domain, &[])?;
                match (&pending.queued, &pending.child, &pending.result) {
                    (Some(queued), None, None) if parked.awaiting.is_none() => {
                        queued.prepare_context(self.host, frame)?;
                        handoffs += 1;
                    }
                    (None, Some(child), None) if parked.awaiting == Some(child.id) => {
                        if !Arc::ptr_eq(&child.session, &self.identity) {
                            return Err("E_NATIVE: foreign pending borrower".into());
                        }
                        let child = self
                            .parked
                            .get(&child.id)
                            .or_else(|| self.completed.get(&child.id).map(|c| &c.parked))
                            .ok_or("E_NATIVE: missing pending borrower context")?;
                        if child.parent != Some(id)
                            || child.parent_invocation != Some(frame.owner)
                            || !child.sequential
                            || !self.domains.same(&pending.domain, &child.machine.root.key)
                        {
                            return Err("E_NATIVE: pending borrower relationship mismatch".into());
                        }
                        handoffs += 1;
                    }
                    (None, Some(child), Some(Ok(Completion::Outcome(_))))
                        if parked.awaiting.is_none() =>
                    {
                        if !Arc::ptr_eq(&child.session, &self.identity)
                            || self.parked.contains_key(&child.id)
                            || self.completed.contains_key(&child.id)
                            || self.terminal.contains_key(&child.id)
                        {
                            return Err("E_NATIVE: pending delivery has unretired or foreign child identity".into());
                        }
                        deliveries += 1;
                    }
                    _ => return Err("E_NATIVE: pending borrower transition is unresolved".into()),
                }
                descriptions.push(&pending.dependencies);
            }
            if parked.awaiting.is_some()
                && !matches!(
                    parked.machine.stack.last().and_then(|a| a.pending.as_ref()),
                    Some(Pending::Await(_))
                )
            {
                return Err("E_NATIVE: sequential borrower lacks its retained handoff".into());
            }
        }
        self.domains
            .require_descendants(&owner.machine.member, &edges)?;
        // Do not report the first overlap until every description is complete and
        // live: a later partial descendant must keep the whole census inconclusive.
        let mut overlap = None;
        let mut comparisons = 0usize;
        let mut components = 0usize;
        for description in descriptions {
            let Dependencies::Complete {
                reachable,
                origins: super::super::reference_evidence::Origins::Complete(origins),
            } = description
            else {
                return Err("E_NATIVE: aggregate borrower dependencies are incomplete".into());
            };
            for place in reachable
                .iter()
                .map(|d| &d.place)
                .chain(origins.iter().map(|o| &o.place))
            {
                components = components
                    .saturating_add(1)
                    .saturating_add(place.fields.len());
                if components > LIMIT * 16 {
                    return Err("E_NATIVE: aggregate dependency validation limit exceeded".into());
                }
            }
            let cost = reachable
                .len()
                .saturating_add(origins.len())
                .saturating_mul(plan.accesses.len());
            comparisons = comparisons.saturating_add(cost);
            if comparisons > LIMIT * 16 {
                return Err("E_NATIVE: aggregate interference comparison limit exceeded".into());
            }
            match description.interference(&self.values, &plan) {
                Interference::Disjoint => {}
                Interference::Overlap { access, dependency } => {
                    if overlap.is_none() {
                        overlap = Some((access, dependency));
                    }
                }
                Interference::Inconclusive(message) => return Err(message),
            }
        }
        let contexts = queue.len() - 1;
        Ok(match overlap {
            None => BorrowerInterference::Disjoint {
                contexts,
                handoffs,
                deliveries,
            },
            Some((access, dependency)) => BorrowerInterference::Overlap {
                contexts,
                handoffs,
                deliveries,
                access,
                dependency,
            },
        })
    }
}
