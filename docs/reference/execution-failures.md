# Execution failures and activation reports

[Language reference](../language-reference.md) · [Process entry](process-entry.md) · [Boundary design](../faults-and-activations.md)

A declared outcome, even one named `failed`, is ordinary completion. A source
fault stops the current entry invocation, including all its callers. Ordinary
controls cannot catch it and continue. Explicit isolated boundaries are planned;
there is no catch syntax or isolated request/task boundary in compiler 0.62.

## Source faults and permissions

`fault expression;` evaluates a `string` message and terminates the boundary.
The message is diagnostic information, not an outcome payload. No value is returned,
no caller resumes, and effects already performed are not rolled back.

<!-- tellur:check -->
```tellur
control fail(string reason) faults {
    fault reason;
}

pub control check(bool valid) exits accepted faults {
    delta valid { true => good, false => broken }

    [good]: exit accepted;
    [broken]: fail("the invariant does not hold");
}
```

`faults` is a permission separate from the normal `exits` contract. A control may
permit faults even if its current body always completes normally. A public control
or trait operation must declare this permission if its body can fault, directly or
through another control. Private controls infer and propagate it. Implementations
inherit their trait operation's permission; they cannot add permission that the
trait did not grant. Generic definitions are checked against their bounds before
specialization, so an unconstrained owned type cannot simply be abandoned by faulting.
There is no generic fault-release capability bound yet; use a concrete resource
contract or complete/transfer the abstract owner before faulting.

A control that has **no normal outcomes** omits `exits` and writes `faults`, as
`fail` does above. Its call can be a statement; that call terminates its selected
history. `forward fail(reason);` also works. `capture fail(reason)` is rejected:
there is no completed result to store. A private `exits auto` control may infer an
empty normal boundary from a fault-only body. Omitting `exits` without `faults`
is not valid syntax. Ordinary statements still require all selected histories to
be live: work following a fault on that same history is rejected.

A flat delta arm may terminate directly:

<!-- tellur:check -->
```tellur
pub control check(bool valid) exits accepted faults {
    delta valid {
        true => exit accepted,
        false => fault "the invariant does not hold"
    }
}
```

`fault` is a statement or a flat-arm termination, not an outcome expression. Inline
outcome adapters do not have fault-valued leaves; use a flat decision or a helper
control. A call inside `delta`, `capture` or `forward` can still fault: its normal
outcomes are handled as usual, while its fault propagates out of the invocation.
A stored outcome contains only completed normal results and carries no deferred fault.

The permission describes **source faults**, not a promise against all executor or
host failures. Fuel exhaustion, malformed scripted replies, Rust panics and process
termination have their own existing behavior. No diagnostic-message prefix is used
to infer a fault permission or classify a report.

Try [the source-fault example](../../playground/source_faults.tellur):

```sh
./bin/tellurc exec playground/source_faults.tellur -- -- true
./bin/tellurc exec playground/source_faults.tellur -- --json -- false
```

The first call completes with `done`; the second reports a source fault and returns
status 1. Faults are outside outcome annotations and launcher status mappings. A
fault-only entry has no normal outcomes requiring status mappings.

## Try a bounded failure

From the repository root:

```sh
./bin/tellurc exec playground/execution_faults.tellur -- --fuel 40
./bin/tellurc exec playground/execution_faults.tellur -- --json --fuel 40
```

[The example](../../playground/execution_faults.tellur) deliberately makes one worker
call complete, then invokes the same worker again and loops. The instruction budget
ends execution. Its failure report shows root activation 1 and worker activation 3;
activation 2 has already completed. Loop iterations do not allocate new activations.
The budget is a host limit, not one of the control's declared outcomes.

A generated executable returns status 1 and writes the failure report to stderr.
Normal text mode gives a readable call chain. `--json` or `--scenario FILE` selects
a JSON failure report on stderr; stdout remains reserved for normal JSON completion.
In text mode, output emitted before failure remains visible. Effects performed
before failure are not rolled back, even if no JSON completion is printed.

`--json` applies to failures returned after execution begins, and to result-validation
failures. Native launcher errors before execution, compiler/build errors, malformed
scenario JSON and foreign panics retain their existing diagnostic behavior. Do not
assume every nonzero exit has a JSON failure report. Status 1 can also be a declared
application outcome, as explained in [process entry](process-entry.md).

## Reference executor and editor

The reference interpreter accepts:

```sh
./bin/tellurc run SOURCE MANIFEST SCENARIO [--expanded] [--json]
```

Successful output keeps its existing JSON shape. On execution failure, `--json`
requests the failure envelope on stderr; the command returns nonzero and prints
no successful result. The ordinary reference budget remains one million instructions.
The Rust API `run_detailed(scenario, steps)` exposes both the report and the existing
diagnostic, plus the events observed before termination; `run` and `run_with_limit` retain their diagnostic API.

VS Code's **Tellur: Build and Run** shows native reports in the Tellur Build output.
To run the example with a budget, set:

```json
{"tellurLanguage.programArguments":["--json","--fuel","40"]}
```

Clear those arguments when returning to a different program. Failure reports do not
create static editor diagnostics: they describe one observed execution, whereas
hover/explanation reports describe structural possibilities from checking.

## Reading the report

The envelope is `format: "tellur.execution-failure"`, `version: 1`. It contains:

- `kind`: `source_fault` for explicit source termination. An absent field means
  `returned_error`, preserving the earlier report shape.
- `boundary`: the selected entry.
- `phase`: `entry_validation`, `execution`, or `result_validation`. Entry validation
  is currently exposed by the reference API; native pre-execution errors remain
  ordinary launcher diagnostics.
- `message` and `message_truncated`: the existing diagnostic text, bounded to 4096
  Unicode scalars. The wording is not a typed taxonomy or an input to fault analysis.
- `activations`: up to 32 active invocations, in root-to-leaf order, with invocation
  `id`, `parent`, `control`, `called_at`, and `current_site`.
- `omitted_activations`: an explicit count when the middle of a deep chain is omitted.
  The first 16 and final 16 activations remain; a parent ID may identify an omitted
  activation.
- `site`: the current instruction site, when one exists. Source positions are
  one-based Unicode scalar coordinates. Instruction-budget checks happen before
  evaluating the instruction's guard, so this site does not assert that its effects ran.

Source sites carry a source control, optional file path/label and up to 32 expansion
origins, with an explicit omission count. Paths come from compilation and may reveal
the build machine's source directory. Sources compiled from strings have no file path.
Arguments, payloads and a complete event history are not added to this report.

An activation is an invocation, not a declaration, path label or lexical scope.
Identities restart in each entry invocation. Expanded code can have fewer physical
activations; its source and expansion provenance still identify the original work.
Native optimizer inlining does not change the recorded Tellur graph calls. Reports
are not invariant under source/graph transformation, and instruction fuel is not a
semantic step count. Neither these IDs nor the JSON report are resumable state,
serialized reference addresses, a certificate or a proof of isolation.

## Failure and resource obligations

At every source fault and potentially faulting call, the checker examines each
still-owned must-complete resource on the selected histories. Every such resource
must permit fault cleanup. A caller's retained resources count even when the fault
originates deep in a callee. Arguments are transferred first: a callee may complete
a transferred resource normally before it faults, without needing a fault disposition
for that already-completed resource. Borrowing an owner does not transfer it.

The first supported disposition is `"fault": "release"` on a manifest opaque,
linear type mapped to a retained Rust owner:

```json
{"name":"connection","kind":"opaque","linear":true,"fault":"release"}
```

The same manifest must map `connection` under `rust.resources`. This is a trusted
adapter contract: ordinary host teardown may destroy that retained owner on a
source fault. The compiler does not infer it from the existence of a Rust destructor.
Without the explicit disposition, a faulting path or call retaining the resource
is rejected with `E_FAULT_RESOURCE`. Records, options and sum payloads retain this
requirement for their contained resources. An unhandled outcome containing only
ordinary droppable data does not prevent boundary termination.

This permission does **not** relax normal completion obligations: returning normally
with an uncompleted resource remains an error. Destruction does not invoke a Tellur
completion control, report successful fallible cleanup, reverse effects, or promise
a particular cross-resource cleanup order. Custom fault protocols and fallible cleanup
are not implemented. The reference interpreter validates the same contracts but uses
scripted resource tokens; it does not execute Rust destructors.

Existing returned executor errors retain their previous host teardown behavior;
they are not newly subject to source-fault permission checking. Rust panics, aborts,
forced termination and allocation failure are not newly intercepted and may prevent
reporting or cleanup. See [retained resource rules](../rust-adapters.md#retained-resources-049).

## Analysis and transformation

Expansion retains fault termination and its source provenance. Tail extraction
rechecks the generated helper and preserves the public fault permission. Public
contract snapshots include `faults: true` and explicit opaque resource dispositions.
Declaration hover and Outline show inferred or declared fault permission.

Structural protocols use a distinct `fault` terminal symbol. A faulted residual is
not a normal completion; `can_fault` and `faulted` are separate from `can_complete`
and `complete`. Inclusion compares terminated structural traces, including faults,
and still ignores values. Finite scenario comparison observes a source fault's full
message and preceding event history, and reports equality of fault permissions
separately. Other executor errors, including fuel exhaustion, remain inconclusive.
Neither analysis proves general behavioral equivalence or termination.

Value storage remains the current sequential store. Diagnostic activation IDs do
not enable recursion, simultaneous invocations, async execution or borrowed values
across suspension. Those require the [activation storage and isolation checkpoints](../faults-and-activations.md#checkpoints-and-decision-points).
