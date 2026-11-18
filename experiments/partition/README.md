# Consuming stable partition on Apple M2

The experimental source module classifies each input once, borrowing it while deciding where its owner goes. Both output vectors preserve input order. A zero stops and returns both prefixes, that current element, the untouched suffix and `unclassified`. The Rust baseline consumes a `Vec<u32>` iterator and returns the same owners and ordering. Every run checks all returned elements and the selected outcome. [Source caller](../../examples/partition_batch/main.vn) · [Library contract](../../docs/collections-and-algorithms.md#consuming-stable-partition)

| Workload | Villanelle 0.297 | Rust | Villanelle allocations / reallocations | Requested bytes |
| --- | ---: | ---: | ---: | ---: |
| Empty | 7.88 µs | 41 ns | 19 / 1 | 4,589 |
| Full, 128 items | 325 µs | 2.17 µs | 3,349 / 9 | 418,093 |
| Full, 4,096 items | 30.41 ms | 11.79 µs | 106,517 / 19 | 13,270,445 |
| Stop on first | 24.46 µs | 459 ns | 36 / 2 | 8,234 |
| Stop at index 2,048 | 20.86 ms | 5.13 µs | 53,285 / 11 | 6,639,114 |
| Stop on last | 30.51 ms | 11.63 µs | 106,508 / 20 | 13,271,122 |

Inputs contain ascending values starting at one, replacing the selected stopping element with zero. The pivot is `n / 2 + 1`. The last three rows use 4,096 items. These are medians of 13 shuffled retained trials after one discarded trial, with one consuming operation per process. The timers exclude input construction, decoding, output serialization and verification; short cases include timer noise. LLVM uses the compiler's matching embedded runtime, pinned 22.1.8 default O3 passes and O3 object emission. Rust uses `opt-level=3`. This is a baseline, not a parity claim: the full large partition is about **2,579× slower than Rust**. [Raw timings](results/timings.csv) · [Environment](results/environment.json)

Separate probes attribute calls and heap requests inside the partition. The 8,193 collection calls request 4,558,160 bytes; the 4,096 classifier calls request 3,448,832 bytes in their own ABI work. The two scalar comparisons per item still use generic binary/variant dispatch. Rust makes 20 allocation/reallocation requests totalling 32,736 bytes for a complete large partition, and no requests for the first-item stop. Its `IntoIter` can reuse the untouched input buffer. [Per-operation requests](results/operations.csv) · [Totals and peak storage](results/allocations.csv)

Full-partition peak extra storage is 561,648 allocator-usable bytes for Villanelle and 16,384 requested live bytes for Rust. These differ in allocator size-class rounding. Both are relative to storage live at entry and exclude preexisting input and allocator-internal transient reallocation storage; neither is RSS. Peak-enabled Villanelle probes reproduce the plain request counts and checked outputs.

The current `vector_take` implementation drains the first slot and shifts the remaining flat vector. A full 4,096-item drain therefore requires 8,386,560 suffix-slot relocations. At the measured 136-byte runtime value width this represents **1,140,572,160 bytes of suffix movement**, before argument/result transport and output buffer growth. This count is derived from the drain algorithm and input size, not a hardware traffic counter. Rust's consuming iterator advances a cursor instead of repeatedly shifting a suffix. Both return 16,384 bytes of logical u32 payload on full completion. [Collection implementation](../../src/collection_support.rs)

A separate sample over 40 complete partitions captures 1,467 stacks inside partition, including 1,315 in collection dispatch. Copy routines appear in 444 stacks, including the drain implementation and allocation growth; these categories overlap. The complete LLVM path still dispatches collection access and classifier calls for each item. The next pass will prove a live, effect-free scalar classifier and move an advancing prefix in bulk, retaining the first stopping element's checked route and ordinary execution for effects, resources, tracing and fuel. [CPU counts](results/cpu.json)

The resource fixture separately verifies stable returned ticket identities, effects through the stopping element, and exactly one close for every owner, including a failed close. Original and expanded execution agrees across interpreter and LLVM/Rust. The scalar caller also has 492 backend comparisons across ordinary execution and 40 fuel budgets in both original and expanded forms. The module remains experimental while native performance, stable ordering and fresh-project adoption are completed.

```sh
python3 -B experiments/partition/measure.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/partition-results
```

Use `--alloc-only` to reuse the timing table while collecting per-operation allocation and peak evidence. Temporary executables and runtime extraction are removed on exit. Compiler/LSP/editor inputs are unchanged by this source-library checkpoint.
