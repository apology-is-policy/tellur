# Complete ordered-query baseline

Compiler 0.305.0, Apple M2, LLVM 22.1.8 `opt default<O3>` and `llc -O3`,
Rust `opt-level=3`. Each input is an already sorted vector of u32 values `i / 3`.
The independent oracle is Python `bisect_left`/`bisect_right`; Rust also checks
against a linear traversal outside timing. Every native result is checked.

Run on arm64 macOS, from the checkout root:

```sh
python3 -B experiments/ordered_bounds/measure.py ./bin/vnc \
  ../../work/llvm-22.1.8/install/bin /tmp/vn-bounds-results
```

Thirteen retained shuffled trials follow one discarded trial. The timed boundary
includes the complete public library call, its comparator and internal helper
calls. Input construction, command-line decoding and output checks are excluded.
Each observation is one query in a fresh process; small Rust times include clock
resolution and cold-call noise. This is a baseline, not a steady-state throughput
or significance claim. Allocation counting uses separate binaries and a separate
peak-memory pass; timing does not include those counters.

| Operation / case | Villanelle median | Rust median | VN allocations + reallocations | VN requested / peak-extra bytes |
| --- | ---: | ---: | ---: | ---: |
| Lower / empty | 16.208 µs | 42 ns | 25 + 3 | 8,584 / 6,096 |
| Lower / 128 middle | 80.667 µs | 83 ns | 248 + 19 | 38,048 / 11,872 |
| Lower / 4,096 middle | 114.125 µs | 208 ns | 403 + 29 | 54,028 / 11,872 |
| Upper / 4,096 middle | 103.166 µs | 250 ns | 422 + 29 | 56,781 / 11,872 |
| First equal / 4,096 middle | 128.000 µs | 250 ns | 454 + 34 | 63,481 / 14,960 |
| First equal / 4,096 miss | 123.042 µs | 292 ns | 416 + 31 | 59,395 / 14,768 |

Rust makes zero allocations in every measured query. Its baseline uses
`slice.partition_point`; first-equivalent search adds `get` and equality checking.
Both queries perform zero source element moves. Returning a found scalar in the
runner happens outside the Villanelle timing boundary; Rust copies that one scalar
inside its query, so this does not give Villanelle extra measured work. Neither
algorithm owns or rebuilds the source during the timed operation. Runtime u32
payload slots are 136 bytes versus Rust's 4-byte u32; metadata transport remains.

The 4,096-middle lower bound performs twelve borrowed element reads and twelve
comparisons. Its generated IR still routes those through the generic ABI. Scoped
allocation attribution is:

| ABI operation | Calls | Allocations | Reallocations | Requested bytes |
| --- | ---: | ---: | ---: | ---: |
| Reference formation | 52 | 128 | 2 | 5,678 |
| Collection read | 12 | 96 | 12 | 6,648 |
| Nested control call | 13 | 53 | 0 | 14,420 |
| Outcome routing | 29 | 42 | 0 | 2,630 |
| Reference evidence | 52 | 36 | 12 | 6,432 |
| Binary expression | 27 | 27 | 0 | 7,344 |
| Completion | 14 | 16 | 0 | 358 |
| Outer call | 1 | 5 | 0 | 1,270 |
| Frame growth | 14 | 0 | 3 | 9,248 |

Those totals reconcile to 403 allocations, 29 reallocations and 54,028 bytes.
The gap is generic reference, comparator and control transport on a logarithmic
algorithm, not element movement or an accidental linear scan. Live checked
arithmetic already uses the direct scalar lowering from 0.296. The next performance
pass should prove the effect-free binary-search stepping graph and keep its scalar
state/comparands native, retaining checked fallback for resources, effects,
observations, scripts and unsupported comparison shapes. Do not recognize a
shortcut by library or control name, or assume ordering laws from an interface.

Raw timings, allocations, ABI attribution and environment are in [results](results/).
Emitted IR is retained in the maintained work evidence directory. Temporary native
binaries/runtime extraction are removed by the harness. The source library remains
experimental; this checkpoint makes no parity claim and does not change the compiler.
