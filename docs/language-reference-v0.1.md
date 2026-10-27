# Path: small language reference

Language v0.1, compiler 0.5.0. “Path” and `.path` are provisional names.

## Start in VS Code

Open `path-prototype.code-workspace`, then `playground/hello.path`.
Trust this workspace when prompted to enable the language server and build commands.
Use the editor's **▶** button or **Path: Build and Run** (`⌘⌥R` on macOS).
Output appears in **Path Build**. **Path: Build Native Program** (`⌘⌥B`) builds
without running; **⇧⌘B** also builds the active file through the workspace task.
The commands save the active file before compiling. Diagnostics also work while typing.

Try `playground/count.path`, `port.path`, and `read_file.path`. They each have a
zero-argument `main` and use real host operations. Edit their inputs directly.
Start new programs beside them to reuse `playground/manifest.json`.
Each source file is a compilation unit: put helper regions in that same file.

For another entry or command-line inputs, set `pathLanguage.entryRegion` and
`pathLanguage.programArguments` in workspace settings. Arguments are an array of
strings, without shell interpretation. For example, `parse_port` and `["443"]`
run that region in the port example. Return the entry setting to `main` afterward.

The supplied Apple Silicon package includes pathc and path-lsp. Native builds use
the installed Rust/Cargo toolchain and pinned Serde dependencies; the current
machine's dependency cache is prepared. Executables run without Rust installed.

## Regions and selected work

```path
region main() exits Done {
    [Entry]:
        emit("Hello!");
        exit Done;
}
```

A region declares typed parameters and every possible exit. An exit ends only that
region invocation. `Entry` selects every history entering the region. Statements
run in source order whenever their selector holds. Semicolons are required;
indentation is for readers. `//` starts a comment. Identifiers are ASCII and case
sensitive; string contents are UTF-8.

## Decisions and paths

```path
region parse(input: String) exits Ready(value: U32), Invalid {
    [Entry]:
        delta parse_u32(input) -> [Number(n) => Parsed, Malformed => Bad, Overflow => Large];
    [Parsed]: exit Ready(n);
    [Bad | Large]: exit Invalid;
}
```

`delta` (also written `Δ`) invokes a primitive, calls a region, or decides a Bool
or variant. Every declared outcome must have exactly one arm. There are no wildcard
arms or implicit error propagation. Payloads are available only on their arm's paths.
The names after `=>` are fresh path atoms. A decision can be named:
`read_result = delta read(handle) -> [...]` for inspection/provenance.

Selectors use `A & B` (both) and `A | B` (either). Intersection binds more tightly.
A union executes its statement once even when both constituents hold. Atoms remain
true on descendants; a prior operation's atom is false where that operation was
skipped. Referencing a future or out-of-scope atom is an error.

```path
path AnyFailure = ReadBad | CloseBad;
[Open] over &: {
    [ReadBad]: emit("Read failed");
    [CloseBad]: emit("Close failed");
}
```

`over &` intersects the outer selector into each inner selector. It introduces a
lexical scope: inner values, atoms and aliases cannot be used outside the block.
Ordinary `[selector]:` groups do not introduce a scope. No executable path negation
or subtraction is available.

Every live history must exit or explicitly restart a loop. Selected work after an
overlapping exit is an error. Uncovered/conflicting histories report a **structural
witness: may reach**, not proof that concrete inputs can cause that combination.
Separate Boolean decisions are independent in this model; reuse established atoms.

## Values and expressions

| Type | Examples |
| --- | --- |
| Bool | `true`, `false` |
| U32 | `0`, `42u32`, up to `4294967295` |
| String | `"text"`, escapes `\n`, `\r`, `\t`, `\\`, `\"` |
| Bytes | `bytes"00ff7a"` (pairs of hex digits) |
| Unit | `unit` |
| Record / variant | Declared in manifest.json |
| Option<T> | `Some(value)`, contextually typed `None` |

Bind with `name := expression;`. Bindings are immutable; the same name can have
separate definitions on disjoint paths, with the same type. Inner shadowing is
forbidden. A use must have one prior definition on every selected path.

Pure expressions support constructors, record fields, `!`, comparisons,
`&&`, and `||`. The logical operators short-circuit. Precedence is application,
projection, `!`, comparison, `&&`, `||`. Comparisons cannot chain. Ordering is for
U32; equality is for Bool, U32, String and Unit. No implicit conversions exist.
Arithmetic is performed through checked primitives rather than `+`, `*`, etc.
`emit` accepts String and is a logical observation; the native harness prints it.

Records are positional: `HandlePair(left, right, "label")`; fields use `pair.left`.
Variant constructors can be qualified, e.g. `FileState.Present(handle)`.
Qualified patterns such as `FileState.Present(h)` work in a decision too.

`Some(handle)` infers `Option<Handle>`. `None` needs an expected type, for example
`exit Found(None)` when Found declares an Option payload. `x := None` is ambiguous;
there are no local type annotations yet. `Option.Some` and `Option.None` disambiguate
builtin constructors from manifest constructors with the same short names.

## Ownership

An opaque type marked `linear: true` must be consumed or transferred exactly once
on each selected exit/restart. There is no implicit fallible cleanup. Constructor,
region argument and exit transfers move linear values. A primitive may borrow a
named value for that synchronous call or consume it, as declared in the manifest.

Records inherit ownership from their fields. After `left := pair.left`, you may
use `pair.right` and non-linear sibling metadata; you cannot use `pair` as a whole
or reuse `pair.left`. Borrowing a parent conflicts with consuming a child in the
same invocation. Different fields can be borrowed/consumed independently.

```path
[Available]: delta file -> [None => Absent, Some(handle) => Present];
[Present]: delta close(handle) -> [Closed => Closed, Failed(error) => CloseFailed];
```

Deciding an owning variant consumes it and transfers the active payload to its
arm. Here `handle` exists only on Present, and `file` cannot be reused on Available.
An empty arm has no resource payload to dispose of. Even a known empty value of
an owning variant must be decided or transferred under the current conservative
rules. Bind owning temporaries before borrowing or projecting them.

These are small linear-resource rules, not full Rust lifetimes or Copy/Clone/Drop.
Non-linear values remain reusable. See `examples/partial_moves.path` and
`examples/optional_handle.path` for executable ownership examples.

## Loops

```path
loop Count(number: U32 = 1) {
    [Entry]: delta (number <= 5) -> [true => More, false => Finished];
    [More]: delta checked_add(number, 1) -> [Sum(next) => Next, Overflow => TooLarge];
    [Next]: again Count(next);
    [Finished]: exit Done;
    [TooLarge]: exit ArithmeticFault;
}
```

A loop is the last item in its region or enclosing `over &` block. Parameters have
types and initial expressions. `again` supplies all next values simultaneously.
Iteration-local values/atoms are reset; outer non-Entry facts stay available.
Every path must exit the region or restart. Transfer outer resources through loop
parameters. Nested source loops and recursive region calls are excluded. Loops may
run forever; no termination proof is claimed.

## Live primitive library

`playground/manifest.json` contains these exact contracts. Payload names are omitted
here; the types and outcomes are significant.

| Operation | Outcomes |
| --- | --- |
| `parse_u32(String)` | `Number(U32)`, `Malformed`, `Overflow` |
| `checked_add(U32, U32)` | `Sum(U32)`, `Overflow` |
| `checked_sub(U32, U32)` | `Difference(U32)`, `Underflow` |
| `checked_mul(U32, U32)` | `Product(U32)`, `Overflow` |
| `checked_div(U32, U32)` | `Quotient(U32)`, `ZeroDivisor` |
| `u32_text(U32)` | `Text(String)` |
| `concat(String, String)` | `Text(String)` |
| `decode_utf8(Bytes)` | `Text(String)`, `Invalid` |
| `open(String)` | `Opened(Handle)`, `NotFound`, `Denied` |
| `read(borrow Handle)` | `Data(Bytes)`, `Failed(ReadError)` |
| `close(consume Handle)` | `Closed`, `Failed(CloseError)` |

Parsing requires one or more ASCII decimal digits (no sign or whitespace).
Files open read-only; read consumes the remaining stream contents; close consumes
the handle on success or failure. Relative paths use the workspace directory when
run through VS Code. There are no live network/time adapters yet: the retry example
uses a scenario script. A manifest declaration alone does not implement an operation.

Define your own data types in the manifest, for example:

```json
{"name":"Point","kind":"record","fields":[
  {"name":"x","type":"U32"},{"name":"y","type":"U32"}
]}
```

Add this to `types`, then use `Point(1, 2)` and `point.x` in source. For a variant use
`{"name":"Choice","kind":"variant","constructors":[...]}` with named constructors
and typed `fields`. Recursive types are excluded. New behavior can be written as
regions in the same source file. Custom foreign operations can be tested with
scripts; new live adapters require runtime implementation and a matching contract.

## Command line and inspection

From the compiler directory:

```sh
./bin/pathc check playground/hello.path
./bin/pathc exec playground/count.path
./bin/pathc exec playground/port.path --entry parse_port -- 443
./bin/pathc build playground/hello.path --release
./playground/build/hello
./bin/pathc inspect examples/refresh.path examples/manifest.json refresh
./bin/pathc expand examples/refresh.path examples/manifest.json refresh
```

Build creates `build/name`, `build/name.ll` and `build/name.path-build/src/main.rs`.
The generated Rust is an intermediate; edit the `.path` source. LLVM verification
runs on every build. `--expanded` builds the expanded entry; `--release` uses LLVM
optimization level 2. The interpreter remains available via `pathc run SOURCE
MANIFEST SCENARIO [--expanded]`.

Native programs accept `--json` for an execution report, `--scenario FILE` for a
deterministic primitive script, and `--fuel N` for an optional testing limit.
Arguments for String, U32, Bool and Unit use ordinary CLI text; aggregates can use
JSON values (see the example scenarios). All declared region outcomes are normal
process completion; malformed inputs and host failures return nonzero status.

Diagnostics currently report the first error per source file. Hover, completion,
rename, modules and a native source debugger are future tooling work. The compiler
implements the draft with the explicit clarifications in `v0.1-addendum.md`.
