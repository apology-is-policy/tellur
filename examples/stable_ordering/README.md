# Stable ordering

Run from the repository root:

```sh
./bin/vnc check examples/stable_ordering/main.vn
./bin/vnc exec examples/stable_ordering/main.vn -- --scenario examples/stable_ordering/main.scenario.json
```

The input keys are `[2, 1, 2, 1, 3]`. The comparator reads only the key;
`identity` records the original position. The sorted records are
`[(1, 1), (1, 3), (2, 0), (2, 2), (3, 4)]`. Equal keys retain input order.

The call consumes the source and returns all owners in `sorted`. No clone capability
is required. A comparator implements `ordering.comparator`, declares its `item`
and borrows two elements, returning `before`, `equal` or `after`. This first version
has no fallible comparator. The comparator must supply a consistent ordering.

The source module is experimental. Its bottom-up merge sort uses front drains,
which make current storage movement quadratic despite O(n log n) comparisons.
[Performance evidence](../../experiments/stable_ordering/README.md) records the
remaining native gap. The [resource fixture](../../tests/fixtures/owned_ordering.vn)
sorts tickets without copying their handles, observes comparisons and then closes
each handle once, including when a close reports failure.
