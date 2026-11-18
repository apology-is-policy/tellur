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
