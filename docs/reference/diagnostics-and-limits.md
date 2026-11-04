# Diagnostics and limits

[Language reference](../language-reference.md) · [Feature status](support-status.md)

The compiler reports diagnostic codes, source locations and explanations. Since
0.52, checking collects errors from independent control bodies; 0.73 also recovers
across independent straight-line statements within a body. Release 0.74 extends
recovery across disjoint paths and nested scopes; 0.77 collects independent
public-shape and signature errors, and 0.78 adds named completion bindings. Release
0.83 extends statement recovery to named boundary work and contingency bodies, and
0.84 adds lexical boundary work bodies; 0.85 adds lexical contingency handlers;
0.86 adds explicit exit-adapter arms; 0.87 adds explicitly typed stored-adapter arms,
0.93 inferred stored adapters, and 0.94 inferred exit adapters. Release 0.192 also
collects independent origin-schema declaration errors before later preparation.
Fixing an error
may expose another within that body or a dependent control. The precise
message wording may improve between releases. The negative examples here are checked
for their stated code, so they also serve as small regression cases.

## Public interfaces: E_PUBLIC_CONTRACT

A public control must spell out its exit labels, payload types and borrowed-result
origins. Private helpers may infer these from their bodies.

<!-- tellur:reject E_PUBLIC_CONTRACT -->
```tellur
pub control answer() exits auto {
    exit ready { 42 };
}
```

Write `exits ready { u32 value }` or use a public named outcome contract. Replacing
`exits auto` with `exits ready...` still requests inference and is also rejected.
Since 0.194, this diagnostic selects the written `exits auto` clause or the first
inferred `label...` clause in source order. In a mixed or wrapped contract such as
`exits { plain, zebra..., alpha... }`, it selects `zebra...`; commas, braces and
other alternatives are excluded. Internal whitespace and comments are included,
while trailing comments and the control body are excluded. Legacy `region ... exits
auto` keeps the same range behavior; legacy payload inference remains a syntax error.
Imported and unused generic definitions keep their original header coordinates.
Missing synthetic metadata retains the control-name fallback.

<!-- tellur:reject E_PUBLIC_CONTRACT -->
```tellur
pub control select(bool flag) exits plain, ready... {
    delta flag {
        true => exit plain,
        false => exit ready { 1 }
    }
}
```

The error selects `ready...`. Making the control private permits inference; spelling
`ready { u32 value }` preserves an explicit public contract. The public-shape batch
still reports one error per control before shared schemas and bodies, up to 32 errors.
Fixing one unsaved declaration removes its error while independent failures remain;
checked hover returns only after the complete program validates. Missing borrowed
result origins are checked in the later signature phase.


<!-- tellur:reject E_PUBLIC_CONTRACT -->
```tellur
pub control view(ref string input) exits ready { ref string value } {
    exit ready { input };
}
```

Add `from input` after the result binding. Reference-bearing aggregate results
need explicit bracket mappings as well; references to such aggregates need both
storage and contained-origin contracts. An unused generic public declaration must
satisfy the same rule. Since 0.195, a missing `from` or aggregate bracket contract
selects the written result type, excluding the binding name and any existing origin
clauses. For a named completion it selects the completion type at the control's use,
including type arguments, rather than the shared schema's field declaration. The
absent clause has no source range of its own. Imported and generic definitions retain
authored coordinates; synthetic types without metadata retain the control-name point.
The first missing result contract in each control still wins, and earlier schema,
shape and named-binding failures still block signature checking. See [explicit public contracts](../language-reference.md#explicit-public-contracts).

For incompatible `forward` calls, missing adapter arms and unfinished identity
paths, CLI notes and VS Code
related information can show producing exits, calls, captures and adapters. A
private helper can absorb several input alternatives into one result: the result's
notes retain those structural contributors. Notes also accompany incompatible
inline adapter results.

Since 0.46 this includes moved completed values, delayed `exit saved`, nested
adapters, local record fields and nested sum payloads, direct borrowed decisions,
and merged path labels. Different fields and alternatives stay separate. Disjoint
writes are filtered by the selected path; overwriting a value removes its old
lineage on that path. This analysis does **not** narrow a value's type: every
contract alternative still needs a disposition, even if its only known construction
was overwritten. Since 0.196, an outcome declared by an ordinary control selects its complete written `exits` clause,
including alternatives, payloads and origin mappings but excluding a following
`faults` annotation or body. Named contracts select that clause at the control's use,
not the shared schema declaration. Imported and generic headers retain authored
coordinates. All declared alternatives share this header range; the note identifies
the relevant alternative by name. Lowered trait methods, constructors and named
boundaries retain the original point when no ordinary declaration header matches. The range is attached after aggregation, preserving raw
contributor evidence, note deduplication/order and omission limits.

<!-- tellur:reject E_ARMS -->
```tellur
control lookup() exits auto {
    exit missing;
}
control main() exits done {
    auto saved = capture lookup();
    auto adapted = delta saved { missing => unavailable {} };
    exit delta adapted {};
}
```

The missing `unavailable` arm points back to its adapter construction, the capture,
and `lookup`'s `missing` exit. Several source contributors do not prove that all
paths can run for real inputs. Summaries and diagnostics retain at most 32 notes
plus an omission note.

Coverage is deliberately incomplete: loop-carried values, indirect reference
aliases, dynamic collection elements, and payload dependencies across calls are
not traced. Calls summarize their result alternatives rather than substituting
caller argument histories. Unknown indirect writes, calls with exclusive-reference
parameters and collection operations conservatively forget stored-value lineage
on the affected histories. Their actual ownership, typing and exhaustiveness
checks still run normally. [Statement recovery](#statement-recovery) has separate,
conservative limits.

## Machine-readable checking

Since 0.47, use:

```sh
./bin/tellurc check playground/count.tellur --json
./bin/tellurc check main.tellur manifest.json --json
```

The command writes **one JSON object to stdout**, with no normal progress text,
and returns OS status 0 for a successful check or 1 for a failed check. This is
compiler status; executable outcome-to-status mapping is a separate launcher
policy. The legacy `pathc` command accepts the same option.

| Field | Meaning |
| --- | --- |
| `schema` | `tellur.check.v1`; consumers should check this identifier and allow additional fields |
| `ok` | Whether the check succeeded |
| `position_encoding` | `one_based_unicode_scalars`; columns count Unicode scalar values, not bytes or UTF-16 units |
| `diagnostics` | Empty on success; one or more diagnostics on failure, capped at 32 |
| `diagnostics_limited` | True when collection reached the 32-error budget; remaining statements or controls may be unchecked |
| `summary` | Counts `controls` and `generic_definitions` on success; null on failure |

Each diagnostic contains `code`, `message`, `path`, `position`, `range`, `notes`
and `witness`. A position is `{ "line": 2, "column": 7 }`. Since 0.51, `range`
is either `{ "start": position, "end": position }` or null. Its start is included
and its end is excluded; `position` remains the same start point for existing
consumers. A missing position is null, as with a manifest error
that has no checked source location. A missing path is null, as with command
usage errors. Notes have their own `path`, `position`, `range` (since 0.193) and `message`; imported
source locations refer to their original files. Message wording is for humans
and can change; consumers should use codes and structured fields.

Path-related errors such as `E_UNCOVERED`, `E_UNAVAILABLE` and ownership conflicts
can carry a witness:

```json
{
  "kind": "structural_may_reach",
  "control": "main",
  "selected_paths": ["read_failed", "close_failed"]
}
```

These are selected path **display names** in one satisfying structural history.
They are not concrete input values or a proof that the history is feasible.
An empty list represents the entry history. `witness` is null when the error has
no path witness; there is no need to parse the explanatory message.

The LSP supplies the same witness through diagnostic `data`, with schema
`tellur.diagnostic.v1`. Its normal ranges retain the LSP's zero-based UTF-16
coordinates. JSON checking includes usage, configuration,
manifest and file-read failures; it does not build or execute Rust adapters.

## Expression locations

Since 0.51, value expressions retain their written source ranges through module
resolution and generic specialization. Type, move, field and borrow errors found
while checking an expression select that expression. A bad call argument selects
the argument; an argument-count error selects the call. For example, in
`u32 count = "three";`, the type error selects `"three"`. In a comparison with
incompatible operands, it selects the right operand. A more precise child error
keeps its own range as it passes through the enclosing expression checks.

Ranges include written parentheses, string delimiters and escape sequences, and
can cross lines. Comments and whitespace following an expression are excluded.
Parser errors select the unexpected token; an error at end of input has an empty
range. Imported-file diagnostics point to the original file; VS Code includes
that location as related information and uses the current unsaved dependency text.

Expressions and the type annotations below have independent source ranges. Other
checks can still carry source points: their JSON `range` is null and VS Code uses
its existing token-sized selection. Hover information always comes from the current
checked source rather than a stale successful version.

## Type-annotation locations

Since 0.97, parsed field, parameter, payload and typed-pattern annotations retain
their complete **written type**. Type validation in source data/outcomes, control and
constructor signatures, generic data/control preparation and trait declarations can
select that annotation. Typed delta-binding mismatches do likewise. Module lookup
and trait-visibility errors discovered while validating such types also select them.

<!-- tellur:reject E_MANIFEST -->
```tellur
struct settings {
    vector<
        string,
        u32
    > entries;
}
```

`vector` takes one element type. This error selects from `vector` through `>`, across
all four lines; it excludes the binding name `entries` and following semicolon.
References include their written `ref mutable` (or legacy `&`) prefix. Source types
retain whitespace and line comments inside the annotation, but exclude trailing
comments, origin brackets/arrows, `from` clauses and binding names. Legacy
`name: type` fields select the type, without the name or colon.

Since 0.100, type validation for ordinary and supervised **loop parameters** uses
these ranges too. This includes module lookup/visibility and generic type preparation.
For `loop count(vector<string, u32> entries = [])`, the invalid type selects
`vector<string, u32>`; it excludes `loop`, the binding modifier `mutable`, the name and initializer.
Previously some loop type errors had no source position at all.

<!-- tellur:reject E_MANIFEST -->
```tellur
control main() exits done {
    loop pending(vector<string, u32> entries = []) {
        exit done;
    }
}
```

An initializer mismatch still selects its expression or the more precise invalid
child expression, as does an `again` argument error. Duplicate parameter names,
loop placement and other structural errors retain their existing locations.
This changes coordinates, not which types, loops or histories are accepted.

Since 0.101, local variable annotations retain their written ranges as well. Module
lookup and generic preparation errors use them for ordinary declarations, `capture`
and stored `delta` adapters. Ordinary local type validation and stored adapter target
validation select the annotation, including multiline types and reference prefixes.
The binding modifier `mutable`, name, trailing comments and initializer are excluded.

<!-- tellur:reject E_MANIFEST -->
```tellur
control main() exits done {
    vector<string, u32> entries = [];
    exit done;
}
```

Here the error selects `vector<string, u32>`. By contrast, `u32 count = true;`
selects `true`; `vector<u32> numbers = [1, true];` selects the invalid element.
Capture compatibility and other checks that previously had only a statement point
keep that point; the type range is not assigned to every declaration error. `auto`
inference and generated temporary bindings have no invented annotation range.

Since 0.102, the late public-interface visibility check also selects authored types:
public struct fields, variant/outcome payloads, control parameters and explicit
control result payloads. For example, this selects `vector<hidden>` because a public
field cannot expose a private type:

<!-- tellur:reject E_VISIBILITY -->
```tellur
struct hidden {
    u32 value;
}
pub struct container {
    pub vector<hidden> values;
}
```

The same rule covers producer-indexed `outcome<operation>` types in public interfaces.
The checker still validates bodies before this phase and reports its existing first
visibility error, checking exported data before control signatures. No extra batch
or new visibility rule is introduced. Generated associated-type checks without
authored type metadata retain their declaration points.

Since 0.104, a named completion type after `exits` also has an authored range.
Module lookup, generic type preparation, trait completion-type validation and public
visibility errors select that type, including qualified names and multiline generic
arguments. For `exits replies.answer<string>[input]`, the type range ends at `>`;
the origin mapping `[input]` is a separate contract and is not included. A private
named contract still cannot appear in a public signature, even with nullary exits.

<!-- tellur:reject E_VISIBILITY -->
```tellur
outcome internal_reply {
    done
}
pub control work() exits internal_reply {
    exit done;
}
```

A lone bare label such as `exits done` still means a nullary alternative when it
does not name a declared outcome type. Explicit braced contracts and `exits auto`
keep their existing meaning. This location metadata does not change resolution,
public visibility, origin checking or generic compatibility.

Since 0.114, a separate range covers the complete **named completion origin mapping**.
For `exits reply[input['source]]`, an invalid input/group or incompatible shared versus
exclusive origin selects `[input['source]]`, including nested brackets. A wrong
number of origin arguments selects the same mapping. Multiline whitespace and
comments inside the brackets are retained; the named type and trailing comments
are excluded. These ranges also apply to generic controls and trait signatures.

<!-- tellur:reject E_LIFETIME -->
```tellur
outcome view['a] {
    found { ref string value <- 'a }
}
control peek(ref string input) exits view[missing] {
    exit found { input };
}
```

Here `missing` does not name a parameter; the diagnostic selects `[missing]`.
Missing mappings have no invented range. Shared schema errors use the field mapping locations described below when available. A body
error still selects its own expression. Error order and the shared recovery budget
are unchanged; improving a location does not enable checking through an invalid schema.

Since 0.115, explicit borrowed-result **`from` clauses** have their own ranges.
Unknown input names, invalid contained selectors, incompatible borrow authority and
`from` on a non-reference payload select the complete clause, from `from` through
the final input name or `]`. The payload type/name and trailing comments are excluded;
internal whitespace and line comments are preserved. This applies to modern and
legacy control signatures, generic controls, and trait declarations/implementations.

<!-- tellur:reject E_LIFETIME -->
```tellur
control peek(ref string input)
    exits found { ref string value from missing }
{
    exit found { input };
}
```

The diagnostic selects `from missing`; `from input['missing]` would select the whole
contained-selector clause. Each result field retains its own source range, even when
a preceding field has a valid clause. An absent clause still has no invented range.
The declaration batch still reports its first error per control before checking bodies.
Named completion mappings keep their bracket ranges from 0.114; body and type errors
keep their more specific locations. Lifetime acceptance and public contract rules are
unchanged.

Since 0.116, **individual payload origin brackets** also have separate ranges.
For `exits ready { view[input] value }`, invalid input/group names, wrong group
counts, missing grouped types and incompatible borrow authority select `[input]`.
The payload type, binding name, trailing comments and any storage `from` clause are
excluded. Multiline comments, nested selectors, modern/legacy spelling, generic
controls and trait declarations/implementations preserve the written bracket range.

<!-- tellur:reject E_ORIGIN -->
```tellur
struct view['a] {
    ref string value <- 'a;
}
control peek(ref string input)
    exits found { view[missing] value }
{
    exit found { view { value: input } };
}
```

Here the diagnostic selects `[missing]`, even if another payload has valid brackets.
For named completions such as `exits reply[missing]`, that invocation mapping takes
precedence over the payload's group names in the reused outcome definition. A separate
invalid `from` clause keeps its own range. Missing mappings retain their existing source points; schema field mappings use the
ranges described below, and body and type errors keep their own ranges.
Origin acceptance, declaration ordering and the 32-error budget are unchanged.

Since 0.190, **schema field origin mappings** select their written brackets or
storage arrow. This applies to structs, variants and named outcomes, including
unused generic definitions and legacy `name: type` fields. Unknown groups and
non-group entries select the mapping containing that entry. Mapping-wide errors
(such as incomplete groups or a field with no reference origins) select referent
brackets when present, otherwise the primary bracket or storage-arrow mapping.

<!-- tellur:reject E_ORIGIN -->
```tellur
struct view['a] {
    ref string value <- 'missing;
}
control main() exits done {
    exit done;
}
```

This selects `<- 'missing`, excluding the field name and semicolon. For a carrier
field `ref view['missing] value <- 'a`, the error selects `['missing]`; an invalid
storage group instead selects its arrow. Internal comments and whitespace are
included; trailing comments are excluded. Storage groups are still checked before
referent groups, even though their spelling follows the brackets. Missing mappings
and schema depth limits keep their existing fallback points. Schema failures block
body checking; independent schema recovery is described below.

Since 0.191, **duplicate and unused declared origin groups** select the individual
apostrophe name in a struct, variant or named-outcome header. A duplicate selects
the repeated occurrence, while an unused group selects its declaration. The range
includes internal whitespace/comments between the apostrophe and name, but excludes
commas, brackets and trailing comments. Generic definitions retain the authored
coordinates even when unused or reached through transitive imports.

<!-- tellur:reject E_ORIGIN -->
```tellur
struct view['a, 'unused] {
    ref string value <- 'a;
}
control main() exits done {
    exit done;
}
```

This selects `'unused`. In `view['a, 'a]`, the second `'a` is selected. Duplicate
groups are still checked before fields; unused groups are checked after all fields
validate, so an invalid field mapping keeps priority over an unused group. The first
unused group follows declaration order. Missing mappings, malformed header syntax
and global schema limits retain their existing diagnostic behavior. Shared schema
preparation must succeed before any schema reaches later checking or hover.

Since 0.192, checking collects **independent origin-schema failures** from structs,
variants and named outcomes, including unused generic declarations and imports.
Each declaration retains its existing first-error validation order. If checking a
nested schema fails, declarations that depend on that failure are skipped, including
transitive and shared dependencies. Fixing that schema can expose further errors in
its dependents. Independent declarations continue up to the shared 32-error limit.

<!-- tellur:reject E_ORIGIN -->
```tellur
struct first['a, 'a] {
    ref string value <- 'a;
}
struct second['b, 'unused] {
    ref string value <- 'b;
}
control main() exits done {
    exit done;
}
```

Checking this program reports the duplicate `'a` and unused `'unused` together.
Repairing either declaration removes its error without hiding the other. Partial
schemas never reach trait lowering, instantiation, body checking or editor hover;
those phases resume only after every schema succeeds. The first-error compile API
still returns the same first diagnostic. Ambiguous declaration identities retain
first-error behavior; parsing, import loading and earlier preparation keep their
existing barriers. Schema cycles, depth and size limits retain their diagnoses,
while unrelated declarations can still report their own failures. Ordinary recursive
layouts without origin schemas remain a separate validation phase.

Since 0.193, **contributor notes** also carry optional half-open ranges. When a
note's source anchor exactly matches an authored statement or outcome expression,
the note selects that occurrence: for example, `exit changed;`, `forward leaf();`,
a capture statement, or the inner `changed {}` of an adapter. If multiple recorded
occurrences share the same start, the shorter range is used. Internal comments and
newlines are included; trailing comments and surrounding expressions are excluded.
Statement delimiters are included when they belong to the recorded statement.

The note's existing position and message stay unchanged. CLI JSON exposes a `range`
object or null for every note; editor related information converts an authored range
to UTF-16 using the current source buffer. Declaration notes, omission markers and
occurrences without exact parser metadata retain their point/token fallback. Generic
substitution and private/imported names never determine the range length. Notes still
have the same order, deduplication and 32-note limit plus omission marker; this adds
no new provenance claims or recovery paths. Outcome-absorption snapshots are unchanged.

Imports preserve both endpoints in the original file; unsaved dependencies supply
the text used for editor UTF-16 conversion. Generic substitution preserves the
authored spelling, including a type parameter, rather than estimating a range from
a longer lowered type name. Reused named-outcome payload annotations keep their
original declaration locations. CLI JSON continues to use one-based Unicode scalar
coordinates; LSP uses zero-based UTF-16 coordinates.

This is not a range for every declaration error. Other whole-contract shape/origin errors,
associated normalization, contributor declaration notes and checks without authored metadata retain
their existing locations. Synthetic fields have no invented annotation range.
More precise child errors keep their own coordinates. Diagnostic codes, phase order,
batch limits and acceptance rules are unchanged.

## Independent body errors

Since 0.52, textual `check`, `check --json` and the language server collect the
errors from independent control bodies in the current checking phase. Since 0.73,
some bodies can contribute several errors through statement recovery below.
They check dependencies before callers, then report errors in source order within
the module graph. If a body fails, its partial inferred contracts, bindings and
ownership state are discarded. Callers and controls whose types depend on that
failed producer are skipped, transitively. This includes `outcome<producer>` in
parameters, local annotations, loop parameters and arm payload annotations.

For example, separate type errors in two leaf helpers can appear together.
A caller of a broken helper does not also report misleading missing-arm or
payload errors based on an unfinished inferred contract. Repairing the helper
allows that caller to be checked again; its own errors may then become visible.
This conservative policy also applies to helpers with explicit exit contracts.

Collection stops at **32 errors**. JSON reports `diagnostics_limited: true`;
text output and the editor display a limit notice. The count is a budget, not
an estimate of all remaining errors. A failed report has no success summary and
never supplies an executable or partially checked program. Inferred hover remains
unavailable until the current program checks successfully; declared syntax can
still appear in outline or hover.

Recovery is deliberately bounded. Parsing, import/name resolution, shared type
validation outside the origin-schema batch, implementation normalization/conformance, generic expansion
and graph-cycle failures still stop at their first error. Independent control
declarations use the three batches described below; constructor-call access has
its own preparation batch since 0.91, generic-bound declarations since 0.92, and
trait declarations since 0.95, and implementation-local preparation since 0.197. Since 0.96, constructor definitions also have an earlier
file-local preparation batch. Independent origin schemas have a separate batch since
0.192, with failed dependencies suppressed. Generic definition bodies may
report several errors during universal checking; a failed phase prevents later
concrete checking. Broader recovery across shared preparation remains future work.

Compiler integrations can use `check_source`, `check_file` or
`modules::check_with_loader`, which return a `Diagnostics` batch on failure.
The existing `compile`/`compile_file`/`compile_with_loader` APIs retain their
single-error return types and select the first diagnostic from that batch.

## Control-declaration batches

Compiler 0.78 checks independent declarations within three preparation phases
(public shape and concrete signatures were added in 0.77):

1. Public contract shape: each public control must declare its exit labels and
   payload types, or name a declared outcome contract. Several public `exits auto`
   or `ready...` declarations can be diagnosed together, including generic controls.
2. Named completion binding: after shared outcome schemas, layouts and trait checks
   succeed, each control binds its named contract's origin arguments. Wrong origin
   counts in separate controls can be reported together, as can origin arguments
   supplied to a bare exit or manifest-only contract. Invalid shared schemas still
   stop before this batch; unknown origin sources and deeper lifetime errors are
   checked in later phases.
3. Concrete signature preparation: after shared types and named contracts resolve,
   each control checks boundary input ownership, explicit public return origins,
   operation/parameter/exit names, supported input/result types and declared borrow
   summaries. Errors in separate controls can appear together.

Each phase reports at most one error per control and shares the existing 32-error
budget. The first failed phase stops compilation: later phases and all body checks
remain unavailable until those declaration errors are repaired. Successful siblings
never supply a partially valid program or stale inferred/call-site hover. A later
declaration cannot reuse an earlier declaration's operation name just because that
earlier declaration has another error.

This is not recovery within a shared layout or a parser error. Fixing a declaration
may expose another error in that declaration, a later preparation phase, or a body.
CLI JSON preserves the original diagnostic code and source point; existing ranges
remain present where supported. Imports keep their defining-file locations, and the
single-error compiler APIs still return the first diagnostic in the failed phase.
Try [declaration_diagnostics.tellur](../../playground/declaration_diagnostics.tellur):
rename both secondary parameters to `value`, then repair them independently.
For borrowed contracts, try
[contract_binding_diagnostics.tellur](../../playground/contract_binding_diagnostics.tellur):
change both `exits view[input]` bindings to `exits view[input, input]`. The schema
declares one origin group, so both controls report `E_ORIGIN`. Repairing one leaves
the other diagnostic; repairing both restores checked call and inferred-type hover.

<!-- tellur:reject E_ORIGIN -->
```tellur
outcome view['source] {
    found { ref string value <- 'source }
}
control first(ref string input) exits view[input, input] {
    exit found { input };
}
control second(ref string input) exits view[input, input] {
    exit found { input };
}
```

This produces two diagnostics through the batch APIs and CLI; the compatibility
single-error APIs return the first. A declaration with no origin arguments keeps
the existing inference/public-contract rules: an empty list is not a new arity error.

## Generic-bound preparation batches

Since 0.92, independent generic controls can report invalid requirement declarations
together. This phase runs after parsing, module/name resolution and public contract
shape checks, before trait lowering and constructor-call preparation. It checks:

- Duplicate requirements after resolving import aliases to trait identities.
- A requirement that is neither a sealed capability nor a declared trait.
- A private trait required by a public generic control.

The rules for `copy`, `clone`, `drop` and user traits are unchanged. The phase reports
one error per control, including unused generic definitions, under the shared
**32-error budget**. It does not invent witnesses or continue through trait lowering,
generic expansion, signatures or bodies after a bound error. Repairing the first
bad requirement can expose another in the same control; later phases resume only
when every declaration in this phase is valid.

<!-- tellur:reject E_VISIBILITY -->
```tellur
trait internal_tag {}

pub control first<t: internal_tag>(t input) exits ready { t value } {
    exit ready { input };
}

pub control second<t: internal_tag>(t input) exits ready { t value } {
    exit ready { input };
}
```

Both public interfaces expose the private trait and report `E_VISIBILITY`. Making
the trait public, or making both controls private, repairs this phase. Duplicate
resolved requirements and invalid trait/capability names report `E_BOUND`.
Names rejected during module lookup still stop earlier; this batch does not recover
from an unresolved import or namespace. Unknown names can reach this phase in the
source-only API, while file-based resolution may reject them earlier.

CLI/LSP retain each declaration's original source point and imported-file location.
Single-error compatibility APIs return the first error. Unsaved edits update the
whole batch; checked call and inferred-type hover remain unavailable until a full
successful check. Trait declarations have the separate batch below. Associated-type
normalization, shared origin schemas, trait coherence and concrete witness conformance
retain their existing barriers.

## Trait-declaration preparation batches

Since 0.95, independent trait declarations report errors together after public
control shapes, generic requirements, shared origin schemas and trait identities
validate. Each trait supplies at most one diagnostic under the shared **32-error
budget**, whether or not a concrete implementation uses it. This phase checks
duplicate members, reserved associated names, explicit method profiles, payload type
names/arity, public visibility and declared result origins.

<!-- tellur:reject E_TRAIT_SIGNATURE -->
```tellur
trait reader {
    control read() exits auto;
}

trait writer {
    control write(string value) exits auto;
}
```

Both declarations need explicit exit contracts. An error in one trait does not
prevent the other declaration from being checked, but a second error inside the
same trait may appear only after repairing its first. Unknown types rejected by
file/module resolution still stop earlier; source-only checking can diagnose them
in this declaration phase.

Duplicate or reserved trait identities and shared origin-schema failures remain
barriers before this batch. A failed declaration batch blocks implementation lookup,
coherence, associated-type substitution, method conformance, generic expansion and
body checking. It publishes no witness or partially lowered method. The accepted
trait, ownership and origin rules are unchanged.

Imported diagnostics retain their original source points; the editor includes their
file locations and refreshes them from unsaved text. The single-error APIs return
the first diagnostic in the failed phase. Checked call contracts and inferred hover
return only when the entire source checks successfully. Try temporarily changing
the `reader` declaration in [traits.tellur](../../playground/traits.tellur) to
`exits auto`, alongside a second invalid trait declaration.


## Implementation preparation batches

Since 0.197, independent implementations with valid, unique trait/type identities
can report one member-inventory or method-profile error each, up to the shared
32-error budget. This includes missing/extra/duplicate associated types or controls,
generic/inferred method profiles and duplicate parameters or exit names. Unused
implementations are checked too.

<!-- tellur:reject E_TRAIT -->
```tellur
trait reader {
    control read(self input) exits done;
}
implement reader for u32 {}
implement reader for string {}
```

Both implementations must supply `read`. Repairing one leaves the other error;
associated-type normalization, conformance, generated methods and body checking wait
until every implementation has prepared. The failed phase exposes no partial witness
table, substitutions or checked hover. Imported errors retain original points, and
unsaved repairs update their locations before restoring hover on a fully valid program.

A read-only identity check admits this recovery only when every trait exists, each
target is an allowed concrete owned type, module ownership is coherent and each
trait/type pair is unique. Otherwise checking follows the previous sequential order:
a later invalid identity never preempts an earlier member/profile failure. These
coherence cases remain one-error barriers. Shared trait/schema errors and the existing
1024-implementation limit still run before this phase. Associated-type cycles,
conformance and async/fault/contingency permission checks remain separate later
checks. Trait acceptance, ownership and origin rules do not change.


## Associated-type normalization batches

Since 0.198, normalization collects independent associated-type failures after all
shared schemas, traits, implementation identities and local members have prepared.
The shared 32-error limit applies, including unused assignments. Roots keep the
existing canonical trait/type order, followed by each implementation's written
member order; this is not global source order. The first-error APIs preserve the
first failure and its original root location.

<!-- tellur:reject E_LIFETIME -->
```tellur
trait source {
    type item;
    type alias;
}
implement source for u32 {
    type item = ref string;
    type alias = item;
}
implement source for string {
    type item = ref u32;
    type alias = item;
}
```

Both `item` assignments violate the owned-associated-type rule. Their aliases depend
on those failures and add no repeated diagnostics. Dependency tracking follows actual
normalization, including lexical aliases, qualified selections and the types used as
selection arguments. A failed recursive path is retired before the next root; cycles
never become spurious failures in independent roots. Existing depth 64 and type-size
16,384 bounds remain in force. A long dependency chain can exhaust the depth budget
again from a later, previously unvisited root.

Only completed values are cached internally. If any root fails, no substitutions,
normalized witnesses, generated methods or checked hover reach later phases.
Associated bounds, visibility, method conformance and body checking still wait for
the whole normalization phase. Imported diagnostics retain original-file points;
partial unsaved repairs remove resolved failures and restore hover only after the
entire program checks. This changes recovery, not trait acceptance or ownership.


## Implementation permission batches

Since 0.199, independent implementation permission failures report together after
all identities, local members and associated types have prepared. Checking keeps
canonical trait/type order, then each implementation's written method order. It
reports at most one error per implementation and stops at the shared 32-error budget,
including unused implementations. Single-error APIs retain the first diagnostic.

<!-- tellur:reject E_ASYNC -->
```tellur
trait reader {
    control read(self input) exits done;
}
implement reader for u32 {
    async control read(self input) exits done {
        exit done;
    }
}
implement reader for string {
    async control read(self input) exits done {
        exit done;
    }
}
```

Both methods add async permission to a synchronous trait operation. Removing one
`async` leaves the other error. Within a method, checking still compares async
permission first, then fault permission, then contingency permission. Async must
match exactly. An implementation cannot add `faults` or `@contingent` permission
that its trait did not grant; a valid implementation inherits those permissions.
Generic witnesses retain the same rules.

The preflight is read-only. A failed phase publishes no substitutions, generated
controls, visibility entries or witnesses. Associated bounds, origin/signature
conformance and body checking remain later phases. Original imported points and
UTF-16 editor locations follow unsaved partial repairs, with checked hover withheld
until the whole source validates. Permission acceptance and execution are unchanged.


## Implementation conformance batches

Since 0.200, independent implementations can report conformance failures together,
including associated capability requirements, target/associated type checks,
parameter ownership, exit payloads, direct return origins and contained origin
mappings. Unused implementations are checked. The phase reports at most one error
per implementation and stops at the shared 32-error budget.

<!-- tellur:reject E_BOUND -->
```tellur
trait reader {
    type item: copy;
}
implement reader for u32 {
    type item = string;
}
implement reader for string {
    type item = string;
}
```

Both assignments violate `copy`. Fixing one leaves the other error. Checking keeps
the existing canonical implementation order, associated requirements before method
contracts, and expected methods in trait declaration order. Within a method, named
contracts resolve first; direct origins, contained origins and exact ownership/exit
shapes retain their existing priority. Local parameter/payload names and exit-label
order may still differ when the contracts otherwise match.

Shared identities, member profiles, normalization, permissions, generic/layout
preparation and manifest validation keep their earlier barriers. A failed conformance
phase publishes no checked trait-side signatures, and later argument-bound and body
checking waits. Generic definition checking keeps dependency-reachable abstract
witnesses and restores user-facing parameter names across the whole diagnostic batch.
Single-error APIs still return the first failure.

Imported points retain their original files and refresh from unsaved text. Checked
trait-call hover remains unavailable while any implementation fails; after repair,
it uses the trait's declared names and origins. Source acceptance, ownership and
origin semantics are unchanged.


## Constructor-call preparation batches

Since 0.91, inaccessible constructor calls in independent bodies report together.
A public struct may have a private constructor: external callers can name the type,
but cannot call `type(...)` unless its constructor is public. Raw construction still
follows field visibility; a public factory in the defining module can use its private
constructor. These visibility rules are unchanged.

This phase runs after import/name resolution, public shape checks and trait lowering,
before generic expansion, concrete signatures and body checking. It reports at most
one `E_PRIVATE` per prepared body and stops at the shared **32-error budget**. Controls,
unused generic controls, constructors, lowered trait methods and named boundary work
and contingency bodies are independent units. A lexical boundary remains inside its
enclosing body, so its work and handler do not get separate preparation budgets.

Rewriting is staged: if any body fails, no rewritten bodies are committed and all
later phases stay blocked. Successful siblings cannot supply partial contracts or
stale hover. Repairing one inaccessible call can expose the next in that same body;
repairing the phase can expose later signature, ownership or contingency errors.
The earlier parser, module/name and trait-lowering barriers remain in place.
Constructor-definition errors have the separate, earlier batch below since 0.96.

CLI JSON retains each caller's source point and defining file, and single-error
APIs return the first diagnostic. LSP uses the same batch, including related locations
for imported callers and unsaved dependency changes. Try
[constructor_access/main.tellur](../../playground/constructor_access/main.tellur):
remove `pub` from both constructors in
[messages.tellur](../../playground/constructor_access/messages.tellur). Both caller
errors appear together. Restore one modifier, then the other; checked hover returns
only after the complete program is valid.

## Constructor-definition preparation batches

Since 0.96, independent constructor definitions in one parsed source file report
preparation errors together. There is at most one error per constructor under the
shared **32-error budget**, including unused generic structs. This phase checks
result-origin argument counts, reserved `self` parameters, forbidden explicit exits
or local isolation, whole/partial `self` access and unknown initialization fields.

<!-- tellur:reject E_CONSTRUCTOR -->
```tellur
struct first {
    string text;
    constructor() { self.absent = "first"; }
}

struct second {
    string text;
    constructor() { exit done; }
}
```

Both constructors are invalid independently. Generated constructor controls are
staged and appended only after the complete file's preparation succeeds. A failed
file supplies no partially registered constructor set. Later type checking, missing
field initialization, ownership and fault admission still run in their existing
phases after preparation passes; they are not extra errors from this batch.

The boundary is **one file**, not the complete import graph. Parsing of that file
must succeed first. Its constructor preparation precedes symbol registration and
traversal of its imports. Loading stops at the first failing file, so another file's
errors may appear only after that file is repaired. Import cycles, invalid paths,
duplicate names and source-size limits retain their existing phase/order barriers.
Source-only checking uses the same batch for its single parsed unit.

Diagnostics keep the original constructor/body source points, including imported
files and unsaved dependency edits. Single-error APIs select the first diagnostic;
failed batches have no checked program, inferred hover or constructor call contract.
Try invalid `self` fields in both constructors of
[messages.tellur](../../playground/constructor_access/messages.tellur) while viewing
its [caller](../../playground/constructor_access/main.tellur).

## Statement recovery

Since 0.73, an error in a declaration, assignment, `drop`, `emit` or
single-completion call can be followed by diagnostics from independent subsequent
statements. For example, both bad initializers below are reported; the use of
`count` is suppressed until its declaration is repaired.

<!-- tellur:reject E_TYPE -->
```tellur
control main() exits done {
    u32 count = true;
    string title = 7;
    u32 dependent = count;
    exit done;
}
```

The checker restarts the body from clean ownership and inference state for each
additional diagnostic. In these diagnostic-only passes it omits failed statements
and dependent statements. Every value read, written or declared by an omitted
statement becomes uncertain, including its whole record when a field is involved.
Reference carriers and their source owners are conservatively linked; touching
one can suppress diagnostics involving other live references. This deliberately
prefers missing a secondary error to reporting one based on uncertain loan state.

Final coverage, result inference and constructor completion checks are not run on
a replay with omissions. Independent controls are still checked. Release 0.74
tracks uncertainty by selected history, as described below.

The existing **32-error budget** covers both statements and controls. Successful
programs use the ordinary checking path; diagnostic replays never publish contracts,
hover bindings or executable code. No syntax or valid-program semantics change.
Try the editable [diagnostics example](../../playground/diagnostics.tellur).

## Path and scope recovery

Since 0.74, uncertainty from a failed statement belongs to that statement's full
selected domain. A disjoint path retains its normal type, ownership and loan
checking. Flat labels, path aliases, nested groups, lexical scopes and statement
arm bodies can therefore contribute independent diagnostics within one control.
Direct arm exits and ordinary `exit`, `forward` and `again` statements can also
report independent errors on separate paths. Loop bodies use the same policy;
uncertain loop completion does not produce a further coverage diagnostic.

<!-- tellur:reject E_TYPE -->
```tellur
control inspect(bool flag) exits done {
    delta flag { true => left, false => right }
    [left]: u32 count = true; exit done;
    [right]: {
        string title = 7;
        exit done;
    }
}
```

Both initializer errors appear together. The failed declaration on `left` does
not suppress the independent error on `right`. The same rule applies to ownership
diagnostics: an uncertain reference on one path does not hide a real conflicting
borrow on a disjoint path. Within an uncertain path, dependent values and reference
carriers remain conservatively linked, as in straight-line recovery.

When an uncertain path reaches a new decision, capture, adapter, fault boundary,
loop entry or terminal disposition, that history is **stalled** for this diagnostic
pass. Later statements selecting it are omitted. A selector that joins a stalled
history with another history stalls the whole selection: the checker never reports
an error by silently narrowing the authored selector to its remaining paths.
Independent selectors can still be checked. A missing selector after an omission
stops its containing body, since the omitted decision may have introduced that name.

Inferred outcome adapters, decision-header errors, and scope/loop setup or
completion errors retain conservative first-error
limits. A whole-state fault-resource check stalls an uncertain history. Final
resource checks are suppressed for uncertain scope domains, while clean disjoint
paths retain their normal checks. No recovery result becomes executable or supplies
partial inferred hover. The same 32-error budget covers all controls and paths.
Try [path_diagnostics.tellur](../../playground/path_diagnostics.tellur).

## Named boundary body recovery

Since 0.83, independent declaration, assignment, `drop`, `emit` and call-statement
errors can be collected inside a named `boundary` or its attached `contingency`.
Disjoint paths and ordinary nested scopes use the same uncertainty rules above.
For example, both initializers below are diagnosed:

<!-- tellur:reject E_TYPE -->
```tellur
boundary request() {
    u32 count = true;
    string description = 7;
    terminate;
}
contingency(string reason) {
    terminate;
}
```

Work and contingency are checked as separate bodies with fresh replay state. A
failed contingency suppresses its dependent work body; a failed boundary suppresses
its supervisors. Repairing the handler can therefore expose errors in the work.
The 32-error budget remains shared, and no failed check publishes partial contracts,
inferred hover or executable code.

A potentially faulting call checks every retained and live must-complete owner,
including owners absent from its written arguments. A retention or fault-resource
failure on an uncertain history stalls that history instead of producing another
diagnostic. The same failures on a clean disjoint history are still reported and
stop that body's replay. Temporary call scopes are retired even when such a check
stalls a path. Administrative dispositions, new decisions, supervision, lexical fault
boundaries and inline adapters remain barriers after an omission. Lexical work and
handler recovery are described below.

Try introducing two independent bad local initializers in the work or contingency
of [contingency.tellur](../../playground/contingency.tellur), then repairing them one
at a time. The supervision hover returns only after the complete source checks.

## Lexical boundary work recovery

Since 0.84, independent leaf errors inside `fault boundary { ... }` work can also
be collected, including disjoint paths, ordinary scopes, loop bodies and nested
boundary work. The following work reports both bad initializers:

<!-- tellur:reject E_TYPE -->
```tellur
control main() exits done {
    fault boundary {
        u32 count = true;
        string description = 7;
    } contingency [] {
        terminate process 1;
    }
    exit done;
}
```

When any work history becomes uncertain, the diagnostic replay restores that
boundary's entry state and stalls its **whole selected domain**. It does not check
the attached handler or continue the affected outer history. Independent outer
paths remain checkable. An outer selector joining a stalled path with a clean path
stalls the complete selection; the parent entry selector keeps its original meaning.
Nested boundaries apply this rule at each enclosing boundary.

Capture/setup errors retain their first-error limits. Repairing work can expose
handler errors; independent handler errors are collected since 0.85. Fault-resource
checks, retained-owner checks and supervisor protection keep
their existing rules. No diagnostic replay runs a handler or emits a partial program.
This is editor/checker recovery, not recovery from a running program's source fault.

Try independent bad initializers in the work of
[recovery.tellur](../../playground/recovery.tellur). After all errors are repaired,
the example still handles the first request's fault and runs the next request.

## Lexical contingency handler recovery

Since 0.85, independent leaf errors in a lexical `contingency` can be collected
after its work checks cleanly. Handler paths, ordinary nested scopes and loops use
the same bounded dependency and uncertainty rules. Work errors still suppress the
attached handler until repaired.

<!-- tellur:reject E_TYPE -->
```tellur
control main() exits done {
    fault boundary {} contingency [] {
        u32 count = true;
        string description = 7;
        terminate process 1;
    }
    exit done;
}
```

Both initializers are diagnosed. An uncertain handler stalls the whole boundary
domain before its coverage and retirement checks; the checker restores normal-work
parent state and clears the temporary handler context. Disjoint outer histories
remain checkable, while joined selectors stall completely. Uncertainty from a nested
handler also prevents its enclosing work from claiming a valid continuation or handover.

Recovery never relaxes contingency call permissions, capture access, immutable
`@fault`, retained-owner obligations or resource completion. Clean-path violations
remain diagnostics. Fault-admission failures on uncertain histories stall those
histories. Administrative dispositions and new decisions remain barriers after an
omission; clean terminal errors still stop replay. The shared 32-error budget and
requirement for a fully checked program before hover/build are unchanged.

## Explicit exit-adapter recovery

Since 0.86, independent terminal arms of `exit delta` in an ordinary control can
report errors together when the control's entire result shape is explicit. Both
payload errors below are reported:

<!-- tellur:reject E_TYPE -->
```tellur
control choose(bool flag) exits ready { u32 value } {
    exit delta flag {
        true  => ready { true },
        false => ready { "wrong" }
    };
}
```

This reuses terminal replay on each selected arm, including nested adapters and
named result contracts. Failed arms and their dependent histories are omitted;
independent arms keep their original guards, ownership and borrow checks. Partially
moved resource arguments and returned references are reconstructed from clean state
on each replay. Real resource-completion errors on clean arms remain diagnostics.

Inferred exit adapters follow the rules below since 0.94. Captures retain their
existing recovery barriers. Explicit stored adapters have their own recovery rules.
Decision operands, patterns and coverage
must validate before their results can be checked; a nested header error stops that
adapter's replay. Entering an adapter from an already uncertain history still stalls
that history. Fault-context terminal recovery remains conservative. No missing arm,
partial inferred outcome or executable program is synthesized. The same 32-error
budget and complete-check requirement for hover apply.

Try changing payload expressions in the explicit `main` adapter of
[adapters.tellur](../../playground/adapters.tellur). Its inferred `successor` helper
also supports independent payload diagnostics since 0.94.

## Inferred exit-adapter recovery

Since 0.94, terminal arms of `exit delta` also recover under private `exits auto`
and selected payload inference (`ready...`). Independent errors can appear together
even when the arms produce the same exit label:

<!-- tellur:reject E_TYPE -->
```tellur
control choose(bool flag) exits ready... {
    exit delta flag {
        true  => ready { 1 < true },
        false => ready { 2 < false }
    };
}
```

Each inferred exit payload is typed independently, as it is for ordinary `exit`
statements. Successful occurrences must agree on payload arity and types; one arm
does not provide contextual types for another. For example, an unannotated empty
collection remains invalid even if a previous exit uses a typed collection. This
differs from [inferred stored adapters](#inferred-stored-adapter-recovery), whose
later same-label payloads can depend on earlier contextual inference. Diagnostic
recovery does not change either inference rule.

Replay omits failed terminals, blocks their selected histories, and checks independent
arms from clean state. Nested adapters, implicit forwarding, disjoint outer labels,
scopes, loops and unused generic bodies follow the same 32-error budget. Returned
borrows and moves retain ordinary checks; resource and lifetime failures on clean
arms remain real diagnostics. Selected inferred labels still require nonempty
payloads, while bare declared labels remain nullary.

The control's partial contract is never published. Dependent callers and their hover
wait for a complete successful check. Decision headers, uncertain entry histories,
captures and fault-context terminals retain their barriers. This does not introduce
contextual exit inference, fault recovery or any new executable syntax. Try invalid
payload expressions in the inferred `successor` helper in
[adapters.tellur](../../playground/adapters.tellur).

## Type context: E_TYPE and E_INFER

An annotation constrains the initializer; it does not request a conversion.

<!-- tellur:reject E_TYPE -->
```tellur
control main() exits done {
    string text = 42;
    exit done;
}
```

Use the explicit formatting operation:

<!-- tellur:check -->
```tellur
control main() exits done {
    string text = u32_text(42);
    emit(text);
    exit done;
}
```

An empty option does not supply enough information to infer its element type:

<!-- tellur:reject E_INFER -->
```tellur
control main() exits done {
    auto missing = none;
    exit done;
}
```

<!-- tellur:check -->
```tellur
control main() exits done {
    option<string> missing = none;
    exit done;
}
```

`auto` requests inference at a supported binding site; it is not a dynamically typed
value. Empty collection literals similarly need a type from their context.

## Exhaustiveness and availability: E_ARMS, E_UNCOVERED and E_UNAVAILABLE

Even a literal boolean decision requires dispositions for both structural
alternatives. The omitted flat arm introduces `[false]`, which is unfinished here:

<!-- tellur:reject E_UNCOVERED -->
```tellur
control main() exits done {
    delta true { true => yes }
    [yes]: exit done;
}
```

<!-- tellur:check -->
```tellur
control main() exits done {
    delta true { true => yes, false => no }
    [yes | no]: exit done;
}
```

A binding produced on one path is not available on every entry history:

<!-- tellur:reject E_UNAVAILABLE -->
```tellur
control choose(bool flag) exits ready { u32 value } {
    delta flag { true => yes, false => no }
    [yes]: u32 value = 1;
    [entry]: exit ready { value };
}
```

Give both paths a value, or return different outcomes:

<!-- tellur:check -->
```tellur
control choose(bool flag) exits ready { u32 value } {
    delta flag { true => yes, false => no }
    [yes]: u32 value = 1;
    [no]: u32 value = 0;
    [entry]: exit ready { value };
}
```

Disjoint definitions must agree on type and mutability. This form is not general
lexical shadowing and does not permit merging reference-bearing bindings arbitrarily.

## Ownership and loans: E_MOVED and E_LOAN

Emitting an owned string consumes it:

<!-- tellur:reject E_MOVED -->
```tellur
control main() exits done {
    string text = "hello";
    emit(text);
    emit(text);
    exit done;
}
```

Borrow for observation when retaining ownership is intended:

<!-- tellur:check -->
```tellur
control main() exits done {
    string text = "hello";
    emit(borrow text);
    emit(text);
    exit done;
}
```

The owner's mutability does not override a live shared reference:

<!-- tellur:reject E_LOAN -->
```tellur
control main() exits done {
    mutable string owner = "before";
    ref string view = borrow owner;
    owner = "after";
    emit(view);
    exit done;
}
```

Finish the reference's use before mutating its owner:

<!-- tellur:check -->
```tellur
control main() exits done {
    mutable string owner = "before";
    ref string view = borrow owner;
    emit(view);
    owner = "after";
    exit done;
}
```

A copy, returned child, or stored field may keep the dependency live. Moving or
dropping just one reference does not erase another retainer's dependency.

## Escaping storage: E_LIFETIME

An invocation's local owner cannot back a returned reference:

<!-- tellur:reject E_LIFETIME -->
```tellur
control invalid() exits found { ref string value } {
    string owner = "temporary";
    exit found { borrow owner };
}
```

Return ownership, or borrow storage supplied by the caller:

<!-- tellur:check -->
```tellur
control view(ref string input) exits found { ref string value from input } {
    exit found { input };
}
```

`from` documents and checks a dependency; it cannot make a local value live longer.

## Unhandled completion: E_OUTCOME

Capturing a call does not count as handling its result:

<!-- tellur:reject E_OUTCOME -->
```tellur
control produce() exits ready { exit ready; }
control main() exits done {
    auto saved = capture produce();
    exit done;
}
```

<!-- tellur:check -->
```tellur
control produce() exits ready { exit ready; }
control main() exits done {
    auto saved = capture produce();
    delta saved { ready => handled }
    [handled]: exit done;
}
```

A nullary outcome still requires handling or transfer. Resource payloads retain their
separate completion obligations after unpacking; handling an outcome does not close
a file carried inside it. `E_RESOURCE` diagnoses abandoned must-complete resources.

## Incomplete variant schemas: E_ORIGIN

A public origin schema must map every reference leaf, including fields in different
alternatives. Merely declaring a group does not attach fields to it:

<!-- tellur:reject E_ORIGIN -->
```tellur
variant view['source] {
    found { ref string text },
    empty
}
```

<!-- tellur:check -->
```tellur
variant view['source] {
    found { ref string text <- 'source },
    empty
}
```

An ungrouped variant may hold references and be borrowed locally. Passing or returning it as
a whole requires a complete public schema. Missing body evidence for a returned
alternative also requires an explicit result mapping, such as `view[input] value`.

## Optional result origin inference: E_ORIGIN

A borrowed option that is always absent provides no reference leaf from which to
infer its public owner dependency. Give that result an explicit mapping.

<!-- tellur:reject E_ORIGIN -->
```tellur
control absent_text(ref string input) exits ready... {
    option<ref string> value = none {};
    exit ready { value };
}
```

<!-- tellur:check -->
```tellur
control absent_text(ref string input)
    exits ready { option<ref string>[input] value }
{
    exit ready { none {} };
}
```

Named outcome groups obey the same completeness and arity checks as variant groups.
Binding a group to the wrong owner is an `E_LIFETIME` error; `input['source]` on an
optional direct reference selects its external source, while a misspelled group
produces `E_ORIGIN`. Borrowed sum inputs are supported. A shared sum borrow cannot
produce an exclusive payload (`E_MUTABILITY`), and moving a carrier before the last
use of its exclusive child is `E_LOAN`. Returned whole-carrier references preserve
that same storage protection.

## Borrowed sum diagnostics

An exclusive payload reborrow retains the sum that authorizes it. Replacing or
dropping that carrier before the child's last use is rejected:

<!-- tellur:reject E_LOAN -->
```tellur
control main() exits done {
    mutable option<string> saved = some { "hello" };
    delta borrow mutable saved { some { auto text } => some, none => none }
    [some]: drop(saved); deref text = "too late";
    [entry]: exit done;
}
```

A borrowed sum input can yield selected references or a reference to the whole
carrier. The latter retains both storage and contained-source dependencies:

<!-- tellur:check -->
```tellur
control identity(ref option<ref string> input)
    exits ready { ref option<ref string> value from input }
{
    exit ready { input };
}
```

Explicitly dropping a Copy carrier is still a release, so a live reference to its
storage prevents that drop:

<!-- tellur:reject E_LOAN -->
```tellur
struct view['source] { ref string text <- 'source; }
control lend(ref view input) exits ready... { exit ready { input }; }
control main() exits done {
    string owner = "hello";
    view saved = view { borrow owner };
    delta lend(borrow saved) { ready { auto result } => ready }
    [ready]: drop(saved); emit(result.text); exit done;
}
```

A captured carrier reference also keeps the carrier borrowed before unpacking:

<!-- tellur:reject E_LOAN -->
```tellur
struct view['source] { ref string text <- 'source; }
control lend(ref view input) exits ready... { exit ready { input }; }
control main() exits done {
    string owner = "hello";
    view saved = view { borrow owner };
    auto pending = capture lend(borrow saved);
    drop(saved);
    exit delta pending { ready { auto result } => done {} };
}
```

Local containers of carrier references are supported. Public input/result contracts
require a complete mapping of storage and contained groups. Neither permits
reference rebinding or references to references.

<!-- tellur:reject E_ORIGIN -->
```tellur
struct view['text] { ref string text <- 'text; }
// Missing the inner text mapping: storage alone cannot describe this field.
struct job['storage] { ref view target <- 'storage; }
control main() exits done { exit done; }
```


## Other diagnostic families

| Family | What to inspect |
| --- | --- |
| `E_LEX`, `E_LITERAL`, `E_PARSE` | Tokens, supported escapes/literals, delimiters and required semicolons |
| `E_CONSTRUCTOR` | Unsupported constructor operation: partial `self` escape, explicit exit or local isolation; see [constructors](construction.md). Missing and repeated field initialization use `E_UNAVAILABLE` and `E_REDEFINE`; missing result-origin mappings use `E_ORIGIN` |
| `E_NAME`, `E_SHADOW`, `E_REDEFINE` | Name resolution, duplicate names, disjoint definitions and forbidden outer-value shadowing |
| `E_UNCOVERED`, `E_TERMINATED`, `E_EMPTY` | Missing completion, overlapping work after exit, or an empty structural selection |
| `E_FAULT_CONTRACT` | A public or trait control needs explicit source-fault permission |
| `E_FAULT_RESOURCE` | A still-owned resource needs an explicit fault disposition before this faulting path or call |
| `E_FAULT` | Observed source-fault termination at runtime, not a normal outcome |
| `E_MUTABILITY` | Immutable binding assignment or insufficient reference authority |
| `E_ORIGIN` | Origin schema, result mapping or declared input dependency |
| `E_TRAIT`, `E_COHERENCE`, `E_VISIBILITY` | Contract conformance, implementation identity/ownership, module privacy |
| `E_LOOP`, `E_LOOP_CAPTURE` | Tail-loop shape, state transfers and outer values captured by a repeating body |
| `E_GENERIC`, `E_BOUND` | Type arguments, bounds, abstract definition checking and specialization limits |
| `E_UNSUPPORTED` | A recognized construct outside the current implementation profile |

This is a navigation aid, not an exhaustive stable diagnostic API. Similar invalid
programs can fail at different checking stages. Structural witnesses show *possible
modeled histories*, not proved concrete inputs.

## Explicit stored-adapter recovery

Since 0.87, a stored adapter with an explicit outcome type can report independent
payload errors together. The containing control may infer its own exits: the
adapter's declared target supplies the complete payload shape.

<!-- tellur:reject E_TYPE -->
```tellur
outcome answer { ready { u32 value } }

control choose(bool flag) exits answer {
    answer saved = delta flag {
        true  => ready { true },
        false => ready { "wrong" }
    };
    exit saved;
}
```

Both arms are reported. An erroneous arm makes the entire binding uncertain:
recovery restores its parent scope and history, discards the partial sink, and
stalls the binding's full selected domain. It does not define `saved`, infer a
replacement type, or continue through its uses. Independent outer histories keep
their original selectors and checking. Nested adapters share the same error budget.

Clean arms still check their own resource completion and reject references to
arm-local storage. Such retirement errors remain barriers. Inferred stored adapters
follow the rules below; captures, decision headers and adapters in fault contexts
retain conservative first-error behavior. A successful full check is
still required for hover and execution. Try the explicit `message_result message`
adapter in [stored_adapters.tellur](../../playground/stored_adapters.tellur).

## Inferred stored-adapter recovery

Since 0.93, `auto saved = delta ...` can report independent payload errors for
distinct result labels. No target type is invented to make recovery possible.

<!-- tellur:reject E_TYPE -->
```tellur
control choose(bool flag) exits auto {
    auto saved = delta flag {
        true  => left { 1 < true },
        false => right { 2 < false }
    };
    exit saved;
}
```

Both comparisons are invalid independently, so both are reported. After omitting a
failed arm during diagnostic replay, later constructions of that same result label
are suppressed. Their payload shape or contextual types may have depended on the
omitted occurrence—for example, an empty collection may need a preceding arm's
collection type. The rule also covers implicit `forward` results. Label uncertainty
belongs to that one stored binding, including its nested adapters; a different
binding on a disjoint outer path has its own inference state.

This is deliberately conservative: a later arm with that label may contain another
real error, which appears after repairing the earlier one. Successfully checked
occurrences still establish ordinary same-label payload consistency before any
failure. Distinct labels retain normal ownership and borrow checking, including
resource completion and rejection of references to arm-local storage.

Any uncertain result makes the whole stored binding unavailable. Recovery restores
the parent scope/history, discards partial writes, and stalls the binding's complete
selected domain before defining a value or inferred outcome type. Uses of that value,
continuations and joins touching the stalled domain stay suppressed; disjoint outer
histories remain checkable with their original selectors. The 32-error budget covers
all arms and controls, including unused generic definitions.

Inferred exit adapters have the separate recovery rules above since 0.94. Decision
headers, already uncertain entry histories and fault-context adapters retain their
existing barriers. Repairing an arm does not
make a partially checked program executable or available to hover. Try invalid
expressions in the `ready` and `empty` payloads of the first `auto saved` adapter in
[stored_adapters.tellur](../../playground/stored_adapters.tellur); restoring the complete
source restores its inferred-type and call contracts.

## Implementation budgets and operational limits

These bounds protect the reference compiler from unbounded work. They are
implementation limits, not mathematical properties of Tellur or permanent promises.
Exhaustion must produce an error rather than a partially checked successful program.

| Area | Current limit / behavior |
| --- | --- |
| Import graph | 256 files, 8 MiB total source |
| Generic data layouts | 1,024 specializations; nesting depth 64; type spelling 16,384 bytes |
| Generic controls | 1,024 instances per checking pass, including abstract checks; depth 64; type spelling 16,384 bytes |
| Trait implementations | 1,024 implementations |
| Associated type normalization | Depth 64; type spelling 16,384 bytes |
| Origin schemas | 256 groups per declaration; 4,096 leaves; bounded nesting at 64 active schema expansions |
| Inferred-type hover | Depth 64 and 512 rendering nodes; ellipsis truncates display, not type checking |
| Collections | Length represented by u32; allocation exhaustion is a host failure |
| Native execution | No default instruction fuel limit; optional `--fuel N` bounds execution |

Map storage is currently sorted and flat: lookup/insertion/removal are linear.
Front vector removal is linear; repeated consuming drains can be quadratic. These
are current implementation costs, not the intended permanent complexity contract.

Scenario comparison checks supplied cases only. Protocol comparison forgets data;
its finite trace-language result is not full program equivalence. Those tools have
additional analysis budgets described in [protocol analysis](../protocol-analysis.md)
and [semantic foundations](../semantic-foundation.md). See the main reference for
[expansion certification](../language-reference.md#validate-expansion), which proves
a permitted rewrite correspondence rather than arbitrary equivalence or termination.

## Process status diagnostics (`E_EXIT_CODE`)

Invalid source annotations report their source position. Native generation rejects
missing or unknown entry mappings and borrowed entry parameters. Library checking
does not require complete mappings. See [process entry contracts](process-entry.md).

## Lexical recovery authority

`E_CONTINGENCY_CAPTURE` rejects use of ordinary outer values from contingency.
`E_BOUNDARY_BORROW` rejects outer loans/reference-bearing captures, and
`E_SUPERVISOR_STATE` keeps protected supervisor values outside isolated work.
Prepare owned request inputs before boundary entry. `E_BOUNDARY_RETAIN` means a
captured owner is no longer wholly available at a potentially faulting operation.

`E_RECOVER` requires the named enclosing supervised loop. `E_CONTINGENCY_EXIT`
rejects ordinary return/restart from a handler; a fault cannot become an outcome.

<!-- tellur:reject E_CONTINGENCY_EXIT -->
```tellur
control main() exits done {
    fault boundary { fault "broken"; } contingency [] { exit done; }
}
```

`E_PROCESS_EXIT` checks process-termination placement and literal range.
`E_ENTRY_ONLY` rejects calls to controls with that authority. Both distinguish
process termination from ordinary completion.

<!-- tellur:reject E_ENTRY_ONLY -->
```tellur
control root() exits done {
    fault boundary { fault "startup"; } contingency [] { terminate process 2; }
}
control caller() exits done {
    root();
    exit done;
}
```

## Async invocation permissions

`E_ASYNC` reports a missing or mismatched explicit async invocation, a `run` or
`await` used as a value expression, mismatched trait/implementation permissions,
or async invocation in contingency code. Use `run` from a synchronous control and
`await` from an async control; both require an async callee. Errors on calls select
the complete invocation. These declaration-phase checks report the first async
admission error per control; see [direct async execution](async-execution.md).
