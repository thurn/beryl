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
  two contracts, `engine/AGENTS.md` and [benchmarking.md](benchmarking.md) for
  the present-cost gate, and the census section.

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
- **Drafted cards go to `tier:light`.** The Theros pilot ran both tiers on
  GPT-6.2 Sol (section E8).
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
| Cold `cargo check` of 50,000 cards (`just scale`) | 127.5 s under load against a 140 s held limit, 295 CPU-s (three quarters of clones without scenarios)         |
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
`every_card_has_scenarios_and_they_pass` calls it for written, rules-library and
generated scenarios. A scenario whose deal exceeds the small layout plays in the
large layout. The random-game and keystone fuzzers compare both layouts at every
decision of one in sixteen games, until a game outgrows the small layout.

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
kernel mutators and of three rules modules as the pattern (`zones/mod.rs`,
`actions/destroy.rs`, `zones/counters.rs`). The module passes of section C
convert the rest.

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

A section or numeric subrule in the declaration claims its descendants. The word
`only` after a number claims that rule's own text and none of its subrules, for
a rule whose subrules belong to other modules. The kernel's zone mutator
declares `CR 400.7 only, CR 400.7a`: it makes a moved object new and carries a
permanent spell's text changes over. The prevention store declares CR 400.7c,
because it moves a spell's shields to the permanent the spell becomes. The
interpreter declares CR 400.7j, because its later instructions find an object
that an earlier one moved. The exceptions no module implements yet stay in the
coverage list. A child module can cite rules declared in its own header or an
enclosing module's header. A module that implements no rule gives its reason in
the same line, for example `//! Implements: none. Holds scratch buffers.` Test
files and card files are outside the check.

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
they implement them directly. The kernel's zone mutator owns CR 400.7 without
its exceptions, so `zones/` uses specific rules from section 400 and a
cross-reference to CR 400.7. The timestamp clock in `timestamps.rs` owns CR
613.7. The simultaneous-entry ordering in `continuous/arrival_timestamps.rs`
cites that rule as a cross-reference. The layers declare the other parts of
section 613 that they implement. The rules' turn module handles the noncombat
steps. Combat owns its own steps, and the stack owns section 405. Land play
belongs to casting's special action module, while the turn's land limit stays in
the turn module. Delve stays beside the casting costs it adjusts. `keywords/`
declares the keyword rules it implements individually. Protection is in
`replacement/`. That directory also handles the entry choice for an Aura (CR
303.4f and CR 303.4g) and dredge (CR 702.52a). Phasing, mutate and turning a
manifested permanent face up are in `objects/`. Manifesting and transforming
stay with their keyword actions. Combat's damage module handles lifelink and
choosing the source a shield prevents damage from. The zone modules handle
tokens that enter attacking and token copies of double-faced permanents. The
pass labels group related rules for navigation. A declaration claims only the
sections or subrules that the module implements.

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
`engine.rs` stays beside `engine/` and owns priority. `engine/questions.rs`
handles a cast offered during resolution, choosing a land face and decisions
made under another player's control. `sba.rs` performs state-based actions and
ends games after concessions or simultaneous losses. The stack resolver owns
target legality and the final steps of resolution. `interpret/` owns instruction
order, simultaneous effects, choices during resolution and reading last-known
information. The root `lib.rs` keeps the crate contract.

| Pass                                                                                                                            | Destination                         | Existing source files                                                                                                                                                                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 6. Game and turn (CR 103, CR 104, CR 119, CR 305.2, CR 500 to 505, CR 512 to 514, CR 703)                                       | `game/`, `turn/`                    | `ending.rs`, `game.rs`, `life.rs`, `start.rs`, `stops.rs`, `turn.rs`, `engine/paused_step.rs`. Move `ending.rs` → `game/ending.rs`, `game.rs` → `game/mod.rs`, `life.rs` → `game/life.rs`, `start.rs` → `game/start.rs`, `stops.rs` → `turn/stops.rs`, `engine/paused_step.rs` → `turn/paused_step.rs`, `turn.rs` → `turn/mod.rs`                           |
| 7. Casting and activating (CR 107.3b, CR 115, CR 116, CR 118, CR 305.1, CR 305.9, CR 307.1, CR 601, CR 602, CR 606, CR 702.66a) | `casting/`                          | `announced.rs`, `casting.rs`, `costs.rs`, `cast/{mod.rs,costs.rs,delve.rs,legal.rs,parts.rs,stack.rs,steps.rs,targets.rs,timing.rs}`, `engine/land.rs`. Move `casting.rs` → `casting/start.rs`, `costs.rs` → `casting/payment.rs`, `engine/land.rs` → `casting/land.rs`                                                                                     |
| 8. Mana (CR 106, CR 107.4a, CR 605)                                                                                             | `mana/`                             | `mana/{add.rs,lookahead.rs,mod.rs,pay.rs,pool.rs,remembered.rs,solver.rs,sources.rs,units.rs}`                                                                                                                                                                                                                                                              |
| 9. Stack and triggers (CR 405, CR 603, CR 608)                                                                                  | `stack/`                            | `delayed_triggers.rs`, `resolve.rs`, `stack.rs`, `trigger.rs`, `trigger/elsewhere.rs`. Move `stack.rs` → `stack/mod.rs`, `trigger.rs` → `stack/trigger/mod.rs`, `trigger/elsewhere.rs` → `stack/trigger/other_zones.rs`                                                                                                                                     |
| 10. Continuous effects (CR 611 to 613)                                                                                          | `continuous/`                       | `continuous.rs`, `entered.rs`, `rule_change.rs`, `text.rs`, `layers/{apply.rs,copied.rs,dependency.rs,elsewhere.rs,entering.rs,granted.rs,merged.rs,mod.rs,player_rules.rs,text.rs,together.rs}`. Move `continuous.rs` → `continuous/mod.rs`, `entered.rs` → `continuous/arrival_timestamps.rs`, `layers/elsewhere.rs` → `continuous/layers/other_zones.rs` |
| 11. Replacement and prevention (CR 303.4f, CR 303.4g, CR 614 to 616, CR 702.16, CR 702.52a)                                     | `replacement/`                      | `event.rs`, `protection.rs`, `replace.rs`, `replace/{damage.rs,destruction.rs,draws.rs,enchant.rs}`. Move `replace.rs` → `replacement/mod.rs`; its replacements of how a permanent enters (CR 614.12) are `replacement/entering.rs`                                                                                                                         |
| 12. Combat and damage (CR 506 to 511, CR 120)                                                                                   | `combat/`                           | `combat.rs`, `damage.rs`, `combat/assignment.rs`. Move `combat.rs` → `combat/mod.rs`                                                                                                                                                                                                                                                                        |
| 13. Zone changes and counters (CR 110, CR 111, CR 114, CR 121, CR 122, CR 400 to 408)                                           | `zones/`                            | `counters.rs`, `drawing.rs`, `emblems.rs`, `library.rs`, `tokens.rs`, `zones.rs`. Move `zones.rs` → `zones/mod.rs`                                                                                                                                                                                                                                          |
| 14. Copies and object changes (CR 607, CR 610, CR 702.26, CR 702.140, CR 707, CR 708, CR 712, CR 731)                           | `objects/`                          | `actions/copy.rs`, `face_down.rs`, `linked.rs`, `one_shot.rs`, `phasing.rs`, `keywords/mutate.rs`                                                                                                                                                                                                                                                           |
| 15. Keyword actions, keyword abilities, and coin flips (CR 701, CR 702, CR 705)                                                 | `actions/`, `keywords/`             | `actions/{attach.rs,counter.rs,destroy.rs,discard.rs,exile.rs,explore.rs,fateseal.rs,manifest.rs,manifest_dread.rs,mill.rs,mod.rs,regenerate.rs,reveal.rs,sacrifice.rs,scry.rs,search.rs,shuffle.rs,tap.rs,transform.rs}`, `coins.rs`, `keywords/{flashback.rs,mod.rs}`. Move `coins.rs` → `actions/coins.rs`                                               |
| 16. State machine, interpreter, and state-based actions (CR 117, CR 704)                                                        | `engine/`, `interpret/`, crate root | `bodies.rs`, `card.rs`, `choices.rs`, `chosen.rs`, `coverage.rs`, `engine.rs`, `lib.rs`, `reference.rs`, `resolution.rs`, `sba.rs`, `scratch.rs`, `engine/questions.rs`, `interpret/{mod.rs,run.rs,scene.rs}`, `tests/{invariants.rs,mod.rs,replay.rs,widths.rs}`. Move `choices.rs` → `interpret/choices.rs`, `chosen.rs` → `interpret/catalog.rs`         |
| 19. Event and layer tests                                                                                                       | `tests/`                            | `tests/{abilities_away.rs,ability_removal.rs,delayed.rs,entering.rs,entering_together.rs,events.rs,land_types.rs,layer_inputs.rs,layer_sets.rs,many_layers.rs,replace.rs,static_abilities.rs,triggers.rs}`, `tests/events/{causes.rs,groups.rs}`                                                                                                            |
| 20. Casting and payment tests                                                                                                   | `tests/`                            | `tests/{any_target.rs,banned.rs,cast.rs,casting.rs,chosen.rs,life_costs.rs,payment.rs,remembered_mana.rs,spell_copies.rs,spell_costs.rs}`                                                                                                                                                                                                                   |
| 21. Combat, library, and other tests                                                                                            | `tests/`                            | `tests/{combat.rs,effects.rs,game.rs,graveyard_return.rs,library.rs,mulligan.rs,phasing.rs,regeneration.rs,rules.rs,trackers.rs}`, `tests/combat/keywords.rs`                                                                                                                                                                                               |

**Where to implement the remaining rules.** This map gives a rule its first
place to look. Declare only the rules the code there implements. A type or
characteristic belongs to the card vocabulary, its stored value to the kernel,
and its game procedure to the rules module that performs it.

| Comprehensive Rules | Home in the engine                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 100–104             | Game format, setup and outcomes in `game/` and the kernel's game table and initial state. The interpreter handles APNAP choices (CR 101.4).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| 105–114, 200–213    | Characteristics in `beryl_card`, with queries in the kernel and derived characteristics in `continuous/layers/`. Mana goes to `mana/`, spells and abilities to `stack/`, and permanents, tokens and emblems to `zones/`. Printed illustrations and other information below the text box are printing metadata.                                                                                                                                                                                                                                                                                                                       |
| 115–118             | Target and cost choices in `casting/`. The engine offers priority and routes special actions to their implementation, such as land play or turning a permanent face up.                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| 119–123             | Life in `game/life.rs`, damage in `combat/damage.rs`, draws and counters in `zones/`, and sticker placement in `objects/` if added.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 300–315             | Type and subtype vocabulary in `beryl_card`. Spell timing and loyalty activation go to `casting/`. Attachments go to `actions/attach.rs` and `replacement/enchant.rs`. Attacking and protecting battles go to `combat/`. Put the state and procedures for dungeons and variant objects in `zones/` and the corresponding game procedures in `game/`.                                                                                                                                                                                                                                                                                 |
| 400–408             | Zone identity in the kernel and zone-specific behavior in `zones/`. Stack order is in `stack/`. A rule that carries information across a move belongs to the procedure using it, such as a trigger or casting permission.                                                                                                                                                                                                                                                                                                                                                                                                            |
| 500–514             | Turn structure in `turn/`. Combat owns its steps (CR 506–511), phasing at untap stays in `objects/phasing.rs`, and the kernel queues extra turns.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| 600–616             | Casting and loyalty activation in `casting/`, triggered abilities and resolution in `stack/`, and mana abilities in `mana/`. Static abilities and continuous effects go to `continuous/`, linked abilities and one-shot object changes to `objects/`, and replacement and prevention to `replacement/`. Effect execution, choices and last-known information go to `interpret/`.                                                                                                                                                                                                                                                     |
| 700–706             | General effect choices in `interpret/`, keyword actions in `actions/`, keyword abilities beside the procedures they change or in `keywords/`, turn-based actions in the owning step, state-based actions in `sba.rs`, and coin flips and die rolls in `actions/` using the kernel's chance API.                                                                                                                                                                                                                                                                                                                                      |
| 707–723             | Card shapes in `beryl_card` and characteristics in the kernel. Copies, face-down objects, flip and meld go to `objects/`, with individual keyword actions under `actions/`. Spell-side choices for split, Adventure, prototype and Omen belong in `casting/`. Sagas, Classes, Cases, levelers and Station use the turn, activation, trigger, layer, counter and state-based procedures each rule modifies. Substitute cards are printing and deck data. Attractions use a supplementary deck in `game/` and a step action in `turn/`. Preparation's designation belongs to the object and its casting permission goes to `casting/`. |
| 724–734             | Player control, shortcut decisions and illegal-action recovery in the engine. Ending a turn goes to `turn/`, restart and subgames to `game/`, and merging to `objects/`. The monarch, initiative and day/night use game state, with their changes applied by `actions/` or the relevant turn step. Rad counters use `zones/counters.rs` and the precombat main step.                                                                                                                                                                                                                                                                 |
| 800–811, 900–905    | Variant setup and participation in `game/` and the kernel's format. Variant changes to turn order, combat, zone movement or priority belong to the module that performs each procedure.                                                                                                                                                                                                                                                                                                                                                                                                                                              |

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
oracles also play it. Delete its test card when no remaining test uses it. Tests
of engine internals stay where they are, including tests of Beryl's payment
profiles, candidate ordering, and replay state. Opening decisions stay in
whole-game tests because scenarios and their oracle replays start with opening
hands already kept. The reviews of the rules keystones epic list which test
definition each real card's scenario covers.

### D. Compile and check time

- **Rebuild counts.** Ignite measures compile cost as the instructions the
  compiler executes to rebuild after one fixed edit. Ignite keeps the series,
  and its `slow-crate` rule files a ticket based on what agents spent rebuilding
  a crate. See [Rebuild counts](ignite.md#rebuild-counts).

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

- **The rehearsal follows the workflow.** `just scale` clones three of every
  four cards from card files with no `scenarios` module and the fourth from
  files that keep one, as the pool will be (section E). Two runs of
  `just scale --edits 8` on the same tree and machine, 50,000 clones in 68 set
  crates, one with every clone's scenarios and one with the mix. The machine was
  shared with other sessions both times: load average 14 to 19 on 10 cores for
  the first and 13 to 17 for the second.

  | Measure                  | All scenarios       | Three quarters none |
  | ------------------------ | ------------------- | ------------------- |
  | Cold check               | 269.2 s (550 CPU-s) | 127.5 s (295 CPU-s) |
  | Release build            | 80.7 s (216 CPU-s)  | 57.9 s (178 CPU-s)  |
  | One-edit rebuild, median | 2.0 s               | 1.8 s               |
  | Largest compiler process | 1.93 GiB            | 1.04 GiB            |
  | `target` after build     | 11.8 GB             | 7.6 GB              |

  The wall times are inflated by the load, so read the CPU-seconds: the mix
  halves the cold check's CPU cost. The cold check still misses the plan's 60 s
  on this machine under that load, so `BUDGETS` holds it to 140 s, the run's
  127.5 s plus 10%; the plan's 60 s stays as the target for an idle machine. The
  other three budgets are met and unchanged. The script checks of the 182 pooled
  cards take 9.2 s, and 50,000 clones of that mix would take about 6,400
  CPU-seconds of them.

  `every_card_has_scenarios_and_they_pass` was at the tip failing on Jace, the
  Mind Sculptor's generated scenarios, so its timing is that of a failed run:
  2.4 s over 182 cards, which is 13 ms a card and about 670 s for 50,000 cards
  at one thread. Treat it as a lower bound until the test passes.

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

A generated kill counts toward this bar only when the oracles could see it: the
mutant fails a generated scenario or ends it with a different final snapshot. A
generated scenario has no assertions of its own, so a difference only at a
scripted position (which land paid a cost, what a mana ability added before the
pool emptied, which hidden card a search found) would let a definition with that
neighbour's value pass every oracle check too. A Leonin Scimitar written with
"Equip {0}" did exactly that on 2026-11-25; only its render diff failed. The
mutants that generated scenarios kill only out of the oracles' sight are listed
in `ORACLE_BLIND` (`beryl_cards/src/mutation.rs`), 29 of them on 16 cards on
2026-11-25: activation costs, mana products, library searches and Time Spiral's
untap of six lands. The list may only shrink, and `just card check` names a
card's entries as "unconfirmed by oracles". The full mutation test still counts
every difference Beryl can see.

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

**E8. The proof.** Under P3, existing simple cards whose generated scenarios
kill every mutant keep only the hand-written scenarios named in the rulings
ledger and the doc comments specific to their rulings. The new-card pilot worked
through the first six unimplemented candidates in each of the `drafted` and
`composed` tiers from `just set plan ths`. Both workers used Codex with GPT-6.2
Sol. The results describe the two workflows on this sample.

| Measure                                     | Drafted (`tier:light`) | Composed (`tier:sonnet`) | Median before 2026-11-20 |
| ------------------------------------------- | ---------------------: | -----------------------: | -----------------------: |
| Candidates                                  |                      6 |                        6 |                          |
| Cards landed                                |                      2 |                        4 |                          |
| Escalated for missing vocabulary            |                      0 |                        2 |                          |
| Handed off for exporter or scenario work    |                      4 |                        0 |                          |
| Drafts correct as written                   |                 5 of 6 |                   0 of 6 |                          |
| Passed the fast card check on its first run |                 4 of 6 |                   0 of 4 |                          |
| Passed the full card check on its first run |                 2 of 4 |                   4 of 4 |                          |
| Landed with no hand-written scenario        |                 2 of 2 |                   1 of 4 |                          |
| Median lines per landed card file           |                     14 |                     32.5 |                          |
| Tokens per candidate                        |           4.80 million |             6.76 million |                          |
| Direct dollars per candidate                |                  $0.59 |                    $0.85 |                          |
| Agent minutes per candidate                 |                    7.4 |                     11.8 |                          |
| Tokens per landed card                      |          14.41 million |            10.14 million |             8.58 million |
| Direct dollars per landed card              |                  $1.76 |                    $1.27 |                    $1.29 |
| Agent minutes per landed card               |                   22.3 |                     17.7 |                     12.0 |

First-run pass rates use the cards that reached each check. The fast check uses
`--no-oracles`. Draft correctness includes the Crackling Triton wording pin,
which preserved the draft's behavior. The four drafted handoffs were two XMage
scry export failures and two gaps in generated coverage. Costs use the whole
ticket, including handoffs, escalations, the drafted-tier routing fix and the
landing checks. Token totals include cache reads and writes. Direct dollars are
modeled from API prices.

Tickets closed before 2026-11-20 added 84 card files. The historical median
covers the 11 files from five tickets with session data in the Multivac
efficiency store. Each file gets an equal share of its ticket's cost. Both pilot
tiers used more tokens and agent minutes per landed card than that median.
Direct dollars per landed card were close to the median for composed cards and
higher for drafted cards.

The pilot does not show that most new cards need no hand-written scenario. Three
of the six landed cards passed the mutation and oracle checks using generated
scenarios alone, including both landed drafts. The other three needed
hand-written scenarios. Two more drafts were handed off for focused scenarios,
so five of the eight candidates with a determined scenario requirement needed
one. The other four were blocked by exporter or vocabulary gaps. A review on
2026-11-25 also found that two of the three generated-only cards, Asphodel
Wanderer and Crackling Triton, had activation-cost mutants that only Beryl could
tell apart (see E2 and `ORACLE_BLIND`). P2's rule is sound as a condition. On
this sample, the condition held fully for one card in eight.

For the next hundred cards, start with a generated scenario in which a different
permanent enters and the source's own enters ability stays off the stack. The
same missing case held up Horizon Scholar, Ashen Rider and Blood-Toll Harpy, and
contributed to the coverage gaps for Chronicler of Heroes. Give the oracles'
snapshot a fact that shows what a cost or a mana ability spent, so that the
entries of `ORACLE_BLIND` can go. Repair XMage's generated scry export so a
correct draft can complete oracle checking. Both changes let future cards reuse
the fix instead of paying for a scenario or an exporter investigation on each
card.

### F. The shape of a game

`GameTable` holds the format for one game. The kernel's `new.rs` and the rules'
`game/start.rs` and `turn/mod.rs` read its defaults. `Format::CONSTRUCTED` is
the sole value and supplies its rules through constant getters:

- Two seats, with each player on their own side. A random seat chooses play or
  draw, and the starting player skips their first draw.
- Starting life of 20, an opening hand and hand limit of seven, the London
  mulligan, and one land play each turn.
- The turn's thirteen possible steps, the per-player and shared zones, and the
  library and sideboard roles of a player's decklists.
- Loss at zero life or less, when drawing from an empty library, or at ten
  poison counters. The format records the poison threshold for when the engine
  tracks poison.

`Seat` and `PerSeat` still represent two seats. The starting-player draw and the
priority pass count read `Format::seats()`. The format decides whether a loss
leaves only one side in the game.

The format value takes no space. Rules on a rollout's path compile to constants
so the WU mirror's instruction count stays the same.

The table of what each other mode would still need is at the end of this
document, and the documentation ticket moves it to the architecture document.

### G. Turn and game structure

**The turn as a plan.** `next_step` in `turn/mod.rs` matches thirteen steps, and
several readers compare steps with `<` (`turn/stops.rs`, `mana/lookahead.rs`,
the engine's `Limit`, `next_in_step`'s `(turn, step)` pair). The kernel's `Turn`
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
- A game whose cards cannot change the turn keeps the direct step transitions. A
  custom body that can edit the queue declares `changes_turn_structure`, which
  sets `Capability::TurnStructure` for the game.
- The plan is a 24-entry `Fixed` queue in `Small`. When an insertion would
  exceed the queue's capacity, `Full` lets the session retry in `Large`.
- A pass to a scheduled stop uses the queue entry's identity, so insertions and
  skips ahead of it can change its ordinal without changing the target.
- Existing choice records use the turn and step name. Planned games also record
  the ordinal as played, so replay can distinguish choices in the first and
  second occurrence of a step.

**Ending the turn**
([CR 725.1](https://yawgatog.com/resources/magic-rules/#R7251)).
`turn::end_the_turn` follows the rule's steps: abilities that triggered and are
not yet on the stack cease to exist, every object on the stack is exiled
including the one resolving, state-based actions are checked with no priority,
combat ends, and the plan is cut to the cleanup step. The replay transaction in
`stack/resolve.rs` detaches the resolving entry before running its instructions.
The resolving spell is still in the stack zone, so ending the turn exiles it
with the other spells. The transaction commits at cleanup, without running later
instructions or giving priority for a normal resolution. Time Stop and Sundial
of the Infinite use this effect.

**Skipping a turn** replaces the start of a scheduled turn. A replacement choice
pauses and replays through the same mechanism as a draw step's dredge choice.
Skipping a regular turn advances the regular order. Skipping an extra turn
consumes the scheduled turn and any delayed ability tied to it. Effects that
wait for the next turn a player actually takes keep waiting. Time Vault untaps
at the start of the next turn that actually begins. Final Fortune's delayed
ability is scheduled with its extra turn and expires if that turn never has an
end step.

A continuous rule that skips a beginning-phase step removes it just before it
would begin. Stasis removes untap at the start of each turn while its rule is in
force.

**Enchanting a player.** Paradox Haze adds an upkeep immediately after the
enchanted player's first upkeep. The turn plan records whether the step that
just began is a repeat, including when another rule removed an earlier step. An
Aura can attach to a player when it resolves or enters without being cast.
Protection can make the attachment illegal, and the same state-based action that
handles illegal object attachments puts the Aura into its owner's graveyard. In
two-player Constructed, a player leaving ends the game immediately under
[CR 104.2a](https://yawgatog.com/resources/magic-rules/#R1042a). If a future
format can continue when a player leaves, the attachment check already rejects a
player outside the game's player set.

**Restarting** ([CR 728.1](https://yawgatog.com/resources/magic-rules/#R7281)).
`State::restart(table, starter, carried)` builds a new state as `State::deal`
does, from the physical cards in the game, including cards admitted from a
sideboard. The resolving ability's controller is the starting player. Carried
cards stay in exile through mulligans and enter together under the starting
player's control before untap, using the same replacement-choice record as
turn-based actions. Effects, memos, trackers and counters begin empty. An outer
game stays suspended if a subgame restarts.

The turn limit starts again. The session dates checkpoints by turns elapsed
across the playthrough and replays to a post-restart checkpoint when recovering.
At the restart boundary, both seats get the table's existing reset script.

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
  gives the subgame's result to the paused resolution as an entry in its replay
  record. The library shuffle happens at that instruction when the effect
  replays, so any earlier instructions read the original position. Shahrazad's
  body can then read the result and have each player who did not win lose half
  their life.
- The state hash, size checks, layout conversion and cloning cover `outer`. A
  determinization samples hidden cards in the active and suspended games while
  keeping each game's recorded knowledge. For an ordinary subgame, it samples
  the cards that entered it from the parent deck with the evidence available in
  either game.
- The turn limit counts turns across the active, completed and suspended games.
  Entering a subgame spends one decision from the parent turn, and the subgame's
  opening decisions share that budget until its first turn.
- Saves record the subgame's random starting player and the board of every
  suspended game at a checkpoint. Recovery within a subgame replays the log to
  that checkpoint to recover the suspended resolution and its answers.

**Agents.** A search measures time in turns started across the playthrough
(`State::elapsed_turns`), because a turn's number starts again in a restarted
game and in a subgame. The turn search scores the lines of the root's turn up to
the turn after it by that count. A rollout cut off after some turns is never cut
off in a subgame: it plays the subgame out and stops at the first decision back
in the main game. The turn search finishes its lines the same way, and an
evaluating leaf in a subgame is rolled out instead. So no search compares a
subgame's position with a main game's. The rollout policy declines to skip a
turn for Time Vault's untap, because that untap waits for the next turn that
actually begins, and two players who each skipped would skip every turn after.
Card estimates count an extra turn as two points of `turns`, an additional
combat as one, and ending the turn as a counterspell.
`beryl_search/tests/all/game_structure.rs` plays seeded games of the `turn` and
`subgames` families with the greedy agent, the expert and MCTS.
`beryl_session/tests/game_structure.rs` cuts a save at each new state of each
keystone and replays it under every perturbation. The oracle exporters report a
subgame's decisions as unsupported. A step repeated in its turn is exported, and
the scenarios the harnesses misplace are listed in
`data/oracle-exporter-gaps.json`.

### H. Casting and combat exceptions

**Casting inside a resolution.** A cast in progress is `Pending::Casting`, a
paused resolution is `Pending::Resolving`, and the state has one `pending`. A
paused resolution is also replayed from its start over the state as it was
before it began, so a cast that had committed anything would be lost or doubled.
Today's `Effect::MayCast` therefore only records an offer, made once the
resolution has finished, of one card.

The cast's steps (`casting/steps.rs`: announce, choose targets, compute the
total cost, pay) become functions that take each answer from an answer source:

- at priority the source is `Pending::Casting`, one committed step an answer, as
  now, so a rollout's decisions and the count do not change;
- inside a resolution the source is the resolution's own record
  (`Resolution::answer` in `resolution.rs`), so the whole cast is part of the
  transaction and is replayed with it. The new spell goes on the stack above the
  resolving entry, and the body continues with its next instruction.

`Effect::MayCast` loses the rule that it must be the last instruction
(`Malformed::Cast`) and may offer a set of cards. The record holds 32 answers in
`Small`; a longer one is `Full`.

- **Cascade**
  ([CR 702.85a](https://yawgatog.com/resources/magic-rules/#R70285a)) is then
  effect data. Exile from the top until a nonland card of lesser mana value.
  Offer a cast without paying its mana cost, limited to faces whose resulting
  spell also has lesser mana value. Put the cards still in exile on the bottom
  in a random order. Read the spell's last known mana value if it has left the
  stack.
- **Panglacial Wurm** adds a way to cast to the search's own question.
  `interpret::choices::search_pick` offers `Opt::Cast` for each payable cast a
  card in the player's own library permits. The search reads the library again
  after each cast. Finding the first card ends the casting window.
- **Knowledge Pool** and **Possibility Storm** trigger only on casts from a
  hand. The cast event carries the card's zone before it moved to the stack.
  Knowledge Pool offers another card linked to that Pool only if it exiles the
  original spell. Possibility Storm uses the original spell's card types, or its
  last known types if it already left the stack. Either order leaves at least
  the Storm's cast available if it finds a legal match. If the Pool resolves
  first, it may also offer a cast.

**Who assigns combat damage.** `combat::assigner` gives the controller of the
creature assigning damage. When a blocker can block several attackers, the
defender declares each block. After the attacking creatures finish assigning
damage, the blocker's controller chooses its assignments. Each lethal pick
belongs to its source, so a creature can receive damage from several sources.
Palace Guard exercises this rule. A fourth block widens the game to a layout
that can hold as many combat relations as play requires.

Fencing Ace exercises double strike
([CR 702.4b](https://yawgatog.com/resources/magic-rules/#R7024b)). The first
damage step records which creatures started it with first strike or double
strike. The second step uses that history and checks who currently has double
strike.

Lethal picks capture which creatures a source deals lethal damage to. Amounts
would also distinguish sublethal splits, cumulative damage from multiple sources
and a choice to keep excess trampling damage on a blocker. Banding needs amounts
for those assignments when they affect the outcome.

**Banding** ([CR 702.22](https://yawgatog.com/resources/magic-rules/#R70222)) is
behind `Capability::Banding`:

- `Opt::Attack` can start a band or join one with the same attack target. Each
  band member has a combat memo that names its band. At most one member can join
  without banding. A member's memo ends when it leaves combat or combat ends.
- A defender who can block any member of a band blocks the entire band. Losing
  the banding ability leaves the band in place.
- `combat::assigner` gives the defending player the attacker’s assignment when a
  blocker has banding, and the active player the blocker’s assignment when an
  attacker has banding. The damage step offers exact shares for each creature
  and deals the assigned damage together. A trampler can keep excess damage on
  its blockers and can count damage already assigned by other attackers that
  step.

"Bands with other" (Master of the Hunt's Wolves) is a band whose membership rule
is a quality instead of the keyword.

### I. Card shapes, face-down cards and cards outside the game

A `CardDef` already holds one or two faces, `casting/legal.rs` already loops
over faces and the protocol's cast decision already carries a face. Only a test
card uses a second castable face. `actions/transform.rs` can return an exiled
card with its back face up. Transforming a permanent changes its face in place.
Flipping a permanent sets its flipped status and uses its alternative
characteristics. Two meld cards become one melded permanent and split into their
cards when it leaves.

**A shape on the definition.** `CardDef` gains `shape: Shape`, generated from
the card store's `layout` with the faces:

```rust
pub enum Shape { Single, Transforming, Modal, Split { fuse: bool }, Adventure,
                 Prepared, Flip, Meld { with: CardName, into: CardName } }
```

`check` refuses a face count the shape does not allow. The rules read the shape
where they branch on it; nothing infers it from `Face::transformed`.

| Shape or structure                                                        | What it needs                                                                                                                                                                                                                                                                                                                                         | Keystone                  |
| ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| Split ([CR 709.4](https://yawgatog.com/resources/magic-rules/#R7094))     | Off the stack, `State::copiable` answers with both halves combined: two names, both sets of types and colors, the sum of the mana values. On the stack it is the half that was cast.                                                                                                                                                                  | Fire // Ice               |
| Fuse                                                                      | A casting option that casts both halves as one spell: one cost, the left half's instructions then the right's.                                                                                                                                                                                                                                        | Wear // Tear              |
| Adventure ([CR 716.3](https://yawgatog.com/resources/magic-rules/#R7163)) | Cast as the Adventure face; as it resolves it is exiled with a permission to cast the creature from exile.                                                                                                                                                                                                                                            | Bonecrusher Giant         |
| Prepared ([CR 723.1](https://yawgatog.com/resources/magic-rules/#R7231))  | A designation on the permanent, kept as a memo. It creates a copy of the prepare spell in exile. The permanent’s current controller can cast the copy with normal timing and costs. Completing the cast removes the designation.                                                                                                                      | Adventurous Eater         |
| Modal double-faced                                                        | A real card for the path the test card "Test Stupor" uses, and oracle exporters that can cast a chosen face.                                                                                                                                                                                                                                          | Valakut Awakening         |
| Transforming, flip                                                        | The object keeps its incarnation and effects. `Effect::Transform` turns it over and gives it a new timestamp (CR 613.7g). `Effect::Flip` uses the alternative characteristics and keeps its timestamp.                                                                                                                                                | Delver of Secrets, Erayo  |
| Meld                                                                      | The card with the meld ability returns from exile as a copy of the combined back face (`Shape::Melded`, `Copiable::melded`), and its partner waits under it (`Memo::PartOf`). Its mana value is the sum of the front faces. It splits into its cards when it leaves, and an effect that moves it finds both.                                          | Gisela, the Broken Blade  |
| Saga ([CR 715.2](https://yawgatog.com/resources/magic-rules/#R7152))      | `Ability::chapter(n)`: a triggered ability on the lore counter reaching `n`. Entry adds a lore counter through a replacement in `replacement/`. `turn/mod.rs` adds lore at the start of precombat main, and `sba.rs` sacrifices a Saga with lore at or above its final chapter number once none of its chapter abilities are waiting or on the stack. | The Eldest Reborn         |
| Class ([CR 717.3](https://yawgatog.com/resources/magic-rules/#R7173))     | `Ability::level(n, cost)`: an activated ability at sorcery speed that sets the level, and abilities that exist from a level.                                                                                                                                                                                                                          | Wizard Class              |
| Case ([CR 720.3](https://yawgatog.com/resources/magic-rules/#R7203))      | A "to solve" condition checked at the controller's end step, a solved designation as a memo, and abilities that exist once solved.                                                                                                                                                                                                                    | Case of the Crimson Pulse |
| Leveler ([CR 711.2](https://yawgatog.com/resources/magic-rules/#R7112))   | `Ability::level_up(cost)`, an activated ability that adds a level counter; `LayerSpec::leveled::<N1, N2>` and `leveled_from::<N3>`, static abilities whose condition is a range of the permanent's own level counters, setting base power and toughness and adding abilities.                                                                         | Student of Warfare        |
| Prototype ([CR 719.3](https://yawgatog.com/resources/magic-rules/#R7193)) | A casting option that gives the spell, and the permanent it becomes, another cost, color, power and toughness, as copiable values.                                                                                                                                                                                                                    | Arcane Proxy              |

**Fuse.** The definition supplies a combined spell alongside its two printed
halves. A caster can choose that spell from their hand. Its stack entry keeps
the combined characteristics index, so a spell copy uses the same
characteristics and instructions. The combined spell announces the left half's
targets first and uses the normal spell target checks and resolution order.

**Prototype.** The generator reads the inset's mana cost, power and toughness
from the first line of the Oracle text into the face's
`CastingOption::Prototype`, and the definition supplies the prototyped face
after its one printed face, at `FaceIx::PROTOTYPE` (`Face::prototyped`). A
prototyped cast is a cast of that face with the option's bit chosen, which
`option_sets` requires of that face and refuses of the printed one. The object's
face field records it, so the spell, the permanent, a copy of either and the
mana value all read the prototyped characteristics, and every zone change but
resolving turns the front face up again. Arcane Proxy's "copy that card, you may
cast the copy" is `Effect::MayCast` with `copy`: the offer names the card, and
the copy is made in the card's zone once it is chosen, because a decision during
a resolution can only name objects the replayed state holds. "If you cast it" is
`Condition::Cast`, kept as `Memo::Cast` only in a game whose cards ask.

**Face-down spells**
([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)).
`CastingOption::Morph(cost)` casts the card face down: `become_spell` takes
`FaceIx::DOWN`, the spell is a 2/2 creature spell with no name that only its
controller may look at, and it enters face down. Turning it up is the special
action that exists for manifested cards (`objects/face_down.rs`), paying the
morph cost, which may be a non-mana cost (Gathan Raiders). A face-down card is
revealed when it leaves the stack or the battlefield and when the game ends.
Illusionary Mask lets any creature card be cast this way and turns it up with a
replacement when it would deal or be dealt damage.

- The offer is `Effect::MayCast` with `face: CastFace::DownTurnedUpWhenUsed`,
  which offers each card cast at `FaceIx::DOWN` for free, and `within_x`, which
  offers only the cards whose mana cost the mana spent on the ability's X could
  pay, by amount and kind (`casting/spent.rs`).
- The mana spent on X is kept on the ability's stack entry as `L::Spent`. In
  `Small` it is `()`, so recording any mana is `Full`: a session's game widens
  into `Large` for good, and a scenario of such a card starts in `Large`, as one
  that can start a subgame does. A game without such a card stores nothing.
  Payment records it only in a game with `Capability::SpentOnX`, for an ability
  whose effect reads it. Search still scores a rollout that runs out of `Small`
  as a draw, so until the game has widened the agent misjudges such an
  activation.
- The spell remembers `Memo::TurnsUpWhenUsed`, the only memo a spell on the
  stack may hold, and passes it to its permanent as it resolves, beside
  `Memo::Morphed`, which stops the manifest way of turning it up. The memo
  outlives the Mask. `objects/face_down.rs::before_use` turns the permanent up
  as it would become tapped (`event::set_tapped`), assign combat damage
  (`combat::assignment::begin_damage`), or deal or be dealt damage
  (`replacement::damage`). It applies before the other replacements and
  preventions of the same event. A face-down creature that attacks is tapped, so
  it turns up as it attacks; one that blocks turns up as it assigns damage.

**Cards outside the game**
([CR 400.11](https://yawgatog.com/resources/magic-rules/#R40011)). The
`GameTable` holds, per player, the cards of the `DeckRole::Sideboard` list as a
multiset of card indices. They have no slots.

- A wish (Burning Wish) asks its controller to pick one that matches, as a
  search of a hidden set does, and brings it in: the card takes a free slot of
  the pool as a card, not a token, and is removed from the multiset.
- A companion
  ([CR 702.139a](https://yawgatog.com/resources/magic-rules/#R702139a)) is
  declared before the game: `game/companion.rs` asks each player with a
  sideboard card whose companion condition their deck meets, as a decision
  (`Pending::Companion`) before the opening hands. The condition is
  `Ability::companion(&DeckCondition { each, has })`, read of the deck's cards
  by `State::companion_choices`. Putting it into the hand is a special action
  that costs {3} (`Opt::Companion`). The keystone, Lurrus of the Dream-Den, has
  a hybrid mana cost. Its once-each-turn graveyard permission is
  `Ability::cast_from_graveyard_each_turn` (`casting/graveyard.rs`).
- Scenarios set a sideboard with `.sideboard(P0, &[..])`, and a companion
  revealed before the game with `.companion(P0, card)`, which fails the setup
  unless the deck fulfills the card's condition. `Action::Companion` takes the
  {3} special action. Neither oracle can play either: XMage's test API and
  Forge's puzzle state both start past the reveal, with no way to record it. The
  session's best-of-three match and sideboarding between games are not part of
  this plan.

**Agents and the session.** The engine offers one cast option for each face and
set of casting options of each card that is not interchangeable with another
(`decision::collapse`), and a search keys each option by its face and options,
so two ways of casting one card are two candidates and two identical cards are
one. A card's value combines its faces by its shape (`values::by_shape`): an
Adventure or a prepared card is worth both faces, a transforming or flipping
card the mean of its two, and a split, modal or prototype card its best face,
under the estimate and the tuned values alike. The hidden search deals an
opponent's face-down card from what its cast shows: a morph has morph, and a
card cast through Illusionary Mask is a creature card. The rollout policy does
not activate an ability with {X} with no mana to spare for X.
`beryl_search/tests/all/card_shapes.rs` plays a game of each family with the
greedy agent, the expert and MCTS. The view shows a Class's level and the solved
and prepared designations (`ObjectView::designations`), and a candidate that
turns a morph face up carries its morph line. A save names an object's face when
it is not the front and its designations (save format 14).
`beryl_session/tests/card_shapes.rs` cuts a save at each new shape state of each
keystone and replays it under every perturbation. A session's game has no
sideboards, so neither a wish nor a companion can be played in the app yet, and
the census counts chapters, morph, Classes, level up, prototype, fuse,
companions, meld and manifest dread as covered.

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
