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
`bytes`, `unit`, records, options, closed variants and named outcome values
recursively composed of those types: declarations, moves/copies, assignment, pure expressions,
value decisions, synchronous control/primitive calls, path guards, lexical releases,
emit, faults and final exits.
It also handles expansion completions without unwinding. Admitted parameters and
outcome payloads use the same launch validation and process-status mappings as
the Rust backend. Multiple eligible executable entries are supported.

Direct references may point to those reference-free values; shared/exclusive
parameters, direct borrowed results, reborrows and indirect fields are supported.
Collections, reference-bearing aggregates, opaque/resource values, user constructors, loops, fault
boundaries, contingencies, task operations and adapters remain outside this
profile. Launch signal policies are also rejected. Admission checks all checked
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

Storage identity does not add an alternative ownership or lifetime rule. Borrowed
primitive parameters remain rejected pending their host-operation lowering.

## Direct references

Shared and exclusive references can point to the admitted reference-free scalar,
record and tagged values. Synchronous controls can take and return direct borrows,
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

This slice excludes aggregates containing references, references to those carriers,
borrowed host operations and the remaining resource/task/boundary forms. Backend
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

Returning synchronous primitives can take owned `value` or `consume` arguments
and return scalar/record/tagged payloads from the admitted profile. Arguments are
evaluated once in written order and their handles transfer as one validated batch.
The whole declared signature must be supported, including unselected outcomes.
Borrowed parameters, async operations, Rust adapters and unsupported payload types
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

## Build artifacts and requirements

The compiler emits `OUTPUT.ll` and `OUTPUT.tellur-build/program.ll` directly,
then compiles `program.o` and links `OUTPUT` with its embedded LLVM/LLD 22.1.8.
Path guards are LLVM branches. Runtime faults retain Tellur source provenance;
native debugger information is not yet emitted.

The distributed 0.241 compiler includes the `llvm` feature. When building the
compiler from source, enable it and set `LLVM_SYS_221_PREFIX` to the static pinned
LLVM/LLD installation, as described in the repository README. A compiler built
without that feature reports a clear error for `--backend llvm`.

This intermediate release still requires Cargo and the runtime's pinned Rust
dependencies to build a cached static library, plus the macOS SDK for linking.
Tellur program code is not translated to Rust. The C ABI uses opaque value
handles and shares the existing runtime's launch, fuel, fault and result logic.
The resulting executable is standalone. Shipping a precompiled runtime and target
link packs belongs to the later single-binary checkpoint.

The next synchronous slices add reference-bearing aggregates, resource and collection operations, loops and
boundaries. Async lowering follows using frontend state machines, before the
whole-language parity gate permits changing the default backend.
