# Ordered string maps

This experiment measures four implementations of the same ordered string-key operations. Two keep a sorted vector of key/value pairs. One scans linearly for a key, matching the search strategy of Tellur's reference implementation, and the other uses binary search. The other two use Rust's B-tree and the blart adaptive radix tree (ART). Each stores owned UTF-8 keys and a `u32` payload and exposes borrowed keys in byte order. [Tellur collection implementation](../../src/collection_support.rs) · [blart 0.6.0](https://docs.rs/blart/0.6.0/blart/struct.TreeMap.html)

## Results on Apple M2

Median nanoseconds per entry on one machine, using rustc 1.97.1 in release mode. The lookup mix is half hits and half misses. Scan visits borrowed entries in order. Drain repeatedly removes the smallest entry and drops it. Build memory is peak additional requested heap bytes during insertion.

| Workload | Layout | Insert | Lookup | Scan | Drain | Build memory |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 8 headers | Flat, linear | 74 | 7 | 2.3 | 14 | 333 B |
| | Flat, binary | 36 | 17 | 3.0 | 17 | 333 B |
| | B-tree | 41 | 20 | 2.8 | 23 | 397 B |
| | ART | 83 | 20 | 2.4 | 49 | 1,106 B |
| 4,096 settings | Flat, linear | 3,637 | 3,258 | 0.42 | 932 | 266,240 B |
| | Flat, binary | 571 | 125 | 0.38 | 813 | 266,240 B |
| | B-tree | 139 | 107 | 1.98 | 32 | 313,824 B |
| | ART | 154 | 76 | 5.53 | 57 | 684,920 B |

The eight real header names span 4–15 bytes. The larger workloads use 33-byte synthetic settings paths sharing 32 tenant prefixes. At 512 entries the B-tree lookup took 66 ns and ART took 73 ns. Their ordering reversed at 4,096. The gap depends on key distribution, layout and access pattern. Inserting the shuffled 4,096 keys into the flat model shifts 4,173,894 existing entries. Draining from the front shifts another 8,386,560. These are counts of entry moves derived from the trace. Tree-internal byte moves were not instrumented. [Full timings](results/timings.csv) · [Allocation counts](results/allocations.csv)

The current Tellur 0.284 LLVM executable also ran a complete ordered drain. At 4,096 entries the median was 95.5 ms. A control that accepted and dropped the same input took 4.7 ms. At 64 entries the corresponding times were 3.6 and 3.5 ms. These wall times include launch and scenario decoding. The Rust layout timings above cover in-process operations. The trees have yet to be integrated into the Tellur runtime. [Tellur baseline](baseline.tellur) · [Process timings](results/native-timings.csv)

A borrowed ordered scan is worth prototyping on the current flat layout. It could visit every entry once while leaving the map available to the caller. Keep the B-tree as a baseline for large mixed workloads and ART as a candidate for lookup-heavy workloads with shared prefixes. Choosing a representation or promotion threshold needs measurements inside the runtime on application traces.

## Reproducing the experiment

Run from the repository root with Rust, Python 3 and a native Tellur 0.284 compiler.

```sh
export CARGO_INCREMENTAL=0
cargo run --release --locked --manifest-path experiments/ordered_maps/Cargo.toml
cargo run --release --locked --manifest-path experiments/ordered_maps/Cargo.toml --features allocation-counts
python3 experiments/ordered_maps/tellur_baseline.py ./bin/tellur
```

The first two commands validate insertion, replacement, hits, misses, borrowed iteration and ordered removal against a B-tree oracle before measurement. Cases include empty strings, embedded zero bytes, prefixes, long keys, multibyte UTF-8 and distinct Unicode spellings. ART stores both the original string and a prefix-free encoded key. A zero byte becomes `00 FF` and the terminator is `00 00`. Lookup encodes short keys on the stack. This lets iteration expose the original borrowed spelling. The memory figures include the extra representation.

Input generation and shuffling use a fixed seed. The timing run takes the median of seven batches. Each batch visits at least 16,384 entries across repetitions. Setup and final map destruction are outside the timed region. The scan consumes each key reference and value through a black box and does no text processing. A separate single-threaded run counts allocation and reallocation calls and peak live requested bytes above the start of each phase. Allocator metadata and any internal overlap during reallocation are outside this accounting. Tiny-map results include timer overhead.

The Python command checks ordered removal, replacement and lookup on edge-case keys against the same expected Tellur events in the authored and expanded interpreter and LLVM executable. It then measures nine fresh native processes per entry and size. Its inputs use settings-shaped keys at all four sizes.
