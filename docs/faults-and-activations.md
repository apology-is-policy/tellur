# Faults, execution boundaries and activations

30 October 2026. Compiler 0.63 implements the first P6c/P6d slice: dynamic
activation storage, named owned-input boundaries, explicit retained-owner handover,
checked contingency permissions and sequential supervision. See the
[normative reference](reference/contingency.md) for syntax and current limits.

## Agreed boundary policy

A declared outcome is normal control completion, including application failures.
A fault permanently abandons the affected execution's normal continuation.
`supervise` starts a named child boundary and waits for lifecycle completion without
receiving any normal/fault result. Its contingency starts only after failed work
retires, with explicitly retained owners and a diagnostic. It cannot return into
that work. Ordinary outcomes of contingency helper operations stay inside that
execution domain. `terminate` settles it; `escalate` propagates the original fault.
A contingency fault escapes outward, with the original preserved as its cause.

This deliberately excludes catch-and-convert wrappers. The supervisor is a
separate continuation; fault paths cannot join the failed request's normal paths.
Public/trait `@contingent` contracts and trusted host metadata restrict transitive
calls. No implicit service locator or general execution-mode flags are introduced.

Keep these situations distinct:

| Situation | Meaning |
| --- | --- |
| `missing`, `invalid_file`, `write_failed` | Ordinary declared alternatives; callers handle them with delta/adaptation/forwarding |
| Explicit invariant failure | Source-level `fault`; terminates its boundary and is visible transitively |
| Host instruction budget exhausted | Executor policy, not a new outcome or a source assertion failure |
| Malformed scripted host reply | Driver/contract rejection; not evidence of a bug in the Tellur source |
| Runtime invariant violation | Compiler/runtime or trusted-host problem; not a domain error to silently classify |
| Foreign panic, abort, allocation failure, forced termination | Outside the ordinary returned-error channel; reporting and cleanup may be unavailable |

The current report carries the existing diagnostic text and execution phase.
It does not claim a complete typed taxonomy or infer a fault effect from diagnostic
message wording. The additive `source_fault` kind distinguishes explicit source
termination; named fault categories remain deferred.

## Resource disposition is separate from completion

The native host stores retained Rust objects by token. Activation and boundary
teardown release the failed child's owners while protecting retained handover and
parent resources. Legacy file handles still have entry-host teardown fallback. It does not call Tellur completion controls,
produce their outcomes, reverse writes, roll back mutated state or establish that
fallible cleanup succeeded. A consumed Rust owner may already have left the host
store; its wrapper is part of the trusted boundary.

A must-complete resource's normal obligation remains explicit. Faults must not be
used to manufacture a successful completion. Source faults require an explicit
fault disposition for each outstanding resource obligation: which objects can be released
by teardown, which require an explicit fault protocol, and which boundaries cannot
be isolated. This is a prerequisite for recoverable child boundaries, not a hidden
promise made by the current Rust destructor fallback.

Cancellation remains a request followed by a declared completion protocol. It is
neither a fault nor frame abandonment. Future task scopes must account for children
and cleanup before their storage is destroyed; a child fault does not make a sibling's
obligations disappear. Termination under failing cleanup needs an explicit policy.

Rust's [`catch_unwind`](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
captures unwinding panics, not aborts, and requires exception-safety reasoning.
The current bridge adds no blanket panic catch. The [Rust panic model](https://doc.rust-lang.org/reference/panic.html)
also permits non-unwinding termination. An isolated Tellur boundary cannot promise
to recover from arbitrary foreign panics simply by wrapping generated code.

## What an activation identifies

An activation is one dynamic invocation of a control. A definition, call site,
path label, lexical scope and loop iteration are different things. Two completed
calls to the same control have different activation identities. A loop's `again`
continues within the same activation. A nested call records its parent activation
and the source site that invoked it.

In 0.60, identities start at one in each entry invocation. They are diagnostic
identities, not addresses, serializable resources, globally unique IDs or values
available to Tellur source. Only active invocations are retained. Completed calls
and loop iterations do not accumulate an unbounded history.

Both executors now allocate invocation-local storage ranges. References identify
an actual activation's storage plus a projection, rather than just a definition's
slot number. Every normal/error return retires that invocation's remaining storage;
a boundary extracts its retained owners before retirement. Native storage uses a
sequential arena with frame offsets; the reference executor allocates distinct
logical ranges. These are implementation addresses, not source-visible lifetimes.
Recursion and concurrency remain unsupported. Async lowering must make suspended
frames persistent and settle children/loans before retirement; native offsets alone
do not provide a scheduler, pinning protocol or coroutine ABI.

## Lift and extraction

Source transformations preserve the declared semantic boundary and external effect
order. Invocation counts in the executed Tellur graph and instruction fuel consumption are diagnostic
implementation details; they need not remain identical after expansion. A report
must not invent a runtime invocation that no longer exists after inlining.

The 0.60 report therefore distinguishes the observed activation chain from the
source provenance of an expanded instruction. The source control, named site and
bounded expansion chain remain visible even when the instruction executes inside
one expanded root activation. Fault effects and boundary crossings must become
explicit constraints of later transformation/certificate work. No new fault
preservation theorem is claimed for source constructs not yet implemented.

## Async and LLVM remain frontend-owned

The chosen boundary model permits later explicit isolated tasks without requiring
local exception recovery. Tellur must define suspension, resumption, cancellation,
cleanup and child ownership first. LLVM supplies lowering mechanisms for a checked
state machine, not the language's resource protocol.

For switched-resume coroutines, LLVM's [`coro.destroy`](https://llvm.org/docs/Coroutines.html#llvm-coro-destroy-intrinsic)
operates on a suspended coroutine. Tellur must arrange legal frame destruction after
its own disposition rules have been satisfied. This is a design inference from the
LLVM mechanism, not a claim that the intrinsic performs Tellur cleanup. LLVM's
[coroutine documentation](https://llvm.org/docs/Coroutines.html) warns that release
compatibility is not guaranteed, so native coroutine experiments need a pinned
toolchain. The current native backend still generates Rust and uses its LLVM backend.

## Agreed resource policy

Source faults require an explicit fault-disposition contract for every live
must-complete resource. The compiler rejects a faulting path or a call that may
fault while an outstanding resource lacks that permission. This check is transitive:
a resource held by a caller still constrains a fault raised by a callee.

Best-effort host teardown does not itself grant permission to abandon a resource's
normal completion obligation. Compiler 0.62 supports explicit `"fault": "release"`
on manifest opaque linear types mapped to retained Rust owners. The host destroys
those owners on ordinary boundary teardown; this does not claim normal protocol
completion or rollback. Compiler 0.63 adds explicit boundary-owned contingency
cleanup; arbitrary deep-local registration and custom automatic dispositions remain future work. See the
[executable reference](reference/execution-failures.md) for the complete rules.

## Checkpoints and decision points

The author chose one source-fault channel carrying a diagnostic message. Compiler
0.62 spells termination `fault "reason";` and adds `faults` to public and trait
control signatures, such as `control check(...) exits valid, invalid faults { ... }`.
Private controls infer and propagate permission. Fault-only controls omit `exits`:
`control fail(string reason) faults { fault reason; }`. Named typed fault categories
are deferred. Reports retain the 4096-scalar message bound; finite scenario comparison
uses the full diagnostic message and preceding event history.

1. **P6a, compiler 0.60:** record this boundary policy, audit current teardown,
   report existing returned execution failures, and track observed activations in
   both executors. Keep normal outcomes/status mappings unchanged.
2. **P6b, delivered in 0.62:** source termination and transitive fault permissions,
   fault-only controls, explicit retained-owner fault release, editor/contract
   visibility and separate fault terminals in protocol/comparison evidence. No-fault annotations and
   faulting arithmetic/indexing sugar are separate decisions.
3. **P6c, sequential slice delivered in 0.63:** invocation-local storage and
   activation-relative addresses, normal/error retirement, retained handover and
   repeated/nested-call regression coverage. Persistent suspended frames remain E1 work.
4. **P6d, owned synchronous slice delivered in 0.63:** named source boundaries,
   outcome-free supervision, checked contingent calls/traits, terminal escalation,
   causal reporting and bounded emergency HTTP response/close. Only owned inputs
   are admitted; borrowed state, async children and broader capability admission
   need separate contracts.

The settings server now supervises each accepted connection independently. Its
restricted emergency write checks response phase, attempts a generic 500 only
before transmission starts, and always proceeds to explicit connection close.
The supervisor's completed/idle status describes service lifecycle, not a child
fault converted to an ordinary result.

General user-defined annotation policies and execution flags are pinned for later
research. Keep contingency specific until the application supplies evidence for a
larger effect/typestate system.
