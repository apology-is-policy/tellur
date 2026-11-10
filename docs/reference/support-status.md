# Feature status

[Language reference](../language-reference.md) · [Roadmap](../language-revision.md)

This matrix describes **compiler 0.244.0**, not the full planned language. “Supported”
means implemented within the stated boundary. A feature in one local position does
not imply it is accepted at every public, generic, collection or loop boundary.
The linked regression suites give more detailed acceptance and rejection evidence.

Source `spawn`/`yield` now use explicit cooperative scheduling in the reference and
native executors. The current profile admits reference-free values, direct borrows and nested owned record/option/variant/named-outcome
captures/results with direct reference leaves and declared origins, plus borrowed
record/option/variant/named-outcome captures with nested owned contents/direct reference leaves. It
requires normal-path joins. Checked helpers may enter named/lexical boundaries and
reach explicit handoffs beside children when retained owners permit fault settlement.
Children keep their original invocation and fault boundary; direct parent boundary
statements still require joining children first. Cold/direct awaits may borrow parent storage beside live
children. Queued and completed children retain their fault obligations. Spawn requires a
checked producer whose suspended owners, including nested boundary owners and
transitive callees, permit enclosing fault settlement.
See [owned spawned children](async-execution.md#owned-spawned-children).

## Native backends

The default Rust backend implements the language profile below. Since 0.235 the
opt-in direct LLVM backend admits a [bounded synchronous scalar/aggregate profile](llvm-backend.md)
on macOS arm64, extended in 0.236 to nested calls and in 0.237 to owned records, field-sensitive
moves, assignment and temporary projections, then in 0.238 to options/variants/named
outcome values, captures and selected payload decisions, then in 0.239 to returning
synchronous primitives with owned arguments and supported result signatures.
Compiler 0.240 gives admitted locals stable activation storage, shared by synchronous
children and retired on completion/failure; 0.241 adds direct shared/exclusive references to reference-free values, borrowed
control arguments/results and borrowed decisions with existing origin evidence.
Compiler 0.242 adds reference-bearing aggregates, whole borrowed carriers and guarded
leaf evidence. Compiler 0.243 adds opaque/resource values and returning synchronous
borrowed host signatures. Compiler 0.244 adds arrays/vectors/maps, collection
literals/lengths/operations, element borrows, local shared-reference elements and
resource transfers. Loops, boundaries, async host operations and Rust adapters remain gated.
Unsupported checked programs fail with `E_BACKEND` before build
outputs are created. Backend selection does not change language checking.

## Language and data

| Area | Current support | Boundary / next work | Evidence |
| --- | --- | --- | --- |
| Controls and paths | Typed parameters, exhaustive delta, selected histories, inferred contracts, aliases, optional grouping and lexical scopes | No recursive calls or value-level proof of path feasibility | [Revision tests](../../tests/revision.rs), [path grouping](../../tests/path_groups.rs) |
| Adapters and forwarding | Direct arm exits, nested tail adapters, stored adapters, explicit compatible forwarding, one-nullary-completion statements, flat implicit routing, named decisions and grouped arm statements | Inline adapters remain exhaustive; no general expression-position adapters | [Delta routing](delta-routing.md), [identity tests](../../tests/identity_deltas.rs), [Adapter tests](../../tests/adapters.rs), [forwarding tests](../../tests/forward.rs) |
| Completed outcomes | Capture now, handle later; named and producer-indexed contracts; mandatory handling | Not futures; no implicit structural assignment across unrelated named types | [Outcome tests](../../tests/outcomes.rs), [named contracts](../../tests/named_outcomes.rs) |
| Payload inference | Private `exits auto` and selected nonempty `label...`; local/pattern `auto` | Explicit public and trait contracts; no general signature wildcard | [Inference tests](../../tests/payload_inference.rs) |
| Data declarations | Nominal structs, closed variants, wrappers, named/positional struct construction | No recursive layouts, inheritance, defaults or update spreads | [Data tests](../../tests/data_types.rs), [field tests](../../tests/named_fields.rs) |
| Public control contracts | Explicit labels, payloads and borrowed-result mappings, including unused generics; bounded forwarding/stored-outcome/adapter contributor notes | Loop-carried values, indirect aliases, dynamic collections and cross-call payload dependencies are not traced | [Public contract tests](../../tests/public_contracts.rs), [example](../../playground/public_contracts.tellur) |
| Diagnostic reports | Versioned `check --json`, expression/token/written field, loop, local, public-interface and named-completion type ranges, named/payload/schema origin brackets, schema storage arrows, duplicate/unused declared group names, public inference clauses, missing public-origin result/completion types and borrowed-result `from` ranges, bounded independent implementation-local, origin-schema, constructor-definition, generic-bound, trait-declaration, public-shape, named-completion, signature, independent-body, statement, disjoint-path and nested-scope errors, authored contributor statement/outcome/declaration-clause ranges, related fallback points and structured CLI/LSP path witnesses | Fault/adapter/header and preparation recovery remain limited; joins touching stalled histories are suppressed; generated whole-contract and contributor locations may still be points | [Report tests](../../tests/diagnostics.rs), [format](diagnostics-and-limits.md#machine-readable-checking) |
| Modules | Explicit relative imports and namespaces; private by default; public declarations/fields | No package-name resolver or package manager | [Module tests](../../tests/modules.rs) |
| Generic data | Explicit first-order type parameters and specialization | No argument inference, data bounds or higher-kinded parameters | [Generic data tests](../../tests/generic_data.rs) |
| Generic controls | Universal definition checking, sealed/user-trait bounds, explicit specialization | No recursive calls, type-argument inference or reference types as generic arguments | [Control tests](../../tests/generic_controls.rs), [bounds](../../tests/trait_bounds.rs) |
| Traits | Explicit implementations, associated types/capabilities, static qualified dispatch, direct/grouped borrowed results and abstract origin witnesses | No abstract associated-type origin-shape bounds, blanket implementations, default methods, associated equality/trait bounds or runtime polymorphism | [Trait tests](../../tests/traits.rs), [borrowed contracts](../../tests/lifetime_contracts.rs) |
| Repetition | Checked tail loops and explicit state transfer | No nested source loops or reference-bearing loop state | [Loop tests](../../tests/loops.rs) |

## Ownership and reference boundaries

| Area | Current support | Boundary / next work | Evidence |
| --- | --- | --- | --- |
| Declared constructors | One private/public constructor, path-sensitive field initialization, owning/reference-bearing results, explicit result origins, generic nested calls, raw borrowed aggregate composition, direct fresh-result projections, sequential call-borrow lifetimes, fault cleanup, native delivery into caller storage, editor support | Unselected resource/outcome fields require a named owner; constructor-local isolation, overloads and broader physical copy elision remain queued | [Construction](construction.md), [tests](../../tests/constructors.rs) |
| Owning values | Copy/move/explicit clone/disposal, assignment, partial record moves, explicit move expressions and nested owning construction | No user-defined copy/clone/drop behavior or guaranteed physical copy elision | [Owning expressions](ownership-expressions.md), [value tests](../../tests/values.rs), [capabilities](../../tests/capabilities.rs) |
| Direct references | Shared/exclusive calls and returns, reborrowing, last use, field separation, call-duration ordinary temporary borrows | No escaping temporary borrows, reference rebinding or references to references | [Reference tests](../../tests/references.rs), [owning expression tests](../../tests/explicit_moves.rs) |
| Local reference fields | Nested structs, extraction, independent retainers and field last use | Assignment/rebinding of reference-bearing storage remains gated | [Stored record tests](../../tests/stored_records.rs) |
| Public record origins | Ordered groups, returned mappings, whole-record inputs, `input['group]` | Grouped variants, named outcomes and options compose with records; aggregate trait contracts require explicit mappings | [Origin records](../../tests/origin_records.rs), [record inputs](../../tests/record_inputs.rs) |
| Carrier reborrows | Shared/exclusive child access, preserved storage guards and last-use restoration | A shared carrier cannot grant exclusive access | [Carrier tests](../../tests/carrier_reborrows.rs) |
| Opaque splits | Multiple checked independent result permissions from one input anchor | Public anchor remains opaque; independent calls do not imply disjointness | [Split tests](../../tests/opaque_splits.rs) |
| Local borrowed sums | References in variants/options/outcomes, capture/adapters, arbitrary finite record/tag nesting | Ungrouped sums can be owned or borrowed locally; public boundaries require complete schemas | [Sum tests](../../tests/borrowed_sums.rs), [nested example](../../playground/nested_borrows.tellur) |
| Public variant origins | Owned grouped variants cross calls and returns; constructor-guarded origins, contained selectors, nested/generic layouts, capture and forwarding | Direct constructor arguments returning contained borrows remain gated | [Public sum tests](../../tests/public_sums.rs), [example](../../playground/public_variants.tellur) |
| Named-outcome and option origins | Named schemas and completion bindings, inherited option groups, owned calls/results, contained selectors, generic/module composition | Anonymous outcome schemas remain gated; grouped trait contracts require explicit mappings | [Origin tests](../../tests/outcome_origins.rs), [example](../../playground/outcome_origins.tellur) |
| Collections | Arrays/vectors/maps, explicit bounds outcomes, resources, borrowed element access and local shared-reference elements | Exclusive/nested elements, public collection-origin contracts, slices and iterators remain queued | [Collection tests](../../tests/collections.rs) |
| Borrowed sum carriers | Whole shared/exclusive inputs, active payload reborrows, nested carriers, retained handling obligations | Complete schemas required at public boundaries; local borrows use concrete provenance | [Sum carrier tests](../../tests/sum_carriers.rs) |
| Carrier reference results | Direct shared/exclusive results, inferred/declared contained origins, storage retention and forwarding | Local capture and aggregate storage work; aggregate trait results require explicit mappings | [Carrier result tests](../../tests/carrier_results.rs), [example](../../playground/carrier_results.tellur) |
| Stored carrier references | Local recursive dependency trees, capture/adapters, shared retainers, nested/borrowed unpacking and last use | Public schemas map each storage and contained-origin layer; aggregate trait contracts require explicit mappings | [Stored carrier tests](../../tests/stored_carriers.rs), [example](../../playground/stored_carriers.tellur) |
| Public nested references | Separate storage arrows and referent brackets; owned/borrowed boundaries, nested tags, capture and forwarding | Bind constructor arguments before selecting nested origins; abstract type-parameter origin shapes remain unsupported | [Public carrier tests](../../tests/public_carriers.rs), [example](../../playground/public_carriers.tellur) |
| Resources | Must-complete obligations through aggregates, replacement and explicit draining | No implicit fallible cleanup or abandoned outcome disposal | [Completion tests](../../tests/completion.rs), [ownership tests](../../tests/ownership.rs) |

“Borrowed element access” means obtaining a reference into an owning collection.
“Stored reference-bearing elements” means storing references to external owners as
collection elements. Owning-element access and local storage of direct shared
references are supported. Exclusive and nested element types and public collection
contracts remain queued. Grouped source variants now work as whole owned parameters and result fields.
Grouped variants, named outcomes and builtin options also support whole borrowed
inputs and non-consuming decisions. Direct controls may return selected references
or references to whole reference-bearing aggregates. The latter can be stored
inside local aggregates and captured outcomes, or cross public container boundaries
with complete schemas for every nested reference layer.

## Tools, analyses and planned execution features

| Area | Current support | Limit |
| --- | --- | --- |
| Rust source adapters | Typed wrappers, closed result enums, owned scalar/collection data, direct opaque retained resources, call-duration shared borrows, explicit dependencies, explicit shared source modules and locked builds | No stable binary ABI, foreign returned borrows, exclusive mutation, aggregate resource boundary or async adapters (async host operations are polled after retry delays or descriptor readiness) |
| [Source library](../../library/README.md) | JSON string-map file controls with explicit limits and distinct missing/invalid/read/encode/write/uncertain outcomes; shared Rust mechanisms | Explicit file imports and project manifest signatures; no implicit prelude, package resolver or crash-durable transaction guarantee |
| Maintained application | Persistent settings load/get/set/remove CLI and bounded sequential loopback HTTP; Tellur routing/validation/cleanup and Rust mechanisms | Limited HTTP profile; no crash-durable transaction guarantee |
| CLI and native build | Check, execute, build standalone executable through Rust/Cargo and LLVM; dense per-activation storage | No direct LLVM coroutine lowering |
| [Formatting](formatting.md) | CLI stdout/check/write and VS Code Format Document; configurable width, multiline lists/conditions, inline label dispositions; token, comment and literal preservation | Whole documents; soft width target, no comment reflow, range formatting or project-wide discovery |
| VS Code | Highlighting, unsaved diagnostics with related forwarding locations, Outline, declaration, concrete/symbolic constructor and trait, ordinary concrete/symbolic call and supervision contracts, lexical implementation aliases, named result origins, inferred `auto` hover, bounded selected-history hover, build/run | Bounded declaration/body/statement/path diagnostics, including fault contexts, explicit/inferred exit-adapter arms, explicit stored-adapter arms and distinct inferred stored-result labels; constructor-call privacy, generic-bound and trait-declaration batches also preserve phase barriers; headers, uncertain same-label stored arms and fault-context adapter/terminal recovery remain limited; hover requires a successful check. General expression-type hover remains a follow-up; named boundary declarations have no generic parameters |
| Source libraries | JSON string-map files and bounded loopback HTTP, shared with the settings application and independent examples | Explicit imports and manifest primitives; synchronous fixed profiles, no package resolver or general HTTP/async stack |
| Expansion | Checked rewrite certificates, retained invocation/decision provenance | Not arbitrary equivalence or termination checking |
| [Contract snapshots](contract-snapshots.md) | Checked public declarations across the explicit import graph, symbolic permissions, stable JSON, structural diffs and editor export | No automatic compatibility verdict or ABI promise |
| [Explanations](explanations.md) | CLI/editor source-position selections and targeted private/public outcome contributors, Markdown and JSON | Bounded structural evidence; generic/trait outcome roots and full history queries remain unavailable |
| [Outcome absorption](outcome-absorption.md) | Checked bounded internal contributor sets, distinct source sites, public/evidence diffs and unsaved editor export | Explicit gaps for loop-carried/indirect/collection/cross-call payload flows and generic roots; no behavioral-equivalence verdict |
| Tail extraction | Typed helper generation and whole-candidate rechecking | Not general graph extraction or a full equivalence certificate |
| Scenario comparison | Concrete supplied-case comparisons and counterexamples | Agreement on those cases only |
| Protocol analysis | Finite graph/residual queries, completed finite trace-language comparison | Forgets data and concrete feasibility; supervision graphs are rejected; budgets can make results inconclusive |
| [Execution failures](execution-failures.md) | Source faults, transitive private inference, explicit public/trait permissions, retained-owner release dispositions; reference/native reports with invocation IDs and source/expansion sites | No fault-valued inline leaves, general deep-local fault registration, panic recovery or asynchronous fault settlement |
| [Contingency](contingency.md) | Named owned-input boundaries, retained handover, checked contingent calls/traits, outcome-free supervision, terminal escalation and activation-local storage | Sequential source-fault isolation only; no outer borrows, recursive calls or general execution-mode flags |
| [Async execution](async-execution.md) | Async calls, cold tasks with contained/borrowed captures, consuming await and declared result origins; cooperative source spawn/yield with reference-free values, direct borrows and nested owned record/option/variant/named-outcome captures/results and borrowed record/option/variant/named-outcome captures plus whole borrowed record/option/variant/named-outcome results and record/option/variant/named-outcome reference chains; dynamic task sets (`tasks<C>`, `spawn … into`, `await next` in completion order, `count`) with owned reference-free members in the reference executor and native executables; checked helpers and cold/direct awaits beside children; manifest async host operations entered like async controls (await, run, defer, spawn, spawn into), with scripted readiness in the reference interpreter and native executables, and polled Rust adapters natively, including owned resource results and consumed resources with a fault disposition, and read or write readiness; the loopback HTTP library accepts, receives, sends and finishes connections through them, and the settings service serves connections concurrently as task-set members; cooperative cancellation (`cancel child;`, `cancel set;`, structured propagation, `cancelled` alternatives on host operations entered by await or run) in the reference executor and native executables, including live polled adapters; opt-in launch signal cancellation (`launch.<entry>.signals: "cancel"`, scenario `$signal` entries), used by the settings service for graceful shutdown; the Milestone 5 cancellation specimen runs in every executor | No anonymous-outcome/collection spawn carriers, borrowed-handle wrappers, direct inline boundaries or boundary-bearing helper handoffs with live children, escaping/public task types, borrowed task-set captures/results, cleanup deadlines after a launch signal or LLVM coroutine lowering (a standalone, hand-lowered experiment exists: `experiments/coroutines`) |
| Runtime polymorphism | Roadmap only | Traits currently dispatch statically |

The [conformance inventory](../conformance.md) records release validation. The
[roadmap queue](../language-revision.md#active-checkpoint-queue) remains the authority
for execution order, not a promise that every queued syntax is settled.

Trait origin coverage: [aggregate trait tests](../../tests/aggregate_traits.rs) and
[the runnable example](../../playground/aggregate_traits.tellur).

Local shared-reference collections: [tests](../../tests/borrowed_collections.rs),
[example](../../playground/borrowed_collections.tellur).

## Runtime development record

The versioned notes below describe earlier checkpoints; the matrix above describes
the current source profile.

### Executable status contracts (0.48)

Named outcome alternatives accept `@exit N` with N in 0..255. Native generation
checks complete final-entry mappings, including explicit manifest overrides.
`exec` propagates numeric status; JSON/scenario output retains named outcomes.
Library contracts remain annotation-optional. See [process entry](process-entry.md).

### Lexical supervision (0.66)

Lexical boundaries, retained owning captures, structured immutable `@fault`,
supervised recovery preserving loop state, and entry-only process termination are
implemented in both engines and expansion certificates. The settings HTTP server
uses this form. Reference-bearing captures, direct protected-work access to
supervisor state, nested tail loops and asynchronous boundaries remain unsupported.
Constructor bodies are supported in 0.67; isolation inside them remains gated. Protocol graph analysis and source extraction reject
lexical isolation rather than erasing its administrative edges.

Native builds use retained compiled callbacks and a sequential entry driver since
0.138. Calls, constructors, loops, named/lexical boundaries and cold awaits retain
local frames, loans and original diagnostic state through completion and cleanup.
The shared execution support introduced in 0.130 is now used by normal entry lowering;
probe quota controls and independent-root schedules remain excluded. See
[production retained callbacks](../async-execution.md#production-retained-native-callbacks-e1d2d3k).

Cold captures carry host, invocation, task and exact producer identities. Queue/wait
admission validates those identities before transfer; retained child contexts deliver
adopted results once to the original callback. Legal cold lexical movement and normal
await obligations are unchanged. Discarded cold obligations retire during fault or
recovery cleanup, while normal pending discard remains rejected. Borrowed captures
do not retire their suppliers; declared resource dispositions still govern cleanup.
See [task-bound contexts](../async-execution.md#task-bound-native-contexts-e1d2d3f)
and [scoped retirement](../async-execution.md#scoped-native-task-retirement-e1d2d3g).

Sequential await chains preserve original source/call sites, activation identities,
fault causes and raw errors for final reports. Await handoffs and one-use result
callbacks are distinct from host quota pauses, which do not introduce source
interleaving. See [retained await handoffs](../async-execution.md#retained-native-await-handoffs-e1d2d3i).
Since 0.139, independently admitted native children may perform exclusive cold
awaits, preserving the independent child as the outer fault cause and retiring result
borrowers before suppliers. Parent awaits alongside live children and source
concurrency remain gated; the internal readiness protocol provides no source scheduler
or async host API. See [independent-child cold awaits](../async-execution.md#independent-child-cold-awaits-e1d2d3l).

Since 0.140, an already-prepared owner await can enter beside independent native
children. Sibling faults restore the original owning tracker from the suspended
chain or its saved nested-boundary parent before resource retirement. Normal parent
execution with live children remains rejected; this adds no source scheduler. See
[prepared owner awaits](../async-execution.md#prepared-owner-awaits-with-siblings-e1d2d3m).

Since 0.141, the retained native runtime checks child/result membership before
applying frame, loop and lexical-boundary retirement. Final-frame domain authority
is checked before releasing its storage. Parent execution with live children remains
blocked: generated callback evaluation still needs an earlier loan/invocation check.
This internal prerequisite does not enable source scheduling. See
[native retirement preflight](../async-execution.md#native-retirement-preflight-e1d2d3n).

Since 0.142, every retained native child/result records its original parent
invocation, which must remain live and be the selected caller for normal joining.
Lower-level Machine selection now rejects live children before callbacks or pending
delivery change operands, paths, effects or fuel. This conservative check does not
enable parent progress or source scheduling. See
[native invocation ownership](../async-execution.md#native-invocation-ownership-and-atomic-selection-e1d2d3o).

Since 0.143, generated native callbacks expose immutable symbolic access plans:
operand order, field paths, indirection, borrow modes and storage retirement. Normal
cold-await delivery has a separate result-write plan. Transitive operations and
administrative states remain opaque or unknown. Querying a plan has no source effects;
it does not grant permission to run beside a child. Parent selection remains guarded
until captured loans and actual storage dependencies can be checked. See
[native callback access plans](../async-execution.md#native-callback-access-plans-e1d2d3p).

Since 0.144, native access plans can resolve to live arena/frame/slot identities and
field paths without consuming source values. Indirect accesses include their reference
holder reads; normal result writes and caller constructor destinations stay distinct.
Resolved identities reject retired allocations and offset reuse, while opaque/unknown
work remains explicit. These current storage descriptions do not establish captured
loan permission or enable parent progress. See
[native storage resolution](../async-execution.md#native-access-storage-resolution-e1d2d3q).

Since 0.145, native contexts retain bounded descriptions of reachable captured
storage, including borrowed carrier contents and shared/exclusive attenuation.
Snapshots survive readiness, retained results and join-to-callback delivery. Unknown
metadata preserves existing execution and never grants access. Runtime values do not
encode every checked origin/holder guard, so these descriptions alone cannot enable
parent progress or scheduling. See
[retained capture descriptions](../async-execution.md#retained-native-capture-descriptions-e1d2d3r).

Since 0.146, checked reference expressions retain explicit partial origin evidence,
including carrier guards. Expansion remaps it and certificate validation rejects its
removal or alteration. This metadata does not change source execution or native access
permission. Native allocation binding and transport follow in 0.147; complete guarded
loan evidence remains ahead. See
[partial checked reference origins](../async-execution.md#partial-checked-reference-origins-e1d2d3s1).

Since 0.147, native references retain formation identities and partial checked origins
through reborrows, moves, selected sum/collection results and ordinary result delivery.
Capture snapshots retain these origins through admission, readiness, completion and
pending delivery. Only origin allocations are bound: symbolic fields are not read.
Unavailable descriptions never change source execution or become an empty dependency
set. This evidence remains incomplete and grants no access or parent progress. See
[bound native reference origins](../async-execution.md#bound-native-reference-origins-e1d2d3s2).

Since 0.148, reference evidence collects ancestor origins and can certify a bounded
set of direct, unguarded dependencies. Conditional aliases and guarded/contained/tagged
forms remain partial. Native completeness requires complete incoming evidence and is
lost when merged with partial dependencies. This coverage grants no execution permission;
callback interference and invocation/domain authority still gate parent progress. See
[direct reference completeness](../async-execution.md#conservative-direct-reference-completeness-e1d2d3s3).

Since 0.149, an immutable native query compares one retained child's complete dependencies
with its owner's current local callback. It distinguishes disjoint storage, conservative
overlap and inconclusive evidence, checking exact invocation/fault-domain and allocation
identity. Normal pending delivery uses only result writes. This per-child observation
grants no permission; both parent-selection and retirement gates remain unchanged. See
[interference queries](../async-execution.md#immutable-native-interference-queries-e1d2d3s4).

Since 0.150, an aggregate native query reconciles all registered borrower descendants
with fault-domain membership and includes queued await captures and joined pending
results. Missing/incomplete authority is inconclusive; complete results report coverage
counts and conservative interference. This remains an observation: both execution gates
and source ownership rules are unchanged. See
[borrower coverage](../async-execution.md#aggregate-native-borrower-coverage-e1d2d3s5).

Since 0.151, the native host has an explicit one-transition admission operation. It
requires a fresh complete disjoint borrower census and consumes private permission
before exactly one local callback or normal pending delivery. Rejection is administrative;
real execution faults retain existing settlement. Ordinary quota drivers retain their
child gates, and source execution is unchanged. See
[one-transition admission](../async-execution.md#one-transition-native-admission-e1d2d3s6).

Since 0.152, an explicit native host batch composes fresh one-transition admissions and
reports callback/administrative progress with quota, limit, rejection, result or fault
status. A later block preserves prior progress. Zero quota performs no transition;
normal pending delivery consumes no callback allowance. Each batch is capped at 4096
transitions. Source scheduling and ordinary drivers are unchanged. See
[bounded admission](../async-execution.md#bounded-native-admission-e1d2d3s7).

Since 0.153, fresh local reborrows with unconditional ancestry can carry complete holder
guards into native captures and results. Whole borrowed holder ancestors remain guarded;
disjoint local work still needs a fresh census and exact invocation/domain authority.
Copied shared fields gain no holder guard. Indirect guards, permission families,
conditional/tagged ancestry and contained proof gaps remain partial. See
[local holder guards](../async-execution.md#complete-local-holder-guards-e1d2d3s8).

Since 0.154, fresh direct borrows of local records can carry complete holder and
stored-reference coverage, including nested records and projected local holders.
Every reachable native reference still needs complete incoming formation evidence.
Aliases, indirect holders, tagged/conditional contents, returned permission families
and references to carriers remain partial. Source execution remains sequential. See
[local borrowed carriers](../async-execution.md#complete-fresh-local-carriers-e1d2d3s9).

Since 0.155, erased local carrier aliases can retain complete dependency coverage across
moves and projected reborrows. Every broad borrowed ancestor and all of its stored owners
remain included. This permits only conservatively disjoint host work under existing
invocation/domain checks. Incoming/received indirect carriers, permission families and
conditional/tagged/nested-reference gaps remain partial. See
[local carrier aliases](../async-execution.md#complete-local-carrier-aliases-e1d2d3s10).

Since 0.156, singleton ordinary record input leaves preserve validated incoming reference
evidence when copied into private access slots. This applies to owned/borrowed holders
and nested record fields with reference-free pointees. It supplies no new complete proof:
partial, missing or invalid metadata is never promoted. Unioned/tagged/nested-reference
and returned-value evidence remain conservative; indirect holder closure is still queued.
See [incoming evidence transport](../async-execution.md#incoming-record-leaf-evidence-transport-e1d2d3s11).

Since 0.157, supported singleton shared input leaves retain complete dependency coverage
through direct uses, local aliases and projections when incoming runtime evidence is
complete. Full incoming pointees and caller ancestry remain dependencies; copying a shared
field adds no holder guard. Exclusive/grouped/conditional/returned permissions and whole
indirect carrier closure remain conservative. See
[shared input leaves](../async-execution.md#complete-singleton-shared-input-leaves-e1d2d3s12).

Since 0.158, native runtime support can snapshot an actual incoming aggregate holder,
including stored owners outside a detached leaf's reachable graph. Formation identities,
liveness and bounded traversal are checked without changing source values. This internal
prerequisite does not relax compiler admission of exclusive input or indirect holder guards.
See [holder snapshots](../async-execution.md#bounded-native-holder-snapshots-e1d2d3s13).

Since 0.159, supported singleton exclusive input leaves can retain complete dependencies
through direct and local aliased reborrows. The proof explicitly relates the original input
leaf to its actual borrowed aggregate and retains every stored owner plus broad holder guards.
Runtime formation checks and partial evidence remain mandatory. Shared copies remain
holder-free; whole incoming carriers and unsupported permission/ancestry cases stay
conservative. See [checked input holders](../async-execution.md#checked-exclusive-input-holders-e1d2d3s14).

Since 0.160, supported whole incoming ordinary carriers can retain complete dependencies
through direct uses, projected reborrows and local aliases. The original full aggregate and
every singleton reference field remain covered, including owners outside the selected
projection. Native field/formation checks and complete caller evidence are required; partial
or changed evidence never becomes complete. Grouped, returned, conditional, tagged and
nested-reference proof gaps stay conservative. Source execution remains sequential. See
[incoming carriers](../async-execution.md#complete-incoming-carriers-e1d2d3s15).

Since 0.161, the explicit native host admission path can prepare a supported consuming cold
await beside retained children. It checks the exact local handle, producer, invocation and
queue/wait capacity before operand moves, retains fresh borrower checks, and stops at an
explicit handoff before executing the child. Normal result delivery remains separate. This
is internal scheduling support: source execution stays sequential, and ordinary drivers,
direct await/call/effect admission and retirement gates remain. See
[cold-await admission](../async-execution.md#checked-cold-await-admission-e1d2d3t).

Since 0.162, direct `await operation(arguments)` uses an explicit retained handoff in both
executors. Arguments evaluate once in written order; queued captures and returned borrows
retain their original storage and fault ownership. Task/wait identity allocation is atomic,
and result delivery does not reevaluate the call or consume additional source fuel.
Synchronous calls and `run` retain their call behavior. Source execution remains sequential;
started-task joins, spawn/yield and broader parent admission remain gated. See
[direct-await handoffs](../async-execution.md#direct-await-handoffs-e1d2d3u).

Since 0.163, internal native host APIs can retain unjoined started work without a wait or
producer execution. A consuming handle claims completion in any started phase; a separate
one-use join rechecks its original invocation, session and fault domain before adopting the
result. Unjoined captures and results remain retirement obligations, including during fault
settlement. Shared protocol preflight and reference delayed-completion tests cover the same
lifetime requirements. Source `spawn`/`yield` and scheduling remain unavailable. See
[retained started-task joins](../async-execution.md#retained-started-task-joins-e1d2d3v).

Since 0.164, an awaited invocation may own independent native children when it is not itself
waiting on an await. Captures and results retain their original invocation/domain obligations;
outer callers remain parked until every child is delivered. Terminal owning-boundary unwind
restores the original outer diagnostic chain before further settlement. This internal host
composition keeps existing callback/loan gates. Source programs still execute sequentially,
without `spawn`/`yield`. See [children of awaited invocations](../async-execution.md#children-of-awaited-invocations-e1d2d3w).

Since 0.165, the reference runtime also has private queue-only started-task admission and
consuming joins. Immutable spawning identity survives nested child boundaries and awaits;
joining delivers the result without replacing the original caller diagnostic chain. Unjoined
work remains an obligation through normal execution and retires after complete emergency
unwind. This is internal runtime support; source `spawn`/`yield` remain unavailable. See
[reference started-task joins](../async-execution.md#reference-started-task-joins-e1d2d2d).
