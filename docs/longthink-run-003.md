# Long-think run-003

The third run of the long-think arena (`just longthink`, see
[ai-strategy.md §5](ai-strategy.md#5-how-we-get-there)), in the cb8d5d5f WU
Control mirror. It ran from 2026-11-06 13:41 to 2026-11-08 19:15 (about 54 hours
wall clock, 104 game-hours at two games at a time) and played 2,777 games. Its
data is in `$BERYL_CACHE/longthink/run-003`: `summary.txt`, `games.jsonl`,
`decisions.jsonl` (378k searched decisions) and per-game logs and transcripts.

## Setup

- Nine families, each varying one axis from `ismcts18/hidden/rollout/tuned`:
  - PIMC with 18, 72 or 288 worlds.
  - Omniscient sight for `pimc18` and `ismcts18`.
  - Leaves cut off after two turns and evaluated (`cut2`).
  - Card values from 17lands (`17l`) or estimated (`estimate`).
- Budgets started at 1M, 3M, 10M and 30M engine steps a decision and grew by ×3
  while they gained. Six families reached 90M.
- `expert-frozen` was the anchor at 0. `greedy` was listed but never scheduled.
- Seed 0, 18 threads. Earlier runs (001: 479 games, 002: 673) also had `hands`,
  `eval` and `cut1` families. Run-003 dropped them.
- Things later runs changed:
  - There was no turn discount (`TURN_DISCOUNT`, by-a8g), so every win counted
    alike.
  - There were no anchored or `mid` families (by-7wl.7 was dropped).
  - Rollouts were softmax over greedy, with random ties (8376cce1).
  - Decisions recorded before the run adopted 45d1b339 log pooled means. Those
    favour each tree's own leader (Simpson's paradox,
    [incident](incidents/2026-11-08-searcher-mulligan-means.md)). Use visits, or
    the per-tree means from later decisions. Moves were never affected.

## Results

Elo against `expert-frozen`, 95% interval. +356 Elo is about an 89% expected
score; +140 is about 69%.

| Entrant                                 |  Elo | 95% | Games |
| --------------------------------------- | ---: | --: | ----: |
| `ismcts18/omniscient/rollout/tuned@10M` | +389 | ±61 |   214 |
| `pimc18/omniscient/rollout/tuned@10M`   | +388 | ±62 |   212 |
| `pimc18/hidden/rollout/tuned@30M`       | +356 | ±74 |   118 |
| `pimc18/hidden/rollout/tuned@10M`       | +322 | ±62 |   186 |
| `pimc288/hidden/rollout/tuned@30M`      | +295 | ±83 |    84 |
| `ismcts18/hidden/rollout/tuned@30M`     | +283 | ±79 |    94 |
| `pimc72/hidden/rollout/tuned@10M`       | +271 | ±68 |   137 |
| `ismcts18/hidden/rollout/estimate@30M`  | +245 | ±76 |   100 |
| `ismcts18/hidden/rollout/17l@30M`       | +189 | ±78 |    90 |
| `ismcts18/hidden/rollout/tuned@1M`      | +147 | ±65 |   148 |
| `ismcts18/hidden/cut2/tuned@10M`        |  +34 | ±60 |   134 |
| `ismcts18/hidden/cut2/tuned@1M`         |  -60 | ±61 |   128 |

The full table, with every budget, is in the run's `summary.txt`.

### Findings

- **Search with full rollouts beats the expert.** At 1M steps every rollout
  family with tuned or estimated card values rates +137 or more, and from 3M to
  30M every PIMC family rates above +210. Only `17l@1M` (+56 ±63) is within
  noise of the expert.
- **The best fair agent is `pimc18/hidden/rollout/tuned@30M`** at +356 ±74. The
  run gave it a 52% chance of being the best fair entrant. It takes about 4.7
  minutes a game.
- **Budget pays, but less and less.** Each ×3 gains +23 to +57 Elo, fitted over
  each family's range. Gains flatten past 10M–30M. Of the six families that
  reached 90M, only `cut2` rated higher there than at 30M, within noise. The 90M
  entrants played few games (4–48) and their intervals are wide.
- **Seeing everything is worth about +65 to +160 Elo at 1M–10M**, comparing each
  hidden search with its omniscient twin at the same budget. At 30M the gap is
  within noise (−57 ±133 for `pimc18`, +9 ±133 for `ismcts18`). The cheats peak
  at 10M, and both rank only a little above the best fair agent.
- **Full rollouts beat board evaluation by a wide margin.** `cut2` rates about
  180–260 Elo below its full-rollout twin at every budget, and at most +48. It
  is the only family near the expert's level.
- **More worlds do not help PIMC.** At 30M, 18 worlds rate +356, 72 rate +262
  and 288 rate +295. At 10M, 18 worlds also lead. PIMC with 18 worlds edges
  ISMCTS with 18 trees in the hidden game (+356 vs +283 at 30M), within the
  intervals.
- **Hand-tuned card values are best, and 17lands values are worst.** At 30M:
  tuned +283, estimate +245, 17lands +189. At 1M, 17lands lags by about 90.

### What this means for the plan

- Full-game rollouts with modest determinization are the strongest recipe found.
  A board evaluation cannot replace them as a leaf yet: the evaluator in step 3
  must improve before shallow search competes.
- Strength rises up to 10M–30M steps a decision, so engine speed (step 2) still
  buys strength up to there, and less beyond.
- Hidden information costs little once search runs long: the cheats sit close to
  the best fair agent, so they no longer bound much.
  `pimc18/hidden/rollout/tuned@30M` is a usable teacher and yardstick for steps
  3 and 4.
- 17lands' games-in-hand win rates are a poor card value for rollouts in this
  mirror.
