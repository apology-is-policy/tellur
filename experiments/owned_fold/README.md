# Fold with an owned accumulator on Apple M2

Villanelle 0.291.0 and Rust 1.97.1 fold the same readings in order. A zero fails with the previous total. An overflowing addition fails with the previous total and a different reason. Reaching the target stops with the new total. Exhaustion returns the last total. Each timed round checks the outcome, total and reason. [Villanelle reducer](../../examples/batch_fold/scale.vn) · [Rust baseline](fold.rs)

| Case | Items visited | Villanelle | Rust | Villanelle malloc/calloc | Realloc | Requested bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Empty | 0 | 2.46 µs | 1.42 ns | 14 | 0 | 2,021 |
| Target on first | 1 | 9.44 µs | 1.51 ns | 86 | 3 | 10,675 |
| Target on last | 256 | 1.688 ms | 168 ns | 32,216 | 513 | 3,244,075 |
| Exhausted | 256 | 1.690 ms | 169 ns | 32,270 | 513 | 3,249,189 |
| Offline | 128 | 843 µs | 90.4 ns | 16,039 | 257 | 1,616,350 |
| Overflow | 2 | 19.5 µs | 2.03 ns | 210 | 5 | 23,233 |

Times are warm medians on arm64 macOS. Twelve shuffled trials per case followed one discarded trial. Each process ran seven rounds and the median uses rounds two through seven. The Villanelle timer surrounds the outer fold call. The Rust timer covers batches of 1,024 folds with inputs and outputs passed through `black_box`. The short Rust times describe batched throughput. Input decoding, vector construction and printing are outside both timers. The Villanelle artifact uses the compiler's embedded runtime, pinned LLVM 22.1.8's default O3 passes and O3 object emission. Rust uses `-C opt-level=3`. [Per-query timings](results/timings.csv)

The full scan visits 1,024 bytes of logical u32 input in each language. Stopping after the first reading has identical allocation counts and bytes for vectors of length 1 and 256. In separate allocator runs Rust made zero heap requests inside every fold. Villanelle's 512 checked additions during the exhausted scan account for 24,576 allocations and 2,401,792 requested bytes. The 256 reducer calls account for another 276,736 bytes in their own ABI work. Counts are exclusive to each innermost operation. Requested bytes sum allocator requests and do not measure peak memory or bytes copied. [Scoped counts by operation](results/timings.alloc.csv) · [Whole-process counts including decoding and output](results/timings.process.csv)

The generated loop dispatches a host primitive for both the weight addition and the index increment. Each call clones its JSON declaration in `declared_primitive` and validates the invocation and outcome. The probe measures that path as a whole. The first runtime experiment is to borrow or cache immutable primitive declarations and compare allocation counts with identical fold IR. Scripted increments, overflow, traced effects and resource completion need to retain their observable behavior. [ABI entry](../../src/native/abi.rs) · [Host primitive dispatch](../../src/native/runtime.rs)

Run on arm64 macOS from the repository root. The driver extracts the matching runtime archive into a temporary cache and writes timing, scoped allocation and process allocation CSVs.

```sh
python3 -B experiments/owned_fold/measure.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" /tmp/owned-fold.csv
```

## Checked primitive dispatch in 0.292

We linked one optimized fold object against three Darwin arm64 runtimes. The intermediate build changes declaration lookup to borrow the manifest entry. The final build also checks built-in signatures against static slices and compares effect and outcome names in place. Both versions validate the declaration on every live invocation, including calls with legacy outcome spellings.

| Full fold, 256 items | 0.291 | Borrow declaration only | 0.292 |
| --- | ---: | ---: | ---: |
| Allocations in 512 checked additions | 24,576 | 5,120 | 2,048 |
| Bytes requested in those additions | 2,401,792 | 306,176 | 214,528 |
| Allocations in the whole fold | 32,270 | 12,814 | 9,742 |
| Bytes requested in the whole fold | 3,249,189 | 1,153,573 | 1,061,925 |

The baseline spends 38 allocations per addition cloning a JSON declaration. Constructing and comparing the built-in contract takes another six. Each checked addition on this full scan now makes four allocations. Per-call requested bytes fell from 4,691 to 419. The fold still makes the same 512 checked additions, reads the same 256 weights and issues the same number of runtime calls. The allocation probe records 513 additional reallocations across the fold in each version. [Scoped allocation data](results/0.292-allocations.csv) · [Whole-process counts](results/0.292-process.csv)

On the same M2, the warm full scan improved from 1.646 ms to 1.081 ms with declaration borrowing and to 1.004 ms with both changes. Optimized Rust took 166 ns. All twelve paired trial medians improved on last hit, exhaustion, early stop, offline failure and overflow. The empty input visited no primitive and its allocation counts were identical in all builds.

| Case | 0.291 | 0.292 | Rust (batched) |
| --- | ---: | ---: | ---: |
| Empty | 2.35 µs | 2.25 µs | 1.42 ns |
| Target on first | 10.0 µs | 8.00 µs | 1.63 ns |
| Target on last | 1.639 ms | 1.001 ms | 166 ns |
| Exhausted | 1.646 ms | 1.004 ms | 166 ns |
| Offline at item 128 | 821 µs | 503 µs | 90.0 ns |
| Overflow at item 2 | 20.0 µs | 14.1 µs | 2.03 ns |

The timed call excludes argument decoding and output. Rust uses the same workload and batching as the original comparison. Input order and outcome checks cover all timed variants. [Interleaved trial data](results/0.292-paired.csv)

A separate CPU sample ran 1,800 complete folds per runtime and checked every returned total. Primitive dispatch appeared as the innermost ABI call in 1,259 of 2,054 samples inside the baseline fold (61%) and 477 of 1,411 in 0.292 (34%). Collection access, child calls and reference evidence together account for another 377 samples in 0.292. These shares include the callees' work. The remaining scalar-fold performance experiment is to lower a proved live checked addition to native arithmetic while preserving its overflow exit, trace, scripted behavior and fuel accounting. The next collection representation experiment is consuming split/join. [CPU sample counts](results/0.292-cpu.json)

To repeat the paired timings, add `--previous-archive /path/to/0.291-runtime.a` to the benchmark command above. Pass `--borrow-only-archive /path/to/intermediate.a` to include the declaration-only stage. The intermediate uses 0.292's borrowed declaration lookup and 0.291's `contract` and `check_contract` implementations, with the same release runtime flags.
