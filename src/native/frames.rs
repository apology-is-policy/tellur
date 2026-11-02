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
    pub fn enter(&mut self, len: usize) -> R<Frame> {
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
        let base = if let Some((base, size)) = hole {
            self.free.remove(&base);
            if size > len {
                self.free.insert(base + len, size - len);
            }
            base
        } else {
            let base = self.values.len();
            let end = base
                .checked_add(len)
                .ok_or("E_NATIVE: frame storage exhausted")?;
            self.values
                .try_reserve(len)
                .map_err(|_| "E_NATIVE: frame storage exhausted")?;
            self.values.resize_with(end, || None);
            base
        };
        self.next = id;
        assert!(self.live.insert(id, (base, len)).is_none());
        Ok(Frame {
            arena: self.identity.clone(),
            id,
            base,
            len,
        })
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
