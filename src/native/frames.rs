//! Stable slot offsets with independently retired native activation ranges.
use super::{R, V};
#[cfg(test)]
#[path = "frames_tests.rs"]
mod tests;
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

pub struct Frame {
    arena: Arc<()>,
    id: u64,
    base: usize,
    len: usize,
}
impl Frame {
    pub fn base(&self) -> usize {
        self.base
    }
}

/// A slot in one live allocation, not an offset that may be reused by another frame.
/// This identifies storage only; it grants no loan or execution permission.
#[derive(Clone, Debug)]
pub struct Storage {
    arena: Arc<()>,
    frame: u64,
    root: usize,
}
impl PartialEq for Storage {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.arena, &other.arena)
            && self.frame == other.frame
            && self.root == other.root
    }
}
impl Eq for Storage {}

/// Reserved capacity and a checked layout; no slot or identity is installed yet.
pub(super) struct PreparedFrame {
    arena: Arc<()>,
    id: u64,
    base: usize,
    len: usize,
    hole: Option<usize>,
}

/// Values never move between slot offsets while their activation is live.
/// Loan legality and borrower-before-owner retirement belong to checked execution,
/// not this allocator. A frame identity is not a source reference or task handle.
#[derive(Default)]
pub struct Frames {
    values: Vec<Option<V>>,
    identity: Arc<()>,
    next: u64,
    live: BTreeMap<u64, (usize, usize)>,
    free: BTreeMap<usize, usize>,
}
impl Deref for Frames {
    type Target = [Option<V>];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl DerefMut for Frames {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}
impl Frames {
    /// Resolve an activation-relative slot, including an uninitialized destination.
    pub fn local_storage(&self, frame: &Frame, slot: usize) -> R<Storage> {
        self.require(frame)?;
        if slot >= frame.len {
            return Err("E_NATIVE: access slot is outside its activation".into());
        }
        Ok(Storage {
            arena: self.identity.clone(),
            frame: frame.id,
            root: frame.base + slot,
        })
    }
    /// Bind a currently live absolute address to its allocation identity. Existing
    /// source references carry offsets; this cannot reconstruct their past lifetime.
    pub fn storage_at(&self, root: usize) -> R<Storage> {
        let (&frame, _) = self
            .live
            .iter()
            .find(|(_, (base, len))| root >= *base && root - *base < *len)
            .ok_or("E_NATIVE: access address has no live frame")?;
        Ok(Storage {
            arena: self.identity.clone(),
            frame,
            root,
        })
    }
    pub fn require_storage(&self, storage: &Storage) -> R<usize> {
        let live = self
            .live
            .get(&storage.frame)
            .is_some_and(|(base, len)| storage.root >= *base && storage.root - *base < *len);
        if !Arc::ptr_eq(&self.identity, &storage.arena) || !live {
            return Err("E_NATIVE: access storage is foreign or retired".into());
        }
        Ok(storage.root)
    }
    pub(super) fn prepare(&mut self, len: usize) -> R<PreparedFrame> {
        let id = self
            .next
            .checked_add(1)
            .ok_or("E_NATIVE: frame identity exhausted")?;
        let hole = (len != 0)
            .then(|| {
                self.free
                    .iter()
                    .find(|(_, n)| **n >= len)
                    .map(|(&b, &n)| (b, n))
            })
            .flatten();
        let (base, hole) = if let Some((base, size)) = hole {
            (base, Some(size))
        } else {
            let base = self.values.len();
            base.checked_add(len)
                .ok_or("E_NATIVE: frame storage exhausted")?;
            self.values
                .try_reserve(len)
                .map_err(|_| "E_NATIVE: frame storage exhausted")?;
            (base, None)
        };
        Ok(PreparedFrame {
            arena: self.identity.clone(),
            id,
            base,
            len,
            hole,
        })
    }
    pub(super) fn commit(&mut self, prepared: PreparedFrame) -> R<Frame> {
        let PreparedFrame {
            arena,
            id,
            base,
            len,
            hole,
        } = prepared;
        if !Arc::ptr_eq(&arena, &self.identity)
            || self.next.checked_add(1) != Some(id)
            || match hole {
                Some(size) => self.free.get(&base) != Some(&size),
                None => self.values.len() != base || self.values.capacity() - base < len,
            }
        {
            return Err("E_NATIVE: stale or foreign frame admission".into());
        }
        if let Some(size) = hole {
            self.free.remove(&base);
            if size > len {
                self.free.insert(base + len, size - len);
            }
        } else {
            self.values.resize_with(base + len, || None);
        }
        self.next = id;
        assert!(self.live.insert(id, (base, len)).is_none());
        Ok(Frame {
            arena,
            id,
            base,
            len,
        })
    }
    pub fn enter(&mut self, len: usize) -> R<Frame> {
        let prepared = self.prepare(len)?;
        self.commit(prepared)
    }
    #[cfg(test)]
    pub(super) fn exhaust_identity_space(&mut self) {
        self.next = u64::MAX;
    }
    fn require(&self, frame: &Frame) -> R<()> {
        if !Arc::ptr_eq(&self.identity, &frame.arena)
            || self.live.get(&frame.id) != Some(&(frame.base, frame.len))
        {
            return Err("E_NATIVE: frame is foreign or retired".into());
        }
        Ok(())
    }
    pub fn frame_values(&self, frame: &Frame) -> R<&[Option<V>]> {
        self.require(frame)?;
        // An empty live frame can outlast the last nonempty range.
        if frame.len == 0 {
            return Ok(&[]);
        }
        Ok(&self.values[frame.base..frame.base + frame.len])
    }
    pub fn retire(&mut self, frame: &Frame) -> R<()> {
        self.require(frame)?;
        self.live.remove(&frame.id);
        if frame.len == 0 {
            return Ok(());
        }
        let mut base = frame.base;
        let mut len = frame.len;
        for slot in &mut self.values[base..base + len] {
            *slot = None;
        }
        if let Some((&previous, &size)) = self.free.range(..base).next_back()
            && previous + size == base
        {
            self.free.remove(&previous);
            base = previous;
            len += size;
        }
        if let Some(size) = self.free.remove(&(base + len)) {
            len += size;
        }
        if base + len == self.values.len() {
            self.values.truncate(base);
        } else {
            assert!(self.free.insert(base, len).is_none());
        }
        Ok(())
    }
}
