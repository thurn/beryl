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
  `data/card-text-allowlist.json` pins (`card_text.py`). The tests include
  `every_instruction_of_a_card_is_carried_out_by_its_own_scenarios`, which fails
  on any branch of a card's effect data that none of its scenarios reaches.
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
break according to the nightly coverage data (`just coverage`), worst failure
history first. The run log's `chosen` field says why each one ran, and `cut`
marks the ones left to slow CI. The fast check also rejects conflict markers and
badly formatted commit subjects.

## Slow CI

`just ci` (300 s budget; 450 s on Windows) runs the lanes below, plus the fast
check's lanes, but only those that what changed since each lane last passed can
affect (`slow_ci.py`):

- Each lane's base is the commit at which it last passed on this machine
  (`$BERYL_CACHE/ci/lane-passes.json`), or where staging left `origin/master`. A
  lane that fails keeps its old base, so it runs again until it passes.
- A lane runs when a changed path reaches it: Rust and the data files the crates
  read reach the `rust` lane, `ui/` and the data files the UI imports reach
  `ui`, and both reach `app`, `shots` and `ui-test`. A docs-only change runs
  just `repo` and `names`. Within a lane, `rust` tests only the packages the
  change can break, and `shots` and `ui-test` play every check the coverage data
  says the change can break (`--affected`), or all of them when it cannot say.
- Tools changes are judged by fingerprint: each lane's pass records a hash of
  its step commands and of the `tools/src` modules they run or import. A lane
  whose fingerprint changed runs in full, as does a lane reached by a path no
  rule places.
- Every lane runs in full once an hour (`FULL_EVERY`), or when `$BERYL_CI_FULL`
  is set, so a wrong selection is caught within the hour.
- A lane left out gets a `skip` record in the run log saying why, and
  `just ci <lanes>` runs the named lanes in full.

The lanes slow CI adds to the fast check's:

- `app`: `just smoke`.
- `shots`: `just ui-shots`, which fails on any page error.
- `ui-test`: `just ui-test`, which plays the game screen through a fixture.
- `oracle`: `just oracle --select`, which needs `just oracle-setup` once per
  machine. It runs the cards changed since `origin/master` plus a sample chosen
  from the commit hash, or every card if the exporters or harnesses changed. The
  nightly run plays every card on staging.
- `counts`: `just count-gate`, which checks the rollout benchmark's instruction
  count against the limit in `rust/crates/beryl_bench/count-gate.toml`. That
  file's comments say when the limit may go up.
- `mobile`: `cargo check` of the app for the iOS Simulator and Android, which
  needs `just mobile-setup`.

Slow CI runs on two machines: this Mac and Cygnus, a Windows PC. A pass on
either one moves `master` forward. A failure's Steward bead (the P0 bead Enact
files to get a slow-CI failure fixed) goes first to a machine on the OS that
failed, and to any machine after two hours. Slow CI doesn't run on a timer: each
machine tests `staging` whenever it's free and fixes failures as they appear.

Windows leaves `app`, `shots`, `ui-test`, `oracle`, `counts` and `mobile` to the
Mac, and its fast check skips the browser checks and `motion` (see
`WINDOWS_SKIPS` in `checks.py`).

## Budgets and deadlines

- A run that goes over budget still passes, and files a `check-budget` bead
  (`budget.py`) to speed up, move or delete its slowest steps instead of raising
  the budget. The deadline, at least 600 s, only catches a hang.
- Budgets and deadlines are measured in seconds on the reference machine (the
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
- A lane killed by its deadline automatically reruns once with a fresh deadline,
  so if a push still times out, some step is slow or hung. Find it before
  pushing again.

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
  of the pool.
- The classes, highest first, are the fast check of a push, a command an agent
  runs by hand, and slow CI. Agents' commands and slow CI together hold at most
  three quarters of the pool, and slow CI at most half, so a push always finds
  free tokens. A class takes nothing while a higher one waits. Work at utility
  QoS (tournaments, the nightly, the justfile's `low`) takes no tokens, because
  macOS already gives every core to normal-priority work first.
- A check step's wait for tokens counts against the run's deadline, but the
  push's reserved quarter keeps that wait short. `just tokens` shows who holds
  and waits for tokens. `BERYL_CPU_POOL=0` turns the pool off, and
  `just tokens-run -- COMMAND` runs any command inside a lease.
- The machine runs at most half its core count's worth of browser pages at once,
  across all runs (`ui/scripts/slots.mjs`). Runs that aren't checks share a
  third of those slots. Headless WebKit runs at a higher priority than any
  build, so run only the checks your change needs (`just ui-test <title>`,
  `just specs <card>`) rather than a full sweep.
- A new worktree copies the cached `rust/target` and `ui/node_modules` in
  seconds (copy-on-write), and removing the worktree saves them back.
