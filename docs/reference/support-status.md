# Feature status

[Language reference](../language-reference.md) · [Roadmap](../language-revision.md)

This matrix describes **compiler 0.52.0**, not the full planned language. “Supported”
means implemented within the stated boundary. A feature in one local position does
not imply it is accepted at every public, generic, collection or loop boundary.
The linked regression suites give more detailed acceptance and rejection evidence.

## Language and data

| Area | Current support | Boundary / next work | Evidence |
| --- | --- | --- | --- |
| Controls and paths | Typed parameters, exhaustive delta, selected histories, inferred contracts, aliases and scopes | No recursive calls or value-level proof of path feasibility | [Revision tests](../../tests/revision.rs) |
| Adapters and forwarding | Nested tail adapters, stored adapters, explicit compatible forwarding, one-nullary-completion statements | No arm statement blocks or general expression-position adapters | [Adapter tests](../../tests/adapters.rs), [forwarding tests](../../tests/forward.rs) |
| Completed outcomes | Capture now, handle later; named and producer-indexed contracts; mandatory handling | Not futures; no implicit structural assignment across unrelated named types | [Outcome tests](../../tests/outcomes.rs), [named contracts](../../tests/named_outcomes.rs) |
| Payload inference | Private `exits auto` and selected nonempty `label...`; local/pattern `auto` | Explicit public and trait contracts; no general signature wildcard | [Inference tests](../../tests/payload_inference.rs) |
| Data declarations | Nominal structs, closed variants, wrappers, named/positional struct construction | No recursive layouts, inheritance, defaults or update spreads | [Data tests](../../tests/data_types.rs), [field tests](../../tests/named_fields.rs) |
| Public control contracts | Explicit labels, payloads and borrowed-result mappings, including unused generics; bounded forwarding/stored-outcome/adapter contributor notes | Loop-carried values, indirect aliases, dynamic collections and cross-call payload dependencies are not traced | [Public contract tests](../../tests/public_contracts.rs), [example](../../playground/public_contracts.tellur) |
| Diagnostic reports | Versioned `check --json`, expression/token ranges, bounded independent-body errors, related points and structured CLI/LSP path witnesses | One error per independent body; preparation/within-body recovery remains future work; declaration/contract/contributor locations may still be points | [Report tests](../../tests/diagnostics.rs), [format](diagnostics-and-limits.md#machine-readable-checking) |
| Modules | Explicit relative imports and namespaces; private by default; public declarations/fields | No package-name resolver or package manager | [Module tests](../../tests/modules.rs) |
| Generic data | Explicit first-order type parameters and specialization | No argument inference, data bounds or higher-kinded parameters | [Generic data tests](../../tests/generic_data.rs) |
| Generic controls | Universal definition checking, sealed/user-trait bounds, explicit specialization | No recursive calls, type-argument inference or reference types as generic arguments | [Control tests](../../tests/generic_controls.rs), [bounds](../../tests/trait_bounds.rs) |
| Traits | Explicit implementations, associated types/capabilities, static qualified dispatch, direct/grouped borrowed results and abstract origin witnesses | No abstract associated-type origin-shape bounds, blanket implementations, default methods, associated equality/trait bounds or runtime polymorphism | [Trait tests](../../tests/traits.rs), [borrowed contracts](../../tests/lifetime_contracts.rs) |
| Repetition | Checked tail loops and explicit state transfer | No nested source loops or reference-bearing loop state | [Loop tests](../../tests/loops.rs) |

## Ownership and reference boundaries

| Area | Current support | Boundary / next work | Evidence |
| --- | --- | --- | --- |
| Owning values | Copy/move/explicit clone/disposal, assignment, partial record moves | No user-defined copy/clone/drop behavior | [Value tests](../../tests/values.rs), [capabilities](../../tests/capabilities.rs) |
| Direct references | Shared/exclusive calls and returns, reborrowing, last use, field separation | No reference rebinding or references to references | [Reference tests](../../tests/references.rs) |
| Local reference fields | Nested structs, extraction, independent retainers and field last use | Assignment/rebinding of reference-bearing storage remains gated | [Stored record tests](../../tests/stored_records.rs) |
| Public record origins | Ordered groups, returned mappings, whole-record inputs, `input['group]` | Grouped variants, named outcomes and options compose with records; aggregate trait contracts require explicit mappings | [Origin records](../../tests/origin_records.rs), [record inputs](../../tests/record_inputs.rs) |
| Carrier reborrows | Shared/exclusive child access, preserved storage guards and last-use restoration | A shared carrier cannot grant exclusive access | [Carrier tests](../../tests/carrier_reborrows.rs) |
| Opaque splits | Multiple checked independent result permissions from one input anchor | Public anchor remains opaque; independent calls do not imply disjointness | [Split tests](../../tests/opaque_splits.rs) |
| Local borrowed sums | References in variants/options/outcomes, capture/adapters, arbitrary finite record/tag nesting | Ungrouped sums can be owned or borrowed locally; public boundaries require complete schemas | [Sum tests](../../tests/borrowed_sums.rs), [nested example](../../playground/nested_borrows.tellur) |
| Public variant origins | Owned grouped variants cross calls and returns; constructor-guarded origins, contained selectors, nested/generic layouts, capture and forwarding | Direct constructor arguments returning contained borrows remain gated | [Public sum tests](../../tests/public_sums.rs), [example](../../playground/public_variants.tellur) |
| Named-outcome and option origins | Named schemas and completion bindings, inherited option groups, owned calls/results, contained selectors, generic/module composition | Anonymous outcome schemas remain gated; grouped trait contracts require explicit mappings | [Origin tests](../../tests/outcome_origins.rs), [example](../../playground/outcome_origins.tellur) |
| Collections | Arrays/vectors/maps, explicit bounds outcomes, resources, borrowed element access and local shared-reference elements | Exclusive/nested elements, public collection-origin contracts, slices and iterators remain queued | [Collection tests](../../tests/collections.rs) |
| Borrowed sum carriers | Whole shared/exclusive inputs, active payload reborrows, nested carriers, retained handling obligations | Complete schemas required at public boundaries; local borrows use concrete provenance | [Sum carrier tests](../../tests/sum_carriers.rs) |
| Carrier reference results | Direct shared/exclusive results, inferred/declared contained origins, storage retention and forwarding | Local capture and aggregate storage work; aggregate trait results require explicit mappings | [Carrier result tests](../../tests/carrier_results.rs), [example](../../playground/carrier_results.tellur) |
| Stored carrier references | Local recursive dependency trees, capture/adapters, shared retainers, nested/borrowed unpacking and last use | Public schemas map each storage and contained-origin layer; aggregate trait contracts require explicit mappings | [Stored carrier tests](../../tests/stored_carriers.rs), [example](../../playground/stored_carriers.tellur) |
| Public nested references | Separate storage arrows and referent brackets; owned/borrowed boundaries, nested tags, capture and forwarding | Bind constructor arguments before selecting nested origins; abstract type-parameter origin shapes remain unsupported | [Public carrier tests](../../tests/public_carriers.rs), [example](../../playground/public_carriers.tellur) |
| Resources | Must-complete obligations through aggregates, replacement and explicit draining | No implicit fallible cleanup or abandoned outcome disposal | [Completion tests](../../tests/completion.rs), [ownership tests](../../tests/ownership.rs) |

“Borrowed element access” means obtaining a reference into an owning collection.
“Stored reference-bearing elements” means storing references to external owners as
collection elements. Owning-element access and local storage of direct shared
references are supported. Exclusive and nested element types and public collection
contracts remain queued. Grouped source variants now work as whole owned parameters and result fields.
Grouped variants, named outcomes and builtin options also support whole borrowed
inputs and non-consuming decisions. Direct controls may return selected references
or references to whole reference-bearing aggregates. The latter can be stored
inside local aggregates and captured outcomes, or cross public container boundaries
with complete schemas for every nested reference layer.

## Tools, analyses and planned execution features

| Area | Current support | Limit |
| --- | --- | --- |
| Rust source adapters | Typed wrappers, closed result enums, owned scalar/collection data, direct opaque retained resources, call-duration shared borrows, explicit dependencies and locked builds | No stable binary ABI, foreign returned borrows, exclusive mutation, aggregate resource boundary or async adapters |
| Maintained application | Persistent settings CLI and bounded sequential loopback HTTP; Tellur routing/validation/cleanup and Rust mechanisms | Limited HTTP profile; no concurrency, task cancellation or crash-durable transaction guarantee |
| CLI and native build | Check, execute, build standalone executable through Rust/Cargo and LLVM | No direct LLVM coroutine lowering |
| VS Code | Highlighting, unsaved diagnostics with related forwarding locations, Outline, declaration contracts, inferred `auto` hover, build/run | First diagnostic per document; hover requires a successful check and is not general expression hover |
| Expansion | Checked rewrite certificates, retained invocation/decision provenance | Not arbitrary equivalence or termination checking |
| Tail extraction | Typed helper generation and whole-candidate rechecking | Not general graph extraction or a full equivalence certificate |
| Scenario comparison | Concrete supplied-case comparisons and counterexamples | Agreement on those cases only |
| Protocol analysis | Finite graph/residual queries, completed finite trace-language comparison | Forgets data and concrete feasibility; budgets can make results inconclusive |
| Async and tasks | Design direction only | No executable spawn/await/cancellation/task ownership yet |
| Runtime polymorphism | Roadmap only | Traits currently dispatch statically |

The [conformance inventory](../conformance.md) records release validation. The
[roadmap queue](../language-revision.md#active-checkpoint-queue) remains the authority
for execution order, not a promise that every queued syntax is settled.

Trait origin coverage: [aggregate trait tests](../../tests/aggregate_traits.rs) and
[the runnable example](../../playground/aggregate_traits.tellur).

Local shared-reference collections: [tests](../../tests/borrowed_collections.rs),
[example](../../playground/borrowed_collections.tellur).

## Executable status contracts (0.48)

Named outcome alternatives accept `@exit N` with N in 0..255. Native generation
checks complete final-entry mappings, including explicit manifest overrides.
`exec` propagates numeric status; JSON/scenario output retains named outcomes.
Library contracts remain annotation-optional. See [process entry](process-entry.md).
