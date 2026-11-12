# Working on Project Beryl

Project Beryl is a research project to build a Magic: The Gathering AI for the
Legacy constructed format that plays the format's top decks as well as a strong
human player. It is a Tauri 2 desktop app for Apple silicon Macs, with a Rust
workspace behind a React and TypeScript UI. The repository is public.

The project started out targeting cube draft and switched to Legacy on
2026-11-09. Games are two-player, best of three with sideboards, between fixed
tournament decklists. There is no drafting. The V1 card pool is the main decks
and sideboards of the Legacy decks we choose to support, kept as decklists in
`docs/sample-decks/`. The rules engine and AI only need to handle those cards.
Until those decks are implemented, we test with the WU Control mirror left over
from the cube era:
`docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`
(`decks::WU_CONTROL`), and before that `decks::VINTAGE_WU_CONTROL`. Never delete
a card implementation.

The long-term goal is to support 50,000+ different cards, so write modular code
that scales to that size.

All rules engine code must have comments that point to the part of the
Comprehensive Rules it implements. Link to
https://yawgatog.com/resources/magic-rules/ and quote the relevant text from the
rules release pinned in `data/rules-version.json`.

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

- `rust/` is the Cargo workspace.
  - `crates/beryl_app` is the Tauri shell: the window, IPC commands,
    `tauri.conf.json`, capabilities and icons. Keep it thin. Game rules and AI
    go in their own library crates under `rust/crates/`, which build and test
    without Tauri. `docs/engine-architecture.md` §16 describes the crates, and
    `docs/project-50k.md` is the plan for restructuring them to scale.
  - Each card is one file, `rust/cards/<crate>/<set>/<card>.rs`, placed by the
    set it was first printed in. `just card path <name>` prints the path. Card
    crates depend only on the shared vocabulary crate.
  - Nobody maintains lists of card files by hand. Build scripts
    (`beryl_card_build`) generate each set crate's module list and tables, and
    `beryl_cards`' registry and `names` from them. Adding a card means adding a
    file.
  - Nobody types a mana cost or type line either. Each card face starts from a
    `printed::` constant generated from the card store, and each card file's
    header comment is generated too (`just card header`).
- `ui/` is the frontend: Vite, React and TypeScript (strict mode).
  - `src/ipc.ts` has typed wrappers for the Rust commands. Always call Rust
    through it.
  - `src/ipc.ts` also has a fixture transport that replays recorded sessions,
    from seeded games or from scripted boards (`fixtures.json` and `beryl_cli`'s
    `boards.rs`). `just ui-shots` screenshots these in the states listed in
    `shots.json`.
  - `src/generated/protocol.ts` is generated from `beryl_session` by ts-rs. A
    test fails when it is out of date, and its header explains how to regenerate
    it.
  - `src/stage/` is the "stage", the code that draws the game table and animates
    it. It plays the session's animation scripts on a clock that tests can step
    through one tick at a time (plan K25).
- `data/` holds data files.
  - `card-store.tsv` is the card store, built by `just cards` from Scryfall's
    bulk data. It has a record for every card in the pool and every token those
    cards make, sorted so it can be searched by seeking
    (`tools/src/beryl_tools/card_store.py` defines the format). The app embeds
    it (`beryl_card_store`). The UI asks for card records by name through
    `beryl_session`'s `card_records` (printing, artist, text and image URLs for
    each face), and `ui/src/cardRecords.ts` caches them before drawing. The UI
    bundles no card data of its own.
  - `just cards` also writes `set-groups.json`, which says which crate each card
    file goes in. Each crate covers a fixed range of first-printing sets that
    never changes (`set_groups.py`). It also writes `catalogs.json`: Scryfall's
    type and keyword catalogs plus the counter kinds the pool uses
    (`catalogs.py`).
  - `card-pool-exclusions.json`: cards left out of the store even though they
    are legal in Vintage.
  - `card-printings.csv`: the specific printing chosen for each card.
  - `oracle-divergences.json`: test scenarios where an oracle (XMage or Forge,
    see Testing) gets the rules wrong, each with its rules citation.
  - `oracle-exporter-gaps.json`: generated scenarios that an oracle's exporter
    can't express, which are reported as unsupported.
  - `card-names-allowlist.json`: generic code files that still mention card
    names, and one-card mechanics still in the engine (see Code structure).
  - `save-breaks.json`: saved games in the corpus that can't be migrated past a
    particular change.
  - `arena-motion.json`: MTG Arena's animation timings for each animation "verb"
    (a reusable animation such as a card moving to a zone), measured from the
    recordings. Beryl's own timings started from these.
  - `ui-conventions.json`: the UI design rules, which `just conventions` turns
    into `docs/ui-conventions.md`. Edit the JSON, never the generated page.
  - The app downloads card images on demand into its cache (`beryl_app`'s
    `art.rs`). No images are committed.
- `docs/ui_mockups/`: generated images of the UI, kept as visual references.
  They disagree with each other in details, so they are not a spec. Never tune
  UI values to match one image.
- `tools/`: `by`, the Python CLI behind the justfile, including the XMage and
  Forge exporters and their Java harnesses. Run `just` to list the commands.
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
  either one moves `master` forward. A failure's Steward bead goes first to a
  machine on the OS that failed, and to any machine after two hours.
  `enact ci --wait` lists failures on the other OS as `other_os_stewards`. Slow
  CI doesn't run on a timer: each machine tests `staging` whenever it's free and
  fixes failures as they appear.
- The checks are defined in `.enact.toml`: `just check` (fast) and `just ci`
  (slow).
- Beads are the task tracker. They live in this repo (`bd`, prefix `by`);
  publish bead changes with `enact sync`. Plans, designs and decisions go in
  beads, not Markdown, so a bead's history is the record.
- File beads freely and in parallel. Merge conflicts are cheap: `enact push`
  rebases, and you fix and push again. A bead can be picked up within minutes of
  filing, so either land what it depends on in `staging` first or block it with
  a dependency.
- Every session has to spend time learning the code it touches, so each bead
  should name the files, helpers and tests it needs. Give beads that touch the
  same code the same `lane:<name>` label so one session works through them in
  order. Label mechanical beads (wiring, renames, boilerplate tests)
  `tier:light` so they run on Sonnet.
- **No Fable without an explicit ask.** Never label a bead `tier:fable`, and
  never start a session or subagent on a Fable model, unless the user has
  explicitly asked for it. Design work defaults to `tier:opus` and mechanical
  work to `tier:sonnet` (or `tier:light`).
- No plan, bead or skill includes a step that waits for the user's approval
  (plan K31). Where his judgement might be wanted, go ahead with a sensible
  default and record the choice in the bead you're working on. Never file a bead
  asking him to look at something. The one exception is the count gate in K1 of
  `docs/project-50k.md`.
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
  `Object` or `State` (plan K4).
- Helpers used by two or more cards go in the shared library, not in a card
  file.
- The kernel, rules and card crates have no runtime dependencies. When you add a
  dependency elsewhere, its `Cargo.toml` comment says what it replaces.
- Forge is used only as a test oracle. No card data, vocabulary or design may
  come from its scripts or code (plan K16).

## Testing your work

Use the cheapest test that covers your change. Agents never open a window;
`just dev` refuses to run for them.

1. Logic: `just test-rust` runs the workspace's tests. Test engine and AI
   behavior here, not through the app.
   - Each card's test scenarios live in its own file. `just oracle <card names>`
     runs those scenarios against XMage and Forge, two existing Magic engines we
     use as "oracles" to check our rules.
   - `just citations` checks every Comprehensive Rules citation and quote in the
     code against the rules release pinned in `data/rules-version.json`. The
     `names` lane runs it in both checks. `just citations migrate <date>` moves
     the code to a newer rules release: it renumbers the citations it can match
     and lists the ones that need a human decision.
   - `just fuzz` plays long random games, and `just exhaust` tries every
     sequence of decisions near the end positions of the scenarios.
   - To start a new card, run `just card brief <name>`. It shows the card's
     Oracle text, rulings, matching vocabulary entries, the most similar cards
     already implemented and the XMage class.
2. UI: run `cd ui && npm run typecheck`. `npm test` runs the tests that need no
   browser (`ui/scripts/*.test.mjs`, which cover the stage's clock and the order
   animations play in).
   - `just ui-shots [names]` takes headless screenshots of UI states and prints
     their paths.
   - `just ui-test` plays the game screen through a fixture
     (`ui/scripts/kept.mjs`). Then it runs each animation verb on the stage's
     own clock (`ui/scripts/verbs.mjs`; `just ui-test verb:` runs only these).
     Then it runs each prompt type, answering it with the clicks recorded in its
     fixture along with the session's replies (`ui/scripts/shapes.mjs`;
     `just ui-test shape:`).
   - `just ui-play [fixtures]` plays every recorded fixture to the end, on both
     the desktop and phone layouts, with the stage's checks turned on.
   - `just specs --phone` and `just ui-live --phone` open the phone layout.
   - Every card has a spec page (`/specs/<card>`), generated from its definition
     and scenarios. It shows each UI moment of the card as a live game from both
     players' seats. To reproduce, debug or check a card's behavior in the UI,
     use the `specs` skill: `just specs --describe`, `--film`, `--motion` and
     `--drive` show what happens at each moment for both players as text, as
     pictures, as an animation report and under Playwright.
3. The app: `just smoke` builds the debug app and launches it without a window
   or Dock icon, and waits until the UI reports over IPC that it has loaded
   (about a second once built). It fails on a build error or if the UI never
   loads.
4. The phone apps: `just mobile-setup` installs their toolchains, once per
   machine. `just ios-smoke` builds the iOS app and launches it in a headless
   iOS Simulator until the UI loads. `just ios` launches it in a simulator,
   which you can drive with the iOS Simulator tool. Agents never install onto a
   real phone: `just ios-device` (which signs the app and installs it on the
   paired iPhone) is only for the user, and with `--no-install` it only builds.
   `just android-smoke` and `just android` do the same in the Android emulator
   (headless for agents; drive it with `adb shell input` or the WebView's
   DevTools over `adb forward`).

A human runs `just dev` for the app with hot reload, or `just bundle` for a
release `Beryl.app`.

Benchmarks follow `docs/benchmarking.md`: `just bench-count` counts instructions
and `just bench` measures time (it refuses to run on a busy machine).

Tournaments are expensive. One game with an MCTS agent at
`--iterations 300 --trees 4` takes about 180 CPU-seconds, so a 100-game match
between two agents uses the whole machine for about 20 minutes, and a full round
robin for over 2 hours. `just tournament`, `just play` and `just review --agent`
run at utility priority (`taskpolicy -c utility`) so that other sessions' checks
get the CPU first (`nice` doesn't do this on macOS). They slow down while the
machine is busy. Wardens (the sessions that evaluate AI changes) rerun only the
matches that decide whether a change is accepted, never the ones that are only
reported. The "long-think arena" is retired and its command deleted
(`docs/ai-strategy.md` §8): it used the machine for days, and the AI now
improves through the ladder's head-to-head matches instead. Its last run is
recorded in `docs/longthink-run-003.md`.

## Checks

The checks are split into "lanes", one per kind of file or test.

- `just check` (fast, 60 s budget) runs the lanes whose files have changed since
  the branch left `origin/staging`:

  - `repo`: justfile and Markdown formatting.
  - `python`: black, pyre and pytest. If only cards or vocabulary changed, it
    runs just the scale harness's tests.
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
    - `two-player`: no rules module or card file uses
      `beryl_kernel::two_player`.
    - `board-door`: no rules module or card file uses `.board()` or
      `board::Board` outside tests.
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
  marks the ones left to slow CI. The fast check also rejects conflict markers
  and badly formatted commit subjects.

- `just ci` (slow, 300 s budget; 450 s on Windows) runs every lane, plus:

  - `app`: `just smoke`.
  - `shots`: `just ui-shots`, which fails on any page error.
  - `ui-test`: `just ui-test`, which plays the game screen through a fixture.
  - `oracle`: `just oracle --select`, which needs `just oracle-setup` once per
    machine. It runs the cards changed since `origin/master` plus a sample
    chosen from the commit hash, or every card if the exporters or harnesses
    changed. The nightly run plays every card on staging.
  - `counts`: `just count-gate`, which checks the rollout benchmark's
    instruction count against the limit in
    `rust/crates/beryl_bench/count-gate.toml`. That file's comments say when the
    limit may go up.
  - `mobile`: `cargo check` of the app for the iOS Simulator and Android, which
    needs `just mobile-setup`.

  Windows leaves `app`, `shots`, `ui-test`, `oracle`, `counts` and `mobile` to
  the Mac, and its fast check skips the browser checks and `motion` (see
  `WINDOWS_SKIPS` in `checks.py`). A run that goes over budget still passes, and
  files a `check-budget` bead (`budget.py`) to speed up, move or delete its
  slowest steps instead of raising the budget. The deadline, at least 600 s,
  only catches a hang.

- A UI bug fix adds the broken state as a fixture or spec moment, so the generic
  UI checks (overlap, off-screen, reachability, duplicate objects, page errors,
  motion) cover it. If the bug is a new kind of problem, extend one of those
  checks. Only write a custom scripted check in `ui/scripts/kept.mjs` when none
  of the generic checks can express the property, and explain why in a comment
  (`docs/project-50k.md`, K20).

- Budgets and deadlines are measured in load-adjusted seconds: while the load
  average is above the number of cores, the clock runs proportionally slower, up
  to 8x. So a check that would pass on a quiet machine also passes on a busy
  one. A lane killed by its deadline automatically reruns once with a fresh
  deadline, so if a push still times out, some step is slow or hung. Find it
  before pushing again.

- The machine runs at most half its core count's worth of browser pages at once,
  across all runs (`ui/scripts/slots.mjs`). Runs that aren't checks share a
  third of those slots. Headless WebKit runs at a higher priority than any
  build, so run only the checks your change needs (`just ui-test <title>`,
  `just specs <card>`) rather than a full sweep.

- A new worktree copies the cached `rust/target` and `ui/node_modules` in
  seconds (copy-on-write), and removing the worktree saves them back.

## UI

MTG Arena is the reference for Beryl's game flow, the order and timing of
events, the table layout and the wording (`docs/project-50k.md`, K22). Before
you design or change a prompt, animation, arrow or label, look up how Arena does
it with the `arena-video` skill. A screenshot or filmstrip answers questions
that a description can't. To make something match Arena as closely as possible,
pixel for pixel, use the `arena-pixel-match` skill.

- Animations follow Arena in what they show and when: what changed, who did it,
  where it went, in what order, and what stays still. No review is required to
  land an animation: `just motion` checks it against its own recorded values,
  and the `motion-review` skill runs only when the user asks for it.
- Beryl keeps its own visual style: colors, fonts, icon artwork, card frames,
  and full-size cards that tap.
- Beryl differs from Arena only where [UI Conventions](docs/ui-conventions.md)
  lists a numbered exception. Check that list before copying Arena, and before
  "fixing" something that differs from it.
- The user's bug report takes priority over Arena and over any earlier report.
  Build what it asks for, and add, change or remove the matching exception in
  the same change.
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
- Agents don't add Markdown files unless explicitly asked. README.md, AGENTS.md
  and `docs/` are the only Markdown; plans, designs and notes go in beads.
- Never commit logs, screenshots or other output from a run.
- When you fix a rules engine bug, including one in your own code from the same
  session, add an entry to `docs/bugs.md`: a short description of the bug and
  how similar bugs could be prevented in future (for example, through the type
  system). Link the fixing commit's hash when the bug wasn't found and fixed in
  the same session.
