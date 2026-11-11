# Project 50k

*A plan to rebuild Beryl's engine, card and UI structure so it could hold every
known Magic card. Decided 2026-11-10 in a design interview, on an audit of the
code and on measurements taken that day, and revised the same day after an audit
of the plan itself a second interview on the checks (K18 to K21) and a third on
how the UI presents a game (K22 to K31). The work is tracked as an Enact epic in
beads; this document is the plan the epic's beads are written against.*

Beryl supports 47 cards. Its structure was built for that pool: one cards crate,
hand-kept registries, one Rust function per ability, and an enum variant or
state flag wherever an early card needed one. Project 50k replaces that
structure with one that would still work with the full Comprehensive Rules and
50,000 card definitions.

- **In scope:** architecture. Crates, state ownership, the card format, the
  shared rules library, the prompt protocol, how the UI presents a game, the
  card data pipeline, verification, the checks every push runs, and the tools a
  fleet of cheap models needs to write cards.
- **Not in scope:** new rules features. Expanding what the engine can do is the
  next project, [rules-coverage.md](rules-coverage.md). The only new cards are
  the ten of the closing authoring pilot.
- **The card pool** is every card in Scryfall's Oracle data except un-set cards,
  dexterity cards and Alchemy cards.
- **Unchanged:** the engine stays one exact, flat, `Copy` state machine built
  for search ([engine-architecture.md](engine-architecture.md) §2). Nothing here
  trades rollout speed for scale.

## What the audit found

**Card files.** 47 files, 9,226 lines, median 195.

- 64% of every file is scenarios. The definition is about 15%, bodies about 10%.
- Of 67 body functions, about 25 are near-duplicates. Only one helper is shared
  between two cards.
- Cost, types, colors and P/T are typed by hand although `data/cards.json`
  already has them.
- About 25 cards could be pure data over a stock effect library, 16 need a small
  body, and 6 need mechanics the engine has only for them.

**One-card logic in generic code.** About 40 one-card mechanics sit in the
engine and session.

- `Rule::OpponentsCastAsSorceries`, `Player::sorceries_as_flash` (threaded
  through six files), `Modification::PowerIsCreaturesYouControl`,
  `Needs::ExiledWithSource`.
- 11 of the 22 `Ask` variants exist for one card each. A new one touches about
  eight files.
- Waterbend, for one card, has about 25 sites in `cast.rs`, 315 lines of session
  preview, its own IPC command, save-format flags and code in eight UI files.
- The generic agents read `beryl_search/src/values.rs`: 61 hand-tuned entries
  keyed by card name.
- `xmage.py` and `forge.py` carry about 20 tables keyed by card name.

**Ceilings.**

- `Types` uses 7 of 8 bits. `Subtype` is a hand-written enum of 32 in 128 bits.
  `Rule` caps at 8 variants.
- An object holds one long-tail counter kind. A floating effect affects exactly
  one object.
- A deck holds 60 cards, a game 255 distinct cards, and two players are assumed
  in about 100 places.
- `EffectCtx` has 88 methods and depends on 22 of the engine's modules.
- The UI bundles `cards.json` into its JavaScript: 65 KB today, about 42 MB at
  50,000 cards.

**Bugs.** Of the 17 entries in [bugs.md](bugs.md), ten share one cause: inside
`beryl_engine` any module can read or write any field. `object_mut` is called
from 13 files.

**Checks.** See [Measurements](#measurements). Slow CI failed 60 of 414 runs,
and 57 of those were the two browser lanes, which the fast check never runs.

- A UI change's author never sees the browser check it broke. A Steward with no
  context repairs it after it lands: 34 Steward beads in five days.
- About a third of those beads were a real bug, a third a test made stale by an
  intended change, and a quarter a test racing the wall clock.
- Generic detectors run over many screens found most of the real bugs. Scripted
  checks of one behavior caused nearly all the stale and racy failures.
- Nothing records what a check has caught or cost, so nothing can be removed on
  evidence.

**UI reports.** See [Measurements](#measurements). In three days the UI drew 180
bug reports: about 150 on the UI, about 22 on the AI's plays and 4 on the rules.

- About 40 re-filed five themes. One motion, a trigger lifting out of its card,
  took twelve reports and seven commits. It is how Arena presents a trigger.
- The session sends a snapshot and a flat list of events. `beats.ts` rebuilds a
  timeline from them, and cards, trigger flights, arrows, combat lines and mana
  each animate in code of their own, sequenced by fixed waits. One motion ran
  through a different path for each way the views happened to batch.
- A fixing agent had prose to interpret and its own pixel measurements to verify
  against. The fixer closed 178 of the 180, at a median of 47 minutes.
- `docs/ui-conventions.md` covered three topics. The reports state about 60
  rules.

**Compile time.** See [Measurements](#measurements). One crate of 50,000 real
card files would need about 53 GiB of compiler memory. Split into crates, the
same cards build in about 90 seconds.

## Decisions

| ID  | Decision                                                                                                                                                                                                                                                                                                | Why                                                                                                                                                                                                                              |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| K1  | A card's behavior is typed effect data run by one interpreter. A Rust function is the escape hatch. Held to the count gate with no bump; if it cannot hold, the run stops for a human decision.                                                                                                         | Opaque bodies force hand-kept metadata on every card. Bodies run 10 to 20 times a game, so dispatch is about 0.02% of a rollout. K3, K9 and K13 all read the data, so there is no substitute to fall back to.                    |
| K2  | Printed characteristics are generated from Scryfall for the whole pool into the card store. A card gets a Rust file, under a generated header, when it gets written abilities.                                                                                                                          | Nobody types a cost or type line, and keyword-only creatures need no authoring. About 30,000 stub files would be rebuilt by every vocabulary edit and rewritten by every Scryfall refresh.                                       |
| K3  | The bar for a card is tiered. Library effects carry hand-written scenarios; a card built only from them gets generated scenarios.                                                                                                                                                                       | Hand-written scenarios for every card would be about 5.8 million lines and hours of oracle time.                                                                                                                                 |
| K4  | Struct fields hold only state the Comprehensive Rules give that thing. Everything a card creates lives in three stores: Effects, Memos and Trackers.                                                                                                                                                    | Each flag hand-rolls its own duration, expiry, layout conversion, save description and observation.                                                                                                                              |
| K5  | `EffectCtx` becomes free functions over `Game` and `Resolution`, in modules that mirror the Comprehensive Rules.                                                                                                                                                                                        | An agent reading "Scry 2" or a rule number finds the file by name, and each file stays small.                                                                                                                                    |
| K6  | A kernel crate owns the state. Fields are private; other crates use typed mutators, queries and order-free sets.                                                                                                                                                                                        | Ten of 17 logged bugs needed a chokepoint that nothing enforced. A crate boundary enforces it at compile time.                                                                                                                   |
| K7  | Card data says what the card says ("each opponent", "defending player"). The kernel's API is format-neutral. No multiplayer logic is written.                                                                                                                                                           | Card data cannot be re-audited cheaply; the kernel refactor touches every two-player site anyway.                                                                                                                                |
| K8  | Card files are grouped by first-printing set, in many crates, with registries generated at build time.                                                                                                                                                                                                  | A set is how work is batched; a one-card edit rebuilds one crate; parallel agents never edit a shared list.                                                                                                                      |
| K9  | Generic agents derive card knowledge from definitions. Hand-tuned numbers survive only as an optional overlay for supported decks.                                                                                                                                                                      | A name-keyed table cannot cover the pool. The overlay keeps the WU mirror's strength, in the same category as the expert.                                                                                                        |
| K10 | Prompts are about ten shapes plus a closed list of purpose tags.                                                                                                                                                                                                                                        | Half of today's prompt kinds serve one card. Shapes are built once; a new card needs no protocol or UI change.                                                                                                                   |
| K11 | The rewrite happens in place. At every push all cards work, frozen saves replay, rollouts are unchanged and the count gate holds.                                                                                                                                                                       | Three machine checks already define "behavior did not change", which makes an unattended refactor safe.                                                                                                                          |
| K12 | The kernel, rules and card crates have no runtime dependencies. Tests adopt `proptest` and `insta`; tools use mature crates freely.                                                                                                                                                                     | The hot path needs exact layout control. Fuzz failures need shrinking. Tooling should not hand-roll parsing and threading.                                                                                                       |
| K13 | A definition renders back to Oracle-style text, which is compared with Scryfall's.                                                                                                                                                                                                                      | A cheap model's typical error is a plausible definition that differs from the card. Rendering makes that error mechanical to find.                                                                                               |
| K14 | The project ends at parity, a scale rehearsal and a ten-card authoring pilot.                                                                                                                                                                                                                           | Foundations first. The pilot is the only test that a cheap model can write a card from the brief; its cards are Legacy cards the project needs anyway.                                                                           |
| K15 | Six parallel tracks. One Warden per milestone. A Mason audit gates each step of the engine chain (after milestones 2, 3, 4 and 5) and one closes the run. No Sage, one Archivist.                                                                                                                       | Parity is machine-checked. Architecture drift is what the checks cannot see, and it is expensive in the vocabulary, kernel, stores and library.                                                                                  |
| K16 | Beryl stays Apache-2.0 and takes no card data from Forge. Forge is a test oracle and a census of mechanics. A card brief cites Oracle text, rulings and the MIT-licensed XMage class.                                                                                                                   | A translator from Forge's scripts would be a second vocabulary: about 1,600 hand-mapped script entries for 83% of its cards ([Measurements](#measurements)), built from the idioms Beryl avoids, at the cost of a permanent GPL. |
| K17 | `just check` fails a source file over 800 lines. Today's offenders are on an allowlist that may only shrink. Tests, test drivers, generated files, the expert system and the retired long-think code are exempt.                                                                                        | Appending to a big file is an agent's path of least resistance; conventions alone did not hold.                                                                                                                                  |
| K18 | Every check's every run is logged, and each failure gets a verdict the tool infers: flaky, stale test or caught a bug. A weekly agent applies a fixed pruning rule to the records, with no approval step.                                                                                               | Nobody can say which checks earn their time. A verdict an agent must write costs tokens and drifts toward "caught a bug"; the fix's diff already says which it was.                                                              |
| K19 | The fast check runs the browser checks a change affects, chosen by measured coverage and capped by time. Slow CI stays the full sweep on every staging tip until the miss rate is measured.                                                                                                             | The author knows whether the test or the code is wrong; a Steward has to find out. `Board.tsx` changes in a quarter of all commits, so choosing by file would choose everything.                                                 |
| K20 | A UI fix adds the broken state to what the generic audits run over. A scripted check is the exception and says in a comment why no audit can express it.                                                                                                                                                | Audits found about six of eight real UI bugs; scripted checks caused about 21 stale or racy failures. Only audits scale to generated specs for 50,000 cards.                                                                     |
| K21 | A failed check reruns once. One that flakes twice in seven days is quarantined: it runs and is logged but cannot fail a run. A failure that survives the rerun on staging is bisected before its Steward bead is filed.                                                                                 | A racy test should not charge an unrelated change. A Steward told the check, the commit and the check's record starts from the answer.                                                                                           |
| K22 | Arena is the reference for flow, motion, timing, table layout and wording. Beryl keeps its own skin: colors, type, icon drawings, frames, full cards that tap. Beryl differs from Arena only by a numbered exception, and the user's bug report makes or removes one, the latest winning.               | Agents had prose to interpret, and read it differently each time. A recording can be measured. The lift reported twelve times is Arena's own.                                                                                    |
| K23 | For each update the session builds, per seat, a script: ordered steps of about 15 motion verbs covering every visible change, arrows, combat lines and marks included. Engine events carry their cause and what they are simultaneous with. The UI plays scripts and infers nothing.                    | The UI rebuilt a timeline from snapshots and guessed causes the engine knew. Each fix covered the batching patterns it had seen.                                                                                                 |
| K24 | A card's scripts are held to invariants: replaying a script on the previous view gives the next view, and no arrow points at an object that is moving or gone. Text snapshots exist only where a person wrote the expectation: per verb, per prompt shape and for the library's hand-written scenarios. | K20: invariants scale to the pool. A snapshot per card is a change detector that one verb change rewrites everywhere, and K18's rule would delete it.                                                                            |
| K25 | An imperative stage plays scripts over one persistent DOM node per game object, on a clock it owns and a test can step. A step ends when its animation ends. It is built behind a switch and cut over once; the old animation code is deleted in the same change.                                       | Ten layout effects and a table of fixed waits coordinate today's motion. Two animation systems sharing a screen is the seam that flickered. K21's paused clock does not drive CSS or Web Animations.                             |
| K26 | A prompt's state lives in the session. Each click is a command; the answer is marks from a closed list and the state of each button.                                                                                                                                                                    | The UI decided what an outline meant, and one prompt ended up with three colors. A prompt interaction becomes a Rust test, and the phone behaves identically.                                                                    |
| K27 | One tool measures a motion the same way in an Arena recording and in a Beryl capture. Arena's numbers are committed as reference data, and each verb is held to them within a tolerance.                                                                                                                | The motion audit asks only whether something moved. It passed fixes the user reported again.                                                                                                                                     |
| K28 | The design language is data: each verb's values, each mark, the numbered exceptions and the cases Arena does not show. `docs/ui-conventions.md` is generated from it.                                                                                                                                   | Nothing forced the code and the prose to agree. An exception that is data can exempt its verb from the Arena comparison.                                                                                                         |
| K29 | What the recordings do not show is composed from existing verbs and shapes and listed. The phone shares the stage and keeps its own layout, listed the same way. A judgement call files a `human` bead and never blocks.                                                                                | 50,000 cards cannot each need a decision, and the recordings are desktop only.                                                                                                                                                   |
| K30 | From the track's start until the cut-over, reports about motion, arrows and combat lines are banked as cases the stage must pass, not fixed in the code it replaces.                                                                                                                                    | In three days the UI took 14,351 added lines of fixes, most in files the stage deletes.                                                                                                                                          |
| K31 | No step of this plan waits for the user's approval. Where his judgement is wanted, the agent proceeds on a default and files a non-blocking bead labelled `human`. K1's count gate is the one exception: it stops the run.                                                                              | An unattended run that waits on a person stalls, and an approval nobody is there to give blocks every bead behind it. This is how Enact is meant to run.                                                                         |

## Target architecture

### Crates

| Crate                  | Holds                                                                                                  | Depends on                             |
| ---------------------- | ------------------------------------------------------------------------------------------------------ | -------------------------------------- |
| `beryl_kernel`         | Ids, bitboards, fixed queues, `State` and its layouts, mutators, queries, the three stores, invariants | nothing                                |
| `beryl_card`           | The card vocabulary: definition types, effect data, filters, costs, prompt shapes, English templates   | `beryl_kernel`'s id types              |
| `beryl_rules`          | Turns, priority, casting, mana, combat, layers, triggers, the interpreter, the rules library           | `beryl_kernel`, `beryl_card`           |
| `cards/<set group>`    | One file per card, grouped by first-printing set                                                       | `beryl_card`; `beryl_rules` for bodies |
| `beryl_cards`          | The generated registry over every set crate, and the decklists                                         | the set crates                         |
| `beryl_search`, others | As today ([engine-architecture.md](engine-architecture.md) §16)                                        | as today                               |

- `beryl_engine` stays as the facade other crates import until the last caller
  has moved.
- A full rebuild of every card crate is cheap once definitions are data: 10,000
  data-only definitions check in 9 seconds on one core. So an engine edit that
  rebuilds every set crate costs seconds, not minutes.
- Scenarios compile only for tests and tools. They are three quarters of today's
  compile cost and would add megabytes to the app.

### A card file

Illustrative; names are settled in the milestone that builds the vocabulary.

```rust
//! Teferi, Time Raveler  {1}{W}{U}
//! Legendary Planeswalker — Teferi  Loyalty 4
//! (generated header: the printed card and its Oracle text)

pub static CARD: Card = card(names::TEFERI_TIME_RAVELER)
    .static_ability(each_opponent(CastOnlyAtSorcerySpeed))
    .loyalty(1, until_your_next_turn(you_may(CastAsThoughFlash(SORCERY))))
    .loyalty(
        -3,
        target(up_to(1, ARTIFACT.or(CREATURE).or(ENCHANTMENT))),
        seq(&[return_to_hand(TARGET), draw(1)]),
    );
```

- Everything is built with `const fn`, so a definition is a static with no
  startup cost and no macro.
- Costs that are not printed (a kicker cost, "unless they pay") use Scryfall's
  notation, parsed at compile time: `mana("{1}{U}")`.
- What the engine, AI, UI and tools need to know about an ability (whether it
  can pause, whether it is removal, what it needs to do anything) is read from
  the data, not declared beside it.
- A card the vocabulary cannot express keeps a Rust body in the same file. When
  a second card needs the same body, it moves into the library.

### State

- **Structs** hold what the rules give a player, object or turn: life, counters,
  tapped, zone, the step.
- **Effects** are continuous effects with a duration. The affected side is an
  object, a player, a locked-in set or the game. The payload is a layer change,
  a rule change, a replacement, a cost change or a permission. Teferi's +1 is
  one entry.
- **Memos** are what an object remembers about how it was cast or entered:
  kicked, X, a chosen color, what it exiled. They clear on a zone change.
- **Trackers** are this-turn and this-game counts, kept only when a card in the
  game reads them.
- Each store implements duration, expiry, layout conversion, saving and
  observation once. Each is indexed by kind, and a per-game capability bit makes
  an absent kind cost one AND ([rules-coverage.md](rules-coverage.md),
  principles 1 and 2).
- Player count, deck size, slot count and card-index width are parameters of the
  layout. `Small` stays two players within 20 KiB.

### Kernel rules

- Only the kernel writes a field. Each mutator keeps bitboards, caches, links
  and knowledge consistent.
- Other crates read derived answers. `card_behind(object)` is nothing for a
  token; `mana_value(object)` accounts for copies and faces.
- A bitboard outside the kernel offers only order-free operations. Getting a
  sequence means naming an order the rules can see, such as battlefield order.
- Ability, target, mode and option indices are distinct types.
- Players are reached through sets and orders: opponents of a player, players in
  turn order, defending players. The two-player shortcut is not exposed.

### Rules library

```
beryl_rules/src/
  actions/        CR 701, one file per keyword action (destroy, scry, explore)
  keywords/       CR 702, one file per keyword ability (flashback, kicker)
  damage.rs       CR 120
  counters.rs     CR 122
  tokens.rs       CR 111
```

Each module holds the function, the effect-data entry that calls it, its rule
citation, its English template and its scenarios. Functions take `Game` (state,
table, event sink). Those that only make sense while something resolves also
take `Resolution` (source, controller, targets, X, the answer record).

### Prompts

A **shape** says how the player answers: pick up to N objects, order, split into
destinations, yes or no, a number, a mode, a catalog choice (a color, a type, a
card name), divide an amount, pay with permanents. A **purpose** tag says what
it is for (scry, discard, search) and tunes wording, sound and motion. The stage
builds each shape once (K25), and the session holds its state (K26). Crew,
convoke and waterbend are one shape.

### Presentation

Arena is the reference (K22). What Beryl shows, in what order and from where, is
decided once in Rust and played by the UI.

- **Script.** For each update the session sends each seat a script beside the
  view: ordered steps, each a verb applied to objects, with the steps that
  happen together grouped. About 15 verbs: emerge, fly, land, appear, vanish,
  flip, tap, strike, showcase, point, mark, count. The list is settled when the
  type is designed. An arrow, a combat line or a mark is a step with a beginning
  and an end, not something recomputed from each view. A script carries no
  geometry and no timing.
- **Causes.** Engine events carry their cause and what they are simultaneous
  with. Step one infers both in the session from today's flat events. Step two
  takes them from the interpreter (K1), where every effect entry knows its
  source, and deletes the inference. Rollouts drop events at the sink, and the
  tags are held to the count gate with no bump.
- **Invariants.** A debug assertion wherever the session builds a script:
  replaying it on the previous view gives the next view exactly, so a change
  with no step fails. Rule invariants sit beside it: no arrow points at an
  object that is moving or has left; an arrow is hidden before its target moves;
  one visual per object unless a verb says otherwise. Every scenario, fixture
  recording and fuzz game exercises them, and none runs in a rollout. Text
  snapshots are written by hand only (K24).
- **Stage.** One player in the UI owns a persistent DOM node per game object and
  runs steps in order on a clock it owns. A step ends when its animation ends;
  there are no fixed waits. Tests step the clock (K21), and a small probe proves
  that before the animation mechanism is chosen. React keeps the chrome. The
  desktop and phone boards are two layouts of one stage.
- **Prompts.** The session holds a prompt's state (K26). The UI sends clicks and
  draws the marks it gets back: eligible, selected, suggested, friendly target,
  hostile target and the rest of a closed list, each with one look.
- **Design language.** Data files hold each verb's values, each mark, the
  numbered exceptions to Arena and the cases Arena's recordings do not show. The
  stage and the tests read them, and `docs/ui-conventions.md` is generated from
  them.
- **Motion oracle.** One tool tracks a moving card by frame differencing and
  prints the same numbers for an Arena clip and a Beryl capture: duration, start
  delay, stagger, path and peak size. Arena's numbers are measured once and
  committed under `data/` as reference data; they are measurements, not the log
  of a run. Durations, delays and stagger are checked against a verb's values
  with no browser. Path and peak size need a capture, taken in stepped stage
  time when the stage or the values change, and nightly.
- **Browser checks.** Each verb and each shape once, from both seats and in both
  layouts, beside `shots` and its layout audits. The duplicate and motion audits
  read the script's verbs: Arena's cast chooser shows a card twice, and its
  tokens appear without a flight.
- **No approval steps (K31).** Where the user's eye is wanted (how a line looks,
  a new verb, a rule Arena contradicts), the agent proceeds on Arena or on the
  nearest existing verb and files a `human` bead. Nothing waits on it.

### Card pool pipeline

- `just cards` generates characteristics for the whole pool and builds an
  indexed card store. The app serves records over IPC on demand.
- An unimplemented card is a record in the store, not a file. `just card new`
  creates a card's Rust file with its generated header.
- Subtypes, counter kinds and similar lists are generated from Scryfall's
  catalogs, not written by hand.
- A build script lists each set directory. No registry is kept by hand.
- Specs are generated for the card requested, not for every card at start.
- Oracle exporters derive each card's translation from its effect data.

### Verification

- Invariant and reference checks run in debug builds on every `apply`.
- Every scenario is re-run under permuted slot assignments.
- Bounded exhaustive search plays every decision sequence from scenario
  positions to a fixed depth, checking invariants at each state (milestone 1).
- `proptest` shrinks a failing fuzz game to a minimal sequence.
- Oracles run per changed card in CI. The full sweep runs nightly.
- Scenarios use basic lands and a small set of designated prop cards, so one
  card's change cannot break another card's tests.

### Checks

A check is one test: a Rust test, a browser check, a screenshot or an oracle
scenario. All four are logged and judged the same way.

- **Run log.** Every run of every check appends a record to `$BERYL_CACHE`: the
  commit, fast or slow, the outcome, the seconds, the load, and why the check
  was chosen. Nothing is committed. It is one machine's record and is lost with
  the cache.

- **Verdicts.** The tool infers one for each failure and asks the agent only
  when it cannot tell.

  - *Flaky:* it passed on rerun with the tree unchanged.
  - *Stale test:* the fix touched only the check or its fixture.
  - *Caught a bug:* the fix touched product code.
  - *Selection miss:* slow CI failed a check the fast check did not choose.

- **Failure records.** Every failure that reaches a verdict is also written to
  beads, which every machine shares and git keeps: one closed bead per check
  that has ever failed, and one comment on it per failure, with the commit, the
  verdict and what fixed it. Passes and timings stay in the run log. Quarantine
  and pruning read verdicts from the beads, so they agree on every machine.

- **Selection.** A nightly Chromium run records the functions and CSS rules each
  browser check executes. The fast check runs the checks whose recorded lines
  meet the diff. When those exceed the budget, the ones with the worst failure
  records run first and slow CI covers the rest. A changed card's scenarios run
  against the oracles before the push.

- **Flakes.** The browser harness pauses its clock unless a check asks
  otherwise. A failed check reruns once; a second flake in seven days
  quarantines it, with no bead.

- **After a merge.** `by ci` reruns the failed checks over the commits since
  `master` and ends its log with the checks, the culprit commit and each check's
  record. Enact files that as the Steward bead. A run over budget on a loaded
  machine or a cold cache reruns and files nothing.

- **Pruning.** `just ci-report` prints each check's record over 30 days, and
  `just ci-report --prune` each check this rule acts on, with its numbers. A
  weekly agent applies the rule in one commit that lists what it removed; a test
  run in slow CI only goes in `verdicts.SLOW_ONLY`:

  | Record                                        | Action              |
  | --------------------------------------------- | ------------------- |
  | Three stale updates and no catch              | Delete              |
  | Quarantined, never caught anything            | Delete              |
  | Quarantined, has caught a bug                 | Fix                 |
  | Never failed, in the costliest tenth          | Run in slow CI only |
  | A `keep` comment in its source, with a reason | Keep                |

- **New browser checks.** Under K20, a fix adds a fixture or spec moment for the
  audits (overlap, off-screen, reachability, duplicate objects, page errors,
  motion). A new class of bug extends an audit.

The budgets stay at 60 and 300 seconds.

### Authoring tools

1. **No model.** Vanilla and keyword-only creatures are complete from generated
   characteristics.
2. **One cheap model, one card.** `just card brief` prints a packet: Oracle text
   and rulings, matching vocabulary entries, the nearest implemented cards, and
   the XMage class as a reference. `just card check` builds one set crate and
   runs the generated scenarios on all three engines.
3. **Escalation.** A card the vocabulary cannot express is tagged with the
   mechanic it needs. A stronger model takes mechanics in order of cards
   unblocked.
4. **Render and diff.** Every definition renders to text that is compared with
   Scryfall's.

Forge's scripts are never an input to a definition (K16). The vocabulary is
designed from the Comprehensive Rules, and each entry is justified by its rule
and the cards that need it. A census tool parses the scripts only to count which
mechanics block the most cards.

### Conventions

These go into AGENTS.md in the first milestone.

- Spell words out. No misspellings to dodge a keyword: `static_ability`, not
  `statik`.
- No procedural macros written here. A `macro_rules!` only where a `const fn`
  cannot remove the repetition.
- No source file over 800 lines, tests and generated files aside.
- No card name in the kernel, rules, session, generic search or UI.
- A new dependency says in its `Cargo.toml` comment what it replaces.

## Milestones

Each lands in place under K11. They run as six tracks (K15).

| Track        | Milestones                    | Starts after                   |
| ------------ | ----------------------------- | ------------------------------ |
| Engine       | 2, 3, 4, 5, in order          | milestone 1                    |
| Prompts      | 6                             | milestone 2; lands before 5    |
| Data         | 7a                            | milestone 1                    |
| Hygiene      | file splits no milestone owns | milestone 1                    |
| Tail         | 7b, then 8                    | milestone 5                    |
| CI           | two steps, no milestone       | nothing; blocks nothing        |
| Presentation | two steps, no milestone       | milestone 1; cuts over after 6 |

1. **Guardrails.** The scale-rehearsal harness; the card-names test, seeded with
   the audit's inventory as a shrinking allowlist; the file-length check;
   invariants on every debug `apply`; permuted-slot scenario runs; shrinking;
   bounded exhaustive search; the conventions above.
2. **Effect data proof.** The `beryl_card` vocabulary, the interpreter, and the
   benchmark corpus's cards ported. Two proofs. Reach: before the types harden,
   the vocabulary's designer writes unimplemented definitions for 60 random
   in-scope cards and records how many fit, need one more entry, need a new kind
   of expression, or need a Rust body. Speed: the count gate holds at
   405,276,238 instructions with no bump. If it cannot after three attempts, the
   run stops for a human decision.
3. **Kernel.** Private state, mutators, queries, order-free sets, index types,
   the format-neutral API, capacities as layout parameters. No mutators for the
   fields milestone 4 removes.
4. **Stores.** Effects, Memos and Trackers; the one-card state fields and
   variants from the audit removed.
5. **Rules library and card port.** `EffectCtx` dissolved; all 47 cards as
   effect data, written against milestone 6's prompt shapes; generated
   scenarios; prop cards.
6. **Prompts.** Shapes and purposes through engine, session and protocol, with
   each prompt's state held in the session (K26) and the save-format migration.
   The old UI is adapted only enough to keep working; the stage builds each
   shape (presentation track). Runs beside milestones 3 and 4.
7. **Card pool.** 7a, with no engine dependency: generated characteristics for
   the whole pool, the card store over IPC, specs per requested card, the
   indexed card table. 7b: set crates, build-time registries, derived oracle
   exporters, and the rehearsal budgets met.
8. **AI knowledge and authoring tools.** Derived card knowledge and the per-deck
   overlay; `just card brief`, `just card new` and `just card check`; render and
   diff; the Forge census; the authoring pilot, in which a cheap model writes
   ten Legacy cards that today's mechanics can express, each from its brief.
   Cards that pass are kept.

The CI track (K18 to K21) has two steps.

1. **Measure and stop the bleeding.** The run log and `just ci-report`; inferred
   verdicts; the paused clock; rerun and quarantine. Then each existing check's
   record is filled in from the slow-CI log and from how often commits rewrote
   it, and the pruning rule is applied once.
2. **Select and bisect.** Coverage recording, selection in the fast check,
   oracles per changed card, bisection in `by ci`, and the weekly prune.
   Decomposed against the log as built.

The presentation track (K22 to K30) has two steps.

1. **Stage.** The rules still open settled from the recordings, and the
   design-language data; the skills moved to the Arena reference, which starts
   the banking of K30; the script type, verbs and marks; the session's script
   builder with inferred causes; the invariants; the stage behind a switch,
   first as one slice (cast a creature, its enters trigger, a token); the
   measuring tool and Arena's numbers; then every verb, and every shape once
   milestone 6's protocol has landed; the banked reports as cases; the cut-over,
   which deletes `beats.ts` and the old animation code.
2. **Causes from the interpreter.** After milestone 5: cause and simultaneity
   tags from effect data, and the inference deleted.

New cards and engine mechanics wait until milestone 5 lands. Mobile and AI
ladder work continue, and so does UI work outside K30.

## Acceptance

- **Parity.** All 47 cards in the new structure; the frozen saves replay
  exactly; `rollouts_never_change` holds; the count gate holds with no bump.

- **No one-card logic.** The card-names allowlist is empty, outside the expert
  system and the per-deck overlay.

- **No oversized files.** The file-length allowlist is empty.

- **Scale rehearsal.** Every in-scope card has a record in the card store. A
  scratch workspace of 50,000 implemented definitions, cloned from the real
  ones, meets these budgets on the development machine:

  | Measure                  | Budget |
  | ------------------------ | ------ |
  | Cold `cargo check`       | 60 s   |
  | Release build            | 180 s  |
  | Rebuild after one edit   | 2 s    |
  | Largest compiler process | 4 GiB  |

  App startup and the UI bundle do not grow with the card store. A budget that
  cannot be met after three attempts is recorded with its measured value and
  cause, and a follow-up bead is filed.

- **Checks.** Over the two weeks after the CI track lands, at most 2 Steward
  beads per 100 landed commits (12 today), with the fast check within 60 seconds
  for a UI change. Missed after three attempts, it is recorded with its cause
  and a follow-up bead is filed. Then, if selection misses are under 1 per 100
  commits, a follow-up moves the browser lanes' full run to nightly.

- **Presentation.** `beats.ts` and the per-component animation effects are gone,
  and no UI file is over 800 lines. The invariants hold over every scenario and
  the nightly fuzz. Every banked report passes as a stage case, and so do the
  five repeated themes: the trigger lift, arrows on moved cards, the combat
  line, paying with permanents and the scry flight. Every verb is within
  tolerance of Arena's numbers or is a listed exception. The two weeks measured
  for the checks bar above do not include the cut-over.

- **Reported, with no bar:** bug reports per game played and how many repeat a
  closed one; the share of failures by verdict; flakes and quarantined checks;
  checks deleted; seconds per lane; the paper test's four-way split; the
  render-match rate on the 47 cards; the pilot's pass count, cards blocked by
  missing vocabulary and tokens per card; the Forge census, with the missing
  mechanics ordered by cards blocked; the derived-only agent's results against
  the tuned one.

## Risks

- **The vocabulary cannot say enough.** Amounts, references and conditions
  ("that many", "the exiled card", "if you do") are where a card language turns
  into string variables. Milestone 2's paper test measures it on 60 cards before
  anything is built on the types.

- **The interpreter costs more than estimated.** Milestone 2 measures it before
  anything is built on it, and the run stops if the gate cannot hold.

- **Tracks collide.** Prompts and the kernel both edit `decision.rs`,
  `relayout.rs` and the ask sites in `effect.rs`. `enact push` rebases; the
  later push fixes the conflict.

- **The save corpus constrains prompt changes.** Milestone 6 ships a format
  migration; the old vocabulary is closed and small.

- **Selection misses.** Coverage is recorded nightly, so a check can depend on
  code written since. Slow CI still runs everything on every tip, and each miss
  is logged.

- **A verdict is inferred wrongly.** A fix that deletes an assertion to hide a
  bug reads as a stale test. The rule needs three such updates and no catch
  before it deletes, and a `keep` comment overrides it.

- **Dual APIs linger.** Old bodies and effect data coexist from milestone 2 to
  milestone 5. The card-names and file-length allowlists make the remainder
  visible.

- **One instance is thin evidence.** Most of the Arena findings behind the first
  rulings rest on one moment in one recording. Each verb is measured across
  recordings before its numbers are pinned, and a finding that contradicts a
  ruling files a `human` bead.

- **The tracker cannot isolate a card.** Arena's table is animated and busy. If
  frame differencing cannot follow a card, that verb's numbers are read once
  from filmstrips and recorded with how they were taken.

- **The stage is on the way to the final gate.** The cut-over needs milestone
  6's protocol, and the file-length allowlist cannot empty until the old files
  are deleted.

- **The old table changes under the stage.** UI fixes continue until the
  cut-over. K30 limits the churn; any other fix to the old table is repeated on
  the stage or it is lost.

## Measurements

Taken 2026-11-10 on the development machine (M5 Max), under load from other
sessions.

**Compile scaling.** 10,000 generated cards shaped like Counterspell, 75 lines
each, against today's `beryl_engine` and `beryl_scenario`.

| Layout                             | `cargo check` | Release build | One-card edit | Peak compiler memory |
| ---------------------------------- | ------------- | ------------- | ------------- | -------------------- |
| One crate, today's style           | 30 s          | 89 s          | 13 s          | 4.1 GiB              |
| 16 crates, today's style           | 6.5 s         | 7 to 12 s     | 0.9 s         | 1.5 GiB              |
| One crate, no function bodies      | 34 s          | 34 s          | 7.5 s         | 3.5 GiB              |
| One crate, no bodies, no scenarios | 9 s           | 8 s           | 4.8 s         | 1.4 GiB              |

A release binary linking all 10,000 cards with their scenarios is 27.5 MiB.

**Forge's vocabulary.** A census of mechanics, not a design input (K16).
Distinct effect types, static modes, trigger modes and replacement events across
34,134 scripts, ignoring parameters: 450 in all (195, 82, 139 and 34), plus 222
keywords.

| Most common entries | Scripts fully covered |
| ------------------- | --------------------- |
| 25                  | 56.6%                 |
| 50                  | 73.8%                 |
| 100                 | 88.0%                 |
| 200                 | 97.2%                 |
| 450                 | 100%                  |

Counted at the level a translator would have to map, the scripts are far wider.
Share of scripts fully covered by the most common entries:

| Counted                                                   | Distinct | Top 200 | Top 800 | Top 1,600 |
| --------------------------------------------------------- | -------- | ------- | ------- | --------- |
| Effect, mode and keyword names                            | 710      | 84.9%   | 100%    | 100%      |
| Plus parameter keys                                       | 1,888    | 44.1%   | 90.0%   | 99.3%     |
| Plus filter properties, references and computed variables | 6,199    | 23.9%   | 63.2%   | 82.8%     |

36% of scripts compute a value through a script variable, 28% chain
sub-abilities and 12% keep remembered state.

**Slow CI.** From the runner's log and the Steward beads of 2026-11-06 to
2026-11-10, over 280 landed commits.

| Lane failed in slow CI | Runs of 414 |
| ---------------------- | ----------- |
| `ui-test`              | 35          |
| `shots`                | 14          |
| Both                   | 8           |
| `oracle`               | 2           |
| `rust`                 | 1           |
| Any other              | 0           |

The 34 Steward beads, by the cause their close reasons give. Each took a median
of 12 minutes.

| Cause                                      | Beads |
| ------------------------------------------ | ----- |
| Test made stale by an intended change      | 12    |
| Test racing the wall clock or font loading | 9     |
| Real bug, mostly overlapping layout        | 8     |
| Host load or a cold cache                  | 2     |
| Unclear or still open                      | 3     |

`ui-test` is 212 checks in one 12,725-line file and takes 136 to 181 seconds,
the longest lane. All of its checks install a fake clock; 23 pause it.

**UI reports.** The 180 beads labelled `bug-report`, filed 2026-11-08 to
2026-11-10 from the app's bug button, sorted by reading each one.

| Kind                                   | Reports  |
| -------------------------------------- | -------- |
| Motion: where things come from, timing | about 45 |
| Static look and layout                 | about 43 |
| Prompt and interaction flow            | about 37 |
| Arrows and combat lines                | about 28 |
| The AI's plays                         | about 22 |
| The spec viewer                        | about 7  |
| Rules                                  | about 4  |

| Theme reported again            | Reports |
| ------------------------------- | ------- |
| A trigger lifts out of its card | 12      |
| Paying with permanents          | 9       |
| The scry flight                 | 7       |
| Arrows on cards that moved      | 6       |
| The combat line flickers        | 5       |

The reports state 63 standing rules. Checked against the Arena recordings, most
on a single instance: 27 are what Arena does, 20 differ, 7 have no Arena
equivalent and 9 could not be told. Of the 20, six stay as Beryl exceptions and
14 follow Arena. `ui/src` and `ui/scripts` gained 14,351 lines over the three
days.
