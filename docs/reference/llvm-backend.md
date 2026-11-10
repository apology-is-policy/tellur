# Direct LLVM backend

Compiler 0.235 introduces `--backend llvm` for `build` and `exec` on macOS arm64.
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

The first slice accepts synchronous controls using `bool`, `u32`, `string`,
`bytes` and `unit`: declarations, moves/copies, assignment, pure expressions,
Boolean decisions, path guards, lexical releases, emit, faults and final exits.
It also handles expansion completions without unwinding. Scalar parameters and
outcome payloads use the same launch validation and process-status mappings as
the Rust backend. Multiple eligible executable entries are supported.

Control calls, aggregates, projections, references, resources, loops, fault
boundaries, contingencies, task operations and adapters remain outside this
profile. Launch signal policies are also rejected. Admission checks all checked
regions, including unused helpers. Unsupported programs receive `E_BACKEND`
before the backend creates its output directory or files; ordinary checking may
still succeed, and the Rust backend remains available.

## Build artifacts and requirements

The compiler emits `OUTPUT.ll` and `OUTPUT.tellur-build/program.ll` directly,
then compiles `program.o` and links `OUTPUT` with its embedded LLVM/LLD 22.1.8.
Path guards are LLVM branches. Runtime faults retain Tellur source provenance;
native debugger information is not yet emitted.

The distributed 0.235 compiler includes the `llvm` feature. When building the
compiler from source, enable it and set `LLVM_SYS_221_PREFIX` to the static pinned
LLVM/LLD installation, as described in the repository README. A compiler built
without that feature reports a clear error for `--backend llvm`.

This intermediate release still requires Cargo and the runtime's pinned Rust
dependencies to build a cached static library, plus the macOS SDK for linking.
Tellur program code is not translated to Rust. The C ABI uses opaque value
handles and shares the existing runtime's launch, fuel, fault and result logic.
The resulting executable is standalone. Shipping a precompiled runtime and target
link packs belongs to the later single-binary checkpoint.

The next synchronous slices add control calls, richer values, loops and
boundaries. Async lowering follows using frontend state machines, before the
whole-language parity gate permits changing the default backend.
