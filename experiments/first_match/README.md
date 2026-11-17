# First-match search on Apple M2

Both programs search the same vector of 256 consecutive u32 values. The first three cases use a threshold of 0, 255 or 256. The fallible predicate rejects the reading 127 with an error before the threshold of 255. Each program returns the same element, miss or error and prints the same output.

## Where a first-element match spends its time in 0.290

The generated search IR was byte-identical across versions. We linked the same O3 object once with each runtime and interleaved them with the Rust comparator on Apple M2. Each cell below is the median of 200 repeated queries (eight queries from each of 25 fresh processes). The Rust column uses 1,024 black-boxed searches per timed batch to resolve the shortest case. It measures hot throughput per search. Individual Rust calls, including timer overhead, are also in the raw data. [Paired timings](results/0.290-paired.csv)

| Outcome over 256 elements | Villanelle 0.289 | Villanelle 0.290 | Rust per search, batched | Search allocations, 0.289 → 0.290 |
| --- | ---: | ---: | ---: | ---: |
| First element | 7.83 µs | 6.00 µs | 1.06 ns | 68 → 39 |
| Last element | 9.13 µs | 7.29 µs | 119 ns | 74 → 45 |
| Miss | 4.33 µs | 3.98 µs | 120 ns | 28 → 17 |
| Predicate failure at 127 | 8.15 µs | 7.96 µs | 88.6 ns | 66 → 40 |

First and last matched faster in all 25 paired trial medians. A miss improved in 20 and predicate failure in 14, so the timing evidence for the failure path is inconclusive. The allocation counts come from a separate run. Each malloc or calloc is charged once, to the innermost active runtime call between search entry and return. A first hit also made three reallocations in both versions. Requested bytes, including reallocations, fell from 7,350 to 4,387. [0.289 per-call counts](results/0.289-query-allocations.csv) · [0.290 per-call counts](results/0.290-query-allocations.csv) · [Whole-process counts](results/0.290-process-allocations.csv)

On the first hit, four evidence annotations previously allocated 22 times and now allocate six times. A checked reborrow can reuse an identical bounded origin list after validating its formation identity and coverage. A single value transfer now takes its validated handle directly. Six stores and two dereferences together went from eight allocations to zero. Collection access went from 13 to nine by moving the incoming loan into the returned element and avoiding temporary dispatch data.

The first hit still makes 83 ABI calls including the outer call and 41 status checks. Four reference formations allocate 12 times. In a separate sampling run of 300,000 first hits, we checked the borrowed value every iteration. Inside the generated search frame, evidence accounted for 460 of 1,904 samples (24%, formerly 36%), collection access for 290, nested call handling for 270 and reference formation for 248. These shares exclude outer call setup and teardown. The Rust arm64 loop loads a u32, compares and returns a pointer into the vector. Villanelle still passes dynamic values with owned paths and origin lists between the candidate, predicate and result. The next experiment is to carry the proven scalar comparison through LLVM and form the full checked borrow once at the successful exit. The ordinary path provides the comparison for scripts, tracing, fuel, faults and origin validation.

## Prefix scanning in 0.289

| Repeated search, median | Villanelle 0.288 | Villanelle 0.289 | Rust |
| --- | ---: | ---: | ---: |
| First element | 6.58 µs | 6.29 µs | 0.041 µs |
| Last element | 2,092 µs | 7.75 µs | 0.166 µs |
| Miss | 2,089 µs | 3.85 µs | 0.166 µs |
| Predicate error at index 127 | 1,093 µs | 6.83 µs | 0.125 µs |

The timer covers the search call, including production of the borrowed result or error. Vector construction, I/O and checking the result are outside the timer. Each fresh process runs nine queries on that vector. The table uses queries two through nine over 25 interleaved trials per case and language. On the first query in each process the 0.289 medians were 31.33 µs, 35.21 µs, 26.75 µs and 32.58 µs, respectively. Rust's first-query medians ranged from 0.042 to 0.166 µs. The shortest Rust calls span only a few timer ticks. Rust black-boxes the collection, threshold and result. The Villanelle driver instruments the two outer generic calls and compiles the IR at O3. Apple M2 arm64, Villanelle LLVM 22.1.8, rustc 1.97.1 at O3. [Previous samples](results/0.288.csv) · [Current samples](results/0.289.csv)

On a single last-match query, the entire Villanelle process made 965 allocation calls and requested 317,528 bytes, down from 51,740 calls and 5,048,538 bytes. Rust made 131 calls and requested 12,931 bytes. In a separate CPU profile of 300,000 successful last-match queries, the caller checked the returned value and wrote output once per process. Of 459 stack samples in the search control, 120 included reference-evidence handling, 68 included the prefix scan and 53 included collection dispatch. The reference evidence connects the returned element to the original vector. Its construction and transfer are the next performance target. [Previous allocation counts](results/0.288-allocations.csv) · [Current allocation counts](results/0.289-allocations.csv)

From the checkout root on arm64 macOS, pass the pinned LLVM tools and the native archive for the corresponding compiler build. The fixture uses the runnable example's manifest.

```sh
python3 -B experiments/first_match/measure.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" "$archive" /tmp/first-match.csv
```

Pass `--allocator /absolute/path/to/probe.dylib` to record whole-process allocation counts in a separate single-query run for each case. [Allocator and CPU probes](../ordered_maps/comparison/README.md) · [Predicate contracts](../../docs/collections-and-algorithms.md#find-the-first-matching-vector-element) · [Original single-query baseline](hot.csv)


To compare runtimes with identical IR, add `--compare-archive /path/to/previous-runtime.a` to the timing command. Both archives must implement the generated ABI. To count allocations inside the search, first build `experiments/first_match/main.vn` with the example manifest and `--release --out /tmp/search`, then run the separate probe.

```sh
python3 -B experiments/first_match/attribute.py /tmp/search.ll \
  "$LLVM_SYS_221_PREFIX/bin" "$archive" /tmp/search-allocations.csv
```
