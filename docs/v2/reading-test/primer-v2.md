## Part 1. The language in brief

A program is a set of **controls**. A control does not return a value. It ends
with one of its named **outcomes** (also called alternatives), and an outcome
may carry a payload. A caller must deal with every alternative of every
operation it calls. Nothing is passed on implicitly.

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

- The solid rail `│` is the body of the control. A statement on it runs on
  every history that is still running. `╵` ends the body.
- `Δ` decides an operation. Each alternative of the operation is named, on one
  line after a colon (separated by `│`) or on lines of its own marked `├` and
  `└`. Names in braces bind the payload.
- **Every alternative must be named exactly once.** A decision that leaves one
  out is rejected by the compiler. There is no arm for "everything else".
- An alternative with `→` and a terminator ends there: `not_found → ■ missing`.
  One without a target continues: after `opened {file}` the body goes on, with
  `file` bound.
- Every alternative that continues is a **path**, named after itself (`data`,
  `closed`) or renamed by a route (`text {text} → decoded`).
- A junction `╞═` introduces a **label**: a condition over paths (`∧` and, `∨`
  or). The statements on the dashed rail `┆` beneath it run on the histories
  for which the condition holds. The short form is `╞═ name: statement;
  statement` on one line. After a label's block, the solid rail continues for
  every history still running.
- Two decisions here have an alternative `failed`, so the label says
  `read.failed` and `close.failed`.

Other forms:

| Form | Meaning |
| --- | --- |
| `control f(x: T, r: &T) ■ a {p: T} │ b` | A signature. The exits follow `■`. `&T` is a shared reference. `pub` makes a declaration visible to other files, which call it as `alias.f(…)` after `import "file" as alias`. |
| `■ name {values}` | Exit the control with that outcome. |
| `→ ■` with nothing after it | Exit with the same alternative and the same payload. |
| `■ Δ op(args)` with arms | Decide and exit at once. An arm `a {x} → b {x}` exits with `b`. An arm with no target exits with the alternative under its own name, payload included. Inside this form the targets are written without `■`. |
| `outcome name` with `├` lines | Declares a named group of alternatives. `↦ n` gives the process exit status. |
| `…group` | In a signature: every alternative of that group is an exit. In a decision: one arm that stands for all alternatives of the group that have no arm of their own. |
| `? condition → statement` | A test. The statement, or the block on a dashed rail beneath, runs if the condition holds. Otherwise the body goes on. |
| `forward op(args)` | Call `op` and exit with whatever it ends with. |
| `■ auto` | The exits are the ones the body uses. |
| `↻ name(p: T := start)`, `↑ name(args)`, `□ path` | A loop, drawn on a double rail `║`; go round again; leave the loop and continue after it on the path named. `╠═` is a label inside a loop. |
| `⊥` | This alternative cannot occur here. The compiler proves it. |
| `~x`, `▾x` | `~` marks something that may be changed. `▾x` hands `x` over to the operation for good. |
| `x := capture op(args)`, `■ ▾x` | Store the completed outcome of `op` in `x`; later exit with it. |
| `K ⇀ V`, `∅`, `"…{e}…"` | A map type; the empty map; text with the value of `e` inserted (`\{` is a plain brace). |
| `async`, `await`, `run` | An async control can wait. `await` waits for an async operation inside async code. `run` runs one to its end from ordinary code. |
| `tasks()`, `spawn f(…) into set`, `await next set`, `cancel set` | A task set. `spawn` starts a member. `await next` waits for a member to finish and has the member's outcomes plus `empty`, when none is left. `cancel` asks every member to stop: a cancelled task's waiting operation then ends with its `cancelled` alternative, if it lists one. A task is also cancelled when the task that owns its set is. |
| `faults`, `boundary … contingency`, `supervise`, `terminate` | A control marked `faults` may fault: its work is abandoned and it has no outcome. `supervise b(args)` runs the boundary `b`. If the work in it faults, the `contingency` block runs in its place, with the values named after `retains`. `terminate` ends either block. The supervisor then goes on to its next statement and learns nothing of what happened. |
| `resource` | A type whose values must be ended. A control that holds one must hand it over, on every history, to an operation that takes it by value (a parameter without `&`, written `▾x` at the call) before it exits. The compiler rejects a control that can end while it still holds one. |
| `uses a.b` | The effects that a public control may perform. |
| `┄` | A comment. |

Built-in operations used by the program: `m.get(key)` ends with `found {value}`
or `missing`; `m.insert(key, value)` with `inserted` or `replaced {previous}`;
`m.remove(key)` with `removed {value}` or `missing`. `len(m)` is the number of
entries, `s.trim()` the text without surrounding white space, `clone(x)` a
copy, `set.count()` the number of members. `emit(text)` writes a line to the
console.
