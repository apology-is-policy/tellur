# Villanelle v2 corpus on paper

Status: **draft 1**, 19 November 2026. Phase 1, deliverable 3 of the
[redesign plan](../redesign-plan.md). Not yet accepted by the author. Nothing
here is implemented, and no tool has parsed or checked the programs: they were
written and checked by hand against the other documents. One independent
review has read them against the rules (section 8).

[Index of the v2 design](README.md) · [Core calculus](core.md) ·
[Fact logic](facts.md) · [Reference model](references.md) ·
[Surface](surface.md)

## 1. Purpose and scope

The plan asks for the existing programs rewritten by hand in v2, and for three
new ones chosen to stress the design. The rewrite tests the other four
documents on real code, yields the counts the plan asks for, and supplies the
material for the reading and repair test.

| Group | v1 source | v2 text |
| --- | --- | --- |
| The eight collection modules | `library/{membership, search, reduction, partition, sequence_batches, traversal, ordering, ordered_search}.vn` | [`corpus/library/`](corpus/library/) |
| The settings application | `applications/settings/{settings, http, server, main}.vn` | [`corpus/settings/`](corpus/settings/) |
| A retrying network client | none | [`corpus/programs/retry_client.txt`](corpus/programs/retry_client.txt) |
| A recursive-descent parser | none | [`corpus/programs/parser.txt`](corpus/programs/parser.txt) |
| A small interpreter over a tree | none | [`corpus/programs/interpreter.txt`](corpus/programs/interpreter.txt) |

Two more files state what the programs rely on and are not counted:
[`corpus/prelude.txt`](corpus/prelude.txt), the built-in operations with the
contracts the proofs use, and [`corpus/settings/host.txt`](corpus/settings/host.txt),
the signatures of the host and of the two library modules the application
imports.

A rewrite keeps the module, its public controls and their outcomes, the
private helpers, the algorithm, the names and the comments. Inside a control it
is written as v2 is meant to be written. Where the structure differs, section 4
says so.

## 2. How the counts are made

[`corpus/count.py`](corpus/count.py) produces every number in section 3 from
the text. Filler arms are the exception: they are judged by reading and listed
by line.

| Count | v1 | v2 |
| --- | --- | --- |
| Lines | Lines that are not blank and not only a comment | The same; a line that holds only rails is blank, and the closing `╵` counts as a closing brace does |
| Decisions | `delta` | `Δ` |
| Labels | `[mask]:` | A junction `╞═` or `╠═` |
| Routes | An arm that names the path it continues on, `=> name` | An arm `→ name`. An arm with no target continues under its own name and is not counted. |
| Origin marks | `from x`, `['x]`, `<- 'x`, and an origin argument on a type | `∈` on a payload field |
| Borrow and move marks | `borrow`, `borrow mutable`, `deref`, `move` at a use | `~place` and `▾place` at a use |
| Types at bindings | A type or `auto` on a payload name, a local or a loop parameter | A written type on a bind or a loop parameter |
| Type arguments | `name<…>(` at a call | None exist ([U20]) |
| Proof lines | None exist | A line with `⊢` or `⇐` |
| Impossible | None exist | `⊥` |
| Uses | None exist | A `uses` clause |

A **filler arm** is an arm for an alternative that cannot occur where the arm
stands. In v1 it must be written anyway, with a disposition that never runs.

## 3. Results

### 3.1 Totals

| Group | Version | Lines | Decisions | Labels | Routes | Filler arms | Origin marks | Borrow and move marks | Types at bindings | Type arguments | Proof lines | `⊥` | `uses` |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Library | v1 | 476 | 67 | 66 | 72 | 13 | 10 | 19 | 82 | 16 | 0 | 0 | 0 |
| Library | v2 | 231 | 29 | 7 | 7 | 0 | 7 | 0 | 24 | 0 | 8 | 3 | 0 |
| Settings | v1 | 635 | 64 | 86 | 92 | 2 | 0 | 51 | 116 | 0 | 0 | 0 | 0 |
| Settings | v2 | 304 | 41 | 18 | 0 | 0 | 0 | 6 | 3 | 0 | 0 | 2 | 6 |
| New programs | v2 | 208 | 35 | 22 | 9 | 0 | 0 | 9 | 8 | 0 | 16 | 0 | 5 |

The rewritten library and the rewritten application are each a little under
half the length of the original. The labels fall from 152 to 25 and the routes
from 164 to 7, because an alternative that continues needs no name in v2 and a
statement on the solid rail needs no label. The fifteen filler arms are gone.
In their place are five arms marked `⊥`, each of which the checker has to
prove, and eight proof lines in the library.

### 3.2 By file

| File | Version | Lines | Decisions | Labels | Routes | Origin marks | Borrow and move marks | Clone | Mutable | Types at bindings | Type arguments | Proof lines | `⊥` | `uses` |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| membership | v1 | 6 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 0 |
| membership | v2 | 3 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| search | v1 | 56 | 6 | 6 | 6 | 2 | 0 | 0 | 0 | 7 | 0 | 0 | 0 | 0 |
| search | v2 | 25 | 2 | 0 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| reduction | v1 | 33 | 3 | 3 | 3 | 0 | 0 | 0 | 0 | 7 | 0 | 0 | 0 | 0 |
| reduction | v2 | 24 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | 0 | 0 |
| partition | v1 | 45 | 4 | 4 | 5 | 0 | 3 | 0 | 2 | 6 | 0 | 0 | 0 | 0 |
| partition | v2 | 28 | 2 | 2 | 2 | 0 | 0 | 0 | 2 | 3 | 0 | 0 | 0 | 0 |
| sequence_batches | v1 | 47 | 7 | 7 | 7 | 0 | 4 | 0 | 2 | 9 | 1 | 0 | 0 | 0 |
| sequence_batches | v2 | 26 | 2 | 0 | 0 | 0 | 0 | 0 | 2 | 5 | 0 | 2 | 1 | 0 |
| traversal | v1 | 45 | 6 | 4 | 4 | 7 | 0 | 0 | 0 | 5 | 0 | 0 | 0 | 0 |
| traversal | v2 | 18 | 0 | 0 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 |
| ordering | v1 | 161 | 28 | 30 | 33 | 0 | 12 | 0 | 6 | 37 | 12 | 0 | 0 | 0 |
| ordering | v2 | 72 | 17 | 2 | 3 | 0 | 0 | 0 | 6 | 11 | 0 | 0 | 2 | 0 |
| ordered_search | v1 | 83 | 12 | 12 | 14 | 1 | 0 | 0 | 0 | 10 | 3 | 0 | 0 | 0 |
| ordered_search | v2 | 35 | 4 | 3 | 2 | 1 | 0 | 0 | 0 | 4 | 0 | 4 | 0 | 0 |
| settings | v1 | 117 | 14 | 9 | 9 | 0 | 7 | 1 | 2 | 28 | 0 | 0 | 0 | 0 |
| settings | v2 | 50 | 9 | 0 | 0 | 0 | 0 | 1 | 2 | 0 | 0 | 0 | 0 | 4 |
| http | v1 | 221 | 30 | 43 | 42 | 0 | 26 | 2 | 2 | 40 | 0 | 0 | 0 | 0 |
| http | v2 | 114 | 17 | 0 | 0 | 0 | 0 | 2 | 2 | 1 | 0 | 0 | 1 | 1 |
| server | v1 | 242 | 17 | 24 | 29 | 0 | 10 | 3 | 1 | 39 | 0 | 0 | 0 | 0 |
| server | v2 | 98 | 12 | 9 | 0 | 0 | 6 | 2 | 1 | 2 | 0 | 0 | 1 | 1 |
| main | v1 | 55 | 3 | 10 | 12 | 0 | 8 | 0 | 0 | 9 | 0 | 0 | 0 | 0 |
| main | v2 | 42 | 3 | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| retry_client | v2 | 67 | 5 | 8 | 1 | 0 | 1 | 0 | 1 | 3 | 0 | 3 | 0 | 5 |
| parser | v2 | 93 | 16 | 4 | 6 | 0 | 0 | 2 | 0 | 4 | 0 | 13 | 0 | 0 |
| interpreter | v2 | 48 | 14 | 10 | 2 | 0 | 8 | 3 | 1 | 1 | 0 | 0 | 0 | 0 |

### 3.3 Filler arms

The fifteen filler arms of v1, and what stands in their place:

| v1 | Arm | Why it cannot occur | In v2 |
| --- | --- | --- | --- |
| `search.vn` 31, 56 | `overflow => exit missing` | The index is below the length | `∀` has no index to add to |
| `reduction.vn` 31 | `overflow => exit finished { updated }` | The same | `∀` |
| `sequence_batches.vn` 28 | `empty => exit split { taken, [] }` | The caller passes an index below the length | `empty → ⊥`, from a precondition and an invariant |
| `traversal.vn` 38 | `out_of_bounds => finished {}` | `range` made the window | An index proved from `⊢ bounded(view)` |
| `ordering.vn` 36, 44 | `empty => only_right`, `empty => only_left`, each with a label and a decision that never run | The first element was just read | `empty → ⊥`, from the test of the two lengths |
| `ordered_search.vn` 56, 60, 64, 78, 82, 86 | `zero_divisor`, `overflow`, `out_of_bounds`, `overflow`, `underflow`, `underflow`, each `=> exit position { start }` | The invariant of the search | Infix arithmetic and an index, proved from one invariant line |
| `http.vn` 166 | `replaced { string previous } => replaced` | The map was just made | `replaced → ⊥`, from `len(∅) = 0` |
| `server.vn` 97 | `open => completed {}` | The set is empty only after admission ended | The alternatives of the admission loop; `empty → ⊥` while the set is full |

Two arms of the interpreter name an alternative that its author believes
cannot occur and that the logic cannot prove away: `missing` and `inserted`
where `scoped` restores a binding. Both continue with nothing to do. They are
a limit of the fact logic (finding O7), not filler in the sense of v1, and
the plan's exit condition speaks of the library, which has none.

### 3.4 The two risks the plan asked to measure

**Annotation burden.** The corpus has 18 loops. Five have a written invariant:
the binary search, `split_prefix`, the two operator loops of the parser and
the retry loop. Thirteen need none, three of them because they are `∀`. The
library has 8 proof lines in 231, and the application has none. The parser
is the heavy case, 13 lines in 93: every rule states that it starts inside the
input and ends further on. These are the facts a reader of a parser has to
know in any language. Template inference for invariants (plan, section 6) is
not needed for this corpus.

**Second-class borrows.** One declaration changed: `traversal.window` is a
`view struct`, as the plan foresaw. Nothing else in the corpus stores a
borrow. The origin marks fall from 10 to 7. Two places are constrained by
borrow rules. `serve` clones `path` and `authority` for each connection
because a named boundary takes owned values, as in v1. And `scoped` in the
interpreter captures the outcome of the body so that it can restore the scope
on every alternative, which is the price of lending one map down a recursion.

## 4. Notes on the rewrites

| File | What changed in structure |
| --- | --- |
| `search` | The traits stay. `first` and `try_first` are `∀` loops. The reference program [`experiments/syntax_lab/rails/search.txt`](../../experiments/syntax_lab/rails/search.txt) takes callable parameters, a phase 5 form, and is shorter still. |
| `reduction` | The accumulator is a mutable local that each step takes and gives back. |
| `partition` | `left` and `right` are locals of the body. Only the rest of the input goes round the loop. |
| `sequence_batches` | `split_prefix` has a precondition, which `split_at` proves by its two tests. |
| `traversal` | `window` is a view. The function `bounded` says what `range` establishes and `get` relies on, since a type has no invariant. `get` takes the view by value. |
| `ordering` | The same six controls. The two dead branches of `merge` are gone. `reverse_runs` and `merge_pass` leave their loops and join once, after them. |
| `ordered_search` | `bound` is the reference program. `first_equal`, which the reference program omits, tests the insertion position and indexes. |
| `settings` | The reference program, with `remove` added. `validate_key` is a function. The three result declarations of v1 become one group, `storage_error`, and four contracts that include it. |
| `http` | `route` is a sequence of tests with nested blocks: 75 lines and 22 labels in v1, 25 lines and no label here. Each helper decides, exits on the failures, and continues on the solid rail. |
| `server` | `serve` is the reference program: the admission state of v1, a variant with five constructors, is the set of alternatives of the loop `admit`. The boundary `request` decides the exchange and the close one after the other where v1 captures both. `prepare` binds the media type once. |
| `main` | `set` and `remove`, which only pass a result on, have `■ auto`. `serve` names the server's outcome, which carries the exit statuses. |

A trait is implemented nowhere in the corpus. For completeness, the v1
example `examples/stable_ordering/main.vn` (25 lines) in v2:

```text
import "../../library/ordering.vn" as ordering

struct record {key: u32, identity: u32}
struct key_order {}

implement ordering.comparator for key_order
│ type item := record
│ control compare(rule: &key_order, a: &record, b: &record) ■ before │ equal │ after
│ │ ? a.key < b.key → ■ before
│ │ ? a.key > b.key → ■ after
│ │ ■ equal
│ ╵
╵

control main(source: [record]) ■ sorted {values: [record]}
│ rule := key_order {}
│ forward ordering.sort(source, rule)
╵
```

## 5. What the new programs assume

The three programs use forms that the phase 1 documents do not specify. Each
is listed with the least that the program needs of it.

| Form | Used by | Assumed |
| --- | --- | --- |
| Recursion | parser, interpreter | A control may call itself, directly or through others. A call is checked against the callee's contract, as every call is, so the contract is the summary. Nothing proves that the recursion ends. |
| `box<τ>` | parser, interpreter | A box owns one `τ`. `box(e)` makes one. A place of type `box<τ>` is read, lent and decided as the `τ` inside. A variant may contain a box of itself. |
| Equality on variants and structs | parser | `=` and `≠` compare two values of a type whose components can all be compared. The fact logic already treats them as equality of terms ([A5]). |
| Text interpolation | settings, retry client | `"…{e}…"` is a string built from the text and the value of `e`, an integer or a string. A brace that is text is written `\{`. |
| Limits of a type | interpreter | `i64.min`, as the fact logic writes `usize.max`. |

Apart from text interpolation, the retrying client needs nothing beyond the
documents. It is checked by the
task rules of the core's draft 2 ([TK4] at each `await`, with the connection
declared `on fault release`), by the fact logic for its two counters, and by
the ordinary resource rule for the connection that every attempt closes.

## 6. Findings

A finding is a construct that a program needed and the documents did not
give, or gave wrongly. **F** findings are repaired in the documents by the
same commit. **O** findings are open: each is a limit that the corpus could
work within, with the cost stated.

### 6.1 Repaired

| | Finding | Repair |
| --- | --- | --- |
| F1 | Five of the eight library modules declare a trait, and the surface had no form for one. | [G11], [Y11], [U20] of the surface: `trait`, `implement … for …`, drawn on a rail. The rules are v1's. |
| F2 | Under the first draft's rule the generic library carried a move mark on every move: 76 in 231 lines. | The mark is asked only where the type is known to hold an obligation ([U11]). This is question Q19. |
| F3 | A value that holds an obligation could not be marked in an assignment, a literal, a constructor or an adapter's payload. | [G7a] and the grammar accept `▾ place` there. |
| F4 | `■ result`, for a captured outcome, reads as an exit with an alternative named `result`, and moves a value of kind `must` with no mark. | `■ ▾result` ([U14]). The reference program `server.txt` has the older spelling. |
| F5 | The logic did not know that a new vector is empty, so no invariant over a vector that a control builds could be proved at entry. | `len([t₁, …, tₙ]) = n` and `len(∅) = 0` ([A3] of the fact logic), an instance for every literal in a query ([J3]). |
| F6 | The server isolates a fault in one connection and carries on. The core described only the lexical boundary, whose contingency cannot let the control continue. | [E-Supervise] and the checking rule [F6] in the core, [G12] and [Y12] in the surface: the named boundary of v1, with `supervise`, `terminate` and `contingent`. |
| F7 | [W12] let only an extern control list `cancelled`. The library wraps a cancellable host operation in a defined control, which must pass the alternative on. | [W12] relaxed: in a defined control `cancelled` is an alternative like any other. |
| F8 | The server awaits a cancellable operation through `run` inside a member. No rule cancelled the root of that run. | [E-Cancel]: the root of a run that a cancelled task starts or waits for is cancelled. |
| F9 | A contract could not name an outcome or a group of another module. | `QName` in `Contract` and in `…group`. |
| F10 | A type could not be declared `copy`, which [T1] presupposes. | `copy struct`, `copy variant` ([G4b]). |
| F11 | An awaited operation with one alternative had to be written as a decision. | `await f(…)` and `run f(…)` as call statements. |
| F12 | The method form did not say where it looks, so `values.entry_at(position)` inside the module control `entry_at` was ambiguous. | [U12]: built-in operations and the controls of bounding traits; a module's control is called by name. |
| F13 | A contract without conditions could not be broken over lines. | [G1]. |

### 6.2 Open

| | Finding | Cost in the corpus | Suggested |
| --- | --- | --- | --- |
| O1 | A vector cannot be consumed element by element except with `take` and a loop parameter. | Six loops of the library | The iteration work of phase 5: a `∀` that takes each element by value |
| O2 | The result of an operation with one alternative is received by a decision, `Δ join(a, b): joined {all}`. | Seven decisions in the library | Value-position decisions, phase 5 |
| O3 | An expression cannot branch, so a function cannot either. | The next delay of the retry client is a mutable local and a test | The same phase 5 item |
| O4 | An arm's target is a path, a terminator or a block. `forward`, an adapter and a single call are none of these. | Seven labels that hold one statement | Accept `forward`, an adapter and a call as a target |
| O5 | An empty vector of elements that may hold obligations cannot be dropped. | `reverse_runs` and `merge_pass` join an empty vector to dispose of it, as in v1 | A built-in operation with the precondition `len(values) = 0` |
| O6 | A variant has no groups, so a decision on it names every constructor. | The parser's token type is nested to keep its decisions to four arms | Accept nesting as the way to group constructors, or let a variant declare groups |
| O7 | The contents of a map are outside the logic. | Two arms of the interpreter that cannot occur and cannot be proved away | None for v2.0: the plan puts heap predicates out of scope |
| O8 | Effects are listed in full on every public control, and a trait fixes the effects of its controls. | `server.start` lists seven. No implementation of a library trait may perform an effect. | Let a prefix stand for its members (`uses net`); effect parameters only if a real program needs them |
| O9 | An outcome declaration cannot include a group. | `main` uses `■ auto` where v1 named `settings.update_result` | Allow `…group` among the alternatives of an outcome |
| O10 | A lexical contingency cannot end and let its control continue; the named boundary can. | The server needs the named form, and with it owned copies of two strings for each connection | Keep both forms as v1 has them, or let a lexical contingency end with `terminate` |
| O11 | `run` inside a member of a task set does not pass the turn, as in v1. | While one connection exchanges, the others wait | Await `exchange` inside a lexical boundary, which needs O10 |
| O12 | There are no constants at module level. | `prepare` binds its media type as a local | A `function` without parameters serves; nothing to add |
| O13 | Text interpolation, equality on variants, the limits of a type, recursion and `box` are used and not specified (section 5). | Three programs | Phase 5 and the text library |

## 7. Choices and questions

Choices of this draft, recorded for the author's acceptance of phase 1:

1. **A rewrite keeps the public interface and the algorithm** and changes the
   inside of a control freely. The counts then measure the language and not a
   redesign of the programs.
2. **The library keeps its traits.** Callable parameters would shorten
   `search`, and they belong to phase 5.
3. **Built-in operations state the conditions the proofs need**
   ([`prelude.txt`](corpus/prelude.txt)): how `push`, `pop`, `take` and
   `insert` change a length. They are part of the trusted base ([T5]).
4. **An alternative that cannot occur is proved away or left to continue.** No
   arm of the corpus has a disposition that is believed never to run.
5. **The repairs F1 to F13 are in the documents**, since each was a gap and
   not a decision. F2 changes a rule of the first surface draft and is put to
   the author as Q19.

One question for the author, with a recommendation. The corpus and the
surface are written as if it were taken.

| | Question | Recommendation |
| --- | --- | --- |
| Q19 | Is the move mark `▾` written where a value of a parameter type moves? | **No.** A type parameter has kind `must` ([T1a]), so marking every such move puts a mark on every third line of the library and tells the reader nothing: in generic code every move may hand over an obligation. The mark is written where the type is known to hold one (a resource, a captured outcome, an aggregate with either), and the call that instantiates the generic code carries it. |

## 8. How far this has been checked

Each program was traced by hand against the rules that decide it: the
acknowledgement of every alternative ([C2], [U7]), ownership at every
terminator ([O5], [O6]), the loans at every exclusive use ([L3]), the proof
obligations ([P2] to [P6]) and, for the server and the client, the task rules
([TK2] to [TK5]). [`count.py`](corpus/count.py) checks the line width and
makes the counts. The rails were checked by eye and by a throwaway script
that is not kept. No parser, checker or interpreter exists yet, so an error
of the kind a compiler finds may remain. Phase 2 runs the whole corpus
through the oracle, which is the first mechanical check these programs get.

One independent review read the five phase 1 documents together, and every
program here against the rules. It reproduced the counts and found no missing
acknowledgement and no ownership, loan or `⊥` error in the programs. What it
found that touches the corpus:

| Finding | What was done |
| --- | --- |
| Two proofs rested on an axiom that the fact logic stated and never used: the entry invariant of `split_prefix`, and the `⊥` in `encode_value`. | The length of a literal is now an instance of every query that has one ([J3] of the fact logic). |
| The boundary `request` had no checking rule, and nothing said whether its body is `async`. | [F6] and [W10] of the core. The body is not `async`, which is why it uses `run`. `request` was checked again against [F6]: `connection` is available where `exchange` may fault, and is consumed after it. |
| The parser writes `op.add` and `punctuation.let_word` without braces, which the grammar did not derive. So were a label in a loop and a decision on a Boolean expression. | The grammar of the surface, section 4. |
| Four programs that the task rules accepted and the semantics left stuck or with a lost resource. No corpus program is one of them. | [TK3] to [TK6], [E-Await], [E-AwaitHost] and [E-Stop] of the core. The server and the retry client were checked again against the new [TK4]: every value given to an awaited operation, and every payload that one returns, is a text, a number or a connection declared `on fault release`. |
| The contracts with conditions in `prelude.txt` put two entries on a line, against [G1]. | One entry on a line. |
| `route` declares `faults`, and nothing in the application faults, so the contingency of `request` cannot run. | Left as it is, and not an error: a permission need not be used, and v1's `route` declares the same. It is why `exchange` and `prepare` say `faults`. |

`cancel active` in `serve` is kept from v1 although it changes nothing where
it stands: a member is cancelled when the task that owns its set is
([E-Cancel]), and `serve` reaches the statement only when it is cancelled
itself. The preparation of the reading test found this, not the review.
