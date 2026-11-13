---
name: flakefix
description: Work the open flaky-check beads (checks that failed and then passed on a rerun of the same tree) as an Enact run — group them by shared cause, route them into lanes and model tiers, then run them through /enact-plan's vizier loop, where worker sessions find each flake's cause and fix the product race, replace the check with a deterministic test, delete it, or move it to the nightly. Use when asked to work, fix or clear the flaky checks or flaky tests, or to run /flakefix.
---

# Fixing flaky checks

A check is flaky when it failed and then passed on its rerun on the same tree.
Its lane still passes, but `run_lane` in `tools/src/beryl_tools/checks.py`
prints a `flaky: <lane> <check>` line, and `tools/src/beryl_tools/flaky.py`
files a P1 bead labeled `flaky-check` for it. The bead's `beryl_flaky_check`
metadata names the check. Each check has at most one open bead, and a later
flake of the same check adds a comment to it. There is no quarantine (see
"Flaky checks" in `docs/checks.md`), so a flaky check keeps running in CI until
a session deals with it. `just beads` leaves these beads out.

`/flakefix` is an Enact run, shaped like `/bugfix`. The session that runs it is
the 🔮 **vizier** of `/enact-plan` (`~/enact/skills/enact-plan/SKILL.md`). It
plans the run, then hands every bead to worker sessions, and it never fixes or
closes a bead itself. Workers are Enact executor sessions. Each one follows
[Worker: one flaky check](#worker-one-flaky-check) below.

`/enact-plan` cannot be invoked as a skill by a model, so read its `SKILL.md`
(resolve `ENACT` from it as it says) and follow it. The user's `/flakefix`
invocation is the invocation `/enact-plan` expects. It authorizes the run's
sessions, and any parallelism, **light** keyword or other instructions given
with `/flakefix` pass straight through. The input is the beads chosen in
[Vizier: planning the run](#vizier-planning-the-run), handled as "Several
beads". Everything `/enact-plan` says about pre-flight, permission mode, the
Claude Desktop session pool, registering the run, the loop, titles and the
final report applies unchanged. In Claude Desktop the pool can only be started
in the turn the user invoked you, so do all of the planning below in that turn.

Nothing in this workflow waits for the user.

## Where the evidence is

- The bead: which check, which lane, which script (`by check` is the fast
  check, `by ci` is slow CI), which host and which commit. Its comments list
  each later flake of the same check.
- The run log, `$BERYL_CACHE/ci/runs.jsonl`: one JSON line per check per run
  (`runlog.py`). The failing run's records have `"outcome": "fail"` followed by
  a pass of the same check with the same `tree`. Filter it with `python3` or
  `jq` by `check` and `commit`. Other checks that failed in that run are clues
  to a shared cause such as a loaded machine, a full disk or a stuck page slot.
- `just ci-report` sums the run log over 30 days: each check's runs, failures
  and verdicts. `just ci-report --json` gives the same as JSON. A `catch`
  verdict on a check means it once caught a real bug. Each check's verdicts
  are in a bead named in the flaky bead's description (`by-ci-…`).
- Slow CI's latest log: `.git/enact/ci.log` in the main checkout
  (`enact ci --status` prints its path). It holds only the latest run, so the
  failing output may already be gone. The Steward bead of a failed slow-CI run
  quotes a log tail.
- `docs/checks.md` describes each lane, its steps and its hang limits.

## Vizier: planning the run

This is `/enact-plan`'s pre-flight. Start with its steps 1 to 3 (title, update
Enact, project setup, permission mode), then:

1. **Choose the beads.** `bd list --label flaky-check --status open -n 0`
   lists them. Leave out beads another session or vizier is working, and name
   them in your summary. A flaky check whose bead was filed by hand (with no
   `beryl_flaky_check` metadata) belongs here too, if it carries the label.

2. **Find the shared causes.** Many flakes in one day usually have far fewer
   causes than beads. For each bead, read its run-log records and the other
   checks that failed in the same run. Group the beads that share a cause.
   Typical groups:

   - **Several checks in one feature:** for example, every check of the pile
     viewer. Suspect one race in the product or in that feature's test helper
     before suspecting each check.
   - **A step that has no reason to be flaky:** formatters, lint, the
     `file-length` check, `just --fmt`, pytest on pure functions. A flake there
     is almost always the environment: a timeout under load, files that another
     process changes during the run, a temporary path or cache shared between
     worktrees, or a tool that downloads something. The fix goes in the step
     or in `tools/`, and the step stays. Never delete a lint step.
   - **One check alone:** a test that depends on timing (a wall-clock
     threshold, a sleep, a first animation frame, a short `until`), on order
     (a hash map's iteration order, two processes writing one file), or on
     the machine (free disk, a port, a browser page slot).

   Read the code only as far as you need to name the suspected cause and
   where it is. Do not edit code, open a worktree or start reproducing.

3. **Write each bead's context packet.** Append to each bead's notes
   (`bd update <bead> --append-notes`) the group it belongs to and the
   suspected cause, the files and tests involved, the run-log records that
   show the flake, and the line "Follow the `flakefix` skill's 'Worker: one
   flaky check' section."

4. **Group, route and lane.** The beads of one group share a `lane:<name>`
   so one session finds the cause once and closes each bead against it. Give
   every bead a tier:

   - `tier:opus` for an unknown or shared cause, a suspected race in the
     product, and any group of browser checks.
   - `tier:sonnet` for a single check whose cause you could already name,
     such as a sleep to replace with a wait on an event.
   - `tier:light` only to delete a check that has no `catch` verdict and that
     nothing else depends on.

   Never use `tier:fable` unless the user asked for it. Then run
   `/enact-plan`'s "Lane and tier pass": two to four beads of one tier on the
   same code share a lane, and a group larger than four becomes two lanes
   with a dependency between them (`bd dep add <dependent> <prerequisite>`).

5. **Run them.** Continue with `/enact-plan` at "A bead", case "Several
   beads": create the run epic (title `Flaky checks <date>`; its description
   lists the groups, their suspected causes and the tier policy above),
   reparent each bead into it, then "Finish planning" and the loop. Add no
   Wardens, Masons, Sages or Archivist unless the invocation asks.

6. **Report.** When the loop returns `done` (or the run stops), give a short
   outcome for every bead: the cause, and whether the product was fixed, the
   check was replaced, deleted or moved to the nightly, or the bead was handed
   back unfinished.

## Worker: one flaky check

You are an Enact executor session working one `flaky-check` bead or a lane of
them. Your role files from Enact still apply: worktree, `enact push`, the
handoff comment and `enact close`. Read the bead's notes first: the vizier
recorded the suspected cause and its group.

1. **Find the failing test.** When the bead names a whole step (its title says
   "the flaky test in step"), the step reported no checks of its own. Find
   which test or command inside it failed from the evidence above.

2. **Find the cause.** Read the check and the code it exercises, and look for
   the patterns in planning step 2. Try to reproduce the flake under load:
   run the check 10 to 20 times beside `just test-rust` or a full
   `just ui-test`. Load is the normal state of this machine (20 to 40 is
   typical), so a check has to pass under it. A flake you cannot reproduce
   may still have a cause you can see in the code. If you can neither
   reproduce it nor see a cause, record that and go to step 3's last case.

3. **Choose one of these, in this order,** and record which and why on the
   bead:

   - **Fix the product.** When the flake comes from a race in Beryl itself,
     such as a view that arrives before the state it shows, fix the race.
     Players hit it too. A rules engine race also gets an entry in
     `docs/bugs.md`.
   - **Fix the environment.** When a step that should be deterministic
     flaked because of a shared file, a timeout, the disk or the network,
     fix the step or the tool in `tools/`. Keep the step.
   - **Replace the check.** When a fast, deterministic test can keep most of
     what the check protects (a Rust unit test, a Node test with no browser,
     a check against a recorded fixture, or a browser check on the paused
     clock that waits for an event instead of a time), write it and delete
     the flaky check. Count work instead of timing it. Keep a timeout only to
     catch a hang, with about ten times the loaded duration. Loosening a
     threshold or adding a retry is not a fix.
   - **Delete the check** when it has never caught a bug (no `catch` verdict
     in `just ci-report`), or **move it to the nightly** when it has.

   A browser check you change follows `ui/AGENTS.md`. When you delete one,
   make sure the generic UI checks (overlap, off-screen, reachability,
   duplicate objects, page errors, motion) still cover what it showed, and
   that no fixture or spec moment is left unused.

4. **Verify.** Run a replaced or changed check 10 times under the same load
   as step 2 and record the passes. A deleted check needs no runs. Run the
   lane's other checks once.

5. **Close.** Land the change with `enact push`. Then post the handoff
   comment and run `enact close <bead> --reason "<cause and action>"` (with
   `--continue` in a lane). When one cause closes several beads in your lane,
   close each with the same reason and name the commit. Enact's vizier waits
   for slow CI, so ignore `watch_ci`.

A bead you cannot finish goes back to the queue. Record what you tried, the
cause you suspect and the next step with `bd note <bead> "..."`, run
`enact release <bead> --reason "..."`, then `bd update <bead> --parent ""`
and `enact sync`. Retitle ⛔ as Enact's role files say for a release, and end
your turn with a summary of where the bead stands.
