# Incident: Searcher mulligans against its own means (2026-11-08)

## Summary

In a human game against Searcher (`mcts-hidden`), WU Control mirror, the agent
was on the draw and mulliganed this seven:

> The Wandering Emperor, Floodfarm Verge, Counterspell, Oust, Mana Crypt, Thirst
> for Discovery, Jace, the Mind Sculptor

The new AI-play recorder (`beryl_session::ai`) showed its search like this:

```
Turn 0 untap, keepOrMulligan (log 2): Mulligan, in 6983 ms, 3528 rollouts
   [0]    1403  0.726  Keep
  *[1]    2125  0.650  Mulligan
```

The agent plays the candidate visited most. Here the visits favoured Mulligan
while the means favoured Keep by 0.076, so it looked as if the agent had played
against its own estimate.

It had not. The means were the wrong statistic:

- **The recorder was right about the order.** Candidate labels and visits both
  follow the decision's candidate order.
- **The means pooled the trees.** Each was a candidate's rewards summed over all
  18 trees divided by its visits summed over them. Each tree searches a world of
  its own and spends most of its visits on its own best candidate, so a pooled
  mean comes mostly from the worlds that favour that candidate. Averaged tree by
  tree instead, Mulligan led, as its visits said. This is Simpson's paradox.
- **The choice code was never at fault.** `Mcts` votes by visits. `long.rs`'s
  anchor check already pairs means tree by tree.

The fix adds `Search::means`, each candidate's mean averaged over the trees with
every tree weighing the same. The recorder, `just review` and the long-think
decision log now show it. No agent chooses differently.

Whether Mulligan was the right call is a separate question, open below.

## Reproducing

The original report folder was deleted. The replay file
[`2026-11-08-searcher-mulligan-means.replay.json`](2026-11-08-searcher-mulligan-means.replay.json)
was rebuilt by hand:

- the game's seed (1794147518325253000);
- both seats on `decks::WU_CONTROL`, the human in seat 0, smart stops;
- the log as the engine asks it: the human plays first and keeps, the agent
  mulligans, keeps six, and bottoms Trinket Mage.

`just review` without `--agent` replays it to the same six-card hand the game
recorded, Trinket Mage first among the bottom candidates.

```bash
just review docs/incidents/2026-11-08-searcher-mulligan-means.replay.json --agent recorded --turn 0
```

Turn 0 has no searches before it, so the rebuilt agent's random stream is the
game's own, and the review repeats the game's three searches exactly: the same
visits and rollouts the recorder showed. With the fix, the means are averaged
over the trees:

```
P1 mcts-hidden KeepOrMulligan keep | mulligan  -> mulligan
  search: [0] 1403 0.385 [1] 2125 0.501  (3528 rollouts, 2.8M steps)
P1 mcts-hidden KeepOrMulligan keep | mulligan  -> keep
  search: [0] 2065 0.516 [1] 1935 0.474  (4000 rollouts, 3.0M steps)
```

Mulligan now leads on its mean by 0.116 as well as on its visits. The keep on
six agrees too.

The search itself takes about a second on a quiet machine. The review took 21
minutes of wall time at utility priority under a load average near 89 on 18
cores, most of it building.

## What the search shows

### Each tree is one world, and both candidates are nearly fixed in it

- `Observation::determinize` deals each tree's hidden cards once, the agent's
  own library order included. Every iteration of that tree draws the same cards
  in the same order.
- `Tree::iterate` applies the root choice to a copy of the root state before
  reseeding for the rollout. So Mulligan's shuffle (`start::advance_mulligans`)
  runs on the same random stream every iteration: each tree mulligans into one
  new hand, every time.

So within a tree, Keep is this hand with one fixed run of draws, and Mulligan is
one fixed new hand. UCT soon finds which wins that world and spends most of the
tree's 222 visits there, often over 90%.

### The pooled means

One fresh search of the position (agent seed 1, the UI budget of 4000 iterations
over 18 trees, no early stop), tree by tree:

| tree | Keep visits | Keep mean | Mulligan visits | Mulligan mean |
| ---: | ----------: | --------: | --------------: | ------------: |
|    0 |         214 |     0.804 |               9 |         0.111 |
|    1 |          14 |     0.357 |             209 |         0.689 |
|    5 |           3 |     0.000 |             219 |         0.950 |
|    9 |         209 |     0.957 |              13 |         0.615 |
|   13 |          41 |     0.000 |             181 |         0.133 |
|   14 |         173 |     0.994 |              49 |         0.878 |
|   16 |           4 |     0.000 |             218 |         0.748 |

That is seven of the 18 trees. Over all 18:

|                          |  Keep | Mulligan |
| ------------------------ | ----: | -------: |
| root visits              |  1754 |     2246 |
| trees led                |     8 |       10 |
| pooled mean (as shown)   | 0.765 |    0.676 |
| mean averaged over trees | 0.424 |    0.541 |

Keep's pooled 0.765 is almost all from the 8 trees where it led, whose worlds
drew it lands. The 10 trees where it lost gave it 129 visits, mostly losses. The
average over trees counts each world once and ranks the candidates as the visits
do.

Over 23 fresh searches (agent seeds 1 to 3 and 11 to 30):

- Mulligan was visited most in 19.
- The average over trees agreed with the visits in 21. The two exceptions were
  near-ties, with each candidate leading 9 trees.
- The pooled mean favoured Keep in 12 of the 19 searches that chose Mulligan, as
  in the game.

So the game's own search was a typical sample.

## Is it systematic?

Yes, wherever the trees' worlds disagree. Three hidden-search mirror games
played by Searcher on both sides (seeds 1000 to 1002, the UI budget) were
checked at every searched decision. A "split" is one where the candidate with
the best mean is not the one visited most, has at least a tenth of the visits,
and leads it by more than 0.02:

| search     | searched decisions | splits, pooled mean | splits, mean averaged over trees |
| ---------- | -----------------: | ------------------: | -------------------------------: |
| hidden     |                568 |            92 (16%) |                          51 (9%) |
| omniscient |                646 |              5 (1%) |                           4 (1%) |

- Priority decisions held most of the hidden splits: 49 pooled and 31 averaged,
  of 308.
- Proportionally, `ChooseUpTo` split most often: 10 pooled and 6 averaged, of
  28\. These are mostly searches of a library, where each tree's world holds
  different cards.

The omniscient search's trees share the true world and differ only in their
random streams, so its splits are the noise floor. Its few splits include
mulligans: its trees reseed the root state, so each tree mulligans into a
different hand.

So for the hidden search, a pooled mean was the wrong number to read at one
searched decision in six. Even averaged tree by tree, the best mean is not the
candidate visited most at one in eleven. Summing visits over trees is close to a
vote among the worlds, and a vote is not an expectation: a candidate that wins
big in a few worlds and loses narrowly in more has the better mean but loses the
vote. Follow-up 3 is about which of the two the agent should play.

## Was Mulligan right?

Not settled. Three things bear on it:

- **The hand has one land.** On the draw, with Mana Crypt, a strong player might
  keep it. But rollouts never cast Mana Crypt (`values.rs`, `NEVER_CAST`). The
  trees are only a few decisions deep, so nearly all of each game is a rollout,
  where this hand is a one-lander with a dead card. The search is pricing a
  worse hand than the real one. This is the same mismatch as the
  [Trinket Mage incident](2026-11-07-trinket-mage-declines-crypt.md).
- **18 worlds are few for a mulligan.** Each tree values Keep on one run of
  draws and Mulligan on one new hand. The trees led ranged from 4 to 14 of 18
  across the 23 searches.
- **Most of each tree's value is decided by its world.** Many trees score a
  candidate 0.000 or above 0.95. More iterations a tree buy little; more worlds
  would buy a lot.

## Fix

`Search::means` (`beryl_search`, `mcts.rs` and `long.rs`) averages each tree's
mean for a candidate, counting only trees that visited it. The test
`tree_means_weigh_each_tree_the_same` builds three trees that disagree, as here,
and checks that the pooled rewards rank the candidates against their visits
while the averaged means do not.

Readers changed:

- the AI-play recorder (`SearchView::of`);
- `just review`'s search lines;
- the long-think arena's `decisions.jsonl` `mean` field. Old logs without
  `means` fall back to pooling.

`Search::rewards` stays as it was, summed, for the logs that train on it.

Playing strength is unchanged: no agent's choice reads the new field.

## Follow-ups

1. **Let rollouts cast Mana Crypt** on the terms `expert` uses (`CRYPT_LANDS`:
   while short of lands), or stop the tree casting it. This hand's value hangs
   on it. It is follow-up 2 of the Trinket Mage incident.
2. **More worlds for a mulligan.** Mulligan decisions come once a game and are
   decided by draws. Searching them over many worlds, or with `long.rs`'s
   `Method::Ismcts` (a fresh world every iteration), would measure the hand, not
   18 runs of draws. A tournament would have to show it helps.
3. **Choose by the tree-averaged mean, or keep the vote?** They disagree at 9%
   of the hidden search's decisions. A tree's losing candidate has few visits,
   so its mean there is noisy, which is the case for the vote. The evidence
   would be a paired tournament: Searcher as it is against Searcher choosing by
   `means`, same seeds, at the UI budget. That is about 20 minutes a hundred
   games.
4. **Read `means`, not pooled rewards, wherever means are compared.**
   `agreement.rs` still classifies decided positions from pooled log rewards,
   and `anchor_replay.rs` reads pooled rewards from old logs.
