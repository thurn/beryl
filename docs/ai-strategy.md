# AI Strategy

*Our best guess, as of 2026-11-05, at how Beryl gets a general game-playing AI
that is stronger than its expert system, and how we would know. It builds on
[engine-architecture.md](engine-architecture.md) §19.1 and the evidence in
[rules-engine-research.md](rules-engine-research.md).*

## 1. Goal

A **general** AI: no per-card code, able to play any deck drafted from the
540-card cube.

- In the WU Control mirror, it beats the frozen `expert`, which has hand-written
  knowledge of every card in that deck.
- On decks with no expert, it beats `greedy` by more than the expert beats
  `greedy` in the mirror.
- The long-term target is a strong human player. The expert is a milestone, not
  the destination.

## 2. Where we are

From the Milestone 1 round robins on epic by-b0q, in the earlier MTGO mirror
(`decks::VINTAGE_WU_CONTROL`, whose 29 cards the expert knows; 100 games per
pairing, seats alternated, MCTS at 300 iterations and 4 trees):

| Pairing                       | Seed 2026 | Fresh seeds    |
| ----------------------------- | --------- | -------------- |
| expert v mcts-hidden          | 88%       | 89% [81, 94]   |
| expert v mcts-omniscient      | 88%       | 91% [84, 95]   |
| mcts-omniscient v mcts-hidden | 52%       | 50% [40, 60]   |
| expert v greedy               | 79%       | 80% [71, 87]   |
| mcts-hidden v greedy          | 67%       | 72% [62.5, 80] |

What this says:

- **MCTS is barely searching.** The 300 iterations are shared by 4 trees, so
  each tree gets 75, and each iteration is a noisy full-game rollout.
- **Hidden information is not the bottleneck yet.** Seeing every hidden card
  gains MCTS nothing.
- **It is one budget point.** It says nothing about how MCTS scales.
- **The bar is lower than 88% suggests.** The expert beats `greedy` only about
  80%, and transcript reviews find a blunder in about 3 of 4 fresh games
  (by-ovn).
- **The expert does not scale.** It is about 3,200 lines for 29 cards. The cube
  has 540.

## 3. What other projects tell us

**Monolith**, our Netrunner AI prototype on the Jinteki.net engine:

- Its turn planner beat MCTS, and MCTS ranked below the plain heuristic.
- More search did nothing: a bigger beam and twice the engine budget was a null
  (p = 1.0). The evaluator was the bottleneck.
- The winning agent ran the heuristic by default. The planner overrode it only
  when its line won by a margin, and rollouts reranked the top 3 lines.
- R-NaD learned nothing in 145,000 self-play games.
- A learned value model was neutral as a correction to the evaluator. On one
  side it was harmful (p = 0.002). Its training games came from a bot that
  always made the obvious play, so the data had no contrast.
- The planner farmed any overvalued feature. New evaluator terms need traces
  before A/Bs.
- Reviews by a language model plateaued near 3.3 of 5, with noise of about ±0.3
  per 10 games. Decks mined for fixes over-predicted fresh decks by 0.3–0.65.

**netrunner-rs**: its turn planner beat MCTS 0.84 and 0.95 by side, and 0 of 6
AlphaZero-style runs were promoted. A well-calibrated value head made search
worse.

**Cowling, Ward & Powley (2012)**, in simplified Magic: MCTS reached 50.5%
against an expert player, using many shallow determinizations, yes/no splitting
of multi-card choices, pruning of dominated moves and weak rollout policies.

**DeepScry** claims MCTS beats its heuristic 71%, but the evidence is weak:

- It documents nothing about its MCTS.
- Almost every arena game is one deck's mirror.
- Random beats its MCTS 30–0 in the arena data.
- 1,000 ms MCTS scores the same against the heuristic as 300 ms (71.4% v 71.2%),
  even though it beats 300 ms MCTS 78.5%. Self-play gains need not show up
  against a fixed opponent.

## 4. The bet

**Shallow determinized search over a strong general evaluator.**

- **Search** looks ahead to the end of the turn or into the next one, across
  many sampled hidden hands, and searches the opponent's replies one level deep.
- **The evaluator** reads card data, never card names: life, cards, board, mana,
  plus a quality score for each card.
- **17lands supplies what nobody else had.** For this cube there are about
  266,000 PremierDraft games:
  - Per-card win rates give every cube card a quality score.
  - The replay files have an end-of-turn snapshot of every turn (hand, both
    boards, life, mana spent), labeled with the winner. That is millions of
    positions from human games, with real contrast. They lack targets, timing
    inside a turn, counters, tapped status and loyalty, so they train a value
    estimate, not a policy.
- **Most of the expert's card knowledge is something search should find.** Strip
  Mine targets and planeswalker abilities are lookahead plus evaluation. Saving
  a counterspell for a bigger threat is sampling the opponent's hand.

## 5. How we get there

1. **Find what limits MCTS (days).** Play `mcts-hidden` against the expert at
   300, 1,200 and 4,800 iterations, about 50 games each. At a fixed budget, also
   try more trees with fewer iterations, and rollouts cut off after a few turns
   and scored by a simple evaluation. If strength climbs with budget, speed is
   the lever. If it is flat, rollouts and evaluation are.
2. **Make the engine fast.** The goal is more full-game rollouts per core-second
   on the cube rollout benchmark: one `next` plus `apply` at 1.5 µs per decision
   instead of about 27 µs, which takes about 120 rollouts per core-second to
   about 2,000. This also speeds research: one 300-seed paired A/B costs about 2
   hours on 16 cores today.
3. **Build the general evaluator and shallow search.** Start from generic
   features and 17lands card scores. Add each term in its own paired A/B.
4. **Learn a value model** from 17lands snapshots, then from self-play once the
   engine is fast. Add it as a correction on top of the hand-written evaluator,
   and keep it only if it wins a paired A/B. Good calibration is not evidence.
5. **Leave the WU mirror.** Measure on cube decks the evaluator was never tuned
   on.

## 6. How we prove it works

- **A frozen reference opponent.** Freeze the current `expert` as a separate
  agent before tuning anything. Improving the live expert must not move the bar,
  and every general-agent result is reported against the frozen copy.
- **Paired, seat-alternated games.** Both seats play the same shuffle seeds. Use
  McNemar's test on discordant pairs, Wilson 95% intervals on rates, and at
  least 300 paired seeds per A/B. No conclusions from partial runs.
- **Matched budgets.** Compare agents at the same iterations or wall-clock per
  decision, and record it with every result.
- **Pre-registered bars,** set by a human before measuring. Proposed:
  - Mirror: the general agent beats the frozen expert with the 95% interval's
    lower bound above 50%, over at least 400 games, at the UI's default budget.
  - Held-out decks: it beats `greedy` by more than the expert beats `greedy` in
    the mirror.
- **Held-out decks.** A set of cube decks is never used for tuning, debugging or
  reading traces. It is played only to confirm.
- **A tactical puzzle suite.** Positions with a known best play (take lethal,
  hold the counterspell, block or die), built with the scenario framework, split
  into development and held-out puzzles. Monolith's suite found its worst bug.
- **Reviews only find problems.** Transcript reviews by a language model are for
  diagnosis, then fixes through replays and constructed positions. They are not
  gates.

## 7. Not now

- AlphaZero-style or R-NaD training from scratch.
- Card-specific expert work beyond what Milestone 1 needs. A generic core
  (combat math, lethal checks, mana) stays useful as a fallback and in rollouts.
- ISMCTS and other information-set methods, until search is strong enough that
  hidden information costs it something.
- A learned move policy, until a value model has proven itself.

## 8. Open questions

- Can a general evaluator match the expert's card knowledge in the WU mirror?
  Step 1 shows soonest which way this goes.
- How far past the end of the turn should search look, and how should the
  opponent's instant-speed replies be searched?
- Do the 17lands snapshots, which lack targets, counters and tapped status,
  carry enough to train a value estimate that helps search?
