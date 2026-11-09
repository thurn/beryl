# Incident: Deep Searcher evokes Subtlety, holds its Spirits, exposes a Samurai (2026-11-08)

## Summary

In a human game against Deep Searcher (`pimc72-hidden-30m-anchor`), the player
filed three AI reports from one game, on turns 9, 10 and 11:

- **Turn 9: Subtlety evoked into nothing** (by-6fe). In the human's end step,
  with an empty stack, the agent evoked Subtlety and exiled Snapcaster Mage to
  pay for it. Subtlety's enters ability targets only a creature or planeswalker
  spell, so it lost two cards for nothing.
- **Turn 10: no attack** (by-cjj). The human had no creatures and 20 life. The
  agent kept both 1/1 flying Spirit tokens home.
- **Turn 11: Samurai made into Jace** (by-s0t). The human cast Jace, the Mind
  Sculptor. The agent flashed in The Wandering Emperor in response and used her
  −1 at once. The Samurai token then sat in front of Jace's −1 bounce. Made at
  the human's end step, it would have dodged it: the Emperor allows
  instant-speed loyalty abilities on the turn she enters.

None is a rules engine bug, and none comes from hidden information: the same
choices come out when the search can see every hand (`hands`) or everything
(`omniscient`). They come from the rollout policy (`rollout.rs`), which:

- evokes Subtlety whenever it can, with or without a target (confirmed);
- never spends Jace's −1 or other removal on a token (confirmed);
- penalizes attacking within the next two turns, for a reason not yet found.

## Reproducing

The replay file is
[`2026-11-08-subtlety-spirits-samurai.replay.json`](2026-11-08-subtlety-spirits-samurai.replay.json),
copied from the last of the three reports' folders. It holds all three
decisions.

```bash
just review docs/incidents/2026-11-08-subtlety-spirits-samurai.replay.json --agent recorded --turn 9 --out review.txt
```

- Use `--turn 10` for the attack and `--turn 11` for the Samurai.
- Each took 20 to 75 seconds of wall time under a load average of about 5 on 18
  cores.
- `--agent "pimc72/hands/rollout/tuned/anchor@30M"` (or `omniscient`) shows the
  same search with more sight; `.../eval/...` and `.../cut2/...` swap the leaf.
- The reports carry `searches.json`, so the game's own searches are below beside
  the fresh samples.

## What the searches show

Means are the searcher's estimated win rate for the option. The game ran on
0a39b892; `staging` had only documentation on top, so the replay does not drift.

### Turn 9: Subtlety for evoke

```
P1 Priority  pass | evoke Subtlety | activate Staff of the Storyteller
  game:   [0] 11331 0.664 [1] 20769 0.711 [2] 19698 0.710  -> evoke Subtlety
  fresh:  [0] 10356 0.645 [1] 20473 0.693 [2] 20971 0.701  -> Staff
P1 ExileCost Jace, Wielder | Snapcaster Mage | Counterspell
  game:   [0]  3787 0.526 [1] 26059 0.729 [2] 22297 0.720  -> Snapcaster Mage
```

- Passing trails because it gives up the Staff's draw, which fits only in this
  end step. Evoking Subtlety and activating the Staff tie (0.001 in the game).
  The agent activated the Staff after Subtlety resolved, so the search prices
  the two cards spent at nothing. The fresh sample chose the Staff.
- **Why two cards cost nothing.** `Policy::cast` scores Subtlety cast for evoke
  at its value 45 less 30, which is 15, against 0 for passing. A creature spell
  has no spell ability, so `enters_worth` decides, and it returns the score for
  any creature: "A creature keeps its body." An evoked one does not, and
  Subtlety's enters trigger, which targets a spell, is never checked.
  - So every rollout that holds Subtlety evokes it at an early priority, target
    or not, exiling a blue card. Held, Subtlety is worth nothing in the
    rollouts. That is the price the search compares against.
- **The anchor could not apply.** `guarded` (`long.rs`) holds the search to
  greedy's answer only where greedy passes. Greedy scored Subtlety and the Staff
  at 15 each and, taking the first of equals, chose Subtlety itself. The
  expert's rule (`expert/mod.rs`: "Subtlety's evoke is two cards for its enters
  ability, which answers only a spell") would have passed. Deep Searcher's
  rollouts do not use it.
- On turn 8 the tree passed on the same evoke three times: the root can see that
  evoking does nothing when it searches enough. On turn 9 the Staff's draw
  sharing the same end step made it a tie.

### Turn 10: the Spirits stay home

```
P1 Attacker  done | attack with Spirit Token
  game:        [0] 34004 0.741 [1] 20497 0.708
  fresh:       [0] 33005 0.738 [1] 21420 0.708
  hands:       [0] 33724 0.743 [1] 22294 0.721
  omniscient:  [0] 33535 0.792 [1] 21544 0.769
  eval leaf:   [0] 358910 0.629 [1] 364335 0.626
  cut2 leaf:   [0] 208976 0.694 [1] 64020 0.631
```

- Not a near-tie. Every rollout leaf prefers holding by 0.022 to 0.033, and does
  at every sight. Turn 8 did the same (0.697 against 0.671).
- Greedy attacks with both Spirits: `safe_attack` sees no untapped creature that
  could block and kill them. The anchor does not cover `Attacker` decisions.
- The board evaluation alone calls it even (0.003, with attacking visited more).
  Rollouts cut off after two turns penalize attacking by 0.063. So whatever
  punishes the attack happens in the rollouts' next two turns. The Spirits stay
  tapped through the human's turn 11, but no reason that costs them anything
  turned up:
  - The human had no creatures to block.
  - The human's Jace's −1 and Damn would not target a token (below).
  - The Wandering Emperor's −2 exiles a tapped creature, and a hidden world may
    deal the human their copy. But the human's hand held none, and the `hands`
    run, which knows that, held the Spirits back too. Whether their library held
    one soon was not checked.

### Turn 11: the Samurai at once

```
P1 Priority  pass | Emperor +1 | Emperor −1      (Jace, the Mind Sculptor on the stack)
  game:        [0] 15903 0.717 [1] 17466 0.727 [2] 25185 0.747  -> −1
  fresh:       [0] 17358 0.707 [1] 20256 0.724 [2] 24230 0.732  -> −1
  hands:       [0] 20762 0.738 [1] 20838 0.738 [2] 20516 0.735  -> +1
  omniscient:  [0] 21126 0.780 [1] 18031 0.773 [2] 25585 0.792  -> −1
  eval leaf:   [0] 78957 0.595 [1] 218287 0.625 [2] 381750 0.635
  cut2 leaf:   [0] 95694 0.680 [1] 54296 0.664 [2] 128333 0.691
```

- **The rollouts cannot see the risk the player named.** Jace's −1 ("Return
  target creature to its owner's hand") is removal: `worth_targeting` asks its
  best target to reach `REMOVAL_MIN` (25). A token's value comes from
  `values::estimate` plus 3 a power on the battlefield. That is 22 for the 2/2
  Samurai and 16 for a 1/1 flying Spirit. So a rollout never bounces a token,
  though bouncing one destroys it.
  - Single-target Damn and the Emperor's own −2 miss tokens for the same reason.
  - The Samurai made now looks as safe as one made later.
- **The rollouts never wait.** `Policy::activate` scores the −1 at 25 against 0
  for passing, so a rollout activates it at its first priority. The Emperor's
  instant-speed window (`Rule::InstantLoyaltyTheTurnItEntered`, which the engine
  implements) is used, but never saved for the end step. Only the tree can find
  that line, several of the human's priority decisions deep.
- With the risk unseen, a token on board sooner scores better: the evaluation
  leaf prefers −1 by 0.040, and the rollouts by 0.015 to 0.030. With the hands
  known it was a three-way tie that chose +1.
- The agent's earlier pass on Counterspell for Jace (0.665, last of three) was
  not reported and was not looked into.

## Follow-ups

The ladder (by-97g, `docs/ai-strategy.md` §6) already holds the rungs that fit.
Notes on each bead name this replay as a case:

1. **Evoke in the rollouts** (rung 4, by-97g.8). A creature cast for evoke is
   worth its enters ability alone. `enters_worth` should check its targeted
   enters triggers as it does a noncreature's: Subtlety evoked with no creature
   or planeswalker spell to target scores 0. Read from the casting option, not
   the card's name.
2. **Bounce kills a token** (rung 4, by-97g.8). Score a "return to hand" effect
   on a token at the token's whole value plus what it cost to make, or drop
   `REMOVAL_MIN` for it, so rollouts punish a token left in front of Jace.
3. **Instant-speed loyalty waits** (rung 4, by-97g.8). Like rung 4's flash
   creatures: a planeswalker whose loyalty abilities are at instant speed this
   turn holds them for the opponent's end step.
4. **Attacks** (rung 2, by-97g.6). Guarding `Attacker` with the anchor's margin
   (0.02 plus noise) may not hold here: the search leads by 0.022 to 0.033. Find
   what the rollouts' next two turns punish first. A cut1 leaf, or a rollout
   trace of one attacking line, should show it.
