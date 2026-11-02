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
identities and invocation obligations are integrated in E1d.2a below; ready/wait
transitions and scheduler settlement admission remain next.
The native backend and LLVM coroutine route are unchanged.

## Invocation and cold-task identities (E1d.2a)

Compiler 0.112 adds a private registry to shared reference execution. Each admitted
call frame has an invocation owner ID; each deferred value has a distinct task ID.
Neither is a slot address or diagnostic activation ID. Captured values remain in
the handle, preserving existing reference storage and resource representation.
Admission checks owner-ID capacity before entering the diagnostic activation and
installing frame storage. IDs never wrap or reuse retired identities.

The registry checks cold → running → completed → consumed transitions. Await
validates the original owner and producer before entering the body. A normal
completion remains an obligation until delivered to the caller's decision; it
cannot be consumed twice. Normal invocation retirement rejects unfinished task
obligations. Emergency retirement removes obligations without running cold bodies
or constructing normal fault outcomes. Work/handler retirement and supervised loop
recovery retire task records along with their discarded values, before contingency.

Private tests exercise moves across existing normal lexical boundaries, nested
borrowed awaits, cold and running tasks through repeated fault recovery, and failed
await admission. Independent harness contexts also preserve one owner’s cold task
while another faults. All finish with no retained task records or frame slots. Registry
tests also cover one-shot completion, wrong-owner/producer rejection and identity
exhaustion. These checks supplement the existing static ownership checks.

This is integrated bookkeeping for the sequential interpreter, not independent
progress or source `spawn`. The native executor keeps its current checked inline
captures. No public task-type syntax or change to cold-task boundary permission is
introduced. E1d.2b below adds queued dispatch and completion waits; external readiness,
fault-domain settlement and source concurrency still require their own checkpoints.

## Queued cold dispatch and completion waits (E1d.2b)

Compiler 0.113 integrates a ready queue and distinct completion-wait IDs into cold
await execution. Arguments are validated before ready admission; the registry then
checks the original owner and producer, allocates a non-reused wait token and queues
the task. A dispatch selects the next ready task exactly once. Failed admission or
wait-ID exhaustion leaves the registry unchanged; retiring queued work removes it
from dispatch as well as the owner's obligations.

The executor parks the caller's entire control state and retains a prepared child
invocation. The child runs against the same value arena, fuel and observation history.
Its normal completion validates the wait token before restoring the caller; result
consumption remains a separate obligation. Early, stale, wrong-task or duplicate
completion signals cannot resume the caller. An escaping fault retires the wait and
unwinds the parked caller through the existing emergency channel, without delivering
a normal completion. All source behavior remains sequential.

The diagnostic tracker moves into the selected child and back to its caller; it is
not cloned or reset. Nested async call traces, activation identities, causes and
borrowed payloads retain their prior meaning. Dispatch and restoration are
administrative steps, preserving instruction fuel and host stepping quotas. Tests
check the parked frame's original storage, early/duplicate delivery, repeated fault
recovery, every small fuel budget under different quota partitions, and exact native
fault reports before and after expansion.

This queue currently dispatches the sole child demanded by a consuming cold await.
It does not yet select independently progressing siblings or wait on host I/O. Queue
ordering tests exercise the internal registry; they do not establish a source FIFO
fairness guarantee. E1d.2c covers external readiness and fault-domain admission before
source `spawn`. Cancellation and native coroutine lowering remain separate work.

## Addressable suspended callers (E1d.2c.1)

Compiler 0.117 stores parked callers in a separate suspension table, rather than
nesting a boxed caller inside the active child's continuation. Each parking
registration gets a fresh identity, paired with the exact child task and completion
wait. A context that parks again receives a new registration; these are not public
task identities, diagnostic activation IDs or external readiness subscriptions.

Admission checks suspension identity capacity before changing the ready queue or
moving the active context. Completion validates all three identities before taking
the original caller exactly once. A crossed task/wait pair, stale registration or
exhausted identity space cannot remove another parked caller. Normal delivery and
emergency unwind both remove their registration; a fault still travels exclusively
through the emergency channel. Suspended frames and their borrowers remain in the
same shared value arena, and the diagnostic tracker still moves with the sequential
chain. No state is cloned, no script input is replayed and no fuel is added.

Checked execution tests retain two independent borrowed caller chains simultaneously,
reject crossed and stale registrations, and retire each without touching the other.
Nested awaits and repeated fault recovery verify bounded live suspension storage
and removal before contingency. Host quota/fuel partitions, returned borrows and
native/reference observations keep their existing meaning. These independent chains
are an internal test harness, not source `spawn` or a public scheduling API.

The next E1d.2c slice connects independent ready/wait contexts to modeled external
readiness and distinct fault domains. It must follow the accepted `await`/`yield`
handoff policy; quota pauses alone cannot select another source task. This table is
preparation for that work, not completion of the concurrent executor.

## Independent ready-context admission (E1d.2c.2)

Compiler 0.118 retains prepared child invocations and their control contexts in
separate ready storage, keyed by task identity. A consuming await parks its caller,
admits the prepared child, and requests an administrative dispatch. Dispatch checks
the task queue and matching retained context before moving anything. It cannot
replace an active control chain. Child frame installation still happens only after
selection; admission failure then unwinds through the child's paired suspended caller.

The current sequential driver selects only at an explicit internal dispatch request.
It never chooses a different ready task because a host instruction quota expired.
Dispatch consumes no extra source fuel, and moving a ready context does not clone
captures, replay inputs or change borrowed addresses. The existing source profile
still awaits one cold child at a time, with unchanged native execution.

An internal harness queues two independently checked borrowed callers before dispatch,
checks missing-context admission without partial queue changes, rejects replacement
of an active chain, and completes each caller without touching the other's storage.
Ordinary host stepping still drains administrative dispatch and cannot expose this
harness split. Internal queue order is not a public fairness guarantee.

External readiness registration, repeated waiting after execution has started, and
fault-domain admission/settlement remain the next E1d.2c slice. Until those protocols
are complete, this storage separation does not enable source `spawn`, `yield`, async
host adapters or independently progressing siblings.

## Executable readiness protocol model (E1d.2c.3a)

This initial test-only checkpoint was delivered on 0.118. The same scenarios now
exercise [runtime waiting and resumption](#runtime-waiting-and-resumption-e1d2c3b)
in 0.119; the description below records the original model and its bounds.

The [readiness model](../src/runtime/readiness_model.rs) runs real checked borrowed
calls through explicit test-harness suspension cuts. It is compiled only for tests;
compiler 0.118 and its installed tools still expose no external readiness API or
source scheduler. This model fixes the transition obligations before those are
integrated with runtime waiting states and fault domains.

A parking registration owns one original control context. Its token pairs a dedicated
execution-driver identity with a monotonically allocated generation, separate from
the task's one-use completion wait. A wake moves that registration from waiting to
eligible exactly once; it neither executes the context nor completes the task.
Unknown, repeated and stale signals leave other registrations unchanged. Parking
the same context again uses a new generation, so a late signal for its previous
wait cannot wake it. A token from another driver is rejected even when its generation
matches a live local wait. Late tokens retain the dedicated driver identity, preventing
identity reuse after that driver retires; it is not a frame or diagnostic identity.
Generation exhaustion is checked before moving an active frame or changing its loans.

Selection is separate from signalling. A signal may make another context eligible
while the current one runs, but selection cannot replace that active chain. Host
quota pauses do not themselves signal or select work. The explicit harness cuts
model future I/O suspension boundaries; they do not grant arbitrary source code new
handoff points. The accepted source policy remains `await`/`yield` only.

The regression scenario retains two independent exclusive borrows across repeated
waiting, changes wake order, and consumes one shared scripted-input history. It
checks source identities through emitted owner values, rejects late/duplicate wakes,
keeps fixed fuel and parked caller state, and delivers each borrowed result only
when its checked child actually completes. Both chains must finish with no values,
tasks, ready contexts or suspended callers left behind. Separate cases check generation
exhaustion and cross-execution signals, including a late token after its driver retires.

These are independent harness chains, not spawned siblings in one fault domain.
The model does not settle a failed domain or define cancellation. Runtime integration
must add an explicit waiting phase, resumed-ready entries and execution-scoped tokens,
then stop normal dispatch and settle every affected borrower before contingency.
No source `spawn` or async host adapter is enabled by this model, and unchanged
production binaries do not require a new release.

## Runtime waiting and resumption (E1d.2c.3b)

Compiler 0.119 integrates the readiness protocol into the reference task registry
and context storage. Tasks can now transition from running to waiting, from waiting
to ready on one valid signal, then back to running on explicit dispatch. Prepared
starts and resumed contexts use distinct ready-entry forms. Resumption moves the
original frame, continuation, facts and diagnostic state back into execution; it
never installs another invocation or replays captured arguments.

Each active child chain records its task and invocation owner separately from its
current call frame. This association survives nested synchronous calls and nested
awaits. Waiting preflights the task state and generation capacity before moving the
context or changing storage. Signals validate the execution identity, generation,
task state and retained context before making anything ready. Readiness cannot
complete a waiting task, discharge its owner's obligation or deliver a borrowed
result. Late tokens retain the dedicated execution identity after its runtime retires.

The original model regressions now exercise these runtime transitions. Added cases
check missing context storage, stale admission, wrong owners, identity exhaustion,
metadata retirement, and a fault after resumption through a nested synchronous call.
Independent test chains retain their own storage when another chain completes or
faults. This is not permission for siblings in a failed owning domain to continue.

The safe-point and signal methods remain private and have no source or adapter
callers. Harness cuts invoke them explicitly; public host quota pauses still only
pause the current sequential chain. Fault domains must stop admission and dispatch,
retire every affected waiting/ready borrower and settle resources before contingency
before these mechanics can support source concurrency or async adapters. Cancellation
and LLVM coroutine lowering remain later work.

## Fault domain identity and admission (E1d.2c.4a)

Compiler 0.120 gives each executing root, named supervised boundary and lexical
fault boundary a fresh runtime domain. Domain identities are execution-local,
monotonic and separate from invocation owners, task handles, readiness generations
and diagnostic activations. Only live domains are retained. Admission preflights
identity capacity before installing a root or replacing a lexical boundary's tracker.
A handled child closes without poisoning its parent; escalation stops each enclosing
domain as emergency unwinding reaches it.

A context carries its current executing domain through calls, cold awaits, waiting
and ready storage. Cold-task creation does not freeze the later executing boundary:
an existing handle may still move across normal lexical boundaries and inherit the
boundary where it is awaited. Future spawned-task ownership must separately retain
its accepted owning boundary; the current domain is not a substitute for that rule.

Normal task creation/await admission, readiness registration/signalling and ready
dispatch require an open domain and open ancestors. Emergency unwinding marks its
domain stopped before further continuations run. Contingent calls remain permitted
inside the failed domain through the existing checked contingency mode. Boundary
finalization and contingency admission require no nested live domains or retained
ready, waiting or suspended caller contexts in that domain. These checks cannot
release a loan or count metadata removal as resource settlement.

Two registry tests cover parent/child independence, stopped ancestors, stale scopes,
non-reused identities and exhaustion. Checked execution tests cover nested named and
lexical handling, root/nested admission exhaustion, and blocked wake/dispatch with
unchanged borrowed storage. The latter explicitly selects the stopped chain for
emergency unwind in the harness; it is not automatic sibling draining. Repeated
supervised recovery also checks bounded live-domain storage.

E1d.2c.4b must retain the original boundary continuation while draining all affected
children, invalidate their registrations and settle borrowers before owner storage
or contingency is released. It must also propagate an escaping child's fault to its
owning boundary at the accepted safe point and retain causal diagnostics. This
checkpoint supplies identities and admission guards, not that settlement driver.
Source `spawn`/`yield`, async adapters, cancellation and LLVM lowering remain gated.

## Reference sibling retirement (E1d.2c.4b.1)

Compiler 0.121 retains the original failing context, diagnostic tracker and boundary
continuation while it selects ready/waiting descendants for emergency retirement.
An invocation with started children cannot release its frame; a boundary cannot
enter contingency or close while nested domains or retained contexts remain. The
reference driver follows each selected child's original unwind and await chain,
retiring its task and wait registrations before returning to the parked emergency.
Normal return still requires consuming children; it does not acquire an implicit join.

A prepared invocation releases its captures without installing or executing its
body. A waiting or resumed-ready invocation unwinds without executing more normal
instructions. Nested named/lexical boundaries close through emergency retirement;
they do not enter contingency or hand retained resources to an abandoned handler.
The only handler selected by this cleanup is the original owning boundary's, after
all affected chains have retired. Unrelated domains and their readiness remain live.
The original causal report, shared fuel and source effect history are preserved.

Checked-execution harnesses exercise two simultaneous borrowed children in prepared,
waiting and resumed-ready states with small/large host quotas and an unrelated
waiting root. They compare the original fault report and exact remaining fuel,
reject late signals, and verify complete retirement of contexts, owners and domains.
Nested named-plus-lexical and lexical-only cases prove their handlers cannot emit,
escalate or terminate supervision during sibling cleanup. Existing recovery tests also
check that no settlement contexts survive normal handling.

This is private reference machinery: source `spawn`/`yield` and asynchronous adapter
admission remain disabled. The scripted executor's abrupt value retirement is
infallible. Its settlement restoration asserts that cleanup has fully unwound and
preserved the original failure; this is not a policy for fallible or suspending host
cleanup. Actual retained host-resource settlement evidence and owning-boundary
fault delivery at source safe points remain the next integration gates.

## Recovery and host-resource audit (E1d.2c.4b.2a)

The test-only follow-up on compiler 0.121 runs 24 successive supervised recoveries
in both ordinary and expanded checked execution. Each request admits two borrowed
children, alternating prepared, waiting and resumed-ready states. A separate waiting
root survives the whole run. Every contingency observes retired children, bounded
live frame/domain/task storage and an unchanged unrelated wait. Old readiness tokens
are rejected again during later requests; the one script cursor advances once per
request and the supervisor eventually returns normally. These are harness admissions,
not source `spawn` or host-quota scheduling.

A separate native-host regression exercises the real `Host::release_frame` and
`Resources` registry with Rust destructor counters across 24 requests. Releasing
borrowed task captures preserves their owners. Releasing owned task/record captures
invalidates only those resource tokens, once, before consuming the retained response
owner. An unrelated Rust object survives all requests, and final host teardown cannot
destroy an already-released object twice. The assertion requires child cleanup before
retained-owner use; it does not impose an order between unrelated child destructors.

The mechanisms and remaining integration requirements are distinct:

| Mechanism | Evidence now | Gate before concurrent native scopes |
| --- | --- | --- |
| Reference sibling unwind | Original boundary/fault retention, repeated recovery, stale waits, bounded live storage | Wire accepted source safe points and spawned owning-boundary identity without changing cold-task movement |
| Native explicit frame release | Real Rust owners, borrowed versus owned captures, stale handles and exact-once destruction | Connect release to actual scheduler borrower retirement; the explicit-release harness is not that scheduler |
| Native boundary cleanup | Existing generated sequential boundary/resource tests | Replace temporal resource snapshots with explicit boundary membership before unrelated scopes can interleave |

The last gate follows from the current native implementation: `Host::lexical` and
`Host::supervise` snapshot resource tokens at entry, then use that before-set plus
owned/retained tokens during cleanup. Under synchronous nesting, allocations made
while the boundary runs belong to that work. Interleaved independent work would break
that assumption: a later allocation can belong to another live boundary. Passing the
frame-release test does not make this temporal cleanup safe for concurrent scopes.

The next production slice should give resource registrations explicit live scope
membership, distinct from allocation order, diagnostic activations and invocation
frames. It must adopt owned inputs (including task/record captures) without adopting
borrowed inputs; retain emergency response owners until their handler; preserve normal
outgoing owners and cold handles; and release only the failed subtree after borrowers
settle. Boundary entry/exit, temporary adapter arguments, stale identities and normal
ownership transfer need regression coverage. Keep synchronous adapters and explicit
`fault: release` permissions; do not infer a fallible or suspending cleanup protocol.

No compiler/LSP/VSIX release is needed for this test-and-documentation checkpoint.
The 0.121 release evidence remains applicable; native source concurrency stays gated.

## Native resource scope membership (E1d.2c.4b.2b)

Compiler 0.122 replaces native boundary allocation snapshots with explicit live
membership in the Rust resource registry. A fresh scope records its parent and a
registry-specific identity. Admission validates the owned value graph and identity
capacity before committing any ownership change. Existing owned inputs, including
nested record and cold-task captures, move into the boundary's resource scope;
references never transfer the owners they point to. New Rust objects register in
the currently executing scope. Opaque scripted and builtin values without a retained
Rust registration do not acquire one through this mechanism.

Fault cleanup releases only the selected scope after its nested scopes have closed,
keeping the declared response resources for contingency. Named supervision now also
retires owners stranded in temporary adapter arguments before its handler starts.
Normal lexical completion promotes surviving owners to the parent so returned owned
results and cold handles remain live; handling and named termination retire their
remaining scope-owned resources. Closed scope metadata is removed. Resource scope
identities cannot be reused, selected after retirement or used in another registry.
An incomplete nested scope blocks parent retirement rather than silently destroying
its owners. These are backend identities, not task handles or diagnostic activations.

Native-host tests explicitly select an independent scope and allocate its Rust owner
*after* another boundary entered. Both lexical and named fault cleanup preserve that
owner, settle transferred and stranded inputs before contingency, and preserve retained
and borrowed parents. Further cases check normal owned-task delivery into a later
faulting boundary, exactly-once destructors, failure-atomic adoption, nested-scope
guards, foreign/stale scope selection and exhaustion. Generated native resource,
boundary, frame and reference-parity tests maintain the existing sequential behavior.

Internal scope selection does not add a scheduler, async adapters or a public host
handoff API. The next integration must bind these resource scopes to suspended native
execution contexts and settle all borrowers before selecting a scope for cleanup.
It must preserve the distinction between a task's current execution domain and its
spawned owning boundary. Source `spawn`/`yield`, fallible/suspending cleanup and LLVM
coroutine lowering remain ahead. Existing explicit resource fault permissions and
normal completion obligations are unchanged.

## Native activation frame ranges (E1d.2c.4b.3)

Compiler 0.123 gives native activation storage explicit arena-scoped identities and
live slot ranges. Generated calls allocate their own dense range and retire that
exact range on normal or error return. Fault resource release visits only that
frame's values, so retiring an earlier activation cannot release or truncate later
independent storage. Existing references and constructor destinations retain their
absolute slot offsets. Growing the backing vector never changes those offsets.

Retired ranges are cleared, reused and coalesced; a free suffix is removed. Empty
frames keep distinct identities even after their original offset is beyond the
remaining storage. Foreign, stale and exhausted frame admissions fail without
changing live values. A retired identity cannot act on a replacement at the same
offset. These internal frame handles are distinct from source references, task
handles, resource scopes and diagnostic activation identities.

Host regressions cover every retirement order of four differently sized frames,
repeated hole reuse, mutable projected borrows through backing-store growth, stable
owned buffers, zero-slot frames, failure-atomic admission and exact Rust resource
release in the presence of later frames and borrowed owners. Generated native tests
continue to check dense capacity, constructor destinations, borrowed outcomes,
boundary recovery and reference/expanded parity at each instruction-fuel boundary.

This removes the storage stack-order assumption; generated calls still execute
synchronously on the Rust call stack. Decision facts and resume continuations are
not yet heap-retained. The allocator does not decide when a loan ends: checked
execution must retire borrowers before owners and never resume stale references.
The next stage must retain executable continuations and select their diagnostic and
resource contexts together before source scheduling can be admitted. No source
`spawn`/`yield`, async adapters, cancellation or LLVM coroutine ABI is added here.

## Compiled native continuation model (E1d.2c.4b.4a)

An internal test-only model now emits compiled resume functions for ordinary
instructions, decisions, synchronous calls, `run`, direct `await`, cold-task await
and constructor delivery. Each activation retains its original frame identity,
decision facts, next instruction and one-use result-delivery continuation. A call
parks its caller and admits a separate child activation. Completion transfers its
normal payload or constructor destination before resuming the caller. This is
compiled Rust control flow, not a checked-graph interpreter or a retained Rust stack.

The model can return to its host between instructions while preserving the shared
value arena, script cursor, effects, diagnostic activation chain and fuel budget.
Zero instruction budgets do nothing. Completion and errors retire the entire model
chain; an already finished machine cannot be advanced again. Source faults remain
errors with their original activation trace and never enter normal result delivery.
The cuts are test observation points, not source task switches or `yield` semantics.

Differential tests compile original and expanded source at optimization levels 0
and 2. They compare normal results/effects and failure sites, identities, phases and
causes at every fuel cut under multiple quota partitions, including zero-sized
pauses. Source-fault reports match exactly. Only the existing native/reference
host-fuel message wording is normalized. The fixture combines a reference-bearing
constructor, selected decision facts, one scripted primitive, a moved cold handle,
an exclusive captured borrow, returned borrowed completion and a nested child fault.

This first slice rejected loops, lexical/named boundary continuations and administrative
recovery. Public native generation remains the synchronous 0.123 path;
no compiler/LSP/VSIX release or editor reload is required. A shared build-helper
extraction lets tests use the real locked native toolchain without a new public mode.
Resource settlement with suspended native boundaries remains unproved by this bounded
call model.

## Compiled native loop continuations (E1d.2c.4b.4b)

The test-only model now retains active loop targets and their body/continuation
positions within each activation. Loop entry and `again` stage argument values before
resetting the iteration's local slots and decision facts. A restart reuses the same
activation and frame; it does not admit a call or deliver an ordinary outcome.
Expanded helper completion unwinds the selected loop and resumes its compiled
continuation, preserving any enclosing loop. A skipped loop jumps over its whole body.

The loop fixture combines a selected helper loop, simultaneous movement of owned loop
parameters, repeated cold awaits capturing the same exclusive borrow, two scripted
inputs and a fault after restart. Original and expanded native programs at O0/O2 match
reference results and fault reports at every fuel cut under the same quota partitions
as the call model. Expansion explicitly targets the async caller as well as the entry,
and the test requires nested loop IR and an administrative completion. Source-level
lexically nested loops remain unsupported. The original call/constructor fixture also
continues to pass with the shared resume machinery.

Lexical/named boundary continuations and administrative recovery remain gated. The
public native backend, compiler/editor release and source handoff rules are unchanged.
Next, retain boundary work/handler continuations and select diagnostic/resource context
alongside them before admitting suspended-boundary settlement or source concurrency.

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

Accepted by the author on 2 November 2026: when a failed owning boundary stops a
sibling suspended inside a nested boundary, the sibling applies its declared resource
fault dispositions. Its nested contingency does not run. Only the owning boundary's
contingency handles the fault after settlement. Cleanup cannot recover into a stopped
sibling, transform the fault into an outcome, or take over process termination through
that sibling's handler. Fallible cleanup reporting remains a separate contract.

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
accepted direction for E1d; compiler 0.113 still executes cold tasks sequentially
and does not yet accept `spawn`.

## Accepted decision: source scheduling handoffs

Accepted by the language author on 2 November 2026: tasks switch at `await` and
an explicit `yield`; `spawn` only queues work. Synchronous stretches run without
interleaving sibling tasks. Long synchronous loops can therefore delay siblings;
programmers place a handoff where cooperative progress is required.

Host instruction quotas can pause the whole execution session, but do not authorize
a source task switch. An escaping child fault is observed at a scheduler safe point
and settles the owning boundary before contingency, as specified above. The next
E1d slice must implement this protocol before enabling general source `spawn`.
The current sequential cold-task executor has no source `yield` or sibling scheduler.

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
| E1d.2a | Invocation and cold-task identities | Delivered in 0.112: owner/producer admission, one-shot completion, normal obligation checks and emergency retirement in sequential execution |
| E1d.2b | Queued cold dispatch and completion waits | Delivered in 0.113: ready admission, parked callers, one-use completion wait IDs, stale/duplicate rejection and unchanged sequential observations |
| E1d.2c | External readiness and fault-domain admission | In progress: suspended/ready storage delivered in 0.117–0.118 and runtime readiness in 0.119; domain identities/admission are delivered in 0.120 and reference sibling retirement in 0.121; host resource settlement and source integration remain before spawn |
| E1d.2c.1 | Addressable suspended callers | Delivered in 0.117: one-use suspension registrations paired with task/wait identities, atomic admission, original context restoration and emergency retirement |
| E1d.2c.2 | Independent ready-context admission | Delivered in 0.118: prepared child contexts retained outside the active stack, checked dispatch selection and active-chain protection; source execution remains sequential |
| E1d.2c.3a | Executable readiness protocol model | Delivered as test-only evidence on 0.118: repeated execution-scoped, generation-safe waits over checked borrowed execution, separate signalling/selection/completion and fixed shared history. These scenarios now target runtime integration in E1d.2c.3b; fault-domain settlement remains next |
| E1d.2c.3b | Runtime waiting and resumption | Delivered in 0.119: execution-scoped readiness tokens, explicit Waiting state, original contexts in wait storage, distinct start/resume dispatch and atomic signal admission. No source or adapter handoffs yet; owning fault-domain settlement is next |
| E1d.2c.4a | Fault domain identity and admission | Delivered in 0.120: distinct live root/named/lexical domains, stopped-ancestor gates, context membership and quiescence checks. Automatic sibling draining and retained-resource settlement remain E1d.2c.4b |
| E1d.2c.4b.1 | Reference sibling retirement | Delivered in 0.121: original emergency/boundary retention, borrower-before-owner retirement, queued-body suppression, descendant handler suppression, late-signal rejection and independent-domain preservation. Infallible scripted cleanup only; host resources and source concurrency remain follow-ups |
| E1d.2c.4b.2a | Repeated recovery and host-resource audit | Delivered as tests/documentation on 0.121: repeated borrowed sibling recovery in reference/expanded execution and real Rust frame-release probes; explicit native boundary resource membership remains the next gate |
| E1d.2c.4b.2b | Native resource scope membership | Delivered in 0.122: registry-scoped live identities, owned-input/capture adoption, borrowed-input exclusion, before-handler temporary cleanup and normal outgoing-owner promotion. Independent later allocations survive; native source scheduling remains ahead |
| E1d.2c.4b.3 | Native activation frame ranges | Delivered in 0.123: independently retired dense ranges, arena-scoped identities, hole reuse/coalescing and exact fault-resource release. Existing borrowed offsets and constructor destinations remain stable; executable native continuations are next |
| E1d.2c.4b.4a | Compiled native continuation model | Test-only evidence on 0.123: retained call/await/constructor activations, decision facts, one-use return delivery and quota/fuel/fault parity; public native execution is unchanged |
| E1d.2c.4b.4b | Compiled native loop continuations | Test-only evidence on 0.123: iteration reset, staged owned arguments, nested expanded completion and repeated cold awaits under quota/fuel/fault comparisons. Boundary continuations and context selection remain gated |
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
