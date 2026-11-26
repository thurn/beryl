# Ignite: builds, checks and delivery for Beryl's agents

Ignite is a build, check and delivery system written in Rust for Project Beryl.
It replaces Enact, the tool that manages worktrees, the task queue and CI, and
the Python programs in `tools/` that choose, run and judge checks. Only coding
agents use it.

Ignite exists to shorten one number: the wall-clock time from a ticket being
claimed to its work **landing**, which means its commits reaching the shared
`staging` ref. It does that by owning every build, test and check an agent runs,
on every machine, so that one program per machine can see all of the work and
avoid doing any of it twice.

The design rests on five decisions:

- **Ignite owns the agent's edit-build-test loop**, not only the checks before a
  push. That loop is where most CPU and about a third of a session's time go.
- **Cargo stays the build tool.** Ignite adds, above Cargo, what hermetic build
  systems do well: declared inputs, a hash of them, and never rerunning a check
  whose inputs already passed.
- **A check blocks a landing when its past failures on similar changes justify
  its cost**, however busy the machine is. Under load Ignite stops background
  work, and the set of checks stays the same.
- **Workers run as headless coding-agent CLI sessions.** The daemon starts them
  as the machine has room, and the hub chooses the ticket and provider. A person
  can watch and steer a worker from the dashboard.
- **The system repairs itself as ordinary work.** No scheduled agent fixes CI. A
  regression goes back to the ticket that caused it. The operator can give
  workflow improvements a share of the workers through Allocation.

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
  documents the agent's model and context settings.

## What the measurements show

This section shows where time, CPU and money go today, because those numbers
decide what Ignite is. It starts with the names the numbers use.

### How work flowed on 2026-11-17

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
- **`master` lag**, the minutes from a landing until `master` reaches it. Today
  `master` is behind `staging` for 1,728 minutes a week. Expected: `master`
  trails a landing by the tip budget plus the gate, about six minutes on an idle
  machine. A tip round runs its best-scored checks for `[tip] seconds` (300) of
  running checks before `master` moves, and the rest afterwards, so a landing
  that changes inputs nearly every check reads, such as the kernel crate, no
  longer holds `master` for the whole suite. See
  [Tip verification](#tip-verification).
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
- **Landing** is delivering a ticket's commits to the `staging` ref. The
  **landing gate** is the set of checks that must pass first, on the commit the
  change was written on. A pass is stored as the change's **verdict**, and the
  landing rebases the commits onto the newest commit and pushes them without
  checking again.
- The **tip** is the newest commit on `staging`. **Tip verification** runs the
  checks the gate left out. It runs the build checks first and moves the
  **`green`** ref, where new tickets start, to a tip that compiles. Then it
  moves `master` forward once the budgeted checks pass. A landing that broke the
  build or caused a required check to stall `master` can be
  [**reverted**](#a-broken-build-is-reverted). Other failures get regression
  tickets and are fixed forward.
- **Ignite's branches and internal refs**: `staging` and `master` are explicit
  remote branches (`refs/heads/staging` and `refs/heads/master`, tracked as
  `origin/staging` and `origin/master`), and the main checkout keeps local
  `staging` and `master` branches tracking them. Ignite's internal refs —
  `green`, the **release ref** that daemons build Ignite from, and the saved
  work of released tickets — live under `refs/ignite/` on the remote
  (`refs/ignite/green`, `refs/ignite/release` and
  `refs/ignite/tickets/<ticket>`, set in `ignite.toml`'s `[repository]`).
  Because those live outside `refs/heads/`, GitHub lists only `master` and
  `staging` as branches, while git fetches and pushes the internal refs as it
  does branches (copied to `origin/ignite/green`, `origin/ignite/release` and so
  on). A push by hand to an internal ref names the full ref:
  `git push origin <commit>:refs/ignite/release`.
- A **worker** is a headless CLI session that implements a ticket. The **pool**
  is a machine's set of workers. A **hand-off** is a reservation used to start a
  worker.

An agent meets all of this through one command, `ignite`.

## The agent's commands

An agent's whole workflow is a handful of verbs. Each prints one compact result
and tells the agent what to do next.

A ticket from claim to landing looks like this:

```text
$ ignite start TQ44S.9.48
ticket   TQ44S.9.48  Beryl 3D manual table: counter names, untap lock
tree     /Users/dthurn/beryl/.worktrees/tree-07  at green 8c1f2ab
title    ⚒️ [TQ44S.9.48] Counter names, untap lock
files    rust/beryl3d/crates/beryl_scene/src/table/badges.rs  (+4, below)
needs    TQ44S.9.31 landed 2f0c1d9: "badges draw from one record pool"
next     set the title first, then cd to the tree, read the files and edit
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
$ ignite check
gate     38 checks: 31 from cache, 7 run, all passed in 52 s
verdict  stored for this tree
```

```text
$ ignite land -m "Named counters draw as rows above the plaque."
verdict  passed for this tree 6 min ago on 8c1f2ab
rebased  onto staging 91d07be without a checkout: 4 commits landed since 8c1f2ab
landed   3a9e4c1 on staging; ticket closed; tree-07 released
title    ✅ [TQ44S.9.48] Counter names, untap lock
next     nothing; wait for your next ticket
```

The verbs an agent uses while working:

- `ignite start <ticket>` claims the ticket, leases a build tree at `green` (the
  newest tip known to compile), starts building it, and prints the ticket with
  what its dependencies left behind. `ignite start "<summary>"` files a ticket
  first, for a session that was given a task and no ticket.

- `ignite test <names>` runs the checks the agent names, in its tree. A name
  without a known `<family>:` prefix is looked up in each family's kept listing
  for the tree's inputs or, when it has none, in the newest listing the machine
  keeps of it, and only the families that match, or that were never listed on
  the machine, are listed. A change to one card file changes the inputs of most
  Rust families, and listing them all again built every product:
  `ignite test oust` after a one-line comment change in Oust took 39 minutes on
  2026-11-19. A name that no listing holds may name a check added since, so then
  every family without a kept listing for the tree's inputs is listed too, in
  two rounds: first the families that are cheap to list, then, only when some
  name still matches no check, those whose listing needs a product build. The
  families left out are named on the `skipped` line described below. On
  2026-11-24 `ignite test a_turn_past_small_does_not_narrow`, a test the branch
  had just added to an engine crate, listed all 23 families in 1241 s, most of
  it waiting for the 3D client and the scenario dump to build, to run two tests
  that the Rust family's listing found. A name that no family could hold is
  refused at once with a hint on how checks are named, so a Rust test function
  name never builds every family. When every family has a listing on the
  machine, a name that no listing holds is also refused at once if it cannot
  name a check added since: its text before the first `:` is shaped like a
  family name but names no family (`rust-test:beryl_card`), or it is a test name
  or a `::` path to one whose last part no file of the tree holds, tracked or
  untracked. A new test's name is in the file the branch wrote it in. On
  2026-11-25 `ignite test rust-test:beryl_card` listed every family for 449 s,
  and `the_plain_card_back_is_brown`, which no file held, for 1040 s, before
  each said that no check matches. A name with a real family's prefix still
  lists that family afresh.

  When every name matched, a `nextest` family found only through its newest
  listing, whose `list` says `{packages}`, is built and listed for the packages
  of the matching tests and the packages the branch changes, and that listing is
  not kept. Listing a Rust test family builds every test binary it lists: on
  2026-11-19 `ignite test fork` after a one-line change to a card file
  recompiled 35 crates (33 s alone, 77 s beside other builds) to run one test of
  one package. A test added in a package the branch does not change, whose name
  matches as well, is left out until the family is listed whole.

  A family whose listing needs a product build (its listing is not kept for the
  tree, its `list` needs a product, and it cannot be listed for some packages
  only) is left out when every name it matches also matches a family that is
  cheap to list. A `skipped` line names it, with the prefixed name that runs its
  checks. On 2026-11-19 `ignite test fork`, after a one-line change to the card
  Fork, matched one Rust test and also the recordings of Fork in two families of
  Beryl 3D's interface checks, whose listing needs the client built; it spent 24
  to 55 minutes listing those families before it ran the Rust test. A family
  never listed on the machine whose listing needs a build is likewise listed
  only when some name matched no cheap family.

  The families are listed, and then run, each on a thread of its own, so a
  family whose `list` needs no product is listed while another family's product
  builds, and the products of different families build at once where the
  scheduler grants the cores. Two families that ask for one product share its
  build, which is reported once. A command that took 5 s or more ends with a
  `took` line that splits its time into its stages (opening the tree, finding
  families, listing, running, and `other` for what none of them covers), and a
  `slow` line that names every other step of 5 s or more, such as keying a large
  family's checks or waiting for a build another command started. On 2026-11-19
  `ignite test fork` after a one-line comment change took 300 s, its lines
  accounted for 130 s, and its three products had built one after another.

- `ignite check` runs what the landing gate would run on the tree as it is, and
  is what a change needs before landing. When the gate passes with nothing
  incomplete, it stores a verdict for the tree (see [Processes](#processes)) and
  says so on a `verdict  stored for this tree` line, or
  `verdict  not stored: <reason>`. Its express path stores none. In verdict mode
  it first refuses a change that mixes Ignite's own source with files outside
  the `[express] paths`, as `ignite land` does (see
  [Landing by verdict](#landing-by-verdict)).

- `ignite sync` rebases the tree's branch onto the tip, or onto `green` when
  `repository.green` is set and the ref exists.

- `ignite land` lands the checked commits: it finds the tree's verdict (or runs
  the gate when there is none), rebases the commits onto the tip without a
  checkout, pushes them, records the note for dependent tickets, closes the
  ticket and releases the tree. See [Landing](#landing). The note is the landed
  commits' subjects, one per line; `-m "<note>"` gives an explicit one.

- `ignite run -- <command>` runs any other heavy command with cores from the
  daemon, and records it. A `cargo` on the tree's `PATH` does the same for plain
  `cargo` calls.

- `ignite ticket <verb>` creates, shows, lists, links and comments on tickets.
  `ignite ticket children <epic> --open` lists an epic's unfinished tickets with
  their assignees, blockers and last comments, and `ignite ticket dep list <id>`
  shows what a ticket waits on and what waits on it.

- `ignite why <check>` prints a check's record: its cost, its failures, where it
  runs and why. Outside a build tree, such as in the main checkout, it prints
  only the hub's record, since the cache and the gate's choice depend on a
  tree's change.

- `ignite status` and `ignite doctor` print the machine's state and health. If
  `[hub] url` reaches the hub's read-only listener, every write is answered 405;
  the daemon does not retry it, logs one line naming the URL and the fix, and
  logs nothing more about it until a heartbeat succeeds. `doctor` also probes
  the hub's write path with one tokenless `POST` to the heartbeat route: the
  writable hub refuses it with 401, while the read-only public listener
  (`IGNITE_HUB_PUBLIC_LISTEN`) answers 405, and `doctor` then reports a
  `listener` problem naming the URL to point at the writable listener.

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
  `ignite stop <handle>` ends a running command of the session's own. See
  [Steering work in flight](#steering-work-in-flight).

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
| 6    | Paused for an emergency; `ignite wait emergency`      |

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
- **A verb process** runs one long verb from start to finish:
  `ignite internal verb <handle>`, a hidden subcommand. `land`, `test`, `check`,
  `run`, `rebuild`, `audit` and the internal verbs `idle-job` (one idle job) and
  `tip-round` (one round of tip verification) run in verb processes; every other
  verb runs on a thread of the daemon. The daemon gives the verb its handle and
  records the request in the handle's row in `daemon.db`, then starts the verb
  process from its own executable, `bin/<commit>/ignite`, in the repository's
  main checkout, in a process group of its own (detached and outside the
  daemon's job object on Windows, as supervisors are). The verb process holds
  `verbs/<handle>/lock` in the Ignite home for its whole life, records its
  process ID, its executable's commit and its protocol version in the handle's
  row, runs the verb, writes the reply into the row and exits. Its log is
  `verbs/<handle>/verb.log`. It starts no process itself: it asks the daemon for
  what needs one authority on the machine over the daemon's socket, one
  connection per call (the host protocol, calls named `host-…`): starting a
  supervised command, which includes waiting for cores, making a build product
  current, leasing and freeing a landing tree, and reporting its step and
  whether it may be stopped. Each call carries a request ID, the handle and a
  counter, so a call cut off by a daemon restart is sent again, for up to five
  minutes, and answered as the first attempt would have been. A verb process
  waits for its own commands through their run directories, as a restarted
  daemon does.
- **The hub** runs on jarvis as a login service of its own, `ignite hub`,
  started from `bin/current`. It serves the shared database over HTTPS on the
  Tailscale network (`IGNITE_HUB_LISTEN`), and a read-only copy for the public
  address (`IGNITE_HUB_PUBLIC_LISTEN`), and does nothing else. The public copy
  serves the dashboard's pages and, of the API, only the change feed their
  script follows; every other API route answers 404 there. So a daemon restart
  or crash loop on jarvis never takes the hub away from the other machines, and
  the hub's own restart takes well under a second.

How the executable is installed and kept current:

- `ignite install`, run once per machine from a checkout, builds the executable
  with plain Cargo, registers the machine with the hub and sets the daemon to
  start at login. Registering stores a write token in the user's configuration.
  Running `ignite install` again on a registered machine keeps that token, so
  the daemon and the verb processes that hold it go on writing; a client the hub
  refuses with 401 reads the token from the file again and retries once before
  it reports the refusal. The hub accepts only requests that come over the
  Tailscale network with a registered token, apart from the dashboard's ticket
  edits described under [Tickets and the hub](#tickets-and-the-hub).
- Install runs the steps after the build (registering and writing the login
  services) in the executable it just built, so an older `ignite` never writes
  the services its own way. It records the repository's main checkout for the
  daemon, even when run in a linked worktree, and links `~/.local/bin/ignite` to
  the current build when that directory is on `PATH`, or says where to link it.
- On the hub's machine, install also writes the hub's login service,
  `ignite.hub`, which runs `bin/current hub` with the hub's listen addresses.
  Install takes them from its environment, or from the services already
  installed (the hub's, else the daemon's), so running it again keeps them. The
  daemon's service never carries them. A daemon that still has
  `IGNITE_HUB_LISTEN` on a machine without the hub service serves the hub itself
  until install moves it. When the hub cannot be reached, install serves it
  itself for the length of the registration, then stops before the hub's service
  starts.
- After that the daemon runs the executable built from the release ref,
  `repository.release` in `ignite.toml` (`refs/ignite/release` here), or
  `master` when that is unset. When the ref moves and Ignite's own source
  changed, each daemon builds the new executable in a tree of its own and points
  `bin/current` at it, so clients and the hub run it. The daemon restarts into
  it only when the release changes the code the daemon runs: anything under
  `ignite/` except the `ignite_cli` crate, test files (`tests.rs`, `*_tests.rs`,
  files under a `tests` directory), Markdown and the check manifests. A release
  that changes none of that, such as a fix to the client alone, is taken on
  without a restart: the daemon reports it as its version from then on and
  compares later releases with it. A restart that is due waits for a quiet
  window: until no newer release has appeared for `self_update.quiet_seconds`
  (5), no newer release is still building and, when the release changes the
  landing code (the daemon's `land` module), no landing runs, but at most
  `self_update.cap_seconds` (30) from the first release it waits with, counted
  from when its build started. Releases that land one after another therefore
  cause one restart into the newest: a release that lands while another builds
  is built as soon as that build ends, and replaces it as the one to restart
  into. The restart also waits, up to the same cap, until the main checkout
  holds the release: the daemon fast-forwards the checkout's integration
  branches itself, without waiting for the 30-second fast-forward loop, and asks
  whether the commit checked out there contains the release. The new daemon then
  reads the `ignite.toml` that came with the release. On 2026-11-20 a release
  that changed both `[repository]` and the daemon's code restarted every daemon
  two seconds before its checkout was fast-forwarded, so each kept the old names
  until a later release. A checkout with local edits cannot be fast-forwarded,
  so the daemon restarts at the cap on the `ignite.toml` it has. A push in
  progress holds the restart even past the cap. It never waits for running
  commands or verb processes, which survive it. Before it restarts, the daemon
  answers every new request, except the calls of verb processes, with a reply
  that says it is restarting, which the client sends again, and waits up to 30
  seconds for the verbs running on its own threads, such as a `release`. It does
  not wait for a `start` that only waits for a free tree: the new daemon runs
  that `start` again under its handle, and the rerun claims the tree if one was
  leased to the session meanwhile. Daemons fetch the ref every five minutes, so
  a fix pushed to it by hand (`git push origin <commit>:refs/ignite/release`)
  deploys too. A landing on the machine moves the release ref from a worktree of
  the main checkout, which updates the checkout's copy of the ref. The sender
  loop runs a round every 10 seconds, but it reads that copy every second
  between rounds and starts a round as soon as the ref moves, a release build
  finishes, or a landing names a release candidate (below). Before 2026-11-19
  each of those waited for the next round, up to 10 s.
- An express landing builds its release before it lands. After its rebase and
  formatting, it points the local ref `refs/ignite/release-candidate` at the
  commit it is about to check. The daemon reads the ref, and when the commit
  follows its own version, changes Ignite's source and no other build runs, it
  builds and stores that commit's executable while the landing runs its express
  commands, without a selftest or a restart. An express landing pushes the
  commit it checked, so when the release ref reaches it the build is already
  stored and only the selftest is left. On 2026-11-19 the release build took 36
  s after each Ignite landing, as long as the landing's express commands. A
  landing that fails or rebases again leaves a stored build nobody restarts
  into; a build already running is never interrupted for a candidate.
- The release build is not run by a loop. When the release ref moves, or the hub
  reports a version this daemon doesn't have, the daemon starts one build on a
  thread of its own in `update/update-tree` with a one-hour hang limit; only one
  runs at a time. The build runs as the daemon's own child process on the
  machine's express reserve, borrowing whatever cores are free when it starts up
  to 12 in all, and logs to `update/build.log`. Before 2026-11-19 it was a
  supervised `agent` command, which waited up to 7.5 minutes for the scheduler's
  cores under load and then compiled on one core. The sender loop never waits
  for it: each round it looks whether the build has finished and shows "building
  release <commit>" as its step meanwhile, and the next heartbeat after it
  finishes restarts the daemon into it, or takes it on without a restart as
  above. A failed build is logged once and not retried until another commit is
  wanted. A daemon that restarts mid-build waits for Cargo's lock before it
  resets the update tree.
- Before the daemon restarts into a new release, the release thread runs the new
  executable's selftest, `bin/<commit>/ignite internal selftest`, the same way
  as the build, on one express core (`update/selftest.log`). It parses
  `ignite.toml` and every check manifest as the daemon does at start and applies
  the build's migrations to a copy of `daemon.db`. `bin/current` points at the
  build only after it passes. A build that fails is added to `bin/rejected`, so
  no daemon updates to it, and is reported once to the justiciar; the daemon
  stays on its own build until the release ref moves.
- A restart on Windows starts the new executable with `IGNITE_RESTART_FROM` set
  to the old daemon's process ID, and the old daemon exits once the new one has
  logged its start in `starts.jsonl`. The new daemon waits up to 30 seconds for
  the old one to free `daemon.lock` and its pipe, instead of exiting because the
  lock is held. Every other start waits 2 seconds for the lock.
- Each database records `min_reader_version`: the lowest schema version a build
  must know to use it. A migration that only creates tables or indexes, or adds
  nullable or defaulted columns, is additive and leaves it alone; any other
  carries a `-- breaking:` line, which raises it, and a test enforces the rule.
  A build opens a database newer than its last migration, applying nothing, when
  its last migration is at least that value, so a rollback to `bin/last-good`
  still starts after a release that added a table or column.
- Landing by verdict has four settings: `[land] mode` (`"strict"`, the default,
  or `"verdict"`, which this repository sets) with `max_base_age_hours` (default
  12; see [Landing](#landing)), `repository.green` (unset by default; it must
  differ from `staging` and `master`), which gives tip rounds a build step (see
  [Tip verification](#tip-verification)), and `[revert]` settings `broken_build`
  (default false) and `stall_minutes` (unset by default, see
  [A broken build is reverted](#a-broken-build-is-reverted)). Their defaults
  keep every landing checked on the exact tip.
- A setting in `ignite.toml` takes effect when the process that uses it next
  starts and reads the file from the main checkout. `ignite.toml` is not
  Ignite's source, so a change to it builds no release and restarts nothing.
  Every verb process reads it again when it starts (`Base::read`, called from
  `verbproc.rs`), so `[land]` and `[revert]` reach the next `ignite land`,
  `ignite check` and tip round once the daemon has fast-forwarded the main
  checkout to the change, which it does every 30 seconds while the checkout has
  no local edits. The daemon reads it only when it starts, and it alone uses
  `repository.green` for `ignite start` and `ignite sync`, resource definitions
  for its scheduler's limits, and the refs the main checkout fast-forwards.
  Those take effect at the daemon's next restart, which the next release that
  changes the daemon's code brings about. That restart waits until the main
  checkout holds the release (see [Processes](#processes)), so a release that
  changes `ignite.toml` and the daemon's code together restarts the daemon onto
  the new file. Nothing reloads the daemon's copy. A verb process and its daemon
  can therefore disagree for a while: a tip round that reads `green` from the
  file pushes the ref even while the daemon still starts tickets at the tip.
- A verdict is keyed by the tree ID of every file in the ticket tree, committed
  or not, the merge base of the tree's `HEAD` with the tip as the checkout last
  fetched it, and `verdict_settings`, a digest of the `[gate]` section that
  holds nothing about Ignite's own release, which changes hundreds of times a
  day. The hub stores and looks up verdicts (`POST /verdicts/store`, which needs
  a registered machine's token, and `POST /verdicts/lookup`, which replies with
  the verdict or `null`) in its `verdicts` table. `ignite check` stores a
  verdict whenever its gate passes with nothing incomplete, at the hub and in
  the daemon's local `verdicts` table, and `verdicts::find` looks one up at the
  hub first and then in that table, so a landing works while the hub cannot be
  reached. Committing a tree's edits unchanged keeps its verdict, because the
  tree ID stays the same. The hub and the daemon each delete verdicts that
  passed more than 30 days ago. The gate's scope is the change's own files: the
  paths that differ between the merge base and the tree, so files landed on the
  tip since the tree started are not counted.
- Git rebases and reverts in its object database, with no working tree, for
  landing by verdict and for a tip round's revert. `Git::replay` rebases a
  branch's commits onto the tip one by one with `git merge-tree --write-tree`
  against each commit's own parent and `git commit-tree`, keeping each commit's
  author, author date and message, dropping a commit whose change the tip
  already has and leaving out merge commits, as `git rebase` does. `Git::revert`
  makes one commit on the tip, written as `ignite <ignite@localhost>`, that
  undoes a list of commits, newest first. A conflict in either returns the
  conflicting files. Neither changes any working tree, index or ref, so a ticket
  tree keeps the build `ignite check` made. Both need git 2.40
  (`merge-tree --merge-base`) and fail naming the version found on an older one.
  `Git::merge_base`, `committed_at` (the committer date) and `tree_id` (the tree
  ID of every file on disk, committed or not) go with them.
- A landing that changed Ignite's own source moves the release ref to the landed
  commit once `repository.release_family` (the `ignite` family) has passed on
  it, or the express commands have (see [Express landings](#express-landings)).
  A fix to Ignite therefore reaches every machine minutes after it lands,
  without waiting for tip verification of everything else, which an Ignite bug
  can itself be blocking.
- The check manifests (`repository.checks`, `ignite/checks/`) are not Ignite's
  source, although they lie under `ignite/`: no executable compiles them. A
  landing that changes only them judges no release, so the release ref stays
  where it is and no daemon builds or restarts. Every command reads the families
  and products it lists, builds and runs from the tree it runs in: the ticket
  tree's working directory for `ignite test` and `ignite check`, the rebased
  commit in the landing tree for a landing's gate, and the tip's tree for a tip
  round. The build manager reads each product from the tree it builds in. A
  change to a family or product therefore takes effect in the tree that makes
  it, before it lands, and never runs against sources it does not match. The
  daemon keeps its own copy for what has no tree (which directories a tree reset
  keeps, what is prebuilt and the exploration families its idle loop chooses
  from), and reads it again each time it fast-forwards the main checkout. Before
  2026-11-19 every manifest edit moved the release and restarted every daemon,
  which ended the verbs running on them.
- Whether the release moves is decided before the push. The gate's outcomes
  count when it ran every check of the release family; checks of other families
  that it left to tip verification do not matter. Otherwise the landing runs the
  whole family in the gate's tree before it takes the lease, building any
  product the family needs that the tree has never built, and a passing run
  counts with the gate's checks. A failing run does not refuse the landing; the
  release stays where it is and the `release` line says why. After the push the
  ticket closes and its tree is released first, and the only work left before
  the reply is the push to the release ref. On 2026-11-18 two landings each
  spent 80 to 135 seconds after their push building and running the `ignite`
  family, with their ticket trees still leased: one because its gate had left
  unrelated checks to tip verification, the other because its gate had not
  listed the family for lack of a built product.
- A release survives a rebase. A commit pushed to the release ref by hand is
  rebased when its ticket lands, so the release ref then holds a commit that the
  landed one does not contain. The landing moves the release ref over it anyway,
  with `--force-with-lease` against the commit it read there, when the landed
  commit holds that commit's changes to Ignite's source: some commit of the
  landed history has the same `ignite/` tree, or every commit that changed
  `ignite/` has an equivalent patch there (as `git cherry` compares them). A
  daemon that runs a commit the release ref does not contain uses the same test
  before it calls itself ahead of the release: it builds the release if the
  release holds its changes, and counts as running the release when the two
  `ignite/` trees match. On 2026-11-18 each of three hand pushes followed by a
  rebased landing left every daemon refusing new releases until someone moved
  the ref by hand and ran `ignite install`.
- Each build is stored under its commit, so a new executable never replaces a
  file that is running. Supervisors and verb processes finish under the version
  that started them, except a landing that has not begun its push when the
  release changed the landing code (see "How a restart is survived");
  `ignite status` names the commit each running verb runs under, such as
  `L-6 land under 87631b5 …`.
- The hub updates first. Before the daemon on jarvis restarts into a release, it
  kickstarts the hub's service, which comes back on the new `bin/current` and
  reports its version in every heartbeat reply. A daemon whose version differs
  from the hub's builds or finds the hub's version and restarts into it at once.
  On jarvis a hub that reports another build than the daemon (a failed restart,
  a rollback) is kickstarted again, at most every five minutes, rather than
  followed. Daemons retry refused connections and 502, 503 and 504 answers from
  the hub for 15 seconds, so a hub restart is invisible to them.
- Each record names the Ignite commit that produced it.
- `ignite status` has a `release` section: the release ref's tip, the build this
  daemon runs and what it is doing about the difference (waiting for its next
  check of the ref, with when that is due; building; self-testing; restarting,
  with when the quiet window ends and the latest the restart can wait; failed to
  build; or rolled back, with the reason from `bin/rollbacks.log`), then the
  hub's version and each machine's, which the hub reports at
  `GET /machines/versions`.
- When `ignite land` moves the release ref, its `next` line names
  `ignite wait deploy-<commit>`. That wait asks the daemon to fetch the release
  ref at its next round instead of at its five-minute check, for a release that
  moved on another machine. It then waits until this daemon, the hub and every
  machine the hub heard from in the last ten minutes run the commit or a later
  one (exit 0), or the commit was rolled back, failed its selftest or failed to
  build on this machine (exit 2, with the reason). A wait that ends first exits
  5, or 4 when the hub cannot be reached. The wait keeps no state, so a client
  cut off by the daemon restarting into the release sends it again. A rollback
  on another machine shows only as that machine staying behind.

How a restart is survived:

- Every command runs under its own supervisor, and every long verb in its own
  verb process, and both outlive the daemon.
- Every verb that does work gets its handle, with the request that started it,
  before it begins. The client is told the handle first.
- The restarted daemon looks at each handle that was running. When
  `verbs/<handle>/lock` is held, the verb process is alive: the daemon lists it
  at once for `ignite status`, `ignite wait` and `ignite stop`, and the process
  goes on with the new daemon at its next call. Nothing is run again. When the
  lock is free and the row holds a reply, the process finished while no daemon
  ran, and whoever waits gets the reply. When the lock is free and the row has
  no reply, the process died (a crash, a kill, a reboot), and only then does the
  daemon run the verb itself, as below. A verb process that dies while the
  daemon runs is noticed within a second and treated the same way, after the
  daemon ends the commands it left.
- A landing whose verb process is alive but runs another build than the
  restarted daemon, when the landing code (the daemon's `land` module, outside
  its tests) differs between the two builds, is asked to stop, as `ignite stop`
  would ask it, so that a release that fixes a landing bug reaches the landings
  already queued. On 2026-11-18 six landings went on waiting for the express
  lane under a broken daemon after the fix had reached every machine, until each
  session stopped its landing and landed again by hand. When Git cannot compare
  the two builds, it is asked to stop too. A landing whose landing code is
  unchanged goes on under its build, as a `check` or `test` does, so a restart
  into such a release does not wait for the landings that run. On 2026-11-19
  every restart waited for the landings instead, and with about 194 landings a
  day one was nearly always running, so `ignite wait deploy-<commit>` took 470
  to 563 seconds during bursts of landings. A landing whose push has begun
  refuses to stop and finishes under its build, since a push cut short may or
  may not have reached the remote. One that stops gives back its cores, lease
  and tree, and once its process has ended the daemon runs it again under the
  same handle, with the same request, so the same note and session. The run
  again starts in a new verb process, as does the run of any verb whose process
  died while no daemon ran, so it outlives the next restart too. On 2026-11-20 a
  landing run again on a thread of the daemon was cut off by each of the four
  restarts that followed in 13 minutes, since a restart waits only for landings
  whose code its release changes, and each run started its gate over. A verb
  process that dies while the daemon runs is run again on a thread of the daemon
  instead, so a process that dies at once cannot start another forever. A
  landing that agreed to stop and is still running 60 seconds later has its verb
  process ended, as `ignite stop` ends one. The stopped reply never reaches the
  session: its `ignite wait` keeps waiting and gets the new run's reply, and a
  wait that ends first starts with a line such as
  `restart  L-128 was restarted under ignite eda21bd: it had not begun its push under ignite 87631b5`.
  A `check` or `test` started under the old build is not restarted: it only
  reports on a commit, restarting it would end its running commands, and the
  landing gate runs its checks again under the new build anyway.
- For a verb that ran on the previous daemon's threads, or whose verb process
  died, the daemon runs `test`, `check`, `land` and a waiting `start` again
  under the same handle. Finished checks come from the result cache. Each
  command still running is taken over from its supervisor, found by the handle
  in its `spec.json`, instead of being started again, and the files it writes
  through its environment, such as a check's results file, are handed to the
  rerun. So results are stored once, and trees, cores and leases are given back
  as the old daemon would have. A landing finds where it got to from its record
  on disk.
- Any other verb cut off by a restart ends its handle `incomplete`, with the
  reason.
- A landing tree records the handle of the landing that leased it. A restarted
  daemon keeps a landing tree whose landing still runs in its verb process, or
  that it runs again, with the commands running in it. The rerun gets the same
  tree back, untouched if it is still at the commit the landing asks for and
  reset otherwise. Every other landing tree, and every tree left being prebuilt
  or reset, is reset at start and its commands are ended. Each command of a verb
  with a handle records its checkout's commit as `IGNITE_TREE_COMMIT`; a
  previous command is taken over only while the checkout is still at that
  commit.
- The daemon serves its socket right after it takes the machine's lock, before
  it reads its configuration, takes over runs or builds its parts. A request
  that arrives meanwhile waits until the daemon is ready and is then answered;
  `status` and `doctor` answer at once that it is starting. Trees left being
  prebuilt, reset or landed without a live lease holder are marked `resetting`
  before the socket is served and are reset by a background thread afterwards; a
  lease that needs one waits for it.
- The daemon's loops start their round over, except fleet metrics, whose first
  round waits until ten minutes have passed since the last ingest.
- A client whose connection drops after it was told the handle waits on it as
  `ignite wait` does, for up to five minutes, prints the verb's steps as they
  start, as the first request did, and prints the reply as if nothing had
  happened. A read cut off earlier is sent again. Other work answers that its
  outcome is unknown and to check `ignite status`. A refused connection, or a
  missing socket or pipe while a process holds `daemon.lock` or a start was
  logged in the last minute, means a daemon that is starting, and is retried for
  five minutes.

How a broken build or a crash is survived:

- A panic in any of the daemon's loops writes a crash record to the Ignite
  home's `crash` folder and ends the process, so that the login service starts
  it again. A loop that died while the process lived on would stop that part of
  Ignite without anyone noticing. A panic in a verb fails only that verb, and
  leaves a crash record too.
- A daemon that has run for ten minutes marks its build last-good
  (`bin/last-good`). A build that starts three times in five minutes is rolled
  back to the last-good build and is never installed again. A new release is on
  trial (`bin/trial`) until it has run for a minute: if it starts again within
  five minutes of a start that never reached that minute, it goes back to the
  release before it and is never installed again either.
- Both guards count only runs that crashed or hung. A daemon that is asked to
  stop, by `SIGTERM` (`launchctl kickstart -k`, `bootout`, an install) or by
  restarting itself into another build, records a clean stop on its line in
  `starts.jsonl` and in `bin/trial` before it ends, and that run is not counted,
  so restarting the daemon by hand or in a drill never rolls it back. A run that
  ends with no record still counts, as does one the monitor kickstarts because
  it stopped answering: the monitor marks that run hung first. On Windows,
  stopping the scheduled task ends the process with no chance to record
  anything, so such a stop still counts as a crash. `ignite install` starts a
  fresh trial for the build it installs.
- A clean stop also records its `reason` on the run's line in `starts.jsonl`:
  the release the daemon restarted into and the daemon files that release
  changed, with how long the restart waited for landings; a drill; or `SIGTERM`.
  A run's line lists in `adopted` the releases it took on without restarting.
  Together they let a postmortem count the restarts releases caused and the ones
  they avoided.
- `ignite install` also installs a monitor, a second login service that runs
  `ignite monitor` from the last-good build every two minutes. It restarts a
  daemon that does not answer, rolls back a build that crash-loops, and when
  neither helps for 15 minutes, writes `justiciar-needed.json` in the Ignite
  home and posts a notification. On the hub's machine it also kickstarts a hub
  that has not answered an HTTP request in two runs in a row. The monitor has no
  agent logic of its own, and its only hub call is the report of a stuck trees
  drive, below. A run gives up after 100 seconds, because the login service
  starts no new run while one is still running, and the notification's helper
  process runs without the monitor waiting for it. A restart ends every daemon
  in `starts.jsonl` that did not stop on purpose, not only the newest, so a hung
  daemon cannot keep `daemon.lock` through later restarts.
- Each monitor run also probes every trees root the daemon uses, which the
  daemon records in `trees-roots.json` in the Ignite home after each heartbeat.
  The probe is a child process, `ignite internal probe-storage`, that reads the
  root's metadata and lists the root and its first entry. A read that fails
  counts as an answer; a probe still running after 20 seconds means the drive is
  stuck. The monitor never reads the drive itself, because a process blocked on
  a stuck drive cannot exit, and the login service starts no new monitor while
  one runs. A stuck probe keeps holding its lock file under `monitor/` in the
  Ignite home, so later runs count the root as stuck without starting another.
- While a trees root is stuck, the monitor neither kickstarts the daemon nor
  rolls back its build, since a new daemon would block on the drive too. It
  writes `justiciar-needed.json` at once with a reason that names the root and
  says the machine needs a restart, notifies, and repeats both hourly. It also
  posts `POST /machines/storage-stuck` to the hub, which files the machine's P0
  "has stopped reporting" ticket at once, naming the drive as the cause, rather
  than after half an hour of silence. The headless justiciar's prompt carries
  the same reason. Once the drive answers again, the daemon's own failures count
  from that run.
- A machine's entry in `ignite.toml` can let the monitor restart it:
  `[machine.<name>] stuck_storage_reboot_after_s` is how long a root may stay
  stuck before the monitor restarts the machine (`shutdown /r /f /t 0` on
  Windows, `shutdown -r now` on macOS, which needs the right to run `shutdown`
  as the superuser). Unset, the monitor never restarts it. It restarts a machine
  at most once in 6 hours for this reason, and records each attempt in
  `monitor.json`. A graceful restart can hang on the stuck drive itself, so
  `stuck_storage_force_command` names a program and its arguments that the
  monitor runs if the machine has not started again 10 minutes after the
  restart.
- The force command is the operator's to install, since forcing a restart needs
  administrator rights. On Windows, create a scheduled task that runs as SYSTEM
  with the highest privileges, for example `ignite.force-restart` running
  `shutdown /r /f /t 0`, and let the user run it on demand
  (`schtasks /Create /TN ignite.force-restart /RU SYSTEM /RL HIGHEST /SC ONCE /ST 00:00 /TR "shutdown /r /f /t 0"`,
  then grant the user read and execute rights on the task). Then set
  `stuck_storage_force_command = ["schtasks", "/Run", "/TN", "ignite.force-restart"]`.
  On macOS, allow the user to run `/sbin/reboot` without a password in `sudoers`
  and name `["sudo", "-n", "/sbin/reboot", "-q"]`, which restarts without
  waiting for processes to exit.
- The daemon reports each crash record, and a loop that keeps failing, to the
  hub as a justiciar report. See [The justiciar](#the-justiciar).

How a command knows who is asking:

- The client reads the session's ID from the environment the desktop app gives
  its sessions, or from `IGNITE_SESSION` when that is set.
- The tree is the build tree that holds the working directory.

A removed tree, a finished ticket or an expired lease ends every process that
was started in that tree. `ignite release` and `ignite close` first stop every
verb running from the tree, verb processes included, and wait for each process
to exit.

## Build trees

A build tree stays at a fixed path across leases because rustc ignores
incremental caches built at another path. On a one-function kernel edit, a new
directory took 421 to 544 CPU-seconds to build the tests. A tree that had built
the previous state took 18. Each tree keeps its own build directories, and a
lease can start from a copy of another tree's build.

Ignite manages two kinds of trees:

- A **ticket tree** is leased to one ticket from `ignite start` until its
  landing or release.
- A **landing tree** stays at the tip. Tip verification builds in it, and so
  does a landing when commits landed since the branch's base, so each builds
  only what one change added. A ticket tree last built whatever the tip was when
  its ticket started, often 20 to 30 commits earlier. When nothing landed since
  the base, the landing gates in the ticket tree instead: the rebase changed
  nothing, so that tree already holds the branch's own build at the tip, and a
  landing tree would only build it again. It also gates in the ticket tree when
  that leaves less to build than every free landing tree: first fewer of the
  build directories the change's files want are missing, then fewer files that
  lie beside them differ between the tree's build and the rebased commit. A
  one-card change rebased over commits to another Cargo workspace has nothing to
  rebuild in its ticket tree, and its card crate's dependents to rebuild in a
  landing tree. That holds because a rebase writes the branch's own files again
  even where their content ends up as it was, and Ignite gives each such file
  back the modification time it had, so Cargo rebuilds only what reads the files
  that the landed commits changed.
- Idle work (exploration, cache audits and rebuild counts) also borrows a
  landing tree, but only while another landing tree stays free. Nothing stops
  idle work for a tree, and a rebuild count can hold one for a long build, so a
  landing or tip verification that arrives meanwhile still finds a tree. A
  machine with one landing tree runs no idle work that needs a tree.
- The idle loop in the daemon decides whether idle work may run and which job is
  next; the job itself runs as the internal verb `idle-job` in a verb process
  with a handle, so a restart of the daemon does not cut it. The job's landing
  tree is leased through the daemon (`host-lease-landing` with `keep_free = 1`),
  owned by the job's handle. The daemon records the handle in `idle-job.handle`
  beside `idle.json`, and a restarted daemon's idle loop that finds a running
  handle there waits for it and starts no second job. When a higher class needs
  the cores, the scheduler still ends the job's commands from the daemon's side;
  the job fails, its process exits, and the job runs again later from
  `idle.json`. The processes that share `idle.json` (the daemon, the job,
  `ignite rebuild`, `ignite audit`) read it again under a file lock before every
  change, and a job holds a second file lock for its whole run, so no two of
  them edit one tree at once. Keeping a free tree built at the tip stays in the
  daemon.

The daemon derives the landing-tree count from the machine's physical cores (see
[Derived capacity](#derived-capacity)).

### Tree roots and volumes

Each machine uses its Fleet `trees_root`, or the main checkout's
`repository.trees` directory when the setting is unset. A root change takes
effect when the daemon receives the new settings. New ticket and landing trees
are created under that root. A tree under a previous root keeps its path until
its lease ends, then the tree maintenance loop retires it. The first build at
the new path is cold or seeded from a tree already there.

On a Mac, run `ignite trees volume --size-gb <n>` on the machine that will use
the volume. It creates `BerylTrees` in the APFS container that holds the
checkout, with equal reserve and quota in decimal GB, then sets `trees_root` to
the mounted volume's `trees` directory. It enables ownership, writes
`.fseventsd/no_log` and `.metadata_never_index`, and excludes the volume from
Time Machine. If a step needs administrator privileges, the command prints the
corresponding `sudo` command. It reuses an existing volume of the same name and
size when run again.

APFS sets a volume's reserve and quota at creation. The macOS `diskutil`
interface exposes those options on `apfs addVolume`, and `apfs resizeContainer`
sizes the container.
[Apple's Disk Utility guide](https://support.apple.com/guide/disk-utility/dskua9e6a110/mac)
also exposes reserve and quota when adding a volume. To resize a tree volume,
create a volume with the new size and let Ignite move to its new root as leases
end.

`ignite trees volume --state --size-gb <n>` prepares `BerylState` with the same
volume settings. It prints the shell commands to stop the Ignite services, move
the state directory and leave a symbolic link at its original path. Run them
when the machine can stop running Ignite. On Windows, set `trees_root` and
`tree_space_gb` by hand in Fleet or with `ignite fleet machine`.

`ignite doctor` warns when the tree volume on a Mac holds other files. It also
lists Cargo `target` directories in registered worktrees outside the current
tree root and the main checkout.

A disposable APFS image was remounted with `.fseventsd/no_log` in place. A live
FSEvents stream reported a source edit in a git worktree on the image, and its
Cargo build passed. The `fseventsd` footprint was 6,684,672 bytes before that
build and 6,668,288 bytes after it.

Rules for trees:

- Ignite builds in its managed worktrees. When no ticket tree is free, the lease
  creates the next available `tree-NN`. The machine can keep up to twice its
  logical core count plus four ticket trees. At that ceiling, `ignite start`
  waits for a tree to become free. The daemon advertises existing free trees
  plus room for new ones to the hub. `ignite doctor` counts the trees present.
- `ignite start` leases a free tree and checks out a branch at the tip. With
  `repository.green` set and the ref present, the start is `green` instead,
  except in two cases. When a dependency of the ticket landed at a commit that
  `green` does not hold yet, the tree starts at the newest such commit, at once,
  and the brief's `note` line says so. A regression ticket starts at the tip of
  `staging`, because its failure is there. The `tree` line says
  `at green <commit>`, `at <commit>, where <ticket> landed` or
  `at staging <commit>`. For a ticket expected to compile Rust, it starts the
  usual builds at once, and the agent reads the ticket while the tree builds.
- The choice of tree follows the ticket. A ticket expected to compile Rust gets
  the free tree that holds the build directories beside the files the ticket
  names (`rust/beryl3d/target` for a file under `rust/beryl3d/`), and among
  those the one whose last build is nearest the tip. Any other ticket gets the
  free tree with the oldest build, so that it does not take a prebuilt tree from
  a ticket that needs one. See
  [Light and heavy tickets](#light-and-heavy-tickets).
- A ticket expected to compile starts from a copy of the Cargo target
  directories of the tree whose copy would leave the least to compile at the
  tip, when that is less than the leased tree's own build would leave. Donors
  are compared first by how many products they never built that another tree
  holds (each would be a cold build in the copy), then by how many files differ
  between their build and the tip, then by commit distance. A leased tree's
  build is taken to match its checked-out commit and its uncommitted files,
  since its ticket builds its own edits. Ranking by commit distance alone chose
  donors whose copies rebuilt most of the workspace: 85 crates for Beryl 3D and
  98 for clippy after a one-card change, because the donor had never built
  clippy or had edits of its own that its recorded commit did not show. The
  seeded tree then counts as holding the donor's products, so it can be a donor
  in turn, but its next build of each still runs Cargo, which finds the copied
  units current. The donor is a free tree or a leased ticket tree, and Cargo's
  own lock files are held while it is copied, so no build writes it meanwhile.
  On macOS each file is an APFS clone, so the copy takes a few seconds and
  almost no disk. On Windows a copy is a clone only on a volume that counts
  block references (ReFS, as on a Dev Drive), where `CopyFile` clones from
  Windows 11 24H2: a 984 MB copy on cygnus took 23 MB of free space. Anywhere
  else nothing is copied. Cargo judges a unit by the modification times of the
  files it read and by a few absolute paths. So every file of the leased tree
  whose content is the donor's takes the donor file's time, every other file is
  touched, and the donor's paths in Cargo's dependency files and build-script
  output are rewritten. The leased tree's own target directories are replaced,
  not merged, so that none of its older units looks current. Measured with two
  trees 14 commits apart, the `cli`, test-binary and release fuzz-test builds
  took 5.4, 6.3 and 0.2 CPU-s after the copy, against medians of 947, 1,070 and
  1,390 CPU-s for a cold build.
- The copy runs after `ignite start` has replied, on the thread that then
  prebuilds the tree. Under load a copy took up to 141 s, and the session could
  do nothing while `ignite start` waited for it. Every build in the tree waits
  for the copy, because the copy deletes and replaces the target directories,
  and `ignite status` shows the wait as "waiting for tree-06's build directories
  to be copied from tree-03". A release of the tree also waits for it. A file
  the session edits during the copy keeps its own modification time, so the
  units that read it are rebuilt. The daemon's log line for each copy names the
  donor, the commit its build is taken to match, how many commits and files that
  is behind the tip, the products the donor never built, and how long the copy
  took.
- A lease prebuilds the products that write a directory beside the ticket's
  files first, then the rest, each group in name order. In name order alone, a
  ticket that changed only Ignite had its tree build three of Beryl's products,
  two of them optimized, before Ignite's test build, and its express landing a
  few minutes later took 165 s; with Ignite's test build current, the express
  commands take 32 to 37 s after a one-file edit under the same load.
- While cores are idle, the daemon keeps one free ticket tree built at the tip,
  so the next lease starts with nothing to compile.
- A machine's build slots are its configured tree space divided by its per-tree
  build ceiling, rounded down to at least one. Both settings use decimal
  gigabytes. A lease alone uses no slot. A ticket or landing tree takes a slot
  when it builds compiled output or copies one from a donor, and holds that slot
  while its build directories exist. Documentation and Python work can use a
  tree without taking a build slot. A build product with only path inputs can
  declare `compiles = true` to take a slot for its output directories.
- On a machine with at least two build slots, ticket trees and idle work can
  occupy all but one. Tip verification and foreground landing gates can use that
  last slot, so leases held between commands by ordinary tickets cannot consume
  the whole budget. A landing gate can need a second tree while its ticket tree
  still holds its outputs. On a one-slot machine all work shares the slot. If it
  holds a leased tree's outputs, a cold tree waits for the lease to end.
- A build first uses its tree's slot or takes an empty one within its limit.
  When it needs to reclaim a slot, it clears the build directories of the oldest
  free tree whose removal makes room under the limit. A speculative seed skips
  the copy when there is no empty slot it can use. When no slot can be
  reclaimed, the build waits and `ignite status` shows the wait.
- After a build command ends, the daemon measures that tree's build directories
  in logical bytes. Before its next build, a tree over its ceiling is cleaned
  until its outputs are below 80% of the ceiling. Cleanup removes replaced
  objects and Cargo units first, then fingerprint variants and outputs unused by
  the last traced Cargo build, then incremental caches, and finally whole build
  directories. The daemon logs what it deleted and shows the latest cleanup in
  `ignite status`. It forgets recorded builds before deletion and holds off
  cleanup while a command is using the tree.
- Before starting a build, the daemon checks free space on the volume holding
  the configured tree root. A build waits while the volume has less than half a
  tree ceiling available, checking again every ten seconds until there is room
  or the work is stopped.
- The tree maintenance loop retires ticket trees after they have been free for
  24 hours, keeping the two most recently used free trees as warm donors. It
  removes the worktree, its build directories and its local database row. A
  restart resumes unfinished creations and retirements. Surplus landing trees
  are retired as soon as they are free.
- Releasing a tree first saves what it holds, then resets it to a clean checkout
  and keeps its build directories. See [Releasing work](#releasing-work).
- Releasing also ends every process whose working directory or executable is
  inside the tree, whether or not Ignite started it, except the caller, its
  ancestors and the rest of its process group (the other side of a pipe such as
  `ignite close -m ... | tail`). A server that an agent started by hand and
  forgot is the usual case.
- Only the session that holds a tree's lease may use it. Every `ignite` verb
  refuses any other session with exit code 3, so a session that was replaced
  cannot go on working in a tree that now belongs to someone else.
- The daemon keeps the main checkout's local `staging` and `master`
  fast-forwarded to the remote every 30 seconds, on a loop of its own
  (`ignite-main`), and only while that checkout has no local changes. Sessions
  start there and read its instruction files, and nobody works in it.

### Releasing work

A ticket can leave a session unfinished: the worker releases it, or the daemon
releases it for a worker that has gone. Either way the work is kept, because
work lost at release has to be done again from nothing.

- Release commits whatever the tree holds that is not committed, as one commit
  marked as saved at release, and pushes the ticket's branch to the remote as
  `refs/ignite/tickets/<ticket>` (`repository.saved` followed by the ticket's
  ID). If that push fails, the release fails and the session keeps the ticket.
- The next `ignite start` of the ticket, on any machine, checks that ref out and
  says so in the brief: how many commits it holds, who made them, and whether
  the last one is unfinished work saved at release.
- The saved ref is deleted when the ticket lands or closes.
- `ignite release --needs <what>` also records why the ticket was given back.
  `--needs <machine>` sets the ticket's machine pin, and `--needs macos` records
  the operating system the work needs (see
  [Machine and provider pins](#machine-and-provider-pins)). `--needs person`
  sets it aside for the person: no worker is handed it, the dashboard lists it,
  and the note must state the exact need and what was tried.
- A ticket that waits on another ticket's work is released with a dependency on
  that ticket, never held while its session watches `staging`. It is handed out
  again when the other ticket closes.
- A ticket that cannot be completed as written goes to a justiciar. Its
  acceptance may contradict its body, gate on something the worker cannot
  control (such as a timing on a loaded machine), or need a constraint relaxed.
  The worker runs
  `ignite justiciar "<ticket>: <what is impossible, and the evidence>"` and then
  releases the ticket with its exact state. This is distinct from ordinary
  difficulty, which stays the worker's job. See [The justiciar](#the-justiciar).

```text
$ ignite release --needs person -m "Needs the App Store Connect key. Tried the
  keychain and the CI secrets; neither holds it."
saved    2 commits and 3 uncommitted files as refs/ignite/tickets/TQ44S.7.12
released TQ44S.7.12; tree-04 is free; do not use this path again
title    ⛔ [TQ44S.7.12] Sign the iOS build
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

A build in a tree runs the product as that tree's own declarations say, so a
change to a product is built in the tree that makes it, and the families that
read the product list and run with the same command line that built it.

The daemon runs builds so that sessions do not compete inside one tree:

- One build runs per target directory at a time, since Cargo locks the
  directory. Requests for the same product in the same tree state join the
  running build.
- A request for a product the tree already holds is answered at once, without
  starting Cargo. This covers the 4,982 runs in 3 days that compiled nothing.
- What each tree holds and which builds failed are kept in the daemon's
  database, so a restarted daemon answers a request for a product the tree
  already holds without starting Cargo; a recorded product whose output
  directories were deleted or emptied is forgotten, at start and whenever the
  disk sweep clears a tree. A build command still running across a restart is
  rejoined: requests for the same build wait for it instead of starting Cargo.
- A build that exited with a failing code is not run again for the same inputs:
  later requests get the same failure and its output until an input changes or
  the tree is leased again. A build that a signal ended, or that hung, is tried
  again on the next request. So is one whose wrapper exited with 128 plus a
  signal's number (143 for `SIGTERM`), since a tree reset ends the processes in
  the tree with a signal that leaves no mark in the run, and one that a previous
  daemon started and a restart may have cut off. `ignite check --rerun` and
  `ignite test --rerun` build again over a remembered failure, and the `BUILD`
  line of a failed run says so.
- Before a failure is reported, a build whose output says that a tree's own
  build products are damaged (a link error, a compiler crash, unreadable
  metadata), or whose inputs another tree has built, is cleared and run once
  more. Ignite deletes the failing crates' `incremental/<crate>-*` folders and
  their `deps` files in that tree. The same applies to the Cargo build inside a
  cargo-nextest run. A build or `cargo` command that ends partway, by a signal,
  a hang or a restart, marks its target directory, so the next build there first
  clears each crate whose compile it left unfinished. On 2026-11-18 a compile
  that a restart ended left one landing tree failing to link `session` on every
  run, and `master` waited an hour for it.
- Checks waiting for a build hold no cores.
- Each product built for a command is sent to the hub as a record of verb
  `build`, with the product as its family and one compile unit per crate the
  compiler wrapper saw (crate, mode, CPU seconds, whether it was rebuilt). The
  command's `built` line names the three crates that took the most CPU seconds,
  as in
  `built ignite-tests in 319 s, 191 CPU-s, 0.6 cores busy (22 crates recompiled; most CPU: ignite_daemon 80 s, ignite_core 31 s, rusqlite 12 s)`.
  So a build that recompiled more than its change reaches says which crates it
  was, then and afterwards.
- The gate builds test binaries only for the packages whose tests it will run. A
  `beryl_rules` change does not build `beryl_session`'s tests before landing
  unless the gate chose one of them.

Building a subset of packages must not change the features of the dependencies
they share, or Cargo would rebuild those. Ignite checks this whenever a manifest
or the lock file changes, by comparing `cargo tree -e features` for each subset
it uses with the whole workspace's. When they differ, Ignite builds the whole
workspace instead of the subset, because almost every subset of Beryl's
workspace enables different features.

A product's `exclude` names packages its whole-workspace build leaves out:
`exclude = ["beryl_link_iroh"]` makes `{packages}` stand for
`--workspace --exclude beryl_link_iroh` when no packages are named, and the
comparison uses the same set. Every Cargo command that selects the same packages
then builds with the same features. On 2026-11-19 the `rust-tests` build took
the whole workspace while the `rust` family's `list` and `run` left
`beryl_link_iroh` out, which gave 18 dependencies other features. Each listing
then compiled the whole workspace a second time, unreported: about 1,000 s of
one landing's 1,160 s. A command without `{packages}` builds the same thing for
any caller, so Ignite compares no features for it.

Every compiler process runs through Ignite as Cargo's `RUSTC_WRAPPER`, which
records each unit's cost and whether it was rebuilt.

## Cores, memory and shared resources

The daemon decides which heavy command can start with the cores, memory and
shared resources available. Its reserves and class floors follow
[Derived capacity](#derived-capacity).

A command asks for cores in one of five classes. A lower class starts nothing
while a higher one waits:

| Class   | Used by                                                                      |
| ------- | ---------------------------------------------------------------------------- |
| `land`  | Landing gates                                                                |
| `agent` | Commands a worker runs itself, and the prebuild of a tree leased to a ticket |
| `tip`   | Tip verification and finding a regression's culprit                          |
| `batch` | Long experiments, such as AI tournaments                                     |
| `idle`  | Exploration checks, cache audits, rebuild counts, keeping a free tree built  |

Rules for cores:

- A command starts with as many threads as it was granted
  (`NEXTEST_TEST_THREADS`, `RUST_TEST_THREADS`, `IGNITE_CORES`). Where there is
  no jobserver (on Windows), `CARGO_BUILD_JOBS` is the grant too.

- One command takes at most two thirds of the machine's cores.

- Agent commands for a regression or a master-lag justiciar ticket go ahead of
  other waiting agent commands, even ones that have waited two minutes. Tip
  builds and landing gates still come first. Queue priority does not interrupt
  running commands. While a command granted this priority runs, `ignite status`
  names its ticket in the cores section.

- Within a class, a request that has waited two minutes goes first, in arrival
  order. The others go smallest first, then in arrival order. Each request gets
  all the cores it asked for before the next one gets any. On 2026-11-20, 16 to
  18 agent requests waited for jarvis's 15 cores. The scheduler then shared the
  free cores evenly, so each command ran on about one core. A one-card change's
  `ignite check` ran 80 minutes without finishing: its one-core listing steps
  waited behind ten-core builds.

- A build that only compiles gives a core back on each step to a waiting request
  of its own class, but only to a small one (at most two cores) or one that has
  waited two minutes. Before this rule, every waiting request counted, and with
  eighteen waiting every build shrank to one core.

- The daemon reads the machine's total CPU use from the operating system about
  once a second, and with it the CPU each command has used: its process group,
  every process those started (even one that left the group), and the children
  they reaped. What its own commands do not use belongs to processes outside
  Ignite, such as the desktop app or a virtual machine, and it grants only what
  is left. The reading compares the operating system's counters with the
  previous reading, and reports no reading, not zero, when none of them moved. A
  command whose use has not been read yet counts all its cores.

- Ignite's own use is the larger of what its commands used and what its
  processes used: the daemon and every process it started, every verb process,
  and on the hub's machine the hub, each with every process it started, and each
  process counted once. The hub and a verb process that a previous daemon
  started run outside the daemon's process tree: launchd starts the hub, and
  adopts a verb process when the daemon that started it exits. So `ignite hub`
  writes its PID to `hub/pid` in the Ignite home and holds `hub/lock` while it
  runs, and the daemon reads the PIDs of verb processes from their handles' rows
  while each holds its handle's lock. On 2026-11-19 jarvis counted the hub and a
  verb process from an older release, about two cores, as load outside Ignite
  while 9 agent commands waited for cores.

- Some of the load outside Ignite is caused by Ignite's builds. On jarvis on
  2026-11-20, `fseventsd` used about one core recording file events, four fifths
  of them for the codegen-unit object files that `rustc` writes and deletes in
  the trees' `target/debug/deps`. macOS records them for every folder, so only a
  change in what builds write would lower that. `syspolicyd` and
  `XprotectService` used about one core more scanning each newly built program
  before it first ran, about 180 ms per program. A program started by a process
  listed under Developer Tools in System Settings is not scanned.
  `ignite doctor`'s `scanning` line times a fresh program's first run from the
  daemon, warns when it is scanned and says what to add to that list. Each
  release is signed ad hoc, so macOS knows it only by its code hash and path,
  and a grant to a release would lapse at the next one. So the daemon's launch
  agent starts `bin/launcher launch bin/current daemon`: the launcher is a copy
  of `ignite`, written once by `ignite install` (or by the monitor when it
  rewrites an older agent) and never replaced, which starts the daemon and stays
  its parent. launchd makes the launcher the responsible process of everything
  the daemon starts, so the launcher is the program to list, and one grant
  lasts. Spotlight does not index the trees, because they are under a hidden
  folder.

- A command that uses well below its grant lends the idle cores to waiting
  requests. After 30 seconds of measurement a grant counts as using its measured
  use plus one core, rounded up: the larger of its average over the last 30
  seconds and over the last 5. So a grant lends slowly and takes its cores back
  within seconds when its use rises, and `idle` work is stopped first if the
  machine is then over. A grant that grows counts all its cores again until its
  use at the new size is known. Over an hour on 2026-11-18, before this rule,
  every core was granted while the machine stayed half idle and requests queued:
  Ignite's commands used about one core on average. `ignite status` shows the
  cores granted, used and counted.

- While swap grows by a quarter of a gigabyte within a minute, or free memory is
  under a gigabyte, nothing new starts and no grant grows, unless Ignite runs
  nothing.

- `land`, and a class with a floor, always get one core while they hold none,
  even when processes outside Ignite fill the machine, so a landing and `master`
  always make progress.

- `tip` and `batch` each have a floor of cores per machine that higher classes
  cannot take while a request of that class waits, so `master` keeps moving and
  an experiment is slowed but never stopped. A floor class whose commands are
  running and that waits for nothing more keeps no cores back.

- `idle` work starts only while the machine is under 70% busy, and is the first
  thing stopped when a higher class waits for cores, or for a resource the idle
  work holds. Stopping sends `SIGTERM`, then `SIGKILL` ten seconds later. While
  an update waits for commands to finish, idle work is stopped and none starts.

- An idle job that fails waits before it runs again: half an hour after its
  first failure, then twice as long after each later one, up to a day. The wait
  is kept in the idle state file, so a restart does not retry at once. After
  three failures in a row the daemon sends one justiciar report for the job
  (subject `idle:<job>`, such as `idle:rebuild:kernel-body`), and nothing more
  until the job succeeds. A failure because the hub cannot be reached is not
  counted.

- Time spent waiting for cores is recorded apart from a command's own time.

- A quick command starts at once on one core, without waiting for a grant: one
  whose last five or more runs each used under 2 CPU-seconds, that needs no
  shared resource, is not `idle` work and does not name Cargo. `uv sync`, which
  builds the Python environment most families list their checks with, used at
  most 1.2 CPU-seconds in 82 runs but had waited up to 726 s for a core.

- A machine's derived express reserve is kept for the express lane's commands,
  which run outside the scheduler (see [Express landings](#express-landings)).

- A machine's derived tip build reserve is kept for the build checks of a tip
  round, whose commands carry `Reserve::TipBuild` in their `CommandSpec`. These
  checks decide when `green` moves, and every new ticket waits on `green`. On
  2026-11-19, 8,053 of 9,692 commands started on one core, and every command of
  the slowest landing ran on one, because a higher class does not take cores
  from the running grants of a lower one. So the scheduler leaves the reserve
  out of the cores it shares, as it does the express reserve:

  - A request with `Reserve::TipBuild` takes up to all of the reserve that no
    other such grant holds and starts at once, whatever the shared cores and the
    load outside Ignite. It takes the rest of what it wants from the shared
    cores, at once or, with a cores file, by growing. Several such requests
    share the reserve in the order they were first asked for.
  - For the shared cores, a request with `Reserve::TipBuild` is scheduled ahead
    of every class, `land` included, though `ignite status` still counts it in
    `tip`. It takes free cores before any landing or agent request, the
    shrinking grants of every class give cores back to it, and its own grant
    gives none back. On 2026-11-20 tip rounds took 35 to 90 minutes, most of it
    building in `tip` behind up to 15 agent sessions, so each round tested a tip
    that predated the fixes for what the round before had found.
  - Express runs and release builds can borrow idle tip cores. Agents can borrow
    from either reserve while its owner has no running or waiting work for it. A
    tip build gets its cores in the same scheduling step, and express work
    starts on its reserve without waiting. The scheduler shrinks elastic agent
    grants where possible. Fixed-size and minimum-size grants can overlap the
    reserve owner and count against the shared pool until they finish.
  - The reserve's cores come back when the grant is released, also for a grant a
    restarted daemon took over, whose `spec.json` grant names the reserve.
  - The `cores` line of `ignite status` shows how many cores are kept for tip
    builds, how many of those the builds hold, and how many cores agents have
    borrowed from the reserves.

  The `reserve` field is left out of `spec.json` and of the host protocol when
  it is unset, so daemons of older and newer releases still read each other's
  files.

  This repository keeps 4 of jarvis's 18 cores for tip builds beside its 3
  express cores, and 2 on multivac (10 cores) and cygnus (20 threads) beside
  their 2 express cores. The daemon derives the reserves at startup.

- Classes are enforced by what the daemon grants, never by the operating
  system's priority. Work run at low priority on this machine once got almost no
  CPU for hours.

- A grant can change while its command runs. Every command runs under a
  jobserver its supervisor keeps at the grant's size, whatever its first word,
  so a Cargo build started inside `uv`, `just` or a script follows the grant
  too. `CARGO_BUILD_JOBS` is then set to the most one command may hold, so the
  jobserver, not the starting grant, limits the build. The grant grows to what
  the command asked for as cores free up. Only a Cargo command that only
  compiles gives cores back, one per second, while a higher class waits, or
  while a request of its own class waits and it holds more than its share of the
  machine, and takes them again afterwards. Test threads stay at the grant's
  first size. A command that can use more threads is told when its grant grows,
  through the file `IGNITE_CORES_FILE` names. A restarted daemon keeps resizing
  the grants of the commands it takes over. Windows has no jobserver, so there a
  grant keeps its first size.

The daemon also records each command's peak memory, and delays a command whose
usual peak (the median of its last nine runs) is more than the free memory,
while something else runs.

A daemon that restarts while commands run registers their cores and resources
again, so an exclusive resource is not handed out twice.

Check families name shared **resources** such as the oracle engines and
benchmark virtual machines. The daemon holds each resource to its capacity:

```toml
[machine.jarvis.resources]
oracle = 1
bench-vm = 1
```

Use `per_core` for a resource that scales with logical cores. The scheduler
rounds down and keeps a capacity of at least one on machines that can run checks
using the resource. A resource with a fixed count goes in the machine's table.

A long experiment registers the same way, so the scheduler counts its cores:

```sh
ignite run --class batch --ticket TIX1T.12 -- just tournament --games 2000
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
- The CPU seconds come from the leader's record, whose child fields total the
  CPU time of every descendant reaped back up to it, however briefly it ran. The
  supervisor adds the descendants that the leader's record cannot hold: those
  still running at the last reading, and those whose parent had exited (launchd
  reaps them). This matches `/usr/bin/time -l` for the same command.
- The undercount that remains is in instructions: `rusage_info` (every version
  through V6) has child fields for CPU time, wakeups and page-ins of reaped
  children, but none for their instructions, so no total for a command's
  finished descendants exists to read. A descendant's instructions are counted
  up to its last reading, and one that starts and exits between two readings (a
  100 ms `cc` run, or a cargo-nextest test process) adds none. Commands made of
  long compiler runs lose little; one that starts thousands of tiny processes
  can read low. A count that is low against the command's CPU seconds shows this
  error.
- When the command's leader exits, the supervisor reads the group one last time
  and ends every process still in it, as Windows does through the job object.
- Each compiler process is counted by the `RUSTC_WRAPPER`, and each Rust test
  process by a Cargo target runner, so costs are known per crate and per test.
  The daemon's builds and each `nextest` family's `run` command get both, with a
  units file of their own, and the units are stored on the command's record.
  (cargo-nextest's `--list` calls also pass through the runner and appear as
  units of mode `run`.)
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

Each check family is declared in `ignite/checks/*.toml`. The declaration names
the inputs, build products, platforms and commands for the family.

`ignite/checks/script.toml` declares the shared script checks and the app
families. `native-app` starts the desktop client headlessly. `native-ios` builds
an unsigned Simulator app on macOS, and `native-android` builds an Android APK
on macOS and Windows. `ignite/checks/client.toml` declares the native
workspace's build, source, replay, corpus, interface and frame checks.

```toml
[family.rust]
kind = "regression"
adapter = "nextest"
platforms = "any"
needs = ["rust-tests"]
inputs = ["crate-closure", "engine/.config/nextest.toml"]
```

```toml
[family.native-app]
kind = "regression"
adapter = "jsonl"
platforms = ["macos", "windows"]
needs = ["python-env"]
list = "uv run --quiet --project tools python -m beryl_tools.ignite.native_app list native-app"
run = "uv run --quiet --project tools python -m beryl_tools.ignite.native_app run native-app {checks}"
# See ignite/checks/script.toml for the full inputs list.
```

The fields:

- `kind` is `regression` or `exploration`.
- `adapter` says how results are read. There are two: `nextest` for Rust tests,
  and `jsonl` for every other command. See
  [Reporting results](#reporting-results).
- `platforms` is `any` when one pass on any operating system is enough, as for
  pure engine logic, or a list when the check must pass on each one named.
- `needs` names build products. `resources` names shared resources.
- `cores`, when set, is what a `run` command asks for instead of the machine's
  usual share, for a family whose work runs elsewhere, such as in a virtual
  machine, while its own process only waits.
- `inputs` lists what the checks read: path patterns, `crate:<package>` for a
  package with every workspace package it depends on, and `crate-closure` for
  the same computed per test from the package that holds it. The lock file, the
  toolchain version and the family's own commands are always included.
- `dispatch_paths` can specify the paths that make this family a requirement for
  ordinary tickets. It defaults to the path patterns in `inputs`,
  `package_inputs`, and `digest_inputs`. The family's checks still hash their
  declared inputs.
- A `nextest` family's checks also hash what their builds were observed to read,
  so `inputs` names only what its tests read at run time. See
  [Observed inputs](#observed-inputs).
- `package_inputs` maps a package to what only its checks read beyond `inputs`,
  in the same syntax except `crate-closure`, as in
  `beryl_card_build = ["rust/cards/**"]` under `[family.rust.package_inputs]`.
  Only the checks that package holds hash those entries, so the family's other
  checks keep their cached results when the files change.
- `list` prints the family's checks, each with any inputs of its own. A check
  that reads something no file holds, such as the commit subjects since the
  branch left staging, also prints a `digest` of it, which enters its input
  hash. `run` runs the checks named. In a `nextest` family's `list` and `run`,
  `{packages}` stands for Cargo's arguments that select packages, as in a
  product's build: `-p` for each package of the checks a run runs, or the whole
  workspace less the `exclude` of the first product in `needs`. cargo-nextest
  compiles every test binary of the packages it selects before it lists or runs,
  so a run of one package's tests that selected the whole workspace compiled
  every package's. A run selects the whole workspace when building its packages
  alone would enable other features on shared dependencies.
- A listing is kept under the family's name and the hash of everything `list`
  can read, in the daemon's memory, its local database and the hub, so a listing
  one machine made serves every machine whose tree has that hash. A listing is
  also checked against its checks' own inputs and the folders holding them. A
  listing with no checks, with a digest, or with refused checks is never kept,
  so `list` must read nothing beyond the family's inputs, the products it needs
  and what its checks print as their own inputs.
- `list_needs` names the products `list` reads, when that is fewer than `needs`.
  A family whose `list` reads no build output says `list_needs = []` or names
  only its Python environment, so listing it never waits for a build: Beryl's
  clippy family lists one check per package from `cargo metadata`, and waiting
  for the `clippy` product made a one-line card change build clippy only to list
  it.
- `list_without_resources = true` lets `list` run without the family's shared
  resources when it only reads files. The benchmark families use it to list
  checks while the virtual machine is busy. Their run commands still reserve the
  virtual machine.
- `listing_ignores_comments = true` says that what `list` prints cannot depend
  on the comments in a Rust file, as with test names or what a built program
  does. The family's listing hash and its listing's sources hash then cover each
  `.rs` file by its code with comments and layout removed, so a change that
  edits only comments keeps the kept listing and builds nothing to list. The
  checks' own input hashes still cover every file whole, since a comment can
  change what a check finds. A `list` that reads Rust source text itself must
  leave comments out of what it prints, as the `script` family's adapter does
  with the card names it finds.
- A `nextest` listing also keeps, for each package whose tests it can hold, a
  hash of what can change that package's test names: the package's own files,
  its observed reads, every manifest and lock file of its workspace, the
  family's path inputs and the package's `package_inputs`. A tree that misses
  the kept listing compares the family's newest listing package by package. When
  no package changed it serves as it is, and nothing is built or listed. When
  some did and `list` says `{packages}`, only those packages are built and
  listed, and their tests replace theirs in the newest listing, which is then
  kept for the tree's inputs. A package gets no tests from another package's
  code here, since the repository has no procedural macros and no exported macro
  writes a test; a workspace where one did could not use this.
- `digest_inputs` names the files a check's `digest` is computed from, beyond
  `inputs` and the inputs of the products in `needs`. With them declared, the
  family's listing is kept under a hash that covers them, and a landing that
  touches none of them leaves the family out of scope.
- `always = true` marks a family that blocks every landing its inputs touch. See
  [Where regression checks run](#where-regression-checks-run).
- `always_when` narrows `always` to changes that touch one of its path patterns.
  For any other change the family's checks are scored. See
  [The always set](#the-always-set).
- `builds = true` marks a family whose checks fail when the code does not
  compile or type-check, such as Clippy and Pyre. With `repository.green` set, a
  tip round runs these build checks first and moves `green` once they pass (see
  [Tip verification](#tip-verification)). Formatting and the source rules are
  not build checks, and neither is Ignite's own lint: a change to Ignite's
  source lands checked on the exact tip, and `green` is not held back for it.
- `covered_by` names a family whose checks find every fault this family's checks
  find. Tip verification then runs the covering family only. See
  [How long a round takes](#how-long-a-round-takes).
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
{"check": "native-app:smoke", "outcome": "fail",
 "message": "native desktop smoke failed"}
```

The `nextest` adapter reads cargo-nextest's own machine-readable output, so Rust
tests need no change. For a Rust test, `crate-closure` is the package that
defines the test with every workspace package it depends on, development
dependencies included, as `cargo metadata` reports them.

Some runs produce no verdict, and Ignite never guesses one:

- A command that ends without reporting a check it was asked to run leaves that
  check `incomplete`, which is never a pass and never a failure.
- A `list` command that fails leaves the whole family `incomplete` for that run.
- A `list` line that names a check of the family that Ignite refuses, such as a
  name with a space in it, adds the check `<family>:listing`, which is
  `incomplete` and names the refused lines, so a family that loses checks this
  way never passes.
- A tool that judges a check in several steps may write a line per step, and
  marks each line but the last `"partial": true`. A check whose last line is a
  partial pass is `incomplete`, because its later steps never reported. A
  failure in any step stands.
- An `incomplete` check is run again, at most twice more. The hub files P1
  tickets for incomplete checks one per cause, never one per check. Each ticket
  opens with the cause and says how many checks wait on it, naming the first 20.
  A build that failed or was refused is one ticket per product, "Build fails:
  <product>", with the build's output. An invalid manifest entry is one ticket
  per family and message. A command that ended without reporting some of its
  checks is one ticket per family. A ticket's signature is its cause, so a later
  sighting comments on the open ticket. One run of a family on one machine at
  one commit files at most five tickets; further causes are comments on a single
  "Too many causes" ticket. Only tip verification files these tickets: the
  landing gate tells the landing session, and a fault the change did not cause
  is filed when the tip shows it. A landing or a tip that needs the checks waits
  for them.
- A command that says it cannot run on this machine, because a tool, a virtual
  machine or a download is missing, is `incomplete` with that reason and is not
  run again there. The ticket, one per family, names the machine. A command that
  runs past its hang limit likewise files one ticket per family. A `jsonl`
  command says it cannot run with `"reason": "cannot-run"` on the check's line,
  as in the example below.
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
- A check's inputs are its family's `inputs`, the `package_inputs` of the
  package that holds it, the inputs its own `list` line names, and the inputs of
  the products its family needs. A `nextest` family's checks take only the crate
  patterns of its products' inputs, and add what their builds were observed to
  read (see [Observed inputs](#observed-inputs)).
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

### Observed inputs

A family's declared `inputs` are a claim nothing enforces. A Rust test that
embeds a data file with `include_str!` reads a file outside `crate-closure`, and
the old way to say so was a path in the family's `inputs`, which every test of
the family then hashed. For `nextest` families Ignite instead hashes what each
test's build was observed to read. Tracing system calls is ruled out: on macOS
it needs root and a weakened System Integrity Protection, and it slows every
process.

- **Where the reads come from.** The compiler writes a dep-info file beside each
  compile unit (`<target>/<profile>/deps/<unit>.d`) that lists every source file
  and every file an `include_str!` or `include_bytes!` names. A build script
  names the files it reads with `cargo:rerun-if-changed=` lines in its output
  (`<target>/<profile>/build/<package>-<hash>/output`). The target directory is
  the first `outputs` entry of the family's first product.
- **When they are read.** Right after a fresh listing: `cargo nextest list`
  compiles every test binary it lists, so the dep-info of every unit the tests
  use is current. For each workspace package Ignite keeps the tracked paths its
  units name outside the package's own directory. A path is a file, or a
  directory when a build script watches one. Dep-info files that an earlier
  build with other features left behind are read too, which only adds paths.
- **Where they are kept.** In the family's kept listing, beside its checks. A
  listing is already kept in memory, in the local database and at the hub under
  the family's listing hash, which covers every package of the workspace, so one
  machine's observation serves every machine whose tree has that hash. This is
  ccache's depfile mode in two steps: the listing hash finds the listing, and
  the listing says which further files to hash. A `nextest` listing kept without
  observed reads, as older daemons kept them, is not used.
- **What a check hashes.** `crate-closure`, the family's other inputs, the
  `package_inputs` of its package, and the observed reads of every package in
  its closure. Its products' path inputs are left out: a product lists the files
  its build reads so that it is rebuilt when they change, and the observed reads
  already name every repository file a Cargo build reads. A product built by a
  script that chooses how the tests are built, such as Beryl 3D's, has that
  script in the family's own `inputs`.
- **When a listing no longer holds.** The observed paths join the listing's
  sources hash, so a kept listing whose observed files changed is listed again,
  which rebuilds the tests and observes them afresh. A build script may generate
  tests from the files it reads, so the listing itself depends on them.
- **Scope.** The landing gate counts a kept listing's observed paths among the
  family's inputs, so a landing that edits only an embedded data file still runs
  the tests that embed it.

Files a test opens while it runs leave no trace in dep-info, so the test opens
them through a helper of its workspace: `beryl_test_files` in the game's
workspaces and `ignite_core::test_files` in Ignite's own. Each takes a path
relative to the checkout's root.

- **How a read is recorded.** For each run of a `nextest` family, Ignite sets
  `IGNITE_READS` to a fresh directory. The helper appends one line per read,
  `<binary id>\t<test name>\t<path>`, to a file there named after the test's
  process, with the binary and test as cargo-nextest names them
  (`NEXTEST_BINARY_ID`, `NEXTEST_TEST_NAME`). After the run Ignite merges the
  files into a read list per test, which is empty for a test that reported and
  opened nothing through the helper.
- **Where the lists are kept.** In the listing store, locally and at the hub,
  one entry per package, under `<family>#reads:<package>` and the package's
  coarse hash: the hash of its `crate-closure`, the family's commands and the
  toolchain. Code that could change which files a test opens is in the coarse
  hash, so one machine's lists serve every tree with the same coarse hash.
- **What a check hashes.** Two steps: the coarse hash finds the read lists of
  the package's last run, and the check's input hash covers the tracked files
  its list names, with everything above. A directory the test read with
  `read_dir` enters as its git tree, so an added file changes the hash.
- **A check with no list** is not looked up in the result cache. It runs once,
  and its result is stored under the hash that covers the files it was seen to
  open.
- **A list that changes between runs**, as when a data file names other files to
  open, is replaced by the newest run's list, and that run's result is stored
  under the hash the new list gives. Two machines that run different tests of
  one package at the same time may each keep only their own lists; a test whose
  list is lost runs once more.
- **Scope.** The landing gate counts every path the kept lists of a family's
  packages name among the family's inputs.

What observation does not see, which stays declared:

- Files a test opens without the helper. Beryl's `check-inputs` source rule
  fails a raw `fs::read`, `fs::read_to_string`, `fs::read_dir` or `File::open`
  in test code that opens the checkout (see
  [Auditing the cache](#auditing-the-cache)). A read made by library code that
  tests call, which cannot depend on the helper, goes in the family's
  `package_inputs` for the package whose tests reach it.
- Anything a `jsonl` family reads, including its Python adapter. Those families
  keep their declared inputs.
- A build that writes outside the product's first output directory: Ignite then
  observes nothing, and only the cache audits would notice.

### The result cache

The result cache maps a check and an input hash to an outcome. It is what lets
one global view save work:

- A pass is reused everywhere. The gate, tip verification and `ignite check` all
  look up before they run.
- A failure is reused on the machine that produced it, with a pointer to its
  saved output. `--rerun` forces a fresh run, and builds again over a remembered
  build failure.
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
  files a P1 ticket naming both runs, the cached run's record and the rerun's.
  The cause is an undeclared input or a check that is not deterministic.
- Because the sample is small, a repository can also lint its test code. Beryl's
  `check-inputs` source rule reads the test code of the game's and Ignite's
  workspaces and fails a raw filesystem read of a repository file: a file that
  names `CARGO_MANIFEST_DIR` (or Beryl 3D's `recordings::root()`) and also reads
  with `std::fs`, or a read in a statement with a string literal that names a
  tracked path (outside Ignite's workspace, whose tests write fixture
  repositories under the real repository's file names). The read goes through
  `beryl_test_files` or `ignite_core::test_files` instead, so that Ignite
  records it (see [Observed inputs](#observed-inputs)). Compile-time reads
  (`include_str!`, `include_bytes!`, `rerun-if-changed`) are observed from
  Cargo's dep-info files and need nothing.

## Where regression checks run

A regression check runs in one of three places: on every landing its inputs
touch, on landings where its record justifies its cost within the gate's budget,
or in tip verification, either before `master` moves (within the tip's budget)
or after. There are two budgets, both stated in seconds of running checks and
spent in instructions:

- **The gate's budget**, `[gate] seconds = 60`: what one landing spends on
  checks chosen by score before it reaches `staging`.
- **The tip's budget**, `[tip] seconds = 300`: what a tip verification round
  spends on its best-scored checks before `master` moves to the tip.

A landing that changes only Ignite runs no regression check before it lands. It
runs the express commands instead, and tip verification runs the rest (see
[Express landings](#express-landings)).

Seconds convert to instructions at the rate the class's check commands really
achieve on the machine's average load, not at a full grant of cores on a quiet
machine. The hub computes each class's rate once a day (`GET /rates`): the
median, over the seven whole days before, of each day's instructions retired per
second of wall time by that class's check commands. On 2026-11-18, landing
commands retired 17.5 billion instructions a second and tip commands 39.5, so
the gate's budget was about 1,050 billion and the tip's about 11,850 billion.
Those figures stand in until the hub has a week of its own.

The daily rate is only a starting point, because under load the seconds stretch.
On 2026-11-19, at a load average of 12 to 17, a landing spent the 469 billion
instructions its budget allowed in 173 s, 2.7 billion a second against the day's
7.8. Each daemon therefore remembers the rate its own gates achieved: the
instructions their checks were planned to cost, divided by the seconds the check
commands ran. A gate that ran checks for at least 10 s sets it, and each later
gate moves it halfway to what that gate achieved. Within an hour of the last
such gate, the next one converts its seconds at that rate, kept between a tenth
of the daily rate and twice it, so a 60 s budget runs in about 60 s.

The gate's budget belongs to the change, not to each command that gates it.
`ignite check` and then `ignite land` of the same change used to spend it twice.
The land found the checks the check had chosen in the cache, where they cost
nothing, and spent a fresh budget on the next slice of checks with lower scores.
For a one-line comment in a card file that was 398 more checks and 173 more
seconds. Each daemon now remembers, for each change, the checks its last gate
chose by score. It remembers them once the gate has planned, before anything
runs, so a gate that fails or is cut off by a daemon restart still fixes what
the next gate of the change spends. On 2026-11-20 the two checks and the first
landing of a one-card change all failed on a file missing from the machine's
cache, so no gate of it was remembered, and each gate spent a fresh budget: its
two landings ran about 3,800 checks. A change is known by its own files: the
paths that differ from `staging`. A later gate of the change charges its budget
for those checks when it answers them from the cache, with their families'
start-ups, as if it had run them. So a gate of an unchanged change chooses the
same checks and runs none of them again. A rebase over landed commits reruns
only those whose inputs the commits changed. The `budget` line names what an
earlier gate spent. Those charges can take the spending past the budget, and a
check whose result is cached is still chosen then, since answering it costs
nothing.

Building is outside both budgets. The gate runs checks on what its tree has
already compiled, as Enact's worktrees did: a product the tree has built before
is simply rebuilt incrementally, and its build time is printed on its own line.
A product the tree has never built is not built for the gate, and the checks
that need it are left to tip verification, unless they are in the always set,
the change edits them, the landing fixes their regression, or they recently
caught a product bug in an area the change touches. A tree has built a product
when the daemon built it there, or when the product's output directories are in
the tree and not empty.

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

- They are outside the threshold and both budgets. Nothing leaves one out, and a
  product one of them needs is built even when the tree has never built it.
- They are looked up or run for the exact commit that lands, so a tip that does
  not compile cannot be produced.

A family can be in the always set only for some changes. `always_when` lists
path patterns, and a change that touches none of them has the family's checks
scored like any other. `b3d-clippy` uses it. Its run checks the game's crates
again in its own target directory, so it cost about 1,900 billion instructions
on every change to the game, about four times the gate's budget. It had never
failed after a change that touched only the game. It stays in the always set for
changes to Beryl 3D itself, its toolchain and its commands.

The hub's record of a check, which scoring reads, is every cost and area count
stored under the check's name. Before 2026-11-19 the hub read only the checks
that the import of the old systems' history had registered. Every family listed
under Ignite therefore looked as if it had never run, and each of its checks was
planned as certain to fail at one billion instructions. A one-card change then
ran both `allocations` checks: 369 s of checks that cost 3,041 billion.

A check of the always set that already fails at the tip, because it has an open
regression ticket or the landing found it failing at the tip it was rebased onto
(see [Landing](#landing)), is `known-failing` like any other check. That tip
already fails it, so the landing makes nothing worse. It still runs when the
change edits its own file, and for the ticket that fixes it.

### Scores

Each regression check scored for a change gets a **score**: how likely it is to
catch a product bug on a change like this one, divided by what it costs to run.

```text
score(check, change) = P(product catch | areas the change touches) / cost(check)

P     = (product catches after changes to these areas + 20 x family rate)
        / (runs after changes to these areas + 20)
cost  = median instructions of its last 20 runs, in billions
```

The parts:

- An **area** is a workspace package, or a second-level directory outside the
  Rust workspaces. A change's areas come from its paths. For a change that
  touches several areas, P is computed for each and the largest is used.
- The cost is what running the check takes: its equal share of the command it
  ran in, after the family's start-up is taken out. Builds are not part of it
  (see above). The hub estimates a family's start-up from its recent commands of
  different sizes: where the line of a command's cost against its number of
  checks meets zero checks, kept between zero and what the cheapest tenth of its
  commands cost. The gate charges a family's start-up once, when it first runs
  one of the family's checks; a check chosen by score pays it out of the budget.
- Scores use the product fixes recorded when tickets close (see
  [Regression tickets](#regression-tickets)). Each catch counts in the areas of
  its first bad commit. A catch with no known area contributes to the family
  rate. Recorded runs still count toward the denominator, including runs whose
  failures were never classified, whose fix changed only tests, or whose ticket
  closed without a fix.
- The family rate is the family's overall share of runs that caught a product
  bug. It stands in for 20 runs, so a check with few runs starts from its
  family's record.
- A check that has never run scores as certain to fail, at its family's median
  cost, so new checks run first.
- A check whose own file the change edits always runs, outside the budget. The
  agent's new regression test is never skipped.
- A check with a product outcome in the previous 14 days runs outside the budget
  for changes that touch an area of its first bad commit. It still uses the
  result cache, and a check with an open regression is handled as known failing.
  The window starts when the fix ticket closes.
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
  P = (1 + 0.2) / 330 = 0.004     cost 5 billion       score 0.0007   at the tip
```

The second check scores below the threshold. Had it scored above, and the tree
had never built `beryl_session`'s test binary, it would still go to the tip,
where that build happens once for everyone.

### The threshold and the gate's budget

Only families the change can affect are scored (see [Landing](#landing)). A
family it cannot affect has the same inputs as at the tip, so its result there
stands.

The landing gate runs every check whose score is at or above a threshold, in
score order, until their summed cost would pass the gate's budget:

```toml
[gate]                 # example values
threshold = 0.004      # expected failures per billion instructions
seconds = 60           # of running checks per landing, builds not counted
```

- Checks above the threshold but past the budget, and checks that need a product
  the tree has never built, are counted in the landing's output with their
  instructions and run in tip verification. So is a family whose listing needs
  such a product when no listing of it is kept: it is not listed at all.
- The budget counts only checks chosen by score. The always set, the checks the
  change edits, the regression being fixed, and recent product catches in the
  change's areas are outside it.
- The budget is a ceiling, not a target: the threshold stops a landing from
  spending its 60 seconds on checks unlikely to find anything, so a small change
  lands in a few seconds.
- The threshold is set by replaying history: the largest threshold at which the
  gate would still have run 95% of the recorded product catches in the previous
  14 days by score. `ignite gate replay` repeats the calculation from Ignite's
  own records in the hub's database, which it opens read-only, and also prints
  what 95% of those gates would have spent, to compare with the budget. Only a
  commit changes the values.
- A check's score changes as its record does, so checks move across the
  threshold by themselves. No ticket is filed to move one.

`ignite check --plan` prints what the gate would run and why. Each check
selected for a recent product catch gets a `recent` line.

```text
$ ignite check --plan
always   4 checks (clippy, fmt, card-names, file-length)
budget   60 s of checks at 17.5 billion/s = 1,050 billion
gate     61 checks above 0.004; 44 cached, 17 to run, about 96 billion
tip      1,204 affected checks below the threshold
top      rust:beryl_rules::tests::entering::*      0.91  edited by this change
         script:aura/enters-attached/you           0.13  9 of 74 runs failed
```

The gate's own output puts running checks, building and listing on separate
lines, so a landing that was slow because it compiled shows as that:

```text
gate     65 checks: 48 from cache, 17 run, all passed in 41 s of running checks
budget   60 s at 17.5 billion/s = 1,050 billion: 15 checks (94 billion) within it, 2 checks (2 billion) outside it, 48 checks (48 billion) cached
build    12 s building 1 product
listed   668 s listing 3 families afresh, 9 kept; slowest: rust 640 s (600 s waiting for Cargo's lock; 31 crates compiled in 38 s), ignite 19 s, script 9 s
tip      1,204 checks left to tip verification: 1,204 checks (1,204 billion) below the threshold
```

The `listed` line, which `ignite test` prints too, counts the seconds that
families' `list` commands ran, how many families were listed afresh and how many
came from a kept listing, and names the three slowest. A `cargo nextest list`
compiles whatever its product's build did not, and waits while another Cargo
command in the tree holds the target directory's lock, so each slow family says
which. Compiling is timed from the first compile's start to the last one's end,
as Ignite's compiler wrapper records them. A lock wait is the time before the
first compile, or the whole command when it compiled nothing, and counts only
when Cargo printed "Blocking waiting for file lock", which `--cargo-quiet`
hides.

## Landing

`ignite land` is the only way commits reach `staging` (`refs/heads/staging`). It
replaces `enact push`, the ticket's closing comment and `enact close`.

A change is checked once, on the commit it was written on, and lands by that
result. `ignite check`, or the landing itself when no check has run, runs the
gate in the ticket tree and stores a **verdict** when it passes: the record that
these files passed on this base under these gate settings. `ignite land` then
rebases the commits onto the tip in git's object database and pushes them
without building or checking anything again, however many landings reached
`staging` meanwhile. What the gate could not see, two changes that each pass
alone and fail together, is found after landing by
[Tip verification](#tip-verification): a tip that no longer compiles is repaired
by reverting the landing that broke it. A required non-build failure can also
trigger a revert during a prolonged stall. Other failures are fixed forward
through regression tickets. `master` still moves only to a tip whose budgeted
checks passed.

This is `[land] mode = "verdict"`, which this repository sets. The default,
`mode = "strict"`, checks every landing on the exact tip instead (see
[Strict mode](#strict-mode)) and is the setting that restores the old behaviour.
A change to Ignite's own source lands checked on the exact commit it pushes in
either mode (see
[Changes to Ignite's own source](#changes-to-ignites-own-source)).

### Why a landing checks once

Until landing by verdict was switched on, every landing was checked on the exact
tip, and that cost grew faster than the number of landings. On 2026-11-19, 135
landings reached `staging`; the 52 that went through the gate took a median of
7.8 minutes, 33 at the 90th percentile and 91 at worst, and agent sessions spent
24.9 hours inside `ignite land`. Three costs grew with the volume:

- **The race.** A landing whose tip moved before its push had to rebase, rebuild
  what had landed and gate again. Suppose commits that touch a landing's checks
  arrive at random, `r` per hour, and one pass takes `g` hours. A pass survives
  with probability `exp(-r × g)`, so a landing needs `exp(r × g)` passes on
  average. With 7-minute passes that is 1.6 passes at 4 such commits an hour,
  2.5 at 8, 5.8 at 15, 33 at 30 and 1,097 at 60. The slowest landing of
  2026-11-19 met about 8 an hour and landed on its seventh pass. Over 2026-11-18
  and 2026-11-19, 13 landings lost at least one race and ran 23 extra passes;
  all 13 landed, and no extra pass found a failure.
- **Compiling.** A ticket tree holds the build of the commit its ticket started
  on, so a landing on the exact tip compiled everything that had landed since.
  For the 253 tickets that landed on those two days, a median of 3 commits and
  21 at the 90th percentile reached `staging` between the claim and the landing.
  Every landing compiled the same commits again, so the work grew with the
  number of landings times how far behind each one was, and both grow with the
  number of agents.
- **The scheduler.** It shares the free cores among every waiting command; with
  thirteen waiting, each got one core, and a build of 300 CPU-seconds took five
  minutes or more however many cores the machine had. 8,053 of that day's 9,692
  commands started on one core.

A verdict removes the first two: the commits are checked once, in the tree that
already holds their build, and a moved tip costs a rebase in the object database
instead of a build. What strict checking guaranteed, that `staging` never holds
a broken combination, is not needed: `master` is the branch that only holds
verified commits, and `green` is the ref new work starts from (see
[Tip verification](#tip-verification)).

### Landing by verdict

The steps, in order:

1. Check that the session holds the ticket and the tree has no uncommitted
   edits, settle a record an earlier landing of the ticket left, and run the
   formatters of `[format]` on the changed files, committing what they change
   (see [Formatting at landing](#formatting-at-landing)).
2. The base is the merge base of the tree's `HEAD` with the tip. A base
   committed more than `max_base_age_hours` (12) ago is replaced: the ticket
   tree is rebased onto the tip for real, so a verdict never rests on code half
   a day older than the tip.
3. Look up the verdict for the tree's files, the base and the `[gate]` section,
   at the hub and then in the daemon's local table. Without one, run the gate in
   the ticket tree as it is (see [The gate](#the-gate)), and store the verdict
   when it passes. A failure that the gate's rerun confirmed is compared with
   the base rather than the tip. A check that fails at the base too is filed as
   a regression and left out only when the base is the tip. On an older base it
   may be fixed at the tip, so nothing is filed against the old base: the ticket
   tree is rebased onto the tip for real and the gate runs again there, where a
   check that still fails is filed and left out. (The reply once said to run
   `ignite sync`, which rebases onto `green`, often the same old base, so
   workers released finished tickets and the pool gave them to new workers.)
4. Rebase the commits onto the tip in git's object database (`Git::replay`),
   take the hub's short `staging` lease, check that the tip has not moved, write
   the landing record and push the rebased commit. When the tip moves or the
   remote rejects the push, rebase again in the object database and run nothing,
   20 times at most. A conflict exits with code 3 and the files.
5. Tell the hub, record the commits and the files they touch on the ticket with
   the agent's note, close the ticket, and release the ticket tree parked at its
   own `HEAD`, which is what its builds match: the pushed commit exists only in
   the object database.

```text
$ ignite land -m "Named counters draw as rows above the plaque."
verdict  passed for this tree 6 min ago on 91d07be
rebased  onto staging 3a9e4c1 without a checkout: 14 commits landed since 91d07be
landed   7c1d2e0 on staging; ticket closed; tree-07 released
title    ✅ [TQ44S.9.48] Counter names, untap lock
next     nothing; wait for your next ticket
```

The reply starts with a `verdict` line and a `rebased` line, as above, then the
usual `landed`, `title` and `next` lines. When no verdict was found, the
`verdict` line is replaced by the gate's own lines and a line saying whether the
verdict was stored.

A ticket that a tip round reverted (metadata `reverted` at the hub) skips steps
2 and 3: its tree is rebased onto the tip for real and the gate runs there, so
the build checks judge the rebased code, which is the combination that failed
before (see [A broken build is reverted](#a-broken-build-is-reverted)).

### Changes to Ignite's own source

A change to Ignite's own source (a file under `ignite/` other than the check
manifests) lands as it did before landing by verdict, in either mode: by the
express lane, or through the gate on the exact tip. That path compiles and tests
Ignite on the exact commit that lands and then moves the release ref to it,
which every daemon builds and restarts into. A rebased commit that nothing
compiled could move the release ref to a build no daemon can make, and a broken
Ignite cannot run the tip round that would revert it. For the same reason a tip
round never reverts a change to Ignite, and `green` is not held back for one.

So that every change to Ignite stays an express landing, in verdict mode
`ignite land` and `ignite check` refuse a change that mixes Ignite's own source
with files outside the `[express] paths`, with exit code 1, before running
anything:

```text
refused  this change mixes Ignite's own source with other files
         rust/crates/beryl_cli/src/main.rs
next     land the files under ignite/ and the other files as two tickets
```

A check manifest with the tool it runs is not a mix.

### The gate

The gate is the same in both modes; only the tree it runs in differs. Before
anything is listed or built, it decides which families the change can affect,
from the paths that differ between the change's base and the commit it checks. A
family is left out when no changed path matches its inputs, its
`package_inputs`, the inputs of the products it needs, the lock file, manifest
and toolchain file of the Cargo workspaces those name, its manifest file, or any
input its checks printed in its kept listing. A `nextest` family with no kept
listing for the tree's inputs is judged by its declared inputs, with
`crate-closure` standing for the packages of its own workspaces, since its
checks print no inputs of their own. Any other family with no kept listing is
listed. The always set follows the same rule. Then it computes input hashes,
looks up the result cache, builds what the remaining checks need, and runs the
always set and the gate. The output's `scope` line says how many families were
left out.

A path deleted since the merge base still matches the input patterns that
covered it. The gate brings in families whose declared inputs, kept listings or
recorded reads match a deletion. An enabled check with a matching input runs
outside the score threshold and time budget, even if it needs a product the tree
has never built. The gate reports an unresolved required input by name and marks
the check incomplete.

The gate stops at the first fault it knows of:

- After scoping, it lists first the families that need no build to be listed
  (those with a kept listing, or whose `list` needs no product), each on a
  thread of its own, while every product the other families need to be listed
  builds. If any listed check's manifest entry is invalid, a listing fails, or a
  build fails, the landing is refused once those builds end, before any other
  family is listed or any check runs, and the reply names the faulty entries and
  the families it did not list. Otherwise the other families are listed at once.
- It starts every family at once, each on a thread of its own, in order of cost
  per expected failure, cheapest first, with families answered wholly from the
  cache first.
- Once a check's failure is confirmed by its rerun, it starts no further command
  in any family. What already passed stays in the result cache, and the
  `stopped` line says how many checks it did not run.
- Its output ends, as `ignite test`'s does, with a `slow` line and a `took` line
  that splits its time into opening the tree, finding the scope, listing,
  planning and running. On 2026-11-19 `ignite check` of a one-card comment
  change took 273 s: it listed 26 families one after another for 137 s, building
  each of its two products only when the listing reached the family that needed
  it, ran its families one after another for 123 s, and its output accounted for
  164 s.
- A landing's reply, landed or refused, adds a `pass` line for each pass that
  did not land, with its seconds and why it ended (the tip moved, the lease was
  lost, the push was refused), and a `landing` line with the seconds since its
  handle began, split into earlier runs that daemon restarts cut off, the passes
  before the last and the last pass, whose gate is the one the `took` line
  splits. On 2026-11-20 a landing ran for 13.4 minutes and its reply said only
  `took 28 s`.
- When judging scope, `crate-closure` stands for the packages the family's
  listed checks belong to. In a listing hash it stands for the packages of the
  workspaces the family's other inputs reach into, so a change to Ignite's own
  workspace leaves the repository's Rust families out.
- While a landing runs in a tree, `ignite land` and every other verb that acts
  on that tree are refused there with exit code 3, naming the running handle to
  `ignite wait` or `ignite stop`.

### Strict mode

With `[land] mode = "strict"`, the default, and for a change to Ignite's own
source that the express lane does not take, every landing is checked on the
exact tip it pushes. The steps, in order:

1. Check that the session holds the ticket and the tree has no uncommitted
   edits.
2. Rebase the branch onto the tip. A conflict exits with code 3 and the files.
   On the first pass, run the formatters of `[format]` on the changed files and
   commit what they change.
3. Take a landing tree, unless the ticket tree is already at the tip or leaves
   less to build (see [Build trees](#build-trees)). Check out the rebased commit
   there. The rest of the landing reads the check families and products from
   that commit, not from the main checkout.
4. Run the gate on the rebased commit, scoped by the paths that differ between
   it and the tip.
5. Take the hub's short `staging` lease. If the tip moved since step 2, release
   it and go back to step 2. The lease is taken only after the gate passes and
   held only for the push, so landings gate in parallel and never wait for
   another landing's gate.
6. Push the commit from the landing tree to `staging` (`refs/heads/staging`) on
   the git remote, and release the lease.
7. Record the commits and the files they touch on the ticket with the agent's
   note, close the ticket and release the ticket tree.

```text
$ ignite land -m "Named counters draw as rows above the plaque."
rebased  onto staging 91d07be (4 commits landed since start)
gate     38 checks: 31 from cache, 7 run, all passed in 52 s
landed   3a9e4c1 on staging; ticket closed; tree-07 released
```

A strict landing that loses the race for the tip makes its next pass cheap:

- Every pass gates in the tree the first pass chose. A landing tree stays leased
  to the landing until it ends and is checked out at each new rebased commit, so
  it builds only what the commits landed since changed. A landing that started
  in its ticket tree stays there, because the rebase moves that tree too.
- A pass after one whose gate passed judges again only the families whose inputs
  the files changed since that pass touch, by the same rule as step 4, and
  carries the others over. When the files touch none, it pushes the rebased
  commit without running anything. The output's `carried` line says how many
  families were carried over. A rebase that changes the change's own files gates
  in full.

### Rules for every landing

The machine's daemon carries out the steps for the client. The `staging` lease
lasts 180 seconds and is renewed while it is held, so a daemon that dies holding
it blocks other landings for three minutes at most. A push that the remote
rejects because the tip moved rebases again. A remote that cannot be reached
releases the lease and exits with code 4.

- A ticket closes when it lands. The session does not wait for tip verification.
- A failed gate exits with code 2 and leaves the ticket tree as it is: unrebased
  when the gate ran there by verdict, rebased in strict mode.
- `ignite land` refuses a tree whose branch is not the ticket's, and a tree with
  files that are neither committed nor ignored, naming them. Scratch output
  belongs in the session's scratch folder.
- The daemon writes a record of the landing to its own disk before it pushes. If
  the push succeeds and the hub cannot be told, the record stays queued and the
  ticket is closed when the hub returns. A record whose push was rejected is
  dropped. A landing is never pushed twice. When a push fails with no answer,
  the daemon later checks whether its commit reached `staging`; if it did, the
  daemon closes the ticket and releases the ticket tree, as the landing would
  have, unless the tree is leased to another ticket by then.
- Paths in a failed gate's output are relative to the repository, because the
  gate may have run in a landing tree that the next landing reuses. The rerun
  command works in the ticket tree.
- In strict mode the landing tree that ran a gate already holds the new tip's
  build when the landing succeeds.
- A check with an open regression ticket is left out of other tickets' gates and
  reported as `known-failing`, so one broken check fails nobody else's landing.
  The ticket that fixes it must make it pass, and the check leaves that state
  when it next passes at a tip. The output's `known` line names these checks
  with their tickets. The daemon asks the hub for them
  (`GET /regressions/open`), so a check the hub grouped onto another check's
  ticket counts too, though no title names it.
- A check can fail at the tip before tip verification has filed its ticket. So
  when every fault of a gate is a check failure that its rerun confirmed, the
  landing asks whether each of those checks fails where the change began too: at
  the tip it was rebased onto in strict mode, at its base by verdict. It runs
  them in a free landing tree checked out at that commit, or, when none is free,
  in its own landing tree, which it then checks out at the landing's commit
  again. Each check is answered from the result cache where that commit's input
  hash has a result. A check that fails there too is reported to the hub as a
  regression (by verdict, only when the base is the tip). The hub records the
  failure immediately, so other gates can use the ticket as a known failure. If
  every failure fails there too, the gate runs again with those checks
  `known-failing`, and the checks that already passed come from the cache. If
  any passes at the tip, the landing is refused and the `base` line says which
  check passed there. A check that fails at the tip counts as known to fail even
  when its failure in the landing reports more findings, because the adapters
  report no list of individual findings to compare.
- The reporting daemon later runs the same bounded culprit search as tip
  verification in a separate command. A historical probe leases a landing tree
  only while another tree stays free. On a machine with one landing tree, the
  search reports when it cannot narrow the range because it needs a probe. The
  result updates the original open regression with its culprit or remaining
  range.

### Express landings

On 2026-11-18 every fix to Ignite's own scheduling waited in the queue it was
fixing. Four fixes waited 10 to 30 minutes each for `ignite land`, because their
gates asked the scheduler for cores that it had granted to other work, often
because of the bug being fixed, and three were pushed to the release ref by
hand. So a landing that changes only Ignite does not go through the gate:

```toml
[express]
paths = ["ignite/**", "ignite.toml", "docs/ignite.md", "data/efficiency-changes.json"]
tree = ".worktrees/express"
dir = "ignite"
commands = [
  "cargo fmt --check",
  "uv run --quiet --project ../tools mdformat --check --exclude target/** ../docs/ignite.md .",
  "cargo clippy --locked --all-targets --jobs default -- -D warnings",
  "cargo nextest run --locked --cargo-quiet --workspace --profile express --build-jobs default --test-threads=num-cpus --status-level=fail --final-status-level=fail",
]
families = ["ignite", "ignite-lint"]
inert = ["docs/ignite.md", "data/efficiency-changes.json"]
```

- A landing whose every changed file matches `paths` runs `commands` in order,
  in `dir`, in its own ticket tree, which the rebase has already put at the
  rebased commit, and pushes from there. It lands when every command exits 0,
  and moves the release ref at once. A failure exits with code 2 and shows the
  end of the command's output.
- The commands run as the daemon's own processes, never through a supervisor or
  the scheduler, with `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`,
  `NEXTEST_TEST_THREADS` and `IGNITE_CORES` set to the machine's express
  reserve. An express run claims those cores before it starts, which recalls
  loans to agents. The run can still start when the scheduler is unreachable.
- When a run starts it also asks the scheduler, without waiting, for the cores
  that are free at that moment, as a `land` grant of up to 12 cores in all
  counting the express reserve, and sets the four variables to the total. Cores
  of the tip reserve that no tip build holds count as free. On 2026-11-18 eight
  landings queued behind a lane of 3 cores while the scheduler had granted none
  of its 15 cores; a run took about 164 s on 3 cores under load and about 65 s
  on 12. The reserved cores stay the floor: when nothing is free, or the daemon
  cannot be asked, the run uses its reserve and does not wait. A landing runs in
  a verb process and claims the reserve with `host-borrow-cores` (host protocol
  version 2), even when it gets no extra cores. The claim and any extra cores
  are returned with `host-return-cores` or released when the handle ends. The
  `express` line names the cores used.
- The build commands still compile on every core of the machine
  (`--jobs default`, `--build-jobs default`), and the tests run on as many
  threads (`--test-threads=num-cpus`), whatever the four variables say. Every
  process runs at the same priority, so under load a build gets CPU in
  proportion to its jobs, and the cores the scheduler leaves out are only kept
  out of its grants. On multivac (10 cores) at a load average of 50 to 110, the
  test build after an edit to `ignite_core` took 31 to 43 s on 2 jobs and 24 to
  27 s on 10. The commands run at `nice 10` were no slower, so priority is no
  lever on macOS.
- Express runs take no lock and run side by side, each in its own tree with its
  own warm target directory. Until 2026-11-18 they all ran in one shared tree,
  `.worktrees/express`, under one file lock; that evening eight landings queued
  behind it for up to 32 minutes, because each one that waited while others
  pushed found its commit stale and ran its commands again, so a queue of N cost
  about N² runs.
- Side by side, express landings still race each other for the tip. A pass that
  loses the race runs the commands again only when the commits landed since
  touch one of the change's own files, a `Cargo.toml` or `Cargo.lock` in `dir`,
  or a toolchain file or `.cargo` directory anywhere. Otherwise it pushes the
  rebased commit on the earlier pass's run, as the gate carries over families
  whose inputs are untouched, and its `express` line says "on an earlier pass".
  On 2026-11-18, before this, L-371 took 4 passes and L-372 3, each a full run
  of 70 to 230 s. The risk is a semantic conflict between two Ignite changes
  that touch different files, such as one that renames a function the other
  calls: both land, and tip verification of the `ignite` family catches the
  broken tip.
- A run that passes is remembered for an hour in the daemon's database, by the
  git tree ID of what it checked and a digest of the `[express]` section and the
  machine's express reserve. A later run of a tree with the same tree ID and
  section, as when `ignite land` follows `ignite check` at an unmoved tip, skips
  the commands, and its `express` line says "passed for this tree 4 min ago". An
  edit to `[express]` or an hour's wait runs the commands again. On 2026-11-18
  jarvis ran 17 express checks (213 s on average) beside 30 landings, so every
  change paid for the lane twice.
- A tree with uncommitted edits is remembered too, by the tree ID a commit of
  every file on disk would have, apart from ignored files. Ignite computes it in
  a copy of the index, so the tree's own index is untouched. When the agent then
  commits exactly those edits, the commit has that tree ID and the landing
  reuses the `ignite test` pass. A further edit before the commit gives another
  tree ID and runs the commands again. On 2026-11-19 two Ignite fixes each reran
  the commands once or twice after a passing `ignite test` in a dirty tree,
  waiting 220 s and 379 s on Ignite.
- A pass is also remembered with the tree it ran in, the commit it was at and
  the change's own files there. For a dirty tree that commit is one Ignite
  writes of the files on disk, on top of `HEAD`, which no branch holds. When a
  commit lands between `ignite check` and `ignite land`, the rebase gives the
  tree a new tree ID, so the landing looks up the tree's last pass instead. It
  reuses that pass under the rule for a pass that lost the race: the change's
  own files are the same, and the commits landed since touch none of them and no
  build input. Its `express` line then says "passed for this tree 4 min ago on
  1a2b3c4d; the commits landed since touch none of its files". On 2026-11-18 a
  landing reran all four commands, taking 56 s, a minute after its
  `ignite check` had passed them in 60 s.
- A file of the change that a landed commit also edited does not count against
  an earlier pass when it matches `inert`: files no command builds or runs, or
  whose formatting is all a command checks. The landing's formatters rewrite
  such a file after the first rebase, and the rebase merged the two edits
  without a conflict, so the commands would build the same code. Nearly every
  Ignite change edits `docs/ignite.md` and `data/efficiency-changes.json`, so
  without this rule any landing that rebased over another Ignite change ran all
  four commands again (65 s and 71 s for two landings on 2026-11-19, although
  those two also shared files under `ignite/` and would have rerun anyway). A
  merge of two Markdown edits that a later lost race brings in is not formatted
  again; tip verification's lint family reports one that came out wrong.
- The two formatting commands come first, because they take a second or so and
  clippy and nextest take 90 to 140 s under load. On 2026-11-19 an Ignite-only
  fix lost a full run of 144 s to a misformatted `docs/ignite.md`, which the
  Markdown check, then last, reported only after clippy and nextest had run.
- The Markdown command checks `docs/ignite.md` and every `.md` file under
  `ignite/` (except `ignite/target`), formatted as `just fmt` formats Markdown
  (80 columns). Those are all the Markdown files under the express paths, and
  the same files the landing's Markdown rule in `[format]` rewrites, so an
  express pass means the landing's formatters change nothing. On 2026-11-19 a
  landing's `style: format` commit rewrote a file under `ignite/` that the old
  command did not check, so the tree changed and the landing ran all commands
  again (87 s). `ignite test ignite:` and `ignite check` run it too, so the
  agent sees the failure before landing.
- The commands wait while the tree's seed (the copy of another tree's build that
  a lease makes) is under way, which they see by its directory beside the tree,
  `.<tree>.seeding`. A seed swaps the target directory in once no build holds
  Cargo's lock, and cargo-nextest gives up that lock while the tests run. On
  2026-11-20 a seed swapped a landing tree's target directory in under
  cargo-nextest: the run failed and showed the end of the donor tree's express
  log, in which every test had passed. A failure's output is now read from the
  log file the command wrote, wherever it was moved.
- The nextest command uses the `express` profile in
  `ignite/.config/nextest.toml`, which kills a test after 120 s (30 s, four
  times). On 2026-11-18 one hung test held a landing for the whole 1800 s verb
  limit; now it fails in about two minutes with its name. Under a load average
  of 12 to 16 the slowest test took 15 s. The `ignite` family uses the `ignite`
  profile, which has the same limit.
- The `express` profile leaves out the tests that took over 3 s each, by name.
  On 2026-11-19, at a load average of about 20, the workspace's 1638 tests took
  257 s on one thread, and ten of them took 121 s of it. With them the test run
  took 90 to 96 s, and the slowest of them alone took 51 s. The `ignite` profile
  runs every test, so tip verification and the gate of a landing that is not
  express still run the slow ones, and a change that breaks one is caught before
  `master` moves. Later that day, at a load average of 46 to 62, the run took 26
  to 61 s, set each time by three more tests: two that drive a Cargo stand-in
  through a growing grant (20 to 58 s) and one that runs a new script, which
  takes under a second alone but waited 13 to 25 s for macOS to check it behind
  the other tests' new executables. Without them it took 18 to 19 s at a load
  average of 32 to 40.
- The nextest command passes `--test-threads=num-cpus`, which overrides the
  `NEXTEST_TEST_THREADS` above: the tests mostly wait for the processes and
  `git` commands they start, so threads beyond the express cores cost little
  CPU. On 2026-11-19, at a load average of 15 to 23 on 18 cores, the run took 56
  to 82 s on 3 threads, 14 to 17 s on 12 and 12 s on 18. More build jobs did not
  help: after a one-line daemon edit the build took about 30 s on 3 jobs or 12,
  because it waits on compiling `ignite_daemon` and then `ignite_cli`.
- `ignite_cli`'s integration tests are one test binary,
  `tests/integration/main.rs`. macOS checks a newly linked executable the first
  time it runs, about 1.5 to 2 s each and one at a time, and cargo-nextest runs
  every test binary once to list its tests. On 2026-11-19 a one-line daemon edit
  relinked 11 test binaries, and their first runs took about 20 s of the test
  command before any test started.
- The commands are what the `ignite` family (every test, under cargo-nextest)
  and the `ignite-lint` family (`cargo fmt` and clippy) run. Those families
  still run in the gate when a change to Ignite lands together with other files,
  and in tip verification.
- The `[express]` section that `ignite check`, `ignite test` and `ignite land`
  use is the one in the `ignite.toml` of the tree they run in (for a landing,
  the ticket tree after the rebase, which is the commit that lands). The
  daemon's own section, loaded from its release, is the fallback when the tree
  has no `ignite.toml`, it does not parse or it has no `[express]`. On
  2026-11-18 a tree started before a commit that changed the nextest command and
  added the file that command reads failed every `ignite check` with
  `profile 'express' not found`, because the daemon's newer commands ran in the
  older tree. The pass memory's digest covers the section that ran.
- `ignite test` and `ignite check` use the express lane too, for a tree whose
  every changed file matches `paths`. `ignite test` does so when every name it
  is given is in one of `families` (`ignite test ignite: ignite-lint:`, or the
  bare family name), or is a bare word with no `family:` prefix, such as a test
  name (the reply then says the whole command list ran), and `ignite check` does
  so for any such tree, because its landing would take the express lane. They
  run `commands` in the agent's own tree as it is, with no checkout, on the same
  cores, and reply with the `express` line or with the failed command and the
  end of its output. The whole command list runs whatever names were given, and
  no result goes into the check cache. On 2026-11-18 `ignite test` of these
  checks took 5 to 8.5 minutes through the scheduler while Beryl builds held its
  cores: one test build got 4 cores for 319 s and its run 1 core for 180 s,
  against 65 s in all when it got 12 cores.
- `ignite run` of a command in the commands' directory of a build tree still
  asks the scheduler, and its reply ends with a hint naming
  `ignite test ignite: ignite-lint:`.
- `ignite check` and the gate also use `paths` for scope. When every changed
  file matches, a family with no kept listing (a fresh tree has none) is judged
  by its declared inputs instead of being listed, so a change to Ignite alone
  selects the `ignite` and `ignite-lint` families and the always-set families,
  and builds nothing for the game.
- A repository with no `[express]` or no release ref sends every landing through
  the gate.
- The integration ref's lease and the push are the same as for any landing.

### Formatting at landing

On 2026-11-18 workers on one epic lost about 25 minutes to formatting and lint
failures that Ignite found one round trip at a time: two `ignite test` runs
failed `cargo fmt --check` after 512 and 759 s, a landing failed the Markdown
format check on this document, and one worker hit clippy's 100-line function
limit four times in a row at 54 to 555 s a round. A formatter fixes in a second
what those checks only report, so `ignite land` runs the formatters itself.

- `[format] rules` in `ignite.toml` lists them. Each rule has `paths` (patterns
  as in a family's `inputs`), an optional `exclude`, an optional `dir` to run in
  (the checkout's root when empty) and a `command`, split on whitespace and run
  without a shell. The word `{files}` in a command becomes the changed files the
  rule matches that still exist, relative to `dir`.
- After the first pass's rebase, the landing runs each rule that matches a file
  it changes, in the ticket tree. If they changed any tracked file, it commits
  the result on the ticket's branch as `style: format`, and the gate or the
  express commands run on that commit. Later passes rebase the format commit
  with the rest of the branch and do not run the formatters again.
- `ignite test` and `ignite check` on the express lane run the same rules on the
  tree's changed files before the express commands, and leave what they rewrote
  in the tree, uncommitted. Their `format` line names the files. On 2026-11-19
  two Ignite fixes each lost one or two round trips to those verbs failing on
  `cargo fmt --check` or the Markdown check.
- The reply's `format` line says how many formatters ran and names the commit,
  or says there was nothing to change. A landing that fails after formatting
  still shows the line, because the commit stays on the tree's branch.
- A formatter that fails (missing from the machine, or unable to parse a file)
  becomes a `warning` line and the landing goes on: the checks then report what
  is wrong, as they would have without formatting.
- Lint cannot fix itself. When an agent has changed Ignite's Rust code, it runs
  `cargo clippy -p <crate> --all-targets -- -D warnings` in `ignite/` in its
  tree before `ignite test` or `ignite land`. A failed express clippy command's
  reply ends with that command for the crate that failed.

## Tip verification

Tip verification runs the regression checks the gate left out once per tip.

Since a change lands by the verdict it earned on its own base (see
[Landing](#landing)), tip verification is also where two changes that each
passed alone and fail together are found. A round first runs the build checks
and moves `green`, the ref new tickets start from, to a tip that compiles. When
a build check fails or a required non-build check is still blocking `master`
past the stall threshold, the round can revert the landing that caused it (see
[A broken build is reverted](#a-broken-build-is-reverted)). Other failures are
fixed forward through [regression tickets](#regression-tickets). `master` moves
once the budgeted checks pass.

How a daemon verifies a tip:

- It learns of a new tip from the hub's change feed.

- It lists every regression check whose inputs changed since the last verified
  commit and that has no cached result for the tip, among the families this
  machine can run.

- It scores those checks as the gate scores them, over the areas the landings
  since `master` touched, with no threshold. Portable checks and checks that
  must pass on this platform share the tip budget, `[tip] seconds` of running
  checks at the `tip` class's rate:

  - **Before `master` moves:** the best-scored checks within the budget, plus,
    whatever the budget, cached failures being run again, checks with an open
    regression ticket, always checks and checks with no hub record.
  - **After `master` moves:** the rest.

  Builds are outside the budget, and a tip round builds what it needs, cold
  builds included.

- The machine keeps `tip_build_cores` cores for the round's work before `master`
  moves: physical cores / 5, clamped to 2 to 6, unless the machine's table in
  `ignite.toml` sets `tip_build_cores`. That work is the round's listing, its
  build checks, the checks within the budget, the forced checks, and the builds
  all of them need. The build checks take the kept cores first, and the rest of
  what they ask for ahead of every class. The other work takes whatever kept
  cores no build check holds and starts on them at once, then takes the rest of
  what it asks for in the `tip` class, below agents. Agents borrow the kept
  cores only while no tip work wants them. On 2026-11-25 cygnus kept 2 cores for
  build checks only, and its other tip commands waited up to 1,500 s for cores
  behind agent work, so its rounds took 12 to 150 minutes.

- With `repository.green` set, the build checks (the checks of families declared
  `builds = true`) are outside the budget and run first, before any other check
  of the round, on the cores the machine sets aside for them. When every build
  check of the tip has a pass, run or cached, and none waits, the round pushes
  the tip to `green` and goes on with its other checks; whatever they find,
  `green` stays. A refused push is fine when `green` already holds the tip. A
  failing build check is blamed like any failure and ends the round as failed
  without running its other checks. The failed tip leaves `green` where it was.
  With `green` unset a round has no build step. The main checkout fast-forwards
  its local `green` with `staging` and `master`, and `ignite status` shows
  beside the tip round which commit `green` is at and how many commits it is
  behind the tip.

- It claims those checks from the hub in batches, so two machines do not run the
  same ones, then runs them in the `tip` class. Each family is listed once per
  round and its batches run from that listing. Several families run at once,
  each in its own landing tree, leaving one landing tree free for landings. See
  [How long a round takes](#how-long-a-round-takes) for how large a batch is.

- Before it leases a tree for a family, the round asks the hub
  (`POST /tips/unclaimed`) which of the family's checks still need a run: no
  other machine holds their batch's claim or has stored their result. When none
  do, it skips the family without listing or building it, and the round's record
  counts those checks as left to other machines (`elsewhere` in its budget), not
  as unrun. Nothing is stored for them, so a later round runs them if the claims
  lapse. A hub that cannot answer leaves the family to be built as before.

- A build that fails keeps its family's checks from running. The hub hears of it
  once, as `<family>:build`, not once for each check. A check whose build failed
  is never held off: holding off is for checks that ran and hung or went
  unreported. Its build is reported once per tip, and the check runs again in
  the next round. A nextest run whose own build fails reports its checks as
  waiting on that build, not as unreported.

- A tip left waiting (for incomplete checks, or checks another machine holds) is
  verified again after ten minutes, or when a newer tip arrives, fleet settings
  change, or its platform has deferred checks at the tip that just reached
  `master`.

- A round that has started runs to its end. When a newer tip arrives meanwhile,
  the round is not restarted: its results are cached by input, and the next
  round verifies the newest tip. Checks whose inputs did not change again are
  cache hits. Tips in between are not verified one by one.

- When every check before the move has a pass, and every check answered from the
  cache passed, the daemon moves `master` to the tip. A check switched off as
  flaky is excluded. An incomplete required check or a cached failure holds
  `master`. When a peer platform finishes its pre-promotion checks, it shares
  the exact native result keys that fell outside its own budget. Other verifiers
  still wait for its required results and listings. Fleet settings and a
  family's `defer_platforms` can defer native checks as described in
  [Tip verification and machines that are off](#tip-verification-and-machines-that-are-off).

- The same round then runs the checks past the budget, in the `tip` class, on
  whatever shared cores are free, without the kept tip cores. A failure among
  them is reported as a regression, which files its ticket; `master` has already
  moved and stays. From then on the check runs before `master` moves on every
  round, because its regression ticket is open and its cached failure is run
  again, so it holds `master` as any cached failure does, until the fix lands. A
  newer tip ends this part of the round before its next family or batch, even
  with batches under way, and the checks not yet run are in the newer tip's set
  instead. An outstanding native check also leaves a coverage gap so its
  platform can finish it when another verifier moves `master`.

- When a required check fails, the round still reports the failure. It looks
  back through up to eight landing boundaries before the earliest landing known
  to fail, or before the failed tip's landing if the range is unknown. It
  advances `master` to the newest of those landings whose required checks
  already have passing results for their exact inputs. It uses stored listings
  and verdicts, applies coverage and deferrals at that commit, and records any
  deferred native checks before pushing. If no landing qualifies, `master` stays
  where it is.

- When nothing can be scored (git or the hub cannot be read), every check runs
  before `master` moves.

- Each round runs as the internal verb `tip-round` in a verb process of its own
  (see [Processes](#processes)), so a restart of the daemon into a new release
  does not cut it off. The daemon's tip loop only waits on the hub's feed,
  decides whether the newest tip needs a round, starts it under a handle and
  waits on the handle. The round leases its landing trees through the daemon,
  owned by its handle, so a restarted daemon keeps those trees and the commands
  in them. It moves `master` itself. When its reply says `master` moved, the
  loop has the new release built. A restarted daemon whose loop finds a running
  `tip-round` handle waits for that round instead of starting another. A round
  whose verb process dies is not run again: its handle ends `incomplete` and the
  loop starts a new round if the tip still needs one.

- While a batch runs, the round claims it again every five minutes, which renews
  its claim at the hub, so a batch that runs longer than the hub's 15-minute
  claim is not taken by another machine meanwhile.

- The verifier's memory survives a restart: the feed position, the last settled
  tip, what it already reported, the checks held off, the cached failures
  already run again, each platform's wait and the tip waiting to be retried are
  kept as one row in the daemon's database. Each round loads it when it starts,
  and saves it after each report and at its end.

- Each round's record says how it split its checks: the budget, the rate used,
  and the checks before the move (within the budget and forced) and after it,
  each with their instructions, and when `master` moved. `ignite status` shows
  it with the round's minutes.

- A tip that has waited an hour for checks that only an unreachable machine can
  run files one P1 ticket naming the machine. `master` waits for those checks.

- A cached failure stands for every later tip that leaves the check's inputs
  unchanged, so `master` cannot move past a broken check. It moves again at the
  first tip where the check passes, which is the tip that holds the fix.

- A regression is reported only for a check that fails at the newest tip, or
  whose inputs have not changed since the tip where it failed. A failure that a
  later landing already repaired files nothing.

- A landing whose gate finds a check failing at the tip it was rebased onto
  reports it as a regression itself (see [Landing](#landing)). Tip verification
  should have found it first, so when that report opens or joins a regression
  ticket, the hub records why tip verification missed the check, from what it
  knows of the tip and of the tips it saw after it:

  - **Stale pass:** a round passed the check at one of those tips. The check's
    declared inputs miss something it reads, or it is not deterministic.
  - **Not run:** a round finished at one of them without running the check and
    left none of its family's checks unrun. By its declared inputs nothing
    changed since the last verified commit, so an earlier pass stood.
  - **Skipped:** every round that finished there left checks of the family
    unrun, because a newer tip ended it or because `master` did not move and the
    checks past the budget wait for a later round. Each round's record lists how
    many checks of each family it left unrun, and the tip that ended it.
  - When no round has finished at the tip or after it, tip verification has not
    reached that tip yet. That is not a miss, and nothing is recorded.

  The reason is added to the regression ticket as a comment, and the dashboard's
  tips page counts the last seven days' misses by family. When one family is
  missed three times within seven days, the hub files one `hub` ticket for it,
  naming each miss and its reason. Its signature is the family, so a later miss
  adds a comment to the open ticket instead of filing another.

Because results are cached by input, a check that only the sweeps ran every
three hours now runs exactly when something it reads changes. Every family's
listing is kept as [Declaring a family](#declaring-a-family) describes, so a
round whose families' inputs did not change lists and builds nothing.

### How long a round takes

A round costs the checks whose inputs changed, and close to nothing per check
beyond them. The first rounds after the switch broke that rule. A kernel change
reran 17,700 Beryl 3D interface checks in about an hour, although the program
that runs 9,153 of them finishes in 11 to 13 seconds. The hour was Ignite's own
overhead: batches of 50 checks, each a new launch of that program, run one after
another, with the family listed again for every batch. These rules, with the
single listing and the families run at once above, prevent it:

- **A batch is sized by cost, not by count.** A claim holds about one minute of
  measured work (`claim_seconds` under `[tip]` in `ignite.toml`), so a family of
  9,000 checks that cost milliseconds each is one or two commands. A check
  nobody has measured counts as its family's median. A family nobody has
  measured starts at 50 checks a claim and doubles with each claim.
- **A covering check stands in for a matching check.** `covered_by = "<family>"`
  says that the named family's checks find every fault the covered family's
  checks find. Tip verification drops a covered check when a check of the same
  name and platform reads at least the same inputs. It does this before scoring,
  so only the covering check's history affects the tip budget. The landing gate
  scores both families independently.
- **A round reports where its time went.** Its record holds the minutes spent
  running checks, listing, building and waiting for cores, each summed over the
  families that ran at once, and the minutes during which a family was being
  listed and nothing else of the round ran. When more than 10% of a round's wall
  time goes to that lone listing and Ignite's own work between commands, the
  round files an improvement ticket. Listing beside a build or another family's
  checks delays nothing, so it never counts. Waiting for cores is shown but
  never files a ticket: the round's commands run in the `tip` class, which
  yields to landings and agents by design. For a week after a person closes that
  ticket, slow rounds file nothing: round times vary, and a ticket closed after
  finding nothing to cut would otherwise come back within the hour.

How these work:

- A claim's cost is measured in CPU seconds. A claim holds about `claim_seconds`
  less the family's start-up of recorded cost, each check counted at the median
  of its recent costs with the start-up taken out.
- The hub fixes the costs and `claim_seconds` for each tip and family the first
  time any machine asks (`POST /tips/costs`). Each machine cuts batches over
  every check of the family it lists at the tip, including checks switched off
  as flaky and checks another family covers, so a batch number names the same
  checks on every machine even when the hub switches a check off partway through
  a round.
- A covered check is left out only when its covering check reads at least its
  inputs.
- A round's record (`POST /tips/rounds`) counts any command with a results file
  as running checks, other commands as listing, `queued_s` as waiting for cores,
  and time inside builds as building. `ignite status` and the dashboard's tips
  page show the last rounds. Only rounds of two minutes or more can file a
  ticket, and an open ticket is reused without a comment.

Expected: a round after a landing that touches one crate or one area finishes in
under 5 minutes. A round after a change to the kernel crate or the lock file
costs the build, the recordings and each large family run as a few commands.

### Finding the culprit

A failed check names its own suspects, because Ignite knows what the check reads
and what each commit changed:

- The candidates are the commits since the last verified commit that changed any
  of the check's inputs.
- One candidate is the culprit, with nothing run.
- With several candidates, Ignite runs that one check at commits between them,
  by bisection, for at most 10 runs. A search that ends early reports the range.
- The unit of blame is one landing, which is one ticket's commits. A commit that
  no landing recorded, such as a push by hand, is a landing of its own.
- The regression ticket says how the search ended: the only candidate with
  nothing run, found in N runs, ended early with the reason, or not run with the
  reason.

### A broken build is reverted

A landing type-checks on the tree it was gated in, not on the exact tip, so two
landings that each compile alone can leave a tip that does not compile. While it
stays that way `green` cannot move, tip rounds build nothing, and later landings
pile onto code nobody can compile. With `[revert] broken_build = true`, a tip
round can remove such a landing. This applies to checks of a family declared
`builds = true` when `repository.green` is set.

Non-build failures normally get regression tickets. A prolonged stall can leave
`master` behind a series of fix-forward landings, each awaiting another round.
With `[revert] stall_minutes = 120`, a required non-build failure also qualifies
for a revert once the oldest landing between `master` and the tip has waited at
least 120 minutes, measured from the committer date of the last commit in that
landing. A settled tip with a required failure gets one more round when the
stall threshold is reached. Checks past the tip budget, and native checks
catching up on coverage from before `master`, still get regression tickets. A
platform catching up from `master` itself has no promoted landing to blame, so
its failures qualify like any other. So does a failed check that has a pass at
`master` on its platform, however old the gap: the gap did not leave that check
unverified. The stall setting is optional and independent of `broken_build`.

Before that revert, upkeep switches off a check that has held `master` back for
an hour without a record of catching a real defect. A check holds `master` back
when a tip seen after the newest verified tip failed it, and its hour starts
when the first such tip finished verification. Its record is the hub's
`check_fix_outcomes`: a check with any `product` outcome is kept, except in the
`[rules] budgets` families, such as instruction counts, where a fix that brings
the cost back under its limit is performance work, not a defect caught.
Protected checks, lints and formatters among them, are never switched off this
way, and neither is a check on a platform no machine can verify. The switch is
recorded against the check's open regression ticket, which gets a comment
telling its worker to switch the check back on with the fix. Deferring such a
check costs nothing while its fix is written, where a stall revert would take a
product landing out of `master`.

An eligible failure reverts its culprit when all of these hold, and otherwise
gets a regression ticket:

- the search ended at one landing: the only candidate, or found by bisection;
- the check fails again when the round runs it at the tip once more, ignoring
  cached results, so a fault of one run reopens no ticket;
- the landing changed none of Ignite's own source, whose releases the landing
  itself judges and moves;
- the landing is not made only of a round's own revert commits (subjects
  starting `revert: `, with no landing recorded);
- the round has reverted fewer than 3 landings;
- one commit on the tip undoes all the landing's commits without a conflict
  (`Git::revert`), and the check passes at that commit, run there or answered by
  the result cache;
- the tree at that commit still builds: each build check (a check of a family
  declared `builds = true`) that this machine can run, that passed at the tip,
  and whose inputs the revert changes passes at the revert commit too.

The last condition exists because a test suite can pass at a revert that does
not compile. Once a commit that added a function was reverted for a failing
pytest check while a later commit already called the function, and `staging`
stopped compiling until someone reverted the caller by hand. So when a build
check fails at the revert, or the revert conflicts, the round looks for the
later landings up to the tip that depend on the culprit, undoes them in the same
commit and builds again, once. A later landing depends on the culprit when a
line it added uses a word the culprit introduced (a word of three or more
letters, digits and underscores in the culprit's added lines that no text file
held before the culprit, which `Git::words_before` finds with `git grep`), or
when it removed a line of 12 or more characters that the culprit added
(`Git::line_changes` reads both). A later landing that changed Ignite's own
source, or is a round's own revert, is never undone. If the revert still does
not build, nothing is pushed, and the round reports the failing check as a
regression, even one reported before, with a note before its output that names
the culprit, the later landings undone with it and the build checks that failed.

The revert commit's subject is `revert: <the landing's first subject>`, cut to
60 characters, and its body names the check and the commits it undoes, with the
later landings undone with the culprit listed apart. The round takes the hub's
lease on `staging` as a landing does, waiting while a landing holds it, checks
that the tip has not moved, and pushes. When the tip moved, it builds the revert
again on the new tip, three times at most, then reports a regression instead.
When the new tip changed what the check reads, the round runs the check there
first. Where it no longer fails, because a fix or another machine's revert
landed, nothing is undone and the next round runs the check at that tip. Where
it still fails, the revert is built on the new tip, because a check that reads
most of the tree has its inputs changed by nearly every landing. A check is left
to the next round this way only once until `master` moves: on a busy day every
round's tip is overtaken before it ends, so a check that fails again is reported
as a regression whatever the newer tip changed. The round then tells the hub
(`Hub::reverted`, `POST /reverted`), once for each landing the commit undid; a
later landing's report says it was undone with the culprit because it uses what
the culprit added. In one batch the hub reopens the landing's ticket, sets its
priority to 0 and its metadata `reverted` (the revert commit, the landed commits
and the failing checks), and comments with the check, its output and the line
`apply your work again with: git revert --no-edit <revert commit>`. For commits
no landing recorded it files a new P0 ticket with the same text. A second report
of the same revert commit changes nothing, and a landing of the ticket clears
`reverted`. A session already holding a ticket that depends on the reopened one
is told, as `ignite tell` tells it, to release that ticket, which is handed out
again when the reopened one lands. A reopened CLI landing is offered on the
machine and provider that ran the landing, as for a regression. The worker
resumes that transcript with the revert details. The brief of `ignite start`
prints a `reverted` line with the revert commit, the checks and the command to
apply the work again. If the hub misses the report, the verifier sends it again
at the start of the next round. Another eligible check that failed with the same
landing is covered by the same revert once it passes at that revert commit.

A round that sees a required check fail tells the hub at once
(`POST /tips/failures`), and the hub then lets a newer `staging` tip replace the
candidate, since a newer landing may hold the fix. On a busy day the round is
replaced before it reaches its verdict and its culprit search, and so is the
next. So the hub also reports a check itself once it has failed on the same
platform at two consecutive tips with failures on record, matched by name rather
than input hash. The report's range runs from the last verified tip to the
oldest tip of the streak, says that no search ran, and names the machine and
path of the failing run's output. A round that later finishes its search adds
the culprit to the same ticket.

Once a round sees a runnable required check fail before `master` moves, it stops
starting further families or batches for that tip, publishes the failed keys
locally so the tip loop does not stop it for a newer candidate while in-flight
batches finish, and proceeds directly to record the failed verdict, run the
culprit search, revert when eligible and promote an ancestor before the
culprits. Only `master` reaching the tip, or every failed check passing, stops
it. On 2026-11-25, with a landing every 10 to 20 minutes and rounds of 30
minutes or more, rounds kept running unrelated families after a non-build check
failed and were superseded before reaching their failed verdict, so no search
finished, nothing was reverted and `master` did not move. The search runs
between fixed commits, so a newer tip does not change its answer.

A newer candidate does not stop a round that is still running its own checks
before `master` moves, on any machine, unless `master` has reached the round's
tip. Once it has listed its checks, the round saves its tip and handle locally,
so the tip loop leaves it running; a round still listing gives way as before. It
finishes its checks, then gives way to the newer candidate if none of its own
checks failed. The newer round reuses its results wherever the inputs did not
change. Before, any machine's failure replaced the candidate and ended the other
machines' rounds at the next landing. On 2026-11-25 a Windows-only lint failed
at every tip from 14:21 to 20:50 UTC. Between 15:40 and 21:50, 84 rounds ran
about 5,000 checks between them, against some 24,000 in one full round.
Breakages that landed meanwhile went unseen and were found one at a time after
the lint stopped failing. The last of them was reverted at 22:14, 1 hour 40
minutes after it landed, while 99 product commits waited on `staging`.

### Regression tickets

The hub then files a **regression ticket**, or adds the check to one already
open. A landing whose gate finds a check failing at the tip it was rebased onto
searches before filing, so dispatch can use the result as soon as the ticket
opens (see [Landing](#landing)). The ticket has priority P0:

```text
TK3FC  P0 bug  regression  check:rust
Regression: beryl_bench::family::tests::family_corpora_never_change
caused-by  TW8GV.21.73 (commit 5e1b0aa "feat: ward counts mana spent")
culprit: found by running the check at earlier commits (3 runs)
first bad 5e1b0aa, last good 77c2d14
rerun: ignite test family_corpora_never_change
output (last 40 lines; whole output on jarvis at ...)
```

- It links to the ticket whose landing caused it and records that ticket in its
  metadata. When a search ends with a range, the ticket links to every landing
  in the range. A later report can set the culprit. If a worker has already
  claimed the ticket, it receives the new attribution through the same mechanism
  as `ignite tell`.
- It records the operating system when the check is not portable. For a CLI
  culprit landing, the hub pins the repair to the machine that ran it if that
  machine can run the check, and asks for the same provider. Otherwise the
  regression keeps its usual machine pin. After 30 minutes without a report from
  the pinned machine, another machine that can run the check may take it. The
  provider of the original landing is required only on the original machine (see
  [Machine and provider pins](#machine-and-provider-pins)).
- It goes first in the dispatch order. The hub reserves it for a machine under a
  one-time code. On the culprit's machine and provider, the daemon tries to
  resume the original transcript under a new Ignite session. A worker that takes
  over elsewhere starts a new transcript.
- Failing checks share as few tickets as they can. One bad landing often fails
  several checks, and when each check had a ticket and a worker of its own, most
  of those workers closed their ticket as fixed by another. A check is added to
  an open regression ticket, as a comment with its output, when that ticket
  holds a check with the same first bad commit, or when no worker holds that
  ticket yet. It gets a ticket of its own only when it comes from another
  landing and every open regression ticket is being worked. At most three are
  open (`OPEN_TICKETS` in the hub's `upkeep::regressions`); past that, it joins
  the open ticket for the same operating system, else the one with the fewest
  checks.
- A worker who finds that a grouped check belongs to another ticket moves it
  with `ignite ticket regression move <check> <ticket>`
  (`POST /regressions/move`). The target must be open. It gets the `regression`
  and `check:<family>` labels and counts as given another check, and both
  tickets get a comment. From then on the check's failures reopen the target,
  the landing gate lists it as known-failing under the target, and the old
  ticket settles once its remaining checks pass. Filing a new ticket alone does
  not move the check: the hub keeps reopening the old ticket.
  `ignite ticket close` on a regression ticket prints the checks still on it
  that have not passed.
- When every check on a regression ticket passes at the newest verified tip, the
  hub closes the ticket immediately if nobody holds it. That tip must have been
  first seen after the ticket was filed, reopened, or last given another check.
  A switched-off check counts as passing. The close reason names the tip and any
  landings known to have produced passing results.
- If a worker holds the ticket, the hub sends a message as `ignite tell` does.
  The worker can land work that is still needed or run `ignite close`. If the
  worker releases the ticket, the hub closes it in the same transaction. Passing
  regressions are skipped in dispatch and cannot trigger a repeated-release
  justiciar ticket.
- `master` stays at the last verified commit until the check passes again.
  Landings on `staging` continue.
- The fix lands like any other change and closes the ticket. Tip verification
  then runs the check on the new tip. If it still fails, the hub reopens the
  ticket with the new output.

When a regression or flaky-check ticket closes, the hub records an outcome for
each check on it. A fix is `product` if any landed path is outside
`repository.test_paths` in `ignite.toml`, `test-only` if every landed path
matches a test pattern, and `none` if no files landed. Each new close of a
reopened ticket gets its own record. `ignite why <check>` lists the check's
recorded outcomes from the last 30 days.

A test-only repair must delete the invalid test or redesign it to prevent the
same failure. The worker records the remedy on the regression ticket. When the
repair lands, upkeep switches off the registered check on every machine and
files a delete-or-redesign ticket with the failure output and the fixing commit.
Protected checks stay on and get the same ticket. When the culprit and the test
update landed from the same ticket, upkeep treats the update as part of that
work and files no remedy. Card and rules definitions and oracle scenario sources
and data are product paths.

## Flaky checks

A flaky check costs every agent that meets it, so Ignite stops running one as
soon as the evidence is in, without waiting for an agent.

- Every check that fails is run a second time at once. One that then passes, on
  the same input hash, is recorded as flaky, and the gate or verification that
  saw it passes.
- The daemon reports a command's flakes together so upkeep can tell when the
  machine caused them.
- The first flake switches the check off on every machine. The hub files a P3
  ticket to delete or redesign the check and quotes the failed run's output.
  Protected checks keep running and get the same ticket.
- One ticket covers a whole check family. While a family's flaky ticket is not
  closed, the next check of that family to be switched off is added to it as a
  comment, so one session investigates them together instead of one session per
  check. Flakes in one family often share a cause, such as a loaded machine.
- `ignite flaky on <check> -m "<why>"` switches a check back on once the ticket
  is fixed, and `ignite flaky off <check> -m "<why>"` switches one off on every
  machine by hand. Each needs the machine's write token. The hub records who
  switched the check, from which machine and why, and comments on the check's
  remedy ticket if it has one. Upkeep records the ticket against the failure
  evidence so a later pass cannot switch the check back off for that failure.
- `ignite flaky list` lists every check that is off, has flaked or was switched
  by hand, with its flaky records, its ticket and the last switch by hand.
- A run that could not complete, because a tool was missing or a process was
  killed, is `incomplete`. It is never flaky, never a regression and never a
  culprit, and it is not stored in the result cache.
- A check in the always set is never switched off. A formatter or a lint that
  flakes is almost always the machine: a file another process changed, a shared
  temporary path, a download. The hub files a P1 ticket to fix the step, also
  one per family.
- A flaky record does not count toward switching a check off when three or more
  unrelated checks flaked in the same command. That is one fault of the machine,
  such as a full disk, and it gets one ticket.
- Checks that run outside the landing gate may get few cores. Each command is
  told its class. A check that waits on a slow tool needs a hang limit that
  allows for the cores it receives.

## Exploration checks

Exploration checks find bugs that nobody wrote a test for. They run on cores
nobody else wants, and what they find is product work.

- They run only in the `idle` class.
- An exploration family declares a `next` command that produces checks nobody
  has played, such as new scenario seeds.
- Up to three P2 exploration tickets wait for approval at once. A failure joins
  the pending ticket for its check family when one exists. If three tickets are
  already pending, a new family joins the ticket with the fewest recorded
  findings. Each ticket carries a `found-by:<family>` label for every family it
  covers. The description and comments record the individual findings and replay
  steps. Approving a ticket makes room for another pending ticket and fixes the
  set of findings for its worker.
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

A ticket ID is a letter, a root of a capital letter and three capitals or
digits, and one dotted number for each level below the root: `EQ44S` is an epic
and `EQ44S.7.24` a ticket two levels under it. The letter is `E` for a root
ticket created as an epic, `M` for a meta-ticket (`MAB12`, whose copies are
numbered under it as `MAB12.3`; see [Scheduled tickets](#scheduled-tickets)),
`A` for a root ticket that Ignite filed on its own, such as a regression, a
flaky check or a rule's finding (`AR2D2`, whatever its type), `B` for a root
ticket carrying the `bug-report` label that a person or session filed from a
player's bug report (`BX9K2`; `by bugs file` adds the label), `J` for a
justiciar root, `I` for an operator-declared incident root, and `T` for any
other root ticket, such as `TR2D2`. Existing tickets keep their IDs when a new
prefix is introduced. A child takes its parent's ID, so every ticket in an
epic's tree starts with `E`. The `E` records the type at creation: changing a
ticket's type to or from epic later keeps its ID, and epics created before the
`E` existed keep their `T`. IDs are stored and printed this way.

The roots of all seven letters are one space: a new root is never one already
used under any letter, by a ticket or an alias. Lookups ignore case, accept a
bare root without its letter (`q44s.7` finds `EQ44S.7` or `TQ44S.7`, whichever
exists) and accept the spelling from before IDs were stored in capitals
(`Tq44s.7`), so old commit messages and notes still find their tickets. Roots
carried over from earlier trackers keep their dashes (`TCI-00031CED38`).

What carries over unchanged from the current service:

- Tickets with their IDs, every alias from earlier trackers, labels, priorities,
  parents, comments and history.
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

Closing a ticket closes each ancestor left with no open descendant, except an
epic with an acceptance. When that epic's last open descendant closes, the hub
files one more child, "Verify <epic>'s acceptance", open and released, at the
epic's priority and with the tier label of its most recently closed child. Its
body quotes the epic's acceptance. Its session measures the acceptance, records
the numbers on the epic, and closes the verify ticket, which closes the epic. If
the acceptance does not hold, the session first files the missing work under the
epic, and when that work closes, another verify ticket is filed. An epic with an
empty acceptance closes with its last child.

A ticket that the hub files by itself is matched against open ones by a
signature of the check, the kind of failure and its message. Host names, thread
numbers and source line numbers are left out of the signature, and the values in
a failed assertion are kept, so two different faults of one check stay two
tickets. A repeat adds a comment.

Upkeep also closes these tickets once the fault they report has cleared, so they
do not pile up for a person to check by hand, and a released one does not send a
worker after a fault that is gone. Each kind of ticket says what shows its fault
cleared:

- A graph-repair ticket for an epic: something in the epic can start, is in
  progress or waits for a person's release, or the epic is closed. For a loop of
  tickets under no epic: the tickets no longer wait on one another.
- A ticket held in progress for six hours, or for 30 minutes by a stopped or
  idle session: it is no longer held that long, or its session is working.
- A machine that has not reported: it has reported since, and no tip has waited
  an hour for verification since.
- A machine that has stopped reporting (the P0 ticket for a person): it has
  reported since, or it was set off.
- `master` has not moved: no landing has waited two hours for it.
- Checks left incomplete (a failed build, a hung command, an invalid manifest
  entry, a command that cannot run or did not report): each check the ticket
  holds back has a result stored after the last sighting.
- Slow tip rounds: the last three rounds came after the ticket and none was
  slow.
- A daemon loop or idle job: the reporting machine records a successful round
  after the latest failure report.
- A startup crash loop or failed release selftest: the reporting machine records
  three consecutive clean starts on releases newer than the failed one. The
  first two runs stopped cleanly and the third reached the daemon's startup
  health threshold.

[Regression tickets](#regression-tickets) close as soon as the newest verified
tip confirms that their checks pass. The hub marks those regressions fixed when
it closes the ticket.

The rules for the faults listed above:

- A ticket that anyone other than the actors that filed its sightings commented
  on, edited or claimed stays open until a person or worker closes it. A release
  does not count.
- A ticket in progress is never closed. Its worker gets one comment saying the
  fault cleared.
- The fault must stay clear for an hour, or a day once it has come back after an
  automatic close. A pass that sees it again restarts the hour.
- The close reason begins "Ignite closed this ticket because its fault cleared"
  and says what showed it.
- A fault that returns within a week of an automatic close reopens that ticket,
  in the status it had, so one that was never released still waits for its
  release. A fault that returns after a person closed its ticket, or later than
  a week, gets a new ticket that names the old one.

Some kinds are never closed automatically, because nothing the hub can observe
shows their fault cleared: flaky checks and test-only regressions that need a
remedy, flakes blamed on a machine and passes that replaced another machine's
failure (both intermittent), cache audits (caching is switched off), exploration
finds, justiciar requests from sessions, daemon fault reports without recovery
evidence, and improvement tickets from the rules.

```sh
ignite ticket create "Split beryl_session's replay code" -p P2 \
  --parent TIG9A --labels tier:high --deps TIG9A.1 --body-file body.txt
ignite ticket ready --limit 10 --brief
ignite ticket wait TIG9A.1 --status closed
```

A ticket's ID always extends its parent's ID by one dotted number, and a ticket
with no parent has a root ID with no dots. The hub keeps this true for every
ticket. Giving a ticket another parent with
`ignite ticket update TQ44S.113 --parent EME7A` therefore gives it a new ID, the
new parent's next child number (`EME7A.30`), and renumbers every ticket below it
the same way (`TQ44S.113.1` becomes `EME7A.30.1`). `--parent ""` detaches a
ticket and gives it a new root ID. The command prints
`moved TQ44S.113 to EME7A.30`. Each old ID becomes an alias, so a commit message
or note that names it still finds the ticket, and `ignite ticket show` lists the
old IDs on a `formerly:` line. Edges, comments, history and the records of
checks and landings move to the new ID; ticket text that mentions the old ID is
left as written. A ticket cannot move while it or a ticket below it is claimed
or held by a build tree, since its branch, tree and session title carry the ID:
release it first. An import is refused if it would break the rule, and tickets
moved before the rule existed were renumbered once by a migration.

Labels say what a worker needs to run the ticket: a `tier:` label names the
model, and `lane:<name>` a chain. A provider takes a ticket only when its
`[pool.models.<provider>]` table in `ignite.toml` has the ticket's tier as a
key. The semantic tiers `tier:high` (design work) and `tier:light` (mechanical
work) are in every provider's table, each mapped to that provider's model of
that strength. A model tier such as `tier:opus`, `tier:sol` or `tier:hydrogen`
is in only the table of the provider that runs that model, so it pins the ticket
to one model. [Machine and provider pins](#machine-and-provider-pins) constrain
where the ticket runs. `ignite ticket batch` creates or changes many tickets in
one request that succeeds or fails as a whole.

Every change to anything in Beryl has a ticket. `ignite start "<summary>"` makes
that cheap for a session that was handed a task directly: it files the ticket
with the label `adhoc`, claims it and leases a tree in one command.

The hub also serves the dashboard, the one page meant for a person. It keeps the
current ticket views and adds each machine's cores and trees, the pool with what
each worker holds and what it is running, the landing queue, the state of tip
verification, open regressions and each exploration family's finds. Its first
line shows tickets marked `needs:person`, a machine with a paused pool, the
tickets that hold `master` back while a pool is paused, an open regression that
no worker can take, and tickets pinned to an unknown machine or provider.

The dashboard is at the hub's root and a ticket's page is at `/<id>` (for
example `/TBEJE.34`). Only the API is under the project name (`/by/api`). Page
addresses under `/by/` redirect to the root.

On the private listener, a person edits a ticket from its page. The public
listener shows no edit controls and refuses every write.

- The **Allocation** field in the ticket's rail sets an epic's worker share or a
  ticket's weight in its queue. The Pool page has the same editor for the
  General pool, Flaky test investigations, Exploration findings, Efficiency
  improvements and Other upkeep tickets. A blank field uses the inherited or
  default value shown in the editor. Zero parks ordinary work. See
  [Allocating workers](#allocating-workers).
- Pencils beside the title, text sections, status and labels open editors in
  place. The rail also has controls to close or reopen work. A box under the
  comments adds a comment.
- Ticket edits use `POST /by/api/batch`. Pool allocation edits use the fleet
  settings API. Both are recorded and relayed to dispatch.
- The browser holds no token. `tailscale serve` replaces any
  `Tailscale-User-Login` header a client sends with the Tailscale user's login,
  and Funnel requests carry none, so the hub accepts a batch from a request with
  that header and the `X-Ignite-Dashboard` header that the page's script sends.
  A page on another site cannot add that header to a cross-origin request
  without a CORS preflight, which the hub never approves. The edit is recorded
  under the part of the login before the `@`, since the public listener shows
  the changelog.
- A text edit names the ticket's version. If the ticket changed after the page
  loaded, the editor stays open and says so, and a second Save replaces the
  other change.

The hub backs up its database hourly and exports it daily, as the service does
now. It keeps the newest 6 snapshots and the newest of each day for 7 days, and
removes any staging directory a restart left behind: each snapshot is a whole
copy of a database of several gigabytes, on the disk the build trees use. Those
backups sit on the hub's own disk, so every daemon also keeps a ticket mirror:
an append-only copy of every version of every ticket, in `mirror.db` beside its
own database.

- An empty mirror notes the hub's feed position, then copies every ticket
  through `GET /export`, one page of 200 a round. Afterwards it long-polls the
  feed and fetches each changed ticket with `GET /export?id=<ticket>`, so it
  follows the hub within seconds. Only tickets are mirrored; metrics, listings
  and check records can be rebuilt from the machines' own records.
- A version is stored only when the ticket's content differs from its newest
  stored version, and a stored version is never changed or removed (the
  database's triggers refuse it). A bad edit made through the hub's API reaches
  every mirror at once, and the earlier versions are what a rollback restores.
- When the hub's feed position drops below the mirror's, as when the hub is
  restored from an older snapshot, the mirror keeps everything it has and copies
  the hub again, adding versions only where they differ.
- `ignite hub restore --from-mirror --at <time>` sends the hub each ticket as
  this machine's mirror held it at `<time>`, through a forced `POST /import`
  that replaces later copies. `--actor <name>` limits the rollback to the
  tickets that actor changed after `<time>`, and `--dry-run` only says what
  would be sent. Without `--at`, the newest version of every ticket is sent,
  which seeds a new hub after the hub's machine is lost.
- `ignite doctor` shows the mirror's feed position against the hub's, and warns
  when the mirror has not caught up with the hub for over an hour.

While the hub cannot be reached:

- Sessions keep working in their trees. `ignite test` and `ignite check` use the
  machine's copy of the result cache and run whatever it does not hold.
- Records queue on each machine and are sent when the hub returns. A queued
  record the hub refuses as too large (413) holds back nothing behind it: the
  daemon sends its run again in halves, and moves one record too large on its
  own into a `.bad` file in the `outbox` directory beside the daemon's database
  and reports it in the daemon's log.
- Claims and landings exit with code 4, because both need the hub.

### Attachments

A ticket can carry images, such as the screenshot of a bug report, so that a
worker on another machine and the status dashboard can see what the reporter
saw.

- **Storage.** The hub keeps each file once, named by its SHA-256, in an
  `attachments/` directory beside `hub.db`, and an `attachments` table records
  which ticket uses it under which name. Files are PNG, JPEG, GIF or WebP, at
  most 20 MiB each. Uploading the same bytes under the same name again returns
  the existing row, so a client may retry. A second image under a name the
  ticket already uses is stored as `name-2`, `name-3`. `ignite hub import`
  leaves attachment rows as they are.
- **Budget and removal.** `[hub] attachment_budget_mb` (default 2048) limits the
  files. The hub's hourly upkeep removes the files of tickets closed more than
  90 days ago, then, while the total is over budget, the files of the
  longest-closed tickets first. It never removes an open ticket's file. A file
  that no kept row names is deleted once it is an hour old, which leaves an
  upload in progress alone. When open tickets alone fill the budget, uploads are
  refused with 507 and the hub files an `A` ticket (category `attachments-full`)
  that closes itself when space returns.
- **Serving.** Upload and download are `POST` and `GET`
  `/tickets/<id>/attachments[/<name>]` on the private listener only. The
  dashboard serves a file at `/attachments/<sha256>`, also private only. The
  public page shows a placeholder line where an image would be.
- **Commands.** `ignite ticket attach <id> <files>` uploads files and adds a
  comment that shows them (`--no-comment` skips the comment, `--name` and
  `--caption` set the name and caption of a single image). `--pasted[=N]`
  attaches the Nth most recent image pasted into this Claude Code session (the
  newest when N is left out), read from its transcript, and `--clipboard`
  attaches the macOS clipboard's image. `ignite ticket create --attach <file>`
  attaches files as it files the ticket. `ignite ticket attachment list <id>`
  lists a ticket's images and
  `ignite ticket attachment get <id> <name> [-o <path>]` downloads one and
  prints where it was saved, by default under
  `$TMPDIR/ignite-attachments/<id>/`. `ignite ticket show` and `ignite start`
  list the attachments.
- **On the dashboard.** In a ticket's body and comments,
  `![alt](attachment:<name>)` shows that ticket's own image. An external image
  URL stays a link.
- **Bug reports.** `by bugs file` attaches a report's `screenshot.png` and
  `window.png` and embeds them in the ticket's body. Reports from a Beryl 3D
  launch share an epic with allocation zero and inherit its worker pool.
  `/bugfix` releases the epic by setting its allocation to 1. A standalone
  report starts at allocation zero for review. `by bugs attach` adds images to
  tickets filed earlier that have no attachment.

### Scheduled tickets

A meta-ticket is a template for work that recurs, such as a weekly sweep of
stale tickets. It is a ticket of type `meta` with an interval in hours, and the
hub copies it into a new child ticket on that schedule.

```sh
ignite ticket create "Sweep stale tickets" --every 24h -p P2 \
  --labels tier:light --body-file sweep.txt
ignite ticket update MAB12 --every 2d
```

- `--every` takes hours (`6`, `6h`) or days (`2d`), and on create implies
  `--type meta`. A meta-ticket has no parent, and keeps its type: an ordinary
  ticket cannot become one, or the other way round, because the first letter of
  the ID says which it is.
- Copies are due on a fixed clock that starts when the meta-ticket is created:
  at once, then every interval after. Each hub upkeep pass, once a minute, files
  the copies that are due.
- At most one copy is live (not closed) at a time. A due time that passes while
  a copy is live is skipped, so the next copy is filed at the first due time
  after that copy closes.
- A copy is an ordinary task, filed `open` so the pool may be offered it at
  once. It takes the meta-ticket's title, body, acceptance, design, notes,
  labels, priority and cores at the moment it is filed, so an edit to the
  meta-ticket changes every later copy and none already filed.
- The notes are how one copy leaves something for the next. A copy starts with
  the notes the meta-ticket held when the copy was filed, and before it closes
  it replaces them with `ignite ticket update <meta-ticket> --notes-file <file>`
  (the meta-ticket is the copy's parent). The body says what every copy does;
  the notes say what earlier copies found, so they are rewritten to stay short
  and are never a log that grows with each copy.
- The meta-ticket itself is never ready or offered to a worker, and closing its
  last live copy does not close it. Deferring it pauses the schedule, reopening
  it resumes the schedule, and closing it retires the schedule.
- On the dashboard, open meta-tickets have a "Scheduled" section on the home
  page. A meta-ticket's page shows the interval under "Every", which a person
  edits in place like any other value, and when the next copy falls due under
  "Next copy". A copy's page links to its meta-ticket under "Schedule".

## Workers and the pool

The daemon runs each worker as a headless CLI process, one `claude -p` or
`codex exec` process per turn. A person can watch workers on the dashboard or
with `ignite pool watch`. To work a ticket in a desktop session, run
`ignite start <ticket>`.

### The pool

`ignite pool` provides `list`, `pause`, `resume`, `usage` and `watch`. The Pool
page shows each machine's fleet state, providers and running workers. Use
`ignite pool usage` for locally observed plan windows and fleet holds. On the
private dashboard, open a worker's session to read its transcript. See
[Fleet settings](#fleet-settings) for controls that apply across the pool.

### Headless workers

A provider declares its executable and optional arguments in
`[pool.cli.<provider>]` and the tiers it offers in `[pool.models.<provider>]`.
The daemon uses [worker room](#derived-capacity) to decide how many new workers
to request.

```toml
[pool.cli.claude]
program = "claude"

[pool.models.claude]
sonnet = "claude-sonnet-5-5"
```

The provider's name identifies its sessions, probes, and model table. It uses
ASCII letters, digits, hyphens, or underscores, with `-slot` reserved as a
suffix for slot IDs. The optional `dialect` in `[pool.cli.<provider>]` selects
the adapter that builds commands and reads CLI events. It defaults to the
provider's name. A provider with its own executable and models can set
`dialect = "codex"` to reuse the Codex adapter.

To add a dialect, add a file under `ignite/crates/ignite_core/src/cli/` that
implements `CliApp::name`, `command`, `read`, and `classify`, and register the
adapter in that module's `ADAPTERS` list. The adapter can also implement
`digest` to publish readable transcript lines. Configure the provider and its
tiers with the same `[pool.cli]` and `[pool.models]` tables.

- The daemon includes each provider whose probe passed when it asks the hub for
  work. The hub names the provider in its reservation. The daemon gives back a
  reservation if that provider went on a CLI hold or has no configured program.
  The daemon reads available memory when it computes worker room and checks for
  memory pressure again after getting a reservation.
- Each new worker gets a session ID, `cli-<app>-<8 hex>`, which every turn
  carries in `IGNITE_SESSION`. The daemon takes the lowest free worker number
  for the provider and stores the worker's ticket, model, effort, turn number
  and prompt in the local database. When the ticket ends, the daemon removes the
  worker's row. Turn history and transcripts let a lane successor, regression or
  reverted ticket continue an earlier transcript. A CLI landing keeps its
  transcript available for repairs for 24 hours. Repairs from the same landing
  can use it in sequence. If the transcript cannot be resumed, the worker starts
  fresh. `ignite pool list` and the ticket's history show the hand-off and its
  reason.
- Three loops follow the pool. `ignite-cli` reads running turns and acts on
  their ends about once a second. `ignite-cli-health` runs probe turns and
  reports CLI holds. `ignite-cli-dispatch` asks the hub for tickets every ten
  seconds. `ignite status` shows the machine's worker room and its limits.
  `ignite doctor` lists all three loops.

A turn:

- A turn is a supervised command, so it outlives the daemon like any other. Its
  `output.log` is the CLI's event stream, one JSON object per line.
- Starting is two-phase. One transaction records the turn as starting, with its
  prompt and the messages it delivers; then the supervisor starts, with the slot
  and turn number in its `spec.json`. A daemon that dies between the two finds
  no run for the turn and starts it, and one that finds the run takes it over,
  so a turn is never started twice.
- The supervisor's command, `ignite internal cli-turn`, takes
  `sessions/<session>/lock` in the Ignite home, waiting while another turn holds
  it, and then becomes the CLI program, which keeps the lock until it exits. A
  repair waits if another worker still uses its culprit's transcript. The
  supervisor records the last turn it ran in `sessions/<session>/turn` and
  refuses a turn that is not newer. If two supervisors start for the same turn,
  only one runs the program.
- At start, a daemon looks for the run whose `spec.json` names each recorded
  turn. No run means the turn never started, so it starts it. A run whose lock
  is held is running, one with `result.json` has ended, and one with a free lock
  and no result was interrupted, as by a reboot: its process group is ended
  first. The daemon starts the turns that never started before its socket
  answers, and leaves the rest to `ignite-cli`. Within one daemon, a hand-off
  and the loop can both find a turn recorded as starting; looking for its run
  and starting it happen under one lock, so the second finds the first's run. A
  turn's run is marked with `output.keep`, so its log is kept for 14 days.
- The model and effort are arguments of every turn. Claude's first line names
  the model before any request is made. If it is not the one asked for, the
  daemon ends the turn, releases the ticket and stops reporting that tier to the
  hub for 10 minutes. Codex names no model, so its turns rely on the argument
  and on the refusal it prints.
- The transcript ID is read from each turn's first line and never assumed: a
  Claude turn resumed after its process was killed reported a new one.
- The daemon that is running reads the events and decides what follows a turn's
  end, so a fix to that logic reaches every worker at the next restart. Reading
  is repeatable: what a line causes is keyed by slot, turn and line number. A
  line that is not JSON, and an event of an unknown type, is skipped.

What follows a turn's end:

1. The ticket is no longer held (landed, released or closed): the slot is free.
2. The turn ended normally with the ticket held. If an `ignite` verb of the
   session still runs, the daemon resumes the worker when the verb ends. If
   messages wait, it resumes with them. Otherwise it resumes once with the text
   that `ignite turn-ended` blocks with, and a second such end releases the
   ticket with the worker's last message as the note. Claude's `Stop` hook runs
   in a headless turn too and gives the same block inside the process; this rule
   covers Codex and a hook that fails.
3. The turn was interrupted: it printed no result, because the process crashed,
   was killed, the machine rebooted, or the daemon ended a hung turn. The daemon
   resumes it at once and tells the worker its last tool call may not have
   finished. After 5 such ends in a row with no `ignite` command between them,
   it releases the ticket and reports to the justiciar.
4. The plan limit was reached: the ticket is released, and a 100% sample of that
   window starts the usage hold.
5. The failure is not the session's: not logged in, a refused model, a program
   that is missing or fails before its first line, or an API error. The slot is
   parked with its ticket held and the app goes on a CLI hold. During a hold no
   turn starts for that app on this machine, and the daemon runs the probe turn
   every 5 minutes; the first success resumes every parked slot. A hold posts
   one notification when it starts. It files one justiciar report only after a
   probe that ran after the hold started has also failed, so a backend that
   recovers within 5 minutes files no ticket. A single API error first retries
   after 30 seconds.
6. Anything else parks the slot and is reported. An end the daemon cannot place
   never releases a ticket and never starts a turn.

A turn is hung when its `output.log` has not grown for 30 minutes while no
`ignite` verb of the session runs and the CLI has no live child process.

Messages, usage and repair:

- `ignite tell`, stall reminders and wake-ups for a CLI session are rows in
  `cli_messages`, and the next turn's prompt is every undelivered one in order.
  A message is marked delivered in the transaction that records the turn as
  starting, and queued again if that turn printed nothing.
- `ignite tell --now`, and every order from the holder of an incident or
  justiciar ticket, ends the running turn's processes and resumes with the
  message. It waits while a `land` of the session has begun its push. An
  ordinary `tell` waits for the turn's end.
- Every `rate_limit_event` of a Claude turn is a sample of the plan's usage, so
  the daemon can report usage while workers run. When a Codex turn ends, the
  daemon reads the latest rate-limit snapshot from that turn in the Codex
  session transcript. Each turn's cost and token counts go to the hub with its
  ticket.
- Every column of the CLI tables and every field of JSON kept in them has a
  default, and the release selftest decodes every row of the copied database
  with the new build, so a release that cannot read an older build's rows is
  rejected before any daemon restarts into it.
- When the daemon has been down for 15 minutes and `justiciar-needed.json`
  exists, a machine with a configured CLI provider has its monitor start one
  headless justiciar turn in the main checkout, on the `high` tier at `high`
  effort, as session `cli-monitor-justiciar`. Its prompt is
  `ignite guide justiciar` with the crash evidence: the monitor's request, the
  newest crash record, and the last 50 lines of the daemon's log and of
  `starts.jsonl`. The turn holds `monitor/justiciar.lock` in the Ignite home and
  logs to `monitor/justiciar-<n>.log`. Later runs resume its transcript while
  the daemon still does not answer, at most 6 turns per incident and never two
  at once. The incident ends once the daemon has answered in two runs in a row.
- The daemon sends a digest of each transcript to the hub, which shows it at
  `/pool/session/<id>` with a box that sends `ignite tell`.
  `ignite pool watch <ticket or session>` follows the same digest in a terminal,
  read from the local logs (`--no-follow` prints what exists and exits). The
  digest of a turn is one line for each message the worker wrote, each tool call
  (its name and the first line of its input) and the turn's result, each cut to
  2,000 characters. The daemon queues new lines in its outbox, so a hub outage
  loses none. It forgets what it queued when it restarts and queues a running
  turn's lines again, which the hub stores once, keyed by session, turn and line
  number. `ignite pool list` shows each active worker's state and each CLI hold,
  and `ignite doctor` warns when a configured provider has no program on the
  daemon's `PATH`. Transcripts can hold secrets, so the hub serves them on the
  Tailscale listener only.

A CLI worker's `ignite turn-ended` blocks if a process under the session is
still running outside an `ignite` verb. The supervisor ends that process with
the turn. The first block names `ignite run -- <command>` and `ignite wait`. The
second releases the ticket. Titles do not apply. `ignite start` and
`ignite prime` leave out the title lines for a session whose ID starts with
`cli-`, and name the `cli-worker` guide, which says: start no background shell
commands, wait in the foreground with `ignite wait`, never ask a person, and end
the turn only after `ignite land` or `ignite release`, or to wait on an `ignite`
verb.

The `cli_e2e_*` tests in `ignite_daemon` use a script that stands in for a
provider, a fake clock, real supervisors and a real database file. They cover a
daemon restart during a turn, a program killed mid-turn, a reboot that left no
supervisor and no result, a crash between recording a turn and starting it, two
starts of one turn, a row an older build wrote, a plan-limit end, a refusal with
its CLI hold and probe, `ignite tell --now`, and the block text followed by a
release.

### What the daemon hands off, and when

A daemon asks the hub for work with its current room, provider offers and
scheduler readings. The hub considers tickets in this order:

1. A justiciar ticket for a fault in Ignite or its workflow. Like a regression,
   it skips the test of expected cores.
2. A repair blocking tip verification, then other open regression tickets. These
   go ahead of allocation-controlled work, even when their Allocation is zero.
   They still need a tree on a machine that can run the check and can bypass the
   ordinary worker room and expected-core limits.

An active machine takes every ticket of the first two kinds that it can, even
while it asks only for tickets that hold `master` back, for example because a
check fails the same way for several of its workers. A checks-only machine works
one such ticket at a time. 3. An eligible improvement ticket when improvements
are under their default share of recent hand-offs. This applies to improvements
outside the explicitly weighted Efficiency improvements pool. See
[Improvement tickets](#improvement-tickets). 4. A ready ticket from the pool
furthest below its allocation. Within that pool, the highest ticket allocation
goes first, then the longest chain of tickets waiting behind it, then the
oldest.

A machine can take a ticket when all of these hold:

- Its state permits the ticket and the hub can reserve it for an eligible
  provider. See [Fleet settings](#fleet-settings),
  [The provider mix](#the-provider-mix) and
  [Machine and provider pins](#machine-and-provider-pins).
- The daemon can lease an existing ticket tree or create one.
- For a ticket other than a regression, its expected use of cores fits beside
  the tickets already in progress there. See
  [Light and heavy tickets](#light-and-heavy-tickets).
- The machine runs the ticket's required operating system, if it has one.
- The machine can run every check family that Ignite requires for the ticket's
  named files. The Simulator family requires a Mac worker for iOS host paths.
  Shared engine and client changes still invalidate the Simulator result for
  macOS tip verification.

Not every ticket is offered:

- File new plans with epic allocation zero. The operator activates a plan by
  setting a positive weight in Allocation or running
  `ignite ticket update <epic> --allocation <weight>`. A new epic and tickets
  filed under a pending epic start in `pending_approval` by default. Setting a
  positive allocation opens a pending epic and its pending descendants. A
  descendant with its own zero stays parked until its allocation changes.
- Standalone tickets are normally filed `open`. Setting a ticket's Allocation to
  zero parks it for automatic dispatch, as does a zero on its effective pool. A
  source set to zero parks its work even if one of its tickets sits under an
  epic. Setting an allocation affects future hand-offs. A session can still
  start a parked ticket by name if it meets the claim rules.
- The hub files improvements and fault reports pending, and files work holding
  `master` back open. Each pending report belongs to an automated source, such
  as Efficiency improvements, and inherits that source's allocation: its own
  weight, or the General pool's when it has none. Upkeep opens the report while
  that allocation is positive, so setting a source to zero is how an operator
  holds its reports for review. A report that a person set back to pending stays
  pending. The hub also opens an existing report when a later tip begins waiting
  on it.
- A ticket marked `needs:person` is never offered.

### Machine and provider pins

A `machine:<name>` label pins a ticket to a machine. A `provider:<name>` label
pins it to a coding-agent CLI, such as `codex`. Each pin is inherited
independently from the nearest ancestor that sets it. An unpinned ticket can go
to any eligible worker. When a ticket has multiple labels of the same kind, the
first nonempty one applies. The ticket page shows the effective values in its
Machine and Provider rows and marks inherited pins. The dropdowns list
registered machines and known providers. Choosing None clears the ticket's own
pin, so any pin from an ancestor applies again.

Use `ignite ticket create --labels machine:<name>,provider:<name>` when filing a
pinned ticket. On an existing ticket, `ignite ticket update --add-label` and
`--remove-label` change its labels. `ignite ticket show` and `list` print pins
with the other labels. `ignite plan check` warns when an effective pin names a
machine or provider unknown to the fleet.

File platform-specific work with `ignite ticket create --needs-os macos` or
`--needs-os windows` so the requirement is in place when dispatch starts. Use
`ignite ticket update <id> --needs-os <os>` to change the requirement, or
`--needs-os any` when the work can run on either system.

- `ignite ticket create --automatic` adds a machine pin for the filing machine
  unless the ticket already has one. The hub pins the tickets it files about a
  specific machine to that machine too.
- `ignite release --needs <machine>` replaces the ticket's machine pin.
  `--needs <os>` records the required operating system, and `--needs person`
  sets the ticket aside for a person.
- When a ticket holds up `master` and its pinned machine has been silent for 30
  minutes, a ticket marked for fallback can run on another machine that meets
  its operating system requirement and any provider pin.

The choice of tier and the provider's availability are described under
[The provider mix](#the-provider-mix).

The hub reserves the ticket for that machine in the same request, so two
machines never take one ticket. An operating system requirement is matched
against the machine's registration. A ticket with no such requirement can run on
either operating system if the machine and provider are eligible. What happens
next:

- The daemon starts a CLI worker, whose `ignite start` turns the reservation
  into a claim held by that session and tree. The machine's reservation code is
  required while the reservation is live.
- The daemon records pending starts in its local database. After a restart it
  retries any unexpired start with the same reservation code. If the CLI
  supervisor already started that worker, it returns the existing session.
- A reservation that no `ignite start` has claimed in 10 minutes lapses, and the
  hub can offer the ticket again. Closing or assigning a ticket cancels any
  unused reservation.
- A session that runs a command for a ticket whose lease expired, and that
  another session has since claimed, gets exit code 3.

For an unpinned ordinary ticket, the hub also checks whether
[placement](#placement) favors another eligible machine.

### Allocating workers

Pool allocations divide concurrent workers across epics, the General pool and
separately weighted automated sources. For two epics, run
`ignite ticket update <id> --allocation 200` on the first and use
`--allocation 100` on the second.

- The hub offers the next free worker to the eligible pool with the fewest
  claims and outstanding reservations per unit of allocation, counted across the
  fleet. If these are the only two eligible pools and both have enough ready
  work, the first six worker slots split four to two. When a worker in the
  100-weight epic lands a ticket, the next free slot goes to that epic to
  restore the split.
- A positive allocation is a relative weight. If one pool runs out of ready
  work, other eligible pools can use the free slots. Use `--max-workers` for a
  hard cap on an epic's concurrent workers. The cap can leave slots idle when
  there is no other eligible work.
- A blank epic or milestone allocation lets work use the nearest ancestor with a
  worker weight. Ordinary work in an epic with no weight in its line stays
  parked. Standalone tickets use the General pool, whose default weight is 1. A
  blank source allocation lets its standalone tickets share the General pool. An
  explicit positive source weight gives them a separate pool.
- A ticket's allocation orders it within its pool without changing the pool's
  share. A blank ticket allocation uses the default queue weight of 100. Clear
  an override in the dashboard or set `allocation` to `null` in a batch update.

### Light and heavy tickets

Tickets ask very different amounts of the machine. A fix to a Python script uses
almost no CPU, and a change to a core crate rebuilds half the workspace many
times. The hub admits tickets using the machine's reported capacity and the
expected cost of each ticket, so cheap tickets can run beside expensive ones.

- A ticket's **expected cores** is a figure from history: over the tickets
  landed in the last 30 days that named files in the same areas, the median of
  core-minutes used per minute worked. An area is a workspace package or a
  second-level directory.

- A ticket that names files in several areas takes the largest figure. A ticket
  that names no files takes the machine's median.

- A planner can state the figure directly with `--cores`.

- The hub keeps a total for the tickets in progress on a machine or reserved for
  it, counting each at its expectation. It cannot see what a ticket has used so
  far.

- A ticket is admitted when that total plus its own figure is within the
  machine's `admit_cores`: every core its scheduler hands out, which is its
  cores less its express reserve. On jarvis, with 18 cores and 3 express cores,
  that is 15. It is not a setting, because the scheduler already makes every
  heavy command wait for cores; admission only keeps the expected load of the
  tickets in progress within what the machine has.

- The `pool` line in `ignite status` and the Fleet page show the latest reason a
  machine received no ticket. It may have no ready work, no tree or worker room,
  or no free cores or build slots. A ready ticket may need another provider,
  check family or machine, reach its epic's cap, or be cheaper to build
  elsewhere.

- When a `land` or `agent` request has waited two minutes for cores and the host
  is at least 90% busy, the daemon asks the hub only for tickets that hold
  `master` back. Commands can wait while the host has CPU headroom because the
  scheduler initially counts all the cores a grant holds. On 2026-11-21 jarvis
  recorded a 325-second wait at 63% CPU. If the last CPU reading is missing or
  at least five seconds old, the daemon uses the wait alone. The `pool` line
  says
  `commands have waited 150 s for cores, so only tickets that hold master back start`.

The machine reports its total [build slots](#build-trees) and how many are
available to ordinary tickets, including slots it can reclaim from free trees.
The hub deducts one available slot for each reserved or claimed build ticket
whose tree does not hold a slot yet. A build ticket waits when that count is
zero. Light tickets can still start. An older daemon that omits the counts has
no free slots until it updates. The daemon can reclaim a slot from a free tree
when a build starts.

The machine can start up to three build tickets at once when slots and cores
permit. With three or more active, the newest must have been held for 90 seconds
before another can start. Once a ticket command has waited 30 seconds for cores,
new build tickets wait until the queue has stayed below that threshold for 90
seconds. The two-minute rule still applies to all ordinary handoffs.

A regression can use a free slot during a queue hold, and a justiciar can start
to free space. A lane successor follows the same pressure checks. If it needs to
wait, the landing closes and the successor stays ready. Workers that already
hold tickets can finish. `ignite status` names the condition holding up the next
handoff.

The half-core figure also decides whether `ignite start` prebuilds the tree:
lighter tickets take an older tree.

### Chains and neighbours

Two rules replace Enact's lanes, because tickets queued behind one session
waited five times longer than the rest:

- A `lane:<name>` label is only for a true chain, where each ticket needs the
  previous one's result. The next ticket of a chain goes to the same session
  without clearing it.
- Ordinary tickets that touch the same area can end up on the same machine
  because [placement](#placement) accounts for build outputs it already has.

### Stalled workers

A worker can stop without finishing, and its ticket and tree must not stay
leased for nothing.

- Any `ignite` command from a tree renews its ticket's lease, and a command
  still running keeps it renewed however long it takes.
- After 30 minutes without a command starting or running, the daemon checks the
  CLI worker's process. The silence clock survives a restart because the daemon
  saves each held tree's last activity.
- A command stuck far past its time also counts as a stall, although it is
  running and keeps the lease renewed: a command the worker started, other than
  `wait`, that has run for 45 minutes, or whose current step has lasted 30
  minutes. The daemon checks the CLI worker again every 30 minutes while the
  same command stays stuck. An idle session gets one message that names the
  command, its step and how long each has run, and says: if the ticket can still
  be done, continue it; if it waits on something outside the ticket, file that,
  make it a dependency and release; if it cannot be completed as written, run
  `ignite justiciar` with the evidence and release. A worker with a `land`,
  `release` or `close` running is never looked at for a stuck command, because a
  landing's long waits belong to the landing queue and tip verification.
- A session that is still running is left alone. An idle one is sent one message
  asking it to continue or release.
- The daemon releases the ticket only when the app confirms that the session is
  gone, or that it is idle and has stayed idle for 15 minutes after the message.
  Releasing keeps the work, as [Releasing work](#releasing-work) describes.
- A worker that ends its turn while holding a ticket gets the instructions in
  [Headless workers](#headless-workers) without waiting for the stall check.
- The hub releases the claims of a machine that has not reported for 2 hours,
  with a comment on each ticket. When the machine reports again, a worker that
  still holds its tree for one of those tickets gets the ticket back, for the
  same session and tree, if nobody took, closed or changed it meanwhile.
  Otherwise the daemon saves the tree's work on the ticket's saved branch, ends
  the worker and says so on the ticket.
- The hub also releases a ticket that is in progress although no Ignite session
  or machine holds it, once it has not changed for 2 hours. Such a claim came
  from outside Ignite, such as a ticket imported in progress from the old
  tracker or a status set by hand, and nothing else would ever end it.

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
  The stall rule above acts at 30 minutes of silence, or when a command has run
  for 45 minutes or one of its steps for 30, even while the session sits in
  `ignite wait` on it.
- Every running command records its current step: rebasing, listing a family,
  building a product, running a number of checks, or waiting for the lease,
  cores or another build. `ignite wait` prints the step and how long it has
  lasted whenever it returns still running, and `ignite land` prints each step
  as it starts. `ignite status` lists the running commands with handle, verb,
  ticket, tree, class, age and step. It also lists what each background loop's
  round is doing or waiting for. `ignite doctor` names that, and advises a
  restart only for a round that names nothing.
- Builds run in the class of the work that asked for them and are charged to its
  ticket. A landing's builds are `land` work and tip verification's are `tip`
  work. The free tree the idle loop keeps built is `idle` work.

Stopping a command:

- `ignite stop <handle>` ends a command the session started. Its processes are
  ended, a wait for cores or for another build is withdrawn (the build it waited
  on runs on), and its cores, landing tree and lease are given back. The handle
  ends `stopped` with exit code 3. A verb process that agreed to stop and is
  still running after 60 seconds never looks at its stop where it is, as in a
  loop on the CPU. It is ended with its whole process group, as `--force` ends
  one, and the stop waits another 60 seconds for the daemon to see it finish.
- A landing whose push has begun refuses, because a cut-short push may or may
  not have reached the remote.
- A verb process that was asked to stop by any other path, such as
  `ignite close` or `ignite release` of its tree, and is still running 60
  seconds later is ended by a loop of the daemon (`ignite-overdue-stops`, every
  10 seconds). It reads the stops the daemon keeps in its local database, writes
  the handle as `stopped`, and ends the verb process with its process group, as
  `ignite stop` does. A handle that refused the stop, such as a landing whose
  push has begun, is left alone. This also ends a verb process an older release
  started.
- `ignite stop <handle> --force` is for an emergency. Only the operator (a
  request from a terminal, which names no session) or a session holding an
  `in_progress` ticket labelled `incident` or `justiciar` may give it. It stops
  a command of any session, or one with no session such as a tip round a daemon
  loop started. It asks the command to stop, and if the command refuses or has
  not finished within 10 seconds, it ends the command's verb process and the
  process's whole group with `SIGTERM`, then `SIGKILL` 5 seconds later. The
  daemon then ends the commands the process left and its handle.
- A landing tree stays leased while the verb process that holds it still holds
  its lock, even after the daemon ended its handle. When the daemon frees a
  landing tree whose command has finished, it first ends any process still
  running inside the tree.
- `ignite close` and `ignite release` first stop every command still running
  from the tree and wait for them. If one refuses or does not end, the tree
  stays leased and the command says why.
- If the ticket was already closed at the hub (for example with
  `ignite ticket close`), `ignite release` and `ignite close` still free the
  tree, after saving its work, and say "<ticket> was already closed; <tree>
  freed". The daemon's tick, about once a minute, frees the tree of any held
  ticket that is closed at the hub, so a session that never retries loses
  nothing.
- A person stops a session's command by telling the session.

Correcting a ticket so that it lasts:

- A comment on a ticket, or an edit to its text, is the durable correction. It
  stays with the ticket for whoever works it next.
- Every `ignite` command from a tree prints, as `changed` lines, the comments
  and edits made to its ticket since the session's previous command, leaving out
  the session's own. A worker therefore hears of a change on its next command,
  without anyone messaging it.

Correcting it now:

```sh
ignite tell TQ44S.9.48 "Do not wait for multivac. Land when the gate passes."
ignite tell --all "The oracle engines are down until noon. Skip oracle checks."
```

- `ignite tell <ticket>` records the text as a comment. If a CLI worker holds
  the ticket, its daemon delivers the comment to its next turn. It delivers only
  comments made during that worker's claim, since older comments are already in
  the worker's brief.
- A worker in `ignite wait` receives a notice that the wait has ended and a
  message is queued. Its next turn receives the message text. A person working
  from a desktop session can read the comment with
  `ignite ticket show <ticket> --comments`.
- `ignite tell <session>` does the same for the ticket that session holds, and
  `--all` for every ticket in progress, on every machine.
- Another session can use it too. A worker that learns something the holder of
  another ticket needs says so with `ignite tell`.

`ignite prime` carries the rule that prevents the commonest case: a worker never
waits on another machine or on tip verification. When its ticket needs something
outside itself, it finishes what it can and lands, or releases the ticket with a
note.

### The justiciar

Ignite escalates faults and impossible tickets to a justiciar. A worker holding
the `justiciar` ticket has the authority in `ignite guide justiciar` to repair
the ticket graph, release or rewrite tickets, repair the machine, and fix Ignite
itself. If Ignite blocks its own landing, the justiciar can deploy the fix
through the release ref.

- The hub files justiciar tickets at P0 with the `justiciar` and `tier:high`
  labels and a root ID beginning with `J`. It matches reports by signature so a
  repeated sighting adds a comment and never a second ticket. They come from
  upkeep, when the oldest landing since the last verified tip has waited two
  hours or a ticket has been held for six hours (30 minutes when the holding
  session's title starts with ⛔, or the daemon has reported it idle that long),
  and from `POST /justiciar`, which daemons send for a crash record or a loop
  that keeps failing, and which `ignite justiciar "<reason>"` sends for a
  session that meets an emergency.
- Upkeep checks `master` only after the oldest landing on `staging` that
  `master` does not yet hold has waited two hours. A tip round under load takes
  a few minutes, and a failing check already has a regression ticket, so a
  shorter wait filed tickets that workers closed as "no emergency". When no
  regression ticket is open, upkeep files a master-lag ticket, unless a worker
  closed one for that landing less than two hours ago: the worker either found
  nothing for Ignite to fix or landed a fix that has not yet moved `master`. A
  stall that outlasts that gets a new ticket.
- When regression tickets are open, upkeep judges each one instead of filing a
  master-lag ticket. A regression ticket whose work is progressing gets a
  comment, once per stuck landing, as `ignite tell` would write it, so the
  session working it gets the comment as a message. A regression ticket whose
  work has stalled gets its own justiciar ticket, one per regression, matched by
  signature so a repeated sighting adds a comment. A regression counts as
  stalled when the first of these holds, checked in this order:
  1. Two separate fixes have landed for the same check, and tip verification
     still failed that check after each one.
  2. The ticket has been open without a claim for 10 minutes.
  3. Its holder has recorded no activity on it for 50 minutes. Activity is a
     record, a comment or a change to the ticket by the holding session, and the
     claim itself counts.
  4. The ticket has been open longer than the 90th percentile of the lifetimes
     of the last 50 closed regression tickets that were worked by a session
     resumed from the culprit's transcript. With fewer than 20 such tickets, the
     limit is 20 hours.
- A regression ticket held by a session that resumed the culprit's transcript
  (see [Regression tickets](#regression-tickets)) is never stalled, whatever the
  four conditions say, because that session already has the failed change's
  context.
- A stalled-regression ticket clears by itself when the fault no longer holds:
  the holder resumed the culprit's transcript, or recorded activity that ends
  the inactivity, or a claim ended the unclaimed wait, or a tip that includes
  the regression has passed its checks. A passing tip clears it even while the
  regression ticket is still open. A reopened regression keeps its original
  lifetime and failed-fix count until a tip passes, and a regression that
  returns after a passing tip starts a new one.
- A report about one machine, such as its daemon's crash, is pinned to that
  machine. It can go elsewhere after the pinned machine goes silent (see
  [Machine and provider pins](#machine-and-provider-pins)).
- Justiciar tickets are offered before every other ticket, whether or not an
  epic is active. So are all the other tickets the hub files by itself, which
  carry the label `hub`.
- The hub may reserve a ticket that holds `master` back at zero worker room
  while the machine is free of memory pressure.
- When the hub cannot be reached for 15 minutes, the daemon starts a local CLI
  justiciar without a ticket. The monitor can also start a local CLI justiciar
  when the daemon is down. See [Headless workers](#headless-workers).
- A worker also sends for a justiciar for one ticket it cannot complete as
  written, not only for a fleet-wide emergency. Hard work goes back to its
  worker, but an impossible ticket is the justiciar's: it relaxes the
  self-imposed constraint (rewrites the acceptance, removes an impossible edge
  or narrows the scope), records the old rule, the new rule and why on the
  ticket, and releases it to the pool.
- When relaxing a constraint cannot resolve it, because the work needs
  credentials, access or a decision only the person has, the justiciar does two
  things. It files a ticket for the person with `--needs person`, stating the
  exact need and what was tried. And it rewrites the blocked work to continue in
  the interim with fake data, a stub, a fixture or a local substitute, filing
  the step that swaps in the real thing as a ticket that depends on the person's
  ticket. Nothing in the pool waits on the person. The justiciar then closes its
  ticket, and titles itself ⛔ only when no interim path exists.
- Each mitigation recorded on the justiciar ticket has exactly one outcome. Link
  a filed root-cause ticket with evidence of why the normal handler failed, or
  write one sentence explaining why nothing needs fixing.

### The incident commander

The operator invokes `/ic` to appoint one session as incident commander for one
emergency. That session files or claims a root ticket labelled `incident` with
an ID beginning with `I`, records the operator's declaration, and declares the
fleet-wide pause. The commander follows `ignite guide incident` to stop the
harm, restore service, resume the fleet and verify the repair. The commander or
the operator ends the emergency. Each mitigation on the incident ticket has the
same root-cause requirement as a justiciar mitigation.

When `master` stalls while `green` advances, the commander uses
`ignite emergency declare <ticket> --landings-only`. The target is to move
`master` within 30 minutes of declaring the incident. If slow checks on a
secondary platform still hold promotion at that point,
[defer that platform](#tip-verification-and-machines-that-are-off) and record
the checks to rerun. If a known culprit still blocks promotion, revert it. Once
`master` advances, record the recovery and close the incident ticket. Reserve a
full pause for a broken build, hub or daemon fault, or resource exhaustion.

### The emergency pause

- `ignite emergency declare <ticket>`, run by the holder of an incident or
  justiciar ticket, sets an emergency flag at the hub naming the ticket and its
  holder. Each daemon hears the flag in its next heartbeat's reply (within 10
  seconds) and keeps it in its local database, so a restart or a hub outage does
  not lift it.
- A full pause refuses new heavy verbs (`start`, `test`, `check`, `land`, `run`,
  `rebuild` and `audit`) and stops worker hand-offs. With `--landings-only`,
  only `land` refuses and hand-offs continue. Refused verbs exit with code 6 and
  name the emergency. Read-only verbs keep working. To end a command that is
  already running, use `ignite stop --force`. Use `--landings-only` once the hub
  and daemons support it. Older daemons enforce the landings-only scope as a
  full pause.
- The session that declared the emergency, any session working the emergency
  ticket, and tickets allowed with `ignite emergency allow <ticket> [<verb>]`
  can still run those verbs.
- `ignite wait emergency` returns when the flag clears and prints the all-clear.
- The flag lapses two hours after it was declared unless the holder renews it. A
  renewal sets the scope for the next two hours. `ignite status` and
  `ignite doctor` show the scope, age and declaring session.
- A commander or justiciar sends orders when the fault needs action beyond the
  pause, such as stopping a running command that is causing harm. For CLI
  workers, `ignite tell` delivers the message as
  `Message about <recipient ticket>:` and signs the comment `from <session>`.
  The order names the emergency ticket and the sender's session. Before acting,
  use `ignite ticket show <emergency ticket>` to confirm the `incident` or
  `justiciar` label and that the sender holds it.
- `ignite pool resume --all` ends the pause on every machine. The hub refuses
  when the emergency was declared on a ticket the caller does not hold, because
  another session may have declared a newer one. The caller then adds
  `--emergency <ticket>` to name it on purpose. An operator stuck behind a flag
  nobody can end can lift it on one machine with
  `ignite emergency clear --local`, which follows the same rule.
- When a declaration replaces another session's emergency, the earlier
  declarer's ticket gets a comment and an `ignite tell`.

### Planning

The `/ignite` skill is how a person starts work. It replaces `/enact` and
`/enact-plan`.

- Given a ticket or a small task, the session does it itself: `ignite start`,
  the work, `ignite land`.
- Given a plan or a large task, the session settles the design, files the epic
  and its tickets in one `ignite ticket batch`, runs `ignite plan check`, fixes
  what it reports, and stops. The daemon runs the pool's CLI workers. Use
  [machine and provider pins](#machine-and-provider-pins) only for hard
  requirements. The [Fleet page](#fleet-settings) and `ignite fleet` show the
  machines and provider mix.
- Given `sweep`, the session triages the open tickets that nothing is working.
  For each it decides whether the work is done, obsolete, still wanted or in
  need of a person, with a line of evidence. It shows the table, closes the
  first two kinds and activates what is left.

`ignite plan check` reports the shape of an epic, since dependency chains are
56% of lead time. It also reports estimated effort left and lists work tickets
that need an estimate.

```text
$ ignite plan check TIG9A
tickets        41, ready at the start 17
longest chain  6: TIG9A.1 > .4 > .9 > .15 > .22 > .30
agent effort   31h estimated left · 6h unfiled
suspect edge   .12 depends on .7, and they name no file in common
no files       .19, .27 name no files to read
missing file   .21 names tools/old.py, which does not exist
lane           lane:replay holds .31 and .33, which do not depend on each other
```

A ticket names a file in backquotes, or on a `Files:` line, where the backquotes
are optional. A short path such as `beryl_scene/src/lib.rs` stands for the one
tracked file whose path ends in it. A named file that does not exist is listed,
since it is either a typo or a file the ticket creates.

A blocking dependency outside the outermost epic fails `ignite plan check` with
exit code 2. A prerequisite must share the epic so the pool schedules it with
that plan. The hub rejects ticket creation, dependency edits, and moves that put
a blocking dependency outside the epic. It also refuses to release a pending
epic with an existing external blocker. Use a `related` link for outside
context. Other plan check findings remain advisory.

`ignite ticket dep add <id> <on> --external` (`"external": true` on a batch's
`dep_add`) makes the hub accept a blocking dependency outside the epic. Use it
only after the user approves that edge in chat. The epic's worker allocation
does not reach the prerequisite, so the epic stalls if nobody works it; this is
why the rule exists. The approval is stored with the edge, so later moves and
releases do not reject it, `ignite plan check` does not report it, and
`ticket show` prints it as `(blocks, external)`. Upkeep still files a ticket for
an epic whose open tickets are all blocked and none can move, as it does for any
stuck epic. A model trial's block tickets use it to wait on the source and copy
of each pair they file, which live outside the trial's epic.

Set `--estimate-minutes` when creating or updating a work ticket. Use
`--unfiled-minutes` for effort expected to become follow-up tickets. These
figures are agent effort. An in-progress ticket counts at its full estimate
until it closes. When the follow-ups are filed, replace the reserve with their
individual estimates in the same batch. A batch uses `estimate_minutes` and
`unfiled_minutes` in the ticket fields and `null` to clear either. A CLI update
uses `0` to clear either flag.

The dashboard divides the estimated effort on closed tickets by the whole
estimated scope, including unfiled work. Hover or focus the percentage for the
breakdown. Unsized tickets use the repository's average saved estimate, or 30
minutes before anyone has sized a ticket. A percentage that depends on those
defaults is approximate. The ticket's Work estimate field in the details panel
shows the saved estimate or the current default. An editor can enter their own
estimate in minutes or clear the field to use the default again. Plan check
lists tickets that still need their own estimates.

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
families = ["rust", "doctests", "clippy", "rust-format", "ignite", "source"]
tip_build_cores = 6
```

`families` lists the check families the machine can run, here the Rust tests and
doctests, clippy, Rust formatting, Ignite's own tests and the source rules (the
full list is in `ignite.toml`). `tip_build_cores` sets how many cores the
machine keeps for tip rounds (see [Tip verification](#tip-verification)) in
place of the count derived from its physical cores. A machine with no such list
can run every family supported on its operating system. A machine with no
configuration table uses its native operating system and the same default family
selection.

`tip_families`, when a machine has it, narrows what the machine does in tip
rounds to the families it names; the landing gate still runs every family in
`families`. A family left out is never listed or built there in a tip round: its
checks are keyed from the listing another machine kept at the hub, and other
machines' results pass it. Cygnus names only the families that must pass on
Windows, so that its rounds leave the portable families to the faster Macs.

What is shared, and what is not:

- **Tickets, the result cache and records are shared** through the hub.
- **Build output is not shared.** Incremental builds belong to one path on one
  machine, and Windows output is useless to a Mac. The fleet grows by placing
  whole sessions, each with a build tree, on machines with cores.
- **A machine is known by an ID** the daemon creates and stores, not by its
  hostname, so renaming a machine does not split its history.

How the machines divide verification:

- A check with `platforms = "any"` needs one pass anywhere. Cygnus can verify
  engine logic and run the XMage and Forge oracle checks for the fleet.
- A check with a platform list needs a pass on each platform that lists it. The
  `counts` family checks rollout instructions and present costs in Lima on macOS
  and in WSL on Windows. The `ignite-platform` family covers Ignite code that
  behaves differently across operating systems. Both systems verify native
  desktop startup and Android builds. The Macs verify iOS Simulator builds.
- A daemon prefers the checks only it can run, then claims portable ones. Fleet
  policy controls which machines can take tickets and verify tips. See
  [Fleet settings](#fleet-settings).

On Windows the daemon uses a named pipe for the client (a client that
disconnects ends its `ignite run`), job objects to end a tree's processes, and
CPU seconds for cost. The login service and the monitor are scheduled tasks
(`ignite.daemon` and `ignite.monitor`), and notifications are toasts posted
through PowerShell. The machine's name is `COMPUTERNAME` in lower case, its
memory and free disk space come from the system rather than `sysctl` and `df`,
and the user's home is `USERPROFILE` where `HOME` is unset, as it is outside Git
Bash. ReFS reports no per-file count of shared blocks, so on a block-cloning
volume each file counts as shared under an ID made from its name, length and
modification time, which a seeded copy keeps until a build replaces it. Windows
refuses a command line over 32,767 characters, so there a family's run whose
check names or test filter would pass that is split into several commands, one
after another, and their reports are joined.

A machine lists a family it does not run only to key the checks other machines
run. It keeps that listing, or its failure, for the family's current inputs, so
a family it cannot build is not tried again on every tip round.

### Qualifying Windows checks

Cygnus runs the native desktop, Android, engine, client, Python, and oracle
families. The iOS Simulator build remains on macOS. The Windows audit also
excludes the retired browser families `ui-test`, `ui-smoke`, `shots`, `motion`,
`ui-counts`, and `b3d-web-drift`. The browser-free `ui-format`, `ui-types`,
`ui-unit`, `b3d-timeline`, `b3d-timeline-drift`, and `b3d-web-match` families
were retired with the web client. The native replay checks cover the former
every-tick family as well.

Use the account running the daemon for these setup commands. Install the pinned
Windows Rust toolchain, cargo-nextest, `uv`, and `just`. Put Linux `uv` on the
default WSL distribution's path so Pyre can use the locked Linux environment.
Run `just oracle-setup` for XMage and Forge. Provision the pinned Android SDK
and NDK with `just mobile-setup` after installing JDK 21 and the Android
command-line tools. Run `just bench-count` and `just client vm` to provision the
WSL counting guests. Keep `oracle` and `bench-vm` in Cygnus's resource
declaration. Use the daemon's registered verifier ID for both counting profiles.
See
[calibrating instruction counts](benchmarking.md#calibrating-instruction-counts).

Add any new Windows family to Cygnus's `families` and declare the resources it
needs. The `test_ignite_inputs.py` pytest checks enrollment against all current
declarations. To qualify a revision, use a clean Windows lease and record its
commit and verifier ID. List the applicable finite checks with their native
adapters. Run them with `ignite test --rerun --json` and explicit `family:*`
selectors. Compare the listed check names with the switch-offs in
`ignite flaky list`. Generate and run a fixed sample from each exploration
family's `next` command. Keep the native listings and results with the ticket
evidence. A shared cache hit, switch-off, partial result, skip or nonpassing
verdict leaves that check unqualified.

`uv run --project tools python -m beryl_tools.ignite.windows_parity` compares
the evidence with the current manifest. Supply the JSON file, `--revision`, and
`--verifier`. The file's `listings` object maps each family to its independent
check names. Its `results` array contains one row per check with `check`,
`outcome`, `cached`, `machine`, `os`, and `revision` from the observed run. Use
`cached: false` only for locally executed checks. Keep run output out of Git.

## Dynamic scaling

Each daemon measures what its machine has available. It uses those measurements
to schedule
[cores, memory and shared resources](#cores-memory-and-shared-resources) and
size its [build trees](#build-trees). The hub uses the reports, fleet policy and
[ticket pins](#machine-and-provider-pins) to choose which machines and providers
take work. The daemon starts a [headless worker](#headless-workers) for each
reservation.

The [fleet](#the-fleet) shares tickets, check results and plan usage through the
hub. Build outputs stay on the machine that produced them. `ignite.toml`
declares each provider's executable, arguments and models, plus machines'
operating systems, check families and fixed resources. These definitions change
through a reviewed landing. Operators edit policy in Fleet.

### Fleet settings

The dashboard's Fleet page at `/fleet` shows registered machines and their
state, tree root, tree space and ceiling, recent work, build slots, and
capacity. Capacity includes worker count and room for more, admitted and free
cores, cores busy outside Ignite, memory, free disk space, the largest measured
build tree, and the longest wait for cores. The page shows the latest reason the
hub gave when it could offer no ticket. Reports older than 60 seconds are marked
stale with their sample age. The provider section shows enabled shares as
percentages, current worker counts, recent usage and reset times, holds and
eligible alternatives, and machine CLI probe results when reported.

On the private dashboard through Tailscale, the operator, incident commander or
justiciar can edit policy. The public dashboard shows the saved settings.
`ignite fleet` prints the fleet view, including the last 20 settings changes.
Its edit commands are:

- `ignite fleet machine <name> --state active|checks-only|off` changes the
  machine's state.
- `ignite fleet machine <name> --trees-root <path> --tree-space-gb <n|auto> --tree-ceiling-gb <n>`
  edits the tree settings. The root is an absolute path. Pass
  `--trees-root default` to use `repository.trees`. The sizes are decimal GB and
  the ceiling defaults to 60 GB. Use `auto` for the size of a dedicated volume,
  after the daemon reports it. Use a number when the volume is shared. The tree
  space divided by the ceiling gives the number of build slots, rounded down to
  a minimum of one.
- `ignite fleet provider <name> [--on|--off] [--share <n>] [--stop-percent <n>] [--five-hour-stop-percent <n>]`
  edits provider policy. A share is a weight from 0 to 1,000. A share of zero
  keeps the provider out of the mix: it takes only tickets whose `provider:`
  label names it, which routes chosen tickets (a model trial's arm, say) to a
  provider without giving it other work. `--off` refuses every ticket. The hub
  holds new work for a provider when one of its plan windows reaches that
  window's stop percentage, from 1 to 100. `--stop-percent` sets it for the
  weekly windows and `--five-hour-stop-percent` for the 5-hour window, which
  uses the weekly percentage until it is set. Reaching the 5-hour limit costs a
  few hours at most and a weekly one can cost days, so the 5-hour percentage
  only needs to leave room for the work already running when the hold starts.

An **active** machine takes tickets and verifies tips. A **checks only** machine
verifies tips and can take one regression or justiciar ticket at a time when no
active machine heard from in the last 30 minutes matches its operating system
requirement, pins and provider eligibility. The hub checks probe state, tier and
model holds from fresh reports, or checks advertised provider names and holds
when it has no fresh report. A compatible active machine takes precedence even
while busy. An **off** machine starts no new tickets, tip checks or idle jobs.
Running workers and commands can finish. Its daemon still sends heartbeats and
updates itself.

`ignite pool pause` and `ignite pool resume` set the local machine to checks
only and active, respectively. The same commands with `--all` stop or resume
hand-offs across the fleet independently of machine state. Global resume also
clears an emergency pause.

The hub stores versioned settings and an audit log. Each change records the
actor, time, summary and resulting policy. A new database seeds the machines and
providers named in `ignite.toml` with the standard defaults: active machines
with the default tree settings, and providers with the switch on, zero share and
a 99% stop percentage. Zero share means a provider takes no unpinned work until
an operator gives it a share, so a provider added to `ignite.toml` never starts
workers by itself unless a ticket's `provider:` label names it. Missing entries
use the same defaults. Saved settings persist across upgrades.

`GET /fleet` under the project API returns the view also available through
`ignite fleet --json`. `PUT /fleet` requires a machine token with an actor or an
authenticated private dashboard request with the `X-Ignite-Dashboard` header.
The request carries the settings and the version the operator read. On HTTP 409,
the CLI reapplies its edit to the returned version and retries once. The
dashboard reloads with a notice instead. Each heartbeat reply carries the
settings version, the machine's state and cap, the platforms whose registered
machines are all off, and the provider holds. The daemon saves the notice across
restarts and hub outages.

### Derived capacity

The daemon derives these values at startup. `P` is the number of physical cores
and `L` the number of logical cores. Round to the nearest integer with halves
rounding up.

| Value                             | Formula                   | jarvis (18/18) | multivac (10/10) | cygnus (12/20) |
| --------------------------------- | ------------------------- | -------------- | ---------------- | -------------- |
| Express reserve and landing trees | round(P / 6), from 2 to 4 | 3              | 2                | 2              |
| Cores reserved for tip builds     | round(P / 5), from 2 to 6 | 4              | 2                | 2              |
| `tip` class floor                 | 2                         | 2              | 2                | 2              |
| `batch` class floor               | round(P / 3), at least 2  | 6              | 3                | 4              |
| Ordinary worker ceiling           | 2 × L                     | 36             | 20               | 40             |

A shared resource configured with `per_core` multiplies the logical core count
by that value and rounds down, with a minimum of one. Ticket trees are created
on demand, up to the worker ceiling plus four. The daemon keeps the two most
recently used free trees and can retire the rest after 24 hours free. See
[Build trees](#build-trees) for build slots and reclamation.

For ordinary tickets, worker room is the smallest of three limits:

- Three starts per request cycle.
- The worker ceiling less running workers and pending starts.
- The number of whole worker footprints that fit in current free memory after
  reserving 1,024 MB, less pending starts. If the machine cannot report
  available memory, the other limits apply. A footprint is the median peak of
  the last 20 measured worker turns on the machine, with a 2,000 MB default.

Worker room is zero under memory pressure. The daemon uses that condition when
free memory falls below 1,024 MB or swap grows by at least 256 MB within a
minute. A regression or justiciar ticket can bypass ordinary worker room while
memory pressure is clear. The daemon checks again before starting a reservation
and gives it back if pressure has developed. The hub also checks expected CPU
use, disk growth and queue pressure. See
[Light and heavy tickets](#light-and-heavy-tickets).

The `capacity` line in `ignite status` shows the core reserves and floors,
landing tree count, total and free build slots, and worker room and ceiling. The
`fleet` line shows the machine's last reported state. `ignite doctor` shows the
build slots and the tree ceiling and reports when the tree volume has too little
free space to start a build.

### The provider mix

A provider is a configured coding-agent CLI such as `claude` or `codex`. Each
provider offers the tiers in its `[pool.models.<provider>]` table. A ticket
without a tier uses the provider's `light` model, or its `sonnet` model in a
table without `light`, or the model used for its health probe.

The hub chooses a provider for each reservation. It considers providers that are
on with a positive share, have passed their CLI probe and are available on the
requesting machine, offer the ticket's model tier, and satisfy the ticket's
provider pin and current usage holds. It counts live reservations and claimed
tickets across the fleet.

1. Among eligible providers, choose the fewest tickets per unit of share. Break
   ties by larger share, then provider name.
2. If none are eligible, skip the ticket on this machine and report why.

A share edit applies to the next reservation. A reservation and its claim keep
the chosen provider, and a lane successor stays with the session that finished
the previous ticket if its provider remains eligible. Older daemons that offer
particular idle workers can still receive reservations when their providers are
on with a positive share and clear of applicable usage holds.

### Usage and failover

Every machine uses one shared plan per provider. The hub keeps the newest sample
of each provider's plan windows. A window at or above the provider's stop
percentage holds new work until the window resets or a newer sample falls below
the threshold. A model-family window holds only that family's models. A turn
that ends at the plan limit records 100% usage for its provider. The daemon uses
a future reset from the turn or the last stored sample for that window. If
neither supplies one, it sets the reset one hour ahead. For providers that
report no utilization, a plan-limit end is the source of usage holds.

Other eligible providers divide the work while a provider is held. A provider
with zero share never takes unpinned work, even when every other provider is
held. When the hold ends, the provider returns to the mix at its configured
share.

### Check families in dispatch

The hub reads check manifests from the latest locally fetched staging commit. It
refreshes the dispatch declarations without restarting when manifest changes
land. Until staging has been fetched, it uses the locally fetched verified
branch. A read or parse failure leaves the previous declarations in use.

For ordinary work, a file named in the ticket body or acceptance makes a
regression check family required when it matches a path in the family's
`inputs`, `package_inputs` or `digest_inputs`. Cargo dependency patterns, build
product inputs and paths discovered while running checks are resolved by the
landing gate. A regression ticket requires the families of its recorded failing
checks. A ticket's explicit `needs_os` still constrains dispatch independently
and limits path-based requirements to families that run on that system.

For each required family, the machine must support one of its declared platforms
and include the family in its latest reported list. An empty list means every
family supported on that machine's operating system. The hub applies the same
eligibility when comparing placement candidates, offering a lane successor and
deciding whether an active machine takes precedence over a checks-only machine
for a master blocker.

### Placement

Daemons pull work when they can start workers. Alongside their regular
heartbeats, they report capacity once a minute. The report includes scheduler
and memory state, disk space and budget, the build outputs each free or leased
ticket tree holds, and provider probes, tiers and models. Placement uses peer
reports at most 60 seconds old. The Fleet API uses the same window. When
available, the hub uses current scheduler and build information from the
requesting machine's request. The hub keeps machine snapshots for 14 days.

The hub estimates how soon each eligible active machine can build the ticket.
The estimate is the wait for cores plus 1,000 CPU-seconds per missing output,
divided by the machine's free cores with a minimum divisor of one for the build
cost. An output is present if a free or leased tree can donate it. A ticket with
no build cost pays only the queue wait. The cold-build estimate comes from
measurements of 947 to 1,390 CPU-seconds per output, compared with about 6
seconds after copying a donor. Peers need worker room after their pending
reservations, compatible operating systems and check families, and an eligible
provider.

The requesting machine can take the ticket when no peer can take it, or when the
best peer saves at most 30 seconds. For equal estimates, the machine with the
larger fraction of admitted cores free wins, then the first machine by name.
When placement defers a ticket, the hub tries the next one in order. After 60
seconds from the first deferral, any eligible machine can take it. The wait
clears when the ticket is reserved, claimed or leaves the open queue.
Regressions, justiciar tickets, tickets with a machine pin, and lane
continuations to the same session skip the comparison.

### Tip verification and machines that are off

A tip round defers outstanding checks that must pass separately on a platform
when each of its registered machines is off or has not been heard from for 30
minutes, or when it has an online checks-only machine and no online active
machine. A machine counts as online for this policy if the hub heard from it in
the last 30 minutes. A machine whose daemon or storage hangs stops sending
heartbeats without anyone turning it off, so after 30 minutes `master` stops
waiting for it, and its platform returns with its next heartbeat. Upkeep also
files a P0 ticket labelled `needs:person`, which the dashboard shows as an
attention card, for each machine that is not off and has been silent for 30
minutes. A machine silent for more than a day is taken to be retired and gets no
ticket. An online active machine on the same platform keeps its native checks in
the promotion gate. Checks-only machines can still run portable checks before
promotion.

A check family can defer just its results on named platforms, even while those
platforms have active verifiers. For example, with
`platforms = ["macos", "windows"]` and `defer_platforms = ["windows"]`, the
family participates in the macOS gate and verifies Windows after promotion.
Every entry in `defer_platforms` must appear in the family's `platforms` list.

After the other required checks pass, the hub records the gap before `master`
moves. `ignite status` and the dashboard's first line name the first tip with
deferred checks, even if more tips advance before the platform catches up.

An available verifier catches up at the promoted tip, including when `master`
already points to it. A failure files a regression whose culprit search starts
at the last commit before the first deferral, unless the check has a pass at
`master` on that platform, read through `master`'s kept listing: then the search
starts at `master`. A gap closes only when one round passes every check of the
platform, so it can stay open for a day while the platform verifies every tip,
and without this rule each failure then blamed hundreds of landings and could
never be reverted. Once the platform verifies the tip, the warning clears. The
hub skips filing wait tickets for off machines and closes existing tickets about
waiting for them.

## Records and metrics

Ignite improves the workflow from what it records, so the record of every
command is part of the design, not an afterthought.

The supervisor writes one **record** per command, and the daemon sends records
to the hub as they are made:

```json
{"machine": "m-7f3a", "ticket": "TQ44S.9.48", "session": "local_c8470108",
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
  store covers every machine with no manual export. The ingest and export run in
  the `batch` class, because the scheduler stops `idle` work whenever other work
  waits. The export goes in parts of about 4 MB, each with its own header,
  because a machine's first export holds its whole history (426 MB on jarvis),
  more than the hub reads in one request.
- Each round also runs the dashboard's `spend` command and posts what it writes
  under the source `spend`: one line per session window that worked a ticket,
  holding that window's time and dollar segments in order. The hub keeps the
  latest post of each window in `ticket_spend`, and a ticket's page draws them
  in its efficiency section, rolling an epic's subtree up into one band per
  ticket. A segment may name a command, which only the private listener shows.

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

Improvement tickets are how the workflow gets better without a separate role. A
rule over the records files each one with an estimate of what it would save, and
the hub guarantees them workers. A rule finds the waste that a query can name.
What takes judgement to find is the work of a [patrol](#patrols).

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
TM41Q  P2 task  improvement  rule:slow-crate
beryl_session costs 38.2 core-hours a week in rebuilds (2,100 rebuilds,
median 1,100 billion instructions). Estimated saving: 15.3 core-hours a
week if its replay and export code move to crates that fewer changes rebuild.
measure: rebuild count `session-body`, now 1,450 billion
```

Rules for filing:

- The hub evaluates the rules when new records arrive, at most once an hour. It
  files a ticket for a finding with no open ticket for the same rule and
  subject.
- Nothing is filed under a minimum saving. A finding must reach $25, 2
  agent-hours or 10 core-hours a week, in whichever unit its rule measures.
- A rule files nothing for a subject within 30 days of closing a ticket for it.
- When a rule stops producing a finding for a subject, the first evaluation
  without it starts a seven-day wait. If the finding returns, the wait starts
  over the next time it disappears. After seven days, a completed evaluation
  that still finds nothing closes the ticket and names the rule and subject in
  the close reason. Ignite leaves tickets in progress and tickets people have
  edited, commented on or claimed alone. Releasing a ticket from pending
  approval still lets Ignite close it. Only improvements that land get a
  follow-up measurement after seven days.
- A rule measures what the work costs now, never how much a number grew. Code
  that gets larger as features are added files nothing unless agents pay for it
  often enough to clear the minimum.
- A session that has measured a source of waste can file an improvement itself:
  `ignite ticket create --saving agent-hours:12 --labels improvement` files the
  ticket with the session's own estimate of the saving a week, in `dollars`,
  `agent-hours` or `core-hours`. The estimate is recorded under the rule
  `estimate`, and the ticket takes the share like any other improvement. The hub
  adds no comment seven days after it lands, because no rule found the waste and
  so none can measure it again.
- A rule can be listed as report-only. `tier-gap`, which compares model tiers,
  is report-only, because the choice of model stays with people.
- `never-caught` never names a check on the protected list in `ignite.toml`: the
  native app startup, Clippy, Pyre, source rules and oracle families. They are
  cheap, and what they guard is too basic to give up for want of a recent
  failure.

Set **Efficiency improvements** on the Pool page to give standalone improvements
their own worker share. Zero parks that source. With the field blank, standalone
improvements share the General pool and remain eligible for the default
preference based on recent hand-offs. Setting a positive source allocation gives
that pool the same concurrent-worker balancing as any other.

The default preference follows these rules.

- Improvement tickets get at least 15% of the last 20 hand-offs to each machine,
  not counting hand-offs of regression tickets or hand-offs whose reservation
  lapsed unclaimed. The hub counts them when it answers a machine's request for
  a ticket.
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

## Patrols

A patrol is a scheduled ticket (see [Scheduled tickets](#scheduled-tickets))
whose copies, called rounds, study one kind of recurring workflow problem. The
scheduled ticket defines what each round does. The parent ticket's notes carry
evidence and results to the next round.

The **velocity patrol** fixes the issue with the biggest demonstrated cost to
feature delivery. Measure the time from a worker's first claim to a feature
commit landing on `staging`. Include tickets that directly deliver a feature
even if they are filed as tasks. Time spent on releases, handoffs and retries
after the first claim counts toward the duration. Report the cohort, median and
p90 from ticket history, and investigate long-running feature claims without a
landing.

CI failures and flaky tests can delay a feature directly or consume worker time
on separate repair tickets. Both costs count when choosing what to fix. Report
feature elapsed time and worker time spent on repairs separately so that a retry
and the delay it caused are visible without being counted twice.

Each round fixes one issue and files zero tickets. If a ready, unclaimed ticket
already covers the most urgent issue, record the choice, close the round, then
claim and execute the existing ticket. Leave work in progress with its current
owner and move on to the next actionable issue. If the selected issue has no
ticket, fix it in the round's leased tree and land it through the normal Ignite
workflow.

Use session records and the landing gate's execution and wait times to find the
cause. Preserve validation coverage and check that the change still detects
defects. Use later verification, regressions, reverts and rework to assess
quality after landing. Record the evidence, the fix and its validation in the
parent notes so the next round can check whether it helped.

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
  dashboard draws the series. A measurement that fails is tried again after the
  wait every failing idle job gets, whatever the commit, and is reported after
  three failures (see
  [Cores, memory and shared resources](#cores-memory-and-shared-resources)).

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
  tickets.
- **`AGENTS.md` and the project's skills** hold the rules that are about Beryl
  and not about Ignite.
- **Ignite itself** does what a rule used to ask of an agent, wherever a program
  can.

```text
$ ignite start TK3FC
ticket   TK3FC  Regression: family_corpora_never_change
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
- `ignite check` is what a change needs before landing: `ignite land` lands the
  checked commits by their verdict. A change to Ignite's own source lands alone,
  and a change that mixes it with other files is split into two tickets.
- Ordinary difficulty is the job: failing tests, confusing code, conflicts.
  Never wait for a person, for another ticket, for another machine or for
  verification.
- Start `land`, `check`, `test` and `wait` in the background where the agent's
  tool can, and end the turn: the command's exit wakes the session. Never poll
  with `sleep` or a loop. Where the tool has no background mode, the verb runs
  in the foreground and answers within nine minutes with its result or a handle
  for `ignite wait`.
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
- `blocked`, on a refused landing and on a landing that went past a
  `known-failing` check. The verdict says whose failure it is: a check that
  passes at the tip is the change's to fix, a `known-failing` check is nobody's
  in this ticket, and a tip that could not be asked is one rerun, then a release
  with a note. Never end a turn holding the ticket.
- `review`, on `ignite start` of a review ticket or one labeled `review:inline`.
  What a review looks for, when to rewrite, what is not a finding, and that the
  reviewer fixes what it finds.
- `reverted`, on `ignite start` of a ticket whose landing a tip round reverted,
  and named by `ignite prime` for a session told that a ticket its own depends
  on was reverted. The reopened ticket starts again, applies its work with the
  `git revert --no-edit <revert commit>` its brief gives, fixes the failing
  check and lands. A dependent ticket is released and handed out again when the
  reverted one lands.
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
- Choosing checks by coverage data. Ignite uses inputs and scores.
- Quarantining a test. The project's rule is to delete or replace it.
- Light runs. A planner files the reviews a plan needs, and nothing else is
  automatic.

## The switch from Enact

The following section records the original Enact cutover plan.

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

The work is tracked in the epic `THRH4`, whose design field holds the code
contract: where each part lives and which ticket owns it.

What the switch imports:

- Every ticket, with its ID, aliases, edges, comments and history, from the
  current service's database.
- The run log and the verdicts on each check's failures, as each check's record
  of runs, costs and failures, so the gate has scores on the first day.
- The recorded per-crate compile timings, stored as compile units in the hub.

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
- The rehearsal daemons run their own headless workers.

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
- **A wall-clock budget for the gate.** Today's 60 to 90 second target runs
  fewer checks when the machine is busy, which sends more defects to `staging`
  when the machine can least afford the repair.
- **Starting a ticket on its dependency's unlanded branch.** It would save only
  the dependency's check-and-land tail, a few minutes per link, at the cost of
  rebasing work onto commits that can still change. Chains are shortened by the
  plan's shape.
- **Checking every landing on the exact tip, made cheaper.** This is what Ignite
  did until landing by verdict: carrying results over between passes, holding
  the `staging` lease only for the push, keeping one landing tree per landing.
  Each fix handled one case, and the number of passes still grows exponentially
  with the rate of landings that invalidate each other (see
  [Why a landing checks once](#why-a-landing-checks-once)).
- **A merge queue that tests each change on top of the ones ahead of it**, as
  Bors, GitHub's merge queue and Uber's SubmitQueue do. It keeps `staging`
  always passing, but makes every landing wait for at least one full run of the
  gate at the tip, and builds the tip for every landing, sometimes on a
  combination thrown away when a change ahead fails. `master` is already a
  branch that always passes, so `staging` does not need to be one.
- **Reverting the landing behind any failing check.** A revert pulls code out
  from under tickets that build on it, and a failing test stops nobody from
  working. Automatic reverts are reserved for broken builds and required
  failures during prolonged stalls (see
  [A broken build is reverted](#a-broken-build-is-reverted)).
- **More cores.** A larger machine shortens every build and leaves the race and
  the repeated builds growing as before.
- **Randomized trials of workflow settings.** Set aside by decision. Ignite
  reports comparisons and leaves settings to people.
- **Replacing Enact in stages.** Two systems would share cores and worktrees
  without seeing each other, which is the problem Ignite exists to remove.
- **Pacing usage to a plan's reset.** A forecast can be wrong on a busy day, and
  pacing would make the effective shares diverge from the ones the operator set.
  With another provider available, the pool can keep doing work when a plan
  reaches its stop percentage.
- **Pushing tickets from the hub.** The hub would need to know that a machine is
  alive and has room at the instant it assigns a ticket. A request gives it the
  machine's current readings.
- **A weighted placement score.** Time to the first current build gives a single
  estimate a person can check against measurements and the hub can use to
  explain a deferral.

## Manual QA

This section records the original Ignite rehearsal checklist for
[the switch from Enact](#the-switch-from-enact). The current gate is
`ignite check`, with check families declared in `ignite/checks/`.

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
- **The culprit's transcript.** File a regression without a culprit, then report
  the same check with the culprit identified. Expect the hub to reserve it on
  the original machine and provider, and the worker to resume the earlier
  transcript. Another repair for the same landing can resume it after that
  worker finishes. If another worker claimed the regression before the search
  finished, expect it to receive the new attribution. If the original machine is
  silent for 30 minutes, expect a worker on another machine to start fresh.
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
- **On-demand trees.** Lease every existing ticket tree below the machine's
  ceiling and run `ignite start` again. Expect a new tree. Repeat at the ceiling
  and expect the lease to wait until a tree becomes free.
- **CLI workers.** Configure two providers with passing probes and file four
  ready light tickets. With four free trees and worker room, expect
  `ignite pool list` to show the four workers. Run `ignite pool watch <ticket>`
  to see messages and tool calls. After landing each ticket, its worker drops
  off the list.
- **A CLI turn killed.** Kill a CLI worker's process mid-turn, then restart the
  daemon. Expect the turn to resume with the interruption message, the ticket
  still held, and no second turn on the transcript.
- **A CLI hold.** Make the app's program fail before its first line. Expect one
  notification and one justiciar report, no turn started for that app, a probe
  turn every 5 minutes, and every parked slot resumed after the first success.
- **A headless justiciar.** On a machine with a working provider, stop the
  daemon for 15 minutes with its kickstart failing. Expect the monitor to start
  a justiciar turn and no second one while it runs.
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
- **A parked epic.** File an epic with tickets and allocation zero. Expect no
  hand-off from it. Set `--allocation 100` and expect the next eligible ticket
  to be offered. A ticket with its own zero stays parked.
- **Checks only.** Run `ignite pool pause` on a machine with busy workers.
  Expect those workers to finish and verification to continue. Offer a
  regression with no compatible active machine and expect it to be eligible
  there. Run `ignite pool resume` and expect ordinary work to resume.
- **A worker that only looks stalled.** Leave a worker reading for 31 minutes
  with no `ignite` command. Expect the daemon to check it, find it running, and
  send nothing.
- **A hung command.** Add a check that never returns, with its hang limit set to
  one minute in the rehearsal. Expect it ended with its child processes, marked
  `incomplete` as hung, not run again, and a P1 ticket listing the processes.
- **A machine's own failure.** Make a portable check fail on multivac only, by
  breaking a tool there. Expect jarvis to run it again and pass, landings on
  jarvis not blocked, and a P1 ticket for multivac.
- **A late hand-off.** Give a hand-off's prompt to a session after its
  reservation has lapsed. Expect `ignite start` to exit with code 3 and claim
  nothing.
- **Improvement share.** With a blank Efficiency improvements allocation, file
  eligible improvement and product tickets. Expect the default preference to
  offer improvement tickets while they are under their share of recent
  hand-offs. Set the source to zero and expect its work to stay parked.
- **A batch job.** Start a tournament with `ignite run --class batch` on cygnus
  and then several landings. Expect the tournament to keep its floor of cores
  and the landings to take the rest.
- **Allocations between epics.** Give one epic allocation 200 and another 100,
  each with enough ready tickets for six workers. Expect four and two across
  claims and reservations. Land a claimed ticket in the 100-weight epic and
  expect the next hand-off to restore the split.
- **A light ticket on a busy machine.** Fill a machine with core-crate tickets
  until another build ticket would exceed expected cores, with the queue still
  below the two-minute hold. File a Python-only ticket whose expected cores fit.
  Expect it to take the free tree with the oldest build and skip the Rust
  prebuild.
- **A plan.** Give `/ignite` a plan document. Expect an epic filed in one batch
  at allocation zero and a `plan check` report. The pool starts the ready
  tickets once the operator sets a positive allocation for the epic.

### Fleet policy and placement

Use a rehearsal hub with two eligible machines and two providers with passing
probes. Use scripted plan samples and seeded capacity reports for the hold and
placement cases.

- **Fleet page.** On the private dashboard, set a machine's tree root, tree
  space and tree ceiling. Reload and expect those values in `ignite fleet` and
  changes with the editor in the audit log. On the public dashboard, expect the
  same saved values and no edit controls. Compare the total and free build
  slots, ceiling and largest measured tree on Fleet and Available. A capacity
  sample older than 60 seconds is marked stale on Fleet.
- **Share change.** Give one provider all the share and the other zero. Reserve
  a ticket that either can run, then swap the shares on Fleet. Expect the
  existing reservation to keep its provider and the next one to name the new
  provider with a positive share.
- **Failover.** Give both providers a positive share, then report usage at the
  first provider's stop percentage for an all-model window. Expect Fleet to show
  the hold and reset and the next reservation to go to the second provider.
  After the reset, expect the first provider to take the next reservation. Set
  the second provider's share to zero and repeat: expect no reservation while
  the first provider is held.
- **A machine off and on.** Use a fixture with a check that requires a pass on a
  platform served by one machine. Switch that machine off and let the heartbeat
  deliver its state. Expect no new ticket hand-offs, tip claims or idle jobs
  there. Advance the tip and expect `master` to move after the other required
  checks pass, with a warning naming the first deferred tip. Switch the machine
  to checks only and expect it to verify the current tip without a restart.
  After it passes, expect the warning to clear.
- **Placement and pins.** Give one machine a build output the next unpinned
  ticket needs and give the other a fresh cold report whose estimate is more
  than 30 seconds slower. Have the cold machine ask first. Expect a deferral
  naming the warm machine and expect the warm machine to take the ticket when it
  asks. Repeat with a fresh ticket pinned to the cold machine through the
  ticket's Machine dropdown, and expect it to take the ticket on its first
  request.

### Restarts under load

- **A restart loses no work.** Put jarvis under load: one `ignite land` whose
  gate builds `rust-tests` and runs the b3d interface family, one `ignite test`
  of a Rust family, `ignite run -- sleep 900`, a tip round and idle work. Every
  30 to 90 seconds for 20 minutes, run `ignite internal restart` from a shell
  outside every tree. It restarts the daemon into its own executable, as a
  release would, and prints how long after the request the new daemon answered.
  Then land a no-op change under `ignite/` so a real release deploys. Expect no
  "its tree is reset" or "again after the restart" lines in the daemon's log, no
  run in the window ended with exit `None` or 143 except those `ignite stop`
  ended, every handle `done`, one push of the landing, one new `results` row per
  check key, under 2 s from each start in `starts.jsonl` to the first accepted
  request, and ordinary replies from every client.
- **A rollback leaves verb processes running.** Write the running commit into
  `fault-start` in the Ignite home and restart the daemon. A build the file
  names exits with code 1 right after it records its start, and an empty file
  names every build. Expect the daemon to roll back to `bin/last-good` within
  three starts, and the verb processes started before it to finish. Delete the
  file afterwards. The rolled-back commit is recorded as rejected, so the next
  release is what moves the machine forward.

The end-to-end test `ignite_cli/tests/integration/restart.rs` runs the first
check in miniature on macOS and Windows.

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
