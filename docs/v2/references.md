# Villanelle v2 references: borrows and views

Status: **draft 1**, 19 November 2026. Phase 1, part of deliverable 2 of the
[redesign plan](../redesign-plan.md) (its section 5.3). Not yet accepted by the
author; it becomes normative when it is. Nothing here is implemented. This
draft has not been reviewed independently.

[Index of the v2 design](README.md) · [Core calculus](core.md) ·
[Fact logic](facts.md) · [Surface](surface.md)

## 1. Purpose and scope

The rules for borrows belong to the [core calculus](core.md). The tag
semantics of its section 6.3 says when a borrow may be used, and the loan rules
of its section 7.5 are how the checker guarantees it. This document adds no
rule of its own. It gives:

- **the model** as a programmer holds it (section 2), each statement with the
  clauses of the core that make it true;
- **how a borrow is written** (section 3);
- **worked examples** (sections 4 to 8), each in Rails and in core form, with a
  verdict and the clause that decides it;
- **what is rejected**, and what to write instead (section 9);
- **the tag semantics** traced on one example (section 10);
- what the fact logic knows about a lent place (section 11), and what a task
  may borrow (section 12).

Where this text and the core differ, the core governs and the difference is a
defect here. The verdicts are binding in one sense: every example is a
conformance test, and an implementation that decides one differently is wrong
or the core is.

Writing the examples showed places where the core and the surface had left
something unsaid, and one case that calls for a rule. Section 13 lists them;
they have been applied to those documents. Two questions for the author are in
section 14.

Draft 1 covers the synchronous language and, in section 12, tasks as the
core's draft 2 defines them. Statements of the model carry
identifiers of the family **RM**. A verdict is marked **Accepted** or
**Rejected**.

## 2. The model

Every value has one owner, the name it is bound to. A **borrow** is permission
to use a place that another name owns, without taking it. The place is
**lent**. The record of the lending is a **loan**, and the names that may carry
the borrow, or a borrow derived from it, are the loan's **holders**.

- **[RM1] Two modes.** `&τ` is a shared borrow: it reads. `&~τ` is an exclusive
  borrow: it reads and writes. A shared borrow is of kind `copy`. An exclusive
  borrow is never copied: it is in one name at a time (core 4.1).
- **[RM2] Second-class.** A borrow lives in a parameter, a local name, a loop
  parameter, a payload field, or a field of a view. It lives nowhere else: not
  in a struct, a variant, a vector, a map or a captured outcome, not as a type
  argument, and not behind another borrow ([W6], [T6]).
- **[RM3] Four sources.** A borrow is made by lending a place; by an operation
  whose contract says with `∈` which parameters a payload field comes from; by
  deciding a variant through a borrow, which yields borrows of its fields; and
  from another borrow, copied, handed on, or lent again in part. Nothing else
  makes one ([L1], [T4], [E-Outcome]).
- **[RM4] A borrow name is the place.** Where a value is wanted, `r` is the
  value behind the borrow: `r.x`, `r[i]`, `len(r)`. `r ← e` writes it. The name
  borrows one place for as long as it lives and is never made to refer to
  another ([W8]).
- **[RM5] A loan lasts until its last use.** It is live from the lending to the
  last statement that mentions one of its holders, on the histories that run
  those statements. The end of a block or of a scope does not matter ([L2]).
- **[RM6] One rule while a loan is live.** A place lent shared may be read by
  anyone and written by no one. A place lent exclusively may be touched only
  through the loan's holders. Writing includes assigning, moving, lending
  exclusively, changing a collection's shape, and the end of the owner's scope
  ([L3], [B5]).
- **[RM7] Overlap.** Two places overlap when one is part of the other. Two
  fields of one struct do not overlap. All elements of one collection overlap,
  because the checker does not compare indices (core 7.5).
- **[RM8] Exclusive needs mutable.** A place is lent exclusively, assigned, or
  changed in shape only if its root was bound with `~` or it is reached through
  an exclusive borrow ([C1]).
- **[RM9] Leaving.** A borrow leaves a control only in a payload field whose
  `∈` names the parameters it came from. It leaves a loop, or goes round one,
  only if what it borrows outlives the iteration ([L4]).
- **[RM10] Loops.** A borrow that goes from one iteration to the next is a loop
  parameter. It is treated as holding its loans through the whole body
  ([L2a]).
- **[RM11] Views.** A view is a value made of borrows and `copy` values. It is
  used as one borrow: shared if all its borrows are shared, exclusive otherwise
  ([W6], [L5]).
- **[RM12] Nothing leaves through a borrow.** A value of kind `drop` or `must`
  is never moved out from behind a borrow, and a value of kind `must` is never
  overwritten through one ([O3], [O4]).

Three things that other languages with borrows have are absent. There are no
lifetime names: the only annotation is `∈` on a payload field. There are no
stored borrows, so no data structure holds a borrow into another. And a borrow
name is not re-pointed.

A loan is conditional. Its liveness is a formula over the histories of the
body, like every other fact the checker keeps (core 7.1), so a loan that
exists on one path does not constrain another (example 6.3).

A consequence of [RM6] at calls: inside a control, an `&~` parameter overlaps
no other parameter, because a caller that passed overlapping arguments was
rejected at its call.

## 3. Writing a borrow

A borrow is never written as an expression of its own. A place becomes a borrow
where the slot it fills has a borrow type ([W8], surface [U11]):

| The slot expects | Written | Core operand |
| --- | --- | --- |
| `&τ` | a place `p` | `&p` |
| `&τ` | an expression that is not a place | The value is bound to a hidden name, which is lent |
| `&τ` | a name `r` of type `&τ` | `r`: a copy of the borrow |
| `&~τ` | `~p` | `&~p` |
| `&~τ` | `~r`, for a name `r` of type `&~τ` | `&~r`: what is behind `r` is lent again |
| a value | `e` | `e` |

The slots with a declared type are a parameter of a call, a bind with a written
type, a loop parameter (at the loop statement and at each `↑`), a payload field
of `■`, and a field of a view. A bind without a written type takes a value, so
`x := r` reads through `r`. The receiver of the method form is lent as the
operation declares, with no mark (surface [U12]).

`~` is the mark of write permission. It appears where a name is bound mutable
(`~t: tally := …`) and where a place is lent exclusively (`bump(~t.count)`).

A payload field of a contract is read `value: &τ ∈ m`: a borrow of a `τ`, from
`m`. The names after `∈` are borrow or view parameters. The caller then knows
that the payload holds every loan the arguments for those parameters hold, and
no other.

The examples use these types and operations. The operations stand for
built-in and library operations with the contracts shown; their names follow
the Rails reference programs.

```text
struct tally {count: u64, note: string}
struct pair {left: [u32], right: [u32]}

variant shape
├ circle {radius: u32}
└ rect {width: u32, height: u32}

emit(text: &string)                                ■ done
get(map: &k ⇀ v, key: &k)                          ■ found {value: &v ∈ map} │ missing
insert(map: &~k ⇀ v, key: k, value: v)             ■ inserted │ replaced
push(values: &~[t], value: t)                      ■ done
```

## 4. Shared borrows

### 4.1 A control that returns one of its arguments

```text
control widest(a: &string, b: &string) ■ pick {text: &string ∈ a, b}
│ ? len(a) ≥ len(b) → ■ pick {a}
│ ■ pick {b}
╵
```

```text
1  ⊤       ▷ t : ? len(a) ≥ len(b)
2  t.true  ▷ ■ pick {a}
3  ⊤       ▷ ■ pick {b}
```

**Accepted.** In a payload field of borrow type, `a` stands for the borrow
itself ([W8]), and a shared borrow is copied. `len(a)` reads the value behind
`a`. Each exit hands out a borrow that came from a parameter the field names
([L4]).

### 4.2 The caller, and when the loan ends

```text
control announce() ■ done
│ ~left: string := "alder"
│ right: string := "birch"
│ Δ widest(left, right): pick {text}
│ emit(text)
│ emit(left)
│ left ← "cedar"
│ ■ done
╵
```

```text
1  ⊤ ▷ ~left : string := "alder"
2  ⊤ ▷ right : string := "birch"
3  ⊤ ▷ widest : Δ widest(&left, &right) ▸ pick {text}
4  ⊤ ▷ emit₁ : Δ emit(text) ▸ done {}
5  ⊤ ▷ emit₂ : Δ emit(&left) ▸ done {}
6  ⊤ ▷ left ← "cedar"
7  ⊤ ▷ ■ done {}
```

Statement 3 lends both names. The contract derives `text` from both
parameters, so `text` holds both loans ([L1]):

| Place | Mode | On | Holders | Last use of a holder |
| --- | --- | --- | --- | --- |
| `left` | shared | `⊤` | `text` | 4 |
| `right` | shared | `⊤` | `text` | 4 |

**Accepted.** Statement 5 reads `left` while it is lent shared, which [L3]
allows. Statement 6 writes `left`, and by then no statement mentions `text`
again, so the loan is not live ([L2]). At run time `text` borrows only one of
the two strings. The caller cannot know which, so both stay lent until the
last use of `text`: the contract is what the caller reasons from.

### 4.3 Writing the lender too early

The same control with statements 4 and 6 exchanged:

```text
│ Δ widest(left, right): pick {text}
│ left ← "cedar"
│ emit(text)
```

**Rejected** at the assignment, by [L3]. The write to `left` does not go
through the loan, and the loan is live after it because `emit(text)` follows.
The obligation `Θ ⊨ ¬(R ∧ L)` fails with `R = L = ⊤`.

A report of such a conflict can always name four things, because [L3] is made
of them: where the place was lent, which name holds the loan, the access that
conflicts, and a later use of the holder, all on one feasible history.

```text
`left` is assigned while it is lent
    lent, shared      Δ widest(left, right): pick {text}
    the loan is held  by text
    assigned          left ← "cedar"
    used afterwards   emit(text)
    on the history    widest.pick
```

The wording is an illustration and not part of this document.

### 4.4 Reading through a borrow

```text
control describe(item: &tally) ■ seen {count: u64, note: string}
│ count: u64 := item.count
│ note: string := clone(item.note)
│ ■ seen {count, note}
╵
```

```text
1  ⊤ ▷ count : u64 := item.count
2  ⊤ ▷ note : string := clone(item.note)
3  ⊤ ▷ ■ seen {count, note}
```

**Accepted.** `item.count` is a `u64` read from behind the borrow and copied.
`clone` reads its place and builds a new string.

**Rejected**, by [O3], with `note: string := item.note` at statement 2: the bind
would keep a `string` that is behind a borrow, and nothing of kind `drop`
moves out through one ([RM12]). A second name for the same borrow is written
with its type, `again: &tally := item`, which copies the borrow ([W8]).

A function takes shared borrows too, as `valid_key(key: &string)` does in the
reference programs. It reads the place where it is, makes no borrow value, and
cannot return one ([W5a]). The read is still an access: calling a function on a
place that is lent exclusively to someone else is a conflict.

## 5. Exclusive borrows

### 5.1 Lending to an operation

```text
control bump(counter: &~u64) ■ done
│ ? counter < 1000 → counter ← counter + 1
│ ■ done
╵

control count_twice() ■ done
│ ~t: tally := tally {0, "start"}
│ bump(~t.count)
│ bump(~t.count)
│ emit(t.note)
│ ■ done
╵
```

```text
bump         1  ⊤       ▷ z : ? counter < 1000
             2  z.true  ▷ counter ← counter + 1
             3  ⊤       ▷ ■ done {}

count_twice  1  ⊤ ▷ ~t : tally := tally {0, "start"}
             2  ⊤ ▷ bump₁ : Δ bump(&~t.count) ▸ done {}
             3  ⊤ ▷ bump₂ : Δ bump(&~t.count) ▸ done {}
             4  ⊤ ▷ emit : Δ emit(&t.note) ▸ done {}
             5  ⊤ ▷ ■ done {}
```

**Accepted.** In `bump`, the assignment writes through the borrow ([W8]), and
the sum is in range by the test ([P5]). In `count_twice`, each loan is held by
the operation alone, because the contract of `bump` returns no borrow. It ends
when the call completes, so the place can be lent again at once.

### 5.2 A place that is not mutable

**Rejected**, by [C1], when statement 1 of `count_twice` binds `t` without `~`:
`&~t.count` needs a mutable place. The same holds when `t` is a parameter of
type `&tally`: a place reached through a shared borrow is not mutable.

### 5.3 A named borrow of one field

```text
control restart(item: &~tally) ■ done
│ slot: &~u64 := ~item.count
│ slot ← 0
│ item.note ← "restarted"
│ bump(~slot)
│ ■ done
╵
```

```text
1  ⊤ ▷ slot : &~u64 := &~item.count
2  ⊤ ▷ slot ← 0
3  ⊤ ▷ item.note ← "restarted"
4  ⊤ ▷ bump : Δ bump(&~slot) ▸ done {}
5  ⊤ ▷ ■ done {}
```

Statement 1 lends a place behind the borrow `item`. The loan is on
`*item.count`, the place behind `item`, and `slot` holds it ([L1]).

**Accepted.** Statement 3 writes `*item.note`, which does not overlap
`*item.count` ([RM7]). Statement 4 lends what is behind `slot` again, to the
operation; afterwards `slot` could still be used.

### 5.4 Writing the lent field

**Rejected**, by [L3], with `item.count ← 5` inserted before statement 4. The
write reaches `*item.count` through `item`, which is not a holder of the loan,
and `slot` is used afterwards. Section 10 runs this program in the
instrumented semantics.

### 5.5 Two borrows from one exclusive parameter

```text
control halves(both: &~pair) ■ split {left: &~[u32] ∈ both, right: &~[u32] ∈ both}
│ ■ split {~both.left, ~both.right}
╵

control fill() ■ done
│ ~p: pair := pair {[1, 2], [3]}
│ Δ halves(~p): split {left, right}
│ left.push(4)
│ right.push(5)
│ left.push(6)
│ ■ done
╵
```

```text
halves  1  ⊤ ▷ ■ split {&~both.left, &~both.right}

fill    1  ⊤ ▷ ~p : pair := pair {[1, 2], [3]}
        2  ⊤ ▷ halves : Δ halves(&~p) ▸ split {left, right}
        3  ⊤ ▷ push₁ : Δ push(&~left, 4) ▸ done {}
        4  ⊤ ▷ push₂ : Δ push(&~right, 5) ▸ done {}
        5  ⊤ ▷ push₃ : Δ push(&~left, 6) ▸ done {}
        6  ⊤ ▷ ■ done {}
```

**Accepted.** In `halves`, the exit uses both payload borrows at once, so the
second lending must not conflict with the first: `*both.left` and
`*both.right` do not overlap ([L3]). Both are places behind `both`, which the
fields name ([L4]). In `fill`, one loan on `p` has two holders, `left` and
`right`. Until the last use of either, statement 5, `p` can be touched only
through them.

### 5.6 The same field twice

**Rejected**, by [L3], when `halves` exits with
`■ split {~both.left, ~both.left}`: the second lending writes a place that
the first, still to be used by the exit, has lent exclusively. This check in
the callee is what lets the caller use `left` and `right` together. For an
extern control it is an assumption about the host ([E-Host]).

### 5.7 A value and a loan in one call

```text
│ ~values: [usize] := [1, 2]
│ values.push(len(values))
```

```text
1  ⊤ ▷ ~values : [usize] := [1, 2]
2  ⊤ ▷ push : Δ push(&~values, len(values)) ▸ done {}
```

**Accepted**, because the operands of a command that do not lend are evaluated
before those that do ([E-Operands]): `len(values)` is read first, and only
then is `values` lent. In strict left-to-right order the read would come after
the exclusive lending that the operation is about to use. [L3] would reject
the program, and the instrumented semantics would be stuck on it: the read
kills the tag the operation needs. Question Q17 asks the author to confirm the
order.

### 5.8 Two elements of one vector

```text
control swap_first_two(grid: &~[u32]) ⊢ len(grid) ≥ 2 ■ done
│ first: u32 := grid[0]
│ grid[0] ← grid[1]
│ grid[1] ← first
│ ■ done
╵
```

```text
1  ⊤ ▷ first : u32 := grid[0]
2  ⊤ ▷ grid[0] ← grid[1]
3  ⊤ ▷ grid[1] ← first
4  ⊤ ▷ ■ done {}
```

**Accepted.** Each access is over before the next begins. The indices are in
range by the precondition, and a write to an element keeps the length
([P5], fact logic [A6]).

### 5.9 Two element borrows at once

```text
│ a: &~u32 := ~grid[0]
│ b: &~u32 := ~grid[1]
│ a ← b
```

**Rejected** at the second bind, by [L3]: all elements of one collection
overlap ([RM7]), and `a` is used afterwards. The instrumented semantics would
not be stuck here, since the two places differ. The checker is stricter on
purpose (section 10).

## 6. Borrows that come back from an operation

### 6.1 A lookup

```text
control greeting(settings: &string ⇀ string) ■ text {value: string}
│ Δ settings.get("greeting"): found {value} │ missing
╞═ found:   ■ text {clone(value)}
╞═ missing: ■ text {"hello"}
╵
```

```text
1  ⊤            ▷ k : string := "greeting"
2  ⊤            ▷ get : Δ get(settings, &k) ▸ found {value} │ missing {}
3  get.found    ▷ ■ text {clone(value)}
4  get.missing  ▷ ■ text {"hello"}
```

**Accepted.** The literal is not a place, so it is bound to the hidden name `k`
and lent. `value` has the type `&string` and comes from `map` alone, so it
holds the loans that `settings` holds and none on `k`. The loan on `k` was the
operation's and is over.

There is no `option<&string>`: a borrow is not a type argument ([RM2]). The
outcome is the option. An operation that may find something returns
`found {value: &τ ∈ m} │ missing`, and the caller names both.

### 6.2 Keeping what was found

**Rejected**, by [O3], with `╞═ found: ■ text {value}`. The payload field is a
`string`, so `value` stands for the string behind the borrow, and that string
belongs to the map ([RM12]). The control either clones, as above, or returns
the borrow: `■ text {value: &string ∈ settings}`.

### 6.3 A loan on one path only

```text
control touch(settings: &~string ⇀ string, key: &string) ■ kept {length: usize} │ added
│ Δ settings.get(key): found {value} │ missing
│
╞═ missing
│ ┆ Δ settings.insert(clone(key), ""): inserted │ replaced
│ ┆ ■ added
╞═ found: ■ kept {len(value)}
╵
```

```text
1  ⊤            ▷ get : Δ get(&settings, key) ▸ found {value} │ missing {}
2  get.missing  ▷ insert : Δ insert(&~settings, clone(key), "") ▸ inserted {} │ replaced {}
3  get.missing  ▷ ■ added {}
4  get.found    ▷ ■ kept {len(value)}
```

Statement 1 lends what is behind `settings`, shared, and `value` holds the
loan. `value` is mentioned again at statement 4, which runs on `get.found`. The
loan is therefore live after statement 2 on `get.found` and on no other
history.

**Accepted.** Statement 2 writes `*settings` on `get.missing`. [L3] asks for
`Θ ⊨ ¬(get.missing ∧ get.found)`, which holds because one site has one
alternative ([K4]). A checker that tied a loan to a stretch of program text
would see the map borrowed across the insertion. Here the loan belongs to the
histories on which its holder exists.

### 6.4 The same write on the path that holds the loan

```text
╞═ found
│ ┆ Δ settings.insert(clone(key), clone(value)): inserted │ replaced
│ ┆ ■ kept {len(value)}
```

**Rejected** at the insertion, by [L3]: it writes `*settings` on `get.found`,
and `value` is used by the exit that follows. Taking what is needed first is
accepted, because the last use of `value` then comes before the write:

```text
╞═ found
│ ┆ length: usize := len(value)
│ ┆ Δ settings.insert(clone(key), clone(value)): inserted │ replaced
│ ┆ ■ kept {length}
```

In this version `clone(value)` is evaluated before `settings` is lent
([E-Operands], example 5.7), and nothing mentions `value` afterwards.

### 6.5 Deciding a variant through a shared borrow

```text
control longest_side(figure: &shape) ■ side {length: u32}
│ Δ figure: circle {radius} → ■ side {radius} │ rect {width, height}
│ ? width ≥ height → ■ side {width}
│ ■ side {height}
╵
```

```text
1  ⊤              ▷ figure : Δ shape(figure) ▸ circle {radius} │ rect {width, height}
2  figure.circle  ▷ ■ side {radius}
3  ⊤              ▷ w : ? width ≥ height
4  w.true         ▷ ■ side {width}
5  ⊤              ▷ ■ side {height}
```

**Accepted.** A decision on a variant through a borrow completes with borrows
of the fields, in the mode of that borrow ([E-Outcome]). `radius`, `width` and
`height` have the type `&u32` and hold the loans of `figure` ([L1]). The
payload field `length` is a `u32`, so each name is read through and copied.
The site is named after the place it decides (surface [U1]).

### 6.6 Through an exclusive borrow

```text
control make_square(figure: &~shape) ■ done
│ Δ ~figure: circle │ rect {width, height}
╞═ rect: height ← width
│ ■ done
╵
```

```text
1  ⊤            ▷ figure : Δ shape(&~figure) ▸ circle {r} │ rect {width, height}
2  figure.rect  ▷ height ← width
3  ⊤            ▷ ■ done {}
```

**Accepted.** `width` and `height` are exclusive borrows of two fields. They
hold one loan on what is behind `figure`, and an access through either goes
through it. The arm `circle` names no field, so elaboration gives the payload
the hidden name `r` (surface [U8]).

### 6.7 Replacing the variant under its fields

```text
╞═ rect
│ ┆ figure ← shape.circle {1}
│ ┆ height ← 0
```

**Rejected** at the first assignment, by [L3]. It writes `*figure` through
`figure`, which is not a holder of the loan that `height` holds, and `height`
is used afterwards. This is the case the rule exists for: after the assignment
the value is a circle and has no `height`.

### 6.8 A borrow of a local cannot leave

```text
control first_or_default(words: &[string]) ■ pick {word: &string ∈ words}
│ fallback: string := "none"
│ ? len(words) = 0 → ■ pick {fallback}
│ ■ pick {words[0]}
╵
```

```text
1  ⊤       ▷ fallback : string := "none"
2  ⊤       ▷ z : ? len(words) = 0
3  z.true  ▷ ■ pick {&fallback}
4  ⊤       ▷ ■ pick {&words[0]}
```

**Rejected** at statement 3, by [L4]: the payload would hold a loan on
`fallback`, a name of this control, and the field allows only what is behind
`words`. The exit ends the scope that owns `fallback`. Statement 4 alone is
accepted: its index is in range by the test, and `*words[0]` is behind
`words`.

Three ways to write the control: return a `string` and clone; add an
alternative `missing` and let the caller decide; or take the default from the
caller, `fallback: &string`, and write `∈ words, fallback`.

## 7. Loops

A loop body is checked once, for one generic iteration ([K6]). A borrow can
enter an iteration as a loop parameter and leave it through `↑`, `□` or `■`.

### 7.1 A borrow as loop state

```text
control longest(words: &[string]) ⊢ len(words) > 0 ■ found {word: &string ∈ words}
│ ↻ scan(next: usize := 1, best: &string := words[0])
│ ║ ⊢ next ≤ len(words)
│ ║ ? next = len(words) → ■ found {best}
│ ║ ? len(words[next]) > len(best) → ↑ scan(next + 1, words[next])
│ ║ ↑ scan(next + 1, best)
╵
```

```text
1  ⊤ ▷ scan : ↻ scan(next : usize := 1, best : &string := &words[0])
              ⊢ next ≤ len(words)
       1  ⊤       ▷ z : ? next = len(words)
       2  z.true  ▷ ■ found {best}
       3  ⊤       ▷ g : ? len(words[next]) > len(best)
       4  g.true  ▷ ↑ scan(next + 1, &words[next])
       5  ⊤       ▷ ↑ scan(next + 1, best)
```

**Accepted.** `best` cannot be re-pointed, so the choice of a new best is a new
iteration with a new `best` ([RM4], [RM10]). The loop parameter holds the loans
of its initial argument and of every `↑` argument, through the whole body
([L2a]): shared loans on the elements behind `words`. The body only reads. At
statement 2 the borrow leaves in a field that names `words` ([L4]). The
indices and the invariant are proved from the precondition, the test at 1 and
the invariant ([P2], [P5]).

### 7.2 A borrow of what the iteration owns

```text
│ ║ copy: string := clone(words[next])
│ ║ ? len(copy) > len(best) → ↑ scan(next + 1, copy)
```

**Rejected** at the `↑`, by [L3]. Its argument is `&copy`, and `↑` ends the
scope that owns `copy`, which is a write to it. The next iteration's `best`
would hold a borrow of a string that no longer exists ([L4]).

### 7.3 An exclusive element borrow across iterations

```text
control zero_smallest(values: &~[u32]) ⊢ len(values) > 0 ■ done
│ ↻ scan(next: usize := 1, low: &~u32 := ~values[0])
│ ║ ⊢ next ≤ len(values)
│ ║ ? next = len(values) →
│ ║ ┆ low ← 0
│ ║ ┆ ■ done
│ ║ ? values[next] < low → ↑ scan(next + 1, ~values[next])
│ ║ ↑ scan(next + 1, ~low)
╵
```

**Rejected**, by [L3], at the first read of `values` in the body, which is
`len(values)` in the test. Through the whole body `low` holds an exclusive
loan on the elements behind `values` ([L2a]), every element overlaps every
other ([RM7]), and the body mentions `low` again. While one element is lent
exclusively across iterations, the vector cannot even be measured.

### 7.4 The index instead

```text
control zero_smallest(values: &~[u32]) ⊢ len(values) > 0 ■ done
│ ↻ scan(next: usize := 1, low: usize := 0)
│ ║ ⊢ low < next ∧ next ≤ len(values)
│ ║ ? next = len(values) →
│ ║ ┆ values[low] ← 0
│ ║ ┆ ■ done
│ ║ ? values[next] < values[low] → ↑ scan(next + 1, next)
│ ║ ↑ scan(next + 1, low)
╵
```

```text
1  ⊤ ▷ scan : ↻ scan(next : usize := 1, low : usize := 0)
              ⊢ low < next ∧ next ≤ len(values)
       1  ⊤       ▷ z : ? next = len(values)
       2  z.true  ▷ values[low] ← 0
       3  z.true  ▷ ■ done {}
       4  ⊤       ▷ s : ? values[next] < values[low]
       5  s.true  ▷ ↑ scan(next + 1, next)
       6  ⊤       ▷ ↑ scan(next + 1, low)
```

**Accepted.** The loop carries a position and no borrow. Each access to the
vector is momentary, and the invariant is what makes `values[low]` an index in
range ([P5]). An index with a stated range is the idiom for a position that
must be remembered while the collection stays in use.

### 7.5 Leaving a loop with a borrow

```text
control report(words: &[string], prefix: &string) ■ done
│ ↻ scan(next: usize := 0)
│ ║ ⊢ next ≤ len(words)
│ ║ ? next = len(words) → □ none
│ ║ word: &string := words[next]
│ ║ ? word.starts_with(prefix) → □ hit {word}
│ ║ ↑ scan(next + 1)
│
╞═ hit:  emit(word)
╞═ none: emit("no word starts with it")
│ ■ done
╵
```

```text
1  ⊤          ▷ scan : ↻ scan(next : usize := 0) ⊢ next ≤ len(words)
                  1  ⊤       ▷ z : ? next = len(words)
                  2  z.true  ▷ □ none {}
                  3  ⊤       ▷ word₁ : &string := &words[next]
                  4  ⊤       ▷ m : ? starts_with(word₁, prefix)
                  5  m.true  ▷ □ hit {word₁}
                  6  ⊤       ▷ ↑ scan(next + 1)
                ▸ none {} │ hit {word}
2  scan.hit   ▷ emit₁ : Δ emit(word) ▸ done {}
3  scan.none  ▷ h : string := "no word starts with it"
4  scan.none  ▷ emit₂ : Δ emit(&h) ▸ done {}
5  ⊤          ▷ ■ done {}
```

**Accepted.** A payload field of a loop has the type of what leaves through
it, so `word₁` leaves as the borrow it is ([W9]). The name `word` of the
enclosing body then holds its loans, which are on places behind `words`. The
iteration owns nothing that is lent. `starts_with` stands for a function of
the text library. Elaboration renames the inner `word` (surface [U9]).

### 7.6 Leaving with a borrow of what the iteration owns

```text
│ ║ copy: string := clone(words[next])
│ ║ shown: &string := copy
│ ║ ? shown.starts_with(prefix) → □ hit {word: shown}
```

**Rejected** at the `□`, by [L3] and [L4]: leaving ends the scope that owns
`copy` while `word` would go on holding a loan on it. `□ hit {word: copy}`
is accepted. It moves the string out, and `word` then owns it.

### 7.7 For each

`∀ value ∈ source` lends each element in turn, shared: its expansion binds
`value := &source[i]` in every iteration (surface [U10]). The borrow may leave
in a payload, as in the control `first` of the surface document, section
10.2. Draft 1 has no form of `∀` that lends each element exclusively. A loop
over an index does it, as in example 7.4.

## 8. Views

A **view** is a value of a type declared `view struct`. Its fields are borrows,
views, and values of kind `copy` ([W6]). It is the one way to keep a borrow
together with other data, and it is second-class like the borrows it holds
([RM2]). A view whose borrows are all shared is of kind `copy`. One that holds
an exclusive borrow is of kind `drop` and moves.

### 8.1 A window over a vector

In v1, `library/traversal.vn` declares `window<item>['source]`, a struct with
a named origin, and its `get` returns a borrow `from view['source]`. In v2:

```text
pub view struct window<item> {values: &[item], start: usize, end: usize}

function bounded<item>(view: window<item>): bool :=
    view.start ≤ view.end ∧ view.end ≤ len(view.values)

pub control range<item>(values: &[item], start: usize, end: usize)
    ■ ready {view: window<item> ∈ values} ⇐ bounded(view)
    ■ invalid
│ ? start > end ∨ end > len(values) → ■ invalid
│ ■ ready {window {values, start, end}}
╵

pub control get<item>(view: window<item>, offset: usize) ⊢ bounded(view)
    ■ found {value: &item ∈ view} │ finished
│ ? offset ≥ view.end - view.start → ■ finished
│ ■ found {view.values[view.start + offset]}
╵
```

```text
range  1  ⊤       ▷ t : ? start > end ∨ end > len(values)
       2  t.true  ▷ ■ invalid {}
       3  ⊤       ▷ ■ ready {window {values, start, end}}

get    1  ⊤       ▷ t : ? offset ≥ view.end - view.start
       2  t.true  ▷ ■ finished {}
       3  ⊤       ▷ ■ found {&view.values[view.start + offset]}
```

**Accepted.**

- In `range`, the view is built from the borrow `values`, which stands for
  itself in a field of borrow type ([W8]). The view holds the loans its borrow
  holds ([L5]), and the payload field names `values` ([L4]). The condition
  `bounded(view)` is proved at the exit from the failed test ([P4]); the
  function is transparent to the logic (fact logic [I8]).
- In `get`, the view arrives by value. It is `copy`, and a borrow of a view
  does not exist ([W6]). `∈ view` says that the payload comes from the view:
  it holds what the view holds, which is the caller's loan on the vector.
  The subtraction, the sum and the index are in range by the precondition and
  the test ([P5]).

A type has no invariant of its own in draft 1. What a `window` promises is the
predicate `bounded`, stated where a window is made and required where one is
used.

### 8.2 A borrow that outlives the view it came through

```text
control second_of(names: &[string]) ■ found {name: &string ∈ names} │ missing
│ Δ range(names, 1, len(names)): ready {rest} │ invalid → ■ missing
│ Δ get(rest, 0): found {value} → ■ found {value} │ finished → ■ missing
╵
```

```text
1  ⊤              ▷ range : Δ range(names, 1, len(names)) ▸ ready {rest} │ invalid {}
2  range.invalid  ▷ ■ missing {}
3  ⊤              ▷ get : Δ get(rest, 0) ▸ found {value} │ finished {}
4  get.found      ▷ ■ found {value}
5  get.finished   ▷ ■ missing {}
```

**Accepted.** `rest` comes from `names`, and `value` comes from `rest`, so
`value` holds the loans of `names` and nothing of this control ([L1]). It
leaves at statement 4 although `rest` ends with the scope ([L4]). The
precondition of `get` at statement 3 is the condition that `range` stated for
`ready` ([P3]). No origin is named anywhere: v1's `['source]` is the `∈` of
the two contracts.

### 8.3 Changing the vector under a view

```text
control grow() ■ done
│ ~names: [string] := ["ash", "elm", "oak"]
│ Δ range(names, 0, 2): ready {front} │ invalid → ■ done
│ names.push("yew")
│ Δ get(front, 0): found {value} │ finished
│ ■ done
╵
```

**Rejected** at the `push`, by [L3]: `front` holds a shared loan on `names`
and is used by the next statement, and a change of shape is a write to the
whole vector ([B5]).

### 8.4 Declarations that are not allowed

```text
struct holder {front: window<u32>}                 a view inside a struct
view struct owner {values: [u32], start: usize}    a view field that owns a vector
control peek(view: &window<u32>) ■ done            a borrow of a view
names: [&string] := …                              borrows as elements
kept := capture settings.get(key)                  a captured outcome with a borrow
```

**Rejected**, the first four by [W6] and the last by [T6].

### 8.5 A view that holds an exclusive borrow

```text
view struct tail {values: &~[u32], from: usize}

control clear(rest: tail) ⊢ rest.from ≤ len(rest.values) ■ done
│ ↻ each(next: usize := rest.from)
│ ║ ⊢ next ≤ len(rest.values)
│ ║ ? next = len(rest.values) → ■ done
│ ║ rest.values[next] ← 0
│ ║ ↑ each(next + 1)
╵

control wipe() ■ done
│ ~numbers: [u32] := [4, 5, 6]
│ ? len(numbers) ≥ 2 → clear(tail {~numbers, 2})
│ ■ done
╵
```

```text
clear  1  ⊤ ▷ each : ↻ each(next : usize := rest.from) ⊢ next ≤ len(rest.values)
                 1  ⊤       ▷ z : ? next = len(rest.values)
                 2  z.true  ▷ ■ done {}
                 3  ⊤       ▷ rest.values[next] ← 0
                 4  ⊤       ▷ ↑ each(next + 1)

wipe   1  ⊤       ▷ ~numbers : [u32] := [4, 5, 6]
       2  ⊤       ▷ z : ? len(numbers) ≥ 2
       3  z.true  ▷ clear : Δ clear(tail {&~numbers, 2}) ▸ done {}
       4  ⊤       ▷ ■ done {}
```

**Accepted.** The assignment at statement 3 of `clear` writes a place reached
through an exclusive borrow, which is a mutable place although `rest` is a
parameter ([C1]). The view is used as one exclusive borrow ([L5]): it moves
into `clear`, and for as long as a view of this type is live the vector can be
touched only through it. To lend such a view again, a new one is built from
its fields, `tail {~rest.values, rest.from}`.

## 9. What is rejected, and what to write instead

| Attempt | Rejected by | Write instead |
| --- | --- | --- |
| Assign, move or reshape a place while a borrow of it is still to be used | [L3] | Finish with the borrow first; copy or clone what is needed from it |
| Read a place while it is lent exclusively and the borrow is still to be used | [L3] | Read through the borrow, or after its last use |
| Lend exclusively a place whose root is not bound with `~`, or that is reached through a shared borrow | [C1] | Bind the owner with `~`; take `&~` in the signature |
| Exit with a borrow of a name of the control | [L4] | Exit with the value; or let the caller own the value and lend it |
| Exit with a borrow from a parameter that the field's `∈` does not name | [L4] | Name the parameter after `∈` |
| Carry into the next iteration, or out of the loop, a borrow of a name the iteration owns | [L3], [L4] | Carry the value itself as loop state or payload; or own it outside the loop |
| Keep a borrow in a struct, a variant, a vector, a map, a captured outcome or a type argument | [W6], [T6] | A view, for a bundle that is used and dropped; an index or a key, for a relation that is stored; an outcome with a payload borrow, for `option<&τ>` |
| Make a borrow name refer to another place | [W8] | A loop parameter (7.1); one bind on each of two paths; a control that returns the chosen borrow (4.1) |
| Take a value of kind `drop` or `must` out from behind a borrow | [O3] | `clone`; an operation that removes or exchanges it; take the owner apart |
| Overwrite a value of kind `must` through a borrow | [O4] | An operation that completes the old value first |
| Borrow two elements of one collection exclusively at once | [L3] | One at a time, by value (5.8); a library operation whose contract returns disjoint parts |
| A borrow of a borrow or of a view | [W6] | Pass the borrow or the view itself |

One bind on each of two paths is the direct replacement for a borrow that
would be chosen by assignment:

```text
│ Δ len(a) ≥ len(b): true → first │ false → second
╞═ first:  pick: &string := a
╞═ second: pick: &string := b
│ emit(pick)
```

```text
1  ⊤        ▷ t : ? len(a) ≥ len(b)
2  t.true   ▷ pick : &string := a
3  t.false  ▷ pick : &string := b
4  ⊤        ▷ emit : Δ emit(pick) ▸ done {}
```

**Accepted.** The two binders run on histories that exclude each other
([O1]), agree on the type ([W4]), and one of them has run on every history
that reaches statement 4 ([O2], [K5]). No name is re-pointed.

What the model gives up is one thing: an ordinary data structure that holds
borrows into another. In the v1 corpus that is `window`, which becomes a view.

## 10. The tag semantics on one example

The instrumented semantics does not know about loans. It gives every slot an
owner tag and every borrow a tag of its own, and it is stuck when an access
goes through a dead tag ([B1] to [B5]). Example 5.4, with a local owner:

```text
1  ⊤ ▷ ~t : tally := tally {0, "start"}
2  ⊤ ▷ slot : &~u64 := &~t.count
3  ⊤ ▷ slot ← 7
4  ⊤ ▷ t.note ← "seen"
5  ⊤ ▷ t.count ← 0
6  ⊤ ▷ slot ← 1
```

| Step | Access | Clause | Tags afterwards |
| --- | --- | --- | --- |
| 1 | `t` is bound | | `T₀`: owner of `t` |
| 2 | Exclusive borrow of `t.count`: a write access through `T₀`, then a fresh tag | [B3] | `T₁`: exclusive, place `t.count`, parent `T₀`. The slot `slot` holds the borrow with tag `T₁`. |
| 3 | Write to `t.count` through `T₁`. `T₁` is alive and exclusive. `T₀` overlaps but is an ancestor. | [B1], [B2] | Unchanged |
| 4 | Write to `t.note` through `T₀`. The place of `T₁` does not overlap it. | [B2] | Unchanged |
| 5 | Write to `t.count` through `T₀`. `T₁` overlaps and is not an ancestor of `T₀`. | [B2] | `T₁` is dead |
| 6 | Write through `T₁` | [B1] | Stuck: X4 |

The checker never runs the program. At statement 2 it records the loan
`⟨t.count, exclusive, ⊤, {slot}⟩`. At statement 5 it finds a write to
`t.count` that does not go through the loan, and a later statement that
mentions `slot`, and rejects the program there ([L3]). Without statement 6 the
program is accepted: `T₁` still dies at step 5, and nothing uses it again.

That is the relation between the two. The semantics kills a tag at the
conflicting access and is stuck at the next use. The checker refuses the
conflicting access whenever a next use exists. The loans invariant of the
core's section 8 states it for every reachable configuration: a borrow whose
holder is used later on this history has a live tag.

The checker is stricter than the semantics in these places, each on purpose:

- **Elements.** The semantics tells `grid[0]` from `grid[1]`. The checker
  treats all elements of one collection as one place (example 5.9), because it
  does not compare index values.
- **Handing a borrow on.** Moving or copying a borrow touches no tag. The
  checker counts it as an access to everything behind the borrow ([L3], core
  choice 11), so a borrow cannot be lent again in part and handed on whole in
  one payload.
- **Contracts.** A payload that the contract derives from two parameters
  holds both loans, whichever it borrows at run time (example 4.2).
- **Loops.** A loan that can pass from one iteration to the next is treated as
  present through the whole body, in the first iteration too ([L2a]).
- **Mentions.** A loan is live while a later statement mentions a holder, even
  if that statement only states a fact about it ([L2]).

## 11. What is known about a lent place

A borrow has no value of its own in the fact logic. It has the sort of what it
refers to, and a view the sort of a struct of what its borrows refer to (fact
logic, section 3.1).

- A local borrow name abbreviates the place it borrows ([I7]). After
  `slot: &~u64 := ~item.count` and `slot ← 0`, the fact is `item.count = 0`.
- Lending a place exclusively to an operation gives its root a new logical
  name. What the contract says with `⇐` and `old` is known afterwards, and the
  parts of the root outside the lent place keep their facts ([I9]; fact logic,
  section 10.3).
- A borrow that arrives as a parameter, a loop parameter or a payload field is
  a logical name for the value behind it. A write through it renames that
  value, and every root on which it holds a loan gets a new name with no fact
  ([I9]). In example 5.5, after `left.push(4)` nothing is known about `p`
  beyond its type.
- A formula reads through borrows and makes no access ([E-Formula]). A loan
  never stops the checker from stating a fact about a lent place. It stops the
  program from reading it.

## 12. Tasks

Tasks have their rules in the core's draft 2 (sections 6.12 and 7.8). For
borrows they add one statement, [TK3]: a borrow given to a child by
`spawn … into set` is held by the set, and so is every borrow that comes back
from `await next`. A set is of kind `must` and is consumed only by the
alternative `empty` ([TK2]), so the loans last until the set is drained.

### 12.1 Children that borrow from their parent

```text
async control shout(text: &string) ■ done
│ emit(text)
│ ■ done
╵

async control chorus(line: &string) ■ done
│ ~voices: tasks<shout> := tasks()
│ spawn shout(line) into voices
│ spawn shout(line) into voices
│ ↻ drain
│ ║ Δ await next voices: done → ↑ drain │ empty → □ drained
│ ■ done
╵
```

```text
1  ⊤ ▷ ~voices : tasks⟨shout⟩ := tasks⟨shout⟩()
2  ⊤ ▷ spawn shout(line) into voices
3  ⊤ ▷ spawn shout(line) into voices
4  ⊤ ▷ drain : ↻ drain()
           1  ⊤           ▷ next : Δ await next(voices) ▸ done {} │ empty {}
           2  next.done   ▷ ↑ drain()
           3  next.empty  ▷ □ drained {}
         ▸ drained {}
5  ⊤ ▷ ■ done {}
```

**Accepted.** Both children hold the shared borrow `line`, through `voices`
([TK3]). The loop leaves only on `empty`, which consumes the set ([TK2],
[O6]), so nothing is owed at statement 5 ([O5]). A shared loan may go to many
children. An exclusive loan goes to one, and the parent cannot touch the place
until the set is drained: a read of it between the `spawn` and the loop is
rejected by [L3], because the loop mentions `voices`.

### 12.2 A lender that an inner scope owns

```text
│ ~voices: tasks<shout> := tasks()
│ ↻ each(next: usize := 0)
│ ║ ⊢ next ≤ len(lines)
│ ║ ? next = len(lines) → □ started
│ ║ copy: string := clone(lines[next])
│ ║ spawn shout(copy) into voices
│ ║ ↑ each(next + 1)
```

**Rejected** at the `↑`, by [L3]: it ends the scope that owns `copy` while
`voices`, a name of the enclosing body, holds a loan on it. No rule about
tasks is involved. `spawn shout(lines[next]) into voices` is accepted, since
the vector outlives the set.

In the service of the Rails reference programs, `serve` clones `path` and
`authority` for every connection. Under these rules it may lend them: both
outlive the set `active`.

## 13. What this document asked of the core and of the surface

Applied to `core.md`:

1. [W8]: a field of a view is among the slots where a borrow name stands for
   the borrow, and a view's field of borrow type is read as a borrow name is.
2. [W9]: a payload field of a loop has the type of the operand that leaves
   through it, so a name of borrow type leaves as a borrow (example 7.5).
3. [T4]: the names after `∈` are borrow or view parameters (example 8.1).
4. Section 7.5: a place behind a borrow field of a view is a place behind a
   borrow.
5. The expression grammar gains the vector literal `[ē]` and the empty map
   `∅`, which the surface and the reference programs already use.
6. A new clause, [E-Operands], with [O0] and [L2] following it: the operands
   of a command that lend are evaluated after the others (question Q17). This
   is the one addition that is a rule and not a clarification.

Applied to `surface.md`:

7. The grammar accepts `~ place` wherever a slot can have an exclusive borrow
   type: a bind, a loop parameter, an argument of `↑`, a payload of `■` or
   `□`, and a field of a view constructor ([G7a]).
8. [U11] applies to every slot with a declared type, and says how a borrow
   name is passed (question Q16).
9. `&` and `&~` apply to the whole type that follows, so `&string ⇀ string` is
   a borrow of a map ([G4a]).
10. A contract written `■ auto` infers `∈` for its borrowed payloads ([G3]).
11. A `□` entry that is a name of borrow type leaves as the borrow ([U9]).

Found and left open, for the corpus to weigh:

- There is no `∀` that lends each element exclusively (7.7).
- Two parts of one collection cannot be lent exclusively at once (5.9). A
  library operation with a trusted contract, such as an exchange of two
  elements or a split into two views, is the expected remedy.
- A type has no invariant; a predicate function in contracts carries it (8.1).
- A view that holds an exclusive borrow cannot be lent again as a whole (8.5).
- The fact logic did not know the length of a vector literal. The
  [corpus](corpus.md) needed it, and [A3] of the fact logic now gives it.

## 14. Choices and questions

Choices of this draft, recorded for the author's acceptance of phase 1:

1. **The verdicts are conformance tests.** Every example carries the clause
   that decides it, and the index below is the start of the phase 2 suite for
   the families B and L.
2. **Where the checker is stricter than the tag semantics, it is on purpose**
   and named (section 10).
3. **The outcome is the option.** No type holds "a borrow or nothing"; an
   operation that may find something has two alternatives (6.1).
4. **A view's promises travel in contracts**, as a predicate function (8.1).

Questions for the author, each with a recommendation. The draft is written as
if the recommendation were taken.

| | Question | Recommendation |
| --- | --- | --- |
| Q16 | An exclusive borrow name `r` is given where an `&~τ` is expected. Is it written `r` or `~r`? | **Always `~r`**, which lends what is behind `r` again; `r` stays usable when that loan ends. One spelling for every slot of exclusive type, and every call that can change something shows the mark. The bare name is rejected with a message that suggests `~r`. The alternative accepts both and makes the bare name a move of the borrow, after which `r` is gone. |
| Q17 | Is `values.push(len(values))` accepted? | **Yes**, by the order of evaluation: the operands of a command that do not lend are evaluated before those that do ([E-Operands], example 5.7). Expressions have no effects, so a program cannot observe the order; only the loan rules and the tags can. The alternative keeps strict left-to-right order, rejects the call, and has the programmer bind the length first. |

## 15. Index of examples

| Example | Subject | Verdict | Decided by |
| --- | --- | --- | --- |
| 4.1 | Return one of two shared arguments | Accepted | [W8], [L4] |
| 4.2 | A loan ends at its holder's last use | Accepted | [L1], [L2], [L3] |
| 4.3 | Assign the lender before that use | Rejected | [L3] |
| 4.4 | Read and clone through a borrow; move through one | Accepted; rejected | [O3] |
| 5.1 | Lend a field to an operation, twice | Accepted | [W8], [L2] |
| 5.2 | Lend exclusively a place that is not mutable | Rejected | [C1] |
| 5.3 | A named borrow of one field, a write to another | Accepted | [L1], [L3] |
| 5.4 | Write the lent field | Rejected | [L3] |
| 5.5 | Two payload borrows from one exclusive parameter | Accepted | [L3], [L4] |
| 5.6 | The same field in both | Rejected | [L3] |
| 5.7 | A value argument that reads the receiver | Accepted | [E-Operands], [L3] |
| 5.8 | Exchange two elements by value | Accepted | [P5] |
| 5.9 | Two element borrows at once | Rejected | [L3] |
| 6.1 | A lookup, cloned | Accepted | [L1] |
| 6.2 | Exit with the borrowed value itself | Rejected | [O3] |
| 6.3 | A write on the path without the loan | Accepted | [L2], [L3], [K4] |
| 6.4 | The write on the path with the loan; reordered | Rejected; accepted | [L3] |
| 6.5 | Decide a variant through a shared borrow | Accepted | [E-Outcome], [L1] |
| 6.6 | Through an exclusive borrow | Accepted | [E-Outcome], [L3] |
| 6.7 | Replace the variant under a field borrow | Rejected | [L3] |
| 6.8 | Exit with a borrow of a local | Rejected | [L4] |
| 7.1 | A borrow as loop state | Accepted | [L2a], [L4] |
| 7.2 | Carry a borrow of an iteration's own name | Rejected | [L3], [L4] |
| 7.3 | An exclusive element borrow across iterations | Rejected | [L2a], [L3] |
| 7.4 | The index instead | Accepted | [P2], [P5] |
| 7.5 | Leave a loop with a borrow | Accepted | [W9], [L1] |
| 7.6 | Leave with a borrow of an iteration's own name | Rejected | [L3], [L4] |
| 8.1 | A window over a vector | Accepted | [W8], [L5], [L4], [P4] |
| 8.2 | A borrow that outlives the view it came through | Accepted | [L1], [L4], [P3] |
| 8.3 | Reshape the vector under a view | Rejected | [L3], [B5] |
| 8.4 | Views and borrows in ordinary types | Rejected | [W6], [T6] |
| 8.5 | A view that holds an exclusive borrow | Accepted | [C1], [L5] |
| 9 | One bind on each of two paths | Accepted | [O1], [O2], [W4] |
| 10 | The tag trace of 5.4 | Stuck at step 6; rejected at statement 5 | [B1], [B2], [L3] |
| 12.1 | Two children that borrow from their parent | Accepted | [TK2], [TK3], [O6] |
| 12.2 | A child that borrows what an iteration owns | Rejected | [L3], [TK3] |
