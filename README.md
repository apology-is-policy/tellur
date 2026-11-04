# Tellur compiler and playground

The language is now named **Tellur** (formerly the Path placeholder). Release 0.36
uses `.tellur` sources, `tellurc`, `tellur-lsp`, and `tellur.code-workspace`. Existing
`.path` programs and the `pathc` / `path-lsp` commands remain supported. See the
[migration notes](docs/language-reference.md#tellur-name-and-migration).

Compiler release **0.197.0** implements the v0.1 checked core and the current language
revision: `control`, `implement`, snake_case types, type-first declarations,
`ref` types, `borrow`/`deref` expressions and transparent reference field access,
brace outcome patterns, private `exits auto` and selected payload inference (`ready...`), explicit public contracts, pure conversion expressions, move-by-default
owned values, mutation, shared/exclusive references with checked borrowed results and explicit `from input` origins,
source structs and nested variants/outcomes with local reference fields, checked opaque owner splits, exclusive contained reborrows, whole-record inputs and contained origins (`input['a]`), grouped record results (`text_pair[a, b]`) and field-sensitive last use, ordinary variants, generic data and controls with explicit copy/clone/drop bounds, concrete and generic trait dispatch with associated types, named struct fields, file modules with private representations, arrays, vectors and maps, implicit entry statements, nested tail and stored adapters,
named outcome contracts, checked `forward` routing, and single-completion call statements. It includes
a reference interpreter, graph expansion, native compiler, language server, and
VS Code build/run commands. The [revision plan](docs/language-revision.md) separates
delivered features from the remaining reference, slice, outcome-expression and async work. The language is still
fluid; this is an experimental implementation, not a production compiler.

Release 0.197 collects independent implementation-member and method-profile
errors after shared identities validate. Ambiguous or invalid identities retain
the original first-error path; no partial witness reaches associated-type or body
checking. Unsaved partial repairs update the remaining errors and restore hover.

Release 0.196 gives declaration contributor notes the complete written `exits`
clause. Imported generic headers and named completion uses retain original ranges;
raw evidence, note order and omission limits remain unchanged.

Release 0.195 selects result types for missing public borrowed/aggregate origins,
and the completion type at a named-contract use. Imported schema definitions stay
distinct from their use sites; partial unsaved repairs retain exact ranges.

Release 0.194 selects the written inference clause when a public control uses
`exits auto` or `label...`. Mixed contracts select their first inferred label in
source order. Imported unused generics and unsaved partial repairs keep exact ranges;
public/private inference rules and diagnostic phase ordering remain unchanged.

Release 0.193 adds authored ranges to contributor notes for source statements and
outcome expressions, including producing exits, received captures and forwarding
calls. Imported generic definitions and unsaved editor changes retain their original
locations; declaration and omission notes keep conservative point fallbacks.

Release 0.192 collects independent origin-schema errors within the existing 32-error
budget. Declarations depending on a failed schema are skipped; later preparation and
body checking wait for all schemas to validate. Unsaved partial repairs update the
remaining diagnostics without publishing partial hover information.

Release 0.191 selects the offending declared origin group for duplicate and unused
schema-group errors, including imported generic and named-outcome declarations.
Validation order and accepted origin contracts remain unchanged.

Release 0.190 selects written schema field origin mappings in diagnostics, including
separate storage arrows and referent brackets through imports and unsaved editor changes.
Origin semantics and shared preparation order are unchanged.

Release 0.189 adds [boundary helpers with explicit handoffs](playground/boundary_handoff_tasks.tellur).
Named and lexical helpers may yield or await beside live children when retained
owners permit fault settlement. Children keep their original invocation and fault
boundary; local recovery, returned borrows and required joins retain their rules.
Direct parent boundaries still require joining children first.

Release 0.188 keeps local fault recovery in the owning execution chain: native
execution no longer selects an unrelated sibling between fault settlement and its
contingency. The [local recovery example](playground/local_recovery_tasks.tellur)
checks this through an awaited helper. Existing explicit handoff and contingency
restrictions remain unchanged; inline boundary-bearing helper handoffs stay gated.

Release 0.187 adds [uninterrupted boundary helpers](playground/boundary_helpers.tellur):
local named and lexical recovery can run beside live children without moving their
handles or changing handoffs. Direct parent boundaries and boundary-bearing helpers
that can hand off remain gated. Generic dependency checking now retains named
contingencies, including their calls and diagnostics.

Release 0.186 adds [named-outcome reference chains](playground/outcome_chain_spawn.tellur).
Borrowed results retain holder storage and selected origins while the owning caller
still handles every active, empty or nested outcome. Owned carriers transfer their
handling obligations through consuming joins.

Release 0.185 adds [option and closed-variant reference chains](playground/tagged_chain_spawn.tellur).
Active and empty tags retain every intermediate holder; selected suppliers and exclusive
guards survive consuming join. Deeper named-outcome nodes remain the next bounded audit.

Release 0.184 adds [reference chains through records](playground/record_chain_spawn.tellur)
for spawned captures and results. Every intermediate holder, selected supplier origin
and exclusive ancestor remains protected through joins and fault settlement. Tagged
nodes in deeper reference chains remain the next bounded audit.

Release 0.183 adds [whole borrowed named-outcome results](playground/borrowed_outcome_spawn_results.tellur).
Joining retains holder storage and contained permissions. The owning caller still handles
every outcome, including empty alternatives, after borrowed inspection. Deeper reference
pointees remain the next admission audit.

Release 0.182 adds [whole borrowed option and variant results](playground/borrowed_tagged_spawn_results.tellur).
Empty results retain their holder storage without inventing supplier loans; active tags
preserve contained origins, exclusive guards and nested outcome handling. Whole borrowed
named-outcome results remain the next bounded audit.

Release 0.181 adds [whole borrowed record results](playground/borrowed_record_spawn_results.tellur).
Joining a child preserves both holder storage authority and contained reference origins,
including projected records, shared copies and exclusive guards. Caller outcome handling
and resource settlement remain checked. Whole borrowed tagged results remain separate.

Release 0.180 adds [borrowed named outcome captures](playground/borrowed_outcome_spawn.tellur).
A spawned reader retains the outcome holder while its caller keeps the obligation to
handle that outcome, including empty and nested values. Selected supplier origins and
exclusive holder guards remain checked. Whole borrowed-carrier results and deeper
reference pointees remain separate admission work.

Release 0.179 adds [borrowed option and variant captures](playground/borrowed_tagged_spawn.tellur).
Borrowed holders may contain nested records, options and closed variants with direct
reference leaves. Selected and empty alternatives preserve ordinary origin permissions,
exclusive guards and resource retirement. Reference-bearing outcome borrows and whole
borrowed-carrier results remain separate admission work.

Release 0.178 adds [borrowed record captures](playground/borrowed_record_spawn.tellur).
Spawned children retain holder storage and its contained reference leaves until join.
Selected results retain ordinary supplier loans and detached exclusive holder guards.
Borrowed tagged captures, deeper reference pointees and whole borrowed-carrier results
remain separate admission work.

Release 0.177 adds [named outcome spawn payloads](playground/outcome_spawn.tellur).
Capturing and returning a named outcome transfers its mandatory handling obligation,
including empty/shared payloads and nested values. Declared origins, guards and resource
retirement remain checked. Anonymous reference-bearing contracts and borrowed carriers
retain their existing limits.

Release 0.176 adds [owned tagged spawn payloads](playground/tagged_spawn.tellur).
Options and closed variants may nest with records in captures/results. Selected tag
origins, empty alternatives, exclusive guards and retained resource obligations remain
checked. Reference-bearing outcomes, borrowed carriers and collections stay gated.

Release 0.175 adds [nested owned record spawn payloads](playground/nested_spawn.tellur).
Captured and returned records retain full borrowed-field paths through partial moves
and consuming joins. Reference leaves still point to reference-free storage; borrowed
carriers and reference-bearing tagged/collection payloads remain gated.

Release 0.174 adds [flat record spawn results](playground/carrier_spawn_results.tellur).
Returned records retain declared origins for each borrowed field, alongside ordinary
owned fields. Consuming joins preserve selected loans, guarded authority and completion
retirement. Nested and borrowed carriers remain gated.

Release 0.173 adds [flat record spawn captures](playground/carrier_spawn.tellur).
Records may carry direct shared/exclusive references and reference-free owned fields.
Leaf loans remain until consuming join, and selected result origins remain borrowed
through final use. Nested/borrowed capture carriers and result carriers stay gated.

Release 0.170 admits synchronous helpers that reach explicit handoffs through `run`,
provided their inline call chain stays outside fault boundaries and suspended owners
permit fault settlement. `run` enters inline; only a reached `await` or `yield`
switches siblings. See [cooperative helpers](playground/cooperative_helpers.tellur).

Release 0.169 admits cold/direct awaits while owned spawned children remain live.
Awaited work may borrow parent storage; sibling faults restore the owning continuation
and retire awaited borrowers before its contingency. See the runnable
[parent await example](playground/parent_await.tellur).

Release 0.168 checks resource settlement inside spawned producers and their callees.
Owners held across a handoff need a fault disposition, including owners retained
for a nested contingency that an enclosing fault would suppress. Completing such
an owner before the handoff keeps it admissible; sequential use remains available.

Release 0.167 admits checked synchronous helpers while spawned children remain live.
Helpers without transitive task handoffs or fault boundaries run uninterrupted,
including borrowed parent inputs, constructors and concrete generic/trait calls.
Try `./bin/tellurc exec playground/structured_helpers.tellur playground/manifest.json`.

Release 0.166 adds [owned source scheduling](docs/reference/async-execution.md#owned-spawned-children):
`spawn` queues invocation-owned children, and `await`/`yield` explicitly hand off
execution. Try [structured_tasks.tellur](playground/structured_tasks.tellur) with
`./bin/tellurc exec playground/structured_tasks.tellur playground/manifest.json`.
The current reference describes the admitted profile; earlier versioned notes below
record the limits at those historical checkpoints.

Release 0.79 adds checked constructor contracts at call sites: concrete input types,
ordered result origins, fault permission and selected histories. Try hovering over
`text_pair(...)` or `editable_text(...)` in
[borrowed_construction.tellur](playground/borrowed_construction.tellur).

Release 0.80 adds concrete trait-call contracts, preserving the trait's names and
origins while resolving associated types. Unsaved declarations refresh the contract;
generic trait calls retain their abstract associated projections. Try `reader<text_source>.read`
in [traits.tellur](playground/traits.tellur).

The [settings CLI](applications/settings/README.md#remove-a-setting) now supports
key removal through `--entry remove -- FILE KEY`, using the shared JSON-file library
and distinct missing, failed and uncertain-replacement outcomes.

Release 0.100 adds [loop type-annotation locations](docs/reference/diagnostics-and-limits.md#type-annotation-locations):
invalid loop-parameter types select their complete written annotation, including
multiline and imported forms. Initializer errors retain their expression ranges.

Release 0.103 separates reference argument validation from frame installation. Invalid
later arguments cannot leave partial invocation storage. Synchronous execution uses
both steps immediately; source tasks and scheduling remain planned.

Release 0.99 adds [persistent reference execution](docs/reference/execution-failures.md#stepping-a-reference-execution):
the host Rust API can pause between checked instructions and resume with the same
live state, effects and fault context. This enables the next async work without
adding source task or scheduling syntax.

Release 0.98 starts the [async execution foundation](docs/async-execution.md): related
runtime contexts share unique activation IDs even when their lifetimes overlap.
Source tasks, suspension and scheduling remain planned; the design records the
accepted prompt child-fault propagation policy separately from implemented behavior.

Release 0.97 adds [written type-annotation ranges](docs/reference/diagnostics-and-limits.md#type-annotation-locations):
field, parameter, payload and typed-pattern errors can select the full source type,
including multiline annotations and imported aliases. Release 0.101 extends this
to local variable types; 0.102 adds late public-interface visibility checks, and 0.104
adds named completion types after `exits`, including imported and generic spellings. Generated
type names never supply guessed source widths.

Release 0.96 adds [constructor-definition diagnostic batches](docs/reference/diagnostics-and-limits.md#constructor-definition-preparation-batches):
independent invalid constructors within one file report together, including unused
generic definitions. Generated controls are registered only after that file's whole
preparation step succeeds; imported diagnostics retain their original locations.

Release 0.95 adds [trait-declaration diagnostic batches](docs/reference/diagnostics-and-limits.md#trait-declaration-preparation-batches):
independent invalid trait contracts report together before implementation resolution,
including unused declarations, public visibility and borrowed-result origins.

Release 0.94 adds [inferred exit-adapter diagnostics](docs/reference/diagnostics-and-limits.md#inferred-exit-adapter-recovery):
independent payload errors under `exits auto` and `ready...` report together, including
repeated labels, while partial contracts remain unavailable to callers and hover.

Release 0.93 adds [inferred stored-adapter diagnostics](docs/reference/diagnostics-and-limits.md#inferred-stored-adapter-recovery):
distinct result labels can report errors together. Later arms sharing an uncertain
label are suppressed, and the partial result never supplies a type or continuation.

Release 0.92 adds [generic-bound diagnostic batches](docs/reference/diagnostics-and-limits.md#generic-bound-preparation-batches):
independent invalid requirements report together before trait resolution, including
unused definitions and duplicate requirements reached through different import aliases.

Release 0.91 batches inaccessible constructor calls across independent bodies,
with imported source locations, separate repairs and no partial checked program.
Try [constructor_access/main.tellur](playground/constructor_access/main.tellur).

Release 0.90 adds symbolic constructor contracts inside generic bodies, preserving
abstract input types, borrow modes, ordered origins and checked permissions. Try
[generic_construction.tellur](playground/generic_construction.tellur).

Release 0.89 adds symbolic trait-call [hover](docs/language-reference.md#call-site-contract-hover):
associated types, declared names, origin groups and fault permissions stay tied to
the enclosing generic definition and its trait bounds.

Release 0.88 adds symbolic ordinary generic [call hover](docs/language-reference.md#call-site-contract-hover):
abstract types, declared bounds, named origins and inferred return lifetimes follow
the universally checked definition, independently of concrete callers.

Release 0.87 adds [explicit stored-adapter diagnostics](docs/reference/diagnostics-and-limits.md#explicit-stored-adapter-recovery):
independent arms report errors together when the stored outcome type is declared.
An uncertain binding stalls its continuation without publishing a partial result.

Release 0.86 adds [explicit exit-adapter diagnostics](docs/reference/diagnostics-and-limits.md#explicit-exit-adapter-recovery):
independent `exit delta` arms report errors together when the result shape is fully
explicit. Nested arms, borrowed results and owning payloads keep their checked semantics.

Release 0.85 adds [lexical handler diagnostics](docs/reference/diagnostics-and-limits.md#lexical-contingency-handler-recovery):
independent contingency errors appear together after the work checks cleanly, while
fault-mode permissions, retained resources and terminal rules remain enforced.

Release 0.84 adds [lexical work-body diagnostics](docs/reference/diagnostics-and-limits.md#lexical-boundary-work-recovery):
independent errors inside `fault boundary` work appear together while uncertain
boundary histories stop before their handlers and outer continuations.

Release 0.83 extends [diagnostic recovery](docs/reference/diagnostics-and-limits.md#named-boundary-body-recovery)
to independent errors in named boundary work and contingency bodies. Retained-owner
checks stay conservative, and failed handlers suppress their dependent work and supervisors.

Release 0.82 adds named boundary contracts at `supervise` calls: owned inputs,
retained parameters and permission for faults to escape contingency. Hover makes
outcome-free completion explicit. Try [contingency.tellur](playground/contingency.tellur).

Release 0.81 resolves implementation-local `self` and associated aliases in call
hover, and preserves named generic result contracts with their origin mappings.
Try [trait_aliases.tellur](playground/trait_aliases.tellur) and the helper call in
[aggregate_traits.tellur](playground/aggregate_traits.tellur).

Release 0.78 extends [control-declaration diagnostics](docs/reference/diagnostics-and-limits.md#control-declaration-batches)
to independent named completion bindings. Public shapes, origin bindings and concrete
signatures have bounded batches; invalid shared preparation still stops its dependent
phases. Try [contract_binding_diagnostics.tellur](playground/contract_binding_diagnostics.tellur)
or [declaration_diagnostics.tellur](playground/declaration_diagnostics.tellur).

Release 0.76 adds [call-site contract hover](docs/language-reference.md#call-site-contract-hover):
hover over a control or manifest primitive call to see its checked inputs, outcomes,
borrow contracts and fault permission alongside the selected histories. Imported
contracts follow unsaved dependency edits; invalid source suppresses stale information.
Try `json_file.load` in [the independent library example](examples/json_file/main.tellur).

The source library also provides [bounded loopback HTTP](library/README.md#bounded-loopback-http)
on the existing 0.76 compiler: shared transport contracts, retained connections and
explicit contingency operations. Try the [independent greeting server](examples/http_hello/README.md).
The settings service now uses both shared libraries while keeping its own policy.

Release 0.75 starts the [shared source library](library/README.md): JSON string-map
file loading/replacement, explicit failure contracts and reusable Rust adapter modules.
The settings service and an [independent program](examples/json_file/main.tellur)
share the implementation. Module paths resolve from the manifest; native builds keep
the existing type checks and locked dependencies.

Release 0.74 extends diagnostic recovery across disjoint paths, grouped and lexical
scopes, statement arms and loop bodies. Errors on one path no longer hide independent
errors on another; joins touching stalled histories remain conservative. Try
[path_diagnostics.tellur](playground/path_diagnostics.tellur) and the
[path recovery rules](docs/reference/diagnostics-and-limits.md#path-and-scope-recovery).

Release 0.73 reports independent errors within straight-line control code, alongside
errors from independent controls. Dependent statements are suppressed, with a shared
32-error budget and no partial inferred hover. Try editing the two initializers in
[diagnostics.tellur](playground/diagnostics.tellur); the
[recovery rules](docs/reference/diagnostics-and-limits.md#statement-recovery) describe
where checking stops conservatively.

Release 0.72 checks initializer accesses in written order: a completed constructor
releases its nonescaping call borrows before the next initializer. Returned references
and pending resources stay protected. Try
[sequential_initializers.tellur](playground/sequential_initializers.tellur) and the
[initializer lifetime rules](docs/reference/construction.md#initializer-borrow-lifetimes).

Release 0.71 supports direct fields of fresh borrowed results: `text_view(borrow owner).text`.
External origins, shared/mutable permissions and unselected completion obligations
remain checked. Try [projected_construction.tellur](playground/projected_construction.tellur)
and [fresh result projections](docs/reference/construction.md#projecting-fresh-results).

Release 0.70 lets constructor calls compose inside raw borrowed records, options,
variants and named outcomes. Written evaluation order, loans and pending resources
remain checked. Try [composed_construction.tellur](playground/composed_construction.tellur)
and the [construction reference](docs/reference/construction.md#composing-borrowed-storage).

Release 0.69 adds [reference-bearing constructors](docs/reference/construction.md#reference-bearing-results).
Write `constructor(ref string input)[input]` to map the result's origin groups.
Shared and mutable borrows, nested fields and path-sensitive initialization use the
existing ownership rules. Try [borrowed_construction.tellur](playground/borrowed_construction.tellur).

Release 0.68 improves constructor lowering: completed values enter caller storage
without an intermediate outcome payload, and owned temporary projections transfer
the selected field without cloning its buffer. Native activation frames now reserve
only their local value/decision storage. Source syntax stays the same.

Release 0.67 adds [declared struct constructors](docs/reference/construction.md):
checked field initialization, nested owning calls, generic forwarding and partial-fault
cleanup. Public fields still permit raw brace construction. VS Code shows constructors
in the outline and inferred types on `auto`. Try [construction.tellur](playground/construction.tellur).

Release 0.66 adds [lexical fault boundaries and supervised recovery](docs/reference/contingency.md):
attached contingency blocks, retained owners, immutable `@fault`, preserved loop
state and entry-only process termination. The settings server now uses this syntax.
Normal outcomes and administrative recovery remain separate in both engines.

Release 0.65 adds [owning expressions and borrowed temporary arguments](docs/reference/ownership-expressions.md):
explicit `move`, generic owning forwarding, and anonymous call-duration storage
when a borrowed temporary cannot escape. Try
[owning_expressions.tellur](playground/owning_expressions.tellur).

Release 0.64 adds [implicit delta routing and named decisions](docs/reference/delta-routing.md):
`received = delta request();` imports outcome paths and payload names. Select
`[received.ready]`, or use a short path while it is unambiguous. Grouped arm
statements can introduce values for later use on that path. Try
[identity_deltas.tellur](playground/identity_deltas.tellur). The
[accepted control-flow revision](docs/control-flow-revision.md) tracks the remaining
ownership, lexical supervision and constructor checkpoints.

Release 0.62 adds [source faults](docs/reference/execution-failures.md):
`fault "reason";` terminates an invocation, public and trait contracts declare
`faults`, and private controls infer it. Fault-only controls omit `exits`.
Live must-complete resources need explicit fault dispositions. Try
`./bin/tellurc exec playground/source_faults.tellur -- --json -- false`.

Release 0.61 adopts the author's server layout in the [formatter](docs/reference/formatting.md):
short label dispositions remain inline, and long lists and conditions wrap at a
configurable width. Use **Format Document**, set `tellurLanguage.formatWidth`, or
run `./bin/tellurc format SOURCE --width 100 --write`.

The [active checkpoint queue](docs/language-revision.md#active-checkpoint-queue) now
prioritizes stable public interfaces, diagnostics, readable paths and a sustained
application with Rust library adapters. Further reference expansion is deferred;
first-class async and direct LLVM coroutine lowering remain architectural targets.
Release 0.46 adds [outcome contributor diagnostics](docs/reference/diagnostics-and-limits.md)
through stored values, adapters and merged path labels. Release 0.47 adds
`tellurc check SOURCE --json` and structured path witnesses for tools.
Release 0.48 adds [explicit process exit contracts](docs/reference/process-entry.md):
`@exit N` on named alternatives, checked entry overrides, and exact status propagation
through `tellurc exec`. Runnable playgrounds now declare their status policy.
Release 0.49 adds [retained Rust resources](docs/rust-adapters.md#retained-resources-049):
opaque owners survive between calls, with checked borrowing and explicit consumption.
Try the [file adapter](examples/retained_file/README.md) in VS Code or the terminal.
Release 0.50 adds the [loopback HTTP settings service](applications/settings/README.md#run-the-http-service),
with Tellur routing, validation and cleanup, bounded socket I/O and explicit shutdown
through connection budgets or idle timeouts.
Release 0.51 adds [precise expression diagnostics](docs/reference/diagnostics-and-limits.md#expression-locations)
to the compiler and VS Code: bad arguments, initializers and field accesses select
their source expressions, including multiline strings and imported generic code.
Release 0.52 adds [independent-body error reporting](docs/reference/diagnostics-and-limits.md#independent-body-errors):
`check` and VS Code show multiple independent errors while suppressing callers of
failed controls. Inferred hover remains unavailable until the current program checks.
Release 0.53 adds [optional path grouping](docs/language-reference.md#labels-and-optional-grouping):
`[path]: { ... }` gives a selection an explicit end without creating a new value or
lifetime scope. Ordinary labels stay unbraced. Try [path_groups.tellur](playground/path_groups.tellur).

Release 0.54 adds [direct arm exits](docs/language-reference.md#direct-arm-exits):
`missing => exit absent` immediately completes that history without a throwaway path
label. The settings application now uses them for validation and storage failures.
Try [arm_exits.tellur](playground/arm_exits.tellur).

Release 0.55 adds [selected-history hover](docs/language-reference.md#selected-history-hover):
hover over selected code to see resolved paths, enclosing intersections, and decision
origins. Try [history_hover.tellur](playground/history_hover.tellur). Existing `auto`
hover keeps showing inferred types.

Release 0.56 adds [source formatting](docs/reference/formatting.md): use VS Code's
**Format Document**, or `./bin/tellurc format SOURCE [--check | --write]`.
Labels, explicit groups, comments and literal spelling are preserved. Formatting
works on unsaved buffers and needs no manifest.

Release 0.57 adds [public contract snapshots and diffs](docs/reference/contract-snapshots.md):
`tellurc contract-snapshot` exports checked public interfaces, and `contract-diff`
compares them for review or CI. VS Code's **Tellur: Show Public Contract Snapshot**
uses current unsaved sources. The settings library now carries a checked baseline.

Release 0.58 adds [outcome absorption snapshots and diffs](docs/reference/outcome-absorption.md):
review changes to bounded internal contributors even when the public contract stays
equal. Use `absorption-snapshot`, `absorption-diff`, or VS Code's **Tellur: Show
Outcome Absorption Snapshot**. The settings application includes an evidence baseline;
`playground/outcome_absorption.tellur` is a small runnable example.

Release 0.59 adds [path and outcome explanations](docs/reference/explanations.md).
Use `tellurc explain` at a source position or for one control outcome, including
private controls. VS Code provides **Tellur: Explain Path Selection at Cursor** and
**Tellur: Explain Outcome Contributors**, with readable reports and source links.
Try `playground/path_explanations.tellur`.

Release 0.60 starts [faults and activation boundaries](docs/faults-and-activations.md):
returned execution failures now identify the actual control invocations and source
sites in reference/native reports. Try `playground/execution_faults.tellur` with
`--fuel 40`. The [reference](docs/reference/execution-failures.md) distinguishes
boundary failure, resource teardown and normal outcomes. Source fault syntax is available in 0.62;
isolated tasks remain subsequent work.

The [semantic foundation](docs/semantic-foundation.md) distinguishes compatibility,
ownership transfer, behavioral equivalence and provenance. Executable law tests and
`tellurc compare` provide finite scenario evidence; they do not establish general equivalence.
Expansion now also carries [independently checked rewrite certificates](docs/expansion-certificates.md).
Every expanded execution/build passes this validator; artifacts can be exported and checked separately.
The [simulation argument](docs/expansion-simulation.md) records its semantic premises.
Try [explicit public contracts and private forwarding](playground/public_contracts.tellur),
[local shared-reference collections](playground/borrowed_collections.tellur),
[aggregate trait origin contracts](playground/aggregate_traits.tellur),
[public nested-reference contracts](playground/public_carriers.tellur),
[stored carrier references](playground/stored_carriers.tellur),
[returned carrier references](playground/carrier_results.tellur),
[whole sum borrowing](playground/sum_carriers.tellur),
[named outcome and option origins](playground/outcome_origins.tellur),
[public variant contracts](playground/public_variants.tellur),
[borrowed outcomes](playground/borrowed_outcomes.tellur),
[opaque owner splits](playground/opaque_splits.tellur),
[exclusive carrier reborrows](playground/carrier_reborrows.tellur),
[record inputs and contained origins](playground/record_inputs.tellur),
[inferred exit payloads](playground/inferred_payloads.tellur),
[stored reference fields](playground/stored_references.tellur),
[borrowed trait contracts](playground/borrowed_traits.tellur),
[named struct construction](playground/named_fields.tellur) for readable nested data
and [file modules](docs/modules.md) for public interfaces and private representations.

[Protocol analysis](docs/protocol-analysis.md) now provides finite structural models,
residual queries and completed-trace inclusion with counterexamples. Its abstraction
forgets data and cannot establish concrete program equivalence.
[Source tail extraction](docs/extraction.md) now generates a typed helper and rechecks
its ownership and public contracts; it is a conservative refactoring, not a proof.

The maintained [settings service](applications/settings/README.md) now provides a
working persistent CLI and sequential loopback HTTP server. Its Tellur modules use
[typed Rust adapters](docs/rust-adapters.md) for file, JSON and socket operations,
with explicit Cargo dependencies and locked builds.
This starts library reuse while keeping first-class Tellur async and direct LLVM
coroutine lowering open.

## Build from a fresh clone

Install Rust/Cargo, then run:

```sh
git clone https://github.com/apology-is-policy/tellur.git
cd tellur
cargo build --locked --release --bins
./target/release/tellurc check playground/hello.tellur
./target/release/tellurc exec playground/hello.tellur
./target/release/tellurc build playground/hello.tellur --release
./playground/build/hello
```

Generated binaries and VSIX packages are not committed. Use `target/release/tellurc`
in place of `bin/tellurc` in the examples below, or copy the release binaries into
`bin/`. To build the editor extension, follow the
[VS Code development guide](editors/vscode/README.md). The project is licensed under
[Apache 2.0](LICENSE).

## Try programming

1. Open `tellur.code-workspace` in VS Code.
2. Open `playground/hello.tellur`.
3. Click **▶** or run **Tellur: Build and Run** (`⌘⌥R`).
4. Read the **Tellur Build** output, then try `stored_references.tellur`, `generic_controls.tellur`, `generic_data.tellur`, `named_fields.tellur`, `modules/main.tellur`, `data_types.tellur`, `forwarding.tellur`, `named_outcomes.tellur`, `stored_adapters.tellur`, `adapters.tellur`, `outcomes.tellur`, `collections.tellur`, `references.tellur`, `ownership.tellur`, `count.tellur`, `port.tellur`, or `read_file.tellur`.

Hover over a control declaration name to see its checked exit contract, including
inferred payload types. The [detailed language reference](docs/language-reference.md) explains the syntax,
ownership rules, primitive library, editor settings and command-line workflow. Its
companion chapters cover [grammar](docs/reference/grammar.md),
[diagnostics](docs/reference/diagnostics-and-limits.md),
[terminology](docs/reference/glossary.md) and [feature status](docs/reference/support-status.md).
Complete manual examples are checked with `cargo test --offline --test reference_manual`.
The local `tellur-language-0.48.0-darwin-arm64.vsix` contains the compiler and language
server for Apple Silicon macOS. It is installed on this machine; elsewhere use
**Extensions: Install from VSIX…**. Native builds need Rust/Cargo; the current
machine's toolchain and dependencies have been prepared. The resulting binaries
run independently of the compiler and Rust installation.

The playground uses **real** parsing, arithmetic, text and read-only file operations.
The broader `examples/` directory also includes scripted network/retry cases and
negative-edit suggestions. An entry `.tellur` file and its explicit imports form a
compilation; helpers can live in that file or in imported modules. Put new files beside `playground/manifest.json` to use
its types and primitive contracts.

## Command line

From this directory, use the supplied binaries:

```sh
./bin/tellurc check playground/hello.tellur
./bin/tellurc exec playground/forwarding.tellur
./bin/tellurc exec playground/adapters.tellur
./bin/tellurc exec playground/outcomes.tellur
./bin/tellurc exec playground/collections.tellur
./bin/tellurc exec playground/count.tellur
./bin/tellurc exec playground/port.tellur --entry parse_port -- 443
./bin/tellurc build playground/hello.tellur --release
./playground/build/hello
./bin/tellurc expand examples/refresh.tellur examples/manifest.json refresh
./bin/tellurc certify-expansion playground/forwarding.tellur playground/manifest.json main > expansion.json
./bin/tellurc verify-expansion playground/forwarding.tellur playground/manifest.json expansion.json
./bin/tellurc protocol playground/read_file.tellur playground/manifest.json show_file playground/protocol/read_failed.json
./bin/tellurc run examples/load.tellur examples/manifest.json examples/double-failure.json
```

`build` emits an executable, LLVM IR (`name.ll`), and the generated intermediate
source (`name.tellur-build/src/main.rs`). `--expanded` expands the chosen entry before
lowering; `--release` optimizes at LLVM level 2. Native executables accept `--json`,
`--entry`, `--scenario FILE`, and an optional `--fuel N` host limit. All declared
outcomes are normal completions; host errors return nonzero process status.

The native pipeline is checked graph → explicit Rust control flow → LLVM → native
binary. Controls become functions and loops remain loops. Executables do not contain
the graph interpreter. Rust is the implementation bridge to LLVM and supplies value
storage. LLVM verification runs on every build. Generated code retains all declared
outcomes rather than replacing inconvenient paths with undefined behavior.

The tested toolchain is Rust 1.97.1 with LLVM 22.1.6 on Apple Silicon macOS. Generated
program dependencies are pinned in an embedded Cargo.lock. Other platforms are not
yet validated. Foreign adapters are finite: a new manifest primitive can be scripted,
but requires an implementation before it can run live.

## Develop and verify

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bins
```

Tests include native compilation, so Cargo/Rust must be available. Set
`TELLUR_NATIVE_TARGET_DIR` to share a native dependency cache across builds; otherwise
it lives in `.tellur-cache` beside output binaries. `--offline` can be used for Cargo
commands once dependencies are cached. Compilation artifacts are ignored by Git.

The [conformance inventory](docs/conformance.md) maps the draft's requirements and
C01–C24 cases to implementation and evidence. Tests cover ownership, loops,
provenance, typed declarations, inferred contracts, pure calls, malformed inputs,
generated programs, native versus interpreted behavior
before/after expansion and O0/O2 optimization, live host adapters, and actual editor
build/run. This is finite evidence, not a soundness or termination proof.

The [VS Code development guide](editors/vscode/README.md) describes the extension
and its integration runner. The installed release bundles both `tellurc` and
`tellur-lsp`. Rebuild and override their paths when working on compiler code.

## Design and boundaries

The original [specification](docs/path-language-spec-v0.1.md) and
[vision](docs/path-language-vision.md) are unchanged. The
[design notes](docs/design-notes.md) record issues worth revisiting. The
[addendum](docs/v0.1-addendum.md) explicitly fixes the implementation profile rather
than hiding restrictions in the code.

Ordinary owned values and must-complete resources are checked separately. Strings
and bytes move in `control`; copying owned data requires `clone`. Shared/exclusive
references cross control boundaries and can be returned from input references.
Local lifetimes end at last use, with conservative path and loop rules. Hover shows
which inputs a returned borrow depends on. Grouped records and source variants carry
references through owned control inputs and results. Local borrowed variants/outcomes
retain references through capture, adapters and delta unpacking. Named-outcome and
option schemas support owned boundaries and whole borrowed inputs. Borrowed matching
reborrows active payloads without discharging handling obligations. Trait contracts
carry explicit storage and contained-origin mappings through generic dispatch.
Local arrays, vectors and maps support shared-reference elements. Exclusive elements,
nested/public collection contracts, reference rebinding and reference loop parameters
remain upcoming.
Historical `region` syntax retains reusable values for compatibility. Borrowed slices,
scheduled async tasks remain upcoming. Other
future work includes recursion, concurrency, richer proofs, general graph
extraction and the graphical editor. Constant constructors do not refine path
coverage or resource obligations. The editor reports bounded independent-control errors (one per body, up to 32).

See [generic control checking](docs/generic-controls.md) for the definition/specialization
boundary and the implemented capability rules.
[Concrete traits](docs/traits.md) add coherent implementations and explicitly selected
static calls; try `playground/traits.tellur` and `playground/trait_bounds.tellur`.
Generic bodies are checked against declared trait operations and associated permissions.

## Code map

| Location | Responsibility |
| --- | --- |
| `src/syntax.rs`, `src/manifest.rs` | Syntax and trusted type/primitive contracts |
| `src/bdd.rs`, `src/check.rs`, `src/inference.rs` | Symbolic path analysis, typing, coverage and ownership |
| `src/generics.rs`, `src/generic_controls.rs`, `src/traits.rs` | Concrete layouts, generic definition checks, trait conformance and static dispatch |
| `src/ir.rs`, `src/verify.rs` | Checked representation, provenance, inspection and structural verification |
| `src/expand.rs`, `src/runtime.rs` | Region expansion and scripted reference execution |
| `src/extract.rs` | Rechecked source tail extraction |
| `src/modules.rs` | File imports, identities, visibility and source maps |
| `src/native.rs`, `src/native/runtime.rs` | Native lowering, LLVM toolchain bridge and host adapters |
| `src/main.rs` | Check, interpret, inspect, expand, build and execute commands |
| `src/lsp.rs`, `editors/vscode/` | Editor diagnostics, highlighting, build/run and integration tests |
| `tests/` | Conformance, regressions, generated/differential tests and live host tests |
| `playground/` | Small live programs to edit and run |

The latest borrowing example is [origin_groups.tellur](playground/origin_groups.tellur).
It demonstrates separate returned-field dependencies and inferred forwarding.

The 0.63 checkpoint adds [contingency and supervised boundaries](docs/reference/contingency.md):
`@contingent`, explicit retained-owner handover and `supervise`, with per-request
isolation in the settings server. General annotation policies remain deferred.

### Direct async calls in 0.105

`async control`, `run` and direct `await` are available in the sequential execution
profile. Try `./bin/tellurc exec playground/async_entry.tellur` or read the
[reference chapter](docs/reference/async-execution.md). Concurrent scheduling,
async I/O and LLVM coroutine lowering remain later checkpoints.


### Local cold tasks in 0.106

`auto pending = defer operation(owned_arguments);` transfers arguments immediately
without starting the async body. `await pending` consumes the handle; moving it
transfers the obligation to await. Pending captures retain their fault dispositions.
Try `./bin/tellurc exec playground/cold_tasks.tellur` and hover its `auto` bindings.
Borrowed captures, public task types and concurrent scheduling remain ahead. The
[reference](docs/reference/async-execution.md) gives the exact current rules.


### Direct borrowed cold captures in 0.107

Deferred tasks can retain shared or exclusive borrows of existing reference-free
storage. Moving a task preserves the loans; consuming await releases them. Ordinary
reference arguments reborrow, while `move alias` transfers an exclusive reference.
Try `./bin/tellurc exec playground/borrowed_tasks.tellur`. Contained captures,
borrowed completion payloads and nested storage of borrowed tasks remain ahead.

### Borrowed task completions in 0.108

Await now delivers borrowed completion payloads, preserving each result's checked
origins through task moves, selected outcomes, records, sums and outcome adapters.
Unrelated captures become available at await; returned references protect their
sources until their last use. Try `./bin/tellurc exec playground/task_results.tellur`.
Contained captures and public task contracts remain ahead; execution is sequential.

### Owned reference-bearing captures in 0.109

Cold tasks can now capture records, variants and options containing references,
retaining each field's origins and nested carrier dependencies. Passing shared
reference records preserves ordinary copying; exclusive references transfer with
the owning value. Try `./bin/tellurc exec playground/contained_tasks.tellur`.
Borrowing a whole reference-bearing carrier remains the next bounded step.


## Whole borrowed carrier captures in 0.110

Cold tasks can borrow an entire reference-bearing record or sum. The task retains
both its storage and the references inside it. Awaited results preserve separate
storage and contained-origin contracts, including nested carriers and tagged sums.
A shared outer borrow cannot grant exclusive access to an inner reference.
Try `./bin/tellurc exec playground/carrier_tasks.tellur`; hover `auto` to inspect
the pending task and its detached borrowed completion. Public/escaping tasks,
nested task storage and scheduling remain separate work.


## Execution groundwork in 0.111

The reference interpreter now separates execution-wide storage, fuel and history
from resumable control state. Internal interleaving checks preserve borrowed storage,
frame retirement and isolated contingency state. Source execution remains sequential;
there is no new syntax or `spawn` support in this release.


## Task identity checks in 0.112

The reference executor now tracks each local cold task's original invocation and
one-shot await lifecycle. Normal completion cannot forget pending work; faults and
supervised recovery retire abandoned task records before contingency. Existing
source syntax, borrowing and sequential native behavior are unchanged. Readiness
and source `spawn` remain the next async implementation steps.


## Cold-await dispatch in 0.113

Cold-task awaits now use queued dispatch and parked caller state in the reference
executor. One-use completion waits reject early or duplicate delivery; borrowed
storage, fault traces and instruction quotas keep their existing behavior. Source
execution is still sequential. External readiness and scoped `spawn` remain ahead.

## Settings HTTP removal

The maintained settings service now exposes its existing removal operation through
`POST /remove` with `{"key":"theme"}`. It preserves other entries, returns 404 without
writing for absent keys, and reports uncertain replacement explicitly. See the
[HTTP workflow](applications/settings/README.md#run-the-http-service). This application
update runs on compiler 0.113 without changing compiler or host contracts.


## Named completion origin diagnostics in 0.114

Errors in a named result mapping such as `exits reply[input['source]]` now select
its written brackets. CLI and editor locations survive imports, generic checking
and unsaved changes; type annotations, body errors and missing mappings keep their
separate locations. Borrowing rules and diagnostic recovery order are unchanged.


## Borrowed-result origin diagnostics in 0.115

Invalid explicit borrowed-result origins now select their complete `from input`
clause, including contained selectors and multiline spelling. Imported generic and
trait signatures retain the original location. Missing clauses, unrelated body
errors and lifetime checking rules keep their existing behavior.


## Payload origin diagnostics in 0.116

Result payload mappings such as `view[input] value` now underline their written
brackets when an input, group count or borrowing permission is invalid. Imported,
generic and trait signatures retain exact locations through unsaved edits. Named
completion mappings, `from` clauses and type/body errors keep separate ranges;
repair restores checked hover information. Borrowing rules are unchanged.


## Suspended caller storage in 0.117

Cold awaits now retain callers in a separate suspension table. One-use registrations
validate the matching task and completion wait before restoring the original caller;
fault unwind retires the registration without producing a normal result. Borrowed
storage, diagnostic traces and host instruction quotas retain their existing behavior.
Source execution remains sequential; scoped concurrency is the next feature group.


## Ready context storage in 0.118

Prepared cold-task children now remain in separate ready storage until dispatch.
Selection validates the retained context and cannot replace an active control chain.
Borrowed storage, host instruction quotas and source effect order are unchanged.
This prepares the runtime for independent readiness; source execution remains sequential.

## Waiting contexts and readiness in 0.119

The reference runtime now has separate waiting storage and ready entries for either
starting or resuming a task. Readiness tokens carry an execution identity and a fresh
generation; signals only make the retained context eligible, and dispatch restores
that same context without replaying a call. Completion remains a separate one-use
operation. Host instruction quotas never select a sibling.

This is internal groundwork validated with checked borrowed execution. Source
`spawn`/`yield`, async host I/O and fault-domain settlement remain ahead; current
source execution and editor behavior stay sequential.

## Fault domain admission in 0.120

Reference execution now tracks live fault domains separately from invocation and
trace identities. Entry, named supervision and lexical boundaries receive distinct
domains; handling closes only the affected domain and restores its parent. Deferred
tasks acquire their executing domain when awaited, preserving existing cold-task
movement across lexical boundaries.

Stopped domains reject normal task admission, readiness and ready dispatch. Boundary
finalization and contingency admission check for retained execution contexts first.
These gates prepare for automatic sibling settlement; they do not enable source
concurrency, async host adapters or cancellation.

## Reference sibling settlement in 0.121

The reference driver now preserves a failed boundary while retiring its retained
children. Queued bodies do not start, waiting bodies do not resume normally, and
borrowers unwind before their owners. Unrelated domains remain available, and late
readiness signals cannot revive settled work. The original fault report survives.

When a stopped sibling is inside a nested boundary, its resources follow emergency
retirement without entering that sibling's contingency. Only the owning boundary
handles the original fault after settlement. This implements the accepted policy
in the private reference machinery; source `spawn`/`yield`, async host adapters and
fallible or suspending cleanup remain ahead.

## Native resource scopes in 0.122

Native fault cleanup now follows explicit resource ownership instead of allocation
snapshots. It preserves unrelated resource scopes, retires temporary owned inputs
before contingency, and keeps normal outgoing results and cold-task captures alive.
Retained response resources remain available to their handler. This backend groundwork
does not yet enable source concurrency or asynchronous host adapters.

## Native activation storage in 0.123

Native calls now use separately identified frame ranges, preserving borrowed slot
addresses while permitting independent retirement and storage reuse. Fault cleanup
releases only the affected frame's resources. This prepares persistent native
execution contexts; source task scheduling remains unimplemented.

## Native host contexts in 0.125

The native host now retains diagnostic, contingency and resource-selection state
together, with checked admission and retirement of independent ownership roots.
Compiled context tests share these runtime operations and verify that a failed root
preserves unrelated owners and the host's original context. Public calls remain
synchronous; source scheduling and native sibling settlement are still ahead.

## Task queue and completion joins in 0.126

The reference runtime now separates queue admission from one-use completion joins.
A completed task remains an obligation until its result is delivered and consumed;
cold await still admits both operations atomically. This internal foundation
preserves existing source behavior. Independent completion-payload storage and
source `spawn`/`yield` remain ahead.

## Retained runtime completions in 0.127

Completed task results now live in shared reference-runtime storage until delivery
or emergency retirement. Fault settlement releases their borrows and unwinds parked
callers before the owning contingency. Cold await still delivers immediately;
source scheduling and native completion ownership remain ahead.

## Administrative process settlement in 0.128

Reference process termination now closes task admission and retires all registered
ready/waiting chains, completed results and parked callers before delivering its
original status. Cleanup does not resume ordinary code or nested contingencies,
consume source fuel, or turn termination into an outcome/fault. Written termination
remains restricted to the executable entry's outermost lexical contingency.
Source `spawn`/`yield` and shared native scheduling remain unimplemented.

## Native result resource adoption in 0.129

The shared native host now validates and transfers a completed context's owned
result resources into a receiving scope without selecting either context or following
borrowed references. Foreign/stale/duplicate owners and incomplete result graphs
reject before mutation. The compiled completion model uses this API; source execution
remains synchronous and scheduler integration is still ahead.

## Shared native retained execution in 0.130

The native continuation, context/completion and fault-domain implementation now lives
in normal generated support. Compiled regression probes use that same code for
retained results, readiness and fault/process cleanup; their scheduling drivers remain
test-only. Default native entries still use synchronous control flow. Source task
queue/wait wiring, resumable lowering and `spawn`/`yield` remain ahead.

## Shared task protocol and native invocation owners in 0.131

Reference and native support now share task queue, completion-join and readiness
transitions. Retained native activations have separate invocation owners even when
frames are reused; normal return requires settled tasks, and emergency retirement
invalidates only the affected owner's obligations. Native task payload/queue binding
and source `spawn`/`yield` remain ahead. Current source behavior is unchanged.

## Bound native task payloads in 0.132

Internal native cold-task APIs now bind captures to the original invocation, host
and compiled producer. Invalid queue admission preserves the handle and captures;
FIFO dispatch transfers them once. Source generation remains synchronous while
retained context dispatch and completion integration continue.

## Task-bound native contexts in 0.133

Internal queued tasks now retain their original parent and completion wait through
native context admission, readiness, result delivery and fault/process cleanup.
Failed admission preserves captures. Source callback and scheduler integration remain
ahead; current source execution stays synchronous.

## Scoped native task retirement in 0.134

Retained native scopes now retire discarded cold-task obligations during fault and
recovery cleanup, while preserving moved handles and borrowed suppliers. Normal
scope exits and loop restarts still require task completion. Source callback and
scheduler integration remain ahead.

## Sequential native await contexts in 0.135

Internal cold-await context chains now preserve the original diagnostic activations
through readiness, result delivery and faults. Exact report and resource checks cover
nested awaits and owning contingencies. Source callback integration remains ahead;
current source execution stays synchronous.

## Retained native await handoffs in 0.136

Internal compiled callbacks now retain cold-await captures and their return callback
across explicit admission and one-use result delivery. Failure and process cleanup
retire pending values before suppliers. Generated source callback integration remains
next; current source execution stays synchronous.

## Generated retained await callbacks in 0.137

Compiled probes now lower cold tasks into bound captures and explicit retained await
handoffs. Their drivers preserve original diagnostic chains, result ownership and
fault cleanup. Production lowering promotion is next; source execution remains
sequential and no scheduler syntax is enabled.

## Production retained native callbacks in 0.138

Normal native builds now use retained compiled callbacks for calls, constructors,
loops, boundaries and cold awaits. The shared sequential entry driver preserves loans,
resource cleanup, fuel and fault reports. Probe scheduling controls stay internal;
source execution remains sequential and no new syntax is enabled.

## Native independent-child cold awaits in 0.139

Internal native children can now perform nested cold awaits while preserving their
original fault cause, one-use result delivery and resource cleanup order. The shared
runtime has host and compiled O0/O2 evidence. Source execution remains sequential;
`spawn`, `yield` and parent awaits alongside live children remain gated.

## Prepared native owner awaits in 0.140

The internal native host admits an already-prepared cold await alongside independent
children. Sibling faults restore the original owning diagnostic chain before retiring
awaited work, including nested boundaries and active handlers. Normal parent execution
with live children and source scheduling remain gated.

## Native retirement checks in 0.141

Retained native frames, loops and lexical boundaries now check child/result
membership before runtime storage retirement. Final-frame domain checks also precede
frame release. Parent execution with live children remains gated pending checks
before generated callbacks move operands; source scheduling is not enabled.

## Native invocation ownership in 0.142

Every child and retained result now keeps its original parent invocation, preventing
another invocation in the same context from adopting it. Lower-level selection
rejects live children before callbacks or pending delivery can change state.
Fine-grained parent progress and source scheduling remain gated.

## Native callback access plans in 0.143

Generated native callbacks now describe symbolic reads, moves, borrows, writes and
retirement before evaluation. Normal cold-await result writes have a separate plan.
Queries preserve execution state; opaque accesses remain explicit. Parent progress
with live children and source scheduling still require further dependency checks.

## Native access storage resolution in 0.144

Native access queries now identify live storage and indirect reference-holder reads,
without consuming values or resource owners. Resolved allocation identities reject
retirement and offset reuse. Captured-loan checks and source scheduling remain ahead;
parent execution with live children is still guarded.

## Retained native capture descriptions in 0.145

Native contexts now retain bounded descriptions of captured reference storage through
waiting, completion and pending delivery. Carrier contents preserve shared/exclusive
attenuation; unknown metadata leaves execution unchanged. Checked origin guards and
access-interference checks remain necessary before enabling parent progress.

## Partial checked reference origins in 0.146

Checked reference expressions now preserve partial origin paths, including carrier
guards, before local aliases disappear. Expansion remaps this evidence, and certificate
validation rejects its removal or alteration. Execution and editor contracts keep their
existing behavior. Native values, complete loan evidence and parent-access permission
remain separate integration work.

## Bound native reference origins in 0.147

Native reference values now retain allocation identities and partial checked origins,
including carrier guards. Copies, moves, reborrows and result delivery preserve them;
capture snapshots carry them through waiting and completion. Symbolic origin fields
are never read as runtime projections. Missing or bounded-out evidence remains
unavailable, with unchanged source behavior. Complete loan authority and access
interference checks still gate parent progress and source scheduling.

## Conservative direct reference completeness in 0.148

Checked reference evidence now retains ancestor origins. A bounded set of direct,
unguarded references can carry complete dependency coverage; guarded, contained and
conditional cases remain partial. Native reborrows require complete incoming evidence,
and partial dependencies stay partial through result merging. Expansion certificates
protect the coverage status. Parent execution and source scheduling remain gated.

## Immutable native interference queries in 0.149

Native support can compare one retained child's complete dependencies with its owner's
next local callback under exact invocation and fault-domain identity. Results distinguish
disjoint storage, conservative overlap and inconclusive evidence. Queries preserve all
state, and both parent-selection gates remain in place. Source concurrency is unchanged.

## Native borrower coverage in 0.150

An immutable aggregate query now reconciles retained child contexts with fault-domain
membership, including descendants, readiness, completed results and pending await
captures/delivery. Missing, incomplete or mismatched evidence stays inconclusive.
Queries report coverage counts without granting execution; both parent gates remain.

## One-transition native admission in 0.151

The native host can explicitly admit one supported local callback or normal await
result delivery beside retained children, after a fresh complete borrower census
proves disjointness. Private permission is consumed immediately and never covers the
next callback. Ordinary quota drivers and source execution retain their existing gates.

## Bounded native admission in 0.152

An explicit host batch now recomputes complete borrower coverage before every supported
callback and pending delivery. It reports progress and the reason it stopped, preserving
successful transitions before a later rejection. Callback quotas and administrative
delivery counts remain distinct; source scheduling and ordinary drivers are unchanged.

## Local guarded reference coverage in 0.153

Fresh local reborrows can now carry complete conservative holder dependencies through
native capture and result delivery. Whole borrowed holder ancestors remain protected;
copied shared fields gain no invented holder guard. Indirect guards, returned permission
families and conditional/contained proof gaps remain partial. Try
`./bin/tellurc exec playground/local_guarded_tasks.tellur`.

## Local borrowed carrier coverage in 0.154

Fresh borrows of local records can carry complete holder and stored-reference
coverage when every leaf has unconditional checked provenance. Nested records and
local field projections are supported; aliases, indirect holders, returned permission
families and reference-to-carrier leaves remain partial. Runtime capture still requires
complete incoming formation evidence for every reachable reference. Try
`./bin/tellurc exec playground/local_carrier_tasks.tellur`.

## Local carrier aliases in 0.155

Erased local carrier aliases can retain complete dependencies through moves and
projected reborrows. Every broad borrowed ancestor and all its stored owners remain
covered, even owners outside the projected runtime value. Conditional/received carriers,
permission families, tagged contents and references to carriers remain partial. Try
`./bin/tellurc exec playground/aliased_carrier_tasks.tellur`.

## Incoming record-leaf evidence in 0.156

Supported singleton record input fields retain already-bound reference evidence during
private access-slot materialization. Exact allocation/path checks preserve complete
caller evidence without promoting partial or invalid metadata. Public permissions,
source execution and scheduling remain unchanged; indirect holder closure is still queued.

## Shared incoming leaf dependencies in 0.157

Singleton shared input leaves preserve complete incoming dependency coverage through
reborrows, local aliases and projections. Broad pointee ancestry remains protected, while
copying a shared field does not retain its record holder. Partial caller evidence and
unsupported permission cases remain conservative. Try
`./bin/tellurc exec playground/incoming_leaf_tasks.tellur`.

## Native holder snapshots in 0.158

Native runtime support now retains bounded dependency snapshots of incoming aggregate
holders and their stored owners. Partial or invalid formation evidence cannot become
complete. Host and generated tests establish this prerequisite; compiler admission of
exclusive input and indirect holder guards remains gated.

## Exclusive incoming leaf dependencies in 0.159

Supported singleton exclusive input leaves retain their original pointee and the actual
borrowed holder, including other stored owners. Direct and local aliased reborrows carry
an explicit checked relationship; partial or invalid runtime evidence stays conservative.
Shared field copies remain holder-free. Try
`./bin/tellurc exec playground/guarded_input_tasks.tellur`.

## Incoming carrier dependencies in 0.160

Whole incoming ordinary carriers retain their full original aggregate and every stored owner
through direct forwarding, projected reborrows and local aliases. Partial or changed caller
metadata stays conservative, including evidence outside the selected projection. Run
`./bin/tellurc exec playground/incoming_carrier_tasks.tellur`.

## Checked cold-await preparation in 0.161

Native host support can advance checked local work to a consuming cold await while retained
children remain live. Handle and wait-capacity validation precedes source changes; an explicit
handoff stops preparation before the child runs. Source execution remains sequential. Run
`./bin/tellurc exec playground/owner_await_progress.tellur` for the maintained source example.

## Direct-await handoffs in 0.162

Direct await now retains its caller and hands off explicitly through the existing task
completion machinery. Nested direct and cold awaits preserve borrowed storage and owning
fault cleanup. Source execution remains sequential. Run
`./bin/tellurc exec playground/direct_await.tellur` for the maintained source example.

## Retained started-task joins in 0.163

Native runtime groundwork now retains unjoined child work and separates consuming join
handles from queued captures. Owned resources and borrowed results survive until delivery
or owning fault cleanup. Source programs still use sequential direct and cold awaits;
`spawn` and `yield` remain planned.

## Awaited invocation children in 0.164

Native runtime groundwork now composes independent children with awaited invocation owners.
Borrowed results and fault cleanup preserve the original caller chain. Source execution
remains sequential while started-task and scheduling integration continues.

## Owned source scheduling in 0.166

`spawn` queues invocation-owned children; `yield` and `await` explicitly hand off
execution. Every normal path must await each child. The first profile uses owned,
reference-free captures/results and joins before another source call or fault
boundary. Child faults settle siblings before the owning contingency.
See the structured tasks playground and the current async execution reference.

Direct shared/exclusive spawned captures: `./bin/tellurc exec playground/borrowed_spawn.tellur playground/manifest.json`. Loans remain live until the consuming join.

Direct borrowed spawned results: `./bin/tellurc exec playground/borrowed_spawn_results.tellur playground/manifest.json`. Selected result origins remain borrowed through final use.
