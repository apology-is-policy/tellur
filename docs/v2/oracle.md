# Villanelle v2 oracle

Status: **slice 1, first checkpoint**, 20 November 2026. Phase 2 of the
[redesign plan](../redesign-plan.md). The oracle runs the scalar core. The
program generator, the erased semantics, the differential harness, the
mutation tests and the reader of Rails are the next checkpoints of this slice
(section 8).

[Index of the v2 design](README.md) · [Core calculus](core.md) ·
[Fact logic](facts.md) · [Surface](surface.md)

## 1. What it is

The oracle is the instrumented semantics of the [core](core.md), section 6,
written as a program. It runs a core program and is stuck on every violation
the checker is meant to exclude. A stuck run names the row of the table of
stuck states (X1 to X10) and the clause that had no successor. The checker of
phase 3 and the compiled code of phase 4 are tested against it.

It lives in `v2/`, a crate of its own beside v1. The crate has no
dependencies and shares no code with the v1 compiler, so that the definition
can be read on its own. Nothing in it is built for speed.

| Module | Holds |
| --- | --- |
| `int` | Mathematical integers. A formula is evaluated over them ([E-Formula]), and a result fits its type when the mathematical result lies within the type's bounds ([E-Arith]). |
| `syntax`, `text` | The core text form (section 3): its tree, printer and reader |
| `typed`, `resolve` | The typed core, and what is checked on the way to it (section 4) |
| `oracle` | Section 6 of the core, one function for each rule |
| `host` | Hosts for extern controls (section 5) |
| `script` | The directives that state runs beside a program (section 6) |

Slice 1 is the scalar language: `bool`, the integer types, functions, tests,
decisions on defined and extern controls, loops, exits, obligations and `⊥`.
Every value has kind `copy`, so no slot is moved and no borrow tag exists yet.

## 2. Running it

```sh
cd v2
cargo test                                     # unit tests and the conformance suite
cargo run --bin vn2 -- test tests/slice1/*.core
cargo run --bin vn2 -- run tests/slice1/e-loop.core gcd 48 18
cargo run --bin vn2 -- print tests/slice1/words.core
```

`vn2 run FILE ENTRY VALUES...` prints the events of the run and how it ends:

```text
$ vn2 run tests/slice1/e-arith.core add 250 6
stuck X7 E-Arith at add:5: 250 + 6 = 256 is not a value of u8
```

`--arithmetic` gives the run a host that computes checked arithmetic,
`--usize 32` makes the greatest `usize` `2³² - 1`, and `--fuel N` bounds the
steps.

## 3. The core text form

The oracle reads the notation of the core document: a body is a list of
guarded statements, `selector ▷ command`. This is a form for tests and for
reports. Programs are written in Rails, and the reader of Rails, which
elaborates into this form, is a later checkpoint.

```text
Program    ::= Declaration*
Declaration ::= function name ( Param,* ) [⊢ Formula] : Type := Expr
              | [pub] control name ( Param,* ) Signature Statement*
              | extern control name ( Param,* ) Signature
Signature  ::= [⊢ Formula] ■ { Alt (│ Alt)* } [uses name,*]
Alt        ::= name [{ Param,* }] [⇐ Formula]
Param      ::= name : Type
Type       ::= bool | u8 | u16 | u32 | u64 | i8 | i16 | i32 | i64 | usize

Statement  ::= [number] Selector ▷ Command
Selector   ::= ⊤ | name . name | ¬ Selector | Selector ∧ Selector
             | Selector ∨ Selector | ( Selector )
Command    ::= [~] name [: Type] := Expr
             | name ← Expr
             | ⊢ Formula
             | name : ? Expr
             | name : Δ name ( Expr,* ) [▸ Arm (│ Arm)*]
             | name : ↻ name [( LoopParam,* )] { [⊢ Formula] Statement* }
                   [▸ Arm (│ Arm)*]
             | ↑ name ( Expr,* )
             | □ name [{ Expr,* }]
             | ■ name [{ Expr,* }]
             | ⊥
Arm        ::= name [{ name,* }]
LoopParam  ::= [~] name : Type := Expr

Expr       ::= number | true | false | name | name ( Expr,* ) | ( Expr )
             | ¬ Expr | - Expr | Expr op Expr
```

The operators and their order are those of the [surface](surface.md), section
4.3. A formula is an expression of type `bool`.

- The body of a control is the statements after its signature, up to the next
  declaration. The body of a loop is in braces, and a `⊢` line that opens it
  is the loop's invariant.
- A statement may begin with a number, as the documents print them. The
  number means nothing.
- A comment runs from `┄` or `//` to the end of the line.
- The text has no layout, with one exception: a `(` that is the first token
  on its line does not continue the expression before it. A statement may
  therefore begin with a selector in parentheses.
- `x<-1` is an assignment. A comparison with a negative number needs the
  space: `x < -1`.

Every glyph has a spelling in ASCII. They are those of the surface ([G8]),
with three more that only the core form needs:

| Glyph | Word | Glyph | Word | Glyph | Word |
| --- | --- | --- | --- | --- | --- |
| `⊤` | `top` | `▷` | `\|>` | `▸` | `>>` |
| `Δ` | `delta` | `■` | `exit`, `exits` | `□` | `leave` |
| `↻` | `loop` | `↑` | `again` | `?` | `if` |
| `⊢` | `where` | `⇐` | `when` | `⊥` | `impossible` |
| `←` | `<-` | `~` | `mutable` | `│` | `\|` |
| `≠ ≤ ≥` | `!= <= >=` | `∧ ∨ ¬` | `&& \|\| !` | `÷` | `/` |

`=` may be written `==`. `vn2 print` gives a program in the glyph form, and
reading the print gives the same program.

## 4. What is refused, and what is stuck

A program has to be resolved and typed before it can run. `resolve` does that
and refuses a program whose text is not well formed. Everything that depends
on which history runs is left to the oracle, which runs the program and is
stuck where it goes wrong. The checker of phase 3 starts from the same
`resolve` and adds the judgments that exclude the stuck states.

| Refused by `resolve` | Clause |
| --- | --- |
| A site name used twice in one control | [W1] |
| A selector that mentions a site which does not precede it in its body or around its loop | [W2] |
| `↑` outside its loop, `□` outside a loop | [W3] |
| A name bound in two nested bodies; binders of one name that differ in type or mutability | [W4] |
| A function that reaches itself | [W5] |
| A contract that lists an alternative twice | [W7] |
| An expression that is not typed: operands of two types, a literal outside its type, an unknown name, an assignment to a name that is not mutable | [C1] |
| An arm for an alternative the operation does not have, a repeated arm, a wrong number of payload names | [C2] |
| An exit or a `□` with an alternative that is not listed, a listed alternative that no `□` uses, `□` statements that differ in a payload type | [C3] |

| Stuck in the oracle | State | Clause |
| --- | --- | --- |
| A body reaches its end | X1 | [E-End] |
| A name is read where it has no slot | X2 | [E-Var] |
| A name is assigned where it has no slot | X2 | [E-Assign] |
| A name is bound twice in one run of its body | X3 | [E-Bind] |
| `⊥` runs | X6 | [E-Impossible] |
| A result leaves its type; a divisor is zero | X7 | [E-Arith] |
| A false precondition of a function, a defined control, an extern control | X7 | [E-Fn], [E-Call], [E-Extern] |
| A false obligation | X7 | [E-Oblige] |
| A false condition of the alternative at an exit | X7 | [E-Exit] |
| A false invariant at entry, at `↑` | X7 | [E-Loop], [E-Again] |
| An effect that an activation on the stack does not allow | X9 | [E-Effect] |
| An operation completes with an alternative the decision does not list | X10 | [E-Decide] |

A missing arm is left to the oracle on purpose: it is the gap that decision
D3 closes, and the checker's rejection of it ([C2]) is tested against a run
that is stuck.

## 5. Hosts

An extern control is a step of the host: given the arguments, an alternative
and a payload ([E-Extern]). The oracle checks each answer against the
contract ([E-Host]): the alternative is listed, the payload has the types of
its fields, the condition of the alternative is true, and a control without
an effect gives the same answer to the same arguments. An answer that fails
ends the run in `violation`. That is a fault of the trusted host and not a
stuck state. A step of a control with an effect is recorded as an event.

Two hosts exist. `Script` answers from a list, in order, and a run whose list
is exhausted or names another control ends in `host-failed`: a defect of the
test. `Arithmetic` computes the checked arithmetic of the prelude: a control
whose name begins with `checked_add`, `checked_sub`, `checked_mul`,
`checked_div` or `checked_rem`, with two alternatives of which the first
carries the result, and passes every other call on.

## 6. Directives

A line that begins with `//!` states a run beside the program, and what it
must yield.

```text
//! run add(250, 5) => done sum {255}
//! run add(250, 6) => stuck X7 E-Arith
//! run pump(2) with [read -> value {7}; read -> closed] => done total {7}
//!     trace [read(2) -> value {7}; read(2) -> closed {}]
//! run address(4294967295) usize 32 => stuck X7 E-Arith
//! reject W1
```

`run` names an entry control and its arguments. The options follow: `with
arithmetic`, `with [...]` for a scripted host, `usize 32`, `fuel N`. After
`=>` comes the expected end: `done` with the alternative and its payload,
`stuck` with the state and the clause, `violation`, `host-failed`, `fuel`, or
`not-an-execution` (the arguments are not values of the parameter types, or
the precondition of the entry is false; section 8 of the core does not count
such a run as an execution). `trace [...]` states the events. `reject` names
the clause by which `resolve` must refuse the program.

## 7. Conformance of slice 1

`v2/tests/slice1` holds 40 programs with 86 runs. The suite carries them out
and then checks that together they reach what the slice has: each of the
twelve rules a statement can apply, each of the fifteen ways of being stuck,
and each of the nine clauses of refusal.

| Clause | File |
| --- | --- |
| [E-Arith], [A2] | `e-arith.core` |
| [E-Bool] | `e-bool.core` |
| [E-Fn] | `e-fn.core` |
| [E-Formula] | `e-formula.core`, `e-fn.core` |
| [E-Oblige] | `e-oblige.core` |
| [E-Skip], [E-End] | `e-end.core` |
| [E-Var] | `e-var.core` |
| [E-Bind] | `e-bind.core` |
| [E-Assign] | `e-assign.core` |
| [E-Test], [H2], [H3] | `e-test.core` |
| [E-Decide], [E-Call], [E-Exit] | `e-decide.core` |
| [E-Impossible] | `e-impossible.core` |
| [E-Extern], [E-Host], [E-Effect] | `e-extern.core` |
| [E-Loop], [E-Again], [E-Leave], [H1], [H4] | `e-loop.core` |
| [C3], [W9] | `c3-leave-types.core` |
| [G8] | `words.core` |
| [W1] to [W5], [W7], [C1] to [C3] | `reject-*.core`, 24 files |

The clauses of checking (K, N, P) have no test here. They are judgments, and
their accepting and rejecting tests come with the checker. Each has its
stuck state above: coverage [K7] is X1, [P1] to [P5] are X7, and [P6] is X6.

## 8. What this checkpoint leaves open

- **The rest of the slice in phase 2.** A generator of random core programs
  with a count of the constructs it reaches, the erased semantics
  ([core](core.md), section 9) as a second interpreter, the differential
  harness between the two, and the mutations that the checker will have to
  reject.
- **The reader of Rails.** The corpus is written in Rails. The plan's exit
  for phase 2, that the oracle runs the whole paper corpus, needs the reader
  and the elaboration of the [surface](surface.md), and the later slices.
- **Operators that the surface does not have yet.** The wrapping and
  saturating operators and the numeric conversions are named in the core and
  the fact logic and have no spelling. They belong to phase 5.

Choices the oracle makes where the core leaves one:

1. **A quotient by zero in a formula is 0 and a remainder by zero is the
   dividend.** [E-Formula] asks only that the value depend on the operands
   alone.
2. **The greatest `usize` is a parameter of the run**, `2⁶⁴ - 1` unless the
   run says `2³² - 1`. The fact logic proves for every width between ([A2]),
   so a checked program must not be stuck under either.
3. **A literal of type `usize` must fit 32 bits**, since it has to be a value
   on every target.
4. **Effects are checked in slice 1** ([E-Effect]), because an extern control
   with an effect is what makes a trace. The checker's rule for them ([F3])
   belongs to slice 2.
5. **A run is bounded by fuel**, and a run that exhausts it ends in `fuel`.
   Running forever is not stuck ([core](core.md), section 6.10).

Writing the oracle asked four questions of the core, which section 12 of the
[core](core.md) records with the answers: the order of `∧` and `∨`, a
function call inside a formula, an assignment to a name without a slot, and
the type of an integer literal.
