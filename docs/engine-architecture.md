# Engine Architecture

*How Project Beryl's rules engine is built for search. Decided 2026-11-04 on the
evidence in [rules-engine-research.md](rules-engine-research.md); measured the
way [benchmarking.md](benchmarking.md) describes.*

This is the design of the Rust rules engine and of the boundaries that search,
the desktop UI and drafting build on. It is not the Phase 1 implementation plan;
that plan is written against this document. The twenty-two decisions it rests on
are marked **D1** to **D22** where they apply and listed again in the appendix.

## 1. Goals

- **Exact rules for one card pool.** The 540 cards of the V1 cube in
  `data/vintage-cube-2026-10-07.csv`, plus the five basic lands that decks add
  at deckbuilding. Two players, 40-card decks, both decklists public (**D19**).
  Nothing is approximated inside the engine.

- **Search throughput.** The targets, in the `search` build (§18),
  single-threaded on a Super core, with caches hot, measured on the cube rollout
  benchmark (§18, **D16**):

  | Metric                                                    | Target                                   |
  | --------------------------------------------------------- | ---------------------------------------- |
  | `size_of::<State>()`                                      | 20 KiB or less, asserted at compile time |
  | Copy a state                                              | 170 ns or less                           |
  | One decision: `next` plus `apply`, averaged over rollouts | 1.5 µs or less                           |
  | The rollout policy's choice, per decision                 | 300 ns or less (**D21**)                 |
  | Heap allocations on the hot path                          | 0                                        |
  | Decisions per turn under the search profile               | reported; about 8 expected (**D18**)     |
  | Games ending in `Stalled` under random play               | below 0.5%                               |

  Where the numbers come from:

  - **The workload** is rollouts as MCTS runs them: from mid-game positions
    between real cube decks, under the rollout policy (§19.1) and the search
    profile (§15). Random games stay for fuzzing and stall rates, but they
    undercount the decisions search pays for, because a random agent spends its
    fetchlands and instants at once.
  - **Per decision**, dirac costs 4.54 µs in its random games (its 2.7 µs figure
    is a single-transition microbenchmark), so 1.5 µs is about a third of dirac
    in play. The layout in §4 is designed around this budget.
  - **Decisions per turn** multiply it. Under exact stops, a policy that holds a
    fetchland or an instant faces 16 to 20 empty-stack priority passes a turn on
    top of about 5 real decisions. Smart stops (§15) bring a turn to about 8
    decisions, an estimate. At 1.8 µs per decision with the policy, a 24-turn
    game then costs about 350 µs, about 2,900 games per core-second; the
    machine-wide figure that follows is a consequence, not a target.
  - **The copy** target is what the hardware does: a hot 20 KiB copy measured
    167 ns on an M5 Max Super core, store-bandwidth bound at about 120 bytes per
    ns, and 12 KiB measured 110 ns. Rollouts copy once each, so the copy matters
    for tree nodes (§19.1), not for rollouts.

- **Hardware.** Apple M5 Max: 6 Super cores with 128 KiB of L1 data cache each,
  12 Performance cores with 64 KiB, 128-byte cache lines, 64 GiB of memory. A 20
  KiB state and its scratch buffers fit in L1 on either kind of core.

**Non-goals.** Cards outside the cube, more than two players, hidden decklists,
WebAssembly, parsing Oracle text, and any mode that plays a different game from
the real one.

**The rules surface the cube needs.** From the card list: the full stack with
APNAP trigger ordering, replacement and prevention effects, layers including
type-changing lands (Urborg, Yavimaya), copy effects (Phyrexian Metamorph,
Phantasmal Image, Thespian's Stage, Shifting Woodland, Lutri), control change
(Oko, Dack Fayden, Wishclaw Talisman, Coveted Jewel), the monarch (Palace
Jailer), energy (Galvanic Discharge), poison and infect (Blightsteel Colossus),
phasing, level up (Hexdrinker), sagas (Urza's Saga, Fable of the
Mirror-Breaker), vehicles and crew, equipment and reconfigure, transforming
double-faced cards, modal double-faced cards, split and adventure cards, extra
turns (Time Walk, Emrakul), emblems, storm (Brain Freeze, Tendrils of Agony),
delve, improvise, Phyrexian mana, votes (Council's Judgment), opponent choices
during resolution (Karn, Scion of Urza; Palantír of Orthanc), piles (Liliana of
the Veil), face-down exile (Memory Jar, Shelldock Isle), coin flips (Mana
Crypt), and alternative win and loss conditions (Thassa's Oracle, Jace, Wielder
of Mysteries). Absent, and so not designed for: controlling another player, game
restarts, dungeons, wishes, day and night, dredge, morph and manifest.

## 2. Principles

01. **One exact engine** (**D2**). Speed comes from the representation and from
    restriction profiles (§15), smart stops above all, that offer fewer
    decisions during search. Every transition is rules-exact, so any line search
    finds is a legal line.
02. **The engine never calls the AI.** It is a state machine: `next` advances
    until it needs a choice or the game ends, `apply` consumes one choice, and
    the pending decision lives inside the state.
03. **The state is a flat `Copy` value** with fixed capacities and no heap
    (**D3**, **D4**). Everything static lives in shared read-only tables: one
    for the cube, and one built per game from the two decklists (**D20**).
04. **Code for bodies, data for interfaces** (**D1**). What the engine must
    inspect about an ability is typed Rust data; what an ability does is a Rust
    function.
05. **Decisions at the granularity of distinct consequences** (**D6**).
    Candidates that lead to the same game state are one candidate.
06. **Determinism.** A game is a pure function of the decks, the seed and the
    sequence of choices. No clocks, no global state, no randomly seeded hashing.
07. **The engine tracks who knows what** (**D9**). Agents, rollout policies and
    the UI see an Observation, a borrowed view that masks what the viewer does
    not know, never the state.
08. **Budgets with an honest outcome** (**D11**). Magic is Turing complete, so
    every loop has a bound, and running out is a distinct terminal.
09. **Correctness by black-box scenarios**, checked against XMage and Forge
    (**D13**).
10. **No `unsafe`.** The workspace denies it, and nothing here needs it: u8
    indices into fixed arrays bounds-check in a cycle the predictor hides.

## 3. The engine in one loop

```rust
pub fn next(s: &mut State, t: &GameTable, x: &mut Scratch, a: &mut dyn Answers) -> Step;
pub fn apply(s: &mut State, t: &GameTable, x: &mut Scratch, a: &mut dyn Answers, c: Choice);

pub enum Step { Decide(Decision), Over(Outcome) }
pub struct Choice(pub u8);
```

- `CardTable` is immutable, built once at startup from `beryl_cards`, and shared
  by every thread as a `&'static`.
- `GameTable` is built at game start from the two decklists (§6.4) and shared,
  read-only, by every thread searching that game. It holds the `CardTable`.
- `Scratch` is per-thread working memory: candidate buffers, the mana solver's
  arrays, the event buffer the UI reads, and a spare `State` used by
  transactional replay (§7.3). It is never copied with the state.
- `Answers` is where a card body's mid-resolution choices come from (§7.3).
  `Replay` answers from the choice record and pauses the body when the record
  runs out, for tree nodes and the UI; the rollout policy answers inline and
  never pauses.
- A rollout is the loop below. The search crate inlines it, so the "callback"
  shape costs nothing where it is the right shape:

```rust
let mut s = root; // State is Copy
loop {
    match next(&mut s, t, x, &mut policy) {
        Step::Over(outcome) => break outcome,
        Step::Decide(d) => {
            let c = policy.choose(Observation::new(&s, d.player), &d);
            apply(&mut s, t, x, &mut policy, c);
        }
    }
}
```

**Why the engine does not call the AI.** A synchronous callback is the fastest
structure for a rollout, and the loop above matches it; inside a rollout the
policy even answers mid-resolution choices inline (§7.3). It fails everywhere
else. Tree search must be able to hold a position at any node and copy it to
expand; a position paused inside a Rust function has no copyable representation,
so no node could sit at a mid-resolution choice. The choices that decide games
in this cube are exactly those: what to fetch with Demonic Tutor, what to put
back with Brainstorm, what to keep from a scry, what the opponent puts down with
Show and Tell. Under callbacks the AI answers them by heuristic because it
cannot copy the position and simulate forward. DeepScry and Ironsmith both
started with callbacks and both had to bolt on checkpoint-and-replay to get back
into a decision. Here the paused position is a value (§7.3), so a tutor choice
is an ordinary node with forty legal children.

## 4. State layout (D3, D4)

### 4.1 Shape and budget

```rust
#[derive(Clone, Copy)]
pub struct State {
    objects: [Object; 256],        // base data, one per slot          ~6 KiB
    battlefield: Fixed<u8, 128>,   // slots currently on the battlefield
    derived: [Derived; 128],       // layer results, by battlefield index ~5 KiB
    zones: Zones,                  // bitboards and the ordered zones    ~1 KiB
    players: [Player; 2],
    turn: Turn,
    stack: Fixed<StackEntry, 64>,                                    // ~2 KiB
    triggers: Fixed<PendingTrigger, 64>,                             // ~1 KiB
    delayed: Fixed<DelayedTrigger, 32>,
    effects: Fixed<FloatingEffect, 64>,
    pending: Pending,              // the open question and its choice record
    knowledge: [Bitboard; 2],      // who knows the identity in each slot
    rng: Rng,                      // two SplitMix64 streams
    budgets: Budgets,
    flags: Flags,                  // derived_dirty and friends
}
const _: () = assert!(size_of::<State>() <= 20 * 1024);
```

- `Fixed<T, N>` is an in-house fixed-capacity vector: `[T; N]` plus a `u8`
  length, `Copy` whenever `T` is. Pushing past `N` is a stall (§12.3), never a
  panic in release.
- `Bitboard` is `[u64; 4]`: one bit per slot. Zone membership, tapped, "is a
  creature", "controlled by player 1" and every filter the engine evaluates are
  AND and OR over these, in the style of chess bitboards.
- There are no pointers, `Vec`s, `Box`es, maps or `fn` pointers anywhere in
  `State`. Copying it is one `memcpy`; hashing it is a walk over plain fields;
  serializing it needs no custom code.
- The estimate above lands near 17 KiB. The assertion at 20 KiB leaves room for
  what the card work discovers; growth past it must be argued for in a
  benchmark.
- A hot 20 KiB copy costs about 170 ns (§1). A leaner layout of about 12 KiB
  would save about 60 ns per copy, which a rollout pays once, and would not
  change what bounds tree memory, the search's node-storage policy (§19.1). So
  the layout stays generous.

### 4.2 Objects and slots

Every card, token and spell copy occupies one of 256 slots, addressed by a `u8`.
The object in a slot:

| Field                         | Size | Notes                                                                                       |
| ----------------------------- | ---- | ------------------------------------------------------------------------------------------- |
| `card: u8`                    | 1    | Index into the game's table (§6.4). Tokens and copies point at their definition too         |
| `incarnation: u16`            | 2    | Bumped on every zone change (§5)                                                            |
| `zone`, `owner`, `controller` | 2    | Small enums and player bits                                                                 |
| `flags`                       | 4    | tapped, summoning sick, attacking, blocking, face down, transformed, phased out, token, ... |
| `damage: u8`                  | 1    | Marked damage this turn                                                                     |
| `counters`                    | 5    | +1/+1, -1/-1, loyalty, and one (kind, count) pair for the long tail                         |
| `attached_to`, `linked`       | 2    | Equipment and auras; what exiled, imprinted or hid this card                                |
| `copy_of: u8`                 | 1    | Layer 1 copiable values when this object is a copy, as a game-table index                   |
| `timestamp: u16`              | 2    | Battlefield entry order, for layers                                                         |
| `chosen`, `face`              | 2    | A chosen color or type; which face is up                                                    |

About 22 bytes, so the table is under 6 KiB.

- **Deck cards get fixed slots.** Each player's deck, up to 60 cards, is
  assigned slots by a random permutation drawn from the hidden RNG at game
  start. Slot numbers therefore carry no information about identity or library
  position, which closes the leak Arcana found when it allocated IDs in decklist
  order.

- **Tokens and spell copies** share the remaining slots, about 130 of them,
  allocated from a free bitboard and recycled once they cease to exist. The
  incarnation counter makes a stale reference to a recycled slot detectable.

- **Emblems** are a per-player bitmask over the handful of emblem kinds the cube
  has, not objects. The continuous-effect machinery treats an emblem as a source
  like any other.

- **Capacities**, each a stall when exceeded:

  | Quantity                      | Capacity |
  | ----------------------------- | -------- |
  | Players                       | 2        |
  | Object slots                  | 256      |
  | Cards per deck                | 60       |
  | Permanents on the battlefield | 128      |
  | Stack entries                 | 64       |
  | Pending triggers              | 64       |
  | Delayed triggers              | 32       |
  | Floating continuous effects   | 64       |
  | Targets per stack entry       | 8        |
  | Choice record per process     | 32       |

  The heaviest known users: Brain Freeze and Tendrils of Agony at storm 10 to 20
  for the stack, Sheoldred or Orcish Bowmasters against a Wheel of Fortune for
  triggers, and Thopter Foundry with Sword of the Meek, Myr Battlesphere,
  Lingering Souls and Elspeth's doubling for tokens.

### 4.3 Zones

- **Membership** is a bitboard per zone per player: library, hand, battlefield,
  graveyard, exile, command, plus the shared stack. Each object also carries its
  zone, and a debug invariant keeps the two in agreement.
- **Order** is kept only where the rules read it: `library` and `graveyard` as
  `Fixed<u8, 60>` per player (Corpse Dance and Shallow Grave read the top of a
  graveyard), and the stack. Hand, battlefield and exile are unordered.
- **Linked information** rides on the object: `linked` points at the object that
  exiled, imprinted or hid it, so Skyclave Apparition, Chrome Mox, Memory Jar,
  Shelldock Isle and Agatha's Soul Cauldron need no side tables. Dauthi
  Voidwalker's void counters are ordinary counters on exiled objects.
- **Companions.** Lurrus, Lutri and Zirda can serve as companions when a drafted
  deck qualifies, and Lutri's condition is trivially met in singleton. Whether
  to support the mechanic is open (§20); if so, a per-player companion slot is
  the whole cost.

### 4.4 Players and turn

`Player` holds life, poison, energy, the mana pool, monarch, land drops this
turn, the emblem mask, and the per-turn history counters that rules read: spells
cast this turn by each player for storm, cards drawn this turn for Narset and
Sheoldred, life gained and lost, creatures that died, lands that entered.
**Every history fact a rule reads is a counter maintained at the mutation
chokepoint.** None is derived from a log, which is the trap DeepScry fell into
when its rollouts ran with the log off.

The mana pool is six `u8` color counts plus a short list of restricted mana
entries, each tagged with its restriction: Mishra's Workshop's artifact-only
mana, Relic of Sauron's, Delighted Halfling's legendary-only mana.

`Turn` holds the turn number, active player, step, the priority holder and pass
count, the queue of extra turns, and any inserted combat phase.

### 4.5 Queues

- `stack`: 32-byte entries with the object slot, kind (spell, activated,
  triggered, copy), controller, up to eight targets with their damage divisions,
  modes, X, the chosen costs, and an 8-byte last-known-information record for
  triggers that need one.
- `triggers`: abilities that have triggered and wait to be put on the stack,
  with their source, event payload and LKI.
- `delayed`: delayed triggered abilities with their duration and source (Mana
  Drain, Sneak Attack, Through the Breach, Memory Jar, Fable's Reflection).
- `effects`: continuous effects from resolved spells and abilities, with source,
  timestamp, duration, layer, affected-set filter and modification
  (until-end-of-turn pumps, Oko's Elk, control changes, protection from Giver of
  Runes, haste from Sneak Attack).

## 5. Identity, zone changes and last-known information

CR 400.7: an object that changes zones becomes a new object. The engine models
that by bumping the slot's `incarnation` on every zone change and clearing the
battlefield-only fields (damage, counters, attachments, flags). References
elsewhere are `ObjectRef { slot: u8, incarnation: u16 }`, so a target that left
and came back is recognized as a different object and the spell fizzles, as the
rules require.

Leaves-the-battlefield abilities look back in time (CR 603.10). The zone-move
pipeline (§8.2) therefore matches look-back triggers **before** the move, using
the object's derived characteristics as they were, and copies an 8-byte LKI
record (power, toughness, counters, controller, types) into the pending trigger
and later its stack entry. Bodies read LKI from there. Token slots are not
recycled until the state-based pass that removed them has finished enqueuing
triggers.

## 6. Cards: data for interfaces, code for bodies (D1)

### 6.1 The split

The transition budget is about 20,000 instructions. Nearly all of it goes to
machinery that is identical however card text is expressed: legal-move
generation, payability, zone moves, trigger matching, state-based actions, layer
recomputation. A body like Lightning Bolt is three primitive calls, and
dispatching those through a `match` on a typed enum would cost one or two
percent. So the choice between data and code for effect bodies turns on three
other things:

- **Resumability.** Resolution must stop for a player's choice without the
  engine calling anyone. A suspended Rust function cannot be copied; §7.3 shows
  how plain Rust bodies pause anyway.
- **Inspectability.** The engine must look inside abilities: which spells have a
  legal target now, what mana a permanent can make, which events a trigger
  listens to, what layer a static effect sits in, whether an activated ability
  is a mana ability. Search and evaluation want the same facts. As opaque code
  these become hand-maintained metadata on 540 cards, and a mismatch is a silent
  rules bug.
- **Hot predicates.** Target and affected-set filters run per candidate per
  legality check. They must be bitboard operations, never tree walks.

Hence the split: **everything the engine inspects is typed Rust data; what an
ability does is a Rust function.** No scripting language, no text parsed at
runtime, no general-purpose VM. Both compile ahead of time into one static
table.

### 6.2 The table

```rust
pub struct CardDef {
    pub name: &'static str,
    pub faces: &'static [Face],          // 1, or 2 for DFC, MDFC, split, adventure
}
pub struct Face {
    pub cost: ManaCost, pub types: Types, pub subtypes: Subtypes,
    pub colors: Colors, pub power: Option<Pt>, pub toughness: Option<Pt>,
    pub loyalty: Option<u8>, pub keywords: Keywords,
    pub abilities: &'static [Ability],
}
pub struct Ability {
    pub kind: AbilityKind,               // Spell, Activated, Mana, Triggered, Static, Replacement
    pub cost: Cost,                      // mana, tap, sacrifice, discard, life, exile, ...
    pub timing: Timing,                  // sorcery speed, instant speed, "any time you could cast an instant"
    pub targets: &'static [TargetSpec],  // filter, count, optional
    pub modes: Modes,                    // choose N of these, each with its own targets
    pub listens: EventMask,              // triggered and replacement: which event kinds
    pub matches: Filter,                 // triggered and replacement: which events
    pub layer: Option<LayerSpec>,        // static: layer, sublayer, affected set, modification
    pub produces: ManaSet,               // mana abilities: colors, amount, restriction
    pub body: fn(&mut EffectCtx) -> Flow,
}
pub type Flow = Result<(), Pending>; // Err carries the question to ask
```

- `Filter` is a small enum over bitboard-backed predicates: creature, controlled
  by, tapped, mana value at most, has counter, subtype, color, and combinations.
  Evaluating one is a few ANDs over `State`'s bitboards.
- `fn` pointers are fine in the table because the table is `&'static` data
  shared by all threads; they are banned from `State`, which stores only
  game-table indices and ability indices.
- A generated `names` module (`names::LIGHTNING_BOLT`, built from the cube CSV)
  makes a misspelled card a compile error in both card files and scenarios.
- One file per card under color directories in `beryl_cards`. The file holds the
  `CardDef`, its bodies, and its black-box scenarios (§17).

### 6.3 Bodies

Bodies are straight-line Rust against `EffectCtx`, which wraps the working
state, the tables, the scratch space and the answer source (§3):

```rust
fn lightning_bolt(ctx: &mut EffectCtx) -> Flow {
    let target = ctx.target(0); // legality and fizzling are the caller's job
    ctx.deal_damage(ctx.source(), target, 3);
    Ok(())
}

fn brainstorm(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    ctx.draw(you, 3);
    for _ in 0..2 {
        let card = ctx.choose_card(you, Zone::Hand, Filter::Any)?;
        ctx.put_on_top(card);
    }
    Ok(())
}
```

Every `ctx.choose_*` call asks the answer source (§3): `Replay` pops the next
recorded answer or, when the record has none, returns `Err(Pending)` through
`?`, carrying the question to ask; the rollout policy answers inline. The
primitives (`deal_damage`, `draw`, `move_to`, `create_token`, `add_counters`,
`gain_control`, `search_library`, `each_player`, `reveal`, `shuffle`, ...) are
the only way a body touches the state, and each builds the event that flows
through §8. Painter's Servant's color change on cards in every zone, and the
cost modifiers of Elite Spellbinder, Zirda and Leyline Binding, are reached
through explicit engine hooks rather than the battlefield layer pass.

### 6.4 The per-game table (D20)

Both decklists are public (§13), so the cards a game can contain are known when
it starts: the two decks plus every token they can create. `GameTable` is built
from them once per game:

- **Dense game-local indices**, at most 256, which is what `State` stores. The
  hot static data for those cards sits contiguously, a few KiB that stay in L1:
  types, costs, timing class, keywords, listener masks and mana production, with
  each mana source's flexibility rank (§10).
- **Capability masks** per game and per player: whether any card can create a
  replacement effect, a layer effect or an instant-speed action. A mechanic that
  is absent costs one AND.
- **Static filter parts** become per-slot attribute bitboards in `State` (card
  types, colors, mana value), rebuilt from the table wherever identities bind:
  game start, shuffles and determinization. Most target and affected-set checks
  are then one to three ANDs.

A build compiled for one matchup (only its cards, absent mechanics as constants,
bodies and filters inlined) gains in the same places the table already targets.
What remains is constant-folding table reads; dispatch to inlined bodies still
compiles to a jump table, an indirect branch like the call through a `fn`
pointer. It is estimated at single-digit percent over the table, costs a minute
or more of compilation per matchup, and does nothing for draft simulation, where
every candidate pick is a new deck. It is tried once the cube rollout benchmark
exists and adopted only if it clears 10%. Engine variants compiled per
combination of mechanics (about 1 to 2%) and a JIT, which would need the engine
expressed as an IR (the path **D1** rejected), were considered and dropped.

## 7. Decisions (D6) and transactional replay

### 7.1 The decision value

```rust
pub struct Decision { pub player: u8, pub kind: DecisionKind, pub candidates: Candidates }
pub enum Candidates { Objects(Bitboard), Options(Fixed<Opt, 64>) }
```

`DecisionKind` is the closed list of factored single picks: a priority action
(pass, cast a card with a given face or alternative cost, activate an ability,
play a land, a special action such as ninjutsu), one target, one mode, X, a
payment plan, one attacker, an attack target, one blocker, the attacker it
blocks, a damage division, the next trigger to put on the stack, the next
replacement to apply, one card from a set, the next card in an ordering, yes or
no, a number, a color, a player, a vote, a pile assignment, keep or mulligan.
Every answer is `Choice(u8)`: an index into the candidates.

### 7.2 What the engine settles itself

The exact profile offers a choice only across **distinct consequences**:

- A choice with one legal answer is applied silently.
- A player with no legal action passes priority silently. In this cube that is
  rarer than it sounds: an uncracked fetchland, Wasteland, a manland, Lightning
  Bolt or Brainstorm is a legal action at almost every priority pass, so under
  the exact profile most of the 16 to 20 empty-stack passes in a turn are
  decisions. Search profiles cut them with smart stops (§15).
- Interchangeable objects collapse to one candidate: two untapped basic Forests,
  two identical tokens with the same counters and nothing attached. The
  signature compared is card, zone, controller, counters, damage, flags and
  attachment, with no pending reference to the object.
- Triggers from the same source card and ability with the same targets are
  equivalent and ordered silently; non-equivalent simultaneous triggers are
  ordered by their controller under APNAP.
- Mana payments and combat damage divisions are offered only when plans differ
  in outcome (§10, §11).
- Alternative and optional costs are separate cast candidates: Force of Will for
  mana and Force of Will exiling a blue card are two options, and the card to
  exile is a following pick.

The result is that the surface the UI offers and the surface search expands
differ only by the restrictions of §15, and the branching factor never
multiplies by factors that cannot change the game.

### 7.3 Transactional replay

Casting and combat declaration are engine-generic and hot, so they are explicit
state machines whose step lives in `pending`. Card bodies are the long tail, and
they pause differently:

1. When a body must run, the engine copies the committed state into the scratch
   `State` and runs the body there with the choice record so far.
2. If the body completes, the scratch state becomes the committed state and the
   record is cleared.
3. If the body asks a question the record does not answer, the scratch state is
   discarded, and the question is stored in `pending` with the record intact.
   The committed state is still the pre-resolution state.
4. The next `apply` appends the answer to the record and runs the body again
   from step 1.

A paused position is therefore just pre-state plus a short record, and search
copies it like any other state. Randomness inside a body replays identically
because the RNG is part of the copied pre-state. A body with k choices costs k
short re-runs; the record holds 32 answers, which covers Balance and Doomsday,
and overflowing it is a stall. Events emitted during a run that ends pending are
discarded with the scratch state. Viewing a paused position (§13) replays the
body once into scratch and takes the view there, so the viewer sees, for
example, the three cards Brainstorm just drew.

An ability declares `may_pause` in its data; only those bodies run on the
scratch copy, which costs about 170 ns, and a debug assertion catches a body
that asks a question without declaring it. The alternative, a hand-written step
enum per multi-step card, was rejected (**D1**) as the error-prone half of the
work for no measurable speed.

**Rollouts never pause.** Their answer source is the rollout policy, called
inline, so a `may_pause` body runs once on the committed state, with no scratch
copy and no re-run. This cube is full of such bodies: twelve fetch-style lands,
tutors, Thoughtseize, Brainstorm, Ponder, Preordain, Sensei's Divining Top and
Fact or Fiction. At 10 to 20 resolutions a game, replaying them would cost about
2 to 3% of rollout time. Both paths must reach the same state for the same
answers, so a differential test records a rollout's inline answers, replays them
through `Replay`, and compares state hashes at every decision (§17.4). An undo
log that rolls back on pause was considered and rejected: exact undo for every
primitive is the bug class that broke DeepScry's rewind.

### 7.4 Casting

Casting follows CR 601.2 as an explicit machine: announce, choose face and
alternative cost, choose modes, choose targets, choose X, determine total cost
with modifiers, choose a payment plan, pay, then emit the cast event. Spells
whose cost cannot be paid or that have no legal target are never offered, so the
machine never has to rewind.

## 8. Events, replacement effects, triggers, state-based actions (D10)

### 8.1 The event

`Event` is one small `Copy` enum covering only the kinds some cube card can
replace, prevent or trigger on, about thirty: zone change (with its reason:
destroy, sacrifice, discard, mill, exile, bounce, cast, play, die), damage,
draw, life gain and loss, counter placement and removal, tap and untap, token
creation, cast, activate, attack, block, combat damage, target chosen, gain
control, transform, phase out, step and phase boundaries, win, lose. Where the
rules treat happenings as simultaneous, one event carries a set: a Wrath is one
`ZoneChange` with a bitboard of victims, a Wheel of Fortune one `Draw` of seven
per player. That keeps "whenever one or more" and simultaneous-death rules
exact. Flag writes and cache updates never go through events.

### 8.2 The pipeline

For each event a primitive builds:

1. **Replacement lookup.** A per-kind bitmask index names the active replacement
   and prevention effects that listen to this kind: static abilities of
   permanents, floating effects, and the cube's few from other zones. Their
   filters run against the event.
2. **Ordering.** Self-replacement effects apply first (CR 616.1a). If two or
   more others apply and are not equivalent, the affected player or the affected
   object's controller orders them, one pick at a time (CR 616.1). Each applies
   at most once to the event.
3. **Look-back trigger matching** for zone changes off the battlefield,
   capturing LKI (§5).
4. **Execution** of the final event: the actual mutation of `State`.
5. **Trigger matching** for everything else through the same kind of index, then
   enqueueing with the event payload.
6. **Sink.** If a `Scratch` event buffer is attached, the executed event is
   appended for the UI or a replay. Search attaches none.

### 8.3 Triggers

Pending triggers go on the stack the next time a player would receive priority,
in APNAP order, each controller ordering their own non-equivalent ones.
Intervening-if conditions are checked when the ability triggers and again on
resolution. Reflexive triggers ("when you do") are enqueued by the body that
created the condition. Delayed triggers carry their duration and fire from
step-boundary events. State triggers (Dark Depths) are checked with state-based
actions and enqueued once until they resolve. Triggers from outside the
battlefield are indexed too: Sword of the Meek in a graveyard, Emrakul's cast
trigger from the stack.

### 8.4 State-based actions

One routine, run whenever a player would receive priority and repeated until
nothing changes, bounded at 32 iterations: zero or negative life, ten poison,
drawing from an empty library, lethal damage and deathtouch, zero toughness, the
legend rule (a choice), zero loyalty, annihilating +1/+1 against -1/-1 counters,
illegally attached auras and equipment, tokens and spell copies outside their
zones, a saga with enough lore counters, Dark Depths with no ice counters. All
simultaneous actions are one event batch.

## 9. Derived characteristics: the layers (D5)

Base characteristics live in `Object` and `CardTable`. The result of applying
continuous effects lives in `derived`, indexed by battlefield position:

```rust
pub struct Derived {
    pub power: i8, pub toughness: i8,
    pub types: u8, pub supertypes: u8, pub colors: u8, pub controller: u8,
    pub protection: u8,
    pub keywords: u64,                   // flying, first strike, deathtouch, ...
    pub subtypes: [u64; 2],              // the cube's creature, land, artifact and enchantment types
    pub granted: [AbilityRef; 4],        // abilities granted by other objects
}
```

- Any mutation that can change a layer result sets `derived_dirty`: a permanent
  entering or leaving, a counter, a floating effect starting or ending, a
  control change, a transform, a chosen color.
- The next query recomputes every battlefield object from base by applying the
  active static abilities and floating effects in layer, sublayer and timestamp
  order: copy (1), control (2), text (3), types (4), colors (5), abilities (6),
  then characteristic-defining P/T, set P/T, modify P/T and counters, switch (7a
  to 7d).
- When no continuous effect is active, which is most of a game, `derived` is a
  straight copy of base: one branch.
- **CR 613.8 dependencies** are resolved by a pairwise test run only when two or
  more effects share a layer: if applying B would change what A applies to or
  what A does, A waits for B. The cube appears to have no same-layer dependency
  today; the test corpus (§17) keeps that claim honest.
- Reads never recompute. Legality checks and state-based actions read arrays.
- Copy effects (layer 1) use copiable values: `copy_of` names the definition
  copied, and the few exceptions (Metamorph is an artifact in addition,
  Phantasmal Image is an Illusion with its sacrifice trigger) are flags the
  recompute honors.

The research found every engine's layer bugs in incremental apply-and-unapply
schemes. Recomputing from base over compact arrays is simpler and, for a cube
with at most a handful of active effects, faster.

## 10. Mana (D7)

The engine owns a payment solver. Inputs: the cost after modifiers (generic,
colored, hybrid, Phyrexian, X), the floating pool with its restrictions, delve
and improvise sources, and every untapped mana ability with its produce set, its
own cost and side effects (Lotus Petal, Black Lotus and Lion's Eye Diamond are
consumed; Mana Confluence, the Talismans and Ancient Tomb cost life; Mox Opal
needs metalcraft; Gaea's Cradle and Tolarian Academy count; Lion's Eye Diamond's
timing restriction stops it paying for a spell being cast). A color-count
prefilter answers "payable at all" for every card in hand on each priority, and
an exact matching runs only for candidates that pass.

What becomes a decision:

- Among plans that differ only in which lands or rocks are tapped, the solver
  picks one and offers nothing. It picks by one of two heuristics, the only ones
  inside the exact surface, named as such (**D7**):
  - **At tree nodes and in the exact profile**, the plan that leaves the largest
    set of still-payable costs. That is a search over distinct plans. On a
    mid-game cube mana base, say five lands, a Mox and Sol Ring, a three-mana
    spell has 10 to 20, and testing the other costs against each is an estimated
    0.5 to 1.5 µs per cast, growing combinatorially with Moxen, Signets,
    Talismans and dual lands.
  - **In rollouts**, greedily. Each mana source carries a flexibility rank in
    the per-game table (§6.4): fewest colors first, then no other ability, then
    no restriction. Payment taps the least flexible sources that cover the
    colors, in tens of nanoseconds. It always pays legally and occasionally taps
    a source wanted later in the turn.
- A decision is offered when plans differ in consumed resources: permanents
  sacrificed, life paid, cards exiled for delve, counters removed, restricted
  mana spent, floating mana left over.
- Standalone mana-ability activations are offered only when timing-restricted
  (Lion's Eye Diamond in response to one's own spell) or when the ability has
  effects beyond mana; otherwise pre-tapping is dominated by paying during the
  cast.
- A `manual_mana` profile flag turns every tap into an offered choice for a
  human who wants it.

Mana abilities that target are not mana abilities (CR 605.1a): Deathrite
Shaman's first ability uses the stack.

## 11. Combat (D8)

Combat is factored into single picks with legality-aware candidates:

- **Attackers**, one at a time: a creature, then its defender only when a
  planeswalker makes that a real choice, then "done". "Done" is withheld while a
  creature that must attack still legally can, and restrictions are checked on
  each pick, so every completed declaration satisfies CR 508.1.
- **Blockers**, one at a time: a blocker, then the attacker it blocks. Evasion
  (flying and reach, shadow, menace, Questing Beast's restriction) prunes the
  candidates; a menace attacker is offered only while two blockers remain
  possible, so the declaration never dead-ends.
- **Damage.** Current rules have no damage assignment order: a blocked
  attacker's controller divides damage among blockers freely, and trample needs
  lethal assigned to every blocker before any reaches the player. A lone blocker
  without trample is automatic. Otherwise the engine enumerates the distinct
  outcomes, which blockers die and how much reaches the player, and offers
  those. Deathtouch makes one damage lethal. First-strike and regular damage
  steps exist only when a first or double striker is in combat.
- **Special actions.** Ninjutsu is offered to the attacking player in the
  declare blockers step while an unblocked attacker exists. Crew taps creatures
  one pick at a time until the power requirement is met. Exalted, attack
  triggers (Adeline, Ragavan), combat-damage triggers (Coveted Jewel, Palace
  Jailer's monarch) and planeswalker damage fall out of the event pipeline.

## 12. Turn structure, budgets and outcomes (D11)

### 12.1 Turns

The turn is a small program of steps: untap, upkeep, draw, main, beginning of
combat, declare attackers, declare blockers, combat damage (twice with first
strike), end of combat, main, end, cleanup. Extra turns queue; extra combat
phases insert. Each step boundary is an event, so "at the beginning of" and
"until end of turn" are triggers and durations like any other. Priority passes
in succession with an empty stack advance the step; with a non-empty stack they
resolve the top object. Cleanup discards to hand size, removes damage and ends
until-end-of-turn effects, and repeats if that caused a trigger.

The game starts with the London mulligan: keep or mulligan, then the cards to
bottom one pick at a time.

### 12.2 Outcomes

```rust
pub enum Outcome { Win(u8), Draw, Stalled(StallReason) }
```

`Draw` is a rules draw: both players lose at once. `Stalled` is the engine
giving up, with a named reason, and it is distinct so that logs, tests and
search statistics never confuse the two.

### 12.3 Budgets

Defaults, overridable per profile: 500 decisions and 2,000 trigger resolutions
per turn, 200 turns per game, 32 state-based iterations per check, and the
capacities of §4.2. Running out is `Stalled`. Search scores it as 0.5 unless an
evaluation says otherwise; every test treats a stall as a failure; benchmarks
report stall rates.

The cube has real unbounded loops: Devoted Druid under Swift Reconfiguration
makes unlimited mana, and Walking Ballista turns it into damage one activation
at a time. Under per-decision budgets a search undervalues such lines. The fix
is a later profile meta-action that proposes a loop and a count as one decision
in the spirit of the rules on shortcuts, executed as that many real activations;
it is noted in §20 and not part of Phase 1.

## 13. Hidden information (D9, D19)

**Decklists are public.** Both players know both decklists, so hidden
information is only where the known cards are: in hands, in library order and
face down in exile.

Slots are public handles. Both players see that the object in slot 17 moved from
a hand to the top of a library; what is hidden is which card sits behind the
slot.

- `knowledge[p]` is a bitboard: player p knows the identity in slot s. It is
  updated only at chokepoints: draw, look at, reveal, cast, play, face-down
  exile, and the search and tutor primitives. Thoughtseize sets the caster's
  bits for a whole hand; Sensei's Divining Top sets its owner's for the top
  three; Gitaxian Probe and Elite Spellbinder look at a hand, Collective
  Brutality reveals one, and all three set bits the same way.
- **Shuffling rebinds identities among the shuffled slots** and clears their
  knowledge bits, in addition to reordering. Without the rebinding, the owner of
  a Brainstormed-back Ponder would know where it went.
- Face-down exile (Memory Jar, Shelldock Isle) is a known-to-owner slot in
  exile; cards exiled face up are known to both.
- **Observation** is a borrowed view of the state for one viewer.
  `Observation<'a>` wraps `&State` and the viewer, and its accessors mask on
  read: a slot the viewer does not know reads as `card = UNKNOWN`, with the
  fields that could betray it hidden. Agents and rollout policies receive views
  only, at about one bit test per masked read. A masked copy would cost about
  170 ns per decision, over 10% of the decision budget, and a third 20 KiB
  buffer in the 64 KiB L1 of a Performance core. The UI receives an owned masked
  copy built from the view, so the webview never holds a `State`.
- **Determinization** takes an Observation and an RNG and deals the unknown
  remainder of each decklist into that player's unknown slots, keeping
  everything the viewer knows. With public decklists the candidate multiset per
  hidden group is exact, so no belief model over the cube is needed.
- **The information-independence test** runs in CI: the same agent must make
  identical decisions from the full state and from a re-determinized one over
  many games, which catches a leaking accessor.

Leak checklist carried over from the research: the RNG seed, ID allocation
order, true library order, and any accessor that returns an opponent's hidden
zone.

## 14. Determinism, randomness and hashing

- `Rng` is two SplitMix64 streams inside `State`: one for shuffles and deck slot
  assignment, one for everything else (coin flips, random discards, random
  bottom orders). Separate streams keep shuffle outcomes stable across replays
  whose other random events differ. Search gives each copy a derived seed so
  sibling rollouts diverge.
- No `std::collections::HashMap` with `RandomState`, no wall clock, no statics
  with interior mutability, no thread-locals. A test greps the engine crates for
  the banned types, as Arcana does.
- A replay is `(decklists, seed, choices)`. Replaying one reproduces the game
  bit for bit, and a scenario can be cut from a replay.
- `State` implements `Hash` over its fields, computed on demand for tests,
  divergence checks and the first transposition tables. The mutation chokepoints
  are placed so an incremental Zobrist hash can be added without touching card
  bodies if transposition tables earn it.

## 15. Restriction profiles

A `Profile` restricts which decisions are offered and sets budgets. The exact
profile offers everything §7.2 describes. Search profiles, used at tree nodes
and in rollouts, shrink it:

- **Smart stops** (**D18**). A player gets priority only at a stop: their own
  main phases, after attackers and after blockers are declared, the opponent's
  end step, and whenever the stack is non-empty. Elsewhere both players pass
  silently. A turn has 8 to 10 steps in which both players get priority, and a
  player holding a fetchland or an instant turns most of those 16 to 20 passes
  into decisions; smart stops bring a turn from about 20 to 25 decisions to
  about 8. Lines that need another stop, such as an upkeep activation of
  Sensei's Divining Top, are lost unless the stop set adds them.
- The default damage division, all triggers ordered by rule, X capped, and
  attacks on the player only.
- In rollouts only, greedy mana taps (§10) and mid-resolution choices answered
  inline by the rollout policy (§7.3).

Every profile is sound in the sense that any line it produces is a legal line of
the real game; it may miss options, which is a search-quality question, not a
correctness one. A profile that restricts the opponent's options makes the
searcher optimistic, which is tuned, not fixed.

## 16. Crate map (D12)

| Crate            | Holds                                                                                                                                    | Depends on                    |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| `beryl_engine`   | `State`, events, layers, mana, combat, turn structure, decisions, the Observation view, the `CardDef` types, `GameTable` and `EffectCtx` | nothing in the workspace      |
| `beryl_cards`    | 540 card files under color directories, the generated `names` module, the static table, per-card scenarios                               | `beryl_engine`                |
| `beryl_scenario` | Scenario value types, the strict runner, the JSON dump                                                                                   | `beryl_engine`                |
| `beryl_search`   | The `Agent` trait, `RandomAgent`, the heuristic baseline, the rollout policy, later MCTS, determinization, evaluation                    | `beryl_engine`                |
| `beryl_draft`    | Pack generation, draft state, the draft bot, deckbuilding                                                                                | `beryl_cards`                 |
| `beryl_bench`    | Benchmark support, as today, plus the engine's `_counts`, `_abba` and `_timing` targets and the cube rollout benchmark                   | the above as dev-dependencies |
| `beryl_app`      | The Tauri shell: sessions, IPC commands, events                                                                                          | all of the above              |

The engine knows no card and no card knows the engine's internals: cards use
`EffectCtx` and the table types only. One card edit recompiles `beryl_cards`,
not the engine's tests. Exporters that turn dumped scenarios into XMage and
Forge inputs are Python in `tools/`, per AGENTS.md.

## 17. Testing and verification (D13)

### 17.1 Scenarios

Each card file declares `SCENARIOS: &[Scenario]`, built with a typed builder and
serializable with serde:

- **Setup:** cards by `names::` constant in any zone for either player, life,
  counters, tapped, attachments, the top cards of a library in order, the turn
  and step, the active player.
- **Steps:** cast with targets, modes and X; activate; play a land; attack;
  block; advance to a step; answer a named decision.
- **Assertions:** life, zone contents and counts, power and toughness, counters,
  controller, tapped, the stack, the outcome.

The runner drives Beryl through the public decision API only. It is strict:
every decision the engine offers must be scripted or marked auto, and an
unexpected decision fails the test. That is the opposite of XMage's lenient
default, and it keeps the decision surface itself under test.

### 17.2 Oracles

`beryl_scenario dump` writes every scenario as JSON. Python exporters generate:

- **XMage**, the primary oracle. A JUnit class per card under
  `Mage.Tests/src/test/java`, on `CardTestPlayerBase`: `addCard`, `setLife`,
  `castSpell`, `activateAbility`, `playLand`, `attack`, `block`, `addTarget`,
  `setChoice`, `setModeChoice`, `setStrictChooseMode(true)`, `setStopAt`,
  `execute`, and the `assert*` family. Its build files indicate that classes
  dropped into that tree run under Maven's defaults after a one-time build.
- **Forge**, the secondary oracle. Its `forge.game.GameState` text format can
  set up any board (`p0life`, `p0hand`, `p0battlefield` with `|Tapped`,
  `|Counters:P1P1=1`, `|Id:n`, `|AttachedTo:id`), but its scripted-player
  harnesses reject modes and combat damage, so the exporter emits only the
  scenarios inside Forge's vocabulary and reports the rest as unsupported.
  Forge's headless `sim` command plays AI-versus-AI matches from decklists with
  logs, which serves statistical differential testing later.

The oracle lane runs outside `just ci`'s 300-second budget, on demand and
nightly.

### 17.3 The bar for a supported card

A card is supported when every one of its abilities is exercised by at least one
scenario, its scenarios pass on Beryl and on XMage where exportable, and fuzz
play including the card raises no invariant. dirac's lockstep bar is the model.

### 17.4 Other checks

- **Fuzzing:** random-agent games in debug builds with invariants after every
  decision: zone bitboards agree with object zones, every stack reference names
  a live incarnation, no object is in two zones, `derived` is clean when read,
  counters and life are in range.
- **Determinism:** the same decks, seed and choices reproduce the same state
  hash at every decision.
- **Inline and replayed answers agree:** a rollout's inline answers, replayed
  through `Replay`, give the same state hash at every decision (§7.3).
- **Information independence** (§13).
- **No allocation:** a counting allocator in tests asserts zero heap allocations
  across a random game.
- **Perf counters:** deterministic logical-operation counts per random game
  (state copies, layer recomputes, solver runs, events) with tolerance bands, in
  the style of phase.rs, as the stable signal CI can gate on.

## 18. Performance discipline (D16, D22)

Every target in §1 gets a `_counts` Gungraun benchmark and a `_timing` Criterion
baseline in Phase 1, following `docs/benchmarking.md`: deterministic inputs
pinned by a test, instruction counts first, ABBA timing to confirm on a quiet
machine, single-threaded on a Super core. Gates come once baselines exist.

**The cube rollout benchmark** is the workload the targets are measured on: a
pinned corpus of mid-game positions between real cube decks, cut from
heuristic-agent games, rolled out under the rollout policy and the search
profile to the horizon Phase 2 chooses. It reports decisions per turn and per
rollout, nanoseconds per decision for the engine and for the policy, and
rollouts per core-second. Until the Phase 1 heuristic agent exists, random games
stand in.

**Builds.** A `search` Cargo profile builds the benchmark and headless research
binaries: fat LTO, `panic = "abort"` and `codegen-units = 1`, with
`-C target-cpu=apple-m4` passed by the recipes that build it, since a Cargo
profile cannot set the CPU; `apple-m4` is what `native` resolves to here under
rustc 1.98. Profile-guided optimization follows once the cube rollout benchmark
exists, trained on it. It needs `llvm-tools` in `rust/rust-toolchain.toml`, and
BOLT, the usual next step on Linux, does not support macOS binaries.

- Estimated gains for a branchy engine: PGO 5 to 15%, fat LTO and abort a few
  percent together, the CPU target 0 to 2%.
- Code changes are compared without PGO, by counts and ABBA; PGO applies only to
  the build behind the headline numbers.
- The app keeps the release profile until a measured gain argues for moving it
  over. Fat LTO and abort are whole-binary settings, so in the app they would
  cover Tauri's dependency graph too.

Rules for the hot path:

- `u8` indices and bitboards; no `Vec`, no maps, no `Box<dyn>`, no `Rc`.
- One `memcpy` per copy; no `Clone` impls that do work. A rollout copies once,
  at its start; nothing copies per decision.
- Fast paths for the common emptiness: no active replacement effects, no dirty
  derived data, no pending triggers, no stop for either player (§15).
- Per-kind bitmask indexes for replacement effects and triggers, rebuilt when
  the battlefield changes, so matching touches only listeners.
- `#[inline]` on primitives, `#[cold]` on stall paths, no formatting or logging
  in the engine crate.
- Everything static precomputed: into `CardTable` at startup (per-card bitmasks
  of event kinds listened to, keyword sets, mana produce sets) and into
  `GameTable` at game start (§6.4).

## 19. Boundaries

### 19.1 Search

Phase 1 ships a baseline (**D15**); the real search is Phase 2. The engine
already provides what that search needs: cheap copies, factored decisions inside
the state, the `Observation` view and determinization, profiles, budgets, the
state hash, per-thread `Scratch`, inline answers, and the inlined rollout loop.

```rust
pub trait Agent {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice;
}
```

The baseline is a scripted heuristic with a static evaluation: play a land, cast
the most expensive castable spell with sensible targets, attack when the simple
combat math says safe, block to avoid lethal or trade up, pass otherwise. It
ships beside a `RandomAgent` used for fuzzing, and its games supply the
positions of the cube rollout benchmark (§18).

**The rollout policy** (**D21**) is separate from the baseline, and cheaper:

- It reads per-card scores that the search crate precomputes per game, indexed
  by game-table index (§6.4), plus simple combat rules: attack when no untapped
  blocker can kill the attacker, and chump only to prevent lethal.
- It makes no copies and no lookahead. Its budget is about 300 ns per decision,
  20% of the engine's, tracked by the cube rollout benchmark.
- It reads the same `Observation` view as agents and answers mid-resolution
  choices inline (§7.3).

Lookahead would cost about 10 µs a decision for five candidates (a copy, an
`apply` and an evaluation each), seven times the engine. Cheap is also better:
Cowling et al. found rollouts driven by a weaker, reduced-rules player beat the
expert player's by about 10 percentage points.

The intended search, from the research: ensemble determinized MCTS at the root,
many shallow determinizations rather than few deep ones, each factored decision
a node, rollouts under the rollout policy to a horizon Phase 2 chooses (full
games, or a few turns scored by the static evaluation), common random numbers
across candidates, and root parallelization across determinizations on all 18
cores with no shared mutable state. Then a learned value function at the leaves,
and EPIMC-style deferred evaluation for bluff-sensitive spots. ISMCTS is an
experiment, not the default.

**Node storage** is a Phase 2 decision. At about 10,000 new nodes per second per
thread, a 20 KiB state in every node would be 3.6 GB/s across 18 threads, or 36
GB for a 10-second search, and restoring one from DRAM takes about 630 ns.
Search can replay choices from the root, or keep states only in nodes visited at
least twice; a state is a pure function of the root and the choices (§2), so
either is exact.

### 19.2 Sessions and IPC (D14)

`beryl_app` owns a `GameSession` per game on a dedicated thread running the
step/apply loop. When the pending decision belongs to the human, the session
emits a Tauri event with an owned masked copy of the human's Observation (§13),
the Decision with its candidates described by public slot, and the engine events
since the last view for animation, then waits for one
`choose(session, decision_seq, index)` command. AI decisions run on the session
thread, and a search may use a pool, so IPC never blocks. The webview never sees
a `State`. Every session records seed, decklists and the choice log as a replay
file, and `ui/src/ipc.ts` keeps the typed wrappers for the commands and events.

### 19.3 Draft

`beryl_draft` generates eight seats of three 15-card packs from the 540-card
list, tracks picks and pass direction, and exposes a `DraftAgent` with the same
shape as `Agent`. The Phase 1 bot is a pick-order rating with color commitment,
the Forge and XMage baseline, and the deckbuilder picks a two-color 23 with 17
lands including basics. A human drafts and builds in the UI. Learned card
representations and simulation-based evaluation are later work, and the latter
is one more reason the engine must be fast. Simulation plays a new deck for
every candidate pick, so it runs on the generic engine with per-game tables
(§6.4), never on builds compiled per matchup.

## 20. Open questions

- **Companions.** Lurrus, Lutri and Zirda can be companions in cube. Supporting
  the mechanic costs a sideboard slot per player and a special action; whether
  the cube experience wants it is a Phase 1 plan decision.
- **Loop shortcuts.** The "repeat N times" meta-action of §12.3, and how search
  should propose counts.
- **613.8 corpus.** Which cube interactions, if any, exercise same-layer
  dependencies; the scenario suite should contain the candidates.
- **Observation size over IPC.** The owned masked copy is about 17 KiB; the
  webview may want a reduced `PlayerView` instead, derived from the view.
- **Forge's reach as an oracle**, given its scripted harness's limits, versus
  investing in XMage only.
- **Rollout horizon and node storage** (§19.1): full games, or a few turns and
  an evaluation; replay from the root, or states in nodes visited twice.
- **Per-matchup builds** (§6.4): the experiment, once the cube rollout benchmark
  exists, adopted at 10% or more.
- **To measure once the engine runs:** how often layers recompute with equipment
  attached all game (§9), and what declaring combat one creature at a time costs
  a rollout against declaring a whole attack at once (§11).

## Appendix: decisions

| ID  | Decision                                                                                                                                                   | Why                                                                                                                                                                                  |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| D1  | Typed Rust data for ability interfaces, Rust functions for bodies; transactional replay for pauses at tree nodes and in the UI, inline answers in rollouts | Dispatch cost is 1 to 2%; the engine must inspect abilities; bodies must pause without callbacks or step enums; rollouts never need to pause, and replaying there would cost 2 to 3% |
| D2  | One exact engine; speed from representation plus restriction profiles                                                                                      | dirac shows exact and fast coexist; two implementations double the card work; search exploits approximations                                                                         |
| D3  | Flat `Copy` state, fixed capacities, no heap                                                                                                               | One-`memcpy` copies and zero allocations are the proven pattern; copy-make suits determinized root-parallel search                                                                   |
| D4  | u8 slots, 256 objects, decks to 60, generous queues, about 17 to 20 KiB; copies 170 ns or less                                                             | Fits L1 on both core types; token and storm decks do not stall; emblems as a bitmask; a 20 KiB copy measured 167 ns and rollouts copy once, so a leaner layout buys little           |
| D5  | Base plus derived arrays, full recompute when dirty, pairwise 613.8 test                                                                                   | Reads are array loads; the cube has few active effects; incremental schemes are where layer bugs live                                                                                |
| D6  | Offer only distinct consequences                                                                                                                           | One surface for the UI and search, which search profiles only restrict; branching never multiplies by outcome-neutral factors                                                        |
| D7  | Mana solver picks taps (greedily in rollouts, keeping the most costs payable at tree nodes), offers resource trade-offs, `manual_mana` flag                | Payability needs a solver anyway; sacrifices and life are real decisions; tap choice rarely is; searching tap plans costs about 1 µs a cast                                          |
| D8  | Sequential combat picks with legality-aware candidates; division by distinct outcomes                                                                      | Whole-declaration decisions are combinatorial; defenders and divisions are real decisions                                                                                            |
| D9  | Engine-tracked knowledge over public slots; shuffles rebind identities; Observation as a borrowed view                                                     | The chokepoints are in the engine; every engine that tracked knowledge elsewhere leaked; a masked copy per decision would cost over 10% of the budget                                |
| D10 | Typed `Event` enum for about 30 kinds, per-kind indexes, set-carrying events                                                                               | One replacement and trigger protocol; everything-is-an-event is slow; per-primitive hooks drift                                                                                      |
| D11 | Distinct `Stalled(reason)` terminal scored as a draw                                                                                                       | Budgets are unavoidable; a stall must be distinguishable from a rules draw in tests and statistics                                                                                   |
| D12 | Crates: engine, cards, scenario, search, draft, bench, app                                                                                                 | The engine knows no card; card edits do not rebuild rules tests; the AI cannot reach engine internals                                                                                |
| D13 | Scenarios as Rust-built serde values, strict runner, exported to XMage and Forge                                                                           | Typo-safe authoring with the compiler's help; one scenario, three engines                                                                                                            |
| D14 | Rust `GameSession` on its own thread; Observations over IPC; replay files                                                                                  | AI thinking never blocks the window; hidden identities never reach the webview                                                                                                       |
| D15 | Scripted heuristic baseline behind an `Agent` trait over Observations                                                                                      | Plays sanely in Phase 1 and supplies the benchmark's positions; no API change for MCTS                                                                                               |
| D16 | Per-core budgets measured on cube rollouts as MCTS runs them; metrics first-class                                                                          | The per-decision budget drives the layout; random games undercount the decisions search pays for; machine-wide numbers reward degenerate games                                       |
| D17 | This document at `docs/engine-architecture.md`, engine plus boundaries                                                                                     | Reference material every session reads; the interfaces the UI and AI consume are written down                                                                                        |
| D18 | Search profiles give priority only at smart stops                                                                                                          | A held fetchland or instant makes most of a turn's 16 to 20 priority passes into decisions; smart stops cut decisions 2.5 to 3 times, which no codegen recovers                      |
| D19 | Decklists are public; hidden information is where the known cards are                                                                                      | The AI is built for open decklists; determinization deals each list's unknown remainder, so no belief model is needed                                                                |
| D20 | A dense per-game table over the game's known cards; per-matchup builds only on a measured gain of 10% or more                                              | Decision cost is generic machinery whose work depends on the board; the table gets most of specialization's gain and serves draft simulation, where every pick is a new deck         |
| D21 | A table-driven rollout policy with no copies or lookahead, about 300 ns per decision                                                                       | Policy cost multiplies every rollout decision; lookahead costs seven times the engine; weaker rollout players gave better estimates in Cowling et al.                                |
| D22 | A `search` build with fat LTO, abort and the M4 CPU target, then PGO trained on the rollout benchmark                                                      | An estimated 5 to 20% for a build profile; the app follows only once the gain is measured                                                                                            |
