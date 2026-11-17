# Native map comparison

The Rust executable constructs a standard-library `BTreeMap<String, u32>` from the same scenario as the Villanelle executable. Both return the same outcome and ordered events. Rust's observed traversal either stores a string per event or writes the formatted messages to one string with end offsets (`--pooled`). The second case uses the same trace layout as Villanelle. Both finish decimal formatting inside the timed operation.

## Reproduce

From the repository root, supply the pinned LLVM development installation used for compiler builds. The AOT driver calls `build_llvm(..., 3)` for each authored and expanded entry. Keep the Cargo target directory on the project build cache.

```sh
export CARGO_INCREMENTAL=0
d=experiments/ordered_maps/comparison
out=$(mktemp -d)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$out/target"}
cargo build --release --locked --manifest-path "$d/Cargo.toml" --bin rust_map
cargo run --release --locked --manifest-path "$d/Cargo.toml" --features tellur \
  --bin build_o3 -- experiments/ordered_maps/baseline.vn "$out"
python3 -B "$d/measure.py" "$out" "$CARGO_TARGET_DIR/release/rust_map" "$out/process.csv"
```

The process test interleaves both languages and checks all output after each timed run. The scan without emissions permits Villanelle to skip unused pairs. Rust deliberately black-boxes each pair in that case. Use `observe_scan` for a comparison that requires visiting every pair in both languages.

On arm64 macOS, `time_trace.py` also measures traversal and event construction in isolation. Supply the runtime archive extracted by the compiler, found in `tellur-runtime-pack` under its native cache. The script verifies that relinking that archive reproduces the executable's text section. It redirects the generated scan call in a temporary object to a timed wrapper. The Rust timer covers its traversal and trace construction. Each sample uses a freshly decoded map. Event validation and JSON serialization fall outside both timers.

```sh
python3 -B "$d/time_trace.py" "$out/observe_scan-expanded" \
  "$CARGO_TARGET_DIR/release/rust_map" "$archive" \
  "$LLVM_SYS_221_PREFIX/bin" "$out/hot.csv"
```

For allocator counts, build `profile.c` as a dynamic library with `cc -arch arm64 -O2 -dynamiclib` and set `DYLD_INSERT_LIBRARIES` to its absolute path for the benchmark child only. The four counters report malloc/calloc calls, realloc calls, non-null frees and cumulative requested bytes. For single-threaded CPU sampling, compile with `-DVN_CPU_ONLY` and run with `VN_SAMPLE=1`. The signal handler captures program counters and frame pointers. Symbolization runs after the workload. Both probes run separately from timed trials.
