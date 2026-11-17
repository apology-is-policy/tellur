# First-match search on Apple M2

Both programs search the same vector of 256 consecutive u32 values. The first three cases use a threshold of 0, 255 or 256. The fallible predicate rejects the reading 127 with an error before the threshold of 255. Each program returns the same element, miss or error and prints the same output.

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
