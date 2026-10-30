# Tellur semantic foundation — draft 3

25 October 2026. Compiler checkpoint 0.16.0.

This document separates the intended mathematical model, executable checks, and
remaining proof obligations. It is a semantic design draft, not a machine-checked
soundness proof or a claim that arbitrary program equivalence is decidable. The
original specification and vision remain unchanged. Concrete observation comparisons
are evidence for the supplied scenarios only.

## 1. Four different relations

| Relation | Meaning | Current evidence or mechanism |
| --- | --- | --- |
| Outcome-shape embedding | Every source label occurs in the target with the same ordered payload types | `algebra::outcome_embedding`; used by tail forwarding and typed capture |
| Safe boundary transfer | Shape embedding plus actual value availability, ownership, resource obligations and borrow origins | Existing call/decision/exit checker; no separate general soundness proof |
| Observational equivalence | Equal permitted observable behavior in every admissible common context/environment | Intended semantic relation; finite comparisons are not its decision procedure |
| Explanatory correspondence | A transformation retains a mapping between relevant definitions, invocations, decisions and exit contributors | Structured provenance in IR/expansion; explicit regression assertions |

Named outcome types are nominal: two differently named types do not become the same
value type because their shapes agree. An explicit adapter or typed capture can
perform a checked boundary mapping. Field names document positional payloads; they
are not runtime record-field identities within an outcome tuple.

## 2. Objects at a boundary

Write a closed output boundary as a labelled sum:

    O = sum over l in L of (l × T_l,1 × ... × T_l,n)

For example, `ready { string text }, empty` has two alternatives. Only one is selected
on a returning invocation. Empty payloads still have a label and a disposition.

An actual boundary includes more than this sum. Use the schematic descriptor
`B = (V, ownership, loans, obligations, admissible effects)`, where:

- `V` contains ordered values and their types; public results may transfer owners.
- Ownership distinguishes Copy values, movable owners and shared/exclusive loans.
- Loans identify the owners or input-origin sets that keep references valid.
- Obligations include must-complete resource protocols and unhandled outcome values.
- An effect/environment contract constrains externally visible operations.

The implementation has concrete ownership checks and per-outcome borrow-return
summaries, but not a general source-level effect-contract calculus. Merely listing
primitive effects does not prove that two effects commute or can be rolled back.

A named or captured outcome has one sealed handling obligation even if its payload
is empty. Deciding it discharges that obligation and exposes any payload obligations.
Moving it transfers the obligation. Ordinary owned data may be destroyed when its
scope ends; a must-complete protocol may not be abandoned. An identity adapter moves
owning payloads and preserves borrowed origins; it does not clone them.

### Outcome-shape embedding

`O <=shape O'` holds when every label of `O` appears in `O'`, with equal payload
arity and exactly equal ordered types, including reference modes. It induces an
index map from source alternatives to target alternatives. Extra target alternatives
are permitted. There is no implicit renaming, coercion, cloning, or narrowing.

For checked boundaries with distinct labels:

- Reflexivity: `O <=shape O`, with the identity index map.
- Transitivity: if `O <=shape O'` and `O' <=shape O''`, their index maps compose to
  the unique label-preserving map `O <=shape O''`.

These follow directly from label uniqueness and transitivity of exact type equality.
The executable law suite checks concrete embeddings and rejects duplicate labels,
missing alternatives, arity changes and payload-type changes. This shape relation
alone says nothing about lifetime safety, effects or behavioral refinement.

## 3. Sequential execution and observations

The intended small-step state includes control position, active frames, value/store
locations, current decision facts, and the corresponding ownership/obligation state.
Ownership and loan components may be ghost state: runtime enforcement is not required
when checking has established their invariants. Loop iteration starts with fresh
iteration-local facts; old iterations do not make later source selectors true.

Transitions are internal steps or observable events. A decision invokes/evaluates
its operand once, establishes exactly one alternative, and transfers that payload
into the selected scope. A union guard runs a statement once when the guard is true.
A call completes into its caller's selected continuation. A callee exit becomes a
caller exit only when the caller explicitly forwards it.

For the current deterministic scripted interpreter, the executable observation is:

    (ordered events, final outcome label, final payload)

Events record emits and non-pure primitive calls, including arguments, selected
outcomes, returned fields and declared effects. A valid script and primitive
contracts are assumptions of the experiment. Script-supplied resource identities
remain trusted. Private slots, allocation addresses and inserted adapter nodes are
not public observations. Pure implemented builtins have no primitive event.

The eventual semantic observation model must additionally account for externally
observable final state, protocol behavior, and termination/divergence. Borrowed
controls are tested through an owning caller that observes relevant mutations.
Current finite reports do not claim to capture every possible external observation.

Define `Beh(P, input, environment)` over maximal behaviors: finite completed traces
with results, explicitly modelled host failures, and divergent behaviors. Divergence
can have infinitely many observable events or eventually only internal steps. It
must not be equated with a successfully returning finite prefix. Timing, allocation
failure and unspecified host failures require explicit modelling before laws about
them can be claimed.

For a declared observation projection `Obs`, the intended relation is:

    P ≃obs Q  iff  Obs(Beh(P, i, e)) = Obs(Beh(Q, i, e))
                   for every admissible common input i and environment e.

Behavioral refinement uses inclusion instead of equality, under a fixed common
boundary and observation model. For API substitution with different boundaries,
input admissibility and output embeddings must also be specified. An outcome-shape
embedding is not a behavioral-refinement proof. Maximal behaviors or an equivalent
termination-sensitive model are needed: inclusion of finite prefixes alone can hide
divergence.

## 4. Typed composition and identity

Write `wire(P, H)` for connecting every outcome `l` of `P` to a continuation `H_l`.
Each handler receives exactly the payload and permissions it is entitled to use.
There must be a disposition for every declared alternative. The composition retains
P's execution order and invokes only the selected handler.

The identity handler `id_O` forwards each label with its unchanged payload and
permissions. The intended laws, under compatible boundaries, are:

    wire(P, id_O) ≃obs P
    wire(id_I, H) ≃obs H
    wire(wire(P, H), K) ≃obs wire(P, {l -> wire(H_l, K)})

The second equation treats an incoming labelled boundary `I`; it does not mean that
calling an arbitrary effectful producer is an identity. These laws re-associate
wiring without exchanging, duplicating or deleting effects. Fresh local identities
and appropriate scope/loan mappings are required. No implicit destruction of an
unrelated resource is justified by a right-identity law.

Counterexamples are part of the law suite:

    emit("a"); emit("b")  !=  emit("b"); emit("a")
    emit("a"); emit("a")  !=  emit("a")

Moving a capture across a later emit also changes the event sequence. An identity
adapter around a stored result cannot move the producer's invocation to the later
handling site. Semantic names may remain as provenance annotations when a runtime
identity adapter is eventually optimized away.

## 5. Masks, program alternatives and trace queries

Current masks form a Boolean algebra over structural history facts. Union and
intersection are idempotent and satisfy distributivity. BDD representation checks
these facts symbolically; it does not prove that every structural combination can
arise from concrete data. Compiler witnesses remain structural possibilities.

There are different operations on different domains:

- `A | B` in a selector is a predicate union; shared work executes once.
- Alternative composition in a trace model is union of behavior families.
- `P ; Q` composes compatible executions in order.
- `P*` in a regular abstraction denotes zero or more finite repetitions.
- Trace intersection, hiding and residuals belong to a query/abstraction layer whose
  alphabet and universe must be stated. They are not arbitrary executable operators.

Kleene algebra with tests provides equational program reasoning with Boolean tests
and regular-event operations. This motivates a finite abstraction, but importing
its laws does not establish a complete model of Tellur's data, ownership or
nontermination. See [Cornell's KAT project](https://www.cs.cornell.edu/Projects/KAT/)
and [Kozen's treatment of nonlocal control flow](https://ecommons.cornell.edu/entities/publication/26393b5f-a8d7-4a40-879b-272b8780de9c).

## 6. Abstraction, lift and extract

A public exit map `pi` sends internal completion histories to public alternatives.
Several histories may map to `failed`. The map is generally many-to-one; the public
result alone cannot reconstruct the internal history. The retained graph and its
provenance map make inspection and expansion possible.

Lift expands at the invocation site, freshens identities, intersects local guards
with the caller context, substitutes values, and wires internal exits to caller
continuations. It must preserve observable behavior and provide an explanatory
correspondence for definitions, invocations and internal exit contributors. Comparing
only rendered strings is insufficient. Current expanded IR retains static identity
mappings; it is not a complete recorded runtime history of loop iterations.

Extraction is a proposed inverse up to these correspondences, not byte-for-byte text:

    expand(extract(G)) ≃obs G

Its proof obligations include a closed graph cut: explicit live-ins, every outgoing
control/data edge, ownership transfers, returned-borrow origins, effects, unresolved
obligations and provenance. External selectors cannot silently depend on hidden
internal atoms. Loops crossing the cut need an explicit structured/state boundary;
we cannot extract arbitrary textual ranges and assume the equation holds.

Effect order, divergence and resource behavior must be preserved in addition to
labels and payloads. Version 0.16 provides [source tail extraction](extraction.md): typed live-ins, an
explicit exit boundary, whole-unit rechecking and preservation of existing public
contracts. General graph extraction and a source correspondence checker remain
unimplemented. Rechecking alone does not establish the equation above. Expansion has differential tests, structural assertions and independently checked
[rewrite certificates](expansion-certificates.md), not a completed formal preservation proof.

## 7. Forwarding and acknowledgement

The executable syntax follows the identity-routing interpretation:

```tellur
display(first);             // Exactly one outcome, with no payload: continue.
forward display(second);    // Invoke once, then forward every declared alternative.
exit saved;                 // Forward an already completed outcome.
```

A call statement rejects multiple outcomes and any payload, including a Copy
payload. It establishes no source-visible path name. Keep an explicit delta when a
semantic path name is useful. Internal call/decision identities still survive.

```tellur
exit delta lookup(key) {
    found => forward,       // Tag-only pattern retains the entire payload.
    missing => unavailable {}
};
```

Bare `forward` is an inline arm result, including nested/stored adapters. It preserves
the selected label and all payloads. Explicit bindings may be supplied and are
checked normally. Stored borrowed payloads remain unsupported. A forwarding call
uses the same checked decision/exit lowering, so local references cannot escape and
unrelated outstanding obligations cannot be abandoned.

Forwarding is deliberate delegation of the closed contract checked for this build.
If a callee adds an alternative absent from an explicit target, forwarding fails.
If the target already admits it, or `exits auto` grows to include it, forwarding can
remain valid. We do not claim that every caller independently acknowledged every
new alternative by name. Explicit arm lists still reject unhandled additions.
Future change tooling should show the difference in delegated behavior; no frozen
contract-version acknowledgement mechanism is implemented here.

## 8. Executable evidence

`tests/algebra.rs` exercises outcome embeddings, identity, associative wiring,
Boolean laws, effect-order counterexamples, capture timing, expansion contributors,
and the distinction between observed agreement and a changed public contract.
`tests/forward.rs` checks resources, borrowed origins, escaping references, lexical
selectors, payload retention, and native execution at O0/O2 before/after expansion.

`algebra::compare_scenarios` requires matching owning entry parameter types and a
nonempty scenario set. Across revisions, reachable named type definitions must
also match; this is conservative, including their field names and order. Output
layout differences make declared outcome-shape equality false even if the tested
cases select another alternative. The report identifies the checked entries. It compares complete interpreter observations and reports
outcome-shape and source-fault-permission equality separately. Source faults compare
their full diagnostic messages and preceding events, without activation IDs or
source locations. Invalid scripts, other runtime errors and step-budget exhaustion
are inconclusive errors—even when both sides fail. Fuel exhaustion is
not proof of divergence. A borrowed entry requires an owning observer harness.

From the project root:

```sh
./bin/tellurc compare playground/algebra/original.tellur playground/manifest.json \
  playground/algebra/identity.tellur playground/algebra/scenarios.json

# Expected counterexample; reports changed event order and exits unsuccessfully.
./bin/tellurc compare playground/algebra/original.tellur playground/manifest.json \
  playground/algebra/reordered.tellur playground/algebra/scenarios.json
```

The JSON claim is deliberately `agreement_on_supplied_scenarios_only`. Contract
shape growth is reported even when no supplied case produces the new alternative.
The CLI uses a 100,000-step budget per execution; a graph transformation may change
step counts without changing observations. A budget failure is inconclusive.

Version 0.15 also provides [structural protocol analysis](protocol-analysis.md):
finite models, residual queries, silent-cycle detection and completed finite trace
inclusion. These forget data and do not decide the observational equivalence above.
The [conditional simulation argument](expansion-simulation.md) relates expansion to
the current runtime and identifies the remaining formal proof obligations.

## 9. Next mathematical checkpoints

1. Formalize the transfer judgments and the [conditional expansion simulation](expansion-simulation.md),
   including resource accounting and the finite administrative rank. The written
   argument and runtime case analysis are delivered; a mechanized proof is not.
2. Extend the [certificate format](expansion-certificates.md) beyond call expansion.
   Version 0.14 checks that narrow rewrite independently; the semantic soundness
   of its rules still needs a preservation argument.
3. Refine the finite structural abstraction delivered in 0.15. Residual queries and
   completed finite language inclusion are executable. Retaining selected data and
   checking infinite behaviors require additional models and explicit claims.
4. Extend the rechecked tail extraction delivered in 0.16 to general graph cuts
   with a checked provenance correspondence and a preservation argument.
5. Extend observations to causal partial orders and structured task ownership before
   admitting concurrency laws. `A & B` remains a history test, not a synchronization.

For a finite-state protocol abstraction, the proposed residual is
`residual(P,h) = {s | h;s belongs to P}`. It describes allowed suffixes, not permission
to duplicate a live continuation. Sheaf-inspired editor views likewise require
precise contexts, restrictions and gluing conditions before any local-to-global
correctness claim. Neither research direction is an executable feature of 0.13.0.
