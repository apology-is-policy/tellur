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
Compiler 0.253 adds a bounded frontend async-frame profile; 0.254 adds cold control tasks. Compiler 0.256 adds spawned controls and consuming joins; 0.257 adds control cancellation requests.
Compiler 0.258 adds scripted async host operations and their cancellation alternatives.
Compiler 0.259 adds a private task-set runtime bridge. Compiler 0.260 admits retained
loops with repeated task calls and expanded loop completions. Compiler 0.261 adds
generated task sets with the existing completion-order and cancellation rules.
Compiler 0.262 adds the private retained supervision bridge; 0.263 admits generated
retained named supervision. Compiler 0.267 adds retained lexical scopes and supervised recovery.
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
A separate bounded async profile admits `run`, direct `await` and `yield`, including
synchronous callers that reach those controls. Cold control tasks support `defer`,
local moves/option wrappers and consuming `await`. Spawned controls and consuming joins
are supported since 0.256; control cancellation requests since 0.257. Scripted
async host operations share those task forms since 0.258. Retained loops are supported
since 0.260; task sets since 0.261; named supervision since 0.263; lexical scopes
and supervised recovery since 0.267. Suspending constructors and Rust adapters
remain outside that profile. Launch signal policies are also rejected. Admission checks all checked
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
no control activation, fuel tick or scheduling handoff. Scripted waits require
an async host operation, admitted by the separate retained profile since 0.258. A manifest-only
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
those remain checker restrictions. Supervised recovery is supported in synchronous
controls; its retained form still needs a separate integration checkpoint.

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

The distributed 0.256 compiler includes the `llvm` feature. When building the
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


## Frontend async-frame experiment

Compiler 0.250 accompanies a standalone comparison of hand-written explicit LLVM
state machines and LLVM coroutine splitting. Each control has a typed frame and
resume-state switch; direct-await children occupy fields inside their parents,
and independently scheduled tasks use a stable-address arena. The shared experiment
runtime keeps scheduling, host readiness, cancellation, joins and fault settlement.

Both lowerings match 88 reference schedules at O0/O2 through clang and embedded
LLVM/LLD. A nested direct-await fault checks child retirement before parent storage;
mutation tests reject omitted/duplicate retirement and unjoined borrowed children.
The [experiment](../../experiments/coroutines/README.md#frontend-frame-comparison-b14a)
records frame allocations, arena slack, code size and bounded in-process timings.
These measurements guide generated async lowering; they do not admit async source
programs through `--backend llvm`. Rust remains the default until parity.


## Retained resume callback foundation (0.251)

A private runtime root can now retain activation slots, temporary handles, outcomes,
errors and lexical state between C ABI resume callbacks. The callback receives only
an opaque context pointer, a caller-owned typed frame pointer and initial argument
handles; subsequent resumes do not transfer arguments again. Host and shared storage
borrows end when each callback returns. Host context selection and resource-root
retirement reuse the existing runtime operations.

A callback reports yield, ordinary completion or failure through a checked step tag.
Foreign host/arena/frame selection, early result consumption, malformed completion
and repeated resumption/retirement are rejected. Completion adopts owned results
once; administrative stop retires cold, suspended or unadopted completed work without
executing it or inventing an outcome. Original faults and fuel failures keep their
reports, while the caller's selected context is restored.

This root is runtime infrastructure, not a source task or a new awaitable value.
Direct-await children sharing the original invocation/domain, generated checked-IR
resume functions and source scheduling admission still follow. Existing LLVM task,
async host, adapter and signal gates remain intact. The default Rust backend and
language ownership, origin, cancellation and fault rules are unchanged.


## Retained invocations and inline children (0.252)

Private ABI callbacks can execute as existing runtime activations. A descriptor
fixes the callback, physical layout and by-value child offsets. Source slots remain
in the shared stable arena; a callback borrows its existing activation's identity
and host only while selected. The runtime owns temporary values and outcomes across
returns. Initial argument handles arrive once, and child delivery supplies one
outcome handle for ordinary payload routing before the next source instruction.

The callback reports one checked transition: continue, yield, direct await, normal
completion or failure. Direct await uses the existing queued invocation and wait,
original parent owner and owning-domain membership. The existing source scheduler
performs the await handoff before child admission and preserves yield order and
administrative result delivery. No second scheduler or independent child root is
introduced. A lifetime covers the entire physical root frame and its inline children;
layout checks reject invalid extents, alignment and overlapping child storage.

Normal and emergency retirement drop parked ABI borrowers before source slots and
resource owners. Failed or stopped children settle before parent retirement; stopped
work never runs a normal continuation. Completed by-value storage can be initialized
for a later invocation only after the original child's consuming delivery.

This0.252 foundation used private callbacks and Rust-side descriptors. The0.253
profile below now generates module metadata, frame layouts and resume functions
from checked source. Cold tasks follow in 0.254 below; staged task/cancellation/host
integration remains next. Rust remains the default backend.


## Generated async frames (0.253)

The compiler now emits a typed LLVM struct and resume-state switch for each control
that suspends, and for synchronous callers reaching one through `run` or a control
call. The frame retains its program counter and path facts. Each direct child has
its own typed field, initialized before capture and kept alive until delivery or
emergency retirement. Dynamic source values keep the existing stable runtime slots;
this change does not replace the runtime's value/metadata allocation strategy.

The first profile admits straight-line checked instructions, guarded decisions,
`run`, direct `await`, `yield`, owned/borrowed arguments and results, synchronous
calls/constructors/primitives, and source faults. A direct await hands off before
child admission; `run` pushes the child on the same context stack. Only explicit
source handoffs select another context. Payload routing happens before the next
source tick, so it neither repeats capture nor spends extra instruction fuel.
Original and expanded programs retain the same diagnostics and result origins.

An immutable C-safe module table describes names, parameter slots, callbacks,
physical extents and child offsets. The runtime validates it before admission and
owns a single process-lifetime registry, reused by repeated launches. Rust layouts
are never constructed by generated LLVM, and scoped borrows are never extended to
static lifetimes. Physical frame sizes, alignments and child offsets are LLVM
constant layout expressions. All frame types are declared before those expressions.
The runtime keeps invocation/domain, adoption and child-before-parent cleanup policy.

```sh
./bin/tellurc build playground/llvm_async.tellur playground/manifest.json --backend llvm --out /tmp/tellur-async
/tmp/tellur-async --json true Tellur
/tmp/tellur-async --json false Tellur
```

The first run emits and returns `Tellur!`; the second emits and returns `Tellur`.
The example carries an exclusive caller borrow through a yielding child, then
returns the owned string. Both select `done` with status0.

The 0.253 profile excluded cold handles; 0.254 adds them as described below.
Spawn/join is added in 0.256 below. Task sets, cancellation, asynchronous host operations, loops or boundaries
inside retained controls, Rust adapters and launch signals are still rejected before output creation. Constructors that enter retained controls
are also outside this slice. Synchronous loops/boundaries/constructors retain their
previous support. By-value recursive frame graphs are rejected. This bounded source
admission does not flip the default backend or claim whole-language parity.


## Cold control tasks (0.254)

`defer producer(arguments)` captures supported arguments once, in written order,
using the selected invocation's existing task registry. It returns the original
opaque cold task value without starting the producer, allocating a physical task
frame or introducing a scheduling handoff. Local moves and nested option wrappers
preserve that identity. The frontend still requires consumption on every normal
path and rejects copying, dropping or escaping local cold handles.

Consuming `await` validates the original host, owner, producer and wait admission,
then queues the existing obligation. The child physical frame occupies a typed
field of its caller, initialized at that await site. Delivery reuses the existing
outcome and origin rules. Owned strings/bytes/records, resource captures, direct
shared/exclusive borrows and supported contained references retain their ownership.
An abandoned cold capture is retired by the same runtime cleanup as a Rust-backed
task; no producer body runs merely because the enclosing invocation faults.

```sh
./bin/tellurc build playground/llvm_cold.tellur playground/manifest.json --backend llvm --out /tmp/tellur-cold
/tmp/tellur-cold --json false Tellur
/tmp/tellur-cold --json true Tellur
```

Both runs emit `captured` before `producer started`. The first returns `Tellur!`
with status 0; the second reports the producer's source fault. The example retains
an exclusive caller borrow through cold capture, a local move and a child yield.

This is the bounded sequential cold-control profile. Spawn/join and cancellation are added below; task sets,
async host producers and retained loops/boundaries/constructors still
require their later integration checkpoints. By-value physical frame cycles remain
rejected. The default backend and the language's ownership rules are unchanged.


## Private physical frame arena (0.255)

The runtime now has aligned chunk storage for future spawned LLVM frames. A private
owned-invocation entry leases a zero-initialized frame from this arena. Inline run
and await children retain pins on the same allocation, so retiring a parent cannot
recycle storage still held by a descendant. The last pin returns the slot for reuse;
no physical heap allocation is required for each logical frame. Source values and
task metadata continue to use their existing stores and allocations.

The arena validates extents/alignment before allocation, supports over-aligned and
larger frames, and keeps chunks alive while any pin survives its arena handle.
Reusing a slot clears its previous counter/facts. Layout rejection and failed task
or wait admission leave no live physical pin behind. Existing Machine/Contexts
still own task identity, scheduling, borrowed results and fault settlement.

The 0.255 checkpoint was private runtime infrastructure. The 0.256 bridge below
uses its leases for source spawn/join; caller-owned root storage remains valid.
Neither checkpoint changes the default backend or claims whole-language parity. The 0.250 coroutine/frontend
experiments remain the performance baseline; the new allocator tests measure
physical chunks and reuse, not whole-program allocation or throughput.


## Spawned controls and consuming joins (0.256)

`spawn producer(arguments)` evaluates captures once and queues an invocation-owned
child without selecting it. The parent continues until an explicit source handoff.
`await handle` consumes the original started handle and observes its normal completion;
local moves and supported option wrappers retain that same authority. Borrowed
captures/results, contained origins, guards and fault/fuel behavior follow the checker
and the existing source scheduler. A yielded or completed child remains owned until
its join or fault settlement; completion alone does not release a caller loan.

The compiler emits C-safe descriptors with internal producer identity separate from
its display name. The module registry owns exact cold/started type spellings once,
including imported controls. Each root shares one arena with spawned descendants;
inline run/await children pin the enclosing physical allocation. Spawned leases
retire through existing task/domain cleanup and only their last pin permits reuse.
The source driver, claim/join policy and resource settlement are unchanged.

```sh
./bin/tellurc build playground/llvm_spawn.tellur playground/manifest.json --backend llvm --out /tmp/tellur-spawn
/tmp/tellur-spawn --json false Tellur
/tmp/tellur-spawn --json true Tellur
```

The first run emits `queued` before either child starts and returns `Tellur!`.
The second reports the first child's fault and settles its sibling. The example
keeps an exclusive caller borrow through the original consuming join.

This profile admits the same payload types as the supported retained controls.
Existing source restrictions still apply: an owner without a fault disposition
cannot cross a spawned producer's handoff. Such an owner can be completed before
yielding. Cancellation is added below. Async host producers, task sets, retained loops/boundaries/
constructors, Rust adapters and launch signals remain outside LLVM admission.

The private runtime matrix uses one 4096-byte chunk for two spawned frames whose
children are inline; all physical pins retire after normal and fault paths. Compare
this allocation policy with the [0.250 measurements](../../experiments/coroutines/README.md):
the nested-fault frontend experiment used one 4120-byte heap chunk including its
header, while coroutine frames used four allocations (136 bytes at O0, 120 at O2).
Those are different fixtures and runtime representations, so these counts are a
storage-policy comparison, not a throughput or total-memory improvement claim.
Source values, task metadata and arena bookkeeping have separate allocations.

Spawned join preflight checks the original producer/owner, unclaimed phase and
completion-wait capacity before consuming its handle. Admission failure stays in
the selected activation so ordinary unwind retires unselected siblings and their
physical pins. It never escapes through a partially admitted source handoff.


## Control cancellation (0.257)

`cancel child;` requests cancellation of a started control through the existing
runtime task binding. The request is sticky and reaches descendants, including
those created later. It preserves the original handle and its consuming join,
introduces no source handoff, and allocates no completion wait or physical frame.
Repeated requests do not complete, retire or restart the child. Already completed
children retain their ordinary outcome and result ownership until joined.

Control tasks without cancellable host operations continue to ordinary completion
or fault despite the request. Since 0.258 a host operation's explicit `cancelled`
alternative observes it; no implicit interruption at `yield` or join is introduced.
The [cancellation rules](async-execution.md#cancellation) remain the language contract.

```sh
./bin/tellurc build playground/llvm_cancel.tellur playground/manifest.json --backend llvm --out /tmp/tellur-cancel
/tmp/tellur-cancel --json false Tellur
/tmp/tellur-cancel --json true Tellur
```

The example requests cancellation twice before the writer runs, then joins both
children. The first run returns `Tellur!`; the second reports the writer's ordinary
source fault. Its exclusive caller loan remains live until join or fault settlement.
Cancellation of sets, retained loops/boundaries/constructors,
adapters and launch signals remain outside this LLVM profile. Rust stays default.


## Scripted async host operations (0.258)

Supported manifest async operations now use the same generated control frames as
other async targets. `defer op(args)` captures arguments without starting the host;
`spawn op(args)` queues its arena-owned task without a handoff. Consuming `await`
uses the existing task identity and delivers the original outcome exactly once.
A synchronous control enters with `run`; an async control uses `await`. `run`
retains its invocation context, but a pending host leaf still lets other source
tasks progress. Readiness polling and result delivery consume no instruction fuel.

Host leaves reuse the runtime's operation validation, scripted wait counts,
resource publication and cleanup. Borrowed resources stay live through pending
readiness and can be consumed after completion. Signatures retain the existing
owned-data host ABI restrictions; reference-bearing host result signatures and
Rust adapters are not admitted. No new live async provider is introduced: these
operations need `--scenario` until the later adapter checkpoint.

An explicit `cancelled` arm on `run` or `await` observes the current task's sticky
request. Cancellation before start consumes no scenario call. Cancellation while
pending suppresses the completion event and payload; a completed operation retains
its ordinary result. Without that arm the operation completes normally. Repeated
requests, inherited requests and required joins keep the language's existing rules.

```sh
./bin/tellurc build playground/llvm_host.tellur playground/llvm_host.manifest.json --backend llvm --out /tmp/tellur-host
/tmp/tellur-host --scenario playground/llvm_host.scenario.json
```

The example emits `captured`, then `run`, `cold`, `queued`, `spawn` and `await`.
Its supplied scenario deliberately makes every host call report pending readiness.
Retained loops and task sets are added below. Retained boundaries/constructors,
adapters and launch signals remain later work. Rust stays default until parity.


## Private task-set runtime bridge (0.259)

The retained ABI can create an exact-producer set, queue a child into it, select
its next completion and request cancellation of its current members. Immutable
module metadata supplies producer identities. Whole-set validation rejects wrong
shapes, mixed producers, duplicate or stale members and foreign task authority
before transferring handles or changing any cancellation flag. Spawn validates
all captures and its destination before allocating an arena frame.

The original scheduler controls completion order and appends the remaining set to
the selected member's payload. Only that member is joined; other members keep their
ownership and fault obligations. An empty selection consumes its set immediately,
without a source handoff, completion-wait allocation or outcome handle. Cancellation
restores the same set and does not require wait capacity. Later insertions do not
inherit a cancellation request previously applied to the set.

Private tests exercise ordinary and scripted host members, caller loans, queued,
running and completed cancellation, faults, capacity rejection and retirement at
fuel cuts and quotas 1, 3 and unbounded. These tests use private callbacks: checked
LLVM task-set programs remained gated at that checkpoint. Retained loops are added
in 0.260 and generated set lowering in 0.261 below. Rust remains the default backend.


## Retained loops (0.260)

Loops may now retain an LLVM control across its existing `await`, `run`, `yield`,
cold-task, spawned-task, cancellation and scripted-host operations. This includes
synchronous callers that reach an async operation through a helper. A repeated
inline call reuses its by-value child frame only after the previous call has
delivered and retired; spawned children keep their arena storage and join rules.

Loop entry and `again` evaluate every parameter expression before resetting local
bindings and path facts. The whole parameter transfer finishes in the selected
callback. No temporary handle array or LLVM SSA value is kept across a suspension.
Outer bindings retain their state, and ordinary per-iteration loans end at their
checked last uses. A restart adds no scheduler handoff or source tick.

Expanded synchronous helpers may introduce internal nested loops into a retained
caller. Their selected completion transfers payloads, sets the caller's path fact
and resumes the corresponding outer continuation. Nested states and inline child
metadata use the same recursive instruction inventory. Source fault locations and
fuel charges, including skipped guards, retain the existing execution semantics.

```sh
./bin/tellurc build playground/llvm_async_loops.tellur playground/manifest.json --backend llvm --out /tmp/tellur-async-loops
/tmp/tellur-async-loops 0 false
/tmp/tellur-async-loops 3 false
/tmp/tellur-async-loops 3 true
```

The first run emits `left`, `right`, `0`; the second emits `right`, `left`, `3`.
The third faults with `iteration failed` in its third child invocation.
Authored nested loops and reference-bearing loop parameters keep their checker
restrictions. Task-set source lowering is added below. Retained supervision/recovery,
fault boundaries and constructors, launch signals and Rust adapters remain gated. Rust is
the default backend until parity.


## Generated task sets (0.261)

`tasks<producer>` now creates an exact-producer set in a retained LLVM activation.
`spawn producer(arguments) into set;` evaluates captures in written order and queues
an arena-owned member without handing off. `count(borrow set)` includes running and
completed members that have not been joined. Sets can move locally and cross loop
backedges under the existing checked ownership rules.

`await next` delegates completion ordering to the original scheduler. Its selected
member payload is followed internally by the remaining set, which returns to the
checked set binding. Only that member is joined. Empty selection retires the set
without a source handoff, wait or outcome allocation. Cancellation requests affect
all current members and preserve the set's drain obligation; later insertions do
not inherit earlier requests. Scripted async host producers use the same path.

The source restrictions are unchanged: captures and results are owned and
reference-free; no set escapes in a control argument/result, helper argument or
collection. A producer cannot name an outcome `empty`. Retained resource owners
still need their existing fault disposition, and Rust adapter resources remain
gated with the adapter backend. Member-local synchronous resource operations may
complete before a handoff. Escaping member faults retain their original source
cause and stop sibling work; they never become a normal `await next` result.

```sh
./bin/tellurc build playground/llvm_task_sets.tellur playground/manifest.json --backend llvm --out /tmp/tellur-task-sets
/tmp/tellur-task-sets 0
/tmp/tellur-task-sets 6
```

The first run drains an empty set. The second feeds six jobs while keeping at most
two unjoined, printing each spawn and reap. Original and expanded programs preserve
fuel charges, path facts, fault reports and host readiness. Retained boundaries,
supervised recovery, constructors that suspend, launch signals and Rust adapters
remain later slices. Rust remains the default until parity.


## Private retained supervision (0.262)

Named retained work and its contingency now have distinct by-value physical frames
behind the private runtime ABI. A request validates both child identities, frame
extents, parameter arities and retained-slot metadata before transferring arguments.
Descriptor version 2 records retained parameters in immutable module storage; its
versioned entry symbol prevents reading the previous descriptor layout as the new
one. Other value and task ABI operations retain their existing version.

The original runtime admits the work domain and selects its handler only after
failed descendants, loans and sibling task domains retire. The waiting supervisor's
physical frame and owned locals survive a handled fault. Failed work and handlers
still retire before their owners; pins keep arena-backed ancestors alive until all
inline descendants stop. Repeated supervision resets child storage after retirement.

Work and handlers return administrative settlement, never an outcome for the failed
work. The supervisor resumes its prepared state without an outcome handle or source
handoff. A separate lifecycle validates work admission, optional handler admission
and one-time settlement. Ordinary controls cannot use this settlement channel.
Handler failures cannot re-enter the same handler; fuel and driver errors bypass
contingency. No additional scheduler, cancellation or fault-domain policy is added.

Private callbacks are compared with original runtime continuations across normal
and faulted work, secondary faults, escalation, repeated requests, borrowed resource
suppliers, spawned/set siblings, cancellation, pending host operations and every
fuel cut at quotas 1, 3 and unbounded. Both caller-owned and arena-owned parents are
covered, including inline, awaited, spawned and cancelled ancestors. Invalid child
roles, duplicate handles, descriptor slots and lifecycle transitions reject without
partial transfer. Physical pins and resource/task obligations retire exactly once.

The 0.262 checkpoint kept source boundaries across handoffs gated. Generated named
supervision is admitted in 0.263 below. Lexical same-frame recovery, constructors
that suspend and launch signals still need their own integration. Rust adapter resource mappings remain at
the adapter station. Rust remains the default; accepted language semantics are unchanged.


## Generated retained named supervision (0.263)

A named boundary reached by retained execution now has generated work and handler
frames. The same applies when its work reaches a yielding control or supported
async host operation. Both frames are distinct typed fields within the supervisor;
repeated requests reuse them only after the previous invocation retires. Ordinary
synchronous-only call trees retain their synchronous path.

`supervise` evaluates arguments once in written order and stores the supervisor's
successor state before requesting work admission. The original runtime retains
control of fault domains, cancellation, readiness and handler selection. Only an
explicit source fault enters contingency, after failed descendants and their loans
retire. Retained parameters transfer in declaration order, including owned records
and collections. Imported private names and source sites remain in causal reports.

Normal work and contingency end with administrative settlement. Neither produces
an outcome, payload, fault value, extra fuel tick or handoff for the supervisor.
Only the supervisor's own continuation resumes. Handler faults and escalation
propagate outward; the handler cannot handle its own failure. Fuel exhaustion,
malformed host responses and driver errors bypass contingency. Supported spawned
children, task sets and cancellation keep their existing ownership and settlement
rules, including owning-domain authority when an outside child faults.

```sh
./bin/tellurc build playground/llvm_async_supervision.tellur playground/manifest.json --backend llvm --out /tmp/tellur-async-supervision
/tmp/tellur-async-supervision --json false
/tmp/tellur-async-supervision --json true
```

Both invocations finish the supervisor with `done` and status 0. The first completes
both requests normally. The second handles the first request's fault after its
borrowed helper yields, reports the retained context and reason, then starts the
independent second request. Original and expanded builds preserve the same events.

Lexical same-frame boundaries, supervised recovery and process termination inside
retained controls remain rejected before output creation, as do suspending
constructors, Rust adapters and launch signal policies. This admission checkpoint
does not change accepted source semantics or switch the default backend.


## Private retained lexical scopes (0.264)

Private retained callbacks can now request lexical work admission and normal
fallthrough through immutable scope metadata. The original runtime opens and
closes each fault domain and resource scope. Selected body, handler and after
states reach the callback through an explicit resume-state query; the runtime
never assumes an offset or representation for the physical program counter.
Scope fallthrough and handler selection remain administrative, with no extra
source fuel, outcome allocation or scheduler handoff.

A selected lexical handler shares the original physical frame. Failed temporary
values, borrowed handles, outcomes and child-delivery state retire before source
owners are released. Handle identities advance so discarded handles cannot become
valid again. Unaffected owned buffers and the frame's arena pin survive handover.
The original domain authority selects a handler only after descendants and sibling
tasks retire. A fault from outside that owning domain cannot enter this handler.
Secondary handler faults and escalation propagate outward, preserving their cause.
Fuel, driver and internal failures bypass contingency.

Requests reject invalid metadata, duplicate admission, wrong scope identity and
work-end during contingency before effects. Handler-state validation precedes
clearing the sticky work failure. Named work and lexical scopes compose in either
nesting order, preserving the named supervision record until its own handler or
settlement has been selected. Repeated normal entry reuses retired storage; an
ordinary outcome exit closes active work scopes. Private scope-aware callbacks
also exercise process disposition through the original completion path.

Twelve private matrices compare original continuations with retained callbacks
for caller-owned and arena-owned roots, each fuel cut through 40 and quotas 1, 3
and unbounded. They cover nested scopes, borrowed resources, repeated entry,
process statuses, owned buffer identity, awaited/spawned/cancelled ancestors,
borrowing siblings and task-set members, pending host work, malformed replies,
outside owning-domain faults and fail-closed lifecycle transitions.

The generated C descriptor remains version 2 and has no lexical registration yet.
The LLVM emitter and source admission are unchanged. Retained loop/recovery
coordination must be proved before generated lexical scopes and recovery are
admitted. Suspending constructors, signals and Rust adapters remain later work;
Rust remains the default. These private callbacks do not add source-language forms.


## Private retained loop and recovery transfers (0.265)

Private retained callbacks now request loop entry, ordinary repetition, supervised
recovery and expanded loop completion through immutable loop metadata. Exact loop
identities, parameter slots and bounded resume states are checked before argument
handles move. Duplicate or stale handles, unmatched targets, wrong transfer roles
and unfinished temporaries reject before runtime scope cleanup or physical reset.

Arguments transfer together into a runtime-owned vector after callback borrows end.
The original Machine closes intervening scopes, retires task obligations and applies
normal or fault resource disposition. Only after that cleanup succeeds does it
reset source locals, store all new parameters and invoke the private physical-fact
reset callback. That callback receives only its opaque frame, with no Host, Context
or source arguments. The runtime selects the next resume state explicitly; it never
assumes a physical program-counter offset. Loop transfers keep the invocation
active and do not masquerade as final outcomes or named settlement.

Recovery retains the existing source rule: the compiler supplies supervisor values
preserved at boundary entry. No new source arguments or recovery outcomes are added.
Failed work cannot resume. Nested handlers preserve secondary causes, and named
work settles only after its local loops and scopes retire. Fuel and driver failures
still bypass contingency. By-value children and arena pins keep their original
ownership and cancellation rules.

Twelve private matrices compare original continuations and retained callbacks at
every fuel cut through 35 (40 for named work), quotas 1, 3 and unbounded, using both
caller-owned and arena-owned roots. They cover owned-buffer swaps, reset path facts,
updated boundary-entry parameters, repeated child storage, outward transfers,
nested recovery and secondary faults, process status, borrowed child frames,
spawned/set siblings, inline/awaited/spawned/cancelled ancestors, pending host I/O,
malformed replies, named settlement and invalid metadata/lifecycle requests.

Generated C descriptor v2 and LLVM source admission are unchanged. The next step
is generated lexical registration and work/handler/end states using these private
bridges. Suspending constructors, launch signals and Rust adapters remain later
checkpoints; Rust remains the default backend.


## Compiled lexical and loop registration (0.266)

The runtime now accepts an explicit version 3 control table through
`tellur_rt_v3_run_entry`. Each entry contains the existing version 2 control
fields plus an optional pointer to immutable lexical and loop metadata. The old
entry symbol retains its original stride; version 2 allocations are never read
as larger version 3 descriptors. Module identity includes the ABI version.

A scope descriptor declares the resume-state bound, lexical boundaries and loops.
Lexical entries carry their diagnostic name, body/handler/after states, retained,
retirement, work and handler slot groups, and the fault-report slot. Loop entries
carry the original target, body/after states, iteration locals, ordered parameter
slots and an `extern "C"` physical-fact reset callback. All spans and callbacks are
module-static, immutable data. Reset callbacks receive only their physical frame;
they cannot fail, retain it, access host state or implement source cleanup.

Registration rejects malformed spans, excessive counts, alignment errors, invalid
UTF-8 names, invalid state ranges, overlapping state roles, duplicate slots or loop
targets, missing reset callbacks and invalid retained/parameter/fault slot roles.
Host-operation descriptors cannot carry source scopes. These checks finish before
publishing control identities. Entry storage, arity and quota checks precede capture
transfer. The original source Machine still performs scope/task/resource retirement
and recovery; registered metadata does not add a scheduler or a scope policy.

One process-lifetime registry owns the normalized controls and initialized scope
layouts. Concurrent registration and repeated execution reuse the same identities;
a losing registration candidate is dropped normally. There are no per-launch
metadata leaks or forged static references. Original version 2 tables continue to
work and cannot be mixed with a different version or module in that registry.

Nine added tests cover malformed metadata and entry preflight, version separation,
concurrent registration, stable identities, owned captures and registered lexical/
loop execution. The execution matrices compare original continuations at every
fuel cut, quotas 1/3/unbounded, caller/arena roots, nested recovery, owned buffers,
physical facts, host waits/failures, borrowed children and named work settlement.

The 0.266 checkpoint registered these C-safe descriptors. Compiler 0.267 emits
them and admits retained lexical recovery, as described below. Version 2 remains
supported by the runtime; Rust is still the default backend.


## Retained lexical scopes and recovery (0.267)

Lexical fault boundaries now compose with retained `run`, `await`, `yield`, cold
and spawned tasks, task sets and named supervision. Normal work fallthrough closes
its scope before the following source instruction. A work fault retires affected
children, captures and loans before entering its original contingency. Nested
handlers preserve their original diagnostic causes; process termination preserves
its declared status. Contingencies retain their existing disposition restrictions.

Supervised loops evaluate `again` arguments in written order and transfer the
whole batch after retiring abandoned scopes and iteration owners. Recovery uses
the checked boundary-entry values, including permitted supervisor updates. The
runtime resets only local path facts after retirement. Outward loop transfers and
expanded helper completion use the same cleanup path. Cancellation remains sticky:
source handoffs and explicit host cancellation alternatives determine observation;
no interruption is added at lexical scope entry or normal scope completion.

Generated version 3 tables contain module-static lexical and loop metadata.
Resume functions accept runtime-selected states and preserve child payload delivery
before the next source tick. Boundary-end states are administrative and consume no
source fuel. State ranges are checked for overflow. Direct child frames stay
embedded by value, spawned frames stay arena-owned, and the original Machine owns
scope selection, fault settlement and resource retirement.

The [retained recovery example](../../playground/llvm_async_recovery.tellur) yields
inside each request. A failed first request retires and recovers into a healthy
second iteration; a healthy first request completes directly.

```sh
./bin/tellurc build playground/llvm_async_recovery.tellur playground/manifest.json --backend llvm --out /tmp/tellur-async-recovery
/tmp/tellur-async-recovery --json false
/tmp/tellur-async-recovery --json true
```

Both runs complete with status 0. The first emits `request context`. The second
emits `retired request`, `request invariant failed`, then `request context`.
Original and expanded lowering are compared with the Rust backend at O0/O2,
including every tested fuel cut, nested faults, borrowing siblings, cancellation,
imports, guarded scopes, pending host replies and driver errors. Runtime quotas
1/3/unbounded preserve the same reports and fuel accounting. Suspending
constructors, launch signal policies and Rust adapters remain later checkpoints.


## Retained constructor runtime bridge (0.268)

The private runtime can retain an inline constructor while its child yields or
waits for a host operation. Its destination is the original caller's live storage,
qualified by arena and frame generation. Nested constructors can return owned
buffers or shared/exclusive references without relocating the original owner.
Each constructor completion is delivered only after the original Machine retires
the child. Delivery creates no outcome handle, source tick or task handoff.

The request checks child identity, constructor role, arity, destination vacancy
and all capture handles before moving values. Every resumption revalidates the
original destination. Missing, stale, occupied or mismatched destinations and
wrong completion forms fail; publication followed by a malformed callback is
revoked before fault retirement. Constructors cannot serve as invocation roots,
cold/spawned task producers, task-set members, named boundary roles or host leaves.

Local recovery retains the constructor's own destination while retiring failed
work and pending child delivery. Partial resource owners retire before its
contingency. Existing host readiness, sticky cancellation, faults and quota rules
remain authoritative. Private matrices compare original source-machine callbacks
with ABI callbacks at every tested fuel cut and quotas 1/3/unbounded, including
caller-owned and arena-owned ancestor frames, repeated nested calls and buffer
identity. No source language rule changes.

This checkpoint does not admit suspending constructors to LLVM source builds.
Versioned constructor registration and generated destination delivery follow;
version 2/3 descriptor strides remain unchanged. The Rust backend stays default.
