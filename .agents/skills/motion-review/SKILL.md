---
name: motion-review
description: Get a verb's motion on Beryl's stage judged by an independent reviewer, when the user asks for that. Films the verb from both seats in both layouts beside Arena's clips of it, hands only the films to a fresh Opus or Fable reviewer who scores a fixed rubric (readable, staged, path and easing, anticipation and settle, depth, groups, fits Beryl, pace), and reports the verdict. Use only when the user asks to review, judge or pass an animation; it gates nothing, so never run it because a verb is new, its motion changed or a ticket is closing.
---

# Motion review

Automated replay checks measure the frames. They cannot say
whether the motion is any good: straight glides, bare arrivals and jostling
batches pass it. This review can.

Run it only when the user asks (2026-11-12, of the reviews run on every verb: "I
am really not finding that these are delivering useful results"). No verb,
ticket or plan waits on it, and a motion change ships without one. The user
judges motion himself, through his bug reports.

When he asks, you never judge the verb yourself. You film it, hand the films to
a reviewer with none of your context, and report what it says.

It is not a check: nothing here is a check family or is recorded, and no image
is ever committed.

## 1. Film the verb

Run `just b3d motion-film <verb> [case]` and save its output for the reviewer.
The tool selects a recording from `data/beryl3d/motion-recordings.json` or
uses `--recording <name>`. It films the first step of each case from both
seats in both layouts at 15 frames per second and 760 pixels wide. It cuts
available Arena instances at the same rate and width. Use `--instances N`
to choose how many Arena instances to cut.

The output names each film and Arena frame folder and lists the step, any
steps playing at the same time and what moves in each frame. The tool reports
missing Arena recordings. For a closer look at one instant, run
`just b3d png <recording> --at <moment> --tree`.

## 2. Hand it to a fresh reviewer

One reviewer a verb, with a context of its own: a subagent (the Agent tool) or a
session started for it, on Opus, or on Fable when the user explicitly asks for it. Never review in your
own context, and never continue an earlier reviewer: each round gets a new one.

Give it this and nothing else:

```text
Review the motion of the verb `<verb>` on Beryl's stage [, case `<case>`].
Read <repo>/.agents/skills/motion-review/reviewer.md and follow it exactly.
The packet is <path to the saved motion-film output>.
Verbs already passed, for consistency: <names, or none>.
```

Do not tell it what you built or changed, what you think of it, what an earlier
round found, or which ticket it is. Do not give it the code. A reviewer handed a
conclusion returns it.

## 3. Report the verdict

Give the user the reviewer's report as it wrote it, with the paths of the sheets
it names. `PASS` is every line of the rubric scored pass or note. When the work
has a ticket, put the report there too as one comment:

```sh
ignite ticket comment <ticket> "Motion review, round <n> (<model>): <its report>"
```

## 4. After the report

The findings are the user's to act on. Fix the ones he asks for, and review
again only if he asks: film again and hand the new films to a new reviewer. File
no ticket for a finding he did not ask to have fixed, and none whose work is
another review.
