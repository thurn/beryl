---
name: bugfix
description: Triage existing Beryl bug-report epics in Ignite. Use for /bugfix or requests to clean up, deduplicate, plan, and activate in-app bug reports already filed under one or more epics.
---

# Triage bug-report epics

`/bugfix [epic IDs]` prepares existing report epics and releases them by
setting each parked epic's Ignite allocation to 1. The epic controls dispatch
for its reports. Leave individual ticket allocations alone. The triage pass
reads the evidence, groups work, and writes the verification plan. Workers
reproduce, fix, verify, and close reports.

## Read the reports

Use the named epics. With no IDs, list Ignite's open and pending epics and
select the parked bug-report epics. the client creates one per app launch with a
`bug-report-run:<run id>` label. Start with `ignite ticket children <epic>` and
`ignite ticket show <ids...> --comments`. Read the graph for existing
dependencies and find relevant open and closed reports and fixes. Compare
candidate fixes to the reported build and to current staging.

Read the player's full words, the screenshot and surrounding window, and the
steps or recorded moment for each report. The ticket points to `report.md` and
its evidence under `$BERYL_BUGS` (default
`~/Library/Application Support/ca.thurn.beryl/bugs` on macOS,
`%APPDATA%\ca.thurn.beryl\bugs` on Windows). If the local folder is
unavailable, `ignite ticket attachment get <id> screenshot.png` downloads the
attached crop. Use `window.png` for context. A spec report names its card,
moment, and seat instead. `just client bug <id>` reads a local the client capture and
prints the recorded state and applicable reproduction commands. Read targeted
code or history where it helps identify a shared mechanism or hand a worker a
useful starting path. Record gaps in the evidence and how the worker can
reconstruct the case.

Account for every report. Preserve existing claims, deferrals, blockers, and
human holds. Do this pass directly, with no separate investigations to triage
each report.

## Turn reports into work

- **Group by the likely shared mechanism.** Pick one existing lead when a
  worker can test a plausible shared cause across cases. For example, missing
  letters in a turn banner, a prompt, and a library header warrant one pass
  through glyph preparation and rendering before anyone changes individual
  widgets. Record the hypothesis and the first comparison that would
  distinguish it from separate causes. Keep independently diagnosable faults
  separate, such as missing glyphs and two cards' intact badges overlapping.
- **Keep every report traceable.** Retain the original ticket, wording,
  attachments, and local report ID. Put the follower IDs and their distinct
  expectations on the lead. Put the lead ID on each follower. If a newer
  request replaces an earlier remedy, check that the new remedy resolves the
  earlier symptom and record which instruction governs the fix. Use a
  dependency for each follower and for other work that truly requires a
  predecessor's result.
- **Make each lead usable.** Add the observed symptom, affected client and
  build, reproduction data and interaction, related fixes, relevant code
  paths and tests, the first diagnostic step, and any constraints. Put
  hypotheses in notes as hypotheses. Give each lead an effort estimate and
  `tier:high` for investigation or design, or `tier:light` for a scoped
  mechanical change. Carry a tier and estimate onto a follower if it is later
  released as independent work.
- **Write acceptance before release.** For each original report, state the
  visible or behavioral outcome, how to reach the original failure, what
  evidence shows it passed, and a neighboring case for shared behavior.
  Require before/after verification and a regression guard that exercises the
  failure. Describe the observable result if the worker still needs to choose
  a command or write a fixture. For UI, capture the rendered frame or
  interaction at the failing seat, layout, and scale. Use a film for motion,
  before/after bounds for unwanted movement, or a rendered pixel check for
  missing glyphs. A text tree cannot show whether a glyph reached the screen.
  Capture the reference the report calls for and compare the affected behavior.
  Follow [UI Conventions](../../../docs/ui-conventions.md) and the root
  `AGENTS.md` for Arena, Beryl's visual style, and UI regression coverage.
- **Give the epic an acceptance criterion.** It should be possible to audit the
  final result by following each original report to its recorded evidence and
  landing. Link each lead to [the worker handoff](#worker-fixing-one-report).

## Release the prepared work

1. Use a version-checked `ignite ticket batch` to update the plan. Put
   `{"id":"<ticket>","version":123}` entries in the batch's `if` array for
   tickets whose state the edits depend on. Re-read and reconcile a conflict.
   Preserve the report bodies and append triage context to notes. Keep pending
   tickets pending during this update.
2. Run `ignite plan check <epic>`. Resolve unintended blockers and check that
   every runnable lead has an estimate. Include follower verification in the
   lead's estimate. Verify that each follower depends on a lead that can take
   the work.
3. Re-read the epic and its descendants. Release a parked epic with a
   version-checked batch whose only update sets the epic's `allocation` to
   `1`. Ignite opens pending descendants when it activates the epic. If this
   would lift an explicit approval hold, leave the epic parked and report the
   hold. Read back the epic and descendants to confirm readiness and report
   any existing ticket overrides that still prevent dispatch.

## Worker: fixing one report

Read the lead's acceptance and each linked report before editing. Reproduce
each case on the current tree, or record why the capture no longer replays and
reconstruct the failure in a deterministic case. If a specific landed commit
already resolves a report, verify the original expectation on a build
containing it and record the evidence and commit.

- For the client use [client](../client/SKILL.md), including native rendered captures for
  visual claims. A live capture may require replaying from before the reported
  decision to see a transient event. For web UI use the report's fixture or
  [specs](../specs/SKILL.md), and use [arena-video](../arena-video/SKILL.md)
  where Arena is the reference.
- For a wrong arrow, combat line, or animation, inspect the script before
  changing the client. A wrong script comes from
  `engine/crates/beryl_session/src/script/`. A correct script played incorrectly
  belongs to the client.
- For rules or AI use the saved replay with `just review`. For an AI report,
  check `searches.json` and judge the line using the information the agent had.
  A confirmed blunder is complete when its incident and replay are cataloged
  in `docs/incidents/` and `data/blunders.json` and the blunder check reaches
  the reported decision.

Record the reproduction, regression check, and observed pass for each
report. Land the fix through Ignite. Close each verified follower with
`ignite ticket close <id> --reason "<evidence and landing SHA>"` and archive
verified local reports with `just bugs done <ticket> --commit <sha>`. If a
follower needs independent work, record what the lead covered, update the
remaining acceptance, and give it a tier and estimate before landing. The
follower's dependency clears when the lead lands. For a case with inconclusive
evidence, record the gap and the next reproduction step. Keep the report filed
until its own criteria are verified.
