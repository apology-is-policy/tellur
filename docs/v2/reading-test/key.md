# Reading and repair test: answer key and scoring

Written on 19 November 2026, before any run. The sections "Runs" and "Pilot"
at the end were completed after the two pilot runs and before the scored
runs; the answers and the scoring rules were not changed. The key is the same
for both versions unless a row says otherwise.

## Task 1. Every way a `POST /lookup` is answered with status 500

Five ways. Four reach `text {500, …}` in `http`; the fifth is the fault path.

| | Origin | Chain | Where 500 is chosen |
| --- | --- | --- | --- |
| A | `json_file.load` reports `invalid {message}` | `settings.load` renames it `invalid_file {message}`; `settings.get` passes `invalid_file` on | `http.lookup_setting`, arm `invalid_file` of the decision on `settings.get`: `text {500, message}` |
| B | `json_file.load` reports `read_failed {message}` | `settings.load` renames it `storage_failed {message}`; `settings.get` passes it on | `http.lookup_setting`, arm `storage_failed`: `text {500, message}` |
| C | `json_file.load` reports `too_large` | `settings.load` and `settings.get` pass `too_large` on | `http.lookup_setting`, arm `too_large`: `text {500, "Settings file exceeds 1 MiB"}` |
| D | `json_encode` reports `encode_failed {message}` | In `http.encode_value`, after the setting was found | `http.encode_value`: `text {500, message}`; `lookup_setting` passes it on (v1: the inner arm `text { status, content } => text { status, content }`; v2: `forward encode_value(value)`) |
| E | A fault while the request is handled (`route` is declared `faults`) | The boundary `request` in `server` abandons the work; its `contingency` runs | `transport.reply_failure(connection)`, which sends a fixed generic 500 if no reply was started |

After `http`, every `text {status, content}` goes through `route` (a
`forward`), `server.prepare` (arm `text` gives `prepared {status, content,
…}`) and `server.exchange` (`transport.send`). A subject need not list these
steps to be credited.

Scoring:

- **Found**, 0 to 5: one point for each of A to E that the answer identifies.
  A, B and C count separately even when the answer gives them in one item,
  provided each alternative is named.
- **Origin**, 0 to 2: one point each for A and B when the answer names the
  alternative of `json_file.load` (`invalid`, `read_failed`) and not only the
  name it has in `settings`.
- **False**, a count: each listed way that does not give a 500 on
  `POST /lookup`. Examples: `commit_uncertain` (only `PUT /settings` and
  `POST /remove`), a failure of `transport.send`, `too_large` as 413.

## Task 2. `json_file.load` gains `locked`

The task fixes the behaviour: 423 with the holder as a plain-text body for
every HTTP request; `failed` for the entries `main` and `get`; `locked` passed
on by the entries `set` and `remove`.

### v1: fourteen edits

| | File | Place | Edit |
| --- | --- | --- | --- |
| 1 | `settings.vn` | `outcome loaded_settings` | add `locked { string holder }` |
| 2 | `settings.vn` | `outcome update_result` | add `locked { string holder }` |
| 3 | `settings.vn` | `outcome removal_result` | add `locked { string holder }` |
| 4 | `settings.vn` | `load`, the `exit delta json_file.load` | add `locked { string holder } => locked { holder }` |
| 5 | `settings.vn` | `set`, the `delta load(path)` | add `locked { string holder } => exit locked { holder }` |
| 6 | `settings.vn` | `get`, the `exits` list | add `locked { string holder }` |
| 7 | `settings.vn` | `get`, the `exit delta load(path)` | add `locked { string holder } => locked { holder }` |
| 8 | `settings.vn` | `remove`, the `delta load(path)` | add `locked { string holder } => exit locked { holder }` |
| 9 | `http.vn` | `list_settings` | add `locked { string holder } => text { 423, holder }` |
| 10 | `http.vn` | `lookup_setting`, the adapter on `settings.get` | the same arm |
| 11 | `http.vn` | `update_setting`, the adapter on `settings.set` | the same arm |
| 12 | `http.vn` | `remove_setting`, the adapter on `settings.remove` | the same arm |
| 13 | `main.vn` | `main` | an arm for `locked` and a disposition that ends with `exit failed` (an arm `=> exit failed`, or a path and a label) |
| 14 | `main.vn` | `get` | the same |

No edit: `main.vn` `set` and `remove` (they name `settings.update_result` and
`settings.removal_result`, which gained `locked`, and `forward`); `server.vn`
(a `text` response passes through `prepare` whatever its status).

### v2: eight edits

| | File | Place | Edit |
| --- | --- | --- | --- |
| 1 | `settings.vn` | `outcome storage_error` | add `├ locked {holder: string}` |
| 2 | `settings.vn` | `load`, the `■ Δ json_file.load` | add the arm `├ locked` (passes on under its own name) |
| 3 | `http.vn` | `list_settings` | add `├ locked {holder} → ■ text {423, holder}` |
| 4 | `http.vn` | `lookup_setting` | add `├ locked {holder} → ■ text {423, holder}` |
| 5 | `http.vn` | `update_setting`, inside `■ Δ` | add `├ locked {holder} → text {423, holder}` |
| 6 | `http.vn` | `remove_setting`, inside `■ Δ` | add `├ locked {holder} → text {423, holder}` |
| 7 | `main.vn` | `main` | an arm for `locked` that ends with `■ failed` (on the arm, or by a label) |
| 8 | `main.vn` | `get` | the same |

No edit: `settings.vn` `set`, `get` and `remove` (their contracts include
`…storage_error`, and their decisions on `load` have the arm
`…storage_error → ■`); `main.vn` `set` and `remove` (`■ auto` and `forward`);
`server.vn`.

An answer that adds `locked` to the four contracts of `settings` one by one,
and an arm to each of the three decisions on `load`, in place of edit 1, is
also a complete repair. It is scored against the sites that approach needs
(fourteen).

### Scoring

- **Needed**: the number of edit sites the subject's approach requires.
- **Missed**: needed sites that the answer does not list.
- **Wrong**: listed edits that are incorrect: the wrong place, a line the
  compiler would reject, the wrong status or body, or a behaviour the task did
  not ask for.
- **Needless**: listed edits that are correct to make and not required.
- **Complete**: no site missed and no edit wrong.

Small slips of spelling in a line (a missing comma, an arrow in the wrong
style) are not counted when the intent is plain.

## Task 3. Two seeded omissions

| | File | Control | Removed | How a reader can tell |
| --- | --- | --- | --- | --- |
| S1 | `http.vn` | `lookup_setting` | The arm for `storage_failed` of the decision on `settings.get` (v1: `storage_failed { string message } => text { 500, message },`; v2: `├ storage_failed {message} → ■ text {500, message}`) | `settings.get` can end with `storage_failed`; in v2 that is read through the group `…storage_error`. Every alternative must have an arm. |
| S2 | `server.vn` | `start` | `transport.release(listener);` (v2: `transport.release(▾listener)`) | `listener` is a resource and is still held when `start` exits. |

Scoring: **S1** found or not; **S2** found or not; **False**, the number of
other items claimed.

## Runs

Each subject is one agent with no other context, told to read one file and to
use nothing else. A run that opened any other file is discarded and repeated.
Tasks 1 and 2 are one run on the unchanged program (part A); task 3 is a
separate run on the seeded copy (part B), so that no subject sees both copies.

Subjects: two models, a smaller one (`haiku`) and a larger one (`sonnet`).
Three runs of each part, for each version, on each model: twenty-four runs.
For each run the tokens used and the number of tool calls are recorded.

## Pilot (not scored)

Two pilot runs on the smaller model, v1 only, checked the instrument before
the key was used. Part A: all five ways found, both origins named, the
fourteen edits complete, nothing wrong. Part B, which then told the subject
that exactly two lines in two different files were removed: both found.

One change followed. The pilot was at the ceiling, so task 3 no longer says
how many lines were removed or that they are in different files. A subject
has to decide when it has found everything, and a wrong item counts.

A seed considered and rejected before any run: removing `cancel active` from
`serve`. In both versions a member is cancelled when the task that owns its
set is, so the statement is redundant where it stands and its removal changes
nothing.
