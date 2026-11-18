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

## Where the remaining batch time goes

The next profile confirms `size_of::<V>() == 136` on this arm64 host, compared
with four bytes for `u32`. A plain 136-byte record performs similarly to the
runtime enum. The split allocates 1,365 values at 4,096 inputs: **185,640 bytes**,
or 94.1% of the complete operation's 197,302 requested bytes. Join reuses the
original buffer's capacity. The other ABI requests total 11,662 bytes at both
128 and 4,096 inputs.

Timing the bulk calls separately in 0.293 attributes about 31 µs to transfers
and 24 µs to the remaining control work at 4,096 inputs. This instrumentation
adds clock reads, so its totals are not substituted for the uninstrumented
comparison above. A separate repeated-call CPU sample found 378 of 470 samples
inside bulk transfer: 252 in `memmove` and 124 in Rust's `extend_trusted` loop.
The caller's checked loop is excluded from these sampled stacks. Small batches
spread their remaining cost across call transport, allocation, locals, length,
comparison, reference evidence and terminal outcomes.

The isolated kernel comparison uses the actual runtime enum, a fully checked
136-byte plain record, and Rust `u32`. Inputs are created before timing and every
output element is checked afterwards. Each result is a median of 13 shuffled
trials after one discarded trial. “Repeated” averages 256 operations in one
process, with fresh input storage for each operation. Runtime-matching O2,
debug assertions and overflow checks are enabled. An O3 build gives similar
results. Allocation instrumentation runs separately.

| 4,096 runtime values | One operation | Repeated, per operation | Requested bytes |
| --- | ---: | ---: | ---: |
| Existing prefix drain/extend, then append | 31.0 µs | 19.3 µs | 185,640 |
| Contiguous prefix copy, then append | 25.4 µs | 14.5 µs | 185,640 |
| Split off suffix, then extend | 41.9 µs | 9.61 µs | 1,114,248 |
| In-place rotation | 27.0 µs | 27.7 µs | 0 |

The scalar prefix kernel takes 1.42 µs once and 0.335 µs in the repeated process.
The backing payload width remains a substantial gap. The suffix strategy wins
in the repeated process but requests six times the memory and loses on a single
operation. In-place rotation is a diagnostic comparison: it does not implement
the two independent split and join calls.

Generated arm64 assembly shows the old large-value extension looping over
136-byte loads and stores. The contiguous candidate uses one prefix `memcpy`
and one suffix `memmove`, followed by the existing append. Both prefix kernels
explicitly transfer 5,461 elements, or 742,696 payload bytes, at this size; the
candidate changes how the prefix is copied, not its ownership or volume.
Regression checks cover non-Clone owners with 128-byte alignment, every split
position up to length 32, populated and empty destinations, spare capacity,
zero-sized destructors, invalid bounds, and exactly one destruction per owner.

The released 0.294 runtime was then compared with 0.293 using **identical LLVM
IR and the same timed object**, linked separately to each runtime archive.
Both executables and the Rust baselines were shuffled together for 13 measured
trials after one discarded trial. This controls for the differences between
separate benchmark batches.

| Complete rotation | Runtime 0.293 | Runtime 0.294 | Rust flat |
| --- | ---: | ---: | ---: |
| 128 u32 items | 29.92 µs | 30.42 µs | 2.25 µs |
| 4,096 u32 items | 59.50 µs | 54.75 µs | 4.63 µs |

The large operation improves **8.0%**. There is no small-batch improvement in
this run. Both versions still request 42 allocations and three reallocations;
the large request volume remains 197,302 bytes. The remaining large-case gap
is **11.8×** against Rust flat storage. The next targets are temporary reference
formation and control transport, alongside the established checked-arithmetic
fold gap. Payload width remains open before selecting a tree representation.
In the follow-up 0.294 CPU sample, 292 of 366 large-batch samples remain inside
bulk transfer. The old extension loop disappears from the sampled leaves;
the platform memory-copy routine now dominates the remaining payload work.

[Isolated timings](results/layout.csv) · [Allocation and peak memory](results/layout-alloc.csv) ·
[Compiler settings](results/layout-environment.json) · [0.293 transfer timing](results/transfer-time-0293.csv) ·
[0.293 CPU sample counts](results/batch-cpu-0293.json) ·
[0.294 CPU sample counts](results/batch-cpu-0294.json) ·
[Identical-IR runtime comparison](results/native-0294-paired.csv) ·
[Paired allocation counts](results/native-0294-paired-alloc.csv)

## Fresh collection length reads in 0.295

LLVM now reads a collection's length directly when its operand is a fresh
borrow of an owned place and its checked origin is exactly that place. The
read retains the frame, slot, projection, collection-type and u32-range checks.
It needs no temporary reference or reference evidence. Received loans,
indirect places, guarded origins and more complex dependencies keep their
ordinary lowering. Length queries also validate generic type shape without
allocating owned type strings or a temporary argument vector.

The same rotation comparison interleaves 0.294 and 0.295 with the Rust
baselines. Here each compiler supplies its own LLVM IR because the lowering
changed. Every executable verifies the same rotated IDs. The median of 13
shuffled trials, after one discarded trial, is:

| Complete rotation | Compiler 0.294 | Compiler 0.295 | Rust flat |
| --- | ---: | ---: | ---: |
| 128 u32 items | 28.63 µs | 21.46 µs | 2.13 µs |
| 4,096 u32 items | 56.92 µs | 44.71 µs | 4.58 µs |

The improvements are **25.0%** and **21.5%**. The large-case gap remains
**9.75×**. Allocation requests fall from 42 to **34**, reallocations from three
to **two**, and requested bytes from 197,302 to **195,608**. Both direct length
calls make zero allocation requests. Join's allocation profile is unchanged.
The payload still occupies 136 bytes per element and moves the same 742,696
bytes; this pass removes temporary reference and parsing work.

A separate peak probe tracks live allocator-usable bytes relative to each
control's entry. For the large split, peak extra live storage falls from
202,656 to 202,016 bytes; join remains 1,216 bytes. At 128 items the split falls
from 12,192 to 11,552 bytes. These figures include allocator size-class rounding,
exclude input storage already live at entry, and exclude allocator-internal
reallocation transients and RSS. They are not summed across controls. Enabling
peak accounting preserves every request-count row from the ordinary probe.

Regression coverage compares authored and expanded LLVM execution with Rust
and the interpreter, including 25 fuel cuts, effects and received borrows.
Direct ABI cases preserve opaque owners and match the original projection and
invalid-slot/type errors. Differential parser cases include nested generics,
array-size boundaries, malformed delimiters, whitespace and UTF-8. The existing
split/join, collection, origin and resource cases remain part of qualification.
The owned fold contains no length expression, so its checked-arithmetic gap is
unchanged and its existing evidence is retained for the next pass.

[Paired timings](results/native-0295-paired.csv) ·
[Compiler settings](results/native-0295-paired-environment.json) ·
[Allocation requests](results/native-0295-paired-alloc.csv) ·
[ABI attribution](results/length-0295-abi.json) ·
[Peak live storage](results/native-0295-peaks.csv)

## Reproduce

From the repository root:

```sh
python3 -B experiments/sequence_storage/run.py /tmp/sequence-results
python3 -B experiments/sequence_storage/native.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/sequence-results
python3 -B experiments/sequence_storage/layout.py /tmp/sequence-layout
python3 -B experiments/sequence_storage/transfer_time.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/sequence-profile
python3 -B experiments/sequence_storage/sample.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/sequence-profile
python3 -B experiments/sequence_storage/peaks.py \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/sequence-peaks.csv ./bin/vnc
./bin/vnc exec examples/rotate_batch/main.vn -- \
  --scenario examples/rotate_batch/main.scenario.json
```

The scripts use temporary build directories and remove their binaries after completion. Timings use an uninstrumented Rust binary. Allocation builds share the existing ordered-map allocator probe. The native probe uses the current compiler's embedded runtime archive, with LLVM 22.1.8 O3 passes and object emission.

An optional fourth argument to `native.py` supplies a previous compiler. The
script requires both compilers to emit identical IR, links the same timed
object against each embedded runtime, and interleaves both binaries in its
trial schedule. Run timings without concurrent compiler or packaging jobs.
When comparing changes to LLVM emission, add `--previous-ir`: each compiler
then contributes its own timed object and matching runtime. The environment
record identifies which comparison was used.
