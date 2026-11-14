//! Compare the rollouts and search branches of two moves at a suite position.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Decision, Observation, Outcome, Scratch, Seat, State, view};
use beryl_search::{
    DEEP_SEARCHER, Family, LongSearch, ROLLOUT_TEMPERATURE, RolloutPolicy, Settings, Softmax,
    Visits, outcome_value, soft_rollout_after,
};

use crate::blunders::{Reached, entries, reach, run_seed};
use crate::game::parse_work;
use crate::transcript::{Transcript, label, labels};

const USAGE: &str = "usage: beryl_cli why ENTRY --a LABEL --b LABEL [--rollouts 2000] \
    [--family FAMILY] [--tree [--work 3M] [--seed 0]]";
const DEFAULT_ROLLOUTS: u32 = 2_000;
const EXAMPLES: usize = 3;

pub fn main(args: &[String]) -> Result<(), String> {
    print!("{}", run(args)?);
    Ok(())
}

fn run(args: &[String]) -> Result<String, String> {
    let (name, flags) = args.split_first().ok_or(USAGE)?;
    let options = parse(flags)?;
    let entry = entries()?
        .into_iter()
        .find(|entry| entry.name == *name)
        .ok_or_else(|| format!("unknown blunder entry {name:?}"))?;
    let at = reach(&entry)?;
    let candidates = labels(&at.t, view(&at.s, &at.t, &mut Scratch::new()), &at.d);
    let find = |name: &str| {
        // Identically labeled objects use the first candidate, in the suite's printed order.
        candidates
            .iter()
            .position(|candidate| candidate == name)
            .map(|index| Choice(index as u16))
            .ok_or_else(|| {
                format!(
                    "unknown choice {name:?}. Choices: {}",
                    candidates.join(" | ")
                )
            })
    };
    let [a, b] = options.names;
    let choices = [find(a)?, find(b)?];
    let mut out = format!(
        "{name}, P{} acting, {} paired rollouts\n{}\nA  {a:?}\nB  {b:?}\n{}",
        at.d.player.get(),
        options.rollouts,
        options.family,
        compare(&at, choices, options.family, options.rollouts),
    );
    if let Some((work, seed)) = options.tree {
        write_tree(
            &mut out,
            &at,
            choices,
            &candidates,
            options.family,
            (work, seed, name),
        );
    }
    Ok(out)
}

struct Options<'a> {
    names: [&'a str; 2],
    rollouts: u32,
    family: Family,
    tree: Option<(u64, u32)>,
}

fn parse(flags: &[String]) -> Result<Options<'_>, String> {
    let [mut a, mut b] = [None, None];
    let mut rollouts = DEFAULT_ROLLOUTS;
    let mut family = DEEP_SEARCHER;
    let (mut tree, mut work, mut seed) = (false, None, None);
    let mut flags = flags.iter().map(String::as_str);
    while let Some(flag) = flags.next() {
        if flag == "--tree" {
            tree = true;
            continue;
        }
        let value = flags
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        match flag {
            "--a" => a = Some(value),
            "--b" => b = Some(value),
            "--rollouts" => rollouts = crate::number(flag, value)?,
            "--family" => family = value.parse()?,
            "--work" => {
                work =
                    Some(parse_work(value).ok_or_else(|| format!("--work: no budget {value:?}"))?);
            }
            "--seed" => seed = Some(crate::number(flag, value)?),
            _ => return Err(format!("unknown flag {flag}\n{USAGE}")),
        }
    }
    let names = [a.ok_or(USAGE)?, b.ok_or(USAGE)?];
    if rollouts == 0 {
        return Err("--rollouts must be positive".to_owned());
    }
    if !tree && (work.is_some() || seed.is_some()) {
        return Err("--work and --seed require --tree".to_owned());
    }
    Ok(Options {
        names,
        rollouts,
        family,
        tree: tree.then(|| (work.unwrap_or(3_000_000), seed.unwrap_or(0))),
    })
}

type Event = (u8, String, String);

fn compare(at: &Reached, choices: [Choice; 2], family: Family, rollouts: u32) -> String {
    let policy = family.rollout_policy(&at.t);
    let mut scratch = Scratch::new();
    let mut summary = RolloutSummary::default();
    let mut events = BTreeMap::<Event, [u64; 2]>::new();
    let mut examples = [Vec::new(), Vec::new()];
    let player = at.d.player;
    let start = at.s.turn().number();
    let last_turn = start.saturating_add(1);
    for pair in 0..rollouts {
        let (world, rng) = deal(at, pair);
        let results = [0, 1].map(|side| {
            let mut state = world;
            let outcome = play(
                at,
                &mut state,
                &mut scratch,
                &policy,
                choices[side],
                rng,
                |obs, d, c| {
                    let shown = obs.shown();
                    if shown.turn().number() <= last_turn {
                        let event = (
                            d.player.get(),
                            format!("{:?}", d.kind),
                            label(&at.t, shown, d, c),
                        );
                        events.entry(event).or_default()[side] += 1;
                    }
                },
            );
            (outcome, state.turn().number().saturating_sub(start))
        });
        summary.add(player, results);
        for side in [0, 1] {
            if results[side].0 == Outcome::Win(player)
                && matches!(results[1 - side].0, Outcome::Win(other) if other != player)
                && examples[side].len() < EXAMPLES
            {
                examples[side].push(pair);
            }
        }
    }

    let n = f64::from(rollouts);
    let mut out = summary.report(n);

    let mut changed: Vec<_> = events.into_iter().filter(|(_, [a, b])| a != b).collect();
    // Stable sorting keeps the player, kind and label order for tied absolute differences.
    changed.sort_by_key(|(_, [a, b])| std::cmp::Reverse(a.abs_diff(*b)));
    let _ = writeln!(
        out,
        "\nDecisions through turn {last_turn} (counts per 1,000 rollouts)\n{:>8} {:>8}  Player  Decision        Choice",
        "A", "B",
    );
    for ((seat, kind, label), [a, b]) in changed.into_iter().take(30) {
        let _ = writeln!(
            out,
            "{:8.1} {:8.1}  P{seat}      {kind:<14}  {label}",
            1_000.0 * a as f64 / n,
            1_000.0 * b as f64 / n
        );
    }

    write_examples(&mut out, at, &policy, choices, examples);
    out
}

#[derive(Default)]
struct RolloutSummary {
    wins: Paired,
    scores: Paired,
    turns: [f64; 2],
}

impl RolloutSummary {
    fn add(&mut self, player: Seat, results: [(Outcome, u16); 2]) {
        self.wins
            .add(results.map(|(outcome, _)| f64::from(outcome == Outcome::Win(player))));
        self.scores.add(results.map(|(outcome, turns)| {
            let p0 = outcome_value(outcome, turns);
            if player == Seat::P0 { p0 } else { 1.0 - p0 }
        }));
        for (total, (_, turns)) in self.turns.iter_mut().zip(results) {
            *total += f64::from(turns);
        }
    }

    fn report(&self, n: f64) -> String {
        let [a, b] = self.wins.means(n);
        let diff = a - b;
        let mut out = format!(
            "\nWin rate\nA  {:.2}%    B  {:.2}%    A - B  {:+.2} pp",
            100.0 * a,
            100.0 * b,
            100.0 * diff,
        );
        if let Some(se) = self.wins.error(n, diff) {
            let _ = writeln!(out, "    paired SE  {:.2} pp", 100.0 * se);
        } else {
            let _ = writeln!(out, "    paired SE  unavailable for one pair");
        }
        let [a, b] = self.scores.means(n);
        let [turns_a, turns_b] = self.turns.map(|sum| sum / n);
        let diff = a - b;
        let _ = write!(
            out,
            "Search score\nA  {a:.4} ({turns_a:.2} turns)    B  {b:.4} ({turns_b:.2} turns)    A - B  {diff:+.4}",
        );
        if let Some(se) = self.scores.error(n, diff) {
            let _ = writeln!(out, "    paired SE  {se:.4}");
        } else {
            let _ = writeln!(out, "    paired SE  unavailable for one pair");
        }
        out
    }
}

#[derive(Default)]
struct Paired {
    sums: [f64; 2],
    squared_differences: f64,
}

impl Paired {
    fn add(&mut self, [a, b]: [f64; 2]) {
        self.sums[0] += a;
        self.sums[1] += b;
        self.squared_differences += (a - b).powi(2);
    }

    fn means(&self, n: f64) -> [f64; 2] {
        self.sums.map(|sum| sum / n)
    }

    fn error(&self, n: f64, diff: f64) -> Option<f64> {
        (n > 1.0)
            .then(|| ((self.squared_differences / n - diff * diff).max(0.0) / (n - 1.0)).sqrt())
    }
}

fn write_tree(
    out: &mut String,
    at: &Reached,
    choices: [Choice; 2],
    candidates: &[String],
    family: Family,
    (work, seed, entry): (u64, u32, &str),
) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let settings = Settings {
        family,
        work,
        threads,
    };
    let report = LongSearch::new(&at.t, settings, run_seed(entry, seed)).choose_with_tree(
        Observation::new(&at.s, at.d.player),
        &at.d,
        choices,
        labels,
    );
    let chosen = &candidates[usize::from(report.choice.0)];
    let _ = writeln!(
        out,
        "\nTree ({work} steps, suite seed {seed}, chose {chosen:?})"
    );
    if let Some(turn) = report.turn {
        let _ = writeln!(
            out,
            "Turn search used {} proposal steps and {} scoring steps. \
             {} lines completed, {} merged, {} reserves kept, {} continuation fallbacks.{}",
            turn.proposal_work,
            turn.scoring_work,
            turn.completed,
            turn.merged,
            turn.reserved,
            turn.fallbacks,
            if turn.defaulted {
                " Kept the default after incomplete scoring."
            } else {
                ""
            },
        );
    }
    for ((side, choice), branch) in ["A", "B"].into_iter().zip(choices).zip(report.branches) {
        let name = &candidates[usize::from(choice.0)];
        let _ = writeln!(out, "{side} {name:?}  {}", stats(branch.visits));
        for (depth, level) in branch.levels.into_iter().enumerate() {
            for (g, decision) in level.decisions.into_iter().enumerate() {
                let _ = writeln!(
                    out,
                    "  {}. P{} {:?} ({} trees)",
                    depth + 1,
                    decision.player.get(),
                    decision.kind,
                    decision.trees
                );
                for (c, (name, visits)) in decision.children.into_iter().enumerate() {
                    let mark = if level.follow == Some((g, c)) {
                        '>'
                    } else {
                        ' '
                    };
                    let _ = writeln!(out, "    {mark} {name}: {}", stats(visits));
                }
            }
        }
    }
}

fn stats(visits: Visits) -> String {
    let mean = visits
        .mean()
        .map_or_else(|| "-".to_owned(), |mean| format!("{mean:.4}"));
    format!("{} visits, mean {mean}", visits.count)
}

fn write_examples(
    out: &mut String,
    at: &Reached,
    policy: &RolloutPolicy,
    choices: [Choice; 2],
    examples: [Vec<u32>; 2],
) {
    let mut scratch = Scratch::new();
    for (heading, pairs) in ["A won, B lost", "B won, A lost"].into_iter().zip(examples) {
        let _ = writeln!(out, "\n{heading}");
        if pairs.is_empty() {
            let _ = writeln!(out, "  No such pairs.");
        }
        // Recreate only the displayed pairs, so collecting examples costs no transcript
        // formatting on the thousands of other runs.
        for pair in pairs {
            let (world, rng) = deal(at, pair);
            let _ = writeln!(out, "\nPair {}", pair + 1);
            for (side, choice) in ["A", "B"].into_iter().zip(choices) {
                let mut transcript = Transcript::new(["", ""]);
                let shown = view(&world, &at.t, &mut scratch);
                transcript.decision(&at.t, shown, &at.d, choice, &[]);
                let mut state = world;
                let outcome = play(
                    at,
                    &mut state,
                    &mut scratch,
                    policy,
                    choice,
                    rng,
                    |obs, d, c| {
                        transcript.decision(&at.t, obs.shown(), d, c, &[]);
                    },
                );
                transcript.end(&at.t, &state, outcome, &[]);
                let _ = writeln!(out, "\n{side}\n{}", transcript.text());
            }
        }
    }
}

fn deal(at: &Reached, pair: u32) -> (State, SplitMix64) {
    let mut rng = SplitMix64::new(u64::from(pair));
    let world = Observation::new(&at.s, at.d.player).determinize(&at.t, &mut rng);
    (world, rng)
}

fn play(
    at: &Reached,
    state: &mut State,
    scratch: &mut Scratch,
    policy: &RolloutPolicy,
    choice: Choice,
    mut rng: SplitMix64,
    record: impl FnMut(Observation<'_>, &Decision, Choice),
) -> Outcome {
    let mut soft = Softmax {
        rng: &mut rng,
        temperature: ROLLOUT_TEMPERATURE,
    };
    soft_rollout_after(state, &at.t, scratch, policy, &mut soft, choice, record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beryl_search::{Agent, Method};

    fn oust(a: &str, b: &str) -> Result<String, String> {
        run(&["oust-own-token", "--a", a, "--b", b, "--rollouts", "8"].map(str::to_owned))
    }

    #[test]
    fn identical_choices_have_identical_rollouts() {
        let out = oust("pass", "pass").unwrap();
        assert!(
            out.contains("A - B  +0.00 pp    paired SE  0.00 pp"),
            "{out}"
        );
        assert!(out.contains("Choice\n\nA won, B lost"), "{out}");
        assert_eq!(out.matches("No such pairs.").count(), 2, "{out}");
        let score = out.split_once("Search score\nA  ").unwrap().1;
        let (a, rest) = score.split_once("    B  ").unwrap();
        let (b, rest) = rest.split_once("    A - B  ").unwrap();
        assert_eq!(a, b);
        assert!(rest.starts_with("+0.0000    paired SE  0.0000"));
    }

    #[test]
    fn compares_oust_and_pass() {
        let out = oust("cast Oust", "pass").unwrap();
        assert!(out.contains("\nWin rate\n"));
        assert!(out.contains("Target          Rogue Token (P1)"), "{out}");
        assert!(out.contains("\nA won, B lost\n"));
        assert!(out.contains("\nB won, A lost\n"));
    }

    #[test]
    fn an_unknown_choice_lists_candidates() {
        let err = oust("missing", "pass").unwrap_err();
        assert!(err.contains("unknown choice \"missing\""));
        assert!(err.contains("cast Oust"));
        assert!(err.contains("pass"));
    }

    #[test]
    fn prints_both_tree_branches() {
        let out = run(&[
            "oust-own-token",
            "--a",
            "cast Oust",
            "--b",
            "pass",
            "--rollouts",
            "1",
            "--tree",
            "--work",
            "100k",
        ]
        .map(str::to_owned))
        .unwrap();
        let tree = out.split_once("\nTree (100000 steps").unwrap().1;
        assert!(tree.contains("\nA \"cast Oust\""), "{tree}");
        assert!(tree.contains("\nB \"pass\""), "{tree}");
        assert!(tree.contains("1. P"), "{tree}");
    }

    #[test]
    fn tree_report_counts_the_ordinary_search_visits() {
        let name = "oust-own-token";
        let entry = entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == name)
            .unwrap();
        let at = reach(&entry).unwrap();
        let obs = Observation::new(&at.s, at.d.player);
        let candidates = labels(&at.t, &at.s, &at.d);
        let choices = ["cast Oust", "pass"]
            .map(|name| Choice(candidates.iter().position(|label| label == name).unwrap() as u16));
        for method in [DEEP_SEARCHER.method, Method::Ismcts { trees: 4 }] {
            let family = Family {
                method,
                ..DEEP_SEARCHER
            };
            let settings = Settings {
                family,
                work: 100_000,
                threads: 2,
            };
            let seed = run_seed(name, 0);
            let mut ordinary = LongSearch::new(&at.t, settings, seed);
            let mut traced = LongSearch::new(&at.t, settings, seed);
            let report = traced.choose_with_tree(obs, &at.d, choices, labels);
            assert_eq!(
                report.choice,
                ordinary.choose(obs, &at.d, beryl_search::AgentDecision(0))
            );
            let search = ordinary.last_search().unwrap();
            for (branch, choice) in report.branches.into_iter().zip(choices) {
                let i = usize::from(choice.0);
                let mut parent = branch.visits.count;
                assert_eq!(parent, u64::from(search.visits[i]));
                let expected = search.rewards[i] / f64::from(search.visits[i]);
                assert!((branch.visits.mean().unwrap() - expected).abs() < 1e-12);
                assert!(!branch.levels.is_empty());
                for level in branch.levels {
                    let children = level.decisions.iter().flat_map(|d| &d.children);
                    assert!(children.map(|(_, visits)| visits.count).sum::<u64>() <= parent);
                    let (group, child) = level.follow.unwrap();
                    parent = level.decisions[group].children[child].1.count;
                }
            }
        }
    }
}
