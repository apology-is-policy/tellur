# Tellur Language for VS Code

Experimental editor support for the Tellur compiler.

## Start reading code

1. Install the supplied `tellur-language-darwin-arm64-0.144.0.vsix` using **Extensions:
   Install from VSIX…** in VS Code's Command Palette. This package includes `tellur-lsp`
   for Apple Silicon macOS. No Rust or npm installation is needed to use that package.
2. Open `tellur.code-workspace` from the compiler checkout.
3. Open `playground/hello.tellur` and click **▶** or **Tellur: Build and Run** (`⌘⌥R`).
   Try `playground/sequential_initializers.tellur`, `playground/projected_construction.tellur`, `playground/composed_construction.tellur`, `playground/borrowed_construction.tellur`, `playground/construction.tellur`, `playground/named_outcomes.tellur`, `stored_adapters.tellur`, `adapters.tellur`, `outcomes.tellur`, `collections.tellur`, `references.tellur`, and `count.tellur` next.
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

The server reports independent control, statement and disjoint-path errors, debounced
during typing, with up to 32 per check. Dependent statements and failed callers are suppressed
to avoid cascades. Expression diagnostics select their source ranges; other
diagnostics may point to a token, statement or declaration. Structural
witnesses describe what *may* reach a point, not proven feasible input values.
Declaration and ordinary call-name hovers show checked control contracts. **Format Document** formats the
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
then `tellur-lsp` on PATH. The packaged server and compiler match the extension
version; rebuild or override them when experimenting with compiler changes.

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
0.73 adds the bounded statement recovery described below.

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

## Statement diagnostics in 0.73

Open `playground/diagnostics.tellur` and change `count = 3` to `count = true`,
and `subtitle = "Ready"` to `subtitle = 7`. Problems shows both initializer errors.
The dependent use of `count` does not produce a follow-on error. Repair either
initializer to remove its diagnostic; repair both to restore inferred `auto` hover.

Recovery uses fresh compiler state and stops at decisions, nested scopes, loops,
boundaries and exits. Whole-state fault-resource checks can also stop recovery.
The 32-error budget is shared across statements and controls; no partial checked
program or inferred hover escapes a failed check. See the
[precise recovery limits](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/diagnostics-and-limits.md#statement-recovery).

## Path and scope diagnostics in 0.74

Open `playground/path_diagnostics.tellur`. Change `count = 3` to `count = true`
and `summary = "Brief"` to `summary = 7`: both path-local initializer errors appear,
including the one inside the nested group. Repair either independently; inferred
`auto` hover returns only after the complete program checks again.

Recovery also works inside lexical scopes, statement arms and loop bodies. It keeps
full path selectors: a join touching a stalled history is suppressed, not narrowed.
Fault boundaries/contingencies, inline adapters, decision headers and scope/loop
setup or completion retain conservative limits. See the
[path recovery reference](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/diagnostics-and-limits.md#path-and-scope-recovery).


## Shared source libraries in 0.75

The bundled compiler supports explicit Rust adapter modules and the new JSON file
source library. The maintained settings service and `examples/json_file/main.tellur`
share storage code with distinct application policy. Module metadata is checked
without loading Rust code; Build and Run reads the configured sources and checks
the generated Rust interface. Paths resolve from the manifest, including when the
native output is placed elsewhere. See the [library guide](https://github.com/apology-is-policy/tellur/blob/main/library/README.md).

## Call-site contract hover in 0.76

Hover over `json_file.load` in `examples/json_file/main.tellur` to see checked
parameter types, its named completion contract and alternatives, followed by the
selected histories. Local/imported controls, concrete explicit generic calls and
manifest primitives are supported. Borrow origins, inferred private return lifetimes,
fault permissions, primitive effects and immediate/blocking host execution stay
visible; public controls do not expose private field paths.

Unsaved dependency edits refresh signatures. Invalid arguments or dependencies
suppress stale hover. The callee range excludes value arguments; other positions
retain their existing feedback. Constructor calls gain contracts in 0.79 below,
concrete trait calls in 0.80, and named boundaries in 0.82. Abstract generic-body
calls retain selected-history feedback for now. See the
[call-site reference](https://github.com/apology-is-policy/tellur/blob/main/docs/language-reference.md#call-site-contract-hover).

## Declaration diagnostics in 0.77

Independent public-contract and concrete-signature errors now appear together,
including imported and generic declarations. Each preparation phase reports at most
one error per control within the 32-error budget. Later phases and bodies wait until
the failed declarations are repaired; inferred and call-site hover remain unavailable
while the program is invalid. Shared type/trait preparation retains conservative
stopping points. Try `playground/declaration_diagnostics.tellur`, or see the
[phase rules](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/diagnostics-and-limits.md#control-declaration-batches).

## Named completion bindings in 0.78

Wrong origin-argument counts in separate control declarations now appear together,
after their shared outcome schemas validate. Try changing both `exits view[input]`
bindings in `playground/contract_binding_diagnostics.tellur` to `exits view[input, input]`.
Repairing one removes its diagnostic; repairing both restores checked call contracts
and `auto` hover. The existing 32-error budget and dependent-phase barriers apply.
Shared schema failures remain conservative, and valid programs keep their existing
borrowing and execution semantics.

## Constructor call hover in 0.79

Hover over a constructor call to see the constructed type and checked constructor
signature, including concrete generic inputs, ordered result-origin groups and
fault/contingency permission. It uses current unsaved definitions and the import
alias at the call, and does not list private fields. Invalid source suppresses stale
contracts. Try `text_pair(...)` and `editable_text(...)` in
`playground/borrowed_construction.tellur`; selected histories remain underneath.
Concrete trait calls gain contracts in 0.80 below, and named boundaries in 0.82.
Abstract generic-body calls retain their existing selected-history feedback.

## Trait call hover in 0.80

Concrete calls such as `reader<text_source>.read(...)` now show the checked trait
contract with associated types resolved. Parameters, payloads, origins and permissions
come from the trait declaration; private implementation fields and independently
chosen binding names stay out of the hover. Unsaved edits refresh the contract, and
invalid source suppresses stale results. Concrete associated-type selections also
resolve in call targets and explicit type arguments. Generic-body calls remain
abstract even if a real type has the same name as a type parameter. Try
`playground/traits.tellur`; named boundaries gain contracts in 0.82 below. Abstract
calls retain selected-history feedback.

## Lexical call types and named results in 0.81

Hover inside an implementation now resolves its `self` and associated-type aliases,
including sibling aliases, before looking up module types. Same-named nominal types
do not override that lexical meaning. Generic helper calls also retain named result
contracts after associated-type substitution, including explicit origin mappings on
their alternatives. Unsaved edits refresh both views; invalid source suppresses stale
contracts. Try `playground/trait_aliases.tellur` and `prepare<text_editor>(...)` in
`playground/aggregate_traits.tellur`. Named boundaries gain contracts in 0.82 below;
abstract generic calls retain their existing selected-history feedback.

## Named supervision contracts in 0.82

Hover at the boundary name in `supervise request(...)` to see its checked owned
inputs, ordered retained parameters and contingency escape permission. Internally
handled faults do not add `faults` to this interface. The hover explains that
supervision produces no outcome or fault value, and that escaping contingency
abandons the supervisor continuation. Imported aliases and unsaved declarations
refresh normally; invalid source suppresses stale contracts. Try both supervision
sites in `playground/contingency.tellur`.

## Named boundary diagnostic recovery in 0.83

Independent local errors in named boundary work and contingency bodies now appear
together. Disjoint paths retain their own checks; dependent values and uncertain
retained-owner/fault-resource checks stay suppressed. A failed handler suppresses
its dependent work and supervisors until repaired. The 32-error budget is shared.
Try two invalid local initializers in `playground/contingency.tellur`; repairs clear
errors independently, and hover returns only after the complete source checks.
Lexical work gains recovery in 0.84 below; lexical handlers gain recovery in 0.85 below; inline adapters remain conservative
recovery barriers.

## Lexical boundary work diagnostics in 0.84

Independent local errors inside `fault boundary` work now appear together, including
nested work, paths and ordinary scopes. An uncertain work body stalls its entire
boundary history before handler and outer continuation checks; independent outer
paths keep their checks. Capture headers retain first-error limits; lexical handlers
gain recovery in 0.85 below. Try two invalid initializers in `playground/recovery.tellur`; inferred hover
returns only after every error is repaired. Native fault/recovery behavior is unchanged.

## Lexical contingency diagnostics in 0.85

After lexical work checks cleanly, independent errors inside its contingency now
appear together. Fault-mode permissions, capture access, immutable `@fault` and
resource obligations remain enforced. An uncertain handler stalls the whole boundary
history while preserving disjoint outer checks. Try two invalid initializers in the
handler of `playground/recovery.tellur`; partial repairs clear errors independently,
and inferred hover refreshes only after every error is repaired.

## Explicit exit-adapter diagnostics in 0.86

Independent terminal arms of `exit delta` now report errors together when the
control has a fully explicit result shape. Nested arms preserve their own guards,
ownership and borrow checks. Stored adapters, inferred results and decision headers
retain conservative barriers. Try two bad payload expressions in the explicit
`main` adapter of `playground/adapters.tellur`; repairs clear errors independently
and call hover returns only after the complete source checks.

## Explicit stored-adapter diagnostics in 0.87

A stored adapter with a declared outcome type can report independent arm errors
in one batch. Its uncertain result and continuation stay suppressed until repaired;
clean arms retain their resource and lifetime checks. Try invalid payloads in both
arms of the `message_result message` binding in `playground/stored_adapters.tellur`.
Inferred stored types, headers and fault-context adapters remain conservative.

## Symbolic ordinary generic call contracts in 0.88

Hover `identity<item>(value)` inside `playground/generic_controls.tellur`: the
contract retains `item`, independently of concrete callers. It includes declared
bounds, named result origins, inferred payloads and private checked return lifetimes.
Anonymous payloads use bounded type rendering without internal compiler names.
Unsaved edits refresh the signature; invalid sources suppress it. Subsequent
releases add symbolic trait and constructor calls below.

## Symbolic trait call contracts in 0.89

Hover `reader<r>.peek(input)` inside `playground/borrowed_traits.tellur`. The trait's
names, associated projections, origins and fault permissions remain abstract and
independent of concrete implementations. Named results retain their alternatives
and nested origin mappings. Edits refresh the interface; invalid bounds or
contracts suppress it. Symbolic constructors follow in 0.90.

## Symbolic constructor contracts in 0.90

Hover `box<item>(move input)` or `view<item>(input)` inside
`playground/generic_construction.tellur`. The checked contract preserves abstract
types, shared/exclusive input modes, ordered result origins and permissions. Calls
inside constructors and nested or associated-type arguments work the same way.
Private fields and compiler-generated completion names are hidden. Unsaved
parameter edits refresh the contract; invalid initialization suppresses it until
repaired. Named boundaries do not declare generic parameters; their existing
concrete supervision hover remains available in generic bodies.

## Constructor access diagnostics in 0.91

Open `playground/constructor_access/main.tellur` and `messages.tellur`. Removing
`pub` from both constructors reports both inaccessible callers together. Restore
one modifier to leave the other diagnostic; restore both to recover checked hover.
Unsaved dependency edits participate immediately. One error is reported per prepared
body, with the shared 32-error budget and original imported source locations. Later
phases wait until constructor-call preparation succeeds; no partial contract is shown.

## Generic-bound diagnostics in 0.92

Independent controls can report invalid requirement declarations together: duplicate
resolved traits, unknown capability/trait names and private traits required by public
generic interfaces. One error is reported per control under the shared 32-error
budget, including unused definitions. Imported locations and unsaved repairs stay
current. Checked hover returns only after the full source checks; trait resolution
and later type/ownership checks remain blocked while the bound phase is invalid.

## Inferred stored-adapter diagnostics in 0.93

The first `auto saved` adapter in `playground/stored_adapters.tellur` now supports
independent errors in distinct result labels. For example, invalid expressions in
both `ready` and `empty` payloads report together. Later occurrences of an uncertain
label stay suppressed because they may depend on its missing inferred payload shape.
The complete stored result and its continuation remain unavailable until repaired;
hover never shows a partial inferred type. Nested arms and imported/generic locations
follow the same shared 32-error budget. Inferred exits, headers and fault contexts
retain conservative barriers.

## Inferred exit-adapter diagnostics in 0.94

The inferred `successor` helper in `playground/adapters.tellur` now reports independent
payload errors across its terminal arms. This also applies to selected `ready...`
contracts and repeated exit labels. Payload expressions infer independently; no
partial exit contract or inferred hover is published while errors remain. Imported
and generic source locations, nested paths, ownership checks and the 32-error budget
are preserved. Headers, captures and fault-context terminals remain conservative.

## Trait-declaration diagnostics in 0.95

Independent trait declarations now report invalid contracts together, including unused
traits. Method profiles, public type visibility, payload shapes and borrowed-result
origins are checked one error per trait, under the shared 32-error budget. Shared
identity/schema failures still stop earlier; implementations and all later checks
wait until declarations are valid. Unsaved repairs update imported locations and
restore call/inferred hover only after a complete successful check. The editor
regression uses `playground/traits.tellur` with a second independent interface.

## Constructor-definition diagnostics in 0.96

Independent invalid constructors within one parsed file now report together, including
unused generic definitions. Invalid result-origin arity, partial-self accesses,
explicit exits and constructor-local isolation are checked before generated controls
are registered. The shared 32-error budget and original imported locations apply;
loading still stops at the first failing file. Later initialization and ownership
checks keep their existing phase boundaries.

The isolated editor regression uses `playground/constructor_access`: two unsaved
invalid field initializations in `messages.tellur` produce separate diagnostics in
the caller, with related locations in the dependency. Repairing each definition
updates diagnostics; constructor contracts return only when the checked program is
valid. The repaired program builds and runs through the normal editor command.

## Written type ranges in 0.97

Type errors in supported declaration, signature and typed-pattern checks now select
the complete written annotation, including multiline generic types and reference
prefixes. Imported related locations use the current unsaved dependency text;
generic substitution does not replace source ranges with generated-name widths.
Whole-contract/origin checks, some later validation and contributor notes still have
point locations. Error codes and checking order are unchanged.

The isolated regression changes a constructor parameter in
`playground/constructor_access/messages.tellur` to an invalid multiline type, then a
compact equivalent. It verifies both exact related ranges, unavailable stale hover,
repair and native execution from the caller.

## Runtime foundation in 0.98

The bundled compiler shares an execution-local activation ID allocator between
related runtime contexts. Existing sequential native reports preserve their IDs and
source information. This is groundwork for async execution; no task syntax, scheduler
or new highlighting is enabled. Editor behavior is unchanged.

## Reference runtime foundation in 0.99

The bundled compiler uses persistent reference continuations. Its host Rust API can
pause and resume a scripted simulation without changing instruction fuel or fault
handling. Native Build and Run, source syntax, highlighting and diagnostics are
unchanged. There is no source task or readiness scheduler yet.

## Loop type diagnostics in 0.100

Invalid loop parameter types now select their full written annotation instead of
an absent or loop-wide location. Multiline types, imported unsaved files, generic
substitution and legacy fields retain their original endpoints. Initializer errors
stay on their expressions. The isolated editor test derives an imported counter from
`playground/count.tellur`, checks exact multiline/compact ranges and fresh hover after
repair, then builds and runs it to emit 1 through 5. No source async syntax is enabled.

## Local type diagnostics in 0.101

Invalid local type annotations now retain their authored locations through imports,
generic substitution and declaration checking. This also covers preparation of
`capture` and stored adapter types. Initializer errors retain their expression
locations; capture compatibility keeps its existing statement point. An isolated
editor test edits multiline/compact local types in an imported counter, verifies
unsaved ranges and hover repair, then builds and runs the restored program.

## Public interface type diagnostics in 0.102

A public declaration exposing a private or producer-indexed type now selects its
written field, parameter or explicit result type. Imported and unsaved locations,
multiline types and generic spellings stay intact. Whole named-contract references
without a field annotation keep their existing declaration points. Visibility rules
and phase order are unchanged; the isolated editor check repairs an exported type
and then builds and runs the imported counter.

## Reference invocation admission in 0.103

The bundled reference executor validates a whole argument list before installing a
frame. Failed argument or activation admission leaves no partial parameter storage.
Source syntax, diagnostics, highlighting and native Build and Run behavior are
unchanged; source tasks and scheduling remain planned.


## Named completion type diagnostics in 0.104

Invalid or private completion types after `exits` now select their full written
type, including qualified names, generic arguments and multiline spelling. Separate
origin mappings retain their own contract meaning and are excluded from the type
range. Imported unsaved changes refresh the exact location and invalidate stale
hover until repaired. The isolated editor check edits a generic public helper's
named contract, repairs it, then builds and runs the importing counter.

## Direct async calls in 0.105

`async`, `run` and `await` have syntax highlighting and explanatory keyword hover.
Callee hover preserves `async control`; diagnostics reject missing or mismatched
invocation permission, including unsaved imported contracts. The compiler supports
sequential calls; local owned cold tasks are added in 0.106. Try
[`playground/async_entry.tellur`](https://github.com/apology-is-policy/tellur/blob/main/playground/async_entry.tellur) and the
[async reference](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/async-execution.md).


## Local cold tasks in 0.106

`defer` has keyword highlighting and hover. Hovering `auto` on a pending task
shows `task<producer>`; this describes the inferred internal type rather than a
public writable type. Diagnostics reject forgotten or reused tasks, borrowed
captures and escaping task results. Unsaved repair refreshes checked hover.
Try [cold_tasks.tellur](https://github.com/apology-is-policy/tellur/blob/main/playground/cold_tasks.tellur)
and the [reference](https://github.com/apology-is-policy/tellur/blob/main/docs/reference/async-execution.md).


## Direct borrowed cold captures in 0.107

Task hover and diagnostics now cover retained shared/exclusive borrows. Conflicting
access before await is rejected, including after a task move; unsaved repair restores
checked type hover. The isolated editor fixture builds and runs
[borrowed_tasks.tellur](https://github.com/apology-is-policy/tellur/blob/main/playground/borrowed_tasks.tellur).
Contained captures and borrowed result delivery remain separate follow-ups.

## Borrowed task completions in 0.108

Awaited reference payloads now show their inferred type on `auto`. Unsaved edits
that mutate a still-borrowed source report `E_LOAN`, and repair restores checked
hover. The isolated editor fixture builds and runs
[task_results.tellur](https://github.com/apology-is-policy/tellur/blob/main/playground/task_results.tellur),
showing one capture released at await and the returned borrow released after its
last use. Existing windows are not reloaded during release installation.

## Owned reference-bearing captures in 0.109

The checker and inferred-type hover support task captures containing shared or
exclusive references. Dropping another shared copy does not release the task's
borrow; diagnostics and repair track this in unsaved buffers. The isolated fixture
builds and runs [contained_tasks.tellur](https://github.com/apology-is-policy/tellur/blob/main/playground/contained_tasks.tellur).


## Whole borrowed carrier captures in 0.110

Cold tasks can borrow an entire reference-bearing record or sum. The task retains
both its storage and the references inside it. Awaited results preserve separate
storage and contained-origin contracts, including nested carriers and tagged sums.
A shared outer borrow cannot grant exclusive access to an inner reference.
Try `./bin/tellurc exec playground/carrier_tasks.tellur`; hover `auto` to inspect
the pending task and its detached borrowed completion. Public/escaping tasks,
nested task storage and scheduling remain separate work.


## Execution groundwork in 0.111

The reference interpreter now separates execution-wide storage, fuel and history
from resumable control state. Internal interleaving checks preserve borrowed storage,
frame retirement and isolated contingency state. Source execution remains sequential;
there is no new syntax or `spawn` support in this release.


## Task identity checks in 0.112

The reference executor now tracks each local cold task's original invocation and
one-shot await lifecycle. Normal completion cannot forget pending work; faults and
supervised recovery retire abandoned task records before contingency. Existing
source syntax, borrowing and sequential native behavior are unchanged. Readiness
and source `spawn` remain the next async implementation steps.


## Cold-await dispatch in 0.113

Cold-task awaits now use queued dispatch and parked caller state in the reference
executor. One-use completion waits reject early or duplicate delivery; borrowed
storage, fault traces and instruction quotas keep their existing behavior. Source
execution is still sequential. External readiness and scoped `spawn` remain ahead.


## Named completion origin diagnostics in 0.114

Named completion origin errors now underline the complete bracket mapping, including
multiline and contained selectors. Imported unsaved edits preserve their original
locations; repair restores checked hover information. Type and body diagnostics keep
their own ranges. This changes diagnostic presentation without changing lifetime rules.


## Borrowed-result origin diagnostics in 0.115

Borrowed-result `from` errors now underline the written clause, including nested
selectors and multiline comments. Imported generic and trait signatures keep their
original locations. Missing clauses and body errors retain their existing locations;
unsaved repairs restore checked hover information.


## Payload origin diagnostics in 0.116

Result payload mappings such as `view[input] value` now underline their written
brackets when an input, group count or borrowing permission is invalid. Imported,
generic and trait signatures retain exact locations through unsaved edits. Named
completion mappings, `from` clauses and type/body errors keep separate ranges;
repair restores checked hover information. Borrowing rules are unchanged.


## Suspended caller storage in 0.117

Cold awaits now retain callers in a separate suspension table. One-use registrations
validate the matching task and completion wait before restoring the original caller;
fault unwind retires the registration without producing a normal result. Borrowed
storage, diagnostic traces and host instruction quotas retain their existing behavior.
Source execution remains sequential; scoped concurrency is the next feature group.


## Ready context storage in 0.118

Prepared cold-task children now remain in separate ready storage until dispatch.
Selection validates the retained context and cannot replace an active control chain.
Borrowed storage, host instruction quotas and source effect order are unchanged.
This prepares the runtime for independent readiness; source execution remains sequential.

## Waiting contexts and readiness in 0.119

The reference runtime now has separate waiting storage and ready entries for either
starting or resuming a task. Readiness tokens carry an execution identity and a fresh
generation; signals only make the retained context eligible, and dispatch restores
that same context without replaying a call. Completion remains a separate one-use
operation. Host instruction quotas never select a sibling.

This is internal groundwork validated with checked borrowed execution. Source
`spawn`/`yield`, async host I/O and fault-domain settlement remain ahead; current
source execution and editor behavior stay sequential.

## Fault domain admission in 0.120

Reference execution now tracks live fault domains separately from invocation and
trace identities. Entry, named supervision and lexical boundaries receive distinct
domains; handling closes only the affected domain and restores its parent. Deferred
tasks acquire their executing domain when awaited, preserving existing cold-task
movement across lexical boundaries.

Stopped domains reject normal task admission, readiness and ready dispatch. Boundary
finalization and contingency admission check for retained execution contexts first.
These gates prepare for automatic sibling settlement; they do not enable source
concurrency, async host adapters or cancellation.

## Reference sibling settlement in 0.121

The reference driver now preserves a failed boundary while retiring its retained
children. Queued bodies do not start, waiting bodies do not resume normally, and
borrowers unwind before their owners. Unrelated domains remain available, and late
readiness signals cannot revive settled work. The original fault report survives.

When a stopped sibling is inside a nested boundary, its resources follow emergency
retirement without entering that sibling's contingency. Only the owning boundary
handles the original fault after settlement. This implements the accepted policy
in the private reference machinery; source `spawn`/`yield`, async host adapters and
fallible or suspending cleanup remain ahead.

## Native resource scopes in 0.122

Native fault cleanup now follows explicit resource ownership instead of allocation
snapshots. It preserves unrelated resource scopes, retires temporary owned inputs
before contingency, and keeps normal outgoing results and cold-task captures alive.
Retained response resources remain available to their handler. This backend groundwork
does not yet enable source concurrency or asynchronous host adapters.

## Native activation storage in 0.123

Native calls now use separately identified frame ranges, preserving borrowed slot
addresses while permitting independent retirement and storage reuse. Fault cleanup
releases only the affected frame's resources. This prepares persistent native
execution contexts; source task scheduling remains unimplemented.

## Native host contexts in 0.125

The bundled compiler adds shared native host-context admission, selection/restoration
and retirement. Fault causes, contingency messages and resource selection stay paired,
with checks for foreign, stale and nested-active state. Public calls remain synchronous;
source scheduling and native sibling settlement are still ahead.

## Task queue and completion joins in 0.126

The reference runtime now separates queue admission from one-use completion joins.
A completed task remains an obligation until its result is delivered and consumed;
cold await still admits both operations atomically. This internal foundation
preserves existing source behavior. Independent completion-payload storage and
source `spawn`/`yield` remain ahead.

## Retained runtime completions in 0.127

Completed task results now live in shared reference-runtime storage until delivery
or emergency retirement. Fault settlement releases their borrows and unwinds parked
callers before the owning contingency. Cold await still delivers immediately;
source scheduling and native completion ownership remain ahead.

## Administrative process settlement in 0.128

Reference process termination now closes task admission and retires all registered
ready/waiting chains, completed results and parked callers before delivering its
original status. Cleanup does not resume ordinary code or nested contingencies,
consume source fuel, or turn termination into an outcome/fault. Written termination
remains restricted to the executable entry's outermost lexical contingency.
Source `spawn`/`yield` and shared native scheduling remain unimplemented.

## Native result resource adoption in 0.129

The shared native host now validates and transfers a completed context's owned
result resources into a receiving scope without selecting either context or following
borrowed references. Foreign/stale/duplicate owners and incomplete result graphs
reject before mutation. The compiled completion model uses this API; source execution
remains synchronous and scheduler integration is still ahead.

## Shared native retained execution in 0.130

The native continuation, context/completion and fault-domain implementation now lives
in normal generated support. Compiled regression probes use that same code for
retained results, readiness and fault/process cleanup; their scheduling drivers remain
test-only. Default native entries still use synchronous control flow. Source task
queue/wait wiring, resumable lowering and `spawn`/`yield` remain ahead.

## Shared task protocol and native invocation owners in 0.131

Reference and native support now share task queue, completion-join and readiness
transitions. Retained native activations have separate invocation owners even when
frames are reused; normal return requires settled tasks, and emergency retirement
invalidates only the affected owner's obligations. Native task payload/queue binding
and source `spawn`/`yield` remain ahead. Current source behavior is unchanged.

## Bound native task payloads in 0.132

Internal native cold-task APIs now bind captures to the original invocation, host
and compiled producer. Invalid queue admission preserves the handle and captures;
FIFO dispatch transfers them once. Source generation remains synchronous while
retained context dispatch and completion integration continue.

## Task-bound native contexts in 0.133

Internal queued tasks now retain their original parent and completion wait through
native context admission, readiness, result delivery and fault/process cleanup.
Failed admission preserves captures. Source callback and scheduler integration remain
ahead; current source execution stays synchronous.

## Scoped native task retirement in 0.134

Retained native scopes now retire discarded cold-task obligations during fault and
recovery cleanup, while preserving moved handles and borrowed suppliers. Normal
scope exits and loop restarts still require task completion. Source callback and
scheduler integration remain ahead.

## Sequential native await contexts in 0.135

Internal cold-await context chains now preserve the original diagnostic activations
through readiness, result delivery and faults. Exact report and resource checks cover
nested awaits and owning contingencies. Source callback integration remains ahead;
current source execution stays synchronous.

## Retained native await handoffs in 0.136

Internal compiled callbacks now retain cold-await captures and their return callback
across explicit admission and one-use result delivery. Failure and process cleanup
retire pending values before suppliers. Generated source callback integration remains
next; current source execution stays synchronous.

## Generated retained await callbacks in 0.137

Compiled probes now lower cold tasks into bound captures and explicit retained await
handoffs. Their drivers preserve original diagnostic chains, result ownership and
fault cleanup. Production lowering promotion is next; source execution remains
sequential and no scheduler syntax is enabled.

## Production retained native callbacks in 0.138

Normal native builds now use retained compiled callbacks for calls, constructors,
loops, boundaries and cold awaits. The shared sequential entry driver preserves loans,
resource cleanup, fuel and fault reports. Probe scheduling controls stay internal;
source execution remains sequential and no new syntax is enabled.

## Native independent-child cold awaits in 0.139

Internal native children can now perform nested cold awaits while preserving their
original fault cause, one-use result delivery and resource cleanup order. The shared
runtime has host and compiled O0/O2 evidence. Source execution remains sequential;
`spawn`, `yield` and parent awaits alongside live children remain gated.

## Prepared native owner awaits in 0.140

The internal native host admits an already-prepared cold await alongside independent
children. Sibling faults restore the original owning diagnostic chain before retiring
awaited work, including nested boundaries and active handlers. Normal parent execution
with live children and source scheduling remain gated.

## Native retirement checks in 0.141

Retained native frames, loops and lexical boundaries now check child/result
membership before runtime storage retirement. Final-frame domain checks also precede
frame release. Parent execution with live children remains gated pending checks
before generated callbacks move operands; source scheduling is not enabled.

## Native invocation ownership in 0.142

Every child and retained result now keeps its original parent invocation, preventing
another invocation in the same context from adopting it. Lower-level selection
rejects live children before callbacks or pending delivery can change state.
Fine-grained parent progress and source scheduling remain gated.

## Native callback access plans in 0.143

Generated native callbacks now describe symbolic reads, moves, borrows, writes and
retirement before evaluation. Normal cold-await result writes have a separate plan.
Queries preserve execution state; opaque accesses remain explicit. Parent progress
with live children and source scheduling still require further dependency checks.

## Native access storage resolution in 0.144

Native access queries now identify live storage and indirect reference-holder reads,
without consuming values or resource owners. Resolved allocation identities reject
retirement and offset reuse. Captured-loan checks and source scheduling remain ahead;
parent execution with live children is still guarded.
