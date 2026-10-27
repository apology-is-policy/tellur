# Data and abstraction direction

Accepted direction, 25 October 2026. This document records design commitments;
examples marked proposed are not a claim of implemented syntax. The ordered work
queue is in [the roadmap](language-revision.md#active-checkpoint-queue).

Tellur will combine nominal algebraic data types, explicit generic constraints,
traits with associated types, and ownership-aware control signatures. Composition
provides reuse. There is no implementation inheritance or implicit struct subtyping.
Type-first declarations, snake_case names and explicit public contracts remain the
readability defaults; `auto` remains available for local inference.

## Distinct responsibilities

| Construct | Responsibility |
| --- | --- |
| Struct | A named product of fields |
| Ordinary variant | One of a closed set of data alternatives |
| Outcome | A completed result with a handling/transfer obligation |
| Trait | A named collection of required operations and associated types |
| Stateful protocol | Permitted operation sequences, transitions and outstanding obligations |

An outcome declaration describes completion alternatives, not a whole temporal
protocol. Ordinary variants do not acquire mandatory handling merely because their
representation resembles an outcome. Resource and nested outcome obligations still
propagate through structs, variants and collections.

## Data declarations

Expose the existing record/variant semantics in Tellur source. Struct fields are
written `type name;`, with named nominal types and composition. Variant alternatives
use the existing outcome-style brace payload declarations, with comma separation.
Construction supports positional braces and named struct fields (0.19), with
qualified variant constructors. Named initializers preserve written evaluation order;
field names do not authorize an implicit conversion. Forward declaration order within one compilation unit must not matter.

A one-field struct already provides a distinct wrapper: `user_id` and `invoice_id`
remain different even when both wrap `u32`. We need no additional wrapper keyword
for that capability. Layout transparency and ABI guarantees are separate future
questions. Copy, owning moves and must-complete obligations derive from fields under
the existing rules. Resource completion must never become an implicit destructor
that hides failing cleanup.

The accepted module spelling is `import "people.tellur" as people;`, with explicit
public declarations and private-by-default internals. Paths are resolved relative
to the importing file. Logical package-name resolution is deferred. Source data
declarations shipped in 0.17; [file modules and visibility](modules.md) shipped in
0.18, including private representations, interface checks, cycle diagnostics and
editor dependency tracking. Resolution does not depend on global import order.

## Generics and traits

Generic structs, variants and outcomes shipped in 0.20, with explicit type arguments,
finite checked layouts and ownership derived from fields. Unconstrained generic
controls shipped in 0.21: definitions are checked with opaque, must-complete parameters,
then concrete specializations are rechecked. Sealed `copy`, `clone` and `drop` bounds
ship in 0.22. Concrete traits and associated types ship in 0.23; trait-constrained
generic controls and associated sealed bounds ship in 0.24. [The checking pipeline](generic-controls.md) describes
the current generic permissions.
Check generic bodies
against declared capabilities, not accidental properties of one instantiation.
Unconstrained owning values cannot be implicitly cloned or abandoned when they may
carry a completion obligation. Diagnostics should explain the missing capability.
Begin with first-order type parameters and deterministic static dispatch. Higher-
kinded parameters, type families and richer type-level computation require concrete
library use cases before inclusion.

Executable generic trait syntax (with the outcome declared below). The [trait checkpoint design](traits.md) records
implementation/coherence rules and the accepted qualification for this sketch (`reader<r>.item` / `reader<r>.read`):

```tellur
outcome read_result<item, error> {
    received { item value },
    end,
    failed { error cause }
}

trait reader {
    type item;
    type error;
    control read(ref mutable self input) exits read_result<item, error>;
}

control read_one<r: reader>(ref mutable r input)
    exits read_result<reader<r>.item, reader<r>.error>
{
    forward reader<r>.read(input);
}
```

Traits have explicit conformance, associated types and one unambiguous applicable
implementation. An implementation must meet declared ownership modes, borrowed
return relationships and the complete outcome boundary. Extra exits cannot silently
appear. Module ownership/coherence rules will prevent imports from changing which
implementation is selected. Multiple trait requirements express composition, without
inheriting stored fields or creating implicit subtype conversions.

Compiler-recognized capabilities such as safe copying and ordinary destruction are
not arbitrary user promises: implementing an ordinary trait cannot authorize unsafe
copies or erase resource obligations. Ordinary clone operations and cleanup controls
must retain their actual outcome/effect contracts.

Runtime polymorphism will be explicit and separate from static generic dispatch.
Its design must specify erased associated types, supported signatures, borrowing,
storage and dispatch costs. We will not promise that every trait can be erased.

## Ownership, effects and transformation laws

Local struct references are implemented in 0.27; 0.29 adds grouped record results and field-sensitive last use. Whole-record inputs and borrowed
outcomes require the [precise origin-group design](precise-origins.md), selected for
design before implementing multi-input contracts. Generic
interfaces must express those relationships before borrowed generic containers and
async captures can be soundly supported. Activation identities and stable frame
storage are additional async prerequisites.

An outcome contract is not an effect summary. Two `done` controls may perform
unrelated I/O. Reordering, duplication and elimination require justified effect and
ownership conditions. Generic/trait abstractions must carry the information required
by the transformation that consumes them.

Statically resolved implementations can be expanded with correspondence and
provenance. Runtime-selected calls may remain opaque boundaries; their graph display
must not invent a selected body. Stateful protocols and async laws will describe
causal order, task scope and cleanup, including failure and cancellation.

## Influences and limits

Haskell's algebraic data types, polymorphism and type-class constraints are useful
influences; adopting them does not require changing Tellur to lazy evaluation or
implicit public signatures. Rust-style traits and Swift-style associated types are
also relevant precedents. Tellur retains its borrowing and path-sensitive obligations.
There is no commitment to reproduce any one language's complete type system.

Background: [Haskell types and classes](https://www.haskell.org/onlinereport/haskell2010/haskellch4.html),
[Rust traits](https://doc.rust-lang.org/reference/items/traits.html),
[Rust implementation coherence](https://doc.rust-lang.org/reference/items/implementations.html#trait-implementation-coherence),
[Swift associated types](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/generics/).
