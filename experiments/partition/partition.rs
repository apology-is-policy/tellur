#[cfg(allocation_counts)]
#[path = "../ordered_maps/src/allocations.rs"]
mod allocations;
use std::{hint::black_box, time::Instant};

fn partition(values: Vec<u32>, limit: u32) -> (Vec<u32>, Vec<u32>, Option<u32>, Vec<u32>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut rest = values.into_iter();
    while let Some(current) = rest.next() {
        if current == 0 {
            return (left, right, Some(current), rest.collect());
        }
        if current < limit {
            left.push(current);
        } else {
            right.push(current);
        }
    }
    (left, right, None, Vec::new())
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let n: usize = args[0].parse().unwrap();
    let stop: usize = args[1].parse().unwrap();
    let values: Vec<_> = (0..n)
        .map(|i| if i == stop { 0 } else { i as u32 + 1 })
        .collect();
    let limit = n as u32 / 2 + 1;
    let input = black_box(values.clone());
    let start = Instant::now();
    #[cfg(allocation_counts)]
    let (result, calls, peak, requested) =
        allocations::measure_requests(|| partition(input, black_box(limit)));
    #[cfg(not(allocation_counts))]
    let (result, calls, peak, requested) = (partition(input, black_box(limit)), 0, 0, 0);
    let elapsed = start.elapsed().as_nanos();
    let (left, right, current, remaining) = black_box(result);
    let end = stop.min(n);
    assert_eq!(
        left,
        values[..end]
            .iter()
            .copied()
            .filter(|&x| x < limit)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        right,
        values[..end]
            .iter()
            .copied()
            .filter(|&x| x >= limit)
            .collect::<Vec<_>>()
    );
    assert_eq!(current, (stop < n).then_some(0));
    assert_eq!(
        remaining,
        if stop < n {
            values[stop + 1..].to_vec()
        } else {
            vec![]
        }
    );
    println!("{elapsed},{calls},{peak},{requested}");
}
