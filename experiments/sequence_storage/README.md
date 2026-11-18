# Consuming split and join on Apple M2

The source prototype consumes both inputs to join and consumes the source to split. An invalid split returns the original vector. On success, every element belongs to exactly one returned vector, in its original order. The runnable batch example moves a prefix of pending request IDs to the back. [Source controls](../../library/sequence_batches.vn) · [Caller](../../examples/rotate_batch/main.vn)

The storage experiment compares Rust's flat `Vec<T>`, its consuming `IntoIter<T>` as a moving-start baseline for front draining, and an exclusively owned relaxed radix tree. Both split/join implementations reuse an entire owner at an empty boundary. The tree has 32-wide leaves, prefix-size tables, a radix estimate for indexing and 16–32 slots per non-root node. It repairs boundary occupancy during split and join. The prototype uses unique ownership, Vec-backed nodes and immediate boundary repair. Persistent snapshots, focused tails and a hybrid regular-node layout require further experiments. The original RRB work describes cumulative sizes and rebalancing for efficient concatenation. [Bagwell and Rompf](https://hypirion.com/pdf/RMTrees.pdf) · [Prototype](tree.rs)

## Timings

Rust 1.97.1, `-C opt-level=3`, arm64 macOS on Apple M2. Each cell is the median of 13 shuffled trials, after one discarded trial. The table reports microseconds for a complete workload on 65,536 elements.

| Workload | Flat, 8-byte items | Tree, 8-byte items | Flat, 64-byte items | Tree, 64-byte items |
| --- | ---: | ---: | ---: | ---: |
| Append all items | 77.2 | 1,131 | 598 | 1,838 |
| Scan all IDs | 6.75 | 126 | 55.3 | 159 |
| Pseudorandom index, N reads | 68.9 | 1,447 | 149 | 1,810 |
| Split and rotate, 128 times | 1,906 | 296 | 17,500 | 323 |
| Join 256-item batches | 50.4 | 155 | 508 | 146 |
| Drain from the front | 300,783 | 39,758 | 2,300,995 | 42,079 |

The moving-start flat buffer drained all 65,536 items in **32.0 µs** for 8-byte items and **126 µs** for 64-byte items. It made zero allocation requests during the drain. Its original allocation stays live until the iterator is dropped.

For 128 repeated splits and joins, the tree leads by **6.4×** at 65,536 eight-byte items and **54×** at 64-byte items. At 128 items the flat vector leads, taking 29.3 µs against 93.7 µs for eight-byte items. At 4,096 eight-byte items the pair takes 293 µs and 209 µs. For joining pre-existing batches, the tree wins on large records and pays overhead on small records.

Each process constructs its inputs before timing, except in the append workload. Scan and index read the ID word of each record. Full records move in append, split/join and drain. Every run verifies sequence order and all words in every surviving record, or the final drain checksum. Input/output go through `black_box`. Short, single-scan timings are subject to clock granularity and cache state. [Raw timings](results/timing.csv) · [Workloads](main.rs)

## Allocation and element movement

Separate instrumented builds count allocator calls, requested bytes, peak extra live bytes, explicit payload transfers and elements exposed to buffer growth. Metadata and child-pointer movement are outside the payload counter. Growth exposure records how many live elements *could* move if realloc cannot extend in place. It is separate from explicit moves.

For the 128 split/rotate operations on 65,536 eight-byte items:

| Measurement | Flat | Tree |
| --- | ---: | ---: |
| Allocator requests, including realloc | 254 | 3,011 |
| Cumulative requested bytes | 115,172,208 | 946,496 |
| Peak extra live bytes above input storage | 1,051,216 | 1,168 |
| Explicit element transfers | 8,323,072 | 3,855 |
| Elements exposed to buffer growth | 4,082,752 | 949 |

The tree reduces payload traffic while allocating more small nodes. Its single-item front drain currently requests 632,181 allocations and 231 MB cumulatively for 65,536 eight-byte items. That cost follows from repeatedly splitting and rebuilding the left boundary. The moving-start buffer advances through its allocation.

Appending 65,536 eight-byte values peaks at 524,288 extra live bytes for the flat vector and 1,566,656 for the tree. These measurements support further tree work for workloads dominated by rearranging large batches. Append, scans, random access and front drains each favor the flat storage or cursor baseline in this experiment. [All allocation and movement counts](results/counts.csv)

## Complete Villanelle operation

The runnable source uses the existing checked `vector_take` and `vector_push` operations. A second comparison times only the outer split and join calls in its LLVM O3 build and checks the returned IDs. It uses u32 elements in Villanelle and both Rust baselines.

| Rotate the first N/3 items to the end | Villanelle 0.292 | Rust flat | Rust tree |
| --- | ---: | ---: | ---: |
| N = 128 | 161 µs | 2.38 µs | 1.79 µs |
| N = 4,096 | 18.24 ms | 4.92 µs | 3.67 µs |

The Villanelle 4,096-item case makes 43,722 malloc/calloc requests plus 12 reallocations and requests 4,321,924 bytes. Rust flat makes two allocator requests totalling 32,772 bytes. The tree makes 17 totalling 4,320 bytes. The source loop's repeated front extraction shifts the flat collection's remaining elements. Native bulk split/join is the next compiler target before comparing the candidate representation through the same controls. [Native timing data](results/native.csv) · [Native allocation data](results/native-alloc.csv)

Original and expanded interpreter, LLVM and Rust execution agree on normal, empty, boundary and invalid splits. Resource tests return each opaque handle exactly once. The checker rejects discarded resources and consuming a vector while a returned element loan remains live. The tree tests rotate non-Clone owners, check one destruction per owner, audit node occupancy and size tables, and exercise a forest of interleaved append/split/join operations. [Module tests](../../tests/modules.rs)

## Native bulk transfer in 0.293

Optimized LLVM builds recognize a complete iteration that takes the front item from an owned vector and appends it to another owned vector. A comparison with the destination length can bound the transfer. The runtime moves the entire proven prefix in bulk. The original graph handles the final bound or empty outcome. Resource owners keep their destination. A loop with iteration effects retains checked execution, as do traced, scripted and fuel-limited invocations.

A fresh same-session comparison used the installed 0.292 compiler and the packaged 0.293 compiler, each in its own batch of 13 shuffled trials after one discarded trial. Rust was rebuilt with the same settings. These are complete u32 rotations, timing the two outer controls.

| Items | Villanelle 0.292 | Villanelle 0.293 | Rust flat in the 0.293 batch | Rust tree |
| --- | ---: | ---: | ---: | ---: |
| 128 | 152.5 µs | 31.9 µs | 2.29 µs | 1.88 µs |
| 4,096 | 17.17 ms | 62.6 µs | 4.79 µs | 3.08 µs |

The 4,096-item rotation improved **274×**. It still takes **13.1×** the Rust flat baseline. Its 43,722 malloc/calloc requests fell to **42**, reallocations fell from 12 to **3**, and requested bytes fell from 4,321,924 to **197,302**. The allocation count now stays constant between 128 and 4,096 items. Generic value storage, bulk payload movement and control-call transport remain candidates for the next profile. Short single-operation measurements vary with clock granularity and cache state.

The LLVM regression compares original and expanded programs with and without iteration effects, empty and nonempty inputs, valid boundaries and invalid positions, live and scripted execution, and 65 fuel limits. The ABI regression covers resource tokens and bounds both below and above the initial destination length. The existing collection ownership and module conformance tests also pass. [0.292 timings](results/native-0292-paired.csv) · [0.292 allocations](results/native-0292-paired-alloc.csv) · [0.293 timings](results/native-0293.csv) · [0.293 allocations](results/native-0293-alloc.csv)

## Reproduce

From the repository root:

```sh
python3 -B experiments/sequence_storage/run.py /tmp/sequence-results
python3 -B experiments/sequence_storage/native.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/sequence-results
./bin/vnc exec examples/rotate_batch/main.vn -- \
  --scenario examples/rotate_batch/main.scenario.json
```

The scripts use temporary build directories and remove their binaries after completion. Timings use an uninstrumented Rust binary. Allocation builds share the existing ordered-map allocator probe. The native probe uses the current compiler's embedded runtime archive, with LLVM 22.1.8 O3 passes and object emission.
