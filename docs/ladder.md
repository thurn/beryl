# The ladder

Champion: `pimc72/hidden/rollout/tuned/anchor+guard`

One row a match or audit, newest first. docs/ai-strategy.md §4 says how each row
is made.

| Date       | Challenger                                         | Champion                                    | Budget | Games | Score | Elo (95%)      | Verdict             | Suite open / fixed | Human games |
| ---------- | -------------------------------------------------- | ------------------------------------------- | ------ | ----- | ----- | -------------- | ------------------- | ------------------ | ----------- |
| 2026-11-13 | turn72/hidden/rollout/tuned/anchor+guard           | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 14 / 7             | ?           |
| 2026-11-13 | pimc72/hidden/rollout/tuned/anchor+guard+mid       | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 14 / 6             | ?           |
| 2026-11-13 | pimc72/hidden/rollout/tuned/anchor+guard+combat    | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 15 / 5             | ?           |
| 2026-11-13 | pimc72/hidden/rollout/tuned/anchor+guard+hold      | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 9 / 11             | ?           |
| 2026-11-13 | pimc72/hidden/rollout/tuned/anchor+guard+order@3M  | pimc72/hidden/rollout/tuned/anchor+guard@3M | 3M     | 1652  | 50.7% | +6 [-13, +24]  | fail                | 13 / 7             | ?           |
| 2026-11-12 | pimc18/hidden/rollout/tuned/anchor+guard           | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 16 / 4             | ?           |
| 2026-11-12 | pimc72/hidden/rollout/tuned/anchor+guard+mid       | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 14 / 6             | ?           |
| 2026-11-12 | pimc72/hidden/rollout/tuned/anchor+guard+combat    | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 15 / 5             | ?           |
| 2026-11-12 | pimc72/hidden/rollout/tuned/anchor+expert+guard@3M | pimc72/hidden/rollout/tuned/anchor+guard@3M | 3M     | 400   | 56.8% | +54 [+17, +92] | yardstick           | 12 / 8             | ?           |
| 2026-11-12 | pimc72/hidden/rollout/tuned/anchor+guard+waste     | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 14 / 6             | ?           |
| 2026-11-12 | pimc72/hidden/rollout/tuned/anchor+guard+hold      | pimc72/hidden/rollout/tuned/anchor+guard    | -      | -     | -     | -              | failed at the suite | 11 / 9             | ?           |
| 2026-11-12 | pimc72/hidden/rollout/tuned/anchor+guard@3M        | pimc72/hidden/rollout/tuned/anchor@3M       | 3M     | 574   | 52.4% | +23 [-10, +57] | pass                | 15 / 5             | ?           |

## Measured on the first match

The spread of pair scores was 0.310 from positions (230 pairs) and 0.256 from
the shuffle (57 pairs). The match took 6.235 wall-clock hours on Cygnus.
