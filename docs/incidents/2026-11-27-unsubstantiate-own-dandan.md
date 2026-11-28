# Incident: Deep Searcher bounces its own Dandân with Unsubstantiate

On turn 29 of a Dandân game (the shared-deck format, build `93520716`), Deep
Searcher (`pimc72-hidden-30m-anchor`) cast Unsubstantiate in its second main
phase and returned one of its own two tapped Dandâns to its hand. It then cast
that Dandân again and was left with one untapped land. Unsubstantiate reads
"Return target spell or creature to its owner's hand." The human had no creature
and the stack was empty, so the agent's own Dandâns were the only targets. The
human reported it from the app's bug button: "Casting unsubstantiate on your own
DanDan is fairly obviously a blunder made because the AI is winning dramatically
and thus MCTS is now random."

**Verdict: a blunder, though not for the reason the reporter gave.** The values
were not saturated. Every candidate's mean was between 0.49 and 0.60, and the
rollouts behind them won only about 55% of the time. The search preferred the
bounce because its rollouts misplay the fight over the human's next Dandân, and
they misplay it less often when the agent has fewer cards and less mana.

## The position

The agent (P1) was ahead on board, the human (P0) ahead on life. Both draw from
one 19-card library and share one graveyard.

- **P1**, 8 life: two Dandâns (4/1), both tapped from attacking, and seven
  lands. In hand: Unsubstantiate, Memory Lapse, Magical Hack and two Day's
  Undoing.
- **P0**, 12 life: ten lands, no creatures, three cards in hand. P1 knew that
  one of them was a Dandân, because its own Unsubstantiate had returned that
  Dandân to P0's hand on turn 28. The other two were Islands.

Dandân reads "Dandân can't attack unless defending player controls an Island.
When you control no Islands, sacrifice Dandân." Both players had plenty of
Islands, so each Dandân was a 4/1 that would attack every turn.

## The two lines compared

**Pass (correct).** P1 keeps seven lands open through P0's turn 30 with Memory
Lapse, Unsubstantiate and Magical Hack in hand.

- When P0 casts their Dandân, Memory Lapse puts it on top of the shared library.
  In this format P1 is the next player to draw, so P1 gets the Dandân.
- Magical Hack can change "Island" on a resolved Dandân of P0's to another basic
  land type. P0 controls none of that type, so P0 must sacrifice the Dandân.
- On turn 31 two Dandâns attack for 8, putting P0 at 4.

**Bounce and recast (played).** P1 spends Unsubstantiate and six of its seven
lands to end the turn exactly as it began: two Dandâns, one of them now
untapped. An untapped Dandân does nothing on P0's turn, because P0 has no
creature that could attack. P1 has one card fewer and cannot pay for Memory
Lapse, so P0's Dandân resolves and blocks one of P1's Dandâns on turn 31.

The bounce gained nothing and cost a card and the only window to counter P0's
Dandân.

## Reproduction

The [app replay](2026-11-27-unsubstantiate-own-dandan.replay.json) reaches the
decision as turn 29's eighth decision (index 7, log 310).

```bash
just review docs/incidents/2026-11-27-unsubstantiate-own-dandan.replay.json --agent recorded --turn 29
just why unsubstantiate-own-dandan --a pass --b "cast Unsubstantiate" --rollouts 4000
```

Until this change, `beryl_cli review` and the blunder suite could replay only WU
Control mirror games. `Walk::of_file` now builds the recorded game's table and
deal with `ReplayFile::setup`, which the session uses, so a shared-deck game
replays too.

## What the search shows

The game's own search, a turn search over 4,096 sampled worlds per candidate:

```
   [0]  4096  0.562  Pass
   [1]  4096  0.572  Cast Day's Undoing
   [2]  4096  0.494  Cast Magical Hack
  *[3]  4096  0.596  Cast Unsubstantiate
   [4]  4096  0.575  Cast Day's Undoing
  anchor: greedy Pass, lead 0.034 ± 0.006
```

The anchor holds the greedy policy's answer (here, Pass) unless the search's
choice leads it by `ANCHOR_MARGIN` (0.02) and two standard errors. A lead of
0.034 with a standard error of 0.006 cleared both. Rerunning the recorded agent
on this build before the fix chose Unsubstantiate again (0.589 against 0.557).

`just why` played 4,000 pairs of rollouts from the same sampled worlds, one
after each choice:

| Rollout policy                        | Pass wins | Bounce wins | Pass − bounce     |
| ------------------------------------- | --------- | ----------- | ----------------- |
| Before the fix                        | 54.5%     | 58.3%       | −3.8 pp (SE 0.8)  |
| Day's Undoing held until after combat | 61.4%     | 64.9%       | −3.5 pp (SE 0.8)  |
| Both rules                            | 79.4%     | 64.3%       | +15.1 pp (SE 0.9) |

## Cause

Two flaws in the rollout policy (`beryl_search/src/rollout/policy.rs`) made a
winning position look like a coin flip, and the first made having more cards
look worse.

1. **Bouncing a spell its owner can cast again.** In nearly every rollout after
   Pass, P1 answered P0's Dandân spell with Unsubstantiate rather than Memory
   Lapse. P0 had ten lands, cast the Dandân again at once, and often countered
   the Unsubstantiate with a Memory Lapse of their own. P1 then threw Magical
   Hack and Memory Lapse after it, and P0's Dandân resolved anyway. In the
   bounce line, P1 had no Unsubstantiate left to misuse. The policy counted a
   spell on the stack as a target worth removing for any harmful targeted spell,
   but returning a spell to its owner's hand does not counter it. Its owner
   loses only the mana they paid if they have that much to spare.
2. **Day's Undoing in the first main phase.** In both lines, P1's rollouts often
   cast Day's Undoing on turn 31 before combat. On its caster's turn it ends the
   turn, so the two Dandâns never attacked. The policy cast a wheel (a spell
   that shuffles away and refills every hand) whenever its hand held three cards
   or fewer, at any point in its turn.

## Fix

- `Effect::only_returns_target_to_hand` (`beryl_card/src/facts.rs`) marks an
  ability whose whole effect is to return its target to its owner's hand.
  Reprieve, which also draws a card, is not marked. The rollout policy's
  `worth_targeting` no longer counts a spell on the stack as a worthwhile target
  for such an ability when its controller could cast it again at once
  (`Policy::recastable`): it is an instant or has flash, or it is their main
  phase, and they have an untapped mana source for each point of its mana value.
- `Effect::ends_turn` marks a spell that can end the turn. The hold rule keeps
  such a spell until combat is over while it is the caster's turn and they
  control a creature that could attack (`Policy::before_attacking`).

Neither rule names a card. The first applies to every pure bounce aimed at a
spell, and the second to every spell that ends the turn. Neither touches a card
of the WU Control mirror, so its rollouts are unchanged. A first version that
also covered Reprieve moved one WU run of `spirits-stay-home` from pass to fail.

The suite entry `unsubstantiate-own-dandan` in `data/blunders.json` forbids
casting Unsubstantiate at this decision. Before the fix the champion cast it in
all five runs at 30M steps and in three of five at 3M steps (one more cast Day's
Undoing). After the fix it passed in all ten. The rest of the suite is as it was
on the base commit: 19 entries fixed and `staff-before-combat-damage` open at 4
of 5 runs for each budget.

## The saturation hypothesis

The report and the triage notes suggested that the agent was so far ahead that
every line's value was close to a certain win, so the choice among them had
become arbitrary. The numbers rule this out for this decision. The means were
between 0.49 and 0.60, the bounce led by more than five standard errors, and the
rollouts won about 55% of the time. The search had a real, if wrong, preference,
and that preference came from the rollout policy. No tiebreak among saturated
values was added, because nothing here was saturated.
