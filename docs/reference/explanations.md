# Explaining paths and outcomes

[Language reference](../language-reference.md) · [Outcome absorption](outcome-absorption.md)

Two questions are available directly from the checker: which structural histories
select this piece of code, and which recorded outcomes contribute to one control
completion? Both work with private code. They expose the same bounded facts used by
selected-history hover and outcome absorption snapshots.

## Terminal

From the repository root:

```sh
./bin/tellurc explain playground/path_explanations.tellur --at 23:5
./bin/tellurc explain playground/path_explanations.tellur --control classify --outcome invalid
./bin/tellurc explain applications/settings/settings.tellur --control validate_key --outcome invalid --json
```

The command is `explain SOURCE [MANIFEST] (--at LINE:COLUMN | --control NAME
--outcome NAME) [--json]`. The manifest is discovered like `check` when omitted.
Without `--json`, stdout is a readable Markdown report with source links. Links open
the source line; their labels show the exact scalar column. `--json`
emits a structured report containing the same Markdown. Successful queries return
zero, including reports marked as truncated. Invalid source, unavailable evidence,
unknown targets and malformed arguments return nonzero with no partial stdout report.
This is an inspection command, not a completeness or CI equality check.

`--at` uses **one-based Unicode scalar** line and column positions in `SOURCE`, like
compiler diagnostics. Put the position on a token in a statement, path label,
path alias, group, loop or delta arm. The smallest enclosing selection is explained.
Comments, whitespace and declaration headers do not have a selected history. The
position is an exact query, not a search for the nearest executable statement.

An outcome query names an ordinary concrete control declared in `SOURCE`, followed
by one of its outcomes. Private controls are included. Query an imported file as
`SOURCE` to inspect its private roots; import aliases are not query namespaces.
Generic definitions and trait operations cannot yet be outcome-query roots.
Concrete generic and trait calls still appear as contributors to ordinary roots.
Source-position queries do work inside generic and trait bodies.

Try [the runnable example](../../playground/path_explanations.tellur). The `invalid`
completion of `classify` combines missing and invalid input paths. Its main control
uses the `inspected` alias for an outcome union; the group continuation runs once,
even though two alternatives are listed.

## VS Code

With a Tellur file active:

- **Tellur: Explain Path Selection at Cursor** opens the selected history at the
  cursor, with decision origins and links back to their source positions.
- **Tellur: Explain Outcome Contributors** offers a picker of the file's ordinary
  concrete controls and their outcomes, including private ones, then opens a report.

The result is an editable Markdown document. Use Markdown preview for rendered
links, or Save As to keep notes. Reports are fixed at request time; rerun the command
after editing. Generating a report neither saves nor changes the source.

Both commands check the current unsaved buffer and its explicit imports. An invalid
source graph or manifest produces an error instead of a stale or partial report.
Outcome queries recheck after the picker, so changes made while choosing do not reuse
old evidence. Save an untitled file first to give imports and source links an identity.
The language server must be running in a trusted workspace.

## Reading the result

A selection report expands aliases into `&` and `|` conditions. `entry` means the
entry for the current checking context. A loop body is explained within one
iteration: this is not the entire history through previous iterations. Repeated
local outcome names receive `@line:column` suffixes where needed. A structurally
empty selection is explicitly marked; other selections are not claims that a
concrete input can reach the code.

At most **96 captured condition nodes**, **48 levels**, and **8 displayed decision
origins** are retained for a selection. `condition_truncated` and
`origins_truncated` distinguish the two omissions. An ellipsis is an unknown
omitted condition, never shorthand for unconditional execution.

An outcome report is a **set of contributors**, not their execution order or a
complete call tree. It distinguishes declarations, produced/constructed outcomes,
and received/forwarded outcomes. Declarations are contract facts, not proof that an
outcome is produced. It retains at most **32 contributors per outcome**; `truncated`
is explicit when that bound is exceeded. Absence from the set is not a proof of
unreachability. The [absorption analysis limits](outcome-absorption.md#analysis-boundary)
also apply: no concrete feasibility, complete execution histories, loop-carried
payload lineage, indirect storage, dynamic collection contents, cross-call payload
lineage, or uninstantiated generic-root evidence.

## Structured format and editor protocol

JSON reports have `format: "tellur.explanation"`, `version: 1`, `query`, `result`,
`source`, `modules`, and `markdown`. `result.kind` is `selection` or `outcome`.
Selection ranges are half-open, one-based scalar positions. Outcome contributor
locations use the same relative module identities as absorption snapshots;
`modules` maps those identities to absolute navigation paths. Because it includes
current locations and absolute paths, this report is not a portable comparison
artifact. Use contract/absorption snapshots for reviewable baselines.

The language server advertises `experimental.tellurExplain: 1`. Requests use
`textDocument: {uri}` with `tellur/explainTargets` to list outcome targets, or
`tellur/explain` with one of these `query` objects:

```json
{"kind":"selection","position":{"line":22,"character":4}}
```

```json
{"kind":"outcome","control":"classify","outcome":"invalid"}
```

LSP input positions use **zero-based UTF-16** coordinates; returned report positions
remain one-based scalars. A position inside a surrogate pair is invalid. Malformed
requests and failed checks return errors, never an older successful report.

Lexical contingency selections carry `contingency: true` in structured selection
reports. Their hover defines `entry` as handler entry after failed work retires;
it does not describe an ordinary continuation of that work.
