# AI Plan

*The plan, as of 2026-11-14, for Beryl's AI opponent: what it must be, how it is
built, and the ladder of changes that should raise its strength and remove its
blunders. It replaces `ai-strategy.md` (2026-11-11), whose ladder promoted one
change in three days; §2 says why. The opponent is now built the other way
round. A policy of general rules answers every decision, and the search
overrules it only on evidence that survives a paired test. The session that
works the ladder judges each result itself; no mechanical gate decides.*

## 1. Goal

A strong opponent that **does not cheat** and **plays like a plausible human**.

- **Fair.** It sees what a player sees (`Sight::Hidden`). The cheating searches
  stay in the registry, labeled, as yardsticks.
- **No apparent blunders.** A debatable play is fine. A play no strong player
  would make under any reading is not, whatever the game's result. Humans have
  caught four kinds (§5 lists all 20):
  - **Acting at the first window instead of the last.** Snapcaster Mage cast in
    its own main phase, Staff of the Storyteller activated in an upkeep.
  - **Declining something free.** A search that finds nothing, an untap that
    takes no land, a safe attack not made.
  - **Spending a card on nothing.** Subtlety evoked with no spell to target.
  - **Attacking into blockers that win the fight.**
- **Blunders weigh more than strength.** We will trade some Elo for the last of
  them.
- **General by data, not by code.** A rule reads what a card's definition says
  (its types, keywords, costs, targets) and the game's state. It never names a
  card. Per-card numbers live only in tables (`values.rs`, the per-deck
  overlay).
- **The testbed** is the WU Control mirror until both targets below are met.

Two numbers measure progress, and each has a target:

| Measure      | How it is counted                                                                 | Now                                                | Target       |
| ------------ | --------------------------------------------------------------------------------- | -------------------------------------------------- | ------------ |
| Blunder rate | Suite runs passed, of 400 (20 entries, 3M and 30M, 10 seeds each)                 | 398 of 400                                         | 380 of 400   |
|              | New suite entries per human game, from the app's games ledger                     | 0 in 9 games                                       | under 1 in 5 |
| Strength     | Elo against the reference agent (§3) at 10M steps, over 400 games (`--bar audit`) | +26 Elo (−8 to +60). Score 53.8% (48.9% to 58.6%). | +50          |

The suite total comes from the rung 8 comparison in [the ladder](ladder.md). The
strength estimate and its 95% interval come from the November 16 reference
audit, which tested the champion of three promotions before (`waste`, `walker`
and `turnbudget72` have been promoted since). Nine human games are too few to
say the second target is met. No entries in nine games is still consistent with
one game in four.

## 2. What we learned

### 2.1 From run-003

2,777 games over 54 hours ([longthink-run-003.md](longthink-run-003.md)):

- **Full rollouts win.** Evaluation leaves trailed them by 180 to 260 Elo.
- **Budget stops paying.** Each ×3 of engine steps gained +23 to +57 Elo and
  nothing measurable past 30M.
- **Hidden information costs nothing measurable at 30M.** The omniscient twins
  led by +65 to +160 Elo at 1M to 10M only.
- **Hand-tuned card values beat 17lands win rates** by about 90 Elo.
- **Most moves are coin flips by the search's own estimate.** The chosen move
  led the runner-up by under 0.01 in 62% of decisions.

### 2.2 From the ladder of 2026-11-12 and 2026-11-13

Twelve rungs were tried ([ladder.md](ladder.md)). Three reached a match:

| Rung                           | Suite runs passed (champion, then challenger) | Match                                | Outcome             |
| ------------------------------ | --------------------------------------------- | ------------------------------------ | ------------------- |
| `guard`                        | not recorded                                  | +23 Elo [−10, +57], 574 games at 3M  | Promoted            |
| `hold`                         | 90 → 141 of 200                               | None                                 | Stopped by the gate |
| `combat`                       | 90 → 112 of 200                               | None                                 | Stopped by the gate |
| `waste`                        | Two claims fixed, one entry worse             | None                                 | Stopped by the gate |
| `order`                        | No change                                     | +6 Elo [−13, +24], 1,652 games at 3M | Not stronger        |
| `mid`, `pimc18`, `turn72`      | No claim; one or two entries lower            | None                                 | Stopped by the gate |
| The expert as the root default | 8 of 20 entries fixed                         | +54 Elo [+17, +92], 400 games at 3M  | Yardstick only      |

- **Neither number moved because almost nothing was played.** The champion
  differs from the one of 2026-11-10 by `guard` alone.
- **The gate rejected on noise.** `hold` fixed five claimed entries and was
  stopped because `decline-both-teferi-untaps` passed 17 of 20 runs against the
  champion's 20 of 20. The champion's own 20-seed check of that entry passed 37
  of 40, the same rate.
- **The gate rejected a large gain for missing a threshold.** `combat` took
  `beza-trinket-into-blockers` from 0 of 20 to 16 and `manifest-into-samurai`
  from 1 of 20 to 17, with no entry lower. A claim needed 18, so its row reads
  "failed at the suite".
- **Nobody read the tables.** The plan was written for small models to execute
  without judgement, and they did.
- **No two rungs were ever tried together.** Every challenger was the champion
  plus one option.

### 2.3 Why the search blunders

- **A blunder is a fact about dominance, and the search measures win rate.** An
  untapped land or a play made one step later is worth about 0.001 of a win.
  Paired rollouts have a standard error of 0.009 at 2,000 pairs, so resolving
  0.001 takes about 700M engine steps a decision. No budget fixes this. In the
  62% of decisions that are coin flips, noise chooses, and a human notices
  whenever noise chooses a dominated play.
- **The tree manufactures leads that the rollouts do not have.** At
  `beza-trinket-into-blockers` the tree rated the attack 0.066 above holding
  back. Two thousand paired rollouts rated it 0.006 below, ± 0.009
  (`just why --tree`, 2026-11-12). The same holds at `manifest-into-samurai`,
  `spirits-stay-home` and `skip-teferi-plus-one`.
- **The cause is how the tree explores.** `Tree::select` uses UCB1 with
  `EXPLORATION` 0.7. At the visit counts a node gets (15 to 600), the
  exploration bonus is 0.07 or more, against real differences of 0.01 to 0.05.
  So below the root every child is visited about equally, and a node's mean is
  the value of answering that decision at random. Holding back at the Beza entry
  was scored as the average of 11 follow-up plays, among them a pass rated 0.48.
  An attack is overrated for the same reason: it hands the opponent a wide
  choice of blocks, most of them bad.
- **Comparing means across trees misses that bias.** The old override test let
  the search overrule the default on a lead of 0.02 and two standard errors
  across the trees. The bias was the same in every tree, so it passed. The
  expert alone got 18 entries right and the search anchored to the expert got 8.
- **`order` could not help.** It changes which child is tried first, which does
  not matter when every child ends up visited equally.

### 2.4 Where strength comes from

- **Policy quality is the lever with evidence.** The two large gains on record
  are hand-tuned card values (+90 Elo) and the expert as the root default (+54).
  Budget and hidden information are flat at 30M.
- **The expert's blunder avoidance is general; the rest of it is not.** It
  avoids blunders with three ideas that name no card: it acts only in five kinds
  of window (`Expert::timing`), it scores a play with no worthwhile target at
  zero, and it plays combat out on a copy of the game. Its judgement of what to
  do in those windows is per-card: 47 of its 111 functions name a card, and they
  hold 1,812 of its 2,669 lines, about 45 lines for each of 40 cards. That
  cannot reach 50,000 cards, and every rollout search with tuned card values
  already rates 137 Elo or more above it.
- **The three ideas are already rebuilt from card data.** They are `hold`,
  `waste` and `combat`. None is in the champion.

## 3. The design: the policy plays, the search overrules

- **The policy** is greedy (`Policy`, `rollout/policy.rs`) with the general
  rules set and the combat planner (`combat.rs`) at combat decisions. It is
  deterministic and answers the whole suite in seconds, so its own blunders are
  found and fixed without a match.
- **The policy's answer is the default at every decision.** Today the default is
  held at four kinds of decision only (`guarded`).
- **A sure decision is not searched.** Where taking is free, such as lands to
  untap or a card to find, the policy's answer is played.
- **Elsewhere the search proposes and a paired test decides.** When the search
  prefers another move, both moves are played out in the same worlds with the
  same seeds, and the search's move is played only if it wins that comparison
  clearly.
- **So blunders are the policy's own plus false overrides**, and both can be
  driven down directly. Strength comes from the true overrides and from sounder
  rollouts, since the rollouts play the same policy.

The **champion** is the app's Deep Searcher (`beryl_search::DEEP_SEARCHER`), and
a promotion is a change to it. Today it is
`turnbudget72/hidden/rollout/tuned/anchor+guard+hold+waste+combat+sure+paired+walker@30M`:

| Axis      | Setting                                                                                                                                                                                                                                            |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Method    | Whole-turn search at its own empty-stack priority, scoring as many worlds as the budget allows; elsewhere PIMC, 72 worlds dealt from what it can see, a tree each                                                                                  |
| Sight     | Hidden                                                                                                                                                                                                                                             |
| Leaf      | Softmax rollout to the end of the game, temperature 10                                                                                                                                                                                             |
| Knowledge | Hand-tuned card values (`Knowledge::Tuned`)                                                                                                                                                                                                        |
| Options   | `anchor+guard+paired` checks overrides with paired rollouts. `hold` delays plays, `waste` devalues evoke without a worthwhile target, `combat` plans combat, `walker` spares a planeswalker that is about to win, and `sure` answers free choices. |
| Scores    | Turn discount 0.98: win sooner, lose later                                                                                                                                                                                                         |
| Budget    | 30M engine steps a decision, on every core                                                                                                                                                                                                         |

Every change is a named part of the family's name, so any past champion can be
played again from the current build.

The **reference** is `pimc72/hidden/rollout/tuned/anchor+guard`, the champion of
2026-11-12. It never changes, and the strength target is measured against it.

Yardsticks, never candidates: `expert-frozen`, `greedy`, Deep Seer (the
omniscient twin) and the search anchored to the expert (`anchor+expert+guard`).

## 4. How a rung is run

### 4.1 Where, and who judges

- **Cygnus only.** Every suite, screen, match and audit runs on Cygnus, the
  Windows PC (20 threads, `BERYL_CACHE=D:\beryl-cache`). The position set is at
  `D:\beryl-cache\ladder\positions`.
- **One match at a time.** Each measure ticket depends on the one before it, so
  no two matches run at once. The tickets are the children of the epic by-ix1t.
- **The session that works a measure ticket judges.** It reads each table and
  each match report and decides: promote, drop, stack with another rung, or
  measure again. It writes its reasoning with the numbers in the ticket's notes.
  §4.4 is guidance for that judgement, not a rule that overrides it.
- **It never waits for a person** and never files a ticket asking one to look.
  Where a step fails in a way the ticket does not cover, it writes what
  happened, chooses the sensible course and goes on.
- **Builds run ahead of matches.** A match takes 6 to 18 hours. Build tickets
  are in lanes of their own and are worked on the Mac while a match runs, since
  an option that is off changes no move. Nothing else heavy runs on Cygnus
  beside a match.

### 4.2 A rung is two tickets

- **Build.** Adds the change as an option that is off by default, with its
  tests, and pushes it. Its ticket names the files, the exact rule and the
  tests.
- **Measure.** Runs §4.3 on the pushed build, judges and records. It needs
  judgement, so it is not labeled `tier:light`.

### 4.3 What the measure ticket runs

`CHALLENGER` is the champion's family plus the rung's option.

1. **The policy alone**, when the rung changes the policy:
   `just blunders POLICY --seeds 1`, where `POLICY` is greedy with the
   challenger's rules (§6.1 builds this name). It takes seconds. An entry the
   policy gets wrong cannot be fixed by this rung's champion either, so a miss
   here is read before any match is played.
2. **The suite.** `just gate CHALLENGER --claims CLAIMS`. It runs both agents at
   3M and 30M from 10 seeds and prints, for each entry, the runs each passed of
   20, then each agent's total of 400 (§6.1 changes it to print this).
3. **The screen**, for a rung that changes only the root's choice:
   `just screen CHALLENGER DIR`, where `DIR` is the champion's last match. It
   reports how many decisions a game differ and what they cost by the search's
   own estimate.
4. **The match.**
   `just ladder CHALLENGER --work WORK --bar BAR --positions D:\beryl-cache\ladder\positions --cap 2000`.
   Every seed is played twice with seats swapped. Four pairs in five start from
   the position set. A sequential test ends the match when the evidence is in. A
   match stopped by a crash, a kill or a power cut continues when the same
   command is run again: finished games are kept, and a game in flight goes on
   from its last logged decision. `--cap` may be raised the same way.

| The rung changes        | `WORK` | `BAR`       | The bar                       |
| ----------------------- | ------ | ----------- | ----------------------------- |
| A policy rule           | 1M     | `plausible` | Not weaker: −25 Elo against 0 |
| The root's choice       | 3M     | `plausible` | Not weaker: −25 Elo against 0 |
| The tree or the numbers | 3M     | `stronger`  | Stronger: 0 against +30 Elo   |

5. **The verdict and the record** (§4.6).

**Each command finishes within 30 minutes.** Ignite treats an `ignite run` that
has lasted 45 minutes as stuck, and takes the ticket back from a session still
waiting on it 15 minutes later. A full suite or match takes far longer than
that, so the measure ticket splits each step into commands that each take less
than 30 minutes, run one after another:

- **The suite:** one `just gate` for each budget (`--work 3M`, then
  `--work 30M`), each with its own `--out` directory. If one budget still takes
  too long, run it with fewer `--seeds`, and give the totals as passed of the
  runs played. Add the two tables together for the totals of 400.
- **The screen:** `--games N`, with `N` chosen so that one run takes less than
  30 minutes.
- **The match:** give every run the same `--out` directory, and raise `--cap`
  each time. Start at `--cap 20`, time it, and then raise the cap by as many
  games as fit in 30 minutes, up to 2000. A run that reaches its cap undecided
  prints the score so far. The next run continues the same match from the games
  already played. Stop when the sequential test decides the match or the cap
  reaches 2000.

### 4.4 Guidance for the judgement

- **Read totals before entries.** One entry moves by three or four runs in 20
  between two runs of the same agent. A total that moves by 20 or more of 400 is
  real; `hold` moved it by the equivalent of 100.
- **An entry that falls by six or more runs of 20 deserves a look**, with
  `just why ENTRY --a LABEL --b LABEL --tree --family CHALLENGER`. If the fall
  is real and the rung is otherwise good, promote it and name the entry in the
  row, so that a later rung claims it. One lower entry does not outweigh five
  fixed.
- **A blunder rung is promoted** when the total rises clearly and the match does
  not show it weaker.
- **A strength rung is promoted** when the match accepts the stronger bar and
  the suite total has not fallen by more than noise.
- **A match that ends at the cap undecided** is not a failure. The score and its
  interval go in the row. A blunder rung with a clear suite gain and a score of
  49% or more is promoted on it.
- **Stack what is independent.** Two rungs that each passed the suite and change
  different things (a policy rule and a root default) are played in one match.
  If the stack fails, each is played alone.
- **A rung that fails leaves its option unset.** The session writes one sentence
  on why it thinks it failed, and what that suggests for a later rung.

### 4.5 The audit against the reference

After every third promotion, and once the last audit is seven days old:

`just ladder CHAMPION --vs REFERENCE --work 10M --bar audit --cap 400 --positions D:\beryl-cache\ladder\positions`

It plays all 400 games and prints the score with its 95% interval. This is the
strength number of §1. If the whole interval is below 50%, the session plays
each promotion since the last audit against the champion before it, newest
first, at 10M over 200 games, and unsets the first one that scores under 45%.

### 4.6 The record and a promotion

- `docs/ladder.md` is the log: a `Champion:` line, then one row for each match,
  audit or dropped rung, newest first, as `just ladder` prints it. A row for a
  rung dropped before a match gives both agents' suite totals and the verdict
  `dropped`, with the reason in the ticket.
- The ticket's notes hold the command lines, the tables, the printed result and
  the reasoning. Logs, transcripts and position files are never committed.
- **To promote:** in a worktree, set `DEEP_SEARCHER`'s options in
  `rust/crates/beryl_search/src/lib.rs` to the challenger's; update §3's table
  and §5's table here; put the family on the log's `Champion:` line and add the
  row with the new champion's suite total. Land it with `ignite land`.
- **A promoted policy rule changes what rollouts play.** If the push's slow CI
  fails `counts` or `rollouts_never_change`, that is reason 1 of
  `rust/crates/beryl_bench/count-gate.toml`: update the pinned rollouts and the
  bar in the same change, with the old and new counts in the commit body.

### 4.7 A campaign: a run with no session

A campaign runs several measurements back to back under one command, with no
agent session watching:

`just campaign data/campaigns/<name>.json [--hours N] [--smoke] [--accept-commit]`

- **What it runs.** A plan file lists stages in priority order: building a
  position set (`positions`), running the blunder suite (`gate`), playing a
  paired match (`match`, optionally extending an earlier match to a higher cap
  if it ended undecided), and running the value test over the matches' games
  (`value-test`, which runs last and is exempt from the time limit). Before
  starting the real stages, `just campaign` runs the whole plan in miniature
  (`--smoke`: 10k work, cap 4, 4 positions, 1 gate seed) and stops if any stage
  fails.
- **No promotions during a campaign.** Every match plays the exact families
  named in the plan on one checked-out commit. Nothing changes `DEEP_SEARCHER`,
  `docs/ladder.md`, or `docs/ai-plan.md` while the campaign runs.
- **Directory and report.** State and game logs live under
  `$BERYL_CACHE/campaign/<name>/` (with the smoke preflight in
  `$BERYL_CACHE/campaign/<name>-smoke/`). Each stage runs as its own child
  process and writes to its subdirectory; `state.jsonl` records stage outcomes
  and `report.txt` is rewritten after every stage. Re-running the same command
  after a crash, kill, or reboot resumes from the last unfinished stage and the
  last finished game in it.
- **Judging afterwards.** When the run finishes, a session reads `report.txt`,
  applies §4.4's guidance to each gate and match result, adds a row to
  `docs/ladder.md` for every match stage, updates §1 with any reference audit,
  and promotes any winning challenger under §4.6.

The first plan, `data/campaigns/strength-1.json`, tests four search options, the
10M reference audit, budget scaling, and the value model against the champion
(`CH` =
`turnbudget72/hidden/rollout/tuned/anchor+guard+hold+waste+combat+sure+paired+walker`,
`REF` = `pimc72/hidden/rollout/tuned/anchor+guard`):

| Order | Stage                                                 | Kind                | What it plays                                                                   |
| ----- | ----------------------------------------------------- | ------------------- | ------------------------------------------------------------------------------- |
| 1     | `positions`                                           | `positions`         | 250 champion games at 3M, cut into start positions                              |
| 2     | `bonus-gate`, `bonus`                                 | `gate`, `match`     | Suite, then `CH+bonus` v `CH`, 3M, `stronger`, cap 1200, seed 1                 |
| 3     | `audit`                                               | `match`             | `CH@10M` v `REF@10M`, `audit`, cap 400, seed 2                                  |
| 4     | `fitroll-gate`, `fitroll`                             | `gate`, `match`     | Suite, then `CH+fitroll` v `CH`, 3M, `stronger`, cap 600, seed 3                |
| 5     | `fitroot-gate`, `fitroot`                             | `gate`, `match`     | Suite, then `CH+fitroot` v `CH`, 3M, `stronger`, cap 600, seed 4                |
| 6     | `budget-10`                                           | `match`             | `CH@10M` v `CH@3M`, `audit`, cap 400, seed 5                                    |
| 7     | `budget-30`                                           | `match`             | `CH@30M` v `CH@10M`, `audit`, cap 400, seed 6                                   |
| 8     | `race-gate`, `race`                                   | `gate`, `match`     | Suite, then `CH+race` v `CH`, 3M, `stronger`, cap 1200, seed 7                  |
| 9     | `prior`                                               | `match`             | `CH+prior` v `CH`, 3M, `audit`, cap 100, seed 8                                 |
| 10    | `bonus-ext`, `race-ext`, `budget-30-ext`, `audit-ext` | `match` (`extends`) | `bonus` and `race` to cap 2000 if undecided; `budget-30` and `audit` to cap 800 |
| 11    | `value`                                               | `value-test`        | Value test over all eight matches above                                         |

## 5. The blunder suite

A ledger of every blunder a human has caught: `data/blunders.json`.

- **An entry** is a position (an incident's replay or a rebuilt board), the
  forbidden choices and, where one exists, the required choice. It cites its
  incident and names its class.
- **A run** searches the position once at one budget from one seed. It passes
  when it makes no forbidden choice and every required one. An entry is *fixed*
  when at least 19 of its 20 runs pass.
- **Entries come only from a human's games.** A play they would call wrong goes
  through the bug button (⌘B) and gets an incident write-up, which also records
  what greedy, the policy with its rules, and the expert each play there. If the
  play is judged sound, the write-up says so and no entry is made: Reprieve on a
  recastable Oust and Fractured Identity on the agent's own Mana Crypt were both
  reported and both sound. No model adds an entry from match transcripts.
- **A fix addresses the class.** Each policy rule ships with a test over every
  card in the pool that has the property the rule reads (every card with flash,
  every evoke cost), not only the card in the incident.
- **The human measure** is entries opened per finished human game. The app
  appends each finished game to `games.jsonl` in its data folder on the Mac, and
  each incident write-up records that file's line count. The ledger began on
  2026-11-12.
- **Drift.** An entry whose replay no longer reaches its position is rebuilt as
  a board (`data/blunders/*.json`) or retired with a note. The suite fails on it
  until then.

The champion column shows runs passed of 20 in the rung 8 comparison. The best
measured column records pass counts out of 10 from the gates of 2026-11-12 and
2026-11-13. The Policy column shows one run per entry with
`greedy+hold+waste+combat`. The last column names the rungs of §6 that claim
each entry.

| Entry                              | Greedy | Policy | Champion | Best measured   | Claimed by        |
| ---------------------------------- | ------ | ------ | -------- | --------------- | ----------------- |
| `oust-own-token`                   | right  | right  | 20/20    |                 | Fixed             |
| `skip-beza-after-winds`            | right  | right  | 20/20    |                 | Fixed             |
| `decline-mana-crypt`               | right  | right  | 20/20    |                 | `sure` pins it    |
| `decline-both-teferi-untaps`       | right  | right  | 20/20    |                 | `sure` pins it    |
| `decline-second-teferi-untap`      | right  | right  | 20/20    |                 | `sure` pins it    |
| `snapcaster-sorcery-speed`         | wrong  | right  | 20/20    | 10 with `hold`  | `hold`            |
| `evoke-subtlety-no-target`         | wrong  | right  | 20/20    | 10 with `waste` | `waste`           |
| `staff-in-upkeep`                  | wrong  | right  | 20/20    | 10 with `hold`  | `hold`            |
| `staff-before-combat-damage`       | wrong  | right  | 18/20    | 9 with `hold`   | `hold`            |
| `subtlety-before-own-draw`         | wrong  | right  | 20/20    | 10 with `hold`  | `hold`            |
| `jace-loots-before-draw`           | wrong  | right  | 20/20    | 9 with `hold`   | `hold`            |
| `discard-emperor-over-spare-crypt` | right  | right  | 20/20    | 10 with `hold`  | `hold`, `paired`  |
| `evoke-subtlety-empty-stack-hunt`  | right  | right  | 20/20    | 10 with `hold`  | `hold`, `waste`   |
| `beza-trinket-into-blockers`       | wrong  | right  | 20/20    | 8 with `combat` | `combat`          |
| `manifest-into-samurai`            | right  | right  | 20/20    | 8 with `combat` | `combat`          |
| `subtlety-main-phase-empty-stack`  | right  | right  | 20/20    | 4               | `paired`          |
| `spirits-stay-home`                | right  | right  | 20/20    | 5 with `hold`   | `paired`, `first` |
| `skip-teferi-plus-one`             | right  | right  | 20/20    | 4 with `turn72` | `first`           |
| `samurai-into-jace`                | wrong  | right  | 20/20    | 4 with `hold`   | `hold`, `paired`  |
| `spare-jace-wielder`               | wrong  | wrong  | 20/20    | 4 with `hold`   | `walker`          |

## 6. The rungs

In order. First the tools. Then what is already built is played, stacked. Then
the root is rebuilt around the policy. Then the two strength rungs. The session
may reorder rungs when a result suggests it, and says why in the ticket.

| Order | Rung                       | Aim      | Changes         | Built  |
| ----- | -------------------------- | -------- | --------------- | ------ |
| 0     | Tools                      |          |                 | Partly |
| 1     | `sure` + `hold` + `combat` | Blunders | Policy and root | Yes    |
| 2     | `paired`                   | Blunders | Root            | Yes    |
| 3     | `first`                    | Both     | Root            | No     |
| 4     | `prior`                    | Strength | Tree            | No     |
| 5     | `waste`                    | Blunders | Policy          | Yes    |
| 6     | `fit`                      | Strength | Numbers         | No     |
| 7     | `walker`                   | Blunders | Planner         | Yes    |
| 8     | `turn72`                   | Strength | Method          | Yes    |
| 9     | `bonus`                    | Strength | Scores          | Yes    |

### 6.1 Tools

Two tickets, before rung 1.

- **`just gate` reports and does not veto.** It runs 10 seeds, prints each
  entry's runs passed of 20 for both agents, marks an entry whose counts differ
  by six or more, and prints each agent's total of 400. It prints no
  `GATE: PASS` or `GATE: FAIL` and exits 0 unless an entry drifted. The rerun
  round is removed (`beryl_cli/src/gate.rs`).
- **The policy as an agent.** `greedy+hold+waste+combat`, with any subset of
  those names, is greedy with those rules set and, with `combat`, the planner's
  answer at an attacker, an attack's target, a blocker and a block's target. It
  is what `just blunders POLICY` and a quick `just tournament` take. `greedy`
  alone plays as today.
- **`just gate` also prints a `dropped` row** for `docs/ladder.md` from the two
  suite totals, for a rung that plays no match.

### 6.2 Rung 1: `sure`, `hold` and `combat`, stacked

`hold` and `combat` are built and measured alone (§2.2). `sure` is new and
small. They change different things, so one match judges all three. If it fails,
`hold` is played alone at 1M and `sure+combat` at 3M.

**`sure`: a free choice is not searched.** With the option set, at any
`DecisionKind::ChooseUpTo` the root plays the policy's answer without searching.
These are the choices of lands to untap and of a card to find. Three entries
depend on them. The champion passes them today, and four of the gates of
2026-11-12 and 2026-11-13 saw one of them fall by noise. The build ticket reads
how `decline-mana-crypt` reaches its find: if a yes-or-no question is asked
before it, `sure` answers that with the policy too.

**`hold`: act at the last window** (`Rules::hold`, built). A cast or an
activation at instant speed scores as a pass until the opponent's end step.
Instant speed is any cast or activation offered outside the player's own main
phase with an empty stack; in that main phase it is an instant, a spell with
flash, or an activated ability with no loyalty cost. It is not held when:

1. it is the opponent's end step;
2. an opponent's spell is on top of the stack and the play can target a spell;
3. it is the opponent's turn and a creature is attacking;
4. it is the player's own combat and the play has a target;
5. it is the player's own end step and their hand holds more than seven cards;
6. it draws a card, in the player's own main phase with an empty stack.

**`combat`: the planner is the default at combat** (`Options::combat`, built).
The planner plays each attack or block it weighs to the end of combat on a copy
of the game and scores the position left. With the option set, its answer is the
root's default at an attacker, an attack's target, a blocker and a block's
target, and it answers the combat of the root's own turn inside the tree for
both players.

- **Claims:** the ten entries marked `hold` or `combat` in §5.
- **Measure:** a root rung, 3M, `plausible`.

### 6.3 Rung 2: `paired`, the override decided by paired rollouts

The original anchor spends the last quarter of its work on its own choice and
the default inside the trees and compares their means (`check`, `AnchorCheck`).
The champion uses that quarter to play pairs outside the trees.

1. Deal a world from what the player sees, as a tree's root does.
2. On two copies of it, play the search's choice on one and the default on the
   other.
3. Play both to the end with the rollout policy from the same rollout seed
   (`soft_rollout_within`), and score each as the search scores a finished game
   (`value`).
4. Repeat until the quarter is spent. The lead is the mean of the differences
   and its error is their standard error.

The search's choice is played only if the lead is at least `ANCHOR_MARGIN`
(0.02) and at least three standard errors. Otherwise the default stands.

- **What to expect.** At 30M the quarter buys about 6,500 pairs, a standard
  error near 0.005. At 3M it buys 650 pairs and an error near 0.015, so at 3M
  the search overrules only on leads of about 0.045 or more.
- **What it gives up.** A paired playout prices a move by what the policy does
  after it. A plan that needs a second non-policy move is not confirmed. The
  tree found few of those: it is four or five decisions deep.
- **Tests.** At each of `beza-trinket-into-blockers`, `manifest-into-samurai`,
  `spirits-stay-home` and `subtlety-main-phase-empty-stack`, a seeded search
  with `paired` plays the default. The paired lead of a move against itself is
  zero.
- **Claims:** `subtlety-main-phase-empty-stack`,
  `discard-emperor-over-spare-crypt`, `samurai-into-jace`, `spirits-stay-home`.
- **Measure:** a root rung, 3M, `plausible`. The screen first.

### 6.4 Rung 3: `first`, the policy's answer at every decision

With `first` set, `guarded` holds the default at every kind of decision, as
`Options::expert` does for the expert, and the paired test of rung 2 decides
each override. The default is the policy's answer, or the planner's at combat.

- **Why after `paired`.** With the tree's own test, the expert as a default at
  every decision kept 8 of its 18 right answers (§2.3).
- **What it should fix.** `skip-teferi-plus-one`: greedy takes the +1, the
  search prefers to pass by 0.03, and paired rollouts prefer the +1 by 0.029.
  Today nothing guards a pass chosen over greedy's action.
- **What it may cost.** The search's own estimate was 0.04 to 0.07 a game for
  guarding every decision. That estimate came from the tree's means, so the
  match is the first real measurement.
- **Claims:** `skip-teferi-plus-one`, `spirits-stay-home`.
- **Measure:** a root rung, 3M, `plausible`. After it, run the audit (§4.5)
  whatever its date, since this rung changes the most moves.
- **Result:** the suite rose from 351/400 to 353/400. Teferi's +1 rose from 1/20
  to 20/20 and the targetless Subtlety evoke fell from 17/20 to 0/20. At 3M,
  `first` scored 41.5% over 294 games against `paired` (−80 Elo, 95% interval
  −131 to −32). `first` was dropped. The `waste` rung will test a policy rule
  for the targetless evoke.

### 6.5 Rung 4: `prior`, the tree explores as the policy plays

With `prior` set, `Tree::select` weights a child's exploration by the policy's
score of it, in place of UCB1's equal bonus.

- **The rule.** A child's bound is its mean plus
  `C × P(child) × sqrt(parent visits) / (1 + child visits)`. `P` is the softmax
  of the policy's scores at `ROLLOUT_TEMPERATURE`, the same weights a rollout
  draws from. A candidate the policy does not score takes an equal share. The
  first visit goes to the child with the highest `P`.
- **Why.** Weighting exploration aims to make a node's mean reflect sensible
  play by both players. An attack's score should depend less on weak blocks.
- **`C`.** The build ticket tries three values of `C` on `just why --tree` at
  the four clear-lead entries and takes the one whose tree lead is closest to
  the paired rollouts' lead.
- **Tests.** A child the policy scores far below its siblings gets fewer visits
  than under UCB1 at the same budget. At `beza-trinket-into-blockers`, the
  tree's lead for the attack is under 0.02.
- **Claims:** none.
- **Measure:** the tree, 3M, `stronger`.

`prior` was dropped after scoring 23.3% over 60 games at 3M against the champion
(−174 Elo, 95% interval −309 to −73). The suite gained 18 passes to reach 369 of
400\. It fixed the missed Teferi +1 and the main-phase Subtlety cast, while the
targetless Subtlety evoke fell from 17 to 9 of 20.

### 6.6 Rung 5: `waste`

Built (`Rules::waste`). A creature cast for its evoke cost is scored by its
targeted enters abilities, so with nothing worth targeting it scores 0. Alone it
fixed both evoke entries and took `snapcaster-sorcery-speed` from 9 of 10 to 3.
`turn72` did the same to that entry, so the fall is real. With `hold` in the
champion the Snapcaster cast is held by rule. If the entry still falls, the
measure ticket reads it with `just why` before a match.

- **Claims:** `evoke-subtlety-no-target`, `evoke-subtlety-empty-stack-hunt`.
- **Measure:** a policy rung, 1M, `plausible`.

`waste` was promoted after scoring 62.8% over 148 games at 1M (+130 Elo, 95%
interval +55 to +217). The suite rose from 351 to 356 of 400. The targetless
Subtlety evoke rose from 17/20 to 20/20, and Snapcaster stayed at 20/20. The
main-phase Subtlety cast fell from 13/20 to 11/20.

### 6.7 Rung 6: `fit`, the policy's numbers fitted to the search

The policy's card values (`values.rs`) and the constants of its rules
(`rollout/policy.rs`) were set by hand. This rung fits them.

- **The labels.** Every searched decision of a ladder match is logged with its
  candidates' visits and means and its override test (`DecisionRecord`,
  `AnchorRecord` in `beryl_cli/src/match_log`). A decision where the paired test
  overruled the policy says the policy's score order was wrong there. A match
  holds a few thousand of these. If that is too few, the build ticket adds a
  labelling run: paired rollouts of the policy's top three candidates at
  positions sampled from match games.
- **The fit.** `just fit <match-dir>... --seed 0 --out <file>` replays the
  labelled decisions and adjusts one number at a time to rank more confirmed
  moves first. It penalises distance from the hand-set values and assigns
  roughly a fifth of the starting positions to the holdout, grouping repeat
  starts and games with swapped seats.
- **The output** is a Rust table for the `fitted` knowledge, beside `tuned` and
  `estimate`. The challenger is `pimc72/hidden/rollout/fitted/...`. The fit
  adjusts card values, targeting thresholds and battlefield value adjustments.
- **A check before the match.** The fitted policy plays the tuned one alone,
  20,000 games. It must not score under 50%.
- **A caution.** A policy that agrees with the search more does not have to make
  better rollouts. Only the match says.
- **No new model.** A model trained to imitate the search is §9, not this rung.
- **Claims:** none.
- **Measure:** the numbers, 3M, `stronger`.

### 6.8 Rung 7: `walker`, the attack that leaves a lethal planeswalker alive

At `spare-jace-wielder` the base planner prefers killing the higher-valued
planeswalker, although the other one threatens a library win. With `walker`, the
planner adds a premium to a surviving planeswalker whose loyalty abilities and
draw replacement or explicit win effect give it a short clock. It reads the
ability definitions, current loyalty and library size. Test it without search
using `just blunders greedy+hold+waste+combat+walker --seeds 1`.

- **Claims:** `spare-jace-wielder`.
- **Measure:** a root rung, 3M, `plausible`.

`walker` took `spare-jace-wielder` from 0 to 20 passes of 20 and the suite from
356 to 376 of 400, with no entry lower. Its 3M match scored 50.25% over 2,000
games and ended undecided at the cap, so it was promoted under §4.4.

### 6.9 Rung 8: Search over whole turns

`turn72` proposes continuations of the player's turn with the opponent passing
and scores them in up to 48 fresh worlds. With `walker` enabled on both sides,
it improved the suite from 376 to 396 of 400 but lost the 3M match against
`pimc72` with a score of 40.4%.

`turnbudget72` raises the scoring limit to 4,096 worlds and stops at the
decision's work budget. It requires at least 48 complete scoring worlds before
it can override the rollout policy. Outside the active player's empty-stack
priority, it uses PIMC as `turn72` does.

With `walker` enabled, the budgeted search passed 398 of 400 suite runs against
376 for PIMC. Teferi rose from 5 to 20 passes out of 20 and Subtlety from 11 to
20\. Staff before combat damage fell from 20 to 18. At 3M the budgeted search
scored 54.3% over 600 held-out games and 52.6% over 1,200 fresh confirmation
games. Both matches ended undecided under the `stronger` bar and meet the
capped-match guidance for a blunder rung in §4.4. The short 10M match was
inconclusive. `turnbudget72` was promoted under §4.4's rule for a blunder rung:
a clear suite gain and a capped match score of 49% or more. Its Elo intervals in
both matches lie above zero. The 30M suite measures blunders, so the measured
configuration is the 3M family:

```text
turnbudget72/hidden/rollout/tuned/anchor+guard+hold+waste+combat+sure+paired+walker@3M
```

With `race` (`Options::race`), built but not yet measured, the budgeted search
stops scoring root moves that are clearly losing. It checks after the first 48
scoring worlds and then after every 16. At a check, the leader is the root with
the highest mean reward. A root is dropped when the leader's mean lead over it,
in the worlds both were scored in, exceeds 3 standard errors of that paired
difference (`RACE_ERRORS`). The default's root and the leader are never dropped,
and a dropped root is never chosen. The roots still in the race are scored in
every later world with that world's seed, so their rewards stay paired, and the
work saved becomes more worlds for them. When only the default is left, scoring
stops. `race` requires `turnbudget72`.

- **Claims:** more scoring worlds for the roots that matter at the same budget.
- **Measure:** the suite and a 3M match against the champion, `stronger`.

### 6.10 Rung 9: `bonus`, speed as a bounded bonus

Built (`Options::bonus`), not yet measured. Every family scores a win
`0.5 + 0.5 * 0.98^t` and a loss `0.5 - 0.5 * 0.98^t`, `t` the turns since the
root (`TURN_DISCOUNT`). So a certain loss on turn 13 scores 0.115 and one on
turn 3 scores 0.029: stalling ten turns is worth more than a line that wins one
game in twenty. When ahead, winning six turns sooner is worth risking five games
in a hundred. The discount was added so that noise would not choose among moves
in a decided position, and the `paired` test now does that job.

With `bonus`, a finished game scores
`SPEED_WEIGHT * discounted(p0, t) + (1 - SPEED_WEIGHT) * p0`, with
`SPEED_WEIGHT` 0.02 and `discounted` the full discount above. So speed is worth
at most 0.01 of a win, and a win scores at least 0.99 however late. An
evaluation leaf is scored the same way from its estimate.

- **Claims:** none.
- **Measure:** the numbers, 3M, `stronger`.

### 6.11 Rung 10: `cede`, stop once the default cannot be beaten

Built (`Options::cede`), not yet measured. An anchored search with `paired`
spends its first 12 of 16 rounds (`FREE_ROUNDS`) growing its trees freely. Only
a challenger that leads after those rounds gets the paired check against the
default answer, with the work the last four rounds would have used. When the
default leads instead, the last four rounds still run today, and a challenger
that first leads in them is checked with whatever work the trees left unspent,
usually none. The ordinary stop needs the leader settled over the whole budget,
and at a decision whose candidates score alike, such as passing against a minor
instant-speed play in combat, it rarely is. Those decisions spend the full 30M
steps, two to three seconds, and then play the default.

With `cede`, the search stops as soon as the default leads on visits by more
than the rest of the free rounds could overturn, and skips the last four rounds
whenever the default leads after the free rounds. It requires `paired` and
excludes `means`. The move changes only where a late challenger would have won
its check on leftover work.

- **Claims:** none; it makes those decisions faster for a player watching.
- **Measure:** the suite, the screen against the champion's last match, and a 3M
  match against the champion, `plausible`.

## 7. Speed

- **A faster engine is more verdicts a day.** A match is 6 to 18 hours, so a
  rollout twice as fast doubles what the ladder learns in a week. It adds little
  strength by itself, since budget is flat at 30M.
- The rollout benchmark and `count-gate.toml` stay the measure
  ([benchmarking.md](benchmarking.md)).
- A speed-up that keeps every answer (`rollouts_never_change` pinned) changes no
  move, so it needs no match.
- A policy rule's code runs only when its rule is set, out of line, so the
  benchmark's count does not move until a promotion (§4.6).

## 8. Not now

- **Entries found by a model.** Suite entries come only from human games (§5).
  `just blunder-hunt` remains for a machine that would otherwise sit idle, and
  what it finds is not an entry.
- **Forge's and XMage's AI hints.** In Forge, 5% of 34,132 card scripts carry an
  `AILogic` tag, and each tag selects hand-written code, so the tags alone carry
  little. No card data or design may come from Forge, which is also GPL-3. A
  count of its tags by kind, as `just census` counts mechanics, may later rank
  which general rules a larger card pool needs first.
- **Card-specific code.** Rules read definitions; numbers are table entries.
- **A learned value model, or self-play training from scratch.** Evaluation
  leaves lost by 180 to 260 Elo here. In Monolith, R-NaD learned nothing in
  145,000 games. In netrunner-rs, 0 of 6 AlphaZero-style runs were promoted.
- **A larger budget, and fewer worlds.** 90M rated no higher than 30M, and
  `pimc18` lowered the suite.
- **A second deck.** The mirror is the only testbed until §1's targets are met.
  `hold` rests on dominance, which does not depend on the deck. Card values and
  the planner's score do, and will need another deck to test them. The app also
  offers the shared-deck format's Dandân deck (`decks::FORGETFUL_FISH`), which
  the overlay has no entry for. For a game whose cards the overlay tunes none
  of, basic lands aside, every tier of the champion values cards as
  `Knowledge::Estimate` does, from their definitions, with no override
  (`overlay::tunes`). The work on that deck stops at legality: seeded games
  against every agent end with every answer legal, and nothing measured its
  strength.
- **A solver that weighs every attack and every block.** The planner weighs a
  small set of plans. Four creatures a side is 16 attacks by 625 blocks.
- **The Mac.** It plays no ladder games.

## 9. The next architecture: a policy model distilled from the search

Not a rung yet. It is recorded so that the rungs above are built toward it.

- **The problem.** General rules scale with the number of mechanics, which is
  hundreds. Card values scale with the number of cards, which is 50,000, and
  nobody can set them by hand.
- **The design, in outline.** A small model reads features of a decision taken
  from card definitions (the effect, its cost, its targets, the step) and the
  state. It is trained to rank the move the paired test confirmed. It then
  serves as the default, as the rollout policy and as the tree's prior, and the
  search built on it labels the next round. Each round is one step of policy
  improvement.
- **Why this differs from what failed.** It is supervised learning from a
  teacher that already beats the expert by over 137 Elo, with no value network
  and no self-play from random play.
- **What it needs first.** The paired test supplies labels for moves that beat
  the policy. Rung 6 fitted card values and policy constants to those labels.
  Held-out agreement rose from 2.78% to 12.15%, and fitted greedy won 65.1% of
  20,000 games against tuned greedy. In search, the blunder suite rose from 356
  to 381 of 400, but fitted scored 46.3% in 322 paired games at 3M steps (−30
  Elo, 95% interval −71 to +11) and failed the stronger bar. The fit changed the
  search's default and rollout policy together. The options `fitroll` and
  `fitroot` change one at a time (§10).
- **What it keeps.** The general rules stay as hard limits on the model, so that
  a model's mistake cannot become a blunder of a known class.
- **Testing a value model first.** A learned value model would replace or steer
  rollouts, so it can only help if it judges positions better than the search
  already does. `just value-test <match-dir>...` measures this from ladder
  matches. At each searching entrant's first searched decision of each turn, it
  asks who wins. Four predictors answer: the search's own mean reward for the
  move it played, rescaled by a fitted `sigmoid(a * logit(x) + b)` so that the
  turn discount's pull toward 0.5 is not held against it; a logistic regression
  and a network with one hidden layer of 32 units, both reading counts the
  decider can see (life, cards, lands, creatures and each deck card's copies by
  zone); and the network with the search's estimate as one more input. Every
  number is scored on games the model was not trained on, in five folds by start
  position. Read the two differences in log loss against the search. If the
  network's 95% interval lies below zero, a model trained on that many games
  already sees something the search misses. If the interval for the network
  given the search's estimate lies below zero while the plain network's does
  not, the model adds to the search rather than replacing it. If both lie above
  zero, more games or better features come before any value network. `eval.rs`'s
  eight-feature evaluation is not this comparison: it was fitted on greedy games
  and lost 180 to 260 Elo as a leaf.

## 10. Open questions

- What does `hold` do to strength? Its suite gain is the largest on record and
  it has never played a match.
- How often does the paired test overrule the policy, and how much of the
  search's strength lives in those overrides? Rung 3's match and screen say.
- How much of rung 6's match result (§9) came from changing the default move,
  and how much came from changing the rollout policy? Two options on `tuned`
  knowledge answer it. With `fitroll`, the root keeps the tuned values and
  everything simulated (tree expansion, rollouts, the paired test's playouts and
  the turn search's lines) uses the fitted ones. With `fitroot`, the root's
  default move, its `guarded` test, its `sure` answers and its combat planner
  use the fitted values, and everything simulated stays tuned.
- Why does `snapcaster-sorcery-speed` fall under both `waste` and `turn72`?
- Is the planner's small set of plans enough? `spare-jace-wielder` is the case
  it gets wrong.
- Do gains measured at 1M and 3M hold at 10M? The first audit says.
