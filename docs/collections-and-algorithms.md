# Collections and algorithms for the Villanelle standard library

Supported source slice 0.1.0, 18 November 2026, tested with compiler 0.306.0.
The calling convention and eight module contracts are implemented; further
algorithms and backing representations below remain proposals where stated.
The [language reference](language-reference.md#arrays-vectors-and-maps) specifies
built-in operations and ownership. The [standalone library contract](../library/COLLECTIONS.md)
is the current public API and compatibility policy; historical prototype wording
in this design records how that slice was developed.

## Delivered slice and next boundaries

| Roadmap item | Delivered result |
| --- | --- |
| Collection calls and discovery | Explicit module-qualified controls, capability traits and ownership-aware editor suggestions. |
| Membership and traversal | String-keyed ordered-map membership/entry loans; shared vector windows. |
| Search and reduction | First match, fallible first match, and fold with owned accumulator and explicit stop/failure. |
| Consuming algorithms | Split/join, stable partition with every partial owner returned, stable sort/merge. |
| Ordered search | Lower/upper bounds and first-equivalent source loan under caller-supplied ordering laws. |
| Performance passes | Paired Rust baselines, hot-path profiles, allocation/byte/peak/movement evidence, guarded native lowering and checked fallback. |
| Representation selection | ART/B-tree/flat map and flat/moving-start/RRB sequence experiments; reference-layout candidates qualified and rejected where they regress. |
| Adoption and support | Fixed source inventory, no-overwrite vendoring, hashes, canonical arithmetic requirements, fresh-project checks and a composed caller-owned Rust adapter example. |

See [adoption instructions and the measured performance table](../library/COLLECTIONS.md)
and [the pipeline example](../examples/collection_pipeline/README.md). Supported
semantics are distinct from speed parity: stable sort still measures roughly
648× optimized Rust and middle lower bound 154× in their respective paired M2
runs. Generic reference/value transport and payload representation remain the
next performance targets. Current and prior compiler/editor qualification is
reused for this source-only release; no language or runtime behavior changes.

Future design includes array windows, generic map-key constraints, persistent
sharing eligibility, fallible sorting and broader algorithms. These are outside
this first supported slice; no implicit sharing, new origin capability or new
sorting failure policy has been chosen. Thylacine and Rust-backend retirement
remain separate work.

## Calling collection controls

Use small capability traits for the operations a collection supplies. Put reusable algorithms in ordinary modules. Imported controls use their module qualifier, as in the prototype call `membership.contains_key<string>(borrow headers, borrow "origin")`. Inside a trait-bounded control, use the existing trait/type qualifier to select an implementation, as in `reader<r>.read(input)`. The checker resolves the same callee regardless of import order. [Trait qualification](traits.md#accepted-qualification)

Keep ownership visible in the generated and handwritten source. A call on an owned collection spells out `borrow values` for a shared loan, `borrow mutable values` for exclusive access, or `values` for a consuming parameter. An existing reference can be passed directly where its type and permissions match. The caller routes the control's named outcomes in the usual way.

### Try collection discovery in the editor

Open the [runnable example](../examples/collection_discovery/main.vn) with the 0.284 language server. In its first `delta`, replace the whole call before the opening brace with `headers.`. Choose `contains_key`. The suggestion says **borrows headers** and inserts `membership.contains_key<string>(borrow headers, borrow key)`, with the key argument selected. Enter `borrow "origin"` to restore the example.

The two calls on `values` show why the preview matters. Choosing `at` borrows the vector and lets the caller use its returned element before partitioning. Choosing `partition` says **moves values** and inserts `sequences.partition(values, pivot)`. The caller receives both output vectors through the `split` outcome.

Discovery works for a named collection parameter or an identifiable local in the current control. It looks through direct imports for public synchronous controls with a matching array, vector or map as their first parameter. Generic arguments must be inferable from that parameter and free of trait bounds. Existing references retain their permissions, and record receivers offer visible fields. The checker validates the completed call, including loans and path availability.

## A first operation with two callers

The settings HTTP service and the standalone greeting server both inspect two headers. They need the value of `Host` to check the authority. For `Origin`, presence alone determines a response. A library can express that distinction today.

```villanelle
pub control contains_key<item>(ref map<string, item> values, ref string key) exits present, absent {
    exit delta map_get(values, key) {
        found { ref item value } => present {},
        missing => absent {}
    };
}
```

In temporary copies of both services, routing `contains_key<string>(borrow headers, borrow "origin")` to their existing response paths passed the 0.283 checker. The operation consumes neither input, returns no loan and imposes no cloning requirement on the value. Each caller still chooses its status and message. This is a source prototype with string keys. Today's other supported map keys are `u32` and `bool`. A generic key constraint is a separate design decision. [Settings caller](../applications/settings/http.vn) · [Greeting caller](../examples/http_hello/main.vn)

## Borrow a range of a vector

The experimental `traversal.range<item>(borrow values, start, end)` returns a shared window over `[start, end)`. An empty range at either end is valid, including on an empty vector. Reversed bounds or an end past the vector return `invalid`. The window stores one shared reference and two indices. Its fields are private.

Call `traversal.get<item>(borrow view, offset)` to read relative to the start. It returns `found` with an element borrowed from the original vector, or `finished` if the offset falls outside the window, including arithmetic overflow. Two windows can overlap. Structural mutation becomes available after the last use of the windows and their returned elements. The source module uses the manifest's existing `checked_add` primitive. Array windows remain open. [Source module](../library/traversal.vn) · [Runnable example](../examples/borrowed_traversal/main.vn)

## Scan an ordered map

For a string-keyed map, call `traversal.entry_at<item>(borrow values, position)` starting at zero. Route `item { ref string key, ref item value, u32 next }` to the loop body and pass `next` on the back edge. Route `finished` to the end of the scan. The header example prints both fields, then removes a header after the scan. The builtin also accepts the existing `bool` and `u32` key types. Positions are ranks in the current map. Restart at zero to scan the whole map after editing it. [Runnable example](../examples/borrowed_traversal/headers.vn) · [Reference](language-reference.md#arrays-vectors-and-maps)

## Find the first matching vector element

Import `library/search.vn` and implement `search.predicate` for a rule type with an associated `item`. Call `search.first<rule_type>(borrow source, borrow rule)`. The predicate inspects each element by shared borrow in vector order. Its first `matched` ends the search with `found { ref item value }` borrowed from `source`. An empty vector or a sequence of `skipped` results returns `missing`. The caller can mutate the vector after the last use of the returned reference.

A `search.fallible_predicate` also declares an `error` type. Its `failed { error reason }` immediately becomes the failure of `search.try_first`. The caller must route that outcome. Omitting it produces a diagnostic tracing the failure through the library to the concrete predicate. Both experimental controls use the manifest's `checked_add` operation to advance the index. The example searches sensor readings twice and prints 17 and 29 before reaching the offline reading. Run `vnc exec examples/first_match/main.vn`. [Source module](../library/search.vn) · [Runnable example](../examples/first_match/main.vn)

Optimized LLVM builds can skip a consecutive run of u32 values below a rule's threshold when the checked graph proves the rejection path has no effects. A predicate can also stop at a particular u32 constant, for example to report an offline sensor. The first candidate executes the authored control and retains its declared origin and failure behavior. Tracing, scripted increments, fuel limits and effects on the rejection path preserve step-by-step evaluation. On Apple M2, repeated last-match searches over 256 readings took 7.75 µs in 0.289 versus 2.09 ms in 0.288 and 0.166 µs in Rust. [Search benchmarks](../experiments/first_match/README.md)

Villanelle 0.291 also proves the successful branch of simple u32 threshold predicates. LLVM compares the candidate with the rule field directly. The successful exit forms its loan into the vector, including when the vector itself is a field of a borrowed owner. User effects on the matched path and predicate failures execute as authored. On M2 a first hit took 1.69 µs with 13 allocations, a last hit 2.58 µs with 19 allocations. [Paired comparison with Rust](../experiments/first_match/README.md#scalar-predicate-and-borrowed-hit-in-0291)

## Fold a borrowed vector with an owned accumulator

`reduction.fold` borrows the vector and the rule. It takes the initial state by value. The reducer declares its item, state and error types. Each call to `step` owns the current state and borrows one item. The state can contain a must-complete resource.

| Step or input condition | Who owns the state next |
| --- | --- |
| Empty vector | The caller gets the initial state in `finished`. |
| `advanced { updated }` | The fold moves the updated state into the next step, or returns it in `finished` at exhaustion. |
| `stopped { result }` | The caller gets this state in `stopped`. The fold visits no further items. |
| `failed { partial, reason }` | The caller gets the state and the reducer's error together. The reducer determines what work the partial state includes. |

The scale example commits a successful reading before testing the target. A zero reading or an overflowing addition therefore returns the total of the preceding readings. Its live native invocation is `./bin/vnc exec examples/batch_fold/main.vn -- 20 0`. The resource fixture transfers a handle through the loop and closes it once on every route. Scripted index increments use the manifest's `checked_add` operation, as in first-match search. [Library](../library/reduction.vn) · [Example](../examples/batch_fold/main.vn) · [Resource fixture](../tests/fixtures/owned_fold.vn) · [M2 comparison](../experiments/owned_fold/README.md)

With 0.292, a full fold over 256 u32 readings requests 1.06 MB across 9,742 allocations and 513 reallocations. The same fold on 0.291 requested 3.25 MB across 32,270 allocations and 513 reallocations. Stopping after the first reading costs the same 6,403 requested bytes for a one-item vector and a 256-item vector. [Profile by outcome](../experiments/owned_fold/README.md#checked-primitive-dispatch-in-0292)

Compiler 0.297 carries proven effect-free advancing fold steps in native scalars inside the precompiled runtime. The full 256-item fold takes 6.67 µs against Rust at 348 ns in the paired run; requests fall to 14 allocations and 2,021 bytes. Terminal steps and unsupported/resource-bearing reducers retain checked execution. The remaining 19.2× gap and empty-input regression are documented. Next: consuming stable partition and stable ordering, then representation qualification and supported-library adoption. [Comparison and memory measurements](../experiments/owned_fold/README.md#scalar-advancing-prefixes-in-0297)

## Consuming stable partition

The experimental `partition.partition<rule_type>` owns its source vector and borrows a statically selected classifier. Each element is presented to `classify` by shared borrow, in input order. `left` and `right` move that element to the corresponding output, preserving relative order in both outputs. The classifier may emit effects; those effects occur exactly once for each visited element.

| Outcome | Ownership returned to the caller |
| --- | --- |
| `split { left, right }` | Both completed output vectors; every input has moved to one destination. |
| `stopped { left, right, current, remaining, reason }` | Both completed prefixes, the element the classifier stopped on, the unclassified suffix in original order, and the classifier-specific reason. |

The classifier declares associated `item` and `error` types. It cannot consume or mutate through its shared item borrow. Outstanding loans prevent consuming the source. Resources in either partition, the current element or the remainder keep their completion obligations. This prototype does not change the existing scalar partition used by the editor-discovery example.

Run `./bin/vnc exec examples/partition_batch/main.vn -- --scenario examples/partition_batch/main.scenario.json` for `[4, 1, 3, 2]` at pivot 3: the outputs are `[1, 2]` and `[4, 3]`. A zero stops classification and returns the remaining owners. The separate resource fixture emits visited ranks in order and closes each ticket exactly once, including when a close reports failure. Original and expanded interpreter and both native backends agree; unhandled resource ownership and consuming through an outstanding loan are rejected. [Library](../library/partition.vn) · [Caller](../examples/partition_batch/main.vn) · [Resource fixture](../tests/fixtures/owned_partition.vn)

Compiler 0.298 moves a proven effect-free scalar prefix in bulk, preserving the checked terminal classifier and all resource/observed fallback paths. In the paired run, the 4,096-item partition improves from 75.76 ms to 174.25 µs, versus Rust at 24.92 µs. Allocations fall from 106,517 to 21. Repeated suffix shifts disappear on full completion; wide generic storage and fixed transport remain measured gaps. Next: stable ordering, representation qualification and supported-library adoption. [Measurement, movement and profile](../experiments/partition/README.md#bulk-scalar-partition-in-0298)

## Stable ordering

The experimental `ordering.sort<order>(source, borrow rule)` consumes a vector and returns `sorted { values }`. Implement `ordering.comparator` for the rule type, declare its associated `item`, and supply `compare(ref self rule, ref item left, ref item right) exits before, equal, after`. Comparisons borrow both elements; the algorithm moves owners and requires no cloning capability. Equal keys retain input order. Empty and single-element inputs succeed without invoking the comparator.

The rule must describe a consistent total preorder: reversing the inputs reverses before/after, equal is an equivalence relation, and the ordering is transitive. Effects such as logging are allowed, but must not change comparison results. The checker verifies the interface and ownership, not these mathematical laws. An inconsistent comparator has no sortedness guarantee. This first interface has no failure outcome; a fallible comparator still needs a separate decision about the state returned on early exit.

`ordering.merge<order>(left, right, borrow rule)` returns `merged { values }`. Both inputs must already be sorted under the same rule. It takes equal elements from the left input first. The bottom-up sort forms singleton runs, merges adjacent runs and repeats without recursion. The current source invokes comparisons in that pass order; resource/effect tests compare the exact trace across expansion and both native backends. Comparison counts are not a promise across future library revisions.

The current merge pass reverses its run headers once, then removes headers from the end. The first pair is still the original leftmost pair, so comparisons and equal-key stability retain their order. An odd final run is carried forward unchanged. Terminal consuming joins settle the empty generic owners without requiring a new drop or clone capability. Optimized LLVM can batch the reversal's effect-free advancing pop/push path; terminal effects, scripts, tracing and fuel limits retain checked execution.

Run the [record example](../examples/stable_ordering/README.md) to sort duplicate keys while observing their original identities. A separate [ticket fixture](../tests/fixtures/owned_ordering.vn) proves that each resource is returned and completed once, including after a close failure. Dropping returned resources and consuming a collection through an outstanding loan are rejected. Both comparands' loans end before the selected owner moves.

This remains a source prototype on compiler 0.298. On M2, 1,024 mixed two-u32 records take 81.33 ms versus Rust stable sort at 11.25 µs; the complete Villanelle call makes 622,401 allocations plus 37,578 reallocations. Front drains and generic per-element dispatch remain expensive. Next, reduce proven effect-free merge traversal and transport while preserving comparison order, resource fallback and diagnostic identities; then qualify representations and library adoption. [Timings, allocations, movement and CPU evidence](../experiments/stable_ordering/README.md)

Compiler 0.299 lowers proven effect-free merge prefixes for records containing u32 fields. The complete mixed sort improves 4.59× to 10.32 ms in a paired M2 run, versus Rust at 7.042 µs. Allocations fall to 91,749; 166.2 MB of modeled suffix movement remains, mostly in singleton/run construction. Resource and observed execution keep the checked path. Next, reduce those remaining run costs before representation qualification and supported-library adoption. [Paired evidence](../experiments/stable_ordering/README.md#bulk-merge-traversal-in-0299)

Compiler 0.300 batches singleton-run formation while preserving generic element ownership, including resources. In the paired M2 run the complete mixed sort improves 1.33× to 10.06 ms, versus Rust at 9.042 µs; allocations fall to 68,197. Peak extra usable memory rises to 442,016 bytes, which remains an investigation target alongside run-header movement and terminal transport. [Paired evidence and memory limitation](../experiments/stable_ordering/README.md#singleton-run-construction-in-0300)

Compiler 0.301 identifies the peak increase as retained spare capacity in decoded records. It compacts fixed-arity payload fields before reserving run output, retaining growable collection reserves and all resource owners. Mixed-sort peak extra usable memory returns to 114,336 bytes; this adds 1,024 reallocations and costs about 4.5% in the final paired run. Next, reduce run-header and terminal transport before representation qualification and supported-library adoption. [Root cause and tradeoff](../experiments/stable_ordering/README.md#fixed-payload-compaction-in-0301)

Compiler 0.302 reduces whole consuming join transport while preserving resource ownership, effects and later fault identities. The paired mixed sort improves 7.773 → 6.278 ms, still 881× Rust; rotation improves 50.21 → 45.08 µs, still 9.33× Rust flat. Run-header movement and generic storage remain open performance work before representation qualification and supported-library adoption. [Sort](../experiments/stable_ordering/README.md#complete-join-call-transport-in-0302) · [Rotation](../experiments/sequence_storage/README.md#complete-join-call-transport-in-0302)

Compiler 0.303 reuses owned vector metadata during head removal. Mixed-sort time improves 6.8% to 6.367 ms, allocations fall to 41,550 and peak memory stays unchanged. The 813× Rust gap and repeated run-header suffix shifts remain. A reversal/pop traversal candidate preserves comparison order in initial interpreter checks; resource and native performance qualification are still required before adopting it. [Evidence](../experiments/stable_ordering/README.md#owned-head-transport-in-0303)

Compiler 0.304 adopts reversed run headers with a proven advancing-loop summary. Mixed 1,024-record time improves 16.9% to 4.644 ms, still 675× Rust; header suffix shifts disappear. The 128-item case regresses 7.2% and allocations rise to 52,030. Pop/reference transport is the next measured target before representation qualification and supported-library adoption. [Evidence and limits](../experiments/stable_ordering/README.md#reversed-run-headers-in-0304)

Compiler 0.305 reduces live borrowed-pop transport with identical generated IR. Mixed sort improves 6.1% to 4.645 ms, with 45,832 allocations and unchanged peak memory. It remains 648× Rust; the 128-item case is still 3.5% slower than the original 0.303 traversal in a separate paired check. Generic value/reference representation qualification is next, followed by applicable ordered bounds and supported-library adoption. [Evidence and remaining costs](../experiments/stable_ordering/README.md#borrowed-pop-transport-in-0305)


## Ordered bounds and first-equivalent search

The experimental `ordered_search` module borrows a vector, an `ordering.comparator`
rule and a needle. It uses the same consistent total-preorder and sorted-input
obligations as `ordering.merge`. The caller must establish sortedness under the
same rule and keep its results consistent throughout the query. The checker
verifies interfaces and loans, not sortedness or ordering laws. A prior
`ordering.sort` establishes the required order if the rule is consistent; changing
keys or the rule can invalidate it.

| Control | Result |
| --- | --- |
| `lower_bound<order>(source, rule, needle)` | `position { index }`: first element that does not compare `before` the needle, or the length |
| `upper_bound<order>(source, rule, needle)` | `position { index }`: first element that compares `after` the needle, or the length |
| `first_equal<order>(source, rule, needle)` | `found { index, value from source }` for the first equivalent element; `missing { insertion }` at the lower bound otherwise |

The equivalent range is half-open `[lower, upper)`. Empty inputs return position
zero or missing insertion zero without comparisons. The same definitions work for
descending rules. Equality here means the comparator's `equal` outcome, so records
can be equivalent while having different identities. A stable sort followed by
`first_equal` selects the earliest original equivalent record.

Bounds perform O(log n) comparisons with zero element movement. `first_equal`
performs a lower-bound query and at most one confirmation comparison. The source
and needle remain borrowed; there is no element clone requirement. The returned
loan prevents moving or mutating the source while it remains live. Copy any needed
scalar field and release the loan before returning or consuming its owner. Effects
such as comparison logging are preserved, but comparison counts/order are not an
API promise across library revisions. Faults remain faults; the comparator has no
failure outcome in this initial interface.

The module needs the canonical `checked_add`, `checked_sub` and `checked_div`
primitive signatures and arithmetic behavior in its caller's manifest. Its
start/count invariant prevents overflow, underflow and division by zero. Defensive
failure/out-of-bounds branches return the current start; they do not promise valid
bounds if a scripted or nonstandard arithmetic provider violates the invariant.
Interpreter scenarios must script those primitives; native execution uses live
checked arithmetic. Keeping this dependency in a separate module allows existing
ordering-only callers to retain their manifests.

The [runnable example](../examples/ordered_search/README.md) sorts duplicate-key
records and queries their range. The resource fixture checks ascending/descending
rules, empty vectors, duplicate identities, comparator effects, unchanged owners,
exactly-once completion and live-loan rejection across expansion and both native
backends. [Performance and ABI transport evidence](../experiments/ordered_bounds/README.md)
separates complete query costs from construction and output; logarithmic comparison
count is not a native-performance parity claim.

Compiler 0.306 summarizes proven effect-free scalar binary-search advances while leaving the terminal iteration and returned loan checked. The 4,096-middle lower bound improves 4.82× to 25.666 µs versus Rust at 167 ns; allocations fall 403 → 25. First-equivalent hits improve 2.75×. Empty-case medians regress slightly at unchanged allocation counts, and fixed transport remains a large gap. These are measured performance limits for the upcoming supported-source slice. [Paired measurements and checks](../experiments/ordered_bounds/README.md#scalar-binary-search-advances-in-0306)

## Proposed algorithm contracts

| Family | First useful contract | Observable behavior |
| --- | --- | --- |
| Membership and search | Key membership by shared borrow. A sequence search returns a borrowed first match or an explicit miss. | Search visits elements in sequence order and stops at the first match. The match borrows the collection. A miss releases the loan. |
| Borrowed traversal | Start with shared array/vector slices over a half-open range and ordered key/value traversal for maps. | An empty range at either end is valid. A reversed or out-of-bounds range takes an explicit failure exit. Reads can coexist. Structural mutation waits until the last conflicting view or returned reference is released. |
| Reduction and early exit | Fold a borrowed sequence with an owned accumulator. An outcome from the step control can stop the fold. | Visit in order. Continuation transfers the updated state to the fold. Stop and failure return the state to the caller. Failure also returns the step's reason. |
| Consuming partition | Move each input to one of two outputs while preserving relative order in each. | A classifier borrows the current element. If it stops early, return both completed partitions, the current element and the untouched remainder. Every owner has a destination, including must-complete resources. |
| Ordering | Stable sort with an infallible ordering for the first version. Ordered bounds/search use the same caller-supplied sorted-input obligation as merge. | Equal keys keep input order. Moving records during sorting preserves their resource obligations. A later fallible comparator needs an explicit contract for collection state on early exit. |

For the first search and fold prototypes, use a statically named predicate or step control with a closed outcome contract. Try Villanelle's existing trait dispatch for reuse across element types. The design question is how callers route a step's additional outcomes without losing where they arose. A two-outcome predicate, a fallible predicate and a step with an owned resource are three distinct tests. Closure syntax and a universal iterator trait can be considered after those examples.

Keep read-only traversal and consuming traversal independently useful. In particular, a vector containing file handles can be inspected through shared borrows and later drained into explicit close operations. Existing whole-collection borrow checks apply even if a particular storage layout happens to keep addresses stable.

## Two representation experiments

**Ordered string maps.** Prototype an adaptive radix tree (ART) behind the current map contract. Its nodes adapt to branching density and its key traversal follows byte order. This fits Villanelle's lexicographic UTF-8 string order and makes a bounded prefix scan a plausible later operation. Compare against today's sorted flat map and an ordered B-tree. Start with the tiny HTTP-header maps as well as larger settings maps with common prefixes. A compact flat form for small maps with promotion to a tree is a candidate. Measure the crossover and the cost of promotion before selecting it. Empty strings, embedded zero bytes, a key that prefixes another, multibyte keys, and equal-key replacement must all preserve Villanelle's behavior. [ART paper](https://db.in.tum.de/~leis/papers/ART.pdf) · [Ordered B-tree baseline](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html)

**Vectors with cheap split and concatenation.** Prototype a relaxed radix balanced tree (RRB) for a workload that repeatedly joins batches and splits them again. RRB trees support logarithmic concatenation and splitting through a tree of chunks. Compare with a flat vector for append, random access, scanning, split and join. Include a flat vector with a moving start index as a baseline for front drains. First try consuming split and join, where both the old owners and the destination of each element are clear. Any persistent snapshot API would need its own eligibility rules for element identity, mutation, explicit cloning and resources. Tree sharing alone cannot grant those permissions. [Bagwell and Rompf's RRB paper](https://infoscience.epfl.ch/record/169879/files/RMTrees.pdf)

Record time per operation, allocations, peak live bytes, and bytes or elements moved. Report key lengths, collection sizes, hit/miss ratios and scan order with the results. Compare complete Villanelle operations as well as the isolated data structure. Today front-draining a flat vector or map can take quadratic time across a full drain. A linear full ordered traversal with no element clones is a useful first target. The existing collection conformance cases cover key order, value replacement, invalidation and resource completion. Run them against each candidate and compare original and expanded behavior in the interpreter and LLVM executables. [Collection conformance cases](../tests/collections.rs)

The first ordered string-map comparison is complete on Apple M2. Linear lookup was fastest for eight headers. At 4,096 shared-prefix settings keys ART led on lookup, the B-tree led on insertion and draining, and flat storage led on borrowed scans. The 0.286 source prototype scans the existing flat map by borrowing each pair. In the native settings workload, scanning 4,096 entries through the library took 22.1 ms and draining took 92.4 ms, including process startup and decoding. [Measurements and reproduction](../experiments/ordered_maps/README.md)

## Consuming sequence batches

The experimental `sequence_batches` module provides `split_at<item>` and `join<item>`. A split consumes its input and returns an ordered prefix and suffix. Positions zero and length are valid. An out-of-bounds position returns the original vector. Join consumes both vectors and preserves the order of the left input followed by the right. Resource obligations follow each element into its destination. A surviving element loan prevents consuming its source. [Source module](../library/sequence_batches.vn) · [Batch rotation example](../examples/rotate_batch/main.vn)

The M2 storage prototype uses a uniquely owned 32-way relaxed radix tree with size tables. For 128 splits and rotations at 65,536 elements, eight-byte records take 296 µs in the tree versus 1,906 µs in a flat vector. With 64-byte records, the pair takes 323 µs and 17,500 µs. Flat storage leads on append, scans and random access. Its consuming moving-start iterator drains the eight-byte workload in 32 µs. The tree remains a candidate for rearranging large batches.

Compiler 0.293 lowers proven consuming split/join iterations to bulk transfers. In a fresh M2 comparison, the complete 4,096-u32 rotation fell from 17.17 ms to 62.6 µs, while Rust flat storage took 4.79 µs. Allocation count fell from 43,722 to 42, with three remaining reallocations. The 13.1× gap now calls for profiling generic value storage, payload movement and control-call transport before selecting a backing representation. [Workloads, memory and measurements](../experiments/sequence_storage/README.md)

The follow-up profile confirms 136 bytes per runtime value against four bytes per Rust u32. Compiler 0.294 copies the proven prefix contiguously and compacts its suffix once. This improves the isolated large-value kernel while keeping its allocation volume and ownership unchanged. Generic payload width, temporary reference formation and control transport remain performance targets. [Payload and call-cost evidence](../experiments/sequence_storage/README.md#where-the-remaining-batch-time-goes)

Compiler 0.295 removes temporary reference formation for proven fresh length reads and allocates no type-parser storage for length queries. The complete 4,096-item rotation takes 44.71 µs against Rust flat's 4.58 µs, with 34 allocations and two reallocations. Peak extra allocator-usable storage in split falls by 640 bytes; the large payload remains unchanged. The next performance pass returns to the owned fold's checked arithmetic. [Length-read comparison](../experiments/sequence_storage/README.md#fresh-collection-length-reads-in-0295)

## Path to a supported library

1. Exercise the existing public-outcome change on both JSON callers and use it to document failure routing and provenance. The results are in the [language roadmap](language-revision.md#active-checkpoint-queue).
2. Specify the ownership and outcome contracts above using caller examples. Start with membership and one borrowed traversal. Prototype the ordered string map alongside them.
3. Evaluate higher-order algorithms and the sequence representation with real callers. Introduce slices, generic bounds or additional origin contracts as the examples require them.
4. Try adopting a module in a fresh project, including its import and any host adapter. Publish the first supported modules with documented semantics, compatibility expectations and runnable examples. The existing JSON-file module and the collection algorithms can mature independently.

### Generic value representation qualification

The first compact reference-target candidate lowers generic value width from 136 to 104 bytes and reduces peak memory, but adds allocations to borrowed search and sorting. It is not adopted. The runtime remains on the measured 0.305 layout while compact projection storage is investigated. This private metadata experiment changes no collection ownership or public library contract. [Results and reproducible candidate](../experiments/reference_layout/README.md)

An inline boxed-path candidate also remains experimental. It reduces `V` to 128 bytes at unchanged borrowed-search allocation counts, but nominal buffer shrinking can retain more allocator memory and the complete timing comparison still regresses some workloads. An exact-capacity move explains and repairs the peak regression; it does not establish a general speed win. The initial qualification therefore retains flat storage and the current generic layout. Next, exercise ordered bounds using the same sorted-input and consistent-comparator obligations already required by `merge`, then complete fresh-project adoption. [Second candidate and allocator evidence](../experiments/reference_layout/README.md#inline-target-with-compact-projection-storage)
