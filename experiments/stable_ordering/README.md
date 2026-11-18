# Stable ordering: complete native path

The experimental source algorithm consumes a vector of records `{u32 key; u32 identity;}`,
compares only keys and returns the complete stable ordering. Identities are original
indices. This compares the entire optimized Villanelle call with Rust's stable
`Vec::sort_by_key`; tuple ordering independently checks the output, including ties.
Input construction, process launch, decoding, checking and output are outside timers.
No primitive or library contract changed to obtain these numbers.

Apple M2, compiler 0.298.0, embedded LLVM 22.1.8, native LLVM O3 and optimized Rust.
See [exact toolchain and procedure](results/0.298-environment.json). One consuming
operation per process, 13 shuffled retained trials after one discarded trial.
Timings are from a shared development machine; compare this run's paired variants.
Empty/single measurements include timer noise. Allocation probes run separately.

| Input | Villanelle | Rust | Ratio |
| --- | ---: | ---: | ---: |
| Empty | 24.29 µs | 42 ns | 578× |
| Single record | 38.50 µs | 42 ns | 917× |
| 128 mixed records | 6.535 ms | 3.459 µs | 1,889× |
| 1,024 mixed records | 81.33 ms | 11.25 µs | 7,229× |
| 1,024 sorted records | 50.00 ms | 2.708 µs | 18,465× |
| 1,024 reversed records | 53.10 ms | 17.08 µs | 3,108× |
| 1,024 equal keys | 50.92 ms | 2.750 µs | 18,515× |

Mixed keys are `(index * 37 + 11) % 31`. Sorted keys are `index / 3`;
reversed keys are `(length - 1 - index) / 3`. All paths verify every returned
key and identity. This is an initial correctness baseline with a large performance
gap, not qualification for a supported library implementation.

## Where the cost goes

The 1,024 mixed-record call requests **622,401 allocations plus 37,578 reallocations**,
**65,792,779 bytes** and **115,856 peak additional allocator-usable bytes**. Rust makes
one allocation for 8,192 requested bytes, also its peak requested storage. Input
storage exists before counting. Villanelle uses the macOS allocator's usable-size
rounding for peak bytes; Rust counts requested bytes, so those peaks are not identical
metrics. The 128-record Rust call needs no heap allocation during sorting.

| ABI operation, mixed input | Calls | Requested bytes |
| --- | ---: | ---: |
| Collection operations | 44,202 | 24,452,110 |
| Reference evidence | 57,931 | 15,312,096 |
| Control calls | 11,995 | 12,472,772 |
| Binary expressions | 15,773 | 4,290,256 |
| Reference formation | 57,931 | 3,734,155 |

The bottom-up algorithm makes 8,913 comparisons. Its use of front drains shifts
523,776 source slots while forming singleton runs, 698,027 run-header slots across
passes and 518,155 element slots inside merges. At the runtime's 136 bytes per
generic slot this is **236,634,288 bytes** of modeled suffix movement. This count
excludes buffer growth, joins, record-field buffers and other transport; it is not a
hardware memory-traffic measurement. O(n log n) comparisons do not imply O(n log n)
physical work with repeated front deletion.

A separate 500-µs CPU sampler over 40 sorts collected 4,678 stacks inside the sort:
2,494 have collection dispatch as their innermost runtime ABI, 333 reference evidence,
308 control calls and 296 dereference. Counts include each ABI's callees.
[CPU counts](results/0.298-cpu.json) · [Movement model](results/0.298-movement.json) ·
[Allocation breakdown](results/0.298-operations.csv) · [Raw timings](results/0.298-timings.csv)

Next: prove effect-free merge traversal and reduce repeated head loans, suffix shifts
and transport without changing authored comparison/fault identities. Keep the
resource/effectful path checked and preserve trace, script and fuel behavior. A
shortcut for already trivial inputs is another measured opportunity. Representation
qualification must account for nested records as well as scalar vectors before
promoting these modules. No backing representation is selected by this baseline.

## Reproduce

```sh
CARGO_INCREMENTAL=0 python3 -B experiments/stable_ordering/measure.py \
  ./bin/vnc ../../work/llvm-22.1.8/install/bin /tmp/vn-ordering-results
```

The driver extracts the compiler's own runtime, wraps only the outer sort call for
timing and ABI allocation counting, checks every output and removes temporary build
artifacts. `--alloc-only` preserves existing timings while collecting allocation data.
The [source caller](../../examples/stable_ordering/README.md) and
[resource/effect regression](../../tests/fixtures/owned_ordering.vn) exercise the same
library. The source-only checkpoint reuses the qualified 0.298 compiler/LSP/editor;
it does not produce a new compiler package.

## Bulk merge traversal in 0.299

The compiler now proves all three comparator results and both ownership transfers
in an advancing merge. Equal records must come from the left run first. For the
first profile, every field of each participating record must be u32. The runtime
merges the proven prefix, compacts each input suffix once and leaves exhaustion and
joins to the original graph. It accounts for one skipped comparator activation per
decision, preserving the identities of later faults. Effects, resources, unsupported
shapes, retained execution, scripts, traces and fuel limits keep checked execution.

The paired M2 run uses each compiler's own IR and matching runtime, the unchanged
source algorithm and the same optimized Rust baseline. Shared-machine load differs
from the earlier baseline; compare columns within this run.

| Input | 0.298 | 0.299 | Rust | Speedup |
| --- | ---: | ---: | ---: | ---: |
| Empty | 15.33 µs | 14.21 µs | 41 ns | no merge work |
| Single | 22.67 µs | 25.25 µs | 42 ns | no merge work |
| 128 mixed | 3.983 ms | 1.065 ms | 2.375 µs | 3.74× |
| 1,024 mixed | 47.34 ms | 10.32 ms | 7.042 µs | 4.59× |
| 1,024 sorted | 30.70 ms | 10.19 ms | 1.708 µs | 3.01× |
| 1,024 reversed | 34.23 ms | 10.79 ms | 11.50 µs | 3.17× |
| 1,024 equal | 31.58 ms | 9.764 ms | 1.708 µs | 3.23× |

Empty and single inputs invoke no merge; their allocation counts are unchanged and
the timing differences do not demonstrate an improvement from this lowering.
The complete mixed sort remains **1,466× Rust**. Allocations fall from 622,401 to
91,749 and reallocations from 37,578 to 656. Requested bytes fall from 65,792,779 to
12,929,355; peak extra usable storage falls from 115,856 to 114,336 bytes. Rust still
uses one 8,192-byte allocation. Peak accounting retains the definitions above.

Comparison count remains 8,913. Modeled merge suffix shifts fall from 518,155 slots
to 499. Singleton creation and run-header drains still shift 1,221,803 slots, leaving
166,233,072 bytes of modeled total suffix movement, versus 236,634,288 before. The
current algorithm still has quadratic movement outside the optimized merge.

The new CPU sample contains 537 in-sort stacks: 205 in collection dispatch, 152 in
bulk vector transfer, 77 in local access and only nine in the merge helper. Next,
reduce run construction and run transport, then qualify representations and supported
library adoption. The scalar and resource contracts stay the same.

Validation covers each comparator branch's effects, unstable equal routing (which
must reject the optimization), empty/odd/duplicate/extreme inputs, original and
expanded LLVM O2/O0 versus Rust, 60 fuel cuts, script/JSON modes and a fault after
sorting. ABI cases cover preexisting output, both depletion directions, ties, aliasing,
bad projections and types, resource-bearing records and observer fallback. The
existing resource sort regression passes with the new compiler. All 566 selected
Rust tests and 48 editor tests passed. The packaged CLI/LSP and loopback HTTP service
passed; both Linux targets cross-link the helper to ELF, with no Linux execution
claim on this M2.

[Paired timings](results/0.299-timings.csv) · [Allocation breakdown](results/0.299-operations.csv) ·
[Summary](results/0.299-summary.json) · [Movement](results/0.299-movement.json) ·
[CPU samples](results/0.299-cpu.json) · [Toolchain](results/0.299-environment.json)

To repeat a paired comparison, add `--previous-vnc /path/to/previous/vnc` to the
command above. The driver builds each compiler's own IR and uses its own runtime.


## Singleton run construction in 0.300

The compiler proves an effect-free loop that takes one owner, constructs its
singleton vector, appends that vector and continues with the remaining source.
The runtime transfers all elements in one traversal, preserving arbitrary owners,
including resources and empty records. The checked graph still handles exhaustion
and terminal effects. Other constructor shapes, advancing effects, retained execution,
scripts, tracing and fuel keep ordinary execution. No comparator calls are skipped.

Each compiler uses its own IR and runtime in this paired M2 run, with unchanged
source and the same Rust baseline. The methodology and peak-memory definitions above
apply; absolute times from separate runs should not be compared.

| Input | 0.299 | 0.300 | Rust | Speedup |
| --- | ---: | ---: | ---: | ---: |
| Empty | 19.54 µs | 19.79 µs | 42 ns | no elements transferred |
| Single | 31.17 µs | 25.25 µs | 42 ns | 1.23× |
| 128 mixed | 1.348 ms | 1.102 ms | 2.792 µs | 1.22× |
| 1,024 mixed | 13.37 ms | 10.06 ms | 9.042 µs | 1.33× |
| 1,024 sorted | 13.05 ms | 9.722 ms | 2.167 µs | 1.34× |
| 1,024 reversed | 13.55 ms | 10.37 ms | 14.33 µs | 1.31× |
| 1,024 equal | 12.51 ms | 9.137 ms | 2.208 µs | 1.37× |

The mixed call makes 68,197 allocations and 648 reallocations, down from 91,749
and 656. Requested bytes fall from 12,929,355 to 11,202,411. **Peak additional
allocator-usable memory increases from 114,336 to 442,016 bytes**. This memory
regression remains an investigation target; reduced request counts do not establish
lower peak storage. Rust uses one 8,192-byte allocation. The complete mixed sort
remains **1,113× Rust**, so this is an incremental optimization, not parity.

Singleton formation now shifts no source suffixes, removing 523,776 slot shifts.
There are still 698,027 run-header shifts and 499 merge suffix shifts, totaling
94,999,536 modeled bytes at 136 bytes per generic value. This excludes buffer growth
and other transfers. Comparison count remains 8,913. A separate CPU sample has
799 in-sort stacks: 300 in control calls, 187 in vector transfer, 120 in collection
operations, 82 in merge and six in singleton construction. Counts include callees.

All 568 selected compiler/runtime/manual regressions and 48 editor tests pass.
Focused original/expanded LLVM O2/O0 and Rust comparisons cover effects, constructor
shape, terminal effects, scripts, 45 fuel budgets and subsequent fault identities.
The ABI regression transfers real resources exactly once and tests fallback owners;
the end-to-end resource sorting regression also passes. Packaged CLI/LSP, cold Hello
and HTTP qualification pass. Both Linux targets cross-link ELF executables using the
new helper; those executables were not run on this M2.

Next: isolate the peak-memory increase and remaining run-header/terminal transport,
then qualify representations, applicable ordered bounds and supported-library adoption.
[Raw timings](results/0.300-timings.csv) · [Allocations](results/0.300-allocations.csv) ·
[ABI breakdown](results/0.300-operations.csv) · [CPU](results/0.300-cpu.json) ·
[Movement](results/0.300-movement.json) · [Environment](results/0.300-environment.json)


## Fixed payload compaction in 0.301

The 0.300 peak increase has a concrete cause. JSON decoding gives a two-u32
record four field slots (640 allocator-usable bytes). The former checked Copy
operation cloned it into two slots (320 usable bytes). Bulk moving retained the
original reserve. The 320-byte difference per record accounts for the entire
327,680-byte end-of-singletons offset. Independent paired control scopes show
identical downstream merge/pass/flatten allocations and memory deltas.
[Layout and scoped evidence](results/0.301-capacity-root-cause.json)

The runtime now compacts fixed-arity field buffers before reserving the output runs.
It preserves the original owners and requires no clone capability. Resources are
neither duplicated nor completed; growable vector and map payloads retain their
useful reserve. Compacting inside the transfer loop was also measured, but retained
a larger temporary overlap. Compacting before output reservation restores the prior
peak while retaining bulk transfer. This is a memory improvement with a measured
allocation and time tradeoff, not a speed improvement.

| Input | 0.300 | 0.301 | Rust |
| --- | ---: | ---: | ---: |
| Empty | 23.29 µs | 23.00 µs | 42 ns |
| Single | 26.83 µs | 27.58 µs | 42 ns |
| 128 mixed | 1.178 ms | 1.168 ms | 3.000 µs |
| 1,024 mixed | 10.95 ms | 11.44 ms | 10.54 µs |
| 1,024 sorted | 10.53 ms | 10.68 ms | 2.458 µs |
| 1,024 reversed | 11.42 ms | 11.96 ms | 15.92 µs |
| 1,024 equal | 9.956 ms | 10.43 ms | 2.291 µs |

These are final paired compiler packages, each with its matching IR/runtime, on the
shared M2. The earlier runtime-only probe's large small-input timing regression did
not reproduce in this comparison. The complete mixed call is about **4.5% slower**
and still **1,085× Rust**. Median changes are observations, not significance claims.
Peak extra usable memory falls **442,016 → 114,336 bytes (74%)**. Allocations stay
at 68,197; reallocations rise from 648 to 1,672 and requested bytes from 11,202,411
to 11,480,939. Shrinking still requests replacement space; total requested bytes and
peak live storage measure different things. Rust uses one 8,192-byte allocation.
The 128-record peak falls from 69,280 to 28,320 bytes.

Source movement and comparison count are unchanged: 94,999,536 modeled suffix
bytes and 8,913 comparisons. Of 580 in-sort CPU samples, 201 are in collection
operations, 155 in vector transfer, 49 in control calls, 43 in merge and two in
singleton construction. Next, address run-header draining and terminal transport,
then representation qualification, applicable ordered bounds and supported modules.

All 569 selected Rust regressions and 48 editor tests pass. New ABI cases cover
resource-bearing fixed records and arrays, exact owner completion, and preserved
vector/map reserves. Existing source-level effect, resource and observer tests pass.
Packaged CLI/LSP, cold Hello and settings HTTP pass; both Linux targets cross-link
with the helper and were not executed on this M2. Existing windows were not reloaded.
[Timings](results/0.301-timings.csv) · [Allocations](results/0.301-allocations.csv) ·
[Operations](results/0.301-operations.csv) · [CPU](results/0.301-cpu.json) ·
[Environment](results/0.301-environment.json)


## Complete join call transport in 0.302

The compiler now proves the complete consuming join control: argument-to-loop
mapping, each advancing step and the empty terminal outcome. A guarded runtime
call transfers both owners once without building a child context and executing
its terminal graph. Recognition is independent of names. Entry, advancing or
terminal effects disqualify the summary; observed and unsupported calls execute
the original callback. Completed frame and activation identities are preserved.

| Input | 0.301 | 0.302 | Rust |
| --- | ---: | ---: | ---: |
| Empty | 16.38 µs | 14.54 µs | 41 ns |
| Single | 19.83 µs | 15.96 µs | 42 ns |
| 128 mixed | 860.5 µs | 673.6 µs | 2.333 µs |
| 1,024 mixed | 7.773 ms | 6.278 ms | 7.125 µs |
| 1,024 sorted | 7.543 ms | 5.717 ms | 1.917 µs |
| 1,024 reversed | 7.926 ms | 6.182 ms | 11.42 µs |
| 1,024 equal | 7.219 ms | 5.708 ms | 1.667 µs |

These are paired M2 medians from 13 shuffled retained trials, with each compiler's
own IR and runtime. Compare within this run; the shared host and observed outliers
make cross-run absolute timings unsuitable. The mixed call is 19.2% faster, but
still **881× Rust**. Empty input executes no join, so its timing change is not
attributed to this optimization.

Mixed allocations fall **68,197 → 51,821**, reallocations **1,672 → 1,670**,
requested bytes **11,480,939 → 8,950,650**, and peak extra allocator-usable bytes
**114,336 → 108,128**. Rust uses one 8,192-byte allocation. Comparison order and
payload/suffix movement are unchanged; the earlier model still gives 8,913
comparisons and 94,999,536 suffix bytes. This pass removes call transport, not
run-header shifting. Of 288 in-sort CPU samples, 87 are in collection operations,
69 in join, 57 in merge and 34 in outcome routing, including their callees.

All 571 selected compiler/runtime/manual cases and 48 editor cases pass. The new
ABI test covers resource identity/order, both parameter positions, empty inputs,
invalid/aliased handles and checked fallback. Original/expanded LLVM O2/O0 versus
Rust tests cover renamed controls/outcomes, entry/advancing/terminal effects,
scripts, 65 fuel limits and subsequent fault identities. Existing resource sorting
and split/join tests pass. Packaged CLI/LSP, cold Hello and settings HTTP qualify;
both Linux targets cross-link the new helper but were not executed on the M2.

Next: reduce measured run-header draining and remaining generic transport, qualify
representations and applicable ordered bounds, then fresh-project adoption and
supported modules. [Paired timings](results/0.302-timings.csv) ·
[Allocation totals](results/0.302-allocations.csv) ·
[ABI attribution](results/0.302-operations.csv) · [CPU](results/0.302-cpu.json) ·
[Environment](results/0.302-environment.json)
