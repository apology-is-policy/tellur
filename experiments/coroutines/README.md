# E2.1: LLVM coroutine experiment

This standalone experiment asks whether Tellur's checked async controls can be
lowered to LLVM's switched-resume coroutines while keeping the reference
executor's observable behavior. The production backend is unchanged: generated
executables still use the Rust continuation machinery. Nothing here is a
supported compiler feature.

## Contents

| File | Role |
| --- | --- |
| `runtime.c` | Single-thread runtime: FIFO runnable tasks, scripted host operations, round-robin idle readiness signals, sticky structured cancellation, joins, task sets, fault settlement, loan and frame accounting |
| `intrinsics.ll` | `coro_resume`, `coro_destroy` and `coro_done` for the runtime (C cannot name the intrinsics) |
| `cancellation.ll` | [`playground/cancellation.tellur`](../../playground/cancellation.tellur), the Milestone 5 specimen |
| `sets.tellur`, `sets.ll` | A task set whose first completion cancels the remaining members, drained in completion order |
| `fault.tellur`, `fault.ll` | A sibling fault while another child's operation is pending |

Each `.ll` file was lowered by hand from its Tellur source, following the rules in
the header of `cancellation.ll`:

- An async control is a coroutine (`presplitcoroutine`, `llvm.coro.id`/`begin`/
  `suspend`/`free`/`end`).
- Path labels are basic blocks, and `delta` is a switch on outcome codes.
- `spawn` creates a frame that starts suspended. An await of a host operation is a
  handoff (requeue, suspend, then start).
- A join is a loop that suspends while the runtime reports pending.
- A direct await of an async control drives the nested frame inside the caller's
  task.

## Running

The toolchain is pinned to Homebrew `llvm@22` (22.1.4; rustc uses LLVM 22.1.6):

```sh
clang=/opt/homebrew/opt/llvm@22/bin/clang
$clang -isysroot "$(xcrun --show-sdk-path)" -O2 -Wall -Werror -Wno-override-module \
    runtime.c intrinsics.ll cancellation.ll -o cancellation
./cancellation script.txt
```

A script lists host calls in start order as `operation outcome waits [text]`.
[`tests/coroutine_experiment.rs`](../../tests/coroutine_experiment.rs) builds every
specimen at O0 and O2. It derives the script from the same scenario it gives the
reference executor, and requires identical events and process status. It skips
when the toolchain is absent, unless `TELLUR_REQUIRE_COROUTINE_EXPERIMENT=1` is set.

Since 0.234, with the cargo feature `llvm`, the test also builds every specimen with
the LLVM and LLD embedded in `tellurc` (`tellurc llvm-smoke [--release] OUT IN.ll...`).
The C runtime is lowered to IR once by the clang above; the rest happens in-process.

## Evidence

- **Cancellation specimen:** 12 schedules agree at O0 and O2:
  - the slower receive pending when cancelled;
  - both receives completed before the request (the ordinary outcome wins the race);
  - the primary ending early, or answering later;
  - neither answering;
  - three close-failure combinations;
  - an unavailable handle.
- **Task set:** all 64 combinations of three members' readiness delays agree,
  including members that completed before `cancel active;`.
- **Fault:** a sibling fault with the other child's operation pending, or already
  completed, gives the reference events, the fault message and status 1 for six
  delays.
- **Frames:** the runtime allocates every frame and fails if any frame is released
  twice or is still allocated at exit. Frames destroyed while suspended (fault
  settlement) take the same cleanup path as completed frames.
- **Loans:** a frame keeps its borrowed handle until the consuming join.
  - Two mutations of `cancellation.ll` are caught at run time as "loan violated".
    One closes a handle before the borrowing reader is joined; the checker rejects
    the equivalent program with `E_LOAN`. The other skips the cancelled child's join.

## Findings

1. **Frames.** CoroSplit stores every value live across a suspension in the
   heap frame. Examples are `data` held across two joins and the direct-await
   result slots, so no manual frame layout was needed.
   - Frame sizes are identical at O0 and O2: reader, worker, step and gather
     32 bytes; first_answer 56; fetch 72; failing 32, or 24 at O2.
   - Frames never move, so a borrowed reference stored in a frame stays valid
     until the frame is destroyed.
2. **Ownership of protocol state.** Coroutines own only control state.
   Scheduling, readiness, cancellation flags, task sets, completion order and
   fault settlement all stay in the runtime, as the roadmap requires. The lowering
   also never relies on `llvm.coro.destroy` to mean cancellation. Cancellation
   is observed at an operation, and the frame finishes through a declared outcome.
   The frame is destroyed only after its join, or by settlement after a fault.
3. **Optimization.**
   - At O2 the child ramps are inlined into their spawner.
   - CoroElide did not remove the heap allocation of a frame created and
     destroyed by a direct await. The handle passes through the runtime, so this
     is expected.
   - Elision would need frames owned and destroyed in the same function, which
     is a later optimization, not a correctness requirement.
4. **Integration cost.** Rust cannot call the coroutine intrinsics. A production
   lowering therefore needs one of two approaches:
   - Emit LLVM IR for async control bodies directly and link it with the
     Rust-generated executable, keeping the current runtime in Rust behind a C ABI.
   - Emit all of the native program as LLVM IR.

   Either way the LLVM version must be pinned, because coroutine lowering is not
   guaranteed to be compatible across releases. Intrinsic signatures also differ
   between releases: in LLVM 22, `llvm.coro.end` takes a token and returns void.
5. **Not covered by this experiment.**
   - Lowering is by hand, not generated from checked IR.
   - Boundaries and contingency recovery (local settlement that resumes an
     owner) are not covered.
   - The original three specimens do not cover nested direct-await faults; B1.4a below adds this case for both lowerings.
   - There are no live adapters, launch signals or debug information.

## Original E2.1 recommendation (superseded by B1)

**Go, staged.** The coroutine model expresses spawn, joins, task sets, cooperative
cancellation, borrowed captures and fault settlement with the reference schedules
and no hidden semantics. Suggested E2.2 scope:

- Generate this lowering from the checked IR for the async subset these specimens
  use, behind a build flag.
- Keep the Rust runtime and call it through a C ABI.
- Compare against the reference executor with the existing scenario, fuel and
  quota matrices before any production use.


## Frontend frame comparison (B1.4a)

The author selected explicit frontend state machines on 9 November 2026. Release
0.250 measures that model before generated checked-IR lowering. The original
coroutine artifacts remain the comparison baseline; production LLVM still admits
synchronous programs only, and Rust remains default.

`frontend-{cancellation,sets,fault,nested-fault}.ll` contain hand-written typed frames
and resume-state switches, with `frontend.ll` providing the common dispatch header.
There are no coroutine intrinsics in these files. A direct-await frame is a field
inside its parent's struct. Root/spawned frames use the shared runtime's stable bump
arena; the runtime retires frames individually and frees arena chunks after domain
settlement. This bounded experiment uses one 4096-byte chunk per run, not a production
allocator or a claim that arena space is fully utilized.

`nested-fault.tellur` and both corresponding lowerings add a fault inside an awaited
child. Cleanup destroys the inline/heap child before its parent. Runtime checks reject
retiring a parent that still owns an inline child, duplicate retirement, unjoined
borrowers and surviving frames/loans. Direct-await entry hands off before the child
starts: the new reference comparison caught that missing handoff in the initial hand
lowerings. The cancellation baseline also spells out that handoff now; its prior
root-only trace could not expose the omission.

The same runtime retains FIFO scheduling, scripted readiness, sticky cancellation,
non-interruptible joins, set completion order and fault settlement in both models.
The 12 cancellation, 64 task-set, 6 sibling-fault and 6 nested-fault schedules match
the reference through clang22 and embedded LLVM/LLD22.1.8, at O0 and O2 (704 executions).
Mutations exercise missing joins and missing/duplicate inline cleanup. These tests
still omit generated lowering, resource destructors inside arbitrary frames, local
boundary recovery, live adapters, launch signals and debug information.

### Reproducing the measurements

```sh
CARGO_INCREMENTAL=0 TELLUR_REQUIRE_COROUTINE_EXPERIMENT=1 \
  cargo test --features llvm --test coroutine_experiment
python3 experiments/coroutines/measure.py --out /tmp/tellur-frame-measurements
```

The measurement script uses `bin/tellurc llvm-smoke` and its embedded LLVM/LLD22.1.8
for both models. Homebrew clang22.1.4 only translates the shared C test runtime and
benchmark driver to IR. All inputs, tool versions and hashes are recorded in
[the raw evidence](measurements-0.250.json). The unchanged 0.249 compiler engine was
used before the 0.250 metadata bump. The machine was macOS arm64; these numbers do
not predict other targets or larger programs.

`benchmark.c` runs each scenario 100,000 times per sample, with five samples and
alternating model order. Its monotonic timer excludes process launch, script parsing,
1001 warmup runs and output. It includes runtime reset, allocation/free, scheduling,
scripted operations, hashed event text, result checks and settlement. Every model
produces the same event checksum and status. No compiler/test job was run concurrently
with the recorded serial measurement; ordinary machine activity is not controlled.
The table shows medians, not confidence intervals or general speedup claims.

| Specimen | O0 coroutine / frontend (ns/run) | O2 coroutine / frontend (ns/run) |
| --- | ---: | ---: |
| cancellation | 834.6 / 805.6 | 525.6 / 494.3 |
| sets | 961.3 / 887.5 | 617.0 / 570.6 |
| fault | 517.3 / 387.3 | 248.5 / 230.1 |
| nested-fault | 437.4 / 380.2 | 264.9 / 230.9 |

At O2, live frame bytes (coroutine → frontend) are cancellation 192 → 184, task sets
128 → 160, sibling fault 88 → 112 and nested fault 120 → 144. Direct awaits remove one
separate frame allocation. The coroutine runs request 3–4 heap allocations totaling
88–192 bytes; every frontend run requests one **4120-byte** arena chunk including its
header. These requested sizes exclude allocator metadata, runtime tables and stack
storage. Fewer allocation calls cost more reserved memory in these tiny scenarios.

O2 executable text bytes are cancellation 8148 → 7972, sets 7660 → 7548, fault
7468 → 7440 and nested fault 7800 → 7940. Whole Mach-O binaries are slightly larger
with explicit frames (112–192 bytes in these O2 samples); symbol/link alignment means
binary size and instruction size need not move together. The raw record includes
both levels, every sample and allocation counts/peaks.

Proceed with the accepted frontend model and private async ABI. Measure the eventual
generated lowering again: these results include this C scheduler and its exact
lifetime instrumentation, not the production Rust runtime. Keep the whole-language
parity gate before changing the default backend.
