# Incident: Deep Searcher declines a free Mana Crypt (2026-11-07)

## Summary

In a human game against Deep Searcher (`pimc72-hidden-30m-anchor`), the agent
resolved Trinket Mage on turn 7, said yes to its search, was offered Mana Crypt,
and took nothing.

- Taking the Crypt was free. It goes to hand, the library is shuffled either
  way, and a card in hand never has to be cast.
- It was also strong there. The agent had three lands, all tapped for the Mage,
  and a Plains to play: the Crypt meant 6 mana next turn instead of 4.
- Portable Hole, the deck's other trinket, was already in its hand, so the Crypt
  was the only find.

This was not a rules engine bug. The search offered the Crypt correctly, and a
fresh search at the same decision declined it again. The cause is a mismatch
between the search tree and its rollout policy over Mana Crypt.

## Reproducing

The replay file is
[`2026-11-07-trinket-mage-declines-crypt.replay.json`](2026-11-07-trinket-mage-declines-crypt.replay.json),
copied from the app's `~/Library/Application Support/ca.thurn.beryl/replays/`.

```bash
just review docs/incidents/2026-11-07-trinket-mage-declines-crypt.replay.json --agent recorded --turn 7 --out review.txt
```

- `--turn 7` searches only that turn's decisions and stops there. It took about
  100 s of wall time under a load average of about 85 on 18 cores.
- Skipping the earlier searches moves the agent's random stream, so these
  searches are a fresh sample, not the game's own. One near-tie (playing the
  Plains, 0.626 against 0.623) went the other way.
- Without `--agent`, `just review` replays the game on a dev build in seconds,
  with no searches.

## What the search shows

```
P1 YesNo       yes | no          -> yes
  search: [0] 24817 0.644 [1] 21748 0.644
P1 ChooseUpTo  done | Mana Crypt -> done
  search: [0] 24424 0.671 [1] 20235 0.644
```

- Searching or not scored the same, and taking nothing led taking the Crypt by
  0.027. To the search, the Crypt in hand was worth less than nothing.
- `greedy` would have taken it: the rollout policy takes every find a search
  offers (`rollout.rs`, `DecisionKind::ChooseUpTo`).

### Why the Crypt scores below nothing

Deep Searcher's options are `ANCHORED` (`anchor: true, mid: false`), so the tree
answers mid-resolution questions like this one itself, and the anchor guards
only priority decisions.

- **Rollouts never cast Mana Crypt.** `values.rs` lists it in `NEVER_CAST` and
  values it at 0. In a rollout the Crypt in hand is a dead card: no mana, no
  damage.
- **The tree does cast it.** Every priority node of the tree, the agent's own
  main phases below this decision included, lists "cast Mana Crypt" as one more
  candidate. `Tree::select` plays each new child once before UCB1 compares them.
  So the "take" subtree spends visits casting the Crypt in-tree, where its coin
  flips deal 3 damage on half the upkeeps the rollout then plays out, by a
  policy that plans nothing around its mana.
- **The extra candidate also thins the tree.** One more child at every
  main-phase node spreads about 620 rollouts a world (44,659 over 72 worlds)
  more thinly. A subtree's mean averages its exploration, so the branch with
  more bad options scores lower.

The search is pricing the Crypt as "a card that may be cast badly, then
ignored", not as a card it could choose never to cast. This is the same family
as the Deep Seer incident's
[Fateseal over Brainstorm](2026-11-06-deep-seer-misplays.md#fateseal-over-brainstorm-options-with-a-choice-after-them-score-low):
a subtree's mean includes its random exploration.

The anchor does not catch it. `guarded` covers a priority decision where greedy
passes and the search casts, and it exempts Mana Crypt by design (by-bzy). A
search find declined is outside it.

### Not confirmed

The mechanism above fits the numbers but has not been measured. A rerun with
"cast Mana Crypt" removed from the tree's candidates should close most of the
0.027 gap if it is right.

## Follow-ups

Each needs a bead and a measurement against this replay.

1. **Confirm the mechanism.** Rerun turn 7 with never-cast cards pruned from the
   tree's priority candidates, and compare "done" against "Mana Crypt".
2. **Make the tree and rollouts agree on Mana Crypt.** Either prune never-cast
   cards from the tree too, or let both cast it on the terms `expert` uses
   (`CRYPT_LANDS`: while short of lands). The second keeps the Crypt's real
   value; the first is one line.
3. **Never decline a free find.** A search's find to hand is never worse than
   nothing. Answer `ChooseUpTo` for `Ask::Search` by greedy at the root too, or
   add it to `guarded`.
