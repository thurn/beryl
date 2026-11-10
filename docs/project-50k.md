# Project 50k

*A plan to rebuild Beryl's engine, card and UI structure so it could hold every
known Magic card. Decided 2026-11-10 in a design interview, on an audit of the
code and on measurements taken that day. The work is tracked as an Enact epic in
beads; this document is the plan the epic's beads are written against.*

Beryl supports 47 cards. Its structure was built for that pool: one cards crate,
hand-kept registries, one Rust function per ability, and an enum variant or
state flag wherever an early card needed one. Project 50k replaces that
structure with one that would still work with the full Comprehensive Rules and
50,000 card definitions.

- **In scope:** architecture. Crates, state ownership, the card format, the
  shared rules library, the prompt protocol, the card data pipeline,
  verification, and the tools a fleet of cheap models needs to write cards.
- **Not in scope:** new rules features. Expanding what the engine can do is the
  next project, [rules-coverage.md](rules-coverage.md). No new cards are written
  here either.
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

**Compile time.** See [Measurements](#measurements). One crate of 50,000 real
card files would need about 53 GiB of compiler memory. Split into crates, the
same cards build in about 90 seconds.

## Decisions

| ID  | Decision                                                                                                                                             | Why                                                                                                                                |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| K1  | A card's behavior is typed effect data run by one interpreter. A Rust function is the escape hatch. Held to the count gate with no bump.             | Opaque bodies force hand-kept metadata on every card. Bodies run 10 to 20 times a game, so dispatch is about 0.02% of a rollout.   |
| K2  | Printed characteristics are generated from Scryfall for the whole pool. A card file holds abilities and scenarios under a generated header.          | Nobody types a cost or type line, every card exists as a stub, and keyword-only creatures need no authoring.                       |
| K3  | The bar for a card is tiered. Library effects carry hand-written scenarios; a card built only from them gets generated scenarios.                    | Hand-written scenarios for every card would be about 5.8 million lines and hours of oracle time.                                   |
| K4  | Struct fields hold only state the Comprehensive Rules give that thing. Everything a card creates lives in three stores: Effects, Memos and Trackers. | Each flag hand-rolls its own duration, expiry, layout conversion, save description and observation.                                |
| K5  | `EffectCtx` becomes free functions over `Game` and `Resolution`, in modules that mirror the Comprehensive Rules.                                     | An agent reading "Scry 2" or a rule number finds the file by name, and each file stays small.                                      |
| K6  | A kernel crate owns the state. Fields are private; other crates use typed mutators, queries and order-free sets.                                     | Ten of 17 logged bugs needed a chokepoint that nothing enforced. A crate boundary enforces it at compile time.                     |
| K7  | Card data says what the card says ("each opponent", "defending player"). The kernel's API is format-neutral. No multiplayer logic is written.        | Card data cannot be re-audited cheaply; the kernel refactor touches every two-player site anyway.                                  |
| K8  | Card files are grouped by first-printing set, in many crates, with registries generated at build time.                                               | A set is how work is batched; a one-card edit rebuilds one crate; parallel agents never edit a shared list.                        |
| K9  | Generic agents derive card knowledge from definitions. Hand-tuned numbers survive only as an optional overlay for supported decks.                   | A name-keyed table cannot cover the pool. The overlay keeps the WU mirror's strength, in the same category as the expert.          |
| K10 | Prompts are about ten shapes plus a closed list of purpose tags.                                                                                     | Half of today's prompt kinds serve one card. Shapes are built once; a new card needs no protocol or UI change.                     |
| K11 | The rewrite happens in place. At every push all cards work, frozen saves replay, rollouts are unchanged and the count gate holds.                    | Three machine checks already define "behavior did not change", which makes an unattended refactor safe.                            |
| K12 | The kernel, rules and card crates have no runtime dependencies. Tests adopt `proptest` and `insta`; tools use mature crates freely.                  | The hot path needs exact layout control. Fuzz failures need shrinking. Tooling should not hand-roll parsing and threading.         |
| K13 | A definition renders back to Oracle-style text, which is compared with Scryfall's.                                                                   | A cheap model's typical error is a plausible definition that differs from the card. Rendering makes that error mechanical to find. |
| K14 | The project ends at parity plus a scale rehearsal. No pilot authoring.                                                                               | Foundations first. The 47 ported cards are the test bed for the authoring tools.                                                   |
| K15 | Enact review: a Mason audit gates each milestone, one Warden per milestone, no Sage, one Archivist.                                                  | Parity is machine-checked. Architecture drift is what the checks cannot see.                                                       |
| K16 | Beryl moves to GPL-3.0-or-later and gains a translator from Forge's card scripts. Forge is a source of card data only, never of engine design.       | Forge's 34,134 scripts are a formal statement of nearly every card. Beryl is not a port of Forge.                                  |
| K17 | `just check` fails a source file over 800 lines. Today's offenders are on an allowlist that may only shrink.                                         | Appending to a big file is an agent's path of least resistance; conventions alone did not hold.                                    |

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
it is for (scry, discard, search) and tunes wording, sound and motion. The UI
builds each shape once, as physical cards. Crew, convoke and waterbend are one
shape.

### Card pool pipeline

- `just cards` generates characteristics for the whole pool and builds an
  indexed card store. The app serves records over IPC on demand.
- Subtypes, counter kinds and similar lists are generated from Scryfall's
  catalogs, not written by hand.
- A build script lists each set directory. No registry is kept by hand.
- Specs are generated for the card requested, not for every card at start.
- Oracle exporters derive each card's translation from its effect data.

### Verification

- Invariant and reference checks run in debug builds on every `apply`.
- Every scenario is re-run under permuted slot assignments.
- Bounded exhaustive search plays every decision sequence from scenario
  positions to a fixed depth, checking invariants at each state.
- `proptest` shrinks a failing fuzz game to a minimal sequence.
- Oracles run per changed card in CI. The full sweep runs nightly.
- Scenarios use basic lands and a small set of designated prop cards, so one
  card's change cannot break another card's tests.

### Authoring tools

1. **No model.** Vanilla and keyword-only creatures are complete from generated
   characteristics: 1,882 of Forge's 34,134 scripts.
2. **Translator.** A tool in `tools/` turns a Forge script into a Beryl
   definition where the mapping is clean, and declines otherwise.
3. **One cheap model, one card.** `just card brief` prints a packet: Oracle text
   and rulings, matching vocabulary entries, the nearest implemented cards, and
   the Forge script and XMage class as references. `just card check` builds one
   set crate and runs the generated scenarios on all three engines.
4. **Escalation.** A card the vocabulary cannot express is tagged with the
   mechanic it needs. A stronger model takes mechanics in order of cards
   unblocked.
5. **Render and diff.** Every definition, from any source, renders to text that
   is compared with Scryfall's.

Forge guardrails: the vocabulary is designed from the Comprehensive Rules, and
each entry is justified by its rule and the cards that need it. The translator
maps into it; the vocabulary never bends to a script. No Forge engine code is
ported or used as a design reference. A translated card cannot be checked by
Forge, so XMage and the render check are its independent checks.

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

Each lands in place under K11. A Mason audit gates the next.

1. **Guardrails.** The scale-rehearsal harness; the card-names test, seeded with
   the audit's inventory as a shrinking allowlist; the file-length check;
   invariants on every debug `apply`; permuted-slot scenario runs; shrinking;
   the conventions above.
2. **Effect data proof.** The `beryl_card` vocabulary, the interpreter, and the
   benchmark corpus's cards ported. The count gate holds at 405,276,238
   instructions with no bump. If it cannot after three attempts, the fallback is
   Rust bodies over the same shared library, and the plan continues.
3. **Kernel.** Private state, mutators, queries, order-free sets, index types,
   the format-neutral API, capacities as layout parameters.
4. **Stores.** Effects, Memos and Trackers; the one-card state fields and
   variants from the audit removed.
5. **Rules library and card port.** `EffectCtx` dissolved; all 47 cards as
   effect data; generated scenarios; prop cards.
6. **Prompts.** Shapes and purposes through engine, session, protocol and UI,
   with the save-format migration.
7. **Card pool.** Generated characteristics, set crates, build-time registries,
   the card store, derived oracle exporters, and the rehearsal budgets met.
8. **AI knowledge and authoring tools.** Derived card knowledge and the per-deck
   overlay; the relicense; `just card brief` and `just card check`; render and
   diff; the Forge translator, as a dry run.

New cards and engine mechanics wait until milestone 5 lands. UI, mobile and AI
ladder work continue.

## Acceptance

- **Parity.** All 47 cards in the new structure; the frozen saves replay
  exactly; `rollouts_never_change` holds; the count gate holds with no bump.

- **No one-card logic.** The card-names allowlist is empty, outside the expert
  system and the per-deck overlay.

- **No oversized files.** The file-length allowlist is empty.

- **Scale rehearsal.** Every in-scope card exists as a generated stub. A scratch
  workspace of 50,000 implemented definitions, cloned from the real ones, meets
  these budgets on the development machine:

  | Measure                  | Budget |
  | ------------------------ | ------ |
  | Cold `cargo check`       | 60 s   |
  | Release build            | 180 s  |
  | Rebuild after one edit   | 2 s    |
  | Largest compiler process | 4 GiB  |

  App startup and the UI bundle do not grow with the card store. A budget that
  cannot be met after three attempts is recorded with its measured value and
  cause, and a follow-up bead is filed.

- **Reported, with no bar:** the render-match rate on the 47 cards; the
  translator's agreement with the 47 hand-ported definitions; its coverage over
  all 34,134 scripts, with the missing mechanics ordered by cards blocked; the
  derived-only agent's results against the tuned one.

## Risks

- **The interpreter costs more than estimated.** Milestone 2 measures it before
  anything is built on it, with a fallback agreed.
- **The save corpus constrains prompt changes.** Milestone 6 ships a format
  migration; the old vocabulary is closed and small.
- **Dual APIs linger.** Old bodies and effect data coexist from milestone 2 to
  milestone 5. The card-names and file-length allowlists make the remainder
  visible.
- **GPL and app stores.** GPL version 3 is widely held to conflict with Apple's
  App Store terms. Installing to one's own phone is not distribution.
- **Forge's shapes leak in.** Each Mason gate checks for string-typed
  parameters, script-variable indirection and per-card AI hints.

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

**Forge's vocabulary.** Distinct effect types, static modes, trigger modes and
replacement events across 34,134 scripts, ignoring parameters: 450 in all (195,
82, 139 and 34), plus 222 keywords.

| Most common entries | Scripts fully covered |
| ------------------- | --------------------- |
| 25                  | 56.6%                 |
| 50                  | 73.8%                 |
| 100                 | 88.0%                 |
| 200                 | 97.2%                 |
| 450                 | 100%                  |
