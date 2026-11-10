# Incident: Deep Searcher attacks into Beza and Subtlety

On turn 22 of the WU Control mirror (build `7d72fb5a`), Deep Searcher
(`pimc72-hidden-30m-anchor`) sent its 4/5 Beza at one-loyalty Jace, the Mind
Sculptor and its 2/2 Trinket Mage at four-loyalty Teferi, Time Raveler. The
defender had an untapped 4/5 Beza, 3/3 Subtlety, and two 1/1 Fish. Beza and one
Fish together had lethal damage for the attacking Beza while the defending Beza
could survive. Subtlety could kill Trinket Mage and survive on its own. The
human put both Fish in front of Beza. Both attackers and both Fish died, and
both planeswalkers survived.

At the attacker choice, **Done** keeps both creatures untapped to defend the
agent's own planeswalkers and preserves its future attacks. The defender could
make these blocks using public information, and the agent had no combat trick in
hand. Earlier in the turn, The Wandering Emperor's available +1 could have made
Beza a 5/6 with first strike. That would let Beza attack Jace and kill the
defending Beza before regular combat damage if it blocked. Even if all four
creatures blocked and Beza assigned all five first-strike damage to its
counterpart, the remaining blockers would deal at most five damage.

## Reproduction

The [app replay](2026-11-10-beza-trinket-attack.replay.json) reaches the attack
at turn 22 decision index 5 (log 294) and ends during blocking. The report's
recorded views supply the combat damage and deaths.

```bash
just review docs/incidents/2026-11-10-beza-trinket-attack.replay.json --agent recorded --turn 22
just blunders --only beza-trinket-into-blockers
```

In the game's search, Beza led at 0.845 (51,859 visits), then Trinket Mage at
0.834 (39,255), and Done at 0.769 (13,763). After choosing Jace as Beza's
target, adding Trinket Mage led at 0.845 (59,492) against Done at 0.817
(31,912). Those estimates favored the attack by more than a marginal visit
count. The first damage-division decision began a fresh tree after the human
blocks and its best reported mean was 0.725. These are estimates at different
positions, so their difference alone does not identify a search or rollout
cause.

The fresh recorded-agent search repeated both attacks and their targets. At the
first choice it estimated Beza at 0.840, Trinket Mage at 0.830, and Done at
0.765. The catalog reached the recorded decision without drift. The current
champion chose Beza in all five samples at each of 3M and 30M steps.
