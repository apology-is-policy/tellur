# Public contract snapshots and diffs

[Language reference](../language-reference.md) · [Feature status](support-status.md)

Compiler 0.57 can export a checked description of a source module's public
declarations and those of its explicitly imported modules. A comparison reports
added, removed and changed contract entries. It does not decide semantic-version
compatibility or prove that two implementations behave alike.

## Terminal workflow

From the compiler checkout, compare the maintained settings library with its
committed baseline:

```sh
./bin/tellurc contract-snapshot applications/settings/settings.tellur > /tmp/settings.contract.json
./bin/tellurc contract-diff applications/settings/settings.contract.json /tmp/settings.contract.json --check
```

`contract-snapshot SOURCE [MANIFEST]` performs ordinary checking of the complete
import graph, including unused generic definitions. It writes a JSON snapshot to
stdout only after checking succeeds. Manifest discovery follows `check`; Rust
adapters are checked through their declarations without building or executing host
code. Syntax, type, ownership, resource or public-contract errors prevent export.

`contract-diff BEFORE_JSON AFTER_JSON [--check]` reads two snapshots and prints a
JSON report. With `--check`, differences return status 1; identical contracts return
0. Without `--check`, a successfully generated report returns 0 even when contracts
differ. Invalid input or unsupported snapshot versions return a nonzero status and
an explanation on stderr. The comparison needs neither source files nor a manifest.

Generate a candidate snapshot, review its diff, then deliberately update the
baseline when the change is intended. Neither command overwrites a baseline on its
own. Shell redirection still has its usual behavior: redirecting directly over an
existing baseline replaces that file, even if the command subsequently fails.

## Editor workflow

With a Tellur source file active, run **Tellur: Show Public Contract Snapshot**.
The language server checks the current unsaved source and unsaved imported buffers,
then opens the result in a new JSON document. Use Save As to retain a baseline.
The view is a snapshot at the time of the request; rerun the command to refresh it.
It does not save or edit the Tellur source.

The source needs a file identity and a valid manifest. Save untitled source first.
If checking fails, the command reports the error and opens no partial or cached
snapshot. The extension must be running in a trusted workspace.

## What the snapshot includes

The snapshot has `format: "tellur.public-contracts"` and `version: 1`. Its `modules`
map groups declarations by source module:

- `@entry` is the requested source file, independent of its filename or checkout path.
- `file:relative/path.tellur` identifies an imported file relative to the entry's
  directory. Paths outside that directory retain `..` components.
- Private-only imported modules are omitted. An empty `@entry` remains present.

This describes the public surfaces of the loaded graph. It does not claim that
importing the entry module re-exports every imported declaration, and it does not
scan unrelated project files. Moving the entire tree or changing local import
aliases leaves these identities unchanged; moving a dependency relative to the
entry can change its identity. Source paths must be UTF-8.

| Declaration | Captured contract |
| --- | --- |
| Public control | Ownership profile, ordered parameters, type parameters and requirements; explicit inline outcomes or named completion contract; direct and contained result origins |
| Public struct | Public fields, origin groups, type parameters, external constructibility, copy/clone/drop permissions |
| Public variant | Named alternatives, ordered payloads, origin groups, type parameters and permissions |
| Public named outcome | Alternatives, payloads, origin groups, type parameters, `@exit` statuses and mandatory handling |
| Public trait | Associated-type requirements and operation signatures |
| Visible implementation | Public trait, externally visible target and associated-type bindings; operation contracts come from the checked trait |

Only manifest types referenced by these surfaces are included in `manifest_types`,
with their recursively referenced schemas. Their references use `@manifest::name`.
Control signatures distinguish the current `control` profile from `legacy_region`,
whose value-transfer rules differ. Source type references use `@entry::name` or `file:path.tellur::name`. These are
snapshot identities, not new Tellur syntax. Builtin type spellings are normalized.

Private declarations, function bodies, implementation-local parameter names, source
positions, formatting and comments are excluded. A private struct field is not
exported, but its consequences for public permissions are included. For example,
replacing a private `string` with a must-complete `handle` removes clone and drop
permission from the containing public struct.

Each `capabilities` entry describes `copy`, `clone` or `drop`:

- `null`: the type never has that permission.
- `[]`: the permission is unconditional.
- `["t", "u"]`: both named type arguments must have that same permission.

For a generic struct containing a private `vector<t>`, copy is `null`, while clone
and drop each require `["t"]`. These rules describe the compiler's current sealed
capabilities; they are not user-defined trait implementations. Capability expansion
is memoized and bounded to 128 nested types and 20,000 type visits per capability.
Exceeding a bound refuses export rather than inventing a permission.

## Reading a diff

A report has `format: "tellur.public-contract-diff"`, `version: 1`, an `equal`
Boolean and a `changes` array. Each change carries `kind` (`added`, `removed` or
`changed`), a JSON-pointer `path`, and `before`/`after` values. Pointer segments
escape `~` as `~0` and `/` as `~1`.

Declarations and alternatives are compared by name, so reordering them adds no
noise. Ordered lists are compared as whole lists: parameter order, payload order,
type/origin parameter order and positional public struct construction remain
visible. Opaque structs list their public fields by name because callers cannot
construct their full representation. Public parameter and payload names are
retained; renaming them can produce a diff even when existing positional calls work.

An added outcome deserves review because callers may match exhaustively. A changed
borrow origin can change what a caller must keep alive. Neither change receives an
automatic blanket compatibility verdict in this checkpoint.

## Boundaries

Snapshot equality is equality of the recorded public contract. It is not behavioral
equivalence, binary ABI stability, layout compatibility or an expansion certificate.
Primitive implementations, launcher overrides, scheduling, performance and internal
failure propagation are outside this format. A hand-edited snapshot is compared as
provided; reading it does not recheck the original program. Unknown schema fields,
duplicate object keys and unsupported versions are rejected.

In particular, a new private failure mapped into an existing public `failed` outcome
can leave the snapshot unchanged. [Outcome absorption snapshots](outcome-absorption.md)
now compare bounded contributor evidence separately from the public contract. The
settings contract regression also changes an internal input-size limit while
demonstrating an unchanged contract; this is intentional. Contributor comparison
also does not compare arbitrary scalar values or predicates.

Compiler 0.63 snapshots public and trait `contingent` permissions. A public
boundary has a `retains` array (present even when empty), distinguishing lifecycle
supervision from an ordinary control's outcome contract. Its `faults` field means
permission for contingency escape. Internal contingency bodies are not exported.
Changing these permissions or retained-parameter contracts appears in a structural
diff; the report still makes no automatic compatibility verdict.
