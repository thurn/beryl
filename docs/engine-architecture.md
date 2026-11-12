# Engine Architecture

*How Project Beryl's rules engine is built for search. Decided 2026-11-04 on the
evidence in [rules-engine-research.md](rules-engine-research.md); measured the
way [benchmarking.md](benchmarking.md) describes.*

This is the design of the Rust rules engine and of the boundaries that search
and the desktop UI build on. It is not an implementation plan: milestone plans
are Enact epics in beads, written against this document. It was written when the
target was cube draft (the MTG Arena Powered Cube), and the project moved to
Legacy constructed on 2026-11-09. Cube cards stay as examples throughout, since
Legacy plays many of them. Milestone 1 built the engine, the game UI and the
first AI opponents for a mirror match of the deck in
`docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`, a mythic
drafter's WU Control deck from the cube's Arena run. It built the structures
described here, but only the mechanics its cards use. The Legacy decks come in
later milestones. The twenty-two decisions it rests on are marked **D1** to
**D22** where they apply and listed again in the appendix.

## 1. Goals

- **Exact rules for one card pool.** The main decks and sideboards of the Legacy
  metagame decks V1 supports, kept in `docs/sample-decks/`. Two players, 60-card
  decks with 15-card sideboards, best of three, both decklists public (**D19**).
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

**Non-goals.** Cards outside the supported decks, more than two players, hidden
decklists, WebAssembly, parsing Oracle text, and any mode that plays a different
game from the real one.

**The rules surface the cube needed.** Written for the cube, and kept because
Legacy shares much of it; the Legacy decks' own list is drawn up once they are
chosen. From the cube's card list: the full stack with APNAP trigger ordering,
replacement and prevention effects, layers including a type-changing land
(Yavimaya), copy effects (Phyrexian Metamorph, Phantasmal Image, Thespian's
Stage, Shifting Woodland, Lutri, Fractured Identity), control change (Oko, Dack
Fayden, Wishclaw Talisman, Coveted Jewel), energy (Galvanic Discharge), poison
(Etali, Primal Conqueror), sagas (Urza's Saga, Fable of the Mirror-Breaker),
vehicles and crew, equipment and reconfigure, transforming double-faced cards,
modal double-faced cards, split and adventure cards, extra turns (Time Walk,
Emrakul), emblems, storm (Brain Freeze, Tendrils of Agony), delve, improvise,
overload, evoke, flashback, Phyrexian mana, day and night (Outland Liberator),
opponent choices during resolution (Karn, Scion of Urza), piles (Liliana of the
Veil), face-down exile (Memory Jar, Shelldock Isle), manifest dread (Abhorrent
Oculus), coin flips (Mana Crypt), and an alternative win condition (Jace,
Wielder of Mysteries). Absent, and so not designed for: controlling another
player, game restarts, the monarch, votes, phasing, infect, dungeons, wishes,
dredge and morph.

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
    objects: [Object; 255],        // base data, one per slot          ~6 KiB
    battlefield: Fixed<Slot, 128>, // slots currently on the battlefield
    derived: [Derived; 128],       // layer results, by battlefield index ~5 KiB
    zones: Zones,                  // bitboards and the ordered zones    ~1 KiB
    players: [Player; 2],
    turn: Turn,
    stack: Fixed<StackEntry, 64>,                                    // ~2 KiB
    triggers: Fixed<PendingTrigger, 64>,                             // ~1 KiB
    delayed: Fixed<DelayedTrigger, 32>,
    effects: Effects,              // the stores of what cards create:
    memos: Memos,                  // continuous effects, what an object
    trackers: Trackers,            // remembers, this turn's facts, and the
    counters: CounterTable,        // counters no object keeps in a field
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

The engine is generic over the storage layout, `State<L: Layout>`, resolved at
compile time. The struct above is `Small`, the layout search and rollouts run
on. `Large` stores the same fields with `Vec` queues whose push never fails and
tables with an entry per slot; it is `Clone`, not `Copy`. A player game starts
in `Small`. When an engine call stalls it for a capacity, the session widens the
state as it stood before the call (`State::widen`, which keeps every slot, queue
order and knowledge bit) and makes the call again in `Large`, where the game
stays. Effect data is the same for every layout, and only its interpreter is
compiled for each; a Rust body, where a card has one, is a generic function
instantiated once per layout (`custom_body`). The slot and index types are the
layout's too (`Layout::Slot`, `Layout::Index`): `Large` has four times the
slots, 1,024 (`LargeSlot`, a `u16`, with 1,024-bit bitboards), and grows the
queues nested in other values (a stack entry's targets, a player's emblems, a
paused resolution's choice record, a decision's options). So `ObjectRef`,
`Decision`, `Opt`, `Pending` and `Event` take the layout as a parameter, and
`Relayout` moves any of them to the other layout by slot number; the session
keeps its own copies in `Large`. The mana solver (§10) grows too: in `Large` a
payment holds any number of classes of interchangeable sources, plans and spent
units.

### 4.2 Objects and slots

Every card, token and spell copy occupies one of 255 slots, addressed by a
`Slot`: a `u8` stored plus one, so `Option<Slot>` is one byte and "no object" is
`None`, never a reserved slot. Players are a `Seat` and game-table indices a
`CardIx` (stored plus one the same way), so one cannot pass for another. The
object in a slot:

| Field                         | Size | Notes                                                                                       |
| ----------------------------- | ---- | ------------------------------------------------------------------------------------------- |
| `card: CardIx`                | 1    | Index into the game's table (§6.4). Tokens and copies point at their definition too         |
| `incarnation: u16`            | 2    | Bumped on every zone change (§5)                                                            |
| `zone`, `owner`, `controller` | 3    | Small enums; `controller` is the base one, the layer-2 result is in `Derived`               |
| `flags`                       | 4    | tapped, summoning sick, attacking, blocking, face down, transformed, phased out, token, ... |
| `damage: u8`                  | 1    | Marked damage this turn                                                                     |
| `counters`                    | 3    | +1/+1, -1/-1 and loyalty; any other kind is a row of a table keyed by object and kind       |
| `attached_to`                 | 1    | `Option<Slot>`: equipment and auras. What exiled a card is a memo of the card               |
| `timestamp: u16`              | 2    | Battlefield entry order, for layers                                                         |
| `face`                        | 1    | Which face is up; a chosen color or type joins it with the first card that names one        |
| `combat: Option<Slot>`        | 1    | The planeswalker an attacker attacks, or the attacker a blocker blocks                      |

About 20 bytes, 24 as laid out, so the table is about 6 KiB.

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

- **Capacities** of `Small`, each a stall when exceeded (§4.1):

  | Quantity                      | Capacity |
  | ----------------------------- | -------- |
  | Players                       | 2        |
  | Object slots                  | 255      |
  | Cards per deck                | 60       |
  | Permanents on the battlefield | 128      |
  | Stack entries                 | 64       |
  | Pending triggers              | 64       |
  | Delayed triggers              | 32       |
  | Continuous effects            | 64       |
  | Locked-in sets of an effect   | 4        |
  | Memos                         | 32       |
  | Rows of counters              | 32       |
  | Targets per stack entry       | 8        |
  | Choice record per process     | 32       |
  | Source classes per payment    | 16       |
  | Payment plans offered         | 64       |

  The heaviest known users: Brain Freeze and Tendrils of Agony at storm 10 to 20
  for the stack, Sheoldred, the Apocalypse against a Wheel of Fortune for
  triggers, and Myr Battlesphere, Lingering Souls and Elspeth, Storm Slayer's
  doubling for tokens.

### 4.3 Zones

- **Membership** is a bitboard per zone per player: library, hand, battlefield,
  graveyard, exile, command, plus the shared stack. Each object also carries its
  zone, and a debug invariant keeps the two in agreement.
- **Order** is kept for `library` and `graveyard`, as `Fixed<u8, 60>` per
  player, and for the stack. Hand, battlefield and exile are unordered.
- **Linked information** is a memo of the card (`beryl_kernel`'s contract,
  "Stores"): a card in exile remembers the slot of the object that exiled,
  imprinted or hid it, so Skyclave Apparition, Chrome Mox, Memory Jar, Shelldock
  Isle and Agatha's Soul Cauldron need no fields of their own. Dauthi
  Voidwalker's void counters are ordinary counters on exiled objects.
- **Companions.** A companion starts outside the game, in the sideboard. Whether
  to support the mechanic is open (§20); if so, a per-player companion slot is
  the whole cost.

### 4.4 Players and turn

`Player` holds life, poison, energy, the mana pool, land drops this turn, the
emblem mask, and the per-turn history counters that rules read: spells cast this
turn by each player for storm, cards drawn this turn for Narset and Sheoldred,
life gained and lost, creatures that died, lands that entered. **Every history
fact a rule reads is a counter maintained at the mutation chokepoint.** None is
derived from a log, which is the trap DeepScry fell into when its rollouts ran
with the log off.

The mana pool is six `u8` color counts plus a short list of restricted mana
entries, each tagged with its restriction: the artifact-only mana of Mishra's
Workshop and The Mightstone and Weakstone.

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
  Drain, Sneak Attack, Memory Jar, Fable's Reflection).
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
  these become hand-maintained metadata on hundreds of cards, and a mismatch is
  a silent rules bug.
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
    pub body: Behavior,                  // effect data, or a Rust function (§6.3)
}
pub type Flow = Result<(), Stop>;    // Err: paused at a question, or out of room
```

- `Filter` is a small enum over bitboard-backed predicates: creature, controlled
  by, tapped, mana value at most, has counter, subtype, color, and combinations.
  Evaluating one is a few ANDs over `State`'s bitboards.
- `fn` pointers are fine in the table because the table is `&'static` data
  shared by all threads; they are banned from `State`, which stores only
  game-table indices and ability indices.
- A generated `names` module (`names::LIGHTNING_BOLT`) makes a misspelled card a
  compile error in both card files and scenarios. No list of names is committed:
  a build script writes `beryl_cards::names` from the decklists in
  `docs/sample-decks/` and the card files, and each set crate's writes the
  constants its own files use, each found in the card store.
- One file per card, `rust/cards/<crate>/<set>/<card>.rs`, by the card's first
  printing (`data/set-groups.json`; `just card path <name>` prints it). The file
  holds the `CardDef`, its bodies, and its black-box scenarios (§17). Nothing
  lists the files by hand: a new card is a new file (`beryl_card_build`).
- Nobody types a printed characteristic (plan K2). A set crate's build script
  writes a `printed::<FACE>` constant for each face of its cards, and a
  `printed::token::<TOKEN>` for each token its files name, from the card store;
  a face starts from it and adds its abilities, and a file that types a cost,
  type, subtype, color, power, toughness, loyalty or face name fails the build.
  Each file opens with a generated header of its card's printed faces, which
  `just card header` rewrites and the `names` lane holds to the store.

### 6.3 Bodies

What an ability does as it resolves is a `Behavior`: effect data, or a Rust
function where the vocabulary cannot say it (`docs/project-50k.md`, K1). Every
card of the pool is effect data, written with `beryl_card`'s `const fn`
builders:

```rust
// "Return target spell to its owner's hand. Draw a card."
Ability::spell()
    .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
    .effect(&sequence(&[return_to_hand(TARGET), draw(YOU, 1)]))
```

The interpreter (`beryl_rules`' `interpret`) carries the instructions out in
order. Each entry's arm calls a function of the rules library: free functions
over a `Game` (the state, the tables and the event sink) and, where what they do
makes sense only while something resolves, a `Resolution` (the source, the
controller, the targets and the record of answers), in modules named for the
Comprehensive Rules they implement (`actions::scry`, `zones`, `tokens`).
`beryl_rules`' `lib.rs` is the contract. What the engine, the AI and the UI need
to know of an ability (whether it asks, what it takes from a target) they read
from its data.

The escape hatch is a `CustomBody`: a unit type in the card's file whose
`resolve` is generic over the layout and takes the game and the resolution, made
the ability's body by `custom_body`. It calls the same library, and says of
itself what nothing can read from code (`CustomBody::SAYS`). No card has one.

A question asked while resolving goes through `choices`, which asks the answer
source (§3): the record answers it or, when the record has none, the resolution
stops with `Err(Stop::Pause)` through `?` and the question stays on the
`Resolution`; the rollout policy answers inline. The library's functions are the
only way an effect touches the state, and each builds the event that flows
through §8. The cost modifiers of Elite Spellbinder, Thalia and Leyline Binding
are reached through explicit engine hooks rather than the battlefield layer
pass.

### 6.4 The per-game table (D20)

Both decklists are public (§13), so the cards a game can contain are known when
it starts: the two decks plus every token they can create. `GameTable` is built
from them once per game:

- **Dense game-local indices**, which is what `State` stores, in the width its
  layout gives one (`Layout::Card`): a byte in `Small`, so at most 255 distinct
  cards there, and two in `Large`, 65,535. The table itself holds any number,
  and a game with more than a layout numbers is refused where its state is made
  (`Misfit::TooManyCards`). The hot static data for those cards sits
  contiguously, a few KiB that stay in L1: types, costs, timing class, keywords,
  listener masks and mana production, with each mana source's flexibility rank
  (§10).
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
or more of compilation per matchup. It is tried once the cube rollout benchmark
exists and adopted only if it clears 10%. Engine variants compiled per
combination of mechanics (about 1 to 2%) and a JIT, which would need the engine
expressed as an IR (the path **D1** rejected), were considered and dropped.

## 7. Decisions (D6) and transactional replay

### 7.1 The decision value

```rust
pub struct Decision { pub player: u8, pub kind: DecisionKind, pub candidates: Candidates, pub prompt: Prompt, pub about: Bitboard }
pub enum Candidates { Objects(Bitboard), Options(Fixed<Opt, 64>) }
```

`DecisionKind` is the closed list of factored single picks: a priority action
(pass, cast a card with a given face or alternative cost, activate an ability,
play a land, a special action such as ninjutsu), one target, one mode, X, a
payment plan, one attacker, an attack target, one blocker, the attacker it
blocks, a damage division, the next trigger to put on the stack, the next
replacement to apply, one card from a set, the next card in an ordering, yes or
no, a number, a color, a player, a pile assignment, keep or mulligan. Every
answer is `Choice(u8)`: an index into the candidates. The index is only for the
engine and its search. A save records an answer by what it does
(`beryl_session::save`, §14), and every `DecisionKind` and `Opt` has a
description there, checked by exhaustive matches.

The kind says the form of one answer, and no kind belongs to one mechanic: the
card a mulligan puts on the bottom, the cleanup discard, the legendary permanent
to keep, the creatures that crew and the cards an exile cost takes are each one
card from a set, the life an entering land asks is a yes or no, and the
permanents a waterbend taps are a payment. The `Prompt` (`beryl_card::prompt`)
says which prompt the pick belongs to: its purpose, from a closed list (a scry,
a look that may take a card, a fateseal, an explore, "untap up to six", "unless
pays", the cleanup discard); the shape that purpose is answered in; the shape's
parameters (counts, destinations, a mana cost); and the part of the shape this
pick answers. `about` is the set of objects the prompt concerns beyond the
pick's candidates. The record has no slot in it, so it is the same in every
layout, and a decision is no bigger for it. Card code states the prompt where it
asks, so the UI and agents dispatch on it and never on the name of the card
asking, and a card whose question fits a purpose needs no code outside its own
file.

### 7.2 What the engine settles itself

The exact profile offers a choice only across **distinct consequences**:

- A choice with one legal answer is applied silently, but for the target of a
  spell or ability a human casts or activates: a profile naming them
  (`asks_targets`) asks it, so they choose it on the board and may take back a
  cast begun at their priority.
- A player with no legal action passes priority silently. In this cube that is
  rarer than it sounds: an uncracked fetchland, Wasteland, a manland, Lightning
  Bolt or Brainstorm is a legal action at almost every priority pass, so under
  the exact profile most of the 16 to 20 empty-stack passes in a turn are
  decisions. Search profiles cut them with smart stops (§15).
- Interchangeable objects collapse to one candidate: two untapped basic Forests,
  two identical tokens with the same counters and nothing attached. The
  signature compared is every field of the object but its incarnation and
  timestamp (card, face, zone, owner, controller, counters, damage, flags,
  attachment, link, copy, chosen value), what the stores hold about it and which
  players know it: a copy in hand the opponent has seen is not one they haven't.
  An object that another record of the state names is never collapsed with a
  twin, since nothing about the object itself shows it: a target or the source
  of an ability on the stack, what a triggered ability will act on, a blocked
  attacker, the permanent an Aura is on, the permanent a card is exiled with.
  The kernel answers which they are in one query (`State::named`), beside the
  fields that name them.
- Triggers of the same ability from interchangeable sources, on the same event
  with interchangeable objects, are equivalent and ordered silently, where
  renaming one's source and object to the other's leaves the waiting triggers
  what they were; non-equivalent simultaneous triggers are ordered by their
  controller under APNAP.
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

Only an ability that may ask a question runs on the scratch copy, which costs
about 170 ns. An ability written as effect data says so itself: its effect
contains an instruction that asks (`Effect::asks`). A Rust body, which nothing
can read, declares it with `Ability::pauses`, and a debug assertion catches a
body that asks a question without declaring it. `Ability::asks` answers for
both. The alternative, a hand-written step enum per multi-step card, was
rejected (**D1**) as the error-prone half of the work for no measurable speed.

**Rollouts never pause.** Their answer source is the rollout policy, called
inline, so a `may_pause` body runs once on the committed state, with no scratch
copy and no re-run. This cube is full of such bodies: twelve fetch-style lands,
tutors, Thoughtseize, Brainstorm, Ponder, Preordain, Stock Up and Dig Through
Time. At 10 to 20 resolutions a game, replaying them would cost about 2 to 3% of
rollout time. Both paths must reach the same state for the same answers, so a
differential test records a rollout's inline answers, replays them through
`Replay`, and compares state hashes at every decision (§17.4). An undo log that
rolls back on pause was considered and rejected: exact undo for every primitive
is the bug class that broke DeepScry's rewind.

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

An event does not say what made it happen. A recording sink keeps a `Tag` beside
each event it keeps, whose `Cause` does:

- `Rules`: a turn-based or state-based action, or a step of starting the game.
- `Player`: a player's own action, a special action or any step of casting a
  spell, activating an ability or putting a triggered ability on the stack, with
  the spell or ability it casts or pays for. A cost's taps and sacrifices so
  name what they pay for.
- `Effect`: the effect of the spell or ability that is resolving, down to its
  leaving the stack.
- `Object`: the effect of an ability that never was on the stack, a mana ability
  or a static ability's replacement, with its source and the spell or ability
  whose payment the mana ability was activated in.

A cause names a spell or ability by its place on the stack (`StackIx`), since
`State` gives its stack's entries no identity of their own: a field there is
copied in every rollout. A reader follows the stack through two events only a
recording sink ever keeps: `StackArrival` puts an entry on top, and
`StackDeparture` takes the one at its place out, the last event of a resolution
for the spell or ability resolving, which keeps its place until then though the
state has taken it off its stack. The tag is no part of the event, so trigger
matching never reads one.

The tag also says which events are one event of the rules, a group: the
permanents one instruction destroys whoever owns them (CR 608.2f), the actions
of one state-based check (CR 704.3), all the combat damage of a step (CR 510.2),
the permanents of the untap step (CR 502.3), the attackers' taps with their
declaration (CR 508.1f), the taps and mana of one payment (CR 601.2h), and a
permanent leaving with what it had exiled until then returning (CR 610.3). Cards
drawn are not one (CR 121.2). The rules open a group around what they do at
once, and one opened inside another is part of it. A tag has no group number,
only `together`: the event is of the group of the event kept before it. A body
that runs again from its start (§7.3) so keeps the same tags as its run before,
which the session relies on to tell the repeats, and `Tag::groups` numbers a
list's groups for a reader. A block declared (CR 509.1) is an event only a
recording sink keeps, `Block`, one for each blocker, together.

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
   appended for the UI or a replay, with its tag. Search attaches none.

The tag is written from a register in the sink, not at the sites that emit
events. The rules open a scope around what they do (a player's action, a
resolution, a mana ability's effect) and close it on the way out; scopes nest,
as a mana ability activated to pay a cost an effect asks for sits inside that
payment, inside the resolution; and with none open the cause is the rules.
Search pays nothing per event for this and one branch to open or close a scope
or a group: a sink that is off never builds a cause, and the code that records
sits out of line. A body run that is discarded (§7.3) drops its tags with its
events.

### 8.3 Triggers

Pending triggers go on the stack the next time a player would receive priority,
in APNAP order, each controller ordering their own non-equivalent ones.
Intervening-if conditions are checked when the ability triggers and again on
resolution. Reflexive triggers ("when you do") are enqueued by the body that
created the condition. Delayed triggers carry their duration and fire from
step-boundary events. State triggers (Dark Depths) are checked with state-based
actions and enqueued once until they resolve. Triggers from outside the
battlefield are indexed too: Emrakul's cast trigger from the stack.

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
    pub types: Types, pub supertypes: Supertypes, pub colors: Colors,
    pub controller: Seat,                // the layer-2 controller, which State::controller reads
    pub protection: Colors,
    pub keywords: Keywords,              // flying, first strike, deathtouch, ...
    pub subtypes: Subtypes,              // the cube's creature, land, artifact and enchantment types
    pub granted: [Option<GrantRef>; 4],  // abilities granted by other objects
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
- Copy effects (layer 1) use copiable values: a copy is a token whose `card` is
  the definition copied. A copy of a face-down permanent is a face-up token of
  `FACE_DOWN_COPY`, a nameless 2/2 every game table holds (CR 707.2, 708.2). The
  few exceptions (Metamorph is an artifact in addition, Phantasmal Image is an
  Illusion with its sacrifice trigger) are flags the recompute honors.

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
  - **Wherever search or a player pays**, at tree nodes, in rollouts and in the
    exact profile alike, the plan that leaves the largest set of still-payable
    costs. That is a search over distinct plans. On a mid-game cube mana base,
    say five lands, a Mox and Sol Ring, a three-mana spell has 10 to 20, and
    testing the other costs against each is an estimated 0.5 to 1.5 µs per cast,
    growing combinatorially with Moxen, Signets, Talismans and dual lands.
  - **Under `Payment::Greedy`**, greedily. Each mana source carries a
    flexibility rank in the per-game table (§6.4): fewest colors first, then no
    other ability, then no restriction. Payment taps the least flexible sources
    that cover the colors, in tens of nanoseconds. It always pays legally and
    occasionally taps a source wanted later in the turn. No profile selects it,
    rollouts included: only tests and the reference check run it.
- **The human's payment** (`Profile::taps_mana`) offers the same plans, but
  picks each one's taps by looking ahead over the rest of their turn: the most
  mana of later costs payable together (two copies of a card count twice), only
  instant-speed costs once no main phase is left before their untap step, the
  land they may still play, and creatures tapped last. Search clears the seat
  below the root, so rollouts never pay for it.
- A decision is offered when plans differ in consumed resources: permanents
  sacrificed, life paid, cards exiled for delve, counters removed, restricted
  mana spent, floating mana left over.
- Standalone mana-ability activations are offered only when timing-restricted
  (Lion's Eye Diamond in response to one's own spell) or when the ability has
  effects beyond mana; otherwise pre-tapping is dominated by paying during the
  cast.
- The profile's `taps_mana` seat, the human, is offered every untapped source's
  mana at priority, one candidate per kind of mana for each set of
  interchangeable objects, so a click taps the land clicked and floats its mana,
  as on Arena. These candidates come last in the list and stop at its capacity,
  and a priority offering nothing else still passes silently. Search clears the
  seat below the root. A plan that taps a source only to leave mana floating
  that source could make again is dominated, so floating mana is spent first.
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
  triggers (Adeline, Ragavan), combat-damage triggers (Coveted Jewel, Etali's
  poison) and planeswalker damage fall out of the event pipeline.

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
report stall rates. A player game that runs out of a capacity of `Small` goes on
in `Large` instead (§4.1). A player game also lifts the decision and turn
budgets to their counters' maximum (`Limits::PLAYER`), since a human can always
concede; search below it plays under the defaults again, or that much room past
where the game stands once it has gone past one (`Limits::search`).

The cube has long loops: Underworld Breach recasts Lion's Eye Diamond and Brain
Freeze from the graveyard until a library is empty, one cast at a time. Under
per-decision budgets a search undervalues such lines. The fix is a later profile
meta-action that proposes a loop and a count as one decision in the spirit of
the rules on shortcuts, executed as that many real activations; it is noted in
§20 and not part of Milestone 1.

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
  bits for a whole hand; Jace, the Mind Sculptor's +2 sets its controller's for
  a library's top card; Gitaxian Probe and Elite Spellbinder look at a hand,
  Collective Brutality reveals one, and all three set bits the same way.
- **Shuffling rebinds identities among the shuffled slots** and clears their
  knowledge bits, in addition to reordering. Without the rebinding, the owner of
  a Brainstormed-back Ponder would know where it went.
- **A card's identity is settled as a player first learns it.** Every chokepoint
  above calls `chance::settle` first, while no player knows the card yet. Live
  play keeps what the deal and the shuffles put there and records it; a replay
  swaps in the recorded card from another slot of the same owner and zone that
  no player knows, the move a shuffle's rebinding makes (§14).
- Face-down exile (Memory Jar, Shelldock Isle) is a known-to-owner slot in
  exile; cards exiled face up are known to both. A face-down permanent (a
  manifested card) is a known-to-controller slot on the battlefield whose face
  is `FaceIx::DOWN`, a 2/2 with no text; it is revealed as it leaves.
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
- **A save records what chance produced, not the seed** (save format 3). The
  session lends the engine a `chance::Chance` through the sink of the calls that
  play its game on; search never does, and pays one branch per chokepoint. It
  records each card a player first learns (§13), by its owner and name, and each
  coin flip's winner, after the log entry they followed, and the play-first
  flip's winner in the file. A replay hands chance the outcomes logged after
  each entry as that entry plays out: each first-seen card is dealt the recorded
  card, each flip falls as recorded, and the seed decides only what nobody has
  seen. A shuffled library's order never matters until one of its cards is
  revealed, and a random bottom order settles the same way. An outcome chance
  cannot reproduce, one the game makes that the record lacks, or one the record
  has that never happens stops the replay at its turn and step. Slot numbers are
  random too (the deal and each shuffle choose them), so no tie the engine or
  session breaks for a player may depend on them: a save must replay the same
  under another seed.
- No `std::collections::HashMap` with `RandomState`, no wall clock, no statics
  with interior mutability, no thread-locals. A test greps the engine crates for
  the banned types, as Arcana does.
- A replay is `(decklists, seed, choices, chance)`. Replaying one on the same
  build reproduces the game bit for bit, and a scenario can be cut from a
  replay. Since save format 2, each choice is recorded by what it does (who
  decided, the turn and step, the kind of decision, and the candidate by rules
  terms: cast this card, attack with that creature, target that player, in the
  position the question is asked in), with its old index kept only to break
  ties. Since format 5 an object also names its owner where another player
  controls it, and whether the other player knows a card in a hand or library; a
  version 4 answer's objects are marked as named before that, and match either
  way. Since format 6 a permanent also names whether it is a token or summoning
  sick, its counters, damage and place in combat, a trigger which of its
  source's triggered abilities it is, and an answer among alike permanents its
  place in battlefield (or graveyard) order, which a later answer may depend on;
  version 5 answers match either way. A later build replays it though candidates
  moved, and goes on where record and game part only if the rules leave nothing
  to choose: one candidate, a priority passed unasked, or a pass no priority
  asks. Anything else stops the replay at the turn and step.
- **A save records the board as each turn begins** (save format 4,
  `beryl_session::checkpoint`), taken as the upkeep begins: each zone's cards by
  name (the library as a count and the cards someone knows in it), life, tapped,
  summoning sickness, counters, damage, attachments, exile links, faces and who
  knows each hidden card. A replay compares each turn's board with the record's;
  a difference is divergence, reported with the turn and the first difference.
  Interchangeable things are not differences: like permanents in another order,
  a graveyard's order and a noncreature's summoning sickness, which slot-order
  ties can move. Where a replay stops (an answer no candidate does, a reveal it
  cannot deal, a board that differs), the game is rebuilt from the latest board
  the replay reached at or before that turn: `State::begin_before_turn` puts the
  board as it stood once the turn before ended, and the engine begins the turn
  again. The rebuilt save records that board (`from`) and logs from there. What
  lasts past a turn's beginning off the board (an effect, a delayed trigger, a
  permission to cast) is not recorded: `State::off_board` names it in the
  checkpoint, and that turn refuses to rebuild rather than rebuild wrong.
- **Old saves are migrated** (`beryl_session::migrate`). A save names its format
  version, and loading runs it up a chain of migrations, one per version, each
  on the file's JSON, to the format the build writes. Versions 1 and 2 lack what
  later formats record, so their migrations replay the file on the build and
  record it again; they fail where it no longer replays. A change that breaks a
  save adds a migration or, for a save of the frozen corpus that no migration
  can carry, an entry in `data/save-breaks.json` naming where it stops matching
  and why, in the same commit.
- **A frozen corpus of saves stays playable** (`beryl_session::corpus`,
  `beryl_session/tests/saves/`): gzipped saves of every age, only ever added,
  which `tests/corpus.rs` holds to three levels without rebuilding
  (`Game::replay_exactly`): each replays to its end, matches every board it
  recorded, and ends with the same winner on the same turn (`endings.json`). A
  save on the breaks list is held to its listed entry. `just freeze-saves`, run
  weekly or so, plays seeded games of the cheap agents and freezes a small set
  covering every implemented cube card played, cast or activated and every kind
  of decision, printing what it reached; `--replay FILE` freezes one game.
- **Perturbations prove a save depends on none of the engine's internals**
  (`beryl_session::perturb`, test-only hooks on one thread that reach only
  replay and game setup): the seed derived another way, each decision's
  candidates reversed or shuffled before matching, and each deck dealt into its
  slots last first (`State::reverse_deck_slots`). The corpus replays at all
  three levels under each, from the format that made it so (3 for the seed, 6
  for the rest: older answers name too little to tell some objects apart). Each
  hook breaks a save of indices (version 1), so none passes by doing nothing.
  Batches of objects move in battlefield, then library, then name order, not
  slot order, so cards put into a graveyard together lie in an order no seed
  decides.
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
  about 8. Lines that need another stop, such as a Brainstorm in the upkeep
  before a draw, are lost unless the stop set adds them.
- The default damage division, all triggers ordered by rule, X capped, and
  attacks on the player only.
- In rollouts only, mid-resolution choices answered inline by the rollout policy
  (§7.3).

Every profile is sound in the sense that any line it produces is a legal line of
the real game; it may miss options, which is a search-quality question, not a
correctness one. A profile that restricts the opponent's options makes the
searcher optimistic, which is tuned, not fixed.

## 16. Crate map (D12)

| Crate                  | Holds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Depends on                                                            |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| `beryl_card`           | The card vocabulary: `CardDef` and the definition types, filters, costs, and effect data (`Effect`, `Objects`, `Players`, `Amount`, `Condition`) with its `const fn` builders                                                                                                                                                                                                                                                                                                                    | nothing in the workspace                                              |
| `beryl_kernel`         | `State` and its layouts, slots, bitboards and fixed queues, the zone-move and chance chokepoints, invariants, decisions, events and their sink, `GameTable`, the Observation view and determinization; its `lib.rs` is the contract for how other crates read and write the state                                                                                                                                                                                                                | `beryl_card`                                                          |
| `beryl_rules`          | Events' pipeline, layers, mana, combat, turn structure, casting, the rules library (free functions over `Game` and `Resolution`, in modules named for the Comprehensive Rules), the interpreter of effect data and the `CustomBody` escape hatch, with their tests (`cargo test -p beryl_rules`); it re-exports the card vocabulary (`card`) and the kernel's layouts, profiles and events (`layout`, `profile`, `event`, the last two beside items of its own), and no other part of the kernel | `beryl_card`, `beryl_kernel`                                          |
| `beryl_engine`         | The facade other crates import until the last caller has moved: one `lib.rs` that re-exports `beryl_rules` and the kernel's modules at the paths they had as one crate, and defines nothing                                                                                                                                                                                                                                                                                                      | `beryl_kernel`, `beryl_rules`                                         |
| `cards/<set group>`    | One file per card, `<set>/<card>.rs`, by first printing: a crate per group of `data/set-groups.json` that has a card (`cards_1993_1994`, ...). Its build script writes its module tree, `CARDS`, `SCENARIOS` and the `names` its files use                                                                                                                                                                                                                                                       | `beryl_card`, `beryl_card_shared`, `beryl_scenario_types`             |
| `beryl_card_shared`    | What card files share: tokens several cards create, the scenarios' prop cards                                                                                                                                                                                                                                                                                                                                                                                                                    | `beryl_card`                                                          |
| `beryl_card_build`     | What those build scripts run: lists card files and set crates, finds each file's card in the card store, writes the generated sources and printed faces                                                                                                                                                                                                                                                                                                                                          | `beryl_card_store`                                                    |
| `beryl_cards`          | The generated registry over every set crate (`CARDS`, `SCENARIOS`, crate by crate, then set, then file), the generated `names` module, the decklists, the rules library's scenarios, `beryl_scenario dump`                                                                                                                                                                                                                                                                                       | the set crates, `beryl_card_shared`, `beryl_engine`, `beryl_scenario` |
| `beryl_scenario`       | The strict runner, the bounded exhaustive search, the JSON serialization; re-exports the value types                                                                                                                                                                                                                                                                                                                                                                                             |                                                                       |
| `beryl_scenario_types` | Scenario value types, naming `beryl_card` alone, so a card crate with its scenarios rebuilds only with the vocabulary                                                                                                                                                                                                                                                                                                                                                                            |                                                                       |
| `beryl_search`         | The `Agent` trait, `RandomAgent`, the greedy agent that is also the rollout policy, the expert system, MCTS, evaluation                                                                                                                                                                                                                                                                                                                                                                          | `beryl_cards`, `beryl_engine`                                         |
| `beryl_card_store`     | The card store embedded from `data/card-store.tsv`: a record for every card in the pool and every token, looked up by name without a scan                                                                                                                                                                                                                                                                                                                                                        | nothing in the workspace                                              |
| `beryl_session`        | `Sessions` and their threads, the `PlayerView` protocol and IPC names with generated TypeScript types, replay files                                                                                                                                                                                                                                                                                                                                                                              | `beryl_card_store`, `beryl_cards`, `beryl_engine`, `beryl_search`     |
| `beryl_cli`            | Headless games between agents, tournaments, the ladder, transcripts, UI fixtures recorded from sessions of seeded games or of scripted boards                                                                                                                                                                                                                                                                                                                                                    | `beryl_cards`, `beryl_scenario`, `beryl_search`, `beryl_session`      |
| `beryl_bench`          | Benchmark support and the `_counts`, `_abba` and `_timing` targets of the example and of the cube rollout benchmark                                                                                                                                                                                                                                                                                                                                                                              | `beryl_cards`, `beryl_engine`, `beryl_search`                         |
| `beryl_app`            | The Tauri shell: IPC commands and events around `beryl_session`, the card-art cache                                                                                                                                                                                                                                                                                                                                                                                                              | all of the above                                                      |

The engine knows no card and no card knows the engine's internals: a card is
effect data in `beryl_card`'s vocabulary, and `beryl_cards` takes from the
engine only the table types, the decklists and, for scenarios, what a scenario
names. A set crate depends on the vocabulary alone, so a rules edit rebuilds no
card and one card edit recompiles its set crate and `beryl_cards`, not the
engine's tests. Nothing may read meaning into the order of `CARDS`: a game
numbers its cards from its decklists, and everything else looks a card up by
name. Exporters that turn dumped scenarios into XMage and Forge inputs are
Python in `tools/`, per AGENTS.md.

## 17. Testing and verification (D13)

### 17.1 Scenarios

Each card file declares `SCENARIOS: &[Scenario]` in an inline `scenarios` module
behind its crate's `scenarios` feature, which `beryl_cards`' feature of the same
name passes on, and which tests and tools turn on and the app never does. They
are built with a typed builder and serializable with serde. A scenario names its
own card, that card's tokens, basic lands and the designated props
(`beryl_cards::props`); one about two particular cards says so with
`Scenario::interaction`, so one card's change cannot break another card's tests.

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

`beryl_scenario dump` writes every scenario as JSON, with a trace of where each
action began as Beryl played it: the turn, step, open decision and stack, the
coin flips and taps on the way, and the answers to questions with one answer,
which `Stops::Forced` stops at so the trace sees them. Oracles script by turn
and step rather than by decision, so the exporters place each action from the
trace. Python exporters in `tools/` generate:

- **XMage**, the primary oracle. A JUnit class per card in the XMage checkout's
  `Mage.Tests`, on `CardTestPlayerBase` through Beryl's `BerylScenarioTest`
  helpers: `addCard`, `castSpell`, `attack`, `addTarget`, `setChoice`,
  `setStrictChooseMode(true)`, the `assert*` family. Where XMage asks a card's
  questions differently from Beryl, rules in `card_facts.py` translate them,
  reading the dump's description of each card a scenario names (its abilities'
  costs, mana, casting options and effects), never its name; a new card's
  scenario that fails on XMage usually needs a rule there for what it does.
- **Forge**, the secondary oracle. Each scenario becomes a
  `forge.game.GameState` text setup (`p0life`, `p0hand`, `p0battlefield` with
  `|Tapped` and `|Counters:LOYALTY=3`) and a list of steps that Beryl's own
  scripted harness, `tools/oracles/BerylForgeHarness.java`, plays. It picks an
  ability or casting option from the same card descriptions (`card_facts.py`): a
  loyalty change, a keyword, or the first words of the ability's effect in its
  Oracle text. A scenario outside that vocabulary is reported as unsupported
  with the reason.

`just oracle-setup` installs Maven with Homebrew if it is missing,
shallow-clones XMage and Forge at pinned commits into
`~/Library/Caches/beryl/oracles` and builds them once with JDK 21; agents run it
unattended. `just oracle [cards]` exports the scenarios and runs them on both,
in under a minute once built, and writes its full report to
`oracles/logs/oracle.json` there. The nightly runs that full sweep on staging;
`just ci`'s `oracle` lane plays the cards changed since master and a sample
fixed by the commit, or every card when the exporters or harnesses changed
(`oracle_selection.py`).

- The dump ends each scenario that ends at a quiet position (priority over an
  empty stack) with its snapshot: assertions of the whole public position, life,
  zone sizes, the public cards of each zone, each permanent's control, power and
  toughness and counters, so an oracle compares more than the author asserted.
  Which mana sources paid and which cards a shuffle dealt are left out: the
  oracles choose and shuffle their own way.
- The dump adds 100 generated scenarios (`beryl_cards::generate`, filed under
  "Generated"): a seeded turn 3 main phase of random lands, permanents and cards
  in hand from the cube, where player 0 takes one to three actions, either
  player may respond, and some go to combat, each choice random. A `Live` runner
  plays each action as it is chosen, so the script replays as written. The
  generator avoids what the oracles' harnesses would read another way, rather
  than rules: cards that trigger before the main phase, two cards of one name an
  action could mean, and tap abilities of mana sources an oracle may have tapped
  to pay.
- A generated scenario an oracle fails for a gap in its exporter, not a rules
  difference, is listed with the gap in `oracles.py`'s `EXPORTER_GAPS` and
  reported as unsupported; an entry whose scenario no longer fails fails the
  run.
- A card an oracle does not implement, or a scenario outside its harness's
  vocabulary, is reported as unsupported, not failed.
- A disagreement is a bug to investigate, and the Comprehensive Rules decide it.
  When the oracle is wrong, the scenario joins a short allowlist of known
  divergences, each with its CR citation (`data/oracle-divergences.json`). Only
  a different game outcome belongs there: a question an oracle asks that Beryl
  does not is the exporter's to answer.

### 17.3 The bar for a supported card

A card is supported when every one of its abilities is exercised by at least one
scenario, its scenarios pass on Beryl and, where exportable, on XMage and Forge
or appear on the divergence allowlist, and fuzz play including the card raises
no invariant. dirac's lockstep bar is the model.

`beryl_engine`'s `coverage` feature, which only tests enable, records each
ability whose body runs, mana ability that makes mana, static ability whose
effect applies or rule bites, replacement that modifies an event, and casting
option a spell is cast with. A `beryl_cards` test fails on any no scenario
exercises, and `just fuzz` reports those random play never reaches.

### 17.4 Other checks

- **Invariants:** debug builds of `next` and `apply` check every state they
  return or are handed, so every test that plays the engine fuzzes it: zone
  bitboards agree with object zones, every stack reference names a live
  incarnation, no object is in two zones, `derived` is clean when read, counters
  and life are in range. Search plays its simulated games on a
  `Scratch::unchecked`, which skips them.
- **Fuzzing:** random-agent games in debug builds, checked as above.
- **Shrinking:** a fuzz game is also a seed and a vector of choice indices that
  proptest generates (`beryl_search/tests/shrink.rs`). A failing game shrinks to
  the fewest choices that still fail and prints as a scenario, its setup at the
  first priority, to paste into a card's file.
- **Exhaustive search:** from each card scenario's end position, every sequence
  of decisions to a fixed depth, each branch on its own copy of the state, with
  the invariants and references at every position and no panic allowed
  (`beryl_scenario::exhaust`, `beryl_cards/tests/exhaust.rs`). Chance is not
  enumerated, positions seen with as much depth left are skipped by state hash,
  and a cap on decisions per scenario names the scenarios it cuts short. The
  fast check searches to depth 3; `just exhaust [card] --depth N` goes deeper. A
  failure prints as the scenario with the failing sequence after its actions.
- **References:** slow reference implementations of the fast paths, checked at
  every fuzz decision and scenario position, and at every 32nd checked step of
  any debug game: the layer results, listeners and knowledge against a recompute
  from scratch, and the prefilter, funds and solver against payability matched
  symbol by symbol from mana abilities read off the cards.
- **Slot numberings:** every card's scenarios run under three numberings of the
  decks' slots and must trace the same, as slot numbers say nothing (§13).
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
baseline in Milestone 1, following `docs/benchmarking.md`: deterministic inputs
pinned by a test, instruction counts first, ABBA timing to confirm on a quiet
machine, single-threaded on a Super core. Milestone 1 gates only what is
deterministic: the size assertion, zero hot-path allocations, the stall rate and
the perf-counter bands. The time targets are reported from counts, and timing
waits for a human on a quiet machine, which an unattended run never has.

**The cube rollout benchmark** is the workload the targets are measured on: a
pinned corpus of mid-game positions between real cube decks, cut from
expert-system and greedy games, rolled out under the rollout policy and the
search profile to the end of the game, Milestone 1's horizon (§19.1). In
Milestone 1 the decks are the WU Control mirror. It reports decisions per turn
and per rollout, nanoseconds per decision for the engine and for the policy, and
rollouts per core-second.

**Builds.** A `search` Cargo profile builds the benchmark and headless research
binaries: fat LTO, `panic = "abort"` and `codegen-units = 1`, with
`-C target-cpu=apple-m4` passed by the recipes that build it, since a Cargo
profile cannot set the CPU; `apple-m4` is what `native` resolves to here under
rustc 1.98. Profile-guided optimization, trained on the cube rollout benchmark,
is not used yet. It needs `llvm-tools` in `rust/rust-toolchain.toml`, and BOLT,
the usual next step on Linux, does not support macOS binaries.

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

Milestone 1 ships five agents behind one trait, named in `beryl_search::AGENTS`:
`random`, `greedy` (the rollout policy), `expert` (**D15**), and a basic MCTS as
`mcts-omniscient` and `mcts-hidden`. A sixth, `expert-frozen`, is a fixed copy
of the expert, the reference opponent of `docs/ai-strategy.md` §3. The engine
already provides what search needs: cheap copies, factored decisions inside the
state, the `Observation` view and determinization, profiles, budgets, the state
hash, per-thread `Scratch`, inline answers, and the inlined rollout loop.

```rust
pub trait Agent {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice;
}
```

**The expert system** is the scripted baseline: a prioritized rule set with
card-specific knowledge (when to hold a counterspell, when to cast a wheel,
which land to Strip Mine), using engine copies only for combat math, lethal
checks and comparing payment plans. At the tournament budget of 300 iterations
and 4 trees it beats both MCTS agents about 88% of the time: the search is
starved, at 75 full-game rollouts a tree. It ships beside a `RandomAgent` used
for fuzzing, and its games and the greedy agent's supply the positions of the
cube rollout benchmark (§18).

**The rollout policy** (**D21**) is separate from the expert system, and
cheaper. Wrapped as an `Agent`, it is also the greedy opponent:

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
a node, rollouts under the rollout policy, common random numbers across
candidates, and root parallelization across determinizations on all 18 cores
with no shared mutable state. Then a learned value function at the leaves, and
EPIMC-style deferred evaluation for bluff-sensitive spots. ISMCTS is an
experiment, not the default.

**Milestone 1's MCTS** is the basic form of that search:

- UCT over the engine's factored decisions, with full-game rollouts under the
  rollout policy. A stall scores 0.5.
- Nodes store choices only, and each iteration copies the root and replays its
  path.
- Budgets are iteration counts, and each search is seeded from the agent's seed
  and the decision's index, so a seed reproduces a move and an agent rebuilt
  midway through a game plays on as the one it replaces.
- Independent trees run root-parallel on threads, and their root visit counts
  are summed in a fixed order. They grow in rounds and stop once no spending of
  the iterations left could change the candidate visited most.
- Priority with the player's own spell or ability on top of the stack passes
  unsearched.
- A decision asked straight after the one before, with no card but the chosen
  one changing zones and nothing hidden learned, as the steps of casting a spell
  follow one another, keeps the trees and searches on from the chosen child.
- The hidden-information variant samples K determinizations from the searcher's
  Observation, one tree each. The omniscient variant runs the same search on the
  true state and is labeled as cheating wherever a human meets it.

**Node storage** beyond replay from the root is a later decision. At about
10,000 new nodes per second per thread, a 20 KiB state in every node would be
3.6 GB/s across 18 threads, or 36 GB for a 10-second search, and restoring one
from DRAM takes about 630 ns. Search can replay choices from the root, or keep
states only in nodes visited at least twice; a state is a pure function of the
root and the choices (§2), so either is exact.

### 19.2 Sessions and IPC (D14)

`beryl_session` owns `Sessions`, which numbers games, starts each on a dedicated
thread running the step/apply loop and refuses commands for a replaced one;
`beryl_app` only wraps it in Tauri commands and events, and the CLI's fixture
recorder drives the same API, starting a board fixture from a prepared position
(`Sessions::start_prepared`) whose two seats play a script. The session emits a
`PlayerView` as the game starts, as each step begins (`next_in_step`, which
pauses `next` at a new step), before each agent decision, at each of the human's
and at the end: a reduced, owned view built from the human's Observation (§13),
holding only what the UI draws, rather than the 17 KiB masked state, and naming
players relative to the viewer. Its status is the open Decision, with its
candidates described by public slot, the opponent thinking, a step playing on, a
stop, or the outcome; the engine events since the last view, for animation, also
pass through the Observation, so each event is in exactly one view. Under the
smart stops the session also halts the human at the start of their precombat
main phase when the engine would pass their priority for them: a priority
decision whose only candidate is to pass, which neither the engine's profile nor
the agent sees, and which the replay records. The opponent's turn plays on to
the human's unhalted. The session also answers four kinds of the human's
decisions for them under the smart stops, and the replay records each answer: it
passes their priority while their own spell or ability is on top and only their
own entries came onto the stack or left it since their last answer in the step
(or its start), and in combat with an empty stack after their own attack or a
declaration of no blockers, as Arena stops a player only after the opponent's
attack and after blocks; it orders their simultaneous triggers so that plain
effects resolve first, then triggers with targets, then those that ask a
question as they resolve; it searches their library where they may (Trinket
Mage's "you may search") when they know no card in it, as declining then saves
only a shuffle and the search may still find nothing; and it pays a waterbend
cost with mana whenever mana can pay it. The exact stops ask all four. At the
human's decision the session waits for one `choose(decision, index)` command,
naming the decision id it received; the session's record of the open decision is
the one place an answer is checked. AI decisions run on the session thread, and
a search may use a pool, so IPC never blocks. The webview never sees a `State`.
Every session records seed, decklists, the choice log, what chance produced and
each turn's board as a replay file (§14), rewritten at each decision of the
human's, and the app resumes the newest unfinished one on launch, from the
latest turn's beginning where this build can no longer replay it to its end. The
protocol's TypeScript types are generated from the Rust types, and
`ui/src/ipc.ts` keeps the typed wrappers for the commands and events.

The human plays under the smart stops of §15 by default. A setting switches to
the exact profile, which offers every priority. Milestone 1 offers a game
against any one agent; the human and the AI both play the WU Control mirror.

As a practice tool, the human may take back their answers one at a time, from
any decision of theirs, across the agent's decisions and turns. The session
keeps a copy of the state at each of their last 500 decisions (`UNDO_POINTS`, 20
KiB each, more than a whole game's answers), random streams included, so going
back takes a play back without dealing the luck again: shuffles fall as they
did, and the cards drawn since come out again in the same order, which the human
now knows. An agent seeded by decision index goes back with it (`Agent::rewind`)
and decides again as play goes on. The choice log keeps everything taken back
and an `Undo` entry, so a replay goes back too. The engine is untouched.

## 20. Open questions

- **Companions.** Supporting the mechanic costs a companion slot per player and
  a special action; whether to is decided once the Legacy decks are chosen.
- **Hidden decklists.** A Legacy opponent's 75 is not shown, but the metagame
  narrows it to a few lists. Whether the AI keeps D19's public decklists or
  deals each world's list from the metagame is open.
- **Sideboarding.** Best of three needs an agent that swaps cards between games,
  and a session that carries a match across them.
- **Loop shortcuts.** The "repeat N times" meta-action of §12.3, and how search
  should propose counts.
- **613.8 corpus.** Which interactions in the supported decks, if any, exercise
  same-layer dependencies; the scenario suite should contain the candidates.
- **Rollout horizon and node storage beyond Milestone 1** (§19.1): whether a few
  turns and an evaluation beat full games, and whether states in nodes visited
  twice beat replay from the root.
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
| D7  | Mana solver picks taps (keeping the most costs payable, in rollouts too; the human's payments look ahead), offers resource trade-offs, `manual_mana` flag  | Payability needs a solver anyway; sacrifices and life are real decisions; tap choice rarely is; searching tap plans costs about 1 µs a cast                                          |
| D8  | Sequential combat picks with legality-aware candidates; division by distinct outcomes                                                                      | Whole-declaration decisions are combinatorial; defenders and divisions are real decisions                                                                                            |
| D9  | Engine-tracked knowledge over public slots; shuffles rebind identities; Observation as a borrowed view                                                     | The chokepoints are in the engine; every engine that tracked knowledge elsewhere leaked; a masked copy per decision would cost over 10% of the budget                                |
| D10 | Typed `Event` enum for about 30 kinds, per-kind indexes, set-carrying events                                                                               | One replacement and trigger protocol; everything-is-an-event is slow; per-primitive hooks drift                                                                                      |
| D11 | Distinct `Stalled(reason)` terminal scored as a draw                                                                                                       | Budgets are unavoidable; a stall must be distinguishable from a rules draw in tests and statistics                                                                                   |
| D12 | Crates: engine, cards, scenario, search, session, cli, bench, app                                                                                          | The engine knows no card; card edits do not rebuild rules tests; the AI cannot reach engine internals                                                                                |
| D13 | Scenarios as Rust-built serde values, strict runner, exported to XMage and Forge                                                                           | Typo-safe authoring with the compiler's help; one scenario, three engines                                                                                                            |
| D14 | Rust `GameSession` on its own thread; Observations over IPC; replay files                                                                                  | AI thinking never blocks the window; hidden identities never reach the webview                                                                                                       |
| D15 | A scripted expert system behind an `Agent` trait over Observations                                                                                         | Plays sanely in Milestone 1 and supplies the benchmark's positions; no API change for MCTS                                                                                           |
| D16 | Per-core budgets measured on cube rollouts as MCTS runs them; metrics first-class                                                                          | The per-decision budget drives the layout; random games undercount the decisions search pays for; machine-wide numbers reward degenerate games                                       |
| D17 | This document at `docs/engine-architecture.md`, engine plus boundaries                                                                                     | Reference material every session reads; the interfaces the UI and AI consume are written down                                                                                        |
| D18 | Search profiles give priority only at smart stops                                                                                                          | A held fetchland or instant makes most of a turn's 16 to 20 priority passes into decisions; smart stops cut decisions 2.5 to 3 times, which no codegen recovers                      |
| D19 | Decklists are public; hidden information is where the known cards are                                                                                      | The AI is built for open decklists; determinization deals each list's unknown remainder, so no belief model is needed                                                                |
| D20 | A dense per-game table over the game's known cards; per-matchup builds only on a measured gain of 10% or more                                              | Decision cost is generic machinery whose work depends on the board; the table gets most of specialization's gain                                                                     |
| D21 | A table-driven rollout policy with no copies or lookahead, about 300 ns per decision                                                                       | Policy cost multiplies every rollout decision; lookahead costs seven times the engine; weaker rollout players gave better estimates in Cowling et al.                                |
| D22 | A `search` build with fat LTO, abort and the M4 CPU target, then PGO trained on the rollout benchmark                                                      | An estimated 5 to 20% for a build profile; the app follows only once the gain is measured                                                                                            |
