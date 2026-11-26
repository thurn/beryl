# Benchmarking

How to measure small optimizations in Beryl without fooling yourself. The
`beryl_bench` crate (`engine/crates/beryl_bench`) holds the support code and a
worked example, `example_*`, that follows every rule here.

The problem: a 2% speedup is smaller than the noise in a wall-clock benchmark on
a machine that is always busy. The development Mac runs many agent sessions and
slow CI at once, and a load average of 20 to 40 on its 18 cores is normal. There
is no quiet machine to wait for, so every measurement here is made under that
load. We measure two ways, and believe a result only when they agree.

## The strategy

1. **Make the benchmark deterministic.** Same inputs, same work, every run.
2. **Count instructions** with Gungraun in a Linux VM (`just bench-count`).
   Exact, identical on every run, unaffected by load. The first and fastest
   answer.
3. **Confirm with interleaved timing** on macOS or Windows, under the machine's
   normal load (`just bench`): old and new in one executable, timed in A B B A
   order, with the median ratio and its confidence interval as the spread. This
   catches what counts miss: cache misses, branch mispredictions, the real
   allocator.
4. **Track absolute times** with Criterion and saved baselines, for regressions
   over time.

| Tool                   | Measures                  | Noise              | Use it for                |
| ---------------------- | ------------------------- | ------------------ | ------------------------- |
| Gungraun (`*_counts`)  | instructions, cache model | none               | every change, first       |
| ABBA (`*_abba`)        | wall-clock ratio B/A      | cancels drift      | is new faster than old?   |
| Criterion (`*_timing`) | wall-clock per call       | drift between runs | absolute times, baselines |

## Commands

```bash
just bench-count
```

```bash
just bench -p beryl_bench --bench example_abba
```

```bash
just bench -p beryl_bench --bench example_timing
```

- `just bench-count [cargo bench flags]` runs the Gungraun benchmarks (default
  `--bench '*_counts'`) in `beryl-bench` under Lima on macOS or WSL on Windows,
  creating the Linux environment on first use. `just bench --counts` runs the
  same command. `just bench-count` and `just bench` build `-p beryl_bench`
  unless the flags name a package: from the whole workspace, Cargo would build
  in the features other members' tests turn on, such as `coverage`, which no app
  or search build has.

- `just bench [--force] [cargo bench flags] [-- harness flags]` builds, checks
  that the CPU is not slowed (battery, Low Power Mode, throttling), then times
  and logs the load before and after. `-- --quick` gives the ABBA harness 30
  rounds instead of 300; Criterion takes its own flags, such as
  `-- --save-baseline before`. It builds under the `search` profile unless the
  flags name a `--profile`. The default CPU is `apple-m4` on ARM64 Macs and
  `native` on Windows x86-64.

- `just count-gate` counts only `rollout_counts`' `rollouts` subject in the VM,
  about 15 s once built, and fails when it runs more than the tolerance above
  the verifier's bar in `engine/crates/beryl_bench/count-gate.toml`. Compare
  counts taken on the same verifier with the same calibration. Ignite keeps a
  separate trend for each calibration. The file's comments say when a bar may
  rise. `just count-gate --family <name>` gates a family's present cost
  ([The present-cost gate](#the-present-cost-gate)). Ignite runs the family
  checks against the calibrated ratios on macOS and Windows.

Name benchmark targets by kind: `<subject>_counts`, `<subject>_abba`,
`<subject>_timing`.

## The rollout benchmark

`rollout_*` measure what search spends its time on (architecture §18), over a
corpus of mid-game WU Control mirror positions cut from seeded `expert` and
`greedy` games, each rolled out four times to the end of the game under the
rollout policy and smart stops (`beryl_bench::rollout`). Four subjects, each
over the whole corpus:

- `copy`: one `State` copy per rollout start;
- `next_apply`: the engine alone, replaying each rollout's recorded answers;
- `policy`: the rollout policy alone, on decisions sampled from the rollouts;
- `rollouts`: whole rollouts, the three together.

`rollout_timing` prints the decisions per rollout and per turn first, and its
throughputs read as copies, decisions and rollouts per second on one core.
`rollout_abba` runs the current code on both sides until an optimization puts
its old version in A. `rollout::tests::corpus_never_changes` pins the corpus,
and `rollouts_never_change` every corpus rollout's answers and ending.

- **Profile** with the counts: after `just bench-count`, `callgrind_annotate` in
  the VM reads the `rollouts` subject's exclusive cost by symbol from
  `$HOME/gungraun/<worktree>/beryl_bench/rollout_counts/cube_rollout/rollouts.corpus/callgrind.rollouts.corpus.out`.
  The largest cost is building priority decisions (`engine::decision`, about
  19%), then the mana solver's source scan (`mana::source`, about 9%).

## The Dandân rollout benchmark

`dandan_*` are the rollout benchmark over the Dandân deck
(`decks::FORGETFUL_FISH` in `Format::SHARED_DECK`, where both players draw from
one library), so that a Dandân rollout getting slower shows in the trend. They
use `rollout`'s constants, `Corpus::starts`, `Workload` and subjects, with the
same four (`copy`, `next_apply`, `policy`, `rollouts`) over positions cut from
seeded games of `greedy` agents on both seats, because the frozen expert names
WU Control cards (`beryl_bench::dandan`). `dandan::tests::corpus_never_changes`
and `rollouts_never_change` pin them. Unlike the WU Control corpus they are not
in `tests/allocations.rs`: a Dandân rollout allocates, because each resolution's
repeated-position check (`State::canonical_hash`) copies the state into the
`Large` layout.

It is tracked and has no bar: `count-gate.toml` keeps one benchmark. The gate's
run counts `dandan_counts`'s `rollouts` subject after the gated one, and the
trend stores its count beside the gated one in `dandan-<calibration>.jsonl`,
printing the change since the last measured tip and filing no ticket.
`just bench-count` prints it with the other counts.

## The present-cost gate

The count gate proves a mechanism costs the WU mirror nothing while no card of
it is in play. The present-cost gate measures what it costs while one is:
`just count-gate --family <name>` (`beryl_bench::family`,
`benches/family_counts.rs`).

- **A family** is a name and a list of swaps into `decks::WU_CONTROL`: how many
  of which card replace as many of which card, so the deck keeps its size
  (`family::FAMILIES`). `blank` swaps in a Tundra, which has no static or
  triggered ability, and checks the method: it must measure 1.00. `attachments`
  swaps in Swift Reconfiguration and Leonin Scimitar, the cards whose static
  abilities the layers apply. `layers` swaps in Blood Moon, Humility, Glorious
  Anthem and Tarmogoyf (and Mountains and Forests, so the first and last can be
  cast), one effect in each layer the engine has. `replacement` swaps in Rest in
  Peace, Chains of Mephistopheles, Containment Priest and Furnace of Rath, the
  replacement pipeline's cards. `costs` swaps in Trinisphere, Thalia, Pithing
  Needle and Cavern of Souls, the cost rules read at every priority for every
  card in hand. `zones-and-text` swaps in Painter's Servant, Mind Bend, Crystal
  Spray and Underworld Breach (and Mountains, so Breach can be cast): a color
  added in every zone, text-changing effects and casting from the graveyard.
  Gungraun ids cannot hold hyphens, so its benchmarks are `zones_and_text`.
  `copies` swaps in Phantasmal Image, Quicksilver Gargantuan, Thespian's Stage
  and Fork. In its control a permanent that was a copy by a card of the family
  is the card it really is again, then its stand-in; a step at a legend-rule
  question is left out when such a copy was in the group. `identity` swaps in
  Animate Dead, Gemrazer, Teferi's Protection and Stifle (and Swamps and
  Forests, so the first two can be cast). Its positions also count a merged
  permanent or a phased-out permanent as the family being in play. Its control
  swaps cards for their stand-ins and cannot un-merge or phase in, so it leaves
  a phased-out permanent as it is, and a step while a card of the family is
  merged with a permanent is left out: ending Gemrazer's merge would leave the
  cards under the permanent behind in the merged zone once it left the
  battlefield. `two-faced` swaps in Fire // Ice, Wear // Tear, Bonecrusher
  Giant, Valakut Awakening and Delver of Secrets (and Mountains, so the red
  halves can be cast), `levels` The Eldest Reborn, Wizard Class, Case of the
  Crimson Pulse and Student of Warfare, `face-down` Exalted Angel and Gathan
  Raiders (not Illusionary Mask, whose rollouts outgrow `Small` as soon as it
  spends mana on X), and `outside` Burning Wish and Lurrus of the Dream-Den,
  `turn-structure` Time Stop, Paradox Haze, Relentless Assault and Time Vault
  (ending the turn, additional upkeep, combat phases and an extra turn), and
  `restart` Karn Liberated, three copies, `cast-in-resolution` Bloodbraid Elf,
  Knowledge Pool and Panglacial Wurm with Sylvan Scrying to search for it (and
  Mountains and Forests, so they can be cast), and `combat-assignment` Benalish
  Hero, Palace Guard, Fencing Ace and Master of the Hunt (and Forests, so the
  last can be cast). A stand-in drops abilities and keywords, not a card's
  shape, so these families measure what the cards' abilities cost in play; the
  cost of the shape itself, which every card pays through `State::copiable`, is
  in the absent-cost count.
- **Its corpus** is cut from seeded games between greedy agents playing the
  family's deck in both seats: the first decision with a card of the family on
  the battlefield, on the stack or the source of an effect in force, which is
  often the turn it arrives, then every third turn after while one is in play.
  `family::tests::family_corpora_never_change` pins each corpus.
- **Each step is paired.** Each position is rolled out under the rollout policy.
  At every decision, with the state S and the policy's choice c, the benchmark
  builds the control S' (`family::control`): S with each permanent and spell of
  the family replaced by its stand-in, the same card with no static or triggered
  ability and no keyword, and every effect a card of the family created ended.
  It finds the candidate of S' that does what c does, as a save matches a
  recorded answer (`beryl_session::save::Chosen`), and applies the choice to
  both and advances each to the next decision. The rollout goes on from S and S'
  is dropped. A step whose choice S' does not offer is left out of both sums and
  counted as skipped.
- **The control has its own table** (`family::control_table`): the same cards
  under the same indices, with the capabilities
  (`beryl_engine::table::Capability`) the game would have if no card of the
  family, no stand-in and no token that only they make were in it
  (`GameTable::without_capabilities_of`). The engine switches a path on for
  every game whose cards could use it, such as banding's attack options or the
  turn plan, so a control on the present table would pay for every path the
  family switches on. A stand-in counts as out because it keeps its card's spell
  and activated abilities: Time Stop's stand-in still ends the turn. S' forgets
  the trackers and counts the control's table does not keep, and a debug build
  checks every control step against the engine's invariants on that table
  (`families_pair_their_steps`).
- **Why the control is rebuilt at every step.** Rolling the same positions out
  with and without the family plays different games from the first decision.
  Glorious Anthem makes games with more combat and more creatures dying, which
  are expensive decisions; Trinisphere makes games where a player can often only
  pass, the cheapest decision there is. Either moves instructions per decision
  by more than the mechanism does. Two transitions from the same state differ
  only by the family.
- **What is counted.** The transition on S runs in `family::present_step` and
  the one on S' in `family::control_step`, two functions with the same body.
  `family_counts` has a benchmark for each side, each running every rollout in
  full with Callgrind's collection toggled on only inside its function
  (Gungraun's `EntryPoint::Custom`, Callgrind's `--toggle-collect`). Building
  S', advancing it to its decision and matching the choice run outside both
  functions, so neither side counts them.
- **The gate** reports both sums, their ratio and the steps paired and skipped.
  It fails when the ratio passes the verifier's limit in `count-gate.toml`, or
  when more than a tenth of the steps were skipped, since the ratio then
  describes too few of them. The steps come from `benches/family_steps.rs`,
  which runs the same rollouts natively and also prints the rollouts that ended
  in `Stalled(Capacity)` (a list filled, so a `Large` game would have gone on)
  and the layer recomputes per step. Ignite lists each family with a bar as
  `counts:family/<name>`.

Measured on the ARM64 Lima verifier on 2026-11-25, after the control got its own
table, each of the last eight families still measured a ratio of 1.000:
`two-faced` 300,593,256 against 300,611,447 instructions, `levels` 383,200,168
against 383,208,828, `face-down` 361,992,460 against 362,002,502, `outside`
201,553,275 against 201,593,397, `turn-structure` 189,760,044 against
189,767,934, `restart` 257,314,902 against 257,330,478, `cast-in-resolution`
330,385,225 against 330,392,334 and `combat-assignment` 267,645,025 against
267,651,394. The control does take other paths: in `combat-assignment` 71 of
3,118 control decisions lost their banding options, and in `turn-structure` 820
of 1,871 control steps ended in another state than on the present table. What a
capability switches on costs a step too little to show.

A subgame cannot be paired: only `State<Large>` suspends a game, and a state
inside a subgame has no control with a card taken out.
`just count-gate --subgame` (`beryl_bench::subgame`,
`benches/subgame_counts.rs`) reports it unpaired, with no bar: instructions per
rollout step in the opening turns of a subgame against the same turns of a plain
`Large` game, both cut from seeded games with four Shahrazad in the deck. A step
before a subgame begins has Shahrazad only as a spell, which has no static or
triggered ability, so its control is itself.

What a paired step does not see: work the mechanism did earlier that changed the
state, such as a card exiled where it would have gone to a graveyard. Both sides
share that history, and its cost was counted at the step that did it. Nor does
it see a capability check: the control keeps the game's `GameTable`, so a check
behind a `Capability` the family's cards turn on runs on both sides. A family
whose work is all behind such checks (every family from `two-faced` on) measures
1.000, and what those checks cost is in the absent-cost count. Each family's
measured ratio is in the architecture document's third appendix, "Measurements
behind Part II".

To add a family: add it to `family::FAMILIES`, add its name as a
`#[bench::<name>(FamilyWorkload::new("<name>"))]` on both functions in
`benches/family_counts.rs`, add `[family.<name>]` with its `ratio` to
`count-gate.toml`, give each verifier a limit at or below that ratio, and pin
its corpus and pairing in `family/tests.rs`. Swap out cards the family's
positions can do without; the count of copies decides how often a card of the
family is in play.

### Calibrating instruction counts

Run `just bench-count` once to provision `beryl-bench` in Lima or WSL. Then run
`just count-gate --environment` to get the registered Ignite verifier ID and the
versions of the tools in that guest. The gate reports an incomplete result when
the verifier has no profile or the observed runtime differs from it.

Measure an accepted revision in an Ignite lease. Run the rollout twice with
`just bench-count --bench rollout_counts -- --output-format=json` and check that
the Callgrind `Ir` counts agree. For a new profile, record that count under
`profile.<verifier>.limits.instructions`. For an existing profile, follow the
rules in `count-gate.toml` before changing its bar. Record the full revision,
fixture ID and observed runtime in the profile. The initial bars were
378,137,522 on ARM64 Lima and 436,580,889 on x86-64 WSL.

Run `just count-gate` and the paired `--family` checks on that verifier. Confirm
that the pairings are complete enough to judge and that an intentionally lower
limit makes the same measurement fail. Restore the intended limit before
committing. When the corpus or pairing fixture is replaced, update the fixture
ID and recalibrate on each verifier. A toolchain, runner or guest change
requires a new observed runtime and an accepted measurement. Previous trend
files stay under `$BERYL_CACHE/ci/counts/<verifier>/<calibration digest>.jsonl`.

Rollout and Beryl 3D checks can use `beryl_tools.calibration.select` with a TOML
or JSON mapping keyed by verifier ID. Each entry has `revision`, `fixture`,
`runtime` and `limits`. Each caller supplies its fixture ID, probes the runtime
that will run the measurement and validates its own limits. Use `Profile.digest`
in the machine-relative Ignite listing and to scope the trend. Runtime or
profile errors give setup or incomplete results.

For Windows qualification, run
`ignite test --rerun counts:* client-frame-counts:*` on Cygnus. Record the
verifier, checkout revision, and selected guest profiles. A cached pass from
another verifier or calibration cannot establish the new baseline. See
[qualifying Windows checks](ignite.md#qualifying-windows-checks).

[Frame calibrations](beryl-3d.md#frame-calibrations) describes the native and
Callgrind profiles for Beryl 3D.

## Determinism

A benchmark that does different work on different runs can't detect a small
change, however carefully it's timed.

- **Seed every input, with a fixed algorithm.** `beryl_bench::rng::SplitMix64`
  gives the same numbers for a seed forever. Don't use `rand::thread_rng`
  (unseeded) or `StdRng` (its algorithm may change between `rand` releases).
- **Pin the inputs with a test.** `example::tests::inputs_never_change` fails if
  the generated pools change, which would quietly invalidate every saved
  baseline.
- **No randomly seeded hashing in measured code.** `std::collections::HashMap`
  seeds its hasher per process, so iteration order and collisions differ run to
  run, and so do instruction counts. Use a fixed-seed hasher or a `BTreeMap`.
- **Test that old and new agree** (`example::tests::old_and_new_agree`). A
  faster wrong answer is the most common "optimization".
- **Build inputs outside the measurement.** Gungraun evaluates `#[bench::id(…)]`
  arguments before counting starts; in Criterion use `iter_batched` when each
  iteration needs fresh input.
- **`black_box` inputs and outputs**, so the compiler can neither precompute the
  work nor discard it. Return something that depends on all of it
  (`example::over_pools` folds every curve into one number).
- **One thread**, unless parallelism is the subject: scheduling isn't
  repeatable.
- **Vary the inputs.** Calling a function on one small input lets the branch
  predictor memorize it; the example runs 64 different pools per call.

## Instruction counts: Gungraun

Gungraun runs each benchmark once under Valgrind's Callgrind, which simulates
the CPU and counts every instruction. The example's output:

```text
example_counts::mana_curve::new pools:(…)
  Instructions:                     12160|12160      (No change)
  Comparison with old pools:(…)
  Instructions:                     32107|12160      (+164.038%) [+2.64038x]
```

The left column is old and the right new: old runs 2.6 times the instructions,
almost all of it the allocator. Each run is also compared with the previous run
of the same benchmark ("No change"), so editing code and running again shows the
effect directly.

The VM:

- Under Lima, `beryl-bench` is ARM64 Ubuntu on Apple's virtualization, with four
  CPUs and 4 GiB. It mounts only the main checkout, read-only. Under WSL, it is
  a dedicated x86-64 Ubuntu distribution. WSL receives the checkout as a tar
  archive and builds from its ext4 filesystem. Lima uses the same transfer when
  it cannot see the worktree. The transfer keeps unchanged source mtimes so
  Cargo can reuse artifacts. `by bench-count` installs Valgrind, the Rust pinned
  in `rust-toolchain.toml` and the `gungraun-runner` matching `Cargo.lock`.
- Each worktree builds into a target directory of its own in the VM, so it
  counts its own code: Cargo names the engine's artifacts the same in every
  worktree and trusts file times. A new one starts as a copy of the count gate's
  warm directory, rebuilding only the workspace; those of removed worktrees are
  deleted.
- Runs take turns: a run waits on a lock in the VM ("another bench-count run is
  using the VM") while another provisions, builds or counts. Each worktree keeps
  its own Gungraun results, so "No change" compares with that worktree's
  previous run, not another's.
- It keeps running between uses. `limactl stop beryl-bench` frees its memory;
  `limactl delete beryl-bench` removes it (the next `just bench-count` recreates
  it).

Writing one: see `benches/example_counts.rs`. Gungraun is a Linux-only
dev-dependency, so the benchmark is `#[cfg(target_os = "linux")]` with a stub
`main` elsewhere. Same `#[bench::<id>]` ids plus `compare_by_id = true` print
new against old. Gungraun's macros reject doc comments on benchmark functions.

What counts don't tell you:

- **Instructions aren't time.** A cache miss costs hundreds of cycles; a
  mispredicted branch a dozen; a division more than an addition. "Estimated
  Cycles" is a crude model of that.
- **The VM runs Linux.** It's the same arm64 instruction set we ship, but glibc,
  not macOS, provides `malloc`, so allocation-heavy code counts differently from
  how it runs in the app.

Counts show what changed. Native wall-clock timings measure the effect.

## Timing on macOS and Windows

### Timing under load

`just bench` times under whatever load the machine has. It builds first, then
logs how much of the CPU is in use and which compilers are running (`rustc`,
`cargo`, `clippy-driver`, `clang`, `swift-frontend`). On macOS it also logs the
five busiest processes. It samples the load again after timing. It refuses to
time on battery or in macOS Low Power Mode or Windows Energy Saver. On macOS it
also checks thermal and performance warnings and CPU speed limits.

`--force` times anyway, for checking that a harness works, not for numbers you
keep.

To get an answer you can trust under load:

- **Count instructions first.** Counts don't depend on load at all.
- **Compare old and new in one executable** with the ABBA harness, never two
  runs taken minutes apart. Other sessions' load at any moment lands on both
  versions within the same round, so it adds noise to the ratio but no bias.
- **Read the spread, not one number.** The harness reports the median ratio of B
  to A and its 95% confidence interval. Start at the default 300 rounds and
  double `--rounds` until the interval is narrower than the effect you care
  about. Many rounds disturbed by other load are expected; the median and the
  interval already allow for them.
- **Treat absolute times as rough.** Criterion's times per call move with the
  load, so compare its baselines only alongside the load each run logged.
- **For whole programs** (builds, tournaments, the scale rehearsal), report
  CPU-seconds beside wall time, use the load-adjusted clock that the checks use
  (`tools/src/beryl_tools/clock.py`: wall time divided by the load beyond the
  cores and by the machine's speed factor), and alternate old and new runs (A B
  B A) for several rounds, reporting the median and the range.

### Core performance classes

Apple silicon has two core types that run the same code at different speeds.
`sysctl hw.perflevel0` is the faster kind and `hw.perflevel1` the slower: 6
"Super" and 12 "Performance" cores on an M5 Max, "Performance" and "Efficiency"
on earlier chips. A benchmark whose thread moves between them measures the move.

macOS can't pin a thread to a core. It places threads by quality-of-service
class, and keeps user-interactive threads on the fastest cores while one is
free. So:

- Call `beryl_bench::cores::prefer_fastest_cores()` before timing (the example
  benchmarks do, first thing in `main`).
- Stay single-threaded, or use at most as many threads as `hw.perflevel0` has
  cores.
- Expect other load to take the fast cores at times. The ABBA harness counts a
  round with a block more than 25% off its version's median as disturbed (a
  slower core or an interruption), and notes when over a tenth of the rounds
  were. Its median and interval allow for them; more rounds narrow the interval.

On Windows, `prefer_fastest_cores()` raises the calling thread's priority and
requests the available CPU sets with the highest efficiency class. On CPUs with
one class, it raises the priority and leaves Windows to place the thread.

### Old against new: the ABBA harness

Criterion times old to completion and then new. Anything that drifts in between
(temperature, clock speed, a background process waking up) lands on one side and
looks like a difference.

`beryl_bench::abba::compare` instead times both versions in one executable, in
blocks of about 5 ms, in the order A B B A (B A A B on odd rounds), for 300
rounds:

- Both versions' blocks share the same average time in every round, so drift
  that's roughly linear over a round cancels.
- Alternating the order cancels any advantage of going first, such as warm
  caches.
- Each round gives one ratio, B over A. The report is the median ratio, with a
  95% confidence interval from order statistics, so a few interrupted rounds
  barely move it.
- Blocks of milliseconds dwarf the clock's 42 ns resolution on Apple silicon.

```text
mana_curve (A old, B new): A 1.8µs, B 1.1µs per call
  B/A = 0.6400 (95% CI 0.6383 to 0.6411; 30 ABBA rounds of 4096 calls per block)
  B is 36.0% faster
```

The interval decides it: entirely below 1 means B is faster, entirely above
means slower, and spanning 1 means no difference detected. Raise `--rounds`
until the interval is narrower than the effect you care about.

To use it, keep the old version next to the new while you work (as in
`example.rs`), and write a short `harness = false` benchmark like
`benches/example_abba.rs`. Delete the old version once you've decided.

To track absolute ABBA timings over time, save a named run and compare a later
run on the same machine with the same benchmark and harness flags.

```bash
just bench --bench example_abba --save-abba-baseline before
just bench --bench example_abba --abba-baseline before
```

The comparison shows the prior and current times per call, B/A ratios and
intervals. Each interval describes its own run. Use the interleaved A/B
comparison within one executable to measure the speedup from a code change.

### Criterion

`benches/example_timing.rs` raises Criterion's defaults: 5 s warm-up, 20 s
measurement, 200 samples, changes under 2% called noise, and significance at 1%.
Save a baseline before a change and compare after:

```bash
just bench -p beryl_bench --bench example_timing -- --save-baseline before
```

```bash
just bench -p beryl_bench --bench example_timing -- --baseline before
```

The two runs are minutes apart, so drift between them, including any change in
the machine's load, is exactly what this compares. Trust a small change only
when counts and ABBA agree.

`just bench` keeps Criterion and ABBA baselines in
`$BERYL_CACHE/bench/<os>-<arch>-<hostname>/` under `criterion/` and `abba/`.
They are shared by worktrees on the same machine, including on Windows.
Criterion honors `CRITERION_HOME` if you set it. Point it at an earlier
checkout's `engine/target/criterion` directory to use baselines saved there.

## Optimizing something, step by step

1. Write the benchmark against the current code, with deterministic inputs, and
   a test that pins them.
2. `just bench-count` for a starting count.
3. Write the new version next to the old; test that they agree.
4. `just bench-count`: did the instructions drop, and where? Look at the cache
   lines too.
5. `just bench` with an ABBA benchmark: is the interval below 1? If it spans 1,
   raise `--rounds` before concluding there is no difference.
6. If counts and time disagree, find out why before deciding: usually memory
   (counts miss cache misses) or the allocator (the VM uses glibc).
7. Delete the old version. Keep the `_counts` and `_timing` benchmarks for
   regressions.

## Pitfalls

- **Code layout.** An unrelated change can move a hot loop across a cache-line
  or page boundary and shift its time by a few percent. Counts don't move. Be
  skeptical of timing differences under about 2% that counts don't show.
- **`just bench` builds the `search` profile** (fat LTO, `panic = "abort"`,
  `codegen-units = 1` and a machine-specific CPU), as the headless research
  binaries are built. `--profile bench` inherits the engine workspace's
  `release` profile; `just bench-count` uses it, since Valgrind's CPU is not the
  M4. Never benchmark a debug build.
- **Use deterministic work counts in checks.** Wall-clock timings depend on what
  else the machine is running. For native rendering, see
  [Beryl 3D frame gates](beryl-3d.md#performance-gates).
