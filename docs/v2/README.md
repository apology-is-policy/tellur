# Villanelle v2 design

Phase 1 of the [redesign plan](../redesign-plan.md): the design on paper. The
author accepted it on 20 November 2026, and the documents here are the working
specification of v2 ([Acceptance](#acceptance)). Nothing in them is implemented
yet.

v1, the language and compiler at 0.306, is described by the
[language reference](../language-reference.md) and stays as it is.

| Document | Deliverable | State |
| --- | --- | --- |
| [Core calculus](core.md) | The core language, its instrumented semantics, the checker's judgments, and the claims to test and mechanize | Draft 2, accepted. The synchronous language has two reviews folded in. Tasks and named boundaries were read by the combined review, which found four programs that the task rules accepted and the semantics left stuck, and no checking rule for named boundaries; the repairs are in and not yet reviewed again. Records the decisions on Q1 to Q5 and Q18. |
| [Fact logic](facts.md) | The interpreted fragment behind `⊨`, the decision procedure, certificates, counterexamples, worked examples | Draft 1, accepted; two reviews folded in. Records the decisions on Q6 to Q10. The corpus added the length of a vector literal. |
| [Reference model](references.md) | Second-class borrows and views: the model, 36 worked examples in Rails and core form with verdicts, the tag semantics on an example | Draft 1, accepted; one review folded in, which found the 36 verdicts to agree with their clauses. Records the decisions on Q16 and Q17. |
| [Surface](surface.md) | The Rails grammar, layout, and the elaboration into the core, including the acknowledgement rule and outcome groups | Draft 1, accepted, with the task forms of the core's draft 2 and the forms the corpus needed (traits, named boundaries, `copy` types); one review folded in. Records the decisions on Q11 to Q15 and Q19. |
| [Corpus on paper](corpus.md) | The settings application, the eight collection modules and three new programs, rewritten by hand, with counts and findings | Draft 1, accepted; checked by hand and read by one review, which found no error in the programs. Library 476 to 231 lines, application 635 to 304, no filler arm; thirteen gaps repaired in the other documents, thirteen limits left open. |
| [Oracle](oracle.md) | Phases 2, 3 and 4: the instrumented semantics, the Rails surface reader, elaborator and formatter, the certificate-producing checker, call expansion (Theorem 4) and the typed LLVM IR and C ABI backend (the crate `v2/`), with the conformance suite | Slice 1 complete across phases 2, 3 and 4: the scalar core runs in the oracle, under the erased semantics ([R1] to [R3]) and compiled to native code through the typed LLVM IR backend at `-O0`, `-O2` and `-O3`, with the differential harness, the seeded host, the random program generator (61 constructs, 12 statement rules), the mutation classifier (all 15 stuck states), property tests of axioms [A7] to [A9], the Rails surface reader, elaborator, canonical formatter (`vn2 format`, [G10]) and compiler warnings ([U5], [U16], [U17], [U18]), the phase 3 checker ([K1] to [K8], [N1] to [N5], [C2], [O1], [O2], [O4], [O6], [P1] to [P8], [F3]) with the ROBDD engine, the `Th_Z` fact logic, the independent certificate kernel ([V1] to [V5]) and counterexample model builder ([M1] to [M6]), in-place call expansion (`vn2 expand`, Theorem 4), and the phase 4 typed LLVM IR and C ABI backend (`vn2 emit-llvm`, `vn2 emit-c`, 0.991x optimized Rust on paired M2 scalar kernels with 0 heap allocations). Slice 2 (data, moves, resources, captured outcomes, faults and boundaries) is next. |
| [Reading and repair test](reading-test.md) | v1 and v2 versions of the same programs, given to people and to agents | First run with agents done: 24 runs, 23 fully correct, so no difference between the versions is shown on reading. The repair needs 8 edits in v2 against 14. People have not taken it. |

## Acceptance

The author accepted phase 1 on 20 November 2026, and with it the choices that
each document lists and the recommendation of every question, Q1 to Q19 (Q1 to
Q5 on 19 November). The documents are normative for the phases that follow.
They are corrected, with a note of what changed, when the oracle, the checker
or a review finds an error. A change to an accepted decision goes to the
author.

Three things were open at acceptance, and the author accepted with them named:

- **The task rules.** The repairs that the combined review led to have not
  been reviewed again: [TK3] to [TK6], [E-Await], [E-AwaitHost], [E-Stop],
  [N5], [F5] and [F6] of the core, [J3] and [I10] of the fact logic, [G3] of
  the surface. They get a second review before the task slice, which is the
  last of the four.
- **Workarounds.** The plan's exit asks that the corpus show no construct that
  v2 cannot express without a workaround. Three of the thirteen open limits of
  the [corpus](corpus.md) (section 6.2) are workarounds until phase 5:
  consuming a vector element by element (O1), receiving a single result
  through a decision (O2) and disposing of an empty vector (O5).
- **The reading test.** It showed no difference between the versions in either
  direction, with agents only. People have not taken it.

## Conventions

- Program text is written in Rails, the adopted surface syntax, and placed in
  `text` fences so that the manual checks do not try to compile it. The
  reference for how Rails looks is
  [`experiments/syntax_lab/rails/`](../../experiments/syntax_lab/rails/).
- Core programs use the same glyphs without sugar, plus `▷` between a selector
  and the command it guards.
- Normative clauses carry identifiers in brackets. The phase 2 conformance suite
  takes an accepting and a rejecting test from each.
- A choice that goes beyond the accepted decisions is listed at the end of the
  document that makes it, as a question for the author with a recommendation.

## Order of work

1. Core calculus, draft 1, and its reviews (done).
2. Fact logic, since the worked examples of every other document depend on what
   can be proved (draft 1 done).
3. Surface and elaboration, then the reference model as examples in that
   syntax (draft 1 of both done).
4. Core calculus draft 2: tasks (done).
5. The corpus on paper, which tests all of the above and yields the counts the
   plan asks for (lines, labels, filler arms, annotations) (draft 1 done).
6. One independent review of all five documents together, for consistency and
   soundness (done; the repairs it led to have not been reviewed again).
7. The reading and repair test (first run with agents done; people are the
   author's part and open).
8. The author's acceptance of phase 1 (20 November 2026).
9. Phase 2 in slices: the oracle, erased semantics, generator, differential
   harness, mutation tests and Rails surface reader with elaboration for
   scalars and decisions (done).
10. Phase 3 for slice 1: the checker on the typed core with ROBDD histories,
    the `Th_Z` fact logic, the independent certificate kernel, counterexample
    models, the canonical Rails formatter and compiler warnings (done).
11. Phase 4 for slice 1: Theorem 4 call expansion and the typed LLVM IR and C
    ABI backend under the erased semantics ([R1] to [R3]), verified against the
    oracle and the erased interpreter at `-O0` and `-O2` (original and expanded)
    and measured against optimized Rust on paired scalar kernels (0.991x on
    Apple M2, 0 allocations) (done); slice 2 (data, moves, resources, captured
    outcomes, faults and boundaries) is next.
