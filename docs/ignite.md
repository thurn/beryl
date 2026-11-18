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

- [`docs/checks.md`](checks.md) describes the fast check, slow CI, the sweeps
  and the CPU token pool that Ignite replaces. Its measurements of flaky checks,
  budgets and incremental builds are the evidence for several decisions here.
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
- Agent sessions run in the Claude and Codex desktop apps on two machines:
  jarvis (a Mac with 18 cores and 64 GB, the main development machine) and
  multivac (a MacBook Pro).
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

### One of two machines is unmeasured

Jarvis runs Claude Code sessions, and multivac runs Codex sessions. About 285
tickets ran on multivac and a machine since retired, and the efficiency store
holds no data for them, because moving data between machines is a manual export
and import.

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
- **Fleet coverage**: one machine of three measured today. Required: all three.

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
- `ignite why <check>` prints a check's record: its cost, its failures, where it
  runs and why.
- `ignite status` and `ignite doctor` print the machine's state and health.

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
  the machine's daemon over a local socket, prints the reply and exits.
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
- After that the daemon runs the executable built from `master`. When `master`
  moves and Ignite's own source changed, each daemon builds the new executable
  in a tree of its own and restarts itself.
- Each build is stored under its commit, so a new executable never replaces a
  file that is running. Supervisors finish under the version that started them.
- The hub updates first. A daemon whose version differs from the hub's finishes
  its running commands, updates and reconnects before it asks for new work.
- Each record names the Ignite commit that produced it.

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
- Releasing a tree resets it to a clean checkout and keeps its build
  directories.

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
- The daemon reads the machine's total CPU use from the operating system every
  few seconds. What its own commands do not account for belongs to processes
  outside Ignite, such as the desktop app, and it grants only what is left.
- `tip` and `batch` each have a floor of cores per machine that higher classes
  cannot take, so `master` keeps moving and an experiment is slowed but never
  stopped.
- `idle` work starts only while the machine is under 70% busy, and is the first
  thing stopped when a higher class waits.
- Time spent waiting for cores is recorded apart from a command's own time.

The daemon also records each command's peak memory. It delays a command whose
family usually needs more memory than is free.

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
  of its own the supervisor also reads those descendants about once a second
  while they run.
- Each compiler process is counted by the `RUSTC_WRAPPER`, and each Rust test
  process by a Cargo target runner, so costs are known per crate and per test.
- Where one process runs many checks, as pytest does, the process's instructions
  are divided among its checks by the seconds each reports.

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
- `list` prints the family's checks, each with any inputs of its own. `run` runs
  the checks named.
- `always = true` marks a family that blocks every landing its inputs touch. See
  [Where regression checks run](#where-regression-checks-run).

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
- A failure is reused too, with a pointer to its saved output. `--rerun` forces
  a fresh run.
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
- A cost known only in CPU seconds, from history imported at the switch, is
  converted at the rate measured on jarvis, about 17 billion instructions per
  CPU second, until the check has run under Ignite.

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

```sh
ignite ticket create "Split beryl_session's replay code" -p P2 \
  --parent Tig9a --labels tier:opus --deps Tig9a.1 --body-file body.txt
ignite ticket ready --limit 10 --brief
ignite ticket wait Tig9a.1 --status closed
```

Labels carry what the dispatcher needs to know about a ticket: `tier:opus`,
`tier:sonnet` or `tier:light` names the model it needs, and `lane:<name>` a
chain. `ignite ticket batch` creates or changes many tickets in one request that
succeeds or fails as a whole.

Every change to anything in Beryl has a ticket. `ignite start "<summary>"` makes
that cheap for a session that was handed a task directly: it files the ticket
with the label `adhoc`, claims it and leases a tree in one command.

The hub also serves the dashboard, the one page meant for a person. It keeps the
current ticket views and adds each machine's cores and trees, the pool and what
each worker holds, the landing queue, the state of tip verification, open
regressions and each exploration family's finds.

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

Each machine has one standing pool that serves every epic and every stray
ticket.

- A person adds workers in a turn they start, because the desktop app lets only
  such a turn start sessions, at most ten per message: `/ignite pool 8`.
- The pool outlives epics. Nothing is rebuilt when a plan starts or ends.
- Claude Code workers are cleared and prompted again for each ticket. Codex has
  no clear operation, so a Codex worker is a new chat per ticket.

### The dispatcher

The dispatcher is one session per machine and per app whose whole job is to make
the tool calls the daemon asks for.

- It runs on Sonnet 5.5, in a folder of its own outside the repository, so its
  fixed prefix holds none of Beryl's instruction files.
- That folder's `.claude/settings.json` sets `autoCompactWindow` to 120,000
  tokens, so its context never grows the way an orchestrator's does today.
- It never plans, reads tickets or decides anything.
- Codex has no such setting. When a Codex dispatcher's context passes the same
  size, the daemon's next instruction has it open a new chat as the dispatcher
  and stop.

Its loop is one blocking command and the tool calls that command returns:

```text
$ ignite dispatch --wait 540
```

```json
{"id": 4812, "do": "handoff", "session": "local_c8470108",
 "clear": true, "model": "opus", "effort": "high",
 "prompt": "Run `ignite start Tq44s.9.48` and follow what it prints."}
```

The dispatcher clears that session, sets its model and sends the prompt, each as
one call, then reports `ignite dispatch ack 4812 ok` and waits again. The other
instructions are `message`, which sends text to a live session, and `idle`,
which means the wait ended with nothing to do.

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
  bring into its gate.

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
  dispatcher send the session one message asking it to continue or release.
- When 15 more minutes pass without a command, the daemon releases the ticket,
  resets the tree and comments on the ticket with what the tree held.

### Planning

The `/ignite` skill is how a person starts work. It replaces `/enact` and
`/enact-plan`.

- Given a ticket or a small task, the session does it itself: `ignite start`,
  the work, `ignite land`.
- Given a plan or a large task, the session settles the design, files the epic
  and its tickets in one `ignite ticket batch`, runs `ignite plan check`, fixes
  what it reports, and stops. The pool takes the tickets. The planning session
  runs no loop, which is what removes today's orchestration cost.

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
[machine.multivac]
os = "macos"
ticket_trees = 8
landing_trees = 2
families = ["rust", "clippy", "format", "source-rules", "python"]
floor = { batch = 2, tip = 2 }
```

`families` lists the check families the machine can run, here the Rust tests,
clippy, the formatters, the source rules and the Python tools' tests. A machine
with no such list can run every family.

What is shared, and what is not:

- **Tickets, the result cache and records are shared** through the hub.
- **Build output is not shared.** Incremental builds belong to one path on one
  machine. The fleet grows by placing whole sessions, each with a build tree, on
  machines with cores.
- **A machine is known by an ID** the daemon creates and stores, not by its
  hostname, so renaming a machine does not split its history.

How the machines divide verification:

- A check with `platforms = "any"` needs one pass anywhere, on any machine.
- A check with a platform list needs a pass on each platform it lists.
- A daemon prefers the checks only it can run, then claims portable ones.

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
edit = "append a no-op statement to one function in beryl_kernel"
products = ["clippy", "rust-tests"]

[[rebuild]]
name = "one-card"
edit = "append a no-op statement to one card file"
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

- `.enact.toml`, the `/enact`, `/enact-plan`, `/enact-sweep` and `cifix` skills,
  and every reference to `enact`, `tk` and Enact's roles (Vizier, Executor,
  Warden, Steward and the rest).
- The orchestration modules in `tools/` and their tests: the token pool, the
  lanes, test selection, the run log, verdicts, budgets, hang limits, flaky
  handling, bisection, slow CI, the sweeps, the nightly run, the build-cache
  copying and the incident writer.
- The `just check`, `just ci`, `just sweep` and `just nightly` recipes and the
  background agents that run the nightly, the sweeps, the CPU sampler and the
  ticket service. Every other `just` recipe stays, `just efficiency` among them.
- Every worktree and build cache under the old layout.

What the switch rewrites:

- The root `AGENTS.md` sections on Enact and on checks become one short section
  that names `ignite prime`.
- `docs/checks.md` is deleted. This document and `ignite help` replace it.
- The efficiency dashboard's sources change as described under
  [Records and metrics](#records-and-metrics).

The switch happens when the checks under [Manual QA](#manual-qa) pass on all
three machines in a rehearsal:

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
multivac. They are the condition for the switch.

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
  Expect multivac to claim and run the portable Rust checks for that tip, and
  jarvis to show them as cached with multivac as the source.
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
  check off on all three machines, and one P3 ticket quoting both failures.
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
- **The dispatcher.** Add four workers with `/ignite pool 4` and start the
  dispatcher. File six ready tickets. Expect four hand-offs at once and two as
  workers finish, and each worker titled with its ticket. After 50 hand-offs,
  expect the dispatcher's transcript to show at least one compaction and no
  request with more than about 130,000 tokens of context.
- **A stalled worker.** Stop a worker mid-ticket. Expect one message after 30
  minutes, then the ticket released, the tree reset and a comment on the ticket.
- **Improvement share.** File several improvement tickets and twenty product
  tickets. Expect at least three of the next twenty hand-offs to be improvement
  tickets, largest saving first.
- **A batch job.** Start a tournament with `ignite run --class batch` on
  multivac and then several landings. Expect the tournament to keep its floor of
  cores and the landings to take the rest.
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
- **Fleet metrics.** After tickets have run on all three machines, expect
  `just efficiency coverage` on jarvis to list rows for each of them.
