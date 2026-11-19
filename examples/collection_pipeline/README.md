# Sort, locate duplicates and fold readings

This caller composes three collection modules: stable sorting, ordered bounds
and a fold with owned state. It sorts five readings, returns the half-open range
of readings equal to 2, and sums until a target, an offline sensor or overflow.
Every route returns the sorted vector as well as its total or partial total.

From the checkout, with compiler 0.306.0:

```sh
./bin/vnc check examples/collection_pipeline/main.vn
./bin/vnc exec examples/collection_pipeline/main.vn -- 100 3
./bin/vnc exec examples/collection_pipeline/main.vn -- 5 3
./bin/vnc exec examples/collection_pipeline/main.vn -- 100 0
```

The results are `finished` with 12, `stopped` with 5, and `failed` with partial
0 and `sensor offline`, respectively. The first two return duplicate bounds
[1, 3); the last returns [2, 4). Failure deliberately has process exit code 1.
Use `4294967295 4294967295` to observe addition overflow with partial total 9.

`accept_reading` is caller policy, declared in `manifest.json`, re-exported from
`host.rs` and implemented in `limits.rs`. It is not a collection dependency.
Its locked Rust adapter requires a Rust toolchain on the first build. The
manifest explicitly lists both sources and `Cargo.lock`; after dependency
changes regenerate the lock with `./bin/vnc lock-native
examples/collection_pipeline/main.vn`.

For an independent project, vendor the supported source slice using
[the source contract](../../library/COLLECTIONS.md), copy these five source/host
files plus the lockfile, and change the three import paths to the vendored
location. `ci/check-collections.py` checks this from a fresh directory with spaces
and a different working directory, then produces a self-contained source bundle.
