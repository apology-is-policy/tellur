# Expansion: a conditional simulation argument

Compiler 0.15.0, 25 October 2026.

This is a handwritten argument for the existing sequential expansion rules, tied
to `runtime.rs`, `expand.rs` and `certificate.rs`. It is not a mechanized proof of
the language or compiler. It identifies the state relation and the local proof
obligations that a formal development would need to discharge.

## Scope and assumptions

Consider well-defined executions of a checked program in the current acyclic-call,
structured-loop subset. Ignore host fuel limits, allocation failure and timing.
Assume that the front end correctly establishes availability, move/loan safety,
resource completion, exit coverage and the absence of enabled work after a source
exit. Foreign operations respect their declared contracts and do not observe
compiler slot identities. Ordinary storage destruction has no user-defined visible
effect. Recursive calls, async scheduling and stored borrowed aggregates are outside
this argument.

These assumptions are substantive. In particular, front-end soundness and foreign
resource identity discipline have not been proved by the expansion certificate.
The certificate verifies the syntactic derivation of a rewrite under those assumptions.

The conditional preservation claim is that expansion keeps the ordered observable
events and public completion, modulo renaming internal addresses, and does not add
or remove divergence. The argument below explains why the rules support that claim;
it does not replace the outstanding formalization and proof work.

## Related states

Represent a source machine configuration by its active call/loop frames, continuation
positions, value store, per-call decision facts and event prefix. The interpreter
implements this through recursive `invoke`/`execute` functions and a shared store.
Acyclic calls ensure that the same control is not simultaneously active through
recursive invocation. Slots are globally identified in the checked source.

For every active source frame, use its certificate instance map `rho`. Relate that
frame to its expanded storage as follows:

1. Each available source slot maps to a distinct expanded slot of the same type.
   Scalars, tags and opaque resource tokens agree. Aggregate fields correspond
   recursively. Moved-out fields and unavailable slots correspond too.
2. A reference to `(root, fields)` maps to `(rho(root), fields)` for an owner in
   that active frame, or through the appropriate enclosing owner's map. The
   relation is on owner locations, not on the slot that happens to hold a reference.
3. Local decision facts correspond under the atom map. Caller facts remain present
   in the expanded machine; fresh local identities cannot alias those facts.
4. The expanded control position lies at the mapped source instruction or within
   the finite administrative sequence for argument binding/completion.
5. The event prefixes agree. Outstanding ownership, loan and handling obligations
   correspond as ghost state; they are not additional runtime counters.

Inactive fresh storage and private facts left after a completed invocation are
irrelevant to the caller, except for its explicit outcome atom and payload slots.
If a containing loop repeats, the expanded reset tables clear its introduced local
atoms and storage before another activation.

## Expression substitution

The key local lemma is that evaluating a substituted expression in related stores
produces related values and updated stores. It needs both value and store results:
a `Move` expression changes availability.

- Literals and ordinary scalar operators retain their values.
- Place lookup follows the same field indices in corresponding owners.
- Whole-owner moves remove corresponding roots. Partial moves replace corresponding
  fields with the interpreter's moved-out marker; later use is excluded by checking.
- Reference construction and dereference resolve the owner address before applying
  field projections, preserving aliasing under the injective location map.
- Constructors and pure operations evaluate corresponding operands in the same
  order. Boolean short-circuiting takes the same branch.

These are the cases of `evaluate`; a full proof would induct on its expression
structure and explicitly formalize partially moved aggregates and reference validity.
The checker validates the same expression structure under substitution, including
move/reference distinctions, rather than merely matching result types.

## Instruction cases

**Guards.** On a called frame, the expanded guard is `caller_context & rho(local_guard)`.
The caller context is true when the source invocation runs and stable while the
callee executes: new local decisions use fresh atoms. Therefore the enabled source
instructions and their copies agree. Disabled instructions do not evaluate values.
BDD comparison in the certificate checks Boolean equality of guard forms.

**Call entry.** Evaluate source arguments left to right. Expansion binds corresponding
callee parameter slots in the same order under the call guard. Destination slots
are fresh, so these writes cannot change a later argument's caller source. This
finite binding sequence is unobservable. In particular, expansion does not move an
eager captured invocation to its eventual handling site.

**Ordinary work and decisions.** Assignments, drops, emits and primitive calls retain
their order. The expression lemma preserves their values and mutations. Given the
same admissible primitive response, the same outcome atom and corresponding payload
slots become available. First-class outcome decisions retain their tag and mandatory
handling semantics established before expansion.

**Completion.** A source exit evaluates its payload before `invoke` removes the
callee's storage and transfers the result into the caller's arm. Expanded `Complete`
also evaluates all payload expressions first, writes the caller destinations,
releases callee storage and establishes the caller outcome atom. Its release set
cannot include caller owners because the instance map is fresh and disjoint.

Returned references therefore still point into a live input owner. A local reference
escape is forbidden by the source checks, not repaired by expansion. Moving a
resource payload transfers the same token and obligation; completing a call does
not justify abandoning unrelated obligations.

For a completion outside a callee loop, remaining copied instructions on that
terminated history are disabled by the source coverage/disposition invariant. They
are finite silent steps. For a callee-loop exit, the completion's unwind target
leaves that callee loop and resumes the caller continuation. It must not restart the
callee or terminate a containing caller loop. Existing completion targets are renamed
when a previously expanded helper is expanded again.

**Loop restart.** The source evaluates all restart arguments before clearing local
storage/facts and installing the next parameter values. Expansion retains that
simultaneous-transfer structure. Its reset tables additionally include the slots
and atoms introduced by expanded calls inside the loop. Thus one iteration cannot
reuse a private decision fact from an earlier iteration. The loop target map
preserves the structured control transfer admitted by the current language.

## Source-fault extension (0.62)

The checker now also establishes fault permission and disposition requirements for
live resources. A copied `Fault` instruction evaluates the substituted message and
terminates the whole invocation; expansion never rewrites it to the normal `Complete`
join. Under the expression lemma, the diagnostic message and preceding event prefix
agree. No caller continuation or normal outcome follows. Certificates check that the
instruction and enclosing fault permission are retained; mutation tests reject their
erasure or replacement by a normal exit. Native tests also exercise ordinary retained
owner teardown before and after expansion.

This extends the finite reference-observation argument. It does not prove externally
observable destructor ordering, cleanup success or isolation: Rust destruction is a
trusted host contract, and actual activation stacks/source sites can differ under
expansion. Activation-owned storage and richer cleanup protocols need their own
preservation argument. The earlier assumptions above remain limits of this draft.

## Finite execution and divergence

The local cases suggest a weak simulation: one source step may match several silent
expanded steps, with matching observable actions. The reverse direction contracts
those same certified bindings, copies and completions. Injective maps and fixed
instruction order prevent independent effects from being exchanged or duplicated.

Silent stuttering needs a progress argument. Each acyclic call expansion and each
straight-line suffix after completion is finite. Expansion introduces no new
backedge; repeated control transfer still passes through a source loop restart.
Consequently, an infinite run cannot be explained solely by newly inserted
administrative work. A formal proof should make the finite administrative rank and
its reset at a matching source step explicit, establishing the divergence-sensitive
relation in both directions.

A host step budget is not invariant under this correspondence: expansion adds and
removes interpreter steps. An `E_FUEL` result is therefore inconclusive, not a language
outcome or proof of divergence. Native lowering is another transformation with its
own proof obligations, even though differential tests exercise it.

## Evidence and remaining obligations

Certificate mutation tests check the concrete premises involving identity maps,
ordering, payloads, release, loop reset/unwind and provenance. Existing differential
tests cover original/expanded executions with moves, returned borrows, resources,
loops and stored outcomes. They provide finite evidence, not universal quantification.

Remaining formal work includes an explicit small-step machine, the expression/store
lemma, preservation of the source ownership and disposition invariants, and a
well-founded administrative rank for the two simulation directions. A future proof
assistant model must account for these rather than treating the certificate's
acceptance as a semantic theorem. Extraction additionally needs a checked graph cut
that establishes the boundary this argument currently assumes.
