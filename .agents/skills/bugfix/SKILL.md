---
name: bugfix
description: Work the bug reports filed from Beryl's in-app bug button (⌘B) — turn them into beads, reproduce each from its screenshot, replay, views and logs, fix it, and mark it done. Use when asked to fix reported bugs, work the bug reports, or run /bugfix.
---

# Fixing reported bugs

Please investigate and fix the following issues. Mostly fix these yourself, but
you can run child sessions for the big ones. Please feel free to use and extend
the `/specs` tool to assist with this. Please ask any follow-up questions at the
start of this session needed to clarify expected behavior: read every report
first, then ask them all in one go before you fix anything.

## Where reports are

The app's bug button (top right, or ⌘B / Ctrl+B) freezes the screen, lets the
player select the area that shows the bug, and writes a folder under
`~/Library/Application Support/ca.thurn.beryl/bugs` (`$BERYL_BUGS` overrides it;
`beryl_session`'s `bugs.rs` writes it):

- `report.md`: front matter (`kind`: `bug`, or `ai` for a play of the AI's the
  player ticked "AI play" for; `status`: open, filed or done; `bead`; the game's
  `seed`, `turn`, `step`, `decision`; the `commit` the app was built from,
  marked if it had uncommitted changes; the page `url`), then the player's words
  and how to reproduce it.
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

1. **File them.** `just bugs` lists open and filed reports. `just bugs file`
   files a bead for each open one (type bug, label `bug-report`, priority 1)
   holding the report's path and the player's words, and writes the bead into
   the report's front matter. `bd list --label bug-report --status open` lists
   the beads afterwards.
2. **Read every report** and ask your questions (above). A report from a spec
   page has a `url` naming the card and moment (`/m/<card>/<moment>/…`).
3. **Reproduce.** Pick by what went wrong:
   - An AI play (`kind: ai`, bead label `ai-play`): an incident, as the ones in
     `docs/incidents/` are. Start from the digest and `searches.json`, which are
     the game's own searches, then search the decision again with
     `just review "<folder>/replay.json" --agent recorded --turn N` (slow, at
     low priority: a fresh sample, so a near-tie may go the other way). Copy the
     replay beside the incident, find why the search chose as it did, and fix
     it or file what it would take.
   - Rules or AI: `just review "<folder>/replay.json" --turn N` prints the game
     as a transcript to the reported turn; `--agent recorded` adds the
     opponent's searches. If the report's `commit` is not `staging`'s, the
     replay may drift: the transcript says where.
   - The screen: copy `fixture.json` to `ui/public/fixtures/bug-<id>.json`
     (gitignored), start `just ui-live`, and drive its page there:
     `await page.goto(new URL("/?fixture=bug-<id>#/game", page.url()).href)`,
     then `shot(...)` and `probe`. `&at=-1` stops a view short so the last one
     plays live when you answer.
   - A card's UI moment, or a report with a `spec`: use the `specs` skill (`just specs --describe`,
     `--film`, `--motion`, `--drive`). If no moment covers the report, write a
     scenario that reaches it, which turns it into one: extending the specs is
     part of the fix.
   - Errors: search `journal.jsonl` and `ui.json` for `"level":"error"`,
     `"warn"`, `panic` and `refused`.
4. **Fix** it through a worktree as AGENTS.md says, with a test that fails
   before the fix (a scenario, a session test, a ui-test). A rules engine bug
   also gets its `docs/bugs.md` entry. Claim the bead
   (`bd update <bead> --status in_progress`) while you work it.
5. **Close** it once `enact push` has landed the fix:
   `bd close <bead> --reason "<what was wrong>"`, then
   `just bugs done <bead> --commit <sha>`, which marks the report done and moves
   it to `done/`, then `enact sync`.

If the reported behavior appears to be working as intended, use
`bd note <bead> "<evidence and expected behavior>"`, then
`bd update <bead> --status open --add-label human` and `enact sync`. Leave it in
the `bd human list` queue for the reporter to review and skip it in later
bugfix runs while it awaits a response. Once the reporter confirms, record
the confirmation with `bd human respond <bead> "<confirmation>"`, mark the local
report done with `just bugs done <bead>`, and run `enact sync`. If the reporter
identifies a bug, remove the `human` label and resume the fix.

A report you cannot reproduce, or whose expected behavior is still unclear,
stays filed. Use `bd note <bead> "..."` to record what you tried.
