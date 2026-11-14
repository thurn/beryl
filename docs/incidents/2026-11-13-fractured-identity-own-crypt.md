# Incident: Deep Searcher aims Fractured Identity at its own Mana Crypt

On turn 16 of the WU Control mirror (build `6116e7ab`), Deep Searcher
(`pimc72-hidden-30m-anchor`) cast Fractured Identity during its own declare
attackers step and chose its own Mana Crypt as the target. Fractured Identity
reads "Exile target nonland permanent. Each player other than its controller
creates a token that's a copy of it." The agent therefore exiled its own Crypt
and gave the human a second one. The human reported it from the app's bug
button: "The AI cast fractured identity on its own mana crypt to give me a mana
crypt!"

## The position

The agent (P1) was far ahead and knew it: every candidate's mean was above 0.84
and the best near 0.95.

- **P1**, 17 life: Teferi, Time Raveler (8 loyalty after its +1, so P1 could
  cast the sorcery at instant speed and P0 could not answer it), The Wandering
  Emperor (3 loyalty), a 4/4 first-strike vigilance Samurai attacking, a 2/2
  Samurai, Snapcaster Mage, Portable Hole, Staff of the Storyteller, Mana Crypt
  and seven lands. In hand: Fractured Identity, Reprieve, Spara's Headquarters.
- **P0**, 13 life: Subtlety (3/3 flying, its only creature), Portable Hole
  holding P1's Jace, Vryn's Prodigy, Staff of the Storyteller, Currency
  Converter, a Treasure, Mana Crypt and ten lands. One hidden card in hand
  (Counterspell, uncastable under Teferi).

Neither player needed more mana: P0 had ten lands and P1 seven. A Mana Crypt
there is mostly its coin flip, 3 damage to its controller on half of their
upkeeps.

## What a strong player plays

**Subtlety (P0).** P0 loses their only creature, and P1 gets a 3/3 flier.

- P0 cannot block the 4/4 first striker profitably either way, so it goes to 9.
- On P1's next turn it attacks with 4 + 2 + 2 + 3 = 11 power into an empty
  board. That is lethal unless P0 draws a sweeper or a blocker, and P0's own
  Crypt may take it to 6 first.

**Own Mana Crypt (played).** P0 keeps Subtlety and gets a second Crypt.

- P0 still goes to 9. Its two flips deal 0, 3 or 6 damage next upkeep.
- On P1's next turn it attacks with 8 power, and Subtlety blocks one attacker,
  so about 6 gets through. That is lethal only when P0 lost at least one flip
  (three times in four), and Subtlety stays free to attack P1's planeswalkers.
- It also hands P0 two more mana each turn for whatever they draw.

P0's Portable Hole was another good target: exiling it returns Jace, Vryn's
Prodigy to P1, and P1's token copy of the Hole exiles one of P0's cheap
permanents as it enters. Targeting any of P1's own permanents gives P0 a copy of
it.

The cost was small in a won game, but the reasoning is wrong in a way that will
cost games: the agent spent a card to trade one Crypt's flips for two of the
opponent's, instead of removing the opponent's only creature.

## Reproduction

The [app replay](2026-11-13-fractured-identity-own-crypt.replay.json) reaches
the target choice at turn 16 decision index 14 (log 322).

```bash
just review docs/incidents/2026-11-13-fractured-identity-own-crypt.replay.json --agent recorded --turn 16
just blunders --only fractured-identity-own-crypt
```

## What the search shows

The game's own search at the target choice (86,648 rollouts, trees reused from
decision 91):

```
   [0]  12692  0.946  Subtlety (P0)
   [1]  11218  0.939  Portable Hole (P0)
   [3]   5948  0.861  Mana Crypt (P0)
   [8]   4779  0.844  Teferi, Time Raveler (P1)
  *[9]  13803  0.953  Mana Crypt (P1)
```

The two Crypts are the telling pair. Exiling P0's Crypt leaves P1 with two
Crypts and P0 with none; exiling P1's leaves the reverse. The search put 0.09
between them, more than the gap between Subtlety and Teferi's own exile, so it
treats each Crypt as costing its controller about 0.045 of a win. It rated
Subtlety, the target that removes a blocker and adds a flier, 0.007 below moving
the Crypts. The cast itself was a near tie with passing (0.912 against 0.909).

Rerunning the recorded agent with `just review --agent recorded --turn 16`
reproduced every turn-16 search to the visit, the target choice included, so the
choice is the agent's settled answer from this position, not one unlucky sample.
The catalog reached the recorded decision without drift. The current champion
chose its own Mana Crypt in four of five samples at 3M steps (Subtlety once) and
in all five at 30M steps.

### Likely cause

The rollout policy never casts Mana Crypt and values it at 0 (`values.rs`,
`NEVER_CAST`), and its upkeep trigger still flips for 3 damage in every rollout.
Rollouts in this control mirror run many turns, so a Crypt's flips add up over
the playout while its mana buys nothing a rollout uses. That makes moving Crypts
across the table look like a large life swing. A 3/3 flier, by contrast, is one
more body that the rollout policy's attacks and blocks may not convert into
damage before the playout's end. The expert policy already discounts a Crypt to
10 for a player with five or more lands (`appraise` in
`beryl_search/src/expert/mod.rs`), but the tree's means come from rollouts, not
from that appraisal.

This is not confirmed. A rerun with Crypt flips disabled in rollouts should
bring Subtlety above the Crypt if it is right.

## Follow-ups

The blunder catalog case `fractured-identity-own-crypt` forbids every one of
P1's own permanents as the target. The ladder owns the fix.
