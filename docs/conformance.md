# Revision status

Compiler 0.57.0 retains the v0.1 profile below and adds revised syntax plus owned
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


## 0.42 — aggregate trait origin contracts

28 October 2026. C1b.3b.2f extends trait signatures and implementation conformance
to explicit grouped structs, variants, named outcomes, options, direct carrier
references and nested stored carrier references. Named completion groups are bound
before comparing contracts. Parameter names may differ and exit labels may be
reordered; storage and contained origins compare by parameter position, public
schema group and complete reference-layer path. Missing mappings are rejected even
for unused declarations. Implementation bodies still prove each promised origin.

Abstract generic witnesses carry the declared contracts. Known generic schemas may
use associated payload types; unused generic bodies cannot acquire extra origins
from today's concrete implementation. Unknown associated/self origin layouts still
need a future origin-shape bound. Whole-carrier argument checking recognizes its
own authorizing storage guards while preserving conflicts across arguments and
within constructor expressions. Runtime representation and native lowering are
unchanged.

`tests/aggregate_traits.rs` covers grouped/direct/nested results, renamed parameters,
reordered labels, named completion binding, associated payload types, generic
forwarding/capture, group/input swaps, local body violations, missing mappings,
shared/exclusive permissions, storage retention, unused invalid generic bodies,
private imported layouts, empty/populated sums and retained alias guards. Native
checks compare the nested exclusive fixture and independent shared fields before
and after expansion at O0/O2. LSP and real VS Code checks cover inferred payload
hover, unsaved conformance errors, repair and native execution.

See [the new reference section](language-reference.md#aggregate-trait-origin-contracts)
and [aggregate_traits.tellur](../playground/aggregate_traits.tellur). Borrowed
collection elements are the next roadmap checkpoint; origin-shape bounds, rebinding,
reference-bearing loop state and runtime polymorphism remain separate work.


Final verification: all **686 Rust tests** passed across the completed suites.
The full run found one legacy named-trait gate assertion; it now accepts the explicit
contract and rejects its missing-origin form. Its corrected suite and all remaining
targets passed. The **14** aggregate-trait tests include **8** native comparisons
(two programs, original/expanded at O0/O2). Clippy passed with warnings denied;
all **32** Node syntax/configuration tests and the full real VS Code integration
suite passed, including the new aggregate-trait milestone. Doc-tests passed (none
currently defined).

Manual checks accepted **51** complete examples, required **15** diagnostics,
classified **14** context fragments and resolved **138** local links. The release
CLI checked all **51** canonical playground/example sources. The 0.42.0 Apple Silicon
VSIX matches all nine staged payload files and was installed; installed metadata,
assets, binaries and executable permissions were verified. Canonical and legacy
local binaries were updated. Source snapshot:
`work/revision-backups/tellur-0.42-source.tar.gz` (**261** source files,
all archived bytes verified).

Logs: `work/tellur-0.42-tests.log`, `tellur-0.42-tests-remaining.log`,
`tellur-0.42-focused.log`, `tellur-0.42-clippy.log`, `tellur-0.42-doctests.log`,
`tellur-0.42-grammar.log`, `tellur-0.42-reference.log`, `tellur-0.42-examples.log`,
`tellur-0.42-editor.log`, `tellur-0.42-release.log`, `tellur-0.42-package.log`,
`tellur-0.42-install.log` and `tellur-0.42-installed-integrity.log`.


## 0.43 — local collections of shared references

28 October 2026. C1b.3c.1 adds local arrays, vectors and maps containing direct
shared references to owning values. Lookup returns the stored reference directly,
including through mutable collection access. It can survive collection mutation or
destruction while its external source remains protected. Copies and clones retain
independent source dependencies. Insertions track successful outcome domains;
replacement, removal, consuming take and conversion preserve returned dependencies.
Dynamic indices and keys use a conservative union of sources. Only an explicit
empty pop proof releases all of that collection's retainers before its last use.

The checker rejects escaping inner-scope owners and outer collection mutation in
loops. Exclusive-reference elements, nested reference-bearing elements/storage,
public collection boundaries and stored collection outcomes await later checkpoints.

`tests/borrowed_collections.rs` covers arrays, vectors, maps, alias access, independent
retainers, branch-sensitive insertion, source release, empty pop, rejected mutation
and escape, generic local bodies, public selected-reference returns and unsupported
boundaries. Its native comparisons run three programs before/after expansion at
O0/O2. LSP and actual VS Code tests cover inferred `auto` hover, unsaved owner
conflicts, repair and native execution. See the
[reference section](language-reference.md#local-collections-of-shared-references)
and [borrowed_collections.tellur](../playground/borrowed_collections.tellur).


Final verification: all **704 Rust tests** passed in the full regression run,
including **17** borrowed-collection tests with **12** native comparisons (three
programs, original/expanded at O0/O2). Clippy passed with warnings denied. All **32**
Node syntax/configuration tests and the complete real VS Code integration suite
passed, including the new borrowed-collection milestone. Doc-tests passed (none
currently defined).

Manual checks accepted **52** complete examples, required **16** diagnostics,
classified **14** context fragments and resolved **141** local links. The release
CLI checked all **52** canonical playground/example sources. The 0.43.0 Apple Silicon
VSIX matches all nine staged payload files and was installed; installed metadata,
assets, binaries and executable permissions were verified. Canonical and legacy
local binaries were updated. Source snapshot:
`work/revision-backups/tellur-0.43-source.tar.gz` (**265** source files,
all archived bytes verified).

Logs: `work/tellur-0.43-tests.log`, `tellur-0.43-focused.log`,
`tellur-0.43-borrowed.log`, `tellur-0.43-clippy.log`, `tellur-0.43-node.log`,
`tellur-0.43-reference.log`, `tellur-0.43-examples.log`, `tellur-0.43-editor.log`,
`tellur-0.43-build.log`, `tellur-0.43-package.log`, `tellur-0.43-install.log` and
`tellur-0.43-installed-integrity.log`.

## 0.44 — explicit public contracts and forwarding diagnostics

Public controls require explicit outcome labels, payload types and complete
borrowed-result origins. Private whole-contract and selected-payload inference
remain supported. Public named outcomes bind their origin groups explicitly;
unused generic declarations receive the same checks before specialization.

`tests/public_contracts.rs` covers inferred-public rejection, direct and nested
origin requirements, named contracts, generic definitions, private inference,
invalid implementation origins, changed labels and payloads, declared unproduced
alternatives and bounded contributor notes. Its native fixture compares interpreted
and expanded behavior with native O0/O2 execution. Module tests check source paths,
line numbers and readable names across imports. LSP and real editor tests exercise
unsaved public inference, forwarded outcome growth, related locations, repair and
native execution. `playground/public_contracts.tellur` is the maintained example.

Contributor coverage is deliberately bounded to producing exits and direct
forwarding chains. Captured/stored outcomes and complete adapter-absorption
explanations remain P1b work. Related locations are the first P2 increment; full
ranges, JSON diagnostics and independent-error recovery are not implemented here.

Final verification: all **718 Rust tests** passed, including the **12** new public
contract tests and **four** native comparisons (original/expanded at O0/O2).
Clippy passed with warnings denied. All **32** Node tests and the complete real
VS Code integration suite passed. The editor test's legacy manifest now supplies
the formatting primitive used by the new example, and its older public borrowed
result fixture carries the required explicit origin.

Manual checks accepted **54** examples, required **18** diagnostics, classified
**14** context fragments and resolved **147** local links. The release compiler
checked all **53** canonical playground/example sources. The 0.44.0 Apple Silicon
VSIX matches all nine staged payload files and is installed; installed metadata,
assets, binaries and executable permissions were verified. Canonical and legacy
local binaries were updated. Source snapshot:
`work/revision-backups/tellur-0.44-source.tar.gz` (**269** source files,
all archived bytes verified).

Logs: `work/tellur-0.44-tests.log`, `tellur-0.44-focused.log`,
`tellur-0.44-clippy.log`, `tellur-0.44-node.log`, `tellur-0.44-reference.log`,
`tellur-0.44-examples.log`, `tellur-0.44-example-run.log`, `tellur-0.44-editor.log`,
`tellur-0.44-build.log`, `tellur-0.44-package.log`, `tellur-0.44-install.log` and
`tellur-0.44-installed-integrity.log`.

## 0.45 — Rust source adapters and a maintained settings application

I1a generates Rust result enums, exact function signatures and concrete data
conversions from primitive contracts. The profile supports scalar/string/bytes,
nested vectors/maps and shared call-duration inputs. Each binding classifies its
execution; foreign pure expressions, returned references, retained resources,
callbacks and async bindings remain gated. Project dependencies have explicit
resolution and locked builds; manifest-relative paths work in CLI and editor.

P5a supplies a persistent settings CLI with Tellur validation, defaults and failure
mapping. Rust supplies bounded file reads, strict JSON and file replacement. The
application exposes uncertain replacement rather than promising rollback or crash
durability. The same program and scenarios are intended to grow into the server.

`tests/rust_adapters.rs` covers supported/rejected binding contracts, checking without
host-source access, typed signature errors, external Rust dependency reuse, stale
lock rejection, owned/borrowed conversions, panic termination and scripted isolation.
`tests/settings_application.rs` compares policy and injected failures before/after
expansion, with native O0/O2 execution; live tests cover persistence/reload, invalid
keys/files, duplicates, size limits, preparation/commit failures and cleanup.
LSP and real editor tests exercise project-relative adapters, unsaved errors and
repair, locked native builds, persistent updates and reads.

Validation covers **728 Rust tests** (the full suite plus a corrected application
fixture rerun). The fixture correction uses the original source manifest when
compiling a fresh probe, rather than feeding compiler-generated internal types back
through manifest validation. All three application tests pass after that correction.
Adapter/library round trips and application execution run original/expanded at
O0/O2, with additional panic and live replacement-failure cases. Clippy passes with
warnings denied. All **32** Node tests and the complete real VS Code suite pass,
including persistent settings updates and reads through the extension.

Manual checks accepted **54** examples, required **18** diagnostics, classified
**14** context fragments and resolved **149** local links. Release tools checked
**55** sources: 53 canonical examples/playground files and two application files.
The 0.45.0 Apple Silicon VSIX was verified against all nine staged payloads and
installed; installed metadata, assets, binaries and executable permissions match.
The **280-file** source snapshot at `work/revision-backups/tellur-0.45-source.tar.gz`
was byte-verified. Canonical and compatibility local binaries were updated.

Logs: `work/tellur-0.45-tests.log`, `tellur-0.45-focused.log`,
`tellur-0.45-settings.log`, `tellur-0.45-clippy.log`, `tellur-0.45-node.log`,
`tellur-0.45-reference.log`, `tellur-0.45-examples.log`, `tellur-0.45-editor.log`,
`tellur-0.45-build.log`, `tellur-0.45-package.log`, `tellur-0.45-install.log` and
`tellur-0.45-installed-integrity.log`.

## 0.46 — Stored outcome and adapter contributors

The diagnostic analysis walks checked operations without changing the executable
IR or exhaustiveness rules. It follows captures, moves, local record fields and
nested sum payloads, stored/nested adapters and delayed exits. Label merges retain
contributing alternatives. Guarded writes remove old lineage on overwritten
histories. Unknown mutation, indirect aliases, loop parameters and cross-call
payload dependencies are explicit coverage limits.

`tests/contributors.rs` covers capture/move filtering, renaming and absorption,
merged labels, payload separation, disjoint definitions, whole/field overwrite,
mutable calls, borrowed matching/cloning of ordinary variants, invalid adapter
results, nested adapters, loop boundaries and bounded chains. Module/LSP and real
editor tests cover original-file related locations, unsaved edits and repairs.

All **743 Rust tests** passed in the full suite, including the new 13 contributor
cases, module/LSP tests and the existing original/expanded native comparisons.
Clippy passed with warnings denied. All **32** Node tests and the complete real
VS Code integration suite passed, including contributor locations and repairs.

The manual checks accepted **54** examples, required **19** diagnostics, classified
**14** context fragments and resolved **150** local links. The release compiler
checked all **55** maintained sources. The 0.46.0 Apple Silicon VSIX matches all nine
staged payloads; installed files, metadata and executable permissions were verified.
Canonical and compatibility local binaries were updated. The **282-file** source
snapshot at `work/revision-backups/tellur-0.46-source.tar.gz` was byte-verified.

Logs: `work/tellur-0.46-tests.log`, `tellur-0.46-focused.log`,
`tellur-0.46-clippy.log`, `tellur-0.46-node.log`, `tellur-0.46-reference.log`,
`tellur-0.46-examples.log`, `tellur-0.46-editor.log`, `tellur-0.46-build.log`,
`tellur-0.46-package.log`, `tellur-0.46-install.log` and
`tellur-0.46-installed-integrity.log`.

## 0.47 — JSON checks and structured path witnesses

P2 now provides `tellurc check SOURCE [MANIFEST] --json`: one versioned stdout
report, source points with explicit coordinate encoding, related notes and typed
structural witnesses. LSP diagnostics expose the same witness data. Imported
control names are restored, null locations stay explicit, and driver/manifest
errors follow the same envelope. Textual diagnostics keep their existing meaning.
Full expression ranges and independent-error recovery remain queued.

`tests/diagnostics.rs` checks typed selected/entry histories, report counts and
legacy CLI parity, UTF-8/CRLF coordinates, module names and related source points,
manifest/IO/usage errors, nested-directory discovery and source symlinks. LSP tests
check witness updates and absence after unrelated errors. The real extension test
also checks a report through its bundled compiler.

The full suite passed **752 Rust tests**. After compacting the optional witness
storage, the final **84** CLI, LSP and contributor tests passed again; Clippy passed
with warnings denied. All **32** Node tests and the complete real VS Code suite
passed, including a JSON check with the bundled compiler and the persistent
settings application's native build/run.

Manual checks accepted **54** examples, required **19** diagnostics, classified
**14** context fragments and resolved **152** local links. The release compiler
successfully checked all **55** maintained sources through JSON reports. The
0.47.0 Apple Silicon VSIX matches its nine staged payloads; installed metadata,
files and executable permissions were verified. Canonical and compatibility local
binaries were updated. The **283-file** source snapshot at
`work/revision-backups/tellur-0.47-source.tar.gz` was byte-verified.

Logs: `work/tellur-0.47-tests.log`, `tellur-0.47-focused.log`,
`tellur-0.47-focused-final.log`, `tellur-0.47-clippy.log`, `tellur-0.47-node.log`,
`tellur-0.47-reference.log`, `tellur-0.47-examples.log`, `tellur-0.47-editor.log`,
`tellur-0.47-build.log`, `tellur-0.47-package.log`, `tellur-0.47-install.log` and
`tellur-0.47-installed-integrity.log`.


## 0.48 — Explicit executable outcome status

Named outcome alternatives accept `@exit N` defaults. Final-entry mappings are
exhaustive, permit per-entry manifest overrides, and never inherit hidden policy
from a captured result or producer. Normal outcome compatibility is unchanged.
Native execution, JSON output and scripted scenarios preserve outcome/payload
reporting while returning the mapped status; `tellurc exec` propagates it exactly.
Only controls with complete mappings and supported entry parameters are exposed
by the generated launcher. Library checks remain independent of launcher policy.

`tests/exit_codes.rs` covers declaration/range/duplicate validation, missing and
unknown mappings, borrowed-entry rejection, partial defaults and explicit overrides,
module identity, generic contracts, destination-contract forwarding/capture,
certified expansion, every maintained executable playground, and native O0/O2
status/payload behavior including 0, 23, 255, hidden entries and runtime faults.
The settings application exercises meaningful live and scripted failure statuses.
LSP and TextMate checks cover annotation errors, repair, highlighting and contract
hover. The real VS Code suite also checks nonzero Build and Run status with intact
JSON output.

Validation covers **762 distinct Rust tests**. The full 761-test run exposed two
migration assumptions in fixtures (anonymous main inference and concatenated
contracts sharing a name). Both were corrected; all **66 LSP/native tests** passed
again. The final **9 exit-contract tests** include the additional playground
completeness regression. The consolidated last-result inventory contains no failed
tests. Clippy passed for all targets with warnings denied. All **33 Node tests**
and the full real VS Code suite passed.

The manual accepts **55** examples, requires **20** diagnostics, classifies **14**
context fragments and resolves **161** local links. All **55** maintained sources
pass release JSON checking. The documented alternate `parse_port` entry also builds
and executes. Canonical `.tellur` playgrounds were migrated; preserved `.path`
snapshots remain unchanged and need explicit launch policy for native builds.

The local compiler and language server report 0.48.0. All nine VSIX payloads were
compared byte-for-byte against staging, and installed files, metadata and executable
permissions were verified. The source snapshot is
`work/revision-backups/tellur-0.48-source.tar.gz`.

Evidence: `work/tellur-0.48-validation.json`, `tellur-0.48-tests.log`,
`tellur-0.48-final-focused.log`, `tellur-0.48-migration-regressions.log`,
`tellur-0.48-clippy.log`, `tellur-0.48-reference.log`, `tellur-0.48-examples.log`,
`tellur-0.48-node.log`, `tellur-0.48-editor.log`, `tellur-0.48-build.log`,
`tellur-0.48-port.log`, `tellur-0.48-package.log`,
`tellur-0.48-package-integrity.log`, `tellur-0.48-install.log` and
`tellur-0.48-installed-integrity.log`.

## 0.49 — Retained Rust resource owners

I1b.1 binds opaque must-complete Tellur types to owned Rust objects through
`rust.resources`. Shared calls borrow the actual object; consuming calls remove
and transfer it. Returned resource payloads receive fresh identities, including
when the wrapper returns an object for retry. The generated interfaces retain
concrete Rust types and require no `Clone`, `Send` or `Sync` implementation.

`tests/rust_adapters.rs` covers binding validation, ownership diagnostics without
reading Rust, duplicate shared borrows, mixed borrowed/consuming inputs, transfers
through a Tellur helper before/after expansion at O0/O2, explicit failed cleanup,
host teardown and fuel faults. The retained-file example runs live and with
scripted end/read-failure paths. Nested resource arguments supplied as live JSON
are rejected before acquisition or other wrapper execution; scenario handles stay
symbolic. `tests/native_host.rs` checks nominal identity, consuming aliases, stale
and cross-host handles, wrong concrete Rust types, fresh identities and exactly-once
destruction. LSP tests check unsaved completion errors and repair without `host.rs`.

The full **769-test** Rust run passed. Final resource/process-entry regression
checks also passed after strengthening the helper-transfer fixture and adding the
live-entry rejection, for **770 distinct passing Rust tests**. Clippy passed for
all targets with warnings denied. All **33 Node tests** passed. The manual accepts
**55** examples, requires **20** diagnostics, classifies **14** context fragments
and resolves **162** local links. All **56** maintained Tellur sources pass checking.

The full real VS Code suite passed again with the final rebuilt compiler, including
the retained-file completion diagnostic, unsaved repair and native open/read/release
sequence. Compiler and LSP initialization report **0.49.0**. All nine packaged
payloads match staging; the installed extension's contents, metadata and executable
permissions match the package. The verified source snapshot is
`work/revision-backups/tellur-0.49-source.tar.gz`.

This release supports direct resource parameters/payloads. Foreign exclusive
mutation, returned references, aggregate boundary conversion, async suspension and
cancellation remain unimplemented. Host teardown drops remaining Rust objects;
it does not execute fallible Tellur cleanup or report its outcomes. The file
example's release operation does not claim checked close errors or write durability.

Evidence: `work/tellur-0.49-validation.json`, `tellur-0.49-tests.log`,
`tellur-0.49-resources-final.log`, `tellur-0.49-clippy.log`,
`tellur-0.49-reference.log`, `tellur-0.49-examples.log`,
`tellur-0.49-node.log`, `tellur-0.49-build.log`, `tellur-0.49-editor-final.log`,
`tellur-0.49-versions.log`, `tellur-0.49-package.log`,
`tellur-0.49-package-integrity.log`, `tellur-0.49-install.log`,
`tellur-0.49-installed-integrity.log` and `tellur-0.49-backup.log`.

## 0.50 — Sequential loopback settings service

P5b.1 adds a real HTTP interface to the maintained settings application. The
Tellur modules own endpoint selection, JSON shape/key validation, response status,
independent exchange/cleanup errors, connection budgets and idle completion.
Typed Rust adapters provide retained listeners/connections, bounded request parsing
through pinned `httparse` 1.10.1, response framing and socket completion. The CLI
continues to use the same settings module and persistence contracts.

`tests/settings_http.rs` checks live listing, lookup, updates and persistence across
restarts; invalid keys/JSON/files/media/methods/routes; Host/Origin restrictions;
duplicate/conflicting framing; header count/size and body-size limits; request
deadlines; disconnects; HEAD framing; ignored pipelined requests; occupied ports;
budget/idle shutdown and listener release. Eighteen scripted cases preserve
read/write/cleanup failures, both errors together, idle-after-failure and uncertain
replacement. They agree in reference and expanded execution and native original/
expanded O0/O2 builds. The staged connection close fixes the truncated-response
failure exposed by an early live oversized-header test; its bounded nonblocking
drain avoids changing socket timeouts after half-close on macOS.

The final full run passed **775 Rust tests**, with no failed targets. Clippy passed
for all targets with warnings denied. All **33 Node tests** and the full real VS
Code suite passed, including unsaved listener-completion errors, repair, Build and
Run, a real HTTP exchange and graceful stop. The manual checks **55** accepted
examples, **20** expected diagnostics, **14** context fragments and **165** local
links; all **58** maintained Tellur sources pass release checking.

The rebuilt application also passed the documented curl workflow against an
ephemeral loopback port: list, persist an update, lookup and normal budget completion.
Compiler and LSP initialization report **0.50.0**. All nine packaged payloads match
staging; installed files, metadata and executable permissions were verified. The
verified source snapshot is `work/revision-backups/tellur-0.50-source.tar.gz`.

This is a bounded sequential development HTTP profile, with no TLS, authentication,
full HTTP/1.1 coverage, concurrency or signal/task cancellation. Socket deadlines
do not bound filesystem operations. Response delivery and descriptor release do
not imply rollback, peer receipt, observable close-error success or crash durability.
The [application guide](../applications/settings/README.md) records exact limits.
Async tasks and LLVM coroutine lowering remain separate roadmap checkpoints.

Evidence: `work/tellur-0.50-validation.json`, `tellur-0.50-tests.log`,
`tellur-0.50-http-tests-final.log`, `tellur-0.50-clippy.log`,
`tellur-0.50-reference.log`, `tellur-0.50-examples.log`, `tellur-0.50-node.log`,
`tellur-0.50-editor.log`, `tellur-0.50-build.log`,
`tellur-0.50-application-build.log`, `tellur-0.50-http-smoke.log`,
`tellur-0.50-versions.log`, `tellur-0.50-package.log`,
`tellur-0.50-package-integrity.log`, `tellur-0.50-install.log`,
`tellur-0.50-installed-integrity.log` and `tellur-0.50-backup.log`.

## 0.51 — Expression source ranges

Value-expression nodes retain half-open source ranges through parsing, module
resolution and generic rewriting. Expression checking preserves a child error's
range instead of replacing it with its enclosing statement location. Argument
and payload mismatches, annotated initializers, assignments, reference bindings,
field accesses and moved values select their relevant expressions. Missing call
arguments select the call; incompatible comparison operands select the right
operand. Parser errors select the unexpected token, with a zero-width EOF range.

`check --json` retains the `tellur.check.v1` schema and existing `position` field;
the additive nullable `range` contains one-based Unicode scalar start/end points.
LSP ranges convert both endpoints to zero-based UTF-16, including multiline
strings and CRLF. Imported diagnostics keep the original file's coordinates and
prefer its open editor URI when available. Contributor notes, path witnesses and
point-only declaration/contract diagnostics retain their existing representation.

The new tests cover nested calls and comparisons, list elements, escapes,
parenthesized initializers, assignment/reference mismatches, moved values,
multiline Unicode/CRLF, imported generic bodies and parser token/EOF locations.
Editor tests cover unsaved imported sources, repair, inference hover freshness
and a bad HTTP status payload in the settings service without reading Rust host
code. The real VS Code test selects the exact multiline string containing emoji.

This remains a single-primary-error checker. Independent-error recovery, broader
contract/declaration ranges and ranged contributor notes are separate follow-ups;
no language syntax, ownership rules or execution policy changes in this release.

Validation: **781 Rust tests** pass in the full regression run; the final range
checks pass all **84** library/diagnostic/LSP tests. Clippy passes all targets with
warnings denied. All **33 Node tests** and the complete real VS Code suite pass,
including live settings HTTP and retained-resource execution. The current manual
checks **55** accepted examples, **20** expected diagnostics, **14** context
fragments and **165** local links. All **58** maintained Tellur sources pass release
checking. Compiler and LSP initialization report **0.51.0**. All nine packaged and
installed payloads, package metadata and executable permissions are verified.

Evidence: `work/tellur-0.51-validation.json`, `tellur-0.51-tests.log`,
`tellur-0.51-ranges.log`, `tellur-0.51-clippy.log`, `tellur-0.51-reference.log`,
`tellur-0.51-examples.log`, `tellur-0.51-node.log`, `tellur-0.51-editor.log`,
`tellur-0.51-build.log`, `tellur-0.51-versions.log`, `tellur-0.51-package.log`,
`tellur-0.51-package-integrity.log`, `tellur-0.51-install.log`,
`tellur-0.51-installed-integrity.log` and `tellur-0.51-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.51-source.tar.gz`.

## 0.52 — Independent control diagnostics

The checker collects a bounded batch of errors from independent control bodies.
A failed body restores the manifest and fresh-ID state used for checking, discards
its inferred bindings, and publishes no checked region or inferred contract.
Dependency ordering skips failed callers transitively, including dependencies in
`outcome<producer>` type annotations, loop parameters and arm payload bindings.
Errors retain their own source ranges, related notes and structural witnesses,
then sort by source coordinates. Generic witness names use authored type parameters.
No partial `CheckedProgram` escapes a failed check.

Textual and JSON `check` commands and the LSP consume the batch APIs. The existing
single-error compiler APIs select the first diagnostic. `tellur.check.v1` gains
`diagnostics_limited`, and its diagnostics array may contain multiple entries.
Collection stops at 32 errors, with a visible limit notice in textual/editor output.
The LSP rechecks current unsaved sources and provides no inferred hover from a
failed or previous successful version.

Tests cover independent ordinary and generic bodies, failed-call chains and typed
producer dependencies, ownership/inference isolation, distinct path witnesses,
imported source ranges, textual/JSON parity, budget exhaustion and repair, unsaved
import edits and closing buffers, stale-version rejection and hover invalidation.
The settings project checks two broken leaf helpers together without executing
host Rust code. The real editor test exercises two errors, suppressed caller
cascades, partial repair and full repair.

This is one primary error per independent body in a checking phase. Parsing,
import/name resolution, shared declarations, generic expansion and call-graph
cycles remain fail-fast; an unsuccessful generic phase prevents concrete checking.
Recovery within a damaged body and broader preparation-phase recovery remain
follow-ups. P3 path readability is the next main roadmap checkpoint.

Validation: the full regression run passes **793 Rust tests**, and the final
focused diagnostic/LSP/manual run passes all **94** tests. Clippy passes all
targets with warnings denied. All **33 Node tests** and the complete real VS Code
suite pass, including live HTTP and retained-resource execution. The manual checks
**55** accepted examples, **20** expected diagnostics, **14** context fragments
and **165** local links. All **58** maintained Tellur sources pass release checking.
Compiler and LSP initialization report **0.52.0**; all nine packaged and installed
payloads, package metadata and executable permissions are verified.

Evidence: `work/tellur-0.52-validation.json`, `tellur-0.52-tests.log`,
`tellur-0.52-diagnostics-final.log`, `tellur-0.52-clippy.log`,
`tellur-0.52-examples.log`, `tellur-0.52-node.log`, `tellur-0.52-editor.log`,
`tellur-0.52-build.log`, `tellur-0.52-versions.log`, `tellur-0.52-package.log`,
`tellur-0.52-package-integrity.log`, `tellur-0.52-install.log`,
`tellur-0.52-installed-integrity.log` and `tellur-0.52-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.52-source.tar.gz`.

## 0.53 — Optional path grouping

`[path]: { ... }` gives a selection an explicit end. Inner selectors intersect the
group domain, inner `entry` selects that domain, and closing the group resumes the
enclosing body's entry selection. A preceding flat label stays ended. Existing
unbraced label extent remains unchanged. Empty groups are explicit no-ops; they do
not complete a live history. Unlabeled block statements remain unsupported.

Grouping introduces no lexical value or lifetime scope: compatible bindings and
path aliases remain visible afterward, live borrows retain their owners, moves
remain conditional, and resource obligations survive `}`. Scoped `over &:` retains
its separate lifetime behavior. Tail loops may end a selected group; nested source
loops remain rejected. Module resolution, generic rewriting, inference dependency
collection, liveness and tail extraction all traverse the new body item. Extraction
preserves complete groups and still rejects a cut inside a nested body.

`tests/path_groups.rs` covers nested/intersecting selection, entry restoration,
unbraced compatibility, empty groups, moved values, live references, resource
completion, tail loops, imported generic bindings, extraction boundaries, and
reference/expanded/native agreement at O0/O2. LSP, TextMate and real VS Code tests
exercise grouped inferred bindings, an impossible nested selector, unsaved repair
and native execution through a shared continuation. The runnable example is
`playground/path_groups.tellur`; the reference gives checked positive and negative
examples plus canonical layout guidance. Default labels stay unbraced. The editor
adds a separate `path_group` snippet and clarifies the scoped `over` snippet.

Selected-history editor feedback and direct arm exits remain the next P3 work.
This release does not add a formatter, multi-statement arm bodies or new lifetimes.

Validation: all **803 Rust tests** pass. Clippy passes all targets with warnings
denied; all **34 Node tests** and the complete real VS Code suite pass. The manual
checks **56** accepted examples, **21** expected diagnostics, **14** context fragments
and **167** local links. All **59** maintained Tellur sources pass release checking.
Compiler and LSP initialization report **0.53.0**. All nine packaged and installed
payloads, package metadata and executable permissions are verified.

Evidence: `work/tellur-0.53-validation.json`, `tellur-0.53-tests.log`,
`tellur-0.53-focused.log`, `tellur-0.53-manual-final.log`, `tellur-0.53-clippy.log`,
`tellur-0.53-examples.log`, `tellur-0.53-node.log`, `tellur-0.53-editor.log`,
`tellur-0.53-build.log`, `tellur-0.53-versions.log`, `tellur-0.53-package.log`,
`tellur-0.53-package-integrity.log`, `tellur-0.53-install.log`,
`tellur-0.53-installed-integrity.log` and `tellur-0.53-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.53-source.tar.gz`.

## 0.54 — Direct arm exits

Ordinary delta arms may use `=> exit` with the same nullary, payload, stored-outcome
and nested-adapter meanings as an exit statement. The arm comma/end replaces the
statement semicolon. Continuing arms still publish their path atom and payloads;
terminating arms use private payload scopes and publish no source path. Exhaustive
coverage, fixed contracts, moves, borrowing and resource completion remain checked.
Later selected work cannot overlap a terminated history. Legacy decision syntax
and an existing path named `exit` remain accepted.

The checker resolves terminating payload declarations before exporting continuing
payloads, making sibling name/type reuse independent of arm order. This does not
reorder runtime effects: exactly one outcome is selected. Actual outer bindings
still cannot be shadowed. The existing decision IR and exit actions preserve
provenance and remain subject to expansion certificates; no new runtime primitive
or activation model is introduced.

`tests/arm_exits.rs` checks mixed/all-terminal routing, private and shared sibling
names, fixed/inferred contracts, resources, borrowed results, local escape,
stored completions, nested adapters, legacy parsing, groups/loops, module and
generic traversal, imported expression ranges, failed-caller suppression,
extraction, and original/expanded native agreement at O0/O2. LSP and real VS Code
tests check inferred payload hover, precise exit errors, repairs and native output.
TextMate tests distinguish exit keywords, outcomes and continuing paths. The
`delta_exit` snippet and `playground/arm_exits.tellur` make the feature available
for experimentation.

The maintained settings application now directly exits validation, loading and
encoding failures while retaining named success paths and its existing public
contracts. Scripted and live CLI/HTTP tests maintain the behavior. General arm
statement blocks and direct loop restarts remain unsupported. Selected-history
editor feedback is the next P3 checkpoint.

Validation covers **816 distinct Rust tests**: the full run passed **815**, then
final focused runs passed **84** compiler/LSP tests and **8** CLI/HTTP tests after
the sibling-name ordering fix; the added sibling regression also passes with
reference payloads in both arm orders. Clippy passes all targets with warnings
denied. All **35 Node tests** and the complete real VS Code suite pass, including
live HTTP and retained-resource execution. The manual checks **57** accepted
examples, **22** expected diagnostics, **14** context fragments and **169** local
links. All **60** maintained Tellur sources pass release checking. Compiler and
LSP initialization report **0.54.0**; all nine packaged and installed payloads,
metadata and executable permissions are verified.

Evidence: `work/tellur-0.54-validation.json`, `tellur-0.54-tests.log`,
`tellur-0.54-focused-final.log`, `tellur-0.54-application-final.log`,
`tellur-0.54-sibling-final.log`, `tellur-0.54-manual-final.log`,
`tellur-0.54-clippy.log`, `tellur-0.54-examples.log`, `tellur-0.54-node.log`,
`tellur-0.54-editor.log`, `tellur-0.54-build.log`, `tellur-0.54-versions.log`,
`tellur-0.54-package.log`, `tellur-0.54-package-integrity.log`,
`tellur-0.54-install.log`, `tellur-0.54-installed-integrity.log` and
`tellur-0.54-backup.log`. The verified source snapshot is
`work/revision-backups/tellur-0.54-source.tar.gz`.

## 0.55 — Selected-history editor feedback

The checker records bounded source selections separately from executable IR and
expansion certificates. Hovers join those checked selections to parser-recorded
ranges for statements, selectors, aliases, loop headers, delta arms and inline
results. Resolved guards include enclosing intersections and expanded aliases;
exact duplicate Boolean terms are removed. Empty aliases/groups are marked empty.
Decision-arm origins retain authored outcome spellings and source coordinates.
Duplicate fact names from distinct decisions are disambiguated with explanatory
`@line:column` suffixes, while implicit local outcomes are distinguished from
exported paths. These display suffixes are not language syntax.

Existing declaration and inferred-type hovers take precedence. Generic-body
selections use the universally checked definition rather than a particular call.
Source modules map ranges/origins back to their own coordinates; UTF-16 editor
positions and multiline source ranges remain correct. Every hover checks the
current compilation unit, including unsaved dependencies. Failed checks publish
no old/partial selection, and comments/whitespace have no selection hover.

The display describes structural selection, not feasible inputs. Loop-body
selections are relative to one iteration's entry, including its inherited entry
conditions; they do not enumerate invocations or backedges. Rendering has a
96-node traversal budget, depth limit 48 and eight displayed origin entries.
Truncation is explicit, with no claim that omitted conditions are unconditional.
These are source/editor facts, not general history enumeration or behavioral proof.

Eight new LSP regressions cover flat/grouped extents, aliases, empty selections,
entry restoration, local/direct/nested arm facts, duplicate names, loops, generic
and trait bodies, unsaved edits/import failures, UTF-16/CRLF/multiline ranges,
comments, and bounded displays. Existing string-hover coverage now distinguishes
selection of the containing statement from fabricated `auto` inference. The real
VS Code test exercises aliases, nested groups, source ranges, comments, selection
changes, invalidation, repair and native execution of `history_hover.tellur`.

This completes the planned P3 slice alongside optional groups and direct exits.
P4 remains queued for formatting, broader explain queries and contract changes;
multi-statement arm syntax and executable async remain separate future work.

Validation: all **824 Rust tests** pass, along with **35 Node tests** and the
complete real VS Code suite, including live HTTP and retained-resource execution.
Clippy passes all targets with warnings denied. The manual checks **57** accepted
examples, **22** expected diagnostics, **14** context fragments and **171** local
links. All **61** maintained Tellur sources pass release checking. Compiler and
LSP initialization report **0.55.0**; all nine packaged and installed payloads,
package metadata and executable permissions are verified.

Evidence: `work/tellur-0.55-validation.json`, `tellur-0.55-tests.log`,
`tellur-0.55-focused.log`, `tellur-0.55-manual-final.log`, `tellur-0.55-clippy.log`,
`tellur-0.55-examples.log`, `tellur-0.55-node.log`, `tellur-0.55-editor.log`,
`tellur-0.55-build.log`, `tellur-0.55-versions.log`, `tellur-0.55-package.log`,
`tellur-0.55-package-integrity.log`, `tellur-0.55-install.log`,
`tellur-0.55-installed-integrity.log` and `tellur-0.55-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.55-source.tar.gz`.

## 0.56 — Source formatting

P4a supplies `tellurc format SOURCE [--check | --write]` and the language server's
standard whole-document formatting provider. The default CLI mode writes formatted
source to stdout; checking and explicit in-place writing are separate modes. No
manifest discovery, import loading, semantic checking or native execution is needed
by the formatter. Syntax failures leave source untouched; empty/comment-only files
and syntax-valid files with semantic errors can be formatted.

Parser layout metadata distinguishes control/data/trait bodies, delta arms, generic
angles, flat labels and optional groups. Flat selected statements receive a further
indentation level; groups retain their authored end, restoring the enclosing body's
layout. Every token, brace, separator, literal spelling and comment stays in order.
A raw token/comment comparison and output parse guard the returned text. This is a
whitespace-preservation guarantee, not a proof that an input program is correct.

Authored header/argument/payload wrapping and paragraph separation are retained;
blank runs are bounded. Generated layout uses the first newline convention outside
literals, with LF as fallback. Multiline literal bytes are unchanged. The CLI uses
four spaces; LSP honors spaces/tabs and widths 1–16. The current buffer is used,
including untitled and unsaved files; unchanged or syntactically broken documents
produce no edits. No range/on-type formatting, automatic width wrapping, import
sorting, implicit brace insertion or project-wide discovery is claimed.

Eight formatter regressions cover exact label/group/adapter layout, every token
boundary in a comment-insertion specimen, UTF-8/multiline literals, CRLF, tabs,
authored wrapping, generic/comparison disambiguation, origin selectors, annotations,
compatibility syntax, CLI modes and rejected writes. All **106 modern and legacy
maintained sources** are formatted twice, reparsed and checked with formatted
imports; accepted programs stay accepted, and rejected fixtures retain their
primary diagnostic code/message. A selected-path example compares original,
formatted and expanded executions for all four Boolean input combinations.

Two new LSP regressions and the extended protocol test cover current buffers,
UTF-16 replacement extents, indentation options, invalid options, missing manifests,
syntax refusal, stale changes and closed documents. Actual VS Code integration
checks its standard Format Document action, minimal edit handling, unsaved source,
spaces/tabs, Unicode, comments, idempotence, syntax refusal and type-invalid source.
The existing grouped-path example is formatted before native build/run, preserving
its selected effects and shared continuation.

Validation: **834 Rust tests**, **35 Node tests** and the full real VS Code suite
pass, including live HTTP and retained-resource execution. The final formatter and
LSP/manual focused runs cover the last layout refinement. Clippy passes all targets
with warnings denied. The manual checks **58** accepted examples, **22** expected
diagnostics, **14** context fragments and **176** local links. All **61** maintained
Tellur sources pass release checking. Compiler and LSP report **0.56.0**; formatting
capability, all nine packaged/installed payloads, metadata and executable permissions
are verified.

Evidence: `work/tellur-0.56-validation.json`, `tellur-0.56-tests.log`,
`tellur-0.56-focused.log`, `tellur-0.56-format-final.log`, `tellur-0.56-clippy.log`,
`tellur-0.56-examples.log`, `tellur-0.56-node.log`, `tellur-0.56-editor.log`,
`tellur-0.56-build.log`, `tellur-0.56-versions.log`, `tellur-0.56-package.log`,
`tellur-0.56-package-integrity.log`, `tellur-0.56-install.log`,
`tellur-0.56-installed-integrity.log` and `tellur-0.56-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.56-source.tar.gz`.

## 0.57 — Checked public contract snapshots

P4b.1 exports versioned public source contracts with `tellurc contract-snapshot`
and compares them with `contract-diff`. Export uses the ordinary module loader and
full checker, including generic-definition validation; invalid source never yields
a partial snapshot. Diff reports named additions, removals and structural changes.
`--check` fails when contracts differ. No semantic compatibility classifier or
behavioral-equivalence claim is attached to these changes.

Snapshots cover public declarations in the explicit import graph, using portable
entry-relative module identities. They retain the current/legacy ownership profile,
ordered signatures, generic requirements, direct/contained/referent origins, visible
trait implementations and associated bindings, named outcomes and process statuses.
Referenced manifest schemas are included transitively. Private representations,
implementation-local parameter names, bodies, source locations and local import
aliases are excluded. Public construction order and private-field effects on
copy/clone/drop remain visible; generic capabilities are recorded symbolically with
memoized, bounded expansion. Strict reading rejects unknown fields, duplicate object
keys and unsupported format versions.

Thirteen new contract regressions cover relocation and alias/order stability,
private representation changes, construction permissions, nested/generic capabilities,
borrow origins, trait requirements/bindings, status changes, manifest dependencies
and normalization, invalid-source refusal, strict deserialization, CLI statuses and
all **61 maintained sources**. The settings library has a checked, committed baseline;
a regression changes its internal input-size limit while preserving that baseline.
This demonstrates the intended boundary: equal public contracts do not imply equal
behavior. Internal absorption/contributor changes remain the next P4b.2 increment.

Two new LSP regressions and the protocol test cover checked JSON export, unsaved
import graphs, invalid dependencies, closed/stale documents, missing file identities
and manifests, and capability advertisement. **Tellur: Show Public Contract Snapshot**
opens an editable JSON document from the live source graph. Actual VS Code tests
check its contents, private-field filtering, immediate unsaved changes and refusal
to open a snapshot after checking fails. The editor harness now closes its output
panel before the standard formatting action so the readonly output editor cannot
receive that action.

Validation: **849 Rust tests**, **35 Node tests** and the full real VS Code suite
pass, including live HTTP and retained-resource execution. The final **142-test**
focused run covers the last snapshot/profile/manifest changes and module/LSP/manual
integration. Clippy passes all targets with warnings denied. The manual checks
**58** accepted examples, **22** expected diagnostics, **14** context fragments and
**181** local links. All **61** maintained Tellur sources pass release checking and
snapshot export. The release settings diff reports equality. Compiler and LSP
report **0.57.0**; snapshot capability, all nine packaged/installed payloads,
metadata and executable permissions are verified.

Evidence: `work/tellur-0.57-validation.json`, `tellur-0.57-tests.log`,
`tellur-0.57-focused-final.log`, `tellur-0.57-clippy.log`,
`tellur-0.57-examples.log`, `tellur-0.57-settings-diff.log`, `tellur-0.57-node.log`,
`tellur-0.57-editor-panel.log`, `tellur-0.57-build.log`, `tellur-0.57-versions.log`,
`tellur-0.57-package.log`, `tellur-0.57-package-integrity.log`,
`tellur-0.57-install.log`, `tellur-0.57-installed-integrity.log` and
`tellur-0.57-backup.log`. The verified source snapshot is
`work/revision-backups/tellur-0.57-source.tar.gz`.
