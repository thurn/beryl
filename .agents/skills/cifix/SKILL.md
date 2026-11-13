---
name: cifix
description: Work the open beads that Beryl's CI files by itself — flaky checks, checks to cut for slow CI's budget, fast-check budget audits, browser-sweep failures, nightly failures and unclaimed slow-CI Steward beads — as an Enact run. Groups them by shared cause, routes them into lanes and model tiers, then runs them through /enact-plan's vizier loop, where worker sessions fix the cause, replace a check with a deterministic test, delete it or move it to the nightly. Use when asked to work, fix or clear the CI beads, flaky checks, CI cuts or sweep or nightly failures, or to run /cifix.
---

# Fixing the beads CI files

Beryl's checks file beads by themselves. Each kind has its own label, and
`just beads` leaves all of them out:

| Label             | Filed by                    | What it asks for                                                                                          |
| ----------------- | --------------------------- | --------------------------------------------------------------------------------------------------------- |
| `flaky-check`     | `flaky.py`                  | A check failed, then passed on a rerun of the same tree. Find the cause; fix, replace or delete it.       |
| `ci-cut`          | `budget.py` (`file_cut`)    | Slow CI's full run is over its CPU budget. Take the listed low-value checks out of slow CI.               |
| `check-budget`    | `budget.py` (`report`)      | A fast check (`by check`) ran over its 60 s budget. Speed up, move or delete its slowest steps.            |
| `sweep-failure`   | `sweep.py`                  | The 3-hourly browser sweep failed checks on staging, bisected to a commit or range. Fix the break.        |
| `nightly-failure` | `nightly_failures.py`       | The nightly (fuzz, motion capture, oracle sweep) failed checks on staging. Fix the break.                 |
| `role:steward`    | Enact's slow-CI runner      | Slow CI failed (P0, titled "Fix slow CI failure on …"). `master` stays put until it is fixed.             |

The sweep and nightly beads also carry `role:steward`. `docs/checks.md`
describes each source ("Flaky checks", "Budgets and hang limits", the browser
sweep and the nightly).

`/cifix` is an Enact run, shaped like `/bugfix`. The session that runs it is
the 🔮 **vizier** of `/enact-plan` (`~/enact/skills/enact-plan/SKILL.md`). It
plans the run, then hands every bead to worker sessions, and it never fixes or
closes a bead itself. Workers are Enact sessions. A `role:steward` bead runs as
a Steward (`steward.md` in Enact's roles) and every other bead as an executor;
each also follows [Worker: one CI bead](#worker-one-ci-bead) below.

`/enact-plan` cannot be invoked as a skill by a model, so read its `SKILL.md`
(resolve `ENACT` from it as it says) and follow it. The user's `/cifix`
invocation is the invocation `/enact-plan` expects. It authorizes the run's
sessions, and any parallelism, **light** keyword or other instructions given
with `/cifix` pass straight through. The input is the beads chosen in
[Vizier: planning the run](#vizier-planning-the-run), handled as "Several
beads". Everything `/enact-plan` says about pre-flight, permission mode, the
Claude Desktop session pool, registering the run, the loop, titles and the
final report applies unchanged. In Claude Desktop the pool can only be started
in the turn the user invoked you, so do all of the planning below in that turn.

Nothing in this workflow waits for the user.

## Where the evidence is

- The bead: the checks it names (in its description and in its metadata:
  `beryl_flaky_check`, `beryl_nightly_checks`, or for a cut, its checks with
  their runs, catches and CPU seconds), the host, the commit or range, and an
  output tail. Its comments list later runs that failed or flaked again.
- The run log, `$BERYL_CACHE/ci/runs.jsonl`: one JSON line per check per run
  (`runlog.py`). Filter it with `python3` or `jq` by `check`, `commit` and
  `script` (`check`, `ci`, `nightly`, `sweep-run`). A flaky check's failing
  record is followed by a pass of the same check on the same `tree`. Other
  checks that failed in the same run are clues to a shared cause: a loaded
  machine, a full disk, a stuck page slot.
- `just ci-report` sums the run log over 30 days: each check's runs, failures,
  verdicts and timings (`--json` for JSON, `--prune` for what the pruning rule
  would do). A `catch` verdict means the check once caught a real bug.
  `just ci-value` scores each check by bugs caught per CPU second.
- Slow CI's latest log: `.git/enact/ci.log` in the main checkout
  (`enact ci --status` prints its path). It holds only the latest run.
- The sweep's record: `$BERYL_CACHE/sweep/log.jsonl` and `last-output.txt`. The
  nightly's: `$BERYL_CACHE/nightly`.

## Vizier: planning the run

This is `/enact-plan`'s pre-flight. Start with its steps 1 to 3 (title, update
Enact, project setup, permission mode), then:

1. **Choose the beads.** List each label above with
   `bd list --label <label> --status open -n 0`. Leave out beads another
   session or vizier is working (in progress or assigned), and Steward beads
   whose `enact_ci_host` is another machine, since only that machine can
   reproduce them. Name both in your summary. A bead filed by hand with one of
   these labels belongs here too.

2. **Find the shared causes.** A day of CI beads usually has far fewer causes
   than beads. Read each bead and its run-log records, then group beads that
   share a cause. Typical groups:

   - **One break, several reports.** A sweep bead, a nightly bead and a
     Steward bead whose checks fail at the same commit usually share one
     culprit. Fix it once.
   - **Several flaky checks in one feature**, for example every check of the
     pile viewer. Suspect one race in the product or in that feature's test
     helper before suspecting each check.
   - **A step that has no reason to be flaky:** formatters, lint,
     `file-length`, `just --fmt`, pytest on pure functions. A flake there is
     almost always the environment: a timeout under load, files another
     process changes during the run, a temporary path or cache shared between
     worktrees, or a tool that downloads something. The fix goes in the step or
     in `tools/`, and the step stays. Never delete a lint step.
   - **A cut or budget bead and a flaky bead naming the same check.** Taking
     the check out of the run settles both.
   - **One check alone:** a test that depends on timing (a wall-clock
     threshold, a sleep, a first animation frame, a short `until`), on order
     (a hash map's iteration order, two processes writing one file), or on the
     machine (free disk, a port, a browser page slot).

   Read the code only as far as you need to name the suspected cause and where
   it is. Do not edit code, open a worktree or start reproducing.

3. **Write each bead's context packet.** Append to each bead's notes
   (`bd update <bead> --append-notes`) its group, the suspected cause, the
   files and tests involved, the run-log records that show it, and the line
   "Follow the `cifix` skill's 'Worker: one CI bead' section."

4. **Group, route and lane.** The beads of one group share a `lane:<name>` so
   one session finds the cause once and closes each bead against it. Give
   every bead a tier:

   - `tier:opus` for an unknown or shared cause, a suspected race in the
     product, a break in rules or UI code, a group of browser checks, and
     every `ci-cut` and `check-budget` bead (choosing what a replacement test
     must keep is judgment).
   - `tier:sonnet` for a single check whose cause you could already name, such
     as a sleep to replace with a wait on an event.
   - `tier:light` only to delete a check that has no `catch` verdict and that
     nothing else depends on.

   Never use `tier:fable` unless the user asked for it. Then run
   `/enact-plan`'s "Lane and tier pass": two to four beads of one tier on the
   same code share a lane, and a group larger than four becomes two lanes with
   a dependency between them (`bd dep add <dependent> <prerequisite>`). A
   Steward bead is P0 and blocks `master`: give it no dependency on anything
   else, and put it first in its lane.

5. **Run them.** Continue with `/enact-plan` at "A bead", case "Several
   beads": create the run epic (title `CI beads <date>`; its description lists
   the groups, their suspected causes and the tier policy above), reparent
   each bead into it, then "Finish planning" and the loop. Add no Wardens,
   Masons, Sages or Archivist unless the invocation asks.

6. **Report.** When the loop returns `done` (or the run stops), give a short
   outcome for every bead: the cause, and what changed (the product or a step
   fixed; a check replaced, deleted or moved to the nightly), or that it was
   handed back unfinished.

## Worker: one CI bead

Your Enact role files still apply: worktree, `enact push`, the handoff comment
and `enact close` (a Steward also follows `steward.md`). Read the bead's notes
first: the vizier recorded the suspected cause and its group. Then follow the
part for its kind.

### A failure: `role:steward`, `sweep-failure`, `nightly-failure`

These report checks that really failed on staging, so something broke.

1. **Reproduce** with the command the bead gives (its acceptance names it),
   on the culprit commit the bead names and on the staging tip.
2. **Find the cause.** When the product broke, fix the product: a rules engine
   bug also gets an entry in `docs/bugs.md`, and a UI break adds the broken
   state as a fixture or spec moment (AGENTS.md, Checks). When the check is
   wrong and the product is right, fix or replace the check as in "A check to
   change" below. A failure caused by this machine's state (a full disk, a
   stale cache) gets fixed in `tools/` so it cannot recur.
3. **Verify** that the reproduce command passes at the staging tip.

### A check to change: `flaky-check`, `ci-cut`, `check-budget`

These name checks that pass but cost too much: a flake, CPU time in slow CI,
or seconds in the fast check.

1. **Find the check.** When a flaky bead names a whole step (its title says
   "the flaky test in step"), the step reported no checks of its own. Find
   which test or command inside it failed from the evidence above. A
   `check-budget` bead lists each lane's slowest steps.
2. **For a flake, find the cause.** Look for the patterns in planning step 2.
   Try to reproduce it under load: run the check 10 to 20 times beside
   `just test-rust` or a full `just ui-test`. Load is the normal state of this
   machine (20 to 40 is typical), so a check has to pass under it. A flake you
   cannot reproduce may still have a cause you can see in the code.
3. **Choose one of these, in this order,** for each check, and record which and
   why on the bead:
   - **Fix the product** when a flake comes from a race in Beryl itself, such
     as a view that arrives before the state it shows. Players hit it too.
   - **Fix the environment** when a step that should be deterministic flaked
     because of a shared file, a timeout, the disk or the network. Keep the
     step.
   - **Make it cheaper or replace it** when a fast, deterministic test can keep
     most of what the check protects: a Rust unit test, a Node test with no
     browser, a check against a recorded fixture, or a browser check on the
     paused clock that waits for an event instead of a time. Write it, then
     delete the old check. Count work instead of timing it. Keep a timeout only
     to catch a hang, with about ten times the loaded duration. Loosening a
     threshold, adding a retry or raising a budget is not a fix.
   - **Move it.** A `check-budget` step can move from the fast check to slow CI
     (`verdicts.SLOW_ONLY`). A check that once caught a bug (a `catch` verdict)
     can move from slow CI to the nightly (a step in `nightly.STEPS`). A check
     the fast check also runs leaves slow CI alone and stays in the fast check.
   - **Delete it** when it has never caught a bug.

   A browser check you change follows `ui/AGENTS.md`. When you delete one, make
   sure the generic UI checks (overlap, off-screen, reachability, duplicate
   objects, page errors, motion) still cover what it showed, and that no
   fixture or spec moment is left unused. Never cut a check in `budget.KEEP`.
4. **Verify.** Run a replaced or changed check 10 times under the same load as
   step 2 and record the passes. A deleted or moved check needs no runs. For a
   `check-budget` bead, record the step's seconds before and after.

### Closing

Land the change with `enact push`. Then post the handoff comment and run
`enact close <bead> --reason "<cause and action>"` (with `--continue` in a
lane). When one cause closes several beads in your lane, close each with the
same reason and name the commit. Enact's vizier waits for slow CI, so ignore
`watch_ci`.

A bead you cannot finish goes back to the queue. Record what you tried, the
cause you suspect and the next step with `bd note <bead> "..."`, run
`enact release <bead> --reason "..."`, then `bd update <bead> --parent ""` and
`enact sync`. Retitle ⛔ as Enact's role files say for a release, and end your
turn with a summary of where the bead stands.
