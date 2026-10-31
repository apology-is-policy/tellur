# Accepted control-flow and ownership revision

This is the accepted direction following 0.63, not a claim that every spelling
below is implemented. The language reference remains the source for executable
syntax. Work is proceeding through the checkpoints below.

## Checkpoints

1. **Delivered in 0.64:** identity-routing flat deltas, qualified decision-site paths, and grouped arm
   statements. Omitted arms import their producer's outcomes and payload names;
   every resulting history must still have a disposition. An arm body adds values
   on its selected path; it does not replace the outcome's identity or schema.
2. **Delivered in 0.65:** explicit `move`, nested owning construction and call-duration
   temporary borrows for ordinary reference-free values with no escaping result loan.
   Generic forwarding preserves ownership/reference mode. Moves do not clone;
   eliminating physical storage transfers is a separate lowering objective.
3. **Delivered in 0.66:** lexical `fault boundary { ... } contingency [...] { ... }` and
   `supervised loop name { ... }`. `recover again name;` is a terminal recovery
   edge to the designated enclosing supervisor, never a general path jump.
4. **Delivered in 0.66:** structured handler-local `@fault`, explicit process termination, retained-owner
   capture and request-resource completion. Failed frames and loans retire before
   contingency access. A handler cannot return an ordinary outcome or fall through.
5. Constructor declarations with definite field initialization and partial-failure
   cleanup; direct destination construction where the ABI and ownership permit.
6. **Delivered for checkpoints 3–4 in 0.66:** reference, editor, formatter,
   native/reference parity and settings-server migration. Repeat for constructors.

Each delivered checkpoint must update this list and the language reference with
its actual scope and limitations. Existing named 0.63 boundaries remain a
compatibility form while lexical boundaries are introduced.

## Accepted boundary policies

The author accepted entry-only process termination and state-preserving recovery.
Both are implemented in 0.66. Ordinary outcome completion, administrative boundary
settlement, recovery and process termination have distinct runtime representations.

- `terminate process N;` belongs only to an entry's outermost lexical contingency.
  The containing control is entry-only and cannot be called as a library control.
- `recover again requestor;` preserves supervisor state at boundary entry and
  discards the failed iteration's locals. It accepts no replacement arguments.
- The first lexical slice rejects direct access to supervisor values inside work;
  prepare owned inputs before entry. Reference captures and nested tail loops
  remain unsupported. Diagnostic `@fault` exposes bounded message/stack/sites data.

With retained supervisor state, an accept budget or listener belongs to the
supervisor; an accepted connection belongs to the protected request. Any deliberate
budget update happens before entering the request boundary. Recovery must neither
silently reset that budget to its initial value nor carry values computed by the
failed request into a new iteration. Moving a request connection into contingency
does not confer permission to reuse it for another request.

## Recovery invariants

- A fault retires ordinary continuations up to the nearest active boundary.
- Contingency can use only its captured capabilities, local values and explicitly
  permitted operations. Ordinary outcomes of those operations stay inside that
  contingency; they cannot complete the failed work.
- `recover again` starts a fresh supervised iteration after resource obligations
  are settled. It cannot export failed-work payloads, fault reports or references
  to retired storage. Ordinary `exit` from contingency remains forbidden.
- A secondary fault or escalation leaves the current handler and reaches an outer
  boundary. A handler never catches its own failure.
- Capture is established at boundary entry. Required resources must already exist;
  construction failures belong to an enclosing boundary. Owning a resource or
  ending a loan does not prove a network/application protocol is healthy again.
- Initial recovery uses owned request resources. Reusing outer borrowed mutable
  state needs an explicit recovery contract; blanket reference capture is not an
  implied permission.
- Expected connection/decoding failures remain ordinary outcomes. A root fault may
  terminate the process with a status, but cannot manufacture an entry outcome.
- Recovery necessarily permits abandoning work and starting new work. Restrictions
  govern the destination, resources and data crossing that edge, not author intent.

## Syntax and ownership decisions

`supervised` is the adjective on the loop declaration. Path labels stay unbraced
unless explicit grouping helps. Named decisions provide qualified path selection,
such as `[first.has_image]`; ambiguous short names require qualification. Implicit
payload binding makes producer payload names part of the source interface.

Normal completion without a payload remains distinct from having no normal
outcomes. `exits done faults` may return; a control with only `faults` cannot.

An owning parameter receives ownership from a named owner or temporary. `ref`
continues to mean a borrow, never a consuming rvalue reference. Explicit `move`
must respect liveness, outstanding loans and must-complete obligations. Temporary
borrows require a backing owner and cannot escape its lifetime; deferred/async jobs
must retain any backing owners they need.

Constructor syntax does not make malformed external data an invariant fault.
Fallible decoding uses explicit normal outcomes. The optional `byte[]` spelling
needs one precise storage meaning before adoption; it is not permission to merge
owned buffers and borrowed views. Method lookup and arbitrary annotation policies
are not prerequisites for this revision.

The runtime remains synchronous initially. Lexical boundaries should lower to
isolated activations that can later suspend and settle child work; no implicit
event loop or new machine stack is required by the source semantics.
