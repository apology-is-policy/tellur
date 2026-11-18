#[cfg(allocation_counts)]
#[path = "../ordered_maps/src/allocations.rs"]
mod allocations;
use std::{hint::black_box, time::Instant};

type Query = fn(&[u32], u32) -> (usize, Option<u32>);
fn lower(values: &[u32], needle: u32) -> (usize, Option<u32>) {
    (values.partition_point(|x| *x < needle), None)
}
fn upper(values: &[u32], needle: u32) -> (usize, Option<u32>) {
    (values.partition_point(|x| *x <= needle), None)
}
fn first(values: &[u32], needle: u32) -> (usize, Option<u32>) {
    let index = values.partition_point(|x| *x < needle);
    (index, values.get(index).copied().filter(|x| *x == needle))
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let n: u32 = args[1].parse().unwrap();
    let needle: u32 = black_box(args[2].parse().unwrap());
    let values: Vec<_> = (0..n).map(|i| i / 3).collect();
    let query: Query = match args[0].as_str() {
        "lower_bound" => lower,
        "upper_bound" => upper,
        "first_equal" => first,
        _ => panic!("unknown operation"),
    };
    let input = black_box(values.as_slice());
    let query = black_box(query);
    let start = Instant::now();
    #[cfg(allocation_counts)]
    let (result, calls, peak, bytes) = allocations::measure_requests(|| query(input, needle));
    #[cfg(not(allocation_counts))]
    let (result, calls, peak, bytes) = (query(input, needle), 0, 0, 0);
    let elapsed = start.elapsed().as_nanos();
    let (index, value) = black_box(result);
    let expected = values
        .iter()
        .take_while(|&&x| {
            if args[0] == "upper_bound" {
                x <= needle
            } else {
                x < needle
            }
        })
        .count();
    assert_eq!(index, expected);
    if args[0] == "first_equal" {
        assert_eq!(
            value,
            values.get(expected).copied().filter(|&x| x == needle)
        );
    }
    println!(
        "{elapsed},{calls},{peak},{bytes},{index},{}",
        value.map(i64::from).unwrap_or(-1)
    );
}
