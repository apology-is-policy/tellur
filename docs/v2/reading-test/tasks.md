=== intro ===
# A reading test

You are taking part in a reading test of a programming language that you have
not seen before. Everything you need is in this one file: a short description
of the language (part 1), a program (part 2) and the tasks (part 3).

Rules:

- Answer from this file alone. Do not open any other file, do not search, and
  do not run anything. An answer that used any other source is discarded.
- Nothing in the program is a trick, except where a task says that something
  was changed.
- Answer every task. Be exact and brief: names of files, controls and
  alternatives, and program lines where asked. Do not explain the language
  back.
- If you are unsure of an item, give it and mark it "(unsure)".

=== tasks-a ===
## Part 3. Tasks

### Task 1. Where does an outcome come from?

A client sends a `POST /lookup` request to the running service. List every
distinct way in which the reply can have HTTP status 500.

For each way, give the chain from its origin to the reply: the operation that
first reports the condition and the alternative it reports (or the event, if it
is not an alternative), then every control that passes it on, with the name the
condition has at each step, up to the place where status 500 is chosen. Do not
list replies with any other status.

### Task 2. A callee gains an outcome

The library control `json_file.load` gains one alternative:

    {LOCKED}

It means that another process holds the file. `{LOAD_RESULT}` in part 2 is to
be read as already having it. Required behaviour:

- Every HTTP request that meets it is answered with status 423 and the holder
  as the body, as plain text.
- The command-line entries `main` and `get` end with `failed`.
- The command-line entries `set` and `remove` end with `locked` itself, payload
  included.

List every edit to the application's four files that is needed for the program
to be accepted by the compiler again and to behave as required. Give each edit
as: the file, the control or declaration, and the new or changed line or lines
in full. Make no change that is not needed. Where a place needs no edit
although a reader might expect one, say so in one line.

## Answer format

    TASK 1
    1. <chain>
    2. <chain>
    ...

    TASK 2
    1. <file> / <control or declaration>: <line or lines>
    2. ...
    No edit needed: <place>: <why, in a few words>

=== tasks-b ===
## Part 3. Task

This copy of the program was changed: one or more lines were removed from it,
and nothing was added. Each removal by itself makes the compiler reject the
program, because a rule of the language described in part 1 is no longer met.

Find every removal. For each give: the file, the control, what is missing
(write the missing line as well as you can), and how you can tell.

List only what you believe was removed. A wrong item counts against you, and
so does a removal you do not find.

## Answer format

    REMOVAL 1
    file: ...
    control: ...
    missing: ...
    how I can tell: ...

    REMOVAL 2 (if there is one, and so on)
    ...
