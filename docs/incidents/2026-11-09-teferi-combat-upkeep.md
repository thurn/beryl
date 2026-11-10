# Incident: Teferi untaps, combat and upkeep timing (2026-11-09)

Four plays from one WU Control mirror against `pimc72-hidden-30m-anchor`, seed
1794289994865599000, build `6bd3386e`. The
[replay](2026-11-09-teferi-combat-upkeep.replay.json) is the longest recording
from this game. The other reports' logs are exact prefixes. Decision indices
below count both players from zero within each turn.

## Declining both Teferi untaps (by-n81i)

At turn 11 decision 4 (log 93), the delayed ability from Teferi, Hero of
Dominaria's +1 offers the agent five of its own tapped lands and three opposing
lands. It chooses Done before selecting any. Untap Seachrome Coast and Floodfarm
Verge, for example. Its actual hand is Portable Hole, Mana Crypt and Plains, so
the extra mana has no immediate spell to pay for. Taking two own lands costs
nothing and represents interaction on the human's turn. The catalog checks
against immediately ending the prompt, allowing different land choices.

The original search chose Done (6,534 visits, 0.351) over every land, including
Seachrome Coast (6,311, 0.361) and Floodfarm Verge (5,866, 0.356).

## Attacking into the 3/3 Samurai (by-gnlj)

At turn 21 decision 16 (log 373), the agent attacks with its face-down Spara's
Headquarters and sends it at a one-loyalty Teferi. The 6/6 Abhorrent Oculus is
tapped, but the opponent has an untapped 3/3 Samurai and untapped 2/2 manifests.
The agent knows its own manifest is a land and has no combat trick to win the
fight. The Samurai blocks, survives and keeps Teferi alive. Keeping the 2/2
gives the agent another blocker for its two Jaces.

The original search chose the attack (64,597 visits, 0.511) over Done (37,212,
0.479). The attack-target search had three nearly equal estimates, but choosing
a different target still lets the Samurai block.

## Casting Subtlety before drawing (by-g5qd)

At turn 23 decision 7 (log 406), the agent hardcasts Subtlety in its own upkeep
after looting with Currency Converter and declining to exile the discarded Oust.
The stack beneath Subtlety has the human's Abhorrent Oculus manifest trigger and
the agent's Mana Crypt coin-flip trigger. Both are abilities, so Subtlety's
enters ability has no spell to target. It also cannot attack this turn. Pass and
see the draw before committing mana. If the agent still wants a blocker after
seeing the main phase play out, it can cast Subtlety later. Until then it can
use Subtlety against an eligible spell.

The original search reused the preceding decision's trees and chose the cast
(98,040 visits, 0.6984) over pass (88,843, 0.6978).

## Skipping combat after Winds of Abandon (by-6tzy)

At turn 23 decision 24 (log 438), overloaded Winds has cleared all opposing
creatures. The human's Teferi is at two loyalty and The Wandering Emperor is at
one. Beza, a 4/5, can attack Teferi while the Rogue and Fish stay back to cover
the two Jaces. A flashed Subtlety could save Teferi by chumping, but Beza
survives that block. The opposing Emperor is already in play, cannot activate at
instant speed here and has too little loyalty for its exile ability. The agent's
own Subtlety was cast this turn and cannot attack. The catalog checks that Beza
joins combat and leaves the other attack decisions open.

The original search chose Done (52,504 visits, 0.932) over Beza (49,524, 0.935),
Rogue (48,248, 0.934) and Fish (46,695, 0.931).

## Reproduction

```sh
replay=docs/incidents/2026-11-09-teferi-combat-upkeep.replay.json
for turn in 11 21 23; do
  just review "$replay" --agent recorded --turn "$turn"
done
just blunders --only decline-both-teferi-untaps,manifest-into-samurai,subtlety-before-own-draw,skip-beza-after-winds --seeds 1 --work 3M
```
