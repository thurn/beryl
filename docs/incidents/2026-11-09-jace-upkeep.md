# Incident: Jace loots before its controller's draw (2026-11-09)

In the WU Control mirror reported as by-cmz, Deep Searcher activated Jace,
Vryn's Prodigy in its own turn 21 upkeep. Its hand was Plains, Abhorrent Oculus,
and Fractured Identity. Its graveyard held seven cards, so the loot could
transform Jace immediately. The activation was optional and cost no mana.

Passing in upkeep preserves the option to activate in the draw step. Jace's
controller receives priority after the draw and can choose whether to loot after
seeing that card, then choose a discard from the larger hand. This position
offered no upkeep effect for an early activation to influence.

At log entry 296, the recorded search gave Jace 60,399 visits and a mean reward
of 0.179, Currency Converter 53,025 and 0.171, and pass 43,681 and 0.158. A
fresh recorded-agent review reached the same decision and favored Converter by
visits, with all three rounded values within 0.001. The acceptance choice is to
pass and take the turn's draw before deciding whether to loot.
