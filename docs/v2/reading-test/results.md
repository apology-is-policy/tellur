# Reading and repair test: raw results

Scored against `key.md`. One row per run, added as each run reports.
Tokens and tool calls are the figures the harness reports for the run.

## Part A (tasks 1 and 2, unchanged program)

Task 1: found (of 5), origin (of 2), false. Task 2: needed, missed, wrong,
needless, complete.

| Version | Model | Run | T1 found | T1 origin | T1 false | T2 needed | T2 missed | T2 wrong | T2 needless | T2 complete | Tokens | Tools | Seconds | Notes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: | ---: | ---: | --- |
| v1 | haiku | pilot | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 71496 | 1 | 67 | Not scored. E marked unsure. |
| v1 | sonnet | 1 | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 60844 | 1 | 31 | |
| v1 | sonnet | 2 | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 60784 | 1 | 32 | |
| v1 | sonnet | 3 | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 60603 | 1 | 30 | |
| v1 | haiku | 2 | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 68040 | 1 | 51 | E marked unsure. |
| v1 | haiku | 3 | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 67495 | 1 | 50 | E marked unsure. |
| v1 | haiku | 1 | 5 | 2 | 0 | 14 | 0 | 0 | 0 | yes | 69347 | 1 | 56 | E marked unsure. |
| v2 | sonnet | 1 | 5 | 2 | 0 | 8 | 0 | 0 | 0 | yes | 59369 | 1 | 38 | |
| v2 | sonnet | 2 | 5 | 2 | 0 | 8 | 0 | 0 | 0 | yes | 58124 | 1 | 30 | E marked unsure. |
| v2 | sonnet | 3 | 5 | 2 | 0 | 8 | 0 | 0 | 0 | yes | 57864 | 1 | 29 | |
| v2 | haiku | 1 | 5 | 2 | 0 | 8 | 0 | 0 | 0 | yes | 69039 | 1 | 65 | E marked unsure. |
| v2 | haiku | 2 | 5 | 2 | 0 | 8 | 0 | 2 | 0 | no | 70244 | 1 | 70 | E marked unsure. In `list_settings` and `lookup_setting`, whose decisions are plain `Δ`, the new arm is written `→ text {423, holder}` without `■`: the form of an adapter. The compiler would reject both. In `load` the arm is `locked {holder}` with a pattern and no target; counted as a slip. |
| v2 | haiku | 3 | 5 | 2 | 0 | 8 | 0 | 0 | 0 | yes | 70883 | 1 | 73 | E marked unsure. |

## Part B (task 3, seeded copy)

| Version | Model | Run | S1 | S2 | False | Tokens | Tools | Seconds | Notes |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: | --- |
| v1 | haiku | pilot | yes | yes | 0 | 66993 | 1 | 48 | Not scored. Earlier wording: told that exactly two lines in two files were removed. |
| v1 | sonnet | 1 | yes | yes | 0 | 59295 | 1 | 26 | |
| v1 | sonnet | 2 | yes | yes | 0 | 60598 | 1 | 32 | |
| v1 | sonnet | 3 | yes | yes | 0 | 60681 | 1 | 33 | |
| v1 | haiku | 1 | yes | yes | 0 | 74895 | 1 | 76 | Exact form of the missing line marked unsure. |
| v1 | haiku | 2 | yes | yes | 0 | 78438 | 1 | 90 | Gave the files by a repository path it inferred; one tool call, so nothing else was read. |
| v1 | haiku | 3 | yes | yes | 0 | 81911 | 1 | 104 | Exact form of the missing line marked unsure. |
| v2 | sonnet | 1 | yes | yes | 0 | 56475 | 1 | 22 | |
| v2 | sonnet | 2 | yes | yes | 0 | 57051 | 1 | 25 | |
| v2 | sonnet | 3 | yes | yes | 0 | 56658 | 1 | 23 | |
| v2 | haiku | 1 | yes | yes | 0 | 80352 | 1 | 109 | Wording of the second line marked unsure. |
| v2 | haiku | 2 | yes | yes | 0 | 77304 | 1 | 99 | Gave the path of the subject file as the file; one tool call. |
| v2 | haiku | 3 | yes | yes | 0 | 72945 | 1 | 84 | |

## Subject files

Built by `build.py` from the repository at commit `20302c3`. The v1 runs were made one commit earlier, from files with the same digests. First 16 hex digits of SHA-256:

| File | Lines | Characters | Digest |
| --- | ---: | ---: | --- |
| `subject-v1-a.md` | 965 | 35592 | `c0a767b253520ea9` |
| `subject-v1-b.md` | 940 | 34492 | `26f9af03a20b0ef9` |
| `subject-v2-a.md` | 609 | 24846 | `9e3fd8348b16ec51` |
| `subject-v2-b.md` | 584 | 23764 | `504fb92d08aa4dd8` |
