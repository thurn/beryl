# Rules Coverage

How the rules engine grows from the cards implemented so far to every Magic
card, without slowing rollouts. The rule throughout: a mechanism that nothing in
the game uses costs one predictable branch.

*Revised 2026-11-13 in a design interview. The first version (2026-11-06) listed
26 hard effect types and sequenced them by the cube cards they unlocked. This
version turns that list into one piece of work: 24 keystone cards, each chosen
because it forces a decision that would be expensive to change once thousands of
cards exist. The work is tracked as an Enact epic in beads; this document is the
plan its beads are written against.*

- **In scope:** the engine mechanisms the keystone cards need, the keystone
  cards, and the partner cards their scenarios need. Each keystone is playable
  in the app.
- **Not in scope:** complete decks, breadth (more keywords, more trigger events)
  beyond what a keystone or partner needs, AI strength, and UI polish.
- **Unchanged:** the engine stays one exact, flat, `Copy` state machine built
  for search ([engine-architecture.md](engine-architecture.md) §2).

## Where the engine stands

Project 50k ([project-50k.md](project-50k.md)) rebuilt the structure: a kernel
that owns the state, a card vocabulary of effect data, and three stores for what
cards create. It added no rules. What the rules can do is still what the 48
implemented cards need.

| Mechanism           | In code today                                                                                                                                                          | What the keystones need                                                                                                   |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Layers              | A static ability affects its own permanent or the one it is attached to; the card table refuses any other set. Layers 2, 4, 6, 7a and 7c. Timestamps order everything. | Sets chosen by a filter, layers 3, 5, 7b and 7d, ability removal, CR 613.6 and 613.8, a permanent "as it would exist"     |
| Replacement effects | Five cases, each written by hand: three ways to enter tapped, "exile it instead" on one object, and a draw replaced by winning or losing                               | One pipeline for moves, draws, damage and destruction, with the ordering choice of CR 616.1 and replacements outside play |
| Triggers            | 11 kinds of event in a `u16` mask. "Dies" matches only "when this dies". Only permanents and emblems listen.                                                           | Look-back for anything leaving, state triggers, abilities that work in a graveyard or on the stack                        |
| Characteristics     | Layer results exist for permanents. Everywhere else an object has its printed values.                                                                                  | A color or an ability given to cards in every zone; characteristic-defining abilities read in every zone                  |
| Copies              | A token whose `card` is the copied definition, with one exception flag (`ObjectFlags::ONE_ONE`)                                                                        | A card that is a copy, exceptions that are copied in turn, copies with a duration, copies of spells                       |
| Rule changes        | Two things governed (casting spells, activating loyalty abilities) at two timings                                                                                      | Cost increases, reductions and floors, "can't" effects keyed on a chosen name, permissions for a class of cards           |
| Mana                | Six counts in a pool                                                                                                                                                   | Mana that remembers what it may be spent on and what it does to the spell                                                 |
| Objects             | One slot is one card or token. Abilities on the stack have no identity. Every permanent is in the battlefield list.                                                    | Abilities as targets, one permanent made of several cards, permanents that are treated as though they don't exist         |
| Decisions           | `Decision::player` both owns the decision and answers it. Every pick is public to the engine's search.                                                                 | A player who answers for another, a pick hidden until both players have made one, a name chosen from every card ever made |

`State<Small>` is 20,216 of its 20,480 bytes, and the count gate stands at
389,343,295 instructions.

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
5. **Data for interfaces and for bodies.** Every card is effect data
   (project-50k.md, K1). A mechanism is a new entry of the vocabulary and its
   arm in the rules, and the compiler finds every place a new variant matters.
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

| ID  | Decision                                                                                                                                                                                                                                                                                                             | Why                                                                                                                                                                                                                                                                                                                                   |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | The cards are chosen by what is expensive to retrofit: how card data is written, what the kernel assumes an object is, and what the decision protocol assumes. 24 keystones and about 25 partners. No complete deck.                                                                                                 | Legacy's most-played decks are mostly easy cards, so choosing by popularity delivers the hard cases late. Breadth is cheap to add later; a wrong assumption in the kernel is not.                                                                                                                                                     |
| R2  | A mechanism that the WU Control mirror does not use costs it nothing. The count gate is never raised for a new mechanism, and a bead that cannot hold it after three attempts stops the run for a human decision.                                                                                                    | This is the one bar an unattended run cannot argue its way past (project-50k.md, K1).                                                                                                                                                                                                                                                 |
| R3  | Each family of keystones has a counted rollout corpus with the family in play. Every step of those rollouts is also counted on the same state with the family taken out, and the family's steps stay within 1.25 times that. A miss after three attempts is recorded with its value and cause and files a follow-up. | The count gate cannot see what a mechanism costs when present. Two rollouts, one with the family and one without, play different games, so their difference is mostly the game's and not the mechanism's; one step from one state differs only by the mechanism. At 1.25 times, a search over such a board keeps 80% of its rollouts. |
| R4  | `State<Small>` stays within 20,480 bytes. Each bead that changes its size has an allotment and reports the size. Last-known information moves out of every waiting trigger into a shared table, which frees about 900 bytes for the rest.                                                                            | A state and its scratch copy must fit the 64 KiB first-level cache of a Performance core.                                                                                                                                                                                                                                             |
| R5  | The layers stay one full recompute from base, applied one layer at a time. A same-layer dependency (CR 613.8) is found by applying one effect to a scratch copy and seeing whether the other changed, never from a table of card pairs.                                                                              | A table of pairs cannot cover 50,000 cards. The trial runs only when two effects share a layer and one writes what the other reads.                                                                                                                                                                                                   |
| R6  | Each replaceable happening (a move, a draw, damage, destruction) builds a typed value before it changes the state, and replacement effects rewrite that value. There is no general hook on events. A step that may have to ask a player runs as a transaction.                                                       | Principle 1. A typed value per kind keeps each gate one AND and lets the compiler check each rewrite.                                                                                                                                                                                                                                 |
| R7  | An object's copiable values are a face of a definition plus a short list of additions. `Object::card` holds the definition the object currently has the values of; the card it really is, where that differs, is a memo.                                                                                             | `State::copiable` is read on every hot path, so a copy must cost it nothing. One representation serves copies, copies of copies and merged permanents.                                                                                                                                                                                |
| R8  | Outside the battlefield an object's characteristics are computed when read: its copiable values, then the few continuous effects that reach other zones.                                                                                                                                                             | Layer results for all 255 slots would add 5 KiB. Such effects are rare, and a game without one pays a length test.                                                                                                                                                                                                                    |
| R9  | A color word, land type or creature type in an ability is a value of a private type, read only through the word changes on the object the ability is printed on.                                                                                                                                                     | A text change (CR 612) must reach every such word. Making the read the only way to the value turns a missed one into a compile error.                                                                                                                                                                                                 |
| R10 | An ability on the stack is named by its place on the stack. The kernel repairs such names when an entry below the top is removed. Stack entries gain no identity field.                                                                                                                                              | A field in each of 64 entries is copied by every rollout. Only an entry above an ability can name it, so the repair touches few entries and runs only on a removal from the middle.                                                                                                                                                   |
| R11 | A merged permanent is one object in the slot it always had. The other cards of it wait in a zone state of their own. A phased-out permanent leaves the battlefield list and bitboard and keeps its zone, incarnation and everything stored about it.                                                                 | Both keep "one slot is one object" true for every reader. Everything that walks the battlefield ignores the cards underneath and the phased-out permanents for free.                                                                                                                                                                  |
| R12 | A `Decision` names both the player it belongs to and the player who answers it. A pick from a hidden zone that the other player has not seen constrains determinization. A named card is one of the game's cards or "another card".                                                                                  | Each is an assumption of the session, the saves and the UI as well as the engine, so it is cheapest to settle while few cards depend on it.                                                                                                                                                                                           |
| R13 | A loop of mandatory actions is a draw (CR 104.4b), found by a repeated position. The "repeat N times" shortcut for optional loops is designed here and not built.                                                                                                                                                    | The draw is a rule. The shortcut is a search feature, and the longest line among these cards is about 60 decisions against a budget of 500.                                                                                                                                                                                           |
| R14 | Every keystone is playable in the app from both seats with the prompt shapes and animation verbs that exist. Agents answer every new decision legally. No Arena matching, no new animation and no strength bar.                                                                                                      | The protocol changes of R12 are only proven by a session, a save and a UI that use them. Polish for cards in no supported deck is not worth its cost.                                                                                                                                                                                 |
| R15 | Eight groups. Three foundations land in order; five groups follow in parallel. One Warden per group, a Mason audit after the foundations and one at the end, one Archivist, no Sage. Foundations, keystones and reviews run on Opus, partner cards and wiring on Sonnet.                                             | The foundations edit the same kernel and rules files. Architecture drift is what the checks cannot see, and it is expensive in exactly those files.                                                                                                                                                                                   |
| R16 | Subgames, game restarts, banding and casting from a library mid-search are designed below and not built. So are shared zones, which the DanDân format needs: nothing built here may assume a library or graveyard has one owner.                                                                                     | Each needs a state that holds more than one game, or serves a handful of cards. A shared zone is a constraint on this work, not part of it.                                                                                                                                                                                           |

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
| 5     | Fork                     | A copy of a spell with its choices, new targets and a changed color                                                 | Tendrils of Agony                                          |
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

Each section is the proposed design for one group. The foundation bead of a
group may amend it where the code proves it wrong, and records the change in
this document.

### 1. Events and listeners

- **Kinds.** The listener mask is a `u16` with eleven kinds, two of which are
  not events (`RULE_CHANGE`, `DRAW_REPLACEMENT`). It becomes a typed set of 64
  kinds for triggers and a second for replacements, so a replacement's gate is
  its own AND.
- **Abilities outside the battlefield** (CR 113.6). An ability says where it
  works, defaulting from its kind. `GameTable` records, for each card, the zones
  other than the battlefield in which some ability of it works. The state keeps
  one bitboard of the slots that are in such a zone now, written by the zone
  move behind a capability. Matching walks the battlefield's listeners, then
  that set. A game whose cards have no such ability never writes or reads it.
- **Look-back** (CR 603.10). `trigger::match_leaving` matches only "when this
  dies". It becomes the match for every ability that looks back, with any
  subject, against every permanent of the batch as it last existed, so a
  creature sees the others die with it and a creature without abilities under
  Humility has no dies trigger.
- **Last-known information.** Every waiting trigger, stack entry and delayed
  trigger carries an 8-byte `Lki`: about 1,280 bytes for 160 records that are
  nearly always empty. It becomes a table of up to 16 rows keyed by the
  `ObjectRef` of the object that left, each a copy of its `Derived` row with its
  counters, written only when something that looks back matched. A reader asks
  `State::last_known`. The table empties when no waiting trigger, stack entry or
  delayed trigger names a row. This frees about 900 bytes and lets an effect
  read any characteristic of an object that has left, not only five of them.
- **State triggers** (CR 603.8). Checked with the state-based actions behind
  their own kind. Whether one is already waiting or on the stack is found by
  looking there, so no flag is stored.
- **This turn's counts.** The Trackers store gains a count kept for each player:
  spells cast this turn and cards drawn this turn, each behind its capability.
- **The present-cost corpus.** `rollout_counts` gains a corpus for each family:
  positions cut from greedy games of the WU mirror with the family's cards
  swapped in, each with a keystone in play. The gate does not compare those
  rollouts with rollouts of other positions: a card such as Glorious Anthem
  makes a game with more combat, and Trinisphere one with more passes, and
  either would move instructions per decision by more than the mechanism does.
  It pairs each step instead. As a rollout visits a state, the benchmark builds
  the control state from that same state, with the family's permanents and the
  effects they are the source of removed, applies the rollout's choice to both
  for one transition, and counts each. The control state is then thrown away, so
  the two sides never drift apart. The gate is the sum over the family's steps
  divided by the sum over the controls' (R3). A step whose choice the control
  does not offer is left out of both sums and counted in the report.

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
  filter is read against the results so far (`layers::passes`, which today
  serves one case). The refusals `Unapplied::Affected` and
  `Unapplied::Condition` go as their cases are built.
- **Ability removal.** `Derived` gains a mask of the face's abilities the
  permanent has lost, all of them for "loses all abilities". An effect that
  removes abilities also clears the keywords and grants applied before it; those
  applied after it stay. `State::has_ability` is the one question every reader
  asks: trigger matching, activation, the static scan, the replacement lookup
  and the mana solver. With no removal in effect it is a constant.
- **Continuing once begun** (CR 613.6). An effect with parts in several layers
  finds its set once, where it first applies, and keeps it though its ability is
  removed on the way. This is what makes Humility with Opalescence come out as
  the rules say, and Oko's Elk keep its 3/3.
- **Land types** (CR 305.7). Setting a land's type removes its printed abilities
  in layer 4, through the same mask. The mana ability of a basic land type comes
  from the derived subtypes, not from the card. The solver's per-card facts in
  `GameTable` stay the fast path, behind a flag that says no land's types differ
  from its card's.
- **Dependencies** (CR 613.8). In a layer with two or more effects, A depends on
  B when applying B changes whether A exists, what A applies to, or what A does
  to it. Each kind of change declares what it reads and writes among the
  characteristics, and a pair whose sets do not meet is never tried. For a pair
  that might, B is applied to a scratch copy of the results and A's existence,
  set and change are compared before and after. The effect applied next is the
  earliest by timestamp that depends on nothing still waiting; a loop falls back
  to timestamps; the order is found again after each application. Effects from
  characteristic-defining abilities are compared only with each other.
- **Timestamps** (CR 613.7). An Aura or Equipment gets a new one as it becomes
  attached, and a permanent as it turns face up or transforms. Objects that
  enter together are ordered by a decision only when two of them have effects in
  one layer that the dependency prefilter says could interact. The `u16` counter
  is renumbered, out of line, when it would overflow.
- **As it would exist** (CR 614.12). `layers::as_it_would_exist` runs the
  recompute with one entering object added as a row of scratch memory. The
  replacement pipeline calls it only for an object that has, or meets, a
  replacement of its entering. Under Blood Moon a land that would ask for life
  as it enters is a Mountain with no such ability, and enters untapped.
- **What makes the cache stale.** `layers::counters_matter` is one case of a
  general fact: a characteristic-defining ability reads something no layer
  writes, as Tarmogoyf reads the graveyards. Each card's abilities declare such
  inputs, the recompute folds them for the battlefield as it folds the
  listeners, and a mutator marks the cache stale only when its input is in that
  set.
- **A granted static ability.** An ability granted in layer 6 whose effect
  belongs to an earlier layer is found only after that layer has run. The
  recompute then runs once more with the ability present, at most three times.
  The table knows whether any card grants a static ability, so other games run
  the recompute once.

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

### 4. Characteristics in every zone, and text

- **Reading.** `query.rs` answers colors, types, keywords and "has this ability"
  for any object. For a permanent the answer is its layer result. For anything
  else it is the copiable values, then the continuous effects that reach other
  zones. The recompute lists those effects, in the order they apply, in a short
  list beside the layer results. Most games never have one, and then a read off
  the battlefield tests the list's length. A filter over a hand or a graveyard
  uses the per-slot attribute bitboards unless the list is non-empty.
- **Characteristic-defining abilities** (CR 604.3) work in every zone and are
  read the same way: Tarmogoyf's power in a graveyard is its amount, computed
  when asked.
- **Words.** A color word, a basic land type and a creature type inside an
  ability become values of private types. The only way to the value takes the
  word changes of the object the ability is printed on. Filters, parameterized
  keywords (protection from a color, landwalk), conditions and modifications
  hold them. A card's name is never changed (CR 612.2), and a mana symbol is not
  a color word.
- **A text change** is a layer 3 entry of the Effects store on one object,
  permanent or spell: a kind of word, from and to. Several compose in timestamp
  order. It does not reach an ability another object granted (CR 612.3), because
  only the object's own face is read through its changes.
- **On the stack.** An ability that goes on the stack keeps the words its source
  had then. The state holds up to four captured sets of changes for abilities on
  the stack, named by a byte in the stack entry, and empties them with the
  stack.
- **Absent.** A capability says whether any card of the game changes text. If
  none does, reading a word is the identity.
- **Shown.** The session's rendered rules text takes the same word changes, so a
  changed card reads as changed.

### 5. Copies

- **Copiable values** are a face of a definition and up to two additions in
  `Small`, more in `Large`. An addition names an entry of some card's list of
  copy exceptions (add a type or subtype, set a color, set P/T, not legendary,
  has this ability), or every ability of another face. Copying an object copies
  its additions and appends the copier's own, so a copy of a copy never walks a
  chain (CR 707.9).
- **Where they live.** `Object::card` and `Object::face` hold the definition the
  object has the values of, so `State::copiable` reads what it reads today. The
  card an object really is, where that differs, is a memo, read behind the keyed
  bit by `State::card_behind` and restored by the zone move. The additions and
  the copy's duration are a layer 1 entry of the Effects store. The copy
  mutators keep the fields equal to what layer 1 gives, and an invariant checks
  it. `ObjectFlags::ONE_ONE` becomes an addition.
- **Entering as a copy** is a replacement of the class CR 616.1c names. The
  choice is not a target. The other replacements of entering are then found
  against the copy.
- **Becoming a copy** is an instruction with a duration and the copier's
  exception.
- **Copies of spells** (CR 707.10). A copy takes a slot of the token pool on the
  stack and the original's modes, targets, X and costs paid. It is not cast.
  Where the effect allows, each target may be chosen again through the casting
  machine's target step. A copy of a permanent spell becomes a token. Storm is a
  triggered ability that works on the stack and reads the count of spells cast
  this turn as it triggers.

### 6. Costs, prohibitions and mana

- **The total cost** (CR 601.2f) is computed in one place, in the order the rule
  gives: the mana cost or an alternative cost, then additional costs and
  increases, then reductions, then an effect that sets a floor, which says of
  itself that it applies last. A static cost change is a rule change the game
  numbers, so with none in force the check is the one AND the rule sets already
  give. Delve pays generic mana while the cost is paid, so it can pay a floor.
- **Prohibitions.** `Governed` gains activating the abilities of sources a
  filter accepts, and `When` gains "never". A filter may name "the chosen name",
  read from the memo of the permanent that makes the rule. The check finds the
  makers only once the kind is in force.
- **A chosen value** is a memo of the permanent: a name, a color, a creature
  type. It is chosen as the permanent enters, through the replacement pipeline,
  in the `Catalog` prompt shape. The candidates for a name are the names in the
  game's table and "another card": every name outside both decklists has the
  same consequences (D6).
- **A class of permission.** An effect that lets a player cast every card a
  filter accepts from a zone is a rule change with casting terms. A keyword that
  does so by granting an ability to cards in a graveyard is read through group
  4's list. `cast::legal::cast_sources` scans the zone only while such a rule is
  in force.
- **Mana with memory.** A pool gains up to four entries of mana that remembers a
  restriction and a rider, both numbered by the game as rule changes are. The
  solver treats each as a class of its own. Spent on a spell, the rider becomes
  a rule change on that spell. A capability keeps the entries out of every other
  game.
- **A player.** "Your life total can't change" and protection for a player are
  rule changes on that player. The life mutators, the cost check and the
  targeting check each ask the one question.

### 7. Stack and object identity

- **Abilities as targets.** A target may be an ability, named by its place on
  the stack. Only an entry above an ability can name it. When an entry leaves
  from below the top, the kernel moves the names that pointed above it and marks
  those that pointed at it as gone. A save describes such a target by the
  ability's source and index.
- **An Aura on a card in a graveyard.** `attached_to` may name an object in
  another zone. The state-based check reads the enchant ability in force, which
  is a layer 6 result: Animate Dead loses one and gains another, and what "put
  onto the battlefield with this" means is a memo.
- **Mandatory loops** (CR 104.4b). The engine counts resolutions in a row with
  no choice but passes. Past eight, it compares a canonical hash of the state,
  which ignores incarnations and reads timestamps as ranks, with a stored one
  that it replaces at each power of two. A match ends the game in a draw. The
  counter and the hash are ten bytes of the state, so a game is still a pure
  function of its choices.
- **Merged permanents** (CR 731). The permanent stays the object it was, in its
  slot, with its counters, attachments and timestamp. The card that merges moves
  to a zone state of its own that names the permanent it is part of and its
  place in the pile. The permanent's copiable values are the top card's face
  plus an addition for every ability of each card under it (R7), so the layers,
  the listeners and a copy of it need nothing new. When the permanent leaves,
  the move expands to every card of it at once.
- **Phased-out permanents** (CR 702.26). A phased-out permanent is in no
  battlefield list and no battlefield bitboard, and in a bitboard of its own
  that the untap step reads. Its zone, incarnation and stored state are
  untouched, so nothing triggers, and what is attached to it phases with it.

### 8. The decision protocol

- **Who answers** (CR 724). `Decision` gains `decider`. The state knows whether
  a player's turn is controlled, and `State::ask` is the one place that sets
  both players. The rollout loop and every agent are called for the decider with
  an `Observation` that joins what both players know. A search node belongs to
  the decider, so its value is the decider's. A save records both players, with
  a migration for older saves.
- **Hidden picks** (CR 101.4a). When a player picks a card from a zone the other
  player cannot see, the slot is public and the card is not. The record of the
  pick notes the filter it had to pass. Determinization for the other player
  deals that slot a card the filter accepts. The information-independence test
  covers the case.
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
| Ability coverage    | Every instruction of a card is carried out by its own scenarios                                           | `just test-rust`                      |
| Oracles             | Scenarios agree with XMage and Forge, or the divergence is in `oracle-divergences.json` with its citation | `just oracle <cards>`                 |
| Classic corpus      | The named interaction of each keystone and its partner comes out as the rules say                         | `just test-rust`                      |
| Fuzzing, exhaustion | Long random games and every short line from a scenario's end break no invariant                           | `just fuzz`, `just exhaust`           |
| Replay differential | Inline rollout answers and paused replay reach the same state hash                                        | `just test-rust`                      |
| Absent cost         | The WU mirror's rollouts cost what they did (R2)                                                          | `just bench-count`, `just count-gate` |
| Present cost        | Each step of a rollout with a family in play costs within 1.25 times the same step without it (R3)        | `just count-gate --family <name>`     |
| Saves               | The frozen saves replay, and `rollouts_never_change` holds                                                | `just test-rust`                      |
| Playable            | Each keystone's spec moments pass the generic UI checks from both seats                                   | `just specs <card>`                   |

The classic interactions held as scenarios: Blood Moon with Urborg; Humility
with Opalescence in both orders; Oko's Elk wearing Equipment; Dress Down against
Tarmogoyf; Chains of Mephistopheles with Notion Thief; Containment Priest
against Flickerwisp and Animate Dead; Painter's Servant with Force of Will; Mind
Bend on Dandân and on Blood Moon; Phantasmal Image copying a copy; Thespian's
Stage copying Dark Depths; Trinisphere with Force of Will's alternative cost;
Animate Dead on Worldgorger Dragon with nothing else to return.

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
  group's foundation bead is told to amend its section, and the Mason after the
  foundations reads them against what was built.
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
parameter or emits a definition (project-50k.md, K16).
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
