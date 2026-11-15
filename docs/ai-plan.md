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

| Measure      | How it is counted                                                                 | Now          | Target       |
| ------------ | --------------------------------------------------------------------------------- | ------------ | ------------ |
| Blunder rate | Suite runs passed, of 400 (20 entries, 3M and 30M, 10 seeds each)                 | 351 of 400   | 380 of 400   |
|              | New suite entries per human game, from the app's games ledger                     | 0 in 9 games | under 1 in 5 |
| Strength     | Elo against the reference agent (§3) at 10M steps, over 400 games (`--bar audit`) | 0            | +50          |

The suite total comes from the rung 2 comparison in [the ladder](ladder.md).
Nine human games are too few to say the second target is met. No entries in nine
games is still consistent with one game in four.

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
`pimc72/hidden/rollout/tuned/anchor+guard+hold+combat+sure+paired@30M`:

| Axis      | Setting                                                                                                                                   |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Method    | PIMC, 72 worlds dealt from what it can see, a tree each                                                                                   |
| Sight     | Hidden                                                                                                                                    |
| Leaf      | Softmax rollout to the end of the game, temperature 10                                                                                    |
| Knowledge | Hand-tuned card values (`Knowledge::Tuned`)                                                                                               |
| Options   | `anchor+guard+paired` checks overrides with paired rollouts. `hold` delays plays, `combat` plans combat, and `sure` answers free choices. |
| Scores    | Turn discount 0.98: win sooner, lose later                                                                                                |
| Budget    | 30M engine steps a decision, on every core                                                                                                |

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
- **One match at a time.** Each measure bead depends on the one before it, so no
  two matches run at once. The beads are the children of the epic by-ix1t.
- **The session that works a measure bead judges.** It reads each table and each
  match report and decides: promote, drop, stack with another rung, or measure
  again. It writes its reasoning with the numbers in the bead's notes. §4.4 is
  guidance for that judgement, not a rule that overrides it.
- **It never waits for a person** and never files a bead asking one to look.
  Where a step fails in a way the bead does not cover, it writes what happened,
  chooses the sensible course and goes on.
- **Builds run ahead of matches.** A match takes 6 to 18 hours. Build beads are
  in lanes of their own and are worked on the Mac while a match runs, since an
  option that is off changes no move. Nothing else heavy runs on Cygnus beside a
  match.

### 4.2 A rung is two beads

- **Build.** Adds the change as an option that is off by default, with its
  tests, and pushes it. Its bead names the files, the exact rule and the tests.
- **Measure.** Runs §4.3 on the pushed build, judges and records. It needs
  judgement, so it is not labeled `tier:light`.

### 4.3 What the measure bead runs

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
   the position set. A sequential test ends the match when the evidence is in.

| The rung changes        | `WORK` | `BAR`       | The bar                       |
| ----------------------- | ------ | ----------- | ----------------------------- |
| A policy rule           | 1M     | `plausible` | Not weaker: −25 Elo against 0 |
| The root's choice       | 3M     | `plausible` | Not weaker: −25 Elo against 0 |
| The tree or the numbers | 3M     | `stronger`  | Stronger: 0 against +30 Elo   |

5. **The verdict and the record** (§4.6).

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
  `dropped`, with the reason in the bead.
- The bead's notes hold the command lines, the tables, the printed result and
  the reasoning. Logs, transcripts and position files are never committed.
- **To promote:** in a worktree, set `DEEP_SEARCHER`'s options in
  `rust/crates/beryl_search/src/lib.rs` to the challenger's; update §3's table
  and §5's table here; put the family on the log's `Champion:` line and add the
  row with the new champion's suite total. Push with `enact push`.
- **A promoted policy rule changes what rollouts play.** If the push's slow CI
  fails `counts` or `rollouts_never_change`, that is reason 1 of
  `rust/crates/beryl_bench/count-gate.toml`: update the pinned rollouts and the
  bar in the same change, with the old and new counts in the commit body.

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

The champion column shows runs passed of 20 in the rung 2 comparison. The best
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
| `evoke-subtlety-no-target`         | wrong  | right  | 17/20    | 10 with `waste` | `waste`           |
| `staff-in-upkeep`                  | wrong  | right  | 20/20    | 10 with `hold`  | `hold`            |
| `staff-before-combat-damage`       | wrong  | right  | 20/20    | 9 with `hold`   | `hold`            |
| `subtlety-before-own-draw`         | wrong  | right  | 20/20    | 10 with `hold`  | `hold`            |
| `jace-loots-before-draw`           | wrong  | right  | 20/20    | 9 with `hold`   | `hold`            |
| `discard-emperor-over-spare-crypt` | right  | right  | 20/20    | 10 with `hold`  | `hold`, `paired`  |
| `evoke-subtlety-empty-stack-hunt`  | right  | right  | 20/20    | 10 with `hold`  | `hold`, `waste`   |
| `beza-trinket-into-blockers`       | wrong  | right  | 20/20    | 8 with `combat` | `combat`          |
| `manifest-into-samurai`            | right  | right  | 20/20    | 8 with `combat` | `combat`          |
| `subtlety-main-phase-empty-stack`  | right  | right  | 13/20    | 4               | `paired`          |
| `spirits-stay-home`                | right  | right  | 20/20    | 5 with `hold`   | `paired`, `first` |
| `skip-teferi-plus-one`             | right  | right  | 1/20     | 4 with `turn72` | `first`           |
| `samurai-into-jace`                | wrong  | right  | 20/20    | 4 with `hold`   | `hold`, `paired`  |
| `spare-jace-wielder`               | wrong  | wrong  | 0/20     | 4 with `hold`   | `walker`          |

## 6. The rungs

In order. First the tools. Then what is already built is played, stacked. Then
the root is rebuilt around the policy. Then the two strength rungs. The session
may reorder rungs when a result suggests it, and says why in the bead.

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

### 6.1 Tools

Two beads, before rung 1.

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
2026-11-12 and 2026-11-13 saw one of them fall by noise. The build bead reads
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

### 6.5 Rung 4: `prior`, the tree explores as the policy plays

With `prior` set, `Tree::select` weights a child's exploration by the policy's
score of it, in place of UCB1's equal bonus.

- **The rule.** A child's bound is its mean plus
  `C × P(child) × sqrt(parent visits) / (1 + child visits)`. `P` is the softmax
  of the policy's scores at `ROLLOUT_TEMPERATURE`, the same weights a rollout
  draws from. A candidate the policy does not score takes an equal share. The
  first visit goes to the child with the highest `P`.
- **Why.** A node's mean is then close to the value of the position under
  sensible play by both players, which is what its parent needs. The tree stops
  rating an attack by the opponent's worst blocks.
- **`C`.** The build bead tries three values of `C` on `just why --tree` at the
  four clear-lead entries and takes the one whose tree lead is closest to the
  paired rollouts' lead.
- **Tests.** A child the policy scores far below its siblings gets fewer visits
  than under UCB1 at the same budget. At `beza-trinket-into-blockers`, the
  tree's lead for the attack is under 0.02.
- **Claims:** none.
- **Measure:** the tree, 3M, `stronger`.

### 6.6 Rung 5: `waste`

Built (`Rules::waste`). A creature cast for its evoke cost is scored by its
targeted enters abilities, so with nothing worth targeting it scores 0. Alone it
fixed both evoke entries and took `snapcaster-sorcery-speed` from 9 of 10 to 3.
`turn72` did the same to that entry, so the fall is real. With `hold` in the
champion the Snapcaster cast is held by rule. If the entry still falls, the
measure bead reads it with `just why` before a match.

- **Claims:** `evoke-subtlety-no-target`, `evoke-subtlety-empty-stack-hunt`.
- **Measure:** a policy rung, 1M, `plausible`.

### 6.7 Rung 6: `fit`, the policy's numbers fitted to the search

The policy's card values (`values.rs`) and the constants of its rules
(`rollout/policy.rs`) were set by hand. This rung fits them.

- **The labels.** Every searched decision of a ladder match is logged with its
  candidates' visits and means and its override test (`DecisionRecord`,
  `AnchorRecord` in `beryl_cli/src/match_log`). A decision where the paired test
  overruled the policy says the policy's score order was wrong there. A match
  holds a few thousand of these. If that is too few, the build bead adds a
  labelling run: paired rollouts of the policy's top three candidates at
  positions sampled from match games.
- **The fit.** `just fit` replays each labelled game to its decision, as
  `eval-fit` replays games, and searches the numbers one at a time for the
  setting that most often ranks the confirmed move first. It holds out a fifth
  of the games, and it penalises distance from the hand-set values.
- **The output** is a third knowledge, `fitted`, beside `tuned` and `17l`, so
  the challenger is `pimc72/hidden/rollout/fitted/...`.
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

### 6.9 Rung 8: `turn72`, the search over whole turns

Built (`long/turn.rs`). It proposes continuations of the player's turn with the
opponent passing and tests them in fresh worlds. Alone it fixed both Subtlety
entries and lowered the Snapcaster entry, and no match was played. It is
measured again once `hold` and `paired` are in the champion.

- **Claims:** none.
- **Measure:** the method, 3M, `stronger`.

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
  the planner's score do, and will need another deck to test them.
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
- **What it needs first.** The paired test and the policy-first root (rungs 2
  and 3), which produce clean labels, and `fit` (rung 6), which shows whether
  the search's labels improve a policy at all.
- **What it keeps.** The general rules stay as hard limits on the model, so that
  a model's mistake cannot become a blunder of a known class.

## 10. Open questions

- What does `hold` do to strength? Its suite gain is the largest on record and
  it has never played a match.
- How often does the paired test overrule the policy, and how much of the
  search's strength lives in those overrides? Rung 3's match and screen say.
- Does a policy fitted to the search make stronger rollouts? Rung 6 says, and §9
  depends on the answer.
- Why does `snapcaster-sorcery-speed` fall under both `waste` and `turn72`?
- Is the planner's small set of plans enough? `spare-jace-wielder` is the case
  it gets wrong.
- Do gains measured at 1M and 3M hold at 10M? The first audit says.
