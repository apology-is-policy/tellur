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

## Bulk scalar partition in 0.298

Optimized LLVM now proves both advancing branches of an effect-free u32 classifier and the consuming source loop. One runtime call scans the prefix up to the first sentinel, reserves each destination once, and moves that prefix into the two outputs in stable order. The first stopped element still executes the authored classifier, including terminal effects or a fault. A single prefix drain replaces repeated front drains. The ordinary terminal take can still shift its remaining suffix once. A resource element, an effect on either advancing branch, tracing, scripts or a fuel limit retains checked execution.

| Workload | 0.297, paired | 0.298 | Rust |
| --- | ---: | ---: | ---: |
| Empty | 21.46 µs | 21.54 µs | 83 ns |
| Full, 128 | 857 µs | 32.33 µs | 4.92 µs |
| Full, 4,096 | 75.76 ms | 174.25 µs | 24.92 µs |
| Stop on first | 70.83 µs | 55.08 µs | 833 ns |
| Stop at index 2,048 | 56.15 ms | 141.83 µs | 11.04 µs |
| Stop on last | 89.16 ms | 172.21 µs | 25.54 µs |

The complete large partition improves **435×** and remains **7.0× slower than Rust**. The small partition improves 26.5×. Empty input has no improvement. First-stop execution batches no elements and its heap counts are unchanged; do not attribute its timing difference to bulk work. Both 0.297 and Rust were slower than in the preceding baseline run, and the shared machine was under load. Compare variants within this paired run. The same 13 retained shuffled trials and checked outputs are used, with each compiler's own IR and matching runtime. Local tests, release construction and package qualification finished before measurement. [Paired timings](results/0.298-timings.csv) · [Environment](results/0.298-environment.json)

The complete large partition falls from **106,517 allocations and 19 reallocations to 21 allocations and one reallocation**. Requested bytes fall from 13,270,445 to 561,645. The prefix helper allocates two output buffers containing 557,056 bytes of generic values. Peak extra allocator-usable storage changes only slightly, from 561,648 to 559,872 bytes: this pass eliminates repeated shifts and transport, not the output storage. Rust requests 32,736 bytes and has 16,384 extra requested live bytes; its element representation is four bytes versus Villanelle's 136-byte generic value. The peak probes use the same baseline and rounding limits documented above and reproduce ordinary counts and outputs. [Allocation and peak totals](results/0.298-allocations.csv) · [Per-operation costs](results/0.298-operations.csv)

A full advancing prefix no longer shifts any source suffix: it transfers 4,096 generic values once to the outputs, representing 557,056 payload bytes, excluding allocator bookkeeping and loop-terminal transport. This removes the previous 1.14 GB of repeated suffix relocation. This is an algorithmic movement count, not measured hardware traffic. Early stopping retains a checked terminal take and its wide suffix movement. Generic value width, fixed call/outcome transport and that terminal path are the remaining costs.

A separate 6,000-partition CPU profile captures 470 stacks inside the partition. The prefix helper appears in 376, collection dispatch in ten. Joining the two outputs and repeating the operation runs outside the sampled partition. The old collection-dispatch hot loop has been replaced; native payload traversal and output writes now dominate the sampled helper. [CPU counts](results/0.298-cpu.json)

Regression coverage compares original/expanded optimized LLVM with unoptimized LLVM and Rust for stable ordering, sentinel positions, u32 limits, effects on all or only one advancing branch, terminal effects and terminal faults. Sixty fuel budgets and scripted/JSON execution agree byte for byte. A concrete u32 fault fixture preserves activation identities; the generic library itself cannot fault while arbitrary resource owners remain live. ABI cases verify preexisting outputs, aliased/invalid operands and observed-mode fallback. Resource ownership retains the existing ordinary path. Both Linux target packs link the actual new helper to ELF; those binaries were not executed on the M2.

The next library step is the prepared nonrecursive stable merge-sort prototype, followed by representation qualification and fresh-project adoption. The measured remaining gaps stay open during those passes. Repeat this comparison by adding `--previous-vnc /path/to/0.297/vnc` to the command above.
