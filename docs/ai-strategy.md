# AI Strategy

*The plan, as of 2026-11-08, for Beryl's AI opponent: what it must be, which
agent we start from, and how each change to it earns its place. It replaces the
2026-11-05 plan, which asked how search scales and what limits it; the
long-think arena answered that ([longthink-run-003.md](longthink-run-003.md)),
and five games against humans showed what it did not measure
([incidents/](incidents/)).*

## 1. Goal

A strong opponent that **does not cheat** and **plays like a plausible human**.

- **Fair.** It sees what a player sees (`Sight::Hidden`). The cheating searches
  stay in the registry, labeled, as yardsticks; none is ever the opponent we
  improve.
- **No apparent blunders.** A debatable play is fine: Snapcaster Mage as a 2/1
  on turn two is sometimes right, and Reprieve on an Oust the opponent can
  recast was sound ([incident](incidents/2026-11-08-reprieve-on-oust.md)). A
  play no strong player would make under any reading is not, whatever the game's
  result:
  - Oust on its own Rogue token
    ([Deep Seer](incidents/2026-11-06-deep-seer-misplays.md));
  - Snapcaster Mage in its own main phase, flashing back a Counterspell it could
    never pay for
    ([Snapcaster](incidents/2026-11-07-snapcaster-and-jace-wielder.md));
  - Trinket Mage's search offered a free Mana Crypt and took nothing
    ([Trinket Mage](incidents/2026-11-07-trinket-mage-declines-crypt.md));
  - the Spirit token held back when attacking Jace, Wielder of Mysteries with
    everything killed the opponent's only quick win (Snapcaster incident).
- **Blunders weigh more than strength.** We will trade some Elo for the last of
  them, and we hold the line hardest where search is least reliable: positions
  the agent rates as nearly won or nearly lost, where every move looks the same
  to its rollouts and noise chooses.
- **General by data, not by code.** Per-card knowledge is allowed as table
  entries (`values.rs`: a value, a flag) that the policy and search read
  generically. Card-specific code paths are not: the expert is 3,200 lines for
  29 cards, and the Legacy decks will have hundreds.

## 2. What we learned

From run-003 (2,777 games over 54 hours) and the incidents:

- **Full rollouts win.** Leaves cut off and evaluated by
  `beryl_search::evaluate` trailed full rollouts by 180 to 260 Elo at every
  budget, and sat at the frozen expert's level. The general evaluator the old
  plan was built on is not a leaf yet.
- **Budget pays, less and less.** Each ×3 of engine steps a decision gained +23
  to +57 Elo, flattening past 10M to 30M. The 90M entrants were within noise of
  30M.
- **Hidden information costs little at length.** The omniscient twins led the
  fair searches by +65 to +160 Elo at 1M to 10M, and by nothing measurable at
  30M.
- **Hand-tuned card values beat 17lands win rates** by about 90 Elo, at every
  budget.
- **Fewer worlds, deeper trees.** PIMC with 18 worlds rated +356 at 30M, 72
  worlds +262 and 288 worlds +295, within wide intervals.
- **Most moves are coin flips by the search's own estimate.** The chosen move
  led the runner-up's mean by under 0.01 in 62% of the leader's decisions and
  under 0.02 in 73%. Every incident blunder was one of these ties, settled by
  visit counts, which is noise. Visits can even overrule the means: Jace,
  Wielder led by 0.015 and lost on visits.
- **The rollout policy is where the blunders start.** The search prices a move
  by games the greedy policy plays out, so what greedy cannot do the search
  cannot value: holding a flash creature, milling oneself with Jace, Wielder,
  casting Mana Crypt, or a Mana Crypt in hand that the tree casts badly and the
  rollouts never cast. Fixes there are a line each.
- **The anchor works where it applies.** Playing greedy's answer unless the
  search leads it clearly (`Options::anchor`, epic by-7wl) stopped the Oust and
  costs about 0.005 a game by the search's estimate. It guards only priority
  casts; three of the five incidents fell outside it.
- **Round robins and the arena cannot answer today's questions.** After 54 hours
  the arena's intervals were ±60 to ±80 Elo, and a 600-game head-to-head
  (by-7wl.8) was dropped as powerless against an effect of half a percent. The
  questions left are small effects between two agents, which take a different
  design.

## 3. The champion

The ladder improves one agent, the **champion**. The app's Deep Searcher is
always the current champion, and a promotion is a change to it.

The starting champion is Deep Searcher as it ships:
`pimc72/hidden/rollout/tuned/anchor@30M` (`beryl_search::AGENTS`).

| Axis      | Setting                                                   |
| --------- | --------------------------------------------------------- |
| Method    | PIMC, 72 worlds dealt from what it can see, a tree each   |
| Sight     | Hidden                                                    |
| Leaf      | Softmax rollout to the end of the game, temperature 10    |
| Knowledge | Hand-tuned card values (`Knowledge::Tuned`)               |
| Options   | `anchor`: greedy's answer unless the search leads clearly |
| Scores    | Turn discount 0.98: win sooner, lose later                |
| Budget    | 30M engine steps a decision, on every core                |

Why this one and not run-003's best fair entrant (18 worlds, +356): it is what
humans have played, every incident was measured against it, and the 18-world
twin's lead is within noise. "Fewer worlds" is the cheapest rung on the ladder
(§6), not an assumption.

Yardsticks, never candidates: `expert-frozen` (the fixed bar of the old plan,
pinned by a test), `greedy` (the rollout policy as a player) and Deep Seer (the
champion's omniscient twin, an upper reference for what search could know).

## 4. The ladder

One challenger at a time plays the champion. The challenger differs in one thing
we can name. It is promoted only by a pre-registered bar, and the match's result
is recorded whether it passes or not.

### 4.1 The match

- **Paired seeds, seats swapped.** Every seed is played twice, each agent in
  each seat (`beryl_cli tournament --paired`). The unit of evidence is the
  seed's pair: swept, split or lost. A seed on which both agents play the same
  moves splits 1–1 and carries no information, which is right: it is a seed the
  change did not touch.
- **Curated start positions.** Most games start from a fixed set of mid-game
  positions rather than the shuffle, to take the openings' luck out of the
  variance: positions cut at the start of a turn, before the draw, from seeded
  champion games, kept where the champion's own estimate from each seat lies
  within 0.45 to 0.55, then a sample read by a human. The hidden cards are
  reshuffled by the game's seed below what either player knows. A fifth of each
  match still starts from the shuffle, so mulligans and openings stay tested.
  The first match measures what the positions buy, comparing the spread of
  paired scores from positions against the shuffle; if they buy nothing, they
  go.
- **The proxy budget.** Matches run at 3M engine steps a decision, one thread a
  game, a game a hardware thread at a time (18 on the Mac, 20 on the Windows
  machine): roughly 20 to 30 s of wall clock a game. Run-003's families kept
  their order from 3M to 30M, which is the assumption the confirmation step
  checks.
- **Confirmation at 10M.** A challenger that passes at 3M plays 400 games at 10M
  (about 8 hours). Its Wilson 95% interval must clear 50%. A pass is shipped at
  30M.
- **The shipped budget** stays 30M. It rises to 90M only when a budget rung (§6)
  shows strong evidence, since run-003 saw little past 30M.

### 4.2 The bars

A sequential test (SPRT, as Stockfish's fishtest) ends each match as soon as the
evidence is in, and a cap ends it undecided. The bars are set before a match and
never moved during one.

- **Stronger.** SPRT between "no gain" and "+50 Elo" at α = β = 0.001, and the
  suite (§5) re-opens no fixed entry. Promoted when the test accepts the gain: a
  challenger no stronger than the champion passes one match in a thousand. About
  700 games end it either way at +50 Elo, or about 5 hours at 3M. Cap: 2,000
  games (an overnight run). A challenger worth +25 Elo will usually hit the cap;
  it is not promoted, and its score is kept for the record. +25 Elo at this
  confidence costs about 2,700 games, which is more than one night, and the
  gains we have seen are not that small when they are real.

- **More plausible at the same strength.** For a challenger that fixes blunders,
  two conditions:

  1. The blunder suite (§5): it fixes at least one open entry its rung claimed
     and re-opens no fixed one.
  2. The match shows it is not weaker: SPRT between "−25 Elo" and "no change" at
     α = β = 0.05, accepting "no change". About 1,100 games, or 8 hours at 3M.
     Cap 2,000.

  The lower confidence is deliberate: we would trade a little strength for a
  blunder, and most such changes touch few moves a game, so the match's power is
  low by construction.

- **A screen before the match.** A challenger that changes few moves is first
  replayed over the champion's last match games: at each decision where it would
  choose differently, both candidates are searched again from fresh seeds with
  the whole budget split between them (`LongSearch::duel`, as
  `beryl_cli anchor-replay --recheck` does) and the cost is summed by the
  search's own estimate. A screen showing a cost above 0.03 a game does not stop
  the match, but it is written down beside the result. The screen costs minutes;
  the match hours.

### 4.3 The record

- Every match is a bead labeled `lane:ladder`, holding its pre-registered bar,
  the command line, the seed, and the result in its notes. One lane, so matches
  run one at a time and never share the machine.
- `docs/ladder.md`, created with the first match, is the log: one row a match
  (date, challenger, champion, budget, games, score, Elo interval, verdict, and
  the suite's open and fixed counts after it), newest first, and the current
  champion at the top. Logs, transcripts and position files are never committed.
- A promotion changes the registry's Deep Searcher and its doc comment names the
  match.

### 4.4 What we build

Each is its own bead; name the files.

- `just ladder <challenger> [--vs <champion>] [--work 3M] [--cap N]`: a paired
  match from the position set with the SPRT, printing a row for `ladder.md`.
  Builds on `beryl_cli tournament --paired` (`tournament.rs`), long-think
  entrant names (`game.rs`, `family@budget` with options) and a trinomial SPRT
  over paired seeds. Runs at utility QoS as `just tournament` does.
- The position set: cut by a `beryl_cli positions` from seeded champion games
  into the save format (`beryl_session::save`, which the frozen corpus already
  migrates), with each file's source seed and turn, under
  `$BERYL_CACHE/ladder/positions`. A fixed seed rebuilds it.
- `just blunders [agent]`: the suite (§5).
- The screen: `beryl_cli anchor-replay`'s `--recheck`, generalized from "the
  anchor's answer" to "another entrant's answer".

## 5. The blunder suite

A regression gate built from every blunder a human has caught.

- **An entry** is a position (an incident's replay file and a decision's log
  index, `docs/incidents/*.replay.json`), the forbidden choices and, where one
  exists, the required choice. The list is `data/blunders.json`, each entry
  citing its incident. The first entries:
  - Deep Seer, turn 14: Oust on its own token is forbidden.
  - Snapcaster, turn 12: Snapcaster Mage in the agent's own main phase is
    forbidden.
  - Snapcaster, turn 16: attacking Jace, Wielder of Mysteries with both
    creatures is required.
  - Trinket Mage, turn 7: taking Mana Crypt is required.
- **The check** builds the agent from the game's seed as the app did, searches
  the position at the proxy budget and at the shipped budget, from five seeds
  each, and fails on any forbidden play or any missed required one. It runs
  before every match, for both bars.
- **Open and fixed.** The champion made every play the suite forbids, so a new
  entry fails for it by construction. An entry is *open* while the current
  champion fails it at any seed or budget, and *fixed* once it passes at all of
  them; the suite's report says which is which. The suite is a ledger of known
  blunders, not a score the champion is expected to clear: at any time some
  entries are open, and those are what the next rungs are for. The gate reads
  the ledger. A plausibility promotion fixes at least one open entry its rung
  claimed and re-opens none; a strength promotion re-opens none. A fixed entry
  re-opening is the regression the suite exists to catch, whatever the Elo
  beside it.
- **Growth changes no past verdict.** A match's statistics are about Elo and
  never read the suite, and a promotion was judged by the suite as it stood when
  its bar was registered: an entry added while a match runs is reported with the
  result, not made a condition of it. So a new entry re-decides nothing. It says
  the current champion has a blunder we did not know of, which every promotion
  before it also said. A plausibility promotion is relative, better than the
  last champion on what we knew, never a certificate.
- **Every new entry has a rung.** Its write-up names the class of the blunder,
  not only the position: Snapcaster Mage in its own main phase is "a flash
  creature cast at sorcery speed", and a Samurai token made in the agent's own
  turn instead of at the opponent's end step (by-s0t) is the same class. A new
  entry joins the rung that covers its class or opens a new rung, whose bar
  claims it. The fix must address the class. A table entry that moves one card's
  value in one spot passes the entry and leaves the class, and the human's next
  games find the class again; entries caught after a fix landed are that fix's
  held-out test.
- **The measure over time is entries opened per human game.** Discovery comes
  from humans playing the champion, so the suite grows with play, and the log
  records with each promotion the suite's open and fixed counts and the human
  games played since the last one. Progress is open entries closing and new ones
  arriving more slowly. A champion that opens an entry every few games is not
  plausible yet, whatever its Elo.
- **Cost grows with the ledger.** An entry costs about a minute and a half at
  both budgets and five seeds. Past about 30 entries, every entry runs at the
  proxy budget, and the shipped budget runs only on the entries a rung claims
  and on any whose state changed at the proxy budget.
- **The human's games.** After each promotion, a human plays a few games against
  the new champion. Any play they would call wrong goes through the bug button
  (⌘B), whose report carries the replay and the agent's own searches, gets an
  incident write-up, and becomes a suite entry. A replay-reviewed incident takes
  an hour of `just review --agent recorded`; the suite entry takes a line.
- **Drift.** A replay from before an engine change may stop reproducing its
  position (`just review` fails on it, as the Snapcaster replay's old undo did).
  An entry that no longer replays is rebuilt or retired with a note, never
  silently skipped. The first four entries' replays had drifted before the suite
  existed: f3f4baeb changed which lands a payment taps, so their games go
  elsewhere on every later build. They are rebuilt boards
  (`data/blunders/*.json`): the game at the blunder turn's first priority, cut
  on cae2787d, the last build that replayed them, with the record's answers from
  there.

## 6. The first rungs

Ten challengers, in the order we would play them. Each names its change, its
evidence, whether it aims at strength (S) or blunders (B), and what would make
us drop it. Each is one setting (`Options`, `Settings`) or one table entry away
from the champion, so a loser is dropped by not setting it.

01. **Means over visits at near-ties** (B, S). `leader` (`long.rs`) and `Mcts`
    play the most visited candidate. Play the best tree-averaged mean
    (`Search::means`) instead when the two differ, or the most visited unless
    another's mean leads it by more than the noise across trees. Evidence: Jace,
    Wielder led by 0.015 and lost on visits; the Searcher mulligan showed the
    vote and the expectation disagree at 9% of hidden decisions. Cost: one
    function. Drop it if the match loses: a losing candidate's mean within a
    tree rests on few visits, which is the case for the vote.

02. **Anchor at combat and finds** (B). `guarded` covers only a priority
    decision where greedy passes and the search casts. Extend it to `Attacker`
    and `AttackTarget` at near-ties, and to a search's find (`ChooseUpTo` under
    `Ask::Search`), where a card to hand is never worse than nothing. Evidence:
    the Spirit held back and the Crypt declined, both outside the anchor. Risk:
    by-7wl.1 put a broad anchor at 0.04 to 0.07 a game by the search's biased
    estimate; the screen and the match measure it properly.

03. **Decided positions default to greedy** (B). Where the root's estimate is
    above 0.9 or below 0.1, widen the anchor to every decision kind and raise
    its margin. Evidence: 45% of the leader's decisions at 10M came in such
    positions; the turn discount lifted seed agreement there from 60% to 78%,
    against 90% in contested positions, so a fifth of these moves still turn on
    noise. A human closing out a won game makes the obvious play; so should the
    champion.

04. **Rollout common sense, from card data** (B, S). One challenger bundling
    four table-and-policy fixes, each a line or two in `rollout.rs` and
    `values.rs`, each checked by the suite on its own:

    - a creature with flash waits in its controller's main phase and is cast at
      the opponent's end step (read from `Keywords`, not a card name);
    - Jace, Wielder of Mysteries's +1 is `FRIENDLY`, and the card is valued as
      the expert's `appraise` does when its controller's library is short;
    - Mana Crypt is castable while short of lands, as the expert's `CRYPT_LANDS`
      says, and the tree and the rollouts agree on it (a never-cast card is
      pruned from the tree too);
    - a counterspell or Reprieve answers only a spell its caster cannot recast
      this turn, read from their untapped mana.

    The suite tests each fix; the match tests the bundle; a loss is bisected.

05. **Greedy answers mid-resolution choices below the root** (S, B).
    `Options::mid` exists and was never measured head-to-head. Evidence: Jace's
    0 scored 0.498 against the +2's 0.782 because its put-back was a tree node
    of random children; in the Deep Seer re-search, `mid` changed 16 of 62
    moves.

06. **18 worlds** (S). `Method::Pimc { worlds: 18 }`. Evidence: +356 against
    +262 at 30M in run-003. Cost: a number. If it wins, rung 7 must answer the
    mulligan incident's "18 worlds are few".

07. **Once-a-game draws over many worlds** (B, S). Mulligans, and any decision
    whose value is mostly its world (the trees scored Keep 0.000 or above 0.95),
    searched by `Method::Ismcts` or 288 worlds at the same budget, the rest of
    the game as before.

08. **Children tried in greedy's order** (S). `Tree::select` plays each new
    child once in random order before UCB1 compares them, so a node's mean
    averages its exploration. Order expansion by the rollout policy's scores, so
    the sensible options are tried first and a wide choice is priced by its best
    answers. This is `mid` for the decisions `mid` leaves to the tree.

09. **Turn discount 0.95** (B). The discount is 0.98 by the agreement
    measurement alone: 0.95 agreed in 80% and 90% of positions against 78% and
    77%, at a cost in close games nobody measured. The ladder measures it.

10. **The budget rung** (S). Champion at 90M against itself at 30M, 300 paired
    games at those budgets, about a day. Run only when the cheaper rungs are
    exhausted or the engine has sped up enough that 90M costs what 30M does
    today.

## 7. Speed

Engine speed is a multiplier on every rung: strength rose with steps up to 10M
to 30M, and the ladder's cost is wall clock.

- The cube rollout benchmark and `count-gate.toml` stay the measure
  ([benchmarking.md](benchmarking.md)).
- A speed-up that keeps every answer (`rollouts_never_change` pinned) changes no
  move, so it needs no ladder match: it ships on the fast check.
- A speed-up that moves answers is a challenger like any other.

## 8. Not now

- **The long-think arena is retired.** `just longthink` and its runner stay
  buildable, and run-003's data stays the record of how search scales. No new
  run unless a question needs many families rated at once.
- **Round robins.** `just tournament` remains for a quick table; nothing is
  decided by one.
- **Learned value models, AlphaZero-style or R-NaD training.** Evaluation leaves
  lost by 180 to 260 Elo here, and the record elsewhere stands. In Monolith, our
  Netrunner prototype, R-NaD learned nothing in 145,000 self-play games, and a
  learned value model was neutral as a correction to the evaluator and harmful
  on one side (p = 0.002), trained on games from a bot that always made the
  obvious play. In netrunner-rs, 0 of 6 AlphaZero-style runs were promoted, and
  a well-calibrated value head made search worse. Good calibration is not
  evidence.
- **Evaluation leaves**, until something changes what run-003 measured. The
  17lands card values it measured are gone with the cube.
- **ISMCTS beyond rung 7.** PIMC with few worlds led it in the hidden game.
- **Card-specific code.** Table entries only (§1).
- **Decks beyond the WU Control mirror.** The project's target is now Legacy
  constructed, but the engine covers only the mirror's cards
  ([rules-coverage.md](rules-coverage.md)), so the mirror stays the ladder's
  testbed. The ladder moves to Legacy metagame decks as the engine covers them,
  and the position set grows a family per deck.

## 9. Open questions

- Do curated start positions cut the variance of paired scores enough to pay for
  building them, against paired seeds from the shuffle alone?
- Does a gain at 3M transfer to 30M for changes to the root choice (rungs 1 to
  3), which fire on margins that shrink as the budget grows?
- How far can the anchor widen before the champion plays greedy's game? The
  rungs 2 and 3 matches put a number on it.
- What does a plausibility promotion cost in Elo over a year of them? The log
  answers that, if every match is recorded.
