# Async calls and structured tasks

[Language reference](../language-reference.md) · [Design and next steps](../async-execution.md) ·
[Execution failures](execution-failures.md)

Compiler 0.176 supports `async control`, synchronous entry with `run`, direct
`await`, cold tasks with owned and borrowed captures, and a bounded cooperative
scheduler for `spawn` tasks with owned values, direct borrows and nested owned record/option/variant captures/results. Source handoffs occur at `await` and `yield`.
Async host I/O, cancellation and LLVM coroutine lowering remain later work.

## Owned spawned children

`auto child = spawn operation(arguments);` evaluates the arguments now and queues
an independent child of the current async invocation. The body does not run until
an explicit handoff. `yield;` places the caller behind already runnable work;
`await child;` consumes its handle and waits for the declared normal completion.
An already completed child is joined without running it again. Host instruction
quotas pause the selected chain and never introduce a source handoff. Emergency
cleanup retires spawned borrowers before their backing storage, then unwinds suspended
await callers. The owning boundary waits for complete settlement before contingency.

<!-- tellur:check -->
```tellur
async control worker(string name) exits done {
    emit(borrow name);
    yield;
    emit(borrow name);
    exit done;
}

async control startup() exits done {
    auto first = spawn worker("first");
    auto second = spawn worker("second");
    emit("queued");
    yield;
    await first;
    await second;
    exit done;
}

control main() exits done { forward run startup(); }
```

This emits `queued`, `first`, `second`, `first`, `second`. See the runnable
[structured tasks example](../../playground/structured_tasks.tellur) for local
handle moves and parent work between handoffs. Every normal path must await its
children. Handles cannot be copied, cloned, dropped, returned, or carried across a
fault boundary. Local moves preserve the original invocation and spawning domain.
Local inferred wrappers retain the same child obligations: unwrap and await the
handle before a synchronous call that may enter an inline fault boundary.
Checked cold/direct awaits remain available beside wrapped children. Their possible
producer faults still apply at handoffs. There is no detach or implicit await at scope exit.

The source profile accepts direct shared/exclusive borrows of existing reference-free
storage, reference-free owned values, and nested owned records, options and closed variants whose reference leaves
point to reference-free storage. Normal results use the same profile with their
declared origins. References to reference-bearing carriers and reference-bearing
outcome/collection payloads remain gated. Broader reference-bearing
`defer` tasks keep their existing semantics. A spawned body may itself use such
cold/direct awaits and may own further spawned children. Before a parent enters a synchronous
control whose inline call chain may enter a fault boundary, it must join its started
children. Cold/direct awaits remain available beside children when the awaited
producer permits fault settlement at handoffs. Synchronous helpers proved to
avoid inline boundaries may also run while children are live. Their calls, including
constructors and concrete generic/trait calls, retain the same borrowing and fault
rules. A helper without a reached handoff runs uninterrupted. A helper may
borrow parent storage and return a reference with its declared origin. See
[structured helpers](../../playground/structured_helpers.tellur). The proof is
conservative across all structural paths and transitive calls; an abstract trait
contract alone does not promise this behavior. Local work, primitive effects,
further spawns, yields and started joins remain available.
These are current admission limits, not changes to the accepted ownership model.

Spawn also requires every resource owner retained across a producer's handoff to
permit fault settlement. This includes transitive callees, locally acquired owners,
partially moved aggregates and owners retained for a nested contingency. An enclosing
owner's fault stops that work without running its nested contingency. The check is
conservative even when the current caller has no faulting sibling, and an unknown
abstract callee does not establish this permission. It does not add `faults` to the
producer's contract. Sequential cold/direct use remains legal where no spawned
owner can stop it. Complete an owner before handing off, or use a resource type
with a declared fault disposition.

<!-- tellur:reject E_ASYNC -->
```tellur
async control finish(handle h) exits done {
    yield;
    delta close(h) { closed => closed, failed { close_error e } => failed }
    [closed | failed]: exit done;
}
async control startup(handle h) exits done {
    auto child = spawn finish(h);
    await child;
    exit done;
}
```

The manifest's `handle` has no fault disposition. Moving the handoff after normal
resource completion makes the producer admissible:

<!-- tellur:check -->
```tellur
async control finish(handle h) exits done {
    delta close(h) { closed => closed, failed { close_error e } => failed }
    [closed | failed]: yield; exit done;
}
async control startup(handle h) exits done {
    auto child = spawn finish(h);
    await child;
    exit done;
}
```

A fault-capable child requires fault permission at spawn. At handoffs, all live
owners must still permit the possible child fault. An unjoined handle accounts for
both captured owners and every possible retained completion. An escaping child
fault stops the owning boundary, applies resource fault dispositions to stopped
children, and only then runs that boundary's contingency. Stopped sibling nested
contingencies do not run. A child's own boundary can still handle its own fault.
Faults remain separate from normal outcomes.

## Direct borrowed captures

A spawned child may borrow existing storage whose type contains no references.
Shared child captures may overlap; exclusive captures require disjoint places.
The parent can continue using other storage, but every capture loan remains live
until its handle is consumed by `await`, including while queued, after yielding and
after the child has completed. Joining one shared reader does not release another
reader's loan. Local handle moves preserve these obligations. Handles with borrowed
captures remain local bindings; storing them in wrappers is still rejected.

<!-- tellur:check -->
```tellur
async control edit(ref mutable string text) exits done {
    yield;
    deref text = "edited";
    exit done;
}
async control startup() exits done {
    mutable string text = "original";
    auto child = spawn edit(borrow mutable text);
    emit("parent work");
    await child;
    emit(borrow text);
    exit done;
}
control main() exits done { forward run startup(); }
```

Moving `emit(borrow text)` before the join is an `E_LOAN` error. Extra `yield`
statements do not release the loan. Reference arguments are reborrowed unless
explicitly moved; the same origin and authority rules apply to stored references,
concrete generic calls and concrete trait calls. Captures must refer to existing
storage, not a temporary constructor result. The [borrowed spawn example](../../playground/borrowed_spawn.tellur)
combines two shared readers and two disjoint exclusive field captures.

Child faults and instruction-budget exhaustion settle retained borrowers before
supplier storage. Stopped children apply resource fault dispositions without running
their nested contingencies. The original invocation and boundary still own the child;
borrowing does not permit handles to escape or cross an inline boundary.

## Borrowed results from spawned children

A child may return direct shared/exclusive references to reference-free storage
borrowed by its inputs. Declared result origins use the ordinary `from` rules.
The capture loan remains live while the child is queued, running or completed but
unjoined. Consuming `await` adopts the selected result's origins and releases
unrelated capture loans. The result's loan then lasts through its final use.

<!-- tellur:check -->
```tellur
async control lend(ref mutable string text)
    exits ready { ref mutable string value from text } {
    yield;
    exit ready { text };
}
async control startup() exits done {
    mutable string text = "original";
    auto child = spawn lend(borrow mutable text);
    yield;
    yield;
    delta await child { ready { auto value } => ready }
    [ready]:
        deref value = "through result";
        emit(value);
        text = "available after result";
        exit done;
}
control main() exits done { forward run startup(); }
```

Accessing `text` before `emit(value)` would conflict with the live result loan,
even though the child has already been joined. Outcome alternatives retain only
their declared result origins; same-producer task instances keep separate captures.
Explicitly moving an exclusive reference into a child still consumes the original
reference. Generic forwarding, `capture await` and inline result adapters preserve
these relationships. A result cannot borrow a child's owned input or local storage.
See [borrowed spawned results](../../playground/borrowed_spawn_results.tellur).

Completed but unjoined results remain retirement obligations. Sibling faults settle
them before supplier storage and the owning contingency. Owned result records
may contain nested record fields and direct reference leaves as described below. Aggregate storage of borrowed
task handles remains outside this profile.

## Owned record captures and results

Owned record fields may nest through owned records, options and closed variants, with shared/exclusive
reference leaves pointing to reference-free storage. Other fields may hold
reference-free values, including resources whose existing fault dispositions permit
stop settlement. References to carriers and reference-bearing outcome/collection fields
remain outside this profile.

<!-- tellur:check -->
```tellur
struct view['text] { ref mutable string text <- 'text; }
async control edit(view input) exits ready { view[input['text]] value } {
    yield;
    deref input.text = "edited";
    exit ready { input };
}
async control startup() exits done {
    mutable string text = "before";
    view input = view { borrow mutable text };
    auto child = spawn edit(move input);
    delta await child { ready { auto value } => ready }
    [ready]:
        emit(value.text);
        text = "released";
        exit done;
}
control main() exits done { forward run startup(); }
```

The ordinary Copy rule still applies: a record consisting of shared references can
be copied, even with an explicit `move`. A record with an exclusive reference moves
into the child. Each captured leaf retains its origin, loan and any carrier-storage
guards through queued, running and completed-but-unjoined states. Local handle moves
do not release those obligations. Shared siblings keep independent loans; disjoint
exclusive fields may be captured by different children.

Join adopts the selected result field origins and releases unrelated captures.
A returned exclusive reference still prevents access through its supplying parent or
guarded carrier until final use. Generic forwarding and imported contracts preserve
these relationships. A result can combine returned owned fields with external borrows;
its own moved input or local storage cannot supply an escaping reference. Shared result
records follow ordinary Copy rules; exclusive records transfer their permissions.
Field-sensitive final use releases each leaf loan independently. A shared reference to
a result record cannot mutate an exclusive field through shared authority.

See [flat record captures](../../playground/carrier_spawn.tellur) and
[flat record results](../../playground/carrier_spawn_results.tellur), and
[nested record payloads](../../playground/nested_spawn.tellur). Borrowed carriers,
reference-bearing outcome/collection payloads and aggregate wrappers of borrowed
handles remain gated. Completed unjoined result records remain obligations and settle
before supplying storage on an owning fault, including owned resource fields.

Nested records preserve the full field path of each capture and result loan. Moving
a subrecord or an individual exclusive field transfers that path's permission;
unmoved siblings retain their own loans. Shared nested records retain ordinary Copy
behavior. A selected result may project one nested leaf and release unrelated captures
at join, including through generic forwarding and imported contracts. Moving an owned
resource out of a carrier before entering an isolated boundary still follows the
ordinary boundary rules; nesting does not grant access to outer references.

<!-- tellur:check -->
```tellur
struct cell['a] { ref mutable string text <- 'a; }
struct request['a] { cell['a] item; }
async control relay(request input) exits ready { request[input['a]] result } {
    yield;
    exit ready { input };
}
async control startup() exits done {
    mutable string text = "before";
    auto child = spawn relay(request { cell { borrow mutable text } });
    delta await child { ready { auto result } => ready }
    [ready]:
        auto item = move result.item;
        deref item.text = "after";
        emit(item.text);
        text = "released";
        exit done;
}
control main() exits done { forward run startup(); }
```

## Owned option and variant payloads

`spawn` can capture and return owned options and closed variants, including inside
records and other admitted tagged values. Each reference leaf still points to
reference-free storage. Capture retains each selected tag's loans through queueing,
execution and unjoined completion. Join adopts the declared result origin groups;
a later `delta` selects their tag-specific permissions. Empty alternatives carry no
reference leaves. Distinct origin groups remain independent; a contract that merges
suppliers into one group remains conservative.

Shared payloads follow ordinary Copy rules. Exclusive payloads transfer their loans;
local handle moves and payload extraction do not create another authority. Detached
exclusive references keep their parent and carrier-storage guards after being wrapped
in an option or variant. Owned resources in completed tagged results remain retirement
obligations and settle before the owning contingency on a sibling fault.

<!-- tellur:check -->
```tellur
async control relay(option<ref mutable string> input)
    exits ready { option<ref mutable string>[input['source]] result }
{
    yield;
    exit ready { input };
}
async control startup() exits done {
    mutable string owner = "before";
    auto child = spawn relay(some { borrow mutable owner });
    delta await child { ready { auto result } => ready }
    [ready]: delta result { some { auto text } => present, none => absent }
    [present]: deref text = "after"; emit(text);
    [present | absent]: owner = "free"; exit done;
}
control main() exits done { forward run startup(); }
```

See [tagged spawned payloads](../../playground/tagged_spawn.tellur) for generic
variant alternatives, empty payloads, nested record capture and selected result
origins. Reference-bearing named/completed outcomes as task payloads remain a
separate audit; ordinary `capture await` and outcome adapters keep working.
Borrowed carriers, reference-bearing collections and borrowed-handle wrappers remain gated.

## Parent awaits beside children

A parent may await a cold task or invoke an async control directly while its owned
children remain unjoined. The awaited work may borrow parent storage. Those loans
last through the awaited completion, and the existing ownership rules still prevent
conflicting access. Spawned sibling captures retain their own checked loans until consuming join.

<!-- tellur:check -->
```tellur
async control worker() exits done { yield; exit done; }
async control edit(ref mutable string text) exits done {
    yield;
    deref text = "edited";
    exit done;
}
async control startup() exits done {
    mutable string text = "original";
    auto child = spawn worker();
    await edit(borrow mutable text);
    auto cold = defer edit(borrow mutable text);
    await cold;
    await child;
    exit done;
}
control main() exits done { forward run startup(); }
```

Each await is an explicit handoff. Sibling completions stay owned until their join.
A sibling fault can arrive before the await dispatches or while its nested work is
suspended. The executor restores the original owning continuation and diagnostic
chain, retires awaited borrowers before supplying storage, and runs only the owning
boundary's contingency. Stopped nested contingencies and normal await delivery do
not run. Quotas do not add handoffs. See the runnable
[parent await example](../../playground/parent_await.tellur).

The awaited producer must permit enclosing stop settlement for its owners across
handoffs, even if the currently known siblings do not fault. This is the same
conservative producer proof used by spawn. Join the started children first when a
sequential producer cannot satisfy it. Fault-capable siblings also require fault
permission for the parent's owners and queued await captures. Parent inline fault-boundary crossing and escaping handles remain
outside this profile.

## Helpers that reach handoffs

A synchronous helper may use `run` to enter async work while its caller owns started
children. `run` stays in the same execution context and does not itself switch tasks.
A reached `await` or `yield` inside that work is still an explicit handoff. The caller
waits for the helper's normal result, including declared borrowed results.

<!-- tellur:check -->
```tellur
async control pause() exits done { yield; exit done; }
control helper() exits done { forward run pause(); }
async control startup() exits done {
    auto child = spawn pause();
    helper();
    await child;
    exit done;
}
control main() exits done { forward run startup(); }
```

The proof follows ordinary calls, `run`, loops, constructors and concrete generic or
trait bodies. An unknown abstract body cannot establish permission. Inline lexical
or named boundaries remain gated even when a structural branch is unreachable.
An awaited control has a separate retained context and may use its already-supported
nested boundaries. See [cooperative helpers](../../playground/cooperative_helpers.tellur)
for parent borrowing and a returned reference.

Every owner retained through the helper's handoffs must permit enclosing stop
settlement. Inputs move synchronously into the helper: it may complete a resource
before its first handoff. Parent owners and possible sibling faults still require
ordinary fault permission. Joining the children first preserves sequential use of
a helper whose retained resources cannot be stopped.

An unrelated child fault restores the context containing the original spawning
invocation, even below several inline helper frames. Those active inline frames
remain in its diagnostic chain; separately awaited work is excluded from that chain.
Affected retained borrowers retire before any inline helper frame releases storage.
Stopped work cannot resume normally or run a nested contingency.

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

Detaching an exclusive reference from a borrowed carrier can also retain a storage
guard for that carrier. Passing the detached reference into a task preserves this
guard through task moves and borrowed completion delivery. A shared reborrow of the
exclusive field still retains the guard. Copying an already shared field can instead
release the carrier while retaining the field's external owner. The runnable
[guarded capture example](../../playground/guarded_task_captures.tellur) demonstrates
both cases, including a detached reference returned by an ordinary control.

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

## Transformations and runtime development record

The versioned notes below record earlier runtime checkpoints. Current source
scheduling and its admission limits are specified in [owned spawned children](#owned-spawned-children).

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
separate integration work in that checkpoint; see [native result adoption](../async-execution.md#shared-native-result-adoption-e1d2d3b).

Since 0.130, native continuation, context/completion and fault-domain machinery is
embedded in ordinary native support. The compiled probes exercise that same code,
with test schedules kept separate. Its explicit context API preserves one-use joins,
result ownership, readiness identities and fault/process cleanup. Default native
entries still run synchronous generated control flow; invocation queue/wait wiring,
resumable source lowering and source task handoffs remain unimplemented. See
[shared native retained execution](../async-execution.md#shared-native-retained-execution-e1d2d3c).

Since 0.131, the reference and native support share one internal task registry
protocol. Retained compiled activations have separate invocation owners, including
nested calls; normal completion requires their task obligations to be settled and
emergency retirement invalidates only their obligations. Native task-payload/queue
binding and retained contexts are described below; source handoffs remain ahead. These internal identities do not
change source lifetimes, cold lexical moves or public contracts. See
[shared task ownership](../async-execution.md#shared-task-protocol-and-native-invocation-owners-e1d2d3d).

Since 0.132, internal native APIs can bind cold captures to their original host,
invocation, task and compiled producer. Queue/wait admission and FIFO capture dispatch
preserve values on rejection. Default source generation still uses synchronous
unbound tasks; internal retained context integration is described below.
The synchronous helper cannot consume bound payloads. Cold lexical movement and
source contracts are unchanged. See
[bound native payloads](../async-execution.md#bound-native-task-payloads-e1d2d3e).

Since 0.133, internal queued native tasks can enter retained child contexts after
admission preflight. Their original parent invocation and completion wait stay paired
through readiness and one-use result adoption; fault/process cleanup retires the
obligations before suppliers and contingency. Source callbacks still use synchronous
lowering, so this does not enable source scheduling. See
[task-bound native contexts](../async-execution.md#task-bound-native-contexts-e1d2d3f).

Since 0.134, retained native scopes retire bound cold obligations carried by values
discarded during fault cleanup or recovery. Normal scope exit/restart rejects pending
discard. Moved handles remain live, and borrowed captures do not retire their suppliers.
Resource dispositions remain responsible for captured resources. This is preparation
for source callback integration, not a new discard or scheduling feature. See
[scoped native task retirement](../async-execution.md#scoped-native-task-retirement-e1d2d3g).

Since 0.135, internal sequential cold-await contexts carry the original diagnostic
chain through nested waits, normal join and owning-boundary fault settlement. They
preserve synchronous fault reports and result ownership. Mixed independent-child trees
and emitted callback handoffs remain outside this internal slice; source execution
and the accepted ownership/fault rules are unchanged. See
[sequential native await contexts](../async-execution.md#sequential-native-await-contexts-e1d2d3h).

Since 0.136, retained native callbacks have an explicit await handoff separate from
quota pauses. Queued captures survive rejected admission, and the original callback
receives an adopted result once without additional source fuel. Fault/process cleanup
retires pending captures and results before suppliers. Generated source callbacks and
source scheduling remain ahead; see
[retained native await handoffs](../async-execution.md#retained-native-await-handoffs-e1d2d3i).

Since 0.137, generated compiled probes use bound `defer` captures and explicit await
handoffs with retained child contexts. Entry driving returns the original diagnostic
chain and untruncated error for final reporting. This probe checkpoint preceded the
production promotion in 0.138; no source scheduler is enabled. See
[generated retained await callbacks](../async-execution.md#generated-retained-await-callbacks-e1d2d3j).

Since 0.138, ordinary native builds use the same retained compiled callbacks and
sequential entry driver. Calls, constructors, loops, boundaries and cold awaits retain
frames, loans, source fuel and diagnostic chains through completion and cleanup.
Executable entry eligibility is unchanged. Probe quota/schedule controls are absent
from normal executables; source execution remains sequential. See
[production retained callbacks](../async-execution.md#production-retained-native-callbacks-e1d2d3k).

Since 0.139, the native host also permits an independently admitted child to perform
exclusive cold awaits. Its diagnostic chain remains one child cause across nested
awaits; result adoption and fault/process resource retirement preserve ownership.
This is an internal integration slice. Parents with other live children cannot begin
a cold await, and source `spawn`/`yield` and scheduling remain gated. See
[independent-child cold awaits](../async-execution.md#independent-child-cold-awaits-e1d2d3l).

Since 0.140, an already-prepared native cold await can be admitted alongside live
independent children. Sibling fault settlement restores the original owning diagnostic
segment, including when awaited work is inside a nested lexical or named boundary.
Stopped frames do not become the fault cause; original resource dispositions still
run before the owning contingency. Parent execution stays blocked until all child
contexts/results settle. Source scheduling remains gated. See
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

Escaping tasks, a public `task<...>` spelling, nested borrowed task storage, borrowed
spawn payloads, cancellation and async host adapters remain unimplemented. No implicit
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
