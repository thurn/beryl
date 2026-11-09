# Incident: Deep Searcher activates Staff too early (2026-11-09)

Two reports from the same WU Control mirror, by-4bo and by-oli, caught Deep
Searcher spending mana on Staff of the Storyteller before it could use the draw.
Both activations were legal. In both positions, passing would have kept a live
answer available.

## Turn 8: The human's upkeep

Deep Searcher had a Plains and two Islands untapped. It held Reprieve, Teferi,
Hero of Dominaria, and Beza, the Bounding Spring. With the human's Mana Crypt
trigger on the stack, it spent its only white mana on Staff. It then used the
Islands for Currency Converter, drew an Island, and discarded the Jace, Vryn's
Prodigy it had just drawn.

Passing on the trigger would have left Reprieve available if the human cast
something worth delaying. If the human gave it no reason to use Reprieve, Deep
Searcher could use Staff and Converter at the end step. Tapping the Plains in
upkeep told the human that white interaction was off the table.

The game's own search (log entry 69) gave Staff 16,227 visits and a mean of
0.390, Currency Converter 16,183 and 0.388, and pass 15,850 and 0.380. The
options were close. The search chose the most visited option.

## Turn 11: Deep Searcher's combat

Subtlety blocked the Rogue attacking Jace. Currency Converter and the
Counterspell that Reprieve returned to hand had already tapped all four Islands.
Deep Searcher spent one of its two untapped Plains on Staff before combat damage
and drew Snapcaster Mage. Its deck had no instant-speed play that could affect
combat for the one white mana left. The Rogue died, and Subtlety survived.

Passing through combat would have preserved both the option to exile Subtlety
with Winds of Abandon in the second main phase and the option to use Staff then
or at the human's end step. The human already knew about Counterspell from
Reprieve, and the four tapped Islands made it uncastable.

At log entry 181, the game's own search gave Staff 28,646 visits and pass
28,245. Both means round to 0.254.

## What the search misses

The rollout policy's `activate` in `rollout.rs` scores an untargeted ability
like Staff 15 whenever the library is safe to draw from, and pass scores zero.
It applies that score in upkeep, combat, and end steps alike. A root pass can
lead straight to an early activation in the rollout, instead of modelling the
value of keeping mana free. The anchor's `guarded` in `long.rs` only compares
casts with a greedy pass, so it does not settle these activation near-ties.

The suite entries `staff-in-upkeep` and `staff-before-combat-damage` expect a
pass. A ladder challenger should defer optional instant-speed activations when
the draw has no immediate use and paying gives up another play. Activations that
can affect the current spell or combat still need to be searched.

## Reproducing

The [replay](2026-11-09-staff-timing.replay.json) is the later report's game.
Search turn 11 for the combat decision, even though the report was filed after
turn 12 began. Adding `--agent recorded` searches that turn afresh.

```sh
just review docs/incidents/2026-11-09-staff-timing.replay.json --turn 8
just review docs/incidents/2026-11-09-staff-timing.replay.json --turn 11
just blunders --only staff-in-upkeep,staff-before-combat-damage
```
