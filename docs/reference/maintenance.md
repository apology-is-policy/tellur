# Maintaining the language reference

[Language reference](../language-reference.md) · [Roadmap](../language-revision.md)

The main reference and this directory form one maintained manual. Design notes can
explore alternatives; the manual describes what the current compiler accepts and
what its constructs mean. Historical release notes may retain older restrictions
when clearly dated, but the current manual must not contradict itself.

## Checkpoint checklist

For every language or editor checkpoint:

1. Update the affected semantic sections, including ownership, origins, evaluation
   order, visibility and interactions with other features. State remaining boundaries.
2. Add or update a complete checked example and a rejection example when a new rule
   can be violated. Keep examples small enough to explain the rule.
3. Update lexical/grammar entries for syntax changes, and the glossary when meanings
   change. Retain existing heading anchors where practical; repair incoming links
   if a heading moves or changes.
4. Update the feature matrix, roadmap completion boundary and conformance evidence.
   Mark proposed syntax as proposed in design notes; do not present it as working code.
5. Run the manual checks below alongside feature-specific compiler/editor/native
   tests. Documentation checks complement those tests; they do not replace them.
6. When releasing a new compiler version, update the manual's version marker and
   current-version statements in companion chapters. Documentation-only improvements
   do not require a new compiler binary version.

## Checking the manual

From the repository root:

```sh
cargo test --offline --test reference_manual
```

In the current development workspace, the shared build-cache form is:

```sh
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=../../work/path-build/target \
  cargo test --offline --test reference_manual
```

The suite checks every `path` fence in the main reference and Markdown files in
this directory. Each needs one of these HTML comments immediately before its fence:

| Annotation | Check |
| --- | --- |
| `<!-- tellur:check -->` | Compile as a standalone source unit using the playground manifest |
| `<!-- tellur:body -->` | Wrap in `control main() exits auto`, append `[entry]: exit done;`, then compile |
| `<!-- tellur:tail -->` | Wrap in `control main() exits auto` without appending an exit, then compile |
| `<!-- tellur:reject E_CODE -->` | Compile the complete source and require the specified diagnostic code |
| `<!-- tellur:fragment reason -->` | Exclude from standalone compilation; require a meaningful reason and a visible context-fragment label |

Prefer a complete example or a body wrapper. Use fragments only when the text
intentionally relies on declarations, paths or imports shown elsewhere; explain
that context to the reader. The test reports the counts of accepted, rejected and
fragment examples. Fragment classification is not evidence that a program compiles.
Use real multi-file fixtures and module tests for import behavior.

The same suite checks relative Markdown links and heading anchors, and requires
the main version marker to agree with Cargo's package version. It does not fetch
external URLs, validate shell commands, mechanically prove the EBNF, or establish
that prose fully matches semantics. Changes still need a semantic read-through.

Use `path` fences for executable examples. Use `ebnf` for grammar and ordinary
inline code for short schematic forms. Do not evade checks by relabeling a complete
Tellur example as plain text. Unsupported proposals belong in separately identified
design notes, not in the executable manual examples.

Canonical source fixtures and examples use `.tellur`, and Rust/editor regression
suites should read those copies. Preserved `.path` playground files are migration
snapshots, not synchronized mirrors. Keep dedicated legacy filename/command tests;
do not silently redirect modern regression coverage back to the snapshots.
