# Direct LLVM backend

Compiler 0.235 introduces `--backend llvm` for `build` and `exec` on macOS arm64.
Compiler 0.236 adds synchronous calls between scalar controls, including imported
private helpers, implicit single-completion calls and identity-routed outcomes.
Compiler 0.237 adds ordinary owned records with scalar leaves, including nested,
empty, imported and concrete generic records. Compiler 0.238 adds options, closed
variants, named outcome values and their decisions/captures with recursively owned
payloads. The default remains `--backend rust`. Both start from the same checked program;
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
Boolean decisions, path guards, lexical releases, emit, faults and final exits.
It also handles expansion completions without unwinding. Admitted parameters and
outcome payloads use the same launch validation and process-status mappings as
the Rust backend. Multiple eligible executable entries are supported.

Collections, references, opaque/resource values, user constructors, loops, fault
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

## Build artifacts and requirements

The compiler emits `OUTPUT.ll` and `OUTPUT.tellur-build/program.ll` directly,
then compiles `program.o` and links `OUTPUT` with its embedded LLVM/LLD 22.1.8.
Path guards are LLVM branches. Runtime faults retain Tellur source provenance;
native debugger information is not yet emitted.

The distributed 0.238 compiler includes the `llvm` feature. When building the
compiler from source, enable it and set `LLVM_SYS_221_PREFIX` to the static pinned
LLVM/LLD installation, as described in the repository README. A compiler built
without that feature reports a clear error for `--backend llvm`.

This intermediate release still requires Cargo and the runtime's pinned Rust
dependencies to build a cached static library, plus the macOS SDK for linking.
Tellur program code is not translated to Rust. The C ABI uses opaque value
handles and shares the existing runtime's launch, fuel, fault and result logic.
The resulting executable is standalone. Shipping a precompiled runtime and target
link packs belongs to the later single-binary checkpoint.

The next synchronous slices add references, primitive operations, loops and
boundaries. Async lowering follows using frontend state machines, before the
whole-language parity gate permits changing the default backend.
