## Part 1. The language in brief

A program is a set of **controls**. A control does not return a value. It ends
with one of its named **outcomes** (also called alternatives), and an outcome
may carry a payload. A caller must deal with every alternative of every
operation it calls. Nothing is passed on implicitly.

```villanelle
control show_file(string filename) exits auto {
    [entry]:
        delta open(filename) {
            opened { handle file } => open,
            not_found => missing,
            denied => blocked,
        }
    [open]:
        delta read(borrow file) {
            data { bytes contents } => read,
            failed { read_error problem } => read_bad,
        }
    // Always attempt close, even if reading failed.
    [open]:
        delta close(file) {
            closed => closed,
            failed { close_error close_problem } => close_bad,
        }
    [read]:
        delta decode_utf8(contents) {
            text { string text } => decoded,
            invalid => bad_encoding,
        }
    [decoded & closed]: emit(text); exit done;
    [read_bad | close_bad | bad_encoding | blocked]: exit failed;
    [missing]: exit missing;
}
```

Reading it:

- `delta` decides an operation. Each arm names an alternative of the
  operation, binds its payload with declared types, and after `=>` names the
  **path** on which that history continues (`opened { handle file } => open`).
- A **label** `[mask]:` selects the statements that follow it, up to the next
  label. They run on the histories for which the mask holds (`&` and, `|` or).
  `[entry]` selects every history still running. Indentation is only layout.
- An arm `=> exit name { values }` ends the control there instead of naming a
  path.
- **Every alternative must be dealt with.** In an `exit delta` (below) each
  alternative needs an arm of its own, and the compiler rejects one that is
  left out. In a plain `delta`, an alternative that is left out continues as a
  path of its own name, which a label must then select and finish. There is no
  arm for "everything else".
- Every history must end in an `exit`.

Other forms:

| Form | Meaning |
| --- | --- |
| `control f(T x, ref T r) exits a { T p }, b` | A signature. The exits follow `exits`. `ref T` is a shared reference. `pub` makes a declaration visible to other files, which call it as `alias.f(…)` after `import "file" as alias;`. |
| `exit name { values };` | Exit the control with that outcome. |
| `exit delta op(args) { … };` | Decide and exit at once. An arm `a { T x } => b { x }` exits with `b`. The right side may be another `delta` with arms of the same kind, optionally named by a label, `=> [name]: delta …`. |
| `outcome name { … }` | Declares a named set of alternatives that a signature can use, `exits name`. `@exit n` gives the process exit status. |
| `delta (condition) { true => a, false => b }` | A boolean decision. |
| `forward op(args);` | Call `op` and exit with whatever it ends with. |
| `exits auto` | The exits are the ones the body uses. |
| `loop name(T p = start) { … }`, `again name(args);` | A loop; go round again with new values for its parameters. |
| `borrow x`, `borrow mutable x`, `deref r`, `move x` | Lend `x`; lend it for change; read the value behind a reference; hand `x` over for good. `mutable` marks a variable that may be changed. |
| `auto x = capture op(args);`, `delta x { … }` | Store the completed outcome of `op` in `x`; later decide on it. |
| `map<K, V>`, `map {}`, `concat(a, b)` | A map type; the empty map; two texts joined. |
| `async`, `await`, `run` | An async control can wait. `await` waits for an async operation inside async code. `run` runs one to its end from ordinary code. |
| `tasks()`, `spawn f(…) into set;`, `await next set`, `cancel set;` | A task set. `spawn` starts a member. `await next` waits for a member to finish and has the member's outcomes plus `empty`, when none is left. `cancel` asks every member to stop: a cancelled task's waiting operation then ends with its `cancelled` alternative, if it lists one. A task is also cancelled when the task that owns its set is. |
| `faults`, `boundary … contingency`, `supervise`, `terminate` | A control marked `faults` may fault: its work is abandoned and it has no outcome. `supervise b(args);` runs the boundary `b`. If the work in it faults, the `contingency` block runs in its place, with the values named after `retains`. `terminate;` ends either block. The supervisor then goes on to its next statement and learns nothing of what happened. `@contingent` marks a control that a contingency may call. |
| `resource` | A type whose values must be ended. A control that holds one must hand it over, on every history, to an operation that takes it by value (a parameter without `ref`) before it exits. The compiler rejects a control that can end while it still holds one. |
| `variant name { … }`, `name.a {}` | A data type with named cases, and a value of it. |
| `//` | A comment. |

Built-in operations used by the program: `map_get(m, key)` ends with
`found { ref V value }` or `missing`; `map_insert(m, key, value)` with
`inserted` or `replaced { V previous }`; `map_remove(m, key)` with
`removed { V value }` or `missing`; `text_trim(s)` with `trimmed { string }`;
`checked_sub(a, b)` with `difference { value }` or `underflow`.
`map_length(m)` is the number of entries, `clone(x)` a copy, `count(set)` the
number of members, and `drop(x)` discards a value. `emit(text)` writes a line
to the console.
