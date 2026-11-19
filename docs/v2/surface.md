# Villanelle v2 surface: Rails

Status: **draft 1**, 19 November 2026. Phase 1, deliverable 3 of the
[redesign plan](../redesign-plan.md). Not yet accepted by the author; it becomes
normative when it is. Nothing here is implemented. This draft has not been
reviewed independently.

[Index of the v2 design](README.md) · [Core calculus](core.md) ·
[Fact logic](facts.md) · [Rails reference programs](../../experiments/syntax_lab/rails/)

## 1. Purpose and scope

Rails is the source syntax of v2, adopted in the plan's section 5.5. This
document gives it three things the reference programs cannot give by example.

- **Layout** (section 3): how the rails express block structure, exactly enough
  for a parser to read it and a formatter to draw it.
- **Grammar** (section 4): the declarations, statements, arms, labels and
  expressions.
- **Elaboration** (sections 5 to 8): the translation of every surface form into
  the [core calculus](core.md). The meaning of a Rails program is the meaning
  of its core form, and every check is made there.

It also fixes the input spelling for people who type words (section 9).

Draft 1 covers what the core's draft 1 covers: the synchronous language. Tasks,
fault boundaries, callable parameters, modules and process entry are listed in
section 11 with the syntax the reference programs already use for them; their
rules follow the core's draft 2.

Clause identifiers here use the families **Y** (layout), **G** (grammar) and
**U** (elaboration).

## 2. Rails in one example

```text
control show_file(filename: string) ■ auto
│ Δ open(filename)
│ ├ opened {file}
│ ├ not_found → ■ missing
│ └ denied    → ■ failed
│ Δ read(file): data {contents} │ failed
│
│ ┄ Always attempt close, even if reading failed.
│ Δ close(▾file): closed │ failed
│
╞═ data
│ ┆ Δ decode_utf8(contents): text {text} → decoded │ invalid
│
╞═ decoded ∧ closed
│ ┆ emit(text)
│ ┆ ■ done
╞═ read.failed ∨ close.failed ∨ invalid
│ ┆ ■ failed
╵
```

Reading it:

- The solid rail `│` is the body of the control. A statement on it runs on every
  history that is still running. `╵` ends the body.
- `Δ` decides an operation. Each alternative of the operation is named, on one
  line after a colon or on lines of its own marked `├` and `└`.
- An alternative with `→` and a terminator ends there: `not_found → ■ missing`.
  One without a target continues: after `opened {file}` the body goes on, with
  `file` bound.
- Every alternative that continues is a **path**, named after itself (`data`,
  `closed`) or renamed by a route (`text {text} → decoded`). A route names a
  path. It does not jump anywhere.
- A junction `╞═` introduces a **label**: a condition over paths. The dashed
  rail `┆` beneath it is the extent of the label, and the statements on it run
  on the histories for which the condition holds.
- Two decisions here have an alternative `failed`, so the name alone would be
  ambiguous; the label says `read.failed` and `close.failed`.

Its core form, with each site named after its operation:

```text
1  ⊤                    ▷ open : Δ open(&filename) ▸ opened {file} │ not_found {} │ denied {}
2  open.not_found       ▷ ■ missing {}
3  open.denied          ▷ ■ failed {}
4  ⊤                    ▷ read : Δ read(&file) ▸ data {contents} │ failed {}
5  ⊤                    ▷ close : Δ close(file) ▸ closed {} │ failed {}
6  read.data            ▷ decode_utf8 : Δ decode_utf8(&contents) ▸ text {text} │ invalid {}
7  decode_utf8.text ∧ close.closed
                        ▷ emit : Δ emit(&text) ▸ done {}
8  decode_utf8.text ∧ close.closed
                        ▷ ■ done {}
9  read.failed ∨ close.failed ∨ decode_utf8.invalid
                        ▷ ■ failed {}
```

Statement 4 has the selector `⊤` and still does not run after statement 2 or 3
has exited ([K3]). The checker proves that no history reaches the end ([K7]):
every combination of `read`, `close` and `decode_utf8` is disposed by statement 8 or
9. `■ auto` makes the contract the set of exits the body uses: `missing`,
`failed` and `done`.

## 3. Layout

Rails are syntax. The block structure of a body is read from the rail
characters at the start of each line, as other languages read it from braces
or indentation.

- **[Y1]** Source is UTF-8 text with lines ended by a line feed. A declaration
  starts in the first column. A signature may continue on following lines
  indented by four spaces.
- **[Y2]** Inside a control, every line is a **prefix** followed by content. The
  prefix is a sequence of **segments**, each two columns wide: a rail character
  and a space. There is one segment for each block the line lies in, outermost
  first:

  | Segment | Block |
  | --- | --- |
  | `│ ` | The body of the control |
  | `║ ` | The body of a loop |
  | `┆ ` | The block of a label, of a test, or of an arm |
  | `│ ` or two spaces, before a `┆ ` | The arms above: the rail continues while more arms follow, and is blank under the last arm |

- **[Y3] Control.** After the signature, the lines of the body carry `│ `. The
  body ends with a line that holds `╵` alone in the first column.
- **[Y4] Loop.** A loop statement is one line (`↻` or `∀`). The lines of its
  body carry one more segment, `║ `. The loop ends before the first line that
  does not carry it.
- **[Y5] Label.** A label stands directly in the body of a control or of a
  loop, never inside a `┆` block. On its line the junction takes the place of
  the body's own segment: `╞═` on a control's rail, `╠═` on a loop's. The mask
  follows. The lines of its block carry the body's segment again and then `┆ `.
  The short form puts a colon after the mask and the statements on the same
  line, separated by `;`.
- **[Y6] Test.** `? e →` is followed on the same line by one statement that
  needs no block, or ends the line and is followed by a block whose lines carry
  one more `┆ `.
- **[Y7] Arms.** In the vertical form each arm is a line of its own that begins
  with `├ `, and the last with `└ `, at the nesting of the decision. An arm that
  ends in `→` opens a block; its lines carry the arm rail of [Y2] and then `┆ `.
  In the horizontal form the arms follow the operation on its line, after a
  colon and separated by `│`; no arm has a block.
- **[Y8] Continuation.** A line whose content begins with white space continues
  the statement of the line above.
- **[Y9]** A blank line in a body is its prefix alone. A comment is `┄` and the
  rest of the line, as content at any nesting or in the first column between
  declarations.
- **[Y10]** A line whose prefix does not fit the blocks open at that point is an
  error. The compiler does not guess; the formatter draws rails, and a person
  does not have to (section 9).

The formatter chooses line breaks, aligns the arrows of a group of arms and
the bodies of a run of short labels, and wraps at 100 columns. Those choices
are not syntax: the parser accepts any spacing inside content.

## 4. Grammar

Blocks are written `{…}` in the grammar below and are drawn as section 3 says.
`,*` is a comma-separated list. Words in the grammar are keywords.

### 4.1 Declarations

```text
File      ::= Decl*
Decl      ::= import "path" as name
            | [pub] outcome name Alts
            | [pub] struct name [Generics] { Field,* }
            | [pub] variant name [Generics] Alts
            | [pub] view struct name [Generics] { Field,* }
            | [pub] function name [Generics] ( Param,* ) [⊢ Formula] : Type := Expr
            | extern resource name [on fault release]
            | extern control name ( Param,* ) Sig
            | [pub] control name [Generics] ( Param,* ) Sig {Body}
Sig       ::= [⊢ Formula] ■ Contract [faults] [uses QName,*]
Contract  ::= auto | name | Entry (│ Entry)*
Entry     ::= name [{ Field,* }] [⇐ Formula] | …name
Alts      ::= : Alt (│ Alt)*   |   one line for each, marked ├ and └
Alt       ::= name [{ Field,* }] [↦ number]
Field     ::= name : Type [∈ name,*]
Param     ::= name : Type
Generics  ::= < (name [: QName]),* >
QName     ::= name (. name)*
Type      ::= bool | uN | iN | usize | string | name [< Type,* >] | name . name
            | [ Type ] | Type ⇀ Type | & Type | &~ Type
```

- **[G1]** A contract with conditions may put each entry on a line of its own,
  each introduced by `■`.
- **[G2]** `…name` in a contract includes every alternative of the outcome
  declaration `name`, and a contract that is one name declared as an outcome
  is that declaration's contract. The contract remembers which declarations
  it took alternatives from ([U7], [U14]).
- **[G3]** `■ auto` is allowed on a control that is not `pub`. Its contract is
  the set of alternatives its exits use, with the payload types found there.
- **[G4]** `↦ number` on an alternative of an outcome declaration is the
  process status of a program that exits with it. It has no other meaning.

### 4.2 Statements

```text
Body     ::= (Stmt | Label)*
Label    ::= Mask {Stmt*}   |   Mask : Simple (; Simple)*
Mask     ::= Path | ¬ Path | Mask ∧ Mask | Mask ∨ Mask | ( Mask )
Path     ::= name | name . name

Stmt     ::= [~] name [: Type] := Expr                      bind
           | [~] name := capture Call                       capture a completed outcome
           | name { name,* } := Expr                        take a struct apart
           | Place ← Expr                                   assign
           | ⊢ Formula                                      obligation
           | ? Expr → (Simple | {Stmt*})                    test
           | [name :] Δ Operand Arms                        decision
           | ■ Δ Operand Arms                               exit by adapting a decision
           | forward Call                                   exit with the callee's outcome
           | Call                                           call with one outcome
           | ↻ name [( LoopParam,* )] {[⊢ Formula] Stmt*}   loop
           | ∀ name ∈ Place {Stmt*}                         for each element
           | Terminator
Terminator ::= ■ name [{ Expr,* }] | □ [name [{ Out,* }]] | ↑ name [( Expr,* )]
           | fault Expr | ⊥
Simple   ::= a statement with no block and no vertical arms
LoopParam ::= [~] name : Type := Expr
Out      ::= name | name : Expr

Operand  ::= Call | [▾ | ~] Place
Call     ::= QName ( Arg,* ) | Place . name ( Arg,* )
Arg      ::= Expr | ~ Place | ▾ Place
Arms     ::= : Arm (│ Arm)*   |   one line for each, marked ├ and └
Arm      ::= (name [{ Pattern,* }] | …name) [→ Target]
Pattern  ::= [~] name [: Type]
Target   ::= name | Terminator | ■ | {Stmt*}
           | name [{ Expr,* }]                              in an adapter only
```

- **[G5]** In `Mask`, `¬` binds tighter than `∧`, and `∧` tighter than `∨`. `¬`
  applies to a single path and not to a parenthesis.
- **[G6]** A `⊢` line that is the first statement of a loop body is the loop's
  invariant. Anywhere else it is an obligation at that point.
- **[G7]** `fault` and `forward` are words. Everything else in a statement has
  a glyph.

### 4.3 Expressions

```text
Expr   ::= literal | name | Expr . name | Place [ Expr ] | QName ( Expr,* )
         | Expr . name ( Expr,* ) | name { Expr,* } | name . name { Expr,* }
         | [ Expr,* ] | ∅ | len ( Place ) | clone ( Place )
         | ( Expr ) | ¬ Expr | - Expr
         | Expr op Expr
Place  ::= name | Place . name | Place [ Expr ]
```

Operators, loosest first: `∨`; `∧`; `= ≠ < ≤ > ≥` (not associative); `+ -`;
`* ÷ mod`; unary `¬` and `-`. `=` is equality, and binding is always `:=`. A
formula is an expression of type `bool`; in a contract condition it may also
use `old(name)` ([W7]).

## 5. Paths

- **[U1] Sites.** Every decision, test and loop is a site of the core. A
  decision's site is named after its operation: the last name in the call, or
  the last name of the place it decides. A loop's site has the loop's name. A
  site may be named explicitly, `name : Δ …`, and must be when two sites of one
  control would otherwise share a name and a label needs to tell them apart.
  The sites of tests have no names.
- **[U2] Path names.** An arm without a target gives its alternative the path
  name of the alternative. An arm with a route, `→ name`, gives it that name
  instead. An arm with a terminator or a block gives it no path name. A `□ a`
  gives the alternative `a` of its loop the path name `a` in the body that
  contains the loop.
- **[U3] Meaning.** In a mask, `site.alternative` is the atom of that name. An
  unqualified `name` is the disjunction of the atoms that have the path name
  `name`. If more than one arm has it only by default, the unqualified name is
  an error and the qualified form is required; routes may share a name on
  purpose.
- **[U4] Masks.** A mask is the selector built from its paths with `∧`, `∨` and
  `¬`. It may mention the paths of sites that precede the label in its own body
  or that precede an enclosing loop ([W2]).
- **[U5] Negation.** `¬ name` holds on a history where no arm with that path
  name was taken, including one where its decision never ran. The compiler
  warns when `¬` is applied to a path whose complement in its own decision is
  a single other path, and suggests naming that path.

## 6. Elaboration of statements

Every surface block has a **selection**, a selector of the core. The body of a
control and the body of a loop have the selection `⊤`. The block of a label has
its mask. The block of a test at site `d` has `d.true`, and the block of an arm
for alternative `a` at site `d` has `d.a`; neither needs the selection of the
block around it, because `d.a` implies that `d` ran ([K4]).

- **[U6] Statements.** A statement in a block with selection `σ` becomes one or
  more core statements, in source order:

  | Surface, under `σ` | Core |
  | --- | --- |
  | A bind, capture, unpacking, assignment or `⊢` | `σ ▷` the same command |
  | A terminator | `σ ▷` the same terminator |
  | `? e → B` | `σ ▷ d : ? e`, then `B` under `d.true` |
  | `Δ ω(ā) arms` | `σ ▷ d : Δ ω(ā′) ▸ …`, listing every alternative of the contract with its payload names; then the target of each arm, in order, under `d.a` |
  | `Δ e arms`, `e` of type `bool` | `σ ▷ d : ? e`; the arms `true` and `false` each need a target |
  | `Δ p arms`, `p` a variant | A decision on the variant through a shared borrow; `Δ ~p` through an exclusive one; `Δ ▾p` by value |
  | `Δ ▾x arms`, `x` a captured outcome | `σ ▷ d : Δ outcome(x) ▸ …` |
  | `ω(ā)` | `σ ▷ d : Δ ω(ā′) ▸ a {}`; the operation must have exactly one alternative, and its payload, if any, is dropped |
  | A loop | `σ ▷ ℓ : ↻ ℓ(…) ⊢ φ {body under ⊤} ▸ …` ([U9]) |

- **[U7] Acknowledgement.** The arms of a decision name every alternative of
  the operation's contract exactly once: by an arm of its own, or by one group
  arm `…g`. A group arm is allowed only when the operation's contract includes
  `g` by name, and stands for the alternatives of `g` that have no arm of their
  own. There is no arm that means "everything else". This is the rule [C2] of
  the core as a person writes it.
- **[U8] Arms.** A pattern names the payload fields in order; types come from
  the contract and may be written. An arm without a pattern binds nothing. For
  each arm, by its target:

  | Target | Under `d.a` |
  | --- | --- |
  | None, or a route | Nothing. The history continues, and the alternative has a path name ([U2]). |
  | A terminator | That terminator |
  | `■` alone | `■ a {ȳ}`: exit with the same alternative and its payload |
  | A block | The statements of the block |
  | A group arm with a target | The same target for each alternative it stands for |

- **[U9] Loops.** The loop statement lists the alternatives that the `□`
  statements of its body use ([W9]). All `□ a` of one loop give the same field
  names: an entry `name` both names the field and supplies the value of that
  name, and `name : e` supplies another value. After the loop those names are
  bound in the body that contains it. If the loop body has a name of its own
  that is the same, elaboration renames the inner one, so [W4] holds in the
  core. A `□` without a name is allowed when every `□` of the loop has none;
  the body then continues after the loop with nothing to tell apart.
- **[U10] For each.** `∀ x ∈ p {B}` is a loop over the indices of the vector
  `p`, with a hidden index `i`:

  ```text
  ℓ : ↻ ℓ(i : usize := 0) ⊢ i ≤ len(p)
        ⊤       ▷ e : ? i = len(p)
        e.true  ▷ □ done {}
        ⊤       ▷ x := &p[i]
                  B
        ⊤       ▷ ↑ ℓ(i + 1)
      ▸ done {}
  ```

  A history that reaches the end of `B` goes on to the next element. `B` may
  exit, and may leave with a named alternative. The three obligations of the
  expansion (the index, the increment and the invariant) are proved from the
  test and the invariant in every case; a body that shortens `p` fails the
  invariant at `↑` and is rejected there.

## 7. Elaboration of calls and exits

- **[U11] Arguments.** How an argument is passed is decided by the parameter's
  type, and marked at the call where the call can change or end something:

  | Parameter | Written | Core argument |
  | --- | --- | --- |
  | `&τ` | `p`, or any expression | `&p`; an expression that is not a place is first bound to a hidden name |
  | `&~τ` | `~p` | `&~p` |
  | By value, kind `copy` or `drop` | `e` | `e` (a move, for `drop`) |
  | By value, kind `must` | `▾p` | `p` (a move) |

  A borrow name passed where a borrow is expected is passed as it is. A bind
  whose written type is a borrow type takes its value by the same table:
  `r: &τ := p` borrows the place `p`, and `r: &~τ := ~p` borrows it
  exclusively. A bind without a written type always takes a value ([W8]).
- **[U12] Method form.** `p.m(ā)` is `m(p, ā)`, where `m` is found among the
  operations of the type of `p` or of the capabilities it is bound by. The
  receiver is lent as `m` declares; an exclusive loan needs a receiver whose
  root is bound with `~`.
- **[U13] Adapters.** `■ Δ ω(ā) arms` decides `ω` and exits on every
  alternative. Each arm is `a` alone, which exits with the alternative `a` and
  its payload; or `a {pattern} → b {ē}`, which exits with `b`. A group arm
  exits with each of its alternatives under its own name. The elaboration is
  the decision followed by one `d.a ▷ ■ …` for each alternative.
- **[U14] Forward.** `forward ω(ā)` is the adapter in which every alternative
  exits under its own name. It requires every entry of the callee's contract,
  a single alternative or a group included by name, to be an entry of the
  caller's contract with the same payload types. `■ x`, for a captured outcome
  `x`, is the same for `Δ ▾x`.
- **[U15] Exits.** The payload of `■ a {ē}` is positional, against the fields
  of `a` in the contract. Conditions `⇐ φ` of the caller's contract are proof
  obligations at each exit the elaboration produces ([P4]), including those of
  an adapter and of `forward`.

The rule [U14] is what keeps `forward` from absorbing a new alternative
silently. A new alternative in a callee's own list has no entry in the caller
and is an error there. A new alternative in a group that both contracts
include appears in both signatures by that one edit.

## 8. Warnings

- **[U16]** An arm that disposes an alternative the checker proves impossible
  gets a warning that suggests `⊥`, in code that is not generic (core, Q5).
- **[U17]** A route to a path name that no label mentions gets a warning.
- **[U18]** The warning of [U5].

Warnings never change the meaning of a program.

## 9. Input spelling and the formatter

The stored text is the glyph form with its rails. Nobody is asked to type it.

- **[G8] Words for glyphs.** Each glyph has a spelling in ASCII, accepted
  wherever the glyph is:

  | Glyph | Word | Glyph | Word |
  | --- | --- | --- | --- |
  | `Δ` | `delta` | `∀ x ∈ p` | `for x in p` |
  | `→` | `=>` or `->` | `∈` | `in`, `from` |
  | `■` | `exit`, `exits` | `⊢` | `where` |
  | `□` | `leave` | `⇐` | `when` |
  | `↻` | `loop` | `⊥` | `impossible` |
  | `↑` | `again` | `←` | `<-` |
  | `?` | `if` | `↦` | `\|->` |
  | `&` | `ref` | `…` | `..` |
  | `~` | `mutable` | `∅` | `map {}` |
  | `▾` | `move` | `┄` | `//` |
  | `≠ ≤ ≥` | `!= <= >=` | `∧ ∨ ¬` | `&& \|\| !` |
  | `÷` | `/` | `=` | `==` |
  | `[T]` | `vector<T>` | `K ⇀ V` | `map<K, V>` |

- **[G9] Braces for rails.** In the **plain form** a body, a loop body, a test
  block and an arm block are written in braces, the arms of a decision in
  braces separated by commas, and a label as `[mask]:` followed by a braced
  block or by statements on the same line. Statements are separated by line
  ends or `;`. A declaration is read in the plain form when its body opens
  with `{`, and in Rails otherwise.
- **[G10] The formatter** turns either form into the canonical Rails text. It
  is the identity on its own output. It never changes the order of statements,
  adds or removes a label, or changes what a mask says, and it keeps comments
  and blank lines.

The control `spend` of the [fact logic](facts.md), section 2.1, in the plain
form:

```text
control spend(total: u32, used: u32) exits left {value: u32} | over {
    if used > total => exit over
    delta checked_sub(total, used) { difference {value}, underflow => impossible }
    exit left {value}
}
```

The plain form has the grammar of section 4: names come before types, and
binding is `:=`. The word-form files under `experiments/syntax_lab/syntax/v2`
predate this document; they put types first, as v1 does, and are the lab
transpiler's input only (question Q11).

## 10. Worked elaborations

### 10.1 An adapter with a group

```text
pub outcome storage_error
├ invalid_file {message: string}
├ storage_failed {message: string}
└ too_large

pub control load(path: &string) ■ loaded {values: string ⇀ string} │ …storage_error
│ ■ Δ json_file.load(path, 1048576)
│ ├ loaded
│ ├ too_large
│ ├ missing               → loaded {∅}
│ ├ invalid {message}     → invalid_file {message}
│ └ read_failed {message} → storage_failed {message}
╵
```

```text
control load(path : &string)
    ■ { loaded {values : string ⇀ string} │ invalid_file {message : string}
      │ storage_failed {message : string} │ too_large {} }

1  ⊤                ▷ load : Δ json_file.load(path, 1048576)
                          ▸ loaded {v} │ too_large {} │ missing {}
                          │ invalid {message} │ read_failed {message}
2  load.loaded      ▷ ■ loaded {v}
3  load.too_large   ▷ ■ too_large {}
4  load.missing     ▷ ■ loaded {∅}
5  load.invalid     ▷ ■ invalid_file {message}
6  load.read_failed ▷ ■ storage_failed {message}
```

`message` has two binders on histories that exclude each other, which the core
allows ([O1]). The arm `loaded` binds nothing in the source; elaboration gives
its payload the hidden name `v`. A caller disposes the three errors in one arm
and still names them where it wants to:

```text
│ Δ load(path): loaded {~values} │ …storage_error → ■
```

Under [U7] the group arm stands for `invalid_file`, `storage_failed` and
`too_large`, and `→ ■` exits with each under its own name. If `storage_error`
gains an alternative, this line and the caller's own signature take it up
through the group, and every caller that named the alternatives one by one
stops compiling until it names the new one.

### 10.2 For each

```text
pub control first<item>(source: &[item], accepts: &item → bool)
    ■ found {value: &item ∈ source} │ missing
│ ∀ value ∈ source
│ ║ ? accepts(value) → ■ found {value}
│ ■ missing
╵
```

```text
1  ⊤ ▷ each : ↻ each(i : usize := 0) ⊢ i ≤ len(source)
          1  ⊤         ▷ end : ? i = len(source)
          2  end.true  ▷ □ done {}
          3  ⊤         ▷ value := &source[i]
          4  ⊤         ▷ t : ? accepts(value)
          5  t.true    ▷ ■ found {value}
          6  ⊤         ▷ ↑ each(i + 1)
        ▸ done {}
2  ⊤ ▷ ■ missing {}
```

At statement 3 the index is in range because `end.false` and the invariant give
`i < len(source)`. At statement 6 the same two facts give the invariant for
`i + 1`. At statement 5 the payload borrow is derived from `source` alone,
which is what the contract's `∈ source` requires ([L4]). The callable parameter
`accepts` is outside draft 1; it is shown as the reference program has it.

### 10.3 Binary search

The Rails text and the core form of `bound` are in the
[core calculus](core.md), section 2. Three points of this document appear in
it. `Δ upper: true → high │ false → low` is a decision on a `bool`, so it is a
test whose two alternatives have the path names `high` and `low` ([U6]). The
labels `after ∨ low` and `before ∨ high` mix the default names of one decision
with the routes of another ([U3]). And the `⊢` line is the loop's invariant
because it is the first line on the loop's rail ([G6]).

## 11. Outside draft 1

The reference programs use these forms. Their syntax stands as shown there;
their rules come with the core's draft 2 or with phase 5.

| Form | Example | Comes with |
| --- | --- | --- |
| Tasks | `async control`, `Δ await f(…)`, `spawn f(…) into set`, `cancel set`, `capture run f(…)` | Core draft 2 |
| Fault boundaries | `fault boundary` and its contingency, as in v1 | Core draft 2, with a Rails drawing |
| Callable parameters and function values | `accepts: &item → bool`, `value ↦ value ≥ limit`, `test: (&item) ■ matched │ skipped` | Phase 5 |
| Resource protocols | `protocol a? ; (b │ c)` | Phase 6 |
| Modules and visibility | `import "…" as name`, `pub` | Draft 2 of this document |
| Process entry and status | `outcome process_result: done ↦ 0 │ missing ↦ 3` | Draft 2 of this document |
| Text interpolation | `emit("{value}")` | The library |

## 12. What this document asked of the core

1. A bind may take a borrow, `x := &p` and `x := &~p`. The core now writes
   this with operands; an assignment never takes one ([W8]).
2. The test at the site of a `Δ` on a `bool` and the test `?` are one core
   form; nothing changes.
3. The acknowledgement rule [C2] stays as it is. Groups exist only in the
   surface and in the record a contract keeps of the groups it includes.

## 13. Choices and questions

Choices of this draft that follow from the accepted decisions and from the
reference programs, recorded for the author's acceptance of phase 1:

1. **Rails are syntax** ([Y10]). The parser reads block structure from them
   and does not repair them.
2. **A label stands only on a body's rail** ([Y5]), never inside a dashed
   block. Paths belong to the body, as names and atoms do in the core.
3. **A route names and does not jump** ([U2]). Every statement on the solid
   rail runs on every history still running.
4. **A name that two arms have by default must be qualified; routes may
   merge** ([U3]).
5. **An arm without a pattern binds nothing** ([U8]).
6. **`▾` is required exactly where a value of kind `must` moves**, and for
   taking a variant apart by value ([U6], [U11]). Moving a `drop` value is
   unmarked.
7. **A group arm needs the group in the callee's contract by name** ([U7]),
   and `forward` needs each entry of the callee in the caller ([U14]). This
   refines the plan's section 5.4, which asked for a named contract; the
   reference program `ordered_search` forwards a contract of one unnamed
   alternative, and the rule admits it.
8. **`¬` applies to one path** ([G5]), with the warning of [U5]. This is the
   answer to Q4 of the core.
9. **Mutating a vector inside `∀` is checked, not forbidden** ([U10]): the
   invariant at `↑` decides.

Questions for the author, each with a recommendation. The draft is written as
if the recommendation were taken.

| | Question | Recommendation |
| --- | --- | --- |
| Q11 | Is the input spelling a second grammar or the same one? | **The same one** ([G8], [G9]): Rails with words for glyphs and braces for rails, names before types. One grammar to specify, to parse and to learn. The plan's table shows the type-first form `type name = value` as the word for a binding; taking the recommendation retires that form, and the migrator of decision D8 reads v1 instead. |
| Q12 | How is assignment written? | **`x ← e`**, typed `<-`. The reference programs never assign, so nothing was adopted. `=` is equality and `:=` binds. |
| Q13 | How is the remainder written? | **`mod`**, as a word operator: `hash mod len(buckets)`. No glyph for it is in common use. |
| Q14 | How is a decision site named when two would collide? | **`name : Δ …`** ([U1]), the core's own form. |
| Q15 | What does a `□` with no name mean? | **Leaving with nothing to tell apart**, allowed only when no `□` of the loop has a name ([U9]). |
