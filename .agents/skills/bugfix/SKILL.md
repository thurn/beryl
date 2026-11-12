---
name: bugfix
description: Work the bug reports filed from Beryl's in-app bug button (⌘B) — turn them into beads, reproduce each from its screenshot, replay, views and logs, fix it, and mark it done. Use when asked to fix reported bugs, work the bug reports, or run /bugfix.
---

# Fixing reported bugs

The parent handles triage and may fix small items directly. For larger fixes,
create separate Codex tasks in the Beryl project using the Sol model
(`gpt-6.1-sol`). Use the desktop task creation tool for these independently
visible tasks. Do not spawn in-chat subagents for this workflow. Group reports
that share a mechanism to avoid overlapping fixes.

Give each task the original reports or bead IDs, acceptance criteria, relevant
prior findings, and ownership boundaries. Each task uses its own Enact worktree,
lands through CI, and returns the commit and verification evidence per report.
The parent tracks all assigned reports, reviews that evidence, and handles bead
closure and report archival. Use and extend `/specs` as needed.

Nothing in this workflow waits for the user. Where a report leaves a real
question, decide it (step 2) and proceed. Never file a bead asking the user to
look at something.

MTG Arena is the reference for the UI's flow, motion, timing, table layout and
wording (AGENTS.md, UI). Beryl differs from it only where
[UI Conventions](../../../docs/ui-conventions.md) lists a numbered exception.
The user's report wins over Arena and over an earlier report.

A plausible code change is a candidate fix. For rules and UI reports, closure
requires evidence that the player's requested behavior works, including the
scope of the complaint beyond the pictured card. AI reports retain the
blunder-catalog completion criteria below.

## Where reports are

The app's bug button (top right, or ⌘B / Ctrl+B) freezes the screen, lets the
player select the area that shows the bug, and writes a folder under
`~/Library/Application Support/ca.thurn.beryl/bugs` (`$BERYL_BUGS` overrides it;
`beryl_session`'s `bugs.rs` writes it):

- `report.md`: front matter (`kind`: `bug`, or `ai` for a play of the AI's the
  player ticked "AI play" for; `status`: open, filed or done; `bead`;
  the game's `seed`, `turn`, `step`, `decision`; the `commit` the app was built
  from, marked if it had uncommitted changes; the page `url`), then the player's
  words and how to reproduce it.
- `screenshot.png`: the area they selected. `window.png`: the whole window at
  that instant, animations frozen mid-flight and hovers held. Look at both with
  the Read tool.
- `replay.json`: the game's replay as of its latest decision, which reproduces
  it bit for bit.
- `view.json`: the latest view the session emitted.
- `searches.json`: the agent's latest 200 decisions as the game ran them: each
  one's candidates as the UI names them, its choice, its thinking time and its
  search (root visits and mean reward per candidate, rollouts, the decision
  whose trees it reused, an anchored search's check against greedy). An `ai`
  report's `report.md` digests the latest dozen.
- `fixture.json`: the UI's latest views (up to 60) as a UI fixture.
- `ui.json`: the page, and the UI's log: every command and answer, every view as
  it arrived (summarized: status, decision, turn, step, event kinds), routes,
  uncaught errors, rejected promises, console warnings and errors.
- `journal.jsonl`: the app's journal before the report: games started and ended,
  each command the human sent and any refused, each view, panics with
  backtraces. The whole journal is `~/Library/Logs/ca.thurn.beryl/beryl.jsonl`
  (rotated to `.jsonl.1`).

A note filed from a spec page (a moment's 🐞 on `/specs/<card>`, or the bug
button on a moment's full-size page) also names its moment in the front matter's
`spec` (`<card>/<moment>/<yours|theirs>`), and its Reproduce section gives the
`just specs` commands that show it. A 🐞 note has no images or fixture: the spec
reproduces it.

## The loop

1. **File them.** `just bugs` lists open and filed reports. `just bugs file` files a bead for each open one (type bug, label
   `bug-report`, priority 1) holding the report's path and the player's words,
   and writes the bead into the report's front matter. `bd list --label bug-report --all --limit 0`
   includes closed history and work left in progress. Reconcile the local
   reports with the beads and establish the reports in scope for this run; do
   not silently omit blocked, deferred, or interrupted work. Do not take over a
   bead another session is actively working.

2. **Understand each complaint.** Read every report in scope and inspect its
   available images and evidence before choosing an implementation. A report
   from a spec page has a `url` naming the card and moment
   (`/m/<card>/<moment>/…`). Record in each bead:

   - What the player saw, the experience they want, and observable acceptance
     criteria covering every part of their complaint.
   - The intended scope: a named card may illustrate a shared interaction. For
     example, a chooser that opens with the wrong button on one card is a
     complaint about that prompt shape on every card that opens it. Infer scope
     from the player's words and evidence; do not turn a local preference into
     an unrelated redesign.
   - Related open and closed reports and fixing commits. For a repeat report,
     check whether its app build contains the prior fix, and explain what that
     fix or its verification missed before proposing another patch.

   Before delegating implementation or choosing a patch, check each complaint
   for materially different interpretations. For sequencing words such as
   "before", "after", or "when", identify the visible milestones: opening a
   chooser, confirming a choice, paying a cost, and moving cards can be separate
   events. Inspect the recorded views or do a short reproduction during this
   review if needed to distinguish them. For example, "tap before exiling"
   might mean before the exile chooser opens or before selected cards depart;
   do not defer that question until a flight-order patch is already underway.

   Settle each such question yourself, in this order: what the player's words
   and screenshot show; what Arena does at the same moment (the `arena-video`
   skill: a still or filmstrip settles it); UI Conventions; the nearest
   behavior Beryl already has. Take the interpretation that covers the whole
   complaint, not a narrower one that is easier to build, record it and its
   evidence in the bead, and proceed. The user overrules it with a new report
   if he disagrees.

3. **Triage** each UI report by what it is about:

   - **script**: what plays, in what order, from where;
   - **verb**: how a motion feels (its timing, path, easing);
   - **shape**: a prompt (its buttons, defaults, ways to decline);
   - **mark**: what an outline, glow or arrow means;
   - **layout**: where things sit on the table;
   - **skin**: colors, type, icons and frames, which stay Beryl's;
   - **wording**: a label or prompt's text, which is Arena's exactly.

   Rules and AI reports stay as they are. Record the kind on the bead.

   A report about motion, arrows or combat lines (most script, verb and mark
   reports) is fixed where the stage plays it. Read the moment's script first
   (`just specs --script <card>`, or the steps `just ui-shots --views <fixture>`
   lists; the `specs` skill, step 2): a wrong script is a session bug to fix
   in the builder (`rust/crates/beryl_session/src/script/`), and a right
   script played wrongly is the stage's (`ui/src/stage/`). Hold the fix with
   a case of the verb or shape suite (`ui/scripts/verb-cases*.mjs`,
   `shape-cases*.mjs`).

4. **Reproduce.** Pick by what went wrong:

   - An AI play (`kind: ai`, bead label `ai-play`): start from the digest and
     `searches.json`, then review the position and rerun the recorded agent with
     `just review "<folder>/replay.json" --agent recorded --turn N`. A fresh
     sample can reverse a near tie. Judge the play by the information available
     to the agent at the time. Identify a better line and explain what it
     preserves or accomplishes.
     - If you judge it a blunder, write an incident in `docs/incidents/` with
       its replay and add the case to `data/blunders.json`. Verify that
       `just blunders --only <id>` reaches the reported decision. Land the case
       through the worktree workflow, then close and archive the report with
       that commit as in step 6. The ladder owns later changes to the AI.
     - Otherwise, or if the evidence is inconclusive, record your reasoning in
       the bead and return it to the reporter through the human review queue
       below.
   - Rules: `just review "<folder>/replay.json" --turn N` prints the game as a
     transcript to the reported turn; `--agent recorded` adds the opponent's
     searches. If the report's `commit` is not `staging`'s, the replay may
     drift: the transcript says where.
   - The screen: copy `fixture.json` to `ui/public/fixtures/bug-<id>.json`
     (gitignored), start `just ui-live --port 0`, and drive its page there:
     `await page.goto(new URL("/?fixture=bug-<id>#/game", page.url()).href)`,
     then `shot(...)` and `probe`. `&at=-1` stops a view short so the last one
     plays live when you answer. Follow the `ui-review` skill's harness cleanup
     when you finish.
   - A card's UI moment, or a report with a `spec`: use the `specs` skill
     (`just specs --describe`, `--film`, `--motion`, `--drive --port 0`). If no
     moment covers the report, write a scenario that reaches it, which turns it
     into one: extending the specs is part of the fix.
   - Errors: search `journal.jsonl` and `ui.json` for `"level":"error"`,
     `"warn"`, `panic` and `refused`.

5. **Fix and verify** rules and UI bugs through a worktree as AGENTS.md says.
   Claim the bead (`bd update <bead> --status in_progress`) while you work it.
   Identify the shared mechanism responsible for the complaint and fix it at
   that level. Choose representative neighboring cases that could expose the
   same failure, such as both seats, a trigger resolving immediately versus
   stopping for priority, or different destination zones. Record why those cases
   cover the scope; avoid card-specific patches for shared behavior.

   Reproduce the failure before editing and repeat the same interaction after
   the fix. If the original replay drifts or lacks the relevant transition,
   reconstruct it in a scenario or spec and record the limitation. An
   unexplained inability to reproduce is not evidence of a fix.

   Add a regression test that fails before the fix and asserts the player's
   expectation (a scenario, session test, or ui-test). For example, a test that
   a chooser opens does not establish that its default button is Arena's:
   exercise the prompt and check the forbidden behavior. A UI fix also adds the
   broken state as a fixture or spec moment for the generic audits (AGENTS.md,
   Checks). A rules engine bug also gets its `docs/bugs.md` entry.

   When the user's report asks for something Arena does differently, or
   reverses an earlier report, build what it asks, and add, change or remove
   the exception in `data/ui-conventions.json` in the same change (then
   `just conventions` regenerates `docs/ui-conventions.md`). Name the report's
   bead in the entry.

   For UI complaints, inspect the actual interaction headlessly after the fix,
   including the neighboring cases, and put it beside Arena's: film the same
   moment in a recording (the `arena-video` skill) and in Beryl (the `specs`
   skill's `--film`, or `just ui-shots`), and compare what is shown, when, in
   what words and with which buttons. A final screenshot, passing typecheck,
   engine event, or DOM presence alone cannot prove the screen behaves as
   requested. Extend the specs' coverage when necessary.

   Record the reproduction, shared cause, tests and commands, and observed
   result against each acceptance criterion in the bead. Keep remaining
   uncertainty explicit; a partial fix leaves the report open.

6. **Close** it only after the above verification and required CI pass and
   `enact push` has landed the fix, or the AI blunder case meets step 4. Agents
   close their own reports. The evidence for closing a UI report, recorded on
   its bead, is an Arena still or filmstrip beside Beryl's at the same moment,
   or, where Arena has no such moment, a hand-written expectation (the test
   above, or the UI Conventions entry it follows). Then:
   `bd close <bead> --reason "<what was wrong>"`, then
   `just bugs done <bead> --commit <sha>`, which marks the report done and moves
   it to `done/`, then `enact sync`.

If a rules report describes behavior the Comprehensive Rules require, or an AI
report needs the reporter’s judgment, use
`bd note <bead> "<evidence and expected behavior>"`, then
`bd update <bead> --status open --add-label human` and `enact sync`. Leave it in
the `bd human list` queue for the reporter to review and skip it in later bugfix
runs while it awaits a response. Once the reporter confirms, record the
confirmation with `bd human respond <bead> "<confirmation>"`, mark the local
report done with `just bugs done <bead>`, and run `enact sync`. If the reporter
identifies a bug or confirms a blunder, remove the `human` label and resume the
corresponding workflow.

A UI report is never working as intended merely because Arena does the same:
the user's report wins (step 5). A rules or UI report you cannot reproduce stays
filed. Use `bd note <bead> "..."` to record what you tried.

Before ending the run, reconcile its report list with the final bead and local
report states. Give the reporter a concise outcome for every report in scope:
verified fixed (with commit and evidence), cataloged
AI blunder, awaiting human input, unresolved/partially fixed, or untouched (with
reason). For unfinished work, record the remaining acceptance criteria and next
action in the bead. Do not let a successful subset stand in for completing the
run, and do not archive a report merely because a related change landed.
