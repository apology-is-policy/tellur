# Generic control checking

Compiler 0.26 implements first-order, explicitly instantiated controls with sealed
capability and trait bounds. The user-facing syntax is in the [language reference](language-reference.md#generic-controls).
Borrowed trait results use declared owner anchors in 0.25. Stored references are next in the [roadmap](language-revision.md#active-checkpoint-queue).

## Checking pipeline

1. Resolve file identities and lexical type parameters. Reject recursive control
   dependencies, including cycles that change type arguments.
2. Give each generic definition distinct opaque type parameters. Each receives exactly
   its declared permissions; unconstrained parameters are owning, non-Copy and
   must-complete. Check every definition, including unused definitions,
   and infer its parametric exit contract where requested. Trait requirements introduce
   distinct associated opaque types and signature-only operations. These assumptions
   are discarded after checking; they never become executable stubs.
3. Instantiate requested concrete controls and data layouts. Validate every type
   argument, including arguments unused by the body or representation. Check sealed bounds
   against the instantiated layouts before checking concrete bodies.
4. Recheck the concrete program's types, ownership, path coverage and lifetime
   relationships, then run the structural IR verifier.
5. Compile or interpret that concrete graph. Expansion certificates validate rewrites
   of this graph using the existing correspondence rules.

Abstract checking identities cannot be spelled in source and never enter the emitted
program. Lexical value bindings take precedence over constructor fallback; type
parameters shadow outer constructor names. Imported controls retain their defining
module's access rights after specialization. Public generic interfaces are checked
at their definitions. A private caller's specialization is not a separately exported
API, but a public caller still cannot expose private types through its own boundary.

The abstract check prevents a concrete call from granting its generic definition
accidental capabilities. For example, a definition that duplicates `item` is rejected
even if every current caller supplies `u32`. Distinct parameters stay distinct even
when one caller supplies the same concrete type twice. Known structure remains
available: `box<item>` has its declared field, while an unconstrained `item` has no
inspectable fields or completion alternatives.

This is implemented checking and differential evidence, not a mechanized theorem
that one symbolic run proves all future language extensions sound. New operations
must specify which type properties they require. Concrete layout and body checks
remain mandatory. There is no general proof yet that source-level generic
specialization commutes with every proposed Tellur algebra operation.

## Sealed capabilities

The B1c spellings are `item: copy`, `item: clone`, and `item: drop`, with
`+` available to combine requirements. These are executable in 0.22.

- `copy` permits implicit value duplication, explicit cloning and ordinary disposal.
- `clone` permits explicit total cloning and ordinary disposal, while ordinary
  variable use still moves the value.
- `drop` permits ordinary disposal without promising copying or cloning.

The implication chain is `copy => clone => drop`. These permissions are checked by
the compiler and derived through fields and collection elements. Outcome handling
and resource completion obligations prohibit these permissions where applicable.
A bound never authorizes abandoning an outstanding completion obligation.

In this design, `drop` is permission to discard ordinary storage, not a user-defined
cleanup hook. Cleanup that can fail remains an explicit control with its real exits.
Likewise, the builtin `clone` permission does not allow user code to disguise an
effectful or failing duplication operation as a pure expression. Such operations
belong in explicit control/trait contracts.

Definition checking uses exactly the declared permissions. A `drop`-only generic
body must not gain cloning merely because today's concrete argument happens to be
cloneable. Calls check requirement implication before checking the instantiated
body, so missing-bound diagnostics point to the caller. Concrete operation traits, associated types and coherence ship in 0.23. Trait
requirements in generic definitions and associated sealed bounds ship in 0.24;
see [abstract witness checking](traits.md#abstract-definition-checking-024).

## Limits

The current profile has no type-argument inference, higher-kinded parameters,
recursive control calls or runtime-selected generic dispatch. Type arguments are
owned types; borrowed parameters use `ref item` or `ref mutable item`. Stored borrowed
aggregates remain a separate lifetime checkpoint. Concrete entry points call generic
controls; the CLI does not instantiate an otherwise unused generic definition solely
from an entry-name argument. Specialization budgets fail explicitly rather than
truncating the program.
