# Benchmarking

How to measure small optimizations in Beryl without fooling yourself. The
`beryl_bench` crate (`rust/crates/beryl_bench`) holds the support code and a
worked example, `example_*`, that follows every rule here.

The problem: a 2% speedup is smaller than the noise in a wall-clock benchmark on
a laptop running a browser, a backup agent and a few agents' builds. So we
measure two ways, and believe a result only when they agree.

## The strategy

1. **Make the benchmark deterministic.** Same inputs, same work, every run.
2. **Count instructions** with Gungraun in a Linux VM (`just bench-count`).
   Exact, identical on every run, unaffected by load. The first and fastest
   answer.
3. **Confirm with interleaved timing** on a quiet Mac (`just bench`): old and
   new in one executable, timed in A B B A order. This catches what counts miss:
   cache misses, branch mispredictions, the real allocator.
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
  `--bench '*_counts'`) in the `beryl-bench` Lima VM, creating it on first use.
- `just bench [--force] [cargo bench flags] [-- harness flags]` builds, checks
  the machine is quiet, then times. `-- --quick` gives the ABBA harness 30
  rounds instead of 300; Criterion takes its own flags, such as
  `-- --save-baseline before`. It builds under the `search` profile with
  `-C target-cpu=apple-m4` unless the flags name a `--profile`.

Name benchmark targets by kind: `<subject>_counts`, `<subject>_abba`,
`<subject>_timing`.

## The cube rollout benchmark

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
- **A speed-up that keeps every answer** reaches a running long-think arena:
  `just longthink --upgrade` adopts the new binary only once the run's games
  replay identically and a sample of its searches choose as they did. A change
  that moves any answer re-pins `rollouts_never_change` and needs
  `just longthink --new`.

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

- `beryl-bench`: arm64 Ubuntu on Apple's virtualization, 4 CPUs, 4 GiB. It
  mounts only the main checkout, read-only, and builds into its own target
  directory. `by bench-count` installs Valgrind, the Rust pinned in
  `rust/rust-toolchain.toml` and the `gungraun-runner` matching `Cargo.lock`.
- Every worktree builds into that one target directory, so runs take turns: a
  run waits on a lock in the VM ("another bench-count run is using the VM")
  while another provisions, builds or counts, then reports its own code's
  counts. Each worktree keeps its own Gungraun results, so "No change" compares
  with that worktree's previous run, not another's.
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

So counts say what changed; the Mac says what it's worth.

## Timing on the Mac

### A quiet machine

`just bench` builds first (a build heats the machine and loads every core), then
refuses to time if:

- more than 10% of the CPU is in use (it names the five busiest processes);
- a compiler is running (`rustc`, `cargo`, `clippy-driver`, `clang`,
  `swift-frontend`): another worktree's build or an Enact check;
- the Mac is on battery, or in Low Power Mode;
- macOS has recorded a thermal or performance warning, or a CPU speed limit.

Before a session of timing:

- Plug in. On a MacBook Pro with a Max chip, choose High Power in System
  Settings > Battery > Energy Mode.
- Quit browsers, Electron apps and anything with animations. Pause backups (Time
  Machine, Arq) and let Spotlight finish indexing.
- Don't run Enact or other agent sessions that build. Stop the Lima VM if it's
  busy.
- Compare runs taken in one sitting, in the same conditions.

`--force` times anyway, for checking that a harness works, not for numbers you
keep.

### Two kinds of core

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
- Keep the machine quiet, so a fast core is always free.
- Watch for the ABBA harness's warning about disturbed rounds: blocks more than
  25% off their median usually mean a slower core or an interruption.

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

The two runs are minutes apart, so drift between them is exactly what this
compares. Trust a small change only when counts and ABBA agree.

## Optimizing something, step by step

1. Write the benchmark against the current code, with deterministic inputs, and
   a test that pins them.
2. `just bench-count` for a starting count.
3. Write the new version next to the old; test that they agree.
4. `just bench-count`: did the instructions drop, and where? Look at the cache
   lines too.
5. `just bench` with an ABBA benchmark on a quiet machine: is the interval below
   1?
6. If counts and time disagree, find out why before deciding: usually memory
   (counts miss cache misses) or the allocator (the VM uses glibc).
7. Delete the old version. Keep the `_counts` and `_timing` benchmarks for
   regressions.

## Pitfalls

- **Code layout.** An unrelated change can move a hot loop across a cache-line
  or page boundary and shift its time by a few percent. Counts don't move. Be
  skeptical of timing differences under about 2% that counts don't show.
- **`just bench` builds the `search` profile** (fat LTO, `panic = "abort"`,
  `codegen-units = 1`, the M4 CPU), as the headless research binaries are built.
  `--profile bench` inherits `release` instead, the shipped app's thin LTO;
  `just bench-count` uses it, since Valgrind's CPU is not the M4. Never
  benchmark a debug build.
- **Don't time in CI.** `just ci` shares the machine with whatever else is
  running; instruction counts could gate CI later, timing never.

## The Windows machine

Not used. It would bring a different instruction set (x86) from the one we ship,
Windows' own background noise, and remote access to maintain. It earns its keep
only as a dedicated Linux benchmark machine: isolated cores, turbo and SMT off,
a fixed clock. Reach for that only if counts plus ABBA on the Mac stop being
enough.
