# Villanelle v2 reading and repair test

Status: **first run**, 20 November 2026. Phase 1, deliverable 4 of the
[redesign plan](../redesign-plan.md). The subjects were language-model agents.
No person has taken the test yet; that part is the author's and is open.

[Index of the v2 design](README.md) · [Corpus on paper](corpus.md) ·
[Surface](surface.md)

## 1. Result

Twenty-four runs, twelve on each version of the settings application. In 23
the answer was complete and correct, so the test is at its ceiling and cannot
rank the two versions on reading.

| Task | v1 | v2 |
| --- | --- | --- |
| Find every origin of an outcome | Every run found all five | Every run found all five |
| Add an outcome to a callee and repair the callers | 14 edits needed. 6 of 6 answers complete. | 8 edits needed. 5 of 6 answers complete. |
| Find the seeded omissions | Every run found both, with no false item | Every run found both, with no false item |

The one faulty answer was a v2 repair by the smaller model. It listed all
eight places and wrote two of the new arms in the wrong form (section 4.2).

The plan's exit condition is that the test does not favor v1. On finding
origins and omissions the versions are equal. On the repair, v2 needs six
fewer edits, and one v2 answer in six had an error where no v1 answer did. One
in six against none in six is not a difference that a test of this size can
establish. I read the condition as met in the weak sense that nothing was
shown either way, and I would not rest more than that on these runs.

## 2. Design

The plan asks for the v1 and v2 versions of the same programs to be given to
people and to agents, with three tasks. Everything in this section was fixed
before the scored runs, and the [answer key](reading-test/key.md) was written
before any run.

**Material.** The settings application: four files and the signatures of what
they call. The v1 text is `applications/settings/`, 635 lines. The v2 text is
[`corpus/settings/`](corpus/settings/), 304 lines. The signatures of the host
and of the two libraries are the same for both, with the same comments, in
each version's syntax.

**Primer.** A subject has never seen the language, so each file starts with a
description of its version: one example with notes, and a table of the other
forms. The two are of similar length (960 and 1,025 words), use the same
example program, and say the same things about what the versions share
(tasks, cancellation, faults, resources).

**Subjects.** An agent with no other context, told to read one file and to
use nothing else. Two models, a smaller and a larger one (Claude Haiku and
Claude Sonnet, as the harness names them), three runs of each part for each
version. Every run made exactly one tool call, the read of its
file. A subject saw one version only.

**Tasks.**

1. *Origin.* List every way a `POST /lookup` request is answered with status
   500, each with its chain from the operation that first reports the
   condition. There are five: three alternatives of the file load, one of the
   encoder, and a fault that the request boundary answers.
2. *Repair.* The library's `load` gains `locked {holder}`. It must become
   status 423 in every HTTP reply and `failed` at two command-line entries,
   and pass through two others. List every edit.
3. *Omission.* A copy of the program with lines removed, each of which makes
   the compiler reject it. The subject is not told how many. Two were removed:
   the arm for `storage_failed` in `lookup_setting`, and the release of the
   listener in `start`.

Tasks 1 and 2 are one run on the unchanged program. Task 3 is a separate run,
so that no subject sees both copies.

**Scoring.** By the key: items found, items that are wrong, and for the
repair the edit sites missed, wrong or needless. I scored the answers and knew
which version each was.

**Two changes before the scored runs.** Two pilot runs on v1, not scored,
checked that the instructions were understood. Both were fully correct, so
task 3 was made harder: it had said that exactly two lines in two files were
removed. And the second seeded omission was to be `cancel active` in the
server. Writing the key showed that its removal changes nothing in either
version, since a member is cancelled when the task that owns its set is, so
the release of the listener took its place.

The material, the key, the scores for each run and every answer as given are
in [`reading-test/`](reading-test/). `build.py` there assembles the four
files that the subjects read, and `results.md` has their digests.

## 3. Results

### 3.1 Tasks 1 and 2

Six runs for each version, three on each model.

| | v1 | v2 |
| --- | ---: | ---: |
| Ways found, of 5 in each run | 30 of 30 | 30 of 30 |
| Origins traced to the library's own alternative, of 2 in each run | 12 of 12 | 12 of 12 |
| Ways listed that are wrong | 0 | 0 |
| Edit sites the repair needs | 14 | 8 |
| Edit sites missed | 0 | 0 |
| Edits that are wrong | 0 | 2, in one run |
| Edits that are not needed | 0 | 0 |
| Repairs complete and correct | 6 of 6 | 5 of 6 |

Every v2 subject chose the short repair: one line in the group
`storage_error` and one arm in `load`, after which `set`, `get` and `remove`
need nothing. In v1 the same change is eight edits in that file: three
outcome declarations, one contract and four arms.

Nine of the twelve subjects marked the fifth way, the fault, as unsure, and
most of them said why: nothing in the program raises the fault that `route`
permits.
The independent review of the corpus made the same remark
([corpus](corpus.md), section 8).

### 3.2 Task 3

| | v1 | v2 |
| --- | ---: | ---: |
| Missing arm found | 6 of 6 | 6 of 6 |
| Missing release found | 6 of 6 | 6 of 6 |
| Items claimed that were not removed | 0 | 0 |

In v2 the missing arm can only be seen through a group: `settings.get` is
declared with `…storage_error`, and the reader has to look up which
alternatives that stands for. All six did, without being told to.

### 3.3 Effort

Mean of three runs. The token figure is what the harness reports for a run.
It includes the agent's own fixed instructions, which are the same in every
run, so the differences between cells mean something and the ratios do not.

| Part | Model | v1 tokens | v2 tokens | v1 seconds | v2 seconds |
| --- | --- | ---: | ---: | ---: | ---: |
| Tasks 1 and 2 | Smaller | 68,294 | 70,055 | 52 | 69 |
| Tasks 1 and 2 | Larger | 60,744 | 58,452 | 31 | 32 |
| Task 3 | Smaller | 78,415 | 76,867 | 90 | 97 |
| Task 3 | Larger | 60,191 | 56,728 | 30 | 23 |

The file a v2 subject reads is 37 percent shorter in lines and 30 percent
shorter in characters (609 lines against 965 for tasks 1 and 2). The larger
model used about 2,300 to 3,500 fewer tokens on v2. The smaller model used
fewer only on task 3. On tasks 1 and 2 it used more and took a third longer.

## 4. What this says about the design

### 4.1 Outcome groups did what they are for

Decision D3 lets a named group stand for alternatives. The test touched it
from both sides. The repair shrank from fourteen edits to eight because the
new alternative entered one group. And the omission hidden behind a group was
found by every subject. The risk that a group hides an unhandled alternative
from a reader did not show here.

### 4.2 The exit mark in an arm is easy to get wrong

The faulty answer added this arm to two plain decisions:

```text
│ ├ locked {holder} → text {423, holder}
```

That is the form of an arm inside `■ Δ`, where every arm exits and the mark is
left out. In a plain `Δ` the arm must say `→ ■ text {423, holder}`. The same
answer wrote the form correctly in the two controls that do use `■ Δ`, so it
copied the nearest pattern without seeing which kind of decision it was in.
The compiler rejects the arm, since a payload after a route is not a
statement, and the repair is one glyph. v1 has the same pair (`=> exit
text {…}` in a `delta`, `=> text {…}` in an `exit delta`), and no v1 subject
confused them.

Two things follow. The compiler's message for this arm should name the
remedy: write `■` before the alternative, or make the decision `■ Δ`. And the
pair belongs with open limit O4 of the corpus, what an arm's target may be,
when that is settled.

### 4.3 Shorter text was not cheaper to read for every subject

Rails has fewer lines and fewer characters. For the larger model that showed
as fewer tokens and the same or less time. For the smaller model, on the
repair, it did not. A likely reason is that box-drawing characters and
mathematical signs are rare in what these models were trained on, and common
words are not. A person does not read tokens, so
this says little about people, but it is a cost for the plan's goal of
agents that repair programs, and the phase 8 repair benchmark should measure
it with a compiler in the loop.

## 5. Limits

- **Three runs in a cell.** Nothing here is a measured rate.
- **Agents are not people.** They do not tire or skim, and the box-drawing
  layout that Rails relies on reaches them as characters in a line, not as a
  picture. What a person sees in a drawn rail this test cannot say.
- **The ceiling.** A program of 300 to 600 lines with tasks that can be done
  by reading every arm is too easy for these subjects. A difference between
  the versions, in either direction, could be hidden under it.
- **One program.** The settings application has no proof obligations, one
  loop of note and no generic code. The parts of v2 that are furthest from
  v1 (`⊢`, `⇐`, `⊥`, views) were not in the test.
- **One author.** I wrote the v2 text, both primers, the tasks and the key,
  and scored the answers knowing the version. The primers decide much of
  what a subject can do, and a different primer could move either column.
- **No compiler.** An agent repairing real code sees diagnostics. Here it
  saw none, in either version.

## 6. What remains

- **People.** The plan asks for people as well as agents. The four files in
  `reading-test/build/` are ready to be given to a reader as they are, in a
  font that has the glyphs (plan, section 5.5). The key and the scoring rules
  apply unchanged.
- **A harder test.** The reading study of phase 8 uses the application of
  5,000 to 10,000 lines, where a reader cannot hold every arm in view. That
  is where a difference would show, and where blind scoring is worth
  arranging.
