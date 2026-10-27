# Checked expansion certificates

Compiler 0.14.0, 25 October 2026.

Expansion now produces a candidate graph and an explicit correspondence record.
A separate validator checks that candidate against the original checked program.
Every `expanded(...)` operation uses this validation, including `expand`,
`run --expanded`, and native `build --expanded` / `exec --expanded`.

This establishes **conformance to the implemented expansion rules**. It does not
establish a machine-checked proof that those rules preserve all language semantics.
The distinction matters: a correct certificate checker can reject a broken rewrite,
but it cannot rescue an incorrect semantic rule shared by the design.

## Try it

From the compiler directory:

```sh
./bin/tellurc certify-expansion playground/forwarding.tellur playground/manifest.json main > expansion.json
./bin/tellurc verify-expansion playground/forwarding.tellur playground/manifest.json expansion.json
./bin/tellurc exec playground/forwarding.tellur --expanded
```

The first command emits JSON containing the expanded graph and correspondence.
The second compiles the source and validates that artifact without calling the
expander. It reports how many call substitutions were checked. Change an emitted
string, delete an instruction, erase a provenance chain or change a payload in the
artifact: verification fails with `E_EXPANSION_CERTIFICATE`.

The source and manifest are required. An artifact is not independently trusted
code, a signature, or an authorization token. It is checked against the supplied
program; unrelated controls remain exactly those of that program. The format is
versioned but experimental, and internal identity changes can invalidate an old
artifact even when the revised source behaves the same.

## The rewrite being checked

A call has a guard, ordered arguments, and one continuation for each public
outcome. Its derivation records:

- The instruction being replaced and its callee.
- Total maps from the callee's slots, decision atoms, loops and instructions to
  fresh identities. Nested calls also have records, including their transient IDs.
- One parameter-binding instruction per argument, in declaration order.

All fresh identities must be distinct, disjoint from the original allocation
interval, and accounted for. No record may be missing, duplicated, or unused.
Names and types accompany their mapped slots and atoms.

For a callee instruction with guard `g`, caller guard `c`, and identity map `rho`,
the required guard is:

    c & rho(g)

The producer distributes the context through the guard expression. The validator
factors it once and compares the resulting Boolean functions using canonical BDDs.
It therefore accepts different equivalent guard expressions, such as `g | g`,
without permitting selection to become weaker. This comparison concerns structural
facts; it does not infer concrete data feasibility.

Ordinary instructions retain their operation and argument order under substitution.
A callee exit becomes a completion of its matching caller continuation:

    exit label { values }
        ↦ set caller's label atom;
          transfer substituted values into caller payload slots;
          release callee storage;
          leave the active callee loop, if any

The actual IR keeps that completion as one structured action. The checker validates
its payload destinations and order, storage-release set, and unwind target. Existing
completion nodes are renamed when expanding a previously expanded helper.

Loop headers, simultaneous restart arguments and iteration-local reset tables must
remain intact under renaming. Storage and atoms introduced by calls inside a loop
are included in that loop's reset tables. Calls are replaced in place: their effects
cannot migrate to a later handling site, change order, disappear or be duplicated.
The caller's public parameter/exit contract and borrowed-return summary stay fixed.

Provenance is part of this derivation. Copied definitions retain their original
identity, region, source location and site name; invocation chains distinguish
multiple uses of one definition. Several internal exits may still contribute to
one public outcome. No runtime trace history is inferred from this static mapping.

## Implementation and trust boundary

`src/expand.rs` emits the artifact. `src/certificate.rs` validates it using separate
substitution and traversal code; it does not call the producer or share its rewrite
helpers. The checker reconstructs the graph allowed by the supplied correspondence,
compares it with the candidate, and runs structural/type verification before
returning a `CheckedProgram`. Other controls and the manifest come from the source
program, not from the artifact.

The Rust API is:

```rust
let artifact = program.expansion_artifact("main")?;
let json = serde_json::to_string(&artifact)?;
let decoded = serde_json::from_str(&json)?;
let expanded = program.accept_expansion(&decoded)?;
```

Shared dependencies remain trusted: the front end's ownership/disposition checks,
IR definitions, BDD implementation, runtime semantics and primitive contracts.
Independent code reduces correlated implementation failures; it is not independent
mathematical evidence that the rewrite rules themselves are sound. This validator
also does not certify arbitrary optimization, source extraction, native lowering,
termination, or asynchronous behavior. Diverging loops are permitted and are not
executed during certificate validation.

`tests/certificates.rs` exercises serialization, nested loops, repeated invocations,
staged expansion, borrowed summaries, Boolean-equivalent guards, and deliberate
corruptions of identities, effects, completions, loop state and provenance. The
existing original/expanded interpreter and native tests now run through the
validator as well. Those execution comparisons remain finite evidence.

## Next proof obligation

A [conditional simulation argument](expansion-simulation.md) now describes these rules
against the runtime. The next step is to formalize and mechanically check it. Relate source
call frames to fresh expanded storage, caller continuation facts and callee-local
loop state. Parameter binding and local instructions should preserve that relation;
a completion should re-establish it at the selected caller continuation. The
argument must account for moves, loans, outstanding obligations and divergent
executions, not only final outcomes. Extraction will need a separate checked graph
cut and boundary construction before it can reuse this correspondence machinery.
