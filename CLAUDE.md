# Villanelle

Villanelle is an experimental language built around `control`s with explicit named
outcomes; a `delta` routes each outcome to a path. This repository holds the Rust
compiler (`vnc`: checker, reference interpreter, direct LLVM backend, legacy
generated-Rust backend, formatter, language server), the precompiled runtime, the
source library, the VS Code extension and the maintained settings application.

The language was called **Path** (to 0.35), then **Tellur** (0.36–0.284), and
**Villanelle** since 0.285. Old names are still load-bearing: the library crate is
`tellur`, environment variables are `TELLUR_*`, build directories are
`*.tellur-build`, the extension package is `tellur-language`, and `tellur`,
`tellurc`, `pathc` and friends remain as compatibility commands. Do not rename
these in passing. Sources are `.vn`; `.tellur`/`.path` files are legacy snapshots.

Source style and the native performance rule are in `AGENTS.md`:

@AGENTS.md

Machine-specific paths, authorizations and the current working state belong in a
git-ignored `CLAUDE.local.md` beside this file. Keep them out of this one.

## Hard rules

- Stage files by explicit path; never `git add -A` or `git add .`. A checkout can
  hold untracked files that are private to the author. Do not read, stage or copy
  an untracked file you did not create.
- Never reload or restart the author's existing VS Code windows. Editor tests run
  in an isolated VS Code instance and workspace.
- Do not rebuild LLVM when a pinned static LLVM/LLD 22.1.8 install exists; a cold
  build takes over an hour. Point `LLVM_SYS_221_PREFIX` at the existing install.
- Build with `CARGO_INCREMENTAL=0`. Run one heavy build or test suite at a time
  and check free disk space before large builds.
- Agent configuration (`.claude/`, `.opencode/`, `.mcp.json`, `CLAUDE.md`,
  `CLAUDE.local.md`, `AGENTS.md`) is excluded from source freezes and backups.

## Working with the author

- Proceed through routine checkpoints autonomously. Ask only at a real
  language-design fork, with a recommended option first. The exit of each plan
  phase that says "the author accepts" is such a point.
- Record a failure as a failure. Never describe unverified work as done, and never
  claim Rust parity that was not measured on the complete Villanelle path. Keep
  measured gaps visible in docs.
- Reuse valid evidence for unchanged inputs instead of rerunning suites.
- Accepted design decisions are recorded in `docs/redesign-plan.md` (section 3
  and section 5.5, for v2) and in `docs/language-revision.md` ("Decisions carried
  forward", for v1). Where they differ, the plan governs v2. Do not relitigate
  them.
- Commit and push only within the authorization the author has given.

## Where the truth lives

Prefer, in order: the latest instruction from the author, current repository
state, then `docs/language-revision.md`. Older handoff notes lose to newer facts.

| Source | Holds |
| --- | --- |
| `docs/redesign-plan.md` | The roadmap: the accepted v2 redesign, its decisions, phases and design sketches. Section 5.5 is the adopted Rails surface syntax. |
| `docs/v2/` | The v2 specification: core calculus first, then fact logic, surface, reference model and the corpus. `README.md` there lists the state of each. Accepted on 20 November 2026; corrected when the oracle, the checker or a review finds an error, with the change recorded in the document. `oracle.md` describes the v2 crate, the core text form and the conformance suite. |
| `docs/language-revision.md` | The v1 record. "Active checkpoint queue" is newest-first; later sections hold history, decisions and the six language milestones. |
| `experiments/syntax_lab/` | Reference programs in Rails form, their word-form sources and the transpiler. Nothing in it compiles yet. |
| `docs/collections-and-algorithms.md`, `library/COLLECTIONS.md` | Standard-library design, contracts and the measured performance table. |
| `docs/reference/` | The maintained manual. `maintenance.md` is the per-checkpoint checklist, `support-status.md` the feature matrix, `llvm-backend.md` the backend reference. |
| `docs/conformance.md` | Requirement-to-evidence inventory. |
| `docs/path-language-vision.md`, `docs/path-language-spec-v0.1.md` | Original vision and spec. Unchanged on purpose. |
| `experiments/*/README.md` | Performance and representation experiment reports, including rejected candidates. |

## Roadmap

**v1 is frozen** at compiler 0.306.0 and collections 0.1.0 (19 November 2026): no
further performance passes, library modules or target work. v1 stays buildable,
receives corrections, and becomes the test oracle for v2. Its seventeen stations
are history in `docs/language-revision.md`; the Thylacine target and the
retirement of the generated-Rust backend were never started.

The roadmap is the accepted redesign, **v2**, in `docs/redesign-plan.md`. The
plan's phase sections are authoritative for scope and exit conditions.

| Phase | Work | State |
| --- | --- | --- |
| 0 | Close out v1 | Done; tagged `v1-final` |
| 1 | Design on paper: core specification, fact logic, reference model, surface, the corpus rewritten by hand, a reading and repair test. No compiler code. | Done; accepted 20 November 2026 |
| 2 | Oracle interpreter, program generator, differential harness, mutation tests | Active: slice 1, scalars and decisions |
| 3 | Checker on a typed core, with the formatter that owns the rails | |
| 4 | Typed backend; exit is 1.2× optimized Rust on every kernel under `experiments/` | |
| 5 | Language breadth: numbers, functions, loops as decisions, recursion, closures, recursive data, views | |
| 6 | Algebra at compile time: symbolic automata, policies, resource protocols, behavioral contracts | |
| 7 | Editor and migration | |
| 8 | Evidence: a 5,000 to 10,000 line application, repair benchmark, reading study | Continuous |

Phases 2 to 4 are built as vertical slices (scalars and decisions, then data,
then borrows, then tasks); each slice passes specification, oracle, checker,
backend and differential fuzzing before the next begins.

v2 source is written in **Rails** (plan section 5.5): glyphs and box-drawing
rails are the canonical text, and the word form is the input spelling. New
design text and examples use Rails. Existing `.vn` files are v1 and keep v1
syntax and the style in `AGENTS.md`.

Standing goal: match or beat equivalent optimized Rust for every operation. In
v1 the generic value/reference transport, checked payloads and the 136-byte
generic value are the measured costs; they are the baseline for phase 4.

## Build and test

The default `llvm` feature needs a static LLVM 22.1.8 install that includes LLD:

```sh
export CARGO_INCREMENTAL=0 LLVM_SYS_221_PREFIX=/path/to/llvm-22.1.8
```

```sh
cargo test --offline --test reference_manual      # manual examples, links, version marker; seconds
cargo test --test collections --test llvm_backend --test modules --test native_host --test reference_manual
cargo test --locked                               # everything; long, run it alone
TELLUR_TEST_BACKEND=rust cargo test --locked --test settings_application   # or =llvm
cargo clippy --locked --all-targets -- -D warnings
cargo fmt
cargo build --locked --release --bin vnc          # add --no-default-features for a Rust-only bootstrap
python3 -B ci/check-collections.py --vnc /absolute/path/to/vnc   # library adoption check
(cd editors/vscode && npm test)                   # grammar and configuration tests
```

The v2 crate builds on its own, in seconds, and needs no LLVM:

```sh
(cd v2 && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test)
(cd v2 && cargo run --bin vn2 -- test tests/slice1/*.core)   # or: run FILE ENTRY VALUES, print FILE
```

Set `TELLUR_NATIVE_TARGET_DIR` to share the native dependency cache between
builds. `./bin/vnc` is the installed release compiler when present; otherwise use
`target/release/vnc`. `vnc --help` lists every command:

```sh
vnc check playground/hello.vn
vnc exec playground/hello.vn                      # add: -- ARGS, --json
vnc build FILE --release [--backend rust|llvm] [--target x86_64-unknown-linux-musl]
vnc run SOURCE MANIFEST SCENARIO [--expanded]     # reference interpreter
vnc format FILE --write
vnc lock-native FILE                              # after changing Rust adapter dependencies
```

LLVM is the default backend on macOS arm64, Linux x86_64/arm64 and Windows x64;
`--backend rust` selects generated Rust. A plain build needs only `vnc`. Rust
source adapters also need Cargo. `bin/`, `*.vsix`, `**/build/` and
`*.tellur-build/` are ignored build outputs.

## Checkpoint discipline

Every implementation checkpoint is a small release (0.3xx). A change to compiler
or editor inputs runs the full workflow. A source- or docs-only checkpoint reuses
the current release evidence and needs no version bump or reinstall.
This workflow governs corrections to v1, and v2 work from the point where v2
code is linked into `vnc`.

Until then v2 is the crate in `v2/`, which is not an input of the released
compiler. A v2 checkpoint needs: tests that accept and tests that reject for
every clause it implements, `cargo fmt --check`, `cargo clippy -D warnings`
and `cargo test` in `v2/`, the documents under `docs/v2/` brought up to date
(a question the code asks of the specification is answered in the
specification, with an entry in that document's list of changes), and a green
`v2` workflow after the push. It needs no version bump of the compiler, no
release build and no reinstall: the v1 evidence stands for unchanged inputs.

1. Implement with positive tests and rejection tests, a runnable example, and
   agreement between interpreter, LLVM and generated Rust, original and expanded,
   at O0 and O2 where it applies.
2. Follow `docs/reference/maintenance.md`: update the reference, grammar, feature
   matrix and conformance entries, and add an entry at the top of "Active
   checkpoint queue". Put proposed syntax in design notes, never in checked
   examples.
3. For performance work, follow `AGENTS.md`: paired runs against the previous
   compiler and optimized Rust, with allocations, requested and peak bytes and
   data movement. Reject candidates that regress, and record the rejection in
   `experiments/`.
4. Bump the version together in `Cargo.toml`, `Cargo.lock`,
   `editors/vscode/package.json` and its `package-lock.json`, the version marker
   in `docs/language-reference.md`, `docs/reference/support-status.md`, the README
   and the VSIX name in `editors/vscode/README.md`. `reference_manual` checks that
   the marker matches Cargo.
5. Qualify the release: tests, release build, package, `ci/check-release.py`,
   paired measurement, Linux cross-links, isolated editor tests, install, source
   freeze.
6. Commit with a descriptive subject (see `git log`), push when authorized, and
   check CI with `gh run list`.
7. Update the roadmap and handoff records so the next session, in any agent, can
   resume from them.

## Context economy and handoff

Adapted from the Thylacine operational contract.

- **The compaction line is 400k tokens** (`.claude/ctx-thresholds`, read by the
  context hook). Below it, run through checkpoints without stopping. At the
  line, carry the current step to a clean boundary and hand off.
- **Checkpoint contract.** At every resting point: account for every shell,
  monitor and background task, or say nothing is running; keep the handoff
  already written, not offered, so the author can compact at any moment; and say
  what is next in one `Next` line and one `Ahead` line.
- **Handoff.** At every session boundary update the state and the pickup pointer
  (in `CLAUDE.local.md`, plus the shared records it names), including a
  **Working set**: the files to load first, in order, one `- path` each
  (`path:A-B` for a window). Commit the updates. Do not land partial work just
  to close a chunk.
- **Resume note.** Make the last message before a compaction the resume note:
  what is in flight, what must not be redone, and the working set, in at most
  8,000 characters. It is re-injected after the compaction.
- **After a compaction** load the working set first, and decide what else is
  needed only then. Do not re-survey the project.
- **Every call re-reads the whole context.** Batch independent calls and
  bookkeeping writes; never poll a background job; cap output (count before
  dumping, read windows not whole files); load deferred tools once.
- **Compact before researching the next chunk**, not after, when a chunk closes
  within about 100k of the line. Do not leave a session idle at high context.
- **Stopping versus finishing.** A full summary is for stopping: the compaction
  line, a design fork, a real block, or the author asking. At a checkpoint that
  is run through, give three lines (what landed, what is running, what is next)
  and continue.
- **Self-compaction.** Where `CLAUDE.local.md` names a self-compaction tool and
  the session runs in tmux, use it at the line: commit, write the resume note
  to the tool's note slot, run its check, then run it and end the turn. It
  refuses a tracked tree with uncommitted changes, a stale or oversized note, a
  working set with a missing path, and a second compaction with no new commit.
  Without the tool, write the resume note and recommend `/compact` to the
  author.

## Code map

| Location | Responsibility |
| --- | --- |
| `src/syntax.rs`, `src/manifest.rs`, `src/modules.rs` | Syntax, trusted primitive contracts, file imports |
| `src/check.rs`, `src/check/`, `src/bdd.rs`, `src/inference.rs`, `src/origins.rs`, `src/lifetimes.rs` | Path analysis, typing, coverage, ownership and origins |
| `src/generics.rs`, `src/generic_controls.rs`, `src/traits.rs` | Layouts, generic checks, trait dispatch |
| `src/ir.rs`, `src/verify.rs`, `src/expand.rs`, `src/certificate.rs` | Checked IR, verification, expansion and certificates |
| `src/runtime.rs`, `src/runtime/` | Scripted reference interpreter |
| `src/native.rs`, `src/native/` | Native lowering and runtime for both backends |
| `src/llvm.rs`, `src/llvm/lld_shim.cpp`, `build.rs` | Embedded LLVM and in-process LLD |
| `runtime/` | Precompiled runtime crate and target link packs |
| `src/lsp.rs`, `src/lsp/`, `src/format.rs`, `editors/vscode/` | Language server, formatter, extension |
| `src/main.rs`, `src/bin/` | CLI; `vnc` is the canonical binary |
| `library/` | Source library: eight collection modules, plus experimental JSON and loopback HTTP |
| `applications/settings/` | The one maintained real application (settings service); also the style reference |
| `examples/`, `playground/`, `experiments/` | Runnable examples, small live programs, performance and representation experiments |
| `tests/` | About 115 integration suites: conformance, differential, native and live host tests |
| `v2/` | The v2 crate (`villanelle-v2`, binary `vn2`), with no dependencies: mathematical integers, the core text form, `resolve` to the typed core, the oracle and its hosts, and the conformance suite under `v2/tests/` |
| `ci/`, `.github/workflows/` | Release qualification, Windows, Linux and Thylacine pack scripts |
