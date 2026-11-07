# Incident: Deep Seer misplays against a human (2026-11-06)

## Summary

The first human game against Deep Seer (`pimc18-omniscient-30m`, added in
6b6ca5a) went badly for the agent. It made plays a strong player would not:

- **Oust on its own Rogue token** (turn 14), giving up a card and a 2/2 for 3
  life.
- **Jace, the Mind Sculptor's +2 instead of his 0** (turns 10 and 12), when Jace
  was safe enough to Brainstorm.
- **Jace's −1 on the human's Subtlety** (turn 14), handing back an
  enters-the-battlefield creature.

The human player judged the position winnable for a strong player. The agent's
own estimates fell from about 0.55 on turn 14 to 0.11 on turn 16 and 0.03 on
turn 18.

This was not a rules engine bug. A rerun of the agent's searches reproduced
every one of its 62 searched decisions exactly. The misplays come from how the
long-think search scores its options.

## Reproducing

The replay file is
[`2026-11-06-deep-seer-misplays.replay.json`](2026-11-06-deep-seer-misplays.replay.json),
copied from the app's `~/Library/Application Support/ca.thurn.beryl/replays/`.
It holds the seed, the decks, the stops and every answer, from both seats.

```bash
just review docs/incidents/2026-11-06-deep-seer-misplays.replay.json --out review.txt
```

- The command prints the transcript, adding a `search:` line after each of the
  agent's decisions: `[i] visits mean` for each candidate, in the order the
  decision lists them, plus the rollouts and engine steps spent.
- `REPLAY CHOSE [i]` marks a search that disagrees with the record. None did at
  the commit that added this document. Once the engine or `beryl_search`
  changes, a mismatch means the agent now plays differently there.
- The rerun took 183 s of wall time (850 CPU-s) on 18 cores under a load average
  of about 40, about 3.0 s a searched decision.
- If the engine changes the decisions it asks, the replay drifts. Check out the
  commit that added this document to recreate it exactly.

## What the searches show

Means are the searcher's estimated win rate for the option.

### Oust on its own token: a coin flip

Turn 14, after Jace's −1 bounced Subtlety, the only creatures left to target
were its own two Rogue tokens:

```
P1 Priority  pass | cast Oust  -> cast Oust
  search: [0] 24144 0.427 [1] 36921 0.433
P1 Target    Rogue Token | Rogue Token  -> Rogue Token
  search: [0] 36073 0.363 [1] 35319 0.354
```

- Casting it and passing scored within 0.006 of each other, and the more visited
  candidate wins.
- Rollouts play both sides with the softmax greedy policy. That policy holds
  removal for an opposing target worth `REMOVAL_MIN`, but nothing in a rollout
  plays well enough to show what holding Oust is worth.

### Fateseal over Brainstorm: options with a choice after them score low

Turn 12, Jace at 5 loyalty, no opposing creatures:

```
... | Jace +2 | Jace +0 | Jace -1  -> Jace +2
  search: ... [6] 25697 0.782 [7] 628 0.498 [8] 549 0.492
```

- The 0 (draw three, put two back) scored 0.28 below the +2. On turn 10 the gap
  was 0.714 against 0.651.
- Every decision below the root is a tree node, mid-resolution questions
  included. `Tree::select` plays each new child once, in random order, before
  UCB1 compares them, and a node's mean averages everything played below it.
- 628 visits across 18 trees is about 35 a tree. That is barely enough to try
  each put-back once, so the 0's mean is close to the value of putting back two
  random cards, often the best ones.
- The same should hold for any option that leads to a choice with many
  candidates: Currency Converter's discard, Stock Up, targets.

### The trees are shallow and optimistic

- `Sight::Omniscient` deals each tree the true state, with only its RNG
  reseeded. So pimc18 grows 18 trees of one world, each about 2–3k iterations a
  decision: root parallelism that averages out noise but adds no depth.
- The agent knew the human held The Wandering Emperor, which has flash, yet
  scored casting Oust at 0.433. The search at the very next decision, its
  target, scored it 0.36: one level deeper, the human's replies showed.
- How deep the trees reach has not been measured. That measurement comes first
  among the follow-ups.

### Subtlety bounce: a mild preference

```
pass | Oust | Jace +2 | Jace +0 | Jace -1  -> Jace -1
  search: [0] 2608 0.433 [1] 9104 0.506 [2] 6531 0.493 [3] 2233 0.424 [4] 28619 0.550
```

Jace was at 4 loyalty, and Subtlety had hit him for 3 the turn before. Bouncing
it protects Jace, so the play is defensible. Its 0.044 margin comes from the
same rollouts as above, though, not from planning past the human's recast.

## How the agent was presented

6b6ca5a described Deep Seer as "the long-think arena's best", and the session
that added it estimated 3–5 s a decision. Run-003's record does not support
"best":

- `pimc18/omniscient/rollout/tuned@30M` had played 2 games: 1–1, Elo +0 ±410.
- The leader was the same family at @10M: +576 ±211 over 22 games.
- Every rating is against `expert-frozen`, a scripted agent. None came from a
  human.

The registry description now drops the claim.

## Follow-ups

Each needs a bead and a measurement against this replay and the arena.

1. **Measure the trees.** Record, for each search, the depth it reaches and how
   often an iteration gets past the opponent's next decision.
2. **Mid-resolution choices.** Let the rollout policy answer questions asked
   while a spell or ability resolves (put-backs, discards, scry), or widen them
   progressively, so an option's mean stops depending on random sub-choices.
3. **One world, one tree.** For an omniscient search, grow one tree on all the
   threads, not 18 shallow trees of the same world.
4. **Near-ties.** Choosing by visits alone settles ties by noise. Consider the
   policy's choice or a confidence bound as the tiebreak when means are within
   noise.
