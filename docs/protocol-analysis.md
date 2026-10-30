# Structural protocols and residual queries

Introduced in compiler 0.15.0; source-fault terminals added in 0.62.0.

Tellur can now build a finite protocol model and answer: **after this prefix, what may
happen next?** It also compares the terminated finite trace languages of two such
models and produces counterexamples when they differ. This implements a first
restricted form of the residual operation proposed in the vision.

The model deliberately forgets data. A possible abstract trace may be impossible
for every concrete input. Equal abstract protocols do not establish equal programs,
compatible types, safe API substitution or equivalent infinite behavior.

## Ask what follows a failed read

From the compiler directory:

```sh
./bin/tellurc protocol playground/read_file.tellur playground/manifest.json show_file playground/protocol/read_failed.json
```

The prefix file contains:

```json
[
  {"kind":"primitive","operation":"open","outcome":"opened"},
  {"kind":"primitive","operation":"read","outcome":"failed"}
]
```

The next symbols are `close → closed` and `close → failed`. There is no direct public
exit after this prefix. The model reflects the source's cleanup ordering. This
result concerns operation/outcome traces; it does not independently prove a foreign
handle implementation obeys its resource protocol.

Omit the prefix filename to export the complete graph as JSON. Edge symbols of
`null` are silent transitions. Each edge also identifies its expanded instruction;
`tellurc expand` can inspect the corresponding graph and provenance. The model does
not execute the program or perform its file operations.

The residual report includes:

| Field | Meaning within this abstraction |
| --- | --- |
| `prefix_possible` | Some path can consume precisely this token prefix |
| `complete` | The consumed prefix includes a public exit and is a completed word |
| `can_complete` | Some further finite suffix reaches a public exit |
| `faulted` | The consumed prefix ends with a source-fault terminal, not a normal exit |
| `can_fault` | Some finite suffix reaches a source fault |
| `next_symbols` | Tokens enabled after any number of silent steps |
| `may_silently_diverge` | A silent cycle is reachable without another token |
| `residual_states` | The remaining nondeterministic state set after silent closure |

A false `may_silently_diverge` does not rule out infinite executions containing
observable events. A true value is only an abstract possibility. Run the terminating
`playground/protocol/finite_retry.tellur` and query it with
`playground/protocol/empty.json`: forgetting the loop's Boolean update also permits
an endless silent retry in the model.

## What is retained and forgotten

The model is built from independently validated expanded IR. Its state is a control
position and a finite set of active decision facts. It retains guards, operation
order, outcome alternatives, loop entry/restart resets, completion targets and the
public exit or source-fault terminal. Loops are cycles, not a fixed number of unrolled iterations.

Its finite alphabet is:

- `{"kind":"emit"}`: an emit, with its text forgotten.
- `{"kind":"primitive","operation":"name","outcome":"label"}`:
  a non-pure primitive completion, with arguments, payloads and effect annotations forgotten.
- `{"kind":"exit","outcome":"label"}`: the final public completion, with payloads forgotten.
- `{"kind":"fault"}`: source-fault termination, with its diagnostic message forgotten.

Pure expressions, pure primitive decisions and collection operations are silent.
Their declared decision alternatives are retained nondeterministically. Boolean
and stored-outcome decisions likewise permit every declared alternative, even when
a literal or previously constructed tag would determine one concretely. Bounds,
arithmetic, data correlations, ownership state and externally visible store contents
are not represented. The source must still pass the normal checker before analysis.

For each well-defined concrete transition, the intended abstraction argument either
keeps its projected event or takes a silent step; a concrete decision chooses one
of the retained alternatives. The control facts and loop resets follow the concrete
rules. This motivates an overapproximation of projected successful behavior.
That soundness argument has not been mechanized; the implementation is tested by
projecting interpreter executions into accepted model words.

The graph builder returns a complete model or `E_PROTOCOL_LIMIT`, never a truncated
success. The CLI permits 20,000 states per model. The Rust API accepts a state limit.
The state space can grow exponentially with independent decision facts; a budget
error says nothing about the analyzed program's behavior.

## Residuals and language inclusion

Let `E(S)` be the silent closure of a set of states and `step(S,a)` its destinations
on token `a`. Starting at the initial state's closure, consume a prefix with:

    S_next = E(step(S, a))

For the language `L` of finite words ending in a normal exit or source fault, this remaining state set represents
`L / h = { s | h;s belongs to L }`. It also represents possible finite path prefixes,
which need not extend to a completion. The report keeps those two questions separate:
a silently diverging model can have a possible prefix and no completing suffix.

```sh
./bin/tellurc protocol-compare playground/algebra/original.tellur playground/manifest.json playground/algebra/identity.tellur main
```

The command checks inclusion in both directions. Each direction explores reachable
pairs of nondeterministic state sets, closing silent transitions before and after
each token. A pair accepting on the left but not on the right gives a counterexample.
Breadth-first search returns a shortest counterexample by token count. Exhausting
all reachable pairs establishes inclusion for these finite models. The CLI permits
20,000 pairs per direction and returns an inconclusive error if that limit is reached.
A completed comparison returns nonzero when either direction fails.

Counterexamples are abstract words, not reconstructed input values or concrete
executions. Only finite words ending in a public exit or source fault are compared.
Two models with neither form of termination have equal empty terminated languages even if one emits forever
and the other silently loops. Unused declared exits, parameter/payload types, and
infinite-trace or fairness properties are outside this comparison.

There is a useful deliberate counterexample in the playground: compare
`original.tellur` with `reordered.tellur`. `tellurc compare` finds a concrete difference in
emit order. `protocol-compare` accepts both inclusions because the messages have
been erased and both produce two indistinguishable emit tokens before their exits.
The JSON claim is `inclusion_of_completed_structural_traces_only` when neither
model has fault terminals, or `inclusion_of_terminated_structural_traces_only` when
either does. A declared `faults` permission alone does not add a graph edge: this
model follows the checked body. Fault-permission contracts are compared separately
by contract snapshots and finite scenario comparison.

## Relationship to transformation certificates

The three checks answer different questions:

- Expansion certificates validate an explicit graph rewrite correspondence.
- Scenario comparison checks concrete observations for supplied executions.
- Protocol analysis computes properties of a declared finite abstraction, including
  arbitrarily long words admitted by its cycles.

Protocol equality cannot replace an expansion certificate: it intentionally forgets
payloads, mutation, borrowing and much provenance. Conversely, certificate validation
does not make a coarse abstraction precise. Future refinements could retain selected
finite values or effect labels, but must state their new abstraction and inclusion
claims explicitly.
