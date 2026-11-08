# Incident: Deep Searcher wastes Snapcaster, spares Jace, Wielder (2026-11-07)

## Summary

In a human game against Deep Searcher (`pimc72-hidden-30m-anchor`), the human
won on turn 21 by drawing from an empty library under Jace, Wielder of
Mysteries. They had stolen it from the agent with Fractured Identity on turn 11.
Two of the agent's plays stood out:

- **Turn 12: Snapcaster Mage for no value.** After Jace, the Mind Sculptor and a
  land, the agent cast Snapcaster in its postcombat main with its last 2 mana.
  It gave Counterspell flashback that it could never pay for. Flashed in at the
  human's end step, it would have done the same and dodged sorcery-speed answers
  (the human held Winds of Abandon).
- **Turn 16: no lethal attack on Jace, Wielder.** The human was tapped out with
  no creatures; the agent's Winds of Abandon had just exiled their Snapcaster.
  Jace, Wielder had 3 loyalty, and the agent had Snapcaster (2 power) and a
  Spirit token (1). It sent Snapcaster at the human's stolen Jace, the Mind
  Sculptor (2 loyalty) and held the Spirit back.

Neither was a rules engine bug. Two causes stack:

- Near-ties are settled by visit counts, which is noise.
- The rollout policy never plays Jace, Wielder's self-mill and values it below
  Jace, the Mind Sculptor, so the search does not see how close the human was to
  winning.

## Reproducing

The replay file is
[`2026-11-07-snapcaster-and-jace-wielder.replay.json`](2026-11-07-snapcaster-and-jace-wielder.replay.json),
from the app's `~/Library/Application Support/ca.thurn.beryl/replays/`, trimmed:

- The game ran on a build from before per-action undo (dd106d2), where an undo
  went back to the start of the human's turn. On turn 14 the human undid to the
  start of turn 13.
- Today's tools read every undo as taking back one answer, and fail there ("an
  undo with nothing to take back").
- The copy drops the undone branch: log entries 65 to 92, the `undo` included.
  Before and after it, `just review` gives the same transcript as the original
  file under the old `review.rs`.
- So the copy has the turn 12 Snapcaster, which came before the undo, and only
  the turn 13 and 14 played after it.

```bash
just review docs/incidents/2026-11-07-snapcaster-and-jace-wielder.replay.json --agent recorded --turn 12 --out review.txt
```

- Use `--turn 16` for the attack.
- Turn 16 took about 9 minutes of wall time under a load average of about 80 on
  18 cores. Turn 12 took 30, starved at utility QoS by a long-think run.
- These searches are fresh samples, not the game's own: `--turn` moves the
  agent's random stream. Both misplays came out differently on the rerun, which
  is the point of the first finding below.

## What the searches show

Means are the searcher's estimated win rate for the option.

### Snapcaster: a coin flip

Turn 12, postcombat main, 2 lands untapped:

```
P1 Priority  pass | cast Snapcaster Mage | cast Portable Hole  -> cast Snapcaster Mage
  search: [0] 24254 0.279 [1] 22015 0.285 [2] 18362 0.272  REPLAY CHOSE [0]
P1 Target    Oust | Counterspell  -> Counterspell
  search: [0] 32966 0.280 [1] 31474 0.281
```

- All three options sit within 0.013. The rerun passed; the game cast.
- The rollout policy casts anything worth more than passing as soon as it can
  (`rollout.rs`, `Policy::cast`): Snapcaster is worth 35 whenever it can be
  cast. Holding a flash creature for the opponent's end step is worth nothing in
  a rollout, so the tree cannot learn it.
- The anchor does not catch it. `guarded` covers a priority decision where
  greedy passes and the search casts, and greedy casts Snapcaster here too.

### Jace, Wielder spared: visits over means, then a coin flip

Turn 16, after Winds of Abandon and Currency Converter:

```
P1 Attacker     done | attack with Snapcaster Mage | attack with Spirit Token  -> attack with Snapcaster Mage
  search: [0] 20202 0.141 [1] 40583 0.217 [2] 29411 0.191
P1 AttackTarget the player | Jace, Wielder of Mysteries | Jace, the Mind Sculptor  -> Jace, the Mind Sculptor
  search: [0] 18189 0.148 [1] 37360 0.228 [2] 38477 0.213
P1 Attacker     done | attack with Spirit Token  -> done
  search: [0] 49733 0.177 [1] 50336 0.184  REPLAY CHOSE [1]
```

- **Visits overruled the means.** Jace, Wielder led by 0.015, but Jace, the Mind
  Sculptor had about 1,100 more visits, and `leader` in `long.rs` picks by
  visits.
- **Holding the Spirit back was a coin flip** at 0.007. The rerun attacked.
- **The margins are small because the rollouts miss the threat.** Killing Jace,
  Wielder there ends the human's only quick win, yet the search priced it barely
  above the other Jace. Two gaps in the rollout policy explain that.

### The rollouts never see Jace, Wielder win

- **Its +1 mills the opponent.** Rollouts target by `Policy::targeting`. An
  ability targets its controller's side only when `values.rs` lists it in
  `FRIENDLY` or its filter says "you". "Target player mills two cards" lists
  neither, so a rollout mills the opponent. The human milled themself each turn
  and won from 15 cards in five turns.
- **The tree knows better at its root.** With `mid: false` the tree answers
  targets itself, and the agent chose itself for its own +1 on turns 8 and 10.
  Below the tree, the rollouts that score every option play Jace, Wielder as a
  slow mill against the opponent.
- **It is worth less than Jace, the Mind Sculptor.** `Policy::value` gives a
  planeswalker its card value plus 4 a loyalty counter: 65 + 12 = 77 for Jace,
  Wielder against 90 + 8 = 98 for the other Jace. `walker_target` sends
  attackers by that value, so greedy and the rollouts attack the wrong Jace.
- **The expert already handles both.** `expert::appraise` values a Jace, Wielder
  whose controller is near a decking win at 150 plus 6 a loyalty counter
  (`JACE_WINS`), and the expert never mills an opponent whose Jace, Wielder
  could still win. Deep Searcher's rollouts use the plain policy
  (`rollout/tuned`), not the expert's.

The same blind spot fits turn 18. The human's library was at 9 and the agent had
5 power against Jace, Wielder's 4 loyalty, behind three blockers. It did not
attack at all.

### Not confirmed

- That the rollouts' view of Jace, Wielder is what kept the turn 16 margins
  small. A rerun with its +1 marked friendly and the expert's appraisal should
  widen the lead of attacking Jace, Wielder with both creatures if it is right.
- The turn 18 non-attack was not searched again.

## Follow-ups

Each needs a bead and a measurement against this replay and the arena.

1. **Jace, Wielder in the rollouts.** Add its +1 to `FRIENDLY` so a rollout
   mills its own library. Value it as `appraise` does when its controller's
   library is short.
2. **Flash creatures wait.** Let the rollout policy hold a flash creature in its
   own main phase and cast it at the opponent's end step. Greedy would then pass
   on turn 12, and the anchor's `ANCHOR_MARGIN` of 0.02 would have kept the
   0.006 lead from casting Snapcaster.
3. **Near-ties at attacks.** Let greedy's answer stand when the search's choice
   does not lead it clearly at `Attacker` and `AttackTarget` too. That would
   have sent the Spirit. `guarded`'s comment puts the cost of guarding beyond
   priority casts at 0.04 to 0.07 a game, by the search's own estimates.
4. **Visits against means.** Across pooled PIMC trees, the most visited child
   can trail on mean, as Jace, the Mind Sculptor did here. Consider choosing by
   mean, or a lower confidence bound, when the visits are close. This is the
   Deep Seer incident's [near-ties](2026-11-06-deep-seer-misplays.md#follow-ups)
   follow-up again.
5. **Old undos.** Replays from before dd106d2 that contain an undo cannot be
   reviewed. Trim them by hand as this one was, or teach `review.rs` the old
   rule if more turn up.
