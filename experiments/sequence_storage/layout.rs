//! Isolate payload width and the two flat split/join kernels from ABI transport.
#![allow(dead_code)]
#[cfg(allocation_counts)]
#[path = "../ordered_maps/src/allocations.rs"]
mod allocations;
#[path = "../../src/native/runtime.rs"]
mod runtime;

use runtime::V;
use std::{hint::black_box, mem::size_of, time::Instant};

fn rust_readiness() -> runtime::R<Option<Box<dyn runtime::ReadinessBackend>>> {
    Ok(None)
}

trait Item: Sized {
    fn new(id: u32) -> Self;
    fn verify(&self, id: u32);
}
impl Item for u32 {
    fn new(id: u32) -> Self {
        id
    }
    fn verify(&self, id: u32) {
        assert_eq!(*self, id);
    }
}
impl Item for V {
    fn new(id: u32) -> Self {
        V::U32(id)
    }
    fn verify(&self, id: u32) {
        assert!(matches!(self, V::U32(value) if *value == id));
    }
}
// The same width/alignment as V on this host, with every byte observable.
struct Wide([u64; 17]);
impl Item for Wide {
    fn new(id: u32) -> Self {
        Self([id as u64; 17])
    }
    fn verify(&self, id: u32) {
        assert_eq!(self.0, [id as u64; 17]);
    }
}

#[inline(never)]
fn rotate<T>(mut source: Vec<T>, at: usize, kernel: &str) -> Vec<T> {
    match kernel {
        "bulk_prefix" => {
            let mut prefix = Vec::new();
            runtime::append_vector_prefix(&mut source, &mut prefix, at);
            runtime::append_vector_prefix(&mut prefix, &mut source, at);
            source
        }
        "prefix" => {
            // Exact payload operations of 0.293's two vector_transfer calls.
            let mut prefix = Vec::new();
            prefix.extend(source.drain(..at));
            source.append(&mut prefix);
            source
        }
        "suffix" => {
            let mut suffix = source.split_off(at);
            suffix.extend(source);
            suffix
        }
        // Diagnostic lower bound only: split and join are separate public calls.
        "in_place" => {
            source.rotate_left(at);
            source
        }
        _ => panic!("unknown kernel"),
    }
}

fn measure<T: Item>(n: usize, kernel: &str, repeats: usize) {
    let mut elapsed = 0;
    let mut calls = 0;
    let mut peak = 0;
    let mut requested = 0;
    for _ in 0..repeats {
        let input = black_box((0..n as u32).map(T::new).collect::<Vec<_>>());
        let at = black_box(n / 3);
        let start = Instant::now();
        #[cfg(allocation_counts)]
        let (output, c, p, r) = allocations::measure_requests(|| rotate(input, at, kernel));
        #[cfg(not(allocation_counts))]
        let (output, c, p, r) = (rotate(input, at, kernel), 0, 0, 0);
        let output = black_box(output);
        elapsed += start.elapsed().as_nanos();
        calls += c;
        peak = peak.max(p);
        requested += r;
        assert_eq!(output.len(), n);
        for (index, value) in output.iter().enumerate() {
            value.verify(((index + at) % n) as u32);
        }
    }
    println!(
        "{},{elapsed},{repeats},{calls},{peak},{requested}",
        size_of::<T>()
    );
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(size_of::<V>(), size_of::<Wide>());
    let n = args[2].parse().unwrap();
    let repeats = args[3].parse().unwrap();
    match args[0].as_str() {
        "u32" => measure::<u32>(n, &args[1], repeats),
        "value" => measure::<V>(n, &args[1], repeats),
        "wide" => measure::<Wide>(n, &args[1], repeats),
        _ => panic!("unknown payload"),
    }
}
