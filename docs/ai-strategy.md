# AI Strategy

*The plan, as of 2026-11-11, for Beryl's AI opponent: what it must be, which
agent we start from, and how each change to it earns its place. It replaces the
2026-11-08 plan. That plan's first two rungs failed at the blunder suite and no
match was ever played; §2 says why. The ladder now keeps greedy's answer where
greedy is right and repairs the rollout policy where it is wrong. It runs on one
machine, Cygnus, and is written for small models to execute: every step names
its command and its pass condition.*

## 1. Goal

A strong opponent that **does not cheat** and **plays like a plausible human**.

- **Fair.** It sees what a player sees (`Sight::Hidden`). The cheating searches
  stay in the registry, labeled, as yardsticks; none is ever the opponent we
  improve.
- **No apparent blunders.** A debatable play is fine: Snapcaster Mage as a 2/1
  on turn two is sometimes right, and Reprieve on an Oust the opponent can
  recast was sound ([incident](incidents/2026-11-08-reprieve-on-oust.md)). A
  play no strong player would make under any reading is not, whatever the game's
  result. Humans have caught four kinds (§5 lists all 20):
  - **Acting at the first window instead of the last.** Snapcaster Mage and
    Subtlety cast in its own main phase, Staff of the Storyteller in the
    opponent's upkeep, Jace looting before its own draw.
  - **Declining something free.** Trinket Mage's search taking nothing, Teferi's
    untap taking no land, a safe attack not made.
  - **Spending a card on nothing.** Subtlety evoked with no spell to target,
    Oust on its own token.
  - **Attacking into blockers that win the fight.**
- **Blunders weigh more than strength.** We will trade some Elo for the last of
  them.
- **General by data, not by code.** A policy rule reads what a card's definition
  says (its types, keywords, costs, targets) and the game's state. It never
  names a card. Per-card numbers survive only as table entries (`values.rs`),
  the overlay of plan K9 ([project-50k.md](project-50k.md)).
- **The bar.** The WU Control mirror is the only testbed until a human finds a
  new blunder in fewer than one game in five. Then the ladder moves to the first
  Legacy deck.

## 2. What we learned

### 2.1 From run-003

2,777 games over 54 hours ([longthink-run-003.md](longthink-run-003.md)):

- **Full rollouts win.** Evaluation leaves trailed them by 180 to 260 Elo at
  every budget.
- **Budget pays, less and less.** Each ×3 of engine steps gained +23 to +57 Elo,
  flattening past 10M to 30M.
- **Hidden information costs little at length.** The omniscient twins led by +65
  to +160 Elo at 1M to 10M, and by nothing measurable at 30M.
- **Hand-tuned card values beat 17lands win rates** by about 90 Elo.
- **Fewer worlds, deeper trees.** 18 worlds rated +356 at 30M against +262 for
  72, within wide intervals.
- **Most moves are coin flips by the search's own estimate.** The chosen move
  led the runner-up by under 0.01 in 62% of decisions.
- **The anchor works where it applies.** Playing greedy's pass unless the search
  clearly wants to cast (`Options::anchor`) stopped the Oust for about 0.005 a
  game.

### 2.2 From the first two rungs and the review of 2026-11-11

- **The champion's judgement is the rollout policy's.** 30M steps buy about
  52,000 rollouts a decision, which is about 720 a tree over 72 trees. The tree
  has a node for every priority, attacker and target decision, so 720 rollouts
  reach four or five decisions deep: less than a turn. A move's value is, to a
  close approximation, how often greedy wins after it.
- **Greedy has no clock.** `Policy::action` (`rollout/policy.rs`) scores a cast
  or an activation the same in upkeep, a main phase, combat and an end step, on
  either player's turn. Only counterspells wait. So the search cannot price
  waiting, in a matchup that is mostly waiting.
- **Greedy alone gets 11 of the 20 suite positions right.** The champion,
  searching 30M steps on top of greedy, is fixed on 2
  (`just blunders greedy --seeds 1`, 2026-11-11; greedy is deterministic, so one
  run decides). The 20 entries split three ways:

| Where the fault is                                         | Entries | What repairs it                             |
| ---------------------------------------------------------- | ------- | ------------------------------------------- |
| Greedy is right; the search loses it in a near-tie         | 8       | The root keeps greedy's answer              |
| Greedy is right; the search clearly prefers the wrong play | 3       | Combat played out in the tree (§6.4)        |
| Greedy's own answer is wrong                               | 9       | A policy rule or the planner (§6.2 to §6.4) |

- **The expert alone gets 18 of the 20 right**
  (`just blunders expert --seeds 1`, 2026-11-11; it deals a world for combat, so
  this is one sample). It misses only the main-phase Snapcaster Mage and the
  attack on Jace, Wielder. The search beats the expert at winning and makes
  blunders the expert does not. The expert is not the answer: it is 3,200 lines
  for 29 cards (§1). But its two general ideas are the next rungs: a clock for
  instant-speed plays (`hold`) and combat played out on a copy of the game
  (`combat`).
- **The search throws away answers its own policy had.** With rewards this
  close, 30M steps cannot tell a free land untap from none, so noise chooses.
  The anchor exists for this and covers only a cast against greedy's pass.
- **A root rule is only as good as greedy at that kind of decision.**
  - Rung 1 (means over visits) changed no move on any suite entry.
  - Rung 2 (the guard) fixed the two entries where greedy is right, the declined
    Mana Crypt and Beza's attack after Winds of Abandon. It made the Jace,
    Wielder entry worse, because greedy's own attack target is wrong there.
- **Six of greedy's nine wrong answers are timing.** The others are an evoke
  with no target, an attack target and an attack into two blockers.
- **Two policy faults were read from the code and are not yet confirmed by a
  test.** The rung that claims one writes the failing test first (§6).
  - A choice of lands to untap is scored by each land's value with no check of
    who controls it (`rollout.rs`, `DecisionKind::ChooseUpTo`). Teferi, Hero of
    Dominaria's untap offers both players' lands, so a rollout may untap the
    opponent's. That would explain Teferi's +1 scoring 0.025 to 0.04 below a
    pass. Greedy took its own land at both suite positions, so this is not
    shown.
  - `Policy::block` never adds a second blocker to an attacker and chumps only
    when the player would die, never to save a planeswalker. An attack that two
    blockers punish looks safe to it.
- **Two incidents ended "cause not found".** Each is a policy fault that also
  costs strength in every game. Finding them needs a tool that shows what the
  rollouts do after each candidate (`just why`, §4.7).
- **The process produced nothing in three days.** No match was played and no row
  was written, while the suite grew from 4 entries to 20. Rung 2 fixed two
  entries, re-opened none and cost 0.006 a game on the screen. It was closed
  unpromoted because its bead asked for both of its claimed entries where the
  plan asked for one. A bead may not tighten the gate (§4.3).

## 3. The champion

The ladder improves one agent, the **champion**. The app's Deep Searcher is
always the current champion, and a promotion is a change to it.

The starting champion is Deep Searcher as it ships:
`pimc72/hidden/rollout/tuned/anchor@30M` (`beryl_search::DEEP_SEARCHER`).

| Axis      | Setting                                                 |
| --------- | ------------------------------------------------------- |
| Method    | PIMC, 72 worlds dealt from what it can see, a tree each |
| Sight     | Hidden                                                  |
| Leaf      | Softmax rollout to the end of the game, temperature 10  |
| Knowledge | Hand-tuned card values (`Knowledge::Tuned`)             |
| Options   | `anchor`: greedy's pass unless the search leads clearly |
| Rules     | None yet. Each policy rung adds one (§6)                |
| Scores    | Turn discount 0.98: win sooner, lose later              |
| Budget    | 30M engine steps a decision, on every core              |

Every change is a named part of the family's name, so any past champion can be
played again from the current build: `…/tuned/anchor+guard+hold` is the champion
after the first two rungs. The audit (§4.5) depends on this.

Yardsticks, never candidates: `expert-frozen`, `greedy` (the rollout policy as a
player), Deep Seer (the omniscient twin) and the champion anchored to the expert
(§6.6).

## 4. The ladder

One challenger at a time plays the champion. It differs in one named thing. It
is promoted by the gate below and by nothing else.

### 4.1 Where it runs, and who runs it

- **Cygnus only.** Every suite, screen, match and audit runs on Cygnus, the
  Windows PC (i7-12700F, 20 threads, `BERYL_CACHE=D:\beryl-cache`). The Mac
  plays no ladder games. The position set is at
  `D:\beryl-cache\ladder\positions` (216 positions).
- **One lane.** Every bead of the ladder carries `lane:ladder`. One session
  works them in order; no two matches ever run at once.
- **Small models.** A bead's steps are numbered, each with its command and what
  to do on each outcome. A step that is not written is not done. Where a step
  fails in a way the bead does not cover, the session writes what happened in
  the bead's notes, closes the bead as failed and goes on to the next. It never
  waits for a person and never files a bead asking one to look.
- **Long commands run in the background.** The `just` recipes named here run at
  low priority by themselves. Nothing else heavy runs beside a match.

### 4.2 A rung is two beads

- **Build.** Adds the change as an option that is off by default, with its
  tests, and pushes it. The champion's moves do not change. Its bead names the
  files, the exact rule and the tests.
- **Gate.** Runs §4.3 on the pushed build and records the result. It depends on
  the build bead. It is mechanical, so it is labeled `tier:light`.

### 4.3 The gate

The gate bead runs these steps in order. `CHALLENGER` is the family the build
bead named; `CLAIMS` is the comma-separated list of suite entries it claims.

1. **Audit check.** If §4.5 says an audit is due, run it first.
2. **Suite.** `just gate CHALLENGER --claims CLAIMS`. It runs the suite (§5) for
   the champion and the challenger and prints `GATE: PASS` or `GATE: FAIL`. The
   gate passes when all three hold:
   - at least one claimed entry is open for the champion and fixed for the
     challenger;
   - no entry fixed for the champion is open for the challenger;
   - no entry passes 4 or more fewer of its 10 runs than it did for the
     champion.
3. **On `GATE: FAIL`:** copy the printed table into the bead's notes, add a row
   to `docs/ladder.md` with the verdict `failed at the suite`, close the bead as
   failed. Play no match. Go on to the next rung.
4. **Match.** On `GATE: PASS`, play the match of the rung's kind (§4.4). A root
   rung first runs the screen and writes its cost a game in the bead's notes;
   the cost stops nothing.
5. **On the test accepting the bar:** promote (§4.6).
6. **On a rejection or the cap:** add the row with the score and the verdict,
   leave the champion as it is, close the bead as failed.

A bead's claims list says which entries the rung expects to fix. Fixing any one
of them passes the first condition. A search rung claims none (`--claims none`),
and its gate is the second and third conditions alone. No bead may ask for more
than §4.3 does.

### 4.4 The match and the bars

- **Paired seeds, seats swapped.** Every seed is played twice, each agent in
  each seat. The unit of evidence is the pair: swept, split or lost.
- **Curated start positions.** Four pairs in five start from the position set,
  the fifth from the shuffle. The first match's report gives the spread of pair
  scores from each; the gate bead of the first rung copies that line into
  `docs/ladder.md`, with the hours the match took.
- **A sequential test** ends the match as soon as the evidence is in, and a cap
  ends it undecided. The bar is the rung's kind, fixed in its bead:

| Rung kind | Budget | Bar                                         | Command                                            |
| --------- | ------ | ------------------------------------------- | -------------------------------------------------- |
| Root      | 3M     | Not weaker: −25 Elo against 0, α = β = 0.05 | `just ladder CHALLENGER --work 3M --bar plausible` |
| Policy    | 1M     | Not weaker: −25 Elo against 0, α = β = 0.05 | `just ladder CHALLENGER --work 1M --bar plausible` |
| Search    | 3M     | Stronger: 0 against +30 Elo, α = β = 0.05   | `just ladder CHALLENGER --work 3M --bar stronger`  |

All three add `--positions D:\beryl-cache\ladder\positions --cap 2000`.

- **Why 1M for policy rungs.** A policy rule changes what every rollout plays at
  any budget, so its effect should not depend on the budget as a root rule's
  does. About 1,100 games decide it. The first match measures how long that
  takes on Cygnus and its gate bead writes the hours into `docs/ladder.md`.
- **Why 3M for root and search rungs.** A root rule fires on margins that shrink
  as the budget grows, and worlds, depth and expansion order trade against the
  budget, so both are measured nearer the shipped one. Run-003's families kept
  their order from 3M to 30M.
- **Why +30 at 0.05.** The old bar (+50 Elo at 0.001) sent a real +25 Elo gain
  to the cap. Four of those are +100 Elo. A challenger no stronger than the
  champion now passes one match in twenty; the audit is the second check.
- **No confirmation per rung.** The suite already runs at the shipped budget,
  and the audit confirms the champion as a whole.
- **The screen** is for a root rung, which changes few moves:
  `just screen CHALLENGER D:\beryl-cache\ladder\champion-2026-11-10`. If a
  promotion has changed the champion since that directory was played, first play
  a new one:
  `just ladder CHAMPION --vs CHAMPION --cap 20 --out D:\beryl-cache\ladder\champion-DATE`.
  A policy or search rung changes every rollout, so the screen's count of
  differing moves means little there and those rungs skip it.

### 4.5 The audit

A check that gains measured at 1M and 3M hold at 10M.

- **Due** when `docs/ladder.md` shows at least one `pass` row since its last
  `audit` row, and that row (or, with none, the first `pass` row) is seven or
  more days old.
- **Run:**
  `just ladder CHAMPION --vs LAST --work 10M --bar audit --cap 400 --positions D:\beryl-cache\ladder\positions`,
  where `LAST` is the family named in the last `audit` row, or the starting
  champion of §3 when there is none. It plays all 400 games and prints the score
  with its 95% interval.
- **Passed** when the interval's upper end is 50% or more. Add an `audit` row
  naming the champion's family.
- **Failed** when the whole interval is below 50%. Then, newest first, play each
  promotion since the last audit against the champion before it:
  `just ladder AFTER --vs BEFORE --work 10M --bar audit --cap 200`. Unset the
  first one that scores below 45% (change `DEEP_SEARCHER` back, push), add its
  row with the verdict `reverted by audit`, and stop there. Add the `audit` row.

### 4.6 The record and a promotion

- `docs/ladder.md`, created by the first gate bead, is the log: a line
  `Champion:` naming the current champion's family, then one row a match or
  audit, newest first, as `just ladder` prints it: date, challenger, champion,
  budget, games, score, Elo interval, verdict, the suite's open and fixed counts
  after it, and the human games since the last promotion. A match's verdict is
  `pass`, `fail` or `undecided`; a rung stopped by the gate is
  `failed at the suite`; an audit is `audit`; §6.6's measurement is `yardstick`.
- The bead's notes hold the command lines, the seed and the printed result.
- Logs, transcripts and position files are never committed.
- **To promote:** in a worktree, set `DEEP_SEARCHER`'s options in
  `rust/crates/beryl_search/src/lib.rs` to the challenger's and name the gate
  bead in its doc comment; update §3's table here; put the family on the log's
  `Champion:` line and add the row. Run `just blunders` once more on the shipped
  agent and put its counts in the row. Push with `enact push`.
- **A promoted policy rule changes what rollouts play.** If the push's slow CI
  fails `counts` or `rollouts_never_change`, that is reason 1 of
  `rust/crates/beryl_bench/count-gate.toml`: update the pinned rollouts and the
  bar in the same change, with the old and new counts in the commit body.

### 4.7 Tools to build first

Each is its own bead in the lane, before the rung that needs it (by-97g.17 to
by-97g.21).

- **Rules.** The policy takes a set of named rules (`hold`, `waste`), each a
  part of the family's name after the options, each off by default. With none
  set, greedy plays exactly as today.
- **`just gate CHALLENGER --claims A,B [--vs CHAMPION]`.** The suite for both
  agents and the verdict of §4.3, exit code 0 on a pass.
- **Bars.** `--bar stronger` becomes 0 against +30 Elo at α = β = 0.05.
  `--bar audit` plays to the cap with no early stop and prints the score's 95%
  interval.
- **`just why ENTRY --a LABEL --b LABEL`.** For one suite entry and two of its
  candidates: the same worlds and seeds played out after each by the champion's
  rollouts; each one's win rate; the events of the next two turns counted per
  thousand rollouts after each, sorted by how much they differ; and three
  transcripts each way where the two ended differently. It answers "what do the
  rollouts do that makes this look good".
- **A games ledger.** The app appends a line for every finished game to a file
  it never rotates (date, build, agent, result), and a bug report carries the
  count of games finished against that champion. The measure of §5 needs it.
  This bead is the app's, outside the lane.

## 5. The blunder suite

A ledger of every blunder a human has caught, and the gate's first condition.

- **An entry** is a position (an incident's replay or a rebuilt board), the
  forbidden choices and, where one exists, the required choice. The list is
  `data/blunders.json`; each entry cites its incident and names its class.
- **The check** (`just blunders`) builds the agent as the app did and searches
  the position at 3M and at 30M from five seeds each: 10 runs an entry. A run
  passes when it makes no forbidden choice and every required one.
- **Fixed and open.** An entry is *fixed* when all 10 runs pass and *open*
  otherwise. The champion made every play the suite forbids, so some entries are
  always open; those are what the rungs are for.
- **Greedy's and the expert's own answers are recorded for every entry.**
  `just blunders greedy --seeds 1` takes seconds. Where greedy passes, the
  search lost a good answer and a root rule may keep it. Where greedy fails,
  only a policy rule can fix it. Where the expert passes, a rule for it is known
  to exist. An incident's write-up says which.
- **The entries, by the rung that claims them** (§6):

| Entry                              | Class                                  | Greedy | Expert | Rung     |
| ---------------------------------- | -------------------------------------- | ------ | ------ | -------- |
| `decline-mana-crypt`               | A free find declined                   | right  | right  | `guard`  |
| `decline-both-teferi-untaps`       | A free untap declined                  | right  | right  | `guard`  |
| `decline-second-teferi-untap`      | A free untap declined                  | right  | right  | `guard`  |
| `discard-emperor-over-spare-crypt` | A spell discarded over surplus mana    | right  | right  | `guard`  |
| `skip-beza-after-winds`            | A free attack not made                 | right  | right  | `guard`  |
| `snapcaster-sorcery-speed`         | Flash creature at sorcery speed        | wrong  | wrong  | `hold`   |
| `subtlety-main-phase-empty-stack`  | Flash creature at sorcery speed        | right  | right  | `hold`   |
| `subtlety-before-own-draw`         | Flash creature before the draw         | wrong  | right  | `hold`   |
| `staff-in-upkeep`                  | Activation that spends held mana early | wrong  | right  | `hold`   |
| `staff-before-combat-damage`       | Activation that spends held mana early | wrong  | right  | `hold`   |
| `jace-loots-before-draw`           | Activation before the draw             | wrong  | right  | `hold`   |
| `samurai-into-jace`                | Instant-speed loyalty spent at once    | wrong  | right  | `hold`   |
| `evoke-subtlety-no-target`         | Evoke with no target                   | wrong  | right  | `waste`  |
| `evoke-subtlety-empty-stack-hunt`  | Evoke with no target                   | right  | right  | `waste`  |
| `skip-teferi-plus-one`             | A free loyalty ability skipped         | right  | right  | `waste`  |
| `spare-jace-wielder`               | Attack leaves a lethal walker alive    | wrong  | wrong  | none yet |
| `beza-trinket-into-blockers`       | Attack into blockers that win          | wrong  | right  | `combat` |
| `manifest-into-samurai`            | Attack into blockers that win          | right  | right  | `combat` |
| `spirits-stay-home`                | A free attack not made                 | right  | right  | `combat` |
| `oust-own-token`                   | Removal on its own permanent           | right  | right  | none     |

Greedy is right at `subtlety-main-phase-empty-stack` and
`evoke-subtlety-empty-stack-hunt` only because it prefers another spell there;
it would make both plays with that spell gone. `oust-own-token` is what the
anchor was built for.

- **A fix addresses the class.** A table entry that moves one card's value in
  one spot passes the entry and leaves the class, and the next games find it
  again. Each policy rule ships with a test over every card in the pool that has
  the property the rule reads (every card with flash, every evoke cost), not
  only the card in the incident.
- **Growth changes no past verdict.** A promotion was judged by the suite as it
  stood when its gate ran.
- **New entries come from two places.**
  - *A human's game.* A play they would call wrong goes through the bug button
    (⌘B), gets an incident write-up and becomes an entry with a class. A new
    class joins the rung that covers it or opens a new rung.
  - *The match's own games.* After a promotion, the gate bead reads 20 of the
    new champion's match transcripts for the classes that rung claimed and notes
    each sighting. A new position of a class a human has already confirmed, in a
    game that started from the shuffle, is added as an entry (at most three a
    rung), as that fix's held-out test. A play of a class no human has confirmed
    is not added and no bead is filed for it; one line in the gate bead's notes
    records it.
  - No machine time is scheduled for hunting. `just blunder-hunt` remains for a
    machine that would otherwise sit idle.
- **The measure is entries opened per human game**, from the games ledger
  (§4.7). The mirror is done when it is under one in five (§1). A champion that
  opens an entry every game is not plausible yet, whatever its Elo.
- **Cost.** An entry takes about 45 seconds a suite run on Cygnus (20 entries in
  15 minutes). Past about 40 entries, every entry runs at 3M and only claimed
  entries and those whose state changed run at 30M.
- **Drift.** A replay from before an engine change may stop reproducing its
  position. An entry that no longer replays is rebuilt as a board
  (`data/blunders/*.json`) or retired with a note, never silently skipped; the
  suite fails on it until then.

## 6. The rungs

In order. First the root keeps what greedy already gets right, which is built
and cheap. Then the policy rules, which fix greedy's own wrong answers and make
every rollout sounder. Then the combat planner, which the expert already has.
Then the search rungs, which pay more once rollouts are sound.

| Order | Rung     | Kind   | Challenger (the champion's family, plus) | Claims | Beads (build, gate)  |
| ----- | -------- | ------ | ---------------------------------------- | ------ | -------------------- |
| 1     | `guard`  | Root   | `+guard`                                 | 5      | by-97g.22, by-97g.23 |
| 2     | `hold`   | Policy | `+hold`                                  | 7      | by-97g.24, by-97g.25 |
| 3     | `waste`  | Policy | `+waste`                                 | 3      | by-97g.26, by-97g.27 |
| 4     | `combat` | Root   | `+combat`                                | 3      | by-97g.28, by-97g.29 |
| 5     | `mid`    | Search | `+mid`                                   | none   | built, by-97g.9      |
| 6     | Worlds   | Search | `pimc18` in place of `pimc72`            | none   | none, by-97g.10      |
| 7     | `order`  | Search | `+order`                                 | none   | by-97g.12, by-97g.30 |

A rung that fails leaves its option unset and the next rung plays the same
champion.

### 6.1 `guard`: the root keeps greedy's answer where greedy is right

`guarded` (`long.rs`) holds greedy's answer unless the search leads it clearly
(by 0.02 and by two standard errors across the trees). With `guard` it holds at:

1. **An attacker** (`DecisionKind::Attacker`), as the option does today.
2. **Any `ChooseUpTo`**: a find, lands to untap. Today it holds only at a
   library search's find.
3. **A discard**: a `ChooseObject` among cards in the player's own hand.
4. **A play greedy scores as a pass.** At a priority decision, when the search
   wants a cast or an activation that `Policy::action` scores 0 or less,
   greedy's answer stands, whatever greedy plays instead. Today the anchor holds
   only a cast against greedy's pass. This is how every later policy rule
   reaches the champion's own moves: a play the rule holds scores 0.

It no longer holds at an attack's target (`DecisionKind::AttackTarget`), where
greedy is wrong until `combat`.

### 6.2 `hold`: act at the last window

With the rule set, a cast or an activation at instant speed scores as a pass
until the opponent's end step.

- **Instant speed** is any cast or activation offered when it is not the
  player's own main phase with an empty stack. In that main phase it is a spell
  that is an instant or has flash, or an activated ability with no loyalty cost.
- **It is not held** in exactly these cases:
  1. It is the opponent's end step.
  2. An opponent's spell is on top of the stack and the play's first target can
     be a spell (for a permanent spell, its targeted enters ability).
  3. It is the opponent's turn and a creature is attacking.
  4. It is the player's own combat and the play has a target.
  5. It is the player's own end step and their hand holds more than seven cards.
  6. It draws (`CardInfo::draws`), in the player's own main phase with an empty
     stack.
- The list is closed. A hold that is debatable is acceptable; the suite and the
  match judge the rule as a whole.
- The turn and step come from the observation, as `Expert::timing`
  (`expert/mod.rs`) reads them. The expert's own timing rules, which name cards,
  get six of this rung's seven entries right; `hold` is the same idea read from
  card data.

### 6.3 `waste`: spend nothing on nothing

With the rule set:

1. **Evoke needs a target.** A creature cast for its evoke cost is scored by its
   targeted enters abilities, as a noncreature permanent already is
   (`Policy::enters_worth`): with nothing worth targeting it scores 0.
2. **A choice among permanents takes only its own.** In a `ChooseUpTo` that is
   neither a library search nor a scry, an object an opponent controls is never
   taken. The test comes first: over 200 seeded rollout decisions that offer
   Teferi's untap with tapped lands on both sides, count how often the softmax
   policy takes an opponent's land. If it never does, the reading in §2.2 was
   wrong: say so in the bead and leave this part out.

Before building, the bead runs `just why skip-teferi-plus-one` and writes the
top differing events into its notes: that entry is why part 2 is suspected.

### 6.4 `combat`: the combat planner

Combat is a small game inside the game, and the search handles it worst. An
attack is a node for every attacker and every target, so the defender's blocks
lie five or six nodes down in a tree four or five deep, and greedy's block rules
decide them: one blocker an attacker, and a chump only to save the player.

The expert already plans combat another way (`expert/combat.rs`, `Sim`):

- It plays each attack or block it weighs to the end of combat on a copy of the
  game, both players passing priority, and scores the position left: the
  permanents on each side, life, and the damage the opponent could swing back
  with next turn.
- In an attack, the defender blocks by the same planner, one level deep.
- It weighs a small set of plans: one attacker or two at each target, enough
  power to kill each planeswalker, every safe attacker, and every attacker at
  the player, which is the lethal check.
- The engine resolves the damage, so first strike, deathtouch and trample need
  no code of the planner's own.

With the expert as the agent, the suite's combat entries come out right at four
of five, including both attacks into blockers and both free attacks. It still
sends Snapcaster Mage at the wrong Jace in `spare-jace-wielder`, so no rung
claims that entry yet.

With `combat` set, an option of the search:

1. **The planner is its own module** and names no card. Three judgements of the
   expert's that name cards stay in the expert, behind a hook.
2. **At the root, the planner's answer is the default** at an attacker, an
   attack's target, a blocker and a block's target, from a world dealt from what
   the player sees. `guarded` holds it unless the search leads clearly.
3. **In the tree, the combat of the root's own turn is the planner's**, for both
   players, as `mid` lets greedy answer mid-resolution choices. An attack is
   then priced against the blocks a defender would make, which is what the three
   clear-lead entries need.

Combats later in a rollout stay greedy's: the planner costs hundreds of engine
steps a decision and a whole rollout is about 580.

The build bead first runs `just why` on `beza-trinket-into-blockers`,
`manifest-into-samurai` and `spirits-stay-home` and writes the top differing
events into its notes. It needs judgement the others do not and is not labeled
`tier:light`.

### 6.5 The search rungs

- **`mid`.** Greedy answers the questions asked while casting or resolving below
  the root (`Options::mid`, built, never matched).
- **Worlds.** 18 worlds in place of 72: four times the rollouts a tree.
- **`order`.** A node's children are first tried in the order greedy scores them
  instead of at random, so a wide choice is priced by its best answers.

### 6.6 A yardstick: the champion anchored to the expert

The expert gets 18 of the 20 suite positions right and the champion 2 (§2.2).
One measurement says how much of that gap the root alone can close, and at what
price in strength. It is a yardstick, measured once after the `guard` rung, and
never promoted: the expert is 3,200 lines for 29 cards, so a champion that
leaned on it would tell us nothing about a Legacy deck, and would not count
toward §1's bar.

- **The option `expert`.** With `anchor` and `expert` set, the root's default at
  every decision is the expert's answer, and `guarded` holds it at every kind of
  decision unless the search leads clearly.
- **What is measured** (by-97g.32 builds it, by-97g.33 measures):
  - the suite: how many of the 20 entries are fixed;
  - the screen: how many moves a game differ from the champion's, by kind;
  - a match against the champion, 400 games at 3M with `--bar audit`: the score
    with its interval.
- **What it tells us.** The suite count is the most that root defaults can buy:
  entries still open are ones where the search overrules a right answer by a
  clear lead, which only sounder rollouts fix. The score is what plausibility
  costs, or gains, when the default is a good one. The rungs after it are then
  read against both numbers.
- Its row in `docs/ladder.md` has the verdict `yardstick`.

## 7. Speed

Engine speed is a multiplier on every rung.

- The cube rollout benchmark and `count-gate.toml` stay the measure
  ([benchmarking.md](benchmarking.md)).
- A speed-up that keeps every answer (`rollouts_never_change` pinned) changes no
  move, so it needs no match.
- A policy rule's code runs only when its rule is set, out of line, so the
  benchmark's count does not move until a promotion (§4.6).

## 8. Not now

- **Other root-choice rules.** Means over visits (old rung 1) changed nothing.
  Greedy in decided positions (old rung 3) and a turn discount of 0.95 (old rung
  9\) are parked until the policy rungs are played: a default is only as good as
  greedy, and greedy is about to change.
- **Once-a-game draws over many worlds and the 90M budget rung** (old rungs 7
  and 10): after the seven rungs above.
- **A second deck.** The mirror is the only testbed until §1's bar is met. The
  timing rule rests on dominance (the same play is available later at no loss),
  which does not depend on the deck; greedy's card values and combat rules do,
  and will need the first Legacy deck to test them.
- **Combo decks.** A combo turn is 10 to 20 linked decisions, which needs the
  tree of turns (§9).
- **A solver that weighs every attack and every block.** The planner weighs a
  small set of plans. Four creatures a side is 16 attacks by 625 blocks; that is
  the step after, if the suite shows the set is too small.
- **Block rules in greedy** (a second blocker, a chump for a planeswalker): only
  if the clear-lead combat entries stay open after `combat`, since they would
  then be lost in combats after the root's turn.
- **A lethal check of its own.** The planner's all-in attack is one.
- **The Mac.** It plays no ladder games (§4.1).
- **Hunts on a schedule** (§5).
- **The long-think arena and round robins.** `just longthink` is retired;
  `just tournament` remains for a quick table and decides nothing.
- **Learned value models, AlphaZero-style or R-NaD training.** Evaluation leaves
  lost by 180 to 260 Elo here. In Monolith, R-NaD learned nothing in 145,000
  self-play games and a learned value model was neutral or harmful (p = 0.002).
  In netrunner-rs, 0 of 6 AlphaZero-style runs were promoted.
- **Card-specific code.** Rules read definitions; numbers are table entries
  (§1).

## 9. The next architecture: a tree of turns

Not a rung yet. It is what the search should become once the rungs above have
run, and it is recorded here so that they are built toward it.

- **The problem.** The tree's unit is one priority decision. A turn is 10 to 20
  of them, so 720 rollouts a tree never see a whole turn, and every rung above
  works around that.
- **The design, in outline.** At the root, enumerate what is left of the
  player's own turn with the opponent passing; merge the sequences that reach
  the same position; take combat from the planner; score each end-of-turn
  position by rollouts. When the opponent acts, plan again.
- **What it needs first.** Rollouts that are sound (the policy rungs), since its
  leaves are still rollouts, and the planner (§6.4).
- **What it does not fix.** Holding mana on the opponent's turn. A plan has to
  name what it keeps up, which is the harder half and the reason `hold` comes
  first.
- **What it opens.** Combo decks, whose turns are one long sequence.
- **How it is judged.** As one challenger with a new method in its family's
  name, by a search rung's gate (§4.4). Its design is a bead of its own
  (by-97g.31), blocked on the last rung.

## 10. Open questions

- How long does a 1,100-game match at 1M take on Cygnus? The first gate bead
  measures it.
- Do the start positions cut the spread of pair scores against the shuffle? The
  first match's report says; if they buy nothing, they go.
- Does a policy gain at 1M hold at 10M? The first audit says.
- Does a fixed entry stay fixed from run to run? An entry is fixed at 10 of 10,
  so one unlucky run re-opens it and fails a gate. If that happens to a rung
  whose change cannot have touched the entry, the gate needs a rerun rule.
- Why does the search prefer holding two fliers back from an empty board, and an
  attack into a blocker that kills the attacker? `just why` on those entries
  says, in the `combat` rung.
- Is the planner's small set of plans enough, or does combat need every attack
  and block weighed? `spare-jace-wielder` is the case it gets wrong today: it
  has a plan that kills Jace, Wielder and scores another higher.
- What does a plausibility promotion cost in Elo over a month of them? The log
  answers that, if every match is recorded.
