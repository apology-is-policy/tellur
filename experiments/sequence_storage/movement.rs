// Explicit payload transfers and exposure to allocator relocation are counted separately.
#[cfg(movement)]
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
#[cfg(movement)]
static TRANSFERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(movement)]
static GROWTH: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn transfer(n: usize) {
    #[cfg(movement)]
    TRANSFERS.fetch_add(n, Relaxed);
    #[cfg(not(movement))]
    let _ = n;
}
#[inline]
pub fn growth(n: usize) {
    #[cfg(movement)]
    GROWTH.fetch_add(n, Relaxed);
    #[cfg(not(movement))]
    let _ = n;
}
pub fn reset() {
    #[cfg(movement)]
    {
        TRANSFERS.store(0, Relaxed);
        GROWTH.store(0, Relaxed);
    }
}
pub fn counts() -> (usize, usize) {
    #[cfg(movement)]
    {
        (TRANSFERS.load(Relaxed), GROWTH.load(Relaxed))
    }
    #[cfg(not(movement))]
    {
        (0, 0)
    }
}
