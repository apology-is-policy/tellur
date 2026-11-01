# Async execution: foundation and decisions

[Roadmap](language-revision.md#milestone-5--async-reference-semantics) ·
[Fault boundaries](faults-and-activations.md) ·
[Current feature status](reference/support-status.md)

1 November 2026. This is an E1 design and implementation plan. Compiler 0.98 adds
shared activation identity allocation, a prerequisite for overlapping execution
contexts. It does **not** implement source tasks, suspension, scheduling or async
I/O. Proposed execution rules below are not new executable syntax.

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

The [reference interpreter](../src/runtime.rs) invokes controls recursively through
`Machine::invoke` and recursively traverses loop/boundary instruction lists through
`execute`. Local decision facts, position in an instruction list and return handling
currently live in Rust stack frames. A language suspension therefore cannot be
implemented just by retaining the current values map.

Reference values already use invocation-specific logical slot ranges. A reference
identifies storage and projections rather than a Rust vector element pointer.
The [native generator](../src/native.rs) uses dense activation-local offsets, but its
ordinary function calls and arena retirement still rely on sequential execution.
Both mechanisms are useful foundations; neither is a persistent coroutine frame.

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

## Pending decision: unhandled spawned-child faults

A question is open with the language author. **Neither option is implemented or
accepted by this document.** The decision concerns a fault escaping a child's own
boundaries; an inner boundary that already settles the fault remains isolated.

| Choice | Observable consequence |
| --- | --- |
| Recommended: fault the owning boundary promptly | At the next cooperative scheduler safe point, its ordinary parent/sibling continuations stop. Resource settlement must finish before contingency or supervised recovery. The fault does not wait for an `await`. |
| Alternative: propagate when awaited | The child's fault stays in a separate pending fault channel until observation; ordinary owner work can continue meanwhile. Unobserved failure at scope exit and the boundary that owns eventual propagation need further rules. |

The recommendation fits the existing rule that ordinary execution does not continue
inside a failed boundary. It also avoids making the placement of `await` a way to
redirect emergency control flow. It requires admission checks for everything the
boundary could abandon, including resources currently held by siblings.

Neither choice permits a fault to become `failed`, `cancelled`, a boolean, or any
other normal outcome. Nor does stopping siblings assert that their fallible cleanup
succeeded. Fault disposition and cooperative cancellation must remain distinct.
A request boundary can send its emergency response only after child loans end and
retained response capabilities are safe to hand over.

The owner-scope relationship, moving handles across fault boundaries, deterministic
selection of multiple escaping faults, and secondary settlement failures must be
specified before general `spawn`. An atomic ID allocator makes no choice here.

## Implementation sequence

| Checkpoint | Deliverable and evidence | Dependencies |
| --- | --- | --- |
| E1a | Runtime audit and shared activation identity allocator; interleaved children, independent failure state, no reuse after return, overflow, and sequential reference/native report parity | Independent of the pending child-fault choice; delivered in 0.98 |
| E1b | Persistent reference continuations and explicit step/resume protocol; preserve current synchronous outcomes, fuel, effects, faults, loans and expansion observations under different step partitions | Does not require selecting a child-fault policy; no source async claim from this refactor alone |
| E1c | Bounded cold tasks and consuming await; eager argument transfer, no body effects before drive, fixed result contracts, task ownership and retained borrows | Settle source runtime entry/suspension effect contracts and disposal rules before enabling syntax |
| E1d | Scoped spawn and deterministic readiness; two overlapping children, duplicate/stale wake handling and owner settlement | Resolve the pending fault propagation/ownership decision and keep sibling dispositions explicit |
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
