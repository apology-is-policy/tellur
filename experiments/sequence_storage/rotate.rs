#[cfg(allocation_counts)]
#[path = "../ordered_maps/src/allocations.rs"]
mod allocations;
mod movement;
mod tree;
use std::hint::black_box;
use std::time::Instant;
use tree::Tree;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let n: usize = args[1].parse().unwrap();
    let at = black_box(n / 3);
    let values: Vec<u32> = (0..n as u32).collect();
    let mut tree = Tree::new();
    if args[0] == "relaxed" {
        for &i in &values {
            tree.push(i);
        }
    }
    let operation = || {
        if args[0] == "flat" {
            let mut first = values;
            let mut rest = first.split_off(at);
            rest.extend(first);
            (Some(black_box(rest)), None)
        } else {
            let (first, rest) = match tree.split(at) {
                Ok(parts) => parts,
                Err(_) => panic!(),
            };
            (None, Some(black_box(rest.join(first))))
        }
    };
    let begin = Instant::now();
    #[cfg(allocation_counts)]
    let ((flat, relaxed), calls, peak, requested) = allocations::measure_requests(operation);
    #[cfg(not(allocation_counts))]
    let ((flat, relaxed), calls, peak, requested) = (operation(), 0, 0, 0);
    let ns = begin.elapsed().as_nanos();
    let expected: Vec<u32> = (at..n).chain(0..at).map(|i| i as u32).collect();
    if let Some(flat) = flat {
        assert_eq!(flat, expected);
    }
    if let Some(relaxed) = relaxed {
        let mut actual = Vec::new();
        relaxed.visit(|v| actual.push(*v));
        assert_eq!(actual, expected);
    }
    println!("{ns},{calls},{peak},{requested}");
}
