# Contingency and supervised boundaries

[Language reference](../language-reference.md) · [Source faults](execution-failures.md) · [Design](../faults-and-activations.md)

Compiler 0.66 implements synchronous, owned-input isolation for explicit source
faults. A fault abandons the affected execution's normal continuation permanently.
A contingency can satisfy outstanding external obligations, but cannot turn that
fault into an outcome of the failed work.

## Lexical boundaries and supervised recovery

Compiler 0.66 adds a lexical form. This is the recommended form for new code;
[named boundaries](#declaration-and-invocation) remain compatible.

<!-- tellur:check -->
```tellur
control work(bool broken) exits done {
    delta broken {
        true => fault "request invariant failed",
        false => exit done
    }
}

control main() exits done {
    supervised loop requestor(mutable bool first = true) {
        bool broken = first;
        first = false;
        string context = "request context";

        fault boundary {
            work(broken);
            emit(context);
            exit done;
        } contingency [context] {
            emit(context);
            emit(borrow @fault.message);
            recover again requestor;
        }
    }
}
```

`fault boundary` encloses ordinary work. Its immediately attached `contingency`
encloses emergency work; the brackets list the whole owned values retained at
entry. Here the first request faults, the contingency reports it, and recovery
starts another iteration with `first` still `false`. The second request returns
`done` normally. No branch receives a successful result from the failed request.

The body has a lexical value and path scope. On normal completion it may fall
through to the next statement, or use ordinary `exit`/`again`. Locals and decision
labels declared inside do not escape. On a source fault, ordinary invocations and
work-local storage retire before contingency starts. The handler cannot fall
through, return an ordinary outcome, use ordinary `again`, or establish fresh
protected work. Each live history ends with `recover again`, `escalate`, another
fault, or authorized `terminate process`.

`recover again name;` must name the enclosing **supervised tail loop**. It takes
no arguments. The compiler preserves that loop's parameters as they stood at
boundary entry; the handler cannot replace them. Update counters or other
supervisor state before entering the boundary. Every restart clears the old
iteration's decisions and request locals. As with ordinary loops, lexically nested
loops are not supported yet.

Loop parameters and values from outside the loop remain outside protected work.
This first implementation rejects even shared access to them from the boundary:
prepare owned request inputs beforehand. Outer reference values and aggregates
containing references cannot enter the work. Borrows created inside the work may
refer to its owned inputs and retained owners; they end before handover.
Contingency can access only its captures, local values, and `@fault`; earlier path
labels and ordinary locals are unavailable. Owning a capability does not imply
rollback of its external effects.

Captured owners must exist and be fully available at entry and at every reachable
faulting operation. They cannot be supervisor state or values containing
references. Other abandoned must-complete resources need their declared fault
disposition. Before recovery or process termination, contingency must complete
its retained and locally acquired must-complete resources. A secondary fault or
`escalate;` reaches the next outer boundary; the same handler never handles its
own failure. Escaping faults also check dispositions for supervisor resources
that would now be abandoned.

## Structured fault information

`@fault` has type `ref fault_report` within lexical contingency. It is an immutable
borrow of a compiler-provided record with these fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `message` | `string` | The source diagnostic message |
| `stack` | `vector<string>` | Bounded observed control/activation identities |
| `sites` | `vector<string>` | Bounded current source sites and selected labels |

Use `emit(borrow @fault.message);`, or pass `@fault` to a contingent helper taking
`ref fault_report`. The field layout is reserved. The stack/sites are diagnostic
summaries, not a complete execution history or a stable serialization protocol.
The reference cannot outlive contingency or be exported on recovery. The native
JSON failure report retains its richer cause chain and source metadata.

## Entry-only process termination

<!-- tellur:check -->
```tellur
control main() exits done {
    fault boundary {
        fault "startup invariant failed";
    } contingency [] {
        emit(borrow @fault.message);
        terminate process 23;
    }
}
```

`terminate process N;` accepts a literal status from 0 to 255. It is permitted only
in the executable entry's outermost lexical contingency. A control containing it
becomes **entry-only**: ordinary invocation is rejected, including through imports
or generic specialization. Public contract snapshots and declaration hover expose
this restriction. This is not a transitive permission granted to library calls.

Process termination carries no outcome tag or payload. Native execution and
`tellurc run` return the status. JSON uses `termination: "process"`, `status`, and
`events`, with no `outcome` or `fields`. The reference library's `run_completion`
API returns `RunCompletion::Outcome` or `RunCompletion::Process`; the older
outcome-only APIs report `E_PROCESS_EXIT` when used on an administrative result.
That API mismatch is not a Tellur source fault. Scenario comparisons observe the
termination status separately from ordinary outcomes.

## Compatibility example: named boundaries

<!-- tellur:check -->
```tellur
@contingent
control report(string reason) exits done {
    emit(reason);
    exit done;
}

boundary request(bool broken, string context) retains context {
    delta broken { true => failed, false => healthy }

    [failed]: fault "request invariant failed";
    [healthy]:
        emit(context);
        terminate;
}
contingency(string reason) {
    emit(context);
    report(reason);
    terminate;
}

control main() exits done {
    supervise request(true, "first request");
    supervise request(false, "second request");
    exit done;
}
```

The first request faults. Its normal continuation is discarded and its retained
`context` moves into a new contingency activation. The contingency emits that
context and the diagnostic, then terminates. The supervisor starts the second
request, which completes normally. `main` finally produces its own `done` outcome.
There is no outcome representing the first request's fault.

## Declaration and invocation

A boundary is a named execution root:

_Context fragment: provide an application response-channel type and bodies with complete resource dispositions._

<!-- tellur:fragment application response-channel type and complete bodies required -->
```tellur
pub boundary request(string input, response_channel channel)
    retains channel
{
    // Ordinary request work, followed by terminate on every normal history.
}
contingency(string reason) {
    // Retained channel and diagnostic reason are available here.
}
```

The `contingency` body immediately follows its boundary. It takes exactly one
`string` parameter for the source diagnostic; the parameter name is chosen by the
author. `retains` lists distinct owning parameters, in handover order. It can be
omitted when nothing needs to survive the failed execution.

`supervise request(arguments);` is a statement, not an expression. It evaluates and
transfers the arguments, runs the boundary, and waits until normal work or
contingency completes. It exposes **no outcome, payload, boolean status or captured
fault**. Only then can the supervisor execute its next statement. A boundary
cannot be called through `delta`, `capture`, `forward`, a call statement, or the
executable entry interface. Start an ordinary entry control that supervises it.

`terminate;` ends the current boundary body or contingency administratively. It
does not exit the process and does not create a Tellur outcome. It is rejected in
ordinary controls. Every live history in either body needs an explicit terminal
disposition; normal `exit` cannot invent a boundary result. Flat path selection
still persists until the next label: use `[entry]: terminate;` to finish all live
histories after several selected diagnostic statements.

## Retained owners and retirement

Boundary inputs must own their data. Direct references, records containing
references and borrowed collections cannot cross this first isolation interface.
Borrow ordinary boundary-local values while doing the work, but do not pass an
outer activation's loans into a child boundary. This keeps parent storage outside
the failed child's authority. Owned foreign capabilities can still affect external
systems: ownership does not imply rollback or prove their protocol state safe.

At each reachable source fault or fault-permitting call, the checker requires:

- Every retained parameter remains wholly available. Moving it or a field before
  that point is rejected. It may be consumed on a normal path once no later fault
  can need it.
- Other live must-complete resources have their explicit fault dispositions, as
  described in [source faults](execution-failures.md).
- Calls that may let a contingency fault escape satisfy the supervisor's own fault
  permission and resource checks.

On a source fault, child invocations stop and their ordinary storage retires.
Retained owners transfer exactly once, after the failed work's loans have ended.
The contingency receives only those owners and its diagnostic, not arbitrary
locals, the original request continuation, or hidden ambient dependencies.

The native bridge releases retained Rust objects covered by fault release during
frame/boundary teardown. It protects surviving parent objects. It does not call
fallible completion operations implicitly, undo external effects, or claim those
operations succeeded. The contingency must explicitly complete or transfer each
must-complete owner before `terminate` or `escalate`.

## The checked contingency subset

`@contingent` declares that a control is available during contingency execution.
Normal code can also call it. Public and trait operations require the annotation;
private helpers infer eligibility from their bodies. A contingency body is checked
as contingent without needing its own annotation.

<!-- tellur:check -->
```tellur
trait reporter {
    @contingent
    control report(self value) exits done;
}

implement reporter for string {
    control report(self value) exits done {
        emit(value);
        exit done;
    }
}

@contingent
control report<t: reporter>(t value) exits done {
    reporter<t>.report(value);
    exit done;
}
```

An implementation inherits the trait operation's permission and must satisfy it.
It cannot add permission to an unannotated trait operation. Generic definitions
are checked against their trait bounds before specialization. Passing a resource
through an unrestricted helper cannot evade the check.

Contingent bodies may call contingent operations, pure builtins and collection
operations, and may use local values, mutation, decisions, loops and `emit`.
They cannot supervise new work. Foreign primitives require explicit
`"contingent": true` metadata in the trusted manifest; describing an effect as
empty is not sufficient. Their adapters must implement the promised recovery
protocol, including any state/phase checks and I/O deadlines.

Helper calls can have ordinary outcomes. For example, a permitted emergency write
can return `sent`, `unavailable` or `failed` **inside the contingency**. These
outcomes describe that new operation. They do not complete the abandoned request,
and their paths cannot join its normal paths.

The annotation checks transitive call permission. It does not prove termination,
a time bound, rollback, resource health, or safety of arbitrary foreign code.
An infinite loop remains possible; the HTTP example uses bounded host operations.

## Escalation and public fault permission

`escalate;` is available only in a contingency. After its normal resource
obligations are discharged, it propagates the original fault to the enclosing
boundary. A new `fault "message";` in the contingency propagates a secondary source
fault, preserving the original in the diagnostic causal chain. Neither re-enters
the same contingency. An outer boundary may run its own contingency; without one,
the entry invocation fails.

<!-- tellur:check -->
```tellur
pub boundary request() faults {
    fault "request invariant failed";
}
contingency(string reason) {
    emit(reason);
    escalate;
}

pub control main() exits done faults {
    supervise request();
    exit done;
}
```

A public boundary's `faults` permission describes **escape from its contingency**,
not the faults it handles internally. Omitting it forbids such escape. Private
boundaries infer it. A public supervisor needs `faults` when a supervised boundary
permits escape. Foreign/runtime failures remain outside that source permission.

Handled source faults produce diagnostic fault events. Escaping faults preserve
bounded causal reports, including the original failure when cleanup faults.
Diagnostic activation identities continue increasing across supervised requests.

## Current scope and the settings server

The [settings server](../../applications/settings/server.tellur) retains each
accepted connection. Its contingency calls the trusted `http_reply_failure`
operation, then closes the connection. That operation attempts a fixed generic
500 only before response transmission has started. It reports `unavailable` after
any previous write attempt, including a partial one. Network failure remains
possible and is handled inside the contingency. Request internals are not sent to
the client. The accept loop can serve the next connection afterwards.

Service `completed` and `idle` describe the accept loop's lifecycle. They do not
aggregate request failures. Responses and diagnostic events describe individual
requests; no failure outcome crosses the recovery edge.

This release handles explicit Tellur source faults. Driver errors, fuel exhaustion,
foreign panics, aborts and forced termination do not become recoverable source
faults. There is no blanket Rust panic catcher. Storage is activation-owned but
execution remains sequential: recursion, suspension, async children and LLVM
coroutine lowering are later checkpoints. Call expansion preserves lexical
boundary/handler storage and recovery identities. Compatible named `supervise`
operations also remain isolated; their children are not flattened into supervisors.
Structural protocol analysis currently rejects graphs containing supervision,
and tail extraction does not move boundary/contingency bodies into ordinary controls.
General user-defined execution flags, annotation policies and dependency injection
are deliberately deferred.
