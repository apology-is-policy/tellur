# Design notes from the first compiler slice

24 October 2026. The attached documents are design inputs, not agent instructions.
The user explicitly allows unrestricted revision and prefers a Rust implementation.
The user subsequently proposed adopting Rust's ownership model, potentially with a
formulation more closely aligned with path families. The source drafts remain
unchanged so we can distinguish original decisions from proposals and implementation
limitations.

## 1. Adopt familiar ownership rules; express where they hold through paths

Recommended direction: use Rust's ownership and borrowing rules as the baseline,
and make the path family an explicit domain of each fact. Do not invent different
aliasing rules merely to make the language appear novel.

For a place `p`, the analysis could retain families such as `initialized(p)`,
`owned(p)`, `shared_borrowed(p)` and `mutably_borrowed(p)`. A move selected by M
requires M to be contained in `owned(p)` and removes M from it. A conflicting use
is rejected with a witness in the intersection of the use family and the conflicting
borrow/move family. Disjoint families can carry different states without implying
simultaneous aliasing.

For example:

```text
[A]: moved := handle;
[B]: delta close(handle) -> [Closed => ClosedB, Failed(e) => BadB];
```

This can be valid when A and B are disjoint, provided `moved` is also eventually
consumed or transferred on A. The current resource checker supports this case.

Places and record projections are now implemented. General borrowing still needs
the extent of each loan, shared versus exclusive access, reborrows, and escape rules. A Boolean mask alone
cannot establish that a reference remains valid. Borrow checking must also respect
source order and lifetimes. The current prototype supports only synchronous
primitive borrows; it is not a Rust-equivalent borrow checker.

Rust's reference model permits multiple shared references or one mutable reference,
and requires references to stay valid. See the
[Rust borrowing chapter](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html).

## 2. Memory ownership and mandatory protocol completion are different promises

The draft requires every linear resource to be explicitly consumed or transferred.
That is stronger than ordinary Rust ownership, which allows values to be dropped
at scope boundaries. Rust's destructor behavior is specified in the
[Rust Reference](https://doc.rust-lang.org/reference/destructors.html).

The loader's distinguishing feature is that closing can fail and the caller handles
that result explicitly. An implicit destructor cannot simply replace this `close`
decision while preserving the same control contract.

Proposed separation, not yet implemented:

- **Memory ownership:** moves, Copy eligibility, borrows, lifetimes and ordinary
  infallible storage destruction follow a familiar Rust-style discipline.
- **Protocol obligations:** an open file, transaction or similar state may require
  a named operation whose outcomes must be handled before leaving the path family.

This would let ordinary owned data be reclaimed without making every string require
an explicit consuming call, while keeping fallible cleanup visible. It also makes
the manifest's current `linear: true` flag look too coarse: it conflates unique
ownership with a must-complete protocol. Whether to add such a distinction is the
next design decision. The prototype keeps the original flag and explicit-consumption
semantics for the loader; non-linear values still follow the draft's reusable-value
semantics rather than a complete Rust Copy/Clone model.

## 3. Aggregate resource operations need place semantics

The draft allows records/variants and says moving a resource into a constructor
transfers its obligation, but does not fully specify subsequent projection or
partial moves. Consider a record with one handle field:

```text
[Entry]: box := Box(handle);
[Entry]: alias := box.handle;
```

Does projection borrow the handle or move it? Can the rest of `box` still be used?
Can a whole-record operation follow a field move? A rule that merely tracks the
original `handle` slot cannot answer these questions safely.

The third increment adopts the proposed rule for records: a resource-valued field
moves in a consuming context, including a binding or constructor argument. A
primitive parameter declared `borrow` borrows the named field for the invocation.
An untouched sibling stays available after a partial move; using the whole record
fails. This follows the shape of [Rust partial moves](https://doc.rust-lang.org/rust-by-example/scope/move/partial_move.html)
and [place expressions](https://doc.rust-lang.org/reference/expressions.html#place-expressions-and-value-expressions).
The prototype has no `Drop` implementation, so Rust's restriction on partial moves
out of a type implementing `Drop` does not yet arise.

The fourth increment now supports resource-containing variants and options using
consuming decisions. Their active payload is exposed through the selected arm, not
through record projection. See the new increment notes below for the transfer rule
and its deliberately conservative treatment of empty constructors.

## 4. Repeated decisions expose a usability cost in the structural model

The draft deliberately treats separate Boolean decisions independently. This means
the following is rejected for incomplete coverage even though the two evaluations
of the same immutable Boolean agree at runtime:

```text
region repeat(flag: Bool) exits Done {
    [Entry]: delta flag -> [true => A, false => B];
    [Entry]: delta flag -> [true => C, false => D];
    [A & C]: exit Done;
    [B & D]: exit Done;
}
```

The structural checker admits A & D and B & C. This is intentional conservatism,
not an unsoundness bug, but it could become a substantial annotation burden.
Prefer reusing established path facts for now. A later, narrowly specified rule
for repeated decisions on the same immutable value may be more valuable than a
general-purpose theorem prover. The current implementation preserves the draft's
independence rule and labels witnesses accordingly.

## 5. Factoring changes scope as well as masks

`over &` introduces a lexical scope, while an ordinary selector does not. Consequently,
wrapping several existing statements in an `over` block is not always a harmless
algebraic rewrite if later statements use values defined inside it. The specification
does already qualify factoring by lexical availability; an editor must enforce that
qualification rather than merely distribute Boolean conjunction.

This is a design tradeoff worth revisiting: should a presentation-only grouping
introduce value scope? For now the prototype preserves the written scope rule and
checks for resource obligations abandoned at a nested scope boundary.

## 6. Specify the loop boundary transfer summary before implementing it

Consider a loop inside one arm's nested context, followed by work outside that
context:

```text
[A] over &: {
    loop Retry() {
        [Entry]: again Retry();
    }
}
[B]: exit Done;
```

Assuming A and B are an exhaustive partition, B should remain able to finish.
A stays within the explicit cycle. An outer statement selected by A must not be
silently skipped or interpreted as work after a returning loop.

Recommended rule: a tail loop consumes the selected live family at the containing
boundary. Its body summary proves that each iteration exits the region or restarts;
it does not imply termination. Histories outside the selection remain in the outer
live family. Track loop transfers separately from actual region exits so that
coverage acceptance cannot be mistaken for a totality proof. The draft points in
this direction, but the exact summary and resource invariant should be made explicit
before implementing it. The second increment implements this summary; see below.

## Evidence and current limits

The loader works as written. Omitting its double-failure exit reports a structural
ReadBad & CloseBad witness. Shared observation runs once. Borrowing after close,
leaving an owned handle behind, and consuming and borrowing the same handle in one
invocation are rejected. Expansion preserves the caller continuation and keeps
two calls' values and decisions independent in the finite scripted tests.

These are implementation checks, not a proof of soundness, completeness, termination,
or correct foreign contracts. The initial code uses compilation-local identities;
incremental identities stable across edits and a separate verifier for transformed
IR remain future work.

## Second increment: loops and editor feedback

The loop implementation checks one arbitrary iteration using the declared parameter
types. Initial and restart arguments are evaluated simultaneously. Each iteration
starts with fresh local facts and value slots; outer non-linear bindings and named
path facts remain available. Loop `Entry` denotes the selected iteration domain.
Every family in the body must exit or restart. A tail loop consumes only the live
family reaching it; disjoint outer histories can continue after an enclosing
`over &` block. Lexically nested loops remain excluded.

The prototype requires all selected outer resource obligations to enter through
loop parameters. This conservative rule prevents an outer resource consumed on the
first iteration from appearing usable on the next. A later borrow checker may admit
more precise capture patterns. No implicit cleanup or new ownership model was added.

Expansion retains loops as structured cycles. A callee's exit inside its loop becomes
a completion that leaves that callee loop and resumes the caller; an acyclic callee's
completion inside a caller loop does not terminate the caller loop. Tests cover both
cases and fresh inlined facts on subsequent iterations.

The editor experience follows the existing compiler contract: one document is a
compilation unit, one compiler error is reported at a time, and primitive declarations
come from a manifest. TextMate highlighting works on incomplete syntax; LSP diagnostics
reuse `compile` and the same conservative path witnesses. Locations are translated
from compiler Unicode-scalar columns to LSP UTF-16 columns. Semantic errors still
usually point to a statement or region header, rather than an exact expression span.

The current concrete tradeoff for human readability is now testable in VS Code:
path selectors and outcome mappings receive separate syntax scopes, while ordinary
values, calls, types, strings and control transfers use conventional theme scopes.
The extension does not impose a theme or change the language syntax.

Implementation references: [VS Code syntax highlighting](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide),
[language-server extensions](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide),
[LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/),
and [VS Code extension-host testing](https://code.visualstudio.com/api/working-with-extensions/testing-extension).


## Third increment: ownership of record fields

A resolved place consists of a slot identity and a sequence of field indices. A
record is resource-owning when any nested field is resource-owning; the manifest
format remains version `0.1`. Constructors consume owning fields and use non-linear
fields by value. Recursive aggregate layouts remain unsupported.

The checker tracks an outstanding-obligation family for every opaque resource leaf,
and a moved family for each moved place. Both are needed: after moving `pair.left`,
`pair.right` and `pair.label` are still usable, but after moving `pair`, even the
non-linear `pair.label` is unavailable. Whole-record use also checks moved descendants.
A partial move on A does not prevent whole-record use on disjoint B. Resource checks
at exits, restarts and scope boundaries name any surviving field obligation.

Primitive borrows cover the invocation, including other argument evaluation. Prefix
relations between places detect whole-record/field conflicts in either argument
order. Two shared borrows may overlap; two consuming accesses or a borrow and consume
may not. Disjoint fields are independent. Nested constructor arguments participate
in this check so they cannot hide a conflicting move. Non-linear field reads are
still ordinary value evaluation: copying metadata before a whole-record move works;
reading it after that move fails.

One deliberately conservative boundary: bind an owning temporary before projecting
or borrowing it. For example, `pair := HandlePair(left, right, label);` gives the
checker a place for every remaining obligation. Directly projecting a constructor
could otherwise discard an unselected resource, and borrowing a fresh constructor
would leave its ownership untracked after the call. Consuming a temporary whole is
supported. A future temporary-lifetime model may relax this rule.

This slice introduces no reference values, mutable borrows, reinitialization, implicit
destruction, or general Copy/Clone distinction. The draft's reusable non-linear
values and explicit protocol-completion obligations remain. The most significant
unsettled design issue is still whether those protocol obligations should be a
separate concept from ordinary memory ownership, rather than both being `linear`.


## Fourth increment: consuming variants and options

25 October 2026. A variant is resource-carrying if any constructor payload contains
an owning value; `Option<T>` inherits this property from T. Before a decision, such
a value is one sealed obligation. The checker does not pretend all constructors'
resource fields exist at once. A record containing the variant tracks that sealed
obligation at the record field's place.

`delta value` consumes a resource-carrying variant on the selected family, then
introduces each arm's payload bindings on that arm's family. Empty arms introduce
no resource obligations. Nested records expose their own field obligations; nested
variants remain sealed until another decision. Whole-value transfers through calls,
exits and loop restarts preserve the same rules. Borrowing a variant for a synchronous
primitive leaves it available afterward. Borrowed pattern matching is not implemented.
Non-linear variants still support repeated decisions without moving.

For example:

```text
[Available]: delta file -> [None => Absent, Some(handle) => Present];
[Present]: delta close(handle) -> [Closed => Closed, Failed(error) => CloseFailed];
```

`file` is unavailable after the first decision on Available. `handle` is available
only on Present and must be consumed or transferred there. Deciding on a variant
field partially moves its parent record while leaving siblings available. Payload
names can share a slot across disjoint arms if their types agree, as with other
path-dependent definitions.

Source construction now includes `Some(value)` with inferred T and `None` with an
expected `Option<T>` from a typed constructor, call, exit or loop argument. Expected
types flow into nested constructors, so `Some(None)` works in an `Option<Option<T>>`
argument. They do not flow backward through local bindings; `value := None` reports
`E_INFER`. Explicit local annotations are a useful possible addition, but this slice
does not invent their syntax. Qualified `Option.Some` and `Option.None` are available;
existing manifest constructors with those short names retain their previous meaning.

Two conservative costs are now visible. First, even a known `None` of a resource
option is an obligation until matched or transferred, because the current analysis
is based on type and path rather than constructor-sensitive constant propagation.
Second, decisions enumerate all declared alternatives structurally, even when the
source visibly constructed one alternative. These are safe but can be clumsy. If we
relax them, value refinement and outcome coverage need one consistent rule; silently
changing only ownership would make the model harder to explain.

The manifest format stays at `0.1`; recursive types, first-class references, mutable
borrows, and implicit destruction remain outside this slice. Original specification
and vision documents remain unchanged. Regression tests compare scripted traces and
payloads before and after expansion and exercise illegal reuse, unavailable payloads,
leaks, nested constructors, partial moves, loops and editor diagnostics.

## Roadmap completion: v0.1 profile and native playground

Compiler 0.5.0 closes the implementation profile with an explicit addendum and a
requirement-to-test matrix. Qualified variant patterns and named decision provenance
are implemented. Expansion is followed by a structural/type verifier; inspection
shows definition sites and call contexts, including internal completion mappings.
This verifier complements, rather than independently reproduces, source ownership
and coverage analysis.

Native lowering generates ordinary Rust functions, branches and loops, then uses
the installed LLVM-backed toolchain with an explicit verifier pass. The generated
programs contain a value/host runtime, not an interpreter for the path graph. This
bridge was chosen to establish executable semantics with a small auditable backend;
a direct LLVM emitter or more efficient value layout can replace it later without
changing source semantics. Generated Rust and LLVM IR are retained for inspection.

The playground is intended to test how the notation feels in everyday editing.
It includes zero-argument main regions, real arithmetic/parsing/text/file adapters,
and VS Code build/run commands. Network/time example primitives remain scripted;
missing live adapters are explicit host errors. A compact reference documents both
the implemented language and its sharp edges.

Performance work remains: values use tagged storage and source reuse can clone
non-linear data; runtime flags implement path facts conservatively. Proofs of
termination, general transformation correctness and foreign-resource discipline
are not claimed. The same open design questions about ordinary ownership versus
mandatory protocol completion, structural independence, empty owning variants and
local type annotations remain worth testing through actual programs.


## Compiler 0.6.0: language revision foundation

The conversation after v0.1 selected `control`, type-first declarations with `auto`,
brace payload patterns and snake_case throughout. This increment implements those
forms, `exits auto`, and pure `u32_text` / `concat` expressions, preserving the original
syntax as a compatibility path. The new [revision roadmap](language-revision.md)
records the larger ownership, collections, first-class outcomes and async design.

Contract inference uses checked callees in dependency order, not a parallel approximate
type checker. Inferred payload types must agree at every exit site. Bodies still undergo
coverage, value availability and resource checks. Local annotations supply expected
constructor types, so `option<u32> value = none;` is unambiguous. Pure manifest flags
are accepted only for implemented exact builtin contracts; one outcome alone is not
purity. New expressions participate in IR verification, expansion and both backends.

The mutation keyword remains unsettled. This release does not change string ownership,
introduce first-class borrows, or claim that coroutine destruction is cancellation.
The next semantic increment must separate Copy/Drop capabilities from mandatory
protocol completion before collections or tasks can safely own resources.


## Compiler 0.7.0: owned values and local references

25 October 2026. `control` now moves strings/bytes and aggregates containing them.
Copy eligibility is distinct from the manifest's must-complete resource flag.
`clone`, `drop`, provisional `mutable`, assignment and partial-field reinitialization
are checked per path family. Runtime moves remove slots or record fields; mutation
writes the original place. Scope storage is released, including expanded callee
storage after payload transfer. Ordinary destruction cannot close a fallible resource.

Shared and exclusive local references have stable named referents. They lower to
checked places, with path-sensitive loans. Shared parameters are read-only references
at source level and snapshots internally. This is intentionally limited to the
synchronous non-escaping subset: no pointer identity, mutable parameter ABI, returned
reference, reference aggregate or stored reborrow is claimed. Reference lifetimes
are scope-based with explicit release, not yet inferred to last use.

A discovered loop hazard is covered by a regression: consuming an outer non-Copy
capture on a restarting history would reuse a moved slot on the next iteration.
Such owners must enter through loop parameters; outer references cannot be moved or
explicitly released inside the repeating body. Locals and loans start fresh each
iteration. This constraint is conservative and can be relaxed by future backedge
analysis, but cannot be ignored when shortening lifetimes.

Further reference work must precede borrowed collection APIs. The next checkpoint
is a real reference representation with lifetime relationships at control boundaries;
read-only snapshot calls must not be extended to mutate caller data by accident.


## Compiler 0.8.0: reference boundaries and local lifetimes

25 October 2026. Shared and exclusive control parameters now carry real references
to caller storage. Returned borrows have checked summaries identifying input indices
and field paths for each completion payload. The checker substitutes these summaries
at call sites; alternatives with no reference result do not create a result loan.
A result with multiple possible inputs conservatively borrows each. Declaration
hover exposes those relationships without introducing unimplemented lifetime syntax.

Reborrows track parent loans and underlying origins separately from runtime access
places. This lets disjoint fields remain independent while freezing overlapping
parent access. Local last-use analysis shortens loans, including aliases and returned
borrows; loop captures retain loans across backedges. Explicit release is still
available. The analysis is conservative over future syntax, not a complete region
solver or a claim of full Rust equivalence.

Reference values contain internal slot/field addresses, never serializable driver
pointers. An acyclic sequential call graph permits shared invocation storage because
callee locals cannot escape; returned references flatten to their input owners before
callee storage is released. Expansion remaps access places and preserves those
addresses. Async/concurrent execution must introduce independent activation storage
before allowing overlapping invocations of the same control. No coroutine support
is claimed by this storage change.

Lifetime-bearing aggregates, rebinding, loop-carried references and explicit exported
lifetime contracts remain separate work. Existing manifests cannot introduce reference
fields or forge reference-returning foreign primitives. This keeps foreign contracts
outside the new source-level lifetime inference boundary until their API is defined.


## Compiler 0.9.0: collection ownership and element borrows

25 October 2026. Arrays/vectors/maps use builtin specialized contracts. Source
annotations supply literal element types; maps begin empty and insertion exposes
replacement. Bounds failure on replacement returns the rejected new value, avoiding
implicit loss of a must-complete payload. A resource collection is a sealed obligation;
consuming `take` exposes its first entry and rest, or proves it empty through an outcome.

Returned element references contain actual runtime storage indices, while checked
return summaries use a wildcard element projection. The latter conservatively freezes
the owner collection and is never a runtime address. Origin substitution composes
through nested collections and helper controls. This prevents insertion/removal from
invalidating live references without claiming disjointness for arbitrary indices.

Collection algorithms are shared between interpreter and generated native support.
Map storage is sorted flattened key/value pairs with bool/u32/string keys; lookup and
updates are currently linear. `take` from the front is linear, so full draining can be
quadratic. A queue/iterator representation is needed before promising efficient large
collection traversal. Allocation failure remains a host failure. Aggregate JSON input
checks arity, recursive element types, map uniqueness and sorted order before execution.

The source still uses brackets for both selected histories and list literals, but the
positions are unambiguous to the parser: a selected history prefixes an item and ends
in `:` or `over`; a list occupies an expression position. Editor highlighting now
separates ordinary single-line selectors from nested/multiline list literals. Multiline
selector coloring remains a lexical limitation of the TextMate grammar.

Slices, stored references, inferred collection literals and source-defined generic
library contracts remain open. Compiler-specialized builtin contracts are an interim
library boundary, not a claim that user-defined generics already exist.


## Compiler 0.10.0: inline adapters and owned completion values

25 October 2026. Initial statements acquire the enclosing entry selection. Existing
flat `[path]:` groups remain explicit and do not reset through omission. Exit payload
versus body parsing now recognizes a complete field list, because bodies may begin
with ordinary declarations or statements. This preserves explicit/legacy contracts.

Tail `exit delta` adapters reuse the decision checker. Each arm has its own lexical
scope, source span and fresh atom identity; named path labels are retained as display
names. Branch payloads transfer references/resources under the same existing rules.
The lowered graph uses ordinary decisions and exits, preserving provenance under
expansion without materializing an unnecessary outcome object.

`capture` uses an invocation decision followed by selected tag/payload construction
at that exact source position. Temporary payload slots and private capture paths do
not escape. Later consuming handling cannot replay effects or duplicate the result.
A private producer-indexed closed schema backs `outcome<producer>`; source and foreign
manifest constructors cannot forge these values. The IR verifier checks each generated
schema against its producer's checked contract and separately validates the foreign
manifest. Schema dependencies participate in acyclic checking order.

Completion obligations use the existing per-path must-complete tracking machinery,
but the type capability and diagnostics distinguish unhandled outcomes from their
resource payloads. Outcome Copy/Clone/Drop are prohibited regardless of payload type.
Deciding the outer result discharges handling and transfers any resource obligations;
containers preserve both. Nothing here makes every ordinary variant must-handle.

Stored borrowed results remain rejected until aggregate lifetimes can express their
owners. Immediate adapters support them without storage. General bound delta
expressions, shared named outcome contracts and structural cross-producer assignment
are still needed. Producer-indexed types are a concrete first boundary, not the final
public contract syntax. Async storage, cancellation and coroutine lowering are not
implemented by capture; it is eager and synchronous.


## Compiler 0.11.0: stored adapters and explicit target contracts

25 October 2026. `auto result = delta ...;` lowers through the same exhaustive
branch checker as tail adapters. Each leaf constructs its selected tag and moves
its fields into a common destination slot before its arm scope is released. The
slot's closed schema is resolved after all leaves are checked; unfinished lowering
is never exposed as a checked program. Nested arms contribute to the same result.
Branch-local resource obligations must be transferred, while unrelated outer
obligations remain available for subsequent statements. Effectful operands run once
at their original positions; storage introduces no deferred computation.

An annotation `outcome<producer>` checks explicit mappings into that producer's
contract, including safe widening to extra alternatives. Labels, arity and field
types must agree; this does not add implicit cross-producer conversion. Type
annotations now participate in dependency ordering even on local bindings, so a
later producer's inferred contract is available before checking the adapter.
Anonymous adapter schemas are compiler-private and separately verified for closed,
nonempty, distinct alternatives and valid owning payload types. Their handling
obligation is identical to a captured result's obligation.

An aesthetic limitation remains: spelling a reusable contract through a producer
couples type names to operations. A standalone named outcome declaration should
replace that coupling in a later checkpoint. Independently inferred adapter types
currently remain distinct even when their labels and payloads coincide. Explicit
target contracts provide a deliberate way to unify them without inferring semantic
interchangeability solely from shape. Stored references, statement-block arms and
adapter expressions in arbitrary value positions still require further work.


## Compiler 0.12.0: named outcome contracts

25 October 2026. Standalone `outcome message { ready { string text }, empty }`
declarations introduce nominal must-handle types. `exits message` shares the declared
closed contract; capture inherits that type. Inline contracts and producer-indexed
schemas remain compatible. A wrapped inline contract or explicit nullary payload
avoids the single-bare-name ambiguity when a nullary exit shares a contract's name.
This resolution is declaration-aware, including declarations later in the unit.

Named outcomes use an explicit outcome layout kind, separate from ordinary variants.
Copy/Clone/Drop remain forbidden regardless of payloads. Constructing a named outcome
is explicit and creates the same handling obligation as capture; it does not assert
that any effectful producer ran. Short constructors require an expected named type;
qualified constructors work with auto. Global short constructor/option lookup is
unchanged. Stored references and recursive layouts remain rejected.

Typed capture can target a named contract if every producer alternative has exactly
matching arity and payload types. A wider target is allowed, and consumers must cover
its full contract. Different named types have no implicit structural assignment;
adapters make deliberate renaming or conversion visible. Expansion still occurs at
actual invocation sites and retains their effects and provenance.

The full native suite exposed excessive incremental query-cache growth from many
independently named generated crates. Generated build profiles now disable that
incremental cache while retaining compiled dependency reuse. Disposable caches were
cleared; source and packaged artifacts were preserved.
