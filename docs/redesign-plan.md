# Villanelle redesign plan

Status: **accepted** by the author on 19 November 2026: the surface syntax
(section 5.5, Rails) and all eight decisions in section 3, as recommended. Written
by Claude Code at the author's request. Nothing of v2 is implemented. Syntax
outside section 5.5 is illustrative, predates Rails and uses `text` fences so the
manual checks ignore it. "v1" means the language and compiler at 0.306; "v2" means
the redesign.

The plan follows from the [vision](path-language-vision.md), the
[semantic foundation](semantic-foundation.md) and a review of 0.306 that included
hands-on probes of the compiler. It revisits several accepted decisions on purpose;
section 3 lists them.

## 1. Diagnosis

The review found nine weak points. They come from four root causes. Removing the
causes removes the symptoms together, which is why the plan is ordered by cause
and not by symptom.

| Root cause | Evidence in 0.306 | Symptoms it produces |
| --- | --- | --- |
| **A. Path facts have no meaning.** A fact records which alternative was taken, never what that implies about data. | Deciding the same boolean twice yields four histories. `checked_div(count, 2)` needs a `zero_divisor` arm. `ordered_search.bound` has six arms that cannot occur, each given an invented exit. | Filler dispositions that readers cannot tell from policy; false alarms in every analysis; protocol models that forget all data |
| **B. The type and ownership core grew case by case.** | Runtime values carry their type names as strings. Borrow validity is re-checked at run time through five forms of `ReferenceEvidence`; 104 of 147 files in `src/native` handle it. The async chapter admits borrowed captures one shape at a time, in about 18 sections. References fill 35% of the manual, yet stored references appear in 1 of 39 source files under `applications`, `library` and `examples`. | Code 7× to 648× slower than Rust on the project's kernels; a manual nobody can hold in their head; soundness argued by tests; a restriction at every boundary |
| **C. The surface is the intermediate representation.** | Addition is a two-arm decision. Loops are tail-only blocks. There are no general expressions, no recursion, no nested loops, no closures and one integer type. | Ceremony; low expressiveness; label count that grows with arithmetic |
| **D. The hypothesis is untested at scale.** | About 8,000 lines of Villanelle exist, against 113,000 lines of compiler and 70,000 of tests. The largest program is about 700 lines. | Unknown readability; design choices made without data |

One further finding, made while preparing this plan, is a gap in the central
guarantee:

> A flat delta routes omitted alternatives to same-named paths, and a later
> `[entry]:` statement disposes all of them. A callee can therefore gain an
> outcome while its caller still compiles. This was verified on 0.306. A bare
> `forward` has the same property when the new label already exists in the
> caller's contract. The guarantee "a new outcome cannot pass unnoticed" holds
> today only for exhaustive adapters and for callers that name every disposition.

## 2. Principles for v2

1. **One small core, specified before it is built.** The surface language
   elaborates into a core calculus with a written semantics. The manual defers to
   that specification.
2. **Facts are propositions.** A path fact is either an event (an effectful
   operation completed with this alternative) or a logical statement about values.
   One engine checks coverage, availability, ownership and feasibility.
3. **Checked means erased.** What the checker proves, generated code does not
   re-check. The dynamic runtime of v1 survives as the test oracle, not as the
   production path.
4. **Every proof leaves a certificate.** Solvers and analyses are untrusted
   producers; a small independent kernel validates what they claim. This extends
   the expansion-certificate pattern that already works.
5. **No hidden control flow, no anonymous absorption.** Closed outcomes, no
   exceptions, no wildcard. Anything that covers several alternatives at once
   does so through a named, closed group.
6. **Programs before compiler.** Each design step is validated by rewriting real
   programs in the proposed form before any compiler work starts.

What stays unchanged: deltas and named outcomes, path selectors with symbolic
coverage, must-complete resources, the fault and contingency model, structured
tasks with cancellation as an outcome, certified expansion, the CLI, language
server, packaging, CI and the embedded LLVM toolchain.

## 3. Decisions for the author

**All eight recommendations were accepted on 19 November 2026.** D4 was settled
by adopting Rails (section 5.5), where structure is drawn. The alternatives stay
listed as the record of what was declined and what it would have cost.

| # | Fork | Recommendation | Alternative and its cost |
| --- | --- | --- | --- |
| D1 | Reference model | **Second-class borrows and views** (section 5.3). Drops origin groups and stored references in ordinary data. | Keep stored references and re-found them on compositional origin types in the style of Polonius. More expressive; multiplies the cost of the soundness proof, refinement checking and resource protocols. |
| D2 | Feasibility | **Logical facts in linear integer arithmetic, with proven `impossible` arms** (5.2). | Constants and repeated decisions only, with no solver. Cheap, but binary search and every indexed loop keep their filler arms. |
| D3 | Acknowledgement | **Every alternative is named at its decision or covered by a named group** (5.4). Closes the gap in section 1. | Keep identity routing of unnamed alternatives and rely on contract-diff tooling to show absorbed outcomes. |
| D4 | Label extent | **Indentation is normative**: a label governs its line and the block indented under it. Labels stay unbraced. | Keep the current rule and make misleading indentation a compile error. |
| D5 | Build strategy | **New core beside v1 in this repository**, built as vertical slices; v1 remains buildable and becomes the oracle. Parser shell, formatter, language server, CLI, packaging and CI are kept. | Evolve v1 in place. Lower risk per step, but the stringly typed IR and runtime evidence are woven through the checker and all of `src/native`. |
| D6 | Assurance | **Differential fuzzing against the oracle from the first slice; mechanize the core in Lean 4 in stages.** | Fuzzing and certificates only. Reasonable if D1 goes to the alternative, where mechanization becomes a research project. |
| D7 | Solver | **A built-in decision procedure that emits certificates**, with an external SMT solver used only in CI to cross-check it. Keeps the single-binary rule and makes results reproducible. | Embed an SMT solver. Faster to start; adds a large dependency and heuristic behavior. |
| D8 | Compatibility | **v2 drops the Path and Tellur legacy forms and ships a migrator from current v1 syntax.** | Carry all legacy spellings into v2. |

Accepted decisions this plan reopens: precise origin groups (26 October);
"checked arithmetic is outcome-producing, not hidden inside `+`"; dynamic values
first with typed lowering later; identity routing of omitted arms; JSON manifests
as the home of primitive contracts.

## 4. Order of work

Phases 2 to 4 are built as **vertical slices**: scalars and decisions first, then
data, then borrows, then tasks. Each slice passes specification, oracle, checker,
backend and differential fuzzing before the next begins.

| Phase | Removes | Relative size |
| --- | --- | --- |
| 0. Close out v1 | Loose ends | Small |
| 1. Design on paper | Root causes A, B, C at the level of design; first evidence for D | Medium |
| 2. Oracle and harness | Soundness by anecdote | Medium |
| 3. Checker | Filler arms, the accountability gap, case-by-case typing | Very large |
| 4. Typed backend | The performance limit | Very large |
| 5. Language breadth | Numeric types, nesting, recursion, closures, recursive data | Large |
| 6. Algebra at compile time | Explicit-state, synchronous-only, data-blind analysis | Large |
| 7. Editor and migration | Readability at scale, ceremony in existing code | Medium |
| 8. Evidence | The untested hypothesis | Medium, continuous |

### Phase 0. Close out v1

- Review and commit the collections 0.1.0 slice that sits uncommitted in the
  working tree, so `main` is clean. Its performance table becomes the v1 baseline.
- Diagnose the Windows parity failures since `2fe0b08`, or record Windows as
  unqualified at 0.306.
- Freeze v1: no further performance passes, library modules or target work. Those
  goals move to phases 4 and 5.
- Replace the Yip arc with the v2 phases once this plan is accepted.

Exit: `main` clean and pushed; v1 tagged; roadmap replaced.

State on 19 November 2026:

- The collections slice is committed (`ccf5141`) after requalification: 28
  adoption cases on LLVM and generated Rust, 3 manual tests.
- The Windows failures were a fault in two tests, not in the compiler. The tests
  derive programs by replacing anchors that contain a line break in embedded
  sources, and a Windows checkout delivered those sources with CRLF. Converting
  the sources to CRLF on macOS reproduces both failures exactly. Every `.vn` file
  is now pinned to LF (`019d012`). Windows stays unqualified at 0.306 until the
  workflow run on that commit passes.
- v1 is frozen from this point. Corrections stay possible, because v1 is the
  oracle in phase 2.
- The roadmap records are replaced by the phases of this plan.
- Not done: the v1 tag, which waits for the Windows run and for the author's
  word.

### Phase 1. Design on paper

No compiler code. Deliverables:

1. **Core specification** (section 5.1), including an instrumented small-step
   semantics that gets stuck on every violation the checker is meant to exclude.
2. **Fact logic** (5.2), **reference model** (5.3) and **surface** (5.4) as
   normative text with worked examples.
3. **The corpus on paper.** Rewrite by hand, in v2: the settings application, the
   eight collection modules, and three new programs chosen to stress the design
   (a retrying network client, a recursive-descent parser, a small interpreter
   over a tree). Record lines, labels, filler arms and annotations for both
   versions.
4. **A first reading and repair test.** Give the v1 and v2 versions of the same
   programs to people and to agents: find the origin of an outcome, add a new
   outcome to a callee and repair the callers, find a seeded omission.

Exit: the author accepts the specification; the rewritten library has no filler
arm; the paper corpus shows no construct that v2 cannot express without a
workaround; the reading test does not favor v1.

### Phase 2. Oracle and harness

- **Oracle.** A reference interpreter that implements the instrumented semantics
  directly. It tracks ownership, loans, obligations and facts dynamically and
  traps on any violation. Where v1 semantics are unchanged, v1's interpreter and
  runtime are the starting point.
- **Program generator.** Produces random well-typed core programs, biased toward
  borrows, resources, nested decisions and loops.
- **Differential harness.** Oracle against compiled code, original against
  expanded, optimized against unoptimized, on the same inputs and scripts.
- **Checker mutation tests.** Seed defects into valid programs (remove a
  disposition, leak a resource, reorder a cleanup, use a value after its move)
  and require the checker to reject each.
- **Conformance suite** regenerated from specification clauses, so every rule has
  an accepting and a rejecting test.

Exit: the oracle runs the whole paper corpus; the generator reaches every core
construct of the current slice.

### Phase 3. Checker

- A typed, interned intermediate representation. Types are never strings.
- Coverage and availability on the existing BDD engine, which already scales (48
  independent decisions check instantly).
- Ownership under the chosen reference model: usage kinds, moves, loans, one
  origin set per borrowed value, scoped task captures as a single rule.
- Effects: inferred for private controls, declared for public ones.
- Fact logic: logical facts, outcome conditions, `where` clauses, `impossible`,
  the decision procedure and its certificate kernel.
- The acknowledgement rule and outcome groups.
- Diagnostics keep the named-history witness and add concrete values when a
  history is feasible.

Exit: the rewritten library checks with zero filler arms; no generated well-typed
program traps in the oracle across a long fuzzing campaign; mutation tests are
rejected at a rate the author sets in advance (proposed: 95% or better, with every
survivor explained).

### Phase 4. Typed backend

- Monomorphized generics and static layouts: structs and variants as native
  aggregates, strings and vectors as pointer, length and capacity.
- Borrows are pointers. Origins and loans are erased.
- Facts are SSA bits. They double as drop flags, so destruction is placed
  statically from the availability analysis.
- Outcomes return as a tag with payload in registers or a caller slot; inlining
  removes most of them.
- Alternatives proven impossible generate no code. Proven bounds and overflow
  facts remove the corresponding checks, which safe Rust keeps unless its
  optimizer happens to eliminate them.
- Two build profiles: **release**, with no fuel, event recording or scripting,
  and **instrumented**, which is the oracle.
- Async keeps frontend state machines, with typed frames.
- Host adapters get a typed C ABI generated from `extern` declarations.
- The generated-Rust backend and the dynamic runtime leave the release path.

Exit: compiled code agrees with the oracle on the corpus and on generated
programs; every kernel under `experiments/` is within 1.2× of optimized Rust for
time, allocations and peak memory, measured as `AGENTS.md` prescribes.

### Phase 5. Language breadth

Each feature lands with its core rule, oracle semantics, and a statement of how
precisely phase 6 can analyze it.

- **Numbers.** Signed and unsigned integers of 8 to 64 bits, floats, explicit
  conversions. Infix operators carry proof obligations (5.2); `+%` wraps and `+|`
  saturates.
- **Functions.** `function` declares a total, effect-free, single-result
  computation usable in expressions and in `where` and `when` clauses.
- **Loops as decisions.** A loop is a local recursive control that completes with
  its own outcomes through `leave`; the enclosing body continues on those paths.
  Nested loops follow. A tail loop is the case with no local outcomes.
- **Recursion**, with explicit contracts. Recursive calls are opaque to
  expansion; analysis uses summaries.
- **Closures** as anonymous trait implementations. Non-escaping closures capture
  by borrow; escaping closures capture by move.
- **Recursive data** through owned `box<t>`.
- **Views and iteration.** Built-in `slice<t>` and text views, an iterator trait,
  and `for` as sugar over it.
- **Value-position decisions**: `delta` as an expression when every arm yields a
  value of one type.

Exit: the collection library is rewritten idiomatically (stable merge sort in
about 40 lines); the phase 8 application compiles.

### Phase 6. Algebra at compile time

- **Symbolic automata.** The checker already computes, for each instruction, the
  set of live histories as a BDD. That is a symbolic state set. Protocol queries
  become symbolic image computations in place of today's explicit enumeration,
  which stops at 14 independent decisions.
- **Policies.** Regular expressions over events and effect labels, declared in
  source and checked by language inclusion: `require serve satisfies audited;`.
  Counterexamples are shortest traces, with values when available.
- **Resource protocols.** A must-complete type declares its permitted operation
  order. `linear: true` becomes the special case "ends with a consuming
  operation". Operations that change protocol state require exclusive access;
  shared access permits only state-preserving operations. Under that rule and
  linear ownership, each resource follows one sequential trace even across
  tasks, so checking is modular and does not enumerate interleavings.
- **Behavioral contracts.** A public control's contract snapshot includes its
  trace summary. `contract-diff` reports behavioral change, and a refinement
  gate (new traces within old traces) can run in CI. This also exposes the
  delegated changes of `forward`.
- **Equivalence for refactoring.** Two control bodies are compared with
  primitives left uninterpreted, in the style of guarded Kleene algebra with
  tests extended with mutable tests. Extraction and expansion can then be
  certified by one mechanism.
- **Feasibility.** The logical facts of phase 3 prune infeasible traces, so
  policies do not drown in impossible paths.
- **Explanations as histories.** `explain` returns ordered alternatives derived
  from the automaton, as the vision's section 8 shows, in place of an unordered
  set.

Exit: the asynchronous settings server is analyzed end to end; the 48-decision
stress control is analyzed; three real policies are enforced in the application;
the CI gate reports a behavioral change that a signature diff misses.

Not pursued: sheaf-style gluing, reversible execution, a novel logic. General
reasoning about independent effects stays out of the decision procedures, since
equivalence modulo commutation is undecidable in general. Declared commuting
effects may be used later to reduce schedules in testing.

### Phase 7. Editor and migration

- **History lens.** Select a label, outcome or diagnostic witness; the editor
  dims every statement not on that history. This is the smallest useful form of
  the vision's projection editor and the direct answer to label-heavy bodies.
- **Nest and flatten** refactorings between flat selectors and nested arms,
  certified by BDD equivalence.
- **Migrator** from v1 syntax, plus a formatter that owns the layout rule.
- **Trust report.** `vnc check --trust` lists every `extern` contract, every
  unproven assumption and every policy in force.

### Phase 8. Evidence

- One application of 5,000 to 10,000 lines, maintained across releases as the
  settings service was. A service with storage, retries and concurrent clients
  exercises the design better than a compiler-shaped program would.
- If it can be arranged, one real module from the C# codebase that motivated the
  language, rewritten in v2. It is the most direct test of the original claim.
- An agent repair benchmark: seeded omissions and API changes in Villanelle and
  in equivalent Rust, scored on correct repairs and unrelated edits.
- A human reading study on the same material.
- Results published in the repository, whichever way they point, and used to
  settle the surface questions in D3 and D4.

After phase 8 the ordinary roadmap resumes: supported library modules, public
releases, the Thylacine target.

## 5. Design sketches

### 5.1 Core

- **Terms**: statements guarded by selectors over facts, decisions, local
  recursive controls (loops), calls, exits, faults.
- **Types**: nominal structs, variants and outcomes; three usage kinds (`copy`,
  droppable, must-complete); borrows and views per D1.
- **Judgments**: coverage (every live history is disposed), availability (a value
  is read only where defined), ownership (moves, loans, obligations), effects, and
  fact entailment.
- **Theorems to state, test, then mechanize**: no execution reaches an undisposed
  state; no read of an unavailable value; every must-complete resource is
  completed exactly once or settled by a declared fault disposition; no access
  through a dead or conflicting borrow; no `impossible` arm is reached; the
  erased semantics agrees with the instrumented one on well-typed programs.

### 5.2 Fact logic

Two kinds of fact. An **event fact** says an effectful operation completed with a
given alternative; it stays uninterpreted. A **logical fact** is a predicate over
immutable values in a decidable theory: linear integer arithmetic, equality,
constructor tests and collection length.

Logical facts come from boolean decisions on comparisons, from outcome conditions
in contracts, from `where` clauses and from constants. A pure decision is a
function of its operands, so deciding the same value twice yields the same fact.

```text
extern control vector_get<t>(ref vector<t> v, u32 i)
    exits found { ref t value from v } when i < len(v),
          out_of_bounds               when i >= len(v);
```

The checker keeps the BDD for the Boolean structure and asks the theory whether a
candidate history is satisfiable, the arrangement used by lazy SMT solvers. An
uncovered or leaking history is reported only when it is feasible, with concrete
values as the witness.

An alternative that cannot occur is written `=> impossible`. The compiler must
prove it, and a failed proof is an error with a counterexample. The unproven form
`=> assume impossible "reason"` compiles to a fault, appears in the trust report
and carries `faults`.

Infix `a + b`, `a / b` and `v[i]` are the checked operations with the failing
alternative `impossible`. When the proof fails, the author handles the
alternative with an explicit delta, adds a `where` precondition, or chooses a
wrapping or saturating operator. No failure is hidden: either it cannot happen or
the program does not compile.

`where` on control parameters is a precondition checked at each call. `where` on
loop parameters is an invariant checked at entry and at each `again`.

Binary search today takes about 45 lines and has six filler arms. The same
control in v2:

```text
control bound<order: comparator>(
    ref vector<order.item> source, ref order rule, ref order.item needle, bool upper
) exits position { u32 index }
{
    loop search(u32 start = 0, u32 count = len(source))
        where start + count <= len(source)
    {
        delta (count == 0) { true => exit position { start }, false => halve }
        [halve]:
            u32 half = count / 2;
            u32 middle = start + half;
            delta rule.compare(source[middle], needle) {
                before => less, equal => same, after => greater
            }
        [same]: delta upper { true => high, false => low }
        [greater | low]: again search(start, half);
        [less | high]: again search(middle + 1, count - half - 1);
    }
}
```

Every arithmetic step and the index are proved from the invariant and the
`count == 0` decision. Deleting the `where` line makes the program fail to
compile, with a counterexample.

Out of scope for v2.0: quantifiers, heap predicates, user-defined recursive
predicates, nonlinear arithmetic.

### 5.3 References

- `ref` and `ref mutable` parameters, as today.
- Borrowed results in outcome payloads, declared `from input` on public controls
  and inferred on private ones, as today.
- **Views**: built-in `slice<t>` and text views, and user types declared
  `view struct`, may hold borrows. A view behaves as one borrow with one origin
  set.
- Borrows and views are **second-class**: they live in parameters, locals and
  payloads. They cannot be stored in ordinary structs, variants or collections,
  and cannot be captured by a task that outlives its scope.
- A scoped task may borrow from its parent because the parent joins its children
  before leaving scope. This one rule replaces the shape-by-shape admissions of
  the async chapter.
- Origin groups, carriers, contained origins and reference evidence disappear.

What is given up: an ordinary data structure that holds borrows into another.
`library/traversal.vn`'s `window` becomes a `view struct`; nothing else in the
corpus is affected.

### 5.4 Surface

- **Acknowledgement.** Each alternative of a decision is named at the decision or
  covered by a named group. A bare name routes to the same-named path.
  `forward f(...)` requires the callee's contract to be a named contract or group
  included in the caller's; otherwise the labels are listed.
- **Outcome groups.** A named outcome declaration can be included in a contract
  and routed as a unit. A group is closed; extending it is an edit to one
  declaration that every including contract shows.

  ```text
  outcome storage_error {
      invalid_file { string message }, storage_failed { string message }, too_large
  }
  pub control get(ref string path, ref string key)
      exits found { string value }, missing, invalid_key, ..storage_error

  // today: four arms, repeated in three controls
  delta load(path) { loaded { values }, ..storage_error => exit }
  ```

- **Names-only patterns.** `failed { message }` binds; payload types come from
  the contract and may still be written.
- **Layout.** A label governs its own line and the block indented under it.
  `[entry]:` is no longer needed to resume the enclosing selection.
- **Live histories.** A statement without a label runs on every history that is
  still live, so code after an exiting arm needs no label. This is safe only
  together with the acknowledgement rule.
- **Boolean tests** are written `if`. `delta` is kept for operations with named
  outcomes.
- **Call-site marks.** Reading a value is unmarked. `mutable x` marks a call that
  may change `x`; `move x` marks the call that ends a resource.
- **Default path names.** A path is named after its operation and outcome, as in
  `read.failed`, unless the arm renames it.
- **Short associated names and method form.** `order.item`, `rule.compare(a, b)`
  and `values.get(key)` where the bound is unambiguous.
- **Contracts in source.** `extern` declarations replace JSON manifests for
  primitives, with outcome conditions, effects (`uses net.write`) and resource
  protocols.

Several conveniences already exist in v1 and are underused by the corpus: `exit
result;` for an identity adapter, plain statements for single-outcome calls,
named decision sites. The migrator should apply them.

### 5.5 Adopted surface: Rails

On 19 November 2026 the author adopted the **Rails** rendering wholesale as the
surface syntax of v2, after trying alternative glyphs in a playground and
keeping the defaults. It supersedes the word spellings shown in sections 5.2 to
5.4 and settles D4: structure is drawn, so a label's extent is the dashed rail
beneath it. The other decisions in section 3 were accepted the same day.

Canonical source text is the glyph form, rail characters included. The word
form remains the input spelling: every glyph has a word, and the formatter
stores the glyph. Six reference programs are in
[`experiments/syntax_lab/rails/`](../experiments/syntax_lab/rails/), produced
from the word form by `experiments/syntax_lab/glyphs/lab.js`.

| Role | Glyph | Word |
| --- | --- | --- |
| Decide on an operation | `Δ` | `delta` |
| Route an alternative | `→` | `=>` |
| Exit the control; also introduces the exits in a signature | `■` | `exit`, `exits` |
| Leave a loop with an outcome | `□` | `leave` |
| Loop, and go round again | `↻`, `↑` | `loop`, `again` |
| For each element | `∀ x ∈ xs` | `for x in xs` |
| Boolean test | `? c →` | `if c` |
| Must be proved here | `⊢` | `where` |
| Outcome occurs when | `⇐` | `when` |
| Cannot occur | `⊥` | `impossible` |
| Shared reference, mutable, move | `&`, `~`, `▾` | `ref`, `mutable`, `move` |
| Borrowed from | `∈` | `from` |
| Bind a name | `name: type := value` | `type name = value` |
| Function value, status mapping | `↦` | `(x) => e`, `@exit` |
| Vector and map types | `[T]`, `K ⇀ V` | `vector<T>`, `map<K, V>` |
| Named group of outcomes | `…group` | `..group` |
| Compare and combine | `= ≠ ≤ ≥ ∧ ∨ ¬ ÷` | `== != <= >= && \|\| ! /` |
| Empty map | `∅` | `map {}` |
| Body rail and its end | `│`, `╵` | braces |
| Loop rail | `║` | braces |
| Path label and its rail | `╞═` (`╠═` inside a loop), `┆` | `[a & b]:` |
| Alternatives of a decision | `├`, `└`, or `: a │ b` on one line | `{ a, b }` |
| Comment | `┄` | `//` |

Words that stay words: `control`, `function`, `outcome`, `import … as`, `pub`,
`async`, `extern`, `resource`, `protocol`, `uses`, `emit`, `spawn … into`,
`await`, `next`, `cancel`, `capture`, `run`, `forward`, `auto`, `clone`.

Consequences for the plan:

- The formatter owns the rails. It is needed with the parser in phase 3, not as
  a late convenience in phase 7.
- The language needs a blessed font. DejaVu Sans Mono covers every glyph above;
  Menlo lacks `⊢` and `⊥`; SF Mono, Cascadia Code and Monaco miss most of the
  arrows and logic signs.
- Box characters join only at the font's natural line height (1.164 em for
  DejaVu Sans Mono), so the editor extension must set it.
- Thin arrows such as `⇥` and `≔` are illegible at code size and are not used.
  Characters with emoji variants are avoided.

## 6. Risks

- **Second-system scope.** Contained by vertical slices, by keeping v1 shippable,
  and by exit gates that are measurements.
- **Solver predictability.** Contained by a decidable fragment, a deterministic
  built-in procedure and certificates. A missing proof is always a compile error
  with a counterexample or an "outside the fragment" message, never a timeout.
- **Annotation burden from `where` clauses.** Measured on the paper corpus in
  phase 1 before anything is built. If invariants are needed often, inference
  from templates is added in phase 3.
- **Expressiveness lost with second-class borrows.** Measured on the paper
  corpus. D1's alternative remains available if a real program needs it.
- **Loss of working features during the transition.** v1 stays buildable and
  remains the oracle until v2 reaches parity on the corpus.

## 7. Prior art the plan relies on

- Refinement types over ownership: Liquid types; Flux for Rust.
- Proof of absence of run-time errors as a compile condition: SPARK.
- Second-class references and mutable value semantics: Hylo.
- An instrumented interpreter as the definition of validity: Miri.
- Faults separated from expected outcomes: Midori's error model; Erlang
  supervision.
- Calls with several return points: Shivers and Fisher, "Multi-return function
  call".
- Typestate: Strom and Yemini; Vault; Plaid.
- Equivalence and inclusion: Kozen's Kleene algebra with tests and its treatment
  of nonlocal control; guarded KAT (Smolka and others, 2020); KAT with mutable
  tests (Grathwohl, Kozen and Mamouras); bisimulation up to congruence (Bonchi and
  Pous); symbolic KAT (Pous); KATch for NetKAT.
- Untrusted producer with trusted checker: translation validation and
  proof-carrying code.
