# Tellur compiler and playground

The language is now named **Tellur** (formerly the Path placeholder). Release 0.36
uses `.tellur` sources, `tellurc`, `tellur-lsp`, and `tellur.code-workspace`. Existing
`.path` programs and the `pathc` / `path-lsp` commands remain supported. See the
[migration notes](docs/language-reference.md#tellur-name-and-migration).

Compiler release **0.55.0** implements the v0.1 checked core and the current language
revision: `control`, `implement`, snake_case types, type-first declarations,
`ref` types, `borrow`/`deref` expressions and transparent reference field access,
brace outcome patterns, private `exits auto` and selected payload inference (`ready...`), explicit public contracts, pure conversion expressions, move-by-default
owned values, mutation, shared/exclusive references with checked borrowed results and explicit `from input` origins,
source structs and nested variants/outcomes with local reference fields, checked opaque owner splits, exclusive contained reborrows, whole-record inputs and contained origins (`input['a]`), grouped record results (`text_pair[a, b]`) and field-sensitive last use, ordinary variants, generic data and controls with explicit copy/clone/drop bounds, concrete and generic trait dispatch with associated types, named struct fields, file modules with private representations, arrays, vectors and maps, implicit entry statements, nested tail and stored adapters,
named outcome contracts, checked `forward` routing, and single-completion call statements. It includes
a reference interpreter, graph expansion, native compiler, language server, and
VS Code build/run commands. The [revision plan](docs/language-revision.md) separates
delivered features from the remaining reference, slice, outcome-expression and async work. The language is still
fluid; this is an experimental implementation, not a production compiler.

The [active checkpoint queue](docs/language-revision.md#active-checkpoint-queue) now
prioritizes stable public interfaces, diagnostics, readable paths and a sustained
application with Rust library adapters. Further reference expansion is deferred;
first-class async and direct LLVM coroutine lowering remain architectural targets.
Release 0.46 adds [outcome contributor diagnostics](docs/reference/diagnostics-and-limits.md)
through stored values, adapters and merged path labels. Release 0.47 adds
`tellurc check SOURCE --json` and structured path witnesses for tools.
Release 0.48 adds [explicit process exit contracts](docs/reference/process-entry.md):
`@exit N` on named alternatives, checked entry overrides, and exact status propagation
through `tellurc exec`. Runnable playgrounds now declare their status policy.
Release 0.49 adds [retained Rust resources](docs/rust-adapters.md#retained-resources-049):
opaque owners survive between calls, with checked borrowing and explicit consumption.
Try the [file adapter](examples/retained_file/README.md) in VS Code or the terminal.
Release 0.50 adds the [loopback HTTP settings service](applications/settings/README.md#run-the-http-service),
with Tellur routing, validation and cleanup, bounded socket I/O and explicit shutdown
through connection budgets or idle timeouts.
Release 0.51 adds [precise expression diagnostics](docs/reference/diagnostics-and-limits.md#expression-locations)
to the compiler and VS Code: bad arguments, initializers and field accesses select
their source expressions, including multiline strings and imported generic code.
Release 0.52 adds [independent-body error reporting](docs/reference/diagnostics-and-limits.md#independent-body-errors):
`check` and VS Code show multiple independent errors while suppressing callers of
failed controls. Inferred hover remains unavailable until the current program checks.
Release 0.53 adds [optional path grouping](docs/language-reference.md#labels-and-optional-grouping):
`[path]: { ... }` gives a selection an explicit end without creating a new value or
lifetime scope. Ordinary labels stay unbraced. Try [path_groups.tellur](playground/path_groups.tellur).

Release 0.54 adds [direct arm exits](docs/language-reference.md#direct-arm-exits):
`missing => exit absent` immediately completes that history without a throwaway path
label. The settings application now uses them for validation and storage failures.
Try [arm_exits.tellur](playground/arm_exits.tellur).

Release 0.55 adds [selected-history hover](docs/language-reference.md#selected-history-hover):
hover over selected code to see resolved paths, enclosing intersections, and decision
origins. Try [history_hover.tellur](playground/history_hover.tellur). Existing `auto`
hover keeps showing inferred types.

The [semantic foundation](docs/semantic-foundation.md) distinguishes compatibility,
ownership transfer, behavioral equivalence and provenance. Executable law tests and
`tellurc compare` provide finite scenario evidence; they do not establish general equivalence.
Expansion now also carries [independently checked rewrite certificates](docs/expansion-certificates.md).
Every expanded execution/build passes this validator; artifacts can be exported and checked separately.
The [simulation argument](docs/expansion-simulation.md) records its semantic premises.
Try [explicit public contracts and private forwarding](playground/public_contracts.tellur),
[local shared-reference collections](playground/borrowed_collections.tellur),
[aggregate trait origin contracts](playground/aggregate_traits.tellur),
[public nested-reference contracts](playground/public_carriers.tellur),
[stored carrier references](playground/stored_carriers.tellur),
[returned carrier references](playground/carrier_results.tellur),
[whole sum borrowing](playground/sum_carriers.tellur),
[named outcome and option origins](playground/outcome_origins.tellur),
[public variant contracts](playground/public_variants.tellur),
[borrowed outcomes](playground/borrowed_outcomes.tellur),
[opaque owner splits](playground/opaque_splits.tellur),
[exclusive carrier reborrows](playground/carrier_reborrows.tellur),
[record inputs and contained origins](playground/record_inputs.tellur),
[inferred exit payloads](playground/inferred_payloads.tellur),
[stored reference fields](playground/stored_references.tellur),
[borrowed trait contracts](playground/borrowed_traits.tellur),
[named struct construction](playground/named_fields.tellur) for readable nested data
and [file modules](docs/modules.md) for public interfaces and private representations.

[Protocol analysis](docs/protocol-analysis.md) now provides finite structural models,
residual queries and completed-trace inclusion with counterexamples. Its abstraction
forgets data and cannot establish concrete program equivalence.
[Source tail extraction](docs/extraction.md) now generates a typed helper and rechecks
its ownership and public contracts; it is a conservative refactoring, not a proof.

The maintained [settings service](applications/settings/README.md) now provides a
working persistent CLI and sequential loopback HTTP server. Its Tellur modules use
[typed Rust adapters](docs/rust-adapters.md) for file, JSON and socket operations,
with explicit Cargo dependencies and locked builds.
This starts library reuse while keeping first-class Tellur async and direct LLVM
coroutine lowering open.

## Build from a fresh clone

Install Rust/Cargo, then run:

```sh
git clone https://github.com/apology-is-policy/tellur.git
cd tellur
cargo build --locked --release --bins
./target/release/tellurc check playground/hello.tellur
./target/release/tellurc exec playground/hello.tellur
./target/release/tellurc build playground/hello.tellur --release
./playground/build/hello
```

Generated binaries and VSIX packages are not committed. Use `target/release/tellurc`
in place of `bin/tellurc` in the examples below, or copy the release binaries into
`bin/`. To build the editor extension, follow the
[VS Code development guide](editors/vscode/README.md). The project is licensed under
[Apache 2.0](LICENSE).

## Try programming

1. Open `tellur.code-workspace` in VS Code.
2. Open `playground/hello.tellur`.
3. Click **▶** or run **Tellur: Build and Run** (`⌘⌥R`).
4. Read the **Tellur Build** output, then try `stored_references.tellur`, `generic_controls.tellur`, `generic_data.tellur`, `named_fields.tellur`, `modules/main.tellur`, `data_types.tellur`, `forwarding.tellur`, `named_outcomes.tellur`, `stored_adapters.tellur`, `adapters.tellur`, `outcomes.tellur`, `collections.tellur`, `references.tellur`, `ownership.tellur`, `count.tellur`, `port.tellur`, or `read_file.tellur`.

Hover over a control declaration name to see its checked exit contract, including
inferred payload types. The [detailed language reference](docs/language-reference.md) explains the syntax,
ownership rules, primitive library, editor settings and command-line workflow. Its
companion chapters cover [grammar](docs/reference/grammar.md),
[diagnostics](docs/reference/diagnostics-and-limits.md),
[terminology](docs/reference/glossary.md) and [feature status](docs/reference/support-status.md).
Complete manual examples are checked with `cargo test --offline --test reference_manual`.
The local `tellur-language-0.48.0-darwin-arm64.vsix` contains the compiler and language
server for Apple Silicon macOS. It is installed on this machine; elsewhere use
**Extensions: Install from VSIX…**. Native builds need Rust/Cargo; the current
machine's toolchain and dependencies have been prepared. The resulting binaries
run independently of the compiler and Rust installation.

The playground uses **real** parsing, arithmetic, text and read-only file operations.
The broader `examples/` directory also includes scripted network/retry cases and
negative-edit suggestions. An entry `.tellur` file and its explicit imports form a
compilation; helpers can live in that file or in imported modules. Put new files beside `playground/manifest.json` to use
its types and primitive contracts.

## Command line

From this directory, use the supplied binaries:

```sh
./bin/tellurc check playground/hello.tellur
./bin/tellurc exec playground/forwarding.tellur
./bin/tellurc exec playground/adapters.tellur
./bin/tellurc exec playground/outcomes.tellur
./bin/tellurc exec playground/collections.tellur
./bin/tellurc exec playground/count.tellur
./bin/tellurc exec playground/port.tellur --entry parse_port -- 443
./bin/tellurc build playground/hello.tellur --release
./playground/build/hello
./bin/tellurc expand examples/refresh.tellur examples/manifest.json refresh
./bin/tellurc certify-expansion playground/forwarding.tellur playground/manifest.json main > expansion.json
./bin/tellurc verify-expansion playground/forwarding.tellur playground/manifest.json expansion.json
./bin/tellurc protocol playground/read_file.tellur playground/manifest.json show_file playground/protocol/read_failed.json
./bin/tellurc run examples/load.tellur examples/manifest.json examples/double-failure.json
```

`build` emits an executable, LLVM IR (`name.ll`), and the generated intermediate
source (`name.tellur-build/src/main.rs`). `--expanded` expands the chosen entry before
lowering; `--release` optimizes at LLVM level 2. Native executables accept `--json`,
`--entry`, `--scenario FILE`, and an optional `--fuel N` host limit. All declared
outcomes are normal completions; host errors return nonzero process status.

The native pipeline is checked graph → explicit Rust control flow → LLVM → native
binary. Controls become functions and loops remain loops. Executables do not contain
the graph interpreter. Rust is the implementation bridge to LLVM and supplies value
storage. LLVM verification runs on every build. Generated code retains all declared
outcomes rather than replacing inconvenient paths with undefined behavior.

The tested toolchain is Rust 1.97.1 with LLVM 22.1.6 on Apple Silicon macOS. Generated
program dependencies are pinned in an embedded Cargo.lock. Other platforms are not
yet validated. Foreign adapters are finite: a new manifest primitive can be scripted,
but requires an implementation before it can run live.

## Develop and verify

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bins
```

Tests include native compilation, so Cargo/Rust must be available. Set
`TELLUR_NATIVE_TARGET_DIR` to share a native dependency cache across builds; otherwise
it lives in `.tellur-cache` beside output binaries. `--offline` can be used for Cargo
commands once dependencies are cached. Compilation artifacts are ignored by Git.

The [conformance inventory](docs/conformance.md) maps the draft's requirements and
C01–C24 cases to implementation and evidence. Tests cover ownership, loops,
provenance, typed declarations, inferred contracts, pure calls, malformed inputs,
generated programs, native versus interpreted behavior
before/after expansion and O0/O2 optimization, live host adapters, and actual editor
build/run. This is finite evidence, not a soundness or termination proof.

The [VS Code development guide](editors/vscode/README.md) describes the extension
and its integration runner. The installed release bundles both `tellurc` and
`tellur-lsp`. Rebuild and override their paths when working on compiler code.

## Design and boundaries

The original [specification](docs/path-language-spec-v0.1.md) and
[vision](docs/path-language-vision.md) are unchanged. The
[design notes](docs/design-notes.md) record issues worth revisiting. The
[addendum](docs/v0.1-addendum.md) explicitly fixes the implementation profile rather
than hiding restrictions in the code.

Ordinary owned values and must-complete resources are checked separately. Strings
and bytes move in `control`; copying owned data requires `clone`. Shared/exclusive
references cross control boundaries and can be returned from input references.
Local lifetimes end at last use, with conservative path and loop rules. Hover shows
which inputs a returned borrow depends on. Grouped records and source variants carry
references through owned control inputs and results. Local borrowed variants/outcomes
retain references through capture, adapters and delta unpacking. Named-outcome and
option schemas support owned boundaries and whole borrowed inputs. Borrowed matching
reborrows active payloads without discharging handling obligations. Trait contracts
carry explicit storage and contained-origin mappings through generic dispatch.
Local arrays, vectors and maps support shared-reference elements. Exclusive elements,
nested/public collection contracts, reference rebinding and reference loop parameters
remain upcoming.
Historical `region` syntax retains reusable values for compatibility. Borrowed slices,
async tasks remain upcoming. Other
future work includes recursion, concurrency, richer proofs, general graph
extraction and the graphical editor. Constant constructors do not refine path
coverage or resource obligations. The editor reports bounded independent-control errors (one per body, up to 32).

See [generic control checking](docs/generic-controls.md) for the definition/specialization
boundary and the implemented capability rules.
[Concrete traits](docs/traits.md) add coherent implementations and explicitly selected
static calls; try `playground/traits.tellur` and `playground/trait_bounds.tellur`.
Generic bodies are checked against declared trait operations and associated permissions.

## Code map

| Location | Responsibility |
| --- | --- |
| `src/syntax.rs`, `src/manifest.rs` | Syntax and trusted type/primitive contracts |
| `src/bdd.rs`, `src/check.rs`, `src/inference.rs` | Symbolic path analysis, typing, coverage and ownership |
| `src/generics.rs`, `src/generic_controls.rs`, `src/traits.rs` | Concrete layouts, generic definition checks, trait conformance and static dispatch |
| `src/ir.rs`, `src/verify.rs` | Checked representation, provenance, inspection and structural verification |
| `src/expand.rs`, `src/runtime.rs` | Region expansion and scripted reference execution |
| `src/extract.rs` | Rechecked source tail extraction |
| `src/modules.rs` | File imports, identities, visibility and source maps |
| `src/native.rs`, `src/native/runtime.rs` | Native lowering, LLVM toolchain bridge and host adapters |
| `src/main.rs` | Check, interpret, inspect, expand, build and execute commands |
| `src/lsp.rs`, `editors/vscode/` | Editor diagnostics, highlighting, build/run and integration tests |
| `tests/` | Conformance, regressions, generated/differential tests and live host tests |
| `playground/` | Small live programs to edit and run |

The latest borrowing example is [origin_groups.tellur](playground/origin_groups.tellur).
It demonstrates separate returned-field dependencies and inferred forwarding.
