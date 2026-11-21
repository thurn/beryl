# Rules Coverage

How the rules engine grows from the cards implemented so far to every Magic
card, without slowing rollouts. The rule throughout: a mechanism that nothing in
the game uses costs one predictable branch.

*Revised 2026-11-13 in a design interview. The first version (2026-11-06) listed
26 hard effect types and sequenced them by the cube cards they unlocked. This
version turns that list into one piece of work: 24 keystone cards, each chosen
because it forces a decision that would be expensive to change once thousands of
cards exist. The work is tracked as an epic in tickets; this document is the
plan its tickets are written against.*

- **In scope:** the engine mechanisms the keystone cards need, the keystone
  cards, and the partner cards their scenarios need. Each keystone is playable
  in the app.
- **Not in scope:** complete decks, breadth (more keywords, more trigger events)
  beyond what a keystone or partner needs, AI strength, and UI polish.
- **Unchanged:** the engine stays one exact, flat, `Copy` state machine built
  for search ([engine-architecture.md](engine-architecture.md) §2).

## Where the engine stands

The keystone run built on Project 50k's kernel, card vocabulary and state stores
([engine-architecture.md](engine-architecture.md), K1 to K29). The following
describes staging `950e3c614` on 2026-11-16.

| Mechanism                  | In code today                                                                                                                                                                                                                                                                                                |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Layers                     | When the cache is stale, recompute from base values in layer order. A possible dependency is tested by applying the other effect to scratch state. Use timestamps when effects are independent or form a dependency loop. Entry replacements can ask how a permanent would exist on the battlefield.         |
| Replacement and prevention | Moves, draws, damage and destruction enter typed rule paths with a proposal or the action's subject before the write. Affected-player choices can pause both step actions and resolutions. Prevention and protection participate in damage replacement ordering. A regeneration shield replaces destruction. |
| Triggers                   | Typed listener masks index events separately from replacements. Zone changes that look back read an indexed table of last-known information. State triggers and listeners in the graveyard, exile and on the stack are supported.                                                                            |
| Characteristics and text   | Permanent characteristics use the layer cache. Queries for objects elsewhere combine copiable values with applicable effects and characteristic-defining power and toughness. Text reads use the object's composed word changes. A stacked ability keeps its source's words from when it went on the stack.  |
| Copies                     | The object's definition and face name its current copiable values. Copy effects hold any additions, while a memo remembers the original definition of a card. The same values describe tokens, permanents, spell copies and the abilities of cards under a merged permanent.                                 |
| Rule changes and mana      | Total cost applies the chosen base or alternative cost, increases, reductions and then a floor. Rules can forbid activations by chosen card name and permit casting from the graveyard. Remembered mana carries spending conditions and effects on the spell it pays for.                                    |
| Objects and stack          | An ability target is a stack position repaired when an entry below the top leaves. A merged permanent keeps one battlefield slot, and the other cards wait in the merged zone. Phasing removes the permanent from battlefield membership while preserving its zone and incarnation.                          |
| Decisions and end of game  | A decision records both whose choice it is and who answers. Hidden picks constrain the identities sampled for search. Card naming offers game-table names and an option for another card. A repeated canonical position during mandatory resolutions can end the game in a draw.                             |

`State<Small>` measures 20,240 bytes against a 20,480-byte limit. The WU Control
rollout measures 379,667,731 instructions against the current baseline of
384,206,865 and its 1% tolerance.

## Principles

1. **Index by listener.** Every event, replacement, trigger and rule change is
   found through a per-kind mask of who listens. With no listeners the check is
   one AND and a branch the predictor learns.
2. **Check the decks at game start.** `GameTable` records which mechanisms
   either deck can ever produce (`table::Capability`). A flag false all game
   makes its branch free.
3. **Rare and heavy goes to `Large`.** A mechanism that needs more room than
   `Small` has stalls into `Large`. `Small` stays within 20 KiB and one
   `memcpy`.
4. **Exact first, restricted second.** The engine always plays the exact rules.
   Search speed comes from restriction profiles that offer fewer choices, never
   from approximating a rule.
5. **Data for interfaces and for behavior.** Every card is effect data
   (engine-architecture.md, K1). A mechanism is a new entry of the vocabulary
   and its arm in the rules, and the compiler finds every place a new variant
   matters.
6. **Each mechanism lands with its numbers.** The absent-cost gate holds with no
   bump, and the mechanism's own corpus holds the present-cost gate (R2, R3).
7. **One query for each question.** "Does this object have this ability", "what
   are its copiable values" and "what color is it" are each answered by one
   kernel function. A mechanism that changes the answer changes that function,
   and no caller reads the field behind it.

The common failure is the opposite of the first rule: a general hook that runs
on every event in case some card cares. Engines built for completeness tend
toward that design; Beryl cannot afford it.

## Decisions

| ID  | Decision                                                                                                                                                                                                                                                                                                                                                                                                       | Why                                                                                                                                                                                                                                                                                                                                   |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | The cards are chosen by what is expensive to retrofit: how card data is written, what the kernel assumes an object is, and what the decision protocol assumes. 24 keystones and about 25 partners. No complete deck.                                                                                                                                                                                           | Legacy's most-played decks are mostly easy cards, so choosing by popularity delivers the hard cases late. Breadth is cheap to add later; a wrong assumption in the kernel is not.                                                                                                                                                     |
| R2  | A mechanism that the WU Control mirror does not use costs it nothing. The count gate is never raised for a new mechanism, and a ticket that cannot hold it after three attempts stops the run for a human decision.                                                                                                                                                                                            | This is the one bar an unattended run cannot argue its way past (engine-architecture.md, K1).                                                                                                                                                                                                                                         |
| R3  | Each family of keystones has a counted rollout corpus with the family in play. Every step of those rollouts is also counted on the same state with the family taken out, and the family's steps stay within 1.25 times that. A miss after three attempts is recorded with its value and cause and files a follow-up.                                                                                           | The count gate cannot see what a mechanism costs when present. Two rollouts, one with the family and one without, play different games, so their difference is mostly the game's and not the mechanism's; one step from one state differs only by the mechanism. At 1.25 times, a search over such a board keeps 80% of its rollouts. |
| R4  | `State<Small>` stays within 20,480 bytes. Each ticket that changes its size has an allotment and reports the size. Last-known information moves out of every waiting trigger into a shared table (landed: 56 bytes freed). After the foundations it reached 20,344 bytes. Reducing the alignment of each object freed 504 bytes for the remaining groups. See "Where the engine stands" for the measured size. | A state and its scratch copy must fit the 64 KiB first-level cache of a Performance core.                                                                                                                                                                                                                                             |
| R5  | The layers stay one full recompute from base, applied one layer at a time. A same-layer dependency (CR 613.8) is found by applying one effect to a scratch copy and seeing whether the other changed, never from a table of card pairs.                                                                                                                                                                        | A table of pairs cannot cover 50,000 cards. The trial runs only when two effects share a layer and one writes what the other reads.                                                                                                                                                                                                   |
| R6  | Each replaceable happening (a move, a draw, damage, destruction) builds a typed value before it changes the state, and replacement effects rewrite that value. There is no general hook on events. A step that may have to ask a player uses scratch-state replay, as does a resolution that can pause.                                                                                                        | Principle 1. A typed value per kind keeps each gate one AND and lets the compiler check each rewrite.                                                                                                                                                                                                                                 |
| R7  | An object's copiable values are a face of a definition plus a short list of additions. `Object::card` holds the definition the object currently has the values of; the card it really is, where that differs, is a memo.                                                                                                                                                                                       | `State::copiable` is read on every hot path, so a copy must cost it nothing. One representation serves copies, copies of copies and merged permanents.                                                                                                                                                                                |
| R8  | Outside the battlefield an object's characteristics are computed when read: its copiable values, its characteristic-defining power and toughness where applicable, and the continuous effects that reach other zones.                                                                                                                                                                                          | Layer results for all 255 slots would add 5 KiB. Such effects are rare, and a game without one pays a length test.                                                                                                                                                                                                                    |
| R9  | A color word, land type or creature type in an ability is a value of a private type, read through the word changes on the object the ability is printed on. With an ability on the stack, read the source's changes as they were when the ability went on the stack.                                                                                                                                           | A text change (CR 612) must reach every such word. Making the read the only way to the value turns a missed one into a compile error.                                                                                                                                                                                                 |
| R10 | An ability on the stack is named by its place on the stack. The kernel repairs such names when an entry below the top is removed. Stack entries gain no identity field.                                                                                                                                                                                                                                        | A field in each of 64 entries is copied by every rollout. Only an entry above an ability can name it, so the repair touches few entries and runs only on a removal from the middle.                                                                                                                                                   |
| R11 | A merged permanent is one object in the slot it always had. The other cards of it wait in a zone state of their own. A phased-out permanent leaves the battlefield list and bitboard and keeps its zone, incarnation and everything stored about it.                                                                                                                                                           | Both keep "one slot is one object" true for every reader. Everything that walks the battlefield ignores the cards underneath and the phased-out permanents for free.                                                                                                                                                                  |
| R12 | A `Decision` names both the player it belongs to and the player who answers it. A pick from a hidden zone that the other player has not seen constrains determinization. A named card is one of the game's cards or "another card".                                                                                                                                                                            | Each is an assumption of the session, the saves and the UI as well as the engine, so it is cheapest to settle while few cards depend on it.                                                                                                                                                                                           |
| R13 | A loop of mandatory actions is a draw (CR 104.4b), found by a repeated canonical hash after resolutions without an intervening choice. The "repeat N times" shortcut for optional loops is designed here and not built.                                                                                                                                                                                        | The draw is a rule. The shortcut is a search feature, and the longest line among these cards is about 60 decisions against a budget of 500.                                                                                                                                                                                           |
| R14 | Every keystone is playable in the app from both seats with the prompt shapes and animation verbs that exist. Agents answer every new decision legally. No Arena matching, no new animation and no strength bar.                                                                                                                                                                                                | The protocol changes of R12 are only proven by a session, a save and a UI that use them. Polish for cards in no supported deck is not worth its cost.                                                                                                                                                                                 |
| R15 | Eight groups. Three foundations land in order; five groups follow in parallel. One Warden per group, a Mason audit after the foundations and one at the end, one Archivist, no Sage. Foundations, keystones and reviews run on Opus, partner cards and wiring on Sonnet.                                                                                                                                       | The foundations edit the same kernel and rules files. Architecture drift is what the checks cannot see, and it is expensive in exactly those files.                                                                                                                                                                                   |
| R16 | Subgames, game restarts, banding and casting from a library mid-search are designed below and not built. So are shared zones, which the DanDân format needs: nothing built here may assume a library or graveyard has one owner.                                                                                                                                                                               | Each needs a state that holds more than one game, or serves a handful of cards. A shared zone is a constraint on this work, not part of it.                                                                                                                                                                                           |

## The keystone cards

A keystone is a card chosen because it forces one of the decisions above. A
partner is a card its scenarios need, or the second half of a classic
interaction.

| Group | Keystone                 | What it forces                                                                                                      | Partners                                                   |
| ----- | ------------------------ | ------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| 2     | Blood Moon               | Layer 4 over a filtered set; a type change that removes abilities (CR 305.7); a dependency; lands entering under it | Urborg, Tomb of Yawgmoth; Dryad Arbor; Glorious Anthem     |
| 2     | Humility                 | Ability removal and base P/T over every creature; every ability lookup asks whether the ability is still there      | Tarmogoyf, Dress Down                                      |
| 2     | Opalescence              | A type added with P/T from mana value; with Humility, the classic test of timestamps and CR 613.6                   |                                                            |
| 2     | Oko, Thief of Crowns     | One effect across layers 4, 5, 6 and 7b on a locked-in object; an exchange of control                               | Leonin Scimitar (exists)                                   |
| 3     | Chains of Mephistopheles | A draw replaced by instructions that themselves draw; applies to every player's draws                               | Notion Thief, Griselbrand                                  |
| 3     | Life from the Loam       | Dredge: an optional replacement that works from the graveyard; choosing among replacements                          |                                                            |
| 3     | Rest in Peace            | A replacement of every move to a graveyard; "dies" never happens                                                    | Tarmogoyf                                                  |
| 3     | Containment Priest       | A replacement that asks how the object got there, and what it would be on the battlefield                           | Flickerwisp                                                |
| 3     | Mother of Runes          | Protection: prevention, targeting, blocking and attachment at once; the order of prevention and doubling            | Circle of Protection: Red, Furnace of Rath, Lightning Bolt |
| 4     | Painter's Servant        | A color given to cards in every zone; a value chosen as it enters                                                   | Force of Will, Circle of Protection: Red                   |
| 4     | Mind Bend                | A text change (layer 3) to color words and land types, wherever an ability holds one                                | Dandân, Crystal Spray, Lord of Atlantis                    |
| 4     | Underworld Breach        | An ability given to cards in a graveyard; a cost that exiles other cards; a long loop                               | Lion's Eye Diamond, Brain Freeze                           |
| 5     | Phantasmal Image         | Entering as a copy; exceptions that become copiable values; a copy of a copy                                        | Quicksilver Gargantuan                                     |
| 5     | Thespian's Stage         | Becoming a copy while keeping an ability; a state trigger; the legend rule                                          | Dark Depths                                                |
| 5     | Fork                     | A copy of a spell with its choices, new targets and a changed color                                                 | Tendrils of Agony, Braingeyser                             |
| 6     | Trinisphere              | The whole order of CR 601.2f: alternative cost, increases, reductions, then a floor                                 | Thalia, Guardian of Thraben; Force of Will; Gurmag Angler  |
| 6     | Pithing Needle           | A card named from an open-ended set; a "can't" keyed on the chosen name                                             |                                                            |
| 6     | Cavern of Souls          | Mana that carries a restriction and a rider to the spell it pays for                                                |                                                            |
| 7     | Stifle                   | Abilities on the stack as targets                                                                                   |                                                            |
| 7     | Animate Dead             | An Aura on a card in a graveyard that rewrites its own enchant ability; a mandatory loop                            | Worldgorger Dragon                                         |
| 7     | Gemrazer                 | Mutate: one permanent made of several cards                                                                         |                                                            |
| 7     | Teferi's Protection      | Phasing; a life total that can't change; protection for a player                                                    |                                                            |
| 8     | Show and Tell            | A pick hidden from the other player until both have made one; an Aura entering without being cast                   | Griselbrand                                                |
| 8     | Mindslaver               | The player who answers a decision is not the player it belongs to                                                   |                                                            |

Group 1 has no keystone: it builds what the others stand on.

## Designs

Each section is the proposed design for one group. The foundation ticket of a
group may amend it where the code proves it wrong, and records the change in
this document.

### 1. Events and listeners

- **Kinds.** What a card's triggered abilities listen to is `Listens`, a typed
  set of up to 64 `EventKind`s. What its static abilities replace is a second
  64-bit set, `Replaces`, of `ReplacementKind`s, so a replacement's gate is its
  own AND. Each has the same three levels: the game's cards
  (`GameTable::listens_any`, `replaces_any`), the battlefield now
  (`State::listening`, `replacing`) and one card's face (`Copiable::may_listen`,
  `may_replace`). Whether a static ability changes a rule for players is no
  replacement, but the layers' pass collects it with them as the last bit of
  `Replaces`, `PLAYER_RULE_CHANGE`.
- **Abilities outside the battlefield** (CR 113.6). An ability says where it
  works (`Ability::zones`), defaulting from its kind: a spell's on the stack,
  any other on the battlefield. `.in_graveyard()`, `.on_the_stack()` and
  `.everywhere()` say otherwise. `GameTable` keeps the triggered abilities that
  work in a graveyard, in exile or on the stack (`listener::AWAY`) in a list of
  their own, and records for each card the zones of those it works in. Matching
  of events, of look-back and of state triggers walks the battlefield's
  listeners, then the objects in those zones whose card works there
  (`trigger::away`). The state keeps no bitboard of them: the zone move has no
  table to ask, the board door moves objects too, and the membership of the
  zones already says where each object is, so the set is found from it when a
  game has such a card and never otherwise. A library or a hand is refused
  (`Unmatched::Zone`), since a card there may be dealt anew among hidden cards,
  and so is any ability but a triggered one outside its kind's zone
  (`TableError::Zone`) until the rules find it there (characteristic-defining
  abilities, group 2; replacement effects, group 3).
- **Look-back** (CR 603.10). `trigger::match_look_back` matches every ability
  that looks back ("dies" and "leaves the battlefield", `TriggerEvent::Leaves`),
  with either subject, against every permanent of the batch before any of it
  moves, so a creature sees the others die with it. It reads each permanent's
  abilities from its copiable values, as the other trigger matching does: when
  layer 6 can remove abilities (group 2), both must read the layer results, so
  that a creature without abilities under Humility has no dies trigger.
- **Last-known information.** Each waiting trigger, stack entry and delayed
  trigger carried an 8-byte `Lki`. It is now a table of the state
  (`beryl_kernel::last_known`), of up to 16 rows in `Small` keyed by the
  `ObjectRef` of the object that left. Each row is a copy of the object's
  `Derived` row with its three field counters, its token flag, its copiable
  values and what it was attached to. A row is written only when an ability that
  looks back matched the object, and a reader asks `State::last_known`. The
  engine forgets the rows that no waiting trigger, stack entry or delayed
  trigger names each time a player would receive priority. An effect can now
  read any characteristic of an object that has left, not only five of them. A
  `Derived` row was 40 bytes, so a row was 56 (since ability removal, 42 and
  58): the records give back 960 bytes and the table takes 904, and
  `State<Small>` went from 20,112 to 20,056 bytes rather than freeing about 900.
  The interpreter's own record of the targets an effect moves
  (`interpret::scene::Known`, CR 608.2h) stays in the resolution: it keeps every
  kind of counter and the mana value, which a row does not.
- **State triggers** (CR 603.8). `TriggerEvent::State(&Condition)`, matched by
  `trigger::match_states` at each state-based check behind its own kind,
  `EventKind::State`. Whether one is already waiting or on the stack is found by
  looking there for the same source and ability, so no flag is stored. Its
  condition is read only as it triggers, never again as it resolves.
- **This turn's counts.** The Trackers store gains a count kept for each player:
  spells cast this turn and cards drawn this turn, each behind its capability.
- **The present-cost corpus.** A benchmark of its own, `family_counts`, has a
  corpus for each family: positions cut from greedy games of the WU mirror with
  the family's cards swapped in, each with a keystone in play. The gate does not
  compare those rollouts with rollouts of other positions: a card such as
  Glorious Anthem makes a game with more combat, and Trinisphere one with more
  passes, and either would move instructions per decision by more than the
  mechanism does. It pairs each step instead. As a rollout visits a state, the
  benchmark builds the control state from that same state, with each of the
  family's permanents and spells replaced by a stand-in (the same card with no
  static or triggered ability and no keyword) and the effects they are the
  source of ended, applies the rollout's choice to both for one transition, and
  counts each. The control state is then thrown away, so the two sides never
  drift apart. The gate is the sum over the family's steps divided by the sum
  over the controls' (R3). A step whose choice the control does not offer is
  left out of both sums and counted in the report. `docs/benchmarking.md`, "The
  present-cost gate", describes what was built.
- **Keywords the partner cards need.** Trample, lifelink and indestructible are
  bits of `Keywords`. Landwalk was planned as a keyword holding a `Subtype`,
  with a byte in `Derived` for the basic land types a permanent walks. `Derived`
  has no padding, so that byte grows each of its 128 rows by 8 bytes and
  `State<Small>` by 1,024, past its limit. Instead each basic landwalk is a bit
  of `Keywords` (`ISLANDWALK` and the others, with `Keywords::LANDWALK` naming
  each one's land type). That costs no bytes, a card complete from the store
  reads "Islandwalk" with no other code, and Lord of Atlantis grants islandwalk
  through `Modification::AddKeywords`. Trample offers no division when one
  creature blocks, and gives what it attacks only the most it can take: damage
  past lethal on a blocker changes nothing the pool reads.

### 2. Layers

The recompute keeps its shape: when the derived cache is stale, every permanent
is recomputed from base (D5). What changes is that it runs one layer at a time,
because what an effect applies to depends on the layers before it.

```
base: each permanent's copiable values (layer 1)
for each layer 2, 3, 4, 5, 6, 7a, 7b, 7c, 7d:
    begin the static abilities whose first layer this is,
        of each permanent that still has the ability
    order this layer's effects: timestamps, then dependencies
    for each effect:
        find what it applies to, against the results so far
        lock that set in if the effect has a part in a later layer
        apply it
then counters (7c), then the rules static abilities change (CR 613.11)
```

- **Sets.** A static ability may affect the permanents a filter accepts. The
  filter is read against the results so far (`layers::apply::passes`), and so is
  an "as long as" that asks whether the permanent passes a filter or how many
  permanents do (landed with Glorious Anthem). The effects are applied in one
  list sorted by layer, then timestamp, which is the loop above: an effect whose
  set or condition reads the results is listed by its source and ability and
  finds its set as it comes up. `Unapplied::Affected` remains for sets in other
  zones, which have no layer results.
- **Ability removal** (landed with Humility and Dress Down). `Derived::lost` is
  a mask of the face's abilities the permanent has lost, all of them for "loses
  all abilities": 16 bits in `Small`, 64 in `Large`, and a removal past the
  mask's width stalls the game into `Large`. `Derived` is packed to an alignment
  of two bytes so that the mask costs two bytes a row. An effect that removes
  abilities also clears the keywords and grants applied before it; those applied
  after it stay. `State::has_ability` is the one question every rules reader
  asks: trigger matching, activation, the static scan, the replacement lookups,
  the rule changes and the mana solver. With no removal in effect it is one test
  of a flag the recompute stores. The AI's heuristics in `beryl_search` still
  read the cards' faces.
- **Continuing once begun** (CR 613.6). A static ability's `LayerSpec` lists the
  parts of its one effect (`modifications`), and the recompute lists one entry
  for each part. A static ability begins in its first layer if its permanent
  still has it when the effect comes up, after the effects of that layer it
  depends on. A part in a later layer applies to the set the first part found,
  though the ability is removed on the way. This is what makes Humility with
  Opalescence come out as the rules say, and Oko's Elk keep its 3/3. Two effects
  that remove each other's abilities form a loop of dependencies and apply in
  timestamp order (CR 613.8b).
- **Land types** (landed with Blood Moon, Urborg and Dryad Arbor). A land has
  the intrinsic mana ability of each basic land type it has (CR 305.6). A card
  file may write that ability out, as Plains does, or not, as Dryad Arbor; the
  card table's facts hold the ones a face does not write out
  (`table::land_mana`), and `ManaFace` counts them, so the solver's per-card
  fast path is unchanged. Abilities a layer effect gives are bits of
  `Derived::keywords` (`Keywords::PLAINS_MANA` to `FOREST_MANA`), since a row
  has no byte to spare: "is a [basic land type]" in addition to the land's
  others adds the bit for a type it lacked (CR 305.7), and
  `Modification::SetLandTypes` replaces the land types, sets the whole ability
  mask and leaves only the new types' bits. A layer 6 removal of all abilities
  clears the bits with the other keywords. `State::intrinsic_mana` is what the
  mana readers ask; `State::makes_card_mana`, one flag in a game with no removal
  and no land type change, says when the card's facts still hold.
- **Dependencies** (landed with the same cards). In a layer with two or more
  effects, A depends on B when applying B changes whether A exists, what A
  applies to, or what A does to it (`continuous/layers/dependency.rs`). Each
  `LayerChange` declares what it writes (`LayerChange::writes`) among
  `Characteristics`, and each listed effect what it reads: its filter and "as
  long as", its counted amount, and, for a static ability, whether its permanent
  has lost it. Gaining and losing abilities are apart, and no filter reads
  power, toughness or abilities, so a layer whose effects write only those is
  never searched and a WU mirror game runs no trial. For a pair that might
  depend, B is applied to the results, A's existence, set and change are
  compared, and the rows are put back. The effect applied next is the earliest
  by timestamp that depends on nothing still waiting; with every one waiting,
  the earliest in a loop; the order is found again after each application. A
  static ability's existence is asked as its effect comes up, not at the head of
  its layer. The cards' characteristic-defining abilities are all in layer 7a,
  where nothing else applies, so CR 613.8a's (c) needs no code.
- **Timestamps** (CR 613.7). An Aura or Equipment gets a new one as it becomes
  attached, and a permanent as it turns face up (both already built). The `u16`
  counter is renumbered by rank, out of line, before it would run out
  (`State::take_timestamp`). Not built: transforming permanents, which the
  engine does not yet turn, and the order of objects that enter together (CR
  613.7m), which is still the order they move in; a decision is needed only when
  two of them have effects in one layer that the dependency search's prefilter
  says could interact.
- **As it would exist** (CR 614.12). `layers::entering::as_it_would_exist`
  copies the state, moves the entering object onto the battlefield in the copy
  as its entry says, and runs the recompute there, so every effect finds the
  object as it finds any permanent and the real state is never changed. The
  replacement pipeline (`replacement::entering`, `replacement::life_to_enter`)
  asks for it only for an object whose face has a replacement of its entering,
  and only when an effect in force removes abilities or the face has a static
  ability of its own (`continuous::layers::lost_entering`), so other games pay
  one flag test. Under Blood Moon a land that would ask for life as it enters is
  a Mountain with no such ability, and enters untapped. A recompute over the
  battlefield's rows plus one scratch row was the first design; it would have
  needed every filter and layer read to know about a row that is not a
  permanent, and the copy costs nothing in a game that never asks.
- **What makes the cache stale** (landed with Tarmogoyf). A static ability may
  read something no layer writes, as Tarmogoyf's characteristic-defining ability
  reads the graveyards. Each face's static abilities declare such inputs
  (`static_ability::Inputs`, held per face in the card table's `FaceFacts`): the
  counters of a kind on its own permanent, which an "as long as" compares, and
  the cards in graveyards, which `Amount::CardTypesAmong` counts. In a game
  where some card has one (the table's `Capability::LayerInputs`), the recompute
  stores the union of the battlefield's inputs. The zone move marks the cache
  stale for a card entering or leaving a graveyard only while that union holds
  the graveyards, and `State::layers_read_counters` answers the counter mutators
  from it. A hand's size and a life total are not inputs yet, since no
  characteristic-defining ability of the cards reads one. Tarmogoyf defines
  power and toughness together (`Modification::SetPt`, with `Amount::Plus` for
  "that number plus 1").
- **A granted static ability** (landed with Opalescence). An ability granted in
  layer 6 whose effect belongs to an earlier layer is found only after that
  layer has run. The recompute then runs the layers once more with the abilities
  the run before found granted (`continuous/layers/granted.rs`), until a run
  finds the same ones, at most three runs. A granted ability's effect has its
  permanent's timestamp and comes after the face's own abilities.
  `Capability::GrantsLayerEffect` says whether any card of the game grants a
  static ability with a continuous effect, so other games run the layers once.
  The card table refuses a granted ability whose amount the layers read as they
  apply (`Unapplied::GrantedAmount`), since they find such an amount by the
  ability's place on its permanent's face.
- **Each other, and its mana value** (landed with Opalescence). `Objects::Other`
  is a set without the object whose ability it is, in a static ability's
  affected set and in a resolving effect. In a static ability's base power and
  toughness, `Amount::ManaValue(Objects::Those)` is the mana value of each
  permanent it affects, read for each as layer 7b applies.

The single stale flag stays. The Layers family's corpus says whether it needs
splitting.

### 3. Replacement and prevention

Each replaceable happening builds a value before it changes the state: a move
(with how a permanent enters), one card drawn, damage, a destruction. This
document calls that value the proposal. A replacement effect is a kind of
proposal, a filter over it, a class from CR 616.1 and a rewrite.

- **Sources.** A static ability of an object in a zone where it works; an entry
  of the Effects store, which is how a shield with an amount and a chosen source
  is kept; and the entering object's own.
- **Rewrites**, as vocabulary: send it to another zone; change how it enters
  (tapped, with counters, as a copy, choosing a value, attached to something);
  prevent some or all; multiply; skip; or do these instructions instead, an
  `Effect` in which "that player" is the affected player.
  `Replacement::EntersTapped` and its two siblings become entries of this form.
- **The loop** (CR 616.1). Gate on the kind: with nothing listening the proposal
  is returned as built. Otherwise collect the effects that apply and have not
  yet applied to this event (CR 614.5). Take the classes in order,
  self-replacement first. Within a class, effects whose rewrites are the same
  data are one candidate; if more than one candidate is left, the affected
  player or the affected object's controller chooses. Apply the rewrite, then
  collect again (CR 616.1f). An event that is replaced never happens, so nothing
  triggers on it.
- **Asking in the middle of a step.** A replacement's choice can come in a draw
  step or in combat damage, where no resolution is open to pause. The engine
  already pauses a resolution by running it on a scratch copy and replaying it
  (architecture §7.3). The same transaction wraps any step of `next` while the
  state says a choice is possible: an optional replacement is live, or two that
  are not the same. Rollouts answer inline and never pause, as today.
- **Draws.** A draw of several cards is split into single draws only when
  something replaces draws or triggers on each card, as `drawing::draw` already
  does for one case.
- **Entering.** The effects that apply to an entering permanent are found
  against the permanent as it would exist (group 2). An Aura that enters without
  being cast chooses what it enchants as part of its entry (CR 303.4f).
- **Prevention.** Protection is a prevention effect read from the layer results.
  A shield is an Effects entry with what is left of it and the source it was
  chosen for. A proposal carries "can't be prevented". Doubling and prevention
  on one event are ordered by the loop above.
- **Regeneration** is a shield that rewrites a destruction. by-ilmp (epic
  by-sxgz) builds it first as an Effects entry; this group folds that shield
  into the pipeline as a destruction proposal.

Counters and tokens are proposals of the same form with no keystone here. They
are built with their first card.

Built so far (by-bajq.3.1):

- `beryl_card::Replacement` is a product of what it replaces (`Replaced`: this
  permanent entering, a draw by some players, an object a filter accepts being
  put into a zone) and a rewrite (`Rewrite`). An `Ability` holds it by
  reference, as it holds its trigger, so `Ability` keeps its power-of-two size.
  `replacement::check` in the kernel accepts only the pairs the rules have code
  for, and the card table refuses the rest. Optional replacements and
  replacements that work outside the battlefield are refused until the choice of
  CR 616.1 is built.
- `beryl_rules/src/replacement/mod.rs` holds every replacement. A move is
  proposed as `replacement::Move` (object, from, to, how it enters);
  `replacement::moving` decides where it goes, collecting the Effects store's
  move entries and the static abilities of permanents (gated by
  `ReplacementKind::Move` in the layers' mask) and applying candidates until
  none is left (CR 616.1f). Candidates that put the object into the same zone
  are one. A self-replacement effect is applied by the instruction that proposes
  the move, so the loop needs no class of its own for it, and no rewrite of a
  move belongs to CR 616.1b to 616.1d yet. Where two candidates differ, a
  `debug_assert` marks the choice that by-bajq.3.2 builds. The entering object's
  own replacements are applied after its destination is settled: they only
  change how it enters and never conflict with one another or with a change of
  destination.
- A draw keeps `drawing::draw`'s loop of single draws and asks
  `replacement::draw` before each. Damage gets its proposal with prevention
  (by-bajq.3.9), the first replacement of damage.

Built by by-bajq.3.2:

- The choice of CR 616.1 is a decision of purpose `Replace`, asked as
  `ChooseUpTo`: one candidate for each card whose effect could apply, and `Done`
  for none of the optional ones. Dredge (`Rewrite::Dredge`) is the first
  optional replacement and the first that works from a graveyard; the card table
  says whether any card of a game has it (`Capability::Dredge`), and a draw then
  looks in the drawing player's graveyard before each single draw.
- The flag is `replacement::may_ask`, found when a step ends or a resolution
  begins rather than kept in the state: a dredge card in a graveyard with enough
  cards in its owner's library. A game without dredge pays one AND. A dredge
  card that reaches a graveyard in the same run that would offer it is not
  offered there (the run cannot pause, so the optional replacement is declined);
  no card of the pool makes that happen yet.
- The answers of a run that may pause are in the sink (`beryl_kernel::asking`),
  so a choice can stop the run from any depth: the code that asked returns
  `Full`, and whoever opened the run tells a stop from a capacity running out. A
  resolution's own questions and a replacement's choice below it share one
  record. A step's turn-based actions are the other place a run opens: the step
  begins on the state itself, so players see it begin, and the actions' pause is
  `Pending::StepActions`; they run again from the start with the record. An
  inline source answers each question as the run stops and the run starts again,
  so rollouts with a live choice also run on the copy.
- Two replacements of a move into different zones are still not a choice: no
  card yet puts an object anywhere but exile, so `replacement::destination`
  keeps its `debug_assert`. Combat damage meets no replacement until damage has
  its proposal (by-bajq.3.9); damage dealt as a combat damage step begins is
  that step's turn-based action, so its choices will pause the same way.

Built by by-bajq.3.9:

- Damage is proposed before it is dealt (`replacement::Damage`: source,
  recipient, amount, combat, and whether it can be prevented), and
  `replacement::damage` applies its prevention. The only prevention yet is
  protection; shields and doubling (by-bajq.3.10) join the same function.
- Protection is a layer-6 change (`LayerChange::AddProtection`, from
  `Modification::AddProtection`) into `Derived::protection`. Colors are the only
  quality (`beryl_card::Protection`); "the color of your choice" is chosen as
  the effect begins, through `choices::catalog` with `ChosenKind::Color`, the
  same question a color chosen as a permanent enters asks.
- The gate is a bit of the state's replacement mask, `Replaces::PROTECTION`, set
  by the layers when an effect that gives protection applied:
  `State::protection` is one AND without it.
  `beryl_rules/src/replacement/protection.rs` reads it for targets
  (`casting/targets.rs`, as they are chosen and as the spell or ability
  resolves), Auras (`sba.rs`, CR 704.5m), Equipment (`attach::may_equip_in`, CR
  704.5n) and blockers (`combat::can_block`).
- No printed protection exists yet: a face's keywords have no parameter, so a
  card with "protection from red" adds it to the face when its first card comes.

Built by by-bajq.3.4 (Chains of Mephistopheles, Notion Thief):

- "Except the first one they draw in each of their draw steps" is part of what a
  replacement of draws replaces, not a condition: `Replaced::Draw` carries
  `Draws::Each` or `Draws::ExceptFirstInDrawStep`, which `replacement::draw`
  reads from the `DrewInDrawStep` count. A condition could not say it, since the
  ability's condition is read for its controller and knows no drawing player.
- `Rewrite::Instead` of a draw may be any data body (`Instead::Instructions`),
  carried out as a resolution would, with "that player" the drawing player.
  `beryl_card`'s check allows `Players::ThatPlayer` in such a body. A card with
  one adds `ReplacementKind::DrawInstructions`, which `replacement::may_ask`
  reads from the card table: its instructions may ask (a discard), so in a game
  with such a card each resolution and step runs on the copy.
- CR 614.5 is the sink's `applied` list: the replacements that have applied to
  the draw being made or to a draw that replaced it. A draw made by their
  instructions skips them and is offered to the others, so two Chains each apply
  once, and with a Notion Thief each the player who would draw draws.
- Where the replacements of a draw do not all do the same (same ability, same
  controller), the drawing player chooses which applies first: a `ChooseObject`
  of purpose `Replace`. Dredge is still offered before them; declining it is
  applying the others first.
- Two Chains of different players are still two candidates: the check for "the
  same" does not know that Chains' body never reads "you".

Built by by-bajq.3.10:

- A prevention shield is an Effects entry on a player or a permanent
  (`Payload::Prevent`, `beryl_kernel::effects::prevention`): what is left of it
  (an amount, or the next event) and, for "a source of your choice", the source
  as an `ObjectRef` with the face and ability that made it, whose filter the
  source must still pass (CR 615.9). It ends when used up or at end of turn. The
  vocabulary is `Effect::PreventDamage` (Circle of Protection: Red, Mending
  Hands); the source is chosen among the permanents and the spells on the stack
  the filter accepts.
- Doubling is `Replacement::multiplies_damage` (`Replaced::Damage`,
  `Rewrite::Multiply`), a static ability of a permanent gated by
  `ReplacementKind::Damage` (Furnace of Rath).
- `replacement::damage` (`beryl_rules/src/replacement/damage.rs`) is the loop of
  CR 616.1 for damage: protection, shields and multipliers, each applying once
  (CR 614.5). One that prevents all the damage applies first without asking, and
  shields of an amount apply earliest first; the one question is a multiplier
  against a shield of an amount, asked of the affected player as `Replace`
  (`ChooseObject`, the shield's card or the permanent). A card that replaces
  damage adds `ReplacementKind::Damage` to the card table's mask, which turns on
  `replacement::may_ask`, so resolutions and combat damage can pause for it.
- Regeneration's shields are applied by `replacement::destruction`, the
  destruction proposal that `actions::destroy` and the lethal-damage SBA make.
- Not built: damage that can't be prevented skips every prevention (no card
  makes damage unpreventable yet), and a permanent spell chosen as a source is
  not followed onto the battlefield (CR 609.7a's last sentence).

### 4. Characteristics in every zone, and text

- **Reading** (built). `query.rs` answers colors, card types, supertypes,
  subtypes, keywords, power and toughness for any object. For a permanent the
  answer is its layer result. For anything else it is the copiable values, then
  the continuous effects that reach other zones
  (`beryl_kernel/src/elsewhere.rs`). Such an effect is a static ability whose
  affected set is `Objects::Everywhere(filter)` ("all cards that aren't on the
  battlefield, spells, and permanents"), with parts in layers 4 and 5 only and
  no "as long as" (the card table refuses others, and refuses granting one). The
  recompute lists each such part, in the order the layers apply it, as three
  bytes (the source's slot, the ability and the part) in a list of at most four
  beside the layer results; a fifth is `Full`. A read off the battlefield tests
  the list's length and walks it only when it is not empty, resolving each part
  from the source's face and its chosen color as it applies. There are no
  per-slot attribute bitboards: a filter over a hand or a graveyard asks the
  query for each object, which is the face read and one length test while the
  list is empty.
- **What still reads a layer result directly.** Readers that walk the
  battlefield (state-based actions, combat, the mana solver, the session's
  views) take a permanent's layer result, which is the query's answer for a
  permanent. Lifelink and `Amount::CardTypesAmong`, which read objects in any
  zone, ask the query. A rule change's filter over a spell not yet cast
  (`continuous::rule_change::accepts`) reads the card's face: no effect of the
  cards reaches a card being cast through it yet.
- **Which abilities an object has** is answered by one kernel query that yields
  its face's abilities it has not lost and those granted to it (by-bajq.4.2), so
  a grant to a card in a graveyard (Underworld Breach) is one more source of
  that query, not a change to each reader.
- **Characteristic-defining abilities** (CR 604.3) work in every zone and are
  read the same way: Tarmogoyf's power in a graveyard is its amount, computed
  when asked. The kernel evaluates the amounts a characteristic-defining ability
  can say (`beryl_kernel::defined`), behind one capability test, so
  `State::power` off the battlefield applies them.
- **Words.** A color word, a basic land type and a creature type inside an
  ability become values of private types. The only way to the value takes the
  word changes of the object the ability is printed on. Filters, parameterized
  keywords (protection from a color), conditions and modifications hold them.
  The basic landwalks are five bits of `Keywords` (group 1), so a change of land
  type word maps one landwalk bit to another, as it maps the bits of the
  intrinsic mana abilities a layer 4 effect gave (`Keywords::PLAINS_MANA` to
  `FOREST_MANA`, group 2). A card's name is never changed (CR 612.2), and a mana
  symbol is not a color word.
- **Words as built** (2026-11-15). `beryl_card::words` holds `ColorWords` (a set
  of colors), `SubtypeWord` (any subtype: a change names only basic land types
  and creature types, so any other reads as itself), `SubtypeWords` (a type
  line's subtypes, `Face::subtypes`) and `LandTypeWords` (the basic land types
  of `Modification::SetLandTypeWords`), each read only through a `Words`. The
  vocabulary variants hold them (`Filter::SubtypeWord`, `Filter::ColorWords`,
  `Modification::AddSubtypeWord` and so on), and `const fn`s with the old names
  build them, so no card file changed. `State::words_of` gives an object's
  words; `Evaluate` and `LayerChange::resolve` take the words of the object the
  filter or effect is printed on, and the layers read a static ability through
  its permanent's words, a granted one as printed (CR 612.3). Still read as
  printed, each to be given an object when text changes arrive: a rule change in
  force (it is known by its card, not its permanent), a prevention shield's
  filter after its source is gone, the table's per-definition caches of a face's
  subtypes and land mana, the AI's target guesses and the UI's labels. Text
  changes since gave most of these an object ("Text changes as built" below).
- **A text change** is an entry of the Effects store on one object, permanent or
  spell: a kind of word, from and to (`beryl_card::TextChange`, the payload
  `Payload::Text`). Several compose in timestamp order into the object's `Words`
  (`State::words_of`, `beryl_kernel/src/effects/text.rs`). It does not reach an
  ability another object granted (CR 612.3), because only the object's own face
  is read through its changes.
- **Text changes as built** (2026-11-15). The cards say one with
  `Effect::ChangeText { objects, duration }`, not a `Modification`: a
  modification changes permanents in the layers, and Crystal Spray targets a
  spell too. Its controller picks the replaced word among ten
  (`beryl_card::TextWord`: five colors, five basic land types,
  `Purpose::ReplaceWord`) and then a different word of the same kind
  (`Purpose::NewWord`), both in the catalog prompt shape. Layer 3 is no pass of
  the layers: a word is read through `words_of` wherever it is read, and the
  layer 1 values of a permanent whose type line reads otherwise are rewritten
  before layer 4 (`beryl_rules/src/continuous/layers/text.rs`): its subtypes,
  its landwalks, and for a land whose basic land types changed, the intrinsic
  mana of the new types and the loss of a written mana ability of the old ones
  (a Forest that reads "Island" taps for {U}). A permanent spell's changes are
  carried onto the battlefield by the zone move (CR 400.7a). A prevention shield
  keeps the color words of the text it was made from
  (`beryl_card::ColorChanges`, two bytes), since its payload has no room for a
  whole `Words`; its subtype words read as printed. Still read as printed: a
  rule change in force, the AI's target guesses and the UI's cost labels.
- **On the stack.** An ability that goes on the stack keeps the words its source
  had then. The state holds up to four captured sets of changes for abilities on
  the stack (`State::captured_words`), named by a byte in the stack entry
  (`StackEntry::words`: as printed, a set's number, or "read now" for a mana
  ability that never goes on the stack), and empties them with the stack. A
  resolution and the legality of its targets read `State::entry_words`. Growth:
  the table of four sets, 88 bytes; the byte fits the entry's padding.
- **Absent.** No capability flag: the Effects store's index of payload kinds is
  the test. With no text change in force, `words_of` is one AND, an ability goes
  on the stack as printed without a look at its source, and the byte and the
  empty table are left out of the state's hash, so a game without a text change
  hashes as before.
- **Shown.** The session sends an object's rules text and type line read through
  its words when they differ from the printed ones (`ObjectView::text` and
  `typeLine`, rendered by `beryl_card::render::render_face_read`), and the card
  frame shows them in place of the card store's.

### 5. Copies

- **Copiable values** are a face of a definition and up to two additions in
  `Small`, more in `Large`. An addition names an entry of some card's list of
  copy exceptions (add a type or subtype, set a color, set P/T, not legendary,
  has this ability), or every ability of another face. Copying an object copies
  its additions and appends the copier's own, so a copy of a copy never walks a
  chain (CR 707.9).
- **What the additions reach** (Mason audit, 2026-11-14). An addition that gives
  an ability ("has this ability", every ability of another face) is read through
  the query of the abilities an object has (by-bajq.4.2), never by the readers
  that walk a face. What the card table keeps per definition does not see
  additions: the listeners (`GameTable::listeners`), `Copiable::may_listen` and
  `may_replace`, and `FaceFacts`. The mask of lost abilities (`Derived::lost`,
  16 bits in `Small`) counts the face's abilities, so an addition's abilities
  need places in it too. A `Copiable` with additions is held by each of the 16
  last-known rows, so it grows them; its bytes come from the room by-bajq.4.3
  finds.
- **Where they live.** `Object::card` and `Object::face` hold the definition the
  object has the values of, so `State::copiable` reads what it reads today. The
  card an object really is, where that differs, is a memo, read behind the keyed
  bit by `State::card_behind` and restored by the zone move. The additions and
  the copy's duration are a layer 1 entry of the Effects store. The copy
  mutators keep the fields equal to what layer 1 gives, and an invariant checks
  it. `Flag::OneOne` becomes an addition.
- **As built** (by-bajq.6.1, 2026-11-15). An addition is a card and an index
  into its face's `Face::excepts`; a `CopyException` is a list of `CopyChange`s,
  of which only power and toughness exist so far: each later change (a type, a
  color, "has this ability", every ability of a face) comes with the card that
  needs it, as does applying it in the layers. A token copy made with an
  exception is a copy effect on the token (`Payload::Copy` of the Effects store,
  lasting indefinitely), behind `Flag::Copy`, and the zone move carries it to
  the token's next incarnation. `Memo::Really`, a card under a copy effect and
  the copy mutators that end one come with the first card that becomes a copy
  (by-bajq.6.2, 6.3).
- **Entering as a copy** is a replacement of the class CR 616.1c names. The
  choice is not a target. The other replacements of entering are then found
  against the copy.
- **As built** (by-bajq.6.2, 2026-11-15). `Rewrite::EnterAsCopy` (a filter and
  an optional exception) is applied first in `replacement::entering`: the
  entering object's controller chooses a permanent or declines, the copied
  values (with their additions, and the copier's exception after them) wait in
  `Sink::copies`, and the face they give is the one the other replacements of
  entering are read from, so a copy of Painter's Servant chooses a color. The
  copy effect starts as the permanent arrives (`State::enter_as_copy`), before
  the layers or any trigger look at it. A card under a copy effect remembers the
  card it is (`Memo::Really`); the zone move makes it that card again.
  `CopyChange` gained `AddSubtype` (read by layer 1 through
  `State::copiable_subtypes`) and `Has`, an ability the face lists in its
  grants. A triggered ability given this way is matched by a pass over the
  copies (`trigger::match_given`, behind `Capability::GivesTriggers`), and
  triggers on the stack are now named by `FaceAbility` so that one can be a
  grant. "Becomes the target of a spell or ability" is
  `TriggerEvent::BecomesTarget`, matched on `Event::Targeted`, which the rules
  raise after a spell is cast, an ability activated or a trigger put on the
  stack, and keep from the sink. Gaps: no effect changes targets yet, so a
  changed target raises nothing; an ability-removing effect does not remove an
  ability a copy exception gives; and what an entering copy would lose to such
  effects is not found.
- **Becoming a copy** is an instruction with a duration and the copier's
  exception.
- **As built** (by-bajq.6.3, 2026-11-15). `Effect::BecomeCopy` names the
  permanents that become copies, what they copy and an exception of the
  ability's face, and no duration: no card of the pool has a copy effect that
  ends, so ending one and restoring the values under it is not built. The
  permanent takes the copiable values of the first permanent `of` names, with
  the exception after them (`State::become_copy`): its definition and face
  change, a card remembers what it really is (`Memo::Really`), and a copy effect
  already on it is ended, since the new one replaces every value it gave.
  Becoming a copy is not entering, so the copy keeps its counters and status,
  and a Thespian's Stage that copies Dark Depths has no ice counters and its
  state trigger triggers at once. "Except it has this ability" is
  `CopyChange::Has` of an activated ability: layer 1 puts it with the abilities
  granted to the permanent (`State::copy_grants`), where activation and every
  reader of abilities find it, and an effect that sets a land's type (Blood
  Moon, CR 305.7) or removes abilities removes it. "Enters with N counters" is
  `Rewrite::EntersWithCounters`. Gaps: a copy effect with a duration; an
  exception giving a static ability.
- **Copies of spells** (CR 707.10). A copy takes a slot of the token pool on the
  stack and the original's modes, targets, X and costs paid. It is not cast.
  Where the effect allows, each target may be chosen again through the casting
  machine's target step. A copy of a permanent spell becomes a token. Storm is a
  triggered ability that works on the stack and reads the count of spells cast
  this turn as it triggers.
- **As built** (by-bajq.6.4, 2026-11-14). `Effect::CopySpell` names the spells,
  a count, an exception of the copier's face and whether new targets may be
  chosen. A copy is a slot of the shared pool marked as a token, which no card
  casts, so a token on the stack is always a copy: it moves as a spell does, and
  the state-based check ends it off the stack (CR 704.5e). Its copy exception
  (`CopyChange::Colors`, Fork's red) is a copy effect on it, as a token copy's
  is, and layer 1 and the queries of objects off the battlefield read its
  colors. New targets are not chosen through the casting machine but by a
  resolution question for each target, with the target it has listed first
  (`choices::retarget`), before the copy goes on the stack. Storm's count is the
  amount of the "when you cast" trigger (`Amount::SpellsCastBefore`), the spells
  cast this turn by every player less the spell itself, read as it triggers. A
  spell countered or otherwise taken off the stack while an ability of its own
  that copies it waits or is on the stack (CR 113.7a, 608.2h) is kept as it last
  was there, its stack entry and copiable values, in a table of the state beside
  last-known information (`last_known::DepartedSpell`, two rows in `Small`).
  Only a game whose cards have such an ability keeps one
  (`Capability::CopiesItself`), and the ability copies from it.

### 6. Costs, prohibitions and mana

- **The total cost** (CR 601.2f) is computed in one place, in the order the rule
  gives: the mana cost or an alternative cost, then additional costs and
  increases, then reductions, then an effect that sets a floor, which says of
  itself that it applies last (built: `casting::costs::spell_cost`, Trinisphere
  and Thalia). A static cost change is a rule change the game numbers
  (`Governed::SpellCosts`, `RuleKind::SpellCost`). It is asked of the permanents
  that make it, not kept in the players' rule sets, because Trinisphere's "as
  long as this artifact is untapped" is a condition the layers don't read. In a
  game whose cards make none, the check is one test of the table's set.
- **Alternative costs and delve.** An alternative cost no keyword names is
  `CastingOption::Alternative`, mana and parts such as life and a card exiled
  from hand (built: Force of Will). Delve (`Keywords::DELVE`, CR 702.66a) pays
  generic mana of the locked-in total, after any floor, so it can pay what a
  floor adds and never colored mana (built: Gurmag Angler). It shares the
  casting steps' "objects rather than generic mana" payment with waterbend: the
  player picks graveyard cards from the bottom up, so each set of cards is one
  sequence of answers, and a pick is offered only while it is a real choice. The
  oracles report delve as unsupported: XMage pays it as a special action the
  exporter does not write, and Forge's AI chooses the cards itself.
- **Prohibitions.** `Governed::ActivateAbilities` is activating the abilities of
  the sources a `Named` names, with or without mana abilities, under
  `When::Never` (built for Pithing Needle). A filter has no permanent to read a
  memo from, so the sources are `Named::Chosen`, not a filter: the name the
  permanent that makes the rule chose. The check
  (`continuous::rule_change::cant_activate`, last in
  `casting::legal::activatable`) finds the makers only once the kind is in
  force, and compares each one's chosen name with the source's copiable name.
- **A chosen value** is a memo of the permanent: a name, a color, a creature
  type. It is chosen as the permanent enters, through the replacement pipeline,
  in the `Catalog` prompt shape (built by by-bajq.3.3: `Rewrite::Choose`,
  `Memo::Chosen`, `State::chosen`). The candidates for a name are the names in
  the game's table and "another card": every name outside both decklists has the
  same consequences (D6). A filter is evaluated for a player and X, never for
  the permanent with the ability, so no filter reads the memo: each reader (the
  maker of a rule change, the source of a layer effect) has the permanent and
  asks `State::chosen` for its value.
- **A class of permission.** An effect that lets a player cast every card a
  filter accepts from a zone is a rule change with casting terms. A keyword that
  does so by granting an ability to cards in a graveyard is read through group
  4's list. `casting::legal::cast_sources` scans the zone only while such a rule
  is in force.
- **Escape as built** (by-bajq.5.5). Underworld Breach's grant is a static
  ability on `Objects::Everywhere` whose filter names the zone
  (`Filter::InZone(Graveyard)`) and whose part is `Modification::GrantEscape`
  (layer 6, `LayerChange::GrantEscape`, which changes nothing of a permanent).
  The layers list it in group 4's list, and `State::escapes` answers whether a
  card there has it. `cast_sources` adds the caster's graveyard cards with
  escape while that list is not empty. A cast for escape carries a reserved bit,
  `Castings::ESCAPE` (so a face lists at most seven options), and
  `Castings::paid` yields `CastingOption::Escape`: the face's mana cost and
  `ESCAPE_EXILES` ("exile three other cards from your graveyard"; Breach is the
  only card that grants escape, so the count is fixed). The total cost, X, the
  parts step and `Memo::Paid` (`CastingKind::Escape`) read it with no special
  case. A card that also has flashback offers both casts, and a cast for escape
  starts no permission's replacement. The rule-change form of a class of
  permission is not built: no card of the pool says one.
- **Mana abilities activated only as an instant** (by-bajq.5.5). Lion's Eye
  Diamond's ability ("Discard your hand", `CostPart::DiscardHand`;
  `Timing::AsInstant`) is never a source of the mana solver, so it can't be
  activated while a cost is paid. Priority offers it as an activation of its own
  (`Capability::InstantMana`); a color of "any one color" is asked first
  (`Pending::ManaColor`, the color question a land asks as it enters), then the
  costs are paid and the mana added without the stack (CR 605.3b). The Breach
  loop with Brain Freeze empties a 53-card library in 118 of a turn's 500
  decisions.
- **Mana with memory** (built by by-bajq.7.4). A mana ability may restrict its
  mana (`ManaRestriction`: the spells it casts, "of the chosen type", a rider).
  The game numbers restrictions as it numbers rule changes, and the rider is a
  rule change it numbers too ("that spell can't be countered" is
  `Governed::Counter`, `RuleKind::CantBeCountered`). A pool gains up to four
  entries of mana that remembers: a kind, a restriction, the chosen type
  captured as the mana is made, and the source's card. A payment says what it is
  for (`mana::Paying`), and only a spell's payment reads restrictions. A source
  of restricted mana the spell accepts is a class of its own, linked to the
  class of its unrestricted mana, and a plan that spends restricted mana with a
  rider differs in outcome from one that doesn't, so both are offered (D7).
  Spent on a spell, the rider becomes `Continuous::Rule` on that spell, which
  `actions::counter` asks. `Capability::RemembersMana` keeps all of it out of
  every other game.
- **A player** (built by by-bajq.8.4). "Your life total can't change" and
  protection from everything for a player are one rule change on that player,
  `Governed::Player(PlayerRules)`, of one rule kind (`RuleKind::Player`): an
  ability starts at most one rule change, and Teferi's Protection makes both.
  The gain and loss of life (`life::gain`, `life::lose`), damage to a player,
  the payment of life (`life::can_pay`), the targeting check and the damage
  prevention each ask `continuous::rule_change::life_can_change` or
  `continuous::rule_change::protected_from_everything`, one AND while no card in
  the game makes such a change. Damage to a player whose life can't change is
  dealt and changes no life total. An Aura attached to a player is not built.

### 7. Stack and object identity

- **Abilities as targets.** A target may be an ability, named by its place on
  the stack. Only an entry above an ability can name it. When an entry leaves
  from below the top, the kernel moves the names that pointed above it and marks
  those that pointed at it as gone. A save describes such a target by the
  ability's source and index. Built as a decision option for the ability's
  place; the table names a stack ability by a number the script gives it, so a
  target choice that offers one is answered by the candidates' buttons.
- **An Aura on a card in a graveyard** (built). `attached_to` may name an object
  in a graveyard, and the card leaving the graveyard unattaches the Aura
  (`event::move_batch_to`, in a game with an Aura). An enchant ability is what
  an Aura's spell ability targets; `attach::may_enchant` asks every one the Aura
  has now through `State::abilities` (CR 702.5c), so a layer 6 `LoseAbility`
  removes one and a granted spell ability adds one. The state-based action (CR
  704.5m) and the attach instruction both ask it. "Put onto the battlefield with
  this Aura" is `Memo::PutOntoBattlefieldWith` on the creature, written by a
  linked `Effect::PutOntoBattlefield` (the general zone change to the
  battlefield: under whose control, tapped or not) and read by
  `TargetSpec::put_with_this` and `Objects::PutOntoBattlefieldWithThis`. Animate
  Dead's "When this Aura leaves the battlefield" sits in its enters ability's
  paragraph, so it is a delayed triggered ability that waits for its source to
  leave (`delayed_triggers::fire_on_leaving`). An Aura entering other than as a
  resolving Aura spell chooses what it enchants (CR 303.4f,
  `replacement::enchants`), or stays where it is (CR 303.4g).
- **Mandatory loops** (CR 104.4b, built). The engine counts resolutions in a row
  in which no player chose among several answers, other than by passing priority
  (`State::note_resolution`, `State::note_choice`). Past eight, it compares a
  canonical hash of the state as each resolution ends with a stored one that it
  replaces at each power of two. The canonical hash is the state's `Hash`
  without the budgets, reading each object's incarnation as nothing, each
  reference to one as how many zone changes ago it was current, and each
  timestamp as its rank (`beryl_kernel::repeats`). A match ends the game in a
  draw. The counter and the hash are 16 bytes of the state with padding, so a
  game is still a pure function of its choices; the state's `Hash` leaves them
  out, so no pinned hash changed. Worldgorger Dragon with Animate Dead is the
  scenario.
- **Merged permanents** (CR 731, built). The permanent stays the object it was,
  in its slot, with its counters, attachments and timestamp (`State::merge`).
  The card that merges moves to `Zone::Merged`, which comes after
  `Zone::Nowhere` so no `ZoneSet` holds it, with `Memo::PartOf` naming the
  permanent (`State::pile` lists them). The permanent's copiable values are kept
  by a copy effect: the top card's face plus `Addition::every_ability` for each
  other card (R7). Over, the permanent's definition becomes the merging card's
  and `Memo::Really` remembers its own; under, a copy's exceptions stay. Layer 1
  adds the other cards' keywords (`State::copy_layer_one`), and their triggered
  abilities are matched from the table's listeners of their faces, on events and
  as the permanent leaves (`trigger::elsewhere`), in a game with mutate
  (`Capability::Merges`, which adds every battlefield kind to `listens_away`). A
  copy of the permanent copies its values, every-ability additions included.
  When the permanent leaves, `State::move_object` moves its other cards to the
  same zone, in the order they merged rather than an order their owner chooses.
  Mutate is `CastingOption::Mutate`, whose spell ability only the mutating spell
  has (`Face::spell_ability_with`); `Effect::Mutate` asks over or under
  (`Purpose::Mutate`) and raises `Event::Mutated`, and a mutating spell whose
  target is illegal enters as a creature (CR 702.140b). Not yet built: the
  activated, mana, static and replacement abilities of the cards under a merged
  permanent (`State::abilities` and the layers read only the top face's), and an
  effect removing those abilities. A checkpoint keeps the permanent's values and
  not the cards under it.
- **Phased-out permanents** (CR 702.26; built by by-bajq.8.4). A phased-out
  permanent is in no battlefield list and no battlefield bitboard, and in a
  bitboard of its own that the untap step reads. Its zone, incarnation and
  stored state are untouched, so nothing triggers, and what is attached to it
  phases with it. `Object::on_battlefield`, a test of the battlefield index,
  says "on the battlefield and phased in" for the readers of one object, as
  cheaply as a test of the zone. Each permanent that phases out directly under a
  player other than its base controller remembers that player
  (`Memo::PhasedOutUnder`), whose untap step phases it in. The keyword phasing
  is not built: no card has it.

### 8. The decision protocol

- **Who answers** (CR 724). `Decision` gains `decider` (built: 320 bytes, no
  spare byte in the 312). A player-controlling effect is an Effects entry on the
  controlled player that `State::begin_turn` takes as that player's turn begins,
  setting `Turn::controller`. Every decision is built for its own player, and
  `beryl_engine`'s decision function, with the two places an inline answer
  source is asked mid-resolution, sets the decider from `State::decider`: no
  question builder knows of control. `State::knows` answers for the controller
  what the active player knows, and the next turn's beginning keeps it known, so
  every `Observation` for the decider joins both. The rollout loop and every
  agent are called for the decider. A search node belongs to the decider, so its
  value is the decider's; the greedy policy, deciding for another player, picks
  what it ranks lowest for them. A scenario's actions are the decider's, as in
  XMage's tests. A save records the decider of an answer where it is not the
  player (version 11), and older answers are their player's.
- **Hidden picks** (CR 101.4a). When a player picks a card from a zone the other
  player cannot see, the slot is public and the card is not. Built:
  `Effect::Choose` with `hidden` is one question, "none" and the candidates,
  asked of every player whatever their hand holds and never answered silently,
  since which cards pass the filter is hidden too. Its answer is recorded as the
  slot picked, not as an index into candidates a redealt state may not have;
  `Resolving` marks those entries in its padding (no size change). The filter is
  the effect's one hidden pick's, which determinization for the other player
  reads from the stack top and deals the slot a card it accepts. The
  information-independence test
  (`a_hidden_pick_redealt_shows_and_asks_the_same`) covers the case. Showing the
  pick face down, set apart from the hand, is a follow-up for the stage.
- **Naming** is in group 6. Its prompt, its save description and its view are
  built here with the rest of the protocol.
- **The view.** A `PlayerView` says who it is acting for, which permanents are
  phased out, which cards are under a merged permanent, and the definition a
  copy shows.

## Designed, not built

- **Subgames and restarts.** A restart builds a new game from part of the old
  one. A subgame needs a stack of states and would live only in `Large`.
- **Banding** changes who divides combat damage. It belongs behind a capability
  in the damage division.
- **Casting from a library during a search** needs the casting machine to run
  inside a resolution that is itself paused.
- **Shared zones.** `state::ZoneKey` says how a shared library joins it. Owner
  and "whose library" are separate questions from then on.
- **Wishes.** A sideboard is a set of cards per player and no slots: a wished
  card takes a slot of the pool as a card, not a token.
- **The repeat shortcut** (CR 733). The session replays a recorded run of
  choices N times, matching each by what it does, as a save does. The engine
  needs nothing.
- **Split cards** in a hand or graveyard have both halves' characteristics. That
  is one more value of the face `State::copiable` answers for.

## Bars and validation

| Check               | What it proves                                                                                            | Command                               |
| ------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| Card scenarios      | Each ability does what its text says, with CR citations                                                   | `just test-rust`                      |
| Mutation check      | Every change to one part of a card's effect data changes what one of its scenarios shows                  | `just test-rust`                      |
| Oracles             | Scenarios agree with XMage and Forge, or the divergence is in `oracle-divergences.json` with its citation | `just oracle <cards>`                 |
| Classic corpus      | The named interaction of each keystone and its partner comes out as the rules say                         | `just test-rust`                      |
| Fuzzing, exhaustion | Long random games and every short line from a scenario's end break no invariant                           | `just fuzz`, `just exhaust`           |
| Replay differential | Inline rollout answers and paused replay reach the same state hash                                        | `just test-rust`                      |
| Absent cost         | The WU mirror's rollouts cost what they did (R2)                                                          | `just bench-count`, `just count-gate` |
| Present cost        | Each step of a rollout with a family in play costs within 1.25 times the same step without it (R3)        | `just count-gate --family <name>`     |
| Saves               | The frozen saves replay, and `rollouts_never_change` holds                                                | `just test-rust`                      |
| Playable            | Each keystone's spec moments pass the generic UI checks from both seats                                   | `just specs --shots <card>`           |

The classic interactions held as scenarios: Blood Moon with Urborg; Humility
with Opalescence in both orders; Oko's Elk wearing Equipment; Dress Down against
Tarmogoyf; Chains of Mephistopheles with Notion Thief; Containment Priest
against Flickerwisp and Animate Dead; Painter's Servant with Force of Will; Mind
Bend on Dandân and on Blood Moon; Phantasmal Image copying a copy; Thespian's
Stage copying Dark Depths; Trinisphere with Force of Will's alternative cost;
Animate Dead on Worldgorger Dragon with nothing else to return.

**The count after the foundations.** The gate's bar is 390,293,470 and the count
at staging 8c868ebb is 393,590,215, 0.84% over it, of the 1% tolerance the whole
epic shares: group 1 added about 0.19% after its review, group 2 about 0.06%,
group 3 about 0.55%. Groups 4 to 8 have about 0.16% left between them, so
by-bajq.3.17 wins back group 3's unexplained rises before they start, and each
of their tickets holds its own change to the count at or below zero, or names
where its instructions go.

**Room in the state.** After groups 1 to 3, `State<Small>` measured 20,344 bytes
(R4). Reducing `Object` from 24 to 22 bytes lowered it to 19,840 bytes after
alignment of the enclosing state. The later groups used part of that room.

Every rules bug a mechanism exposes gets a [bugs.md](bugs.md) entry saying how
its class could be prevented, ideally through the type system.

## Sequencing

1. **Events and listeners.** No keystone. Everything else stands on it.
2. **Layers.** Blood Moon, Humility, Opalescence, Oko.
3. **Replacement and prevention.** Chains of Mephistopheles, Life from the Loam,
   Rest in Peace, Containment Priest, Mother of Runes.

A Mason audits the kernel's contract after these three. Then, in parallel:

4. **Characteristics in every zone, and text.** Painter's Servant, Mind Bend,
   Underworld Breach.
5. **Copies.** Phantasmal Image, Thespian's Stage, Fork.
6. **Costs, prohibitions and mana.** Trinisphere, Pithing Needle, Cavern of
   Souls.
7. **Stack and object identity.** Stifle, Animate Dead, Gemrazer, Teferi's
   Protection.
8. **The decision protocol.** Show and Tell, Mindslaver.

## Risks

- **A design here is wrong.** These are proposals written before the code. Each
  group's foundation ticket is told to amend its section, and the Mason after
  the foundations reads them against what was built.
- **The absent-cost gate.** `State::copiable`, the zone move and the event
  pipeline are on every path. R7 and the listener masks are shaped so that they
  read no new field, but only the count shows it.
- **The transaction around a step** (group 3) is new ground: `next` has never
  been replayed outside a resolution. The replay differential test is extended
  to it before any card depends on it.
- **The oracles disagree with each other.** These cards are where XMage and
  Forge are most likely to differ. The Comprehensive Rules decide, and each
  divergence is recorded with its citation.
- **Words as private types** touch every filter in the vocabulary and every card
  file's builders. The builders keep their names, so the card files should not
  change; if they must, that is 48 files now and would be thousands later.

## Census

`just census` counts, over the pool's cards, which mechanics in Forge's card
scripts the vocabulary lacks. It reads only kind names (ability APIs, trigger
modes, static modes, replacement events and keywords) and never keeps a
parameter or emits a definition (engine-architecture.md, K16).
`data/census-coverage.json` lists the kinds `beryl_card` covers, each with its
entry; a test fails when an entry no longer exists. The report goes to
`$BERYL_CACHE/census.json`, ordered by the cards that only that kind blocks. On
2026-11-12, 19.0% of 32,177 pool cards used only covered kinds. The top 15
uncovered kinds, with the cards each blocks alone and in all:

| Kind                  | Alone | Cards |
| --------------------- | ----: | ----: |
| `trigger:ChangesZone` | 1,814 | 6,641 |
| `effect:ChangeZone`   | 1,210 | 5,155 |
| `static:Continuous`   | 1,094 | 4,199 |
| `effect:Effect`       |   315 | 1,622 |
| `effect:PumpAll`      |   305 |   919 |
| `effect:Animate`      |   239 |   781 |
| `effect:Tap`          |   190 | 1,426 |
| `static:ReduceCost`   |   178 |   485 |
| `effect:Charm`        |   154 |   759 |
| `effect:DamageAll`    |   150 |   384 |
| `effect:Untap`        |   149 |   458 |
| `keyword:Trample`     |   145 |   990 |
| `effect:Cleanup`      |   140 | 2,503 |
| `keyword:etbCounter`  |   140 |   463 |
| `trigger:DamageDone`  |   127 |   799 |

Some kinds are broader than one vocabulary entry (`ChangeZone` covers return,
exile and put-into-library), so they stay uncovered until a single entry
matches. `effect:Cleanup` is Forge bookkeeping that Beryl needs no counterpart
for; it stays listed because the file has no "not needed" state.

This epic moves the first three rows: look-back triggers for anything that
leaves, the general zone-change instruction that Animate Dead and Show and Tell
need, and static abilities over a set. Breadth after it follows this table.
