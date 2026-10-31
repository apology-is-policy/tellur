# Glossary

[Language reference](../language-reference.md) · [Feature status](support-status.md)

These definitions describe the current sequential implementation. Async concepts
in the roadmap have not yet added execution semantics to these terms.

| Term | Meaning |
| --- | --- |
| Alternative | One member of a closed set of variant tags or completion outcomes. |
| Associated type | A type member required by a trait and assigned by an implementation. `reader<message>.item` denotes a type, not a stored field. |
| Binding | A local name attached to a typed value. A declaration or delta payload pattern introduces it. |
| Borrow | Access to existing storage through a reference, checked against competing uses and the owner's lifetime. It does not clone or transfer the owner. |
| Capability | A compiler-derived permission such as `copy`, `clone` or `drop`. These sealed capabilities currently differ from user-defined traits. |
| Carrier | A record that stores references to other owners. Its own storage and the externally borrowed storage are different lifetime sources. |
| Completion | One invocation finishing through an outcome label with its payload. A failure label is still a normal completion. |
| Control | A callable unit with typed parameters, a closed exit contract and a checked body. It was called a region in the original syntax. |
| Copy | Permission to duplicate a value implicitly on ordinary use. Shared references are Copy; exclusive references are not. |
| Delta | An exhaustive decision over a control completion, boolean or closed tagged value. It produces path facts, directly exits selected histories, or produces an inline adapted outcome. |
| Drop | Ordinary disposal of a disposable value. It cannot silently complete a resource or abandon an outcome. |
| Effect | An observable operation represented by a primitive contract/trace. Having one outcome does not imply purity. |
| Entry | The distinguished selection of histories entering the current control or loop; nested entry also respects surrounding group/scope selections. |
| Exit contract | A control's declared or inferred set of completion alternatives and payload types, including checked borrow dependencies. |
| Exclusive reference | `ref mutable t`: a non-Copy permission that excludes overlapping competing access and can authorize mutation/reborrowing. |
| Field | A named part of a struct, or a name documenting a payload position in a contract. A contract field name is not a body-local variable. |
| Forwarding | Explicit delegation of compatible completions, preserving labels, payload types, transfers and borrow dependencies. |
| History | A modeled sequence of control-flow choices and operations. Structural analysis may include histories impossible for concrete inputs. |
| Host failure | Failure outside a declared Tellur contract, such as allocation failure or an unavailable live adapter. It is not an implicit outcome. |
| Invocation | A particular call occurrence, distinct from the reusable control declaration. Expansion retains this identity. |
| Grouped selection | `[path]: { ... }` explicitly bounds a selection. Its closing brace restores the enclosing body's entry; it introduces no value/lifetime scope. |
| Last use | The last use relevant to the conservative liveness analysis. A loan may end here before the reference's lexical scope ends. |
| Lexical scope | The source region that bounds names. Flat and grouped path selection do not create one; a scoped selection or inline adapter arm does. |
| Loan | The checker's tracked restriction on owner access while references retain permission/dependencies. It is not a runtime resource supplied by the programmer. |
| Move | Transfer of a non-Copy value and its obligations/dependencies to another owner. Later use of the moved value is invalid until valid reinitialization. |
| Nominal type | A type identified by its declaration and arguments, rather than solely by its field shape. |
| Origin | Storage on which a reference's validity depends. A contract constrains that relationship; it does not extend storage lifetime. |
| Origin group | A public name grouping a record's contained-reference dependencies. It is not a duration, address or runtime field. |
| Outcome | A labeled completion with an ordered payload. A stored completed outcome has a mandatory handling obligation. |
| Owner | The value/storage whose lifetime and permitted uses must be respected by references into it. |
| Path alias | A reusable name for a mask built from already established atoms/aliases. |
| Path atom | A fresh fact produced by a delta arm; it remains true on descendant histories. It is not a bool variable. |
| Payload | The ordered values carried by an alternative. Payloads may be empty, copied, moved, borrowed or resource-bearing according to their types. |
| Place | Addressable storage, such as a local binding, record field or dereferenced reference. Borrowing requires a suitable place. |
| Protocol model | A finite graph abstracting observable operation/completion sequences. It forgets values and is not a complete program semantics. |
| Reborrow | A child reference created under an existing reference's authority. Its liveness can temporarily restrict use of that parent. |
| Resource | A must-complete value whose obligation must be consumed or transferred on every relevant completion/restart. |
| Retainer | A live value/reference that preserves a dependency on an owner. Copies can introduce independent retainers. |
| Selection | The histories on which a statement executes, computed from a path mask. A union does not execute shared work twice. |
| Shared reference | `ref t`: permits non-mutating access and can coexist with other shared references. |
| Sum / tagged value | A value with one active alternative and that alternative's payload, such as a variant, option or stored outcome. |
| Trait | An explicit static contract of required controls and associated types. It does not prescribe implementor storage or provide inheritance. |

## Lifetime, liveness and permission

These words answer different questions. **Lifetime** concerns whether the required
storage still exists. **Liveness** concerns whether a reference or retained field
will be used again under the compiler's analysis. **Permission** concerns which
accesses may coexist. Keeping an owner alive does not make an overlapping mutable
access legal. Ending one retainer does not release dependencies still held by a copy.

With two separately grouped reference fields, the last use of the left field can
release its owner while the right field remains live. A later whole-record use may
retain both. With an exclusive child reborrow, the authorizing carrier may also have
to remain borrowed even if the ultimate referent lives elsewhere. See
[carrier reborrowing](../language-reference.md#reborrowing-exclusive-fields-through-a-carrier)
for the exact boundary rules. A returned reference to a whole reference-bearing
carrier preserves two layers: the carrier storage and its contained sources.
[Carrier result contracts](../language-reference.md#returning-references-to-reference-bearing-aggregates)
describe them with `from input` and bracket mappings respectively. Local storage
retains these dependencies recursively when a reference is placed in another
aggregate or captured outcome.

## Type, contract and protocol

A named outcome is a nominal type and a reusable completion contract. Its alternatives
describe what a single invocation can produce. A trait instead prescribes operations
and associated types for implementors. A temporal protocol describes allowed operation
sequences. The language's research direction connects these ideas, but the current
compiler does not treat them as interchangeable notions of “protocol.”

An **inherited option origin schema** preserves its payload type's ordered groups.
A direct shared or exclusive reference payload contributes the single group
`'source`; an absent alternative contains no reference. A **named completion origin
binding**, as in `exits reply[input]`, applies those groups to the chosen exit's
payload without wrapping it in another outcome value.

**Borrowed sum decision** — An exhaustive delta on a shared or exclusive reference
to a variant, option or named outcome. It borrows the active payloads and preserves
the owner's tag and handling obligations. Existing reference payloads are reborrowed
directly; ordinary payloads are borrowed in place. See
[borrowing whole sums](../language-reference.md#borrowing-whole-sums).

A **reference layer** is one stored reference and the carrier reached through it.
A public nested schema maps its storage with `<-` and its referent groups with
brackets. The compiler keeps these dependency paths distinct across calls.

**Process status** — A numeric interpretation of the final executable outcome.
Declaration defaults use `@exit N`; entry mappings can override them. It is not
part of an outcome payload or its ordinary compatibility. See [process entry](process-entry.md).

**Lexical boundary:** an isolated ordinary work block with an attached retained-owner
contingency. A source fault abandons the work before its contingency runs.

**Supervised loop:** a tail loop whose protected state survives terminal recovery
into a fresh iteration. `recover again` cannot carry handler-chosen arguments.

**Entry-only control:** a control containing authorized process termination. It can
be selected as an executable entry, but ordinary calls to it are rejected.
