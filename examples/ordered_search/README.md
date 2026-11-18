# Ordered bounds and first equivalent record

From the repository root:

```sh
./bin/vnc check examples/ordered_search/demo.vn
./bin/vnc exec examples/ordered_search/demo.vn -- 2
```

The input keys are `[2, 1, 2, 1, 3]`, with identities `[0, 1, 2, 3, 4]`.
The program sorts them stably, then finds the records equivalent to key `2`.
It returns `present` with first index `2`, exclusive end index `4`, original
identity `0`, and all five records in sorted order. Change the command argument to `4` to get `absent` with insertion position `5`.

The comparator reads only each key. Sortedness and consistent comparison results
are caller obligations, as with `ordering.merge`; the checker does not prove them.
This example establishes sortedness by calling `ordering.sort` and does not change
the vector or ordering rule between sorting and querying.

`first_equal` returns a borrow from the source vector. The program copies the
identity and explicitly drops that borrow before returning the owned vector.
Resource-bearing elements follow the same lifetime rule and need no copying.
The manifest supplies the canonical checked-add/subtract/divide primitives used
by the source binary search. Native execution supplies their live arithmetic;
interpreter scenarios must script those primitive calls explicitly.

[Public contract](../../docs/collections-and-algorithms.md#ordered-bounds-and-first-equivalent-search)
· [Complete-operation performance evidence](../../experiments/ordered_bounds/README.md)
