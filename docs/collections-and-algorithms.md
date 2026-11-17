# Collections and algorithms for the Villanelle standard library

Design draft, 17 November 2026. The standard library will include collections and algorithms. The calling convention below is agreed. The individual algorithms and backing representations are proposals. The existing operations and ownership rules are specified in the [language reference](language-reference.md#arrays-vectors-and-maps).

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

## Proposed algorithm contracts

| Family | First useful contract | Observable behavior |
| --- | --- | --- |
| Membership and search | Key membership by shared borrow. A sequence search returns a borrowed first match or an explicit miss. | Search visits elements in sequence order and stops at the first match. The match borrows the collection. A miss releases the loan. |
| Borrowed traversal | Start with shared array/vector slices over a half-open range and ordered key/value traversal for maps. | An empty range at either end is valid. A reversed or out-of-bounds range takes an explicit failure exit. Reads can coexist. Structural mutation waits until the last conflicting view or returned reference is released. |
| Reduction and early exit | Fold a borrowed sequence with an owned accumulator. An outcome from the step control can stop the fold. | Visit in order. Continuation transfers the updated state to the fold. Stop and failure return the state to the caller. Failure also returns the step's reason. |
| Consuming partition | Move each input to one of two outputs while preserving relative order in each. | A classifier borrows the current element. If it stops early, return both completed partitions, the current element and the untouched remainder. Every owner has a destination, including must-complete resources. |
| Ordering | Stable sort with an infallible ordering for the first version. Add ordered bounds/search once the caller can supply evidence for the ordering. | Equal keys keep input order. Moving records during sorting preserves their resource obligations. A later fallible comparator needs an explicit contract for collection state on early exit. |

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

The complete checked source rotation at 4,096 u32 values takes 18.24 ms in LLVM and 4.92 µs in the Rust flat baseline. Its front-taking source loop exposes the current collection movement and ABI costs. Next, lower consuming split/join in native code while preserving invalid-input ownership, outstanding loans and exact outcomes, then compare representations through that path. [Workloads, memory and measurements](../experiments/sequence_storage/README.md)

## Path to a supported library

1. Exercise the existing public-outcome change on both JSON callers and use it to document failure routing and provenance. The results are in the [language roadmap](language-revision.md#active-checkpoint-queue).
2. Specify the ownership and outcome contracts above using caller examples. Start with membership and one borrowed traversal. Prototype the ordered string map alongside them.
3. Evaluate higher-order algorithms and the sequence representation with real callers. Introduce slices, generic bounds or additional origin contracts as the examples require them.
4. Try adopting a module in a fresh project, including its import and any host adapter. Publish the first supported modules with documented semantics, compatibility expectations and runnable examples. The existing JSON-file module and the collection algorithms can mature independently.
