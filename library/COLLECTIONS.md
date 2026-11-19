# Villanelle collections 0.1.0

This supported source slice is tested with compiler **0.306.0**, using its default
LLVM backend. Support here means checked ownership and documented outcomes,
maintained regression tests and reproducible source distribution. It does not
mean performance parity with Rust or a stable binary ABI.

## Adopt the sources

From the Villanelle checkout run:

```sh
python3 -B library/vendor_collections.py /path/to/project/vendor/villanelle
```

The destination must not exist. The tool copies only the eight modules, this
contract, arithmetic declarations, inventory and license. It writes a
`collections.lock.json` receipt with SHA-256 hashes. Existing destinations,
including symlinks, are refused. A write failure can leave a partial destination;
inspect it and retry into a new directory. The receipt checks local integrity;
it is not a signature or a claim of publisher authenticity.

Import modules by relative file path, for example:

```villanelle
import "vendor/villanelle/ordering.vn" as ordering;
```

Keep the modules together: their internal relative imports are part of the
source distribution. Calls remain module-qualified; traits select concrete
implementations. There is no implicit prelude, package resolver or method import.

Merge the primitive declarations in `arithmetic.manifest.json` into the caller's
manifest, preserving their signatures and outcomes. Do not silently replace an
existing conflicting declaration. These are the compiler's live checked
arithmetic operations, not Rust adapter implementations. Supply the caller's own
launch declaration when needed. Built-in vector and map operations need no
additional adapter. A scripted scenario must supply the calls it intercepts;
live native execution does not require a scenario.

The collection modules contain no Rust adapter or hidden host dependency. If an
application supplies a Rust primitive, its manifest must explicitly name its
source root, auxiliary modules and lockfile. Run `vnc lock-native main.vn` after
changing dependency requirements. Building such an application requires a
matching Rust toolchain; applications using only built-in mechanisms use the
compiler's embedded runtime and linker. The `collection_pipeline` example shows
an application-owned adapter in a separate module.

## Public contracts

| Module | Controls and obligations |
| --- | --- |
| `membership` | `contains_key<item>` borrows a string-keyed map and key; returns `present` or `absent`, with no escaping loan and no clone requirement. |
| `search` | `first<rule>` and `try_first<rule>` visit shared vector elements in order. A predicate's first match returns a loan into the source; exhaustion returns `missing`. The fallible predicate's first failure returns its reason and stops traversal. |
| `traversal` | `range<item>` returns a shared window over valid half-open bounds, or `invalid`. Empty windows are valid. `get<item>` returns a source-element loan or `finished` for an out-of-window offset, including overflow. `entry_at<item>` visits string-keyed map entries in key order, returning key/value loans and the next rank, or `finished`. Ranks refer to the current map; restart traversal after editing it. |
| `reduction` | `fold<rule>` borrows the source and reducer and owns the accumulator. Each step owns its input state. `advanced` passes it onward; `stopped` returns it immediately; `failed` returns the partial state and reason. Exhaustion, including empty input, returns `finished`. The reducer defines which work its partial state includes. |
| `sequence_batches` | `split_at<item>` consumes a vector and returns prefix/suffix at a valid boundary, including both ends. An invalid index returns the original owner in `out_of_bounds`. `join<item>` consumes both vectors and returns their concatenation. Elements need no clone capability. |
| `partition` | `partition<rule>` consumes the source and borrows a classifier. `split` returns stable left/right outputs. `stopped` returns both classified prefixes, the current item, untouched remaining suffix and reason. No input owner is lost on either route. |
| `ordering` | `sort<rule>` consumes a vector and returns every element in stable order. `merge<rule>` consumes two already sorted vectors; equal elements from the left precede equal elements from the right. The borrowed comparator must provide a consistent total preorder. The compiler does not prove these laws. |
| `ordered_search` | `lower_bound<rule>` returns the first rank not before the needle; `upper_bound<rule>` returns the first rank after it. `first_equal<rule>` returns the first equivalent source-element loan, or `missing`. Inputs must be sorted under the same consistent comparator; this is a caller obligation, not a certified sorted type. |

Capability traits have associated item/state/error types as applicable. Calls to
user predicates, reducers, classifiers and comparators retain their declared
effects and outcome routing. A borrowed element is never converted to shared
ownership. Mutation or consumption of a source waits until its outstanding loans
are no longer used. Owned resources retain all completion obligations in returned
values and partial results. Failure reasons do not implicitly complete resources.
No collection operation grants additional permissions or origin capabilities.

For a given implementation, optimized and checked execution preserve observable
user calls, effects, fault provenance, scripts and fuel behavior. Unsupported or
observed graphs use the checked path. The public API does not promise an exact
comparison count, allocation count or internal sorting algorithm across versions.

## Compatibility and scope

The source version is independent of the compiler version. The published
`tested_compilers` list states the qualification performed, not a compatibility
claim for every later compiler. Pin both versions for reproducible builds and
review the receipt before updating. Within 0.1.x, public signatures, ownership,
outcomes and the contracts above stay compatible; an intentional breaking API
change requires a new minor source version. Correctness and performance fixes may
change internal algorithms while preserving these contracts.

The supported slice covers vector traversal/search/reduction/batches/partition,
stable sorting and ordered bounds, plus membership and borrowed traversal of the
built-in ordered string-keyed map. The builtin also supports bool/u32 keys, but
the map wrappers here are deliberately string-keyed. Array windows, generic key
constraints, persistent sharing, fallible sorting and broader algorithms remain
future design work. JSON and HTTP source profiles are separate experimental
modules and are not included in this distribution.

## Performance limits

These are isolated hot-operation measurements on Apple M2 with optimized LLVM
and equivalent optimized Rust, not launch or decoding timings. They come from
separate paired runs and must not be treated as a same-session ranking.

| Workload | Villanelle | Rust | Evidence version |
| --- | ---: | ---: | --- |
| Fold 256 u32 values | 6.67 µs; 14 allocations | 348 ns | 0.297 |
| Stable partition 4,096 u32 values | 174.25 µs; 21 allocations | 24.92 µs | 0.298 |
| Rotate 4,096 u32 values via split/join | 45.08 µs; 26 allocations | 4.833 µs | 0.302 |
| Stable sort 1,024 mixed records | 4.645 ms; 45,832 allocations | 7.167 µs | 0.305 |
| Middle lower bound in 4,096 u32 values | 25.666 µs; 25 allocations | 167 ns | 0.306 |

The latest lower-bound pass reduces time 4.82× and allocations 403 → 25;
first-equivalent hits improve 2.75×. Empty bounds have slightly slower medians
with unchanged allocations and overlapping ranges. The sort still has a roughly
648× Rust gap, and lower bound roughly 154×. Generic value/reference transport,
checked payloads and the 136-byte generic value representation remain major
costs; zero source-element movement does not imply zero allocation.

Storage experiments favor ART for large shared-prefix lookup, B-tree for
insertion/draining, flat storage for scans and tiny maps, and an owned RRB
candidate for repeated large rotations. They do not establish a universal
winner. Production retains the qualified flat vector representation; ART/RRB
and the rejected compact reference layouts are experiments, not shipped storage
promises. Keep the standing goal of matching or beating equivalent optimized
Rust for every operation, including memory management; these remaining gaps are
explicit follow-up work.

Reproduction sources, allocation/byte/peak/movement measurements and machine
settings live in the Villanelle repository under `experiments/ordered_maps`,
`first_match`, `owned_fold`, `sequence_storage`, `partition`, `stable_ordering`,
`ordered_bounds` and `reference_layout`. See `docs/collections-and-algorithms.md`
for the design history and links. The supported-source adoption check is
`python3 -B ci/check-collections.py --vnc /absolute/path/to/vnc`.
