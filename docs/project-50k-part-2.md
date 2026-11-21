# Project 50k, Part II

*The plan for the second half of Project 50k: an engine structure and a card
workflow that would still work with a full implementation of the Comprehensive
Rules and 50,000 card definitions. Decided 2026-11-15 in a design interview, and
revised 2026-11-20 when the work was planned again for Ignite. The tickets of
its epic are written against this document.*

Part I rebuilt the engine around a kernel crate, card behavior as effect data,
set crates and a card store. The rules keystones epic (planned in
[rules-coverage.md](rules-coverage.md)) then built the 24 cards that force the
hardest rules machinery. Part II does three things:

1. It checks that those keystone cards get every edge case right.
2. It builds 29 more keystone cards, chosen because each breaks an assumption
   the engine still makes about the shape of a game, a turn, a cast, combat or a
   card.
3. It changes the engine's structure and the card workflow so that both keep
   working as the pool grows: fewer classes of bug the compiler lets through,
   source files organised by the rule they implement, a cheaper way to write and
   test a simple card, and compile and check times that are measured.

This is an engine and tooling plan. The game UI is moving to Beryl 3D, so
nothing here builds UI.

**Folded in from the rules keystones epic.** That epic's closing architecture
audit and its documentation pass were closed unrun on 2026-11-15 and moved here,
so that one audit looks at the keystones as built and at what Part II is about
to change:

- The baseline audit (the first ticket of this plan) answers the six questions
  the closing audit was to answer: whether decisions R4 to R13 of
  [rules-coverage.md](rules-coverage.md) were built as decided; which variants,
  fields, branches and prompt purposes exist for exactly one keystone; whether
  each question of that plan's principle 7 is answered in one place; which five
  vocabulary entries the census says would unblock the most cards; the numbers
  (the count gate from that epic's first ticket to its last, `State<Small>`'s
  size, each family's present-cost ratio, the stall rate under random play); and
  that epic's acceptance, line by line. It rewrites "Where the engine stands" in
  that document from the code. Its list of one-card logic goes to the module
  passes (section C), which are where such logic is removed.
- This plan's documentation ticket retires both plan documents into the
  technical documents, and takes over the list that epic's documentation pass
  had: sections 4, 5, 7, 8, 9, 10, 13 and 20 of the architecture document, the
  two contracts, `rust/AGENTS.md` and [benchmarking.md](benchmarking.md) for the
  present-cost gate, and the census section.

## Status

Work on this plan began on 2026-11-15 and paused on 2026-11-17, when the project
moved its build, check and delivery system to Ignite. It was planned again on
2026-11-20 from the code as it then was.

Built by then:

- the baseline audit of the rules keystones;
- the rulings ledger, its check and `just card rulings`, with every ruling of
  the 49 first keystone cards a scenario or a recorded reason (section A);
- exhaustive matches and checked casts in `beryl_kernel` and `beryl_rules`,
  candidate order private to the kernel, and the typed-reference contract with
  its three pattern modules (section B);
- the `Implements:` declaration, the `rule-ownership` check,
  `just citations coverage` and the module map, the decision on `L: Layout`, and
  the first two module passes of the kernel (section C);
- the card workflow's tools: mutants for every part of a definition, the wider
  snapshot, generator stimuli and witnesses, the generated bar, the prelude
  functions, the brief that drafts, `just set list` and `just set plan`, and the
  rewritten `card-writing` skill (section E).

Not yet built: the same guards in `beryl_card` and the both-layout comparison;
the keystone pool; fifteen bugs the audits and the fuzzers found; the move of
the rules' files to their rule-owned places and the remaining module passes; the
conversion of the existing cards to the generated bar and the twelve new cards;
and all 29 new keystone cards.

What the second planning changed:

- **The keystones no longer wait for the module passes.** The files move first,
  in one chain of tickets that changes nothing but paths (the division). After
  it the module passes, the keystone families and the card workflow run
  together. A keystone ticket conflicts with a move, and not with the rest of a
  pass (P11, "Sequencing").
- **The keystone pool is played by Ignite's exploration checks** on idle cores,
  which file their own tickets for failures. No ticket runs a fixed number of
  games (section A).
- **Per-card tests are not sampled yet**, and **compile cost is measured by
  Ignite's rebuild counts** (P12, section D).
- **The twelve new cards are written with no trial of a cheaper model.** The
  pool has no such tier, so drafted cards are `tier:light` (section E).
- All 29 keystones stay, including subgames, banding and Illusionary Mask, which
  cover few cards (P5).

## Where things stand

Measured on 2026-11-15 at staging `bc14e0f8`, with the card files, card layouts,
census, bug log and state size checked on 2026-11-16 at staging `950e3c614`.

| What                                              | Measurement                                                                                                   |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Card files                                        | 121, median 123 lines                                                                                         |
| Lines of a card file that are its definition      | Median 28: 13 of code, 11 of doc comment, the rest header                                                     |
| Lines of a card file that are scenarios           | Median 85 from the scenarios module and its feature gate; 74.5% of all card-file lines                        |
| Card layouts implemented                          | Single-faced, transforming double-faced (Jace), and modal double-faced (Glasspool Mimic and Sink into Stupor) |
| Vocabulary                                        | 415 enum variants in `beryl_card`; `Effect` has 63                                                            |
| Pool cards the vocabulary fully covers            | 6,552 of 32,177 (20.4%), by `just census`                                                                     |
| Bug log                                           | 64 entries in [bugs.md](bugs.md)                                                                              |
| Wildcard `_ =>` match arms in the engine crates   | 259 line-leading arms outside tests (`beryl_kernel` 74, `beryl_rules` 96, `beryl_card` 89)                    |
| Integer `as` casts in the engine crates           | 158, with all four clippy cast lints switched off                                                             |
| Engine source files between 745 and 800 lines     | 16 of 287 across `beryl_kernel`, `beryl_rules` and `beryl_card`                                               |
| Rules functions that repeat `<L: Layout>`         | 644 of about 1,300 in `beryl_rules`                                                                           |
| `size_of::<State<Small>>()`                       | 20,240 bytes of 20,480                                                                                        |
| Cold `cargo check` of 50,000 cards (`just scale`) | 58.7 s against a 60 s budget (2026-11-12)                                                                     |
| Most expensive check in CI                        | `rust:clippy`, about 76 s a run over 1,081 runs                                                               |
| Per-card tests (`every_card_has_scenarios_...`)   | About 12 CPU-seconds a run at 121 cards                                                                       |

**Oracle text is not parsed.** The interview asked again whether a simple card's
definition could be read from its Oracle text. Normalising card names, numbers
and mana symbols and counting sentence templates over the 32,496 cards of the
card store:

| Measure                                                | Cards |
| ------------------------------------------------------ | ----: |
| Every sentence also appears on at least one other card |   33% |
| Every sentence appears on at least five cards          |   20% |
| At least one sentence that no other card has           |   66% |
| Fully covered by the 10,000 most common templates      |   45% |

Two thirds of cards have a sentence nobody else has, so a parser would need a
grammar of composition, not a table of templates, and phase.rs spends about
694,000 lines on one. Cards stay hand-written Rust files. The same count gives
the shape of the work: 14,619 cards have exactly one unique sentence and 11,149
have none. A typical card is shared clauses composed in one new way, which is
what the vocabulary models, so the cost of a card is in finding the clauses and
testing the result. The card workflow below attacks those two costs.

## Decisions

| ID  | Decision                                                                                                                                                                                                                                                                                              | Why                                                                                                                                                                                                                                       |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1  | A card is a hand-written Rust file. Oracle text is never parsed, at build time or run time.                                                                                                                                                                                                           | Measured above. Rust files passed the 50,000-card rehearsal and are type checked for free.                                                                                                                                                |
| P2  | A card needs no hand-written scenario when its generated scenarios kill every mutant of its definition and agree with XMage and Forge. Hand-written scenarios belong to vocabulary entries, written once, and to cards with rulings the generator cannot reach.                                       | Scenarios are 71% of card-file lines and three quarters of card compile cost. K3 already intended this; three tests and the `card-writing` skill still demanded hand-written ones.                                                        |
| P3  | A card file cites the Comprehensive Rules only where the card relies on something no vocabulary entry says. Rules code and vocabulary entries keep their citations.                                                                                                                                   | Half of a definition was quotes of rules the vocabulary entry already cites.                                                                                                                                                              |
| P4  | Part II builds the card workflow and checks it on about a dozen cards. No whole set is implemented.                                                                                                                                                                                                   | A set run is its own epic; this one stays about structure.                                                                                                                                                                                |
| P5  | Three families of new keystones are built, 29 cards: turn and game structure, casting and combat exceptions, and card shapes with face-down cards and cards from outside the game. The monarch, the initiative, day and night and voting are left to later set work.                                  | The three change engine structure. The fourth is mostly new store entries.                                                                                                                                                                |
| P6  | The first keystone cards are audited with a rulings ledger, which a check holds complete, and with fuzzing and exhaustive search over a pool of only those cards.                                                                                                                                     | Rulings are a bounded list of what each card must do (about 500 statements). Lockstep games against the oracles would need a decision bridge that is fragile on exactly these cards.                                                      |
| P7  | The compiler refuses the bug classes it can: no wildcard match arm over an enum, no lossy integer cast, no walk over a set in slot order outside the kernel, and object references typed by what they point at. Every scenario also plays in both state layouts.                                      | The table under "From the bug log to guards".                                                                                                                                                                                             |
| P8  | Each engine module declares the Comprehensive Rules it implements. A check holds a file's citations to that range and reports rules no module owns. The engine is reorganised once to match. The 800-line limit stays.                                                                                | About 30 files were cut at the limit instead of at a boundary. The same data is a coverage map of the rules.                                                                                                                              |
| P9  | Shared card helpers are prelude functions that compose vocabulary. Pass card-specific inputs as arguments and keep straightforward builder calls inline. The brief drafts definitions, and a set planner sorts a set.                                                                                 | Reusable builders give a name to a composition and can cover many cards with a single function.                                                                                                                                           |
| P10 | The engine reads the shape of a game from one `Format` value. Only two-player constructed exists. No other mode is built.                                                                                                                                                                             | Subgames and restarts need it anyway, and Commander, Planechase, Archenemy, Two-Headed Giant and DanDân are other values of it.                                                                                                           |
| P11 | The engine's files move to their rule-owned places first, in one chain of tickets that changes nothing but paths (the division). Each module then gets one pass that converts it to typed references and simplifies it, as two commits. Lines before and after are recorded. No line count is a gate. | Learning a module is most of what a session costs, so two trips would cost twice. A keystone ticket conflicts with a move and not with the rest of a pass, so only the moves come before the keystones. A line target invites dense code. |
| P12 | Compile cost is measured by Ignite's rebuild counts, and Ignite files the ticket for the crate that agents spend most on rebuilding. Every card's tests run on an engine change while the pool is small; sampling them waits until the pool is in the thousands.                                      | Per-card tests cost about 0.1 CPU-seconds a card: 13 seconds at 132 cards and over an hour at 50,000. Ignite reruns a check only when its inputs change.                                                                                  |
| P13 | New keystones have no UI bar. The session's views and animation scripts cover a new object state with existing prompt shapes and verbs, so the script checks keep passing.                                                                                                                            | The UI is moving to Beryl 3D.                                                                                                                                                                                                             |
| P14 | The performance bars of the rules keystones epic carry over: the count gate is never raised for a mechanism, a family's rollout step stays within 1.25 times the same step without it, and `State<Small>` stays within 20,480 bytes. Subgames live only in the `Large` layout.                        | A mechanic the WU mirror does not use must cost it nothing.                                                                                                                                                                               |
| P15 | Design, foundation, keystone and review tickets are `tier:opus`. The rulings ledger, partner cards, lint sweeps and measurements are `tier:sonnet`, and drafted cards are `tier:light`. Nothing is `tier:fable`.                                                                                      | The designs here come from the one session that planned this.                                                                                                                                                                             |
| P16 | Out of scope: dexterity cards, un-cards, Alchemy cards, and cards that only matter during a draft. Ante is designed below and not built. Omen and Station cards are left to set work.                                                                                                                 | The target is every other known card. Omens follow the adventure pattern and Stations are counters.                                                                                                                                       |

## From the bug log to guards

The 59 entries of [bugs.md](bugs.md), grouped by what would have stopped each
one. Counts are approximate because some entries belong to two classes.

| Class of bug                                                                                                     | Entries | Guard                                                                                       | State today                                                                                                         |
| ---------------------------------------------------------------------------------------------------------------- | ------: | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| A reader handled some kinds of a thing and skipped the rest (merged abilities, a rule change listed twice)       |       9 | No wildcard arm over an enum, so a new variant fails to compile at every reader             | 256 wildcard arms                                                                                                   |
| Slot order reached an event, a save or a tie-break                                                               |       9 | A walk in slot order cannot mutate; a per-member action takes an order the rules see        | `Bitboard::iter` is private to the kernel, but `State::in_candidate_order` hands the same iterator to 58 rule sites |
| A value was narrowed or widened wrongly between the `Small` and `Large` layouts                                  |       5 | No lossy `as` cast; every scenario plays in both layouts and must trace the same            | Cast lints off; one test compares the layouts on two seeds                                                          |
| Two functions answered one rules question differently                                                            |      10 | One named query per question, with the fields private; one module owns each rule            | The kernel's fields are private; the rules crate has parallel readers                                               |
| A cache or a flag went stale                                                                                     |       6 | Every cache compared with a recompute in debug builds                                       | Done for layers, listeners, knowledge and payments (`reference.rs`)                                                 |
| The wrong kind of object was assumed (a token counted as a card, a hidden card behind a copy, a spell that left) |       7 | A reference type that says what it points at, made only by a kernel query that checked      | One untyped `ObjectRef`; each rules function checks `is_current` itself                                             |
| A list ran out of room, or a push was ignored                                                                    |       5 | `discarded-push` (exists); a test that repeats each growing action past capacity            | The check exists; the test is by hand                                                                               |
| A paused resolution or step replayed differently                                                                 |       4 | The replay differential (exists), extended to each new place a pause can happen             | Exists for resolutions and steps                                                                                    |
| A card did less than its text                                                                                    |       4 | The `card-text` check and the mutation check (exist), widened to every part of a definition | Only the effects of spell, activated and triggered abilities are mutated                                            |

From Part II on, a bug's entry in the log names the guard that now stops its
class, or says why none can.

## Designs

Each section is the proposed design for one group of tickets. A group's first
ticket may amend its section where the code proves it wrong, and records the
change here in the same push.

### A. Auditing the first keystones

**The rulings ledger.** `just card brief` already prints Scryfall's rulings for
a card, from the bulk file cached under `$BERYL_CACHE/scryfall/` (79,706 rulings
for 20,080 cards). `data/rulings-ledger.json` records, for each card under
audit, each ruling by its date and a hash of its text, with one of:

- `scenario`: the names of the scenarios in the card's file or rules library
  that show it;
- `untestable`: why no scenario can show it (it restates reminder text, it is
  about a tournament procedure, it needs a card nobody has implemented and that
  card is named);
- `covered`: the vocabulary entry or rules-library scenario that shows it for
  every card.

The `rulings` check fails when a card listed in the ledger has a ruling with no
entry, an entry names a scenario that does not exist, or an entry's hash matches
no ruling. Like `just citations`, it is skipped where the bulk file is not
cached. `just card rulings <name>` prints a card's rulings beside their entries.
Cards join the ledger as they are audited: the 49 cards of the rules keystones
epic first, every keystone of this plan as it is written, and any later card a
writer chooses to list. A card not listed is not checked, so the ledger never
blocks a simple card.

**The keystone pool.** `just fuzz` already plays a quarter of its games with
decks dealt from one keystone family at a time (`decks::KEYSTONE_FAMILIES`,
`fuzz_deck_from`), so the cards of a family meet each other. Cards of different
families rarely meet: Humility with Gemrazer, Rest in Peace with Underworld
Breach, Painter's Servant with Mother of Runes. A pool of every keystone and
partner makes those pairings the common case. The fuzzers deal both decks from
it in a test of their own, and the exhaustive search starts from positions cut
from those games, with the invariants, the references and the both-layout
comparison (section B) at every decision. Ignite plays both as exploration
checks on idle cores and files a ticket for a failure ([ignite.md](ignite.md),
"Exploration checks"), so no ticket of this plan runs a fixed number of games. A
failure shrinks to a scenario (`beryl_search/tests/all/shrink.rs`). Each
keystone of this plan joins the pool with its card.

### B. Guards

**B1. Exhaustive matches.** `clippy::wildcard_enum_match_arm` is denied in
`beryl_kernel`, `beryl_rules` and `beryl_card`, by a `#![deny(..)]` at the top
of each crate's `lib.rs`, because a crate that inherits the workspace's lint
table cannot add to it in its `Cargo.toml`. Each of the 256 arms is rewritten to
name the variants it stands for. Where a reader really means "anything else", it
calls a predicate on the enum that is itself an exhaustive match
(`Effect::moves_objects`), so that a new variant is classified once, in the
vocabulary, beside its definition. Matches over integers, tuples of non-enums
and `Option` are not affected.

**B2. Conversions.** The four cast lints (`cast_possible_truncation`,
`cast_possible_wrap`, `cast_sign_loss`, `cast_precision_loss`) are denied in the
same three crates, the same way. A discriminant read is a `const fn` on the enum
(`Zone::index`); a narrowing is `try_from` with the failure handled, or
`layout::index::<L>`, which panics out of range; a count that must fit a byte
saturates and says so. Most of the 158 casts are discriminants.

**B3. Sets without an order.** The kernel owns `Bitboard::iter`. The public
`State` and `Observation` expose zone membership as sets and the order a zone
keeps.

- Reads combine independent contributions with queries such as `Bitboard::any`,
  `filter`, `fold` and `sum`.
- `Candidates::extend_options` builds an option list from an object set in the
  recorded choice order. Its callback can append several options for one card,
  as casting needs. `Candidates::objects` reads an existing decision and the
  kernel selects representatives of interchangeable objects.
- A per-object action takes `State::in_game_order` or a zone's order, such as
  battlefield entry order or the top of the stack. `SlotId::pool` still locates
  free storage for new tokens and spell copies. The seeded mulligan shuffle uses
  its historical input order inside the kernel, after reporting the hand's moves
  in card definition order. Automatic mana payments still choose between copies
  of one definition by slot. The kernel keeps that compatibility ordering in
  `State::mana_payment_sources`, since changing it moves the pinned rollouts.

The three slot numberings every scenario already plays under remain the test.

**B4. Both layouts.** The scenario runner plays on `State<Small>` only, and one
test compares the layouts on two seeds. `beryl_scenario` gains
`plays_alike_in_both_layouts`: the scenario is played on `Small` and on the
widened `Large` state in lockstep, and at every decision `small.widen()` must
equal the large state and the decisions must match after `relayout`.
`every_card_has_scenarios_and_they_pass` calls it beside the three slot
numberings, and the fuzzers call it at every decision of the keystone pool.

**B5. Typed references.** Today a rules function takes an `ObjectRef` (a slot
and an incarnation number that every zone move bumps), checks `is_current`
itself and does nothing if the reference is stale, and reads the zone at run
time. The kernel gains reference types that say what the object was when the
reference was made:

```rust
/// A permanent: on the battlefield at this incarnation.
pub struct Permanent<L: Layout = Small>(ObjectRef<L>);
/// A spell's object. Ability entries use stack places.
pub struct Spell<L: Layout = Small>(ObjectRef<L>);
/// A card (never a token or a copy) in one zone of one player.
pub struct CardIn<Z: ZoneMark, L: Layout = Small>(ObjectRef<L>, PhantomData<Z>);
/// A set of permanents; `Cards<Z, L>` likewise.
pub struct Permanents<L: Layout = Small>(Bitboard<L::Slot>);
```

- Only kernel queries make one, and each checks: `State::permanent(ObjectRef)`
  returns `Option<Permanent>`, `State::permanents()` returns `Permanents`, and
  `State::cards_in(player, Graveyard)` returns `Cards<Graveyard>` with tokens
  left out.
- An incarnation changes on every zone move. Trying to increment past 65,535
  stalls the game in either layout. `State::current(reference)` checks the
  incarnation and returns the proof the counter mutators take. Phasing keeps the
  same incarnation and requires a separate check for battlefield presence. Typed
  sets select slots for one instruction. For actions in sequence,
  `State::next_permanent` checks battlefield presence in battlefield order.
- Rule actions take the kind they act on. The first conversion covers
  destruction, permanent counters, spell removal, and paying exile costs. Kernel
  counter mutators accept any checked object because an ability can have a
  counter cost outside the battlefield. Exile payments take `Cards<Graveyard>`
  or `Cards<Hand>`.
- The interpreter turns a vocabulary `Objects` value into a typed set based on
  what the instruction needs. `Run::permanents` returns `Permanents`.
  `Permanents::slots` adapts it to the batch pipeline and the remaining
  unconverted callers.
- The types are transparent wrappers, so `State` does not grow and the count
  gate should not move. `Relayout` maps them like `ObjectRef`.
- The bare slots that bypass `ObjectRef` today (`Memo::ExiledWith`,
  `Memo::PartOf`, `Object::attached_to`, `AttackTarget::Planeswalker`, the slot
  in `Affects::Object`) are listed in the kernel contract with the reason each
  is safe, or become references.

The contract ticket lands the types, the queries and the conversion of the
kernel mutators and of three rules modules as the pattern (`zones.rs`,
`actions/destroy.rs`, `counters.rs`). The module passes of section C convert the
rest.

### C. Rule ownership and the module pass

**Declaring ownership.** A rules module's `//!` header gains one line the
citation tool reads:

```rust
//! Implements: CR 601, CR 602.2, CR 118.8
```

`just citations` already finds every citation in a file. The `rule-ownership`
check fails when:

- a file in the kernel's or the rules' `src` cites a rule outside the ranges it
  or its parent module declares, unless the citation is written as a
  cross-reference, with the word `see` before it;
- two modules declare the same rule;
- a module declares a rule it never cites.

A section or numeric subrule in the declaration claims its descendants. A child
module can cite rules declared in its own header or an enclosing module's
header. A module that implements no rule gives its reason in the same line, for
example `//! Implements: none. Holds scratch buffers.` Test files and card files
are outside the check.

`just citations coverage` lists every rule of the pinned release that no module
owns, by section, and writes the same to `$BERYL_CACHE/rule-coverage.json`. A
claim identifies the module to extend for rules under that claim. The list shows
where the engine still needs an owner.

**The module map.** The tickets name a pass by its row here. Paths are relative
to each crate's `src/`. Files keep their names and nesting under the destination
unless a named move or a directory move below says otherwise. A surviving test
stays in its test module. The table inventories the source trees at the start of
the passes.

The kernel groups its code by what it stores. Its modules declare rules where
they implement them directly. The kernel's zone mutator owns CR 400.7, so
`zones/` uses specific rules from section 400 and a cross-reference to CR 400.7.
The timestamp clock in `timestamps.rs` owns CR 613.7. The simultaneous-entry
ordering in `continuous/arrival_timestamps.rs` cites that rule as a
cross-reference. The layers declare the other parts of section 613 that they
implement. The rules' turn module handles the noncombat steps. Combat owns its
own steps, and the stack owns section 405. Land play belongs to casting's
special action module, while the turn's land limit stays in the turn module.
Delve stays beside the casting costs it adjusts. `keywords/` declares the
keyword rules it implements individually. Protection is in `replacement/`, and
phasing and mutate are in `objects/`. The pass labels group related rules for
navigation. A declaration claims only the sections or subrules that the module
implements.

#### `beryl_kernel/src/`

| Pass                                     | Destination | Existing source files                                                                                                                                                                                                                                                                                                                                                                                         |
| ---------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Object references and characteristics | `.`         | `abilities.rs`, `additions.rs`, `copiable.rs`, `defined.rs`, `definition.rs`, `derive.rs`, `elsewhere.rs`, `ids.rs`, `last_known.rs`, `lib.rs`, `object.rs`, `query.rs`, `reference.rs`, `stack.rs`, `subtypes.rs`, `timestamps.rs`, `stack/tests.rs`. Move `additions.rs` → `copiable/additions.rs`, `defined.rs` → `query/defined_power.rs`, `elsewhere.rs` → `query/zone_characteristics.rs`               |
| 2. State and mutators                    | `.`         | `bitboard.rs`, `board.rs`, `invariants.rs`, `mutate.rs`, `new.rs`, `order.rs`, `phasing.rs`, `player.rs`, `player_set.rs`, `players.rs`, `state.rs`, `turn.rs`, `two_player.rs`, `zone.rs`, `mutate/merge.rs`                                                                                                                                                                                                 |
| 3. Stores                                | `.`         | `chosen.rs`, `counters.rs`, `effects.rs`, `listener.rs`, `mana_memory.rs`, `memos.rs`, `repeats.rs`, `replacement.rs`, `rule_changes.rs`, `static_ability.rs`, `store.rs`, `trackers.rs`, `effects/{change.rs,control.rs,copy.rs,extra_turn.rs,on.rs,prevention.rs,regeneration.rs,sets.rs,store.rs,text.rs,tests.rs}`, `counters/tests.rs`, `memos/tests.rs`, `static_ability/tests.rs`, `trackers/tests.rs` |
| 4. Decisions and information             | `.`         | `asking.rs`, `cause.rs`, `chance.rs`, `decision.rs`, `event.rs`, `knowledge.rs`, `observation.rs`, `payable.rs`, `pending.rs`, `profile.rs`, `rng.rs`, `decision/collapse.rs`, `event/kinds.rs`                                                                                                                                                                                                               |
| 5. Layouts and table                     | `.`         | `filter.rs`, `fit.rs`, `fixed.rs`, `layout.rs`, `relayout.rs`, `table.rs`, `relayout/events.rs`, `table/{capability.rs,cards.rs,listeners.rs,tests.rs}`                                                                                                                                                                                                                                                       |

#### `beryl_rules/src/`

`cast/` becomes `casting/`, `layers/` moves to `continuous/layers/`, and
`replace/` becomes `replacement/`. `stack/trigger/` holds the trigger matcher.
`engine.rs` stays beside `engine/`, and owns priority where it advances or
passes it. The interpreter and decision helpers cite the steps they call through
to. The root `lib.rs` keeps the crate contract.

| Pass                                                                                                                            | Destination                         | Existing source files                                                                                                                                                                                                                                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 6. Game and turn (CR 103, CR 104, CR 119, CR 305.2, CR 500 to 505, CR 512 to 514, CR 703)                                       | `game/`, `turn/`                    | `ending.rs`, `game.rs`, `life.rs`, `start.rs`, `stops.rs`, `turn.rs`, `engine/paused_step.rs`. Move `ending.rs` → `game/ending.rs`, `game.rs` → `game/mod.rs`, `life.rs` → `game/life.rs`, `start.rs` → `game/start.rs`, `stops.rs` → `turn/stops.rs`, `engine/paused_step.rs` → `turn/paused_step.rs`, `turn.rs` → `turn/mod.rs`                   |
| 7. Casting and activating (CR 107.3b, CR 115, CR 116, CR 118, CR 305.1, CR 305.9, CR 307.1, CR 601, CR 602, CR 606, CR 702.66a) | `casting/`                          | `announced.rs`, `casting.rs`, `costs.rs`, `cast/{mod.rs,costs.rs,delve.rs,legal.rs,parts.rs,stack.rs,steps.rs,targets.rs,timing.rs}`, `engine/land.rs`. Move `casting.rs` → `casting/start.rs`, `costs.rs` → `casting/payment.rs`, `engine/land.rs` → `casting/land.rs`                                                                             |
| 8. Mana (CR 106, CR 107.4a, CR 605)                                                                                             | `mana/`                             | `mana/{add.rs,lookahead.rs,mod.rs,pay.rs,pool.rs,remembered.rs,solver.rs,sources.rs,units.rs}`                                                                                                                                                                                                                                                      |
| 9. Stack and triggers (CR 405, CR 603, CR 608)                                                                                  | `stack/`                            | `delayed_triggers.rs`, `resolve.rs`, `stack.rs`, `trigger.rs`, `trigger/elsewhere.rs`. Move `stack.rs` → `stack/mod.rs`, `trigger.rs` → `stack/trigger/mod.rs`, `trigger/elsewhere.rs` → `stack/trigger/other_zones.rs`                                                                                                                             |
| 10. Continuous effects (CR 611 to 613)                                                                                          | `continuous/`                       | `continuous.rs`, `entered.rs`, `rule_change.rs`, `text.rs`, `layers/{apply.rs,dependency.rs,elsewhere.rs,entering.rs,granted.rs,merged.rs,mod.rs,player_rules.rs,text.rs,together.rs}`. Move `continuous.rs` → `continuous/mod.rs`, `entered.rs` → `continuous/arrival_timestamps.rs`, `layers/elsewhere.rs` → `continuous/layers/other_zones.rs`   |
| 11. Replacement and prevention (CR 614 to 616, CR 702.16)                                                                       | `replacement/`                      | `event.rs`, `protection.rs`, `replace.rs`, `replace/{damage.rs,destruction.rs,draws.rs,enchant.rs}`. Move `replace.rs` → `replacement/mod.rs`                                                                                                                                                                                                       |
| 12. Combat and damage (CR 506 to 511, CR 120)                                                                                   | `combat/`                           | `combat.rs`, `damage.rs`, `combat/assignment.rs`. Move `combat.rs` → `combat/mod.rs`                                                                                                                                                                                                                                                                |
| 13. Zone changes and counters (CR 110, CR 111, CR 114, CR 121, CR 122, CR 400 to 408)                                           | `zones/`                            | `counters.rs`, `drawing.rs`, `emblems.rs`, `library.rs`, `tokens.rs`, `zones.rs`. Move `zones.rs` → `zones/mod.rs`                                                                                                                                                                                                                                  |
| 14. Copies and object changes (CR 607, CR 610, CR 702.26, CR 702.140, CR 707, CR 708, CR 712, CR 731)                           | `objects/`                          | `actions/copy.rs`, `face_down.rs`, `linked.rs`, `one_shot.rs`, `phasing.rs`, `keywords/mutate.rs`                                                                                                                                                                                                                                                   |
| 15. Keyword actions, keyword abilities, and coin flips (CR 701, CR 702, CR 705)                                                 | `actions/`, `keywords/`             | `actions/{attach.rs,counter.rs,destroy.rs,discard.rs,exile.rs,explore.rs,fateseal.rs,manifest.rs,manifest_dread.rs,mill.rs,mod.rs,regenerate.rs,reveal.rs,sacrifice.rs,scry.rs,search.rs,shuffle.rs,tap.rs,transform.rs}`, `coins.rs`, `keywords/{flashback.rs,mod.rs}`. Move `coins.rs` → `actions/coins.rs`                                       |
| 16. State machine, interpreter, and state-based actions (CR 117, CR 704)                                                        | `engine/`, `interpret/`, crate root | `bodies.rs`, `card.rs`, `choices.rs`, `chosen.rs`, `coverage.rs`, `engine.rs`, `lib.rs`, `reference.rs`, `resolution.rs`, `sba.rs`, `scratch.rs`, `engine/questions.rs`, `interpret/{mod.rs,run.rs,scene.rs}`, `tests/{invariants.rs,mod.rs,replay.rs,widths.rs}`. Move `choices.rs` → `interpret/choices.rs`, `chosen.rs` → `interpret/catalog.rs` |
| 19. Event and layer tests                                                                                                       | `tests/`                            | `tests/{abilities_away.rs,ability_removal.rs,delayed.rs,entering.rs,entering_together.rs,events.rs,land_types.rs,layer_inputs.rs,layer_sets.rs,many_layers.rs,replace.rs,static_abilities.rs,triggers.rs}`, `tests/events/{causes.rs,groups.rs}`                                                                                                    |
| 20. Casting and payment tests                                                                                                   | `tests/`                            | `tests/{any_target.rs,banned.rs,cast.rs,casting.rs,chosen.rs,life_costs.rs,payment.rs,remembered_mana.rs,spell_copies.rs,spell_costs.rs}`                                                                                                                                                                                                           |
| 21. Combat, library, and other tests                                                                                            | `tests/`                            | `tests/{combat.rs,effects.rs,game.rs,graveyard_return.rs,library.rs,mulligan.rs,phasing.rs,regeneration.rs,rules.rs,trackers.rs}`, `tests/combat/keywords.rs`                                                                                                                                                                                       |

**The division (P11).** The files move first. A chain of four tickets takes the
rows of the rules' table in order (6 to 8, 9 to 11, 12 to 15, then 16). Each
moves its files to their rule-owned places with `Implements:` headers and takes
them off the `rule-ownership` allowlist, and changes nothing else: no function
body, no signature and no test. A fifth ticket adds the headers to rows 3 to 5
of the kernel's table, where no file moves. The first two rows of the kernel
were done before the division. Every other ticket that changes `beryl_rules`
waits for the last ticket of the chain.

**One pass per module (P11).** Each ticket of the module pass then takes one row
of the table and makes two commits:

1. **Typed references.** The module's functions take the types of B5. The
   `is_current` test each function made for itself goes away where the type now
   carries it.
2. **Simplify.** Parallel readers of one rules question become one query. Dead
   parameters, wrappers that only forward and comments that restate the code go.
   A file that exists only because another reached 800 lines rejoins it or gets
   a name that says what it holds. Behavior is held by the scenarios,
   `just fuzz`, the frozen saves, `rollouts_never_change` and the count gate.

The passes run at the same time as the keystone families, which add to the same
modules. A pass changes a module's internals and a keystone adds to them, so
each rebases over the other.

The ticket's landing note gives the module's non-test lines before and after
each commit. A module whose simplification finds little says so; no number is a
target.

**The `L: Layout` parameter.** Keep free functions in their rule modules and
keep the engine entry points generic. The module passes follow the rules
contract in `beryl_rules/src/lib.rs`.

At `6780191a6`, 640 of 831 non-test declarations in `beryl_rules` named
`L: Layout`. Of those, 94 took `&mut Game` as the first argument, including 71
whose `game` parameter was on a line of its own. The method receiver still needs
that line in a multiline signature. Moving `destroy` and `destroy_all` into one
`impl<L: Layout> Game<'_, L>` took `actions/destroy.rs` from 90 to 93 lines
after formatting, with six signature lines in either version. Moving the other
methods would also need names that distinguish `tokens::create`,
`emblems::create` and `delayed_triggers::create` on one type.

The second prototype used the same methods and moved the engine bodies for
`next`, `next_in_step`, `next_within`, `apply` and `apply_decided` behind
concrete `Small` and `Large` functions. Generic callers selected their function
through `Layout::of`. All three runs passed the existing count gate. The
concrete entries increased the rollout count by 0.68% over the baseline.

| Prototype                                | Warm downstream check (s) | Rollout instructions |
| ---------------------------------------- | ------------------------: | -------------------: |
| Free functions                           |                      2.79 |          379,802,256 |
| Methods for destroy                      |                      2.51 |          379,802,256 |
| Methods and concrete engine entry points |                      3.57 |          382,373,878 |

The check timed `cargo check -p beryl_search -p beryl_session` after a change in
`beryl_rules`, once per prototype on a shared machine. The individual check
units for `beryl_search` and `beryl_session` took 0.43 and 1.07 s, 0.37 and 0.89
s, and 0.51 and 1.06 s, respectively.

`cargo llvm-lines --lib` reported 1,174,938 lines for `beryl_search` and 861,026
for `beryl_session` with free functions. With the concrete entry points, those
totals were 583,060 and 860,946. A baseline test build for both packages,
including dependencies and integration tests, took 148 s. The prototype took 156
s and used 333 s of user CPU, down from 474 s. It reused more dependencies and
waited for Cargo's build lock, so the command times are individual observations.
Cargo timed the `beryl_rules` library unit at 3.2 s before the change and 42.7 s
with it. The `beryl_search` library unit went from 53.4 to 43.2 s, and
`beryl_session` from 51.9 to 52.5 s.

**Rules tests.** `beryl_rules/src/tests/` is about 14,600 lines of tests that
define their own test cards and arrange boards through the kernel's test door,
because `beryl_scenario` sits above the engine. A test that only drives
decisions and reads the result is a scenario in disguise: it moves to
`beryl_cards/src/library/` as a hand-written scenario on a real card, where the
oracles also play it, and its test card is deleted. A test that calls an
internal function directly stays where it is. The reviews of the rules keystones
epic already listed which test definition each real card's scenario now covers.

### D. Compile and check time

- **Rebuild counts.** Ignite measures compile cost as the instructions the
  compiler executes to rebuild after one fixed edit ([ignite.md](ignite.md),
  "Rebuild counts"). It keeps the series, and files a ticket for the crate that
  agents spend most on rebuilding. A per-crate series of seconds was built for
  this plan first (`just efficiency compile`). The checks that wrote its data
  were removed with the move to Ignite, so it is removed too.
- **What was tried.** Non-generic engine entry points raised the rollout count
  by 0.68% and are not built (section C). Merging `beryl_search`'s test binaries
  into one saved nothing that could be measured. No ticket of this plan reduces
  compile time: Ignite's rule decides when a crate is worth it.
- **What an engine change tests.** Every card. At 132 cards
  `every_card_has_scenarios_and_they_pass`, the mutation check and the generated
  scenarios cost about 13 CPU-seconds together, and Ignite reruns them only when
  their inputs change. When the pool is in the thousands they become a check
  family with one check a card, as the oracle family already is. A change to one
  card then reruns that card, and an engine change runs the deck cards, the
  keystones and a sample fixed by the commit, with the rest played on idle
  cores.
- **The rehearsal follows the workflow.** `just scale` clones today's card
  files, scenarios and all. Once simple cards carry no hand-written scenarios
  (section E), the rehearsal's mix is three quarters cards without a scenarios
  module, as the pool will be, and its budgets are set again from that run.

### E. The card workflow

A card writer today reads the brief, reads vocabulary source to compose a
definition, writes scenarios by hand for every part of every ability, and quotes
a rule for each. The target is that a simple card costs a cheap model one short
session with no engine source read.

**E1. Mutants for every part of a definition.** The mutation check
(`beryl_cards/src/mutation/`) changes one node of a card's definition and
requires some scenario to tell the mutant from the card. It tests effects,
target optionality, static modifications and affected sets, replacement events
and rewrites, mana products, triggers, rule changes, activation and casting
costs, and face keywords. Some gameplay differences disappear before a scenario
ends. The mutation runner compares scripted positions as well as the final
oracle snapshot. The runner also checks a scheduled trigger against a standing
one when it can detach the schedule without losing carried resolution data. A
mana product neighbour must fit the payment engine's supported sources.

**E2. The bar, and the gates that change.** `just card check` reports, per card,
the mutants that generated scenarios alone kill. A card whose generated
scenarios kill them all, pass in at least one oracle, and have no unexpected
oracle failures is `generated`: its file has no `scenarios` module. Four gates
make this possible:

- the set crates' build scripts stop requiring a `SCENARIOS` constant in every
  card file (`beryl_card_build/src/generate.rs`);
- `every_card_has_scenarios_and_they_pass` counts generated scenarios;
- `every_ability_and_casting_option_is_exercised_by_a_scenario` reads generated
  and rules-library scenarios as well as the card's own;
- `every_entry_a_card_says_is_carried_out_by_a_hand_written_scenario` stays as
  it is: it is the gate that every vocabulary entry has a hand-written scenario
  somewhere.

A card with a surviving mutant gets a hand-written scenario for that mutant
only, or a witness when the mutant is of a vocabulary entry many cards share.
The single-card check runs the same generator used by the full sweep so it can
report the answer limit without generating scenarios for unrelated cards. Specs
and Playground use generated and rules-library scenarios when a card file
supplies none.

**E3. A generator that reaches more.** `derive/` plays player 0 acting at the
first main-phase priority, an attack, and steps through turn 4, from seven
hand-picked witnesses. Twenty-seven abilities are listed as unreachable
(`UNREACHED`). It gains:

- **Stimuli from the trigger and the replacement.** For each event a card
  listens to or replaces, the generator has a recipe that makes the event
  happen: the opponent attacks, either player casts, draws, discards, gains or
  loses life, a creature dies, the card itself leaves the battlefield, damage is
  dealt, a land is played or a permanent enters. A test and exhaustive matches
  hold every `TriggerEvent` and replaced event to a recipe. An ability can be
  exercised from the graveyard or exile. When an effect gives permission to cast
  from either zone, the recipe plays the granting effect and uses the permission
  at priority. The 16-run cap applies to each recipe opening.
- **Witnesses from neighbours.** A witness is a position fragment under which a
  field's value changes the outcome. The mutation test requires a generated or
  scripted scenario to distinguish a real mutant of each kind. The match that
  selects the scenario must cover each kind the mutation generator declares.
- **A wider snapshot.** The final snapshot, which the oracles compare, gains
  each permanent's colors, card types, subtypes, keywords, damage marked and
  attachment. It records power and toughness for every creature, identifying
  same-name permanents in battlefield entry order. It also records each mana
  pool and the names and controllers of spells and abilities on a nonempty
  stack. Beryl can take this snapshot at any open decision. XMage and Forge
  compare the new fields where their harnesses can stop for the final
  assertions.
- **Partners by filter.** An extra starting position keeps the full filter
  together, including ownership and bounds. It adds a permanent that passes and
  a contrasting one on the relevant side, reusing the board and preferring props
  before other implemented cards. The original positions still test play with
  fewer legal targets. The filter vocabulary describes printed types, subtypes
  and colors, among other qualities. Flying is a keyword.

`UNREACHED` and `EQUIVALENT` may only shrink.

**E4. Shared prelude functions.** A builder in `beryl_card/src/phrases.rs`
composes existing vocabulary where a named operation makes the card definition
easier to read. Accept arguments for details that vary by card:

```rust
/// When this permanent enters, do `effect`.
pub const fn when_enters(effect: &'static Effect) -> Ability {
    Ability::triggered(&Trigger::ENTERS).effect(effect)
}
```

- Keep simple expressions such as `draw(you(), 3)` inline. Use documented
  functions such as `instant_or_sorcery()` for common compositions.
- The brief's vocabulary search reads the builder's doc comment. The `card-text`
  check verifies the definitions that use it.
- Card writers may add a helper that combines existing vocabulary. Escalate
  clauses that need a new variant or interpreter arm.
- Shared tokens and scenario props stay in `beryl_card_shared`.

**E5. A brief that drafts.** `just card brief` prints a draft of the definition,
and `just card new` writes it into the file. The draft comes first from
[phase.rs](https://github.com/phase-rs/phase)'s published typed parse of every
card (`card-data.json`, MIT/Apache-2.0), translated node by node into vocabulary
entries and prelude functions by a table in
`tools/src/beryl_tools/phase_draft.py`. For a sentence that table cannot
translate, the brief looks for a matching rendered ability from an implemented
card. The fallback uses a single prelude-only ability on a plain printed face.
If it finds none, it prints the unresolved sentence and its three nearest
examples.

- **Nothing of Phase is built.** Python reads the JSON from `$BERYL_CACHE` at
  authoring time, pinned by a hash in `data/phase-data.json`. There is no Phase
  crate, no Cargo dependency and no card data in the repository, and a drafted
  card file is the same size as a hand-written one, so build and check times do
  not change. P1 still holds: Beryl parses no Oracle text.
- **The renderer checks the draft.** Phase drops or misreads some clauses on
  cards it reports as parsed, so a draft is only a draft until it renders back
  to the Oracle text. `card new` prints the render diff for the draft it wrote.
  Before writing, `card brief` shows the status of the card file if one exists
  or marks rendering pending.
- **A field the table does not read stops the draft**, naming the field, instead
  of dropping a clause silently.
- **When the draft is complete, the brief is short:** the Oracle text, the
  draft, each sentence's render status and the rulings. The vocabulary,
  nearest-card and XMage sections print only for sentences the draft does not
  cover.

A prototype on 2026-11-15 mapped about 45 of the 637 node kinds that Phase's
commons and uncommons use. It drafted 60 random commons and uncommons
completely. 52 of them compiled, and 33 rendered exactly to their Oracle text.
The other 19 named their difference in the render diff: 6 were the table's
wording and 13 were clauses Phase had dropped. A `drafted` card is then a fix-up
for a `tier:light` writer (E7).

**E6. The set planner.** `just set list <code>` prints the cards first printed
in a set. `just set plan <code>` sorts them:

| Tier         | Meaning                                                                       | Who writes it                          |
| ------------ | ----------------------------------------------------------------------------- | -------------------------------------- |
| `store`      | Complete from the card store (keyword-only creatures)                         | Nobody                                 |
| `drafted`    | Every sentence drafted by the brief                                           | A `tier:light` writer checks the draft |
| `composed`   | The keywords and layout are supported, with clauses to compose by hand        | A `tier:sonnet` writer                 |
| `vocabulary` | A missing Phase node kind or a clause with distant examples, grouped by cards | One `tier:opus` ticket a group         |
| `mechanism`  | A keyword, layout or rule the engine lacks                                    | A design ticket                        |

It writes `$BERYL_CACHE/set-plan-<code>.json` and prints the table. A card can
need several vocabulary groups. Cards with Phase parse gaps go through the
sentence fallback. The sentence cutoff is the lower quartile of the closest
matches between clauses on different implemented cards, measured when the
planner was added. A draft is complete when the drafter covers its clauses. The
table also reports any prior compile and render audit results for cards in the
set. Planning reuses the renderer built in the current checkout and the cached
census when available. `--tickets` prints commands for each vocabulary group and
batches of at most twenty drafted or composed cards.

**E7. The skill.** The `card-writing` skill is rewritten around the draft: run
the brief, check the draft against the Oracle text and the rulings, compose what
it could not draft using the existing vocabulary and builders, run
`just card check`, add a scenario only for a surviving mutant or a ruling, and
list the card in the rulings ledger when it has rulings worth holding. It keeps
its rules that a writer never edits the vocabulary's variants, the engine or
another card.

**E8. The proof.** The existing simple cards are converted first: each card
whose generated scenarios kill every mutant loses its hand-written scenarios,
except those the rulings ledger names, and its doc comment keeps only
card-specific rulings (P3). Then a dozen cards nobody has implemented are
written with the new skill: the first six of the `drafted` tier of
`just set plan` for one set by a `tier:light` session, and the first six of its
`composed` tier by a `tier:sonnet` session. A report ticket then gives, from the
efficiency store and by tier, tokens and minutes per card against the cards
written before, the share that passed `just card check` first time, the share
with no hand-written scenario, and the lines per card file. It names the model
each tier ran on. No cheaper model is tried: the pool has no tier for one.

### F. The shape of a game

`Format` is a `const` value the `GameTable` holds and the kernel and rules read,
in place of numbers written into `new.rs`, `start.rs` and `turn.rs`:

```rust
pub struct Format {
    pub seats: u8,                       // 2
    pub teams: Teams,                    // Teams::None
    pub starting_life: i16,              // 20
    pub opening_hand: u8,                // 7
    pub maximum_hand: u8,                // 7
    pub lands_each_turn: u8,             // 1
    pub mulligan: Mulligan,              // Mulligan::London
    pub first_draw: FirstDraw,           // FirstDraw::SkippedByStartingPlayer
    pub turn: &'static [TurnStep],       // the thirteen steps
    pub zones: &'static [ZoneRule],      // which zones are per player, which shared
    pub decks: &'static [DeckRole],      // DeckRole::Library, DeckRole::Sideboard
    pub loses: Losses,                   // zero life, a draw from an empty library, poison
}
impl Format { pub const CONSTRUCTED: Format = ..; }
```

- Only `Format::CONSTRUCTED` exists, and what subgames and restarts need of it
  (section G).
- `Seat` stays a two-value enum and `PerSeat` a two-entry array. `Format::seats`
  is read where a count was written as `2` (`chance::flip`'s `below(2)` for the
  starting player, the pass count), so that the places a third seat would break
  are the ones the compiler already lists (`two_player.rs`).
- `sba::end` stops assuming the game ends when one player loses: it asks the
  format whether the players left are one side. With two seats the answer is
  what it was.
- Everything a rollout reads from the format is a `const`, so the WU mirror's
  count should not move.

The table of what each other mode would still need is at the end of this
document, and the documentation ticket moves it to the architecture document.

### G. Turn and game structure

**The turn as a plan.** `turn::next_step` is a `match` over thirteen steps, and
several readers compare steps with `<` (`stops.rs`, `mana/lookahead.rs`, the
engine's `Limit`, `next_in_step`'s `(turn, step)` pair). The kernel's `Turn`
gains the rest of this turn as a plan: a short queue of steps still to come,
filled from `Format::turn` as the turn begins.

- An additional step or phase is an insertion into the plan at the place the
  rules give ([CR 500.8](https://yawgatog.com/resources/magic-rules/#R5008),
  [CR 500.9](https://yawgatog.com/resources/magic-rules/#R5009)); a skipped step
  or phase is a removal.
- A step is identified within its turn by its ordinal in the plan as played, not
  by its `TurnStep`, so a second upkeep and a second combat are distinct to the
  stops, the limits and the script. Readers that compared `TurnStep`s compare
  ordinals or ask a named query (`Turn::is_before_combat_damage`).
- A game whose cards cannot change the turn (`Capability::TurnStructure` absent)
  keeps the `match` and stores no plan, so the mirror pays nothing.
- The plan is a `Fixed` queue in `Small`. Its bytes come from the kernel
  contract's allotment table; a turn with more inserted steps than it holds is
  `Full`, which widens the game to `Large`.

**Ending the turn**
([CR 725.1](https://yawgatog.com/resources/magic-rules/#R7251)).
`turn::end_the_turn` follows the rule's steps: abilities that triggered and are
not yet on the stack cease to exist, every object on the stack is exiled
including the one resolving, state-based actions are checked with no priority,
combat ends, and the plan is cut to the cleanup step. It is called from inside a
resolution, so it is the first effect that removes its own stack entry mid-body:
the replay transaction of `resolve.rs` must commit a body that ends this way.
Time Stop and Sundial of the Infinite are its cards.

**Skipping a turn** is a replacement of beginning the turn (Time Vault), in the
replacement pipeline, which already asks optional replacements. Stasis skips a
step by removing it from the plan as the turn begins.

**Restarting** ([CR 728.1](https://yawgatog.com/resources/magic-rules/#R7281)).
`State::restart(table, starter, carried)` builds a new state as `State::deal`
does, from every card each player owns, with `starter` taking the first turn and
the `carried` cards put onto the battlefield before it under one player's
control. Nothing of the old state survives: no effects, memos, trackers or
counters. The session treats it as one update whose script is the existing reset
of the table.

**Subgames** ([CR 730.1](https://yawgatog.com/resources/magic-rules/#R7301)). A
subgame is a second state, played to its end while the main game waits.

- `State<Large>` gains `outer: Option<Box<State<Large>>>`. Starting a subgame
  boxes the main state into `outer` of a fresh state dealt from each player's
  library, over the same slots: a card's slot number is the same in both games,
  every slot that is not a library card of the main game is `Nowhere` in the
  subgame, and a token of the subgame may reuse a pool slot the main game also
  uses, because they are different states.
- `State<Small>` has no such field. A resolution that starts a subgame returns
  `Full`, which is how the session widens a game today.
- The engine's `next` and `apply` act on the innermost game. When it ends, the
  engine takes `outer` back, shuffles each main-game library (its cards never
  left it, [CR 730.5](https://yawgatog.com/resources/magic-rules/#R7305)), and
  gives the subgame's result to the paused resolution as the answer to the
  question it stopped at. Shahrazad's body is then ordinary effect data: play a
  subgame, and each player who did not win loses half their life.
- The state hash, `Relayout`, `fits` and `clone_state` cover `outer`.
  Determinization redeals hidden cards of both games.
- A rollout that reaches a subgame plays it to the end like any other decisions,
  under the same turn and decision limits.

### H. Casting and combat exceptions

**Casting inside a resolution.** A cast in progress is `Pending::Casting`, a
paused resolution is `Pending::Resolving`, and the state has one `pending`. A
paused resolution is also replayed from its start over the state as it was
before it began, so a cast that had committed anything would be lost or doubled.
Today's `Effect::MayCast` therefore only records an offer, made once the
resolution has finished, of one card.

The cast's steps (`cast/steps.rs`: announce, choose targets, compute the total
cost, pay) become functions that take each answer from an answer source:

- at priority the source is `Pending::Casting`, one committed step an answer, as
  now, so a rollout's decisions and the count do not change;
- inside a resolution the source is the resolution's own record
  (`Resolution::answer`), so the whole cast is part of the transaction and is
  replayed with it. The new spell goes on the stack above the resolving entry,
  and the body continues with its next instruction.

`Effect::MayCast` loses the rule that it must be the last instruction
(`Malformed::Cast`) and may offer a set of cards. The record holds 32 answers in
`Small`; a longer one is `Full`.

- **Cascade**
  ([CR 702.85a](https://yawgatog.com/resources/magic-rules/#R70285a)) is then
  effect data: exile from the top until a nonland card of lesser mana value,
  cast it without paying its mana cost, put the rest on the bottom in a random
  order.
- **Panglacial Wurm** adds a way to cast to the search's own question.
  `choices::pick_or_end` offers `Opt::Cast` for a card of the searched library
  whose definition says it may be cast while its owner searches, and the search
  goes on after the cast.
- **Knowledge Pool** and **Possibility Storm** are triggers on a spell cast from
  a hand that exile it and cast another, free. They need the cast trigger to
  know the zone the spell was cast from.

**Who assigns combat damage.** `combat/assignment.rs` asks the active player to
divide an attacker's damage, never asks about a blocker's (a creature blocking
several attackers deals it all to the first), and has no double strike.

- `division_decision` takes its chooser from one query, `combat::assigner`,
  which names the player the rules give for an attacker and for a blocker.
- A blocker that blocks several attackers divides its damage (Palace Guard is
  the partner card).
- Double strike
  ([CR 702.4b](https://yawgatog.com/resources/magic-rules/#R7024b)) is a keyword
  bit and a second read of `first_strike_step` (Fencing Ace).

**Banding** ([CR 702.22j](https://yawgatog.com/resources/magic-rules/#R70222j),
[CR 702.22k](https://yawgatog.com/resources/magic-rules/#R70222k)) is then three
additions behind `Capability::Banding`:

- attackers are declared in bands: `Opt::Attack` gains the band an attacker
  joins, and the bands of this combat are a short list of sets in the combat
  state, kept only while the capability is present;
- a creature that blocks one member of a band blocks them all;
- `combat::assigner` answers the defending player for an attacker blocked by a
  bander, and the active player for a blocker of a bander.

"Bands with other" (Master of the Hunt's Wolves) is a band whose membership rule
is a quality instead of the keyword.

### I. Card shapes, face-down cards and cards outside the game

A `CardDef` already holds one or two faces, casting already loops over faces and
the protocol's cast decision already carries a face. Nothing says what kind of
two-faced card a card is, only a test card uses a second castable face, and the
engine cannot turn a permanent over.

**A shape on the definition.** `CardDef` gains `shape: Shape`, generated from
the card store's `layout` with the faces:

```rust
pub enum Shape { Single, Transforming, Modal, Split { fuse: bool }, Adventure,
                 Prepared, Flip, Meld { with: CardName, into: CardName } }
```

`check` refuses a face count the shape does not allow. The rules read the shape
where they branch on it; nothing infers it from `Face::transformed`.

| Shape or structure                                                        | What it needs                                                                                                                                                                                                       | Keystone                  |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| Split ([CR 709.4](https://yawgatog.com/resources/magic-rules/#R7094))     | Off the stack, `State::copiable` answers with both halves combined: two names, both sets of types and colors, the sum of the mana values. On the stack it is the half that was cast.                                | Fire // Ice               |
| Fuse                                                                      | A casting option that casts both halves as one spell: one cost, the left half's instructions then the right's.                                                                                                      | Wear // Tear              |
| Adventure ([CR 716.3](https://yawgatog.com/resources/magic-rules/#R7163)) | Cast as the Adventure face; as it resolves it is exiled with a permission to cast the creature from exile.                                                                                                          | Bonecrusher Giant         |
| Prepared ([CR 723.1](https://yawgatog.com/resources/magic-rules/#R7231))  | A designation on the permanent, kept as a memo; while it has it, its controller may cast a copy of the back face's spell, which removes the designation.                                                            | Adventurous Eater         |
| Modal double-faced                                                        | A real card for the path the test card "Test Stupor" uses, and oracle exporters that can cast a chosen face.                                                                                                        | Valakut Awakening         |
| Transforming, flip                                                        | `actions::transform` and `actions::flip` turn a permanent in place: the face changes, the object and its incarnation do not, and it gets no new timestamp. An `Effect::Transform` instruction.                      | Delver of Secrets, Erayo  |
| Meld                                                                      | Two permanents leave to exile and return as one object whose definition is the melded card, with both cards under it as merged permanents keep theirs (`Memo::PartOf`). It splits into its cards when it leaves.    | Gisela, the Broken Blade  |
| Saga ([CR 715.2](https://yawgatog.com/resources/magic-rules/#R7152))      | `Ability::chapter(n)`: a triggered ability on the lore counter reaching `n`. A lore counter as it enters and after each draw step is a turn-based action; sacrifice after the last chapter is a state-based action. | The Eldest Reborn         |
| Class ([CR 717.3](https://yawgatog.com/resources/magic-rules/#R7173))     | `Ability::level(n, cost)`: an activated ability at sorcery speed that sets the level, and abilities that exist from a level.                                                                                        | Wizard Class              |
| Case ([CR 720.3](https://yawgatog.com/resources/magic-rules/#R7203))      | A "to solve" condition checked at the controller's end step, a solved designation as a memo, and abilities that exist once solved.                                                                                  | Case of the Crimson Pulse |
| Leveler ([CR 711.2](https://yawgatog.com/resources/magic-rules/#R7112))   | Level up as an activated ability that adds a level counter; abilities and power and toughness that hold for a range of level counters, as static abilities with a condition.                                        | Student of Warfare        |
| Prototype ([CR 719.3](https://yawgatog.com/resources/magic-rules/#R7193)) | A casting option that gives the spell, and the permanent it becomes, another cost, color, power and toughness, as copiable values.                                                                                  | Arcane Proxy              |

**Face-down spells**
([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)).
`CastingOption::Morph(cost)` casts the card face down: `become_spell` takes
`FaceIx::DOWN`, the spell is a 2/2 creature spell with no name that only its
controller may look at, and it enters face down. Turning it up is the special
action that exists for manifested cards (`face_down.rs`), paying the morph cost,
which may be a non-mana cost (Gathan Raiders). A face-down card is revealed when
it leaves the stack or the battlefield and when the game ends. Illusionary Mask
lets any creature card be cast this way and turns it up with a replacement when
it would deal or be dealt damage.

**Cards outside the game**
([CR 400.11](https://yawgatog.com/resources/magic-rules/#R40011)). The
`GameTable` holds, per player, the cards of the `DeckRole::Sideboard` list as a
multiset of card indices. They have no slots.

- A wish (Burning Wish) asks its controller to pick one that matches, as a
  search of a hidden set does, and brings it in: the card takes a free slot of
  the pool as a card, not a token, and is removed from the multiset.
- A companion
  ([CR 702.139a](https://yawgatog.com/resources/magic-rules/#R702139a)) is
  declared before the game: the starting procedure asks each player with a
  sideboard card whose companion condition their deck meets, as a decision
  before the mulligans. The condition is a `Filter` over the deck's cards,
  checked by the table. Putting it into the hand is a special action that costs
  {3}.
- Scenarios set a sideboard with `.sideboard(P0, &[..])`. The session's
  best-of-three match and sideboarding between games are not part of this plan.

## The keystone cards

| Group               | Keystone                  | Partners                                        |
| ------------------- | ------------------------- | ----------------------------------------------- |
| G. Turn and game    | Time Stop                 | Sundial of the Infinite                         |
| G                   | Paradox Haze              | Stasis                                          |
| G                   | Relentless Assault        |                                                 |
| G                   | Time Vault                | Final Fortune                                   |
| G                   | Karn Liberated            |                                                 |
| G                   | Shahrazad                 |                                                 |
| H. Casting          | Bloodbraid Elf            |                                                 |
| H                   | Panglacial Wurm           | A card that searches a library, already written |
| H                   | Knowledge Pool            | Possibility Storm                               |
| H. Combat           | Benalish Hero             | Benalish Infantry, Palace Guard, Fencing Ace    |
| H                   | Master of the Hunt        |                                                 |
| I. Shapes           | Fire // Ice               |                                                 |
| I                   | Wear // Tear              |                                                 |
| I                   | Bonecrusher Giant         |                                                 |
| I                   | Adventurous Eater         |                                                 |
| I                   | Valakut Awakening         |                                                 |
| I                   | Delver of Secrets         |                                                 |
| I                   | Erayo, Soratami Ascendant |                                                 |
| I                   | Gisela, the Broken Blade  | Bruna, the Fading Light; Brisela                |
| I                   | The Eldest Reborn         |                                                 |
| I                   | Wizard Class              |                                                 |
| I                   | Case of the Crimson Pulse |                                                 |
| I                   | Student of Warfare        |                                                 |
| I                   | Arcane Proxy              |                                                 |
| I. Face down        | Exalted Angel             | Gathan Raiders                                  |
| I                   | Illusionary Mask          |                                                 |
| I. Outside the game | Burning Wish              |                                                 |
| I                   | Lurrus of the Dream-Den   |                                                 |

Mana matters: only Island, Plains and Swamp are basic lands with files, so the
first ticket of each family that needs {R} or {G} writes Mountain's and Forest's
scenarios' missing producer, as the card-writing skill already directs.

## Designed, not built

- **Ante.** A zone per player that `Format::zones` would list; `Zone` has no
  variant for it. Cards that mention ante are not written.
- **The repeat shortcut**
  ([CR 733.2](https://yawgatog.com/resources/magic-rules/#R7332)). Unchanged
  from [rules-coverage.md](rules-coverage.md): the session replays a recorded
  run of choices N times.
- **Shared zones.** `ZoneKey`'s rustdoc says how one joins; `Format::zones` is
  where a format would ask for it.
- **A third seat.** `two_player.rs` says what breaks.
- **Bringing an outside card into a subgame.** The subgame's table holds no
  sideboard.
- **Omen and Station cards**, and the designations (the monarch, the initiative
  and dungeons, day and night) and votes.
- **A session match.** Best of three and sideboarding between games.

## Bars and validation

Every ticket of the epic holds these, and says so in its landing note where it
measured one. A ticket runs each command through Ignite
(`ignite run -- <command>`), and `ignite check` runs the checks a landing needs.

| Check               | What it proves                                                                                | Command                           |
| ------------------- | --------------------------------------------------------------------------------------------- | --------------------------------- |
| Absent cost         | The WU mirror's rollouts cost what they did; the gate is never raised for a mechanism         | `just count-gate`                 |
| Present cost        | Each step of a rollout with a family in play costs within 1.25 times the same step without it | `just count-gate --family <name>` |
| State size          | `size_of::<State<Small>>()` stays within 20,480 bytes                                         | compile-time assertion            |
| Saves               | The frozen saves replay and `rollouts_never_change` holds                                     | `just test-rust`                  |
| Card scenarios      | Each ability does what its text says, generated or hand-written                               | `just card check <name>`          |
| Mutation check      | Every one-node change to a definition changes what some scenario shows                        | `just test-rust`                  |
| Oracles             | Scenarios agree with XMage and Forge, or the divergence is recorded with its rule             | `just oracle <cards>`             |
| Rulings             | Every ruling of a card in the ledger is a scenario or says why not                            | the `rulings` check               |
| Both layouts        | A scenario traces the same in `Small` and `Large`                                             | `just test-rust`                  |
| Rule ownership      | A file cites only the rules its module declares                                               | the `rule-ownership` check        |
| Fuzzing, exhaustion | Long random games, and every short line from a scenario's end, break no invariant             | `just fuzz`, `just exhaust`       |
| Script checks       | Each keystone's spec moments pass from both seats with existing verbs and prompt shapes       | `beryl_cli script-check`          |

A ticket whose change cannot hold the count gate after three attempts records
the counts and what each attempt tried on the ticket and on the epic, and is
given back for the owner's decision. The tickets that depend on it wait, and
independent work goes on. A family's present-cost miss after three attempts is
recorded and files a follow-up.

## Sequencing

Revised 2026-11-20. Under the first order every module pass and an audit came
before any keystone, which put about twenty refactoring tickets ahead of the
first new card.

1. **Fixes carried over** start at once: the bugs that the rulings audits and
   the fuzzers found. They depend on nothing.
2. **The division** starts at once and runs as a chain: four tickets move the
   rules' files to their rule-owned places, and one adds the kernel's remaining
   `Implements:` headers. An audit of the division follows it and gates nothing.
3. **Beside the division**, since they do not touch the rules' paths: the guards
   that are left (the `beryl_card` lint sweep, the both-layout comparison), the
   keystone pool, the conversion of the existing cards to the generated bar, the
   twelve new cards, and the passes over the card vocabulary and the renderer.
4. **After the division, in parallel**:
   - the module passes, one ticket a row of the module map, and the rules tests
     that become scenarios;
   - the three keystone families. Each starts with its foundation (`Format` and
     the turn plan; casting inside a resolution and `combat::assigner`; `Shape`,
     sagas, face-down spells and the sideboard), then its keystones, then its
     present-cost measurement and its agents ticket.
5. **A review of each area** follows the tickets it reviews: the guards, the
   keystone audit, the card workflow, the module passes in four groups, and each
   keystone family in one or two.
6. **A closing audit** of the whole against this plan, and then the
   documentation ticket, which retires this document into the architecture
   document.

## Risks

- **Passes and keystones collide.** After the division, a module pass and a
  keystone ticket can change the same module at once. Each pass owns a disjoint
  set of files, a pass changes internals where a keystone adds to them, and
  `ignite land` rebases. A pass whose module a foundation ticket is rebuilding
  (the cast's steps, the chooser of a damage division, comparisons of turn
  steps) leaves that part to the foundation ticket, and its ticket says so.
- **The division is not only paths.** A move changes the order in which code is
  compiled, which can shift the count by layout alone. Each division ticket
  reports the count before and after, and the audit that follows adds them up.
- **Typed references cost instructions.** They are transparent wrappers, but a
  proof that is checked where it was once assumed is a branch. Only the count
  shows it, and each pass reports it.
- **The generator cannot reach "every mutant" for most cards.** Then P2 covers
  few cards and the workflow's gain comes from the draft and the prelude
  functions. The conversion of the existing cards (E8) measures the share, and
  the twelve new cards measure it again on cards nobody has written.
- **A helper changes what its callers render.** Card text checks the complete
  definition against its Oracle text, including the composed effects and
  targets.
- **A cast inside a resolution changes the replay model.** The replay
  differential test (inline answers against replayed ones) is extended to a
  resolution that casts before any card depends on it.
- **Subgames in search.** A rollout that enters a subgame is long. The turn and
  decision limits bound it, and the present-cost measurement of the family
  reports it.
- **The oracles do not play these cards.** XMage and Forge may not implement
  subgames, banding or every layout. A scenario an oracle cannot play is
  reported as unsupported, and the rulings ledger is then the only outside
  check, which is why every keystone of this plan joins it.

## What each other mode would still need

For the architecture document, once `Format` exists. "Value" means a `Format`
constant with no new code.

| Mode             | Values of `Format`                                     | Still to build                                                                                                                                         |
| ---------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Commander        | 40 life, 100-card decks, a command zone                | A third and fourth seat; a player leaving the game; attacking several players; the command zone and the commander tax, damage and color identity rules |
| Two-Headed Giant | Teams of two, 30 life a team                           | Four seats; a shared turn and shared life; team blocks                                                                                                 |
| Planechase       | A planar deck role, the planar die as a special action | Plane and phenomenon cards as objects in the command zone                                                                                              |
| Archenemy        | Teams of one against many, a scheme deck role          | More seats; scheme cards                                                                                                                               |
| DanDân           | One shared library and graveyard, 20 life              | The shared `ZoneKey` variant; "your library" as a question separate from ownership                                                                     |
