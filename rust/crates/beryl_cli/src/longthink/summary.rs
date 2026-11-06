//! summary.txt: what the run has learned so far, derived from its logs alone, so it can be
//! rebuilt whenever it is missing.
//!
//! - The table: every entrant by rating, with its 95% margin, games, score, chance of being
//!   the best of all and of the fair ones, engine steps a searched decision and minutes a game.
//! - Sight: at each budget where a hidden search has a twin that sees the hands or
//!   everything, what seeing is worth in Elo.
//! - Families: each one's Elo per tripling of the budget.

use std::collections::HashMap;
use std::fmt::Write;

use beryl_search::{Family, Sight};

use super::ratings::{Chances, Ratings, climb, climb_text, is_fair};
use super::schedule::{Config, budget_of, family_of, open};
use super::{GameRecord, Verdict, rung_name};

/// Every entrant played, best first, then those open but not yet played, by name.
pub fn ranked(c: &Config, r: &Ratings) -> Vec<String> {
    let mut all: Vec<String> = r.names().to_vec();
    for e in open(c, r) {
        if !all.contains(&e) {
            all.push(e);
        }
    }
    let unplayed = |e: &String| r.tally(e).games == 0;
    all.sort_by(|x, y| {
        unplayed(x)
            .cmp(&unplayed(y))
            .then(r.get(y).0.total_cmp(&r.get(x).0))
            .then(x.cmp(y))
    });
    all
}

/// The summary of run `name`'s games, with each game's wall seconds.
pub fn text(
    name: &str,
    c: &Config,
    games: &[GameRecord],
    seconds: &HashMap<String, f64>,
) -> String {
    let r = Ratings::new(c, games, seconds, &[]);
    let mut out = format!(
        "{name}: {} games, {} threads, seed {}, Elo against {}\n\n",
        games.len(),
        c.threads,
        c.seed,
        c.anchor
    );
    let all = ranked(c, &r);
    let best = best(&r, &all, games.len());
    let width = all.iter().map(String::len).max().unwrap_or(0).max(7);
    let _ = writeln!(
        out,
        "{:<width$} {:>6} {:>5} {:>6} {:>13} {:>6} {:>6} {:>9} {:>6}",
        "entrant", "elo", "95%", "games", "w-d-l", "best", "fair", "steps/dec", "min"
    );
    for (e, chance) in all.iter().zip(&best) {
        let (m, s) = r.get(e);
        let t = r.tally(e);
        let rated = t.scored() > 0 || *e == c.anchor;
        let steps = (t.searched > 0).then(|| t.work as f64 / t.searched as f64);
        let _ = writeln!(
            out,
            "{e:<width$} {:>6} {:>5} {:>6} {:>13} {:>5.0}% {:>6} {:>9} {:>6}",
            if rated {
                format!("{m:+.0}")
            } else {
                "-".to_owned()
            },
            if rated {
                format!("±{:.0}", 1.96 * s)
            } else {
                "-".to_owned()
            },
            t.games,
            format!("{}-{}-{}", t.wins, t.draws, t.losses),
            100.0 * chance.overall,
            if is_fair(e) {
                format!("{:.0}%", 100.0 * chance.fair)
            } else {
                "-".to_owned()
            },
            steps.map_or("-".to_owned(), short),
            t.mean_seconds()
                .map_or("-".to_owned(), |s| format!("{:.1}", s / 60.0)),
        );
    }
    let sight = sight_gaps(&r, &all);
    if !sight.is_empty() {
        let _ = writeln!(out, "\nseeing, Elo over the hidden search at its budget:");
        for line in sight {
            let _ = writeln!(out, "  {line}");
        }
    }
    let climbs = climbs(c, &r, &all);
    if !climbs.is_empty() {
        let _ = writeln!(out, "\nElo per ×3 budget:");
        for (f, text) in climbs {
            let _ = writeln!(out, "  {f:<width$} {text}");
        }
    }
    let crashed: Vec<String> = games
        .iter()
        .filter(|g| g.outcome == Verdict::Crashed)
        .map(|g| {
            format!(
                "{} (seed {}, decision {})",
                g.game,
                g.seed,
                g.crashed_at.unwrap_or_default()
            )
        })
        .collect();
    if !crashed.is_empty() {
        let _ = writeln!(out, "\ncrashed: {}", crashed.join(", "));
    }
    out
}

/// Each of `all`'s chances of being the best among those that have scored a game: none for
/// one that has not, whose rating is the prior's.
pub fn best(r: &Ratings, all: &[String], seed: usize) -> Vec<Chances> {
    let scored: Vec<String> = all
        .iter()
        .filter(|e| r.tally(e).scored() > 0)
        .cloned()
        .collect();
    let chances = r.best(&scored, seed as u64);
    all.iter()
        .map(|e| {
            scored
                .iter()
                .position(|s| s == e)
                .map_or_else(Chances::default, |i| chances[i])
        })
        .collect()
}

/// For every hidden search with a played twin that sees more at its budget: `ismcts18
/// rollout tuned @3M: hands +40 ±60, omniscient +110 ±70`.
pub fn sight_gaps(r: &Ratings, all: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for e in all {
        let Some((f, b)) = e.rsplit_once('@') else {
            continue;
        };
        let Ok(hidden) = f.parse::<Family>() else {
            continue;
        };
        if hidden.sight != Sight::Hidden || r.tally(e).scored() == 0 {
            continue;
        }
        let mut parts = Vec::new();
        for (sight, word) in [(Sight::Hands, "hands"), (Sight::Omniscient, "omniscient")] {
            let twin = format!("{}@{b}", Family { sight, ..hidden });
            if r.tally(&twin).scored() > 0 {
                let (d, s) = r.diff(&twin, e);
                parts.push(format!("{word} {d:+.0} ±{:.0}", 1.96 * s));
            }
        }
        if !parts.is_empty() {
            out.push(format!("{e}: {}", parts.join(", ")));
        }
    }
    out
}

/// Each family with two budgets played, and its Elo per tripling of the budget.
pub fn climbs(c: &Config, r: &Ratings, all: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in &c.families {
        let points: Vec<(u64, f64, f64)> = all
            .iter()
            .filter(|e| family_of(e) == f && r.tally(e).scored() > 0)
            .filter_map(|e| {
                let (m, s) = r.get(e);
                Some((budget_of(e)?, m, s))
            })
            .collect();
        if let Some(slope) = climb(&points) {
            let budgets: Vec<u64> = points.iter().map(|p| p.0).collect();
            let (lo, hi) = (
                budgets.iter().min().copied().unwrap_or(0),
                budgets.iter().max().copied().unwrap_or(0),
            );
            out.push((
                f.clone(),
                format!(
                    "{}  ({}–{})",
                    climb_text(slope),
                    rung_name(lo),
                    rung_name(hi)
                ),
            ));
        }
    }
    out
}

/// A count, short: 950, 1.5k, 48k, 2.1M.
pub fn short(n: f64) -> String {
    if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e4 {
        format!("{:.0}k", n / 1e3)
    } else if n >= 1e3 {
        format!("{:.1}k", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::longthink::schedule::Matchup;

    fn config() -> Config {
        Config {
            families: vec![
                "ismcts2/hidden/rollout/tuned".to_owned(),
                "ismcts2/omniscient/rollout/tuned".to_owned(),
            ],
            fixed: vec!["expert".to_owned()],
            anchor: "expert".to_owned(),
            budgets: vec![1_000, 3_000],
            threads: 1,
            seed: 0,
        }
    }

    fn games(m: &Matchup, n: u32, wins: u32) -> Vec<GameRecord> {
        (0..n)
            .map(|g| {
                let v = if g < wins {
                    Verdict::Win
                } else {
                    Verdict::Loss
                };
                GameRecord::example(m, g, v)
            })
            .collect()
    }

    /// The table ranks the entrants, and the omniscient twin's edge and the hidden family's
    /// climb are reported.
    #[test]
    fn the_summary_ranks_and_compares() {
        let c = config();
        let hidden1 = "ismcts2/hidden/rollout/tuned@1k";
        let hidden3 = "ismcts2/hidden/rollout/tuned@3k";
        let omni1 = "ismcts2/omniscient/rollout/tuned@1k";
        let mut g = Vec::new();
        // a is "expert" in each: the searches win 25%, 75% and 90% of their games.
        g.extend(games(&Matchup::new("expert", hidden1), 40, 30));
        g.extend(games(&Matchup::new("expert", hidden3), 40, 10));
        g.extend(games(&Matchup::new("expert", omni1), 40, 4));
        let text = text("run-001", &c, &g, &HashMap::new());
        let rows: Vec<&str> = text.lines().skip(3).take_while(|l| !l.is_empty()).collect();
        let order: Vec<&str> = rows
            .iter()
            .map(|l| l.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(order[..4], [omni1, hidden3, "expert", hidden1], "{text}");
        assert!(text.contains(&format!("{hidden1}: omniscient +")), "{text}");
        let climb = text
            .lines()
            .find(|l| l.trim_start().starts_with("ismcts2/hidden/rollout/tuned "))
            .expect(&text);
        assert!(climb.contains(" +") && climb.ends_with("(1k–3k)"), "{text}");
        let unplayed = text.lines().find(|l| l.contains("@9k")).expect(&text);
        assert_eq!(unplayed.split_whitespace().nth(1), Some("-"), "{text}");
        assert_eq!(short(1_500.0), "1.5k");
    }
}
