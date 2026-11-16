# Checks

How `just check` and `just ci` work. The rules an agent must follow about checks
are in the root `AGENTS.md`; this page is the reference behind them.

The checks are defined in `.enact.toml`: `just check` (fast) and `just ci`
(slow). They are split into "lanes", one per kind of file or test.

## The fast check

`just check` runs the lanes whose files have changed since the branch left
`origin/staging`. It is held to a time contract (`deadline.py`), because slow CI
on staging is the gate that keeps `master` green and the fast check only has to
catch the likely breaks before a push:

- It should finish in 60 s on the reference machine with nothing else running
  (the load-adjusted clock, `clock.py`).
- However loaded the machine is, it should finish in 90 wall seconds. The steps
  that can choose their work plan for less of it under load.
- At 180 wall seconds it kills any step still running. That step and the rest of
  its lane are deferred to slow CI (`deferred: <lane> <step>` lines), and the
  run passes on what it did check.

The steps that choose read the seconds left until the target: the browser
checks, the Rust tests, the oracles and the motion capture. Each runs what fits
and logs the rest as cut or deferred.

The lanes:

- `repo`: justfile and Markdown formatting.
- `python`: black, pyre and pytest. If only cards or vocabulary changed, it runs
  just the scale harness's tests.
- `rust`: rustfmt, clippy and tests, then `card-text`. The tests that run are
  those that fit the time left once they are built (`rust_fit.py`): tests the
  run log has never timed first, then those that caught a break, then those that
  fail most often, then the cheapest. Doctests and the `allocations` binaries
  are planned the same way. Then `card-text` checks every card file's rendered
  rules text against its Oracle text, with the differences
  `data/card-text-allowlist.json` pins (`card_text.py`), skipped when the change
  cannot reach `beryl_cards` or the check's own lists and code. The tests
  include the mutation check `every_mutant_of_a_card_is_killed_by_its_scenarios`
  (about 3 s over every card file), which fails when changing one node of a
  card's effect data changes the outcome of none of its scenarios.
- `script`: `beryl_cli script-check`, when a crate it depends on changed. In a
  debug build it runs every card's spec moments from both seats and every UI
  fixture's recording, checking each animation script against its assertions.
  Each one is logged as a check named `script:<card>/<moment>/<seat>`. It and
  the browser lanes build `beryl_cli` with `paths.CLI_BUILD`, which names the
  same packages and profile as the rust lane's `cargo test`, so the two share
  one build in the checkout's target. `tools/tests/test_cli_build.py` fails if a
  dev-dependency turns on a feature that no normal dependency does, which would
  split them again.
- `ui`: `design-tokens` (`ui/src/design/tokens.css` is what
  `data/design-tokens.json` renders), prettier, tsc and `npm test`. A change to
  `data/design-tokens.json` runs it too.
- `names`, which holds several source checks:
  - `card-names`: no card names in generic code, apart from the entries in
    `data/card-names-allowlist.json`, whose counts may only go down.
    `python -m beryl_tools.checks card-names --shrink` lowers them after you
    remove some.
  - `two-player`: no rules module or card file uses `beryl_kernel::two_player`.
  - `board-door`: no rules module or card file uses `.board()` or `board::Board`
    outside tests.
  - `discarded-push`: no kernel or rules module or card file assigns a push's
    result to `_` outside tests.
  - `plan-references`: no comment in `rust/`, `ui/src`, `ui/scripts` or
    `tools/src` cites a planning document by decision id.
  - `citations`: `just citations` on the changed files, using the cached rules
    text, and skipped if none is cached. Slow CI runs it on the whole tree and
    downloads the text.
- `oracle`: in XMage and Forge, the scenarios in each changed card file that
  have not passed before, and, when the change reaches `beryl_cards` (a card
  file, the card store or a crate it depends on), each random scenario under the
  card `Generated` that no run has played. A rules change rewrites some of those
  without touching any card file. Every oracle run records each scenario's
  outcome under a hash of its dump entry and the oracles' side (their pins, the
  exporters, the harnesses and the card store) in
  `$BERYL_CACHE/oracles/outcomes.txt` (`oracle_memory.py`). A generated scenario
  that already failed elsewhere is left to slow CI, so a failure on staging
  fails no one else's push. It plays what fits the time left, cheapest first,
  each generated scenario on its own (`oracle_selection.fitted`). Skipped if
  another oracle run has held the oracles for more than 10 s. The scenarios are
  dumped by the `beryl_scenario` that `paths.CLI_BUILD` builds with the tests
  step's units, at opt-level 1: unoptimized, the dump took 48 s.
- `motion`: `just motion`, when the stage, its capture code or the animation
  data changed. It records each animation verb in stepped stage time, measures
  it the same way Arena's clips were measured, and compares it with its own
  values in `data/ui-conventions.json` (Arena's values are listed alongside).
  Each is logged as a check named `motion:<verb>/<layout>`. It shares the
  browser checks' 45 s box: when recording the fixtures (which builds
  `beryl_cli` if the cloned `rust/target` is behind) leaves too little of it for
  the capture, the capture is skipped and each check logged as cut. It also runs
  nightly.
- `efficiency`: black, pyre and pytest on `efficiency/`, the efficiency
  dashboard's own project. Only its paths, `efficiency/` and
  `data/efficiency-*.json`, select it, and they select no other lane but `repo`
  in the fast check (`tools/src/beryl_tools/efficiency.py`). Slow CI runs it
  too, when one of those paths changed.

For a change to `ui/` or `rust/`, it also runs as many of the `ui-test` and
`shots` browser checks as fit in the time left, choosing those the change could
break according to the coverage data (`just coverage`), worst failure history
first. The run log's `chosen` field says why each one ran, and `cut` marks the
ones left to the browser sweep. The fast check also rejects conflict markers and
badly formatted commit subjects.

`enact push` runs the fast check in the branch's own worktree and pushes to
staging. A push that loses the race to staging rebases and checks again,
rerunning only the lanes that both the branch's own change and the commits that
landed meanwhile select (`narrow` in `changes.py`). Enact's merge queue, which
checked each batch of pushes a second time behind a lock shared with Cygnus, is
off (`[push] queue` in `.enact.toml`): a lost lock threw passing checks away,
and pushes waited up to 20 minutes in it.

## Slow CI

`just ci` (an 800 CPU-second budget; see below) runs the lanes below, plus the
fast check's lanes, but only those that what changed since each lane last passed
can affect (`slow_ci.py`). It leaves out the steps in `verdicts.FAST_ONLY`, such
as the formatters, the Python tests and the script checks, which the fast check
already ran on every push that could break them. The `scale` and `script` lanes
have no other step, so slow CI runs nothing for them:

- Each lane's base is the commit at which it last passed on this machine
  (`$BERYL_CACHE/ci/lane-passes.json`), or where staging left `origin/master`. A
  lane that fails keeps its old base, so it runs again until it passes.
- A lane runs when a changed path reaches it: Rust and the data files the crates
  read reach the `rust` lane, `ui/` and the data files the UI imports reach
  `ui`, and both reach `app` and `ui-smoke`. A docs-only change runs just `repo`
  and `names`. Within a lane, `rust` tests only the packages the change can
  break, but clippy and the test build cover the whole workspace, so that cargo
  resolves the same features as the cached target and rebuilds only what
  changed.
- Tools changes are judged by fingerprint: each lane's pass records a hash of
  its step commands and of the `tools/src` modules they run or import. A lane
  whose fingerprint changed runs in full, as does a lane reached by a path no
  rule places.
- Every lane runs in full once an hour (`FULL_EVERY`), or when `$BERYL_CI_FULL`
  is set, so a wrong selection is caught within the hour.
- A lane left out gets a `skip` record in the run log saying why, and
  `just ci <lanes>` runs the named lanes in full, including the browser sweep's.

The lanes slow CI adds to the fast check's:

- `app`: `just smoke`.
- `ui-smoke`: two `ui-test` checks (`checks.SMOKE_TITLES`): the start screen
  starts a game, and on a recorded game the stage passes priority and plays a
  land. Each takes about a second and runs on the page's own clock, so slow CI
  still sees the browser UI start and play.
- `oracle`: `just oracle --select`, which needs `just oracle-setup` once per
  machine. It plays a sample of cards chosen from the commit hash whole, the
  scenarios of the cards changed since `origin/master` that have not passed
  before, and every other scenario that has not passed under its hash
  (`oracle_memory.py`), or every card if the exporters or harnesses changed. A
  scenario listed in `data/oracle-exporter-gaps.json` or
  `data/oracle-divergences.json` plays only with its card. The nightly run plays
  every card on staging.
- `counts`: `just count-gate`, which checks the rollout benchmark's instruction
  count against the limit in `rust/crates/beryl_bench/count-gate.toml`. That
  file's comments say when the limit may go up. Each staging commit's count goes
  in `$BERYL_CACHE/ci/counts.jsonl`, and the step prints the change since the
  last commit measured. Without failing anything, it files a `count-creep` bead
  when one tested range adds more than 0.25%, or when the count is more than
  0.5% above the lowest it reached since the limit last moved, and a
  `count-lower` bead when the count is more than 0.5% under the limit
  (`count_trend.py`). Its second step, `count-gate-families`, runs
  `just count-gate --family <name>` for each family with a bar in that file,
  failing a family whose present cost passes its ratio (`docs/benchmarking.md`,
  "The present-cost gate").

Slow CI runs on two machines: this Mac and Cygnus, a Windows PC. A pass on
either one moves `master` forward. A failure's Steward bead (the P0 bead Enact
files to get a slow-CI failure fixed) goes first to a machine on the OS that
failed, and to any machine after two hours. Slow CI doesn't run on a timer: each
machine tests `staging` whenever it's free and fixes failures as they appear.

Windows leaves `app`, `ui-smoke`, `oracle` and `counts` to the Mac, and its fast
check skips the browser checks and `motion` (see `WINDOWS_SKIPS` in
`checks.py`). Because a Windows pass moves `master` without running those lanes,
the Mac counts their changes from where each last passed on the Mac, even when
that is older than `master`, and a bisection of one of them searches from that
pass rather than from `master`.

The nightly also runs `just ui-counts --gate` on staging, which plays the board
fixtures named in `data/ui-counts.json` on Playwright's paused clock, one frame
at a time, and counts the UI's work: React components rendered, DOM mutations,
and elements Chromium restyled and laid out. These counts don't depend on the
machine's speed or load, so a rise of more than the file's tolerance (3%) over
its limits fails the step. The tolerance sits above the counts' run-to-run
noise, which is under 1% for these fixtures. A drop past the tolerance passes
and asks for lower limits (`--update`). It left slow CI for its CPU budget.

The nightly builds `beryl_link_iroh` and tests its transport over loopback. The
fast and slow Rust lanes exclude that package from clippy and tests, and the
slow app lane calls `just smoke --without-iroh`. A cold build of iroh can exceed
the fast budget. `just dev`, `just smoke` and `just bundle` enable the app's
optional `iroh` feature.

## The browser sweep

The full browser lanes are not slow CI's. `just sweep` (`sweep.py`) runs them on
`origin/staging` at utility priority on the Mac. Slow CI starts one in the
background after a run when none has run in 3 hours, so the sweep keeps pace
while staging moves with nothing to install; `just sweep-install` also adds a
launchd agent that fires every 3 hours.

- `shots`: `just ui-shots`, which renders every screen in `ui/shots.json` and
  fails on any page error.
- `ui-test`: `just ui-test`, which plays the game screen through fixtures,
  including every verb and prompt shape check.
- `mobile`: `cargo check` of the app for the iOS Simulator and Android, which
  needs `just mobile-setup`.

Which browser checks run where:

- The fast check plays the `ui-test` and `shots` checks a change can break, as
  many as fit its budget. This is the enforced form of "run the checks your
  change needs".
- Slow CI plays only `ui-smoke`.
- The sweep plays all of them, every 3 hours. With about 8 commits an hour, a
  failure leaves about 24 commits to search.

A firing skips when staging has not moved since the last sweep, when the load is
over 8 times the cores, or while another sweep runs. A sweep failure never
blocks `master`. The sweep bisects each failed check over the commits since the
last sweep that passed (up to 30 minutes), then files one P1 `sweep-failure`
Steward bead per lane naming the checks, the culprit commit or range and how to
replay them. A check an open sweep bead already names gets a comment instead.
Its steps' hang limit is 2 hours, because at utility priority on a loaded
machine they run many times slower than in a check. For the same reason, work at
low priority sets `BERYL_LOW_PRIORITY=1`, and its Playwright waits time out
after 3 minutes instead of 30 s (`WAIT_MS` in `ui/scripts/hang.mjs`): at a load
of 50 to 70, a shot that takes 2 s in a check ran past 30 s, passed on its rerun
and was filed as a flaky check to delete. A sweep that passes starts a coverage
run of its commit in the background, so the coverage data the fast check chooses
by is measured nightly and after each passing sweep. A ui/src line changed since
the coverage's commit is one the data cannot place. The record is in
`$BERYL_CACHE/sweep`.

## Beryl 3D

Beryl 3D, the native client in its own Cargo workspace, has checks of its own
(see [Checks and isolation](beryl-3d.md#checks-and-isolation)).

- A change under `rust/beryl3d/` or `data/beryl3d/` runs the `beryl3d` lane of
  the fast check, plus `repo` and `names`, and no other lane. Every other lane
  is chosen from the rest of the change's paths
  (`tools/src/beryl_tools/beryl3d/paths.py`), so a change elsewhere never runs
  or builds Beryl 3D's code.
- Slow CI has no `beryl3d` lane, and Beryl 3D's paths reach none of its lanes
  but `repo` and `names`.
- `just b3d lane` runs the lane by hand, and `just b3d` lists Beryl 3D's other
  commands. Its steps are in `tools/src/beryl_tools/beryl3d/lane.py`; `test`
  runs every interface test (`just b3d test`) with the checks applied at the
  check times.
- A change to Beryl 3D's tools, under `tools/src/beryl_tools/beryl3d/`, runs the
  python lane, as any change to `tools/` does, and not the `beryl3d` lane.
- A step of the `beryl3d` lane that the fast check cuts off at its 180 s hard
  limit is left to the Beryl 3D sweep, not to slow CI.

### The Beryl 3D sweep

`just b3d sweep` (`tools/src/beryl_tools/beryl3d/sweep.py`) finds what isolation
hides: a change to the session, the fixture recorder, the image cache or a data
file Beryl 3D reads can land without Beryl 3D's checks and leave it broken.

- Slow CI starts it in the background after each run, on both machines, when
  staging holds a commit since the last sweep that changes a Beryl 3D path, its
  tools, `rust/Cargo.toml`, `beryl_session`, `beryl_cli` or `beryl_art` (with
  every crate they depend on by path), `data/ui-conventions.json`,
  `data/card-store.tsv` or `ui/fixtures.json`. It skips when the load is over 8
  times the cores or another sweep runs.
- In a detached worktree of the staging tip it runs every step of the `beryl3d`
  lane in full, then the sweep's own steps, each as a lane of its own
  (`b3d-<step>`), at utility priority. Its checks are logged under the script
  `b3d-sweep`, which slow CI's CPU budget does not count.
- Its own steps: `every-tick` (`just b3d test --every-tick`) runs every
  interface test again with the checks applied on every tick, the exhaustive
  form of the lane's `test` step; `frame-counts` (`just b3d gate --counts`, on
  the Mac only) counts the gated boards' instructions per frame under Callgrind
  in the `beryl3d-bench` machine and holds them to `data/beryl3d/gates.json`.
- A failed step is bisected over the commits since the last sweep that passed,
  for at most 15 minutes, and filed as a P1 `b3d-sweep` bead under Beryl 3D's
  epic. A check an open sweep bead already names gets a comment. Nothing holds
  `master`.
- After its build passes, it saves the workspace's build directory to
  `$BERYL_CACHE/b3d/target`, which `just b3d` clones into new worktrees.
- `--step <name>` runs one step, `--commit` sweeps another commit, and
  `--dry-run` prints the beads it would file instead of filing them. The record
  is in `$BERYL_CACHE/b3d/sweep`: `log.jsonl` and `last-output.txt`.

## Beads the checks file

The checks file beads by themselves: `flaky-check`, `ci-cut` and `check-budget`
(below), `sweep-failure` (above), `nightly-failure`, and the P0 `role:steward`
bead for a failed slow-CI run. `just beads` leaves them all out, and the `cifix`
skill works them as one Enact run, grouped by shared cause. The Beryl 3D sweep's
`b3d-sweep` beads are not among them: they are Beryl 3D's own work, filed under
its epic.

## Flaky checks

- A step that fails is rerun once, narrowed to the checks it says failed. A
  check that passes on the rerun is flaky: its lane passes, it prints a
  `flaky: <lane> <check>` line, and it files a P1 `flaky-check` bead
  (`flaky.py`) to delete the check. Each check has at most one open bead; a
  later flake comments on it.
- The agent working that bead first asks whether a fast, deterministic test (a
  Rust unit test, a Node test with no browser, a check against a recorded
  fixture) could keep most of what the check protects. If so, it writes that
  test and deletes the check. If not, it deletes the check when it has never
  caught a bug, or moves it to the nightly when it has.
- There is no quarantine. Every check that runs can fail CI, except those on
  `verdicts.PENDING`, each tied to the bead whose work will make it pass.

## Budgets and hang limits

- A run that goes over budget still passes. No timer fails a run for being slow.
  A fast check over its 60 s target, or over 90 wall seconds, files a
  `check-budget` bead (`budget.py`) to speed up, move or delete its slowest
  steps instead of raising the budget. A step the fast check's 180 s hard limit
  cuts off is deferred to slow CI, not failed.
- Slow CI's budget is in CPU seconds (`budget.SLOW_CPU_BUDGET`, 800), because
  the machine is never quiet and wall seconds measure its load. Its summary line
  still shows wall seconds against 300 s (450 s on Windows), which files
  nothing. After each run on the Mac, slow CI estimates a full run: each check's
  median CPU seconds, summed over the checks that ran in one of their lane's
  last 10 runs (`check_value.full_run`). The oracle lane plays a sample of cards
  that changes with each commit, so it counts the median CPU seconds of one of
  its runs instead, and no cut bead names its cards (`check_value.SAMPLED`).
  When that is over the budget, it files one P1 `ci-cut` bead per lane naming
  the lowest-value checks (see below) whose removal brings a full run under the
  budget, with each check's runs, catches and CPU seconds. The agent working a
  bead deletes each check that caught nothing in 30 days and moves each one that
  caught something to the nightly, after first asking whether a fast,
  deterministic test could keep what the check protects. A check the fast check
  also runs leaves slow CI alone: it joins `verdicts.FAST_ONLY`, which slow CI's
  lanes leave out (a Rust or Python test, a script check, or a whole step such
  as `ui:test` or `rust:fmt`), and a test that only the nightly's `just fuzz`
  should play is marked `#[ignore]`. The checks in `budget.KEEP` (the app
  starting, the `ui-smoke` checks, clippy, the UI's typecheck, the `names` lane,
  and `rust:tests`, the record that stands for every Rust test where
  cargo-nextest is missing) are never named. While a `ci-cut` bead is open
  nothing more is filed; once the last one closes, only runs that started after
  it count, and slow CI waits for 10 of them.
- Each step has a hang limit (`hang.py`): 4x its slowest pass on this machine in
  the last 30 days, at least 900 s, at most 2400 s, and 1800 s for a step with
  no pass on record. A step still running at its limit is very likely stuck, so
  it is killed with everything it started, and its lane fails with an
  `APPEARS HUNG` line followed by the processes it left running. Its records in
  the run log carry `"timeout": "hang"`, so it gets no verdict, and bisect does
  not search it. A hung lane is not rerun. `CHECK_HANG_LIMIT_S` sets every
  step's limit.
- Slow CI cuts off a step still running a minute before Enact's `slow_timeout`
  (an hour, in `.enact.toml`) would kill the whole run, so the log still names
  the step (`CUT OFF`). The timeout sits above every step's hang limit, so it
  should never be reached. A bisection after a failed run searches for at most
  600 s.
- Budgets and hang limits are measured in seconds on the reference machine (the
  one the budgets were set on) with nothing else running. While the load average
  is above the number of cores, the clock runs proportionally slower, up to 8x.
  On a machine slower than the reference, it also runs slower by that machine's
  speed factor (`speed.py`). So a check takes the same budget seconds on a busy
  or slow machine as on an idle reference one.
- The speed factor is this machine's time for a fixed CPU workload divided by
  the reference's time in `data/speed-reference.json`. The first check run on a
  machine with no saved time measures it in the background, and the factor
  applies from the next run; `just speed` measures it on demand. Run
  `just speed --reference` on the reference machine and commit the file to set
  the reference. Neither needs a quiet machine: each round of the workload is
  adjusted for load the same way the clock is, and the best of five rounds over
  about a minute is kept. Until both times exist, the factor is 1.
- If a push fails because a step appears hung, look at the processes its log
  lists before pushing again: a rerun will hang again.

## What each check is worth

`just ci-value` scores every check slow CI ran in the last 30 days by its real
catches (failures judged `catch`: a fix to product code made it pass) per
CPU-second, lowest value first (`check_value.py`). CPU time, not wall time, is
the cost, because the machine is never quiet. Records from before the run log
measured CPU time are estimated from their wall seconds and marked `~`. Checks
that failed in exactly the same runs, such as the variants of one verb, are
scored as one group, since together they cost the sum but catch only once. A
full run's cost is each check's median CPU seconds, summed over the checks slow
CI still runs, because most runs leave out the lanes and Rust packages a change
cannot affect; the oracle lane, which plays a different sample of cards each
run, counts what one of its runs costs. `--budget <cpu seconds>` also lists the
lowest-value groups whose removal brings a full run under that budget.

## Recording CPU use

`just cpu install` keeps a launchd agent running `by cpu sample`, which samples
every process's CPU time every 5 seconds and appends one row per minute to
`$BERYL_CACHE/cpu/samples.jsonl`, keeping 30 days; `just cpu uninstall` removes
the agent, and `just cpu sample` runs the sampler in a terminal instead. Each
process's own CPU and the CPU of the children it has reaped come from
`proc_pid_rusage`, so processes that exit between two samples are still counted.
`just cpu report --hours 3` sums the rows into cores used by context (slow CI,
fast check, session, editor, desktop, browser, other), kind of work (compile,
Rust test, browser, oracle, simulator, editor, desktop), worktree and the
largest executables no rule names, with per-minute and burst figures and `top`'s
own busy figure for comparison. `cpu.py` explains how processes are attributed.

## Shared machine resources

- Heavy commands take CPU tokens from one pool per machine, one token per core
  (`tools/src/beryl_tools/tokens.py`), and run with as many threads as they got
  (`CARGO_BUILD_JOBS`, `NEXTEST_TEST_THREADS`, `RUST_TEST_THREADS`). The heavy
  steps of a check (clippy, the Rust tests, the app, browser, script, oracle,
  motion, mobile and counts steps), `just test-rust`, `just fuzz`,
  `just exhaust` and `just freeze-saves` take them; the other steps are scripts
  that use one core at most and take none. One command takes at most two thirds
  of the pool, and a check's heavy steps take an even share of it among the
  lanes with heavy steps still running, so ten lanes that start at once share
  the pool instead of the first taking most of it.
- Cargo holds one lock on a target directory for the whole of a command, so two
  cargo commands in one target run one after the other. Clippy therefore builds
  in its own target directory, `rust/target/clippy` (`paths.CLIPPY_TARGET`),
  since its units are not the ones a build or test produces, and runs beside the
  build. The script, shots, ui-test, ui-smoke and oracle steps all start by
  building `beryl_cli` (`paths.CLI_BUILD`) in the checkout's target. The first
  of them to start builds it for all of them, with the most tokens one command
  may take, before any of them takes its share (`step_runner.BUILDS_CLI`). The
  others wait for that build holding no tokens, and the wait is reported with
  the wait for tokens.
- The classes, highest first, are the merge queue's fast check of a batch of
  pushes (unused while the queue is off), the fast check a push runs in its own
  worktree, a command an agent runs by hand, and slow CI. Every push on the
  machine waits for the queue's check, so it comes first: pushes' own checks
  together hold at most seven eighths of the pool. Agents' commands and slow CI
  together hold at most three quarters, so a push always finds free tokens. Slow
  CI may take any of those tokens that nobody else holds. A class takes nothing
  while a higher one waits. Work at utility QoS (tournaments, the nightly, the
  justfile's `low`) takes no tokens, because macOS already gives every core to
  normal-priority work first.
- A check step's wait for tokens does not count against its hang limit or the
  run's budget: the step's log line reports the wait apart from the step's own
  time. `just tokens` shows who holds and waits for tokens. `BERYL_CPU_POOL=0`
  turns the pool off, and `just tokens-run -- COMMAND` runs any command inside a
  lease.
- The machine runs at most half its core count's worth of browser pages at once,
  across all runs (`ui/scripts/slots.mjs`). Runs that aren't checks share a
  third of those slots. Headless WebKit runs at a higher priority than any
  build, so run only the checks your change needs (`just ui-test <title>`,
  `just specs --shots <card>`) rather than a full sweep. Bare
  `just specs <card>` serves the spec pages until interrupted; it is not a
  check.
- `enact worktree create` hands out one of ten reused directories,
  `.worktrees/slot-01` to `slot-10` (`[worktree] slots` in `.enact.toml`). A
  slot keeps its `rust/target` and `ui/node_modules` between changes, because
  rustc ignores incremental caches built at another path, and a new path rebuilt
  every changed crate from scratch. When every slot is busy, the worktree gets a
  new directory. A new directory starts with no `rust/target` or
  `ui/node_modules`. The first `just` command that needs one copies it from the
  cache in seconds (copy-on-write, `tools/src/beryl_tools/warm.py`), and
  removing the worktree saves them back. Plain `npm` or `cargo` commands don't
  do this, so run `just warm` (or `just ui-typecheck`, `just ui-unit`) first.
