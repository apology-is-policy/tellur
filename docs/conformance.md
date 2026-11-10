# Revision status

Compiler 0.145.0 retains the v0.1 profile below and adds revised syntax plus owned
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

## 0.58 — Outcome absorption evidence

P4b.2 exposes the checked contributor analysis as typed, bounded source evidence.
`absorption-snapshot` exports public contracts alongside the internal contributors
to public exits; `absorption-diff` reports public-contract equality separately from
recorded-evidence equality. A private failure can change while its existing public
mapping stays the same. The settings regression introduces `blank` under public
`invalid_key` and verifies precisely that distinction.

The checker retains structured contributors outside executable IR and certificates;
existing diagnostic text is rendered from the same facts. Capture/moves, local
aggregate payloads, stored/nested adapters, guarded overwrites, merged labels and
forwarding retain their existing bounded lineage. Each exported contributor has
its source control, outcome, operation where applicable, local site and location.
Named decision sites survive, repeated calls remain distinct, and qualified trait
members cannot capture unrelated top-level declaration names. Locations are excluded
from comparison; formatting, alias changes, module relocation and declaration
reordering do not create evidence differences.

The snapshot records its analysis profile and limitations. It contains at most
32 contributors per outcome and propagates truncation explicitly; even identical
truncated artifacts fail `--check`. Generic public templates have explicit
`generic_definition_only` entries, while concrete generic/trait callees can supply
body evidence. Loop-carried values, indirect storage, dynamic collection contents,
cross-call payload lineage, scalar/predicate changes and concrete feasibility remain
outside this analysis. Contributor sets are not complete invocation trees or
execution traces. Neither equality flag establishes behavioral equivalence.

Fourteen new absorption regressions cover hidden and added failures, capture and
forwarding, merged outcomes, guarded replacement, borrowed inspection, distinct and
named sites, formatting/relocation stability, generic roots and concrete callees,
trait-member identities, loop limits, truncation, strict reading, CLI modes, the
settings baseline and every maintained source. Two new LSP regressions cover unsaved
imported implementation changes, invalid dependencies, stale edits, close/revert,
missing manifests and file identities. The protocol advertises the new capability.
Actual VS Code tests invoke **Tellur: Show Outcome Absorption Snapshot**, inspect
the opened JSON, check an unsaved hidden failure change and reject invalid source.

The editor harness uses the real formatter provider and live-buffer edit API when
macOS leaves its isolated test window inactive; focused windows retain the standard
format action. The verification log records which route ran. This avoids making
compiler/editor regression coverage depend on foreground-window activation.

Validation records **865 passing Rust regressions** across the full run and final
focused reruns, **35 Node tests**, and the full real VS Code suite including live
HTTP and retained-resource execution. The initial full run caught a missing process
status in the new playground example; its named `@exit 0` contract was added and all
nine exit-code regressions rerun successfully. The final 129-test focused run covers
the last name-normalization change. Clippy passes all targets with warnings denied.
The manual checks **58** accepted examples, **22** expected diagnostics, **14**
context fragments and **190** local links. All **62** maintained sources pass release
checking and both snapshot exports. The settings absorption baseline matches without
truncation. The new example runs in the interpreter and as a native executable.
Compiler and LSP report **0.58.0**; both snapshot capabilities and all nine packaged
and installed payloads, metadata and executable permissions are verified.

Evidence: `work/tellur-0.58-validation.json`, `tellur-0.58-tests.log`,
`tellur-0.58-focused-final.log`, `tellur-0.58-final-regressions.log`,
`tellur-0.58-clippy.log`, `tellur-0.58-examples.log`, `tellur-0.58-settings-diff.log`,
`tellur-0.58-node.log`, `tellur-0.58-editor-verified.log`,
`tellur-0.58-example-run.log`, `tellur-0.58-example-native.log`,
`tellur-0.58-build-final.log`, `tellur-0.58-versions.log`,
`tellur-0.58-package.log`, `tellur-0.58-package-integrity.log`,
`tellur-0.58-install.log`, `tellur-0.58-installed-integrity.log` and
`tellur-0.58-backup.log`. The verified source snapshot is
`work/revision-backups/tellur-0.58-source.tar.gz`.

## 0.59 — Bounded path and outcome explanations

The P4c query slice provides `tellurc explain SOURCE [MANIFEST] --at LINE:COLUMN`
and `--control NAME --outcome NAME`, with readable Markdown by default and a
versioned `tellur.explanation` JSON report under `--json`. Source-position queries
reuse the selected-history metadata, including aliases, groups, local outcome
facts, empty selections, generic/trait source bodies and one-iteration loop context.
Outcome queries reuse the absorption source index and bounded contributor sets;
ordinary private controls can be queried without making their declarations public.
Imported contributors retain their module identities and source locations.

The language server advertises `tellurExplain: 1` and implements checked
`tellur/explain` and `tellur/explainTargets`. VS Code adds **Explain Path Selection
at Cursor** and **Explain Outcome Contributors**, including an outcome picker and
editable Markdown reports. Requests use current unsaved source graphs; the outcome
request checks again after the picker. Invalid sources, manifests or targets do
not produce partial/stale reports. Input positions are scalar-based in the CLI and
UTF-16 in the editor. Markdown links navigate to source lines, with scalar columns
in their labels, avoiding editor-dependent column encodings.

The implementation preserves the analysis bounds: 96 captured condition nodes,
depth 48, eight displayed decision origins and 32 contributors per outcome.
Condition/origin omissions and contributor truncation are explicit. These are
structural observations, not feasible-input proofs, complete execution histories,
execution certificates, or behavioral equivalence judgments. Generic/trait
outcome roots remain unavailable; source-position queries inside their bodies
remain supported. Loop-carried values, indirect storage, dynamic collection contents
and cross-call payload lineage remain follow-up work.

The new `playground/path_explanations.tellur` example has an explicit executable
status contract and runs both in the interpreter and natively, emitting
`classification completed`, `rejected`, then completing with `done`. The settings
application documents queries for private `validate_key.invalid` and public
`set.storage_failed`; the latter retains file-read, encoding and write failure
contributors. Its committed contract/absorption baseline remains unchanged.

Validation: **879 distinct Rust tests pass**. The full all-targets run passed 878;
the final 103-test explanation/LSP/manual run also passed, adding a Unix filename
encoding regression after final review. The latter confirms that a report which
cannot be serialized returns an error without terminating the server. Clippy with
warnings denied and Rust formatting checks pass. All **63 maintained modern sources**
check; the manual verifies **58 accepted examples, 22 expected diagnostics,
14 declared context fragments and 197 local links**. All **35 Node tests** pass.

The complete real VS Code integration suite passes, including the new cursor
query with UTF-16 input, Markdown output and source links, the actual private-outcome
picker, updated unsaved contributors and invalid-source refusal. The existing
formatter test uses the real provider and applies edits to the live buffer when
macOS leaves the isolated test window inactive. Existing native builds, LLVM output,
retained resources and the live HTTP application remain covered. The final report
serialization error handling is covered by the targeted LSP regression above.

Compiler and language server report **0.59.0**, including the explanation capability.
All nine packaged and installed extension payloads match staging; executable bits
are retained. The verified extension is installed without reloading the user's
existing editor windows. Reload VS Code to activate it in an already open window.

Evidence: `work/tellur-0.59-validation.json`, `tellur-0.59-tests.log`,
`tellur-0.59-final-regressions.log`, `tellur-0.59-clippy-final.log`,
`tellur-0.59-examples.log`, `tellur-0.59-settings-diff.log`, `tellur-0.59-node.log`,
`tellur-0.59-editor.log`, `tellur-0.59-example-run.log`,
`tellur-0.59-example-native.log`, `tellur-0.59-selection.md`,
`tellur-0.59-explanation.json`, `tellur-0.59-settings-validation.md`,
`tellur-0.59-settings-failure.md`, `tellur-0.59-build-final.log`,
`tellur-0.59-versions.log`, `tellur-0.59-package.log`,
`tellur-0.59-package-integrity.log`, `tellur-0.59-install.log`,
`tellur-0.59-installed-integrity.log` and `tellur-0.59-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.59-source.tar.gz`.

## 0.60 — Execution-boundary policy and observed activations

P6a records the agreed boundary model: invariant faults terminate the current
execution boundary; ordinary controls do not catch them and resume. Explicit
isolated child boundaries remain future work. The checkpoint implements reporting
for existing returned execution failures, not a source `fault` keyword or a
transitive fault-effect system. Resource teardown is explicitly distinguished from
normal/fallible protocol completion. The author also chose explicit fault-disposition
contracts for live must-complete resources, checked at faulting paths and calls;
that P6b requirement is recorded in `docs/faults-and-activations.md`.

The shared `tellur.execution-failure` envelope records entry, phase, diagnostic
text, observed activation identities/parents, call/current sites and expansion
provenance. Reference `run_detailed` exposes it alongside the existing diagnostic;
`run`/`run_with_limit` preserve their diagnostic API. Reference CLI `run --json`
and native `--json`/`--scenario` write returned execution failures to stderr, with
status 1 and no successful completion object. Native pre-execution launcher errors
and foreign panics retain their existing behavior. Normal outcomes and process
status mappings are unchanged.

Invocation IDs restart at one per entry boundary. Completed calls are discarded;
`again` reuses the activation. Reports retain the first/final 16 active frames when
necessary, up to 32 expansion origins per site and 4096 diagnostic Unicode scalars,
with explicit omissions. Report IDs are not addresses: value storage is still the
sequential compilation-slot store, so recursion, isolation and suspension remain
unimplemented. Native optimizer inlining preserves the instrumentation of checked
Tellur calls; source expansion can eliminate those calls while preserving source
and expansion origins in the failure site.

The new `playground/execution_faults.tellur` deliberately exhausts a host instruction
budget during its second worker invocation. Its native report identifies root 1
and worker 3, not the already completed worker 2. The selected source site is the
instruction-budget check site; a guard can still be false there. The settings
application documents its current whole-entry boundary and unchanged cleanup limits.

Validation: **888 distinct Rust tests pass**. The full all-targets run passed 886;
the final 12-test failure-report/manual run added two regressions for imported
entry display names and invalid entry text that resembles deeply nested generics.
All eight settings CLI/HTTP tests were rerun successfully after incorporating the
author's server layout, including real persistence, live loopback HTTP and ordinary
versus expanded execution at optimization levels 0 and 2. The source edit changes
layout only; labels remain unbraced and semantically unchanged.

Clippy with warnings denied and Rust formatting checks pass. All **64 maintained
modern sources** check. The reference manual verifies **58 accepted examples,
22 expected diagnostics, 14 context fragments and 208 local links**. All **35 Node
tests** pass, and the settings library's committed public-contract/absorption
baseline is unchanged.

The complete real VS Code integration suite passes, including the new runtime
failure report, stderr JSON, exact failure status, repeated-call activation identity
and source mapping. Existing diagnostics, hover, formatting, explanation reports,
native builds, LLVM output, retained resources and live HTTP remain covered. Final
imported-entry and invalid-entry refinements are covered by the targeted Rust run.

Compiler and language server report **0.60.0**. All nine packaged and installed
extension payloads match final staging, with executable bits retained. The verified
extension is installed without reloading existing editor windows; reload VS Code
to activate it in an already open window.

Evidence: `work/tellur-0.60-validation.json`, `tellur-0.60-tests.log`,
`tellur-0.60-final-regressions.log`, `tellur-0.60-settings-final.log`,
`tellur-0.60-clippy-final.log`, `tellur-0.60-examples.log`,
`tellur-0.60-settings-diff.log`, `tellur-0.60-node.log`, `tellur-0.60-editor.log`,
`tellur-0.60-example-failure.json`, `tellur-0.60-build-final.log`,
`tellur-0.60-versions.log`, `tellur-0.60-package.log`,
`tellur-0.60-package-integrity.log`, `tellur-0.60-install.log`,
`tellur-0.60-installed-integrity.log` and `tellur-0.60-backup.log`.
The verified source snapshot is `work/revision-backups/tellur-0.60-source.tar.gz`.

## 0.61 — Readable wrapping and inline path dispositions

The formatter adopts the author's settings-server layout. An authored first
statement beside a path label remains inline; a standalone label keeps its selected
statements indented below it. Long signatures, calls, payloads, collection literals,
generic arguments and exit contracts wrap at syntax boundaries. Logical conditions
break before `&&`/`||`. A nested adapter beginning below its arm label now retains
the continuation indentation throughout its body. No braces, tokens, comments or
literal bytes are added, removed or reordered.

The preferred width defaults to 100 Unicode scalar columns, with tabs advancing to
indentation tab stops. CLI `format --width N` and VS Code's resource setting
`tellurLanguage.formatWidth` accept 40–240. The LSP option is `maxLineLength`.
Comments and indivisible tokens may exceed the target; existing multiline lists
stay multiline. Formatting still operates on the current unsaved editor buffer,
needs no manifest and refuses broken syntax. It does not enable format-on-save.

The settings server is formatted with the updated compiler, preserving the author's
labels, blank-line divisions and semantic selection extent. Repository instructions
record this convention for subsequent work. The source-fault contract decision is
also recorded: one message-bearing channel, `fault`, and explicit public/trait
`faults` permission with private inference. Those language features remain P6b work.

Validation: **891 distinct Rust tests pass** in the full all-targets run. The final
104-test formatter/LSP/manual run also passes after equivalent loop and formatting
performance refinements. All **109 maintained modern and legacy sources** format
idempotently at widths 40, 100 and 240; token/comment fingerprints and reparsing
hold, checking is preserved at the default width, and selected execution remains
equivalent before/after formatting and graph expansion. Invalid width settings and
CLI combinations cannot write a file. Clippy with warnings denied and Rust
formatting checks pass.

All **64 maintained modern sources** check; the manual verifies **58 accepted
examples, 22 expected diagnostics, 14 context fragments and 209 local links**.
The settings library's public-contract/absorption baseline is unchanged. The full
suite includes the formatted HTTP server's live requests and scripted cleanup
failures, with ordinary/expanded native execution at both optimization levels.
All **35 Node tests** pass.

The complete real VS Code suite passes, including resource-scoped width changes
from 60 to 100 without restarting, formatting the current unsaved buffer, inline
label layout, applying edits and a second no-op format. Existing diagnostics,
hover, contract/explanation reports, native builds, retained resources and live HTTP
remain covered. Compiler and LSP report **0.61.0**; all nine packaged and installed
extension payloads match final staging, with executable bits retained. Existing
editor windows were not reloaded.

Evidence: `work/tellur-0.61-validation.json`, `tellur-0.61-tests.log`,
`tellur-0.61-final-regressions.log`, `tellur-0.61-clippy-final.log`,
`tellur-0.61-examples.log`, `tellur-0.61-settings-diff.log`, `tellur-0.61-node.log`,
`tellur-0.61-editor.log`, `tellur-0.61-build-final.log`, `tellur-0.61-versions.log`,
`tellur-0.61-package.log`, `tellur-0.61-package-integrity.log`,
`tellur-0.61-install.log`, `tellur-0.61-installed-integrity.log` and
`tellur-0.61-backup.log`. The verified source snapshot is
`work/revision-backups/tellur-0.61-source.tar.gz`.

## 0.62 — Source faults and transitive permissions

P6b implements one diagnostic fault channel. `fault expression;` requires a string
and terminates the current execution boundary; ordinary controls cannot catch or
resume it. Flat delta arms also accept direct fault termination. A fault-only
control omits `exits`: `control fail(string reason) faults { fault reason; }`.
Private controls infer transitive permission, while public and trait contracts must
permit `faults`. Implementations inherit the trait permission; generic definitions
check it before specialization. Fault-only calls terminate their selected histories
and cannot be captured as completed outcomes.

The checker examines resources still owned after argument transfer at every fault
and potentially faulting call. Callers' retained owners remain relevant. A callee
can complete a transferred resource before faulting; that does not authorize a
separate resource retained by its caller. Opaque linear types mapped to retained
Rust owners may explicitly declare `"fault": "release"`. This permits ordinary
host destruction on a source fault, without relaxing normal completion obligations.
Records, options and sum payloads preserve contained-resource requirements. Normal
outcome-handling obligations over ordinary data end with the boundary.

Reference and native execution keep source faults separate from normal outcomes,
process status mappings and returned executor errors. Source reports add
`kind: "source_fault"`; earlier returned-error JSON remains readable and unchanged.
The bounded report retains source/activation context. The reference failure API
also exposes prior events, and finite scenario comparison observes full fault
messages and those events separately from permission equality. Fuel/driver failures
remain inconclusive for comparison.

Expansion and certificates retain fault termination and reject forged normal exits
or erased permissions. Tail extraction infers a private helper's permission and
rechecks public contracts. Structural protocols have distinct fault terminals and
residual `faulted`/`can_fault` fields; normal completion fields keep their meaning.
Public snapshots expose control permission and resource disposition. LSP declaration
and `auto` hover, Outline, unsaved diagnostics, TextMate highlighting and a fault-only
snippet reflect the same syntax. `playground/source_faults.tellur` is a runnable
normal/fault comparison; the reference and active roadmap are updated.

Limits remain explicit: no fault-valued inline-adapter leaves, generic fault-release
capability bound, custom fallible fault cleanup, local catch, isolated requests,
activation-owned storage, recursion or
async execution. Source permission does not promise immunity from executor limits,
foreign panics or process termination. Existing returned-error teardown remains its
previous behavior. Retained-owner release is a trusted adapter contract, not a proof
of cleanup success or destructor ordering. P6c activation storage is next.

Validation: **904 distinct Rust tests** pass in the full offline all-target suite,
including source faults, retained-owner destruction, certificates, imports, generics,
traits, original/expanded native O0/O2 execution and the live settings HTTP service.
A final 105-test LSP/source-fault/manual run covers inferred `auto` hover and the
finished reference. Clippy passes with warnings denied; Rust formatting is clean.
All **65 maintained sources** check. The manual checks **60 accepted examples,
22 expected diagnostics, 14 contextual fragments and 210 local links**. Both new
fault-layout fixtures remain idempotent at widths 40, 100 and 240. The settings
contract/contributor baseline remains equal without truncation.

All **36 Node tests** and the full real VS Code suite pass, including unsaved fault
permission diagnostics/repair, inferred hover and fault-only native Build and Run.
The final CLI comparison wording is separately checked against a permission-only
contract change. Reference CLI ordinary/expanded faults and native normal/fault
execution preserve their JSON/status behavior. Compiler/LSP report **0.62.0**;
all nine packaged and installed payloads match staging. The extension was installed
without reloading the author's open windows.

Evidence: `work/tellur-0.62-validation.json`, `tellur-0.62-tests.log`,
`tellur-0.62-final-regressions.log`, `tellur-0.62-clippy-final.log`,
`tellur-0.62-examples.log`, `tellur-0.62-format.log`, `tellur-0.62-settings-diff.log`,
`tellur-0.62-node.log`, `tellur-0.62-editor.log`, `tellur-0.62-cli.log`,
`tellur-0.62-build-final.log`, `tellur-0.62-versions.log`,
`tellur-0.62-package-integrity.log` and `tellur-0.62-installed-integrity.log`.
The verified source snapshot is `work/revision-backups/tellur-0.62-source.tar.gz`.

## 0.63 — Contingency, retained handover and supervised boundaries

P6c/P6d's first synchronous slice is implemented. Named `boundary` declarations
own their inputs; `retains` explicitly selects parameters that survive a source
fault. The checker requires those whole values to remain available at every
faulting path/call. Other must-complete owners still need fault dispositions.
Boundary inputs cannot carry references to outer storage.

`supervise` waits for administrative completion without exposing a child outcome
or payload. `terminate` settles a boundary/contingency; `escalate` propagates the
original fault. Failed ordinary continuations never resume. A contingency starts
after failed activations retire, receives retained owners exactly once, and may
handle ordinary outcomes of its own permitted helper operations. Secondary faults
escape rather than recursively invoking the same handler; bounded causal reports
retain the original failure.

`@contingent` checks transitive call permission. Public and trait contracts are
explicit, private helpers infer eligibility, implementations inherit the trait
subset, and generic witnesses preserve it. Trusted non-pure primitives need
manifest permission. Contingencies cannot supervise new execution. Public boundary
`faults` describes contingency escape. Snapshots, hovers, unsaved diagnostics,
TextMate grammar and snippets reflect these contracts. General annotation policies
and execution flags remain deferred.

Both execution engines use invocation-local storage ranges and retire frames on
normal/fault returns. References address the actual invocation's storage. Native
resource teardown protects handover and parent owners; a live retained-owner test
checks destruction order and continued parent access. Expansion certificates
preserve supervision as an isolation operation. Structural protocol analysis rejects
supervision graphs, and extraction rejects boundary/contingency bodies.

The maintained settings server supervises accepted connections. A trusted emergency
reply operation attempts a generic 500 only before response transmission starts,
then explicit cleanup closes the connection. A real loopback regression injects
source faults through its test loader: a pre-response fault returns 500, another
request succeeds, and a post-response fault closes without appending a second
response. Shipped routes have no fault-injection endpoint. Service completed/idle
statuses describe supervisor lifecycle; request failures remain responses and
reports rather than a result returned by supervision.

Validation: **919 distinct Rust tests pass**, checked against the complete test
inventory. The full run's formatter assertion referred to an old server path; it
was updated to the new unbraced labels, and the remaining inventory plus final
138-test boundary/fault/LSP/manual/format regression pass completed successfully.
**37 Node tests** and the full real VS Code suite pass, including boundary hover,
unsaved permission errors/repair, native build/run, retained resources and HTTP.
All **66 maintained Tellur sources** check. The manual validates **63 accepted
examples**, **22 expected diagnostics**, **15 context fragments** and **219 local
links**. The new runnable contingency example has identical native/reference
reports, including source fault provenance and subsequent normal execution.
Settings library public-contract and absorption baselines remain unchanged.

The 0.63 release binaries and editor package are synchronized. Package and installed
payload integrity are checked; installation does not reload existing editor windows.
Execution remains sequential: no async tasks, recursive calls, foreign panic
recovery, general deep-local recovery registration or proof of handler termination.
The activation model leaves suspended frames and LLVM coroutine lowering for E1/E2.

## Identity deltas and decision sites (0.64)

Flat decisions may omit their arms or list only the transformed alternatives.
Omitted alternatives import the producer's payload names and outcome paths; every
history still needs a disposition. Named decisions qualify those paths, and a
short name becomes ambiguous when more than one site supplies it. Grouped arm
statements share the enclosing value scope while retaining their selected history.
Inline adapters and legacy region decisions remain exhaustive.

Payload inference retains agreeing source names, including forwarding and extracted
helpers. Disagreeing or duplicate names use deterministic positional names. Module
qualification preserves implicit local bindings even when a declaration shares the
payload name. Remaining sibling arm bodies participate in reference liveness.

Validation: **930 distinct Rust tests pass**, reconciled against the complete test
inventory after updating diagnostics and payload-name expectations. The focused
identity suite covers coverage, qualified ambiguity, grouped scope, sibling loans,
module name collisions and original/expanded/native execution. **38 Node tests**
pass. The full isolated VS Code suite passes, including grouped `auto` hover,
qualified path hover, unsaved coverage errors/repair, native execution and the real
HTTP application. The editor fixture was corrected to remove a formatter-wrapped
disposition before the successful final run.

All **67 maintained Tellur sources** check. The manual validates **66 accepted
examples**, **22 expected diagnostics**, **15 context fragments** and **223 local
links**. Settings public-contract and absorption baselines remain unchanged.
Compiler/LSP release binaries, the editor package and the installed extension are
synchronized at 0.64.0; package and installed payload integrity are verified.
Installation does not reload existing editor windows.

Explicit move expressions, lexical supervision and constructor declarations are
the next [accepted revision checkpoints](control-flow-revision.md); this release
does not claim to implement them.

## Owning expressions and call-duration temporaries (0.65)

Explicit `move` composes with owned construction, generic forwarding, partial
record moves, stored reference carriers, completed outcomes and direct reference
transfer. Copy values retain their Copy behavior. Passing an exclusive reference
with `move` consumes that reference; a bare reference argument retains call-duration
reborrowing. Result origins survive an explicitly transferred reference argument.

Borrowed ordinary rvalues receive real call-duration storage. Staging preserves
argument order and path selection, with release before outcome continuation.
Temporary resource/contained-reference owners and potentially escaping result
loans require named storage. Local reference bindings do not extend rvalue lifetimes.
Nested owning constructors already avoid intermediate source variables; this
checkpoint does not add constructor declarations or promise physical copy elision.

Validation: **195 ownership/compiler regressions** and **97 LSP/manual tests** pass,
including nine new owning-expression tests. Differential native tests cover owning
construction and temporary shared/exclusive access before and after expansion at
O0/O2. **39 Node tests** pass. The targeted real VS Code test verifies unsaved move
diagnostics and repair, formatting and native execution using the bundled compiler.
Its formatting assertion now supplies an unformatted document, rather than expecting
edits for a document already in canonical form.

All **68 maintained sources** check. The manual validates **68 accepted examples**,
**22 expected diagnostics**, **15 context fragments** and **229 local links**.
Compiler/LSP binaries and the packaged and installed extension are synchronized at
0.66.0. This historical checkpoint preceded the lexical supervision work delivered below; constructor declarations arrived in 0.67 below.


## Lexical isolation and recovery (0.66)

Delivered lexical boundaries, attached retained-owner contingency, supervised tail
loops with preserved state, structured immutable `@fault` and entry-only process
termination. Checked IR, expansion certificates, reference/native execution,
formatting, hover and syntax highlighting preserve the new administrative edges.
The settings HTTP server uses lexical isolation. Constructor declarations and
direct destination lowering remain the next ownership checkpoint.

Validation for 0.66: all **954 Rust tests across 68 nonempty targets** have passing
results, reconciled against the final test inventory. The full-suite run exposed a
missing process-status contract in the new playground; that fixture was corrected
and its test rerun. Focused reruns include the added recovery/certificate/contract
tests and final contingency-selection metadata. All 40 editor unit tests pass.
The isolated VS Code checkpoint verifies structured hover, forbidden ordinary
return and unsaved repair, formatting, native recovery and process status 23 with
no outcome. The real HTTP suite verifies a fault response followed by a successful
request, alongside response-start protection and framing/deadline regressions.

Manual validation covers 70 accepted examples, 24 expected diagnostics, 15 explicitly
identified context fragments and 232 local links. Local compiler/LSP binaries and
VS Code package/install payloads are verified at 0.66.0. The runtime remains
synchronous, with owned captures and the documented supervisor-access restrictions.

## 0.67 constructor procedures

Structs can declare one constructor with path-sensitive, exactly-once field
initialization and implicit normal completion. Constructor expressions preserve
written argument/field order and boolean short-circuiting; pending arguments and
initialized fields remain owned if a later call faults. Generic bodies forward
unconstrained values without assuming cloning. Public constructor contracts are
separate from public-field raw constructibility, following the author's choice.

Tests cover missing/read-before-init/repeated initialization, disjoint paths,
private/public and imported generic construction, temporary borrows, nested calls,
selected histories, explicit fault/contingency permissions, native/expanded parity,
and actual Rust retained owners released once after partial construction failure.
The import loader keeps parsing off its recursive traversal stack, preserving the
256-file budget diagnostic. Constructor-local isolation and reference-bearing
results remain explicitly gated; direct destination lowering remains queued.

The maintained reference, `playground/construction.tellur`, formatter, public
snapshots, constructor outline/hover, inferred `auto` hovers and unsaved diagnostics
accompany the compiler change. The VS Code integration harness exercises these
features through native build/run. The settings server constructs owned request
context before isolation; live HTTP checks retain the 500-then-next-request behavior.

Final checkpoint validation: 974 Rust tests across 69 nonempty targets (the full
regression suite plus final focused runs), 41 Node editor tests, and an isolated
VS Code session covering constructor/auto hovers, outline, unsaved diagnostics,
formatting and native build/run. Live settings HTTP tests include source-fault 500
handling followed by a successful request. Release compiler/server binaries,
packaged VSIX and installed extension payloads were verified at 0.67.0.


## 0.68 constructor delivery and frame lowering

Native constructor calls deliver completed records into caller storage, using an
internal completion marker rather than an outcome tag and payload vector. Partial
fields remain in the constructor activation until successful initialization;
failure leaves the caller destination unavailable. Tests cover real retained Rust
owners released once on constructor failure and on a later caller fault after
successful delivery. Ordinary outcome and administrative completion channels stay
separate. Single-value completion bindings avoid an extra payload vector, while
multi-value bindings still evaluate all values before installing them.

Owned temporary field projections transfer the selected field in both execution
engines; a buffer-identity test detects accidental cloning. Shared-place reads
retain their existing semantics. These changes preserve instruction identities,
source traces, evaluation order and fuel accounting. They do not establish a
stable ABI, eliminate every record allocation or promise general physical copy
elision.

Native activation frames now use separate dense maps for values and decisions.
Unrelated declarations cannot inflate a control's frame capacity. Instruction IDs,
loop/recovery targets and source provenance remain unchanged. The same storage
mapping covers ordinary calls, constructor destinations, borrowed inputs/results,
loop reset, named retention and lexical boundary retirement. The reference engine
retains its independent logical address representation.

Regression tests check local capacity with 128 unrelated controls and compare
native/reference behavior at every instruction-budget cutoff through nested
construction and returned exclusive borrows, before and after expansion at both
optimization levels. Existing boundary, stored-reference and real retained-resource
suites exercise the same mapping. This remains synchronous activation storage;
suspended frames, scheduling and LLVM coroutine lowering are subsequent work.

Release validation: all 980 Rust tests across 70 nonempty targets passed, alongside
41 Node editor tests and an isolated VS Code session covering constructor/auto
hover, outline, unsaved diagnostics and repair, formatting and native build/run.
The maintained manual checks 72 accepted examples, 25 expected diagnostics,
15 context fragments and 247 local links. Live HTTP checks include source-fault
500 handling followed by a successful next request; the release settings CLI also
reads its sample configuration successfully. Compiler/LSP binaries, packaged VSIX
and installed extension payloads were verified at 0.68.0.

For this release's settings `serve` entry, generated frame capacity falls from
633 value slots and 633 decision slots to 5 and 4 respectively. This measures that
control's storage reservation, not total process memory or execution speed.

## 0.69 reference-bearing constructor contracts

Constructors can return structs containing shared or exclusive references, with
an explicit result-origin mapping after their parameters:
`constructor(ref string left, ref string right)[left, right]`. Each entry supplies
one declared struct origin group. Parameter storage and carried references remain
distinct: `input` selects the parameter's storage, while `input['a]` selects its
contained public origin group. Private constructors also declare their mappings.

Initialization retains exactly-once, path-sensitive checks. Reference field
addresses survive local initializer scopes and merge across disjoint paths with
conservative provenance. Tagged payload addresses are read only after selecting
their alternative. Returning a local borrow, borrowing a temporary owner for a
stored result, or escaping partial `self` remains rejected. Constructor results
retain the existing independent-field last-use rules and exclusive permissions.

Tests cover nested constructors, references to reference-bearing carriers,
optional reference payloads, generic/imported public construction, private fields,
projected exclusive inputs, retained copies, capture/adapters, and earlier argument
borrows protected against later calls. Differential execution compares reference
and native results before and after expansion at O0/O2. Real Rust retained owners
are released once both when a borrowed constructor faults before completion and
when its caller faults after delivery.

Public contract snapshots and constructor hovers include the origin mapping. The
maintained reference and `playground/borrowed_construction.tellur` demonstrate
independent field origins and exclusive access. The isolated VS Code checkpoint
checks constructor outline/origin hover, inferred `auto`, unsaved loan diagnostics
and repair, formatting and native execution.

Raw reference-bearing aggregate expressions containing nested constructor calls
still require intermediate bindings; this composition is the next checkpoint.
Constructor-local isolation, self-referential records and abstract origin-shape
bounds remain outside this slice. Native activation storage remains synchronous.

Release validation: all 997 Rust tests across 71 nonempty targets passed. A final
28-test constructor rerun also covers explicit `move` around a borrowed constructor
call, including native/expanded parity at O0/O2. All 42 Node editor tests and the
isolated VS Code checkpoint passed. The maintained manual checks 73 accepted
examples, 26 expected diagnostics, 15 context fragments and 250 local links.
Live HTTP tests include a source-fault 500 followed by a successful next request.
Release compiler/LSP binaries, packaged VSIX and installed extension payloads were
verified at 0.69.0.

## 0.70 raw borrowed aggregate composition

Raw record fields and variant, option and named-outcome payloads can contain
declared constructor calls when the aggregate stores references. The checker stages
earlier values before later calls, preserving written named-field order and
retaining owned resources and contained loans until complete assembly. A fault in
a later initializer cannot abandon an earlier resource or deliver a partial value.
Existing argument-alias checks protect earlier shared/exclusive references.

Coverage includes generic/imported constructors, private inner representations in
public raw storage, independent field last use, explicit moves, nested optional
tags, factory returns and capture, selected adapter arms and constructor field
initialization across local scopes. Negative cases reject conflicting access,
wrong origins, private construction and temporary-owner escapes. Native/reference
comparisons run before and after expansion at O0/O2. Retained Rust resources are
released exactly once on both later-initializer faults and post-assembly faults;
an always-faulting final initializer still requires earlier resources to declare
their fault disposition.

The reference, roadmap, editor checks and `playground/composed_construction.tellur`
accompany the feature. No new syntax, origin rules or borrowed-collection contracts
are introduced. Bind a reference-bearing constructor result before projecting its
fields or borrowing the whole temporary; direct result projections remain the next
construction checkpoint. The runtime remains synchronous.

Release validation: all 1,009 Rust tests across 72 nonempty targets passed. A final
focused retainer check covers local raw composition, explicit move and surviving
shared copies. All 42 Node editor tests and the isolated VS Code checkpoint passed;
the latter checks origin/auto hovers, outline, unsaved borrow diagnostics and repair,
formatting and native execution of the new example. The manual checks 74 accepted
examples, 26 expected diagnostics, 15 context fragments and 253 local links.
Live HTTP tests retain fault-500-then-next-request behavior. Release compiler/LSP
binaries, packaged VSIX and installed extension payloads were verified at 0.70.0.

A manual comparison also records the existing conservative list-wide alias check:
a mutating nested constructor followed by a borrow of the same source requires
separate declarations even when the first call returns no borrow. This limitation
is documented and queued for initializer access precision; the release does not
claim that every safe sequential initializer arrangement is accepted.


## 0.71 fresh borrowed result projections

Direct and chained field access now works on fresh reference-bearing constructor
results and raw records. The checker materializes the completed record when the
expression is evaluated, then uses ordinary place and contained-origin rules.
Selecting an external reference retains its source and permission; selecting an
owned field transfers it. It does not extend temporary-owned storage or permit a
local reference to escape through a fresh result.

Every owning step of the projection checks unselected fields for resource or
outcome completion obligations. A selected resource or outcome can be transferred;
otherwise a named owner is required to handle the remaining fields explicitly.
Checks also preserve private fields across imported generic constructors. No new
syntax, reference rebinding, asynchronous runtime behavior or copy-elision promise
is introduced. The existing conservative list-wide alias rule remains documented
and is the next construction follow-up.

Coverage includes shared/exclusive references, nested generic records, options,
carrier references with both storage and contained origins, returned borrows,
cloning, direct external mutation, independent unselected loans, collection
reference arguments, selected adapter arms and written evaluation order. The native
matrix covers shared, nested, absent-option, exclusive and carrier cases before and
after expansion at O0/O2, compared with complete reference execution reports.
Retained Rust resource tests verify exactly-once fault release before and after
field delivery, and reject missing fault dispositions.

The maintained reference and `playground/projected_construction.tellur` accompany
this checkpoint. Editor coverage checks inferred owning/shared/mutable types on
`auto`, constructor outline, unsaved borrow errors and repair, formatting and native
execution in an isolated VS Code instance.

Release validation: all 1,021 Rust tests across 73 nonempty targets and all 42 Node
editor tests passed. The isolated VS Code checkpoint passed with the release
compiler and language server. Manual validation covers 75 accepted examples,
27 expected diagnostics, 15 context fragments and 256 local links. Live HTTP fault
recovery still sends 500 and serves the next request. The packaged VSIX and installed
extension's nine payloads were verified against the 0.71.0 release staging files.


## 0.72 sequential initializer access precision

Initializer lists now compare each next expression with the values already
produced by its predecessors. A completed constructor's nonescaping call borrows
no longer overlap later initializers. Returned references, including nested and
projected fields, continue retaining their actual sources. Earlier snapshots stay
frozen in written order; the checker does not reorder named fields into layout
order. Nested lists check their own alias separation.

Anonymous loans created during list evaluation retire when no pending result
retains them. This includes unused reference fields after an owned projection.
Existing caller loans and outer pending arguments are excluded from that retirement;
carrier storage dependencies follow the ordinary liveness closure. This is a
permission change only: pending resource owners and their fault obligations remain
until delivery or boundary settlement. Temporary-owner escape restrictions and
unselected completion obligations are unchanged.

Coverage exercises repeated exclusive calls on one owner, mutation followed by
shared borrowing, independent returned and nonreturned parameter origins, nested
constructors, pure arguments, owning collections, call-duration temporary borrows,
raw/optional/named payloads, exits, captures and selected adapters. Negative cases
retain earlier direct/exclusive/caller loans, reject move-then-use and check aliasing
inside one nested initializer. Reuse after an earlier staged move reports `E_MOVED`
at the later access, replacing the previous whole-list `E_ALIAS` in those cases.
Imported generic constructors preserve privacy and
borrowed results. Native comparisons cover written order, nested calls, retained
origins, owned projections, shared-reference collections and adapters before/after
expansion at O0/O2. A retained
Rust resource fixture checks exactly-once release on later-initializer and
post-assembly faults, and rejects missing fault dispositions.

The maintained manual and `playground/sequential_initializers.tellur` accompany this
checkpoint. The same example is rejected by 0.71's list-wide alias check. The new
editor checkpoint checks `auto` hover, constructor outline, an unsaved retained-borrow
error and repair, formatting and native execution. General reference rebinding,
arbitrary expression-level named-loan lifetimes and async execution remain separate.

Release validation: all 1,034 Rust tests across 74 nonempty targets and all 42 Node
editor tests passed. The final isolated VS Code checkpoint passed against the
release binaries. Manual validation covers 76 accepted examples, 28 expected
diagnostics, 15 context fragments and 260 local links. Live HTTP fault recovery
still sends 500 and serves the next request. All nine packaged and installed
extension payloads were verified against the final 0.72.0 staging files.

## 0.73 straight-line statement diagnostic recovery

P2a extends the bounded diagnostic batch inside a control. Failed top-level
declarations, assignments, drops, emits and single-completion calls can be followed
by independent statement errors. A diagnostic-only replay starts from fresh checker
state, omits previously failed statements and suppresses statements depending on
their read, written or declared values. Record fields taint their whole carrier;
references and source owners are conservatively linked, including nested carriers.

Replay never publishes inferred contracts, hover bindings, selected histories or
executable IR. Failed controls and their callers remain suppressed as before.
Coverage, result inference and constructor completion are not checked on a body
with omissions. The 32-error budget is shared between statements and controls.

Recovery deliberately stops at decisions, captures/adapters, aliases, nested scopes,
loops, boundaries and terminal dispositions. Nested-body errors remain first-only.
A whole-state fault-resource error also stops replay, avoiding conclusions about
resources whose earlier transfer or completion is uncertain. Preparation-phase
recovery and finer path/field precision remain separate checkpoints.

`tests/diagnostic_recovery.rs` covers independent and transitive value errors,
partially moved call arguments, direct and nested reference aliases, independent
ownership witnesses, final resource/coverage suppression, fault-obligation barriers,
structural and termination barriers, generic definitions, constructors and the shared
budget. `tests/diagnostics.rs` adds imported CLI batches with exact source ranges;
`tests/lsp.rs` adds unsaved same-body diagnostics, partial repairs and fresh inferred
hover after a complete repair. Existing parsing/shared-declaration tests keep their
first-error expectations; the former same-body first-error assertion is replaced by
the explicit recovery suite.

`playground/diagnostics.tellur` is a runnable editing exercise. The real VS Code
checkpoint changes two initializers, verifies both expression ranges and dependent
suppression, repairs them independently, checks a changed inferred type, then builds
and runs the restored program. The initial editor run caught the example's missing
process-status annotation; the corrected example declares `@exit 0 done`, and the
final isolated editor run passed. The same broken source reports one error with
0.72 and both independent errors with 0.73.

Validation: **1,045 Rust tests across 75 nonempty targets** and **42 Node editor
tests** passed. The manual checked **76 accepted examples, 29 expected diagnostics,
15 context fragments and 262 local links**. The full run includes native constructor
and borrowing parity, retained-resource fault cleanup, and the live HTTP test that
sends a 500 response for a source fault before serving the next request successfully.
Formatting and whitespace checks passed. All six local compiler/server copies match
the 0.73.0 release artifacts; all nine packaged and installed extension payloads were
verified. Existing user VS Code windows were not reloaded.

## 0.74 path and scope diagnostic recovery

P2b carries diagnostic uncertainty on the selected BDD domain. Omitting a failed
statement taints its read, written and declared values on that domain; disjoint
paths retain ordinary ownership and loan checking. Fresh replay now covers flat
path labels and aliases, nested groups and lexical scopes, statement-arm bodies,
direct/ordinary exits, forwarding and loop-body/backedge statements. No replay
publishes inferred contracts, hover bindings, selected histories or executable IR.

Structural operations reached on an uncertain history stall that history. A later
selector touching stalled histories is suppressed in full; it is never narrowed to
the remaining histories. Unknown selectors after omissions stop their containing
body. Final scope resource checks are suppressed for uncertain scope domains;
clean disjoint paths still check resources. Damaged loop completion does not invent
coverage errors. Fault boundaries and contingencies, inline outcome adapters,
decision headers and scope/loop setup/completion retain conservative limits.

The recovery suite covers separate flat paths, aliases, nested groups and lexical
continuations, statement arms, direct arm exits, new decisions on clean paths,
path-local loan witnesses, mixed-selector suppression, unknown-selector boundaries,
resource completion, loop backedges, generic and constructor bodies, fault/adapter
barriers and one shared 32-error budget. Imported CLI ranges and unsaved LSP
diagnostics preserve their source locations and fresh-hover rules. The earlier
nested-body first-error assertion now expects both independent nested errors.

Review caught and corrected an enclosing-decision suppression bug: a clean branch's
`E_FAULT_RESOURCE` must not disappear merely because another branch is uncertain.
The new regression and CLI before/after comparison verify both the independent type
error and that clean-branch fault obligation. Suppression applies only to the leaf
statement whose own selected domain is uncertain. The first full-suite run was
stopped for this correction; final verification uses the corrected source.

`playground/path_diagnostics.tellur` is a runnable editing exercise with a flat path
and a nested group. The same broken source reports one initializer error with 0.73
and both with 0.74. The isolated VS Code checkpoint verifies both precise expression
ranges, independent partial repairs, unavailable inferred hover while broken,
a fresh changed type after repair, and native Build and Run of both paths.

Final corrected-source validation passed **1,060 Rust tests across 75 nonempty
targets** and **42 Node editor tests**. The manual checked **76 accepted examples,
30 expected diagnostics, 15 context fragments and 263 local links**. The full suite
includes native original/expanded parity, retained-resource fault cleanup and the
HTTP source-fault 500/next-request test. Formatting and whitespace checks passed.
All six local compiler/server copies match the final 0.74.0 release artifacts;
all nine packaged and installed extension payloads were verified. The final isolated
VS Code run used those rebuilt binaries. Existing user windows were not reloaded.


## 0.75 reusable JSON file library and explicit Rust modules

P5b.2a extracts `library/json_file.tellur`, `library/host/text_file.rs` and
`library/host/json_string_map.rs` from the settings application. The source controls
preserve missing data, invalid JSON, read failure, encoding failure, write preparation
failure, uncertain replacement and size limits. The settings application's five
public entry declarations are unchanged; its snapshot now includes the imported
library and its contributor baseline includes the new internal call layer. Temporary
file names use the neutral `.tellur-file-` prefix. Defaults, key policy, HTTP decisions
and process statuses remain in their existing callers.

I1b.2a supplies explicit `rust.modules`: named manifest-relative source files embedded
as child modules of the root Rust adapter, with generated signature checks and one
locked dependency graph. Native generation reads every listed source; ordinary
checking/LSP does not read host code. Duplicate/invalid names, empty paths, lockfile
paths and unresolved native paths are rejected. Native generation identifies missing
modules by name/path. Embedded sources may use sibling modules and re-export functions
or resource types; unlisted file discovery and package resolution are not added.

`examples/json_file` is a second executable using these same files without the settings
service or HTTP dependency. Library regressions exercise every declared outcome,
original/expanded parity, exact UTF-8 byte limits, duplicate/non-string/trailing JSON,
invalid UTF-8, preservation on size failure, real replacement and uncertain commit
with temporary cleanup. Adapter tests cover manifest-relative Unicode/space paths,
raw module identifiers, inline documentation/trailing comments, sibling calls,
source refresh and rejected function-type mismatches. Editor tests retain checked
source diagnostics without any Rust files present.

The first focused run exposed a test fixture that copied only settings source files;
it now also copies the newly imported library and adjusts that fixture's import.
No language rule was changed to accommodate the fixture. Full release evidence is
recorded after final validation below.

The isolated editor harness initially expected a callee signature at the imported
call. Existing hover there explains selected histories; the final harness verifies
that supported behavior, unsaved argument diagnostics/repair and native save/reload.
Call-site contract hover is now tracked separately as P4d, prompted by this example.

Final validation passed **1,066 Rust tests across 76 nonempty targets**, including
all three manual checks, and **42 Node editor tests**. The complete CLI/HTTP suite
passed against the extracted mechanisms, including source-fault 500/next-request
handling. The independent release executable also verified missing-file status 3,
Unicode save/reload status 0, invalid-file status 1 and uncertain-replacement status 4.
Rust/Tellur formatting and whitespace checks passed. All six compiler/server copies
match the final release artifacts; all nine packaged and installed extension payloads
were verified. The isolated VS Code run passed selected-history hover, unsaved argument
error/repair and native Unicode save/reload through the shared modules. Existing user
windows were not reloaded.

## 0.76 checked call-site contracts

P4d adds contracts at authored callee names while retaining selected-history feedback.
The AST walk covers calls inside expressions, captures, stored/inline adapters,
statement arms, groups, scopes and loops. Ordinary local/imported controls, concrete
explicit generic specializations and manifest primitives resolve against a successful
current check. The displayed declaration uses checked types and outcomes, declared
public origins, inferred private return lifetimes and fault permission. Public calls
do not expose implementation field paths. Named outcome contracts list alternatives;
manifest primitives include borrowing/consuming modes, effects, contingency admission,
immediate/blocking Rust execution and pure-expression behavior.

The module loader retains nonserialized import metadata for editor lookup. Definition
text is captured by the same loader that checked it; hover does not re-read a different
disk version. Caller aliases and whole-identifier substitutions preserve nominal names
without prefix collisions or internal module identifiers. Transitive types with no
caller import use explicitly explanatory file-qualified identities. Serialized source
maps and executable representations are unchanged.

Ten added LSP regressions cover fresh unsaved dependencies and repairs, UTF-16 ranges,
parenthesized calls, comments/arguments, inferred faults and borrowed results, concrete
generics, private layout protection, public grouped results, manifest modes/effects,
nested call sites, transitive nominal identities and untitled documents. Trait dispatch,
constructors, named boundaries and abstract generic-body calls retain their existing
selection hover; no generic expression-type hover or new language syntax is claimed.

Final affected-suite validation passed **232 distinct Rust tests across eight targets**,
including all **113 LSP tests** and all three manual checks, plus **42 Node tests**.
Module/native parity, diagnostic recovery, contract snapshots, absorption and explanation
checks passed. The unchanged execution implementation retains the 0.75 full-suite
baseline rather than claiming a repeated full native/HTTP run for this editor change.
The isolated VS Code test passed combined contract/selection hover, unsaved argument
failure and repair, fresh imported parameter renaming and restoration, and native
Unicode save/reload through shared Rust modules. All six staged binaries and all nine
packaged/installed extension payloads were verified at 0.76.0. Existing user windows
were not reloaded. Rust formatting and whitespace checks passed.

## P5b.2b — Shared loopback transport on compiler 0.76

`library/loopback_http.tellur` exposes seven public transport controls with named
closed outcomes. It forwards the existing retained-resource primitives without
absorbing framing, limit, timeout, disconnect or I/O alternatives. Listen/accept
return owned handles; receive/send borrow them; close/release consume them.
Only close and fixed failure reply are admitted as contingent operations. Boundaries,
request budgets, route policy and restart points remain in the application.

`library/host/loopback_http.rs` extracts the settings host's synchronous mechanisms
through existing explicit Rust modules, re-exporting the same resource types and
function signatures. The fixed framing, byte, deadline, response and bounded-close
profile is preserved. A new repeat-receive guard makes its one-request protocol
explicit for independent callers: a second receive reports the existing `failed`
alternative before touching input or HEAD state. The prior application already
received once. Response-start tracking continues to prevent duplicate transmission.

The settings server now calls these public controls. Its settings-domain contract
and contributor snapshots are byte-for-byte unchanged. `examples/http_hello` is an
independent consumer without file or JSON mechanisms, using its own routes and the
same explicit retained-connection contingency. The supplied source has no fault
endpoint; tests inject faults into separate source loaders.

Five library regressions cover scripted failure alternatives and original/expanded
agreement, cleanup plus independent read/write errors, consuming completion and
contingency permissions, native Unicode/HEAD/method/Origin handling, idle/configuration
statuses, 500/next-request recovery, no second response after writing, and repeat-read
rejection while preserving the first result. The maintained settings CLI and HTTP
suites continue to cover persistence, protocol limits, deadlines, disconnections,
O0/O2 scenario parity and source-fault cleanup through the extracted implementation.

Final affected validation passed **142 distinct Rust tests across six targets**,
including 113 LSP, 12 formatting, three manual, three settings CLI, six settings HTTP
and five library tests. The isolated VS Code run used the final shared sources and
passed imported contingent-call hover, listener obligation error/repair, native
Unicode HTTP greeting and bounded shutdown. The documented standalone release build
also passed idle completion and listener release. Rust/Tellur formatting and whitespace
checks passed. Socket tests used approved local-network execution after sandboxed
binding was denied. Compiler/server and production extension code are unchanged,
so the existing 0.76 binaries, installed extension and their release evidence remain
current; this checkpoint does not publish a duplicate compiler/VSIX release.

## 0.77 — Bounded control-declaration diagnostics

P2c collects independent failures within public contract-shape checking and concrete
control-signature preparation. Each declaration contributes at most its first error
in that phase. The 32-error budget and conservative limit notice apply before body
checking as well. Failed phases discard every successful sibling result; no partial
signature environment or checked program reaches downstream checking or hover.
All authored operation names are reserved before signature validation, preserving
duplicate-name checks even when an earlier declaration is invalid.

Shared parsing, module/name resolution, type layouts, named-contract binding, trait
preparation, generic expansion and graph cycles keep their existing barriers. The
first failed phase prevents dependent phases from running. Valid-source typing,
ownership, origins, paths and fault rules are unchanged. Single-error compilation
APIs continue to return the first diagnostic from the failed phase.

Five diagnostic regressions cover independent public shapes, concrete signatures and
borrow origins, phase barriers during repair, both budgets, imported source points,
generic public declarations and shared-layout failure. Two LSP regressions check
unsaved root/imported declarations, independent repair and suppression/restoration
of call-site hover. The new `playground/declaration_diagnostics.tellur` provides a
valid example with two independently editable declarations and a native result of 36.

Final validation passed **361 Rust tests across 18 targets**, including all 28
diagnostic tests, 115 LSP tests and three manual checks, plus **42 Node tests**.
The broader checks include concrete/generic traits, generic data/controls, module
privacy, constructor and borrowed-result contracts, lexical boundaries and source
faults, with their existing native expansion/parity scenarios. The first isolated
editor run caught a missing process-status mapping in the example; its final named
contract now declares done/overflow statuses explicitly. The final run passed both
declaration batches, partial/full repair, fresh hover and native result 36. All six
compiler/server copies and all nine packaged/installed extension payloads were
verified at 0.77.0. Existing windows were not reloaded. Formatting and whitespace
checks passed; unchanged execution/library evidence remains valid outside this
affected-suite run.

## 0.78 — Named completion binding diagnostics

P2d collects independent errors while binding named completion contracts to their
controls. Wrong origin-argument counts, including arguments on zero-group schemas,
and arguments on bare or manifest-only completions share the existing 32-error
budget. Each binding reads validated shared schemas and mutates only its own control.
A failed batch returns before any bound declaration reaches signature preparation
or body checking. Shared schemas, layouts and trait checks keep their earlier
barriers; later source/lifetime validation keeps its existing phase. Valid-source
ownership, origin inference, path selection and fault behavior are unchanged.

Six diagnostic regressions cover independent bindings, partial repair into later
signature/body errors, nullary schemas and bare exits, invalid shared schemas,
CLI/library budgets, imported source points and unused generic definitions. Two LSP
regressions cover unsaved root bindings and dependency schema edits, independent
repair, original defining-file locations and fresh call/auto hover. The runnable
`playground/contract_binding_diagnostics.tellur` demonstrates two borrowed views;
its explicit `@exit 0 done` contract also makes it an executable entry.

Final validation passed **375 Rust tests across 17 affected targets**, including
all 34 diagnostic tests, 22 statement/path recovery tests, 117 LSP tests and three
manual checks, plus **42 Node tests**. Named outcomes, nested/public origins,
generic data/controls, traits and modules retain their native expansion/parity
scenarios; the public nested-reference cases also compare optimization levels.
The first isolated editor run caught the example's missing executable status;
after adding the named status contract, the final run passed both diagnostics,
partial/full repair, fresh call and `auto` hover, and both native borrowed-view
outputs. Six binary copies and nine packaged/installed payloads were verified at
0.78.0. Existing windows were not reloaded. Formatting and whitespace checks passed;
unchanged runtime/library evidence is reused outside this affected-suite run.

## 0.79 — Constructor call contracts in hover

P4d.1 extends checked call-site hover to declared constructors. The callee range
shows the constructed type and specialized input types, authored parameter names,
ordered result-origin groups and checked fault/contingency permission. Imported
aliases and exact unsaved source snapshots use the existing call-hover machinery.
Private implementation fields and internal constructor/completion names are not
displayed. The existing selected-history explanation remains below the contract.
Raw brace construction, trait dispatch, named boundaries and abstract generic-body
calls retain their existing feedback. Compiler and runtime semantics are unchanged.

Four LSP regressions cover borrowed origin contracts and precise ranges,
private imported generic constructors, unsaved dependency edits and repair, nested
call selection, inferred faults, untitled documents and conservative abstract-call
fallback. The existing `playground/borrowed_construction.tellur` demonstrates both
shared and exclusive constructor call contracts and independent source release.

Final validation passed **213 Rust tests across seven affected targets**, including
all 121 LSP tests and three manual checks, plus **42 Node tests**. Constructor,
borrowed-constructor, generic-data and module suites retain their native parity
coverage. Isolated VS Code confirmed both constructor contracts, ordered groups,
invalid-source suppression, fresh parameter/origin names after repair, and all four
native output lines. Its first run expected `E_TYPE` for a Boolean reference-field
initializer; the compiler correctly reports `E_BORROW`, and the corrected harness
also checks the exact `true` range. Six binary copies and nine packaged/installed
payloads were verified at 0.79.0. Formatting and whitespace checks passed; existing
windows were not reloaded. The 0.78 diagnostic/contract evidence remains valid
outside this editor change; no unchanged full-suite rerun is claimed.

## 0.80 — Concrete trait call contracts in hover

P4d.2 retains the trait-side signatures already validated by conformance checking,
along with normalized associated types, as source-tooling metadata. Hover at an
explicit concrete trait call shows declared parameter/payload names, outcome order,
public origin anchors and fault/contingency permission. Implementation binding
names and private body projections do not replace that contract. Named source and
manifest outcomes list their alternatives. Concrete associated-type selections in
call targets and type arguments resolve against the same checked snapshot.

The lookup preserves lexical generic scope: a type parameter cannot accidentally
select a same-named real type's implementation or constructor. Abstract calls,
implementation-local `self`/unqualified associated aliases, and named boundaries
retain existing selected-history feedback. Result types are formatted before origin
brackets are added, preserving lowercase builtin spellings inside generic payloads.
Executable IR, certificates, typing, ownership and fault semantics are unchanged.

Eight LSP regressions cover independent trait/implementation names and outcome order,
borrowed and nested named origins, imported aliases and unsaved repairs, declared
permissions, manifest-only completions, untitled/abstract calls, generic shadowing,
and concrete associated-type selections across trait, constructor and ordinary calls.
The existing `playground/traits.tellur` provides the runnable editor example.

Final validation passed **292 Rust tests across 12 affected targets**, including
all 129 LSP tests and three manual checks, plus **42 Node tests**. Trait conformance,
aggregate origins, generic controls, modules, public contracts, snapshots, faults,
boundaries and certificates retain their native/expansion coverage. The manual checks
were repeated only after documenting the final lookup boundary and are counted once.
Isolated VS Code passed trait-side parameter/payload names, associated types,
implementation ownership diagnostics, invalid-source suppression, fresh repair and
native output. Six compiler/server copies and nine packaged/installed payloads were
verified at 0.80.0. Existing windows were not reloaded. Formatting and whitespace
checks passed; earlier unaffected diagnostic/runtime/library evidence is reused.

## 0.81 — Lexical call types and named result contracts

P4d.3 resolves implementation-local `self` and associated-type aliases from the
same checked snapshot used for call hover. Lexical aliases take precedence over
same-named module types, including sibling aliases and imported traits/constructors.
Ordinary generic helpers retain named result contracts after concrete associated
projections are normalized in their defining module. Validated source outcome schemas
restore declared origin mappings on alternatives. Private inferred mappings remain
checked lifetime notes; hover does not invent public group arguments.

Six LSP regressions cover lexical precedence, untitled documents, sibling aliases,
imported declarations, fresh unsaved dependency changes and repair, associated named
results, nested origin mappings and private inference. The runnable
`playground/trait_aliases.tellur` exercises lexical trait dispatch and constructor
types. Abstract generic calls and named boundaries retain selected-history feedback.
The additional checked schemas are source-tooling metadata; executable IR,
certificates, typing, ownership and fault semantics are unchanged.

Final validation passed **298 Rust tests across 12 affected targets**, including
all 135 LSP tests and three manual checks, plus **42 Node tests**. Traits, associated
origins, generic controls/data, modules, public contracts, snapshots and certificates
retain their native/expansion coverage. The initial focused sibling-alias fixture
attempted an explicit move into a copy-valued argument; the corrected fixture uses
two owned types and all six focused tests pass. Isolated VS Code confirmed lexical
precedence, invalid-source suppression, fresh repair, named nested origins and native
example output. Six compiler/server copies and nine packaged/installed payloads were
verified at 0.81.0. Existing windows were not reloaded. Formatting and whitespace
checks passed; earlier unaffected runtime/library/diagnostic evidence is reused.

## 0.82 — Named boundary contracts at supervision sites

P4d.4 presents the checked boundary contract at the callee in `supervise` statements:
owned inputs, authored retention order, and permission for faults to escape the
contingency. The escape marker comes from the checked handler, not the ordinary
work body's internal faults. Hover explains outcome-free completion and abandoned
continuations. Imported aliases, private layout protection and unsaved snapshot
freshness use the existing checked-call machinery. No syntax, compiler metadata,
ownership, fault handling or executable semantics change.

Four LSP regressions cover handled versus escaping faults, explicit public and
inferred private permissions, empty retention, exact callee ranges, imported owned
types, changed retention order, invalidation/repair and nested constructor calls.
The existing `playground/contingency.tellur` remains the runnable example.

Final validation passed **213 unique Rust tests across five affected targets**,
including all 139 LSP cases, named/lexical boundary and module suites, and three
manual checks; **42 Node tests** passed. The initial nested-call fixture first
selected a declaration, then expected `E_BOUNDARY` where ordinary call disposition
checking reports `E_DISPOSITION`. The corrected case passed; successful cases were
reused and counted once. Isolated VS Code passed retained-name and escape-permission
edits, invalid-source suppression, repair, and native execution with one handled
fault followed by the next request. Six binaries and nine packaged/installed
payloads were verified at 0.82.0. Existing windows were not reloaded. Formatting
and whitespace checks passed; unaffected 0.81 trait/origin and earlier runtime,
diagnostic and library evidence remains valid.

## 0.83 — Diagnostic recovery in named boundary bodies

P2e extends fresh statement replay to independent leaf errors in named boundary
work and contingency bodies. Disjoint histories and ordinary nested scopes retain
their checks. A failed handler suppresses its dependent work body, and failed
boundaries suppress supervisors. The existing 32-error budget remains shared;
replays publish no partial executable program, inferred hover or public contract.

Fault admission inspects all retained and live owners. Its retention/fault-resource
failures stall uncertain histories rather than producing misleading secondary
diagnostics; clean-path failures remain real barriers. Temporary call scopes now
retire even after a failed call, preserving the scope of later disjoint checks.
Lexical boundaries and inline adapters remain first-error barriers, and omissions
still stall histories at administrative dispositions or new control-flow structure.
Valid-program typing, resource completion, fault behavior and runtime are unchanged.

Six diagnostic regressions cover dependency suppression, paths/scopes, ownership
witnesses, retained-owner and resource barriers, fault-only calls, contingency
permissions/terminals and the global budget. Two LSP regressions cover unsaved
handler/work repairs, imported disjoint-path errors, precise ranges and fresh hover.
The initial terminal test expected `E_OUTCOME`; the actual undeclared named-handler
exit diagnostic is `E_EXIT`, and the corrected test passes.

Final validation passed **359 Rust tests across 12 affected targets**, including
all 28 recovery tests, 34 structured diagnostic tests, 141 LSP tests and three
manual checks, plus **42 Node tests**. Named/lexical boundaries, source faults,
constructors, ownership, loops, modules and certificates retain native/expansion
coverage. Isolated VS Code confirmed two handler errors before dependent work
errors, exact highlighted tokens, partial/full repairs, stale-hover suppression and
native supervision with one handled fault followed by the next request. Six binary
copies and nine packaged/installed payloads were verified at 0.83.0. Existing windows
were not reloaded. Formatting and whitespace checks passed; earlier unaffected
trait/origin and application/library evidence is reused.

## 0.84 — Diagnostic recovery inside lexical boundary work

P2f collects independent leaf errors inside lexical `fault boundary` work. Disjoint
work paths, ordinary scopes, loop bodies and nested boundary work use fresh replay.
If work becomes uncertain, the checker restores its entry scopes, live-history state
and outer code, then stalls the whole authored boundary domain before checking the
handler or outer continuation. Independent outer histories remain checkable; parent
entry selectors and joins keep their original full meaning. This diagnostic-only
retirement cannot publish a partial program or alter runtime fault handling.

Six recovery regressions cover handler/continuation suppression, disjoint outer
histories, parent entry joins, nested boundaries, retained-owner admission, capture
errors, supervisor state, loops and the shared 32-error budget. Two LSP regressions
cover staged work/handler repairs, exact source lines, imported locations and fresh
inferred/call hover. Initial fixtures attempted to reuse an existing path atom and
expected `E_NAME` for an invalid capture; the fixtures now respect fresh path names
and the actual `E_BOUNDARY_CAPTURE` diagnostic. Lexical handlers, capture/setup
failures and inline adapters retain their conservative first-error limits.

Final validation passed **367 Rust tests across 12 affected targets**, including
all 34 recovery tests, 34 structured diagnostic tests, 143 LSP tests and three manual
checks, plus **42 Node tests**. Boundary, source-fault, ownership, loop, constructor,
module and certificate suites retain native/expansion coverage. Isolated VS Code
confirmed two work errors before first-error handler diagnostics, exact token ranges,
partial/full repair, restored `ref string` inference hover and native recovery after
one handled fault. Six compiler/server copies and nine packaged/installed payloads
were verified at 0.84.0. Existing windows were not reloaded. Formatting and whitespace
checks passed; unaffected trait/origin and application/library evidence is reused.

## 0.85 — Diagnostic recovery inside lexical contingency

P2g collects independent leaf errors in lexical handlers after their work checks
cleanly. An uncertain handler restores normal-work parent scopes/history/code, clears
the temporary handler context and stalls the entire boundary domain before coverage
or retirement checks. Disjoint outer paths retain their state and checking; parent
entry selectors preserve full joins. Nested handler uncertainty propagates through
enclosing work, preventing an unsupported continuation or handover.

Six recovery regressions cover work dependency, ordinary scopes, paths/loops,
normal-parent restoration, nested handlers, retained-resource fault admission,
contingency-only calls, capture access, immutable fault reports, clean terminal
errors and the shared 32-error budget. Two LSP regressions cover handler repairs,
changing inferred types, imported disjoint errors and failed-caller suppression.
Existing staged work/handler coverage now expects independent handler diagnostics.
Capture/setup, administrative-terminal and inline-adapter barriers remain explicit;
valid-program typing, fault handling, resource completion and runtime are unchanged.

Final validation passed **375 Rust tests across 12 affected targets**, including
all 40 recovery tests, 34 structured diagnostic tests, 145 LSP tests and three manual
checks, plus **42 Node tests**. Native/expansion evidence remains covered by boundary,
fault, constructor, ownership, loop, module and certificate suites. Isolated VS Code
passed work-before-handler batches, exact token ranges, partial/full repair, fresh
handler inference from `ref string` to `u32`, and native recovery after one handled
fault. Six binary copies and nine packaged/installed payloads were verified at
0.85.0. Existing windows were not reloaded. Formatting and whitespace checks passed;
unaffected trait/origin and application/library evidence is reused.

## 0.86 — Diagnostic recovery across explicit exit-adapter arms

P2h reuses terminal replay beneath `exit delta` when the control's entire result
shape is explicit. Independent arms can report payload, arity and exit-name errors
together, including nested, named and generic adapters. Fresh replay preserves
borrow and resource state; a clean arm still reports real completion obligations.
Stored/captured adapters, inferred result shapes, decision headers and fault-context
terminals retain conservative barriers. No partial checked program is published.

Six recovery regressions cover independent and nested arms, full selected-domain
joins, owned and borrowed payloads, real resource errors, inferred/stored barriers,
uncertain entry, fault contexts and the shared 32-error budget. Two LSP regressions
cover independent repairs, precise source locations, unsaved imported nested arms
and checked call hover returning only after all errors are repaired.

Final validation passed **370 Rust tests across 12 affected targets**, including
all 46 recovery tests, 34 structured diagnostic tests, 147 LSP tests and three manual
checks, plus **42 Node tests**. Adapter, stored-adapter, named-outcome, outcome-origin,
generic-control, module and certificate suites retain native/expansion coverage.
Isolated VS Code verified two arm errors at their exact tokens, partial/full repair,
restored call hover and native execution of `playground/adapters.tellur` returning
42. Six binary copies and nine packaged/installed payloads were verified at 0.86.0.
Existing windows were not reloaded. Formatting and whitespace checks passed;
unaffected boundary, trait and application/library evidence is reused.

## 0.87 — Diagnostic recovery across explicitly typed stored adapters

P2i collects independent payload errors under an explicit stored outcome type.
An uncertain binding restores its parent scopes, history and instruction list,
then stalls its full selected domain without defining a partial result. Nested
arms and disjoint outer paths retain their original selection. Clean arms still
check resource completion and prohibit references to arm-local storage. Inferred
sinks, captures, headers and fault-context adapters retain conservative barriers.

Six recovery regressions cover result/continuation suppression, nested arms,
scope and loop retirement, prior terminated histories, partial resource moves,
returned references, clean resource/lifetime errors, generic definitions and the
shared 32-error budget. Two LSP regressions cover partial/full repairs, suppressed
result-type hover, fresh call contracts and unsaved imported nested-arm locations.
A new scope fixture initially included an extra colon before `over &:`; correcting
that fixture resolved its parser error without changing the implementation.

Final validation passed **407 Rust tests across 14 affected targets**, including
all 52 recovery tests, 34 structured diagnostic tests, 149 LSP tests and three manual
checks, plus **42 Node tests**. The three suites completed before the fixture failure
were reused; the failed target and remaining targets then passed. Adapter, stored
adapter, named-outcome, outcome-origin, generic-control, loop, boundary, module and
certificate suites retain native/expansion coverage. Isolated VS Code confirmed
exact error tokens, partial/full repair, restored call hover and native execution
of `playground/stored_adapters.tellur`. Six binary copies and nine packaged/installed
payloads were verified at 0.87.0. Existing windows were not reloaded. Formatting and
whitespace checks passed; unaffected trait and application/library evidence is reused.

## 0.88 — Symbolic ordinary generic call contracts

P4d.5 shows ordinary generic helper contracts inside generic bodies using their
universally checked definitions. Source-only metadata retains the normalized
interface, named result origins, checked private return lifetimes and bounded
anonymous payload descriptions before abstract layouts disappear. Lexical type
parameters stay protected through namespace and associated-type lookup; a concrete
type with the same name cannot supply an unrelated witness. Authored payload names
are preserved even when they match type parameter names. The shared lifetime
renderer retains existing concrete hover behavior.

Eight new LSP regressions cover bounds, exact callee ranges, unused definitions,
distinct and nested arguments, private borrow evidence and fault permissions,
imported named origins, associated-type shadowing, anonymous outcomes, namespace
collisions and fresh unsaved dependencies. Existing untitled-call coverage now
expects a symbolic contract. Initial focused expectations were corrected for the
formatter's comma spacing and the inferred `value_0` payload name; the final focused
run and affected suites passed. Abstract trait, constructor and boundary calls
remain separate presentation work. Executable IR and certificate formats are unchanged.

Final validation passed **307 Rust tests across 14 affected targets**, including
all 157 LSP tests and three manual checks, plus **42 Node tests**. Generic controls
and data, trait bounds and origins, public contracts, constructors, snapshots and
certificates retain native/expansion coverage. Isolated VS Code verified symbolic
named contracts, checked borrow origins, failed-check invalidation, repaired/renamed
parameter display and native execution emitting `Ada`. Six binary copies and nine
packaged/installed payloads were verified at 0.88.0. Existing windows were not reloaded.
Formatting and whitespace checks passed; unchanged diagnostic, runtime and library
evidence is reused.

## 0.89 — Symbolic trait-call contracts

P4d.6 retains checked abstract trait interfaces per generic definition and its
distinct seeded inputs. Hover shows the trait's parameter/payload names, associated
projections, named alternatives, direct and nested origin mappings, and declared
fault/contingency permissions. It cannot select an unrelated concrete witness or
mix equal parameter spellings across definitions. Both concrete and symbolic trait
calls share the interface renderer. A possible contract name on a parsed lone
nullary exit is now resolved before presentation, keeping ordinary labels distinct
from actual named outcomes.

Six new LSP regressions cover unused definitions, concrete-name shadowing, nested
named origins, multiple bounds/inputs, fault-only and contingent methods, imported
contracts, unsaved invalidation/repair and bound removal. Existing abstract-negative
cases now require symbolic contracts and continued separation from concrete types.
One focused expectation used an extra closing apostrophe in a group selector and
was corrected to the existing syntax. Abstract constructor and boundary calls
remain separate presentation work; typing, ownership and runtime behavior are unchanged.

Final validation passed **313 Rust tests across 14 affected targets**, including
all 163 LSP tests and three manual checks, plus **42 Node tests**. Eight affected
targets completed before two old absent-hover assertions failed; those passes were
reused, then the corrected LSP suite and remaining targets passed. Generic data and
controls, trait bounds/origins, public contracts, constructors, snapshots and
certificates retain native/expansion coverage. Isolated VS Code confirmed symbolic
associated types and origins, invalidation/repair, changed trait binding names and
native execution of `playground/borrowed_traits.tellur`. Six binary copies and nine
packaged/installed payloads were verified at 0.89.0. Existing windows were not reloaded.
Formatting and whitespace checks passed; unchanged diagnostic, runtime and library
evidence is reused.

## 0.90 — Symbolic constructor contracts

P4d.7 uses the universally checked constructor template for calls inside generic
controls and constructors. It keeps abstract, nested and associated-type arguments,
shared/exclusive modes, authored parameter names, result-origin mappings, ordered
group names and checked fault/contingency permission. Imported aliases and unsaved
definitions come from the same successful check. No private layout, synthetic
completion label or convenient concrete witness is exposed. Existing concrete
constructor presentation shares the source lookup and renderer.

Five new LSP tests cover independent origins, imported privacy and edits, fault and
contingency permissions, nested associated types and lexical constructor parameters.
Two older abstract-absence assertions now require symbolic contracts. The initial
permission probe incorrectly expected an unannotated private constructor to lose
its inferred contingency admission; the final test uses an explicit public contract.
Named boundaries have no generic parameter syntax, so the roadmap's abstract
boundary-hover entry was removed rather than adding a new language feature.

Final validation passed **263 Rust tests across nine affected targets**, including
all 168 LSP tests and three manual checks, plus **42 Node tests**. The first run
passed six targets before two existing LSP fixtures failed due to an accidental
test-only rename. Those fixtures were restored; the final three targets passed,
and earlier unaffected passes are retained in the combined evidence. No compiler
changes were needed after the release build.

Isolated VS Code verified symbolic inputs, borrowed result groups, failed-check
suppression, fresh parameter names, repair and native execution emitting
`Hello through generic construction`. Six binary copies and nine packaged/installed
payloads were verified at 0.90.0. Existing windows were not reloaded. Formatting and
whitespace checks passed. Executable IR, certificates, ownership and fault semantics
are unchanged; earlier diagnostic/runtime/library evidence is reused.

## 0.91 — Constructor-call preparation diagnostics

P2j stages constructor-call rewriting per independent body using the complete
constructor and visibility tables. Inaccessible calls report one `E_PRIVATE` per
prepared body under the shared 32-error budget. All staged bodies are committed
only after the phase succeeds. A failed phase blocks generic expansion, concrete
signature checks, body diagnostics and checked hover. Earlier module resolution,
constructor-definition preparation and trait lowering retain their existing barriers.

Five CLI/batch tests cover source points with Unicode/CRLF, same-body successive
errors, unused generic controls, constructors, methods, named and lexical isolation,
transitive imported locations, first-error compatibility, budget and phase barriers.
Two LSP tests cover unsaved constructor visibility changes, independent repairs,
imported related locations and fresh inferred/call contracts. The runnable
`playground/constructor_access` example keeps implementation fields private.

Final validation passed **428 Rust tests across 14 affected targets**, including
all 170 LSP tests, 39 diagnostic tests, 52 statement-recovery tests and three manual
checks, plus **42 Node tests**. Constructor, module, generic, trait, boundary, public
contract, snapshot and certificate checks preserve native and expansion behavior.
Focused checks and the broader run passed without test or implementation corrections.

Isolated VS Code verified two inaccessible constructors, independent unsaved
visibility repairs, no stale hover and native execution emitting `First constructor`
and `Second constructor`. Six binaries and nine packaged/installed payloads match
release 0.91.0. Existing windows were not reloaded. Formatting and whitespace checks
passed; unchanged runtime/library evidence is reused. Executable formats and the
language's ownership, origin and fault rules are unchanged.

## 0.92 — Generic-bound preparation diagnostics

P2k extracts the existing pure requirement checks into a bounded declaration phase
between public shape checking and trait lowering. Independent controls report unknown
capability/trait names, duplicate resolved requirements and private traits in public
contracts together. Checks retain one error per control, the 32-error budget, unused
definition checking, original locations and first-error API compatibility. No partial
witness state is published; shared trait/schema and later checks retain their barriers.

Five CLI/batch tests cover mixed unknown/private bounds, alias duplicates with
Unicode/CRLF sources, transitive imports, independent repairs, first-error behavior,
phase isolation, multiple invalid parameters and the budget. Two LSP tests cover
untitled unknown requirements, imported private traits, related locations and fresh
inferred/call hover after independent unsaved repairs. The editor/native probe reuses
`playground/trait_bounds.tellur` with two public generic interfaces in its isolated fixture.

Final validation passed **368 Rust tests across 12 affected targets**, including all
172 LSP tests, 44 diagnostic tests and three manual checks, plus **42 Node tests**.
Generic controls/data, trait bounds, aggregate origins, modules, public contracts,
snapshots and certificates retain native and expansion evidence. All focused and
broader checks passed without implementation or fixture corrections.

Isolated VS Code verified two private-bound errors, independent repairs, no stale
hover and native execution emitting `Hello through a generic trait`. Six binaries
and nine packaged/installed payloads match release 0.92.0. Existing windows were not
reloaded. Formatting and whitespace checks passed; unchanged constructor, statement
recovery and runtime/library evidence is reused. The accepted bounds, ownership,
origin and fault semantics and executable formats are unchanged.

## 0.93 — Inferred stored-adapter diagnostics

P2l reuses terminal replay for inferred stored payloads, with sink-local uncertain
result labels. Omitting a construction or forwarding result quarantines that label;
later occurrences are skipped before their contextual typing can invent a secondary
error. Distinct labels retain independent checks. Any uncertain sink restores its
parent state, discards writes and blocks its whole binding domain before publishing
a value or inferred outcome type. Inferred exit contracts, decision headers and
fault-context adapters retain their existing barriers.

Seven recovery tests cover distinct/same labels, contextual empty collections,
clean shape consistency, implicit forwarding, nested/disjoint domains, sink isolation,
moves, loans, resource/lifetime retirement, scopes, loops, phase barriers, generic
bodies and the budget. Two LSP tests cover independent repairs, newly exposed
same-label errors, imported locations and no partial hover. One CLI test covers
generic imported ranges with Unicode/CRLF and first-error compatibility.
Initial probes used `missing`, a valid nullary constructor in the playground manifest;
fixtures now use unambiguously undefined names. A LSP fixture using `concat` was
updated to load its playground manifest. Neither issue required a production fix.

Final validation passed **428 Rust tests across 14 affected targets**, including all
174 LSP tests, 45 diagnostic tests, 59 recovery tests and three manual checks, plus
**42 Node tests**. Adapter, outcome, origin, generic, module, boundary and certificate
checks retain valid-program and native execution evidence.

Isolated VS Code verified independent inferred payload errors, exact expression
ranges, partial/full repair, absence of partial inferred hover and native execution
returning `hello from a stored adapter`. An initial isolated launch omitted the
extension-development flag; it was stopped and corrected before testing. Six binary
copies and nine packaged/installed payloads match release 0.93.0. Existing windows
were not reloaded. Formatting and whitespace checks passed; earlier unrelated
preparation, trait and library evidence remains valid. Executable formats and the
accepted ownership, origin and fault semantics are unchanged.

## 0.94 — Inferred exit-adapter diagnostics

P2m removes the inferred-exit wrapper barrier from terminal replay. Each exit payload
already infers independently; clean occurrences still unify arity/types, while
omitted terminals cannot publish contracts or make dependent callers checkable.
Stored-adapter contextual inference retains its distinct label quarantine. No
executable lowering, ownership, origin or fault rules change.

Six recovery tests cover same/distinct labels, selective inference, empty collections,
clean consistency, nested paths, scopes/loops, moves, returned loans, resource and
lifetime failures, forwarding, headers, fault contexts, unused generic definitions
and the shared budget. One CLI test covers imported generic ranges with Unicode/CRLF
and first-error compatibility. Two LSP tests cover unsaved repairs and fresh inferred
and call contracts. The initial LSP probe expected payload detail on a named outcome
binding hover; it now checks the named binding type and the separate call contract.
The implementation did not need correction.

Final validation passed **453 Rust tests across 16 affected targets**, including all
176 LSP tests, 46 diagnostic tests, 65 recovery tests and three manual checks, plus
**42 Node tests**. The affected inference, forwarding, adapter, outcome, origin,
module, generic, boundary and certificate suites preserve valid-program/native
behavior and expansion evidence. The broader run passed without further corrections.

Isolated VS Code verified two inferred exit payload errors, exact ranges, independent
repairs, no partial inferred signature, restored call hover and native execution
returning 42. Six binaries and nine packaged/installed payloads match release 0.94.0.
Existing windows were not reloaded. Formatting and whitespace checks passed. Earlier
unrelated trait, preparation and library evidence remains valid; this checkpoint does
not change executable formats or the accepted ownership, origin and fault semantics.

## 0.95 — Trait-declaration diagnostics

P2n separates shared trait identity checking from pure local declaration validation.
After shared origin schemas and identities validate, declarations collect one error
per trait with the existing 32-error budget. Any failure stops before implementation
lookup/coherence, normalization, lowered methods, generics and body checking. Unused
traits retain the same validation as used ones. No trait semantics or executable
lowering are changed.

Five diagnostic tests cover mixed method/member/type errors, public visibility,
borrowed origins, named shapes, transitive imports with Unicode/CRLF, first-error
compatibility, independent repairs, phase barriers and the budget. Two LSP tests cover
unsaved local and imported repairs with fresh inferred/call contracts. An initial
fixture used bare `exits string`, which denotes an exit label; it was corrected to
name a declared record when testing a non-outcome contract. The implementation
required no correction. The isolated editor fixture reuses `playground/traits.tellur`;
its initial ordinary-call heading assertion was corrected to the existing
`Trait control contract` heading before rerunning the editor test.

Final validation passed **388 Rust tests across 13 affected targets**, including all
178 LSP tests, 51 diagnostic tests and three manual checks, plus **42 Node tests**.
Trait, generic, origin, module, public contract, snapshot and certificate suites retain
valid-program/native and expansion evidence. The broader run passed without further
corrections; earlier unaffected statement recovery and runtime/library evidence is reused.

Isolated VS Code verified two invalid declarations, independent repairs, current
trait-call contracts and native execution emitting `Hello through a trait`. Six
binary copies and nine packaged/installed payloads match release 0.95.0. Existing
windows were not reloaded. Formatting and whitespace checks passed. Executable
formats and the accepted trait, ownership, origin and fault semantics are unchanged.

## Settings key-removal follow-up on 0.95

P5b.2c adds `settings.remove` and a `remove` CLI entry using the current compiler,
`map_remove` and the shared JSON-file library. Invalid keys stop before reads; missing
files/keys stop before encoding or writing. A successful replacement reports `removed`;
preparation failures and uncertain replacement retain separate outcomes. Other keys
survive, and removing the last key leaves an empty document. The public baseline adds
only `remove` and `removal_result`; all existing public declarations are preserved.

The policy test checks eleven scripted cases and expanded execution, including exact
remaining-map arguments to encoding and no write on absence. The existing native
application matrix now checks removal at O0/O2 before/after expansion, Unicode data,
missing-file noncreation, repeat deletion, invalid keys, corrupted/oversized input,
last-key removal, process statuses and the same scripted failures. Contract and
contributor baselines include the new public operation. Source formatting was applied;
existing compact dispositions in `main.tellur` were retained after preview.

Final validation passed **45 Rust tests across six affected targets**, covering the
settings CLI/server, shared JSON library, public/contributor snapshots and manual.
Three live HTTP tests initially failed because the sandbox prohibited loopback socket
binding; only those tests were rerun with local network permission, and all passed.
The earlier successful checks are retained. No implementation correction was needed.

The maintained native CLI was rebuilt and passed seven additional temporary-file
commands covering statuses, missing-file noncreation, other-key preservation and
last-key removal. Fresh LSP checking exposes both borrowed inputs and the removal
result contract. Compiler/server/editor release 0.95 remains unchanged and installed;
no new extension package or window reload was needed. Formatting and whitespace
checks passed. Existing HTTP policy, host mechanisms and storage limits are preserved.

## 0.96 — Constructor-definition diagnostics

P2o collects independent constructor preparation failures within one parsed file,
with one error per constructor and the existing 32-error budget. All generated
constructor controls are staged until the file's entire preparation step succeeds.
The module loader preserves individual source locations and first-failing-file
traversal. Parsing still precedes preparation, which still precedes symbol registration
and import traversal; later initialization/type/ownership checks remain separate.
No valid-program constructor, origin, ownership or fault rules change.

Five diagnostic tests cover result-origin arity, partial-self access, unused generic
constructors, forbidden exits/isolation, one error per definition, staged later checks,
transitive imports with Unicode/CRLF, first-error compatibility, independent repairs,
parse/import/resolution barriers and the batch limit. Two LSP tests cover unsaved local
and imported repairs with unavailable partial contracts and restored fresh hover.
The isolated VS Code harness reuses `playground/constructor_access` for two invalid
imported constructors, independent repairs and native execution.

Final validation passed **429 Rust tests across 16 affected targets**, including all
180 LSP tests, 56 diagnostic tests and three manual checks, plus **42 Node tests**.
Constructor, borrowed composition/projection, initializer, generic, trait, module,
boundary, snapshot and certificate suites retain valid-program/native and certified
expansion evidence. Focused, broader and editor checks passed without fixture or
implementation corrections; earlier unrelated recovery/runtime/library evidence is reused.

Isolated VS Code verified original imported locations, independent unsaved repairs,
no partial constructor contract and native execution emitting `First constructor`
and `Second constructor`. Six binary copies and nine packaged/installed payloads
match release 0.96.0. Existing windows were not reloaded. Formatting and whitespace
checks passed. Executable formats and the accepted ownership, origin and fault
semantics are unchanged.

## 0.97 — Written type-annotation diagnostic ranges

P2p stores source type ranges on parsed fields, parameters, payloads and patterns.
Module qualification shifts both endpoints; generic substitution and named payload
reuse retain the original spelling and location. Signature/trait/generic validation,
typed binding mismatches and ordered manifest storage-field validation attach those
ranges without changing error codes, validation order or accepted programs. Existing
child locations take precedence; synthetic fields do not invent source widths.

Five diagnostic tests cover data/variant/outcome fields, signatures, constructors,
unused generic controls/data, traits, legacy name-first types, typed delta patterns,
repeated payload names, line comments, multiline/Unicode/CRLF text, transitive aliases,
private/missing imported members, named payload reuse, batches and first-error APIs.
Two LSP tests cover UTF-16 positions, unsaved multiline/compact constructor annotations,
original related locations and restored hover. Initial probes corrected unsupported
block comments, collision with the manifest's `read` operation, and existing
`E_MANIFEST` classifications. They also exposed the manifest-backed validation path;
ordered source field ranges and zero-location manifest type errors now retain the
authored annotation there. No validation rule or phase was reordered.

Final validation passed **443 Rust tests across 17 affected targets**, including all
182 LSP tests, 61 diagnostic tests and three manual checks, plus **42 Node tests**.
Source data, generic, trait, lifetime, borrowed-constructor, adapter, module, formatting,
contract-snapshot and certificate suites retain valid-program/native and expansion
evidence. The broader run passed without further corrections; unaffected recovery,
application and library evidence is reused.

Isolated VS Code verified exact multiline/compact imported ranges, current unsaved
locations, unavailable stale hover, repair and native execution emitting both
constructor messages. Six binary copies and nine packaged/installed payloads match
release 0.97.0. Existing windows were not reloaded. Formatting and whitespace checks
passed. Executable formats and accepted ownership, origin and fault rules are unchanged.

## 0.98 — Async audit and shared activation identities

E1a audits recursive reference execution, native sequential frames and task/fault
requirements in `docs/async-execution.md`. Spawned-child fault timing is explicitly
pending the author's decision; the implementation does not choose a policy.

Related fault trackers now share one execution-local monotonic allocator. They keep
independent active chains, source-fault flags and causes; overflow cannot wrap or
partially enter an invocation. Completed calls do not accumulate history, and fresh
entry executions restart numbering. The same tracker code is embedded in native
programs. Allocation establishes identity uniqueness only, not scheduling or memory
synchronization of Tellur effects.

Four focused tests cover children created before either joins, interleaved nested
invocations, local failure/frozen trackers, reordered joins, 2,048 completed calls,
fresh roots and exhausted allocation across siblings. Source tasks, suspension,
cancellation and scheduling remain unimplemented; no new grammar is claimed.

Validation: 68 Rust tests passed across eight targets (library unit tests, faults,
source faults, named and lexical boundaries, native frames, native host and manual).
The four new identity tests also run when the shared fault source is compiled in
the native-host harness. Existing sequential native/reference reports, resource
handover, fuel limits and expansion observations retain their regression evidence.
No full-suite rerun is claimed.

The release build and fresh compiler/LSP version/capability handshake passed; six
binary copies and nine packaged/installed payloads match release 0.98.0. Unchanged
frontend/editor behavior reuses the 0.97 Node and isolated VS Code evidence, with
relevant source hashes checked. Existing windows were not reloaded. Formatting and
whitespace checks passed. The next independent E1b step is persistent reference
continuations; the spawned-child fault decision remains pending.

## 0.99 — Persistent reference continuations

E1b replaces recursive reference control calls, loops and named/lexical boundary
traversal with explicit continuation frames. Current instruction positions, caller
facts, borrowed storage, result delivery, retained handover and contingency context
survive a host pause. The same engine drives the existing synchronous APIs; normal
outcomes, administrative settlement/process termination and source faults remain
separate. Native lowering and executable formats are unchanged.

`start_execution(scenario, fuel)` prepares a scripted reference simulation;
`advance(quota)` returns `Paused`, `Complete(result)` once, then `Finished` on every
later call. Zero quota performs no entry validation, instruction or effect. A quota
counts attempted instructions including guard skips; it does not refill fuel.
Return/unwind bookkeeping drains before pausing, and a terminal failure cannot resume.
Dropping this host object abandons a simulation; it is not language cancellation or
resource settlement. No source tasks, readiness scheduler or async syntax is enabled.

New partition tests cover borrowed constructor values and mutation across calls,
named/lexical and nested boundaries, escalation and secondary faults, successful and
failed resource cleanup outcomes, process termination, driver/entry/result errors,
zero quota and interleaved independent sessions. Each finite specimen is compared
across every tested fuel boundary and several instruction partitions; expansion is
included where relevant. An internal test checks bounded live frames, slots and facts
through 1,000 single-instruction pauses and terminal fuel exhaustion. The initial
nested lexical test included an unreachable `exit` after an always-faulting inner
boundary; the checker correctly rejected it and the specimen was corrected.

Validation: 286 Rust tests passed across 24 targets, combining the five affected
fault/boundary/native-frame suites and the new retirement unit with 18 broader
runtime, transformation, manual and stepping targets. Validated regression/unit
runs from before the version-only bump are reused. The final seven-case partition
suite passed; no full-suite rerun is claimed. Native/reference agreement includes
both optimization levels, expansions, storage/borrow behavior and fuel reports.

Release 0.99.0 built successfully. Six compiler/server binary copies, the version and
capability handshake, and nine packaged/installed payloads were verified. Unchanged
frontend/editor code reuses the earlier Node and isolated VS Code evidence; existing
windows were not reloaded. Formatting and whitespace checks passed. Source async
contracts and scoped scheduling remain separate work, with the existing child-fault
question still pending.

## 0.100 — Loop type-annotation locations and cold-task audit

P2q extends authored field type ranges to ordinary/supervised loop parameters. Module
resolution now shifts both endpoints and attaches lookup/visibility failures; generic
data/control preparation and body type validation use the same field-range helper.
Type spellings are never reconstructed from lowered names. The 0.99 CLI probe for an
invalid `vector<string,u32>` loop parameter had null position/range; it now selects
the annotation. Initializer children, `again` arguments and structural-loop errors
keep their existing locations. Checking order, codes, batching and accepted programs
are unchanged.

Four diagnostic tests cover multiline/CRLF/Unicode, legacy and mutable parameters,
lookup/visibility, generic arity and substitution, imported offsets, first-error APIs,
independent-body batches and non-type locations. One LSP test checks imported unsaved
ranges, unavailable stale hover and repair. Initial fixtures were corrected for the
existing `>=` token, E_MANIFEST/E_VISIBILITY codes and borrowing boxed Span metadata;
a later producer-argument probe exposed that the generic-control field visitor did
not attach ranges. Its loop branch now attaches the original field range explicitly.

In parallel, the E1c audit distinguishes argument effects from deferred body effects,
captured owners/loans from eventual result contracts, and cold-task backing storage
from ordinary call temporaries. The author was asked whether cold tasks must be
awaited/transferred or may be explicitly discarded when all captures are droppable.
That choice and the earlier spawned-child fault question remain open. No source
async/task/disposal behavior is implemented by this checkpoint.

Validation: 434 Rust tests passed across ten targets, with the latest affected
four-target run replacing their earlier results and six unchanged targets reusing
valid broad-run evidence. This includes 65 diagnostic and 183 LSP tests, plus
recovery, loop, module, generic data/control, trait-bound, constructor and manual
coverage. All 42 Node tests passed. The unchanged 0.99 execution engine retains
its prior partition and native/fault validation; no full-suite rerun is claimed.

An isolated VS Code profile verified exact multiline and compact imported loop type
ranges, unsaved dependency repair, unavailable stale hover, restored contract hover,
and native execution of the existing counter producing 1 through 5. Six binary copies
and nine packaged/installed payloads match release 0.100.0; compiler/LSP version and
capability checks passed. Existing windows were not reloaded. Formatting and
whitespace checks passed.

## 0.101 — Local type-annotation locations and accepted task policies

P2r retains a local declaration's written type separately from its normalized type.
Ordinary bindings, captures and stored adapters keep that range through module lookup,
import offsets, generic preparation and substitution. Ordinary type validation and
stored adapter target validation select the full annotation. Binding modifiers,
names, trailing comments and initializers are excluded. Generated constructor and
projection bindings carry no invented type coordinates.

Three diagnostic regressions cover multiline/CRLF types with Unicode line comments,
modern and legacy reference prefixes, private imported types, generic data and
producer arity, symbolic type substitution, adapters, body batches and first-error
APIs. Initializer mismatches retain their expression/child ranges; capture
compatibility retains its existing statement point. The new LSP regression covers
unsaved imported type changes, UTF-16 conversion, suppression of stale hover and
restoration after repair. No acceptance rule, validation order or recovery policy
has changed.

The isolated editor test derives an imported counter from `playground/count.tellur`.
It edits the local text type to invalid multiline and compact annotations, verifies
related ranges in the dependency, repairs the document and restores its call hover.
Native Build and Run then emits 1 through 5 and exits `done`. The test uses its own
VS Code profile; existing user windows are not reloaded.

The author also accepted two E1 policies: an escaping spawned-child fault reaches
its owning boundary at the next scheduler safe point, stopping ordinary parent and
sibling work and settling resources before contingency; even a never-started task
must be awaited or transferred, with no ordinary explicit discard. These are design
decisions, not executable task features. The separate synchronous-entry question
(`run` operation versus lexical runtime block) remains open. The
[async audit](async-execution.md) records the accepted rules, pending question and
capture/loan/temporary-storage requirements.

Validation: **528 Rust tests across 18 selected targets**, **42 Node tests**, and the
isolated editor/native check passed. Targets include diagnostics, LSP, recovery,
modules, generic controls/data, traits, construction and borrowed construction,
projections, outcomes, references, formatting, revision and manual checks. This is
not a full-suite claim. Unchanged runtime, fault, native-generator and editor
production sources match the earlier foundation evidence. Six binary copies and
nine packaged/installed payloads match 0.101.0; compiler/LSP version and capability
checks passed. The source backup excludes the protected unrelated C# file.

## 0.102 — Public interface type-annotation locations

P2s retains authored type metadata until the late public-interface visibility check.
Private or producer-indexed types exposed by a public field, control parameter or
explicit result payload select the written annotation instead of only the enclosing
declaration. Public structs, variants and named outcomes retain their field ranges;
generic definitions and imported aliases keep their original spelling and endpoints.
Whole named-contract references and generated associated-type checks without an
authored field annotation keep their existing source points.

Three diagnostic tests cover these interfaces, multiline/CRLF source with Unicode
comments, imported and nested generic types, producer-indexed control parameters,
symbolic generic results and first-error APIs. The phase-order test keeps earlier
body/data validation failures ahead of late visibility checking and keeps public data
before control signatures. Source data referring to a producer outcome still fails
its earlier manifest check; this checkpoint does not move that validation. No source
acceptance, visibility policy, batch behavior or error ordering has changed.

The LSP regression verifies exact UTF-16 related locations for unsaved imported
public field types, no stale inferred hover, and repair. The isolated VS Code test
adds an unused public data declaration to the imported counter derived from
`playground/count.tellur`, changes it to expose a private type in multiline and
compact forms, restores it, and runs the native counter to emit 1 through 5.

Validation: **361 Rust tests across eight selected targets**, **42 Node tests**, and
the isolated editor/native check passed. Constructors retain the passing evidence
from the initial run; the corrected diagnostic fixture and remaining seven targets
use the final run. The initial fixture incorrectly expected a late visibility error
for producer-indexed source data; it now explicitly preserves the earlier manifest
error. This is not a full-suite claim. Runtime, parser, core body checker, native generator and
editor production sources are unchanged from 0.101. Six binary copies and nine
packaged/installed payloads match 0.102.0; compiler/LSP version and capability checks
passed. Existing user windows were not reloaded; the source backup excludes the
protected unrelated C# file.

The existing async entry-syntax question remains unchanged. Both previously accepted
task fault/settlement policies remain recorded; this independent diagnostics slice
does not select a source async spelling or add task execution.

## 0.103 — Atomic reference invocation admission

E1b.1 separates validated invocation arguments from installed execution state.
Preparation validates the whole argument vector, including nested values, and retains
it in an internal owned handoff. It does not reserve frame addresses, consume an
activation ID or run the body. Starting consumes that handoff and checks address
capacity and activation admission before installing parameter slots. A malformed
later argument or failed admission cannot leave earlier arguments or a partial frame
installed. Successful synchronous calls perform both steps immediately.

Three runtime unit regressions cover independently prepared values, selected start
order and normal retirement; malformed later nested arguments without any storage,
fuel, events or identity changes; and exhausted address space or an already failed
activation context with no partial installation. A new host stepping regression
checks malformed later entry payloads under several quota partitions, preserving
`E_DRIVER`, the entry-validation phase, no events/active invocation, and exactly one
terminal delivery. The existing bounded-frame/slot test remains passing.

Validation: **111 Rust tests across nine selected targets** and **42 Node tests**
passed. The selection includes stepping, conformance, algebra, named/lexical
boundaries, source faults, fault reports and the language manual, including existing
native/expanded comparisons. The initial frozen-context test expected different
wording; it was corrected to the existing already-failed error. No full-suite claim
is made. Editor implementation, parser, core checker and native-generator sources
are unchanged and hash verified; the 0.102 isolated editor/native evidence is reused,
not claimed as a new UI run. Fresh compiler/LSP handshake and all six binary copies
and nine packaged/installed payloads match 0.103.0. Existing windows were not reloaded;
the source backup excludes the protected unrelated C# file.

This is an internal execution foundation, not a source task API or permission to
abandon deferred work. Captured-loan checking, task-owned temporary storage, public
capture contracts, cancellation and scheduling remain ahead. The async audit now
identifies synchronous checker loan retirement and immediate fault-permission checks
that must be separated for source deferral. The existing `run` versus `runtime`
entry-syntax question remains unchanged; neither form is implemented or selected.


## 0.104 — Named completion type locations

P2t preserves the written completion type after `exits` independently of its origin
mapping. Qualified names and generic arguments keep their authored endpoints through
module offsets, specialization and trait normalization. Name/type preparation errors
and public named-contract visibility errors select that range. Generated declarations
have no fabricated location. Bare nullary labels, explicit inline contracts, inferred
exits, origin/shape errors, first-error ordering and body-before-visibility checks keep
their existing behavior.

Three new CLI regressions cover local/imported and generic/trait contracts, unknown
qualified names, wrong type arity, private contracts, Unicode comments and multiline
spelling. They check that ranges exclude origin arguments, origin errors retain their
points, initializer errors retain their expression ranges, and bare-label/auto/inline
resolution remains valid. A prior public visibility test now expects the authored
named type. One new LSP regression edits an imported generic public helper's contract,
checks precise UTF-16 related locations and absent stale hover, then repairs it.

Validation: **439 Rust tests across fourteen selected targets** and **42 Node tests**
passed. Targets cover diagnostics, LSP, modules, generics, traits, aggregate/trait origin
contracts, named outcomes, constructors, public snapshots, formatting and the manual;
existing native/expanded comparisons are included. No full-suite claim is made.
The first focused LSP assertion accidentally selected the declaration's same-spelled
type; it was corrected to select the completion use. Production code did not change
for that correction. The isolated editor test checks multiline/compact unsaved
contracts and repair, then builds and runs the counter to emit 1 through 5.

Compiler/LSP version checks, six binary copies and nine packaged/installed payloads
are verified for 0.105.0. Existing user windows are not reloaded, and the source
backup excludes the protected unrelated C# file. No runtime semantics, async entry
spelling or task permissions change; the existing async design question remains open.

## 0.105 — direct async admission (E1c.1)

`tests/async_entry.rs` checks explicit call permission, generic and trait dispatch,
parenthesized/staged arguments, temporary and returned borrows, step/resume order,
fault-channel preservation, host entry rejection and expansion certificate tampering.
Original and expanded native programs agree with reference execution. Snapshot and
LSP tests cover public async permission and unsaved imported contract changes; editor
coverage checks keyword scopes, hover, diagnostics, repair and a native sample.
This is sequential direct invocation, not cold tasks, scheduling or coroutine lowering.

## 0.106 — owned local cold tasks (E1c.2)

`auto pending = defer operation(arguments);` evaluates and transfers owned arguments
without entering the async body. The internal task type retains captured resource
fault dispositions independently of the producer's normal completion contract.
Consuming `await pending` uses ordinary delta/capture/forward/single-completion forms;
local moves transfer the await obligation. A task cannot be copied, cloned, dropped
or forgotten, including one with no captures. This profile rejects direct/contained
borrowed captures and task-bearing control results, including wrapped tasks. It does
not introduce a public task type or erase captured obligations behind a result type.

Twelve `tests/cold_tasks.rs` tests cover eager constructor effects, cold body effects,
generic captures, disjoint consuming paths, zero-capture obligations, local transfer,
inline and stored completion adapters, body-fault permission at await, immediate
argument faults with pending prefix resources, escaping and borrowed-capture
rejection, one-instruction stepping, serialized expansion certificates and native
original/expanded parity. `tests/rust_adapters.rs` also proves that faulting before
await releases a captured retained Rust owner before contingency starts, without
executing the cold body; absence of its fault disposition is rejected.

Focused and relevant regression checks pass: **535 Rust tests across 21 targets**
and **44 editor tests**, plus offline all-target checking and Rust formatting. This
is selected regression coverage, not a full-suite claim. Existing ownership, moves,
initializer access, lexical boundaries, generic/trait/module checking, public
contracts, stepping, protocols, contributors, certificates and diagnostic recovery
remain covered. The manual's examples, links and compiler version checks pass.

The isolated VS Code test checks `task<producer>` hover on `auto`, keyword help,
unsaved clone rejection and repair, and native execution of the new example with
arguments captured before the body runs. Six compiler/server binary copies and nine
packaged/installed payloads are verified for 0.106.0. The source backup excludes the
protected unrelated C# file. Existing editor windows are not reloaded.

Source and native execution remain sequential. Borrowed task captures need persistent
loan holders and result-origin transfer in E1c.3; public task contracts, scheduling,
spawn, readiness, cancellation and direct LLVM coroutine lowering remain ahead.

## 0.107 — direct borrowed cold captures (E1c.3a)

A local cold task can capture a direct shared or exclusive reference to existing
reference-free storage. Captured loans remain attached to the task through local
moves and end when consuming await delivers its reference-free completion. A
reference argument reborrows under ordinary call rules; `move alias` explicitly
transfers the exclusive reference. The old alias remains consumed after that form.
Pending-task holders do not expire merely because source liveness finds no later
handle use, including on a path ending in a fault. Task ownership and capture-owner
fault dispositions remain independent obligations.

Ten `tests/borrowed_tasks.rs` tests cover overlapping and disjoint captures, shared
coexistence, exclusive conflicts, local moves, branch-sensitive consumption, explicit
reference transfer versus reborrow, references loaded from stored fields, generic
and trait dispatch, initializer alias conflicts, one-instruction stepping and
reference/native parity before and after expansion. Boundary checks reject an outer
borrowed task entering isolated work; locally created captures settle before recovery.
The added retained-Rust-resource regression checks both normal completion and a fault
before await, with exactly one owner release, no early body execution and rejection
when the owner lacks a required fault disposition.

Selected verification passes **441 Rust tests across 16 targets**, **44 editor
tests**, offline all-target checking and formatting/whitespace checks. The borrowed
task target combines nine focused tests with one separately validated boundary test;
completed tests were reused when only that fixture needed correction. Existing
owned tasks, direct async calls, references, stored records, borrowed sums, lexical
boundaries, initializers, generics, certificates and diagnostic recovery remain
covered. This is selected regression evidence, not a full-suite claim.

The reference includes executable borrowed-task examples and rejection cases. The
isolated VS Code test checks `auto` hover, unsaved `E_LOAN` diagnostics, repair and
live native execution of `playground/borrowed_tasks.tellur`. Six binaries and nine
packaged/installed payloads are verified for 0.107.0; the source backup excludes the
protected unrelated C# file. Existing editor windows are not reloaded.

Contained captures, references to reference-bearing storage, borrowed completion
payloads, nesting borrowed tasks in aggregates and public/escaping task contracts
remain rejected. E1c.3b must preserve per-capture dependency identities and transfer
only the selected completion's origins. Scheduling, cancellation and coroutine
lowering remain later work.

## 0.108 — borrowed cold-task completions (E1c.3b)

Captured loans now retain their parameter identity through local task moves. Await
maps the producer's checked completion origins onto those saved dependencies, then
retires the captures and installs only the selected result's loans. An unrelated
capture is available immediately after await; a returned reference continues to
protect its source until its last use. The ordinary result machinery preserves
exclusive reborrow ancestry, inferred projections, opaque split permissions,
conservative inferred unions and separate outcome domains.

Ten `tests/task_results.rs` cases cover direct shared/exclusive results, explicit
reference transfer, disjoint task moves, separate capture instances of one producer,
grouped record and option results, stored/inline adapters, forwarding, generic and
trait producers, source lifetime rejection, instruction stepping and native parity
before and after expansion. Existing task ownership, captured-resource fault
settlement and boundary isolation regressions still pass. Contained captures,
references to reference-bearing capture storage, nested borrowed tasks and
public/escaping task types remain rejected; no scheduler or coroutine lowering is
introduced.

Validation passes **488 Rust tests in 20 targets**, **44 Node editor tests**,
all-target compilation, Rust formatting and whitespace checks. These are selected
regressions, not a full-suite claim. The first nine new result tests are recorded in
the final focus log; the repeated-producer-instance check was added and passed
separately. Prior successful capture/owned-task targets are reused after version
and hover-help-only edits. Three reference checks cover documented examples and
links, including the distinction between borrowed results and contained captures.

The isolated VS Code test checks task/result `auto` hover, unsaved `E_LOAN`, repair
and native execution of `playground/task_results.tellur`. Release 0.108.0 verifies
six binary copies and nine packaged/installed payloads. Existing windows are not
reloaded. The verified source backup excludes the protected unrelated C# file.
Next E1c.3c extends captured reference-bearing values and carrier dependencies.

## 0.109 — owned captures containing references (E1c.3c.1)

A cold task can own a record, variant or option containing references. Capture
holders preserve parameter indices, record fields, active tags and nested carrier
contents through task moves. Shared copies remain independent retainers; exclusive
references and resource-bearing values transfer normally. Await maps contained
result origins onto these saved dependencies and releases unrelated captures.

Carrier references inside an owned capture retain both their backing storage and
external referents. Task holder paths are hidden metadata, not source projections;
liveness names their task and retains active carrier dependencies even if the path
faults without another task use. This prevents a captured carrier from releasing
its dependent loans before settlement.

Ten `tests/contained_tasks.rs` cases cover independent groups, shared copies,
exclusive transfer, options and nested tags, generic calls, nested carrier guards,
constructor evaluation/aliasing, fault retention, path-sensitive settlement, existing
isolation barriers and original/expanded native parity. Two additional retained-Rust
resource tests cover reference-only records and records containing both an owner
and a reference: missing fault dispositions are rejected, bodies remain cold, and
normal/fault paths release the host owner exactly once.

The reference documents the distinction between an owned reference-bearing value
and borrowing a whole reference-bearing carrier as a parameter. The latter remains
gated, along with nested borrowed tasks, escaping/public task types and unsupported
temporary backing storage. Existing collection-origin limits still apply. This is
sequential task execution, without scheduling or coroutine lowering.

The isolated VS Code fixture checks pending/result `auto` hover, an unsaved conflict
after dropping a separate shared copy, repair and native execution of
`playground/contained_tasks.tellur`. Six binaries and nine packaged/installed payloads
are verified for 0.109.0. Existing windows are not reloaded; the source backup excludes
the protected unrelated C# file. Next is E1c.3c.2, whole borrowed carrier parameters.

Validation passes **357 Rust tests in 14 targets**, **44 Node editor tests**,
all-target compilation, formatting and whitespace checks. This is selected regression
evidence, not a full-suite claim. The focus log supplies nine contained-task cases;
a separate filtered invocation supplies the tenth and contributes no LSP/manual
results. The full baseline supplies 191 LSP tests, three reference checks and 28
Rust-adapter cases; the separately repeated resource case is counted only once.
Previously completed tests are reused after version/help-only and test/doc edits.

## 0.110 — whole borrowed carrier captures (E1c.3c.2)

A cold task can borrow a whole record or sum containing references. Its direct
capture loan now retains a snapshot of the carrier's nested contents. Borrowed
completion lookup starts from that snapshot while preserving the outer authority,
then follows the existing field, tag and nested origin selectors. Direct storage
origins and contained external origins remain distinct.

Eleven `tests/carrier_tasks.rs` cases cover shared/exclusive carrier parameters,
whole-carrier versus detached results, explicit alias transfer versus reborrowing,
shared outer restrictions, nested guards, empty/nested sum alternatives, generic
parameters, disjoint producer instances, projected results, path-sensitive/fault
retention, repeated capture through an async parameter, isolation and temporary
lifetime rejection. Reference execution agrees with expansion, instruction stepping
and native execution for shared, exclusive, nested and sum captures. A retained-Rust
resource case checks missing fault dispositions, normal/fault settlement, no body
execution before await and exactly one host release.

The maintained reference documents both forms of carrier capture with accepted and
rejected examples. `playground/carrier_tasks.tellur` demonstrates releasing the
borrowed carrier after await while retaining the selected external source. LSP and
isolated VS Code checks cover pending/result inference, unsaved `E_LOAN`, stale-hover
removal, repair and native execution. Release 0.110.0 verifies six binary copies and
nine packaged/installed payloads without reloading existing windows. The source
backup excludes the protected unrelated C# file.

Public/escaping task contracts, nested borrowed task storage, temporary lifetime
extension, scheduling and coroutine lowering remain outside this checkpoint.
The author accepted invocation ownership for E1d: each async invocation owns its
spawned children, normal paths await them, and handles cannot escape the invocation
or cross fault boundaries in the first profile. No extra task-scope syntax is
required. This decision does not claim implemented `spawn` support.

Validation passes **352 Rust tests in 15 targets**, **44 Node editor tests**,
all-target compilation, formatting and whitespace checks. These are selected
regressions, not a full-suite claim. The first focus invocation supplies only its
two successful targets; an obsolete cold-task rejection expected `E_ASYNC` for a
raw carrier temporary where a reference was required. Its replacement explicitly
checks `E_BORROW`, and the final task invocation passes the remaining three task
targets. A separately added forwarding case brings the new carrier suite to eleven.
The filtered Rust-adapter run contributes one resource test. Repeated manual checks
are counted once, using the final log after adding the carrier rejection example.

## 0.111 — shared execution state and independent contexts (E1d.1)

The reference interpreter now keeps its value arena, monotonic frame-address
allocator, script cursor, events and fixed fuel in one execution-wide structure.
Current frame facts, continuations, pending control disposition, diagnostic tracker,
contingencies and resource handover belong to a movable context. The public session
still executes one sequential chain. This introduces no source scheduling points,
spawned-task API or change to fault/outcome semantics.

Five internal interleaving cases exercise shared borrowed storage without frame
collisions, retirement of only the completed child's slots, separate decision
facts with one scripted input history, a fuel budget that context selection cannot
refill, failure-atomic admission beside a suspended owner, and contingency/recovery
state retained in the correct chain. The owner stays suspended until its borrowers
finish. These are interpreter harness tests, not a claim that unchecked source
concurrency or sibling recovery after an escaping fault is permitted.

Validation passes **347 Rust tests in 15 targets**, including all stepping, fault,
boundary, cold-task, borrowed-result, carrier-task, collection and LSP cases selected
for this refactor. Reference/native comparisons and expansion preserve the existing
sequential observations. Three manual tests pass. A structural diff audit confirms
that machine method bodies differ only in field qualification and rustfmt layout;
the runtime algorithm is unchanged. Initial fixture failures were corrected for
canonical reference spelling, the manifest's required `returns` boolean and a Rust
helper's output lifetime.

The **44 Node editor tests from 0.110 are reused** after verifying unchanged editor
source/tests/configuration and version-only package manifest changes. A fresh
isolated VS Code run verifies inferred task/result types, unsaved loan diagnostics,
repair and native execution. Six staged binaries and nine packaged/installed
payloads are verified for 0.111.0; existing windows are not reloaded. The source
backup excludes the protected unrelated C# file. These are selected regressions,
not a full-suite claim.

Next E1d.2 adds internal owner/task identities and a checked readiness lifecycle;
source spawn and child settlement still require their own implementation evidence.

## 0.126 — separate task queue admission and completion joins (E1d.2d.1)

The private reference registry separates execution phase from completion ownership.
Queueing allocates no completion wait. A join can attach before dispatch, during
execution/readiness waiting or after completion without changing queue order.
Delivery requires the original one-use wait; normal consumption requires delivery.
Completed unjoined tasks still block normal owner retirement, while emergency
retirement cleans their obligations without inventing a completion.

Four new registry tests cover all started phases, duplicate/foreign/incorrect
producer rejection, completed unjoined work, wait identity exhaustion with atomic
cold-await failure, and cleanup that preserves another owner's live join. Existing
runtime and source task regressions check captured storage, returned references,
one-use resumption, readiness and fault settlement. Source `spawn`/`yield` and
independent retained completion payloads remain unimplemented.

Validation passes **164 distinct selected Rust cases across 13 targets**: 13
registry cases, 31 other runtime cases and 120 integration cases. The focused
registry run preceded the version-only bump and covers identical functional source.
The remaining runtime/source/manual targets ran with version 0.126. All-target
checking, formatting and whitespace checks pass. This is selected regression
evidence, not a full-suite claim. Unchanged editor/LSP and compiled native model
checks retain their recorded evidence.

The fresh isolated VS Code profile passes pending/result hover, unsaved loan
conflict and repair, and native execution with the final release binaries. Compiler,
LSP and VSIX 0.126.0 verify six staged binary copies and nine packaged/installed
payloads, including executable permissions. Existing windows were not reloaded.
The source backup excludes the protected unrelated C# file before enumeration is
used to read or archive sources. Next is retained completion payload storage,
including ownership and borrow preservation through delayed joins and settlement.

## Retained completion model on 0.126 (E1d.2d.2a)

Two test-only reference cases retain normal payloads after checked child frames
retire. Twenty-four original/expanded combinations vary early versus delayed joins,
join order and host quota. Owned strings and exclusive references survive until
one-use delivery; original caller storage remains live. Missing storage, crossed
waits, foreign owners and repeated delivery preserve the retained obligations.
Separate original/expanded owning-stop cases reject normal delivery, settle the
completed payload before its caller and preserve an independent root's completion.
Exact effect order and empty final task/storage/domain tables are asserted.

The fixture is formatted with the installed 0.126 compiler. Production runtime
changes are limited to a test-module declaration; current compiler/LSP/VSIX and
selected runtime/editor evidence remain applicable. The model does not yet connect
the ledger to automatic domain draining or validate real native resource release.
Runtime completion storage and native ownership integration remain the next gates.

Five selected cases pass: the two final model cases and three current manual
checks. All-target compilation, Rust formatting and whitespace checks also pass.
The earlier model pass predates added early-join, missing-storage and crossed-wait
assertions; only the final cases are counted. No rebuild, install or editor reload
is needed for this test-only checkpoint.

## 0.127 — retained runtime completions (E1d.2d.2b)

The reference runtime now stores completed task payloads and diagnostic handover
independently of immediate caller restoration. Publication validates before moving
values; delivery validates original owner, open domain and one-use join before
removing storage. Domain quiescence and emergency settlement include completed
results. Stopped payloads retire before source frames; already-parked callers restore
their diagnostic context and unwind without executing normal continuations.
Cold await uses the same storage/delivery path with unchanged sequential behavior.

The completion cases now use runtime storage directly across 24 original/expanded,
join-order, early/delayed-join and quota combinations. Rejected publication, missing
storage, foreign owners, crossed waits and repeated delivery leave owned payloads
and registry state intact. Owning-stop cases use automatic production cleanup.
An extended boundary case retains two completed borrowers with parked awaiting
callers; original fault evidence/fuel and independent-root behavior remain intact.
The initial focused probe still inspected the old test ledger for payloads; its
assertion was updated to shared runtime storage before the final passes.

Native result ownership, source `spawn`/`yield`, external readiness APIs, cancellation
and suspending cleanup remain separate work. The native backend stays sequential.

Validation covers **166 distinct selected Rust cases across 13 targets**: 46
runtime cases and 120 source/manual integration cases. The final publication checks
rerun the two completion cases; 44 other runtime cases reuse the immediately prior
successful run on identical functional source. Task/native parity, borrowed result
storage, fault traces, fuel partitions and current manual checks pass. This is
selected regression evidence, not a full-suite claim. Unchanged native models,
checker, LSP and editor unit evidence remain applicable.

The final isolated editor profile passes pending/result hover, an unsaved loan
error and repair, and native execution. Compiler/LSP/VSIX 0.127.0 verify six staged
binary copies and nine packaged/installed payloads with executable permissions.
Existing VS Code windows were not reloaded. The verified source backup excludes the
protected unrelated C# file before reading or archiving. Next is native retained
completion ownership; process-wide delayed-reference-result cleanup also remains
a gate before source scheduling.

## Administrative process retirement checkpoint (0.128)

E1d.2d.2c closes reference task admission on entry process termination and retires
registered results, ready/waiting chains and parked callers before status delivery.
The status stays administrative while nested continuations retire; no source handler
or ordinary continuation runs during cleanup, and no source fuel is consumed.
Written termination remains restricted to the entry's outermost lexical contingency.

Selected validation comprises 47 runtime cases and 121 source/manual cases (168
cases over 14 targets). The focused shared-runtime case covers completed owned and
exclusive-borrowed payloads inside nested domains, waiting/signalled/prepared
independent roots, exact original fault/fuel preservation, full retirement and
atomic late-operation rejection across original/expanded execution and three quota
partitions. The public regression verifies entry-only rejection, one-use status
delivery after nested fault unwinding, and four O0/O2 native comparisons.

Native model/host/generator, checker, fault tracker and LSP/editor implementation
are unchanged. Their prior native, 195 LSP and 44 Node checks remain valid; the final
release uses a fresh isolated editor smoke test with new binaries. Verification and
release records distinguish reused evidence from new checks. This is selected
coverage, not a full-suite claim or source concurrency support.

## Shared native result adoption checkpoint (0.129)

E1d.2d.3b promotes the compiled completion model's resource-transfer step into the
shared host. Preflight checks source/receiver scopes and the complete unique result
owner graph. Successful adoption changes resource membership and retires only the
source root, preserving selection, diagnostics, borrowed suppliers and source fuel.
Task completion and fault-domain authority remain caller responsibilities.

Four direct cases use real Rust owners and cover nested aggregate/task captures,
non-followed references, foreign/retired/duplicate/mistyped identities, incomplete
graphs, crossed/stale scopes, receiving-boundary cleanup and 32 repeated adoptions,
including exhausted scope-allocation capacity. The full native-host target and the
compiled result, independent-context and sibling/process suites check the shared API.
Selected direct native/LLVM and Rust resource-adapter regressions cover default
emission and existing ownership/fault behavior. No source concurrency or ownership
permission changes are introduced.

## Shared native retained execution checkpoint (0.130)

E1d.2d.3c extracts the compiled model's continuation, context/completion and domain
implementation into ordinary native support. Only fixture callbacks and explicit
schedule drivers remain injected by tests. An emission regression checks that the
shared implementation appears once and that model quota environment readers,
fixture control tables and driver calls are absent from normal output.

Three direct shared-context tests use actual Rust owners and check retained owned
and borrowed results, separate readiness and join transitions, atomic invalid/foreign
or repeated operations, result-before-supplier fault cleanup, independent result
survival and whole-session process cleanup before status. All fifteen compiled model
cases now exercise this same shared implementation across their existing expansion,
optimization, quota and fuel matrices. Native-host, selected direct native/resource
adapter and reference-manual checks cover integration. Source behavior is unchanged;
this is selected coverage, not a full-suite or source-concurrency claim.

## Shared task protocol and native invocation owners checkpoint (0.131)

E1d.2d.3d compiles one task protocol under the reference diagnostic adapter and the
native error adapter. Its thirteen existing queue/join/readiness/retirement cases
run in both targets. Four native activation cases prove fresh owners across nested
calls and frame reuse, pending normal-return rejection in five task phases, atomic
owner-capacity failure before capture transfer, and administrative retirement with
no fabricated completion or effect on independent owners.

Reference-runtime and cold-task/result/process/stepping regressions preserve actual
source behavior, including original/expanded and native checks where applicable.
All compiled native model drivers now verify that invocation owners retire fully
alongside frames/resources/domains under normal, source-fault, host-limit and process
paths. Selected native/resource adapters, manual, formatting and all-target checks
cover the integration. This does not claim source scheduling or full-suite coverage.

## Bound native task payload checkpoint (0.132)

E1d.2d.3e adds five direct native payload cases: original host/invocation/producer
identity, rejected malformed/stale/unbound/duplicate handles, task/wait exhaustion
atomicity, FIFO capture transfer and one-use dispatch, distinct completion waits,
real owned-buffer/resource preservation over sixteen rounds, emergency pending-return
cleanup with borrowed suppliers, and legal cold lexical movement. The ordinary
synchronous await helper rejects bound payloads instead of bypassing the registry.

Existing compiled model, native/LLVM, resource adapter and manual regressions cover
integration of the optional payload binding and embedded support. Reference runtime,
checker, fault tracker and editor implementation retain previous verified evidence.
These are selected internal-support checks, not a source scheduler or full-suite claim.

## Task-bound native context checkpoint (0.133)

E1d.2d.3f adds five native context integration cases and one frame preparation case.
They cover capture preservation on nine admission failures, original parent/invocation
identity, distinct readiness/completion, two-waiter FIFO selection and atomic rejection,
real owned and borrowed result adoption, duplicate/early joins, result/waiter cleanup
before suppliers, independent roots and owning contingency after all affected task
obligations retire. Frame preparation reserves without installing storage; cancelled,
stale and foreign plans preserve live slots. Existing host cases and both adapters'
shared protocol cases remain valid.

Compiled model, native/LLVM, resource adapter and manual regression matrices cover
integration. Reference continuation/checker/editor implementations are unchanged and
retain their earlier evidence. This is selected internal-runtime coverage; source
handoffs, resumable production lowering and full-suite conformance are not claimed.

## Scoped native task retirement checkpoint (0.134)

E1d.2d.3g adds five native-host cases for graph-wide preflight, foreign host identity,
nested task captures, loan non-traversal, repeated retirement, normal-exit/restart
rejection, lexical/recovery moves, captured-resource release and active-handler
failure. Four focused cases and the later handler case pass independently; the 68
existing host cases are checked separately without rerunning those focused cases.
All 15 retained compiled-model cases and four retained resource-adapter cases provide
regression evidence. Reference execution, shared task protocol, checker, LSP and
extension implementation retain their earlier evidence. This selected coverage does
not enable source scheduling or claim full-suite conformance.

## Sequential native await context checkpoint (0.135)

E1d.2d.3h adds seven host cases. Four compare nested call-chain diagnostics, readiness,
resource adoption and rejected admission; a fifth covers administrative completion and
whole-session process retirement. A sixth checks a fresh await after recovery, preserving
the original caller and allocating a fresh child identity. A seventh checks fuel
exhaustion at child/caller return points without changing failure kind. Full reports,
source sites, activation IDs, fuel and
events match the existing synchronous retained machine, including handler failure.
The 73 existing host cases run separately from these focused cases. All 15 compiled
models and four retained resource-adapter cases cover integration. Reference execution,
checker, shared task protocol, fault tracker and editor implementation retain prior
evidence. This selected coverage does not claim source scheduling or full conformance.

## Retained native await handoff checkpoint (0.136)

E1d.2d.3i adds eight host cases: queued admission retry and exact one-use delivery,
rejected admission with captured owners/loans, rejected delivery with adopted owners,
escaping child fault and owning contingency, administrative completion, process
retirement of undelivered fields, nested callback parity, and await delivery inside a
synchronous callee followed by its independent call return. The parity case compares
complete reports, events and fuel against direct compiled calls across 19 fuel budgets
and three quota partitions. The 80 previous host cases, all 15 compiled models, four
retained resource-adapter cases and three manual cases supply integration coverage.
Reference execution, checker, shared task protocol, fault tracker and editor logic
retain prior evidence. This selected coverage does not claim generated callback
handoff lowering, source scheduling or full conformance.

## Generated retained await callback checkpoint (0.137)

E1d.2d.3j adds two entry-context host cases and one generated-lowering regression.
The full selected matrix comprises 90 native-host cases, 16 compiled models, four
retained resource-adapter cases and three manual checks. Generated probes now exercise
actual bound `defer` and retained `await` callbacks across original/expanded O0/O2
programs, fuel cutoffs and quota partitions. Entry tests compare complete ambient
chains, sites, events, fuel and long-error truncation with direct compiled calls.
Ordinary source emission, the reference executor, checker, shared task protocol and
LSP/editor logic retain prior evidence. This is selected coverage; source scheduling
and production lowering promotion remain outside this checkpoint.

## Production retained native callback checkpoint (0.138)

E1d.2d.3k promotes the validated compiled callback emitter and shared sequential entry
driver to normal native builds. The native-host suite includes a production-entry
comparison of complete reports, events, fuel and long-error truncation against direct
execution. Two lowering regressions check shared emission, unchanged private-entry
restrictions and absence of probe quota/schedule controls. Frame-capacity checks retain
the local-layout bound after synchronous region functions are removed.

Fresh validation covers the retained compiled models and all integration-test targets
that directly request native source/builds, including tasks, borrowed results,
constructors, generic/trait calls, loops, named/lexical boundaries, process termination,
Rust resource adapters and settings/file/HTTP applications. Original/expanded O0/O2
fuel and fault matrices exercise the production path. This is selected coverage, not
a rerun of every checker/editor test: unchanged LSP/Node evidence is reused, and a
fresh isolated editor checks the packaged compiler and native execution. Source
scheduling and mixed spawned-child integration remain outside this checkpoint.

The broader native-consumer run also updates an old reserved-syntax regression:
async declarations are accepted, while the remaining reserved statement forms still
reject. Async/await acceptance and context restrictions retain their dedicated tests;
this correction changes no parser or checker behavior.

## Independent-child native cold-await checkpoint (0.139)

E1d.2d.3l permits an independently admitted child to perform exclusive cold awaits.
Four native-host cases cover exact independent-child causes, lexical handler faults,
long-message truncation, one-use owned/borrowed result delivery, atomic admission
exhaustion, sibling-fault retirement at each await phase and every fuel cutoff.
Existing process-retirement coverage now also includes mixed child trees.

A compiled fixture covers nested borrowed awaits, recovery followed by another await
and an escaping child fault using the production callback emitter. Original/expanded
O0/O2 builds compare complete observations across positive/zero quota partitions and
fuel cutoffs. The retained model regression suite and selected cold-task, fault,
process and resource targets verify unchanged sequential behavior. Previous broad
native-consumer and unchanged LSP/Node evidence is reused; this is selected coverage,
not a full-suite claim. A fresh isolated editor checks the release payload.

Independent admission is explicit in the test harness. Parents with live children
still cannot begin their own await, and sequential/awaiting contexts cannot admit
independent children. This checkpoint neither adds source scheduling nor changes
accepted ownership, origin, outcome, fault/contingency or path-label semantics.

## Prepared owner-await checkpoint (0.140)

E1d.2d.3m admits a prepared owner cold await beside independent children while keeping
the parent-selection and completion-quiescence guards. Four new native-host cases
check exact owning reports across pending, waiting, completed and delivered await
phases; nested call/boundary/handler restoration; resource result joins in either
order; atomic exhaustion; and source-fault/host-limit cleanup. Existing process
retirement also covers owners awaiting alongside siblings.

A generated fixture exercises lexical and named boundaries, stopped active handlers,
completed results and pending delivery at original/expanded O0/O2. Complete owning
reports match the pre-admission baseline, and quota partitions preserve all observed
reports, events and fuel. Selected existing sibling/process/recovery and cold-task
models plus production cold-task/source-fault/process/manual tests run freshly.
Unchanged broader native-consumer and LSP/Node evidence is reused, with a fresh
isolated editor check. This is selected coverage, not a full-suite claim.

The host prepares the owner's await before admitting siblings. General parent
execution with outstanding children and source scheduling remain unimplemented.

## Native retirement preflight (0.141)

E1d.2d.3n checks retained child/result membership before runtime frame, constructor,
loop and lexical-boundary retirement. Four new host tests include a 30-scenario
transition/borrower-phase matrix, retained/immediate final-domain preflight, last-join
success and real Machine error settlement. The tests bypass the public parent guard
intentionally and verify that runtime retirement leaves storage, scopes, diagnostics
and obligations intact before borrowers settle. The initial regressions failed on
0.140's lower-level retirement path, then passed with the new preflight.

Selected generated call/constructor, loop, named/lexical boundary, result, sibling,
owner-await and process models run freshly with the embedded runtime, alongside
production cold-task, source-fault, process, native-frame and reference-manual targets.
Unchanged checker, reference-executor and LSP/Node evidence is reused; these selected
checks are not a full-suite claim. A fresh isolated editor check verifies the release.
The earlier parent-selection guard remains essential because callbacks may move
source operands before returning a step. No source scheduler is enabled.

## Native invocation ownership and atomic selection (0.142)

E1d.2d.3o preserves exact parent invocation authority across independent children,
queued tasks, cold awaits and retained results. Four new host tests cover callback
operand/fact/effect/fuel preservation; pending delivery and subsequent normal join;
wrong-invocation adoption in the same context/domain; and absent nested callers in
all three admission routes. Three regression cases fail on 0.141 before the fixes.
The prior lower-level retirement fallback is still tested directly beneath selection.

A generated probe checks rejected selection before callbacks and pending delivery in
the existing loop/cold-await fixture. Full observations match the unprobed schedule
at original/expanded O0/O2 and different quota partitions. Six existing generated
cold-await, owner-await, result, sibling, lexical-recovery and process models run
freshly, alongside cold-task/source-fault/process/manual targets. Unchanged checker,
reference and LSP/Node evidence is reused; this is selected coverage, not a full suite.
Release checks include a fresh isolated editor session. No source scheduler is added.

## Native callback access plans (0.143)

E1d.2d.3p adds five planner cases covering written evaluation order, fields and
indirection, shared/exclusive modes, simultaneous binding moves, distinct call kinds,
separate delivery writes, constructor destinations and loop-local retirement. One
host case distinguishes normal pending delivery from waiting, error, process and
settled states, while checking immutable repeated queries and resource cleanup.

The generated selection probe repeatedly queries callback and delivery plans across
original/expanded O0/O2 runs and quota partitions, compares complete observations and
retains atomic rejection of parent selection with live children. Existing generated
call/constructor, loop, lexical/named boundary, fault and process models run freshly,
alongside selected production cold-task/source-fault/process/manual targets. This is
selected coverage, not a full-suite claim. Unchanged checker, reference and LSP/Node
evidence is reused; a fresh isolated editor verifies the packaged compiler. Symbolic
plans do not grant concurrent access or add source scheduler handoffs.

## Native access storage resolution (0.144)

E1d.2d.3q adds five host tests for live allocation identity, foreign arenas and offset
reuse, direct/indirect field paths, shared/exclusive authority, unavailable reads,
uninitialized destinations and atomic failure. Buffer pointers and real resource
owners survive repeated queries. The await-delivery case now resolves normal writes
and checks that error/process/settled states remain unknown.

Two generated probes query every selected context in existing loop and constructor
fixtures across original/expanded O0/O2 runs and quota partitions. The unexpanded
constructor verifies its external destination; expansion can inline it. Full observations
agree and existing rejected parent selections remain atomic. Selected retained-child,
result, lexical/owner-boundary and process models and production/manual targets run
freshly. Unchanged checker, reference and LSP/Node evidence is reused. A fresh isolated
editor checks the release. This is selected coverage, not a full-suite claim, and does
not enable source concurrency or prove captured carrier loan permissions.

## Retained native capture descriptions (0.145)

E1d.2d.3r adds six host cases for nested, owned and projected carriers, shared/exclusive
attenuation, repeated/cyclic references, bounded traversal, invalid storage, allocation
reuse, exact resource destruction, all admission routes and readiness/result retention.
An oversized description remains unknown while normal execution and joining succeed.
Existing failed-admission and pending-delivery cases now inspect retained descriptions.

Three generated probes query all selected contexts across original/expanded O0/O2 and
quota partitions. They include the runnable carrier-task example, where detached
result delivery still allows later carrier retirement. Full observations and existing
parent-selection rejection agree. Selected real-resource, fault/process and retained
context models run freshly, alongside native production/manual targets. Unchanged
checker/reference/LSP/Node evidence is reused; a fresh isolated editor checks release
integration. This is selected coverage, not a full-suite claim. Captured runtime graphs
do not certify hidden checked origin/holder guards or permit source scheduling.

## Checked capture guard acceptance audit (on 0.145)

`tests/task_capture_guards.rs` adds three cases for detached field captures returned
by an ordinary control. Positive paths retain reference/expanded observations;
negative paths reject carrier retirement, owner mutation and conflicting alias use
before await and while a borrowed result is live. Checked result contracts explicitly
retain their storage guard. Explicit transfer consumes the old alias; reborrow permits
its use after result delivery. A shared reborrow of an exclusive field still protects
the carrier, while a copied shared field permits carrier retirement before defer.

The runnable `playground/guarded_task_captures.tellur` also runs through a generated
native selection probe at O0/O2, original/expanded form, with quotas 1 and 0/3/1,
with and without immutable queries and rejected parent selections. Its full native
observation matches the reference executor. All four selection probes now compare
across original and expanded forms with one shared baseline. No production compiler
or editor changes are part of this audit; 0.145 release evidence remains applicable.

## Partial checked reference-origin evidence (0.146)

`tests/reference_evidence.rs` checks four aspects of E1d.2d.3s.1: direct and returned
exclusive captures retain their erased carrier guards; copied shared captures retain
their external owner without inventing a holder dependency; received carrier fields
carry partial evidence on private address reads; and expansion remaps each invocation's
origin slots independently. Serialized certificates roundtrip and reject missing
wrappers/origins, retargeted roots/paths and weakened guard or opaque flags. The evidence
schema cannot claim completeness. A verifier unit case rejects non-reference operands,
unknown slots and indirect origins rooted in non-reference slots.

Reference execution, native emission/access plans and contributor analysis treat the
wrapper transparently. Carrier/sum and contract regressions include abstract tagged
origins whose symbolic paths cannot be validated as current ordinary field accesses.
Selected generated native probes compare original/expanded O0/O2 and quota partitions;
source checking, editor and cold-task/fault consumers receive fresh coverage. Native
runtime dependencies remain descriptive, and source scheduling stays gated.

## Bound native reference origins (0.147)

Five new native-host cases cover guarded reborrow/copy/move propagation, shared
attenuation, symbolic origin paths through selected sums, borrowed and copied collection
results, stale/foreign formation identities, value equality and bounded/unavailable
metadata. The three-route admission/readiness/retained-result case now transports a
bound guard. The generated guarded-capture probe requires those origins at admission
and pending delivery, retaining original/expanded O0/O2, quota-partition and immutable
rejected-selection comparisons. Other native capture, fault, retained-resource, sum and
collection consumers check behavior preservation. Partial origins remain descriptive;
no completeness, source concurrency or new access permission is claimed.

## Conservative direct reference completeness (0.148)

The reference-evidence suite now checks broad ancestor origins, complete scalar aliases,
conditional/guarded fallback and certificate rejection of promoted or weakened coverage.
Structural validation rejects empty and guarded complete descriptions. Two native-host
cases verify that local annotations cannot promote partial incoming references and
that result merging preserves completeness only across complete inputs. The generated
direct-capture probe requires complete admission and pending-delivery descriptions;
guarded probes retain partial origin checks. Native consumers, checker/contract/LSP
regressions and source/reference observations remain covered. No parent-selection or
retirement gate is relaxed, and complete coverage alone grants no access permission.

## Immutable native interference queries (0.149)

Six native-host cases compare direct child dependencies with field accesses, broad
ancestor origins and retained results. They reject incomplete/unknown evidence,
unsupported transitions, stale or foreign allocations, wrong invocations and boundaries,
and waiting owners without state changes. Indirect callback references require saved
formation identity after allocation reuse. Pending normal delivery compares result
writes only. Mode checks preserve conservative shared overlap; bounded comparison
returns inconclusive. Generated original/expanded O0/O2 selection probes repeat queries
at real callbacks across quota partitions and retain immutable gate rejections. This
query does not authorize independent parent execution or source concurrency.

## Aggregate native borrower coverage (0.150)

Seven native-host cases cover sibling/descendant interference, readiness and completed
children, independent membership reconciliation, queued/live await captures, joined
pending result provenance, aggregate limits and real resource release only during fault
settlement. Existing authority cases now reject aggregate queries under another current
invocation or boundary. Generated original/expanded O0/O2 selection probes require both
conclusive/inconclusive aggregate observations and complete pending-delivery coverage,
while preserving both execution gates and source observations across quota partitions.

## One-transition native admission (0.151)

Five native-host cases verify fresh disjoint admission beside live/waiting/completed
children, changing borrower/evidence rejection, exact lower invocation/fact/callback
identity, instruction-free one-use delivery and real borrower-before-owner settlement
after fuel failure. Generated original/expanded O0/O2 selection fixtures exercise real
admitted callbacks beside a retained child and preserve source observations across quota
partitions. Legacy multi-instruction drivers keep rejecting children at all quotas.

## Bounded native admission (0.152)

Five new host tests cover partial progress before overlap/opaque rejection, zero quota,
explicit public progress reporting, the 4096-transition ceiling with untouched siblings,
normal delivery plus next-callback counting, readiness/foreign/terminal authority and
report preservation. Existing admission tests additionally exercise batch freshness and
real resource settlement after an admitted fault. Generated original/expanded O0/O2
fixtures compare bounded batch observations across quota partitions and preserve legacy
rejection checks, complete pending delivery and checked capture evidence.

## Complete local holder guards (0.153)

Checker cases distinguish complete local exclusive/shared reborrows from copied shared
fields and partial returned-family/conditional cases. Six certificate mutations and six
structural mutations reject weakened guarded evidence. Native tests cover fresh-only
binding, unsupported proof fallback and disjoint admission with holder/pointee/broad
ancestor exclusion across live, waiting and completed children. The local guarded example
runs through reference/expanded and native original/expanded O0/O2 execution; generated
probes require complete guarded capture and pending result delivery across quota partitions.

## Complete fresh local carriers (0.154)

Metadata tests require the holder and both stored owners, preserve nested/projected
records and reject certificate removal of every dependency or the completeness flag.
Alias, conditional and constructor-result cases remain partial. Structural tests require
fresh direct holder coverage. Native tests show that complete root annotations cannot
promote partial or foreign stored-reference evidence. Generated original/expanded O0/O2
runs require nonempty complete captures and retained delivery through quota partitions,
while checking disjoint host progress and identical source observations.

## Complete local carrier aliases (0.155)

Metadata cases retain both projected and whole carrier storage plus all three owners
through moved exclusive aliases and shared reborrows. Conditional and received aliases
stay partial. Certificates reject erasing the broad holder, its additional owner or the
completeness flag; structural checks reject unrelated, indirect or unsupported ancestor
origins. Native host tests block broad holder/pointee accesses even outside the actual
projected runtime graph, across live/waiting/completed children, and preserve state on
rejection. Original/expanded O0/O2 models require complete nonempty capture and delivery
with equal source observations across quota partitions.

## Incoming record-leaf evidence transport (0.156)

Metadata cases cover owned/borrowed input records, nested record fields, singleton versus
unioned groups, tagged/nested-reference exclusions and unchanged expanded observations.
Certificates reject altered/removed transport markers and retargeted copies; structural
checks reject moved/fresh/non-field operands. Native tests preserve complete and partial
origins exactly, reject missing/foreign/stale/mismatched/over-budget metadata and retain
source equality. Original/expanded O0/O2 local-carrier models assert nonempty complete
incoming copies across quota partitions alongside existing capture/delivery/admission
checks. Received aliases and family-derived uses remain conservative.

## Complete singleton shared input leaves (0.157)

Metadata cases require full incoming pointee and projected origins for borrowed/owned
record inputs, copies and moved aliases. Certificates reject broad-origin removal and
weakened status. Exclusive, grouped, conditional, returned-permission and whole-carrier
cases remain partial. Native host probes permit shared-holder release and reject broad
pointee writes through live/waiting/completed children, including complete empty caller
origin lists and partial caller fallback. Original/expanded O0/O2 models check all captures
and pending deliveries, inject partial incoming evidence without promotion, and preserve
source observations across quota partitions.

## Bounded native holder snapshots (0.158)

Native probes retain actual stored owners and symbolic ancestry without source-value
changes; partial root/leaf evidence stays partial. Missing, mismatched, foreign, stale,
non-aggregate and over-budget inputs are unavailable. Repeated and cyclic storage stays
bounded. Detached-leaf host probes retain hidden owners with complete empty incoming
origin lists, block holder/sibling writes across live/waiting/completed states, and reject
partial admission without effects. Generated original/expanded O0/O2 carrier models query
real holder inputs and preserve source observations. Compiler proof gates remain unchanged.

## Checked exclusive input holders (0.159)

Metadata tests cover direct and aliased singleton exclusive leaves, nested record fields,
full pointee and holder guards, exact entry materialization and grouped-permission fallback.
Independent verifier and certificate mutations reject missing/retargeted/weakened descriptors
and generic complete substitutions. Native attachment probes reject foreign/missing identities,
changed holder fields or reference types and weakened guards without effects; partial operands,
leaves, holders and unselected stored references cannot become complete. Host admission retains
otherwise unreachable owners through live/waiting/completed children. Original/expanded O0/O2
models exercise actual generated attachment, captures and pending delivery, and injected partial
callers preserve the same source observations across quota partitions.

## Complete incoming carriers (0.160)

Metadata tests cover whole and projected incoming carriers through direct and erased aliases,
including every original private input field and the broad original root. Grouped/returned
permissions remain partial. Independent structural and certificate mutations reject omitted,
changed or duplicate descriptors, missing broad origins and generic complete substitutions.
Native probes reconcile actual fields with original materializations and preserve partial
operand, holder, leaf and unselected sibling evidence without changing source state. Host
admission blocks whole-holder and outside-owner access across live/waiting/completed children.
Original/expanded O0/O2 models cover generated attachment, capture, pending delivery and
injected partial inputs under quota partitions with unchanged reference observations.

Nested expansion regressions cover both carrier and guarded input descriptors inside loops
and fault boundaries, comparing original/expanded execution and rejecting removed nested
materialization evidence. This also retains existing generic wrapper and loop-local carrier
behavior after descriptor verification.

## Checked cold-await admission (0.161)

Protocol tests prove immutable scheduling preflight and capacity/phase revalidation. Host
cases cover local parent progress with live/waiting/completed borrowers, exact handoff and
separate one-use delivery, partial/overlapping evidence, foreign or replaced handles, wrong
producers, malformed access plans and exhausted wait identities. Rejections preserve values,
facts, fuel and queues; actual callback failure settles the owning work. Generated original/
expanded O0/O2 models match reference observations under zero/small/large quota partitions.
Lexical/named sibling-fault models now inject the sibling before await preparation and retain
owning diagnostic restoration through nested work, active handlers and result delivery.

## Direct-await handoffs (0.162)

Protocol and host cases cover atomic task/wait allocation, stale owners, foreign registries,
invalid producers/arity and unchanged captured arguments on rejection. Reference tests
observe separate parked callers before child execution, nested direct/cold awaits, storage
and suspension capacity failures, unchanged events and fuel across host quota partitions.
Generated original/expanded O0/O2 probes verify direct/cold handoff counts, stable repeated
pending polls and one-use borrowed completion. Named/lexical fault comparisons preserve
reports at every fuel cutoff; real-resource probes compare releases and retained owners.
Source scheduling remains gated.

## Retained started-task joins (0.163)

Shared join preflight preserves registry state. Native host tests retain unjoined work in
Ready/Running/Waiting/Completed phases, reject foreign/stale/crossed-domain/wrong-producer
claims and capacity failure atomically, and require the consuming join for delivery.
Completed payloads retain owned resources and borrowed supplier storage; owning faults and
process exit retire them before supplier cleanup and contingency, skipping stopped nested
handlers. Generated original/expanded O0/O2 probes explicitly substitute host queueing at
an existing cold-await site and compare normal owned/borrowed results with the reference
executor under quota partitions. This is host integration evidence, not source concurrency.

## Children of awaited invocations (0.164)

Native host coverage composes raw, paired and unjoined children with awaited invocation
owners. Retained borrowed results, early/late claims, stale ancestor authority, readiness,
source-fault recovery, fuel exhaustion and process retirement preserve original chains and
release every resource once before supplier cleanup. Generated original/expanded O0/O2
probes compare nested started/direct-await results with reference execution and exercise
sibling faults across work, handler, completed and delivered phases. Harness scheduling is
explicit; these checks do not claim source concurrency support.

## Reference started-task joins (0.165)

Private reference queue/claim/delivery tests preserve original/expanded owned and borrowed
outcomes across Ready, Running, Waiting, nested-await and Completed claims. Foreign sessions
with matching numeric IDs, wrong producer/invocation/domain, early/stale delivery, malformed
captures and wait exhaustion reject without consuming authority or payloads. Generic result
delivery cannot bypass the consuming join. Emergency drain retires unjoined task obligations
after child unwind, including stopped nested boundaries and sequential descendants, before
the owning continuation resumes. Source scheduling remains gated.

## Owned source scheduling (0.166)

Source spawn/yield cases check eager capture, queue-only spawn, deterministic handoffs,
local handle moves, required normal-path await, original/expanded result and diagnostic
agreement, nested cold/direct awaits and invocation-owned grandchildren. Native cases
cover O0/O2, varying quotas including zero, every instruction-budget cut and disposal
of real queued/running/completed resource owners before the owning contingency. Negative
cases reject borrowed spawn, handle escape, duplicate/missing joins, cross-boundary use,
and parent calls before joining. Pending results and newly acquired owners retain their
fault-disposition checks at handoffs. Formatter, LSP and editor cases cover the syntax,
unsaved repair and native example execution.

## Synchronous helpers beside started children (0.167)

Source cases cover borrowed parent updates/results, constructor staging, concrete
generic and trait calls, imported helper bodies, sealed started-handle wrappers,
and transitive rejection of task handoffs or fault boundaries. Unsaved imported
helper edits invalidate caller diagnostics and restore them after repair. Original/
expanded reference/native comparisons cover helper work before/after handoffs,
queued-child settlement on helper fault, O0/O2, zero/small/large quotas and every
fuel cut. Real resource probes cover helper faults with suspended and completed
siblings; all created owners retire once before the owning contingency.

## Spawned producer stop obligations (0.168)

Negative source cases retain resources without fault dispositions across yield,
direct/cold awaits, ordinary helpers, named supervision and retained lexical/named
boundaries; aggregates and transitive generic callees keep the same requirement.
The same sequential cold producers remain checked. Positive repairs complete whole
or projected owners before the handoff, including an ordinary callee that consumes
its input first. Unsaved imported producer edits invalidate spawn admission and
restore it after repair. Native original/expanded O0/O2 probes use a real Rust owner
without a source fault disposition: every fuel cutoff and quota partition retires
each allocation once, while normal execution closes it before yielding.

## Parent cold/direct awaits beside children (0.169)

Reference cases cover normal parent borrows, completed unjoined siblings, direct
and cold awaits, and sibling faults before dispatch and inside a nested awaited
boundary. The context census covers every fuel cutoff and quota partition. Native
differential models cover original/expanded O0/O2, queued/nested/cold await faults
and normal interleaving. Real Rust resource logs check suspended borrowers, nested
local owners and completed sibling resources retire before the owning contingency.
Unsaved imported producer edits recheck the required stop permissions. Inferred
parent borrows and fault-capable sibling obligations remain enforced.

## Inline helper handoffs beside children (0.170)

Source regressions cover inline entry order, returned parent borrows, transitive,
constructor, generic and concrete trait calls, imports, wrapped handles, transferred
inputs completed before handoff, and rejection of inline boundaries or unsafe stop
obligations. Unsaved imported helper changes refresh both boundary and stop proofs.

Reference retirement census and original/expanded native models exercise sibling
faults while the original owner is below helper/run frames at yield, started join,
queued await and nested cold/direct await. Quota partitions include zero, and fuel
cuts run through completion. Real resource logs require awaited locals to retire
before helper-owned backing storage and before the owning contingency; stopped
nested handlers never execute. The cooperative helper example also runs in a fresh
isolated editor. Existing host callback access rules remain unchanged.

## Direct borrowed spawned captures (0.171)

Direct shared/exclusive references to reference-free storage remain loans until the
consuming join, including queued, running and completed-but-unjoined children.
Conformance covers shared readers, disjoint fields, local handle/reference moves,
rejected aggregate wrappers, branch joins, stored sources, generic/trait calls and conflicting parent or
sibling accesses. Temporary captures, nested/reference-bearing captures and borrowed
results remain rejected. Original/expanded native O0/O2, quota/fuel cuts, nested
fault settlement and real resource lifetime checks agree with reference execution.

## Direct borrowed spawned results (0.172)

Conformance checks queued/completed capture loans, consuming result adoption,
unrelated capture release, selected outcome origins, distinct producer instances,
exclusive reborrow ancestry, moves, generic forwarding and stored/inline adapters.
Child-owned/local storage cannot escape. Unsaved imported result-origin changes
invalidate conflicting caller access and repair restores hover. Original/expanded
O0/O2, quota/fuel cuts and resource/census models cover unjoined and delivered result
retirement before owning contingency. Detached carrier guards and an awaited child
owner exercise stopped cleanup with suspended ancestors. Bounded reference polling
and the nineteen-variant context census reject the previously observed fuel-cut
stall, without releasing supplier storage early or running stopped nested handlers.
Fault reports retain the awaited spawning activation and original site in both
executors, while excluding unrelated stopped await frames. Reference-bearing carriers
remain gated.

## Flat record spawned captures (0.173)

Owned records with direct reference fields preserve leaf loans until join and selected
direct result origins afterward. Tests cover shared Copy records, exclusive transfer,
disjoint sibling captures, generic outcome alternatives, moved handles and detached
carrier guards. Unsaved imported origin edits invalidate conflicting callers; repair
restores diagnostics and hover. Nested/borrowed/tagged captures, result carriers and
borrowed-handle wrappers remain rejected. Original/expanded O0/O2 native comparisons,
zero/nonzero quota partitions, all fuel cuts and the expanded reference context census
cover normal results, awaited-owner faults and resource/borrower retirement. Only the
owning contingency runs after stopped child settlement.

## Flat record spawned results (0.174)

Result records preserve selected contained origins and field-sensitive loan release.
Tests cover queue/run/unjoined/adopted phases, generic outcome alternatives, shared
copies, exclusive moves, shared outer authority, detached guards and forbidden child
storage escape. Unsaved imported origin and nested-shape edits refresh caller diagnostics
and repair restores hover. Nested/borrowed/tagged result carriers remain rejected.
Original/expanded O0/O2 models exercise normal execution, guards, faults before/after
join, awaited owners and real resources returned beside borrowed fields. Quota partitions,
every fuel cutoff and the 26-variant reference census verify settlement before the
owning contingency. Fresh editor checks include result-loan conflict repair and native run.

## Nested owned record spawned payloads (0.175)

Capture/result field paths survive recursive owned records, partial field/subrecord
moves, shared copies, disjoint exclusive siblings and selected generic result origins.
Detached guards remain live through completion/adoption; imported unsaved origin swaps
invalidate conflicting callers and repair restores hover. Borrowed/tagged carriers stay
rejected, and collection-contained origin signatures retain their earlier restriction.
Original/expanded O0/O2 quota/fuel comparisons and the 30-variant reference census cover
normal schedules, awaited-owner faults before/after join, nested guarded results and
nested resources beside borrowed suppliers. Existing isolation and stop dispositions
remain required. Fresh editor checks exercise partial-move loan repair and native run.

## Owned tagged spawned payloads (0.176)

Options and closed variants retain selected/empty payload permissions through spawn,
completion and consuming join, including nested records, generic forwarding, constructor
preparation and independent shared/exclusive siblings. Detached guards remain live;
unsaved imported tag-origin swaps invalidate callers and repair restores hover.
Reference-bearing outcome payloads, borrowed carriers and reference-bearing pointees
stay rejected. Original/expanded native O0/O2 quota/fuel comparisons and the 34-variant
reference census cover tagged values, guards, faults before/after join and real resources
in variant captures/option results. Fresh editor checks exercise tag-specific loan repair
and native execution. Existing fault dispositions and handling obligations are unchanged.

## Named outcome spawned payloads (0.177)

Named outcomes retain mandatory handling across owned capture and result delivery,
including empty/shared alternatives and nested records/options. Nominal identity,
selected origins, generic forwarding and imported unsaved origin changes stay checked;
discard, clone, duplicate capture and normal unhandled exit remain rejected. Anonymous
reference-bearing contracts retain their earlier cold-call restriction. Original/expanded
native O0/O2 fuel/quota models and the 38-variant reference census cover guards, faults
before/after join and real resources inside named outcome captures/results. Fresh editor
checks cover nested handling diagnostics, repair and native execution.

## Borrowed record spawn captures (0.178)

`source_borrowed_records` checks holder storage and contained supplier loans through
queueing, running, completion, handle/reference moves and consuming joins. Selected
shared contents can release their holder; exclusive contents retain detached guards
and reborrow ancestry. Siblings, disjoint fields, generic origins and rejected broader
carrier shapes are covered. LSP and fresh editor probes check unsaved invalidation and
repair. Five `compiled_source_borrowed_records_*` models cover original/expanded O0/O2
execution, all fuel cutoffs and quota partitions, joined/unjoined faults and exact-once
resource retirement before the owning contingency. The source census includes these
fixtures. Existing cold semantics and conservative host evidence remain unchanged.

## Borrowed option and variant spawn captures (0.179)

`source_borrowed_tags` checks selected/empty alternatives in shared/exclusive holders,
including nested records/options/variants, field reborrows, generic forwarding,
siblings and disjoint storage. Holder loans last through join; selected result loans
and exclusive guards last through final use. Borrowed outcomes, deeper references and
whole borrowed-carrier results remain rejected. LSP and fresh editor probes check
unsaved origin invalidation, repair and native execution. Five
`compiled_source_borrowed_tags_*` models compare original/expanded O0/O2 execution at
every fuel cutoff and quota partition, including real resources in borrowed variant
storage and joined/unjoined faults. The source census includes these fixtures.

## Borrowed named outcome spawn captures (0.180)

`source_borrowed_outcomes` checks shared/exclusive borrowed outcome storage, nested and
empty alternatives, unchanged caller handling obligations, selected origins, detached
guards, moves, siblings, nominal identity and generic forwarding. Consuming an outcome
before join, leaving it unhandled, dropping/cloning it or hiding it in a wrapper remains
rejected. Imported unsaved origin changes invalidate callers and repair restores hover.
Five `compiled_source_borrowed_outcomes_*` models compare original/expanded O0/O2 programs
at every fuel cutoff and quota partition, including real resources in borrowed outcome
storage and joined/unjoined faults. The source census includes these fixtures; fresh
editor checks cover caller handling, holder conflicts, repair and native execution.

## Whole borrowed record spawn results (0.181)

`source_borrowed_record_results` checks holder authority and contained origins through
completion and consuming join, shared copies and detached leaves, exclusive result
moves/reborrows, projected guards, disjoint holders, nested named-outcome obligations,
generic forwarding, stopped siblings and rejected local escapes. LSP and fresh-editor
checks cover unsaved imported storage/contained-origin changes and repair.

Six `compiled_source_borrowed_record_results_*` models compare original/expanded O0/O2
programs at every fuel cutoff and quota partition. They include nested tagged/outcome
contents, completed whole-record references, real resources in retained holders and
awaited parents. The 55-variant reference context census also covers joined/unjoined
faults. Existing cold carrier and result contract suites remain part of regression
coverage. No production scheduler changes or broader whole-borrowed result admission.

## Whole borrowed option/variant spawn results (0.182)

`source_borrowed_tagged_results` checks active/empty whole-holder storage, selected
contained permissions, shared Copy, exclusive moves/reborrows, projected guards,
independent siblings, generic forwarding, nominal identity, local escapes and nested
outcome obligations. Imported LSP contracts and a fresh editor cover holder conflicts,
unsaved repair, inferred result types and native execution.

Six `compiled_source_borrowed_tagged_results_*` models compare original/expanded O0/O2
programs at every fuel cutoff and quota partition, including nested named outcomes,
completed whole tagged references, resource-bearing holders, awaited parents and stopped
siblings. The 60-variant reference context census checks retirement before and after join.
Previously validated cold carrier semantics remain the baseline; no production scheduler
or ownership policy changes accompany this bounded source result admission.

## Whole borrowed named-outcome spawn results (0.183)

`source_borrowed_outcome_results` checks whole holder and selected supplier loans,
shared copies, exclusive transfers, projected/detached guards, sibling results, generic
forwarding, nominal identity and local escape limits. Active, empty and nested outcomes
still require owning caller handling after inspecting a borrowed result; dropping,
cloning and unhandled wrapper escapes remain rejected. Imported LSP contracts and fresh
editor repair/native execution cover both storage origins and handling diagnostics.

Six `compiled_source_borrowed_outcome_results_*` models compare original/expanded O0/O2
programs at every fuel cutoff and quota partition. Real resources reside in named-outcome
holders while completed references, awaited parents and stopped siblings retire before
the owning contingency. The 65-variant reference census checks context retirement.
Existing cold task evidence remains baseline. No production scheduler changes accompany
this bounded result admission.

## Record-only source reference chains (0.184)

`source_record_chains` exercises shared/exclusive references through records, every
intermediate holder and supplier loan, queued/running/completed phases, selected result
origins, Copy/move and detached guards, whole and owned results, repeated spawn/join,
siblings, generic forwarding, local escapes and the still-gated tagged-node boundary.
Imported public origins and fresh editor repair/native execution retain both storage
and contained permissions. Superseded record-pointee gates now assert admission.

Seven `compiled_source_record_chains_*` models compare original/expanded O0/O2 executions
at every fuel cutoff and quota partition. Real resources in backing records exercise
owned and borrowed chain captures, completed references, awaited owners and stopped
siblings. The reference context census covers 71 variants, including whole-chain results
and joined/unjoined faults. Production scheduler behavior and conservative host evidence
rules are unchanged; cold carrier/result evidence remains the baseline.

## Tagged source reference chains (0.185)

`source_tagged_chains` covers active/empty option and closed-variant targets, all child
phases, shared/exclusive owned/borrowed roots, selected origins and holder guards,
Copy/move, whole/owned repeated joins, sibling isolation, forwarding and local escapes.
Public contract edits invalidate imported callers; fresh editor repair restores hover
and native execution. Previous option/variant chain admission gates now assert success;
reference-bearing named-outcome nodes and collections retain negative controls.

Eight `compiled_source_tagged_chains_*` tests compare original/expanded O0/O2 executions
at every fuel cutoff and quota partition. Their models include active/empty and whole
results, joined/unjoined faults, awaited parents and real resources in active and
reference-free empty alternatives. The reference context census covers 80 variants.
Production schedulers and conservative host dependency evidence are unchanged.

## Named-outcome source reference chains (0.186)

`source_outcome_chains` covers active/empty/nested named outcomes across shared/exclusive
owned/borrowed roots, queued/running/completed/joined phases, selected suppliers and
holder guards, Copy/move, repeated joins, siblings and generic forwarding. Borrowed
inspection retains caller handling; owned outcome roots transfer obligations. Missing
handling, drop/clone, nominal mismatches and local escapes retain negative controls.
Public-origin edits invalidate callers and fresh editor repairs restore hover/native run.

Nine `compiled_source_outcome_chains_*` tests compare ten original/expanded O0/O2 models
at every fuel cutoff and quota partition. Real resources in active and empty outcome
alternatives retire after borrowers, completed results and stopped siblings beside
awaited parents. The reference context census covers 88 variants. Production scheduler
and conservative host-dependency behavior are unchanged; cold baseline evidence is reused.

## Uninterrupted source boundary helpers (0.187)

`source_boundary_helpers` checks normal and locally recovered named/lexical work
beside queued, suspended and completed children, borrowed parent results, child
capture/result loans, owned handle wrappers and escaped helper faults. Direct
boundaries and transitive boundary-bearing handoff helpers retain negative controls.
Imported/trait helpers use checked bodies; unsaved changes invalidate and repair
callers. Generic closure checks retain the named contingency and its dependency,
diagnostic and recursion evidence even in unused generic definitions.

Four `compiled_source_boundary_helpers_*` tests compare original/expanded O0/O2
models at every fuel cutoff and quota partition. Real helper resources retire on
normal and escaped-fault paths; named local recovery closes its retained resource
while completed results and suspended siblings survive until their original owner
joins or settles them. The reference context census now covers 90 variants.
Production scheduler code is unchanged; admission follows the checked no-handoff
proof, including named contingency bodies.

## Local fault recovery preserves the selected source turn (0.188)

`compiled_local_recovery_preserves_source_turn` compares a legal awaited-helper
program against the reference executor at every fuel cutoff and quota partition,
before/after expansion and at O0/O2. The 0.187 native baseline incorrectly ran an
unaffected sibling before the local contingency. The regression requires recovery
first; a fresh editor checks async hover, unsaved missing-await rejection/repair,
and the exact native emission order in `playground/local_recovery_tasks.tellur`.

`compiled_gated_helper_local_recovery_and_children` extends this comparison to eight
named/lexical, direct-await/spawned-child and outside-success/fault models while
asserting that their reordered source remains rejected. Reference census checks
all retained stores at every cutoff. Spawned resource observers add two native
models proving exact destruction and borrower-before-holder ordering; scripted
reference opaque values separately check calls and storage retirement.

`source_boundary_helpers` also retains direct async/yield and transitive helper
rejection in both contingency forms. The native host regression suite guards the
unchanged host polling protocol, and existing source task/fault/boundary and real
resource cases cover settlement composition. This release changes native owner
resumption and queue retirement, with no new source admission or fault semantics.

## Checked helper boundary handoffs (0.189)

The former E1e.23 gated IR models now compile spawn-before-helper source directly.
Four `compiled_checked_*` tests compare twenty named/lexical, direct/awaited,
recovering/nested-owner and helper-owned-child schedules in original/expanded O0/O2
execution at every fuel cutoff and quota partition. The resource suite adds six
source shapes with real Rust holders and inline/awaited/spawned observers, checking
normal closes, fault dispositions, exact destruction and borrower-before-holder
retirement. Private reference censuses require all retained stores to retire.

`source_boundary_helpers`, `source_tasks` and LSP tests cover public/shared/exclusive
result origins, generic/trait/transitive/constructor helpers, owned handle wrappers,
imported unsaved edits and local recovery. Direct inline boundaries, nonstoppable
retained owners, contingency async calls and unhandled captured outcomes remain
rejected. The runnable `boundary_handoff_tasks.tellur` example has a separate native
fuel/quota comparison and fresh editor hover, unsaved repair and execution checks.
Runtime code remains unchanged from 0.188; this release changes compiler admission.

## Pending adapter operation model (internal on 0.189)

`runtime::continuation::adapter_operation_model` intercepts a checked primitive only
inside its private harness, retaining evaluated operands, a resolved borrowed
supplier and partial input on a test-only continuation. Five cases cover two
overlapping operations in both completion orders, repeated/spurious/duplicate/stale
wakes, atomic rejection of malformed results and exhausted readiness registration,
one-use completion, and owning-fault retirement of waiting/signalled work beneath
ordinary or nested boundary frames. Partial operations retire while their suppliers
are still live; stopped nested contingencies and normal continuations do not run.

A single-operation comparison matches normal scripted reference execution before
and after expansion at every fuel cutoff through success, with three host quota
partitions. It compares complete outcomes, events, diagnostics and fault reports.
The existing runtime census and readiness/settlement regressions remain separate
checks. This model has no live I/O, native adapter implementation, public pending
result, cancellation or fallible cleanup claim. No compiler/LSP/VSIX release is
needed for these test-only branches and documentation.


The E1f.2a `native_host` pending-operation model adds five cases over real native
contexts and registered Rust resources. It covers six overlapping completion/order
and quota schedules; twelve waiting/signalled/result-ready fault or process-stop
schedules; fifteen fuel/quota combinations through normal completion; malformed
completion and readiness-exhaustion atomicity; and rejection of foreign or changed
operation, host, invocation, domain and supplier identities. Lease destructors
assert that borrowed resources remain alive, and traces check exact destruction
counts, no replay, no source fuel on polls/delivery and no late wake revival.

These are private callback models, separate from generated checked-source native
comparisons. E1f.2b below adds delivery, nested-supplier, report and resource-trace
differential evidence; pending external-fault settlement remains a separate gate. The
current model adds no source handoff, manifest mode, live I/O or cleanup policy.


The E1f.2b generated pending-operation cases use the same activation record with a
real Rust resource type and a private fixture-only lowering hook. Two source shapes
compare ordinary/pending native builds at O0/O2, before/after expansion, with normal
and handled-fault input and three quota partitions at every fuel cutoff through
completion. They compare exact native output/reports and resource traces, then
compare the ordinary native result with scripted reference execution (normalizing
only established fuel-message wording). Readiness cycles preserve fuel/effects,
partial input and fixed model deadline; operation creation, real adapter call,
delivery and destruction each occur once. Nested suppliers retire correctly on
post-delivery faults. A zero-quota pause after result completion cannot poll or
complete that operation again.

The E1f.2c generated fault comparison parks two operations beneath lexical and named
boundaries, then explicitly selects the checked owner's operand-free fault callback.
It asserts that ordinary parent admission remains closed. Waiting, signalled and
completed-but-undelivered states compare reference/native complete events, outcomes
and reports in original/expanded O0/O2 builds across three quota partitions and every
remaining-fuel cutoff from fault through handler completion. Setup fuel is separate.
Successful schedules additionally park an independent root and resume it after
owning-domain settlement. Fuel-report wording is the only normalization.

Real Rust lease destructors check live suppliers. The owning contingency's first
adapter call requires both operations and all three stopped resource owners already
retired, while the owning handler's own resource remains live. Per-resource traces
prove exact destruction and borrower-before-supplier order without imposing an
order between independent siblings. Stopped nested contingencies, normal operation
arms and late readiness/operation tickets cannot run. Shared production scheduling
and adapter admission remain unchanged; source-scheduler readiness is the next gate.


The E1f.3a reference source-pump tests use the existing source scheduler with three
pending borrowed adapter shims, two under an owning request and one outside it.
Twenty-four generous-fuel schedules combine normal/escaping-sibling-fault behavior,
FIFO/reverse event order, early/idle-only signals and three host quota partitions.
Partial and empty polls, one-time argument preparation/completion, adjacent source
effects, stale generations, stopped nested contingencies and independent joins are
asserted directly. An additional sweep covers every source fuel cutoff through
success before/after expansion, with exact per-representation event/report/diagnostic
agreement across quotas. Completed cross-expansion effects and dispositions agree while
retaining each report's explicit expansion annotations.

Test-only pumping appends eligible work and supplies readiness only at source
selection or when no runnable producer remains; quotas alone never select a sibling.
All retained value/task/await/domain and source queue/join/handle stores retire.
This is reference scheduling evidence, separate from E1f.2c's generated host-schedule
resource proof. Native source-pump differential evidence is recorded below; public adapter admission
remains outstanding. No live I/O or new production suspension is claimed.


The E1f.3b generated source-pump test builds original/expanded resource programs at
O0/O2 and sweeps every source fuel cutoff through completion for three quotas,
normal/faulting siblings, FIFO/reverse readiness and early/idle-only signals. It
compares complete native/reference events, outcomes and fault reports, normalizing
only existing fuel wording. The reference pump reuses its existing String cases
and accepts an explicit scripted resource scenario for this comparison.

Six real resources span named retained-owner and lexical supplier scopes. Operation
leases assert live suppliers; the owning handler's first typed adapter requires
all four stopped resources and both operations already retired while the two
outside resources remain live. Normal completion calls each typed sample once;
pending faults suppress normal arms and stopped named/lexical contingencies.
Partial/empty polls preserve fuel/effects, duplicate completions and late tokens
are rejected, zero quotas do not replay ready results, and every created resource
is destroyed once at every fuel cut. An independent outside task completes and is
joined after request settlement. The production source driver, primitive bindings
and shared native runtime are unchanged; only the test's generated overlay enables
this pump.


E1f.4a replaces the source-pump experiment's hardcoded partial-input polling with an
owned typed Rust adapter. Four original/expanded O0/O2 builds sweep every source fuel
cutoff, three quotas, FIFO/reverse and early/idle-only readiness, with normal and
faulting source siblings. Each poll first revalidates runtime ownership/supplier
identity, then borrows the actual resource only for the Rust call. Typed completion
uses existing result validation/events and compiled delivery; the synchronous sample
wrapper is a panic trap for replay. Reference events/reports and existing source-turn,
late-token and exact resource-destruction assertions remain in force.

Adapter-state logs additionally require one preparation and destruction per admitted
operation, a fixed deadline across partial/empty polls, state destruction before
result adoption and supplier release, and both stopped states gone at the first
owning contingency call. Seven compile-fail probes reject escaping preparation/poll
borrows, borrowed results, exclusive poll signatures and wrong outcome types. A
positive non-Send probe verifies moved allocation identity and exact destruction at
unpolled, waiting, completed-unadopted and taken-result phases, including rejected
premature/duplicate operations. These are private typed-contract tests; the source
matrix covers waiting/signalled sibling faults. E1f.4b below adds completed-unadopted
externally routed faults and runtime registration-failure integration. Production
adapter admission, host scheduling and the installed 0.189 release are unchanged.


E1f.4b uses the existing generated owning-fault comparison with typed Rust operation
state and a disposal witness around the generated plain-data result. Four
original/expanded O0/O2 builds cover waiting, signalled and completed-unadopted states,
three quotas and every settlement/contingency fuel cutoff, with exact reference
reports/events and independent-root survival. Each run first rejects a foreign
operation ticket and exhausted readiness generation, checking unchanged host/value/
wait/operation observations before successful registration and owning-fault routing.
Late typed polls and readiness signals reject after retirement.

State and candidate witnesses require exact destruction before actual supplier
release and before the owning handler's first typed checkpoint. Completed candidates
remain unadopted; stopped source result arms and nested contingencies do not run,
and the synchronous sample wrapper is a panic trap. Counter-exhaustion injection
covers runtime preflight only; OS backend registration rollback/unregistration and
live transport behavior are not claimed. Existing public compiler behavior and the
0.189 installed release remain unchanged.


E1f.5 overlays the typed source comparison with a deterministic subscription backend.
Each attempted wait tests rejection before insertion, after insertion with an event
already queued, and at runtime generation preflight after backend preparation. A
reservation removes tentative interests on failure; a successful commit transfers
subscription ownership to the operation. Full source/reference event/report checks
run in original/expanded O0/O2 builds through every fuel cutoff, three quotas, both
readiness orders, early/idle-only events and normal/faulting siblings.

The backend rejects tentative and foreign notifications, duplicate signals and
queued events from removed generations while replacement interests are live.
Positive-quota polling removes the old subscription before invoking the typed
adapter; incomplete polls re-register with the original deadline. Fault disposal
removes subscriptions before state and actual suppliers, and owning-contingency
witnesses require complete stopped-state retirement. Every inserted interest has
one removal, all backend entries retire, and remaining queued events reject after
completion. No OS I/O, elapsed-time/timeout behavior, public async adapter mode or
new source handoff is claimed.


E1f.6a compiles an owned HTTP receive/parser state against the independent example's
actual generated Rust outcomes and pinned parser dependency. The synchronous
reference is extracted from the current library source, including error mapping,
bounds and timeout constants; only stream and clock access are substituted. A
deterministic blocking stream and a nonblocking stream consume the same scripted
bytes/errors, with blocking waits represented as readiness pauses in the latter.

O0/O2 builds each pass 1,599 comparisons: every split of the small request corpus,
immediate/delayed data, Interrupted/WouldBlock, framing/header/target/UTF-8 outcomes,
HEAD state, partial EOF and I/O failures, exact/overflow header and count bounds,
and maximum body size. Repeated waits/body progress do not reset the fixed deadline.
Already-read complete data is parsed before checking whether another read may start;
incomplete data expires before a new read. Full typed outcomes/payloads, consumed
bytes and HEAD state agree. Completion and a second receive cannot replay input.
This test uses no live sockets or wall-clock sleeps and does not claim real OS
readiness/timer race ordering or source-scheduler integration. Public compiler and
library behavior, and the installed 0.189 release, remain unchanged.


E1f.6b compiles the unchanged owned receive state with private pinned Mio 1.2.4
and actual generated HTTP outcomes. O0/O2 each pass twelve live-loopback scenarios:
fragmented header/body/UTF-8 input, duplicate receive without reads or HEAD mutation,
empty and partial EOF, the original two-second deadline across body progress and
empty polls, waiting/signalled stop, and unadopted typed-candidate disposal.

The concrete backend retains physical registration while draining reads and uses
fresh one-use logical tokens for each wait. Real collected notifications and an
injected empty hint exercise stale/duplicate/foreign rejection; separate backends
can have colliding token numbers. Exhausted token allocation preserves the current
registration. A new connection actually reuses the old numeric Unix descriptor,
and the copied old notification still cannot claim its live replacement. Successful
OS deregistration and state-wrapper/candidate disposal precede socket release;
all subscriptions retire. Tests tolerate spurious readiness and partial TCP delivery.

This is macOS transport/lifetime evidence, not a source-scheduler differential or
cross-platform guarantee. Actual resets, OS insertion/removal failures and timer/
readiness races remain separate. Failed removal would be recorded and fail the
fixture; no public cleanup fault policy is inferred. The previous 1,599-per-build
deterministic parser comparisons remain applicable because the parser and baseline
are unchanged. No compiler/LSP/VSIX release or public HTTP behavior changes.


E1f.6c overlays the existing typed source/reference matrix with real loopback resources
and persistent Mio registrations. Each operation keeps one physical registration
across tentative/committed logical waits. Every attempted wait injects failures
before/after logical insertion and at runtime generation preflight; rollback removes
the logical interest while preserving the physical owner. Copied tentative, stale,
foreign and duplicate events cannot consume replacements. Generation allocation is
spent before touching OS registration.

Only a selected positive-quota adapter poll obtains the freshly checked shared
resource and drains real bytes to WouldBlock. Notification admission and zero-quota
execution assert no adapter reads. A controlled peer offers A/empty/B and a
non-consuming test-only peek barrier confirms data arrival plus a real OS event;
this makes the existing full reference event/report comparison deterministic. The
model deadline remains 57, not elapsed HTTP time.

Original/expanded O0/O2 matrices sweep all fuel cutoffs, three quotas, both readiness
orders, early/idle-only signals and normal/faulting siblings. Per-operation traces
require exactly one physical registration/removal, every logical removal before
physical removal, and physical removal before supplier release and source result
adoption. Stopped state retires after deregistration; real sockets and affected
resources are gone at the first owning checkpoint. The independent outer task
survives and nested contingencies stay suppressed. HTTP parser/deadline source
composition, concrete routed-fault candidate disposal and actual backend/reset
failures remain separate. The installed 0.189 release is unchanged.


E1f.6d combines the controlled routed-owner fault matrix with actual loopback reads
and persistent physical registration. Each checked partial poll first publishes and
signals a logical runtime wait, consumes eligibility and removes the logical
subscription. Real partial/A/B input is consumed through a fresh shared supplier
borrow without changing source fuel, events or cursor. The peer barrier remains
explicit test machinery.

Both nested operations then remain waiting, signalled, or completed with an unadopted
typed candidate. All three cases retain a real registration. Logical insertion and
runtime-generation rejections leave the operation available for valid registration;
foreign/late tickets cannot perform I/O. Original/expanded O0/O2 cases compare full
reference events and fault reports through settlement fuel cutoffs, three quotas
and independent-root survival. Traces require each logical removal before physical
deregistration, and state/candidate disposal before supplier release. The owning
checkpoint requires actual socket destruction; stopped nested handlers and result
arms remain silent. Existing source and HTTP/parser matrices are unchanged and
reused. Real HTTP deadline/source composition and actual OS/reset failure handling
remain separate; no public release changes.


E1f.6e compares the exact HTTP receive signature and all nine generated alternatives
through retained checked continuations. Twelve deterministic transport cases cover
immediate/delayed input, split UTF-8, HEAD state, protocol failures, EOF, repeated
empty waits and original header/body deadline expiry. Each source invocation takes
only its declared shared connection. Generated encoders and the host validator
preserve complete request fields, including the headers map; malformed result
shapes reject without recording an event. The second invocation returns the existing
duplicate-start failure without reading more input.

Original/expanded O0/O2 builds compare full ordinary-native output with scripted
reference events/reports at every source fuel cutoff, then compare retained native
execution under three host quota cycles. Notification/zero-quota probes preserve
input position, read count and clock. State is consumed once; repeated poll/completion
reject, and state plus the loan witness retire before actual supplier destruction.
A trap prevents synchronous-wrapper replay. Reference scripts use the baseline
synchronous results, with separately asserted alternative labels and duplicate-start
payload; unchanged E1f.6a parser cases provide broader protocol/deadline evidence.
This is a sequential retained chain, not concurrent HTTP scheduling or a public
async-host release. Live source/parser/deadline and HTTP routed-fault composition
remain separate.


E1f.6f adds a private reference HTTP profile on the existing cooperative source queue.
All nine typed outcomes cover immediate/delayed left completion, normal and sibling-
fault paths, FIFO/reverse readiness, early/idle-only hints and three quota cycles.
The specimen retains three independent connection owners through nested named
boundaries. Ready left completion must precede the sibling's first queued effect and
must have zero actual readiness registrations. Delayed operations acquire exactly
their declared fixture wait counts; empty host quotas preserve state and event order.

Invalid labels, arity, scalar fields and nested-map payloads preserve pending
operations, original suppliers, values, fuel, events and script position. Normal
results bind once. Fault settlement retires pending operations before suppliers and
the owning contingency, suppresses nested handlers, preserves completed work and
allows the independent child to join. Every saved readiness generation is stale
at completion. Representative request/timeout/failed cases compare exact events and
reports through every fuel cutoff and quota partition before/after expansion; final
effects agree across representations after normalizing intentional fault provenance.
The affected sample reference tests are rechecked. Native/parser evidence is reused
separately: this reference profile uses trusted completion/readiness plans and does
not implement HTTP parsing, a timer policy or public async-host admission.


E1f.6g connects the unchanged owned HTTP parser to native source scheduling with
actual Rust connection owners. Source opens three canonical resources; explicit
reference identity maps supply independent typed outcomes and readiness counts.
Initial ready polling stays selected without a synthetic wait. Actual parser waits
park and return selection to the source queue; later eligible positive-quota polls
resume with a fresh checked supplier borrow. Notifications and empty quotas perform
no reads. Every wait preserves the original Instant deadline, including expiry
before a late input event can be consumed.

Original/expanded O0/O2 matrices compare all nine outcomes across immediate/delayed,
normal/fault, early/idle-only and FIFO/reverse schedules. Representative request,
fixed-deadline timeout and stopped-failure schedules compare every fuel cutoff and
three quotas with complete reference events/reports. Typed request headers/body and
diagnostic payloads match independently specified fields. State is consumed once,
then disposed before real supplier destruction. The first owning checkpoint requires
affected connections gone and independent work live; stopped nested handlers and
result arms remain silent. The synchronous receive wrapper cannot be replayed.
This uses simulated transport/clock. Live HTTP registration, completed-but-unadopted
HTTP fault candidates remain separate lifetime audits. Repeated operation identity
is checked below. No public adapter mode or release is introduced.


E1f.6h reproduces the private HTTP pump's retained completed-ticket stall with a
bounded driver, then checks three consecutive receives in one retained context.
The active ticket map is rebound for every operation. Saved tickets reject against
the replacement; stale readiness rejects while its new wait is live without changing
wait state, host accounting or input. Reference completion/wait plans are consumed
per operation and progress is owned by the pending continuation. Duplicate receive
must report the existing failure message without another read, input advance or
clock change.

Original/expanded O0/O2 matrices compare complete events/reports for request/timeout,
immediate/delayed first receives, FIFO/reverse and early/idle-only hints, all fuel
cutoffs in representative schedules and three quota cycles. A fault while the second
receive waits checks completed-state preservation, replacement disposal before both
real suppliers and owning contingency, stopped nested-handler suppression and outside
child survival. The full previous HTTP queue matrix and affected reference preparation
cases are rechecked. Transport remains simulated; live registration and unadopted
HTTP candidates are distinct follow-ups, and public admission remains pending.


E1f.6i composes the unchanged owned HTTP parser with actual nonblocking loopback
sockets and the native source queue. Initial ready polling needs no physical
registration. Actual `WouldBlock` registers a wait and persistent readable interest;
rearms use fresh backend tokens and runtime wait generations. Old/foreign/duplicate
notices reject while replacements are live. Notifications and zero quotas perform
no reads. Controlled peer writes, bounded non-consuming barriers and deliberately
empty hints produce real partial-header/body/UTF-8 reads and repeated `WouldBlock`.

Original/expanded O0/O2 matrices compare full events/reports through every source
fuel cutoff in representative normal/fault schedules and three quota partitions,
plus FIFO/reverse, early/idle-only and immediate-first schedules. Repeated receives
use one retained context; the duplicate fails without reading. Runtime wait-removal
instrumentation requires logical retirement before physical deregistration, state
disposal and supplier/socket destruction. Owning faults preserve completed state,
suppress nested handlers and retain independent work through the checkpoint.

A separate eight-run real-deadline matrix uses partial input followed by no bytes,
ready outside connections, original/expanded O0/O2 and two quota cycles. It compares
the actual two-second expiry and full events with reference plans, preserving one
original Instant through both waits. This deliberately does not multiply real waits
across every fuel cutoff or define multi-timer arbitration. Existing standalone live
receive/parser and simulated-transport matrices are reused where inputs are unchanged.
OS deregistration failures fail the fixture; public error policy, unadopted HTTP fault
candidates and async-host source admission remain separate.


E1f.6j combines the live HTTP host/parser with the checked original-owner fault
fixture. Four internal states cover partial waiting, accepted notification before
another poll, a held typed result, and an encoded completion before checked-arm
adoption. Both a complete request (including headers and split UTF-8 body) and a
malformed diagnostic retain their actual generated payload. Destruction inspects
that payload while the original supplying connection remains live; encoding is
one-use and does not create a substitute payload merely to witness cleanup.

Original/expanded O0/O2 comparisons check exact events/reports at every settlement
fuel cutoff and three quota cycles, plus independent-root survival. Waiting logical
registrations retire before physical interest, parser state and suppliers. Completed
receives already removed physical interest; administrative candidate holds never
recreate it. Candidate disposal precedes supplier destruction and the owning
contingency. Stopped normal arms/nested handlers stay silent, and old tickets and
readiness reject. Unsignalled waits reject advancement even at zero quota; accepted
notifications and zero quotas never read input.

Held typed results have no receive event. The existing private completion helper
publishes an event when validating an encoded result, so those cases preserve that
event while still suppressing arm adoption. The reference explicitly distinguishes
these observations; no new public event-timing policy or source handoff is implied.
Public async-host admission remains pending. Existing parser/source/deadline evidence
is reused where helper bodies are unchanged; compiler/LSP/VSIX stays 0.189.


E1f.6k adds three actual TCP reset cases at each of O0/O2: no request input,
partial headers and complete HEAD headers with a partial body. An abortively closed
owned loopback peer produces a read error; the observer requires ConnectionReset
and a raw OS error, not EOF or a close readiness flag. The unchanged owned parser
and shared synchronous error mapping return disconnected in all three cases.

The probes retain the original deadline, consumed-byte count and established HEAD
state, reject duplicate receive without reading and reject saved notices after
completion. Physical deregistration precedes state/socket destruction. The earlier
ordinary-EOF, full parser, live source-queue and candidate matrices remain separately
recorded evidence. These are bounded standalone macOS runs, not general reset timing
or source-queue error coverage. The private fixture adds the exact cached libc
version from its existing lock for peer configuration; production dependencies and
compiler/LSP/VSIX 0.189 are unchanged.


E1f.6l supplies complete HEAD headers and a partial UTF-8 body to the left source
operation, then abortively closes its owned peer. The unchanged source driver
accepts readiness without reading; checked polling subsequently observes an actual
ConnectionReset/raw OS error and the existing disconnected outcome. Full native
events and fault reports compare with the independent two-wait reference plan.

Original/expanded O0/O2 cover normal schedules with FIFO/reverse and early/idle-only
hints. A representative schedule compares every source fuel cutoff and three quota
partitions. A reset notification followed by fuel retirement may
produce no read error or receive event; successful completion requires one actual
reset read. The original deadline, established HEAD flag and partial byte count are
preserved. Following operations retain distinct identities in the same context, and
duplicate receive performs no read. Wait removal precedes deregistration, state and
supplier/socket release. Fuel failure disposes pending operations before suppliers;
normal execution joins independent work.

This is the controlled partial-body source case on macOS; other reset positions,
ordinary EOF, real deadline expiry, sibling faults and unadopted candidates keep
separate evidence. The existing repeated-operation sibling-fault profile assumes
an immediate first receive and is not widened here to pending-first-receive faults.
General backend/cleanup-error policy and public async-host admission remain pending.
The maintained transport and compiler/LSP/VSIX 0.189 are unchanged.


E1f.6m extends the repeated HTTP fixture's immediate-first fault assumptions to a
fault while the first two-wait receive is still pending. Explicit reference counts
require one prepared/retired first operation and no completed first operation or
prepared successor. Idle-only hints permit one poll/wait; an early batch permits
two. The unstarted second resource still releases before the owning contingency.
Both resources release exactly once; prepared states drop exactly once, and no
state-drop or physical registration is invented for an unstarted operation.

The new reference matrix compares request/disconnected plans, original/expanded
programs, FIFO/reverse and early/idle hints, all fuel cutoffs and three quota cycles.
The affected deterministic repeated, live source and reset-capable native matrices
add pending-first faults at O0/O2 while preserving their previous cases. Exact
reference events/reports, no stopped result adoption, suppressed nested handlers,
independent work and logical-wait/physical-interest/state/supplier retirement remain
checked. The reset-capable peer never reaches its abortive close in this fault
profile; early hints supply partial body bytes, not a reset error or completion.
Existing actual reset cases retain their separate requirements.

Shared checkpoint changes also trigger rechecks of the earlier HTTP source and
candidate matrices. This is a private test-profile correction and extension, with
no production scheduler, language, parser or maintained transport change. Public
async-host admission remains pending; compiler/LSP/VSIX stays 0.189.


Release 0.190 (P2x) preserves authored schema field mapping ranges: primary brackets
or storage arrows and separate referent brackets in structs, variants and named
outcomes. Exact CLI/imported generic tests cover invalid groups, non-group entries,
arity and absent reference origins, multiline comments, legacy fields and unchanged
shared preparation order. Unsaved dependency LSP/editor checks verify UTF-16 ranges,
stale-hover suppression and repair. Missing mappings and declaration-level errors
retain fallback points. Origin acceptance and the shared diagnostic budget are unchanged.


Release 0.191 (P2y) selects the repeated declared group for a duplicate and the
individual declaration for an unused origin group. Structs, variants and named
outcomes retain original group ranges through generic copies and module offsets.
Exact CLI tests cover internal Unicode comments, CRLF, transitive imports, unused
generics, declaration order, field-error priority and unchanged global schema limits.
Unsaved dependency LSP/editor checks verify UTF-16 ranges, stale-hover suppression,
repair and normal native execution. Source syntax, accepted origin schemas and
first-error shared preparation remain unchanged.


## Shared origin-schema preparation recovery (0.192 / P2z)

Independent struct, variant and named-outcome origin-schema errors now collect at
the existing schema preparation points. The original recursive visitor and its
first diagnostic are preserved. Failed active chains and their transitive/diamond
dependents are retired; successful private cache entries never escape a failed
phase. Ambiguous identities retain the first-error barrier, and all batches use
the existing 32-error budget before later preparation or body checking.

Diagnostic tests cover independent and dependent declarations, generic/reference/
option edges, valid shared children, named outcomes, cycles, depth/group limits,
ambiguous identities, ordinary recursive layouts, earlier public-shape/bound phases,
first-error API compatibility, imported Unicode/CRLF ranges and budget exhaustion.
LSP and isolated editor checks cover unsaved partial repair, exact dependency ranges,
stale-hover suppression, full repair and normal native execution. The affected
frontend, trait, generic, module and origin suites retain normal acceptance and
native comparisons. Runtime and public async-host admission are unchanged.


## Contributor statement and outcome locations (0.193 / P2aa)

Finished diagnostic batches now enrich supported contributor notes from exact parser
statement/outcome anchors. Imported source ranges offset and return both endpoints;
generic definitions keep authored spelling. Matching uses no lowered-name length or
new lineage inference. Note construction, ordering, deduplication and omission bounds
finish before range enrichment. Declaration and omission notes retain point fallback;
public absorption snapshots and executable IR are unchanged.

CLI tests cover multiline Unicode/CRLF producing exits and forwarders, transitive
imports, generic definitions, nested outcome expressions, received capture statements,
first-error API compatibility, declaration fallback and long-chain omission bounds.
LSP and isolated editor checks update unsaved dependency ranges, suppress stale hover,
restore after repair and run the repaired program natively. Existing contributor,
absorption, contract-snapshot, module, generic-control and public-contract tests verify
unchanged evidence and execution; manual, formatting and all-target checks accompany
the release. Public async-host admission remains pending its recorded design choice.


## Public inferred-contract locations (0.194 / P2ab)

The parser retains the first authored inference clause in each control header:
`exits auto` or an individual `label...`. Public-shape diagnostics use this range
without changing acceptance, declaration ordering, phase barriers or the 32-error
budget. Mixed/wrapped headers preserve written order even when label names sort
differently. Both module-resolution paths retain both endpoints; synthetic boundary
and constructor contracts default to no inference range.

CLI tests cover modern/legacy/async spelling, multiline Unicode/CRLF comments,
unused generics, transitive imports, independent repairs and later body errors.
Private inference still compiles; trait and legacy payload-inference barriers remain
separate. A focused unit test checks the missing-metadata control-name fallback.
LSP and isolated editor tests cover unsaved partial repair and exact imported UTF-16
ranges, hover restoration and native execution. Affected diagnostics, formatting,
payload inference, public contracts, traits, generics, modules and manual checks
accompany the release; the ownership/runtime and pending async-host decision are
unchanged.


## Missing public-origin contract locations (0.195 / P2ac)

Missing public borrowed/aggregate result contracts select the authored result type.
Named completion expansion copies schema fields, so these errors instead select the
completion type at the control's use. No range is fabricated for an absent clause;
synthetic metadata falls back to the original control-name point.

CLI cases cover shared/exclusive results, borrowed carriers with an existing `from`,
unused generic results/completions, named aggregate payloads, multiline Unicode/CRLF,
and transitive imports whose schema and use occur in different files. Partial
repairs preserve independent errors and expose body errors only after contracts
validate. First-result ordering, first-error APIs, earlier schema/shape barriers,
the 32-error budget and existing invalid-binding locations remain checked. A focused
unit case checks inline and copied-schema fallback. Fresh LSP and isolated editor
checks exercise exact unsaved UTF-16 ranges, partial repairs, stale-hover suppression,
full repair and native execution. The selected public-contract, origin, generic,
trait, module and manual suites accompany the release. Public async-host admission
still awaits its recorded design choice.


## Declared contributor clause locations (0.196 / P2ad)

For ordinary controls, parser metadata retains the full explicit exit clause separately from body, named
type and origin ranges. Declaration notes select that clause after aggregation;
raw absorption evidence, ordering, deduplication and omission bounds do not change.
All declared alternatives share their written clause, with the note naming the
relevant alternative. Named contracts point to their use, not their shared schema.

CLI cases cover inline/wrapped/legacy headers, generic payloads, named contracts,
Unicode/CRLF and exclusion of following fault annotations. Transitive imported
generic named contracts retain both endpoints in the original use file. A focused
internal case clears metadata and confirms the control-name fallback. LSP and
isolated editor checks cover unsaved header edits, distinct body/forwarding ranges,
stale-hover suppression, repair and native execution. Contributor, absorption,
contract snapshot, module, generic, public-contract, formatting and manual suites
accompany the checkpoint. The pending public async-host decision is unchanged.


## Independent implementation preparation (0.197 / P2ae)

A read-only identity/coherence preflight enables recovery for independent member
inventories and method profiles. Valid unique trait/type pairs retain source-order
checking, one error per implementation and the 32-error budget. Invalid or duplicate
identities use the original interleaved validation order, so later ambiguity never
hides an earlier local error. All successful preparation remains private until the
whole phase passes; associated normalization and substitutions run afterward.

Focused diagnostics cover missing/extra/duplicate members, invalid method profiles,
unused definitions, partial repair, earlier schemas/traits, later cycles/conformance
and body errors, the 1024-implementation limit, imports and foreign ownership.
First-error APIs retain the first diagnostic. LSP and isolated editor cases repair
independent unsaved implementations, follow UTF-16 coordinates after a preceding
emoji changes position, suppress stale hover and execute the repaired trait calls
natively. Trait, generic-trait, aggregate-origin, module, generic-control, public
contract and manual checks accompany this slice. Existing async/fault/contingency
permissions and the pending public async-host decision remain unchanged.


## Independent associated-type normalization (0.198 / P2af)

Normalization keeps completed values, active recursion and failed dependencies
separate. It collects original independent failures in the existing canonical
trait/type and written member order, suppresses failed lexical/qualified selections,
and retires only the active path entries owned by each recursive invocation.
Substitution remains atomic; no successful sibling exposes a partial witness.

Diagnostics cover independent owned-type violations, lexical diamonds, computed
selection targets, cross-implementation cycles, missing members/implementations,
partial repair, depth/type-size limits, the 32-error cap, first-error compatibility,
transitive imports and earlier/later phase barriers. Repaired aliases execute in
the reference and expanded interpreters. LSP and isolated editor checks repair
unsaved imported assignments, suppress dependent aliases and stale hover, track
UTF-16 points after preceding emoji edits, and execute repaired code natively.
Selected trait, bound, generic, aggregate-origin, module, public-contract, async,
fault, contingency and manual suites accompany this release. The public async-host
call model remains an unanswered design question.


## Independent implementation permissions (0.199 / P2ag)

A read-only preflight collects permission failures only after every implementation
identity, local member and associated type has prepared. Canonical trait/type order,
written method order and async-before-fault-before-contingency priority preserve the
first diagnostic. Each implementation contributes at most one error, within the
shared 32-error limit. Failed phases expose no substitutions, generated methods,
visibility entries or witnesses; accepted permission propagation is unchanged.

Tests cover mixed independent failures, multiple invalid methods, async equality
in both directions, fault/contingency subsets, inherited generic permissions,
partial repair, later bounds/origin/signature/body barriers, earlier preparation,
unused implementations, transitive imports and first-error compatibility. LSP and
isolated editor checks repair async and fault annotations in an unsaved dependency,
track UTF-16 points after preceding emoji text, suppress stale hover, and execute
repaired trait calls natively. Trait, generic, aggregate-origin, module, public
contract, async, fault, contingency and reference checks accompany this release.
The pending public async-host call-model decision remains separate.


## Independent implementation conformance (0.200 / P2ah)

Each implementation stages its validated trait-side signatures privately. The common
collector preserves canonical implementation order and each implementation's existing
capability/type/origin/signature priority, producing one original error per failed
implementation within the 32-error cap. Signatures publish only when all checks pass.
The earlier normalization/permission/schema phases and later argument/body phases
remain separate. Abstract witnesses keep dependency-reachable method handling.

Focused cases cover mixed associated-bound, parameter, payload, direct-origin and
contained-origin failures, trait declaration method order, unused implementations,
partial repairs, named contracts, generic witnesses, imports, first-error APIs and
the budget. The permission-to-origin regression now expects both independent origin
errors after permission repair. Repaired generic calls match reference and expanded
execution. LSP and isolated editor cases check mixed bound/signature errors, exact
UTF-16 imported points, partial repairs, suppressed stale hover, restored trait-side
parameter names and native calls. Selected trait, bound, aggregate-origin, generic,
module, public-contract, async/fault/contingency and reference suites accompany this
release. The public async-host call-model question remains unanswered.


## Implementation completion locations (0.201 / P2ai)

Completion-shape conformance errors with matching parameters and direct/contained
return-origin mismatches now select the actual implementation's written `exits`
clause. Named completion binding preserves use-site metadata separately from copied
schema fields. Parameter mismatches, child origin diagnostics and absent metadata
keep their previous priority and locations. Conformance acceptance is unchanged.

CLI cases cover multiline alternatives, Unicode/CRLF, fault/body exclusions,
direct and contained origins, imported generic named contracts, explicit mappings,
generic callers, parameter priority, child ranges, fault-only methods, canonical
batch ordering, budgets and first-error compatibility. A unit case removes the
metadata to verify synthetic fallback. LSP and isolated VS Code cases change two
imported implementations without saving, check full UTF-16 clause ranges through a
partial repair, suppress stale hover and restore trait-side names and native calls.
Selected trait, associated-bound, aggregate-origin, generic, module, public-contract,
async/fault/contingency and reference suites accompany the release. The recorded
public async-host call-model question remains unanswered.


## Implementation parameter locations (0.202 / P2aj)

Equal-length implementation signatures now select the first actual parameter type
that differs from the trait contract. The existing field range helper preserves
written ownership modifiers, generic arguments and associated/self aliases, while
missing metadata or different arities retain the method-point fallback. Earlier
origin comparisons and the parameter-before-completion shape priority are unchanged.

CLI cases cover modern and compatibility annotations, multiline Unicode/CRLF,
first differing positions, nested and associated types, transitive imports, generic
callers, repair, missing/extra parameters, child and outer origin precedence, batch
order, budgets and first-error APIs. The earlier completion-priority regression now
expects the precise parameter type. A synthetic unit case removes type metadata.
LSP and isolated VS Code exercise independent imported parameter failures, unsaved
partial repairs, full UTF-16 annotations, suppressed stale hover, restored trait names
and native calls. The prior mixed bound/signature editor case is updated and rerun.
Selected trait, bound, aggregate-origin, generic, module, public-contract, async,
fault/contingency and reference checks accompany the release. The public async-host
call-model decision remains pending.


## Associated assignment locations (0.203 / P2ak)

Associated assignments retain a written RHS range through parsing, module offsets,
normalization-root diagnostics and capability requirements. Normalized bound types
carry their original annotation separately from their canonical spelling. Module
errors attach ranges before remapping; normalization recovery and caching are
unchanged. Identity errors and absent metadata preserve their existing points.

CLI cases cover multiline reference/nested/qualified types, comments and CRLF,
capabilities, missing selections and implementations, original imported alias uses,
module visibility/lookup, generic callers, root versus dependency attribution,
cycles, suppression, phase order, repairs, canonical batches and budgets. A unit
case removes metadata before normalization and bound checking. Earlier normalization
and mixed bound/signature regressions now expect RHS ranges. LSP and three isolated
VS Code fixtures cover unsaved partial repairs, UTF-16 endpoints, restored trait
names and native calls. Selected formatter and public contract snapshots join the
trait, generic, module, origin, permission and reference suites because syntax
metadata changed. Later instantiated-type and public-visibility locations remain
follow-ups. Public async-host admission still awaits the recorded design decision.


## Concrete associated-type locations (0.204 / P2al)

Lowered concrete associated types retain their written RHS annotations for manifest
type validation and public-interface visibility. Abstract witnesses keep synthetic
metadata, and the earlier generic-instantiation input still carries its existing
type/span pairs. Target fallback, capability-before-type, type-before-method and
body-before-visibility ordering are unchanged.

CLI cases cover nested unknown types, local/qualified aliases, original imported
Unicode/CRLF ranges, generic witnesses, canonical ordering, the 32-error cap,
first-error APIs and the existing single-error visibility barrier. A unit case
removes assignment metadata and checks synthesized abstract witnesses. LSP and
isolated VS Code cases cross from two concrete type failures through partial repair
to a later visibility failure, then restore trait-side hover and native execution.
The previous assignment-range editor fixture remains a regression. Public
async-host admission still awaits the recorded design decision.


## Associated generic instantiation locations (0.205 / P2am)

Trait-required type inputs retain associated annotations through generic layout
instantiation. Target, ordinary generic argument and expected-signature requirements
retain synthetic metadata and original fallback points. The extra-type loop attaches
ranges only to matching fallback diagnostics; shared declaration/schema validation,
region signature/body requirements and specialization ordering remain unchanged.

CLI cases cover arity, missing and invalid arguments, nested/qualified/local aliases,
imported Unicode/CRLF ranges, generic witnesses, declaration/signature/body priority,
earlier invalid-target rejection, later capability errors, canonical first-error
ordering and repeated repairs. A unit case removes metadata for each error class.
LSP and isolated VS Code cases repair the first imported assignment to expose the
next failure, then restore checked trait-side hover and native execution. The prior
concrete-type/visibility editor fixture remains a regression. Generic-data budget
and specialization checks join the selected suites. Public async-host admission
still awaits the recorded design decision.


## Implementation target locations (0.206 / P2an)

Implementation targets retain a parsed range around the type after `for`. Module
lookup attaches that range before remapping, and preparation applies it to concrete
owned-target profile and declared-type shape errors. Shared identity preflight and
interleaved replay still use the same checks; unknown traits, coherence and member
errors retain their original priority and locations.

CLI cases cover unknown/unapplied/wrong-arity targets, dependent selections, array
lengths, multiline borrowed types, comments, imports, private and missing qualified
names, first-error APIs, member/identity/coherence ordering, repairs and the 1024
implementation limit. A unit case checks missing metadata and precise child ranges.
Two earlier target-priority regressions now expect target ranges. LSP and isolated
VS Code cases repair the first invalid target to reveal the next, then restore
trait-side hover and native calls. The previous independent preparation fixture
remains a regression. Public async-host admission still awaits the recorded decision.


## Implementation trait-name locations (0.207 / P2ao)

Implementation trait names retain a parsed range independently from target types.
Module name lookup attaches it before remapping, and core identity checking uses it
for unknown-trait failures. Unresolved bare names still reach core E_TRAIT checking;
qualified/private/wrong-kind lookups preserve their existing codes and phases.

CLI cases cover missing and qualified names, comments, namespace-only and wrong-kind
uses, imported privacy, original Unicode/CRLF ranges, single-error APIs, trait-before-
target lookup, member/coherence interleaving, repairs and implementation budgets.
A unit case checks absent metadata, distinct target ranges and precise children.
The previous target-priority case now expects a range for its unknown-trait branch.
LSP and isolated VS Code cases reveal the next invalid trait after partial repair,
then restore trait-side hover and native calls. The previous target-range fixture
remains a regression. Public async-host admission still awaits the recorded decision.


## Duplicate trait-member locations (0.208 / P2ap)

The shared uniqueness traversal accepts original member-name positions while method
parameter, exit and payload uniqueness retains its existing point diagnostics.
Member names are still unqualified parser identifiers at preparation time; imported
spans have already been remapped, and later method lowering cannot affect the range.

CLI cases cover associated/control/cross-kind duplicates, longer identifiers,
associated-before-control order, repeated names, async modifiers, imported repairs,
Unicode/CRLF coordinates, first-error APIs, declaration/identity/coherence priorities,
independent batches and the 32-error/1024-implementation limits. A unit case removes
each kind of member metadata in declarations and implementations to check fallback.
LSP and isolated VS Code repair declaration errors before implementation batches,
then restore exact checked hover and native calls; the previous trait-name fixture
remains a regression. Public async-host admission still awaits its recorded decision.


## Duplicate trait-signature field locations (0.209 / P2aq)

Syntax fields retain a parsed name range separately from type and origin metadata.
Both field spellings and compatibility bindings capture the actual identifier;
module remapping preserves both endpoints. Generated constructor results, manifest
payloads and checker adapter fields carry empty metadata. Cloned fields retain their
authored ranges through rewriting. The shared uniqueness traversal accepts complete
ranges and selects them only for trait parameter and inline payload duplicates.

CLI regressions cover both spellings, comments, long identifiers, borrowed payload
origins, transitive imports, Unicode/CRLF coordinates, repairs, first-error APIs,
profile/member/type/identity priorities, independent batches and budgets. Two unit
cases cover missing metadata and preservation after field renaming. Ordinary-control
and exit-label diagnostics keep their existing points. The earlier member regression
now expects the new parameter/payload ranges while retaining the exit-label point.
LSP and isolated VS Code repair parameter and payload errors across declarations and
implementations, then restore checked hover and native execution. Constructor and
named-outcome suites exercise shared field carriers alongside existing trait/generic
coverage. Public async-host admission still awaits its recorded decision.


## Duplicate trait exit-label locations (0.210 / P2ar)

Syntax outcomes retain a parsed name range independently of payload and origin
metadata. Control exits, variants and named outcome alternatives capture names;
module remapping preserves both endpoints. Generated constructor and manifest
completions use empty metadata; named completion binding preserves cloned ranges.
Only trait method exit uniqueness consumes the new ranges. Existing ordinary and
nominal diagnostics remain separate.

Six CLI regressions cover qualified/wrapped spellings, comments, long names,
transitive imports, Unicode/CRLF, repairs, first-error APIs, profile/member/type/
identity priorities, coherence replay, independent batches and budgets. Three unit
cases cover missing metadata, renaming, nominal carriers and completion binding.
LSP and isolated VS Code exercise partial declaration and implementation repairs,
multiline UTF-16 ranges, stale-hover suppression, restored hover and native calls.
Shared constructor, generic, module and outcome suites cover generated/cloned
carriers. Public async-host admission still awaits its recorded decision.


## Trait inferred-contract locations (0.211 / P2as)

Trait method-profile rejection uses the existing first inference-clause range only
for modern nongeneric methods. Structural restrictions keep their method point;
parser acceptance, module remapping and ordinary public-shape handling are unchanged.
The diagnostic is created locally before attachment, avoiding a mismatch with the
public-shape helper's control-name fallback.

Five CLI cases cover auto/payload clauses, mixed/wrapped lists, qualifications,
comments, Unicode/CRLF, imports, repairs, first-error APIs, structural/member/type/
identity/coherence priorities and both budgets. Two units cover absent metadata,
structural fallback despite present ranges, and names rewritten after parsing.
The earlier target-priority regression now expects the inference range while
preserving coherence points. LSP and isolated VS Code cover partial declaration
and implementation repair, UTF-16 ranges, hover recovery and native execution.
No public async-host call model has been selected.


## Generic trait-method profile locations (0.212 / P2at)

Syntax regions retain the authored control type-parameter clause separately from
parameter names and bounds. Normal controls capture both angle brackets; generated
boundary signatures and constructor-inherited parameters use empty metadata. Both
module resolution paths remap endpoints. Trait profile rejection selects the generic
clause before inference, with legacy and missing-metadata method fallbacks.

Five CLI cases cover bounded/multiline/qualified clauses, comments, Unicode/CRLF,
imports, repairs, first-error APIs, generic/inference/member/type/identity/coherence
priorities and budgets. Three units cover missing metadata, legacy fallback,
rewritten parameter/bound names, cleared specialization parameters and generated
regions. Three earlier priority tests now expect generic ranges. LSP and isolated
VS Code repair generic clauses before exposing inference errors, then restore
checked hover and native execution. Constructor and borrowed-constructor suites
exercise the shared Region initializer. Public async-host admission remains pending.


## Legacy trait-method keyword locations (0.213 / P2au)

Syntax regions retain the parsed `control`/`region` keyword after optional
annotation and async parsing. Generated boundary signatures and constructors use
empty metadata. Both module resolution paths remap endpoints. Only the existing
legacy trait-profile rejection selects this range, ahead of generic/inference
restrictions; missing metadata preserves the enclosing method point.

Five CLI cases cover annotated/commented keywords, Unicode/CRLF imported source,
repairs, first-error APIs, ordinary legacy acceptance, parser barriers, profile,
member/type/identity/coherence priorities and diagnostic/implementation limits.
Three units cover missing metadata without inference fallback, rewritten names,
retained metadata after repair, actual keyword tokens and generated signatures.
Two earlier CLI priority tests now expect legacy keyword ranges; two earlier
metadata tests explicitly remove keyword metadata for their point fallback.
LSP and isolated VS Code tests repair unsaved imported legacy declarations and
implementations before remaining inference errors, then restore hover and native
execution. Public async-host admission remains pending its recorded decision.


## Implementation async-permission locations (0.214 / P2av)

Syntax regions retain the parsed async modifier independently of the declaration
keyword. Synchronous and generated boundary/constructor signatures use empty async
metadata. Both module paths remap endpoints. The existing implementation permission
preflight selects extra `async` or missing-async `control`, preserving point fallback.

Five CLI regressions cover both directions, annotations/comments, imported Unicode
and CRLF ranges, repair, first-error compatibility, earlier preparation barriers,
async/fault/contingency/bound/conformance replay, canonical implementation and written
method order, diagnostic/implementation budgets and ordinary async/parser behavior.
Three units cover selected metadata fallback, rewritten names, equal permissions and
generated signatures. One prior associated-assignment priority test now expects the
async token range. LSP and isolated VS Code repair both mismatch directions in unsaved
imports, withholding stale hover until repair and then running the native program.
The first imported fixture incorrectly marked an async trait contingent; correction
preserves the existing separate rejection without changing production semantics.
Public async-host admission remains pending its recorded decision.


## Implementation fault-permission locations (0.215 / P2aw)

The common parsed `faults` consumption retains a token range for both suffix and
fault-only control contracts. Absent/inferred permissions and generated boundary or
constructor signatures use empty metadata. Both module paths remap endpoints. Only
the existing implementation fault-permission rejection selects that range.

Five CLI cases cover the two forms, annotations/comments, Unicode/CRLF imports,
repair, first-error compatibility, async/fault/contingency/bound/conformance replay,
earlier preparation and profile barriers, canonical implementation and written method
order, diagnostic/implementation limits, inherited narrower permissions and separate
ordinary fault/parser behavior. Three units cover missing metadata without selecting
other available ranges, rewriting and permission narrowing, and generated/implicit
metadata. One previous async-priority regression now expects the faults range.
LSP and isolated VS Code repair independent unsaved implementations through fault-only
and suffix forms, suppress stale hover and recover checked hover/native execution.
Public async-host admission remains pending its recorded design decision.


## Implementation contingency-permission locations (0.216 / P2ax)

`Parser::control` retains the full `@contingent` range from `@` through
`contingent`, including intervening comments and line breaks; top-level annotated
controls retain the same range. Unannotated, inherited, generated boundary-handler
and constructor signatures use empty metadata. Both module paths remap endpoints.
Only the existing implementation contingency-permission rejection selects it.

Five CLI cases cover compact, separated and split annotations with async, fault and
fault-only contracts, Unicode/CRLF imports and repair, first-error compatibility,
async/fault/contingency priority, earlier preparation and coherence barriers,
bound/conformance replay after repair, canonical implementation and written method
order, diagnostic/implementation limits, inherited and equal subsets with
reference/expanded execution, and separate ordinary contingency/parser behavior.
Three units cover missing metadata without selecting other available ranges,
rewriting and permission narrowing, and generated/implicit metadata. Two earlier
priority regressions now expect the annotation range. LSP and isolated VS Code
repair independent unsaved implementations, suppress stale hover and recover
checked hover/native execution. This closes the dedicated P2 location series.
The accepted async-host call model (same task model, staged admission) is recorded
in the revision plan and async design; no async behavior changes in this release.


## Async host operations, reference interpreter (0.217 / E1g.1)

A manifest primitive may declare `"async": true`. Validation admits resource
borrows, Copy values, consumed owned data and owned outcome payloads, and rejects
borrowed data, consumed resources, non-Copy values passed by value, resource or
reference payloads, contingent/pure operations and Rust bindings. Each invoked
operation gets a generated async control, `op$host`, that forwards the single host
leaf. Every source invocation is routed to it before admission, so only direct
`await` passes; plain calls, `run`, `defer` and `spawn` report `E_ASYNC` naming
the operation. Contributor evidence and diagnostics name the operation, not the
generated control. The verifier keeps async host leaves in their own generated
control and admits it only through `Operation::Await`. Unused declarations
generate nothing.

The reference interpreter binds a scripted completion when the operation starts.
An optional per-call `waits` parks the task through the readiness registry. One
operation is signalled per idle point in FIFO order and re-polled without fuel or
quota. The event is published once, at completion. Owning faults retire pending
operations without events. Synchronous primitives reject `waits`. Native source
generation reports `E_BACKEND`.

Nine integration tests cover scripted interleavings with original/expanded
agreement, every fuel cut under three quotas, borrowed resources across sequential
waits, pending and ready operations beside a faulting sibling, script errors,
admission messages and locations, generated-control visibility, native rejection
and manifest limits. A unit test tampers with the IR: a moved host leaf, a
deferred generated control and an orphaned control are each rejected. LSP covers
operation hover, keyword hover and unsaved rejection without generated names.
Isolated VS Code repairs `E_ASYNC` forms, recovers hover and reports the native
limit. The playground manifest adds `receive`. The baseline check compares every
maintained `.tellur` source with the 0.216 compiler and finds identical reports.
`playground/guarded_task_captures.tellur` now declares an `@exit` contract for
its executable entry; the maintained-playground exit-code test had been failing
since that file was added.


## Native scripted async host operations (0.218 / E1g.2a)

The leaf of a generated `op$host` control lowers to a production `Step::Host`
callback step. The native host validates and binds the scripted completion at start,
including the scenario's `waits`, and rejects nonzero `waits` on synchronous
primitives. A pending operation is recorded as `Pending::Host` and pauses its
activation without fuel. The sequential driver elapses waits immediately. The
source driver parks the context through the readiness registry and signals the
oldest live operation at idle points. Completion publishes the event with
start-time arguments and binds the outcome without an instruction. Fault unwinding
drops pending operations without releasing borrowed supplier handles. Without a
scenario, a native executable reports that no live adapter exists. The `E_BACKEND`
rejection is removed, and the private `Step::Adapter` gates and text-surgery
anchors are unchanged.

Two native differential tests build five executables. They compare interleaved
operations with five waits combinations, original and expanded programs, and every
fuel cut through completion. They also cover sequential borrowed resources with and
without waits, owning faults with pending and ready operations (report message,
phase, activations and site), script exhaustion and the missing-adapter error. LSP
hover describes scripted reference and native execution. Isolated VS Code builds the
fixture, reports the missing adapter when a live `open` reaches the operation without
a scenario, and runs it with a scripted `waits` scenario. The full native library
suite (184 models) exposed one exhaustive pending-state match in the private
selection model; it now names `Pending::Host`, and its ten models were rerun.


## Polled Rust adapters for async host operations (0.219 / E1g.2b)

An async primitive may bind `"rust": {"execution": "poll"}`. Validation rejects
`poll` on synchronous primitives and `immediate`/`blocking` on async ones. Generated
`rust_api` code adds `Poll<T>`, `Wait::after` and one `{op}_operation` trait per
operation. The adapter function is checked with Rust as
`fn(start arguments) -> Box<dyn rust_api::{op}_operation>`. Generated glue keeps the
borrowed resource handles, re-resolves them for every poll and validates outcomes
as declared. `rust_dispatch` covers only synchronous bindings, and `rust_start`
covers async ones; without a Rust configuration both are stubs. Native hosts prefer
a scenario, then the adapter, then report a missing adapter.

The runtime polls once during the start instruction. A signal marks a pending
operation for one in-turn poll, without fuel, for both scripted and live
operations. The source driver signals the oldest due parked operation at idle
points and sleeps until the earliest delay when none is due. The sequential driver
sleeps and polls directly. Completion drops adapter state before the event and the
outcome; fault unwinding drops it before supplier frames.

Three integration tests use a real Rust adapter with a retained resource. The first
runs two interleaved polled operations, checking events, poll and drop order, and
compares a scripted scenario with the reference interpreter. The second covers a
sequential polled operation and an owning fault that stops a pending operation
(state drop, no event, supplier released last). The third rejects mismatched
execution kinds and a mismatched adapter signature at build time. The scripted
differential tests from 0.218 pass unchanged. A first implementation polled
scripted operations once at start, consuming a wait early. The scripted
comparison caught it before release.


## Descriptor readiness for polled adapters (0.220 / E1g.3)

`rust_api::Wait` gains `readable(&impl AsRawFd)` and `or_after(delay)`.
`HostProgress::Pending` carries an optional delay and descriptor. A pending report
with neither is a host error. Generated crates with polled adapters add the renamed
`tellur_mio` dependency (`os-poll`, `os-ext`; the name is reserved) and emit
`rust_readiness`, a Mio `ReadinessBackend`. Other crates emit a stub, and `launch`
installs the backend. A live operation registers its descriptor once under a fresh
token, re-registers when the descriptor changes, and deregisters on completion and
when unwinding stops it (`HostOperation::stop`). Idle points block on the backend
up to the earliest delay and mark reported tokens due. The sequential driver waits
for its single operation the same way.

The adapter integration tests now resolve their lockfile with `lock-native`. A new
test uses socket-pair resources with delayed writer threads. Two concurrent receives
complete in readiness order with exactly two polls each (the start `WouldBlock` and
the readable poll), and no retry loop. A sibling fault stops a registered receive
without waiting for its data, dropping state before the channel. The earlier delay,
sequential, fault and binding tests, the scripted differential tests and all Rust
adapter tests pass.


## Async loopback HTTP receive (0.221 / E1g stage b)

`library/loopback_http.tellur` declares `receive` as an async control that forwards
`await http_receive(connection)`. Both maintained manifests mark `http_receive` as
`"async": true`, bind it with `"execution": "poll"`, embed
`library/host/loopback_http_receive.rs` and re-lock their adapter dependencies. The
polled adapter keeps the request buffer and parsed head in operation state. It reads
a nonblocking duplicate handle until `WouldBlock`, then parks on readability bounded
by the request's single two-second deadline. The deadline is checked before every
read, as before. Blocking mode is restored when the operation completes or is
dropped, so responses and failure replies keep their blocking writes. The parser,
limits and outcomes follow the blocking baseline. That baseline stays in
`loopback_http.rs` for the private receive experiments, which now also strip the
async flag from their copied manifest entry. The settings service, the HTTP example
and the second-receive test enter the async control with `run`.

The library and settings HTTP suites (scripted failures, live routes, the framing and
deadline matrix, fault recovery and O0/O2 builds) pass unchanged against the async
receive. LSP and format checks of the maintained sources stay clean.
The private E1f receive models splice the blocking baseline into generated code, so
they now compile a frozen synchronous copy of the HTTP example and facade
(`tests/fixtures/http_sync`).


## Dynamic task sets (0.222 / E1g stage c, part 1)

Accepted design (7 November 2026): `tasks<C>` is a linear local set of started
children of one async control. `spawn C(arguments) into set;` queues a member with
ordinary spawn admission. `delta await next set { ..., empty => ... }` joins the earliest
completed member, or waits for one, and delivers the producer's outcome. The
compiler-added `empty` alternative selects without suspending and is the only path
that retires the set. `count(borrow set)` returns the number of unjoined members. Sets move
locally and through loop parameters only, never across direct inline fault boundaries,
helper calls or outcomes. A member fault stops the owning boundary and settles every
member, including completed but unjoined ones, before the owning contingency.

In this release, members capture and return owned, reference-free values. The checker
lowers task sets to two IR operations: `SpawnInto` takes the captures followed by the
set and re-owns the set; `Next` takes the moved set, and member arms receive the payload
and then the remaining set. The verifier checks the producer profile, argument
types and arm shapes. The reference executor keeps a waiting caller's set in its
slot, so emergency retirement abandons the members. It records completion order
and routes faults and settlement through these waits. Native builds report
`E_BACKEND`.

`tests/task_sets.rs` covers:
- completion-order joins, including members that complete before the first await;
- bounded width;
- an empty set draining immediately;
- loop-carried sets;
- resource-owning members;
- fault settlement with running and completed members;
- the rejections listed in the reference (set leaks, a missing `empty` path, inline
  boundaries, `drop`, outer sets in loops, untyped `tasks()`, `forward await next`,
  a wrong producer, synchronous controls, parameters and outcomes, borrowed profiles,
  an `empty` outcome name, and bare `spawn` statements);
- the native gate.

Every run is compared across original and expanded programs and four host quotas.
The runtime census retires every context at every fuel cut for the example, the
fault fixture and their variants. LSP hover and diagnostic repair, and the
contextual grammar scopes, are tested too.

## Native task sets (0.223 / E1g stage c, part 2)

Native executables now run task sets. The generated step for `spawn producer(..) into set;`
pops the moved set, starts the child and hands the queued task to the source scheduler,
which records the handle in the set and restores the set to its slot. `await next set`
selects `empty` directly for an empty set; otherwise the caller parks on a set wait.
After each scheduler step the wait joins its earliest completed member (by source
completion order), delivers the member payload followed by the remaining set, and
returns the caller to the FIFO. Owning settlement that abandons a member or the
waiting owner retires the wait, so faults follow the spawned-children rules. The
`E_BACKEND` gate is gone, `count(borrow set)` counts members natively, and the
example is back at `playground/task_sets.tellur`.

`tests/task_sets.rs` compares native `--scenario` output with the reference executor
for the example, the member-fault fixture and the owner-fault fixture. The
source-scheduler unit tests compare the same three programs, original and expanded at
O0 and O2, across every fuel cut and host quota. Existing playground and exit-code
tests now build the example natively.

## Resource-bearing async results and async accept (0.224 / E1g stage c, part 3a)

Async host operations may complete with owned resources. Manifest validation
requires each resource type in an async outcome payload to declare
`"fault": "release"`, since a stopped completed-but-unjoined child, or a task
holding the result across a handoff, must release it. Polled Rust glue ends the
poll's borrowed resources before inserting returned resources into the resource
table. The loopback library's `accept` is an async control over the polled
`http_accept` in `library/host/loopback_http_accept.rs`, which parks on listener
readability bounded by the idle timeout. The settings and HTTP example manifests
bind it with `"execution": "poll"`, declare `"fault": "release"` on
`http_connection`, and enter it with `run`. The blocking adapter stays as the
internal baseline.

`tests/async_adapters.rs` builds a polled operation that returns a resource. Two
concurrent members receive and close their pieces in completion order; the
scripted run matches the reference executor; a sibling fault stops a member that
owns a returned piece across a pending wait, dropping the wait state, then the
piece, then the borrowed supplier; and a resource result without a fault
disposition is rejected. `tests/host_operations.rs` updates the manifest-profile
rejection. The live settings and HTTP library suites run with async accept.

## Write readiness, consumed resources and async send/finish (0.225 / E1g stage c, part 3b)

Polled adapters gain `Wait::writable`. Native pending progress carries one
descriptor with its interest, registered with Mio as readable or writable and
re-registered when either changes. Async host operations may consume resources
whose types declare `"fault": "release"`; manifest validation rejects other
consumed resources. Generated start code takes consumed owners from the resource
table before borrowing, and the operation state releases them when dropped. The
loopback library's `send` is async over the polled `http_send` with owned text,
and the new async `finish` forwards the polled `http_connection_finish`. The
settings service and HTTP example use them on normal paths; contingencies keep
the blocking `close` and `reply_failure`, since async invocation is unavailable in
contingency code.

`tests/async_adapters.rs` adds a write that blocks on a slowly read pipe: it parks
on write readiness while another task runs, and finishes well before its
ten-second fallback timer. Another test consumes resources in a polled operation,
compares the scripted run with the reference executor, drops the consumed resource
with the stopped operation's state, and rejects a consumed resource without a
fault disposition. The settings and HTTP library scripts now expect
`http_connection_finish` on normal paths, the fault-injection tests wrap the async
send, and the contingency-permission test expects `E_ASYNC` for an async send in a
handler. The live settings and HTTP library suites run with async send and finish.

## Concurrent settings server (0.226 / E1g stage c, part 3c)

The settings service serves connections concurrently with a task set. The async
`serve` loop keeps one global budget and idle timeout, admits a connection only
while `count(borrow active) < 64`, and drains the set after admission stops; the
admission state then selects `completed`, `idle` or `failed`. Each member owns its
connection and supervises a named boundary that retains it, so a request fault is
handled locally with `http_reply_failure` and `close` and never stops siblings.

`tests/settings_http.rs` adds a live test: a slow client stalls mid-headers while a
fast client receives its response in under a second, then the slow client completes;
against the 0.225 server the fast client waits about 1.9 seconds and the test fails.
The scripted cases keep their sequential descriptions and are converted to the
concurrent start order by one documented helper. The existing live suites (persistent
settings, framing and deadlines, removal, injected source fault followed by the next
request) pass with the concurrent server. `tests/format.rs` follows the server's
new contingency terminal line.

## Run, defer and spawn of host operations (0.227 / E1g stage d)

Async host operations are entered like async controls. Admission no longer
rejects `run`, `defer`, `spawn` or `spawn … into` of a host operation; a plain call
reports `must be invoked with await` in async controls and `must be entered with
run` in synchronous ones. `tasks<op>` annotations and loop parameters are routed to
the generated control, and type display hides the generated name. The verifier's
direct-await-only rule for generated controls is removed; its leaf-placement rule
stays.

`tests/host_operations.rs` runs one program that uses `run` from `main`, `spawn`,
`defer`, `spawn … into` and `await next` over a scripted operation with mixed
waits. The reference, expanded and native runs agree on completion order. The test
also checks the remaining rejections and that their messages never name `$host`.
`tests/async_adapters.rs` builds a polled adapter with `run`, `defer` and two
set members that complete in readiness order. The LSP test checks the new hover
text and that a borrowed-resource task set reports `tasks<receive>`.

## Cooperative cancellation (0.228 / E1e.1)

`cancel child;` and `cancel set;` are checked and run in the reference executor.
The checker accepts the contextual statement only in async controls outside
contingencies, on a local started handle or task set, and rejects deferred tasks.
A `cancelled` arm is admitted only on a host operation entered by `await` or
`run`; the verifier admits it only there, and manifests may not declare it.
Native builds report `E_BACKEND`.

`tests/cancellation.rs` covers a child cancelled while its operation is pending,
before it starts (no scenario call consumed) and after it completed; operations
without the arm and joins that are not interrupted; a set whose current members
are cancelled while a later member completes normally; and propagation to a
grandchild spawned after the request. Every run matches the expanded program
and small host quotas. `tests/host_operations.rs` adds reference and native
evidence that `run` of a pending operation parks a root chain beside started
children and that child faults reach owners parked by `run`. The grammar and LSP
tests cover the contextual keyword and hovers.

## Native cancellation (0.229 / E1e.2)

Native executables run `cancel child;`, `cancel set;` and `cancelled`
alternatives; the `E_BACKEND` gate is removed. An `await` or `run` that lists
`cancelled` now enters a cancellable generated control, and both executors decide
observation at its leaf.

Every scenario in `tests/cancellation.rs` now also runs as a native executable and
must produce the same JSON as the reference executor, including pending,
unstarted and completed operations, set members, later descendants and
synchronous-helper runs. `tests/async_adapters.rs` cancels a pending live polled
operation: its state drops before the supplier and it never completes.
`tests/host_operations.rs` adds native parity for a child fault reaching a root
chain parked by `run`, which previously stopped with `E_NATIVE: source task wait
has no runnable producer`.

## Direct LLVM scalar calls (0.236 / B1.3b)

The LLVM emitter admits checked synchronous `Operation::Region` calls and emits
helper functions separately from executable entry metadata. The private ABI
creates a child invocation borrowing the host, transfers evaluated argument
handles once, and returns a separate one-use outcome handle. Routing checks the
outcome name and whole payload arity before publishing any caller value handles.
Nonmatching arms leave the outcome intact; duplicate/stale transfers fail. A
normal return leaves the child activation, while a fault retains its activation
chain and blocks subsequent effects. Dispatch and payload delivery add no fuel.

Three additional ABI cases cover nested callbacks, Unicode/NUL/bytes payloads,
empty outcomes, caller locals/completion independence, malformed transfers and
fault propagation. Two emitter cases compare original and expanded O0/O2 programs
with Rust/reference results: nested multiple-field calls and zero-payload implicit
calls; imported public/private helpers invoked repeatedly, with exact stdout,
stderr and status agreement through every fuel cut on both normal and fault paths.
The previous scalar, expansion, profile-gate and CLI cases remain covered.
`playground/llvm_calls.tellur` supplies a runnable nested-call example.

This extends backend admission only. Checker ownership/origin/outcome/path rules,
the Rust default, and the unsupported aggregate/reference/loop/boundary/async
profile remain unchanged. The existing coroutine experiment and common LLVM/LLD
embedding are unchanged by this slice.

## Direct LLVM scalar controls (0.235 / B1.3a)

`--backend llvm` admits synchronous scalar controls, assignment, moves, pure
expressions, Boolean decisions/path guards, expansion completions, emit, faults
and exits. It emits LLVM functions/branches directly; no per-program Rust source
is generated. LLVM and LLD verify, optimize and link in-process. The transitional
runtime staticlib is built by Cargo and cached under its source hash; archive
names include the hash so different versions cannot overwrite one another.

The private v1 C ABI uses opaque invocation pointers and one-use integer handles;
no Rust enum or struct layout crosses the boundary. Batched transfers validate
all handles before consumption, zero is invalid, handles are not reused, and the
first failure blocks further effects. The existing launcher retains argument,
scenario, exit-status, diagnostic and report handling.

`tests/llvm_backend.rs` compares LLVM O0/O2 builds with both the reference and Rust
executors across scalar arguments/results, Unicode/NUL strings, bytes, moves,
assignment, pure functions, short-circuit Boolean expressions and multiple entries.
Fault and normal paths match Rust stdout, stderr and status at every fuel cut.
Unsupported operations and signal policies create no build artifacts. ABI tests
in `tests/native_host.rs` exercise invalid/duplicate/consumed transfers, sticky
failure, Unicode/bytes and invalid scalar input.

See the [bounded backend profile](reference/llvm-backend.md). Calls, non-scalar
values, loops, boundaries, async execution, adapters and native debug information
remain later slices; this release does not claim whole-language LLVM parity or
installation-free native building.

## Embedded LLVM and LLD (0.234 / B1.2)

With the cargo feature `llvm` (off by default), `tellurc` contains LLVM and LLD
22.1.8, linked statically from a build of the pinned release. `src/llvm.rs` parses
textual IR modules, links them, verifies the result, runs the new pass manager's
`default<O0|O2>` pipeline with verification after every pass, and emits the object
in memory. LLD's Mach-O, ELF and COFF drivers run in-process through a small C++ shim.

Under the feature, `tests/coroutine_experiment.rs` also builds every E2.1 specimen
through `tellurc llvm-smoke` at O0 and O2, in addition to the external clang builds.
All four builds of each specimen match the reference executor on every schedule.
Linking still uses the SDK's `libSystem` stub; a bundled stub is planned. Default
builds and every other suite are unaffected.

## Runtime library crate (0.233 / B1.1)

Native builds no longer paste the runtime into each generated program. The same
module tree is written once per content hash, as the crate `tellur_runtime` under the
native target directory. Generated programs depend on it as a path dependency, and
the lockfile entry is added at build time, including for adapter lockfiles saved by
`lock-native`. The runtime is compiled with the program's optimization level, codegen
units and debug settings, as before.

`CheckedProgram::native_source` still returns the self-contained, pasted form, and it
is unchanged byte for byte. Runtime evidence tests that instrument the runtime's text
keep building that form. Every native, adapter and parity suite passes unchanged.

## LLVM coroutine experiment (0.232 / E2.1)

`tests/coroutine_experiment.rs` builds the hand-lowered coroutine specimens in
`experiments/coroutines` with Homebrew LLVM 22 at O0 and O2. It requires the same
events and status as the reference executor:

- `playground/cancellation.tellur`: 12 schedules.
- `experiments/coroutines/sets.tellur`: all 64 delay combinations for three task-set
  members, the first completion cancelling the rest.
- `experiments/coroutines/fault.tellur`: six delays of a pending or completed
  operation at a sibling fault; status 1 and the fault message.

The runtime fails if a frame is released twice or is still allocated at exit, or if
a handle is closed while borrowed. Two IR mutations are caught: closing early, and
skipping a cancelled child's join. Without the pinned toolchain the tests skip,
unless `TELLUR_REQUIRE_COROUTINE_EXPERIMENT=1` is set. The release validation ran
them with the toolchain present. The production backend is unchanged.

## Milestone 5 acceptance specimen (0.231)

`playground/cancellation.tellur` is the specimen: two concurrent receives, the
cancellation of one, borrowed handles held across suspension and a cleanup failure.
`tests/cancellation_specimen.rs` runs it under scenarios in which the slower
receive is pending when cancelled (it stops and reports `stopped`), both receives
complete before the request is observed (the ordinary outcome wins the race), the
primary ends without data (the backup answers), neither answers, and every
combination of close failures. Each run must give identical events in the reference
executor, its certified expansion and three small host quotas, and identical JSON
and process status in a native executable. Two rejections check that a handle
cannot be closed while a started task borrows it (`E_LOAN`) and that a cancelled
child cannot be left unjoined (`E_RESOURCE`).

Contributor notes no longer name generated host-operation controls such as
`receive$host$cancel`, nor point at their placeholder locations; a note now says
the outcome is received from `receive`.

## Signal cancellation and graceful settings shutdown (0.230 / E1e.3)

An entry whose manifest launch configuration sets `"signals": "cancel"` is
cancelled by the first SIGINT or SIGTERM; a second terminates with 128 + signal.
Scenario `$signal` entries reproduce this deterministically in both executors.
The loopback HTTP library adds `accept_cancellable` and `receive_cancellable`, and
the settings service uses them to stop gracefully. Its HTTP adapter now sends
status 503.

`tests/cancellation.rs` cancels an entry with a pending set member and a pending
accept, in the reference executor and as a native executable with identical JSON,
and checks that a scripted signal without the policy, or a second signal, ends the
run with the termination status. `tests/settings_http.rs` scripts a SIGTERM while a
member waits for its request and the loop waits to accept: the member replies 503,
the service reports `stopped`, and the native executable matches. Two live tests
send real signals to a native server: after a served request, SIGTERM makes an idle
connection receive 503 and the process exit 0 with `stopped`; two pending signals
terminate it with 130 or 143.
