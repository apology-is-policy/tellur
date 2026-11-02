# Direct async execution

[Language reference](../language-reference.md) · [Design and next steps](../async-execution.md) ·
[Execution failures](execution-failures.md)

Compiler 0.105 supports `async control`, explicit synchronous entry with `run`, and
**direct** `await operation(arguments)`. This first profile executes sequentially.
It does not yet create stored tasks, schedule concurrent children, wait for modeled
I/O readiness, or lower to LLVM coroutines.

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

## Contracts and resources

The `async` permission is part of a control's public contract and its checked graph.
Public snapshots expose `asynchronous: true`; changing the permission changes the
snapshot. Trait implementations must match the trait operation's permission exactly.
Generic and imported contracts preserve it. Hovering the callee shows `async
control`; hovering `run` or `await` explains its current execution rules.

Arguments evaluate and transfer in written order. The existing owning, borrowing,
origin and fault-disposition checks still apply. Caller storage remains live until
the child completes. Returned borrows keep their declared dependencies after await;
await does not make the source available for mutation while such a borrow is live.
Direct call-duration temporary borrows retain their existing restrictions. This is
not permission to capture a temporary in a future stored task.

Faults propagate through the existing emergency channel and nearest enclosing
boundary. They never become a normal `failed` or `cancelled` outcome. Public and
trait controls still declare `faults` when necessary. Async invocation is unavailable
in contingency code in this bounded profile; async controls cannot be `@contingent`.

## Transformations and current limits

The reference interpreter retains explicit call continuations for both operations.
Its host instruction quotas can pause execution between instructions; these pauses
are not source await points or readiness scheduling. The native backend executes the
same bounded profile with sequential calls and stable caller storage.

Path expansion retains `run` and `await` as explicit calls. It may inline ordinary
synchronous calls around them, but cannot erase the async boundary. Expansion
certificates check this distinction. Protocol analysis currently rejects these
boundaries rather than treating them as ordinary unobservable decisions.

Stored `defer`, `await pending`, `task<...>`, `spawn`, scheduling, cancellation and
async host adapters remain unimplemented. Cold-task capture ownership and retained
loans are the next checkpoint. The accepted future task rules require await or
ownership transfer even before a task starts; normal dropping, implicit detach and
cancellation by destruction are not permitted. See the [design plan](../async-execution.md).
