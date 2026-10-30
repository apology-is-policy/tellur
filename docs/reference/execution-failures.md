# Execution failures and activation reports

[Language reference](../language-reference.md) · [Process entry](process-entry.md) · [Boundary design](../faults-and-activations.md)

A declared outcome, even one named `failed`, is ordinary completion. An executor
failure stops the current entry invocation. Ordinary controls cannot catch it and
continue. The agreed direction is explicit isolated execution boundaries later;
there is no catch syntax or isolated request/task boundary in compiler 0.60.

Compiler 0.60 reports existing **returned** execution failures with the active
control invocations and their source sites. This includes instruction-limit
exhaustion and rejected scripted host replies. It does not introduce source-level
invariant assertions, a `fault` statement or transitive fault declarations yet.

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
diagnostic; `run` and `run_with_limit` retain their diagnostic API.

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

The current native host's ordinary teardown releases retained objects after a
returned execution error. It does not invoke Tellur completion controls, claim that
fallible cleanup succeeded, or reverse already performed effects. An error report
therefore makes no resource-protocol completion guarantee. [Retained resource rules](../rust-adapters.md#retained-resources-049)
continue to apply. Rust panics, aborts, forced termination and allocation failure are
not newly intercepted and may prevent reporting or cleanup.

Value storage remains the current sequential store. Diagnostic activation IDs do
not enable recursion, simultaneous invocations, async execution or borrowed values
across suspension. Those require the [activation storage and isolation checkpoints](../faults-and-activations.md#checkpoints-and-decision-points).
