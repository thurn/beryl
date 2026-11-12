//! Compare what the champion's rollout policy does after two moves at a suite position.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Decision, Observation, Outcome, Scratch, State, view};
use beryl_search::{
    DEEP_SEARCHER, ROLLOUT_TEMPERATURE, RolloutPolicy, Softmax, soft_rollout_after,
};

use crate::blunders::{Reached, entries, reach};
use crate::transcript::{Transcript, label, labels};

const USAGE: &str = "usage: beryl_cli why ENTRY --a LABEL --b LABEL [--rollouts 2000]";
const DEFAULT_ROLLOUTS: u32 = 2_000;
const EXAMPLES: usize = 3;

pub fn main(args: &[String]) -> Result<(), String> {
    print!("{}", run(args)?);
    Ok(())
}

fn run(args: &[String]) -> Result<String, String> {
    let (name, flags) = args.split_first().ok_or(USAGE)?;
    let [mut a, mut b] = [None, None];
    let mut rollouts = DEFAULT_ROLLOUTS;
    let mut flags = flags.iter().map(String::as_str);
    while let Some(flag) = flags.next() {
        let value = flags
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        match flag {
            "--a" => a = Some(value),
            "--b" => b = Some(value),
            "--rollouts" => rollouts = crate::number(flag, value)?,
            _ => return Err(format!("unknown flag {flag}\n{USAGE}")),
        }
    }
    let names = [a.ok_or(USAGE)?, b.ok_or(USAGE)?];
    if rollouts == 0 {
        return Err("--rollouts must be positive".to_owned());
    }
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
    let choices = [find(names[0])?, find(names[1])?];
    Ok(format!(
        "{name}, P{} acting, {rollouts} paired rollouts\nA  {:?}\nB  {:?}\n{}",
        at.d.player.get(),
        names[0],
        names[1],
        compare(&at, choices, rollouts),
    ))
}

type Event = (u8, String, String);

fn compare(at: &Reached, choices: [Choice; 2], rollouts: u32) -> String {
    let policy = DEEP_SEARCHER.rollout_policy(&at.t);
    let mut scratch = Scratch::new();
    let mut wins = [0; 2];
    let mut discordant = 0;
    let mut events = BTreeMap::<Event, [u64; 2]>::new();
    let mut examples = [Vec::new(), Vec::new()];
    let player = at.d.player;
    let last_turn = at.s.turn().number().saturating_add(1);
    for pair in 0..rollouts {
        let (world, rng) = deal(at, pair);
        let outcomes = [0, 1].map(|side| {
            let mut state = world;
            play(
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
            )
        });
        let won = outcomes.map(|outcome| outcome == Outcome::Win(player));
        for side in [0, 1] {
            wins[side] += u32::from(won[side]);
            if won[side]
                && matches!(outcomes[1 - side], Outcome::Win(other) if other != player)
                && examples[side].len() < EXAMPLES
            {
                examples[side].push(pair);
            }
        }
        discordant += u32::from(won[0] != won[1]);
    }

    let n = f64::from(rollouts);
    let [a, b] = wins.map(|wins| f64::from(wins) / n);
    let diff = a - b;
    let mut out = format!(
        "\nWin rate\nA  {:.2}%    B  {:.2}%    A - B  {:+.2} pp",
        100.0 * a,
        100.0 * b,
        100.0 * diff,
    );
    // The pair differences are -1, 0 or +1, so their sum of squares is the discordant count.
    if rollouts > 1 {
        let variance = (f64::from(discordant) / n - diff * diff).max(0.0);
        let _ = writeln!(
            out,
            "    paired SE  {:.2} pp",
            100.0 * (variance / (n - 1.0)).sqrt()
        );
    } else {
        let _ = writeln!(out, "    paired SE  unavailable for one pair");
    }

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
}
