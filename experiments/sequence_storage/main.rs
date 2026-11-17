#[cfg(allocation_counts)]
#[path = "../ordered_maps/src/allocations.rs"]
mod allocations;
mod movement;
mod tree;

use std::hint::black_box;
use std::time::Instant;
use tree::Tree;

enum Sequence<T> {
    Flat(Vec<T>),
    Cursor(std::vec::IntoIter<T>),
    Relaxed(Tree<T>),
}
impl<T> Sequence<T> {
    fn new(kind: &str) -> Self {
        match kind {
            "flat" => Self::Flat(Vec::new()),
            "cursor" => Self::Cursor(Vec::new().into_iter()),
            "relaxed" => Self::Relaxed(Tree::new()),
            _ => panic!("unknown representation"),
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Flat(v) => v.len(),
            Self::Cursor(v) => v.len(),
            Self::Relaxed(v) => v.len(),
        }
    }
    fn get(&self, i: usize) -> Option<&T> {
        match self {
            Self::Flat(v) => v.get(i),
            Self::Cursor(v) => v.as_slice().get(i),
            Self::Relaxed(v) => v.get(i),
        }
    }
    fn visit(&self, mut f: impl FnMut(&T)) {
        match self {
            Self::Flat(v) => v.iter().for_each(f),
            Self::Cursor(v) => v.as_slice().iter().for_each(f),
            Self::Relaxed(v) => v.visit(&mut f),
        }
    }
    fn push(&mut self, item: T) {
        match self {
            Self::Flat(v) => {
                movement::transfer(1);
                if v.len() == v.capacity() {
                    movement::growth(v.len());
                }
                v.push(item);
            }
            Self::Cursor(_) => unreachable!("cursor benchmark is front drain only"),
            Self::Relaxed(v) => v.push(item),
        }
    }
    fn split(self, at: usize) -> (Self, Self) {
        match self {
            Self::Flat(mut v) => {
                if at == 0 {
                    return (Self::Flat(Vec::new()), Self::Flat(v));
                }
                if at == v.len() {
                    return (Self::Flat(v), Self::Flat(Vec::new()));
                }
                movement::transfer(v.len() - at);
                let tail = v.split_off(at);
                (Self::Flat(v), Self::Flat(tail))
            }
            Self::Relaxed(v) => {
                let (left, right) = match v.split(at) {
                    Ok(parts) => parts,
                    Err(_) => panic!("invalid split"),
                };
                (Self::Relaxed(left), Self::Relaxed(right))
            }
            Self::Cursor(_) => unreachable!(),
        }
    }
    fn join(self, right: Self) -> Self {
        match (self, right) {
            (Self::Flat(mut a), Self::Flat(b)) => {
                if a.is_empty() {
                    return Self::Flat(b);
                }
                if b.is_empty() {
                    return Self::Flat(a);
                }
                movement::transfer(b.len());
                if a.len() + b.len() > a.capacity() {
                    movement::growth(a.len());
                }
                a.extend(b);
                Self::Flat(a)
            }
            (Self::Relaxed(a), Self::Relaxed(b)) => Self::Relaxed(a.join(b)),
            _ => unreachable!(),
        }
    }
    fn pop_front(&mut self) -> Option<T> {
        match self {
            Self::Flat(v) if !v.is_empty() => {
                movement::transfer(v.len());
                Some(v.remove(0))
            }
            Self::Flat(_) => None,
            Self::Cursor(v) => {
                let item = v.next();
                movement::transfer(usize::from(item.is_some()));
                item
            }
            Self::Relaxed(v) => v.pop_front(),
        }
    }
}

fn filled<const WORDS: usize>(kind: &str, start: usize, n: usize) -> Sequence<[u64; WORDS]> {
    if kind == "cursor" {
        return Sequence::Cursor(
            (start..start + n)
                .map(|i| [i as u64; WORDS])
                .collect::<Vec<_>>()
                .into_iter(),
        );
    }
    let mut v = Sequence::new(kind);
    for i in start..start + n {
        v.push([i as u64; WORDS]);
    }
    v
}

fn measure<const WORDS: usize>(kind: &str, operation: &str, n: usize) {
    let mut input = filled::<WORDS>(
        kind,
        0,
        if operation == "append" || operation == "join_batches" {
            0
        } else {
            n
        },
    );
    let batches: Vec<_> = if operation == "join_batches" {
        (0..n)
            .step_by(256)
            .map(|i| filled::<WORDS>(kind, i, 256.min(n - i)))
            .collect()
    } else {
        Vec::new()
    };
    let mut model: Vec<_> = (0..n as u64).collect();
    let count = if operation == "split_join" { 128 } else { n };
    let indexes: Vec<_> = (0..count)
        .map(|i| i.wrapping_mul(2654435761usize) % n)
        .collect();
    if operation == "split_join" {
        for &at in &indexes {
            model.rotate_left(at);
        }
    }
    let expected = match operation {
        "index" => indexes.iter().map(|&i| i as u64).sum(),
        "scan" | "front_drain" => (n as u64) * (n as u64 - 1) / 2,
        _ => 0,
    };
    movement::reset();
    let task = || {
        let mut checksum = 0u64;
        match operation {
            "append" => {
                for i in 0..n {
                    input.push(black_box([i as u64; WORDS]));
                }
            }
            "scan" => input.visit(|item| checksum = checksum.wrapping_add(item[0])),
            "index" => {
                for &i in &indexes {
                    checksum = checksum.wrapping_add(input.get(black_box(i)).unwrap()[0]);
                }
            }
            "split_join" => {
                for &at in &indexes {
                    let (a, b) = input.split(black_box(at));
                    input = b.join(a);
                }
            }
            "join_batches" => {
                for batch in batches {
                    input = input.join(batch);
                }
            }
            "front_drain" => {
                while let Some(item) = input.pop_front() {
                    checksum = checksum.wrapping_add(item[0]);
                }
            }
            _ => panic!("unknown operation"),
        }
        (black_box(input), black_box(checksum))
    };
    let start = Instant::now();
    #[cfg(allocation_counts)]
    let ((output, checksum), calls, peak, requested) = allocations::measure_requests(task);
    #[cfg(not(allocation_counts))]
    let ((output, checksum), calls, peak, requested) = (task(), 0, 0, 0);
    let ns = start.elapsed().as_nanos();
    let (moves, growth) = movement::counts();
    assert_eq!(checksum, expected);
    if operation == "front_drain" {
        assert_eq!(output.len(), 0);
    } else {
        let mut index = 0;
        output.visit(|item| {
            assert!(item.iter().all(|v| *v == model[index]));
            index += 1;
        });
        assert_eq!(index, n);
    }
    println!(
        "{kind},{operation},{n},{},{ns},{calls},{peak},{requested},{moves},{growth}",
        WORDS * 8
    );
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let n: usize = args[2].parse().unwrap();
    assert!(n > 0);
    match args[3].as_str() {
        "8" => measure::<1>(&args[0], &args[1], n),
        "64" => measure::<8>(&args[0], &args[1], n),
        _ => panic!("record size must be 8 or 64"),
    }
}
