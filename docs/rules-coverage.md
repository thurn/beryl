# Rules Coverage

How the engine grows from the cards implemented so far to the whole 540-card
cube, and past it, without slowing rollouts. The rule throughout: a mechanism
that nothing in the game uses costs one predictable branch.

- **Why expand.** The engine handles what the implemented cards need. Its layer
  system has no text-changing (layer 3), color-setting (5), ability-removing (6)
  or P/T-setting (7b) effects yet, so Oko, Thief of Crowns cannot be written
  today. The rest of the cube needs those and about a dozen other hard
  mechanisms.
- **The tension.** Every rule added to a hot path (priority, casting, events,
  layers) is paid in every rollout. A 100-game MCTS pairing already holds the
  machine for about 20 minutes.
- **The answer.** Index every mechanism by who listens to it, skip the work when
  nobody does, check the decklists at game start for which mechanisms can ever
  appear, and send rare heavy cases to the `Large` layout rather than growing
  `Small`.

## Where the engine stands

The architecture ([engine-architecture.md](engine-architecture.md)) has a place
for every mechanism below. The gaps are in the closed enums that describe what
cards may do, which have grown only as far as the implemented cards.

Those interfaces live in `beryl_engine/src/card.rs` as small `Copy` enums:
`Trigger`, `Replacement`, `Rule`, `Modification`. Expanding coverage mostly
means adding variants and teaching the pipeline about each one.

| Mechanism              | Design                                                     | In code today                                                                                                          | Main gap                                                                                |
| ---------------------- | ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Layers                 | Recompute from base behind `derived_dirty`, layers 1 to 7d | Copy (1), control (2), add, remove and set types (4), keywords and grants (6), one CDA (7a), modify P/T (7c), counters | Text (3), color (5), ability removal (6), set P/T (7b), switch (7d), 613.8 dependencies |
| Replacement effects    | Per-kind listener index, ordering by the affected player   | Entering tapped (three forms), Jace's empty-library win                                                                | Zone-change, draw, damage and token replacements; ordering choices                      |
| Rule changes           | Statics that change what players may do                    | Teferi's sorcery-speed lock, The Wandering Emperor's instant loyalty                                                   | Draw limits, cost taxes, "can't" effects, casting from other zones                      |
| Triggers               | Indexed by event kind, APNAP ordering, LKI                 | Enters, dies, attacks, steps, cast, tokens, discard and more                                                           | Opponent-scoped and "one or more" triggers; delayed and state triggers at scale         |
| Copy                   | A token pointing at the copied definition, exception flags | Token copies                                                                                                           | Copying a copy with exceptions; copying spells                                          |
| Mid-resolution choices | Transactional replay, inline answers in rollouts           | Implemented (`may_pause`)                                                                                              | Stress cases such as Balance                                                            |
| Capacities             | `Small` stalls into `Large`                                | Implemented                                                                                                            | Loop shortcuts                                                                          |

Two choices shape everything below. The state is one flat `Copy` value with no
heap, so new mechanisms fit in fixed-size fields. Derived characteristics are
recomputed from base rather than patched, so a new layer is a new arm in one
function, not a new undo path.

## Principles

1. **Index by listener.** Every event, replacement, trigger and rule change is
   found through a per-kind bitmask of who listens. With no listeners the check
   is one AND and a branch the predictor learns.
2. **Check the decks at game start.** `GameTable` records which mechanisms
   either deck can ever produce: text changes, casting from the graveyard, copy
   effects, control changes. A flag false all game makes its branch free.
   Per-matchup builds (architecture §6.4) are the extreme version.
3. **Rare and heavy goes to `Large`.** A mechanism that needs more room than
   `Small` has (a long choice record, many floating effects, a huge token count)
   stalls into `Large`. `Small` stays at 20 KiB or less and one `memcpy`.
4. **Exact first, restricted second.** The engine always plays the exact rules.
   Search speed comes from restriction profiles that offer fewer choices (a
   fixed trigger order, canonical payments), never from approximating a rule.
5. **Data for interfaces, code for bodies.** What the engine must inspect (the
   layer, the event kind, the zone permission) is a typed enum variant; what a
   card does is a Rust function. The compiler then finds every place a new
   variant matters.
6. **Each mechanism lands with its numbers.** `just bench-count` on the rollout
   benchmark is unchanged when the mechanism is absent, and `just count-gate`
   holds the bar.

The common failure is the opposite of the first rule: a general hook that runs
on every event in case some card cares. Engines built for completeness tend
toward that design; Beryl cannot afford it.

## The hard effect types

Sixteen, roughly hardest first. Cube cards are named where they exist; text
changes and same-layer dependencies have no cube card yet and use classic
examples.

### 1. Layers, timestamps and ability removal

**Examples:** Oko, Thief of Crowns on a creature wearing Kaldra Compleat; Dress
Down against Tarmogoyf.

**Why it's hard:** Oko's Elk ability touches four layers: type (4), color (5),
abilities (6) and base P/T (7b). If Oko acts after Kaldra attached, the creature
loses Kaldra's granted abilities but keeps its +5/+5: an 8/8 vanilla Elk.
Re-equipping gives Kaldra a new timestamp (CR 613.7e) and the abilities return.
Dress Down removes abilities in layer 6, before characteristic-defining
abilities apply in 7a, so Tarmogoyf loses its own P/T ability and is a 0/1.

**Plan:**

- Add `SetColors`, `LoseAllAbilities`, `SetBasePt` and `SwitchPt` to
  `Modification`.
- Ability removal sets a per-object mask in `Derived` that every ability lookup
  reads. Triggers, statics and activations all ask one function whether an
  ability is still present.
- Keep the dirty flag coarse until measured: count how often equipment keeps it
  set all game before splitting it per layer.

### 2. Same-layer dependencies (CR 613.8)

**Examples (outside the cube):** Blood Moon with Urborg, Tomb of Yawgmoth;
Opalescence with Humility.

**Why it's hard:** Within a layer, effects apply in timestamp order unless one
depends on another, and then the dependency wins. Blood Moon makes Urborg a
Mountain, which removes the ability that would make every land a Swamp, whatever
the timestamps. Dependency loops fall back to timestamps.

**Plan:** Run the pairwise dependency test only when two or more effects share a
layer and affect overlapping objects, which is almost never. The cube appears to
have no such pair, so this lands with the first card that creates one, plus a
scenario per known classic pair.

### 3. Text-changing effects (CR 612)

**Examples (outside the cube):** Sleight of Mind (one color word), Artificial
Evolution (one creature type word), Mind Bend (either).

**Why it's hard:** A text change rewrites words inside an ability: "destroy
target blue creature", "protection from red", "other Goblin creatures",
"islandwalk". It applies in layer 3 and doesn't touch abilities granted by other
objects (CR 612.3). In Beryl, bodies are Rust functions, so a color hardcoded in
a body cannot be rewritten.

**Plan:**

- Make every changeable word data, not code. Filters and keywords name a
  `ColorWord`, `SubtypeWord` or `LandTypeWord`, and bodies resolve them through
  one function.
- Layer 3 writes a small swap table into `Derived` for the affected object: one
  (from, to) pair per word kind, a few bytes per object.
- A `GameTable` flag says whether either deck has a text changer. If not, word
  resolution is the identity and never reads the table.
- A lint test walks every card's data and flags a color or type used inside a
  body where it should be a word.

### 4. Copy effects (CR 707)

**Examples:** Phantasmal Image copying a Phyrexian Metamorph that is copying
Grave Titan; Fractured Identity; Thespian's Stage; Reflection of Kiki-Jiki;
storm copies from Brain Freeze and Tendrils of Agony; Lutri copying a spell.

**Why it's hard:** The Image becomes a Grave Titan that is also an artifact and
an Illusion with the sacrifice trigger, because exceptions written into a copy
effect become copiable values (CR 707.9). Copying a spell copies its choices
(modes, targets, X) and may allow new targets.

**Plan:**

- Layer 1 stays a pointer swap plus a small set of exception flags: extra types,
  an extra subtype, an extra ability.
- When a copy effect starts, resolve the full copiable values once and store
  them. Never walk a chain of copies at query time.
- Spell copies are stack entries that copy the original's choices. A body asks
  for new targets through the normal `may_pause` path.

### 5. Control changes

**Examples:** Thieving Skydiver stealing Kaldra Compleat and attaching it;
"until end of turn" steal effects such as Act of Treason (outside the cube).

**Why it's hard:** Control moves in layer 2 and ripples everywhere: who untaps
it, summoning sickness ("controlled continuously since the start of your turn"),
who may activate it, which "you control" filters count it, and who chooses for
its triggers. Equipment that changes control stays attached until its new
controller moves it.

**Plan:**

- Keep control in `Derived.controller` and build the per-player "controls"
  bitboards from it in the same recompute. Every filter reads the bitboards,
  never the base field.
- Summoning sickness is a flag cleared at the start of its controller's turn and
  set again on a control change.
- Give the fuzzer a control-swap action, so every card is also played under the
  other player's control.

### 6. Replacement and prevention effects (CR 614 to 616)

**Examples:** Containment Priest with Dauthi Voidwalker against Reanimate;
Hullbreacher against Timetwister; Jace, Wielder of Mysteries drawing from an
empty library.

**Why it's hard:** When several replacements apply, the affected player or
controller chooses the order, and self-replacements go first (CR 616.1). Each
applies at most once to an event. Draws are individual events (CR 121.2), so
Hullbreacher replaces each card of a Timetwister separately, which clashes with
batching a draw-seven as one event.

**Plan:**

- Extend `Replacement` with zone-change, draw, damage and token variants, each
  in the per-kind listener index.
- Split a batched event into single events only when a per-card listener is
  present.
- Ordering is a decision only when the orders lead to different results. Search
  can fix it by a heuristic.

### 7. Mass triggers, simultaneity and APNAP

**Examples:** Sheoldred, the Apocalypse against Timetwister (seven triggers);
Hellrider; Ocelot Pride's intervening-if; Dark Depths' state trigger.

**Why it's hard:** Seven triggers have 5,040 orderings. "Whenever one or more"
fires once per batch while "whenever a" fires per object. Intervening-if
conditions are checked twice, on triggering and on resolution.

**Plan:**

- Order equivalent triggers silently: seven identical Sheoldred triggers are
  zero decisions.
- Add opponent-scoped and batch-scoped `Trigger` variants, each in the listener
  index.
- Search profiles order a controller's non-equivalent triggers by a fixed rule
  unless the order changes the outcome.

### 8. Zone changes, identity and last-known information

**Examples:** Skyclave Apparition blinked by Ephemerate; Flickerwisp; Solitude
evoked.

**Why it's hard:** When Skyclave leaves, the owner of the exiled card gets an
X/X token, where X is that card's mana value. That needs the link between
Skyclave's two abilities and look-back triggers that see the object as it was
(CR 603.10). A blinked object is a new object (CR 400.7), so anything targeting
it loses track.

**Plan:** Already designed: an incarnation counter makes a stale reference a
single compare, and pending triggers carry an 8-byte LKI snapshot. What is left
is coverage: linked abilities across more card pairs, and LKI fields for
whatever new characteristics later cards read.

### 9. Casting permissions and alternative costs

**Examples:** Underworld Breach, Bolas's Citadel, Snapcaster Mage, Lurrus,
Yawgmoth's Will, Kroxa's escape, Lightstall Inquisitor; Force of Will, Daze,
Fireblast and Snuff Out as alternative costs.

**Why it's hard:** Listing legal actions at each priority pass now means
scanning the graveyard, the top of the library and exile, not just the hand.
Each candidate needs its own cost, built from alternative costs, taxes and
reductions in the order CR 601.2f fixes.

**Plan:**

- Keep a "castable from" bitboard that only permission sources add to. With no
  Breach or Citadel out, it is the hand.
- A `GameTable` flag says whether either deck can grant such a permission. If
  not, the extra scan is a branch that is always false.
- Compute cost modifiers once per priority pass, not once per candidate.

### 10. Player-level rule changes

**Examples:** Narset, Parter of Veils and Leovold (one draw per turn), Thalia
(noncreature spells cost more), Teferi, Time Raveler (sorcery speed only),
Containment Priest (creatures that weren't cast are exiled), Hazoret (can't
attack or block with a full hand).

**Why it's hard:** These change what a player may do, not what an object is.
"Can't" beats "can" (CR 101.2), so every permission must also check every
prohibition. A draw limit interacts with replacements: Narset stops the second
draw, so Hullbreacher has nothing to replace.

**Plan:**

- Grow `Rule` into a small set of player flags and counters recomputed with the
  layers: `max_draws_per_turn`, `sorcery_speed_only`, `noncreature_tax`.
- Every check reads a precomputed field and never walks the battlefield for
  statics.
- Order the checks as the rules do: prohibitions, then permissions, then
  replacements.

### 11. Mana payment

**Examples:** Mishra's Workshop (artifact-only mana), Tolarian Academy and
Gaea's Cradle (variable output), Mana Confluence and Ancient Tomb (life costs),
Lotus Petal and Lion's Eye Diamond (sacrifice for mana).

**Why it's hard:** Which sources you tap matters: leaving up Wasteland or a
manland, paying life or not, keeping blue open for Force of Will. A naive solver
is exponential, and offering every plan blows up branching. This runs on nearly
every cast, so it is the most performance-sensitive item here.

**Plan:** Already designed: group interchangeable sources into classes and offer
only plans whose outcomes differ. What is left is restricted mana for more
cards, and a search profile that picks a canonical plan unless a strategic
alternative exists, such as keeping counterspell mana open.

### 12. Choices in the middle of resolving

**Examples:** Demonic Tutor, Natural Order, Show and Tell, Balance, Brainstorm,
Dig Through Time.

**Why it's hard:** The choices that decide games come halfway through an effect.
A callback engine can't copy a position paused inside a Rust function, so its AI
answers them by heuristic.

**Plan:** Designed and built: transactional replay, with rollouts answering
inline and never pausing. Balance is the stress case for the 32-answer choice
record. Overflowing it stalls into `Large`, which is correct but should stay
rare; the rollout benchmark should count how often it happens.

### 13. Hidden information and knowledge

**Examples:** Brainstorm putting cards back (the caster knows them, the opponent
doesn't), a fetchland shuffle erasing that knowledge, Thoughtseize, Shelldock
Isle, Jace, the Mind Sculptor's fateseal, Bolas's Citadel, Lightstall
Inquisitor.

**Why it's hard:** The engine must never leak identity, through slot numbers or
anything else, and the AI has to search positions it can't fully see.

**Plan:** One knowledge bitboard per player, updated where cards are revealed,
looked at or shuffled. Each new look, reveal or hide effect is one call into
that update, checked by an invariant. Most of the cost is in the search, not the
engine.

### 14. Cards with more than one form

**Examples:** Bonecrusher Giant and Brazen Borrower (adventures), Valki, God of
Lies (modal double-faced), Life // Death (split), Fable of the Mirror-Breaker (a
saga that transforms), Overlord of the Mistmoors (impending), Tamiyo,
Inquisitive Student (transforms); morph and disguise outside the cube.

**Why it's hard:** Each form answers "what are this card's characteristics"
differently by zone. A split card has both halves' characteristics in the
graveyard; a modal double-faced card is only its front face outside the stack
and battlefield; an adventure goes to exile and can be cast later. Face-down
permanents must hide their identity from the opponent.

**Plan:** The face is already a byte on the object. Resolve characteristics
through one function keyed by zone and face. Each form is a few cast candidates
and a characteristics rule, with no per-card special case on the hot path.

### 15. Combat

**Examples:** Goblin Rabblemaster (other Goblins must attack), Questing Beast
(evasion, deathtouch, damage to planeswalkers), Glorybringer (exert), Fallen
Shinobi (ninjutsu), Sword of Fire and Ice (protection), Lavaspur Boots (ward).

**Why it's hard:** A declaration must obey as many attack requirements as
possible without breaking a restriction, and blocks have the same structure.
Damage with first strike, double strike, trample and deathtouch has many
outcomes. Declaring a whole attack at once multiplies the branching factor.

**Plan:** Partly designed: declare attackers and blockers one at a time, and
offer damage divisions only when outcomes differ. Requirements become a
precomputed per-creature mask; the legality check maximizes them only when a
requirement exists, which is rare.

### 16. Loops and unbounded repetition

**Examples:** Underworld Breach with Lion's Eye Diamond and Brain Freeze;
Tendrils of Agony at storm 15 or more; Displacer Kitten chains; extra turns from
Time Walk and Time Warp.

**Why it's hard:** Magic is Turing complete. Without a bound, a rollout can spin
forever, and the stack, trigger queue and tokens can overflow fixed capacities.

**Plan:**

- Bound every loop and treat running out of budget as its own outcome (already
  designed).
- Add the "repeat N times" shortcut, following the rules on shortcuts, so N
  identical iterations are one decision.
- Detect repeated state hashes for loops nobody can stop.

## Sequencing

Finish the layers first: they block the most powerful cube cards today, and
every later phase reads derived characteristics. Each phase starts when the
previous gate passes and the count gate shows no regression with the new
mechanism absent.

1. **Finish the layers.**
   - Builds: color, ability removal, set and switch P/T, a per-object ability
     mask.
   - Unlocks: Oko, Thief of Crowns; Dress Down.
   - Gate: Oko with Kaldra Compleat and Dress Down with Tarmogoyf match the
     oracles.
2. **Events and rule changes.**
   - Builds: zone-change, draw and damage replacements; batch triggers; player
     rule flags.
   - Unlocks: Containment Priest, Dauthi Voidwalker, Hullbreacher, Narset,
     Leovold, Thalia.
   - Gate: Timetwister against Hullbreacher, Narset and Sheoldred matches the
     oracles.
3. **Casting permissions and copies.**
   - Builds: the castable-from bitboard, cost modifiers, spell copies, loop
     shortcuts.
   - Unlocks: Underworld Breach, Bolas's Citadel, Snapcaster Mage, Lurrus,
     Phantasmal Image.
   - Gate: Breach with Lion's Eye Diamond finishes inside its budget.
4. **Beyond the cube.**
   - Builds: text-changing effects, the same-layer dependency test.
   - Unlocks: cards outside the cube; the classic interaction corpus.
   - Gate: Humility with Opalescence and Blood Moon with Urborg pass.

The types already designed (LKI, mid-resolution choices, mana classes, combat)
grow card by card alongside these phases.

## Validation

Each mechanism ships with proof of two things: it is rules-exact, and it costs
nothing when absent.

| Check               | What it proves                                                                                            | Command                               |
| ------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| Card scenarios      | Each ability does what its text says, with CR citations                                                   | `just test-rust`                      |
| Ability coverage    | Every ability of a supported card is exercised by some scenario (the `coverage` feature)                  | `just test-rust`                      |
| Oracles             | Scenarios agree with XMage and Forge, or the divergence is in `oracle-divergences.json` with its citation | `just oracle <cards>`                 |
| Mechanism corpus    | Classic interactions per mechanism, before a cube card needs them                                         | `just test-rust`                      |
| Fuzzing             | Long random games never break an invariant or stall unexpectedly                                          | `just fuzz`                           |
| Replay differential | Inline rollout answers and paused replay reach the same state hash                                        | `just test-rust`                      |
| Instruction counts  | The rollout benchmark is unchanged when the mechanism is absent                                           | `just bench-count`, `just count-gate` |

Every rules bug a new mechanism exposes also gets a [bugs.md](bugs.md) entry
saying how its class could be prevented, ideally through the type system.

## Open questions

- **Scope beyond the cube.** Build text changes and same-layer dependencies now,
  or wait for the first card that needs them?
- **Dirty-flag granularity.** How often does equipment keep layers dirty all
  game? Measure before splitting the flag per layer.
- **Batched events.** Is splitting a batched draw only when a per-card listener
  exists enough, or should every draw be its own event?
- **Loop shortcuts.** How should search propose N for "repeat N times"?
- **Per-matchup builds.** Do game-start flags capture most of the benefit, or
  are engines compiled per matchup worth their build cost?
