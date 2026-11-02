# Async execution: foundation and decisions

[Roadmap](language-revision.md#milestone-5--async-reference-semantics) ·
[Fault boundaries](faults-and-activations.md) ·
[Current feature status](reference/support-status.md)

Updated 2 November 2026. This is an E1 design and implementation plan. Compilers 0.98–0.103 add
shared activation identities, persistent continuations and atomic invocation admission,
prerequisites for overlapping execution contexts. They do **not** implement source
tasks, source suspension, scheduling or async I/O. Compiler 0.105 additionally implements [direct async calls](reference/async-execution.md):
`async control`, `run` and direct `await`, with sequential execution. Compiler 0.106
adds local cold tasks with owned captures and consuming await. Compiler 0.107
retains direct shared/exclusive captured loans through await. Compiler 0.108
transfers selected borrowed completion origins. Compiler 0.109 retains contained
references in owned captures. Compiler 0.110 retains whole borrowed carrier storage
and nested dependencies through cold capture and result delivery. Compiler 0.111
separates execution-wide storage and observations from resumable context state.
Scheduling remains future work.

## Existing direction

The roadmap already separates completed outcomes from pending computations:

- `capture` invokes immediately and stores an ordinary completed outcome.
- `defer` evaluates and transfers owned arguments now, without running the body;
  direct borrowed captures are retained in 0.107 and owned contained captures in 0.109.
- `await` consumes a local cold task and exposes only its declared normal outcomes.
  Waiting for an independently running task remains planned.
- Planned `spawn` explicitly schedules child work. A child belongs to an owning
  scope; every path settles or explicitly transfers ownership. There is no hidden
  detach or parallelism caused by an ordinary call.
- Suspension preserves captured loans. Waiting does not end an exclusive borrow.
  Owner storage must remain valid until all dependent tasks and results release it.
- Cancellation is a request followed by a checked completion protocol. It is not a
  source fault, hidden exception, forced frame destruction or guarantee of progress.

The first executor is cooperative and single-threaded, with deterministic modeled
readiness. Native coroutine lowering remains a separate E2 checkpoint. Thread
transfer, synchronization traits and general annotation flags are not introduced.

## What the current runtime does

Before 0.99, the reference interpreter invoked controls recursively through
`Machine::invoke` and traversed loop/boundary instruction lists through `execute`.
Local decisions, instruction positions and return handling lived in Rust stack
frames. Retaining only the values map was insufficient to pause execution.

In 0.99, [explicit continuations](../src/runtime/continuation.rs) hold invocation
returns, decision delivery, loop restarts, and named/lexical work and contingency
transitions. Caller facts and storage bases survive nested calls. Borrowed values
continue to address the same live storage. Normal return and fault unwinding retire
the same slots as before; completed loop iterations do not accumulate continuations.

The [host stepping API](reference/execution-failures.md#stepping-a-reference-execution)
can pause before a checked instruction, retain its execution state, and resume later.
A zero quota does nothing; terminal results are delivered once and cannot resume.
Quota is independent of fixed fuel, whose exhaustion remains terminal. Ordinary
`run` APIs drive the same engine to completion. This is a scripted reference
simulation mechanism, not source `await`, cancellation or an I/O readiness scheduler.

In 0.103, invocation admission has two internal steps. Preparation validates every
argument, including nested values, and owns that validated argument vector without
installing frame storage, consuming an activation ID or running the body. Starting
consumes that handoff, checks address capacity and activation admission, then installs
the whole frame. A malformed later argument, exhausted address space or already
failed diagnostic context cannot leave a partially installed invocation.

Ordinary synchronous calls use both steps immediately. This provides a concrete
handoff point for later deferred execution; it is not yet a source task. The handoff
has no scheduler, public task type, captured-loan checker or cancellation contract.
Host simulation disposal must not be treated as permission to drop a Tellur task.
The source entry form is `run`; E1c subsequently added checked cold captures and
consuming await around this internal admission handoff.

Reference values already use invocation-specific logical slot ranges. A reference
identifies storage and projections rather than a Rust vector element pointer.
The [native generator](../src/native.rs) uses dense activation-local offsets, but its
ordinary function calls and arena retirement still rely on sequential execution.
The reference continuations now persist between host calls. Native frames still need
a separate suspension and retirement protocol; they are not LLVM coroutine frames.

The [fault tracker](../src/fault.rs) records one active chain and bounded causes.
Child trackers isolate a named/lexical boundary's diagnostics; they are not tasks,
task scopes, or scheduling decisions. Before 0.98, `child()` copied the last ID and
`absorb()` merged the maximum after the child returned. Two children created before
either finished would allocate duplicate invocation IDs.

In 0.98, all related trackers share one execution-local monotonic allocator. They
retain separate active chains, source sites, failure flags and causes. Allocation
exhaustion fails without wrapping or partially entering a control. Separate entry
executions still start from ID 1; completed calls do not accumulate a history.
Existing sequential reference/native report IDs retain their order.

The counter orders identity allocation only. Its atomic implementation does not
make Tellur programs concurrent, publish their memory effects, prescribe scheduling,
introduce a task ID, or define spawn/await ancestry. A frozen failed tracker cannot
resume, even if another tracker in that identity family can still allocate IDs.
The executor decides which execution contexts may run.

## Shared execution state and independent contexts (E1d.1)

Compiler 0.111 separates the interpreter into one `SharedExecution` and a selected
`Context`. The execution owns the value arena, monotonic frame-address allocator,
fixed fuel, scripted input cursor and event history. A context owns its current
frame facts, call/loop/boundary continuations, pending control disposition, diagnostic
tracker, contingency stack and retained-resource handover. Moving or selecting a
context does not copy storage, reset addresses, replay input or refill fuel.

Internal tests interleave independent control chains against that same execution.
They keep the caller suspended while child references point into its storage;
children write the original slots and retire only their own frames. Other tests
separate decision facts and contingency state while sharing script order, effects
and fuel. Admission failures preserve a suspended owner's storage and do not
consume an activation identity.

This is a structural foundation, not source scheduling. The public execution
session still runs one sequential chain; its stepping quotas remain a host test
facility. Context selection in these tests is explicit harness work, not a new
source safe point or permission to resume siblings after an escaping fault. Task
identities, owner links, ready/wait transitions and settlement admission come next.
The native backend and LLVM coroutine route are unchanged.

## Separate the identities and continuations

| Entity | Purpose | Must not be confused with |
| --- | --- | --- |
| Definition/site | Authored control, delta and source provenance | One runtime invocation |
| Activation | One invocation with owned state and local decisions | A loop iteration or allocation address |
| Frame storage | Locations retaining values and loans | An observed diagnostic ID |
| Task handle | Owned pending computation and result obligation | A completed outcome or copyable scheduler token |
| Owning scope | Responsibility for settling children | Whichever control happens to await a handle |
| Fault boundary | Emergency settlement and permitted recovery target | An ordinary outcome consumer |
| Resume point | Explicit checked continuation after suspension | A retained Rust call stack |

A future suspended frame needs its current instruction/structured continuation,
local slots and selected facts, return-delivery target, live children, owner scope,
loan/retention requirements and fault domain. A stored outcome needs none of that
execution machinery. Task completion cannot be represented merely by stuffing a
fault into an outcome payload.

## Accepted decision: unhandled spawned-child faults

Accepted by the language author on 1 November 2026: a fault escaping a spawned
child's own boundaries faults its owning boundary at the next cooperative scheduler
safe point. Ordinary parent and sibling continuations stop; resource settlement must
finish before contingency or supervised recovery. Propagation does not wait for an
`await`. An inner boundary that already settles the fault remains isolated.

This preserves the rule that ordinary execution cannot continue inside a failed
boundary. The placement of `await` cannot redirect emergency control flow. Admission
checks must account for everything the boundary could abandon, including resources
currently held by siblings.

The fault cannot become `failed`, `cancelled`, a boolean, or another normal outcome.
Stopping siblings does not assert that their fallible cleanup succeeded. Fault
disposition and cooperative cancellation remain distinct. A request boundary can
send its emergency response only after child loans end and retained response
capabilities are safe to hand over. This is an accepted design rule, not an
implemented scheduler feature.

Invocation ownership and the initial restriction on handle movement are accepted
below. Cooperative scheduling points, deterministic observation of escaping faults
and secondary settlement failures still need an executable protocol before general
`spawn`. An atomic ID allocator makes no choice here.

## Accepted decision: invocation-owned spawned children

Accepted by the language author on 2 November 2026: each async invocation owns
its spawned children. Every normal path must await them. In the first bounded
profile, handles may move locally but cannot escape that invocation or cross a
fault boundary. No explicit `task scope { ... }` ownership block is required.

An escaping child fault still stops ordinary work in the owning fault boundary
and settles affected children and retained resources before contingency. Moving a
handle does not move its owner or redirect emergency propagation. This is the
accepted direction for E1d; compiler 0.110 still executes cold tasks sequentially
and does not yet accept `spawn`.

## Cold-task contract audit

This audit guided E1c: creating a computation is distinct from invoking it.
The local cold-task profile now implements capture and consuming await; public
task contracts and independent scheduling remain bounded as described below.

| Operation | Existing direction and required checking |
| --- | --- |
| `defer operation(arguments)` | Evaluate arguments in written order and transfer ownership now. Retain captured loans now. Callee body effects, decisions and faults have not happened; argument construction can still fault immediately. |
| Store or move a cold handle | Preserve the pending computation and all captured owners/loans. A handle is not Copy or a completed outcome. Its eventual result contract alone does not describe everything it owns. |
| `await pending` | Consume the handle once; drive cold work and retain caller/child frames until actual completion. Transfer declared normal payloads and their origins; keep faults outside that result. |
| Fault before the task starts | Check the live captured owners' fault dispositions and retire their borrowers before boundary handover. Discarding a computation cannot claim its external resource protocols completed. |
| Public or generic task types | Describe captured state as well as possible result payloads; `task<result>` alone must not erase loan, completion or fault-disposition requirements. Bound the first source profile if those contracts are unavailable. |
| Entry and suspension | Keep the planned `async control` permission and explicit runtime boundary for a synchronous entry. Host instruction quotas are not source suspension points. |

Ordinary borrowed rvalue calls currently allocate anonymous storage for one
invocation. Deferred arguments would need that storage to travel with the task
instead; extending temporary lifetime incidentally through an implementation pointer
is unsound. A named owner or an explicit checked task-owned backing allocation is
required before enabling that case. Existing grouped/contained origin contracts must
survive capture and delivery, not collapse to the return contract's outer type.

### Accepted decision: never-started tasks must be settled

Accepted by the language author on 1 November 2026: even a never-started deferred
task must be awaited or have its ownership transferred. Normal execution cannot
discard it with `drop(pending)`, even when all captured values are droppable. Task
ownership is not Copy or Clone; transferring the handle also transfers its pending
work, captures, loans and completion obligation. This initial rule keeps planned
work from being silently forgotten.

This does not permit cancellation by dropping running work or hidden detach. A fault
still terminates ordinary execution through the owning boundary's checked fault
dispositions; it does not satisfy normal completion by pretending the task ran.
The host `ExecutionSession` remains a scripted simulation with a different disposal
contract. The local cold-task profile enforces these source task rules.

### Accepted decision: explicit runtime entry with `run`

Accepted by the language author on 2 November 2026: synchronous code enters async
execution explicitly with `run`, for example `delta run startup(args) { ... }`.
It drives the async operation and its owned scope to settlement, then exposes normal
outcomes through the existing delta syntax. A lexical `runtime { ... }` block is
not part of the accepted entry form.

The runtime entry is not a fault boundary: faults retain their emergency channel
and propagate to the surrounding fault boundary. Scheduler scope settlement cannot
leave detached children or borrowed storage behind. This decision selects the source
form. Compiler 0.105 implements the direct sequential subset; owned child scopes
and scheduler settlement remain part of the later task implementation.

### E1c admission and representation work

The two accepted task policies do not make the current instruction quota a scheduler
or `capture` a cold task. The next implementation needs these distinct pieces:

- **Delivered in 0.105 (E1c.1):** async permission in source signatures, direct
  `run`/`await` calls, public snapshots and hover. Trait implementations match the
  permission; synchronous host entries require a `run` wrapper. Expansion retains
  async calls. Reference/native execution is sequential without stored tasks.
- **Delivered in 0.106 (E1c.2):** immutable local `auto` task bindings with owned
  captures, moves and consuming `await pending`. Captured resources retain fault
  obligations independently of completion payloads. Bodies remain cold until await;
  reference/native execution, expansion, auto hover and contingency cleanup are checked.
  Tasks cannot escape through outcomes; public task type contracts remain undecided.
- **Delivered in 0.107 (E1c.3a):** direct shared/exclusive reference captures of
  existing reference-free storage, persistent holders through moves, ordinary
  reborrowing versus explicit reference transfer, and release on consuming await.
  Native addresses and resource settlement agree with reference execution.
- **Delivered in 0.108 (E1c.3b):** per-parameter captured dependencies and selected
  borrowed result origins, including grouped records/sums, forwarding/adapters,
  reborrow ancestry and native/reference delivery.
- **Delivered in 0.109 (E1c.3c.1):** owning captures of reference-bearing records,
  sums and options, including nested carrier contents, tagged origins, selected
  result delivery, retained fault obligations and reference/native parity.
- **Delivered in 0.110 (E1c.3c.2):** whole borrowed reference-bearing carrier
  parameters retain direct storage authority and nested contents, including
  result origin groups, tagged leaves and storage guards. Shared outer authority
  cannot grant exclusive inner access. Body effects and body faults occur only when driven; argument
  construction and its faults remain immediate.
- Represent pending captures separately from the result contract. A task can hold
  a must-complete owner or borrow even if all eventual outcomes have empty payloads.
  Copy/clone/drop capability checks must not derive permission from that result alone.
- Consume a handle exactly once on await or transfer. On normal paths, scope exit
  cannot silently retire it. On fault paths, check captured owners' dispositions
  before unwinding and release borrowers before retained-resource handover.
- Retain stable storage for the suspended caller, the child and any task-owned
  argument backing storage. Bound unsupported public capture/origin shapes explicitly
  rather than erasing those requirements behind `task<result>`.
- Add a separate await-delivery continuation. Existing `ExecutionSession` pause and
  fuel behavior remains the reference observation mechanism, not source suspension.

The current checker paths are synchronous too. `Checker::arguments` retires new
anonymous stored-loan holders after argument evaluation, while decision lowering
computes return origins from the actual arguments and checks the callee's fault
permission at invocation. `stage_values` expires nonescaping initializer loans while
preserving pending values. A deferred-call implementation cannot simply reuse these
paths and save the emitted call: captured loans need a task-owned holder until actual
settlement, and a callee's future body fault must stay distinct from an immediate
argument-construction fault. The runtime handoff in E1b.1 does not establish any of
those static permissions or extend temporary lifetimes.

Acceptance must cover argument effects before body effects, no execution merely from
creating/moving a task, duplicate consumption and dropped-task rejection, a borrowed
capture protecting its owner until settlement, a fault during argument construction,
a fault before the task starts, and normal payload/origin delivery. The accepted
`run` entry must preserve the surrounding fault channel.

## Capture-transfer requirements

This audit derives implementation requirements from the accepted task rules. It does
not enable source syntax or settle a public capture/effect type spelling. The
synchronous-entry form is accepted separately as `run`. E1b.1's runtime argument vector is necessary
execution state; it is not the static proof that retaining those arguments is legal.

### State that must survive deferral

A checked pending computation needs more than its normal result alternatives:

| State | What must be retained | Why the result contract is insufficient |
| --- | --- | --- |
| Owned captures | Captured values and each live completion obligation on its selected histories | A control returning only `done` may still own a connection before it runs. |
| Direct borrowed captures | The target place, reference mode, origin set, parent permissions and carrier guards | A reference parameter may produce no borrowed result but still excludes conflicting access while pending. |
| Contained borrowed captures | The existing nested loan tree, active sum alternatives, public origin groups and storage dependencies | Flattening an aggregate to its outer type loses distinctions between a carrier's storage and external referents. |
| Temporary backing storage | A proven live owner for every retained address | Keeping the argument vector alive does not keep an unrelated caller temporary alive. |
| Future invocation contract | Target definition, normal outcomes, result-origin mappings and body fault permission | Creating a task does not yet execute the body; a later await must recover its actual contract. |

Moving a task transfers this state and its normal completion obligation together.
It neither completes the computation nor releases captured loans. The must-settle
rule applies even when all captured data and eventual payloads are ordinary
droppable values. Copy/clone/drop checks must not infer task capabilities from those
values or from a nullary result alone.

### Current checker seams

The existing machinery provides several parts of the proof, with synchronous
lifetime boundaries that must remain explicit:

- [Initializer staging](../src/check/construction.rs) checks earlier arguments against
  the next initializer and retains evaluated pending values. Later initializer faults
  must still see earlier uncompleted owners. Its loan expiry is scoped to anonymous
  loans created during that initializer list.
- [Argument checking](../src/check.rs) validates aliases, reference mode, transfers
  and types. `reference_argument` resolves and checks a direct place; its use at a
  synchronous call is not itself a persistent task-owned loan holder.
  `arguments` retires newly stashed stored-loan holders after evaluation.
- [Stored-reference handling](../src/check/stored.rs) preserves nested loan state and
  consumes exclusive permissions on transfer. A task capture must adopt the retained
  state rather than retire it with the synchronous call temporaries.
- [Call origin mapping](../src/check/public.rs) resolves returned dependencies through
  concrete arguments, origin groups, carrier storage and nested sum alternatives.
  Await needs the corresponding captured dependencies, even if the original source
  argument binding has moved or is no longer a name in scope.
- [Resource capability checking](../src/manifest.rs) derives existing aggregate fault
  dispositions from their contents. A future task's disposition must account for its
  pending captures, not only the callee's eventual output type or body permission.

The existing [initializer resource tests](../tests/constructors.rs),
[borrowed composition tests](../tests/borrowed_composition.rs) and
[borrowed constructor tests](../tests/borrowed_constructors.rs) are reusable baseline
evidence for these synchronous rules. They are not evidence of task support.

### Transfer and settlement cases

The following are acceptance requirements for the future source implementation,
covered for local cold tasks through E1c.3c.2; cancellation is not yet executable:

| Situation | Required observation |
| --- | --- |
| A later deferred argument faults | Earlier evaluated arguments still have their ownership and fault-disposition requirements; no half-created task is published. |
| A resource moves into a task whose result is only `done` | The resource remains a pending obligation. An intervening caller fault checks its disposition even if the future body would normally complete it. |
| A shared borrow is captured, then the caller attempts to mutate or move its owner | The conflicting access remains rejected while the task, its result or any other live holder retains that dependency. |
| An exclusive reference is moved into a task | The old reference cannot be reused, and its parent permissions remain restricted while the task owns the child permission. |
| Await returns a borrowed result on one alternative only | The selected result retains the declared dependency; a different alternative without it must not acquire a spurious result loan. |
| A task is moved between local bindings | Ownership and capture dependencies move together; the old binding is unavailable and the destination still must be awaited or transferred. |
| Control leaves a scope normally with a pending task | Reject the unfinished task even when its body would have no effects or its result payload is empty. |
| A cancellation request is issued in the later cancellation profile | No loan ends merely from requesting cancellation; actual checked settlement remains necessary. |

Borrowing a temporary across deferral needs an explicit backing-storage proof. The
first bounded implementation must reject cases for which it cannot establish that
proof; it must not silently inherit a synchronous call's temporary lifetime. If an
awaited result retains a borrow into such storage, retiring the task cannot retire
that storage while the result is live.

Likewise, returning a task, storing it in an aggregate or passing it through a generic
interface must preserve its hidden capture requirements. A result-only `task<r>`
view cannot silently erase them. Keeping concrete capture descriptors for a bounded
local implementation is a candidate implementation strategy; it does not settle
public task type syntax, erasure rules or compatibility between different captures.
Those boundaries need explicit contracts before being accepted by the checker.

## Implementation sequence

| Checkpoint | Deliverable and evidence | Dependencies |
| --- | --- | --- |
| E1a | Runtime audit and shared activation identity allocator; interleaved children, independent failure state, no reuse after return, overflow, and sequential reference/native report parity | Delivered in 0.98; shared identity is independent of scheduling policy |
| E1b | Persistent reference continuations and explicit step/resume protocol; current synchronous outcomes, fuel, effects, faults, loans and expansion observations preserved under different step partitions | Delivered in 0.99; no source async claim |
| E1b.1 | Separate argument preparation from frame installation; reject malformed later values, address exhaustion and frozen activation state without partial storage; preserve all synchronous observations | Delivered in 0.103; internal handoff only, no source task admission |
| E1c | Bounded cold tasks and consuming await; eager argument transfer, no body effects before drive, fixed result contracts, task ownership and retained borrows | Await/transfer obligation and explicit `run` entry accepted; direct invocation delivered in E1c.1, owned captures/handles delivered in E1c.2 and direct borrowed captures in E1c.3a and borrowed results in E1c.3b; owned contained captures are delivered in E1c.3c.1 and whole borrowed carriers in E1c.3c.2; scoped scheduling remains ahead |
| E1c.1 | Direct async permission and explicit entry | Delivered in 0.105: sequential `async control`, `run`, direct `await`, public/trait contracts and preserved graph boundaries; no stored tasks or scheduler |
| E1c.2 | Local cold tasks with owned captures, consuming handles and separate capture fault dispositions | Delivered in 0.106; no borrowed captures or public task type erasure |
| E1c.3 | Persistent borrowed captures and result-origin transfer | Direct capture holders delivered in E1c.3a and result delivery in E1c.3b; owned contained captures are delivered in E1c.3c.1 and whole borrowed carriers in E1c.3c.2; scoped scheduling remains ahead |
| E1c.3a | Direct borrowed captures of existing reference-free storage; moves, explicit transfer, faults and native addresses | Delivered in 0.107; no nested borrowed task storage or borrowed results |
| E1c.3b | Captured dependency mapping for borrowed completions | Delivered in 0.108: per-parameter capture origins and selected result holders, including records/sums and adapters |
| E1c.3c | Contained capture shapes | Owned inputs delivered in E1c.3c.1 and whole borrowed carriers in E1c.3c.2 |
| E1c.3c.1 | Owned record/sum captures containing references | Delivered in 0.109: field/tag identities, carrier contents and origin-group delivery |
| E1c.3c.2 | Whole borrowed carrier parameters | Delivered in 0.110: retained storage authority, nested contents, reborrows, moves and selected result origins |
| E1d | Scoped spawn and deterministic readiness; two overlapping children, duplicate/stale wake handling and owner settlement | Invocation ownership and no cross-invocation/boundary handle transfer accepted; implement scoped scheduling, sibling settlement and deterministic fault observation |
| E1d.1 | Separate execution-wide state from resumable control chains | Delivered in 0.111: one storage arena/address allocator, script cursor, effect history and fuel budget; independent frames, continuations and emergency state; internal interleaving evidence, no source spawn |
| E1d.2 | Task ownership and readiness lifecycle | Next: owner/task identities, ready/wait/complete transitions, duplicate/stale wake rejection and settlement admission before source scheduling |
| E1e | Explicit cooperative cancellation and suspending cleanup; normal completion races, cleanup failures and no abandoned losers | Define cancellation observation and cleanup admission; no implicit cancellation exception |
| E2 | Pinned native coroutine experiment, then checked lowering and schedule-based differential validation | Reference semantics and legal frame retirement must precede native destruction |

These are completion boundaries, not a promise that every row can be implemented
without another design decision. Keep syntax proposals out of the runnable playground
until they work. When a row changes source behavior, update the grammar, reference,
editor, executable examples and rejection tests together.

## Reference execution invariants

A step partition must not itself become a source effect. The same selected schedule
must retain effect order, outcome payloads and fault dispositions. Fuel exhaustion
remains a host limit, not a task result or cancellation request.

Only the currently executing context may alter its ordinary local state. A pending
readiness registration owns an explicit continuation, and a wake cannot resume a
completed/retired frame or consume a result twice. Readiness and task completion are
not automatically identical: a resumed control may suspend again or complete with
one of its declared alternatives.

No owner frame retires while a child or escaped result still holds its storage.
Fault handover waits until all affected borrowers have ended. Cancellation requests
do not release loans; actual checked settlement does. Retaining a frame does not
by itself grant permission to access a suspended exclusive borrower’s source.

The first complete async acceptance specimen still needs two overlapping operations,
a borrow held across suspension, cancellation racing normal completion, a visible
cleanup failure, and an escaping child fault. Compare normal and emergency histories
separately. A timer that prints in the expected order is insufficient evidence.

## Native coroutine route

LLVM provides coroutine suspension, resumption and destruction mechanisms and warns
that compatibility across releases is not guaranteed. E2 will pin a toolchain rather
than treating the host Rust compiler's LLVM backend as direct coroutine lowering.
[LLVM coroutine documentation](https://llvm.org/docs/Coroutines.html).

Tellur's design responsibility is to establish when a frame is quiescent and its
children, references and resource obligations are settled before native destruction.
This is a language/runtime requirement, not a cleanup guarantee supplied by an
intrinsic. Keep normal result delivery, cancellation and source-fault settlement as
separate transitions in the checked representation and comparison evidence.
