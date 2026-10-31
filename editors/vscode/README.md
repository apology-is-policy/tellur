# Tellur Language for VS Code

Experimental editor support for the Tellur compiler.

## Start reading code

1. Install the supplied `tellur-language-0.64.0-darwin-arm64.vsix` using **Extensions:
   Install from VSIX…** in VS Code's Command Palette. This package includes `tellur-lsp`
   for Apple Silicon macOS. No Rust or npm installation is needed to use that package.
2. Open `tellur.code-workspace` from the compiler checkout.
3. Open `playground/hello.tellur` and click **▶** or **Tellur: Build and Run** (`⌘⌥R`).
   Try `playground/named_outcomes.tellur`, `stored_adapters.tellur`, `adapters.tellur`, `outcomes.tellur`, `collections.tellur`, `references.tellur`, and `count.tellur` next.
4. Read `docs/language-reference.md` for the detailed language reference.

The package contains both `tellur-lsp` and `tellurc`. Native builds require the Rust/Cargo
toolchain and pinned dependencies; no npm installation is needed. Executables are
standalone. Build/run saves the active file and sends output to **Tellur Build**.
Use **Tellur: Build Native Program** (`⌘⌥B`) to build without running. Windows/Linux
keybindings use Ctrl instead of Cmd; the supplied binary package is macOS arm64.

`tellurLanguage.entryRegion` defaults to `main`; `tellurLanguage.programArguments` is
an array of strings. Each item is passed as one argument, without a shell.
`tellurLanguage.releaseBuild` enables LLVM optimization level 2. Override
`tellurLanguage.compilerPath` to use a development compiler. Build outputs live in
`build/` beside the source; they include executable, LLVM IR and generated Rust.

Build and Run executes your program, including any declared live file operations.
It runs only in a trusted workspace. The language server itself only analyzes source.

Try deleting the final `[ReadBad & CloseBad]` exit in the loader. An `E_UNCOVERED`
squiggle appears at the region declaration; its message includes a structural witness.
Undo the edit and the error clears. Deleting an operation outcome arm reports
`E_ARMS`. These diagnostics also appear in VS Code's Problems panel.

In `partial_moves.tellur`, insert `duplicate := pair;` after `first := pair.left;`.
The compiler reports `E_PARTIAL_MOVE` and names `pair.left`. Reading `pair.label`
or moving `pair.right` remains valid. Undo the edit to clear the diagnostic.

In `optional_handle.tellur`, the comments suggest two edits: reuse the option after
its decision (`E_MOVED`), or use the handle on the absent path (`E_UNAVAILABLE`).
The error names the value and a structural path witness; undo clears it.

The language is named **Tellur**. `.tellur` is the canonical source extension; legacy `.path` files remain supported.

## Features

- TextMate syntax highlighting, including selectors, outcome mappings, `Δ` / `delta`,
  loops, literals, types and comments. It uses your current theme.
- Live diagnostics for unsaved `.tellur` buffers from the actual Rust compiler.
- Hover on `auto` for the full inferred binding type, including references and
  mutability, or the inferred signature for `exits auto`. Updated from unsaved
  source; types are shown only after successful checking.
- Resolved path-selection hover on labels, aliases, arms and executable statements, with bounded decision origins and loop context.
- Region names and signatures in Outline / Go to Symbol in Editor.
- Control, decision, selector, optional path-group, scoped-intersection and loop snippets.
- Bracket matching, automatic closing, indentation and line-comment toggling.
- **Tellur: Restart Language Server**, plus a **Tellur Language** output channel.
- Native build/run commands, toolbar button, shortcuts and **Tellur Build** output.

Highlighting works in restricted workspaces. The language server runs after the
workspace is trusted. It analyzes source; it does not run the source program or
invoke real primitives.

Try `playground/path_groups.tellur` for optional `[path]: { ... }` grouping.
The `path_group` snippet ends a selection explicitly without creating a new lifetime
scope. The ordinary `guard` snippet stays unbraced; `over` creates a lexical scope.

## Manifests and compilation units

Each entry document and its explicit file imports form a compilation unit, just as with
`tellurc check`. Other open `.tellur` or `.path` files are not implicitly imported.
Unsaved imported buffers participate in diagnostics. Imports may mix both extensions.

The server looks for the nearest `manifest.json`, starting beside the source file
and walking toward its workspace root. Set `tellurLanguage.manifestPath` to choose
another file; relative paths are resolved against the workspace folder. Manifest
edits take effect after saving. With no valid manifest, syntax diagnostics still
work and a warning explains that type/coverage/resource checks did not run.

The server reports bounded independent-control errors, debounced during typing,
with one primary error per body and up to 32 per check. Failed callers are suppressed
to avoid cascades. Expression diagnostics select their source ranges; other
diagnostics may point to a token, statement or declaration. Structural
witnesses describe what *may* reach a point, not proven feasible input values.
Declaration hover shows checked control contracts. **Format Document** formats the
current buffer while preserving labels, explicit groups, comments and literals.
Completion, rename and cross-file references are not implemented.

## Formatting (0.61)

Use VS Code's **Format Document** command or editor context menu. The server uses
the current unsaved source and your indentation preference (spaces/tabs, width
1–16), including untitled Tellur buffers. Formatting needs no manifest and works
despite type errors or missing imports. Broken syntax produces no formatting edit;
repair the syntax diagnostic first. Formatting does not save the document.

Set `tellurLanguage.formatWidth` to a preferred width from 40 through 240 (default
100). It applies to the current document without restarting the server. Long
signatures, calls, payloads and outcome lists wrap automatically; conditions put
`&&` and `||` at the start of continuation lines. Width is a soft target measured
in Unicode scalar columns; comments and literals may exceed it.

Flat labels stay distinct. Short authored inline dispositions remain beside their
labels; statements beneath a standalone label are indented. The
formatter preserves every explicit brace, comment and literal, including multiline
strings, and retains multiline headers and payloads. It does not add
braces, sort imports or enable format-on-save. Range formatting, on-type formatting
and comment reflow remain future work. If another formatter is
active, use **Format Document With…** to select Tellur.

## Public contract snapshots (0.57)

Run **Tellur: Show Public Contract Snapshot** with a Tellur file active. It checks
the current unsaved source and imported buffers, then opens a JSON document with
the public contracts. Save As can retain a baseline. The view is a fixed snapshot;
rerun the command after changes. Source files are not saved or modified.

The command needs a file identity, a valid manifest and successful checking. Save
untitled source first. On errors it reports the failure without showing a partial
or cached result. Public fields, outcome payloads, borrowing origins, trait
requirements and copy/clone/drop permissions are recorded; private bodies and fields
are omitted. The CLI's `contract-diff BEFORE_JSON AFTER_JSON --check` can compare a
saved baseline. Equal contracts do not prove equal behavior or binary compatibility.

## Migration from Path

Both `.tellur` and `.path` files use Tellur highlighting and the same compiler.
Old `path.*` command IDs and explicitly configured `pathLanguage.*` settings remain
compatibility aliases. Explicit `tellurLanguage.*` settings take precedence.
Uninstall the old `path-language-local.path-language` extension when installing this
new `tellur-language-local.tellur-language` package, then reload the window. See the
migration notes in `docs/language-reference.md` in the compiler checkout.

## Server settings

`tellurLanguage.serverPath` overrides the server executable. Otherwise the extension
uses its bundled server, then `target/debug/tellur-lsp` from a development checkout,
then `tellur-lsp` on PATH. The packaged server matches the 0.47.0 compiler; rebuild or
override it when experimenting with compiler changes.

For another platform, build the Rust server and set the executable path. The source
extension is platform-independent; the supplied VSIX is explicitly `darwin-arm64`.

## Develop and test

From the compiler directory:

```sh
cargo build --locked --bins
cd editors/vscode
npm ci
npm test
npm run build
```

Open the compiler directory in VS Code and run **Develop Tellur extension** with F5.
The launch task builds the compiler tools and client. Set `tellurLanguage.serverPath`
to your checkout’s `target/debug/tellur-lsp` while developing the compiler, so the
bundled release server does not take precedence. Development dependencies are
needed only to build/test the extension, not to use the packaged VSIX.

The extension-host integration runner is `test/integration.cjs`; it also compiles
and runs a native program, so its environment needs Rust/Cargo and compiler support. Give it an isolated
workspace containing copies of `load.tellur` and `manifest.json`, and configure
`tellurLanguage.serverPath` there. Launch VS Code with `--extensionDevelopmentPath`
pointing at this directory and `--extensionTestsPath` pointing at that runner. Tests
temporarily modify those fixture files; do not point the runner at your actual work.
The main README records the validated test coverage.

For packaging, build the release server, copy it and `tellurc` to `server/` (use `.exe` names on Windows), then run `npm run package -- --target PLATFORM`. The build
bundles the language client; npm dependencies are not shipped separately. Packaging
does not publish anything to the Marketplace.


## Syntax revision in 0.6.0

The playground now uses `control`, lowercase types, type-first bindings and brace
outcome patterns. Both old and revised syntax have highlighting and compiler diagnostics.
Hover over a control declaration name to inspect the checked contract; Outline shows
inferred `exits auto` contracts too. Snippets use the revised syntax. The setting
`tellurLanguage.entryRegion` retains its name but selects the entry control.

The remaining ownership/collection/async proposals are tracked separately in
`docs/language-revision.md`; the shipped checkpoints are listed below.

## Ownership checkpoint in 0.7.0

`mutable`, shared/exclusive references, dereference, `clone` and `drop` have syntax
highlighting. Compiler diagnostics report moved values, overlapping loans, invalid
mutation and reference lifetime restrictions in unsaved buffers. Declaration hover
keeps `ref string` parameter spelling. `playground/ownership.tellur` is an executable
example. Integration tests exercise live reference diagnostics, repair and native
mutation in addition to the previous syntax and build/run checks.

## Reference boundary checkpoint in 0.8.0

Control hover lists checked return lifetime sources. Shared/exclusive calls, returned
borrows and reborrows are checked in unsaved buffers; local last-use inference removes
unnecessary explicit drops. `playground/references.tellur` demonstrates these features.
The integration runner checks local-escape diagnostics, later-use loan conflicts,
lifetime hover, repair, caller mutation and native execution.


## Collection checkpoint in 0.9.0

Typed arrays, vectors and maps compile and run from the editor. List literal strings,
numbers and names have expression highlighting distinct from single-line path
selectors. Compiler diagnostics reject conflicting collection mutation while an
obtained element reference is still in use. Control hover shows returned element
origins as `values[*]`. `playground/collections.tellur` demonstrates lookup, insertion,
lengths and consuming iteration. The integration runner tests unsaved conflicts,
last-use repair and native execution. Multiline path selectors may have approximate
TextMate coloring; compiler parsing and diagnostics support them.


## Adapter and outcome checkpoints in 0.14.0

Initial `[entry]` is optional. `exit_delta` and `capture` snippets accompany syntax
highlighting for nested adapters, named inline paths and captured results. Control
hover infers adapter/forwarding contracts. The language server checks exhaustiveness,
borrowed result lifetimes, resource transfer and mandatory outcome handling in unsaved
buffers. The extension-host tests repair an unhandled capture and verify native
producer-before-later-effect ordering. Try `playground/named_outcomes.tellur`, `stored_adapters.tellur`, `adapters.tellur` and `outcomes.tellur`.


Named `outcome` declarations appear in the outline and expose their alternatives and
handling obligation on declaration hover. A control declared with `exits message`
shows that name and the checked alternatives. The `outcome_contract` snippet starts
a declaration. Try `playground/named_outcomes.tellur` for two producers sharing a type.

## Forwarding and semantic checkpoint in 0.13.0

`playground/forwarding.tellur` demonstrates payload-free call statements,
`forward operation(...)` and `outcome => forward` adapter arms. Highlighting and
snippets recognize the new forms; diagnostics reject added alternatives in a bare
call and incompatible tail forwarding. Inferred contracts and borrowed origins
remain visible in declaration hover. See `docs/semantic-foundation.md` for the
mathematical draft and finite comparison command.

## Expansion certificates in 0.14.0

The packaged compiler validates expansion correspondence and adds
`certify-expansion` / `verify-expansion` terminal commands. Source syntax and editor
commands are unchanged. Read `docs/expansion-certificates.md` for the artifact
workflow and the precise scope of validation.

## Protocol analysis in 0.15.0

The packaged compiler adds `protocol` residual queries and `protocol-compare`
completed-trace inclusion. These commands analyze a finite structural abstraction
and never execute the program. Source syntax and editor commands remain unchanged.
See `docs/protocol-analysis.md` for prefix files and the facts the model forgets.

## Tail extraction in 0.16.0

The bundled compiler includes `extract-tail`. Run it in the terminal to generate a
separate candidate file, then open that file for normal highlighting, diagnostics
and build/run. `playground/extraction/` contains runnable original/extracted examples.
See `docs/extraction.md` for the command and boundary rules. There is no extraction
code action yet; this release changes compiler tooling without adding source syntax.

## Source data types in 0.17.0

`struct` and `variant` declarations have highlighting, snippets, Outline entries and
hover with their layouts and Copy/completion properties. Diagnostics follow unsaved
layout and nominal-type errors. `playground/data_types.tellur` runs directly from the
editor. Modules, generic data and generic controls are described below.

## File modules in 0.18.0

Imports use `import "file.tellur" as namespace;`. Highlighting covers imports, qualified
types and `pub` declarations/fields. Unsaved dependency errors update importing
files, with related source locations. Tellur-file changes inside workspace folders
trigger rechecking alongside manifest changes. Open `playground/modules/main.tellur`
to try the private-representation example. See `docs/modules.md` for current limits.

## Named fields in 0.19.0

Named struct construction uses `person { name: "Ada", age: 36 }`. Field labels are
highlighted, and diagnostics report missing/duplicate/unknown fields, incompatible
types and ownership errors in the written initializer order. Variant and outcome
payloads remain positional. See `playground/named_fields.tellur` in the compiler checkout.

## Generic data in 0.20.0

Generic data uses `struct box<item> { item value; }` and explicit construction such
as `box<string> { value: "Ada" }`. Generic variants and outcomes are supported too.
Hover shows template parameters; concrete fields determine ownership. Generic controls are described below; trait constraints remain planned. Try `playground/generic_data.tellur`.

## Generic controls in 0.21.0

Generic controls use `control identity<item>(item value) exits auto` and explicit calls
such as `identity<string>(text)`. Hover and Outline show the checked parametric exits.
Unused generic bodies are checked too: unconstrained parameters can be borrowed or
transferred, while copying/cloning/disposal require capabilities that are still planned.
Try `playground/generic_controls.tellur`.

## Generic bounds in 0.22.0

Control parameters accept `item: copy`, `item: clone` and `item: drop` bounds.
The grammar highlights capability names and hover/outline retain the declared bounds.
Unsaved definition errors explain missing permissions; concrete argument errors point
to the call site. `playground/capabilities.tellur` runs through **Tellur: Build and Run**.

## Concrete traits in 0.23.0

`trait`, `implement ... for ...`, associated `type` declarations and `self` are highlighted.
Outline shows traits/implementations with associated types and controls as children.
Declaration hover shows the explicit contract. Unsaved conformance and associated-type
errors use compiler diagnostics. Run `playground/traits.tellur` with **Tellur: Build and Run**.
Trait-constrained generics are a later checkpoint; concrete calls use `reader<string>.read(...)`.

## Trait bounds in 0.24.0

Generic trait requirements and associated sealed bounds are highlighted. Generic hover
shows checked projected result types using source import aliases. Missing requirements
and invalid associated permissions appear while editing; abstract compiler identities
are hidden. `playground/trait_bounds.tellur` runs through **Tellur: Build and Run**.


## Declared borrow origins in 0.25.0

`from input` in an exit payload names the borrowed parameter that keeps a returned
reference alive. Trait declarations, implementations and generic controls expose
this contract in hover. Unsaved diagnostics reject unknown origins, wrong-input
returns, local escape and incompatible implementation origins. Try
`playground/borrowed_traits.tellur`, then use **Tellur: Build and Run**.


## Reference words in 0.26.0

Use `implement`, `ref t`, `ref mutable t`, `borrow owner`, `borrow mutable owner`,
and `deref reference`. Reference field access is transparent: `borrow input.text`
borrows the field with the same privacy and ownership checks as an explicit
reference projection. Hover, snippets and highlighting use these words. Previous
`impl`, `&` and `*` spellings remain accepted. `&` still intersects path selectors.

## Local reference fields in 0.27.0

Try `playground/stored_references.tellur` for shared/exclusive references inside local
structs, whole-record moves, partial moves and last-use borrowing. Unsaved diagnostics
report owner conflicts and moved fields. The integration test repairs both errors
and builds/runs the same example through the installed compiler commands.

## Selected payload inference in 0.28.0

`exits ready..., missing` infers a fixed, nonempty payload only for `ready`.
Try `playground/inferred_payloads.tellur`; hover over `lookup` to see its checked
payload. Empty inferred payloads produce an unsaved `E_INFER` diagnostic. The
integration suite checks highlighting separately and verifies hover, diagnostic
repair and native build/run in VS Code.

## Grouped record results in 0.29.0

`playground/origin_groups.tellur` demonstrates `struct text_pair['a, 'b]`, reference
field arrows, and `text_pair[a, b]` result bindings. Hover describes the public
origin groups; Outline retains their spelling. Swapping the result bindings gives
an `E_LIFETIME` diagnostic. Moving an owner mutation before its field's final use
produces `E_LOAN`; separate fields release their dependencies independently.
The extension integration test exercises hover, unsaved errors/repair and native run.

## Reference inference fix in 0.29.1

Local and payload bindings accept `ref auto` and `ref mutable auto`. The referent
is inferred while the reference mode remains explicit; ownership checks are unchanged.

## Record inputs in 0.30.0

`playground/record_inputs.tellur` distinguishes a carrier's own storage from its
contained references. Contracts use `from input['heading]`; grouped results can
use `view[input['heading]]`. Hover preserves these public selectors. Unsaved
unknown-group and wrong-origin errors are checked, and the integration test runs
the example natively after repairing them. Borrowed carriers currently require
shared reference fields; carriers with exclusive fields can be passed by value.

## Exclusive carrier reborrows in 0.31.0

`playground/carrier_reborrows.tellur` reads exclusive fields through a shared carrier,
then borrows both fields exclusively and restores parent access at their individual
last uses. Hover notes when a returned reference retains carrier authorization.
The integration suite checks that explanation, unsaved parent conflicts, repair and
native execution. No new source syntax is required.

## Opaque owner splits in 0.32.0

`playground/opaque_splits.tellur` returns two exclusive references from one owner,
uses them together and restores owner access after their last use. Hover explains
that exclusive results have checked disjoint access even when their lifetime origins
coincide. Unsaved edits diagnose duplicate exclusive results and premature owner
access. The integration suite checks repair and native execution as well.

## Local borrowed outcomes in 0.33.0

`playground/borrowed_outcomes.tellur` captures a borrowed lookup, adapts it to a named
outcome, handles it and then mutates the owner after the last use of the reference.
Named outcome hover displays reference payloads. Unsaved edits diagnose premature
owner access and unhandled borrowed outcomes. The integration suite exercises repair
and native execution. Local variants, optional references and reference-bearing
record payloads follow the same checked storage rules.


## Nested borrowed storage in 0.34.0

Open `playground/nested_borrows.tellur`: a completed outcome contains a record whose
field is another outcome carrying two mutable references. Hover `auto` on the
payload bindings to inspect their types. The title can be used again after its
reference's last use while the body stays protected. Moving the owner mutation
before its final reference use reports `E_LOAN`. Build and Run prints the updated
owned strings. Public borrowed-sum call boundaries remain a later checkpoint.


## Public grouped variants in 0.35.0

Try `playground/public_variants.tellur`. Grouped variants and nested records now cross
owned control boundaries while retaining alternative-specific borrow dependencies.
Variant declaration hover displays group and field mappings; control hover explains
guarded dependencies; `auto` hover displays received aggregate types. Moving an owner
mutation before its selected reference's last use produces an unsaved `E_LOAN`
diagnostic. Build and Run demonstrates independent owner restoration. Whole-sum
borrows and named-outcome/option schemas remain queued.

## Named outcome and option origins (0.37)

Open `playground/outcome_origins.tellur`. Hover over `text_result` to see its ordered
origin groups and payload mappings, or over `lookup_text` to see the completion
binding `exits text_result[title, body]`. The optional result inherits those groups.
Assigning `title` before its final reference use reports `E_LOAN`; after its last
use it is independently available while `body` remains borrowed. Both populated
and absent results work in native build/run. Whole-sum borrows remain gated.

### Borrowed sum carriers (0.38)

`delta borrow value`, `delta borrow mutable value`, and decisions on reference
parameters inspect the active alternative without consuming its owner. Hover on
`auto` shows shared/exclusive payload references, including directly reborrowed
reference payloads. Diagnostics catch early owner/carrier mutation and moving a
payload through a borrow. Try `playground/sum_carriers.tellur` in the workspace.


### Returned carrier references (0.39)

`playground/carrier_results.tellur` returns references to a reference-bearing record
and option. Control hover lists storage and contained-source lifetimes; `auto` hover
shows the inferred outer reference mode. Unsaved edits report premature carrier
release or conflicting access to its external sources. Build and Run exercises the
same rules in the native backend.


### Locally stored carrier references (0.40)

`playground/stored_carriers.tellur` captures a carrier reference, moves the completed
outcome, then edits through it after unpacking. It also stores a reference in a local
job record inside an option and borrows that option before consuming it. Hover on
`auto` shows container and nested reference types; premature carrier release is an
unsaved `E_LOAN` diagnostic. Capture still executes synchronously.


## Public nested-reference contracts in 0.41.0

Hover on a data declaration shows both the referent's bracket mapping and the
reference field's storage arrow. Control hover reports the independently mapped
result groups; `auto` hover reports captured outcomes and returned containers.
The integration suite checks unsaved premature-drop errors, repair and native
execution using `playground/public_carriers.tellur`.


## Aggregate trait origin contracts in 0.42.0

Open `playground/aggregate_traits.tellur` to follow grouped lifetimes through an
associated type, a named completion contract, generic dispatch and capture. Hover
`auto job` to see `edit_job<string>`; editing the implementation's origin mappings
reports conformance diagnostics immediately. The compiler and language server in
this package both support these contracts.


## Local shared-reference collections in 0.43.0

Open `playground/borrowed_collections.tellur`. Arrays, vectors and maps can store
direct shared references locally, and lookups expose the referent directly. Hover
`auto word` to see `ref string`; editing an owner while the reference remains live
reports a loan conflict. Removal and replacement preserve possible source dependencies.
Exclusive elements, nested borrowed collections and public collection-origin contracts
remain later stages.

## Explicit public contracts in 0.44.0

Public controls require explicit exit labels, payload types and borrowed-result
origins; `exits auto` and `label...` remain available on private helpers. A missing
public annotation produces `E_PUBLIC_CONTRACT`, including in unused generic controls.
For incompatible direct forwarding and missing direct-call arms, the Problems view
includes related producing and forwarding locations. Notes follow unsaved buffers
and imports; repairing the source clears the diagnostic. Stored values and adapters
are covered below. Try `playground/public_contracts.tellur`.

## Rust adapter projects (since 0.45.0)

Manifests may declare a Rust source adapter and locked dependencies. Adapter paths
resolve beside the manifest, even when the workspace root is elsewhere. Diagnostics
check Tellur contracts without reading or executing Rust. Build and Run compiles the
wrappers through Cargo with the committed lockfile. Resolve dependency changes
explicitly using `tellurc lock-native` in a terminal. Try the maintained
`applications/settings/main.tellur`; its README describes entry and argument settings.

## Stored outcome diagnostics (since 0.46.0)

Missing delta arms and incompatible adapter results can link to their producing
exit, capture and intervening adapters. Moving a completed value or placing it in
a local record/sum retains known lineage; merged labels retain their contributing
alternatives. Notes update with unsaved changes and clear after repair. These are
bounded structural locations, not feasible-input proofs. See the
[coverage limits](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/diagnostics-and-limits.md).

## Structured checking (0.47.0)

The bundled compiler supports `tellurc check SOURCE [MANIFEST] --json`. It prints
one versioned JSON result to stdout and exits with 0 for success or 1 for failure.
Source points use one-based Unicode scalar columns; LSP ranges keep their standard
zero-based UTF-16 encoding. Path-related LSP diagnostics additionally carry a
`structural_may_reach` witness in diagnostic data. See the
[report format](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/diagnostics-and-limits.md#machine-readable-checking).

## Settings HTTP service in 0.50

The maintained settings application now includes a bounded loopback HTTP server.
Open `applications/settings/main.tellur`, select entry `serve`, and supply the data
path, port, connection budget and idle timeout in milliseconds. Use a known port
with Build and Run, which captures output until completion; launching the built
program in the integrated terminal shows its listening URL immediately.

The [service guide](https://github.com/apology-is-policy/tellur/blob/main/applications/settings/README.md#run-the-http-service)
includes curl requests and the exact HTTP/cleanup limits. Unsaved diagnostics still
enforce explicit resource completion across the server's imported controls.

## Retained Rust resources in 0.49

The compiler now supports manifest-bound opaque Rust owners across calls, with
shared borrowing and explicit consuming operations. Open
[`examples/retained_file/main.tellur`](https://github.com/apology-is-policy/tellur/blob/main/examples/retained_file/main.tellur)
to try it. Removing `file_release(file);` produces an unsaved resource diagnostic;
repairing it permits a native build with the bundled compiler. No new Tellur
syntax is required. See the
[adapter reference](https://github.com/apology-is-policy/tellur/blob/main/docs/rust-adapters.md#retained-resources-049).

## Process status contracts in 0.48

Named outcome alternatives support `@exit N` highlighting, annotation diagnostics,
and declaration hover listing status defaults. Build and Run propagates the final
entry's configured status. Migrated `.tellur` playgrounds declare their entry policy;
older programs need explicit annotations or manifest mappings before native build.
See [process entry contracts](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/process-entry.md).


## Expression diagnostics in 0.51

Type, move, field and borrow errors found during expression checking now underline
the affected expression. Invalid arguments select their argument; missing arguments
select the call. Multiline strings, emoji and unsaved imported files retain their
correct editor ranges. Parser errors select the unexpected token, with an empty
selection at end of input. Path coverage and contract errors retain their existing
locations. Release 0.52 extends checking to independent bodies, as described below.


## Independent diagnostics in 0.52

Problems can now show the first error from each independent control body, up to
32 errors. Failed controls and their callers do not contribute partial inferred
contracts or misleading follow-on errors. Repairing a helper makes its callers
eligible for checking again. Unsaved imported files participate in the same check.
`auto` hover is unavailable while the current program is invalid and returns after
repair. Parsing and shared declaration failures still stop at their first error;
recovery within a damaged body remains future work.

## Direct arm exits (0.54)

An ordinary delta arm can finish its control with `missing => exit absent` or
`ready { auto value } => exit done { value }`. Terminating payloads have private
arm scopes; continuing arms keep their path labels and bindings. Highlighting,
inferred `auto` hover, unsaved diagnostics and native build/run support the syntax.
Try `playground/arm_exits.tellur`. Arm separators are commas, without a statement
semicolon inside the arm; multi-statement arm bodies remain unsupported.

## Selected-history hover (0.55)

Hover over `[path]:`, a `path` alias, a delta arm, or an executable statement to see
its resolved structural selection. Enclosing group intersections and aliases are
included. `auto` and declaration hovers retain their type/contract information.
Decision-arm locations distinguish the contributing facts; duplicate names gain
explanatory `@line:column` suffixes. These suffixes are not Tellur syntax.

The display describes possible structural histories, not proven input values.
Loop context means one iteration, with entry conditions already inherited. Large
conditions/origin lists are bounded and explicitly marked when incomplete. Comments
and whitespace have no selection hover. Invalid source, including broken unsaved
imports, suppresses stale information. Try `playground/history_hover.tellur`.

## Outcome absorption (0.58)

**Tellur: Show Outcome Absorption Snapshot** opens checked public contracts together
with bounded internal contributor evidence. It uses current unsaved source and
imports. The JSON includes source locations, distinct call sites, explicit analysis
limitations and truncation flags. Failed checking opens no partial or stale result.
Save As can retain the snapshot; rerun the command to refresh it.

The terminal `absorption-diff` command compares these artifacts, reporting public
contract changes separately from internal contributor changes. Try
`playground/outcome_absorption.tellur`; its `unavailable` public outcome absorbs two
different private outcomes. See [the reference](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/outcome-absorption.md)
for the exact scope and baseline workflow.


## Path and outcome explanations (0.59)

**Tellur: Explain Path Selection at Cursor** opens the checked selection at the
cursor. **Tellur: Explain Outcome Contributors** offers a picker of the active
file's ordinary concrete controls and their outcomes, including private controls.
Both open editable Markdown reports with source links; Markdown preview renders
the links. Rerun after editing: these are fixed reports, using current unsaved
sources and imports. Invalid checking produces an error instead of stale evidence.

Try `playground/path_explanations.tellur`. Generic/trait source selections work;
generic/trait outcome roots remain unavailable. Conditions, decision origins and
contributors have explicit bounds. These are structural facts, not concrete-input
proofs. See [the query reference](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/explanations.md)
for terminal commands, JSON, coordinate conventions and remaining analysis gaps.


## Execution failure reports (0.60)

**Tellur: Build and Run** now shows observed control activations and source sites
for returned execution failures. Use `playground/execution_faults.tellur` with
`"tellurLanguage.programArguments": ["--json", "--fuel", "40"]` to observe the
second worker invocation exhausting its budget. The report goes to stderr and the
program returns 1; it does not invent a declared outcome. Clear these arguments
when returning to other programs.

There is no local catch-and-resume or async execution. Source faults and synchronous
request boundaries are described below. Normal host teardown is distinct from a resource's declared completion
protocol. See [execution failures](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/execution-failures.md)
and the [activation plan](https://github.com/apology-is-policy/tellur/blob/main/docs/faults-and-activations.md).

Source faults (0.62): `fault`/`faults` are highlighted; declaration hover and Outline
show inferred permissions. Public/trait fault contracts and retained-resource
dispositions are checked in unsaved buffers. A fault-only control omits `exits`.


## Contingency and boundaries (0.63)

`@contingent`, `boundary`, `retains`, `contingency`, `supervise`, `terminate` and
`escalate` are highlighted. Boundary declarations hover with supervision and
retained-owner contracts; eligible controls expose their contingent permission.
Unsaved diagnostics check transitive call permission, retained-resource availability
and illegal ordinary calls to boundaries. Snippets `boundary` and
`control_contingent` provide starting points.

Try `playground/contingency.tellur` with entry `main` and no program arguments. It
handles one source fault, then runs another request. The updated settings server
uses the same mechanism for per-request isolation. Faults never become ordinary
outcomes. See [the full reference](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/contingency.md).
