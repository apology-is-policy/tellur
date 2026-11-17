// Standalone representation experiment. The Villanelle runtime does not link this crate.
#[cfg(feature = "allocation-counts")]
mod allocations;

use blart::TreeMap;
use std::collections::BTreeMap;
use std::hint::black_box;
#[cfg(not(feature = "allocation-counts"))]
use std::time::Instant;

trait OrderedMap: Default {
    const NAME: &'static str;
    fn insert(&mut self, key: String, value: u32) -> Option<u32>;
    fn get(&self, key: &str) -> Option<u32>;
    fn entries(&self) -> impl Iterator<Item = (&str, u32)>;
    fn pop_first(&mut self) -> Option<(String, u32)>;
}

#[derive(Default)]
struct Flat<const BINARY: bool>(Vec<(String, u32)>);

impl<const BINARY: bool> Flat<BINARY> {
    fn insertion_point(&self, key: &str) -> Result<usize, usize> {
        if BINARY {
            return self.0.binary_search_by(|(k, _)| k.as_str().cmp(key));
        }
        let i = self
            .0
            .iter()
            .position(|(k, _)| k.as_str() >= key)
            .unwrap_or(self.0.len());
        if self.0.get(i).is_some_and(|(k, _)| k == key) {
            Ok(i)
        } else {
            Err(i)
        }
    }
}

impl<const BINARY: bool> OrderedMap for Flat<BINARY> {
    const NAME: &'static str = if BINARY { "flat_binary" } else { "flat_linear" };
    fn insert(&mut self, key: String, value: u32) -> Option<u32> {
        match self.insertion_point(&key) {
            Ok(i) => Some(std::mem::replace(&mut self.0[i].1, value)),
            Err(i) => {
                self.0.insert(i, (key, value));
                None
            }
        }
    }
    fn get(&self, key: &str) -> Option<u32> {
        if BINARY {
            self.insertion_point(key).ok().map(|i| self.0[i].1)
        } else {
            self.0.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
        }
    }
    fn entries(&self) -> impl Iterator<Item = (&str, u32)> {
        self.0.iter().map(|(k, v)| (k.as_str(), *v))
    }
    fn pop_first(&mut self) -> Option<(String, u32)> {
        (!self.0.is_empty()).then(|| self.0.remove(0))
    }
}

impl OrderedMap for BTreeMap<String, u32> {
    const NAME: &'static str = "btree";
    fn insert(&mut self, key: String, value: u32) -> Option<u32> {
        self.insert(key, value)
    }
    fn get(&self, key: &str) -> Option<u32> {
        self.get(key).copied()
    }
    fn entries(&self) -> impl Iterator<Item = (&str, u32)> {
        self.iter().map(|(k, v)| (k.as_str(), *v))
    }
    fn pop_first(&mut self) -> Option<(String, u32)> {
        self.pop_first()
    }
}

// blart needs prefix-free keys. Escape zero as 00 FF and terminate with 00 00.
// This preserves byte order, including when one Villanelle string prefixes another.
fn with_art_key<T>(key: &str, use_key: impl FnOnce(&[u8]) -> T) -> T {
    let zeros = key.bytes().filter(|&b| b == 0).count();
    let len = key.len() + zeros + 2;
    let mut short = [0u8; 96];
    let mut long;
    let bytes = if len <= short.len() {
        &mut short[..len]
    } else {
        long = vec![0; len];
        long.as_mut_slice()
    };
    if zeros == 0 {
        bytes[..key.len()].copy_from_slice(key.as_bytes());
    } else {
        let mut index = 0;
        for b in key.bytes() {
            bytes[index] = b;
            index += 1;
            if b == 0 {
                bytes[index] = 255;
                index += 1;
            }
        }
    }
    use_key(bytes)
}

#[derive(Default)]
struct Art(TreeMap<Vec<u8>, (String, u32)>);

impl OrderedMap for Art {
    const NAME: &'static str = "art";
    fn insert(&mut self, key: String, value: u32) -> Option<u32> {
        self.0
            .try_insert(with_art_key(&key, |bytes| bytes.to_vec()), (key, value))
            .expect("prefix-free encoding")
            .map(|(_, v)| v)
    }
    fn get(&self, key: &str) -> Option<u32> {
        with_art_key(key, |encoded| self.0.get(encoded).map(|(_, v)| *v))
    }
    fn entries(&self) -> impl Iterator<Item = (&str, u32)> {
        self.0.iter().map(|(_, (k, v))| (k.as_str(), *v))
    }
    fn pop_first(&mut self) -> Option<(String, u32)> {
        self.0.pop_first().map(|(_, pair)| pair)
    }
}

fn keys(n: usize) -> Vec<String> {
    let mut keys: Vec<_> = if n == 8 {
        [
            "host",
            "origin",
            "accept",
            "content-type",
            "content-length",
            "connection",
            "accept-encoding",
            "user-agent",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect()
    } else {
        (0..n)
            .map(|i| format!("settings/tenant/{:02}/property/{:05}", i % 32, i))
            .collect()
    };
    let mut seed = 1729u32;
    for i in (1..n).rev() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        keys.swap(i, seed as usize % (i + 1));
    }
    keys
}

fn build<M: OrderedMap>(keys: &[String]) -> M {
    let mut map = M::default();
    for (i, key) in keys.iter().enumerate() {
        map.insert(key.clone(), i as u32);
    }
    map
}

fn check<M: OrderedMap>() {
    let mut map = M::default();
    let mut expected = BTreeMap::new();
    let edge_keys = [
        "é", "e\u{301}", "a\0b", "a", "🍏", "", "aa", "\0", "a\0", "éx", "\0\0", "a",
    ];
    for (i, k) in edge_keys.iter().enumerate() {
        assert_eq!(
            map.insert(k.to_string(), i as u32),
            expected.insert(k.to_string(), i as u32)
        );
    }
    for k in ["x".repeat(120), "x".repeat(120) + "\0", "\0".repeat(100)] {
        assert_eq!(map.insert(k.clone(), 42), expected.insert(k, 42));
    }
    assert_eq!(map.get("absent"), None);
    for (k, v) in &expected {
        assert_eq!(map.get(k), Some(*v));
    }
    assert!(
        map.entries()
            .eq(expected.iter().map(|(k, v)| (k.as_str(), *v)))
    );
    for pair in expected {
        assert_eq!(map.pop_first(), Some(pair));
    }
    assert_eq!(map.pop_first(), None);
}

#[cfg(not(feature = "allocation-counts"))]
fn phase<T>(setup: impl Fn() -> T, mut op: impl FnMut(&mut T), n: usize, name: &str, model: &str) {
    let repeats = (16_384 / n).max(4);
    let mut samples = [0.0; 7];
    for sample in &mut samples {
        let mut elapsed = std::time::Duration::ZERO;
        for _ in 0..repeats {
            let mut state = setup();
            let start = Instant::now();
            op(black_box(&mut state));
            elapsed += start.elapsed();
            black_box(&state);
        }
        *sample = elapsed.as_nanos() as f64 / (n * repeats) as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!("{model},{n},{name},{:.2}", samples[3]);
}

#[cfg(feature = "allocation-counts")]
fn phase<T>(setup: impl Fn() -> T, mut op: impl FnMut(&mut T), n: usize, name: &str, model: &str) {
    let mut state = setup();
    let (_, calls, peak) = allocations::measure(|| op(black_box(&mut state)));
    println!("{model},{n},{name},{calls},{peak}");
}

fn bench<M: OrderedMap>(keys: &[String]) {
    check::<M>();
    let n = keys.len();
    let miss: Vec<_> = keys.iter().map(|k| format!("{k}~")).collect();
    phase(
        M::default,
        |m: &mut M| {
            for (i, k) in keys.iter().enumerate() {
                black_box(m.insert(k.clone(), i as u32));
            }
        },
        n,
        "insert",
        M::NAME,
    );
    phase(
        || build::<M>(keys),
        |m| {
            for (i, k) in keys.iter().enumerate() {
                let query = if i % 2 == 0 { k } else { &miss[i] };
                black_box(m.get(black_box(query)));
            }
        },
        n,
        "lookup",
        M::NAME,
    );
    phase(
        || build::<M>(keys),
        |m| {
            for (k, v) in m.entries() {
                black_box((k, v));
            }
        },
        n,
        "scan",
        M::NAME,
    );
    phase(
        || build::<M>(keys),
        |m| {
            while let Some(pair) = m.pop_first() {
                black_box(pair);
            }
        },
        n,
        "drain",
        M::NAME,
    );
}

fn main() {
    #[cfg(not(feature = "allocation-counts"))]
    println!("model,size,operation,median_ns_per_entry");
    #[cfg(feature = "allocation-counts")]
    println!("model,size,operation,allocations,additional_peak_live_bytes");
    for n in [8, 64, 512, 4096] {
        let input = keys(n);
        bench::<Flat<false>>(&input);
        bench::<Flat<true>>(&input);
        bench::<BTreeMap<String, u32>>(&input);
        bench::<Art>(&input);
    }
}
