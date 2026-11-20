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
- **Landing** is delivering a ticket's commits to the `staging` branch. The
  **landing gate** is the set of checks that must pass first, on the commit the
  change was written on. A pass is stored as the change's **verdict**, and the
  landing rebases the commits onto the newest commit and pushes them without
  checking again.
- The **tip** is the newest commit on `staging`. **Tip verification** runs the
  checks the gate left out, once per tip. It runs the build checks first and
  moves the **`green`** branch, where new tickets start, to a tip that compiles;
  then it moves `master` forward once the budgeted checks pass. A landing that
  broke the build is **reverted**; any other failure gets a regression ticket
  and is fixed forward.
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
  every family without a kept listing for the tree's inputs is listed too. A
  name that no family could hold is refused at once with a hint on how checks
  are named, so a Rust test function name never builds every family.

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
  `repository.green` is set and the branch exists.

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
  address (`IGNITE_HUB_PUBLIC_LISTEN`), and does nothing else. So a daemon
  restart or crash loop on jarvis never takes the hub away from the other
  machines, and the hub's own restart takes well under a second.

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
- After that the daemon runs the executable built from the release branch,
  `repository.release` in `ignite.toml`, or `master` when that is unset. When
  the branch moves and Ignite's own source changed, each daemon builds the new
  executable in a tree of its own and points `bin/current` at it, so clients and
  the hub run it. The daemon restarts into it only when the release changes the
  code the daemon runs: anything under `ignite/` except the `ignite_cli` crate,
  test files (`tests.rs`, `*_tests.rs`, files under a `tests` directory),
  Markdown and the check manifests. A release that changes none of that, such as
  a fix to the client alone, is taken on without a restart: the daemon reports
  it as its version from then on and compares later releases with it. A restart
  that is due waits for a quiet window: until no newer release has appeared for
  `self_update.quiet_seconds` (5), no newer release is still building and, when
  the release changes the landing code (the daemon's `land` module), no landing
  runs, but at most `self_update.cap_seconds` (30) from the first release it
  waits with, counted from when its build started. Releases that land one after
  another therefore cause one restart into the newest: a release that lands
  while another builds is built as soon as that build ends, and replaces it as
  the one to restart into. A push in progress holds the restart even past the
  cap. It never waits for running commands or verb processes, which survive it.
  Before it restarts, the daemon answers every new request, except the calls of
  verb processes, with a reply that says it is restarting, which the client
  sends again, and waits up to 30 seconds for the verbs running on its own
  threads, such as a `release`. It does not wait for a `start` that only waits
  for a free tree: the new daemon runs that `start` again under its handle, and
  the rerun claims the tree if one was leased to the session meanwhile. Daemons
  fetch the branch every five minutes, so a fix pushed to it by hand deploys
  too. A landing on the machine moves the release branch from a worktree of the
  main checkout, which updates the checkout's copy of the branch. The sender
  loop runs a round every 10 seconds, but it reads that copy every second
  between rounds and starts a round as soon as the branch moves, a release build
  finishes, or a landing names a release candidate (below). Before 2026-11-19
  each of those waited for the next round, up to 10 s.
- An express landing builds its release before it lands. After its rebase and
  formatting, it points the local ref `refs/ignite/release-candidate` at the
  commit it is about to check. The daemon reads the ref, and when the commit
  follows its own version, changes Ignite's source and no other build runs, it
  builds and stores that commit's executable while the landing runs its express
  commands, without a selftest or a restart. An express landing pushes the
  commit it checked, so when the release branch reaches it the build is already
  stored and only the selftest is left. On 2026-11-19 the release build took 36
  s after each Ignite landing, as long as the landing's express commands. A
  landing that fails or rebases again leaves a stored build nobody restarts
  into; a build already running is never interrupted for a candidate.
- The release build is not run by a loop. When the release branch moves, or the
  hub reports a version this daemon doesn't have, the daemon starts one build on
  a thread of its own in `update/update-tree` with a one-hour hang limit; only
  one runs at a time. The build runs as the daemon's own child process on the
  machine's `express_cores`, borrowing whatever cores are free when it starts up
  to 12 in all, and logs to `update/build.log`. Before 2026-11-19 it was a
  supervised `agent` command, which waited up to 7.5 minutes for the scheduler's
  cores under load and then compiled on one core. A machine with no express
  cores still runs it that way. The sender loop never waits for it: each round
  it looks whether the build has finished and shows "building release <commit>"
  as its step meanwhile, and the next heartbeat after it finishes restarts the
  daemon into it, or takes it on without a restart as above. A failed build is
  logged once and not retried until another commit is wanted. A daemon that
  restarts mid-build waits for Cargo's lock before it resets the update tree.
- Before the daemon restarts into a new release, the release thread runs the new
  executable's selftest, `bin/<commit>/ignite internal selftest`, the same way
  as the build, on one express core (`update/selftest.log`). It parses
  `ignite.toml` and every check manifest as the daemon does at start, requires
  this machine's `[machine.<host>]` table, and applies the build's migrations to
  a copy of `daemon.db`. `bin/current` points at the build only after it passes.
  A build that fails is added to `bin/rejected`, so no daemon updates to it, and
  is reported once to the justiciar; the daemon stays on its own build until the
  release branch moves.
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
  [Tip verification](#tip-verification)), `[revert] broken_build` (default
  false; see [A broken build is reverted](#a-broken-build-is-reverted)), and
  each machine's `tip_build_cores` (default 0; see
  [Cores, memory and shared resources](#cores-memory-and-shared-resources)).
  Their defaults keep every landing checked on the exact tip.
- A setting in `ignite.toml` takes effect when the process that uses it next
  starts and reads the file from the main checkout. `ignite.toml` is not
  Ignite's source, so a change to it builds no release and restarts nothing.
  Every verb process reads it again when it starts (`Base::read`, called from
  `verbproc.rs`), so `[land]` and `[revert]` reach the next `ignite land`,
  `ignite check` and tip round once the daemon has fast-forwarded the main
  checkout to the change, which it does every 30 seconds while the checkout has
  no local edits. The daemon reads it only when it starts, and it alone uses
  `repository.green` for `ignite start` and `ignite sync`, `tip_build_cores` for
  its scheduler's limits, and the branches the main checkout fast-forwards.
  Those take effect at the daemon's next restart, which the next release that
  changes the daemon's code brings about. Nothing reloads the daemon's copy. A
  verb process and its daemon can therefore disagree for a while: a tip round
  that reads `green` from the file pushes the branch even while the daemon still
  starts tickets at the tip.
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
- A landing that changed Ignite's own source moves the release branch to the
  landed commit once `repository.release_family` (the `ignite` family) has
  passed on it, or the express commands have (see
  [Express landings](#express-landings)). A fix to Ignite therefore reaches
  every machine minutes after it lands, without waiting for tip verification of
  everything else, which an Ignite bug can itself be blocking.
- The check manifests (`repository.checks`, `ignite/checks/`) are not Ignite's
  source, although they lie under `ignite/`: no executable compiles them. A
  landing that changes only them judges no release, so the release branch stays
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
  the reply is the push to the release branch. On 2026-11-18 two landings each
  spent 80 to 135 seconds after their push building and running the `ignite`
  family, with their ticket trees still leased: one because its gate had left
  unrelated checks to tip verification, the other because its gate had not
  listed the family for lack of a built product.
- A release survives a rebase. A commit pushed to the release branch by hand is
  rebased when its ticket lands, so the release branch then holds a commit that
  the landed one does not contain. The landing moves the release branch over it
  anyway, with `--force-with-lease` against the commit it read there, when the
  landed commit holds that commit's changes to Ignite's source: some commit of
  the landed history has the same `ignite/` tree, or every commit that changed
  `ignite/` has an equivalent patch there (as `git cherry` compares them). A
  daemon that runs a commit the release branch does not contain uses the same
  test before it calls itself ahead of the release: it builds the release if the
  release holds its changes, and counts as running the release when the two
  `ignite/` trees match. On 2026-11-18 each of three hand pushes followed by a
  rebased landing left every daemon refusing new releases until someone moved
  the branch by hand and ran `ignite install`.
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
- `ignite status` has a `release` section: the release branch's tip, the build
  this daemon runs and what it is doing about the difference (waiting for its
  next check of the branch, with when that is due; building; self-testing;
  restarting, with when the quiet window ends and the latest the restart can
  wait; failed to build; or rolled back, with the reason from
  `bin/rollbacks.log`), then the hub's version and each machine's, which the hub
  reports at `GET /machines/versions`.
- When `ignite land` moves the release branch, its `next` line names
  `ignite wait deploy-<commit>`. That wait asks the daemon to fetch the release
  branch at its next round instead of at its five-minute check, for a release
  that moved on another machine. It then waits until this daemon, the hub and
  every machine the hub heard from in the last ten minutes run the commit or a
  later one (exit 0), or the commit was rolled back, failed its selftest or
  failed to build on this machine (exit 2, with the reason). A wait that ends
  first exits 5, or 4 when the hub cannot be reached. The wait keeps no state,
  so a client cut off by the daemon restarting into the release sends it again.
  A rollback on another machine shows only as that machine staying behind.

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
  prebuilt, reset or landed without a live owner are marked `resetting` before
  the socket is served and are reset by a background thread afterwards; a lease
  that needs one waits for it.
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
  agent logic and makes no hub calls.
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

Build trees keep Cargo's incremental builds usable, which the measurements above
show is worth about 25 times on a small edit to a core crate. A tree keeps its
own build directories, and no cache is shared between trees. Instead, a lease
can start from a copy of another tree's build (see the rule on seeding below).

Each machine has a fixed number of trees of two kinds:

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

```toml
# ignite.toml, per machine
[machine.jarvis]
ticket_trees = 24
landing_trees = 3
trees_disk_gb = 250
express_cores = 3   # see Express landings
```

Rules for trees:

- Ignite never builds in a directory outside this set. When every ticket tree is
  leased, `ignite start` waits and says so, and the dispatcher hands off no
  ticket.
- `ignite start` leases a free tree and checks out a branch at the tip. With
  `repository.green` set and the branch present, the start is `green` instead,
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
- A tree keeps the directories that its build products declare as their output,
  each created when a command first needs it. Today those are `rust/target`,
  `rust/beryl3d/target` and `ui/node_modules`, and none of them is named in
  Ignite's code. A tree that has only held documentation or Python tickets uses
  little disk.
- Every round, before measuring, the daemon deletes the object files that newer
  compiles of the same codegen unit replaced. rustc names each object
  `<crate>-<hash>.<codegen unit>.<invocation>.rcgu.o` with a new invocation part
  at every compile and leaves it in `deps` for the binary's debug info, so a
  test binary rebuilt in place gained a full set at each compile: they were half
  of a 113 GB tree. Each codegen unit keeps its newest object.
- To hold the machine under `trees_disk_gb`, the daemon deletes builds that a
  newer build of the same unit replaced, then the build directories of the free
  trees used longest ago, then, with no free tree left, those of the idle leased
  trees holding the most bytes of their own. A replaced build is a top-level
  unit (a test binary or program) with a newer unit of the same crate and
  settings, together with every unit only it depends on, followed through the
  dependency fingerprints that Cargo records. A unit that a kept build depends
  on is never deleted, however alike it looks to another. While the disk has
  less than 50 GB free, it also holds the trees under their current size less
  what the disk lacks, since the limit was set for the disk as it was then.
- The trees' size counts each file's blocks once. A seeded tree's files are APFS
  clones of its donor's and share their blocks, so each file counts its private
  bytes (macOS `getattrlist`, `ATTR_CMNEXT_PRIVATESIZE`) in its own tree and its
  shared bytes once for all trees with the same clone ID. Counting every tree's
  blocks in full had measured hundreds of gigabytes the disk did not hold, and
  cleared every free tree, the donors the next leases would have copied from.
  Free trees are cleared in two passes, each oldest first: first those whose
  deletion alone frees at least 1 GB, so a donor whose blocks a leased tree's
  clones share is kept, then any whose bytes no tree outside the free ones
  shares.
- A build waits to start while the disk has less than 30 GB free, reading the
  free space again every 10 seconds, until there is room or its work is stopped.
  A backup's APFS snapshot keeps every block that builds rewrite or delete
  allocated, so sweeping free trees can free almost nothing for hours, and a
  build started on a nearly full disk fails with `ENOSPC` partway through.
- A tree's recorded builds are forgotten before its build directories are
  deleted, by the disk loop or by seeding, so a daemon restart partway through a
  deletion cannot leave records of builds whose files are gone.
- The daemon's disk loop checks this every minute, in a loop of its own so that
  idle work waiting for cores cannot hold it up. It measures the trees at most
  once every 30 minutes (every 5 while under 50 GB free), and not at all while
  the disk has 150 GB free, since a measurement walks every build directory.
  Free trees are swept, and so are leased trees while nothing runs in them: no
  verb was asked from the tree, no process has its working directory or
  executable inside it, and the build manager holds back any build there until
  the sweep ends. A tree running a command keeps its build directories. With no
  free tree left to clear, idle leased trees first lose their incremental caches
  (`<target>/<profile>/incremental`), which costs only a slower next compile,
  and only then, largest first, their whole build directories. On 2026-11-20 the
  trees held 1,800 GB under a 250 GB limit with nearly every tree leased, and
  the disk filled because only free trees were swept. The daemon logs what it
  deleted and how much it freed.
- Lowering `ticket_trees` or `landing_trees` removes the trees above the new
  count: the disk loop deletes each one's worktree, build directories and row
  once it is free. A session that held one keeps working in it until it releases
  the tree, and no lease or prebuild takes it again.
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
- The hub reads its routing facts (each machine's system, the apps and
  `[pool] default_owner`) from the main checkout's `ignite.toml` at start and
  again every 30 seconds, so a change to routing takes effect within a minute of
  landing without restarting the hub.

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
  `--needs <machine>` (or `<machine>/<app>`) makes that machine the ticket's
  owner, so only its workers take it, and `--needs macos` records the operating
  system the work needs (see [Owners](#owners)). `--needs person` sets it aside
  for the person: no worker is handed it, the dashboard lists it, and the note
  must state the exact need and what was tried.
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
saved    2 commits and 3 uncommitted files as ignite/TQ44S.7.12
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
  again on the next request.
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

The daemon is the only place that decides which heavy command runs now. It
replaces the Python token pool, the browser page slots and Enact's locks.

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

- A machine's `express_cores` are never granted: they are kept for the express
  lane's commands, which run outside the scheduler (see
  [Express landings](#express-landings)).

- A machine's `tip_build_cores` (default 0) are kept for the build checks of a
  tip round, whose commands carry `Reserve::TipBuild` in their `CommandSpec`.
  These checks decide when `green` moves, and every new ticket waits on `green`.
  On 2026-11-19, 8,053 of 9,692 commands started on one core, and every command
  of the slowest landing ran on one, because a higher class does not take cores
  from the running grants of a lower one. So the scheduler leaves the reserve
  out of the cores it shares, as it does `express_cores`:

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
  - No other request is ever granted the reserve's cores. Cores of the reserve
    that no grant holds are lent to the express lane and the release build when
    they borrow free cores (see [Express landings](#express-landings)); a tip
    build that arrives during the loan still takes them.
  - The reserve's cores come back when the grant is released, also for a grant a
    restarted daemon took over, whose `spec.json` grant names the reserve.
  - The `cores` line of `ignite status` says how many cores are kept for tip
    builds and how many of them are in use. With `tip_build_cores = 0` nothing
    changes.

  The `reserve` field is left out of `spec.json` and of the host protocol when
  it is unset, so daemons of older and newer releases still read each other's
  files.

  This repository keeps 4 of jarvis's 18 cores for tip builds beside its 3
  express cores, and 2 on multivac (10 cores) and cygnus (20 threads) beside
  their 2 express cores. The daemon reads the reserve when it starts, so a
  change to it takes effect at the daemon's next restart.

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
inputs = ["ui/src/**", "ui/shots.json", "crate:beryl_cli_main"]
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
  compile or type-check: Beryl's clippy, Beryl 3D's clippy, `tsc` (`ui-types`)
  and pyre. With `repository.green` set, a tip round runs these build checks
  first and moves `green` once they pass (see
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
- Anything a `jsonl` family reads, such as a Python adapter or a browser check.
  Those families keep their declared inputs.
- A build that writes outside the product's first output directory: Ignite then
  observes nothing, and only the cache audits would notice.

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
the change edits them, or the landing fixes their regression. A tree has built a
product when the daemon built it there, or when the product's output directories
are in the tree and not empty.

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

Every other regression check gets a **score** for a given change: how likely it
is to fail on a change like this one, divided by what it costs to run.

```text
score(check, change) = P(fail | areas the change touches) / cost(check)

P     = (failures after changes to these areas + 20 x family rate)
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
- A failure counts when a later change made the check pass again, whether that
  change fixed product code or updated the check. Either way the landing needed
  another edit. Flaky failures do not count.
- The family rate is the family's overall share of runs that failed. It stands
  in for 20 runs, so a check with few runs starts from its family's record.
- A check that has never run scores as certain to fail, at its family's median
  cost, so new checks run first.
- A check whose own file the change edits always runs, outside the budget. The
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
  change edits and the regression being fixed are outside it.
- The budget is a ceiling, not a target: the threshold stops a landing from
  spending its 60 seconds on checks unlikely to find anything, so a small change
  lands in a few seconds.
- The threshold is set by replaying history: the largest threshold at which the
  gate would still have run 95% of the regression checks that caught something
  in the previous 14 days. `ignite gate replay` repeats the calculation from
  Ignite's own records in the hub's database, which it opens read-only, and also
  prints what 95% of those gates would have spent, to compare with the budget.
  Only a commit changes the values.
- A check's score changes as its record does, so checks move across the
  threshold by themselves. No ticket is filed to move one.

`ignite check --plan` prints what the gate would run and why:

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

`ignite land` is the only way commits reach `staging`. It replaces `enact push`,
the ticket's closing comment and `enact close`.

A change is checked once, on the commit it was written on, and lands by that
result. `ignite check`, or the landing itself when no check has run, runs the
gate in the ticket tree and stores a **verdict** when it passes: the record that
these files passed on this base under these gate settings. `ignite land` then
rebases the commits onto the tip in git's object database and pushes them
without building or checking anything again, however many landings reached
`staging` meanwhile. What the gate could not see, two changes that each pass
alone and fail together, is found after landing by
[Tip verification](#tip-verification): a tip that no longer compiles is repaired
by reverting the landing that broke it, and any other failure by a regression
ticket that is fixed forward. `master` still moves only to a tip whose budgeted
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
verified commits, and `green` is the branch new work starts from (see
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
Ignite on the exact commit that lands and then moves the release branch to it,
which every daemon builds and restarts into. A rebased commit that nothing
compiled could move the release branch to a build no daemon can make, and a
broken Ignite cannot run the tip round that would revert it. For the same reason
a tip round never reverts a change to Ignite, and `green` is not held back for
one.

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
6. Push the commit from the landing tree to `staging` on the git remote, and
   release the lease.
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
  regression at once (by verdict, only when the base is the tip), which opens or
  joins a regression ticket as tip verification would, naming the commits since
  the last verified one without a culprit search. If every failure fails there
  too, the gate runs again with those checks `known-failing`, for the rest of
  the landing, and the checks that already passed come from the cache. If any
  passes at the tip, the landing is refused as before, and the `base` line says
  which check passed there. A check that fails at the tip counts as known to
  fail even when its failure in the landing says something different, such as
  more findings: the adapters report no list of findings to compare.

### Express landings

On 2026-11-18 every fix to Ignite's own scheduling waited in the queue it was
fixing. Four fixes waited 10 to 30 minutes each for `ignite land`, because their
gates asked the scheduler for cores that it had granted to other work, often
because of the bug being fixed, and three were pushed to the release branch by
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

[machine.jarvis]
express_cores = 3
```

- A landing whose every changed file matches `paths` runs `commands` in order,
  in `dir`, in its own ticket tree, which the rebase has already put at the
  rebased commit, and pushes from there. It lands when every command exits 0,
  and moves the release branch at once. A failure exits with code 2 and shows
  the end of the command's output.
- The commands run as the daemon's own processes, never through a supervisor or
  the scheduler, with `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`,
  `NEXTEST_TEST_THREADS` and `IGNITE_CORES` set to the machine's
  `express_cores`. The scheduler leaves those cores out of what it hands out, so
  a saturated or broken scheduler cannot hold them. While an express run goes,
  the scheduler also sees its CPU as load outside Ignite and hands out that much
  less.
- When a run starts it also asks the scheduler, without waiting, for the cores
  that are free at that moment, as a `land` grant of up to 12 cores in all
  counting `express_cores`, and sets the four variables to the total. Cores of
  the tip reserve (`tip_build_cores`) that no tip build holds count as free. On
  2026-11-18 eight landings queued behind a lane of 3 cores while the scheduler
  had granted none of its 15 cores; a run took about 164 s on 3 cores under load
  and about 65 s on 12. The reserved cores stay the floor: when nothing is free,
  or the daemon cannot be asked, the run uses `express_cores` and does not wait.
  The extra grant is released when the run ends, and the scheduler may reclaim
  it for a higher class like any other grant. A landing runs in a verb process,
  so it borrows through the host protocol (`host-borrow-cores` and
  `host-return-cores`, version 2), and the daemon releases a grant its handle
  still holds when the handle ends. The `express` line names the cores used.
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
  machine's `express_cores`. A later run of a tree with the same tree ID and
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
  landing's `style: format` commit rewrote
  `ignite/crates/ignite_cli/src/pool_init/dispatcher.md`, which the old command
  did not check, so the tree changed and the landing had to run all commands
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
- A machine whose `express_cores` is 0, or a repository with no `[express]` or
  no release branch, sends every landing through the gate.
- The integration branch's lease and the push are the same as for any landing.

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

Tip verification is what slow CI, the browser sweep and the nightly run become:
the regression checks that the gate left out, run once per tip. Nothing in it
runs on a timer.

Since a change lands by the verdict it earned on its own base (see
[Landing](#landing)), tip verification is also where two changes that each
passed alone and fail together are found. A round first runs the build checks
and moves `green`, the branch new tickets start from, to a tip that compiles.
When a build check fails, the round reverts the one landing that broke it (see
[A broken build is reverted](#a-broken-build-is-reverted)). Every other failure
is fixed forward: the round reports a regression, the hub files a regression
ticket, and the fix lands on top (see
[Regression tickets](#regression-tickets)). `master` moves only once the
budgeted checks pass, as before.

How a daemon verifies a tip:

- It learns of a new tip from the hub's change feed.

- It lists every regression check whose inputs changed since the last verified
  commit and that has no cached result for the tip, among the families this
  machine can run.

- It scores those checks as the gate scores them, over the areas the landings
  since `master` touched, with no threshold, and splits them by the tip's
  budget, `[tip] seconds` of running checks at the `tip` class's rate:

  - **Before `master` moves:** the best-scored checks within the budget, plus,
    whatever the budget, cached failures being run again and checks with an open
    regression ticket.
  - **After `master` moves:** the rest.

  Builds are outside the budget, and a tip round builds what it needs, cold
  builds included.

- With `repository.green` set, the build checks (the checks of families declared
  `builds = true`) are outside the budget and run first, before any other check
  of the round, on the cores the machine sets aside for them
  (`tip_build_cores`). When every build check of the tip has a pass, run or
  cached, and none waits, the round pushes the tip to `green` and goes on with
  its other checks; whatever they find, `green` stays. A refused push is fine
  when `green` already holds the tip. A failing build check is blamed like any
  failure and ends the round as failed without running its other checks, and
  `green` and `master` both stay. With `green` unset a round has no build step.
  The main checkout fast-forwards its local `green` with `staging` and `master`,
  and `ignite status` shows beside the tip round which commit `green` is at and
  how many commits it is behind the tip.

- It claims those checks from the hub in batches, so two machines do not run the
  same ones, then runs them in the `tip` class. Each family is listed once per
  round and its batches run from that listing. Several families run at once,
  each in its own landing tree, leaving one landing tree free for landings. See
  [How long a round takes](#how-long-a-round-takes) for how large a batch is.

- A build that fails keeps its family's checks from running. The hub hears of it
  once, as `<family>:build`, not once for each check. A check whose build failed
  is never held off: holding off is for checks that ran and hung or went
  unreported. Its build is reported once per tip, and the check runs again in
  the next round. A nextest run whose own build fails reports its checks as
  waiting on that build, not as unreported.

- A tip left waiting (for incomplete checks, or checks another machine holds) is
  verified again after ten minutes, or at once when a newer tip arrives.

- A round that has started runs to its end. When a newer tip arrives meanwhile,
  the round is not restarted: its results are cached by input, and the next
  round verifies the newest tip. Checks whose inputs did not change again are
  cache hits. Tips in between are not verified one by one.

- When every check before the move has a pass, and every check answered from the
  cache passed, the daemon moves `master` to the tip. A check switched off as
  flaky is not part of the set. An `incomplete` check, or one with a cached
  failure, holds `master`. So do checks only another platform's machines can
  run, and families that cannot be listed: after the move no round would run
  them.

- The same round then runs the checks past the budget, in the `tip` class, on
  whatever cores are free. A failure among them is reported as a regression,
  which files its ticket; `master` has already moved and stays. From then on the
  check runs before `master` moves on every round, because its regression ticket
  is open and its cached failure is run again, so it holds `master` as any
  cached failure does, until the fix lands. A newer tip ends this part of the
  round before its next family or batch, even with batches under way, and the
  checks not yet run are in the newer tip's set instead. Otherwise the tip loop,
  which waits for the round, would verify no newer tip until every check past
  the budget had run, which can take hours.

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
- **A covered family is not run twice.** `covered_by = "<family>"` on a family
  says that the named family's checks find every fault this family's find, as
  `b3d-every-tick` does for `b3d-interface`. The gate may still choose the
  cheaper family. In tip verification a covered check counts as passed when the
  check of the same name in the covering family passed on the same inputs, and
  is run only when the covering family has no such check.
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
round removes such a landing instead of waiting tens of minutes for a fix. Any
other failing check is fixed forward through a regression ticket, as below.

Nothing but a broken build is reverted. A revert pulls code out from under
tickets that already build on it, and it is a second way for work to move
through the system. A failing test stops nobody from working: the tip still
compiles, `green` still moves, and only `master` waits for the fix. An earlier
design reverted the landing behind any failing check and was cut down to this
for those reasons.

A failing build check (a check of a family declared `builds = true`, run only
when `repository.green` is set) reverts its culprit when all of these hold, and
is otherwise reported as a regression:

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
  the result cache.

The revert commit's subject is `revert: <the landing's first subject>`, cut to
60 characters, and its body names the check and the commits it undoes. The round
takes the hub's lease on `staging` as a landing does, waiting while a landing
holds it, checks that the tip has not moved, and pushes. When the tip moved, it
builds the revert again on the new tip, three times at most, then reports a
regression instead. A new tip that changed what the check reads, such as a fix
or another machine's revert, is left to the next round, which runs the check
there first. A check is left to the next round this way only once until `master`
moves: on a busy day every round's tip is overtaken before it ends, so a check
that fails again is reported as a regression whatever the newer tip changed. The
round then tells the hub (`Hub::reverted`, `POST /reverted`). In one batch the
hub reopens the landing's ticket, sets its priority to 0 and its metadata
`reverted` (the revert commit, the landed commits and the failing checks), and
comments with the check, its output and the line
`apply your work again with: git revert --no-edit <revert commit>`. For commits
no landing recorded it files a new P0 ticket with the same text. A second report
of the same revert commit changes nothing, and a landing of the ticket clears
`reverted`. A session already holding a ticket that depends on the reopened one
is told, as `ignite tell` tells it, to release that ticket, which is handed out
again when the reopened one lands. The dispatcher offers the reopened ticket to
the idle worker whose last ticket it was ("your landing of <ticket> was
reverted: the tip did not build with it"), and the brief of `ignite start`
prints a `reverted` line with the revert commit, the checks and that command. A
report the hub does not take is kept in the verifier's memory and sent again at
the start of the next round. Another build check that failed with the same
landing is covered by the same revert once it passes at that revert commit.

### Regression tickets

The hub then files a **regression ticket**, or adds the check to one already
open. A landing whose gate finds a check failing at the tip it was rebased onto
reports it the same way, before tip verification reaches that tip (see
[Landing](#landing)). It is an ordinary P0 ticket that any worker can take:

```text
TK3FC  P0 bug  regression  check:rust
Regression: beryl_bench::family::tests::family_corpora_never_change
caused-by  TW8GV.21.73 (commit 5e1b0aa "feat: ward counts mana spent")
culprit: found by running the check at earlier commits (3 runs)
first bad 5e1b0aa, last good 77c2d14
rerun: ignite test family_corpora_never_change
output (last 40 lines; whole output on jarvis at ...)
```

- It links to the ticket whose commit caused it. When the search ended with a
  range, it links to every ticket in the range.
- It names the operating system the check failed on, and records that system
  when the check is not portable. It is owned by the machine that found it (see
  [Owners](#owners)).
- It goes first in the dispatch order, and the hub alone decides which machine
  works it: it reserves the ticket for a machine under a one-time code, as it
  does any ticket. A reservation's code may be used by any session of that
  machine, so when the worker that landed the culprit is idle there, the
  machine's daemon hands the reservation to that session, which still holds the
  context, without clearing it. A daemon never points a worker at a regression
  the hub has not reserved for its machine, since the hub refuses that start.
- Failing checks share as few tickets as they can. One bad landing often fails
  several checks, and when each check had a ticket and a worker of its own, most
  of those workers closed their ticket as fixed by another. A check is added to
  an open regression ticket, as a comment with its output, when that ticket
  holds a check with the same first bad commit, or when no worker holds that
  ticket yet. It gets a ticket of its own only when it comes from another
  landing and every open regression ticket is being worked. At most three are
  open (`OPEN_TICKETS` in the hub's `upkeep::regressions`); past that, it joins
  the open ticket for the same operating system, else the one with the fewest
  checks. The ticket is done when every check on it passes.
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
- The hub files a P3 ticket to delete the check or replace it with a
  deterministic test. The ticket quotes both failures.
- One ticket covers a whole check family. While a family's flaky ticket is not
  closed, the next check of that family to be switched off is added to it as a
  comment, so one session investigates them together instead of one session per
  check. Flakes in one family often share a cause, such as a loaded machine.
- `ignite flaky on <check> -m "<why>"` switches a check back on once the ticket
  is fixed, and `ignite flaky off <check> -m "<why>"` switches one off on every
  machine by hand. Each needs the machine's write token. The hub records who
  switched the check, from which machine and why, and comments on the check's
  flaky ticket if it has one. A check switched back on goes off again only after
  two new flaky records, because the records that switched it off already have
  their ticket.
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

A ticket ID is a letter, a root of a capital letter and three capitals or
digits, and one dotted number for each level below the root: `EQ44S` is an epic
and `EQ44S.7.24` a ticket two levels under it. The letter is `E` for a root
ticket created as an epic, `M` for a meta-ticket (`MAB12`, whose copies are
numbered under it as `MAB12.3`; see [Scheduled tickets](#scheduled-tickets)),
`A` for a root ticket that Ignite filed on its own, such as a regression, a
flaky check or a rule's finding (`AR2D2`, whatever its type), `B` for a root
ticket carrying the `bug-report` label that a person or session filed from a
player's bug report (`BX9K2`; `by bugs file` adds the label), and `T` for any
other root ticket, such as `TR2D2`. Tickets filed automatically before the `A`
existed keep their `T`. A child takes its parent's ID, so every ticket in an
epic's tree starts with `E`. The `E` records the type at creation: changing a
ticket's type to or from epic later keeps its ID, and epics created before the
`E` existed keep their `T`. IDs are stored and printed this way.

The roots of all five letters are one space: a new root is never one already
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
- `master` has not moved: no landing has waited two hours for it.
- Checks left incomplete (a failed build, a hung command, an invalid manifest
  entry, a command that cannot run or did not report): each check the ticket
  holds back has a result stored after the last sighting.
- Slow tip rounds: the last three rounds came after the ticket and none was
  slow.
- A regression: a tip first seen after the ticket was filed, or last reopened,
  passed verification. The regression is marked fixed too.

The rules that apply to every kind:

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
shows their fault cleared: flaky checks (the check is switched off), flakes
blamed on a machine and passes that replaced another machine's failure (both
intermittent), cache audits (caching is switched off), exploration finds,
justiciar reports from a daemon or a session, and improvement tickets from the
rules.

```sh
ignite ticket create "Split beryl_session's replay code" -p P2 \
  --parent TIG9A --labels tier:opus --deps TIG9A.1 --body-file body.txt
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

Labels carry what the dispatcher needs to know about a ticket: `tier:opus`,
`tier:sonnet` or `tier:light` names the model it needs, and `lane:<name>` a
chain. Which machine and app work it is the ticket's owner, a field of its own
(see [Owners](#owners)). `ignite ticket batch` creates or changes many tickets
in one request that succeeds or fails as a whole.

Every change to anything in Beryl has a ticket. `ignite start "<summary>"` makes
that cheap for a session that was handed a task directly: it files the ticket
with the label `adhoc`, claims it and leases a tree in one command.

The hub also serves the dashboard, the one page meant for a person. It keeps the
current ticket views and adds each machine's cores and trees, the pool with what
each worker holds and what it is running, the landing queue, the state of tip
verification, open regressions and each exploration family's finds. Its first
line says what needs a person: tickets marked `needs:person`, a machine with no
dispatcher or with a paused pool, the tickets that hold `master` back while a
pool is paused, an open regression that no worker can take, and a ticket owned
by a machine or app that `ignite.toml` does not declare.

The dashboard is at the hub's root and a ticket's page is at `/T<id>` (for
example `/TBEJE.34`). Only the API is under the project name (`/by/api`). Page
addresses under `/by/` redirect to the root.

On the private listener, a person edits a ticket from its page. The public
listener shows no edit controls and refuses every write.

- Pencils beside the title, each text section and the rail's status, priority,
  labels and, for an epic, workers and max workers open an editor in place.
  Buttons at the top of the rail release a ticket that is pending approval,
  close an open one with a reason, or reopen a closed one. A box under the
  comments adds a comment.
- Each edit is one `POST /by/api/batch`, so it is recorded and relayed like any
  other ticket write. A released ticket reaches dispatch through the change feed
  within about a second when a worker is idle.
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
now. Those backups sit on the hub's own disk, so every daemon also keeps a
ticket mirror: an append-only copy of every version of every ticket, in
`mirror.db` beside its own database.

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

### Scheduled tickets

A meta-ticket is a template for work that recurs, such as a weekly sweep of
stale tickets. It is a ticket of type `meta` with an interval in hours, and the
hub copies it into a new child ticket on that schedule.

```sh
ignite ticket create "Sweep stale tickets" --every 24h -p P2 \
  --labels tier:sonnet --body-file sweep.txt
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
  once. It takes the meta-ticket's title, body, acceptance, design, labels,
  priority, owner and cores at the moment it is filed, so an edit to the
  meta-ticket changes every later copy and none already filed.
- The meta-ticket itself is never ready or offered to a worker, and closing its
  last live copy does not close it. Deferring it pauses the schedule, reopening
  it resumes the schedule, and closing it retires the schedule.
- On the dashboard, open meta-tickets have a "Scheduled" section on the home
  page. A meta-ticket's page shows the interval under "Every", which a person
  edits in place like any other value, and when the next copy falls due under
  "Next copy". A copy's page links to its meta-ticket under "Schedule".

## Workers, the pool and the dispatcher

Workers are sessions in the Claude and Codex desktop apps, so a person can see
every one in the app's sidebar, open it, and steer it from another device with
the app's Remote Control feature. Ignite makes handing them work cheap, and each
machine's daemon holds work back while that machine is full.

### The pool

Each machine has one standing pool that serves every active epic.

- `ignite pool init`, run from the main checkout, writes the dispatcher's
  instructions into `~/ignite`. In Claude, a person opens a session there and
  sends `/ignite pool 8`. That session registers itself as the machine's
  dispatcher, starts the workers in the main checkout (at most ten per message)
  and runs the dispatch loop. The desktop app lets only a turn a person started
  start sessions, and its tools for clearing and prompting a session are only
  known to work on sessions the caller started, so the session that starts the
  workers must be the dispatcher.
- Registering a dispatcher replaces any earlier dispatcher of the same app on
  that machine, and removes the workers the earlier one started, which the new
  one could not drive.
- Workers must run in a permission mode that never prompts. A permission prompt
  stalls a worker nobody is watching, so `/ignite pool` refuses to start workers
  in any other mode.
- `ignite pool pause` stops new feature work on a machine, and
  `ignite pool resume` starts it again. Workers that are running finish. A
  paused machine is still handed the tickets that hold `master` back (regression
  and justiciar tickets), one at a time: a person may pause to save usage or to
  work by hand, and `master` must not stay stuck while nobody watches.
  `ignite pool pause --all` stops every hand-off on every machine, those tickets
  included, until `ignite pool resume --all`. While any pool is paused,
  `ignite status` and the dashboard's first line name the tickets that hold
  `master` back. Nothing but a person's `resume` ends a pause.
- A usage hold is a different state, which Ignite starts and ends by itself.
  When the newest sample of a Claude plan window (see `usage` under "The
  dispatcher") is at or above `usage_hold_percent` under `[pool]` in
  `ignite.toml` (99 unless set), the machine behaves as `ignite pool pause`
  makes it: running workers finish, no new Claude ticket is handed out, and the
  tickets that hold `master` back still go to one worker at a time. When the
  plan runs out, every running worker fails mid-ticket and new tickets cannot
  run at all; a median ticket uses about 0.06 to 0.14% of the weekly limit, so
  99% leaves room for the tickets already running. The hold ends when a newer
  sample of that window is below the threshold or when the window's `resetsAt`
  passes, and samples are still taken every 15 minutes while it lasts. A window
  named for one model family, such as `Weekly · Fable`, holds only hand-offs
  whose model ID contains that name; any other window, such as
  `Weekly · all models` or `5-hour`, holds every Claude hand-off on the machine.
  Codex hand-offs are never held. `ignite status`, `ignite pool list`,
  `ignite pool usage` and the dashboard's first line name the window that holds,
  its percentage and when it resets.
- `ignite pool usage` prints how much of the Claude plan is used, from the
  dispatcher's latest samples (see `usage` under "The dispatcher").
- The pool outlives epics. Nothing is rebuilt when a plan starts or ends.
- Claude Code workers are cleared and prompted again for each ticket. Each Codex
  ticket uses a fresh local chat. A person can open it in the main checkout and
  send `ignite start <ticket>` directly.
- Codex desktop pool dispatch runs within an active turn. The thread tools
  require an explicit request to create the worker chats and explicit
  authorization to message their destinations. Slot registration records
  capacity. Start with the `In Codex` section generated by `ignite pool init`.
- Codex capacity is counted in slots. In Codex, `/ignite pool 5` registers the
  dispatcher with five slots
  (`ignite pool add <thread> --app codex --dispatcher --slots 5`); sending it
  again with another count changes the count, and `0` keeps it. Each free slot
  counts as an idle Codex worker when the daemon asks the hub for tickets, under
  a placeholder session `codex-slot-<k>`. The slot's hand-off names no session:
  the dispatcher opens a new chat in the main checkout and acknowledges with its
  thread ID (`ignite dispatch ack <id> ok <thread>`). The daemon adds that chat
  to the pool as a single-use worker, so stall checks, messages and justiciar
  instructions reach it like any other worker. The slot is free again when the
  ticket lands, its lease expires, its reservation lapses unclaimed after 10
  minutes, or the app says the chat is gone. A chat never gets a second ticket.
- Slots belong to the dispatcher, not the machine, so a Claude pool and a Codex
  pool on one machine stay independent; both share the machine's ticket trees
  and cores. A Codex dispatcher that rotates (see below) keeps its slots and the
  chats still running; a new Codex dispatcher registered with a count of its own
  replaces the old one's slots and chats. `ignite pool list` and the dashboard
  show each slot as a `codex` worker, idle or reserved, and each chat as a
  worker holding its ticket.

### The dispatcher

The dispatcher is one session per machine and per app whose whole job is to make
the tool calls the daemon asks for.

- In Claude it runs on Haiku 5.5, in `~/ignite`, a folder outside the
  repository, so its fixed prefix holds none of Beryl's instruction files. The
  folder's instructions name the main checkout, because a session started from
  the dispatcher otherwise runs in the dispatcher's folder.
- That folder's `.claude/settings.json` sets `autoCompactWindow` to 120,000
  tokens, so its context never grows the way an orchestrator's does today. It
  also sets the environment variable `CLAUDE_CODE_AUTO_COMPACT_WINDOW` to the
  same value. A Claude Desktop session can keep a window of its own that
  outranks `autoCompactWindow`: a dispatcher showed a 1M window and reached
  131,000 tokens without compacting until `/autocompact` was run in it. The
  environment variable outranks that window.
- The same file denies every tool the loop never calls (`permissions.deny`): the
  desktop app's browsers, simulator, charts, terminal, panes and task list, and
  the built-in editing, research, subagent and artifact tools. Claude Code drops
  a denied tool from the model's context. Without the list, about 60,000 of the
  120,000 tokens were in use before the dispatcher's first message, most of them
  tool definitions, which left room for only a short stretch of the loop between
  compactions.
- The same file holds a `PreToolUse` hook on the tools that start, clear,
  reconfigure or message a session: `start_session`, `clear_session`,
  `set_session_model`, `set_session_effort` and `send_message`. The hook runs
  `ignite dispatch allowed`, which reads the tool call from stdin and asks the
  daemon whether an instruction the dispatcher holds (given and not yet
  acknowledged) calls for that tool on that session. If not, it exits 2, which
  blocks the call and shows the dispatcher the current instruction. Without it,
  only the instructions' wording stopped a confused dispatcher from starting
  duplicate workers or clearing a busy one.
  - Clearing needs a hand-off with `clear` set to that session; setting the
    model or effort needs a hand-off to it; sending needs a `message` to it, or
    a hand-off to it whose `raw` read-back matched, so a prompt never reaches a
    worker before the daemon has seen its model and effort stick.
  - A hand-off with `clear` set may set the model and effort only after the
    clear succeeded. The app refuses to clear a session that is still working,
    and on 2026-11-19 a Haiku dispatcher set a busy worker's model anyway before
    stopping. A `PostToolUse` hook on `clear_session` runs
    `ignite dispatch cleared`, which tells the daemon the clear succeeded;
    Claude Code runs it only after a call that did not fail, and the app reports
    a refusal as a failure. A `raw` read-back of a hand-off whose worker was
    never reported cleared fails the hand-off, so the prompt is not sent. Claude
    Code reads hooks when a session starts, so the daemon holds hand-offs to
    this only once a clear has been reported: a dispatcher started before the
    hook existed carries on as before.
  - Starting a session needs a hand-off that names no session, a `rotate` in the
    last 10 minutes, or the pool setup of `/ignite pool <n>`: from
    `ignite pool add --dispatcher` until the workers are registered, at most 15
    minutes. It is also allowed while no Claude dispatcher is registered.
  - Every failure blocks, including a daemon that does not answer, except for
    the client's own justiciar hand-off while the daemon is down.
- It never plans, reads tickets or decides anything.
- On a Codex `rotate` instruction, the person can open a fresh dispatcher chat
  in `~/ignite` and send `/ignite pool 0` to keep the slots and running workers.

The Claude loop is one blocking command and the tool calls that command returns:

```text
$ ignite dispatch --wait 3600
```

The dispatcher runs the wait as a background command and ends its turn, and the
app wakes it when the command exits. A foreground tool call is cut off after 600
seconds, and every wait that ends `idle` costs a full request over the
dispatcher's context, so the wait is as long as an hour; it ends as soon as
there is something to do. Anything else that wakes the dispatcher meanwhile,
such as a notice from a worker, may lead it to start a second wait. The daemon
keeps only the newest wait from each dispatcher session: the older one ends
printing `replaced`, and the dispatcher then does nothing. Two waits never both
take instructions, and starting one too many never leaves the dispatcher without
a wait.

```json
{"id": 4812, "do": "handoff", "session": "local_c8470108",
 "clear": true, "model": "claude-opus-5-5", "effort": "medium",
 "prompt": "Run `ignite start TQ44S.9.48 --reservation 7f3a` and follow it."}
```

The dispatcher carries out a hand-off as separate calls, in the order listed
below, and reports `ignite dispatch ack 4812 ok`, or `failed` with the reason.

The `model` is an ID the app's session tool accepts, not a tier name. The daemon
looks the ticket's `tier:` label up in `[pool.models.<app>]` in `ignite.toml`:

```toml
# The model ID each app's session tool accepts for each `tier:` label.
[pool.models.claude]
opus = "claude-opus-5-5"
sonnet = "claude-sonnet-5-5"
light = "claude-sonnet-5-5"
```

Tier names stay the vocabulary for matching: ticket labels and a pool member's
`models` list use them, and the table only says what to ask the app for once a
worker has been chosen. When a model is released, change the IDs here and no
ticket changes. An app with no table is given the tier name as it is. A Claude
tier with no ID in the table is not reported to the hub as one the worker runs,
so no ticket that names it is handed to a Claude worker, and `ignite doctor`
warns about each such tier.

The `effort` is the ticket's `effort:` label, or `medium` when it has none. A
justiciar hand-off is always `high`.

1. Clear the session. A session that refuses is still finishing a turn. The
   hand-off fails, the daemon tries another worker, and it comes back to this
   one after a minute.
2. Set the model and effort, then read the session back with `get_session` and
   report what it returned, as it came:
   `ignite dispatch ack 4812 raw '<the JSON>'`. The app can ignore a model
   without saying so, so the daemon, not the dispatcher, compares the
   read-back's `model` and `effort` with what it asked for. A model ID's
   context-window suffix, such as `[1m]`, does not count as a difference. When
   they match, the instruction stays open and the reply says to send the prompt.
   When they differ, the hand-off fails as if the dispatcher had reported
   `failed`, and the reply says not to send the prompt: a ticket is never run on
   a model other than the one it names. An `ok` for a hand-off to a named
   session with no matching read-back is refused.
3. Send the prompt. It carries a reservation code that only this hand-off's
   `ignite start` can use, so a hand-off that arrives late, after the ticket
   went elsewhere, claims nothing.

The other instructions are `message`, which sends text to a live session,
`check`, which asks whether a session is running, idle or gone, `usage`, which
asks how much of the Claude plan is used, and `idle`, which means the wait ended
with nothing to do.

Nothing else in Ignite knows how much of the Claude subscription plan is used:
without samples, the pool kept handing out tickets until workers failed at the
limit, and nobody could say what a ticket cost as a share of the plan. The
desktop app tells any session through its `get_usage` tool, which the daemon
cannot call, so it asks a Claude dispatcher with `usage`:

- The daemon queues a `usage` ahead of a waiting hand-off when the last one was
  more than 5 minutes ago, and on its own every 15 minutes, so the series has no
  long gaps while nothing is handed off. A Codex dispatcher is never asked.
- The instruction's text names the tool: the dispatcher loads
  `mcp__ccd_session_mgmt__get_usage` with `ToolSearch`, calls it, and reports
  the result as it came with `raw`.
- The daemon stores one row per window of `plan.windows` (its `label`, such as
  `Weekly · all models`, `percentUsed` and `resetsAt`) in the local database's
  `plan_usage` table, with the time and the machine. A `plan.status` other than
  `ok` (`not_applicable`, `unavailable`) is stored as one row with no window and
  the plan's note, and a report that is not `get_usage`'s JSON as `unreadable`.
- Each row goes to the hub's `POST /metrics/import` under the source
  `plan_usage`, with `holds` set when it is at or above `usage_hold_percent`,
  which is how the dashboard knows of a usage hold (see "The pool"). A row the
  hub did not acknowledge is sent with the next sample.
- `ignite pool usage` prints the newest sample of each window, its age and when
  the window resets.
- `ignite pool init` no longer denies `get_usage` in the dispatcher folder's
  settings, and removes the rule an earlier run added. Claude Code reads the
  settings when a session starts, so a dispatcher started before then reports
  the tool's refusal, which is stored as `unreadable`.

The two judgments where a dispatcher's mistake costs most are whether a session
is gone and whether a hand-off's model stuck: a wrong `gone` releases a live
worker's ticket, and a skipped read-back runs a ticket on the wrong model. So a
Claude dispatcher makes neither. For a `check` it calls `get_session` and
reports the result as it came, JSON or error, with `raw`, and the daemon reads
it:

- an error saying the session was not found, or `isArchived: true`, is gone (an
  archived session can take no message);
- `isRunning: true` is running, and `isRunning: false` is idle;
- anything else, such as a timeout or a result about another session, is
  unknown, which proves nothing.

A Codex dispatcher answers a `check` with `running`, `idle`, `gone` or `unknown`
after reading the thread. For a hand-off, it passes the model and effort at chat
creation and acknowledges the resulting thread ID. A bounded Codex dispatch uses
`ignite dispatch --wait 30` and reads the result before the turn ends. In
Claude, the dispatcher stays titled `🔮 dispatcher <machine>` while it waits.
Each instruction identifies an action and its inputs. The numbered tool calls
for each action live in the dispatcher's `AGENTS.md`.

### CLI mode

A dispatcher is a session only a person can start, it can make mistakes, and
when it stops answering nothing automatic repairs the pool. In CLI mode an app
has no dispatcher on a machine: the daemon runs each worker itself as a headless
CLI process, `claude -p` or `codex exec`, one process per turn.
`ignite pool list` shows which mode each app on a machine runs.

```toml
# How each app's workers run on this machine: "desktop" (the default) or "cli".
[machine.jarvis.pool.claude]
mode = "cli"
slots = 8

# The program and the arguments every turn of the app gets.
[pool.cli.claude]
program = "claude"
args = ["--dangerously-skip-permissions"]
```

- `ignite pool mode <app> cli|desktop|config` stores an override in the daemon's
  database that outranks the file until it is set back to `config`, so a machine
  switches back without a landing or a restart. A switch never touches a ticket
  in progress: workers of the old mode finish, and only new hand-offs follow the
  new mode.
- Nothing about a session lives in the daemon's memory. A worker is a row in
  `cli_slots` (its ticket, app, model, effort, turn number and next prompt) and
  the transcript the CLI keeps on disk.
- A slot is offered to the hub as an idle worker under a placeholder session,
  `cli-<app>-slot-<k>`, as a Codex dispatcher's slots are. One slot above
  `slots` takes only tickets that hold `master` back, which replaces keeping a
  worker free for the justiciar. A reservation gives the slot an Ignite session
  ID, `cli-<app>-<8 hex>`, which every turn carries in `IGNITE_SESSION`. Each
  claim starts a new transcript; only the next ticket of a `lane:` chain resumes
  one.
- An app's slots are offered only after a one-word probe turn has succeeded on
  that machine since the daemon started.
- The daemon builds its CLI pool at start when `[pool.cli.<app>]` gives any app
  a program, and gives every such app an adapter whatever its mode, so
  `ignite pool mode` switches an app without a restart. Two loops follow the
  pool: `ignite-cli` reads every running turn and acts on each end about once a
  second, and `ignite-cli-health` runs the probe turns of the apps in CLI mode
  and sends each CLI hold's report. A probe runs a whole turn, so it never holds
  up the turns of other slots. `ignite doctor` lists both loops.

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
  it, and then becomes the CLI program, which keeps the lock until it exits. Two
  turns on one transcript are therefore impossible whatever a daemon decides. It
  also records the last turn it ran in `sessions/<session>/turn` and refuses a
  turn that is not newer, so even two supervisors started for one turn run the
  program once.
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
   one notification and one justiciar report. A single API error first retries
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
- `ignite tell --now`, and every order from the session that holds a justiciar
  ticket, ends the running turn's processes and resumes with the message. It
  waits while a `land` of the session has begun its push. An ordinary `tell`
  waits for the turn's end.
- Every `rate_limit_event` of a Claude turn is a sample of the plan's usage, so
  a machine whose Claude app is in CLI mode queues no `usage` instruction. Each
  turn's cost and token counts go to the hub with its ticket.
- Every column of the CLI tables and every field of JSON kept in them has a
  default, and the release selftest decodes every row of the copied database
  with the new build, so a release that cannot read an older build's rows is
  rejected before any daemon restarts into it.
- When the daemon has been down for 15 minutes and `justiciar-needed.json`
  exists, the monitor starts one headless justiciar turn in the main checkout
  with the crash evidence and resumes it on later runs, at most 6 turns per
  incident and never two at once.
- The daemon sends a digest of each transcript to the hub, which shows it at
  `/pool/session/<id>` with a box that sends `ignite tell`.
  `ignite pool watch <ticket or session>` follows the same digest in a terminal,
  read from the local logs (`--no-follow` prints what exists and exits). The
  digest of a turn is one line for each message the worker wrote, each tool call
  (its name and the first line of its input) and the turn's result, each cut to
  2,000 characters. The daemon queues new lines in its outbox, so a hub outage
  loses none. It forgets what it queued when it restarts and queues a running
  turn's lines again, which the hub stores once, keyed by session, turn and line
  number. `ignite pool list` shows each CLI slot's state and each CLI hold, and
  `ignite doctor` warns when an app in CLI mode has no program on the daemon's
  `PATH`. Transcripts can hold secrets, so the hub serves them on the Tailscale
  listener only.

A CLI worker's `ignite turn-ended` follows the desktop rules with two changes. A
live process under the session that is not an `ignite` verb does not let the
turn end, because the supervisor ends it with the turn: the first such stop is
blocked with a line that names `ignite run -- <command>` and `ignite wait`, and
the second releases the ticket. Titles do not apply. `ignite start` and
`ignite prime` leave out the title lines for a session whose ID starts with
`cli-`, and name the `cli-worker` guide, which says: start no background shell
commands, wait in the foreground with `ignite wait`, never ask a person, and end
the turn only after `ignite land` or `ignite release`, or to wait on an `ignite`
verb.

Clearing a worker, the settle time after a ticket, the model read-back, the
`dispatch allowed` hooks, rotation and session titles exist only for the desktop
dispatcher.

The `cli_e2e_*` tests in `ignite_daemon` run the whole of this against a shell
script that stands in for `claude`: real supervisors, a real database file and
the real `decide`, with only the clock faked. They cover a daemon restart during
a turn, a program killed mid-turn, a reboot that left no supervisor and no
result, a crash between recording a turn and starting it, two starts of one
turn, a row an older build wrote, a plan-limit end, a refusal with its CLI hold
and probe, `ignite tell --now`, and the block text followed by a release.

### What the daemon hands off, and when

A daemon with an idle worker asks the hub for the next ticket and says what the
machine has free. The hub answers with the first ticket, in this order, that the
machine can take:

1. An open regression ticket. It needs an idle worker and a free tree on a
   machine that can run its check, and skips the test of expected cores below.
2. An improvement ticket, when improvements are under their share. See
   [Improvement tickets](#improvement-tickets). It skips its epic's allocation
   but not its `--max-workers` cap.
3. A ready ticket from the epic that is furthest below its allocation. See
   [Dividing workers between epics](#dividing-workers-between-epics). Within an
   epic, the ticket of highest priority goes first, then the one with the
   longest chain of tickets waiting behind it, then the oldest.

A machine can take a ticket when all of these hold:

- A pool member that suits the ticket is idle: one of the ticket's owner (see
  [Owners](#owners)) whose app can run the model the ticket's `tier:` label
  names. A worker is idle when it holds no ticket.
- A ticket tree is free.
- The ticket's expected use of cores fits beside the tickets already in progress
  there. See [Light and heavy tickets](#light-and-heavy-tickets).
- The machine can run every check family that the ticket's named files could
  bring into its gate. Cygnus never takes a ticket that names UI files.

Not every ticket is offered:

- An epic is scheduled only once a person releases it. A ticket filed with no
  status takes one from the hub: an epic, and any ticket filed under a pending
  ticket (a parent created earlier in the same batch included), is filed with
  the status `pending_approval`; any other ticket is filed `open`, so a single
  ticket under no epic is offered to the pool at once. The pool is never offered
  a pending ticket. The hub's own improvements and fault reports are filed
  pending as well, so a person decides which of them to work. The exception is a
  ticket for anything that holds `master` back, which the hub files already
  released: a regression (a check that tip verification found failing on
  `staging`), every justiciar ticket, a machine a tip has waited an hour for,
  and checks left incomplete that the tip being verified waits on. A ticket for
  incomplete checks that no tip waits on stays pending, and the hub releases it
  when a later tip waits on them. `ignite ticket update <id> --release`, or the
  Release button on the dashboard, makes the ticket and every descendant still
  pending `open`: the hub adds the descendants to any update that releases a
  pending ticket. `ignite ticket create --release` files a ticket `open` even
  when it is an epic or sits under a pending one. `--pending` files any ticket
  pending, as `just bugs file` does for a player's bug report, so an agent can
  deduplicate and group the reports before any of them runs. A session may still
  start a pending ticket by name.
- An epic is worked only once it is active, which means it has an `--allocation`
  above 0. An epic filed without one gets an allocation of 3, so releasing it is
  enough; an allocation is a weight bounded by the pool, so it reserves nothing.
  An epic that has no allocation, such as one imported from Enact, gets the same
  allocation when it is released, so releasing one epic inside a larger project
  works it even when the project's epic has no allocation either. File it with
  `--allocation 0`, or set that later, to park a backlog: its tickets then wait,
  however ready they are. A ticket under no epic has nothing to activate, so
  releasing it is enough.
- Once released, tickets that the hub files itself are always offered:
  regressions, improvements and repairs to a machine. So is a ticket that a
  session starts by name.
- A ticket marked `needs:person` is never offered.

### Owners

Every ticket goes to one owner: a machine and the desktop app whose workers on
that machine take it, such as `jarvis/claude` or `cygnus/codex`.

- A ticket's owner is its own `owner` field or, when that is empty, its nearest
  ancestor's, so changing an epic's owner moves its tickets. A ticket with
  neither goes to the default owner, `[pool] default_owner` in `ignite.toml`
  (`jarvis/claude` here). Only a hub with no default owner configured offers
  such a ticket to any worker.
- A ticket a person or a session files goes where its line's owner, or the
  default owner, says, whichever machine filed it. It reaches another machine
  only when it, or an ancestor, names that machine as its owner, with `--owner`
  or `ignite release --needs <machine>`.
- A ticket a program files stays on the machine that filed it. A script or check
  files with `ignite ticket create --automatic`, which makes the machine running
  the command the owner (any app there) unless `--owner` names another, as the
  count trend does.
- Dispatch offers a ticket only to its owner's workers. An idle worker on
  another machine or of another app is never offered it, so the other five
  destinations get only work that is routed to them.
- The dashboard shows each ticket's owner and where it comes from, and its Owner
  dropdown lists None and every machine with every app. `ignite ticket create`
  and `update` take `--owner <machine>/<app>`, or `--owner none` to clear it.
- An owner may name only a machine (`cygnus`). Then any app's workers on that
  machine may take it.
- A ticket the hub files from a machine's report, such as a regression, checks
  left incomplete, a machine fault or a justiciar report, is owned by that
  machine alone, because the fault is often reproducible only there. Upkeep's
  own tickets, such as `master` not moving, have no owner and go to the default
  owner.
- A ticket that holds `master` back (each one the hub files released) falls back
  when its owner's machine has not been heard from for 30 minutes: then any
  machine that runs the operating system the ticket needs may take it.
- `ignite release --needs <machine>` makes that machine the owner,
  `--needs <os>` records the system the work needs, and `--needs person` sets
  the ticket aside for a person.
- Each app's workers run the tiers its `[pool.models.<app>]` table names. Codex
  runs every tier on `gpt-6.2-sol`.

The hub reserves the ticket for that machine in the same request, so two
machines never take one ticket. What happens next:

- The daemon gives the dispatcher the hand-off. The worker's `ignite start`
  turns the reservation into a claim held by that session and tree.
- A hand-off the dispatcher has not acknowledged in 2 minutes is given again.
  After three failures the daemon stops handing off, and the dashboard shows the
  dispatcher as down. Workers already running are unaffected.
- The dispatcher's queue (pending and given hand-offs, reservations, refusals,
  stalls and the next instruction ID) is kept in the daemon's database, so a
  restart loses none of it; a hand-off given and not acknowledged before a
  restart is given again at once.
- A reservation that no `ignite start` has claimed in 10 minutes lapses. The
  ticket returns to the queue, and the worker is treated as stalled.
- A session that runs a command for a ticket whose lease expired, and that
  another session has since claimed, gets exit code 3.

That is the whole of fleet scheduling: a machine with free workers, trees and
cores pulls work, and a busy one does not. The hub keeps no model of any
machine's load.

### Dividing workers between epics

A person decides how much of the fleet each epic gets by giving the epic an
allocation:

```sh
ignite ticket update TQ44S --allocation 5   # Beryl 3D
ignite ticket update TW8GV --allocation 2   # Project 50k
```

- An allocation is a weight, not a reservation. The hub gives the next free
  worker to the epic with the fewest tickets in progress per unit of allocation,
  counted across every machine.
- An allocation by itself never leaves a worker idle. An epic with fewer ready
  tickets than its allocation takes what it can use, and the others divide the
  rest in proportion.
- `--max-workers` is a hard cap, for an epic whose tickets conflict when too
  many run at once. It is the one setting here that can leave a worker idle.
- An epic with no allocation counts as 1. Tickets under no epic form one group
  with an allocation of 1.
- The allocation may sit on any ticket that has children. A ticket belongs to
  the nearest ancestor that has one.
- `--workers` is the flag's old name and still works. The hub stores the
  allocation in the `workers` column and sends it as the `workers` field.

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
- The hub keeps a total for the tickets in progress on a machine or reserved for
  it, counting each at its expectation. It cannot see what a ticket has used so
  far.
- A ticket is admitted when that total plus its own figure is within the
  machine's `admit_cores`: every core its scheduler hands out, which is its
  cores less its `express_cores`. On jarvis, with 18 cores and 3 express cores,
  that is 15. It is not a setting, because the scheduler already makes every
  heavy command wait for cores; admission only keeps the expected load of the
  tickets in progress within what the machine has.
- While workers stay idle, the `pool` line of `ignite status` says why: the
  first reason, in dispatch's order, that no ready ticket went to them. The
  daemon asks the hub for the reason with each request for tickets
  (`MachineFree.explain`), and the hub answers `{"nothing": "<reason>"}` instead
  of `null`. More workers would help only when the line is about workers or
  trees, never when it is about tickets or cores.
- While a `land` or `agent` request has waited two minutes or more for cores,
  the daemon asks the hub only for tickets that hold `master` back, as a paused
  pool does. The machine then has more work than cores, and a new ticket's
  commands would only make every command wait longer. Expected cores come from
  history and cannot see this: on 2026-11-20 jarvis kept starting workers while
  18 commands waited. The `pool` line says
  `commands have waited 150 s for cores, so only tickets that hold master back start`.

```text
$ ignite status
pool       3 idle workers: 2 ready tickets wait for cores: the smallest expects 2.5, and 1.0 of the machine's admitted cores are free
```

The other reasons are that no ready ticket is offered (file tickets or release
an epic), that the ready tickets are for another machine's or model's workers,
that their epics are at their `--max-workers` cap, that they need check families
the machine cannot run, that no ticket tree is free, and that the last idle
worker is kept for justiciar tickets.

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
  shown that the session is gone. The silence clock survives a restart: each
  held tree's last activity is kept in the daemon's database, so a worker that
  died just before a restart is still looked at 30 minutes after its last
  command. A worker removed from the pool when a new dispatcher replaces its own
  is looked at straight away if it still holds a ticket.
- A command stuck far past its time also counts as a stall, although it is
  running and keeps the lease renewed: a command the worker started, other than
  `wait`, that has run for 45 minutes, or whose current step has lasted 30
  minutes. The daemon has the dispatcher look at the session as it does for
  silence, and again every 30 minutes while the same command stays stuck. An
  idle session gets one message that names the command, its step and how long
  each has run, and says: if the ticket can still be done, continue it; if it
  waits on something outside the ticket, file that, make it a dependency and
  release; if it cannot be completed as written, run `ignite justiciar` with the
  evidence and release. A worker with a `land`, `release` or `close` running is
  never looked at for a stuck command, because a landing's long waits belong to
  the landing queue and tip verification.
- A session that is still running is left alone. An idle one is sent one message
  asking it to continue or release.
- The daemon releases the ticket only when the app confirms that the session is
  gone, or that it is idle and has stayed idle for 15 minutes after the message.
  Releasing keeps the work, as [Releasing work](#releasing-work) describes.
- A Claude worker that ends its turn while holding its ticket is caught at once,
  without waiting for the silence above. The repository's `Stop` hook
  (`.claude/settings.json`) runs at the end of every turn and asks
  `ignite turn-ended`, which decides. The first stop with the ticket held is
  blocked with a message that names the ticket and every way out: `ignite land`
  if the work is done, `ignite release` if it is not, and `ignite justiciar`
  followed by `ignite release` when the ticket cannot be completed as written
  (its acceptance contradicts its body, or gates on something the worker cannot
  control, such as a timing on a loaded machine). If the worker stops again in
  the same turn without doing either, the daemon releases the ticket itself,
  saving the tree as `ignite/<ticket>` and using the worker's last message as
  the release note. This applies only to a worker of the machine's pool that
  holds a ticket. A person's own session, including one running `/ignite`, ends
  turns to talk to the person, and is never blocked. Neither is a justiciar's
  session, nor a worker with an `ignite` verb of its session still running (the
  daemon guarantees a wake-up with a check two minutes after the verb ends), nor
  a worker with a live process under the session's process, such as a background
  shell command or monitor script, whose end wakes the worker. The hook passes
  its parent process with `--parent`, and Ignite looks for live descendants of
  it (macOS and Windows). With nothing running, the first stop is blocked and
  the next releases the ticket. A released worker is refused by `ignite start`
  until the dispatcher hands it a ticket. The hook names the session by the
  desktop app's `CLAUDE_CODE_HOST_SESSION_ID`, the ID Ignite records for the
  lease; the hook input's own `session_id` is Claude Code's internal ID, which
  Ignite never sees. Codex has no such hook, so Codex workers rely on the
  silence check alone, which also still catches workers that died.
- A pool worker whose title starts with ⛔ ("stopped unfinished") when it ends a
  turn holding its ticket is released at the first stop, without the block
  message: a worker once kept a ticket, its tree and a running build for hours
  under a ⛔ title. `ignite turn-ended` reads the title from the desktop app's
  session metadata, saves the tree as `ignite/<ticket>`, releases the ticket
  with the worker's last message as the note (resetting the tree stops the
  session's running commands, as `ignite release` does), and sends for a
  justiciar as `ignite justiciar` does, with the reason
  `<ticket>: worker stopped unfinished: <the first 400 characters of its last message>`.
  The hub folds repeated reports for the same ticket into one. A title that
  cannot be read changes nothing. A person's own session, a justiciar's session,
  a worker with a `land`, `release` or `close` running and a ticket labelled
  `needs:person` are left alone.
- The hub releases the claims of a machine that has not reported for 2 hours,
  with a comment on each ticket.
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
  `in_progress` ticket labelled `justiciar` may give it. It stops a command of
  any session, or one with no session such as a tip round a daemon loop started.
  It asks the command to stop, and if the command refuses or has not finished
  within 10 seconds, it ends the command's verb process and the process's whole
  group with `SIGTERM`, then `SIGKILL` 5 seconds later. The daemon then ends the
  commands the process left and its handle.
- A landing tree stays leased while the verb process that holds it still holds
  its lock, even after the daemon ended its handle. When the daemon frees a
  landing tree whose owner has finished, it first ends any process still running
  inside the tree.
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

- `ignite tell <ticket>` records the text as a comment on the ticket and marks
  it for delivery. The daemon of the machine where the ticket is being worked
  has its dispatcher send the text to that session as a message. Only a session
  that holds the ticket and claimed it before the comment was written gets the
  message. A comment from before the claim is already in the session's brief,
  and a ticket that has just been released sends nothing, so an old order never
  reaches the next session to take the ticket.
- A session that is inside `ignite wait` gets its wait ended at once, so it does
  not sit out the rest of the wait. The reply says only "<ticket> has a message
  for you from the dispatcher. End your turn to receive it." It does not carry
  the text, because a command's reply is tool output, which agents rightly treat
  as data and not as instructions. The dispatcher's message, which the session
  receives when its turn ends, is the only copy that carries the text.
- The told comment's `changed` line records that the message exists, who sent it
  and its first line in quotes:
  `changed  comment: message from <sender>, delivered by the dispatcher, starting "<first line>"`.
  Other comments print as before.
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
  two hours or a ticket has been held for six hours (30 minutes when the holding
  session's title starts with ⛔, or the daemon has reported it idle that long),
  and from `POST /justiciar`, which daemons send for a crash record or a loop
  that keeps failing, and which `ignite justiciar "<reason>"` sends for a
  session that meets an emergency.
- While open regression tickets explain the stall, upkeep files no master-lag
  ticket. It comments on each of those tickets instead, once per stuck landing,
  as `ignite tell` would, so the session working it gets the comment as a
  message. Nor does it file one for a stuck landing whose master-lag ticket a
  worker closed less than two hours ago: the worker either found nothing for
  Ignite to fix or landed a fix that has not yet moved `master`. A stall that
  outlasts that gets a new ticket.
- A report about one machine, such as its daemon's crash, is owned by that
  machine, so its pool works it unless the machine goes silent (see
  [Owners](#owners)).
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
- A person can also declare an emergency by invoking the `/ignite-justiciar`
  skill in any session. That session files or claims the justiciar ticket,
  declares the emergency pause (below), fixes the fault first and validates
  afterwards, and ends the emergency. Only it or the person ends the emergency.
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
- An order is a message from the dispatcher that starts
  `Message about <ticket>:`. `ignite tell` signs each comment `from <session>`,
  taken from the session the caller's environment names. A worker checks an
  order with `ignite ticket show <ticket>`: the ticket must be labelled
  `justiciar` and held by the session the order names. The skill therefore reads
  its own session ID (`mcp__ccd_session_mgmt__get_session` with `self`, or the
  thread ID on Codex) before it declares, and uses it in the ticket comment and
  every order.

#### The emergency pause

Asking sessions by message to stop work does not work: agents treat instructions
that arrive as tool output as data, so during one emergency no worker paused. A
refused command is a fact about the environment, which agents handle, so Ignite
enforces the pause itself.

- `ignite emergency declare <ticket>`, run by the justiciar's session, sets an
  emergency flag at the hub naming the justiciar ticket and the session, and
  pauses hand-offs on every machine. Each daemon hears the flag in its next
  heartbeat's reply (within 10 seconds) and keeps it in its local database, so a
  restart or a hub outage does not lift it.
- While the flag is in force, the verbs that start heavy work (`start`, `test`,
  `check`, `land`, `run`, `rebuild` and `audit`) refuse with exit code 6 and one
  line naming the emergency. Verbs that only read keep working. Commands already
  running are not stopped; the justiciar uses `ignite stop --force` for those.
- The justiciar's session, any session working the justiciar ticket, and the
  tickets the justiciar names with `ignite emergency allow <ticket> [<verb>]`
  are not refused.
- `ignite wait emergency` returns when the flag clears and prints the all-clear.
- The flag lapses two hours after it was declared unless the justiciar declares
  it again, so a justiciar that dies cannot leave the fleet stopped.
  `ignite status` and `ignite doctor` show it with its age and owner.
- Because Ignite enforces the pause, the justiciar sends `ignite tell --all`
  only when the fault needs something Ignite cannot enforce (for example "delete
  nothing of your own" on a full disk, or a session that must change what it is
  doing). It never asks sessions to pause or to retitle themselves. The
  all-clear message is optional, since refused verbs and `ignite wait emergency`
  already tell sessions. `ignite pool resume --all` ends it on every machine,
  and `ignite emergency clear --local` lifts it on one machine alone, for an
  operator stuck behind a flag nobody can end.

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
$ ignite plan check TIG9A
tickets        41, ready at the start 17
longest chain  6: TIG9A.1 > .4 > .9 > .15 > .22 > .30
suspect edge   .12 depends on .7, and they name no file in common
no files       .19, .27 name no files to read
missing file   .21 names tools/old.py, which does not exist
lane           lane:replay holds .31 and .33, which do not depend on each other
```

A ticket names a file in backquotes, or on a `Files:` line, where the backquotes
are optional. A short path such as `beryl_scene/src/lib.rs` stands for the one
tracked file whose path ends in it. A named file that does not exist is listed,
since it is either a typo or a file the ticket creates.

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
ticket_trees = 3
landing_trees = 2
families = ["rust", "doctests", "clippy", "rust-format", "ignite", "source"]
floor = { batch = 4, tip = 2 }
```

`families` lists the check families the machine can run, here the Rust tests and
doctests, clippy, Rust formatting, Ignite's own tests and the source rules (the
full list is in `ignite.toml`). A machine with no such list can run every
family.

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
  browser, oracle and app families. The `ignite-platform` family lists both
  `macos` and `windows`: it holds Ignite's tests of the code that behaves
  differently on each, so a change must pass them on a Mac and on cygnus before
  `master` moves.
- A daemon prefers the checks only it can run, then claims portable ones.

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
TM41Q  P2 task  improvement  rule:slow-crate
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
  tickets and for the dispatcher.
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
  `git revert --no-edit <revert commit>` its brief gives, fixes the build and
  lands; a dependent ticket is released and handed out again when the reverted
  one lands.
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

The work is tracked in the epic `THRH4`, whose design field holds the code
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
  working. Only a broken build is reverted (see
  [A broken build is reverted](#a-broken-build-is-reverted)).
- **More cores.** A larger machine shortens every build and leaves the race and
  the repeated builds growing as before.
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
- **An inactive epic.** File an epic with ready tickets and `--allocation 0`.
  Expect no hand-off from it. Set `--allocation 1` and expect one. File another
  with no `--allocation` and expect hand-offs at once.
- **A pause.** Run `ignite pool pause` with two workers busy and tickets ready.
  Expect both to finish, no new hand-off of feature work, and hand-offs again
  after `ignite pool resume`. File a regression while paused and expect it
  handed to one worker; with `--all`, expect it handed to nobody.
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
- **Allocations between epics.** Give one epic `--allocation 5` and another
  `--allocation 2`, each with twenty ready tickets, and a pool of seven. Expect
  five and two in progress. Close all but one of the second epic's tickets and
  expect six and one.
- **A light ticket on a busy machine.** Fill the machine with core-crate tickets
  until `ignite status` shows idle workers waiting for cores. File a ticket that
  names only a Python file. Expect it handed off at once, in the tree with the
  oldest build, with no Rust build started.
- **A plan.** Give `/ignite` a plan document. Expect an epic filed in one batch,
  a `plan check` report, and the planning session ending while the pool starts
  the ready tickets.

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
