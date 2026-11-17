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

## Proposed algorithm contracts

| Family | First useful contract | Observable behavior |
| --- | --- | --- |
| Membership and search | Key membership by shared borrow. A sequence search returns a borrowed first match or an explicit miss. | Search visits elements in sequence order and stops at the first match. The match borrows the collection. A miss releases the loan. |
| Borrowed traversal | Start with shared array/vector slices over a half-open range and ordered key/value traversal for maps. | An empty range at either end is valid. A reversed or out-of-bounds range takes an explicit failure exit. Reads can coexist. Structural mutation waits until the last conflicting view or returned reference is released. |
| Reduction and early exit | Fold a borrowed sequence with an owned accumulator. An outcome from the step control can stop the fold. | Visit in order. Specify who owns the accumulator on each exit and preserve the step's declared outcomes and origin through expansion. A fallible step that consumes an accumulator must return, transfer or settle it on that path. |
| Consuming partition | Move each input to one of two outputs while preserving relative order in each. | A classifier borrows the current element. If it stops early, return both completed partitions, the current element and the untouched remainder. Every owner has a destination, including must-complete resources. |
| Ordering | Stable sort with an infallible ordering for the first version. Add ordered bounds/search once the caller can supply evidence for the ordering. | Equal keys keep input order. Moving records during sorting preserves their resource obligations. A later fallible comparator needs an explicit contract for collection state on early exit. |

For the first search and fold prototypes, use a statically named predicate or step control with a closed outcome contract. Try Villanelle's existing trait dispatch for reuse across element types. The design question is how callers route a step's additional outcomes without losing where they arose. A two-outcome predicate, a fallible predicate and a step with an owned resource are three distinct tests. Closure syntax and a universal iterator trait can be considered after those examples.

Keep read-only traversal and consuming traversal independently useful. In particular, a vector containing file handles can be inspected through shared borrows and later drained into explicit close operations. Existing whole-collection borrow checks apply even if a particular storage layout happens to keep addresses stable.

## Two representation experiments

**Ordered string maps.** Prototype an adaptive radix tree (ART) behind the current map contract. Its nodes adapt to branching density and its key traversal follows byte order. This fits Villanelle's lexicographic UTF-8 string order and makes a bounded prefix scan a plausible later operation. Compare against today's sorted flat map and an ordered B-tree. Start with the tiny HTTP-header maps as well as larger settings maps with common prefixes. A compact flat form for small maps with promotion to a tree is a candidate. Measure the crossover and the cost of promotion before selecting it. Empty strings, embedded zero bytes, a key that prefixes another, multibyte keys, and equal-key replacement must all preserve Villanelle's behavior. [ART paper](https://db.in.tum.de/~leis/papers/ART.pdf) · [Ordered B-tree baseline](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html)

**Vectors with cheap split and concatenation.** Prototype a relaxed radix balanced tree (RRB) for a workload that repeatedly joins batches and splits them again. RRB trees support logarithmic concatenation and splitting through a tree of chunks. Compare with a flat vector for append, random access, scanning, split and join. Include a flat vector with a moving start index as a baseline for front drains. First try consuming split and join, where both the old owners and the destination of each element are clear. Any persistent snapshot API would need its own eligibility rules for element identity, mutation, explicit cloning and resources. Tree sharing alone cannot grant those permissions. [Bagwell and Rompf's RRB paper](https://infoscience.epfl.ch/record/169879/files/RMTrees.pdf)

Record time per operation, allocations, peak live bytes, and bytes or elements moved. Report key lengths, collection sizes, hit/miss ratios and scan order with the results. Compare complete Villanelle operations as well as the isolated data structure. Today front-draining a flat vector or map can take quadratic time across a full drain. A linear full ordered traversal with no element clones is a useful first target. The existing collection conformance cases cover key order, value replacement, invalidation and resource completion. Run them against each candidate and compare original and expanded behavior in the interpreter and LLVM executables. [Collection conformance cases](../tests/collections.rs)

The first ordered string-map comparison is complete on Apple M2. Linear lookup was fastest for eight headers. At 4,096 shared-prefix settings keys ART led on lookup, the B-tree led on insertion and draining, and flat storage led on borrowed scans. The 0.286 source prototype scans the existing flat map by borrowing each pair. In the native settings workload, scanning 4,096 entries through the library took 22.1 ms and draining took 92.4 ms, including process startup and decoding. [Measurements and reproduction](../experiments/ordered_maps/README.md)

## Path to a supported library

1. Exercise the existing public-outcome change on both JSON callers and use it to document failure routing and provenance. The results are in the [language roadmap](language-revision.md#active-checkpoint-queue).
2. Specify the ownership and outcome contracts above using caller examples. Start with membership and one borrowed traversal. Prototype the ordered string map alongside them.
3. Evaluate higher-order algorithms and the sequence representation with real callers. Introduce slices, generic bounds or additional origin contracts as the examples require them.
4. Try adopting a module in a fresh project, including its import and any host adapter. Publish the first supported modules with documented semantics, compatibility expectations and runnable examples. The existing JSON-file module and the collection algorithms can mature independently.
