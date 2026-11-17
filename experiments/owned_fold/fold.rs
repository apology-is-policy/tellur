use std::hint::black_box;
#[cfg(not(alloc_probe))]
use std::time::Instant;

#[cfg(alloc_probe)]
unsafe extern "C" {
    fn probe_begin();
    fn probe_end();
}

fn fold(weights: &[u32], target: u32) -> (&'static str, u32) {
    let mut state = 0u32;
    for &weight in weights {
        if weight == 0 {
            return ("scale offline", state);
        }
        let Some(next) = state.checked_add(weight) else {
            return ("weight overflow", state);
        };
        state = next;
        if state >= target {
            return ("target reached", state);
        }
    }
    ("batch exhausted", state)
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let weights: Vec<u32> = args[0]
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .collect();
    let target = args[1].parse().unwrap();
    let rounds: usize = args[2].parse().unwrap();
    let batch: usize = args[3].parse().unwrap();
    for _ in 0..rounds {
        #[cfg(not(alloc_probe))]
        let start = Instant::now();
        #[cfg(alloc_probe)]
        unsafe {
            probe_begin()
        };
        let mut result = ("", 0);
        for _ in 0..batch {
            result = black_box(fold(black_box(&weights), black_box(target)));
        }
        #[cfg(alloc_probe)]
        unsafe {
            probe_end()
        };
        #[cfg(not(alloc_probe))]
        eprintln!("{}", start.elapsed().as_nanos());
        println!("{}\n{}", result.0, result.1);
    }
    println!("done");
}
