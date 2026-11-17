# Ordered string maps

This experiment measures four implementations of the same ordered string-key operations. Two keep a sorted vector of key/value pairs. One scans linearly for a key, matching the search strategy of Villanelle's reference implementation, and the other uses binary search. The other two use Rust's B-tree and the blart adaptive radix tree (ART). Each stores owned UTF-8 keys and a `u32` payload and exposes borrowed keys in byte order. [Villanelle collection implementation](../../src/collection_support.rs) · [blart 0.6.0](https://docs.rs/blart/0.6.0/blart/struct.TreeMap.html)

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

The current Villanelle 0.284 LLVM executable also ran a complete ordered drain. At 4,096 entries the median was 95.5 ms. A control that accepted and dropped the same input took 4.7 ms. At 64 entries the corresponding times were 3.6 and 3.5 ms. These wall times include launch and scenario decoding. The Rust layout timings above cover in-process operations. The trees have yet to be integrated into the Villanelle runtime. [Villanelle baseline](baseline.vn) · [Process timings](results/native-timings.csv)

### Borrowed traversal in Villanelle 0.286

The public `traversal.entry_at<u32>` control projects two slots in the sorted flat map and returns the next position. A full scan visits each pair once, with no key comparisons, entry clones or shifts. The timed loop obtains both references and advances. A separate conformance run reads each key and value and checks the emitted sequence before timing.

| Settings entries | Accept and drop | Borrowed scan | Consuming drain |
| ---: | ---: | ---: | ---: |
| 512 | 3.4 ms | 5.7 ms | 5.0 ms |
| 4,096 | 4.4 ms | 22.1 ms | 92.4 ms |
| 16,384 | 8.3 ms | 79.1 ms | — |

At four times the input size, scan time above the accept-and-drop baseline grew from 17.7 to 70.8 ms. For tiny maps, process startup dominates these measurements. Each figure is the median of nine fresh native processes on Apple M2, including scenario decoding. The earlier 0.284 drain measurement is retained above. [0.286 process timings](results/borrowed-traversal.csv)

Keep the B-tree as a baseline for large mixed workloads and ART as a candidate for lookup-heavy workloads with shared prefixes. Choosing a representation or promotion threshold needs measurements inside the runtime on application traces.

### Optimized traversal and memory in Villanelle 0.287

On the same Apple M2, Villanelle 0.287 builds the complete ordered event trace in about the same time as optimized Rust with a pooled trace. Each program reads every UTF-8 key, formats every decimal value and retains both messages. The runtime stores UTF-8 bytes in one buffer with an end offset per message. The pooled Rust control applies that layout to the standard-library B-tree. The original Rust control keeps one owned string per event.

| Median time | Villanelle LLVM O3, expanded | Rust B-tree, owned strings | Rust B-tree, pooled |
| --- | ---: | ---: | ---: |
| 4,096 entries, build trace in memory | 95.54 µs | 176.04 µs | 95.50 µs |
| 16,384 entries, build trace in memory | 334.79 µs | 656.00 µs | 345.58 µs |
| 4,096 entries, complete process and JSON output | 5.03 ms | 4.46 ms | 4.35 ms |
| 16,384 entries, complete process and JSON output | 10.06 ms | 10.33 ms | 9.52 ms |
| 4,096 entries, consume by repeated front removal | 46.28 ms | 3.74 ms | — |

The previous expanded O3 build took 28.75 ms for the 4,096-entry process with output. On the old release build, 480 of 1,470 CPU samples landed inside reference-origin evidence creation or its callees. Slot storage accounted for another 324 samples. Comparing one and nine expanded traversals on the same input gave about 74 allocations and four reallocations per additional entry visited. LLVM saw calls across a precompiled runtime boundary for each of those operations.

The optimized compiler recognizes a synchronous `map_entry_at` loop or a transparent forwarding control. It fuses complete successful iterations when references stay local and the observable actions consist of supported key/value emissions. A partially funded iteration, exhaustion, and loops with arbitrary predicates or escaping references run through the checked graph. An unobserved walk can advance straight to the end. Only the observed timing rows above compare actual visits in both languages.

The observed 4,096-entry process dropped from 430,837 allocation calls at 0.286 release to 4,725 at 0.287 O3. Pooled Rust used 4,911. Villanelle still requested 4.54 MB in total and reached 5.08 MiB peak RSS, compared with 2.82 MB requested and 3.75 MiB peak RSS for pooled Rust. Counts cover the entire process. Requested bytes include reallocations. The validation pass now borrows schema types and compares adjacent keys in place. The remaining front-drain cost includes 8,386,560 pair shifts for 4,096 entries even after removing each pair in one move.

Apple M2 arm64, rustc 1.97.1 (LLVM 22.1.6), Villanelle LLVM 22.1.8. The in-memory table reports medians of 25 interleaved measured trials. The process table uses 13. Every complete JSON result was checked after timing. RSS is the median of five additional processes. Small differences within a few percent vary between runs. [Harness and reproduction](comparison/README.md) · [Hot samples](results/0.287-trace.csv) · [Process samples](results/0.287-process.csv) · [Memory samples](results/0.287-memory.csv)

## Reproducing the experiment

Run from the repository root with Rust, Python 3 and a native Villanelle 0.286 compiler.

```sh
export CARGO_INCREMENTAL=0
cargo run --release --locked --manifest-path experiments/ordered_maps/Cargo.toml
cargo run --release --locked --manifest-path experiments/ordered_maps/Cargo.toml --features allocation-counts
python3 experiments/ordered_maps/tellur_baseline.py ./bin/vnc
```

The first two commands validate insertion, replacement, hits, misses, borrowed iteration and ordered removal against a B-tree oracle before measurement. Cases include empty strings, embedded zero bytes, prefixes, long keys, multibyte UTF-8 and distinct Unicode spellings. ART stores both the original string and a prefix-free encoded key. A zero byte becomes `00 FF` and the terminator is `00 00`. Lookup encodes short keys on the stack. This lets iteration expose the original borrowed spelling. The memory figures include the extra representation.

Input generation and shuffling use a fixed seed. The timing run takes the median of seven batches. Each batch visits at least 16,384 entries across repetitions. Setup and final map destruction are outside the timed region. The scan consumes each key reference and value through a black box and does no text processing. A separate single-threaded run counts allocation and reallocation calls and peak live requested bytes above the start of each phase. Allocator metadata and any internal overlap during reallocation are outside this accounting. Tiny-map results include timer overhead.

The Python command checks ordered removal, replacement and lookup on edge-case keys against the same expected Villanelle events in the authored and expanded interpreter and LLVM executable. It then measures nine fresh native processes per entry and size. It also checks both fields of every borrowed entry. Its inputs use settings-shaped keys at five sizes, with the destructive baseline measured through 4,096 entries.
