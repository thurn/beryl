# Ignite at high volume: land first, verify the tip once

Ignite is the program that builds, checks and delivers every change to Project
Beryl. Coding agents are its only users. An agent claims a **ticket** (one
task), edits in a **ticket tree** (a git worktree at a fixed path that keeps its
build between tickets), and delivers with `ignite land`, which pushes the
ticket's commits to the shared `staging` branch. That delivery is a **landing**.
The newest commit on `staging` is the **tip**. A second branch, `master`, only
ever points at a commit of `staging` whose checks have passed.

Today a landing must pass its checks on the exact tip. When another landing
moves the tip first, the landing rebases and checks again. On 2026-11-19 that
rule, on a machine short of cores, made one landing take 91 minutes and seven
attempts. The median landing that ran the checks took 7.8 minutes, although the
checks it chooses are budgeted at 60 seconds.

This document proposes to change what a landing waits for, following the way
Google and Meta run continuous integration on one large repository. It has six
parts:

- **Check once.** A change is checked on the commit it was written on. The
  result is stored and stays valid while the tip moves.
- **Land with git alone.** A landing rebases and pushes. It takes seconds, and
  it never waits for another landing or starts again because of one.
- **Verify the tip in batches.** The tip is compiled and checked once for each
  batch of landings, in one tree that has cores kept for it.
- **Revert a broken build, and nothing else.** A landing that leaves the tip
  unable to compile is reverted, and its ticket opens again. Any other failing
  check is fixed forward, as today.
- **Start from a commit that builds.** New work begins at the newest commit
  known to compile, not at whatever the tip is.
- **Keep cores for the tip.** The tip's checks run on cores that no other work
  can take, and Ignite's own processes stop counting as load from outside it.

Checking once and landing with git alone remove the race for the tip. Reverting
a landing that breaks the build is what makes that safe. Verifying in batches
and starting from a commit that builds remove most compiling, and the cores kept
for the tip make its one build finish soon.

All of this applies to changes to the game and its tools. A change to Ignite
itself keeps today's landing, for the reasons under
[Changes to Ignite itself](#changes-to-ignite-itself).

## Related information

- [`docs/ignite.md`](ignite.md) is Ignite's design as built. This proposal
  replaces its sections "Landing" and "Express landings", and changes "Tip
  verification", "Build trees" and "Cores, memory and shared resources".
- `ignite.toml` in the repository root holds Ignite's settings, and
  `ignite/checks/` declares the checks. Every setting this proposal adds goes in
  `ignite.toml` and is shown where it is introduced.
- [Software Engineering at Google, chapter 23, "Continuous Integration"](https://abseil.io/resources/swe-book/html/ch23.html)
  describes checking a change before submission on a subset of tests, running
  the rest after submission, and building from the last commit known to pass.
- "Taming Google-Scale Continuous Testing", by Memon and others at ICSE-SEIP in
  2017, describes running tests after submission on batches of changes instead
  of on every change.
- ["Predictive Test Selection"](https://arxiv.org/abs/1810.05286), by Machalica
  and others at Meta, describes choosing which tests to run for a change from
  each test's record of failing on similar changes.
- "Keeping Master Green at Scale", by Ananthanarayanan and others at Uber,
  published at EuroSys in 2019, describes the main alternative: a queue that
  tests each change on top of the changes ahead of it.
  [Alternatives considered](#alternatives-considered) says why this proposal
  does not take that route.

## How a change is delivered today

This section describes the current path of a change, because the proposal is a
set of changes to it. It also introduces the terms the rest of the document
uses.

- **The daemon** is Ignite's one long-running process on each machine. It runs
  every heavy command, hands out cores, and records what each command cost. The
  **hub** is the daemon on the main machine, jarvis (a Mac with 18 cores), which
  also holds the shared database of tickets and results.
- **A check** is one named verification, such as one Rust test. A **family** is
  the group of checks that one command runs, such as all Rust tests. A family's
  declaration names its **inputs**, the files its checks read, and the **build
  products** it needs, such as the compiled test binaries.
- **The result cache** stores each check's outcome under its **input hash**, a
  hash of everything the check reads. A check whose inputs already passed is not
  run again, in any tree or on any machine.
- **The gate** is the set of checks a landing must pass before it is pushed. It
  has three parts. The **always set** is formatting, the project's lint rules
  for source files, and the type checks (clippy, `tsc` and pyre). Next are the
  checks whose own files the change edits. Last are the checks chosen by
  **score**, which is each check's record of failing after similar changes
  divided by its cost, up to a budget of 60 seconds of running checks.
- `ignite land` rebases the ticket tree onto the tip, runs the gate on the
  rebased commit, takes a short **lease** on `staging` from the hub (a lock held
  only for the push), and pushes. If the tip moved while the gate ran, it starts
  again from the rebase. The gate usually runs in a **landing tree**, one of
  three spare trees on jarvis that stay near the tip.
- **Tip verification** runs the checks the gate left out. It works in
  **rounds**: each round takes the newest tip, runs the best-scored checks
  within a budget of 300 seconds, moves `master` to that tip if they pass, and
  then runs the rest.
- When a check fails in a round, Ignite searches for the landing that broke it
  and files a **regression ticket**, a top-priority ticket to repair it. Until
  the repair lands, `master` does not move past the failure, and other landings
  are told that the check is known to fail.
- A change that touches only Ignite's own files takes the **express lane**: a
  fixed list of four commands that run in place of the gate, on cores the
  scheduler never gives to other work. Such a landing also moves the **release
  branch**, the branch every daemon builds itself from.
- **Exploration checks** search at random for bugs, such as generated game
  scenarios. They never block a landing or `master`, and this proposal does not
  change them.

## What one day showed

Every figure in this section is from the daemon's records on jarvis for
2026-11-19, from midnight to about 20:30 Pacific time, unless it says otherwise.

Volume and landing time:

- 135 landings reached `staging`, at most 15 in one hour, carrying 236 commits.
- 83 of them took the express lane and 52 went through the gate.

| Landing             | Count | Median  | 90th percentile | Longest |
| ------------------- | ----- | ------- | --------------- | ------- |
| Through the gate    | 52    | 7.8 min | 33 min          | 91 min  |
| By the express lane | 83    | 2.1 min | 5.6 min         | 9.6 min |

Time that sessions and `master` waited:

- Agent sessions spent 63 hours inside three commands: `ignite test` (18.7
  hours), `ignite check` (19.5 hours) and `ignite land` (24.9 hours). That is
  three sessions blocked for the whole day.
- A round of tip verification took a median of 5.2 minutes, 20 minutes at the
  90th percentile and 89 minutes at worst.
- One failing test held `master` back for over an hour in the afternoon.

Where the CPU went:

| Work            | Commands | CPU-hours | Share |
| --------------- | -------- | --------- | ----- |
| Compiling       | 1,479    | 57.0      | 75%   |
| Running checks  | 2,695    | 15.8      | 21%   |
| Listing checks  | 4,810    | 2.4       | 3%    |
| Everything else | 708      | 0.9       | 1%    |

- That is 25 CPU-minutes of compiling and 11 build commands for each landing.
- 8,053 of the 9,692 commands (83%) started with one core. A build that took
  over 30 seconds used 0.93 of a core at the median. Such builds took 115
  seconds at the median and 426 at the 90th percentile.
- Commands waited 49 hours for cores. Commands that list a family's checks
  waited 23 hours to do 2.4 hours of work.

### One landing, 91 minutes

The slowest landing changed some Python tools, a data file and one agent skill
file. Its record shows each problem this proposal addresses.

- **It ran Ignite's own test suite.** The declaration of that family names the
  skill files as an input of one package's tests, so a one-file skill edit put
  1,513 tests in the gate.
- **Every other Ignite change then invalidated it.** After a lost race, the gate
  runs again each family whose inputs the newly landed commits touch. Every
  Ignite change touches that family's inputs, so each attempt rebuilt the test
  binaries and ran the suite again, which took 6 to 11 minutes. Ignite changes
  were landing every 6 to 9 minutes.
- **Every one of its 178 commands ran on one core**, builds included, although a
  landing is the scheduler's highest class of work.
- **A test already failing on `staging` cost 24 minutes.** The first attempt hit
  a test that an earlier landing had broken. Showing that the test failed at the
  tip too took a second build, after 19 minutes in which the landing ran no
  command.

It landed on its seventh attempt. Ignite gives up after ten.

### Checking again after a lost race found nothing

The rule that makes a landing check again exists to catch two changes that each
pass alone and fail together. The daemon's records since 2026-11-18 hold 13
landings that lost at least one race and say how each attempt ended. Together
they ran 23 extra attempts. All 13 landed, and no later attempt found a failure.

That sample is small. It shows what the rule cost over two days and that it
caught nothing in them. It does not show how often two changes fail together,
which [Records](#records) below sets out to measure.

### Why the cost grows faster than the volume

Three costs rise faster than the number of landings. Each follows from the
current design, not from a slow machine.

**The race.** Suppose commits that invalidate a landing's gate arrive at random,
`r` per hour, and one attempt takes `g` hours. An attempt survives with
probability `exp(-r × g)`, so a landing needs `exp(r × g)` attempts on average:

```text
attempt length 7 minutes (the landing above took 6 to 11)

 4 invalidating commits an hour     1.6 attempts
 8 invalidating commits an hour     2.5 attempts   what that landing met
15 invalidating commits an hour     5.8 attempts
30 invalidating commits an hour      33 attempts
60 invalidating commits an hour   1,097 attempts
```

A landing that no other landing invalidates is unaffected, which is why most
landings are fine today. At four times the rate that the 91-minute landing met,
the average such landing needs more attempts than Ignite's limit of ten.

**Compiling.** A ticket tree holds the build of the commit its ticket started
on. `ignite land` rebases onto the tip and builds there, so the landing compiles
everything that landed since. For the 253 tickets that landed on 2026-11-18 and
2026-11-19, a median of 3 commits and 21 at the 90th percentile reached
`staging` between the ticket's claim and its landing. Every landing compiles the
same commits again, so the work is the number of landings multiplied by the
commits each one is behind, and both grow with the number of agents.

**The scheduler.** It divides the free cores among every waiting command. With
thirteen commands waiting, each gets one core, and a build that needs 300
CPU-seconds takes five minutes or more, however many cores the machine has.

## What Google and Meta do, and what Ignite already has

The proposal is easier to judge against the practice it borrows from. This table
names each part of that practice, what Ignite does today and what changes.

| Practice                                                     | Ignite today                                        | This proposal                                                                      |
| ------------------------------------------------------------ | --------------------------------------------------- | ---------------------------------------------------------------------------------- |
| Cache a check's result under a hash of its inputs            | Yes: the result cache                               | Unchanged                                                                          |
| Before submission, run the tests most likely to fail         | Yes: scores and the gate's budget                   | Unchanged                                                                          |
| After submission, run the rest on batches of changes         | Yes: tip verification                               | Adds a build step that runs first                                                  |
| Keep a branch that only holds verified commits               | Yes: `master`                                       | Adds `green`, which moves sooner                                                   |
| Submit without checking again when the main branch has moved | No: every landing checks on the exact tip           | Yes: [The change's verdict](#the-changes-verdict)                                  |
| Remove a change that breaks the build for everyone           | No need: every landing type-checks on the exact tip | Yes, and only then: [A broken build](#a-broken-build-is-reverted-and-nothing-else) |
| Start new work from the last commit known to pass            | No: new work starts at the tip                      | Yes: [The green branch](#the-green-branch)                                         |
| Compile each commit once                                     | No: each landing compiles what others landed        | Mostly: [Builds](#builds)                                                          |

Ignite already has the half of this model that costs the most to build: the
cache, the scoring, tip verification and the search for the landing that broke a
check. It pays for that half and for strict checking before every landing as
well. The proposal drops the strict checking and adds the missing parts that
make dropping it safe.

## What stays the same

These properties of Ignite are fixed, and each one ruled out some other design.

- `master` only ever points at a commit whose budgeted checks passed.
- Cargo stays the build tool, with incremental builds in trees at fixed paths.
  There is no shared cache of compiled output.
- Checks, families, input hashes, the result cache, scores, both budgets and
  exploration checks work as they do now.
- Agents keep the same commands with the same exit codes, and remain sessions in
  the desktop apps.
- Machines on different releases of Ignite, and on macOS and Windows, keep
  working side by side.
- A fix to Ignite never waits behind the work it is meant to fix.

## A ticket from start to landing

This is what an agent sees under the proposal. Two things in it are new and are
specified in the sections that follow. A **verdict** is the stored result of a
change's checks. `green` is a branch that points at the newest commit known to
compile.

```text
$ ignite start <ticket>
tree     .worktrees/tree-07  at green 91d07be

$ ignite check
verdict  passed: 41 checks, 24 from cache, 17 run in 38 s; stored for this tree

$ ignite land -m "Named counters draw as rows above the plaque."
verdict  passed for this tree 6 min ago on 91d07be
rebased  onto staging 3a9e4c1 without a checkout: 14 commits landed since
         91d07be
landed   7c1d2e0 on staging in 4 s; ticket closed; tree-07 released
```

Fourteen commits landed while the agent worked. None of them delayed the
landing, and the ticket tree compiled none of them.

## The change's verdict

A verdict is the record that one exact state of a change passed its checks. It
replaces the requirement that the gate pass on the tip.

A verdict is identified by three things:

- The git tree ID of the ticket tree's commit, which names the content of every
  file in it.
- The **base**, the commit of `staging` that the ticket's branch started from.
- A digest of the settings that decide which checks run, so that a change to
  those settings does not leave old verdicts standing.

Nothing in those three names the build of Ignite that computed the verdict.
Ignite is released hundreds of times a day, and a verdict outlives those
releases.

The checks behind a verdict are the gate's, chosen the same way: the formatters
run first and commit what they change, then the always set, then the checks the
change edits, then the checks chosen by score within the budget. Whatever the
budget leaves out goes to tip verification, as today.

Two things differ from today:

- **The checks run in the ticket tree as it is, with no rebase.** The families
  the change can affect are found from the paths that differ between the base
  and the tree. The tree has already built this state for the agent's own tests,
  so the checks usually build nothing.
- **The verdict is stored at the hub.** `ignite check` and `ignite land` on any
  machine look it up, so `ignite check` followed by `ignite land` runs the
  checks once.

A stored verdict:

```json
{"tree": "9f3c41d", "base": "91d07be", "settings": "c41e07a",
 "machine": "jarvis", "passed_at": "2026-11-19T18:44:29Z",
 "families": ["pytest", "markdown", "source"],
 "left_to_tip": 447, "always_not_run": []}
```

- `families` names the families whose checks were judged.
- `left_to_tip` counts the affected checks that the budget left for tip
  verification.
- `always_not_run` names always-set families that this machine cannot run, such
  as a browser type check on the Windows machine. The verdict is still complete
  for landing, and the tip's build step runs those families (see
  [Verifying the tip](#verifying-the-tip)).

### A check that was already failing

A check can fail for a change because it was already failing at the base. Today
the landing proves that by building the tip in a second tree. Under the proposal
the answer usually costs nothing.

- Git can produce a check's input hash for any commit without a checkout. The
  daemon computes the failing check's input hash at the base and looks it up in
  the result cache.
- A pass at the base means the change broke the check. The verdict fails.
- A failure at the base means the check was already broken. It is left out of
  the verdict as known to fail, and the hub is told, as today.
- No result at the base is the uncommon case. The check runs once at the base in
  a landing tree, as today, and the result is cached for every later change.

## Landing

In the common case `ignite land` runs no build and no check. This is how every
change lands except one that touches Ignite's own source (see
[Changes to Ignite itself](#changes-to-ignite-itself)). Its steps:

1. Check that the session holds the ticket and the tree has no uncommitted
   edits.
2. Find the verdict for the tree's commit. If there is none, run its checks now,
   as `ignite check` would.
3. Rebase the branch onto the tip without a working tree, as `git replay` does.
   The ticket tree's files are not touched.
4. Take the lease on `staging`, push, and release the lease. If the tip moved
   since step 3, repeat step 3, which takes about a second.
5. Record the landing on the ticket, close the ticket and release the tree.

**A clean rebase is all a verdict needs to stand.** A landing checks again in
only three cases:

- **The rebase conflicts.** The landing ends with the conflicting files, as
  today.
- **The base is over 12 hours old.** The landing rebases the ticket tree for
  real, computes a new verdict there, and goes on from step 3. `ignite sync`
  does the same ahead of time. None of the 253 landings of 2026-11-18 and
  2026-11-19 had a base that old: the longest time from claim to landing was 4.4
  hours.
- **The ticket was reverted before.** See
  [A broken build](#a-broken-build-is-reverted-and-nothing-else).

An earlier draft of this rule also checked again whenever a landed commit
touched a file the change touches. Measurement ruled it out: 73 of those 253
landings (29%) shared a file, other than documentation, with a commit that
reached `staging` between their claim and their landing. The files were ordinary
source files, not a few shared lists that could be exempted. A rule that sends
three landings in ten back through a build at the tip brings the race back.

```toml
[land]
mode = "verdict"          # "strict" checks on the exact tip, as before
max_base_age_hours = 12
```

What this removes from Ignite:

- The loop that runs the gate again when the tip moves, and its limit of ten
  attempts.
- The rules that decide which families to carry over from one attempt to the
  next.
- The use of a landing tree for an ordinary landing.
- The express lane's separate rules for when its commands run again (see
  [Changes to Ignite itself](#changes-to-ignite-itself)).

At 60 landings an hour, a push attempt of two seconds loses the race about once
in thirty, and the next attempt takes another second.

## Verifying the tip

Tip verification already works on batches: a round verifies the newest tip, and
landings that arrive during a round are covered by the next one. The proposal
adds a first step to each round and gives it cores of its own, because the tip
is now the only place where the combined code is compiled and type-checked.

A round has three parts, in order:

- **The build step.** Run the **build checks** at the tip: the checks that fail
  when the code does not compile or type-check (clippy, `tsc` and pyre). The
  repository marks their families with `builds = true`. Ignite's own lint is not
  one of them. When they pass, move `green` to the tip.
- **The budgeted checks.** Run the best-scored checks within the budget of 300
  seconds. When they pass, move `master` to the tip. This part exists today.
- **The remaining checks.** Run everything else. This part exists today.

Rules for the build step:

- It runs in a landing tree at the tip, as rounds do today, so it compiles only
  what its batch of landings changed.
- `tip_build_cores` cores are kept for it and never given to other work, as the
  express lane's cores are today. It asks the scheduler for more on top of
  those, in the highest class.
- A round with no `green` branch configured does what it does today.

```toml
[repository]
green = "green"

[machine.jarvis]
tip_build_cores = 4
```

`ignite status` reports each pointer:

```text
staging  3a9e4c1
green    3a9e4c1  the build checks passed 70 s after it landed
master   91d07be  14 commits behind; the round ends in about 3 min
```

## A broken build is reverted, and nothing else

The rule is one line: "the build is broken and nobody can work" is a revert, and
"a test failed" is not.

**Why a broken build is different.** Today a landing type-checks on the exact
tip, so a tip that does not compile cannot be produced. Under this proposal two
landings that each compile alone can produce one. While the tip stays that way,
`green` cannot move, no round can build anything, and every later landing piles
onto code that nobody can compile. A fix written by an agent takes tens of
minutes. Removing the landing takes one round.

**Why nothing else is reverted.** A revert pulls code out from under tickets
that already build on it, and it is a second way for work to move through the
system. A failing test stops nobody from working: the tip still compiles, and
`master` simply waits. So a check that fails outside the build step is handled
exactly as today. Ignite finds the landing that broke it and files a regression
ticket, and the fix lands on top.

A landing is reverted only when all of these hold:

- A build check failed at the tip, and a second run confirmed it.
- The search for the culprit, which is unchanged, ended at one landing.
- That landing changed none of Ignite's own source.
- The revert of the landing's commits applies cleanly at the tip.
- The build check passes at the tip with the revert applied. When the revert
  restores the check's inputs to a state that already passed, the result cache
  answers this and nothing runs.
- The round has reverted fewer than three landings. The limit stops a failure
  that the machine caused, not a commit, from reverting a whole batch.

When any of them fails, Ignite files a regression ticket, as it does for any
other check.

What a revert does:

- Ignite pushes one revert commit to `staging`, under the same lease a landing
  takes. Its message names the check that failed and the commit it reverts.
- The reverted ticket opens again at the highest priority, with the compiler's
  output.
- The ticket's work is not saved on a branch. Its commits are already in the
  history of `staging`, so a branch of them would rebase to nothing. The
  ticket's brief gives the command that applies the work again:
  `git revert --no-edit <revert commit>`.
- If the session that landed it has not started another ticket, the reopened
  ticket goes to that session, which still has the context. Otherwise it goes
  first in the order tickets are handed out.

```text
revert: feat: ward counts mana spent

This reverts commit 5e1b0aa.

After it landed, the tip did not build: clippy:workspace failed at the tip and
passed with this commit reverted. Its ticket is open again and holds the
compiler's output.
```

Rules for what follows a revert:

- **A revert commit is a landing with no ticket.** The next round verifies it
  like any other. It is never reverted itself: if the tip fails with it, Ignite
  files a regression ticket.
- **A ticket that depends on the reverted one** is blocked again if no session
  holds it, because a reopened ticket counts as unfinished. A session that
  already works on it is told to release its ticket, which is handed out again
  when the reverted ticket lands. Its tree could not build anyway: it started on
  the commit that broke the build.
- **A reverted ticket lands under a stricter rule the next time.** The landing
  rebases the ticket tree onto the tip for real and runs its checks there, which
  include the build checks. Then it pushes by the ordinary steps.

```toml
[revert]
broken_build = true
```

## The green branch

`green` is a branch that points at the newest tip whose build step passed. It
only moves forward.

- `ignite start` leases a tree at `green`, not at the tip, and `ignite sync`
  rebases onto `green`.
- A tip that does not compile therefore never reaches a new ticket. It delays
  `green` until its culprit is reverted, and nothing else.
- `master` keeps its meaning: the newest tip whose budgeted checks passed.
- A regression ticket is the exception: it starts at the tip. Its failure is at
  the tip, and when that failure is a broken build `green` is behind it.

```text
staging   A - B - C - D - E     E landed 20 s ago
green             C             the build step passed at C
master        B                 the budgeted checks passed at B
```

If `D` does not compile, a round reverts it with a new commit `F` after `E`, and
`green` moves from `C` to `F` once `F` builds. Tickets started meanwhile began
at `C`.

**A ticket never waits for `green`.** Suppose ticket B depends on ticket A. The
hub hands B out as soon as A lands, which is before any round has looked at A's
commit, so `green` does not hold A's code yet. Starting B at `green` would give
it a tree without the code it depends on, and waiting for `green` would add one
build step to every link of a chain of tickets.

So when `green` does not hold the commit a dependency landed as, `ignite start`
checks the tree out at that commit instead, at once, and says so:

```text
$ ignite start <ticket B>
tree     .worktrees/tree-04  at 7c1d2e0, where <ticket A> landed
note     green does not hold 7c1d2e0 yet; no tip round has checked it
```

- With several such dependencies the tree starts at the newest, which holds the
  others because the history of `staging` is linear.
- That commit passed its own ticket's checks, so it usually builds. It can still
  be one that a round later reverts. A's ticket then reopens, and B's session is
  sent a message.

## Builds

Compiling is three quarters of Ignite's CPU, so this is where most of the saving
is. The proposal does not add a shared compiler cache. It removes the builds
that repeat another tree's work.

- **A ticket tree builds only its own ticket.** Landing no longer rebases it, so
  it never compiles what other tickets landed. The exceptions are the cases
  under [Landing](#landing) and a deliberate `ignite sync`.
- **A tip round builds each landed commit once**, in a landing tree that last
  built an earlier tip, so the build is incremental.
- **A new ticket starts from a nearby build.** `ignite start` already copies the
  build of the tree nearest its starting commit by cloning its files, which uses
  no disk until they change.

Each landed commit should then cost its author's own incremental builds and one
build at the tip. On the day measured, compiling cost 25 CPU-minutes per
landing.

## Scheduling

The proposal changes three things in the scheduler. How waiting requests share
the free cores is a separate change that is already under way, and this document
does not repeat it.

- **Cores are kept for the tip's build step.** `tip_build_cores` are taken out
  of what the scheduler hands out, as the express lane's cores are. A command of
  the build step starts on them at once, whatever else is waiting or running.
- **The kept cores are lent to Ignite's own updates.** The express lane and the
  release build borrow idle cores when they start. Cores kept for the tip that
  no build step holds are lent to them too, so the reserve never slows an update
  to Ignite.
- **Ignite's own processes count as Ignite's.** The scheduler hands out the
  machine's cores less the load from outside Ignite. On 2026-11-19 that outside
  load included the hub's own process and a check's process left over from an
  older release. Both now count as Ignite's, so they no longer reduce what is
  handed out.
- **A command told to stop is ended after 60 seconds.** `ignite stop` already
  does this. A stop asked for in any other way, such as by a restart into a new
  release, did not: on 2026-11-19 one check had been stopping for over five
  hours and used most of a core the whole time.

## Changes to Ignite itself

Ignite is updated hundreds of times a day: jarvis restarted into 49 releases on
2026-11-19. Those updates are how a stuck queue or a bug in Ignite gets fixed,
so they have to deploy quickly even when the rest of the system is broken. That
requirement shapes four rules.

**Ignite is treated as if it lived in a repository of its own.** A real split
was considered and set aside: it needs a daemon that serves two repositories,
and nothing here depends on it. Inside one repository the same separation comes
from one rule: a change to Ignite's own source lands only with other Ignite
files, never with the game's code, tools or documents.

- Ignite's own source is every file under `ignite/` except the check manifests.
  The files that may land with it are the express lane's paths: `ignite/`,
  `ignite.toml`, `docs/ignite.md` and the efficiency log.
- `ignite land` and `ignite check` refuse a change that mixes the two, and say
  to split it into two tickets.
- A check manifest is not Ignite's source, so a new check family can still land
  with the tool that runs it.

```text
$ ignite land
refused  this change mixes Ignite's own source with other files
         rust/crates/beryl_cli/src/main.rs
next     land the files under ignite/ and the other files as two tickets
```

Every change to Ignite's source is then an express landing, and the remaining
rules keep the two kinds of change out of each other's way.

**A change to Ignite's own source lands the way it does today, whatever the
mode.** That covers the express lane and any other landing that would move the
release branch.

- Today such a landing compiles and tests Ignite on the rebased commit. After a
  lost race it runs the commands again when a landed commit touched one of its
  files or a Cargo build input. It builds the release executable while the
  commands run, and it moves the release branch on that pass.
- Landing by verdict would push a commit that nothing compiled and move the
  release branch to it. Changes to Ignite collide more than any others: 43 of
  168 Ignite-only landings on 2026-11-18 and 2026-11-19 shared a file with a
  concurrent landing.
- A release that does not build stops every update. No daemon retries it until
  the release branch moves again, and the next fix lands on top of the same
  broken code.
- The executable built ahead of time would be for a commit that never lands, so
  every deploy would wait for a build after its landing.
- The existing path is fast enough to keep: 83 landings on 2026-11-19 at a
  median of 2.1 minutes, and 2 of the 83 needed a second pass. Its checks take
  about a minute, so the race this proposal removes elsewhere hardly bites it.

**A tip round never reverts a change to Ignite.** Ignite's own lint is not a
build check, so it does not hold `green` back either. A revert would have to
move the release branch from a tip round, on the judgement of code that may be
the broken part. A broken Ignite build at the tip gets a regression ticket, as
today, and daemons stay on the release they run.

**A verdict is not tied to a release.** Its key names the tree, the base and the
gate's settings. Keyed by the build of Ignite, a verdict would die every few
minutes and be computed again before nearly every landing.

**New data is readable by the build before it.** A daemon that restarts into a
release takes over the commands and verb processes the older build started, and
a rollback opens a database a newer build wrote. So every field this proposal
adds to something stored or sent between processes is optional, and every
database change only adds.

What frequent releases gain from the rest of the proposal:

- Other landings take seconds, so a restart rarely has one to wait for.
- A change to the game no longer runs again because Ignite moved the tip. That
  was the whole cost of the 91-minute landing.

## Records

The design keeps the records Ignite already makes and adds little.

- A landing's reply says how its seconds divided between the verdict, the rebase
  and the push.
- A revert is recorded on the reverted ticket: the check, how the culprit was
  found, and the revert commit.

The comparison that decides between `strict` and `verdict` is session time. Each
revert costs its ticket a second landing under the stricter rule. Strict
checking cost 63 session-hours on the day measured.

The values the design aims for are estimates from the measurements above:

| Measure                                          | 2026-11-19        | Aim        |
| ------------------------------------------------ | ----------------- | ---------- |
| `ignite land` once a verdict exists, 90th pct.   | 33 min (gated)    | under 15 s |
| Push attempts per landing at 60 landings an hour | does not converge | under 1.1  |
| From a landing until `green` holds it, median    | not measured      | 3 min      |
| CPU-minutes of compiling per landing             | 25                | under 10   |

## Migration and compatibility

The change is switched on for the whole repository by one setting and switched
back without moving any data.

- **The switch.** `[land] mode` is a setting in `ignite.toml`, which is not part
  of Ignite's own source: changing it builds no release and restarts no daemon.
  A process reads the file when it starts, so the change that sets the mode also
  carries a change to Ignite's source, and the restart into that release is what
  switches the mode on. `"strict"` is today's behaviour.
- **Tickets in progress.** A ticket tree leased before the switch needs nothing.
  Its base is the commit it started on, and its first `ignite check` or
  `ignite land` after the switch stores a verdict.
- **The hub.** Verdicts are a new table in the hub's database, added by a
  migration, with one route to store a verdict and one to look it up. The hub
  runs the same release as every daemon, so it gains them first. A daemon that
  cannot reach the hub keeps a verdict in its own database and lands from that.
- **Machines on an older release.** The lease on `staging` and the push are
  unchanged, so an older daemon lands the old way beside newer ones. Only one
  machine runs a round for a given tip, as today. A round on an older daemon
  files regression tickets, and a round on a newer one reverts.
- **`green`.** The first round whose build step passes creates the branch by
  pushing it. Until then `ignite start` begins at the tip, as today.
- **Reverts alone can be switched off.** `[revert] broken_build = false` leaves
  a regression ticket as the only response to a tip that does not build.
- **The design document.** When this is built, the sections of
  [`docs/ignite.md`](ignite.md) named under
  [Related information](#related-information) are rewritten to match, and this
  document is deleted.

## Alternatives considered

These are the other ways to make landing scale, and why the proposal prefers
verifying after landing.

- **Keep checking on the exact tip and make it cheaper.** This is what Ignite
  has done since it was switched on: carrying results over between attempts,
  holding the lease only for the push, keeping one tree per landing. Each fix
  handled one case, and the number of attempts still grows exponentially with
  the rate of landings that invalidate each other.
- **A merge queue that tests each change on top of the ones ahead of it**, as
  Bors, GitHub's merge queue and Uber's SubmitQueue do. It keeps `staging`
  always passing. It also makes every landing wait for at least one full run of
  the gate at the tip, and it builds the tip for every landing, sometimes on a
  combination that is thrown away when a change ahead fails. Beryl already has a
  branch that always passes, `master`, so `staging` does not need to be one.
- **A shared cache of compiled output**, such as sccache or a Bazel remote
  cache. `docs/ignite.md` rejected this because it gives up Cargo's incremental
  builds, which are worth about 25 times on a small edit to a core crate. That
  still holds. This proposal removes the repeated builds instead of caching
  them.
- **Revert any landing that fails a check.** An earlier draft did this. It was
  cut down to a broken build for the reasons under
  [A broken build](#a-broken-build-is-reverted-and-nothing-else): a revert pulls
  code out from under tickets that build on it, and a failing test stops nobody
  from working.
- **More cores.** A larger machine shortens every build and leaves the race and
  the repeated builds growing as before.

## Manual QA

Run these on a scratch clone of the repository with its own hub and daemon, the
rehearsal setup that `docs/ignite.md` describes, so that nothing reaches the
real `staging`.

**Run this flow first.** It exercises the verdict, the landing, the build step,
the revert and `green` together:

- Start two tickets from `green`. In the first, rename a function in one file.
  In the second, add a call to the old name in another file. Run `ignite check`
  in both, and expect each to store a verdict. Land the first, then the second.
- Expect both landings to finish in seconds with no build.
- Expect the next round's build step to fail to compile, find the second landing
  as the culprit, and push a revert commit within a few minutes.
- Expect `green` never to point at the commit that did not compile, and the
  second ticket to be open again with the compiler's output and the command that
  applies its work again.

Landing:

- Land a change while a script pushes an unrelated commit to `staging` every ten
  seconds. Expect one or two push attempts and a landing in under ten seconds.
- Land a change after another landing edited one of its files without a textual
  conflict. Expect it to land by the ordinary rule.
- Set `max_base_age_hours` to a value smaller than the tree's age and land.
  Expect the landing to rebase the ticket tree, check again and say that the
  base was too old.
- Make a change fail a check that already fails at its base. Expect the verdict
  to leave the check out as known to fail.

Reverts:

- Land a change that compiles and breaks a unit test. Expect a regression ticket
  and no revert.
- Break the build with a landing whose revert conflicts with a later landing.
  Expect a regression ticket and no revert.
- Make a build check fail once and pass on its second run. Expect no revert and
  the check marked flaky.
- Land four changes in one batch that each break the build. Expect three reverts
  and one regression ticket.
- Start the reverted ticket again, apply its work with the command in the brief
  and land it without a fix. Expect the landing to rebase the ticket tree, run
  the build checks there and refuse the landing.
- Revert a ticket that another session's ticket depends on. Expect that session
  to be told to release its ticket.

Starting:

- While the tip does not compile, run `ignite start`. Expect a tree at `green`,
  behind the tip, that builds.
- Land ticket A and start ticket B, which depends on it, before the next round.
  Expect B's tree at A's commit at once, with the note that no round has checked
  it, and A's code present.
- Work a ticket while twenty commits land, then land it. Expect the ticket tree
  to have compiled no crate the ticket did not touch.

Scheduling:

- Fill the scheduler with builds, then land a change. Expect the round's build
  step to start at once on the cores kept for it.
- Tell a running check to stop by a path other than `ignite stop`, and prevent
  its process from exiting. Expect it to be ended after 60 seconds.
