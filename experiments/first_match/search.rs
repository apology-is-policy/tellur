use std::{hint::black_box, time::Instant};
fn main() {
    let values: Vec<u32> = (0..256).collect();
    let values = black_box(&values);
    let limit = black_box(255);
    let start = Instant::now();
    let found = black_box(values.iter().find(|&&x| x >= limit));
    let ns = start.elapsed().as_nanos();
    eprintln!("{ns}");
    println!("{}\ndone", found.unwrap());
}
