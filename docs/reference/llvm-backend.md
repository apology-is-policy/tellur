# Direct LLVM backend

Compiler 0.235 introduces `--backend llvm` for `build` and `exec` on macOS arm64.
Compiler 0.236 adds synchronous calls between scalar controls, including imported
private helpers, implicit single-completion calls and identity-routed outcomes.
Compiler 0.237 adds ordinary owned records with scalar leaves, including nested,
empty, imported and concrete generic records. Compiler 0.238 adds options, closed
variants, named outcome values and their decisions/captures with recursively owned
payloads. Compiler 0.239 adds synchronous primitive calls with owned arguments
and supported payload types. Compiler 0.240 moves locals into stable runtime activation storage.
Compiler 0.241 adds direct shared/exclusive references and borrowed decisions.
Compiler 0.242 adds reference-bearing aggregates and whole borrowed carrier results.
Compiler 0.243 adds opaque/resource values and synchronous borrowed host parameters.
Compiler 0.244 adds arrays, vectors, maps and synchronous collection operations.
Compiler 0.246 adds synchronous tail loops and expanded loop completions.
Compiler 0.247 adds synchronous declared constructors with direct destination delivery.
Compiler 0.248 adds named synchronous boundaries and contingency settlement.
Compiler 0.249 adds lexical boundaries, supervised recovery and process termination.
The default remains `--backend rust`. Both start from the same checked program;
backend selection does not relax ownership, origins, outcomes or path rules.

```sh
./bin/tellurc build playground/llvm_scalars.tellur playground/manifest.json --backend llvm --out /tmp/tellur-scalars
/tmp/tellur-scalars true 9 Ada
./bin/tellurc exec playground/llvm_scalars.tellur playground/manifest.json --backend llvm --release -- false 12 Ada
```

The first invocation exits with status 0; the second selects `declined` and exits
with status 7. Both report the declared outcome and its payload. Native `--json`,
`--scenario`, `--entry`, `--help` and `--fuel` retain their existing behavior.

## Current admission

The current slice accepts synchronous controls using `bool`, `u32`, `string`,
`bytes`, `unit`, opaque/resources, arrays, vectors, maps, records, options, closed variants and named outcome values
recursively composed of those types: declarations, moves/copies, assignment, pure expressions,
value decisions, synchronous control/primitive calls, path guards, lexical releases,
emit, faults, tail loops and final exits.
It also handles expansion completions, including returns from looping helpers. Admitted parameters and
outcome payloads use the same launch validation and process-status mappings as
the Rust backend. Multiple eligible executable entries are supported.

Direct references and reference-bearing aggregates compose recursively; shared/exclusive
parameters, borrowed results, carrier reborrows and indirect fields are supported.
Task operations and adapters remain outside this profile. Launch signal policies are also rejected. Admission checks all checked
regions, including unused helpers. Unsupported programs receive `E_BACKEND`
before the backend creates its output directory or files; ordinary checking may
still succeed, and the Rust backend remains available.

## Calls and payloads

Each admitted control is emitted as an LLVM function. Its callback borrows the
current host for one synchronous invocation and returns an opaque outcome handle.
The selected delta arm transfers all payload fields into its caller; unselected
arms do not consume that outcome or publish payloads. Calls evaluate arguments
once in written order. Ownership and outcome coverage remain checker rules.

Nested calls preserve activation identities, caller locations and instruction
fuel accounting. A failed helper keeps its activation chain in the ordinary fault
report and stops subsequent caller effects. Calls themselves introduce no new
scheduling handoff, suspension or cancellation behavior. Recursive controls remain
rejected by the language checker.

The [nested-call example](../../playground/llvm_calls.tellur) includes private
helpers, scalar payloads and identity routing:

```sh
./bin/tellurc build playground/llvm_calls.tellur playground/manifest.json --backend llvm --out /tmp/tellur-calls
/tmp/tellur-calls true 9 Ada
/tmp/tellur-calls false 12 Ada
```

The first invocation reports `accepted` with status 0; the second reports
`declined` with status 7. The first also emits the number; both emit the greeting.

## Declared constructors

Synchronous [constructors](construction.md) deliver a completed record directly into
a validated caller local. They do not package an outcome tag or payload vector.
Partial fields remain in the constructor frame until completion; faults retire that
frame without publishing a partial value. Nested/generic/imported calls preserve
written argument order, short-circuiting and ordinary source activation names.

Reference-bearing constructor results retain their declared caller origins and use
the same checked evidence as raw records and control results. The destination's
storage identity is validated before argument transfer and delivery. It grants no
new borrowing permission. Task operations remain outside the admitted profile.

The [constructor example](../../playground/llvm_constructors.tellur) constructs an
owning message and a borrowed view:

```sh
./bin/tellurc build playground/llvm_constructors.tellur playground/manifest.json --backend llvm --out /tmp/tellur-constructors
/tmp/tellur-constructors false
/tmp/tellur-constructors true
```

The first invocation emits `Hello, Ada` and returns the message. The second faults
after initializing its field, before any result reaches the caller.

## Named synchronous supervision

Since 0.248, `supervise` invokes a named boundary through compiled work and
contingency callbacks. They use administrative settlement, with no outcome tag,
payload or fault value delivered to the supervisor. After settlement, only the
supervisor's own continuation resumes; faulted work remains abandoned.

On an explicit source fault, retained owners transfer in declaration order after
ordinary child frames and their loans retire. Existing runtime scope rules release
abandoned resources before contingency begins and preserve unrelated caller owners.
Normal settlement, escalation and secondary faults finalize the scope exactly once.
A handler's failure cannot re-enter that same handler. Fuel, driver and internal
errors bypass contingency; they do not become source faults.

Original and expanded programs preserve private/imported helper names, activation
causes, path locations and fuel. Contingent helper calls, ordinary loops and admitted
constructors compose with named supervision. The source isolation/retention rules
remain unchanged. Lexical isolation and supervised recovery are described below.

The [supervision example](../../playground/llvm_supervision.tellur) runs two requests:

```sh
./bin/tellurc build playground/llvm_supervision.tellur playground/manifest.json --backend llvm --out /tmp/tellur-supervision
/tmp/tellur-supervision false
/tmp/tellur-supervision true
```

Both complete the supervisor with `done`. The second invocation handles the first
request's source fault, reports its retained context and reason, then starts the
independent second request.

## Lexical boundaries and supervised recovery

Since 0.249, `fault boundary` and its attached contingency share the enclosing
control's stable frame and checked path facts. Normal fallthrough, ordinary exits,
loop restarts and expanded helper completions finalize every lexical scope they
cross. Jumps that stay inside a boundary preserve that scope.

Only an explicit source fault transfers to contingency. Abandoned temporary
borrowers and checked work slots retire before resource release and handler entry.
Retained owners keep their original storage; `@fault` exposes the existing immutable
message, stack and site report. Handler errors finalize the current scope and
propagate outward, preserving escalation and secondary causes. Fuel, malformed host
responses and internal errors bypass contingency.

`recover again` restarts the enclosing supervised loop with the checked supervisor
state preserved at boundary entry. It cannot produce an ordinary result for the
failed work. Entry-only `terminate process N` returns an administrative status with
no outcome tag or payload, including status zero. Source restrictions are unchanged.
All temporary LLVM arrays remain allocated at function entry, including scope
metadata used by repeated iterations.

The [recovery example](../../playground/llvm_recovery.tellur) constructs owned
request context before each iteration:

```sh
./bin/tellurc build playground/llvm_recovery.tellur playground/manifest.json --backend llvm --out /tmp/tellur-recovery
/tmp/tellur-recovery false
/tmp/tellur-recovery true
```

The first invocation completes normally. The second reports the failed request,
retires it and starts a new request with the already-updated supervisor state.
Both finish with `done`. The [contingency reference](contingency.md) defines the
retention, recovery and process-entry restrictions shared by both backends.

## Stable local storage

Each synchronous LLVM invocation has its own checked local layout in the existing
runtime frame allocator. The call tree shares that arena; a local keeps the same
storage identity when its value is replaced or moved and reinitialized. Temporary
value handles and selected outcome handles remain separate one-use transfers.

Child storage retires on completion, fault or failed execution without disturbing
caller locals. Reused offsets receive new allocation identities, so retirement
cannot make an earlier storage identity refer to a later invocation. Fault reports
still retain the failed control's activation chain. Storage operations add no fuel
ticks or scheduling handoffs.

Storage identity does not add an alternative ownership or lifetime rule.

Since 0.245, fully consumed temporary value/outcome tables are cleared at instruction
boundaries while retaining reusable capacity. Monotonic identity bases ensure that
an old handle never names a later value. A live temporary prevents retirement;
counter exhaustion reports an internal runtime failure and cannot wrap an identity.
Outcome capacity is reserved before dispatch or collection mutation, and payload
capacity before a selected outcome transfers its fields. Stable frame owners and
reference evidence are independent of these temporary tables.

LLVM scratch arrays, Boolean expression cells and path-fact storage are allocated
once at each function entry, including at O0. Together these bound temporary backing
storage by the checked instruction/control layout rather than execution length.
Since 0.246, loop backedges reuse this storage.
Runtime sites, fuel charges, effects and failure ordering for valid programs are
unchanged.


## Direct references

Shared and exclusive references can point to the admitted scalar, record and tagged
values, including carriers with reference leaves. Synchronous controls can take and return direct borrows,
including reborrows of fields; returned loans remain tied to their checked caller
origins after callee storage retires. Reads and assignments through references use
the existing runtime place operations. Shared references retain Copy behavior;
exclusive references preserve the checker's move and reborrow restrictions.

Borrowed decisions inspect an owned option, variant or outcome without transferring
its payload owners. Selected payload references keep the original storage and
origin evidence. Borrowed temporary arguments retain their ordinary call lifetime.
Reference formation and checked partial/complete/incoming origin annotations use
the same runtime metadata as Rust generation; incomplete incoming evidence is
never promoted by a callee annotation. Metadata does not grant execution permission.

This slice excludes async host operations and task operations. Backend
admission does not relax lifetime, aliasing, origin or outcome checks.

The [reference example](../../playground/llvm_references.tellur) mutates a record,
returns a shared field borrow, then returns an exclusive reborrow of that field:

```sh
./bin/tellurc build playground/llvm_references.tellur playground/manifest.json --backend llvm --out /tmp/tellur-references
/tmp/tellur-references false
/tmp/tellur-references true
```

Both emit `updated` and `through returned borrow`. The first returns the modified
record with status 0; the second faults with `reference stopped` and preserves
ordinary source and activation provenance.

## Reference-bearing carriers

Records, options, variants and named outcomes may contain shared or exclusive
references, including nested concrete generic shapes. Synchronous controls can
return those values or borrows of whole carriers. Selected origins, holder storage
and contained supplier loans retain their existing checker contracts; empty tags
do not invent a supplier loan or discharge an owning outcome obligation.

Guarded leaf reborrows and whole-carrier input annotations retain the original
holder and its materialized reference leaves. The runtime checks actual holder
paths separately from symbolic origin paths. Metadata stays in immutable module
storage; malformed ABI descriptors fail before consuming their operand, while
unavailable checked evidence remains metadata and cannot grant permission or
promote partial incoming coverage. Callee retirement preserves caller identities.

The [carrier example](../../playground/llvm_carriers.tellur) returns a nested
exclusive carrier and then a leaf, restoring each parent after the child's last use:

```sh
./bin/tellurc build playground/llvm_carriers.tellur playground/manifest.json --backend llvm --out /tmp/tellur-carriers
/tmp/tellur-carriers false
/tmp/tellur-carriers true
```

Both emit `through child` and `through carrier`. The first returns `parent restored`
with status 0; the second faults with `carrier stopped` and source provenance.

## Owned records and fields

Positional and named record initializers evaluate each field once in written
order, then store fields in declaration order. Nested field reads follow the
existing Copy rules; moving an owned field leaves its siblings available.
Assignment can reinitialize a moved field. Projecting an owned temporary transfers
the selected field and drops unused siblings. Lexical release also handles partially
moved records. These are the same checked ownership rules as the Rust backend.

Record values cross helper calls and selected outcomes through opaque handles;
no Rust record layout crosses the C ABI. Constructor batches and field destinations
are validated before consuming transferred owners. The profile checks temporary
constructors as well as declared slots, so nesting an unsupported value in a
record does not bypass admission.

The [record example](../../playground/llvm_records.tellur) replaces a moved nested
field, copies a count before moving its enclosing record, calls two helpers and
projects a temporary:

```sh
./bin/tellurc build playground/llvm_records.tellur playground/manifest.json --backend llvm --out /tmp/tellur-records
/tmp/tellur-records false Ada
/tmp/tellur-records true Ada
```

Both emit `9` and `temporary`. The first returns a parcel containing `Ada!` with
status 0; the second faults with `record stopped` and retains the ordinary source
and activation report.

## Tagged values and decisions

Options, variants and named outcome values preserve their constructor names and
payloads. A value decision consumes a moved value, or a copy where the checker
permits Copy, then routes only the selected alternative's payloads. Empty tags and
nested generic/imported payloads follow the same rules. Captured control outcomes
can be stored as named outcome values and later decided; their must-complete
obligations still come from ordinary checking.

Value decisions and control calls share the one-use outcome routing ABI. Each
matching arm validates payload arity before publishing handles. Unsupported payload
types reject the entire aggregate type, even if the source constructs only an empty
alternative. Boolean decisions use the same runtime routing and native path guards;
this adds no instruction fuel ticks or scheduling handoffs.

The [tagged-value example](../../playground/llvm_tagged.tellur) captures a named
outcome, passes a variant through an option and selects its nested record payload:

```sh
./bin/tellurc build playground/llvm_tagged.tellur playground/manifest.json --backend llvm --out /tmp/tellur-tagged
/tmp/tellur-tagged 0 true false Ada
/tmp/tellur-tagged 1 true false Ada
/tmp/tellur-tagged 2 false false Ada
/tmp/tellur-tagged 0 true true Ada
```

The first emits `42` and returns `delivered` with status 0. The next two return
`skipped` with status 7 via different empty alternatives. The last faults inside
`finish`, retaining the nested activation and source report.

## Synchronous primitives

Returning synchronous primitives can take `value`, `borrow` or `consume` arguments
and return reference-free scalar/record/tagged/opaque payloads. The primitive type profile
remains reference-free even when controls use reference-bearing carriers. Arguments are
evaluated once in written order and their handles transfer as one validated batch.
The whole declared signature must be supported, including unselected outcomes.
Async operations, Rust adapters and unsupported payload types
remain rejected before output creation.

Dispatch uses the existing host runtime. Live builtin contracts, scripted outcomes,
trace events and malformed-script diagnostics are unchanged. A primitive call adds
no control activation, fuel tick or scheduling handoff. Scripted waits still require
an async host operation, which this backend does not yet admit. A manifest-only
operation with no live adapter still requires a scenario when run.

The [primitive example](../../playground/llvm_primitives.tellur) calls `parse_u32`,
`checked_add` and `checked_mul` through a helper. Its additional `decode` entry
uses `decode_utf8`:

```sh
./bin/tellurc build playground/llvm_primitives.tellur playground/manifest.json --backend llvm --out /tmp/tellur-primitives
/tmp/tellur-primitives 12 3 2
/tmp/tellur-primitives 4294967295 1 1
/tmp/tellur-primitives --entry decode '{"kind":"bytes","value":[65,66]}'
```

The first emits and returns `30` with status 0. The second returns `invalid` with
status 7 on addition overflow. The decode entry returns `text` with `AB`; malformed
UTF-8 selects `invalid` with status 7. Scalar CLI arguments and JSON byte arguments
use the same launch validation as the Rust backend.

## Resource ownership and borrowed host operations

Opaque host values, including must-complete resources, can travel through locals,
records, options, variants, synchronous control calls and outcomes. Borrowing a
resource for a host call preserves its owner; a consuming parameter transfers it
on every selected completion, including a declared failure. The checker still
rejects dropping or cloning resource-containing values, unfinished obligations,
and conflicting live borrows. There is no implicit fallible protocol completion.

Borrowed host arguments are lowered to the same checked value reads as in Rust
emission. The ABI transfers that temporary snapshot, while the source owner and
physical host object remain available. Live builtin file operations share the
existing host table: `read` retains the open file and `close` removes it. Scripted
opaque identities, errors and tracing keep their ordinary representation. Invalid
ABI batches reject before host dispatch and value transfer. Host shutdown retains
the existing cleanup behavior on execution failure; this does not declare a
resource successfully closed by its protocol.

The [resource example](../../playground/llvm_resources.tellur) wraps the file owner
in a record, reads through a borrow, closes it, then returns bytes or faults:

```sh
./bin/tellurc build playground/llvm_resources.tellur playground/manifest.json --backend llvm --out /tmp/tellur-resources
/tmp/tellur-resources playground/message.txt false
/tmp/tellur-resources playground/message.txt true
```

The first returns `done` and the file bytes with status 0. The second faults after
closing the file. Missing files return `missing` with status 3; ordinary read/close
failures return `failed` with status 1. The example only reads the named file.

## Collections

Arrays, vectors and maps use the existing runtime representation and operations.
Typed literals, nested owning collections, collection fields in records or tagged
values, lengths, lookup, replacement, insertion, removal and consuming conversion
or take operations are admitted. Map keys retain deterministic ascending order.
Bounds failures return the existing outcomes and rejected replacement values.
Reference-free collection payloads are also accepted in synchronous host signatures.

Element references retain the caller's stable storage and existing origin evidence.
Mutation and moves still require conflicting whole-collection loans to have ended.
Local collections of direct shared references preserve external owners when the
collection is moved, removed or dropped. Exclusive-reference elements and public
collection-origin contracts retain their checker restrictions.

Resource collections retain their sealed completion obligation: removing a resource
transfers that obligation, and the remainder must be transferred or consumed through
its empty outcome. Consuming draining loops are supported. Literal shapes and handle batches validate before
ownership transfer. Collection operations introduce no additional fuel ticks.

The [collection example](../../playground/llvm_collections.tellur) edits a vector,
returns an element borrow through a helper, and removes the smallest map key:

```sh
./bin/tellurc build playground/llvm_collections.tellur playground/manifest.json --backend llvm --out /tmp/tellur-collections
/tmp/tellur-collections 1 false
/tmp/tellur-collections 99 false
/tmp/tellur-collections 0 true
```

The first emits `second`, `changed`, `last`, `a`, `7`, `2` and returns the vector
`["first", "changed"]` with status 0. The second emits `missing` in place of the
selected element. The third faults after the collection operations.

## Synchronous loops

Tail loops evaluate initializers and restart arguments in written order, then
transfer the completed batch before the next iteration begins. Each iteration
clears its local bindings and path facts; surrounding locals keep their state.
Owned parameters can be swapped, collections drained and resource completions
handled through the same checked protocols as the Rust backend. References returned
by a helper within an iteration retain their ordinary last-use restrictions.

Expanding a looping helper creates internal nested loop targets. Its selected
completion sets the caller's bindings and path fact before returning to that
caller's continuation. Restarting the outer loop resets the inlined facts too.
This does not admit authored nested loops or reference-bearing loop parameters;
those remain checker restrictions. Supervised loops and fault boundaries remain
outside the LLVM profile.

Backedges add no fuel tick, activation or scheduler handoff. Every checked body
instruction, including a skipped guard, retains its existing fuel charge and
source site. Fixed-size transfer buffers and scratch cells are allocated once at
function entry, so long-running loops do not grow the native stack per iteration.

The [loop example](../../playground/llvm_loops.tellur) swaps two string owners,
updates an outer counter and returns from a looping helper:

```sh
./bin/tellurc build playground/llvm_loops.tellur playground/manifest.json --backend llvm --out /tmp/tellur-loops
/tmp/tellur-loops 0 false
/tmp/tellur-loops 1 false
/tmp/tellur-loops 4 true
```

The first emits `left`, `right`, `0`; the second emits `right`, `left`, `1`.
Both exit with status 0. The third faults with `loop stopped` inside the helper.

## Build artifacts and requirements

The compiler emits `OUTPUT.ll` and `OUTPUT.tellur-build/program.ll` directly,
then compiles `program.o` and links `OUTPUT` with its embedded LLVM/LLD 22.1.8.
Path guards are LLVM branches. Runtime faults retain Tellur source provenance;
native debugger information is not yet emitted.

The distributed 0.249 compiler includes the `llvm` feature. When building the
compiler from source, enable it and set `LLVM_SYS_221_PREFIX` to the static pinned
LLVM/LLD installation, as described in the repository README. A compiler built
without that feature reports a clear error for `--backend llvm`.

This intermediate release still requires Cargo and the runtime's pinned Rust
dependencies to build a cached static library, plus the macOS SDK for linking.
Tellur program code is not translated to Rust. The C ABI uses opaque value
handles and shares the existing runtime's launch, fuel, fault and result logic.
The resulting executable is standalone. Shipping a precompiled runtime and target
link packs belongs to the later single-binary checkpoint.

Async lowering follows using frontend state machines, measured against the
existing coroutine experiments. The whole-language parity gate must pass before
changing the default backend.
