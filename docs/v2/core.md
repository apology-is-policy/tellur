# Villanelle v2 core calculus

Status: **draft 1**, 19 November 2026. Phase 1, deliverable 1 of the
[redesign plan](../redesign-plan.md). Not yet accepted by the author; it becomes
normative when it is. Nothing here is implemented.

An independent review of the first version of this draft found seven programs
that its checking rules accepted and its semantics left stuck, most of them in
the loan rules. The rules below include the repairs. The repaired text has not
been reviewed again.

[Index of the v2 design](README.md) · [Plan](../redesign-plan.md) ·
[Rails reference programs](../../experiments/syntax_lab/rails/)

## 1. Purpose and scope

The core is the small language that every v2 program elaborates into. The meaning
of a program is the meaning of its core form. The surface syntax (Rails) is
defined separately, by elaboration into the core.

This document gives three things.

- **The instrumented semantics** (section 6). It executes a core program while
  tracking facts, ownership, borrows and obligations, and it is *stuck* on every
  violation the checker is meant to exclude. It is the definition of validity.
  The phase 2 oracle implements it directly.
- **The checker** (section 7): the judgments a program must satisfy.
- **The claims** (sections 8 and 9): a checked program is never stuck, and the
  erased semantics, which is what release code does, agrees with the instrumented
  one.

Each normative clause has an identifier in brackets, such as **[K3]** or
**[E-Exit]**. The conformance suite of phase 2 takes one accepting and one
rejecting test from every clause.

Core programs are written in Rails glyphs without sugar. One sign is added: `▷`
separates a selector from the command it guards. Surface programs never show it;
they draw the same thing with rails. All program text below is in `text` fences
and is not compiled.

Draft 1 specifies the synchronous language completely: scalars, decisions, loops
and calls (slice 1), data, ownership, resources and faults (slice 2), borrows and
views (slice 3). Tasks (slice 4) are stated as requirements in section 10 and get
their rules in draft 2. The theory behind `⊨`, the certificate format, the Rails
grammar and the elaboration are the subjects of the companion documents.

## 2. The core in one example

Binary search, as written in Rails:

```text
control bound<order: ordering.comparator>(
    source: &[order.item], rule: &order, needle: &order.item, upper: bool
) ■ position {index: u32} ⇐ index ≤ len(source)
│ ↻ search(start: u32 := 0, count: u32 := len(source))
│ ║ ⊢ start + count ≤ len(source)
│ ║ ? count = 0 → ■ position {start}
│ ║
│ ║ half: u32 := count ÷ 2
│ ║ middle: u32 := start + half
│ ║ Δ rule.compare(source[middle], needle): before │ equal │ after
│ ║
│ ╠═ equal
│ ║ ┆ Δ upper: true → high │ false → low
│ ║
│ ╠═ after ∨ low
│ ║ ┆ ↑ search(start, half)
│ ╠═ before ∨ high
│ ║ ┆ ↑ search(middle + 1, count - half - 1)
╵
```

Its core form names every decision site and gives every statement a selector:

```text
control bound⟨order : comparator⟩(source : &[order.item], rule : &order,
                                  needle : &order.item, upper : bool)
    ■ { position {index : u32} ⇐ index ≤ len(source) }

1  ⊤ ▷ s : ↻ search(start : u32 := 0, count : u32 := len(source))
             ⊢ start + count ≤ len(source)
     1  ⊤                 ▷ z : ? count = 0
     2  z.true            ▷ ■ position {start}
     3  ⊤                 ▷ half : u32 := count ÷ 2
     4  ⊤                 ▷ middle : u32 := start + half
     5  ⊤                 ▷ c : Δ order.compare(rule, &source[middle], needle)
                                  ▸ before {} │ equal {} │ after {}
     6  c.equal           ▷ u : ? upper
     7  c.after ∨ u.false ▷ ↑ search(start, half)
     8  c.before ∨ u.true ▷ ↑ search(middle + 1, count - half - 1)
```

Reading it:

- A body is a list of guarded statements. It runs once, top to bottom. A
  statement runs when its selector holds for what has happened so far.
- A decision (`z`, `c`, `u`) runs its operation once and records exactly one
  alternative. `c.equal` is the fact "site `c` completed with `equal`".
- A terminator (`■`, `↑`, `□`, `fault`, `⊥`) ends the body for the history that
  reaches it. Statement 3 has selector `⊤` and still does not run after
  statement 2 has exited.
- A loop is a decision whose operation is a local body that may restart itself.
  This loop never leaves, so site `s` has no alternatives and nothing follows it.

The checker establishes, among other things:

| Obligation | Where | Follows from |
| --- | --- | --- |
| Every history ends | End of the loop body | The selectors of 2, 7 and 8 cover `z`, `c` and `u` |
| `index ≤ len(source)` | 2 | Invariant, with `count = 0` |
| `2 ≠ 0` | 3 | Constant |
| `start + half` fits `u32` | 4 | `half ≤ count`, the invariant, and the range of `len` |
| `middle < len(source)` | 5 | `count ≠ 0`, so `half < count`; then the invariant |
| Invariant for the next iteration | 7 | `start + half ≤ start + count` |
| `middle + 1` and `count - half - 1` in range; invariant | 8 | `half < count`; the sum is `start + count` |

No arm is a filler, and no check remains at run time. Deleting the `⊢` line makes
the obligations at 2, 4, 5 and 8 unprovable, and the program is rejected with a
counterexample. The example keeps `u32` lengths, as the reference programs do;
question Q3 in section 11 may change that.

## 3. Syntax

Metavariables: `x`, `y` variables; `d` decision sites; `a`, `b` alternatives; `f`
controls; `fn` functions; `ℓ` loops; `S`, `V`, `W`, `R`, `O` declared type names;
`α` type parameters; `ε` effect labels; `φ` logical formulas (section 7.2); `n`
integer literals. An overbar is a sequence.

```text
Program       P ::= D̄
Declaration   D ::= struct S⟨ᾱ⟩ {x̄ : τ̄}
                  | variant V⟨ᾱ⟩ {c̄ {x̄ : τ̄}}
                  | view struct W⟨ᾱ⟩ {x̄ : τ̄}
                  | outcome O⟨ᾱ⟩ K
                  | extern resource R [on fault release]
                  | function fn⟨ᾱ⟩(x̄ : τ̄) [⊢ φ] : τ := e
                  | [pub] control f⟨ᾱ : C̄⟩(x̄ : τ̄) Sig := B
                  | extern control f(x̄ : τ̄) Sig
Signature   Sig ::= [⊢ φ] ■ K [faults] [uses ε̄]
Contract      K ::= { a₁ {q̄₁} [⇐ φ₁] │ … │ aₙ {q̄ₙ} [⇐ φₙ] }
Payload field q ::= x : τ  |  x : τ ∈ ȳ          the second for a borrow or view type

Type          τ ::= bool | uN | iN | string
                  | S⟨τ̄⟩ | V⟨τ̄⟩ | [τ] | τ ⇀ τ | R | outcome⟨K⟩
                  | &τ | &~τ | W⟨τ̄⟩
                  | α | α.t

Body          B ::= s₁ … sₙ
Statement     s ::= g ▷ c
Selector      g ::= ⊤ | d.a | g ∧ g | g ∨ g | ¬g
Command       c ::= [~]x [: τ] := e                     bind
                  | S {x̄} := e                           take a struct apart
                  | [~]x := capture ω(ā)                 capture a completed outcome
                  | p ← e                                assign
                  | ⊢ φ                                  obligation
                  | d : ? e                              test
                  | d : Δ ω(ā) ▸ a₁ {x̄₁} │ … │ aₙ {x̄ₙ}    decision
                  | d : ↻ ℓ(x̄ : τ̄ := ē) [⊢ φ] {B} ▸ a₁ {x̄₁} │ … │ aₙ {x̄ₙ}    loop
                  | ↑ ℓ(ē)                               again        (terminator)
                  | □ a {ē}                              leave        (terminator)
                  | ■ a {ē}                              exit         (terminator)
                  | fault e                              fault        (terminator)
                  | ⊥                                    impossible   (terminator)
Operation     ω ::= f⟨τ̄⟩ | α.m | op | outcome | V
Arguments     ā ::= a sequence, each one of  e | &p | &~p
Expression    e ::= n | true | false | "…" | x | e.x | S {ē} | V.c {ē}
                  | fn⟨τ̄⟩(ē) | e ⊕ e | ⊖ e | len(p) | p[e] | clone(p) | W {ā}
Place         p ::= x | p.x | p[e]
```

`op` ranges over the built-in operations on vectors, maps and strings that have
several outcomes or an effect. `outcome` decides a captured outcome value and `V`
decides which constructor a variant has, given the variant or a borrow of it.
`⊕` ranges over arithmetic, comparison and Boolean operators. `p[e]` indexes a
vector; a map is reached only through operations.

Faults also arise from `fault boundary` and its contingency, which are carried
over from v1 and described in section 6.9.

Well-formedness:

- **[W1]** Site names are unique within a control, loops included. Every
  decision, test and loop has one.
- **[W2]** The selector of a statement mentions only sites that precede the
  statement in its own body, or that precede the enclosing loop statement in an
  enclosing body.
- **[W3]** `↑ ℓ` occurs only inside the body of `ℓ`, at any depth. `□` occurs
  only inside a loop body and leaves the innermost loop.
- **[W4]** A name belongs to the body that binds it: the control body or a loop
  body. Nested blocks of the surface do not open scopes. A loop body sees the
  names of the bodies around it. A name is never bound in two nested bodies, and
  all binders of one name in one body give it the same type and mutability.
- **[W5]** Expressions have no effect, cannot fault and always terminate.
  Functions are not recursive. Everything with an effect or several outcomes is
  an operation, and occurs only in a decision or a capture.
- **[W6]** Borrow types `&τ`, `&~τ` and view types occur only as the types of
  parameters, locals, loop parameters and payload fields, and as fields of view
  structs. They never occur inside `S`, `V`, `[τ]`, `⇀` or `outcome⟨K⟩`, never
  as a type argument, and never as what a borrow refers to.
- **[W7]** A contract lists each alternative once. A formula `⇐ φ` mentions only
  the parameters and the fields of its own alternative. For an `&~` parameter
  `y`, `y` means the value at completion and `old(y)` the value at entry.
- **[W8]** A name of borrow type stands for the value it refers to wherever a
  value of that type is expected, and for the borrow itself wherever a borrow
  is expected. Since no borrow refers to a borrow, the two never compete.
  `r ← e` writes through `r` when `e` has the type `r` refers to, and makes `r`
  borrow something else when `e` is a borrow.
- **[W9]** A loop statement lists the alternatives that the `□` statements of
  its body use, with a name for each payload field. A payload field of borrow
  or view type always has `∈`.

A decision with one alternative and no payload is how the core writes a plain
call such as `emit(e)`.

## 4. Types, kinds and contracts

### 4.1 Usage kinds

Every type has one of three usage kinds, ordered `copy < drop < must`.

| Kind | Meaning | Types |
| --- | --- | --- |
| `copy` | May be duplicated; reading it leaves the source available | `bool`, integers, `&τ`, views of shared borrows, structs and variants declared `copy` whose components are all `copy` |
| `drop` | Has one owner; reading it by value moves it; may be destroyed silently | `string`, `[τ]`, `τ ⇀ τ`, `&~τ`, views that hold an exclusive borrow, other structs and variants |
| `must` | Has one owner; may not be destroyed; must be consumed by an operation | Resources `R`, `outcome⟨K⟩`, task handles and task sets |

- **[T1]** The kind of a struct, variant, vector or map is the largest kind among
  its components, and at least `drop` unless it is declared `copy`.
- **[T1a]** A type parameter has kind `must` and is not fault-releasable, unless
  its bounds say otherwise. Generic code may do with a value only what the
  bounds of its type allow.
- **[T2]** Destroying a `drop` value runs no program code and is not observable.
  An implementation may destroy it at any point after its last use and no later
  than the end of its scope.
- **[T3]** A type is **fault-releasable** when it has kind `copy` or `drop`, or
  is a resource declared `on fault release`, or is an aggregate or `outcome⟨K⟩`
  all of whose components and payload types are fault-releasable.

### 4.2 Contracts

A contract is a closed set of alternatives. Each alternative has a payload and
may state a condition `⇐ φ` that holds whenever it occurs. A signature may also
state a precondition `⊢ φ`, the permission `faults`, and the effects it `uses`.

- **[T4]** A borrowed payload field `x : &τ ∈ ȳ` names the borrow parameters it
  may be derived from. A payload borrow has no other origin.
- **[T5]** An `extern` signature is trusted. Its conditions, payload types,
  effects and resource behavior are assumptions about the host, listed in the
  trust report. A defined control's signature is checked against its body.
- **[T6]** `outcome⟨K⟩` is allowed only when no payload of `K` contains a borrow
  or a view.

Generic definitions are checked once, against their bounds. The dynamic
semantics is given for closed instances: every type parameter is replaced by a
type and every capability call `α.m` by the control that implements it.

## 5. Histories

An **atom** is a pair `d.a` of a decision site and one of its alternatives. A
test has the alternatives `true` and `false`. A loop has one alternative for each
name used by a `□` in its body.

A **history** `Φ` is the set of atoms established so far in one run of one body.
Satisfaction is ordinary: `Φ ⊨ ⊤`; `Φ ⊨ d.a` when `d.a ∈ Φ`; and the Boolean
connectives as usual. An atom that has not been established is false.

- **[H1]** Each run of a body starts with the empty history. This includes each
  iteration of a loop.
- **[H2]** A site is reached at most once in a run of its body. When its
  operation completes with alternative `a`, the atom `d.a` joins the history. A
  history therefore holds at most one atom per site.
- **[H3]** An atom, once established, stays. A selector that held keeps holding
  for the rest of the run, unless it contains `¬`.
- **[H4]** A loop body may consult the history of the bodies around it, which
  does not change while the loop runs.

Atoms come in two sorts. An **event atom** records the completion of an
operation with an effect, and has no further meaning. A **logical atom** has a
meaning: a formula over the values the operation was given and returned. A test
`d : ? e` gives `d.true` the meaning `e` and `d.false` the meaning `¬e`. A
decision on an operation with conditions gives `d.a` the meaning `φₐ`. The
meaning is what lets the checker discard histories that cannot happen
(section 7.2).

## 6. Instrumented semantics

### 6.1 Values, places and slots

```text
Value   v ::= n | true | false | "…"
            | S {v̄} | V.c {v̄} | [v̄] | {k̄ ↦ v̄}
            | res(r)                      a resource, r its identity in the host
            | out(a, v̄)                   a captured outcome
            | ref(m, π, t)                a borrow: mode m ∈ {shr, exc}, place π, tag t
            | W {v̄}                       a view
Place   π ::= (ς, x) | π.x | π[n] | π[k]  a variable of scope instance ς, then a path
Slot      ::= v | ◌                       available with value v, or moved
```

Data has value semantics. There is no heap of shared objects: a value is a tree,
each variable owns its tree, and the only way to refer to part of another
variable's tree is a borrow. Two places **overlap** when one is a prefix of the
other.

### 6.2 Configurations

A **scope** `Σ = ⟨ς, B, pc, ρ, Φ⟩` is one run of one body: a fresh identity, the
body, the index of the next statement, the slots of the names bound so far, and
the history.

An **activation** is one call of a control: a nonempty stack of scopes, the
control body first and one scope for each loop currently entered, together with
the control's declared effects and fault permission. Variables are looked up
from the innermost scope outward. `Φ*` is the union of the histories of the
activation's scopes.

A **configuration** is `⟨Ā, W, T, mode⟩`: a stack of activations, the host world
`W` (section 6.6), the tag table `T` (section 6.3), and a mode, which is `run`,
`fault(message)` or a final result.

### 6.3 Borrow tags

The tag table records which borrows are still usable. Every slot gets an *owner
tag* when it is bound. Every borrow gets a fresh tag whose parent is the tag it
was made through: the owner tag for `&x`, or the tag of `r` for a reborrow
through `r`. A tag has a mode, a place, a parent, and is alive or dead.

- **[B1] Access.** An access through tag `t` to place `π`, reading or writing,
  requires that `t` is alive, that `π` lies within the place of `t`, and, for
  writing, that `t` is an owner tag or has mode `exc`. Otherwise the
  configuration is stuck.
- **[B2] Invalidation.** The access then kills every other live tag `u` whose
  place overlaps `π`, unless `u` is an ancestor of `t`, in these cases: for a
  write, always; for a read, when `u` has mode `exc`. Killing a tag kills its
  descendants.
- **[B3] Creation.** Making a shared borrow of `π` through `t` is a read access
  followed by a fresh tag with mode `shr`. Making an exclusive borrow is a write
  access followed by a fresh tag with mode `exc`.
- **[B4] Moves and destruction.** Moving a value out of a slot, assigning to it
  and destroying it are write accesses through the owner tag. When a scope ends,
  the owner tags of its slots are killed.
- **[B5]** Operations that change the shape of a collection (insert, remove,
  push, pop) are write accesses to the whole collection. Reading or writing one
  element is an access to that element's place only.

Copying a shared borrow copies its tag. Moving an exclusive borrow moves its
tag. Neither is an access.

### 6.4 Expressions

`e ⇓ v` evaluates an expression in the current activation. It can change slots
(a move) and tags (an access), and nothing else.

- **[E-Var]** `x` looks up the slot of `x`. If `x` has no slot, or the slot is
  `◌`, evaluation is stuck. If the type of `x` has kind `copy`, the result is
  the value, after a read access. Otherwise the result is the value, after a
  write access, and the slot becomes `◌`. Where [W8] makes a borrow name stand
  for what it refers to, that value is read through the borrow's tag.
- **[E-Field]**, **[E-Index]** `e.x` and `p[e]` read a component of kind `copy`
  after a read access to its place, through the owner tag or, when the path
  passes through a borrow, through that borrow's tag. An indexed place, wherever
  it is evaluated (read, borrowed or assigned), is stuck when the index is not
  less than the length.
- **[E-Arith]** `e₁ ⊕ e₂` on integers of type `τ` is stuck when the mathematical
  result is not a value of `τ`, and for division when the divisor is zero. The
  wrapping and saturating operators are total and never stuck.
- **[E-Fn]** `fn⟨τ̄⟩(ē)` evaluates the arguments left to right and then the body
  of `fn` with its parameters bound. It is stuck when the precondition of `fn`
  is false for them.
- **[E-Borrow]** An argument `&p` or `&~p` resolves `p` to a place and creates a
  borrow of it by [B3].
- **[E-Build]** `S {ē}`, `V.c {ē}`, `W {ā}` and `clone(p)` build values. `clone` reads its
  place and requires a type whose components are all clonable; resources,
  captured outcomes and exclusive borrows are not.

There are no partial moves: a component of kind `drop` or `must` leaves its
owner only when the owner moves as a whole or is taken apart, a struct by
[E-Unpack] and a variant by a decision on it.

### 6.5 Statements

Let the innermost scope be at statement `g ▷ c`. If the mode is `run`:

- **[E-Skip]** If `Φ* ⊭ g`, the statement is skipped: `pc` advances.
- **[E-End]** If `pc` is past the last statement, the configuration is stuck.
  A body never falls off its end.

Otherwise `c` runs:

- **[E-Bind]** `[~]x : τ := e`. Evaluate `e` to `v`. If `x` already has a slot in
  this scope, stuck. Otherwise `x` gets the slot `v` and an owner tag.
- **[E-Unpack]** `S {x̄} := e`. Evaluate `e` to `S {v̄}` and bind each name to its
  component, as in [E-Bind].
- **[E-Assign]** `p ← e`. Evaluate `e` to `v`, then write `v` at the place of
  `p`: a write access by [B1]. If the value being replaced contains an
  obligation (section 6.8), stuck. A moved slot may be assigned as a whole and
  becomes available.
- **[E-Oblige]** `⊢ φ`. If `φ` is false in the current state, stuck.
- **[E-Formula]** A formula is evaluated on current values, over the
  mathematical integers, and makes no access: it reads through borrows and
  changes no tag. In a contract condition a parameter stands for the value it
  had at entry, which the instrumented semantics keeps; an `&~` parameter
  stands for the value at completion, and `old(y)` for the value at entry. A
  term that divides by zero or indexes out of range has an unspecified value.
- **[E-Test]** `d : ? e`. Evaluate `e`; add `d.true` or `d.false` to the scope's
  history.
- **[E-Decide]** `d : Δ ω(ā) ▸ …`. Evaluate the arguments left to right and run
  `ω` (sections 6.6 and 6.7). When it completes with `a` and payload `v̄`: if `a`
  is not among the listed alternatives, stuck. Otherwise add `d.a` to the
  history and bind the names of that alternative to `v̄`, as in [E-Bind].
- **[E-Capture]** `x := capture ω(ā)`. As [E-Decide], but no atom is added: `x`
  is bound to `out(a, v̄)`.
- **[E-Outcome]** The operation `outcome` takes a captured outcome by value and
  completes with its alternative and payload. The operation `V` takes a variant
  value and completes with its constructor and fields. Given a borrow of a
  variant, it completes with borrows of the fields, in the mode of that borrow.
- **[E-Impossible]** `⊥`. Stuck.

The remaining commands end scopes and are given in sections 6.7 to 6.9.

### 6.6 Calls and the host

**Defined controls.** `ω = f⟨τ̄⟩`, with argument values `v̄`.

- **[E-Call]** If `f` has a precondition and it is false for `v̄`, stuck.
  Otherwise push a new activation of `f` with its parameters bound to `v̄`. The
  caller waits at its decision.
- **[E-Exit]** `■ a {ē}` in an activation of `f`. Evaluate `ē` to `v̄`. If the
  condition `φₐ` of the contract is false for the parameters and `v̄`, stuck. End
  every scope of the activation (section 6.8) and pop it. The waiting decision
  completes with `a` and `v̄`. If there is no caller, the configuration is final
  with result `done(a, v̄)`.

**The host.** The world `W` holds the resource table, the event trace, and
whatever state the host keeps. An extern control is a relation
`host(W, f, v̄) ∋ (W′, a, v̄′)`: given the world and the arguments it yields a new
world, an alternative and a payload.

- **[E-Extern]** A decision on an extern control takes one step of the host
  relation and appends the event `(f, v̄, a, v̄′)` to the trace when `f` has an
  effect. If the precondition of `f` is false for `v̄`, stuck. A borrow that
  the host returns in a field `∈ ȳ` is a reborrow, by [B3], of a place within
  one of the arguments `ȳ`.
- **[E-Host]** If the host yields an alternative outside the contract, a payload
  of the wrong type, a borrow that is not such a reborrow, or a payload for
  which `φₐ` is false, the configuration
  ends in `host-violation`. This is a fault of the trusted host and not a stuck
  state: the claims of section 8 assume a host that respects its contracts.
- **[E-Resource]** A resource value `res(r)` is created only by the host, which
  records `r` as open. An extern control that takes a resource by value
  completes it: the host records `r` as closed, unless it returns `res(r)` in
  its payload. Between those two events exactly one slot holds `res(r)`.

Built-in operations `op` behave as extern controls with fixed contracts and no
host state, except for the collection shapes they change by [B5].

Effects are checked against allowances. Each activation carries the effect set
its control declares.

- **[E-Effect]** A host step with effect `ε` is stuck when some activation on
  the stack does not allow `ε`.

### 6.7 Loops

- **[E-Loop]** `d : ↻ ℓ(x̄ := ē) ⊢ φ {B}`. Evaluate `ē` to `v̄`. If `φ` is false
  for `v̄`, stuck. Push a scope for `B` with `x̄` bound to `v̄` and an empty
  history. The enclosing scope waits at site `d`.
- **[E-Again]** `↑ ℓ(ē)`. Evaluate `ē` to `v̄` in the current scope. End every
  scope up to and including the scope of `ℓ` (section 6.8). If `φ` is false for
  `v̄`, stuck. Push a fresh scope for the body of `ℓ` with `x̄` bound to `v̄`.
- **[E-Leave]** `□ a {ē}`. Evaluate `ē` to `v̄`. End the innermost loop scope.
  In the scope that contains the loop statement, add `d.a` to the history and
  bind the payload names of `a` to `v̄`. That scope resumes after the loop.

`↑` replaces the scope; it does not nest. A loop runs in constant space.

### 6.8 Ending a scope

A value **contains an obligation** when it contains `res(r)` with `r` open, a
captured outcome, or a live task.

- **[E-Scope]** To end a scope, visit each of its slots. A moved slot is passed
  over. If the value contains an obligation, the configuration is stuck. Any
  other value is destroyed. Then the owner tags of the scope are killed by [B4].

Payload and argument values are evaluated before the scope ends, so they have
already moved out of their slots.

### 6.9 Faults

A fault abandons the normal continuation of the execution it occurs in. It is
not an outcome and cannot be converted into one.

- **[E-Fault]** `fault e` evaluates the message and sets the mode to
  `fault(message)`. It is stuck when the control of the current activation does
  not permit `faults`, unless a fault boundary of the same activation encloses
  the statement.
- **[E-Unwind]** In mode `fault`, the innermost scope is ended by this variant
  of [E-Scope], which follows [T3] through each value: an open resource whose
  type is declared `on fault release` is released by the host, which appends a
  release event; an open resource of any other type makes the configuration
  stuck; a captured outcome and an aggregate are released part by part;
  everything else is destroyed. When an activation has no scopes
  left it is popped, and the unwinding continues in the caller, whose waiting
  decision completes with no alternative. An activation that does not permit
  `faults` is stuck when a fault reaches it, unless a boundary encloses the
  waiting decision.
- **[E-Root]** When the last activation is popped, the configuration is final
  with result `faulted(message)`.

Fault boundaries and contingency are carried over from v1 without change of
meaning ([reference](../reference/contingency.md)). In core terms:

- **[E-Boundary]** `d : boundary {B₁} contingency [x̄] {B₂}` moves the listed
  owned values `x̄` into the boundary's care and runs `B₁` in a scope of its own.
  `B₁` may fall off its end, which completes site `d` with its single
  alternative; this is the one exception to [E-End]. Its names and atoms do not
  escape. It may also end with any terminator.
- **[E-Contingency]** When a fault unwinds to the boundary, the scope of `B₁` is
  ended by [E-Unwind] and `B₂` runs with `x̄` and the fault's message. `B₂` cannot
  complete site `d`, exit, leave, or go again normally. Every history of `B₂`
  ends with `recover ↑ ℓ`, which restarts the enclosing supervised loop with
  the parameters it had when the boundary was entered; with `escalate`, which
  continues the original fault outward; with a new `fault`, which carries the
  original as its cause; or, in the entry control only, with `terminate n`.

### 6.10 Stuck states

A configuration is **stuck** when its mode is `run` or `fault`, it is not final,
and no rule applies. Each way of being stuck is a violation that the checker
excludes.

| | Stuck when | Clause | Excluded by |
| --- | --- | --- | --- |
| **X1** | A body reaches its end with the history undisposed | [E-End] | Coverage [K7] |
| **X2** | A name is read, moved or borrowed where it is unbound or moved | [E-Var] | Availability [O2] |
| **X3** | A name is bound twice in one run | [E-Bind] | [O1] |
| **X4** | An access goes through a dead tag, or writes through a shared one | [B1] | Loans [L3] |
| **X5** | A value that contains an obligation is destroyed or overwritten | [E-Scope], [E-Assign] | Obligations [O5] |
| **X6** | `⊥` runs | [E-Impossible] | [P6] |
| **X7** | A stated or implied condition is false: `⊢`, an invariant, a precondition, an outcome condition, an arithmetic range, a divisor, an index | [E-Oblige], [E-Arith], [E-Index], [E-Call], [E-Exit], [E-Loop], [E-Again] | Proof obligations [P1] to [P5] |
| **X8** | A fault occurs or arrives where it is not permitted, or meets an obligation that is not fault-releasable | [E-Fault], [E-Unwind] | Fault permission [F1], [F2] |
| **X9** | An effect occurs outside an allowance | [E-Effect] | Effects [F3] |
| **X10** | A value has the wrong shape, or a decision meets an unlisted alternative | [E-Decide] and typing | Typing [C1], acknowledgement [C2] |

Running forever is not stuck. `host-violation` is not stuck.

### 6.11 Observations

The observation of an execution is its event trace together with how it ends:
`done(a, v̄)`, `faulted(message)`, or divergence. Scope identities, tags, slot
states and histories are not observable. Given the host's choices, the
synchronous language is deterministic.

## 7. Checking

The checker works on one body at a time. Everything that depends on control flow
is a **history formula**: a Boolean formula over the atoms of the body. There is
no control-flow graph and no fixed point; a body is a straight line, and a loop
body is checked once.

### 7.1 Live histories and coverage

For a body `s₁ … sₙ` with `sᵢ = gᵢ ▷ cᵢ`, the checker computes which histories
are still running before each statement (`Lᵢ`) and which of them run it (`Rᵢ`):

```text
[K1]  L₁    = ⊤
[K2]  Rᵢ    = Lᵢ ∧ gᵢ
[K3]  Lᵢ₊₁  = Lᵢ                           cᵢ binds, captures, assigns, obliges or tests
            = Lᵢ ∧ (gᵢ → d.a₁ ∨ … ∨ d.aₙ)   cᵢ is a decision or loop at site d
            = Lᵢ ∧ ¬gᵢ                      cᵢ is a terminator
```

A decision can end a history without establishing an alternative: its operation
may fault, run forever, or, for a loop, exit the control from inside. So a
history is live after a decision it ran only if one of the alternatives holds. A
terminator is the case with no alternatives.

The **structure** `Ω` of a body says how atoms relate:

```text
[K4]  d.a → Rᵢ                  for each alternative a of the site d at statement i
      ¬(d.a ∧ d.b)              for alternatives a ≠ b of one site
[K5]  Rᵢ → d.true ∨ d.false     for a test d at statement i
```

- **[K6]** A loop body is checked for one generic iteration. Its own `L₁` is
  `⊤`, and every formula inside it is taken under the assumption that the loop
  statement runs.
- **[K7] Coverage.** `Θ ⊨ ¬Lₙ₊₁`: no feasible history is live at the end of the
  body.
- **[K8]** After a loop statement at site `d`, the enclosing body keeps the
  atoms and logical names of the generic iteration as hidden atoms and names,
  and `Ω` gains `d.a ↔ R_j₁ ∨ … ∨ R_jₖ` for the `□ a` statements `j₁ … jₖ` of
  the loop body. The facts of the iteration that left are therefore known after
  the loop, on the alternative it left with. An outer name that the loop
  changed has, after the loop, the logical name it had where that iteration
  left. Selectors cannot mention hidden atoms.

### 7.2 Facts and entailment

`Θ` is the checker's state at a statement: `L`, the structure `Ω`, the ownership
formulas of section 7.4, the loans of section 7.5, and a set `Ψ` of
**conditional facts** `G ⇒ φ`, each a history formula and a logical formula.

Logical formulas are built from terms over **logical names**. Each binder
introduces one. Every pure expression denotes a term: an operator or function
outside the interpreted fragment denotes an uninterpreted symbol applied to the
terms of its operands, so two evaluations on equal operands are equal. A place
read through a shared borrow denotes the value it refers to. The interpreted
fragment (linear integer arithmetic, equality, constructor tests, collection
length) and its decision procedure are fixed in the fact-logic document.

An **interpretation** is a truth assignment `h` to the atoms and a valuation `ν`
of the logical names. It is **admissible** for `Θ` when `h` satisfies `Ω` and,
for every `G ⇒ φ` in `Ψ`, `h ⊨ G` implies `ν ⊨ φ`.

- **[N1]** `Θ ⊨ F`, for a history formula `F`, when `h ⊨ F` in every admissible
  interpretation.
- **[N2]** `Θ; G ⊨ φ` when `ν ⊨ φ` in every admissible interpretation with
  `h ⊨ G`.
- **[N3]** The checker may decide these by any procedure that answers yes only
  when the entailment holds. Every yes carries a certificate that an independent
  kernel validates. When the procedure cannot answer yes, the program is
  rejected, with a counterexample interpretation where there is one and
  otherwise with the statement that the formula is outside the fragment. There
  are no timeouts.
- **[N4]** `Ψ` receives:

  | From | Fact |
  | --- | --- |
  | A bind `x := e` at statement `i` | `Rᵢ ⇒ x = e` |
  | An assignment `x ← e` at statement `i` | `Rᵢ ⇒ x′ = e`, for the fresh name `x′` of [N5] |
  | A write to a component of `x` | The fresh name agrees with the old one on its length and on every component other than the one written |
  | A test `d : ? e` | `d.true ⇒ e` and `d.false ⇒ ¬e` |
  | A decision at site `d` on an operation with `a … ⇐ φₐ` | `d.a ⇒ φₐ`, over the arguments and the payload names |
  | A decision on a captured outcome | As for the captured operation, over the logical names its arguments had at the capture |
  | An obligation proved at statement `i` | `Rᵢ ⇒ φ` |
  | The precondition of the control being checked | `⊤ ⇒ φ` |
  | The invariant of the loop whose body is being checked | `⊤ ⇒ φ` |
  | The type of a logical name | Its range, and `0 ≤ len(x) ≤` the largest length |

- **[N5]** A value that changes gets a new logical name, on the histories
  concerned. A borrow and the place it borrows denote the same value, so a
  change through either renames both. An assignment to a name or through a
  borrow, and an operation that receives `&~p` or an exclusive borrow, give a
  fresh name to the root of every place written or lent, and to the root of
  every place on which the borrow used holds a loan. Entry to a loop gives
  fresh names to the loop's parameters and to every outer name that its body
  can change in these ways. Facts stated earlier keep referring to the earlier
  name. When a name has several binders, its logical name on a history is that
  of the binder that ran.

Every check below is an entailment, so a history that cannot happen never
causes an error, and every reported error comes with a feasible history.

### 7.3 Typing

- **[C1]** Expressions, arguments, payloads and assignments are typed in the
  ordinary way. An argument is passed by value, as `&p` or as `&~p`, as the
  parameter's type says. An assignment needs a place whose root was bound with
  `~` or that passes through an exclusive borrow.
- **[C2] Acknowledgement.** A decision lists exactly the alternatives of its
  operation's contract, each once, with one name for each payload field. An
  alternative that is missing, unknown or repeated is an error. The surface
  rules for outcome groups and `forward` expand to this.
- **[C3]** `■ a {ē}` names an alternative of the control's contract. A loop
  statement lists exactly the alternatives that the `□` statements of its body
  use, each once, and all `□ a` of one loop agree on the payload types of `a`.
- **[C4]** `x := capture ω(ā)` gives `x` the type `outcome⟨K⟩` for the contract
  `K` of `ω`.

### 7.4 Ownership

For each name `x` the checker keeps two history formulas: `D(x)`, on which `x`
has been bound, and `M(x)`, on which it has been moved. `x` is **available** on
`D(x) ∧ ¬M(x)`.

- **[O0] Order.** The rules apply to the expressions of a statement one at a
  time, left to right, and then to its command. So the second `x` in `Δ f(x, x)`
  is a use after a move. A terminator's own checks, and the availability `A₀`
  of [O6], are taken after the statement's expressions.

- **[O1] Bind.** Binding `x` at statement `i`, by a bind or as a payload name of
  alternative `a`, requires `Θ ⊨ ¬(Rᵢ ∧ D(x))`. Afterwards `D(x)` includes
  `Rᵢ`, or `d.a` for a payload name.
- **[O2] Use.** Reading, moving or borrowing `x` at statement `i` requires
  `Θ ⊨ Rᵢ → D(x) ∧ ¬M(x)`. A formula reads the names it mentions and moves
  none.
- **[O3] Move.** Reading by value a name whose type is not `copy` moves it:
  `M(x)` afterwards includes `Rᵢ`. Only a whole name owned by the current body
  or an enclosing one moves; a component does not, and nothing moves out
  through a borrow.
- **[O4] Assign.** `x ← e` requires `Θ ⊨ Rᵢ → D(x)` and afterwards removes `Rᵢ`
  from `M(x)`. If the type of `x` has kind `must`, it also requires
  `Θ ⊨ Rᵢ → M(x)`: nothing may be overwritten. An assignment to a component, or
  through a borrow, requires [O2] for the root or the borrow, and a written
  place whose type is not of kind `must`.
- **[O5] Obligations.** A terminator at statement `i` ends scopes: `■` and
  `fault` end all scopes of the control, `□` the innermost loop, `↑ ℓ` the loops
  up to and including `ℓ`. For every name `x` of those scopes whose type has
  kind `must`, after the terminator's own expressions have been checked:
  `Θ ⊨ ¬(Rᵢ ∧ D(x) ∧ ¬M(x))`. For `fault`, names of fault-releasable type are
  exempt.
- **[O6] Outer names in loops.** Let `x` be a name of an enclosing body, and
  `A₀` its availability `D(x) ∧ ¬M(x)` at the loop statement.
  (a) At every `↑` of the loop, at statement `j`, `x` is available exactly when
  it was at entry: `Θ ⊨ Rⱼ → ((D(x) ∧ ¬M(x)) ↔ A₀)`. An iteration that goes
  round again leaves the ownership of outer names as it found it, which is what
  makes one generic iteration stand for all of them.
  (b) For each alternative `a` of the loop, the `□ a` statements agree on `x`:
  at all of them it is as it was at entry, or at all of them it is moved, or at
  all of them it is available. After the loop, the availability of `x` on `d.a`
  is that common answer.

The availability formula is also the destruction condition: when a scope ends,
exactly the names available on the current history are destroyed.

### 7.5 Loans

A **loan** `⟨p, m, G, H⟩` records that the place `p` is borrowed in mode `m` on
the histories `G`, and that the names in `H`, its **holders**, may carry a
borrow derived from it. A place is a name with a path, or the place **behind** a
borrow name `r`, written `*r`, with a path. `*r` is not part of `r`: the name
holds the borrow, and what is behind it belongs to someone else. In a place,
every index of one collection overlaps every other.

- **[L1] Creation.** `&p` or `&~p` at statement `i` creates a loan on `p` with
  `G = Rᵢ`. A borrow bound to a name is held by that name. A borrow passed as an
  argument is held by the operation, and by the payload names that the contract
  derives from that parameter with `∈`. Whenever a borrow flows into a name, by
  a bind, a copy, an assignment or a derived payload, that name becomes a holder
  of every loan its source holds. A borrow made through a borrow `r` is a loan
  on `*r`, and its holder also becomes a holder of every loan that `r` holds.
- **[L2] Liveness.** A statement evaluates its expressions left to right and
  then runs its command, which uses every borrow among their values: an
  operation its arguments, a loop statement its initial arguments, `↑` its
  arguments, `□` and `■` their payloads, a bind its value. A holder is **used
  after** an access when the command of the same statement uses it, or a later
  part of the statement or a later statement mentions it; a loop statement
  mentions what its body mentions. The condition is a history formula: the
  disjunction of `Rⱼ` over those statements. A loan is **live after an access**
  on `G`, conjoined with the disjunction of those conditions for its holders.
  The origin of a borrow parameter is a loan that is live throughout the
  control.
- **[L2a] Loops.** One generic iteration stands for all of them, so a loan that
  can survive an iteration is treated as present from the start of the next. If
  a loan made in a loop body has a holder that outlives the iteration (a name
  of an enclosing body, or a loop parameter that receives it through `↑`), then
  throughout the body that holder holds the loan on every history: its `G` is
  the condition on which the loop runs. A loop parameter likewise holds every
  loan of its initial argument. A holder that outlives the iteration is used
  after an access when any statement of the loop body mentions it, or a
  statement after the loop does.
- **[L3] Conflict.** These are **accesses**. Reading a place and borrowing it
  shared are reads. Assigning to it, moving it, borrowing it exclusively and
  changing a collection's shape are writes. Using a borrow name `r` in any way
  (reading or writing through it, passing it, moving it) is an access to `*r`:
  a write when `r` is exclusive, a read when it is shared. Ending a scope is a
  write to every name of that scope that owns its value. An access **goes
  through** a loan when it is made through one of the loan's holders. For an
  access at statement `i` to place `p′`, and every loan `⟨p, m, G, H⟩` with `p`
  overlapping `p′` that the access does not go through, if the access is a
  write or `m` is exclusive: `Θ ⊨ ¬(Rᵢ ∧ L)`, where `L` is the condition on
  which the loan is live after the access.
- **[L4] Escape.** A borrow or view in a payload field of `■ a {ē}` holds loans
  only on places behind the parameters that the field names with `∈`, and
  loans that those parameters hold. Loans on the control's own names cannot be
  live there, because the exit ends their scope ([L3]); the same holds for `□`
  and `↑` and the names of the loop scopes they end.
- **[L5] Views.** A view is a holder of every loan its borrow fields hold. It
  is used as one borrow: shared if all its borrows are shared, exclusive
  otherwise.

Two consequences are worth stating. Two payload fields may derive from one
exclusive parameter only if they borrow places that do not overlap, because
the exit uses both at once. And a borrow stored in an outer name inside a loop
cannot point at anything the iteration owns, because ending the iteration is a
write to it.

Because a body is a straight line, liveness is one backward pass over
statements whose `Rⱼ` the forward pass has already computed.

### 7.6 Proof obligations

Each of these is an entailment by [N2], on the histories that run the statement.

- **[P1]** `⊢ φ` at statement `i`: `Θ; Rᵢ ⊨ φ`.
- **[P2]** A loop with invariant `φ`: `φ` with the initial arguments, at the
  loop statement; `φ` with the new arguments, at every `↑`. Inside the body it
  is assumed by [N4].
- **[P3]** A decision or capture on an operation with a precondition: the
  precondition, with the arguments.
- **[P4]** `■ a {ē}`: the condition `φₐ` of the contract, with the parameters
  and `ē`.
- **[P5]** A partial primitive: the result of integer arithmetic lies in its
  type; a divisor is not zero; an index is less than the length, wherever an
  indexed place is read, borrowed or assigned; a narrowing conversion fits.
- **[P6]** `⊥` at statement `i`: `Θ ⊨ ¬Rᵢ`. No feasible history runs it.
- **[P7]** A proved obligation is a fact afterwards, by [N4].
- **[P8]** A function body is checked as one expression under the function's
  precondition, with [P5] for its primitives. A call of a function is checked
  by [P3], wherever it occurs.

### 7.7 Faults and effects

- **[F1]** A statement **may fault** when it is `fault e`, or a decision or
  capture on an operation whose signature says `faults`. It requires `faults`
  in the signature of the control being checked, or an enclosing boundary.
- **[F2]** At a statement `i` that may fault, for every name `x` in scope whose
  type is not fault-releasable, after the statement's own arguments have moved:
  `Θ ⊨ ¬(Rᵢ ∧ D(x) ∧ ¬M(x))`. A value lent to a callee is still owned by the
  caller, so this covers faults raised below.
- **[F3]** The effects of the operations in a body are within the `uses` of its
  control. A `pub` control declares them; a private control's are inferred as
  the union. Functions have none.
- **[F4]** The unproven form `assume ⊥ "reason"` is `fault "reason"`. It needs
  [F1] and [F2] and is listed in the trust report.

## 8. Claims

A program is **checked** when every declaration satisfies sections 3, 4 and 7. A
host is **faithful** when it never yields `host-violation`. An execution starts
with one activation of an entry control, on arguments of its parameter types
that satisfy its precondition.

**Theorem 1 (safety).** No execution of a checked program under a faithful host
reaches a stuck configuration.

Read against the table of section 6.10: no history is left undisposed; no value
is read where it is unavailable; no borrow is used after it died or while it
conflicts; no obligation is destroyed; no `⊥` is reached; no stated condition
fails; no fault or effect occurs where it is not permitted.

**Theorem 2 (resources).** In every finite execution of a checked program under
a faithful host, each resource the host created has been closed by exactly one
completing operation, or released by its declared fault disposition, or is part
of the final payload.

**Theorem 3 (erasure).** See section 9.

**Theorem 4 (expansion).** Expanding a decision on a defined control in place
(the control's body with fresh names and sites, its parameters bound to the
arguments, and each exit replaced by the caller's continuation for that
alternative) yields a checked program with the same observations.

The proofs rest on four invariants of reachable configurations, each relating a
scope at statement `i` to the checker's state there:

- **Adequacy.** The histories of the activation's scopes, the hidden atoms of
  each loop that has left as its leaving iteration had them, every other atom
  false, and the current values form an admissible interpretation that
  satisfies `Lᵢ`.
- **Ownership.** A name has an available slot exactly when the history
  satisfies `D(x) ∧ ¬M(x)`, a moved slot exactly when it satisfies
  `D(x) ∧ M(x)`, and no slot otherwise.
- **Loans.** Every borrow in an available slot whose holder is used later on
  this history has a live tag.
- **Linearity.** Every open resource is in exactly one available slot, or is an
  argument or payload in transit.

Order of evidence, as the plan's decision D6 sets it: each claim is first a
property of the phase 2 fuzzer, where a stuck oracle run of a checked program is
a defect in the checker or in this document, and is then mechanized in Lean 4 in
stages: histories, coverage and proof obligations (X1, X6, X7); ownership and
obligations (X2, X3, X5, X8, Theorem 2); borrows (X4); erasure; expansion.

## 9. Erasure

The **erased semantics** is the instrumented semantics with the bookkeeping
removed. It is what release code implements.

- **[R1]** Slots hold values. There are no moved marks and no tags. A borrow is
  an address.
- **[R2]** `⊢`, invariants, preconditions, outcome conditions and effect
  allowances are not evaluated. Integer arithmetic and indexing are unchecked.
  `⊥` has no behavior, and no code is generated for it.
- **[R3]** Histories remain, as one bit per atom that some selector or
  destruction condition mentions.
- **[R4]** At the end of a scope the names destroyed are those whose
  availability formula holds on the current history ([O5], [O6]). No value
  carries a flag of its own.
- **[R5]** Under a fault, resources declared `on fault release` are released
  under the same condition.

**Theorem 3 (erasure).** For a checked program and a faithful host, the
instrumented and the erased semantics yield the same observation from the same
initial state and the same host choices.

The event trace is a device of this specification. Release code performs the
effects and records nothing; recording, fuel and scripted hosts belong to the
instrumented profile.

An implementation may also destroy `drop` values early by [T2], and may expand
calls by Theorem 4.

## 10. Slices, and what draft 1 leaves out

| Slice | Constructs | Clauses |
| --- | --- | --- |
| 1. Scalars and decisions | Integers, `bool`, tests, decisions on controls and externs, loops, exits, `⊥`, obligations | W, H, K, N, P, C; X1, X6, X7, X10 |
| 2. Data | Structs, variants, vectors, maps, strings, moves, resources, captured outcomes, faults, boundaries | T, O, F; X2, X3, X5, X8, X9 |
| 3. Borrows | `&`, `&~`, payload borrows, views | B, L; X4 |
| 4. Tasks | `async`, `spawn … into`, `await`, `cancel`, task sets | Draft 2 |

Requirements that draft 2 must meet for tasks, all carried over from v1's
accepted decisions:

- A task set is a local of kind `must`. Its scope cannot end while it has
  members, so children finish before the scope that owns them. This is the one
  rule that lets a child borrow from its parent.
- Tasks interleave only at `await`. The scheduler's choices are host choices;
  given them, execution is deterministic.
- Cancellation is a request. It arrives as the `cancelled` alternative of an
  awaited operation and never abandons a frame.
- A child's fault reaches the boundary that owns the set at the next scheduler
  safe point.
- The borrow rules extend to tasks without a new mechanism: what a child
  captures is a loan held for the life of the set.

Also outside draft 1: capability declarations and coherence, modules and
visibility, callable parameters and closures, numeric conversions, text views,
`∀` loops (sugar over a loop with an index and a generated invariant), process
entry and launch declarations.

## 11. Choices made in this draft

These follow from the accepted decisions but were not spelled out in the plan.
They are recorded for the author's acceptance of phase 1.

1. **A body is a flat list of guarded statements.** Nesting exists only in the
   surface. Names and atoms belong to the whole body.
2. **A decision may end a history without an alternative.** "Live after a
   decision" means "one of its alternatives holds" ([K3]). A loop that never
   leaves needs nothing after it.
3. **No shadowing.** One name may have several binders on disjoint histories if
   they agree on its type.
4. **Expressions are pure and total.** Partial primitives carry proof
   obligations. Everything with an effect or several outcomes is an operation.
5. **A decision lists exactly the contract's alternatives** ([C2]). This is the
   core form of decision D3.
6. **Conditions cross calls only through contracts.** A defined control's
   conditions are proved at its exits and assumed at its calls. Loops need no
   declared conditions, because the facts of the leaving iteration stay visible
   ([K8]).
7. **Borrow validity is defined by tags** in a tree discipline, as Miri's
   models are. It can be this small because borrows are second-class.
8. **A misbehaving host is not a stuck program.** The claims are conditional on
   the host contracts, which the trust report lists.
9. **Destroying a `drop` value is unobservable.** There are no destructors;
   anything that must happen is a resource protocol.
10. **Borrows never instantiate type parameters, and no borrow refers to a
    borrow** ([W6]). A type parameter has kind `must` unless its bounds say
    otherwise ([T1a]).
11. **Using an exclusive borrow at all is a write to what is behind it**
    ([L3]). The semantics is more permissive, since moving a borrow touches no
    tag; the checker's rule is what keeps two payload borrows from aliasing.
12. **Functions may state preconditions**, and their bodies are checked under
    them ([P8]).

Questions for the author, with the recommended answer first:

| | Question | Recommendation | Alternative |
| --- | --- | --- | --- |
| Q1 | May a component of kind `drop` or `must` be moved out of a struct while the rest stays? | **No** in v2.0: take the owner apart or move it whole. Ownership stays one formula per name. | Partial moves as in Rust: one formula per path, and partly moved values that cannot be passed on. |
| Q2 | When a loop leaves with alternative `a`, may an outer name be moved on some ways of leaving and not on others? | **No** ([O6]): all `□ a` sites agree. The remedy is a second alternative or passing the value as loop state. | Allow it and export an iteration bit as a hidden drop flag. |
| Q3 | What is the type of lengths and indices? | **A dedicated unsigned type of address width**, since v2 has several integer widths. The reference programs, written with `u32` as in v1, would change. | Keep `u32`, capping collections at 2³² elements and requiring conversions for wider counters. |
| Q4 | Is `¬` allowed in a Rails path label? | **Yes, sparingly**: the core has it and it is sometimes the shortest way to say "every other way". | Labels use `∧` and `∨` only; negation is written by naming paths. |
| Q5 | An arm disposes an alternative that the checker can prove impossible. | **A warning** in non-generic code, suggesting `⊥`; silent in generic code, where it may be reachable in another instance. | An error, or no diagnostic. |
