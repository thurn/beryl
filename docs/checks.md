# Checks

How `just check` and `just ci` work. The rules an agent must follow about checks
are in the root `AGENTS.md`; this page is the reference behind them.

The checks are defined in `.enact.toml`: `just check` (fast) and `just ci`
(slow). They are split into "lanes", one per kind of file or test.

## The fast check

`just check` (60 s budget) runs the lanes whose files have changed since the
branch left `origin/staging`:

- `repo`: justfile and Markdown formatting.
- `python`: black, pyre and pytest. If only cards or vocabulary changed, it runs
  just the scale harness's tests.
- `rust`: rustfmt, clippy and tests, then `card-text`: every card file's
  rendered rules text against its Oracle text, with the differences
  `data/card-text-allowlist.json` pins (`card_text.py`). The tests include the
  mutation check `every_mutant_of_a_card_is_killed_by_its_scenarios` (about 3 s
  over every card file), which fails when changing one node of a card's effect
  data changes the outcome of none of its scenarios.
- `script`: `beryl_cli script-check`, when a crate it depends on changed. In a
  debug build it runs every card's spec moments from both seats and every UI
  fixture's recording, checking each animation script against its assertions.
  Each one is logged as a check named `script:<card>/<moment>/<seat>`.
- `ui`: prettier, tsc and `npm test`.
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
- `oracle`: the scenarios in each changed card file, run in XMage and Forge.
  Skipped if another oracle run has held the oracles for more than 10 s.
- `motion`: `just motion`, when the stage, its capture code or the animation
  data changed. It records each animation verb in stepped stage time, measures
  it the same way Arena's clips were measured, and compares it with its own
  values in `data/ui-conventions.json` (Arena's values are listed alongside).
  Each is logged as a check named `motion:<verb>/<layout>`. It also runs
  nightly.

For a change to `ui/` or `rust/`, it also runs as many of the `ui-test` and
`shots` browser checks as fit in the budget, choosing those the change could
break according to the coverage data (`just coverage`), worst failure history
first. The run log's `chosen` field says why each one ran, and `cut` marks the
ones left to the browser sweep. The fast check also rejects conflict markers and
badly formatted commit subjects.

## Slow CI

`just ci` (an 800 CPU-second budget; see below) runs the lanes below, plus the
fast check's lanes, but only those that what changed since each lane last passed
can affect (`slow_ci.py`):

- Each lane's base is the commit at which it last passed on this machine
  (`$BERYL_CACHE/ci/lane-passes.json`), or where staging left `origin/master`. A
  lane that fails keeps its old base, so it runs again until it passes.
- A lane runs when a changed path reaches it: Rust and the data files the crates
  read reach the `rust` lane, `ui/` and the data files the UI imports reach
  `ui`, and both reach `app` and `ui-smoke`. A docs-only change runs just `repo`
  and `names`. Within a lane, `rust` tests only the packages the change can
  break.
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
  machine. It runs the cards changed since `origin/master` plus a sample chosen
  from the commit hash, or every card if the exporters or harnesses changed. The
  nightly run plays every card on staging.
- `counts`: `just count-gate`, which checks the rollout benchmark's instruction
  count against the limit in `rust/crates/beryl_bench/count-gate.toml`. That
  file's comments say when the limit may go up.

Slow CI runs on two machines: this Mac and Cygnus, a Windows PC. A pass on
either one moves `master` forward. A failure's Steward bead (the P0 bead Enact
files to get a slow-CI failure fixed) goes first to a machine on the OS that
failed, and to any machine after two hours. Slow CI doesn't run on a timer: each
machine tests `staging` whenever it's free and fixes failures as they appear.

Windows leaves `app`, `ui-smoke`, `oracle` and `counts` to the Mac, and its fast
check skips the browser checks and `motion` (see `WINDOWS_SKIPS` in
`checks.py`).

## The browser sweep

The full browser lanes are not slow CI's. `just sweep` (`sweep.py`), from a
launchd agent every 3 hours (`just sweep-install`), runs them on
`origin/staging` at utility priority on the Mac:

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
machine they run many times slower than in a check. A sweep that passes starts a
coverage run of its commit in the background, so the coverage data the fast
check chooses by is measured nightly and after each passing sweep. A ui/src line
changed since the coverage's commit is one the data cannot place. The record is
in `$BERYL_CACHE/sweep`.

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
  A fast check over its 60 s budget files a `check-budget` bead (`budget.py`) to
  speed up, move or delete its slowest steps instead of raising the budget.
- Slow CI's budget is in CPU seconds (`budget.SLOW_CPU_BUDGET`, 800), because
  the machine is never quiet and wall seconds measure its load. Its summary line
  still shows wall seconds against 300 s (450 s on Windows), which files
  nothing. After each run on the Mac, slow CI estimates a full run: each check's
  median CPU seconds, summed over the checks that ran in one of their lane's
  last 10 runs (`check_value.full_run`). When that is over the budget, it files
  one P1 `ci-cut` bead per lane naming the lowest-value checks (see below) whose
  removal brings a full run under the budget, with each check's runs, catches
  and CPU seconds. The agent working a bead deletes each check that caught
  nothing in 30 days and moves each one that caught something to the nightly,
  after first asking whether a fast, deterministic test could keep what the
  check protects. A check the fast check also runs leaves slow CI alone. The
  checks in `budget.KEEP` (the app starting, the `ui-smoke` checks, clippy, the
  UI's typecheck, the `names` lane) are never named. While a `ci-cut` bead is
  open nothing more is filed; once the last one closes, only runs that started
  after it count, and slow CI waits for 10 of them.
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
cannot affect. `--budget <cpu seconds>` also lists the lowest-value groups whose
removal brings a full run under that budget.

## Recording CPU use

`just cpu sample` samples every process's CPU time every 2 seconds and appends
one row per minute to `$BERYL_CACHE/cpu/samples.jsonl`; leave it running in a
terminal while agents work. `just cpu report --hours 3` sums the rows into cores
used by context (slow CI, fast check, session, editor, desktop, browser, other),
kind of work (compile, Rust test, browser, oracle, simulator, editor, desktop),
worktree and the largest executables no rule names, with per-minute and burst
figures and `top`'s own busy figure for comparison. `cpu.py` explains how
processes are attributed.

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
- The classes, highest first, are the fast check of a push, a command an agent
  runs by hand, and slow CI. Agents' commands and slow CI together hold at most
  three quarters of the pool, so a push always finds free tokens. Slow CI may
  take any of those tokens that nobody else holds. A class takes nothing while a
  higher one waits. Work at utility QoS (tournaments, the nightly, the
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
  `just specs <card>`) rather than a full sweep.
- A new worktree copies the cached `rust/target` and `ui/node_modules` in
  seconds (copy-on-write), and removing the worktree saves them back.
