# MTG Rules Engines: State of the Art for Tree Search

*Research report for Project Beryl. Compiled 2026-11-04.*

Beryl's plan is to make hyper-optimized tree search the main way it plays. This
report looks at what MTG rules engines exist today, how fast they are, and what
their designs mean for a Rust engine built for search.

______________________________________________________________________

## 1. Executive summary

- **No open-source engine is both fast and rules-complete.** The engines with
  near-complete rules cost milliseconds per game action: Forge, XMage, phase.rs
  and Ironsmith. The search-oriented engines get speed by limiting scope: dirac
  (two players, a verified card subset), Arcana (a slow but search-shaped
  design) and DeepScry (closed source).

- **The closest public design to Beryl's is gorge/dirac, not DeepScry.** It is a
  Go project with two engines:

  - **gorge** is a broad reference engine.
  - **dirac** is a search engine whose whole game state is one flat value of
    about 24 KiB with fixed capacities.
  - dirac copies a position in **40 ns** and makes a transition in **2.7 µs**,
    both with **zero allocations**. It plays **about 1,800 random games per
    core-second**, roughly 750 times faster than XMage on the same benchmark.
  - dirac is verified against gorge in lockstep: the same game is played in both
    and compared decision by decision.

- **DeepScry is the most mature search-first engine, but its docs say little
  about search internals.** It is closed source, and the guide covers
  determinism, replay, networking and hidden information. What it does show:

  - Its headline number is "about 4 million game actions per second per core".
    An "action" there is one undo-log entry, not a decision or a search node.
  - It runs MCTS at about 1,000–2,000 rollouts per second in single-threaded
    WebAssembly (WASM).
  - Its 300 ms MCTS bot beats its heuristic bot about 71% of the time, and its
    1000 ms bot beats its 300 ms bot about 78% of the time.

- **Arcana, phase.rs and Ironsmith are not good foundations for Beryl.** All
  three are written in Rust, very large, mostly written by large language models
  (LLMs), and slow per node:

  - Arcana clones a state in 14 µs and plays 179 random games per second.
  - phase.rs's game state is about 14 KB inline with 362 fields. Its strongest
    AI searches 96 nodes in 1.5 s.
  - Ironsmith decides through callbacks plus replay and uses global ID counters,
    and it is under a restrictive license.
  - Each still has specific ideas worth taking (§5).

- **The research supports cheap determinization over elaborate information-set
  methods.** Three results point the same way:

  - Cowling, Ward and Powley (2012), the only MTG-specific MCTS paper: many
    shallow determinized trees, with multi-card choices broken into binary
    decisions.
  - PIMC theory (Long et al. 2010).
  - A Rust Legends of Code and Magic (LOCM) agent from September 2026 that beat
    the champion with world sampling plus a value network.

  PIMC means perfect-information Monte Carlo: sample a full hidden state, then
  search it as if everything were known.

- **Design recommendation, in brief.** For Beryl's engine:

  - a flat, `Copy`-able, fixed-capacity state with no heap allocation;
  - immutable card tables generated from data;
  - a step/apply state machine whose answers are factored single picks;
  - strict determinism and well-defined termination;
  - correctness checked by differential testing against a slower reference
    engine.

  §7 has the full list.

______________________________________________________________________

## 2. Scope and method

**What was done:**

- **Primary sources.** We read the code or docs of every starting point the user
  named:
  - [DeepScry](https://deepscry.net/guide/print.html)
  - [Arcana](https://github.com/levineuwirth/arcana)
  - [phase.rs](https://github.com/phase-rs/phase)
  - [Ironsmith](https://github.com/Chiplis/ironsmith)
  - [Forge](https://github.com/Card-Forge/forge)
  - [XMage](https://github.com/magefree/mage)
- **Open-source repos** were cloned shallowly and read. Nothing was built or
  run.
- **Wider survey.** We also searched other engines, academic papers, and engines
  for similar card games such as Hearthstone and LOCM.

**Caveats:**

- **Almost every performance number is self-reported**, mostly from README files
  or project benchmarks that nobody has independently reviewed.
- **Hardware, card pools and units differ** from one number to the next. Read
  cross-engine ratios as orders of magnitude, not exact factors.
- **Fact versus inference.** A claim with a link is what the source says.
  Analysis and recommendations are marked as such.

______________________________________________________________________

## 3. Landscape at a glance

**Engine overview**

| Engine            | Lang         | License                            | Cards                            | State model                                                      | Copy / clone cost                               | AI                                         |
| ----------------- | ------------ | ---------------------------------- | -------------------------------- | ---------------------------------------------------------------- | ----------------------------------------------- | ------------------------------------------ |
| **DeepScry**      | Rust (+WASM) | Closed (card data GPL-3 via Forge) | ~24.7k loaded                    | Serde struct + undo log; rollout = clone with log off            | not published                                   | MCTS, heuristic (Forge port)               |
| **dirac** (gorge) | Go           | Apache-2.0                         | verified subset (FDN, BLB pools) | Flat fixed-capacity value \<24 KiB + read-only card table        | **40 ns**, 0 alloc                              | random / heuristic bots (built for search) |
| **gorge**         | Go           | Apache-2.0                         | 32,393 of 34,074 Forge scripts   | Object engine + hash-chained event log                           | 22.7 µs, 200 KB, 51 mallocs                     | bot                                        |
| **Arcana**        | Rust         | MIT                                | 20,590 (LLM-generated)           | `#[derive(Clone)]` struct, `FxHashMap` arena                     | 14–30 µs                                        | PIMC, ISMCTS, flat MC                      |
| **phase.rs**      | Rust (+WASM) | MIT/Apache-2.0                     | ~32k (89%) parse-complete        | 362-field `GameState`; `im::HashMap<Id, Arc<Obj>>` copy-on-write | not published; full clone per action            | beam + rollout, ≤96 nodes                  |
| **Ironsmith**     | Rust (+WASM) | BSL 1.1 / AGPL (inconsistent)      | ~31k claimed                     | `im` persistent maps + `Arc::make_mut` + change journal          | benchmark exists, no results                    | none (auto-pass)                           |
| **Forge**         | Java         | GPL-3.0                            | 34,137 scripts                   | Mutable object graph                                             | rebuilds each card from script; ~80% of AI time | heuristic, flat MC (new), DFS sim          |
| **XMage**         | Java         | MIT                                | 32,782 classes                   | Mutable graph, UUID keys, hand-written deep copy                 | full deep copy; "10s–1000s per priority"        | alpha-beta (5k nodes), MCTS (experimental) |
| **Magarena**      | Java         | GPL-3.0                            | (not checked)                    | Mutable graph, deep clone per MCTS iteration                     | —                                               | MCTS (honest and cheating), minimax        |

**Engine-level throughput.** The workloads differ, so the units are not
comparable.

| Engine        | Throughput                                                                     |
| ------------- | ------------------------------------------------------------------------------ |
| **DeepScry**  | "~4M undo-log actions/s/core" (native); ~1–2k MCTS rollouts/s (WASM, 1 thread) |
| **dirac**     | **1,800 random games/core-s**; 2.7 µs/transition                               |
| **gorge**     | 70–84 random games/core-s                                                      |
| **Arcana**    | 179 random games/s (its own deck)                                              |
| **phase.rs**  | ~3.4 s per AI-vs-AI game; ~1.15 ms/action incl. AI (debug build)               |
| **Ironsmith** | legal-action enumeration 4–23 ms warm (WASM)                                   |
| **Forge**     | 1.02 games/core-s (Forge AI)                                                   |
| **XMage**     | 2.38 random games/core-s; 19.3 with the simple AI                              |
| **Magarena**  | "a few seconds" for enough MCTS samples                                        |

**Other engines worth knowing:**

- [MTG-Ichor](https://github.com/lcmaier/MTG-Ichor) (Rust): correctness first.
  Layers, replacement effects and triggers are largely done.
- [mtg-kernel](https://github.com/jackmaiorino/mtg-kernel) (Rust): built for
  reinforcement learning (RL). It plays 18.6 games/core-s on the gorge
  benchmark.
- [kiln-mtg](https://github.com/SeanMcKeen/kiln-mtg) (Rust): 402 Commander
  cards, each hand-verified.
- [Argentum](https://github.com/wingedsheep/argentum-engine) (Kotlin): immutable
  state, O(1) `fork()`, and AlphaZero-style self-play.
- [managym](https://github.com/jacklionheart/managym) (C++): a toy engine,
  archived.
- [Wagic](https://github.com/WagicProject/wagic) (C++): uses a greedy AI.
- [Parun](https://github.com/parun-mtg/parun) (Haskell): cards are pure data
  with "no escape hatch".
- [MTG Arena's GRE](https://magic.wizards.com/en/news/mtg-arena/on-whiteboards-naps-and-living-breakthrough)
  (C++ plus CLIPS rules generated from Oracle text): closed, and no performance
  figures have been published.

______________________________________________________________________

## 4. Engine profiles

### 4.1 DeepScry (closed source, Rust)

> Source: [guide](https://deepscry.net/guide/print.html),
> [homepage](https://deepscry.net/), the public
> [bug tracker](https://github.com/DeepScryAI/DeepScry_bugs), the WASM binding
> doc comments, and the arena win-rate API. The author is most likely Ryan
> Newton (inferred from public artifacts; the site names no one).

**What it is.** A Rust rewrite of Forge "engineered for tree-search AI":

- It runs both natively and as WebAssembly.
- It runs Forge's card-script DSL unchanged; it does not parse Oracle text.
- Card identities are hidden behind numeric catalog IDs, with names supplied by
  a separate skin.
- It is two-player only.
- It is still experimental: issue
  [#170](https://github.com/DeepScryAI/DeepScry_bugs/issues/170) shows
  unimplemented effects resolving as no-ops.

**Documented architecture:**

- **Determinism above everything.** A game is a pure function of the decks, the
  seed and the ordered list of choices.
  - There is no wall-clock time, no ambient randomness, and no parallelism
    inside a game.
  - The shuffle seed is kept separate from the gameplay seed.
  - State hashes are checked after every choice.
  - A replay hash is compared across native and WASM builds.
- **Undo log.** Every mutation is a `GameAction` log entry, such as `MoveCard`,
  `TapCard`, `RevealCard` or `ChoicePoint`.
  - It supports rewinding to the start of a turn, to a choice point, or to an
    action count, followed by a bit-identical replay.
  - **The undo is incomplete.** Undoing single actions does not restore
    end-of-turn transients (marked damage, until-end-of-turn effects, combat
    state). The only fully trusted anchors are turn starts.
- **Search clones state.** "Inside a Monte-Carlo tree-search ROLLOUT the state
  is cloned with the undo log disabled"
  ([#88](https://github.com/DeepScryAI/DeepScry_bugs/issues/88)). This has a
  side effect: rules that read history, such as "left the graveyard this turn",
  read zero during search.
- **Late-binding hidden information.** The design:
  - Every card slot gets a public, deterministic `CardId`.
  - What the card actually is gets bound per viewer through a
    `revealed_to_mask: u8`.
  - `RevealCard` is a first-class, undoable action, and it is always logged
    before the identity is used.
  - It replaced an older scheme that counted hidden cards.
- **Information-independence rule.** Every controller must make the same
  decisions on the full state and on a client's shadow state. A network-vs-local
  equivalence test checks this.
  - The docs admit the API still leaks: `player_hand(opponent)` returns real
    data in local mode.
- **Caching discipline.** Mana-source caches are bucketed by color, and per-card
  type-flag caches are read-only.
  - The invariant is "Cache is valid ⟺ every mutation has gone through an
    invalidating API method", backed by epoch counters.
  - Caches are skipped by serde and marked dirty on clone.
  - A real bug happened when one direct write of `card.tapped = true` bypassed
    invalidation.
- **Controller interface.** It is Forge-style: the engine calls the AI through
  `choose_*()` callbacks.
  - Arguments are borrowed `&[CardId]` slices and results are `SmallVec`s, "to
    maintain high performance even during tree search with millions of game
    states".
  - Re-entering from WASM needs a rewind and replay, because a Rust stack cannot
    be suspended.
- **Testing.**
  - A `.pzl` puzzle DSL with board setup, scripted actions, and assertions over
    the event log.
  - Golden logs, fuzz and stress harnesses, and replay-based reproducers.
  - LLM agent play used for bug hunting.

**Numbers:**

- "Roughly 4 million game-actions/sec per core — tens of thousands of full
  turns/sec" ([homepage](https://deepscry.net/)). The unit is undo-log entries
  per second in a rewind-and-replay benchmark.
- Browser MCTS logs show 570–1,213 rollouts and 87–466 tree nodes per decision
  at about a 0.6 s budget.
- Arena results, mirror matches
  ([`/api/arena/win-rates`](https://deepscry.net/api/arena/win-rates)):
  - MCTS 300 ms/1 thread vs heuristic: about 71%, over 29k games.
  - MCTS 1000 ms vs MCTS 300 ms: about 78.5%, over 7k games.
- Not documented anywhere: the MCTS selection policy, how determinization works,
  the memory layout, the size of a state, clone cost, the layer system,
  replacement effects, and state-based actions.
  - The tracker shows shortcuts that now need large reworks. One example:
    targeted triggers skip the stack and auto-pick the target with the lowest
    `CardId` ([#167](https://github.com/DeepScryAI/DeepScry_bugs/issues/167)).

**Takeaways for Beryl (inference):**

*Adopt:*

- Stable public IDs plus reveal masks. With these, determinization only rebinds
  the faces behind unrevealed IDs, so the tree structure stays the same.
- The information-independence test, made stronger by giving agents only a
  type-level observation view.
- A serialized RNG with separate seeds.
- The caching invariant.
- The puzzle-based testing approach.
- An arena that runs bots at fixed time budgets.

*Avoid:*

- An engine that calls into the AI.
- Undo that is only partly exact.
- Rules facts that can only be derived from the history log.
- Shortcuts that bypass the rules, such as triggers that skip the stack.
- A benchmark unit that nobody else uses.

### 4.2 gorge / dirac (Go, Apache-2.0)

> Source: [repo](https://github.com/adams-shaun/gorge),
> [dirac architecture](https://adams-shaun.github.io/gorge/architecture/dirac/),
> [performance](https://adams-shaun.github.io/gorge/performance/). The repo was
> created 2026-09; it has a single author and 5 stars.

**gorge, the reference engine:**

- It compiles Forge card scripts into an intermediate representation (IR) and
  plays 32,393 of the 34,074 cards.
- It writes a hash-chained deterministic event log.

**dirac, the search engine:**

- **State.** A game is a flat `State` under 24 KiB plus a small buffer sized to
  the card table. Everything that varies by card lives in a read-only table.

- **Card tables.** `kgen` lowers gorge's IR into `kop` ops and writes binary
  tables.

- **Fixed capacities.**

  | Limit                                | Capacity |
  | ------------------------------------ | -------- |
  | Players                              | 2        |
  | Card instances                       | 192      |
  | Permanents                           | 112      |
  | Stack entries                        | 48       |
  | Pending triggers                     | 48       |
  | Delayed triggers                     | 32       |
  | Attack/block options per declaration | 512      |
  | Triggers resolved per turn           | 2,000    |

  Going past any capacity is a **stall**, which ends the game as a draw with a
  named `StallReason`. About a hundred reasons exist.

- **Decisions.** There are 8 kinds, and every one is answered with a *single
  pick*. gorge, by contrast, has 13 kinds that include multi-select answers. The
  loop is `Step()` to reach the next question, then `Apply(answer)`.

- **No event log or per-seat view inside the engine.** An adapter derives those
  from positions.

- **Lockstep verification.** Every dirac game is replayed through gorge decision
  by decision.

  - A card counts as *supported* only after lockstep games have exercised every
    one of its abilities at least 3 times.
  - Results: 1,399 FDN and 1,456 BLB games with **0 divergences**.
  - Lockstep has also found bugs in gorge itself.

**Benchmarks.** Ryzen 9 9950X, one core, Foundations (FDN) limited decks:

| Engine · policy             | games/core-s | µs/turn | µs/decision | peak RSS |
| --------------------------- | ------------ | ------- | ----------- | -------- |
| dirac · random              | 1,800        | 23.1    | 4.54        | 100 MB   |
| dirac · heuristic bot       | 1,960        | 29.1    | 5.65        | 97 MB    |
| gorge · random (manual tap) | 83.7         | 293     | 10.7        | 356 MB   |
| XMage · random              | 2.38         | 13,193  | 499         | 1,228 MB |
| XMage · mad AI              | 0.947        | 62,343  | 2,902       | 1,288 MB |
| Forge · Forge AI            | 1.02         | 54,513  | n/a         | 882 MB   |
| mtg-kernel (Rust) · random  | 18.6         | 3,075   | —           | —        |

| Operation                  | Time                      | Instructions | Allocations        |
| -------------------------- | ------------------------- | ------------ | ------------------ |
| dirac transition (FDN)     | 2,748 ns                  | 45.6k        | 0                  |
| dirac `CopyFrom`, mid-game | **40.2 ns**               | 187          | 0                  |
| gorge `Engine.Clone`       | 22.7 µs (45.3 µs with GC) | —            | 51 mallocs, 200 KB |

Caveats from the source: dates, machine load and card coverage all differ
between rows, so the ratios are orders of magnitude only. dirac's random runs
stalled in 385 of 215,042 games.

**Takeaways for Beryl (inference):** this is the design pattern to beat.

- Its results suggest these properties together make search cheap:
  - a flat, memcpy-able state;
  - card logic compiled into read-only tables;
  - declared capacity limits with explicit stall outcomes;
  - single-pick decisions;
  - a fast engine checked against a slower, broader reference engine.
- A 24 KiB state fits comfortably in an Apple M-series P-core's L1 data cache,
  which is 128 KB.
- The 40 ns copy figure is almost certainly a hot-cache measurement. (A single
  `rep movsb` instruction accounts for the low instruction count.)
- One open question for Beryl is whether to run two engines like this, or a
  single fast engine plus differential tests against Forge or XMage.

### 4.3 Arcana (Rust, MIT)

> Source: [repo](https://github.com/levineuwirth/arcana). Single author, 673
> commits since 2026-04. 634 of 641 audited commits are co-authored by Claude.

**What it is.** An engine built as an RL substrate:

- The pure step function is
  `step(GameState, Action, &CardRegistry) -> (GameState, EngineYield)`.
- `EngineYield::PendingDecision` carries the list of legal actions.
- It has 20,590 card files, each hand-written in Rust and generated by an LLM.
- The README's targets were more than 20k games/s and clones under 5 µs. The RL
  research lines were later frozen as documented negative results.

**State:**

- One `#[derive(Clone)]` struct holding an
  `FxHashMap<ObjectId(u32), GameObject>` arena.
- Each `GameObject` owns `Vec`s and characteristics that live on the heap.
- Zone queries scan every object.
- The event log grows for the whole game and is cloned along with the state.
- Card behavior is stored as bare `fn` pointers, so the state cannot be
  serialized.
- It is deterministic: Fx hash maps are enforced by a test that greps the
  source.

**Rules coverage:**

- Covered: priority and stack, triggers in APNAP order, last-known information,
  replacement effects, layers 1–7e, and a CR 727 loop detector.
- Missing:
  - Layer dependencies (CR 613.8) are a TODO.
  - The order of replacement effects is not chosen by the player.
  - Characteristics are recomputed on every query with no cache, from about 30
    call sites.

**Search:**

- It has flat Monte Carlo, PIMC, and single-observer ISMCTS (80 iterations, 16
  children per node, rollouts capped at 200 steps).
- **The authors found ISMCTS underperforms PIMC** and ship PIMC.

**Two information leaks they found:**

1. The engine's `rng_seed`, which decides future shuffles, was visible inside
   sampled worlds.
2. Object IDs were allocated in decklist order, so an ID revealed the card's
   library position.

**Numbers** (from `docs/audits/2026-10-06.md`):

| Benchmark     | Result                | README target |
| ------------- | --------------------- | ------------- |
| Random game   | 5.57 ms (179 games/s) | >20k games/s  |
| Clone         | 14.2 µs               | \<5 µs        |
| Legal actions | 3.35 µs               | —             |
| Single step   | 9.85 µs               | —             |

- About 10.3 µs of the clone time is the object arena.
- Later measurements show clones at 21–30 µs.
- A PIMC 32/320 game takes 75–208 s.

**Takeaways for Beryl (inference):**

*Adopt:*

- The pure step API.
- The grep test that bans nondeterministic hash maps.
- Arcana's checklist of information leaks: RNG seed and ID allocation order.
- `stable_candidate_subset`, which keeps action sampling from being biased by
  enumeration order.
- A behavioral "no-op probe" for generated cards.

*Avoid:*

- Heap-owning objects.
- A cloned event log.
- `fn` pointers inside the state.
- Characteristics recomputed with no cache.
- Auto-tap that clones the state.
- A single `CastSpell` action that bundles targets, modes and payment. That
  multiplies branching.

### 4.4 phase.rs (Rust, MIT/Apache-2.0)

> Source: [repo](https://github.com/phase-rs/phase). About 8.6k commits since
> 2026-03, 48 contributors, heavily LLM-written. Roughly 2.46M lines in the
> engine crate alone (about 1.18M of them tests).

**What it is:**

- A web-first, "Arena-quality" open-source client.
- The engine compiles to WASM, with a React UI, an Axum server, and a thin Tauri
  shell.
- Release builds use `opt-level = 'z'`, which optimizes for size.

**Card pipeline. This is its standout asset:**

- MTGJSON Oracle text goes through about 694k lines of `nom` parser combinators
  into a typed AST in `phase-ast`. The `Effect` enum has about 238 variants.
- The result is interpreted at runtime.
- 32,297 of 36,280 cards (89%) parse with no unimplemented nodes. That measures
  parse coverage, not verified behavior.
- Forge scripts can optionally fill gaps.

**State:**

- `GameState` has **362 fields** and is about **14 KB inline**; it was 30 KB
  before large fields were moved into boxes. Size limits are asserted at compile
  time.
- Objects live in `im::HashMap<ObjectId, Arc<GameObject>, Fx>` and are copied on
  write through `Arc::make_mut`.
  - That change took a Hard AI-vs-AI game from about 5.4 s to about 3.4 s, and
    peak RSS from about 1.5 GB to 721 MB.
- `GameObject` has 170 fields of `String`, `Vec` and `HashMap`, and layer
  results are written straight onto the object.
- `apply_action_boundary_core` takes a full `Arc::new(state.clone())` snapshot
  **on every action**, for rollback.
- Legality is checked by clone-and-apply: 8,576 legality clones per 9,000
  actions.

**Rules:**

- The breadth is very large. Layers use `petgraph` Tarjan SCC to resolve
  dependencies, and the code has 398 citations of CR 613.
- There are 44,934 tests.
- The decision loop is `WaitingFor` (144 variants) answered with `GameAction`
  (140 variants). Some decisions are atomic and combinatorial: a single
  `DeclareAttackers` action carries the whole attack.

**AI:**

- It is not MCTS. It is beam search plus rollouts and heuristic policies.
- The budgets are 24 nodes at Medium and 96 nodes at the top tier, within 1.5 s.
- **Every shipped tier searches with perfect information.**
  `determinization_samples: 0` is described as a "product decision".
- Evaluation uses logistic-regression weights trained on 90M 17lands samples,
  split by game phase.

**Determinism:** it uses a ChaCha20 RNG with a saved stream position, but
iteration order of `std` `HashMap`'s `RandomState` leaks into AI decisions
([#4878](https://github.com/phase-rs/phase/issues/4878)).

**Takeaways for Beryl (inference):**

*Adopt:*

- The Oracle → typed-AST parser, as an offline bootstrapping tool for the cube
  card list. Its permissive license allows that.
- The CR-cited test corpus, as a rules oracle.
- The perf-counter CI gate. It tracks 45 logical-operation counts (clones, full
  layer passes, scans) with a tolerance band, which gives a more stable signal
  than wall-clock time.
- Compile-time size assertions on hot types.
- Evaluation features fitted on 17lands data.

*Do not fork it.* It is far too heavy per node and nondeterministic, and its
size makes it hard to audit.

### 4.5 Ironsmith (Rust, BSL 1.1 / AGPL)

> Source: [repo](https://github.com/Chiplis/ironsmith). 4,687 commits since
> 2026-03, about 2.94M lines of Rust. `LICENSE.md` is BSL 1.1, which allows
> non-commercial use only and converts to MIT after 4 years, but the crate
> manifests say AGPL-3.0. **Either way, its code can't be reused; only its ideas
> can.**

**What it is:**

- A browser engine that compiles cards from Oracle text through a full compiler
  pipeline: grammar → AST → semantics → lowering.
- Compiled cards are rendered back to English and scored against the Oracle
  text.
- It claims 31,126 of 32,209 cards supported.
- It is aimed at serverless multiplayer verified with mental poker and
  zero-knowledge proofs.

**Rules fidelity is the highest among the Rust engines:**

- Real CR 613.8 layer dependencies, resolved by simulating effects.
- Replacement-effect order chosen by the player.
- Per-viewer library knowledge.
- 1,744 test scripts ported from XMage.

**State:**

- Persistent `im::HashMap<ObjectId, Arc<Object>>` with copy-on-write and a
  `ChangeJournal`, so clones are O(1).
- About 70 fields per object.
- Effects are `Arc<dyn EffectExecutor>`, with 475 implementations.
- A `Cell`/`RefCell` RNG and caches live inside the state.
- **Process-global atomic ID counters** are a problem for parallel games.

**Decisions:**

- Made through a synchronous `DecisionMaker` callback trait.
- To wait for a choice made mid-resolution, the engine restores a checkpoint and
  replays the root action.
- Priority actions *are* factored: cast first, then targets, modes and payment
  as separate steps.
- There is no search AI.

**Numbers:** legal-action enumeration took 251 ms cold and 4 ms warm in WASM.

**Takeaways for Beryl (inference):**

- `StableId` alongside a per-incarnation `ObjectId`.
- Change journals.
- Dependency resolution by simulation.
- Factored casting.
- Round-trip verification of compiled cards.
- Ported XMage tests as a rules oracle.

### 4.6 Forge (Java, GPL-3.0)

> Source: [repo](https://github.com/Card-Forge/forge). About 15+ years old,
> 34,137 card scripts, very active.

**Cards** are scripts in a key-value DSL, which is the richest machine-readable
corpus of MTG abilities:

```
Name:Lightning Bolt
ManaCost:R
Types:Instant
A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SpellDescription$ ...
```

- Line prefixes: `T:` for triggers, `R:` for replacements, `S:` for statics, and
  `SVar:` for sub-abilities and AI hints.
- AI hints are mixed into card data, for example `SVar:PlayMain1`.
- Layers are recomputed in full by `checkStaticAbilities()`.

**Simulation cost:** `GameCopier` builds a brand-new `Game` and **re-parses
every card from its script**. The code itself says this "accounts for the vast
majority of GameCopier execution time". The original author
[estimated](https://www.slightlymagic.net/forum/viewtopic.php?p=190653) it at
about 80%. Many fields are not copied (TODO lists), and a dedicated check exists
to detect copy errors.

**AI:**

- **Heuristic AI.** 155 `*Ai` classes, one per effect API, return
  `AiPlayDecision`.
  - Forge's own docs say it is weak with control and "pretty bad" with combo.
- **Simulation AI.** A depth-3 depth-first search over the AI's own action
  sequences.
  - It plays "solitaire": it never simulates the opponent's responses.
- **Flat Monte Carlo picker, merged 2026-10-23
  ([PR #12308](https://github.com/Card-Forge/forge/pull/12308)) and on by
  default.**
  - It takes the top 5 heuristic candidates and runs 4 rollouts for each, within
    a 450 ms budget.
  - Each rollout copies the game and re-deals hidden cards with
    `HiddenInfoShuffler`.
  - The heuristic then plays out the rest of the turn, and the position is
    scored at main phase 2.
  - All candidates use common random numbers (the same seeds).
  - Inference: that is about 25–100 ms per rollout.

**Draft AI:** pick-order `.rnk` rankings plus color commitment and synergy
bonuses. It includes a `rankings_cubecobra.txt` for cubes.

**Takeaways for Beryl (inference):**

- **Card scripts.** They are the best reference for how abilities decompose.
  Mechanically translating them into Beryl's format probably makes the result a
  GPL derivative. Card names and Oracle text are not Forge's copyright.
  - gorge and DeepScry both chose to consume the scripts directly.
- **`HiddenInfoShuffler`.** It re-deals unknown cards into the same slots and
  keeps known cards fixed. That is a correct determinization primitive.
- **Common random numbers across candidates.**

### 4.7 XMage (Java, MIT)

> Source: [repo](https://github.com/magefree/mage). 32,782 card classes,
> client/server, public servers.

**Cards** are one Java class each, composed from a library of 716 shared
effects:

```java
this.getSpellAbility().addTarget(new TargetAnyTarget());
this.getSpellAbility().addEffect(new DamageTargetEffect(3));
```

- Everything is driven by `GameEvent`.
- Replacement effects implement `checksEventType`/`applies`/`replaceEvent`.
- "Watchers" track history.
- `ContinuousEffects.apply()` walks explicit layer and sublayer enums
  (`CopyEffects_1` … `PT_7`).

**Copying:**

- State is mutable and keyed by UUID.
- Hand-written copy constructors deep-copy every card, ability, effect and
  watcher.
- `createSimulationForAI()` is simply `copy()`. Rollback stores full copies.
- [Issue #9619](https://github.com/magefree/mage/issues/9619):
  - "The AI copies the state 10s-1,000s of times every time it gets priority."
  - AI games were disabled on the beta server.
  - The maintainers declined a rewrite to a delta-based state.

**AI:**

- **"Mad" AI** (`ComputerPlayer6/7`): alpha-beta search.
  - Depth is max(4, skill), capped at 5,000 simulated nodes, on 5 threads.
  - After the AI casts something, opponents just pass, so it never models
    instant-speed responses.
  - **It appears to search with perfect information**, copying the real game
    without determinization (inferred from the code).
- **`ComputerPlayerMCTS`**: textbook UCT with root parallelization and random
  playouts to the end of the game.
  - Determinization shuffles libraries and re-deals hands, ignoring revealed
    cards.
  - It is experimental.

**Draft AI:**

- `RateCard` uses CSVs derived from 17Lands data, under CC BY 4.0.
- It falls back to a heuristic for unrated cards.

**Takeaways for Beryl (inference):**

- XMage's MIT license makes it the legally clean reference for decomposing
  abilities.
- Its test suite (`Mage.Tests`) can be mined for rules test cases, as Ironsmith
  did.
- It is a convenient engine to play differentially against Beryl's (mtg-kernel
  bridges to it).

### 4.8 Magarena (Java, GPL-3.0)

> Source:
> [MCTSAI.java](https://github.com/magarena/magarena/blob/master/src/magic/ai/MCTSAI.java),
> [developer slides](https://www.slideshare.net/slideshow/building-a-state-of-the-art-ai-to-play-magic-the-gathering/38744310),
> [forum](https://www.slightlymagic.net/forum/viewtopic.php?p=116571).

This is the oldest real-rules MCTS for MTG.

**How it works:**

- Every iteration deep-clones the game.
- Hidden cards are hidden and then re-randomized per playout, so it determinizes
  only during rollouts.
- Selection is UCB1 with C = 0.4.
- Rollouts are capped at 1,000 choices.
- Proven wins and losses are backed up.
- An LRU cache keeps 1,000 subtrees, keyed by state ID.
- It runs on a thread pool with virtual loss.

**Results:**

- The honest (non-cheating) MCTS overtook minimax only at about 4 s per move:
  1.71 against minimax's 1.0 at 4 s, versus 0.88 at 1 s.
- The developer said more simulations per second would substantially improve
  strength.
- They also said "MCTS makes bad plays when it is losing." That is a known
  pathology of win/loss rewards when the evaluation is saturated.

______________________________________________________________________

## 5. Research on search for MTG and similar card games

### 5.1 MTG-specific work

**Cowling, Ward & Powley 2012**, "Ensemble Determinization in MCTS for the
Imperfect Information Card Game Magic: The Gathering," IEEE TCIAIG 4(4)
([PDF](https://eprints.whiterose.ac.uk/75050/1/EnsDetMagic.pdf)). This is still
the only rigorous MCTS study specific to MTG. It used a simplified game: lands
and vanilla creatures, focused on combat. Findings:

- **Chance nodes are a dead end.** Naive MCTS with a chance node for every draw
  won only 23% against the expert rules-based player.
- **Many shallow determinizations beat a few deep ones.** At 10k simulations,
  about 40 determinizations × 250 simulations was best. Extra budget is "best
  used in running additional determinizations rather than searching each
  determinization more deeply."
- **Binary decomposition (BT).** Turning "which subset of cards to cast" into a
  sequence of yes/no decisions made search much faster (0.23 s/move versus about
  1.0 s) and stronger. BT plus dominated-move pruning reached 50.5% against the
  expert player in 0.20 s/move.
- **Use simple rollout policies.** Rollouts driven by a *weaker* "reduced rules"
  player beat rollouts driven by the expert player by about 10 percentage
  points.
- Also tested: rewards discounted over turns (λ = 0.99), a negative reward for a
  loss, and an "interesting determinizations" filter.

**Ward & Cowling 2009**, "Monte Carlo search applied to card selection in MTG"
(CIG). It found flat Monte Carlo with rule-based rollouts promising.

**Complexity results:**

- MTG is Turing complete (Churchill, Biderman & Herrick,
  [arXiv 1904.09828](https://arxiv.org/abs/1904.09828)): deciding the winner is
  undecidable even when every move is forced.

- Mate-in-n is Δ⁰ₙ-hard ([arXiv 2003.05119](https://arxiv.org/abs/2003.05119)).

- Practical consequence (inference): Beryl's engine needs:

  - step and loop budgets, and CR 727 shortcut and loop detection;
  - capped quantities with defined overflow behavior;
  - an outcome for every case where a budget runs out.

  dirac's draw-on-stall is one workable policy.

**Recent MTG RL and LLM projects.** None has produced a strong player:

- MageZero (AlphaZero-style PUCT on an XMage fork, about 150 sims/s).
- MTG-Causal-RL ([arXiv 2605.06066](https://arxiv.org/abs/2605.06066)): a
  56-card pool with a 478-action masked space.
- managym/manabot.
- mage-bench / Commander Bench (LLMs playing in XMage).
- Arcana's RL lines, frozen as negative results.

### 5.2 Determinization versus information-set search

**ISMCTS** (Cowling, Powley & Whitehouse 2012,
[PDF](https://eprints.whiterose.ac.uk/75048/1/CowlingPowleyWhitehouse2012.pdf)):

- It addresses determinization's known flaws:
  - *Strategy fusion*: assuming you can choose differently in states you cannot
    tell apart.
  - *Non-locality*: ignoring that opponents steer play toward some hidden states
    and away from others.
- It is not uniformly better:
  - **In Dou Di Zhu**, a card game, ISMCTS only matched determinization, because
    the union of legal actions across an information set made the branching
    factor "an order of magnitude larger."
  - **Arcana** independently found ISMCTS underperforming PIMC.
- Inference: MTG, where an opponent's possible hands imply huge unions of
  possible actions, is closer to Dou Di Zhu than to games where ISMCTS shines.

**When PIMC works** (Long, Sturtevant, Buro & Furtak, AAAI 2010,
[paper](https://ojs.aaai.org/index.php/AAAI/article/view/7562)):

- Three properties predict PIMC's success: high leaf correlation, low bias, and
  a high *disambiguation factor* (hidden information resolves quickly).
- MTG largely has these properties: hands are small, and cards reveal themselves
  as they are played.
- **Extended PIMC** ([arXiv 2408.02380](https://arxiv.org/abs/2408.02380))
  delays the perfect-information leaf evaluation to a deeper depth, which
  provably reduces strategy fusion.

**LOCM, September 2026** (Rubin,
[arXiv 2609.06816](https://arxiv.org/abs/2609.06816)). This is the strongest
recent evidence for Beryl's approach:

- A **Rust reimplementation of LOCM that runs 443× faster** than the reference.
- It samples worlds from a prior over the opponent's deck.
- It runs beam search over whole-turn plans: beam width 24, up to 384 worlds in
  185 ms, with strength saturating around 32 worlds.
- Leaves are scored by a value network trained by imitation.
- It beat the RL champion ByteRL 51.35% over 10,000 pre-registered games.
  Without search the same agent scores 26.8%.
- It measured the PIMC properties directly: strong disambiguation and high leaf
  correlation.

**Hearthstone** (Świechowski et al. 2018,
[arXiv 1808.04794](https://arxiv.org/abs/1808.04794)):

- A custom C++ simulator with 483 cards ran about 10k full games/s.
- It determinized on every iteration and used information-set keys for the
  transposition table.
- Actions were broken into atomic steps (card → position → target).
- Value-network guidance lifted the first player's win rate from 73.5% to 82.0%
  at 0.5 s/move.

**Hidden-information games at scale:**

- **ReBeL** needs network inputs that grow with the number of information states
  per public state. That is intractable when there is very little common
  knowledge, as in MTG hands drawn from 40-card decks.
- **Ataraxos** (Stratego, 2025,
  [arXiv 2511.07312](https://arxiv.org/abs/2511.07312)) combines a belief
  network trained on public observations with cheap test-time search, and beat
  the top human 15–1.
- **SPARTA** (Hanabi) shows search on top of a blueprint policy, using Monte
  Carlo rollouts, that is guaranteed not to make the blueprint worse.

**Synthesis (inference).** The practical path for Beryl, in order of increasing
sophistication:

1. Ensemble PIMC or determinized MCTS at the root. Sample worlds consistent with
   what the searcher knows, and aggregate visit counts or values across worlds.
2. A learned belief model for sampling opponent hands, as in Ataraxos and the
   LOCM paper. In cube, uniform sampling over a 540-card pool is a weak prior,
   so this matters more than in a fixed-deck game.
3. A value network at the leaves instead of rollouts to the end of the game.
4. EPIMC-style deferred evaluation, or ISMCTS restricted to an abstracted action
   space, for bluff-sensitive spots.

**What every route needs from the engine:**

- very cheap copies;
- fast sampling of hidden state that is consistent with what the searcher knows;
- factored actions.

______________________________________________________________________

## 6. Cross-cutting engine design patterns

This section groups the evidence by design decision. Each "lean" is our
inference.

### 6.1 State representation and copying

| Approach                                            | Examples                      | Copy cost                          | Notes                                                                  |
| --------------------------------------------------- | ----------------------------- | ---------------------------------- | ---------------------------------------------------------------------- |
| Mutable object graph + deep copy                    | Forge, XMage, Magarena        | ms-range; dominates AI time        | Copy-fidelity bugs are endemic (missed fields)                         |
| Owned-heap struct, `derive(Clone)`                  | Arcana                        | 14–30 µs                           | Each object's `Vec`s dominate                                          |
| Persistent / copy-on-write (`im` + `Arc::make_mut`) | phase.rs, Ironsmith, Argentum | O(1) clone; cost paid per mutation | phase.rs halved RSS this way, but full layer passes unshare everything |
| Undo log (make/unmake)                              | DeepScry                      | n/a; rollouts still clone          | Exact undo is hard; DeepScry's skips end-of-turn transients            |
| **Flat fixed-capacity value**                       | **dirac**                     | **40 ns, 0 alloc**                 | Needs capacity limits + stall semantics; card data moved to tables     |

**Lean: a flat, `Copy`, heap-free `GameState` with a small byte budget**, say ≤
16–24 KiB:

- Fixed arrays or `ArrayVec` with small-integer indices (`u8`/`u16`).
- Zones and per-property flags as bitsets: `[u64; N]` over object slots, in the
  style of chess bitboards.
- All per-card static data in read-only tables shared across every state.
- Use copy-make (copy, then apply) rather than undo. Determinized search
  re-samples the root and runs parallel trees, which favors whole-state copies.
  - Chess experience suggests copy-make costs only a few percent when the state
    is small.
- Rust tools that fit: `arrayvec` and `fixedbitset`.
- Rust tools to avoid in hot state: `slotmap`'s generational keys (fine outside
  hot state), and `im`/`imbl`/`rpds` (keep them for cold history).

### 6.2 Object identity

MTG says a card becomes a new object each time it changes zones. Three engines
handle this the same way:

- Ironsmith separates a `StableId` from a per-incarnation `ObjectId`.
- phase.rs keys last-known-information caches by incarnation.
- MTG Arena's developer blog describes a bug where an object ID was stored
  instead of the value it needed.

**Lean:** a dense slot index (for storage), a monotonic incarnation counter (for
rules identity), and a public card-slot ID (for hidden information, as in
DeepScry).

**Determinism caution:** allocation order must not leak hidden information, as
Arcana's ID-order leak showed.

### 6.3 Card definitions

| Approach                 | Examples                           | Trade-off                                                            |
| ------------------------ | ---------------------------------- | -------------------------------------------------------------------- |
| Code per card            | XMage (Java), Arcana (Rust `fn`)   | Arbitrary logic, opaque; `fn`/`dyn` blocks hashing and serialization |
| Text DSL interpreted     | Forge, DeepScry, gorge             | Data, analyzable; needs engine escape hatches                        |
| Oracle text → AST        | phase.rs, Ironsmith, MTG Arena GRE | Scales to every card; parse coverage ≠ correctness                   |
| Compiled to tables / ops | dirac (`kgen` → `kop`)             | Fast, state stays plain data                                         |

**Lean:** a cube is about 540 cards, so hand-verified card definitions are
feasible. They should be **data**: an enum tree or bytecode, compiled into
read-only tables. Never use `fn` pointers or `dyn` objects inside state.

**Bootstrapping options** (license notes in §8):

- Use phase.rs's parser (MIT/Apache) offline to draft the definitions.
- Use XMage (MIT) as the decomposition reference.
- Use Forge scripts only as a reference, unless GPL is acceptable.

### 6.4 Decision interface and branching factor

**Every successful search system breaks compound decisions into factored single
picks:**

- Cowling's binary card-subset tree.
- Hearthstone's card → position → target.
- dirac's 8 single-pick decision kinds.
- Ironsmith's cast → targets → modes → payment.

**The anti-patterns:**

- Arcana's monolithic `CastSpell`.
- phase.rs's whole-attack `DeclareAttackers`.
- Power-set combat enumeration capped at 1024.

**Lean:**

- **The engine is a state machine.** `next_decision(&State) -> Decision` and
  `apply(&mut State, Choice)`, with the pending decision stored *inside* the
  state.
- **The engine never calls the AI.** DeepScry's and Ironsmith's callback models
  force replay to get back into a decision.
- **Mana payment is automatic or planned by the engine** by default, with manual
  payment only where it matters strategically. Note dirac's "casts are offered
  when untapped sources can pay".
- **Attackers and blockers are declared one creature at a time**, as DeepScry
  does in its blockers overhaul.
- **Forced and single-option decisions resolve automatically.**
- **For neural policies**, use fixed action slots whose meaning is carried in
  the observation (manabot), or a masked flat action space (MTG-Causal-RL).

### 6.5 Determinism and parallelism

**The pattern every engine follows:**

- A single serialized RNG, or split streams: DeepScry keeps the shuffle seed
  separate, and phase.rs tags draws as placement versus outcome.
- No `std` `RandomState` hash maps. Arcana enforces this with a test that greps
  the source; phase.rs was bitten by it.
- No wall-clock time.
- No global or thread-local game state. Ironsmith's global ID counters, and the
  static caches in Forge's and XMage's AI, all block running parallel games in
  one process.

**A per-decision state hash** (DeepScry) is cheap and makes divergences
debuggable.

**Two hashes for search:**

- **A full-state Zobrist hash**, for transpositions within one determinized
  world.
- **An observation hash** (public state plus the searcher's private
  information), for ISMCTS nodes and reusing trees across re-determinizations.

### 6.6 Hidden information

**What the engines show:**

- **Public stable IDs with per-viewer reveal masks** (DeepScry), plus
  **library-knowledge groups** for partially known orderings (Ironsmith).
  Forge's `HiddenInfoShuffler` is the matching determinization step: re-deal
  only the unknown cards into the same slots.
- **Leak checklist** (Arcana): the RNG seed, the ID allocation order, the real
  library order, and any API that returns the opponent's hidden zones
  (DeepScry's `player_hand(opponent)`).
- **Perfect-information shortcuts are common in shipped AIs** and inflate
  measured strength:
  - every phase.rs tier;
  - XMage's Mad AI (likely);
  - Magarena's "cheating" mode.

**Lean:**

- Agents receive a type-level `Observation`, not `&GameState`.
- CI runs DeepScry's information-independence test: the same decisions from the
  full state and from a re-determinized state.

### 6.7 Derived characteristics (layers)

**What the engines do:**

- Forge, XMage and Arcana recompute continuous effects from scratch, repeatedly.
  Arcana does it with no cache at all.
- phase.rs writes layer results onto objects, which defeats its copy-on-write
  sharing.
- Dependency handling varies:
  - Ironsmith resolves CR 613.8 by simulation.
  - phase.rs uses Tarjan's strongly connected components algorithm.
  - Arcana uses timestamps only.

**Lean:**

- Keep **base** and **derived** characteristics separate.
- Store derived values in compact arrays inside the flat state.
- Invalidate with an epoch or dirty bit, through write chokepoints. DeepScry's
  caching invariant is the model.
- For a cube, cards with continuous effects are a known, small set, so a cheap
  full recompute over compact data may beat incremental schemes. This needs
  benchmarking.

### 6.8 History-dependent rules

Some rules depend on history, for example "if a creature died this turn" or
"cards that left your graveyard this turn".

**What the engines do:**

- DeepScry derives some of these facts from its undo log and loses them during
  search.
- XMage uses watchers.

**Lean:** every fact a rule reads lives in explicit, bounded, per-turn state
counters. Never derive one from a log.

### 6.9 Correctness strategy

Speed means nothing if the rules are wrong. Approaches seen:

- **Lockstep against a reference engine.** dirac against gorge, with 0
  divergences. A card is "supported" only once its abilities are exercised.
- **Differential play against an existing engine.** mtg-kernel bridges to XMage,
  and Ironsmith ported 1,744 XMage tests.
- **Puzzle DSL with event-log assertions and golden logs** (DeepScry).
- **CR-cited scenario tests** (phase.rs: about 45k tests).
- **Behavioral no-op probes** for card definitions (Arcana).
- **Fuzzing with invariant checks** (kiln, DeepScry).
- **Logical-operation perf-counter gates** in CI (phase.rs).

**Lean:**

- Beryl should plan differential testing against XMage (MIT) or Forge from day
  one.
- Track **ns per copy, ns per transition, decisions/s, rollouts/s and bytes per
  state** as first-class CI metrics.

______________________________________________________________________

## 7. Recommendations for Beryl

These are inferences from the evidence above. Each should be checked against
benchmarks.

1. **Write a new engine. Don't fork one.** No existing Rust engine has a state
   representation compatible with hyper-optimized search, and every one would
   need its core rewritten.

2. **Target dirac-class performance or better in Rust.** Suggested starting
   targets on an M-series P-core:

   | Metric           | Target               |
   | ---------------- | -------------------- |
   | State size       | ≤ 16 KiB             |
   | Copy             | ≤ 100 ns             |
   | Transition       | ≤ 2 µs               |
   | Random cube game | ≥ 2,000 games/core-s |

   dirac reaches about 1,800 games/core-s; its 4.5 µs per decision means about
   10⁵–10⁶ search steps per decision per core in a 1 s budget.

3. **Use a flat, `Copy`, fixed-capacity state with explicit stall/draw
   semantics**, as dirac does, plus immutable card tables compiled from a
   card-definition DSL.

4. **Make the engine a state machine with factored single-pick decisions**, with
   mana planned automatically by default and combat declared one creature at a
   time.

5. **Make determinism a tested invariant:** a single RNG, no `RandomState`, no
   globals, and a state hash per decision.

6. **Build hidden information in from the start:** public slot IDs, reveal
   masks, library-knowledge constraints, a type-level observation API, and an
   information-independence test in CI.

7. **Start search with ensemble determinized MCTS or PIMC at the root.** Add
   learned belief sampling and value networks next. Treat ISMCTS as an
   experiment, not the default.

8. **Spend early on correctness infrastructure:**

   - a puzzle/scenario DSL;
   - differential testing against XMage or Forge;
   - a "supported card" bar based on exercised abilities;
   - perf-counter gates.

9. **Implement the full rules subset the cube needs, correctly**: the stack,
   APNAP trigger ordering, replacement effects, and layers with dependencies.
   DeepScry's trigger shortcut shows how costly retrofits become.

______________________________________________________________________

## 8. Licensing summary

| Source                        | License                                                       | Implication for Beryl (inference, not legal advice)                                                   |
| ----------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| XMage                         | MIT                                                           | Code and tests can be studied, translated or bridged freely, with attribution.                        |
| phase.rs                      | MIT / Apache-2.0                                              | Parser/AST could be reused, e.g. as an offline card-drafting tool.                                    |
| Arcana                        | MIT                                                           | Free to reuse; little worth reusing beyond ideas.                                                     |
| gorge / dirac                 | Apache-2.0                                                    | The code is permissive, but its card IR is compiled from Forge's GPL scripts.                         |
| Forge                         | GPL-3.0                                                       | Mechanically translated card scripts are likely derivative works. It matters if Beryl is distributed. |
| Magarena                      | GPL-3.0                                                       | Ideas only.                                                                                           |
| Ironsmith                     | BSL 1.1 (non-commercial; MIT after 4 yrs) / AGPL in manifests | Ideas only.                                                                                           |
| DeepScry                      | Closed (card mirror GPL-3.0)                                  | Ideas from the published docs only.                                                                   |
| 17lands data (via XMage CSVs) | CC BY 4.0                                                     | Usable with attribution. 17lands has no cube data.                                                    |

______________________________________________________________________

## 9. Note on drafting

This is outside the engine scope, but the engine survey touched it:

- Forge and XMage draft bots are greedy pick-order bots with color commitment.
  They work only as baselines.
- Recent research uses learned card representations:
  - Bertram et al.'s Contextual Preference Ranking
    ([2105.11864](https://arxiv.org/abs/2105.11864));
  - their generalization to unseen cards
    ([2407.05879](https://arxiv.org/abs/2407.05879));
  - DraftFM, trained on 149M picks, which reaches 51–60% top-1 accuracy on
    held-out sets ([2608.19568](https://arxiv.org/abs/2608.19568)).
- The `mulligan` project found simulated bot games rank cards reasonably
  (Spearman +0.62) but rank decks poorly (about 0).
  - Inference: Beryl's play strength limits how well it can evaluate draft picks
    by simulation, which is one more reason the engine must be fast enough for
    strong search.

______________________________________________________________________

## 10. Open questions

**About DeepScry:**

- Its MCTS details: selection rule, how it determinizes, rollout versus
  evaluation, and whether it searches combat, targeting and mana.

- Its state layout and clone cost.

- What `is_hits` means.

  A direct contact may be worthwhile, since the author is publicly identifiable.

**Benchmarks we could not find:** no clean head-to-head numbers exist for
persistent copy-on-write states versus flat-copy states on a realistic MTG
state. A small prototype benchmark would settle Beryl's choice.

**Design questions for Beryl:**

- **How much rules surface does a typical cube need?** Copy effects, control
  change, layer dependencies, and double-faced or adventure cards stress a
  fixed-capacity design.
- **One engine or two?** Either a fast search engine plus a broad reference
  engine (dirac/gorge), or one engine plus external differential testing.
- **Belief modeling in cube**, where the opponent's deck is drafted and
  partially observed during the draft. This is richer than Constructed, and
  little studied.

______________________________________________________________________

## References

### Primary engines

- DeepScry: https://deepscry.net/ · guide: https://deepscry.net/guide/print.html
  · bugs: https://github.com/DeepScryAI/DeepScry_bugs · card mirror:
  https://github.com/DeepScryAI/CardScriptsMirror
- gorge/dirac: https://github.com/adams-shaun/gorge ·
  https://adams-shaun.github.io/gorge/architecture/dirac/ ·
  https://adams-shaun.github.io/gorge/performance/
- Arcana: https://github.com/levineuwirth/arcana
- phase.rs: https://github.com/phase-rs/phase
- Ironsmith: https://github.com/Chiplis/ironsmith
- Forge: https://github.com/Card-Forge/forge · Flat MC PR:
  https://github.com/Card-Forge/forge/pull/12308
- XMage: https://github.com/magefree/mage · perf discussion:
  https://github.com/magefree/mage/issues/9619
- Magarena: https://github.com/magarena/magarena

### Other engines

- MTG-Ichor https://github.com/lcmaier/MTG-Ichor
- mtg-kernel https://github.com/jackmaiorino/mtg-kernel
- kiln-mtg https://github.com/SeanMcKeen/kiln-mtg
- Argentum https://github.com/wingedsheep/argentum-engine
- managym https://github.com/jacklionheart/managym
- MageZero https://github.com/toastblademaster-arch/mtg-rl
- Wagic https://github.com/WagicProject/wagic
- Parun https://github.com/parun-mtg/parun
- MTG Arena GRE blog
  https://magic.wizards.com/en/news/mtg-arena/on-whiteboards-naps-and-living-breakthrough

### Papers

- Cowling, Ward, Powley 2012, Ensemble Determinization in MCTS for MTG:
  https://eprints.whiterose.ac.uk/75050/1/EnsDetMagic.pdf
- Cowling, Powley, Whitehouse 2012, Information Set MCTS:
  https://eprints.whiterose.ac.uk/75048/1/CowlingPowleyWhitehouse2012.pdf
- Long et al. 2010, Understanding the Success of PIMC:
  https://ojs.aaai.org/index.php/AAAI/article/view/7562
- Extended PIMC 2024: https://arxiv.org/abs/2408.02380
- Churchill, Biderman, Herrick, MTG is Turing Complete:
  https://arxiv.org/abs/1904.09828
- Biderman 2020, MTG is as hard as arithmetic: https://arxiv.org/abs/2003.05119
- Rubin 2026, LOCM search with a Rust engine: https://arxiv.org/abs/2609.06816
- Świechowski et al. 2018, Hearthstone MCTS + NN:
  https://arxiv.org/abs/1808.04794
- ReBeL: https://arxiv.org/abs/2007.13544 · Student of Games:
  https://arxiv.org/abs/2112.03178 · SPARTA: https://arxiv.org/abs/1912.02318 ·
  DeepNash: https://arxiv.org/abs/2206.15378 · Ataraxos:
  https://arxiv.org/abs/2511.07312
- MTG-Causal-RL: https://arxiv.org/abs/2605.06066
- Draft: Ward et al. https://arxiv.org/abs/2009.00655 · Bertram et al.
  https://arxiv.org/abs/2105.11864, https://arxiv.org/abs/2407.05879 · DraftFM
  https://arxiv.org/abs/2608.19568

### Rust techniques

- Catherine West, RustConf 2018 (ECS / generational indices):
  https://kyren.github.io/2018/09/14/rustconf-talk.html
- arrayvec https://crates.io/crates/arrayvec · smallvec
  https://crates.io/crates/smallvec · fixedbitset
  https://crates.io/crates/fixedbitset · slotmap https://docs.rs/slotmap · imbl
  https://crates.io/crates/imbl · rpds https://github.com/orium/rpds
- Zobrist hashing: https://www.chessprogramming.org/Zobrist_Hashing
