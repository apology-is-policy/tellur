# Async calls and cold tasks

[Language reference](../language-reference.md) · [Design and next steps](../async-execution.md) ·
[Execution failures](execution-failures.md)

Compiler 0.107 supports `async control`, synchronous entry with `run`, direct
`await operation(arguments)` and local cold tasks with owned and direct borrowed captures. Execution
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

`auto pending = defer operation(arguments);` evaluates and transfers arguments now,
then stores an owned task without entering the async body. The declaration must be
an immutable local `auto` binding inside an async control. Owned arguments may contain
strings, collections, records, sums and resources without contained references.
Direct shared and exclusive references to existing reference-free storage are also
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
is consumed by await or its execution boundary faults. `t` must not itself contain
references in this profile. Multiple shared captures may coexist. Exclusive
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

Move the assignment after `await pending;`. Await currently admits only owned,
reference-free completion payloads, so all capture loans end on delivery. Returning
a borrow requires the next result-origin transfer checkpoint.

Bind a temporary owner or reference before deferring work; deferred borrowing does
not adopt a synchronous temporary's lifetime. Tasks with borrowed captures require
unique local binding names and cannot yet be nested inside sums, records or
collections, passed through a control boundary, or returned. Direct reference
values extracted from existing record fields can be captured when their referent
is reference-free; the task retains their established external dependencies.

<!-- tellur:reject E_ASYNC -->
```tellur
async control lend(ref string text) exits ready { ref string value from text } {
    exit ready { text };
}
async control startup(ref string text) exits done {
    auto pending = defer lend(text);
    exit done;
}
```

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
same bounded profile with sequential calls and stable caller storage.

Path expansion retains `run`, `defer`, direct `await` and task consumption as
explicit operations. It may inline ordinary
synchronous calls around them, but cannot erase the async boundary. Expansion
certificates check this distinction. Protocol analysis currently rejects these
boundaries rather than treating them as ordinary unobservable decisions.

Contained reference captures, borrowed results, escaping tasks, a public `task<...>` spelling, `spawn`,
scheduling, cancellation and async host adapters remain unimplemented. No implicit
detach or cancellation by destruction is permitted. See the [design plan](../async-execution.md).
