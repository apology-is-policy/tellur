# Precise origins for stored references

28 October 2026. Compiler **0.43** implements grouped declarations and results,
whole-record inputs, contained-origin selectors, field-sensitive last use, and
shared/exclusive reborrowing of contained exclusive references, and checked opaque
owner splits. Local borrowed sums arrived in 0.33 and nested local storage in 0.34;
grouped source variants cross owned call boundaries in 0.35. Named-outcome/option
schemas arrived in 0.37; whole-sum borrowed inputs and payload reborrows in 0.38.
Direct whole-carrier reference results arrived in 0.39, and local nested storage of
those references in 0.40. Public contracts for these nested layers arrived in 0.41;
aggregate trait contracts follow in 0.42 and local shared-reference collections in
0.43. Exclusive/nested elements and public collection schemas remain planned.

## Executable surface

```tellur
struct text_pair['a, 'b] {
    ref string left <- 'a;
    ref string right <- 'b;
}

control pair(ref string a, ref string b)
    exits ready { text_pair[a, b] value }
{
    exit ready { text_pair { left: a, right: b } };
}
```

`'a` and `'b` are origin parameters in the struct's public interface. They are not
values, type parameters, runtime fields or durations. `<...>` still supplies type
arguments. `[...]` supplies origins. `<-` attaches a reference field to its group;
it does not perform assignment or construct a reference.

In the result declaration, `text_pair[a, b]` binds the groups **positionally**, in
struct declaration order, to reference parameters `a` and `b`. The names of the
control parameters do not need to resemble the group names. The body must actually
return references from those inputs. An owned parameter or a local variable cannot
supply its storage to a result that outlives the call.

The returned struct's own storage belongs to the caller. Its reference fields keep
borrowing the original owners. Construction uses ordinary field values and needs no
repeated origin arguments. A local binding or decision pattern uses `text_pair`,
without brackets, and inherits the checked value's origins.

The compact alternative is executable too:

```tellur
control pair(ref string a, ref string b) exits ready... {
    exit ready { text_pair { left: a, right: b } };
}
```

The compiler infers both the payload type and its origin dependencies. `exits auto`
can infer the complete exit set. Explicit payload types without bracket bindings
also infer their origins from the body, as direct reference results already do.
An unproduced result needs explicit bindings because there is no body evidence.
Forwarding and immediate nested adapters retain checked origins.

## Nested and generic structs

```tellur
struct view<t>['source] {
    ref t value <- 'source;
}

struct document_view['heading, 'content] {
    view<string>['heading] title;
    text_pair['heading, 'content] parts;
    string caption;
}
```

A grouped struct must account for every reference leaf. Nested fields map their
own groups into the enclosing groups. Ordinary owned fields need no annotation.
The checker rejects missing, unknown, duplicate or unused groups, wrong mapping
arity and origins attached to a field with no reference dependencies. Ungrouped
local structs remain supported, but a returned reference-bearing record requires
a complete declared group interface. Generic schemas are validated even when unused;
generic control bodies are checked symbolically before concrete calls.

Group schemas have a 64-level traversal limit, 256 groups per declaration and 4096
reference leaves per expanded schema. Exceeding a limit is a diagnostic, not a
partially checked contract. Existing finite-layout and specialization limits apply.

## Precision and permissions

Separate groups retain separate dependencies even when their value types are the
same. After the final use of `pair.left`, its source can become available again
while `pair.right` remains live. A copied or extracted left reference still keeps
that source borrowed. Whole-record copying, cloning, moving or explicit dropping
counts as using its contained references. Loops retain conservative outer loans.

A group may cover several fields. At a control boundary it intentionally combines
their possible sources, including alternatives from different exit paths. Every
live field in that group retains that combined dependency. Public contracts with
single input bindings remain precise about the promised input. An explicit union
binding has not yet been added; inference can represent a union internally.

A group does not prove that addresses differ. An exclusive field requires exclusive
input permission. Exclusive fields move and cannot be copied or cloned. Separately
borrowed inputs may supply independent exclusive fields because call-site alias
checking validates them. An explicit input anchor is opaque: it protects the whole
passed subobject, hiding the implementation's projection. Two results anchored to
that same opaque input may include exclusive references: their construction must
establish disjoint access. Inferred concrete owner projections retain their existing
precision. Renaming a group never manufactures an exclusive permission.

## Modules, tooling and transformations

Group names are public interface when their type is public; their field mappings
may remain private. Private fields cannot be accessed or constructed outside their
module. Renaming a private field does not change the public group bindings. Hover
shows the schematic declaration and checked return dependencies by group, without
publishing private field paths from an imported result.

The checked graph records each returned reference leaf, its public group and its
input sources. Group bindings do not create new runtime types or code specializations.
Private address slots access received reference fields; static loans remain attached
to their owning record fields. The interpreter and native backend use the same
existing reference representation. Expansion certificates preserve both those
instructions and the result's origin metadata. Tail extraction rechecks the complete
candidate and rejects changed borrowing contracts; general aggregate extraction is
not claimed.

See [the executable example](../playground/origin_groups.tellur) and
[the current language reference](language-reference.md).

## Carrier boundaries and remaining accepted design

Carrier boundaries distinguish two sources:

- **Carrier storage:** a reference into a borrowed record's own storage depends on
  the carrier's borrow, including any permissions it derives from.
- **Contained origin:** a reference already stored in a record points outside that
  record. An owned carrier can transfer it; copying a shared field may allow it to
  outlive the carrier. Reborrowing an exclusive field through a borrowed carrier
  generally keeps the carrier's authorizing borrow alive.

The accepted selector is **`input['group]`**, implemented in 0.30. It names a public
group, never a private field. `from input` still means carrier storage; `from input['a]`
means the external origins contained in group `'a`. Result mappings can use these
selectors: `text_pair[input['a], input['b]]`. Owned carriers transfer their fields;
shared contained references copied from borrowed carriers may outlive the carrier.
Both shared and exclusive carrier borrows also work with exclusive contained fields
in 0.31. Shared carrier access permits only shared access to their referents; exclusive
carrier access can grant exclusive child borrows. Moving an exclusive field out of a
borrowed carrier remains forbidden.

For an exclusive field, a contained-origin contract implicitly retains the carrier
storage authorizing the reborrow. Checked return metadata records these storage guards
separately from pointee origins. Guards prevent conflicting carrier moves, mutation
and disposal, but are not mistaken for overlapping pointees of independent children.
A copied already-shared field needs no such guard; a shared reborrow of an exclusive
field does. Forwarding, generic calls and returned records preserve both dependencies.
Hover explains retained authorization without displaying private storage paths.

Each child releases its permission at its own last use, restoring its parent field.
Whole-carrier borrows check all contained permissions before lending them. Combined
groups may contain multiple exclusive fields: access permissions are independent,
while lifetime anchors and carrier guards retain the combined dependencies.

Whole-carrier reference results are delivered in 0.39. Rebinding, reference-bearing
loop state and borrowed-record trait contracts remain unsupported. Direct `from input` trait
contracts keep their existing semantics. See [the carrier example](../playground/record_inputs.tellur).

## Checked opaque splitting — delivered in 0.32

A control may return several exclusive references declared `from input`, or a
record whose groups all bind to `input`. Distinct fields may even share one group.
The body must establish that all references returned together coexist without
exclusive aliasing. Shared references may alias other shared references. The same
rule applies to trait implementations and symbolic generic checking. An annotation
cannot create a permission: returning one mutable field twice still fails.

The caller protects the declared owner anchors and receives independent access
permissions for the result leaves. Copying shared references, moving exclusive
references, projecting fields, and reborrowing preserve permission ancestry.
Conflicting parent access stays suspended while a child lives. Different calls
have different permission identities; two overlapping calls cannot claim to be
independent merely because their result positions differ. No runtime permission
objects or new surface syntax are introduced.

An opaque anchor retains the whole passed subobject until the last retaining
child finishes. Releasing one result does not expose an implementation's private
projection. Nevertheless, disjoint local field borrows through a returned reference
can coexist: access projections and lifetime anchors are tracked separately.
Branches may swap which private field occupies each result position; each exit must
be valid, even though the lifetime union across those exits loses that correlation.

The checker attaches local permission families to incoming record leaves and each
call's reference result leaves. Each origin retains the family/member ancestry and
projections taken through it. Overlapping anchors conflict unless a shared family
proves distinct members or disjoint projections. A raw owner has no child permission
and therefore conflicts with its live opaque children. Storage guards retain their
own authority dependencies and cannot be used as pointee separation evidence.

This guarantee follows from checked exit construction and the exclusive-reference
type invariant; it is not a user-supplied assertion or an additional serialized
contract. Existing checked IR retains types, owner anchors and executable reference
construction. Expansion certificates preserve those instructions and contracts.
The tests cover invalid duplicate returns, mixed shared/exclusive overlap, owner
retention, copies/moves, nested reborrows, shared groups, path-dependent swaps,
traits, generics, private modules and original/expanded native execution. This is
an executable checking rule, not a mechanized soundness proof.

See [opaque_splits.tellur](../playground/opaque_splits.tellur).

Local tagged loans now extend to variants, optional references and completed outcomes.
Only constructed payloads contribute active loans; delta transfers the selected
payload dependencies. Nested local storage is delivered in 0.34. Grouped variant schemas and owned variant
boundaries are delivered in 0.35. Named-outcome/option schemas arrived in 0.37,
and whole-sum borrowed inputs in 0.38.
Collection elements follow that work. Extracting/removing an element
transfers its dependencies. A borrow into a vector's storage differs from an external
reference merely stored as an element. Dynamic indices require conservative tracking.
Outcome-handling and resource-completion obligations remain separate from lifetimes.

Associated types need an explicit group-schema interface before an abstract trait can
promise those groups. Generic normalization, adapters, assignments and transformations
must preserve this metadata without inspecting one convenient concrete specialization.
Async and LLVM coroutine work remain later roadmap items.


## Nested local storage — delivered in 0.34

Reference dependencies retain every enclosing constructor, payload position and
record-field path. Unpacking removes one constructor layer and rebases the storage
holder; deeper tags stay sealed until their own delta. This preserves the original
owner anchors, carrier guards and exclusive permissions. Independent nested fields
can reach their last uses separately. Shared copies remain independent retainers;
exclusive values move, and nested outcome obligations remain mandatory.

An adapter cannot smuggle a borrow of a completed arm's storage through a nested
record or variant. Finite nesting composes with generic data, module visibility,
local loops and checked expansion. No new runtime representation or public origin
syntax is introduced. Whole borrowed-sum inputs are delivered in 0.38;
direct whole-carrier reference results follow in 0.39. Stored carrier references,
reference-bearing collections, rebinding and reference-bearing loop state retain their gates.

See [nested_borrows.tellur](../playground/nested_borrows.tellur).


## Grouped variant boundaries — delivered in 0.35

`variant view['source] { found { ref string text <- 'source }, empty }` extends the
record schema notation to alternatives. Nested records and variants map their groups
positionally. A boundary's reference leaf includes each enclosing constructor and
payload index, not just a record projection. Owned input carriers receive abstract
origin identities; their references are materialized only when the corresponding
delta unpacks the active payload. Empty alternatives never trigger inactive reads.

Result dependencies retain these guards through inference, explicit mappings,
contained selectors, generic specialization, capture and forwarding. At a call
boundary the caller retains the possible sources until unpacking selects a payload;
then it releases inactive alternatives' retainers. Group-wide source unions remain
conservative. Exclusive permissions stay separate from lifetime anchors. Shared
copies and nested must-handle outcomes retain their independent obligations.

Schemas on named outcomes and builtin options are delivered in 0.37, and whole-sum
borrowed inputs in 0.38, followed by direct whole-carrier reference results in 0.39.
Stored carrier references follow in 0.40–0.41 and aggregate trait contracts in 0.42.
Reference-bearing loop state and rebinding remain gated.
Arguments returning contained variant borrows must currently be local bindings.
Unproduced reference leaves require explicit result origin mappings. The independent
IR verifier validates tagged result/source paths and rejects corrupted metadata.

See [the reference](language-reference.md#grouped-variant-contracts) and
[public_variants.tellur](../playground/public_variants.tellur).

## Borrowed sum carriers — delivered in 0.38

A borrowed decision preserves the sum's tag, payload storage and handling obligation.
Only its active alternative lends fields. Ordinary fields become references into
storage; reference fields are directly reborrowed with authority bounded by both
the carrier and stored permission. Shared reference leaves may outlive their carrier.
Exclusive leaves retain a storage guard, including when reborrowed as shared.
Nested reference-bearing payloads retain tagged provenance under their borrowed
holder. No inactive reference address is projected.

The IR distinguishes consuming and borrowed decisions. The verifier checks borrowed
operand and payload types; expansion and certificate validation substitute the
reference expression, and interpreter/native execution preserve storage addresses.
Whole reference-bearing carrier reference results add contained-loan propagation in
0.39, described below. See [the reference](language-reference.md#borrowing-whole-sums).


## Whole carrier reference results — delivered in 0.39

`ref view[input['a]] value from input` combines an outer storage lifetime with the
contained source mapping. The mappings may be inferred from the body. The IR uses
its direct return contract for storage and its stored return contracts for leaves
inside the referent. Sum leaves retain their alternative guards; callers attach
these loans to the indirect result holder. Forwarding and expansion preserve both.

Shared outer references restrict exclusive contained permissions to shared access.
An explicit drop releases a holder even if its type is Copy, so it requires that
whole-holder borrows have ended. Independent shared copies retain their own loans.
See [the reference](language-reference.md#returning-references-to-reference-bearing-aggregates).

Nested storage of these carrier references is C1b.3b.2e: capture/aggregate layouts
need a distinct reference-indirection step in their dependency schemas, delivered
in 0.40–0.41. Aggregate trait contracts follow in 0.42. This does not add reference rebinding,
borrowed collection elements or reference-bearing loop state.


## Local stored carrier references — delivered in 0.40

A stored reference retains a recursive tree of the loans inside its referent. Conflict
checks include these dependencies while the reference is packed. Moves and copies
preserve their respective ownership rules; local liveness follows live reference
fields as well as named aliases. Unpacking reconstructs private reference slots and
guarded sum leaves, with no projection into an inactive alternative. The new tag
selection establishes its own domain; possible producer origins are conservatively
united instead of requiring equality with the producer's earlier path atoms.

Local borrowing can use concrete provenance without a public schema. Public control
boundaries continue to validate their schemas separately, including inferred results.
This prevents the local extension from silently accepting incomplete public contracts.
No runtime pointer representation or source syntax changes are required.

C1b.3b.2e.2 is next: public mappings for containers of carrier references, preserving
outer storage separately from nested contained groups. Local success is not evidence
that whole-container input/result contracts or reference type arguments are supported.
See [the reference](language-reference.md#storing-references-to-reference-bearing-aggregates).


## Public nested-reference contracts — delivered in 0.41

`ref mutable view['text] target <- 'storage` maps the referent's contained group
separately from its storage. Each public dependency path records reference-indirection
steps as well as record fields and guarded alternatives. Owned/borrowed container
inputs and results preserve all layers, including captured and forwarded results.
The checker reconstructs the authorization chain without reading inactive payloads;
shared outer authority cannot grant exclusive inner access. Public group selectors
retain storage guards where needed. The independent IR verifier checks every layer.
See [the reference](language-reference.md#public-nested-reference-contracts) and
[the example](../playground/public_carriers.tellur).

Aggregate trait origin contracts arrived in 0.42; local shared-reference collections
follow in 0.43 below. Exclusive/nested elements, public collection schemas, abstract
generic origin-shape bounds, reference rebinding and loop state remain separate work.


## Local shared-reference collections — delivered in 0.43

Collection holders retain a conservative union of element sources under a wildcard
path. Successful insertions add retainers on their selected paths; returned references
and consuming conversion/removal results keep independent dependencies. Lookup copies
the stored shared reference directly, so its lifetime depends on the external owner.
A proven `vector_pop` empty branch releases that holder's sources. Constant index/key
precision, exclusive elements, nested layouts and public schemas remain future stages.
See [the reference](language-reference.md#local-collections-of-shared-references).
