# Tellur language revision: controls, values, outcomes, and tasks

30 October 2026. This is the working revision plan, not a claim that all proposed
features are implemented. Compiler **0.62.0** delivers milestone 1, the owned-value, borrowed-call, local, grouped-result, whole-record input, contained-reborrow and opaque-split
lifetime checkpoints of milestone 2, the array/vector/map core of milestone 3,
and implicit entry, inline/stored adapters, owned capture, named outcome contracts
and checked forwarding in milestone 4. A first [semantic foundation](semantic-foundation.md)
now accompanies executable algebra-law tests and finite scenario comparison. The original
v0.1 documents remain unchanged; the old syntax and conformance fixtures remain
supported. See [the current reference](language-reference.md) for executable syntax.

Release 0.49 delivers I1b.1 retained Rust resource owners, preparing the listener
and connection lifetimes needed by P5b's blocking loopback service. Release 0.48
delivers explicit executable status policies through named outcome annotations
and exhaustive entry mappings.

Release 0.50 delivers P5b.1: the settings application now serves bounded, sequential
loopback HTTP with Tellur routing, validation, status mapping and resource completion.
Release 0.56 delivers P4a formatting through the CLI and VS Code, preserving label
extent, optional groups, comments and literals. Release 0.57 delivers P4b.1 checked
public contract snapshots, structural diffs, an editor JSON view and the settings
library baseline. Release 0.58 delivers P4b.2: bounded internal outcome contributor
snapshots/diffs, including changes under equal public signatures. Release 0.59 delivers
P4c: bounded source-position and targeted outcome queries in the CLI and editor,
including private controls and explicit limits. Release 0.60 starts P6a with the
agreed execution-boundary fault model, observed invocation identities and bounded
runtime failure reports. Release 0.62 delivers source fault termination and transitive
permissions. Activation-owned storage is next, followed by checked isolation. See [the fault/activation plan](faults-and-activations.md).
Keep using this application as a maintained example;
further reference expansion remains driven by application evidence.

Release 0.61 refines P4a using the author's server layout: inline label dispositions,
automatic width-based list/condition wrapping, consistent nested-adapter indentation
and matching CLI/editor width options. Release 0.62 completes P6b: `fault`/`faults`,
fault-only controls without `exits`, explicit retained-owner fault release, and
corresponding contracts, editor feedback and transformation evidence.

Release 0.47 starts P2 structured output: versioned JSON check reports, explicit
point encodings, related locations and typed structural witnesses in CLI/LSP.
Release 0.51 adds value-expression and parser-token ranges through imports and
generics, including multiline/Unicode editor locations. Release 0.52 adds bounded
independent-body recovery with failed-caller suppression and fresh hover. Release
0.53 starts P3 with optional path grouping and canonical layout guidance. Release
0.54 adds direct arm exits and adopts them in the settings application. Selected-history
editor feedback is delivered in 0.55; within-body and preparation-phase
recovery remain follow-ups.

Release 0.46 extends diagnostic contributors through captures, moves, local aggregate
fields, stored/nested adapters, delayed exits and merged path labels. It filters
disjoint writes and forgets unknown mutation or loop-carried lineage. See
[diagnostic coverage and limits](reference/diagnostics-and-limits.md).

Release 0.45 starts I1/P5: typed Rust source adapters, explicit locked dependencies
and a persistent settings CLI with Tellur policy and host file/JSON mechanisms.
Opaque retained Rust resources are delivered in 0.49. HTTP and async bindings
remain later slices.

Release 0.44 requires explicit public outcome/payload/origin contracts and adds
bounded diagnostic contributors through direct forwarding. Private inference stays
available; stored-outcome lineage and complete adapter explanations remain queued.
Release 0.43 adds local shared-reference elements in arrays, vectors and maps,
including lookup, insertion/removal, conversion, copying/cloning, conservative source
unions and checked native/editor behavior. Exclusive elements and public collection
contracts remain separate stages.

Release 0.42 adds aggregate trait origin contracts: explicit grouped inputs/results,
position-based implementation conformance, named completion bindings and abstract
generic witnesses. The existing syntax works for statically known schemas.

Release 0.41 adds public nested-reference contracts: separate storage arrows and
referent brackets, reference-indirection paths, whole owned/borrowed boundaries,
retained authorization chains, checked native execution and editor coverage.

Release 0.40 adds local stored carrier references: recursive retained dependencies,
capture/adapters, nested reference fields, local borrowed inspection and native/editor
coverage. Public container contracts remain queued as a separate origin-schema step.

Release 0.39 adds whole-carrier reference results: storage and contained-source
contracts, direct call bindings, forwarding, native execution and editor hover.
Nested storage of these references remains a distinct checkpoint.

Release 0.38 adds whole-sum borrowed inputs and non-consuming decisions. Active
payloads are borrowed in place or reborrowed directly; shared authority, carrier
guards and mandatory handling survive nesting, imports and generic specialization.
Whole reference-bearing aggregate reference results follow in 0.39.

Release 0.37 adds named-outcome and builtin-option origin schemas, including
`exits reply[input]` and inherited option groups.

Release 0.36 adopts the name **Tellur**, with canonical compiler/server commands,
source extension, editor identity and reference branding. Compatibility entry points
preserve existing programs and settings. This release does not change origin semantics.

Release 0.35 adds public origin groups on variants and owned variant boundaries.
It deliberately retains the gates on whole-sum borrows and named-outcome/option
schemas. See [grouped variant contracts](language-reference.md#grouped-variant-contracts).

Patch 0.29.1 adds referent inference for `ref auto` and `ref mutable auto` local
and payload bindings, retaining exact reference mode and ownership checks.

Patch 0.33.1 adds inferred-type hover on `auto` in local declarations and payload
bindings, including reference modes, captures, stored adapters and generic bodies.
`exits auto` also exposes its checked signature on hover. This is an editor usability
checkpoint; the remaining language-feature queue is unchanged.

## Active checkpoint queue

Updated after the 28 October design review and the user's clarification that
`[path]:` is a first-class label, not a braced construct. The P checkpoints below
take priority over further reference features. Delivered rows and the numbered
milestones retain the historical feature inventory. Compiler 0.29 delivers source data declarations, file modules, named struct fields, generic data and generic controls with sealed capability bounds, plus concrete and generic trait dispatch with associated types.
Research proceeds alongside practical checkpoints and does not indefinitely block
ordinary language usability. P1a is delivered in 0.44 and the bounded P1b slice in 0.46; P2 continues diagnostic
work alongside the sustained application (P5), with small Rust adapter work (I1) supplying
concrete library needs. It does not wait for all P2–P4 tooling. No dates or
unimplemented syntax are promised.

| ID | Checkpoint | Status / completion boundary |
| --- | --- | --- |
| A1 | Source structs and ordinary variants; nominal one-field wrappers | **Delivered in 0.17**: declarations, typing/ownership, runnable example, native and editor checks |
| A2 | Module identities, imports and visibility | **Delivered in 0.18**: file imports, namespaces, public/private interfaces, source maps, dependency diagnostics and editor tracking |
| A3 | Data construction ergonomics | **Delivered in 0.19**: named struct fields, written evaluation order, layout diagnostics, native/certificate/editor checks |
| B1a | Generic data | **Delivered in 0.20**: explicit parameters, structs/variants/outcomes, finite specialization, module privacy, derived ownership, native/editor checks |
| B1b | Unconstrained generic controls | **Delivered in 0.21**: definition checking with opaque must-complete parameters, explicit calls, inferred contracts, module privacy, borrows, native/editor checks |
| B1c | Generic capability bounds | **Delivered in 0.22**: sealed copy/clone/drop requirements, definition permissions, concrete argument checks and editor/native evidence |
| B2a | Concrete traits and associated types | **Delivered in 0.23**: [declarations/implementations, associated projections, coherence, privacy, static calls and contracts](traits.md); explicit qualification accepted |
| B2b | Trait-constrained generic controls | **Delivered in 0.24**: abstract witnesses, associated projections/permissions, checked bound implication, module/editor/native checks |
| C1a | Declared borrow boundaries | **Delivered in 0.25**: accepted `from input`, checked implementation origins, borrowed trait results, generic/editor/native coverage |
| S1 | Reference vocabulary and access | **Delivered in 0.26**: `implement`, `ref`, `borrow`, `deref`, transparent field access; previous syntax remains accepted |
| C1b.1 | Local struct loans | **Delivered in 0.27**: nested reference fields, copy/clone permissions, moves/partial moves, disposal, generic layouts, privacy and native/editor checks |
| S2 | Selected payload inference | **Delivered in 0.28**: `ready...` infers a fixed nonempty payload; bare exits stay nullary; generic/module/native/editor coverage |
| C1b.2a | Grouped record results | **Delivered in 0.29**: schematic schemas, nested/generic layouts, explicit/inferred return bindings, field last use, module/native/certificate/editor checks |
| C1b.2b.1 | Whole-record inputs and contained origins | **Delivered in 0.30**: owned carriers (including exclusive fields), borrowed carriers with shared fields, `input['group]` selectors, carrier/external lifetime separation, generic/module/native/editor checks |
| C1b.2b.2 | Contained exclusive reborrowing | **Delivered in 0.31**: shared/exclusive carrier authority, retained storage guards, independent child last use, generic/module/collection/native/editor checks |
| C1b.2b.3 | Opaque disjoint permission contracts | **Delivered in 0.32**: checked exclusive result separation, opaque owner retention, shared origin groups, projection/reborrow ancestry, traits/generics/modules/native/editor checks |
| C1b.3a | Local borrowed variants and outcomes | **Delivered in 0.33**: tagged loans, optional references, capture/adapters, record payloads, copy/move/drop/handling rules, local scopes and loops, generic/module/native/editor checks |
| C1b.3b.1 | Nested local borrowed storage | **Delivered in 0.34**: recursive tag/field dependencies, nested options/variants/outcomes/records, independent retainers, scope and carrier guards, generic/module/native/editor checks |
| DOC1 | Detailed language manual and documentation checks | **Delivered for 0.34**: semantic reference, lexical/grammar/diagnostic/glossary/status chapters, checked examples and local navigation; maintained at every checkpoint |
| C1b.3b.2a | Public grouped variant contracts | **Delivered in 0.35**: alternative-guarded schemas, owned calls/results, nested/generic record/variant layouts, contained selectors, capture/forwarding, verifier/native/editor checks |
| N1 | Tellur naming and migration | **Delivered in 0.36**: compiler/crate names, `.tellur` sources, editor/package identity, reference, legacy file/command/settings support |
| C1b.3b.2b | Named-outcome and builtin-option origin schemas | **Delivered in 0.37**: grouped named outcomes, completion bindings, inherited option groups, owned boundaries, generic/module/native/editor coverage |
| C1b.3b.2c | Borrowed sum carriers | **Delivered in 0.38**: shared/exclusive whole-sum inputs, active payload reborrows, authorizing carrier guards, nested/generic/module/native/editor checks |
| C1b.3b.2d | Whole carrier reference results | **Delivered in 0.39**: direct references to grouped records/sums, inferred/declared contained origins, storage guards, forwarding and verifier/native/editor checks |
| C1b.3b.2e.1 | Local stored carrier references | **Delivered in 0.40**: capture/adapters, recursive referent dependencies, nested/borrowed unpacking, local generic/module/loop support and native/editor checks |
| C1b.3b.2e.2 | Public stored-carrier contracts | **Delivered in 0.41**: separate storage/contained mappings, reference-layer paths, whole owned/borrowed boundaries, guarded sums, native/editor checks |
| C1b.3b.2f | Aggregate trait origin contracts | **Delivered in 0.42**: explicit grouped inputs/results, nested references, named completions, implementation conformance and abstract generic witnesses |
| C1b.3c.1 | Local shared-reference collection elements | **Delivered in 0.43**: arrays/vectors/maps, direct lookup, branch-sensitive insertion, removal/conversion, independent copies, conservative source unions, native/editor coverage |
| P1a | Stable public contracts | **Delivered in 0.44**: explicit public outcome/payload/origin contracts, private inference, unused generic checks and bounded direct-forwarding contributor notes in CLI/editor |
| P1b | Outcome contributor coverage | **Bounded slice delivered in 0.46**: captures/moves, local record/sum payloads, stored/nested adapters, delayed exits and label merges; guarded writes exclude overwritten sources. Loop-carried values, indirect aliases, dynamic collections and cross-call payload lineage remain future work |
| P2 | Structured diagnostics | **Reports in 0.47; ranges in 0.51; independent-body errors in 0.52**: bounded CLI/LSP batches, failed-caller suppression, source ranges, witnesses and fresh hover. Follow-ups: within-body/preparation-phase recovery and broader contract/contributor ranges |
| I1a | Owned-data Rust adapters | **Delivered in 0.45**: typed wrapper/result generation, registry dependencies and explicit locking, owned scalar/collection data, shared call borrows, blocking/immediate classification and scripted/live conformance |
| I1b.1 | Retained host resource foundation | **Delivered in 0.49**: opaque must-complete Rust owners, direct shared/consuming calls, explicit resource return on retry, checked runtime identities, teardown fallback and live/scripted/editor evidence |
| I1b.2 | Broader Rust reuse | Application-driven follow-up: richer data boundaries, exclusive foreign mutation and dependency/package ergonomics; stable binary ABI and LTO remain later layers |
| P3 | Readable path labels and direct dispositions | **Grouping/layout in 0.53; direct exits in 0.54; selected-history hover in 0.55**: resolved selectors, aliases, local arm facts, one-iteration loop context, bounded origins and fresh unsaved checking. The planned P3 slice is complete; multi-statement arm syntax remains separate |
| P4 | Explain, format and contract changes | **P4a delivered in 0.56:** whole-document CLI/editor formatting preserving labels, grouping, comments and literals; 0.61 adds configurable wrapping and the author’s inline-label style. **P4b.1 delivered in 0.57:** checked public snapshots/diffs, symbolic permissions, editor export and an application baseline. **P4b.2 delivered in 0.58:** typed bounded contributor evidence, separate public/evidence diffs, source sites, editor export and an application baseline. **P4c delivered in 0.59:** source-position and targeted outcome queries, private roots, readable Markdown/structured JSON and editor actions. This planned tooling slice is complete; loop-carried/indirect/cross-call payload lineage and generic root evidence remain explicit follow-ups. All reports distinguish structural evidence from feasible inputs |
| P5a | Sustained application: persistent CLI | **Delivered in 0.45**: settings load/get/set, validation, strict JSON, bounded input/output, replacement uncertainty and live/scripted failure scenarios |
| P5a.1 | Executable outcome status contracts | **Delivered in 0.48**: alternative annotations, exhaustive entry mappings, final-contract policy, exact exec status propagation, and settings/editor migration |
| P5b.1 | Sustained application: blocking HTTP | **Delivered in 0.50**: loopback settings/lookup/update endpoints, Tellur policy, retained listeners/connections, framing/size/deadline limits, budget/idle shutdown and live/scripted cleanup evidence |
| P5b.2 | Sustained application and library growth | Maintain CLI/server during P2–P4; extract reusable mechanisms when contracts settle. Signal-driven shutdown, richer HTTP and concurrency remain later work |
| P6a | Boundary policy and execution evidence | **Delivered in 0.60:** agreed boundary termination without local catch/resume; bounded reference/native failure reports, observed activation IDs, source/expansion origins and explicit teardown limits. These IDs do not yet address storage |
| P6b | Source faults and transitive contracts | **Delivered in 0.62:** source termination, public/trait fault permissions and private inference; live resources require explicit retained-owner release dispositions at faulting paths/calls. Fault-only controls omit `exits`. Faulting operators and no-fault annotations remain separate decisions |
| P6c | Activation-owned storage | **Next:** frame identities, reference addresses, retirement and transformation evidence before recursion/suspension; current storage remains sequential |
| P6d | Isolated boundaries | Checked admission and resource disposition before a supervisor can continue after child failure; borrowed state and async children require explicit rules |
| C1b.3c.2 | Local exclusive-reference collection elements | Deferred pending application evidence: permission transfer, direct reborrowing, dynamic removal splits and carrier invalidation |
| C1b.3c.3 | Nested and public borrowed-collection contracts | Deferred pending application evidence: element origin schemas, nested carriers, generic/trait boundaries and stored outcomes |
| C1c | Generic loan propagation and reference state | Deferred pending application evidence: contained loans across generic calls, assignments and adapters; rebinding/reborrowing |
| C2 | Slices and non-consuming iteration | Deferred pending application evidence: invalidation rules and library interfaces built on C1b/C1c |
| C3 | Remaining adapter ergonomics | Direct arm exits delivered in P3 (0.54); multi-statement arms, whole-outcome binding and expression adapters remain design work |
| D1 | Explicit runtime polymorphism | Queued: erasure rules, borrowed/owned storage, opaque graph boundaries |
| D2 | Stateful protocols and effect contracts | Queued: sequence obligations, justified algebraic laws, groundwork for tasks |
| E1 | Async reference execution | After P3/P6 and activation-frame work: activation identities, deterministic scheduling, task ownership, suspension, cancellation/cleanup |
| E2 | Native coroutine lowering | Preserved architectural target: direct LLVM coroutine primitives, pinned-toolchain experiment, frame lifetime rules and schedule-based differential tests; I1 must keep this route open |
| T1 | Stronger source transformations | Hold current scope during P1–P5: extraction correspondence, continuation cuts, source-level lift and editor actions |
| R1 | Mathematical foundations | Maintain existing evidence; defer extensions during P1–P5: preservation judgments, richer protocol abstractions, infinite/causal behavior, mechanization |

Delivered baseline: v0.1 profile plus syntax/ownership/reference/collection/outcome
checkpoints through 0.13; checked expansion certificates (0.14), structural protocol
analysis (0.15), and rechecked tail extraction (0.16). These remain maintained.
Completion evidence is recorded in [conformance](conformance.md). Every implementation
checkpoint updates this queue, reference, examples and applicable editor support.

## Review response and path labels

The design review motivates a change in sequencing, not removal of the implemented
ownership model. Maintain compiler 0.43's reference features and correctness checks;
reconsider further expansion using application evidence. One application that does
not store references is not sufficient evidence to remove the feature. Keep the
ordinary ownership guide approachable and separate advanced stored-origin material.
Existing expansion certificates, protocol analysis and extraction remain maintained.

`[path]:` remains a fundamental language label selecting subsequent statements,
not a body requiring braces. A selector may describe overlapping histories rather
than a branch in a nested tree. Preserve the current rule that the label applies
until the next label or existing enclosing boundary; use `[entry]:` to return to the
enclosing entry selection. Implicit entry at the start of a body remains supported.
Do not silently reinterpret existing trailing statements as entry-selected work.

The user subsequently accepted optional braces: `[path]: { ... }` may give the
selection an explicit end while the unbraced label form remains supported. This is
implemented in 0.53. The grouping form intersects nested selectors with its
selected domain and restores the enclosing body's entry selection at `}`;
`[entry]:` inside it refers to that group's entry selection. Grouping alone does not
introduce a value/lifetime scope; explicit scoping remains a separate operation.
Omitting braces retains the existing label extent, so removing them is not a
semantics-preserving edit when trailing statements would change selection. Formatting
must preserve the chosen extent rather than blindly adding or removing braces.
The default style remains unbraced labels; use optional braces only where omitting
them would make the selection extent difficult to understand. A formatter must not
add them routinely.

The readability response should make label ownership visible: canonical layout
with labels at one indentation level and their statements indented beneath them;
editor feedback for the selected history at the cursor; and a possible lint for
indentation that visually contradicts the parsed group. Formatting does not prove
that a selection matches the author's intent. Indentation remains presentation,
not a new semantic delimiter. Lint details and editor presentation need evaluation.
The existing scoped `over &:` construct retains its distinct lifetime purpose.

The author's `applications/settings/server.tellur` layout, included in 0.60, keeps
labels inside the enclosing control/loop indentation, expands long signatures and
decisions over multiple lines, and separates decisions from their final path
dispositions with blank lines. Preserve this application example as a concrete
readability reference for further formatter refinements. The 0.61 formatter follows
these conventions and adds configurable width wrapping; it preserves tokens and
comments rather than reproducing arbitrary column alignment.

Direct arm termination, such as `missing => exit absent`, removes names for histories
that immediately finish. It does not require replacing `[path]:` labels with braces.
Multi-statement arm bodies and implicit qualified atom naming are separate syntax
questions; neither is adopted by this roadmap revision.

Public contract enforcement and contract diffs must complement the contributor view:
a new internal failure absorbed into an existing `failed` outcome may leave the
public signature unchanged. Diagnostics should expose the origin and propagation
chain instead of treating compatible forwarding itself as an unacknowledged outcome.

Start repeated-decision correlation narrowly with immutable Boolean bindings,
preserving separate site provenance and respecting selected domains and loop
iterations. Borrowed-sum correlation requires additional alias/mutation analysis.
Scenario generation and history rendering must be bounded: symbolic compression
does not remove exponential enumeration, loop histories can be unbounded, and
structural independence supplies no statistical independence assumption.

The application work measures actual maintenance: introducing a leaf outcome,
changing cleanup, adding retries and moving code across modules. Use those changes
to evaluate whether humans and agents find origins and consequences without altering
unrelated policy. A line-count target is not the success criterion. General loops,
explicit-contract recursion and activation frames should be prioritized when the
application needs them, accounting for native storage and analysis boundaries.

## Sustained application and library foundations

Maintain one real application across compiler releases, rather than building a new
showcase for each feature. The initial working choice is a local configuration and
key-value service. Start with a CLI over reusable Tellur modules, then expose the
same operations through a blocking HTTP server bound to loopback. A sequential
server makes useful progress before Tellur has async. The persistent CLI and typed
owned-data Rust adapters are delivered in 0.45, retained resources in 0.49, and
bounded sequential HTTP in 0.50. See [the application](../applications/settings/README.md).

The first useful slice loads persisted settings and supports reading and updating
keys with validation. Subsequent slices add persistence/reload, request handling,
explicit shutdown, and eventually concurrent requests and cancellation. Treat
persistence guarantees precisely: replacement and crash durability are distinct,
and report uncertainty after partial external effects instead of claiming rollback.
Do not require a thousand lines or force every language feature into the program.

Keep request validation, domain decisions, retry choices, failure mapping, resource
completion and orchestration in Tellur. Rust adapters provide filesystem, encoding,
HTTP and other host mechanisms; they must not hide the application in one large
Rust primitive. Exercise invalid configuration, missing keys, failed writes,
occupied ports, client disconnects and cleanup failures. Scripted fault injection
must complement native integration tests. Maintain the same scenarios and program
through future releases, including deliberate contract changes to evaluate impact
reports and reader/agent repairs.

Extract library operations when the application establishes a reusable contract.
Likely early modules cover text/bytes, filesystem and paths, JSON, clocks and
network I/O. Prefer existing Rust implementations behind Tellur-facing contracts
for substantial mechanisms. Library policy and composition can themselves be
Tellur. Keep experimental modules revisable until repeated use establishes their
shape; this is not a promise to stabilize a large standard library immediately.

General loops, numeric types, further formatter refinements and generic argument inference should
advance when they unblock these slices. Keep an honest executable baseline while
adding them; don't wait for complete language tooling before using the language.

## Rust interoperability direction

The immediate goal is source-level reuse through typed Rust adapters built by
Cargo with the generated Tellur program. The current native pipeline already
emits Rust and invokes Cargo, but its host adapters and dependency template are
fixed in the 0.44 baseline. I1a in 0.45 adds explicit typed wrapper source, registry
dependencies and a committed Cargo lock. It is a bounded source-binding facility,
not automatic import of arbitrary Rust APIs. See [the adapter contract](rust-adapters.md).

A binding should connect a Tellur contract to a concrete Rust wrapper: argument
conversions, owned versus borrowed access, closed outcome mapping, resource
completion and faults. Prefer a single checked binding description from which
signatures and glue are generated. Rust checks the wrapper's use of the crate;
Tellur checks callers against the declared contract. Neither compiler automatically
proves arbitrary foreign behavior matches every semantic promise. Adapters remain
an identified trust boundary with conformance and failure-injection tests.

Preserve error meaning. Rust Result and enum alternatives can map to Tellur outcomes,
but an open error type may require intentional classification and a documented
residual outcome. Rust's non-exhaustive io::ErrorKind is one concrete example.
Do not auto-collapse newly encountered errors into an unreviewed generic failure.
Panics are separate from recoverable outcomes; catch_unwind catches unwinding
panics, not aborts, and is not a blanket recovery guarantee. Initial bindings should
favor synchronous concrete APIs, owned data and call-duration borrows; callbacks,
returned foreign borrows, opaque retained owners and async runtimes require explicit
lifetime/scheduling integration rather than being silently admitted.

For independent native artifacts or a future direct LLVM backend, introduce a
versioned C-compatible adapter ABI: concrete scalar/layout rules, explicit buffer
lengths, opaque handles, matching allocation/release operations, result tags and a
defined panic/unwind policy. Keep Rust String, Vec, traits and implementation enums
behind that boundary. A C ABI does not require writing the adapter in C, nor does
it mandate serialization or copying every buffer; those costs depend on the chosen
ownership and representation contract.

LLVM IR/bitcode can be linked and optimized across the adapter boundary with a
compatible toolchain and target. That is an optimization route, not an ABI or
contract discovery mechanism. Do not promise arbitrary precompiled Rust libraries
can be imported by decorating symbols: the Rust ABI has no stability guarantee,
layouts are not a universal wire format, and source generics/traits still need
Rust compilation. Measure boundary overhead before committing to direct Rust ABI
coupling or LTO as a prerequisite for library reuse.

### Preserve first-class async and LLVM coroutine lowering

The user explicitly requires the Rust reuse work to keep E1/E2 open. Tellur owns
its task/outcome distinction, suspension points, scheduling boundaries, cancellation
and completion obligations. The checked async representation must be independent
of generated Rust, Rust Future layouts and any particular executor. Rust library
integration must not turn Tellur async into an alias for a foreign runtime.

Initial blocking adapters must be identified as blocking, with no hidden scheduler
or implicit suspension. Later asynchronous adapters need explicit operation lifetime,
readiness/completion and cancellation contracts behind opaque state; their precise
ABI is not settled. Supporting a Rust executor can be an adapter capability, not
a requirement of every Tellur program. A future native LLVM backend must be able
to call the same host mechanisms through the defined adapter boundary.

Activation identities and stable frame storage must preserve loans and owned
resources across suspension. Cancellation requests must complete the declared
cleanup protocol before frame destruction; dropping a foreign future or invoking
llvm.coro.destroy is not by itself Tellur cancellation. Keep LLVM coroutine layout
and intrinsics internal to the backend and pin the experimental toolchain.

The reference executor and controlled-schedule tests remain the semantic baseline
for native coroutine lowering. These are constraints on near-term architecture,
not a requirement to implement the async runtime before the first service slice.
See [the async milestone](#milestone-5--async-reference-semantics) and
[LLVM lowering](#milestone-6--llvm-coroutine-lowering).

Primary references for these constraints:

- [Rust ABI guarantees](https://doc.rust-lang.org/reference/items/external-blocks.html#abi)
- [Rust type layout](https://doc.rust-lang.org/reference/type-layout.html)
- [Cross-language LTO and toolchain compatibility](https://doc.rust-lang.org/rustc/linker-plugin-lto.html)
- [Panic capture limits](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
- [Non-exhaustive I/O errors](https://doc.rust-lang.org/std/io/enum.ErrorKind.html)

## Decisions carried forward

- Selected payload inference uses `exits ready..., missing`. Bare exits remain
  payload-free; explicit payloads and named outcome contracts remain available.
- Borrowing parameters will use schematic brackets and apostrophe names, with
  field dependency arrows: `struct view['a] { ref string text <- 'a; }`.
  Result binding `view[input]` is executable in 0.29. Version 0.30 adds whole-record
  inputs and the accepted contained selector `input['group]`, distinct from the
  carrier-storage anchor `input`. 0.31 adds exclusive contained reborrowing with implicit carrier authorization.

- On 26 October the user selected precise field/origin groups before implementing
  aggregate boundaries. The [design proposal](precise-origins.md) separates public
  dependency names from private storage and from exclusive permission proofs.
  Compiler 0.29 implements schematic group declarations and returned records;
  0.30 adds carrier inputs and contained selectors; 0.31 adds contained reborrowing.
  0.32 separates access permissions from lifetime anchors and supports checked opaque splits.

- Reference types use `ref t` / `ref mutable t`; expressions use `borrow value`,
  `borrow mutable value` and `deref reference`. Field access through a reference is
  transparent. The full `implement` spelling replaces `impl` in current examples.
  Previous spellings remain accepted; ownership semantics are preserved.

- Use `control` for a callable unit of control flow. The compiler may still use
  “region” internally for a graph component.
- Use snake_case for types, values, controls, outcomes, and path atoms. Prefer full
  collection names: `array`, `vector`, `map`, and `slice`.
- Bindings are type-first: `string text = ...;`, or `auto text = ...;`.
- Payload patterns declare bindings: `found { string text } => available` or
  `found { auto text } => available`. They do not introduce an extra lexical scope.
- Calls use parentheses; payload construction and matching use braces. Decisions
  use `delta operation(...) { ... }`. Selected work retains `[path]:`.
- `exits auto` infers a closed, checked exit contract. It does not weaken exhaustive
  handling, coverage, or ownership. Since 0.44, public controls require explicit
  outcomes, payload types and borrowed-result origins. Private inference remains supported.
- Pure single-value operations should work in expressions. A single exit alone
  does not establish purity.
- Adopt Rust-style ownership foundations: Copy values, moves for owning values,
  explicit cloning, shared/exclusive references, checked lifetimes. Make those
  checks sensitive to Tellur's selected histories.
- Preserve the distinction between ordinary destruction and a resource protocol
  that requires explicit, potentially failing completion.
- Outcomes become first-class completed results with mandatory handling/forwarding.
  A task is a computation; an outcome is its completed result. Neither is a path.
- Preserve author names and structured provenance through `lift` and `extract`.
- Async is first-class, with explicit scheduling, structured task ownership,
  cooperative cancellation, and checked cleanup.

The mutation keyword is **not settled**. `mutable` is the clearest working spelling;
`vary` and `flux` remain candidates. `mutable` is implemented provisionally in 0.7.0.

## Milestone 1 — executable syntax foundation (delivered in 0.6.0)

- `control`, snake_case builtin spellings, and `entry`.
- Type-first parameters, loop parameters, local declarations and payload patterns.
- `auto` local bindings and payload bindings; explicit parameter/loop types.
- Brace decisions, constructors and exit payloads; optional trailing arm commas.
- Checked `exits auto`, including forward calls, loops, payload consistency, and
  declaration hover / Outline showing the inferred contract.
- `u32_text` and `concat` expression forms when the manifest declares `pure: true`.
  Their exact contracts are validated. Arbitrary custom pure controls are not yet
  implemented. Pure calls, including direct delta calls, compute without script
  entries or observable primitive events.
- Updated live playground, highlighting, snippets, native builds and editor checks.
- Compatibility with old v0.1 syntax and scripted scenarios.

Milestone 1 originally retained the old ownership rules. Compiler 0.7.0 now extends
`control` with the rules below; the historical `region` syntax keeps compatibility.

## Milestone 2 — owned values and references

**Checkpoint 2a delivered in 0.7.0:** Copy versus owning values, move-by-default
strings/bytes, explicit recursive ordinary cloning, ordinary destruction, provisional
`mutable`, whole/field assignment and reinitialization, shared/exclusive local
references, shared control parameters and explicit borrowed calls. Moves and loans
are path-sensitive; disjoint record fields remain independent. Loop captures cannot
repeatedly consume an outer owner. Native/interpreted execution and expansion carry
real moves, mutation and scope storage release.

**Checkpoint 2b delivered in 0.8.0:** real reference addresses across shared and
exclusive calls, returned input/field borrows, checked per-outcome return lifetime
summaries, stored reborrows and conservative local last-use inference. Declaration
hover exposes return borrow sources. Interpreter/native/expanded execution agrees
for caller mutation, chained borrowed returns, field splitting and loops. Shared
calls no longer use data snapshots. Exclusive argument loans include later argument
evaluation; temporary reads finish before subsequent arguments.

**Remaining reference work:** the outstanding aggregate boundaries in the active
queue (exclusive, nested and public borrowed-collection contracts),
reference rebinding and loop state, and more precise path-sensitive lifetime
shortening. Records, owned variants, named outcomes and options now carry public origin schemas;
local collections of shared references now track insertion/removal; exclusive elements
and public collection schemas need further permission and origin rules. The current sequential address store must gain activation identities for
concurrent invocation frames before async lowering.


Implement Copy/Clone/Drop capabilities independently of must-complete obligations.
An owning string/vector is not Copy; a file may additionally require explicit
protocol completion. Type capabilities propagate through aggregates.

Introduce binding mutation, assignments to places, shared and exclusive references,
explicit borrows at calls, and first-class borrowed parameters/results. Infer local
borrow lifetimes; expose required relationships at callable boundaries. No reference
may escape its owner. Path families track availability, moves, initialized state,
loans and destruction. Disjoint paths may use the same owner independently.

Acceptance examples: moving a string, cloning explicitly, borrowing multiple record
fields, rejecting overlapping mutation, safe reinitialization, returned input borrow,
rejected returned local borrow, partial moves followed by destruction, and branch
joins where an owner survives on only some histories. Test before/after expansion.

## Milestone 3 — collections

**Checkpoint 3a delivered in 0.9.0:** typed/nested arrays and vectors, empty map
construction, total length expressions, shared/exclusive element lookup, insertion,
removal, replacement returning old/rejected values, consuming iteration and array to
vector conversion. Map keys are bool/u32/string with deterministic ascending order.
Resource collections remain sealed obligations until drained or transferred. Borrowed
elements conservatively freeze their collection and can be returned through helpers.
The interpreter and native programs use the same deterministic operation semantics;
front-end checks, expansion, O0/O2 builds, malformed drivers and VS Code are tested.

**Remaining collection work:** borrowed slices and non-consuming iteration APIs,
source-level primitive/library declarations, inferred literal element types, efficient
storage/iteration and richer key capabilities. Allocation failure remains a host
failure; recoverable allocation outcomes are not implemented.


Add `array<t, n>`, `vector<t>`, `map<k, v>`, and borrowed slices. Settle source type
and operation declarations so core library contracts need not live only in JSON.

Define owned insertion, borrowed lookup, removal, replacement returning the old
value, iteration, invalidation, and explicit cloning. Missing keys and dynamic
bounds failures use outcomes. Do not smuggle panics into ordinary indexing.

Specify map ordering and key equivalence, allocation policy, and cloning failure
policy. Ordinary memory allocation failure may initially be a documented host
failure, but that is not yet a decision to expose recoverable allocation outcomes.
Collections of must-complete resources cannot silently clear/drop their contents.

Acceptance examples: borrowed lookup followed by rejected conflicting insertion;
remove-and-transfer; replacement preserving an old resource; draining a vector of
files with explicit close outcomes; empty and bounds cases; deterministic iteration
according to the selected map contract.

## Milestone 4 — first-class outcomes and transformation boundaries

**Checkpoint 4a delivered in 0.10.0:** optional initial `[entry]`, immediate nested
`exit delta` adapters, optional arm path names, lexical arm bindings, checked output
union inference, and preserved decision/invocation provenance. Flat selectors keep
their statement-group semantics. Zero-payload outcome expressions use `empty {}`.

**Checkpoint 4b delivered in 0.10.0:** owned `capture` results, `outcome<producer>`
types (or `auto`), delayed exhaustive consuming decisions, and whole-result forwarding
with `exit saved;`. Every result carries a handling obligation even with empty/Copy
payloads. Moves and containers preserve it. Capture executes now; expansion preserves
its position before later effects and retains multiple invocations independently.

**Checkpoint 4c delivered in 0.11.0:** nested adapters in declaration initializers,
private inferred closed outcome types, and explicit `outcome<producer>` target
contracts. Typed adaptation supports exact payload types and a wider target union;
contextual literals use the target payload types. Selected writes precede lexical
payload release; effect order, moves, handling obligations and resource completion
survive storage and expansion. Borrowed inputs can be converted into owning results.

**Checkpoint 4d delivered in 0.12.0:** standalone `outcome name { ... }` declarations,
`exits name`, nominal value types shared by multiple producers, typed and qualified
construction, named adapter targets and explicit capture widening into a named
contract. Default capture inherits a producer's named contract. Mandatory handling
and resource transfer survive nested containers and native/expanded execution.
Declarations have highlighting, outline entries and hover in VS Code.

**Checkpoint 4e delivered in 0.13.0:** single-outcome, payload-free call statements;
`forward operation(...)` for compatible tail routing; bare `forward` arm results
that retain the entire selected payload. These reuse checked decision/transfer
lowering, including borrow origins, resource obligations and call-site provenance.
`tests/algebra.rs` supplies identity/associativity examples, Boolean laws, effect
counterexamples and provenance assertions; `tellurc compare` reports agreement only
on supplied scenarios. The semantic draft separates these from general equivalence,
refinement and future transformation certificates.

**Transformation checkpoint delivered in 0.14.0:** expansion exports a versioned
candidate graph and explicit correspondence, checked by a separate implementation.
All expansion paths validate the derivation; CLI commands can export and verify an
artifact independently. Fresh identities, boundary transfers, ordering, loop state,
Boolean guards and provenance are checked. This is rewrite validation, not a formal
soundness proof. See [checked expansion certificates](expansion-certificates.md).

**Algebra checkpoint delivered in 0.15.0:** a runtime-based conditional simulation
argument, finite structural protocol models, residual queries after token prefixes,
and completed finite trace inclusion with shortest abstract counterexamples. Loops
are cycles; state/pair budget exhaustion is inconclusive. The value-erasing model
and its limitations are explicit; this is not a concrete equivalence/refinement or
termination checker. See [protocol analysis](protocol-analysis.md).

**Extraction checkpoint delivered in 0.16.0:** `extract-tail` factors a top-level
unconditional suffix into a typed helper, preserves named contracts or spells out
inferred exits, and rechecks the whole candidate. Mutable inputs become explicit
mutable locals; borrowed origins and all existing public contracts must remain
unchanged. [The guide](extraction.md) lists conservative rejection cases. This is a
source refactoring with finite behavioral tests, not a certified inverse of lift.
General cuts, continuations and source provenance correspondence remain ahead.

**Remaining:** abstract origin-shape bounds, general expression-position adapters and
arm statement blocks, whole-outcome pattern binding, general lift/extract source
transforms, and more precise static origin/correlation tracking across opaque/dynamic
containers. Named and generic contracts currently have finite, statically known layouts;
abstract associated-type origin shapes and more general lifetime state remain future work.
The current runtime value contains only a tag/payload; compiler provenance retains the
invocation and internal decisions. These checkpoints do not implement async tasks.


Introduce named outcome contracts shared by multiple controls. `capture` (working
spelling) invokes now and returns a completed outcome. Direct delta invocation is
the fused form. A captured control outcome must be decided or explicitly forwarded
on every applicable path, even when its payload is otherwise Copy or droppable.

Keep outcome obligations separate from payload ownership. Do not infer “handled”
from assigning to an unused variable. Decide whether repeated inspection is allowed
and how it preserves correlation; consuming handling must not duplicate an owned
payload or create independent contradictory decisions about the same result.

Track definitions, invocation instances, internal decisions, exit contributors and
caller mappings as structured identities. Display composite names from those
identities. Ordinary values do not need to carry full runtime execution traces.
Dynamic containers/opaque boundaries may yield sets of possible origins; observed
histories require optional tracing. Never label a static possibility an observed
execution.

Lift at the invocation site, preserving effect order even when the outcome is
handled later. Extract only a compatible subgraph with an explicit boundary for
values, references, effects, outcomes, obligations and provenance. Inspection of
internal paths is distinct from an executable dependency on private internals.

Acceptance specimen: invoke a file load, store its outcome, perform another effect,
then handle it; lift and extract without moving either effect or erasing the read /
close failure distinction. Test two invocations of the same control independently.

## Milestone 5 — async reference semantics

The following is a design specimen, NOT currently runnable syntax:

```tellur
async control startup(string filename) exits ready { config settings }, failed { load_error error }, cancelled {
    [entry]:
        task<load_result> pending = defer load_config(filename);
        delta await pending {
            loaded { config settings } => loaded,
            failed { load_error error } => failed,
            cancelled => cancelled,
        }
    [loaded]: exit ready { settings };
    [failed]: exit failed { error };
    [cancelled]: exit cancelled;
}
```

This specimen assumes a separate load_result contract with exactly those alternatives.
`async control` may suspend. `defer` evaluates/transfers arguments now without running
the body. `await` consumes a deferred task and drives it, suspending its caller when
needed. `spawn` explicitly schedules concurrent work and returns an owned running
handle. Awaiting a running handle observes its completion without rescheduling it.
Direct `delta await operation(...)` fuses creation, execution/wait and handling.
A synchronous entry uses an explicit runtime boundary; calling an async control does
not silently create parallel work.

Start with a single-thread cooperative executor, explicit runnable/waiting/completed
states, deterministic test scheduling and modeled I/O readiness. Multithreaded
scheduling requires additional transfer/synchronization capabilities.

Each spawned child belongs to a scope. All paths must settle or explicitly transfer
task ownership; no hidden detach. Captured references remain borrowed until released,
including across suspension. A mutable loan does not end merely because the owner
is waiting. Coroutine frame storage must remain stable where references require it.

Cancellation is a request, followed by checked protocol completion, not frame
abandonment. Cancellable controls explicitly declare their cancellation outcomes.
Cleanup failure remains visible. A racing normal completion is still possible.
An await must not secretly throw a cancellation exception in its caller. Cancellation
points and potentially suspending cleanup require explicit semantics; cancellation
is not a guarantee of termination. Race/select operations must account for losers.

Lift/extract preserve fork/join/suspension boundaries and ownership scope. Concurrent
histories form partially ordered graphs, not one invented sequential trace.

Acceptance specimen: two concurrent operations, cancellation of one, a borrowed
value held across suspension, and a cleanup failure. Check every declared completion,
no premature loan release, no abandoned child, and unchanged provenance after lift.
Do not infer correctness from a happy-path timer demo alone.

## Milestone 6 — LLVM coroutine lowering

The current backend remains explicit Rust -> LLVM; it does not emit LLVM coroutine
intrinsics directly. Build a focused native coroutine experiment against a pinned
LLVM release, then lower the checked async representation while retaining the
reference executor for differential testing.

LLVM supplies coroutine frames, suspension/resumption and splitting mechanisms.
Its switched-resume lowering is the initial candidate; `llvm.coro.id.async` is a
specific alternative ABI, not a requirement imposed by the source keyword. Scheduling,
readiness, result delivery and Tellur cancellation protocols remain frontend/runtime
responsibilities. Validate cancellation cleanup before destroying completed frames.
LLVM warns that coroutine compatibility across releases is not guaranteed.
See [LLVM's coroutine documentation](https://llvm.org/docs/Coroutines.html).

Compare reference and native traces under controlled schedules; include borrowed
captures, resource cleanup, normal completion racing cancellation, nested tasks,
frame lifetime, and expansion/extraction. Benchmark before selecting optimizations.

## Release discipline

Each milestone must ship runnable examples, negative tests, updates to the
[detailed language reference](language-reference.md), editor diagnostics/highlighting,
and native/interpreter agreement where applicable. Follow the
[manual maintenance checklist](reference/maintenance.md): update semantic rules,
grammar, remaining limitations and feature status, and run
`cargo test --offline --test reference_manual` for compiled examples and local links.
Keep proposed syntax out of executable playground files until it works. Update this
status explicitly; completion of milestone 1 does not mean completion of this revision.
