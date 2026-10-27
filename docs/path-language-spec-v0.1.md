# Path Centric Language Specification Draft v0.1

Status: experimental design specification, revision 1. Date: 24 October 2026. No language name has been selected. “Path language” is a descriptive placeholder.

This specification defines a small sequential language in which named families of control paths select statements, operations expose closed outcome sets, and the compiler rejects unacknowledged control flow. It is intended to guide a reference interpreter, semantic checker, and subsequent LLVM compiler experiment. It does not describe an existing implementation.

The companion **Path Centric Language Vision** preserves the broader design, motivations, alternatives, and future work. This specification governs v0.1 when the documents differ. MUST and MUST NOT mark normative requirements for this prototype. SHOULD marks implementation guidance. Provisional decisions below are binding within v0.1 but open to revision in later versions.

# 1 Purpose and guarantees

The motivating problem is unexpected control flow in large programs: a local operation introduces a new outcome that propagates through callers without an explicit, inspectable disposition. Exception unwinding and mixtures of exceptions with ordinary error returns make this particularly difficult to follow.

A valid v0.1 program MUST explicitly enumerate each invoked operation's declared outcomes. Every normally progressing invocation MUST eventually encounter an explicit transfer or remain in an explicitly written cycle; it MUST NOT silently fall through a region boundary. Each value use MUST be valid on every selected path. Modeled linear resources MUST be accounted for on every transfer.

Compilation establishes structural coverage and type/resource consistency within the declared model. It does not establish that the author understood the program, chose the correct business policy, proved termination, or modeled every physical failure. Exact feasible-path enumeration is not promised.

# 2 Scope and provisional choices

The core includes sequential execution; immutable values; closed variants and outcomes; region calls; path masks using intersection and union; named path aliases; intersection nesting; explicit tail loops; simple linear resources; and provenance-preserving region expansion.

The following are excluded from the core: implicit exceptions, general jumps, concurrency, dynamic dispatch, first-class runtime continuations, recursive region calls, unrestricted mutation, executable path difference/complement, general solver-backed contracts, rollback, and arbitrary graph rewriting. A compiler MUST diagnose unsupported constructs rather than silently supply a different semantics.

The call graph MUST be acyclic in v0.1. Loops provide repetition without recursive calls. This is a prototype restriction, not a statement about the eventual language.

The source is ordered. Masks select which statements participate in a particular execution; they do not make those statements unordered. Revealing independence is a later extension.

# 3 Semantic objects

| Object | Meaning |
| --- | --- |
| Region | A reusable ordered computation with named exits and typed payloads |
| Outcome | One member of an operation's closed result set |
| Decision | One invocation of an outcome-producing operation |
| Path atom | The fact that a particular decision selected a particular outcome |
| Path mask | A Boolean selection of execution histories, restricted to established facts at an executable use |
| Path family | The set of histories described by a mask or a richer analysis expression |
| Trace | An ordered record of one execution's decisions and observable events |
| Provenance | Stable correspondence between expanded graph nodes and their source definitions and call sites |
| Transfer | A region exit or explicit loop restart |

An outcome-producing operation chooses exactly one outcome each time it returns. It MUST NOT implicitly duplicate execution. A union of masks also MUST NOT duplicate execution.

An invocation can be understood as a cursor through ordered statements, an environment of values, a set of established decision facts, and a resource state. This is a semantic model; an implementation need not store a literal set of history facts at runtime.

# 4 Surface notation

Identifiers are case sensitive ASCII letters or underscore followed by letters, digits, or underscore. UTF-8 string literals use double quotes and the escapes `\n`, `\r`, `\t`, `\\`, and `\"`. Line comments begin with `//`. Whitespace is insignificant except inside strings and comments. The Unicode `Δ` and ASCII keyword `delta` are interchangeable.

Every executable statement ends with a semicolon. Region and block closings do not require an additional semicolon. Path declarations do end with a semicolon. Examples from the exploratory conversation omitted semicolons; the forms here supersede those sketches.

The following grammar describes the core control syntax. Types and pure expressions are specified separately.

```text
region       ::= "region" ID "(" parameters? ")"
                 "exits" exit_decl ("," exit_decl)* "{" item* "}"
item         ::= path_decl | guarded | nested | tail_loop
path_decl    ::= "path" ID "=" mask ";"
guarded      ::= "[" mask "]:" statement+
nested       ::= "[" mask "]" "over" "&" ":" "{" item* "}"
mask         ::= union
union        ::= intersection ("|" intersection)*
intersection ::= atom ("&" atom)*
atom         ::= qualified_ID | "(" mask ")"
statement    ::= binding | observation | delta | exit | again
binding      ::= ID ":=" expr ";"
observation  ::= "emit" "(" expr ")" ";"
delta        ::= (ID "=")? ("Δ" | "delta") operand
                 "->" "[" arm ("," arm)* "]" ";"
arm          ::= outcome_pattern "=>" ID
exit         ::= "exit" ID ("(" arguments? ")")? ";"
again        ::= "again" ID "(" arguments? ")" ";"
tail_loop    ::= "loop" ID "(" loop_parameters? ")" "{" item* "}"
```

The body of a guarded item ends at the next item or the enclosing closing brace. A statement begins with an identifier binding, `emit`, `Δ`/`delta`, `exit`, or `again`; a path declaration begins with `path`, and a selector item begins with `[`, so these forms can be distinguished without indentation.

Intersection binds more tightly than union. Parentheses SHOULD be used where mixed operators might obscure intent. The colon following a selector does not introduce a new value scope. Regions, loop bodies, and `over` bodies do introduce lexical scopes.

# 5 Path selection and causal availability

`Entry` is true for every execution entering the current region or current loop iteration. It remains true for that context's descendants. A named branch atom also remains true after subsequent decisions; a new decision refines the history rather than replacing it.

At a statement, its mask is evaluated once against the facts already established. If true, the statement executes once; otherwise it is skipped. Statements selected by the same execution run in source order.

An atom introduced by a prior decision is false on executions that did not enter that decision's input domain. For example, `ReadBad` is false on `MissingFile`, where reading never took place. Outcome payloads are unavailable on those executions.

An executable selector MUST NOT refer to an outcome whose decision occurs later in source order, even when a solver might predict it. This is an error, not a false predicate. Forward references through path aliases are also forbidden. This restriction applies to all atoms in a mask, rather than relying on Boolean short-circuit evaluation to hide a future reference.

After its producer, an atom can therefore be true, false because a different outcome was chosen, or false because the producer was not selected. Before its producer, referring to it is illegal.

```text
[Entry]:
    Δ read(handle) -> [Data(bytes) => ReadOK, Failed(e) => ReadBad];

[ReadOK | ReadBad]:
    emit("Read completed");
```

`emit` is a total logical observation in the prototype trace model. It is not a claim that arbitrary console, network, or file logging cannot fail. A production logger with modeled failures MUST expose them through `Δ`.

# 6 Decisions and closed outcomes

`Δ` accepts a Boolean expression, a closed variant value, or a call to a declared outcome-producing operation. A Boolean has outcomes `true` and `false`. A closed variant has exactly its declared constructors. A region call has exactly the exits in the region signature.

Each declared outcome MUST appear exactly once in the arm list. Wildcards, default arms, and implicit propagation are forbidden. An omitted arm is an error even if the compiler could establish that its outcome is unreachable for particular arguments. Arms bind payload values and fresh path atoms. Two arms of one decision cannot use the same atom name.

For input domain P and resulting atoms A and B, the checker records that A and B are disjoint and their union covers P. For three or more outcomes, the analogous pairwise disjoint partition applies. The checker also records that each outcome implies P.

```text
[Supplied]:
    parse = Δ parse_u32(source) -> [
        Number(n) => Parsed,
        Malformed => BadSyntax,
        Overflow  => TooLarge
    ];
```

The optional name `parse` names the decision site, aiding diagnostics and provenance. It does not change the operation or its outcomes.

There are no implicit nonlocal escapes. An adapter for foreign code MUST translate its supported recoverable escapes into declared outcomes. Unadapted unwinding into the language is outside the valid execution model and MUST NOT be used as an ordinary control path.

# 7 Aliases and nested selectors

A path declaration is an immutable alias for a mask, not an executable branch and not a materialized list of paths. Its constituent atoms must already be in lexical scope and established before executable uses. Aliases cannot be cyclic.

```text
path ReadProblem = ReadBad;
path AnyFailure = ReadBad | CloseBad;
path DoubleFault = ReadBad & CloseBad;
```

Intersection nesting is syntactic factoring:

```text
[A] over &: {
    [B]: emit("one");
    [C]: emit("two");
}
```

expands to:

```text
[A & B]: emit("one");
[A & C]: emit("two");
```

Nested `over &` contexts accumulate intersections. They preserve source order and do not execute the outer mask as a one-time branch. Each inner statement is selected by its effective mask at that statement's position.

Path aliases declared inside a nested context denote the effective mask: `path X = B;` inside `[A] over &` denotes `A & B`. This avoids accidentally broadening an alias beyond its lexical context. Locally declared atoms and values do not escape the body; expansion MUST alpha-rename them and retain equivalent scope boundaries.

Only `over &` is supported in v0.1. In particular, union nesting is not inferred from indentation.

# 8 Values and expressions

Core value types are `Bool`, `U32`, `String`, `Bytes`, `Unit`, finite product records, closed variants such as `Option<T>`, and declared opaque resource types. Payload types and region parameter/exit types are explicit. Local binding types are inferred and MUST agree across definitions of the same binding slot.

`String` and `Bytes` are immutable abstract values. Their runtime layout and memory management are implementation choices. General heap allocation syntax, pointers, and user-defined destructors are outside v0.1.

Pure expressions include literals, available bindings, record construction/projection, variant construction, Boolean `!`, `&&`, `||`, comparisons, and explicitly named total builtins. `&&` and `||` short-circuit but neither operand may produce effects or control outcomes. Chained comparisons are unsupported: write `1 <= n && n <= 65535`.

There is no implicit numeric narrowing and no unspecified overflow. Ordinary `+`, `-`, `*`, and division are omitted from the minimal core; `checked_add`, `checked_mul`, and `checked_div` are declared outcome-producing primitives. An implementation may provide explicitly named modular operations as total builtins. Potentially failing work MUST NOT be hidden inside an expression.

The first binding of a name introduces a value slot in the current lexical scope. The same slot may have additional immutable definitions under disjoint masks. On any one execution it MUST be defined at most once, and every selected use MUST have exactly one applicable prior definition. Payload bindings obey the same availability rule. Inner shadowing of an existing value name is forbidden in v0.1.

```text
[Default]: port := 8080u32;
[InRange]: port := n;
[Default | InRange]: exit Ready(port);
```

This is valid if `Default` and `InRange` are exclusive and the final mask is fully covered. It does not mutate `port` between two states on a single execution. Missing coverage, overlap, inconsistent types, or use before definition is a compile error.

The baseline checker uses structural decision relationships, not arbitrary arithmetic reasoning. A checked operation is preferred to an unchecked operation with an unimplemented proof precondition. An extension adding refinement proofs MUST state its supported logic and rejection behavior.

# 9 Transfers and exhaustive disposition

`exit E(payload)` terminates the current region invocation with declared outcome E. It does not exit the caller. The caller's `Δ` establishes the corresponding caller atom and binds the payload. `again L(arguments)` transfers to the next iteration of the named enclosing tail loop and never falls through.

At every region end, all remaining abstract live histories MUST be empty. At every loop body end, all histories MUST have exited the region or transferred through `again`. There is no implicit return or implicit retry.

Ordinary statements may have overlapping masks. Terminal transfers MUST NOT compete for the same history. More generally, a later executable statement whose effective selector selects a previously terminated history is a compile error: source order must not silently hide a disposition or effect after a transfer.

For this check, the analyzer retains terminated-history records while scanning the remainder of the current scope. A later decision executed only by live histories introduces atoms false on those records. Before any such producer is scanned, referring to its atoms remains illegal. An overlap with a terminal record identifies unreachable selected code and is rejected.

```text
[A]: exit Good;
[A | B]: emit("later"); // error: selects already terminated A
```

The program can instead place the shared observation before the exit, or restrict the later observation to B. Structurally impossible masks are errors in executable code; contradictory analysis queries may legitimately describe an empty family in a future extension.

An implementation MUST report a conservative witness family for uncovered or conflicting control flow. It MUST distinguish a structural witness from a proven feasible input. The core may reject programs requiring data-dependent proofs beyond its structural model.

# 10 Region boundaries and change propagation

Region signatures are closed control contracts. Adding an exit makes every unmodified call site non-exhaustive. Removing or renaming an exit also requires updating mappings.

Internal branching can change without changing the public signature when the region explicitly handles each new alternative and deliberately maps it to an existing exit. Callers then remain valid. This is intentional abstraction, not implicit wildcard handling.

An exit's provenance retains the family of internal histories mapped to it. Inspection or expansion can reveal those constituents. A future caller may request a stronger contract about internal history, but such a dependency is not implicit in v0.1.

The guarantee is compositional acknowledgement at each boundary. It is not a requirement for every caller to manually name every internal alternative in all of its dependencies.

# 11 Loops

A loop is a tail construct: it MUST be the last item of its containing region or nested context. Nested loops are excluded from v0.1. All live histories reaching it enter its first iteration. A loop can be placed within `over &`; histories outside that context continue outside the context as usual.

Loop parameters have explicit types and initial pure expressions. `again` supplies the next values simultaneously. Existing bindings are not reassigned. Each iteration has a fresh namespace for local bindings and decision instances; `Entry` refers to that iteration, while outer facts remain available by their non-Entry names.

An iteration-local atom MUST NOT be used to query a previous iteration. Historical queries need a future trace-language facility. Outer bindings are immutable; an outer linear resource cannot be reused across iterations unless explicitly transferred through a loop parameter.

Loops are checked using a finite body summary and parameter/resource contracts. The checker does not unroll indefinitely. Explicit repetition acknowledges possible nontermination; termination proofs and infinite-trace liveness are separate future features.

# 12 Linear resources and trusted primitive contracts

An opaque resource type may be declared linear in the primitive manifest. A parameter either borrows it for the duration of an operation or consumes it. Borrowing cannot escape or overlap asynchronously in the sequential core. A consumed resource is unavailable afterward on every outcome; a returned successor resource must be carried explicitly in that outcome's payload.

Each owned linear resource MUST be consumed or transferred exactly once on every selected region exit or loop restart. Resource obligations are checked by path, separately from outcome coverage. Scope exit does not trigger a hidden destructor.

The primitive manifest is a trusted input to the prototype and declares parameter types and ownership modes, complete outcomes and payloads, observable effects, and whether an operation can fail to return. Core manifests MUST describe returning operations; blocking or divergence inside foreign code cannot be disproved by the compiler and must not be misrepresented as a verified total implementation.

The file example uses an abstract close API that consumes its handle on both success and failure. An operating-system adapter must implement that contract, or expose a different one. It is unsafe to assume all real close APIs have identical failure semantics.

# 13 Canonical program examples

These are source-level conformance fixtures. Their referenced primitives are supplied by typed test manifests. They have not been compiled by an existing implementation.

## 13.1 Optional server port

Manifest: `parse_u32(String)` returns `Number(U32)`, `Malformed`, or `Overflow`. `Option<String>` has `None` and `Some(String)`. The public result remains `U32`; no unchecked narrowing is hidden in this example.

```text
region port(text: Option<String>)
    exits Ready(port: U32), Invalid(message: String)
{
    [Entry]:
        Δ text -> [None => Default, Some(source) => Supplied];

    [Default]: port := 8080u32;

    [Supplied]:
        Δ parse_u32(source) -> [
            Number(n) => Parsed,
            Malformed => BadSyntax,
            Overflow => TooLarge
        ];

    [Parsed]:
        Δ (1u32 <= n && n <= 65535u32) -> [
            true => InRange, false => OutOfRange
        ];

    [InRange]: port := n;
    [BadSyntax]: exit Invalid("Expected a decimal integer");
    [TooLarge | OutOfRange]: exit Invalid("Port must be between 1 and 65535");
    [Default | InRange]: exit Ready(port);
}
```

Expected cases: None gives 8080; `"443"` gives 443; `"0"` and `"65536"` are out of range; `"abc"` is malformed; a decimal exceeding U32 produces the overflow disposition.

## 13.2 File loading with correlated failures

The test manifest declares `open(String) -> Opened(Handle) | NotFound | Denied`; `read(borrow Handle) -> Data(Bytes) | Failed(ReadError)`; and `close(consume Handle) -> Closed | Failed(CloseError)`. `LoadError` is the closed variant `PermissionDenied | ReadOnly(ReadError) | CloseOnly(CloseError) | Both(ReadError, CloseError)`.

```text
region load(path: String)
    exits Loaded(bytes: Bytes), Missing, Failed(error: LoadError)
{
    [Entry]:
        Δ open(path) -> [
            Opened(handle) => Open,
            NotFound => MissingFile,
            Denied => CannotOpen
        ];

    [MissingFile]: exit Missing;
    [CannotOpen]: exit Failed(PermissionDenied);

    [Open]:
        Δ read(handle) -> [Data(bytes) => ReadOK, Failed(re) => ReadBad];

    [Open]:
        Δ close(handle) -> [Closed => CloseOK, Failed(ce) => CloseBad];

    [ReadBad | CloseBad]: emit("File load failed");

    [ReadOK & CloseOK]: exit Loaded(bytes);
    [ReadBad & CloseOK]: exit Failed(ReadOnly(re));
    [ReadOK & CloseBad]: exit Failed(CloseOnly(ce));
    [ReadBad & CloseBad]: exit Failed(Both(re, ce));
}
```

The union observation executes once for the double failure. Both errors remain available in `Both`. Removing the last disposition leaves `Open & ReadBad & CloseBad` uncovered. Removing close leaves a linear-resource obligation. Sharing cleanup does not discard the read outcome.

The final four dispositions can be factored without changing behavior:

```text
[ReadOK] over &: {
    [CloseOK]: exit Loaded(bytes);
    [CloseBad]: exit Failed(CloseOnly(ce));
}
[ReadBad] over &: {
    [CloseOK]: exit Failed(ReadOnly(re));
    [CloseBad]: exit Failed(Both(re, ce));
}
```

## 13.3 Bounded retry without unchecked arithmetic

The manifest declares `fetch(String) -> Received(Response) | Unavailable(NetworkError) | Cancelled`; `pause(U32) -> Elapsed | Cancelled`; `checked_add(U32,U32) -> Sum(U32) | Overflow`; and `checked_mul(U32,U32) -> Product(U32) | Overflow`. Durations here are milliseconds by the pause contract.

```text
region fetch_retry(url: String)
    exits Done(response: Response), Failed(error: NetworkError),
          Cancelled, ArithmeticFault
{
    loop Attempt(number: U32 = 1u32) {
        [Entry]:
            Δ fetch(url) -> [
                Received(response) => Received,
                Unavailable(error) => Unavailable,
                Cancelled => FetchCancelled
            ];

        [Unavailable]:
            Δ (number < 3u32) -> [true => Retryable, false => Exhausted];

        [Retryable]:
            Δ checked_mul(number, 100u32) -> [
                Product(delay) => DelayReady, Overflow => DelayOverflow
            ];

        [DelayReady]:
            Δ pause(delay) -> [Elapsed => Waited, Cancelled => WaitCancelled];

        [Waited]:
            Δ checked_add(number, 1u32) -> [
                Sum(next) => NextReady, Overflow => CounterOverflow
            ];

        [Received]: exit Done(response);
        [Exhausted]: exit Failed(error);
        [FetchCancelled | WaitCancelled]: exit Cancelled;
        [DelayOverflow | CounterOverflow]: exit ArithmeticFault;
        [NextReady]: again Attempt(next);
    }
}
```

The overflow outcomes are conservatively acknowledged even though a stronger arithmetic analysis can prove them unreachable for this program. The source makes at most three fetch attempts assuming the specified arithmetic primitives and initial value. The core compiler is not required to prove that bound. HTTP error responses are still `Received`; transport failure is `Unavailable`.

# 14 Checked representation and algorithm

The compiler SHOULD retain an ordered graph with typed data edges, decision nodes, mask DAGs, transfers, resource transitions, and provenance. It MUST NOT use an unstructured set of strings as the sole meaning of paths. Names are presentation and resolution keys; stable identities distinguish definitions, calls, and iterations.

A baseline checker can proceed as follows:

1. Parse and resolve scopes; expand `over &` without reordering statements.
2. Assign identities to regions, call sites, decisions, outcomes, and value slots.
3. Traverse each acyclic body in source order, maintaining structural path constraints and the live and terminated domains.
4. Validate decision input availability, complete arm lists, and payload types. Introduce a partition of the selected live domain.
5. Check each binding and use against its selected domain; check resource consumption similarly.
6. At transfers, check payload/resource contracts and record terminated domains. Reject later selected overlap.
7. At body end, reject uncovered live domains. Check loop restart arguments against the loop header.
8. Retain symbolic counterexample assignments for diagnostics and source mapping.

A finite propositional representation, such as formulas with a SAT backend or a decision diagram, is suitable for the structural fragment. The language does not require a particular solver or promise polynomial checking. Do not enumerate the cross-product of all path combinations as the normal representation.

The checker conservatively treats separate Boolean decisions as independent unless a supported rule relates them. It can therefore reject infeasible combinations. “May reach” and “proved reachable” MUST be distinguishable in diagnostic output.

# 15 Flattening and factoring

Expansion replaces a region call with its internal graph and wires each internal exit to the appropriate caller continuation. For a call selected by C and an internal guard G, the expanded guard is C intersected with a freshly renamed G. Internal `Entry` becomes the call's entry domain.

Expansion MUST substitute argument values, map exit payloads, preserve resource transitions and source order, and instantiate fresh internal decision identities. Each internal exit completes the expanded call; it MUST NOT become an exit from the enclosing caller. A caller outcome atom denotes the union of internal exit sites mapped to that outcome, retaining the mapping as provenance.

Call-site identity is mandatory even when two calls use the same region. An implementation may display `Startup/config.ReadBad` and `Startup/theme.ReadBad`, with `load` retained as the originating definition. Concatenated display names are not the identity representation.

Expansion MUST preserve observable event traces, exit payloads, termination/divergence behavior, and resource behavior under the declared primitive contracts. It also preserves the correspondence of internal alternatives. Expansion and extraction are inverse only up to scoped renaming and explicit boundary/provenance mappings; arbitrary text is not required to round-trip byte for byte.

The required v0.1 expansion operation covers an acyclic region call graph. Loop bodies remain cyclic structures; expansion MUST NOT claim to turn an unbounded loop into a finite list of complete traces. Runtime debug histories are optional and separate from static provenance.

Factoring selectors into `over &` is permitted when order and lexical availability remain unchanged. Reordering overlapping effectful statements requires a separate equivalence justification and is not licensed by Boolean commutativity alone.

# 16 Implementation experiment

The proposed pipeline is source to scoped path graph, checked graph, conventional executable control/data representation, then LLVM. The language semantics belong to the checked graph and reference interpreter, not to a particular backend.

First implement a reference interpreter with scripted primitive outcomes and an event trace. Next implement the structural checker and graph expansion. Only then add LLVM lowering and compare observable results against the interpreter. Use explicit control branches and value joins as required by the chosen backend; never assume every path atom needs a persistent runtime Boolean.

The prototype MUST NOT lower unsupported outcomes to undefined behavior, drop failure edges because they appear inconvenient, or trust an unproved assumption solely to make backend verification pass. The compiler's own host failures are separate from source-language outcomes.

Before selecting an LLVM release, binding library, ABI, or pass pipeline, consult that release's official documentation. Those engineering choices are intentionally outside this language draft.

# 17 Conformance cases

These are required test cases for a future implementation, not claims of completed tests.

| ID | Fixture or mutation | Expected result |
| --- | --- | --- |
| C01 | Port examples in section 13.1 | Correct explicit exit and payload for each listed input |
| C02 | Remove parse Overflow arm | Reject incomplete decision, naming Overflow |
| C03 | Keep arm but remove TooLarge disposition | Reject uncovered family |
| C04 | Use `n` under Default | Reject unavailable payload |
| C05 | Define port twice on InRange | Reject overlapping immutable definitions |
| C06 | Use CloseBad before close | Reject future decision reference |
| C07 | File read and close both fail | One shared observation, Both payload, handle consumed once |
| C08 | Remove double failure exit | Reject with ReadBad and CloseBad witness |
| C09 | Remove close call and corresponding decision uses | Reject live handle at affected exits |
| C10 | Read after consuming close | Reject resource use after consumption |
| C11 | Select an observation after an overlapping exit | Reject unreachable selected statement |
| C12 | Flatten and nested forms of file exits | Same traces, outcomes, and ownership accounting |
| C13 | Two load calls with distinct inputs | Independent decisions and payloads after expansion |
| C14 | Expand a failing load inside a caller that warns | Caller warning executes; callee exit does not exit caller |
| C15 | Add a public load exit | All old call mappings become incomplete |
| C16 | Add and explicitly handle an internal failure mapped to Failed | Caller source remains valid; provenance gains a constituent |
| C17 | Script Unavailable, Unavailable, Received | Three fetch attempts, two pauses, Done |
| C18 | Cancel fetch or pause | Cancelled from either origin, no subsequent retry |
| C19 | Remove final again from retry | Reject live path at loop end |
| C20 | Refer to a previous iteration's local label | Reject unsupported historical reference |
| C21 | Finite primitive scripts before and after expansion | Identical observable traces and mapped exits |
| C22 | Union of A and B where both hold | Selected statement executes once |
| C23 | A and B are outcomes of the same decision; select A & B | Reject structurally empty executable selector |
| C24 | Selector uses an atom from a prior operation skipped on this history | Atom is false; payload remains unavailable |

The implementation should also use bounded generated programs to compare original and expanded interpreters. Such testing is evidence for a transformation implementation, not a proof of language soundness.

# 18 Draft decisions and revision policy

The following are deliberate prototype choices introduced to make the discussion implementable: sequential source order; ASCII identifier spelling; immutable path-dependent value slots; total pure expressions; strict explicit arithmetic outcomes; lexical forward-reference prohibition; no implicit exception propagation; closed public outcomes; no implicit terminal shadowing; a small linear-resource discipline; tail loops without recursive calls; and `over &` as the only nesting operator.

An implementing agent SHOULD preserve these choices unless a concrete contradiction or counterexample is found. It SHOULD record the failing example and propose a specification revision rather than silently changing semantics. Syntax conveniences can be added only with a documented expansion into the core.

Still open for a subsequent revision: richer structural correlation inference; refinement proofs; generic effects and resource protocols; recursive calls; dynamic dispatch contracts; finite and infinite trace queries; path subtraction and closed-world acknowledgement; runtime routing; executable rewrites; checkpoint capabilities; and editor projections. See the vision document for their rationale.

The first acceptance milestone is the file loader: complete handling of its outcome combinations, useful diagnostics for omissions, correct ownership checking, and behavior-preserving expansion into multiple callers.

# 19 Primitive manifest interchange format

For the prototype, primitive and foreign-type declarations live in a JSON manifest rather than requiring another source declaration language. The manifest has `version: "0.1"`, a `types` array, and a `primitives` array. Builtin types need no declaration. User type names and primitive names are unique within the compilation unit.

Each type has `name` and `kind`. Kind `opaque` requires a Boolean `linear`; kind `variant` requires `constructors`, each with `name` and `fields`; kind `record` requires `fields`. A field has `name` and `type`. Type references are strings: builtin names, declared names, or builtin `Option<T>`. Recursive user types and generic user declarations are excluded from v0.1. Constructors are unique within their variant and must be qualified as `Type.Constructor` when an unqualified use would be ambiguous.

Each primitive has `name`, `parameters`, `outcomes`, `effects`, and `returns`. Each parameter has `name`, `type`, and `mode`, where mode is `value`, `borrow`, or `consume`. Linear arguments require borrow or consume. Each outcome has `name` and `fields`. Outcome lists must be nonempty with distinct names. `effects` is an array of descriptive effect identifiers; it is retained for ordering and inspection, not interpreted as a general effect theorem. `returns` must be `true` for core manifests and is a trusted adapter obligation, not an inferred termination proof.

Example manifest fragment for closing a handle:

```json
{
  "version": "0.1",
  "types": [
    {"name": "Handle", "kind": "opaque", "linear": true},
    {"name": "CloseError", "kind": "opaque", "linear": false}
  ],
  "primitives": [
    {
      "name": "close",
      "parameters": [
        {"name": "handle", "type": "Handle", "mode": "consume"}
      ],
      "outcomes": [
        {"name": "Closed", "fields": []},
        {"name": "Failed", "fields": [{"name": "error", "type": "CloseError"}]}
      ],
      "effects": ["file.close"],
      "returns": true
    }
  ]
}
```

Region parameters use value mode for non-linear values and consume mode for linear values. Region exits transfer linear payloads to the caller. General borrowed region parameters are deferred; primitives provide synchronous borrows for the initial experiment. Whole-program compilation takes source regions, a manifest, and an explicitly selected entry region with test-driver or host-provided arguments. An operating-system process entry ABI is not prescribed.

# 20 Expression and name resolution details

Integer literals are nonnegative decimal values with optional `u32` suffix; all integer literals in the core have type U32 and exceeding its range is a compile error. Boolean literals are `true` and `false`. Unit is `unit`. String and Bytes have no implicit conversion. A byte literal has the form `bytes"00ff7a"`, with an even number of hexadecimal digits and no whitespace; invalid digits or odd length are compile errors. A record is constructed as `RecordType(field1, field2)` in manifest field order and projected with `value.field_name`. Variant constructors use the same positional argument convention. `emit` accepts a String.

From tightest to loosest, expression precedence is parentheses and constructor/builtin application, field projection, unary `!`, one comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`), `&&`, then `||`. Comparisons cannot chain. Ordering comparisons require U32; equality is defined for Bool, U32, String, and Unit in the core. There are no side-effecting expressions or implicit coercions.

Region calls and primitive calls appear only as the operand of `Δ`; payload constructor applications and declared total builtins may appear in pure expressions. Any non-builtin total operation must initially be represented as a region with a single named exit and invoked through `Δ`. This keeps its control contract explicit.

Value, path, decision-site, and region/type namespaces are distinct. A name in a selector resolves only as a path atom or alias. A name in an expression resolves as an available value, literal, or unambiguous constructor. A decision-site name is used for inspection and expansion, not as a Boolean path atom. Source references to expanded callee internals are excluded from v0.1; dotted provenance names shown by tools are not automatically valid source-level imports of private atoms.

For each value slot, the compiler forms the union of its prior definition masks. Every use domain must be a subset of that union, and prior definitions must be pairwise disjoint. The same type must hold on every definition. Moving a linear value into a constructor, exit payload, loop parameter, or consuming call transfers its obligation; copying it into two positions is an error. These checks are structural and conservative.
