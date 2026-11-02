# Async calls and cold tasks

[Language reference](../language-reference.md) · [Design and next steps](../async-execution.md) ·
[Execution failures](execution-failures.md)

Compiler 0.113 supports `async control`, synchronous entry with `run`, direct
`await operation(arguments)` and local cold tasks with owned and borrowed captures, including reference-bearing carriers, and borrowed completions. Execution
remains sequential. There is no concurrent scheduler, I/O readiness or LLVM coroutine
lowering yet.

## Calling and entering

A synchronous control uses `run` to invoke an async control. An async control uses
`await` to invoke another async control. Both expose the callee's declared normal
outcomes through the existing `delta`, `capture`, forwarding and single-completion
call forms. Ordinary synchronous controls remain directly callable from either kind
of control. There is no implicit await or automatic parallel execution.

<!-- tellur:check -->
```tellur
async control greeting(string name) exits ready { string text } {
    exit ready { concat("Hello, ", name) };
}

async control startup() exits done {
    delta await greeting("Tellur") {
        ready { string text } => ready,
    }

    [ready]:
        emit(borrow text);
        exit done;
}

control main() exits done {
    forward run startup();
}
```

`run` is an entry operation, not a lexical block or fault boundary. In this profile
it completes the entire nested call before continuing its caller. `await` likewise
retains the caller until the direct child completes. Neither form creates a task
handle or returns early. `capture run operation(...)` and `capture await
operation(...)` store the **completed outcome**, just like other capture forms.

`run` is allowed in synchronous controls; `await` requires an async control. Their
target must itself be async. They are operation operands, not general value
expressions: use a delta or capture to obtain a result payload. The new `run operation(...)` form coexists with ordinary calls to a control named
`run`, spelled `run(...)`. Direct call syntax also supports qualified imports, generic specialization, traits, temporary argument
staging and parenthesized operands.

The executable or host-scenario entry remains synchronous. To launch an async
control, write a synchronous wrapper using `run`. Process exit-code policy belongs
to the wrapper's final contract. See [process entry](process-entry.md).

<!-- tellur:reject E_ASYNC -->
```tellur
async control work() exits done { exit done; }
control main() exits done { forward work(); }
```

Repair the call above to `forward run work();`. Inside an async control, use
`forward await work();` instead.

## Local cold tasks

A local task belongs to its creating invocation. Moving its handle does not create
another computation or change that owner. Await consumes it once; a completed
outcome captured from await is a separate value with its own handling obligation.
The reference executor now checks these identities explicitly, including retirement
before contingency after a fault. This adds runtime invariant checking without
changing the current sequential execution model.

`auto pending = defer operation(arguments);` evaluates and transfers arguments now,
then stores an owned task without entering the async body. The declaration must be
an immutable local `auto` binding inside an async control. Owned arguments may contain strings, records, sums, resources and their checked
contained references. Existing collection-origin boundary restrictions still apply.
Direct shared and exclusive references to existing storage are also
supported; see the borrowing rules below. A constructor can use transient borrows
while producing an owned argument with no references.

<!-- tellur:check -->
```tellur
async control greeting(string name) exits ready { string text } {
    emit("body started");
    exit ready { concat("Hello, ", name) };
}

async control startup() exits done {
    auto pending = defer greeting("Tellur");
    emit("arguments captured");
    auto moved = move pending;
    delta await moved {
        ready { string text } => ready,
    }

    [ready]:
        emit(borrow text);
        exit done;
}

control main() exits done {
    forward run startup();
}
```

The messages are `arguments captured`, `body started`, then `Hello, Tellur`.
`await pending` consumes the task exactly once and exposes its normal outcomes.
It works with `delta`, `capture`, `forward`, inline adapters and the existing
single-outcome call statement rules. `capture await pending` stores a completed
outcome; its handling obligation remains separate from the task's await obligation.
Moving a handle transfers that obligation. Every normal path must eventually await
it; tasks cannot be copied, cloned, explicitly dropped or silently forgotten, even
when the future has no arguments and no result payload. The existing path-sensitive
ownership checks allow separate mutually exclusive paths to consume the same handle.

Hovering `auto` shows `task<producer>`, identifying the deferred producer. This is
an editor description of a compiler-internal type, not a public type spelling.
Tasks cannot escape through a control result, including inside a wrapper. Public
task parameters/results and result-contract erasure have not been designed yet.

## Borrowed captures

A direct `ref t` or `ref mutable t` capture borrows existing storage until the task
is consumed by await or its execution boundary faults. `t` may itself contain
references; those dependencies are retained along with the carrier storage. Multiple shared captures may coexist. Exclusive
captures exclude overlapping reads, writes and other borrows. Disjoint record fields
can be captured independently. Moving the task keeps its loans and original backing
storage; it does not copy the referent or make the owner available early.

<!-- tellur:check -->
```tellur
async control update(ref mutable string text) exits done {
    deref text = "updated";
    exit done;
}

async control startup() exits done {
    mutable string text = "initial";
    auto pending = defer update(borrow mutable text);
    auto moved = move pending;
    await moved;
    emit(borrow text);
    exit done;
}

control main() exits done { forward run startup(); }
```

A reference argument follows ordinary call rules: `defer update(alias)` reborrows
`alias`, excluding its conflicting use until await, after which it is usable again.
`defer update(move alias)` transfers the exclusive reference instead; the old alias
stays consumed after await. A shared reference remains copyable. Neither spelling
moves the referenced owner's storage into the task.

Captured loans do not expire merely because the source has no later use of the
handle. The task must still be awaited on normal paths. On a faulting path its loans
last until boundary settlement. A borrowed resource's owner still needs its own
fault disposition when a possible fault can retire that owner.

<!-- tellur:reject E_LOAN -->
```tellur
async control display(ref string text) exits done { emit(text); exit done; }
async control startup() exits done {
    mutable string text = "original";
    auto pending = defer display(borrow text);
    text = "too soon";
    await pending;
    exit done;
}
```

Move the assignment after `await pending;`. A completion without references releases
all capture loans. If the completion returns a reference, that result retains its
declared origins after delivery. Unrelated captures are released at await.

Bind a temporary owner or reference before deferring work; deferred borrowing does
not adopt a synchronous temporary's lifetime. Tasks with borrowed captures require
unique local binding names and cannot yet be nested inside sums, records or
collections, passed through a control boundary, or returned. Direct reference
values extracted from existing record fields can be captured even when their referent
itself contains references; the task retains their established external dependencies.

## Borrowed completions

A task keeps each captured parameter's dependencies through local moves. On await,
the selected completion receives the direct and contained result origins described
by the producer's checked contract. This includes shared/exclusive references,
references in returned records and sums, and ordinary inferred result contracts.
No new lifetime annotation is needed at await.

<!-- tellur:check -->
```tellur
async control first(ref string left, ref string right)
    exits ready { ref string value from left }
{
    exit ready { left };
}

async control startup() exits done {
    mutable string left = "first";
    mutable string right = "second";
    auto pending = defer first(borrow left, borrow right);
    delta await pending { ready { auto value } => ready }

    [ready]:
        right = "available";
        emit(value);
        left = "available after last use";
        exit done;
}

control main() exits done { forward run startup(); }
```

Writing `left` before `emit(value)` is an `E_LOAN` error. Only `left` occurs in
this result's contract, so `right` is available immediately after await. An inferred contract that can return either input conservatively retains both
while the result is live. Distinct outcomes and public origin groups preserve their separate
contracts; await does not merge their dependencies.

`capture await pending`, inline outcome adapters and `forward await pending` also
preserve result origins. An exclusive result keeps its parent reference unavailable
for conflicting use until the result's last use, as with a synchronous borrowed
result. Explicitly moving that parent into the task still consumes the old alias.
No result may borrow a task-owned local that is destroyed at completion, or escape
the lifetime of the caller's local storage.

## Owned values containing references

An owned record, variant or option can be captured with its contained references.
Normal value rules still decide whether passing the value copies shared references
or transfers exclusive references and owned fields. Moving the task preserves each
field's origin and each sum's tagged payload. Captures remain retained even on a
faulting path that never awaits the task.

<!-- tellur:check -->
```tellur
struct view['a] { ref string text <- 'a; }

async control first(view input)
    exits ready { ref string value from input['a] }
{
    exit ready { input.text };
}

async control startup() exits done {
    mutable string owner = "hello";
    view saved = view { borrow owner };
    auto pending = defer first(saved);
    drop(saved);
    delta await pending { ready { auto text } => ready }

    [ready]:
        emit(text);
        owner = "released";
        exit done;
}

control main() exits done { forward run startup(); }
```

Dropping `saved` releases its copy of the shared reference; the task still protects
`owner`. After await, `text` retains the same external origin. If the captured value
contains exclusive references, passing it consumes those permissions instead.
It cannot be read again through the old binding. A surviving shared copy continues
to protect its own origins even after the task and its result have finished.

Contained references may themselves refer to records or sums containing references.
Such a capture retains both its referent's storage and the external origins carried
inside it. A returned contained reference preserves its declared storage guards and
origin groups. Selected tags and groups are not merged into one lifetime.

## Borrowing reference-bearing carriers

A task parameter may borrow an entire record or sum that contains references.
The capture retains both the carrier storage and its nested external dependencies.
The same rules apply when the carrier is reached through another reference.

<!-- tellur:check -->
```tellur
struct view['a] { ref string text <- 'a; }
async control show(ref view input) exits done { emit(input.text); exit done; }
async control startup() exits done {
    string text = "hello";
    view input = view { borrow text };
    auto pending = defer show(borrow input);
    await pending;
    exit done;
}
```

The carrier cannot be moved or dropped while the pending task borrows it. An
exclusive outer borrow also prevents conflicting access through the original
carrier; a shared outer borrow never grants mutation through an exclusive field.
Ordinary reference arguments reborrow; explicitly moving an exclusive alias into
the task consumes that alias.

A whole-carrier result such as `ref view value from input` retains the carrier's
storage and its contained origins after await. A detached shared field such as
`ref string value from input['a]` retains only its external origin: once no other
use needs the carrier, that carrier may be dropped while the result remains live.
References reached through exclusive carrier fields also retain their checked
storage guards. Await preserves these distinctions, including nested origin paths
and active sum payloads; it does not flatten them into one lifetime.

See [carrier_tasks.tellur](../../playground/carrier_tasks.tellur) for a detached
shared completion that releases the carrier at await but still protects its source.

<!-- tellur:reject E_LOAN -->
```tellur
struct view['a] { ref string text <- 'a; }
async control show(ref view input) exits done { emit(input.text); exit done; }
async control startup() exits done {
    string owner = "hello";
    view saved = view { borrow owner };
    auto pending = defer show(borrow saved);
    drop(saved);
    await pending;
    exit done;
}
```

Move `drop(saved);` after await. Owning a copy of the view instead, as in the
previous section, permits the original shared view to be dropped immediately.

## Contracts and resources

The `async` permission is part of a control's public contract and its checked graph.
Public snapshots expose `asynchronous: true`; changing the permission changes the
snapshot. Trait implementations must match the trait operation's permission exactly.
Generic and imported contracts preserve it. Hovering the callee shows `async
control`; hovering `defer`, `run` or `await` explains its current execution rules.

Arguments evaluate and transfer in written order. The existing owning, borrowing,
origin and fault-disposition checks still apply. Caller storage remains live until
the child completes. Returned borrows keep their declared dependencies after await;
await does not make the source available for mutation while such a borrow is live.
Direct call-duration temporary borrows retain their existing restrictions. This is
not permission to extend a temporary borrow into a cold task.

Faults propagate through the existing emergency channel and nearest enclosing
boundary. They never become a normal `failed` or `cancelled` outcome. Public and
trait controls still declare `faults` when necessary. Async invocation is unavailable
in contingency code in this bounded profile; async controls cannot be `@contingent`.
Existing fault isolation also applies to tasks: an outer borrowed task cannot enter
an isolated work block. Create the borrowed task and its owned request storage
inside that block. Its pending captures settle before contingency/recovery.

A deferred body may fault, but its fault permission is needed when awaiting it.
Argument constructors run immediately and require permission at creation if they
can fault. A task's owned captures remain live obligations between creation and
await: any intervening fault must have a valid fault disposition for every captured
resource. A payload-free completion contract does not erase those obligations.
On fault, an unstarted task releases captures under their declared dispositions;
its body is not started as cleanup. Native retained owners are settled before the
nearest contingency handler receives control.

## Transformations and current limits

The reference interpreter retains explicit call continuations for direct calls and consuming task awaits.
Its host instruction quotas can pause execution between instructions; these pauses
are not source await points or readiness scheduling. The native backend executes the
same bounded profile with sequential calls and stable caller storage. Since 0.111,
the interpreter separates shared storage, fuel and effect history from its
resumable control chain. This internal preparation does not change source execution
order or introduce scheduling points.
Since 0.113, a cold await queues its prepared child and parks the caller's control
state. One-use completion tokens validate delivery before the caller resumes.
Captured storage stays in the shared arena, and the diagnostic activation chain
travels with the child and back to its caller. Dispatch is administrative: it does
not consume extra instruction fuel or change host quota behavior. These internal
waits cover child completion only; source execution still has no overlapping work
or external I/O readiness.
Since 0.117, parked callers occupy a separate suspension table. Each registration
is paired with its task and completion wait, validated and removed exactly once
on completion or emergency unwind. Stale or crossed registrations cannot resume
a different caller. Since 0.118, prepared children likewise occupy separate ready
storage; dispatch validates the matching context and cannot replace an active chain.
Host quota exhaustion never selects another ready task. These changes affect internal
storage, not source handoff semantics.

Since 0.126, the private task registry separates queue admission from claiming a
one-use completion join. Queueing does not run the body or allocate a completion
wait. A join can attach before dispatch, during execution or a readiness wait, or
after completion; it does not change queue order. Completion remains an owned
obligation until its matching join delivers the result and consumption retires it.
Cold await still performs queue admission and join registration atomically, including
identity-exhaustion failure. Source `spawn`/`yield` are still unimplemented.

Since 0.127, the reference runtime retains completed payloads and diagnostic context
in shared task storage. Publication and delivery validate before moving any values.
Completed results keep their original domain live and remain task obligations.
Fault settlement releases affected result borrows before owner frames or contingency;
parked awaiting callers resume only for emergency unwind. Independent domains survive.
Cold await still delivers immediately, without new source scheduling points.

The [retained-completion tests](../async-execution.md#runtime-retained-completions-e1d2d2b)
exercise delayed owned/exclusive-borrowed delivery, rejected/crossed joins and automatic
owning-domain cleanup using the shared runtime. Harness scheduling remains explicit.
A [compiled native completion model](../async-execution.md#native-retained-completion-model-e1d2d3a)
now retains real Rust result owners and external borrows after child frames retire.
Its consuming joins transfer owners into the original parent scope; faults retire
completed results before suppliers and the owning contingency, and process stop
retires every result before delivering status. This is test-only evidence on 0.127.
Since 0.129, that model uses a shared host operation for result resource adoption.
It validates complete unique owned resources and their source/receiving scopes before
moving ownership, never follows borrowed references, and preserves host selection,
diagnostics and fuel. Task/domain authorization and completion storage remain
separate integration work; see [native result adoption](../async-execution.md#shared-native-result-adoption-e1d2d3b).

Since 0.128, entry process termination closes normal task admission and retires all
registered execution contexts and results before delivering the original status.
Prepared bodies never start; parked callers unwind without normal instructions or
nested contingencies. This administrative work consumes no source fuel. Late joins,
wakes and dispatches reject without changing state. Written termination remains
entry-only; see [process retirement](../async-execution.md#reference-process-wide-retirement-e1d2d2c).

Path expansion retains `run`, `defer`, direct `await` and task consumption as
explicit operations. It may inline ordinary
synchronous calls around them, but cannot erase the async boundary. Expansion
certificates check this distinction. Protocol analysis currently rejects these
boundaries rather than treating them as ordinary unobservable decisions.

Escaping tasks, a public `task<...>` spelling, nested borrowed task storage, `spawn`,
scheduling, cancellation and async host adapters remain unimplemented. No implicit
detach or cancellation by destruction is permitted. See the [design plan](../async-execution.md).

The [executable readiness model](../async-execution.md#executable-readiness-protocol-model-e1d2c3a)
checks future wait/wake obligations against retained checked frames. It is test-only;
it does not add an I/O API, source suspension points or scheduling to this profile.

Compiler 0.119 integrates the model's readiness transitions into private runtime
waiting and resume storage. A readiness signal makes a task eligible; only its actual
normal completion delivers an outcome or borrowed result. Resumption preserves the
original frames and fixed execution history. No source construct, host quota pause
or async adapter enters these waiting states yet. Owning fault-domain settlement
remains required before source concurrency is enabled.

Compiler 0.120 additionally records distinct domains for entry and named/lexical
boundaries. Deferred handles inherit the executing domain when awaited; their
existing movement across normal boundaries is unchanged. Stopped domains gate
normal task admission and wake/dispatch. Domain tracking and checks for retained
contexts do not yet provide automatic sibling settlement or enable source concurrency.

Compiler 0.121 adds private reference settlement of retained siblings. A failed
boundary keeps its original continuation and fault while queued, waiting or resumed
children retire before their owning storage. Prepared bodies never start during
cleanup. Nested sibling boundaries apply emergency resource retirement without
entering their contingencies; only the owning boundary handles the original fault.
This nested-handler policy is accepted for future source concurrency. Unrelated
fault domains remain live, and settled readiness registrations cannot resume work.
The current implementation covers infallible scripted retirement, with explicit
harness admissions; it adds no source scheduling or fallible/suspending host cleanup.

The [recovery and host-resource audit](../async-execution.md#recovery-and-host-resource-audit-e1d2c4b2a)
adds repeated-recovery and native destructor evidence on the same 0.121 release.
Those tests did not enable native concurrency. Compiler 0.122 now replaces native
boundary allocation snapshots with explicit resource scope membership. Owned inputs
and nested cold captures transfer membership; borrowed owners stay where they are.
Normal lexical exits preserve outgoing owners, while fault cleanup preserves unrelated
scopes and settles unretained resources before contingency. Internal scope-selection
tests exercise independent allocations; source scheduling and suspended native
context/resource integration remain unimplemented.

Compiler 0.123 also uses separately identified native activation ranges. Retiring one
frame clears only its slots and resources, preserving other live ranges and borrowed
addresses. Freed ranges can be reused after retirement. Native calls still run
synchronously: independently retireable storage does not implement suspended
continuations or enable source scheduling.

The [compiled continuation model](../async-execution.md#compiled-native-continuation-model-e1d2c4b4a)
provides test-only call/await/constructor resume evidence on 0.123. It retains original
frames, decision facts and result delivery across host observation cuts. Its
[loop continuation extension](../async-execution.md#compiled-native-loop-continuations-e1d2c4b4b)
also checks iteration reset, moved loop parameters and nested expanded helper returns
across repeated cold awaits. Lexically nested source loops remain unsupported. The
model is not a public native mode, source scheduler or new async permission. Existing
compiler and editor behavior is unchanged.

Compiler 0.124 retains native host boundary lifecycle state: parent diagnostic context,
resource scope and contingency phase. Both synchronous boundary forms use this shared
handover/finalization machinery, preserving original causes and cleanup order. This
prepares compiled boundary continuations; public work and handlers still execute
synchronously. It adds no source task handoff or async resource permission.

The test-only [lexical boundary model](../async-execution.md#compiled-lexical-boundary-continuations-e1d2c4b4c2)
retains work/handler positions and ordered boundary/loop state across host pauses. It
checks borrowed cold awaits, recovery, expanded completion, process termination and
nested handler faults against reference execution. Real Rust-resource comparisons also
check captured owner destruction once before contingency and retained-parent cleanup.
The [named supervision extension](../async-execution.md#compiled-named-supervision-e1d2c4b4c3)
also retains its caller while work and contingency run in separate frames. It verifies
retained parameters leave failed work before frame retirement, and only administrative
settlement resumes the supervisor.

The test-only [independent context model](../async-execution.md#independent-compiled-native-contexts-e1d2c4b4d1)
selects compiled roots with their original diagnostic, contingency and resource state,
sharing one host and frame arena. It checks isolated fault causes, borrowed storage,
shared fuel/input consumption and real Rust cleanup while another root remains live.
These are explicit harness selections of independent roots. The related models below
add fault-domain settlement; they do not enable source scheduling or change the
installed compiler's behavior.

Compiler 0.125 promotes host-context admission, selection/restoration and retirement
into shared native support. Retained state is bound to its host; resource selection
must remain beneath the original live root, and root retirement requires nested work
to be settled. Fault causes and contingency messages move together, while effects,
input cursor, fuel and resource owners stay shared. The compiled models use the same
operations. Public native calls remain synchronous; related-context execution below
remains test-only, with source scheduling still unimplemented.

The test-only [fault-domain extension](../async-execution.md#compiled-native-fault-domain-lifetimes-e1d2c4b4d3)
also retains owning/current fault domains separately from native resource scopes.
Named and lexical boundaries keep their original domain across work, handler and
host pauses. Parked selection checks the exact active boundary; retirement requires
its nested domains to have settled. This extension establishes identity and lifetime;
the subsequent model adds related admission, stopped-domain cleanup and deferred
owning-handler entry. Installed source behavior is unchanged.

The subsequent test-only [sibling-settlement model](../async-execution.md#related-compiled-sibling-settlement-e1d2c4b4d4)
admits related children and keeps the owning continuation until their stopped frames
and resources settle. Nested sibling handlers are suppressed, unrun bodies stay unrun,
and only the owning contingency handles the escaping child fault. A child that handles
its own fault leaves siblings live. Real Rust-owner tests cover repeated normal joins
and fault cleanup while an unrelated root remains available. This harness parks normal
parent execution until its children settle; it is not source concurrency.

The [terminal-report extension](../async-execution.md#owning-terminal-reports-and-host-limits-e1d2c4b4d5)
delivers a failed owning root's report once, preserving the original child cause even
when the owning contingency escalates. Fuel exhaustion with retained children skips
source handlers, settles borrowers and owners, and propagates through related parents
and ancestors with live children. Unrelated roots retain their state until selected but
observe the same exhausted shared fuel budget.

Further [active-handler coverage](../async-execution.md#stopping-active-sibling-contingencies-e1d2c4b4d6)
pauses a sibling in its own named or lexical contingency before another child faults.
Its retained owner is released without resuming that handler; the original owning
contingency still receives the triggering child's cause. Nonempty host diagnostic and
contingency state is restored after settlement.

The test-only [process-settlement extension](../async-execution.md#whole-session-native-process-settlement-e1d2c4b4d7)
stops the entire compiled context session after accepted process termination. It retires
all parked roots and child trees, applies resource fault dispositions, suppresses remaining
handler work and clears pending reports before delivering the process status. Further
admission and selection are rejected. This includes entry-owned resources outside the
outermost contingency; its retained obligations must still satisfy the existing checker.

The [compiled readiness protocol](../async-execution.md#compiled-context-readiness-protocol-e1d2c4b4d8)
adds separate session-bound readiness tokens with fresh registration generations.
Only normal started work can wait. Signals change eligibility without executing source
work or delivering completion; explicit harness selection resumes the original context.
Duplicate, stale, crossed or foreign signals are rejected without consuming state.
Domain/process settlement invalidates affected registrations before handler/status
delivery, while unrelated waits survive isolated failure. The original resource logs
and causes are preserved. Shared native integration and source handoffs remain ahead;
no source syntax, scheduler or async adapter is added by this test-only protocol.
