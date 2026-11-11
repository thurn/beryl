# Working on Project Beryl

Project Beryl is a research project to create a Magic: The Gathering AI for
Legacy constructed, which can play the format's top decks at the level of a
strong human player. It is a Tauri 2 desktop app for Apple silicon Macs: a Rust
workspace behind a React and TypeScript UI. The repository is public.

The project began targeting cube draft and moved to Legacy on 2026-11-09. Games
are two-player, between fixed tournament decklists, best of three with
sideboards; there is no drafting. V1's card pool is the main decks and
sideboards of the Legacy metagame decks we choose to support, kept as decklists
in `docs/sample-decks/`. Rules support and AI need only handle those cards.
Until those decks are implemented, the testbed is the WU Control mirror from the
cube era: `docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`
(`decks::WU_CONTROL`), with `decks::VINTAGE_WU_CONTROL` before it. Never delete
a card implementation.

The eventual goal of Project Beryl is to support 50,000+ different cards. We
should attempt to write modular, dramatically scalable code.

All rules engine code must contain comments referencing the relevant portion of
the Comprehensive Rules which they are implementing, linking to
https://yawgatog.com/resources/magic-rules/ URLs with selected quotations.

## Layout

- `rust/`: the Cargo workspace. `crates/beryl_app` is the Tauri shell (window,
  IPC commands, `tauri.conf.json`, capabilities, icons). Keep it thin: game
  rules and AI go in their own library crates under `rust/crates/`, which build
  and test without Tauri. `docs/engine-architecture.md` §16 maps the crates;
  `docs/project-50k.md` is the plan for restructuring them to scale.
- `ui/`: the frontend, Vite + React + TypeScript (strict). `src/ipc.ts` holds
  typed wrappers for the Rust commands; call Rust only through it. Its fixture
  transport replays sessions recorded from seeded games or scripted boards
  (`fixtures.json`, `beryl_cli`'s `boards.rs`), which `just ui-shots` captures
  in the states `shots.json` names. `src/generated/protocol.ts` is generated
  from `beryl_session` by ts-rs: a test fails when it is stale, and its header
  says how to regenerate it.
- `data/`: `cards.json`, the card data `just cards` writes from Scryfall
  (printing, artist, per-face text and image URLs); `card-printings.csv`, the
  printings it pins; `oracle-divergences.json`, the scenarios where an oracle is
  wrong, each with its CR citation; `card-names-allowlist.json`, the generic
  files that still name cards and the one-card mechanics still in the engine;
  `save-breaks.json`, the corpus saves no migration carries past a change;
  `ui-conventions.json`, the design language that `just conventions` renders
  into `docs/ui-conventions.md` (edit the data, never the page). The app
  downloads images on demand into its cache (`beryl_app`'s `art.rs`); none are
  committed.
- `docs/ui_mockups/`: generated images of the UI, kept as visual references.
  They disagree with one another in details, so they are not a spec: never tune
  the UI's values to match one image.
- `tools/`: `by`, the Python CLI behind the justfile, including the XMage and
  Forge exporters and their Java harnesses. `just` lists the commands.
- Outside the tree: `~/Library/Caches/beryl` (`$BERYL_CACHE`: warm `rust/target`
  and `ui/node_modules` shared by worktrees, the oracle builds and their last
  report in `oracles/logs/oracle.json`, and the screenshots), and `~/arena`
  (`$BERYL_ARENA`: the indexes of the Arena recordings in `~/Movies/arena`, the
  reference for the UI; `just video`, the `arena-video` skill). Never crawl `~`
  or `/` with `find`.

## Enact

`enact` below means `python3 ~/enact/scripts/enact.py`.

- **Every change goes through a worktree and CI**, whether a human, an ad hoc
  agent session or an Enact run makes it:
  1. From the main checkout, `enact worktree create <short-name>`. Work only in
     the printed path; never edit or commit in the main checkout.
  2. Commit there (see Conventions).
  3. Run `enact push` in the worktree. It rebases onto `origin/staging` (merging
     `master` if someone bypassed staging), runs the fast check, pushes to
     `staging`, starts slow CI in the background and removes the worktree. On
     exit code 2, fix the reported conflict or failing check in the same
     worktree and run it again.
  4. If its output has `watch_ci`, no Enact run is around to repair a slow-CI
     failure: follow it (`enact ci --wait <sha>` until it passes; on failure,
     claim and fix the Steward bead) before finishing.
- **Session titles show status.** Any agent session making a change here titles
  itself `⚒️ <short summary>` when it starts (Claude Desktop:
  `set_session_title` on `self`; Codex: `set_thread_title`), 🔮 while it waits on
  slow CI, 🚒 while it fixes a Steward bead, and `✅ <same summary>` as its last
  step when done, or `⛔` if it stops without finishing.
- Never commit to, or push directly to, `master` or `staging`. `master` advances
  only when slow CI passes on `staging`; a slow-CI failure files a P0 Steward
  bead. `enact ci --status` shows the runner and its log. Keep the main checkout
  clean: Enact fast-forwards its `staging` and `master`.
- The checks are `.enact.toml`'s: `just check` (fast) and `just ci` (slow).
- Beads live in this repo (`bd`, prefix `by`); publish bead changes with
  `enact sync`. Plans, designs and decisions go in beads, not markdown: a bead's
  history is the record.
- File beads freely, in parallel. Merge conflicts are cheap: `enact push`
  rebases, you fix and push again. A filed bead may start within minutes, so put
  what it builds on in `staging` first or hold it back with a dependency.
- Every session pays to learn the code it touches, so name in each bead the
  files, helpers and tests it needs. Give beads that touch the same code one
  `lane:<name>` label: one session works them in turn. Label mechanical beads
  (wiring, renames, boilerplate tests) `tier:light` to run them on Sonnet.
- No plan, bead or skill has a step that waits for the user's approval (plan
  K31). Where his judgement is wanted, proceed on a default and file a
  non-blocking bead labelled `human` (`bd human list`). The one exception is the
  count gate of K1 in `docs/project-50k.md`.
- `/enact <beads|epic|task>` runs work inline in one session;
  `/enact-plan <bead|plan|task>` runs it unattended across sessions. Most work
  uses neither.

## Code structure

- Spell words out; never misspell to dodge a keyword (`static_ability`, not
  `statik`).
- No procedural macros written in this repo; a `macro_rules!` only where a
  `const fn` cannot remove the repetition.
- No source file over 800 lines (`file-length`, held to
  `data/file-length-allowlist.json`).
- No card name in generic code (`card-names`, held to its allowlist); a one-card
  mechanic is a sign the abstraction is missing.
- State a card creates lives in a store, not in a new field on Player, Object or
  State (plan K4).
- Helpers shared by two cards live in the shared library, not in a card file.
- The kernel, rules and card crates take no runtime dependencies; a new
  dependency's `Cargo.toml` comment says what it replaces.
- Forge is a test oracle only: no card data, vocabulary or design comes from its
  scripts or code (plan K16).

## Testing your work

Cheapest first. Agents never open a window: `just dev` refuses to for them.

1. Logic: `just test-rust` runs the workspace's tests. Engine and AI behavior is
   tested here, not through the app. A card's scenarios live in its file;
   `just oracle <card names>` checks them against XMage and Forge, `just fuzz`
   plays random games at length, and `just exhaust` plays every decision
   sequence near the scenarios' end positions.
2. UI: `cd ui && npm run typecheck`. `just ui-shots [names]` screenshots UI
   states headless and prints their paths; `just ui-test` plays the game screen
   through a fixture. Every card has a spec (`/specs/<card>`), generated from
   its definition and scenarios, showing each of its UI moments as a live game
   from both seats. To reproduce, debug or verify a card's behavior in the UI,
   use the `specs` skill: `just specs --describe`, `--film`, `--motion` and
   `--drive` show what each moment does for both players, in text, in pictures,
   as an animation audit and under Playwright.
3. The app: `just smoke` builds the debug app and launches it with no window and
   no Dock icon until the UI reports over IPC that it loaded (about a second
   once built). It fails on a build error or a UI that never loads.
4. The phone apps: `just mobile-setup` installs their toolchains once per
   machine. `just ios-smoke` builds the iOS app and launches it in a headless
   iOS Simulator until the UI loads. `just ios` launches it in a simulator;
   drive that with the iOS Simulator tool. Agents never install to a phone:
   `just ios-device` (signs and installs to the paired iPhone) is the user's,
   `--no-install` only builds. `just android-smoke` and `just android` do the
   same in the Android emulator (headless for agents; drive it with
   `adb shell input` or the WebView's DevTools over `adb forward`).

A human runs `just dev` for the app with hot reload, or `just bundle` for a
release `Beryl.app`.

Benchmarks follow `docs/benchmarking.md`: `just bench-count` for instruction
counts, `just bench` for timing (it refuses a busy machine).

Tournaments are expensive: a game involving an MCTS agent at
`--iterations 300 --trees 4` costs about 180 CPU-seconds, so a 100-game pairing
holds the whole machine for about 20 minutes and a full round robin for over 2
hours. `just tournament`, `just play` and `just review --agent` run at utility
QoS (`taskpolicy -c utility`), so other sessions' checks get the CPU first:
`nice` does not do that on macOS. They slow down while the machine is busy.
Wardens rerun only the pairings that gate acceptance, never reported-only ones.
`just longthink`, the long-think arena, is retired (`docs/ai-strategy.md` §8):
it holds the machine for days, and the AI improves through the ladder's
head-to-head matches instead. Its last run is `docs/longthink-run-003.md`.

## Checks

- `just check` (fast, budget 60 s) runs the lanes whose files changed since the
  branch left `origin/staging`: `repo` (justfile and Markdown formatting),
  `python` (black, pyre, pytest), `rust` (rustfmt, clippy, tests), `ui`
  (prettier, tsc), `names` (`card-names`: no card name in generic code, held to
  the burn-down list `data/card-names-allowlist.json`, whose counts only fall;
  `python -m beryl_tools.checks card-names --shrink` lowers them) and `oracle`
  (each changed card file's scenarios in XMage and Forge, skipped when another
  oracle run holds the oracles past 10 s). For a change to `ui/` or `rust/` it
  also runs `ui-test` and `shots` on the browser checks the change can break, by
  the nightly coverage (`just coverage`), worst failure record first, as many as
  fit; the run log's `chosen` says why each ran, and `cut` marks those left to
  slow CI. It also refuses conflict markers and bad commit subjects.
- `just ci` (slow, budget 300 s; 450 s on Windows) runs every lane plus `app`
  (`just smoke`), `shots` (`just ui-shots`, which fails on any page error),
  `ui-test` (`just ui-test`, which plays the game screen through a fixture),
  `oracle` (`just oracle`, which needs `just oracle-setup` once per machine),
  `counts` (`just count-gate`: the rollout benchmark's instruction count, held
  to the bar in `rust/crates/beryl_bench/count-gate.toml`, whose comments say
  when it may rise) and `mobile` (`cargo check` of the app for the iOS Simulator
  and Android, which needs `just mobile-setup`). A run over budget fails: speed
  up, move or delete the slowest step rather than raise it.
- A UI fix adds the broken state as a fixture or spec moment for the generic
  audits (overlap, off-screen, reachability, duplicate objects, page errors,
  motion), extending an audit for a new class of bug. Write a scripted check in
  `ui/scripts/interactions.mjs` only when no audit can express the property, and
  say why in a comment (`docs/project-50k.md`, K20).
- Budgets and deadlines count load-adjusted seconds: while the load average is
  over the core count, the clock runs slower in proportion, up to 8x. A check
  passes on a busy machine if it would on a quiet one.
- A fresh worktree clones the cached `rust/target` and `ui/node_modules` in
  seconds (copy-on-write); removing it saves them back.

## UI

MTG Arena is the reference for Beryl's flow, motion, timing, table layout and
wording (`docs/project-50k.md`, K22). Before you design or change a prompt, an
animation, an arrow or a label, find how Arena does it with the `arena-video`
skill and match it. A still or a filmstrip settles what prose cannot.

- Beryl keeps its own skin: colors, type, icon drawings and frames, and full
  cards that tap.
- Beryl differs from Arena only where [UI Conventions](docs/ui-conventions.md)
  lists a numbered exception. Check that list before copying Arena and before
  "fixing" a difference from it.
- The user's bug report wins over Arena and over an earlier report. Build what
  it asks, and add, change or remove the exception in the same change.
- What the recordings do not show is built from the motions and prompt shapes
  Beryl already has, never a new look, and listed in UI Conventions under "Not
  seen in Arena".
- Never wait on the user for a UI judgement. Proceed on Arena or the nearest
  existing behavior, and file a bead labelled `human` saying what to look at.

## Workflow

- When launching more than one subagent at a time, please always use Sonnet.
- UI wording is Arena's, exactly. Where Arena shows no label, put half the words
  on screen that you first wrote.
- Never ask the player a question whose answer the next screen could take: open
  the choice itself with a way to decline (a search with "find nothing", not
  "Search?" first). Never give them priority they would only pass, as with their
  own spell alone on the stack. The first rule is a Beryl exception: Arena asks.
- It's fine to file your own beads outside a specified enact role, but please
  offer to fix them inline in that case, don't just throw them onto the backlog.

## Conventions

- Commits: short Conventional Commits, e.g. `feat: mulligan decisions`; subjects
  of 60 characters at most, a body only when the why isn't obvious. The fast
  check enforces it.
- `just fmt` formats everything: black, rustfmt, prettier, mdformat (80
  columns), the justfile. Python is typed (pyre, strict); Rust passes clippy
  (pedantic) with `-D warnings`; TypeScript is strict.
- Scripts are Python in `tools/`, standard library unless a dependency earns its
  place.
- Markdown: agents add none unless explicitly requested. README.md, AGENTS.md
  and `docs/` are all there is; plans, designs and notes go in beads.
- Never commit logs, screenshots or other evidence of a run.
- When you fix a rules engine bug, including one in your own code from the same
  session, add an entry to `docs/bugs.md`: a short description of the bug and
  how a similar class of bug could be prevented in future (e.g. via the type
  system). Link the fixing commit's hash when the bug was not found and fixed in
  the same session.
