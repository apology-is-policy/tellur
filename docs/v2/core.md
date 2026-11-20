# Villanelle v2 core calculus

Status: **draft 2**, 19 November 2026. Phase 1, deliverable 1 of the
[redesign plan](../redesign-plan.md). Accepted by the author on 20 November
2026 as the working specification ([index](README.md)). Nothing here is
implemented.

Two independent reviews have been folded in. The first found seven programs
that the checking rules accepted and the semantics left stuck, most of them in
the loan rules. The second, of the repaired rules, found twelve further
problems, seven of them confirmed: accepted programs that get stuck or lose a
resource, a grammar that could not write the borrows the rules speak of, and
an ambiguity in what a borrow name stands for. The rules below include both
sets of repairs; choices 11 and 13 to 18 of section 11 come from the second.
Writing the examples of the [reference model](references.md) added five
clarifications and one rule, [E-Operands] (its section 13; choice 19 here).
Draft 2 adds tasks (sections 6.12 and 7.8). A third review, of all five
phase 1 documents together, read the tasks for the first time and found four
programs that the task rules accepted and the semantics left stuck or with a
lost resource, and that the named boundaries which the corpus added had no
checking rule. The repairs are in [TK3] to [TK6], [E-Await], [E-AwaitHost],
[E-Stop], [N5], [F5] and [F6] (choices 25 to 28 of section 11) and have not
been reviewed again. The author has answered the five questions that draft 1
raised (section 11).

[Index of the v2 design](README.md) · [Fact logic](facts.md) ·
[Reference model](references.md) · [Surface](surface.md) ·
[Plan](../redesign-plan.md) ·
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

Draft 1 specified the synchronous language: scalars, decisions, loops and
calls (slice 1), data, ownership, resources and faults (slice 2), borrows and
views (slice 3). Draft 2 adds tasks (slice 4) in sections 6.12 and 7.8. The
theory behind `⊨` and the certificates are the
subject of the [fact logic](facts.md); the Rails grammar and the elaboration
into this core are the subject of the [surface](surface.md) document.

## 2. The core in one example

Binary search, as written in Rails:

```text
control bound<order: ordering.comparator>(
    source: &[order.item], rule: &order, needle: &order.item, upper: bool
) ■ position {index: usize} ⇐ index ≤ len(source)
│ ↻ search(start: usize := 0, count: usize := len(source))
│ ║ ⊢ start + count ≤ len(source)
│ ║ ? count = 0 → ■ position {start}
│ ║
│ ║ half: usize := count ÷ 2
│ ║ middle: usize := start + half
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
    ■ { position {index : usize} ⇐ index ≤ len(source) }

1  ⊤ ▷ s : ↻ search(start : usize := 0, count : usize := len(source))
             ⊢ start + count ≤ len(source)
     1  ⊤                 ▷ z : ? count = 0
     2  z.true            ▷ ■ position {start}
     3  ⊤                 ▷ half : usize := count ÷ 2
     4  ⊤                 ▷ middle : usize := start + half
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
| `start + half` fits `usize` | 4 | `half ≤ count`, the invariant, and the range of `len` |
| `middle < len(source)` | 5 | `count ≠ 0`, so `half < count`; then the invariant |
| Invariant for the next iteration | 7 | `start + half ≤ start + count` |
| `middle + 1` and `count - half - 1` in range; invariant | 8 | `half < count`; the sum is `start + count` |

No arm is a filler, and no check remains at run time. Deleting the `⊢` line makes
the obligations at 2, 4, 5 and 8 unprovable, and the program is rejected with a
counterexample. Lengths and indices have the type `usize` (section 11, Q3).

## 3. Syntax

Metavariables: `x`, `y` variables; `d` decision sites; `a`, `b` alternatives; `f`
controls; `fn` functions; `ℓ` loops; `S`, `V`, `W`, `R`, `O` declared type names;
`α` type parameters; `ε` effect labels; `φ` logical formulas (section 7.2); `n`
integer literals; `o` operands. An overbar is a sequence.

```text
Program       P ::= D̄
Declaration   D ::= struct S⟨ᾱ⟩ {x̄ : τ̄}
                  | variant V⟨ᾱ⟩ {c̄ {x̄ : τ̄}}
                  | view struct W⟨ᾱ⟩ {x̄ : τ̄}
                  | outcome O⟨ᾱ⟩ K
                  | extern resource R [on fault release]
                  | function fn⟨ᾱ⟩(x̄ : τ̄) [⊢ φ] : τ := e
                  | [pub] [async] control f⟨ᾱ : C̄⟩(x̄ : τ̄) Sig := B
                  | extern [async] control f(x̄ : τ̄) Sig
Signature   Sig ::= [⊢ φ] ■ K [faults] [uses ε̄]
Contract      K ::= { a₁ {q̄₁} [⇐ φ₁] │ … │ aₙ {q̄ₙ} [⇐ φₙ] }
Payload field q ::= x : τ  |  x : τ ∈ ȳ          the second for a borrow or view type

Type          τ ::= bool | uN | iN | usize | string
                  | S⟨τ̄⟩ | V⟨τ̄⟩ | [τ] | τ ⇀ τ | R | outcome⟨K⟩
                  | &τ | &~τ | W⟨τ̄⟩ | tasks⟨f⟩
                  | α | α.t

Body          B ::= s₁ … sₙ
Statement     s ::= g ▷ c
Selector      g ::= ⊤ | d.a | g ∧ g | g ∨ g | ¬g
Command       c ::= [~]x [: τ] := o                     bind
                  | S {x̄} := e                           take a struct apart
                  | [~]x := capture ω(ō)                 capture a completed outcome
                  | p ← e                                assign
                  | ⊢ φ                                  obligation
                  | d : ? e                              test
                  | d : Δ ω(ō) ▸ a₁ {x̄₁} │ … │ aₙ {x̄ₙ}    decision
                  | d : ↻ ℓ(x̄ : τ̄ := ō) [⊢ φ] {B} ▸ a₁ {x̄₁} │ … │ aₙ {x̄ₙ}    loop
                  | spawn f⟨τ̄⟩(ō) into x                  start a member of a task set
                  | cancel x                              ask the members of a set to stop
                  | d : boundary {B} contingency [x̄] {B}  lexical fault boundary
                  | supervise b(ō)                        run a named boundary
                  | ↑ ℓ(ō)                               again        (terminator)
                  | □ a {ō}                              leave        (terminator)
                  | ■ a {ō}                              exit         (terminator)
                  | fault e                              fault        (terminator)
                  | ⊥                                    impossible   (terminator)
                  | terminate | escalate | recover ↑ ℓ   in a boundary (terminators, [F5], [F6])
Operation     ω ::= f⟨τ̄⟩ | α.m | op | outcome | V
                  | await f⟨τ̄⟩ | await next | run f⟨τ̄⟩
Operand       o ::= e | &p | &~p
Expression    e ::= n | true | false | "…" | x | e.x | S {ē} | V.c {ē} | [ē] | ∅
                  | fn⟨τ̄⟩(ē) | e ⊕ e | ⊖ e | len(p) | p[e] | clone(p) | W {ō}
                  | tasks⟨f⟩() | count(x)
Place         p ::= x | p.x | p[e]
```

`op` ranges over the built-in operations on vectors, maps and strings that have
several outcomes or an effect. `outcome` decides a captured outcome value and `V`
decides which constructor a variant has, given the variant or a borrow of it.
`⊕` ranges over arithmetic, comparison and Boolean operators. `[ē]` is a vector
of the given elements and `∅` the empty map. `p[e]` indexes a
vector; a map is reached only through operations. `len(p)` applies to vectors,
strings and maps. An **operand** stands where a value is kept: it is an
expression, or a borrow `&p` or `&~p` of a place. Borrows are written nowhere
else.

The forms with `async`, `await`, `run`, `spawn`, `cancel`, `tasks` and `count`
belong to tasks (sections 6.12 and 7.8). `await next` takes the name of a task
set as its one operand.

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
  The parameters of a control are not mutable: a parameter is never assigned
  or lent exclusively, though what is behind an `&~` parameter is.
- **[W5]** Expressions have no effect, cannot fault and always terminate.
  Functions are not recursive. Everything with an effect or several outcomes is
  an operation, and occurs only in a decision or a capture.
- **[W5a]** A function reads its arguments and keeps nothing. Its parameters
  have types of kind `copy`, or shared borrow types `&τ`. The argument for a
  borrow parameter is a place, which the function reads where it is; no borrow
  value is made. Its result is not a borrow or a view.
- **[W6]** Borrow types `&τ`, `&~τ` and view types occur only as the types of
  parameters, locals, loop parameters and payload fields, and as fields of view
  structs. They never occur inside `S`, `V`, `[τ]`, `⇀` or `outcome⟨K⟩`, never
  as a type argument, and never as what a borrow refers to. The fields of a
  view struct have borrow types, view types and types of kind `copy` only, so
  a view never owns anything that must be completed.
- **[W7]** A contract lists each alternative once. A formula `⇐ φ` mentions only
  the parameters and the fields of its own alternative. For an `&~` parameter
  `y`, `y` means the value at completion and `old(y)` the value at entry.
- **[W8]** A name of borrow type stands for the borrow itself where a borrow
  is expected: as an operand for a parameter, a payload field, a loop
  parameter or a field of a view that has borrow type, as the value of a bind
  whose written type is a borrow type, and as a payload of `□` ([W9]).
  Everywhere else it stands for the value it refers to; an untyped bind
  expects a value. A field of a view that has borrow type is read in the same
  two ways. `r ← e` writes the value of `e` through `r`. A name of borrow or
  view type is never assigned: it borrows the same place for as long as it
  lives.
- **[W9]** A loop statement lists the alternatives that the `□` statements of
  its body use, with a name for each payload field. A payload field of a loop
  has the type of the operand that leaves through it, so a name of borrow or
  view type leaves as the borrow or view it is. A payload field of a
  contract that has a borrow or view type always has `∈`. A payload name of a
  loop has none; it holds the loans of the value that leaves ([L1]).

- **[W10]** `await`, `spawn`, `cancel` and `tasks⟨f⟩()` occur only in the body
  of an `async control`, and `run` only in a control that is not `async`. An
  `async control` is invoked only by `await f`, `run f` and `spawn f`. The `f`
  of `spawn f` and of `tasks⟨f⟩` is a defined `async control`, the
  **producer** of the set. The two bodies of a named boundary are not `async`,
  whatever control supervises it: they may use `run` and may not `await`.
- **[W11]** `tasks⟨f⟩` is the type of a local, bound once by
  `~x : tasks⟨f⟩ := tasks⟨f⟩()`, and of nothing else: not a parameter, a loop
  parameter, a payload field, a field or an element. A task set is never
  moved, lent or assigned. `spawn … into x`, `await next(x)`, `cancel x` and
  `count(x)` name it directly.
- **[W12]** The alternative `cancelled` never has a payload. An
  `extern async control` that lists it is **cancellable**, and there the
  alternative has no condition either: a cancelled task receives it without
  the host being asked ([E-AwaitHost]). In the contract of
  a defined control it is an alternative like any other: the body exits with
  it, in practice to pass on the `cancelled` of an operation it awaited. The
  contract of a producer has no alternative `empty`.

A decision with one alternative and no payload is how the core writes a plain
call such as `emit(e)`.

## 4. Types, kinds and contracts

### 4.1 Usage kinds

Every type has one of three usage kinds, ordered `copy < drop < must`.

| Kind | Meaning | Types |
| --- | --- | --- |
| `copy` | May be duplicated; reading it leaves the source available | `bool`, integers, `&τ`, views of shared borrows, structs and variants declared `copy` whose components are all `copy` |
| `drop` | Has one owner; reading it by value moves it; may be destroyed silently | `string`, `[τ]`, `τ ⇀ τ`, `&~τ`, views that hold an exclusive borrow, other structs and variants |
| `must` | Has one owner; may not be destroyed; must be consumed by an operation | Resources `R`, `outcome⟨K⟩`, task sets |

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
  all of whose components and payload types are fault-releasable, or is a task
  set, whose members can be stopped ([E-Stop]).

### 4.2 Contracts

A contract is a closed set of alternatives. Each alternative has a payload and
may state a condition `⇐ φ` that holds whenever it occurs. A signature may also
state a precondition `⊢ φ`, the permission `faults`, and the effects it `uses`.

- **[T4]** A borrowed payload field `x : &τ ∈ ȳ` names the borrow or view
  parameters it may be derived from. A payload borrow has no other origin.
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

An expression is **consumed** when its value is kept: as the value of a bind,
the source of an assignment, an operand given by value, a payload field, a
field of a constructor, or the value of a loop parameter. Every other
occurrence is a **read**: an operand of an operator, an argument of a
function, the base of a field or an index, the place of `len` or `clone`, a
term of a formula.

- **[E-Var]** `x` looks up the slot of `x`. If `x` has no slot, or the slot is
  `◌`, evaluation is stuck. A read, and a consumed `x` whose type has kind
  `copy`, yield the value after a read access and leave the slot as it is. A
  consumed `x` of any other kind yields the value after a write access, and
  the slot becomes `◌`. Where [W8] makes a borrow name stand for what it
  refers to, that value is read through the borrow's tag.
- **[E-Field]**, **[E-Index]** `e.x` and `p[e]` read a component after a read
  access to its place, through the owner tag or, when the path passes through
  a borrow, through that borrow's tag. A consumed component has kind `copy`
  ([O3]). An indexed place, wherever it is evaluated (read, borrowed or
  assigned), is stuck when the index is not less than the length.
- **[E-Arith]** `e₁ ⊕ e₂` on integers of type `τ` is stuck when the mathematical
  result is not a value of `τ`, and for division when the divisor is zero.
  Division truncates toward zero, and the remainder has the sign of the
  dividend. The wrapping and saturating operators are total and never stuck.
- **[E-Bool]** `e₁ ∧ e₂` evaluates `e₁` and, only when it is true, `e₂`.
  `e₁ ∨ e₂` evaluates `e₂` only when `e₁` is false. Expressions have no
  effects ([W5]), so the order shows in one thing: the left operand can keep
  the right one from being evaluated where it would be stuck, as in
  `b ≠ 0 ∧ a ÷ b > 1`. `¬` and the comparisons evaluate their operands.
- **[E-Fn]** `fn⟨τ̄⟩(ē)` evaluates the arguments left to right and then the body
  of `fn` with its parameters bound. A place given for a borrow parameter is
  read where it is ([W5a]). It is stuck when the precondition of `fn` is false
  for them.
- **[E-Borrow]** An operand `&p` or `&~p` resolves `p` to a place and creates a
  borrow of it by [B3].
- **[E-Operands]** An operand **lends** when it is `&p` or `&~p`, or a view
  constructor with a field that lends. Where a command or a view constructor
  takes several operands, those that do not lend are evaluated first, left to
  right, and those that lend afterwards, left to right. Expressions have no
  effects ([W5]), so only the bookkeeping can tell: every read of a statement
  comes before any place is lent by it, and `Δ push(&~v, len(v))` reads the
  length first.
- **[E-Build]** `S {ē}`, `V.c {ē}`, `[ē]`, `∅`, `W {ō}` and `clone(p)` build
  values. `clone` reads its place and requires a type whose components are all
  clonable; resources, captured outcomes and exclusive borrows are not.
- **[E-Temp]** A value that was read and not consumed, and that no slot holds
  (the result of a constructor, a function or `clone`), is destroyed when its
  reader is done. If it contains an obligation (section 6.8), stuck.

There are no partial moves: a component of kind `drop` or `must` leaves its
owner only when the owner moves as a whole or is taken apart, a struct by
[E-Unpack] and a variant by a decision on it.

### 6.5 Statements

Let the innermost scope be at statement `g ▷ c`. If the mode is `run`:

- **[E-Skip]** If `Φ* ⊭ g`, the statement is skipped: `pc` advances.
- **[E-End]** If `pc` is past the last statement, the configuration is stuck.
  A body never falls off its end.

Otherwise `c` runs:

- **[E-Bind]** `[~]x : τ := o`. Evaluate `o` to `v`. If `x` already has a slot in
  this scope, stuck. Otherwise `x` gets the slot `v` and an owner tag.
- **[E-Unpack]** `S {x̄} := e`. Evaluate `e` to `S {v̄}` and bind each name to its
  component, as in [E-Bind].
- **[E-Assign]** `p ← e`. Evaluate `e` to `v`, then write `v` at the place of
  `p`: a write access by [B1]. If the value being replaced contains an
  obligation (section 6.8), stuck. A moved slot may be assigned as a whole and
  becomes available. A name that has no slot in this run cannot be assigned:
  stuck.
- **[E-Oblige]** `⊢ φ`. If `φ` is false in the current state, stuck.
- **[E-Formula]** A formula is evaluated on current values, over the
  mathematical integers, and makes no access: it reads through borrows and
  changes no tag. In a contract condition a parameter stands for the value it
  had at entry, which the instrumented semantics keeps; an `&~` parameter
  stands for the value at completion, and `old(y)` for the value at entry. A
  term that indexes out of range has a value of the element type, and a term
  that divides by zero has an integer value. This document does not fix those
  values, but each depends only on the operands of its term: the same term on
  equal operands has the same value wherever it is evaluated. A formula has
  no order of evaluation: every name it mentions is read, on both sides of
  `∧` and `∨`. A function in a formula is its body on the argument terms
  ([fact logic](facts.md), [I8]), and its precondition is not asked there.
- **[E-Test]** `d : ? e`. Evaluate `e`; add `d.true` or `d.false` to the scope's
  history.
- **[E-Decide]** `d : Δ ω(ō) ▸ …`. Evaluate the operands ([E-Operands]) and run
  `ω` (sections 6.6 and 6.7). When it completes with `a` and payload `v̄`: if `a`
  is not among the listed alternatives, stuck. Otherwise add `d.a` to the
  history and bind the names of that alternative to `v̄`, as in [E-Bind].
- **[E-Capture]** `x := capture ω(ō)`. As [E-Decide], but no atom is added: `x`
  is bound to `out(a, v̄)`.
- **[E-Outcome]** The operation `outcome` takes a captured outcome by value and
  completes with its alternative and payload. The operation `V` takes a variant
  value and completes with its constructor and fields. Given a borrow of a
  variant, it completes with borrows of the fields, in the mode of that borrow:
  each is a reborrow of its field through the argument, as if a contract said
  `∈` the argument for every field.
- **[E-Impossible]** `⊥`. Stuck.

The remaining commands end scopes and are given in sections 6.7 to 6.9.

### 6.6 Calls and the host

**Defined controls.** `ω = f⟨τ̄⟩`, with argument values `v̄`.

- **[E-Call]** If `f` has a precondition and it is false for `v̄`, stuck.
  Otherwise push a new activation of `f` with its parameters bound to `v̄`. The
  caller waits at its decision.
- **[E-Exit]** `■ a {ō}` in an activation of `f`. Evaluate `ō` to `v̄`. If the
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
  of the wrong type, a borrow that is not such a reborrow, payload borrows that
  are not all alive together when it completes (an exclusive one overlaps no
  other), or a payload for which `φₐ` is false, the configuration
  ends in `host-violation`. So it does when the host answers an extern control
  that is pure ([fact logic](facts.md), [I10]) differently on equal arguments. This is a fault of the trusted host and not a stuck
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

- **[E-Loop]** `d : ↻ ℓ(x̄ := ō) ⊢ φ {B}`. Evaluate `ō` to `v̄`. If `φ` is false
  for `v̄`, stuck. Push a scope for `B` with `x̄` bound to `v̄` and an empty
  history. The enclosing scope waits at site `d`.
- **[E-Again]** `↑ ℓ(ō)`. Evaluate `ō` to `v̄` in the current scope. End every
  scope up to and including the scope of `ℓ` (section 6.8). If `φ` is false for
  `v̄`, stuck. Push a fresh scope for the body of `ℓ` with `x̄` bound to `v̄`.
- **[E-Leave]** `□ a {ō}`. Evaluate `ō` to `v̄`. End the innermost loop scope.
  In the scope that contains the loop statement, add `d.a` to the history and
  bind the payload names of `a` to `v̄`. That scope resumes after the loop.

`↑` replaces the scope; it does not nest. A loop runs in constant space.

### 6.8 Ending a scope

A value **contains an obligation** when it contains `res(r)` with `r` open, a
captured outcome, or a task set with members.

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
- **[E-Supervise]** A **named boundary** is a declaration
  `boundary b(x̄ : τ̄) retains ȳ {B₁} contingency(m : string) {B₂}`, with `ȳ`
  among `x̄`. Its parameters own what they hold: no borrow and no view crosses
  into it. `supervise b(ō)` evaluates and moves the operands and runs `B₁` as
  a boundary of its own, with `faults` permitted inside. Every history of
  `B₁` ends with `terminate`. When a fault unwinds to it, the scope of `B₁`
  is ended by [E-Unwind] except for the retained `ȳ`, and `B₂` runs with `ȳ`
  and the message, which is stuck if a retained name is not available then;
  [F6] requires it to be available at every statement of `B₁` that may
  fault. Every history of `B₂` ends with `terminate`, with
  `escalate` or with a new `fault`. After `terminate` in either body the
  supervising control continues with its next statement. It learns nothing:
  `supervise` has no alternatives, no payload and no fact, and it may not
  fault unless `B₂` can end otherwise than by `terminate`. This is the one
  form in which a control goes on after a fault below it, and the control
  cannot tell that there was one.

### 6.10 Stuck states

A configuration is **stuck** when its mode is `run` or `fault`, it is not final,
and no rule applies. Each way of being stuck is a violation that the checker
excludes.

| | Stuck when | Clause | Excluded by |
| --- | --- | --- | --- |
| **X1** | A body reaches its end with the history undisposed | [E-End] | Coverage [K7] |
| **X2** | A name is read, moved or borrowed where it is unbound or moved, or assigned where it is unbound | [E-Var], [E-Assign] | Availability [O2], [O4] |
| **X3** | A name is bound twice in one run | [E-Bind] | [O1] |
| **X4** | An access goes through a dead tag, or writes through a shared one | [B1] | Loans [L3], mutable places [C1] |
| **X5** | A value that contains an obligation is destroyed or overwritten | [E-Scope], [E-Assign], [E-Temp] | Obligations [O4], [O5], temporaries [O3a], task sets [TK2] |
| **X6** | `⊥` runs | [E-Impossible] | [P6] |
| **X7** | A stated or implied condition is false: `⊢`, an invariant, a precondition, an outcome condition, an arithmetic range, a divisor, an index | [E-Oblige], [E-Arith], [E-Index], [E-Call], [E-Exit], [E-Loop], [E-Again] | Proof obligations [P1] to [P5] |
| **X8** | A fault occurs or arrives where it is not permitted, or a fault or a stop meets an obligation that is not fault-releasable | [E-Fault], [E-Unwind], [E-Stop] | Fault permission [F1], [F2], tasks [TK4], [TK5] |
| **X9** | An effect occurs outside an allowance | [E-Effect] | Effects [F3] |
| **X10** | A value has the wrong shape, or a decision meets an unlisted alternative | [E-Decide] and typing | Typing [C1], acknowledgement [C2] |

Running forever is not stuck. `host-violation` is not stuck.

### 6.11 Observations

The observation of an execution is its event trace together with how it ends:
`done(a, v̄)`, `faulted(message)`, or divergence. Scope identities, tags, slot
states and histories are not observable. Given the host's choices, which
include the order of turns ([E-Turn]), execution is deterministic.

### 6.12 Tasks

An `async control` may suspend. Its executions are **tasks**, and tasks take
turns: one task runs at a time, and the turn passes only at an `await`.

A **task** has a stack of activations, a state (`ready`, `waiting`,
`done(a, v̄)`), the task set it is a member of (none for a root), and a
cancellation flag. A value of type `tasks⟨f⟩` is the identity of a **task
set**. The **owner** of a set is the activation whose scope holds it, in the
**owner task**. A **run** is a pool of tasks with one root. Runs nest as calls
do: the activation that started a run waits at its `run` decision.

- **[E-Run]** `d : Δ run f(ō) ▸ …`. Evaluate `ō`, check the precondition of `f`
  ([E-Call]) and start a run whose root task has one activation of `f`. The
  tasks of the run take turns until the root is `done(a, v̄)`, and the decision
  completes with `a` and `v̄`. If the root's last activation is popped in mode
  `fault`, the decision completes with no alternative and the fault continues
  in the caller ([E-Unwind]).
- **[E-Turn]** When no task of the innermost run has the turn, the host gives
  it to a task of that run that is `ready`. That task runs until it awaits,
  is done, or faults; nothing interrupts it in between. If no task of the run
  is `ready`, the host completes an operation that one of them waits for
  ([E-AwaitHost]). A run all of whose tasks wait for the host is not stuck.
  The tasks of the runs around the innermost one stand still until it ends.
- **[E-Await]** `d : Δ await f(ō) ▸ …`, with `f` a defined `async control`.
  Evaluate `ō` and call `f` by [E-Call], in the same task, so that its
  parameters are bound. The task then gives up its turn, before the first
  statement of `f`, and is `ready`. No value is between scopes while the task
  waits.
- **[E-AwaitHost]** `d : Δ await f(ō) ▸ …`, with `f` an `extern async control`.
  Evaluate `ō`. If the task is cancelled ([E-Cancel]) and `f` is cancellable,
  the decision completes with `cancelled`: the host is not asked, and the
  arguments given by value are released as [E-Unwind] releases them.
  Otherwise the host starts the operation and the task is `waiting`. The host
  later completes it as in [E-Extern]: the decision completes, its payload
  names are bound in the task's scope, and the task is `ready`. If the task
  becomes cancelled while it waits and `f` is cancellable, the host withdraws
  the operation and the task is `ready` with `cancelled`. An operation that
  is withdrawn, here or by [E-Stop], has no completion and no event, and the
  arguments it was given by value are released as [E-Unwind] releases them.
- **[E-Spawn]** `spawn f(ō) into x`. Evaluate `ō` and check the precondition of
  `f`. A new task with one activation of `f` on those arguments becomes a
  member of the set in `x`, in state `ready`. It does not run yet: `spawn`
  does not pass the turn.
- **[E-Next]** `d : Δ await next(x) ▸ a₁ {x̄₁} │ … │ empty {}`. The task gives up
  its turn. It is `ready` when the set in `x` has no members, or has a member
  that is `done`. With no members the decision completes with `empty`, and the
  slot of `x` becomes `◌`. Otherwise it completes with the alternative and
  the payload of the member that was done first, and that member is removed.
- **[E-Cancel]** `cancel x` sets the flag of every member of the set in `x`. A
  task is **cancelled** when its flag is set or the owner task of its set is
  cancelled. Nothing else happens at the `cancel`. A cancelled task learns of
  it only as the alternative `cancelled` of an operation it awaits
  ([E-AwaitHost]), and it ends by its own exits. The host may set the flag of
  a root, which is how a launch policy turns a signal into a request. The root
  of a run that a cancelled task starts, or is waiting for, is cancelled too,
  so a request reaches the operations that synchronous code awaits through
  `run`.
- **[E-Done]** `■ a {ō}` in the last activation of a task is [E-Exit], and the
  task is `done(a, v̄)`. A member that is done keeps its payload until
  [E-Next] delivers it.
- **[E-MemberFault]** When the last activation of a member is popped in mode
  `fault(message)`, the member is removed, and no task takes a turn before the
  fault is delivered. Let `A` be the owner of its set. The owner task waits at
  an `await`, in `A` or in an activation that `A` awaits. Every activation of
  that task above `A` is stopped ([E-Stop]). The await at which `A` waits
  completes with no alternative, and the owner task continues in mode
  `fault(message)`, as when a callee faults ([E-Unwind]).
- **[E-Stop]** Stopping an activation ends its scopes, innermost first, as
  [E-Unwind] ends them: a resource declared `on fault release` is released,
  any other open resource makes the configuration stuck, and an operation the
  host has started is withdrawn ([E-AwaitHost]). The values in the care of a
  boundary of that activation are released in the same way. No contingency
  runs and no fault permission is asked: a stopped activation did not fault.
  Stopping a task stops its activations, last first. Whenever a scope that
  holds a task set ends by [E-Unwind] or [E-Stop], every member of the set is
  stopped, and the payload of a member that is already done is released in
  the same way. Ending scopes does not pass the turn, so no member runs
  between the fault or the stop and its own stopping, whichever scope is
  ended first.

A task is stopped only while it is `ready`, `waiting` at an `await` or `done`.
It is never stopped while one of its activations waits at a `run`: the run it
started is then the innermost one, and the task that could fault and unwind
its set stands still ([E-Turn]). The awaits of async controls are therefore
the only places where [TK4] has something to require.

For [E-Effect], the stack of a task continues below its first activation into
the activation that spawned or ran it.

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
[K3]  Lᵢ₊₁  = Lᵢ                           cᵢ binds, captures, assigns, obliges, tests,
                                            spawns, cancels or supervises
            = Lᵢ ∧ (gᵢ → d.a₁ ∨ … ∨ d.aₙ)   cᵢ is a decision, loop or boundary at site d
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
  `R_d`, the condition on which the loop statement at site `d` runs. That is a
  formula over the atoms of the enclosing bodies, which do not change while
  the loop runs ([H4]). Every `Rᵢ`, every fact and every atom of the body
  therefore implies that the loop ran.
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
length), the denotation of every expression and the axioms are fixed in the
[fact logic](facts.md).

An **interpretation** is a truth assignment `h` to the atoms and a valuation `ν`
of the logical names. It is **admissible** for `Θ` when `h` satisfies `Ω` and,
for every `G ⇒ φ` in `Ψ`, `h ⊨ G` implies `ν ⊨ φ`.

- **[N1]** `Θ ⊨ F`, for a history formula `F`, when `h ⊨ F` in every admissible
  interpretation.
- **[N2]** `Θ; G ⊨ φ` when `ν ⊨ φ` in every admissible interpretation with
  `h ⊨ G`.
- **[N3]** The checker decides these exactly: it answers yes when the
  entailment holds and only then, by any procedure it likes. Every yes carries
  a certificate that an independent kernel validates. Every no rejects the
  program and carries a counterexample interpretation, with a remark where the
  interpretation depends on an operator the logic does not interpret. The
  answer does not depend on time or on the machine. A compiler may bound its
  work by a count of steps, and reports exceeding the bound as its own limit,
  never as an answer. The fact logic fixes the entailment ([J4] to [J6]), the
  certificates and the counterexamples.
- **[N4]** `Ψ` receives:

  | From | Fact |
  | --- | --- |
  | A bind `x := e` at statement `i` | `Rᵢ ⇒ x = e` |
  | An assignment `x ← e` at statement `i` | `Rᵢ ⇒ x′ = e`, for the fresh name `x′` of [N5] |
  | A write to a place `x.π`, or an exclusive loan of it, at statement `i` | `Rᵢ ⇒ x′ =` the old `x` with that place replaced: a functional update, so every other part keeps its facts (fact logic, [I9]) |
  | A test `d : ? e` | `d.true ⇒ e` and `d.false ⇒ ¬e` |
  | A decision at site `d` on an operation with `a … ⇐ φₐ` | `d.a ⇒ φₐ`, over the arguments and the payload names |
  | A decision on a captured outcome | As for the captured operation, over the logical names its arguments had at the capture |
  | A decision at site `d` on a variant `x` | `d.c ⇒ x = V.c {ȳ}`, for the payload names `ȳ` of constructor `c` |
  | A decision at site `d` on a pure operation | `d.a ⇒` the alternative and the payload are functions of the arguments (fact logic, [I10]) |
  | An obligation proved at statement `i` | `Rᵢ ⇒ φ` |
  | The precondition of the control being checked | `⊤ ⇒ φ` |
  | The invariant of the loop whose body is being checked | `R_d ⇒ φ`, for the loop statement at site `d` |
  | The type of a logical name | Its range, and `0 ≤ len(x) ≤ usize.max`. The largest `usize` is a constant that the fact logic bounds and does not fix ([A2]). |

- **[N5]** A value that changes gets a new logical name, on the histories
  concerned. A borrow and the place it borrows denote the same value, so a
  change through either renames both. An assignment to a name or through a
  borrow, and an operation that receives `&~p` or an exclusive borrow, give a
  fresh name to the root of every place written or lent, and to the root of
  every place on which the borrow used holds a loan. `spawn … into x` and
  `await next(x)` give a fresh name to the set `x` ([TK6]), and `spawn` is an
  operation in this sense for the borrows among its operands. Entry to a loop gives
  fresh names to the loop's parameters and to every outer name that its body,
  nested loops included, can change in these ways; the parts of a name that
  the body cannot change keep their facts (fact logic, [I9]). Facts stated earlier keep referring to the earlier
  name. When a name has several binders, its logical name on a history is that
  of the binder that ran.

Every check below is an entailment, so a history that cannot happen never
causes an error, and every reported error comes with a feasible history.

### 7.3 Typing

- **[C1]** Expressions, operands, payloads and assignments are typed in the
  ordinary way. An integer literal has the integer type that its place
  expects, and `u32` where nothing expects one. It must be a value of that
  type, and for `usize` a value on every target (fact logic, [A2]). The
  negation of a literal is a literal. Both operands of an arithmetic
  operator or of a comparison have one type; a formula is typed in the same
  way, except that a literal in it may be any integer ([E-Formula]). An operand is given by value, as `&p` or as `&~p`, as the type
  expected there says ([W8]). An assignment, an exclusive borrow `&~p` and an
  operation that changes a collection's shape each need a **mutable place**:
  one whose root was bound with `~`, or that passes through an exclusive
  borrow.
- **[C2] Acknowledgement.** A decision lists exactly the alternatives of its
  operation's contract, each once, with one name for each payload field. An
  alternative that is missing, unknown or repeated is an error. The surface
  rules for outcome groups and `forward` expand to this.
- **[C3]** `■ a {ō}` names an alternative of the control's contract. A loop
  statement lists exactly the alternatives that the `□` statements of its body
  use, each once, and all `□ a` of one loop agree on the payload types of `a`.
- **[C4]** `x := capture ω(ō)` gives `x` the type `outcome⟨K⟩` for the contract
  `K` of `ω`.

### 7.4 Ownership

For each name `x` the checker keeps two history formulas: `D(x)`, on which `x`
has been bound, and `M(x)`, on which it has been moved. `x` is **available** on
`D(x) ∧ ¬M(x)`.

- **[O0] Order.** The rules apply to the operands of a statement one at a
  time, in the order of evaluation ([E-Operands]): those that do not lend,
  left to right, then those that lend, and then its command. So the second `x`
  in `Δ f(x, x)` is a use after a move. A terminator's own checks, and the availability `A₀`
  of [O6], are taken after the statement's expressions.

- **[O1] Bind.** Binding `x` at statement `i`, by a bind or as a payload name of
  alternative `a`, requires `Θ ⊨ ¬(Rᵢ ∧ D(x))`. Afterwards `D(x)` includes
  `Rᵢ`, or `d.a` for a payload name.
- **[O2] Use.** Reading, moving or borrowing `x` at statement `i` requires
  `Θ ⊨ Rᵢ → D(x) ∧ ¬M(x)`. A formula reads the names it mentions and moves
  none. A parameter in a contract condition is not a use: it denotes the value
  at entry ([P4]).
- **[O3] Move.** A consumed expression ([E-Var]) that is a name whose type is
  not `copy` moves it: `M(x)` afterwards includes `Rᵢ`. Only a whole name owned
  by the current body or an enclosing one moves. A consumed expression that is
  a component, an element or a place behind a borrow must have a type of kind
  `copy`: there are no partial moves, and nothing moves out through a borrow.
  `clone`, a borrow, or taking the owner apart are the alternatives. A read
  moves nothing, whatever the kind.
- **[O3a] Temporaries.** A value that is read and not consumed, and that no
  name holds ([E-Temp]), must not have kind `must`.
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
holds the borrow, and what is behind it belongs to someone else. A field of a
view that has borrow type counts as a borrow name here. In a place,
every index of one collection overlaps every other.

- **[L1] Creation.** `&p` or `&~p` at statement `i` creates a loan on `p` with
  `G = Rᵢ`. A borrow bound to a name is held by that name. A borrow passed as an
  argument is held by the operation, and by the payload names that the contract
  derives from that parameter with `∈`. A decision on a variant through a
  borrow derives every payload name from that borrow ([E-Outcome]). A borrow
  written as an operand of `■`, `□` or `↑` is held by the payload field or the
  loop parameter it is given to, which the terminator uses ([L2]). Whenever a
  borrow flows into a name, by a bind, a copy, a loop parameter or a derived
  payload, that name becomes a holder of every loan its source holds. A borrow made through a borrow `r` is a loan
  on `*r`, and its holder also becomes a holder of every loan that `r` holds.
- **[L2] Liveness.** A statement evaluates its operands in the order of
  [E-Operands] and then runs its command, which uses every borrow among their
  values: an
  operation its arguments, a loop statement its initial arguments, `↑` its
  arguments, `□` and `■` their payloads, a bind its value, an assignment the
  borrows through which its target is reached. A holder is **used
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
  changing a collection's shape are writes. Reading or writing a place `r.π`
  through a borrow name `r` is a read or a write of `*r.π`. Passing, moving or
  copying `r` itself is an access to all of `*r`: a write when `r` is
  exclusive, a read when it is shared. Ending a scope is a
  write to every name of that scope that owns its value. An access **goes
  through** a loan when it is made through one of the loan's holders. For an
  access at statement `i` to place `p′`, and every loan `⟨p, m, G, H⟩` with `p`
  overlapping `p′` that the access does not go through, if the access is a
  write or `m` is exclusive: `Θ ⊨ ¬(Rᵢ ∧ L)`, where `L` is the condition on
  which the loan is live after the access.
- **[L4] Escape.** A borrow or view in a payload field of `■ a {ō}` holds loans
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
- **[P4]** `■ a {ō}`: the condition `φₐ` of the contract, with the payload `ō`
  and with each parameter as [W7] reads it: its value at entry, or for an `&~`
  parameter the value behind it at the exit, with `old` for the value at
  entry.
- **[P5]** A partial primitive: the result of integer arithmetic lies in its
  type; a divisor is not zero; an index is less than the length, wherever an
  indexed place is read, borrowed or assigned; a conversion fits its target
  type, unless every value of the source type is a value of the target type.
- **[P6]** `⊥` at statement `i`: `Θ ⊨ ¬Rᵢ`. No feasible history runs it.
- **[P7]** A proved obligation is a fact afterwards, by [N4]. The obligations of
  one statement are proved in the order of evaluation, each a fact for the next.
  The obligations of the right operand of `∧` are proved where the left
  operand is true, and those of the right operand of `∨` where it is false
  ([E-Bool]).
- **[P8]** A function body is checked as one expression under the function's
  precondition, with [P5] for its primitives and with [O3] and [O3a]; by
  [W5a] it owns nothing. A call of a function in an expression is checked by
  [P3]. A formula carries no obligations, so a call in a formula is not
  ([E-Formula]).

### 7.7 Faults and effects

- **[F1]** A statement **may fault** when it is `fault e` or `escalate`, or a
  decision or capture on an operation whose signature says `faults`, or a
  `supervise` of a named boundary from whose contingency a fault can escape
  ([F6]). It requires `faults` in the signature of the control being checked,
  or an enclosing boundary.
- **[F2]** At a statement `i` that may fault, for every name `x` in scope whose
  type is not fault-releasable, after the statement's own arguments have moved:
  `Θ ⊨ ¬(Rᵢ ∧ D(x) ∧ ¬M(x))`. A value lent to a callee is still owned by the
  caller, so this covers faults raised below.
- **[F3]** The effects of the operations in a body are within the `uses` of its
  control. A `pub` control declares them; a private control's are inferred as
  the union. Functions have none.
- **[F4]** The unproven form `assume ⊥ "reason"` is `fault "reason"`. It needs
  [F1] and [F2] and is listed in the trust report.
- **[F5] Lexical boundaries.** The rules of v1 for boundaries carry over
  ([reference](../reference/contingency.md)). These are the ones that the
  other clauses of this section rely on. For
  `d : boundary {B₁} contingency [x̄] {B₂}`, each name of `x̄` owns its value,
  is available at the statement, and is moved by it ([O3]). Where `B₁` can
  fall off its end, that end is checked as a terminator that ends the scope
  of `B₁` ([O5]). `B₂` calls only operations declared `contingent`, built-in
  operations and `emit`, and supervises nothing. Each of its histories ends
  with one of the terminators that [E-Contingency] lists; [O5] applies there
  to `x̄` as to any other name. `escalate` and a
  `fault` in `B₂` may fault in the sense of [F1], and their own boundary does
  not count as enclosing them.
- **[F6] Named boundaries.** For
  `boundary b(x̄ : τ̄) retains ȳ {B₁} contingency(m : string) {B₂}`, no `τ` is or
  contains a borrow or a view, and `ȳ` are distinct names among `x̄`. `B₁` is
  checked as the body of a control that permits `faults` and has no exit:
  each of its histories ends with `terminate`, a terminator to which [O5]
  applies. At every statement `i` of `B₁` that may fault, after the
  statement's own arguments have moved, every retained name is wholly
  available, `Θ ⊨ Rᵢ → (D(y) ∧ ¬M(y))`, and [F2] holds for every other name.
  A retained name may be consumed where no later statement of the history
  may fault. `B₂` is checked as a body in which `ȳ` and `m` are bound, as
  [F5] says of a contingency, and each of its histories ends with
  `terminate`, `escalate` or `fault`. `supervise b(ō)` is typed as a call
  with no alternative, and its operands move. It may fault exactly when a
  history of `B₂` ends with `escalate` or `fault`. The effects of both bodies
  count toward the control that supervises ([F3]). A boundary is invoked only
  by `supervise`, and not from a contingency.

### 7.8 Tasks

A task is checked as any activation of its control is. These rules are about
the set that owns it.

- **[TK1] Typing.** `spawn f(ō) into x`, `await next(x)`, `cancel x` and
  `count(x)` need `x : tasks⟨f⟩`. The operands of `spawn f`, `await f` and
  `run f` are typed as for a call of `f`, and the precondition of `f` is an
  obligation there ([P3]). `await next(x)` has the alternatives of the
  contract of `f`, with their payloads, and `empty`; [C2] applies to it.
  `count(x)` has the type `usize`.
- **[TK2] Ownership.** `tasks⟨f⟩` has kind `must`. The decision
  `d : Δ await next(x)` moves `x` on `d.empty`: afterwards `M(x)` includes
  `d.empty`. Nothing else consumes a set. By [O5], every history therefore
  drains every set before the scope that holds it ends. `spawn`, `await next`,
  `cancel` and `count` require `x` to be available ([O2]).
- **[TK3] Loans.** A borrow among the operands of `spawn f(ō) into x` is held by
  `x`. A payload name of `await next(x)` of borrow or view type becomes a
  holder of every loan that `x` holds. For [L3], `spawn … into x` and
  `await next(x)` are writes to `x`, and `cancel x` and `count(x)` are reads.
  A set uses the loans it holds for as long as it is available, whether or
  not a later statement names it: for [L2], the holder `x` is used after an
  access at statement `i` on every history on which `x` is available after
  that statement. A member may run at any `await` until the set is drained,
  and a history that faults or never ends has no later `await next(x)` to
  stand for that. The end of a scope that owns a lender is a write to it
  ([L3]), so a lender cannot go while the set holds the loan. There is one
  exception: the scopes that a `fault` ends do not conflict with the loans of
  the sets they hold, because the members of those sets are stopped without
  running again ([E-Stop]).
- **[TK4] Stops.** A task can be stopped wherever it waits at an `await`
  ([E-Stop]), so everything it holds there is fault-releasable. At every
  `await` of an `async control`, at statement `i`, the condition of [F2]
  holds: no name in scope whose type is not fault-releasable is available. A
  name in the care of a lexical boundary that encloses the `await` has a
  fault-releasable type. The operands that `await f(ō)` and `spawn f(ō)` pass
  by value have fault-releasable types: `f` holds them while it waits for
  its first turn, or the host does ([E-Await], [E-AwaitHost]). The payloads
  of a producer and of an `extern async control` have fault-releasable types:
  a payload can reach a task that is stopped before it runs again.
- **[TK5] Faults from members.** An `await` at statement `i` may fault, in the
  sense of [F1], unless `Θ ⊨ ¬(Rᵢ ∧ D(x) ∧ ¬M(x))` for every name `x` in scope
  whose type is `tasks⟨f⟩` with a producer that permits `faults`. No task set
  is available at a lexical `boundary` statement: the same entailment holds
  there for every name of a task set type. A set bound inside the body of a
  boundary belongs to that body. `supervise` is not such a statement: the
  bodies of a named boundary do not await ([W10]), so the fault of a member
  is never delivered inside them.
- **[TK6] Facts.** `count` is a measure (fact logic, [A3]).

  | From | Fact |
  | --- | --- |
  | `x := tasks⟨f⟩()` at statement `i` | `Rᵢ ⇒ count(x) = 0` |
  | `spawn f(ō) into x` at statement `i` | `Rᵢ ⇒ count(x′) = count(x) + 1`, for the fresh name `x′` |
  | `d : Δ await next(x)`, an alternative `a` of `f` | `d.a ⇒ count(x′) = count(x) - 1`, and the condition `φₐ` of `f` with a fresh name for each parameter |
  | `d : Δ await next(x)`, `empty` | `d.empty ⇒ count(x) = 0` |

  `await f(ō)` and `run f(ō)` give the facts of a call of `f` ([N4]). No
  operation that is awaited or run is pure, and neither is a control whose
  body contains `run`, directly or in a control it calls: which member is
  done first is the host's choice ([E-Turn]; fact logic, [I10]).
- **[TK7] Effects.** `spawn f`, `await f` and `run f` count the effects of `f`
  toward the control that contains them ([F3]).

Three consequences. A child may borrow from its parent, because the scope
that owns the lender cannot end while the set holds the loan. A place lent
exclusively to a child cannot be read by the parent until the set is drained,
so nothing the parent knows about its own names changes at an `await`. And a
fault that leaves a member reaches the boundary around its set, never an
alternative of `await next`.

## 8. Claims

A program is **checked** when every declaration satisfies sections 3, 4 and 7. A
host is **faithful** when it never yields `host-violation`. An execution starts
with one activation of an entry control, on arguments of its parameter types
that satisfy its precondition.

**Theorem 1 (safety).** No execution of a checked program under a faithful host
reaches a stuck configuration, whatever order of turns the host chooses.

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

The proofs rest on five invariants of reachable configurations, each relating a
scope at statement `i` to the checker's state there:

- **Adequacy.** The histories of the activation's scopes, the hidden atoms of
  each loop that has left as its leaving iteration had them, every other atom
  false, and the current values form an admissible interpretation that
  satisfies `Lᵢ`.
- **Ownership.** A name has an available slot exactly when the history
  satisfies `D(x) ∧ ¬M(x)`, a moved slot exactly when it satisfies
  `D(x) ∧ M(x)`, and no slot otherwise.
- **Loans.** Every borrow in an available slot whose holder is used later on
  this history has a live tag, and so has every borrow that a member of a set
  holds while the set is not drained.
- **Linearity.** Every open resource is in exactly one available slot, or is an
  argument or payload in transit, or is an argument of an operation that the
  host has started, or is in the care of a boundary, or is in the payload of
  a member that is done.
- **Structure.** Every task that is not a root is a member of a set held by an
  available slot of a scope of its owner task, and the owner task is at an
  `await` whenever a member has the turn.

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
- **[R6]** Tasks, sets, turns and cancellation flags are behavior and remain.
  Each task has its own histories.

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
| 4. Tasks | `async`, `spawn … into`, `await`, `cancel`, `run`, task sets | 6.12, TK |

Draft 2 gives tasks their rules. They meet the requirements that draft 1
carried over from v1's accepted decisions:

- A task set is a local of kind `must`. Its scope cannot end while it has
  members, except by a fault or a stop, which stops them; so children finish
  before the scope that owns them ([TK2], [E-Stop]). This is the one rule
  that lets a child borrow from its parent ([TK3]).
- Tasks interleave only at `await`. The order of turns is the host's choice;
  given it, execution is deterministic ([E-Turn]).
- Cancellation is a request. It arrives as the `cancelled` alternative of an
  awaited operation and never abandons a frame ([E-Cancel], [E-AwaitHost]).
- A child's fault reaches the boundary that owns the set before any other task
  takes a turn ([E-MemberFault], [TK5]).
- The borrow rules extend to tasks without a new mechanism: what a child
  captures is a loan held by the set ([TK3]).

Still outside this draft: the rules of trait declarations and coherence (the
core's capabilities; their surface form is [G11] of the surface document, and
their rules are v1's), modules and visibility, callable parameters and
closures, numeric conversions, text views, process entry and launch
declarations. Recursion and `box⟨τ⟩` belong to phase 5; the
[corpus](corpus.md) states what two of its programs assume of them.

## 11. Choices made in this draft

These follow from the accepted decisions but were not spelled out in the plan.
The author accepted them with phase 1 on 20 November 2026.

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
11. **Passing or moving an exclusive borrow is a write to all that is behind
    it** ([L3]); reading or writing through it touches only the place reached.
    The semantics is more permissive, since moving a borrow touches no tag; the
    checker's rule is what keeps two payload borrows from aliasing.
12. **Functions may state preconditions**, and their bodies are checked under
    them ([P8]).
13. **Reading does not move** ([E-Var], [O3]). Operators, functions, `len`,
    `clone` and formulas read their operands where they are. A value moves
    only where it is kept: a bind, an assignment, an operand given by value, a
    payload, a constructor field, a loop parameter.
14. **Functions own nothing** ([W5a]). Their parameters are `copy` values or
    shared borrows read in place.
15. **A borrow name is never re-pointed** ([W8]). `r ← e` always writes
    through `r`. A borrow that changes from one iteration to the next is a
    loop parameter.
16. **A view holds borrows and `copy` values only** ([W6]), so no view carries
    an obligation.
17. **A term of a formula always has a value** ([E-Formula]). An index out of
    range or a zero divisor yields a value that depends only on the operands.
    Formulas carry no obligations of their own.
18. **Parameters are not mutable** ([W4]), so a parameter in a contract
    condition has one value, the one at entry.
19. **Reads before loans** ([E-Operands]). The operands of a command that lend
    are evaluated after the others. It is question Q17 of the
    [reference model](references.md), which the author decided this way.
20. **Task sets are the only form of concurrency** ([W10], [W11]). v1's single
    task handles, deferred tasks and `yield` are not carried over; a single
    child is a set with one member. This is question Q18 below.
21. **A task set is a plain local**, never moved, lent or passed ([W11]), and
    never in scope at a fault boundary ([TK5]).
22. **Every `await` passes the turn** ([E-Await], [E-Next]), also when its
    answer is already there. A program may not depend on who runs next.
23. **A loan given to a child is held by the set as a whole** ([TK3]) and ends
    when the set is drained, not when that child is joined.
24. **Only values that can be released are held across an `await`** ([TK4]),
    in every async control, because any of them can be stopped by a fault
    elsewhere under the same boundary. v1 required this of spawned controls.
25. **A set's loans last as long as the set, not as long as it is named**
    ([TK3]). The third review found that liveness by mention let a parent
    that faults or never ends read what a running child holds.
26. **An awaited control has its arguments before it waits** ([E-Await]), a
    completed host operation binds its payload at once ([E-AwaitHost]), and a
    withdrawn one releases what it was given. With [TK4] this leaves no
    resource between scopes when a task is stopped.
27. **A control that runs tasks is not pure** ([TK6]): which member finishes
    first is the host's choice, so two calls need not agree.
28. **Boundaries have their checking rules written down** ([F5], [F6]). They
    are v1's. The first corpus draft used a named boundary that the core
    described only in its semantics.
29. **`∧` and `∨` evaluate their right operand only where it decides the
    result** ([E-Bool], [P7]). The draft did not say. Writing the oracle
    asked the question, and the answer is the one every reader expects:
    `i < len(v) ∧ v[i] = x` is one test and needs no nesting. A formula is
    not evaluated in an order and reads both sides ([E-Formula]).
30. **A function's precondition is an obligation in expressions only**
    ([P8], [E-Formula]). In a formula a call is the function's body.
31. **An integer literal takes the type its place expects** ([C1]), `u32`
    where nothing expects one, as the surface says of `■ auto`.

The author answered the five questions of this draft on 19 November 2026 and
took the recommendation in each case:

| | Question | Decision |
| --- | --- | --- |
| Q1 | May a component of kind `drop` or `must` be moved out of a struct while the rest stays? | **No** in v2.0. The owner moves whole or is taken apart ([E-Unpack]). Ownership stays one formula per name. |
| Q2 | When a loop leaves with alternative `a`, may an outer name be moved on some ways of leaving and not on others? | **No** ([O6]). All `□ a` statements agree. The remedy is a second alternative, or passing the value as loop state. |
| Q3 | What is the type of lengths and indices? | **`usize`**, an unsigned integer of address width, distinct from the fixed-width types. `len` returns it and indices have it. The ordered-search reference program is updated. The name can still change at no cost. |
| Q4 | Is `¬` allowed in a Rails path label? | **Yes, sparingly.** The surface document says when. |
| Q5 | An arm disposes an alternative that the checker can prove impossible. | **A warning** in non-generic code, suggesting `⊥`. Silent in generic code, where the arm may be reachable in another instance. |

Draft 2 raised one question. The author answered it on 20 November 2026 and
took the recommendation.

| | Question | Decision |
| --- | --- | --- |
| Q18 | Does v2.0 keep v1's single task handles (`child := spawn f(…)`, `await child`), deferred tasks (`defer`) and `yield`? | **No.** Task sets only, as the plan's list of words has it (`spawn … into`, `await`, `next`, `cancel`, `run`). A handle is a set with one member, a deferred task is a call made later, and `yield` only made schedules observable. One form means one ownership rule and one loan rule for all concurrency. |

## 12. Changes after acceptance

The author accepted this document on 20 November 2026. What changed afterwards,
and what asked for it:

| Date | Change | Asked for by |
| --- | --- | --- |
| 20 November 2026 | [E-Bool] added, with its part of [P7]: `∧` and `∨` evaluate the right operand only where it decides the result. | The oracle of slice 1: the semantics did not say whether `b ≠ 0 ∧ a ÷ b > 1` is stuck for `b = 0`. |
| 20 November 2026 | [E-Formula] and [P8] say that a formula reads every name it mentions and that a function call in a formula is the function's body, without its precondition. | The oracle: [P8] said "wherever it occurs", and [S4] of the fact logic says a formula carries no obligations. |
| 20 November 2026 | [E-Assign] is stuck on a name that has no slot, and X2 names the case. | The oracle: [O4] already required `D(x)`, and the semantics had no stuck state for it. |
| 20 November 2026 | [C1] states how an integer literal is typed. | The oracle's reader, which has to give every literal a type before anything runs. |
