# Working on Project Beryl

Project Beryl is a research project to create a Magic: The Gathering AI for cube
draft, which can draft cards and play the game at the level of a strong human
player. It is a Tauri 2 desktop app for Apple silicon Macs: a Rust workspace
behind a React and TypeScript UI. The repository is public.

V1 starts with the 540 cards of the MTG Arena Powered Cube run that began
October 30, 2025, listed in `data/arena-powered-cube-2025-10-30.csv` (the card
columns of that run's 17lands public data). Six carry their Arena names from
Through the Omenpaths (Ademi of the Silkchutes is Scryfall's Spectacular
Spider-Man). Rules support, drafting and AI need only handle these cards. Three
implemented cards are outside it (Aang's Iceberg, Swift Reconfiguration and
Sphinx of False Conclusions, played by `decks::VINTAGE_WU_CONTROL`); never
delete a card implementation. The mirror match target is
`docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`, a mythic
drafter's deck from that run.

The eventual goal of Project Beryl is to support 50,000+ different cards. We
should attempt to write modular, dramatically scalable code.

All rules engine code must contain comments referencing the relevant portion of
the Comprehensive Rules which they are implementing, linking to
https://yawgatog.com/resources/magic-rules/ URLs with selected quotations.

## Layout

- `rust/`: the Cargo workspace. `crates/beryl_app` is the Tauri shell (window,
  IPC commands, `tauri.conf.json`, capabilities, icons). Keep it thin: game
  rules, drafting and AI go in their own library crates under `rust/crates/`,
  which build and test without Tauri. `docs/engine-architecture.md` §16 maps the
  crates.
- `ui/`: the frontend, Vite + React + TypeScript (strict). `src/ipc.ts` holds
  typed wrappers for the Rust commands; call Rust only through it. Its fixture
  transport replays sessions recorded from seeded games or scripted boards
  (`fixtures.json`, `beryl_cli`'s `boards.rs`), which `just ui-shots` captures
  in the states `shots.json` names. `src/generated/protocol.ts` is generated
  from `beryl_session` by ts-rs: a test fails when it is stale, and its header
  says how to regenerate it.
- `data/`: the V1 cube cardlist (`color,name` CSV); `cards.json`, the card data
  `just cards` writes from Scryfall (printing, artist, per-face text and image
  URLs); `card-printings.csv`, the printings it pins; `oracle-divergences.json`,
  the scenarios where an oracle is wrong, each with its CR citation. The app
  downloads images on demand into its cache (`beryl_app`'s `art.rs`); none are
  committed.
- `docs/ui_mockups/`: generated images of the UI, kept as visual references.
  They disagree with one another in details, so they are not a spec: never tune
  the UI's values to match one image.
- `tools/`: `by`, the Python CLI behind the justfile, including the XMage and
  Forge exporters and their Java harnesses. `just` lists the commands.
- Outside the tree: `~/Library/Caches/beryl` (`$BERYL_CACHE`: warm `rust/target`
  and `ui/node_modules` shared by worktrees, the oracle builds and their last
  report in `oracles/logs/oracle.json`, and the screenshots), and
  `~/17lands/Cube_-_Powered` (17lands' draft, game and replay data for the cube,
  PremierDraft and TradDraft, as gzipped CSVs). Never crawl `~` or `/` with
  `find`.

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
- `/enact <bead|epic|task>` runs work inline in one session;
  `/enact-plan <bead|plan|task>` runs it unattended across sessions. Most work
  uses neither.

## Testing your work

Cheapest first. Agents never open a window: `just dev` refuses to for them.

1. Logic: `just test-rust` runs the workspace's tests. Engine, draft and AI
   behavior is tested here, not through the app. A card's scenarios live in its
   file; `just oracle <card names>` checks them against XMage and Forge, and
   `just fuzz` plays random games at length.
2. UI: `cd ui && npm run typecheck`. `just ui-shots [names]` screenshots UI
   states headless and prints their paths; `just ui-test` plays the game screen
   through a fixture. A card's spec (`/specs/<card>`, the `specs` skill) shows
   each of its UI moments as a live game: `just specs --check` and
   `just specs --shots` test and screenshot them, and `just specs` serves the
   pages for a human.
3. The app: `just smoke` builds the debug app and launches it with no window and
   no Dock icon until the UI reports over IPC that it loaded (about a second
   once built). It fails on a build error or a UI that never loads.

A human runs `just dev` for the app with hot reload, or `just bundle` for a
release `Beryl.app`.

Benchmarks follow `docs/benchmarking.md`: `just bench-count` for instruction
counts, `just bench` for timing (it refuses a busy machine).

Tournaments are expensive: a game involving an MCTS agent at
`--iterations 300 --trees 4` costs about 180 CPU-seconds, so a 100-game pairing
holds the whole machine for about 20 minutes and a full round robin for over 2
hours. `just tournament`, `just play` and `just review` run at utility QoS
(`taskpolicy -c utility`), so other sessions' checks get the CPU first: `nice`
does not do that on macOS. They slow down while the machine is busy. Wardens
rerun only the pairings that gate acceptance, never reported-only ones.
`just longthink`, the long-think arena (`docs/ai-strategy.md` §5), holds the
machine for days: with no arguments it starts or resumes the run in
`$BERYL_CACHE/longthink`, any stop costs at most the decisions being searched,
`--status` prints where it stands, and `--power light` leaves room for other
work until `--power full`.

## Checks

- `just check` (fast, budget 60 s) runs the lanes whose files changed since the
  branch left `origin/staging`: `repo` (justfile and Markdown formatting),
  `python` (black, pyre, pytest), `rust` (rustfmt, clippy, tests) and `ui`
  (prettier, tsc). It also refuses conflict markers and bad commit subjects.
- `just ci` (slow, budget 300 s) runs every lane plus `app` (`just smoke`),
  `shots` (`just ui-shots`, which fails on any page error), `ui-test`
  (`just ui-test`, which plays the game screen through a fixture), `oracle`
  (`just oracle`, which needs `just oracle-setup` once per machine) and `counts`
  (`just count-gate`: the rollout benchmark's instruction count, held to the bar
  in `rust/crates/beryl_bench/count-gate.toml`, whose comments say when it may
  rise). A run over budget fails: speed up, move or delete the slowest step
  rather than raise it.
- Budgets and deadlines count load-adjusted seconds: while the load average is
  over the core count, the clock runs slower in proportion, up to 8x. A check
  passes on a busy machine if it would on a quiet one.
- A fresh worktree clones the cached `rust/target` and `ui/node_modules` in
  seconds (copy-on-write); removing it saves them back.

## Workflow

- When launching more than one subagent at a time, please always use Sonnet.
- When designing a UI, reduce the amount of words you put on screen by at least
  50%.
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
