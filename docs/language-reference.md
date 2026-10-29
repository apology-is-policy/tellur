# Tellur language reference

<!-- tellur-reference-version: 0.51.0 -->

This manual describes **compiler 0.51.0**, including the current ownership model,
control flow, data types and editor workflow. It is the reference for writing Tellur
today. The [roadmap](language-revision.md) and linked design notes also discuss
proposals; those proposals are not executable language features unless this manual
says they are implemented. The [archived v0.1 reference](language-reference-v0.1.md)
describes the original profile.

Tellur is still experimental. When this manual and the compiler disagree, treat that
as a bug to investigate, not a promise that either must remain unchanged. The
[maintenance checklist](reference/maintenance.md) requires semantic and example
updates at every implementation checkpoint.

## How to use this manual

For a first program, start with [editing and running](#edit-and-run),
[controls](#controls-and-declarations), and [outcomes](#outcomes-payloads-and-paths).
For exact borrowing semantics, read [ownership](#ownership-cloning-and-mutation)
and [references](#references-borrowed-calls-and-lifetimes), including the sections on
[`from`](#declared-result-origins-from) and [origin groups](#grouped-struct-results-a---and-input).

Complete `path` examples are checked by the compiler in the documentation test.
Standalone statement examples are checked inside an implicit `control main()` with
a final `exit done;` where needed. A *context fragment* is explicitly marked: it
needs declarations or earlier decisions from the surrounding discussion. Rejected
examples in the diagnostics chapter are checked for their stated diagnostic code.
These are static checks, not claims that every example was executed against live I/O.

The companion chapters provide quick lookup:

- [Lexical structure and precedence](reference/lexical-structure.md): names, comments, literals, operators and punctuation.
- [Core grammar](reference/grammar.md): schematic syntax for modern source programs.
- [Diagnostics and limits](reference/diagnostics-and-limits.md): rejected programs, repairs and implementation budgets.
- [Feature status](reference/support-status.md): implemented boundaries and queued features.
- [Glossary](reference/glossary.md): precise meanings of Tellur's terminology.

## Contents

- [Semantic foundations](#semantic-foundations)
- [Edit and run](#edit-and-run)
- [Controls and declarations](#controls-and-declarations)
- [Values and expressions](#values-and-expressions)
- [Structs, variants and distinct wrappers](#structs-variants-and-distinct-wrappers)
- [Outcomes, payloads and paths](#outcomes-payloads-and-paths)
- [Named outcome contracts](#named-outcome-contracts)
- [Inline outcome adapters](#inline-outcome-adapters)
- [Captured completed outcomes](#captured-completed-outcomes)
- [Single-completion calls and forwarding](#single-completion-calls-and-forwarding)
- [Path selection and scopes](#path-selection-and-scopes)
- [Ownership, cloning and mutation](#ownership-cloning-and-mutation)
- [References, borrowed calls and lifetimes](#references-borrowed-calls-and-lifetimes)
- [Arrays, vectors and maps](#arrays-vectors-and-maps)
- [Must-complete resources](#must-complete-resources)
- [Loops](#loops)
- [File modules and private representations](#file-modules-and-private-representations)
- [Generic data](#generic-data)
- [Concrete traits and associated types](#concrete-traits-and-associated-types)
- [Generic controls](#generic-controls)
- [Live primitive library](#live-primitive-library)
- [Tools and compatibility](#tools-and-compatibility)
- [Compare supplied scenarios](#compare-supplied-scenarios)
- [Validate expansion](#validate-expansion)
- [Extract a tail into a helper](#extract-a-tail-into-a-helper)
- [Query structural protocols](#query-structural-protocols)

## Semantic foundations

A program is a set of declarations connected by explicit file imports. A **control**
is invoked with typed arguments. When it completes, it does so through exactly one
declared **outcome**: a label plus an ordered payload. An exit contract describes those possible
completions. It does not run code or allocate a payload. A `delta` handles every
alternative and establishes path facts; selectors choose the histories on which
later statements execute.

There are three distinct uses of similar braces:

| Position | Example | Meaning |
| --- | --- | --- |
| Exit contract | `exits found { string text }` | Describes a payload field and its type |
| Delta arm | `found { string received } => available` | Declares a local binding for the received field |
| Exit statement | `exit found { message };` | Evaluates and transfers the actual payload value |

Field names in a contract document positions. They are not local variables in the
control body, and callers choose their own binding names. The payload above is a
string, not an implicit `option<string>`. Several alternatives form a completion
contract; `option<t>` is a separate ordinary variant with `some` and `none` tags.

<!-- tellur:check -->
```tellur
control greeting() exits found { string text } {
    string message = "hello";
    exit found { message };
}

control main() exits process_result {
    delta greeting() { found { string received } => available }
    [available]: emit(received); exit done;
}

outcome process_result { @exit 0 done }
```

In this program `text` names a contract field, `message` is the producer's local
owner, and `received` is the caller's local owner after transfer. `available` is a
path atom, not a value. No implicit cloning occurs. A `capture` can instead store a
completed outcome for later handling; the producer still runs immediately.
`@exit 0` gives this entry a successful process status when built as an executable.
Most later examples focus on library semantics: native entry points additionally
need a complete [status policy](reference/process-entry.md).

The current execution model is sequential. Statements execute in source order on
their selected histories. Arguments and construction fields evaluate left to right;
boolean `&&` and `||` short-circuit. Only the selected adapter arm executes. Source
control calls cannot recurse; tail loops provide repetition. There are no async
tasks, implicit exceptions, or implicit error propagation in this compiler profile.
Declared failure outcomes are normal completions. Host failures, including allocation
failure or an unsupported live primitive adapter, are outside those contracts.

The checker reasons about **structural histories**. It requires coverage of all
alternatives even if a literal or application invariant makes one unreachable at
runtime. Repeating the same boolean expression in two decisions does not establish
a logical relationship between those decisions. Reuse established path atoms when
you need that relationship. A diagnostic witness means a history *may reach* a site
in this model; it is not a concrete input that necessarily reaches it.

Types are nominal where named source declarations introduce identities. Ownership
and borrowing are separate from path selection: a selected statement must still
have an available value, permission to access it, and valid reference origins.
The language does not infer permission from spelling, a variable's name, or a
programmer's intended runtime invariant.

## Edit and run

Open `tellur.code-workspace` in VS Code and trust this local workspace when
prompted. Open `playground/hello.tellur`, then use **Tellur: Build and Run** (`⌘⌥R` or
`Ctrl+Alt+R`). **Tellur: Build Native Program** uses `⌘⌥B` / `Ctrl+Alt+B`. Output goes
to **Tellur Build**. Hover over a control's declaration name to see its checked exit
contract; Outline also shows inferred contracts. Diagnostics use unsaved source.

Try `outcome_origins.tellur`, `public_variants.tellur`, `borrowed_outcomes.tellur`, `opaque_splits.tellur`, `carrier_reborrows.tellur`, `record_inputs.tellur`, `inferred_payloads.tellur`, `stored_references.tellur`, `forwarding.tellur`, `named_outcomes.tellur`, `stored_adapters.tellur`, `adapters.tellur`, `outcomes.tellur`, `collections.tellur`, `references.tellur`, `count.tellur`, `port.tellur`, and `read_file.tellur`. Create new `.tellur` files beside
`playground/manifest.json` to use the same library. The entry file and its explicit import graph form the program; unrelated files
are not implicitly included. Put helpers in the same file or import their module. File paths are relative to the
working directory (the workspace root when launched by the extension).

```sh
./bin/tellurc check playground/hello.tellur
./bin/tellurc exec playground/count.tellur
./bin/tellurc exec playground/port.tellur --entry parse_port -- 443
./bin/tellurc build playground/hello.tellur --release --out build/hello
./build/hello
```

The compiler discovers the nearest ancestor `manifest.json`, or accepts its path
as the next argument. Entry defaults to `main`. Native builds require Rust/Cargo;
the executable is standalone. Builds also emit `.ll` and `.tellur-build/` source.

## Controls and declarations

<!-- tellur:check -->
```tellur
control main() exits auto {
    u32 number = 42;
    string text = u32_text(number);
    auto message = concat("The answer is ", text);
    emit(message);
    exit done;
}
```

Use snake_case throughout. `control` names a callable unit of control flow. Parameters
and declarations put the type first. Bindings are immutable by default and require initializers. Use `mutable` to permit assignment.
`ref auto` and `ref mutable auto` infer the referent type in initialized local
bindings and decision payload bindings. The reference mode remains explicit:

<!-- tellur:body -->
```tellur
mutable string first = "before";
ref mutable auto first_b = borrow mutable first;
deref first_b = "after";
```

Here `first_b` has type `ref mutable string`. `auto first_b = borrow mutable first;`
also works, inferring the entire reference type. Neither form changes ownership,
exclusivity or lifetime checks. `ref auto` does not accept an exclusive reference
implicitly, and `ref mutable auto` does not accept a shared reference. Parameters,
struct fields and explicit result contracts still require concrete referent types;
this is binding inference, not a general type wildcard.

In VS Code, hover over `auto` to see the full inferred binding type, including
`ref` and `mutable` when applicable. This also works for delta payloads, captures,
stored adapters, and generic bodies (which display their type parameters).
Hovering over the `auto` in `exits auto` shows the inferred control signature.
Hovers follow unsaved edits and require a successfully checked compilation unit;
invalid source does not display stale or guessed types.

`auto` infers a local binding or payload type; it does not change ownership. Parameters
and loop parameters currently require explicit types. There is no forward declaration
of an uninitialized local. No implicit numeric or string conversions are inserted.

At the start of a control, loop or scoped body, `[entry]:` may be omitted. Those
statements belong to the enclosing entry selection, whether or not they produce a
delta. A flat `[selected]:` group still owns following statements until the next
selector or item boundary; omission never silently resets that group. Use `[entry]:`
to explicitly resume the enclosing entry selection.

`entry` selects every history entering the current control or loop. Statements run
in source order on their selected histories. Ordinary `[selector]:` groups do not
introduce a new scope. Inner lexical scopes cannot shadow outer values. Definitions
of the same value on disjoint paths are allowed if their types match.

## Values and expressions

| Type | Examples |
| --- | --- |
| `bool` | `true`, `false` |
| `u32` | `0`, `42u32`, up to 4294967295 |
| `string` | `"text"`, escapes `\n`, `\r`, `\t`, `\\`, `\"` |
| `bytes` | `bytes"00ff7a"` |
| `unit` | `unit` |
| `option<t>` | `some { value }`, contextually typed `none` |
| Record / variant | Declared in source or the manifest; `pair { left, right }`, `state.ready { value }` |

<!-- tellur:body -->
```tellur
[entry]:
    option<u32> missing = none;
    auto present = some { 42 };
    option<option<u32>> nested = some { none };
```

`auto missing = none;` is ambiguous. Explicit types supply context to constructors.
Qualified `option.some` and `option.none` disambiguate builtins from manifest
constructors. Options are ordinary data variants, not control outcome values.

Expressions support construction, record field projection, `!`, `&&`, `||`, and
comparisons. Logical operators short-circuit. Equality is defined for bool, u32,
string and unit; ordering for u32. Comparisons cannot chain. Checked arithmetic is
still outcome-producing (`checked_add`, etc.), not hidden inside `+` expressions.

The playground declares `u32_text` and `concat` as pure with `pure: true` in the
manifest. Both support nested expression calls, and compute without scripted host
calls or observable primitive events. Calling them through delta is also allowed.
Only these implemented builtin contracts can currently be marked pure; a single
outcome or empty effects list alone does not authorize expression invocation.

## Structs, variants and distinct wrappers

<!-- tellur:check -->
```tellur
struct user_id {
    u32 value;
}

struct person {
    user_id id;
    string name;
}

variant contact {
    known { person identity },
    anonymous
}

control main() exits done {
    person ada = person { user_id { 7 }, "Ada" };
    contact selected = contact.known { ada };
    delta selected {
        known { person identity } => available,
        anonymous => missing
    }
    [available]: emit(identity.name);
    [entry]: exit done;
}
```

Struct fields end with semicolons. Variant alternatives and their payload fields use
commas; trailing commas are accepted. Empty structs are allowed. A variant needs at
least one alternative. Struct construction accepts positional values in declaration order or named fields.
Qualified variant constructors avoid ambiguity between similarly named tags.

Types are nominal: a separately declared `invoice_id` does not accept a `user_id`,
even when both contain one `u32`. Convert explicitly by extracting and constructing.

Named construction specifies every field exactly once, in any order:

_Context fragment: Uses the person and user_id declarations above._

<!-- tellur:fragment Uses the person and user_id declarations above. -->
```tellur
person value = person { name: "Ada", id: user_id { value: 7 } };
```

Initializers evaluate left to right **as written**; storage and projection use the
struct's declaration order. For `struct pair { string first; string second; }`,
`pair { second: clone(borrow owner), first: owner }` clones before moving `owner`.
Reversing those initializers is a use after move. Existing local loans remain live
through their last statement; named construction does not shorten a loan mid-statement.

Missing, duplicate, unknown and mistyped fields are errors. Named and positional
fields cannot mix in one construction. There are no implicit defaults, field-name
shorthands, update spreads or nominal conversions. Named fields currently construct
structs; variant/outcome payloads keep positional braces. Imported structs require
all fields to be public for direct construction. See
[the runnable example](../playground/named_fields.tellur).

Composition embeds other types; there is no implementation inheritance. Declarations
may precede or follow their users within one compilation unit, and a unit may contain
only data declarations. Source and manifest types share a type namespace.

An ordinary variant has no intrinsic handling obligation. Copy depends on all of its
payload types; owning fields move, and explicit `clone(borrow value)` duplicates only
cloneable ordinary contents. Resources and nested outcomes keep their completion
obligations through every struct, variant and collection. Existing field moves,
mutation and borrowed field-return rules apply to source structs too.

Declarations and fields are visible within their defining file. Use `pub` on types,
controls and individual struct fields to expose them across imports. Generic
bounds on data parameters and recursive layouts remain unsupported. Local struct
reference fields are supported; see [stored references](#stored-references-in-local-structs). One-field structs supply distinct wrappers without a new keyword.
See `playground/data_types.tellur` and [the accepted type-system direction](type-system.md).

## Outcomes, payloads and paths

<!-- tellur:check -->
```tellur
control parse_port(string input) exits ready { u32 port }, invalid {
    [entry]:
        delta parse_u32(input) {
            number { u32 port } => parsed,
            malformed => bad_text,
            overflow => too_large,
        }
    [parsed]: exit ready { port };
    [bad_text | too_large]: exit invalid;
}
```

This small example parses a u32; the playground's version also checks port range.

`delta` invokes an operation or decides a boolean/closed variant. Every outcome must
have one explicit arm. There are no wildcard arms or implicit error propagation.
`number` is an outcome label, `port` is a declared payload binding, and `parsed` is a
fresh path atom. `number { auto port }` is the inferred form. Payload positions are
ordered, and declared types must match exactly. Pattern braces do not create a scope.
A payload can be used only on paths where its binding is definitely available.

A boolean example is `delta (number <= limit) { true => more, false => finished }`.
Decision blocks do not need a trailing semicolon; ordinary statements do. Trailing
commas in decision arms and parameter/payload lists are accepted. To decide a freshly
constructed brace value directly, parenthesize it: `delta (some { 1 }) { ... }`.
This distinguishes construction braces from the decision's arm braces.

Name a decision site with `reading = delta read(borrow file) { ... }` for provenance.
An exit ends the current control invocation, not its caller. Outcome labels are not
ordinary values by themselves; `capture` produces an owned first-class completed
outcome. Ordinary data variants remain distinct from completion obligations.

On a private control, `exits auto` collects explicit exits from the checked body and infers their payload
types. All sites of an outcome must have matching arity/types. Callers are checked
against that complete contract, even when the callee appears later in the file.
Coverage and resource checks still apply. Recursive calls remain unsupported. A body
with no exit sites needs an explicit contract (for example an indefinitely restarting
loop). Inference is not a search for concretely feasible paths or backward type inference.

### Infer selected payloads with `...`

<!-- tellur:check -->
```tellur
control lookup(bool available) exits ready..., missing {
    exit delta available {
        true => ready { "found" },
        false => missing {}
    };
}
```

`ready...` declares the label and infers a **nonempty, fixed** payload from the
checked body. Bare `missing` remains explicitly payload-free. The ellipsis is one
contiguous token; it is not a variadic payload, a wildcard, or permission to add
other outcome labels. Its inferred types and borrowed-result origins are available
to callers, captures, forwarding and hover just like an explicit contract.

| Declaration | Meaning |
| --- | --- |
| `exits ready..., missing` | Infer only `ready`'s nonempty payload; `missing` has none |
| `exits ready { string text }, missing` | Explicit payload contract |
| `exits lookup_result` | Use a named outcome contract |
| `exits auto` | Infer both labels and payloads; payloads may be empty |

All exits with a marked label must agree on payload arity and types. An empty
payload or a marked label never produced by the body is an inference error. An
ambiguous expression needs a local type annotation or an explicit exit payload
contract. Explicit and inferred alternatives may be mixed. Bare labels and named
outcome contracts retain their existing semantics; an ellipsis-marked name always
names an individual exit, even if a named outcome type has the same spelling.

Generic control definitions are checked symbolically before their concrete uses.
Public controls and the existing trait-method profile require explicit contracts, including in
implementations; body-free trait declarations and named outcome definitions cannot
use payload inference. `...` is only a declaration marker, not an exit expression
or arm-pattern marker. Existing ownership, lifetime, coverage and recursion checks
remain in force. See [the runnable example](../playground/inferred_payloads.tellur).

### Explicit public contracts

Since 0.44, every `pub control` declares its complete public exit interface. Write
explicit labels and payload types, or name a public outcome contract. `exits auto`
and `label...` remain available on private controls. This applies even to an unused
generic definition: an implementation edit must not silently grow its exported API.
A private `main` remains a valid executable entry point.

<!-- tellur:check -->
```tellur
control lookup(bool available) exits auto {
    exit delta available {
        true => ready { "found" },
        false => missing {}
    };
}

pub control read_setting(bool available) exits ready { string value }, missing {
    forward lookup(available);
}
```

A new exit in `lookup` must be handled internally or deliberately added to
`read_setting`'s contract. Merely forwarding it does not enlarge that contract.
The compiler reports the incompatible forward and includes related locations for
the producing exit and forwarding helpers. Missing arms on a direct call
also receive these notes. A declared alternative with no producing exit points to
the declaration. Since 0.46, captures, moves, stored/nested adapters, delayed exits
and merged labels also retain bounded contributor notes, including local record
and sum payloads. Unknown mutation and loop-carried lineage are forgotten.
Notes are bounded; they are structural contributors, not proof
that a concrete input reaches that exit. See the
[diagnostic coverage limits](reference/diagnostics-and-limits.md).

Public borrowed results must also declare their origins: direct references use
`from input` (including contained-origin selectors), and reference-bearing
aggregates use bracket mappings for all required groups. A reference to an
aggregate declares both its storage origin and contained-origin mapping. Named
completion contracts bind their groups at `exits reply[input, ...]`. These are the
same checked origin rules used by private controls; public boundaries require the
annotations instead of inferring them from the body.

<!-- tellur:check -->
```tellur
pub control text_view(ref string input)
    exits ready { ref string value from input }
{
    exit ready { input };
}
```

An incorrect explicit origin still fails lifetime checking. Local `auto` bindings
and pattern inference are unaffected. See [public-contract diagnostics](reference/diagnostics-and-limits.md#public-interfaces-e_public_contract)
and [the runnable example](../playground/public_contracts.tellur).

## Named outcome contracts

Named alternatives can carry `@exit N` defaults for executable status. See
[process entry contracts](reference/process-entry.md) for annotations, per-entry
overrides, completeness checking, and final-contract semantics.

Borrowed named outcomes can declare [origin groups and completion bindings](#named-outcome-and-option-origin-schemas), including dependencies inherited through options.

<!-- tellur:check -->
```tellur
outcome message {
    ready { string text },
    empty
}

control greeting(bool available) exits message {
    exit delta available {
        true  => ready { "hello" },
        false => empty {}
    };
}

control forward(message saved) exits message {
    exit saved;
}
```

`message` is a reusable, nominal type: multiple controls can declare `exits message`,
and their captured results have that same type. `auto saved = capture greeting(true);`
infers `message`. Declaration order does not matter. A named contract must have at
least one distinct alternative; its payload types must have known, finite layouts.
Nested and generic contracts work. Reference-bearing contracts can be stored and
handled locally, including inside other records and tagged values. A whole named
outcome can be borrowed by a control without discharging its handling obligation.
A direct control can also return a reference to the whole outcome, preserving its
storage and contained-source dependencies. Selected direct reference payloads can
be returned too. Recursive layouts remain unsupported.

A named outcome always carries a handling obligation, including nullary or Copy
payloads. Handle it with an exhaustive delta or transfer it to another owner.
`exit saved;` forwards its alternatives. Handling the result does not discharge a
resource payload's separate obligation to be completed or transferred.

Explicit construction does not invoke a producer:

_Context fragment: Uses the message contract above; both outcomes still need handling._

<!-- tellur:fragment Uses the message contract above; both outcomes still need handling. -->
```tellur
message saved = ready { "hello" };  // The annotation selects the contract.
auto absent = message.empty {};    // Qualification selects it without an annotation.
```

Short constructor names require a named expected type; named declarations do not
add their alternatives to global short-name lookup. This keeps `some`/`none` option
constructors and ordinary variants stable. Qualified patterns such as
`message.ready { string text }` are also accepted.

A typed adapter can explicitly target a named contract:

_Context fragment: Uses message above and a boolean available; saved still needs handling._

<!-- tellur:fragment Uses message above and a boolean available; saved still needs handling. -->
```tellur
message saved = delta available {
    true  => ready { "adapted" },
    false => empty {}
};
```

Different named contracts remain distinct even when their alternatives look alike.
Use an explicit adapter to map between them. `message saved = capture producer();`
can also explicitly target `message`: every producer alternative must exist there
with matching payload arity and types. The target may have additional alternatives,
which subsequent handling must cover. There is no implicit narrowing or renaming.

A single bare name after `exits` selects a declared named contract. To explicitly
mean one nullary exit of that same name, write `exits { message }` (or `message {}`).
Without a matching named declaration, the existing `exits done` syntax still means
one nullary exit. `outcome<producer>` remains available for producer-specific types;
an explicit annotation can retain that type even when the producer adopts a named
contract. See `playground/named_outcomes.tellur`.

## Inline outcome adapters

<!-- tellur:check -->
```tellur
control first(ref vector<string> words) exits auto {
    exit delta vector_get(words, 0) {
        found { ref string word } => [available]: found { word },
        out_of_bounds         => [empty]: empty {}
    };
}
```

`exit delta` immediately returns the selected arm's completed outcome. Every input
alternative needs an explicit arm. An arm returns an outcome constructor or another
nested delta. Constructors in this expression position always use braces, including
`empty {}`. The statement `exit empty;` remains a compact nullary exit when `empty`
is not a value binding; `exit empty {};` explicitly names the exit label.

`[available]:` and `[empty]:` are optional semantic path names. Without them the
compiler retains the input outcome names and fresh decision identities. Inline
payload bindings and path names are local to the arm; sibling arms may reuse a name
with different types. Existing rules still prohibit shadowing an enclosing value.
Flat delta patterns keep their existing path-dependent outer bindings.

<!-- tellur:check -->
```tellur
control successor(string input) exits auto {
    exit delta parse_u32(input) {
        number { u32 number } => delta checked_add(number, 1) {
            sum { u32 next } => ready { next },
            overflow         => too_large {}
        },
        malformed => invalid {},
        overflow  => too_large {}
    };
}
```

This infers `ready { u32 value }`, `invalid`, and `too_large`. All contributors to a
label must agree on payload arity and types. Renaming an alternative is explicit;
no implicit payload cloning or conversion is inserted. Only the chosen arm runs.
The compiler emits checked branches and exits directly, so a tail adapter needs no
intermediate stored outcome. Borrowed payloads retain their checked owner origins.

Graph expansion preserves invocation identities and internal decision histories even
when several routes produce the same public outcome. Parse overflow and addition
overflow remain distinguishable. A control's unconditional statements retain the
control/invocation provenance without introducing a fake root decision.

Adapters can also initialize an owning outcome binding:

_Context fragment: Inside a control with a vector<string> words binding._

<!-- tellur:fragment Inside a control with a vector<string> words binding. -->
```tellur
auto saved = delta vector_get(borrow words, 0) {
    found { ref string word } => ready { clone(word) },
    out_of_bounds         => empty {}
};
emit("Lookup complete; handling follows.");
exit saved;
```

The compiler infers a private closed outcome type from all leaves, including nested
adapters. Every leaf with the same label must have the same payload arity and types.
Each selected branch stores its payload before its lexical bindings are released.
Borrowed inputs may be read, cloned, or retained in the result when their owners
outlive the retained references. A result cannot borrow storage owned by an arm
that has completed. See [local borrowed outcomes](#local-borrowed-variants-and-outcomes). An unused resource payload in any arm is an error.

Use a named contract or an explicit producer contract when the result must cross a
typed value boundary:

_Context fragment: Inside a control with available and the choose producer below; saved needs handling._

<!-- tellur:fragment Inside a control with available and the choose producer below; saved needs handling. -->
```tellur
outcome<choose> saved = delta available {
    true  => ready { "an adapted message" },
    false => empty {}
};
```

Here `choose` has the `ready { string } | empty` contract shown below. Each output
label must occur in the target contract, with exactly its payload types. The target
may have additional alternatives; subsequent handling must cover its full contract.
The annotation also supplies context for collection and option literals in payloads.
This is an explicit adaptation; it does not make different producers' outcome types
implicitly assignable. Inferred adapter types are private to their expression; use
`auto` for their moves and an explicit target contract to unify independent adapters.

Adapters currently work in tail exits and declaration initializers, including nested
arms. Assignment right-hand sides, call arguments and arm statement blocks are not
yet adapter positions. See `playground/stored_adapters.tellur` and `adapters.tellur`.

## Captured completed outcomes

<!-- tellur:check -->
```tellur
control main() exits auto {
    outcome<choose> saved = capture choose(true);
    emit("The call has already completed.");
    exit delta saved {
        ready { string message } => done { message },
        empty                    => empty {}
    };
}

control choose(bool available) exits auto {
    exit delta available {
        true  => ready { "a completed result" },
        false => empty {}
    };
}
```

`capture operation(arguments)` invokes **now** and stores its selected completed
outcome. It is neither a future nor a deferred call. `auto saved = capture ...;`
infers the named contract when the producer declares one, otherwise `outcome<choose>`.
`outcome<choose>` names that producer’s checked closed completion contract. Calls used in capture must be source controls or manifest
primitives; wrap a generic collection operation in a control when capture is needed.

The result owns a mandatory **handling obligation**, independently of whether its
payload is Copy, ordinary owned data, a resource, or empty. `delta saved { ... }`
consumes it. Moving it into another binding, parameter, collection or exit payload
transfers the obligation. `drop`, `clone`, leaving it unused at an exit, and deciding
it twice are rejected. Containers cannot hide the obligation. After handling an
outcome, any resource payload still needs its own explicit completion or transfer.

A whole result can be forwarded without listing its alternatives:

_Context fragment: Uses the choose producer in the preceding complete example._

<!-- tellur:fragment Uses the choose producer in the preceding complete example. -->
```tellur
control forward(outcome<choose> saved) exits auto {
    exit saved;
}
```

Forwarding infers the producer's alternatives, or checks them against an explicit
exit contract; a larger explicit contract can include additional alternatives.
For bare `exit name;`, an existing completed-outcome binding is forwarded. If an
ordinary value has that name, write `exit name {};` to explicitly select the nullary
exit label. There is no implicit renaming, default error propagation or wildcard arm.

Capture preserves the invocation's position relative to later effects. Handling the
saved result does not repeat its call. Expansion still occurs at that invocation,
retaining call-site and decision provenance. Stored values carry a tag and payload,
not a full runtime history; internal capture paths are not exported into source scope.

Stored adapters and capture support both owned payloads and local borrowed payloads.
A saved reference keeps its owner borrowed until its last use. Unpacking transfers
that dependency to the selected payload; the other constructors contribute no loans. Producer-indexed outcome types currently match exactly when passed
as values; a typed adapter can explicitly map into an existing producer contract.
Implicit cross-producer structural assignment, whole-outcome
bindings inside adapter patterns, general expression-position adapters, general graph extraction,
and async task support remain future work. See `playground/outcomes.tellur`.

## Single-completion calls and forwarding

_Context fragment: Uses display and the first and second values supplied by the enclosing control._

<!-- tellur:fragment Uses display and the first and second values supplied by the enclosing control. -->
```tellur
display(first);
forward display(second);
```

A bare call statement requires exactly one declared outcome and no payload. It
invokes once and continues on the current selection. It does not imply purity or
introduce a path name. If a callee later gains another outcome or a payload, the
statement is rejected; use an explicit `delta`, `capture`, or `forward`.

`forward operation(arguments);` invokes now and returns its selected outcome,
unchanged, from the enclosing control. Every declared callee outcome must fit the
exit contract: same label, payload count, ordered types and reference modes.
The target may admit additional alternatives; `exits auto` infers them. Moves,
resource completion and borrow lifetimes are checked at the transfer. This is
boundary routing, not implicit conversion between nominal outcome value types.
Use `exit saved;` for an already completed outcome.

In a tail or stored adapter, bare `forward` preserves one entire selected outcome:

_Context fragment: Inside a control with a ref vector<string> words parameter._

<!-- tellur:fragment Inside a control with a ref vector<string> words parameter. -->
```tellur
exit delta vector_get(words, 0) {
    found => forward,
    out_of_bounds => empty {}
};
```

The `found` arm needs no payload variables. Explicit bindings remain allowed and
checked. Nested adapters support the same shorthand. Local stored adapters can
also retain a borrowed payload; immediate adapters can return a borrow of their input.

Forwarding delegates the callee's contract. A new outcome fails against an explicit
target that lacks it, but may remain compatible with a wider target or grow an
inferred contract. Keep explicit arms when each alternative deserves a separate
review. See `playground/forwarding.tellur` and the [semantic foundation](semantic-foundation.md).

## Path selection and scopes

Selectors use `a & b` (both) and `a | b` (either); intersection binds more tightly.
A union executes shared work once even when both constituents hold. Facts remain
true on descendant histories; an operation's atom is false where it was skipped.
There are no forward references to paths or executable path negation/subtraction.

_Context fragment: Uses path atoms produced by earlier read and close decisions._

<!-- tellur:fragment Uses path atoms produced by earlier read and close decisions. -->
```tellur
path any_failure = read_bad | close_bad;
[opened] over &: {
    [read_bad]: emit("Read failed");
    [close_bad]: emit("Close failed");
}
```

`over &:` intersects the outer selector into each inner one and introduces a lexical
scope. Inner bindings, atoms and aliases cannot escape. Every live history must exit
or restart a loop. Selected work after an overlapping exit is rejected. Diagnostics
report a structural witness (**may reach**), not proof that a concrete input reaches
that history. Separate boolean decisions remain independent; reuse established atoms.

## Ownership, cloning and mutation

In a `control`, `string` and `bytes` are owning, non-Copy values. Assigning them to a
new binding, passing them by value, emitting them, or returning them transfers ownership.
`bool`, `u32` and `unit` are Copy. A record, variant or option is Copy only when every
possible payload is Copy. Existing non-resource opaque manifest types are Copy;
resource types are not. There are no user-defined Copy/Clone/Drop implementations yet.

<!-- tellur:body -->
```tellur
[entry]:
    mutable string message = "first";
    string saved = clone(borrow message);
    string moved = message;
    message = "second";
    emit(borrow message);       // observe without consuming
    emit(moved);          // consumes moved
    drop(saved);          // explicitly destroy an ordinary owned value
```

`auto` does not change these rules. Cloning is explicit and recursively copies ordinary
aggregates. It cannot duplicate must-complete resources. Allocation failure is currently
a host failure, not a declared recoverable outcome. Unused ordinary values are destroyed
at scope exit; assignment destroys the replaced ordinary value. Partial record moves
leave other fields available. A moved field of a mutable binding can be reinitialized;
a field of a wholly moved parent requires reinitializing that parent first.

Availability, moves, reinitialization and loans are checked per selected history.
Disjoint definitions of one owned binding must agree on type and mutability. Copy
reads and comparisons do not consume values. `emit(value)` consumes an owned string;
`emit(borrow value)` borrows it.

## References, borrowed calls and lifetimes

Reference types and operations use these words:

| Role | Current spelling | Compatibility spelling |
| --- | --- | --- |
| Shared-reference type | `ref string` | `&string` |
| Exclusive-reference type | `ref mutable string` | `&mutable string` |
| Shared borrow expression | `borrow owner` | `&owner` |
| Exclusive borrow expression | `borrow mutable owner` | `&mutable owner` |
| Access the referent | `deref view` | `*view` |

These spellings have the same ownership semantics. `borrow` and `deref` are now
expression keywords; controls or bindings previously using those names need different
names. `ref` belongs in a type;
`borrow` and `deref` belong in expressions. A borrow creates a reference to existing
storage; it does not move or clone the referent. Dereferencing identifies that
storage; its use determines whether it is read, written, or illegally moved.

Field access through a reference is transparent: `input.text` means
`(deref input).text` when `input` is a reference. Thus `borrow input.text` takes a
reference to the field. The same checks for visibility, exclusivity, liveness and
moves apply to both forms. A bare `input` remains a reference: it is not automatically
dereferenced when passed to a call. No method lookup or implicit borrowing is added.

Field selection binds tighter than the prefix operations. `deref input.text` means
`deref (input.text)`; use `(deref input).text` for an explicit projection, or simply
`input.text`. Prefix operators may compose, as in `borrow deref input`.

`ref mutable string view` permits writing through the reference. By contrast,
`mutable ref string view` would permit rebinding a shared-reference variable; this
second feature remains unsupported and is diagnosed. Declarations still need an
initializer. `&` and `|` remain the path intersection/union operators, and `&&` and
`||` remain boolean operators.

<!-- tellur:check -->
```tellur
control duplicate(ref string text) exits auto {
    [entry]: exit copied { clone(text) };
}

control main() exits auto {
    [entry]:
        mutable string message = "before";
        ref mutable string edit = borrow mutable message;
        deref edit = "after";
        emit(edit);
        drop(edit);
        ref string view = borrow message;
        delta duplicate(view) { copied { string copy } => ready }
    [ready]:
        drop(view);
        exit done { message, copy };
}
```

`ref t` is a shared reference; `ref mutable t` is exclusive. The modifier `mutable`
distinguishes mutable bindings from exclusive borrowing by its position.
An exclusive reference requires a mutable owner. Several shared references may coexist;
an exclusive reference excludes all overlapping access through other names. Disjoint
record fields can be borrowed independently. `deref reference` accesses its referent;
`reference.field` projects a borrowed record. Moving an owning value through a
reference is rejected; Copy reads and explicit cloning are allowed.

A shared reference binding can be copied. Transferring an exclusive reference moves it.
`clone(reference)` clones the referent, not the reference. `emit(reference)` observes a
borrowed string. Borrowed call parameters accept `borrow owner` or an existing reference name;
ordinary owning parameters accept the value itself. There is no implicit borrow at a
modern call. Shared parameter names are references too, so these rules stay consistent
inside and outside a control.

Both shared and exclusive references cross control boundaries. For an exclusive
parameter, pass `borrow mutable owner` or an existing exclusive reference. Calls reborrow
existing references: the caller can use its reference again once the call and any
returned loans finish. A helper can update the caller's original storage.

<!-- tellur:check -->
```tellur
control rewrite(ref mutable string text) exits rewritten { ref mutable string result } {
    [entry]:
        deref text = "changed by a helper";
        exit rewritten { text };
}

control main() exits auto {
    [entry]:
        mutable string owner = "before";
        delta rewrite(borrow mutable owner) {
            rewritten { ref mutable string edited } => ready,
        }
    [ready]:
        emit(edited);
        owner = "after the borrow's last use";
        exit done { owner };
}
```

A returned reference must borrow an input reference, possibly a field of it. Returning
`borrow local` or borrowing an owning parameter into a result is rejected: that owner would
be destroyed when the invocation ends. Return lifetime relationships are inferred from
the checked body for both explicit exits and `exits auto`. Declaration hover lists
the input/field sources. When a result may refer to either of two inputs, the caller
keeps both borrowed until the result's last use. This is conservative; it does not
pretend to know which input a runtime boolean chooses. Each completion alternative
has its own contract.

### Stored references in local structs

A struct field can hold a reference. Its owner stays separate from the struct:

<!-- tellur:check -->
```tellur
struct view {
    ref string text;
}

control main() exits done {
    mutable string owner = "hello";
    view first = view { borrow owner };
    view second = first;
    drop(first);
    emit(second.text);
    owner = "the final view is no longer used";
    exit done;
}
```

Reference origins are inferred for each local value; the struct declaration does
not name a local owner or extend its lifetime. Nested structs and fields such as
`ref t` in generic struct definitions work. Reference types cannot be supplied as arguments to user-defined data/control
type parameters. Builtin `option<ref t>` is a separately supported local storage form.

A shared reference is Copy and Clone. A struct is Copy only when every field is
Copy, and Clone only when every field is Clone. Copying or cloning a shared-reference
field copies its reference, not its referent. For example, a struct containing a
shared reference **and a string** moves on ordinary assignment; explicitly cloning
that struct clones the string and copies the reference. An exclusive reference is
neither Copy nor Clone, so its containing struct cannot acquire either capability.

Moving a record transfers its contained loans. Extracting an exclusive reference
field moves that field; other fields remain available, but the partially moved
record cannot be used as a whole. A shared reference field can be copied into a
local reference. `emit(view.text)`, `clone(view.text)`, borrowed call arguments,
`deref editor.text`, and transparent access such as `view.source.name` work as they
do for local references. `clone(view.text)` clones the **referent**, whereas
`clone(borrow view)` clones the **record**.

Every surviving copy or extracted reference keeps its owner borrowed. A loan ends
at its last use, explicit `drop(...)`, or lexical scope exit. Liveness is conservative
around loops and whole-value uses. Individual record fields now have separate
last uses, so a later use of another field need not keep an unrelated loan alive. Mutation through a shared reference, duplicated exclusive access, moving a
borrowed owner, and borrowing a temporary are rejected. Constructor expressions
are checked and evaluated in written order. Module field privacy still applies.

Local storage, grouped results, and grouped control inputs are supported. Records
containing shared or exclusive reference fields may also be borrowed as whole
records. Shared carrier access grants reads; exclusive carrier access can grant
exclusive reborrows of exclusive fields. Reference-bearing records can be local
variant/outcome payloads, including capture and adapters. Collections containing such records and reference-bearing loop parameters remain
unsupported; local direct shared-reference elements are described below.
Reference-bearing storage cannot yet be rebound or assigned, and disjoint definitions
cannot merge into one record binding. Owned fields can be assigned normally.
These remaining boundaries are diagnosed rather than losing contained-loan information.

See [the runnable example](../playground/stored_references.tellur).

### Local borrowed variants and outcomes

<!-- tellur:check -->
```tellur
variant selection {
    found { ref string text },
    empty
}

control main() exits done {
    mutable string owner = "hello";
    selection saved = found { borrow owner };
    delta saved { found { auto text } => available, empty => missing }
    [available]: emit(text);
    [missing]: emit("empty");
    [entry]: owner = "available again"; exit done;
}
```

A local variant or outcome can carry shared/exclusive references or a record that
contains them. `option<ref string>` and `option<ref mutable string>` work as local
storage too; `auto saved = some { borrow owner };` infers the optional type.
Constructors register each reference before checking later arguments. Only payloads
that are actually constructed contribute loans. Exhaustive delta arms remain
required: a constant constructor does not remove structural paths or obligations.

Moving a tagged value transfers its loans. A shared-reference variant is Copy when
all of its payload fields are Copy, and Clone when all are Clone. Every surviving
copy retains the owner. An exclusive-reference payload makes the variant neither
Copy nor Clone. Ordinary variants may be dropped when their contents are disposable.
**Outcomes still must be handled or transferred exactly once**, including a known
empty outcome; they cannot be copied, cloned or discarded.

`capture` completes the producer immediately, then retains the selected payload's
loans. A stored adapter can also retain reference payloads and reference-bearing
records. Its payload cannot borrow storage owned by an arm that has completed.
For example, borrowing an owned string returned into an arm is rejected; retaining
a reference whose owner is outside the arm is allowed. Carrier authorization guards
and exclusive split permissions survive packing and unpacking.

A delta transfers the selected payload's origins to its bindings. Other constructors'
loans are released from the consumed value. A surviving shared copy remains an
independent retainer. Borrowed captured outcomes can be unpacked and forwarded as
direct reference exits; existing `from input` contracts continue to apply.

Records, variants, options, and outcomes may nest these values locally. Each delta
unwraps one tag layer. Moving a record field or unpacking a payload transfers all
of its nested loans; dropping a disposable field releases only that field's
retainers. An outcome remains mandatory to handle even when hidden inside another
variant or record. Independent exclusive references keep their own last uses.

Variants, builtin options and completed outcomes support **local tagged
storage**. Grouped source variants, grouped named outcomes, and options with complete
inner schemas additionally cross owned and borrowed-input control boundaries as
described below. Exclusive/nested collection elements, assignment/rebinding and
reference-bearing loop parameters remain gated. Loop-local nested borrowed
values are recreated on each iteration. An unproduced borrowed exit still
needs an explicit origin when the body supplies no inference evidence.

See [borrowed_outcomes.tellur](../playground/borrowed_outcomes.tellur) and
[nested_borrows.tellur](../playground/nested_borrows.tellur).

### Grouped variant contracts

Source variants can declare origin groups just as structs do. Groups cover reference
leaves across alternatives; an alternative with no references needs no mapping.

<!-- tellur:check -->
```tellur
variant text_selection['source] {
    found { ref string text <- 'source },
    empty
}

control select_text(ref string input, bool available)
    exits ready { text_selection[input] value }
{
    exit delta available {
        true => ready { text_selection.found { input } },
        false => ready { text_selection.empty {} }
    };
}

control unpack(text_selection input)
    exits found { ref string text from input['source] }, empty
{
    exit delta input {
        found { auto text } => found { text },
        empty => empty {}
    };
}
```

`text_selection[input]` binds the returned variant's group to the input reference.
`input['source]` in `unpack` selects an external dependency carried by its owned
variant parameter. It does not borrow the variant's own temporary storage. The
parameter follows its type's usual Copy/move/obligation rules: passing a variant
with an exclusive reference transfers it; a shared-reference Copy variant can
leave another retaining copy in the caller.

Each checked dependency records the enclosing alternative and payload position.
Until a returned value is unpacked, the caller conservatively retains all possible
origins in its contract. A delta retains the selected payload's dependencies and
releases those belonging only to other alternatives. An empty alternative carries
no reference. A surviving shared copy remains an independent retainer. Grouping
several fields or alternatives under one name still combines their possible sources;
use separate groups for independently releasable sources.

Nested grouped records and grouped source variants compose, including generic
specializations. For example, a variant payload can use
`text_pair['left, 'right] fields`, and a record field can store a grouped variant.
The mapping order follows the nested type's group declaration order. Every borrowed
leaf needs a complete public schema at these boundaries. Native execution never
projects a payload belonging to an inactive alternative.

Explicit mappings constrain every declared alternative, including alternatives the
body does not produce. Inferred `exits auto`, `ready...`, or an explicit payload type
without mappings require body evidence for every reference leaf of the returned
type. If a possible borrowed alternative is never constructed, write explicit
bracket mappings. Inferred and declared contracts preserve exclusive permissions,
contained origins and obligations through forwarding, capture and adapters.

When a call returns borrowed data derived from a variant parameter, first bind that
argument to a local value; direct constructor arguments at this boundary are not
supported yet. The same locally bound argument restriction applies to named outcomes
and options. Whole reference-bearing aggregate references can be returned directly
and stored locally or in public containers with complete nested schemas. Borrowed aggregate
trait results, collection storage and rebinding remain separate checkpoints. Grouped named outcomes and builtin
options now compose with these variant schemas across owned boundaries.

See [public_variants.tellur](../playground/public_variants.tellur) for exclusive references
crossing nested record/variant boundaries and releasing their owners independently.

### Named outcome and option origin schemas

A named outcome uses the same ordered origin groups as a variant. Unlike an ordinary
variant, it still requires handling or transfer, even when its selected alternative
has no payload. Wrapping it in an option does not remove that obligation.

<!-- tellur:check -->
```tellur
outcome borrowed_text['source] {
    found { ref string text <- 'source },
    missing
}

control find_text(ref string input) exits borrowed_text[input] {
    exit found { input };
}

control relay_text(borrowed_text input)
    exits ready { borrowed_text[input['source]] value }
{
    exit ready { input };
}

control main() exits done {
    string owner = "hello";
    borrowed_text saved = capture find_text(borrow owner);
    delta relay_text(saved) { ready { auto result } => received }
    [received]: delta result { found { auto text } => found, missing => missing }
    [found]: emit(text);
    [entry]: exit done;
}
```

`exits borrowed_text[input]` binds the contract's `'source` group to the parameter
`input`. It is shorthand for applying that dependency to every mapped result field;
it does not add an outer `borrowed_text` payload around the chosen exit. In contrast,
`exits ready { borrowed_text[input] value }` returns the whole completed outcome as
one payload, which the caller must subsequently handle or transfer.

Nested payload mappings use the same declaration order as records and variants.
`exits contract[a, b]` requires exactly the contract's number of groups. Parameter
origins and contained selectors such as `input['a]` may appear in these bindings.
The compiler checks actual returned references against the declared dependencies.
Without the bracket bindings, `exits contract` can infer dependencies from its body;
missing evidence still requires explicit contracts. The schema declaration itself
must always map every reference leaf, including alternatives not produced here.

An option automatically inherits the ordered origin groups of its inner type.
`option<ref string>` and `option<ref mutable string>` have one builtin group,
`'source`. `option<text_pair>` inherits `text_pair`'s group names and order;
additional option layers preserve those same groups. An option of an ungrouped
reference-bearing aggregate does not invent a complete schema for that aggregate.

<!-- tellur:check -->
```tellur
control maybe_text(ref string input, bool present)
    exits ready { option<ref string>[input] value }
{
    exit delta present {
        true => ready { some { input } },
        false => ready { none {} }
    };
}

control main() exits done {
    mutable string owner = "hello";
    delta maybe_text(borrow owner, true) { ready { auto saved } => received }
    [received]: delta saved { some { auto text } => some, none => none }
    [some]: emit(text);
    [entry]: owner = "available again"; exit done;
}
```

For an owned parameter `option<ref string> input`, `input['source]` denotes the
carried external owner. It does not denote the option variable's own storage.
The same applies to inherited groups of an option containing a named outcome or
record. Both shared and exclusive leaves transfer through owned calls/results;
the active payload keeps its dependencies, and `none` releases the option's loans.
Surviving shared copies retain their own dependencies. Native code never reads a
payload from an absent alternative. An explicitly declared optional result can be
returned as `none` even when its body provides no reference evidence; an inferred
reference-bearing result with no produced leaf needs explicit bracket bindings.

These schemas compose through finite generic record/variant/outcome/option nesting,
imports, capture, forwarding, borrowed sum inputs and certified expansion.
Anonymous producer-outcome schemas, exclusive/nested collection elements,
rebinding and borrowed loop state remain unsupported. Trait signatures support grouped named contracts and aggregate results with explicit
origin bindings; see [trait origin contracts](#aggregate-trait-origin-contracts).

See [outcome_origins.tellur](../playground/outcome_origins.tellur) for independent
exclusive owner groups carried through a named outcome and an option.

### Borrowing whole sums

`delta borrow value` inspects a variant, option or named outcome without consuming
it. `delta borrow mutable value` lends exclusive access to its active payloads.
A reference parameter or local reference is already a borrow: write `delta input`.
All alternatives still need arms. No payload address is read from an inactive tag.

The binding type follows the access mode:

| Stored payload | Shared sum borrow | Exclusive sum borrow |
| --- | --- | --- |
| `string` (or another non-reference type `t`) | `ref string` (`ref t`) | `ref mutable string` (`ref mutable t`) |
| `ref t` | `ref t` | `ref t` |
| `ref mutable t` | `ref t` | `ref mutable t` |

Ordinary payloads are borrowed in place, including Copy payloads. Reference payloads
are **reborrowed directly**; matching does not create a `ref ref` layer or transfer
an exclusive reference out of its owner. `auto` hover shows the resulting mode.
Nested aggregate payloads become references to those aggregates, so another delta
can inspect a nested sum. A shared carrier never grants exclusive access through
its nested fields. Payload fields still respect module privacy.

<!-- tellur:check -->
```tellur
variant message { text { string value }, empty }

control preview(ref message input)
    exits found { ref string value from input }, missing
{
    exit delta input {
        text { auto value } => found { value },
        empty => missing {}
    };
}

control main() exits done {
    mutable message saved = text { "before" };
    delta preview(borrow saved) { found { auto text } => found, missing => missing }
    [found]: emit(text);
    [entry]: delta borrow mutable saved {
        text { auto editable } => editing,
        empty => empty
    }
    [editing]: deref editable = "after";
    [entry]: delta saved { text { auto owned } => handled, empty => absent }
    [handled]: emit(owned);
    [entry]: exit done;
}
```

The final `delta saved` consumes the sum normally. Borrowed matching itself does
not discharge a named outcome's handling obligation or a resource payload's
completion obligation. It cannot move an owned payload out through a reference.
Independent payload fields may be lent exclusively together; the enclosing sum
cannot be moved, replaced or exclusively borrowed while a child still needs it.

A reference-bearing sum needs a complete origin schema at a public call boundary.
Local borrowing uses the concrete dependencies already known to the compiler, so
local and captured sums can be borrowed without declaring groups. Options inherit
the schema described above at public boundaries. Selectors retain their usual meanings:
`from input` protects the sum's storage, while `from input['source]` selects an
external source carried inside it.

<!-- tellur:check -->
```tellur
control optional_preview(ref option<ref mutable string> input)
    exits found { ref string text from input['source] }, missing
{
    exit delta input {
        some { auto text } => found { text },
        none => missing {}
    };
}
```

Here the payload is shared because `input` is shared, even though the stored
reference is exclusive. That stored exclusive permission also retains its
**authorizing carrier**: dropping or moving the option while the child is live is
rejected. A stored shared reference can be copied out and outlive the carrier, but
its external source stays protected. Last-use release applies to both kinds.

Returning a selected reference into ordinary payload storage uses `from input`;
returning a reborrowed external reference uses the corresponding contained group.
Returning a reference to an entire reference-bearing aggregate preserves both its
storage and contained-source dependencies, as described below.
Reference rebinding, stored references to references, and exclusive/nested
collection elements remain separate checkpoints.

See [sum_carriers.tellur](../playground/sum_carriers.tellur) for a named outcome
that is inspected, edited through an exclusive reborrow, and finally handled.

### Returning references to reference-bearing aggregates

A direct control may return a shared or exclusive reference to a grouped record,
variant, named outcome or option that itself contains references. Two contracts
apply: the outer reference borrows the carrier's storage, and the contained
references retain their external sources.

<!-- tellur:check -->
```tellur
struct text_view['source] {
    ref mutable string text <- 'source;
}

control lend(ref mutable text_view input)
    exits ready { ref mutable text_view[input['source]] value from input }
{
    exit ready { input };
}

control main() exits done {
    mutable string owner = "before";
    mutable text_view saved = text_view { borrow mutable owner };
    delta lend(borrow mutable saved) { ready { auto view } => editing }
    [editing]: deref view.text = "after";
    emit(view.text);
    drop(saved);
    owner = "available again";
    exit done;
}
```

Read the declaration from the inside out:

- `text_view[input['source]]` binds the view's contained source to the source carried
  by `input`. It does not mean the field points into the input record's storage.
- `ref mutable ... value from input` lends the record's storage exclusively.
- `auto view` therefore has type `ref mutable text_view`. Access through that
  reference remains subject to both contracts.

The brackets are optional when the body provides enough evidence to infer the
contained sources. `exits ready...` can infer the complete payload and both lifetime
layers. `forward` and immediate inline adapters preserve those layers. If several
inputs can supply the result, their inferred dependencies are conservatively united.
The complete origin schema remains required for the referenced aggregate, including
alternatives absent from this particular execution.

A shared outer reference grants only shared access, even when a contained field is
exclusive. Borrowed decisions on returned sums follow the same direct-reborrow rule
as borrowed inputs. Borrowing or returning a named outcome does not handle it; its
owner must still handle or transfer it.

Dropping, moving or replacing the carrier while its returned reference is still
needed is rejected, including explicit drop of a Copy carrier. A copied shared
field may outlive the carrier once the whole-carrier reference is no longer used;
an exclusive-field reborrow retains its authorizing carrier. External sources remain
protected until their last dependent reference ends.

These references may also be stored in local aggregates and captured outcomes, as
described below. Public containers use explicit schemas for each reference layer.
Trait results use the same explicit contracts. Reference rebinding and
reference-bearing loop state remain separate work.

Try [carrier_results.tellur](../playground/carrier_results.tellur). Declaration hover
shows the storage origin and contained origin groups; hover on `auto` shows the
inferred reference type.

### Storing references to reference-bearing aggregates

A local record, option, variant or outcome may store a reference to a carrier that
itself contains references. Storage retains the complete dependency tree: the
carrier's storage, its contained sources and any authorizing carrier guards.
Construction, moves, capture and stored adapters preserve this information.

<!-- tellur:check -->
```tellur
struct text_view['source] { ref mutable string text <- 'source; }
struct edit_job { ref mutable text_view target; }

control lend(ref mutable text_view input) exits ready... {
    exit ready { input };
}

control main() exits done {
    mutable string owner = "before";
    mutable text_view saved = text_view { borrow mutable owner };
    auto pending = capture lend(borrow mutable saved);

    // capture has already run lend; delta handles its completed result later.
    delta pending { ready { auto view } => received }
    [received]: deref view.text = "after capture";
    emit(view.text);

    edit_job job = edit_job { borrow mutable saved };
    deref job.target.text = "after stored job";
    emit(job.target.text);
    drop(saved);
    owner = "available again";
    exit done;
}
```

While `pending` is still needed, neither the `saved` carrier nor its external
`owner` can be moved, dropped or accessed incompatibly. Moving `pending` transfers
both dependencies and its outcome-handling obligation. Capture is synchronous;
it does not defer execution or create a task.

Local aggregates can nest multiple carrier-reference layers. Their origins need
not be declared because the compiler knows their construction. A borrowed local
sum can be inspected without consuming it, including a captured outcome; its owner
must still eventually handle it. Shared outer access never grants exclusive access
through a nested field. Whole sums with reference fields are safe to inspect even
when their current alternative is empty: no inactive payload address is read.

Shared references in ordinary records and variants can be copied or cloned when
the container's usual capabilities allow it. Every surviving copy retains its
sources. Captured outcomes retain their existing move-only handling rule. Exclusive
references move; a local wrapper does not make them copyable. A shared field copied
out of a carrier may outlive it; an exclusive-field reborrow keeps its authorizing
carrier protected. Unrelated stored fields can reach their last use independently.

Unpacking reconstructs the reference shape under the newly selected path. When
several producer paths stored the same alternative with different sources, the
compiler conservatively retains their union. It does not infer a correlation between
an earlier boolean branch and a later tag test. Borrowing local arm-owned storage
and retaining that borrow outside the arm is rejected at any nesting depth.

Generic wrappers can declare `ref t` or `ref mutable t` fields; direct reference
type arguments remain gated. Loop-local containers are recreated per iteration;
reference-bearing loop state and reference rebinding are not included.

Public containers also work when their declarations map every reference layer,
as described in [public nested-reference contracts](#public-nested-reference-contracts).

Try [stored_carriers.tellur](../playground/stored_carriers.tellur) for capture, a
nested local record, borrowed inspection and final consumption. Hover on `auto`
shows the inferred container or reference type.

### Public nested-reference contracts

A reference field has two distinct lifetimes when its referent itself contains
references. Brackets on the referent map its contained origins; the arrow after
the field names the referent's storage. Neither mapping replaces the other.

<!-- tellur:check -->
```tellur
struct text_view['text] { ref mutable string text <- 'text; }
struct edit_job['storage, 'text] {
    ref mutable text_view['text] target <- 'storage;
}
control prepare(ref mutable text_view input)
    exits ready { edit_job[input, input['text]] job }
{
    exit ready { edit_job { input } };
}
control relay(edit_job input)
    exits ready { edit_job[input['storage], input['text]] job }
{
    exit ready { input };
}
control main() exits done {
    mutable string owner = "before";
    mutable text_view saved = text_view { borrow mutable owner };
    delta prepare(borrow mutable saved) { ready { auto packed } => packed }
    [packed]: delta relay(packed) { ready { auto job } => ready }
    [ready]: deref job.target.text = "after";
    emit(job.target.text);
    drop(job);
    drop(saved);
    exit done;
}
```

- `edit_job['storage, 'text]` declares two independently mapped public groups.
- `text_view['text]` maps the view's inner text reference to the job's `'text` group.
- `target <- 'storage` separately maps the view object's storage lifetime.
- `edit_job[input, input['text]]` borrows the input view's storage and its external
  text source. `input['storage]` on an owned job selects the external view storage;
  it does not refer to the job's own allocation.

The same notation works in variant and named-outcome payload schemas. Multiple
finite reference layers compose by mapping each intermediate carrier's groups.
Whole owned and borrowed inputs/results, capture, forwarding and nested decisions
preserve all layers. An exit can use `ready...` to infer its payload and mappings;
the public data declaration still needs its complete schema. Empty alternatives
never cause reads of inactive reference payloads. Explicit mappings are required
when a result's origins cannot be inferred, including unproduced alternatives.

Shared outer access restricts inner access to shared reborrows, even when an inner
field's declared type is exclusive. A detached exclusive child retains every
carrier that authorizes it; an independent shared field may outlive its carrier.
Every surviving shared copy keeps its external sources borrowed. Lifetime unions
remain conservative and do not establish disjointness between unrelated calls.

`option<ref carrier>` has an implicit outer group `'source`. Its inner groups are
named by prefixing the carrier's groups with `referent_`, in declaration order.
For example, `option<ref text_view>` has `['source, 'referent_text]`; a returned
mapping uses `option<ref text_view>[input, input['text]]`, and an owned parameter
can expose them as `value['source]` and `value['referent_text]`. Further option
wrapping preserves these groups. Explicit named wrappers can give them domain
specific names instead.

When a call returns nested contained origins from a constructed aggregate argument,
bind that aggregate to a local variable first. Reference-bearing type arguments,
abstract origin-shape requirements on generic or associated types,
reference rebinding and reference-bearing loop state remain separate
work. Generic layouts with statically known carrier schemas do work. This feature
does not add `ref ref` types.

Try [public_carriers.tellur](../playground/public_carriers.tellur). Declaration hover
shows both mappings; `auto` hover reports the resulting container/reference type.

### Grouped struct results: `['a]`, `<-` and `[input]`

<!-- tellur:check -->
```tellur
struct text_pair['a, 'b] {
    ref string left <- 'a;
    ref string right <- 'b;
}

control pair(ref string a, ref string b)
    exits ready { text_pair[a, b] value }
{
    exit ready { text_pair { left: a, right: b } };
}
```

| Form | Exact role |
| --- | --- |
| `['a, 'b]` after the struct name/type parameters | Declares public borrowing groups in a fixed order |
| `ref string left <- 'a;` | Assigns a reference field to a declared group; no value assignment occurs |
| `text_pair[a, b] value` in a result contract | Binds those groups positionally to reference input parameters |
| `text_pair { left: a, right: b }` | Constructs the value with its actual references |
| `ready { text_pair words }` in a decision | Receives the value and its checked dependencies; no brackets needed |

`exits ready...` or `exits auto` can infer the result's type and dependencies.
An explicit result type without bracket arguments also infers its dependencies;
an unproduced result needs explicit bindings. A local variable or owned parameter's
storage cannot escape through either form. The returned struct is owned by the
caller, while its references still borrow the original inputs.

Each group may contain multiple fields. At a control boundary those fields share a
combined set of possible dependencies. Distinct groups remain independent. After
`words.left`'s last use, its source becomes available if no other reference retains
it, even when `words.right` is used later. Explicitly copying a shared field keeps
that field's source borrowed; moving an exclusive field transfers its permission.

Nested schemas compose: `text_pair['a, 'b] parts;` maps a nested field's groups into
its enclosing struct. Type arguments remain separate: `view<string>['a] title;`.
A grouped declaration must cover all reference leaves, and every declared group
must be used. Ungrouped reference structs continue to work locally, but cannot be
returned as whole values. Schema validation also checks unused generic declarations.

Explicit input anchors hide the pointee's exact projection and protect the passed
subobject. Multiple exclusive results may share that anchor when their construction
establishes disjoint access. Inferred projections retain their existing precision.
Origin groups describe dependencies; they do not manufacture access permissions.

Borrowed-record trait results require explicit origin mappings. Local shared-reference collection elements are supported; exclusive elements and
public collection-origin contracts remain separate stages.
Local captured borrowed outcomes are supported. See [origin groups in depth](precise-origins.md)
and [the runnable example](../playground/origin_groups.tellur).

### Record inputs and contained origins: `input['group]`

_Context fragment: Uses the text_pair declaration in the preceding section._

<!-- tellur:fragment Uses the text_pair declaration in the preceding section. -->
```tellur
control first(text_pair input)
    exits found { ref string value from input['a] }
{
    exit found { input.left };
}

control relay(text_pair input)
    exits ready { text_pair[input['a], input['b]] value }
{
    exit ready { input };
}
```

A whole record parameter uses its ordinary nominal type, without repeated origin
arguments. At each invocation the compiler instantiates the public groups using
the actual argument's contained loans. Copy records copy; other records move,
including records containing exclusive references. Generic and nested schemas use
the same rules. Explicit contracts are checked against the body; `ready...` and
`exits auto` can infer the dependencies.

`input['a]` selects the **public origin group** `'a`, never a field named `a`.
The group may cover private fields. Renaming those fields does not change the
contract. A result group can bind to a contained selector, as in `relay` above.
Unknown groups are errors. Bracket expressions here are static contracts, not
runtime indexing or value expressions.

For a borrowed carrier, `from input` anchors a reference into the record's own
storage. `from input['a]` anchors an external referent carried by the record:

_Context fragment: Uses the text_pair declaration above._

<!-- tellur:fragment Uses the text_pair declaration above. -->
```tellur
control first_borrowed(ref text_pair input)
    exits found { ref string value from input['a] }
{
    exit found { input.left };
}
```

A copied shared contained reference may outlive its carrier; its external owner
must still remain valid. In contrast, returning a reference to an owned field
keeps the borrowed carrier alive. Borrowing an owned parameter's own storage and
returning that borrow remains an error. Both shared and exclusive carrier borrows
are supported, including carriers with exclusive reference fields. Returning a
reference to an entire reference-bearing carrier preserves its storage and contained
source contracts, as described in [carrier results](#returning-references-to-reference-bearing-aggregates).

Separate groups retain separate dependencies; one group intentionally combines
its possible sources. Live carrier borrows retain their contained loans. Call alias
checks include those loans and protect earlier arguments during later argument
evaluation. Copies, moves, forwarding and immediate adapters preserve dependencies.

Try [record_inputs.tellur](../playground/record_inputs.tellur).

### Reborrowing exclusive fields through a carrier

<!-- tellur:check -->
```tellur
struct editor['source] {
    ref mutable string text <- 'source;
}

control read_editor(ref editor input)
    exits found { ref string text from input['source] }
{
    exit found { borrow deref input.text };
}

control edit(ref mutable editor input)
    exits found { ref mutable string text from input['source] }
{
    exit found { borrow mutable deref input.text };
}
```

The shared carrier in `read_editor` permits reading or shared reborrowing. It cannot
mutate through its exclusive field or grant an exclusive reference. `edit` has
an exclusive carrier borrow and may reborrow the stored exclusive permission.
Moving a non-Copy field out of either borrowed carrier is forbidden: a declaration
such as `ref mutable string taken = input.text;` attempts a move. Write the explicit
reborrow form instead. Passing a reference field to a borrowed parameter reborrows
it, following the same rules as standalone reference arguments.

For an exclusive contained field, **`from input['source]` also retains its
authorizing carrier storage**. This applies to both shared and exclusive child
references, to inferred contracts, and to reference fields in returned records.
A child cannot outlive that carrier. Moving/copying a child preserves the dependency;
last use releases it. This differs from copying an already-shared reference field,
which can outlive its carrier as described above.

A shared child blocks parent mutation and permission transfer while it remains
live. An exclusive child also blocks conflicting parent reads. Disjoint fields
with separate groups can be reborrowed together; each parent field becomes available
again after its own child's last use. Borrowing a whole carrier checks all contained
permissions, including live children and other arguments to the same call.
Multiple exclusive fields may also share a combined group. Their permissions remain
separate, but each retains the group's combined lifetime dependencies and any carrier
storage guards; that can delay parent restoration compared with separate groups.

Hover reports when a result keeps an authorizing carrier borrow alive, without
exposing private field mappings. Generic/nested schemas, module boundaries,
forwarding, immediate adapters and borrowed collection operations preserve this
information. The historical `region` spelling cannot duplicate exclusive carrier
permissions either.

See [carrier_reborrows.tellur](../playground/carrier_reborrows.tellur) for parent access
being restored independently for two fields.

### Splitting an opaque owner

_Context fragment: Uses a document struct with string title and body fields._

<!-- tellur:fragment Uses a document struct with string title and body fields. -->
```tellur
control edit(ref mutable document input)
    exits ready {
        ref mutable string title from input,
        ref mutable string body from input
    }
{
    exit ready { borrow mutable input.title, borrow mutable input.body };
}
```

The caller receives two independent exclusive permissions. It can mutate them,
pass them together, move them, or reborrow either one. The body must prove that
both can coexist: returning `input.title` twice is an error. An inferred exit or
a grouped record result provides the same checked guarantee. Separate execution
paths may swap which field becomes which result, provided each exit is valid.

Both results still retain the whole passed `input` subobject. The caller cannot
access that owner until all retaining children reach their last use. Finishing the
`title` child alone does not reveal which private owner field is available. Local
field projections through a returned record reference can split that reference's
access permission, while retaining its opaque owner anchor.

There is no `disjoint` assertion or runtime permission object. The compiler checks
exit construction and propagates fresh, separate result permissions through calls;
shared references still cannot grant mutable access. Trait signatures may promise
this result shape, and every implementation must satisfy it. Independent calls do
not automatically produce disjoint references to the same owner.

See [opaque_splits.tellur](../playground/opaque_splits.tellur).

### Declared result origins: `from`

An exit payload declaration has the form `type field_name`, optionally followed by
`from parameter_name` for a reference payload:

<!-- tellur:check -->
```tellur
control echo(ref string input)
    exits found { ref string text from input } {
    exit found { input };
}
```

Read the signature as: “`echo` borrows a string as `input` and may exit through
`found`, carrying a shared string reference named `text` that borrows from `input`.”

- `found` is the outcome label.
- `ref string` is the payload field's type.
- `text` names the field in the exit contract. It does not declare a local variable
  inside the control or evaluate an expression.
- `from input` names this control's input parameter and constrains the returned
  reference's origin. It does not fetch a value, assign the field, clone anything,
  or itself produce an exit.
- `exit found { input };` is the executable statement that supplies the actual
  payload, positionally, and exits the control.

The caller declares its own binding, for example
`found { ref string received } => available` in a delta arm. Neither the caller's
binding name nor the contract field name must match the input parameter's name.

The same spelling may occupy both roles:

<!-- tellur:check -->
```tellur
control echo(ref string x) exits found { ref string x from x } {
    exit found { x };
}
```

Here the first `x` in the payload declaration names the result field; the second
`x`, after `from`, resolves to the input parameter. In the body, `x` still names
that input parameter. Distinct names are usually easier to read. `{ x from x }`
alone is not a complete payload declaration: it is missing the type or field name.
`control echo()` has no input parameter that a `from` clause could name.

The returned reference can point to the input's referent or to a field inside it;
the pointee type need not equal the input's pointee type. While the result remains
live, the caller must keep its underlying owner alive and respect the shared or
exclusive borrow. The compiler checks this ownership relationship; the annotation
does not extend the owner's lifetime.

Trait signatures use the same declaration:

<!-- tellur:check -->
```tellur
trait reader {
    type item;
    control peek(ref self input) exits found { ref item value from input }, empty;
}
```

A plain input anchor must be a borrowed parameter; a contained selector may also
name an owned grouped record parameter. An exclusive result requires an exclusive
input. The body must return a reference derived from that input. Trait declarations
and implementations require explicit origins for borrowed payloads and match them
by parameter position, so local parameter names may differ. Generic controls can
use the same syntax with `ref reader<r>.item`. Ordinary controls may keep inferred
origins instead.

A declared origin protects the whole passed subobject and hides private field layout.
Projecting the result does not narrow that owner anchor. Multiple outputs, including
exclusive ones, may name the same input. Every implementation must construct them
without exclusive aliasing; shared outputs may alias each other. The same rule applies
to trait implementations and generic calls. Declared unions and field-specific anchors
are not yet supported; inferred direct-control contracts retain their existing rules.
Try [borrowed_traits.tellur](../playground/borrowed_traits.tellur).

`borrow deref reference` creates a shared reborrow; `borrow mutable deref reference` creates an exclusive
one when the parent is exclusive. Reborrows and their copies keep the underlying
owner protected. A live shared reborrow excludes mutation through its parent;
an exclusive reborrow excludes overlapping parent reads too. Two distinct record
fields can be returned and mutated independently.

Local lifetimes are shortened to last use. An explicit `drop(reference)` still ends
a loan early and makes later use invalid, but it is usually unnecessary. The analysis
is conservative across selected histories: a later syntactic use may retain a loan
even where a more precise analysis could release it. Outer-loop references stay live
through backedges; they cannot be moved or explicitly dropped by a repeating body.
Loop-local references can end before transferring an owner into the next iteration.

Arguments evaluate left to right. An exclusive borrow passed to a call also excludes
reads in later arguments while the borrow is live. Taking a snapshot first is allowed:
`operation(clone(borrow owner), borrow mutable owner)` can be valid when those are its parameter
types. Neither shared nor exclusive references permit moving a non-Copy referent out.

References have real storage addresses in the interpreter and native backend; shared
calls no longer copy their input data. The address representation is internal and
cannot be supplied in scenario JSON or through CLI arguments. Entry controls therefore
take owned/Copy values, not references. Stable local aliases can still be erased during
lowering. The current address storage supports the sequential, nonrecursive language;
concurrent invocations will require separate coroutine activation storage.

Remaining restrictions include abstract origin-shape bounds, exclusive/nested
collection elements, reference rebinding, reference loop parameters and references
to references. Local reference-bearing records and nested tagged values are supported. Reference binding names must be unique within a lexical scope,
including declarations on disjoint paths. These limitations are diagnosed; this is not the full Rust
borrow model. See `playground/references.tellur` for a complete executable example.

### Aggregate trait origin contracts

Trait methods declare the origin relationships their callers may rely on. In 0.42,
these include grouped structs, variants, named outcomes, builtin options and nested
carrier references. A result's bracket arguments are mandatory when its type contains
references; a direct reference also needs its storage contract, `from input` or
`from input['group]`. Implementation bodies prove those declarations.

<!-- tellur:check -->
```tellur
struct text_view['text] { ref string text <- 'text; }
trait reader {
    control view(ref string input) exits ready { text_view[input] value };
}
implement reader for string {
    control view(ref string owner) exits ready { text_view[owner] result } {
        exit ready { text_view { owner } };
    }
}
control inspect<r: reader>(ref string input)
    exits ready { text_view[input] value }
{
    forward reader<r>.view(input);
}
control main() exits done {
    string text = "hello";
    delta inspect<string>(borrow text) { ready { auto view } => available }
    [available]: emit(view.text); exit done;
}
```

Here `input` and `owner` name the same parameter position. Parameter and result
variable names may differ, and exit labels may be reordered. Parameter ownership,
result types, storage origins and every contained origin group must agree exactly.
An implementation cannot swap groups, choose another input, or omit mappings and
ask the compiler to infer its public trait contract. Private ordinary controls may still
infer these relationships from their bodies.

This unused implementation is rejected because it changes the source promised by
the trait, even though its body agrees with its own declaration:

<!-- tellur:reject E_ORIGIN -->
```tellur
struct text_view['text] { ref string text <- 'text; }
trait reader {
    control view(ref string first, ref string second)
        exits ready { text_view[first] value };
}
implement reader for string {
    control view(ref string a, ref string b)
        exits ready { text_view[b] value }
    {
        exit ready { text_view { b } };
    }
}
```

A named completion schema uses the same bindings, for example
`exits preparation<item>[input, input['text]]`. Its mappings are resolved before
conformance checking. A reference to a grouped carrier declares both layers:
`exits ready { ref text_view[input['text]] value from input }`.
The storage lifetime and the text's external source remain distinct.

Generic bodies are checked against signature-only witnesses, including when never
called. A witness carries the declared origins, not the details of today's concrete
implementation. Concrete implementations are checked independently. Capture,
forwarding and certified expansion preserve these contracts.

Known schemas may contain associated payload types, such as `text_view<item>` with
a declared reference field. An opaque associated type does not itself expose an
origin schema: `type item;` alone cannot promise that `item` contains a group `'a`.
Origin-shape bounds for associated and generic types are future work. This release
does not add runtime dispatch, general reference type arguments, public borrowed-collection
contracts or reference rebinding.

Try [aggregate_traits.tellur](../playground/aggregate_traits.tellur) for a complete
nested, exclusive example with an associated type, named completion, generic helper
and captured result. Hover `auto job` to see its concrete inferred type.

## Arrays, vectors and maps

<!-- tellur:body -->
```tellur
array<u32, 3> numbers = [10, 20, 30];
mutable vector<string> words = ["hello", "path"];
mutable map<string, u32> counts = map {};
auto snapshot = clone(borrow words);
u32 size = vector_length(borrow words);
```

An array has a fixed compile-time length; a vector grows or shrinks. List literals
need a type from a declaration, parameter, assignment or explicit exit contract.
`auto words = [];` is ambiguous and rejected; `auto words = existing_vector;` works
and moves the existing vector. Nested collections work, for example
`vector<array<u32, 2>> pairs = [[1, 2], [3, 4]];`.

An array is Copy exactly when its element type is Copy. Vectors and maps always move.
`clone(borrow collection)` recursively clones ordinary values; it rejects collections of
must-complete resources. Maps start with `map {}` and are populated with insertion
calls, making replacement explicit. Map keys are `bool`, `u32` or `string` and use
value equality. Iteration order is ascending key order: false before true, numeric
order for u32, and lexicographic UTF-8 order for strings, without locale collation.

Collection operations are compiler builtins and need no JSON primitive declaration.
The first argument is a named collection, projected place, or reference. Reads use
`borrow values`; changes use `borrow mutable values` and require a mutable binding or exclusive
reference. `_length` calls are total expressions returning u32. Other operations
use `delta` and require every listed outcome to be handled:

| Operation | Arguments after the collection | Outcomes and payloads |
| --- | --- | --- |
| `array_get`, `vector_get` | `u32 index` | `found { ref t value }`, `out_of_bounds` |
| `array_get_mut`, `vector_get_mut` | `u32 index` | `found { ref mutable t value }`, `out_of_bounds` |
| `array_replace`, `vector_replace` | `u32 index, t value` | `replaced { t previous }`, `out_of_bounds { t rejected }` |
| `vector_push` | `t value` | `pushed` |
| `vector_pop` | — | `item { t value }`, `empty` |
| `vector_remove` | `u32 index` | `item { t value }`, `out_of_bounds` |
| `map_get`, `map_get_mut` | shared `borrow key` | `found { ref v value }` / `found { ref mutable v value }`, `missing` |
| `map_insert` | `k key, v value` | `inserted`, `replaced { v previous }` |
| `map_remove` | shared `borrow key` | `removed { v value }`, `missing` |
| `vector_take` | —; consumes vector | `item { t value, vector<t> rest }`, `empty` |
| `map_take` | —; consumes map | `item { k key, v value, map<k, v> rest }`, `empty` |
| `array_into_vector` | —; consumes array | `converted { vector<t> values }` |

For direct shared-reference elements (`t = ref u`), both `_get` and `_get_mut`
return `ref u`, rather than adding another reference layer. See the local borrowed
collection section below.

`vector_pop` removes the last element; `vector_take` and `vector_remove(..., 0)`
remove the first. `map_take` removes the smallest key. `map_insert` retains the
existing key when replacing an equal key and returns the previous value. Replacement
outside an array/vector's bounds returns the supplied value unchanged. There is no
indexing syntax with an implicit panic.

<!-- tellur:tail -->
```tellur
[entry]:
    mutable vector<string> words = ["first"];
    delta vector_get(borrow words, 0) {
        found { ref string word } => readable,
        out_of_bounds => outside
    }
[readable]: emit(word);
[readable | outside]:
    delta vector_push(borrow mutable words, "next") { pushed => extended }
[extended]: exit done { words };
```

References into owned elements conservatively borrow the **whole collection**, even for a constant
index. Two exclusive element borrows are therefore rejected while both are live.
Mutating, removing, inserting or moving the collection requires existing conflicting
borrows to have ended. Shared element borrows can coexist. Helpers can return element
borrows derived from their input references; hover describes the source as
`values[*]`. Borrowing through nested collections of owned values follows the same rules.
Collections that store shared references use the direct-reference rules below.

A collection containing resources is one sealed completion obligation. Removing an
element transfers its obligation to the result; the remaining collection still needs
to be drained or transferred. Even an empty `vector<handle>` must pass through
`vector_take`'s `empty` outcome (or be transferred), because emptiness is not a static
capability. Use `vector_take`/`map_take` in a loop and explicitly close or transfer each
resource. Ordinary empty/nonempty collections may simply leave scope. Resource arrays
can be converted into vectors for draining. Assignment through a resource element
reference cannot discard the old resource; use replacement and handle its result.

The current map implementation uses sorted flat storage, so lookup/insertion/removal
are linear in entry count. Vector removal at the front is linear too; consuming drain
loops can consequently be quadratic. These are reference-implementation costs, not
promised permanent complexity. Allocation exhaustion is a host failure, not a
recoverable Tellur outcome; length is limited to u32. Borrowed slices, iterators, custom
key ordering, collection literals with inferred element types, exclusive-reference
elements and public collection-origin contracts remain future work. See `playground/collections.tellur` for a complete
runnable example.

### Local collections of shared references

Since 0.43, local `array<ref t, n>`, `vector<ref t>` and `map<key, ref t>` values
can retain references to external owners. The referent type `t` must have an owning
layout without stored references. Map keys remain `bool`, `u32` or `string`.

<!-- tellur:check -->
```tellur
control main() exits done {
    mutable string owner = "hello";
    mutable vector<ref string> views = [borrow owner];
    delta vector_get(borrow views, 0) {
        found { auto text } => found,
        out_of_bounds => missing
    }
    [found]: drop(views); emit(text);
    [missing]: drop(views);
    [entry]: owner = "released"; exit done;
}
```

`text` has type `ref string`. Lookup copies the stored shared reference; it does not
borrow the collection's allocation or copy the string. It can therefore survive
removal, growth or dropping the collection, provided its external owner remains live.
`_get_mut` still requires exclusive collection access but returns the same shared
reference: it grants no permission to change the referent or rebind the stored
reference. Use `_replace` or `map_insert` to replace an entry explicitly.

Insertion retains the supplied source on successful outcomes. Replacement returns
the old reference; an out-of-bounds replacement returns the supplied reference.
Removal/pop transfer the returned reference's dependencies. `vector_take`, `map_take`
and `array_into_vector` transfer dependencies to their returned collections as well.
Shared-reference arrays are Copy. Vectors/maps still move; explicit cloning copies
the reference values and gives the clone its own source retainers.

Indices and keys currently use a conservative union of possible sources, including
constant indices. A removed reference may retain every candidate source, and a
remaining collection retains its previous candidates until its last use or explicit
`drop`. The `empty` outcome of `vector_pop` proves that collection has no elements
and releases its retainers on that branch. Other surviving copies or returned
references keep their own dependencies.

<!-- tellur:reject E_LOAN -->
```tellur
control main() exits done {
    mutable string owner = "hello";
    mutable vector<ref string> views = [borrow owner];
    delta vector_pop(borrow mutable views) {
        item { auto text } => item,
        empty => empty
    }
    [item]: emit(text); owner = "too soon";
    [item]: emit(u32_text(vector_length(borrow views)));
    [entry]: exit done;
}
```

This is deliberately conservative even though a human can see the one-element
vector became empty. Dropping `views` before changing `owner` makes that branch valid.
Insertion cannot retain an owner from an inner scope in an outer collection.
Loop-local collections work; mutating an outer borrowed collection inside a loop
awaits loop origin state.

This stage supports direct local collection values and local aliases. Exclusive
reference elements, reference-bearing aggregate elements, collections nested inside
other storage/outcomes, and collection parameters/results need subsequent origin
and permission contracts. A control may use a local collection to return a selected
reference with an ordinary `from input` contract. Generic bodies can build local
`vector<ref t>` values without admitting reference type arguments to user generics.

Try [borrowed_collections.tellur](../playground/borrowed_collections.tellur).

## Must-complete resources

A manifest type marked `linear: true` must be consumed or transferred on every
selected exit/restart, even though ordinary owning values may be dropped. There is no
implicit fallible cleanup. `drop` and `clone` reject resource-containing values, and
assignment cannot discard an outstanding resource. Resource-containing records track
each field; variants/options are sealed obligations that transfer to the active
payload when decided. Constant empty variants do not erase their type's obligation.

_Context fragment: Inside a file-handling control after its opened path has bound file._

<!-- tellur:fragment Inside a file-handling control after its opened path has bound file. -->
```tellur
[opened]:
    delta read(borrow file) {
        data { bytes contents } => read_ok,
        failed { read_error problem } => read_failed,
    }
[opened]:
    delta close(file) {
        closed => closed,
        failed { close_error problem } => close_failed,
    }
```

The owner survives `read` and is consumed by `close` on either completion. A stored
reference to `file` must reach its last use or be explicitly released before closing it. Borrow/move conflicts across
arguments of one invocation are also rejected. Resource-containing temporaries must
be bound before borrowing or projecting so their remaining obligations are tracked.

## Loops

<!-- tellur:tail -->
```tellur
loop count_up(u32 number = 1) {
    [entry]: delta (number <= 5) { true => more, false => finished }
    [more]:
        emit(u32_text(number));
        delta checked_add(number, 1) {
            sum { u32 next } => advanced,
            overflow => too_large,
        }
    [advanced]: again count_up(next);
    [finished]: exit done;
    [too_large]: exit arithmetic_fault;
}
```

A tail loop is the last item in its scope. Initializers and restart arguments transfer
simultaneously; each iteration has fresh local bindings and facts. An outer owning
value consumed by the body must enter through a loop parameter. Outer references
cannot be moved or explicitly released by a repeating body. Nested source loops and recursive calls remain
unsupported, but a control may call another control containing a loop. Infinite
execution is possible; there is no termination proof.

## Rust adapters and host execution

The [maintained settings application](../applications/settings/README.md) now
provides both a persistent CLI and a bounded, sequential loopback HTTP service.
Its [server](../applications/settings/server.tellur) is a larger example of tail
loops, captures, independent failures, explicit completion and public entry contracts.
Its [HTTP policy](../applications/settings/http.tellur) composes the same settings
module used by the CLI. This is synchronous execution; first-class tasks remain planned.

Since 0.45, a project manifest can bind primitive operations to typed Rust wrappers
and explicitly locked Cargo dependencies. Tellur checks calls against the manifest;
the native build generates Rust outcome enums and checks each wrapper signature.
Supported data includes owned scalars, strings, bytes, vectors and maps, plus shared
input borrows that last for the call. Since 0.49, `rust.resources` binds opaque
must-complete types to retained Rust owners. Shared calls receive `&T`; consuming
calls receive `T`, and resource payloads return ownership under a fresh identity.
The same explicit completion and borrow/move rules apply as for other resources.
Returned foreign borrows, exclusive foreign mutation, callbacks and asynchronous
adapters remain outside this profile. See the runnable
[retained-file example](../examples/retained_file/main.tellur).

Each binding declares `immediate` or `blocking` execution. Neither suspends a Tellur
control. This preserves a separate boundary for future tasks and LLVM coroutine
lowering. Rust panics are not recoverable outcome alternatives. Checking and editor
diagnostics do not execute adapters; native builds compile them, and reference or
native scenario runs use scripts instead of live Rust dispatch.

See [Rust adapter contracts and build workflow](rust-adapters.md) and the maintained
[settings application](../applications/settings/README.md). The settings module
contains the domain policy; Rust supplies file and JSON mechanisms. Normal builds
require the committed dependency lock; `tellurc lock-native` is the explicit action
for updating it. This is source interoperability, not a stable Rust binary ABI.

## File modules and private representations

_Context fragment: Import syntax; see the linked modules playground for the complete file graph._

<!-- tellur:fragment Import syntax; see the linked modules playground for the complete file graph. -->
```tellur
import "people.tellur" as people;
```

Import paths are relative to the file containing the import. Qualify public members
through the chosen namespace: `people.person`, `people.create(...)`. Declarations
are private unless marked `pub`; struct fields require their own `pub` modifier.
A public struct with private fields is constructed through its module's controls.
Public controls require [explicit exit types and borrowed-result origins](#explicit-public-contracts).
Public interfaces cannot expose private types. A public control naming an outcome
contract in `exits` must expose a public contract, even when all payload types are
public. This also applies to generic and grouped completion contracts. Variants/outcomes export their whole
alternative contract when public.

Open `playground/modules/main.tellur` and use the normal build/run commands. The CLI
loads the complete graph and the LSP observes unsaved imported buffers. See
[file modules](modules.md) for identity, visibility, provenance and current limits.

## Generic data

Structs, ordinary variants and named outcomes can have first-order type parameters:

<!-- tellur:check -->
```tellur
struct box<item> { item value; }
variant optional<item> { some { item value }, none }
outcome lookup<item, error> {
    found { item value }, missing, failed { error cause }
}

control wrap(string text) exits lookup<box<string>, u32> {
    exit found { box<string> { value: text } };
}
```

Arguments are explicit on types and constructors: `box<string> { value: "Ada" }`,
`optional<u32>.some { 7 }`, and `lookup<string, u32> saved = capture first(borrow words);`.
Both named and positional struct construction work. Generic variant constructors
require qualification and never inject short constructor names into other code. Match the selected variant or
outcome using its ordinary tag (`some`, `found`, and so on). Constructor type-argument
inference, bounds on data parameters and higher-kinded parameters are not
implemented. Trait bounds on control parameters are supported separately.

Different argument lists produce distinct nominal types. `box<u32>` can be copied;
`box<string>` moves, and `box<handle>` retains the handle's completion obligation.
An outcome specialization always requires handling or transfer. Properties derive
from stored fields: a parameter unused by any field does not itself store a value.
Type parameters are lexical names and may shadow ordinary type declarations;
builtin type names and duplicate parameters are rejected.

Unused templates are checked for names, arity, duplicate fields/alternatives and
recursive declarations. Each used concrete layout is then validated in full,
including collection restrictions: `map<key, value>` still requires a supported
primitive key. Reference fields in generic records and local tagged storage are
supported; their public boundary restrictions still apply. Recursive declarations
remain unsupported. Data layout specialization does not substitute for checking a
generic control body under declared capabilities. Unconstrained generic controls
are checked separately as described below.

Imports qualify the type constructor: `data.box<string>`. Aliases of one file share
specializations; different files retain distinct identities. Field privacy and public
interface restrictions apply to the generic type and its arguments. Specialization
limits (1,024 layouts, depth 64, type spelling 16,384 bytes) produce errors rather than
partial programs. See [the runnable example](../playground/generic_data.tellur).

## Concrete traits and associated types

Trait implementations use `implement trait_name for type_name`; the earlier `impl`
spelling remains accepted. Associated types still use `type name = concrete_type;`.

A trait declares required controls and associated types. Implementations are explicit;
there is no inherited storage or implicit conversion between structs. `type item;`
declares a type slot chosen by each implementation, not a field stored on the
implementor. `type item = string;` supplies that choice; it performs no runtime
assignment and is not a general top-level type-alias declaration. You may call the
slot `t_item` as a naming convention; the prefix has no compiler meaning.

`self` denotes the implementing type within the trait or implementation. Operations
receive their input through explicitly declared parameters; there is no hidden
receiver. A trait cannot require that an implementor store a particular field, but
it can require an operation that provides access to data. Associated bounds such
as `type item: clone;` constrain the chosen item type, not the implementor itself.

<!-- tellur:check -->
```tellur
trait reader {
    type item;
    control read(ref self input) exits ready { item value };
}

implement reader for string {
    type item = string;
    control read(ref self input) exits ready { item value } {
        exit ready { clone(input) };
    }
}

control main() exits auto {
    string source = "Ada";
    reader<string>.item expected = "Ada";
    exit delta reader<string>.read(borrow source) {
        ready { auto text } => done { text == expected }
    };
}
```

`reader<string>.item` selects an associated type; `reader<string>.read(...)`
selects a control. The angle brackets identify the implementing type. Calls dispatch
statically and participate in normal ownership, path coverage, forwarding, capture
and expansion checks. Associated types may also appear inside generic type arguments.
`self` and unqualified associated names are lexical within a trait or implementation.

Supply every declared member exactly once. Trait methods and implementations require
explicit exit contracts and exact parameter ownership modes and exit payload types.
Exit-label order and local parameter names may differ. Extra exits require an explicit
adapter; conformance does not discard them. Implementations are checked even if unused.

A module may implement a trait only if it owns that trait or the outer nominal target
type. A foreign container holding a local type is still foreign. Duplicate trait/type
pairs are errors, including across imports. Visibility applies to both trait and type;
implementations retain their defining module's field access. Public trait/type pairs
cannot expose private associated types.

Concrete owned targets, associated assignments and static calls are implemented.
Trait-constrained generic controls and associated sealed capability bounds are also
implemented (below). Generic/blanket implementations, default methods and runtime
polymorphism remain planned. Borrowed
inputs and results are supported. Direct reference results require a storage origin;
grouped results require explicit bracket mappings. See
[aggregate trait origin contracts](#aggregate-trait-origin-contracts). Associated
reference types remain unsupported. Try [the trait example](../playground/traits.tellur).

## Generic controls

Controls can declare first-order type parameters and call one another with explicit
arguments:

<!-- tellur:check -->
```tellur
control identity<item>(item value) exits auto {
    exit delivered { value };
}

control main() exits auto {
    forward identity<string>("Ada");
}
```

Every generic definition is checked even if unused. Unconstrained type parameters are
represented by distinct opaque, must-complete types during that check. Consequently an
unconstrained value may be borrowed or transferred, but cannot be copied, cloned,
implicitly discarded, emitted as text, or inspected as a particular primitive or
record. Calling an invalid definition only with `u32` does not make it valid. Two
parameters remain distinct even when a particular caller supplies the same type twice.
Generic controls can request explicit capabilities:

<!-- tellur:check -->
```tellur
control duplicate<item: clone>(ref item value) exits ready { item value } {
    exit ready { clone(value) };
}
control discard<item: drop>(item value) exits done { exit done; }
control repeat<item: copy>(item value) exits ready { item first, item second } {
    exit ready { value, value };
}
```

| Bound | Permission |
| --- | --- |
| `copy` | Implicit duplication, explicit cloning and ordinary disposal |
| `clone` | Explicit `clone(borrow value)` and ordinary disposal; normal value use still moves |
| `drop` | Ordinary disposal; neither copying nor cloning is promised |

`copy` implies `clone`, which implies `drop`. Combine requirements with `+`, as in
`item: clone + drop`, though this particular combination is redundant. These are
compiler-checked permissions, derived through struct fields, variant payloads and
collection elements. A `vector<u32>` can be cloned but is not Copy; an
`array<u32, 2>` is Copy. Resources and outcomes cannot acquire permission to abandon
outstanding obligations. Generic calls check these requirements even for unused type
parameters, and a generic body has no permissions beyond its bounds.

`drop` does not define an implicit cleanup operation, and `clone` cannot hide a
failing or effectful user operation. Such operations belong to explicit control
contracts. Bounds on data declarations remain planned.
Generic declarations use `control`, whose owning-value rules apply; legacy `region`
syntax is limited to nongeneric definitions. Try `playground/capabilities.tellur`.

Trait requirements use the same bound list. Associated types can promise sealed
permissions:

<!-- tellur:check -->
```tellur
trait reader {
    type item: clone;
    control read(ref self input) exits ready { item value };
}
control read_one<r: reader>(ref r input) exits ready { reader<r>.item value } {
    forward reader<r>.read(input);
}
```

`r: reader + clone` requires both a coherent `reader` implementation and permission
to clone `r`. `type item: clone` grants cloning of the associated item, not of `r`.
The compiler checks these promises for every implementation and type argument,
including unused parameters. Generic bodies use only declared operations and
permissions and are checked even when never instantiated. Concrete type arguments
do not make invalid generic bodies valid. Distinct traits' associated types remain
distinct in a generic definition even if a particular pair of implementations chooses
the same concrete type. Associated trait bounds and equality constraints remain outside
this profile. Try [the generic trait example](../playground/trait_bounds.tellur).

`exits auto` produces a checked parametric contract, shown in hover and Outline.
Named generic outcome contracts work as well. Concrete calls are specialized and
rechecked, including ordinary data layout constraints. Different file aliases share
one control identity. A specialization retains its defining module's access rights;
callers cannot use specialization to reach a private control or field.

Borrowed parameters and immediate borrowed exits retain the existing lifetime rules:
`control view<item>(ref item value) exits found { ref item value } { exit found { value }; }`.
Grouped reference-bearing records and source variants also cross generic control
boundaries. Ungrouped sums, builtin options and whole completed outcomes retain
their local-storage restriction. Reference types as generic
arguments, higher-kinded parameters,
recursive control calls and type-argument inference remain unsupported. Type parameters
represent owned types; write `ref item` or `ref mutable item` at the borrowing boundary.

Producer-indexed captures can spell the concrete producer, for example
`outcome<identity<string>> saved = capture identity<string>("Ada");`.
A non-generic `main` is the entry point for trying generic calls. `check` reports
concrete controls and checked generic definitions separately. The specialization
budget includes the abstract checking instances: at most 1,024 per checking pass,
with depth 64 and type spelling 16,384 bytes. Exhaustion is an error.

Certified graph expansion handles concrete generic instances. `extract-tail` can
extract from a non-generic caller using generic calls; selecting a generic template
itself is currently rejected. See [the runnable example](../playground/generic_controls.tellur).

## Live primitive library

The playground manifest uses these snake_case contracts. Payload types are shown;
parameter names are omitted. Other manifest primitives need a live adapter or a script.

| Operation | Outcomes / expression value |
| --- | --- |
| `parse_u32(string)` | `number { u32 }`, `malformed`, `overflow` |
| `checked_add(u32, u32)` | `sum { u32 }`, `overflow` |
| `checked_sub(u32, u32)` | `difference { u32 }`, `underflow` |
| `checked_mul(u32, u32)` | `product { u32 }`, `overflow` |
| `checked_div(u32, u32)` | `quotient { u32 }`, `zero_divisor` |
| `u32_text(u32)` | `string` expression; `text { string }` in delta |
| `concat(string, string)` | `string` expression; `text { string }` in delta |
| `decode_utf8(bytes)` | `text { string }`, `invalid` |
| `open(string)` | `opened { handle }`, `not_found`, `denied` |
| `read(borrow handle)` (borrows) | `data { bytes }`, `failed { read_error }` |
| `close(handle)` (consumes) | `closed`, `failed { close_error }` |

`read` reads the remaining file bytes. `close` consumes on either outcome. Error
payloads are opaque. The adapter requires exact types, modes, outcomes and effects;
a changed or unsupported live contract produces a host error. Ordinary process
completion cleans escaped host resources outside the language trace.

## Tools and compatibility

### Tellur name and migration

From compiler 0.36, the language is named **Tellur**. New source files use `.tellur`;
the compiler is `tellurc` and the language server is `tellur-lsp`. The language's
`path` keyword, path atoms, masks and provenance keep their meanings and spelling.
The original v0.1 specification and vision retain their historical names.

Open `tellur.code-workspace`. Both `.tellur` and `.path` files receive Tellur
highlighting, hover and diagnostics. Imports use their exact relative filenames;
a compilation may import either extension, including a mixture of the two.
Changing a filename also requires updating imports that name it.

The `pathc` and `path-lsp` binaries remain compatibility entry points into this same
compiler/server. They report the Tellur release version. Existing `path.*` editor
commands and explicitly assigned `path` language IDs remain accepted. New commands
are `tellur.build`, `tellur.buildAndRun`, and `tellur.restartServer`.

Settings now use `tellurLanguage.*`. An explicitly set new setting wins; otherwise
the matching explicitly set `pathLanguage.*` setting is used. Empty strings and
`false` count as explicit values. The default applies only when neither is set.
The language client's trace setting uses `tellurLanguage.trace.server`.

The extension ID is `tellur-language-local.tellur-language`. When installing it
manually over the old package, uninstall `path-language-local.path-language` to
avoid two servers handling the same files, then reload the editor window. The local
release installation performs this replacement. A new extension identity does not
migrate arbitrary per-extension VS Code state.

The checkout directory remains `outputs/path-prototype`, preserving existing absolute
paths. The old workspace file remains usable. Existing `.path` playground files,
including user edits, were preserved; their `.tellur` copies are the new starting
points. These are independent files, not synchronized views: continue editing the
new copy. Future user-created `.path` files remain supported.

Native builds now write `name.tellur-build/` and default to `.tellur-cache/` beside
the output. Set `TELLUR_NATIVE_TARGET_DIR` to share the Cargo target cache. When it
is unset, `PATH_NATIVE_TARGET_DIR` is accepted as a fallback. Existing build outputs
and caches are left in place. The Rust library crate is now `tellur`.


```sh
./bin/tellurc inspect playground/count.tellur playground/manifest.json main
./bin/tellurc expand playground/count.tellur playground/manifest.json main
./bin/tellurc run examples/load.tellur examples/manifest.json examples/double-failure.json
```

Native executables accept `--json`, `--entry`, `--scenario FILE`, `--fuel N`, and `--`
before positional arguments. Declared completion outcomes use their explicit
[process-status mapping](reference/process-entry.md); host faults report nonzero status.
An optional fuel budget bounds host execution; normal
native execution has no default instruction limit. Aggregates use the JSON value
format in the historical scenarios. Wire/IR builtin type names retain their v0.1
capitalized spelling for compatibility; source and new manifests use lowercase.

Legacy `region`, `name: Type`, `:=`, parenthesized payloads, `-> [...]`, and `Entry`
remain accepted. The original examples/manifest keep their own case-sensitive names.
Do not mix old primitive outcome names with the revised playground manifest.

Editor settings: `tellurLanguage.compilerPath`, `serverPath`, `manifestPath`,
`programArguments`, and `releaseBuild`. The entry setting is still named
`tellurLanguage.entryRegion` for compatibility; it selects the control to run. The
extension reports the first diagnostic per document. Hover supports control and named-outcome declaration names and inferred `auto`
bindings/contracts. It does not yet provide general expression or call-site type hover.

The historical `region` spelling retains v0.1 implicit copying and implicit borrowed
arguments for compatibility. New programs should use `control` for the ownership rules
described here. Borrowed slices, exclusive/nested collection elements, public collection-origin contracts
and async tasks remain planned; they are not executable features of 0.51.0.
## Compare supplied scenarios

```sh
./bin/tellurc compare playground/algebra/original.tellur playground/manifest.json \
  playground/algebra/identity.tellur playground/algebra/scenarios.json
```

The scenario file is a JSON array. The report lists checked entries, observed
agreement, declared outcome-shape equality and counterexamples. Replace
`identity.tellur` with `reordered.tellur` to observe changed event order. The command
returns nonzero for a mismatch or an inconclusive error. Matching inputs and
reachable type layouts are required. Runtime failures and budget exhaustion are
inconclusive. Passing cases establish agreement only for those cases, not general
equivalence, refinement or termination.

## Validate expansion

Expansion preserves invocation and internal decision identities through a checked
correspondence. All `--expanded` executions/builds now validate that derivation.
To export the graph and its certificate for inspection:

```sh
./bin/tellurc certify-expansion playground/forwarding.tellur playground/manifest.json main > expansion.json
./bin/tellurc verify-expansion playground/forwarding.tellur playground/manifest.json expansion.json
```

Verification checks the artifact against the supplied source without rerunning the
expander. It checks the permitted rewrite, including effects, payload transfers,
loops and provenance; it is not a general equivalence or termination proof.
See [checked expansion certificates](expansion-certificates.md).

## Extract a tail into a helper

```sh
./bin/tellurc extract-tail playground/extraction/original.tellur playground/manifest.json \
  greeting 12 finish_greeting > /tmp/path-extracted.tellur
```

The command selects the unconditional tail starting on line 12, infers typed inputs,
generates an explicit exit contract and forwards to the new helper. It checks the
whole candidate and preserves existing public contracts. It prints source without
editing the input; `--report` prints source and boundary metadata as JSON. Use a
separate output file. See [tail extraction](extraction.md) for the runnable example,
ownership restrictions and the distinction between rechecking and equivalence.

## Query structural protocols

```sh
./bin/tellurc protocol playground/read_file.tellur playground/manifest.json show_file playground/protocol/read_failed.json
./bin/tellurc protocol-compare playground/algebra/original.tellur playground/manifest.json playground/algebra/identity.tellur main
```

The first command asks what may follow a prefix: here, a successful open followed by
a failed read. Its next modeled operation is close. Omitting the prefix file exports
the complete finite graph. Loops are represented by cycles, not bounded unrolling.

The second checks both inclusions of the models' completed finite trace languages,
returning a shortest abstract counterexample on failure. Values, emit text, payloads
and data correlations are forgotten. Equal protocols do not establish equivalent
programs; impossible concrete histories may appear in the abstraction. Model or
comparison budget exhaustion is an inconclusive error, never partial success.
Read [protocol analysis](protocol-analysis.md) for symbols, residual fields and limits.
