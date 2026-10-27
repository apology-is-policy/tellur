# Revision status

Compiler 0.41.0 retains the v0.1 profile below and adds revised syntax plus owned
values, mutation, borrowed calls/results, local lifetime inference, core collections, nested adapters and owned completed
outcomes. See [the revision roadmap](language-revision.md).
`tests/revision.rs` covers typed declarations, brace patterns, pure expressions,
inferred contracts and compatibility. `tests/values.rs` covers Copy/move/clone/drop,
path-sensitive assignments and loans, field disjointness, reference transfers,
resource restrictions and loop captures. Native differential scenarios include
mutation and references across calls, scopes, branches, loops and expansion at O0/O2.
LSP and actual extension-host tests check unsaved ownership errors, repairs, borrowed
parameter hover and native mutation. This inventory does not claim that the planned
slice/outcome/async revision is complete.

`tests/references.rs` additionally covers shared/exclusive call storage, returned
input and field borrows, multiple possible origins, reborrows, last use, backedges,
argument loan duration and rejected local escapes. Native cases run before/after
expansion at O0/O2. Editor tests cover inferred lifetime hover and borrowed returns.

`tests/collections.rs` covers typed/nested literals, Copy versus move, borrowed
lookup and helper-returned element origins, mutation invalidation, empty/bounds
outcomes, ordinary and resource replacement, vector/map draining, deterministic key
order, recursive layouts, aliasing, malformed driver aggregates and a runnable
playground. Native collection scenarios run original/expanded at O0/O2. LSP tests
check element-source hover and unsaved borrow conflicts; TextMate and actual editor
tests cover list highlighting, conflict/repair and native build/run.

`tests/adapters.rs` covers implicit entry and exit-contract parsing, selected-group
semantics, nested inference/exhaustiveness, lexical arm scopes, borrowed returns,
resource payload obligations, effect selection and distinct invocation provenance.
`tests/outcomes.rs` covers immediate capture, delayed handling/forwarding, mandatory
empty/Copy handling, moves, containers, forbidden cloning/discard, type dependencies,
constructor isolation, read/close double-failure retention, and independent captures.
Both include original/expanded native comparisons at O0/O2. LSP and real editor tests
check adapter contract hover, missing arms, unhandled results, repairs and effect order.

`tests/forward.rs` covers call statements, exact forwarding shapes, arm payload
retention, borrow origins, resource obligations, selected execution and native
original/expanded agreement at O0/O2. `tests/algebra.rs` checks boundary embedding
laws, identity and reassociation examples, effect counterexamples, distinct expanded
contributors, finite comparison reports, nested layout changes and CLI exit status.
LSP/TextMate/real editor tests cover forwarding, contract growth, repairs and execution.
See [semantic foundation](semantic-foundation.md) for the scope of these claims.

`tests/certificates.rs` covers independently validated expansion artifacts, serialized
round trips, nested loops and repeated invocations, staged expansion, borrowed
summaries, Boolean guard equivalence and adversarial graph/correspondence changes.
Every existing expanded native/interpreter test now passes certificate validation.
See [the certificate contract](expansion-certificates.md) for the narrow guarantee
and remaining soundness obligations.

`tests/protocol.rs` covers failed-read cleanup ordering, loop reset and unbounded
prefixes, completion versus silent cycles, nested callee-loop completion, captures,
repeated calls, collections and borrowed adapters. Inclusion tests cover nondeterminism,
cycles, shortest counterexamples, CLI status and model/pair budgets. Negative examples
show that abstract trace equality need not imply concrete behavior or divergence
equality. See [protocol analysis](protocol-analysis.md) and the
[conditional simulation argument](expansion-simulation.md).

`tests/extraction.rs` covers typed tail inputs, mutable-local restoration, fresh
names, named outcomes, borrowed return origins, resource transfer, copied paths and
whole loops, Unicode/CRLF text, candidate rejection and CLI nonmutation. The runnable
example is compared before/after extraction and expansion in the interpreter and
native O0/O2 builds. [Extraction](extraction.md) preserves checked public contracts;
this evidence is not a general source-transformation proof.

`tests/data_types.rs` covers source structs and ordinary variants, nominal wrappers,
forward type references, data-only units, Copy/move/clone rules, field mutation,
partial moves, borrowed results, resource/outcome propagation, invalid finite layouts,
and integration with extraction/protocol analysis. Native O0/O2 scenarios agree before
and after certified expansion. LSP and editor tests cover declaration hover/outline,
unsaved layout and nominal errors, repair and native execution. TextMate tests cover
declaration names, fields and variant payloads.

`tests/modules.rs` covers relative imports, qualified/public access, private field
reads/borrows/writes/construction, public-interface leaks, nominal identity across
aliases/diamonds/symlinks, distinct files, constructor scope, cycles, source budgets,
source locations, unsaved loaders, resource transfers, CLI working directories and
native O0/O2 agreement before/after expansion. LSP and real editor tests cover imported
unsaved diagnostics, related locations, repair and native builds. Highlighting includes
imports, namespace aliases, qualified types and public declarations.

# v0.1 conformance and roadmap completion

Compiler 0.5.0, 25 October 2026. Target: the original v0.1 draft together with the
explicit clarifications in `v0.1-addendum.md`. The original documents are preserved.
This is an implementation/evidence inventory, not a formal soundness proof.

`tests/stored_adapters.rs` covers inferred and explicitly targeted stored adapters,
nested selected effects, lexical release, resource/handling obligations, borrowed
inputs cloned to owned results, exact payload agreement, loop reinitialization and
nested stored outcomes. Native tests compare original/expanded forms at O0/O2.
LSP and actual VS Code checks exercise inferred contracts, unsaved handling/move
errors, repair and native execution.

`tests/named_outcomes.rs` covers shared nominal contracts, constructors, adapters,
explicit capture widening, exact type distinctions, mandatory handling, nested
containers, finite-layout validation, explicit inline-contract compatibility and
native original/expanded execution at O0/O2. Editor checks cover declaration hover,
outline entries, payload mismatches, handling errors and native execution.

## Roadmap outcomes

| Milestone | Delivered | Evidence |
| --- | --- | --- |
| Language/spec gaps | Qualified variant arms; aggregate ownership and Option inference rules; explicit supported pure core and temporary restrictions | `tests/completion.rs`, `tests/ownership.rs`, `tests/variants.rs`, addendum |
| Provenance and inspection | Definition region/site/location, distinct call IDs and readable contexts, internal-exit → caller-outcome mappings; inspect/expand CLI | completion provenance test; `src/verify.rs` |
| Reference conformance | All C01–C24 tests; expanded ownership, loop and source mutation coverage | conformance, loops, completion, ownership and variants suites |
| Executable lowering | Regions → functions, masks → branches, values → typed runtime variants, loops/restarts → native loops; explicit outcome dispatch | generated `.tellur-build/src/main.rs`; native differential tests |
| LLVM and native execution | Rust bridge into LLVM, explicit verifier pass, O0/O2 executables, live and scripted primitive harness | `.ll` artifacts; native and native_host suites |
| Usable environment | Packaged compiler/server; VS Code build/run, task, diagnostics; small reference and playground | extension-host tests including real builds, spaces/argument boundaries and execution |

## Requirement coverage

| Draft sections | Status / implementation |
| --- | --- |
| §1–3 guarantees, sequential core and semantic identities | Structural checker, ordered IR, finite symbolic BDD families; acyclic calls; future facilities rejected |
| §4 syntax | ASCII identifiers, UTF-8 literals, comments, delta/Δ, semicolons, control grammar and precedence; lexer/parser mutation tests |
| §5 causal selectors | Source-order resolution; prior skipped producer is false; unavailable payloads rejected; observations execute once |
| §6 decisions | Exact closed outcome arms, payload arity/types, fresh atoms, named sites; qualified variant names resolved |
| §7 aliases/scopes | Forward/cyclic aliases rejected, accumulated intersections, effective aliases, lexical scope; expansion renames identities |
| §8 and §20 values | All core types, literal checks, specified operators and short-circuit execution; disjoint immutable definitions; records/variants/options; addendum clarifies inference and owning temporaries |
| §9 disposition | No implicit fallthrough, overlapping later work rejected, impossible selectors rejected, structural witness diagnostics |
| §10 boundaries | Public outcome edits break mappings; handled internal outcomes preserve callers; internal completion sites retained |
| §11 loops | Tail placement, typed simultaneous arguments, fresh locals, outer facts, finite checking, exit/restart coverage; source nesting and historical queries rejected |
| §12 ownership | Path-sensitive moves, primitive loans, explicit consumption/transfer, aggregate obligations; adapters validate supported contracts |
| §14 checked representation | Resolved IDs/typed slots, structured loop IR, symbolic families, provenance; structural/type verifier at transformation/backend boundaries |
| §15 expansion | Acyclic call substitution, caller continuations, fresh identities, payload/source/effect order, structured cycles; finite differential evidence and loop fuel checks |
| §16 executable experiment | Generated Rust is the conventional executable representation; LLVM creates native code, runs verify pass and retains all outcome branches; no unchecked unreachable assumptions |
| §17 conformance | C01–C24 below plus bounded generated/mutated programs |
| §18 revisions | Addendum records implementation decisions; future research remains separate |
| §19 manifest | Version/types/primitives/modes/outcomes validated; cyclic types and invalid contracts rejected; host/scenario arguments validated |

## Named conformance cases

| Cases | Tests and result |
| --- | --- |
| C01–C06 | `tests/conformance.rs`: port cases, missing arms/dispositions, payload availability, overlapping definitions, future references |
| C07–C11 | conformance: both failures preserved, missing combination witness, leaks, use after consumption, later selected work |
| C12–C14 | conformance: nested factoring equivalence, all two-loader combinations, continuation after callee failure |
| C15–C16 | conformance: changed public contracts and handled internal failures; completion suite additionally checks multiple internal contributors and named call provenance |
| C17–C20 | `tests/loops.rs`: retry/cancellation, omitted restart, historical-reference rejection |
| C21 | conformance generated acyclic chains and loader matrices; native suite combines generated branches/scopes/joins/loops with calls/resources and compares all executions |
| C22–C24 | conformance: union runs once, contradictory selector rejection, skipped producer false |

The native differential suite compares the interpreter and compiled binaries for
original and expanded programs, at optimization levels 0 and 2. Scenarios include
port outcomes, loader outcome combinations and repeated calls, retry/cancellation,
record partial moves, owning variants, nested options, scalar expressions, loop
restarts and completion from inlined loops. Bounded generated programs exercise
path-dependent joins, scopes and cycles. Fuel tests reject a host budget exhaustion
without claiming termination. Generated LLVM is checked by its verifier pass.

Malformed-input coverage includes 1,500 bounded token sequences and systematic
mutations of a valid program, in addition to explicit negative conformance tests.
Live-adapter tests check parsing/arithmetic boundaries, Unicode, file reads/closes,
missing files, malformed bytes, adapter signature mismatches and forged handles.

## Limits of the claim

- This completes the implemented v0.1 profile, including its written addendum;
  conservative temporary/inference/loop-capture rules are explicit revisions.
- The native backend uses a Rust intermediate and its LLVM toolchain. There is no
  direct LLVM binding in tellurc. Values use tagged Rust storage; layout optimization
  and efficient elimination of runtime path flags remain performance work.
- Testing is finite. No proof of general semantic preservation, nontermination,
  foreign effects, or resource identity discipline is claimed. The IR verifier
  checks structural/types invariants, not a second independent ownership analysis.
- Live foreign adapters are a finite library. Arbitrary manifest operations work
  through scripts; new live implementations require an adapter, not only a declaration.
- Tested native platform: Apple Silicon macOS, Rust 1.97.1 / LLVM 22.1.6. Other targets
  are unvalidated. Rust/Cargo is needed to build; generated executables are standalone.
- The editor reports one error per compilation unit. Source-level debugging,
  completion/rename and graphical projections are later work.

`tests/named_fields.rs` covers named struct construction, declaration-independent
initializer order, clone-before-move, nested inferred literals, field diagnostics,
nominality, completion obligations, loan liveness and aliasing. Expansion certificate
mutation tests reject reordered/duplicate/out-of-range field indices. Native O0/O2
results agree before and after expansion. Module tests check qualified construction
and private representations; LSP/editor tests check unsaved field/ownership errors
and repair; grammar tests distinguish initializer labels.

`tests/generic_data.rs` covers explicit generic structs/variants/outcomes, nested
specializations, canonical argument identity, inherited ownership/completion,
unused-template validation, recursion/arity diagnostics, lexical parameter shadowing,
collection restrictions, specialization budgets and native O0/O2 agreement before/after
certified expansion. Module tests retain nominal identity and private representations;
LSP/editor tests cover unused generic declaration hover, unsaved errors and repair.
Generic control bodies are covered below; declared trait capabilities remain a later checkpoint.

`tests/generic_controls.rs` covers definition checks independent of concrete uses,
unused invalid bodies, distinct type parameters, resource/outcome transfer, inferred
parametric exits, nested calls/adapters, borrowed return origins, mutable references,
producer-indexed capture, source-extraction boundaries, specialization budgets and
native O0/O2 agreement before/after certified expansion. Module tests check private
representations and imported generic diagnostics. LSP/editor tests expose checked
generic signatures and unsaved definition errors. Unconstrained operations are borrowing and ownership transfer.


## Sealed generic capability bounds (0.22)

`tests/capabilities.rs` checks copy/clone/drop implication, precise abstract permissions,
phantom arguments, derived aggregate/collection permissions, outcome/resource obligations,
legacy-syntax rejection, nominal identity and interpreter/native/expanded agreement at
optimization levels 0 and 2. Module tests exercise imported bounds and private concrete
arguments. LSP and VS Code tests cover bound hover/outline, unsaved definition and call
errors, repair and native execution. The grammar distinguishes capabilities from type
parameter names. These permissions cannot be supplied through foreign manifest JSON.


## Concrete traits and associated types (0.23)

`tests/traits.rs` covers complete conformance, exact owning/shared/exclusive parameter
modes, associated normalization/cycles, lexical generic shadowing, capture, generic
helpers, explicit lifetime limits, resource/outcome obligations, privacy and native
execution before/after certified expansion at O0/O2. Module tests cover trait/type
ownership, foreign-field access, qualified imported projections and diamond identity.
LSP/VS Code checks cover trait and implementation Outline members, signature hover,
unsaved conformance and associated-type errors, repair and native execution.


## Trait-constrained generics (0.24)

`tests/trait_bounds.rs` checks abstract trait operations, exact associated permissions,
combined sealed/trait requirements, phantom arguments, generic-to-generic implication,
distinct projections, captures, borrowed inputs/results of generic controls, mutable
calls, resource/outcome obligations, explicit budgets and native/expanded equivalence.
Regression tests prevent early specialization through shadowed names and internal
abstract identifiers in diagnostics. Module and editor tests cover imported bounds,
public interfaces, readable qualified hover, unsaved errors, repair and native runs.


## Declared borrow origins (0.25)

`tests/lifetime_contracts.rs` checks origin names and permissions, wrong-input returns,
local escape, renamed implementation parameters, generic abstract witnesses, opaque
associated pointees, nested projection/forwarding, shared/exclusive loans and last-use
release. Native O0/O2 runs match the interpreter before and after certified expansion.
Module tests return private fields through public trait contracts without exposing
layout. LSP and VS Code checks cover origin hover, unsaved errors, repair and native
execution. Exact inferred field paths retain the previous disjointness tests.


## Reference vocabulary and transparent field access (0.26)

`tests/reference_words.rs` covers generic trait calls, nested record projections,
shared/exclusive permissions, prefix precedence and assignment, last-use release,
exclusive moves, reference-rebinding diagnostics, and unchanged path/boolean
operators. New spellings and compatibility spellings agree with the interpreter;
native O0/O2 and certified expansion preserve results. Module tests reject private
field access through the shorthand. Current playground files, hover, snippets,
grammar and real VS Code diagnostics/build-run exercise the new vocabulary.

## Local stored references (0.27)

C1b.1 supports nested local struct reference fields, inferred per-value origins,
shared copy/clone and exclusive transfer, whole/partial record moves, direct field
reference use, disposal, record liveness, generic layouts and module privacy.
`tests/stored_records.rs` checks positive and rejected ownership cases and native
O0/O2 execution before/after certified expansion. Module and LSP tests cover private
representations and unsaved diagnostic repair; the VS Code integration suite builds
and runs `playground/stored_references.tellur` and checks move/loan errors.

Remaining C1b/C1c boundaries (whole borrowed aggregates through controls, variants,
outcomes, collections, rebindings and general reference projections) are diagnosed.
This checkpoint does not claim the full stored-reference roadmap item is complete.

Release verification: 450 Rust tests, 25 TextMate grammar tests, strict all-target
Clippy, 29 playground sources and the actual VS Code integration suite pass. Native
stored-record tests compare interpreter results at O0/O2 with original and certified
expanded graphs. The 0.27 VSIX payload and executable permissions were verified.

## Selected payload inference (0.28)

`exits ready..., missing` preserves a closed label set and infers only the marked
nonempty payloads. Empty, unresolved, inconsistent or undeclared exits are errors.
Explicit payloads, nullary outcomes and named contracts retain their semantics.
Generic bodies, imported controls, borrowed origins, captures, nested adapters and
forwarding resolve to ordinary checked contracts. No new runtime/IR form is added.

`tests/payload_inference.rs` covers positive and rejected forms and interpreter/native
O0/O2 agreement before and after certified expansion. LSP/module tests cover checked
hover, unresolved outline, unsaved repair and imported inferred contracts. The actual
VS Code integration suite builds and runs `playground/inferred_payloads.tellur`.

Release verification: 459 Rust tests, 26 TextMate grammar tests, strict all-target
Clippy, 30 playground sources and the actual VS Code integration suite pass.
The 0.28 VSIX payload, executable permissions and installed extension files were
verified. Native payload-inference tests agree with the interpreter at O0/O2
before and after certified expansion.

## Grouped record results (0.29)

C1b.2a implements structured public origin groups on structs, nested/generic schema
composition, explicit/inferred record results, forwarding, field-sensitive last use,
private group mappings and per-leaf checked return metadata. Shared copies retain
their dependencies; exclusive fields transfer permissions. Explicit opaque anchors
do not assert disjointness. Whole-record inputs, contained-origin selectors, stored
sums/collections and borrowed-record trait results remain diagnosed and queued.

`tests/origin_records.rs` covers schema/boundary errors, independent same-type owners,
shared groups, generic definition checks, local escapes, copied/moved fields, repeated
calls, expansion-certificate tampering and interpreter/native O0/O2 agreement. Existing
stored-record tests cover local ownership regressions. Module/LSP/editor tests cover
private field renames, unsaved diagnostics and native build/run.

Release verification: 475 distinct Rust tests pass (474 in the full suite, then
the final 36-test LSP target including one added hover regression). All 27 TextMate
grammar tests, strict all-target Clippy, 31 playground sources and the final actual
VS Code integration suite pass. Native tests compare shared and exclusive grouped
results at O0/O2 before/after certified expansion. The packaged and installed 0.29
extension payloads and binary executable modes were verified.

## Reference binding inference fix (0.29.1)

`ref auto` and `ref mutable auto` infer the referent in local and decision payload
bindings, retaining exact shared/exclusive mode checking. Regression tests cover
reference moves, live loans, immutable owners, stored fields, returned payloads and
unsaved editor repair. Parameter and stored-field type declarations remain explicit.

Patch validation: 87 focused Rust tests, strict all-target Clippy and the actual
VS Code integration suite pass, including reference-auto diagnostics, repair and
native execution. The packaged and installed 0.29.1 payloads were verified.
Origin-group regression tests now use a dedicated fixture, leaving playground
examples free for user experimentation.


## Whole-record inputs and contained origins (0.30.0)

C1b.2b.1 adds owned grouped record parameters, including records containing exclusive
references, and shared/exclusive carrier borrows when all contained references are
shared. `input['group]` selects a public contained origin in direct `from` contracts
and grouped result bindings. Generic/nested schemas, private field mappings, inferred
contracts, immediate adapters and forwarding retain those dependencies.

Carrier storage and external referents are distinct: a shared contained reference
can outlive its carrier, while a reference into an owned carrier field keeps that
carrier borrowed. Owned parameter storage cannot escape. Calls account for contained
loans across argument evaluation; exclusive records move, while Copy records copy.
Live carrier aliases retain their contained loans; released aliases cannot be used
through direct or nested field access. Copying out an external reference removes
that access dependency without releasing the external owner.

`tests/record_inputs.rs` covers these boundaries, shared-group unions, clones,
exclusive forwarding, alias conflicts, local escapes, generic definition checking,
certificate tampering, carrier disposal and O0/O2 native/interpreter agreement before
and after expansion. Module tests check public selectors across private field renames.
LSP and actual VS Code tests exercise selector hover, unsaved wrong/unknown-group
errors, repair, and native execution. `playground/record_inputs.tellur` is the runnable
example; regression tests use a dedicated fixture to preserve user experimentation.

Verification: all 496 tests in the full Rust suite pass, plus the final added
shared-group/clone regression (497 distinct tests). Strict all-target Clippy,
28 TextMate grammar tests, all 32 playground sources, and the actual VS Code
integration suite pass. The installed 0.30.0 extension's binaries, client, grammar,
manifest and executable modes match the packaged release (excluding VS Code's
installation metadata).

C1b.2b.2 remains next: borrowing a carrier that contains exclusive references,
contained permission reborrowing and opaque disjoint splits. Whole-carrier reference
results, borrowed-record trait contracts, stored borrowed sums/collections, rebinding
and loop-state propagation also remain gated. These are explicit boundaries, not
claims of completed aggregate lifetime support.

## 0.31 — contained exclusive reborrowing

C1b.2b.2 permits shared and exclusive carrier borrows of records containing
exclusive references. Shared carrier access permits reading and shared reborrowing;
exclusive carrier access may grant exclusive children. Moving an exclusive field
out of a borrowed carrier is still rejected.

Returned child references retain both their external origins and the carrier
storage authorizing the reborrow. Separate checked storage guards survive copying,
moves, forwarding, generic calls and returned records. Each child's last use
restores its parent field independently. Calls check contained permissions as well
as carrier storage, including argument evaluation order. The legacy `region`
spelling cannot duplicate exclusive carrier permissions.

`tests/carrier_reborrows.rs` covers these rules, nested/generic carriers, collections,
local escapes, inferred contracts, expansion-certificate tampering and native O0/O2
agreement before and after expansion. Module tests preserve public origin contracts
across private field renames. LSP and VS Code tests check authorization hover,
shared-carrier permission errors, live-child storage protection and repair.
`playground/carrier_reborrows.tellur` demonstrates independent parent restoration.

All 511 tests in the full Rust suite pass, along with formatting, strict all-target
Clippy, 28 TextMate grammar tests and all 33 playground sources.
The actual VS Code integration suite also passes, including native execution of
the new example. The installed 0.31.0 extension's binaries, client, grammar,
manifest and executable modes match the packaged release (excluding VS Code's
installation metadata).

C1b.2b.3 is next: verified disjoint result permissions from one opaque ordinary
owner. Combined groups containing several exclusive fields remain conservative.
Whole-carrier reference results, borrowed-record trait contracts, stored borrowed
sums/collections, rebinding and loop-state propagation remain gated.

## 0.32 — checked opaque owner splits

C1b.2b.3 separates access permissions from lifetime anchors. Multiple exclusive
result references may name one opaque input; result construction must establish
their separation on every exit. This also applies to grouped records, including
multiple exclusive fields in one group, trait implementations and symbolic generic
calls. Shared results may alias other shared results, but cannot grant mutable access.

The checker instantiates fresh result permissions at calls and preserves their
ancestry through forwarding, moves, shared copies, local projections and reborrows.
Owner anchors remain conservative: a released child does not reveal its callee's
private field, and other live children continue to protect the owner. Carrier guards
retain their separate storage dependencies. No runtime permission representation,
surface assertion or serialized disjointness claim is added.

`tests/opaque_splits.rs` exercises valid and invalid splits, mixed shared/exclusive
results, opaque owner protection, independent invocations, path-dependent swaps,
local projection splits, grouped carriers, forwarding and abstract trait calls.
Native O0/O2 execution is compared with the interpreter before and after expansion.
Module tests cover private field renaming and invalid implementations; LSP and actual
VS Code tests exercise permission hover, unsaved alias/owner errors, repair and native
execution. `playground/opaque_splits.tellur` is the runnable example.

Verification: all 523 tests in the full Rust suite pass, along with formatting,
strict all-target Clippy, 28 TextMate grammar tests, all 34 playground sources and
the actual VS Code integration suite. The installed 0.32.0 extension's binaries,
client, grammar, manifest and executable modes match the packaged release
(excluding VS Code's installation metadata).

Next is C1b.3: stored borrowed sums and collections. Whole-carrier reference results,
borrowed-record trait contracts, reference rebinding and loop-state propagation still
have explicit implementation gates. This checkpoint does not claim a mechanized
soundness proof or complete aggregate lifetime support.

## 0.33 — local borrowed variants and outcomes

C1b.3a attaches guarded loans to the constructor, payload and record-field path of
a locally stored sum. Shared variants can copy/clone when all fields permit it;
exclusive payloads move. Optional references follow the same rules. Captures and
stored adapters retain reference and record payloads without losing outcome-handling
obligations. Unpacking transfers the selected payload's loans and preserves owner
origins, carrier guards and exclusive permission ancestry. A surviving shared copy
remains a retainer. Only constructed payloads add loans, while all structural delta
arms remain required.

Adapter scope checks reject references into storage owned by completed arms.
Direct borrowed exits can be reconstructed from a captured outcome; whole borrowed
sum call boundaries remain gated. Nested borrowed sums, borrowed collection elements,
whole-sum borrowing, assignment and reference-bearing loop state remain gated too.
Ordinary owned nesting and loop-local borrowed sums retain their existing rules.

`tests/borrowed_sums.rs` covers copies, moves, aliases, optional inference, capture,
adapters, record payloads, carrier authorization, per-constructor release, generic
local use, scope escape, forwarding, loop recreation and explicit remaining gates.
The runnable `playground/borrowed_outcomes.tellur` is compared between interpreter,
native O0/O2 and expanded execution. Module tests preserve visibility; LSP and actual
VS Code tests check reference-payload hover, owner/handling errors, repair and native
execution. The runtime representation and expansion rules do not change.

Verification: 540 distinct Rust tests pass. The full run passed 538 tests; two old
feature-gate assertions were updated for newly supported declarations/captures and
passed their targeted reruns. Formatting, strict all-target Clippy, 28 TextMate
grammar tests, all 35 playground sources and the actual VS Code integration suite
pass. The installed 0.33.0 extension's binaries, client, grammar, manifest and
executable modes match the packaged release (excluding installation metadata).

Next are C1b.3b (public borrowed-sum origins and nested storage) and C1b.3c (borrowed
collection elements). This release does not claim completion of C1b.3 as a whole.

## Inferred-type hover (0.33.1)

Hovering `auto` displays the compiler's inferred binding type. Local values,
shared/exclusive references (including erased aliases), delta payloads, captures,
stored adapters, collections, generic bodies and implementation method locals are
covered. `exits auto` displays the checked signature. Editor-only inference facts
retain source positions independently of executable slots; generic definitions use
the universal check, not whichever concrete call happens to be compiled first.
Anonymous adapter payloads are retained even for uninstantiated generic controls.
Executable IR, certificates, runtime behavior and ownership rules are unchanged.

Eight new LSP regressions check these cases, separate lexical bindings, imported
names and unsaved dependencies, Unicode ranges, comments/strings, type changes,
error invalidation and repair. Invalid compilation units return no inferred hover.
The actual VS Code test verifies hover contents/ranges, unsaved updates and repair.

Verification: all 548 Rust tests pass in the full offline run, including all 49
LSP tests. Formatting, strict all-target Clippy, 28 grammar tests, the extension
build and the full actual VS Code integration suite pass. Installed 0.33.1
binaries, client, grammar and manifest match the tested package; both server
binaries retain executable permissions. Logs are in `work/path-0.33.1-*.log`.

## 0.34 — nested local borrowed storage

C1b.3b is split into nested local storage (C1b.3b.1, delivered here) and public
borrowed-sum contracts (C1b.3b.2, next). Each stored loan now retains the complete
chain of constructor, payload and intervening record-field positions. Unpacking
rebases one storage holder and preserves deeper tags until their own decision.
No runtime layout, permission assertion or expansion-certificate format changes.

Borrowed variants, options, records and outcomes may nest locally. Shared copies
retain independent loans; exclusive contents move. Field disposal and last use
release only the relevant references. Outcome-handling obligations survive nesting.
Stored adapters reject borrows of completed arm storage, and nested reborrows keep
carrier authorization and exclusive ancestry. Public borrowed-sum inputs/results,
whole-sum borrows, borrowed collection elements, rebinding and reference-bearing
loop state remain explicitly gated.

`tests/borrowed_sums.rs` covers three-level options, sibling sums with identical
tags, independent exclusive fields, retained shared copies, branch-specific origins,
arm escape, carrier guards, generic storage, mandatory outcomes and local loops.
The nested fixture executes identically in the interpreter and native O0/O2, before
and after expansion, with both populated and empty inner outcomes. Module tests
check imported generic layouts, loan protection and private fields. LSP tests cover
payload inference, unsaved errors and repairs, including readable nested anonymous
outcome types in universal generic checks. Hover rendering snapshots anonymous
structure before symbolic layouts disappear and bounds recursive display expansion.

The runnable example is `playground/nested_borrows.tellur`; the actual VS Code test
uses its independent fixture copy for hover, owner/field errors, obligations,
repair and native execution. The roadmap and reference distinguish the delivered
local storage checkpoint from the remaining public contract work.

Verification: the full offline regression run passes 563 Rust tests, including
51 LSP tests. A final focused LSP rerun and strict all-target Clippy pass after
bounding recursive hover rendering. Formatting, 28 TextMate grammar tests, the
extension build and the full actual VS Code integration suite pass. Native nested
fixtures match at O0/O2 before/after expansion. Logs: `work/path-0.34-*.log`.

The final release LSP also passes nested generic inference over its stdio protocol.
Installed 0.34.0 binaries, client, grammar and manifest match the package; executable
permissions are preserved. Source snapshot: `work/revision-backups/path-0.34-source.tar.gz`.


## Detailed language manual (0.34 documentation checkpoint)

The [language reference](language-reference.md) is now organized as a current
semantic manual with a reading guide and contents, plus chapters for lexical rules,
core grammar, diagnostic examples, limits, terminology and feature status. It
separates runnable syntax from roadmap proposals and distinguishes contract fields,
caller bindings, stored outcomes, traits and temporal protocol models. Stale limits
on local borrowed storage and generic record boundaries have been corrected. A
complete carrier example's `read` helper was renamed to avoid the playground
manifest's primitive-name collision. No compiler/runtime behavior changed.

`tests/reference_manual.rs` checks all Tellur fences in the main manual and companion
chapters. It compiles 39 accepted examples, verifies eight expected diagnostic codes,
and explicitly identifies 14 context fragments excluded from standalone compilation.
It also checks 110 local links/heading anchors and the compiler-version marker.
The [maintenance checklist](reference/maintenance.md) and roadmap release discipline
require these checks and semantic/status updates at each future checkpoint.

Verification: all three documentation tests pass, `cargo fmt --all -- --check`
passes, and strict Clippy for `reference_manual` passes. These are static documentation
checks; the existing 0.34 native/editor/runtime release evidence above is unchanged
and was not rerun for this documentation-only checkpoint. Documentation test log:
`work/path-0.34-reference-tests.log`. Snapshot of the documentation changes and new
test: `work/revision-backups/path-0.34-reference.tar.gz`, supplementing the existing
0.34 source snapshot. Public borrowed-sum contracts remain the next compiler checkpoint.


## 0.35 — public grouped variant boundaries

Source variants now declare ordered origin groups, using the record vocabulary:
`variant view['a] { found { ref string text <- 'a }, empty }`. Owned variants and
nested grouped records/variants cross control inputs and result payloads. Explicit
result bindings, inferred contracts, contained selectors, generic specialization,
forwarding, capture and adapters preserve constructor-guarded reference dependencies.
Separate groups retain independent last uses; shared copies and nested completion
obligations survive transfers. Exclusive child reborrows retain authorizing carriers.

Checked result and contained-source metadata now includes each enclosing alternative,
payload position and intervening field path. Incoming tagged origins are abstract
until delta materializes the active payload; empty alternatives never cause an
unconditional field read. The existing runtime value layout is unchanged. Independent
IR validation enumerates and checks tagged leaves and rejects missing, duplicate or
malformed result/source paths. Certified expansion preserves the metadata and code.

Whole-sum borrows, named-outcome/builtin-option schemas, aggregate borrowed trait
results, collection elements and rebinding remain gated. Calls returning contained
variant borrows require locally bound arguments. An unproduced borrowed alternative
requires explicit result bindings when inference lacks reference-leaf evidence.
These limits are explicit in the updated manual and split roadmap checkpoints.

`tests/public_sums.rs` covers positive/negative origin maps, absent alternatives,
exclusive results, owned inputs and contained selectors, nested layouts, generic
capture/forwarding, surviving shared copies, nested outcome obligations, local/arm
escapes and carrier guards. Native cases exercise populated/empty results at O0/O2
before and after certified expansion. Module tests check imports, generic schemas,
owner retention and private-type leakage. LSP and TextMate checks cover schemas,
payload mappings, inferred bindings and unsaved owner errors/repair. The independent
IR verifier has targeted tagged-metadata corruption cases.

The runnable example is [public_variants.tellur](../playground/public_variants.tellur).
Editor/native tests use its independent fixture copy, preserving user playground
edits. The [detailed reference](language-reference.md#grouped-variant-contracts),
grammar, lexical guide, status matrix and diagnostic examples describe this release.


Verification: the full offline regression run passes 580 Rust tests, including
52 LSP tests and the eight native public-variant combinations (populated/empty,
original/expanded, O0/O2). A subsequently added carrier-retention regression also
passes, bringing the exercised suite to 581 tests. Strict all-target Clippy,
formatting, 29 TextMate tests and the full actual VS Code integration suite pass.
The manual checks pass 41 accepted examples, nine expected diagnostics, 14 explicitly
classified context fragments and 113 local links. No fragment is counted as compiled.
Logs: `work/path-0.35-tests.log`, `path-0.35-focused.log`, `path-0.35-clippy.log`,
`path-0.35-grammar.log`, `path-0.35-reference.log`, `path-0.35-editor.log`,
`path-0.35-release.log` and `path-0.35-package.log`.

Release 0.35.0 is built and installed. The Apple Silicon VSIX target, packaged and
installed binaries, client, grammar, snippets and executable permissions are verified;
installed package metadata matches apart from VS Code's own installer metadata.
The release compiler checks the new playground example. Source snapshot:
`work/revision-backups/path-0.35-source.tar.gz`.

## 0.36 — Tellur naming and compatibility

Tellur replaces the Path project-name placeholder. The Rust crate is `tellur`,
the canonical tools are `tellurc` and `tellur-lsp`, and new source files use
`.tellur`. The compiler accepts both source extensions, including mixed imports.
The semantic `path` keyword and path algebra retain their spelling and meaning.
No type, borrowing, origin or execution semantics changed in this checkpoint.

The VS Code package is `tellur-language-local.tellur-language`. It contributes
Tellur highlighting, snippets, diagnostics, hover and build/run commands for both
extensions. Legacy command IDs, explicit `path` language associations, and build/
server settings remain supported; explicit canonical settings take precedence.
The client trace setting uses the new namespace. Native output directories use
`.tellur-build` / `.tellur-cache`; `TELLUR_NATIVE_TARGET_DIR` takes precedence over
the legacy `PATH_NATIVE_TARGET_DIR` fallback. Both old binary names remain wrappers
for the same compiler and server. The old extension is replaced to avoid duplicate
servers, and the new package's installed files and executable modes are verified.

The modern reference, canonical examples, workspace and regression fixtures use
Tellur names. Existing `.path` playground copies, including user edits, remain
independent preserved files. The original specification/vision and historical v0.1
reference remain unchanged. The physical checkout directory stays in place.
See [migration details](language-reference.md#tellur-name-and-migration).

`tests/branding.rs` checks canonical/legacy commands, versions and mixed-extension
imports. Editor checks cover both extensions, old command/language IDs and the
new default output filename. Configuration tests cover legacy fallback, explicit
empty/false overrides and scope precedence. Native smoke checks exercise both
cache environment spellings and canonical-over-legacy precedence. All 45 canonical
playground/example sources check successfully. Existing suites now read the
canonical fixture copies, with dedicated compatibility coverage retained.

Verification: all **582 Rust tests** pass with canonical fixtures, including native
original/expanded and optimization comparisons. Strict all-target Clippy, formatting,
31 TextMate/configuration tests, and the actual VS Code integration suite pass.
The manual checks compile 41 accepted examples, verify nine expected diagnostics,
classify 14 context fragments and resolve 113 local links. Packaged and installed
compiler/server binaries, client, grammar, snippets, metadata and executable modes
match the source release. The package is explicitly targeted at `darwin-arm64`.

Logs: `work/tellur-0.36-tests.log`, `tellur-0.36-clippy.log`,
`tellur-0.36-grammar.log`, `tellur-0.36-reference.log`, `tellur-0.36-editor.log`,
`tellur-0.36-release.log` and `tellur-0.36-package.log`. Source snapshot:
`work/revision-backups/tellur-0.36-source.tar.gz`. Named-outcome and option origin
schemas remain the next language checkpoint; they are not claimed by this rename.

## 0.37 — named-outcome and option origin schemas

Named outcomes now declare ordered groups with the same payload mappings as source
variants. A control can bind those groups directly with `exits reply[input]`, which
applies the dependency to the selected exit without adding a wrapper payload.
Whole owned named outcomes also cross parameter/result boundaries, retaining their
mandatory handling obligations through capture, forwarding and nested storage.

Builtin options inherit their payload type's ordered groups; direct shared and
exclusive reference payloads use the single group `'source`. Nested option layers
retain those names, while their reference-leaf metadata records each `Some` tag.
Absent alternatives never materialize a reference address. Contained selectors,
explicit/inferred result dependencies and independent last uses compose with
records, variants, named outcomes, imports and generic specialization. Ungrouped
reference-bearing payloads do not acquire a complete schema merely by being wrapped
in an option. An unproduced borrowed leaf still needs explicit return bindings.

The schema lookup derives builtin option schemas on demand, including for inferred
result types. Source outcome schemas reuse the same finite tagged-leaf builder as
variants. Completion bindings are lowered into the existing direct/aggregate return
contracts. The runtime layout and IR metadata representation remain unchanged;
independent verification now has malformed-contract cases for variants, named
outcomes and builtin options. Expansion preserves those checked dependencies.

Public interface validation also checks the nominal completion contract named by a
public control, closing a gap where a private outcome name could leak through an
otherwise public payload layout. Ordinary, grouped and generic contracts have
negative and positive visibility regressions. Whole-sum borrows, anonymous outcome
schemas, aggregate/grouped trait contracts, reference-bearing collection elements,
reference rebinding and borrowed loop state remain outside this checkpoint.

`tests/outcome_origins.rs` covers capture, completion bindings, owned transfers,
shared copies, exclusive leaves, nested optional schemas, empty inputs/results,
inference failures, owner escapes, generic composition and mandatory handling.
Module/LSP/grammar/editor checks cover imported schemas, qualified contracts, public
visibility, schematic hover, inferred types, unsaved owner diagnostics and repair.
The native fixture runs populated/empty results at O0/O2 before/after expansion.
Try [outcome_origins.tellur](../playground/outcome_origins.tellur); its separate test
fixture preserves playground edits. The reference, grammar, diagnostics, glossary,
feature matrix and roadmap describe the delivered boundary and remaining gates.

Verification: all **596 Rust tests** pass, including native original/expanded and
optimization comparisons. Follow-up checks after the public-contract visibility fix
also pass: 562 semantic tests and 87 targeted module/generic/outcome tests, including
native cases (these overlap the full suite). Strict all-target Clippy, formatting,
32 TextMate/configuration tests and the actual VS Code integration suite pass.
The manual checks compile 44 accepted examples, verify ten expected diagnostics,
classify 14 context fragments and resolve 117 local links. All 46 canonical
playground/example sources check with the release compiler. Packaged and installed
compiler/server binaries, client, grammar, snippets, metadata and executable modes
match the source release. The package targets `darwin-arm64`.

Logs: `work/tellur-0.37-tests.log`, `tellur-0.37-boundaries.log`,
`tellur-0.37-final-semantics.log`, `tellur-0.37-clippy.log`,
`tellur-0.37-grammar.log`, `tellur-0.37-reference.log`, `tellur-0.37-editor.log`,
`tellur-0.37-release.log` and `tellur-0.37-package.log`. Source snapshot:
`work/revision-backups/tellur-0.37-source.tar.gz`.


## 0.38 — borrowed sum carriers

Whole variants, named outcomes and builtin options support shared/exclusive borrowed
inputs and non-consuming decisions. Ordinary payloads become references into active
storage; direct reference payloads are reborrowed with the intersection of carrier
and stored-reference authority. Nested sum/record payloads preserve tagged contained
loans. Shared contained references may outlive the carrier; reborrowed exclusive
permissions retain an authorizing storage guard. Inspection never discharges a
named outcome's handling or a resource's completion obligation.

`Operation::BorrowedVariant` records the distinction from consuming decisions. The
IR verifier checks its reference operand, arguments and payload types; expansion
and independent certificate checking substitute its reference expression. Both
execution backends borrow active payload addresses without moving their owner or
projecting inactive alternatives. Guard validation now admits aggregate storage
that owns exclusive reference leaves.

`tests/sum_carriers.rs` checks payload mode, owned/shared/exclusive fields, direct
and nested origin contracts, empty alternatives, shared detachment, exclusive
carrier retention, generic checking, last use, and illegal moves/mutations. Native
comparisons cover populated/absent named outcomes, ordinary owned payloads, optional
references and nested options, at O0/O2 before/after expansion. Module checks retain
private payload fields and source lifetimes; LSP/editor tests check inferred payload
mode, unsaved owner conflicts, repair and native execution. Malformed borrowed IR
decisions are rejected independently.

Whole reference-bearing carrier reference results remain rejected: direct `from`
contracts do not yet propagate their contained loans. This pre-existing boundary is
now an explicit next roadmap checkpoint, covering records and sums together.
Ungrouped reference-bearing sums, reference rebinding, borrowed collection elements,
borrowed loop state and aggregate trait contracts retain their existing boundaries.
Try [sum_carriers.tellur](../playground/sum_carriers.tellur).

Verification: all **611 Rust tests** pass, including native original/expanded and
optimization comparisons. After making the whole-carrier result diagnostic
consistent, follow-up checks also pass: 576 semantic tests and 147 targeted
reference/storage/trait/value tests, including native cases (these overlap the
full suite). Strict all-target Clippy, formatting, 32 TextMate/configuration tests,
and the actual VS Code integration suite pass. The manual checks compile 46 accepted
examples, verify 12 expected diagnostics, classify 14 context fragments and resolve
121 local links. All 47 canonical playground/example sources check with the release
compiler. Packaged and installed compiler/server binaries, client, grammar, snippets,
metadata and executable modes match the source release. The package targets
`darwin-arm64`.

Logs: `work/tellur-0.38-tests.log`, `tellur-0.38-final-semantics.log`,
`tellur-0.38-final-boundaries.log`, `tellur-0.38-clippy.log`,
`tellur-0.38-grammar.log`, `tellur-0.38-reference.log`, `tellur-0.38-editor.log`,
`tellur-0.38-release.log` and `tellur-0.38-package.log`. Source snapshot:
`work/revision-backups/tellur-0.38-source.tar.gz`.


## 0.39 — whole carrier reference results

C1b.3b.2d adds direct shared/exclusive reference results pointing to grouped records,
variants, named outcomes and options containing references. The outer borrow result
retains carrier storage; stored-return contracts describe reference leaves inside
that carrier. Explicit bracket mappings compose with `from input`, or the body can
infer the payload and both lifetime layers. Direct calls, forwarding and inline
adapters install contained loans on the returned indirect holder.

A shared outer reference does not grant exclusive contained access. Selected sum
payloads preserve constructor guards; returning or borrowing a named outcome does
not discharge its owner's handling obligation. Copied shared fields may outlive a
carrier, while exclusive-field reborrows retain authorization. Explicitly dropping
a Copy carrier now checks live storage borrows before releasing its contained loans.
Independent shared copies still have independent retainers.

`tests/carrier_results.rs` covers explicit/inferred contracts, projected and tagged
carriers, forwarding, generic records, private imported types, multiple possible
owners, detached children, alias transfers, wrong origins, local escapes, illegal
moves/mutations and retained outcome obligations. The IR verifier rejects missing,
duplicate and malformed contained or storage contracts; expansion certificate tests
reject erased or altered metadata. Native tests compare interpreter results with
original/expanded executables at O0/O2 for record and populated/empty option results,
plus the combined playground example.

LSP tests and the real VS Code suite check inferred reference-type hover, both return
lifetime layers, an unsaved premature-drop diagnostic, repair and native Build and
Run. The maintained manual explains the existing notation; no grammar additions are
needed. Try [carrier_results.tellur](../playground/carrier_results.tellur).

C1b.3b.2e is next: storing carrier references inside captured outcomes or aggregates.
Their dependency paths need a reference-indirection step, and those operations remain
gated. Aggregate trait results, reference-bearing collections, rebinding and loop
state are not included in this release.


Final verification: all **630 Rust tests** passed, including native comparisons at
O0/O2 before and after checked expansion. The focused carrier/LSP rerun passed after
the final hover adjustment. Clippy passed with warnings denied; all **32** Node
syntax/configuration tests passed. Manual checks accepted **48** examples, required
**12** diagnostics, classified **14** context fragments and resolved **125** local
links. All **48** canonical playground/example sources checked with the release CLI.
The real VS Code suite exited successfully, including the carrier-result milestone.

The 0.39.0 darwin-arm64 VSIX was checked byte-for-byte against its release sources,
installed, and its installed metadata, client, syntax files and executable compiler/
server binaries were verified. Local canonical and compatibility binaries were also
updated. Source snapshot: `work/revision-backups/tellur-0.39-source.tar.gz` (249 files,
all archived bytes verified against the checkout).

Logs: `work/tellur-0.39-tests.log`, `tellur-0.39-final-feature.log`,
`tellur-0.39-clippy.log`, `tellur-0.39-grammar.log`, `tellur-0.39-reference.log`,
`tellur-0.39-examples.log`, `tellur-0.39-editor.log`, `tellur-0.39-release.log`,
`tellur-0.39-package.log`, `tellur-0.39-install.log` and
`tellur-0.39-installed-integrity.log`.


## 0.40 — locally stored carrier references

C1b.3b.2e.1 permits references to reference-bearing carriers inside local records,
variants, options and captured outcomes. A stored reference retains a recursive tree
of contained loans. Conflict checks traverse that tree while it is packed; liveness
follows live reference fields as well as named aliases. Moving, copying when allowed,
capture and stored adapters retain both storage and external-source dependencies.
Captured outcomes retain their move-only handling rule; capture still runs now.

Unpacking reconstructs the reference shape under its new decision domain, including
empty alternatives, and unions possible producer sources conservatively. Tagged
leaves remain abstract until their own alternative is selected, avoiding reads of
inactive payload storage. Shared outer authority cannot grant exclusive nested
access. Scope-escape checks traverse all retained dependencies. Local borrowing may
use concrete provenance without a public schema; public parameters and results
continue to validate schemas separately, including inferred results.

`tests/stored_carriers.rs` covers capture/transfer, nested record and option storage,
shared retainers, detached fields, borrowed inspection, recursive local reference
layers, same-tag source unions, empty alternatives, private imports, generic fields,
loop-local capture, mandatory handling, scope escape, wrong access modes and the
remaining public-boundary gates. Native cases compare interpreter execution against
original/expanded O0/O2 executables for captured references, nested optional jobs,
empty jobs and stored adapters. Expansion certificates reject modified stored-carrier
code. Existing whole-carrier result coverage now includes later handling of capture.

The example [stored_carriers.tellur](../playground/stored_carriers.tellur), maintained
manual and LSP/VS Code coverage include inferred container/reference types, unsaved
premature-drop diagnostics, repair and native execution. No runtime pointer format
or source syntax changes are introduced.

C1b.3b.2e.2 remains next: public contracts for containers of carrier references,
separating each storage and contained-source layer. Aggregate trait contracts,
reference type arguments, rebinding, reference-bearing loop state and borrowed
collection storage remain outside this release.


Final verification: **651 Rust tests** passed across the suite runs, including
original/expanded native comparisons at O0/O2. The initial run stopped at an old
stored-record test that expected local carrier borrowing to be unsupported; that
expectation was replaced with successful execution and a source-loan rejection.
The complete stored-record suite and every remaining suite then passed. Clippy
passed with warnings denied, and all **32** Node syntax/configuration tests passed.
Manual validation accepted **49** complete examples, required **13** diagnostics,
classified **14** context fragments and resolved **129** local links. All **49**
canonical playground/example sources checked with the release CLI. The real VS Code
integration suite passed, including the stored-carrier milestone.

The 0.40.0 darwin-arm64 VSIX was verified against the release payload, installed,
and its installed metadata, client, syntax assets, Apache license and executable
compiler/server were checked. Local canonical and compatibility binaries were
updated. Source snapshot: `work/revision-backups/tellur-0.40-source.tar.gz`
(254 files, all archived bytes verified against the checkout).

The source checkout now uses the Tellur GitHub repository, retaining its original
Apache 2.0 license commit. Compiler and extension metadata use Apache-2.0. The README
includes fresh-clone build instructions; generated binaries, VSIX packages, native
builds and dependency caches are excluded from Git.

Logs: `work/tellur-0.40-tests.log`, `tellur-0.40-stored-records.log`,
`tellur-0.40-tests-remaining.log`, `tellur-0.40-doctests.log`,
`tellur-0.40-final-feature.log`, `tellur-0.40-clippy.log`,
`tellur-0.40-grammar.log`, `tellur-0.40-reference.log`,
`tellur-0.40-examples.log`, `tellur-0.40-editor.log`,
`tellur-0.40-release.log`, `tellur-0.40-package.log`,
`tellur-0.40-install.log` and `tellur-0.40-installed-integrity.log`.


## 0.41 — public nested-reference contracts

C1b.3b.2e.2 adds separate storage and referent mappings on reference fields:
`ref mutable view['text] target <- 'storage`. Public schemas retain reference-layer
steps independently of record indices and guarded sum alternatives. Whole owned and
borrowed record/variant/outcome/option inputs and results compose these mappings;
capture, forwarding, inference and nested decisions preserve them. Option reference
schemas expose `source` plus `referent_`-prefixed contained groups.

The checker reconstructs nested permissions without projecting inactive alternatives.
Holder access retains its full authorization chain; contained storage guards authorize
their own carrier while detached exclusive reborrows continue to freeze it. Declared
guards may conservatively cover a deeper field. The independent IR verifier checks
all reference layers, payload shapes, source projections and effective shared access.
Runtime pointer representation and native lowering remain unchanged.

`tests/public_carriers.rs` covers explicit/inferred contracts, owned and borrowed
boundaries, shared/exclusive restrictions, detached children and copies, empty tags,
named contracts, option selectors, three reference layers, generic layouts, private
imports, source unions and storage/source escape. Native tests compare original and
expanded O0/O2 execution with the interpreter. An IR regression corrupts both result
and source reference-layer paths. LSP and real VS Code coverage verify declaration
and auto hover, unsaved loan diagnostics, repair and native execution.

The maintained reference documents the syntax and remaining boundaries. Aggregate
trait contracts are next. Abstract type-parameter origin-shape bounds, reference type
arguments, reference-bearing collections, rebinding and reference loop state remain
separate work. Calls selecting nested origins from constructor arguments currently
require a local binding first. Try [public_carriers.tellur](../playground/public_carriers.tellur).


Final verification: all **671 Rust tests** passed. The final focused rerun covered
carrier results/storage, public carriers, named/option origins and LSP. The completed
public-carrier suite passed all **18** tests, including an added forwarding case and
**24** native comparisons: captured public records, plain records, populated/empty
outer variants, and references to populated/empty variant referents, each original
and expanded at O0/O2. Clippy passed with warnings denied. All **32** Node
syntax/configuration tests and the real VS Code integration suite passed, including
the public-carrier milestone.

Manual validation accepted **50** complete examples, required **14** diagnostics,
classified **14** context fragments and resolved **133** local links. All **50**
canonical playground/example sources checked with the release CLI. The 0.41.0
darwin-arm64 VSIX was verified against all release payload files, installed, and
its installed metadata, client, language assets, license and executable compiler/
server were checked. Local canonical and compatibility binaries were updated.
Source snapshot: `work/revision-backups/tellur-0.41-source.tar.gz` (258 files,
all archived bytes verified against the checkout).

Logs: `work/tellur-0.41-tests.log`, `tellur-0.41-final-feature.log`,
`tellur-0.41-public-carriers.log`, `tellur-0.41-clippy.log`,
`tellur-0.41-grammar.log`, `tellur-0.41-reference.log`,
`tellur-0.41-examples.log`, `tellur-0.41-editor.log`,
`tellur-0.41-release.log`, `tellur-0.41-package.log`,
`tellur-0.41-install.log` and `tellur-0.41-installed-integrity.log`.
