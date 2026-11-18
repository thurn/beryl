# Ignite: builds, checks and delivery for Beryl's agents

Ignite is a build, check and delivery system written in Rust for Project Beryl.
It replaces Enact, the tool that manages worktrees, the task queue and CI, and
the Python programs in `tools/` that choose, run and judge checks. Only coding
agents use it.

Ignite exists to shorten one number: the wall-clock time from a ticket being
claimed to its work **landing**, which means its commits reaching the shared
`staging` branch. It does that by owning every build, test and check an agent
runs, on every machine, so that one program per machine can see all of the work
and avoid doing any of it twice.

The design rests on five decisions:

- **Ignite owns the agent's edit-build-test loop**, not only the checks before a
  push. That loop is where most CPU and about a third of a session's time go.
- **Cargo stays the build tool.** Ignite adds, above Cargo, what hermetic build
  systems do well: declared inputs, a hash of them, and never rerunning a check
  whose inputs already passed.
- **A check blocks a landing when its past failures on similar changes justify
  its cost**, however busy the machine is. Under load Ignite stops background
  work, and the set of checks stays the same.
- **Workers are sessions in the Claude and Codex desktop apps**, where a person
  can watch and steer them. One small, cheap session per machine hands them
  tickets, and Ignite's daemon makes every decision.
- **The system repairs itself as ordinary work.** No scheduled agent fixes CI. A
  regression goes back to the ticket that caused it, and improvements to the
  workflow get a guaranteed share of the workers.

Enact and the old check tools are removed in one switch. All other work on Beryl
is halted until then, so everything this document describes is built before work
resumes.

## Related information

- `docs/checks.md`, deleted at the switch (read it with
  `git show <commit before the switch>:docs/checks.md`), described the fast
  check, slow CI, the sweeps and the CPU token pool that Ignite replaces. Its
  measurements of flaky checks, budgets and incremental builds are the evidence
  for several decisions here.
- [`docs/efficiency-dashboard.md`](efficiency-dashboard.md) designs
  `just efficiency`, the tool that produced most numbers in this document. It
  stays, and reads Ignite's records in place of the logs it reads today.
- `.enact.toml` in the repository root holds Enact's configuration. Its comments
  record the measurements behind keeping worktrees at fixed paths.
- `~/enact/docs/design.md` records Enact's design decisions, including the
  ticket service's contract (storage, readiness, leases and the change feed),
  which Ignite's hub carries over.
- `~/enact/skills/` holds the role prompts and the `/enact` and `/enact-plan`
  skills that the `/ignite` skill replaces.
- [`docs/benchmarking.md`](benchmarking.md) describes the instruction-count
  gates for the game's performance. [Rebuild counts](#rebuild-counts) borrow
  their method of measurement.
- [Claude Code's model configuration](https://code.claude.com/docs/en/model-config.md)
  documents `autoCompactWindow`, the setting the dispatcher relies on.

## What the measurements show

This section shows where time, CPU and money go today, because those numbers
decide what Ignite is. It starts with the names the numbers use.

### How work flows today

- A **ticket** is one task in the project's tracker, reached with the `tk`
  command. An **epic** is a ticket that groups child tickets.
- Agent sessions run in the Claude and Codex desktop apps on three machines:
  jarvis (a Mac with 18 cores and 64 GB, the main development machine), multivac
  (a MacBook Pro) and cygnus (a Windows PC).
- Enact gives each ticket a git worktree. `enact push` rebases the work, runs
  the **fast check** (about a minute of checks chosen for the change) and pushes
  to the `staging` branch.
- **Slow CI** then runs on `staging`, and `master` moves forward only when it
  passes. A failure starts a search for the commit that caused it and files a P0
  repair ticket, called a Steward ticket.
- A **browser sweep** every three hours and a **nightly** run hold the checks
  that are too slow for either.
- An **orchestrator** is an agent session that hands an epic's tickets to worker
  sessions and waits for them.
- A heavy command takes a **CPU lease**, permission to use some cores, from a
  pool of one token per core. A **CPU sampler** records each minute's CPU by
  kind of work.
- `just efficiency` reads session transcripts and those logs into a local
  database, the efficiency store, and reports from it.

Every figure below was measured on jarvis on 2026-11-17, with `just efficiency`
and `just cpu report`. The workflow changes quickly, so read each as the state
on that day.

### The CPU goes to agents' own compiles

The CPU sampler's last 72 hours:

- The machine used 10.6 of 18 cores on average and was saturated (90% or more
  busy) in 19% of minutes. It is short of cores in bursts, not on average.
- Commands that agent sessions ran themselves used 60% of the CPU. The fast
  check and slow CI together used 7%.
- Compiling Rust used 42% of the CPU. Running Rust tests used 6%.
- Python, Node, git and shell processes used 19%.

A better choice of tests before a push therefore acts on about 7% of the CPU.
The large pool is the building that sessions do while they work, which no tool
coordinates today.

### Half of a session's time is building, checking and delivering

Sessions that implement tickets spent their minutes like this over 7 days:

- 21% in the model.
- About 17% reading, searching and editing files.
- About 30% in their own builds, tests and Beryl 3D tools, including 5% waiting
  for cores.
- About 13% in `enact push`, the fast check and git.
- About 6% in Enact and ticket commands, including waits for Enact's lock.
- The remaining 13% in waits on background work and many small tools.

Across tickets of every kind, the median took 31.5 minutes of agent time, 45.6
core minutes and $4.37 at API prices. Agent time counts the minutes a session
was active on the ticket. The 36-minute figure further down is wall time, for
implementation tickets only.

### Cold builds are the expensive ones

The log of CPU leases for 3 days shows where compile time goes:

- Of 6,871 `cargo build` runs by agents, 4,982 took under 5 seconds because
  nothing needed compiling.
- 1,090 runs took over 30 seconds, and those carried 87% of the build time.
- More than half of agents' build time (176 of 324 lease core-hours) was in
  worktrees outside the ten reused directories. A worktree at a new path cannot
  reuse an incremental build.

`.enact.toml` records how much that matters. After a one-function change to
`beryl_kernel`, the test build cost 18 CPU-seconds in a tree that had built the
previous state, and 421 to 544 CPU-seconds in a tree at a new path.

### Most of a ticket's lead time is waiting

For 452 implementation tickets closed in 7 days:

| Phase                       | Ticket-hours | Share | Median |
| --------------------------- | ------------ | ----- | ------ |
| Blocked on a dependency     | 1,529        | 56%   | 0 min  |
| Ready, waiting for a worker | 656          | 24%   | 8 min  |
| Being worked                | 551          | 20%   | 36 min |

Most tickets start within minutes. A minority sit in long dependency chains, and
tickets grouped into one session's queue waited five times longer than the rest.

### Two tickets in five are repairs

The checks filed 321 tickets by themselves in 7 days, beside 518 implementation
tickets. That is 38% of the total:

- 97 to repair a failed slow-CI run.
- 96 to repair a failure that a later sweep or nightly run found.
- 57 to delete a check that failed and then passed.
- 49 to cut or speed up checks that ran over a time budget.
- 22 for emergencies, Enact's name for a run that nothing can advance.

Many slow-CI failures were not regressions. They were randomly generated oracle
scenarios (card scenarios replayed in XMage and Forge, two other Magic rules
engines) finding bugs older than the commits under test. Those scenarios are the
best-value checks in the project: 111 real bugs for 26 core-hours in the week.
Each find was treated as a broken build all the same. Slow-CI failures of every
kind held `master` back for 1,728 minutes in the week and spent 2,198
core-minutes on bisection.

The check before a push is valuable: it stopped 128 real defects in 675 pushes
over 3 days, about one push in five. The median push took 146 seconds.

### Orchestration is expensive only because of its context

Enact's orchestrator sessions cost $591 in the week, 7% of all spend:

- They made 6,097 requests, nearly all on Opus 5.5.
- The average request carried 407,802 tokens of context, because the session
  that planned an epic also ran its loop for up to two days.
- $496 of the cost was rereading that context. Output cost $35.

### Two of three machines are unmeasured

Jarvis runs Claude Code sessions, and multivac and cygnus run Codex sessions.
About 285 tickets ran on multivac and cygnus, and the efficiency store holds no
data for them, because moving data between machines is a manual export and
import.

### One rules fix, step by step

Three recent fixes to the rules engine show the same picture at the scale of one
ticket. Each changed a few lines of `beryl_rules`, added a regression test and
landed as one commit. Together they took 108 agent minutes:

| Share | Where it went                                                |
| ----- | ------------------------------------------------------------ |
| 24%   | `enact push`: rebasing, the fast check, pushing              |
| 19%   | The model                                                    |
| 19%   | Reading, searching and editing files                         |
| 21%   | The card tools, the count gate and other `just` commands     |
| 9%    | Enact and ticket commands, 4 points of it waiting for a lock |
| 4%    | Running Rust tests                                           |
| 4%    | Everything else                                              |

What the steps cost:

- Claiming the ticket and creating its worktree took 45 to 110 seconds, as two
  or three commands.
- `cargo test -p beryl_rules`, which runs the new regression test, took 14
  seconds. The agent's own test is cheap.
- One push failed after 99 seconds, the agent reformatted its files, and the
  push that followed took 239. The other two took 445 seconds and 17 minutes.
  The fast check's Rust steps build for the whole workspace, and every crate
  downstream of `beryl_rules` has changed.
- Recording the hand-off and closing took one to three more commands and up to a
  minute each.

Writing the fix is a fifth of the time. Delivering it is a quarter.

## What Ignite must achieve

These are the measures Ignite is built to move, with the starting values above.
The expected values are estimates from those measurements, not promises.

- **Cycle time**, from a ticket's claim to its landing: a median of 36 minutes
  today for implementation tickets, the "being worked" row above. Expected:
  about 26 minutes, the result of the worked example under
  [Two tickets, today and under Ignite](#two-tickets-today-and-under-ignite).
- **Waiting for a worker**: 24% of lead time today. Expected: under 10%, because
  hand-offs follow machine load and every machine draws on one queue.
- **Repair share**, the repair tickets as a share of all tickets: 38% today.
  Expected: under 10%.
- **`master` lag**, the minutes `master` is behind `staging`: 1,728 a week
  today. Expected: under 300.
- **Dispatch cost**: $591 a week today. Expected: under $60.
- **Fleet coverage**: one machine of two measured today. Required: both.

Throughput is the first goal. Test fidelity is a constraint: every check that
runs today still runs somewhere unless its record shows it catches nothing.

## Overview

This section names the parts. Each has its own section below.

- The **daemon** is one long-running Ignite process per machine. It hands out
  cores and build trees, runs every heavy command, and records what each cost.
- The **hub** is the daemon on jarvis. It also holds the shared database
  (tickets, check results, records from every machine) and serves it to the
  other daemons over the Tailscale network.
- A **build tree** is a git worktree at a fixed path that keeps its build
  directories between tickets, so Cargo's incremental builds keep working.
- A **check** is one named verification with declared inputs, such as one Rust
  test, one screenshot or one oracle scenario. A **check family** is a group of
  checks that one command runs, such as all Rust tests.
- The **result cache** remembers each check's outcome under a hash of its
  inputs. A check whose inputs already passed is not run again, in any tree or
  on any machine.
- **Landing** is delivering a ticket's commits to the `staging` branch. The
  **landing gate** is the set of checks that must pass first.
- The **tip** is the newest commit on `staging`. **Tip verification** runs the
  checks the gate left out, once per tip, and moves `master` forward.
- A **worker** is a desktop-app agent session that implements tickets. The
  **pool** is a machine's set of workers. The **dispatcher** is one small
  session, per machine and per desktop app in use there, that hands tickets to
  workers when the daemon tells it to. A **hand-off** is one ticket given to one
  worker.

An agent meets all of this through one command, `ignite`.

## The agent's commands

An agent's whole workflow is a handful of verbs. Each prints one compact result
and tells the agent what to do next.

A ticket from claim to landing looks like this:

```text
$ ignite start Tq44s.9.48
ticket   Tq44s.9.48  Beryl 3D manual table: counter names, untap lock
tree     /Users/dthurn/beryl/.worktrees/tree-07  at staging 8c1f2ab
title    ⚒️ [Tq44s.9.48] Counter names, untap lock
files    rust/beryl3d/crates/beryl_scene/src/table/badges.rs  (+4, below)
needs    Tq44s.9.31 landed 2f0c1d9: "badges draw from one record pool"
next     cd to the tree, set the title, read the files, then edit
```

```text
$ ignite test badges_manual
built    b3d-tests in 41 s (3 crates recompiled)
ran      12 checks: 11 passed, 1 failed
FAIL     b3d-test:beryl_scene::badges_manual::lock_sits_top_left
         tests/all/badges_manual.rs:88  left: 14  right: 12
rerun    ignite test lock_sits_top_left
```

```text
$ ignite land -m "Named counters draw as rows above the plaque."
rebased  onto staging 91d07be (4 commits landed since start)
gate     38 checks: 31 from cache, 7 run, all passed in 52 s
landed   3a9e4c1 on staging; ticket closed; tree-07 released
title    ✅ [Tq44s.9.48] Counter names, untap lock
next     nothing; wait for your next ticket
```

The verbs an agent uses while working:

- `ignite start <ticket>` claims the ticket, leases a build tree at the tip,
  starts building it, and prints the ticket with what its dependencies left
  behind. `ignite start "<summary>"` files a ticket first, for a session that
  was given a task and no ticket.
- `ignite test <names>` runs the checks the agent names, in its tree.
- `ignite check` runs what the landing gate would run on the tree as it is.
- `ignite sync` rebases the tree's branch onto the tip.
- `ignite land -m "<note>"` rebases, runs the gate, lands the commits, records
  the note for dependent tickets, closes the ticket and releases the tree.
- `ignite run -- <command>` runs any other heavy command with cores from the
  daemon, and records it. A `cargo` on the tree's `PATH` does the same for plain
  `cargo` calls.
- `ignite ticket <verb>` creates, shows, lists, links and comments on tickets.
  `ignite ticket children <epic> --open` lists an epic's unfinished tickets with
  their assignees, blockers and last comments, and `ignite ticket dep list <id>`
  shows what a ticket waits on and what waits on it.
- `ignite why <check>` prints a check's record: its cost, its failures, where it
  runs and why.
- `ignite status` and `ignite doctor` print the machine's state and health.
- `ignite tell <ticket | session | --all> "<text>"` sends a correction to work
  that is under way. See [Steering work in flight](#steering-work-in-flight).
- `ignite close -m "<reason>"` closes the ticket without landing anything, for a
  ticket that is obsolete, already done or needs no change to the repository,
  and releases the tree.
- `ignite release -m "<note>"` gives the ticket back unfinished and keeps its
  work. See [Releasing work](#releasing-work).
- `ignite guide <topic>` prints the guidance for one situation. See
  [Guidance for agents](#guidance-for-agents).

`ignite prime` prints the whole workflow in under 60 lines. A worker's first
prompt is one sentence that names `ignite start`, so the role files that every
session reads today are deleted.

### Output rules

Every command follows the same contract, because every tool result is read again
on each later request:

- The first line is the verdict. Failures follow, each with the check's name, a
  file and line, the first lines of its message and the command that reruns it
  alone.
- Default output is at most 40 lines. The full output is in a file whose path is
  printed.
- `--json` prints the same data as one JSON object.
- No command needs polling. A command that would outlive the agent's tool
  timeout returns before it with a handle, a short name for the operation still
  running. `ignite wait <handle>` blocks until it finishes.

```text
$ ignite land -m "..."
waiting  gate is queued behind 2 landings (9 minutes so far)
next     ignite wait L-5521
```

- A comment or edit that someone else made to the session's ticket since its
  last command is printed first, as `changed` lines, at most five. This is how a
  correction reaches work that has already started.

```text
$ ignite test badges_manual
changed  comment by derek: Do not wait for verification on another machine.
         Land when the gate passes.
built    b3d-tests in 12 s (1 crate recompiled)
ran      12 checks: all passed
```

Exit codes are the same for every verb:

| Code | Meaning                                               |
| ---- | ----------------------------------------------------- |
| 0    | Success                                               |
| 1    | Usage or configuration error                          |
| 2    | A check failed; the agent can fix it                  |
| 3    | A conflict: a rebase conflict or a lost claim         |
| 4    | The hub or the git remote cannot be reached           |
| 5    | Still running; the output names the handle to wait on |

## Two tickets, today and under Ignite

This section follows two real kinds of ticket through the design, to show where
the time is expected to go. The "today" figures are measured. The figures for
Ignite are estimates.

### A rules-engine fix

The three fixes under [One rules fix, step by step](#one-rules-fix-step-by-step)
averaged 36 minutes each. Step by step:

- **Claiming.** Today: `enact start`, `enact worktree create` and `enact brief`,
  45 to 110 seconds. Under Ignite: one `ignite start`, which returns in seconds
  with the tree already at the tip and its build running.
- **Running the new test.** Today: 14 seconds, and it stays that. Ignite builds
  `beryl_rules`' test binary only.
- **Formatting and lints.** Today one fix lost a 99-second push to a mistake
  that reformatting repaired. Under Ignite the formatters are the first of the
  always set and answer in seconds, in `ignite check` or at the start of
  `ignite land`.
- **Landing.** Today: 4 to 17 minutes per push that succeeded, with the fast
  check building for every crate downstream of `beryl_rules`. Under Ignite the
  landing tree is at the tip, clippy type-checks the workspace incrementally,
  the gate reuses the agent's own results for `beryl_rules` and runs the card
  scripts that have failed on such changes before. The tests of `beryl_session`
  and `beryl_cli` are built once, at the tip. Expected: about a minute.
- **Closing.** Today: a hand-off comment, a sync and a close, one to three
  commands and up to a minute each, plus waits for Enact's lock. Under Ignite:
  nothing, because `ignite land` did it.

The model, the reading and the card tools are untouched, about 60% of the
ticket. The expected ticket is about 26 minutes.

### A Beryl 3D change

One change to Beryl 3D, the native client in its own Cargo workspace, took 72
minutes: counter names, a lock mark and a refusal animation, with new tests and
a recording.

| Minutes | Where it went                                                |
| ------- | ------------------------------------------------------------ |
| 19      | The model                                                    |
| 16      | Editing files                                                |
| 13      | Rust tests                                                   |
| 13      | Beryl 3D's tools: recording, interface tests, the frame gate |
| 5       | Git and the push                                             |
| 6       | Everything else                                              |

What changes for this ticket:

- **One build serves every tool.** Today each `just b3d` command starts with a
  Cargo run of its own. Those commands asked for CPU leases 3,215 times in 3
  days and waited 14 hours for cores. Under Ignite they name one build product,
  built once per state of the tree.
- **Recordings and interface tests are cached by input.** A recording whose
  inputs the edit did not touch is not replayed, in this tree or at the gate.
- **The tree starts built.** The first test run does not wait for everything
  `staging` changed since the tree was last used.
- **Looking is not shortened.** Reading a drawn tree, a diff or a picture to
  judge the change takes the model's time, which Ignite does not touch.

The expected saving is in the 26 minutes of tests and tools, by perhaps a third,
which makes this ticket about 63 minutes. Tickets like this one gain less than
small fixes do, because more of their time is thought.

## Processes

Ignite is one executable. This section says which processes exist and how they
survive each other's restarts.

- **The client** is the `ignite` command an agent types. It sends the request to
  the machine's daemon over a local socket (a named pipe on Windows), prints the
  reply and exits.
- **The daemon** starts at login on every machine. It keeps its state in a local
  SQLite database, so a restart loses nothing.
- **A supervisor** is a small process, started from the same executable, that
  runs one command. It is the command's parent: it waits for it, collects its
  resource use from the operating system and writes the result to a file. The
  daemon watches supervisors, so it can restart while commands keep running.
- **The hub** is the daemon on jarvis with one more duty: serving the shared
  database over HTTPS on the Tailscale network.

How the executable is installed and kept current:

- `ignite install`, run once per machine from a checkout, builds the executable
  with plain Cargo, registers the machine with the hub and sets the daemon to
  start at login. Registering stores a write token in the user's configuration.
  The hub accepts only requests that come over the Tailscale network with a
  registered token.
- After that the daemon runs the executable built from the release branch,
  `repository.release` in `ignite.toml`, or `master` when that is unset. When
  the branch moves and Ignite's own source changed, each daemon builds the new
  executable in a tree of its own and restarts itself. Daemons look at the
  branch every five minutes, so a fix pushed to it by hand deploys too.
- A landing that changed Ignite's own source moves the release branch to the
  landed commit once `repository.release_family` (the `ignite` family) has
  passed on it. A fix to Ignite therefore reaches every machine minutes after it
  lands, without waiting for tip verification of everything else, which an
  Ignite bug can itself be blocking.
- Each build is stored under its commit, so a new executable never replaces a
  file that is running. Supervisors finish under the version that started them.
- The hub updates first. A daemon whose version differs from the hub's finishes
  the commands that need it, updates and reconnects before it asks for new work.
  Verbs that only wait, such as `ignite dispatch` and `ignite wait`, do not hold
  an update back.
- Each record names the Ignite commit that produced it.

How a broken build or a crash is survived:

- A panic in any of the daemon's loops writes a crash record to the Ignite
  home's `crash` folder and ends the process, so that the login service starts
  it again. A loop that died while the process lived on would stop that part of
  Ignite without anyone noticing. A panic in a verb fails only that verb, and
  leaves a crash record too.
- A daemon that has run for ten minutes marks its build last-good
  (`bin/last-good`). A build that starts three times in five minutes is rolled
  back to the last-good build and is never installed again.
- `ignite install` also installs a monitor, a second login service that runs
  `ignite monitor` from the last-good build every two minutes. It restarts a
  daemon that does not answer, rolls back a build that crash-loops, and when
  neither helps for 15 minutes, writes `justiciar-needed.json` in the Ignite
  home and posts a notification. The monitor has no agent logic and makes no hub
  calls.
- The daemon reports each crash record, and a loop that keeps failing, to the
  hub as a justiciar report. See [The justiciar](#the-justiciar).

How a command knows who is asking:

- The client reads the session's ID from the environment the desktop app gives
  its sessions, or from `IGNITE_SESSION` when that is set.
- The tree is the build tree that holds the working directory.

A removed tree, a finished ticket or an expired lease ends every process that
was started in that tree.

## Build trees

Build trees keep Cargo's incremental builds usable, which the measurements above
show is worth about 25 times on a small edit to a core crate. This is the reason
Ignite does not share compiled output between trees: `rustc` discards an
incremental build made at another path, and no shared cache offers one.

Each machine has a fixed number of trees of two kinds:

- A **ticket tree** is leased to one ticket from `ignite start` until its
  landing or release.
- A **landing tree** stays at the tip. Landings and tip verification build in
  it, so each builds only what one change added. A ticket tree last built
  whatever the tip was when its ticket started, often 20 to 30 commits earlier.

```toml
# ignite.toml, per machine
[machine.jarvis]
ticket_trees = 24
landing_trees = 3
trees_disk_gb = 400
```

Rules for trees:

- Ignite never builds in a directory outside this set. When every ticket tree is
  leased, `ignite start` waits and says so, and the dispatcher hands off no
  ticket.
- `ignite start` leases a free tree and checks out a branch at the tip. For a
  ticket expected to compile Rust, it starts the usual builds at once, and the
  agent reads the ticket while the tree builds.
- The choice of tree follows the ticket. A ticket expected to compile Rust gets
  the free tree whose last build is nearest the tip. Any other ticket gets the
  free tree with the oldest build, so that it does not take a prebuilt tree from
  a ticket that needs one. See
  [Light and heavy tickets](#light-and-heavy-tickets).
- While cores are idle, the daemon keeps one free ticket tree built at the tip,
  so the next lease starts with nothing to compile.
- A tree keeps the directories that its build products declare as their output,
  each created when a command first needs it. Today those are `rust/target`,
  `rust/beryl3d/target` and `ui/node_modules`, and none of them is named in
  Ignite's code. A tree that has only held documentation or Python tickets uses
  little disk.
- To hold the machine under `trees_disk_gb`, the daemon deletes builds that a
  newer build of the same unit replaced, then the build directories of the free
  trees used longest ago.
- Releasing a tree first saves what it holds, then resets it to a clean checkout
  and keeps its build directories. See [Releasing work](#releasing-work).
- Releasing also ends every process whose working directory or executable is
  inside the tree, whether or not Ignite started it, except the caller and its
  ancestors. A server that an agent started by hand and forgot is the usual
  case.
- Only the session that holds a tree's lease may use it. Every `ignite` verb and
  the tree's `cargo` shim refuse any other session with exit code 3, so a
  session that was replaced cannot go on working in a tree that now belongs to
  someone else.
- The daemon keeps the main checkout's local `staging` and `master`
  fast-forwarded to the remote, and only while that checkout has no local
  changes. Sessions start there and read its instruction files, and nobody works
  in it.

### Releasing work

A ticket can leave a session unfinished: the worker releases it, or the daemon
releases it for a worker that has gone. Either way the work is kept, because
work lost at release has to be done again from nothing.

- Release commits whatever the tree holds that is not committed, as one commit
  marked as saved at release, and pushes the ticket's branch to the remote as
  `ignite/<ticket>`. If that push fails, the release fails and the session keeps
  the ticket.
- The next `ignite start` of the ticket, on any machine, checks that branch out
  and says so in the brief: how many commits it holds, who made them, and
  whether the last one is unfinished work saved at release.
- The branch is deleted when the ticket lands or closes.
- `ignite release --needs <what>` also records why the ticket was given back.
  `--needs macos`, or the name of a machine, keeps it for a machine that has
  what it needs. `--needs person` sets it aside for the person: no worker is
  handed it, the dashboard lists it, and the note must state the exact need and
  what was tried.
- A ticket that waits on another ticket's work is released with a dependency on
  that ticket, never held while its session watches `staging`. It is handed out
  again when the other ticket closes.

```text
$ ignite release --needs person -m "Needs the App Store Connect key. Tried the
  keychain and the CI secrets; neither holds it."
saved    2 commits and 3 uncommitted files as ignite/Tq44s.7.12
released Tq44s.7.12; tree-04 is free; do not use this path again
title    ⛔ [Tq44s.7.12] Sign the iOS build
```

### Builds

A **build product** is a named build that checks need, such as the Rust test
binaries, the clippy pass or the `beryl_cli` executable. It is declared beside
the check families, with its command, its inputs and the directories it writes:

```toml
[product.rust-tests]
build = "cargo nextest run --no-run {packages}"
dir = "rust"
inputs = ["crate-closure"]
outputs = ["rust/target"]
```

Ignite's code names no path, crate, tool or branch of Beryl's. Everything
specific to this repository is in `ignite.toml` and these declarations, so
merging a workspace or dropping a directory is an edit to a declaration.

The daemon runs builds so that sessions do not compete inside one tree:

- One build runs per target directory at a time, since Cargo locks the
  directory. Requests for the same product in the same tree state join the
  running build.
- A request for a product the tree already holds is answered at once, without
  starting Cargo. This covers the 4,982 runs in 3 days that compiled nothing.
- A build that exited with a failing code is not run again for the same inputs:
  later requests get the same failure and its output until an input changes or
  the tree is leased again. A build that a signal ended, or that hung, is tried
  again on the next request.
- Checks waiting for a build hold no cores.
- The gate builds test binaries only for the packages whose tests it will run. A
  `beryl_rules` change does not build `beryl_session`'s tests before landing
  unless the gate chose one of them.

Building a subset of packages must not change the features of the dependencies
they share, or Cargo would rebuild those. Ignite checks this whenever a manifest
or the lock file changes, by comparing `cargo tree -e features` for each subset
it uses with the whole workspace's, and fails the landing if they differ.

Every compiler process runs through Ignite as Cargo's `RUSTC_WRAPPER`, which
records each unit's cost and whether it was rebuilt.

## Cores, memory and shared resources

The daemon is the only place that decides which heavy command runs now. It
replaces the Python token pool, the browser page slots and Enact's locks.

A command asks for cores in one of five classes. A lower class starts nothing
while a higher one waits:

| Class   | Used by                                             |
| ------- | --------------------------------------------------- |
| `land`  | Landing gates                                       |
| `agent` | Commands a worker runs itself, and tree prebuilds   |
| `tip`   | Tip verification and finding a regression's culprit |
| `batch` | Long experiments, such as AI tournaments            |
| `idle`  | Exploration checks, cache audits, rebuild counts    |

Rules for cores:

- A command runs with as many threads as it was granted (`CARGO_BUILD_JOBS`,
  `NEXTEST_TEST_THREADS`, `RUST_TEST_THREADS`, `IGNITE_CORES`).
- One command takes at most two thirds of the machine's cores. Waiting commands
  of one class share what is free evenly.
- The daemon reads the machine's total CPU use from the operating system about
  once a second. What its own commands do not account for belongs to processes
  outside Ignite, such as the desktop app, and it grants only what is left. The
  reading compares the operating system's counters with the previous reading,
  and reports no reading, not zero, when none of them moved. The daemon counts
  every granted core as used by its own commands, so when a command leaves its
  cores idle the outside load shown by `ignite status` is a lower bound.
- `land`, and a class with a floor, always get one core while they hold none,
  even when processes outside Ignite fill the machine, so a landing and `master`
  always make progress.
- `tip` and `batch` each have a floor of cores per machine that higher classes
  cannot take, so `master` keeps moving and an experiment is slowed but never
  stopped.
- `idle` work starts only while the machine is under 70% busy, and is the first
  thing stopped when a higher class waits for cores, or for a resource the idle
  work holds. Stopping sends `SIGTERM`, then `SIGKILL` ten seconds later. While
  an update waits for commands to finish, idle work is stopped and none starts.
- Time spent waiting for cores is recorded apart from a command's own time.
- Classes are enforced by what the daemon grants, never by the operating
  system's priority. Work run at low priority on this machine once got almost no
  CPU for hours.
- A grant can change while its command runs. A Cargo build that only compiles
  runs under a jobserver its supervisor keeps at the grant's size, so it gives
  cores back, one per second, while a higher class waits, and takes them again
  afterwards. A command that can use more threads is told when its grant grows,
  through the file `IGNITE_CORES_FILE` names.

The daemon also records each command's peak memory, and delays a command whose
usual peak (the median of its last nine runs) is more than the free memory,
while something else runs.

A daemon that restarts while commands run registers their cores and resources
again, so an exclusive resource is not handed out twice.

Some things are scarce in other ways: headless browser pages, the oracle
engines, the benchmark virtual machines. A check family names the **resources**
it needs, and the daemon holds each to its capacity:

```toml
[machine.jarvis.resources]
browser-pages = 9
oracle = 1
bench-vm = 1
```

A long experiment registers the same way, so the scheduler counts its cores:

```sh
ignite run --class batch --ticket Tix1t.12 -- just tournament --games 2000
```

## Cost in instructions

Ignite measures what work costs in CPU instructions retired, not in seconds,
wherever the operating system reports them. Seconds on this machine measure its
load.

macOS counts the instructions each process executes and reports the total when
the process exits. Six compiles of one synthetic crate on jarvis:

| Run                  | Wall seconds | CPU seconds | Instructions (billions) |
| -------------------- | ------------ | ----------- | ----------------------- |
| Quiet, 3 runs        | 6.3 to 6.6   | 6.5 to 6.7  | 108.41 to 109.00        |
| Beside 24 busy loops | 11.1 to 11.5 | 8.3 to 8.6  | 108.47 to 108.58        |

Wall time rose 75% under load and CPU time 30%. The instruction count moved by
0.2%, or 0.6% counting the first run.

How Ignite uses this:

- Each supervisor reads its command's instructions, CPU seconds and peak memory
  from the operating system when the command exits. The operating system gives
  an exact total for a direct child only, so for a command that starts processes
  of its own the supervisor also reads those descendants four times a second
  while they run.
- The undercount that remains: `rusage_info` (every version through V6) has
  child fields for CPU time, wakeups and page-ins of reaped children, but none
  for their instructions, so no total for a command's finished descendants
  exists to read. A descendant is counted up to its last reading, and one that
  starts and exits between two readings (a 100 ms `cc` run, say) is not counted
  at all. Commands made of long compiler runs lose little; one that starts
  thousands of tiny processes can read a few percent low. The CPU seconds of the
  same command are exact, so a count that is low against its CPU seconds shows
  this error.
- When the command's leader exits, the supervisor reads the group one last time
  and ends every process still in it, as Windows does through the job object.
- Each compiler process is counted by the `RUSTC_WRAPPER`, and each Rust test
  process by a Cargo target runner, so costs are known per crate and per test.
- Where one process runs many checks, as pytest does, the process's instructions
  are divided among its checks by the seconds each reports.
- Windows reports no instruction count. Cygnus records CPU seconds, and a
  check's cost for scoring comes from the Macs' records. A machine that cannot
  count instructions never measures or records a rebuild count, because a total
  over uncounted compiler units would read as a rebuild that costs nothing.

## Checks

A check is the unit Ignite schedules, caches and scores. This section covers how
checks are declared, how their inputs are hashed and how results are reused.

Checks are of two kinds, and the difference decides everything about where they
run:

- A **regression check** gives the same answer for the same tree. A failure
  blames a commit. It may block a landing.
- An **exploration check** searches at random: generated oracle scenarios,
  sampled cards, fuzzing, enumerating decision sequences. A failure is a newly
  found bug, usually older than the commits under test. It never blocks
  anything. See [Exploration checks](#exploration-checks).

### Declaring a family

Each check family is declared once in the repository. The declaration says what
the family's checks read, what must be built first and how to run them:

```toml
[family.rust]
kind = "regression"
adapter = "nextest"
platforms = "any"
needs = ["rust-tests"]
inputs = ["crate-closure", "rust/.config/nextest.toml"]
```

```toml
[family.shots]
kind = "regression"
adapter = "jsonl"
platforms = ["macos"]
needs = ["cli", "ui-deps"]
resources = ["browser-pages"]
list = "node ui/scripts/capture.mjs --list"
run = "node ui/scripts/capture.mjs {checks}"
inputs = ["ui/src/**", "ui/shots.json", "crate:beryl_cli"]
```

The fields:

- `kind` is `regression` or `exploration`.
- `adapter` says how results are read. There are two: `nextest` for Rust tests,
  and `jsonl` for every other command. See
  [Reporting results](#reporting-results).
- `platforms` is `any` when one pass on any operating system is enough, as for
  pure engine logic, or a list when the check must pass on each one named.
- `needs` names build products. `resources` names shared resources.
- `inputs` lists what the checks read: path patterns, `crate:<package>` for a
  package with every workspace package it depends on, and `crate-closure` for
  the same computed per test from the package that holds it. The lock file, the
  toolchain version and the family's own commands are always included.
- `list` prints the family's checks, each with any inputs of its own. A check
  that reads something no file holds, such as the commit subjects since the
  branch left staging, also prints a `digest` of it, which enters its input
  hash. `run` runs the checks named.
- `always = true` marks a family that blocks every landing its inputs touch. See
  [Where regression checks run](#where-regression-checks-run).
- `grows = true` marks a family whose `run` rereads the file `IGNITE_CORES_FILE`
  names and starts more threads when it rises, so its grant grows as cores free
  up. A build product takes the same field, and `ignite run --grows` does the
  same for one command.

The domain tools in `tools/` stay as they are: the oracle exporters, the motion
capture, the card tools. They become commands that families run. What leaves
`tools/` is the orchestration around them.

### Reporting results

A family's `run` command writes one JSON line per check to the file named by
`IGNITE_RESULTS`. Ignite adds the cost and the input hash itself:

```json
{"check": "shots:stage-mode-ways/phone", "outcome": "fail",
 "at": "ui/src/stage/phone.ts:212", "message": "2 elements overlap"}
```

The `nextest` adapter reads cargo-nextest's own machine-readable output, so Rust
tests need no change. For a Rust test, `crate-closure` is the package that
defines the test with every workspace package it depends on, development
dependencies included, as `cargo metadata` reports them.

Some runs produce no verdict, and Ignite never guesses one:

- A command that ends without reporting a check it was asked to run leaves that
  check `incomplete`, which is never a pass and never a failure.
- A `list` command that fails leaves the whole family `incomplete` for that run.
- An `incomplete` check is run again, at most twice more. If it is still
  incomplete, the hub files a P1 ticket that quotes the output, and a landing or
  a tip that needs the check waits for it.
- A command that says it cannot run on this machine, because a tool, a virtual
  machine or a download is missing, is `incomplete` with that reason and is not
  run again there. The ticket names the machine. A `jsonl` command says so with
  `"reason": "cannot-run"` on the check's line, as in the example below.
- Each command is told its class in `IGNITE_CLASS` (`land`, `agent`, `tip`,
  `batch` or `idle`), so a family whose checks wait on a slow tool can wait
  longer outside the gate.
- A command that runs past its hang limit is ended with everything it started
  and is `incomplete` as hung. It is not run again, because a hung command hangs
  again, and its output lists the processes it left. The limit is four times the
  command's slowest pass in 30 days, at least 15 minutes and at most 40, and 30
  minutes for a command with no pass on record. Time spent waiting for cores
  does not count. Each machine keeps the pass times of its own commands, a
  family's `list` and `run` apart, so the limit fits that machine's speed.
- A failure to file a ticket or to reach the hub never changes a check's
  outcome.

A check that cannot run on this machine:

```json
{"check": "ios:smoke", "outcome": "incomplete", "reason": "cannot-run",
 "message": "no simulator runtime is installed"}
```

Ignite keeps the whole output of every command in a file. The output of a
failed, flaky or incomplete run is kept for 14 days, and other output for a day.

### Input hashes

A check's **input hash** identifies everything it reads. Two trees with the same
hash for a check must give the same result.

Git already stores a content hash for every file and directory, so Ignite does
not hash file contents itself:

- For a tree with uncommitted edits, Ignite adds the working files to a
  temporary index and asks git for the tree object, without touching the branch.
  This takes a fraction of a second.
- A directory input contributes its git tree ID, and a file input its blob ID.
- The input hash is SHA-256 over the sorted inputs, the family's commands, the
  toolchain version and, for a check that is not `platforms = "any"`, the
  operating system.

```text
$ ignite why rust:beryl_rules::tests::entering::copied_face_aura --inputs
crate    beryl_rules   tree 4f1c9e0
crate    beryl_kernel  tree a02d77b
crate    beryl_card    tree 90be3f4   (+2 more crates)
file     rust/Cargo.lock  blob 5d0a1c2
hash     b7e2…41aa   passed on jarvis at 3a9e4c1, 2026-11-17 14:12
```

### The result cache

The result cache maps a check and an input hash to an outcome. It is what lets
one global view save work:

- A pass is reused everywhere. The gate, tip verification and `ignite check` all
  look up before they run.
- A failure is reused on the machine that produced it, with a pointer to its
  saved output. `--rerun` forces a fresh run.
- A failure from another machine is not trusted to block. The check runs once
  here first. If it passes here, the failure belongs to that machine's own
  state, such as a stale cache or a broken tool: the pass is stored, and the
  hub, seeing it replace that machine's failure, files a P1 ticket for that
  machine. Each daemon's copy of the cache records which machine produced each
  entry.
- A check is run a second time at once when it fails. Passing the second time
  makes it flaky, not failed. See [Flaky checks](#flaky-checks).
- An exploration check that was played on the same inputs is never played again.
- The hub holds the cache. Each daemon keeps a copy of the entries it has seen
  and asks the hub for the rest in one request per command.
- Entries nobody has looked up for 30 days are deleted.

The cache gives the right behavior when a landing is rebased. A check that reads
only files the agent changed keeps its hash across the rebase and is a cache
hit. A check that reads only files that landed meanwhile belongs to tip
verification. Only a check that reads both has a new hash and runs again.

### Auditing the cache

A check that reads a file it did not declare would be reused wrongly. Ignite
guards against that with reruns, which need no tracing of file access:

- In the `idle` class, the daemon reruns a random 0.5% of each day's cache hits
  in a landing tree at the commit that produced the hit. `ignite audit --now`
  runs a sample at once.
- A rerun that disagrees with the cache switches caching off for that check and
  files a P1 ticket naming both runs. The cause is an undeclared input or a
  check that is not deterministic.

## Where regression checks run

A regression check runs in one of three places: on every landing its inputs
touch, on landings where its record justifies its cost, or once per tip. This
section gives the rule.

### The always set

Some defects stop every other agent, so their checks block every landing whose
change touches their inputs:

- The commit subject's format, and conflict markers.
- The formatters, on changed files.
- The source rules, such as `card-names`, `two-player` and `file-length`.
- Clippy on the whole workspace the change touches, which is also the type
  check, and `tsc` and `pyre` for the UI and the Python tools.

These are ordinary checks with inputs, so they are cached like any other: a
change to documentation does not rerun clippy. They differ from other checks in
two ways:

- They are outside the threshold and the cap described below. Nothing leaves one
  out.
- They are looked up or run for the exact commit that lands, so a tip that does
  not compile cannot be produced.

### Scores

Every other regression check gets a **score** for a given change: how likely it
is to fail on a change like this one, divided by what it costs to run.

```text
score(check, change) = P(fail | areas the change touches) / cost(check)

P     = (failures after changes to these areas + 20 x family rate)
        / (runs after changes to these areas + 20)
cost  = median instructions of its last 20 runs, in billions
        + the builds it needs that nothing chosen so far has paid for
```

The parts:

- An **area** is a workspace package, or a second-level directory outside the
  Rust workspaces. A change's areas come from its paths. For a change that
  touches several areas, P is computed for each and the largest is used.
- Checks are chosen one at a time, highest score first. The first chosen check
  that needs a build the tree does not hold is charged that build's estimated
  cost, from the compile records of the crates that are out of date. Checks
  chosen after it use the build for nothing, so their scores are worked out
  again without it.
- A failure counts when a later change made the check pass again, whether that
  change fixed product code or updated the check. Either way the landing needed
  another edit. Flaky failures do not count.
- The family rate is the family's overall share of runs that failed. It stands
  in for 20 runs, so a check with few runs starts from its family's record.
- A check that has never run scores as certain to fail, at its family's median
  cost, so new checks run first.
- A check whose own file the change edits always runs, outside the cap. The
  agent's new regression test is never skipped.
- Checks that have failed in exactly the same runs are scored as one check, with
  their summed cost.
- A cost known only in CPU seconds, from Windows or from history imported at the
  switch, is converted at the rate measured on jarvis, about 17 billion
  instructions per CPU second, until the check has run under Ignite on a Mac.

Two checks after a change to one function in `beryl_rules`, with a threshold of
0.004:

```text
script:aura/enters-attached/you    9 failures in 74 runs, family rate 0.02
  P = (9 + 0.4) / 94 = 0.10       cost 0.8 billion     score 0.125    runs
rust:beryl_session::replay::*      1 failure in 310 runs, family rate 0.01
  P = (1 + 0.2) / 330 = 0.004     cost 95 billion      score 0.00004  at the tip
```

The second check costs 95 billion mostly because the tree has not built
`beryl_session`'s test binary. The score leaves that build to the tip, where it
happens once for everyone.

### The threshold and the cap

The landing gate runs every check whose score is at or above a threshold, in
score order, until their summed cost reaches a cap:

```toml
[gate]                 # example values
threshold = 0.004      # expected failures per billion instructions
cap = 400              # billions of instructions per landing
```

- Checks above the threshold but past the cap are listed in the landing's output
  and run in tip verification.
- The cap counts only checks chosen by score. The always set and the checks the
  change edits are outside it.
- The same checks run at any load. Seconds stretch; the set does not shrink.
- The values are set at the switch by replaying history: the largest threshold
  at which the gate would still have run 95% of the regression checks that
  caught something, in the fast check or slow CI, in the previous 14 days.
  Exploration finds are left out of that count. `ignite gate replay` repeats the
  calculation. Only a commit changes the values.
- A check's score changes as its record does, so checks move across the
  threshold by themselves. No ticket is filed to move one.

`ignite check` prints what the gate would run and why:

```text
$ ignite check --plan
always   4 checks (clippy, fmt, card-names, file-length)
gate     61 checks above 0.004; 44 cached, 17 to run, about 96 billion
tip      1,204 affected checks below the threshold
top      rust:beryl_rules::tests::entering::*      0.91  edited by this change
         script:aura/enters-attached/you           0.13  9 of 74 runs failed
```

## Landing

`ignite land` is the only way commits reach `staging`. It replaces `enact push`,
the ticket's closing comment and `enact close`.

The steps, in order:

1. Check that the session holds the ticket and the tree has no uncommitted
   edits.
2. Rebase the branch onto the tip. A conflict exits with code 3 and the files.
3. Take a landing tree, unless the ticket tree is already at the tip. Check out
   the rebased commit there.
4. Compute input hashes, look up the result cache, build what the remaining
   checks need, and run the always set and the gate.
5. Take the hub's short `staging` lease. If the tip moved since step 2, release
   it and go back to step 2. The second pass is mostly cache hits. A landing
   that loses this race three times takes the lease before its next pass and
   holds it through the gate, so it cannot lose again.
6. Push the commit from the landing tree to `staging` on the git remote, and
   release the lease.
7. Record the commits and the files they touch on the ticket with the agent's
   note, close the ticket and release the ticket tree.

The machine's daemon carries out these steps for the client. The `staging` lease
lasts 180 seconds and is renewed while it is held, so a daemon that dies holding
it blocks other landings for three minutes at most. A push that the remote
rejects because the tip moved goes back to step 2. A remote that cannot be
reached releases the lease and exits with code 4.

Rules:

- A ticket closes when it lands. The session does not wait for tip verification.
- A failed gate exits with code 2 and leaves the ticket tree as it is, rebased.
- `ignite land` refuses a tree whose branch is not the ticket's, and a tree with
  files that are neither committed nor ignored, naming them. Scratch output
  belongs in the session's scratch folder.
- The daemon writes a record of the landing to its own disk before it pushes. If
  the push succeeds and the hub cannot be told, the record stays queued and the
  ticket is closed when the hub returns. A record whose push was rejected is
  dropped. A landing is never pushed twice.
- Paths in a failed gate's output are relative to the repository, because the
  gate may have run in a landing tree that the next landing reuses. The rerun
  command works in the ticket tree.
- The landing tree that ran a gate already holds the new tip's build when the
  landing succeeds.
- A check with an open regression ticket is left out of other tickets' gates and
  reported as `known-failing`, so one broken check fails nobody else's landing.
  The ticket that fixes it must make it pass, and the check leaves that state
  when it next passes at a tip.

## Tip verification

Tip verification is what slow CI, the browser sweep and the nightly run become:
the regression checks that the gate left out, run once per tip. Nothing in it
runs on a timer.

How a daemon verifies a tip:

- It learns of a new tip from the hub's change feed.
- It lists every regression check whose inputs changed since the last verified
  commit and that has no cached result for the tip, among the families this
  machine can run.
- It claims those checks from the hub in batches, so two machines do not run the
  same ones, then runs them in a landing tree in the `tip` class.
- When a newer tip arrives meanwhile, the next round verifies the newest one.
  Checks whose inputs did not change again are cache hits. Tips in between are
  not verified one by one.
- When every regression check for a tip has a pass on a platform that satisfies
  it, the daemon that completed the set moves `master` to that tip. A check that
  is switched off as flaky is not part of the set. An `incomplete` check or one
  with a cached failure is part of it and holds `master`.
- A tip that has waited an hour for checks that only an unreachable machine can
  run files one P1 ticket naming the machine. `master` waits for those checks.
- A cached failure stands for every later tip that leaves the check's inputs
  unchanged, so `master` cannot move past a broken check. It moves again at the
  first tip where the check passes, which is the tip that holds the fix.
- A regression is reported only for a check that fails at the newest tip, or
  whose inputs have not changed since the tip where it failed. A failure that a
  later landing already repaired files nothing.

Because results are cached by input, a check that only the sweeps ran every
three hours now runs exactly when something it reads changes.

### Finding the culprit

A failed check names its own suspects, because Ignite knows what the check reads
and what each commit changed:

- The candidates are the commits since the last verified commit that changed any
  of the check's inputs.
- One candidate is the culprit, with nothing run.
- With several candidates, Ignite runs that one check at commits between them,
  by bisection, for at most 10 runs. A search that ends early reports the range.
- The unit of blame is one landing, which is one ticket's commits.

### Regression tickets

The hub then files a **regression ticket**. It is an ordinary P0 ticket that any
worker can take:

```text
Tk3fc  P0 bug  regression  check:rust
Regression: beryl_bench::family::tests::family_corpora_never_change
caused-by  Tw8gv.21.73 (commit 5e1b0aa "feat: ward counts mana spent")
first bad 5e1b0aa, last good 77c2d14
rerun: ignite test family_corpora_never_change
output (last 40 lines; whole output on jarvis at ...)
```

- It links to the ticket whose commit caused it. When the search ended with a
  range, it links to every ticket in the range.
- It names the operating system the check failed on, and carries a `needs:`
  label for it when the check is not portable.
- If the worker that landed the culprit has not yet been given another ticket,
  that machine's daemon has the regression sent to that session, which still
  holds the context. Otherwise it goes first in the dispatch order.
- `master` stays at the last verified commit until the check passes again.
  Landings on `staging` continue.
- The fix lands like any other change and closes the ticket. Tip verification
  then runs the check on the new tip. If it still fails, the hub reopens the
  ticket with the new output.

## Flaky checks

A flaky check costs every agent that meets it, so Ignite stops running one as
soon as the evidence is in, without waiting for an agent.

- Every check that fails is run a second time at once. One that then passes, on
  the same input hash, is recorded as flaky, and the gate or verification that
  saw it passes.
- A second flaky record within 7 days of the first, on a different commit,
  switches the check off on every machine. An off check does not run.
- The hub files one P3 ticket to delete the check or replace it with a
  deterministic test. The ticket quotes both failures.
- A run that could not complete, because a tool was missing or a process was
  killed, is `incomplete`. It is never flaky, never a regression and never a
  culprit, and it is not stored in the result cache.
- A check in the always set is never switched off. A formatter or a lint that
  flakes is almost always the machine: a file another process changed, a shared
  temporary path, a download. The hub files a P1 ticket to fix the step.
- A flaky record does not count toward switching a check off when three or more
  unrelated checks flaked in the same command. That is one fault of the machine,
  such as a full disk or a stuck browser page, and it gets one ticket.
- Checks that run outside the landing gate may get few cores. Each command is
  told its class, and a family whose checks wait on a browser or another slow
  tool uses longer waits outside the gate. A screenshot that takes two seconds
  once ran past a 30-second wait on a loaded machine and was filed as flaky.

## Exploration checks

Exploration checks find bugs that nobody wrote a test for. They run on cores
nobody else wants, and what they find is product work.

- They run only in the `idle` class.
- An exploration family declares a `next` command that produces checks nobody
  has played, such as new scenario seeds.
- A failure files an ordinary bug ticket at P2 with the label
  `found-by:<family>` and the steps to replay it. A failure with the same
  signature as an open ticket adds a comment.
- No exploration failure blocks a landing, holds `master` or starts a culprit
  search.

Idle cores are divided between families by what each has found:

```text
yield(family) = (bugs found in 30 days + 1) / instructions spent in those days
share(family) = yield(family) / sum of every family's yield
```

A found bug counts when its ticket closed with a fix to product code. A share
under 5% is raised to 5% and the others are scaled down to make room, which
keeps a family measured after a quiet month. `ignite status --exploration`
prints each family's finds, cost and share, which answers whether the slowest
searches ever catch anything.

## Tickets and the hub

The hub takes over the ticket service that runs on jarvis today. Agents reach
tickets only through `ignite ticket`, and every ticket operation is one request
with no lock held across it.

What carries over unchanged from the current service:

- Tickets with their IDs (such as `Tq44s.7`), every alias from earlier trackers,
  labels, priorities, parents, comments and history.
- Dependency edges, and the readiness rules: a ticket is ready when it is open,
  not deferred, has no open blocker of its own or of an ancestor, and has no
  open child.
- Named expiring leases, the numbered change feed that lets a client wait
  without polling, and per-machine write tokens.

What Ignite adds to a ticket:

- The machine, session and tree working it, and the times of its claim and
  landing.
- The commits it landed, the files they touched and the agent's note, which the
  next ticket's `ignite start` prints.
- For an improvement ticket, the rule that filed it and its estimated saving.

A ticket that the hub files by itself is matched against open ones by a
signature of the check, the kind of failure and its message. Host names, thread
numbers and source line numbers are left out of the signature, and the values in
a failed assertion are kept, so two different faults of one check stay two
tickets. A repeat adds a comment. A fault that returns after its ticket closed
gets a new ticket that names the old one.

```sh
ignite ticket create "Split beryl_session's replay code" -p P2 \
  --parent Tig9a --labels tier:opus --deps Tig9a.1 --body-file body.txt
ignite ticket ready --limit 10 --brief
ignite ticket wait Tig9a.1 --status closed
```

Labels carry what the dispatcher needs to know about a ticket: `tier:opus`,
`tier:sonnet` or `tier:light` names the model it needs, `needs:macos` an
operating system, and `lane:<name>` a chain. `ignite ticket batch` creates or
changes many tickets in one request that succeeds or fails as a whole.

Every change to anything in Beryl has a ticket. `ignite start "<summary>"` makes
that cheap for a session that was handed a task directly: it files the ticket
with the label `adhoc`, claims it and leases a tree in one command.

The hub also serves the dashboard, the one page meant for a person. It keeps the
current ticket views and adds each machine's cores and trees, the pool with what
each worker holds and what it is running, the landing queue, the state of tip
verification, open regressions and each exploration family's finds. Its first
line says what needs a person: tickets marked `needs:person`, a machine with no
dispatcher or with a paused pool, an open regression that no worker can take,
and a ticket that no machine is able to run.

The hub backs up its database hourly and exports it daily, as the service does
now. While the hub cannot be reached:

- Sessions keep working in their trees. `ignite test` and `ignite check` use the
  machine's copy of the result cache and run whatever it does not hold.
- Records queue on each machine and are sent when the hub returns.
- Claims and landings exit with code 4, because both need the hub.

## Workers, the pool and the dispatcher

Workers are sessions in the Claude and Codex desktop apps, so a person can see
every one in the app's sidebar, open it, and steer it from another device with
the app's Remote Control feature. Ignite makes handing them work cheap, and each
machine's daemon holds work back while that machine is full.

### The pool

Each machine has one standing pool that serves every active epic.

- A person staffs a machine in two steps. `ignite pool init`, run once from the
  main checkout, writes the dispatcher's folder, `~/ignite`. The person then
  opens a session there and sends `/ignite pool 8`. That session registers
  itself as the machine's dispatcher, starts the workers in the main checkout
  (at most ten per message) and runs the dispatch loop. The desktop app lets
  only a turn a person started start sessions, and its tools for clearing and
  prompting a session are only known to work on sessions the caller started, so
  the session that starts the workers must be the dispatcher.
- Registering a dispatcher replaces any earlier dispatcher of the same app on
  that machine, and removes the workers the earlier one started, which the new
  one could not drive.
- Workers must run in a permission mode that never prompts. A permission prompt
  stalls a worker nobody is watching, so `/ignite pool` refuses to start workers
  in any other mode.
- `ignite pool pause` stops new hand-offs on a machine, or on every machine with
  `--all`, and `ignite pool resume` starts them again. Workers that are running
  finish. Nothing but a person's `resume` ends a pause.
- The pool outlives epics. Nothing is rebuilt when a plan starts or ends.
- Claude Code workers are cleared and prompted again for each ticket. Codex has
  no clear operation, so a Codex worker is a new chat per ticket.

### The dispatcher

The dispatcher is one session per machine and per app whose whole job is to make
the tool calls the daemon asks for.

- It runs on Sonnet 5.5, in `~/ignite`, a folder outside the repository, so its
  fixed prefix holds none of Beryl's instruction files. The folder's
  instructions name the main checkout, because a session started from the
  dispatcher otherwise runs in the dispatcher's folder.
- That folder's `.claude/settings.json` sets `autoCompactWindow` to 120,000
  tokens, so its context never grows the way an orchestrator's does today.
- It never plans, reads tickets or decides anything.
- Codex has no such setting. When a Codex dispatcher's context passes the same
  size, the daemon's next instruction has it open a new chat in its folder with
  the prompt `/ignite pool 0`, which registers the new chat as the dispatcher,
  and stop.

Its loop is one blocking command and the tool calls that command returns:

```text
$ ignite dispatch --wait 540
```

```json
{"id": 4812, "do": "handoff", "session": "local_c8470108",
 "clear": true, "model": "opus", "effort": "high",
 "prompt": "Run `ignite start Tq44s.9.48 --reservation 7f3a` and follow it."}
```

The dispatcher carries out a hand-off as separate calls, in this order, and
reports `ignite dispatch ack 4812 ok`, or `failed` with the reason:

1. Clear the session. A session that refuses is still finishing a turn. The
   hand-off fails, the daemon tries another worker, and it comes back to this
   one after a minute.
2. Set the model and effort, then read the session back to confirm them. The app
   can ignore a model without saying so. A model that cannot be set fails the
   hand-off: a ticket is never run on a model other than the one it names.
3. Send the prompt. It carries a reservation code that only this hand-off's
   `ignite start` can use, so a hand-off that arrives late, after the ticket
   went elsewhere, claims nothing.

The other instructions are `message`, which sends text to a live session,
`check`, which asks whether a session is running, idle or gone, and `idle`,
which means the wait ended with nothing to do. Every instruction ends with one
line that restates the loop, so the dispatcher still knows its job after its
context is compacted. Its title stays `🔮 dispatcher <machine>` while it waits,
which is how a person sees that hand-offs are alive.

### What the daemon hands off, and when

A daemon with an idle worker asks the hub for the next ticket and says what the
machine has free. The hub answers with the first ticket, in this order, that the
machine can take:

1. An open regression ticket. It needs an idle worker and a free tree on a
   machine that can run its check, and skips the test of expected cores below.
2. An improvement ticket, when improvements are under their share. See
   [Improvement tickets](#improvement-tickets).
3. A ready ticket from the epic that is furthest below its share of workers. See
   [Dividing workers between epics](#dividing-workers-between-epics). Within an
   epic, the ticket of highest priority goes first, then the one with the
   longest chain of tickets waiting behind it, then the oldest.

A machine can take a ticket when all of these hold:

- A pool member that suits the ticket is idle: one whose app can run the model
  the ticket's `tier:` label names, and the app a `provider:claude` or
  `provider:codex` label names, if the ticket has one. A worker is idle when it
  holds no ticket.
- A ticket tree is free.
- The ticket's expected use of cores fits beside the tickets already in progress
  there. See [Light and heavy tickets](#light-and-heavy-tickets).
- The machine can run every check family that the ticket's named files could
  bring into its gate. Cygnus never takes a ticket that names UI files, or one
  labeled `needs:macos`.

Not every open ticket is offered:

- An epic is worked only once it is active, which means it has a `--workers`
  number. `/ignite` sets one when it files an epic or is given one. Until then
  the epic's tickets wait, however ready they are, so the pool never starts on a
  backlog that nobody asked for.
- Tickets that the hub files itself are always offered: regressions,
  improvements and repairs to a machine. So is a ticket that a session starts by
  name.
- A ticket marked `needs:person` is never offered.

The hub reserves the ticket for that machine in the same request, so two
machines never take one ticket. What happens next:

- The daemon gives the dispatcher the hand-off. The worker's `ignite start`
  turns the reservation into a claim held by that session and tree.
- A hand-off the dispatcher has not acknowledged in 2 minutes is given again.
  After three failures the daemon stops handing off, and the dashboard shows the
  dispatcher as down. Workers already running are unaffected.
- A reservation that no `ignite start` has claimed in 10 minutes lapses. The
  ticket returns to the queue, and the worker is treated as stalled.
- A session that runs a command for a ticket whose lease expired, and that
  another session has since claimed, gets exit code 3.

That is the whole of fleet scheduling: a machine with free workers, trees and
cores pulls work, and a busy one does not. The hub keeps no model of any
machine's load.

### Dividing workers between epics

A person decides how much of the fleet each epic gets by giving the epic a
number of workers:

```sh
ignite ticket update Tq44s --workers 5      # Beryl 3D
ignite ticket update Tw8gv --workers 2      # Project 50k
```

- The numbers are shares, not reservations. The hub gives the next free worker
  to the epic with the fewest tickets in progress per share, counted across
  every machine.
- A share by itself never leaves a worker idle. An epic with fewer ready tickets
  than its share takes what it can use, and the others divide the rest in
  proportion.
- `--max-workers` is a hard cap, for an epic whose tickets conflict when too
  many run at once. It is the one setting here that can leave a worker idle.
- An epic with no number has a share of 1. Tickets under no epic form one group
  with a share of 1.
- The number may sit on any ticket that has children. A ticket belongs to the
  nearest ancestor that has one.

With the two settings above, nothing yet in progress and plenty of ready tickets
in both epics:

```text
14 idle workers                                -> 10 and 4
 7 idle workers                                ->  5 and 2
 7 idle workers, Project 50k has 1 ready ticket ->  6 and 1
```

### Light and heavy tickets

Tickets ask very different amounts of the machine. A fix to a Python script uses
almost no CPU, and a change to a core crate rebuilds half the workspace many
times. The daemon admits tickets by what each is expected to use, so cheap
tickets run beside expensive ones.

- A ticket's **expected cores** is a figure from history: over the tickets
  landed in the last 30 days that named files in the same areas, the median of
  core-minutes used per minute worked. An area is a workspace package or a
  second-level directory.
- A ticket that names files in several areas takes the largest figure. A ticket
  that names no files takes the machine's median.
- A planner can state the figure directly with `--cores`.
- The daemon keeps a total for the tickets in progress on its machine, counting
  each at the larger of its expectation and what it has used so far.
- A ticket is admitted when that total plus its own figure is within
  `admit_cores`, a setting per machine. On jarvis, with 18 cores, it starts at
  13\.

```text
$ ignite status --admission
admit_cores 13.0   in progress 11.6 (9 tickets)
next  Tw8gv.21.80  beryl_kernel  expects 2.1 cores   waits
next  Ten2c.4      tools/        expects 0.1 cores   admitted
```

A documentation or tools ticket is therefore admitted on a busy machine, limited
only by idle workers and trees, while a third kernel ticket waits. The same
figure decides whether `ignite start` prebuilds the tree: a ticket expected to
use under half a core is not expected to compile Rust.

### Chains and neighbours

Two rules replace Enact's lanes, because tickets queued behind one session
waited five times longer than the rest:

- A `lane:<name>` label is only for a true chain, where each ticket needs the
  previous one's result. The next ticket of a chain goes to the same session
  without clearing it.
- Tickets that merely touch the same code are not grouped. Among idle workers
  the daemon prefers the one whose last ticket shared the most files with this
  one, and never waits for a particular worker.

### Stalled workers

A worker can stop without finishing, and its ticket and tree must not stay
leased for nothing.

- Any `ignite` command from a tree renews its ticket's lease, and a command
  still running keeps it renewed however long it takes.
- After 30 minutes in which no command started or ran, the daemon has the
  dispatcher look at the session. Silence proves nothing: a healthy worker can
  read and think for a long time, and an app that cannot find a session has not
  shown that the session is gone.
- A session that is still running is left alone. An idle one is sent one message
  asking it to continue or release.
- The daemon releases the ticket only when the app confirms that the session is
  gone, or that it is idle and has stayed idle for 15 minutes after the message.
  Releasing keeps the work, as [Releasing work](#releasing-work) describes.

### Steering work in flight

A person sometimes needs to correct work that is already running: a session that
waits on something it should not, or an instruction in a ticket that turned out
to be wrong. Typing into the session in the desktop app always works, but it
corrects one session and leaves no trace. Ignite adds three things, so that a
correction is noticed, lasts, and can reach every worker.

Seeing that a worker is stuck:

- Each daemon reports, with every heartbeat, the `ignite` command each of its
  workers is running or last ran, and when that command started or ended.
- The dashboard's pool view shows it. A command that has run for over 10
  minutes, and a worker with no command for over 10 minutes, are both marked.
  The stall rule above acts only at 30 minutes of silence, and a session that
  sits in `ignite wait` is never silent.

Correcting a ticket so that it lasts:

- A comment on a ticket, or an edit to its text, is the durable correction. It
  stays with the ticket for whoever works it next.
- Every `ignite` command from a tree prints, as `changed` lines, the comments
  and edits made to its ticket since the session's previous command, leaving out
  the session's own. A worker therefore hears of a change on its next command,
  without anyone messaging it.

Correcting it now:

```sh
ignite tell Tq44s.9.48 "Do not wait for multivac. Land when the gate passes."
ignite tell --all "The oracle engines are down until noon. Skip oracle checks."
```

- `ignite tell <ticket>` records the text as a comment on the ticket and marks
  it for delivery. The daemon of the machine where the ticket is being worked
  has its dispatcher send the text to that session as a message.
- A session that is inside `ignite wait` gets its wait ended at once, with the
  message as the reply, so it does not sit out the rest of the wait.
- `ignite tell <session>` does the same for the ticket that session holds, and
  `--all` for every ticket in progress, on every machine.
- Another session can use it too. A worker that learns something the holder of
  another ticket needs says so with `ignite tell`.

`ignite prime` carries the rule that prevents the commonest case: a worker never
waits on another machine or on tip verification. When its ticket needs something
outside itself, it finishes what it can and lands, or releases the ticket with a
note.

### The justiciar

Some faults stop work for everyone and no ordinary ticket fixes them: Ignite
crashed, a loop of the daemon keeps failing, `master` has not moved for hours, a
plan waits on a clock or on a machine that is off, an epic is deadlocked. The
justiciar is the role that ends such an emergency. It is an ordinary worker
session given a ticket labelled `justiciar`, which brings with it the authority
in `ignite guide justiciar`: to repair the ticket graph, release or rewrite any
ticket, repair the machine, and fix Ignite itself, deploying the fix through the
release branch when Ignite blocks its own landing.

- The hub files justiciar tickets at P0 with `tier:opus`, matched by signature
  so a repeated sighting adds a comment and never a second ticket. They come
  from upkeep, when the oldest landing since the last verified tip has waited
  three hours or a ticket has been held for six, and from `POST /justiciar`,
  which daemons send for a crash record or a loop that keeps failing, and which
  `ignite justiciar "<reason>"` sends for a session that meets an emergency.
- A report about one machine, such as its daemon's crash, is labelled
  `needs:<machine>`, so only that machine's pool works it.
- Justiciar tickets are offered before every other ticket, whether or not an
  epic is active. So are all the other tickets the hub files by itself, which
  carry the label `hub`.
- A daemon keeps one worker free for justiciar tickets while its pool has more
  than one worker, so a pool full of stuck tickets cannot starve the ticket that
  would free them.
- When the hub cannot be reached for 15 minutes, a daemon hands a justiciar
  prompt to its own idle worker directly, since the hub is what would have filed
  the ticket.
- When the daemon itself is down, the dispatcher's `ignite dispatch` cannot
  reach it. If the monitor has written `justiciar-needed.json`, the client
  answers with a hand-off of a justiciar prompt to a worker of the pool, which
  it reads from the daemon's database.
- The one fault nothing automatic repairs is a missing dispatcher: only a turn a
  person started can start sessions. The daemon posts a notification when the
  dispatcher stops answering.

### Planning

The `/ignite` skill is how a person starts work. It replaces `/enact` and
`/enact-plan`.

- Given a ticket or a small task, the session does it itself: `ignite start`,
  the work, `ignite land`.
- Given a plan or a large task, the session settles the design, files the epic
  and its tickets in one `ignite ticket batch`, runs `ignite plan check`, fixes
  what it reports, and stops. The pool takes the tickets. The planning session
  runs no loop, which is what removes today's orchestration cost.
- Given `sweep`, the session triages the open tickets that nothing is working.
  For each it decides whether the work is done, obsolete, still wanted or in
  need of a person, with a line of evidence. It shows the table, closes the
  first two kinds and activates what is left.

`ignite plan check` reports the shape of an epic, since dependency chains are
56% of lead time:

```text
$ ignite plan check Tig9a
tickets        41, ready at the start 17
longest chain  6: Tig9a.1 > .4 > .9 > .15 > .22 > .30
suspect edge   .12 depends on .7, and they name no file in common
no files       .19, .27 name no files to read
lane           lane:replay holds .31 and .33, which do not depend on each other
```

The command only reports. It exits with code 0 whatever it finds, and the
planner decides what to change.

A review is a ticket like any other, with the label `review`, that depends on
the tickets it reviews. A planner files one where a change is large or risky.

The hub watches for an epic with open tickets of which none is ready or being
worked. It files a P1 ticket to repair that epic's graph, which replaces the
emergency role Enact used for deadlocks.

## The fleet

Every machine runs the same daemon and differs only in what it declares it can
do. The hub is the one shared point.

```toml
[machine.cygnus]
os = "windows"
ticket_trees = 6
landing_trees = 2
families = ["rust", "clippy", "format", "source-rules", "python"]
floor = { batch = 8, tip = 2 }
```

`families` lists the check families the machine can run, here the Rust tests,
clippy, the formatters, the source rules and the Python tools' tests. A machine
with no such list can run every family.

What is shared, and what is not:

- **Tickets, the result cache and records are shared** through the hub.
- **Build output is not shared.** Incremental builds belong to one path on one
  machine, and Windows output is useless to a Mac. The fleet grows by placing
  whole sessions, each with a build tree, on machines with cores.
- **A machine is known by an ID** the daemon creates and stores, not by its
  hostname, so renaming a machine does not split its history.

How the machines divide verification:

- A check with `platforms = "any"` needs one pass anywhere. Cygnus can therefore
  verify pure engine logic for the whole fleet.
- A check with a platform list needs a pass on each. Only the Macs run the
  browser, oracle and app families.
- A daemon prefers the checks only it can run, then claims portable ones.

On Windows the daemon uses a named pipe for the client, job objects to end a
tree's processes, and CPU seconds for cost.

## Records and metrics

Ignite improves the workflow from what it records, so the record of every
command is part of the design, not an afterthought.

The supervisor writes one **record** per command, and the daemon sends records
to the hub as they are made:

```json
{"machine": "m-7f3a", "ticket": "Tq44s.9.48", "session": "local_c8470108",
 "tree": "tree-07", "verb": "test", "class": "agent", "family": "b3d-test",
 "queued_s": 4.1, "wall_s": 58.2, "cpu_s": 301.0, "instructions": 1.21e12,
 "peak_mb": 2210, "outcome": "fail", "commit": "8c1f2ab", "ignite": "d97ed0e"}
```

- Check outcomes and per-crate compile costs are records too, keyed to the
  command that produced them.
- These records replace the run log, the log of CPU leases, Enact's lock log and
  the CPU sampler as sources for `just efficiency`.
- The model's tokens and dollars still come from each provider's session
  transcripts, which the efficiency dashboard reads. Each daemon runs that
  ingest for its own machine and posts the new rows to the hub, so the hub's
  store covers every machine with no manual export.

The measures under [What Ignite must achieve](#what-ignite-must-achieve) are
named series in that store, each by machine, provider and ticket label. Three
more exist to keep the gate honest:

- **Escapes**: regression tickets per 100 landings.
- **Cache hits**: the share of checks answered from the result cache, at the
  gate and at the tip.
- **Gate cost**: each landing's instructions and seconds, with its wait for a
  landing tree apart.

Ignite compares groups of tickets chosen by label, and the days before and after
a change, as the dashboard does. It assigns no ticket to a setting at random.
Settings, model tier included, change only by a commit that a person or a ticket
asked for.

## Improvement tickets

Improvement tickets are how the workflow gets better without a scheduled agent
or a separate role. A rule over the records files each one with an estimate of
what it would save, and the dispatcher guarantees them workers.

A **rule** is a query that finds a source of waste and names a subject, a cost
and a saving per week. The efficiency dashboard's findings are rules already.
Ignite adds its own:

- `slow-crate`: a crate whose rebuilds cost the most instructions per week,
  counting every rebuild agents actually ran. The ticket asks to split it or cut
  what depends on it.
- `never-caught`: check code that used cores for 30 days at the tip and caught
  nothing. The ticket asks to delete it or replace it with a cheaper test.
- `repeated-command`: a command that tickets of one kind run far more often than
  their cohort.
- `slow-verb`: an `ignite` verb whose own overhead, apart from the work it runs,
  is over a second at the median.
- `rejected-command`: a verb or flag that at least three sessions typed in the
  last week and the `ignite` executable refused, such as `ticket dep list`. The
  executable sends every command line it cannot parse to the hub, which lists
  them on the `finds` page. The cost is an estimated $0.25 a try, for the
  refused request and the workaround after it. The ticket asks to add the
  command or an alias for it.

```text
Tm41q  P2 task  improvement  rule:slow-crate
beryl_session costs 38.2 core-hours a week in rebuilds (2,100 rebuilds,
median 1,100 billion instructions). Estimated saving: 15.3 core-hours a
week if its replay and export code move to crates that fewer changes rebuild.
measure: rebuild count `session-body`, now 1,450 billion
```

Rules for filing:

- The hub evaluates the rules when new records arrive. It files a ticket for a
  finding with no open ticket for the same rule and subject.
- Nothing is filed under a minimum saving. A finding must reach $25, 2
  agent-hours or 10 core-hours a week, in whichever unit its rule measures.
- A rule files nothing for a subject within 30 days of closing a ticket for it.
- A rule measures what the work costs now, never how much a number grew. Code
  that gets larger as features are added files nothing unless agents pay for it
  often enough to clear the minimum.
- A rule can be listed as report-only. `tier-gap`, which compares model tiers,
  is report-only, because the choice of model stays with people.
- `never-caught` never names a check on the protected list in `ignite.toml`: the
  app starting, the browser smoke checks, clippy, the type checks and the source
  rules. They are cheap, and what they guard is too basic to give up for want of
  a recent failure.

Rules for the share:

- Improvement tickets get at least 15% of the last 20 hand-offs to each machine,
  not counting hand-offs of regression tickets. The hub counts them when it
  answers a machine's request for a ticket.
- The share rises in a straight line to 30% as the backlog grows. The measure is
  the backlog's estimated saving as a fraction of last week's total, taken in
  each of the three units (dollars, agent-hours, core-hours), using the largest.
  At 5% or less the share is 15%, and at 20% or more it is 30%.
- Within the share, the largest saving goes first, taking the three units in
  turn.

```text
backlog  $310 of $8,305 a week (4%), 9 of 1,100 agent-hours (1%),
         200 of 1,800 core-hours (11%)
share    largest is 11%  ->  15% + 15% x (11 - 5) / 15 = 21% of hand-offs
```

Each improvement lands with an entry in `data/efficiency-changes.json`, as the
project already requires of changes made to save time or tokens. Seven days
later the hub comments on the ticket with the rule's measure for the subject
before and after. A ticket about compile cost also records a rebuild count
before and after, which does not depend on load.

## Rebuild counts

Rebuild counts are a measuring instrument for compile cost. They borrow the
method of the game's instruction-count benchmarks, a number that does not depend
on the machine's load, and deliberately not their use as a gate.

A **rebuild count** is the instructions the compiler executes to rebuild after
one fixed, scripted edit, in a tree that had built the state before the edit.

```toml
[[rebuild]]
name = "kernel-body"
edit = "append a no-op item to a module of beryl_kernel"
file = "rust/crates/beryl_kernel/src/counters.rs"
append = "\nconst _: () = ();\n"
products = ["clippy", "rust-tests"]

[[rebuild]]
name = "one-card"
edit = "append a no-op item to one card file"
file = "rust/cards/cards_1993/lea/animate_dead.rs"
append = "\nconst _: () = ();\n"
products = ["clippy", "rust-tests"]
```

What a count is for:

- **Proving an optimization.** A ticket that sets out to cut compile cost names
  a rebuild count as its measure. The agent runs `ignite rebuild` before and
  after, and the two numbers are comparable whatever else the machine was doing.
- **Showing the trend.** The daemon measures each count in the `idle` class, in
  a landing tree, when the verified commit has moved by 100 commits. The
  dashboard draws the series.

What a count is not for:

- A count blocks nothing and files nothing. Compile cost rises whenever cards
  and features are added, so a rule that reacted to a rising count would file a
  ticket for ordinary progress.
- The trigger for an optimization pass is the `slow-crate` rule under
  [Improvement tickets](#improvement-tickets), which ranks crates by what agents
  actually spent rebuilding them in a week. A crate that grows and is seldom
  rebuilt triggers nothing. A crate that many tickets rebuild triggers a ticket
  even if it has not grown.

A measurement records the total instructions and the crates recompiled, with
each crate's own count:

```text
$ ignite rebuild kernel-body
kernel-body  412 billion  14 crates
  beryl_cli            118 billion
  beryl_session         97 billion
  beryl_search          41 billion   (+11 more)
```

## Guidance for agents

Enact's role files, skills and design notes, with `docs/checks.md` and the
`cifix` skill, hold about 800 rules and lessons. Most were written after
something went wrong. Ignite keeps them in less text that is read less often: a
rule lives where it is needed.

- **`ignite prime`**, under 60 lines, is the only text every worker reads. It
  holds the rules that apply to every ticket.
- **A guide topic** holds the rules for one situation. `ignite guide <topic>`
  prints one in under 50 lines, and a command names the topic at the moment it
  applies, so a worker reads it only then.
- **The `/ignite` skill** holds the rules for planning, for sweeping stale
  tickets and for the dispatcher.
- **`AGENTS.md` and the project's skills** hold the rules that are about Beryl
  and not about Ignite.
- **Ignite itself** does what a rule used to ask of an agent, wherever a program
  can.

```text
$ ignite start Tk3fc
ticket   Tk3fc  Regression: family_corpora_never_change
tree     /Users/dthurn/beryl/.worktrees/tree-02  at staging 91d07be
guide    ignite guide regression   (read it before you change anything)
```

### What every worker reads

`ignite prime` holds these rules, each in a line or two:

- Nobody answers questions. Decide, act, and record the choice and its reason on
  the ticket.
- The ticket may be days old. Check it against the code as it is. Where they
  disagree, the code and the epic's intent win, and the difference goes on the
  ticket.
- Read the files the ticket names before searching for others, and send reads
  together.
- Make the smallest change that meets the acceptance criteria. Run the focused
  tests, exercise the change by hand, and read the whole diff as a reviewer
  before landing. A check that could not run is not a pass.
- Tests go through public behavior and never depend on the machine's speed.
- Ordinary difficulty is the job: failing tests, confusing code, conflicts.
  Never wait for a person, for another ticket, for another machine or for
  verification.
- Never end a turn on work that nothing will report. Never leave a ticket held
  when stopping for good: release it with its exact state.
- Never work in another session's tree, take over its ticket or change another
  ticket's scope.
- A `changed` line is an instruction about the ticket. Read it before going on.
- The landing note says what changed, what was decided and what a reviewer
  should look at.
- The title shows the state: working, done, stopped unfinished, or waiting on a
  person's answer.

### The guide topics

Each topic is printed by the command that makes it relevant.

- `follow-ups`, when a session creates a ticket while it holds one. File only
  what is essential. A gap in verifying your own ticket is never optional. No
  ticket's acceptance may need time to pass: measure what can be measured now,
  and defer an observation ticket to the day its data will exist.
- `release`, on `ignite release` or when a command fails for a reason outside
  the ticket. Waiting on another ticket, on another machine or on a person, and
  one fault that blocks several tickets.
- `review`, on `ignite start` of a review ticket or one labeled `review:inline`.
  What a review looks for, when to rewrite, what is not a finding, and that the
  reviewer fixes what it finds.
- `regression`, on `ignite start` of a regression ticket. Reproduce it first and
  read the ticket that caused it. Fix forward. A fault of the product is fixed
  in the product, and a test that fails while the product is right is probably
  worthless.
- `checks`, on a ticket about a flaky, hung or never-catching check. The order
  of remedies: fix the product, fix the machine, replace the check with a
  cheaper deterministic one, delete it. Loosening a threshold, adding a retry
  and raising a limit are not remedies.
- `audit`, on `ignite start` of an audit ticket. What an audit of the
  architecture or of the process looks for, and what is worth filing.
- `docs`, on `ignite start` of a documentation ticket. Describe the shipped
  state, edit before adding, and retire the plan after moving what is durable.
- `machine`, when `ignite doctor` reports a fault. What counts as a fault of the
  machine, what an agent may repair, and what goes to a person.

### Lessons that became behavior

Where a program can do what a rule asked, Ignite does it:

- Released work is saved and travels. Enact added this after two tickets
  released on one machine were redone from nothing on another.
- A worker is released only when the app confirms that it is idle or gone. A
  dozen workers once sat idle for hours, and a healthy one was once taken for
  dead.
- A hand-off carries a code that only it can use, because a delayed hand-off
  once claimed a ticket that had moved on.
- A command past its hang limit is ended and not run again. A hung check once
  held every push on the machine.
- A tree's processes are ended at release. A fuzz run once kept a core busy for
  four hours after its ticket closed.
- One fault files one ticket, matched by signature. A day of failures usually
  has far fewer causes than tickets.
- An epic is worked only once a person activates it, and a paused pool stays
  paused until a person resumes it.

### Dropped on purpose

These go, because Ignite removes what each existed for:

- Roles as prompt files, and the Steward, Justiciar, Mason, Sage, Archivist and
  Marshal roles. Their rules are in the `regression`, `machine`, `audit` and
  `docs` topics. What a Justiciar did by hand, the daemon does or
  `ignite doctor` reports.
- Runs: registering, pruning and finishing them, orchestrator leases, and the
  `watch_ci` notice. A pool has no runs.
- The merge queue, delivery receipts and check trees. The landing gate, the
  landing record and landing trees replace them.
- Wall-clock budgets, the load-adjusted clock, tickets that cut checks to fit a
  budget, and deferring unfinished steps to a sweep. Costs in instructions and
  scores replace them.
- The sweeps, the nightly run and the hourly full run. Checks run when their
  inputs change, and the cache audit catches a wrong declaration.
- Choosing browser checks by coverage data. Scores choose them, and
  `just coverage` stays as a tool.
- Quarantining a test. The project's rule is to delete or replace it.
- Light runs. A planner files the reviews a plan needs, and nothing else is
  automatic.

## The switch from Enact

Ignite replaces Enact in one switch, with every other kind of work halted until
it happens. There is no period in which both run, and nothing stays for
compatibility.

Until the switch, Enact delivers Ignite's own code. Ignite's source is a Cargo
workspace of its own in the repository, so building it never rebuilds the game's
crates and its dependencies never enter the game's lock file. Today's checks
give that workspace a lane of its own, so a change to Ignite runs Ignite's
formatting, clippy and tests and none of the game's checks.

The change that deletes the old tools cannot go through Enact, whose check it
removes. It is prepared on a branch of its own and is the first commit that
Ignite lands.

The work is tracked in the epic `Thrh4`, whose design field holds the code
contract: where each part lives and which ticket owns it.

What the switch imports:

- Every ticket, with its ID, aliases, edges, comments and history, from the
  current service's database.
- The run log and the verdicts on each check's failures, as each check's record
  of runs, costs and failures, so the gate has scores on the first day.
- The recorded per-crate compile timings.

What the switch deletes, in the same change:

- `.enact.toml`, the `/enact`, `/enact-plan` and `/enact-sweep` skills, and
  every reference to `enact`, `tk` and Enact's roles (Vizier, Executor, Warden,
  Steward and the rest).
- The orchestration modules in `tools/` and their tests: the token pool, the
  lanes, test selection, the run log, verdicts, budgets, hang limits, flaky
  handling, bisection, slow CI, the sweeps, the nightly run, the build-cache
  copying and the incident writer.
- The `just check`, `just ci`, `just sweep` and `just nightly` recipes and the
  background agents that run the nightly, the sweeps, the CPU sampler and the
  ticket service. Every other `just` recipe stays, `just efficiency` among them.
- Every worktree and build cache under the old layout.

What the switch rewrites:

- The root `AGENTS.md` sections on Enact and on checks are rewritten for Ignite.
  Rules about how to use Enact go. Rules about the project's own checks stay: a
  UI bug fix adds its broken state as a fixture, the counts in an allowlist only
  go down, a phone screen is a shot at both sizes, and an agent runs only the
  browser checks its change needs.
- The `cifix` skill is rewritten as the project's guide to working a ticket
  about a check, keeping its order of remedies and dropping its orchestration.
- `docs/checks.md` is deleted once each lesson in it has a home. A lesson that
  explains a setting moves into a comment beside that setting in the family's
  declaration. This document holds the rest, and
  [Guidance for agents](#guidance-for-agents) lists what was dropped on purpose.
- Every epic that is open at the switch starts inactive. Nothing in the old
  backlog is worked until a person activates it.
- The efficiency dashboard's sources change as described under
  [Records and metrics](#records-and-metrics).

The switch happens when the checks under [Manual QA](#manual-qa) pass on both
machines in a rehearsal:

- Each machine has a second clone of the repository, with its own build trees.
- A rehearsal hub on jarvis serves a copy of the ticket database on its own
  port, and the rehearsal daemons use it.
- Desktop sessions opened in the clones form the pool and the dispatcher, so the
  hand-off path is the real one.

A switch that fails on the day, before other work resumes, can be undone from
what it leaves untouched. The import only reads the old ticket database, and the
commit before the switch still holds Enact's configuration and the old tools.
Restoring both and restarting the old ticket service returns the project to its
state before the switch. The deleted worktrees and build caches are rebuilt by
the old tools on first use.

## Alternatives considered

These are the choices that were weighed against measurements and set aside.

- **Buck2 or Bazel.** They cache whole-crate compiles and keep no incremental
  state, so every edit to a crate recompiles it and everything downstream from
  nothing: the 421 to 544 second case, against 18 with Cargo's incremental
  build. Their cache helps only for crates that are identical between trees, and
  each agent's tree differs exactly in the expensive crates downstream of its
  edit. Agents also know Cargo far better. Ignite takes their best idea, caching
  by declared inputs, at the level of checks.
- **A shared cache of compiled output between trees.** The same loss of
  incremental builds applies, and the measured waste is cold trees, which fixed
  build trees remove.
- **Workers as command-line sessions started by the daemon.** This removes the
  dispatcher entirely, but a person cannot see or steer those sessions in the
  desktop app. A dispatcher with a small context costs about a tenth of today's
  orchestration and keeps every session visible.
- **A wall-clock budget for the gate.** Today's 60 to 90 second target runs
  fewer checks when the machine is busy, which sends more defects to `staging`
  when the machine can least afford the repair.
- **Starting a ticket on its dependency's unlanded branch.** It would save only
  the dependency's check-and-land tail, a few minutes per link, at the cost of
  rebasing work onto commits that can still change. Chains are shortened by the
  plan's shape.
- **Randomized trials of workflow settings.** Set aside by decision. Ignite
  reports comparisons and leaves settings to people.
- **Replacing Enact in stages.** Two systems would share cores and worktrees
  without seeing each other, which is the problem Ignite exists to remove.

## Manual QA

These checks are run by hand in the rehearsal described under
[The switch from Enact](#the-switch-from-enact), on jarvis first and then on
multivac and cygnus. They are the condition for the switch.

Windows testing never blocks work. A Windows check result, slow CI on cygnus and
the cygnus QA ticket never gate landing, `master` or the switch. A Windows
failure becomes a ticket that is fixed alongside other work.

### The first check

Start with the one flow that crosses every part:

- **One real ticket, end to end.** In a desktop session on jarvis, run
  `ignite start` on a small rules fix. Edit a function in `beryl_rules`, add a
  test, run `ignite test` on it, commit and run `ignite land`. Expect: the tree
  is at the tip with its build already running before the first edit; the test
  build recompiles only `beryl_rules` and what the chosen tests need; the gate
  reports cached and run checks; the commit is on `staging`; the ticket is
  closed with the note; within minutes `master` moves to the commit; the
  dashboard shows each step.

### Caching and landing

These show that a result is reused wherever its inputs match, and that landings
stay correct when they race.

- **Result cache across trees.** Run `ignite check` in a tree. Check out the
  same commit in another tree and run it again. Expect every check from cache
  and no build started.
- **Result cache across machines.** Land a change to `beryl_kernel` on jarvis.
  Expect cygnus to claim and run the portable Rust checks for that tip, and
  jarvis to show them as cached with cygnus as the source.
- **A rebase that matters and one that does not.** Start two tickets from the
  same tip. Land one that edits `ui/`. Land the other, which edits
  `beryl_rules`. Expect its gate to be nearly all cache hits. Repeat with both
  editing `beryl_rules`, and expect the checks that read that crate to run
  again.
- **Two landings at once.** Run `ignite land` in two trees in the same second.
  Expect both to land, the second after one more pass, and no check to run twice
  on the same inputs.
- **A failed gate.** Break a test and run `ignite land`. Expect exit code 2, at
  most 40 lines naming the check, its file and line and its rerun command, and
  nothing on `staging`.

### Failures and repairs

These show that each kind of failure goes where the design sends it.

- **A regression and its culprit.** Land a change that breaks a check scored
  below the threshold. Expect a P0 regression ticket naming the commit and its
  ticket with nothing bisected when one commit touched the check's inputs;
  `master` held at the previous commit; other landings unaffected and the check
  shown as `known-failing` in their gates. Land the fix and expect `master` to
  move.
- **The culprit's session.** Repeat while the worker that landed the break has
  no new ticket. Expect the dispatcher to send the regression to that session.
- **A flaky check.** Add a check that fails every other run. Expect the first
  flake recorded and passed over, the second on a later commit to switch the
  check off on both machines, and one P3 ticket quoting both failures.
- **An exploration find.** Seed a generated oracle scenario that fails. Expect
  an ordinary P2 bug ticket with replay steps, no regression ticket, and
  `master` still moving.
- **A cache audit.** Add a check that reads a file it does not declare, pass it,
  change the file and force an audit with `ignite audit --now`. Expect caching
  switched off for that check and a P1 ticket naming both runs.

### Scheduling and workers

These show that cores, trees and workers go where the rules say.

- **Load.** Start 24 busy loops and land a change. Expect the same checks as on
  a quiet machine, a longer wall time, idle work stopped, and the record's
  instruction count within 1% of the quiet run's.
- **No free tree.** Lease every ticket tree and run `ignite start` in one more
  session. Expect a wait that names the reason, no new directory, and no
  hand-off from the dispatcher.
- **The dispatcher.** Run `ignite pool init`, open a session in `~/ignite` and
  send `/ignite pool 4`. Expect four workers whose folder is the main checkout.
  File six ready tickets. Expect four hand-offs at once and two as workers
  finish, and each worker titled with its ticket. After 50 hand-offs, expect the
  dispatcher's transcript to show at least one compaction and no request with
  more than about 130,000 tokens of context.
- **A stalled worker.** Stop a worker mid-ticket. Expect one message after 30
  minutes, then the ticket released, the tree reset and a comment on the ticket.
- **A waiting worker.** Have a worker run `ignite wait` on a landing that is
  held behind a lease. Expect the dashboard's pool view to show the command and
  to mark it once it has run for 10 minutes.
- **A correction on the ticket.** While a worker is mid-ticket, comment on its
  ticket from another session. Expect the worker's next `ignite` command to
  print the comment as a `changed` line, once, and never its own comments.
- **A correction now.** While that worker is inside `ignite wait`, run
  `ignite tell <ticket> "..."` from another session. Expect the wait to return
  at once with the text, the session to receive it as a message, and the text to
  be a comment on the ticket. Run `ignite tell --all` with two workers busy on
  two machines and expect both to receive it.
- **Closing without landing.** Start a ticket and run
  `ignite close -m "obsolete: ..."`. Expect the ticket closed with the reason,
  the tree free and nothing on `staging`.
- **Released work travels.** Start a ticket on jarvis, commit once, leave one
  file edited and uncommitted, and run `ignite release`. Start the same ticket
  on multivac. Expect the commit and the uncommitted edit there, and a brief
  that says what it resumed from.
- **A ticket that needs a person.** Release a ticket with `--needs person`.
  Expect it on the dashboard's first line, never handed off, and handed off
  again once the label is removed.
- **An inactive epic.** File an epic with ready tickets and no `--workers`.
  Expect no hand-off from it. Set `--workers 1` and expect one.
- **A pause.** Run `ignite pool pause` with two workers busy and tickets ready.
  Expect both to finish, no new hand-off, and hand-offs again after
  `ignite pool resume`.
- **A worker that only looks stalled.** Leave a worker reading for 31 minutes
  with no `ignite` command. Expect the dispatcher to look at it, find it
  running, and send nothing.
- **A hung command.** Add a check that never returns, with its hang limit set to
  one minute in the rehearsal. Expect it ended with its child processes, marked
  `incomplete` as hung, not run again, and a P1 ticket listing the processes.
- **A machine's own failure.** Make a portable check fail on multivac only, by
  breaking a tool there. Expect jarvis to run it again and pass, landings on
  jarvis not blocked, and a P1 ticket for multivac.
- **A late hand-off.** Give a hand-off's prompt to a session after its
  reservation has lapsed. Expect `ignite start` to exit with code 3 and claim
  nothing.
- **Improvement share.** File several improvement tickets and twenty product
  tickets. Expect at least three of the next twenty hand-offs to be improvement
  tickets, largest saving first.
- **A batch job.** Start a tournament with `ignite run --class batch` on cygnus
  and then several landings. Expect the tournament to keep its floor of cores
  and the landings to take the rest.
- **Shares between epics.** Give one epic `--workers 5` and another
  `--workers 2`, each with twenty ready tickets, and a pool of seven. Expect
  five and two in progress. Close all but one of the second epic's tickets and
  expect six and one.
- **A light ticket on a busy machine.** Fill the machine with core-crate tickets
  until `ignite status --admission` shows the next one waiting. File a ticket
  that names only a Python file. Expect it handed off at once, in the tree with
  the oldest build, with no Rust build started.
- **A plan.** Give `/ignite` a plan document. Expect an epic filed in one batch,
  a `plan check` report, and the planning session ending while the pool starts
  the ready tickets.

### Operations and measurement

These show that the system survives its own failures and measures itself.

- **A daemon restart.** Restart the daemon during a long test run. Expect the
  run to finish and its result and cost to be recorded.
- **The hub offline.** Stop the hub. Expect `ignite test` to keep working on the
  other machines, `ignite land` to exit with code 4, and queued records to
  arrive when the hub returns.
- **Rebuild counts.** Run `ignite rebuild kernel-body` twice, once beside busy
  loops. Expect counts within 1% of each other. Add a dependency from a core
  crate to a large one, measure again, and expect a larger count that names the
  added crate and files no ticket.
- **Fleet metrics.** After tickets have run on both machines, expect
  `just efficiency coverage` on jarvis to list rows for each of them.
