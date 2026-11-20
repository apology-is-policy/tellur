# Villanelle v2 fact logic

Status: **draft 1**, 19 November 2026. Phase 1, deliverable 2 of the
[redesign plan](../redesign-plan.md). Accepted by the author on 20 November
2026 as the working specification ([index](README.md)). Nothing here is
implemented.

One independent review has been folded in. It confirmed the worked proofs and
most of the axioms. It also found that the first version of the theory was
inconsistent (a range was claimed for terms that need not have one), that two
axiom families were false outside the range of their type, that purity was
defined too widely, and that entailment was not fixed precisely enough for two
compilers to agree. The text below includes the repairs, which have not been
reviewed again. A later review of all five phase 1 documents found three
things here: the length of a literal ([A3]) was never an instance of a query
([J3]), a control that runs tasks counted as pure ([I10]), and a loop did not
rename a task set that its body changes ([I9]). They are repaired below.

[Index of the v2 design](README.md) · [Core calculus](core.md) ·
[Plan](../redesign-plan.md)

## 1. Purpose and scope

The [core calculus](core.md) states every check as an entailment: `Θ ⊨ F` for a
history formula ([N1]) and `Θ; G ⊨ φ` for a logical formula on the histories `G`
([N2]). It leaves open what a logical formula may say, what `⊨` means for
arithmetic and data, how it is decided, and what the compiler shows when it
fails. This document fixes those four things.

- **The fragment** (sections 3 to 5): the terms and formulas, how program text
  denotes them, and the theory `𝒯` that gives them meaning. It is linear integer
  arithmetic, equality, structs, variants, vector length and element writes,
  and uninterpreted symbols for everything else. There are no quantifiers.
- **Entailment** (section 6): the definition that completes [N1] and [N2], and
  the requirement that the compiler decide it exactly.
- **The procedure and its evidence** (sections 7 to 9): a reference decision
  procedure, the certificate that every proof carries, and the counterexample
  that every failed proof carries.
- **Worked examples** (sections 2 and 10) and the limits of the fragment
  (section 11).

The design follows three accepted decisions of the plan: D2 (logical facts in
linear integer arithmetic, `⊥` proved), D7 (a built-in decision procedure with
certificates, an external solver only in CI) and the sketch in its section 5.2.
Section 12 lists what this document needed the core to say; those changes have
been made there.

Clause identifiers here use the families **S** (sorts and terms), **I**
(interpretation of program text), **A** (axioms), **J** (the judgment), **V**
(certificates and their validation) and **M** (models and counterexamples).

## 2. The logic in three obligations

### 2.1 An alternative proved away

```text
control spend(total: u32, used: u32) ■ left {value: u32} │ over
│ ? used > total → ■ over
│ Δ checked_sub(total, used): difference {value} │ underflow → ⊥
│ ■ left {value}
╵
```

`checked_sub` is the contract from the Rails reference programs: `difference`
occurs `⇐ a ≥ b` and `underflow` occurs `⇐ a < b`. The core form:

```text
1  ⊤            ▷ t : ? used > total
2  t.true       ▷ ■ over {}
3  ⊤            ▷ s : Δ checked_sub(total, used) ▸ difference {value} │ underflow {}
4  s.underflow  ▷ ⊥
5  ⊤            ▷ ■ left {value}
```

Statement 4 needs `Θ ⊨ ¬R₄` ([P6]). The checker has `R₄ = ¬t.true ∧ s.underflow`
after simplification, the structure `t.true ∨ t.false` ([K5]), and the facts

```text
t.false      ⇒ ¬(used > total)
s.underflow  ⇒ total < used
```

Over the integers `used > total` and `total < used` are the same atom, `total + 1
≤ used`. One fact asserts its negation and the other asserts it. The obligation
holds by Boolean reasoning alone; no arithmetic is done.

Most obligations in ordinary code are of this kind: a test established a
comparison and a later statement needs the same comparison. Writing the
subtraction with the infix operator needs the same proof and no arm at all:

```text
│ ? used > total → ■ over
│ ■ left {total - used}
```

### 2.2 An index proved in range

Statement 5 of binary search (core, section 2) reads `source[middle]` and needs
`middle < len(source)` ([P5]). The facts on the histories that reach it:

```text
count ≥ 1                       the test at 1 was false, and count is unsigned
2·half ≤ count                  half = count ÷ 2
start + count ≤ len(source)     the invariant
middle = start + half           the bind at 4
```

Suppose the index were out of range: `middle ≥ len(source)`. Write each line as
a row `e ≤ 0`, or `e = 0`:

```text
B   1 - count                     ≤ 0
A   2·half - count                ≤ 0
I   start + count - len(source)   ≤ 0
E   middle - start - half         = 0
G   len(source) - middle          ≤ 0
```

Then `B + A + 2·I + 2·E + 2·G` is `1 ≤ 0`. The five numbers `(1, 1, 2, 2, 2)` are
the whole proof. The compiler finds them; a separate small program, the kernel,
checks them by adding the rows up (section 8).

### 2.3 A proof that fails

Delete the invariant line `⊢ start + count ≤ len(source)`. Row `I` is gone and
the remaining rows are satisfiable. The compiler reports the obligation with an
assignment that satisfies everything it knows:

```text
cannot prove  middle < len(source)        at source[middle], loop search
on the history  count = 0 is false
with  start = 0, count = 1, len(source) = 0
      half = 0, middle = 0
note  these are values at the start of an iteration; the loop states
      nothing that excludes them
```

Every failed proof has such an assignment (section 9). The loop never starts an
iteration in that state, but nothing written in the program says so, and the
checker reasons only from what is written.

## 3. Terms and formulas

### 3.1 Sorts

- **[S1]** `Int` is the set of mathematical integers. Every integer type of the
  language (`uN`, `iN`, `usize`) has the sort `Int`; the type contributes a
  range ([A1]) and nothing else.
- **[S2]** `Bool`; one sort for each struct type and each variant type; the
  sort `[σ]` of vectors over `σ`.
- **[S3]** Every other type is an **uninterpreted sort**: `string`, maps,
  resources, captured outcomes, task sets, and type parameters. The
  logic knows that a value of such a sort equals itself and nothing more, apart
  from the measures of [A3].

A borrow type has the sort of what it refers to ([I7]). A view has the sort of
a struct whose fields are what its borrows refer to.

### 3.2 Terms and formulas

```text
Term     t ::= n                        integer literal
             | x                        logical name
             | usize.max                the address-width constant ([A2])
             | t + t | t - t | n · t    linear arithmetic
             | t ÷ n | t mod n          quotient and remainder by a nonzero literal
             | len(t)                   measure
             | t.x                      field of a struct
             | S {t̄} | V.c {t̄}          constructors
             | [t̄] | ∅                  vector literal, empty map
             | t[t]                     element of a vector
             | t[t ↦ t]                 the vector with one element replaced
             | g(t̄)                     application of a symbol
Formula  φ ::= t ≤ t | t = t | t is c | b | q(t̄)
             | φ ∧ φ | φ ∨ φ | ¬φ | true | false
```

`<`, `≥`, `>` and `≠` abbreviate the obvious combinations; on integers `s < t` is
`s + 1 ≤ t`. `b` is a term of sort `Bool` and `q` a symbol with a `Bool`
result. `t is c` says that the variant value `t` was built by constructor `c`.
`t[i ↦ e]` and `usize.max` are notation of the logic; programs do not write
them. The remainder is written `mod` here, and the surface document fixes its
spelling.

- **[S4]** A formula in a program (`⊢ φ`, `⇐ φ`) is written as an ordinary
  Boolean expression, plus `old(y)` in a contract condition ([W7]). There is
  no separate specification language. Its arithmetic is that of the
  mathematical integers and carries no obligations ([E-Formula]). A test
  `? e` is an expression: its arithmetic is checked by [P5], and the fact it
  yields is the formula with the same text. Section 4 says which term each
  expression denotes; an expression the fragment does not interpret still
  denotes a term, an application of a symbol.
- **[S5]** A **symbol** `g` is a function the logic does not interpret. There is
  one for each operator outside the fragment at each type it is used at, one
  for the alternative and for each payload field of every pure operation
  ([I10]), and internal ones that section 5 introduces. A symbol is a function:
  equal arguments give equal results. That is the only thing known about it,
  apart from the axioms of section 5.
- **[S6]** An **atomic term** is one that linear arithmetic does not see into:
  any term that is not a literal, a sum, a difference or a multiple by a
  literal. Names, fields, elements, measures, applications of symbols, and
  quotients and remainders are atomic. A linear combination of atomic integer
  terms with a constant is a **linear term**, and the atomic terms are the
  variables of its rows. A **value term** is a logical name, or a field or an
  element of a value term: a part of a value that some name holds.

## 4. What program text denotes

The core gives every value a logical name and renames it when it changes
([N5]). A term never changes its value. The clauses below say which term a
piece of program text denotes at a statement.

- **[I1] Names.** A program name denotes its logical name on the current
  history. Where the name has different logical names on different histories
  at a statement (several binders, or an assignment on some histories), a
  formula that mentions it is taken once for each, on the histories where that
  logical name is current.
- **[I2] Arithmetic.** `+`, `-`, negation, and multiplication by a constant
  denote the mathematical operations on `Int`. The checked operators of the
  language denote the mathematical result; that the result fits its type is the
  proof obligation [P5], and is not assumed.
- **[I3] Constants.** A **constant expression** is an integer literal; or a
  name with a single binder that is not mutable, not a parameter of any kind
  and not a payload name, bound to a constant expression; or `+`, `-`, `*`,
  negation, `÷` and `mod` with a nonzero divisor, or a conversion between
  integer types, applied to constant expressions. Nothing else is constant.
  The order is fixed: functions are unfolded first ([I8]), constant
  expressions are then folded to literals, and only then are divisions and
  products classified ([I4], [I5]). Whether a product is linear is therefore
  decided by the program text and not by the search.
- **[I4] Division.** `e ÷ k` and `e mod k` with a nonzero constant `k` are the
  interpreted terms of [A7]. Signed division truncates toward zero and the
  remainder has the sign of the dividend. With a divisor that is not constant
  they are the symbols `div` and `rem`, with the bounds of [A8].
- **[I5] Products.** A product of two operands of which neither is constant is
  the symbol `mul`, with the bounds of [A9]. Its operands are ordered, so
  `a * b` and `b * a` are one term.
- **[I6] Other operators.** A numeric conversion denotes the same integer.
  That it fits is [P5], unless every value of the source type is a value of
  the target type. The wrapping and saturating operators,
  bitwise operators and shifts are symbols by operator and type, with the
  axioms of [A10] and [A11]. A comparison of integers is a linear formula.
  Equality at any other type is `=`. An ordering at another type is a symbol
  with a `Bool` result. `∧`, `∨` and `¬` are the connectives.
- **[I7] Places.** `p.x` is a field and `p[e]` an element. A place read through
  a borrow denotes the value behind the borrow. A local borrow name is an
  abbreviation of the place it borrows, read at the current logical name of
  that place's root, with any index as it was when the borrow was made. A
  name borrows one place for as long as it lives ([W8]). A borrow that arrives
  as a parameter, as a loop parameter or as a payload field is a logical name
  of its own for the value behind it. `clone(p)` denotes the same term as
  `p`.
- **[I8] Functions.** `fn(ē)` denotes the body of `fn` with the argument terms
  in place of its parameters. Functions are not recursive ([W5]), so this
  ends. A function is transparent to the logic.
- **[I9] Writes.** A change to a place is a functional update of its root. For
  a place `y.π`, where `π` is a path of fields and indices, define

  ```text
  upd(t, ε, e)       = e
  upd(t, .x π, e)    = S {t.x₁, …, upd(t.x, π, e), …, t.xₙ}
  upd(t, [i] π, e)   = t[i ↦ upd(t[i], π, e)]
  ```

  Each of the following events gives `y` a fresh logical name `y′` on the
  histories `G` concerned, with the fact `G ⇒ y′ = upd(y, π, z)`:

  | Event | `z` |
  | --- | --- |
  | An assignment `y.π ← e` | The term of `e` |
  | Lending `&~y.π` to an operation | A fresh name. The contract's conditions read the parameter as `z` and `old` of the parameter as `y.π`. |
  | A write or a loan through a local borrow name | As above, for the place the name abbreviates |
  | A write or a loan through a borrow that is a parameter, a loop parameter or a payload field | The value behind it is renamed in the same way. Every root on which that borrow holds a loan gets a fresh name with no fact. |
  | `spawn … into x`, `await next(x)` | The set `x` is renamed, with the fact about `count` that the core gives ([TK6]). |
  | Entry to a loop | Every name that some event of the body, nested loops included, renames by a row above is renamed at entry. A place the body changes directly gets a fresh `z`, one for each such place, with a path through an index cut at the collection. A name that the row above renames with no fact gets none here. |

  What a body can change is read off its text and its loans: the places it
  assigns or lends exclusively, the values behind the exclusive borrows it
  writes or lends through, and the roots on which those borrows hold loans.
  The fields of `y` outside the places changed directly keep their facts
  across the event, the loop included.
- **[I10] Pure operations.** An operation is **pure** when its set of effects
  is empty, whether declared or inferred ([F3]), it is not `async`, no `run`
  occurs in its body or in the body of a control it calls, and no
  resource type occurs in its parameters or payloads. It neither changes nor
  observes the world, and no order of turns can reach it ([E-Turn]), so its
  alternative and its payload are functions of the values it was given. A decision at site
  `d` on a pure operation `f` with argument terms `t̄` (borrows as the values
  behind them, at entry) adds, for each alternative `a` with payload names `ȳ`,

  ```text
  d.a ⇒ alt_f(t̄) = a ∧ ȳ = pay_f.a(t̄)
  ```

  where the alternatives of `f` are distinct constants. Deciding the same pure
  operation on equal arguments twice therefore gives the same alternative and
  the same payload. For an extern control this is part of what its signature
  asserts, and the trust report lists it ([T5]); a host that answers a pure
  extern control differently on equal arguments is not faithful ([E-Host]).
- **[I11] Variants.** A decision at `d` on the variant `x`, or on a borrow of
  it, adds `d.c ⇒ x = V.c {ȳ}` for each constructor `c` with payload names `ȳ`.
- **[I12] Captured outcomes.** A capture records the terms of its arguments
  with the captured value. A decision on that value adds the conditions of the
  captured operation over those terms. Where the capture is not visible (the
  value arrived as a parameter or a payload), the arguments are fresh names.
- **[I13] Formulas as values.** A Boolean expression that stands where a term
  is needed (an argument of a symbol or of an operation, a field, a payload)
  is named: a fresh name `b` of sort `Bool`, with the fact `b ↔ φ`.

## 5. The theory

`𝒯` is the set of axioms below. Each is true of the real values and operations
of the language, which is what makes the checker's reasoning sound. Free
variables range over all values of their sort, and for `Int` that is every
integer: a term may denote a sum that no type holds ([I2]). An axiom about an
operator of a type is therefore either true for all integers or guarded by its
operands lying in the range of that type. [A1] and [A9] differ in kind from
the rest. They speak about terms, and have one instance for each term they
apply to (section 6).

### 5.1 Integers and measures

- **[A1] Ranges.** A value term of integer type `τ` lies in the range of `τ`:
  `0 ≤ t ≤ 2ᴺ - 1` for `uN`, `-2ᴺ⁻¹ ≤ t ≤ 2ᴺ⁻¹ - 1` for `iN`, and
  `0 ≤ t ≤ usize.max` for `usize`. So does the result of a total operator of
  type `τ` (wrapping, saturating, bitwise, shift) whose operands lie in the
  range of `τ`. No other term has a range by this axiom: not the result of a
  checked operator, a product, a quotient or a remainder, and not a field of a
  constructor term or an element of an update term, which are whatever was put
  there.
- **[A2] Address width.** `2³² - 1 ≤ usize.max ≤ 2⁶⁴ - 1`. The constant is not
  fixed further, so a proof holds on every target whose addresses have between
  32 and 64 bits.
- **[A3] Measures.** `len` applies to vectors, strings and maps, and
  `0 ≤ len(t) ≤ usize.max`. `count` applies to task sets, with the same
  bounds; the core says how the task forms change it ([TK6]). A vector
  literal has the length it shows, `len([t₁, …, tₙ]) = n`, and the empty map
  has none, `len(∅) = 0`. A vector literal and `∅` in program text denote
  these terms, and the logic knows nothing else about a literal: its elements
  are whatever was put there, as for any constructor term. Different string
  literals are different values.
  What `len` counts in a string is for the text library to say, and the logic
  does not know the length of a string literal.

### 5.2 Structs, variants and vectors

| | Axiom | True because |
| --- | --- | --- |
| **[A4]** | `S {t̄}.xᵢ = tᵢ` and `t = S {t.x₁, …, t.xₙ}` | A struct value is the tuple of its fields |
| **[A5]** | `V.c {s̄} ≠ V.c′ {t̄}` for `c ≠ c′`; `V.c {s̄} = V.c {t̄}` implies `s̄ = t̄`; `t is c` holds for exactly one constructor `c` of the type of `t`, and then `t = V.c {ū}` for some `ū` | A variant value is one constructor applied to its fields |
| **[A6]** | `len(t[i ↦ e]) = len(t)`; `0 ≤ i < len(t)` implies `t[i ↦ e][i] = e`; `i ≠ j` implies `t[i ↦ e][j] = t[j]` | Replacing one element changes that element only. A write is made only at an index proved in range. |

Vectors have no other axioms. Two vectors with equal lengths and equal elements
are not known to be equal, and an operation that changes a vector's shape tells
the logic only what its contract states. The term `t[i]` with `i` out of range
denotes a value of the element type that depends only on `t` and `i`
([E-Formula]).

### 5.3 Operators the fragment interprets in part

- **[A7] Division by a constant.** For a literal `k ≥ 1`, with `q = t ÷ k`:

  ```text
  t ≥ 0  implies  k·q ≤ t ≤ k·q + (k - 1)
  t ≤ 0  implies  k·q - (k - 1) ≤ t ≤ k·q
  t mod k = t - k·q
  ```

  Division by `-k` is `-(t ÷ k)`, and `t mod (-k) = t mod k`.
- **[A8] Division by a term.** With `q = div(s, t)` and `r = rem(s, t)`:

  | When | Then |
  | --- | --- |
  | `t ≥ 1`, `s ≥ 0` | `0 ≤ q ≤ s`, `0 ≤ r ≤ t - 1`, `r ≤ s` |
  | `t ≥ 1`, `s ≤ 0` | `s ≤ q ≤ 0`, `-(t - 1) ≤ r ≤ 0`, `r ≥ s` |
  | `t ≤ -1`, `s ≥ 0` | `-s ≤ q ≤ 0`, `0 ≤ r ≤ -t - 1` |
  | `t ≤ -1`, `s ≤ 0` | `0 ≤ q ≤ -s`, `t + 1 ≤ r ≤ 0` |

- **[A9] Products.** Every integer term has a **declared interval**, each end
  of which is a literal or absent. It is computed from the term as section 4
  produces it, before the procedure rewrites anything. A literal is its own
  interval. A value term has the bounds of its type that are literals (so a
  `usize` has the lower bound 0 and no upper bound). Sums, differences,
  multiples by literals, and quotients and remainders by literals combine
  intervals in the usual way. A conversion has the interval of its operand. A
  product has the least and the greatest of the four products of its
  operands' bounds, when all four exist. Every other term (a quotient or
  remainder by a term, the result of a total operator, any other application)
  has no bounds. For `w = mul(s, t)`, with `s` in `[a, A]` and `t` in
  `[b, B]`, the instances are those of the four below whose two bounds
  exist:

  ```text
  w ≥ a·t + b·s - a·b        w ≤ A·t + b·s - A·b
  w ≥ A·t + B·s - A·B        w ≤ a·t + B·s - a·B
  ```

  Each is linear, since the bounds are literals. They follow from
  `(s - a)(t - b) ≥ 0` and its three companions. For unsigned operands they
  say that the product is not negative and is at most `A·t` and `B·s`.
- **[A10] Bit operations.** `t ≥ 0` implies `0 ≤ and(s, t) ≤ t`, and the same
  with the operands exchanged. A right shift of a non-negative value by a
  literal `k` is `÷ 2ᵏ`. Both hold for all integers. The other bit operations
  have only the range that [A1] gives them.
- **[A11] Wrapping and saturating addition and subtraction.** With `m` the
  number of values of the type (`usize.max + 1` for `usize`), and for operands
  that lie in the range of the type: the wrapping result is `s ± t + j·m` for
  one `j` among `-1`, `0`, `1`. The saturating result, for any operands, is
  `s ± t` if that lies in the range, the upper bound if it is above, and the
  lower bound if it is below. Wrapping and saturating multiplication have only
  the range that [A1] gives them.

The axioms of 5.3 are a list, and the list is part of the language version.
Adding an axiom makes more programs provable and breaks none. Each axiom is
also a property test of the phase 2 fuzzer, against the oracle's arithmetic.

## 6. Entailment

Entailment is defined by a base logic and a set of instances. Both are fixed
here, so that two compilers cannot disagree about a program.

- **[J1] Base logic.** Linear arithmetic over the integers, equality, and
  uninterpreted sorts and symbols, with the constructors, fields and tests of
  [A4] and [A5]. A structure gives `Int` and `Bool` their standard meaning,
  every other sort a non-empty set, and every constructor, field, measure and
  symbol a function.
- **[J2] Terms of a query.** A query at a statement is made from `Ω`, from
  every fact that the body and the bodies around it have added to `Ψ` up to
  that statement, and from the formula asked about. Its terms are all the
  terms in those, with their subterms. No fact is left out because it looks
  irrelevant.
- **[J3] Instances.** The instance set of a query is the least set that holds:
  the range of every term to which [A1] gives one; [A2]; [A3] for every
  measure term; the length of [A3] for every vector literal and every `∅`
  among the terms, whether or not the query applies `len` to it; the instance
  of [A7] to [A11] for every quotient, remainder,
  product and other operator application; the first two parts of [A6] for
  every update term `t[i ↦ e]`; and the frame part of [A6] for every update
  term together with every term `j` that the query uses as an index on a
  vector of the same sort. The terms of an instance count as terms of the
  query. The set is finite: an instance adds elements of vectors, and never
  an update term or an index.
- **[J4] Entailment.** [N1] and [N2] hold when no structure of the base logic,
  with a truth assignment `h` to the atoms and a valuation `ν` of the logical
  names, satisfies the formulas of the query together with its instances. An
  interpretation, as the core uses the word, is such a structure with `h` and
  `ν`.
- **[J5] Exactness.** The compiler answers yes when the entailment holds, and
  only then. Whether a program is accepted is therefore a property of the
  program and the language version. It does not depend on heuristics, on the
  order of a search, on which facts an implementation chose to look at, or on
  which release of the compiler is used. An implementation may leave facts or
  instances out of its work only where that cannot change the answer.
- **[J6] Determinism.** The answer, the certificate and the counterexample are
  functions of the program text. Nothing depends on time, on the machine, or
  on the order in which a hash table is traversed. A compiler may bound its
  work by a count of steps. Exceeding the bound is reported as a limit of the
  compiler, with the query that exceeded it; it is neither a yes nor a
  counterexample, and raising the bound never changes an answer already given.

Soundness needs only that every instance is true of real values, which
section 5 argues family by family. Exactness is achievable because the base
logic is decidable: there are no quantifiers, the instance set is finite, and
what remains is linear integer arithmetic with uninterpreted functions and
free constructors. The worst case is exponential. Section 7 says why the
common case is small.

The instances also decide the axioms as general statements, with one
exception that is deliberate. [A9] bounds a product by the declared intervals
of its operands as they are written, and by nothing else. So a product is
proved to fit by the types of what is multiplied, or by a fact the program
states about that product. An earlier statement of the same body that
multiplies the same values contributes its instance, by [J2]; that is part of
the definition, and the same in every compiler.

## 7. Reference procedure

A compiler may use any procedure that meets [J5] and [J6]. This is the reference
one. It is the arrangement of a lazy SMT solver, with the binary decision
diagram the checker already keeps for histories in place of a SAT solver.

A query is the question whether `Ω ∧ Ψ ∧ Q` has an admissible interpretation,
where `Q` is `¬F` for [N1] and `G ∧ ¬φ` for [N2]. The entailment holds exactly
when there is none.

1. **Translate and instantiate.** Program text becomes terms by section 4. The
   instances of [J3] are added.
2. **Canonical atoms.** A linear comparison becomes `Σ aᵢ·uᵢ ≤ b` over atomic
   terms `uᵢ` in a fixed order, with integer coefficients that have no common
   factor. A strict comparison is tightened: `e < b` is `e ≤ b - 1`. An
   integer equality is the pair of its two inequalities, so a disequality is a
   disjunction and becomes a case split of the Boolean structure. An atom and
   its negation share one Boolean variable. Other atoms stand as they are, with
   their arguments in canonical form.
3. **Boolean abstraction.** With each theory atom as a Boolean variable beside
   the history atoms, the query is one Boolean function. Build its diagram.
4. **Candidates.** If the diagram is false, answer yes. Otherwise take a path
   to true. It assigns some atoms; the theory literals among them are the
   candidate.
5. **Theory check.** Decide whether the candidate's literals can hold together:
   congruence closure for equalities and symbols; the simplex method over the
   rationals for the linear rows, one variable for each class of atomic integer
   terms; rounding and splitting for integrality; and a split on `s = t` for
   integer arguments of symbols that the arithmetic leaves undetermined.
6. **Learn or stop.** If the literals cannot hold together, the subset the
   refutation used is a **theory lemma**: not all of these at once. Conjoin it
   with the diagram and return to step 4. If they can, the candidate and the
   model are a counterexample: answer no.

The procedure ends: the atoms are finite, each lemma removes at least the
current candidate, and the integrality step is required to be a method that
decides linear integer arithmetic without assuming bounds, since a product or
a quotient by a term may have none. The Omega test is one. Branching within
the bound that the small-model property of integer programs gives is another.

Two properties keep the work small. Step 2 settles most obligations of ordinary
code in step 3, as in section 2.1: a test and the obligation it serves are the
same atom. And a theory lemma is true in every structure, so the lemmas learned
for one query of a body are kept for all the others; a body is checked against
one growing set.

## 8. Certificates

- **[V1]** Every yes carries a certificate. The compiler accepts an entailment
  only after its **kernel** has validated the certificate. The kernel shares no
  code with the search. A defect in the search can therefore cause a rejection
  or a compiler error, and cannot cause a wrong acceptance.
- **[V2]** A certificate holds the query, the axiom instances used, and the
  theory lemmas, each with a proof. The kernel checks that each instance is an
  instance, validates each proof, and then recomputes the Boolean step: the
  conjunction of the query, the instances and the lemmas is false. That step
  needs no evidence, because it is a function of formulas the kernel already
  has.
- **[V3]** The proof of a lemma refutes a set of literals. It is a tree of
  these steps, and every branch must close:

  | Step | Derives | The kernel checks |
  | --- | --- | --- |
  | Row | A row `e ≤ b` or `e = b` from a literal | That the literal has that canonical form |
  | Chain | An equality `s = t`: a sequence of links, each a literal, an instance, or congruence (`g(s̄) = g(t̄)` from `s̄ = t̄`). A literal `q(s̄)` is the link `q(s̄) = true`, and `¬q(s̄)` is `q(s̄) = false`. | Each link. An integer equality may be used as a row. |
  | Combine | A row, as a sum of earlier rows with integer multipliers, non-negative for inequalities | The sum |
  | Round | From `e ≤ b`, where the coefficients of `e` have the common factor `g ≥ 1`: `e/g ≤ ⌊b/g⌋` | That `g` is positive and divides the coefficients, and the floor |
  | Split | Two branches, with `e ≤ n` and with `e ≥ n + 1`, for a linear term `e` and an integer `n` | That `e` has integer coefficients; the two cases then cover every integer value |
  | Merge | The equality `s = t` from the rows `s - t ≤ 0` and `t - s ≤ 0` | The two rows |
  | Close | The branch, by a row `0 ≤ b` with `b < 0`, by an equality that contradicts a disequality literal, or by an equality between different constructors, different literals (`true` and `false` among them) or different alternatives of one operation | The contradiction |

  Combine with Close is Farkas' lemma, and suffices whenever the rows have no
  rational solution. Round is the Chvátal–Gomory cut; with Combine it is
  complete for rows that have rational solutions and no integer one. Split is
  admitted because it is often shorter.
- **[V4]** The kernel validates the proof of a query. It does not validate that
  the query is the right one for the program: the translation of section 4 and
  the checking rules of the core are outside it. Those are covered by the
  differential tests of phase 2 against the oracle and by the mechanization
  that decision D6 sets out.
- **[V5]** Certificates are written out on request, in a documented text
  format, so that a second checker can read them.

The kernel is integer arithmetic of unbounded size, a congruence check, a
matcher for the axiom list, and the diagram. It needs no search and no
rationals, since multipliers are integers.

## 9. Counterexamples

- **[M1]** Every no carries a model: a history, the values of the logical
  names, and the values of the symbol applications and measures it relies on.
  Before reporting, the compiler evaluates the query in the model. A no is
  checked evidence as a yes is.
- **[M2]** The report gives the obligation and where it arises; the history, as
  the alternatives taken at the sites reached, in program order; and the values
  of the program names the obligation depends on.
- **[M3] Assumed values.** A value the model chose for a symbol application is
  marked as assumed. The compiler then computes the real operation on the
  model's arguments where it has one (a product, a wrapping sum, a function of
  the standard library). If the real result differs, the report says that the
  failure depends on an operator the checker does not interpret, and names it.
- **[M4]** In a loop body the model is a state at the start of one iteration.
  The report says so, as in section 2.3.
- **[M5]** A failed obligation is reported and then assumed, so that each
  report stands for a different missing fact.
- **[M6]** The counterexample is deterministic ([J6]). The compiler should
  prefer a short history and small values; it need not find the smallest.

- **[M7]** A model may hold two vectors, strings or maps that it treats as
  different although they agree in length and in every element the program
  mentions, because the logic does not derive equality from contents. The
  report then says that the checker cannot conclude the two are equal, and
  names the terms. It does not print contents it does not have.

A counterexample is always a model of what the program states. It is not always
a state the program can reach, and there are four reasons: an operator the
fragment does not interpret ([M3]); a host contract that says less than the
host does; a loop or a control that states less than it maintains ([M4]); and
the equality of [M7]. The first three are repaired by writing the missing fact
down: as a precondition, a condition on an outcome, an invariant, or a
decision (section 11).

## 10. Worked examples

### 10.1 Binary search, in full

The program and its core form are in the [core calculus](core.md), section 2.
The names are `source`, `start`, `count`, `half`, `middle` and `usize.max`,
with `n` for `len(source)`. The rows available in the loop body, each with the
histories it holds on:

```text
I   start + count - n       ≤ 0      ⊤          the invariant
H   -half                   ≤ 0      ⊤          range of half
C   -count                  ≤ 0      ⊤          range of count
N   n - usize.max           ≤ 0      ⊤          range of len
A   2·half - count          ≤ 0      ¬z.true    half = count ÷ 2
A′  count - 2·half - 1      ≤ 0      ¬z.true    the same
E   middle - start - half   = 0      ¬z.true    the bind at 4
B   1 - count               ≤ 0      z.false    count ≠ 0, with C
```

`B` is not a fact as written. The test gives `count = 0` as the atoms
`count ≤ 0` and `¬(count ≤ -1)`; `z.false` asserts that one of them fails; the
range of `count` asserts the second. So `¬(count ≤ 0)` holds, which is `B`, by
the Boolean step. [K5] makes `¬z.true` and `z.false` the same histories.

Each obligation is refuted by adding its negation `G` and combining:

| Statement | Obligation | Negation `G` | Combination giving `1 ≤ 0` |
| --- | --- | --- | --- |
| 2 | `start ≤ n` | `n + 1 - start ≤ 0` | `I + C + G` |
| 3 | `0 ≤ count ÷ 2` | `half + 1 ≤ 0` | `A′ + C + 2·G` |
| 3 | `count ÷ 2 ≤ usize.max` | `usize.max + 1 - half ≤ 0` | `A + H + I′ + N + G`, with `I′` below |
| 4 | `start + half ≤ usize.max` | `usize.max + 1 - start - half ≤ 0` | `A + H + I + N + G` |
| 5 | `middle < n` | `n - middle ≤ 0` | `B + A + 2·I + 2·E + 2·G` |
| 7 | `start + half ≤ n` | `n + 1 - start - half ≤ 0` | `A + H + I + G` |
| 8 | `middle + 1 ≤ usize.max` | `usize.max - middle ≤ 0` | `P + N + G`, with `P` below |
| 8 | `0 ≤ count - half` | `count - half + 1 ≤ 0` | `A + H + G` |
| 8 | `0 ≤ count - half - 1` | `count - half ≤ 0` | `B + A + 2·G` |
| 8 | `(middle + 1) + (count - half - 1) ≤ n` | `n + 1 - middle - count + half ≤ 0` | `I + E + G` |

`I′` is `count - n ≤ 0`, which is `I` with the range of `start`. `P` is
`middle + 1 - n ≤ 0`, the obligation proved at statement 5 and a fact from
then on ([P7]). The divisor at statement 3 is the literal 2. At statement 3
the quotient is a term and has no range of its own ([A1]): `A` and `A′` are
its rows, with the quotient in place of `half`, and the two obligations are
proved in order. The first yields `H` for the quotient and the second uses
it. Once `half` is bound, `H` is also its range as a name.

All of these are refuted over the rationals. Tightening strict comparisons in
step 2 of the procedure is the only use of integrality.

### 10.2 An empty set that cannot be empty

From the service in the Rails reference programs: when 64 connections are
active, wait for one to finish.

```text
extern async control next⟨t⟩(set : &~tasks⟨t⟩)
    ■ { handled {} ⇐ count(set) = count(old(set)) - 1
      │ empty {}   ⇐ count(old(set)) = 0 }

1  ⊤          ▷ k : ? count(active) ≥ 64
2  k.true     ▷ n : Δ await next(active) ▸ handled {} │ empty {}
3  n.empty    ▷ ⊥
4  n.handled  ▷ ↑ admit(remaining)
```

`count` is the measure of a task set. The core gives `await next` these facts
directly ([TK6]). The contract above is only a way to read them: it is not
program text, since a set is never lent and a producer is never a type
parameter ([W10], [W11]). An awaited
operation is never pure ([I10]). The decision renames the set: `old(set)` is
`active` and `set` is the fresh `active′`. The facts are

```text
k.true     ⇒ count(active) ≥ 64
n.empty    ⇒ count(active) = 0
n.handled  ⇒ count(active′) = count(active) - 1
```

and the structure has `n.empty → k.true` ([K4]). Statement 3 needs `¬R₃`
([P6]). The only candidate asserts `count(active) ≤ 0` and
`¬(count(active) ≤ 63)`. The rows `c ≤ 0` and `64 - c ≤ 0` sum to `64 ≤ 0`. The
lemma is "not both", its proof is the pair `(1, 1)`, and with the lemma the
diagram is false.

### 10.3 A field changes and the others keep their facts

```text
struct counter {served: u32, limit: u32}

control note(state: &~counter) ⊢ state.served < state.limit
    ■ noted ⇐ state.served = old(state).served + 1 ∧ state.limit = old(state).limit
│ state.served ← state.served + 1
│ ■ noted
╵
```

In the body, `state` is a logical name for the value behind the borrow. The
assignment needs `state.served + 1 ≤ 2³² - 1` ([P5]), which follows from the
precondition and the range of `state.limit`. It then renames ([I9]):

```text
state′ = counter {state.served + 1, state.limit}
```

At the exit, `state` in the condition is `state′` and `old(state)` is `state`.
Both conjuncts follow from [A4] by a chain: `state′.served` is the first field
of that constructor, and `state′.limit` the second.

A caller that lends a whole counter learns exactly what the contract says:

```text
1  ⊤       ▷ k : ? c.served < c.limit
2  k.true  ▷ n : Δ note(&~c) ▸ noted {}
3  k.true  ▷ ⊢ c.served ≤ c.limit
```

After statement 2, `c` denotes a fresh `c′` on `k.true`, with
`n.noted ⇒ c′.served = c.served + 1 ∧ c′.limit = c.limit`. Statement 3 runs on
`k.true ∧ n.noted` and asks for `c′.served ≤ c′.limit`, which is
`c.served + 1 ≤ c.limit`: the test at 1. Had the contract not mentioned
`limit`, the obligation would fail, with a counterexample in which the callee
lowered it.

### 10.4 A product within its type

Two `u32` values are multiplied as `u64`. The conversion denotes the same
integers ([I6]), so the declared interval of each operand is `[0, 2³² - 1]`
([A9]). With `M = 2³² - 1` and `w = mul(width, height)`, the axioms include
`w ≤ M·height`, and the range of `height` gives `w ≤ M·M`, which is less than
`2⁶⁴`. The obligation that the product fits `u64` is proved. No fact about the
product itself was needed, and none is available.

### 10.5 A remainder as an index

```text
control slot(buckets: &[bucket], hash: usize) ⊢ len(buckets) ≥ 1
    ■ at {index: usize} ⇐ index < len(buckets)
│ ■ at {hash mod len(buckets)}
╵
```

The divisor is not constant, so the payload is `rem(hash, len(buckets))`
([I4]). The divisor is not zero by the precondition. [A8], first row, gives
`0 ≤ rem ≤ len(buckets) - 1`, which is the range obligation and the outcome
condition.

### 10.6 The same question asked twice

```text
1  ⊤       ▷ a : Δ order.compare(rule, &x, &y) ▸ before {} │ equal {} │ after {}
…
7  a.equal ▷ b : Δ order.compare(rule, &x, &y) ▸ before {} │ equal {} │ after {}
8  b.before ∨ b.after ▷ ⊥
```

If `compare` declares no effects, [I10] gives `a.equal ⇒ alt(rule, x, y) =
equal` and `b.before ⇒ alt(rule, x, y) = before`. Unless `x`, `y` or `rule`
was renamed between the two statements, the two applications are one term, and
`equal` and `before` are different constants. Statement 8 is proved by a chain
of two links. The same holds for a variant decided twice, by [I11] and [A5].

## 11. Limits, and what to do at one

Out of scope for v2.0, as the plan says: quantifiers, predicates over the
heap, user-defined recursive predicates and nonlinear arithmetic. In practice
the fragment cannot prove:

- A property of every element of a collection (sorted, all below a bound). An
  index read from one vector and used on another needs a decision.
- A property that survives a change of shape, other than what the contract of
  the operation states about length.
- Anything that depends on the value of a product of two unknowns, beyond the
  bounds of [A9]. Row-major indexing, `row * width + column`, is the usual
  case: its counterexample assigns the product a value that the real product
  could not have, and the report says so ([M3]).

When a proof fails, the program is changed in one of four ways, and the
language offers nothing else:

1. **Decide.** Use the operation with outcomes (`get` with its `out_of_bounds`
   alternative, `checked_mul` with `overflow`) and dispose the alternative.
2. **State.** Write the missing fact where it becomes someone's obligation: a
   precondition on a control or function, an invariant on a loop, a condition
   on an outcome, or a `⊢` line that is provable where it stands.
3. **Choose a total operator**: wrapping or saturating arithmetic, where that
   is what the program means.
4. **Assume**, as `assume ⊥ "reason"`: a fault at run time, permission to
   fault, and an entry in the trust report ([F4]).

## 12. What this document asked of the core

Applied to `core.md` together with the repairs from its second review:

1. [N3] gains exactness and determinism ([J5], [J6]). A failed proof always
   has a counterexample; "outside the fragment" becomes the remark of [M3].
   "No timeouts" becomes the step bound of [J6].
2. [N4]: the row for a write to a component is replaced by [I9]; rows are added
   for decisions on variants ([I11]) and on pure operations ([I10]).
3. [N5]: entry to a loop renames what the body can change and no more ([I9]).
4. [E-Arith] states the rounding of signed division ([I4]). [E-Formula] says
   that a term without a defined value has a value of its type that depends
   only on its operands.
5. `len` applies to maps as well as to vectors and strings ([A3]). The measure
   of a task set, `count`, came with the core's draft 2 ([TK6]). The lengths
   of a vector literal and of the empty map came with the [corpus](corpus.md),
   which needs `len([]) = 0` for every invariant over a vector it builds.
6. The largest `usize` is the constant of [A2].
7. [E-Host]: a faithful host answers a pure extern control with the same
   alternative and payload on equal arguments ([I10]).
8. [P5]: a conversion is checked unless every value of its source type is a
   value of its target type ([I6]).
9. [P7]: the obligations of one statement are proved in the order of
   evaluation, each a fact for the next.

## 13. Choices and questions

Choices of this draft that follow from the accepted decisions. The author
accepted them with phase 1 on 20 November 2026:

1. **Formulas are Boolean expressions** ([S4]). There is no second language for
   specifications.
2. **Acceptance is defined by a base logic and an instance set, not by a
   solver** ([J3], [J5]). The reference procedure is one way to decide it.
3. **The kernel is in the acceptance path** ([V1]), and a counterexample is
   evaluated before it is shown ([M1]).
4. **Element facts are minimal** ([A6]): one write changes one element. Nothing
   about elements survives a change of shape.
5. **Products get bounds from declared types only** ([A9]), so that what is
   provable can be read off the program text.
6. **A failed obligation is assumed after it is reported** ([M5]).
7. **Only a part of a held value has a range** ([A1]), and every axiom about a
   typed operator is true for all integers or guarded by its operands' range.
8. **An operation is pure only if nothing about it can touch the world**
   ([I10]): no effects, declared or inferred, no `async`, no resources.

The author answered the questions of this draft on 20 November 2026 and took
the recommendation in each case:

| | Question | Decision |
| --- | --- | --- |
| Q6 | Is the width of `usize` fixed? | **No.** One constant between `2³² - 1` and `2⁶⁴ - 1` ([A2]), so that a checked program is checked for every supported target. The cost: converting `u64` to `usize` needs a proof or a decision, even in a program that will only run on 64-bit machines. A build could later be allowed to fix the constant. |
| Q7 | May the compiler give up on a proof? | **Only as a counted limit** ([J6]), never as a verdict. Acceptance must not depend on the speed of a machine or on the compiler's release. |
| Q8 | Is a function's body visible to the logic at its calls? | **Yes** ([I8]). Functions are non-recursive expressions, and a caller usually needs exactly what the body computes. The body of a public function is then part of its interface. Phase 5, which adds recursion, has to revisit this; the alternative is a stated result condition, as controls have. |
| Q9 | Is an operation with no declared effects a function of its arguments? | **Yes** ([I10]). It is what the plan's section 5.2 says of pure decisions, and it removes filler arms for repeated questions. It makes "no `uses`" a real assertion about an extern control: observing the world is an effect too. |
| Q10 | How does signed division round? | **Toward zero**, and the remainder has the sign of the dividend ([I4]): what the hardware and the systems languages do. v1 had no infix division, so nothing is carried over. |
