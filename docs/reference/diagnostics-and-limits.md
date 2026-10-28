# Diagnostics and limits

[Language reference](../language-reference.md) · [Feature status](support-status.md)

The compiler reports a diagnostic code, source location and explanation. It usually
stops at the first error for a document; fixing it may expose another. The precise
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

<!-- tellur:reject E_PUBLIC_CONTRACT -->
```tellur
pub control view(ref string input) exits ready { ref string value } {
    exit ready { input };
}
```

Add `from input` after the result binding. Reference-bearing aggregate results
need explicit bracket mappings as well; references to such aggregates need both
storage and contained-origin contracts. An unused generic public declaration must
satisfy the same rule. See [explicit public contracts](../language-reference.md#explicit-public-contracts).

For incompatible `forward` calls and missing delta arms, CLI notes and VS Code
related information can show producing exits, calls, captures and adapters. A
private helper can absorb several input alternatives into one result: the result's
notes retain those structural contributors. Notes also accompany incompatible
inline adapter results.

Since 0.46 this includes moved completed values, delayed `exit saved`, nested
adapters, local record fields and nested sum payloads, direct borrowed decisions,
and merged path labels. Different fields and alternatives stay separate. Disjoint
writes are filtered by the selected path; overwriting a value removes its old
lineage on that path. This analysis does **not** narrow a value's type: every
contract alternative still needs an arm, even if its only known construction was
overwritten. A declared but unproduced control outcome points to its declaration.

<!-- tellur:reject E_ARMS -->
```tellur
control lookup() exits auto {
    exit missing;
}
control main() exits done {
    auto saved = capture lookup();
    auto adapted = delta saved { missing => unavailable {} };
    delta adapted {}
    exit done;
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
checks still run normally. Full expression ranges and independent-error recovery
remain future work.

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
| `diagnostics` | Empty on success; currently one diagnostic on failure |
| `summary` | Counts `controls` and `generic_definitions` on success; null on failure |

Each diagnostic contains `code`, `message`, `path`, `position`, `notes` and
`witness`. A position is `{ "line": 2, "column": 7 }`; it is a source **point**,
not an expression range. A missing position is null, as with a manifest error
that has no checked source location. A missing path is null, as with command
usage errors. Notes have their own `path`, `position` and `message`; imported
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
coordinates. These formats do not yet provide full expression ranges or report
multiple independent errors. JSON checking includes usage, configuration,
manifest and file-read failures; it does not build or execute Rust adapters.

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

## Exhaustiveness and availability: E_ARMS and E_UNAVAILABLE

Even a literal boolean decision requires both structural alternatives:

<!-- tellur:reject E_ARMS -->
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
| `E_NAME`, `E_SHADOW`, `E_REDEFINE` | Name resolution, duplicate names, disjoint definitions and forbidden outer-value shadowing |
| `E_UNCOVERED`, `E_TERMINATED`, `E_EMPTY` | Missing completion, overlapping work after exit, or an empty structural selection |
| `E_MUTABILITY` | Immutable binding assignment or insufficient reference authority |
| `E_ORIGIN` | Origin schema, result mapping or declared input dependency |
| `E_TRAIT`, `E_COHERENCE`, `E_VISIBILITY` | Contract conformance, implementation identity/ownership, module privacy |
| `E_LOOP`, `E_LOOP_CAPTURE` | Tail-loop shape, state transfers and outer values captured by a repeating body |
| `E_GENERIC`, `E_BOUND` | Type arguments, bounds, abstract definition checking and specialization limits |
| `E_UNSUPPORTED` | A recognized construct outside the current implementation profile |

This is a navigation aid, not an exhaustive stable diagnostic API. Similar invalid
programs can fail at different checking stages. Structural witnesses show *possible
modeled histories*, not proved concrete inputs.

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
