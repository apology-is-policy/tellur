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
  language-design fork, with a recommended option first. Known forks that need the
  author: persistent sharing eligibility, fallible sorting, new generic origin
  capabilities, array windows, generic map-key constraints, and the mutation
  keyword (`mutable` is provisional).
- Record a failure as a failure. Never describe unverified work as done, and never
  claim Rust parity that was not measured on the complete Villanelle path. Keep
  measured gaps visible in docs.
- Reuse valid evidence for unchanged inputs instead of rerunning suites.
- Accepted design decisions are recorded in `docs/language-revision.md`
  ("Decisions carried forward"). Do not relitigate them.
- Commit and push only within the authorization the author has given.

## Where the truth lives

Prefer, in order: the latest instruction from the author, current repository
state, then `docs/language-revision.md`. Older handoff notes lose to newer facts.

| Source | Holds |
| --- | --- |
| `docs/language-revision.md` | The roadmap. "Active checkpoint queue" is newest-first; later sections hold history, decisions and the six language milestones. |
| `docs/collections-and-algorithms.md`, `library/COLLECTIONS.md` | Standard-library design, contracts and the measured performance table. |
| `docs/reference/` | The maintained manual. `maintenance.md` is the per-checkpoint checklist, `support-status.md` the feature matrix, `llvm-backend.md` the backend reference. |
| `docs/conformance.md` | Requirement-to-evidence inventory. |
| `docs/path-language-vision.md`, `docs/path-language-spec-v0.1.md` | Original vision and spec. Unchanged on purpose. |
| `experiments/*/README.md` | Performance and representation experiment reports, including rejected candidates. |

## Roadmap

The roadmap is a sequence of stations. States below are as of compiler 0.306;
the "Active checkpoint queue" is authoritative for the current position.

| # | Station | State |
| --- | --- | --- |
| 1–7 | Accepted design, runtime C ABI, embedded LLVM/LLD, synchronous emission, async frames, Rust adapters and live I/O, application and suite parity | Done (0.233–0.277) |
| 8 | Single binary and CI releases (`vnc` plus `vnc lsp`, macOS and Linux musl packs) | Done (0.278–0.283) |
| 9 | Windows target (COFF, import libraries) | Shipped in 0.283 |
| 10–11 | Language review against the six original milestones; JSON outcome expansion comparison | Done |
| 12–13 | Collection and algorithm contracts; traversal and storage experiments | Done (0.284–0.306) |
| 14 | Fresh-project library adoption | Active |
| 15 | First supported library modules | Next |
| 16 | Thylacine target (`aarch64-thylacine`, LLVM fork, pouch link pack, `poll()` readiness) | Deferred until the author asks |
| 17 | Retire the generated-Rust backend | Deferred until the author asks |

Standing goal across all stations: match or beat equivalent optimized Rust for
every operation. Generic value/reference transport, checked payloads and the
136-byte generic value are the main remaining costs.

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
| `ci/`, `.github/workflows/` | Release qualification, Windows, Linux and Thylacine pack scripts |
