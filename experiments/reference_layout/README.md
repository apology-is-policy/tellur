# Reference-target layout experiment

The compact reference-target candidate is **not adopted**. Compiler 0.305 keeps
its existing representation. Shrinking the generic value helps bulk movement,
but this candidate adds allocations to ordinary borrowed operations and does
not give a consistent timing improvement.

The runtime represents every scalar, record field and collection element with
`V`. Its reference variant makes that enum 136 bytes on arm64. This isolated
candidate moves only the immutable formation target into `Arc<StoragePlace>`;
`V` becomes **104 bytes**. Projecting a shared target uses copy-on-write so earlier
provenance snapshots remain independent. Empty projections do not clone targets,
and temporary direct bound origins avoid allocating an Arc. This metadata change
introduces no shared collection ownership, new reference permission or public ABI.

## Results on Apple M2

The comparison uses compiler 0.305 and byte-identical LLVM IR with either its
embedded runtime or the isolated candidate. Runtime optimization remains O2 with
checked arithmetic; LLVM object optimization and Rust use O3. All results and
owner identities are checked outside the timed operation. Sort and rotation use
13 shuffled retained trials after warmup. Search uses 25 retained shuffled trials
with nine calls each; repeated results below exclude each process's first call.

| Operation | Embedded 0.305 | Compact candidate | Optimized Rust |
| --- | ---: | ---: | ---: |
| Mixed stable sort, 1,024 records | 7.696 ms | 7.574 ms | 10.625 µs |
| Mixed stable sort, 128 records | 1.008 ms | 1.211 ms | 3.291 µs |
| Rotation, 4,096 u32 values | 66.416 µs | 59.083 µs | 8.583 µs flat; 5.375 µs RRB |
| Rotation, 128 values | 26.291 µs | 26.916 µs | 3.917 µs flat; 2.958 µs RRB |
| Repeated first match | 2.688 µs | 2.625 µs | 1.181 ns batched |
| Repeated last match | 3.334 µs | 3.625 µs | 135.580 ns batched |
| Repeated miss | 2.917 µs | 3.896 µs | 135.376 ns batched |
| Repeated predicate failure | 7.646 µs | 7.438 µs | 98.633 ns batched |

Short timing differences and the mixed-sort 1.6% change are not evidence of a
reliable speed improvement. Machine load varies; compare paired variants within
this run, not these absolute values against earlier release tables. The first
preliminary timing trial overlapped a compiler build and is excluded. Final
measurements ran after our builds and runtime tests finished. The observed 20.0%
small-sort and 33.6% repeated-miss regressions argue against global adoption;
no statistical-significance claim is made.

| Allocation scope | Embedded 0.305 | Compact candidate |
| --- | ---: | ---: |
| Mixed sort requests | 45,832 | 50,294 |
| Mixed sort requested bytes | 8,715,600 | 7,391,184 |
| Mixed sort peak extra usable bytes | 108,128 | 89,696 |
| Rotation 4,096 requests | 26 | 26 |
| Rotation 4,096 requested bytes | 194,383 | 148,815 |
| Repeated successful search requests | 13 | 15 |
| Repeated successful search requested bytes | 2,731 | 2,379 |
| Repeated successful search peak extra usable bytes | 2,128 | 1,664 |

The memory reduction is real, but mixed sort adds **4,462 allocation requests**.
Sort still has 1,670 reallocations and rotation two. The source algorithms and
comparison order are unchanged. Modeled mixed-sort suffix movement remains
499 slots: 67,864 bytes at 136 bytes per value, or 51,896 at 104. This counts logical
suffix movement only, not all memory traffic. The alternative does not resolve
the much larger gap from generic value storage and control transport.

All **364 isolated runtime tests** pass, including the added independent-snapshot
projection test and existing resource, guarded-origin, stale-storage, fault,
async and cancellation cases. The standalone runtime tests do not replace full
compiler/backend/package qualification; none is claimed for this rejected
candidate. The compiler, LSP and editor remain on the validated 0.305 release.

Next, test compact immutable projection storage that does not allocate a separate
formation object for every borrow. Keep the same workload matrix and reject a
change that only shifts costs into common borrowed operations. Representation
qualification, applicable ordered bounds and fresh-project supported-library
adoption remain open.

## Reproduce

Use a new output directory outside the checkout and the 0.305 source baseline.
`prepare.py` copies only the runtime's explicit source directories and applies
[candidate.patch](candidate.patch); it does not edit compiler sources. It needs
Python, `patch`, Rust and an LLVM 22.1.8 install. The compiler supplies the baseline
runtime through its embedded pack. Incremental compilation stays disabled.

```sh
python3 -B experiments/reference_layout/prepare.py /tmp/vn-reference-candidate
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/tmp/vn-reference-target
cargo build --offline --release --manifest-path /tmp/vn-reference-candidate/runtime/Cargo.toml
cargo test --offline --lib --manifest-path /tmp/vn-reference-candidate/runtime/Cargo.toml
python3 -B experiments/stable_ordering/measure.py ./bin/vnc /path/to/llvm/bin /tmp/vn-reference-sort \
  --previous-vnc ./bin/vnc \
  --runtime-archive /tmp/vn-reference-target/release/libtellur_runtime_abi.a
python3 -B experiments/sequence_storage/native.py ./bin/vnc /path/to/llvm/bin /tmp/vn-reference-rotation ./bin/vnc \
  --runtime-archive /tmp/vn-reference-target/release/libtellur_runtime_abi.a
```

For search, build `experiments/first_match/main.vn` with
`examples/first_match/manifest.json` and `--release`. Pass the candidate archive
as the archive argument to `experiments/first_match/measure.py`, and the extracted
baseline archive to `--compare-archive`. `attribute.py` measures the two archives
against that same IR. The existing ABI probe supports `VN_ALLOC_PEAK=1` for the
reported peak scopes. Run measurements after builds and tests have completed.

[Summary](results/summary.json) · [Actual layout](results/layout.json) ·
[Sort timings](results/sort-timings.csv) · [Sort allocations](results/sort-allocations.csv) ·
[Sort ABI attribution](results/sort-operations.csv) ·
[Sort environment](results/sort-environment.json) ·
[Rotation timings](results/rotation-native.csv) · [Rotation allocations](results/rotation-native-alloc.csv) ·
[Rotation environment](results/rotation-native-environment.json) ·
[Search timings](results/search.csv) · [Candidate search allocations](results/search-candidate-allocations.csv) ·
[Baseline search allocations](results/search-baseline-allocations.csv) · [Search peaks](results/search-peaks.csv)
