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
