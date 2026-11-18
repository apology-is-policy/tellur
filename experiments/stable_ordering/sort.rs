#[cfg(allocation_counts)]
#[path = "../ordered_maps/src/allocations.rs"]
mod allocations;
use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let n: u32 = args[0].parse().unwrap();
    let pattern = &args[1];
    let values: Vec<_> = (0..n)
        .map(|i| {
            let key = match pattern.as_str() {
                "sorted" => i / 3,
                "reverse" => (n - 1 - i) / 3,
                "equal" => 1,
                _ => (i * 37 + 11) % 31,
            };
            (key, i)
        })
        .collect();
    let mut input = black_box(values.clone());
    let start = Instant::now();
    #[cfg(allocation_counts)]
    let ((), calls, peak, requested) = allocations::measure_requests(|| input.sort_by_key(|x| x.0));
    #[cfg(not(allocation_counts))]
    let ((), calls, peak, requested) = (input.sort_by_key(|x| x.0), 0, 0, 0);
    let elapsed = start.elapsed().as_nanos();
    let result = black_box(input);
    // Unique input indices make tuple ordering an independent stability oracle.
    let mut expected = values;
    expected.sort_unstable();
    assert_eq!(result, expected);
    println!("{elapsed},{calls},{peak},{requested}");
}
