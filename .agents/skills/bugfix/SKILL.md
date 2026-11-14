---
name: bugfix
description: Work the bug reports filed from Beryl's in-app bug button (⌘B) as an Enact run — file them as beads, group reports that share a cause into lanes and give each a model tier, then run them through /enact-plan's vizier loop, where worker sessions reproduce, fix, verify and close each report. Use when asked to fix reported bugs, work the bug reports, or run /bugfix.
---

# Fixing reported bugs

`/bugfix` is an Enact run. The session that runs it is the 🔮 **vizier** of
`/enact-plan` (`~/enact/skills/enact-plan/SKILL.md`): it plans the run, then
hands every report to worker sessions and never fixes, reproduces or closes a
report itself. Workers are Enact executor sessions; each one follows
[Worker: fixing one report](#worker-fixing-one-report) below for its bead.

`/enact-plan` cannot be invoked as a skill by a model, so read its
`SKILL.md` (resolve `ENACT` from it as it says) and follow it. The user's
`/bugfix` invocation is the invocation `/enact-plan` expects: it authorizes the run's sessions, and any parallelism, **light**
keyword or other instructions given with `/bugfix` pass straight through. Its
input is the bug-report beads chosen in
[Vizier: planning the run](#vizier-planning-the-run), handled as "Several
beads". Everything `/enact-plan` says about pre-flight, permission mode, the
Claude Desktop session pool, registering the run, the loop, titles and the
final report applies unchanged; this skill only adds how to choose and prepare
the beads. In Claude Desktop the pool can only be started in the turn the user
invoked you, so do all of the planning below in that turn.

Nothing in this workflow waits for the user. Where a report leaves a real
question, the worker decides it (Worker step 1) and proceeds. Never file a
bead asking the user to look at something.

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

Phone reports stay on the phone until `just mobile-bugs` pulls them into this folder.
A shared zip is taken with `just mobile-bugs <zip>`.

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

## Vizier: planning the run

This is `/enact-plan`'s pre-flight. Start with its steps 1 to 3 (title, update
Enact, project setup, permission mode), then:

1. **File them.** `just bugs` lists open and filed reports. `just bugs file` files a bead for each open one (type bug, label
   `bug-report`, priority 1) holding the report's path and the player's words,
   and writes the bead into the report's front matter. `bd list --label bug-report --all --limit 0`
   includes closed history and work left in progress. Reconcile the local
   reports with the beads and establish the reports in scope for this run; do
   not silently omit blocked, deferred, or interrupted work. Leave out beads
   labeled `human` (they wait on the reporter, below) and beads another
   session or vizier is actively working; name both in your summary.

2. **Read each report, briefly.** Read every report in scope: its words and
   its screenshot. This reading is for routing, not for deciding what the
   player wants; the worker does that with the evidence in front of it
   (Worker step 1). Do not read code, open a worktree, reproduce, or run
   subagents to interpret reports. Note on each bead only facts you checked:

   - related open and closed reports and the commits that fixed them, found by
     searching `bd list --label bug-report --all --limit 0` and `git log`;
   - for a repeat report, whether its app build (`commit` in the front matter)
     already contains the earlier fix;
   - the line "Follow the `bugfix` skill's 'Worker: fixing one report'
     section."

   Never write a guess as a fact or a default. In particular, do not describe
   how an earlier build or Arena showed something unless you have looked at
   it; the worker finds that out.

3. **Group and route.** Several reports with one cause are one fix: give them
   the same `lane:<name>` so one session fixes the cause once and verifies and
   closes each report against it. Group only on a cause you can see from the
   reports themselves (the same prompt misbehaving the same way, one animation
   reported twice), not because two fixes might touch the same files or
   directory: merge conflicts are cheap, and a lane costs parallelism. Do not
   run `/enact-plan`'s "Lane and tier pass" over these beads. Add an edge
   (`bd dep add <dependent> <prerequisite>`) only where one fix changes how
   another report must be judged.

   Then give every bead a tier, since Beryl assigns models by name
   (`/enact-plan`, "Models by name"; AGENTS.md, Enact):

   - `tier:opus` by default: reports whose cause is not obvious, rules engine
     behavior, motion, prompt shapes, repeat reports, and every `ai-play`
     report (judging a blunder is design work).
   - `tier:sonnet` only where the report itself names a small, local change: a
     wording change, a color, a misaligned element.

   Never use `tier:fable` unless the user asked for it.

4. **Run them.** Continue with `/enact-plan` at "A bead", case "Several
   beads": create the run epic (title `Bug reports <date>`; its description
   lists the reports in scope and the tier policy above), reparent each bead into it, then "Finish planning" and the loop.
   A bug-report bead that already sits under the open epic of an earlier
   `/bugfix` run with claimed children belongs to that run's vizier: leave
   it. Under any other parent, move it into the new epic and note the old
   parent on the bead. Add no Wardens, Masons, Sages or Archivist unless
   the invocation asks; each worker's verification below is the review.
   From here on you are the vizier in `/enact-plan`'s loop and nothing
   else: you never read a bead's progress, review a fix, or close a report.

5. **Report.** When the loop returns `done` (or the run stops), reconcile
   the reports in scope with `just bugs` and the final bead states, and give
   the reporter a concise outcome for every report: verified fixed (with
   commit), cataloged AI blunder, awaiting the reporter (`human`), handed
   back unfinished, or left out (with the reason). Do not let a successful
   subset stand in for completing the run.

## Worker: fixing one report

You are an Enact executor session working one bug-report bead (or a lane of
them). Your role files from Enact still apply: worktree, `enact push`,
handoff comment and `enact close`. This section says what finishing a bug
report means. The bead holds the report and related history, not an
interpretation: deciding what the player is asking for is your first job
(step 1).

1. **Decide what the player wants, then reproduce it.** Read the report's
   words, `screenshot.png` and `window.png` before anything else, and decide
   what the player saw and what they want. Write that down on the bead with
   observable acceptance criteria covering every part of the complaint.

   - **Scope.** A named card often stands for a shared interaction: a chooser
     that opens with the wrong button on one card is a complaint about that
     prompt shape on every card that opens it. Do not turn a local preference
     into an unrelated redesign either.
   - **Sequencing words** such as "before", "after" or "when" can point at
     different visible moments: a chooser opening, a choice confirmed, a cost
     paid, cards moving. Check the recorded views to tell which one the player
     means before you choose a fix.
   - **Settle each open question in this order:** the player's words and
     screenshot; what Arena does at the same moment (for a UI report, the
     `arena-pixel-match` film of step 2, started now: see Arena before you
     design); UI Conventions; the nearest behavior Beryl already has. Take the
     reading that covers the whole complaint, not a narrower one that is
     easier to build.
   - **"It used to be better."** Find out what the earlier build actually
     showed at that moment (`git log`, `git show <commit>^:<path>`, an old
     spec film) before you restore anything. Never assume which component it
     was.
   - **A repeat report:** say what the earlier fix or its verification missed
     before you write another patch.
   - **A motion, arrow or combat-line report:** read the moment's script first
     (`just specs --script <card>`, or the steps
     `just ui-shots --views <fixture>` lists; the `specs` skill, step 2). A
     wrong script is a session bug in the builder
     (`rust/crates/beryl_session/src/script/`); a right script played wrongly
     is the stage's (`ui/src/stage/`). Hold the fix with a case of the verb or
     shape suite (`ui/scripts/verb-cases*.mjs`, `shape-cases*.mjs`).

   Then reproduce it. Pick by what went wrong:

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
       that commit as in step 3. The ladder owns later changes to the AI.
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

2. **Fix and verify** rules and UI bugs through a worktree as AGENTS.md says.
   Enact has already claimed the bead for you.
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

   **Every UI report runs `/arena-pixel-match`. This is not optional, however
   small the change looks.** Read the skill and follow it for the reported
   moment and the neighboring cases: film Arena first, while deciding the fix
   (step 1), and film again after it. It films the same moment in an Arena recording and
   in Beryl side by side (through the `arena-video` and `specs` skills), lists
   every difference in what is shown, where, how large and when, and has you
   fix them until none is left or each one left is named. The player's report
   and UI Conventions override Arena there, as that skill says. A final
   screenshot, passing typecheck, engine event, or DOM presence alone cannot
   prove the screen behaves as requested. Extend the specs' coverage when
   necessary. Only when Arena has no such moment does the hand-written
   expectation below stand in for the side-by-side film, and "no such moment"
   needs evidence: record on the bead the `arena-video` searches you ran and
   what they found.

   Record the reproduction, shared cause, tests and commands, and observed
   result against each acceptance criterion in the bead. Keep remaining
   uncertainty explicit; a partial fix leaves the report open.

3. **Close** it only after the above verification and required CI pass and
   `enact push` has landed the fix, or the AI blunder case meets step 1. You
   close your own reports; the vizier never does. A UI report cannot close
   without this evidence on its bead: the path of the final `arena-pixel-match`
   pair sheet of Arena and Beryl at the same moment, and its difference table
   (each row fixed, or named with its reason). Where Arena has no such moment,
   the bead instead records the `arena-video` searches that found none and the
   hand-written expectation that stands in (the test above, or the UI
   Conventions entry it follows). Without one of these, hand the report back
   as "only partly fixed" below. Then run
   `just bugs done <bead> --commit <sha>` with the `staging` SHA that
   `enact push` reported, which marks the report done and moves it to
   `done/`, and finish as Enact's role files say: the handoff comment, then
   `enact close <bead> --reason "<what was wrong>"` (with `--continue` in a
   lane). Enact's vizier waits for slow CI, so ignore `watch_ci`.

Some reports end without a fix. Hand such a bead back so the run can finish
without it, and so a later `/bugfix` run sees it:

- **Awaiting the reporter.** A rules report describes behavior the
  Comprehensive Rules require, or an AI report needs the reporter's judgment
  (step 1). Record the evidence and expected behavior with
  `bd note <bead> "..."`, run `enact release <bead> --reason "awaiting
  reporter"`, then `bd update <bead> --parent "" --add-label human` and
  `enact sync`. The bead waits in the `bd human list` queue, and `/bugfix`
  leaves it out while it does. Once the reporter confirms, whoever handles
  it records that with `bd human respond <bead> "<confirmation>"`, runs
  `just bugs done <bead>` and `enact sync`. If the reporter identifies a bug
  or confirms a blunder, remove the `human` label so the next `/bugfix` run
  takes it up again.
- **Not reproduced or only partly fixed.** A UI report is never working as
  intended merely because Arena does the same: the user's report wins
  (step 2). A rules or UI report you cannot reproduce stays filed. Record
  what you tried, the acceptance criteria still unmet and the next action
  with `bd note <bead> "..."`, run `enact release <bead> --reason "..."`,
  then `bd update <bead> --parent ""` and `enact sync`. Push any partial fix
  that is correct on its own, but never run `just bugs done` for it, and
  never archive a report merely because a related change landed.

After a hand-back, retitle ⛔ as Enact's role files say for a release and
end your turn with a summary of where the report stands.
