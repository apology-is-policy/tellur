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

This loop slice left lexical/named boundary continuations and administrative recovery
gated. The following checkpoints retain boundary work/handler continuations before
independent diagnostic/resource context selection and source concurrency.

## Retained native boundary lifecycle (E1d.2c.4b.4c.1)

Compiler 0.124 moves the shared host boundary lifecycle into a retained record. It
owns the original parent diagnostic tracker, resource scope, lexical/named policy,
fault message and contingency phase. Existing synchronous named and lexical execution use the same
admission, fault handover, handler entry and finalization operations that a compiled
continuation can later call. Moving the record does not copy resources or replay work.

Source-fault handover releases unretained owners before handler entry and retains the
original cause. Finalization restores the parent tracker and contingency context once;
normal lexical survivors move to the parent resource scope, while named or handled
boundaries settle their owners. Ordinary host errors remain errors. Foreign, retired,
nested-active or duplicate phase transitions cannot finalize another live boundary.
Tests retain records between host calls and exercise resource destruction, nested
handler faults/escalation, recovery and normal survivor promotion.

Public native work and handlers still use synchronous closures. The test-only extension
below joins this record to executable lexical continuations. The active tracker remains
in the host; selecting independent suspended native contexts and settling all sibling
borrowers before owners are still integration gates. Source `spawn`/`yield`, async
adapters and suspending cleanup remain unimplemented.

## Compiled lexical boundary continuations (E1d.2c.4b.4c.2)

The test-only compiled model now retains lexical work and handler positions, their slot
layouts and the original shared host boundary record. Ordered loop/boundary entries
preserve the actual nesting. Normal fallthrough, `again`, expanded helper completion,
`recover again` and process termination finalize only the boundaries they cross. A
boundary-end sentinel consumes neither source fuel nor instruction quota. Recovery
releases remaining iteration resources before resetting the same loop frame and facts.

An escaping child fault retires its original frame before handing over to the enclosing
lexical boundary. The retained diagnostic chain supplies the original report and cause;
unretained resources settle before the compiled handler starts. The aborted call has
no normal delivery. A handler fault or escalation leaves its boundary and can enter an
outer contingency, without reentering the same handler. Host failures bypass handlers.

The [boundary fixture](../tests/fixtures/native_resume_boundaries.tellur) combines guarded
boundary admission, normal fallthrough, loop transfers across boundaries, expanded
nested completion, borrowed cold awaits and repeated recovery. Separate cases compare
nested handler faults/escalation and process status. Original/expanded O0/O2 builds
match reference results, effects and fault reports at every fuel cut under three host
quota partitions, including zero quotas. Only the existing native/reference fuel-error
wording is normalized, including a fuel error's nested cause; report structure, sites,
identities and phases remain exact.

A real Rust-resource fixture separately compares the compiled model against the public
native backend at every fuel cut. It retains a borrowed parent and an owned child in
cold tasks, then either awaits them or faults before they start. Exact resource logs
verify child destruction once before the retained parent's contingency consumption;
process status, JSON and diagnostics also match without normalization. The existing
call/constructor and loop models pass the shared continuation changes.

This is evidence on compiler 0.124, with no public backend, compiler/LSP/VSIX or source
semantics change. The following slice adds named supervision and retained-parameter
delivery. Independent diagnostic/resource context selection must still precede native
sibling settlement and source scheduling. Host quotas observe a single execution chain;
they never select source siblings.

## Compiled named supervision (E1d.2c.4b.4c.3)

The compiled model parks a named supervision record in its caller while work executes
in a separate retained frame. The record owns the shared boundary lifecycle and the
handler target. On a source fault, nested child frames retire first. Retained work
parameters then move out of their original slots before the failed work frame is
released. The same resource scope preserves those owners while unretained values settle;
the handler receives the retained parameters and original fault message once.

Normal work and handler completion require the administrative `terminate` result. They
restore the supervising caller without delivering an ordinary outcome. Handler faults
and escalation finalize the same named record before reaching an outer named or lexical
boundary. Ordinary host failures finalize the record without entering contingency. Named
work and handlers remain unavailable as public entry controls.

The [supervision fixture](../tests/fixtures/native_resume_supervision.tellur) combines
guarded supervision, nested named and lexical boundaries, a repeated request loop,
borrowed cold awaits, retained context and a contingent helper call. Differential cases
also cover nested named handler faults/escalation and real retained Rust owners. Each
runs original/expanded O0/O2 programs at every fuel cut with the existing quota
partitions. The Rust-resource case requires exact destruction order, diagnostics and
completion; the other cases use the existing reference comparison, including its
fuel-message-only normalization. Earlier call, loop and lexical cases exercise the
shared driver changes.

This named-supervision slice retains one execution chain. The following model selects
independent native contexts; stopped-sibling settlement and broader backend validation
must still precede public source scheduling. There is no new native mode, async adapter
contract or compiler/editor release in this checkpoint.

## Independent compiled native contexts (E1d.2c.4b.4d.1)

The test-only context model now alternates independently compiled roots over one real
native host and value arena. Each parked root retains its compiled machine, original
diagnostic tracker, contingency messages and selected resource scope. Selection moves
those host fields together and restores the host's prior context before returning.
The model exclusively borrows the host, so external operations cannot replace its
active state during a selection. Effects, script cursor, fuel, resource registry and
frame storage stay shared; neither values nor active diagnostic chains are cloned.

Dedicated session identities and monotonic context numbers reject foreign and retired
keys. The selected resource scope must still be live and belong to that root. Admission
checks identity exhaustion before allocating a frame or changing host observations;
foreign, retired and wrong-root scope probes also leave parked work unchanged. A zero
quota does not select or advance a context. Each root has an explicit resource scope:
failure retires its frames and stranded temporary owners, while another root's owners
remain live. Normal completion promotes outgoing owners out of the retired root scope.

The [context fixture](../tests/fixtures/native_contexts.tellur) runs separate left/right
roots with nested named/lexical boundaries, borrowed cold awaits, handler escalation
and a shared scripted input stream. Original and explicitly expanded control/handler
bodies run at O0/O2 in both harness orders and three quota partitions, at every shared
fuel cutoff through settlement. Tests check distinct activation identities, isolated
fault causes, expected effects, exact input consumption and restoration of a nonempty
host diagnostic/contingency baseline. They also reject repeated terminal advancement.

A real Rust-resource case alternates one failing root with another root's successful
borrowed and owned awaits. Every acquired owner is destroyed once at every fuel cut;
abandoned children settle before their retained parent's contingency, and the other
root can still borrow and consume its own resources. The final registry, frame arena
and parked-context table are empty. Existing single-chain call/constructor and resource
comparisons cover the shared test harness extraction and generated-source embedding.

This is explicit test harness interleaving of independent roots, not Tellur `spawn` or
quota-based source scheduling. At a fuel cutoff the harness records remaining scripted
inputs instead of applying the launcher's single-root unused-script check; production
launch behavior is unchanged. It does not yet stop sibling contexts in one owning fault
domain, suppress their nested handlers during retirement or expose async host readiness.
Those settlement paths and broader backend parity remain the next integration gates.
This model checkpoint left the 0.124 public backend unchanged; the following release
moves its host-state operations into shared runtime support.

## Shared native host contexts (E1d.2c.4b.4d.2)

Compiler 0.125 provides shared native operations for dormant context admission, paired
selection/restoration, ownership validation and root retirement. A retained host context
owns its diagnostic tracker, contingency messages and resource selection. It is tied
to its host even while holding a scope-less baseline. Both selections are validated
before any field moves. The original context is restored by exchanging the same record;
callers keep exclusive host access throughout that interval.

An independent root is registered without selecting it or inheriting the host's current
boundary. Its owned inputs transfer resource membership, while borrowed leaves remain
with their owner. Root admission rejects invalid handles, a retired current scope or
scope-identity exhaustion before changing ownership or diagnostic allocation. A parked
context must select a live scope beneath its original root. Retirement requires that
root to be selected with no nested live scopes; it releases failed-root owners or
promotes normal outgoing owners, then rejects duplicate retirement.

The compiled context model now uses these shared operations. Direct host tests pause
inside a contingency, select unrelated work, then restore the original handler message
and cause without affecting the other root's Rust owners. Other tests cover foreign
hosts including scope-less records, wrong roots, nested-active retirement, failed
admission, stale selections, normal survivor promotion and 32 repeated context pairs
with distinct diagnostic IDs and bounded live storage. Existing native boundary,
resource and compiled continuation comparisons validate the shared extraction.

The public generated backend still executes calls and boundaries synchronously. These
operations do not by themselves supply task scheduling, fault-domain membership,
stopped-sibling retirement, process-wide stopping, async adapters or suspending cleanup.
Binding native contexts to owning fault domains and draining borrowers before the
owning contingency remains next, under the already accepted suppression policy for
nested sibling handlers. No source ownership, origin, fault or path-label rule changes.

## Compiled native fault-domain lifetimes (E1d.2c.4b.4d.3)

The test-only compiled model now gives each independent execution an owning fault
domain and a current domain. These identities belong to a shared domain registry,
separate from resource scopes, context keys, activation ranges and diagnostic IDs.
Ordinary calls, cold awaits and loop iterations retain their current domain. Named
supervision and lexical boundaries open child domains and retain the original domain
record with their work/handler continuation. Returning, recovery, process completion
and error unwind close the selected child before restoring its parent.

Every advance, including a zero-quota request, validates the owning/current ancestry
and the exact innermost boundary retained by the compiled continuation. A live ancestor
or an unrelated root is not a valid substitute for that boundary. Handler entry and
boundary retirement require a quiescent domain. Foreign, stale, forged-parent and
duplicate transitions fail before registry mutation; admission exhaustion is checked
before context input ownership transfers. Retired IDs are not reused.

Three registry cases cover independent roots, invalid selection and admission, and
64 repeated nested lifetimes with bounded live state. The compiled call, loop,
boundary, resource and independent-context cases now exercise domain retention across
host pauses and shared fuel cuts, and require an empty domain registry after final
settlement. The context harness also rejects substituting a live parent while paused
inside a nested boundary.

This checkpoint remains test-only on compiler 0.125. It establishes domain identity
and lifetime; it does not yet admit related sibling contexts, mark domains stopped,
or defer an owning handler while draining those siblings. That settlement protocol
must preserve the original owning boundary and suppress nested sibling contingencies
under the accepted policy before source scheduling or production backend promotion.

## Related compiled sibling settlement (E1d.2c.4b.4d.4)

The test-only native model now admits related child contexts into their parent's
current owning domain. A separate membership identity records the parent context;
normal child completion retires that membership without closing the shared domain.
Live children prevent parent retirement, and stopped ancestors reject child admission
before resource ownership or diagnostic state changes. Independent roots remain live.

An escaping child fault retains its original report and the original owning boundary
continuation. The model stops that domain, retires affected descendants before their
parent frames, and then resumes the owning fault handover. A stopped sibling applies
its nested resource fault dispositions without entering its lexical or named handlers.
An admitted child whose body has not run releases its captures without executing that
body. The owning handler sees the original child report as its cause; a locally handled
child fault leaves the enclosing domain and other children intact. Handler execution
does not reopen the stopped domain to new child admission.

The [resource fixture](../tests/fixtures/native_siblings.tellur) exercises a retained
named owner, a sibling paused inside nested named/lexical work, a descendant borrowing
the inner owner, an admitted but unrun owned capture, and an unrelated root. Its explicit
harness checks both normal joins and child-fault settlement over three repeated rounds,
original/expanded code, O0/O2 and several host-quota partitions. Actual Rust destructor
logs verify exact release order and counts; fault events preserve the original child
cause. Retired context selectors, admission into a stopped handler and attempts to
resume a parent with live children are rejected before observable work. Frames,
memberships, domains and resources are empty after each settled round.

This is a bounded parent-join harness on compiler 0.125: normal parent execution stays
parked while its children are live. It does not implement source `spawn`/`yield` or a
general scheduler. Terminal reports and related-context fuel exhaustion are covered by
the following slice. Process-wide termination, external readiness, already-running
sibling handlers and broader backend comparisons remain validation/integration gates.
Production generated execution, ownership rules and
fault policy are unchanged; async adapters, cancellation and suspending cleanup remain
separate work.

## Owning terminal reports and host limits (E1d.2c.4b.4d.5)

The compiled context model now preserves the owning root's full terminal report when
settlement escapes its boundary. It delivers that report once through the original
context key, separately from the triggering child's report. The original child remains
in the causal chain. A zero-quota observation leaves pending delivery untouched;
foreign-session keys and duplicate delivery are rejected without effects or fuel use.

The related resource fixture now covers an owning contingency that escalates after
completing its retained owner, plus fuel exhaustion after siblings and borrowed frames
are retained. A host limit bypasses source contingencies and applies resource fault
dispositions. Propagation through a related parent settles its ancestor, including an
ancestor with another retained child, without resuming normal parent work. The unrelated
root survives domain settlement but subsequently observes the same exhausted shared
fuel budget and releases its own owner. The test resets that budget only after the
whole round has settled so it can repeat the same fixture.

Original and expanded code at O0/O2 run normal, handled fault, escaping fault and three
fuel arrangements across three quota partitions and three rounds. Exact Rust logs check
descendant-before-owner release, skipped normal closes on host failure, and no repeated
destruction. Each round ends with empty frames, memberships, domains, resources and
terminal delivery storage. Existing independent-context fuel-cut tests also pass.

This remains test-only evidence on compiler 0.125. Context keys are harness selectors,
not source task handles or readiness signals. Active sibling contingencies are covered
by the following slice. Process-wide stop, readiness integration and explicit source handoffs remain separate gates;
no production scheduler, new cleanup-failure policy or language syntax is introduced.

## Stopping active sibling contingencies (E1d.2c.4b.4d.6)

The related native fixture now pauses a sibling inside an already-entered named or
lexical contingency, before it completes its retained Rust owner. When another child
faults, settlement releases that owner without executing the remaining handler, then
enters the original owning boundary's contingency. The sibling's earlier local fault
remains a separate diagnostic event; it does not replace the triggering child's cause.
No child is newly admitted into the paused handler in these scenarios.

Original/expanded O0/O2 tests repeat both handler forms over three quota partitions
and three rounds. Exact destructor logs distinguish emergency release from the skipped
normal `close`. Other retained work includes a parent borrower and an unrun owned
capture; the independent root completes normally. A nonempty host activation and
contingency baseline is restored after every round, with no leaked cause or fault mode.
The earlier six sibling scenarios run through the same baseline assertions.

This adds test-only coverage of the accepted stopped-sibling policy on 0.125. It
changes no runtime or compiler behavior. Whole-session process termination follows
below; readiness integration and explicit source handoffs remain later gates.

## Whole-session native process settlement (E1d.2c.4b.4d.7)

The compiled context model now treats accepted process termination as an administrative
stop of its entire session. It closes admission and selection, clears undelivered terminal
reports, then retires every remaining context in reverse admission order. Descendant
borrowers retire before their supplying frames. Nested work and already-active sibling
handlers apply resource fault dispositions without executing further source statements.
The process status is delivered only after this settlement; it is not a normal outcome.

The process entry's own resource root also uses emergency retirement. Checked owners
outside its outermost lexical contingency can require fault disposition even after the
handler has completed all of its retained obligations. Normal context completion still
preserves outgoing owned payloads. These tests retain the existing entry-only process
permission and resource checks; they introduce no new source escape.

The real-owner fixture covers nested named/lexical work, a descendant borrower, an
unrun owned capture, an independent root, a pending owning fault report and a process
entry owner outside its contingency. Additional cases stop siblings already paused in
named or lexical handlers. Original/expanded O0/O2 runs use three host-quota partitions
and three fresh sessions per process. Exact destructor logs check settlement order and
absence of ordinary closes. The session ends with empty frames, domains, memberships,
resources and report storage. Late root/child admission and both zero- and positive-quota
selection fail without changing effects, fuel, script position or identities. The host's
nonempty diagnostic and contingency baseline is restored.

This remains test-only context execution on 0.125. It does not make host quotas source
handoffs or implement source `spawn`, `yield`, external readiness or a production
scheduler. Readiness identities follow below; broader backend integration remains ahead.

## Compiled context readiness protocol (E1d.2c.4b.4d.8)

The test-only native context model now retains readiness registrations separately from
context selectors and terminal reports. A token identifies its execution session,
context and fresh registration generation. Registering a wait requires normal work
that has already started; unrun bodies, contingencies, stopped domains and outstanding
waits or unselected signals are rejected. Generation exhaustion is checked before
changing registration state. The original machine, frame storage and host context stay
parked throughout the wait.

Waiting contexts cannot advance or admit children. A valid signal changes eligibility
once without source execution, completion delivery, effects, fuel use or script
consumption. Explicit positive-quota harness selection consumes that eligibility;
zero-quota observations preserve it, as does rejected parent selection while children
remain live. A later wait receives a new generation. Duplicate, stale, crossed and
foreign-session tokens cannot wake it; a registration without its original retained
context cannot consume a signal. Contingencies remain outside readiness admission,
matching the private reference executor's current limits.

Domain settlement invalidates the owning registration and all affected child
registrations before handover. This covers both waiting and signalled-but-unselected
work, including recursive host-limit propagation. Unrelated waits remain usable.
Process settlement invalidates every registration along with pending terminal reports.
Readiness never replaces normal completion or the owning terminal report.

The existing real-owner scenarios now exercise these transitions across normal joins,
source faults, host limits and process stop, with original/expanded O0/O2 code and
three quota partitions. Repeated waits retain borrowed storage, and fresh process
sessions reject old tokens despite matching local counters. Snapshot checks preserve
internal values and observable host state on rejected operations and successful
signals. Exact Rust logs and causal reports retain their earlier expectations; all
wait storage is empty after settlement.

This is an explicitly selected context harness, not a FIFO scheduler or external I/O
adapter. Registration is a harness operation, never an automatic response to exhausting
a host quota. Compiler/LSP/VSIX remains 0.125 with unchanged public execution. Shared
native integration and accepted source `await`/`yield` handoffs remain ahead.

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
| E1d.2c.4b.4c.1 | Retained native host boundary lifecycle | Delivered in 0.124: shared boundary admission, source-fault handover, contingency entry and finalization with retained parent tracker/resource scope. Executable boundary resumes and independent context selection remain ahead |
| E1d.2c.4b.4c.2 | Compiled lexical boundary continuations | Test-only evidence on 0.124: ordered work/handler resumes, loop/recovery/process crossings, nested fault causes and real Rust owner cleanup across quota/fuel cuts. Named supervision and independent context selection remain gated |
| E1d.2c.4b.4c.3 | Compiled named supervision | Test-only evidence on 0.124: retained caller supervision, failed-work parameter handover before frame retirement, separate handler execution and exact real-owner cleanup. Independent context selection and native sibling settlement remain ahead |
| E1d.2c.4b.4d.1 | Independent compiled native contexts | Test-only evidence on 0.124: paired tracker/contingency/resource selection, original compiled machines, session-scoped keys and independent root cleanup over shared frames/effects/scripts/fuel. Native sibling settlement within an owning fault domain remains ahead |
| E1d.2c.4b.4d.2 | Shared native host contexts | Delivered in 0.125: host-bound retained state, dormant ownership-root admission, paired selection/restoration and checked retirement. Compiled models use the shared operations; native owning-domain membership and sibling settlement remain ahead |
| E1d.2c.4b.4d.3 | Compiled fault-domain lifetimes | Test-only on 0.125: distinct owning/current domains retained through named and lexical boundaries, exact parked-boundary validation and quiescent retirement across normal/error crossings. Related sibling admission and stopped-domain settlement remain ahead |
| E1d.2c.4b.4d.4 | Related compiled sibling settlement | Test-only on 0.125: child membership, stopped admission, retained owning handover, borrower-first resource cleanup and suppressed nested sibling handlers. Explicit parent-join harness; terminal/fuel coverage follows in 4d.5 |
| E1d.2c.4b.4d.5 | Owning terminal reports and host limits | Test-only on 0.125: one-use owning terminal reports preserve child causes; related fuel failures bypass contingencies and propagate through ancestors with retained children. Exact owner release and shared-fuel isolation; active handler coverage follows in 4d.6 |
| E1d.2c.4b.4d.6 | Stopping active sibling contingencies | Test-only on 0.125: named and lexical handlers paused with retained owners are retired without running remaining handler work. Exact release, isolated causes and host baseline restoration; process settlement follows in 4d.7 |
| E1d.2c.4b.4d.7 | Whole-session native process settlement | Test-only on 0.125: administrative process stop closes admission/selection, retires all roots and child trees, clears pending reports and applies resource dispositions before status delivery. Readiness follows in 4d.8 |
| E1d.2c.4b.4d.8 | Compiled context readiness protocol | Test-only on 0.125: session/context/generation tokens, separate wait/signal/selection, atomic rejection and fault/process invalidation over retained real-owner contexts. Readiness does not deliver completion; shared native integration and source handoffs remain ahead |
| E1d.2d.1 | Separate task queue and completion joins | Delivered in 0.126: queue admission without a waiter, one-use joins before/during/after execution, explicit delivery before consumption and atomic cold await. Private registry support only; retained completion payloads and source handoffs follow |
| E1d.2d.2a | Retained reference completion model | Test-only on 0.126: completed owned/exclusive-borrowed payloads outlive child frames, join before/after completion in either order, atomic rejected delivery and explicit stopped-result retirement with independent-root preservation. Production storage and native completion ownership remain next |
| E1d.2d.2b | Runtime retained completions | Delivered in 0.127: shared result/context storage, validated one-use delivery, domain quiescence and completed-result retirement before owning contingency, including parked callers. Source execution stays sequential; native completion ownership remains next |
| E1d.2d.2c | Reference process-wide retirement | Delivered in 0.128: entry process termination closes normal admission and retires registered completed results, ready/waiting work and parked callers across all domains before status delivery. Original status, fuel and entry-only source rules are preserved; source scheduling remains gated |
| E1d.2d.3a | Native retained completion model | Test-only on 0.127: completed compiled results retain their original domain membership, host context and resource scope until consuming join or emergency retirement. Owned Rust results and external borrows survive child frames; fault/process cleanup releases results before suppliers. Shared native scheduling and source handoffs remain ahead |
| E1d.2d.3b | Shared native result adoption | Delivered in 0.129: host API atomically validates a parked source, receiving scope and complete unique result-owner graph, transfers only owned leaves and retires the source resource root. The compiled completion model uses it; task lifecycle and scheduler integration remain ahead |
| E1d.2d.3c | Shared native retained execution | Delivered in 0.130: compiled continuations, fault-domain membership and retained contexts/results now live in normal native support. The compiled probes use that exact implementation; explicit selection/readiness, one-use result adoption and fault/process retirement remain separate from source scheduler/lowering integration |
| E1d.2d.3d | Shared task protocol and native invocation owners | Delivered in 0.131: reference/native adapters share queue/join/readiness transitions. Every retained compiled activation has a distinct invocation owner, capacity preflight and normal/emergency obligation retirement; native task payload/queue binding and source handoffs remain ahead |
| E1d.2d.3e | Bound native task payloads | Delivered in 0.132: internal cold captures carry opaque host/invocation/task/producer identity; cold creation and consuming queue/wait admission preflight before moving values, with FIFO one-use capture dispatch. Retained source lowering and context completion wiring remain ahead |
| E1d.2d.3f | Task-bound native contexts | Delivered in 0.133: queued captures enter retained contexts only after frame/resource/domain/owner preflight; original parent invocation and completion wait stay paired through readiness, result adoption and fault/process retirement. Retained source callback integration remains ahead |
| E1d.2d.3g | Scoped native task retirement | Delivered in 0.134: discarded bound cold handles retire before lexical contingency and recovery reset; normal scope/restart checks reject pending discard, while moved handles and borrowed suppliers survive. Retained callback handoffs remain ahead |
| E1d.2d.3h | Sequential native await contexts | Delivered in 0.135: exclusive cold-await context chains carry original diagnostic activations through nested readiness, join and fault settlement, preserving synchronous reports and resource adoption. Mixed child trees and emitted callback handoffs remain ahead |
| E1d.2d.3i | Retained native await handoffs | Delivered in 0.136: compiled callbacks retain queued captures and one-use delivery across explicit await admission, result adoption and fault/process retirement. Failed admission can retry without source reevaluation; delivery is administrative. Generated callback lowering and its driver remain next |
| E1d.2d.3j | Generated retained await callbacks | Delivered in 0.137: compiled fixture lowering binds deferred captures to their invocation and emits explicit await handoffs. Sequential and independent-root probe drivers use retained children and one-use delivery; entry tracker/raw-error return preserves full reports. Production lowering promotion remains next |
| E1d.2d.3k | Production retained native callbacks | Delivered in 0.138: normal native builds share the compiled callback emitter and sequential entry driver with retained probes. Calls, loops, boundaries and cold awaits use retained frames; diagnostics, fuel, loans and resource settlement retain parity. Source concurrency and mixed spawned-child integration remain gated |
| E1d.2d.3l | Independent-child cold awaits | Delivered in 0.139: an independently admitted native child can cold-await an exclusive descendant chain. Normal delivery, full child fault causes and borrower-first resource settlement agree with direct execution. Parent-with-live-children awaits and source scheduling remain gated |
| E1d.2d.3m | Prepared owner awaits with siblings | Delivered in 0.140: an already-prepared native cold await can enter beside independent children. Sibling faults restore the original owning diagnostic segment from live/completed awaited contexts or saved nested boundary parents before resource retirement. Parent execution with live children and source scheduling remain gated |
| E1d.2d.3n | Native retirement preflight | Delivered in 0.141: retained borrower membership is checked before runtime frame, loop or boundary retirement, and final-frame domain authority is checked before releasing storage. Parent selection remains guarded because generated callback evaluation can itself move operands |
| E1d.2d.3o | Native invocation ownership and atomic selection | Delivered in 0.142: every child/result retains its original parent invocation; normal selection and joins validate that authority. Lower-level Machine selection rejects live child/result membership before callbacks or pending delivery can change state. Fine-grained parent progress and source scheduling remain gated |
| E1d.2d.3p | Native callback access plans | Delivered in 0.143: immutable queries describe checked callback operands, field paths, borrow modes and storage retirement, with separate normal await-delivery writes and explicit opaque/unknown cases. Calls, run, direct await and cold-task await remain distinct. These symbolic plans do not grant selection permission; captured-loan resolution and source scheduling remain gated |
| E1d.2d.3q | Native access storage resolution | Delivered in 0.144: immutable queries bind symbolic places to live arena/frame/slot identities, preserve indirect holder reads and field paths, and reject invalid places without source effects. Resolved identities reject retired allocations and reused offsets. Captured-loan analysis and parent selection permission remain separate gates |
| E1d.2d.3r | Retained native capture descriptions | Delivered in 0.145: bounded immutable traversal records reachable carrier storage and reference permissions before admission, retaining snapshots through waiting, results and pending delivery. Unknown metadata preserves execution. Checked origin/holder guards and access-interference validation remain required before parent progress |
| E1e | Explicit cooperative cancellation and suspending cleanup; normal completion races, cleanup failures and no abandoned losers | Define cancellation observation and cleanup admission; no implicit cancellation exception |
| E2 | Pinned native coroutine experiment, then checked lowering and schedule-based differential validation | Reference semantics and legal frame retirement must precede native destruction |

These are completion boundaries, not a promise that every row can be implemented
without another design decision. Keep syntax proposals out of the runnable playground
until they work. When a row changes source behavior, update the grammar, reference,
editor, executable examples and rejection tests together.

## Independent queue admission and completion joins (E1d.2d.1)

Compiler 0.126 separates task execution phase from completion-join state in the
private reference registry. Queueing requires the original owner and producer,
appends ready work once and needs no wait identity. A join may claim any started
phase, including a completed child, without queueing or executing it. Delivery
requires the matching one-use wait; ordinary consumption requires that delivery.
Unjoined completed work therefore still prevents normal owner retirement. Abrupt
retirement removes its obligation without fabricating a normal result.

Cold await keeps its combined admission operation: it checks wait capacity before
queue mutation and commits both together. Wrong owners/producers, duplicate joins,
late delivery and exhausted wait identities leave registry state unchanged. Existing
readiness generations remain separate from completion waits and do not discharge a
join. Four registry cases exercise joins across every started phase, completed
unjoined obligations, exhaustion in every phase and isolated emergency retirement.

This checkpoint does not yet store completed payloads independently of the caller.
That storage must retain owned results and external borrow dependencies through
later joins and fault settlement before source handoffs can be enabled. No source
`spawn`/`yield`, public task contract or native scheduler is claimed here.

## Retained reference completion model (E1d.2d.2a)

A test-only ledger now holds completed outcomes and their diagnostic context after
checked child frames retire. The original caller and external borrowed storage stay
live. Queue admission uses the private registry without a completion wait; the
harness attaches joins either before execution or after both children finish.
Delivery validates the owner, open domain and one-use wait before taking the payload.
Missing storage, foreign owners, crossed waits and repeated delivery cannot consume
another retained result. Completed work still prevents normal owner retirement.

Two tests use [a checked fixture](../tests/fixtures/task_completion_model.tellur)
before and after expansion. Twenty-four normal combinations vary join timing, join
order and host quota, retaining moved strings and exclusive borrowed results after
both child frames disappear. Owning-stop cases reject normal delivery, retire the
completed payload before its caller and leave another root's result available.
Exact effects and empty final task/storage/domain tables are checked.

This is explicit harness scheduling and scripted reference retirement. The ledger
is not connected to production domain draining or source handoffs, and it does not
claim native retained-resource cleanup. Integrate completion storage into the runtime
and native result ownership before source `spawn`/`yield` admission. Installed 0.126
behavior remains unchanged; host quotas remain observation boundaries only.

## Runtime retained completions (E1d.2d.2b)

Compiler 0.127 promotes normal result storage into the shared reference runtime.
Each completed task retains its owned fields, external result references and original
diagnostic context after its child frame retires. Publication validates task/owner,
finished control state and live domain before moving anything. Delivery validates
storage, owner, domain and one-use wait before transferring the original result.
Cold await uses this path and immediately restores its caller as before.

Domain quiescence includes retained completions, and completed children remain
started obligations until delivery/consumption or emergency retirement. A stopped
domain retires affected completed payloads before source frames or contingency.
When a caller is already parked awaiting that result, settlement restores its
original diagnostic state and unwinds its retained chain without running normal
caller code. Unjoined results need no fabricated completion. Independent domains
and their results remain live.

The earlier checked-frame model now exercises these runtime operations. It covers
publication/delivery rejection, early and delayed joins, original/expanded owned
and exclusive-borrowed payloads, and automatic stopped-result retirement. The
owning-boundary regression also retains two completed results with parked callers:
normal caller work is suppressed, original fault evidence and fuel are preserved,
and an independent waiting root survives.

These are internal operations under explicit test scheduling. Source `spawn`/`yield`,
shared native task-owned completion storage and broader scheduler integration remain
separate gates. Reference cleanup here is infallible scripted retirement; this does
not extend native resource dispositions or add asynchronous cleanup.

## Reference process-wide retirement (E1d.2d.2c)

Compiler 0.128 gives administrative process termination a session-wide retirement
path. On the entry's `terminate process N`, all live domains stop admitting normal
work. The original status travels through emergency continuations as a process
disposition, never as a fabricated fault or normal outcome. The selected chain first
retires its affected borrowers; the executor then drains registered independent
chains and results before delivering the status once.

Completed payloads retire before their parked callers; queued, readiness-waiting and
signalled work unwind without executing remaining ordinary instructions or nested
contingencies. Unstarted invocation arguments retire without installing the body.
Nested call/await/supervision continuations propagate the administrative disposition
during this cleanup. No cleanup selection consumes source fuel, replays effects or
uses the ready queue as a source scheduling point. Late invocation admission,
completion publication/delivery, readiness registration/signalling and normal dispatch
reject before changing state.

The [reference regression](../src/runtime/process_tests.rs) retains owned and
exclusive-borrowed completions inside nested lexical domains, alongside waiting,
signalled and prepared independent roots with their parked callers. Original/expanded
runs across three quota partitions prove complete storage/domain/task retirement,
unchanged shutdown fuel and original fault evidence, no normal or nested-handler
output, and atomic rejection of late operations. Existing repeated fault recovery
still proves that domain-local faults preserve unrelated execution until an explicit
process stop. The [public regression](../tests/process_termination.rs) checks entry
termination after nested source-fault unwinding, one-use session delivery and native
O0/O2 parity before/after expansion; calling a terminating entry remains rejected.

This does not relax written termination: it still belongs only to an executable
entry's outermost lexical contingency. Independently progressing source tasks and
shared native scheduling remain unimplemented. The global path covers execution
contexts registered with the shared runtime; arbitrary contexts held outside an
executor by a test harness are not implicitly enrolled. Reference cleanup remains
infallible scripted retirement, without cancellation or asynchronous disposers.

## Native retained completion model (E1d.2d.3a)

The compiled context harness now separates finishing a child from consuming its
normal result. The child retires executable frames but keeps its domain membership,
original host context, resource root and result payload. Its parent cannot continue
while that obligation remains. An explicit consuming join transfers registered owned
result leaves into the original parent's selected resource scope, without following
returned references or moving their suppliers. An independent root joins into its
host scope. Each result can be consumed once.

Join validation precedes removal or resource transfer: it checks session, original
parent, live owning domain, retired child frames, quiescent original resource storage
and the exact set of owned result leaves. Early joins, duplicate/foreign/wrong-parent
joins, invalid domains and missing storage leave results, resources, frames, effects,
fuel and wait registrations unchanged. Early waiter registration remains a separate
reference-registry protocol; this native harness only consumes finished results.

Owning-domain fault settlement first retires affected completed payloads and releases
their Rust owners, then unwinds executable borrowers and suppliers before the owning
contingency. A result already joined belongs to the receiving boundary and follows
its fault disposition. Independent completed children and roots survive. Process
termination drains all retained results before remaining executable contexts and
status delivery, closing late join and selection admission.

The [compiled regression](../src/native/result_model_tests.rs) runs the
[checked fixture](../tests/fixtures/native_results.tellur) before/after expansion at
O0/O2 with three quota partitions. Both join orders, an owning fault before/after a
join, and process termination check exact Rust creation/borrow/close/destruction logs.
Returned borrows resolve against the still-live supplier after child frames retire.
The existing related-context, readiness, independent-context and single-chain models
remain regression coverage for shared test support.

This is a test-only ownership model on compiler 0.127, not a native scheduler or a
source concurrency release. It uses the existing Rust resource registry and its
infallible release disposition. Its result-adoption operation is now provided by the
shared host API in E1d.2d.3b; retained task/domain lifecycle still needs promotion. Host instruction quotas remain
observational cuts; they do not introduce source task handoffs. Source `spawn`,
`yield`, async adapters, cancellation and LLVM coroutine lowering remain gated.

## Shared native result adoption (E1d.2d.3b)

Compiler 0.129 moves result resource adoption into `Host::adopt_context_result`.
The caller supplies a completed parked source context/root, the result fields and
an optional parked receiving context/root. Without a parked receiver, the current
host scope receives the owners. Invocation ownership, finished frames, one-use join
and fault-domain permission remain caller obligations; this host API only manages
resource ownership. The compiled completion harness still validates those higher
level obligations before invoking it.

Preflight validates both contexts against the same host, a live unselected source
root with no nested resource work, and a distinct valid receiving scope. The result
must name every registered source owner exactly once, including nested aggregate or
cold-task captures. Duplicate, mistyped, foreign or retired Rust identities and
omitted/extra owners reject before any scope changes. Ordinary opaque script/builtin
values are not native owners; references are never followed or adopted.

Success changes only the owned resources' scope memberships, removes the completed
source scope and marks its root retired. No context selection, source execution,
resource destruction, new scope identity, diagnostic change or fuel consumption
occurs. The receiving scope subsequently applies the existing explicit completion
or fault-release rules. The original result values remain with the caller until its
join consumes them; the retired source context cannot be selected again.

Four [shared-host regressions](../src/native/result_adoption_tests.rs) use real Rust
owners to cover nested aggregates/captures, borrowed-reference isolation, malformed
owner graphs and crossed/stale scopes, receiving-boundary cleanup, independent roots
and repeated adoption with no new scope capacity. The existing compiled retained
result/context/sibling suites now use this API, proving returned live frame borrows,
both join orders and fault/process cleanup before/after source expansion at O0/O2.

The operation is an internal native foundation. Default source execution remains
synchronous, and native task/completion storage plus scheduler integration remain
separate checkpoints. No source `spawn`/`yield`, cancellation, asynchronous disposal
or change to source raw-construction/ownership permissions is introduced.

## Shared native retained execution (E1d.2d.3c)

Compiler 0.130 promotes the validated compiled continuation, context and fault-domain
implementation into the support embedded by ordinary native generation. The
[execution module](../src/native/execution.rs) retains native function-pointer
continuations, frames, decisions, loops, calls/awaits and named/lexical boundary
handover. The [context module](../src/native/contexts.rs) owns the shared arena and
host selection, ready/parked contexts, completed results and one-use terminal reports.
[Domain membership](../src/native/domains.rs) keeps the owning/current identities and
prevents normal retirement before nested boundaries and retained children settle.

Normal child completion retires executable frames while preserving the original
host/resource root, owning domain and result values. Consuming join checks the
original parent and exact owning boundary before the shared resource-adoption API
transfers owners; returned references keep their suppliers. A waiting/signalled
context is not a completed result. Readiness remains scoped to the execution,
context and generation, with separate signal and explicit positive-quota selection.
Fault settlement releases affected results before supplying frames and runs only
the owning contingency. Process settlement closes the session and drains all results,
waiters and unstarted roots before returning the status. Administrative cleanup does
not consume source fuel or resume ordinary work or stopped sibling handlers.

The original compiled fixture lowering and schedule drivers remain test-only and
inject only fixture callbacks/test schedules around this shared implementation.
Normal emitted programs include no environment-driven model quota reader, fixture
control table or harness schedule. Their default entry wrappers still use synchronous
generated control flow; shared support does not silently select tasks. Its bounded
parent gate still waits for all retained children, and native early join registration
and source queue/await handoffs remain separate integration work.

Three [direct shared-context tests](../src/native/execution_tests.rs) exercise the
exported runtime operations without the compiled fixture generator: retained owned
and borrowed results across readiness, rejected/duplicate joins, domain-local fault
retirement with independent results, and process-wide draining before status delivery.
Real Rust destruction logs and fuel/state checks distinguish result cleanup from
supplier retirement. The existing compiled single-chain, loop, boundary, context,
sibling/readiness and delayed-result probes now use the same emitted support.

This checkpoint changes the location and availability of native execution machinery,
not source semantics. Invocation-level task queue/completion-wait integration and
production resumable lowering still precede `spawn`/`yield`. Cancellation, async host
adapters/disposal and the direct LLVM coroutine target remain separate work.

## Shared task protocol and native invocation owners (E1d.2d.3d)

Compiler 0.131 extracts the existing reference task registry into
[shared protocol support](../src/task_support.rs). Reference and native wrappers
supply their existing error representations; both compile the same owner, task,
ready-queue, one-use completion-wait and generation-safe readiness transitions.
Reference value retirement remains in its adapter and uses the same pending-task
check. No reference transition, outcome, fault classification or source acceptance
rule changes.

Every retained compiled activation now admits its own invocation owner, distinct
from a context key, diagnostic activation or reusable frame address. A shared native
host registry keeps those owners across selection of independent contexts. Capacity
is checked before allocating the frame or transferring context captures; commitment
follows successful frame/diagnostic admission. Nested calls and named work/handler
activations get separate owners, while lexical scopes retain their invocation.

Before normal outcome or constructor completion, the runtime checks that this
invocation has no outstanding tasks while its original frame and boundaries still
exist. An invalid normal return takes the existing host-error unwind path. Emergency
frame retirement, administrative process completion and boundary termination retire
that invocation's obligations without fabricating task completion, invalidating its
ready/waiting registrations while preserving independent owners. Retained result
storage still owns its payload and fault-domain membership after the child invocation
itself has finished.

Four [native invocation probes](../src/native/invocation_tests.rs) cover nested calls
and 32 reused-frame rounds with fresh owners, normal-return rejection across cold,
ready, running, waiting and completed phases, owner exhaustion before frame/capture
admission, and administrative retirement without result delivery. The thirteen
protocol cases run under both error adapters. Compiled call/loop/boundary/context/
sibling/result drivers additionally require the shared owner registry to be empty
at full settlement, including source faults, host limits and process status.

These are internal foundations. Native cold-task payloads are not yet bound to the
new owner/task identities, and context queue/completion-wait wiring remains ahead.
Default native entries still use synchronous lowering. Legal cold lexical moves,
origin/ownership contracts and source fault rules are unchanged. This does not expose
`spawn`/`yield`, early native source joins, cancellation or asynchronous disposal.

## Bound native task payloads (E1d.2d.3e)

Compiler 0.132 adds [native task-payload support](../src/native/task_payload.rs).
An internal bound cold value retains opaque host, invocation, task and exact compiled
producer identities alongside its captures. Equal numeric IDs in another host or
an unrelated invocation do not authorize it; a same-named different control is not
the original producer. Moving the value keeps its identity. No lexical-boundary
identity is captured, preserving legal cold movement within the invocation.

Creation checks the live invocation, producer kind, argument count and task capacity
before taking the capture vector. Consuming queue admission checks the binding,
producer and argument shape, then atomically registers ready work and its completion
wait before removing the handle. Rejected admission preserves the original value
and registry. Dispatch checks host identity and FIFO position before transferring
captures once; it selects a producer but does not execute its body. Owned buffers
move without copying, and resource traversal still follows owned captures rather
than borrowed references. Dropping a queue object does not discharge its obligation.

The synchronous task representation remains available with no binding. Its existing
await helper accepts only that representation, so it cannot bypass the protocol for
a bound payload. The default generator still produces synchronous unbound tasks.
The new bound APIs are exercised directly; retained fixture callbacks do not yet
emit them. Pairing queued tasks and completion waits with retained native contexts,
result delivery, domain admission and fault settlement is the next integration step.
These APIs do not by themselves authorize cross-boundary started handles or source
`spawn`/`yield`.

Five [payload regressions](../src/native/task_payload_tests.rs) cover host/invocation/
producer mismatch, malformed and stale payloads, duplicate and out-of-order dispatch,
separate completion waits, task/wait exhaustion before capture movement, and sixteen
rounds of owned-buffer/resource transfer. A pending normal return takes the existing
emergency path, releases owned leaves once and leaves borrowed suppliers intact.
Cold lexical movement keeps the same invocation. Shared protocol and source language
semantics remain unchanged; asynchronous adapters, cancellation and coroutine lowering
are separate work.

## Task-bound native contexts (E1d.2d.3f)

Compiler 0.133 connects bound queued payloads to retained native contexts through
`Contexts::start_task`. It validates the original parent invocation, ready-queue
position, compiled parameter layout, open fault domain, membership and owner capacity,
resource capture identities and scope capacity, frame storage and diagnostic admission
before taking the queued captures. Duplicate or foreign resource owners reject as
well. A prepared frame reserves capacity without installing slots or consuming an
identity; stale or foreign plans reject before mutation. Preparation may reserve
host memory but changes no source storage, ownership, fuel or effects.

The admitted child keeps its queued task and completion-wait authority alongside its
original parent context and fault domain. Its own activation owner is separate from
the parent's task obligation. Normal child completion marks that task completed while
retaining the result scope; consuming join checks both parent invocation and wait
before resource adoption, then delivers and consumes the obligation once. Parent
normal progression stays blocked until its retained child contexts settle.

Context readiness now parks the associated task too. A signal makes the original
context eligible and queues its task; it neither runs the body nor delivers a result.
Positive-quota selection must follow the shared FIFO order. Zero quota, duplicate
signals and an out-of-order selection leave the registration and captures intact.
Unbound contexts retain their existing explicit host-selection behavior.

Stopped completed results and live task contexts abandon their task obligations
before supplier frames and the owning contingency proceed. Readiness registrations
become invalid, and administrative process termination drains task-bound contexts
across independent roots. Cleanup does not manufacture an outcome or execute a
stopped sibling handler. Queues still held outside `Contexts` remain the caller's
responsibility: failed admission returns their captures unchanged, and the invocation
cannot normally finish with those outstanding obligations.

Five [context integration cases](../src/native/task_context_tests.rs) exercise real
owned resources and returned borrows, two-waiter FIFO selection, early/duplicate/wrong
parent joins, nested invocation rejection, context/frame/owner/resource capacity,
malformed layouts, duplicate/foreign resource captures and stopped domains. They also
check result-before-borrower-before-supplier fault/process retirement, independent
roots and successful owning contingency only after task obligations disappear.
The [frame tests](../src/native/frames_tests.rs) cover cancelled, stale and foreign
preparations while preserving existing live slots.

These remain explicit internal host operations. Retained fixture callbacks still
emit synchronous unbound defer/await operations; production resumable lowering and
source `spawn`/`yield` remain ahead. Source ownership, origins, outcomes, fault policy
and legal cold lexical movement do not change. The next step is to connect retained
callback handoffs and continuation delivery to this task/context machinery.

## Scoped native task retirement (E1d.2d.3g)

Compiler 0.134 prepares retained callbacks for bound cold handles by retiring their
obligations when lexical fault cleanup or recovery discards their owned values.
The traversal follows task captures and constructed fields, never loan targets.
It validates every encountered host identity before mutating the task registry;
a foreign nested payload cannot retire an earlier local obligation. Repeated
emergency retirement is harmless and never starts a cold body or fabricates a result.

Normal scope exit and loop initialization/restart reject a still-pending task in a
discarded slot. Boundary records remain installed until successful cleanup, so a
rejected exit can still unwind the original scope. Fault cleanup, active-handler
failure, recovery and administrative exits retire discarded obligations before
resource dispositions and before the owning contingency proceeds. Task retirement
itself does not release resources: existing boundary/loop cleanup keeps responsibility
for owned captures and retained suppliers.

A move removes the handle from its former slot. Handles moved out of lexical work
or into recovery arguments therefore remain live and awaitable under the original
invocation. Borrowed captures do not retire their supplying values. These rules
match existing reference-value retirement; they add no normal task-discard operation
and do not make handles transferable between invocations.

Five [host cases](../src/native/task_retirement_tests.rs) cover whole-graph rejection,
foreign colliding identities, nested captures, repeated emergency cleanup, loan
non-traversal, normal-exit/restart rejection, legal lexical/recovery moves, real
resource dispositions and failure inside an already active contingency. Existing
retained-control and resource-adapter scenarios cover unchanged generated callbacks.
Source callbacks still produce synchronous unbound tasks; binding their defer/await
handoffs and preserving cold-await diagnostic chains remain the next integration step.

## Sequential native await contexts (E1d.2d.3h)

Compiler 0.135 adds internal `Contexts::start_await` for an exclusive sequential
cold-await chain. It shares queued-task admission preflight with `start_task`, then
enters the producer on the original caller tracker and moves that tracker into the
child. Failed admission retains the caller chain and queued captures. The parked
caller records its awaited child and cannot admit other children or readiness while
its chain is away. Nested sequential awaits use the same rule.

Normal completion retains the tracker with the result until consuming join. Join
validates the original caller and wait, adopts owned results, consumes the obligation
and returns the tracker before caller execution resumes. Borrowed result locations
remain in the original retained frames. A child readiness signal queues execution;
it does not return diagnostics or deliver completion, and zero quota does not switch
source execution.

For an escaping fault, settlement first returns the entire sequential chain to its
owning continuation, then retires stopped borrowers and applies existing resource
dispositions. It does not add the separate-child causal wrapper used by `start_task`.
The owning contingency sees the same activation identities, call/current sites and
causes as synchronous cold await. Failure inside that contingency preserves the same
causal report. Non-outcome administrative completion restores the caller chain without
creating a normal result; process termination drains the entire session.

This API currently accepts roots and nested sequential children without other retained
children. Combining independent child trees with sequential awaits is rejected before
transfer. This is an internal integration boundary, not a new language restriction:
source concurrency is still gated, and existing independent-child APIs keep their
separate diagnostic chains. Source callback emission and automatic handoff/Deliver
wiring remain ahead.

Seven [host cases](../src/native/sequential_await_tests.rs) compare exact reports,
source sites, activation identities, fuel and events against the synchronous retained
machine. They cover nested success/fault, readiness, owning-handler failure, real owned
and borrowed result adoption, failed/mixed admission, non-outcome completion and
process retirement of waiting/completed children, a fresh await after recovery and
host fuel exhaustion at child/caller return points.
Existing compiled-control cases
continue to compare original/expanded programs at O0/O2 across fuel cuts and quota
partitions.

## Retained native await handoffs (E1d.2d.3i)

Compiler 0.136 adds an explicit `Step::Await` and `Poll::Await` to the shared
retained native machine. A callback hands over a queued cold task and its original
return callback. The machine retains both in the current activation; exhausting a
host quota remains `Paused` and never becomes a source handoff. Repeated selection
of a queued await does not reevaluate the instruction or charge source fuel.

`Contexts::admit_await` transfers the retained queue into `start_await` only after
its existing admission checks. A rejected admission restores the exact queue to the
same activation. The host can retry or record a rejection with `reject_await`;
rejection unwinds the original caller and retires captures before borrowed suppliers.
It never runs the cold body or a normal outcome callback.

`return_await` checks the original caller and child, adopts the retained result,
consumes the task wait and restores diagnostics before recording a return. An ordinary
join cannot bypass a compiled await callback. On the next positive-quota caller
selection, the saved callback runs once, without another instruction or fuel charge,
before the next source instruction. Zero quota leaves it pending. Callback failure
uses normal caller unwind with the original frames and adopted resources still live.

Escaping faults discard pending delivery before the owning contingency. Administrative
non-outcome completion returns the caller key and propagates the completion through
its machine without normal outcome delivery. Process retirement releases queued
captures or returned-but-undelivered fields before their supplying frames; it does
not resume stopped callers or nested contingencies.

Eight [host cases](../src/native/await_handoff_tests.rs) cover retry, foreign/stale
returns, readiness, borrowed result storage, rejected admission and delivery, lexical
fault recovery, administrative completion and process retirement. Nested callback
awaits compare complete reports, events and fuel with direct compiled calls over
19 fuel budgets and three quota partitions. Another case checks await delivery inside
a synchronous callee, then its separate return into the original caller. These use
explicit compiled test callbacks.
Generated fixture/source callback lowering and its driver are the next checkpoint;
source scheduling, mixed child trees, `spawn`/`yield`, cancellation, suspending disposal
and LLVM coroutine lowering remain gated. Accepted source contracts are unchanged.

## Generated retained await callbacks (E1d.2d.3j)

Compiler 0.137 connects generated compiled probes to the shared retained await
protocol. `defer` captures its written arguments once, binds the cold value to the
current invocation and exact compiled producer, and stores it in the checked slot.
Consuming `await` queues that bound value and returns `Step::Await` with the original
outcome callback. The existing expression emitter preserves written evaluation order
and borrowed storage locations; source instructions still set their original sites
and charge fuel once.

The sequential probe driver follows only the selected await chain. It admits queued
children, retains the caller, adopts normal results through `return_await`, and selects
the caller for administrative delivery before its next source instruction. Failed
admission records rejection without reevaluation. Fault settlement can retire multiple
descendants and recover an ancestor; selection follows surviving contexts rather than
stale children. Non-outcome returns and process status never invoke ordinary outcome
callbacks. Independent-root probes maintain a separate await chain per explicitly
scheduled root; their host-selected interleaving is not a source scheduler.

Shared `Contexts::start_entry` moves the host's original diagnostic tracker into an
entry root and returns it after result join or terminal failure. An escaping fault
also retains its original untruncated error for final host reporting. Ambient callers,
source/call sites, activation IDs, fault kind, causes and truncation therefore survive
entry driving without a fabricated causal wrapper. Rejected entry admission leaves
the selected chain intact; process termination retires entry authority.

Two [entry host cases](../src/native/entry_context_tests.rs) compare complete reports,
ambient call chains, sites, fuel, events and long-error truncation against direct
compiled execution, and check rejected entry admission. A lowering regression verifies
that generated probes use bound captures and explicit handoffs while ordinary native
source emission remains synchronous. The compiled matrix exercises original/expanded
programs at O0/O2 over fuel cutoffs and quota partitions, including nested awaits,
borrowed constructors, lexical recovery, named contingencies, independent roots,
owned resources and process termination.

This checkpoint deliberately keeps the production `Generator` unchanged. Promotion
of the validated callback lowering and sequential driver is the next integration
step. Mixed spawned-child trees, source `spawn`/`yield`, cancellation, suspending
disposal and LLVM coroutine lowering remain later gates. No accepted source ownership,
origin, outcome, fault or path-label rule changes.

## Production retained native callbacks (E1d.2d.3k)

Compiler 0.138 uses retained compiled callbacks for normal native builds. Checked
regions lower through one [callback emitter](../src/native/continuations.rs); the
quota and independent-context probes embed that same generated code. Ordinary calls,
constructors, loops and named/lexical boundaries use retained frames and saved return
callbacks. Cold `defer` binds captures to the current invocation and exact producer;
consuming `await` admits a retained child and delivers its adopted result once.
The previous synchronous region-function emission is removed.

The production entry driver follows one sequential await chain. Synchronous calls
continue within that chain, and a host quota pause cannot select another task. Normal
result delivery, boundary recovery, administrative returns and process termination
use the shared context lifecycle. Original diagnostic trackers and untruncated errors
return to final host reporting. Invocation obligations and retained resource domains
must be settled when the entry returns. The native backend remains compiled Rust;
this change does not introduce LLVM coroutine lowering or a graph interpreter.

Test quota parsing and explicit independent-root schedules remain confined to probe
injection. Normal executables expose neither. Entry eligibility remains unchanged:
constructors, named boundary work/handlers and borrowed-result controls do not become
public launcher entries. Frame allocation still depends on each region's local layout.

Validation compares production and probe emission, direct and retained entry reports,
original/expanded native outcomes, fuel cutoffs, faults, borrowed constructors/carriers,
cold tasks, resource adapters and the maintained settings/HTTP/file applications.
The callback planning and emission body are unchanged from the validated 0.137 probe
emitter; production integration receives fresh native-consumer coverage.

Source execution remains sequential. Mixed spawned-child trees require diagnostic
chain and fault-settlement integration before the accepted source scheduler is enabled.
`spawn`, `yield`, cancellation, suspending disposal and async host adapters remain
gated. Ownership, origin, outcome, fault/contingency and path-label semantics are
unchanged; no new language-design decision is required for this promotion.

## Independent-child cold awaits (E1d.2d.3l)

Compiler 0.139 permits an independently admitted native child to cold-await its own
exclusive descendant chain. Each cold await still requires a caller with no other
children. Independent admission from a sequential or already-awaiting context remains
rejected, as does a parent beginning a cold await while its other children remain live.
The production entry driver continues to select one sequential chain; independent
admission and schedules are exercised only by internal host and compiled probes.

A fault moves the retained diagnostic tracker back to the head of its cold-await
segment before retiring affected frames. If that head is an independently admitted
child, its original control remains the outer fault cause, including activation IDs,
source/call sites, nested causes and bounded report truncation. A child-owned lexical
boundary restores its own chain before contingency and can recover to await again.
Stopped siblings still apply resource dispositions without running nested handlers.

Normal result adoption preserves owned resources and borrowed storage and delivers
once to the original caller. Admission exhaustion leaves captures and registrations
unchanged. Sibling faults retire pending captures, suspended children, completed
results and pending callback results before their suppliers; stale readiness, joins
and delivery reject. Administrative process termination also retires mixed trees.

Host tests compare exact direct/retained fault reports, long messages, events and fuel
at every cutoff. Resource probes check destruction order across each await phase.
The [compiled fixture](../tests/fixtures/native_mixed_await.tellur) uses generated
callbacks for nested borrowed cold awaits, lexical recovery and an escaping child
fault. Original/expanded O0/O2 runs compare full observations across quota partitions.
The explicit harness admits the independent child; this is not source `spawn` syntax.

Owner awaits alongside other live children require a separate tracker-restoration
slice. Source `spawn`/`yield`, started-task await lowering, cancellation, suspending
disposal, async adapters and LLVM coroutine lowering remain gated. Accepted language
ownership, origin, outcome, fault/contingency and path-label semantics are unchanged.

## Prepared owner awaits with siblings (E1d.2d.3m)

Compiler 0.140 allows an already-prepared native cold await to enter while independent
children remain live. Captures and wait identities still pass admission preflight
before transfer. Normal parent execution remains blocked until all its child contexts
and retained results settle; this checkpoint does not make arbitrary parent/child
selection legal. Independent admission from a sequential or already-awaiting context
remains rejected. Source execution continues along one sequential entry chain.

If an independent sibling faults, the owner's original diagnostic tracker is restored
before awaited descendants retire. That tracker may reside in a live or completed
cold descendant, or in the saved parent of its outermost lexical/named boundary.
Stopped cold and synchronous frames are removed from that owning diagnostic segment;
the independent failing child remains the cause. Stopping an active nested contingency
does not resume it or substitute its earlier fault for the triggering sibling cause.
When the owner's own cold chain faults, its original failure chain remains intact.

Normal joins can adopt the cold result or independent sibling results in either
order, but neither completion alone permits parent execution. Retained resources and
borrowed storage survive until their required joins or emergency retirement. Fault,
host-limit and process paths retire borrowers/results before suppliers, reject late
signals and callbacks, and preserve one-use delivery without additional source fuel.

Host comparisons cover exact reports with sites/identities, nested synchronous calls,
lexical work and active handlers, owned/borrowed result order, atomic admission failure,
shared fuel exhaustion and process stop. The
[compiled owner-await fixture](../tests/fixtures/native_owner_await.tellur) exercises
lexical and named boundary work/handlers, completed results and pending callback
delivery before a sibling fault. Original/expanded O0/O2 runs preserve the complete
owning fault report across stop phases and all observations across quota partitions.

General parent execution with outstanding children requires checked invocation/domain
quiescence before frames can retire or cross boundaries. Starting independent work
from sequential await contexts, started-task await lowering and explicit source
`await`/`yield` scheduling also remain separate gates. No accepted ownership, origin,
outcome, fault/contingency or path-label rule changes in this internal slice.

## Native retirement preflight (E1d.2d.3n)

Compiler 0.141 checks context child membership before the runtime applies a normal
return, constructor return, loop entry/restart/recovery/exit or lexical boundary end.
The check includes children admitted through the host without a task handle and
completed results that still retain borrower membership. Invocation task obligations
alone cannot detect those lifetimes. Final-frame domain authority is also checked
before retiring its invocation, values or diagnostic activation, including when a
normal result will retain the context membership until join.

This is a conservative runtime check: a child prevents retirement of any synchronous
frame in its parent context, even if a future invocation-aware analysis could prove
a particular frame independent. A rejected transition preserves the original scopes,
loop storage, invocation obligations and frame identities for settlement. The ordinary
Machine error path suspends supplier unwind until retained borrowers settle, and
administrative rejection does not create a source fault or run a contingency.

The parent-selection guard remains. A generated callback evaluates and moves its
operands before returning a runtime step; checking that returned step cannot undo
those source effects. This checkpoint only protects the runtime's subsequent storage
transitions. Before allowing parent progress, callback evaluation needs its own
invocation/loan preflight, including returned payload moves and direct-await handoffs.
Process termination retains its existing whole-session settlement route.

Four host regressions deliberately exercise the Machine beneath its earlier selection
guard. They cover ten transitions with live, waiting and completed borrowers; exact
state preservation; resource destruction order; a domain without a member; retained
and immediate root completion; normal last-result join; and error settlement before
supplier unwind. Existing generated call/constructor, loop, boundary/recovery, result,
sibling, owner-await and process models exercise the same embedded production runtime.
Source `spawn`/`yield`, independent admission from sequential contexts, started-task
await, cancellation, suspending disposal, async adapters and LLVM coroutine lowering
remain gated. No accepted source ownership or fault rule changes.

## Native invocation ownership and atomic selection (E1d.2d.3o)

Compiler 0.142 records the original parent invocation for each independently admitted
child, queued task and cold await. That identity stays with a retained result after
its executable frames retire. Normal child selection, readiness validation and join
require the original invocation to remain live. Joining also requires that invocation
to be the selected caller: another synchronous invocation in the same context and
fault domain cannot adopt its result. Checks precede resource adoption, result removal
and one-use task delivery. Emergency retirement still drains borrowers before owners
without requiring a normal join.

The lower-level native Machine now rejects selection with child/result membership
before entering its callback loop, including zero-quota requests. Rejection does not
move source operands, update the program counter or path facts, publish effects,
consume fuel or take a queued delivery. It is an administrative rejection, so it does
not start fault settlement or run a contingency. Already-pending fault settlement
retains its existing route. The outer context-selection and runtime-retirement checks
remain as well.

This is a conservative selection rule, not a new permission to run parents with live
children. A child's invocation identity establishes who owns it; it does not prove
that an arbitrary instruction can access storage borrowed by that child. Selective
parent progress still requires checked access information before callback evaluation
and before returned results write caller slots. Source scheduler handoffs remain a
separate gate, with host quotas never becoming implicit source switches.

Four new host cases cover operand/fact/effect/fuel preservation, pending-delivery
preservation and resumption after join, a different invocation in the same context,
and loss/restoration of the exact nested caller for all three admission routes.
The generated probe uses the existing loop/cold-await fixture: it inserts rejected
selections before generated callbacks and pending delivery, then compares full normal
observations against an unprobed run at original/expanded O0/O2 and quota partitions.
No source syntax or accepted ownership, origin, outcome or fault semantics change.

## Native callback access plans (E1d.2d.3p)

Compiler 0.143 generates an immutable access description beside each native callback.
It uses checked IR and the same dense slot layout and path guards as execution. A
query reads only the activation's program counter and facts: it does not evaluate
source values, follow references, move operands, consume fuel or publish effects.
Inactive paths have an empty skip description. Invalid points and unsupported
administrative states report unknown access.

Each symbolic access records a slot, field path, indirection and read, move, shared
borrow, exclusive borrow, write or retirement mode. Written operand order is retained;
simultaneous bindings evaluate all inputs before their writes. Loop restarts describe
argument evaluation before local retirement and parameter replacement. Return plans
include local retirement; constructor returns also identify the caller-supplied
destination. Short-circuit operands and alternative output writes are conservative
unions, so a listed access need not occur on every execution of that point.

Call, synchronous `run`, direct async `await`, consuming task await and `defer` retain
distinct transition identities. Callee/transitive accesses, host operations, pure
intrinsics, reference-bearing variant inspection and dynamic scope transitions are
explicitly opaque. An indirect place remains symbolic; the description is not a
resolved loan set or proof that an instruction can run beside a child.

For cold awaits, the input plan is retained before callback evaluation. Once a normal
outcome has been returned, the next query describes only the possible result-slot
writes, without repeating consumed argument accesses. Queued/waiting work, errors and
administrative process/settled completions remain unknown. Ordinary synchronous call
return and named supervision still require their opaque transition checks; this
checkpoint does not add an independently selectable synchronous delivery step.

Five planner tests cover operand ordering, field/indirection and borrow modes,
simultaneous moves, caller delivery, constructor destinations and loop-local retirement.
A host test checks normal versus administrative delivery and repeatable queries without
storage, task, fuel or resource changes. The generated selection probe queries actual
callbacks repeatedly across original/expanded O0/O2 executions and quota partitions,
compares complete observations, and still verifies rejection with live children.
Existing generated call/constructor, loop, boundary, fault and process models exercise
the changed emitter and shared support.

Both context and lower-level Machine selection continue to reject outstanding child
membership. The next gate must resolve captured loan/storage dependencies and compare
them with these plans before granting any parent progress. Source `spawn`/`yield`,
independent admission from sequential contexts, started-task await, cancellation,
suspending disposal, async adapters and LLVM coroutine lowering remain gated. No
accepted ownership, origin, outcome, fault/contingency or path-label rule changes.

## Native access storage resolution (E1d.2d.3q)

Compiler 0.144 adds immutable storage resolution for the generated callback plans.
Activation-relative slots resolve within that activation's live range. Each resolved
place records arena identity, allocation identity, slot and field path. Equal numeric
offsets in different arenas differ; a retained identity cannot become live again when
a retired range is reused. Even an empty or unknown plan requires a live activation.

An indirect access first records the read of its local reference holder, then resolves
the referent using the holder's path followed by the instruction's field path. Shared
references cannot describe an indirect move, exclusive borrow or mutation. Field paths
are checked by borrowing the existing values, without cloning owned buffers or resource
owners. Whole-slot writes and retirement may address uninitialized storage; reads,
borrows and nested field paths must already exist. Constructor destinations resolve
in their caller's storage. Normal cold-await delivery retains its separate write plan.

Queries leave frames, operands, task obligations, source fuel, effects and resources
unchanged, including when a later access in the plan fails. Missing holders, invalid
fields, unavailable reads, foreign/retired activations and destinations outside live
storage reject administratively. Opaque/unknown flags remain intact. Conservative
unions can describe accesses that are not taken; a query does not evaluate an expression
to decide which branch occurs or turn an unavailable conservative access into permission.

This is a current storage description, not a loan certificate. Source reference values
still carry offsets: resolving an address cannot reconstruct its earlier allocation
history. Retained resolved identities detect subsequent retirement/reuse, but newly
supplied forged or stale reference values still require the existing checked-execution
assumptions. Queries must be repeated after storage changes; allocation liveness alone
does not prove a holder or aggregate still has the same meaning. Reading a reference
inside a carrier does not recursively read its referent. Captured carrier dependencies,
permission attenuation and their lifecycle through results remain the next analysis.

Five new host cases cover allocation reuse and foreign identity, direct/indirect paths,
shared/exclusive access, atomic failure, uninitialized destinations, preserved buffer
allocations and exact resource destruction. The existing await test also resolves
normal result writes while keeping administrative results unknown. Generated probes
query every selected context in the loop/cold-await and constructor/borrowed-result
fixtures, compare original/expanded O0/O2 observations and quota partitions, and retain
rejected parent-selection checks. Expansion may inline constructors; the original
fixture specifically verifies the caller-supplied destination.

Both parent-selection guards remain unchanged. Resolving a nonopaque plan does not
permit parent execution beside children. Captured-loan membership and per-instruction
interference checks must agree with invocation/domain authority before that gate can
change. Source scheduling, independent admission from sequential contexts, started-task
await, cancellation, suspending disposal, async adapters and LLVM coroutine lowering
remain separate work. Accepted source ownership, origins, outcomes and faults are intact.

## Retained native capture descriptions (E1d.2d.3r)

Compiler 0.145 describes the storage reachable through native captured references.
The immutable traversal follows actual record/sum payloads and owned task arguments.
A borrowed carrier retains its own storage and its contained reference dependencies;
an owned carrier contributes its reference leaves without inventing a loan of its
former source slot. A shared outer reference attenuates contained exclusive authority.
Repeated places merge conservatively, and stronger authority revisits a formerly
shared path. Cyclic reference graphs terminate through visited-place tracking.

Descriptions bind the resolved allocation identities and paths from E1d.2d.3q.
Traversal uses an explicit work list and a budget of 4096 value visits and reference
path components; merged descriptions are also bounded. Missing/invalid storage or an
exceeded budget yields explicit unknown metadata. It is never treated as an empty
loan set. This metadata is descriptive: unknown results do not reject an otherwise
existing execution, consume source fuel, move values or run resource dispositions.

Independent roots/children and queued tasks snapshot arguments before context admission
transfers them. A compiled cold-await handoff also keeps its queued capture description.
Readiness waits and signals preserve these snapshots. Normal completion adds the
reachable result dependencies while conservatively retaining every input dependency;
it does not infer a narrower result-origin contract. Returning a compiled await carries
that combined description into the caller's pending delivery, after child membership
and its one-use wait have retired. The record ends with delivery or emergency retirement.
A manual host join transfers the result out of this runtime record; host result ownership
and the source checker's existing loan rules still govern its subsequent use.

Local never-started cold values retain their existing checker/task rules; this checkpoint
does not attach allocation snapshots at `defer` creation. Its persistent metadata starts
at handoff or context admission. Snapshots contain identities and paths, not source
values or resource owners. Retaining or discarding metadata cannot keep an owner alive,
settle a resource, or revive a retired allocation. Allocation-liveness checks remain
separate from whether a holder or aggregate currently has the same contents.

A resolved runtime graph is not the complete checked loan contract. In particular,
flattened reference values do not encode every origin or storage-holder guard retained
by the checker, including references reached through exclusive carrier fields. Those
guards and exact invocation/domain authority must be incorporated before using these
descriptions to authorize parent access. Both existing parent-selection gates stay in
place. Source scheduling and the accepted ownership/origin/outcome/fault rules do not
change.

Six new host cases cover nested/owned/projected carriers, permission attenuation and
strengthening, cycles and bounds, stale identities, preserved buffers/resources, all
three admission routes, readiness, retained results and unknown-metadata execution.
Admission-rejection tests now compare capture descriptions, and the pending-delivery
case verifies that join does not lose them. Three generated probes query all selected
contexts across original/expanded O0/O2 runs and quota partitions, including the
[runnable carrier example](../playground/carrier_tasks.tellur). Its detached result
still permits carrier retirement after await, with unchanged effects and outcomes.
Existing fault/process and real-resource models cover emergency retirement.

Next, combine these reachable-storage descriptions with checked origin/holder guards
and compare complete dependency sets against callback accesses before relaxing parent
selection. Opaque calls, synchronous return delivery, independent admission from
sequential contexts, started-task await, source `spawn`/`yield`, cancellation,
suspending disposal, async adapters and LLVM coroutine lowering remain separate gates.

## Checked capture guard transport audit (E1d.2d.3s)

The acceptance audit runs on compiler 0.145; guard transport into native dependency
metadata remains in progress. The
[guarded capture example](../playground/guarded_task_captures.tellur) detaches an
exclusive field through an ordinary control, then reborrows it into a moved cold task.
Its completion still protects both the external owner and the carrier. A shared
reborrow of that exclusive field also retains the carrier guard. A copied shared
field has different authority: its carrier can retire before task creation while the
external owner remains protected. Explicit transfer consumes the detached alias;
ordinary reborrow makes it usable again after the result's last use.

The checker already distinguishes these cases. `Loan.origins`, `Loan.parents`,
contained loans, path domains and permission families retain the required authority.
`BorrowSource.guard` and contained result paths also preserve it in checked public
result contracts. Before 0.146, local reference expressions carried only a resolved place
and type. The partial checked evidence below now accompanies those expressions. Native `reference` resolves that place to an offset and field path;
`V::Reference` gained a partial guard record in 0.147, described below. Traversing the detached reference
cannot rediscover the carrier that authorized it. Adding only result-origin handling
would still miss local aliases and nested captures.

The next implementation must carry conservative checked dependency evidence before
that information is erased. It must survive owned carrier and task moves, reborrows,
ordinary/constructor result delivery, selected sum alternatives and expansion remapping.
Unsupported or unavailable evidence must remain explicitly incomplete. Inferring a
guard from every reference holder would incorrectly retain copied shared carriers;
resolving every possible tagged path would read inactive alternatives. Neither an
allocation identity nor the current graph description establishes this authority.
Per-instruction interference and parent selection remain gated until the retained
evidence covers these cases under exact invocation and fault-domain identities.

Three source acceptance tests cover guarded capture/result lifetimes, transfer versus
reborrow, shared attenuation and early retirement of copied shared carriers. A fourth
generated selection probe compares the example with the reference executor at O0/O2,
before/after expansion, with immutable queries and rejected parent selections at host
pause points. All four generated selection probes now compare their observations
across expansion as well as optimization and quota partitions. This audit changes
tests and documentation; it does not enable source scheduling or alter the compiler.

## Partial checked reference origins (E1d.2d.3s.1)

Compiler 0.146 preserves a descriptive subset of the checker's reference origins in
`Expr::WithReferenceEvidence`. Its explicit `Partial` variant records symbolic slot
identities, field paths, carrier-guard flags and opaque-origin flags. Direct call and
task arguments, stored references, borrowed decisions, reference exits and private
address reads for received carrier fields carry this evidence. Local aliases remain
erased from execution, but their current checked origins are recorded before emission.
Copied shared contents do not acquire an invented dependency on their former holder.

The wrapper evaluates exactly its reference operand. It does not read the described
origins, consume extra fuel, retain storage or resources, or change contributor traces.
Native access plans likewise describe only the actual operand accesses. Native values and the runtime dependency snapshots receive this partial metadata
starting in 0.147, described below.

Expansion remaps origin roots independently for each inlined invocation. Its separate
certificate validator reconstructs that remapping and rejects removed evidence, changed
roots/paths or weakened guard/opaque flags. Serialization preserves the explicit
partial status. Structural validation requires a reference operand, existing origin
slots and reference-typed slots for indirect origins. Symbolic paths can describe
abstract tagged storage; validating them as current ordinary field accesses would
reject valid stored adapters. They are never dereferenced by this checkpoint.

This is deliberately incomplete evidence. It does not transport loan ancestry, active
path domains, contained permission trees, selected sum certificates or invocation-local
permission families. An empty partial list and an unwrapped reference both remain
insufficient to authorize execution. Existing source checking continues to enforce
those rules, and both native parent-selection gates stay in place. The next slice must
represent the missing guarded/contained authority and bind usable evidence to runtime
allocation identities before comparing it with callback accesses.

Four integration cases inspect guarded versus copied-shared captures, direct erased
aliases, received carrier address reads, independent expansion instances, serialized
roundtrips and tampered certificates. A structural-validation case rejects malformed
reference operands and slot kinds. Existing carrier, sum, contract, contributor,
editor and native comparisons check that adding metadata preserves accepted behavior.

## Bound native reference origins (E1d.2d.3s.2)

Compiler 0.147 binds native reference targets to allocation identities when they are
formed. Reborrows retain the original identity and append only their actual projection;
copying or moving a reference preserves its record. Capture traversal rejects stale or
foreign bound identities instead of reconstructing them from reused offsets. Unbound
host-created references still have only a current runtime graph description, never a
certificate of their history or complete loan authority.

Emission evaluates each annotated operand once, then attaches its partial checked
origins. Direct origins bind their live allocation; indirect origins inherit the
reference's existing identity and origins. The remaining field path stays symbolic:
no described field or inactive sum payload is read. Guard and opaque flags remain
separate from the runtime graph's shared/exclusive capability. Missing holders,
unavailable identities and exhausted budgets produce unavailable metadata. They do
not cause a source fault, consume fuel, clone owners or run dispositions. Empty
partial evidence also remains incomplete.

Reference values carry this evidence through cold task arguments, owned carriers,
ordinary calls/results and received carrier address reads. Borrowed sum payloads
preserve their selected payload identity and inherited partial origins; collection
borrows project the saved target. Copying a shared reference out of a carrier or
collection preserves that reference's evidence without adding its supplier's guard.
Metadata is excluded from value equality, preserving the existing comparison behavior.

Context descriptions are now explicitly `Partial`, combining reachable runtime
storage with partial checked origins. These records survive admission, readiness,
retained completion and join-to-callback delivery. Origin union is bounded; inherited
origin entries and path components also count against the 4096 capture traversal
budget. Liveness queries validate origin allocations, not symbolic field paths or
current holder meaning. Descriptions contain identities, never resource owners.

Five host cases exercise reborrow/copy/move propagation, shared attenuation and copying,
symbolic selected-sum paths, collection results, allocation reuse and foreign arenas,
value equality and metadata budgets. The admission/readiness/result lifecycle test now
carries bound guard evidence. Generated selection probes require a guarded detached
capture to retain its guard through pending delivery across original/expanded O0/O2
and quota partitions, while comparing observations and rejected selections unchanged.

This completes native transport of the existing partial origin subset, not a complete
loan proof. Loan ancestry, active path domains, contained permissions, selected-tag
certificates and invocation-local families remain unrepresented. Conservative retained
origins may also outlive their relevance after a result narrows its origin contract;
that can make a description unavailable without invalidating source execution. The
next slice must establish conservative completeness for supported reference forms and
track unavailable ancestry explicitly. Both parent-selection and retirement gates stay
in place until complete dependency/interference checks agree with invocation/domain
authority. No source scheduler, `spawn`/`yield`, cancellation, async adapter/disposal or
LLVM coroutine ABI is enabled.

## Conservative direct reference completeness (E1d.2d.3s.3)

Compiler 0.148 collects existing checked loan origins through ancestry before local
aliases disappear. Ancestor origins retain their own projection rather than being
narrowed to the descendant's field. Contained origins are also collected conservatively.
The walk is bounded by 4096 loan/ancestry visits and 4096 origin/path components;
missing ancestry or an exhausted budget leaves partial evidence. It changes no loan
lifetime, source identity, contributor trace or accepted ownership rule. Supplier
storage is included only through existing checked guard origins, never simply because
a shared reference was copied from that supplier.

A new `Complete` annotation certifies a narrow local dependency closure: the referent
contains no references, every included origin has an ordinary valid path and no guard,
opaque flag or invocation-local permission, and all visited loans are unconditionally defined and either active or fully
consumed, with no contained references or selected-sum evidence. A transferred ancestor
can be inactive while its descendant still depends on those origins. Direct
owner borrows can qualify on an executed branch; aliases under conditional checker
domains stay partial, including domains introduced by an earlier constructor result.
Guarded, contained, tagged, abstract, missing and bounded-out cases remain partial.
This is conservative dependency coverage, not exact result-permission disjointness.

Both expansion and its separate certificate validator preserve coverage status and
remap origin roots. Promoting partial evidence, weakening complete status or changing
origins breaks source correspondence. Structural verification also rejects empty or
guarded/opaque complete descriptions, invalid complete paths and referents containing
references. It does not turn a structurally plausible artifact into source authority.

Native direct formation can install complete checked origins; reborrows and received
values must merge their incoming evidence. A complete callee annotation cannot promote
an incomplete caller. Allocation binding, symbolic partial paths and metadata-independent
value equality retain the 0.147 behavior. Capture descriptions become `Complete` only
when every reachable reference has a matching saved target identity and complete origins.
Owned values with no references have an empty complete dependency set; an unannotated
reference has partial evidence. Result merging preserves completeness only if every
input is complete. Unavailable identities, budget failures and partial inputs remain
conservative, with no change to execution or source faults.

The direct-capture generated probe checks complete descriptions at admission and pending
delivery; the guarded-capture probe continues to require partial guards. Both retain
original/expanded O0/O2 and quota-partition comparisons with immutable rejected parent
selections. New checker cases cover unprojected ancestors, complete scalar aliases,
conditional fallback and status tampering. Native host cases exercise incoming-proof
requirements, shared attenuation and complete/partial/unknown result unions.

No gate is relaxed by this checkpoint. Complete coverage still needs callback
interference checks under exact invocation and fault-domain authority. The next bounded
slice is an immutable interference query for complete direct descriptions, with an
explicit inconclusive result for partial/unknown evidence and opaque callbacks. Guarded
and contained completeness remains separate work; both parent-selection and retirement
gates, source scheduling, `spawn`/`yield`, cancellation, async adapter/disposal and LLVM
coroutine lowering remain unchanged.

## Immutable native interference queries (E1d.2d.3s.4)

Compiler 0.149 adds a diagnostic comparison between one retained direct child and its
owner's current callback. The query validates the current activation, original parent
invocation, host context, open owning fault domain and live allocation identities.
An unrelated context, a later invocation in the same context, a different current
boundary, a waiting owner or a retired/foreign child yields `Inconclusive`. Retained
completed children keep their dependency snapshots until join and are compared by the
same rules. This is a per-child observation, not an aggregate scheduling decision or a
reusable permission token; callers must requery after any state change.

Only nonopaque `Skip`, `Local` and normal pending `Delivery` plans qualify. Pending
normal delivery compares result writes, not already consumed argument accesses. Fault,
process, call, loop, boundary, retirement transitions and other administrative results
remain inconclusive. Local slot retirement is still represented as an access; frame and
scope retirement are separate unsupported transitions. Indirect callback accesses also
require their reference's saved formation identity to match the current target. An old
absolute slot cannot regain authority when a new allocation reuses that address.

Only `Complete` dependency descriptions with complete unguarded/nonopaque origins are
compared. The whole description's identities are validated before any overlap is
reported; partial/unknown evidence and stale later entries cannot be hidden by a valid
prefix. Equal allocation identity plus equal or ancestor/descendant field paths gives
`Overlap`; otherwise the comparison can return `Disjoint`. Broad checked ancestor
origins keep their own paths. Origins are not evaluated as runtime reads. Since their
access modes and result families are not represented, all overlaps stay conservative,
including shared read/read overlap. No result-family disjointness is invented.

Access resolution has a 4096 entry/path budget. Dependency and combined query validation
are bounded by the existing capture budgets, with at most 65536 pair comparisons.
Exhaustion is inconclusive. Queries neither evaluate source operands nor consume fuel,
move values, change facts, transfer resources or deliver results. Native-host cases
cover field and allocation identity, broad ancestors, completed children, foreign and
reused storage, incomplete evidence, unsupported transitions, exact invocation/domain
checks, pending delivery and bounded comparison. Generated original/expanded O0/O2
probes query actual callbacks under both quota partitions while preserving observations
and immutable rejections at both parent-selection layers.

Neither parent-selection nor retirement gates are relaxed. This completes the narrow
immutable query, not independent parent execution. The next integration step must
aggregate all relevant retained borrowers under one current owner and account for
pending results and descendant relationships before any permission gate can change.
Guarded/contained/conditional completeness, source scheduling, `spawn`/`yield`,
cancellation, async adapters/disposal and LLVM coroutine lowering remain separate work.

## Aggregate native borrower coverage (E1d.2d.3s.5)

Compiler 0.150 adds an immutable census of context-managed borrowers for the current
invocation. It walks direct children and descendants, including readiness-parked and
completed children, and reconciles those context edges with the independent fault-domain
membership registry. A context map cannot certify an empty borrower set while an
unrepresented membership remains. Missing, duplicate and reparented edges are
inconclusive. Unrelated roots' dependency descriptions do not participate.

The selected invocation and domain must still match every direct child. Descendants
retain their original parent invocation and must belong to the selected owning domain's
subtree. Stopped domains, waiting selected owners, unsupported current callbacks and
unavailable authority remain inconclusive. The supported callback plans and conservative
ancestor-overlap rules from 0.149 are unchanged. A disjoint per-child result cannot
hide an overlapping sibling or descendant; an earlier overlap cannot hide a later
incomplete dependency description.

Await captures also retain their original invocation and domain as query provenance.
The census includes queued handoffs before child admission, live/ready sequential awaits,
and normal results after join has retired the child's membership but before delivery.
Queued payloads validate their task owner; admitted children validate the saved edge;
pending deliveries require the matching owner/domain and a retired child identity.
A host-created sequential wait without a retained compiled handoff remains inconclusive.
Administrative/error results do not become normal delivery evidence. Query provenance
never changes admission, source faults, resource ownership or execution behavior.

Conclusive results report context, handoff and delivery counts with either disjointness
or one conservative overlap witness. Every included description must be complete and
live before a conclusive result is returned. The census limits the context/member scan
and visited activations to 4096 each. Aggregate dependency validation and comparisons
have separate 65536-component/pair budgets in addition to the existing per-description
limits. Exhaustion returns inconclusive without changing state.

Seven native-host cases exercise siblings, descendants, readiness and completed joins;
queued/live/returned awaits; missing memberships and reparenting; wrong pending
invocation/domain; bounded queries; and real resource preservation through fault
settlement. Existing authority cases additionally check aggregate rejection for a later
invocation and nested boundary. Generated original/expanded O0/O2 probes require both
conclusive and inconclusive census observations across quota partitions, including
complete pending delivery, while both selection layers keep rejecting live-child
parent execution. Source observations and fault/resource dispositions remain covered.

This covers registered contexts and retained await payloads, not local source aliases,
unadmitted cold values or results already returned to an external host caller. Source
ownership checking and both execution gates remain intact. No reusable authorization
token is produced, and a query result must be recomputed after any state change.
The next admission slice must bind a fresh aggregate check to each actual callback at
both selection layers, retain rejection before operands/facts/fuel change, and preserve
all retirement preflights. Guarded/contained/conditional completeness, source scheduling,
`spawn`/`yield`, cancellation, async adapters/disposal and LLVM coroutine lowering remain
separate work.

## One-transition native admission (E1d.2d.3s.6)

Compiler 0.151 adds `Contexts::advance_disjoint_one` for explicit host admission of
one supported transition beside retained children. It performs a fresh aggregate census
and requires complete disjoint dependencies under the current invocation and open fault
domain. Partial, unknown, overlapping, opaque, unsupported or exhausted evidence rejects
before source operands, path facts, fuel or resource owners change. A waiting selected
context and a context already settling a fault cannot use this entry point.

Admission creates a private, non-cloneable witness inside the exclusive context operation;
no public query returns it and no caller can cache or replay it. Before stepping, the
machine rechecks the exact activation owner, task registry, control function, program
counter, path facts, constructor destination, domain, membership and resolved storage
plan. No callback, readiness signal, child admission or join can interleave between the
census and this lower check. The copied fact set is bounded at 4096 entries. The witness
is consumed by this check, and rejection remains administrative rather than becoming a
source fault or triggering settlement.

A successful admission executes exactly one `Skip`, `Local` or normal `Delivery`
transition. In particular, an instruction-free pending delivery returns before the next
callback; it cannot carry permission into a quota loop. The next call always performs a
new census. Actual admitted execution faults use the existing owning-boundary settlement
path, retiring borrowers and pending results before supplier owners and contingency.
Frame, scope, loop and final-frame retirement checks are unchanged.

The ordinary `Contexts::advance_retained` and `Machine::advance` quota drivers retain
their child gates at every quota, including zero. Production source driving still uses
those paths. This explicit host primitive does not introduce source interleaving, `spawn`
or `yield`. It is an integration seam for the accepted scheduling model, not a source
scheduler. Context-managed coverage still excludes local source aliases, unadmitted cold
values and results already transferred to an external host caller; source ownership
checking remains required. Host callbacks must obey their declared access plans.

Five host cases cover disjoint field writes beside live, waiting and completed borrowers;
fresh rejection after adding a borrower or losing complete evidence; wrong invocation,
changed facts and callback rejection at the lower layer; one-use pending delivery without
fuel; and real resource release order after an admitted fuel fault. Generated
original/expanded O0/O2 fixtures admit real supported callbacks beside a retained child,
reject unsupported/overlapping/incomplete selections, preserve legacy quota rejection,
and compare source observations across quota partitions. Complete result delivery is
required to exercise the new admission path.

The next integration step is bounded multi-transition host advancement with fresh
admission for every callback and administrative delivery. It must stop at unresolved
handoffs, reject each next transition before evaluation, and preserve quota/fuel and
fault-settlement behavior without treating host quotas as source handoff points.
Guarded/contained/conditional completeness, precise origin access modes, source scheduling,
cancellation, async adapters/disposal and LLVM coroutine lowering remain separate work.

## Bounded native admission (E1d.2d.3s.7)

Compiler 0.152 adds `Contexts::advance_disjoint(key, quota)`, an explicit host batch
built on one-transition admission. Every transition reruns the aggregate census and
consumes a newly issued private witness at the lower machine check. Permission never
survives between transitions, including between normal pending delivery and its next
callback. The ordinary quota APIs and production source driver retain their child gates.

The returned `AdmissionProgress` reports callback attempts (`instructions`), all admitted
attempts including administrative delivery (`transitions`), and an `AdmissionStop`:

- `Quota`: the requested callback allowance was consumed, including a zero allowance.
- `TransitionLimit`: the defensive limit of 4096 transitions was reached first.
- `Blocked(reason)`: the next transition lacks valid authority or complete disjoint evidence.
- `Poll(result)`: execution reached a non-paused context result.
- `Fault(result)`: an admitted attempt faulted; the result includes existing settlement.
- `HostError(reason)`: an internal host operation failed after an admitted attempt.

A block preserves earlier successful progress and does not imply rollback. The blocked
callback has not evaluated operands, changed facts or consumed fuel. A zero quota validates
the live selected owner without resolving or executing the callback; even an opaque next
callback may therefore return zero progress. Waiting, settling, foreign and retired owners
are blocked. Readiness registrations and terminal reports are not consumed by rejection.

The machine reports its actual instruction flag on successful steps. Normal pending
delivery counts as one transition and zero callback attempts, so it can be followed by a
freshly admitted callback within quota one. A faulting callback counts as one attempt;
a faulting administrative delivery counts as zero. These are host instruction allowances,
not fuel: skipped callbacks can consume allowance without source fuel. Both an admitted
fault and an internal host error stop the batch immediately, preserving the existing fault
report/resource settlement rather than relabelling it as administrative rejection.

The 4096-transition ceiling bounds work even for a very large requested quota. It does
not select another context or create a source handoff. The caller receives progress and
can choose its next explicit host operation. Overlap, partial/unknown origins, opaque
work, unresolved await, unsupported transitions and retirement remain conservative;
there is no automatic fallback to the unrestricted legacy driver.

Five new host cases cover partial progress followed by overlap/opaque rejection, the
transition limit and untouched siblings, delivery plus callback counting, selected-owner
readiness/foreign/terminal authority, and the public zero-quota API. Existing admission
cases now also check batch rejection after new borrowers or incomplete evidence and real
borrower-before-owner release after a fault. Generated original/expanded O0/O2 fixtures
exercise bounded batches across quota partitions and compare source observations, while
retaining the lower legacy rejection checks and complete pending-delivery requirement.

This completes bounded host composition of the supported local admission primitive.
The next prerequisite for broader parent progress is a narrow guarded-reference closure:
retain explicit partial status where origin modes, contained permissions, path domains
or formation identities are unavailable, and validate any newly complete guarded evidence
with the same conservative overlap and exact invocation/domain rules before admitting it.
Source scheduling, `spawn`/`yield`, cancellation, async adapters/disposal and LLVM coroutine
lowering remain separate work. Host quotas never become source handoff points.

## Complete local holder guards (E1d.2d.3s.8)

Compiler 0.153 certifies a narrow guarded-reference closure without changing accepted
ownership or origin semantics. The supported value is a fresh direct local reference to
a referent containing no references. Its existing checked guard must name valid local
holder storage containing references, with the paired guard/opaque flags that prevent
projection from narrowing that dependency. Permission families, indirect guards, abstract
paths, conditional ancestry, selected tags, nested loan contents and missing evidence
remain partial.

A borrowed holder ancestor can be wider than its original field guard. The checker keeps
that original guard, retains the ordinary unprojected ancestor origin, and adds an
explicit conservative guard for the entire ancestor storage. It does this only where an
existing checked local guard lies within that ancestor. A copied shared reference with
no holder guard never acquires one. Both exclusive and shared local reborrows preserve
the guard when obtained through an exclusive holder. The existing 4096 ancestry and
origin/path-component budgets include the additional guard; the guard search is bounded.

The independent structural verifier requires fresh direct formation and a local pointee
origin for a complete guarded annotation. Each guard must be paired, local and name a
valid reference-containing holder; every ordinary reference-containing ancestor must
have an exact whole-storage guard. Existing separate expansion/certificate substitution
preserves the complete discriminator, paths and flags. Tests reject missing, narrowed,
retargeted, weakened or unpaired guard proofs and incomplete pointee coverage.

Native binding permits complete guarded annotations only on fresh local formation.
A guarded annotation on an incoming value stays partial. Indirect, unpaired or unsupported
opaque complete annotations produce unavailable metadata, never new execution authority.
Received values and reborrows still require complete incoming evidence; captured and
returned payload unions cannot promote partial inputs. Allocation/formation identities
remain required, and metadata changes neither value equality nor source evaluation.

Complete paired guards participate in the existing conservative interference comparison.
All overlaps, including reads, remain blocked; a whole-holder ancestor is never narrowed
to the actual pointee or one guarded field. The fresh per-transition census and exact
invocation/domain checks remain mandatory. Host tests admit a disjoint local slot while
rejecting holder movement, pointee writes and writes to another field of the broadly
guarded holder, with live, waiting and completed children. No ordinary driver or
retirement check is relaxed.

The [runnable local example](../playground/local_guarded_tasks.tellur) exercises direct
local detachment, a cold task, borrowed result delivery and later owner reuse. Generated
original/expanded O0/O2 probes require complete guarded capture and pending delivery,
nonvacuous disjoint batch admission and equal source observations across quota partitions.
The earlier returned-family example deliberately remains partial. Checker tests also
cover shared attenuation, copied shared fields, conditional fallback, certificate
mutations and structural rejection; native binding tests cover invalid proof fallback.

The next prerequisite is bounded borrowed-carrier completeness, needed before extending
this proof to indirect holder guards. Establish full local stored-reference coverage and
incoming runtime completeness first; retain partial status for unavailable permission
families, conditional/tagged paths and abstract storage. Source scheduling, `spawn`/`yield`,
cancellation, async adapters/disposal and LLVM coroutine lowering remain separate work.

## Complete fresh local carriers (E1d.2d.3s.9)

Compiler 0.154 extends complete dependency evidence to fresh direct borrows of locally
owned records. Each reference leaf must point to a reference-free value and have exactly
one unconditional active, defined stored loan. The checked origin schema enumerates every
leaf, including fields inside nested records. The checker retains the carrier storage
origin and includes every stored loan in the existing conservative ancestry closure.
A direct local projection of a nested record uses that projected holder storage.

The supported layout and its reference paths are bounded by 4096-node/component budgets,
in addition to existing origin and ancestry limits. Missing or duplicate stored loans,
conditional activity, tagged paths, abstract layouts, reference-to-carrier leaves and
permission families remain partial. Borrowing through a carrier alias or indirect holder
also remains partial. Existing opaque holder guards and copied-shared-field rules are
unchanged; this checkpoint adds no new guard merely because a record is borrowed.

The independent structural verifier requires fresh direct formation, a supported bounded
record layout and an exact ordinary origin for the borrowed holder. Other origins must
satisfy the existing scalar/guard rules. Source ownership and complete stored-loan coverage
remain frontend obligations; independent expansion certificates reject erasing any stored
origin, holder origin or completeness discriminator. No public origin contract changes.

Native capture already visits the complete reachable runtime graph. The new static root
annotation cannot promote a partial stored reference or hide a stale/foreign formation
identity. Complete capture therefore requires both the checked closure and complete
incoming evidence at every reachable leaf. Returned permission-family descriptions can
still make result delivery partial. Such delivery stays blocked for disjoint host
admission; it never inherits completeness merely from an earlier complete capture.

The [runnable local carrier example](../playground/local_carrier_tasks.tellur) retains two
stored references through a cold task and reuses their owners after await. Original and
expanded O0/O2 generated executions check complete nonempty captures, retained dependencies
through completion delivery, disjoint batch progress and unchanged source observations
across quota partitions. Metadata cases cover nested records, projected holders,
constructor-result and alias fallback, conditional storage and certificate mutations.
Native evidence tests explicitly reject promotion of incomplete or foreign stored leaves.

The next bounded proof is carrier reborrowing through local aliases, retaining each broad
carrier ancestor and full stored-leaf coverage. Indirect holder guards and returned
permission families still need separate incoming-proof work. Source scheduling,
`spawn`/`yield`, cancellation, async adapters/disposal and LLVM coroutine lowering remain
separate; host quotas are not source handoffs.

## Complete local carrier aliases (E1d.2d.3s.10)

Compiler 0.155 extends the local-record proof to erased local carrier aliases. Their
runtime reference formation is still direct local storage. The checker follows the
retained alias ancestry, including transferred ancestors with unconditional inactive
status, and requires every ancestor to be a direct same-root prefix of the projected
carrier. Conditional activity/definition, tagged ancestry, indirect holders and nested
loan contents remain partial.

Each distinct retained carrier place must have a supported local owned record layout
and complete unconditional stored-leaf coverage. The checker collects all its leaf loans,
including owners outside the final projected record, and retains the original broad
ancestor origins. One shared 4096 budget bounds alias visits, layouts and leaf paths;
existing origin/ancestry closure limits still apply. A shared copy does not acquire new
ancestry or holder guards beyond the checked loan relationships.

The independent structural verifier permits ordinary reference-containing ancestors only
when they are direct supported record prefixes of the actual local target. Exact target
holder coverage remains mandatory. Unrelated carriers, unsupported descendants and
indirect ancestor origins are rejected. Expansion certificates independently reject
removing the broad holder, erasing its additional stored owner, or weakening completeness.
This changes metadata coverage without changing source ownership or origin contracts.

Native formation and graph traversal are unchanged: incoming incomplete or foreign proof
cannot become complete. Host tests use a projected reference whose runtime graph excludes
one ancestor-owned pointee. Complete checked ancestry still blocks that pointee, whole
holder movement and another holder field while admitting disjoint work. The same checks
hold for live, readiness-waiting and completed children, with rejection leaving state
unchanged and normal retirement preserved.

The [runnable alias example](../playground/aliased_carrier_tasks.tellur) moves a local
exclusive carrier alias, takes a shared projected reborrow and passes it to a cold task.
Original/expanded O0/O2 runs require complete nonempty capture and retained delivery,
checked disjoint host batches and source-observation parity across quota partitions.
Conditional aliases and references received from a call remain partial.

Next, audit incoming and received carrier references before extending indirect holder
guards. Static annotations must preserve checked result permissions while requiring
complete incoming formation and stored-leaf evidence. No source scheduling, `spawn`/`yield`,
cancellation, async adapters/disposal or LLVM coroutine lowering is added here.

## Incoming record-leaf evidence transport (E1d.2d.3s.11)

Compiler 0.156 distinguishes copying an incoming reference's evidence from asserting a
new complete local dependency closure. The `Incoming` checked-IR marker supplies no
origins and is not statically complete. It replaces empty partial annotations only when
materializing invocation-input leaves with singleton public origin groups, ordinary
record field paths and reference-free pointees. Owned and borrowed record inputs and
nested ordinary record fields qualify. A shared 4096 leaf/path budget bounds selection.
Unioned groups, tagged paths, nested reference layers and returned-value materialization
keep their partial annotations. All public permission families and source contracts
remain unchanged; later family-derived uses do not become complete in this checkpoint.

Native transport evaluates the field copy once, retains its exact formation target and
origins, and validates the original allocation identity, target path and origin liveness.
Complete evidence survives only when it was already complete. Partial evidence stays
partial; absent, stale, foreign, mismatched or over-budget metadata becomes unavailable.
Transport never rebinds an identity, adds a holder guard, changes value equality or raises
a source failure. Subsequent graph/census/admission checks remain mandatory.

Expansion remaps the copied place and preserves the marker exactly. Its independent
certificate checker rejects removal, retargeting and changes to partial/complete status.
Structural verification separately requires an ordinary record field copy with a
reference-free pointee. Expanded callee parameters are local slots, so the certificate,
not a post-expansion parameter-name test, anchors transport to its checked entry site.

Tests cover owned/borrowed/nested inputs, singleton/unioned groups, tagged and nested
reference exclusions, metadata identity failures and unchanged observations. Generated
original/expanded O0/O2 local-carrier models assert that actual input-field copies retain
nonempty complete evidence across quota partitions. Existing received aliases and
conditional/family-derived captures remain partial.

Next, close bounded indirect carrier and holder ancestry using the transported evidence,
while retaining broad holder/storage dependencies and conservative public group rules.
Source scheduling, `spawn`/`yield`, cancellation, async adapters/disposal and LLVM coroutine
lowering remain separate.

## Complete singleton shared input leaves (E1d.2d.3s.12)

Compiler 0.157 certifies subsequent uses of the shared singleton input leaves whose
formation evidence is transported by E1d.2d.3s.11. Entry checking records each eligible
private address and its exact permission family/member. An origin qualifies only when
it uses that original indirect address, has exactly that one permission, matches its
ordinary projection path, and has neither guard nor opaque flags. The exception applies
to reference-free pointees; it does not certify whole carrier closure. Exclusive leaves,
unioned groups, new returned permissions and conditional alias ancestry remain partial.

Shared field access through a borrowed record can use this evidence without retaining
the record merely as the source of the copy. Local copies, moves and projections retain
the full incoming pointee origin explicitly, even when erased shared aliases no longer
retain parent loan IDs. The existing 4096 origin/path budget charges this broad origin.
Checked ancestor closure still requires unconditional defined ancestry without tagged
or nested reference contents. Public ownership and permission checks are unchanged.

Native binding requires the original incoming allocation identity and complete evidence;
it merges every caller origin as well as the full incoming target and projected paths.
A complete annotation cannot promote a partial caller. No new runtime admission or
retirement rule is introduced. Host probes start with complete empty incoming origin
lists to prove that the explicit full target blocks sibling-field writes independently
of caller annotations. A copied shared reference permits holder release, while broad
pointee writes stay blocked across live, waiting and completed children. Partial inputs
keep admission inconclusive and unchanged.

The [incoming leaf example](../playground/incoming_leaf_tasks.tellur) uses borrowed and
owned input records, direct projections, moved shared aliases and borrowed task results.
The owned holder is dropped before the child is awaited. Original/expanded O0/O2 models
require complete captures and pending deliveries for this program, then inject partial
caller evidence and verify that it stays partial with identical source observations.
Certificates reject removal of broad origins and weakening of completeness. Negative
cases retain exclusive, grouped, conditional, returned and whole-carrier proof gaps.

Next, audit exclusive singleton input descendants and their indirect holder guards.
Those need complete broad holder ancestry in addition to leaf formation evidence; this
shared-copy exception cannot discharge that obligation. Source scheduling, `spawn`/`yield`,
cancellation, async adapters/disposal and LLVM coroutine lowering remain separate.

## Bounded native holder snapshots (E1d.2d.3s.13)

Compiler 0.158 ships an internal native holder-snapshot operation as a prerequisite for
indirect guarded input proofs. A detached leaf's actual reachable graph does not include
all other owners stored in its holder. Merely retaining the holder's symbolic allocation
would omit those owners when incoming origin lists are empty. The new operation snapshots
an actual incoming aggregate reference, independently of any later guard-attachment rule.

The snapshot validates the original holder allocation and exact path, then traverses the
actual aggregate and stored references with strict formation checks. Every stored reference
must have its original target identity; missing identities are not reconstructed from reused
slots. Inherited symbolic origin paths remain symbolic and are checked for allocation
liveness only. The resulting metadata includes the full holder as an ordinary dependency
and paired guard, every reachable stored owner, and all inherited origins. Access-mode flags
are not promoted into authority; subsequent interference stays conservative.

Traversal, paths and the final origin union retain the existing 4096 bounds. Partial root
or leaf evidence stays partial. Missing, mismatched, foreign, stale or oversized evidence
makes the snapshot unavailable without changing source values or raising a source failure.
Repeated or cyclic storage is bounded by the existing visited-place traversal. Snapshotting
does not move, clone or dispose source values, execute callbacks or register borrowers.

Host probes attach the snapshot explicitly in test code to a detached scalar leaf. They
start with complete empty holder origin lists, retain otherwise unreachable owners, block
whole-holder and sibling-owner access through live/waiting/completed children, and permit
only disjoint work. Partial stored-leaf evidence keeps admission inconclusive and unchanged.
Generated original/expanded O0/O2 carrier models query snapshots at real invocation inputs
and verify unchanged observations across quota partitions.

The compiler does not yet emit holder snapshots or relax its exclusive-input/indirect-guard
completeness gates. A snapshot is retained dependency evidence, not permission to attach a
guard, select a callback or retire storage. The next checkpoint must certify the source
holder relationship, broad ancestor coverage and exact input permissions, then preserve that
proof through independent structural verification and expansion certificates. Source
scheduling, cancellation, async adapters/disposal and LLVM coroutine lowering remain separate.

## Checked exclusive input holders (E1d.2d.3s.14)

Compiler 0.159 certifies a bounded exclusive input leaf together with its actual borrowed
holder. Eligible inputs are singleton reference fields along ordinary record paths in an
exclusive aggregate reference; their pointees contain no further references. The checker
matches the original entry permission family/member and projection, retains the exact field
guard, and adds the full incoming aggregate as both an ordinary dependency and a paired guard.
Unconditional local reborrow aliases can retain this proof when every ancestor and stored
leaf also closes. An unselected stored owner remains protected by the whole holder.

The checked IR uses a distinct `GuardedInput` descriptor naming the private original leaf
and its incoming field copy. Independent verification checks the ordinary record shape,
exclusive leaf/holder types, matching `Incoming` materialization, original pointee coverage
and exact field plus broad holder guards. Expansion and its independent certificate mapping
preserve the descriptor and all origins. A generic complete annotation cannot replace it.

After evaluating the reference operand once, native attachment validates the original leaf,
actual stored field and holder formation identities, reference types and operand containment.
It then snapshots the entire aggregate with the strict bounded traversal introduced in 0.158.
Every retained origin and stored owner is merged with incoming operand evidence; partial
operand, leaf, holder or sibling evidence stays partial. Missing, stale, foreign, mismatched,
weakened or oversized evidence becomes unavailable without changing source observations.
Symbolic origins remain symbolic; only the separately checked holder is traversed.

This is dependency metadata, not callback admission or a source scheduling point. Existing
fresh borrower census, conservative overlap, exact invocation/domain authority and retirement
preflights still apply. Shared copied fields remain holder-free. Whole incoming carrier
closure, grouped/returned/conditional/tagged/nested-reference and other unsupported ancestry
remain conservative. Source spawn/yield, cancellation, async adapters/disposal and direct LLVM
coroutine lowering are unchanged. Run `playground/guarded_input_tasks.tellur` for direct and
aliased leaves whose owners become available again after task completion and holder release.

## Complete incoming carriers (E1d.2d.3s.15)

Compiler 0.160 closes bounded references to incoming ordinary record carriers, including
projected reborrows and erased local aliases. Every reference-bearing path must pass through
ordinary records to a singleton input reference with a reference-free pointee. The checker
retains the original full aggregate and all original field materializations, even fields
outside the selected projection. Every checked ancestor must close under the same input
permission family/member and projection; grouped, returned or conditional permissions do
not gain completeness from an otherwise compatible type.

The explicit `CarrierInput` descriptor records the original holder and each private input
leaf with its field path. Independent structural verification reconstructs the full record
shape, requires unique exact field coverage and matching `Incoming` materializations, and
checks full root, selected path and leaf origins. Expansion and independent certificate
mapping preserve each descriptor identity and path. Original field copies remain discoverable
when inlining places them inside loops or fault boundaries, for both carrier and guarded input
descriptors. Generic complete metadata cannot stand in for this proof. Shape, ancestry,
descriptor and origin work remains bounded.

After exactly-once operand evaluation, native attachment checks live formation identities,
operand containment, access strength and each actual stored field against its original
materialization. It independently reconciles every actual reference field, then merges the
strict whole-holder snapshot with incoming evidence. This includes stored owners outside
the selected carrier and broad caller ancestry. Partial operand, holder, original leaf or
stored sibling evidence stays partial. Missing, changed, foreign, stale or over-budget
evidence becomes unavailable without source effects or admission authority.

The source example `playground/incoming_carrier_tasks.tellur` forwards a whole received
carrier and projects it through direct and moved shared aliases. Owners become available
again after completion and holder release. Original/expanded O0/O2 models inspect complete
captures and pending delivery, inject partial field evidence and retain the same observations
across host quota partitions. Host probes block broad holder and outside-owner access while
allowing only freshly checked disjoint work.

The next integration audit connects supported parent progress to explicit await handoffs
with live children. Unsupported proof forms may remain conservative; expanding each one is
not a prerequisite to that audit. Ordinary driver and retirement gates remain in force.
Source spawn/yield, cancellation, async adapters/disposal and direct LLVM coroutine lowering
remain separate work under the accepted explicit-handoff semantics.

## Checked cold-await admission (E1d.2d.3t)

Compiler 0.161 permits explicit native host advancement through a consuming cold-task
await while independent children remain live. The bounded form moves one whole local task
handle. Its access plan names the exact compiled producer, records only the handle move
as immediate access, and keeps possible result writes separate. Direct await, ordinary
calls, effects and cold-task creation retain their existing admission gates.

Before callback evaluation, the host reconciles every retained borrower and requires complete
conservatively disjoint dependencies. Immutable task preflight validates the handle's original
registry and invocation, exact producer identity, captured arity, cold phase, unclaimed join
and wait capacity. The private one-transition witness retains the current storage, binding
and prepared wait identity alongside its existing control/pc/facts/domain authority. The
lower machine rechecks all of them before moving operands, consuming fuel or changing facts.
A changed handle, exhausted capacity or incomplete/overlapping borrower proof rejects without
effects. A real callback failure uses existing fault settlement instead.

Successful preparation queues the cold task and returns an explicit `Await` poll immediately,
even with unused host quota. It does not run the child. Context admission and one-use completion
remain separate operations. Returned payload delivery receives a fresh access/census check;
it neither reevaluates the await nor consumes source fuel. Normal parent frame, loop and
boundary retirement still requires all retained children/results to retire.

The runnable `playground/owner_await_progress.tellur` performs local work before a cold await.
A generated harness admits a borrowed sibling after cold creation, then runs the same parent
callbacks through preparation with live, waiting or completed siblings. Original/expanded
O0/O2 observations match reference execution across quota partitions. Existing lexical/named
owning-fault models now admit a sibling before preparation, then exercise failure before and
after child admission, during nested work/contingency and around completion delivery.

This completes a bounded host integration step. Production source execution remains sequential;
ordinary quota drivers do not gain implicit parent selection or task switching. Direct await
still shares ordinary call lowering, and broader source scheduling must distinguish that
handoff as well as started-task joins. Host quota exhaustion and rejected proofs never become
source handoffs. Spawn/yield, cancellation, async adapters/disposal and LLVM coroutine lowering
remain separate work under the accepted invocation-owned, explicit-handoff policy.

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
