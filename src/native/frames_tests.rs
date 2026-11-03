use super::super::{Host, assign_place, read_place, reference};
use super::*;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[test]
fn storage_identity_rejects_foreign_frames_and_offset_reuse() {
    let mut a = Frames::default();
    let frame = a.enter(2).unwrap();
    let empty = a.enter(0).unwrap();
    let later = a.enter(1).unwrap();
    let stored = a.local_storage(&frame, 1).unwrap();
    assert_eq!(stored, a.storage_at(frame.base + 1).unwrap());
    assert!(a.local_storage(&frame, 2).is_err());
    assert!(a.local_storage(&frame, usize::MAX).is_err());
    assert!(a.local_storage(&empty, 0).is_err());
    assert!(a.storage_at(usize::MAX).is_err());
    let mut b = Frames::default();
    let foreign = b.enter(2).unwrap();
    let other = b.local_storage(&foreign, 1).unwrap();
    assert_ne!(
        stored, other,
        "equal numeric identities in separate arenas differ"
    );
    assert!(a.local_storage(&foreign, 1).is_err());
    assert!(a.require_storage(&other).is_err());
    a.retire(&frame).unwrap();
    assert!(a.storage_at(frame.base).is_err());
    let replacement = a.enter(2).unwrap();
    assert_eq!(replacement.base, frame.base);
    let fresh = a.local_storage(&replacement, 1).unwrap();
    assert_ne!(stored, fresh);
    assert!(a.require_storage(&stored).is_err());
    assert_eq!(a.require_storage(&fresh).unwrap(), frame.base + 1);
    assert!(a.local_storage(&frame, 1).is_err());
    assert!(a.frame_values(&empty).unwrap().is_empty());
    a.retire(&later).unwrap();
    a.retire(&replacement).unwrap();
    a.retire(&empty).unwrap();
    assert!(a.is_empty());
}

#[test]
fn every_retirement_order_preserves_live_ranges_and_coalesces_storage() {
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let order = [a, b, c, d];
                    if (0..4).any(|i| (i + 1..4).any(|j| order[i] == order[j])) {
                        continue;
                    }
                    let mut arena = Frames::default();
                    let frames: Vec<_> = [2, 3, 1, 4]
                        .into_iter()
                        .map(|len| arena.enter(len).unwrap())
                        .collect();
                    for (i, frame) in frames.iter().enumerate() {
                        arena[frame.base..frame.base + frame.len].fill(Some(V::U32(i as u32)));
                    }
                    let mut live = [true; 4];
                    for i in order {
                        arena.retire(&frames[i]).unwrap();
                        live[i] = false;
                        let temporary = arena.enter(2).unwrap();
                        assert!(
                            arena
                                .frame_values(&temporary)
                                .unwrap()
                                .iter()
                                .all(Option::is_none)
                        );
                        arena[temporary.base] = Some(V::String("temporary".into()));
                        for (j, frame) in frames.iter().enumerate().filter(|(j, _)| live[*j]) {
                            assert!(
                                arena
                                    .frame_values(frame)
                                    .unwrap()
                                    .iter()
                                    .all(|v| *v == Some(V::U32(j as u32)))
                            );
                        }
                        assert!(arena.retire(&frames[i]).is_err());
                        arena.retire(&temporary).unwrap();
                    }
                    assert!(arena.values.is_empty());
                    assert!(arena.live.is_empty());
                    assert!(arena.free.is_empty());
                }
            }
        }
    }
}

#[test]
fn frame_identities_and_storage_exhaustion_fail_without_mutation() {
    let mut arena = Frames::default();
    let frame = arena.enter(3).unwrap();
    arena[frame.base] = Some(V::String("keep".into()));
    let mut other = Frames::default();
    let foreign = other.enter(3).unwrap();
    assert_eq!(frame.id, foreign.id);
    assert!(arena.frame_values(&foreign).is_err());
    assert!(arena.retire(&foreign).is_err());
    assert_eq!(arena[frame.base], Some(V::String("keep".into())));
    let before = (
        arena.values.clone(),
        arena.live.clone(),
        arena.free.clone(),
        arena.next,
    );
    assert!(arena.enter(usize::MAX).is_err());
    assert_eq!(
        (
            arena.values.clone(),
            arena.live.clone(),
            arena.free.clone(),
            arena.next
        ),
        before
    );
    let empty = arena.enter(0).unwrap();
    arena.retire(&frame).unwrap();
    assert!(arena.frame_values(&empty).unwrap().is_empty());
    let replacement = arena.enter(3).unwrap();
    assert_eq!(replacement.base, frame.base);
    assert_ne!(replacement.id, frame.id);
    assert!(arena.retire(&frame).is_err());
    arena.next = u64::MAX;
    let before = (arena.values.clone(), arena.live.clone(), arena.free.clone());
    assert!(arena.enter(0).is_err());
    assert_eq!(
        (arena.values.clone(), arena.live.clone(), arena.free.clone()),
        before
    );
    arena.retire(&replacement).unwrap();
    arena.retire(&empty).unwrap();
    assert!(arena.live.is_empty());
    assert!(arena.values.is_empty());
}

#[test]
fn repeated_hole_reuse_preserves_borrowed_paths_and_owned_buffers() {
    let mut arena = Frames::default();
    let owner = arena.enter(1).unwrap();
    let text = "retained".repeat(100);
    let allocation = text.as_ptr();
    arena[owner.base] = Some(V::Construct {
        ty: "state".into(),
        name: "state".into(),
        fields: vec![V::String(text), V::U32(0)],
    });
    let initial_hole = arena.enter(3).unwrap();
    let unrelated = arena.enter(2).unwrap();
    arena[unrelated.base] = Some(V::U32(91));
    arena.retire(&initial_hole).unwrap();
    for round in 0..100 {
        let child = arena.enter(3).unwrap();
        assert_eq!(child.base, initial_hole.base);
        arena[child.base] =
            Some(reference(&arena, owner.base, &[1], false, "&mutable U32").unwrap());
        assert_eq!(
            read_place(&arena, child.base, &[], true).unwrap(),
            V::U32(round)
        );
        let V::Construct { fields, .. } = arena[owner.base].as_ref().unwrap() else {
            panic!()
        };
        let V::String(original) = &fields[0] else {
            panic!()
        };
        assert_eq!(original.as_ptr(), allocation);
        assign_place(&mut arena, child.base, &[], true, V::U32(round + 1)).unwrap();
        let growth = arena.enter(1024).unwrap();
        assert_eq!(
            read_place(&arena, child.base, &[], true).unwrap(),
            V::U32(round + 1)
        );
        arena.retire(&growth).unwrap();
        arena.retire(&child).unwrap();
        assert_eq!(arena[unrelated.base], Some(V::U32(91)));
        assert_eq!(arena.len(), 6);
        assert_eq!(arena.live.len(), 2);
        assert_eq!(arena.free.len(), 1);
    }
    arena.retire(&unrelated).unwrap();
    assert_eq!(arena.len(), 1);
    arena.retire(&owner).unwrap();
    assert!(arena.is_empty());
}

#[test]
fn exact_frame_cleanup_preserves_later_resources_and_borrowed_owners() {
    struct Probe(u32, Rc<RefCell<Vec<u32>>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let drops = Rc::new(RefCell::new(vec![]));
    let mut host = Host::new(json!({}), None, true, None);
    let mut arena = Frames::default();
    let owner = arena.enter(1).unwrap();
    let child = arena.enter(2).unwrap();
    let later = arena.enter(1).unwrap();
    for (frame, id) in [(&owner, 1), (&child, 2), (&later, 3)] {
        arena[frame.base] = Some(
            host.resources
                .insert("probe", Probe(id, drops.clone()))
                .unwrap(),
        );
    }
    arena[child.base + 1] = Some(reference(&arena, owner.base, &[], false, "&probe").unwrap());
    host.release_frame(arena.frame_values(&child).unwrap());
    arena.retire(&child).unwrap();
    assert_eq!(*drops.borrow(), [2]);
    for (frame, id) in [(&owner, 1), (&later, 3)] {
        assert_eq!(
            host.resources
                .get::<Probe>(arena[frame.base].as_ref().unwrap(), "probe")
                .unwrap()
                .0,
            id
        );
    }
    host.release_frame(arena.frame_values(&later).unwrap());
    arena.retire(&later).unwrap();
    assert_eq!(*drops.borrow(), [2, 3]);
    host.release_frame(arena.frame_values(&owner).unwrap());
    arena.retire(&owner).unwrap();
    drop(host);
    assert_eq!(*drops.borrow(), [2, 3, 1]);
    assert!(arena.is_empty());
}

#[test]
fn prepared_frames_preserve_slots_until_commit_and_reject_stale_or_foreign_plans() {
    let mut a = Frames::default();
    let first = a.enter(3).unwrap();
    let last = a.enter(1).unwrap();
    a[last.base] = Some(V::U32(91));
    a.retire(&first).unwrap();
    let before = (a.values.clone(), a.live.clone(), a.free.clone(), a.next);
    let cancelled = a.prepare(2).unwrap();
    drop(cancelled);
    assert_eq!(
        (a.values.clone(), a.live.clone(), a.free.clone(), a.next),
        before
    );
    let old = a.prepare(2).unwrap();
    let fresh = a.prepare(2).unwrap();
    let frame = a.commit(fresh).unwrap();
    assert_eq!(frame.base, first.base);
    assert_eq!(a[last.base], Some(V::U32(91)));
    let before = (a.values.clone(), a.live.clone(), a.free.clone(), a.next);
    assert!(a.commit(old).is_err());
    assert_eq!(
        (a.values.clone(), a.live.clone(), a.free.clone(), a.next),
        before
    );
    let mut b = Frames::default();
    let foreign = b.prepare(2).unwrap();
    assert!(a.commit(foreign).is_err());
    assert_eq!(
        (a.values.clone(), a.live.clone(), a.free.clone(), a.next),
        before
    );
    a.retire(&frame).unwrap();
    a.retire(&last).unwrap();
    assert!(a.is_empty());
}
