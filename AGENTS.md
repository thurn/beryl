# Working on Project Beryl

Project Beryl is a research project to build a Magic: The Gathering AI that
plays as well as a strong human player. It is a Tauri 2 desktop app for Apple
silicon Macs, with a Rust workspace behind a React and TypeScript UI. The
repository is public.

The goal is to support 50,000+ different card definitions, so write modular code
that scales to that size.

Games are two-player, best of three with sideboards, between fixed decklists.
There is no drafting. The card pool is the cards in the decklists kept in
`docs/sample-decks/`, and the rules engine and AI only need to handle those
cards. Until more decks are implemented, we test with the WU Control mirror in
`docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`
(`decks::WU_CONTROL`). Never delete a card implementation.

All rules engine code must have comments that point to the part of the
Comprehensive Rules it implements. Link to
https://yawgatog.com/resources/magic-rules/ and quote the relevant text from the
rules release pinned in `data/rules-version.json`. In a doc comment, cite as a
reference, `[CR 118.12a]`, and define its link at the end of the comment;
`just citations shorten <paths>` rewrites inline links that way.

This file holds the rules that apply to every change. `rust/`, `ui/` and `data/`
each have their own `AGENTS.md` with the reference material for work in that
directory, and `docs/checks.md` describes how the checks work. Read the one for
the area you are changing.

## Writing for readers

This applies to code comments, commit messages, beads and reports, and to this
file.

- Write for a reader who knows Magic, Rust and TypeScript but has not read the
  code you just changed. If a sentence only makes sense after reading the diff,
  rewrite it.
- Start with what someone saw (the symptom), then why it happened, then what you
  changed.
- Refer to things by their identifier in backticks, or describe them in plain
  words. Don't invent metaphors ("dressed", "lifted", "the door"). When you use
  a project term, say briefly what it is the first time.
- Write complete sentences with articles and ordinary verbs, one idea per
  sentence. The "half the words" rule in Workflow is for on-screen UI labels
  only, never for prose.
- Check: would a colleague reading it have to ask "meaning what?" If so, put the
  answer in the text.

For example, instead of:

> The phone dressed the cards the opponent looks at as the pile's top, at a
> depth over it, so the pile showed no card beneath them.

write:

> On the phone layout, cards the opponent was looking at from the top of their
> library were drawn as the library's top card, stacked above the pile, so the
> library looked empty underneath them.

## Layout

- `rust/` is the Cargo workspace (`rust/AGENTS.md`).
  - `crates/beryl_app` is the Tauri shell. Keep it thin. Game rules and AI go in
    their own library crates under `rust/crates/`, which build and test without
    Tauri.
  - Each card is one file, `rust/cards/<crate>/<set>/<card>.rs`. Adding a card
    means adding a file: build scripts generate every list of cards, and each
    card's mana cost, type line and header comment are generated from the card
    store.
- `ui/` is the frontend: Vite, React and TypeScript in strict mode
  (`ui/AGENTS.md`). Always call Rust through the typed wrappers in `src/ipc.ts`.
- `data/` holds data files, each described in `data/AGENTS.md`. The card store,
  `card-store.tsv`, is built by `just cards` and embedded in the app. The UI
  bundles no card data of its own.
- `docs/ui_mockups/`: generated images of the UI, kept as visual references.
  They disagree with each other in details, so they are not a spec. Never tune
  UI values to match one image.
- `tools/`: `by`, the Python CLI behind the justfile. Run `just` to list the
  commands.
- Outside the repository:
  - `~/Library/Caches/beryl` (`$BERYL_CACHE`) holds the shared `rust/target` and
    `ui/node_modules` that worktrees copy from, the oracle builds and their last
    report (`oracles/logs/oracle.json`), and screenshots.
  - `~/arena` (`$BERYL_ARENA`) holds indexes of the MTG Arena gameplay
    recordings in `~/Movies/arena`, which are the reference for the UI. Use
    `just video` or the `arena-video` skill.
  - Never run `find` over `~` or `/`.

## Enact

Enact is the tool that manages worktrees, CI and the task queue. `enact` below
means `python3 ~/enact/scripts/enact.py`.

- **Every change goes through a worktree and CI**, whether it's made by a human,
  an ad hoc agent session or an Enact run:
  1. From the main checkout, run `enact worktree create <short-name>`. Work only
     in the path it prints. Never edit or commit in the main checkout.
  2. Commit there (see Conventions).
  3. Run `enact push` in the worktree. It rebases onto `origin/staging` (and
     merges `master` if someone bypassed staging), runs the fast check, pushes
     to `staging`, starts slow CI in the background and deletes the worktree. If
     it exits with code 2, fix the conflict or failing check it reports in the
     same worktree and run it again.
  4. If its output includes `watch_ci`, there is no Enact run around to fix a
     slow-CI failure, so you must watch it yourself: run `enact ci --wait <sha>`
     until it passes, and if it fails, claim and fix the Steward bead before you
     finish.
- **Session titles show status.** An agent session making a change titles itself
  `⚒️ <short summary>` when it starts (Claude Desktop: `set_session_title` on
  `self`; Codex: `set_thread_title`). It switches to 🔮 while waiting on slow CI,
  🚒 while fixing a Steward bead, `✅ <same summary>` as its last step when done,
  and `⛔` if it stops without finishing.
- Never commit or push directly to `master` or `staging`. `master` only moves
  forward when slow CI passes on `staging`. When slow CI fails, Enact files a P0
  "Steward" bead to get it fixed. `enact ci --status` shows the CI runner and
  its log. Keep the main checkout clean, because Enact fast-forwards its
  `staging` and `master` branches.
- Slow CI runs on two machines: this Mac and Cygnus, a Windows PC. A pass on
  either one moves `master` forward. `enact ci --wait` lists failures on the
  other OS as `other_os_stewards`.
- Beads are the task tracker. They live in this repo (`bd`, prefix `by`);
  publish bead changes with `enact sync`. Plans, designs and decisions go in
  beads, not Markdown, so a bead's history is the record.
- File beads freely and in parallel. Merge conflicts are cheap: `enact push`
  rebases, and you fix and push again. A bead can be picked up within minutes of
  filing, so either land what it depends on in `staging` first or block it with
  a dependency.
- Every session has to spend time learning the code it touches, so each bead
  should name the files, helpers and tests it needs. Label mechanical beads
  (wiring, renames, boilerplate tests) `tier:light` so they run on Sonnet.
- A lane is a group of beads that one session works through in order, so it
  learns their code once. Give two to four beads the same `lane:<name>` label
  when they touch the same code and have the same tier. A lane's session keeps
  the model of its first bead, so a bead with a different tier stays out of the
  lane, as does a design bead large enough to fill a session by itself. Block
  such a bead with a dependency if it edits the lane's code. Never put a lane or
  tier label on an epic, because its children inherit it. `/enact-plan` applies
  the same rules ("Lane and tier pass" in its skill) to beads it is given.
- **No Fable without an explicit ask.** Never label a bead `tier:fable`, and
  never start a session or subagent on a Fable model, unless the user has
  explicitly asked for it. Design work defaults to `tier:opus` and mechanical
  work to `tier:sonnet` (or `tier:light`).
- No plan, bead or skill includes a step that waits for the user's approval.
  Where his judgement might be wanted, go ahead with a sensible default and
  record the choice in the bead you're working on. Never file a bead asking him
  to look at something. The exceptions are the count gate (K1 in
  `docs/engine-architecture.md`) and adding a UI exception (see UI).
- `/enact <beads|epic|task>` runs work inline in one session, and
  `/enact-plan <bead|plan|task>` runs it unattended across several sessions.
  Most work uses neither.

## Code structure

- Spell words out. Never misspell a name to avoid a keyword (`static_ability`,
  not `statik`).
- Don't write procedural macros in this repo. Use `macro_rules!` only where a
  `const fn` can't remove the repetition.
- No source file may be over 800 lines (the `file-length` check, with exceptions
  in `data/file-length-allowlist.json`).
- No card names in generic code (the `card-names` check, with exceptions in its
  allowlist). If a mechanic only exists for one card, that usually means a
  general abstraction is missing.
- Rules code and card code never assume there is exactly one other player (the
  `two-player` check). They ask the game state for a player's opponents, the
  defending players or the next player in turn order (see "Players" in
  `beryl_kernel`'s contract).
- Rules code and card code never access the board directly through `.board()` or
  `board::Board` (the `board-door` check). They change the game state through
  mutator functions (see "Mutators" in `beryl_kernel`'s contract).
- Kernel, rules and card code never ignore a failed push onto a fixed-capacity
  list (the `discarded-push` check). If you can argue the list can't overflow,
  use `Queue::push_bounded`, which checks that claim in debug builds. Otherwise
  propagate the `Full` error, which makes the game switch to the slower `Large`
  state representation, which has room to grow.
- State that a card creates goes in a store, not in a new field on `Player`,
  `Object` or `State` (K4 in `docs/engine-architecture.md`).
- Helpers used by two or more cards go in the shared library, not in a card
  file.
- A card definition implements every clause of its Oracle text, including one
  that no card in the decks makes happen yet. A clause the vocabulary can't say
  is escalated in `data/card-escalations.json`, never left out with a comment.
- It's fine to implement a card that is in no deck only so that scenarios can
  use it (Leonin Scimitar exists to be stolen by Thieving Skydiver). Pick a real
  card, so that XMage and Forge can play the same scenarios.
- The kernel, rules and card crates have no runtime dependencies. When you add a
  dependency elsewhere, its `Cargo.toml` comment says what it replaces.
- Forge is used only as a test oracle and, in `just census`, as a count of which
  mechanics its scripts use. No card data, vocabulary or design may come from
  its scripts or code.

## Testing your work

Use the cheapest test that covers your change. Agents never open a window;
`just dev` refuses to run for them.

1. Logic: `just test-rust` runs the workspace's tests. Test engine and AI
   behavior here, not through the app. `rust/AGENTS.md` lists the card tools:
   the XMage and Forge oracles, the citation check, the fuzzer and
   `just card brief <name>` for starting a new card. To write a card, follow the
   `card-writing` skill.
2. UI: run `cd ui && npm run typecheck`, and `npm test` for the tests that need
   no browser. `ui/AGENTS.md` lists the browser checks. To reproduce, debug or
   check a card's behavior in the UI, use the `specs` skill.
3. The app: `just smoke` builds the debug app, launches it without a window and
   waits until the UI reports that it has loaded.
4. The phone apps: `just ios-smoke` and `just android-smoke` do the same in a
   headless simulator or emulator (`rust/AGENTS.md`). Agents never install onto
   a real phone: `just ios-device` is only for the user.

Benchmarks follow `docs/benchmarking.md`. Tournaments (`just tournament`,
`just play` and `just review --agent`) use the whole machine for minutes to
hours, so read "Tournaments" in `rust/AGENTS.md` before running one.

## Checks

`just check` (fast) and `just ci` (slow) are split into "lanes", one per kind of
file or test. The fast check runs only the lanes whose files have changed.
`docs/checks.md` describes each lane, the time budgets and what Windows skips.

- A UI bug fix adds the broken state as a fixture or spec moment, so the generic
  UI checks (overlap, off-screen, reachability, duplicate objects, page errors,
  motion) cover it. If the bug is a new kind of problem, extend one of those
  checks. Only write a custom scripted check in `ui/scripts/kept.mjs` when none
  of the generic checks can express the property, and explain why in a comment
  (K20 in `docs/engine-architecture.md`).
- The counts in an allowlist under `data/` may only go down.
- A check that runs over its time budget is sped up, moved or deleted. Never
  raise a budget to make it pass.
- A lane killed by its deadline reruns once automatically, so if a push still
  times out, some step is slow or hung. Find it before pushing again.
- Headless browser checks run at a higher priority than any build and share a
  limited number of page slots across all sessions. Run only the checks your
  change needs (`just ui-test <title>`, `just specs <card>`) rather than a full
  sweep.

## UI

MTG Arena is the reference for Beryl's game flow, the order and timing of
events, the table layout and the wording (K22 in `docs/engine-architecture.md`).
Before you design or change a prompt, animation, arrow or label, look up how
Arena does it with the `arena-video` skill. A screenshot or filmstrip answers
questions that a description can't. To make something match Arena as closely as
possible, pixel for pixel, use the `arena-pixel-match` skill.

- Animations follow Arena in what they show and when: what changed, who did it,
  where it went, in what order, and what stays still. No review is required to
  land an animation: `just motion` checks it against its own recorded values,
  and the `motion-review` skill runs only when the user asks for it.
- Beryl keeps its own visual style: colors, fonts, icon artwork, card frames,
  and full-size cards that tap.
- Beryl differs from Arena only where [UI Conventions](docs/ui-conventions.md)
  lists a numbered exception. Check that list before copying Arena, and before
  "fixing" something that differs from it. That page is generated from
  `data/ui-conventions.json` by `just conventions`: edit the JSON, never the
  page.
- The user's bug report takes priority over Arena and over any earlier report.
  Build what it asks for, and change or remove the matching exception in the
  same change.
- Explicitly confirm with the user before adding a numbered exception to UI
  Conventions. Ask in chat, and add it only after a clear yes; never infer one
  from an old report or a design choice nobody asked for.
- Anything the recordings don't show is built from animations and prompt types
  Beryl already has, never a new look, and listed in UI Conventions under "Not
  seen in Arena".
- Never wait on the user for a UI decision. Go ahead based on Arena or the
  closest existing behavior. Never file a bead asking him to look at it.

## Workflow

- When launching more than one subagent at a time, always use Sonnet.
- On-screen UI text uses Arena's wording exactly. Where Arena shows no label,
  cut your first draft of the label to half as many words. (This applies to
  on-screen text only, not to comments or commit messages.)
- Never ask the player a yes/no question when the next screen could carry the
  answer. Open the choice directly, with a way to decline (for example, show the
  search with a "find nothing" option instead of asking "Search?" first). This
  rule is a Beryl exception: Arena asks first. Also never give players priority
  when they would only pass, such as when their own spell is alone on the stack.
- It's fine to file your own beads outside an Enact role, but in that case offer
  to fix them in the current session rather than just adding them to the
  backlog.

## Conventions

- Commits use short Conventional Commits subjects, e.g.
  `feat: mulligan decisions`, of at most 60 characters (the fast check enforces
  this). Add a body for every fix and for any change whose reason a reader
  couldn't guess from the subject. The body follows "Writing for readers": what
  was wrong as someone would have seen it, why, and what changed.
- `just fmt` formats everything: black, rustfmt, prettier, mdformat (80 columns)
  and the justfile. Python is typed (pyre, strict), Rust passes clippy
  (pedantic) with `-D warnings`, and TypeScript is strict.
- Scripts are Python in `tools/`, using only the standard library unless a
  dependency is clearly worth it.
- Agents don't add Markdown files unless explicitly asked. README.md, the
  `AGENTS.md` files and `docs/` are the only Markdown; plans, designs and notes
  go in beads. Each `AGENTS.md` has a one-line `CLAUDE.md` beside it that
  imports it. Keep this root file to rules that apply to every change: reference
  material for one area goes in that directory's `AGENTS.md`, a procedure goes
  in a skill, and a rule that a check already explains when it fails needs one
  line at most.
- Never commit logs, screenshots or other output from a run.
- When you fix a rules engine bug, including one in your own code from the same
  session, add an entry to `docs/bugs.md`: a short description of the bug and
  how similar bugs could be prevented in future (for example, through the type
  system). Link the fixing commit's hash when the bug wasn't found and fixed in
  the same session.
- Do not directly reference planning documents or beads in code (e.g. "plan
  K16"), this gets stale fast. If you touch code with such references, delete
  them.
- Timeouts which fail CI are forbidden. Write tests which deterministically
  fail, delete flaky tests. Infrastructure should file a bead when tests are
  slow, not block. Slow integration tests are strongly discouraged, prefer
  faster testing strategies.
