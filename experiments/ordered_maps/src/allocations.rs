// Only the separate allocation-counting binary uses this allocator.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocated(bytes: usize) {
    CALLS.fetch_add(1, Relaxed);
    REQUESTED.fetch_add(bytes, Relaxed);
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}

// Each operation delegates to System with the original allocator contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(p, old, size) };
        if !new.is_null() {
            LIVE.fetch_sub(old.size(), Relaxed);
            allocated(size);
        }
        new
    }
}

pub fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    let (result, allocations, peak, _) = measure_requests(f);
    (result, allocations, peak)
}

pub fn measure_requests<T>(f: impl FnOnce() -> T) -> (T, usize, usize, usize) {
    let requested = REQUESTED.load(Relaxed);
    let baseline = LIVE.load(Relaxed);
    PEAK.store(baseline, Relaxed);
    let before = CALLS.load(Relaxed);
    let result = f();
    (
        result,
        CALLS.load(Relaxed) - before,
        PEAK.load(Relaxed) - baseline,
        REQUESTED.load(Relaxed) - requested,
    )
}
