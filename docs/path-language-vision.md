# Path Centric Language Vision

Status: research and design vision, revision 1. Date: 24 October 2026. The language remains unnamed. Companion: **Path Centric Language Specification Draft v0.1**.

The central proposal is a language whose primary control structure is a composable family of named execution paths. Statements participate in selected families. Operations explicitly refine those families. Regions introduce reusable boundaries while preserving the internal distinctions needed for expansion, analysis, and explanation.

The practical objective is to make unexpected control flow difficult to introduce unnoticed, especially in large codebases maintained by many people and increasingly edited by agents. The broader ambition is to make control structure available for algebraic composition, verified transformation, and multiple human and machine representations.

This document preserves the ideas and motivations developed in the design conversation. It is a synthesis, not a verbatim transcript. Examples beyond the stated v0.1 subset are proposals, not implemented features. The specification governs the first prototype; this vision explains what that deliberately small prototype should allow us to investigate.

# 1 The problem that made the proposal concrete

The originating work environment is a legacy C# codebase of roughly three million lines, with many engineers modifying different areas. The recurring difficulty is the number of control paths and the way local edits create nonlocal consequences. Exceptions serve both failure handling and ordinary control. They skip stack frames; Try-style APIs add another convention alongside them. A programmer can introduce one branch locally that creates several new behaviors in callers far above it.

The proposed intervention is to make those alternatives explicit obligations. A new outcome should require a deliberate continuation, recovery, exit, or repetition. Compilation should reject an outcome left without a disposition. The source should expose the relationships among alternatives, shared work, cleanup, and caller policy.

The aspiration was expressed strongly: if code compiles, the author has treated every possible control path and knows about it. A defensible language guarantee is narrower: every outcome in the declared model has an explicit disposition, with valid data and resources along it. A compiler cannot establish human understanding or policy correctness merely from coverage.

The distinction matters. We want stronger accountability for control flow without claiming that a syntactic acknowledgement proves good judgment.

# 2 Why an agent first language was the starting point

The discussion began with a language designed from scratch for agentic programming. The initiating observations were personal experience: agents frequently introduced memory errors in C; Rust's ownership rules appeared to remove much of that burden without slowing the agent's work; agents seemed comfortable generating assembly; Python appeared frequently; and agents sometimes produced proofs whose structure was difficult for a human to follow.

These are motivations, not controlled comparative results. They do not establish that agents always write correct assembly, that Rust eliminates every memory issue across all code and foreign interfaces, or that an agent's proof is correct before checking. No claim about a hidden system prompt favoring Python is required by this design.

The transferable lesson is that a language can change the error space. Rather than asking an agent to remember every convention in a vast codebase, the language can make certain omissions mechanically rejectable. Human readability still matters, but the canonical representation need not reproduce the constraints of a conventional line-oriented editor.

One early hypothesis was to devise a novel logic freed from human-intuitive analogies, design the language to map cheaply into it, and let agents formulate contracts and solver queries. That remains a research question. More expressiveness does not automatically make inference easier; a linear-size translation does not imply linear-time proving. The string-theory-versus-Newtonian-physics analogy was exploratory, not a justification for computational tractability.

The emerging direction is a small, checkable semantic core with explicit contracts and useful restrictions. Agents can help construct programs, specifications, and proof objects. A trusted checker must validate their claims independently. Language soundness itself requires a metatheoretic argument about the semantics and implementation assumptions, not just an ordinary query asserting that a particular program is sound.

# 3 From mathematical paradigms to control paths

The conversation explored whether other mathematical structures could inspire a useful programming paradigm: topology, sheaves, algebraic structures, and more exotic areas of mathematics. The analogy with functional programming was a design prompt, not a historical claim that functional programming merely arose by tidying up imperative functions.

Two directions became especially interesting. First, a representation could keep alternatives and independence explicit instead of prematurely linearizing everything. Second, local program descriptions might be composed through compatibility constraints, taking inspiration from local-to-global mathematics.

The practical control-flow problem then supplied a concrete subject for these ideas. A path graph gives explicit alternatives, reusable regions provide local descriptions, and expansion exposes their composition. This does not yet amount to a sheaf-theoretic language or a novel logic. It gives us a tractable experimental object on which to test whether those ideas add value.

There is no claim that existing languages or compiler representations lack graphs, exhaustive variants, effects, continuations, ownership, or control-flow analysis. The proposed contribution is their organization around source-level path families, explicit acknowledgement, retained provenance, and interchangeable abstraction scales.

# 4 The original low level sketch

An early sketch introduced a branching operator whose definition wires paths and a `Delta` operation that applies it:

```text
branching operator x { ... }

Delta(x<params>, A, B)

A: expr
A: expr
AB: expr
B: expr
B: Delta(if<expr>, A, AC)
AC: expr
C: expr
```

The important intuition was that a statement could belong to more than one path without copying the statement or merging away the path identities. The notation evolved toward explicit masks:

```text
[A]: prepare_a();
[B]: prepare_b();
[A | B]: common_work();
[A]: finish_a();
[B]: finish_b();
```

Assume A and B have already been established. An A execution runs prepare_a, common_work, and finish_a. A B execution runs prepare_b, common_work, and finish_b. Sharing common_work does not erase which family brought the execution there.

This is the design feature to preserve when experimenting with syntax. A conventional nested match can express many individual examples, but making every example look like match would obscure the intended primary abstraction: path membership and graph composition.

# 5 Membership is not merging or joining

Several concepts must remain distinct:

| Construct | Intended meaning |
| --- | --- |
| Shared membership | One statement participates in several histories while preserving their identity |
| Outcome abstraction | Several internal histories intentionally map to one public outcome |
| Alternative selection | One execution chooses one outcome |
| Concurrent fork | Several executions or tasks are created |
| Synchronizing join | Several concurrent results are required before continuing |
| Trace concatenation | One segment follows another compatible segment |

In particular, `[A & B]` means the current history satisfies both facts. It does not wait for task A and task B. `[A | B]` executes a statement once when either or both facts hold; it does not execute once per matching label.

The v0.1 core supports sequential alternatives and shared membership. Concurrency would need explicit fork and join semantics, resource transfer rules, and a model of independence. It must not be smuggled into the meaning of the existing combinators.

# 6 A small program that exposes the benefit

Reading and closing a file is a compact example of the original problem. Reading can fail. Closing can also fail. The close failure should not accidentally replace the read failure, and successful cleanup should not erase the earlier outcome.

The abstract close contract used here consumes the handle on either outcome. `ReadOnly`, `CloseOnly`, and `Both` are error constructors. The manifest and complete v0.1 syntax are given in the specification.

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

The last four masks document the intended cases. Removing one produces an uncovered-family error. The shared diagnostic runs once on a double failure. The read outcome remains available after the close decision. Ownership adds another independent check: every path that receives a handle must consume or transfer it.

This example is not evidence that no other language can express the behavior. It is a test of whether our representation makes the combinations, omissions, and later changes particularly explicit.

# 7 Paths as checked documentation

Names such as ReadBad, CloseBad, UseDefaults, and KeepCurrent describe the programmer's interpretation of an event. Because they are attached to control structure and checked dispositions, they can serve as more durable documentation than disconnected comments.

The compiler verifies the structural relationship between these names. It does not verify that an operation named Validate really performs adequate validation, or that a path named Safe is safe. Semantic names need contracts when their meaning is a correctness requirement.

This suggests two separate layers: domain vocabulary for explanation, and machine-checked predicates or effects for guarantees. An agent could help connect them without treating the vocabulary itself as a proof.

# 8 Progressive flattening and lifting

A region boundary should hide detail without destroying the information needed to recover it. Starting from a call, an expansion operation substitutes the callee's graph, intersects its local selectors with the caller context, renames local identities, and connects each exit to the corresponding caller continuation.

Consider a startup caller and a refresh caller that use the same loader. These names stand for already-established contexts; the following fragment illustrates their call mappings.

```text
[Startup]:
    config = Δ load(config_path) -> [
        Loaded(bytes) => Configure,
        Missing => UseDefaults,
        Failed(error) => Abort
    ];

[Refresh]:
    config = Δ load(config_path) -> [
        Loaded(bytes) => Reconfigure,
        Missing => KeepCurrent,
        Failed(error) => Warn
    ];
```

The two fragments belong to distinct caller scopes. Expansion can expose explanations such as:

```text
Startup / config.ReadBad / config.CloseBad / Abort
Refresh / config.ReadBad / config.CloseBad / Warn
```

These are display descriptions of ordered history, not executable identifiers or a replacement for masks. The same internal double failure receives a different policy in each context. A human can read the description as: during refresh, reading and closing failed, then the caller warned.

For a call under C and an internal mask G, the expanded guard is C intersected with a fresh instance of G. The call-site instance is essential. Two calls to `load` must not share decision identity merely because they use the same definition. Internal identities should be compact structured references; very long English-readable names are renderings.

Public outcomes retain a mapping to their constituent histories. For example:

```text
load.Failed =
      load.CannotOpen
    | (load.ReadBad & load.CloseOK)
    | (load.ReadOK & load.CloseBad)
    | (load.ReadBad & load.CloseBad)
```

Expansion must also substitute values, preserve ownership and effects, and map exits into caller continuations. Pasting a callee's `exit Failed` as a caller exit is incorrect. Sharing the callee definition does not authorize deduplicating two executions of an effect.

The inverse direction is factoring or extraction: package a compatible subgraph into a region while retaining its boundary mapping and provenance. Exact text round-tripping is unnecessary. Behavioral and explanatory correspondence are the requirements.

Complete expansion is finite only for suitable finite structure. Loops have arbitrarily many potential repetitions, recursion has unbounded call contexts, and even acyclic graphs may have exponentially many paths. A useful system expands factored graphs and offers bounded or symbolic path queries. It does not promise cheap enumeration of all feasible executions of every program.

# 9 Nested path combinators and alternative factorizations

The proposed nesting form is a direct algebraic abbreviation:

```text
[A] over &: {
    [B]: first();
    [C]: second();
}
```

means:

```text
[A & B]: first();
[A & C]: second();
```

The file result handling can be organized first by read outcome:

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

It can equally be organized first by close outcome. Those particular terminal cases are mutually exclusive, so changing their presentation order preserves behavior. Reordering overlapping effectful statements is a different operation and needs justification. Boolean algebra alone cannot reorder effects.

Nested masks offer several perspectives on the same structure: caller context, failure origin, resource state, or recovery policy. This could become a useful interface between a canonical graph and human-readable source projections.

Intersection might eventually become the default nesting operator. Explicit `over &` is preferable for the experiment because it reveals the expansion. Generalized `over |` has a coherent substitution meaning, but `[A] over | { [B]: first(); [C]: second(); }` makes A run both statements. Its visual nesting may misleadingly suggest restriction. It is deferred until a concrete use justifies it.

# 10 Path algebra beyond masks

The long-term proposal distinguishes a path family, one actual trace, and a continuation describing the remaining possible behavior. Making a family explicit does not by itself make arbitrary executable continuations safe to capture or duplicate.

For families of compatible trace segments, a candidate algebra includes:

| Operation | Meaning |
| --- | --- |
| Union | Alternative histories |
| Intersection | Histories satisfying two descriptions |
| Relative difference | Histories in one family excluding another |
| Concatenation | Compatible segments in sequence |
| Star | Zero or more finite repetitions |
| Projection or hiding | Omit selected internal observations |
| Expansion | Reveal a region's internal structure with its provenance |
| Residual after a history | Describe the permitted suffixes after that history |

The operands' universes and observable alphabets must be explicit. Intersecting sets of traces is not the same operation as intersecting state-transition relations. Hiding internal events is an abstraction operation, not necessarily a reversible one unless an expansion mapping is retained separately.

Typed concatenation should require compatible boundary data, ownership, and effects. It is not unrestricted string concatenation. Star covers finite repetitions; infinite behaviors and fairness need a separate treatment.

Kleene algebra with tests is a relevant foundation for program composition and reasoning, combining regular-event algebra with Boolean tests. Boolean algebra on tests should not be mistaken for unrestricted intersection or complement of whole programs. The proposed richer family-query layer requires its own semantics. See the Cornell KAT project in the references.

# 11 Remaining paths as a continuation type

Suppose a family is described as:

```text
Request = Authenticate ;
          (Denied | (Granted ; Read ; (Found | Missing)))
```

After the history `Authenticate ; Granted`, the remaining family is `Read ; (Found | Missing)`.

The candidate operation is the language derivative or left quotient:

```text
residual(P, h) = { s such that h ; s belongs to P }
```

A continuation could carry a type describing its permitted remaining paths. Executing a decision changes that type; routing transfers the corresponding obligations. This might provide a principled way to express protocol progression, staged workflows, and control capabilities.

For regular finite-state abstractions, a residual can be represented by automaton state instead of an ever-growing literal history. General program data and recursive calls require richer machinery or approximation. This is a research direction, not a promised consequence of merely adopting regex syntax.

# 12 Routing and path transformations

Routing fits the central idea: an execution carries state and established provenance to a destination whose input contract it must satisfy.

```text
path Invalid = Parse.Malformed | Parse.Overflow;
path Transient = Storage.Unavailable;

[Invalid]: route -> Reject;
[Transient]: route -> Retry;
[Success]: route -> Continue;
```

This is future syntax. It describes explicit wiring rather than a nested match. The compiler would check coverage, competing destinations, value availability, and resource/effect compatibility. Labels cannot be arbitrary unchecked jump targets.

NetKAT offers a useful domain-specific precedent: algebraic descriptions of packet forwarding and automated reasoning about network behavior. The proposed analogy is a typed execution moving through a control graph. NetKAT's guarantees do not automatically transfer to unrestricted general-purpose computation.

Path transformations might insert tracing on selected families, factor repeated recovery, constrain a workflow, or derive a specialized region. Every transformation must distinguish three intentions:

```text
select paths satisfying Policy       // describe a subset
require paths(Process) subset Policy  // establish an obligation
restrict Process by Policy           // construct enforcement
```

A query that selects only good traces does not remove bad executable behavior. Enforcement needs explicit behavior when a forbidden transition is reached, and a policy depending on unknown future information may not be implementable online. Instrumentation or retry rewrites must not duplicate effects accidentally.

Relative difference introduces another hazard: `[Outgoing minus Success]` can absorb a newly added failure without anyone naming it. The language must distinguish a closed acknowledged outcome set from an intentionally extensible policy. The prototype avoids executable difference until that choice is precise.

# 13 Rewinding and reversibility

Rewinding has three distinct meanings worth preserving.

First, backward analysis asks which predecessors, caller contexts, or input conditions can lead here. This is a graph/query operation and does not undo execution.

Second, restoring a checkpoint rolls back a controlled set of state changes. That requires ownership or isolation, appropriate logs or snapshots, and a declared boundary around external effects.

Third, inverse execution actually reverses operations. Reversible languages such as Janus are relevant prior work. An assignment that discards an old value requires that information to be retained somewhere if it is to be undone.

A future capability might look like:

```text
checkpoint c;
[Working]: mutate_owned_state();
[Failed]: rewind c;
[Ready]: publish();
```

The compiler would reject a rewind across an effect the checkpoint cannot restore. Sending an email cannot erase its recipient's observation by reversing an edge. Compensation is another forward action with its own possible outcomes, not an inverse.

In concurrent execution, reversal also has a causal condition: consumers of an effect may have to be undone or isolated before its producer can be reversed. Rewind permission should therefore be an effect/resource capability checked over every applicable route, not a universal operation on any path label.

# 14 Alternatives and independence

One of the original insights was that sequential source text commits to an order even when the reason for that order is unclear. A reader must infer whether neighboring statements depend on each other, merely happened to be written that way, or interact through a hidden effect.

The eventual representation could make dependencies and independence explicit. A selected path might then admit several schedules that differ only by reordering independent actions. Queries could reason about the equivalence class instead of enumerating every irrelevant interleaving.

This needs a declared observation model and effect discipline. Operations on distinct data may still interact through I/O, resources, cancellation, or timing. Independence cannot be guessed solely from surface variable names.

The v0.1 prototype intentionally retains sequential order to isolate the path-accountability idea. That restriction should not be mistaken for abandonment of the independence direction. It establishes a baseline against which a partially ordered extension can be evaluated.

# 15 Sheaf inspired local to global composition

The earlier interest in sheaves concerned composing compatible local descriptions. A possible programming interpretation would treat regions or views as local descriptions, identify their overlaps, specify how observations restrict to those overlaps, and ask whether compatible descriptions can be assembled into a global behavior.

This remains a mathematical research sketch. Before claiming an actual sheaf model, we would need to define the underlying space or category of contexts, the objects of local behavior, restriction maps, the compatibility relation, and the intended gluing property. A suggestive diagram is not enough.

There is a potentially useful connection with flattening: abstraction gives a local view of a larger behavior, while provenance retains how that view relates to the surrounding graph. Different editor projections could be required to agree on their shared decisions and data dependencies.

Local consistency would still not automatically establish a realizable online strategy, deadlock freedom, fairness, or satisfaction of global requirements. Those would need their own definitions and obligations. The first prototype should provide a concrete semantics on which a later local-to-global formulation can be tested.

# 16 The holographic editor

The editor was deliberately deferred while the language was developed, but it is a major part of the complete vision. A conventional text view shows a small sequential slice and forces the reader to mentally reconstruct its control context. The proposed view would expose the selected statement together with relevant incoming families, outgoing alternatives, and valid pairings.

The user imagined a zoom gesture such as Ctrl-scroll: expand one call boundary, then another, progressively revealing the caller contexts and internal outcomes surrounding the current point. At one scale a region appears as a simple interface; at another its cases and the caller's recovery policies become visible together.

Useful views might include a single selected execution family, the family of failures reaching one handler, all callers of a region in the current closed program, the possible paths from resource acquisition to release, or the change in outcome families introduced by an edit. Open-world callers and dynamic dispatch would require explicit assumptions.

The editor should distinguish structural possibilities, proved feasible executions, and witnessed runtime traces. It should also distinguish sequential sharing from concurrent joining. Otherwise a visually persuasive projection could create false confidence.

Clutter may reveal poor architecture, but it is not a proof of poor architecture: a legitimate domain may have complex policies, and a bad projection can make a clear design look tangled. The opportunity is to expose complexity and let the programmer choose meaningful factorizations without erasing it.

Canonical structured data could serve agents while humans use several projections. No decision has been made to replace text entirely or to require a particular editor for writing valid programs.

# 17 What the compiler should and should not promise

The intended near-term compiler obligations are explicit outcome enumeration, complete disposition of modeled paths, valid value availability, resource accounting, causal selector availability, and preservation of behavior under defined transformations.

Stronger queries may later express invariants, preconditions, postconditions, variants, protocol rules, or forbidden histories. An agent can help formulate these, but the specification must still be checked against the intended requirement. Proving the wrong contract does not establish the right behavior.

The distinction between structural and feasible paths must remain visible. A graph can contain combinations that no concrete data can produce. A conservative checker may reject them until supplied with a proof or a more precise analysis. In unrestricted programs, an exact general procedure is not the design assumption.

Named public exits also impose a deliberate abstraction boundary. A newly declared public exit breaks old callers until they account for it. A newly handled internal distinction can map to an existing exit without breaking callers. Flattening preserves its inspectability. This keeps the language usable in large systems while making the accountability boundary explicit.

# 18 Prototype plan and success criteria

The first prototype should implement the v0.1 specification and its conformance cases, beginning with a reference interpreter, a structural checker, and graph expansion. LLVM is a proposed executable backend, not the semantic definition of paths.

The initial experiment is the file loader. It should preserve both read and close errors, run shared work once, release the resource on all relevant paths, reject a missing outcome combination, and retain the same observable behavior when expanded into different callers.

The port parser tests data availability and explicit alternatives. The bounded retry tests cycles, cancellation at two suspension points, fresh per-iteration identities, and explicit repetition. Both are specified in the companion document.

Measure more than acceptance of happy-path examples. Useful experiments include the size of a change required when an API adds an outcome; the clarity of the uncovered-path diagnostic; the amount of source and graph growth under expansion; whether an agent can repair an omission without changing unrelated policy; and whether human readers correctly identify both failure origin and caller consequence.

The representation should avoid exhaustive path enumeration as its normal operation. It should retain graph sharing, compact identities, and symbolic family expressions. Runtime costs and annotation burden must be measured rather than assumed away.

The language fails its first practical test if it merely adds verbose labels without making real omissions easier to find and fix. It succeeds provisionally if naming and checking paths improves maintenance under realistic changes while preserving manageable abstractions.

# 19 Decisions retained and questions still open

| Topic | Present position |
| --- | --- |
| Primary abstraction | Named path families and explicit refinement |
| Source order | Sequential in v0.1; independence remains future work |
| Shared statements | Preserve origin; union never duplicates execution |
| Region boundaries | Closed public exits with retained internal provenance |
| Flattening | Typed graph expansion with fresh call identities |
| Nested masks | Intersection factoring with order preserved |
| Acknowledgement | Explicit disposition at each declared boundary |
| Values | Immutable definitions with path-dependent availability |
| Resources | Minimal linear ownership in the prototype |
| General path algebra | Candidate extension with separate query and execution meanings |
| Rewinding | Capability-based research direction, not an unconditional primitive |
| Sheaves and topology | Mathematical research directions awaiting precise models |
| Editor | Future projection of the semantic structure |
| Language name | Not chosen |

Open questions include which history distinctions may be abstracted while preserving useful explanations; how much arithmetic or relational reasoning belongs in the core; how recursive and open-world calls expose provenance; how an intentional broad policy differs from an accidental catch-all; how resource protocols interact with path routing; and whether path descriptions should become runtime values at all.

The v0.1 draft makes provisional choices to enable experimentation. Those choices should be revised when concrete programs expose contradictions or unnecessary burden. The larger design should remain free to change without losing the problem that motivated it.

# 20 Sources and reading directions

These are technical precedents and research leads, not claims that the proposed language has already been constructed or that its combination is novel.

1. [Cornell Kleene Algebra with Tests project](https://www.cs.cornell.edu/Projects/KAT/). Equational reasoning about programs using Kleene algebra and Boolean tests.
2. [Dexter Kozen, Nonlocal Flow of Control and Kleene Algebra with Tests](https://ecommons.cornell.edu/entities/publication/26393b5f-a8d7-4a40-879b-272b8780de9c), 2008. Directly relevant treatment of jumps, multilevel breaks, and exception handlers.
3. [NetKAT project](https://netkat.org/). Algebraic network programming and verification; useful for investigating the routing analogy within a precisely defined domain.
4. [University of Copenhagen Program Inversion and Reversible Computation, Janus](https://topps.di.ku.dk/pirc/?id=janus). Reversible programming and program inversion.

Further research should compare this proposal with effect systems, typed continuations, linear types, session types, control/data-flow intermediate representations, guarded commands, trace semantics, and partial-order models of concurrency. Such a comparison should identify concrete semantic differences rather than claim novelty from vocabulary.

The design law to preserve is: expanding a region or factoring selectors must preserve behavior and the provenance of every alternative that the declared abstraction promises to retain.
