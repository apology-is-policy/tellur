# Outcome absorption and contributor changes

[Language reference](../language-reference.md) · [Public contracts](contract-snapshots.md)

Compiler 0.58 can show which recorded internal outcomes contribute to each public
exit. This complements contract comparison: a private helper can add `malformed`,
and its caller can map that outcome to an existing public `unavailable`. The public
signature stays the same; its contributor evidence changes.

Try [the runnable example](../../playground/outcome_absorption.tellur). Its public
`lookup` returns `ready` or `unavailable`. The latter records both private `missing`
and `malformed` contributors, including the intervening capture and handling.

## Terminal workflow

From the compiler checkout:

```sh
./bin/tellurc absorption-snapshot playground/outcome_absorption.tellur > /tmp/before.absorption.json
# Edit an internal outcome and its handling, keeping the public contract unchanged.
./bin/tellurc absorption-snapshot playground/outcome_absorption.tellur > /tmp/after.absorption.json
./bin/tellurc absorption-diff /tmp/before.absorption.json /tmp/after.absorption.json
```

`absorption-snapshot SOURCE [MANIFEST]` checks the complete explicit import graph
and writes JSON only after checking succeeds. Manifest discovery is the same as
`check`. Export builds no native program and executes no host code.

`absorption-diff BEFORE_JSON AFTER_JSON [--check]` compares the two artifacts without
loading source or a manifest. It reports `public_contracts_equal` separately from
`recorded_evidence_equal`, followed by their respective change lists. Contributor
locations are omitted from comparison; they remain in the snapshots for navigation.

Without `--check`, a successfully generated diff returns status 0. With `--check`,
status 0 means the public contracts and recorded evidence match, and neither input
contains truncated contributor lists. Differences or truncation return status 1.
Malformed artifacts, unsupported versions and invalid source return a nonzero
status without a successful JSON result. Neither command writes a baseline;
shell redirection has its usual overwrite behavior.

The settings application carries [a baseline](../../applications/settings/settings.absorption.json):

```sh
./bin/tellurc absorption-snapshot applications/settings/settings.tellur > /tmp/settings.absorption.json
./bin/tellurc absorption-diff applications/settings/settings.absorption.json /tmp/settings.absorption.json --check
```

Its regression introduces a private `blank` validation outcome absorbed into the
existing `invalid_key`. Public contracts compare equal while contributors differ.
The baseline currently fits the contributor budget for every exported outcome.

## Editor workflow

With a saved Tellur file active, run **Tellur: Show Outcome Absorption Snapshot**.
The language server checks the current buffer and unsaved imports, then opens an
editable JSON document. Save As can retain the snapshot. Rerun the command after
editing source; the JSON view is a fixed snapshot rather than a live document.

The command requires a running server, a trusted workspace, a file identity and
a valid manifest. A failed check opens no partial or cached report. It does not
save or alter the source. **Tellur: Show Public Contract Snapshot** remains available
when only the public interface is wanted.

## Reading the evidence

The artifact has `format: "tellur.outcome-absorption"`, `version: 1`, an `analysis`
profile, an embedded `contracts` snapshot, and a `controls` map. Module/declaration
identities follow [public contract snapshots](contract-snapshots.md): `@entry::lookup`
or `file:relative/path.tellur::lookup`. No checkout-specific compiler hashes appear.

Each nongeneric public control has an `outcomes` map. Each outcome contains a bounded
set of `contributors` and a `truncated` flag. Contributor kinds are:

| Kind | Meaning |
| --- | --- |
| `produced` | An explicit exit in a source control contributes this outcome |
| `constructed` | A locally constructed completed outcome or ordinary variant supplies this tag |
| `received` | This control receives the named outcome at an operation site |
| `forwarded` | Explicit forwarding passes the named operation outcome onward |
| `declared` | The signature declares this outcome, but no producing exit was recorded |

Every contributor includes its `control`, `outcome`, optional `operation`, `site`,
and source `location`. Locations use one-based line and Unicode-scalar column
coordinates within the named module. `@entry` denotes the requested source file.
Declaration evidence does not assert that an execution can reach that outcome.

Sites distinguish occurrences within a control. A named decision such as
`reading = delta ...` retains `reading` in its site name. The remaining site
component is a local source-order ordinal over checked instructions, excluding
emit/drop/release-only positions. It is stable across formatting and declaration
reordering, but inserting or removing contributing statements can renumber sites.
It is an explanation identifier, not new Tellur syntax or a permanent source ID.

The comparison treats contributors as sets, preserving distinct sites. Import
alias changes, checkout relocation, comments and formatting do not create evidence
changes. Source locations may still change. Renaming a private control or a named
decision changes the recorded identity even if runtime behavior stays the same.

Public generic templates have `generic_definition_only: true` and no body evidence
at that root. They are still universally checked, and their public signatures remain
in `contracts`. Concrete generic and trait calls reached from nongeneric public
controls can contribute their checked bodies. The report does not merge all possible
generic instantiations into a purported universal body summary.

## Analysis boundary

The profile is `structural_source_contributors_v1`, with at most **32 contributors
per outcome**. This is the existing diagnostic lineage analysis exposed as typed
evidence; diagnostics and snapshots share that analysis. A limit hit propagates the
`truncated` flag. Equal truncated lists cannot pass `absorption-diff --check`.

Supported lineage includes direct calls/forwarding, capture and moves, local record
and sum payloads, stored/nested adapters, delayed exits, and merged labels. Guarded
replacement excludes overwritten sources. A direct borrowed match can inspect
known local lineage. The report is a set of source contributors, not an ordered
trace or a complete invocation tree; repeated calls retain distinct receiving sites
while their shared source definitions may appear once.

The artifact explicitly lists unsupported analysis dimensions. Loop bodies receive
one structural traversal; loop-carried values are unknown. Indirect storage,
dynamic collection contents and cross-call payload dependencies are not tracked.
Potentially mutating calls conservatively forget local value lineage. Scalar values,
Boolean condition expressions, input feasibility, complete histories, runtime
effects and execution frequencies are not compared. A changed numeric limit or
predicate may therefore leave both contracts and contributor evidence unchanged.

An empty list is not proof that an outcome has no source, and `truncated: false`
only says this list did not hit its size limit. Equal evidence is not behavioral
equivalence, a compatibility verdict, an ABI promise or a proof that every internal
change was found. A hand-edited snapshot is compared as supplied; reading it checks
the schema/profile, not the original program. Unknown fields, duplicate object keys,
duplicate contributors and unsupported profiles/versions are rejected.
