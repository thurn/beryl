# Incident: Three confirmed hunt mistakes (2026-11-09)

The human confirmed these three classes after reviewing the 20 shortlisted
positions from hunt `1794244446-57190`: WU Control mirror, shipped 30M work,
`pimc72/hidden/rollout/tuned/anchor`. These are open regression examples, not
claims of fixes or proof that each mistake changed the winner. Review used only
information available to the acting player.

The fixtures in `data/blunders/2026-11-09-reviewed-hunt-*.json` preserve the
seed and semantic choices through the recorded mistake. They use the hunt's
SMART profile for both seats, rather than an app replay's human priority stops.
Game and decision numbers below are the hunt's zero-based identifiers; suite
indices count decisions within the specified turn.

## Discarding a useful spell instead of surplus mana

`discard-emperor-over-spare-crypt`: game 35, decision 142, P0, turn 16. At 4
life, P0 discards The Wandering Emperor to Thirst for Discovery, then Stock Up,
keeping another Mana Crypt despite ample mana and a Crypt already in play.
Discard the spare Crypt instead: Emperor can supply a blocker and future value.
The opponent's Teferi prevents flashing Emperor in on their turn, but it remains
castable in P0's main phase. Beza subsequently gains life; the finding is poor
card selection, not a proven immediate loss.

The entry forbids discarding Emperor at this decision without prescribing the
second discard or an entire turn's line.

## Declining a free land untap

`decline-second-teferi-untap`: game 4, decision 78, P0, turn 14. P0 selects one
Island for Teferi's end-step untap, then stops with another own tapped Island
available. Taking the second untap costs nothing and preserves options with No
More Lies and Reprieve in hand. The entry forbids stopping here. Game 35,
decision 159 repeats the same mistake; it is corroboration, not a fourth class.
P0 later wins game 4, which does not make the lost option useful.

## Evoking Subtlety without a target

`evoke-subtlety-empty-stack-hunt`: game 27, decision 151, P0, turn 16. With an
empty stack, P0 evokes Subtlety, pitches Abhorrent Oculus, and sacrifices
Subtlety. No creature or planeswalker spell can be affected. Passing preserves
both cards. Game 9, decision 160 repeats the mistake, pitching Fractured
Identity during upkeep with no eligible spell on the stack. These occurrences
were found in the surrounding transcripts, outside the 20 shortlisted decisions.

This belongs to the existing `evoke-subtlety-no-target` class from
[the earlier incident](2026-11-08-subtlety-spirits-samurai.md), rather than a
new Subtlety-specific rule. The new position is another example of that class.

## Follow-up and reproduction

Rung 4 (`by-97g.8`, rollout common sense from card data) owns these examples:
future acceptance should claim these three entries and regress no previously
fixed entry. Changes must handle surplus resources, free beneficial choices, and
casts with no useful effect generally, rather than special-case these cards.
This adds evidence to future evaluation, not a new condition on past promotions.
The relevant code is `beryl_search/src/rollout.rs`, `values.rs`, and `long.rs`;
`beryl_cli/src/blunders.rs` checks replay drift and verdicts.

```sh
just blunders --only discard-emperor-over-spare-crypt,decline-second-teferi-untap,evoke-subtlety-empty-stack-hunt
```
