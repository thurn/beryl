# Incident: Subtlety timing and skipped Teferi activation (2026-11-09)

Two reports from a human WU Control mirror against `pimc72-hidden-30m-anchor`,
seed 1794280812505412000, build `01d95aa6`: by-677 (turn 7) and by-nh2 (turn 15,
reported during turn 16). The
[replay](2026-11-09-subtlety-timing-teferi-pass.replay.json) is copied from the
later report and reproduces both decisions without drift.

## Subtlety in the main phase

At turn 7 decision 1, after playing the fourth land, the agent hardcasts
Subtlety into an empty stack. Neither player has a creature. The opponent has
three untapped lands; the agent also holds Reprieve and two Jaces.

Passing preserves both Reprieve and Subtlety as interaction. If no useful spell
appears, casting Subtlety at the opponent's end step gives the same next-turn
attack while avoiding exposure to sorcery-speed removal. This judgment does not
depend on seeing the opponent's hidden cards. Unlike the previous evoke
incident, this cast keeps its creature; the error is timing.

The recorded search gave pass 0.550 and Subtlety 0.544, with 10,860 and 14,049
visits respectively, and selected Subtlety. A fresh recorded-agent search
instead selected Jace, Wielder of Mysteries (0.514, 14,804 visits), with pass
0.500 and Subtlety 0.492. Sampling changes the choice; it does not provide a
reason to spend flash now. The catalog forbids this Subtlety cast rather than
requiring a particular alternative planeswalker play.

## Teferi's unused +1

On turn 15 Teferi, Hero of Dominaria has one loyalty and its controller has 23
library cards. Its +1 is available in both main phases, but the agent passes
both times. The final opportunity is decision 5, counting both players'
decisions from zero. No visible effect punishes drawing, and the library is far
from empty. Activating gives a card, a loyalty counter and delayed land untaps
without spending mana. Passing gives up all three.

The recorded postcombat search valued pass at 0.754 (21,168 visits) versus +1 at
0.729 (15,536). A fresh search repeated the pass: 0.769 (22,396) versus +1 at
0.727 (12,421). This is distinct from the previous reports about selecting the
wrong lands for Teferi's untap trigger: this time the agent never activates the
ability.

## Reproduction and catalog

```sh
just review docs/incidents/2026-11-09-subtlety-timing-teferi-pass.replay.json --agent recorded --turn 7
just review docs/incidents/2026-11-09-subtlety-timing-teferi-pass.replay.json --agent recorded --turn 15
just blunders --only subtlety-main-phase-empty-stack,skip-teferi-plus-one --seeds 1 --work 3M
```

The catalog entries preserve these positions for later ladder work. This change
does not alter the agent or claim its play has improved.
