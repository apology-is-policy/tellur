# Villanelle v2 design

Phase 1 of the [redesign plan](../redesign-plan.md): the design on paper. No
compiler code belongs to this phase. The documents here become normative when the
author accepts them; until then they are drafts, and nothing in them is
implemented.

v1, the language and compiler at 0.306, is described by the
[language reference](../language-reference.md) and stays as it is.

| Document | Deliverable | State |
| --- | --- | --- |
| [Core calculus](core.md) | The core language, its instrumented semantics, the checker's judgments, and the claims to test and mechanize | Draft 1, synchronous language; tasks in draft 2 |
| Fact logic | The interpreted fragment behind `⊨`, the decision procedure, certificates, counterexamples, worked examples | Not started |
| Reference model | Second-class borrows and views as normative text with worked examples | Not started; the rules are in the core calculus, sections 6.3 and 7.5 |
| Surface | The Rails grammar, layout, and the elaboration into the core, including the acknowledgement rule and outcome groups | Not started; the adopted glyphs are in the plan, section 5.5 |
| Corpus on paper | The settings application, the eight collection modules and three new programs, rewritten by hand, with counts | Not started |
| Reading and repair test | v1 and v2 versions of the same programs, given to people and to agents | Not started |

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

1. Core calculus, draft 1 (this checkpoint), then its review.
2. Fact logic, since the worked examples of every other document depend on what
   can be proved.
3. Surface and elaboration, then the reference model as examples in that syntax.
4. Core calculus draft 2: tasks.
5. The corpus on paper, which tests all of the above and yields the counts the
   plan asks for (lines, labels, filler arms, annotations).
6. The reading and repair test.
