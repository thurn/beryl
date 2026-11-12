---
name: motion-review
description: Get a verb's motion on Beryl's stage judged by an independent reviewer, when the user asks for that. Films the verb from both seats in both layouts beside Arena's clips of it, hands only the films to a fresh Opus or Fable reviewer who scores a fixed rubric (readable, staged, path and easing, anticipation and settle, depth, groups, fits Beryl, pace), and reports the verdict. Use only when the user asks to review, judge or pass an animation; it gates nothing, so never run it because a verb is new, its motion changed or a bead is closing.
---

# Motion review

`just motion` only holds the stage to the values a verb declares. It cannot say
whether the motion is any good: straight glides, bare arrivals and jostling
batches pass it. This review can.

Run it only when the user asks (2026-11-12, of the reviews run on every verb: "I
am really not finding that these are delivering useful results"). No verb, bead
or plan waits on it, and a motion change ships without one. The user judges
motion himself, through his bug reports.

When he asks, you never judge the verb yourself. You film it, hand the films to
a reviewer with none of your context, and report what it says.

It is not a check: nothing here runs in `just check` or is logged, and no image
is ever committed.

## 1. Film

```sh
just motion --film <verb> [case]
```

`<verb>` is one of `ui/verbs.json`'s; `[case]` one of its cases in
`data/ui-conventions.json` (`just motion --values` lists them), for one case
alone. It plays the verb's moments of the verb suite on the stage in stepped
stage time, so the films are the same on any machine, and writes into
`~/Library/Caches/beryl/motion/<checkout>/film/<verb>/`:

- For each layout (desktop, phone), seat (yours, theirs) and case, the first
  take of the case: `table.png` (the whole table before, as it begins, a third
  and two thirds through, as it ends, after), `strip.png` (30 frames a second,
  framed on everything that moves) and `ticks.png` (every tick of the 300 ms in
  which the subject changes most). A take is a run of the verb's steps that play
  together: one flight, or a batch. `--all` films every take.
- `contact/<case>-<k>-<seat>.png`: Arena's instance `k` of the case
  (`data/arena-motion.json`) over that seat's take on the desktop, both at one
  scale of the table and the same time apart. `--instances N` takes more than
  two a case.
- `brief.txt`: the packet's text. The verb's prose, Beryl's values beside
  Arena's, every sheet's path, and for each step what the tracker reads of it
  (easing, sizes, the path's bow, turn and squash) and the stage's own pose of
  its subject at every tick. The tracker's reading of each Arena instance is
  beside, or why it cannot follow it.

It prints every path. A verb with many cases takes minutes (fly, about eight):
film one case while you work on it, and the whole verb for the review.

A verb that plays in no moment of the suite cannot be filmed: add its moment to
`ui/verbs.json` first (`watches`). CSS animations run on the page's time, not
the stage's, and are stopped in every frame: an effect must play on the stage's
clock to be seen, and reviewed.

## 2. Hand it to a fresh reviewer

One reviewer a verb, with a context of its own: a subagent (the Agent tool) or a
session started for it, on Opus or Fable and never Sonnet, which overrides
AGENTS.md's rule of Sonnet for subagents launched together. Never review in your
own context, and never continue an earlier reviewer: each round gets a new one.

Give it this and nothing else:

```text
Review the motion of the verb `<verb>` on Beryl's stage [, case `<case>`].
Read <repo>/.agents/skills/motion-review/reviewer.md and follow it exactly.
The packet is <path to brief.txt>.
Verbs already passed, for consistency: <names, or none>.
```

Do not tell it what you built or changed, what you think of it, what an earlier
round found, or which bead it is. Do not give it the code. A reviewer handed a
conclusion returns it.

## 3. Report the verdict

Give the user the reviewer's report as it wrote it, with the paths of the sheets
it names. `PASS` is every line of the rubric scored pass or note. When the work
has a bead, put the report there too as one comment, then `enact sync`:

```sh
bd comments add <bead> "Motion review, round <n> (<model>): <its report>"
```

## 4. After the report

The findings are the user's to act on. Fix the ones he asks for, and review
again only if he asks: film again and hand the new films to a new reviewer. File
no bead for a finding he did not ask to have fixed, and none whose work is
another review.
