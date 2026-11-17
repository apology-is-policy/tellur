# First-match search on Apple M2

Both programs search a vector containing 0 through 255 for the first value at least 255. The timer starts after vector construction and stops when the search returns the borrowed element. Every run prints the element and the completion outcome. Rust black-boxes the collection, threshold and result. The Villanelle driver instruments only the outer generic call, compiles its IR at O3 and links the release runtime. Each trial starts a fresh process. The driver interleaves the two programs, discards one warm-up per program and writes the 25 measured durations in nanoseconds.

From the checkout root on arm64 macOS, pass the pinned LLVM tools and the native archive for the corresponding compiler build. The fixture uses the runnable example's manifest.

```sh
python3 -B experiments/first_match/measure.py ./bin/vnc \
  "$LLVM_SYS_221_PREFIX/bin" "$archive" /tmp/first-match.csv
```

For this last-element case, the medians were 2.28 ms for Villanelle and 125 ns for Rust. A separate run of the allocation interposer counted 51,552 allocation calls in the complete Villanelle process versus 126 in Rust. Primitive dispatch appeared on 65 of 121 interrupt samples from 50 Villanelle processes. The checked index increment looks up and clones its manifest declaration each time through the loop. [Raw samples](hot.csv) · [Allocator and sampling probes](../ordered_maps/comparison/README.md) · [Predicate contracts](../../docs/collections-and-algorithms.md#find-the-first-matching-vector-element)
