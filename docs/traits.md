# Trait checkpoint design

Compiler 0.26 implements concrete traits and associated types (B2a), plus
trait-constrained generics and associated sealed capability bounds (B2b). The
[data and abstraction direction](type-system.md) records the accepted principles.

## Accepted qualification

The user selected explicit trait/type qualification on 25 October 2026. Name both
the trait and its implementing type when using an associated type or operation.
The generic `read_one` below is executable with `read_result` declared:

```tellur
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

Here `reader<r>` selects the implementation of `reader` for `r`; it is not a runtime
value or a generic data type. Within the trait, `self`, `item` and `error` have
lexical meanings. Outside it, qualification distinguishes associated types from
fields, modules and unrelated traits with members of the same name.

## Explicit conformance

Executable implementation form (with `text_source`, `read_error`, `read_result` and
`next_text` declared in the program):

```tellur
implement reader for text_source {
    type item = string;
    type error = read_error;

    control read(ref mutable self input) exits read_result<item, error> {
        forward next_text(input);
    }
}
```

An implementation supplies every associated type and required control exactly once.
Unknown members, missing members and duplicate assignments are errors. Method
parameters keep their ownership modes. The declared exit boundary has the same
labels, payload arities and payload types after associated-type substitution. Label
order is irrelevant. An author can write an explicit adapter when a useful operation
has a different boundary; conformance does not insert one silently.

`type item: clone;` requires a sealed permission for an associated type. An
unbounded associated type in a generic witness carries the same conservative permissions as an
unconstrained control parameter. User-defined traits cannot replace or redefine the
sealed `copy`, `clone` or `drop` permissions.

No inherited storage, implicit subtyping, implicit conformance, default methods or
runtime dispatch is introduced. The initial implementation profile uses a concrete
owned target type. Blanket implementations and specialization need a separate overlap
model and library use cases before addition.

## Coherence, identity and privacy

There must be exactly one implementation for a given trait/type pair in a compiled
module graph. Adding an import cannot choose a different implementation by priority
or source order. Diamond imports preserve one file identity and one implementation.

The implementation's module must own either the trait declaration or the outer
nominal type declaration. Merely using a local type as an argument of a foreign
container does not confer ownership of that container. Builtin and foreign manifest
types have no local owner, so their implementations require a locally owned trait.
Duplicate pairs are rejected even when their bodies are identical.

Implementations are coherent declarations, not selectively imported extension
methods. Their use still requires visibility of the trait and target type. Method
bodies have their defining module's access rights. A visible implementation must not
expose a private associated type through a public trait/type boundary. Implementing
a trait for a type does not itself grant access to that type's private fields.

## Checking and lowering

All five steps are implemented in 0.24.

1. Parse declarations and implementation members with precise spans. Resolve trait
   identities, implementing types and associated members separately from data-field
   and module lookup; do not overload textual suffix replacement to resolve them.
2. Normalize associated types with cycle detection and explicit depth/size budgets.
   Validate complete implementation contracts and ownership/coherence before calls.
3. Check generic bodies against abstract trait requirements. Each witness exposes
   only declared operations and associated-type bounds. An unused invalid generic
   body must fail; one current implementation cannot grant additional operations.
4. Check concrete implementations, select the unique witness for each call and lower
   static dispatch into ordinary checked control calls. Recheck concrete boundaries.
5. Preserve source locations and readable trait/type names in diagnostics, hover,
   outline and graph provenance. Native execution and certified expansion operate
   on the resulting concrete graph.

Borrowed trait outputs require an explicit `from input` owner contract in 0.25.
The input must be borrowed, and exclusive outputs require exclusive inputs. Concrete
bodies prove the declared relationship; abstract generic witnesses assume only that
relationship. A private field can be returned without exposing its projection in the
public signature. See [lifetime contracts](lifetime-contracts.md) and the
[borrowed trait example](../playground/borrowed_traits.tellur).
Runtime polymorphism remains D1 and has no implicit fallback here.

## Implementation sequence and evidence

- **B2a (delivered in 0.23):** trait declarations, associated assignments, concrete implementations,
  coherence/privacy and explicitly selected concrete calls.
- **B2b (delivered in 0.24):** user trait bounds, abstract witnesses, associated projections and static
  calls inside generic controls; associated sealed bounds.

Both checkpoints need negative tests for incomplete or mismatched contracts,
ambiguous/foreign implementations, private-type leaks, reference escapes and cyclic
associated definitions. Module aliases must preserve identity. Generic definition
errors must be independent of current callers. Runnable examples, unsaved editor
checks, native/interpreter agreement and certified expansion complete each release.

## Current profile

Trait methods and implementations use `control` and explicit exit contracts.
Implementation contracts are compared before method bodies: parameter types and
borrow modes must match, and labels/payload types must match independent of label
order. Parameter and payload variable names may differ. Trait declarations are
validated even without implementations; unused implementation bodies are checked.

Implementing targets are explicit concrete owned types. Associated definitions may
refer to `self`, sibling associated names or other concrete trait projections;
normalization rejects cycles and caps depth at 64 and type spelling at 16,384 bytes.
There are at most 1,024 concrete implementations per compilation. Trait-bound generic data declarations, blanket implementations, generic methods, default bodies and
borrowed associated types are outside this release. Generic controls can call a
fixed concrete implementation, and implementation bodies can call existing generic
helpers. `outcome<reader<text_source>.read>` retains producer-indexed capture semantics.

See [the runnable example](../playground/traits.tellur). Qualified method identities
remain in checked graphs and provenance, so expansion preserves the author's trait
and type selection without adding runtime dispatch.


## Abstract definition checking (0.24)

Each generic type parameter receives a distinct opaque type and exactly its declared
sealed permissions. Each required trait supplies distinct associated opaque types
with their declared sealed permissions, and signature-only operations. This witness
is an assumption used to check the generic body; it is not an executable method body.
The compiler discards the entire abstract program after definition checking. Concrete
implementations are checked separately and their bodies are used in the returned graph.

Every concrete type argument must have the required coherent implementation, even
when the type parameter is unused. Generic-to-generic calls check the same requirements
against the caller's available witnesses. `reader<r>.item` remains distinct from
`writer<r>.item` even if today's implementations happen to assign the same type.
Lexical type substitution precedes associated projection lookup, so shadowed names
cannot accidentally select an existing concrete implementation.

Associated bounds currently accept sealed `copy`, `clone` and `drop`. Requirements
on generic controls can combine traits and sealed permissions with `+`. Associated
trait bounds, associated-type equalities, higher-kinded parameters and blanket impls
are not part of this profile. Definition checking caps abstract types and abstract
operations at 4,096 each, in addition to the existing concrete specialization budgets.

[The generic trait example](../playground/trait_bounds.tellur) demonstrates a reader
used through a checked generic control. Borrowed trait outputs use `from input` in
0.25; ordinary generic controls may also infer origins from their checked bodies.
