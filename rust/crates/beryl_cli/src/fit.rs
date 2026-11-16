//! Fit the rollout policy's existing scores to moves confirmed by paired search rollouts.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use beryl_engine::{
    Choice, Decision, GameTable, LargeCardIx, Replay, Scratch, State, apply, observe,
};
use beryl_search::{AnchorCheck, CardValues, Knowledge, PolicyWeights, RolloutPolicy, Rules};

use crate::anchor_replay::{games, log, logged_decision, start};
use crate::game::mirror;
use crate::match_log::{AnchorRecord, files, game::Line};

const USAGE: &str = "usage: beryl_cli fit <match-dir>... [--seed N] [--out FILE]";

struct Sample {
    state: State,
    decision: Decision,
    label: Choice,
}

#[derive(Default)]
struct Samples {
    train: Vec<Sample>,
    held: Vec<Sample>,
}

fn held_out(seed: u64, game_seed: u64, position: Option<&str>) -> bool {
    let mut hash = DefaultHasher::new();
    seed.hash(&mut hash);
    // Repeated starts, including swapped seats across runs, stay on the same side.
    match position {
        Some(name) => name.hash(&mut hash),
        None => game_seed.hash(&mut hash),
    }
    hash.finish().is_multiple_of(5)
}

fn confirmed(a: AnchorRecord, choice: u16) -> bool {
    let check = AnchorCheck {
        greedy: Choice(a.greedy),
        search: Choice(a.search),
        lead: a.lead,
        error: a.error,
        trees: a.trees,
        pairs: a.pairs,
    };
    a.pairs.is_some() && a.search == choice && check.overrides()
}

fn collect(
    dir: &Path,
    table: &GameTable,
    seed: u64,
    samples: &mut Samples,
) -> Result<usize, String> {
    let mut labels = 0;
    for game in games(dir)? {
        let lines = log::<Line>(dir, &game)?;
        let is_label = |line: &Line| {
            line.search
                .as_ref()
                .and_then(|s| s.anchor)
                .is_some_and(|a| confirmed(a, line.choice))
        };
        let Some(last) = lines.iter().rposition(is_label) else {
            continue;
        };
        let target = if held_out(seed, game.seed, game.position.as_deref()) {
            &mut samples.held
        } else {
            &mut samples.train
        };
        let mut state = start(dir, table, &game)?;
        let mut scratch = Scratch::new();
        for (index, line) in lines[..=last].iter().enumerate() {
            let decision = logged_decision(
                &mut state,
                table,
                &mut scratch,
                &game,
                line.seat,
                line.choice,
            )
            .map_err(|e| format!("{} at decision {index}: {e}", dir.display()))?;
            if is_label(line) {
                let search = line.search.as_ref().expect("label has search statistics");
                let anchor = search.anchor.expect("label has a paired check");
                if decision.len() != search.visits.len()
                    || usize::from(anchor.greedy) >= decision.len()
                {
                    return Err(format!(
                        "{}: {} decision {index}: candidate count or default no longer fits",
                        dir.display(),
                        game.game,
                    ));
                }
                target.push(Sample {
                    state,
                    decision,
                    label: Choice(line.choice),
                });
                labels += 1;
            }
            apply(
                &mut state,
                table,
                &mut scratch,
                &mut Replay,
                Choice(line.choice),
            );
        }
    }
    Ok(labels)
}

/// A card value or policy constant and the range it can move from its tuned value.
#[derive(Clone, Copy)]
struct Coordinate {
    name: &'static str,
    base: i32,
    radius: i32,
    step: i32,
}

fn coordinates(table: &GameTable) -> Vec<Coordinate> {
    let tuned = PolicyWeights::TUNED;
    let values = CardValues::new(table);
    let cards: BTreeMap<&str, i16> = table
        .indices::<LargeCardIx>()
        .map(|card| (table.def(card).name, values.get(card).value))
        .collect();
    let mut coordinates: Vec<_> = [
        ("removal_min", tuned.removal_min, 20, 5),
        ("counter_min", tuned.counter_min, 20, 5),
        ("land_max", tuned.land_max, 10, 2),
        ("power", tuned.power, 3, 1),
        ("loyalty", tuned.loyalty, 4, 1),
    ]
    .into_iter()
    .map(|(name, base, radius, step)| Coordinate {
        name,
        base,
        radius,
        step,
    })
    .collect();
    coordinates.extend(cards.into_iter().map(|(name, value)| Coordinate {
        name,
        base: i32::from(value),
        radius: 20,
        step: 5,
    }));
    coordinates
}

fn weights_and_cards(
    coordinates: &[Coordinate],
    values: &[i32],
) -> (PolicyWeights, Vec<(&'static str, i16)>) {
    let weights = PolicyWeights {
        removal_min: values[0],
        counter_min: values[1],
        land_max: values[2],
        power: values[3],
        loyalty: values[4],
    };
    let cards = coordinates[5..]
        .iter()
        .zip(&values[5..])
        .filter(|(c, value)| c.base != **value)
        .map(|(c, &value)| {
            (
                c.name,
                i16::try_from(value).expect("card value fits in i16"),
            )
        })
        .collect();
    (weights, cards)
}

fn agreement(policy: &RolloutPolicy, table: &GameTable, samples: &[Sample]) -> usize {
    let mut scratch = Scratch::new();
    samples
        .iter()
        .filter(|s| {
            let obs = observe(&s.state, table, &mut scratch, s.decision.decider);
            policy.choose(table, obs, &s.decision) == s.label
        })
        .count()
}

/// Search each coordinate on a coarse grid and then in single steps. A move must earn
/// enough additional labels to pay its distance from the hand-set number.
fn fit(coordinates: &[Coordinate], mut agrees: impl FnMut(&[i32]) -> usize) -> Vec<i32> {
    let mut values: Vec<_> = coordinates.iter().map(|c| c.base).collect();
    let mut hits = agrees(&values);
    for fine in [false, true, true] {
        let mut changed = false;
        for (i, c) in coordinates.iter().enumerate() {
            let original = values[i];
            let mut best = (original, hits);
            let penalty = |value: i32| value.abs_diff(c.base) * (5 / c.step).max(1) as u32;
            let merit = |value, count| 4 * count as i64 - i64::from(penalty(value));
            let lower = (c.base - c.radius).max(0);
            let upper = c.base + c.radius;
            let (first, last, step) = if fine {
                (
                    (original - c.step).max(lower),
                    (original + c.step).min(upper),
                    1,
                )
            } else {
                (lower, upper, c.step)
            };
            for value in (first..=last).step_by(step as usize) {
                if value == original {
                    continue;
                }
                values[i] = value;
                let count = agrees(&values);
                if (merit(value, count), std::cmp::Reverse(penalty(value)))
                    > (merit(best.0, best.1), std::cmp::Reverse(penalty(best.0)))
                {
                    best = (value, count);
                }
            }
            (values[i], hits) = best;
            changed |= best.0 != original;
        }
        if fine && !changed {
            break;
        }
    }
    values
}

fn output(coordinates: &[Coordinate], values: &[i32]) -> String {
    let (_, cards) = weights_and_cards(coordinates, values);
    let mut text = "//! Card values and score thresholds generated by just fit.\n\nuse crate::PolicyWeights;\n\npub(crate) const WEIGHTS: PolicyWeights = PolicyWeights {\n".to_owned();
    for (c, value) in coordinates[..5].iter().zip(values) {
        writeln!(text, "    {}: {value},", c.name).unwrap();
    }
    text.push_str("};\n\npub(crate) const VALUES: &[(&str, i16)] = &[\n");
    for (name, value) in cards {
        writeln!(text, "    ({name:?}, {value}),").unwrap();
    }
    text.push_str("];\n");
    text
}

fn report(name: &str, before: usize, after: usize, count: usize) {
    let rate = |n| 100.0 * n as f64 / count as f64;
    eprintln!(
        "{name}: {before}/{count} ({:.2}%) -> {after}/{count} ({:.2}%)",
        rate(before),
        rate(after)
    );
}

pub fn main(args: &[String]) -> Result<(), String> {
    let mut dirs = BTreeSet::new();
    let mut seed = 0;
    let mut out = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => seed = args.next().and_then(|n| n.parse().ok()).ok_or(USAGE)?,
            "--out" => out = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            s if s.starts_with('-') => return Err(USAGE.to_owned()),
            s => {
                dirs.insert(
                    Path::new(s)
                        .canonicalize()
                        .map_err(|e| format!("{s}: {e}"))?,
                );
            }
        }
    }
    if dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    let table = mirror();
    let mut samples = Samples::default();
    for dir in dirs {
        let count = collect(&dir, &table, seed, &mut samples)?;
        eprintln!("{}: {count} paired overrides", dir.display());
    }
    if samples.train.is_empty() || samples.held.is_empty() {
        return Err("need paired overrides in both the training and held-out sets".to_owned());
    }

    let rules = Rules {
        hold: true,
        waste: true,
        ..Rules::default()
    };
    let coordinates = coordinates(&table);
    let baseline = RolloutPolicy::with_rules(&table, Knowledge::Tuned, rules);
    let score = |policy: &RolloutPolicy| agreement(policy, &table, &samples.train);
    let fitted = |values: &[i32]| {
        let (weights, cards) = weights_and_cards(&coordinates, values);
        RolloutPolicy::with_fit(&table, rules, weights, &cards)
    };
    let values = fit(&coordinates, |values| score(&fitted(values)));
    let policy = fitted(&values);
    report(
        "Training agreement",
        score(&baseline),
        score(&policy),
        samples.train.len(),
    );
    report(
        "Held-out agreement",
        agreement(&baseline, &table, &samples.held),
        agreement(&policy, &table, &samples.held),
        samples.held.len(),
    );
    let mut changes: Vec<_> = coordinates
        .iter()
        .zip(&values)
        .filter(|(c, v)| c.base != **v)
        .collect();
    changes.sort_by_key(|(c, v)| std::cmp::Reverse(c.base.abs_diff(**v)));
    for (c, value) in changes.into_iter().take(10) {
        eprintln!("{}: {} -> {value}", c.name, c.base);
    }

    let generated = output(&coordinates, &values);
    if let Some(path) = out {
        files::write_whole(&path, generated.as_bytes())?;
    } else {
        print!("{generated}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use beryl_cards::{CARDS, names};
    use beryl_engine::{Step, TurnStep};
    use beryl_scenario::{Action, Live, P0, Scenario};
    use beryl_search::Family;

    use super::*;

    #[test]
    fn fit_corrects_an_overvalued_land_deterministically() {
        let scenario = Scenario::new("choice of land")
            .at(4, P0, TurnStep::PrecombatMain)
            .hand(P0, &[names::ISLAND, names::TUNDRA]);
        let mut live = Live::new(&CARDS, &scenario).unwrap();
        let Step::Decide(decision) = live.position(Action::pass(P0)).unwrap() else {
            panic!("expected a land play");
        };
        let table = live.table();
        let label = (0..decision.len())
            .map(|i| Choice(i as u16))
            .find(|&c| live.name_of(&decision, c) == Some(names::TUNDRA))
            .unwrap();
        let sample = Sample {
            state: *live.state(),
            decision,
            label,
        };
        let coordinate = Coordinate {
            name: names::TUNDRA,
            base: 14,
            radius: 10,
            step: 2,
        };
        let score = |values: &[i32]| {
            let policy = RolloutPolicy::with_fit(
                table,
                Rules::default(),
                PolicyWeights::TUNED,
                &[(coordinate.name, values[0] as i16)],
            );
            16 * agreement(&policy, table, std::slice::from_ref(&sample))
        };
        assert_eq!(score(&[coordinate.base]), 0);
        let values = fit(&[coordinate], score);
        assert!(values[0] < coordinate.base);
        assert_eq!(score(&values), 16);
        assert_eq!(values, fit(&[coordinate], score));
    }

    #[test]
    fn a_search_pick_must_pass_the_paired_check_and_be_played() {
        let a = AnchorRecord {
            greedy: 0,
            search: 1,
            lead: 0.06,
            error: 0.01,
            trees: 0,
            pairs: Some(20),
        };
        assert!(confirmed(a, 1));
        assert!(!confirmed(a, 0));
        assert!(!confirmed(AnchorRecord { pairs: None, ..a }, 1));
        assert!(!confirmed(AnchorRecord { error: 0.04, ..a }, 1));
    }

    #[test]
    fn fitted_family_round_trips() {
        let name = "pimc72/hidden/rollout/fitted/anchor+guard+hold+waste+combat+sure+paired";
        let family = name.parse::<Family>().unwrap();
        assert_eq!(family.knowledge, Knowledge::Fitted);
        assert_eq!(family.to_string(), name);
    }
}
