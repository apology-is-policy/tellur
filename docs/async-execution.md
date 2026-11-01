# Async execution: foundation and decisions

[Roadmap](language-revision.md#milestone-5--async-reference-semantics) ·
[Fault boundaries](faults-and-activations.md) ·
[Current feature status](reference/support-status.md)

1 November 2026. This is an E1 design and implementation plan. Compilers 0.98–0.99 add
shared activation identity allocation and persistent reference continuations,
prerequisites for overlapping execution contexts. They do **not** implement source
tasks, source suspension, scheduling or async I/O. Proposed execution rules below
are not new executable syntax.

## Existing direction

The roadmap already separates completed outcomes from pending computations:

- `capture` invokes immediately and stores an ordinary completed outcome.
- Planned `defer` evaluates and transfers arguments now, without running the body.
- Planned `await` consumes a task handle, drives a cold task or waits for an already
  running task, and exposes only its declared normal outcomes.
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

The owner-scope relationship, moving handles across fault boundaries, deterministic
selection of multiple escaping faults, and secondary settlement failures must be
specified before general `spawn`. An atomic ID allocator makes no choice here.

## Cold-task contract audit

The next E1c slice must distinguish creating a computation from invoking it. This
is an implementation audit, not newly accepted syntax or executable behavior.

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
contract. These source task rules are accepted for E1c but not yet executable.

### Pending entry-syntax decision

The accepted direction requires an explicit runtime boundary when synchronous code
starts async work. The remaining question is its source form:

| Choice | Source shape and meaning |
| --- | --- |
| Recommended: explicit `run` operation | `delta run startup(args) { ... }` drives the async operation and its owned scope to settlement, then exposes normal outcomes through the existing delta syntax. |
| Alternative: lexical runtime block | `runtime { ... }` allows `await` inside a synchronous caller's explicit runtime scope. |

This question is open with the author. Neither spelling is implemented or accepted
here. In either form, the runtime boundary is not a fault boundary: faults keep their
emergency channel and propagate to the surrounding fault boundary. Scheduler scope
settlement cannot leave detached children or borrowed storage behind.

### E1c admission and representation work

The two accepted task policies do not make the current instruction quota a scheduler
or `capture` a cold task. The next implementation needs these distinct pieces:

- Preserve a control's async permission in source signatures, checked calls and
  public contracts. Ordinary calls must not acquire an implicit suspension effect.
- Evaluate deferred arguments in their written order, retaining their owners and
  loans in task state. Body effects and body faults occur only when driven; argument
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

Acceptance must cover argument effects before body effects, no execution merely from
creating/moving a task, duplicate consumption and dropped-task rejection, a borrowed
capture protecting its owner until settlement, a fault during argument construction,
a fault before the task starts, and normal payload/origin delivery. Source entry
spelling remains pending; implementation must not choose it by timeout.

## Implementation sequence

| Checkpoint | Deliverable and evidence | Dependencies |
| --- | --- | --- |
| E1a | Runtime audit and shared activation identity allocator; interleaved children, independent failure state, no reuse after return, overflow, and sequential reference/native report parity | Delivered in 0.98; shared identity is independent of scheduling policy |
| E1b | Persistent reference continuations and explicit step/resume protocol; current synchronous outcomes, fuel, effects, faults, loans and expansion observations preserved under different step partitions | Delivered in 0.99; no source async claim |
| E1c | Bounded cold tasks and consuming await; eager argument transfer, no body effects before drive, fixed result contracts, task ownership and retained borrows | Await/transfer obligation accepted; settle source runtime entry/suspension effect contracts before enabling syntax |
| E1d | Scoped spawn and deterministic readiness; two overlapping children, duplicate/stale wake handling and owner settlement | Prompt owner-boundary fault propagation accepted; specify ownership transfers, sibling dispositions and deterministic multiple-fault selection |
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
