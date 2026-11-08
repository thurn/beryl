# Incident: Deep Searcher Reprieves an Oust the human can recast (2026-11-08)

## Summary

In a human game against Deep Searcher (`pimc72-hidden-30m-anchor`), the human
cast Oust on the agent's Beza, the Bounding Spring in their turn 8 precombat
main. The agent answered with Reprieve, though the human had 5 mana left to cast
Oust again at once. The player asked whether that was worth it.

It was not a mistake, and not a rules engine bug.

- Reprieve is card-neutral: it draws a card. The agent's 4 untapped lands were
  idle in the human's turn, so its only cost was holding Reprieve for later.
- The human paid {W} a second time for the same Oust.
- After Reprieve the agent still had {W}{U} up for No More Lies. Countering the
  recast Oust with it would have taxed the human {3} more: 5 of their 6 mana for
  one Oust.
- Passing scored lowest of the three options, about 0.03 below both answers.
  Choosing Reprieve over No More Lies was a near-tie settled by visit counts,
  and Reprieve costs no card.

## Reproducing

The replay file is
[`2026-11-08-reprieve-on-oust.replay.json`](2026-11-08-reprieve-on-oust.replay.json),
copied from the bug report's folder.

```bash
just review docs/incidents/2026-11-08-reprieve-on-oust.replay.json --agent recorded --turn 8 --out review.txt
```

- It took about 20 minutes of wall time under a load average of about 90 to 110
  on 18 cores.
- The report has no `searches.json`: it was filed 22 minutes before 2517b64d
  began saving the agent's searches with each report, so the game's own search
  is lost. The search below is a fresh sample that made the same choice.

## What the search shows

Means are the searcher's estimated win rate for the option.

```
P1 Priority  pass | cast Reprieve | cast No More Lies  -> cast Reprieve
  search: [0] 12899 0.723 [1] 25412 0.751 [2] 22604 0.756  (60915 rollouts, 30.0M steps)
```

- Both answers led passing: Reprieve by 0.028, No More Lies by 0.033.
- No More Lies led Reprieve by 0.005, but `leader` plays the most visited
  candidate. The two are a near-tie, as in the
  [Snapcaster incident](2026-11-07-snapcaster-and-jace-wielder.md#snapcaster-a-coin-flip).
  Here the visit count picked the cheaper answer: No More Lies costs a card, and
  the human could pay its {3}.
- The agent was well ahead (Teferi, Hero of Dominaria at 6 loyalty, Beza on the
  board). The whole decision spans 0.03.

### Why the search values Reprieve here

The rollout policy (`rollout.rs`) answers any spell worth `COUNTER_MIN` (40). It
does not check whether the caster can cast the spell again.

- Oust is worth 50 (`values.rs`), so in rollouts both of the agent's answers are
  worth casting. Greedy itself picks No More Lies (50) over Reprieve (35).
- Rollouts recast Oust after Reprieve and pay for No More Lies ("say yes to
  every unless pays"). So the search prices Reprieve as what it was: a cantrip
  that costs the human {W}.
- Holding Reprieve for a spell the human cannot recast is priced too: the same
  policy would cast it on a later spell worth 40 or more, Jace, Wielder of
  Mysteries (65) say. Against a human with Mana Crypt and five lands, few spells
  qualify. Reprieve is mostly a cantrip here either way.

### The anchor did not apply

`guarded` (`long.rs`) holds the search to greedy's answer only where greedy
passes and the search casts. Greedy cast No More Lies here, so the search chose
freely. The expert's stricter rule (`expert/mod.rs`, `counter_score`) casts
Reprieve only when the caster cannot recast the spell this turn. Expert would
have passed, but Deep Searcher's rollouts do not use it.

## Follow-ups

None filed. The play was sound, and the 0.005 tie between the two answers is the
visit-count noise
[the Snapcaster incident](2026-11-07-snapcaster-and-jace-wielder.md) already
describes.
