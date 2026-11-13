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
- `rust`: rustfmt, clippy and tests.
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

`just ci` (300 s budget; 450 s on Windows) runs every lane, plus:

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

## Shared machine resources

- The machine runs at most half its core count's worth of browser pages at once,
  across all runs (`ui/scripts/slots.mjs`). Runs that aren't checks share a
  third of those slots. Headless WebKit runs at a higher priority than any
  build, so run only the checks your change needs (`just ui-test <title>`,
  `just specs <card>`) rather than a full sweep.
- A new worktree copies the cached `rust/target` and `ui/node_modules` in
  seconds (copy-on-write), and removing the worktree saves them back.
