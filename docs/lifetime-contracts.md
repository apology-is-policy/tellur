# Lifetime contract checkpoint

The user accepted parameter-based `from input` origins on 26 October 2026.
Compiler 0.25 implements C1a: declared reference results, borrowed trait contracts,
implementation conformance and abstract generic witnesses. Compiler 0.27 delivers local struct storage (C1b.1), and 0.29 adds grouped struct results (C1b.2a) and field-sensitive last use. Whole-record inputs and contained reborrowing arrived in 0.30–0.31; 0.32 adds checked opaque splits. Local borrowed sums arrived in 0.33, public sum schemas in 0.35/0.37, and whole-sum borrowed inputs in 0.38. Direct whole-carrier reference results arrived in 0.39; local aggregate/captured storage follows in 0.40. Public contracts for containers of those references follow in 0.41; aggregate trait contracts follow in 0.42. Local shared-reference collection elements follow in 0.43; exclusive elements and public collection contracts remain ahead. See [precise origins](precise-origins.md). Examples use the reference words introduced in 0.26. Existing direct-control results may still infer origins from their bodies.

## Accepted surface

```tellur
trait reader {
    type item;
    control peek(ref self input)
        exits found { ref item value from input }, empty;
}
```

`from input` names the input that keeps the returned borrow valid. It does not
copy, move, fetch or construct a value. The output retains its ordinary payload
type and name. The implementation must establish that the actual reference comes
from the stated input. A reference into a local temporary cannot satisfy the promise.

Each direct borrowed payload may name one input parameter. The input must itself
be borrowed. Trait signatures and implementations must declare every borrowed result's
origin; their parameter positions must match, even when local names differ. The
implementation body is checked against the promise. Ordinary controls may opt in per
payload, or keep existing inferred origins.

A declared anchor hides the exact field projection. It conservatively protects the
whole passed subobject, even if the implementation happens to return one small field.
Projection through the returned reference does not invent a projection through the
owner's unrelated type. References from an inferred direct-control contract retain
exact paths where known. No implicit cloning or address copying is introduced.

Declared contracts cannot yet spell an explicit union of inputs or expose specific
private subfields. Multiple outputs may name one input, including exclusive outputs.
In 0.32, every implementation must establish their separation at exit construction;
the caller retains the owner anchor while receiving independent access permissions.
An inferred contract can represent unions internally.

## Semantics independent of the spelling

The origin contract must describe a loan's owner separately from the reference's
pointee type. A `reader` can return a reference to an associated item inside `self`
without exposing the private field path that locates it. A trait signature promises
the ownership relationship; a concrete implementation supplies and checks the actual
projection and address. Static specialization retains the concrete implementation.

A shared output may originate from a shared or exclusive input. An exclusive output
requires exclusive input permission, and multiple outputs must preserve disjointness
or exclusivity rather than duplicating an exclusive loan. An implementation may
return a narrower subobject than the public origin anchor promises. It must not
return a reference to an unrelated owner.

Multiple possible input origins need an explicit union rule. Such a return remains
constrained by every possible origin while the compiler cannot distinguish the path
that produced it. Path-sensitive provenance can recover precision when a decision
identifies that path. This is a type/ownership obligation, not a new outcome or effect.

Public contracts can name parameter anchors without revealing private representations.
Field-specific anchors, where useful, must respect module visibility. Public checking
must not depend on inlining the callee or reading an implementation's private fields.

## Work order

1. **C1a — delivered in 0.25:** represent origin anchors separately from
   concrete projections, validate implementation conformance, then support borrowed
   trait outputs and their abstract witnesses. Preserve existing inferred direct-control
   returns. Add negative tests for local escape, wrong input, mutability weakening,
   exclusive aliasing and imported private representations.
2. **C1b — stored borrowed values:** carry reference provenance through struct fields,
   ordinary variants, named outcomes and collection elements. Moves and partial moves
   transfer their contained loans; disposal releases them. Copies of shared references
   and exclusive-reference moves must follow the existing ownership model.
3. **C1c — generic propagation and reference state:** propagate contained-loan
   information through generic boundaries, assignments, returned aggregates and
   outcome adapters. Establish the reborrowing and rebinding rules needed by slices
   and non-consuming iteration in C2.

Each step needs interpreter/native agreement and tests with both original and expanded
control graphs. Stored references require recursive runtime/native handling as well
as checker changes; accepting new syntax alone is not completion of this checkpoint.

## Representation constraints

Borrow-return metadata now distinguishes exact pointee projections from opaque owner
anchors. Both retain the input parameter and an anchor path. Exact sources still check
projected type equality; opaque sources carry the declared output type independently.
The body proves the owner relationship, while abstract witnesses assume only their
declared promise. Projecting an opaque result preserves its owner anchor unchanged.

Contained loans also need to survive moving a record or an outcome between scopes.
Tracking only a local reference variable's name is insufficient. The checker needs
loan information attached to values/places and guarded by the same path selections
as their ownership. Native calls must preserve the referenced storage's identity and
lifetime across nested payloads. These requirements also prepare for future coroutine
frames, but this checkpoint does not introduce suspension or asynchronous execution.

## Local record storage — delivered in 0.27

A loan now records its containing storage place separately from its pointee and
origin anchors. Local constructors register each reference before checking later
initializers. A record copy clones shared loans; a move transfers contained loans;
partial moves transfer only the selected field subtree. Extracting an exclusive
reference consumes its field. Scope/drop/last-use release preserves independent
copies and reborrows. Source privacy and path guards remain enforced.

This first local checkpoint uses checked stable pointee aliases for field access.
The runtime record itself carries reference values, and interpreter/native execution
and certified expansion are tested. This does not establish an arbitrary reference
projection representation. The 0.29 result checkpoint below extends this to returned
records. Whole-record inputs, stored collections/outcomes, whole-record borrows and
reassignment remain unsupported. Generic struct fields such as `ref t` work locally; generic
reference type arguments and aggregate control arguments are still unsupported.

## Precise origin groups — record results delivered in 0.29

The user selected precise groups on 26 October and the schematic `['a]` / `<-`
notation. Compiler 0.29 implements public group declarations, nested/generic field
mappings and positional input bindings on returned records. `ready...` infers both
the record type and its group dependencies. Field-sensitive last use releases a
source only after its last retaining reference is finished.

The [precise-origins guide](precise-origins.md) separates executable syntax from
remaining carrier-input, trait, permission, collection and coroutine work. Direct
`from input` contracts retain their semantics. Different group names do not establish
disjointness. Compiler 0.32 supports opaque splits checked from their implementations;
see the guide for the separation between access permissions and lifetime anchors.


## Public nested-reference contracts — delivered in 0.41

`ref mutable view['text] target <- 'storage` maps the referent's contained group
separately from its storage. Each public dependency path records reference-indirection
steps as well as record fields and guarded alternatives. Owned/borrowed container
inputs and results preserve all layers, including captured and forwarded results.
The checker reconstructs the authorization chain without reading inactive payloads;
shared outer authority cannot grant exclusive inner access. Public group selectors
retain storage guards where needed. The independent IR verifier checks every layer.
See [the reference](language-reference.md#public-nested-reference-contracts) and
[the example](../playground/public_carriers.tellur).

Aggregate trait origin contracts arrived in 0.42 and local shared-reference
collections in 0.43. Exclusive/nested elements, public collection schemas, abstract
generic origin-shape bounds, reference rebinding and loop state remain separate work.
