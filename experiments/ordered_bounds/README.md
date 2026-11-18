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

## Scalar binary-search advances in 0.306

The compiler proves the scalar comparator's three outcomes, then all six advancing
paths (before/equal/after × lower/upper). It does not recognize a library/control
name or assume sortedness. A guarded runtime helper keeps start/count and the u32
comparands native, preserving one completed comparator activation/frame identity
per skipped advance. The terminal iteration remains authored code. Reference
root/projection evidence, actual value shape, interval bounds and canonical live
arithmetic are validated before updating state. Unsupported values, resource-bearing
elements, effects, scripts, tracing and fuel retain checked execution. An unsorted
input still follows the original algorithm; optimization does not add an ordering
assumption or promise a valid bound for such an input.

Final paired packaged compilers use their own IR/runtime, with the same source,
inputs, optimization settings and measurement boundary as the baseline:

| Operation / case | 0.305 | 0.306 | Rust | Speedup |
| --- | ---: | ---: | ---: | ---: |
| Lower / 128 middle | 83.291 µs | 21.125 µs | 83 ns | 3.94× |
| Lower / 4,096 middle | 123.834 µs | 25.666 µs | 167 ns | 4.82× |
| Upper / 4,096 middle | 106.334 µs | 23.667 µs | 209 ns | 4.49× |
| First equal / 4,096 middle | 134.250 µs | 48.834 µs | 209 ns | 2.75× |
| First equal / 4,096 miss | 140.833 µs | 30.125 µs | 292 ns | 4.67× |

The 4,096-middle lower bound falls from 403 to 25 allocations and 29 to 3
reallocations. Requested bytes fall 54,028 → 8,584; peak extra usable storage falls
11,872 → 6,096. Lower and upper bounds now have the same allocation counts as their
empty case. A first-equivalent hit falls 454 → 76 allocations, 34 → 8 reallocations,
63,481 → 22,837 requested bytes and 14,960 → 11,936 peak bytes. Rust remains at zero
allocations. Neither implementation moves source elements; generic resident payload
width is unchanged. Remaining costs are fixed call/frame transport and checked
confirmation/loan creation, with a 154× lower-bound and 234× first-hit timing gap in
these cold single-query measurements. Very short Rust samples can be zero at clock
resolution; ratios use medians and should not be read as steady-state throughput.

Empty lower-bound median increases 17.333 → 18.625 µs (7.5%) and empty upper increases
17.750 → 18.625 µs (4.9%), with unchanged allocation counts. Their timing ranges
substantially overlap; this experiment does not establish the cause or significance
of those small-case differences. There is no across-the-board parity claim.

Validation includes renamed controls, both comparison directions, rejected effects
on either advancing path, wrong-step rejection, invalid reference evidence and
intervals, original/expanded LLVM O2/O0 versus Rust, projected source owners,
terminal effects, unsorted inputs, scripts, fuel cutoffs and later fault identities.
The final selected suite passes 580 cases plus the structural proof test, with
focused rechecks after the provenance guard was tightened. Package CLI/LSP/cold
Hello/HTTP and 48 editor checks pass. Both Linux musl targets cross-link the new
helper; Linux executables were not run on this M2.

[Final raw evidence and per-case summary](results/0.306/).
Reproduce a paired run with `--previous-vnc /path/to/vnc-0.305` added to the command
above. Keep timing runs separate from compiler/test jobs. The remaining transport
and wide-payload gaps stay documented as the supported source-library adoption
checkpoint proceeds.
